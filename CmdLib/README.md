# CANweeb CmdLib

Raspberry Pi を Arduino / ESP32 風に扱うための Rust ライブラリ実装です。  
**Raspberry Pi 3, 4, 5 で実機動作確認済み**のGPIO/PWM制御ライブラリです。

## 特徴

- **実機対応**: `/dev/gpiochip*` 経由でRaspberry Pi 3/4/5のGPIOを直接制御
- **ハードウェアPWM対応**: `analogWrite()` がGPIO 12, 13, 18, 19でハードウェアPWMとして動作
- **Arduino互換API**: `pinMode()`, `digitalWrite()`, `digitalRead()`, `analogWrite()` など
- **シミュレーションモード**: テストやデバッグ用の仮想バックエンドも搭載

## 実装済み API

Arduino 互換関数として、以下を実装しています。

- Digital I/O: `digitalRead()`, `digitalWrite()`, `pinMode()`
- Math: `abs()`, `constrain()`, `map()`, `max()`, `min()`, `pow()`, `sq()`, `sqrt()`
- Bits and Bytes: `bit()`, `bitClear()`, `bitRead()`, `bitSet()`, `bitWrite()`, `highByte()`, `lowByte()`
- Analog I/O: `analogRead()`, `analogReadResolution()`, `analogReference()`, `analogWrite()`, `analogWriteResolution()`
- Rotary Encoder: `RotaryEncoder` / `encoderRead()` / `encoderReset()`
- Trigonometry: `cos()`, `sin()`, `tan()`
- External Interrupts: `attachInterrupt()`, `detachInterrupt()`, `digitalPinToInterrupt()`
- Advanced I/O: `noTone()`, `pulseIn()`, `pulseInLong()`, `shiftIn()`, `shiftOut()`, `tone()`
- Characters: `isAlpha()`, `isAlphaNumeric()`, `isAscii()`, `isControl()`, `isDigit()`, `isGraph()`, `isHexadecimalDigit()`, `isLowerCase()`, `isPrintable()`, `isPunct()`, `isSpace()`, `isUpperCase()`, `isWhitespace()`
- Interrupts: `interrupts()`, `noInterrupts()`
- Time: `delay()`, `delayMicroseconds()`, `micros()`, `millis()`
- Random Numbers: `random()`, `randomSeed()`
- Communication: `SPI`, `Print`, `Serial`, `Stream`, `Wire`, `USB`, `Keyboard`, `Mouse`, `Wi-Fi`, `WiFiNetwork`, `IPAddress`, `WiFiClient`, `WiFiServer`, `WiFiUDP`

親が関数内で子実行プログラムを定義して送る API も実装済みです。

- `define_child_program!`
- `send_child_program!`
- `run_child_program!`

## 使用例

### 基本的なGPIO制御（実機）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    // 実機バックエンドを使用（ラズパイのGPIOを直接制御）
    use_real_backend()?;

    // GPIO17を出力に設定
    pinMode("17", PinMode::Output)?;
    
    // LED点滅
    for _ in 0..5 {
        digitalWrite("17", true)?;
        delay(500);
        digitalWrite("17", false)?;
        delay(500);
    }

    Ok(())
}
```

### PWM制御（analogWrite）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    use_real_backend()?;

    // GPIO18でPWM出力（ハードウェアPWM対応ピン）
    // デフォルト: 8bit解像度（0-255）
    for duty in (0..=255).step_by(25) {
        analogWrite(18, duty)?;
        delay(100);
    }

    // 10bit解像度に変更（0-1023）
    analogWriteResolution(10)?;
    analogWrite(18, 512)?;  // 50% duty

    Ok(())
}
```

    ### より Arduino っぽい書き方

    ```rust
    use canweeb_cmdlib::prelude::*;

    fn main() -> Result<(), CmdError> {
        use_real_backend()?;

        let mut fan = PwmOutput::new(18)
            .frequency(25_000)
            .range(0.0, 100.0)
            .duty_percent(30.0);

        fan.start()?;
        fan.step_duty(5.0)?;
        fan.set_frequency(22_000)?;

        Ok(())
    }
    ```

### シミュレーションモード（テスト用）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    // シミュレーションバックエンドを使用（実機不要）
    use_sim_backend()?;

    pinMode("17", PinMode::Output)?;
    digitalWrite("17", true)?;
    let state = digitalRead("17")?;
    assert!(state);

    Ok(())
}
```

    ### ロータリーエンコーダの読み取り

    `RotaryEncoder` は GPIO のエッジイベントをバックグラウンドで監視し、カウントを常時積算します。`read()` は今の累積値を返します。

    ```rust
    use canweeb_cmdlib::prelude::*;

    fn main() -> Result<(), CmdError> {
        use_real_backend()?;

        let encoder = RotaryEncoder::new(17, 18)?;
        loop {
            let count = encoder.read()?;
            println!("encoder count: {}", count);
            delay(10);
        }
    }
    ```

## Raspberry Pi セットアップ

### 対応ハードウェア

- **Raspberry Pi 3** (Model B, B+)
- **Raspberry Pi 4** (全モデル)
- **Raspberry Pi 5** (全モデル)

### 必要権限 (実機)

実機でGPIO/PWMを使用する場合、以下のデバイスへのアクセス権限が必要です：

#### GPIO制御
```bash
# ユーザーをgpioグループに追加
sudo usermod -a -G gpio $USER

# または一時的に権限付与
sudo chmod 666 /dev/gpiochip0
```

#### PWM制御（analogWrite用）
```bash
# PWMデバイスへのアクセス権
sudo chmod 666 /sys/class/pwm/pwmchip0/export
sudo chmod 666 /sys/class/pwm/pwmchip0/unexport
```

#### その他デバイス
- **SPI**: `/dev/spidev*` → `spi` グループ
- **I2C**: `/dev/i2c-*` → `i2c` グループ
- **Serial**: `/dev/tty*` → `dialout` グループ
- **Input**: `/dev/input/*` → `input` グループ
- **Wi-Fi**: `nmcli` 実行権限

**推奨セットアップ:**
```bash
# 必要なグループに追加
sudo usermod -a -G gpio,spi,i2c,dialout,input $USER

# 再ログインして反映
# または
newgrp gpio
```

### ハードウェアPWM対応ピン

`analogWrite()` は以下のGPIOピンでハードウェアPWMとして動作します：

| GPIOピン | PWMチップ | チャンネル | 物理ピン番号 |
|----------|-----------|------------|--------------|
| GPIO 12  | pwmchip0  | 0          | Pin 32       |
| GPIO 13  | pwmchip0  | 1          | Pin 33       |
| GPIO 18  | pwmchip0  | 0          | Pin 12       |
| GPIO 19  | pwmchip0  | 1          | Pin 35       |

**注意**: Raspberry Piには内蔵ADCがありません。`analogRead()` を使用する場合はMCP3008などの外部ADCをSPI経由で接続してください。

## ビルドと実行

### テスト実行（シミュレーション）

```bash
cd CmdLib
cargo test
```

### サンプルプログラム実行

```bash
# シミュレーションモード（実機不要）
cargo run --example standalone_drive

# 実機GPIO/PWMテスト（要：Raspberry Pi）
cargo run --example real_gpio_pwm --release

# Lchika 実行（実機）
cargo run --example Lchika --release

# いったんビルドしてバイナリを直接起動する場合
cargo build --release --example Lchika
./target/release/examples/Lchika
```

### 常駐デーモンでの登録実行

`cmdlibd` を使うと、プログラム登録・起動・停止・TTL指定・外部制御・TUI操作ができます。

```bash
# cmdlibd の起動（手動）
cargo run --release --bin cmdlibd -- daemon

# Lchika を登録
cargo run --release --bin cmdlibd -- register \
    --name lchika \
    --command "$PWD/target/release/examples/Lchika" \
    --ttl-seconds 10

# 登録済みプログラムを実行
cargo run --release --bin cmdlibd -- run --name lchika

# 実行時だけ TTL を上書き
cargo run --release --bin cmdlibd -- run --name lchika --ttl-seconds 3

# 実行中を停止
cargo run --release --bin cmdlibd -- stop --name lchika

# 登録一覧と状態確認
cargo run --release --bin cmdlibd -- list
cargo run --release --bin cmdlibd -- status
```

### 外部プログラム実行と外部制御

外部コマンドを一時実行したい場合は `run-raw` を使います。

```bash
cargo run --release --bin cmdlibd -- run-raw \
    --command /bin/sh \
    --arg -lc \
    --arg 'echo hello from daemon' \
    --ttl-seconds 5
```

外部プログラムからは、同じ `cmdlibd` CLI を呼ぶだけで制御できます（register/run/stop/status）。

### TUI モード

```bash
cargo run --release --bin cmdlibd -- tui
```

- Up/Down: 選択
- Enter: 起動
- s: 停止
- r: 更新
- q: 終了

### systemd 常駐化

```bash
# サービス導入
bash scripts/install_cmdlibd_service.sh

# 状態確認
sudo systemctl status cmdlibd.service
```

必要に応じて `systemd/cmdlibd.service` の `User` や `Group` を環境に合わせて変更してください。

### トラブルシューティング

#### `Failed to open /dev/gpiochip0: Permission denied`

権限エラーの場合：
```bash
sudo usermod -a -G gpio $USER
# 再ログインまたは
newgrp gpio
```

#### `No GPIO chip found`

Raspberry Pi以外の環境で実行しているか、カーネルモジュールが読み込まれていません：
```bash
# GPIO関連モジュールの確認
lsmod | grep gpio

# デバイスの存在確認
ls -l /dev/gpiochip*
```

#### `Failed to export PWM channel`

PWMが既に他のプロセスで使用されているか、権限がありません：
```bash
# PWMをリセット
echo 0 | sudo tee /sys/class/pwm/pwmchip0/unexport 2>/dev/null
echo 1 | sudo tee /sys/class/pwm/pwmchip0/unexport 2>/dev/null

# 権限確認
ls -l /sys/class/pwm/pwmchip0/
```

## ライセンス

MIT

