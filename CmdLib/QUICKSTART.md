# CmdLib クイックスタート

## Raspberry Piで今すぐ使う

### 1. 権限設定（初回のみ）

```bash
# ユーザーをgpioグループに追加
sudo usermod -a -G gpio $USER

# 再ログイン（または以下を実行）
newgrp gpio

# PWMアクセス権設定
sudo chmod 666 /sys/class/pwm/pwmchip0/export
sudo chmod 666 /sys/class/pwm/pwmchip0/unexport
```

### 2. LED点滅（GPIO17）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    use_real_backend()?;
    
    pinMode("17", PinMode::Output)?;
    
    loop {
        digitalWrite("17", true)?;
        delay(1000);
        digitalWrite("17", false)?;
        delay(1000);
    }
}
```

### 3. PWM調光（GPIO18）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    use_real_backend()?;
    
    // フェードイン・フェードアウト
    loop {
        // 0 → 255（明るく）
        for brightness in 0..=255 {
            analogWrite(18, brightness)?;
            delay(10);
        }
        
        // 255 → 0（暗く）
        for brightness in (0..=255).rev() {
            analogWrite(18, brightness)?;
            delay(10);
        }
    }
}
```

### 4. ボタン入力（GPIO27）

```rust
use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::arduino::*;

fn main() -> Result<(), CmdError> {
    use_real_backend()?;
    
    pinMode("27", PinMode::Input)?;
    pinMode("17", PinMode::Output)?;
    
    loop {
        let button_pressed = digitalRead("27")?;
        digitalWrite("17", button_pressed)?;
        delay(50);
    }
}
```

## 配線例

### LED（GPIO17）
```
GPIO17 (Pin 11) ──[330Ω]──(+LED-)──GND
```

### PWM LED（GPIO18）
```
GPIO18 (Pin 12) ──[330Ω]──(+LED-)──GND
```

### ボタン（GPIO27）
```
GPIO27 (Pin 13) ──┬──[10kΩ]──GND
                  │
                  └──[Button]──3.3V
```

## PWM対応ピン

| GPIO | 物理ピン | 用途例 |
|------|---------|--------|
| 12   | 32      | モーター制御 |
| 13   | 33      | サーボ制御 |
| 18   | 12      | **LED調光（推奨）** |
| 19   | 35      | スピーカー |

## よくある質問

**Q: analogWrite()が動かない**
- GPIO 12, 13, 18, 19以外のピンを使っていませんか？
- PWMの権限設定は完了していますか？

**Q: Permission deniedエラー**
```bash
sudo usermod -a -G gpio $USER
newgrp gpio
```

**Q: 10bit PWMを使いたい**
```rust
analogWriteResolution(10)?;  // 0-1023
analogWrite(18, 512)?;  // 50%
```

**Q: ピン番号の指定方法は？**
- BCM番号（GPIO番号）で指定: `"17"` または `"GPIO17"`
- 物理ピン番号ではありません
