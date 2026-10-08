//! Ports (transport-agnostic contracts) that the UI depends on and that
//! adapters (mock, HTTP-over-Wi-Fi/USB) implement.
//!
//! A transport resolves to exactly one thing: a base URL. `MockDevice` and
//! the future `HttpDevice` are indistinguishable behind [`DeviceBackend`].

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::Device;
use crate::domain::entry::{ColorTag, Entry};
use crate::domain::error::DeviceError;
use crate::domain::log::HttpLogEntry;

/// Shared, thread-safe backend handle (GPUI entities store these).
pub type SharedBackend = Arc<dyn DeviceBackend>;

/// The phone-as-server filesystem API (ARCHITECTURE.md §"HTTP API v1"),
/// expressed as one async trait.
#[async_trait]
pub trait DeviceBackend: Send + Sync + 'static {
    /// `GET /list?path=` — direct children, dirs first, name ascending;
    /// directories get a fresh `item_count`.
    async fn list(&self, path: &str, token: &str) -> Result<Vec<Entry>, DeviceError>;

    /// `GET /file?path=` limited to text-ish content: returns the stored
    /// content or a `[Binary File: N bytes]` placeholder (previews, `cat`).
    async fn read_text(&self, path: &str, token: &str) -> Result<String, DeviceError>;

    /// `GET /download?path=` with `Range: bytes=start-(start+max_len-1)` —
    /// the resumable-download workhorse. Returns at most `max_len` bytes
    /// starting at `start` (fewer near end-of-file, empty at/past EOF).
    /// 404 when missing, 400 when `path` is a directory.
    async fn read_range(
        &self,
        path: &str,
        start: u64,
        max_len: u64,
        token: &str,
    ) -> Result<Vec<u8>, DeviceError>;

    /// `POST /mkdir?path=` — full path of the new directory.
    async fn mkdir(&self, path: &str, token: &str) -> Result<(), DeviceError>;

    /// `POST /touch?path=` — create an empty file.
    async fn touch(&self, path: &str, token: &str) -> Result<(), DeviceError>;

    /// `POST /rm?path=` — recursive delete.
    async fn remove(&self, path: &str, token: &str) -> Result<(), DeviceError>;

    /// `POST /mv?from=&to=` — move/rename; a directory destination means
    /// "move into it".
    async fn mv(&self, from: &str, to: &str, token: &str) -> Result<(), DeviceError>;

    /// `POST /cp?from=&to=` — copy into a destination folder, with
    /// automatic `name (copy N)` de-duplication.
    async fn cp(&self, from: &str, to_folder: &str, token: &str) -> Result<(), DeviceError>;

    /// `POST /upload/{path}` — create a file in `dest_folder`.
    async fn upload(
        &self,
        dest_folder: &str,
        name: &str,
        size: u64,
        mime_type: Option<&str>,
        content: Option<String>,
        token: &str,
    ) -> Result<(), DeviceError>;

    /// `POST /upload?path=&offset=` — append one binary chunk at `offset`
    /// (offset 0 creates/truncates). Live uploads stream as a sequence of
    /// chunks so progress and cancellation work without buffering whole
    /// files; the server rejects an offset that does not match the file's
    /// current length (409).
    async fn upload_chunk(
        &self,
        path: &str,
        offset: u64,
        chunk: Vec<u8>,
        token: &str,
    ) -> Result<(), DeviceError>;

    /// `GET /thumb?path=&max=` — downscaled JPEG preview of an image file
    /// (404 when the phone cannot decode one).
    async fn thumb(&self, path: &str, max_dim: u32, token: &str) -> Result<Vec<u8>, DeviceError>;

    /// Recursively enumerate `path` and everything below it (powers the
    /// dashboard aggregates, AI search scope, and the shell `tree`/`ai`
    /// commands). Not part of the phone wire API v1; the HTTP adapter
    /// implements it as a breadth-first walk.
    async fn walk(&self, path: &str, token: &str) -> Result<Vec<Entry>, DeviceError>;

    /// Desktop-local decoration: pin state for quick access.
    async fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), DeviceError>;

    /// Desktop-local decoration: folder color tag (`None` clears it).
    async fn set_color_tag(&self, path: &str, tag: Option<ColorTag>) -> Result<(), DeviceError>;

    /// Identity/capability info of the connected phone.
    async fn device_info(&self) -> Result<Device, DeviceError>;

    /// Observed request log (newest first) for the Phone Server panel.
    /// Real-HTTP adapters log their own calls; backends without a wire log
    /// keep the empty default.
    fn request_log(&self) -> Vec<HttpLogEntry> {
        Vec::new()
    }
}

/// Confidence reported by a search provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

/// One semantic search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    pub path: String,
    pub reason: String,
    pub confidence: Confidence,
}

/// Result of a semantic search query.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub summary: String,
    /// Engine that produced the hits (`glm-4.5-flash`, `heuristic`, …).
    pub engine: Option<String>,
    /// Set when the AI engine was unavailable and a fallback answered.
    pub warning: Option<String>,
}

/// Errors surfaced by search providers.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SearchError {
    #[error("search provider unavailable: {0}")]
    Unavailable(String),
}

/// Pluggable semantic search (heuristic offline provider now; Gemini-backed
/// provider from roadmap phase R5).
#[async_trait]
pub trait SearchProvider: Send + Sync + 'static {
    async fn search(
        &self,
        query: &str,
        entries: &[Entry],
        current_path: &str,
    ) -> Result<SearchResult, SearchError>;
}

/// Shared search-provider handle.
pub type SharedSearch = Arc<dyn SearchProvider>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_serde_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&Confidence::High).expect("serialize confidence"),
            r#""high""#
        );
        let medium: Confidence =
            serde_json::from_str("\"medium\"").expect("deserialize confidence");
        assert_eq!(medium, Confidence::Medium);
    }
}
