# CANweeb

Rust 製のロボティクス向け多経路メッシュ通信デーモン。マイコン・Raspberry Pi・Ubuntu Server 間をつなぐ通信基盤として設計されています。

## 設計方針

- **トランスポート非依存** — USB Ethernet・Wi-Fi Direct・Wi-Fi AP・有線 LAN、どれでも TCP が通れば動く
- **QoS 分離** — `control` / `telemetry` / `stream` を完全に別経路・別キューで扱う
- **センサデータをディスクに落とさない** — 高頻度センサや画像データは SD / eMMC 寿命を消費しない
- **フォールバック自動化** — USB が切れた場合、未 ACK の `control` は Wi-Fi 側へ自動再送

## トランスポートの選び方

CANweeb は **TCP が通る経路ならなんでも使えます**。USB Ethernet がなくても動きます。

| 経路 | 典型的な用途 | 設定 |
|---|---|---|
| USB gadget Ethernet (`192.168.7.x`) | Pi ↔ PC 間の主経路、最低遅延 | `usb_addr` |
| Wi-Fi Direct / AP | USB なし環境・フォールバック | `wifi_addr` |
| 有線 LAN | 机上テスト・チーム PC 間 | `wifi_addr` でも可 |

**USB がない環境**: `usb_addr` を省略して `wifi_addr` だけ設定すれば Wi-Fi のみで動きます。`usb_listen` / `wifi_listen` も片方だけで構いません。

## Traffic Class

| クラス | ACK | 永続化 | 用途例 |
|---|---|---|---|
| `control` | あり | ディスク | 非常停止・モード切替・GPIO 指令・状態遷移 |
| `telemetry` | なし | メモリ (topic 最新値) | IMU・オドメトリ・バッテリ・推定姿勢 |
| `stream` | なし | メモリ (ring buffer) | カメラ・RGB-D・LiDAR・大容量バイナリ |

> **重要**: 100 Hz 超のセンサや画像を `control` に載せないでください。SD/eMMC 摩耗・再送コスト・制御遅延の原因になります。

## 実装済み機能

- USB / Wi-Fi の 2 系統 TCP リスナー・コネクタ（片方のみでも動作）
- USB 優先の送信経路選択（USB 接続中は常に USB を優先）
- 接続ごとの `control_tx` / `bulk_tx` 二重キュー（大容量 stream で制御系を塞がない）
- `control` の ACK + hop-by-hop 再送（複数経路で自動フェイルオーバー）
- `telemetry` の topic ごと最新値キャッシュ（メモリのみ、最大 4096 topic）
- `stream` の chunked 転送 + 受信側自動組み立て（パケロス時は StreamClose で強制完了）
- stream ring buffer（最新 8 件をメモリ保持）
- topic / stream の WebSocket リアルタイム push
- メッセージ重複排除（`message_id` ベース）
- `ttl` / `hops` によるループ防止
- `broadcast` / `node:X` / `nodes:A,B,C` の配送ターゲット
- WebUI（送信テスト・Topic/Stream モニタ・Inbox・wpa\_cli）
- フレームサイズ上限（32 MiB）

## ビルドと起動

```bash
# ビルド
cargo build --release

# 起動
./target/release/canweeb --config config/example.toml
```

WebUI: `http://<bind_addr>:8080`

## 設定

`config/example.toml` をコピーしてノードごとに編集してください。

```toml
[node]
node_id = "node-pi"
tags = ["robot", "sensor"]

[storage]
root = "../data"          # control メッセージのみ永続化
retention_seconds = 86400

[web]
bind = "0.0.0.0:8080"

[transport]
usb_listen  = "0.0.0.0:7001"   # USB Ethernet リスナー（省略可）
wifi_listen = "0.0.0.0:7002"   # Wi-Fi リスナー（省略可）
connect_interval_ms   = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms        = 2500
max_hops              = 4

[[peers]]
node_id  = "node-main"
usb_addr  = "192.168.7.1:7001"   # 省略可（USB なし環境では削除）
wifi_addr = "192.168.49.1:7002"  # 省略可
tags = ["strategy"]
```

**Wi-Fi のみ構成（USB なし）**:

```toml
[transport]
wifi_listen = "0.0.0.0:7002"
# usb_listen は書かなければ起動しない

[[peers]]
node_id   = "node-main"
wifi_addr = "192.168.1.100:7002"
```

## HTTP API

| Method | Path | 説明 |
|---|---|---|
| GET | `/api/status` | ノード状態・ピア一覧 |
| GET | `/api/inbox` | control inbox 一覧 |
| GET | `/api/inbox/:id` | inbox 詳細 + payload base64 |
| POST | `/api/messages` | メッセージ送信 |
| GET | `/api/topics` | telemetry 最新値一覧 |
| GET | `/api/topic?name=<topic>` | topic 最新値詳細（`/` を含む topic 名対応） |
| GET | `/api/streams` | 完成 stream 一覧（ring buffer） |
| GET | `/api/streams/:stream_id` | stream payload base64 取得 |
| POST | `/api/wifi-direct/run` | wpa\_cli コマンド実行 |

## WebSocket API

| Path | 説明 |
|---|---|
| `ws://.../ws/topics` | telemetry topic 更新をリアルタイム push |
| `ws://.../ws/streams` | stream 組み立て完了をリアルタイム push（payload 含む） |

## POST /api/messages リクエスト例

```json
{
  "target": "broadcast",
  "traffic_class": "telemetry",
  "topic": "imu/accel",
  "content_type": "application/json",
  "text": "{\"x\":0.1,\"y\":0.0,\"z\":9.8}"
}
```

```json
{
  "target": "node:node-main",
  "traffic_class": "control",
  "subject": "emergency_stop",
  "content_type": "application/octet-stream",
  "payload_base64": "AQ=="
}
```

## 推奨ネットワーク構成

### Raspberry Pi (USB gadget Ethernet 使用)

```bash
# /boot/firmware/config.txt
dtoverlay=dwc2

# /etc/modules
dwc2
g_ether

# usb0 に固定 IP を振る (NetworkManager / systemd-networkd)
# 例: 192.168.7.2/24
```

### Ubuntu Server 側

```bash
# USB Ethernet が usb0 に現れたら固定 IP を振る
# 例: 192.168.7.1/24
```

### Wi-Fi Direct (wpa_cli) 例

WebUI の "Wi-Fi Direct / wpa_cli" パネルから直接操作できます。

```
interface: wlan0
args: p2p_find
```

## ディレクトリ構成

| パス | 内容 |
|---|---|
| `src/main.rs` | エントリポイント |
| `src/config.rs` | 設定構造体 |
| `src/protocol.rs` | フレーム定義・TrafficClass・DeliveryTarget |
| `src/storage.rs` | 永続キュー・inbox・topic cache・stream ring buffer |
| `src/mesh.rs` | 接続管理・再送・ACK・topic pub/sub・stream 組み立て |
| `src/web.rs` | WebUI・HTTP API・WebSocket |
| `config/example.toml` | サンプル設定 |

## 既知の制約

- ルーティングはフラッディングです（経路学習は未実装）
- 時刻同期は担保しません（PTP/NTP は別途必要）
- 認証・暗号化は未実装です
- Wi-Fi Direct の自動ペアリングは `wpa_cli` 手動操作が必要です
- systemd unit は同梱していません（`ExecStart` に本バイナリを指定して自分で作成してください）

## 用途の分担イメージ

```
Ubuntu Server (strategy / AI)
    ↕ USB Ethernet (主) / Wi-Fi (退避) via CANweeb
Raspberry Pi (sensor / GPIO / actuator)
    ↕ Wi-Fi / その他 TCP
マイコン / 外部デバイス
```
