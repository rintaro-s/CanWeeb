# Marrio - Face Control Edition

マリオ風ゲームを **顔・超音波センサ・ロータリーエンコーダ** で操作する分散制御デモです。  
CanWeeb (メッシュネットワーク) と CmdLib (GPIO/シリアル制御) を組み合わせています。

---

## システム構成

```
[PC]  ──LAN──  [ラズパイA]  ──USB Serial──  [Arduino]
                                               HC-SR04 (TRIG=12, ECHO=13)
[PC]  ──LAN──  [ラズパイB]  ──GPIO──  3ピンロータリーエンコーダ (CLK=GPIO17, DT=GPIO18, GND)
```

| ノード | node_id | 役割 |
|--------|---------|------|
| PC | `marrio-pc` | ゲーム描画・カメラ・CanWeeb 親 |
| ラズパイ A | `marrio-raspi-a` | Arduino から超音波センサデータを受信 → jump イベント送信 |
| ラズパイ B | `marrio-raspi-b` | GPIO で 3ピンロータリーエンコーダを直接監視 → move イベント送信 |
| Arduino | — | HC-SR04 距離をシリアル出力 |

---

## ファイル構成

```
examples/Marrio/
├── config/
│   ├── pc.toml           # PC 用 CanWeeb 設定
│   ├── raspi_a.toml      # ラズパイ A 用 CanWeeb 設定
│   └── raspi_b.toml      # ラズパイ B 用 CanWeeb 設定
├── pc/
│   ├── marrio_game.py    # ゲーム本体 (Python)
│   └── requirements.txt
├── raspi_a/
│   ├── Cargo.toml
│   └── src/main.rs       # Arduino シリアル読み取り → CWB jump 送信
├── raspi_b/
│   ├── Cargo.toml
│   └── src/main.rs       # GPIO 3ピンロータリーエンコーダ直接監視 → CWB move 送信
├── arduino/
│   └── marrio_ultrasonic/
│       └── marrio_ultrasonic.ino  # HC-SR04 距離測定
└── stm32/                # (不要)
    └── marrio_encoder/
        ├── marrio_encoder.c
        └── marrio_encoder.h
```

---

## 操作方法

| 操作 | 手段 |
|------|------|
| **左右移動** | カメラで顔を左右に動かす / ロータリーエンコーダ (RasPi-B) / キーボード ←→ |
| **ジャンプ** | 超音波センサに手・体を近づける (RasPi-A) / キーボード Space・↑ |
| **アイテム取得** | 口を開く (口パク) でコインを食べる / マウス左クリック (テスト用) |

---

## セットアップ

### 1. CanWeeb ノード起動

各マシンでリポジトリルートから起動します。

**PC:**
```bash
cargo run --release --bin canweeb -- --config examples/Marrio/config/pc.toml
```

**ラズパイ A:**
```bash
cargo run --release --bin canweeb -- --config examples/Marrio/config/raspi_a.toml
```

**ラズパイ B:**
```bash
cargo run --release --bin canweeb -- --config examples/Marrio/config/raspi_b.toml
```

> 同一 LAN 内であれば `discovery` により自動でピア接続されます。

---

### 2. Arduino スケッチ書き込み

`arduino/marrio_ultrasonic/marrio_ultrasonic.ino` を Arduino IDE で書き込みます。

**配線 (HC-SR04):**

| HC-SR04 | Arduino |
|---------|----------|
| TRIG | ピン 12 |
| ECHO | ピン 13 |
| VCC | 5V |
| GND | GND |

---

### 3. ラズパイ A — Arduino シリアル連携起動

```bash
cd examples/Marrio/raspi_a

# シリアルポートを自動検出する場合
CANWEEB_API=http://localhost:8080 cargo run --release

# ポートを明示する場合
CANWEEB_API=http://localhost:8080 \
SERIAL_PORT=/dev/ttyACM0 \
BAUD_RATE=9600 \
JUMP_THRESHOLD_CM=30 \
cargo run --release
```

---

### 4. ラズパイ B — 3ピンロータリーエンコーダ起動

**重要:** raspi_b は起動時に **pinctrl** で GPIO ピンを自動設定します。

```bash
cd examples/Marrio/raspi_b

# デフォルト設定 (GPIO_CLK=17, GPIO_DT=18)
# 起動時に pinctrl でピンを入力・プルアップに設定します
CANWEEB_API=http://localhost:8080 cargo run --release

# GPIO ピンを変更する場合
CANWEEB_API=http://localhost:8080 \
GPIO_CLK=17 \
GPIO_DT=18 \
DEBOUNCE_US=1000 \
COUNT_THRESHOLD=2 \
cargo run --release
```

**配線 (3ピンロータリーエンコーダ):**

| エンコーダ | Raspberry Pi |
|-----------|-------------|
| CLK (A相) | GPIO 17 (BCM) |
| DT (B相) | GPIO 18 (BCM) |
| GND | GND |

※ 3ピンエンコーダは CLK, DT, GND の3本のみです。電源は Raspberry Pi 内部のプルアップを使用します。

**初期化手順:**
1. プログラムが `pinctrl set <pin> ip pu` でピンを入力・プルアップに設定
2. GPIO を読み取ってエンコーダ値を監視
3. 回転を検出して CanWeeb 経由で PC に送信

---

### 5. PC ゲーム起動

```bash
cd examples/Marrio/pc
pip install -r requirements.txt
CANWEEB_API=http://<PC_IP>:8080 python marrio_game.py
```

**環境変数:**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | PC の CanWeeb API |
| `CAMERA_INDEX` | `0` | カメラデバイス番号 |

**raspi_a 環境変数:**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |
| `SERIAL_PORT` | (自動検出) | Arduino シリアルポート |
| `BAUD_RATE` | `9600` | シリアル通信速度 |
| `JUMP_THRESHOLD_CM` | `30` | ジャンプ判定距離 (cm) |

**raspi_b 環境変数:**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |
| `GPIO_CLK` | `17` | CLK (A相) ピン (BCM) |
| `GPIO_DT` | `18` | DT (B相) ピン (BCM) |
| `DEBOUNCE_US` | `1000` | デバウンス時間 (µs) |
| `COUNT_THRESHOLD` | `2` | 移動判定カウント閾値 |

---

## CanWeeb トピック仕様

| Topic | 方向 | Traffic Class | Payload (JSON) |
|-------|------|---------------|----------------|
| `marrio/input/jump` | RasPi-A → PC | **control** | `{"event":"jump","distance_cm":24.5,"source":"raspi-a"}` |
| `marrio/input/move` | RasPi-B → PC | **control** | `{"direction":"left"\|"right","delta":-2,"source":"raspi-b-gpio"}` |

> **control** トピックは inbox に永続化され、WebSocket `/ws/inbox` で確実に配信されます。

---

## ゲームルール

- コインブロック (`?`) をジャンプで下から叩くとコインがポップアップ
- 口を開いて近くのコインを食べると 100pt
- 敵 (赤い楕円) を上から踏むと 200pt
- 画面右端のゴールフラグに触れるとステージクリア
- 穴に落ちるか敵に当たるとミス (ライフ 3 つ)
- ライフ 0 でゲームオーバー

---

## トラブルシューティング

### カメラが起動しない
```bash
CAMERA_INDEX=1 python marrio_game.py
```
カメラが使えない場合はマウス操作にフォールバックします。

### CanWeeb ノードが接続されない
- 全ノードが同一 LAN にいるか確認
- ファイアウォールでポート `7002` (TCP) と `7060` (UDP) を開放
- `discovery.enabled = true` になっているか確認
- CanWeeb ノード起動時に `--bin canweeb` を明示して起動しているか確認

### シリアルポートが見つからない (raspi_a)
```bash
ls /dev/ttyACM* /dev/ttyUSB*
# Arduino が接続されているか確認
SERIAL_PORT=/dev/ttyACM0 cargo run --release
```

### Arduino が応答しない
- Arduino が USB 接続されているか確認
- Arduino IDE のシリアルモニタで距離データが出力されているか確認
- ボーレートが 9600 で一致しているか確認

### pinctrl が実行できない (raspi_b)
```bash
# pinctrl がインストールされているか確認
which pinctrl

# インストールされていない場合
sudo apt-get install raspi-gpio

# sudo 権限が必要な場合
sudo cargo run --release
```

### GPIO が見つからない (raspi_b)
```bash
# GPIO チップの確認
ls -la /dev/gpiochip*
# ユーザーが gpio グループに属しているか確認
groups $USER

# gpio グループに追加 (要再ログイン)
sudo usermod -aG gpio $USER
```

### ロータリーエンコーダが反応しない
- CLK・DT・GND ピンの配線を確認
- 3ピンエンコーダは CLK, DT, GND の3本のみです（電源ピンは不要）
- pinctrl でピンが正しく設定されているか確認: `pinctrl get 17` と `pinctrl get 18`
- デバウンス時間を調整: `DEBOUNCE_US=2000` (大きくするとノイズに強くなる)
- カウント閾値を調整: `COUNT_THRESHOLD=1` (小さくすると感度が上がる)
