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
