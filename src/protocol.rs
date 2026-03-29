use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Usb,
    Wifi,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrafficClass {
    #[default]
    Control,
    Telemetry,
    Stream,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelloFrame {
    pub node_id: String,
    pub transport: TransportKind,
    pub capabilities: Vec<String>,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AckFrame {
    pub message_id: Uuid,
    pub from_node: String,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PingFrame {
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeliveryTarget {
    Node(String),
    Nodes(Vec<String>),
    Broadcast,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: Uuid,
    pub source_node: String,
    pub target: DeliveryTarget,
    #[serde(default)]
    pub traffic_class: TrafficClass,
    pub subject: String,
    pub content_type: String,
    pub created_at_ms: u64,
    pub ttl: u8,
    pub hops: u8,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Frame {
    Hello(HelloFrame),
    Data(Envelope),
    Ack(AckFrame),
    Ping(PingFrame),
    Pong(PingFrame),
}

impl TrafficClass {
    pub fn parse(input: &str) -> Result<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "control" => Ok(Self::Control),
            "telemetry" => Ok(Self::Telemetry),
            "stream" => Ok(Self::Stream),
            other => Err(anyhow!("unknown traffic_class: {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Telemetry => "telemetry",
            Self::Stream => "stream",
        }
    }

    pub fn requires_ack(self) -> bool {
        matches!(self, Self::Control)
    }

    pub fn should_persist_queue(self) -> bool {
        matches!(self, Self::Control)
    }

    pub fn should_persist_inbox(self) -> bool {
        matches!(self, Self::Control)
    }

    pub fn should_store_inbox(self) -> bool {
        !matches!(self, Self::Stream)
    }

    pub fn dispatch_priority(self) -> u8 {
        match self {
            Self::Control => 0,
            Self::Telemetry => 1,
            Self::Stream => 2,
        }
    }
}

impl DeliveryTarget {
    pub fn parse(input: &str) -> Result<Self> {
        let normalized = input.trim();
        if normalized.eq_ignore_ascii_case("broadcast") {
            return Ok(Self::Broadcast);
        }
        if let Some(rest) = normalized.strip_prefix("node:") {
            let node = rest.trim();
            if node.is_empty() {
                return Err(anyhow!("target node is empty"));
            }
            return Ok(Self::Node(node.to_string()));
        }
        if let Some(rest) = normalized.strip_prefix("nodes:") {
            let nodes = rest
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            if nodes.is_empty() {
                return Err(anyhow!("target node list is empty"));
            }
            return Ok(Self::Nodes(nodes));
        }
        Ok(Self::Node(normalized.to_string()))
    }

    pub fn matches(&self, node_id: &str) -> bool {
        match self {
            Self::Node(node) => node == node_id,
            Self::Nodes(nodes) => nodes.iter().any(|node| node == node_id),
            Self::Broadcast => true,
        }
    }

    pub fn requires_forwarding_after(&self, node_id: &str) -> bool {
        match self {
            Self::Node(node) => node != node_id,
            Self::Nodes(nodes) => nodes.iter().any(|node| node != node_id),
            Self::Broadcast => true,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Node(node) => format!("node:{node}"),
            Self::Nodes(nodes) => format!("nodes:{}", nodes.join(",")),
            Self::Broadcast => "broadcast".to_string(),
        }
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn payload_preview(content_type: &str, payload: &[u8]) -> String {
    if content_type.starts_with("text/") || content_type.contains("json") {
        match std::str::from_utf8(payload) {
            Ok(text) => text.chars().take(160).collect(),
            Err(_) => format!("<invalid utf8: {} bytes>", payload.len()),
        }
    } else {
        format!("<binary: {} bytes>", payload.len())
    }
}
