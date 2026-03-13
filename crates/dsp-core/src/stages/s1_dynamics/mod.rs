mod declip;
mod expander;

use crate::stages::stage_trait::{DspStage, StageContext};

/// Stage 1: Dynamic Range Restoration.
///
/// - De-clipping: cubic Hermite interpolation to repair clipped peaks
/// - De-limiting: expander to restore micro-dynamics
pub struct DynamicsRestorer {
    envelope_buf: Vec<f32>,
}

impl DynamicsRestorer {
    pub fn new() -> Self {
        Self {
            envelope_buf: Vec::new(),
        }
    }
}

impl DspStage for DynamicsRestorer {
    fn name(&self) -> &'static str {
        "S1:Dynamics"
    }

    fn init(&mut self, max_frame_size: usize, _sample_rate: u32) {
        self.envelope_buf = vec![0.0; max_frame_size];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext) {
        let strength = ctx.config.dynamics * ctx.config.strength;
        if strength < 0.01 {
            return;
        }

        // De-clipping
        if ctx.degradation.clipping_severity > 0.2 {
            declip::declip_block(samples, 0.99);
        }

        // De-limiting (expand dynamics)
        if ctx.degradation.compression_amount > 0.3 {
            let env_len = samples.len().min(self.envelope_buf.len());
            expander::expand_dynamics(
                samples,
                ctx.degradation.compression_amount * strength,
                &mut self.envelope_buf[..env_len],
            );
        }
    }

    fn reset(&mut self) {
        self.envelope_buf.fill(0.0);
    }
}
