use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PinMode {
    Input,
    Output,
    Pwm,
    Analog,
    Interrupt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Pull {
    None,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyState {
    Normal,
    EmergencyStopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControllerState {
    pub connected: bool,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub lb: bool,
    pub rb: bool,
    pub start: bool,
    pub select: bool,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            connected: false,
            lx: 0.0,
            ly: 0.0,
            rx: 0.0,
            ry: 0.0,
            lt: 0.0,
            rt: 0.0,
            a: false,
            b: false,
            x: false,
            y: false,
            lb: false,
            rb: false,
            start: false,
            select: false,
        }
    }
}
