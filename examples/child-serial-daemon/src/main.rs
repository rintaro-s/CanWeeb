use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serialport::{available_ports, SerialPortType};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Deserialize, Debug)]
struct InboxItem {
    message_id: String,
    subject: String,
    #[serde(default)]
    created_at_ms: u64,
    #[serde(default)]
    received_at_ms: u64,
    #[serde(default)]
    preview: String,
}

#[derive(Deserialize, Debug)]
struct InboxDetail {
    #[serde(default)]
    text: String,
    #[serde(default)]
    payload_base64: String,
}

/// CANweeb inbox をポーリングして led_control メッセージを処理する
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let canweeb_api = std::env::var("CANWEEB_API").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let serial_port = resolve_serial_port(std::env::var("SERIAL_PORT").ok().as_deref())?;
    let baud_rate = std::env::var("BAUD_RATE")
        .unwrap_or_else(|_| "9600".to_string())
        .parse::<u32>()
        .unwrap_or(9600);

    info!("Child Serial Daemon 起動");
    info!("CANweeb API: {}", canweeb_api);
    info!("Serial Port: {} @ {} baud", serial_port, baud_rate);

    let client = reqwest::Client::new();
    let mut processed_ids = std::collections::HashSet::<String>::new();
    let mut newest_seen_ms = initial_inbox_watermark(&client, &canweeb_api).await.unwrap_or(0);

    loop {
        match poll_and_process(
            &client,
            &canweeb_api,
            &serial_port,
            baud_rate,
            &mut processed_ids,
            &mut newest_seen_ms,
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                warn!("Poll error: {:#}", e);
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
}

async fn poll_and_process(
    client: &reqwest::Client,
    api_base: &str,
    serial_port: &str,
    baud_rate: u32,
    processed_ids: &mut std::collections::HashSet<String>,
    newest_seen_ms: &mut u64,
) -> Result<()> {
    // inbox 一覧取得
    let inbox_url = format!("{}/api/inbox", api_base);
    let items: Vec<InboxItem> = client
        .get(&inbox_url)
        .send()
        .await
        .context("failed to fetch inbox")?
        .json()
        .await
        .context("failed to parse inbox")?;

    let selected = items
        .into_iter()
        .filter(|item| item.subject == "led_control")
        .filter(|item| !processed_ids.contains(&item.message_id))
        .filter(|item| item.received_at_ms > *newest_seen_ms)
        .max_by_key(|item| (item.received_at_ms, item.created_at_ms));

    let Some(item) = selected else {
        return Ok(());
    };

    info!("新規 led_control メッセージ: {}", item.message_id);

        // 詳細取得
    let detail_url = format!("{}/api/inbox/{}", api_base, item.message_id);
    let detail: InboxDetail = match client.get(&detail_url).send().await {
        Ok(res) => match res.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to parse detail: {:#}", e);
                return Ok(());
            }
        },
        Err(e) => {
            warn!("failed to fetch detail: {:#}", e);
            return Ok(());
        }
    };

    let command = extract_command(&detail).unwrap_or_default();
    info!("LED コマンド: {}", command);

    match command.as_str() {
        "on" | "off" => {
            if let Err(e) = send_serial(serial_port, baud_rate, &command) {
                error!("Serial write error: {:#}", e);
            } else {
                info!("✅ Serial sent: {}", command);
            }
        }
        _ => {
            warn!("Unknown command: {}", command);
        }
    }

    processed_ids.insert(item.message_id.clone());
    *newest_seen_ms = (*newest_seen_ms).max(item.received_at_ms.max(item.created_at_ms));

    // 処理済み ID が 1000 件を超えたら古いものを削除
    if processed_ids.len() > 1000 {
        let to_remove: Vec<String> = processed_ids.iter().take(500).cloned().collect();
        for id in to_remove {
            processed_ids.remove(&id);
        }
    }

    Ok(())
}

async fn initial_inbox_watermark(client: &reqwest::Client, api_base: &str) -> Result<u64> {
    let inbox_url = format!("{}/api/inbox", api_base);
    let items: Vec<InboxItem> = client
        .get(&inbox_url)
        .send()
        .await
        .context("failed to fetch inbox for initial watermark")?
        .json()
        .await
        .context("failed to parse inbox for initial watermark")?;

    Ok(items
        .into_iter()
        .filter(|item| item.subject == "led_control")
        .map(|item| item.received_at_ms.max(item.created_at_ms))
        .max()
        .unwrap_or(0))
}

fn extract_command(detail: &InboxDetail) -> Option<String> {
    let text = detail.text.trim();
    if !text.is_empty() {
        return Some(text.to_lowercase());
    }

    if detail.payload_base64.trim().is_empty() {
        return None;
    }

    let decoded = STANDARD.decode(detail.payload_base64.trim()).ok()?;
    let decoded = String::from_utf8_lossy(&decoded);
    let command = decoded.trim().to_lowercase();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

fn resolve_serial_port(explicit: Option<&str>) -> Result<String> {
    if let Some(port) = explicit.map(str::trim).filter(|port| !port.is_empty()) {
        if Path::new(port).exists() {
            return Ok(port.to_string());
        }
        return Err(anyhow!("SERIAL_PORT was set but does not exist: {}", port));
    }

    let ports = available_ports().context("failed to enumerate serial ports")?;
    if ports.is_empty() {
        return Err(anyhow!("no serial ports detected; set SERIAL_PORT explicitly"));
    }

    if let Some(port) = ports.iter().find(|port| is_stlink_port(port)) {
        return Ok(port.port_name.clone());
    }

    if let Some(port) = ports.iter().find(|port| is_preferred_tty_port(&port.port_name)) {
        return Ok(port.port_name.clone());
    }

    if let Some(port) = ports.first() {
        warn!("ST-Link compatible port was not detected; using first available serial port: {}", port.port_name);
        return Ok(port.port_name.clone());
    }

    Err(anyhow!("no usable serial ports detected"))
}

fn is_stlink_port(port: &serialport::SerialPortInfo) -> bool {
    let name = port.port_name.to_ascii_lowercase();
    if name.contains("stlink") || name.contains("st-link") {
        return true;
    }

    match &port.port_type {
        SerialPortType::UsbPort(info) => {
            info.manufacturer
                .as_deref()
                .is_some_and(|value| value.to_ascii_lowercase().contains("stmicro"))
                || info.product.as_deref().is_some_and(|value| {
                    let value = value.to_ascii_lowercase();
                    value.contains("stlink") || value.contains("st-link")
                })
        }
        _ => false,
    }
}

fn is_preferred_tty_port(port_name: &str) -> bool {
    port_name.starts_with("/dev/ttyACM") || port_name.starts_with("/dev/ttyUSB")
}

fn send_serial(port_name: &str, baud_rate: u32, command: &str) -> Result<()> {
    let mut port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("failed to open serial port {}", port_name))?;

    writeln!(port, "{}", command).context("failed to write to serial")?;
    port.flush().context("failed to flush serial")?;

    Ok(())
}
