/// Marrio - Raspberry Pi A (GPIO 直接制御版)
///
/// HC-SR04 超音波センサを GPIO で直接読み取り、ジャンプイベントを CanWeeb 経由で送信する。
/// Arduino などの外部マイコンは不要。
///
/// 環境変数:
///   CANWEEB_API       - CanWeeb Web API URL (default: http://localhost:8080)
///   GPIO_TRIG         - TRIG ピンの BCM 番号 (default: 23)
///   GPIO_ECHO         - ECHO ピンの BCM 番号 (default: 24)
///   JUMP_THRESHOLD_CM - ジャンプ判定距離 cm (default: 30)
///   SENSOR_SAMPLES    - 1回の測定サンプル数（中央値フィルタ用、default: 3）
///   MAX_DELTA_CM      - 外れ値検出しきい値 cm (default: 50)

use anyhow::{Context, Result};
use canweeb_cmdlib::GpioUltrasonicSensor;
use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

const DEFAULT_API: &str = "http://localhost:8080";
const DEFAULT_GPIO_TRIG: u32 = 23;
const DEFAULT_GPIO_ECHO: u32 = 24;
const DEFAULT_JUMP_THRESHOLD_CM: f64 = 30.0;
const DEFAULT_SAMPLES: usize = 3;
const DEFAULT_MAX_DELTA_CM: f64 = 50.0;
const JUMP_COOLDOWN_MS: u64 = 800;
const MEASURE_INTERVAL_MS: u64 = 100;
const PC_NODE: &str = "marrio-pc";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let api = std::env::var("CANWEEB_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let gpio_trig = std::env::var("GPIO_TRIG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_TRIG);
    let gpio_echo = std::env::var("GPIO_ECHO")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GPIO_ECHO);
    let jump_threshold = std::env::var("JUMP_THRESHOLD_CM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_JUMP_THRESHOLD_CM);
    let samples = std::env::var("SENSOR_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SAMPLES);
    let max_delta = std::env::var("MAX_DELTA_CM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_DELTA_CM);

    info!("====================================================");
    info!("  Marrio RasPi-A  GPIO 超音波センサ (Arduino 不要)");
    info!("====================================================");
    info!("  CANWEEB_API       = {}", api);
    info!("  GPIO_TRIG         = BCM {}", gpio_trig);
    info!("  GPIO_ECHO         = BCM {}", gpio_echo);
    info!("  JUMP_THRESHOLD_CM = {} cm", jump_threshold);
    info!("  JUMP_COOLDOWN_MS  = {} ms", JUMP_COOLDOWN_MS);
    info!("  SENSOR_SAMPLES    = {} (中央値フィルタ)", samples);
    info!("  MAX_DELTA_CM      = {} cm (外れ値検出)", max_delta);
    info!("====================================================");
    info!("");
    info!("⚠ 注意: HC-SR04 の ECHO ピンは 5V 出力です。");
    info!("  Raspberry Pi の GPIO は 3.3V 入力のため、必ず抵抗分圧");
    info!("  (1kΩ + 2kΩ) またはレベルシフタを使用してください。");
    info!("");

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("reqwest Client 構築失敗")?;

    check_canweeb(&client, &api).await;

    let sensor = GpioUltrasonicSensor::new(gpio_trig, gpio_echo)
        .samples(samples)
        .max_delta_cm(max_delta);

    info!("━━━ GPIO 超音波センサ初期化完了 ━━━");
    info!("");

    run_sensor_loop(&client, &api, &sensor, jump_threshold).await
}

async fn run_sensor_loop(
    client: &Client,
    api: &str,
    sensor: &GpioUltrasonicSensor,
    jump_threshold: f64,
) -> Result<()> {
    let mut last_jump_at: Option<Instant> = None;
    let mut total_measures = 0u64;
    let mut valid_count = 0u64;
    let mut timeout_count = 0u64;
    let mut outlier_count = 0u64;
    let mut last_stats_at = Instant::now();

    loop {
        total_measures += 1;

        match sensor.measure() {
            Ok(Some(dist)) => {
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
            Ok(None) => {
                outlier_count += 1;
                if total_measures <= 20 {
                    warn!("[測定 #{:>6}] タイムアウトまたは外れ値", total_measures);
                }
            }
            Err(e) => {
                timeout_count += 1;
                error!("[測定 #{:>6}] センサーエラー: {:#}", total_measures, e);
            }
        }

        if last_stats_at.elapsed() >= Duration::from_secs(5) {
            info!(
                "  [統計] 測定:{} 有効:{} タイムアウト:{} 外れ値:{}",
                total_measures, valid_count, timeout_count, outlier_count
            );
            last_stats_at = Instant::now();
        }

        sleep(Duration::from_millis(MEASURE_INTERVAL_MS)).await;
    }
}

async fn send_jump(client: &Client, api: &str, distance_cm: f64) -> Result<()> {
    let url = format!("{}/api/messages", api);
    let payload = json!({
        "event":       "jump",
        "distance_cm": distance_cm,
        "source":      "raspi-a-gpio",
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
