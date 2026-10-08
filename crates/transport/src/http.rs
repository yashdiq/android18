//! HTTP [`DeviceBackend`] — the desktop client's real transport.
//!
//! One instance = one base URL (`http://ip:port`). Requests carry the
//! pairing token as `X-Auth`; responses map 1:1 onto [`DeviceError`]
//! (401 → [`DeviceError::Unauthorized`], 400/404/409 likewise) and every
//! call is appended to an 80-entry ring that feeds the Phone Server panel,
//! exactly like `MockDevice`. Pin/color-tag decorations are desktop-local:
//! the phone never stores them, so the adapter overlays them on listings.
//!
//! The blocking reqwest client must only be driven from plain threads
//! (the app spawns calls via `background_spawn`), never inside a runtime.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use android18_core::domain::entry::ColorTag;
use android18_core::domain::error::DeviceError;
use android18_core::domain::{Device, DeviceStatus, Entry, HttpLogEntry, HttpMethod, Transport};
use android18_core::fs::paths::resolve;
use android18_core::port::DeviceBackend;
use async_trait::async_trait;

/// Request-log ring size (matches `MockDevice`; the panel shows the newest 10).
const MAX_LOGS: usize = 80;
/// Per-request timeout — Wi-Fi round-trips are slow, not infinite.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Set `ANDROID18_LOG=1` to print a stderr wire trace of every device
/// request: method, base URL, endpoint, status, size, duration, and whether
/// an `X-Auth` token was attached (length only — the token itself is never
/// printed). The app itself has no logger; this is the debugging tap.
fn wire_log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ANDROID18_LOG").is_some())
}

/// Derives the desktop-owned transport + port from a base URL: loopback
/// hosts are `adb forward` tunnels (`Usb`), everything else is direct
/// Wi-Fi/LAN (`Wifi`). Malformed or portless URLs degrade to `(Wifi, 0)`
/// — the request already proved the URL dialable, so the label is all
/// that is at stake.
fn endpoint_kind(base_url: &str) -> (Transport, u16) {
    let host_port = base_url
        .strip_prefix("http://")
        .unwrap_or(base_url)
        .split(['/', '?'])
        .next()
        .unwrap_or(base_url);
    match host_port.rsplit_once(':') {
        Some((host, port)) => {
            let transport = if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
                Transport::Usb
            } else {
                Transport::Wifi
            };
            (transport, port.parse().unwrap_or(0))
        }
        None => (Transport::Wifi, 0),
    }
}

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// A live phone-service connection over HTTP.
pub struct HttpDevice {
    client: reqwest::blocking::Client,
    base_url: String,
    token: String,
    logs: Mutex<VecDeque<HttpLogEntry>>,
    next_log_id: AtomicU64,
    /// Desktop-local `is_pinned` decorations, keyed by resolved path.
    pins: Mutex<HashMap<String, bool>>,
    /// Desktop-local color tags, keyed by resolved path.
    tags: Mutex<HashMap<String, ColorTag>>,
    /// Device id whose state-file decoration entry persists under; `None`
    /// until the first successful connection identifies the phone.
    decorations_key: Mutex<Option<String>>,
}

impl HttpDevice {
    /// Builds a backend for `http://ip:port` using the pairing token.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self, DeviceError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| DeviceError::Io(format!("http client: {e}")))?;
        let device = Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            logs: Mutex::new(VecDeque::with_capacity(MAX_LOGS)),
            next_log_id: AtomicU64::new(1),
            pins: Mutex::new(HashMap::new()),
            tags: Mutex::new(HashMap::new()),
            decorations_key: Mutex::new(None),
        };
        if wire_log_enabled() {
            eprintln!(
                "android18 http backend {} (X-Auth: {} chars)",
                device.base_url,
                device.token.len()
            );
        }
        Ok(device)
    }

    /// Base URL this backend talks to (no trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The pairing token used for `X-Auth`.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Arms state persistence under `device_id` and overlays any
    /// previously saved decorations into the in-memory maps. Called right
    /// after the phone identifies itself (its id is unknown before).
    pub fn set_persistence_key(&self, device_id: &str) {
        match crate::state::load_decorations(device_id) {
            Ok(saved) => {
                let mut pins = self.pins.lock().expect("pins mutex");
                let mut tags = self.tags.lock().expect("tags mutex");
                for (path, record) in saved {
                    if record.pinned {
                        pins.insert(path.clone(), true);
                    }
                    if let Some(tag) = record.tag {
                        tags.insert(path, tag);
                    }
                }
            }
            Err(e) => {
                if wire_log_enabled() {
                    eprintln!("android18 decorations load failed: {e}");
                }
            }
        }
        *self.decorations_key.lock().expect("decorations key mutex") = Some(device_id.to_string());
    }

    /// Snapshots pins + tags to the desktop state file (no-op until
    /// [`Self::set_persistence_key`] armed a device id).
    fn persist_decorations(&self) {
        let Some(device_id) = self
            .decorations_key
            .lock()
            .expect("decorations key mutex")
            .clone()
        else {
            return;
        };
        let pins = self.pins.lock().expect("pins mutex");
        let tags = self.tags.lock().expect("tags mutex");
        let mut paths: Vec<&String> = pins.keys().chain(tags.keys()).collect();
        paths.sort();
        paths.dedup();
        let mut map = HashMap::with_capacity(paths.len());
        for path in paths {
            map.insert(
                path.clone(),
                crate::state::DecorationRecord {
                    pinned: pins.get(path).copied().unwrap_or(false),
                    tag: tags.get(path).copied(),
                },
            );
        }
        if let Err(e) = crate::state::save_decorations(&device_id, &map)
            && wire_log_enabled()
        {
            eprintln!("android18 decorations save failed: {e}");
        }
    }

    /// Resumable download: `GET /download?path=` with `Range: bytes=start-end`.
    /// Returns `(status, body)` — 206 for a partial body, 200 when the
    /// service ignored the range. Powers the transfer-queue chunked runner.
    pub fn fetch_range(
        &self,
        path: &str,
        start: u64,
        end: u64,
    ) -> Result<(u16, Vec<u8>), DeviceError> {
        let resolved = resolve(path)?;
        let endpoint = format!("/download?path={resolved}");
        self.send(
            HttpMethod::Get,
            "/download",
            &[("path", resolved.as_str())],
            None,
            Some((start, end)),
            &endpoint,
        )
    }

    /// Sends one request and appends it to the request-log ring.
    #[allow(clippy::too_many_arguments)]
    fn send(
        &self,
        method: HttpMethod,
        endpoint: &str,
        query: &[(&str, &str)],
        body: Option<(&[u8], Option<&str>)>,
        range: Option<(u64, u64)>,
        label: &str,
    ) -> Result<(u16, Vec<u8>), DeviceError> {
        let started = Instant::now();
        let outcome = self.dispatch(method, endpoint, query, body, range);
        let (status, bytes) = match &outcome {
            Ok((status, bytes)) => (*status, Some(bytes.len() as u64)),
            Err(e) => (e.status(), None),
        };
        self.log(method, label, status, bytes, started.elapsed());
        if wire_log_enabled() {
            eprintln!(
                "android18 http {} {}{} → {} {} {}ms (X-Auth: {} chars)",
                method.as_str(),
                self.base_url,
                label,
                status,
                match bytes {
                    Some(n) => format!("{n}B"),
                    None => "—".to_string(),
                },
                started.elapsed().as_millis(),
                self.token.len(),
            );
        }
        outcome
    }

    /// Sends one request **without logging** (used inside `walk`, which logs
    /// a single aggregate entry). `body` is `(bytes, content_type)`.
    fn dispatch(
        &self,
        method: HttpMethod,
        endpoint: &str,
        query: &[(&str, &str)],
        body: Option<(&[u8], Option<&str>)>,
        range: Option<(u64, u64)>,
    ) -> Result<(u16, Vec<u8>), DeviceError> {
        let http_method = match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
        };
        let mut request = self
            .client
            .request(http_method, format!("{}{endpoint}", self.base_url))
            .header("X-Auth", &self.token)
            .query(query);
        if let Some((bytes, content_type)) = body {
            if let Some(mime) = content_type {
                request = request.header(reqwest::header::CONTENT_TYPE, mime);
            }
            request = request.body(bytes.to_vec());
        }
        if let Some((start, end)) = range {
            request = request.header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
        }
        let response = request.send().map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let bytes = response.bytes().map_err(map_transport_error)?.to_vec();
        Ok((status, bytes))
    }

    /// `send` + non-2xx statuses folded into [`DeviceError`].
    fn send_expect(
        &self,
        method: HttpMethod,
        endpoint: &str,
        query: &[(&str, &str)],
        body: Option<(&[u8], Option<&str>)>,
    ) -> Result<Vec<u8>, DeviceError> {
        let label = endpoint_label(endpoint, query);
        let (status, bytes) = self.send(method, endpoint, query, body, None, &label)?;
        if (200..300).contains(&status) {
            Ok(bytes)
        } else {
            Err(status_to_error(status, &bytes))
        }
    }

    /// Appends one entry to the rolling request log (mock parity).
    fn log(
        &self,
        method: HttpMethod,
        endpoint: &str,
        status: u16,
        bytes: Option<u64>,
        elapsed: Duration,
    ) {
        let entry = HttpLogEntry {
            id: self.next_log_id.fetch_add(1, Ordering::Relaxed),
            timestamp: epoch_ms(),
            method,
            endpoint: endpoint.to_string(),
            status,
            duration_ms: elapsed.as_millis() as u64,
            bytes,
        };
        let mut logs = self.logs.lock().expect("http request-log mutex");
        logs.push_back(entry);
        while logs.len() > MAX_LOGS {
            logs.pop_front();
        }
    }

    /// Overlays desktop-local pin/color-tag decorations onto a listing.
    fn decorate(&self, entries: &mut [Entry]) {
        let pins = self.pins.lock().expect("pins mutex");
        let tags = self.tags.lock().expect("tags mutex");
        for entry in entries.iter_mut() {
            if let Some(pinned) = pins.get(&entry.path) {
                entry.is_pinned = *pinned;
            }
            if let Some(tag) = tags.get(&entry.path) {
                entry.color_tag = Some(*tag);
            }
        }
    }

    /// Lists children of `dir` without touching the request log.
    async fn list_raw(&self, dir: &str) -> Result<Vec<Entry>, DeviceError> {
        let (status, bytes) =
            self.dispatch(HttpMethod::Get, "/list", &[("path", dir)], None, None)?;
        if !(200..300).contains(&status) {
            return Err(status_to_error(status, &bytes));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| DeviceError::Io(format!("malformed /list payload: {e}")))
    }

    /// Breadth-first enumeration below `root` (dashboard, search scope).
    async fn walk_bfs(&self, root: &str) -> Result<Vec<Entry>, DeviceError> {
        let mut out = Vec::new();
        let mut queue = VecDeque::from([root.to_string()]);
        let mut first = true;
        while let Some(dir) = queue.pop_front() {
            // An unreadable subfolder (restricted, deleted mid-walk) must not
            // abort the whole enumeration; only a failing root is an error.
            let children = match self.list_raw(&dir).await {
                Ok(children) => children,
                Err(e) if first => return Err(e),
                Err(_) => Vec::new(),
            };
            first = false;
            for child in children {
                if child.dir {
                    queue.push_back(child.path.clone());
                }
                out.push(child);
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }
}

/// Builds the human-readable endpoint label for the request log
/// (e.g. `/list?path=/storage/emulated/0/DCIM`).
fn endpoint_label(endpoint: &str, query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return endpoint.to_string();
    }
    let pairs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{endpoint}?{}", pairs.join("&"))
}

/// Maps reqwest transport failures onto the error taxonomy.
fn map_transport_error(error: reqwest::Error) -> DeviceError {
    if error.is_timeout() || error.is_connect() {
        DeviceError::Offline(error.to_string())
    } else {
        DeviceError::Io(error.to_string())
    }
}

/// Folds an HTTP status (plus body text) into [`DeviceError`].
fn status_to_error(status: u16, body: &[u8]) -> DeviceError {
    let text = String::from_utf8_lossy(body);
    let detail = text.trim().trim_matches('"');
    let message = if detail.is_empty() {
        format!("HTTP {status}")
    } else {
        detail.to_string()
    };
    match status {
        400 => DeviceError::BadRequest(message),
        401 => DeviceError::Unauthorized,
        404 => DeviceError::NotFound(message),
        409 => DeviceError::Conflict(message),
        503 => DeviceError::Offline(message),
        _ => DeviceError::Io(format!("HTTP {status}: {message}")),
    }
}

#[async_trait]
impl DeviceBackend for HttpDevice {
    async fn list(&self, path: &str, _token: &str) -> Result<Vec<Entry>, DeviceError> {
        let resolved = resolve(path)?;
        let bytes = self.send_expect(
            HttpMethod::Get,
            "/list",
            &[("path", resolved.as_str())],
            None,
        )?;
        let mut entries: Vec<Entry> = serde_json::from_slice(&bytes)
            .map_err(|e| DeviceError::Io(format!("malformed /list payload: {e}")))?;
        self.decorate(&mut entries);
        Ok(entries)
    }

    async fn read_text(&self, path: &str, _token: &str) -> Result<String, DeviceError> {
        let resolved = resolve(path)?;
        let bytes = self.send_expect(
            HttpMethod::Get,
            "/file",
            &[("path", resolved.as_str())],
            None,
        )?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn read_range(
        &self,
        path: &str,
        start: u64,
        max_len: u64,
        _token: &str,
    ) -> Result<Vec<u8>, DeviceError> {
        if max_len == 0 {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(max_len - 1);
        let (status, body) = self.fetch_range(path, start, end)?;
        match status {
            200..=299 => Ok(range_slice(status, body, start, end)),
            // Range starts at/after EOF: nothing left to read.
            416 => Ok(Vec::new()),
            _ => Err(status_to_error(status, &body)),
        }
    }

    async fn mkdir(&self, path: &str, _token: &str) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        self.send_expect(
            HttpMethod::Post,
            "/mkdir",
            &[("path", resolved.as_str())],
            None,
        )
        .map(|_| ())
    }

    async fn touch(&self, path: &str, _token: &str) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        self.send_expect(
            HttpMethod::Post,
            "/touch",
            &[("path", resolved.as_str())],
            None,
        )
        .map(|_| ())
    }

    async fn remove(&self, path: &str, _token: &str) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        self.send_expect(
            HttpMethod::Post,
            "/rm",
            &[("path", resolved.as_str())],
            None,
        )
        .map(|_| ())
    }

    async fn mv(&self, from: &str, to: &str, _token: &str) -> Result<(), DeviceError> {
        let from = resolve(from)?;
        let to = resolve(to)?;
        self.send_expect(
            HttpMethod::Post,
            "/mv",
            &[("from", from.as_str()), ("to", to.as_str())],
            None,
        )
        .map(|_| ())
    }

    async fn cp(&self, from: &str, to_folder: &str, _token: &str) -> Result<(), DeviceError> {
        let from = resolve(from)?;
        let to_folder = resolve(to_folder)?;
        self.send_expect(
            HttpMethod::Post,
            "/cp",
            &[("from", from.as_str()), ("to", to_folder.as_str())],
            None,
        )
        .map(|_| ())
    }

    async fn upload(
        &self,
        dest_folder: &str,
        name: &str,
        _size: u64,
        mime_type: Option<&str>,
        content: Option<String>,
        _token: &str,
    ) -> Result<(), DeviceError> {
        let folder = resolve(dest_folder)?;
        let name = name.trim();
        if name.is_empty() || name.contains('/') {
            return Err(DeviceError::BadRequest("invalid file name".into()));
        }
        let content = content.unwrap_or_default().into_bytes();
        let mime = mime_type.unwrap_or("application/octet-stream");
        let full_path = format!("{folder}/{name}");
        self.send_expect(
            HttpMethod::Post,
            "/upload",
            &[("path", full_path.as_str())],
            Some((content.as_slice(), Some(mime))),
        )
        .map(|_| ())
    }

    async fn upload_chunk(
        &self,
        path: &str,
        offset: u64,
        chunk: Vec<u8>,
        _token: &str,
    ) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        let offset = offset.to_string();
        self.send_expect(
            HttpMethod::Post,
            "/upload",
            &[("path", resolved.as_str()), ("offset", offset.as_str())],
            Some((chunk.as_slice(), Some("application/octet-stream"))),
        )
        .map(|_| ())
    }

    async fn thumb(&self, path: &str, max_dim: u32, _token: &str) -> Result<Vec<u8>, DeviceError> {
        let resolved = resolve(path)?;
        let dim = max_dim.to_string();
        self.send_expect(
            HttpMethod::Get,
            "/thumb",
            &[("path", resolved.as_str()), ("max", dim.as_str())],
            None,
        )
    }

    async fn walk(&self, path: &str, _token: &str) -> Result<Vec<Entry>, DeviceError> {
        let resolved = resolve(path)?;
        let label = format!("/walk?path={resolved}");
        let started = Instant::now();
        // Result produced first: the aggregate log entry must record the
        // outcome either way (no early returns before logging).
        let result = self.walk_bfs(&resolved).await;
        match &result {
            Ok(entries) => self.log(
                HttpMethod::Get,
                &label,
                200,
                Some(entries.len() as u64),
                started.elapsed(),
            ),
            Err(e) => self.log(HttpMethod::Get, &label, e.status(), None, started.elapsed()),
        }
        result
    }

    async fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        self.pins
            .lock()
            .expect("pins mutex")
            .insert(resolved, pinned);
        self.persist_decorations();
        Ok(())
    }

    async fn set_color_tag(&self, path: &str, tag: Option<ColorTag>) -> Result<(), DeviceError> {
        let resolved = resolve(path)?;
        let mut tags = self.tags.lock().expect("tags mutex");
        match tag {
            Some(tag) => {
                tags.insert(resolved, tag);
            }
            None => {
                tags.remove(&resolved);
            }
        }
        drop(tags);
        self.persist_decorations();
        Ok(())
    }

    async fn device_info(&self) -> Result<Device, DeviceError> {
        let bytes = self.send_expect(HttpMethod::Get, "/info", &[], None)?;
        let mut device: Device = serde_json::from_slice(&bytes)
            .map_err(|e| DeviceError::Io(format!("malformed /info payload: {e}")))?;
        // The phone never echoes the token or our view of the endpoint.
        device.base_url = self.base_url.clone();
        device.token = self.token.clone();
        device.status = DeviceStatus::Connected;
        // transport/port are desktop-owned too: loopback base URLs are
        // `adb forward` tunnels, everything else is direct Wi-Fi/LAN.
        let (transport, port) = endpoint_kind(&self.base_url);
        device.transport = transport;
        device.port = port;
        Ok(device)
    }

    fn request_log(&self) -> Vec<HttpLogEntry> {
        self.logs
            .lock()
            .expect("http request-log mutex")
            .iter()
            .cloned()
            .collect()
    }
}

/// Narrows a `/download` response to the requested `[start, end]` window:
/// 206 bodies are already partial, 200 bodies are the whole file.
pub(crate) fn range_slice(status: u16, mut body: Vec<u8>, start: u64, end: u64) -> Vec<u8> {
    let want = end.saturating_sub(start).saturating_add(1) as usize;
    if status == 206 {
        body.truncate(want);
        body
    } else {
        let len = body.len() as u64;
        let from = start.min(len) as usize;
        let to = end.saturating_add(1).min(len) as usize;
        body[from..to].to_vec()
    }
}

#[cfg(test)]
mod test_server;

#[cfg(test)]
mod tests {
    use super::test_server::{Response, StubServer};
    use super::*;
    use futures::executor::block_on;

    const TOKEN: &str = "pairing-token-1234";

    #[test]
    fn endpoint_kind_marks_loopback_as_usb() {
        assert_eq!(
            endpoint_kind("http://127.0.0.1:8186"),
            (Transport::Usb, 8186)
        );
        assert_eq!(
            endpoint_kind("http://localhost:8181"),
            (Transport::Usb, 8181)
        );
        assert_eq!(
            endpoint_kind("http://10.0.2.15:8080"),
            (Transport::Wifi, 8080)
        );
        assert_eq!(
            endpoint_kind("http://192.168.1.5:8080/"),
            (Transport::Wifi, 8080)
        );
        assert_eq!(endpoint_kind("junk"), (Transport::Wifi, 0));
    }

    /// Boots a stub implementing the full wire API over an in-memory tree.
    fn stub() -> (StubServer, HttpDevice) {
        let server = StubServer::start(route);
        let device = HttpDevice::new(server.url(), TOKEN).expect("build http device");
        (server, device)
    }

    fn route(req: &test_server::Request) -> Response {
        let authorized = req.headers.get("x-auth").map(String::as_str) == Some(TOKEN);
        if !authorized {
            return Response::text(401, "missing or invalid X-Auth token");
        }
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/info") => Response::json(
                200,
                serde_json::json!({
                    "id": "pixel-8",
                    "name": "Pixel 8",
                    "model": "Pixel 8",
                    "transport": "wifi",
                    "base_url": "http://192.168.1.42:8080",
                    "status": "connected",
                    "port": 8080,
                    "storage_used_bytes": 100,
                    "storage_total_bytes": 1000,
                    "android_version": "14"
                }),
            ),
            ("GET", "/list") => {
                let path = req.query.get("path").map(String::as_str).unwrap_or("");
                if path.contains("..") {
                    return Response::text(400, "invalid path");
                }
                if path.contains("ghost") {
                    return Response::text(404, "not found");
                }
                let entries = match path {
                    "/storage/emulated/0" => vec![
                        entry("/storage/emulated/0/DCIM", true),
                        entry("/storage/emulated/0/notes.txt", false),
                    ],
                    "/storage/emulated/0/DCIM" => vec![
                        entry("/storage/emulated/0/DCIM/IMG_001.jpg", false),
                        entry("/storage/emulated/0/DCIM/vacation", true),
                    ],
                    _ => Vec::new(),
                };
                Response::json(
                    200,
                    serde_json::to_value(entries).expect("serialize entries"),
                )
            }
            ("GET", "/file") => Response::text(200, "hello from the phone"),
            ("GET", "/download") => {
                let start = req
                    .headers
                    .get("range")
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.split('-').next())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let data = b"0123456789abcdef";
                if start > 0 && start < data.len() {
                    return Response::bytes(
                        206,
                        data[start..].to_vec(),
                        "application/octet-stream",
                    );
                }
                Response::bytes(200, data.to_vec(), "application/octet-stream")
            }
            ("POST", "/rm") => {
                let path = req.query.get("path").map(String::as_str).unwrap_or("");
                if path.contains("ghost") {
                    return Response::text(404, "not found");
                }
                if path.is_empty() {
                    return Response::text(400, "path is required");
                }
                Response::text(200, "")
            }
            ("POST", "/mv") => {
                let (from, to) = (
                    req.query.get("from").cloned().unwrap_or_default(),
                    req.query.get("to").cloned().unwrap_or_default(),
                );
                if from == "/storage/emulated/0/notes.txt" && to == "/storage/emulated/0/notes.txt"
                {
                    return Response::text(409, "destination already exists");
                }
                Response::text(200, "")
            }
            ("POST", "/upload") => {
                if req.body.is_empty() {
                    return Response::text(400, "empty upload");
                }
                Response::text(200, "")
            }
            _ => Response::text(404, "unknown endpoint"),
        }
    }

    fn entry(path: &str, dir: bool) -> Entry {
        if dir {
            Entry::dir(path, 0, None, false)
        } else {
            Entry::file(path, 11, 0, Some("text/plain"), None)
        }
    }

    fn device_of(url: &str) -> HttpDevice {
        HttpDevice::new(url, TOKEN).expect("build http device")
    }

    #[test]
    fn list_maps_entries_and_logs_request() {
        let (server, device) = stub();
        let entries = block_on(device.list("/storage/emulated/0", TOKEN)).expect("list");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.dir && e.path.ends_with("DCIM")));
        let logs = device.request_log();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].endpoint, "/list?path=/storage/emulated/0");
        assert_eq!(logs[0].status, 200);
        drop(server);
    }

    #[test]
    fn traversal_rejected_before_any_request() {
        let (server, device) = stub();
        let err = block_on(device.list("/storage/emulated/0/../..", TOKEN))
            .expect_err("must reject traversal");
        assert!(matches!(err, DeviceError::BadRequest(_)));
        assert!(device.request_log().is_empty());
        drop(server);
    }

    #[test]
    fn unauthorized_maps_to_401() {
        let server = StubServer::start(route);
        let device = HttpDevice::new(server.url(), "short").expect("build");
        let err = block_on(device.list("/storage/emulated/0", "short")).expect_err("401");
        assert_eq!(err.status(), 401);
        drop(server);
    }

    #[test]
    fn not_found_and_conflict_and_bad_request_map_to_statuses() {
        let (server, device) = stub();
        assert_eq!(
            block_on(device.list("/storage/emulated/0/ghost", TOKEN))
                .expect_err("404")
                .status(),
            404
        );
        assert_eq!(
            block_on(device.mv(
                "/storage/emulated/0/notes.txt",
                "/storage/emulated/0/notes.txt",
                TOKEN
            ))
            .expect_err("409")
            .status(),
            409
        );
        drop(server);
    }

    #[test]
    fn read_text_returns_body() {
        let (server, device) = stub();
        let text =
            block_on(device.read_text("/storage/emulated/0/notes.txt", TOKEN)).expect("read");
        assert_eq!(text, "hello from the phone");
        drop(server);
    }

    #[test]
    fn walk_enumerates_descendants_with_one_log_entry() {
        let (server, device) = stub();
        let entries = block_on(device.walk("/storage/emulated/0", TOKEN)).expect("walk");
        assert_eq!(entries.len(), 4); // DCIM, notes.txt, IMG_001.jpg, vacation
        assert!(entries.iter().all(|e| e.path != "/storage/emulated/0"));
        let logs = device.request_log();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].endpoint, "/walk?path=/storage/emulated/0");
        assert_eq!(logs[0].bytes, Some(4));
        drop(server);
    }

    #[test]
    fn upload_sends_body_and_mime() {
        let (server, device) = stub();
        block_on(device.upload(
            "/storage/emulated/0",
            "notes.txt",
            5,
            Some("text/plain"),
            Some("hello".into()),
            TOKEN,
        ))
        .expect("upload");
        let seen = server.requests();
        let last = seen.last().expect("one request");
        assert_eq!(
            last.headers.get("content-type").map(String::as_str),
            Some("text/plain")
        );
        assert_eq!(last.body, b"hello".to_vec());
        drop(server);
    }

    #[test]
    fn decorations_are_local_and_appear_in_listings() {
        let (server, device) = stub();
        block_on(device.set_pinned("/storage/emulated/0/DCIM", true)).expect("pin");
        block_on(device.set_color_tag("/storage/emulated/0/DCIM", Some(ColorTag::Blue)))
            .expect("tag");
        let entries = block_on(device.list("/storage/emulated/0", TOKEN)).expect("list");
        let dcim = entries
            .iter()
            .find(|e| e.path.ends_with("DCIM"))
            .expect("dcim");
        assert!(dcim.is_pinned);
        assert_eq!(dcim.color_tag, Some(ColorTag::Blue));
        drop(server);
    }

    #[test]
    fn device_info_fills_local_endpoint_and_token() {
        let (server, device) = stub();
        let info = block_on(device.device_info()).expect("info");
        assert_eq!(info.id, "pixel-8");
        assert_eq!(info.base_url, server.url());
        assert_eq!(info.token, TOKEN);
        assert_eq!(info.status, DeviceStatus::Connected);
        drop(server);
    }

    #[test]
    fn offline_server_maps_to_offline_error() {
        // Bind then immediately drop the listener to get a dead port.
        let dead = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = dead.local_addr().expect("addr").port();
        drop(dead);
        let device = device_of(&format!("http://127.0.0.1:{port}"));
        let err = block_on(device.list("/storage/emulated/0", TOKEN)).expect_err("offline");
        assert_eq!(err.status(), 503);
    }

    #[test]
    fn fetch_range_returns_partial_body() {
        let (server, device) = stub();
        let (status, bytes) = device
            .fetch_range("/storage/emulated/0/notes.txt", 4, u64::MAX)
            .expect("range");
        assert_eq!(status, 206);
        assert_eq!(bytes, b"456789abcdef".to_vec());
        drop(server);
    }

    #[test]
    fn read_range_returns_only_the_requested_window() {
        let (server, device) = stub();
        let bytes = block_on(device.read_range("/storage/emulated/0/notes.txt", 4, 3, TOKEN))
            .expect("window");
        assert_eq!(bytes, b"456".to_vec());
        // zero-length windows never hit the wire
        assert!(
            block_on(device.read_range("/storage/emulated/0/notes.txt", 4, 0, TOKEN))
                .expect("empty")
                .is_empty()
        );
        drop(server);
    }

    #[test]
    fn range_slice_handles_ignored_ranges_and_eof() {
        // 200: whole body, window [2, 4]
        assert_eq!(range_slice(200, b"abcdef".to_vec(), 2, 4), b"cde".to_vec());
        // 200 with start past EOF
        assert!(range_slice(200, b"abc".to_vec(), 9, 12).is_empty());
        // 206 overshoot is truncated to the window
        assert_eq!(range_slice(206, b"abcdef".to_vec(), 0, 2), b"abc".to_vec());
    }

    #[test]
    fn log_ring_keeps_at_most_80_entries() {
        let (server, device) = stub();
        for _ in 0..90 {
            let _ = device.fetch_range("/storage/emulated/0/notes.txt", 0, u64::MAX);
        }
        assert_eq!(device.request_log().len(), 80);
        drop(server);
    }
}
