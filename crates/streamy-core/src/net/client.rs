//! Common SourceClient interface and errors.

use thiserror::Error;

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
