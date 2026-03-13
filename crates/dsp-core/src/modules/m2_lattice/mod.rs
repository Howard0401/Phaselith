pub mod stft;
pub mod energy;
pub mod group_delay;

use crate::module_trait::{CirrusModule, ProcessContext};
use crate::types::{MICRO_FFT_SIZE, CORE_FFT_SIZE, AIR_FFT_SIZE};

/// M2: Tri-Lattice Analysis.
///
/// Performs three concurrent STFTs at different window sizes:
/// - Micro (256): transient detail
/// - Core (1024): main synthesis
/// - Air (2048): high-frequency stability
pub struct TriLatticeAnalysis {
    /// Scratch buffers for each lattice's FFT.
    micro_scratch: Vec<f32>,
    core_scratch: Vec<f32>,
    air_scratch: Vec<f32>,
    sample_rate: u32,
}

impl TriLatticeAnalysis {
    pub fn new() -> Self {
        Self {
            micro_scratch: Vec::new(),
            core_scratch: Vec::new(),
            air_scratch: Vec::new(),
            sample_rate: 48000,
        }
    }
}

impl CirrusModule for TriLatticeAnalysis {
    fn name(&self) -> &'static str {
        "M2:TriLattice"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.micro_scratch = vec![0.0; MICRO_FFT_SIZE];
        self.core_scratch = vec![0.0; CORE_FFT_SIZE];
        self.air_scratch = vec![0.0; AIR_FFT_SIZE];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        // Analyze micro lattice
        let micro_len = samples.len().min(MICRO_FFT_SIZE);
        self.micro_scratch[..micro_len].copy_from_slice(&samples[..micro_len]);
        for i in micro_len..MICRO_FFT_SIZE {
            self.micro_scratch[i] = 0.0;
        }
        stft::analyze_lattice(
            &self.micro_scratch[..MICRO_FFT_SIZE],
            &mut ctx.lattice.micro,
            self.sample_rate,
        );

        // Analyze core lattice
        let core_len = samples.len().min(CORE_FFT_SIZE);
        self.core_scratch[..core_len].copy_from_slice(&samples[..core_len]);
        for i in core_len..CORE_FFT_SIZE {
            self.core_scratch[i] = 0.0;
        }
        stft::analyze_lattice(
            &self.core_scratch[..CORE_FFT_SIZE],
            &mut ctx.lattice.core,
            self.sample_rate,
        );

        // Analyze air lattice
        let air_len = samples.len().min(AIR_FFT_SIZE);
        self.air_scratch[..air_len].copy_from_slice(&samples[..air_len]);
        for i in air_len..AIR_FFT_SIZE {
            self.air_scratch[i] = 0.0;
        }
        stft::analyze_lattice(
            &self.air_scratch[..AIR_FFT_SIZE],
            &mut ctx.lattice.air,
            self.sample_rate,
        );
    }

    fn reset(&mut self) {
        self.micro_scratch.fill(0.0);
        self.core_scratch.fill(0.0);
        self.air_scratch.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    #[test]
    fn tri_lattice_initializes() {
        let mut m2 = TriLatticeAnalysis::new();
        m2.init(2048, 48000);
        assert_eq!(m2.micro_scratch.len(), MICRO_FFT_SIZE);
        assert_eq!(m2.core_scratch.len(), CORE_FFT_SIZE);
        assert_eq!(m2.air_scratch.len(), AIR_FFT_SIZE);
    }

    #[test]
    fn tri_lattice_processes_sine() {
        let mut m2 = TriLatticeAnalysis::new();
        m2.init(2048, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = crate::types::TriLattice::new();

        let mut samples: Vec<f32> = (0..2048)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();

        m2.process(&mut samples, &mut ctx);

        // Core lattice should have energy at ~1000 Hz
        let bin_1khz = (1000.0 / (48000.0 / CORE_FFT_SIZE as f32)) as usize;
        let peak_bin = ctx.lattice.core.magnitude
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap();

        assert!(
            (peak_bin as i32 - bin_1khz as i32).abs() <= 2,
            "Peak should be near bin {}, got {}",
            bin_1khz,
            peak_bin
        );
    }

    #[test]
    fn tri_lattice_reset_clears() {
        let mut m2 = TriLatticeAnalysis::new();
        m2.init(2048, 48000);
        m2.micro_scratch[0] = 1.0;
        m2.reset();
        assert_eq!(m2.micro_scratch[0], 0.0);
    }
}
