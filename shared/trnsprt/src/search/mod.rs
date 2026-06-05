//! `SearchSvc` — typed-RPC surface for the relay search palette.
//!
//! Layout:
//! - [`dto`] — wire types ([`SearchReq`], [`PreviewRes`], ...).
//! - [`svc`] — `service!` invocation that emits [`SearchSvc`],
//!   [`SearchSvcClient`], and [`serve_search_svc`].
//! - [`mock`] — in-memory [`MockSearchServer`] for tests and
//!   downstream slices (palette UI, preview pane).
//!
//! Crate-internal `service!` invocation is in [`svc`]; consumers
//! re-export the generated trait/client/serve fn from this module.

pub mod dto;
pub mod mock;
pub mod svc;

pub use dto::{
    EdgeKind, EntityKindLite, EntityRef, EntityStatusLite, Facet, NeighborsReq, NeighborsRes,
    PreviewReq, PreviewRes, SearchReq, SearchRes,
};
pub use mock::MockSearchServer;
pub use svc::{serve_search_svc, SearchSvc, SearchSvcClient};
