use canweeb_cmdlib::prelude::*;
use canweeb_cmdlib::{
    bt_axis, bt_connect_controller, define_child_program, digital_write, emergency_stop,
    init_runtime, motor_enable, motor_set_speed, pin_mode, publish, send_child_program,
    servo_attach, servo_write, uart_open, uart_send,
};

fn define_parent_child_programs() -> Result<(), CmdError> {
    define_child_program!("child_action", |program| {
        program
            .pin_mode("gpio23", "output")
            .digital_write("gpio23", "high")
            .motor_set_speed("left_drive", 0.20)
            .servo_write("camera_tilt", 45.0)
            .uart_send("uart0", "RUN");
    })?;
    Ok(())
}

fn main() -> Result<(), CmdError> {
    use_sim_backend()?;

    init_runtime!()?;
    pin_mode!("gpio17", PinMode::Output)?;
    digital_write!("gpio17", Level::High)?;

    uart_open!(port = "uart0", baud = 115200)?;
    uart_send!("uart0", "PING\n")?;

    bt_connect_controller!(id = "gamepad-1")?;
    let axis = bt_axis!("lx")?;
    println!("controller axis: {}", axis.data);

    motor_enable!(motor = "left_drive")?;
    motor_set_speed!("left_drive", 0.35)?;

    servo_attach!(servo = "camera_tilt", pin = "gpio12")?;
    servo_write!("camera_tilt", 90.0)?;

    publish!(topic = "robot/state", payload = "ready")?;

    define_parent_child_programs()?;
    let report = send_child_program!("child-01", "child_action")?;
    println!(
        "child program executed: {}/{}",
        report.executed_steps, report.total_steps
    );

    emergency_stop!()?;

    Ok(())
}
