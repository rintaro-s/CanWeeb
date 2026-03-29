# Raspberry Pi / Ubuntu セットアップ

## 1. 物理構成の考え方

一番現実的なのは次の構成です。

- Raspberry Pi と Ubuntu PC を USB で直結
- Raspberry Pi と Ubuntu PC の両方で Wi-Fi Direct を有効化
- CANweeb は USB と Wi-Fi の両方に対して待受 / 発信を行う

USB は高速・低遅延な主経路、Wi-Fi Direct は切断時の退避経路です。

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

## 3. Wi-Fi Direct の考え方

CANweeb 自体は Wi-Fi Direct の association をゼロから実装していません。代わりに、Linux の標準系である `wpa_cli` を呼べるようにしています。

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
wifi_listen = "0.0.0.0:7002"
connect_interval_ms = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms = 2500
max_hops = 8

[[peers]]
node_id = "node-b"
usb_addr = "192.168.7.2:7001"
wifi_addr = "192.168.49.2:7002"
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
wifi_listen = "0.0.0.0:7002"
connect_interval_ms = 1500
heartbeat_interval_ms = 1000
ack_timeout_ms = 2500
max_hops = 8

[[peers]]
node_id = "node-a"
usb_addr = "192.168.7.1:7001"
wifi_addr = "192.168.49.1:7002"
```

## 5. C ノードを増やす場合

`node-c1`, `node-c2` のように `[[peers]]` を追加してください。

この実装では、1:n や B-C1、C1-C2 のような通信も、同じオーバーレイの上で動きます。

ターゲット指定例:

- 単体: `node:node-c1`
- 複数: `nodes:node-b,node-c1,node-c2`
- 全体: `broadcast`

## 6. 永続性

CANweeb はメッセージを `data/queue` に保存してから転送します。

そのため、少なくとも次には耐えます。

- USB の一時切断
- Wi-Fi の一時切断
- プロセス再起動後の未配送再開

ただし、ノード全体の電源断やストレージ障害、Wi-Fi Direct 自体の association 未回復までは自動で解決しません。そこは systemd と OS 側のネットワーク設定で補強してください。

## 7. 実運用で推奨する補強

- systemd サービス化
- journald へのログ集約
- `wpa_supplicant` / `NetworkManager` の固定化
- USB gadget の起動時自動設定
- ハードウェア watchdog
- ping / healthcheck を使った外部監視
