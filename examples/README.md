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
              │ 同じルーター / 同じ LAN ハブ配下
              │ discovery で自動接続
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

Wi-Fi 自動化は **追加機能** です。デフォルトでは無効です。

- `parent.toml`
  - `role = "parent"`
  - デフォルトは `wifi.auto_manage = false`
  - 試すときだけ `wifi.desired_mode = "parent"` と `wifi.auto_manage = true` を使う

- `child.toml`
  - `role = "child"`
  - デフォルトは `wifi.auto_manage = false`
  - 試すときだけ `wifi.desired_mode = "child"` と `wifi.fallback_networks` を使う

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

- **親**: `config/parent.toml` のままで起動
- **子**: `config/child.toml` のままで起動
- **両方**: discovery で peer を自動検知
- **親 UI**: child の生存確認、電源状態、Wi-Fi 状態、RTT、接続品質、transport 優先順を表示

最短で試すなら、**親と子を同じルーターまたは同じ LAN ハブに有線接続**するだけです。

OS 側は **DHCP のままで構いません**。

- **Parent**: ルーターから IP を取得
- **Child**: 同じセグメントの IP を取得

これで discovery により通常は自動接続されます。

### 1. ネットワーク設定

このサンプルのデフォルトは **同じ LAN 内の discovery** です。

- **同じルーター / LAN ハブ配下の有線 LAN**
- **必要なら追加で Wi-Fi 自動化**

`discovery.enabled = true` のままにしておけば、通常は固定 IP を書かなくても peer を見つけられます。

#### Parent (ノートPC)

Parent 側は同じ LAN に参加していれば十分です。

```bash
ip addr show
```

#### Child (Raspberry Pi)

Child 側も同じ LAN に参加していれば十分です。

```bash
ip addr show
```

疎通確認:

```bash
# Parent から child の LAN IP へ
ping <child-lan-ip>
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
   - Transport が `network` なら、LAN または Wi-Fi で接続中です
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
   ping <child-lan-ip>
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
     - `wifi.auto_manage = true` にしたときだけ関係する失敗です
   - `failed to connect wlan0 to CANweeb-Parent`
     - `wifi.auto_manage = true` にしたときだけ関係する失敗です

3. ファイアウォールの確認
   ```bash
   # ポート 7002 / 7060 / 8080 が開いているか確認
   sudo ufw allow 7002/tcp
   sudo ufw allow 7060/udp
   sudo ufw allow 8080/tcp
   ```

4. discovery で見つからない場合
   ```bash
   ip addr show
   ip route
   curl http://localhost:8080/api/status
   ```

   次を確認してください。

   - 親子が本当に同じ L2 / 同じ LAN セグメントにいるか
   - UDP broadcast が遮断されていないか
   - ルーター越しではなく同じセグメントにいるか

5. Parent WebUI が 404 の場合
   - `cargo build --release` を実行
   - いったん `canweeb` を止めて起動し直す
   - その後に `http://<parent-ip>:8080/parent-ui/` を開く

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

2. `/parent-ui/` が 404 の場合は release を更新
   ```bash
   cargo build --release
   ./target/release/canweeb --config config/parent.toml
   ```

3. それでも駄目なら標準 UI は `http://localhost:8080/` で確認できます

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
3. Parent CANweeb → Child CANweeb へメッセージ転送（同じ LAN 内の `network` transport）
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

デフォルトでは **discovery による同一 LAN 内の自動発見** を使います。

固定 IP 運用にしたい場合だけ `network_addr` を書いてください:

```toml
[[peers]]
node_id = "child"
network_addr = "192.168.1.20:7002"
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
