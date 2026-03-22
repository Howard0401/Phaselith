// Phaselith VST3 Plugin
// Wraps the same DSP core used by APO and Chrome Extension.
// Dual mono engines (L/R) with symmetric cross-channel processing.

use nih_plug::formatters::{s2v_f32_percentage, v2s_f32_percentage};
use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

use phaselith_dsp_core::config::{EngineConfig, FilterStyle, PhaseMode, QualityMode};
use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use phaselith_dsp_core::types::CrossChannelContext;
use phaselith_dsp_core::PhaselithEngine;
use phaselith_license::{clamp_config, FreeLicense, LicenseProvider, Platform};

// ─── Constants ───

const PRIME_FRAMES: usize = 32;
const MAX_BLOCK_SIZE: usize = 8192;

// ─── Parameters ───

#[derive(Params)]
struct PhaselithParams {
    #[id = "strength"]
    strength: FloatParam,

    #[id = "hf_reconstruction"]
    hf_reconstruction: FloatParam,

    #[id = "dynamics"]
    dynamics: FloatParam,

    #[id = "transient"]
    transient: FloatParam,

    #[id = "ambience"]
    ambience_preserve: FloatParam,

    #[id = "style"]
    style: EnumParam<StyleParam>,

    #[id = "quality"]
    quality: EnumParam<QualityParam>,

    #[id = "enabled"]
    enabled: BoolParam,
}

// ─── Enum wrappers for nih-plug ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
enum StyleParam {
    #[name = "Reference"]
    Reference,
    #[name = "Warm"]
    Warm,
    #[name = "Bass+"]
    BassPlus,
}

impl StyleParam {
    fn to_filter_style(self) -> FilterStyle {
        match self {
            StyleParam::Reference => FilterStyle::Reference,
            StyleParam::Warm => FilterStyle::Warm,
            StyleParam::BassPlus => FilterStyle::BassPlus,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
enum QualityParam {
    #[name = "Light"]
    Light,
    #[name = "Standard"]
    Standard,
    #[name = "Ultra"]
    Ultra,
    #[name = "Extreme"]
    Extreme,
}

impl QualityParam {
    fn to_quality_mode(self) -> QualityMode {
        match self {
            QualityParam::Light => QualityMode::Light,
            QualityParam::Standard => QualityMode::Standard,
            QualityParam::Ultra => QualityMode::Ultra,
            QualityParam::Extreme => QualityMode::Extreme,
        }
    }
}

impl Default for PhaselithParams {
    fn default() -> Self {
        Self {
            strength: FloatParam::new(
                "Strength",
                0.7,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(v2s_f32_percentage(0))
            .with_string_to_value(s2v_f32_percentage()),

            hf_reconstruction: FloatParam::new(
                "HF Reconstruction",
                0.8,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(v2s_f32_percentage(0))
            .with_string_to_value(s2v_f32_percentage()),

            dynamics: FloatParam::new(
                "Dynamics",
                0.6,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(v2s_f32_percentage(0))
            .with_string_to_value(s2v_f32_percentage()),

            transient: FloatParam::new(
                "Transient",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(v2s_f32_percentage(0))
            .with_string_to_value(s2v_f32_percentage()),

            ambience_preserve: FloatParam::new(
                "Ambience",
                0.0,
                FloatRange::Linear { min: 0.0, max: 0.3 },
            )
            .with_unit(" %")
            .with_value_to_string(v2s_f32_percentage(0))
            .with_string_to_value(s2v_f32_percentage()),

            style: EnumParam::new("Style", StyleParam::Reference),

            quality: EnumParam::new("Quality", QualityParam::Standard),

            enabled: BoolParam::new("Enabled", true),
        }
    }
}

// ─── Plugin ───

struct PhaselithPlugin {
    params: Arc<PhaselithParams>,
    engine_l: Option<PhaselithEngine>,
    engine_r: Option<PhaselithEngine>,
    sample_rate: f32,
    license: Box<dyn LicenseProvider>,

    // Scratch buffers for deinterleaved processing
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    dry_l: Vec<f32>,

    // Cross-channel symmetric one-frame delay
    cross_channel_current: Option<CrossChannelContext>,
    cross_channel_prev: Option<CrossChannelContext>,
}

impl Default for PhaselithPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(PhaselithParams::default()),
            engine_l: None,
            engine_r: None,
            sample_rate: 48000.0,
            license: Box::new(FreeLicense::new(Platform::Vst3)),

            buf_l: vec![0.0; MAX_BLOCK_SIZE],
            buf_r: vec![0.0; MAX_BLOCK_SIZE],
            dry_l: vec![0.0; MAX_BLOCK_SIZE],

            cross_channel_current: None,
            cross_channel_prev: None,
        }
    }
}

fn build_engine(sample_rate: u32, config: EngineConfig) -> PhaselithEngine {
    PhaselithEngineBuilder::new(sample_rate, MAX_BLOCK_SIZE)
        .with_channels(1) // mono — each channel gets its own engine
        .with_config(config)
        .with_max_sub_block(1) // per-sample OLA readout
        .build_default()
}

fn prime_engine(engine: &mut PhaselithEngine) {
    let mut silence = vec![0.0f32; 128];
    for _ in 0..PRIME_FRAMES {
        engine.process(&mut silence);
    }
}

impl PhaselithPlugin {
    fn sync_config(&self) -> EngineConfig {
        let mut config = EngineConfig {
            strength: self.params.strength.value(),
            hf_reconstruction: self.params.hf_reconstruction.value(),
            dynamics: self.params.dynamics.value(),
            transient: self.params.transient.value(),
            ambience_preserve: self.params.ambience_preserve.value(),
            filter_style: self.params.style.value().to_filter_style(),
            quality_mode: self.params.quality.value().to_quality_mode(),
            enabled: self.params.enabled.value(),
            phase_mode: PhaseMode::Linear, // DAW: latency is acceptable
            style: self.params.style.value().to_filter_style().to_style_config(),
            ..EngineConfig::default()
        };

        // License enforcement
        clamp_config(&mut config, self.license.as_ref());

        config
    }
}

impl Plugin for PhaselithPlugin {
    const NAME: &'static str = "Phaselith";
    const VENDOR: &'static str = "Phaselith Audio";
    const URL: &'static str = "https://phaselith.com";
    const EMAIL: &'static str = "chenhoward0401@gmail.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: Some(NonZeroU32::new(2).unwrap()),
        main_output_channels: Some(NonZeroU32::new(2).unwrap()),
        ..AudioIOLayout::const_default()
    }];

    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        let sr = self.sample_rate as u32;
        let config = self.sync_config();

        // Build dual mono engines
        let mut engine_l = build_engine(sr, config);
        let mut engine_r = build_engine(sr, config);

        // Prime to warm OLA buffers
        prime_engine(&mut engine_l);
        prime_engine(&mut engine_r);

        self.engine_l = Some(engine_l);
        self.engine_r = Some(engine_r);

        // Resize scratch buffers
        let max_samples = buffer_config.max_buffer_size as usize;
        self.buf_l.resize(max_samples, 0.0);
        self.buf_r.resize(max_samples, 0.0);
        self.dry_l.resize(max_samples, 0.0);

        self.cross_channel_current = None;
        self.cross_channel_prev = None;

        true
    }

    fn reset(&mut self) {
        if let Some(ref mut engine) = self.engine_l {
            engine.reset();
            prime_engine(engine);
        }
        if let Some(ref mut engine) = self.engine_r {
            engine.reset();
            prime_engine(engine);
        }
        self.cross_channel_current = None;
        self.cross_channel_prev = None;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Sync params → config (before borrowing engines)
        let config = self.sync_config();

        let (engine_l, engine_r) = match (self.engine_l.as_mut(), self.engine_r.as_mut()) {
            (Some(l), Some(r)) => (l, r),
            _ => return ProcessStatus::Normal,
        };
        engine_l.update_config(config);
        engine_r.update_config(config);

        if !config.enabled {
            return ProcessStatus::Normal;
        }

        let num_samples = buffer.samples();
        if num_samples == 0 {
            return ProcessStatus::Normal;
        }

        // Deinterleave: host buffer → scratch buffers
        let channel_slices = buffer.as_slice();
        self.buf_l[..num_samples].copy_from_slice(&channel_slices[0][..num_samples]);
        if channel_slices.len() > 1 {
            self.buf_r[..num_samples].copy_from_slice(&channel_slices[1][..num_samples]);
        } else {
            // Mono input → duplicate to R
            self.buf_r[..num_samples].copy_from_slice(&self.buf_l[..num_samples]);
        }

        // Save dry L for cross-channel computation
        self.dry_l[..num_samples].copy_from_slice(&self.buf_l[..num_samples]);

        // Inject cross-channel context from previous frame
        if let Some(ref cc) = self.cross_channel_prev {
            engine_l.context_mut().cross_channel = Some(cc.clone());
            engine_r.context_mut().cross_channel = Some(cc.clone());
        }

        // Process L
        engine_l.process(&mut self.buf_l[..num_samples]);

        // Process R
        engine_r.process(&mut self.buf_r[..num_samples]);

        // Compute cross-channel context for next frame
        let new_cc = CrossChannelContext::from_lr(
            &self.dry_l[..num_samples],
            &self.buf_r[..num_samples],
        );
        self.cross_channel_prev = self.cross_channel_current.take();
        self.cross_channel_current = Some(new_cc);

        // Write back to host buffer
        let channel_slices = buffer.as_slice();
        channel_slices[0][..num_samples].copy_from_slice(&self.buf_l[..num_samples]);
        if channel_slices.len() > 1 {
            channel_slices[1][..num_samples].copy_from_slice(&self.buf_r[..num_samples]);
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for PhaselithPlugin {
    const CLAP_ID: &'static str = "com.phaselith.studio";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Real-time audio restoration — spectral repair with self-reprojection validation");
    const CLAP_MANUAL_URL: Option<&'static str> = Some("https://phaselith.com");
    const CLAP_SUPPORT_URL: Option<&'static str> = Some("https://phaselith.com");
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Mastering,
        ClapFeature::Restoration,
    ];
}

impl Vst3Plugin for PhaselithPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"PhaselithVST3001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Mastering,
        Vst3SubCategory::Restoration,
    ];
}

nih_export_clap!(PhaselithPlugin);
nih_export_vst3!(PhaselithPlugin);
