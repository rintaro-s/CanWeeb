# Loli - Rotary Encoder Direction Visualizer

*Note: Some student engineers in Japan jokingly refer to rotary encoders as `lolicon`.*

Loli is a demo that detects the rotation of a 3-pin rotary encoder and visualizes the current direction in eight directions.

---

## System Overview

```
[Raspberry Pi (child)]  ──CanWeeb──  [PC (parent)]
  encoder_sender                   visualizer
  - Uses DTOverlay                 - WebSocket monitoring
  - pinctrl configuration          - Real-time display
  - Direct GPIO reading            - 8-direction visualization
```

| Component | Role |
|--------------|------|
| `encoder_sender` (Rust) | **Child**: Reads a 3-pin rotary encoder using DTOverlay + pinctrl + direct GPIO access, then sends the position through CanWeeb |
| `loli_visualizer.py` (Python) | **Parent**: Receives position updates over the CanWeeb WebSocket without missing events and displays them in eight directions |

---

## File Layout

```
examples/loli/
├── encoder_sender/          # Child script (Rust)
│   ├── Cargo.toml
│   └── src/main.rs          # DTOverlay + pinctrl + direct GPIO reading
├── visualizer/              # Parent script (Python)
│   ├── loli_visualizer.py   # WebSocket monitoring + visualization
│   └── requirements.txt
└── README.md
```

---

## Eight-Direction Mapping

The initial position is **north (up)**. As the encoder rotates, the visualizer moves through eight directions:

```
       N ↑
   NW ↖   |   ↗ NE
      \   |   /
W ←────┼────→ E
      /   |   \
   SW ↙   |   ↘ SE
       S ↓
```

| Direction | Symbol | Description |
|-----|------|------|
| N   | ↑    | North (initial position) |
| NE  | ↗    | Northeast |
| E   | →    | East |
| SE  | ↘    | Southeast |
| S   | ↓    | South |
| SW  | ↙    | Southwest |
| W   | ←    | West |
| NW  | ↖    | Northwest |

By default, the direction changes every 4 steps, so one full turn is 32 steps in total.

---

## Setup

### 1. Start the CanWeeb nodes

**PC (parent):**
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/pc.toml
```

After startup, open `http://localhost:8080` in your browser to check the Web UI.

**Raspberry Pi (child):**
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/raspi.toml
```

> If both nodes are on the same LAN, discovery will connect them automatically.
> Once the peer connection is established, the log will show a message such as `Connected to peer: loli-visualizer`.

---

### 2. Start the child script (Raspberry Pi)

**Important:** This script **always uses DTOverlay**.

```bash
cd examples/loli/encoder_sender

# Default settings (GPIO_CLK=17, GPIO_DT=18)
# The rotary-encoder DTOverlay is loaded automatically
CANWEEB_API=http://localhost:8080 cargo run --release

# When changing GPIO pins
CANWEEB_API=http://localhost:8080 \
GPIO_CLK=17 \
GPIO_DT=18 \
DEBOUNCE_US=1000 \
cargo run --release
```

**Startup log example:**
```
─── DTOverlay setup ─────────────────────────────────
  Loading DTOverlay rotary-encoder...
  ✓ DTOverlay rotary-encoder loaded
  Currently loaded DTOverlays:
    - rotary-encoder
────────────────────────────────────────────────────

─── GPIO pin setup (pinctrl) ───────────────────────
  Setting GPIO17 to input with pull-up...
  Setting GPIO18 to input with pull-up...
  ✓ GPIO17: 17: ip    pu | hi // GPIO17 = input
  ✓ GPIO18: 18: ip    pu | hi // GPIO18 = input
────────────────────────────────────────────────────

━━━ Rotary encoder monitoring started ━━━
━━━ DTOverlay + pinctrl + direct GPIO reading ━━━
```

**Wiring (3-pin rotary encoder):**

| Encoder | Raspberry Pi |
|-----------|-------------|
| CLK (A phase) | GPIO 17 (BCM) |
| DT (B phase) | GPIO 18 (BCM) |
| GND | GND |

Only three wires are used for a 3-pin encoder: CLK, DT, and GND.

---

### 3. Start the parent script (PC)

```bash
cd examples/loli/visualizer

# Install dependencies
pip install -r requirements.txt

# Start the visualizer
CANWEEB_API=http://localhost:8080 python loli_visualizer.py
```

**Example output:**
```
============================================================
  Loli Visualizer - Rotary Encoder Position Visualizer
============================================================
  CANWEEB_API: http://localhost:8080
  WebSocket:   ws://localhost:8080/ws/inbox
  Directions:  8
  Steps/dir:   4
============================================================

WebSocket connected: ws://localhost:8080/ws/inbox
Monitoring encoder position updates...
Press Ctrl+C to exit

[recv:     12] position:     48  direction: → East (E)

============================================================
  Position: 48
  Direction: → East (E)
  Total received: 12
  Last update: 19:45:23.456
============================================================
```

---

## Environment Variables

**`encoder_sender` (child):**

| Variable | Default | Description |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |
| `GPIO_CLK` | `17` | CLK (A phase) pin (BCM) |
| `GPIO_DT` | `18` | DT (B phase) pin (BCM) |
| `DEBOUNCE_US` | `1000` | Debounce time (µs) |
| `USE_DTOVERLAY` | `true` | Whether to use DTOverlay (**must remain true**) |

**`loli_visualizer.py` (parent):**

| Variable | Default | Description |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |

---

## CanWeeb Topic Specification

| Topic | Direction | Traffic Class | Payload (JSON) |
|-------|------|---------------|----------------|
| `loli/encoder/position` | Raspberry Pi → PC | **control** | `{"position":48,"delta":4,"source":"loli-encoder-sender"}` |

> `control` topics are persisted in the inbox and delivered reliably over WebSocket `/ws/inbox`.

---

## Technical Details

### DTOverlay usage

`encoder_sender` **always uses DTOverlay**:

1. **Load DTOverlay:**
   ```rust
   DtOverlay::load(
       "rotary-encoder",
       &[
           ("pin_a", "17"),
           ("pin_b", "18"),
           ("relative_axis", "1"),
           ("steps-per-period", "1"),
       ],
   )?;
   ```

2. **Configure pinctrl:**
   ```rust
   pinctrl_set(17, "ip", "pu")?;  // GPIO17 input + pull-up
   pinctrl_set(18, "ip", "pu")?;  // GPIO18 input + pull-up
   ```

3. **Read GPIO directly:**
   ```rust
   let encoder = GpioRotaryEncoder3Pin::new(17, 18).debounce_us(1000);
   encoder.start()?;
   ```

### Real-time monitoring over WebSocket

`loli_visualizer.py` uses WebSocket `/ws/inbox` to receive every message without missing events:

- Real-time reception over WebSocket instead of HTTP polling
- `control` topics are persisted in the inbox, so delivery is reliable
- JSON is parsed directly from `payload_preview` for fast handling

---

## Troubleshooting

### DTOverlay cannot be loaded

```bash
# Check whether DTOverlay is enabled
sudo raspi-config
# → 3 Interface Options → I5 Device Tree Overlays → Enable

# Load manually
sudo dtoverlay rotary-encoder pin_a=17 pin_b=18 relative_axis=1 steps-per-period=1

# Check loaded overlays
dtoverlay -l
```

### pinctrl cannot run

```bash
# Check whether pinctrl is installed
which pinctrl

# Install if missing
sudo apt-get install raspi-gpio

# If sudo privileges are required
sudo cargo run --release
```

### WebSocket connection error

```bash
# Check whether websocket-client is installed
pip list | grep websocket

# Install it
pip install websocket-client

# Check whether CanWeeb is running
curl http://localhost:8080/api/status
```

### Encoder does not respond

- Check the CLK / DT / GND wiring
- Confirm the pins are configured correctly with `pinctrl get 17` and `pinctrl get 18`
- Confirm DTOverlay is loaded with `dtoverlay -l`
- Adjust debounce time: `DEBOUNCE_US=2000`

---

## License

This sample code is part of the CanWeeb project.

---

## 日本語ver

# Loli - ロータリーエンコーダ方向ビジュアライザ

3ピンロータリーエンコーダの回転を検出し、8方向で現在の向きをビジュアライズするデモです。

---

## システム構成

```
[Raspberry Pi (子)]  ──CanWeeb──  [PC (親)]
  encoder_sender                   visualizer
  - DTOverlay 使用                 - WebSocket 監視
  - pinctrl 設定                   - リアルタイム表示
  - GPIO 直接読み取り              - 8方向ビジュアライズ
```

| コンポーネント | 役割 |
|--------------|------|
| `encoder_sender` (Rust) | **子**: DTOverlay + pinctrl + GPIO で3ピンエンコーダを読み取り、CanWeeb に送信 |
| `loli_visualizer.py` (Python) | **親**: CanWeeb WebSocket で位置情報を漏らさずキャッチし、8方向で表示 |

---

## ファイル構成

```
examples/loli/
├── encoder_sender/          # 子スクリプト (Rust)
│   ├── Cargo.toml
│   └── src/main.rs          # DTOverlay + pinctrl + GPIO 直接読み取り
├── visualizer/              # 親スクリプト (Python)
│   ├── loli_visualizer.py   # WebSocket 監視 + ビジュアライズ
│   └── requirements.txt
└── README.md
```

---

## 8方向定義

初期位置を**北（上）**として、エンコーダを回すと8方向で表示されます：

```
       N (北) ↑
   NW ↖   |   ↗ NE
      \   |   /
W ←────┼────→ E
      /   |   \
   SW ↙   |   ↘ SE
       S (南) ↓
```

| 方向 | 記号 | 説明 |
|-----|------|------|
| N   | ↑    | 北（初期位置） |
| NE  | ↗    | 北東 |
| E   | →    | 東 |
| SE  | ↘    | 南東 |
| S   | ↓    | 南 |
| SW  | ↙    | 南西 |
| W   | ←    | 西 |
| NW  | ↖    | 北西 |

デフォルトでは1方向あたり4ステップで切り替わります（合計32ステップで1周）。

---

## セットアップ

### 1. CanWeeb ノード起動

**PC (親):**
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/pc.toml
```

起動後、ブラウザで http://localhost:8080 にアクセスして Web UI を確認できます。

**Raspberry Pi (子):**
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/raspi.toml
```

> 同一 LAN 内であれば discovery により自動でピア接続されます。
> ピア接続が確立すると、ログに `Connected to peer: loli-visualizer` のようなメッセージが表示されます。

---

### 2. 子スクリプト起動 (Raspberry Pi)

**重要:** このスクリプトは **DTOverlay を必ず使用**します。

```bash
cd examples/loli/encoder_sender

# デフォルト設定 (GPIO_CLK=17, GPIO_DT=18)
# DTOverlay rotary-encoder を自動ロード
CANWEEB_API=http://localhost:8080 cargo run --release

# GPIO ピンを変更する場合
CANWEEB_API=http://localhost:8080 \
GPIO_CLK=17 \
GPIO_DT=18 \
DEBOUNCE_US=1000 \
cargo run --release
```

**起動時のログ:**
```
─── DTOverlay 設定 ─────────────────────────────────
  DTOverlay rotary-encoder をロード中...
  ✓ DTOverlay rotary-encoder をロードしました
  現在ロード済みの DTOverlay:
    - rotary-encoder
────────────────────────────────────────────────────

─── GPIO ピン設定 (pinctrl) ───────────────────────
  GPIO17 を入力・プルアップに設定中...
  GPIO18 を入力・プルアップに設定中...
  ✓ GPIO17 設定: 17: ip    pu | hi // GPIO17 = input
  ✓ GPIO18 設定: 18: ip    pu | hi // GPIO18 = input
────────────────────────────────────────────────────

━━━ ロータリーエンコーダ監視開始 ━━━
━━━ DTOverlay + pinctrl + GPIO 直接読み取り ━━━
```

**配線 (3ピンロータリーエンコーダ):**

| エンコーダ | Raspberry Pi |
|-----------|-------------|
| CLK (A相) | GPIO 17 (BCM) |
| DT (B相) | GPIO 18 (BCM) |
| GND | GND |

※ 3ピンエンコーダは CLK, DT, GND の3本のみです。

---

### 3. 親スクリプト起動 (PC)

```bash
cd examples/loli/visualizer

# 依存関係をインストール
pip install -r requirements.txt

# ビジュアライザを起動
CANWEEB_API=http://localhost:8080 python loli_visualizer.py
```

**実行例:**
```
============================================================
  Loli Visualizer - ロータリーエンコーダ位置ビジュアライザ
============================================================
  CANWEEB_API: http://localhost:8080
  WebSocket:   ws://localhost:8080/ws/inbox
  方向数:      8 方向
  ステップ/方向: 4
============================================================

WebSocket 接続成功: ws://localhost:8080/ws/inbox
エンコーダ位置情報を監視中...
Ctrl+C で終了

[受信:     12] 位置:     48  方向: → 東 (E)  

============================================================
  位置: 48
  方向: → 東 (E)
  受信総数: 12
  最終更新: 19:45:23.456
============================================================
```

---

## 環境変数

**encoder_sender (子):**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |
| `GPIO_CLK` | `17` | CLK (A相) ピン (BCM) |
| `GPIO_DT` | `18` | DT (B相) ピン (BCM) |
| `DEBOUNCE_US` | `1000` | デバウンス時間 (µs) |
| `USE_DTOVERLAY` | `true` | DTOverlay を使用するか（**必ず true にすること**） |

**loli_visualizer.py (親):**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |

---

## CanWeeb トピック仕様

| Topic | 方向 | Traffic Class | Payload (JSON) |
|-------|------|---------------|----------------|
| `loli/encoder/position` | Raspberry Pi → PC | **control** | `{"position":48,"delta":4,"source":"loli-encoder-sender"}` |

> **control** トピックは inbox に永続化され、WebSocket `/ws/inbox` で確実に配信されます。

---

## 技術詳細

### DTOverlay の使用

`encoder_sender` は **DTOverlay を必ず使用**します：

1. **DTOverlay ロード:**
   ```rust
   DtOverlay::load(
       "rotary-encoder",
       &[
           ("pin_a", "17"),
           ("pin_b", "18"),
           ("relative_axis", "1"),
           ("steps-per-period", "1"),
       ],
   )?;
   ```

2. **pinctrl 設定:**
   ```rust
   pinctrl_set(17, "ip", "pu")?;  // GPIO17 を入力・プルアップ
   pinctrl_set(18, "ip", "pu")?;  // GPIO18 を入力・プルアップ
   ```

3. **GPIO 直接読み取り:**
   ```rust
   let encoder = GpioRotaryEncoder3Pin::new(17, 18).debounce_us(1000);
   encoder.start()?;
   ```

### WebSocket によるリアルタイム監視

`loli_visualizer.py` は WebSocket `/ws/inbox` を使用して、すべてのメッセージを漏らさずキャッチします：

- HTTP ポーリングではなく WebSocket でリアルタイム受信
- `control` トピックは inbox に永続化されるため、確実に配信される
- `payload_preview` から直接 JSON をパースして高速処理

---

## トラブルシューティング

### DTOverlay がロードできない

```bash
# DTOverlay が有効か確認
sudo raspi-config
# → 3 Interface Options → I5 Device Tree Overlays → Enable

# 手動でロード
sudo dtoverlay rotary-encoder pin_a=17 pin_b=18 relative_axis=1 steps-per-period=1

# ロード済みを確認
dtoverlay -l
```

### pinctrl が実行できない

```bash
# pinctrl がインストールされているか確認
which pinctrl

# インストールされていない場合
sudo apt-get install raspi-gpio

# sudo 権限が必要な場合
sudo cargo run --release
```

### WebSocket 接続エラー

```bash
# websocket-client がインストールされているか確認
pip list | grep websocket

# インストール
pip install websocket-client

# CanWeeb が起動しているか確認
curl http://localhost:8080/api/status
```

### エンコーダが反応しない

- CLK・DT・GND ピンの配線を確認
- pinctrl でピンが正しく設定されているか確認: `pinctrl get 17` と `pinctrl get 18`
- DTOverlay がロードされているか確認: `dtoverlay -l`
- デバウンス時間を調整: `DEBOUNCE_US=2000`

---

## ライセンス

このサンプルコードは CanWeeb プロジェクトの一部です。
