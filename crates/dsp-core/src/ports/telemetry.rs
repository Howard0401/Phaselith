use crate::types::DegradationProfile;

/// Optional port trait: telemetry and diagnostics.
///
/// Implemented by:
/// - No-op in apo-dll (real-time thread, can't do I/O)
/// - Console.log bridge in wasm-bridge
/// - RecordingTelemetry in tests
pub trait Telemetry: Send + Sync {
    fn report_latency(&self, _stage: &str, _microseconds: u64) {}
    fn report_degradation(&self, _profile: &DegradationProfile) {}
}

/// No-op telemetry for production real-time contexts.
pub struct NoOpTelemetry;

impl Telemetry for NoOpTelemetry {}
