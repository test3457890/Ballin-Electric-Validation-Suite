//! Versioned supervisor <-> worker protocol: one JSON object per line.
//! Requests go to the worker's stdin; messages come back on stdout.

use crate::model::{EventLevel, TestResult};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
/// Maximum accepted size of a single protocol line (bounded IPC).
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub protocol: u32,
    pub test_id: String,
    pub params: serde_json::Value,
    /// Fixture mode enables fixture-only tests and labels results as fixtures.
    pub fixture_mode: bool,
    /// The supervisor has verified authorization for high-load tests.
    pub high_load_authorized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessage {
    Hello {
        protocol: u32,
        pid: u32,
        engine_version: String,
    },
    Heartbeat {
        progress: Option<f64>,
        phase: String,
    },
    Log {
        level: EventLevel,
        code: Option<String>,
        message: String,
    },
    Result {
        result: Box<TestResult>,
    },
    Error {
        message: String,
    },
}

impl WorkerMessage {
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("serializable");
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heartbeat_round_trip() {
        let m = WorkerMessage::Heartbeat {
            progress: Some(0.5),
            phase: "pattern 3".into(),
        };
        let l = m.to_line();
        assert!(l.ends_with('\n'));
        let back: WorkerMessage = serde_json::from_str(l.trim()).unwrap();
        assert_eq!(back, m);
    }
}
