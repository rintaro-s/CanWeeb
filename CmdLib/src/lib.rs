mod backend;
pub mod arduino;
mod command;
mod encoder;
mod error;
pub mod gpio_ext;
mod real_backend;
mod remote_exec;
mod pwm;
mod runtime;
mod types;

#[macro_use]
mod macros;

pub mod prelude;

pub use backend::{Backend, SimBackend};
pub use command::{CommandEnvelope, CommandResult};
pub use encoder::RotaryEncoder;
pub use error::CmdError;
pub use real_backend::RealBackend;
pub use pwm::PwmOutput;
pub use remote_exec::{
	define_child_program, get_child_program, run_child_program, send_child_program_to,
	ChildProgram, ChildProgramReport, ProgramBuilder, ProgramStep,
};
pub use runtime::{dispatch, set_backend, set_backend_arc, use_real_backend, use_sim_backend};
pub use types::{ControllerState, Level, PinMode, Pull, SafetyState};
pub use gpio_ext::{DtOverlay, GpioUltrasonicSensor, GpioRotaryEncoder, GpioRotaryEncoder3Pin, EncDirection, spin_sleep_us, gpio_sysfs_path, gpio_export, gpio_unexport};

pub use serde_json;
