/// Marrio - Raspberry Pi A
///
/// Arduino (HC-SR04 超音波センサ) からシリアルで距離データを読み取り、
/// 障害物検知時に CanWeeb 経由で PC へ "jump" イベントを送信する。
///
/// 環境変数:
///   CANWEEB_API   - CanWeeb Web API URL (default: http://localhost:8080)
///   SERIAL_PORT   - シリアルポートパス (default: 自動検出)
///   BAUD_RATE     - ボーレート (default: 9600)
///   JUMP_THRESHOLD_CM - ジャンプ判定距離 cm (default: 30)

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use serialport::SerialPort;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_BAUD: u32 = 9600;
const DEFAULT_JUMP_THRESHOLD_CM: f64 = 30.0;
const JUMP_COOLDOWN_MS: u64 = 800;
const PC_NODE: &str = "marrio-pc";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let baud = std::env::var("BAUD_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BAUD);
    let jump_threshold = std::env::var("JUMP_THRESHOLD_CM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_JUMP_THRESHOLD_CM);

    let port_name = resolve_serial_port(std::env::var("SERIAL_PORT").ok().as_deref())?;

    info!("Marrio RasPi-A 起動");
    info!("CanWeeb API: {}", api);
    info!("Serial: {} @ {} baud", port_name, baud);
    info!("Jump threshold: {} cm", jump_threshold);

    let client = Client::new();

    loop {
        match run_sensor_loop(&client, &api, &port_name, baud, jump_threshold).await {
            Ok(()) => {}
            Err(e) => {
                error!("センサーループエラー: {:#}", e);
                warn!("3秒後に再接続します...");
                sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

async fn run_sensor_loop(
    client: &Client,
    api: &str,
    port_name: &str,
    baud: u32,
    jump_threshold: f64,
) -> Result<()> {
    let port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(2000))
        .open()
        .with_context(|| format!("シリアルポート {} を開けませんでした", port_name))?;

    info!("シリアルポート接続完了: {}", port_name);

    let mut reader = BufReader::new(port);
    let mut line = String::new();
    let mut last_jump_at = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                warn!("シリアルポートがクローズされました");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(distance_cm) = trimmed.parse::<f64>() {
                    let elapsed = last_jump_at.elapsed().as_millis() as u64;

                    if distance_cm < jump_threshold && elapsed >= JUMP_COOLDOWN_MS {
                        info!("障害物検知: {:.1} cm → ジャンプ送信", distance_cm);
                        last_jump_at = Instant::now();

                        let api_clone = api.to_string();
                        let client_clone = client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = send_jump(&client_clone, &api_clone, distance_cm).await {
                                error!("jump 送信失敗: {:#}", e);
                            }
                        });
                    }
                } else {
                    warn!("パース失敗: '{}'", trimmed);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => {
                return Err(anyhow::anyhow!("シリアル読み取りエラー: {}", e));
            }
        }
    }

    Ok(())
}

async fn send_jump(client: &Client, api: &str, distance_cm: f64) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let body = json!({
        "target": format!("node:{}", PC_NODE),
        "traffic_class": "telemetry",
        "topic": "marrio/input/jump",
        "subject": "jump",
        "content_type": "application/json",
        "text": serde_json::to_string(&json!({
            "event": "jump",
            "distance_cm": distance_cm,
            "source": "raspi-a"
        }))?,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("HTTP送信失敗")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("API エラー {}: {}", status, text));
    }

    Ok(())
}

fn resolve_serial_port(explicit: Option<&str>) -> Result<String> {
    if let Some(port) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(port.to_string());
    }

    let ports = serialport::available_ports().context("シリアルポート列挙失敗")?;
    if ports.is_empty() {
        return Err(anyhow::anyhow!(
            "シリアルポートが見つかりません。SERIAL_PORT 環境変数で指定してください"
        ));
    }

    if let Some(p) = ports
        .iter()
        .find(|p| p.port_name.starts_with("/dev/ttyACM") || p.port_name.starts_with("/dev/ttyUSB"))
    {
        return Ok(p.port_name.clone());
    }

    Ok(ports[0].port_name.clone())
}
