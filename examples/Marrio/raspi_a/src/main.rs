/// Marrio - Raspberry Pi A (Arduino シリアル連携版)
///
/// Arduino から HC-SR04 超音波センサの距離データをシリアル経由で受信し、
/// ジャンプイベントを CanWeeb 経由で送信する。
///
/// 環境変数:
///   CANWEEB_API       - CanWeeb Web API URL (default: http://localhost:8080)
///   SERIAL_PORT       - シリアルポートパス (default: 自動検出)
///   BAUD_RATE         - ボーレート (default: 9600)
///   JUMP_THRESHOLD_CM - ジャンプ判定距離 cm (default: 30)

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_BAUD: u32 = 9600;
const DEFAULT_JUMP_THRESHOLD_CM: f64 = 30.0;
const JUMP_COOLDOWN_MS: u64 = 800;
const PC_NODE: &str = "marrio-pc";
const NO_DATA_WARN_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let baud = std::env::var("BAUD_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BAUD);
    let jump_threshold = std::env::var("JUMP_THRESHOLD_CM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_JUMP_THRESHOLD_CM);
    let explicit_port = std::env::var("SERIAL_PORT").ok();

    info!("====================================================");
    info!("  Marrio RasPi-A  超音波センサ (Arduino 連携)");
    info!("====================================================");
    info!("  CANWEEB_API       = {}", api);
    info!("  BAUD_RATE         = {} bps", baud);
    info!("  JUMP_THRESHOLD_CM = {} cm", jump_threshold);
    info!("  JUMP_COOLDOWN_MS  = {} ms", JUMP_COOLDOWN_MS);
    info!("  SERIAL_PORT       = {}", explicit_port.as_deref().unwrap_or("(自動検出)"));
    info!("====================================================");

    list_serial_ports();

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;

    check_canweeb(&client, &api).await;

    let port_name = resolve_serial_port(explicit_port.as_deref())?;
    info!("使用シリアルポート: {}", port_name);

    let mut attempt = 0u32;
    loop {
        attempt += 1;
        info!("");
        info!("──── [接続試行 #{attempt}] {} @ {} ────", port_name, baud);
        match run_sensor_loop(&client, &api, &port_name, baud, jump_threshold).await {
            Ok(()) => info!("センサーループ終了"),
            Err(e) => error!("センサーループエラー: {:#}", e),
        }
        warn!("5秒後に再接続... (試行 #{attempt})");
        sleep(Duration::from_secs(5)).await;
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
        .timeout(Duration::from_millis(3000))
        .open()
        .with_context(|| {
            format!(
                "シリアルポート {} を開けません (baud={})。\n\
                 Arduino が接続されているか、ポート名が正しいか確認してください。",
                port_name, baud
            )
        })?;

    info!("━━━ {} @ {} baud オープン成功 ━━━", port_name, baud);

    let mut reader = BufReader::new(port);
    let mut line = String::new();
    let mut last_jump_at: Option<Instant> = None;
    let mut total_lines = 0u64;
    let mut valid_count = 0u64;
    let mut error_count = 0u64;
    let mut last_data_at = Instant::now();
    let mut last_stats_at = Instant::now();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                warn!("シリアルポートが EOF を返しました（デバイス切断）");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                total_lines += 1;
                last_data_at = Instant::now();

                if total_lines <= 20 {
                    info!("[起動確認 raw #{:>4}] {:?}", total_lines, trimmed);
                }

                if trimmed == "-1" {
                    error_count += 1;
                    if total_lines <= 20 {
                        info!("  → センサータイムアウト値(-1)。HC-SR04 の配線を確認してください");
                    }
                    continue;
                }

                match trimmed.parse::<f64>() {
                    Ok(dist) if dist > 0.0 && dist < 500.0 => {
                        valid_count += 1;

                        let cooldown_ok = last_jump_at
                            .map_or(true, |t| t.elapsed().as_millis() as u64 >= JUMP_COOLDOWN_MS);

                        if valid_count <= 100 || valid_count % 50 == 0 {
                            info!(
                                "[測定 #{:>6}] {:>8.2} cm   閾値:{:.1} cm   jump_ready:{}",
                                valid_count, dist, jump_threshold, cooldown_ok
                            );
                        }

                        if dist < jump_threshold && cooldown_ok {
                            info!(
                                "★★★ JUMP 検知! {:.2} cm < {} cm → CanWeeb 送信中...",
                                dist, jump_threshold
                            );
                            last_jump_at = Some(Instant::now());

                            let api_c = api.to_string();
                            let cli_c = client.clone();
                            tokio::spawn(async move {
                                match send_jump(&cli_c, &api_c, dist).await {
                                    Ok(()) => info!("  ✓ JUMP 送信完了"),
                                    Err(e) => error!("  ✗ JUMP 送信失敗: {:#}", e),
                                }
                            });
                        }
                    }
                    Ok(dist) => {
                        warn!("[#{:>4}] 範囲外値: {:.2} cm（無視）", total_lines, dist);
                    }
                    Err(_) => {
                        warn!("[#{:>4}] パース失敗: {:?}", total_lines, trimmed);
                        error_count += 1;
                    }
                }

                if last_stats_at.elapsed() >= Duration::from_secs(1) {
                    info!(
                        "  [統計] 受信行:{} 有効:{} エラー:{}",
                        total_lines, valid_count, error_count
                    );
                    last_stats_at = Instant::now();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                let secs = last_data_at.elapsed().as_secs();
                if secs >= NO_DATA_WARN_SECS {
                    warn!(
                        "⚠⚠⚠ {} 秒間データなし！Arduino が動いているか確認してください ⚠⚠⚠",
                        secs
                    );
                }
                continue;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "シリアル読み取りエラー: {} (kind={:?})",
                    e,
                    e.kind()
                ));
            }
        }
    }
    Ok(())
}

async fn send_jump(client: &Client, api: &str, distance_cm: f64) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let payload = json!({
        "event":       "jump",
        "distance_cm": distance_cm,
        "source":      "raspi-a",
    });
    let body = json!({
        "target":        format!("node:{}", PC_NODE),
        "traffic_class": "control",
        "topic":         "marrio/input/jump",
        "subject":       "jump",
        "content_type":  "application/json",
        "text":          payload.to_string(),
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .with_context(|| format!("HTTP POST {} 失敗", url))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("CanWeeb API エラー {} : {}", status, text));
    }
    Ok(())
}

fn list_serial_ports() {
    info!("─── 利用可能なシリアルポート ───────────────────────");
    match serialport::available_ports() {
        Err(e) => error!("列挙失敗: {}", e),
        Ok(ports) if ports.is_empty() => {
            warn!("シリアルポートが見つかりません。");
            warn!("Arduino が USB 接続されているか確認してください。");
        }
        Ok(ports) => {
            for (i, p) in ports.iter().enumerate() {
                let detail = match &p.port_type {
                    serialport::SerialPortType::UsbPort(u) => format!(
                        "USB  vid={:04x} pid={:04x}  mfr={:<20} prod={}",
                        u.vid,
                        u.pid,
                        u.manufacturer.as_deref().unwrap_or("?"),
                        u.product.as_deref().unwrap_or("?"),
                    ),
                    serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                    serialport::SerialPortType::PciPort => "PCI".to_string(),
                    serialport::SerialPortType::Unknown => "Unknown".to_string(),
                };
                info!("  [{i}] {}   ({detail})", p.port_name);
            }
        }
    }
    info!("────────────────────────────────────────────────────");
}

fn resolve_serial_port(explicit: Option<&str>) -> Result<String> {
    if let Some(p) = explicit {
        return Ok(p.to_string());
    }

    let ports = serialport::available_ports().context("シリアルポート列挙失敗")?;
    for p in &ports {
        if let serialport::SerialPortType::UsbPort(_) = p.port_type {
            info!("  → 自動検出: {}", p.port_name);
            return Ok(p.port_name.clone());
        }
    }

    Err(anyhow::anyhow!(
        "USB シリアルポートが見つかりません。SERIAL_PORT 環境変数で明示してください。"
    ))
}

async fn check_canweeb(client: &Client, api: &str) {
    info!("─── CanWeeb API 到達性チェック ────────────────────");
    let url = format!("{}/api/status", api);
    match client
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            info!("  ✓ {} 到達可能 (HTTP {})", api, resp.status());
        }
        Ok(resp) => {
            warn!("  ⚠ {} 到達したが HTTP {} を返しました", api, resp.status());
        }
        Err(e) => {
            error!("  ✗ {} 到達不可: {:#}", api, e);
            error!("    CanWeeb が起動しているか、CANWEEB_API が正しいか確認してください。");
        }
    }
    info!("────────────────────────────────────────────────────");
}
