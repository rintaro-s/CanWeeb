use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 物理トランスポートの種別
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Usb,
    Wifi,
}

/// QoS トラフィッククラス
///
/// - `Control`  : ACK あり・永続化あり・再送あり（GPIO 制御、緊急停止、状態遷移）
/// - `Telemetry`: ACK なし・永続化なし・最新値キャッシュ（IMU、姿勢、バッテリ）
/// - `Stream`   : ACK なし・永続化なし・ring buffer（画像、RGB-D、LiDAR）
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrafficClass {
    #[default]
    Control,
    Telemetry,
    Stream,
}

/// 接続開始ハンドシェイク
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelloFrame {
    pub node_id: String,
    pub transport: TransportKind,
    pub capabilities: Vec<String>,
    pub timestamp_ms: u64,
}

/// hop-by-hop ACK（Control クラスのみ）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AckFrame {
    pub message_id: Uuid,
    pub from_node: String,
    pub timestamp_ms: u64,
}

/// リンク生存確認
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PingFrame {
    pub timestamp_ms: u64,
}

/// topic サブスクリプション要求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubscribeFrame {
    pub topics: Vec<String>,
}

/// topic サブスクリプション解除
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnsubscribeFrame {
    pub topics: Vec<String>,
}

/// chunked stream の開始
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamOpenFrame {
    pub stream_id: Uuid,
    pub source_node: String,
    pub topic: String,
    pub content_type: String,
    pub total_chunks: u32,
    pub total_bytes: u64,
    pub timestamp_ms: u64,
}

/// chunked stream の 1 チャンク
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamChunkFrame {
    pub stream_id: Uuid,
    pub chunk_index: u32,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// chunked stream の完了通知
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamCloseFrame {
    pub stream_id: Uuid,
    pub timestamp_ms: u64,
}

/// 配送ターゲット
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeliveryTarget {
    Node(String),
    Nodes(Vec<String>),
    Broadcast,
}

/// メッセージ本体
///
/// `topic` は pub/sub のチャンネル名。空文字列の場合は topic 配送しない。
/// `traffic_class` で永続化・ACK・優先度が決まる。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: Uuid,
    pub source_node: String,
    pub target: DeliveryTarget,
    #[serde(default)]
    pub traffic_class: TrafficClass,
    /// pub/sub トピック名（例: "imu", "cmd/motor", "image/front"）
    #[serde(default)]
    pub topic: String,
    pub subject: String,
    pub content_type: String,
    pub created_at_ms: u64,
    pub ttl: u8,
    pub hops: u8,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

/// ネットワーク上を流れるフレーム
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Frame {
    Hello(HelloFrame),
    Data(Envelope),
    Ack(AckFrame),
    Ping(PingFrame),
    Pong(PingFrame),
    Subscribe(SubscribeFrame),
    Unsubscribe(UnsubscribeFrame),
    StreamOpen(StreamOpenFrame),
    StreamChunk(StreamChunkFrame),
    StreamClose(StreamCloseFrame),
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
