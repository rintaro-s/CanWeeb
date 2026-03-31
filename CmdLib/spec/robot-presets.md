# ロボット実装ガイド

この文書には実装済み API の使い分けだけを記載します。  
未実装コマンドの提案は含みません。

## 実装済みで組める最小構成

- GPIO: `pinMode`, `digitalRead`, `digitalWrite`
- モータ: `motor_*` マクロ群
- サーボ: `servo_*` マクロ群
- 通信: `Serial`, `SPI`, `Wire`, `WiFi*`
- 安全停止: `emergency_stop!`, `clear_stop!`

## 詳細

- API 一覧: [standalone.md](standalone.md)
