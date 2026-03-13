use crate::config::DspConfig;
use crate::stages::stage_trait::{DspStage, StageContext};

/// The 6-stage DSP pipeline using dynamic dispatch.
/// Stages can be replaced for testing (inject mocks, recording stages, no-ops).
///
/// This is both the production pipeline and the test pipeline.
/// Dynamic dispatch overhead (~2ns per virtual call) is negligible
/// compared to FFT processing time (~1ms).
pub struct Pipeline {
    stages: Vec<Box<dyn DspStage>>,
    context: StageContext,
}

impl Pipeline {
    /// Process audio in-place through all 6 stages.
    /// MUST be zero-alloc on the hot path (all allocation done in new/init).
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.context.config.enabled {
            return; // bypass
        }
        for stage in &mut self.stages {
            stage.process(samples, &mut self.context);
        }
    }

    /// Update the configuration (called when UI changes params).
    pub fn update_config(&mut self, config: DspConfig) {
        self.context.config = config;
        self.context.fft_size = config.quality_mode.fft_size();
    }

    /// Get current degradation profile (for UI display).
    pub fn degradation(&self) -> &crate::types::DegradationProfile {
        &self.context.degradation
    }

    /// Get current context (for inspection/testing).
    pub fn context(&self) -> &StageContext {
        &self.context
    }

    /// Reset all stages (e.g., on stream discontinuity).
    pub fn reset(&mut self) {
        self.context.degradation = Default::default();
        self.context.harmonic_map.clear();
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    /// Number of stages in the pipeline.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Get stage name by index.
    pub fn stage_name(&self, index: usize) -> Option<&'static str> {
        self.stages.get(index).map(|s| s.name())
    }
}

/// Builder for constructing a Pipeline.
/// This is the **composition root** — the single place where DI wiring happens.
pub struct PipelineBuilder {
    sample_rate: u32,
    channels: u16,
    max_frame_size: usize,
    config: DspConfig,
    stages: Vec<Box<dyn DspStage>>,
}

impl PipelineBuilder {
    pub fn new(sample_rate: u32, max_frame_size: usize) -> Self {
        Self {
            sample_rate,
            channels: 2,
            max_frame_size,
            config: DspConfig::default(),
            stages: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: DspConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }

    /// Add a stage to the pipeline. Stages are executed in insertion order.
    pub fn add_stage(mut self, stage: Box<dyn DspStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Build the pipeline with the default 6-stage configuration.
    /// Each stage is initialized with the configured max_frame_size and sample_rate.
    pub fn build_default(self) -> Pipeline {
        use crate::stages::{
            s0_fingerprint::FingerprintDetector,
            s1_dynamics::DynamicsRestorer,
            s2_harmonics::HarmonicTracker,
            s3_spectral::SpectralReconstructor,
            s4_transient::TransientRepairer,
            s5_phase::PhaseCoherence,
        };

        self.add_stage(Box::new(FingerprintDetector::new()))
            .add_stage(Box::new(DynamicsRestorer::new()))
            .add_stage(Box::new(HarmonicTracker::new()))
            .add_stage(Box::new(SpectralReconstructor::new()))
            .add_stage(Box::new(TransientRepairer::new()))
            .add_stage(Box::new(PhaseCoherence::new()))
            .build()
    }

    /// Build the pipeline with whatever stages have been added.
    pub fn build(mut self) -> Pipeline {
        let context = StageContext::new(self.sample_rate, self.channels, self.config);
        for stage in &mut self.stages {
            stage.init(self.max_frame_size, self.sample_rate);
        }
        Pipeline {
            stages: self.stages,
            context,
        }
    }
}

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A stage that records when it was called and what context it saw.
    /// Used in wiring tests to verify stage ordering and context propagation.
    pub struct RecordingStage {
        stage_name: &'static str,
        pub call_log: Arc<Mutex<Vec<String>>>,
        pub saw_cutoff: Arc<Mutex<Option<Option<f32>>>>,
        pub saw_tracks_count: Arc<Mutex<Option<usize>>>,
        /// If set, this stage will write a cutoff value to the degradation profile.
        pub write_cutoff: Option<f32>,
        /// If set, this stage will write a clipping severity to the profile.
        pub write_clipping: Option<f32>,
    }

    impl RecordingStage {
        pub fn new(name: &'static str, call_log: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                stage_name: name,
                call_log,
                saw_cutoff: Arc::new(Mutex::new(None)),
                saw_tracks_count: Arc::new(Mutex::new(None)),
                write_cutoff: None,
                write_clipping: None,
            }
        }

        pub fn with_write_cutoff(mut self, cutoff: f32) -> Self {
            self.write_cutoff = Some(cutoff);
            self
        }

        pub fn with_write_clipping(mut self, clipping: f32) -> Self {
            self.write_clipping = Some(clipping);
            self
        }
    }

    impl DspStage for RecordingStage {
        fn name(&self) -> &'static str {
            self.stage_name
        }

        fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {}

        fn process(&mut self, _samples: &mut [f32], ctx: &mut StageContext) {
            self.call_log
                .lock()
                .unwrap()
                .push(self.stage_name.to_string());

            // Record what we saw
            *self.saw_cutoff.lock().unwrap() = Some(ctx.degradation.cutoff_freq);
            *self.saw_tracks_count.lock().unwrap() = Some(ctx.harmonic_map.tracks.len());

            // Write if configured
            if let Some(cutoff) = self.write_cutoff {
                ctx.degradation.cutoff_freq = Some(cutoff);
            }
            if let Some(clipping) = self.write_clipping {
                ctx.degradation.clipping_severity = clipping;
            }
        }

        fn reset(&mut self) {}
    }
}
