//! Common SourceClient interface and errors.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use thiserror::Error;

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
#[derive(Clone, Default)]
pub struct AbortHandle(Arc<Mutex<Option<TcpStream>>>);

impl AbortHandle {
    pub fn arm(&self, stream: &TcpStream) {
        *self.0.lock() = stream.try_clone().ok();
    }

    pub fn disarm(&self) {
        *self.0.lock() = None;
    }

    pub fn abort(&self) {
        if let Some(s) = self.0.lock().take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
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
}
