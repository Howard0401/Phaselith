pub mod config;
pub mod types;
pub mod ports;
pub mod frame;
pub mod module_trait;
pub mod modules;
pub mod engine;
pub mod fft;
pub mod psychoacoustic;

// Keep old modules alive during migration (APO/WASM still reference them)
pub mod stages;
pub mod pipeline;

// ─── Phaselith public API ───
pub use config::{EngineConfig, PhaseMode, QualityMode};
pub use engine::{PhaselithEngine, PhaselithEngineBuilder};
pub use module_trait::{PhaselithModule, ProcessContext};
pub use types::{
    DamagePosterior, GaussianEstimate, Lattice, TriLattice,
    StructuredFields, ResidualCandidate, ValidatedResidual, QualityTier,
};
pub use frame::{FrameParams, FrameClock, SynthesisMode};

// ─── Legacy re-exports (backward compat) ───
pub use config::{DspConfig, QualityPreset};
pub use pipeline::{Pipeline, PipelineBuilder};
pub use stages::stage_trait::{DspStage, StageContext};
