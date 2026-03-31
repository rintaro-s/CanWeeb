# CANweeb CmdLib

Raspberry Pi を Arduino / ESP32 風に扱うための Rust ライブラリ実装です。  
このディレクトリには実装コードと、実装済み API に一致するドキュメントを置いています。

## 実装済み API

Arduino 互換関数として、以下を実装しています。

- Digital I/O: `digitalRead()`, `digitalWrite()`, `pinMode()`
- Math: `abs()`, `constrain()`, `map()`, `max()`, `min()`, `pow()`, `sq()`, `sqrt()`
- Bits and Bytes: `bit()`, `bitClear()`, `bitRead()`, `bitSet()`, `bitWrite()`, `highByte()`, `lowByte()`
- Analog I/O: `analogRead()`, `analogReadResolution()`, `analogReference()`, `analogWrite()`, `analogWriteResolution()`
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

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;
use canweeb_cmdlib::{define_child_program, send_child_program};

fn parent_define() -> Result<(), CmdError> {
    define_child_program!("child_boot", |program| {
        program
            .pin_mode("gpio22", "output")
            .digital_write("gpio22", "high")
            .motor_set_speed("left", 0.2)
            .uart_send("uart0", "BOOT");
    })?;
    Ok(())
}

fn main() -> Result<(), CmdError> {
    use_sim_backend()?;

    pinMode("gpio17", PinMode::Output)?;
    digitalWrite("gpio17", true)?;
    let state = digitalRead("gpio17")?;
    assert!(state);

    parent_define()?;
    let report = send_child_program!("child-01", "child_boot")?;
    assert_eq!(report.executed_steps, report.total_steps);

    Ok(())
}
```

## 必要権限 (実機)

実機アクセス時は Linux デバイス権限が必要です。

- GPIO: `/dev/gpiochip*` へのアクセス権
- SPI: `/dev/spidev*` へのアクセス権
- I2C (Wire): `/dev/i2c-*` へのアクセス権
- Serial: `/dev/tty*` へのアクセス権
- Mouse/Keyboard 読み取り: `/dev/input/*` へのアクセス権
- Wi-Fi (NetworkManager 制御): `nmcli` 実行権限

一般的には `gpio` / `spi` / `i2c` / `dialout` / `input` グループ追加や、運用に応じた `sudo` 設定が必要です。

## 検証済みコマンド

```bash
cd CmdLib
cargo test
cargo run --example standalone_drive
```

