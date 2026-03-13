use crate::config::DspConfig;

/// Port trait: provides configuration to the DSP engine.
///
/// Implemented by:
/// - `MmapConfigSource` in apo-dll (reads from shared memory)
/// - `JsConfigBridge` in wasm-bridge (reads from JS postMessage)
/// - `StaticConfig` in tests (fixed values)
pub trait ConfigSource: Send + Sync {
    /// Get the current configuration.
    fn current_config(&self) -> DspConfig;

    /// Check if config has changed since last read.
    fn has_changed(&self) -> bool;
}

/// Test adapter: fixed configuration that never changes.
pub struct StaticConfig {
    config: DspConfig,
}

impl StaticConfig {
    pub fn new(config: DspConfig) -> Self {
        Self { config }
    }
}

impl ConfigSource for StaticConfig {
    fn current_config(&self) -> DspConfig {
        self.config
    }

    fn has_changed(&self) -> bool {
        false
    }
}
