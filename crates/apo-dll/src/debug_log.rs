// Debug file logger for APO DLL.
// Writes to C:\ProgramData\Phaselith\apo_debug.log
// Safe for use from audiodg.exe (LocalService account).
//
// RT-path logging: apo_log! calls from process() are guarded by rare conditions
// (startup frames, DPC spikes, click gate). The logger keeps the file handle
// open after first use to avoid repeated open/close syscalls. The Mutex is
// still a concern on RT threads, but since these events are rare (< 1/sec)
// and the critical section is tiny (one writeln), the practical impact is
// negligible compared to the DPC spike that triggered the log.

use std::sync::Mutex;
use std::io::Write;
use std::fs::File;

struct LogState {
    file: Option<File>,
    init_attempted: bool,
}

static LOG_STATE: Mutex<LogState> = Mutex::new(LogState {
    file: None,
    init_attempted: false,
});

const LOG_DIR: &str = "C:\\ProgramData\\Phaselith";
const LOG_PATH: &str = "C:\\ProgramData\\Phaselith\\apo_debug.log";

pub fn log(msg: &str) {
    let Ok(mut state) = LOG_STATE.try_lock() else {
        // If lock is contended (shouldn't happen with single EFX instance),
        // drop the log line rather than block the RT thread.
        return;
    };

    // Lazy-init: create dir and open file once
    if !state.init_attempted {
        state.init_attempted = true;
        let _ = std::fs::create_dir_all(LOG_DIR);
        state.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(LOG_PATH)
            .ok();
    }

    if let Some(ref mut f) = state.file {
        let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now(), msg);
        // Flush periodically is handled by OS — no explicit flush on RT path
    }
}

/// Log with format args.
/// NOTE: format!() allocates on the heap. This is acceptable for rare events
/// (DPC spikes, startup diag, click gate) but should not be called on every frame.
#[macro_export]
macro_rules! apo_log {
    ($($arg:tt)*) => {
        $crate::debug_log::log(&format!($($arg)*))
    };
}
