# 単体運用 (実装済み API 一覧)

この文書は、**実際に実装されている API だけ**を記載します。

## バックエンド

CmdLibは2つのバックエンドを提供します：

- **SimBackend**: シミュレーション用（テスト・デバッグ向け）
  - `use_sim_backend()` で有効化
  - 実機不要で全API動作確認可能
  
- **RealBackend**: 実機用（Raspberry Pi 3/4/5対応）
  - `use_real_backend()` で有効化
  - `/dev/gpiochip*` 経由でGPIO直接制御
  - `/sys/class/pwm/pwmchip*` 経由でハードウェアPWM制御
  - GPIO 12, 13, 18, 19でanalogWrite()がハードウェアPWM動作

## 1. Arduino 互換 API

### Digital I/O

- `digitalRead(pin)`
- `digitalWrite(pin, value)`
- `pinMode(pin, mode)`

### Math

- `abs(x)`
- `constrain(x, low, high)`
- `map(value, from_low, from_high, to_low, to_high)`
- `max(a, b)`
- `min(a, b)`
- `pow(base, exp)`
- `sq(x)`
- `sqrt(x)`

### Bits and Bytes

- `bit(bit_index)`
- `bitClear(value, bit_index)`
- `bitRead(value, bit_index)`
- `bitSet(value, bit_index)`
- `bitWrite(value, bit_index, bit_value)`
- `highByte(value)`
- `lowByte(value)`

### Analog I/O

- `analogRead(pin)` ※Raspberry Piは内蔵ADC非搭載、外部ADC（MCP3008等）必要
- `analogReadResolution(bits)`
- `analogReference(reference)`
- `analogWrite(pin, value)` **★実機でハードウェアPWM動作（GPIO 12, 13, 18, 19）**
- `analogWriteResolution(bits)` デフォルト8bit（0-255）、最大16bit（0-65535）

### Trigonometry

- `cos(x)`
- `sin(x)`
- `tan(x)`

### External Interrupts

- `attachInterrupt(interrupt, callback)`
- `detachInterrupt(interrupt)`
- `digitalPinToInterrupt(pin)`

### Advanced I/O

- `noTone(pin)`
- `pulseIn(pin, state, timeout_us)`
- `pulseInLong(pin, state, timeout_us)`
- `shiftIn(data_pin, clock_pin, bit_order)`
- `shiftOut(data_pin, clock_pin, bit_order, value)`
- `tone(pin, frequency_hz, duration_ms)`

### Characters

- `isAlpha(c)`
- `isAlphaNumeric(c)`
- `isAscii(c)`
- `isControl(c)`
- `isDigit(c)`
- `isGraph(c)`
- `isHexadecimalDigit(c)`
- `isLowerCase(c)`
- `isPrintable(c)`
- `isPunct(c)`
- `isSpace(c)`
- `isUpperCase(c)`
- `isWhitespace(c)`

### Interrupts

- `interrupts()`
- `noInterrupts()`

### Time

- `delay(ms)`
- `delayMicroseconds(us)`
- `micros()`
- `millis()`

### Random Numbers

- `random(max_exclusive)`
- `randomSeed(seed)`
- `randomRange(min_inclusive, max_exclusive)`

## 2. Communication

### SPI

- `SPI::open(path, speed_hz, mode)`
- `SPI::transfer(tx)`

### Print

- `Print` trait: `print()`, `println()`

### Serial

- `Serial::begin(path, baud)`
- `Serial::available()`
- `Serial::read_line()`
- `Serial::write_str()`

### Stream

- `Stream` trait: `read_bytes()`, `write_bytes()`

### Wire (I2C)

- `Wire::begin(bus, address)`
- `Wire::write(data)`
- `Wire::read(len)`

### USB

- `USB::list_devices()`

### Keyboard

- `Keyboard::list_devices()`

### Mouse

- `Mouse::list_devices()`
- `Mouse::read_delta_once()`

### Wi-Fi

- `WiFiOverview()`
- `scanWiFiNetworks()`
- `IPAddress`
- `WiFiClient::connect(addr)`
- `WiFiServer::bind(addr)`
- `WiFiUDP::bind(addr)`

## 3. 親が子へ実行コードを送る API

- `define_child_program!(name, |program| { ... })`
- `send_child_program!(child_id, program_name)`
- `run_child_program!(program_name)`

## 4. 実行権限（RealBackend使用時）

実機で使う場合は以下のデバイスへのアクセス権限が必要です：

| デバイス | パス | 推奨グループ | 用途 |
|---------|------|-------------|------|
| GPIO | `/dev/gpiochip*` | `gpio` | digitalWrite/Read, pinMode |
| PWM | `/sys/class/pwm/pwmchip*` | - | analogWrite (ハードウェアPWM) |
| SPI | `/dev/spidev*` | `spi` | SPI通信 |
| I2C | `/dev/i2c-*` | `i2c` | Wire (I2C通信) |
| Serial | `/dev/tty*` | `dialout` | Serial通信 |
| Input | `/dev/input/*` | `input` | Keyboard/Mouse |
| Wi-Fi | `nmcli` | - | WiFi制御 |

**セットアップ例:**
```bash
sudo usermod -a -G gpio,spi,i2c,dialout,input $USER
# 再ログイン後に有効
```

## 5. 動作確認コマンド

### シミュレーション（実機不要）
```bash
cd CmdLib
cargo test
cargo run --example standalone_drive
```

### 実機テスト（要：Raspberry Pi 3/4/5）
```bash
cd CmdLib
# GPIO/PWM実機テスト
cargo run --example real_gpio_pwm --release

# 実行前に権限確認
ls -l /dev/gpiochip0
groups  # gpioグループに所属しているか確認
```

### ハードウェアPWM対応ピン

`analogWrite()` で以下のピンがハードウェアPWMとして動作：
- **GPIO 12** (Pin 32)
- **GPIO 13** (Pin 33)  
- **GPIO 18** (Pin 12) ← おすすめ
- **GPIO 19** (Pin 35)

