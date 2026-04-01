use crate::command::CommandResult;
use crate::error::CmdError;
use crate::types::{ControllerState, Level, PinMode, SafetyState};
use crate::CommandEnvelope;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

pub trait Backend: Send + Sync {
    fn execute(&self, command: CommandEnvelope) -> Result<CommandResult, CmdError>;
}

#[derive(Default)]
pub struct SimBackend {
    state: Mutex<SimState>,
}

struct SimState {
    pins: HashMap<String, PinState>,
    analog_inputs: HashMap<u8, u16>,
    analog_outputs: HashMap<u8, u16>,
    analog_read_resolution: u8,
    analog_write_resolution: u8,
    analog_reference: String,
    tones: HashMap<String, ToneState>,
    pwm_profiles: HashMap<u8, PwmProfile>,
    motors: HashMap<String, f64>,
    uart_ports: HashMap<String, UartState>,
    controller: ControllerState,
    safety: SimSafety,
    history: Vec<CommandEnvelope>,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            pins: HashMap::new(),
            analog_inputs: HashMap::new(),
            analog_outputs: HashMap::new(),
            analog_read_resolution: 10,
            analog_write_resolution: 8,
            analog_reference: "default".to_string(),
            tones: HashMap::new(),
            pwm_profiles: HashMap::new(),
            motors: HashMap::new(),
            uart_ports: HashMap::new(),
            controller: ControllerState::default(),
            safety: SimSafety::default(),
            history: Vec::new(),
        }
    }
}

#[derive(Default)]
struct SimSafety {
    latched: bool,
}

#[derive(Clone, Copy)]
struct PinState {
    mode: PinMode,
    level: Level,
}

impl Default for PinState {
    fn default() -> Self {
        Self {
            mode: PinMode::Input,
            level: Level::Low,
        }
    }
}

#[derive(Default)]
struct UartState {
    baud: u32,
    tx_log: Vec<String>,
    rx_queue: VecDeque<String>,
}

#[derive(Clone)]
struct ToneState {
    frequency_hz: u32,
    duration_ms: Option<u64>,
}

#[derive(Clone)]
struct PwmProfile {
    frequency_hz: u32,
    duty_percent: f64,
    enabled: bool,
}

impl Default for PwmProfile {
    fn default() -> Self {
        Self {
            frequency_hz: 25_000,
            duty_percent: 0.0,
            enabled: false,
        }
    }
}

impl SimBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_controller_state(self, controller: ControllerState) -> Self {
        let mut state = self.state.lock().expect("sim backend mutex poisoned");
        state.controller = controller;
        drop(state);
        self
    }
}

impl Backend for SimBackend {
    fn execute(&self, command: CommandEnvelope) -> Result<CommandResult, CmdError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CmdError::Backend("sim backend mutex poisoned".to_string()))?;
        let args = command_args(&command.args)?;

        if state.safety.latched && is_blocked_in_stop(&command.domain, &command.action) {
            return Err(CmdError::SafetyStopLatched);
        }

        state.history.push(command.clone());

        let result = match (command.domain.as_str(), command.action.as_str()) {
            ("runtime", "panic_stop") | ("runtime", "emergency_stop") => {
                state.safety.latched = true;
                CommandResult::ok_with_data(
                    command.id,
                    "safety stop latched",
                    json!({ "safety": SafetyState::EmergencyStopped }),
                )
            }
            ("runtime", "clear_stop") => {
                state.safety.latched = false;
                CommandResult::ok_with_data(
                    command.id,
                    "safety stop cleared",
                    json!({ "safety": SafetyState::Normal }),
                )
            }
            ("runtime", "status") => {
                let pins = state.pins.len();
                let motors = state.motors.len();
                let uart = state.uart_ports.len();
                CommandResult::ok_with_data(
                    command.id,
                    "runtime status",
                    json!({
                        "safety_latched": state.safety.latched,
                        "pins": pins,
                        "motors": motors,
                        "uart_ports": uart,
                        "controller_connected": state.controller.connected,
                        "history_len": state.history.len(),
                    }),
                )
            }
            ("runtime", "health_check") => CommandResult::ok_with_data(
                command.id,
                "health check ok",
                json!({ "ok": true }),
            ),
            ("gpio", "pin_mode") => {
                let pin = arg_string(&args, "pin")?;
                let mode = parse_pin_mode(args.get("mode"))?;
                let entry = state.pins.entry(pin.clone()).or_default();
                entry.mode = mode;
                if matches!(mode, PinMode::Output | PinMode::Pwm) {
                    entry.level = Level::Low;
                }
                CommandResult::ok_with_data(
                    command.id,
                    "pin mode updated",
                    json!({ "pin": pin, "mode": mode }),
                )
            }
            ("gpio", "digital_write") => {
                let pin = arg_string(&args, "pin")?;
                let level = parse_level(args.get("level"))?;
                let entry = state
                    .pins
                    .get_mut(&pin)
                    .ok_or_else(|| CmdError::PinNotConfigured(pin.clone()))?;
                if !matches!(entry.mode, PinMode::Output | PinMode::Pwm) {
                    return Err(CmdError::PinModeMismatch(pin));
                }
                entry.level = level;
                CommandResult::ok_with_data(command.id, "digital write ok", json!({ "level": level }))
            }
            ("gpio", "digital_read") => {
                let pin = arg_string(&args, "pin")?;
                let entry = state
                    .pins
                    .get(&pin)
                    .ok_or_else(|| CmdError::PinNotConfigured(pin.clone()))?;
                CommandResult::ok_with_data(
                    command.id,
                    "digital read ok",
                    json!({ "pin": pin, "level": entry.level, "is_high": matches!(entry.level, Level::High) }),
                )
            }
            ("gpio", "digital_toggle") => {
                let pin = arg_string(&args, "pin")?;
                let entry = state
                    .pins
                    .get_mut(&pin)
                    .ok_or_else(|| CmdError::PinNotConfigured(pin.clone()))?;
                if !matches!(entry.mode, PinMode::Output | PinMode::Pwm) {
                    return Err(CmdError::PinModeMismatch(pin));
                }
                entry.level = if entry.level == Level::High {
                    Level::Low
                } else {
                    Level::High
                };
                CommandResult::ok_with_data(command.id, "digital toggle ok", json!({ "level": entry.level }))
            }
            ("analog", "analog_read") => {
                let pin = arg_u8(&args, "pin")?;
                let max = ((1u32 << state.analog_read_resolution.min(16)) - 1) as u16;
                let value = *state.analog_inputs.entry(pin).or_insert(max / 2);
                CommandResult::ok_with_data(
                    command.id,
                    "analog read ok",
                    json!({
                        "pin": pin,
                        "value": value.min(max),
                        "resolution": state.analog_read_resolution,
                        "reference": state.analog_reference,
                    }),
                )
            }
            ("analog", "analog_read_resolution") => {
                let bits = arg_u8(&args, "bits")?;
                if bits == 0 || bits > 16 {
                    return Err(CmdError::InvalidArgument {
                        key: "bits".to_string(),
                        reason: "analog read resolution must be 1..=16".to_string(),
                    });
                }
                state.analog_read_resolution = bits;
                CommandResult::ok_with_data(
                    command.id,
                    "analog read resolution updated",
                    json!({ "bits": bits }),
                )
            }
            ("analog", "analog_reference") => {
                let reference = arg_string(&args, "reference")?;
                state.analog_reference = reference.clone();
                CommandResult::ok_with_data(
                    command.id,
                    "analog reference updated",
                    json!({ "reference": reference }),
                )
            }
            ("analog", "analog_write") => {
                let pin = arg_u8(&args, "pin")?;
                let value = arg_u16(&args, "value")?;
                let max = ((1u32 << state.analog_write_resolution.min(16)) - 1) as u16;
                state.analog_outputs.insert(pin, value.min(max));
                CommandResult::ok_with_data(
                    command.id,
                    "analog write ok",
                    json!({
                        "pin": pin,
                        "value": value.min(max),
                        "resolution": state.analog_write_resolution,
                    }),
                )
            }
            ("analog", "analog_write_resolution") => {
                let bits = arg_u8(&args, "bits")?;
                if bits == 0 || bits > 16 {
                    return Err(CmdError::InvalidArgument {
                        key: "bits".to_string(),
                        reason: "analog write resolution must be 1..=16".to_string(),
                    });
                }
                state.analog_write_resolution = bits;
                CommandResult::ok_with_data(
                    command.id,
                    "analog write resolution updated",
                    json!({ "bits": bits }),
                )
            }
            ("pwm", "pwm_frequency") => {
                let pin = arg_u8(&args, "pin")?;
                let frequency_hz = arg_u32_any(&args, &["frequency_hz", "frequency"])?;
                if frequency_hz == 0 {
                    return Err(CmdError::InvalidArgument {
                        key: "frequency_hz".to_string(),
                        reason: "frequency must be greater than zero".to_string(),
                    });
                }

                let duty_percent = {
                    let profile = state.pwm_profiles.entry(pin).or_insert_with(PwmProfile::default);
                    profile.frequency_hz = frequency_hz;
                    profile.duty_percent
                };

                CommandResult::ok_with_data(
                    command.id,
                    "pwm frequency updated",
                    json!({
                        "pin": pin,
                        "frequency_hz": frequency_hz,
                        "duty_percent": duty_percent,
                    }),
                )
            }
            ("pwm", "pwm_write") => {
                let pin = arg_u8(&args, "pin")?;
                let duty_percent = arg_f64_any(&args, &["duty_percent", "duty"])?;
                let duty_percent = duty_percent.clamp(0.0, 100.0);
                let profile = state.pwm_profiles.entry(pin).or_insert_with(PwmProfile::default);
                profile.duty_percent = duty_percent;
                profile.enabled = duty_percent > 0.0;

                CommandResult::ok_with_data(
                    command.id,
                    "pwm write ok",
                    json!({
                        "pin": pin,
                        "frequency_hz": profile.frequency_hz,
                        "duty_percent": profile.duty_percent,
                        "enabled": profile.enabled,
                    }),
                )
            }
            ("tone", "tone") => {
                let pin = arg_string_any(&args, &["pin", "id", "name"])?;
                let frequency_hz = arg_u32_any(&args, &["frequency_hz", "frequency"])?;
                let duration_ms = args.get("duration_ms").and_then(Value::as_u64);
                state.tones.insert(
                    pin.clone(),
                    ToneState {
                        frequency_hz,
                        duration_ms,
                    },
                );
                CommandResult::ok_with_data(
                    command.id,
                    "tone started",
                    json!({ "pin": pin, "frequency_hz": frequency_hz, "duration_ms": duration_ms }),
                )
            }
            ("tone", "no_tone") => {
                let pin = arg_string_any(&args, &["pin", "id", "name"])?;
                let removed = state.tones.remove(&pin);
                let (frequency_hz, duration_ms) = removed
                    .map(|t| (Some(t.frequency_hz), t.duration_ms))
                    .unwrap_or((None, None));
                CommandResult::ok_with_data(
                    command.id,
                    "tone stopped",
                    json!({ "pin": pin, "frequency_hz": frequency_hz, "duration_ms": duration_ms }),
                )
            }
            ("motor", action)
                if matches!(
                    action,
                    "motor_set_speed"
                        | "motor_set_duty"
                        | "motor_set_voltage"
                        | "motor_enable"
                        | "motor_disable"
                        | "motor_brake"
                        | "motor_stop"
                        | "motor_coast"
                        | "motor_ramp"
                ) =>
            {
                let motor = arg_string_any(&args, &["motor", "name", "id"]).unwrap_or_else(|_| "default".to_string());
                let speed = arg_f64_any(&args, &["speed", "duty", "voltage", "target"]).unwrap_or(0.0);
                state.motors.insert(motor.clone(), speed);
                CommandResult::ok_with_data(
                    command.id,
                    "motor command accepted",
                    json!({ "motor": motor, "action": action, "value": speed }),
                )
            }
            ("uart", "uart_open") => {
                let port = arg_string_any(&args, &["port", "id", "name"])?;
                let baud = arg_u32_any(&args, &["baud", "baud_rate"]).unwrap_or(115200);
                state.uart_ports.insert(
                    port.clone(),
                    UartState {
                        baud,
                        tx_log: Vec::new(),
                        rx_queue: VecDeque::new(),
                    },
                );
                CommandResult::ok_with_data(command.id, "uart opened", json!({ "port": port, "baud": baud }))
            }
            ("uart", "uart_close") => {
                let port = arg_string_any(&args, &["port", "id", "name"])?;
                state.uart_ports.remove(&port);
                CommandResult::ok_with_data(command.id, "uart closed", json!({ "port": port }))
            }
            ("uart", "uart_send") => {
                let port = arg_string_any(&args, &["port", "id", "name"])?;
                let data = arg_string_any(&args, &["data", "text", "payload"]).unwrap_or_default();
                let uart = state
                    .uart_ports
                    .get_mut(&port)
                    .ok_or_else(|| CmdError::ResourceNotFound(format!("uart port `{port}`")))?;
                uart.tx_log.push(data.clone());
                CommandResult::ok_with_data(command.id, "uart send ok", json!({ "port": port, "bytes": data.len() }))
            }
            ("uart", "uart_read") => {
                let port = arg_string_any(&args, &["port", "id", "name"])?;
                let uart = state
                    .uart_ports
                    .get_mut(&port)
                    .ok_or_else(|| CmdError::ResourceNotFound(format!("uart port `{port}`")))?;
                let data = uart.rx_queue.pop_front().unwrap_or_default();
                CommandResult::ok_with_data(
                    command.id,
                    "uart read ok",
                    json!({ "port": port, "baud": uart.baud, "data": data }),
                )
            }
            ("bt", "bt_connect_controller") => {
                state.controller.connected = true;
                CommandResult::ok(command.id, "controller connected")
            }
            ("bt", "bt_disconnect_controller") => {
                state.controller.connected = false;
                CommandResult::ok(command.id, "controller disconnected")
            }
            ("bt", "bt_poll_controller") => {
                let controller = serde_json::to_value(&state.controller)
                    .map_err(|e| CmdError::Backend(format!("serialize controller failed: {e}")))?;
                CommandResult::ok_with_data(command.id, "controller snapshot", controller)
            }
            ("bt", "bt_axis") => {
                let axis = arg_string(&args, "axis")?;
                let value = match axis.as_str() {
                    "lx" => state.controller.lx,
                    "ly" => state.controller.ly,
                    "rx" => state.controller.rx,
                    "ry" => state.controller.ry,
                    "lt" => state.controller.lt,
                    "rt" => state.controller.rt,
                    _ => {
                        return Err(CmdError::InvalidArgument {
                            key: "axis".to_string(),
                            reason: format!("unknown axis `{axis}`"),
                        })
                    }
                };
                CommandResult::ok_with_data(command.id, "axis value", json!({ "axis": axis, "value": value }))
            }
            ("bt", "bt_button") => {
                let button = arg_string(&args, "button")?;
                let pressed = match button.as_str() {
                    "a" => state.controller.a,
                    "b" => state.controller.b,
                    "x" => state.controller.x,
                    "y" => state.controller.y,
                    "lb" => state.controller.lb,
                    "rb" => state.controller.rb,
                    "start" => state.controller.start,
                    "select" => state.controller.select,
                    _ => {
                        return Err(CmdError::InvalidArgument {
                            key: "button".to_string(),
                            reason: format!("unknown button `{button}`"),
                        })
                    }
                };
                CommandResult::ok_with_data(
                    command.id,
                    "button value",
                    json!({ "button": button, "pressed": pressed }),
                )
            }
            _ => CommandResult::ok_with_data(
                command.id,
                "command accepted (simulated)",
                json!({
                    "domain": command.domain,
                    "action": command.action,
                    "args": args,
                }),
            ),
        };

        Ok(result)
    }
}

fn is_blocked_in_stop(domain: &str, action: &str) -> bool {
    if domain == "motor" || domain == "pwm" || domain == "servo" {
        return true;
    }
    domain == "gpio" && matches!(action, "digital_write" | "digital_toggle" | "digital_pulse")
}

fn command_args(value: &Value) -> Result<Map<String, Value>, CmdError> {
    match value {
        Value::Object(map) => Ok(map.clone()),
        Value::Null => Ok(Map::new()),
        _ => Err(CmdError::InvalidArgument {
            key: "args".to_string(),
            reason: "expected object".to_string(),
        }),
    }
}

fn arg_string(args: &Map<String, Value>, key: &str) -> Result<String, CmdError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| CmdError::InvalidArgument {
            key: key.to_string(),
            reason: "missing string".to_string(),
        })
}

fn arg_string_any(args: &Map<String, Value>, keys: &[&str]) -> Result<String, CmdError> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .map(ToString::to_string)
        .ok_or_else(|| CmdError::InvalidArgument {
            key: keys.join("|"),
            reason: "missing string".to_string(),
        })
}

fn arg_u32_any(args: &Map<String, Value>, keys: &[&str]) -> Result<u32, CmdError> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_u64))
        .map(|v| v as u32)
        .ok_or_else(|| CmdError::InvalidArgument {
            key: keys.join("|"),
            reason: "missing unsigned integer".to_string(),
        })
}

fn arg_u8(args: &Map<String, Value>, key: &str) -> Result<u8, CmdError> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|v| v as u8)
        .ok_or_else(|| CmdError::InvalidArgument {
            key: key.to_string(),
            reason: "missing unsigned integer".to_string(),
        })
}

fn arg_u16(args: &Map<String, Value>, key: &str) -> Result<u16, CmdError> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|v| v as u16)
        .ok_or_else(|| CmdError::InvalidArgument {
            key: key.to_string(),
            reason: "missing unsigned integer".to_string(),
        })
}

fn arg_f64_any(args: &Map<String, Value>, keys: &[&str]) -> Result<f64, CmdError> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_f64))
        .ok_or_else(|| CmdError::InvalidArgument {
            key: keys.join("|"),
            reason: "missing number".to_string(),
        })
}

fn parse_pin_mode(value: Option<&Value>) -> Result<PinMode, CmdError> {
    let Some(raw) = value.and_then(Value::as_str) else {
        return Err(CmdError::InvalidArgument {
            key: "mode".to_string(),
            reason: "missing string".to_string(),
        });
    };
    match raw.to_ascii_lowercase().as_str() {
        "input" => Ok(PinMode::Input),
        "output" => Ok(PinMode::Output),
        "pwm" => Ok(PinMode::Pwm),
        "analog" => Ok(PinMode::Analog),
        "interrupt" => Ok(PinMode::Interrupt),
        other => Err(CmdError::InvalidArgument {
            key: "mode".to_string(),
            reason: format!("unknown mode `{other}`"),
        }),
    }
}

fn parse_level(value: Option<&Value>) -> Result<Level, CmdError> {
    let Some(raw) = value else {
        return Err(CmdError::InvalidArgument {
            key: "level".to_string(),
            reason: "missing value".to_string(),
        });
    };
    if let Some(b) = raw.as_bool() {
        return Ok(if b { Level::High } else { Level::Low });
    }
    if let Some(s) = raw.as_str() {
        return match s.to_ascii_lowercase().as_str() {
            "high" | "1" | "true" => Ok(Level::High),
            "low" | "0" | "false" => Ok(Level::Low),
            other => Err(CmdError::InvalidArgument {
                key: "level".to_string(),
                reason: format!("unknown level `{other}`"),
            }),
        };
    }
    Err(CmdError::InvalidArgument {
        key: "level".to_string(),
        reason: "expected bool or string".to_string(),
    })
}
