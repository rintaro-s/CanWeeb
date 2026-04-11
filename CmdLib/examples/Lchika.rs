use canweeb_cmdlib::prelude::*;

fn main() -> Result<(), CmdError> {
    // 実機バックエンドを使用
    use_real_backend()?;

    pinMode("17", PinMode::Output)?;
    
    for _ in 0..5 {
        digitalWrite("17", true)?;
        delay(500);
        digitalWrite("17", false)?;
        delay(500);
    }

    Ok(())
}
