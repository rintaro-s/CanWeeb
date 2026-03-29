# CANweeb

CANweeb は、Raspberry Pi と Ubuntu Server 間で使うことを想定した Rust 製の多経路メッシュ通信デーモンです。

目的は次の 3 点です。

- USB を最優先にして高速に通信する
- USB 切断時に Wi-Fi Direct 系の経路へフォールバックする
- WebUI から状態確認、送信テスト、`wpa_cli` 実行を行う

この実装では、各ノードが同一のオーバーレイプロトコルを動かし、USB と Wi-Fi を別トランスポートとして扱います。全トラフィックを一律にディスクへ落とすのではなく、用途に応じて `control`、`telemetry`、`stream` を分離します。

## 実装済みの中核機能

- USB と Wi-Fi の 2 系統 TCP リスナー
- USB 優先の送信経路選択
- 再接続ループ
- 制御系だけを対象にしたメッセージ永続キュー
- 受信済みメッセージの重複排除
- `control` のみ ACK による hop-by-hop の再送制御
- `control` / `telemetry` / `stream` のトラフィッククラス分離
- 接続ごとの優先度分離送信キュー
- `broadcast` と複数宛先 `nodes:a,b,c` のサポート
- WebUI
- `wpa_cli` ラッパー API

## アーキテクチャ概要

CANweeb は、物理層を直接制御するのではなく、Linux 上で使えるストリーム通信路を統一的に扱います。

- USB 側
  - Raspberry Pi Zero 2 W / 4 / CM 系では USB gadget Ethernet を推奨します
  - 例: `192.168.7.x` の point-to-point IP を振る
- Wi-Fi 側
  - Wi-Fi Direct または同等の専用無線リンクを用意します
  - CANweeb 自体は `wpa_cli` で補助できます
- アプリ側
  - 同じフレーム形式で USB/Wi-Fi を両方扱います
  - ピアごとに `USB > Wi-Fi` の優先度で送信します
  - 接続ごとに `control` と `bulk` を別キューに分離し、重いデータで制御系を塞ぎにくくしています
  - 片方が落ちても未 ACK の `control` メッセージはキューに残るため、次に利用可能な経路へ再送されます

## Traffic Class

CANweeb を実運用で使う場合、最重要なのは「全部を同じ扱いにしない」ことです。

- `control`
  - 非常停止、モード切替、GPIO 制御、状態遷移、重要イベント
  - ACK あり
  - ディスク永続化あり
  - 再送あり
- `telemetry`
  - IMU、姿勢推定、状態配信、軽量センサ更新
  - ACK なし
  - ディスク永続化なし
  - 低遅延優先
- `stream`
  - 画像、RGB-D、LiDAR、点群、連続バイナリフレーム
  - ACK なし
  - ディスク永続化なし
  - 最下位優先の best-effort

**重要**: 高頻度センサや画像を `control` に載せる設計は避けてください。SD / eMMC の寿命、遅延、再送コストの面で不適切です。

## ルーティング方式

この実装は、最初の段階としてフラッディング寄りのメッシュ方式を採用しています。

- 送信元ノードがメッセージを作成
- `control` は受信ノードがディスクに保存してから ACK を返す
- `telemetry` / `stream` はメモリのみで扱う
- ノードは未 ACK の近隣ピアへ `control` を再送する
- 同一 `message_id` は重複排除される
- `ttl` / `hops` により無限ループを防ぐ

これにより、A-B、A-C の外部セッションが存在していても、B-C、C-C 側は CANweeb のオーバーレイとして独立にメッシュ接続できます。

## ディレクトリ構成

- `src/main.rs`
  - エントリポイント
- `src/config.rs`
  - 設定ロード
- `src/protocol.rs`
  - フレーム形式と配送ターゲット
- `src/storage.rs`
  - 永続キュー / inbox / seen 管理
- `src/mesh.rs`
  - 接続管理、再送、ACK、転送
- `src/web.rs`
  - WebUI と HTTP API
- `config/example.toml`
  - サンプル設定

## ビルド

```bash
cargo build
```

## 起動

```bash
cargo run -- --config config/example.toml
```

WebUI はデフォルトで `http://0.0.0.0:8080` に立ちます。

## 設定ファイル

`config/example.toml` をコピーしてノードごとに編集してください。

主な項目:

- `[node]`
  - 自ノード ID
- `[storage]`
  - 永続化ディレクトリ
- `[web]`
  - WebUI bind address
- `[transport]`
  - USB/Wi-Fi の listen address、再接続間隔、ACK timeout
- `[[peers]]`
  - 既知ピアの USB / Wi-Fi アドレス

## WebUI

WebUI でできること:

- 現在の接続状態とピア一覧表示
- テキスト送信テスト
- バイナリ送信テスト
- `traffic_class` を指定した送信テスト
- inbox の確認
- `wpa_cli` コマンド実行

送信ターゲット記法:

- `broadcast`
- `node:node-b`
- `nodes:node-b,node-c1,node-c2`

## HTTP API

- `GET /api/status`
- `GET /api/inbox`
- `GET /api/inbox/:message_id`
- `POST /api/messages`
- `POST /api/wifi-direct/run`

`POST /api/messages` では `traffic_class` に `control` / `telemetry` / `stream` を指定できます。

## Raspberry Pi / Ubuntu 推奨構成

詳しくは `docs/SETUP_RPI_UBUNTU.md` を見てください。

要点だけ先に書くと、実運用では次の構成を推奨します。

- Raspberry Pi 側
  - USB gadget Ethernet を有効化
  - Wi-Fi Direct 用の `wlan0` を専用利用
- Ubuntu 側
  - USB ネットワークを固定 IP 化
  - Wi-Fi Direct の制御を `wpa_cli` または NetworkManager 経由で整備
- CANweeb 側
  - USB listen と Wi-Fi listen の両方を有効化
  - 同じピアに対して USB / Wi-Fi の両アドレスを登録

## 現時点の制約

この段階の実装には、正直に言うと以下の制約があります。

- Wi-Fi Direct の自動ペアリング全自動化は未完成です
- ルーティングは最短経路探索ではなく、堅牢性寄りのフラッディングです
- 認証や暗号化はまだ未実装です
- 巨大フレームの chunked transfer はまだ未実装です
- `stream` は best-effort であり、厳密なフレーム再構成や時刻同期までは担保しません
- LiDAR / RGB-D を高レートで本格運用するには、将来的に専用 chunking と受信 API の分離が必要です
- 実機向けには実ネットワークでのレイテンシ計測、systemd 化、watchdog 設計が必要です

## 次にやると良い拡張

- ルート学習とコストベース転送
- エンドツーエンド配達確認
- `stream` 向け chunked transfer と再構成
- 受信側 API の subscription / ring buffer 化
- Prometheus メトリクス
- 認証と暗号化
- systemd unit と自動起動

## 用途との対応

ロボコン文脈では次の分担を意識しています。

- Ubuntu Server
  - 戦略、画像認識、重い推論
- Raspberry Pi
  - GPIO、センサ、低レイヤ制御
- CANweeb
  - 戦略ノードと制御ノードの間を、USB 優先・Wi-Fi 退避でつなぐ通信基盤
