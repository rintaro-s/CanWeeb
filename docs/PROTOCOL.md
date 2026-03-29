# CANweeb Protocol

## フレーム

CANweeb は length-prefixed binary frame を使います。

- 4 byte length
- `bincode` で直列化した `Frame`

## Frame 種類

- `Hello`
- `Data`
- `Ack`
- `Ping`
- `Pong`

## Hello

接続開始時に相互送信します。

内容:

- `node_id`
- `transport` (`usb` / `wifi`)
- `capabilities`
- `timestamp_ms`

## Data

`Envelope` を配送します。

主なフィールド:

- `message_id`
- `source_node`
- `target`
- `subject`
- `content_type`
- `created_at_ms`
- `ttl`
- `hops`
- `payload`

## DeliveryTarget

- `Node(String)`
- `Nodes(Vec<String>)`
- `Broadcast`

## 信頼性モデル

- 受信ノードはまずディスクへ保存する
- 保存後に送信元へ `Ack` を返す
- 未 ACK のピアに対して再送する
- `message_id` により重複排除する

これは end-to-end ではなく hop-by-hop reliability です。

## フェイルオーバー

ピアごとに複数トランスポートを保持できます。

優先順位:

1. USB
2. Wi-Fi

USB が切れた場合でも、キュー上の未 ACK メッセージは残るため、Wi-Fi 側が生きていれば同じ `message_id` で再送できます。

## メッシュ転送

現段階の実装では、経路探索ではなく保守的なメッシュ転送です。

- 受信ノードは ingress peer を記録
- ingress peer 以外の接続先へ転送候補を作る
- 既に ACK 済みのピアへは再送しない
- `ttl` を超えない範囲で `hops` を増やしながら転送する

## 想定する今後の拡張

- chunked transfer
- route advertisement
- path cost estimation
- end-to-end delivery receipts
- compression
- authentication
