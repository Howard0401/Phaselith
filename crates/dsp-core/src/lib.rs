pub mod config;
pub mod types;
pub mod ports;
pub mod frame;
pub mod module_trait;
pub mod modules;
pub mod engine;
pub mod fft;

// Keep old modules alive during migration (APO/WASM still reference them)
pub mod stages;
pub mod pipeline;

// ─── CIRRUS public API ───
pub use config::{EngineConfig, PhaseMode, QualityMode};
pub use engine::{CirrusEngine, CirrusEngineBuilder};
pub use module_trait::{CirrusModule, ProcessContext};
pub use types::{
    DamagePosterior, GaussianEstimate, Lattice, TriLattice,
    StructuredFields, ResidualCandidate, ValidatedResidual, QualityTier,
};
pub use frame::{FrameParams, FrameClock, SynthesisMode};

// ─── Legacy re-exports (backward compat) ───
pub use config::{DspConfig, QualityPreset};
pub use pipeline::{Pipeline, PipelineBuilder};
pub use stages::stage_trait::{DspStage, StageContext};
