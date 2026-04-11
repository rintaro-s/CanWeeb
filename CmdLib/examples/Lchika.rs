use canweeb_cmdlib::prelude::*;

fn main() -> Result<(), CmdError> {
    // 実機バックエンドを使用（ラズパイのGPIOを直接制御）
    use_real_backend()?;

    // GPIO17を出力に設定
    pinMode("17", PinMode::Output)?;
    
    // LED点滅
    for _ in 0..5 {
        digitalWrite("17", true)?;
        delay(500);
        digitalWrite("17", false)?;
        delay(500);
    }

    Ok(())
}
