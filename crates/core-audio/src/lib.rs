// Phaselith CoreAudio HAL Plugin
//
// macOS AudioServerPlugIn (.driver bundle) loaded by coreaudiod.
// Processes system audio through the same DSP pipeline as the Windows APO.
//
// Loading path (macOS):
//   Install to /Library/Audio/Plug-Ins/HAL/PhaselithAudio.driver/
//   coreaudiod → CFPlugIn → phaselith_driver_factory() → DriverInstance
//   IO thread → do_io_operation() (real-time, zero-alloc)
//
// All macOS-specific code is behind #[cfg(target_os = "macos")].
// Cross-platform code (io_engine, mmap structs, constants) compiles on all targets.
//
// Many items are only used on macOS — suppress dead_code warnings on other platforms.
#![allow(dead_code)]

#[macro_use]
mod debug_log;
pub mod constants;
pub mod io_engine;
pub mod mmap_ipc;
pub mod object_model;
pub mod properties;

#[cfg(target_os = "macos")]
pub mod plugin_interface;
