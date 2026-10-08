//! Desktop side of pairing, in both directions.
//!
//! - **QR (phone-initiated):** while the pairing sheet is open, the desktop
//!   binds an ephemeral TCP port on the LAN. The QR encodes
//!   `a18|2|<ip>|<port>|<code>`; the phone scans it, asks its user to
//!   allow the connection, then sends `POST /pair`
//!   with the one-time code in `X-Pair-Code` and a JSON body
//!   `{name, deviceId, ip, port, token}`. The code works exactly once —
//!   later attempts get 403, and five wrong codes close the port. Payload
//!   delivery and every closure path reach the UI as [`PairingEvent`]s
//!   through a channel polled from the workspace, so a closed listener can
//!   never leave a dead QR on screen.
//! - **Approval / code (desktop-initiated):** [`request_pairing`] POSTs
//!   `/pair-request` to a discovered phone. Either the phone shows an
//!   allow/deny prompt, or the body carries the phone's 6-char pair code
//!   and is granted at once. Same identity/token answer — no token entry
//!   needed on the desktop side.
//!
//! The listener's HTTP is parsed by hand (no new dependencies):
//! read-header → read-body → respond → close, with hard caps on both. Bind
//! failures surface as `Err(String)` so the pairing sheet can fall back to
//! manual entry.

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// Request-line + headers cap; anything bigger is a malformed client.
const MAX_HEAD: usize = 4 * 1024;
/// Body cap — the pairing JSON is ~100 bytes.
const MAX_BODY: usize = 16 * 1024;
/// Per-read timeout for the handshake.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the accept loop sleeps between polls.
const ACCEPT_POLL: Duration = Duration::from_millis(50);
/// Minimum token length (the phone service enforces the same rule).
const MIN_TOKEN_LEN: usize = 8;
/// Pair-code length shown on both devices (e.g. `0X1D8C`).
pub const CODE_LEN: usize = 6;
/// Crockford base32 — digits and letters minus the look-alikes `I L O U`.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Wrong `X-Pair-Code` guesses the one-shot listener tolerates before it
/// closes its port.
const MAX_BAD_CODES: u32 = 5;

/// What the accept thread reports to the UI. [`PairingEvent::Payload`] is
/// delivered once on a successful pairing; `Closed` follows on every exit
/// path so the gate can retire or regenerate the QR instead of
/// advertising a dead endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingEvent {
    /// The phone scanned the QR and posted a valid pairing payload.
    Payload(PairingPayload),
    /// The one-shot port stopped serving; the QR encoding it is dead.
    Closed {
        /// True when the listener closed because pairing succeeded.
        paired: bool,
        /// Short human-readable cause for the status line.
        reason: &'static str,
    },
}

/// What the phone sends after scanning the QR.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PairingPayload {
    /// Human-readable phone name (e.g. "Pixel 9 Pro").
    pub name: String,
    /// Stable service id; empty when the phone did not send one.
    #[serde(default, rename = "deviceId")]
    pub device_id: String,
    /// Phone LAN address the desktop should dial back on.
    pub ip: String,
    /// Phone service port.
    pub port: u16,
    /// Pairing token for `X-Auth`.
    pub token: String,
    /// TCP peer address the pairing POST arrived from — the route that
    /// demonstrably reaches the phone, unlike the self-reported `ip`
    /// (which can be a cellular/VPN interface). Set by the listener.
    #[serde(skip)]
    pub peer: Option<IpAddr>,
}

impl PairingPayload {
    /// `http://ip:port` for the HTTP backend. Prefers the observed peer
    /// address over the body `ip`; a loopback peer (tunnels) is ignored.
    pub fn base_url(&self) -> String {
        match self.peer.filter(|peer| !peer.is_loopback()) {
            Some(IpAddr::V6(peer)) => format!("http://[{peer}]:{}", self.port),
            Some(peer) => format!("http://{peer}:{}", self.port),
            None => format!("http://{}:{}", self.ip, self.port),
        }
    }
}

/// What the phone returns after the user approves a `/pair-request`:
/// everything needed to build an authorized `HttpDevice`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PairGrant {
    /// Human-readable phone name (e.g. "Pixel 9 Pro").
    pub name: String,
    /// Stable service id; empty when the phone did not send one.
    #[serde(default, rename = "deviceId")]
    pub device_id: String,
    /// Pairing token for `X-Auth`.
    pub token: String,
}

/// Why a [`request_pairing`] call failed; each variant maps to distinct
/// copy in the server sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairRequestError {
    /// The phone answered 403 — the user tapped Deny.
    Denied,
    /// The prompt expired with no answer (the phone drops it after 60s).
    NoAnswer,
    /// A code was sent and the phone rejected it (403 with a code).
    WrongCode,
    /// Too many wrong codes — the phone locked pairing for a while (429).
    RateLimited,
    /// No service at `base_url` or an unexpected answer (404 = an app
    /// version without `/pair-request`). Holds the detail line.
    Unreachable(String),
}

impl fmt::Display for PairRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => write!(f, "denied on the phone"),
            Self::NoAnswer => write!(f, "no answer on the phone"),
            Self::WrongCode => write!(f, "wrong pair code"),
            Self::RateLimited => write!(f, "too many wrong codes — wait a moment"),
            Self::Unreachable(detail) => write!(f, "unreachable: {detail}"),
        }
    }
}

impl std::error::Error for PairRequestError {}

/// Tokenless pairing for discovered/manual connects: `POST /pair-request`.
///
/// With `code` (the phone's 6-char pair code) a correct code is granted
/// at once; without it the call blocks while the phone shows an "Allow
/// this computer?" prompt (or auto-grants a USB pre-approval).
///
/// `base_url` is the phone's `http://ip:port`; a trailing slash is
/// tolerated. `desktop_name` is shown on the phone so the user knows who
/// is asking. The granted token is returned to the caller only — it is
/// never logged. Like every adapter here, the blocking client must run
/// on a plain background thread, never inside an async runtime.
pub fn request_pairing(
    base_url: &str,
    desktop_name: &str,
    code: Option<&str>,
    timeout: Duration,
) -> Result<PairGrant, PairRequestError> {
    let url = format!("{}/pair-request", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        // A zero `Duration` would disable the timeout entirely.
        .timeout(timeout.max(Duration::from_secs(1)))
        .build()
        .map_err(|e| PairRequestError::Unreachable(e.to_string()))?;
    let body = match code {
        Some(code) => serde_json::json!({ "name": desktop_name, "code": code }),
        None => serde_json::json!({ "name": desktop_name }),
    };
    let response = client
        .post(url)
        .json(&body)
        .send()
        .map_err(|e| PairRequestError::Unreachable(e.to_string()))?;
    match response.status().as_u16() {
        200 => {
            let grant = response
                .json::<PairGrant>()
                .map_err(|e| PairRequestError::Unreachable(format!("bad grant payload: {e}")))?;
            grant_is_valid(&grant)
                .then_some(grant)
                .ok_or_else(|| PairRequestError::Unreachable("grant missing name or token".into()))
        }
        403 if code.is_some() => Err(PairRequestError::WrongCode),
        403 => Err(PairRequestError::Denied),
        408 => Err(PairRequestError::NoAnswer),
        429 => Err(PairRequestError::RateLimited),
        404 => Err(PairRequestError::Unreachable(
            "no /pair-request endpoint — update the Android18 app on the phone".into(),
        )),
        status => Err(PairRequestError::Unreachable(format!(
            "phone answered {status}"
        ))),
    }
}

/// Grant sanity: nonempty name and an 8+ char token, mirroring
/// [`payload_is_valid`].
fn grant_is_valid(grant: &PairGrant) -> bool {
    !grant.name.trim().is_empty() && grant.token.trim().len() >= MIN_TOKEN_LEN
}

/// A parsed minimal HTTP request.
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    /// Case-insensitive header lookup.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Reads one request: head bytes until `\r\n\r\n`, then the body up to
/// `Content-Length`.
fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| format!("set read timeout: {e}"))?;
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    // Head.
    let head_end = loop {
        if let Some(at) = find_head_end(&buf) {
            break at;
        }
        if buf.len() > MAX_HEAD {
            return Err("request head too large".into());
        }
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("connection closed mid-head".into());
        }
        buf.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || path.is_empty() {
        return Err("malformed request line".into());
    }
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .collect();
    // Body.
    let length: usize = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err("request body too large".into());
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("connection closed mid-body".into());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

/// Offset of the `\r\n\r\n` head terminator, if fully received.
fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Writes a JSON response and closes the connection.
fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let message = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(message.as_bytes());
    let _ = stream.flush();
}

/// The desktop's one-shot pairing endpoint.
pub struct PairingListener {
    /// `ip:port` the QR encodes.
    endpoint: String,
    /// 6-char one-time code ([`CODE_LEN`], Crockford base32).
    code: String,
    /// Events from the accept thread: the payload, then `Closed`.
    rx: Receiver<PairingEvent>,
    stop: Arc<AtomicBool>,
}

impl PairingListener {
    /// Binds an ephemeral port on every interface and starts the accept
    /// thread. Fails (as `Err(String)`) when the bind is refused — the
    /// onboarding sheet falls back to manual pairing then.
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| format!("bind: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("local addr: {e}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking: {e}"))?;
        let ip = lan_ip();
        let code = pair_code();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let paired = Arc::new(AtomicBool::new(false));
        let thread_code = code.clone();
        let thread_paired = Arc::clone(&paired);
        let thread_stop = Arc::clone(&stop);
        // Detached: exits within one ACCEPT_POLL of stop()/a successful
        // pairing, unbinding the socket when the listener drops.
        thread::spawn(move || {
            let mut bad_codes = 0u32;
            while !thread_stop.load(Ordering::Relaxed)
                && !thread_paired.load(Ordering::Relaxed)
                && bad_codes < MAX_BAD_CODES
            {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        let peer = Some(peer.ip());
                        if !handle_connection(&mut stream, peer, &thread_code, &thread_paired, &tx)
                        {
                            bad_codes += 1;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_POLL);
                    }
                    Err(_) => break,
                }
            }
            // Every exit path reports closure: without this a spent or
            // attempt-capped listener would leave a dead QR on screen.
            let paired = thread_paired.load(Ordering::Relaxed);
            let reason = if paired {
                "paired"
            } else if thread_stop.load(Ordering::Relaxed) {
                "stopped"
            } else if bad_codes >= MAX_BAD_CODES {
                "too many wrong codes"
            } else {
                "accept failed"
            };
            let _ = tx.send(PairingEvent::Closed { paired, reason });
        });
        Ok(Self {
            endpoint: format!("{ip}:{port}"),
            code,
            rx,
            stop,
        })
    }

    /// `ip:port` the QR should encode.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The one-time pair code (6 uppercase base32 chars, e.g. `0X1D8C`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The next listener event, if any: [`PairingEvent::Payload`] when the
    /// phone posts one, or [`PairingEvent::Closed`] once the one-shot port
    /// stops serving.
    pub fn try_event(&self) -> Option<PairingEvent> {
        self.rx.try_recv().ok()
    }

    /// Stops accepting; later posts get nothing (the socket closes).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for PairingListener {
    /// A replaced or discarded listener must not keep its port and stale
    /// code alive.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Validates and serves one `/pair` request. Returns `false` only for a
/// wrong or missing pair code, which counts against [`MAX_BAD_CODES`].
fn handle_connection(
    stream: &mut TcpStream,
    peer: Option<IpAddr>,
    code: &str,
    paired: &AtomicBool,
    tx: &std::sync::mpsc::Sender<PairingEvent>,
) -> bool {
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(e) => {
            respond(
                stream,
                400,
                "Bad Request",
                &format!(r#"{{"ok":false,"error":"{e}"}}"#),
            );
            return true;
        }
    };
    if request.method != "POST" {
        respond(
            stream,
            405,
            "Method Not Allowed",
            r#"{"ok":false,"error":"POST only"}"#,
        );
        return true;
    }
    if request.path != "/pair" {
        respond(
            stream,
            404,
            "Not Found",
            r#"{"ok":false,"error":"unknown path"}"#,
        );
        return true;
    }
    let sent = request.header("x-pair-code").and_then(normalize_code);
    if sent.as_deref() != Some(code) {
        respond(
            stream,
            403,
            "Forbidden",
            r#"{"ok":false,"error":"bad pair code"}"#,
        );
        return false;
    }
    if paired.load(Ordering::Relaxed) {
        respond(
            stream,
            409,
            "Conflict",
            r#"{"ok":false,"error":"already paired"}"#,
        );
        return true;
    }
    match serde_json::from_slice::<PairingPayload>(&request.body) {
        Ok(mut payload) if payload_is_valid(&payload) => {
            payload.peer = peer;
            paired.store(true, Ordering::Relaxed);
            respond(stream, 200, "OK", r#"{"ok":true}"#);
            let _ = tx.send(PairingEvent::Payload(payload));
        }
        Ok(_) => respond(
            stream,
            400,
            "Bad Request",
            r#"{"ok":false,"error":"ip, port and an 8+ char token are required"}"#,
        ),
        Err(e) => respond(
            stream,
            400,
            "Bad Request",
            &format!(r#"{{"ok":false,"error":"bad json: {e}"}}"#),
        ),
    }
    true
}

/// Payload sanity: nonempty ip, nonzero port, 8+ char token, nonempty name.
fn payload_is_valid(payload: &PairingPayload) -> bool {
    !payload.ip.trim().is_empty()
        && payload.port != 0
        && payload.token.trim().len() >= MIN_TOKEN_LEN
        && !payload.name.trim().is_empty()
}

/// The LAN address a phone would route to: a UDP "connect" picks the
/// default-interface address without sending anything. Loopback is the
/// offline fallback (USB/manual pairing still work then). Public for the
/// workspace's IP watcher, which rebinds the pairing session on change.
pub fn lan_ip() -> IpAddr {
    UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

/// A fresh [`CODE_LEN`]-char pair code. Bytes come from the OS CSPRNG
/// (`/dev/urandom`, no RNG crate); if that is unavailable the clock and
/// `RandomState` keep the code unpredictable enough for a one-shot,
/// attempt-capped session.
fn pair_code() -> String {
    let mut bytes = [0u8; CODE_LEN];
    let from_os = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok();
    if !from_os {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or_default();
        for (round, byte) in bytes.iter_mut().enumerate() {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(round as u64);
            hasher.write_u64(now);
            *byte = (hasher.finish() >> 24) as u8;
        }
    }
    // 256 % 32 == 0, so the modulo is unbiased.
    bytes
        .iter()
        .map(|b| CODE_ALPHABET[(*b as usize) % CODE_ALPHABET.len()] as char)
        .collect()
}

/// Canonical form of a typed pair code: uppercase, separators dropped,
/// and the Crockford look-alikes folded (`O`→`0`, `I`/`L`→`1`). Returns
/// `None` unless the result is exactly [`CODE_LEN`] alphabet chars.
pub fn normalize_code(input: &str) -> Option<String> {
    let code: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect();
    let valid = code.len() == CODE_LEN && code.bytes().all(|b| CODE_ALPHABET.contains(&b));
    valid.then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal HTTP client for the loopback tests: returns
    /// `(status, body)` from the response.
    fn post(endpoint: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(endpoint).expect("connect");
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .expect("read timeout");
        let mut request = format!("POST {path} HTTP/1.1\r\nHost: desktop\r\n");
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        stream.write_all(request.as_bytes()).expect("write request");
        let mut response = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => response.extend_from_slice(&chunk[..n]),
            }
        }
        let text = String::from_utf8_lossy(&response).to_string();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_default();
        let body = text
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string();
        (status, body)
    }

    fn sample_body() -> String {
        r#"{"name":"Pixel 9 Pro","deviceId":"pixel-9-pro","ip":"127.0.0.1","port":8080,"token":"pairing-token-1"}"#.into()
    }

    #[test]
    fn pairs_once_with_the_right_code() {
        let listener = PairingListener::start().expect("start listener");
        let good = &[("X-Pair-Code", listener.code())];
        let (status, body) = post(listener.endpoint(), "/pair", good, &sample_body());
        assert_eq!(status, 200, "body: {body}");
        let event = listener.try_event().expect("payload delivered after a 200");
        let PairingEvent::Payload(payload) = event else {
            panic!("expected a payload event first, got {event:?}");
        };
        assert_eq!(payload.name, "Pixel 9 Pro");
        assert_eq!(payload.device_id, "pixel-9-pro");
        assert_eq!(payload.port, 8080);
        // Loopback peers fall back to the body ip; a LAN peer (the test
        // dials this host's LAN address) is used as-is.
        assert!(payload.base_url().ends_with(":8080"));
        // One-shot endpoint: after the successful pair the accept thread
        // exits and the port closes, so the code cannot be replayed —
        // either the connection is refused outright or it is rejected.
        thread::sleep(ACCEPT_POLL * 2);
        match TcpStream::connect(listener.endpoint()) {
            Err(_) => {} // port already closed — the usual outcome
            Ok(mut stream) => {
                let request = format!(
                    "POST /pair HTTP/1.1\r\nX-Pair-Code: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    listener.code(),
                    sample_body().len(),
                    sample_body()
                );
                let _ = stream.write_all(request.as_bytes());
                let mut buf = [0u8; 256];
                let _ = stream.read(&mut buf);
                let text = String::from_utf8_lossy(&buf);
                assert!(
                    text.starts_with("HTTP/1.1 4"),
                    "replay must fail with a 4xx, got: {text}"
                );
            }
        }
        // Closure trails the payload so the UI can retire the QR rather
        // than advertise a dead endpoint.
        assert_eq!(
            listener.try_event(),
            Some(PairingEvent::Closed {
                paired: true,
                reason: "paired",
            })
        );
        assert_eq!(listener.try_event(), None);
        listener.stop();
    }

    #[test]
    fn base_url_prefers_a_routable_peer_over_the_body_ip() {
        let mut payload = PairingPayload {
            name: "P".into(),
            device_id: String::new(),
            ip: "10.99.0.5".into(),
            port: 8080,
            token: "pairing-token-1".into(),
            peer: Some("192.168.1.77".parse().expect("ip")),
        };
        assert_eq!(payload.base_url(), "http://192.168.1.77:8080");
        payload.peer = Some("127.0.0.1".parse().expect("ip"));
        assert_eq!(payload.base_url(), "http://10.99.0.5:8080");
        payload.peer = None;
        assert_eq!(payload.base_url(), "http://10.99.0.5:8080");
    }

    #[test]
    fn dropping_the_listener_closes_its_port() {
        let listener = PairingListener::start().expect("start listener");
        let endpoint = listener.endpoint().to_string();
        drop(listener);
        thread::sleep(ACCEPT_POLL * 4);
        let closed = TcpStream::connect(&endpoint).is_err();
        assert!(closed, "port must close once the listener drops");
    }

    #[test]
    fn rejects_wrong_code_and_bad_payloads() {
        let listener = PairingListener::start().expect("start listener");
        let endpoint = listener.endpoint().to_string();
        let code = listener.code().to_string();
        let wrong = [("X-Pair-Code", "ZZZZZZ")];
        let (status, _) = post(&endpoint, "/pair", &wrong, &sample_body());
        assert_eq!(status, 403);
        let none: [(&str, &str); 0] = [];
        let (status, _) = post(&endpoint, "/pair", &none, &sample_body());
        assert_eq!(status, 403);
        let good = [("X-Pair-Code", code.as_str())];
        let (status, body) = post(&endpoint, "/pair", &good, "not json");
        assert_eq!(status, 400, "body: {body}");
        let short = r#"{"name":"P","deviceId":"p","ip":"127.0.0.1","port":8080,"token":"short"}"#;
        let (status, _) = post(&endpoint, "/pair", &good, short);
        assert_eq!(status, 400);
        let no_ip = r#"{"name":"P","deviceId":"p","ip":"","port":8080,"token":"pairing-token-1"}"#;
        let (status, _) = post(&endpoint, "/pair", &good, no_ip);
        assert_eq!(status, 400);
        // GET and unknown paths are refused without consuming the code.
        let (status, _) = post(&endpoint, "/nope", &good, &sample_body());
        assert_eq!(status, 404);
        let (status, _) = post(&endpoint, "/pair", &good, &sample_body());
        assert_eq!(status, 200);
        listener.stop();
    }

    #[test]
    fn code_is_six_base32_chars_and_endpoint_dials_back() {
        let listener = PairingListener::start().expect("start listener");
        assert_eq!(listener.code().len(), CODE_LEN);
        assert_eq!(
            normalize_code(listener.code()).as_deref(),
            Some(listener.code()),
            "generated codes are already canonical"
        );
        // The endpoint is this host's address with a bound port: a
        // connection attempt to it reaches the listener.
        assert!(listener.endpoint().contains(':'));
        // A bare GET-ish probe: wrong method yields 405, proving the
        // endpoint is reachable from this test process.
        let mut stream = TcpStream::connect(listener.endpoint()).expect("dial endpoint");
        let _ = stream.write_all(b"GET /pair HTTP/1.1\r\nHost: x\r\n\r\n");
        let mut buf = [0u8; 128];
        let _ = stream.read(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 405"), "got: {text}");
        assert!(listener.try_event().is_none());
        listener.stop();
    }

    /// One-shot loopback HTTP server for the `request_pairing` tests:
    /// reads a single request, replies `status_line` + JSON `body`, and
    /// hands back what arrived.
    fn serve_once(status_line: &str, body: &str) -> (String, Receiver<Option<Request>>) {
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let endpoint = listener.local_addr().expect("local addr").to_string();
        let (tx, rx) = channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let request = read_request(&mut stream).ok();
            let _ = tx.send(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        (endpoint, rx)
    }

    #[test]
    fn normalize_code_folds_lookalikes_and_rejects_bad_input() {
        assert_eq!(normalize_code("0x1d8c").as_deref(), Some("0X1D8C"));
        assert_eq!(normalize_code("Ox1-d8c").as_deref(), Some("0X1D8C"));
        assert_eq!(normalize_code("OX1DBC").as_deref(), Some("0X1DBC"));
        assert_eq!(normalize_code("lL1iI1").as_deref(), Some("111111"));
        assert_eq!(normalize_code("0X1D8"), None);
        assert_eq!(normalize_code("0X1D8CC"), None);
        assert_eq!(normalize_code("0X1DUC"), None, "U is not in the alphabet");
    }

    #[test]
    fn generated_codes_use_the_alphabet() {
        for _ in 0..50 {
            let code = pair_code();
            assert_eq!(code.len(), CODE_LEN);
            assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)), "{code}");
        }
    }

    #[test]
    fn listener_closes_after_five_wrong_codes() {
        let listener = PairingListener::start().expect("start listener");
        let endpoint = listener.endpoint().to_string();
        let wrong = [("X-Pair-Code", "ZZZZZZ")];
        for _ in 0..MAX_BAD_CODES {
            let (status, _) = post(&endpoint, "/pair", &wrong, &sample_body());
            assert_eq!(status, 403);
        }
        thread::sleep(ACCEPT_POLL * 3);
        let good = [("X-Pair-Code", listener.code())];
        let refused = TcpStream::connect(&endpoint).is_err()
            || post(&endpoint, "/pair", &good, &sample_body()).0 != 200;
        assert!(refused, "port must be closed after the attempt cap");
        // The closure is reported so the gate can regenerate the QR.
        assert_eq!(
            listener.try_event(),
            Some(PairingEvent::Closed {
                paired: false,
                reason: "too many wrong codes",
            })
        );
    }

    #[test]
    fn request_pairing_sends_code_and_maps_wrong_code_and_lockout() {
        let (endpoint, rx) = serve_once(
            "200 OK",
            r#"{"name":"P","deviceId":"p","token":"pairing-token-1"}"#,
        );
        request_pairing(
            &format!("http://{endpoint}"),
            "Mac",
            Some("0X1D8C"),
            READ_TIMEOUT,
        )
        .expect("grant");
        let request = rx.recv().expect("saw request").expect("parsed");
        let body = String::from_utf8_lossy(&request.body).to_string();
        assert!(body.contains("0X1D8C"), "body: {body}");

        let (endpoint, _rx) = serve_once("403 Forbidden", r#"{"error":"bad code"}"#);
        assert_eq!(
            request_pairing(
                &format!("http://{endpoint}"),
                "Mac",
                Some("0X1D8C"),
                READ_TIMEOUT
            ),
            Err(PairRequestError::WrongCode)
        );
        let (endpoint, _rx) = serve_once("429 Too Many Requests", r#"{"error":"locked"}"#);
        assert_eq!(
            request_pairing(
                &format!("http://{endpoint}"),
                "Mac",
                Some("0X1D8C"),
                READ_TIMEOUT
            ),
            Err(PairRequestError::RateLimited)
        );
    }

    #[test]
    fn request_pairing_grants_on_approval() {
        let (endpoint, rx) = serve_once(
            "200 OK",
            r#"{"name":"Pixel 9 Pro","deviceId":"pixel-9","token":"pairing-token-1"}"#,
        );
        let grant = request_pairing(
            &format!("http://{endpoint}"),
            "Nuha's MacBook",
            None,
            READ_TIMEOUT,
        )
        .expect("grant");
        assert_eq!(grant.name, "Pixel 9 Pro");
        assert_eq!(grant.device_id, "pixel-9");
        assert_eq!(grant.token, "pairing-token-1");
        let request = rx.recv().expect("server saw a request").expect("parsed");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/pair-request");
        let body = String::from_utf8_lossy(&request.body).to_string();
        assert!(body.contains("Nuha's MacBook"), "body: {body}");
    }

    #[test]
    fn request_pairing_maps_denial_timeout_and_stale_service() {
        let (endpoint, rx) = serve_once("403 Forbidden", r#"{"error":"denied"}"#);
        assert_eq!(
            request_pairing(&format!("http://{endpoint}"), "Mac", None, READ_TIMEOUT),
            Err(PairRequestError::Denied)
        );
        drop(rx);

        let (endpoint, rx) = serve_once("408 Request Timeout", r#"{"error":"no answer"}"#);
        assert_eq!(
            request_pairing(&format!("http://{endpoint}"), "Mac", None, READ_TIMEOUT),
            Err(PairRequestError::NoAnswer)
        );
        drop(rx);

        let (endpoint, rx) = serve_once("404 Not Found", r#"{"error":"not found"}"#);
        assert!(matches!(
            request_pairing(&format!("http://{endpoint}"), "Mac", None, READ_TIMEOUT),
            Err(PairRequestError::Unreachable(_))
        ));
        drop(rx);
    }

    #[test]
    fn request_pairing_rejects_short_token_grant() {
        let (endpoint, _rx) = serve_once("200 OK", r#"{"name":"P","token":"short"}"#);
        assert!(matches!(
            request_pairing(&format!("http://{endpoint}"), "Mac", None, READ_TIMEOUT),
            Err(PairRequestError::Unreachable(_))
        ));
    }

    #[test]
    fn request_pairing_reports_unreachable_service() {
        // Bind then drop: the port is (almost certainly) closed right after.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = listener.local_addr().expect("local addr").to_string();
        drop(listener);
        assert!(matches!(
            request_pairing(&format!("http://{endpoint}"), "Mac", None, READ_TIMEOUT),
            Err(PairRequestError::Unreachable(_))
        ));
    }
}
