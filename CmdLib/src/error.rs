use thiserror::Error;

#[derive(Debug, Error)]
pub enum CmdError {
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    #[error("invalid argument `{key}`: {reason}")]
    InvalidArgument { key: String, reason: String },
    #[error("pin not configured: {0}")]
    PinNotConfigured(String),
    #[error("pin mode mismatch: {0}")]
    PinModeMismatch(String),
    #[error("resource not found: {0}")]
    ResourceNotFound(String),
    #[error("safety stop is latched")]
    SafetyStopLatched,
    #[error("child program not found: {0}")]
    ProgramNotFound(String),
    #[error("child program `{program}` failed at step {step_index}: {reason}")]
    ProgramExecutionFailed {
        program: String,
        step_index: usize,
        reason: String,
    },
    #[error("backend error: {0}")]
    Backend(String),
}
