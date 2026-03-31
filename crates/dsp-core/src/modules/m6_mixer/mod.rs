pub mod crossover;
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
    /// Hop-delayed dry buffer used by browser-safe transient repair.
    delayed_transient_dry_buffer: Vec<f32>,
    /// Hop-delayed enhancement delta buffer. Pre-echo suppression only shapes
    /// this enhancement layer so the dry signal remains phase- and timing-stable.
    delayed_transient_delta_buffer: Vec<f32>,
    delayed_transient_len: usize,
    /// 2-pole cascaded LP states for HF de-emphasis shelf on freq-domain residual.
    /// Two stages give -12 dB/octave rolloff (vs -6 dB/octave with 1-pole),
    /// providing much steeper HF attenuation to suppress spectral splatter
    /// from M3's pre-echo tanh window.
    hf_deemph_lp1_freq: f32,
    hf_deemph_lp2_freq: f32,
    /// 2-pole cascaded LP states for time-domain residual.
    hf_deemph_lp1_time: f32,
    hf_deemph_lp2_time: f32,
    /// 2-pole cascaded LP states for transient repair delta.
    hf_deemph_lp1_delta: f32,
    hf_deemph_lp2_delta: f32,
    /// Headroom diagnostic log
    #[cfg(feature = "headroom-log")]
    headroom_log: HeadroomLog,
}

/// Buffered headroom-exceed logger. Accumulates per-block stats and flushes
/// to `C:\ProgramData\Phaselith\headroom.log` every ~1 second.
#[cfg(feature = "headroom-log")]
pub struct HeadroomLog {
    entries: Vec<HeadroomEntry>,
    block_count: u64,
    flush_counter: u32,
    // Per-block peak tracking
    block_peak_pre_tpg: f32,
    block_peak_post_tpg: f32,
    tpg_triggered: bool,
    block_peak_dry: f32,
    block_peak_residual: f32,
}

#[cfg(feature = "headroom-log")]
#[derive(Clone)]
struct HeadroomEntry {
    block: u64,
    phase: &'static str,
    sample_idx: usize,
    dry: f32,
    residual: f32,
    headroom: f32,
    scaled_residual: f32,
}

#[cfg(feature = "headroom-log")]
impl HeadroomLog {
    fn new() -> Self {
        // Write init marker immediately so we know the feature compiled in
        {
            use std::io::Write;
            let dir = std::path::Path::new(r"C:\ProgramData\Phaselith");
            let _ = std::fs::create_dir_all(dir);
            let path = dir.join("headroom.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let _ = writeln!(f, "=== HeadroomLog init ===");
            }
        }
        Self {
            entries: Vec::with_capacity(256),
            block_count: 0,
            flush_counter: 0,
            block_peak_pre_tpg: 0.0,
            block_peak_post_tpg: 0.0,
            tpg_triggered: false,
            block_peak_dry: 0.0,
            block_peak_residual: 0.0,
        }
    }

    fn track_dry_residual(&mut self, dry: f32, residual: f32) {
        let d = dry.abs();
        let r = residual.abs();
        if d > self.block_peak_dry { self.block_peak_dry = d; }
        if r > self.block_peak_residual { self.block_peak_residual = r; }
    }

    fn track_pre_tpg(&mut self, samples: &[f32]) {
        let peak: f32 = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        self.block_peak_pre_tpg = peak;
    }

    fn track_post_tpg(&mut self, samples: &[f32], limit: f32) {
        let peak: f32 = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        self.block_peak_post_tpg = peak;
        self.tpg_triggered = self.block_peak_pre_tpg > limit;
    }

    fn record(&mut self, phase: &'static str, sample_idx: usize, dry: f32, residual: f32, headroom: f32, scaled_residual: f32) {
        self.entries.push(HeadroomEntry {
            block: self.block_count,
            phase,
            sample_idx,
            dry,
            residual,
            headroom,
            scaled_residual,
        });
    }

    fn end_block(&mut self) {
        self.block_count += 1;
        self.flush_counter += 1;
        // Flush every 100 blocks (~1 second) with peak stats
        if self.flush_counter >= 100 {
            use std::io::Write;
            let path = std::path::Path::new(r"C:\ProgramData\Phaselith\headroom.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                // Always write peak summary
                let _ = writeln!(f,
                    "blk={} dry_pk={:.4} res_pk={:.4} pre_tpg={:.4} post_tpg={:.4} tpg={}{}",
                    self.block_count,
                    self.block_peak_dry,
                    self.block_peak_residual,
                    self.block_peak_pre_tpg,
                    self.block_peak_post_tpg,
                    if self.tpg_triggered { "YES" } else { "no" },
                    if !self.entries.is_empty() {
                        format!(" exceeds={}", self.entries.len())
                    } else {
                        String::new()
                    }
                );
                // Write individual exceed entries if any
                for e in &self.entries {
                    let _ = writeln!(f,
                        "  exceed: block={} {} i={} dry={:.4} res={:.4} headroom={:.4} scaled={:.4}",
                        e.block, e.phase, e.sample_idx, e.dry, e.residual, e.headroom, e.scaled_residual
                    );
                }
            }
            self.entries.clear();
            self.flush_counter = 0;
            // Reset peak trackers for next window
            self.block_peak_dry = 0.0;
            self.block_peak_residual = 0.0;
            self.block_peak_pre_tpg = 0.0;
            self.block_peak_post_tpg = 0.0;
            self.tpg_triggered = false;
        } else {
            // Reset per-block peaks but keep max across the window
            // (already tracked above via max comparison)
        }
    }

    fn flush(&mut self) {
        use std::io::Write;
        let dir = std::path::Path::new(r"C:\ProgramData\Phaselith");
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("headroom.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            for e in &self.entries {
                let _ = writeln!(f,
                    "block={} {} i={} dry={:.4} res={:.4} headroom={:.4} scaled={:.4} ratio={:.2}",
                    e.block, e.phase, e.sample_idx, e.dry, e.residual, e.headroom, e.scaled_residual,
                    if e.residual.abs() > 1e-6 { e.scaled_residual / e.residual } else { 1.0 }
                );
            }
        }
        self.entries.clear();
    }
}

impl PerceptualSafetyMixer {
    pub fn new() -> Self {
        Self {
            sample_rate: 48000,
            delayed_transient_dry_buffer: Vec::new(),
            delayed_transient_delta_buffer: Vec::new(),
            delayed_transient_len: 0,
            hf_deemph_lp1_freq: 0.0,
            hf_deemph_lp2_freq: 0.0,
            hf_deemph_lp1_time: 0.0,
            hf_deemph_lp2_time: 0.0,
            hf_deemph_lp1_delta: 0.0,
            hf_deemph_lp2_delta: 0.0,
            #[cfg(feature = "headroom-log")]
            headroom_log: HeadroomLog::new(),
        }
    }

    #[inline]
    fn hf_deemph_cutoff_hz(&self, ctx: &ProcessContext) -> f32 {
        (4000.0 - ctx.config.hf_tame.clamp(0.0, 1.0) * 1800.0).max(1800.0)
    }

    #[inline]
    fn hf_deemph_shelf_amount(base_drive: f32, hf_tame: f32) -> f32 {
        (base_drive * 0.85 + hf_tame.clamp(0.0, 1.0) * 0.35).clamp(0.0, 0.98)
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
            // HF de-emphasis shelf on transient delta — prevents sibilance
            // from sharp pre-echo suppression gain transitions.
            let deemph_alpha = 1.0
                - (-2.0
                    * std::f32::consts::PI
                    * self.hf_deemph_cutoff_hz(ctx)
                    / self.sample_rate as f32)
                    .exp();
            let shelf_amount =
                Self::hf_deemph_shelf_amount(pre_echo_amount, ctx.config.hf_tame);

            for i in 0..emit {
                let raw_delta = self.delayed_transient_delta_buffer[i];
                // 2-pole cascade shelf on transient delta
                self.hf_deemph_lp1_delta +=
                    deemph_alpha * (raw_delta - self.hf_deemph_lp1_delta);
                self.hf_deemph_lp2_delta +=
                    deemph_alpha * (self.hf_deemph_lp1_delta - self.hf_deemph_lp2_delta);
                let hp = raw_delta - self.hf_deemph_lp2_delta;
                let filtered_delta = raw_delta - shelf_amount * hp;
                samples[i] = self.delayed_transient_dry_buffer[i] + filtered_delta;
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
        let max_hop = crate::config::QualityMode::UltraExtreme.hop_size();
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
        // True peak ceiling with headroom margin.
        // 0.95 (-0.45 dBFS) provides ~0.4 dB headroom for:
        //   - Phase 4 makeup gain pushing samples toward ceiling
        //   - Delayed transient repair adding dry + delta after makeup
        //   - Inter-sample peaks between discrete samples
        // Previously 0.99 left almost no margin, causing audible clipping
        // especially at lower quality presets (smaller FFT → coarser residuals).
        let limit = 0.95f32;

        if mix_gain < 0.001
            && style.warmth < 0.01
            && style.smoothness < 0.01
            && ctx.config.ambience_preserve < 0.001
            && ctx.config.ambience_glue < 0.001
            && !ctx.config.delayed_transient_repair
        {
            return;
        }

        // ── Phase 1a: Mix freq-domain validated residual ──
        // HF de-emphasis: 1-pole LP shelf at ~4 kHz on the restoration residual.
        // Prevents sibilance/harshness at high Compensation Strength.
        // The shelf splits the residual into LP + HP components and attenuates
        // the HP portion.  Shelf depth scales linearly with strength so the
        // roll-off is proportional.  Below 4 kHz the residual passes mostly
        // unmodified; above 4 kHz it's progressively attenuated (up to 85%
        // at strength=1.0, giving ~-5 dB at 8 kHz, ~-12 dB at 16 kHz).
        if has_residual && mix_gain > 0.001 {
            let deemph_alpha = 1.0
                - (-2.0
                    * std::f32::consts::PI
                    * self.hf_deemph_cutoff_hz(ctx)
                    / self.sample_rate as f32)
                    .exp();
            let shelf_amount = Self::hf_deemph_shelf_amount(strength, ctx.config.hf_tame);

            let res_len = len.min(ctx.validated.data.len());
            for i in 0..res_len {
                let dry = samples[i];
                let raw_res = ctx.validated.data[i] * mix_gain;

                // 2-pole cascade: two 1-pole LP stages → -12 dB/octave rolloff.
                // HP = raw - LP2 captures more HF energy than single-pole.
                self.hf_deemph_lp1_freq +=
                    deemph_alpha * (raw_res - self.hf_deemph_lp1_freq);
                self.hf_deemph_lp2_freq +=
                    deemph_alpha * (self.hf_deemph_lp1_freq - self.hf_deemph_lp2_freq);
                let hp = raw_res - self.hf_deemph_lp2_freq;
                let residual = raw_res - shelf_amount * hp;

                #[cfg(feature = "headroom-log")]
                self.headroom_log.track_dry_residual(dry, residual);

                // Directional headroom mixing: compute max residual that keeps
                // |dry + residual| <= limit, accounting for sign.
                let max_res = if residual >= 0.0 {
                    (limit - dry).max(0.0)
                } else {
                    (limit + dry).max(0.0) // distance to -limit
                };
                let res_abs = residual.abs();
                let scaled_res = if res_abs > max_res && res_abs > 1e-6 {
                    #[cfg(feature = "headroom-log")]
                    self.headroom_log.record("1a-freq", i, dry, residual, max_res, residual * (max_res / res_abs));
                    residual * (max_res / res_abs)
                } else {
                    residual
                };
                samples[i] = dry + scaled_res;
            }
        }

        // ── Phase 1b: Mix time-domain residual (independent gain path) ──
        // time_mix_gain is independent of freq-domain consistency_score and overall_confidence.
        // Declip already has internal scaling (clipping_severity * dynamics * transient),
        // so we only scale by strength and clipping posterior.
        // Same HF de-emphasis shelf applied to prevent time-domain sibilance.
        let time_mix_gain = strength * ctx.damage.clipping.mean.clamp(0.0, 1.0);
        if time_mix_gain > 0.001 && !ctx.validated.time_residual.is_empty() {
            let deemph_alpha = 1.0
                - (-2.0
                    * std::f32::consts::PI
                    * self.hf_deemph_cutoff_hz(ctx)
                    / self.sample_rate as f32)
                    .exp();
            let shelf_amount = Self::hf_deemph_shelf_amount(strength, ctx.config.hf_tame);

            let time_len = len.min(ctx.validated.time_residual.len());
            for i in 0..time_len {
                let raw_time_res = ctx.validated.time_residual[i] * time_mix_gain;

                // 2-pole cascade shelf on time residual
                self.hf_deemph_lp1_time +=
                    deemph_alpha * (raw_time_res - self.hf_deemph_lp1_time);
                self.hf_deemph_lp2_time +=
                    deemph_alpha * (self.hf_deemph_lp1_time - self.hf_deemph_lp2_time);
                let hp = raw_time_res - self.hf_deemph_lp2_time;
                let time_res = raw_time_res - shelf_amount * hp;

                if time_res.abs() > 0.0001 {
                    let cur = samples[i];
                    let max_tr = if time_res >= 0.0 {
                        (limit - cur).max(0.0)
                    } else {
                        (limit + cur).max(0.0)
                    };
                    let tr_abs = time_res.abs();
                    let scaled_tr = if tr_abs > max_tr && tr_abs > 1e-6 {
                        #[cfg(feature = "headroom-log")]
                        self.headroom_log.record("1b-time", i, cur, time_res, max_tr, time_res * (max_tr / tr_abs));
                        time_res * (max_tr / tr_abs)
                    } else {
                        time_res
                    };
                    samples[i] += scaled_tr;
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
        let glue = ctx.config.ambience_glue.clamp(0.0, 1.0);
        if (amb > 0.001 || glue > 0.001) && has_residual {
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
                let effective_amb = ((amb * 0.5) + glue * 0.22).min(0.85) * combined_gate;
                let dry_len = len.min(ctx.dry_buffer.len());

                // ── Source narrowing: 1-pole high-pass on diff ──
                // Glue lowers the cutoff so more low-mid room energy tracks the body.
                // α = 1 - π·fc/fs (first-order approximation)
                // At 48kHz: 500Hz ≈ 0.967, 220Hz ≈ 0.986
                let hp_cutoff_hz = (500.0 - glue * 280.0).max(220.0);
                let hp_alpha =
                    1.0 - (std::f32::consts::PI * hp_cutoff_hz / self.sample_rate as f32);
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

        self.apply_delayed_transient_repair(samples, ctx);

        // ── Final true peak guard ──
        // apply_delayed_transient_repair reconstructs dry + filtered_delta
        // WITHOUT any ceiling enforcement. The sum can exceed 1.0,
        // causing hard clipping in the DAC.
        //
        // Uses uniform block gain reduction (not per-sample hard clipping)
        // to preserve waveform shape while preventing inter-sample overs.
        #[cfg(feature = "headroom-log")]
        self.headroom_log.track_pre_tpg(samples);
        true_peak::apply_true_peak_guard(samples, limit);
        #[cfg(feature = "headroom-log")]
        {
            self.headroom_log.track_post_tpg(samples, limit);
            self.headroom_log.end_block();
        }
    }

    fn reset(&mut self) {
        self.delayed_transient_dry_buffer.fill(0.0);
        self.delayed_transient_delta_buffer.fill(0.0);
        self.delayed_transient_len = 0;
        self.hf_deemph_lp1_freq = 0.0;
        self.hf_deemph_lp2_freq = 0.0;
        self.hf_deemph_lp1_time = 0.0;
        self.hf_deemph_lp2_time = 0.0;
        self.hf_deemph_lp1_delta = 0.0;
        self.hf_deemph_lp2_delta = 0.0;
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

    /// Helper: set up M6 context with dry_buffer matching input signal
    fn setup_ctx_with_dry(signal: &[f32], config: EngineConfig) -> ProcessContext {
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.dry_buffer = signal.to_vec();
        ctx.validated = ValidatedResidual::new(signal.len());
        ctx
    }

    #[test]
    fn processing_still_respects_true_peak() {
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
                "Processing must not exceed true peak, got {}",
                s
            );
        }
    }

    #[test]
    fn processing_keeps_silence_silent() {
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

    /// Helper: run M6 with given dry/processed ambience settings, return output.
    /// Forces all 3 gates open (low consistency, non-transient, decaying energy).
    /// Uses minimal warmth (0.02) to ensure both amb=0 and amb>0 runs go through
    /// the full pipeline (prevents bypass early return).
    fn run_m6_ambience_test(dry: &[f32], processed: &[f32], amb: f32, glue: f32) -> Vec<f32> {
        let n = dry.len();
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.ambience_preserve = amb;
        config.ambience_glue = glue;
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

        let out_no_amb = run_m6_ambience_test(&dry, &processed, 0.0, 0.0);
        let out_with_amb = run_m6_ambience_test(&dry, &processed, 1.0, 0.0);

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
    fn ambience_glue_reintroduces_low_mid_room_energy() {
        let n = 512;
        let dry: Vec<f32> = (0..n)
            .map(|i| 0.35 * (2.0 * std::f32::consts::PI * 320.0 * i as f32 / 48000.0).sin())
            .collect();
        let processed: Vec<f32> = dry.iter().map(|s| s * 0.7).collect();

        let out_no_glue = run_m6_ambience_test(&dry, &processed, 0.0, 0.0);
        let out_with_glue = run_m6_ambience_test(&dry, &processed, 0.0, 1.0);

        let delta: f32 = out_with_glue
            .iter()
            .zip(out_no_glue.iter())
            .skip(10)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / (n - 10) as f32;

        assert!(
            delta > 0.001,
            "Ambience glue should change low-mid tail/body balance, delta={delta:.6}"
        );
    }

    #[test]
    fn residual_mixing_no_longer_adds_makeup_gain() {
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
        ctx.validated.data = original.iter().map(|s| -0.2 * s).collect();
        ctx.validated.consistency_score = 1.0;
        ctx.damage.overall_confidence = 1.0;
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 128.0).sqrt();
        assert!(
            post_rms < dry_rms * 0.9,
            "Residual mixing should not auto-restore RMS anymore: dry={dry_rms:.4}, post={post_rms:.4}"
        );
    }

    #[test]
    fn delayed_transient_repair_shapes_previous_hop() {
        // With hop=128=block, hop_delay=128 (one block). The delay buffer emits
        // the *previous* block's content, so transient suppression affects the
        // block emitted one block later.
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

        // Block 1: no transient — buffered, nothing emitted yet (hop_delay = 1 block)
        let first_block = vec![0.2f32; 128];
        let mut first_ctx = setup_ctx_with_dry(&first_block, config);
        first_ctx.validated = enhancement.clone();
        first_ctx.damage.overall_confidence = 1.0;
        let mut first_samples = first_block.clone();
        m6.process(&mut first_samples, &mut first_ctx);
        assert!(first_samples.iter().all(|s| s.abs() < 1e-10),
            "Block 1: nothing emitted yet (filling delay buffer)");

        // Block 2: transient detected — suppresses THIS block's delta in buffer,
        // emits Block 1's unsuppressed content (dry + full enhancement)
        let second_block = vec![0.2f32; 128];
        let mut second_ctx = setup_ctx_with_dry(&second_block, config);
        second_ctx.validated = enhancement.clone();
        second_ctx.damage.overall_confidence = 1.0;
        second_ctx.hops_this_block = 1;
        second_ctx.fields.is_transient = true;
        second_ctx.fields.spectral_flux = 0.8;

        let mut second_samples = second_block.clone();
        m6.process(&mut second_samples, &mut second_ctx);
        assert!(
            second_samples[64] > 0.19,
            "Block 2: emits Block 1's unsuppressed content, got {}",
            second_samples[64]
        );

        // Block 3: no transient — emits Block 2's SUPPRESSED delta (transient repair active)
        let third_block = vec![0.2f32; 128];
        let mut third_ctx = setup_ctx_with_dry(&third_block, config);
        third_ctx.validated = enhancement.clone();
        third_ctx.damage.overall_confidence = 1.0;
        let mut third_samples = third_block.clone();
        m6.process(&mut third_samples, &mut third_ctx);

        assert!(
            third_samples[0] > 0.19 && third_samples[0] < 0.7,
            "Block 3: dry preserved, enhancement suppressed by transient repair, got {}",
            third_samples[0]
        );

        // Block 4: emits Block 3's unsuppressed delta (full enhancement recovered)
        let fourth_block = vec![0.2f32; 128];
        let mut fourth_ctx = setup_ctx_with_dry(&fourth_block, config);
        fourth_ctx.validated = enhancement;
        fourth_ctx.damage.overall_confidence = 1.0;
        let mut fourth_samples = fourth_block.clone();
        m6.process(&mut fourth_samples, &mut fourth_ctx);

        assert!(
            fourth_samples[127] > 0.3,
            "Block 4: should recover enhancement from Block 3's unsuppressed delta, got {}",
            fourth_samples[127]
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

    // ── HF de-emphasis shelf tests ──

    /// Helper: measure RMS of a frequency band in the M6 output residual.
    /// Feeds a pure-tone residual at `freq_hz` through M6 Phase 1a and returns
    /// the ratio of output residual RMS to input residual RMS.
    fn measure_residual_hf_gain(
        freq_hz: f32,
        strength: f32,
        hf_tame: f32,
        sample_rate: u32,
    ) -> f32 {
        let n = 1024;
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(n, sample_rate);

        let mut config = EngineConfig::default();
        config.strength = strength;
        config.hf_tame = hf_tame;
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        // Silent dry signal — all output change comes from residual
        let dry = vec![0.0f32; n];
        let mut ctx = setup_ctx_with_dry(&dry, config);

        // Pure-tone residual
        let residual: Vec<f32> = (0..n)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate as f32).sin())
            .collect();
        ctx.validated = ValidatedResidual {
            data: residual.clone(),
            time_residual: vec![0.0; n],
            acceptance_mask: vec![1.0; n],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };
        ctx.damage.overall_confidence = 1.0;

        let mut samples = dry.clone();
        m6.process(&mut samples, &mut ctx);

        // Measure RMS after settling (skip first 64 samples for LP transient)
        let skip = 64;
        let input_rms = (residual[skip..].iter().map(|s| s * s).sum::<f32>()
            / (n - skip) as f32)
            .sqrt();
        let output_rms = (samples[skip..].iter().map(|s| s * s).sum::<f32>()
            / (n - skip) as f32)
            .sqrt();

        if input_rms < 1e-10 {
            1.0
        } else {
            output_rms / input_rms
        }
    }

    #[test]
    fn hf_deemph_low_freq_passes_through() {
        // 500 Hz residual should pass through with minimal attenuation
        // regardless of strength, because 500 Hz is well below the shelf crossover.
        let gain = measure_residual_hf_gain(500.0, 1.0, 0.0, 48000);
        assert!(
            gain > 0.85,
            "500 Hz should pass through shelf at strength=1.0, gain={gain:.4}"
        );
    }

    #[test]
    fn hf_deemph_mid_freq_mostly_passes() {
        // 2 kHz residual should mostly pass through (slight shelf transition)
        let gain = measure_residual_hf_gain(2000.0, 1.0, 0.0, 48000);
        assert!(
            gain > 0.70,
            "2 kHz should mostly pass shelf at strength=1.0, gain={gain:.4}"
        );
    }

    #[test]
    fn hf_deemph_sibilance_range_attenuated() {
        // 8 kHz (sibilance range) should be significantly attenuated at high strength.
        // Compare HF/LF ratio to isolate shelf effect from mix_gain scaling.
        let gain_8k_s1 = measure_residual_hf_gain(8000.0, 1.0, 0.0, 48000);
        let gain_500_s1 = measure_residual_hf_gain(500.0, 1.0, 0.0, 48000);
        let ratio_hi_strength = gain_8k_s1 / gain_500_s1.max(1e-10);

        let gain_8k_s03 = measure_residual_hf_gain(8000.0, 0.3, 0.0, 48000);
        let gain_500_s03 = measure_residual_hf_gain(500.0, 0.3, 0.0, 48000);
        let ratio_lo_strength = gain_8k_s03 / gain_500_s03.max(1e-10);

        assert!(
            ratio_hi_strength < 0.65,
            "8kHz/500Hz ratio should be < 0.65 at strength=1.0, got {ratio_hi_strength:.4}"
        );
        assert!(
            ratio_lo_strength > ratio_hi_strength,
            "Low strength should have higher HF/LF ratio: lo={ratio_lo_strength:.4}, hi={ratio_hi_strength:.4}"
        );
    }

    #[test]
    fn hf_deemph_extreme_hf_heavily_attenuated() {
        // 16 kHz should be heavily attenuated at high strength
        let gain = measure_residual_hf_gain(16000.0, 1.0, 0.0, 48000);
        assert!(
            gain < 0.45,
            "16 kHz should be heavily attenuated at strength=1.0, gain={gain:.4}"
        );
    }

    #[test]
    fn hf_deemph_zero_strength_no_effect() {
        // At strength=0, shelf_amount should be 0 → no attenuation
        // But mix_gain = max(strength*confidence*consistency, character_floor)
        // At strength=0, mix_gain = character_floor ≈ 0.015
        // So residual is mixed at very low level, not directly comparable.
        // Instead verify that the shelf itself doesn't apply: gain ratio
        // between 500 Hz and 16 kHz should be close to 1 at low strength.
        let gain_lo = measure_residual_hf_gain(500.0, 0.1, 0.0, 48000);
        let gain_hi = measure_residual_hf_gain(16000.0, 0.1, 0.0, 48000);
        let ratio = gain_hi / gain_lo.max(1e-10);
        assert!(
            ratio > 0.80,
            "At low strength, HF/LF ratio should be near 1, got {ratio:.4}"
        );
    }

    #[test]
    fn hf_tame_adds_extra_sibilance_reduction() {
        let ratio_no_tame = measure_residual_hf_gain(8000.0, 1.0, 0.0, 48000)
            / measure_residual_hf_gain(500.0, 1.0, 0.0, 48000).max(1e-10);
        let ratio_full_tame = measure_residual_hf_gain(8000.0, 1.0, 1.0, 48000)
            / measure_residual_hf_gain(500.0, 1.0, 1.0, 48000).max(1e-10);

        assert!(
            ratio_full_tame < ratio_no_tame,
            "HF tame should further reduce the 8k/500 ratio: tame={ratio_full_tame:.4}, base={ratio_no_tame:.4}"
        );
    }

    #[test]
    fn hf_deemph_selectivity_ratio() {
        // At high strength, the 8kHz/500Hz gain ratio should show
        // meaningful selectivity (HF attenuated, LF preserved).
        let gain_500 = measure_residual_hf_gain(500.0, 1.0, 0.0, 48000);
        let gain_8k = measure_residual_hf_gain(8000.0, 1.0, 0.0, 48000);
        let selectivity = gain_500 / gain_8k.max(1e-10);
        assert!(
            selectivity > 1.5,
            "Shelf selectivity (500/8k) should be > 1.5x, got {selectivity:.4}"
        );
    }

    #[test]
    fn hf_deemph_strength_sweep() {
        // Verify monotonic: higher strength → relatively more HF attenuation.
        // We measure the HF/LF ratio at each strength to isolate the shelf
        // effect from the overall mix_gain scaling.
        let strengths = [0.3, 0.5, 0.7, 1.0];
        let mut prev_ratio = 2.0f32; // start high
        for &s in &strengths {
            let gain_lf = measure_residual_hf_gain(500.0, s, 0.0, 48000);
            let gain_hf = measure_residual_hf_gain(10000.0, s, 0.0, 48000);
            let ratio = gain_hf / gain_lf.max(1e-10);
            assert!(
                ratio <= prev_ratio + 0.05, // allow small float tolerance
                "HF/LF ratio should decrease with strength: s={s}, ratio={ratio:.4}, prev={prev_ratio:.4}"
            );
            prev_ratio = ratio;
        }
        // At max strength, HF should be significantly lower than LF
        assert!(
            prev_ratio < 0.65,
            "At strength=1.0, 10kHz/500Hz ratio should be < 0.65, got {prev_ratio:.4}"
        );
    }
}
