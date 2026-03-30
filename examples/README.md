# CANweeb 親子制御デモ

親（ノートPC）→ 子（Raspberry Pi）→ STM32 の LED を制御するデモアプリケーション。

## 構成

```
┌─────────────────────────────────────────┐
│ Parent (ノートPC)                        │
│  - CANweeb デーモン (port 8080)          │
│  - 親管理 WebUI (examples/parent-ui)     │
│    → 両デバイス情報表示                    │
│    → 子の STM32 LED を ON/OFF             │
└─────────────┬───────────────────────────┘
              │ USB Ethernet (192.168.7.x)
              │ または LAN / Wi-Fi (auto discovery)
┌─────────────▼───────────────────────────┐
│ Child (Raspberry Pi)                    │
│  - CANweeb デーモン (port 8080)          │
│  - child-serial-daemon                  │
│    → inbox ポーリング                     │
│    → ST-LINK COM ポート制御               │
└─────────────┬───────────────────────────┘
              │ USB Serial (/dev/ttyACM0)
┌─────────────▼───────────────────────────┐
│ STM32 (ST-LINK 経由)                     │
│  - "on"  受信 → LED 点灯                  │
│  - "off" 受信 → LED 消灯                  │
└─────────────────────────────────────────┘
```

---

### 1.5 Wi-Fi 自動化

サンプル設定では次の挙動になります。

- `parent.toml`
  - `role = "parent"`
  - `wifi.desired_mode = "parent"`
  - 起動後に `nmcli device wifi hotspot ...` を使って親 AP を作成

- `child.toml`
  - `role = "child"`
  - `wifi.desired_mode = "child"`
  - `wifi.fallback_networks` の優先順で接続を試行
  - デフォルトでは `CANweeb-Parent` へ接続

家庭内 AP に接続させたい場合は `config/child.toml` に追加します。

```toml
[[wifi.fallback_networks]]
ssid = "MyHomeWiFi"
password = "secret"
priority = 200
```

`priority` が大きいほど先に試します。

---

## セットアップ

### 最短スタート

このサンプルは **親を起動して WebUI を開き、子を起動するだけ** で試せるようにしています。

- **親**: `config/parent.toml` により `wlan0` を自動 AP モードで管理
- **子**: `config/child.toml` により `CANweeb-Parent` へ自動接続
- **両方**: discovery で peer を自動検知
- **親 UI**: child の生存確認、電源状態、Wi-Fi 状態、RTT、接続品質、transport 優先順を表示

### 1. ネットワーク設定

このサンプルは次の 2 パターンで動きます。

- **USB Ethernet + LAN/Wi-Fi fallback**
- **LAN/Wi-Fi only + 自動検知**

#### Parent (ノートPC)

USB Ethernet インターフェースに固定 IP を割り当てます。

```bash
# USB ケーブルでラズパイと接続すると usb0 または enx... が現れる
ip addr add 192.168.7.1/24 dev usb0

# または NetworkManager で設定
nmcli connection modify usb0 ipv4.addresses 192.168.7.1/24
nmcli connection modify usb0 ipv4.method manual
nmcli connection up usb0
```

#### Child (Raspberry Pi)

USB gadget Ethernet を有効化します。

```bash
# /boot/firmware/config.txt に追記
dtoverlay=dwc2

# /etc/modules に追記
dwc2
g_ether

# 再起動後、usb0 に固定 IP を設定
sudo ip addr add 192.168.7.2/24 dev usb0
sudo ip link set usb0 up

# または /etc/network/interfaces に記載
# auto usb0
# iface usb0 inet static
#     address 192.168.7.2
#     netmask 255.255.255.0
```

疎通確認:

```bash
# Parent から
ping 192.168.7.2

# Child から
ping 192.168.7.1
```

---

### 2. 依存関係のインストール

#### Ubuntu / Debian

```bash
sudo apt update
sudo apt install -y pkg-config libudev-dev
```

#### Fedora / RHEL

```bash
sudo dnf install -y pkgconf systemd-devel
```

#### Arch Linux

```bash
sudo pacman -S pkgconf systemd
```

---

### 3. CANweeb ビルド

```bash
cd /home/rinta/CANweeb
cargo build --release
```

---

### 4. child-serial-daemon ビルド

```bash
cd examples/child-serial-daemon
cargo build --release
```

---

## 起動手順

### Parent 側

```bash
# Terminal 1: CANweeb デーモン起動
cd /home/rinta/CANweeb
./target/release/canweeb --config config/parent.toml

# Terminal 2: 親管理 WebUI をブラウザで開く
xdg-open examples/parent-ui/index.html
# または http://localhost:8080 で CANweeb の WebUI も使えます
```

親 UI でできること:

- child の生存確認 / power / uptime / queue / inbox の確認
- Wi-Fi mode / SSID / IP の確認
- 親を AP モードへ切り替え
- 手動 SSID 接続 / 切断
- child の relationship / transport order の変更

### Child 側 (Raspberry Pi)

```bash
# Terminal 1: CANweeb デーモン起動
cd /home/rinta/CANweeb
./target/release/canweeb --config config/child.toml

# Terminal 2: child-serial-daemon 起動
cd examples/child-serial-daemon
SERIAL_PORT=/dev/ttyACM0 BAUD_RATE=9600 ./target/release/child-serial-daemon

# SERIAL_PORT と BAUD_RATE は環境に合わせて変更してください
# ST-LINK の COM ポートは lsusb や dmesg で確認できます:
# dmesg | grep tty
```

---

## 使い方

1. **Parent 管理 WebUI を開く**  
   `examples/parent-ui/index.html` をブラウザで開きます。

2. **接続状態を確認**  
   - Parent (ノートPC) のステータスが緑色の点で表示されます
   - Child (Raspberry Pi) の接続状態が表示されます
   - Transport が "usb" と表示されれば USB Ethernet 経由で接続中です
   - Quality / RTT / Wi-Fi / Queue/Inbox で child の状態が見られます

3. **LED を制御**  
   - **💡 LED ON** ボタンをクリック → STM32 の LED が点灯
   - **🌙 LED OFF** ボタンをクリック → STM32 の LED が消灯

4. **Wi-Fi / ノード関係を操作**  
   - 親を AP 化する
   - child の接続順序を `usb,network` などで変更する
   - role / relationship を親 UI から変更する

5. **コマンドログを確認**  
   ボタンをクリックすると、送信状況と結果がログに表示されます。

---

## トラブルシューティング

### Child が接続されない

1. ネットワークの疎通確認
   ```bash
   # Parent から
   ping 192.168.7.2
   ```

2. CANweeb のログを確認
   ```bash
   # Parent 側
   ./target/release/canweeb --config config/parent.toml
   # "peer registered" が出れば接続成功
   ```

3. ファイアウォールの確認
   ```bash
   # ポート 7001 が開いているか確認
   sudo ufw allow 7001/tcp
   ```

### LED が反応しない

1. シリアルポートの確認
   ```bash
   # Child 側
   ls -l /dev/ttyACM*
   # ST-LINK が /dev/ttyACM0 に現れるか確認

   dmesg | grep tty
   ```

2. シリアル権限の確認
   ```bash
   sudo usermod -a -G dialout $USER
   # ログアウト・ログインして反映
   ```

3. child-serial-daemon のログを確認
   ```bash
   # "LED コマンド: on" や "✅ Serial sent: on" が出るか確認
   ```

4. STM32 のファームウェアが正しいか確認
   - シリアルで "on" を受信したら LED を点灯する実装になっているか
   - シリアルで "off" を受信したら LED を消灯する実装になっているか

### Parent WebUI が読み込めない

1. CANweeb が起動しているか確認
   ```bash
   curl http://localhost:8080/api/status
   ```

2. ブラウザのコンソールでエラーを確認
   - F12 キーでデベロッパーツールを開く
   - CORS エラーが出る場合は、`examples/parent-ui/index.html` を file:// ではなく HTTP サーバー経由で開く
   ```bash
   cd examples/parent-ui
   python3 -m http.server 3000
   # http://localhost:3000 で開く
   ```

---

## カスタマイズ

### シリアルポートの変更

```bash
# /dev/ttyUSB0 を使う場合
SERIAL_PORT=/dev/ttyUSB0 ./target/release/child-serial-daemon
```

### ボーレートの変更

```bash
# 115200 baud を使う場合
BAUD_RATE=115200 ./target/release/child-serial-daemon
```

### コマンドの追加

`examples/child-serial-daemon/src/main.rs` の `match command.as_str()` に新しいコマンドを追加できます:

```rust
match command.as_str() {
    "on" | "off" => { /* ... */ }
    "blink" => {
        send_serial(serial_port, baud_rate, "blink")?;
    }
    _ => { /* ... */ }
}
```

---

## アーキテクチャ詳細

### 通信フロー

1. Parent WebUI で「LED ON」ボタンをクリック
2. Parent CANweeb に `POST /api/messages` で制御メッセージ送信
   ```json
   {
     "target": "node:child",
     "traffic_class": "control",
     "subject": "led_control",
     "text": "on"
   }
   ```
3. Parent CANweeb → Child CANweeb へメッセージ転送（USB Ethernet 優先）
4. Child CANweeb が inbox に保存
5. child-serial-daemon が inbox をポーリング（500ms 間隔）
6. subject が "led_control" のメッセージを検出
7. `/dev/ttyACM0` に "on\n" を送信
8. STM32 が受信して LED 点灯

### TrafficClass の選択

この例では `control` を使用しています。理由：

- LED 制御は **確実に届く必要がある** → ACK あり
- ディスク永続化により、接続断時も再送される
- 高頻度ではない（ユーザー操作ベース）

 もし高頻度でセンサ値を返す場合は `telemetry` を使用します。

---

## LAN / Wi-Fi フォールバックと自動発見

USB が切断された場合、自動的に `network` 経路へフォールバックします。  
同一 LAN 内では discovery により `network_addr` を固定設定しなくても自動接続されます。

固定 IP 運用にしたい場合だけ `network_addr` を指定します:

```toml
[discovery]
enabled = true
announce_addr = "255.255.255.255:7060"

[[peers]]
node_id = "child"
usb_addr = "192.168.7.2:7001"
# network_addr = "192.168.1.20:7002"  # 固定 IP 運用時のみ
```

親 WebUI では child の `discovered` 状態、`advertised_network_addr`、`Child WebUI` URL も表示されます。

---

## systemd 自動起動

### Parent 側

`/etc/systemd/system/canweeb-parent.service`:

```ini
[Unit]
Description=CANweeb Parent Daemon
After=network.target

[Service]
Type=simple
User=rinta
WorkingDirectory=/home/rinta/CANweeb
ExecStart=/home/rinta/CANweeb/target/release/canweeb --config /home/rinta/CANweeb/config/parent.toml
Restart=always

[Install]
WantedBy=multi-user.target
```

### Child 側

`/etc/systemd/system/canweeb-child.service` と `/etc/systemd/system/child-serial-daemon.service` を作成します。

```bash
sudo systemctl enable canweeb-parent
sudo systemctl start canweeb-parent
```
