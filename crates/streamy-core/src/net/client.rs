//! Common SourceClient interface and errors.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use thiserror::Error;

pub const HANDSHAKE_TIMEOUT_MS: u64 = 6_000;

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

#[derive(Error, Debug)]
pub enum SourceClientError {
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Client closed")]
    Closed,
    #[error("Connection timed out")]
    Timeout,
}

impl SourceClientError {
    /// Non-retryable errors (such as authentication or handshake rejections) are permanent.
    pub fn is_non_retryable(&self) -> bool {
        matches!(self, SourceClientError::Protocol(_))
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
}
