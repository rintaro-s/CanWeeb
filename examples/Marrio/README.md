# Marrio - Face Control Edition

マリオ風ゲームを **顔・超音波センサ・ロータリーエンコーダ** で操作する分散制御デモです。  
CanWeeb (メッシュネットワーク) と CmdLib (GPIO/シリアル制御) を組み合わせています。

---

## システム構成

```
[PC]  ──LAN──  [ラズパイA]  ──USB Serial──  [Arduino]
                                               HC-SR04 (TRIG=12, ECHO=13)
[PC]  ──LAN──  [ラズパイB]  ──UART──  [STM32]
                                         RotaryEncoder (PB0=A相, PB7=B相)
```

| ノード | node_id | 役割 |
|--------|---------|------|
| PC | `marrio-pc` | ゲーム描画・カメラ・CanWeeb 親 |
| ラズパイ A | `marrio-raspi-a` | 超音波センサ橋渡し → jump イベント送信 |
| ラズパイ B | `marrio-raspi-b` | エンコーダ橋渡し → move イベント送信 |
| Arduino | — | HC-SR04 距離をシリアル出力 |
| STM32 | — | ロータリーエンコーダを UART で L/R 出力 |

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
│   └── src/main.rs       # シリアル読み取り → CWB jump 送信
├── raspi_b/
│   ├── Cargo.toml
│   └── src/main.rs       # シリアル読み取り → CWB move 送信
├── arduino/
│   └── marrio_ultrasonic/
│       └── marrio_ultrasonic.ino
└── stm32/
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
cargo run --release -- --config examples/Marrio/config/pc.toml
```

**ラズパイ A:**
```bash
cargo run --release -- --config examples/Marrio/config/raspi_a.toml
```

**ラズパイ B:**
```bash
cargo run --release -- --config examples/Marrio/config/raspi_b.toml
```

> 同一 LAN 内であれば `discovery` により自動でピア接続されます。

---

### 2. Arduino スケッチ書き込み

`arduino/marrio_ultrasonic/marrio_ultrasonic.ino` を Arduino IDE で書き込みます。

**配線:**

| HC-SR04 | Arduino |
|---------|---------|
| TRIG | ピン 12 |
| ECHO | ピン 13 |
| VCC | 5V |
| GND | GND |

---

### 3. STM32 組み込み

`stm32/marrio_encoder/` の 2 ファイルを STM32CubeIDE プロジェクトに追加します。

**CubeMX 設定:**
- PB0 → `GPIO_Input`, Pull-Up
- PB7 → `GPIO_Input`, Pull-Up
- USART1 → `Asynchronous`, 115200 baud, TX=PA9

**main.c の while(1) に追加:**
```c
#include "marrio_encoder.h"

// while(1) 内:
Marrio_Encoder_Task();
```

**配線:**

| ロータリーエンコーダ | STM32 |
|---------------------|-------|
| A 相 | PB0 (D3) |
| B 相 | PB7 (D4) |
| GND | GND |
| VCC | 3.3V |

---

### 4. ラズパイ A ブリッジ起動

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

### 5. ラズパイ B ブリッジ起動

```bash
cd examples/Marrio/raspi_b
CANWEEB_API=http://localhost:8080 \
SERIAL_PORT=/dev/ttyACM0 \
BAUD_RATE=115200 \
cargo run --release
```

---

### 6. PC ゲーム起動

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

---

## CanWeeb トピック仕様

| Topic | 方向 | Traffic Class | Payload (JSON) |
|-------|------|---------------|----------------|
| `marrio/input/jump` | RasPi-A → PC | telemetry | `{"event":"jump","distance_cm":24.5,"source":"raspi-a"}` |
| `marrio/input/move` | RasPi-B → PC | telemetry | `{"event":"move","direction":"left"\|"right","count":-1,"source":"raspi-b"}` |

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

### シリアルポートが見つからない
```bash
ls /dev/ttyACM* /dev/ttyUSB*
# ラズパイ A (Arduino)
SERIAL_PORT=/dev/ttyACM0 cargo run --release
# ラズパイ B (STM32)
SERIAL_PORT=/dev/ttyACM0 cargo run --release
```

### STM32 UART が無音
- PA9 ピンが RasPi-B の `RX` に接続されているか確認 (クロス接続)
- STM32 の GND と RasPi-B の GND が共通になっているか確認
- ボーレートが `115200` で一致しているか確認
