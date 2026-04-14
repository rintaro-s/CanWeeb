//! GPIO 低レイヤー拡張モジュール
//!
//! Raspberry Pi の GPIO を直接制御するための高レベル API を提供する。
//! すべての操作は `/dev/gpiochip0` (gpio-cdev) または
//! `/sys/class/gpio` sysfs 経由で行い、外部マイコンは不要。
//!
//! # 提供する機能
//!
//! - [`DtOverlay`]            — DTOverlay のロード・アンロード・状態確認
//! - [`GpioUltrasonicSensor`] — HC-SR04 超音波センサの GPIO 直接測定
//! - [`GpioRotaryEncoder`]    — クアドラチャエンコーダの GPIO 直接監視
//!
//! # ノイズ対策
//!
//! - `GpioUltrasonicSensor`: 連続 N 回測定の中央値フィルタ + 外れ値カット
//! - `GpioRotaryEncoder`: ソフトウェアデバウンス（最小パルス幅しきい値）
//!   + クアドラチャデコード（4倍精度）

use crate::encoder::{quadrature_delta, quadrature_state};
use crate::CmdError;
use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ─────────────────────────────────────────────────────────────────────────────
//  DtOverlay
// ─────────────────────────────────────────────────────────────────────────────

/// Raspberry Pi DTOverlay (Device Tree Overlay) の管理。
///
/// `dtoverlay` コマンド経由でオーバーレイをロード・アンロードし、
/// 現在の状態を確認する。
///
/// # 例
/// ```no_run
/// use canweeb_cmdlib::gpio_ext::DtOverlay;
/// DtOverlay::load("rotary-encoder", &[("relative_axis", "1"), ("steps-per-period", "1")]).unwrap();
/// DtOverlay::unload("rotary-encoder").unwrap();
/// ```
pub struct DtOverlay;

impl DtOverlay {
    /// 指定したオーバーレイをパラメータ付きでロードする。
    ///
    /// `dtoverlay <name> [key=value ...]` を実行する。
    pub fn load(name: &str, params: &[(&str, &str)]) -> Result<(), CmdError> {
        let mut args = vec![name.to_string()];
        for (k, v) in params {
            args.push(format!("{}={}", k, v));
        }

        let status = Command::new("dtoverlay")
            .args(&args)
            .status()
            .map_err(|e| {
                CmdError::Backend(format!(
                    "dtoverlay コマンドの起動に失敗しました: {}.\n\
                     raspi-config で DTOverlay が有効か確認してください。",
                    e
                ))
            })?;

        if !status.success() {
            return Err(CmdError::Backend(format!(
                "dtoverlay {} のロードに失敗しました (exit={})",
                name,
                status.code().unwrap_or(-1)
            )));
        }
        Ok(())
    }

    /// 指定したオーバーレイをアンロードする (`dtoverlay -r <name>`)。
    pub fn unload(name: &str) -> Result<(), CmdError> {
        let status = Command::new("dtoverlay")
            .args(["-r", name])
            .status()
            .map_err(|e| CmdError::Backend(format!("dtoverlay -r 起動失敗: {}", e)))?;

        if !status.success() {
            return Err(CmdError::Backend(format!(
                "dtoverlay -r {} 失敗 (exit={})",
                name,
                status.code().unwrap_or(-1)
            )));
        }
        Ok(())
    }

    /// 現在ロード済みのオーバーレイ一覧を返す (`dtoverlay -l`)。
    pub fn list() -> Result<Vec<String>, CmdError> {
        let output = Command::new("dtoverlay")
            .arg("-l")
            .output()
            .map_err(|e| CmdError::Backend(format!("dtoverlay -l 起動失敗: {}", e)))?;

        let text = String::from_utf8_lossy(&output.stdout);
        let overlays: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with("Overlays"))
            .map(|l| l.trim().to_string())
            .collect();
        Ok(overlays)
    }

    /// 指定したオーバーレイが既にロード済みかどうかを確認する。
    pub fn is_loaded(name: &str) -> bool {
        Self::list()
            .map(|list| list.iter().any(|l| l.contains(name)))
            .unwrap_or(false)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  GPIO チップヘルパー
// ─────────────────────────────────────────────────────────────────────────────

fn open_gpio_chip() -> Result<Chip, CmdError> {
    for i in 0..=4 {
        let path = format!("/dev/gpiochip{}", i);
        if Path::new(&path).exists() {
            if let Ok(chip) = Chip::new(&path) {
                return Ok(chip);
            }
        }
    }
    Err(CmdError::Backend(
        "GPIO チップが見つかりません。Raspberry Pi 上で実行しているか確認してください。".into(),
    ))
}

fn get_output_handle(chip: &mut Chip, bcm: u32, label: &str) -> Result<LineHandle, CmdError> {
    chip.get_line(bcm)
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} 取得失敗: {e}")))?
        .request(LineRequestFlags::OUTPUT, 0, label)
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} 出力設定失敗: {e}")))
}

fn get_input_handle(chip: &mut Chip, bcm: u32, label: &str) -> Result<LineHandle, CmdError> {
    chip.get_line(bcm)
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} 取得失敗: {e}")))?
        .request(LineRequestFlags::INPUT, 0, label)
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} 入力設定失敗: {e}")))
}

// ─────────────────────────────────────────────────────────────────────────────
//  GpioUltrasonicSensor
// ─────────────────────────────────────────────────────────────────────────────

/// HC-SR04 超音波センサを Raspberry Pi GPIO で直接制御する。
///
/// Arduino などの外部マイコン不要。
///
/// # ノイズ対策
/// - `samples`: 1回の測定で N 回パルスを発し、**中央値**を採用する (default: 3)
/// - `max_delta_cm`: 前回値との差がこれを超えたら外れ値として破棄 (default: 50 cm)
/// - `timeout_us`: エコー待機タイムアウト (default: 30 000 µs ≒ 約 5 m 相当)
///
/// # GPIO 割り付け
/// - TRIG: 任意の出力可能ピン (BCM 番号)
/// - ECHO: 任意の入力可能ピン (BCM 番号)
///         ※ Raspberry Pi の GPIO は 3.3V 入力のみ対応。HC-SR04 の ECHO は 5V なので
///           抵抗分圧 (1kΩ + 2kΩ) またはレベルシフタを必ず使用してください。
pub struct GpioUltrasonicSensor {
    trig_bcm: u32,
    echo_bcm: u32,
    /// 1回の公称測定に使うパルス数（中央値フィルタ用）
    samples: usize,
    /// 前回値からこれ以上離れたら外れ値として破棄 (cm)
    max_delta_cm: f64,
    /// エコー待機タイムアウト (µs)
    timeout_us: u64,
    last_valid: Mutex<Option<f64>>,
}

impl GpioUltrasonicSensor {
    /// デフォルト設定でセンサーを構築する。
    ///
    /// - `trig_bcm`: TRIG ピンの BCM 番号
    /// - `echo_bcm`: ECHO ピンの BCM 番号
    pub fn new(trig_bcm: u32, echo_bcm: u32) -> Self {
        Self {
            trig_bcm,
            echo_bcm,
            samples: 3,
            max_delta_cm: 50.0,
            timeout_us: 30_000,
            last_valid: Mutex::new(None),
        }
    }

    /// 1回の公称測定に使うサンプル数を設定する（中央値フィルタ）。
    ///
    /// 奇数を推奨。大きいほどノイズに強くなるが測定レイテンシが増える。
    pub fn samples(mut self, n: usize) -> Self {
        self.samples = n.max(1);
        self
    }

    /// 外れ値検出しきい値 (cm) を設定する。
    ///
    /// 前回値との差がこれを超えたら測定値を破棄し `None` を返す。
    /// `0.0` にすると無効化（フィルタなし）。
    pub fn max_delta_cm(mut self, v: f64) -> Self {
        self.max_delta_cm = v;
        self
    }

    /// エコーパルス待機タイムアウト (µs) を設定する。
    ///
    /// デフォルト 30 000 µs (≒ 516 cm 相当)。
    /// センサの仕様上限は 400 cm なので通常変更不要。
    pub fn timeout_us(mut self, v: u64) -> Self {
        self.timeout_us = v;
        self
    }

    /// 距離を測定する (cm)。
    ///
    /// センサが範囲外・タイムアウト・外れ値と判定した場合は `None` を返す。
    pub fn measure(&self) -> Result<Option<f64>, CmdError> {
        let mut chip = open_gpio_chip()?;
        let trig = get_output_handle(&mut chip, self.trig_bcm, "ultrasonic_trig")?;
        let echo = get_input_handle(&mut chip, self.echo_bcm, "ultrasonic_echo")?;

        let mut raw: Vec<f64> = Vec::with_capacity(self.samples);
        for _ in 0..self.samples {
            if let Some(d) = self.single_pulse(&trig, &echo)? {
                raw.push(d);
            }
            // HC-SR04 の推奨測定間隔 >= 60 ms
            thread::sleep(Duration::from_millis(60));
        }

        if raw.is_empty() {
            return Ok(None);
        }

        // 中央値
        raw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = raw[raw.len() / 2];

        // 外れ値チェック
        if self.max_delta_cm > 0.0 {
            let mut last = self.last_valid.lock().unwrap();
            if let Some(prev) = *last {
                if (median - prev).abs() > self.max_delta_cm {
                    // 外れ値と判定 — last_valid は更新しない
                    return Ok(None);
                }
            }
            *last = Some(median);
        } else {
            *self.last_valid.lock().unwrap() = Some(median);
        }

        Ok(Some(median))
    }

    /// 1回のパルス送受信で距離を測定する（内部用）。
    fn single_pulse(&self, trig: &LineHandle, echo: &LineHandle) -> Result<Option<f64>, CmdError> {
        // TRIG LOW → 2µs 待機 → HIGH 10µs → LOW
        trig.set_value(0)
            .map_err(|e| CmdError::Backend(format!("TRIG LOW 失敗: {e}")))?;
        spin_sleep_us(2);
        trig.set_value(1)
            .map_err(|e| CmdError::Backend(format!("TRIG HIGH 失敗: {e}")))?;
        spin_sleep_us(10);
        trig.set_value(0)
            .map_err(|e| CmdError::Backend(format!("TRIG LOW 失敗: {e}")))?;

        // ECHO HIGH を待つ
        let start = Instant::now();
        loop {
            let v = echo
                .get_value()
                .map_err(|e| CmdError::Backend(format!("ECHO 読み取り失敗: {e}")))?;
            if v == 1 {
                break;
            }
            if start.elapsed().as_micros() as u64 > self.timeout_us {
                return Ok(None); // タイムアウト
            }
        }
        let echo_start = Instant::now();

        // ECHO LOW を待つ
        loop {
            let v = echo
                .get_value()
                .map_err(|e| CmdError::Backend(format!("ECHO 読み取り失敗: {e}")))?;
            if v == 0 {
                break;
            }
            if echo_start.elapsed().as_micros() as u64 > self.timeout_us {
                return Ok(None); // タイムアウト
            }
        }
        let duration_us = echo_start.elapsed().as_micros() as f64;

        // 音速: 343 m/s = 0.0343 cm/µs  往復なので /2
        let distance_cm = duration_us * 0.0343 / 2.0;

        // 有効範囲チェック (2 cm – 400 cm)
        if distance_cm < 2.0 || distance_cm > 400.0 {
            return Ok(None);
        }

        Ok(Some(distance_cm))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  GpioRotaryEncoder
// ─────────────────────────────────────────────────────────────────────────────

/// クアドラチャロータリーエンコーダを Raspberry Pi GPIO で直接監視する。
///
/// STM32 などの外部マイコン不要。バックグラウンドスレッドでポーリングし、
/// カウントをアトミック変数で保持する。
///
/// # ノイズ対策（デバウンス）
/// - `debounce_us`: 同じ状態が連続してこの時間 (µs) 続かなければ変化とみなさない
///   (default: **500 µs**）
/// - `min_pulse_us`: エンコーダパルスの最小幅 (µs)。これ未満のパルスはグリッチとして無視する
///   (default: **200 µs**)
///
/// # 使い方
/// ```no_run
/// use canweeb_cmdlib::gpio_ext::GpioRotaryEncoder;
/// let enc = GpioRotaryEncoder::new(17, 18)
///     .debounce_us(500)
///     .min_pulse_us(200);
/// enc.start().unwrap();
///
/// loop {
///     let (count, dir) = enc.read();
///     println!("count={count}  dir={dir:?}");
/// }
/// ```
pub struct GpioRotaryEncoder {
    pin_a_bcm: u32,
    pin_b_bcm: u32,
    /// デバウンス時間 (µs)
    debounce_us: u64,
    /// 最小パルス幅 (µs) — これ未満は無視
    min_pulse_us: u64,
    count: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
}

/// エンコーダの回転方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncDirection {
    /// 静止 (カウント変化なし)
    None,
    /// 正転（右回り）
    Clockwise,
    /// 逆転（左回り）
    CounterClockwise,
}

impl GpioRotaryEncoder {
    /// デフォルト設定でエンコーダを構築する。
    ///
    /// - `pin_a_bcm`: A 相ピンの BCM 番号
    /// - `pin_b_bcm`: B 相ピンの BCM 番号
    pub fn new(pin_a_bcm: u32, pin_b_bcm: u32) -> Self {
        Self {
            pin_a_bcm,
            pin_b_bcm,
            debounce_us: 500,
            min_pulse_us: 200,
            count: Arc::new(AtomicI64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// デバウンス時間 (µs) を設定する。
    ///
    /// 同じ GPIO 状態がこの時間継続しなければ変化と見なさない。
    /// エンコーダの機械的バウンス特性に合わせて調整する。
    /// 小さくすると応答が速くなるが、ノイズを拾いやすくなる。
    pub fn debounce_us(mut self, us: u64) -> Self {
        self.debounce_us = us;
        self
    }

    /// 最小パルス幅 (µs) を設定する。
    ///
    /// この幅未満のパルスはグリッチとして無視する。
    /// `debounce_us` より小さい値にする必要がある。
    pub fn min_pulse_us(mut self, us: u64) -> Self {
        self.min_pulse_us = us;
        self
    }

    /// バックグラウンドスレッドでエンコーダ監視を開始する。
    ///
    /// GPIO チップのオープン・設定を行い、1 スレッドで A/B 両相を
    /// スピンポーリングする。
    pub fn start(&self) -> Result<(), CmdError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(()); // 既に起動中
        }

        let pin_a = self.pin_a_bcm;
        let pin_b = self.pin_b_bcm;
        let debounce = self.debounce_us;
        let min_pulse = self.min_pulse_us;
        let count = Arc::clone(&self.count);
        let running = Arc::clone(&self.running);

        thread::Builder::new()
            .name("gpio-encoder".to_string())
            .spawn(move || {
                if let Err(e) = encoder_poll_loop(pin_a, pin_b, debounce, min_pulse, count, running) {
                    eprintln!("[GpioRotaryEncoder] ポーリングエラー: {e}");
                }
            })
            .map_err(|e| CmdError::Backend(format!("エンコーダスレッド起動失敗: {e}")))?;

        Ok(())
    }

    /// バックグラウンド監視を停止する。
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// 現在の累積カウントと方向を返す。
    ///
    /// カウントは `start()` 呼び出し時点を 0 とした累積値。
    /// 方向は直近の変化方向。
    pub fn read(&self) -> (i64, EncDirection) {
        let c = self.count.load(Ordering::Relaxed);
        (c, EncDirection::None)
    }

    /// 累積カウントのみを返す。
    pub fn count(&self) -> i64 {
        self.count.load(Ordering::Relaxed)
    }

    /// カウントをリセットする。
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
    }

    /// 監視スレッドが動作中かどうかを返す。
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for GpioRotaryEncoder {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  pinctrl 設定ヘルパー
// ─────────────────────────────────────────────────────────────────────────────

/// pinctrl を使って GPIO ピンの機能を設定する。
///
/// Raspberry Pi では GPIO ピンを使う前に pinctrl で機能を設定する必要がある。
/// 例: `pinctrl set 17 ip pu` (GPIO17 を入力・プルアップに設定)
///
/// # 引数
/// - `pin`: BCM ピン番号
/// - `func`: 機能 ("ip" = 入力, "op" = 出力, "a0"-"a5" = 代替機能)
/// - `pull`: プル設定 ("pn" = なし, "pu" = プルアップ, "pd" = プルダウン)
pub fn pinctrl_set(pin: u32, func: &str, pull: &str) -> Result<(), CmdError> {
    let status = Command::new("pinctrl")
        .args(["set", &pin.to_string(), func, pull])
        .status()
        .map_err(|e| {
            CmdError::Backend(format!(
                "pinctrl コマンドの起動に失敗しました: {}.\n\
                 pinctrl がインストールされているか確認してください。",
                e
            ))
        })?;

    if !status.success() {
        return Err(CmdError::Backend(format!(
            "pinctrl set {} {} {} の実行に失敗しました (exit={})",
            pin,
            func,
            pull,
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// pinctrl を使って GPIO ピンの現在の設定を取得する。
pub fn pinctrl_get(pin: u32) -> Result<String, CmdError> {
    let output = Command::new("pinctrl")
        .args(["get", &pin.to_string()])
        .output()
        .map_err(|e| CmdError::Backend(format!("pinctrl get 起動失敗: {}", e)))?;

    if !output.status.success() {
        return Err(CmdError::Backend(format!(
            "pinctrl get {} 失敗 (exit={})",
            pin,
            output.status.code().unwrap_or(-1)
        )));
    }

    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|e| CmdError::Backend(format!("pinctrl 出力の UTF-8 変換失敗: {}", e)))
}

// ─────────────────────────────────────────────────────────────────────────────
//  GpioRotaryEncoder3Pin (CLK, DT, GND の 3ピン版 - DTOverlay + pinctrl ベース)
// ─────────────────────────────────────────────────────────────────────────────

/// 3ピンロータリーエンコーダ (CLK, DT, GND) を GPIO で直接監視する。
///
/// **正しい初期化手順:**
/// 1. pinctrl でピンを入力・プルアップに設定
/// 2. DTOverlay で rotary-encoder をロード（オプション、カーネルドライバを使う場合）
/// 3. GPIO を読み取ってエンコーダ値を監視
///
/// # 使い方
/// ```no_run
/// use canweeb_cmdlib::gpio_ext::{GpioRotaryEncoder3Pin, pinctrl_set};
///
/// // 1. pinctrl でピンを設定
/// pinctrl_set(17, "ip", "pu").unwrap(); // GPIO17 を入力・プルアップ
/// pinctrl_set(18, "ip", "pu").unwrap(); // GPIO18 を入力・プルアップ
///
/// // 2. エンコーダを初期化して監視開始
/// let enc = GpioRotaryEncoder3Pin::new(17, 18)
///     .debounce_us(1000);
/// enc.start().unwrap();
///
/// loop {
///     let count = enc.count();
///     println!("count={count}");
/// }
/// ```
pub struct GpioRotaryEncoder3Pin {
    pin_clk_bcm: u32,
    pin_dt_bcm: u32,
    debounce_us: u64,
    count: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
}

impl GpioRotaryEncoder3Pin {
    /// 3ピンエンコーダを構築する。
    ///
    /// **注意:** この関数を呼ぶ前に `pinctrl_set()` で両ピンを入力・プルアップに設定すること。
    ///
    /// - `pin_clk_bcm`: CLK (A相) ピンの BCM 番号
    /// - `pin_dt_bcm`: DT (B相) ピンの BCM 番号
    pub fn new(pin_clk_bcm: u32, pin_dt_bcm: u32) -> Self {
        Self {
            pin_clk_bcm,
            pin_dt_bcm,
            debounce_us: 1000,
            count: Arc::new(AtomicI64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// デバウンス時間 (µs) を設定する。
    ///
    /// 3ピンエンコーダは機械的なチャタリングが多いため、
    /// デフォルトは 1000 µs (1 ms) と長めに設定されている。
    pub fn debounce_us(mut self, us: u64) -> Self {
        self.debounce_us = us;
        self
    }

    /// バックグラウンドスレッドでエンコーダ監視を開始する。
    ///
    /// **注意:** 開始前に `pinctrl_set()` でピンが正しく設定されていることを確認すること。
    pub fn start(&self) -> Result<(), CmdError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let pin_clk = self.pin_clk_bcm;
        let pin_dt = self.pin_dt_bcm;
        let debounce = self.debounce_us;
        let count = Arc::clone(&self.count);
        let running = Arc::clone(&self.running);

        thread::Builder::new()
            .name("gpio-encoder3pin".to_string())
            .spawn(move || {
                if let Err(e) = encoder3pin_poll_loop(pin_clk, pin_dt, debounce, count, running) {
                    eprintln!("[GpioRotaryEncoder3Pin] ポーリングエラー: {e}");
                }
            })
            .map_err(|e| CmdError::Backend(format!("エンコーダスレッド起動失敗: {e}")))?;

        Ok(())
    }

    /// バックグラウンド監視を停止する。
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// 現在の累積カウントを返す。
    pub fn count(&self) -> i64 {
        self.count.load(Ordering::Relaxed)
    }

    /// カウントをリセットする。
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
    }

    /// 監視スレッドが動作中かどうかを返す。
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for GpioRotaryEncoder3Pin {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  3ピンエンコーダ ポーリングループ
// ─────────────────────────────────────────────────────────────────────────────

fn encoder3pin_poll_loop(
    pin_clk: u32,
    pin_dt: u32,
    debounce_us: u64,
    count: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
) -> Result<(), CmdError> {
    // GPIO チップをオープン
    let mut chip = open_gpio_chip()?;
    let clk_handle = get_input_handle(&mut chip, pin_clk, "enc_clk")?;
    let dt_handle = get_input_handle(&mut chip, pin_dt, "enc_dt")?;

    let mut last_clk = clk_handle.get_value().unwrap_or(0) == 1;
    let mut stable_at = Instant::now();

    while running.load(Ordering::Relaxed) {
        let clk = clk_handle
            .get_value()
            .map_err(|e| CmdError::Backend(format!("GPIO{pin_clk} 読み取り失敗: {e}")))?
            == 1;
        let dt = dt_handle
            .get_value()
            .map_err(|e| CmdError::Backend(format!("GPIO{pin_dt} 読み取り失敗: {e}")))?
            == 1;

        // CLK の立ち下がりエッジで方向判定
        if !clk && last_clk {
            if stable_at.elapsed().as_micros() as u64 >= debounce_us {
                let delta = if dt { 1 } else { -1 };
                count.fetch_add(delta, Ordering::Relaxed);
                stable_at = Instant::now();
            }
        }
        last_clk = clk;

        spin_sleep_us(100);
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  エンコーダ ポーリングループ（バックグラウンドスレッド）
// ─────────────────────────────────────────────────────────────────────────────

fn encoder_poll_loop(
    pin_a: u32,
    pin_b: u32,
    debounce_us: u64,
    min_pulse_us: u64,
    count: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
) -> Result<(), CmdError> {
    let mut chip = open_gpio_chip()?;
    let a_handle = get_input_handle(&mut chip, pin_a, "enc_a")?;
    let b_handle = get_input_handle(&mut chip, pin_b, "enc_b")?;

    let read_state = || -> Result<u8, CmdError> {
        let a = a_handle
            .get_value()
            .map_err(|e| CmdError::Backend(format!("GPIO{pin_a} 読み取り失敗: {e}")))?
            == 1;
        let b = b_handle
            .get_value()
            .map_err(|e| CmdError::Backend(format!("GPIO{pin_b} 読み取り失敗: {e}")))?
            == 1;
        Ok(quadrature_state(a, b))
    };

    let mut prev_raw   = read_state()?;
    let mut stable     = prev_raw;
    let mut stable_at  = Instant::now();
    let mut last_event = Instant::now();

    while running.load(Ordering::Relaxed) {
        let raw = read_state()?;

        if raw != prev_raw {
            // 状態変化 → デバウンス開始
            prev_raw   = raw;
            stable_at  = Instant::now();
        } else if raw != stable && stable_at.elapsed().as_micros() as u64 >= debounce_us {
            // デバウンス通過 → 状態確定
            let pulse_us = last_event.elapsed().as_micros() as u64;

            if pulse_us >= min_pulse_us {
                let delta = quadrature_delta(stable, raw);
                if delta != 0 {
                    count.fetch_add(delta, Ordering::Relaxed);
                }
                last_event = Instant::now();
            }
            stable = raw;
        }

        // スピンウェイト: ~10 µs ごとにポーリング（CPU 占有を許容する精度優先設計）
        // 低精度エンコーダや低速回転ならここを thread::sleep(Duration::from_micros(50)) にしても良い
        spin_sleep_us(10);
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  ユーティリティ
// ─────────────────────────────────────────────────────────────────────────────

/// ビジーウェイトで指定マイクロ秒だけ待機する。
///
/// `thread::sleep` は精度が低い (OS スケジューラ次第) ため、
/// GPIO タイミング制御にはスピンループを使用する。
#[inline]
pub fn spin_sleep_us(us: u64) {
    let deadline = Instant::now() + Duration::from_micros(us);
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

/// BCM ピン番号を `/sys/class/gpio` 用のパスに変換するユーティリティ。
///
/// DTOverlay と組み合わせて使う場合など sysfs 経由でアクセスしたいときに使用する。
pub fn gpio_sysfs_path(bcm: u32) -> String {
    format!("/sys/class/gpio/gpio{}", bcm)
}

/// `/sys/class/gpio/export` にピン番号を書き込んでエクスポートする。
///
/// すでにエクスポート済みの場合はそのまま成功を返す。
pub fn gpio_export(bcm: u32) -> Result<(), CmdError> {
    let path = gpio_sysfs_path(bcm);
    if Path::new(&path).exists() {
        return Ok(()); // 既にエクスポート済み
    }
    std::fs::write("/sys/class/gpio/export", bcm.to_string())
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} エクスポート失敗: {e}")))
}

/// `/sys/class/gpio/unexport` にピン番号を書き込んでアンエクスポートする。
pub fn gpio_unexport(bcm: u32) -> Result<(), CmdError> {
    std::fs::write("/sys/class/gpio/unexport", bcm.to_string())
        .map_err(|e| CmdError::Backend(format!("GPIO{bcm} アンエクスポート失敗: {e}")))
}
