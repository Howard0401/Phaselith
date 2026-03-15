// Debug file logger for APO DLL.
// Writes to C:\ProgramData\Phaselith\apo_debug.log
// Safe for use from audiodg.exe (LocalService account).
// NOT for use in real-time audio thread (uses I/O + mutex).

use std::sync::Mutex;
use std::io::Write;

static LOG_MUTEX: Mutex<()> = Mutex::new(());

const LOG_DIR: &str = "C:\\ProgramData\\Phaselith";
const LOG_PATH: &str = "C:\\ProgramData\\Phaselith\\apo_debug.log";

pub fn log(msg: &str) {
    let _lock = LOG_MUTEX.lock();
    // Create directory if needed
    let _ = std::fs::create_dir_all(LOG_DIR);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now(), msg);
    }
}

/// Log with format args
#[macro_export]
macro_rules! apo_log {
    ($($arg:tt)*) => {
        $crate::debug_log::log(&format!($($arg)*))
    };
}
