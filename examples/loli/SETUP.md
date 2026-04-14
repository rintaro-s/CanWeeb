# Loli セットアップガイド

このガイドでは、すべてのコンポーネントを正しく動作させるための手順を説明します。

---

## 前提条件

### PC (親)
- Rust (cargo)
- Python 3.x
- pip3

### Raspberry Pi (子)
- Rust (cargo)
- 3ピンロータリーエンコーダ (CLK, DT, GND)
- sudo 権限（pinctrl と dtoverlay のため）

---

## セットアップ手順

### 1. PC (親) のセットアップ

#### 1.1. Python 依存関係をインストール

```bash
cd examples/loli/visualizer
pip3 install -r requirements.txt
```

**確認:**
```bash
python3 -c "import websocket; print('OK')"
```

#### 1.2. CanWeeb ノードを起動

```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/pc.toml
```

**確認:**
- ブラウザで http://localhost:8080 にアクセス
- Web UI が表示されることを確認

#### 1.3. ビジュアライザを起動

別のターミナルで:
```bash
cd examples/loli/visualizer
CANWEEB_API=http://localhost:8080 python3 loli_visualizer.py
```

**確認:**
- WebSocket 接続成功のメッセージが表示される
- エラーが出ないことを確認

---

### 2. Raspberry Pi (子) のセットアップ

#### 2.1. ハードウェア接続

3ピンロータリーエンコーダを接続:

| エンコーダ | Raspberry Pi |
|-----------|-------------|
| CLK (A相) | GPIO 17 (BCM) |
| DT (B相) | GPIO 18 (BCM) |
| GND | GND |

#### 2.2. CanWeeb ノードを起動

```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/raspi.toml
```

**確認:**
- ピア接続のメッセージが表示される
- `Connected to peer: loli-visualizer` のようなログを確認

#### 2.3. エンコーダ送信スクリプトを起動

別のターミナルで:
```bash
cd examples/loli/encoder_sender
CANWEEB_API=http://localhost:8080 sudo cargo run --release
```

**重要:** `sudo` が必要です（pinctrl と dtoverlay のため）

**確認:**
- DTOverlay がロードされる
- pinctrl でピンが設定される
- エンコーダ監視が開始される

---

## 動作確認

### 1. エンコーダを回す

Raspberry Pi のエンコーダを回してください。

### 2. PC のビジュアライザで確認

PC のターミナルに以下のような表示が出ることを確認:

```
[受信:     12] 位置:     48  方向: → 東 (E)  

============================================================
  位置: 48
  方向: → 東 (E)
  受信総数: 12
  最終更新: 19:45:23.456
============================================================
```

### 3. 8方向の確認

エンコーダを回し続けて、8方向すべてが表示されることを確認:
- N (北) ↑
- NE (北東) ↗
- E (東) →
- SE (南東) ↘
- S (南) ↓
- SW (南西) ↙
- W (西) ←
- NW (北西) ↖

---

## トラブルシューティング

### エラー: `missing field 'root'`

**原因:** CanWeeb 設定ファイルが古い形式

**解決策:**
```bash
# 設定ファイルを確認
cat examples/loli/config/pc.toml

# [storage] セクションに root フィールドがあることを確認
# [storage]
# root = "./data/loli-pc"
```

### エラー: `ModuleNotFoundError: No module named 'websocket'`

**原因:** websocket-client がインストールされていない

**解決策:**
```bash
pip3 install websocket-client
```

### エラー: `pinctrl コマンドの起動に失敗`

**原因:** pinctrl がインストールされていない、または sudo 権限がない

**解決策:**
```bash
# pinctrl をインストール
sudo apt-get install raspi-gpio

# sudo で実行
sudo cargo run --release
```

### エラー: `dtoverlay rotary-encoder のロードに失敗`

**原因:** DTOverlay が有効になっていない

**解決策:**
```bash
# raspi-config で有効化
sudo raspi-config
# → 3 Interface Options → I5 Device Tree Overlays → Enable

# 再起動
sudo reboot
```

### WebSocket 接続エラー

**原因:** CanWeeb が起動していない、またはポートが違う

**解決策:**
```bash
# CanWeeb が起動しているか確認
curl http://localhost:8080/api/status

# ポートを確認
netstat -tuln | grep 8080
```

### エンコーダが反応しない

**原因:** 配線ミス、pinctrl 設定ミス、DTOverlay ロードミス

**解決策:**
```bash
# 配線を確認
# CLK → GPIO17, DT → GPIO18, GND → GND

# pinctrl 設定を確認
pinctrl get 17
pinctrl get 18

# DTOverlay を確認
dtoverlay -l | grep rotary
```

---

## 完全な起動手順（まとめ）

### PC (親)

ターミナル1:
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/pc.toml
```

ターミナル2:
```bash
cd /path/to/CanWeeb/examples/loli/visualizer
pip3 install -r requirements.txt
CANWEEB_API=http://localhost:8080 python3 loli_visualizer.py
```

### Raspberry Pi (子)

ターミナル1:
```bash
cd /path/to/CanWeeb
cargo run --release --bin canweeb -- --config examples/loli/config/raspi.toml
```

ターミナル2:
```bash
cd /path/to/CanWeeb/examples/loli/encoder_sender
CANWEEB_API=http://localhost:8080 sudo cargo run --release
```

---

## 期待される動作

1. Raspberry Pi でエンコーダを回す
2. encoder_sender が DTOverlay + pinctrl + GPIO で読み取る
3. CanWeeb 経由で PC に送信
4. PC の loli_visualizer.py が WebSocket で受信
5. 8方向で現在の向きを表示

すべてのコンポーネントが正しく動作すれば、エンコーダを回すたびに PC の画面に方向が表示されます。
