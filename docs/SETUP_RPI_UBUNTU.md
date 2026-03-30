# Raspberry Pi / Ubuntu セットアップ

## 1. 物理構成の考え方

一番現実的なのは次の構成です。

- Raspberry Pi と Ubuntu PC を USB で直結
- Raspberry Pi と Ubuntu PC を同じ LAN / Wi-Fi / ハブ / ルーター配下へ接続
- CANweeb は USB と network の両方に対して待受 / 発信を行う

USB は高速・低遅延な主経路、network は切断時の退避経路です。

実運用では、さらにアプリケーション層で次の分離を行ってください。

- GPIO 制御、非常停止、状態遷移は `control`
- IMU、姿勢、状態配信は `telemetry`
- 画像、RGB-D、LiDAR、点群は `stream`

この分離をしないと、重いセンサ通信が制御系を詰まらせます。

## 2. Raspberry Pi 側で USB gadget Ethernet を使う

Pi 側が USB OTG に対応している前提です。

典型的には以下を行います。

- `dtoverlay=dwc2`
- `modules-load=dwc2,g_ether`
- USB に Ethernet Gadget を出す

その後、Pi 側と Ubuntu 側で固定 IP を設定します。

例:

- Ubuntu: `192.168.7.1/24`
- Pi: `192.168.7.2/24`

CANweeb の `usb_addr` / `usb_listen` はこの IP を使います。

## 3. LAN / Wi-Fi と discovery の考え方

CANweeb は通常の TCP/IP ネットワークで動きます。  
同一 LAN であれば `discovery.enabled = true` により UDP broadcast で peer を自動検知できます。

最小構成は次の通りです。

```toml
[transport]
network_listen = "0.0.0.0:7002"

[discovery]
enabled = true
bind = "0.0.0.0:7060"
announce_addr = "255.255.255.255:7060"
announce_interval_ms = 1500
peer_ttl_ms = 8000
```

必要なら補助的に `wpa_cli` で Wi-Fi 接続を操作できます。

代表的な操作例:

```bash
wpa_cli -i wlan0 p2p_find
wpa_cli -i wlan0 p2p_peers
wpa_cli -i wlan0 p2p_connect <PEER_MAC> pbc
wpa_cli -i wlan0 status
```

このプロジェクトでは、WebUI からも `wpa_cli` を実行できます。

## 4. ノード設定の例

### Ubuntu 側

```toml
[node]
node_id = "node-a"
tags = ["strategy"]

[storage]
root = "../data"
retention_seconds = 86400

[web]
bind = "0.0.0.0:8080"

[transport]
usb_listen = "0.0.0.0:7001"
network_listen = "0.0.0.0:7002"
connect_interval_ms = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms = 2500
max_hops = 8

[discovery]
enabled = true
bind = "0.0.0.0:7060"
announce_addr = "255.255.255.255:7060"
announce_interval_ms = 1500
peer_ttl_ms = 8000

[[peers]]
node_id = "node-b"
usb_addr = "192.168.7.2:7001"
# network_addr = "192.168.1.20:7002"  # 固定 IP 運用時のみ
```

### Raspberry Pi 側

```toml
[node]
node_id = "node-b"
tags = ["control"]

[storage]
root = "../data"
retention_seconds = 86400

[web]
bind = "0.0.0.0:8080"

[transport]
usb_listen = "0.0.0.0:7001"
network_listen = "0.0.0.0:7002"
connect_interval_ms = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms = 2500
max_hops = 8

[discovery]
enabled = true
bind = "0.0.0.0:7060"
announce_addr = "255.255.255.255:7060"
announce_interval_ms = 1500
peer_ttl_ms = 8000

[[peers]]
node_id = "node-a"
usb_addr = "192.168.7.1:7001"
# network_addr = "192.168.1.10:7002"  # 固定 IP 運用時のみ
```

## 5. C ノードを増やす場合

`node-c1`, `node-c2` のように `[[peers]]` を追加してください。

この実装では、1:n や B-C1、C1-C2 のような通信も、同じオーバーレイの上で動きます。

ターゲット指定例:

- 単体: `node:node-c1`
- 複数: `nodes:node-b,node-c1,node-c2`
- 全体: `broadcast`

## 6. Traffic Class の使い分け

`traffic_class` は必ず用途で分けてください。

- `control`
  - モータ有効化、停止指令、役割切替、同期イベント
  - ACK あり
  - 永続化あり
- `telemetry`
  - odometry、IMU、バッテリ、姿勢、推定状態
  - ACK なし
  - 永続化なし
- `stream`
  - カメラ、深度、LiDAR、連続バイナリ
  - ACK なし
  - 永続化なし

特に Raspberry Pi 側の SD / eMMC を守るため、**高頻度センサを `control` へ載せない** ようにしてください。

## 7. 永続性

CANweeb は全トラフィックを保存するわけではありません。

- `control`
  - `data/queue` に保存してから転送します
- `telemetry`
  - メモリ上のみで扱います
- `stream`
  - メモリ上のみで扱います

そのため、少なくとも次には耐えます。

- USB の一時切断時の `control` 再送
- network の一時切断時の `control` 再送
- プロセス再起動後の未配送 `control` 再開

`telemetry` / `stream` は low-latency 優先のため、切断中の分まで完全再送する設計ではありません。そこは購読側が「最新値を使う」前提で組むのが現実的です。

ただし、ノード全体の電源断やストレージ障害、LAN を跨ぐ discovery までは自動で解決しません。そこは systemd と OS 側のネットワーク設定で補強してください。

## 8. 実運用で推奨する補強

- systemd サービス化
- journald へのログ集約
- `wpa_supplicant` / `NetworkManager` / DHCP の固定化
- USB gadget の起動時自動設定
- ハードウェア watchdog
- ping / healthcheck を使った外部監視
- 制御系とストリーム系のアプリケーションスレッド分離
- 画像 / LiDAR 向けの chunked transfer 追加
