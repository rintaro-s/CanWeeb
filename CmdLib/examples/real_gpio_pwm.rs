use canweeb_cmdlib::arduino::*;
use canweeb_cmdlib::{use_real_backend, CmdError, PinMode};

fn main() -> Result<(), CmdError> {
    println!("=== Raspberry Pi GPIO/PWM Test ===");
    
    use_real_backend()?;
    println!("✓ Real backend initialized");

    println!("\n--- GPIO Digital Output Test ---");
    pinMode("17", PinMode::Output)?;
    println!("✓ GPIO17 configured as output");

    for i in 0..5 {
        println!("  Iteration {}: Setting GPIO17 HIGH", i + 1);
        digitalWrite("17", true)?;
        delay(500);
        
        println!("  Iteration {}: Setting GPIO17 LOW", i + 1);
        digitalWrite("17", false)?;
        delay(500);
    }

    println!("\n--- GPIO Digital Input Test ---");
    pinMode("27", PinMode::Input)?;
    println!("✓ GPIO27 configured as input");
    
    for _ in 0..3 {
        let value = digitalRead("27")?;
        println!("  GPIO27 state: {}", if value { "HIGH" } else { "LOW" });
        delay(500);
    }

    println!("\n--- PWM (analogWrite) Test ---");
    println!("Testing PWM on GPIO18 (hardware PWM capable)");
    
    let pwm_values = [0, 64, 128, 192, 255];
    for &value in &pwm_values {
        println!("  Setting PWM duty to {}/255", value);
        analogWrite(18, value)?;
        delay(1000);
    }
    
    println!("  Turning off PWM");
    analogWrite(18, 0)?;

    println!("\n--- PWM Resolution Test ---");
    println!("Setting PWM resolution to 10 bits (0-1023)");
    analogWriteResolution(10)?;
    
    let pwm_values_10bit = [0, 256, 512, 768, 1023];
    for &value in &pwm_values_10bit {
        println!("  Setting PWM duty to {}/1023", value);
        analogWrite(18, value)?;
        delay(1000);
    }
    
    analogWrite(18, 0)?;
    println!("  PWM test complete");

    println!("\n--- Timing Functions Test ---");
    let start = millis();
    println!("  Waiting 2 seconds...");
    delay(2000);
    let elapsed = millis() - start;
    println!("  Elapsed time: {} ms", elapsed);

    println!("\n--- Math Functions Test ---");
    println!("  map(50, 0, 100, 0, 255) = {}", map(50, 0, 100, 0, 255));
    println!("  constrain(150, 0, 100) = {}", constrain(150, 0, 100));
    println!("  sq(5) = {}", sq(5));
    println!("  sqrt(16.0) = {}", sqrt(16.0));

    println!("\n=== All tests completed successfully ===");
    Ok(())
}
