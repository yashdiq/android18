//! Semantic file search: a pluggable provider port plus an offline
//! heuristic implementation ported from the web prototype's server-side
//! fallback.

mod heuristic;

pub use heuristic::{HeuristicSearchProvider, heuristic_search};

pub use crate::port::{Confidence, SearchError, SearchMatch, SearchProvider, SearchResult};
