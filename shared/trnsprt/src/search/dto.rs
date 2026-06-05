//! Data transfer objects for [`SearchSvc`](super::SearchSvc).
//!
//! These are mirror types — they intentionally do NOT depend on the
//! `kern` crate. The kern side translates its richer internal types to
//! and from these on the boundary. Keeping DTOs colocated with the
//! transport keeps the `repl` palette free of any kern dependency.
//!
//! All DTOs derive `serde::{Serialize, Deserialize}` so they can be
//! shuttled by either the line-delimited JSON envelope codec or the
//! length-delimited bincode codec — the codec choice is per-channel.

use serde::{Deserialize, Serialize};

// ---- entity kind / status -------------------------------------------------

/// Lightweight mirror of `kern::EntityKind`.
///
/// The canonical seven-variant enum from the PRD. Held here so the
/// `repl` palette never needs the `kern` crate as a build dependency.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum EntityKindLite {
    Fact,
    Claim,
    Document,
    Question,
    Answer,
    Conclusion,
    Superseded,
}

/// Lightweight mirror of `kern::EntityStatus` — orthogonal lifecycle flag.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum EntityStatusLite {
    Active,
    Superseded,
}

// ---- edges ----------------------------------------------------------------

/// Mirror of `kern::Reason` kinds. One variant per typed edge in the
/// connected index.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum EdgeKind {
    Answers,
    Supports,
    Contradicts,
    Extends,
    Requires,
    References,
    Derives,
    Instances,
    PartOf,
    Consolidates,
}

// ---- entity reference (search hit) ----------------------------------------

/// One result row delivered to the palette. Cheap to clone; carries
/// only what `Card` needs to render plus the id used to drill in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityRef {
    pub id: String,
    pub kind: EntityKindLite,
    pub status: EntityStatusLite,
    /// URI scheme without the `://` (e.g. `file`, `ticket`, `session`,
    /// `agent`, `inline`). Lets the palette pick the source-glyph
    /// without parsing a full URI.
    pub scheme: String,
    pub label: String,
    /// Short snippet shown under the label; already truncated by the
    /// server.
    pub snippet: String,
    /// Fused score (HNSW + BM25 + PageRank + heat). Higher = better.
    pub score: f32,
}

// ---- search ---------------------------------------------------------------

/// One filter chip applied to a query. `scheme` and `kind` are
/// independently optional so a facet can constrain by either axis (or
/// both, when the user types e.g. `>file !fact`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Facet {
    pub scheme: Option<String>,
    pub kind: Option<EntityKindLite>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchReq {
    pub query: String,
    pub facets: Vec<Facet>,
    pub k: u32,
    /// Monotonic per-keystroke token. Newer tokens supersede older
    /// ones in the server's mock cancellation logic; production
    /// implementations may use it to early-return stale work.
    pub cancel_token: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchRes {
    pub hits: Vec<EntityRef>,
    /// True iff this response was for the most-recent `cancel_token`
    /// the server has seen. The client may discard stale frames.
    pub fresh: bool,
}

// ---- neighbors ------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NeighborsReq {
    pub entity_id: String,
    /// Empty = all edge kinds.
    pub edge_kinds: Vec<EdgeKind>,
    /// Server clamps to `[0, 3]`.
    pub depth: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NeighborsRes {
    pub neighbors: Vec<EntityRef>,
}

// ---- preview --------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreviewReq {
    pub entity_id: String,
}

/// Preview pane payload. The variant carries everything the renderer
/// needs — the palette decides which sub-renderer to dispatch to based
/// on the discriminant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PreviewRes {
    /// File-backed entity. `language` is a tree-sitter grammar id
    /// (`"rust"`, `"python"`, ...) or `None` for plain text.
    File {
        path: String,
        content: String,
        language: Option<String>,
    },
    /// Generic entity body — Fact, Claim, Conclusion, etc.
    Text { content: String },
    /// Reason edge between two entities; rendered as a sentence in the
    /// preview pane.
    Edge {
        from_label: String,
        to_label: String,
        kind: EdgeKind,
        sentence: String,
    },
}

// ---- bincode + serde_json roundtrip smoke ---------------------------------

#[cfg(test)]
mod dto_serde_tests {
    use super::*;

    #[test]
    fn entity_ref_roundtrips_through_serde_json() {
        let original = EntityRef {
            id: "e1".into(),
            kind: EntityKindLite::Fact,
            status: EntityStatusLite::Active,
            scheme: "file".into(),
            label: "main.rs".into(),
            snippet: "fn main() {}".into(),
            score: 0.92,
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: EntityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, original.id);
        assert_eq!(back.kind, original.kind);
        assert_eq!(back.scheme, original.scheme);
        assert!((back.score - original.score).abs() < f32::EPSILON);
    }

    #[test]
    fn entity_ref_roundtrips_through_bincode() {
        let original = EntityRef {
            id: "e2".into(),
            kind: EntityKindLite::Question,
            status: EntityStatusLite::Superseded,
            scheme: "ticket".into(),
            label: "T-9".into(),
            snippet: "why?".into(),
            score: 0.1,
        };
        let cfg = bincode::config::standard();
        let bytes = bincode::serde::encode_to_vec(&original, cfg).unwrap();
        let (back, _): (EntityRef, _) =
            bincode::serde::decode_from_slice(&bytes, cfg).unwrap();
        assert_eq!(back.id, original.id);
        assert_eq!(back.status, original.status);
    }

    #[test]
    fn preview_res_variants_roundtrip_json() {
        let cases = vec![
            PreviewRes::File {
                path: "a.rs".into(),
                content: "x".into(),
                language: Some("rust".into()),
            },
            PreviewRes::Text { content: "claim".into() },
            PreviewRes::Edge {
                from_label: "A".into(),
                to_label: "B".into(),
                kind: EdgeKind::Supports,
                sentence: "A supports B.".into(),
            },
        ];
        for c in cases {
            let s = serde_json::to_string(&c).unwrap();
            let _back: PreviewRes = serde_json::from_str(&s).unwrap();
        }
    }
}
