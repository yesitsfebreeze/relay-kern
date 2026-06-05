//! Local-socket transport for the kern singleton daemon.
//!
//! Provides:
//! - [`Endpoint::kern`] — resolves the per-user endpoint on disk.
//! - [`UnixStreamAdapter`] (cfg unix) / [`NamedPipeAdapter`] (cfg windows) —
//!   [`Adapter`] impls so existing `Channel<Codec>` callers stay unchanged.
//! - [`LocalListener`] — unified accept loop returning `Box<dyn Adapter>`.
//! - [`bind_kern_listener`] — singleton-aware bind. Returns
//!   [`BindOutcome::AlreadyRunning`] when a live daemon already owns the
//!   endpoint so callers can exit cleanly.
//!
//! This is the foundation under the per-user `kern.sock` singleton:
//! - Unix endpoint: `$XDG_RUNTIME_DIR/kern.sock` (fallback `/tmp/kern-$USER.sock`).
//! - Windows endpoint: `\\.\pipe\kern-<USERNAME>`.

#[cfg(unix)]
use std::path::{Path, PathBuf};

use super::adapter::{Adapter, DynRead, DynWrite};
use super::error::AdapterError;

// ---------- Endpoint resolver ---------------------------------------------

/// Platform-specific endpoint location for a singleton local daemon.
#[derive(Clone, Debug)]
pub enum Endpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    #[cfg(windows)]
    NamedPipe(String),
}

impl Endpoint {
    /// Per-cwd `kern` endpoint.
    ///
    /// The endpoint is scoped to the current working directory (its hash is
    /// folded into the socket/pipe name) so each project gets its own kern
    /// daemon — matching the "one kern per cwd" model. A per-user endpoint
    /// would let the first daemon win for the whole user, so a second project
    /// would silently attach to the first project's graph (cross-project
    /// memory contamination). It also lets multiple daemons coexist on one
    /// host (e.g. for local federation testing).
    pub fn kern() -> Self {
        let tag = cwd_tag();
        #[cfg(unix)]
        {
            let path = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .map(|d| d.join(format!("kern-{tag}.sock")))
                .unwrap_or_else(|| {
                    let user = std::env::var("USER").unwrap_or_else(|_| "default".into());
                    PathBuf::from(format!("/tmp/kern-{user}-{tag}.sock"))
                });
            Endpoint::Unix(path)
        }
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
            Endpoint::NamedPipe(format!(r"\\.\pipe\kern-{user}-{tag}"))
        }
    }

    /// Human-readable identifier for logs and error messages.
    pub fn display(&self) -> String {
        match self {
            #[cfg(unix)]
            Endpoint::Unix(p) => p.display().to_string(),
            #[cfg(windows)]
            Endpoint::NamedPipe(n) => n.clone(),
        }
    }
}

/// Deterministic short tag for the current working directory, used to scope
/// the kern endpoint per cwd. FNV-1a over the canonical path — stable across
/// processes (unlike `DefaultHasher`'s randomized state), so the daemon and
/// any client resolving the endpoint from the same cwd always agree.
fn cwd_tag() -> String {
    let dir = std::env::current_dir().unwrap_or_default();
    let canon = dir.canonicalize().unwrap_or(dir);
    let s = canon.to_string_lossy();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod cwd_tag_tests {
    use super::*;

    #[test]
    fn cwd_tag_is_stable_and_nonempty() {
        let a = cwd_tag();
        let b = cwd_tag();
        assert_eq!(a, b, "same cwd must yield the same tag");
        assert_eq!(a.len(), 16, "tag is 16 hex chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn endpoint_kern_includes_tag() {
        let ep = Endpoint::kern();
        assert!(ep.display().contains(&cwd_tag()), "endpoint scoped by cwd tag");
    }
}

// ---------- Adapters ------------------------------------------------------

#[cfg(unix)]
pub struct UnixStreamAdapter {
    stream: tokio::net::UnixStream,
}

#[cfg(unix)]
impl UnixStreamAdapter {
    pub fn new(stream: tokio::net::UnixStream) -> Self {
        Self { stream }
    }
    pub async fn connect(path: &Path) -> Result<Self, AdapterError> {
        let stream = tokio::net::UnixStream::connect(path).await?;
        Ok(Self { stream })
    }
}

#[cfg(unix)]
impl Adapter for UnixStreamAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        let (r, w) = self.stream.into_split();
        (Box::new(r), Box::new(w))
    }
}

#[cfg(windows)]
pub struct NamedPipeAdapter {
    inner: NamedPipeInner,
}

#[cfg(windows)]
enum NamedPipeInner {
    Server(tokio::net::windows::named_pipe::NamedPipeServer),
    Client(tokio::net::windows::named_pipe::NamedPipeClient),
}

#[cfg(windows)]
impl NamedPipeAdapter {
    pub fn from_server(server: tokio::net::windows::named_pipe::NamedPipeServer) -> Self {
        Self { inner: NamedPipeInner::Server(server) }
    }
    pub async fn connect(pipe_name: &str) -> Result<Self, AdapterError> {
        let client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(pipe_name)?;
        Ok(Self { inner: NamedPipeInner::Client(client) })
    }
}

#[cfg(windows)]
impl Adapter for NamedPipeAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        match self.inner {
            NamedPipeInner::Server(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w))
            }
            NamedPipeInner::Client(c) => {
                let (r, w) = tokio::io::split(c);
                (Box::new(r), Box::new(w))
            }
        }
    }
}

// ---------- Unified local adapter -----------------------------------------

/// Platform-tagged local-socket adapter. Exists so both server (accept)
/// and client (connect) paths return a single concrete type that
/// [`Channel::new`](super::Channel::new) — which is generic over
/// `A: Adapter` — can consume directly.
pub enum LocalAdapter {
    #[cfg(unix)]
    Unix(UnixStreamAdapter),
    #[cfg(windows)]
    NamedPipe(NamedPipeAdapter),
}

impl Adapter for LocalAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        match *self {
            #[cfg(unix)]
            LocalAdapter::Unix(a) => Box::new(a).split(),
            #[cfg(windows)]
            LocalAdapter::NamedPipe(a) => Box::new(a).split(),
        }
    }
}

// ---------- Client connect ------------------------------------------------

/// Connect to a kern singleton at `endpoint`. Returns a [`LocalAdapter`]
/// ready to wrap in a [`Channel`](super::Channel) with any codec.
pub async fn connect_kern(endpoint: &Endpoint) -> Result<LocalAdapter, AdapterError> {
    match endpoint {
        #[cfg(unix)]
        Endpoint::Unix(path) => Ok(LocalAdapter::Unix(UnixStreamAdapter::connect(path).await?)),
        #[cfg(windows)]
        Endpoint::NamedPipe(name) => {
            Ok(LocalAdapter::NamedPipe(NamedPipeAdapter::connect(name).await?))
        }
    }
}

// ---------- Server bind / accept ------------------------------------------

/// Result of [`bind_kern_listener`].
pub enum BindOutcome {
    /// Endpoint bound. Caller now owns the singleton and may [`LocalListener::accept`].
    Bound(LocalListener),
    /// Another live daemon already owns the endpoint. Caller should exit 0.
    AlreadyRunning,
}

#[derive(Debug)]
pub enum BindError {
    Io(std::io::Error),
}

impl From<std::io::Error> for BindError {
    fn from(e: std::io::Error) -> Self {
        BindError::Io(e)
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Io(e) => write!(f, "bind: {e}"),
        }
    }
}

impl std::error::Error for BindError {}

/// Singleton-aware bind. On Unix probes for a live owner before removing
/// a stale socket file; on Windows uses `first_pipe_instance(true)` so
/// the OS itself enforces uniqueness.
pub async fn bind_kern_listener(endpoint: &Endpoint) -> Result<BindOutcome, BindError> {
    match endpoint {
        #[cfg(unix)]
        Endpoint::Unix(path) => {
            match tokio::net::UnixListener::bind(path) {
                Ok(listener) => {
                    return Ok(BindOutcome::Bound(LocalListener {
                        inner: listener,
                        socket_path: path.clone(),
                    }));
                }
                Err(e) if e.kind() != std::io::ErrorKind::AddrInUse => {
                    return Err(e.into());
                }
                Err(_) => {}
            }
            // AddrInUse — probe whether a live daemon owns it.
            if tokio::net::UnixStream::connect(path).await.is_ok() {
                return Ok(BindOutcome::AlreadyRunning);
            }
            // Stale socket file. Remove and retry once.
            let _ = std::fs::remove_file(path);
            let listener = tokio::net::UnixListener::bind(path)?;
            Ok(BindOutcome::Bound(LocalListener {
                inner: listener,
                socket_path: path.clone(),
            }))
        }
        #[cfg(windows)]
        Endpoint::NamedPipe(name) => {
            use tokio::net::windows::named_pipe::ServerOptions;
            match ServerOptions::new()
                .first_pipe_instance(true)
                .create(name)
            {
                Ok(server) => Ok(BindOutcome::Bound(LocalListener {
                    pipe_name: name.clone(),
                    current: Some(server),
                })),
                Err(e)
                    if e.kind() == std::io::ErrorKind::PermissionDenied
                        || e.raw_os_error() == Some(5)    // ERROR_ACCESS_DENIED
                        || e.raw_os_error() == Some(231)  // ERROR_PIPE_BUSY
                =>
                {
                    Ok(BindOutcome::AlreadyRunning)
                }
                Err(e) => Err(e.into()),
            }
        }
    }
}

/// Unified local-socket listener. The Unix path drives a `UnixListener`;
/// the Windows path holds the current `NamedPipeServer` instance and
/// re-creates one per accept.
pub struct LocalListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    current: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl LocalListener {
    pub async fn accept(&mut self) -> Result<LocalAdapter, std::io::Error> {
        #[cfg(unix)]
        {
            let (stream, _peer) = self.inner.accept().await?;
            Ok(LocalAdapter::Unix(UnixStreamAdapter::new(stream)))
        }
        #[cfg(windows)]
        {
            let server = self.current.take().expect("listener uninitialised");
            server.connect().await?;
            // Pre-create the next instance so subsequent accept doesn't race
            // a fast reconnect.
            self.current = Some(
                tokio::net::windows::named_pipe::ServerOptions::new()
                    .create(&self.pipe_name)?,
            );
            Ok(LocalAdapter::NamedPipe(NamedPipeAdapter::from_server(server)))
        }
    }
}

#[cfg(unix)]
impl Drop for LocalListener {
    fn drop(&mut self) {
        // Best-effort cleanup so the next daemon doesn't trip the stale-sock probe.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
