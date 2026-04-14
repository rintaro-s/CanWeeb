/// Loli Encoder Sender - ロータリーエンコーダ送信スクリプト (子)
///
/// **DTOverlay を使用してロータリーエンコーダを読み取り、CanWeeb に送信する**
///
/// 初期化手順:
/// 1. DTOverlay で rotary-encoder をロード（カーネルドライバを使用）
/// 2. pinctrl でピンを入力・プルアップに設定
/// 3. GPIO を読み取ってエンコーダ値を監視
/// 4. CanWeeb に送信
///
/// 環境変数:
///   CANWEEB_API      - CanWeeb Web API URL (default: http://localhost:8080)
///   GPIO_CLK         - CLK (A相) ピンの BCM 番号 (default: 17)
///   GPIO_DT          - DT (B相) ピンの BCM 番号 (default: 18)
///   DEBOUNCE_US      - デバウンス時間 µs (default: 1000)
///   USE_DTOVERLAY    - DTOverlay を使用するか (default: true)

use anyhow::{Context, Result};
use canweeb_cmdlib::{DtOverlay, GpioRotaryEncoder3Pin, pinctrl_get, pinctrl_set};
use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_GPIO_CLK: u32 = 17;
const DEFAULT_GPIO_DT: u32 = 18;
const DEFAULT_DEBOUNCE_US: u64 = 1000;
const POLL_INTERVAL_MS: u64 = 10;
const SEND_INTERVAL_MS: u64 = 50;
const TARGET_NODE: &str = "loli-visualizer";

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
    let use_dtoverlay = std::env::var("USE_DTOVERLAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(true);

    info!("====================================================");
    info!("  Loli Encoder Sender - ロータリーエンコーダ送信");
    info!("====================================================");
    info!("  CANWEEB_API      = {}", api);
    info!("  GPIO_CLK         = BCM {}", gpio_clk);
    info!("  GPIO_DT          = BCM {}", gpio_dt);
    info!("  DEBOUNCE_US      = {} µs", debounce_us);
    info!("  USE_DTOVERLAY    = {}", use_dtoverlay);
    info!("  TARGET_NODE      = {}", TARGET_NODE);
    info!("====================================================");

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;

    check_canweeb(&client, &api).await;

    info!("");
    info!("─── DTOverlay 設定 ─────────────────────────────────");

    // DTOverlay を使用する（必須）
    if use_dtoverlay {
        // 既にロード済みか確認
        if DtOverlay::is_loaded("rotary-encoder") {
            info!("  ✓ rotary-encoder は既にロード済みです");
        } else {
            info!("  DTOverlay rotary-encoder をロード中...");
            
            // rotary-encoder オーバーレイをロード
            // パラメータ: pinA, pinB, relative_axis, steps-per-period
            DtOverlay::load(
                "rotary-encoder",
                &[
                    ("pin_a", &gpio_clk.to_string()),
                    ("pin_b", &gpio_dt.to_string()),
                    ("relative_axis", "1"),
                    ("steps-per-period", "1"),
                ],
            )
            .context("DTOverlay rotary-encoder のロードに失敗しました")?;

            info!("  ✓ DTOverlay rotary-encoder をロードしました");
        }

        // ロード済みオーバーレイを確認
        match DtOverlay::list() {
            Ok(list) => {
                info!("  現在ロード済みの DTOverlay:");
                for overlay in &list {
                    info!("    - {}", overlay);
                }
            }
            Err(e) => warn!("  ⚠ DTOverlay 一覧取得失敗: {:#}", e),
        }
    } else {
        warn!("  ⚠ USE_DTOVERLAY=false のため DTOverlay をスキップします");
        warn!("    （本来は DTOverlay を使用すべきです）");
    }

    info!("────────────────────────────────────────────────────");
    info!("");
    info!("─── GPIO ピン設定 (pinctrl) ───────────────────────");

    // pinctrl でピンを入力・プルアップに設定
    info!("  GPIO{} を入力・プルアップに設定中...", gpio_clk);
    pinctrl_set(gpio_clk, "ip", "pu").context(format!(
        "GPIO{} の pinctrl 設定に失敗しました",
        gpio_clk
    ))?;

    info!("  GPIO{} を入力・プルアップに設定中...", gpio_dt);
    pinctrl_set(gpio_dt, "ip", "pu").context(format!(
        "GPIO{} の pinctrl 設定に失敗しました",
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

    // エンコーダを初期化
    let encoder = GpioRotaryEncoder3Pin::new(gpio_clk, gpio_dt).debounce_us(debounce_us);

    encoder.start().context("エンコーダ監視スレッド起動失敗")?;

    info!("━━━ ロータリーエンコーダ監視開始 ━━━");
    info!("━━━ DTOverlay + pinctrl + GPIO 直接読み取り ━━━");
    info!("");

    run_encoder_loop(&client, &api, &encoder).await
}

async fn run_encoder_loop(
    client: &Client,
    api: &str,
    encoder: &GpioRotaryEncoder3Pin,
) -> Result<()> {
    let mut last_sent_count = 0i64;
    let mut last_send_at = Instant::now();
    let mut total_sent = 0u64;

    loop {
        let current = encoder.count();

        // 定期的に送信（変化がなくても送信して接続を維持）
        if last_send_at.elapsed().as_millis() as u64 >= SEND_INTERVAL_MS {
            let delta = current - last_sent_count;

            if delta != 0 {
                info!(
                    "[送信 #{:>6}] カウント:{:>6} → {:>6}  Δ={:>4}",
                    total_sent + 1,
                    last_sent_count,
                    current,
                    delta
                );
            }

            // CanWeeb に送信
            match send_encoder_position(client, api, current, delta).await {
                Ok(()) => {
                    total_sent += 1;
                }
                Err(e) => error!("  ✗ 送信失敗: {:#}", e),
            }

            last_sent_count = current;
            last_send_at = Instant::now();
        }

        sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

async fn send_encoder_position(
    client: &Client,
    api: &str,
    position: i64,
    delta: i64,
) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let payload = json!({
        "position": position,
        "delta":    delta,
        "source":   "loli-encoder-sender",
    });
    let body = json!({
        "target":        format!("node:{}", TARGET_NODE),
        "traffic_class": "control",
        "topic":         "loli/encoder/position",
        "subject":       "encoder_position",
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
