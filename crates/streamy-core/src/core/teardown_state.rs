//! State models and lifecycle teardown resolution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum StreamState {
    Idle,
    Connecting,
    Live { dropped_frames: u64 },
    Reconnecting { attempt: u32 },
    Error { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum RecordState {
    Idle,
    Recording { path: String },
    Error { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    Connecting,
    Reconnecting { attempt: u32 },
    Backoff { attempt: u32, ms: u64 },
    Live,
    Backpressure { drops: u64 },
    Failed { reason: String },
}

/// Resolve stream state on teardown: preserve terminal Error when forced, clear to Idle on user stop.
pub fn resolve_teardown_state(current: &StreamState, preserve_terminal: bool) -> StreamState {
    if preserve_terminal {
        if let StreamState::Error { reason } = current {
            return StreamState::Error {
                reason: reason.clone(),
            };
        }
    }
    StreamState::Idle
}

/// Resolve record state on teardown: preserve terminal Error when forced, clear to Idle on stop.
pub fn resolve_record_teardown_state(current: &RecordState, preserve_terminal: bool) -> RecordState {
    if preserve_terminal {
        if let RecordState::Error { reason } = current {
            return RecordState::Error {
                reason: reason.clone(),
            };
        }
    }
    RecordState::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_is_preserved_when_preserve_terminal_is_true() {
        let err = StreamState::Error {
            reason: "mic_busy".to_string(),
        };
        assert_eq!(resolve_teardown_state(&err, true), err);
    }

    #[test]
    fn user_stop_clears_to_idle() {
        let err = StreamState::Error {
            reason: "mic_busy".to_string(),
        };
        assert_eq!(resolve_teardown_state(&err, false), StreamState::Idle);
    }

    #[test]
    fn idle_stays_idle() {
        assert_eq!(
            resolve_teardown_state(&StreamState::Idle, true),
            StreamState::Idle
        );
        assert_eq!(
            resolve_teardown_state(&StreamState::Idle, false),
            StreamState::Idle
        );
    }
}
