# 通信 API

この文書は実装済み通信 API の入口です。  
詳細一覧は [standalone.md](standalone.md) を参照してください。

## 実装済み

- `Serial`
- `SPI`
- `Wire` (I2C)
- `WiFiClient`, `WiFiServer`, `WiFiUDP`
- `USB`, `Keyboard`, `Mouse` のデバイス列挙
- `WiFiOverview`, `scanWiFiNetworks`

## 権限

- Serial: `/dev/tty*`
- SPI: `/dev/spidev*`
- I2C: `/dev/i2c-*`
- Input: `/dev/input/*`
- Wi-Fi: `nmcli` 実行権限
