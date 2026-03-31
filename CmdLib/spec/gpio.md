# GPIO / Analog / Timing

この文書の内容は実装済み API の要点のみです。  
詳細一覧は [standalone.md](standalone.md) を参照してください。

## 実装済み

- `pinMode`, `digitalRead`, `digitalWrite`
- `analogRead`, `analogWrite`
- `pulseIn`, `pulseInLong`
- `shiftIn`, `shiftOut`
- `tone`, `noTone`
- `millis`, `micros`, `delay`, `delayMicroseconds`

## 権限

- GPIO: `/dev/gpiochip*`
