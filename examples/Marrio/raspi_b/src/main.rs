/// Marrio - Raspberry Pi B (GPIO 直接制御版)
///
/// ロータリーエンコーダを GPIO で直接読み取り、移動イベントを CanWeeb 経由で送信する。
/// STM32 などの外部マイコンは不要。
///
/// 環境変数:
///   CANWEEB_API      - CanWeeb Web API URL (default: http://localhost:8080)
///   GPIO_ENC_A       - エンコーダ A 相の BCM 番号 (default: 17)
///   GPIO_ENC_B       - エンコーダ B 相の BCM 番号 (default: 18)
///   DEBOUNCE_US      - デバウンス時間 µs (default: 500)
///   MIN_PULSE_US     - 最小パルス幅 µs (default: 200)
///   COUNT_THRESHOLD  - 移動判定カウント閾値 (default: 2)

use anyhow::{Context, Result};
use canweeb_cmdlib::GpioRotaryEncoder;
use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_GPIO_A: u32 = 17;
const DEFAULT_GPIO_B: u32 = 18;
const DEFAULT_DEBOUNCE_US: u64 = 500;
const DEFAULT_MIN_PULSE_US: u64 = 200;
const DEFAULT_COUNT_THRESHOLD: i64 = 2;
const POLL_INTERVAL_MS: u64 = 50;
const PC_NODE: &str = "marrio-pc";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let gpio_a = std::env::var("GPIO_ENC_A")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_A);
    let gpio_b = std::env::var("GPIO_ENC_B")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_B);
    let debounce_us = std::env::var("DEBOUNCE_US")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_DEBOUNCE_US);
    let min_pulse_us = std::env::var("MIN_PULSE_US")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MIN_PULSE_US);
    let count_threshold = std::env::var("COUNT_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_COUNT_THRESHOLD);

    info!("====================================================");
    info!("  Marrio RasPi-B  GPIO ロータリーエンコーダ (STM32 不要)");
    info!("====================================================");
    info!("  CANWEEB_API      = {}", api);
    info!("  GPIO_ENC_A       = BCM {}", gpio_a);
    info!("  GPIO_ENC_B       = BCM {}", gpio_b);
    info!("  DEBOUNCE_US      = {} µs", debounce_us);
    info!("  MIN_PULSE_US     = {} µs", min_pulse_us);
    info!("  COUNT_THRESHOLD  = {} (移動判定)", count_threshold);
    info!("====================================================");

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;

    check_canweeb(&client, &api).await;

    let encoder = GpioRotaryEncoder::new(gpio_a, gpio_b)
        .debounce_us(debounce_us)
        .min_pulse_us(min_pulse_us);

    encoder.start().context("エンコーダ監視スレッド起動失敗")?;

    info!("━━━ GPIO ロータリーエンコーダ監視開始 ━━━");
    info!("");

    run_encoder_loop(&client, &api, &encoder, count_threshold).await
}

async fn run_encoder_loop(
    client: &Client,
    api: &str,
    encoder: &GpioRotaryEncoder,
    count_threshold: i64,
) -> Result<()> {
    let mut prev_count = 0i64;
    let mut total_events = 0u64;
    let mut left_count = 0u64;
    let mut right_count = 0u64;
    let mut last_stats_at = Instant::now();

    loop {
        let current = encoder.count();
        let delta = current - prev_count;

        if delta.abs() >= count_threshold {
            total_events += 1;

            let direction = if delta > 0 {
                right_count += 1;
                "right"
            } else {
                left_count += 1;
                "left"
            };

            info!(
                "[イベント #{:>6}] カウント:{:>6} → {:>6}  Δ={:>4}  方向:{}",
                total_events, prev_count, current, delta, direction
            );

            prev_count = current;

            let api_c = api.to_string();
            let cli_c = client.clone();
            let dir_c = direction.to_string();
            tokio::spawn(async move {
                match send_move(&cli_c, &api_c, &dir_c, delta).await {
                    Ok(()) => info!("  ✓ MOVE {} 送信完了", dir_c),
                    Err(e) => error!("  ✗ MOVE {} 送信失敗: {:#}", dir_c, e),
                }
            });
        }

        if last_stats_at.elapsed() >= Duration::from_secs(5) {
            info!(
                "  [統計] イベント:{} 左:{} 右:{} 現在カウント:{}",
                total_events, left_count, right_count, current
            );
            last_stats_at = Instant::now();
        }

        sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

async fn send_move(client: &Client, api: &str, direction: &str, delta: i64) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let payload = json!({
        "direction": direction,
        "delta":     delta,
        "source":    "raspi-b-gpio",
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

async fn check_canweeb(client: &Client, api: &str) {
    info!("─── CanWeeb API 到達性チェック ────────────────────");
    let url = format!("{}/api/status", api);
    match client.get(&url).timeout(Duration::from_secs(3)).send().await {
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
