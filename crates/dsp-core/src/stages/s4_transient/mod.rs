mod pre_echo;

use crate::stages::stage_trait::{DspStage, StageContext};

/// Stage 4: Transient Repair.
///
/// Detects transients (drum hits, plucks) and suppresses pre-echo
/// artifacts caused by MDCT-based lossy compression.
pub struct TransientRepairer {
    onset_threshold: f32,
    prev_energy: f32,
    prev_block: Vec<f32>,
    output_block: Vec<f32>,
    block_size: usize,
}

impl TransientRepairer {
    pub fn new() -> Self {
        Self {
            onset_threshold: 8.0,
            prev_energy: 0.0,
            prev_block: Vec::new(),
            output_block: Vec::new(),
            block_size: 128,
        }
    }
}

impl DspStage for TransientRepairer {
    fn name(&self) -> &'static str {
        "S4:Transient"
    }

    fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {
        self.prev_block = vec![0.0; self.block_size];
        self.output_block = vec![0.0; self.block_size];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext) {
        let strength = ctx.config.transient * ctx.config.strength;
        if strength < 0.01 {
            return;
        }

        for chunk in samples.chunks_mut(self.block_size) {
            let energy = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;

            let ratio = if self.prev_energy > 1e-10 {
                energy / self.prev_energy
            } else {
                1.0
            };

            if ratio > self.onset_threshold {
                // Transient detected: suppress pre-echo in previous block
                pre_echo::suppress_pre_echo(&mut self.prev_block, strength);
            }

            self.prev_energy = energy;
            let copy_len = chunk.len().min(self.prev_block.len());
            self.prev_block[..copy_len].copy_from_slice(&chunk[..copy_len]);
        }
    }

    fn reset(&mut self) {
        self.prev_energy = 0.0;
        self.prev_block.fill(0.0);
    }
}
