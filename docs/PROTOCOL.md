# CANweeb Protocol

## ワイヤフォーマット

```
┌────────────────┬──────────────────────────────┐
│ length: u32 BE │ bincode-serialized Frame body │
└────────────────┴──────────────────────────────┘
```

- 最大フレームサイズ: **32 MiB**
- ゼロ長フレームは不正とみなして切断する

## Frame 種類

| Frame | キュー | 説明 |
|---|---|---|
| `Hello` | control | 接続開始時の双方向ハンドシェイク |
| `Data(Envelope)` | control or bulk | 実データ配送 |
| `Ack` | control | Control メッセージの hop-by-hop 確認 |
| `Ping` | control | heartbeat 送信 |
| `Pong` | control | heartbeat 応答 |
| `Subscribe` | control | topic 購読登録 |
| `Unsubscribe` | control | topic 購読解除 |
| `StreamOpen` | bulk | chunked stream 開始メタデータ |
| `StreamChunk` | bulk | chunk 本体（最大 chunk_size バイト） |
| `StreamClose` | bulk | stream 終了マーカー（強制組み立て trigger） |

---

## Hello

接続確立直後に双方が送信します。

```
HelloFrame {
    node_id:      String,
    transport:    TransportKind (Usb | Wifi),
    capabilities: Vec<String>,
    timestamp_ms: u64,
}
```

`TransportKind::Wifi` は wire format 上の名前で、実装上は **generic network transport** を意味します。  
通常の Wi-Fi LAN / 有線 LAN / 直結 Ethernet / Wi-Fi AP 配下の TCP 経路でも同じ値を使います。

`capabilities` の現在値: `["reliable-queue", "webui", "mesh-flood"]`

---

## Data (Envelope)

```
Envelope {
    message_id:   Uuid,
    source_node:  String,
    target:       DeliveryTarget,
    traffic_class: TrafficClass,
    topic:        String,      // telemetry/stream で必須、control では空でも可
    subject:      String,
    content_type: String,
    created_at_ms: u64,
    ttl:          u8,          // デフォルト max_hops (設定値)
    hops:         u8,          // 転送時に +1
    payload:      Vec<u8>,
}
```

### TrafficClass

| クラス | ACK | ディスク永続化 | メモリ保持 | 用途 |
|---|---|---|---|---|
| `Control` | あり | あり (queue + inbox) | あり | 制御指令・緊急停止・状態遷移 |
| `Telemetry` | なし | なし | topic 最新値 1 件 | IMU・オドメトリ・センサ値 |
| `Stream` | なし | なし | ring buffer 最新 8 件 | カメラ・LiDAR・大容量バイナリ |

### DeliveryTarget

```
Broadcast
Node(String)
Nodes(Vec<String>)
```

文字列表現: `broadcast` / `node:node-b` / `nodes:node-b,node-c`

---

## Ack

```
AckFrame {
    message_id:   Uuid,
    from_node:    String,
    timestamp_ms: u64,
}
```

`Control` の Data を受信・保存したノードが送信元へ返す。

---

## Ping / Pong

```
PingFrame { timestamp_ms: u64 }
```

heartbeat_interval_ms ごとに送信。応答がない場合は接続タイムアウトとして切断する。

---

## LAN Discovery

同一 LAN 内の peer 自動発見は、Frame とは別に UDP broadcast で行います。

```json
{
  "version": 1,
  "node_id": "node-pi",
  "tags": ["robot", "sensor"],
  "network_port": 7002,
  "web_port": 8080,
  "timestamp_ms": 1710000000000
}
```

- 送信先: `discovery.announce_addr` (デフォルト `255.255.255.255:7060`)
- 受信 bind: `discovery.bind` (デフォルト `0.0.0.0:7060`)
- 受信側は **UDP 送信元 IP + network_port** から接続先 `network_addr` を生成する
- `peer_ttl_ms` を超えて更新されない peer は stale として扱う

---

## Subscribe / Unsubscribe

```
SubscribeFrame   { topics: Vec<String> }
UnsubscribeFrame { topics: Vec<String> }
```

peer が受信したい telemetry topic を宣言するために使う。  
現在の実装では購読フィルタリングは行わず、送信元は常に broadcast する。  
将来的な経路最適化のための予約フィールドとして機能する。

---

## StreamOpen / StreamChunk / StreamClose

### StreamOpen

```
StreamOpenFrame {
    stream_id:    Uuid,
    source_node:  String,
    topic:        String,
    content_type: String,
    total_chunks: u32,
    total_bytes:  u64,
    timestamp_ms: u64,
}
```

### StreamChunk

```
StreamChunkFrame {
    stream_id:   Uuid,
    chunk_index: u32,
    data:        Vec<u8>,  // 推奨 chunk_size: 60 KiB
}
```

### StreamClose

```
StreamCloseFrame {
    stream_id:   Uuid,
    timestamp_ms: u64,
}
```

`StreamClose` を受け取ると、chunk が全て揃っていない場合でも **強制的に組み立てる**（パケロス耐性）。欠けた chunk は空データで補完する。

---

## 接続シーケンス

```
Initiator                 Acceptor
    │── TCP connect ──────────►│
    │◄─ TCP accept ────────────│
    │── Hello ────────────────►│
    │◄── Hello ───────────────│
    │                         │
    │  (heartbeat loop)       │
    │── Ping ────────────────►│
    │◄── Pong ────────────────│
    │                         │
    │  (data exchange)        │
    │── Data(Control) ───────►│
    │◄── Ack ─────────────────│
    │── Data(Telemetry) ─────►│  (no ack)
    │── StreamOpen ──────────►│
    │── StreamChunk×N ───────►│
    │── StreamClose ─────────►│  (triggers assembly)
```

---

## 送信キュー優先度

接続ごとに 2 つの mpsc チャネルを持ちます。  
writer task は `control_tx` を `biased` で優先します。

```
control_tx (cap: 512)
  └── Hello, Ack, Ping, Pong, Subscribe, Unsubscribe
  └── Data(Control)

bulk_tx (cap: 1024)
  └── Data(Telemetry), Data(Stream)
  └── StreamOpen, StreamChunk, StreamClose
```

大容量 stream が `bulk_tx` を埋めても、`control_tx` のフレームは先に送出されます。

---

## 信頼性モデル

**Control (hop-by-hop)**:
1. 受信ノードがディスクに保存
2. 保存完了後に `Ack` を送信元に返す
3. 送信元は `ack_timeout_ms` ごとに未 ACK ピアへ再送
4. ACK 済みピアへは再送しない

**Telemetry / Stream (best-effort)**:
- ACK なし
- 送信後に queue から削除
- 重複排除は `message_id` で実施

---

## フェイルオーバー

ピアごとに `usb` と `network`（wire 上は `Wifi`）を独立したリンクとして保持します。

送信優先順位: **USB > network**

USB が切断された場合:
- `Control` メッセージはキューに残り、network リンクが存在すれば次の dispatch tick で再送される
- `Telemetry` / `Stream` はベストエフォートのため、切断中に送れなかった分は失われる

---

## メッシュ転送

フラッディング方式（経路学習なし）:

- 受信した ingress peer 以外の全接続ピアを転送候補とする
- `target` が特定ノードの場合はそのノードを優先（ソート）
- `hops` が `ttl` に達したフレームは転送しない
- `message_id` による重複排除で無限ループを防ぐ
