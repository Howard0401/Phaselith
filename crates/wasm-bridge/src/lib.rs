// ASCE WASM Bridge (CIRRUS Engine)
// Exports C-style functions for AudioWorklet consumption.
// No wasm-bindgen (AudioWorklet lacks TextDecoder/TextEncoder).
//
// Two independent mono engines (L/R) to prevent state bleed across channels.
// The worklet calls process_block_ch(0, len) for left, process_block_ch(1, len)
// for right. Each engine has its own frame counters, smoothers, and history.
//
// Uses UnsafeCell instead of static mut to satisfy Rust's aliasing model.
// Safety: WASM is single-threaded — no data races are possible.

use core::cell::UnsafeCell;

use asce_dsp_core::config::{EngineConfig, PhaseMode, QualityMode};
use asce_dsp_core::engine::CirrusEngineBuilder;
use asce_dsp_core::CirrusEngine;

// ─── Global state container ───
// WASM is single-threaded, so we wrap in UnsafeCell + manual Sync impl.

struct WasmState {
    engine_l: UnsafeCell<Option<CirrusEngine>>,
    engine_r: UnsafeCell<Option<CirrusEngine>>,
    input_buf: UnsafeCell<[f32; 128]>,
    output_buf: UnsafeCell<[f32; 128]>,
}

// Safety: WASM target is single-threaded. No concurrent access is possible.
unsafe impl Sync for WasmState {}

static STATE: WasmState = WasmState {
    engine_l: UnsafeCell::new(None),
    engine_r: UnsafeCell::new(None),
    input_buf: UnsafeCell::new([0.0; 128]),
    output_buf: UnsafeCell::new([0.0; 128]),
};

fn make_config() -> EngineConfig {
    EngineConfig {
        phase_mode: PhaseMode::Minimum, // Low latency for browser
        quality_mode: QualityMode::Standard,
        ..EngineConfig::default()
    }
}

fn build_engine(sample_rate: u32, config: EngineConfig) -> CirrusEngine {
    CirrusEngineBuilder::new(sample_rate, 1024)
        .with_channels(1) // Each engine processes one deinterleaved mono channel
        .with_config(config)
        .build_default()
}

#[no_mangle]
pub extern "C" fn init(sample_rate: f32) {
    let config = make_config();
    let sr = sample_rate as u32;
    unsafe {
        *STATE.engine_l.get() = Some(build_engine(sr, config));
        *STATE.engine_r.get() = Some(build_engine(sr, config));
    }
}

#[no_mangle]
pub extern "C" fn get_input_ptr() -> *mut f32 {
    STATE.input_buf.get() as *mut f32
}

#[no_mangle]
pub extern "C" fn get_output_ptr() -> *const f32 {
    STATE.output_buf.get() as *const f32
}

/// Process a block for the given channel (0=left, 1=right).
/// Worklet must call this once per channel per render quantum.
#[no_mangle]
pub extern "C" fn process_block_ch(channel: u32, len: u32) {
    unsafe {
        let engine_cell = if channel == 0 {
            &STATE.engine_l
        } else {
            &STATE.engine_r
        };
        let input = &*STATE.input_buf.get();
        let output = &mut *STATE.output_buf.get();

        if let Some(engine) = (*engine_cell.get()).as_mut() {
            let l = (len as usize).min(128);
            output[..l].copy_from_slice(&input[..l]);
            engine.process(&mut output[..l]);
        }
    }
}

/// Legacy single-channel process (uses left engine). Kept for backward compat.
#[no_mangle]
pub extern "C" fn process_block(len: u32) {
    process_block_ch(0, len);
}

// ─── Config control exports ───
// All set_* functions update BOTH engines to keep them in sync.

fn with_both_engines(f: impl Fn(&mut CirrusEngine)) {
    unsafe {
        if let Some(engine) = (*STATE.engine_l.get()).as_mut() {
            f(engine);
        }
        if let Some(engine) = (*STATE.engine_r.get()).as_mut() {
            f(engine);
        }
    }
}

fn current_config() -> EngineConfig {
    unsafe {
        (*STATE.engine_l.get())
            .as_ref()
            .map(|e| e.context().config)
            .unwrap_or_else(make_config)
    }
}

#[no_mangle]
pub extern "C" fn set_strength(value: f32) {
    let mut config = current_config();
    config.strength = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_enabled(enabled: u32) {
    let mut config = current_config();
    config.enabled = enabled != 0;
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_hf_reconstruction(value: f32) {
    let mut config = current_config();
    config.hf_reconstruction = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_dynamics(value: f32) {
    let mut config = current_config();
    config.dynamics = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_transient(value: f32) {
    let mut config = current_config();
    config.transient = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}
