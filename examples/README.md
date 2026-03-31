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
              │ LAN ケーブル直結 (10.42.0.x)
              │ 切断されたら Wi-Fi fallback
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
  - 普段は LAN ケーブル側の `network_addr = "10.42.0.2:7002"` を優先して child と通信
  - `wifi.desired_mode = "parent"`
  - 起動後に `nmcli device wifi hotspot ...` を使って親 AP を作成し、ケーブルが切れたときの受け口を用意
  - `wlan0` ですでに別の Wi-Fi に繋がっていた場合、その接続は AP 用に切り替わる

- `child.toml`
  - `role = "child"`
  - 普段は LAN ケーブル側の `network_addr = "10.42.0.1:7002"` を優先して親と通信
  - `wifi.desired_mode = "child"`
  - LAN 側が見えなくなったら `wifi.fallback_networks` の優先順で接続を試行
  - デフォルトでは `CANweeb-Parent` へ接続
  - 起動時点で別の Wi-Fi に繋がっていても、より優先度の高い設定済み SSID が見えていればそちらへ切り替える

家庭内 AP に接続させたい場合は `config/child.toml` に追加します。

```toml
[[wifi.fallback_networks]]
ssid = "MyHomeWiFi"
password = "secret"
priority = 200
```

`priority` が大きいほど先に試します。

インターネット接続について:

- **親子が既存 LAN / 既存 Wi-Fi に参加する構成** なら、その LAN がインターネットに出られる限り、親子ともにネット接続できます
- **親が `wlan0` で AP を作る構成** では、その `wlan0` は親子通信用になります
- 親も同時にネットへ出したい場合は、有線 LAN か別のネットワークインターフェースを併用してください

---

## セットアップ

### 最短スタート

このサンプルは **親を起動して WebUI を開き、子を起動するだけ** で試せるようにしています。

- **親**: `config/parent.toml` により LAN ケーブルを主経路、`wlan0` を fallback 用 AP として管理
- **子**: `config/child.toml` により LAN ケーブルを主経路、`CANweeb-Parent` を fallback 先として管理
- **両方**: discovery で peer を自動検知
- **親 UI**: child の生存確認、電源状態、Wi-Fi 状態、RTT、接続品質、transport 優先順を表示

最短で試すなら、**親の Ethernet と子の Ethernet を LAN ケーブルで 1 本つなぐ**だけです。

そのうえで OS 側に次の固定 IP を入れてください。

- **Parent の Ethernet**: `10.42.0.1/24`
- **Child の Ethernet**: `10.42.0.2/24`

これで通常は **LAN ケーブル経由** になり、ケーブルが切れたら **親 AP ↔ child Wi-Fi** へ寄ります。

### 1. ネットワーク設定

このサンプルは **USB Ethernet を使いません**。次の順番で使います。

- **LAN ケーブル直結 / 同一有線 LAN**
- **親が自動 AP、子が自動接続する Wi-Fi fallback**

基本は `discovery.enabled = true` のままにしておけば、固定 IP を持たなくても peer を見つけられます。

#### Parent (ノートPC)

Parent 側は Ethernet に固定 IP を 1 つ入れるだけです。

```bash
ip addr add 10.42.0.1/24 dev eth0
# NetworkManager を使う場合は eth0 を実際の有線 IF 名に置き換える
```

#### Child (Raspberry Pi)

Child 側も Ethernet に固定 IP を 1 つ入れるだけです。

```bash
sudo ip addr add 10.42.0.2/24 dev eth0
sudo ip link set eth0 up
```

疎通確認:

```bash
# Parent から
ping 10.42.0.2
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
xdg-open http://localhost:8080/parent-ui/
# 同じ LAN 上の別 PC / スマホからも http://<parent-ip>:8080/parent-ui/ でアクセス可能
# CANweeb の標準 WebUI は http://localhost:8080/
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
   `http://<parent-ip>:8080/parent-ui/` をブラウザで開きます。

2. **接続状態を確認**  
   - Parent (ノートPC) のステータスが緑色の点で表示されます
   - Child (Raspberry Pi) の接続状態が表示されます
   - Transport が `network` なら、LAN ケーブルまたは Wi-Fi fallback のどちらかで接続中です
   - Quality / RTT / Wi-Fi / Queue/Inbox で child の状態が見られます

3. **LED を制御**  
   - **💡 LED ON** ボタンをクリック → STM32 の LED が点灯
   - **🌙 LED OFF** ボタンをクリック → STM32 の LED が消灯

4. **Wi-Fi / ノード関係を操作**  
   - 親を AP 化する
   - child の接続順序を `network` 優先で変更する
   - role / relationship を親 UI から変更する

5. **コマンドログを確認**  
   ボタンをクリックすると、送信状況と結果がログに表示されます。

---

## トラブルシューティング

### Child が接続されない

1. ネットワークの疎通確認
   ```bash
   # Parent から
   ping 10.42.0.2
   ```

2. CANweeb のログを確認
   ```bash
   # Parent 側
   ./target/release/canweeb --config config/parent.toml
   # "peer registered" が出れば接続成功
   ```

   次のログの意味:

   - `listener started addr=0.0.0.0:7002 transport=Wifi`
     - network listener は起動済みです
   - `web ui started bind_addr=0.0.0.0:8080`
     - WebUI は起動済みです
   - `failed to start hotspot on wlan0`
     - 親 AP 化だけ失敗しています
   - `failed to connect wlan0 to CANweeb-Parent`
     - 子は親 AP を見つけられていないか、親 AP が未起動です

3. ファイアウォールの確認
   ```bash
   # ポート 7002 / 7060 / 8080 が開いているか確認
   sudo ufw allow 7002/tcp
   sudo ufw allow 7060/udp
   sudo ufw allow 8080/tcp
   ```

4. 親 AP が立たない場合
   ```bash
   nmcli device status
   nmcli radio wifi
   nmcli device wifi list ifname wlan0
   nmcli device wifi hotspot ifname wlan0 con-name "CANweeb Hotspot" ssid "CANweeb-Parent" password "canweeb1234"
   ```

   これが失敗する場合は、CANweeb の問題ではなく次のどれかです。

   - `NetworkManager` / `nmcli` が使えない
   - `wlan0` が存在しない
   - Wi-Fi アダプタ / ドライバが AP モード非対応
   - 権限不足
   - 既存の接続管理と競合している

5. LAN ケーブル優先でまず動かす
   - 親 Ethernet に `10.42.0.1/24`
   - 子 Ethernet に `10.42.0.2/24`
   - `ping 10.42.0.2` が通ることを確認
   - その後に `http://<parent-ip>:8080/parent-ui/` で状態確認

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
3. Parent CANweeb → Child CANweeb へメッセージ転送（LAN ケーブル優先、切断時は Wi-Fi fallback）
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

デフォルトでは **LAN ケーブル用に設定した `network_addr` を先に試し**、それが通らないときに **discovery で見つかった Wi-Fi 側アドレス** へフォールバックします。

サンプルでは次の固定 IP を想定しています:

```toml
# parent.toml
network_addr = "10.42.0.2:7002"

# child.toml
network_addr = "10.42.0.1:7002"
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
