//! Common SourceClient interface and errors.

use std::io::Write;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use thiserror::Error;

use crate::core::wire_safety::parse_http_status;

pub const HANDSHAKE_TIMEOUT_MS: u64 = 6_000;
/// Bound on a single socket write while streaming. A stalled server must not pin
/// the client mutex forever.
pub const SEND_TIMEOUT_MS: u64 = 10_000;

/// Resolve `host:port`, mapping lookup failures to a readable error.
pub fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, SourceClientError> {
    let host = host.trim();
    if host.is_empty() {
        return Err(SourceClientError::Protocol("no server address configured".into()));
    }
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| SourceClientError::Resolve(format!("{host}: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(SourceClientError::Resolve(host.to_string()));
    }
    Ok(addrs)
}

/// Try each resolved address (dual-stack hosts may only answer on one) until one connects.
pub fn connect_any(addrs: &[SocketAddr]) -> Result<TcpStream, SourceClientError> {
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(addr, Duration::from_millis(HANDSHAKE_TIMEOUT_MS)) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(SourceClientError::Io(e)),
        }
    }
    Err(last_err.unwrap_or(SourceClientError::Protocol(
        "no addresses to connect".into(),
    )))
}

/// Cloned socket handle that lets another thread unblock a client's in-flight
/// handshake read or stalled write by shutting the socket down. Stop paths use
/// this instead of waiting on the client mutex.
///
/// Also carries an optional custom abort (used by composite clients like the
/// fallback wrapper to fan an abort out to every child).
#[derive(Clone, Default)]
pub struct AbortHandle {
    stream: Arc<Mutex<Option<TcpStream>>>,
    custom: Arc<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl AbortHandle {
    pub fn arm(&self, stream: &TcpStream) {
        *self.stream.lock() = stream.try_clone().ok();
    }

    pub fn disarm(&self) {
        *self.stream.lock() = None;
    }

    pub fn abort(&self) {
        if let Some(f) = self.custom.as_ref() {
            f();
            return;
        }
        if let Some(s) = self.stream.lock().take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Replace the socket shutdown with a custom abort fan-out. Call on a fresh
    /// handle before sharing it (the custom action is fixed at set time).
    pub fn set_abort(&mut self, f: Box<dyn Fn() + Send + Sync>) {
        self.custom = Arc::new(Some(f));
    }
}

#[derive(Error, Debug)]
pub enum SourceClientError {
    #[error("{0}")]
    Protocol(String),
    #[error("cannot resolve server address {0}")]
    Resolve(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not connected")]
    Closed,
    #[error("connection timed out")]
    Timeout,
}

impl SourceClientError {
    /// Non-retryable errors (such as authentication or handshake rejections) are permanent.
    pub fn is_non_retryable(&self) -> bool {
        matches!(self, SourceClientError::Protocol(_))
    }
}

/// Station details announced to the server on connect.
#[derive(Debug, Clone)]
pub struct StationInfo {
    pub url: String,
    pub public: bool,
}

impl StationInfo {
    /// Cheap structural duplicate for composite clients that build several
    /// candidate clients from one profile.
    pub fn dup(&self) -> Self {
        Self { url: self.url.clone(), public: self.public }
    }
}

impl Default for StationInfo {
    fn default() -> Self {
        Self {
            url: "http://streamy.app".into(),
            public: true,
        }
    }
}

pub trait SourceClient: Send + Sync {
    fn connect(&mut self, bitrate_kbps: u32, name: &str, genre: &str) -> Result<(), SourceClientError>;
    fn send(&mut self, frame: &[u8]) -> Result<(), SourceClientError>;
    fn close(&mut self);
    fn update_now_playing(&mut self, title: &str) -> Result<(), SourceClientError> {
        let _ = title;
        Ok(())
    }
    /// Handle that can interrupt blocking I/O without taking the client lock.
    fn abort_handle(&self) -> AbortHandle {
        AbortHandle::default()
    }
    /// Structural duplicate (same config, fresh state). Used by composite
    /// clients that need the same protocol twice (e.g. the same Ultravox client
    /// for two candidate ports).
    fn clone_box(&self) -> Box<dyn SourceClient>;
    /// Where `update_now_playing` reports the server's real verdict (the Weblet's
    /// `Title Updated`, an accepted `/admin/metadata`, or a clean 0x902 write).
    /// A client without its own channel (fallback wrapper) forwards to a child's.
    /// Set by the session BEFORE `connect()`; replay/announce pushes stay
    /// fire-and-forget, so unset is never an error.
    fn set_title_ack(&mut self, ack: TitleAckFn) {
        let _ = ack;
    }
}

/// Callback fired when `update_now_playing` learns the server's real verdict.
pub type TitleAckFn = Arc<dyn Fn(TitleAck) + Send + Sync>;

/// The server's verdict on one now-playing push. `ok` is the SOURCE protocol's
/// real answer, never assumed; a title set while not yet live has no verdict
/// (it is queued and replayed after the handshake, fire-and-forget).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TitleAck {
    pub title: String,
    pub ok: bool,
}

/// One-shot GET result for out-of-band title updates: HTTP status + first body line.
#[derive(Debug, Clone)]
pub struct AdminReply {
    pub code: i32,
    pub body: String,
}

impl AdminReply {
    pub fn ok(&self) -> bool {
        (200..=299).contains(&self.code)
    }
}

/// One-shot admin GET (used for SHOUTcast v1 `admin.cgi` and Icecast
/// `/admin/metadata` title updates). Reads the status line and the first body
/// line only, bounded by a short timeout so a slow admin port cannot hold the
/// caller. Returns a `-1` code on transport errors.
pub fn admin_get(host: &str, port: u16, path_and_query: &str, headers: &[( &str, &str )]) -> AdminReply {
    let addrs = match resolve(host, port) {
        Ok(a) => a,
        Err(_) => return AdminReply { code: -1, body: String::new() },
    };
    let Ok(mut s) = connect_any(&addrs) else {
        return AdminReply { code: -1, body: String::new() };
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(ADMIN_TIMEOUT_MS)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(ADMIN_TIMEOUT_MS)));
    let mut req = format!("GET {path_and_query} HTTP/1.0\r\nHost: {host}:{port}\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("Connection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() || s.flush().is_err() {
        return AdminReply { code: -1, body: String::new() };
    }
    let mut reader = std::io::BufReader::new(s);
    let mut status_line = String::new();
    let _ = std::io::BufRead::read_line(&mut reader, &mut status_line);
    let code = parse_http_status(status_line.trim_end());
    // Header block first, then the (short) body — only what the callers check.
    let mut body = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = std::io::BufRead::read_line(&mut reader, &mut line).unwrap_or(0);
        if n == 0 || line.trim_end_matches(['\r', '\n']).is_empty() {
            break; // EOF or end of headers
        }
    }
    let _ = std::io::BufRead::read_line(&mut reader, &mut body);
    AdminReply { code, body: body.trim_end_matches(['\r', '\n']).to_string() }
}

/// Bounded wait for a title-verdict admin GET.
pub const ADMIN_TIMEOUT_MS: u64 = 3_000;
/// Cap on one handshake/admin response line, same as the Android client.
pub const MAX_LINE: usize = 512;

/// Read one CRLF-terminated ASCII line, bounded by [MAX_LINE] and the socket's
/// read timeout. Used by the SHOUTcast v1 handshake and mock servers.
pub fn read_crlf_line<R: std::io::Read>(input: &mut R) -> Result<String, SourceClientError> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < MAX_LINE {
        match input.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                if byte[0] != b'\r' {
                    buf.push(byte[0]);
                }
            }
            Err(e) => return Err(SourceClientError::Io(e)),
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}
