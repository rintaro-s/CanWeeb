use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Deserialize, Debug)]
struct InboxItem {
    message_id: String,
    subject: String,
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
    let serial_port = std::env::var("SERIAL_PORT").unwrap_or_else(|_| "/dev/ttyACM0".to_string());
    let baud_rate = std::env::var("BAUD_RATE")
        .unwrap_or_else(|_| "9600".to_string())
        .parse::<u32>()
        .unwrap_or(9600);

    info!("Child Serial Daemon 起動");
    info!("CANweeb API: {}", canweeb_api);
    info!("Serial Port: {} @ {} baud", serial_port, baud_rate);

    let client = reqwest::Client::new();
    let mut processed_ids = std::collections::HashSet::<String>::new();

    loop {
        match poll_and_process(&client, &canweeb_api, &serial_port, baud_rate, &mut processed_ids).await {
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

    for item in items {
        if item.subject != "led_control" {
            continue;
        }
        if processed_ids.contains(&item.message_id) {
            continue;
        }

        info!("新規 led_control メッセージ: {}", item.message_id);

        // 詳細取得
        let detail_url = format!("{}/api/inbox/{}", api_base, item.message_id);
        let detail: InboxDetail = match client.get(&detail_url).send().await {
            Ok(res) => match res.json().await {
                Ok(d) => d,
                Err(e) => {
                    warn!("failed to parse detail: {:#}", e);
                    continue;
                }
            },
            Err(e) => {
                warn!("failed to fetch detail: {:#}", e);
                continue;
            }
        };

        let command = detail.text.trim().to_lowercase();
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

        // 処理済み ID が 1000 件を超えたら古いものを削除
        if processed_ids.len() > 1000 {
            let to_remove: Vec<String> = processed_ids.iter().take(500).cloned().collect();
            for id in to_remove {
                processed_ids.remove(&id);
            }
        }
    }

    Ok(())
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
