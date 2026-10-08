//! In-memory phone backend with HTTP-shaped semantics.
//!
//! Used for UI-first development until the real Android service exists, and
//! afterwards as a deterministic test double. Behavior mirrors the phone API:
//! token guards (401), traversal guards (400), missing paths (404),
//! conflicts (409), copy de-duplication, and a rolling request log.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::domain::error::DeviceError;
use crate::domain::log::{HttpLogEntry, HttpMethod};
use crate::domain::view::{SortSpec, sort_entries};
use crate::domain::{ColorTag, Device, Entry};
use crate::fs::paths::{STORAGE_ROOT, is_descendant, parent_of, resolve};
use crate::fs::seed;
use crate::port::DeviceBackend;

/// Request-log capacity (matches the web prototype's 80-line ring buffer).
const MAX_LOGS: usize = 80;

/// Minimum token length accepted by the mock (web prototype parity).
const MIN_TOKEN_LEN: usize = 8;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub struct MockDevice {
    /// Storage is behind a mutex because the [`DeviceBackend`] contract is
    /// `&self` (shared by the UI); the HTTP adapter will be stateless instead.
    entries: Mutex<BTreeMap<String, Entry>>,
    info: Device,
    logs: Mutex<Vec<HttpLogEntry>>,
    next_log_id: AtomicU64,
    tick: AtomicU64,
}

impl MockDevice {
    /// Creates a mock seeded with the reference dataset anchored at `now_ms`.
    pub fn new(now_ms: i64) -> Self {
        Self {
            entries: Mutex::new(Self::seed_map(now_ms)),
            info: seed::mock_devices()
                .into_iter()
                .next()
                .expect("seed provides at least one mock device"),
            logs: Mutex::new(Vec::new()),
            next_log_id: AtomicU64::new(1),
            tick: AtomicU64::new(0),
        }
    }

    /// An empty backend: just the storage root, nothing inside — the
    /// offline placeholder until a phone pairs (or while reconnecting).
    pub fn empty(now_ms: i64) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::from([(
                STORAGE_ROOT.to_string(),
                Entry {
                    name: "0".into(),
                    path: STORAGE_ROOT.into(),
                    dir: true,
                    size: 4096,
                    mtime: now_ms,
                    mime_type: None,
                    extension: None,
                    item_count: Some(0),
                    is_pinned: false,
                    color_tag: None,
                    content: None,
                },
            )])),
            info: seed::mock_devices()
                .into_iter()
                .next()
                .expect("seed provides at least one mock device"),
            logs: Mutex::new(Vec::new()),
            next_log_id: AtomicU64::new(1),
            tick: AtomicU64::new(0),
        }
    }

    /// Overrides the reported device info.
    pub fn with_device(mut self, info: Device) -> Self {
        self.info = info;
        self
    }

    /// Restores the seeded dataset (the debug "reset storage" action).
    pub fn reset(&mut self, now_ms: i64) {
        *self.entries.lock().expect("mock entries mutex") = Self::seed_map(now_ms);
        self.log(HttpMethod::Post, "/debug/reset".into(), 200, None);
    }

    /// Snapshot of the emulated request log, newest first.
    pub fn logs(&self) -> Vec<HttpLogEntry> {
        self.logs.lock().expect("mock log mutex").clone()
    }

    fn seed_map(now_ms: i64) -> BTreeMap<String, Entry> {
        seed::seed_entries(now_ms)
            .into_iter()
            .map(|e| (e.path.clone(), e))
            .collect()
    }

    fn log(&self, method: HttpMethod, endpoint: String, status: u16, bytes: Option<u64>) {
        let id = self.next_log_id.fetch_add(1, AtomicOrdering::Relaxed);
        let tick = self.tick.fetch_add(1, AtomicOrdering::Relaxed);
        // Deterministic pseudo-latency so the log looks alive without a clock.
        let duration_ms = 8 + (tick * 7 + endpoint.len() as u64 * 3) % 72;
        let mut logs = self.logs.lock().expect("mock log mutex");
        logs.insert(
            0,
            HttpLogEntry {
                id,
                timestamp: now_ms(),
                method,
                endpoint,
                status,
                duration_ms,
                bytes,
            },
        );
        logs.truncate(MAX_LOGS);
    }

    fn authorize(
        &self,
        token: &str,
        method: HttpMethod,
        endpoint: &str,
    ) -> Result<(), DeviceError> {
        if token.len() < MIN_TOKEN_LEN {
            self.log(method, endpoint.to_string(), 401, None);
            Err(DeviceError::Unauthorized)
        } else {
            Ok(())
        }
    }

    /// Canonicalizes `path`, logging a 400 on rejection.
    fn resolve_or_log(
        &self,
        path: &str,
        method: HttpMethod,
        endpoint: &str,
    ) -> Result<String, DeviceError> {
        match resolve(path) {
            Ok(p) => Ok(p),
            Err(e) => {
                self.log(method, endpoint.to_string(), e.status(), None);
                Err(e)
            }
        }
    }
}

/// Deterministic filler standing in for binary mock content so
/// `read_range` can serve `size` bytes without storing them.
const BINARY_FILLER: &[u8] = b"Android18 mock binary filler block.\n";

/// The `read_range` window of a mock file: text entries serve their content
/// bytes; binary entries serve `size` bytes of filler (never materialized).
fn window_of(entry: &Entry, start: u64, max_len: u64) -> Vec<u8> {
    match &entry.content {
        Some(content) => {
            let bytes = content.as_bytes();
            let end = start.saturating_add(max_len).min(bytes.len() as u64);
            if start >= end {
                Vec::new()
            } else {
                bytes[start as usize..end as usize].to_vec()
            }
        }
        None => {
            let end = start.saturating_add(max_len).min(entry.size);
            if start >= end {
                Vec::new()
            } else {
                (start..end)
                    .map(|i| BINARY_FILLER[(i % BINARY_FILLER.len() as u64) as usize])
                    .collect()
            }
        }
    }
}

/// Direct children of `parent` (dirs first, name ascending), with fresh
/// directory item counts.
fn children_of(entries: &BTreeMap<String, Entry>, parent: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = entries
        .values()
        .filter(|e| parent_of(&e.path).is_some_and(|p| p == parent))
        .cloned()
        .collect();
    for e in out.iter_mut() {
        if e.dir {
            let count = entries
                .values()
                .filter(|c| parent_of(&c.path).is_some_and(|p| p == e.path))
                .count() as u64;
            e.item_count = Some(count);
        }
    }
    sort_entries(&mut out, &SortSpec::default());
    out
}

/// `name (copy N)` de-duplication ported from the web prototype: files
/// keep their extension, directories are copied wholesale.
fn dedupe_name(entries: &BTreeMap<String, Entry>, source: &Entry, folder: &str) -> String {
    let direct = format!("{}/{}", folder, source.name);
    if !entries.contains_key(&direct) {
        return source.name.clone();
    }
    let ext = source
        .extension
        .as_ref()
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let base = if ext.is_empty() {
        source.name.clone()
    } else {
        source
            .name
            .strip_suffix(ext.as_str())
            .unwrap_or(&source.name)
            .to_string()
    };
    let mut n = 1;
    loop {
        let candidate = if source.dir {
            format!("{} (copy {n})", source.name)
        } else {
            format!("{base} (copy {n}){ext}")
        };
        if !entries.contains_key(&format!("{folder}/{candidate}")) {
            return candidate;
        }
        n += 1;
    }
}

#[async_trait]
impl DeviceBackend for MockDevice {
    /// Serves the Phone Server panel from the rolling mock log.
    fn request_log(&self) -> Vec<HttpLogEntry> {
        self.logs()
    }

    async fn list(&self, path: &str, token: &str) -> Result<Vec<Entry>, DeviceError> {
        let endpoint = format!("/list?path={path}");
        self.authorize(token, HttpMethod::Get, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Get, &endpoint)?;
        let result = {
            let entries = self.entries.lock().expect("mock entries mutex");
            match entries.get(&resolved) {
                Some(e) if !e.dir => Err(DeviceError::BadRequest(format!(
                    "`{path}` is not a directory"
                ))),
                None => Err(DeviceError::NotFound(format!(
                    "directory `{path}` does not exist"
                ))),
                _ => Ok(children_of(&entries, &resolved)),
            }
        };
        match result {
            Ok(children) => {
                self.log(HttpMethod::Get, endpoint, 200, None);
                Ok(children)
            }
            Err(e) => {
                self.log(HttpMethod::Get, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn read_text(&self, path: &str, token: &str) -> Result<String, DeviceError> {
        let endpoint = format!("/file?path={path}");
        self.authorize(token, HttpMethod::Get, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Get, &endpoint)?;
        let result = {
            let entries = self.entries.lock().expect("mock entries mutex");
            match entries.get(&resolved) {
                None => Err(DeviceError::NotFound(format!("`{path}` does not exist"))),
                Some(e) if e.dir => {
                    Err(DeviceError::BadRequest(format!("`{path}` is a directory")))
                }
                Some(e) => Ok(e
                    .content
                    .clone()
                    .unwrap_or_else(|| format!("[Binary File: {} bytes]", e.size))),
            }
        };
        match result {
            Ok(text) => {
                let size = {
                    let entries = self.entries.lock().expect("mock entries mutex");
                    entries.get(&resolved).map(|e| e.size)
                };
                self.log(HttpMethod::Get, endpoint, 200, size);
                Ok(text)
            }
            Err(e) => {
                self.log(HttpMethod::Get, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn read_range(
        &self,
        path: &str,
        start: u64,
        max_len: u64,
        token: &str,
    ) -> Result<Vec<u8>, DeviceError> {
        let endpoint = format!("/download?path={path}");
        self.authorize(token, HttpMethod::Get, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Get, &endpoint)?;
        let result = {
            let entries = self.entries.lock().expect("mock entries mutex");
            match entries.get(&resolved) {
                None => Err(DeviceError::NotFound(format!("`{path}` does not exist"))),
                Some(e) if e.dir => {
                    Err(DeviceError::BadRequest(format!("`{path}` is a directory")))
                }
                Some(e) => Ok(window_of(e, start, max_len)),
            }
        };
        match result {
            Ok(bytes) => {
                self.log(HttpMethod::Get, endpoint, 200, Some(bytes.len() as u64));
                Ok(bytes)
            }
            Err(e) => {
                self.log(HttpMethod::Get, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn mkdir(&self, path: &str, token: &str) -> Result<(), DeviceError> {
        let endpoint = format!("/mkdir?path={path}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Post, &endpoint)?;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            if entries.contains_key(&resolved) {
                Err(DeviceError::Conflict(format!("`{path}` already exists")))
            } else {
                entries.insert(
                    resolved.clone(),
                    Entry::dir(&resolved, now_ms(), None, false),
                );
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, None);
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn touch(&self, path: &str, token: &str) -> Result<(), DeviceError> {
        let endpoint = format!("/touch?path={path}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Post, &endpoint)?;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            if entries.contains_key(&resolved) {
                Err(DeviceError::Conflict(format!("`{path}` already exists")))
            } else {
                entries.insert(
                    resolved.clone(),
                    Entry::file(
                        &resolved,
                        0,
                        now_ms(),
                        Some("text/plain"),
                        Some(String::new()),
                    ),
                );
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, None);
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn remove(&self, path: &str, token: &str) -> Result<(), DeviceError> {
        let endpoint = format!("/rm?path={path}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Post, &endpoint)?;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            if resolved == STORAGE_ROOT {
                Err(DeviceError::BadRequest(
                    "cannot remove the storage root".into(),
                ))
            } else if !entries.contains_key(&resolved) {
                Err(DeviceError::NotFound(format!("`{path}` does not exist")))
            } else {
                let doomed: Vec<String> = entries
                    .keys()
                    .filter(|k| k.as_str() == resolved || is_descendant(k, &resolved))
                    .cloned()
                    .collect();
                for key in doomed {
                    entries.remove(&key);
                }
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, None);
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn mv(&self, from: &str, to: &str, token: &str) -> Result<(), DeviceError> {
        let endpoint = format!("/mv?from={from}&to={to}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved_from = self.resolve_or_log(from, HttpMethod::Post, &endpoint)?;
        let mut resolved_to = self.resolve_or_log(to, HttpMethod::Post, &endpoint)?;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            match entries.get(&resolved_from).cloned() {
                None => Err(DeviceError::NotFound(format!("`{from}` does not exist"))),
                Some(source) => {
                    // Files get the "destination is a directory" convenience;
                    // a directory must land on a free path (prototype parity).
                    if !source.dir
                        && let Some(dest) = entries.get(&resolved_to)
                        && dest.dir
                    {
                        resolved_to =
                            format!("{}/{}", resolved_to.trim_end_matches('/'), source.name);
                    }
                    if resolved_to != resolved_from {
                        if is_descendant(&resolved_to, &resolved_from) {
                            Err(DeviceError::BadRequest(
                                "cannot move a directory into itself".into(),
                            ))
                        } else if entries.contains_key(&resolved_to) {
                            Err(DeviceError::Conflict("destination already exists".into()))
                        } else {
                            let moved: Vec<(String, Entry)> = entries
                                .iter()
                                .filter(|(k, _)| {
                                    k.as_str() == resolved_from || is_descendant(k, &resolved_from)
                                })
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            let stamp = now_ms();
                            let new_root_name = resolved_to
                                .rsplit('/')
                                .next()
                                .unwrap_or(&resolved_to)
                                .to_string();
                            for (old, mut e) in moved {
                                entries.remove(&old);
                                e.path = format!("{}{}", resolved_to, &old[resolved_from.len()..]);
                                if old == resolved_from {
                                    e.name = new_root_name.clone();
                                }
                                e.mtime = stamp;
                                entries.insert(e.path.clone(), e);
                            }
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, None);
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn cp(&self, from: &str, to_folder: &str, token: &str) -> Result<(), DeviceError> {
        let endpoint = format!("/cp?from={from}&to={to_folder}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved_from = self.resolve_or_log(from, HttpMethod::Post, &endpoint)?;
        let folder = self.resolve_or_log(to_folder, HttpMethod::Post, &endpoint)?;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            match entries.get(&resolved_from).cloned() {
                None => Err(DeviceError::NotFound(format!("`{from}` does not exist"))),
                Some(source) => {
                    if source.dir
                        && folder != resolved_from
                        && is_descendant(&folder, &resolved_from)
                    {
                        Err(DeviceError::BadRequest(
                            "cannot copy a directory into itself".into(),
                        ))
                    } else {
                        let target_name = dedupe_name(&entries, &source, &folder);
                        let target_path = format!("{folder}/{target_name}");
                        let stamp = now_ms();
                        let mut root_copy = source.clone();
                        root_copy.name = target_name;
                        root_copy.path = target_path.clone();
                        root_copy.mtime = stamp;
                        entries.insert(root_copy.path.clone(), root_copy);
                        if source.dir {
                            let subtree: Vec<(String, Entry)> = entries
                                .iter()
                                .filter(|(k, _)| is_descendant(k, &resolved_from))
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (old, mut e) in subtree {
                                e.path = format!("{}{}", target_path, &old[resolved_from.len()..]);
                                e.mtime = stamp;
                                entries.insert(e.path.clone(), e);
                            }
                        }
                        Ok(())
                    }
                }
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, None);
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn upload(
        &self,
        dest_folder: &str,
        name: &str,
        size: u64,
        mime_type: Option<&str>,
        content: Option<String>,
        token: &str,
    ) -> Result<(), DeviceError> {
        let endpoint = format!("/upload/{dest_folder}/{name}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let folder = self.resolve_or_log(dest_folder, HttpMethod::Post, &endpoint)?;
        let result = if name.trim().is_empty() {
            Err(DeviceError::BadRequest(
                "file name must not be empty".into(),
            ))
        } else {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            let dest = format!("{folder}/{}", name.trim());
            let entry = Entry::file(&dest, size, now_ms(), mime_type, content);
            entries.insert(dest, entry);
            Ok(())
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, Some(size));
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn upload_chunk(
        &self,
        path: &str,
        offset: u64,
        chunk: Vec<u8>,
        token: &str,
    ) -> Result<(), DeviceError> {
        let endpoint = format!("/upload?path={path}&offset={offset}");
        self.authorize(token, HttpMethod::Post, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Post, &endpoint)?;
        let appended = chunk.len() as u64;
        let result = {
            let mut entries = self.entries.lock().expect("mock entries mutex");
            let existing = entries.get(&resolved);
            let existing_len = match existing {
                // Mock file bodies are UTF-8 text; binary chunks fall back to
                // a lossy join (the HTTP adapter carries real bytes).
                Some(entry) if !entry.dir => entry
                    .content
                    .as_ref()
                    .map(|c| c.len() as u64)
                    // Generated filler has a length but no representable
                    // body; appending to it is refused rather than faked.
                    .unwrap_or(entry.size),
                Some(_) => Err(DeviceError::BadRequest(format!("`{path}` is a directory")))?,
                None => 0,
            };
            if existing_len != offset {
                Err(DeviceError::Conflict(format!(
                    "offset {offset} does not match current length {existing_len}"
                )))
            } else if existing.is_some_and(|e| e.content.is_none() && e.size > 0) {
                Err(DeviceError::Conflict(
                    "cannot append to generated filler content".into(),
                ))
            } else {
                let joined = existing.and_then(|e| e.content.clone()).unwrap_or_default();
                let merged = format!("{joined}{}", String::from_utf8_lossy(&chunk));
                let size = merged.len() as u64;
                let mime = existing.and_then(|e| e.mime_type.clone());
                entries.insert(
                    resolved,
                    Entry::file(path, size, now_ms(), mime.as_deref(), Some(merged)),
                );
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                self.log(HttpMethod::Post, endpoint, 200, Some(appended));
                Ok(())
            }
            Err(e) => {
                self.log(HttpMethod::Post, endpoint, e.status(), None);
                Err(e)
            }
        }
    }

    async fn thumb(&self, path: &str, _max_dim: u32, token: &str) -> Result<Vec<u8>, DeviceError> {
        let endpoint = format!("/thumb?path={path}");
        self.authorize(token, HttpMethod::Get, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Get, &endpoint)?;
        // The mock has no bitmap pipeline; it reports the phone's 404 shape.
        let error = DeviceError::NotFound(format!("no preview for `{resolved}`"));
        self.log(HttpMethod::Get, endpoint, error.status(), None);
        Err(error)
    }

    async fn walk(&self, path: &str, token: &str) -> Result<Vec<Entry>, DeviceError> {
        let endpoint = format!("/walk?path={path}");
        self.authorize(token, HttpMethod::Get, &endpoint)?;
        let resolved = self.resolve_or_log(path, HttpMethod::Get, &endpoint)?;
        let entries = self.entries.lock().expect("mock entries mutex");
        let mut out: Vec<Entry> = entries
            .values()
            .filter(|e| e.path != resolved && is_descendant(&e.path, &resolved))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        let mut entries = self.entries.lock().expect("mock entries mutex");
        match entries.get_mut(&resolved) {
            Some(e) => {
                e.is_pinned = pinned;
                Ok(())
            }
            None => Err(DeviceError::NotFound(format!("`{path}` does not exist"))),
        }
    }

    async fn set_color_tag(&self, path: &str, tag: Option<ColorTag>) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        let mut entries = self.entries.lock().expect("mock entries mutex");
        match entries.get_mut(&resolved) {
            Some(e) => {
                e.color_tag = tag;
                Ok(())
            }
            None => Err(DeviceError::NotFound(format!("`{path}` does not exist"))),
        }
    }

    async fn device_info(&self) -> Result<Device, DeviceError> {
        Ok(self.info.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    const TOKEN: &str = "7f9c2d1b84e035a6bc8910fedcba4321";
    const NOW: i64 = 1_772_000_000_000;

    fn device() -> MockDevice {
        MockDevice::new(NOW)
    }

    #[test]
    fn empty_backend_is_a_bare_root() {
        let d = MockDevice::empty(NOW);
        // Root lists empty, walk (descendants only) is empty too, and
        // mutations still work — new content shows up.
        assert!(block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap().is_empty());
        assert!(block_on(d.walk(STORAGE_ROOT, TOKEN)).unwrap().is_empty());
        assert!(block_on(d.mkdir(&format!("{STORAGE_ROOT}/New"), TOKEN)).is_ok());
        let listed = block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "New");
    }

    #[test]
    fn read_range_windows_binary_filler_to_size() {
        let d = device();
        let size = 1_000u64;
        block_on(d.upload(STORAGE_ROOT, "blob.bin", size, None, None, TOKEN)).unwrap();
        let path = format!("{STORAGE_ROOT}/blob.bin");
        let mid = block_on(d.read_range(&path, 200, 100, TOKEN)).unwrap();
        assert_eq!(mid.len(), 100);
        assert_eq!(
            mid[0],
            BINARY_FILLER[(200 % BINARY_FILLER.len() as u64) as usize]
        );
        let tail = block_on(d.read_range(&path, 900, 500, TOKEN)).unwrap();
        assert_eq!(tail.len(), 100);
        assert!(
            block_on(d.read_range(&path, size, 10, TOKEN))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn read_range_slices_text_content() {
        let d = device();
        block_on(d.upload(
            STORAGE_ROOT,
            "win.txt",
            16,
            None,
            Some("0123456789abcdef".into()),
            TOKEN,
        ))
        .unwrap();
        let path = format!("{STORAGE_ROOT}/win.txt");
        assert_eq!(
            block_on(d.read_range(&path, 4, 3, TOKEN)).unwrap(),
            b"456".to_vec()
        );
        assert_eq!(
            block_on(d.read_range(&path, 14, 100, TOKEN)).unwrap(),
            b"ef".to_vec()
        );
        assert!(
            block_on(d.read_range(&path, 16, 5, TOKEN))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn read_range_rejects_dirs_missing_and_bad_tokens() {
        let d = device();
        assert!(matches!(
            block_on(d.read_range(STORAGE_ROOT, 0, 10, TOKEN)).unwrap_err(),
            DeviceError::BadRequest(_)
        ));
        assert!(matches!(
            block_on(d.read_range(&format!("{STORAGE_ROOT}/ghost.bin"), 0, 10, TOKEN)).unwrap_err(),
            DeviceError::NotFound(_)
        ));
        assert_eq!(
            block_on(d.read_range(STORAGE_ROOT, 0, 1, "short")).unwrap_err(),
            DeviceError::Unauthorized
        );
    }

    #[test]
    fn list_root_returns_dirs_first_with_item_counts() {
        let d = device();
        let out = block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap();
        let first_file = out.iter().position(|e| !e.dir).expect("has files");
        assert!(out[..first_file].iter().all(|e| e.dir));
        let dcim = out.iter().find(|e| e.name == "DCIM").unwrap();
        assert_eq!(dcim.item_count, Some(2)); // Camera + Screenshots
    }

    #[test]
    fn short_token_is_unauthorized() {
        let d = device();
        assert_eq!(
            block_on(d.list(STORAGE_ROOT, "short")).unwrap_err(),
            DeviceError::Unauthorized
        );
        assert!(
            d.logs()
                .first()
                .is_some_and(|l| l.status == 401 && l.method == HttpMethod::Get)
        );
    }

    #[test]
    fn traversal_rejected_as_bad_request() {
        let d = device();
        assert!(matches!(
            block_on(d.list("/storage/emulated/0/../../etc", TOKEN)).unwrap_err(),
            DeviceError::BadRequest(_)
        ));
    }

    #[test]
    fn mkdir_conflicts_and_creates() {
        let d = device();
        assert!(matches!(
            block_on(d.mkdir(&format!("{STORAGE_ROOT}/DCIM"), TOKEN)).unwrap_err(),
            DeviceError::Conflict(_)
        ));
        block_on(d.mkdir(&format!("{STORAGE_ROOT}/NewFolder"), TOKEN)).unwrap();
        let out = block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap();
        assert!(out.iter().any(|e| e.name == "NewFolder"));
    }

    #[test]
    fn mv_into_directory_moves_with_original_name() {
        let d = device();
        block_on(d.mv(
            &format!("{STORAGE_ROOT}/system_log.txt"),
            &format!("{STORAGE_ROOT}/DCIM"),
            TOKEN,
        ))
        .unwrap();
        let dcim = block_on(d.list(&format!("{STORAGE_ROOT}/DCIM"), TOKEN)).unwrap();
        assert!(dcim.iter().any(|e| e.name == "system_log.txt"));
        assert!(
            !block_on(d.list(STORAGE_ROOT, TOKEN))
                .unwrap()
                .iter()
                .any(|e| e.name == "system_log.txt")
        );
    }

    #[test]
    fn mv_rejects_conflict_and_self_move() {
        let d = device();
        assert!(matches!(
            block_on(d.mv(
                &format!("{STORAGE_ROOT}/DCIM"),
                &format!("{STORAGE_ROOT}/Download"),
                TOKEN
            ))
            .unwrap_err(),
            DeviceError::Conflict(_)
        ));
        assert!(matches!(
            block_on(d.mv(
                &format!("{STORAGE_ROOT}/DCIM"),
                &format!("{STORAGE_ROOT}/DCIM/Camera"),
                TOKEN
            ))
            .unwrap_err(),
            DeviceError::BadRequest(_)
        ));
    }

    #[test]
    fn cp_dedupes_with_extension_aware_names() {
        let d = device();
        let from = format!("{STORAGE_ROOT}/system_log.txt");
        for expected in ["system_log (copy 1).txt", "system_log (copy 2).txt"] {
            block_on(d.cp(&from, STORAGE_ROOT, TOKEN)).unwrap();
            let root = block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap();
            assert!(
                root.iter().any(|e| e.name == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn rm_is_recursive() {
        let d = device();
        block_on(d.remove(&format!("{STORAGE_ROOT}/DCIM"), TOKEN)).unwrap();
        assert!(matches!(
            block_on(d.list(&format!("{STORAGE_ROOT}/DCIM"), TOKEN)).unwrap_err(),
            DeviceError::NotFound(_)
        ));
    }

    #[test]
    fn upload_creates_entry_and_read_text_round_trips() {
        let d = device();
        block_on(d.upload(
            STORAGE_ROOT,
            "hello.txt",
            5,
            Some("text/plain"),
            Some("hello".into()),
            TOKEN,
        ))
        .unwrap();
        assert_eq!(
            block_on(d.read_text(&format!("{STORAGE_ROOT}/hello.txt"), TOKEN)).unwrap(),
            "hello"
        );
        assert_eq!(
            block_on(d.read_text(
                &format!("{STORAGE_ROOT}/DCIM/Camera/IMG_20261001_143022.jpg"),
                TOKEN
            ))
            .unwrap(),
            "[Binary File: 4210800 bytes]"
        );
    }

    #[test]
    fn upload_chunk_appends_only_at_the_current_length() {
        let d = device();
        let path = format!("{STORAGE_ROOT}/notes.txt");
        block_on(d.upload_chunk(&path, 0, b"hello ".to_vec(), TOKEN)).unwrap();
        block_on(d.upload_chunk(&path, 6, b"world".to_vec(), TOKEN)).unwrap();
        assert_eq!(block_on(d.read_text(&path, TOKEN)).unwrap(), "hello world");
        // An offset that does not match the current length is a conflict…
        assert!(matches!(
            block_on(d.upload_chunk(&path, 3, b"!!!".to_vec(), TOKEN)),
            Err(DeviceError::Conflict(_))
        ));
        // …and a short token never reaches storage.
        assert!(matches!(
            block_on(d.upload_chunk(&path, 11, b"!".to_vec(), "short")),
            Err(DeviceError::Unauthorized)
        ));
        // Empty chunk at offset 0 creates an empty file.
        let empty = format!("{STORAGE_ROOT}/empty.txt");
        block_on(d.upload_chunk(&empty, 0, Vec::new(), TOKEN)).unwrap();
        assert_eq!(block_on(d.read_text(&empty, TOKEN)).unwrap(), "");
    }

    #[test]
    fn walk_enumerates_every_descendant() {
        let d = device();
        let all = block_on(d.walk(STORAGE_ROOT, TOKEN)).unwrap();
        assert!(all.len() > 25);
        assert!(!all.iter().any(|e| e.path == STORAGE_ROOT));
        assert!(
            all.iter()
                .any(|e| e.path.ends_with("Product_Roadmap_Q4.md"))
        );
    }

    #[test]
    fn metadata_updates_apply() {
        let d = device();
        block_on(d.set_pinned(&format!("{STORAGE_ROOT}/Download"), true)).unwrap();
        block_on(d.set_color_tag(&format!("{STORAGE_ROOT}/Podcasts"), Some(ColorTag::Rose)))
            .unwrap();
        let root = block_on(d.list(STORAGE_ROOT, TOKEN)).unwrap();
        assert!(
            root.iter()
                .find(|e| e.name == "Download")
                .unwrap()
                .is_pinned
        );
        assert_eq!(
            root.iter()
                .find(|e| e.name == "Podcasts")
                .unwrap()
                .color_tag,
            Some(ColorTag::Rose)
        );
    }
}
