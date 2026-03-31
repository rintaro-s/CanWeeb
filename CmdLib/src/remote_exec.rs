use crate::runtime::dispatch;
use crate::{CmdError, CommandResult};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramStep {
    pub domain: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildProgram {
    pub name: String,
    pub steps: Vec<ProgramStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildProgramReport {
    pub child_id: String,
    pub program_name: String,
    pub total_steps: usize,
    pub executed_steps: usize,
    pub results: Vec<CommandResult>,
}

#[derive(Debug, Clone, Default)]
pub struct ProgramBuilder {
    steps: Vec<ProgramStep>,
}

impl ProgramBuilder {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn command(&mut self, domain: impl Into<String>, action: impl Into<String>, args: Value) -> &mut Self {
        self.steps.push(ProgramStep {
            domain: domain.into(),
            action: action.into(),
            args,
        });
        self
    }

    pub fn pin_mode(&mut self, pin: impl Into<String>, mode: impl Into<String>) -> &mut Self {
        self.command(
            "gpio",
            "pin_mode",
            json!({
                "pin": pin.into(),
                "mode": mode.into(),
            }),
        )
    }

    pub fn digital_write(&mut self, pin: impl Into<String>, level: impl Into<String>) -> &mut Self {
        self.command(
            "gpio",
            "digital_write",
            json!({
                "pin": pin.into(),
                "level": level.into(),
            }),
        )
    }

    pub fn motor_set_speed(&mut self, motor: impl Into<String>, speed: f64) -> &mut Self {
        self.command(
            "motor",
            "motor_set_speed",
            json!({
                "motor": motor.into(),
                "speed": speed,
            }),
        )
    }

    pub fn servo_write(&mut self, servo: impl Into<String>, angle: f64) -> &mut Self {
        self.command(
            "servo",
            "servo_write",
            json!({
                "servo": servo.into(),
                "angle": angle,
            }),
        )
    }

    pub fn uart_send(&mut self, port: impl Into<String>, data: impl Into<String>) -> &mut Self {
        self.command(
            "uart",
            "uart_send",
            json!({
                "port": port.into(),
                "data": data.into(),
            }),
        )
    }

    pub fn into_steps(self) -> Vec<ProgramStep> {
        self.steps
    }
}

static PROGRAMS: OnceCell<RwLock<HashMap<String, ChildProgram>>> = OnceCell::new();

fn program_store() -> &'static RwLock<HashMap<String, ChildProgram>> {
    PROGRAMS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn define_child_program(
    name: impl Into<String>,
    build: impl FnOnce(&mut ProgramBuilder),
) -> Result<ChildProgram, CmdError> {
    let name = name.into();
    if name.trim().is_empty() {
        return Err(CmdError::InvalidArgument {
            key: "name".to_string(),
            reason: "program name must not be empty".to_string(),
        });
    }

    let mut builder = ProgramBuilder::new();
    build(&mut builder);
    let program = ChildProgram {
        name: name.clone(),
        steps: builder.into_steps(),
    };

    let mut lock = program_store()
        .write()
        .map_err(|_| CmdError::Backend("program registry write lock poisoned".to_string()))?;
    lock.insert(name, program.clone());

    Ok(program)
}

pub fn get_child_program(name: &str) -> Result<ChildProgram, CmdError> {
    let lock = program_store()
        .read()
        .map_err(|_| CmdError::Backend("program registry read lock poisoned".to_string()))?;
    lock.get(name)
        .cloned()
        .ok_or_else(|| CmdError::ProgramNotFound(name.to_string()))
}

pub fn run_child_program(name: &str) -> Result<ChildProgramReport, CmdError> {
    execute_program("local-child", name)
}

pub fn send_child_program_to(child_id: &str, program_name: &str) -> Result<ChildProgramReport, CmdError> {
    if child_id.trim().is_empty() {
        return Err(CmdError::InvalidArgument {
            key: "child_id".to_string(),
            reason: "child_id must not be empty".to_string(),
        });
    }
    execute_program(child_id, program_name)
}

fn execute_program(child_id: &str, program_name: &str) -> Result<ChildProgramReport, CmdError> {
    let program = get_child_program(program_name)?;

    let _ = dispatch(
        "integration",
        "send_child_program",
        json!({
            "child_id": child_id,
            "program": program,
        }),
    )?;

    let mut results = Vec::with_capacity(program.steps.len());
    for (index, step) in program.steps.iter().enumerate() {
        let result = dispatch(&step.domain, &step.action, step.args.clone()).map_err(|err| {
            CmdError::ProgramExecutionFailed {
                program: program.name.clone(),
                step_index: index,
                reason: err.to_string(),
            }
        })?;
        results.push(result);
    }

    Ok(ChildProgramReport {
        child_id: child_id.to_string(),
        program_name: program.name,
        total_steps: program.steps.len(),
        executed_steps: results.len(),
        results,
    })
}
