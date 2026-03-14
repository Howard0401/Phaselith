use crate::config::QualityMode;
use crate::module_trait::{CirrusModule, ProcessContext};

/// M7: Quality Governor & Telemetry.
///
/// Monitors processing load and adjusts quality mode dynamically.
/// Also collects telemetry snapshots for the UI.
pub struct QualityGovernor {
    /// Rolling average of processing time (microseconds).
    avg_process_time_us: f32,
    /// Frame counter for periodic checks.
    frame_counter: u64,
    /// Suggested quality mode (may differ from current).
    suggested_mode: QualityMode,
    /// Last telemetry snapshot.
    pub last_snapshot: TelemetrySnapshot,
}

/// Telemetry snapshot for the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct TelemetrySnapshot {
    /// Current processing load (0-100%).
    pub processing_load: f32,
    /// Current cutoff frequency estimate.
    pub cutoff_freq: f32,
    /// Current consistency score from M5.
    pub consistency_score: f32,
    /// Current quality mode.
    pub quality_mode: u8,
    /// Frame count.
    pub frame_count: u64,
}

impl QualityGovernor {
    pub fn new() -> Self {
        Self {
            avg_process_time_us: 0.0,
            frame_counter: 0,
            suggested_mode: QualityMode::Standard,
            last_snapshot: TelemetrySnapshot::default(),
        }
    }

    /// Check if quality mode should be adjusted.
    pub fn suggested_quality_mode(&self) -> QualityMode {
        self.suggested_mode
    }
}

impl CirrusModule for QualityGovernor {
    fn name(&self) -> &'static str {
        "M7:Governor"
    }

    fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {}

    fn process(&mut self, _samples: &mut [f32], ctx: &mut ProcessContext) {
        self.frame_counter += 1;

        // Update rolling average of processing time (EMA, α=0.05)
        if ctx.processing_time_us > 0.0 {
            let alpha = 0.05f32;
            self.avg_process_time_us =
                self.avg_process_time_us * (1.0 - alpha) + ctx.processing_time_us * alpha;
        }

        // Update telemetry snapshot
        self.last_snapshot = TelemetrySnapshot {
            processing_load: self.avg_process_time_us,
            cutoff_freq: ctx.damage.cutoff.mean,
            consistency_score: ctx.validated.consistency_score,
            quality_mode: match ctx.config.quality_mode {
                QualityMode::Light => 0,
                QualityMode::Standard => 1,
                QualityMode::Ultra => 2,
            },
            frame_count: ctx.frame_index,
        };

        // Quality mode auto-adjustment (every 100 frames)
        if self.frame_counter % 100 == 0 {
            self.suggested_mode = if self.avg_process_time_us > 20000.0 {
                // Too slow, downgrade
                match ctx.config.quality_mode {
                    QualityMode::Ultra => QualityMode::Standard,
                    QualityMode::Standard => QualityMode::Light,
                    QualityMode::Light => QualityMode::Light,
                }
            } else if self.avg_process_time_us < 5000.0 {
                // Headroom available, can upgrade
                match ctx.config.quality_mode {
                    QualityMode::Light => QualityMode::Standard,
                    QualityMode::Standard => QualityMode::Ultra,
                    QualityMode::Ultra => QualityMode::Ultra,
                }
            } else {
                ctx.config.quality_mode
            };
        }
    }

    fn reset(&mut self) {
        self.avg_process_time_us = 0.0;
        self.frame_counter = 0;
        self.last_snapshot = TelemetrySnapshot::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    #[test]
    fn governor_initializes() {
        let mut m7 = QualityGovernor::new();
        m7.init(1024, 48000);
        assert_eq!(m7.frame_counter, 0);
    }

    #[test]
    fn governor_updates_snapshot() {
        let mut m7 = QualityGovernor::new();
        m7.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.damage.cutoff.mean = 15000.0;

        let mut samples = vec![0.0; 1024];
        m7.process(&mut samples, &mut ctx);

        assert_eq!(m7.last_snapshot.cutoff_freq, 15000.0);
    }

    #[test]
    fn governor_reset_clears() {
        let mut m7 = QualityGovernor::new();
        m7.frame_counter = 50;
        m7.reset();
        assert_eq!(m7.frame_counter, 0);
    }
}
