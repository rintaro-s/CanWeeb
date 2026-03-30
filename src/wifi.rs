use crate::config::{WifiConfig, WifiNetworkConfig};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tokio::process::Command;

#[derive(Clone, Debug, Serialize)]
pub struct WifiStatus {
    pub manager_available: bool,
    pub interface: String,
    pub device_state: String,
    pub active_connection: Option<String>,
    pub active_ssid: Option<String>,
    pub mode: String,
    pub ipv4_addresses: Vec<String>,
    pub scanned_networks: Vec<ScannedWifiNetwork>,
    pub fallback_networks: Vec<FallbackWifiNetwork>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScannedWifiNetwork {
    pub active: bool,
    pub ssid: String,
    pub signal: Option<u8>,
    pub security: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FallbackWifiNetwork {
    pub ssid: String,
    pub priority: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct WifiActionResponse {
    pub ok: bool,
    pub action: String,
    pub details: Vec<String>,
    pub status: WifiStatus,
}

pub async fn collect_status(config: &WifiConfig) -> WifiStatus {
    let device = run_command("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device", "status"]).await;
    let wifi_list = run_command(
        "nmcli",
        &[
            "-t",
            "-f",
            "ACTIVE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "ifname",
            &config.interface,
        ],
    )
    .await;
    let ip_output = run_command("ip", &["-o", "-4", "addr", "show", "dev", &config.interface]).await;

    let fallback_networks = config
        .fallback_networks
        .iter()
        .map(|network| FallbackWifiNetwork {
            ssid: network.ssid.clone(),
            priority: network.priority,
        })
        .collect::<Vec<_>>();

    let Ok(device) = device else {
        return WifiStatus {
            manager_available: false,
            interface: config.interface.clone(),
            device_state: "unavailable".to_string(),
            active_connection: None,
            active_ssid: None,
            mode: "unknown".to_string(),
            ipv4_addresses: Vec::new(),
            scanned_networks: Vec::new(),
            fallback_networks,
        };
    };

    let mut device_state = "unknown".to_string();
    let mut active_connection = None;
    for line in device.lines() {
        let mut parts = line.splitn(4, ':');
        let device_name = parts.next().unwrap_or_default();
        let device_type = parts.next().unwrap_or_default();
        let state = parts.next().unwrap_or_default();
        let connection = parts.next().unwrap_or_default();
        if device_name == config.interface && device_type == "wifi" {
            device_state = state.to_string();
            if !connection.is_empty() && connection != "--" {
                active_connection = Some(connection.to_string());
            }
            break;
        }
    }

    let scanned_networks = wifi_list
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            if line.is_empty() {
                return None;
            }
            let mut parts = line.splitn(4, ':');
            let active = parts.next().unwrap_or_default() == "yes";
            let ssid = parts.next().unwrap_or_default().to_string();
            let signal = parts.next().and_then(|v| v.parse::<u8>().ok());
            let security = parts.next().unwrap_or_default().to_string();
            if ssid.is_empty() {
                return None;
            }
            Some(ScannedWifiNetwork {
                active,
                ssid,
                signal,
                security,
            })
        })
        .collect::<Vec<_>>();

    let active_ssid = scanned_networks.iter().find(|item| item.active).map(|item| item.ssid.clone());
    let mode = if active_connection.as_deref() == Some(config.hotspot_connection_name.as_str())
        || active_ssid.as_deref() == Some(config.hotspot_ssid.as_str())
    {
        "ap".to_string()
    } else if active_ssid.is_some() {
        "client".to_string()
    } else {
        "idle".to_string()
    };

    let ipv4_addresses = ip_output
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().nth(3).map(|addr| addr.to_string()))
        .collect::<Vec<_>>();

    WifiStatus {
        manager_available: true,
        interface: config.interface.clone(),
        device_state,
        active_connection,
        active_ssid,
        mode,
        ipv4_addresses,
        scanned_networks,
        fallback_networks,
    }
}

pub async fn apply_mode(config: &WifiConfig, requested_mode: Option<&str>) -> Result<WifiActionResponse> {
    let mode = requested_mode.unwrap_or(config.desired_mode.as_str());
    let mut details = Vec::new();
    match mode {
        "parent" | "ap" | "hotspot" => {
            start_hotspot(config, &mut details).await?;
        }
        "child" | "client" => {
            connect_best_fallback(config, &mut details).await?;
        }
        _ => {
            details.push(format!("mode '{mode}' does not trigger Wi-Fi automation"));
        }
    }
    let status = collect_status(config).await;
    Ok(WifiActionResponse {
        ok: true,
        action: format!("apply:{mode}"),
        details,
        status,
    })
}

pub async fn start_hotspot_now(config: &WifiConfig) -> Result<WifiActionResponse> {
    let mut details = Vec::new();
    start_hotspot(config, &mut details).await?;
    let status = collect_status(config).await;
    Ok(WifiActionResponse {
        ok: true,
        action: "hotspot:start".to_string(),
        details,
        status,
    })
}

pub async fn connect_ssid_now(config: &WifiConfig, ssid: &str, password: Option<&str>) -> Result<WifiActionResponse> {
    let mut details = Vec::new();
    connect_ssid(config, ssid, password, &mut details).await?;
    let status = collect_status(config).await;
    Ok(WifiActionResponse {
        ok: true,
        action: format!("connect:{ssid}"),
        details,
        status,
    })
}

pub async fn disconnect_now(config: &WifiConfig) -> Result<WifiActionResponse> {
    let mut details = Vec::new();
    run_command("nmcli", &["device", "disconnect", &config.interface])
        .await
        .context("failed to disconnect Wi-Fi device")?;
    details.push(format!("disconnected {}", config.interface));
    let status = collect_status(config).await;
    Ok(WifiActionResponse {
        ok: true,
        action: "disconnect".to_string(),
        details,
        status,
    })
}

async fn start_hotspot(config: &WifiConfig, details: &mut Vec<String>) -> Result<()> {
    run_command(
        "nmcli",
        &[
            "device",
            "wifi",
            "hotspot",
            "ifname",
            &config.interface,
            "con-name",
            &config.hotspot_connection_name,
            "ssid",
            &config.hotspot_ssid,
            "password",
            &config.hotspot_password,
        ],
    )
    .await
    .with_context(|| format!("failed to start hotspot on {}", config.interface))?;
    details.push(format!("hotspot started: {}", config.hotspot_ssid));
    Ok(())
}

async fn connect_best_fallback(config: &WifiConfig, details: &mut Vec<String>) -> Result<()> {
    let mut networks = config.fallback_networks.clone();
    networks.sort_by(|left, right| right.priority.cmp(&left.priority).then_with(|| left.ssid.cmp(&right.ssid)));
    let mut last_error = None;
    for network in networks {
        match connect_ssid(config, &network.ssid, network.password.as_deref(), details).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                details.push(format!("connect failed: {} ({error})", network.ssid));
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("no fallback Wi-Fi networks configured")))
}

async fn connect_ssid(config: &WifiConfig, ssid: &str, password: Option<&str>, details: &mut Vec<String>) -> Result<()> {
    let mut args = vec![
        "device".to_string(),
        "wifi".to_string(),
        "connect".to_string(),
        ssid.to_string(),
        "ifname".to_string(),
        config.interface.clone(),
    ];
    if let Some(password) = password {
        args.push("password".to_string());
        args.push(password.to_string());
    }
    let refs = args.iter().map(|item| item.as_str()).collect::<Vec<_>>();
    run_command("nmcli", &refs)
        .await
        .with_context(|| format!("failed to connect {} to {ssid}", config.interface))?;
    details.push(format!("connected to {ssid}"));
    Ok(())
}

async fn run_command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to spawn {program}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{} {:?} failed: {}",
            program,
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn fallback_password<'a>(networks: &'a [WifiNetworkConfig], ssid: &str) -> Option<&'a str> {
    networks
        .iter()
        .find(|network| network.ssid == ssid)
        .and_then(|network| network.password.as_deref())
}
