use crate::backend::{Backend, SimBackend};
use crate::command::{CommandEnvelope, CommandResult};
use crate::error::CmdError;
use crate::real_backend::RealBackend;
use once_cell::sync::OnceCell;
use serde_json::Value;
use std::sync::{Arc, RwLock};

static BACKEND: OnceCell<RwLock<Arc<dyn Backend>>> = OnceCell::new();

fn backend_cell() -> &'static RwLock<Arc<dyn Backend>> {
    BACKEND.get_or_init(|| RwLock::new(Arc::new(SimBackend::new())))
}

pub fn set_backend<B>(backend: B) -> Result<(), CmdError>
where
    B: Backend + 'static,
{
    set_backend_arc(Arc::new(backend))
}

pub fn set_backend_arc(backend: Arc<dyn Backend>) -> Result<(), CmdError> {
    let mut lock = backend_cell()
        .write()
        .map_err(|_| CmdError::Backend("backend write lock poisoned".to_string()))?;
    *lock = backend;
    Ok(())
}

pub fn use_sim_backend() -> Result<(), CmdError> {
    set_backend(SimBackend::new())
}

pub fn use_real_backend() -> Result<(), CmdError> {
    set_backend(RealBackend::new()?)
}

pub fn dispatch(domain: &str, action: &str, args: Value) -> Result<CommandResult, CmdError> {
    if domain.trim().is_empty() {
        return Err(CmdError::InvalidCommand("domain must not be empty".to_string()));
    }
    if action.trim().is_empty() {
        return Err(CmdError::InvalidCommand("action must not be empty".to_string()));
    }

    let backend = {
        let lock = backend_cell()
            .read()
            .map_err(|_| CmdError::Backend("backend read lock poisoned".to_string()))?;
        Arc::clone(&lock)
    };

    let command = CommandEnvelope::new(domain, action, args);
    backend.execute(command)
}
