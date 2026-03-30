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
    pub discovery: DiscoveryConfig,
    #[serde(default)]
    pub wifi: WifiConfig,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
    #[serde(skip)]
    pub loaded_from: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeConfig {
    pub node_id: String,
    #[serde(default = "default_node_role")]
    pub role: String,
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
    #[serde(default, alias = "wifi_listen")]
    pub network_listen: Option<String>,
    #[serde(default = "default_connect_interval_ms")]
    pub connect_interval_ms: u64,
    #[serde(default = "default_heartbeat_interval_ms")]
    pub heartbeat_interval_ms: u64,
    #[serde(default = "default_ack_timeout_ms")]
    pub ack_timeout_ms: u64,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct DiscoveryConfig {
    #[serde(default = "default_discovery_enabled")]
    pub enabled: bool,
    #[serde(default = "default_discovery_bind")]
    pub bind: String,
    #[serde(default = "default_discovery_announce_addr")]
    pub announce_addr: String,
    #[serde(default = "default_discovery_announce_interval_ms")]
    pub announce_interval_ms: u64,
    #[serde(default = "default_discovery_peer_ttl_ms")]
    pub peer_ttl_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WifiConfig {
    #[serde(default = "default_wifi_interface")]
    pub interface: String,
    #[serde(default = "default_wifi_auto_manage")]
    pub auto_manage: bool,
    #[serde(default = "default_wifi_ap_ssid")]
    pub hotspot_ssid: String,
    #[serde(default = "default_wifi_ap_password")]
    pub hotspot_password: String,
    #[serde(default = "default_wifi_hotspot_name")]
    pub hotspot_connection_name: String,
    #[serde(default = "default_node_role")]
    pub desired_mode: String,
    #[serde(default = "default_node_status_interval_ms")]
    pub status_interval_ms: u64,
    #[serde(default)]
    pub fallback_networks: Vec<WifiNetworkConfig>,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            interface: default_wifi_interface(),
            auto_manage: default_wifi_auto_manage(),
            hotspot_ssid: default_wifi_ap_ssid(),
            hotspot_password: default_wifi_ap_password(),
            hotspot_connection_name: default_wifi_hotspot_name(),
            desired_mode: default_node_role(),
            status_interval_ms: default_node_status_interval_ms(),
            fallback_networks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WifiNetworkConfig {
    pub ssid: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub priority: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerConfig {
    pub node_id: String,
    pub usb_addr: Option<String>,
    #[serde(default, alias = "wifi_addr")]
    pub network_addr: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default = "default_peer_relationship")]
    pub relationship: String,
    #[serde(default)]
    pub preferred_transport_order: Vec<String>,
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
        config.loaded_from = Some(path.to_path_buf());
        Ok(config)
    }

    #[allow(dead_code)]
    pub async fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self).context("failed to serialize TOML config")?;
        tokio::fs::write(path, raw)
            .await
            .with_context(|| format!("failed to write config file: {}", path.display()))
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

fn default_node_role() -> String {
    "generic".to_string()
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

fn default_discovery_enabled() -> bool {
    true
}

fn default_discovery_bind() -> String {
    "0.0.0.0:7060".to_string()
}

fn default_discovery_announce_addr() -> String {
    "255.255.255.255:7060".to_string()
}

fn default_discovery_announce_interval_ms() -> u64 {
    1_500
}

fn default_discovery_peer_ttl_ms() -> u64 {
    8_000
}

fn default_wifi_interface() -> String {
    "wlan0".to_string()
}

fn default_wifi_auto_manage() -> bool {
    true
}

fn default_wifi_ap_ssid() -> String {
    "CANweeb-Parent".to_string()
}

fn default_wifi_ap_password() -> String {
    "canweeb1234".to_string()
}

fn default_wifi_hotspot_name() -> String {
    "CANweeb Hotspot".to_string()
}

fn default_node_status_interval_ms() -> u64 {
    2_000
}

fn default_peer_relationship() -> String {
    "peer".to_string()
}
