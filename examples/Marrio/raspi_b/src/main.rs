/// Marrio - Raspberry Pi B (3ピンロータリーエンコーダ版)
///
/// 3ピンロータリーエンコーダ (CLK, DT, GND) を GPIO で直接読み取り、
/// 移動イベントを CanWeeb 経由で送信する。
///
/// **正しい初期化手順:**
/// 1. pinctrl でピンを入力・プルアップに設定
/// 2. GPIO を読み取ってエンコーダ値を監視
///
/// 環境変数:
///   CANWEEB_API      - CanWeeb Web API URL (default: http://localhost:8080)
///   GPIO_CLK         - CLK (A相) ピンの BCM 番号 (default: 17)
///   GPIO_DT          - DT (B相) ピンの BCM 番号 (default: 18)
///   DEBOUNCE_US      - デバウンス時間 µs (default: 1000)
///   COUNT_THRESHOLD  - 移動判定カウント閾値 (default: 2)

use anyhow::{Context, Result};
use canweeb_cmdlib::{GpioRotaryEncoder3Pin, pinctrl_set, pinctrl_get};
use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_GPIO_CLK: u32 = 17;
const DEFAULT_GPIO_DT: u32 = 18;
const DEFAULT_DEBOUNCE_US: u64 = 1000;
const DEFAULT_COUNT_THRESHOLD: i64 = 2;
const POLL_INTERVAL_MS: u64 = 50;
const PC_NODE: &str = "marrio-pc";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let gpio_clk = std::env::var("GPIO_CLK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_CLK);
    let gpio_dt = std::env::var("GPIO_DT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_DT);
    let debounce_us = std::env::var("DEBOUNCE_US")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_DEBOUNCE_US);
    let count_threshold = std::env::var("COUNT_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_COUNT_THRESHOLD);

    info!("====================================================");
    info!("  Marrio RasPi-B  3ピンロータリーエンコーダ (GPIO 直接)");
    info!("====================================================");
    info!("  CANWEEB_API      = {}", api);
    info!("  GPIO_CLK         = BCM {}", gpio_clk);
    info!("  GPIO_DT          = BCM {}", gpio_dt);
    info!("  DEBOUNCE_US      = {} µs", debounce_us);
    info!("  COUNT_THRESHOLD  = {} (移動判定)", count_threshold);
    info!("====================================================");

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;

    check_canweeb(&client, &api).await;

    info!("");
    info!("─── GPIO ピン設定 (pinctrl) ───────────────────────");

    // 1. pinctrl でピンを入力・プルアップに設定
    info!("  GPIO{} を入力・プルアップに設定中...", gpio_clk);
    pinctrl_set(gpio_clk, "ip", "pu").context(format!(
        "GPIO{} の pinctrl 設定に失敗しました。\n\
         sudo 権限が必要な場合があります。",
        gpio_clk
    ))?;

    info!("  GPIO{} を入力・プルアップに設定中...", gpio_dt);
    pinctrl_set(gpio_dt, "ip", "pu").context(format!(
        "GPIO{} の pinctrl 設定に失敗しました。\n\
         sudo 権限が必要な場合があります。",
        gpio_dt
    ))?;

    // 設定確認
    match pinctrl_get(gpio_clk) {
        Ok(status) => info!("  ✓ GPIO{} 設定: {}", gpio_clk, status),
        Err(e) => warn!("  ⚠ GPIO{} 設定確認失敗: {:#}", gpio_clk, e),
    }
    match pinctrl_get(gpio_dt) {
        Ok(status) => info!("  ✓ GPIO{} 設定: {}", gpio_dt, status),
        Err(e) => warn!("  ⚠ GPIO{} 設定確認失敗: {:#}", gpio_dt, e),
    }

    info!("────────────────────────────────────────────────────");
    info!("");

    // 2. エンコーダを初期化
    let encoder = GpioRotaryEncoder3Pin::new(gpio_clk, gpio_dt).debounce_us(debounce_us);

    encoder.start().context("エンコーダ監視スレッド起動失敗")?;

    info!("━━━ 3ピンロータリーエンコーダ監視開始 ━━━");
    info!("");

    run_encoder_loop(&client, &api, &encoder, count_threshold).await
}

async fn run_encoder_loop(
    client: &Client,
    api: &str,
    encoder: &GpioRotaryEncoder3Pin,
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
