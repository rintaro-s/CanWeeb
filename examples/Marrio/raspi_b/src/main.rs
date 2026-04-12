/// Marrio - Raspberry Pi B
///
/// STM32 (ロータリーエンコーダ PB0/PB7) から UART でエンコーダ値を読み取り、
/// CanWeeb 経由で PC へ左右移動イベントを送信する。
///
/// 環境変数:
///   CANWEEB_API   - CanWeeb Web API URL (default: http://localhost:8080)
///   SERIAL_PORT   - シリアルポートパス (default: 自動検出)
///   BAUD_RATE     - ボーレート (default: 115200)

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_BAUD: u32 = 115200;
const PC_NODE: &str = "marrio-pc";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let baud = std::env::var("BAUD_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BAUD);
    let port_name = resolve_serial_port(std::env::var("SERIAL_PORT").ok().as_deref())?;

    info!("Marrio RasPi-B 起動");
    info!("CanWeeb API: {}", api);
    info!("Serial: {} @ {} baud", port_name, baud);

    let client = Client::new();

    loop {
        match run_encoder_loop(&client, &api, &port_name, baud).await {
            Ok(()) => {}
            Err(e) => {
                error!("エンコーダループエラー: {:#}", e);
                warn!("3秒後に再接続します...");
                sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

async fn run_encoder_loop(
    client: &Client,
    api: &str,
    port_name: &str,
    baud: u32,
) -> Result<()> {
    let port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(2000))
        .open()
        .with_context(|| format!("シリアルポート {} を開けませんでした", port_name))?;

    info!("シリアルポート接続完了: {} @ {} baud", port_name, baud);

    let mut reader = BufReader::new(port);
    let mut line   = String::new();
    let mut recv_count: u64 = 0;

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

                recv_count += 1;
                // 全受信行をログ出力（STM32 生データ確認用）
                info!("[{:>5}] シリアル受信: {:?}", recv_count, trimmed);

                // STM32 から送られてくるフォーマット: "L" または "R" または "S:<count>"
                match parse_encoder_line(trimmed) {
                    Some((dir, count)) => {
                        info!("  → エンコーダ解析: 方向={} count={} → 送信中...", dir, count);
                        let api_clone    = api.to_string();
                        let client_clone = client.clone();
                        tokio::spawn(async move {
                            match send_move(&client_clone, &api_clone, &dir, count).await {
                                Ok(())  => info!("  ✓ MOVE 送信完了: {}", dir),
                                Err(e)  => error!("  ✗ MOVE 送信失敗: {:#}", e),
                            }
                        });
                    }
                    None => {
                        warn!("  → エンコーダ解析失敗: {:?}", trimmed);
                    }
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

fn parse_encoder_line(line: &str) -> Option<(String, i32)> {
    if line == "L" {
        return Some(("left".to_string(), -1));
    }
    if line == "R" {
        return Some(("right".to_string(), 1));
    }
    // "S:<count>" フォーマット
    if let Some(rest) = line.strip_prefix("S:") {
        if let Ok(count) = rest.parse::<i32>() {
            let dir = if count >= 0 {
                "right".to_string()
            } else {
                "left".to_string()
            };
            return Some((dir, count));
        }
    }
    None
}

async fn send_move(client: &Client, api: &str, direction: &str, count: i32) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let body = json!({
        "target": format!("node:{}", PC_NODE),
        "traffic_class": "control",
        "topic": "marrio/input/move",
        "subject": "move",
        "content_type": "application/json",
        "text": serde_json::to_string(&json!({
            "event": "move",
            "direction": direction,
            "count": count,
            "source": "raspi-b"
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

    // STM32 は /dev/ttyACM* または /dev/ttyUSB* に現れることが多い
    if let Some(p) = ports
        .iter()
        .find(|p| p.port_name.starts_with("/dev/ttyACM") || p.port_name.starts_with("/dev/ttyUSB"))
    {
        return Ok(p.port_name.clone());
    }

    Ok(ports[0].port_name.clone())
}
