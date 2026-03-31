use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::{
    define_child_program, digital_read, digital_write, emergency_stop, init_runtime,
    motor_enable, motor_set_speed, pin_mode, send_child_program, status, uart_open, uart_read,
    uart_send,
};
use std::sync::{Mutex, OnceLock};

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("test lock poisoned")
}

#[test]
fn basic_gpio_flow_works() {
    let _guard = test_guard();
    use_sim_backend().expect("failed to switch backend");

    init_runtime!().expect("init failed");
    pin_mode!("gpio5", PinMode::Output).expect("pin mode failed");
    digital_write!("gpio5", Level::High).expect("write failed");
    let read = digital_read!("gpio5").expect("read failed");
    assert_eq!(read.data["is_high"].as_bool(), Some(true));
}

#[test]
fn uart_and_motor_commands_work() {
    let _guard = test_guard();
    use_sim_backend().expect("failed to switch backend");

    uart_open!(port = "uart0", baud = 115200).expect("uart open failed");
    uart_send!("uart0", "HELLO").expect("uart send failed");
    let rx = uart_read!("uart0").expect("uart read failed");
    assert!(rx.success);

    motor_enable!(motor = "left").expect("motor enable failed");
    motor_set_speed!("left", 0.5).expect("motor speed failed");
    let st = status!().expect("status failed");
    assert!(st.success);
}

#[test]
fn safety_stop_blocks_actuator_write() {
    let _guard = test_guard();
    use_sim_backend().expect("failed to switch backend");

    pin_mode!("gpio13", PinMode::Output).expect("pin mode failed");
    emergency_stop!().expect("stop failed");
    let result = digital_write!("gpio13", Level::High);
    assert!(result.is_err());
}

fn parent_define_child_boot_program() -> Result<(), CmdError> {
    define_child_program!("child_boot", |program| {
        program
            .pin_mode("gpio22", "output")
            .digital_write("gpio22", "high")
            .motor_set_speed("left", 0.2)
            .uart_send("uart0", "BOOT");
    })?;
    Ok(())
}

#[test]
fn parent_can_define_and_send_program_to_child() {
    let _guard = test_guard();
    use_sim_backend().expect("failed to switch backend");

    uart_open!(port = "uart0", baud = 115200).expect("uart open failed");
    parent_define_child_boot_program().expect("program definition failed");

    let report = send_child_program!("child-01", "child_boot").expect("program send failed");
    assert_eq!(report.executed_steps, 4);

    let read = digital_read!("gpio22").expect("gpio read failed after remote run");
    assert_eq!(read.data["is_high"].as_bool(), Some(true));
}
