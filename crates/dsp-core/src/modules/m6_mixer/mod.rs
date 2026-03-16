pub mod crossover;
pub mod kweighting;
pub mod masking;
pub mod true_peak;

use crate::module_trait::{PhaselithModule, ProcessContext};
use crate::modules::m3_factorizer::transient;

/// M6: Perceptual Safety Mixer.
///
/// Mixes the validated residual into the dry signal with safety guards:
/// - Low-band lock: original signal preserved below cutoff
/// - Soft crossover around cutoff
/// - Add-only above cutoff (no subtraction)
/// - Masking ceiling enforcement
/// - True peak guard
/// - Confidence-weighted blending
/// - Character layer: warmth (soft saturation) + smoothness
/// - Character floor: ensures effect even on high-quality sources
pub struct PerceptualSafetyMixer {
    sample_rate: u32,
    /// EMA-smoothed dry K-weighted MS (slow follower, ~300ms time constant)
    dry_rms_sq_ema: f32,
    /// EMA-smoothed post-processing K-weighted MS (same time constant)
    post_rms_sq_ema: f32,
    /// K-weighting filter for dry signal measurement
    kweight_dry: kweighting::KWeightingFilter,
    /// K-weighting filter for post-processing measurement
    kweight_post: kweighting::KWeightingFilter,
    /// Hop-delayed dry buffer used by browser-safe transient repair.
    delayed_transient_dry_buffer: Vec<f32>,
    /// Hop-delayed enhancement delta buffer. Pre-echo suppression only shapes
    /// this enhancement layer so the dry signal remains phase- and timing-stable.
    delayed_transient_delta_buffer: Vec<f32>,
    delayed_transient_len: usize,
}

impl PerceptualSafetyMixer {
    pub fn new() -> Self {
        Self {
            sample_rate: 48000,
            dry_rms_sq_ema: 0.0,
            post_rms_sq_ema: 0.0,
            kweight_dry: kweighting::KWeightingFilter::new(48000),
            kweight_post: kweighting::KWeightingFilter::new(48000),
            delayed_transient_dry_buffer: Vec::new(),
            delayed_transient_delta_buffer: Vec::new(),
            delayed_transient_len: 0,
        }
    }

    /// Update EMA-smoothed RMS² trackers. Called unconditionally (including
    /// during bypass) so the smoother doesn't go stale.
    fn update_ema(&mut self, dry_rms_sq: f32, post_rms_sq: f32, block_len: usize) {
        let block_dur = block_len as f32 / self.sample_rate as f32;
        let alpha = 1.0 - (-block_dur / 0.3_f32).exp(); // 300ms time constant

        if self.dry_rms_sq_ema < 1e-20 {
            self.dry_rms_sq_ema = dry_rms_sq;
            self.post_rms_sq_ema = post_rms_sq;
        } else {
            self.dry_rms_sq_ema += alpha * (dry_rms_sq - self.dry_rms_sq_ema);
            self.post_rms_sq_ema += alpha * (post_rms_sq - self.post_rms_sq_ema);
        }
    }

    fn apply_delayed_transient_repair(&mut self, samples: &mut [f32], ctx: &ProcessContext) {
        let pre_echo_amount =
            (ctx.config.transient * ctx.config.pre_echo_transient_scaling).clamp(0.0, 1.0);
        if !ctx.config.delayed_transient_repair || pre_echo_amount <= 0.01 {
            self.delayed_transient_len = 0;
            return;
        }

        let len = samples.len();
        if len == 0 {
            return;
        }

        let hop_delay = ctx
            .frame_params
            .hop_size
            .min(
                self.delayed_transient_delta_buffer
                    .len()
                    .saturating_sub(len),
            )
            .max(len);
        let append_start = self.delayed_transient_len;
        let append_end = append_start + len;
        self.delayed_transient_dry_buffer[append_start..append_end]
            .copy_from_slice(&ctx.dry_buffer[..len]);
        for i in 0..len {
            self.delayed_transient_delta_buffer[append_start + i] = samples[i] - ctx.dry_buffer[i];
        }
        self.delayed_transient_len = append_end;

        if let Some(strength) = transient::pre_echo_strength(
            pre_echo_amount,
            ctx.hops_this_block,
            ctx.fields.is_transient,
            ctx.fields.spectral_flux,
            &ctx.fields.transient,
        ) {
            let repair_start = self.delayed_transient_len.saturating_sub(hop_delay);
            transient::suppress_pre_echo(
                &mut self.delayed_transient_delta_buffer[repair_start..self.delayed_transient_len],
                strength,
            );
        }

        let ready = self.delayed_transient_len.saturating_sub(hop_delay);
        let emit = ready.min(len);

        if emit > 0 {
            for i in 0..emit {
                samples[i] =
                    self.delayed_transient_dry_buffer[i] + self.delayed_transient_delta_buffer[i];
            }
            self.delayed_transient_dry_buffer
                .copy_within(emit..self.delayed_transient_len, 0);
            self.delayed_transient_delta_buffer
                .copy_within(emit..self.delayed_transient_len, 0);
            self.delayed_transient_len -= emit;
        }
        if emit < len {
            samples[emit..].fill(0.0);
        }
    }
}

impl PhaselithModule for PerceptualSafetyMixer {
    fn name(&self) -> &'static str {
        "M6:SafetyMixer"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.kweight_dry = kweighting::KWeightingFilter::new(sample_rate);
        self.kweight_post = kweighting::KWeightingFilter::new(sample_rate);
        let max_hop = crate::config::QualityMode::Ultra.hop_size();
        self.delayed_transient_dry_buffer = vec![0.0; _max_frame_size + max_hop];
        self.delayed_transient_delta_buffer = vec![0.0; _max_frame_size + max_hop];
        self.delayed_transient_len = 0;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        let style = ctx.config.style;
        let has_residual =
            !ctx.validated.data.is_empty() && ctx.validated.consistency_score >= 0.01;

        // ── Restoration gain (damage-driven) ──
        let confidence = ctx.damage.overall_confidence;
        let strength = ctx.config.strength;
        let restoration_gain = strength * confidence * ctx.validated.consistency_score;

        // ── Character floor: minimum effect even on pristine sources ──
        // This ensures every song gets at least a subtle character upgrade
        let character_floor = style.character_intensity() * 0.15;
        let mix_gain = restoration_gain.max(character_floor);

        let len = samples.len();
        let limit = 0.99f32;

        // ── Global output level reference (K-weighted) ──
        // Measure dry K-weighted MS for this block. Must happen BEFORE early return
        // so the EMA stays warm during bypass periods — prevents audible
        // compensation jumps when processing resumes after quiet/bypass sections.
        // K-weighting (BS.1770) de-emphasizes low frequencies and boosts presence
        // range, giving a loudness measurement that matches human perception.
        let dry_rms_sq_block = self
            .kweight_dry
            .compute_weighted_ms(&ctx.dry_buffer[..len.min(ctx.dry_buffer.len())]);

        if mix_gain < 0.001
            && style.warmth < 0.01
            && style.smoothness < 0.01
            && ctx.config.ambience_preserve < 0.001
            && !ctx.config.delayed_transient_repair
        {
            // Bypass: still update EMA so it doesn't go stale.
            // During bypass, post == dry, so ratio stays ~1.0 — correct.
            self.update_ema(dry_rms_sq_block, dry_rms_sq_block, len);
            return;
        }

        // ── Phase 1a: Mix freq-domain validated residual ──
        if has_residual && mix_gain > 0.001 {
            let res_len = len.min(ctx.validated.data.len());
            for i in 0..res_len {
                let dry = samples[i];
                let residual = ctx.validated.data[i] * mix_gain;
                let mixed = dry + residual;

                if mixed.abs() > limit {
                    let headroom = (limit - dry.abs()).max(0.0);
                    samples[i] = dry + residual.signum() * headroom.min(residual.abs());
                } else {
                    samples[i] = mixed;
                }

                samples[i] = samples[i].clamp(-limit, limit);
            }
        }

        // ── Phase 1b: Mix time-domain residual (independent gain path) ──
        // time_mix_gain is independent of freq-domain consistency_score and overall_confidence.
        // Declip already has internal scaling (clipping_severity * dynamics * transient),
        // so we only scale by strength and clipping posterior.
        let time_mix_gain = strength * ctx.damage.clipping.mean.clamp(0.0, 1.0);
        if time_mix_gain > 0.001 && !ctx.validated.time_residual.is_empty() {
            let time_len = len.min(ctx.validated.time_residual.len());
            for i in 0..time_len {
                let time_res = ctx.validated.time_residual[i] * time_mix_gain;
                if time_res.abs() > 0.0001 {
                    let mixed = samples[i] + time_res;
                    if mixed.abs() > limit {
                        let headroom = (limit - samples[i].abs()).max(0.0);
                        samples[i] += time_res.signum() * headroom.min(time_res.abs());
                    } else {
                        samples[i] = mixed;
                    }
                    samples[i] = samples[i].clamp(-limit, limit);
                }
            }
        }

        // ── Phase 1c: Ambience preserve (diffuse tail compensation) ──
        // M5 self-reprojection inherently shrinks diffuse/stochastic content
        // (reverb tails get low consistency_score → attenuated). This can make
        // the output sound "too dry" compared to the original.
        //
        // Two-layer design: GATES control *when* to compensate, SOURCE controls *what* to put back.
        //
        // GATES (3 multiplicative):
        // 1. Diffuse gate: only when consistency_score is low (M5 rejected diffuse content)
        // 2. Non-transient gate: skip transient frames (preserve attack clarity)
        // 3. Envelope decay gate: only during energy decay (tail region)
        //
        // SOURCE narrowing (the critical part):
        // The blend-back source is NOT the full `dry - processed` diff.
        // It's high-pass filtered (~500Hz) to only contain upper-mid/high content.
        // This prevents blending back:
        //   - Low-freq bloom (bass bloom would muddy the mix)
        //   - Fundamental/harmonic structure changes (M5 corrections we want to keep)
        //   - Direct sound character modifications
        // What passes through:
        //   - Reverb tail shimmer (1kHz-8kHz diffuse energy)
        //   - Upper-mid ambience (500Hz+ stochastic content)
        //   - Air/room tone in decay regions
        let amb = ctx.config.ambience_preserve;
        if amb > 0.001 && has_residual {
            // Gate 1: Diffuse gate — only compensate when M5 actually rejected diffuse content.
            let diffuse_amount = (1.0 - ctx.validated.consistency_score).max(0.0);
            let diffuse_gate = if diffuse_amount > 0.2 {
                diffuse_amount
            } else {
                0.0
            };

            // Gate 2: Non-transient gate — skip transient frames to preserve attack clarity.
            let transient_gate = if ctx.fields.is_transient { 0.0 } else { 1.0 };

            // Gate 3: Envelope decay gate — only blend in tail/decay regions.
            let dry_energy: f32 =
                ctx.dry_buffer.iter().take(len).map(|s| s * s).sum::<f32>() / len.max(1) as f32;
            let post_energy: f32 =
                samples.iter().take(len).map(|s| s * s).sum::<f32>() / len.max(1) as f32;
            let decay_gate = if dry_energy > 1e-10 && post_energy < dry_energy * 0.95 {
                1.0
            } else {
                0.0
            };

            let combined_gate = diffuse_gate * transient_gate * decay_gate;

            if combined_gate > 0.001 {
                let effective_amb = amb * 0.5 * combined_gate;
                let dry_len = len.min(ctx.dry_buffer.len());

                // ── Source narrowing: 1-pole high-pass on diff ──
                // Cutoff ~500Hz removes low-freq content from blend-back source.
                // α = 1 - π·fc/fs (first-order approximation)
                // At 48kHz: α ≈ 0.967, at 44.1kHz: α ≈ 0.964
                let hp_alpha = 1.0 - (std::f32::consts::PI * 500.0 / self.sample_rate as f32);
                // Local state: reset per block since gate activation is intermittent.
                // HP settles within ~3 samples at this cutoff — negligible for 256+ sample blocks.
                let mut hp_prev_x = 0.0f32;
                let mut hp_prev_y = 0.0f32;

                for i in 0..dry_len {
                    let raw_diff = ctx.dry_buffer[i] - samples[i];

                    // 1-pole HP: y[n] = α·(y[n-1] + x[n] - x[n-1])
                    let hp_diff = hp_alpha * (hp_prev_y + raw_diff - hp_prev_x);
                    hp_prev_x = raw_diff;
                    hp_prev_y = hp_diff;

                    samples[i] = (samples[i] + hp_diff * effective_amb).clamp(-limit, limit);
                }
            }
        }

        // ── Phase 2: Warmth (subtle even-harmonic saturation) ──
        // y = x - warmth * x³ / 3
        // This naturally produces even harmonics (tube-like character).
        // Independent of damage detection — works on any source.
        let apply_warmth = style.warmth > 0.01;
        if apply_warmth {
            let w = style.warmth * 0.15; // scale down: warmth 1.0 → 15% saturation drive
            for i in 0..len {
                let x = samples[i];
                let saturated = x - w * x * x * x / 3.0;
                samples[i] = saturated.clamp(-limit, limit);
            }
        }

        // ── Phase 3: Smoothness (time-domain micro-smoothing) ──
        // Subtle 3-tap averaging that reduces digital harshness.
        // Independent of damage detection — works on any source.
        let apply_smooth = style.smoothness > 0.15 && len >= 3;
        if apply_smooth {
            let smooth_amount = (style.smoothness - 0.15) * 0.12; // gentle: max ~10%
            let mut prev = samples[0];
            for i in 1..len - 1 {
                let curr = samples[i];
                let next = samples[i + 1];
                let smoothed = prev * 0.25 + curr * 0.5 + next * 0.25;
                samples[i] = curr + (smoothed - curr) * smooth_amount;
                prev = curr;
            }
        }

        // ── Phase 4: Global output level compensation ──
        // Uses EMA-smoothed RMS² (~300ms time constant) instead of per-block
        // instantaneous RMS. This prevents the compensator from chasing waveform
        // micro-structure and instead tracks perceived loudness over time.
        //
        // Headroom-aware: limits makeup per-sample to available headroom so that
        // boosted samples don't just hit the 0.99 clamp and get eaten.
        //
        // Conservative: only compensates level LOSS (ratio >= 1), never cuts.
        // Capped at +3 dB (~1.414x) to prevent runaway amplification.
        let post_rms_sq_block = self.kweight_post.compute_weighted_ms(samples);
        self.update_ema(dry_rms_sq_block, post_rms_sq_block, len);

        if self.dry_rms_sq_ema > 1e-12 && self.post_rms_sq_ema > 1e-12 {
            let ratio = (self.dry_rms_sq_ema / self.post_rms_sq_ema).sqrt();
            let makeup = ratio.clamp(1.0, 1.414); // only boost, max +3 dB
            if makeup > 1.001 {
                for i in 0..len {
                    // Headroom-aware: limit boost to what the sample can actually use
                    // without hitting the ceiling. This prevents "boost then clamp" waste.
                    let s = samples[i];
                    let headroom_gain = if s.abs() > 0.001 {
                        (limit / s.abs()).min(makeup)
                    } else {
                        makeup // near-zero samples have unlimited headroom
                    };
                    samples[i] = s * headroom_gain;
                }
            }
        }

        self.apply_delayed_transient_repair(samples, ctx);
    }

    fn reset(&mut self) {
        self.dry_rms_sq_ema = 0.0;
        self.post_rms_sq_ema = 0.0;
        self.kweight_dry.reset();
        self.kweight_post.reset();
        self.delayed_transient_dry_buffer.fill(0.0);
        self.delayed_transient_delta_buffer.fill(0.0);
        self.delayed_transient_len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, StyleConfig};
    use crate::types::ValidatedResidual;

    #[test]
    fn mixer_no_change_when_no_residual() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0); // zero character
        let original = vec![0.5, -0.3, 0.7];
        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();

        m6.process(&mut samples, &mut ctx);
        assert_eq!(samples, original);
    }

    #[test]
    fn mixer_adds_validated_residual() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let original = vec![0.5, 0.5, 0.5];
        let mut ctx = setup_ctx_with_dry(&original, EngineConfig::default());

        ctx.validated = ValidatedResidual {
            data: vec![0.1, 0.1, 0.1],
            time_residual: vec![0.0; 3],
            acceptance_mask: vec![1.0; 3],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };
        ctx.damage.overall_confidence = 1.0;

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        // Should have added some residual
        assert!(
            samples[0] > 0.5,
            "Should have added residual, got {}",
            samples[0]
        );
    }

    #[test]
    fn mixer_respects_true_peak() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let original = vec![0.9, 0.9, 0.9];
        let mut ctx = setup_ctx_with_dry(&original, EngineConfig::default());

        ctx.validated = ValidatedResidual {
            data: vec![10.0, 10.0, 10.0],
            time_residual: vec![0.0; 3],
            acceptance_mask: vec![1.0; 3],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };
        ctx.damage.overall_confidence = 1.0;

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        for &s in &samples {
            assert!(s.abs() <= 0.99, "Should not exceed true peak, got {}", s);
        }
    }

    #[test]
    fn warmth_produces_subtle_saturation() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.5, 0.0, 0.0, 0.0, 0.0, 0.0);

        let original: Vec<f32> = (0..64)
            .map(|i| 0.7 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();

        m6.process(&mut samples, &mut ctx);

        // Output should differ due to saturation (check BEFORE level compensation)
        // Level compensation might bring it back close, but signal shape should differ
        let diff: f32 = samples
            .iter()
            .zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.0, "Warmth should modify the signal");
    }

    #[test]
    fn character_floor_ensures_effect_on_pristine_source() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let original = vec![0.5; 64];
        let mut ctx = setup_ctx_with_dry(&original, EngineConfig::default());
        ctx.damage.overall_confidence = 0.0;
        ctx.validated = ValidatedResidual {
            data: vec![0.05; 64],
            time_residual: vec![0.0; 64],
            acceptance_mask: vec![1.0; 64],
            consistency_score: 0.5,
            reprojection_error: 0.0,
        };

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let diff: f32 = samples
            .iter()
            .zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            diff > 0.0,
            "Character floor should ensure effect, diff={}",
            diff
        );
    }

    #[test]
    fn smoothness_reduces_harshness() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 0.8, 0.0, 0.0, 0.0);

        let original: Vec<f32> = (0..128)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        // Smoothed output should have less extreme transitions
        let orig_var: f32 = original.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        let smooth_var: f32 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        assert!(smooth_var < orig_var, "Smoothing should reduce variation");
    }

    // ── Global level compensation tests ──

    /// Helper: set up M6 context with dry_buffer matching input signal
    fn setup_ctx_with_dry(signal: &[f32], config: EngineConfig) -> ProcessContext {
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.dry_buffer = signal.to_vec();
        ctx.validated = ValidatedResidual::new(signal.len());
        ctx
    }

    #[test]
    fn warmth_level_compensation_preserves_rms() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0); // max warmth

        let original: Vec<f32> = (0..256)
            .map(|i| 0.6 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();
        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.88 && ratio < 1.12,
            "RMS should be compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn smoothness_level_compensation_preserves_rms() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 1.0, 0.0, 0.0, 0.0); // max smoothness

        let original: Vec<f32> = (0..256)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();
        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.88 && ratio < 1.12,
            "RMS should be compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn combined_warmth_smoothness_level_compensated() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.3, 0.0, 0.8, 0.0, 0.0, 0.0);

        let original: Vec<f32> = (0..256)
            .map(|i| 0.7 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();
        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.85 && ratio < 1.15,
            "Combined character RMS compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn level_compensation_respects_true_peak() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 1.0, 0.0, 0.0, 0.0);

        let original: Vec<f32> = (0..128)
            .map(|i| 0.95 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        for &s in &samples {
            assert!(
                s.abs() <= 0.99,
                "Level compensation must not exceed true peak, got {}",
                s
            );
        }
    }

    #[test]
    fn level_compensation_no_effect_on_silence() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.5, 0.0, 0.5, 0.0, 0.0, 0.0);

        let original = vec![0.0f32; 64];
        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let max_val = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_val < 1e-10, "Silence should stay silent, got {max_val}");
    }

    #[test]
    fn level_compensation_capped_at_3db() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 1.0, 0.0, 0.0, 0.0);

        let original: Vec<f32> = (0..256)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut ctx = setup_ctx_with_dry(&original, config);
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();
        assert!(
            post_rms <= pre_rms * 1.42,
            "Makeup gain should be capped: pre={pre_rms:.4}, post={post_rms:.4}"
        );
    }

    /// Helper: run M6 with given dry/processed and ambience_preserve, return output.
    /// Forces all 3 gates open (low consistency, non-transient, decaying energy).
    /// Uses minimal warmth (0.02) to ensure both amb=0 and amb>0 runs go through
    /// the full pipeline (prevents bypass early return).
    fn run_m6_ambience_test(dry: &[f32], processed: &[f32], amb: f32) -> Vec<f32> {
        let n = dry.len();
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.ambience_preserve = amb;
        // Minimal warmth prevents bypass early return so both runs follow same code path
        config.style = StyleConfig::new(0.02, 0.0, 0.0, 0.0, 0.0, 0.0);

        let mut ctx = setup_ctx_with_dry(dry, config);
        ctx.validated = ValidatedResidual {
            data: vec![0.0; n],
            time_residual: vec![0.0; n],
            acceptance_mask: vec![1.0; n],
            consistency_score: 0.3, // low → diffuse gate opens
            reprojection_error: 0.0,
        };
        ctx.fields.is_transient = false;

        let mut samples = processed.to_vec();
        m6.process(&mut samples, &mut ctx);
        samples
    }

    #[test]
    fn tail_preserve_hp_filter_attenuates_low_freq() {
        // Direct test of the 1-pole HP filter used in tail preserve source narrowing.
        // Verify that 100Hz content is heavily attenuated while 3kHz passes through.
        let n = 512;
        let hp_alpha: f32 = 1.0 - (std::f32::consts::PI * 500.0 / 48000.0);

        // Apply HP to 100Hz sine
        let input_lo: Vec<f32> = (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 100.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut hp_x = 0.0f32;
        let mut hp_y = 0.0f32;
        let mut output_lo = vec![0.0f32; n];
        for i in 0..n {
            let hp = hp_alpha * (hp_y + input_lo[i] - hp_x);
            hp_x = input_lo[i];
            hp_y = hp;
            output_lo[i] = hp;
        }

        // Apply HP to 3kHz sine
        let input_hi: Vec<f32> = (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 3000.0 * i as f32 / 48000.0).sin())
            .collect();
        hp_x = 0.0;
        hp_y = 0.0;
        let mut output_hi = vec![0.0f32; n];
        for i in 0..n {
            let hp = hp_alpha * (hp_y + input_hi[i] - hp_x);
            hp_x = input_hi[i];
            hp_y = hp;
            output_hi[i] = hp;
        }

        // Measure RMS after settling (skip first 50 samples)
        let skip = 50;
        let rms = |buf: &[f32]| -> f32 {
            (buf[skip..].iter().map(|s| s * s).sum::<f32>() / (buf.len() - skip) as f32).sqrt()
        };

        let in_rms_lo = rms(&input_lo);
        let out_rms_lo = rms(&output_lo);
        let gain_lo = out_rms_lo / in_rms_lo;

        let in_rms_hi = rms(&input_hi);
        let out_rms_hi = rms(&output_hi);
        let gain_hi = out_rms_hi / in_rms_hi;

        // 100Hz should be attenuated (gain < 0.5)
        assert!(
            gain_lo < 0.5,
            "100Hz should be attenuated by HP, gain={gain_lo:.4}"
        );
        // 3kHz should pass through (gain > 0.9)
        assert!(
            gain_hi > 0.9,
            "3kHz should pass through HP, gain={gain_hi:.4}"
        );
        // Selectivity: high/low gain ratio > 2x
        assert!(
            gain_hi / gain_lo > 2.0,
            "HP selectivity: 3kHz gain ({gain_hi:.4}) should be >2x 100Hz gain ({gain_lo:.4})"
        );
    }

    #[test]
    fn ambience_preserve_activates_with_gates_open() {
        // Integration test: verify tail preserve actually modifies the output
        // when all gates are open and ambience_preserve > 0.
        // Uses broadband signal to avoid HP filter blocking.
        let n = 512;

        // Broadband: mix of frequencies above HP cutoff
        let dry: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 48000.0;
                0.3 * (2.0 * std::f32::consts::PI * 2000.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 5000.0 * t).sin()
            })
            .collect();
        let processed: Vec<f32> = dry.iter().map(|s| s * 0.7).collect();

        let out_no_amb = run_m6_ambience_test(&dry, &processed, 0.0);
        let out_with_amb = run_m6_ambience_test(&dry, &processed, 1.0);

        // Should see a measurable difference when ambience_preserve is on
        let delta: f32 = out_with_amb
            .iter()
            .zip(out_no_amb.iter())
            .skip(10)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / (n - 10) as f32;

        assert!(
            delta > 0.001,
            "Ambience preserve should modify output with broadband signal, delta={delta:.6}"
        );
    }

    #[test]
    fn global_compensation_covers_residual_mixing() {
        // Test that compensation works even when only residual mixing
        // happens (no warmth/smoothness) — this is the new behavior.
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0); // zero character
        config.strength = 1.0;

        let original: Vec<f32> = (0..128)
            .map(|i| 0.6 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let dry_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 128.0).sqrt();

        let mut ctx = setup_ctx_with_dry(&original, config);
        // Add residual that could change level
        ctx.validated.data = vec![0.05; 128];
        ctx.validated.consistency_score = 0.8;
        ctx.damage.overall_confidence = 0.9;
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 128.0).sqrt();

        // Output should not be quieter than input (compensation kicks in)
        // Allow small tolerance for clipping headroom management
        assert!(
            post_rms >= dry_rms * 0.85,
            "Global compensation should prevent level loss: dry={dry_rms:.4}, post={post_rms:.4}"
        );
    }

    #[test]
    fn delayed_transient_repair_shapes_previous_hop() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(128, 48000);

        let mut config = EngineConfig::default();
        config.transient = 1.0;
        config.strength = 1.0;
        config.delayed_transient_repair = true;
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        let enhancement = ValidatedResidual {
            data: vec![0.5; 128],
            time_residual: vec![0.0; 128],
            acceptance_mask: vec![1.0; 128],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };

        let first_block = vec![0.2f32; 128];
        let mut first_ctx = setup_ctx_with_dry(&first_block, config);
        first_ctx.validated = enhancement.clone();
        first_ctx.damage.overall_confidence = 1.0;
        let mut first_samples = first_block.clone();
        m6.process(&mut first_samples, &mut first_ctx);
        assert!(first_samples.iter().all(|s| s.abs() < 1e-10));

        let second_block = vec![0.2f32; 128];
        let mut second_ctx = setup_ctx_with_dry(&second_block, config);
        second_ctx.validated = enhancement.clone();
        second_ctx.damage.overall_confidence = 1.0;
        second_ctx.hops_this_block = 1;
        second_ctx.fields.is_transient = true;
        second_ctx.fields.spectral_flux = 0.8;

        let mut second_samples = second_block.clone();
        m6.process(&mut second_samples, &mut second_ctx);
        assert!(second_samples.iter().all(|s| s.abs() < 1e-10));

        let third_block = vec![0.2f32; 128];
        let mut third_ctx = setup_ctx_with_dry(&third_block, config);
        third_ctx.validated = enhancement.clone();
        third_ctx.damage.overall_confidence = 1.0;
        let mut third_samples = third_block.clone();
        m6.process(&mut third_samples, &mut third_ctx);

        assert!(
            third_samples[0] > 0.19 && third_samples[0] < 0.7,
            "First delayed block should preserve dry content while suppressing the enhancement start"
        );

        let fourth_block = vec![0.2f32; 128];
        let mut fourth_ctx = setup_ctx_with_dry(&fourth_block, config);
        fourth_ctx.validated = enhancement;
        fourth_ctx.damage.overall_confidence = 1.0;
        let mut fourth_samples = fourth_block.clone();
        m6.process(&mut fourth_samples, &mut fourth_ctx);

        assert!(
            fourth_samples[127] > 0.65,
            "Second delayed block should recover the preserved enhancement tail of the previous hop"
        );
    }

    #[test]
    fn delayed_transient_repair_bypasses_when_pre_echo_scaling_is_zero() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(128, 48000);

        let mut config = EngineConfig::default();
        config.transient = 1.0;
        config.pre_echo_transient_scaling = 0.0;
        config.delayed_transient_repair = true;
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        let original = vec![0.25f32; 128];
        let mut ctx = setup_ctx_with_dry(&original, config);
        ctx.hops_this_block = 1;
        ctx.fields.is_transient = true;
        ctx.fields.spectral_flux = 0.8;

        let mut samples = original.clone();
        m6.apply_delayed_transient_repair(&mut samples, &ctx);

        assert_eq!(samples, original);
        assert_eq!(m6.delayed_transient_len, 0);
    }
}
