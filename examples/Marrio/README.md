# Marrio - Face Control Edition

マリオ風ゲームを **顔・超音波センサ・ロータリーエンコーダ** で操作する分散制御デモです。  
CanWeeb (メッシュネットワーク) と CmdLib (GPIO/シリアル制御) を組み合わせています。

---

## システム構成

```
[PC]  ──LAN──  [ラズパイA]  ──GPIO──  HC-SR04 (TRIG=GPIO23, ECHO=GPIO24)
[PC]  ──LAN──  [ラズパイB]  ──GPIO──  RotaryEncoder (A=GPIO17, B=GPIO18)
```

**Arduino・STM32 は不要です。** Raspberry Pi の GPIO で直接センサを制御します。

| ノード | node_id | 役割 |
|--------|---------|------|
| PC | `marrio-pc` | ゲーム描画・カメラ・CanWeeb 親 |
| ラズパイ A | `marrio-raspi-a` | GPIO で HC-SR04 を直接読み取り → jump イベント送信 |
| ラズパイ B | `marrio-raspi-b` | GPIO でロータリーエンコーダを直接監視 → move イベント送信 |

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
│   └── src/main.rs       # GPIO HC-SR04 直接読み取り → CWB jump 送信
├── raspi_b/
│   ├── Cargo.toml
│   └── src/main.rs       # GPIO ロータリーエンコーダ直接監視 → CWB move 送信
├── arduino/              # (参考資料のみ、不要)
│   └── marrio_ultrasonic/
│       └── marrio_ultrasonic.ino
└── stm32/                # (参考資料のみ、不要)
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

### 2. ラズパイ A — GPIO 超音波センサ起動

```bash
cd examples/Marrio/raspi_a

# デフォルト設定 (GPIO_TRIG=23, GPIO_ECHO=24, JUMP_THRESHOLD_CM=30)
CANWEEB_API=http://localhost:8080 cargo run --release

# GPIO ピンを変更する場合
CANWEEB_API=http://localhost:8080 \
GPIO_TRIG=23 \
GPIO_ECHO=24 \
JUMP_THRESHOLD_CM=30 \
SENSOR_SAMPLES=3 \
MAX_DELTA_CM=50 \
cargo run --release
```

**配線 (HC-SR04):**

| HC-SR04 | Raspberry Pi |
|---------|-------------|
| TRIG | GPIO 23 (BCM) |
| ECHO | GPIO 24 (BCM) |
| VCC | 5V |
| GND | GND |

⚠️ **重要:** HC-SR04 の ECHO ピンは 5V 出力です。Raspberry Pi の GPIO は 3.3V 入力のため、**必ず抵抗分圧 (1kΩ + 2kΩ) またはレベルシフタを使用**してください。

```
HC-SR04 ECHO ──┬── 1kΩ ──┬── GPIO 24
               │          │
               └── 2kΩ ──┴── GND
```

---

### 3. ラズパイ B — GPIO ロータリーエンコーダ起動

```bash
cd examples/Marrio/raspi_b

# デフォルト設定 (GPIO_ENC_A=17, GPIO_ENC_B=18)
CANWEEB_API=http://localhost:8080 cargo run --release

# GPIO ピンを変更する場合
CANWEEB_API=http://localhost:8080 \
GPIO_ENC_A=17 \
GPIO_ENC_B=18 \
DEBOUNCE_US=500 \
MIN_PULSE_US=200 \
COUNT_THRESHOLD=2 \
cargo run --release
```

**配線 (ロータリーエンコーダ):**

| エンコーダ | Raspberry Pi |
|-----------|-------------|
| A 相 | GPIO 17 (BCM) |
| B 相 | GPIO 18 (BCM) |
| GND | GND |
| VCC | 3.3V |

---

### 4. PC ゲーム起動

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
| `GPIO_TRIG` | `23` | HC-SR04 TRIG ピン (BCM) |
| `GPIO_ECHO` | `24` | HC-SR04 ECHO ピン (BCM) |
| `JUMP_THRESHOLD_CM` | `30` | ジャンプ判定距離 (cm) |
| `SENSOR_SAMPLES` | `3` | 中央値フィルタサンプル数 |
| `MAX_DELTA_CM` | `50` | 外れ値検出しきい値 (cm) |

**raspi_b 環境変数:**

| 変数 | デフォルト | 説明 |
|------|-----------|------|
| `CANWEEB_API` | `http://localhost:8080` | CanWeeb API URL |
| `GPIO_ENC_A` | `17` | エンコーダ A 相ピン (BCM) |
| `GPIO_ENC_B` | `18` | エンコーダ B 相ピン (BCM) |
| `DEBOUNCE_US` | `500` | デバウンス時間 (µs) |
| `MIN_PULSE_US` | `200` | 最小パルス幅 (µs) |
| `COUNT_THRESHOLD` | `2` | 移動判定カウント閾値 |

---

## CanWeeb トピック仕様

| Topic | 方向 | Traffic Class | Payload (JSON) |
|-------|------|---------------|----------------|
| `marrio/input/jump` | RasPi-A → PC | **control** | `{"event":"jump","distance_cm":24.5,"source":"raspi-a-gpio"}` |
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

### GPIO が見つからない
```bash
# GPIO チップの確認
ls -la /dev/gpiochip*
# ユーザーが gpio グループに属しているか確認
groups $USER

# gpio グループに追加 (要再ログイン)
sudo usermod -aG gpio $USER
```

### HC-SR04 が測定できない
- TRIG・ECHO ピンの配線を確認
- ECHO ピンに抵抗分圧またはレベルシフタが接続されているか確認
- GPIO ピン番号が環境変数と一致しているか確認

### ロータリーエンコーダが反応しない
- A 相・B 相ピンの配線を確認
- デバウンス時間を調整: `DEBOUNCE_US=1000` (大きくするとノイズに強くなる)
- 最小パルス幅を調整: `MIN_PULSE_US=100` (小さくすると応答が速くなる)
