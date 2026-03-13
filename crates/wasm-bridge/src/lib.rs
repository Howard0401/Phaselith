// ASCE WASM Bridge (CIRRUS Engine)
// Exports C-style functions for AudioWorklet consumption.
// No wasm-bindgen (AudioWorklet lacks TextDecoder/TextEncoder).

use asce_dsp_core::config::{EngineConfig, PhaseMode, QualityMode};
use asce_dsp_core::engine::CirrusEngineBuilder;
use asce_dsp_core::CirrusEngine;

static mut ENGINE: Option<CirrusEngine> = None;
static mut INPUT_BUF: [f32; 128] = [0.0; 128];
static mut OUTPUT_BUF: [f32; 128] = [0.0; 128];

#[no_mangle]
pub extern "C" fn init(sample_rate: f32) {
    let config = EngineConfig {
        phase_mode: PhaseMode::Minimum, // Low latency for browser
        quality_mode: QualityMode::Standard,
        ..EngineConfig::default()
    };

    let engine = CirrusEngineBuilder::new(sample_rate as u32, 1024)
        .with_config(config)
        .build_default();

    unsafe {
        ENGINE = Some(engine);
    }
}

#[no_mangle]
pub extern "C" fn get_input_ptr() -> *mut f32 {
    unsafe { INPUT_BUF.as_mut_ptr() }
}

#[no_mangle]
pub extern "C" fn get_output_ptr() -> *const f32 {
    unsafe { OUTPUT_BUF.as_ptr() }
}

#[no_mangle]
pub extern "C" fn process_block(len: u32) {
    unsafe {
        if let Some(engine) = ENGINE.as_mut() {
            let l = (len as usize).min(128);
            OUTPUT_BUF[..l].copy_from_slice(&INPUT_BUF[..l]);
            engine.process(&mut OUTPUT_BUF[..l]);
        }
    }
}

#[no_mangle]
pub extern "C" fn set_strength(value: f32) {
    unsafe {
        if let Some(engine) = ENGINE.as_mut() {
            let mut config = EngineConfig::default();
            config.strength = value.clamp(0.0, 1.0);
            engine.update_config(config);
        }
    }
}

#[no_mangle]
pub extern "C" fn set_enabled(enabled: u32) {
    unsafe {
        if let Some(engine) = ENGINE.as_mut() {
            let mut config = EngineConfig::default();
            config.enabled = enabled != 0;
            engine.update_config(config);
        }
    }
}
