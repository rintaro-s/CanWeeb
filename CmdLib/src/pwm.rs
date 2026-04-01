use crate::arduino::{pinMode, pwmFrequency, pwmWrite};
use crate::{CmdError, PinMode};

#[derive(Debug, Clone)]
pub struct PwmOutput {
    pin: u8,
    frequency_hz: u32,
    duty_percent: f64,
    min_duty_percent: f64,
    max_duty_percent: f64,
}

impl PwmOutput {
    pub fn new(pin: u8) -> Self {
        Self {
            pin,
            frequency_hz: 25_000,
            duty_percent: 0.0,
            min_duty_percent: 0.0,
            max_duty_percent: 100.0,
        }
    }

    pub fn pin(&self) -> u8 {
        self.pin
    }

    pub fn frequency_hz(&self) -> u32 {
        self.frequency_hz
    }

    pub fn duty_percent(&self) -> f64 {
        self.duty_percent
    }

    pub fn range(mut self, min_duty_percent: f64, max_duty_percent: f64) -> Self {
        if min_duty_percent <= max_duty_percent {
            self.min_duty_percent = min_duty_percent;
            self.max_duty_percent = max_duty_percent;
            self.duty_percent = self.clamp_duty(self.duty_percent);
        }
        self
    }

    pub fn frequency(mut self, frequency_hz: u32) -> Self {
        self.frequency_hz = frequency_hz.max(1);
        self
    }

    pub fn with_duty_percent(mut self, duty_percent: f64) -> Self {
        self.duty_percent = self.clamp_duty(duty_percent);
        self
    }

    pub fn duty_ratio(self, ratio: f64) -> Self {
        self.with_duty_percent(ratio * 100.0)
    }

    pub fn start(&self) -> Result<(), CmdError> {
        pinMode(&self.pin.to_string(), PinMode::Pwm)?;
        pwmFrequency(self.pin, self.frequency_hz)?;
        pwmWrite(self.pin, self.duty_percent())?;
        Ok(())
    }

    pub fn apply(&self) -> Result<(), CmdError> {
        self.start()
    }

    pub fn set_frequency(&mut self, frequency_hz: u32) -> Result<(), CmdError> {
        self.frequency_hz = frequency_hz.max(1);
        pwmFrequency(self.pin, self.frequency_hz)?;
        Ok(())
    }

    pub fn set_duty_percent(&mut self, duty_percent: f64) -> Result<(), CmdError> {
        self.duty_percent = self.clamp_duty(duty_percent);
        pwmWrite(self.pin, self.duty_percent())?;
        Ok(())
    }

    pub fn step_duty(&mut self, delta_percent: f64) -> Result<(), CmdError> {
        self.set_duty_percent(self.duty_percent + delta_percent)
    }

    pub fn step_frequency(&mut self, delta_hz: i32) -> Result<(), CmdError> {
        let next = if delta_hz.is_negative() {
            self.frequency_hz.saturating_sub(delta_hz.unsigned_abs())
        } else {
            self.frequency_hz.saturating_add(delta_hz as u32)
        };
        self.set_frequency(next.max(1))
    }

    pub fn stop(&self) -> Result<(), CmdError> {
        pwmWrite(self.pin, 0.0)?;
        pinMode(&self.pin.to_string(), PinMode::Input)?;
        Ok(())
    }

    fn clamp_duty(&self, duty_percent: f64) -> f64 {
        duty_percent.clamp(self.min_duty_percent, self.max_duty_percent)
    }

}