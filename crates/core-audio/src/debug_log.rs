// Debug file logger for CoreAudio HAL plugin.
//
// macOS: writes to /tmp/phaselith/core_audio_debug.log
// Safe for use from coreaudiod context (_coreaudiod user).
// NOT for use on IO thread (uses I/O + mutex).

use std::sync::Mutex;

#[allow(dead_code)]
static LOG_MUTEX: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
const LOG_DIR: &str = "/tmp/phaselith";
#[cfg(target_os = "macos")]
const LOG_PATH: &str = "/tmp/phaselith/core_audio_debug.log";

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
const LOG_DIR: &str = "";
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
const LOG_PATH: &str = "";

#[allow(dead_code)]
pub fn log(msg: &str) {
    if LOG_PATH.is_empty() {
        return;
    }
    let _lock = LOG_MUTEX.lock();
    let _ = std::fs::create_dir_all(LOG_DIR);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        use std::io::Write;
        let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now(), msg);
    }
}

#[macro_export]
macro_rules! ca_log {
    ($($arg:tt)*) => {
        $crate::debug_log::log(&format!($($arg)*))
    };
}
