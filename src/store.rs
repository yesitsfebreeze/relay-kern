use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::base::graph::GraphGnn;
use crate::base::locks::read_recovered;
use crate::config::Config;
use crate::ingest::{LlmFunc as IngestLlmFunc, Worker};
use crate::llm::Client as LlmClient;
use crate::tick::queue::Queue;
use crate::tick::tasks::{BroadcastQuestionFunc, EmbedFunc, LlmFunc as TickLlmFunc};

pub type StoreKey = PathBuf;

pub struct StoreEntry {
    pub key: StoreKey,
    pub graph: Arc<RwLock<GraphGnn>>,
    pub worker: Arc<Worker>,
    pub tick_q: Arc<Queue>,
    pub tick_handle: tokio::task::JoinHandle<()>,
    /// Persists this store's graph. Single instance per store — Worker
    /// holds a clone of the same Arc; downstream consumers (mcp::Server,
    /// gossip handlers) should obtain it from `StoreEntry.save_fn.clone()`
    /// rather than building a duplicate closure over the same graph.
    pub save_fn: Arc<dyn Fn() + Send + Sync>,
    pub last_touch: RwLock<Instant>,
}

#[derive(Default)]
pub struct Registry {
    stores: RwLock<HashMap<StoreKey, Arc<StoreEntry>>>,
}

impl Registry {
    pub fn new() -> Self { Self::default() }

    fn canon(p: &Path) -> StoreKey {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
    }

    pub fn get(&self, data_dir: &Path) -> Option<Arc<StoreEntry>> {
        self.stores.read().unwrap().get(&Self::canon(data_dir)).cloned()
    }

    pub fn len(&self) -> usize { self.stores.read().unwrap().len() }

    pub fn open(
        &self,
        data_dir: &Path,
        cfg: &Config,
        llm_client: LlmClient,
        ingest_llm: Option<IngestLlmFunc>,
        tick_llm: Option<TickLlmFunc>,
        tick_embed: Option<EmbedFunc>,
        broadcast_q: Option<BroadcastQuestionFunc>,
    ) -> Arc<StoreEntry> {
        let key = Self::canon(data_dir);
        if let Some(e) = self.stores.read().unwrap().get(&key) {
            *e.last_touch.write().unwrap() = Instant::now();
            return e.clone();
        }

        let mut store_cfg = cfg.clone();
        store_cfg.data_dir = data_dir.to_string_lossy().into_owned();

        let graph = Arc::new(RwLock::new(crate::commands::load_graph(&store_cfg)));

        let save_g = graph.clone();
        let save_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let g = read_recovered(&save_g);
            crate::commands::save_graph(&g);
        });

        let worker = Arc::new(Worker::new(
            graph.clone(),
            llm_client,
            ingest_llm,
            Some(save_fn.clone()),
        ));

        let tick_q = Arc::new(Queue::new(cfg.tick.queue_capacity.max(1)));

        let tick_handle = crate::tick::start(
            tick_q.clone(),
            graph.clone(),
            tick_llm,
            tick_embed,
            broadcast_q,
            cfg.gnn.into(),
            cfg.tick,
        );

        crate::tick::enqueue_all(&tick_q, &graph);

        let entry = Arc::new(StoreEntry {
            key: key.clone(),
            graph,
            worker,
            tick_q,
            tick_handle,
            save_fn,
            last_touch: RwLock::new(Instant::now()),
        });

        self.stores.write().unwrap()
            .entry(key)
            .or_insert_with(|| entry.clone())
            .clone()
    }
}
