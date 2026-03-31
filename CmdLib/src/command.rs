use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub id: Uuid,
    pub issued_at_ms: u64,
    pub domain: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

impl CommandEnvelope {
    pub fn new(domain: impl Into<String>, action: impl Into<String>, args: Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            issued_at_ms: now_ms(),
            domain: domain.into(),
            action: action.into(),
            args,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub id: Uuid,
    pub success: bool,
    pub message: String,
    pub data: Value,
    pub handled_at_ms: u64,
}

impl CommandResult {
    pub fn ok(id: Uuid, message: impl Into<String>) -> Self {
        Self {
            id,
            success: true,
            message: message.into(),
            data: Value::Null,
            handled_at_ms: now_ms(),
        }
    }

    pub fn ok_with_data(id: Uuid, message: impl Into<String>, data: Value) -> Self {
        Self {
            id,
            success: true,
            message: message.into(),
            data,
            handled_at_ms: now_ms(),
        }
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
