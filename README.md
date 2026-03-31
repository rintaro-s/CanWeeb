# CANweeb

Rust 製のロボティクス向け多経路メッシュ通信デーモン。マイコン・Raspberry Pi・Ubuntu Server 間をつなぐ通信基盤として設計されています。

## 設計方針

- **トランスポート非依存** — 有線 LAN・通常 Wi-Fi LAN・Wi-Fi AP・USB Ethernet、どれでも TCP が通れば動く
- **QoS 分離** — `control` / `telemetry` / `stream` を完全に別経路・別キューで扱う
- **センサデータをディスクに落とさない** — 高頻度センサや画像データは SD / eMMC 寿命を消費しない
- **フォールバック自動化** — 有線が切れた場合、未 ACK の `control` は network 側へ自動再送

## トランスポートの選び方

CANweeb は **TCP が通る経路ならなんでも使えます**。デフォルトでは **親子を同じルーター / 同じ LAN ハブ配下の既存 LAN に参加させる** のがいちばん簡単です。

| 経路 | 典型的な用途 | 設定 |
|---|---|---|
| 有線 LAN | 同じルーター / LAN ハブ配下の通常運用 | `network_addr` |
| 通常 Wi-Fi LAN / AP | 同じ LAN にいる端末同士の接続 | `network_addr` |
| USB gadget Ethernet (`192.168.7.x`) | 追加の別 transport が欲しい場合 | `usb_addr` |

**いちばん簡単な構成**: 親も子も同じルーターまたは同じ LAN ハブに有線接続し、`discovery.enabled = true` のまま起動してください。通常は `network_addr` の固定設定も不要です。

## 自動発見 / 自動ペアリング

同一 LAN 内では UDP broadcast discovery により peer を**自動検知・自動接続**できます。

- `discovery.enabled = true` で有効
- 各ノードが `announce_addr` へ自分の `node_id` / `network_listen port` / `web port` を通知
- 受信側は送信元 IP と通知 port から `network_addr` を自動生成
- `[[peers]]` に `node_id` だけ書いておけば、IP 固定なしでも自動接続可能
- `[[peers]]` 自体を書かなくても、検出された peer は status に現れ、動的接続される

## Wi-Fi 自動化

通常の LAN discovery に加えて、`wifi` セクションで **親の自動 AP 化** と **子の既知 AP 自動接続** を行えます。

これは **追加機能** です。初期セットアップの既定値では使いません。

- まずは **同じ LAN 内で discovery だけで接続確認** してください
- Wi-Fi fallback を使いたい場合だけ `wifi.auto_manage = true` にしてください
- サンプル設定には `CANweeb-Parent` の例を残していますが、デフォルトでは自動起動しません

- 親ノード: `wifi.desired_mode = "parent"`
  - `nmcli device wifi hotspot` で AP を作成
- 子ノード: `wifi.desired_mode = "child"`
  - `wifi.fallback_networks` を priority 順で接続試行
- 親 UI から mode 切替 / AP 起動 / 手動接続 / 切断が可能
- 親 UI から peer の `relationship` と `preferred_transport_order` を変更可能
- child の power / uptime / queue / inbox / RTT / 接続品質も親 UI で監視可能

### Wi-Fi 自動化の実際の挙動

- **child モード**
  - 起動時に `wifi.fallback_networks` のうち、**見えている SSID の中で priority が最も高いもの** へ接続を試みます
  - すでに別の Wi-Fi に繋がっていても、より優先度の高い設定済み SSID が見えていればそちらへ切り替えます
  - 逆に、設定済みの候補が見えていない場合は、見つからない SSID を探すために無意味に切断し続けることはしません

- **parent モード**
  - 起動時に `wifi.interface` を AP 用に使います
  - **同じ無線インターフェースを既存の Wi-Fi クライアント接続に使っていた場合、その接続は切り替わります**
  - つまり、`wlan0` 1 本で親 AP を立てる構成では、元の家庭内 Wi-Fi 接続を維持できないことがあります

### インターネット接続について

- **親子が既存 LAN / 既存 Wi-Fi / LAN ハブに参加する構成**
  - その LAN 自体がインターネットへ出られるなら、**親も子も通常どおりインターネット接続できます**

- **親が `wlan0` で AP を作る構成**
  - その `wlan0` は親子通信用に使われます
  - **同じ `wlan0` 経由の既存インターネット接続は通常維持されません**
  - 親側でインターネットも維持したい場合は、**有線 LAN / USB Ethernet / 別 Wi-Fi インターフェース** を別途使ってください

### 実運用でまず起動させるコツ

- `network_listen = "0.0.0.0:7002"` が起動していれば、LAN / Wi-Fi 経由の通信待受はできています
- 初期セットアップは **同じルーター / 同じ LAN ハブ配下に置くだけ** にしてください
- `wifi.auto_manage = false` のままなら `nmcli` や AP モードに依存しません
- 親向け管理 UI は `http://<bind_addr>:8080/parent-ui/` で外部公開できます
- `/parent-ui/` が 404 の場合は、**古い `target/release/canweeb` が動いている** ことが多いので `cargo build --release` 後に再起動してください

## Traffic Class

| クラス | ACK | 永続化 | 用途例 |
|---|---|---|---|
| `control` | あり | ディスク | 非常停止・モード切替・GPIO 指令・状態遷移 |
| `telemetry` | なし | メモリ (topic 最新値) | IMU・オドメトリ・バッテリ・推定姿勢 |
| `stream` | なし | メモリ (ring buffer) | カメラ・RGB-D・LiDAR・大容量バイナリ |

> **重要**: 100 Hz 超のセンサや画像を `control` に載せないでください。SD/eMMC 摩耗・再送コスト・制御遅延の原因になります。

## 実装済み機能

- USB / network の 2 系統 TCP リスナー・コネクタ（片方のみでも動作）
- 同一 LAN 上の peer 自動検知・自動接続（UDP broadcast discovery）
- transport 優先順の設定と自動フェイルオーバー
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

Parent UI: `http://<bind_addr>:8080/parent-ui/`

## 設定

`config/example.toml` をコピーしてノードごとに編集してください。

```toml
[node]
node_id = "node-pi"
role = "child"
tags = ["robot", "sensor"]

[storage]
root = "../data"          # control メッセージのみ永続化
retention_seconds = 86400

[web]
bind = "0.0.0.0:8080"

[transport]
network_listen = "0.0.0.0:7002"   # LAN / Wi-Fi リスナー（省略可）
connect_interval_ms   = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms        = 2500
max_hops              = 4

[discovery]
enabled = true
bind = "0.0.0.0:7060"
announce_addr = "255.255.255.255:7060"
announce_interval_ms = 1500
peer_ttl_ms = 8000

[wifi]
interface = "wlan0"
auto_manage = false
desired_mode = "child"
hotspot_ssid = "CANweeb-Parent"
hotspot_password = "canweeb1234"
hotspot_connection_name = "CANweeb Hotspot"
status_interval_ms = 2000

[[wifi.fallback_networks]]
ssid = "CANweeb-Parent"
password = "canweeb1234"
priority = 100

[[peers]]
node_id  = "node-main"
role = "parent"
relationship = "parent"
preferred_transport_order = ["network"]
# network_addr = "192.168.1.10:7002"
tags = ["strategy"]
```

**最短構成**:

```toml
# parent.toml / child.toml ともに
[discovery]
enabled = true

[wifi]
auto_manage = false

[[peers]]
# 相手の node_id だけ書く
node_id = "node-main"
```

## HTTP API

| Method | Path | 説明 |
|---|---|---|
| GET | `/api/status` | ノード状態・ピア一覧 |
| GET | `/api/wifi/status` | 自ノードの Wi-Fi 状態 |
| POST | `/api/wifi/apply-mode` | `parent` / `child` / `ap` / `client` の自動適用 |
| POST | `/api/wifi/hotspot/start` | 今すぐ AP を作成 |
| POST | `/api/wifi/connect` | 指定 SSID に接続 |
| POST | `/api/wifi/disconnect` | Wi-Fi 切断 |
| GET | `/api/peer-policies` | peer 関係と transport 優先順 |
| POST | `/api/peer-policies` | peer 関係と transport 優先順を更新 |
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

### Parent / Child を同じルーター / 同じ LAN ハブに接続する場合

```bash
# DHCP のままでよい
# 例:
ip addr show
```

この状態で CANweeb を起動すると、discovery により相互検出できます。

### Wi-Fi fallback を試したい場合

`wifi.auto_manage = true` に変更してから使ってください。初期セットアップの既定値ではありません。

### USB Ethernet を使いたい場合

`usb_listen` / `usb_addr` を追加すれば併用できますが、サンプルの既定値では使いません。

### 補助的な Wi-Fi 操作 (wpa_cli) 例

WebUI の `wpa_cli` パネルは残していますが、通常の LAN / ルーター配下運用では必須ではありません。

親子デモ向けの簡単な操作は `examples/parent-ui/index.html` の管理画面から行う想定です。

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
- discovery は**同一 L2/LAN セグメント内**の自動発見です。サブネットを跨ぐ検出は行いません
- UDP broadcast を遮断するネットワークでは `network_addr` を固定設定してください
- systemd unit は同梱していません（`ExecStart` に本バイナリを指定して自分で作成してください）

## 用途の分担イメージ

```
Ubuntu Server (strategy / AI)
    ↕ same LAN / router via CANweeb
Raspberry Pi (sensor / GPIO / actuator)
    ↕ LAN / Wi-Fi / その他 TCP
マイコン / 外部デバイス
```
