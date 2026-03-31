// ASCE WASM Bridge (Phaselith Engine)
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

use phaselith_dsp_core::config::{
    EngineConfig, FilterStyle, PhaseMode, QualityMode, StyleConfig, StylePreset, SynthesisMode,
};
use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use phaselith_dsp_core::types::CrossChannelContext;
use phaselith_dsp_core::PhaselithEngine;

// ─── Global state container ───
// WASM is single-threaded, so we wrap in UnsafeCell + manual Sync impl.

struct WasmState {
    engine_l: UnsafeCell<Option<PhaselithEngine>>,
    engine_r: UnsafeCell<Option<PhaselithEngine>>,
    input_buf: UnsafeCell<[f32; 128]>,
    output_buf: UnsafeCell<[f32; 128]>,

    // ─── Cross-channel symmetric one-frame delay ───
    // L dry input is saved when ch==0 is processed, because the shared input_buf
    // will be overwritten when ch==1 arrives.
    input_l_saved: UnsafeCell<[f32; 128]>,
    // Cross-channel context computed at end of current frame (after R processing).
    // Only used as prev for next frame — never read during current frame.
    cross_channel_current: UnsafeCell<Option<CrossChannelContext>>,
    // Cross-channel context from previous frame. Both L and R read this same value
    // within a frame, ensuring symmetric processing.
    cross_channel_prev: UnsafeCell<Option<CrossChannelContext>>,
}

// Safety: WASM target is single-threaded. No concurrent access is possible.
unsafe impl Sync for WasmState {}

static STATE: WasmState = WasmState {
    engine_l: UnsafeCell::new(None),
    engine_r: UnsafeCell::new(None),
    input_buf: UnsafeCell::new([0.0; 128]),
    output_buf: UnsafeCell::new([0.0; 128]),
    input_l_saved: UnsafeCell::new([0.0; 128]),
    cross_channel_current: UnsafeCell::new(None),
    cross_channel_prev: UnsafeCell::new(None),
};

fn make_config() -> EngineConfig {
    EngineConfig {
        phase_mode: PhaseMode::Minimum, // Low latency for browser
        quality_mode: QualityMode::Standard,
        ..EngineConfig::default()
    }
}

fn build_engine(sample_rate: u32, config: EngineConfig, max_sub_block: Option<usize>) -> PhaselithEngine {
    let builder = PhaselithEngineBuilder::new(sample_rate, 1024)
        .with_channels(1) // Each engine processes one deinterleaved mono channel
        .with_config(config);

    let builder = if let Some(size) = max_sub_block {
        builder.with_max_sub_block(size)
    } else {
        builder
    };

    builder.build_default()
}

#[no_mangle]
pub extern "C" fn init(sample_rate: f32) {
    init_with_sub_block(sample_rate, 0);
}

#[no_mangle]
pub extern "C" fn init_with_sub_block(sample_rate: f32, max_sub_block: u32) {
    let config = make_config();
    let sr = sample_rate as u32;
    let max_sub_block = (max_sub_block > 0).then_some(max_sub_block as usize);
    unsafe {
        *STATE.engine_l.get() = Some(build_engine(sr, config, max_sub_block));
        *STATE.engine_r.get() = Some(build_engine(sr, config, max_sub_block));
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
///
/// Symmetric one-frame delay for cross-channel analysis:
///   ch==0 (L): save dry input → inject cross_channel_prev → process L
///   ch==1 (R): inject cross_channel_prev → process R → compute new cross_channel → rotate
///
/// Both L and R read the SAME cross_channel_prev within a frame.
/// cross_channel_prev only updates AFTER R processing completes.
/// First frame: cross_channel_prev = None → engines fall back to non-stereo path.
#[no_mangle]
pub extern "C" fn process_block_ch(channel: u32, len: u32) {
    unsafe {
        let l = (len as usize).min(128);
        let input = &*STATE.input_buf.get();
        let output = &mut *STATE.output_buf.get();

        let engine_cell = if channel == 0 {
            &STATE.engine_l
        } else {
            &STATE.engine_r
        };

        if let Some(engine) = (*engine_cell.get()).as_mut() {
            if channel == 0 {
                // Step 1: Save L dry input (input_buf will be overwritten by R)
                let saved = &mut *STATE.input_l_saved.get();
                saved[..l].copy_from_slice(&input[..l]);

                // Step 2: Inject previous frame's cross-channel context
                engine.context_mut().cross_channel = *STATE.cross_channel_prev.get();

                // Step 3: Process L
                output[..l].copy_from_slice(&input[..l]);
                engine.process(&mut output[..l]);
            } else {
                // Step 1: Inject same previous frame's cross-channel context (symmetric)
                engine.context_mut().cross_channel = *STATE.cross_channel_prev.get();

                // Step 2: Process R
                output[..l].copy_from_slice(&input[..l]);
                engine.process(&mut output[..l]);

                // Step 3: Compute cross-channel from dry L (saved) + dry R (input_buf)
                // This happens AFTER R processing — not used until next frame
                let saved_l = &*STATE.input_l_saved.get();
                let cc = CrossChannelContext::from_lr(&saved_l[..l], &input[..l]);
                *STATE.cross_channel_current.get() = Some(cc);

                // Step 4: Rotate: current → prev (for next frame)
                *STATE.cross_channel_prev.get() = *STATE.cross_channel_current.get();
            }
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

fn with_both_engines(f: impl Fn(&mut PhaselithEngine)) {
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

#[no_mangle]
pub extern "C" fn set_pre_echo_transient_scaling(value: f32) {
    let mut config = current_config();
    config.pre_echo_transient_scaling = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_declip_transient_scaling(value: f32) {
    let mut config = current_config();
    config.declip_transient_scaling = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_delayed_transient_repair(enabled: u32) {
    let mut config = current_config();
    config.delayed_transient_repair = enabled != 0;
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_body_pass_enabled(enabled: u32) {
    let mut config = current_config();
    config.body_pass_enabled = enabled != 0;
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_hf_tame(value: f32) {
    let mut config = current_config();
    config.hf_tame = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_air_continuity(value: f32) {
    let mut config = current_config();
    config.air_continuity = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

// ─── Style / Character exports ───

/// Set style preset by index:
/// 0=Reference, 1=Grand, 2=Smooth, 3=Vocal, 4=Punch, 5=Air, 6=Night
#[no_mangle]
pub extern "C" fn set_style(preset_index: u32) {
    let preset = match preset_index {
        0 => StylePreset::Reference,
        1 => StylePreset::Grand,
        2 => StylePreset::Smooth,
        3 => StylePreset::Vocal,
        4 => StylePreset::Punch,
        5 => StylePreset::Air,
        6 => StylePreset::Night,
        _ => StylePreset::Reference,
    };
    let mut config = current_config();
    config.style = StyleConfig::from_preset(preset);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_warmth(value: f32) {
    let mut config = current_config();
    config.style.warmth = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_air_brightness(value: f32) {
    let mut config = current_config();
    config.style.air_brightness = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_smoothness(value: f32) {
    let mut config = current_config();
    config.style.smoothness = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_spatial_spread(value: f32) {
    let mut config = current_config();
    config.style.spatial_spread = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_impact_gain(value: f32) {
    let mut config = current_config();
    config.style.impact_gain = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_body(value: f32) {
    let mut config = current_config();
    config.style.body = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

/// Set filter style: 0=Reference, 1=Warm, 2=BassPlus
/// Updates both filter_style and the derived StyleConfig axes.
#[no_mangle]
pub extern "C" fn set_filter_style(style: u32) {
    let fs = FilterStyle::from_u32(style);
    let mut config = current_config();
    config.filter_style = fs;
    config.style = fs.to_style_config();
    with_both_engines(|e| e.update_config(config));
}

/// Set synthesis mode: 0=LegacyAdditive, 1=FftOlaPilot, 2=FftOlaFull
#[no_mangle]
pub extern "C" fn set_synthesis_mode(mode: u32) {
    let mut config = current_config();
    config.synthesis_mode = SynthesisMode::from_u32(mode);
    with_both_engines(|e| e.update_config(config));
}

/// Set ambience preserve (tail compensation): 0.0-1.0.
/// Compensates dereverb side-effect of M5 reprojection.
/// Recommended range: 0.0-0.15 for subtle compensation.
#[no_mangle]
pub extern "C" fn set_ambience_preserve(value: f32) {
    let mut config = current_config();
    config.ambience_preserve = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}

#[no_mangle]
pub extern "C" fn set_ambience_glue(value: f32) {
    let mut config = current_config();
    config.ambience_glue = value.clamp(0.0, 1.0);
    with_both_engines(|e| e.update_config(config));
}
