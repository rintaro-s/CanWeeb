use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub node: NodeConfig,
    pub storage: StorageConfig,
    pub web: WebConfig,
    pub transport: TransportConfig,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeConfig {
    pub node_id: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageConfig {
    pub root: PathBuf,
    #[serde(default = "default_retention_seconds")]
    pub retention_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WebConfig {
    pub bind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransportConfig {
    pub usb_listen: Option<String>,
    pub wifi_listen: Option<String>,
    #[serde(default = "default_connect_interval_ms")]
    pub connect_interval_ms: u64,
    #[serde(default = "default_heartbeat_interval_ms")]
    pub heartbeat_interval_ms: u64,
    #[serde(default = "default_ack_timeout_ms")]
    pub ack_timeout_ms: u64,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerConfig {
    pub node_id: String,
    pub usb_addr: Option<String>,
    pub wifi_addr: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl AppConfig {
    pub async fn load(path: &Path) -> Result<Self> {
        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let mut config: Self = toml::from_str(&raw).context("failed to parse TOML config")?;
        config.normalize_paths(path);
        Ok(config)
    }

    fn normalize_paths(&mut self, path: &Path) {
        if self.storage.root.is_relative() {
            let base = path.parent().unwrap_or_else(|| Path::new("."));
            self.storage.root = base.join(&self.storage.root);
        }
    }
}

fn default_retention_seconds() -> u64 {
    24 * 60 * 60
}

fn default_connect_interval_ms() -> u64 {
    1_500
}

fn default_heartbeat_interval_ms() -> u64 {
    1_000
}

fn default_ack_timeout_ms() -> u64 {
    2_500
}

fn default_max_hops() -> u8 {
    8
}
