use crate::command::CommandResult;
use crate::error::CmdError;
use crate::types::{Level, PinMode};
use crate::CommandEnvelope;
use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub struct RealBackend {
    state: Mutex<RealState>,
}

struct RealState {
    gpio_chip: Option<Chip>,
    gpio_lines: HashMap<u32, LineHandle>,
    gpio_modes: HashMap<u32, PinMode>,
    pwm_channels: HashMap<u32, PwmChannel>,
    pwm_profiles: HashMap<u32, PwmProfile>,
    analog_read_resolution: u8,
    analog_write_resolution: u8,
    analog_reference: String,
}

struct PwmChannel {
    duty_ns: u64,
    enabled: bool,
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

impl RealBackend {
    pub fn new() -> Result<Self, CmdError> {
        let chip = Self::open_gpio_chip()?;
        Ok(Self {
            state: Mutex::new(RealState {
                gpio_chip: Some(chip),
                gpio_lines: HashMap::new(),
                gpio_modes: HashMap::new(),
                pwm_channels: HashMap::new(),
                pwm_profiles: HashMap::new(),
                analog_read_resolution: 10,
                analog_write_resolution: 8,
                analog_reference: "default".to_string(),
            }),
        })
    }

    fn open_gpio_chip() -> Result<Chip, CmdError> {
        for i in 0..10 {
            let path = format!("/dev/gpiochip{}", i);
            if Path::new(&path).exists() {
                match Chip::new(&path) {
                    Ok(chip) => return Ok(chip),
                    Err(e) => {
                        if i == 0 {
                            return Err(CmdError::Backend(format!(
                                "Failed to open {}: {}. Check permissions (add user to gpio group)",
                                path, e
                            )));
                        }
                    }
                }
            }
        }
        Err(CmdError::Backend(
            "No GPIO chip found. Is this running on a Raspberry Pi?".to_string(),
        ))
    }

    fn bcm_to_line(bcm_pin: &str) -> Result<u32, CmdError> {
        bcm_pin
            .trim_start_matches("GPIO")
            .trim_start_matches("BCM")
            .parse::<u32>()
            .map_err(|_| CmdError::InvalidArgument {
                key: "pin".to_string(),
                reason: format!("Invalid pin format: {}. Use GPIO numbers like '17' or 'GPIO17'", bcm_pin),
            })
    }

    fn setup_pwm(chip: u32, channel: u32, period_ns: u64) -> Result<(), CmdError> {
        let base_path = format!("/sys/class/pwm/pwmchip{}", chip);
        let pwm_path = format!("{}/pwm{}", base_path, channel);

        if !Path::new(&pwm_path).exists() {
            let export_path = format!("{}/export", base_path);
            std::fs::write(&export_path, channel.to_string()).map_err(|e| {
                CmdError::Backend(format!(
                    "Failed to export PWM channel {}: {}. Check permissions.",
                    channel, e
                ))
            })?;
            thread::sleep(Duration::from_millis(100));
        }

        let period_path = format!("{}/period", pwm_path);
        std::fs::write(&period_path, period_ns.to_string()).map_err(|e| {
            CmdError::Backend(format!("Failed to set PWM period: {}", e))
        })?;

        Ok(())
    }

    fn write_pwm_duty(chip: u32, channel: u32, duty_ns: u64) -> Result<(), CmdError> {
        let duty_path = format!("/sys/class/pwm/pwmchip{}/pwm{}/duty_cycle", chip, channel);
        std::fs::write(&duty_path, duty_ns.to_string())
            .map_err(|e| CmdError::Backend(format!("Failed to set PWM duty cycle: {}", e)))
    }

    fn enable_pwm(chip: u32, channel: u32, enable: bool) -> Result<(), CmdError> {
        let enable_path = format!("/sys/class/pwm/pwmchip{}/pwm{}/enable", chip, channel);
        let value = if enable { "1" } else { "0" };
        std::fs::write(&enable_path, value)
            .map_err(|e| CmdError::Backend(format!("Failed to enable/disable PWM: {}", e)))
    }

    fn get_hardware_pwm_mapping(bcm_pin: u32) -> Option<(u32, u32)> {
        match bcm_pin {
            12 => Some((0, 0)),
            13 => Some((0, 1)),
            18 => Some((0, 0)),
            19 => Some((0, 1)),
            _ => None,
        }
    }

    fn pwm_period_ns(frequency_hz: u32) -> Result<u64, CmdError> {
        if frequency_hz == 0 {
            return Err(CmdError::InvalidArgument {
                key: "frequency_hz".to_string(),
                reason: "frequency must be greater than zero".to_string(),
            });
        }
        let period_ns = 1_000_000_000u64 / frequency_hz as u64;
        if period_ns == 0 {
            return Err(CmdError::InvalidArgument {
                key: "frequency_hz".to_string(),
                reason: "frequency is too high for hardware PWM".to_string(),
            });
        }
        Ok(period_ns)
    }
}

impl crate::Backend for RealBackend {
    fn execute(&self, command: CommandEnvelope) -> Result<CommandResult, CmdError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CmdError::Backend("real backend mutex poisoned".to_string()))?;
        let args = command_args(&command.args)?;

        let result = match (command.domain.as_str(), command.action.as_str()) {
            ("gpio", "pin_mode") => {
                let pin_str = arg_string(&args, "pin")?;
                let line_num = Self::bcm_to_line(&pin_str)?;
                let mode = parse_pin_mode(args.get("mode"))?;

                if let Some(old_handle) = state.gpio_lines.remove(&line_num) {
                    drop(old_handle);
                }

                let chip = state.gpio_chip.as_mut().ok_or_else(|| {
                    CmdError::Backend("GPIO chip not initialized".to_string())
                })?;

                let line = chip.get_line(line_num).map_err(|e| {
                    CmdError::Backend(format!("Failed to get GPIO line {}: {}", line_num, e))
                })?;

                let handle = match mode {
                    PinMode::Input => line
                        .request(LineRequestFlags::INPUT, 0, "canweeb")
                        .map_err(|e| {
                            CmdError::Backend(format!("Failed to configure input: {}", e))
                        })?,
                    PinMode::Output | PinMode::Pwm => line
                        .request(LineRequestFlags::OUTPUT, 0, "canweeb")
                        .map_err(|e| {
                            CmdError::Backend(format!("Failed to configure output: {}", e))
                        })?,
                    _ => {
                        return Err(CmdError::InvalidArgument {
                            key: "mode".to_string(),
                            reason: format!("Mode {:?} not supported on real GPIO", mode),
                        })
                    }
                };

                state.gpio_lines.insert(line_num, handle);
                state.gpio_modes.insert(line_num, mode);

                CommandResult::ok_with_data(
                    command.id,
                    "pin mode configured",
                    json!({ "pin": pin_str, "line": line_num, "mode": mode }),
                )
            }
            ("gpio", "digital_write") => {
                let pin_str = arg_string(&args, "pin")?;
                let line_num = Self::bcm_to_line(&pin_str)?;
                let level = parse_level(args.get("level"))?;

                let handle = state.gpio_lines.get(&line_num).ok_or_else(|| {
                    CmdError::PinNotConfigured(format!(
                        "GPIO{} not configured. Call pinMode first.",
                        line_num
                    ))
                })?;

                let value = match level {
                    Level::High => 1,
                    Level::Low => 0,
                };

                handle.set_value(value).map_err(|e| {
                    CmdError::Backend(format!("Failed to write GPIO: {}", e))
                })?;

                CommandResult::ok_with_data(
                    command.id,
                    "digital write ok",
                    json!({ "pin": pin_str, "level": level }),
                )
            }
            ("gpio", "digital_read") => {
                let pin_str = arg_string(&args, "pin")?;
                let line_num = Self::bcm_to_line(&pin_str)?;

                let handle = state.gpio_lines.get(&line_num).ok_or_else(|| {
                    CmdError::PinNotConfigured(format!(
                        "GPIO{} not configured. Call pinMode first.",
                        line_num
                    ))
                })?;

                let value = handle
                    .get_value()
                    .map_err(|e| CmdError::Backend(format!("Failed to read GPIO: {}", e)))?;

                let level = if value == 1 {
                    Level::High
                } else {
                    Level::Low
                };

                CommandResult::ok_with_data(
                    command.id,
                    "digital read ok",
                    json!({ "pin": pin_str, "level": level, "is_high": value == 1 }),
                )
            }
            ("analog", "analog_write") => {
                let pin = arg_u8(&args, "pin")?;
                let value = arg_u16(&args, "value")?;

                let max = ((1u32 << state.analog_write_resolution.min(16)) - 1) as u16;
                let clamped_value = value.min(max);

                if let Some((chip, channel)) = Self::get_hardware_pwm_mapping(pin as u32) {
                    let period_ns = 20_000_000u64;
                    let duty_ns = (period_ns * clamped_value as u64) / max as u64;

                    if !state.pwm_channels.contains_key(&(pin as u32)) {
                        Self::setup_pwm(chip, channel, period_ns)?;
                        state.pwm_channels.insert(
                            pin as u32,
                            PwmChannel {
                                duty_ns: 0,
                                enabled: false,
                            },
                        );
                    }

                    Self::write_pwm_duty(chip, channel, duty_ns)?;

                    if let Some(pwm_ch) = state.pwm_channels.get_mut(&(pin as u32)) {
                        pwm_ch.duty_ns = duty_ns;
                        if !pwm_ch.enabled && clamped_value > 0 {
                            Self::enable_pwm(chip, channel, true)?;
                            pwm_ch.enabled = true;
                        } else if pwm_ch.enabled && clamped_value == 0 {
                            Self::enable_pwm(chip, channel, false)?;
                            pwm_ch.enabled = false;
                        }
                    }

                    CommandResult::ok_with_data(
                        command.id,
                        "analog write (PWM) ok",
                        json!({
                            "pin": pin,
                            "value": clamped_value,
                            "duty_ns": duty_ns,
                            "period_ns": period_ns,
                            "resolution": state.analog_write_resolution,
                        }),
                    )
                } else {
                    return Err(CmdError::InvalidArgument {
                        key: "pin".to_string(),
                        reason: format!(
                            "Pin {} does not support hardware PWM. Use pins 12, 13, 18, or 19.",
                            pin
                        ),
                    });
                }
            }
            ("pwm", "pwm_frequency") => {
                let pin = arg_u8(&args, "pin")?;
                let frequency_hz = arg_u32_any(&args, &["frequency_hz", "frequency"])?;

                let (chip, channel) = Self::get_hardware_pwm_mapping(pin as u32).ok_or_else(|| {
                    CmdError::InvalidArgument {
                        key: "pin".to_string(),
                        reason: format!(
                            "Pin {} does not support hardware PWM. Use pins 12, 13, 18, or 19.",
                            pin
                        ),
                    }
                })?;

                let period_ns = Self::pwm_period_ns(frequency_hz)?;
                let duty_percent = {
                    let profile = state
                        .pwm_profiles
                        .entry(pin as u32)
                        .or_insert_with(PwmProfile::default);
                    profile.frequency_hz = frequency_hz;
                    profile.duty_percent
                };
                let duty_ns = ((period_ns as f64) * duty_percent / 100.0)
                    .round()
                    .min(period_ns as f64) as u64;

                Self::setup_pwm(chip, channel, period_ns)?;
                Self::write_pwm_duty(chip, channel, duty_ns)?;

                let enabled = duty_percent > 0.0;
                Self::enable_pwm(chip, channel, enabled)?;
                if let Some(profile) = state.pwm_profiles.get_mut(&(pin as u32)) {
                    profile.enabled = enabled;
                }

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

                let (chip, channel) = Self::get_hardware_pwm_mapping(pin as u32).ok_or_else(|| {
                    CmdError::InvalidArgument {
                        key: "pin".to_string(),
                        reason: format!(
                            "Pin {} does not support hardware PWM. Use pins 12, 13, 18, or 19.",
                            pin
                        ),
                    }
                })?;

                let frequency_hz = {
                    let profile = state
                        .pwm_profiles
                        .entry(pin as u32)
                        .or_insert_with(PwmProfile::default);
                    profile.duty_percent = duty_percent;
                    profile.frequency_hz
                };

                let period_ns = Self::pwm_period_ns(frequency_hz)?;
                let duty_ns = ((period_ns as f64) * duty_percent / 100.0)
                    .round()
                    .min(period_ns as f64) as u64;

                Self::setup_pwm(chip, channel, period_ns)?;
                Self::write_pwm_duty(chip, channel, duty_ns)?;

                let enabled = duty_percent > 0.0;
                Self::enable_pwm(chip, channel, enabled)?;
                if let Some(profile) = state.pwm_profiles.get_mut(&(pin as u32)) {
                    profile.enabled = enabled;
                }

                CommandResult::ok_with_data(
                    command.id,
                    "pwm write ok",
                    json!({
                        "pin": pin,
                        "frequency_hz": frequency_hz,
                        "duty_percent": duty_percent,
                        "enabled": enabled,
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
            ("analog", "analog_read") => {
                return Err(CmdError::Backend(
                    "Raspberry Pi does not have built-in ADC. Use external ADC like MCP3008 via SPI."
                        .to_string(),
                ));
            }
            ("runtime", "health_check") => CommandResult::ok_with_data(
                command.id,
                "health check ok",
                json!({ "ok": true, "backend": "real" }),
            ),
            _ => CommandResult::ok_with_data(
                command.id,
                "command not handled by real backend",
                json!({
                    "domain": command.domain,
                    "action": command.action,
                    "note": "Command forwarded or not implemented",
                }),
            ),
        };

        Ok(result)
    }
}

impl Default for RealBackend {
    fn default() -> Self {
        Self::new().expect("Failed to initialize RealBackend")
    }
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
