use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

type LogHook = Arc<dyn Fn(LogLevel, &str) + Send + Sync + 'static>;

static LOGGER_HOOK: OnceLock<Mutex<Option<LogHook>>> = OnceLock::new();
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

fn hook_slot() -> &'static Mutex<Option<LogHook>> {
    LOGGER_HOOK.get_or_init(|| Mutex::new(None))
}

pub fn set_log_hook<F>(hook: F)
where
    F: Fn(LogLevel, &str) + Send + Sync + 'static,
{
    if let Ok(mut guard) = hook_slot().lock() {
        *guard = Some(Arc::new(hook));
    }
}

pub fn clear_log_hook() {
    if let Ok(mut guard) = hook_slot().lock() {
        *guard = None;
    }
}

pub fn set_debug(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn info(message: &str) {
    dispatch(LogLevel::Info, message);
}

pub fn debug(message: &str) {
    if DEBUG_ENABLED.load(Ordering::SeqCst) {
        dispatch(LogLevel::Debug, message);
    }
}

pub fn warn(message: &str) {
    dispatch(LogLevel::Warn, message);
}

pub fn error(message: &str) {
    dispatch(LogLevel::Error, message);
}

pub fn log_info(message: &str) {
    info(message);
}

pub fn log_debug(message: &str) {
    debug(message);
}

pub fn log_warn(message: &str) {
    warn(message);
}

pub fn log_error(message: &str) {
    error(message);
}

fn dispatch(level: LogLevel, message: &str) {
    let prefixed = format!("[IngameIME-Rust-Core] {message}");

    if let Ok(guard) = hook_slot().lock()
        && let Some(hook) = guard.as_ref()
    {
        hook(level, &prefixed);
        return;
    }

    eprintln!("{prefixed}");
}
