//! A tiny blocking HTTP/1.1 stub server for transport unit tests —
//! no extra dependencies, just `std::net` and a handler closure.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Parsed inbound request (header names lowercased).
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Path without the query string (still percent-encoded).
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Stub response.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

impl Response {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            body: serde_json::to_vec(&value).expect("serialize stub body"),
            content_type: "application/json",
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "text/plain",
        }
    }

    pub fn bytes(status: u16, body: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            status,
            body,
            content_type,
        }
    }
}

/// A running stub server; stops when dropped.
pub struct StubServer {
    port: u16,
    running: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Request>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl StubServer {
    /// Boots the server on an ephemeral loopback port.
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
        let port = listener.local_addr().expect("stub addr").port();
        let running = Arc::new(AtomicBool::new(true));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let thread_running = Arc::clone(&running);
        let thread_requests = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if !thread_running.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let handler = Arc::clone(&handler);
                let captured = Arc::clone(&thread_requests);
                std::thread::spawn(move || {
                    if let Err(e) = serve(stream, handler.as_ref(), &captured) {
                        eprintln!("stub connection error: {e}");
                    }
                });
            }
        });
        Self {
            port,
            running,
            requests,
            handle: Mutex::new(Some(handle)),
        }
    }

    /// Base URL (`http://127.0.0.1:{port}`).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Every request the stub has served so far (for assertions).
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("stub request log").clone()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Wake the blocking accept() loop; the EOF connection is discarded.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.lock().expect("stub handle").take() {
            let _ = handle.join();
        }
    }
}

fn serve(
    mut stream: TcpStream,
    handler: &dyn Fn(&Request) -> Response,
    captured: &Mutex<Vec<Request>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    let (path, query) = split_target(&target);
    let request = Request {
        method,
        path,
        query,
        headers,
        body,
    };
    captured
        .lock()
        .expect("stub request log")
        .push(request.clone());
    let response = handler(&request);

    let status_text = status_text(response.status);
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.body.len(),
        response.content_type
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn split_target(target: &str) -> (String, HashMap<String, String>) {
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q),
        None => return (percent_decode(target), HashMap::new()),
    };
    let mut map = HashMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(percent_decode(key), percent_decode(value));
    }
    (percent_decode(&path), map)
}

/// Minimal percent-decoding for query values (`%2F`, `+`, …).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).unwrap_or_default();
                if hex.len() == 2 {
                    let high = (hex[0] as char).to_digit(16);
                    let low = (hex[1] as char).to_digit(16);
                    if let (Some(high), Some(low)) = (high, low) {
                        out.push((high * 16 + low) as u8);
                        i += 3;
                        continue;
                    }
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}
