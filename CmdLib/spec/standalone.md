# 単体運用 (実装済み API 一覧)

この文書は、**実際に実装されている API だけ**を記載します。

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

- `analogRead(pin)`
- `analogReadResolution(bits)`
- `analogReference(reference)`
- `analogWrite(pin, value)`
- `analogWriteResolution(bits)`

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

## 4. 実行権限

実機で使う場合は権限が必要です。

- GPIO: `/dev/gpiochip*`
- SPI: `/dev/spidev*`
- I2C: `/dev/i2c-*`
- Serial: `/dev/tty*`
- Input (Keyboard/Mouse): `/dev/input/*`
- Wi-Fi 制御: `nmcli` 実行権限

## 5. 動作確認コマンド

```bash
cd CmdLib
cargo test
cargo run --example standalone_drive
```

