#[macro_export]
macro_rules! __cmd_named {
    ($domain:expr, $action:expr $(, $key:expr => $value:expr )* $(,)?) => {{
        let mut map = $crate::serde_json::Map::new();
        $(
            map.insert($key.to_string(), $crate::serde_json::json!($value));
        )*
        $crate::dispatch($domain, $action, $crate::serde_json::Value::Object(map))
    }};
}

#[macro_export]
macro_rules! __cmd_kv {
    ($domain:expr, $action:expr) => {
        $crate::__cmd_named!($domain, $action)
    };
    ($domain:expr, $action:expr, $( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_named!($domain, $action $(, stringify!($key) => $value )*)
    };
}

#[macro_export]
macro_rules! define_child_program {
    ($name:expr, |$builder:ident| $body:block) => {
        $crate::define_child_program($name, |$builder| $body)
    };
}

#[macro_export]
macro_rules! send_child_program {
    ($child_id:expr, $program_name:expr) => {
        $crate::send_child_program_to($child_id, $program_name)
    };
}

#[macro_export]
macro_rules! run_child_program {
    ($program_name:expr) => {
        $crate::run_child_program($program_name)
    };
}

#[macro_export]
macro_rules! status {
    () => {
        $crate::__cmd_named!("runtime", "status")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_named!("runtime", "status" $(, stringify!($key) => $value )*)
    };
}

#[macro_export]
macro_rules! pin_mode {
    ($pin:expr, $mode:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "gpio",
            "pin_mode",
            "pin" => $pin,
            "mode" => $mode
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! digital_write {
    ($pin:expr, $level:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "gpio",
            "digital_write",
            "pin" => $pin,
            "level" => $level
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! digital_read {
    ($pin:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "gpio",
            "digital_read",
            "pin" => $pin
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! digital_toggle {
    ($pin:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "gpio",
            "digital_toggle",
            "pin" => $pin
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! pwm_write {
    ($channel:expr, $duty:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "pwm",
            "pwm_write",
            "channel" => $channel,
            "duty" => $duty
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! servo_write {
    ($servo:expr, $angle:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "servo",
            "servo_write",
            "servo" => $servo,
            "angle" => $angle
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! motor_set_speed {
    ($motor:expr, $speed:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "motor",
            "motor_set_speed",
            "motor" => $motor,
            "speed" => $speed
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! uart_send {
    ($port:expr, $data:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "uart",
            "uart_send",
            "port" => $port,
            "data" => $data
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! uart_read {
    ($port:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "uart",
            "uart_read",
            "port" => $port
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! bt_axis {
    ($axis:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "bt",
            "bt_axis",
            "axis" => $axis
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! bt_button {
    ($button:expr $(, $key:ident = $value:expr )* $(,)?) => {
        $crate::__cmd_named!(
            "bt",
            "bt_button",
            "button" => $button
            $(, stringify!($key) => $value )*
        )
    };
}

#[macro_export]
macro_rules! init_runtime {
    () => {
        $crate::__cmd_kv!("runtime", "init_runtime")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "init_runtime", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! shutdown_runtime {
    () => {
        $crate::__cmd_kv!("runtime", "shutdown_runtime")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "shutdown_runtime", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! reload_profile {
    () => {
        $crate::__cmd_kv!("runtime", "reload_profile")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "reload_profile", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! health_check {
    () => {
        $crate::__cmd_kv!("runtime", "health_check")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "health_check", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! panic_stop {
    () => {
        $crate::__cmd_kv!("runtime", "panic_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "panic_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! emergency_stop {
    () => {
        $crate::__cmd_kv!("runtime", "emergency_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "emergency_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! clear_stop {
    () => {
        $crate::__cmd_kv!("runtime", "clear_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "clear_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! latch_stop {
    () => {
        $crate::__cmd_kv!("runtime", "latch_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "latch_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! manual_override {
    () => {
        $crate::__cmd_kv!("runtime", "manual_override")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "manual_override", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! set_deadman {
    () => {
        $crate::__cmd_kv!("runtime", "set_deadman")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "set_deadman", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! enable_watchdog {
    () => {
        $crate::__cmd_kv!("runtime", "enable_watchdog")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "enable_watchdog", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! disable_watchdog {
    () => {
        $crate::__cmd_kv!("runtime", "disable_watchdog")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "disable_watchdog", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! feed_watchdog {
    () => {
        $crate::__cmd_kv!("runtime", "feed_watchdog")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "feed_watchdog", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! log_set_level {
    () => {
        $crate::__cmd_kv!("runtime", "log_set_level")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "log_set_level", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! log_flush {
    () => {
        $crate::__cmd_kv!("runtime", "log_flush")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "log_flush", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! sleep_ms {
    () => {
        $crate::__cmd_kv!("runtime", "sleep_ms")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "sleep_ms", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! delay_until {
    () => {
        $crate::__cmd_kv!("runtime", "delay_until")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "delay_until", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! heartbeat {
    () => {
        $crate::__cmd_kv!("runtime", "heartbeat")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("runtime", "heartbeat", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pin_alias {
    () => {
        $crate::__cmd_kv!("gpio", "pin_alias")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "pin_alias", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! reserve_pin {
    () => {
        $crate::__cmd_kv!("gpio", "reserve_pin")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "reserve_pin", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! release_pin {
    () => {
        $crate::__cmd_kv!("gpio", "release_pin")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "release_pin", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! digital_pulse {
    () => {
        $crate::__cmd_kv!("gpio", "digital_pulse")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "digital_pulse", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! digital_pulse_train {
    () => {
        $crate::__cmd_kv!("gpio", "digital_pulse_train")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "digital_pulse_train", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pullup {
    () => {
        $crate::__cmd_kv!("gpio", "pullup")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "pullup", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pulldown {
    () => {
        $crate::__cmd_kv!("gpio", "pulldown")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "pulldown", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! high_z {
    () => {
        $crate::__cmd_kv!("gpio", "high_z")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "high_z", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! debounce {
    () => {
        $crate::__cmd_kv!("gpio", "debounce")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "debounce", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! wait_high {
    () => {
        $crate::__cmd_kv!("gpio", "wait_high")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "wait_high", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! wait_low {
    () => {
        $crate::__cmd_kv!("gpio", "wait_low")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "wait_low", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! attach_interrupt {
    () => {
        $crate::__cmd_kv!("gpio", "attach_interrupt")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "attach_interrupt", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! detach_interrupt {
    () => {
        $crate::__cmd_kv!("gpio", "detach_interrupt")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "detach_interrupt", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! edge_count {
    () => {
        $crate::__cmd_kv!("gpio", "edge_count")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("gpio", "edge_count", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pwm_attach {
    () => {
        $crate::__cmd_kv!("pwm", "pwm_attach")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("pwm", "pwm_attach", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pwm_duty {
    () => {
        $crate::__cmd_kv!("pwm", "pwm_duty")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("pwm", "pwm_duty", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pwm_frequency {
    () => {
        $crate::__cmd_kv!("pwm", "pwm_frequency")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("pwm", "pwm_frequency", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pwm_stop {
    () => {
        $crate::__cmd_kv!("pwm", "pwm_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("pwm", "pwm_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! servo_attach {
    () => {
        $crate::__cmd_kv!("servo", "servo_attach")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("servo", "servo_attach", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! servo_center {
    () => {
        $crate::__cmd_kv!("servo", "servo_center")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("servo", "servo_center", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! servo_trim {
    () => {
        $crate::__cmd_kv!("servo", "servo_trim")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("servo", "servo_trim", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! servo_sweep {
    () => {
        $crate::__cmd_kv!("servo", "servo_sweep")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("servo", "servo_sweep", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! servo_detach {
    () => {
        $crate::__cmd_kv!("servo", "servo_detach")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("servo", "servo_detach", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! buzzer_tone {
    () => {
        $crate::__cmd_kv!("actuator", "buzzer_tone")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("actuator", "buzzer_tone", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! buzzer_stop {
    () => {
        $crate::__cmd_kv!("actuator", "buzzer_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("actuator", "buzzer_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! solenoid_pulse {
    () => {
        $crate::__cmd_kv!("actuator", "solenoid_pulse")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("actuator", "solenoid_pulse", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_enable {
    () => {
        $crate::__cmd_kv!("motor", "motor_enable")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_enable", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_disable {
    () => {
        $crate::__cmd_kv!("motor", "motor_disable")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_disable", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_set_direction {
    () => {
        $crate::__cmd_kv!("motor", "motor_set_direction")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_set_direction", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_set_voltage {
    () => {
        $crate::__cmd_kv!("motor", "motor_set_voltage")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_set_voltage", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_set_duty {
    () => {
        $crate::__cmd_kv!("motor", "motor_set_duty")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_set_duty", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_brake {
    () => {
        $crate::__cmd_kv!("motor", "motor_brake")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_brake", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_coast {
    () => {
        $crate::__cmd_kv!("motor", "motor_coast")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_coast", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_stop {
    () => {
        $crate::__cmd_kv!("motor", "motor_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_ramp {
    () => {
        $crate::__cmd_kv!("motor", "motor_ramp")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_ramp", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_current_limit {
    () => {
        $crate::__cmd_kv!("motor", "motor_current_limit")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_current_limit", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! motor_fault_reset {
    () => {
        $crate::__cmd_kv!("motor", "motor_fault_reset")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motor", "motor_fault_reset", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! differential_drive {
    () => {
        $crate::__cmd_kv!("motion", "differential_drive")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "differential_drive", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! mecanum_drive {
    () => {
        $crate::__cmd_kv!("motion", "mecanum_drive")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "mecanum_drive", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_arcade {
    () => {
        $crate::__cmd_kv!("motion", "drive_arcade")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_arcade", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_tank {
    () => {
        $crate::__cmd_kv!("motion", "drive_tank")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_tank", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_stop {
    () => {
        $crate::__cmd_kv!("motion", "drive_stop")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_stop", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_hold {
    () => {
        $crate::__cmd_kv!("motion", "drive_hold")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_hold", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_coast {
    () => {
        $crate::__cmd_kv!("motion", "drive_coast")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_coast", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! drive_brake {
    () => {
        $crate::__cmd_kv!("motion", "drive_brake")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "drive_brake", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! set_deadzone {
    () => {
        $crate::__cmd_kv!("motion", "set_deadzone")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "set_deadzone", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! set_ramp_time {
    () => {
        $crate::__cmd_kv!("motion", "set_ramp_time")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("motion", "set_ramp_time", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! adc_read {
    () => {
        $crate::__cmd_kv!("sensor", "adc_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "adc_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! adc_calibrate {
    () => {
        $crate::__cmd_kv!("sensor", "adc_calibrate")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "adc_calibrate", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! battery_read {
    () => {
        $crate::__cmd_kv!("sensor", "battery_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "battery_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! battery_publish {
    () => {
        $crate::__cmd_kv!("sensor", "battery_publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "battery_publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! voltage_read {
    () => {
        $crate::__cmd_kv!("sensor", "voltage_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "voltage_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! current_read {
    () => {
        $crate::__cmd_kv!("sensor", "current_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "current_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! temperature_read {
    () => {
        $crate::__cmd_kv!("sensor", "temperature_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "temperature_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! distance_read {
    () => {
        $crate::__cmd_kv!("sensor", "distance_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "distance_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! line_read {
    () => {
        $crate::__cmd_kv!("sensor", "line_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "line_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! reflectance_read {
    () => {
        $crate::__cmd_kv!("sensor", "reflectance_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "reflectance_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! button_read {
    () => {
        $crate::__cmd_kv!("sensor", "button_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "button_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! switch_read {
    () => {
        $crate::__cmd_kv!("sensor", "switch_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "switch_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! imu_read {
    () => {
        $crate::__cmd_kv!("sensor", "imu_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "imu_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! imu_publish {
    () => {
        $crate::__cmd_kv!("sensor", "imu_publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "imu_publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! gyro_read {
    () => {
        $crate::__cmd_kv!("sensor", "gyro_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "gyro_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! accel_read {
    () => {
        $crate::__cmd_kv!("sensor", "accel_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "accel_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! mag_read {
    () => {
        $crate::__cmd_kv!("sensor", "mag_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "mag_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! encoder_read {
    () => {
        $crate::__cmd_kv!("sensor", "encoder_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "encoder_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! encoder_reset {
    () => {
        $crate::__cmd_kv!("sensor", "encoder_reset")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "encoder_reset", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! pose_publish {
    () => {
        $crate::__cmd_kv!("sensor", "pose_publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "pose_publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! odom_publish {
    () => {
        $crate::__cmd_kv!("sensor", "odom_publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "odom_publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! fault_publish {
    () => {
        $crate::__cmd_kv!("sensor", "fault_publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("sensor", "fault_publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! uart_open {
    () => {
        $crate::__cmd_kv!("uart", "uart_open")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("uart", "uart_open", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! uart_close {
    () => {
        $crate::__cmd_kv!("uart", "uart_close")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("uart", "uart_close", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! uart_flush {
    () => {
        $crate::__cmd_kv!("uart", "uart_flush")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("uart", "uart_flush", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! uart_set_baud {
    () => {
        $crate::__cmd_kv!("uart", "uart_set_baud")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("uart", "uart_set_baud", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! uart_set_parity {
    () => {
        $crate::__cmd_kv!("uart", "uart_set_parity")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("uart", "uart_set_parity", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! i2c_open {
    () => {
        $crate::__cmd_kv!("i2c", "i2c_open")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("i2c", "i2c_open", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! i2c_close {
    () => {
        $crate::__cmd_kv!("i2c", "i2c_close")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("i2c", "i2c_close", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! i2c_read {
    () => {
        $crate::__cmd_kv!("i2c", "i2c_read")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("i2c", "i2c_read", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! i2c_write {
    () => {
        $crate::__cmd_kv!("i2c", "i2c_write")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("i2c", "i2c_write", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! i2c_probe {
    () => {
        $crate::__cmd_kv!("i2c", "i2c_probe")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("i2c", "i2c_probe", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! spi_open {
    () => {
        $crate::__cmd_kv!("spi", "spi_open")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("spi", "spi_open", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! spi_close {
    () => {
        $crate::__cmd_kv!("spi", "spi_close")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("spi", "spi_close", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! spi_transfer {
    () => {
        $crate::__cmd_kv!("spi", "spi_transfer")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("spi", "spi_transfer", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! spi_select {
    () => {
        $crate::__cmd_kv!("spi", "spi_select")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("spi", "spi_select", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! spi_deselect {
    () => {
        $crate::__cmd_kv!("spi", "spi_deselect")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("spi", "spi_deselect", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_open {
    () => {
        $crate::__cmd_kv!("bt", "bt_open")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_open", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_close {
    () => {
        $crate::__cmd_kv!("bt", "bt_close")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_close", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_scan {
    () => {
        $crate::__cmd_kv!("bt", "bt_scan")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_scan", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_pair {
    () => {
        $crate::__cmd_kv!("bt", "bt_pair")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_pair", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_unpair {
    () => {
        $crate::__cmd_kv!("bt", "bt_unpair")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_unpair", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_connect_controller {
    () => {
        $crate::__cmd_kv!("bt", "bt_connect_controller")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_connect_controller", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_disconnect_controller {
    () => {
        $crate::__cmd_kv!("bt", "bt_disconnect_controller")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_disconnect_controller", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_poll_controller {
    () => {
        $crate::__cmd_kv!("bt", "bt_poll_controller")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_poll_controller", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_rumble {
    () => {
        $crate::__cmd_kv!("bt", "bt_rumble")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_rumble", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bt_led {
    () => {
        $crate::__cmd_kv!("bt", "bt_led")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("bt", "bt_led", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! on_axis {
    () => {
        $crate::__cmd_kv!("input", "on_axis")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("input", "on_axis", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! on_button {
    () => {
        $crate::__cmd_kv!("input", "on_button")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("input", "on_button", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! controller_map {
    () => {
        $crate::__cmd_kv!("input", "controller_map")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("input", "controller_map", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! controller_deadzone {
    () => {
        $crate::__cmd_kv!("input", "controller_deadzone")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("input", "controller_deadzone", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! publish {
    () => {
        $crate::__cmd_kv!("integration", "publish")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "publish", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! request {
    () => {
        $crate::__cmd_kv!("integration", "request")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "request", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! reply {
    () => {
        $crate::__cmd_kv!("integration", "reply")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "reply", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! subscribe {
    () => {
        $crate::__cmd_kv!("integration", "subscribe")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "subscribe", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! unsubscribe {
    () => {
        $crate::__cmd_kv!("integration", "unsubscribe")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "unsubscribe", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! send_control {
    () => {
        $crate::__cmd_kv!("integration", "send_control")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "send_control", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! recv_control {
    () => {
        $crate::__cmd_kv!("integration", "recv_control")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "recv_control", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! recv_topic {
    () => {
        $crate::__cmd_kv!("integration", "recv_topic")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "recv_topic", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! wait_reply {
    () => {
        $crate::__cmd_kv!("integration", "wait_reply")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "wait_reply", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! wait_topic {
    () => {
        $crate::__cmd_kv!("integration", "wait_topic")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "wait_topic", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bind_parent {
    () => {
        $crate::__cmd_kv!("integration", "bind_parent")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "bind_parent", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! bind_child {
    () => {
        $crate::__cmd_kv!("integration", "bind_child")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "bind_child", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! switch_role {
    () => {
        $crate::__cmd_kv!("integration", "switch_role")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "switch_role", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! set_peer_policy {
    () => {
        $crate::__cmd_kv!("integration", "set_peer_policy")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "set_peer_policy", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! prefer_transport {
    () => {
        $crate::__cmd_kv!("integration", "prefer_transport")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "prefer_transport", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! set_wifi_mode {
    () => {
        $crate::__cmd_kv!("integration", "set_wifi_mode")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "set_wifi_mode", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! start_hotspot {
    () => {
        $crate::__cmd_kv!("integration", "start_hotspot")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "start_hotspot", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! connect_wifi {
    () => {
        $crate::__cmd_kv!("integration", "connect_wifi")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "connect_wifi", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! enable_discovery {
    () => {
        $crate::__cmd_kv!("integration", "enable_discovery")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "enable_discovery", $( $key = $value ),+)
    };
}

#[macro_export]
macro_rules! announce_now {
    () => {
        $crate::__cmd_kv!("integration", "announce_now")
    };
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        $crate::__cmd_kv!("integration", "announce_now", $( $key = $value ),+)
    };
}
