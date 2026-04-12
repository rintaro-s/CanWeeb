/// Marrio - Raspberry Pi B (完全版)
///
/// 起動時に全シリアルポートを列挙、CanWeeb API 到達性を確認、
/// 全受信行をログ出力、毎秒統計、データ無受信警告を行う。
///
/// 環境変数:
///   CANWEEB_API - CanWeeb Web API URL (default: http://localhost:8080)
///   SERIAL_PORT - シリアルポートパス (default: 自動検出)
///   BAUD_RATE   - ボーレート (default: 115200)

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str  = "http://localhost:8080";
const DEFAULT_BAUD: u32  = 115200;
const PC_NODE: &str      = "marrio-pc";
/// この秒数データが来なければ警告
const NO_DATA_WARN_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let api           = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let baud          = std::env::var("BAUD_RATE").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_BAUD);
    let explicit_port = std::env::var("SERIAL_PORT").ok();

    info!("====================================================");
    info!("  Marrio RasPi-B  ロータリーエンコーダ ブリッジ");
    info!("====================================================");
    info!("  CANWEEB_API = {}", api);
    info!("  BAUD_RATE   = {} bps", baud);
    info!("  SERIAL_PORT = {}", explicit_port.as_deref().unwrap_or("(自動検出)"));
    info!("====================================================");

    // ─── 全シリアルポートを列挙 ──────────────────────────────────
    list_serial_ports();

    // ─── CanWeeb API 到達性チェック ──────────────────────────────
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;
    check_canweeb(&client, &api).await;

    // ─── シリアルポート解決 ──────────────────────────────────────
    let port_name = resolve_serial_port(explicit_port.as_deref())?;
    info!("使用シリアルポート: {}", port_name);

    // ─── メインループ（切断時自動再接続） ───────────────────────
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        info!("");
        info!("──── [接続試行 #{attempt}] {} @ {} baud ────", port_name, baud);
        match run_encoder_loop(&client, &api, &port_name, baud).await {
            Ok(()) => info!("エンコーダループ終了"),
            Err(e) => error!("エンコーダループエラー: {:#}", e),
        }
        warn!("5秒後に再接続... (試行 #{attempt})");
        sleep(Duration::from_secs(5)).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  エンコーダ読み取りループ
// ─────────────────────────────────────────────────────────────────────────────
async fn run_encoder_loop(
    client: &Client,
    api: &str,
    port_name: &str,
    baud: u32,
) -> Result<()> {
    let port = serialport::new(port_name, baud)
        .timeout(Duration::from_millis(3000))
        .open()
        .with_context(|| format!(
            "シリアルポート {} を開けません (baud={})。\n\
             STM32 が接続されているか、ポート名が正しいか確認してください。",
            port_name, baud
        ))?;

    info!("━━━ {} @ {} baud オープン成功 ━━━", port_name, baud);
    info!("STM32 から 'R\\r\\n' または 'L\\r\\n' が来るのを待っています...");

    let mut reader        = BufReader::new(port);
    let mut line          = String::new();
    let mut total_lines:  u64 = 0;
    let mut move_count:   u64 = 0;
    let mut unknown_count:u64 = 0;
    let mut last_data_at  = Instant::now();
    let mut last_stats_at = Instant::now();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // ─ EOF（デバイス切断）
            Ok(0) => {
                warn!("シリアルポートが EOF を返しました（デバイス切断）");
                break;
            }
            // ─ データ受信
            Ok(n) => {
                // 生バイト列をhexで表示（最初の30行は必ず）
                let raw_bytes = &line.as_bytes()[..n.min(line.len())];
                let hex_str: String = raw_bytes.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                total_lines   += 1;
                last_data_at   = Instant::now();

                // 最初の30行は生データを常に出力
                if total_lines <= 30 {
                    info!("[起動確認 raw #{:>4}] text={:?}  hex=[{}]", total_lines, trimmed, hex_str);
                }

                match parse_encoder_line(trimmed) {
                    Some((ref dir, count)) => {
                        move_count += 1;
                        info!(
                            "[MOVE #{:>5}] 方向={:<6}  count={:>4}  (合計受信行:{} 未知:{})",
                            move_count, dir, count, total_lines, unknown_count
                        );

                        let api_c   = api.to_string();
                        let cli_c   = client.clone();
                        let dir_c   = dir.clone();
                        tokio::spawn(async move {
                            match send_move(&cli_c, &api_c, &dir_c, count).await {
                                Ok(())  => info!("  ✓ MOVE 送信完了: {}", dir_c),
                                Err(e)  => error!("  ✗ MOVE 送信失敗: {:#}", e),
                            }
                        });
                    }
                    None => {
                        unknown_count += 1;
                        warn!(
                            "[不明行 #{:>4}] {:?}  hex=[{}]  (L/R/S:<n> 以外)",
                            total_lines, trimmed, hex_str
                        );
                    }
                }

                // 毎秒統計
                if last_stats_at.elapsed() >= Duration::from_secs(1) {
                    info!(
                        "  [統計] 受信行:{} 方向イベント:{} 不明行:{}",
                        total_lines, move_count, unknown_count
                    );
                    last_stats_at = Instant::now();
                }
            }
            // ─ タイムアウト（データが来ていない）
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                let secs = last_data_at.elapsed().as_secs();
                if secs >= NO_DATA_WARN_SECS {
                    warn!(
                        "⚠⚠⚠ {} 秒間データなし！STM32 が動いているか確認してください ⚠⚠⚠",
                        secs
                    );
                }
                continue;
            }
            // ─ その他エラー
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "シリアル読み取りエラー: {} (kind={:?})",
                    e, e.kind()
                ));
            }
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  エンコーダ行パーサー
// ─────────────────────────────────────────────────────────────────────────────

/// STM32 から送られてくる行を解析する
/// - "R" または "R\r" → right (+1)
/// - "L" または "L\r" → left  (-1)
/// - "S:<n>"          → n>0 → right, n<0 → left
fn parse_encoder_line(line: &str) -> Option<(String, i32)> {
    // trim() 済みを受け取るが念のため再 trim
    let s = line.trim();
    match s {
        "L" => return Some(("left".to_string(), -1)),
        "R" => return Some(("right".to_string(), 1)),
        _   => {}
    }
    if let Some(rest) = s.strip_prefix("S:") {
        if let Ok(count) = rest.trim().parse::<i32>() {
            let dir = if count >= 0 { "right" } else { "left" };
            return Some((dir.to_string(), count));
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  CanWeeb move 送信
// ─────────────────────────────────────────────────────────────────────────────
async fn send_move(client: &Client, api: &str, direction: &str, count: i32) -> Result<()> {
    let url     = format!("{}/api/messages", api);
    let payload = json!({
        "event":     "move",
        "direction": direction,
        "count":     count,
        "source":    "raspi-b",
    });
    let body = json!({
        "target":        format!("node:{}", PC_NODE),
        "traffic_class": "control",
        "topic":         "marrio/input/move",
        "subject":       "move",
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

// ─────────────────────────────────────────────────────────────────────────────
//  起動時診断
// ─────────────────────────────────────────────────────────────────────────────

/// 全シリアルポートの一覧と USB 機器情報を出力
fn list_serial_ports() {
    info!("─── 利用可能なシリアルポート ───────────────────────");
    match serialport::available_ports() {
        Err(e) => error!("列挙失敗: {}", e),
        Ok(ports) if ports.is_empty() => {
            warn!("シリアルポートが見つかりません。");
            warn!("STM32 が USB 接続されているか確認してください。");
        }
        Ok(ports) => {
            for (i, p) in ports.iter().enumerate() {
                let detail = match &p.port_type {
                    serialport::SerialPortType::UsbPort(u) => format!(
                        "USB  vid={:04x} pid={:04x}  mfr={:<20} prod={}",
                        u.vid, u.pid,
                        u.manufacturer.as_deref().unwrap_or("?"),
                        u.product.as_deref().unwrap_or("?"),
                    ),
                    serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                    serialport::SerialPortType::PciPort       => "PCI".to_string(),
                    serialport::SerialPortType::Unknown        => "Unknown".to_string(),
                };
                info!("  [{i}] {}   ({detail})", p.port_name);
            }
        }
    }
    info!("────────────────────────────────────────────────────");
}

/// CanWeeb API /api/status に GET して到達性を確認
async fn check_canweeb(client: &Client, api: &str) {
    let url = format!("{}/api/status", api);
    info!("─── CanWeeb API 到達性チェック → {} ───", url);
    match client.get(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(r)  => info!("  ✓ 到達可能 (HTTP {})", r.status()),
        Err(e) => {
            error!("  ✗ 到達不可: {}", e);
            error!("    → CanWeeb が {} で起動しているか確認してください", api);
            error!("    → CANWEEB_API 環境変数を PC の IP アドレスに合わせてください");
        }
    }
    info!("────────────────────────────────────────────────────");
}

/// シリアルポートを解決する（環境変数優先、次に自動検出）
fn resolve_serial_port(explicit: Option<&str>) -> Result<String> {
    if let Some(p) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        info!("SERIAL_PORT 環境変数で指定: {}", p);
        return Ok(p.to_string());
    }

    let ports = serialport::available_ports()
        .context("シリアルポート列挙失敗")?;

    if ports.is_empty() {
        return Err(anyhow::anyhow!(
            "シリアルポートが見つかりません。\n\
             STM32 を USB で接続し、必要に応じて\n\
             SERIAL_PORT=/dev/ttyACM0 を指定してください"
        ));
    }

    // ttyACM → ttyUSB → 先頭 の優先順
    if let Some(p) = ports.iter().find(|p| p.port_name.starts_with("/dev/ttyACM")) {
        info!("自動検出 (ttyACM 優先): {}", p.port_name);
        return Ok(p.port_name.clone());
    }
    if let Some(p) = ports.iter().find(|p| p.port_name.starts_with("/dev/ttyUSB")) {
        info!("自動検出 (ttyUSB): {}", p.port_name);
        return Ok(p.port_name.clone());
    }

    let first = ports[0].port_name.clone();
    warn!("ttyACM/ttyUSB が見つからないため先頭ポートを使用: {}", first);
    Ok(first)
}
