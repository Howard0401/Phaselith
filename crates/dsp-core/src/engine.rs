use crate::config::EngineConfig;
use crate::fft::planner::SharedFftPlans;
use crate::module_trait::{PhaselithModule, ProcessContext};
use crate::modules;

/// The CIRRUS engine: M0-M7 processing pipeline.
///
/// Replaces the old 6-stage Pipeline. Each module reads/writes to ProcessContext
/// fields as appropriate. The engine ensures correct execution order.
pub struct PhaselithEngine {
    modules: Vec<Box<dyn PhaselithModule>>,
    context: ProcessContext,
}

impl PhaselithEngine {
    /// Process audio in-place through all modules.
    /// MUST be zero-alloc on the hot path.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.context.config.enabled {
            return; // bypass
        }

        // Save dry copy for M6 safety mixing
        let dry_len = samples.len().min(self.context.dry_buffer.len());
        self.context.dry_buffer[..dry_len].copy_from_slice(&samples[..dry_len]);

        self.context.frame_index += 1;

        // native-rt: skip timing on RT thread (QueryPerformanceCounter can stall)
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "native-rt")))]
        let t0 = std::time::Instant::now();

        for module in &mut self.modules {
            module.process(samples, &mut self.context);
        }

        #[cfg(all(not(target_arch = "wasm32"), not(feature = "native-rt")))]
        {
            self.context.processing_time_us = t0.elapsed().as_micros() as f32;
        }
    }

    /// Update configuration (called when UI changes params).
    pub fn update_config(&mut self, config: EngineConfig) {
        self.context.synthesis_mode = config.synthesis_mode;
        self.context.config = config;
    }

    /// Get current damage posterior (for UI display).
    pub fn damage_posterior(&self) -> &crate::types::DamagePosterior {
        &self.context.damage
    }

    /// Get current context (for inspection/testing).
    pub fn context(&self) -> &ProcessContext {
        &self.context
    }

    /// Get mutable context (for testing).
    pub fn context_mut(&mut self) -> &mut ProcessContext {
        &mut self.context
    }

    /// Reset all modules (e.g., on stream discontinuity).
    pub fn reset(&mut self) {
        self.context.damage.clear();
        self.context.lattice.clear();
        self.context.fields.clear();
        self.context.residual.clear();
        self.context.validated.clear();
        self.context.frame_index = 0;
        for module in &mut self.modules {
            module.reset();
        }
    }

    /// Number of modules in the engine.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Get module name by index.
    pub fn module_name(&self, index: usize) -> Option<&'static str> {
        self.modules.get(index).map(|m| m.name())
    }
}

/// Builder for constructing a PhaselithEngine.
/// This is the composition root — the single place where DI wiring happens.
pub struct PhaselithEngineBuilder {
    sample_rate: u32,
    channels: u16,
    max_frame_size: usize,
    config: EngineConfig,
    modules: Vec<Box<dyn PhaselithModule>>,
}

impl PhaselithEngineBuilder {
    pub fn new(sample_rate: u32, max_frame_size: usize) -> Self {
        Self {
            sample_rate,
            channels: 2,
            max_frame_size,
            config: EngineConfig::default(),
            modules: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }

    /// Add a module to the engine. Modules are executed in insertion order.
    pub fn add_module(mut self, module: Box<dyn PhaselithModule>) -> Self {
        self.modules.push(module);
        self
    }

    /// Build with the default M0-M7 CIRRUS module chain.
    /// Uses a shared FFT plan cache so M2 and M5 reuse the same plans.
    pub fn build_default(self) -> PhaselithEngine {
        let mut plans = SharedFftPlans::new();

        let mut m2 = modules::m2_lattice::TriLatticeAnalysis::new();
        m2.init_with_plans(self.sample_rate, &mut plans);

        let mut m5 = modules::m5_reprojection::SelfReprojectionValidator::new();
        m5.init_with_plans(self.max_frame_size, self.sample_rate, &mut plans);

        self.add_module(Box::new(modules::m0_orchestrator::FrameOrchestrator::new()))
            .add_module(Box::new(modules::m1_damage::DamagePosteriorEngine::new()))
            .add_module(Box::new(m2))
            .add_module(Box::new(modules::m3_factorizer::StructuredFactorizer::new()))
            .add_module(Box::new(modules::m4_solver::InverseResidualSolver::new()))
            .add_module(Box::new(m5))
            .add_module(Box::new(modules::m6_mixer::PerceptualSafetyMixer::new()))
            .add_module(Box::new(modules::m7_governor::QualityGovernor::new()))
            .build()
    }

    /// Build with whatever modules have been added.
    pub fn build(mut self) -> PhaselithEngine {
        let mut context = ProcessContext::new(self.sample_rate, self.channels, self.config);
        context.frame_params = crate::frame::FrameParams::new(
            self.max_frame_size,
            self.sample_rate,
            self.config.quality_mode,
        );
        context.dry_buffer = vec![0.0; self.max_frame_size];

        // native-rt: pre-allocate ALL context fields to max sizes so modules
        // never allocate on the hot path. Without this, M3/M4/M5 allocate on
        // first process() call when they see empty/mismatched field sizes.
        #[cfg(feature = "native-rt")]
        {
            use crate::types::{CORE_FFT_SIZE, StructuredFields, ResidualCandidate, ValidatedResidual};
            let core_bins = CORE_FFT_SIZE / 2 + 1;
            context.fields = StructuredFields::new(core_bins);
            context.residual = ResidualCandidate::new(core_bins);
            context.validated = ValidatedResidual::new(self.max_frame_size);
            context.validated.acceptance_mask = vec![1.0; core_bins];
            context.time_candidate = vec![0.0; self.max_frame_size];
        }

        for module in &mut self.modules {
            module.init(self.max_frame_size, self.sample_rate);
        }

        PhaselithEngine {
            modules: self.modules,
            context,
        }
    }
}

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A module that records when it was called and what context it saw.
    pub struct RecordingModule {
        module_name: &'static str,
        pub call_log: Arc<Mutex<Vec<String>>>,
        pub saw_cutoff_mean: Arc<Mutex<Option<f32>>>,
        /// If set, write this cutoff mean to damage posterior.
        pub write_cutoff_mean: Option<f32>,
    }

    impl RecordingModule {
        pub fn new(name: &'static str, call_log: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                module_name: name,
                call_log,
                saw_cutoff_mean: Arc::new(Mutex::new(None)),
                write_cutoff_mean: None,
            }
        }

        pub fn with_write_cutoff(mut self, cutoff: f32) -> Self {
            self.write_cutoff_mean = Some(cutoff);
            self
        }
    }

    impl PhaselithModule for RecordingModule {
        fn name(&self) -> &'static str {
            self.module_name
        }

        fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {}

        fn process(&mut self, _samples: &mut [f32], ctx: &mut ProcessContext) {
            self.call_log
                .lock()
                .unwrap()
                .push(self.module_name.to_string());

            *self.saw_cutoff_mean.lock().unwrap() = Some(ctx.damage.cutoff.mean);

            if let Some(cutoff) = self.write_cutoff_mean {
                ctx.damage.cutoff.mean = cutoff;
            }
        }

        fn reset(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module_trait::NoOpModule;

    #[test]
    fn engine_bypass_when_disabled() {
        let mut config = EngineConfig::default();
        config.enabled = false;
        let mut engine = PhaselithEngineBuilder::new(48000, 1024)
            .with_config(config)
            .add_module(Box::new(NoOpModule::new("test")))
            .build();

        let mut samples = vec![1.0, 2.0, 3.0];
        engine.process(&mut samples);
        assert_eq!(samples, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn engine_module_count() {
        let engine = PhaselithEngineBuilder::new(48000, 1024)
            .add_module(Box::new(NoOpModule::new("a")))
            .add_module(Box::new(NoOpModule::new("b")))
            .build();
        assert_eq!(engine.module_count(), 2);
    }

    #[test]
    fn engine_module_names() {
        let engine = PhaselithEngineBuilder::new(48000, 1024)
            .add_module(Box::new(NoOpModule::new("M0")))
            .add_module(Box::new(NoOpModule::new("M1")))
            .build();
        assert_eq!(engine.module_name(0), Some("M0"));
        assert_eq!(engine.module_name(1), Some("M1"));
        assert_eq!(engine.module_name(2), None);
    }

    #[test]
    fn engine_reset_clears_state() {
        let mut engine = PhaselithEngineBuilder::new(48000, 1024)
            .add_module(Box::new(NoOpModule::new("test")))
            .build();

        engine.context_mut().damage.cutoff.mean = 8000.0;
        engine.context_mut().frame_index = 100;
        engine.reset();

        assert_eq!(engine.context().damage.cutoff.mean, 20000.0);
        assert_eq!(engine.context().frame_index, 0);
    }

    #[test]
    fn engine_update_config() {
        let mut engine = PhaselithEngineBuilder::new(48000, 1024)
            .add_module(Box::new(NoOpModule::new("test")))
            .build();

        let mut new_config = EngineConfig::default();
        new_config.strength = 0.3;
        engine.update_config(new_config);

        assert!((engine.context().config.strength - 0.3).abs() < f32::EPSILON);
    }
}
