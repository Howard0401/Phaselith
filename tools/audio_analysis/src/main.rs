use clap::Parser;
use hound::WavReader;
use plotters::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

#[derive(Parser)]
#[command(name = "audio_analysis", about = "Compare two WAV files and generate report + PNG")]
struct Args {
    /// Path to "before" WAV file
    before: String,

    /// Path to "after" WAV file
    after: String,

    /// Output PNG path
    #[arg(short, long, default_value = "report.png")]
    output: String,
}

#[derive(Debug)]
struct AudioStats {
    peak_dbfs: f32,
    rms_dbfs: f32,
    crest_factor_db: f32,
    clipped_samples: usize,
    total_samples: usize,
    hf_energy_14k_pct: f32,
    hf_energy_16k_pct: f32,
    spectral_centroid: f32,
    sample_rate: u32,
    channels: u16,
}

struct AudioData {
    /// Mono-mixed samples
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

fn load_wav(path: &str) -> Result<AudioData, Box<dyn std::error::Error>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels;

    let raw: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, _) => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, 24) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 8388607.0))
            .collect::<Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
            .collect::<Result<Vec<_>, _>>()?,
        (fmt, bits) => {
            return Err(format!("Unsupported format: {:?} {}bit", fmt, bits).into());
        }
    };

    // Mix to mono
    let ch = channels as usize;
    let samples: Vec<f32> = if ch == 1 {
        raw
    } else {
        raw.chunks(ch)
            .map(|frame| frame.iter().sum::<f32>() / ch as f32)
            .collect()
    };

    Ok(AudioData {
        samples,
        sample_rate,
        channels,
    })
}

fn analyze(data: &AudioData) -> AudioStats {
    let samples = &data.samples;
    let n = samples.len();

    // Peak
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let peak_dbfs = if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        -120.0
    };

    // RMS
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
    let rms_dbfs = if rms > 0.0 {
        20.0 * rms.log10()
    } else {
        -120.0
    };

    // Crest factor
    let crest_factor_db = peak_dbfs - rms_dbfs;

    // Clipped samples
    let clipped_samples = samples.iter().filter(|s| s.abs() >= 0.99).count();

    // FFT for spectral analysis
    let fft_size = 8192;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let num_frames = if n >= fft_size { (n - fft_size) / (fft_size / 2) + 1 } else { 0 };

    let mut avg_spectrum = vec![0.0f64; fft_size / 2 + 1];

    // Hann window
    let window: Vec<f32> = (0..fft_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
        .collect();

    for frame_idx in 0..num_frames.max(1) {
        let offset = frame_idx * (fft_size / 2);
        let mut buf: Vec<Complex<f32>> = (0..fft_size)
            .map(|i| {
                let s = if offset + i < n { samples[offset + i] } else { 0.0 };
                Complex::new(s * window[i], 0.0)
            })
            .collect();

        fft.process(&mut buf);

        for (k, val) in avg_spectrum.iter_mut().enumerate() {
            if k < buf.len() {
                *val += (buf[k].norm_sqr() as f64) / num_frames.max(1) as f64;
            }
        }
    }

    let sr = data.sample_rate as f64;
    let bin_hz = sr / fft_size as f64;

    // Total energy
    let total_energy: f64 = avg_spectrum.iter().sum();

    // HF energy > 14kHz
    let bin_14k = (14000.0 / bin_hz).ceil() as usize;
    let bin_16k = (16000.0 / bin_hz).ceil() as usize;
    let hf_14k: f64 = avg_spectrum[bin_14k.min(avg_spectrum.len())..].iter().sum();
    let hf_16k: f64 = avg_spectrum[bin_16k.min(avg_spectrum.len())..].iter().sum();

    let hf_energy_14k_pct = if total_energy > 0.0 {
        (hf_14k / total_energy * 100.0) as f32
    } else {
        0.0
    };
    let hf_energy_16k_pct = if total_energy > 0.0 {
        (hf_16k / total_energy * 100.0) as f32
    } else {
        0.0
    };

    // Spectral centroid
    let weighted_sum: f64 = avg_spectrum
        .iter()
        .enumerate()
        .map(|(k, &e)| k as f64 * bin_hz * e)
        .sum();
    let spectral_centroid = if total_energy > 0.0 {
        (weighted_sum / total_energy) as f32
    } else {
        0.0
    };

    AudioStats {
        peak_dbfs,
        rms_dbfs,
        crest_factor_db,
        clipped_samples,
        total_samples: n,
        hf_energy_14k_pct,
        hf_energy_16k_pct,
        spectral_centroid,
        sample_rate: data.sample_rate,
        channels: data.channels,
    }
}

fn compute_spectrum_db(samples: &[f32], fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let window: Vec<f32> = (0..fft_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
        .collect();

    let n = samples.len();
    let num_frames = if n >= fft_size {
        (n - fft_size) / (fft_size / 2) + 1
    } else {
        1
    };

    let bins = fft_size / 2 + 1;
    let mut avg = vec![0.0f64; bins];

    for frame_idx in 0..num_frames {
        let offset = frame_idx * (fft_size / 2);
        let mut buf: Vec<Complex<f32>> = (0..fft_size)
            .map(|i| {
                let s = if offset + i < n { samples[offset + i] } else { 0.0 };
                Complex::new(s * window[i], 0.0)
            })
            .collect();
        fft.process(&mut buf);

        for (k, val) in avg.iter_mut().enumerate() {
            if k < buf.len() {
                *val += buf[k].norm_sqr() as f64 / num_frames as f64;
            }
        }
    }

    avg.iter()
        .map(|&e| {
            let mag = (e as f32).sqrt().max(1e-10);
            20.0 * mag.log10()
        })
        .collect()
}

fn compute_spectrogram(samples: &[f32], fft_size: usize, hop: usize) -> (Vec<Vec<f32>>, usize) {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let window: Vec<f32> = (0..fft_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
        .collect();

    let n = samples.len();
    let bins = fft_size / 2 + 1;
    let mut frames = Vec::new();

    let mut offset = 0;
    while offset + fft_size <= n {
        let mut buf: Vec<Complex<f32>> = (0..fft_size)
            .map(|i| Complex::new(samples[offset + i] * window[i], 0.0))
            .collect();
        fft.process(&mut buf);

        let frame: Vec<f32> = (0..bins)
            .map(|k| {
                let mag = buf[k].norm().max(1e-10);
                20.0 * mag.log10()
            })
            .collect();
        frames.push(frame);
        offset += hop;
    }

    (frames, bins)
}

fn print_comparison(before: &AudioStats, after: &AudioStats) {
    println!();
    println!("=== Audio Comparison Report ===");
    println!();
    println!(
        "  Before: {} Hz, {} ch, {} samples",
        before.sample_rate, before.channels, before.total_samples
    );
    println!(
        "  After:  {} Hz, {} ch, {} samples",
        after.sample_rate, after.channels, after.total_samples
    );
    println!();
    println!(
        "  {:<28} {:>10} {:>10} {:>10}",
        "Metric", "Before", "After", "Delta"
    );
    println!("  {}", "-".repeat(62));

    let rows: Vec<(&str, f32, f32, &str)> = vec![
        ("Peak (dBFS)", before.peak_dbfs, after.peak_dbfs, "dB"),
        ("RMS (dBFS)", before.rms_dbfs, after.rms_dbfs, "dB"),
        (
            "Dynamic Range (crest)",
            before.crest_factor_db,
            after.crest_factor_db,
            "dB",
        ),
        (
            "HF energy >14kHz",
            before.hf_energy_14k_pct,
            after.hf_energy_14k_pct,
            "%",
        ),
        (
            "HF energy >16kHz",
            before.hf_energy_16k_pct,
            after.hf_energy_16k_pct,
            "%",
        ),
        (
            "Spectral centroid",
            before.spectral_centroid,
            after.spectral_centroid,
            "Hz",
        ),
    ];

    for (name, b, a, unit) in &rows {
        let delta = a - b;
        let arrow = if delta > 0.01 {
            "^"
        } else if delta < -0.01 {
            "v"
        } else {
            "="
        };
        println!(
            "  {:<28} {:>8.2} {} {:>8.2} {} {:>+8.2} {} {}",
            name, b, unit, a, unit, delta, unit, arrow
        );
    }

    println!(
        "  {:<28} {:>10} {:>10} {:>+10}",
        "Clipped samples (>=0.99)",
        before.clipped_samples,
        after.clipped_samples,
        after.clipped_samples as i64 - before.clipped_samples as i64,
    );
    println!();
}

fn generate_png(
    before: &AudioData,
    after: &AudioData,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = 1600u32;
    let height = 1200u32;

    let root = BitMapBackend::new(output, (width, height)).into_drawing_area();
    root.fill(&WHITE)?;

    let areas = root.split_evenly((3, 2));

    let max_display_samples = 200_000usize;

    // Row 1: Waveforms
    for (idx, (data, title)) in [(before, "Before - Waveform"), (after, "After - Waveform")]
        .iter()
        .enumerate()
    {
        let samples = &data.samples;
        let display_len = samples.len().min(max_display_samples);
        let step = if samples.len() > display_len {
            samples.len() / display_len
        } else {
            1
        };
        let downsampled: Vec<(f64, f64)> = samples
            .iter()
            .step_by(step)
            .enumerate()
            .map(|(i, &s)| (i as f64 / data.sample_rate as f64 * step as f64, s as f64))
            .collect();

        let x_max = samples.len() as f64 / data.sample_rate as f64;

        let mut chart = ChartBuilder::on(&areas[idx])
            .caption(*title, ("sans-serif", 18).into_font())
            .margin(8)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .build_cartesian_2d(0.0..x_max, -1.0..1.0)?;

        chart
            .configure_mesh()
            .x_desc("Time (s)")
            .y_desc("Amplitude")
            .draw()?;

        chart.draw_series(LineSeries::new(downsampled, &BLUE.mix(0.7)))?;
    }

    // Row 2: Spectrograms
    let fft_size = 2048;
    let hop = 512;

    for (idx, (data, title)) in [
        (before, "Before - Spectrogram"),
        (after, "After - Spectrogram"),
    ]
    .iter()
    .enumerate()
    {
        let (spectrogram, bins) = compute_spectrogram(&data.samples, fft_size, hop);
        let num_frames = spectrogram.len();
        if num_frames == 0 {
            continue;
        }

        let time_max = data.samples.len() as f64 / data.sample_rate as f64;
        let freq_max = data.sample_rate as f64 / 2.0;

        let mut chart = ChartBuilder::on(&areas[2 + idx])
            .caption(*title, ("sans-serif", 18).into_font())
            .margin(8)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .build_cartesian_2d(0.0..time_max, 0.0..freq_max)?;

        chart
            .configure_mesh()
            .x_desc("Time (s)")
            .y_desc("Frequency (Hz)")
            .draw()?;

        // Render spectrogram as colored rectangles
        let time_per_frame = time_max / num_frames as f64;
        let freq_per_bin = freq_max / bins as f64;

        // Downsample for rendering: max ~200 time frames, ~128 freq bins
        let time_step = (num_frames / 200).max(1);
        let freq_step = (bins / 128).max(1);

        for (fi, frame) in spectrogram.iter().enumerate().step_by(time_step) {
            let t0 = fi as f64 * time_per_frame;
            let t1 = t0 + time_per_frame * time_step as f64;

            for (bi, &val) in frame.iter().enumerate().step_by(freq_step) {
                let f0 = bi as f64 * freq_per_bin;
                let f1 = f0 + freq_per_bin * freq_step as f64;

                // Map dB to color: -100 dB = dark, 0 dB = bright
                let normalized = ((val + 100.0) / 100.0).clamp(0.0, 1.0);
                let r = (normalized * 255.0) as u8;
                let g = ((normalized * 0.6) * 255.0) as u8;
                let b_val = ((1.0 - normalized) * 128.0) as u8;
                let color = RGBColor(r, g, b_val);

                chart.draw_series(std::iter::once(Rectangle::new(
                    [(t0, f0), (t1, f1)],
                    color.filled(),
                )))?;
            }
        }
    }

    // Row 3: Frequency spectrum overlay + difference
    let spectrum_fft_size = 8192;
    let before_spectrum = compute_spectrum_db(&before.samples, spectrum_fft_size);
    let after_spectrum = compute_spectrum_db(&after.samples, spectrum_fft_size);

    let sr = before.sample_rate as f64;
    let bin_hz = sr / spectrum_fft_size as f64;
    let max_freq = sr / 2.0;

    // Spectrum overlay
    {
        let mut chart = ChartBuilder::on(&areas[4])
            .caption("Frequency Spectrum Overlay", ("sans-serif", 18).into_font())
            .margin(8)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .build_cartesian_2d((20.0f64..max_freq).log_scale(), -100.0f64..0.0)?;

        chart
            .configure_mesh()
            .x_desc("Frequency (Hz)")
            .y_desc("Magnitude (dB)")
            .draw()?;

        let before_pts: Vec<(f64, f64)> = before_spectrum
            .iter()
            .enumerate()
            .skip(1)
            .map(|(k, &db)| (k as f64 * bin_hz, db as f64))
            .filter(|(f, _)| *f >= 20.0)
            .collect();

        let after_pts: Vec<(f64, f64)> = after_spectrum
            .iter()
            .enumerate()
            .skip(1)
            .map(|(k, &db)| (k as f64 * bin_hz, db as f64))
            .filter(|(f, _)| *f >= 20.0)
            .collect();

        chart
            .draw_series(LineSeries::new(before_pts, &BLUE.mix(0.8)))?
            .label("Before")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLUE.mix(0.8)));

        chart
            .draw_series(LineSeries::new(after_pts, &RED.mix(0.8)))?
            .label("After")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED.mix(0.8)));

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .draw()?;
    }

    // Difference spectrum
    {
        let diff: Vec<(f64, f64)> = before_spectrum
            .iter()
            .zip(after_spectrum.iter())
            .enumerate()
            .skip(1)
            .map(|(k, (&b, &a))| (k as f64 * bin_hz, (a - b) as f64))
            .filter(|(f, _)| *f >= 20.0)
            .collect();

        let y_min = diff.iter().map(|(_, d)| *d).fold(f64::MAX, f64::min).min(-5.0);
        let y_max = diff.iter().map(|(_, d)| *d).fold(f64::MIN, f64::max).max(5.0);

        let mut chart = ChartBuilder::on(&areas[5])
            .caption(
                "Difference Spectrum (After - Before)",
                ("sans-serif", 18).into_font(),
            )
            .margin(8)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .build_cartesian_2d((20.0f64..max_freq).log_scale(), y_min..y_max)?;

        chart
            .configure_mesh()
            .x_desc("Frequency (Hz)")
            .y_desc("Delta (dB)")
            .draw()?;

        // Zero line
        chart.draw_series(LineSeries::new(
            vec![(20.0, 0.0), (max_freq, 0.0)],
            BLACK.stroke_width(1),
        ))?;

        chart.draw_series(LineSeries::new(diff, &GREEN.mix(0.8)))?;
    }

    root.present()?;
    println!("PNG report saved to \"{}\"", output);

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("Loading \"{}\"...", args.before);
    let before_data = load_wav(&args.before)?;
    println!("Loading \"{}\"...", args.after);
    let after_data = load_wav(&args.after)?;

    let before_stats = analyze(&before_data);
    let after_stats = analyze(&after_data);

    print_comparison(&before_stats, &after_stats);

    // Null test: subtract before from after
    null_test(&before_data, &after_data);

    generate_png(&before_data, &after_data, &args.output)?;

    Ok(())
}

fn null_test(before: &AudioData, after: &AudioData) {
    let len = before.samples.len().min(after.samples.len());
    if len == 0 {
        println!("  Cannot perform null test: empty data");
        return;
    }

    let diff: Vec<f32> = before.samples[..len]
        .iter()
        .zip(after.samples[..len].iter())
        .map(|(b, a)| a - b)
        .collect();

    let identical = diff.iter().all(|&d| d == 0.0);
    let diff_peak = diff.iter().map(|d| d.abs()).fold(0.0f32, f32::max);
    let diff_rms = (diff.iter().map(|d| d * d).sum::<f32>() / len as f32).sqrt();
    let diff_peak_db = if diff_peak > 0.0 { 20.0 * diff_peak.log10() } else { -120.0 };
    let diff_rms_db = if diff_rms > 0.0 { 20.0 * diff_rms.log10() } else { -120.0 };
    let nonzero_count = diff.iter().filter(|&&d| d != 0.0).count();
    let nonzero_pct = 100.0 * nonzero_count as f64 / len as f64;

    // Signal-to-noise ratio (original signal vs difference)
    let signal_rms = (before.samples[..len].iter().map(|s| s * s).sum::<f32>() / len as f32).sqrt();
    let snr_db = if diff_rms > 0.0 {
        20.0 * (signal_rms / diff_rms).log10()
    } else {
        f32::INFINITY
    };

    println!("=== Null Test (After - Before) ===");
    println!();
    if identical {
        println!("  *** FILES ARE BIT-IDENTICAL — DSP had NO effect ***");
    } else {
        println!("  Files differ!");
        println!("  Non-zero samples:   {} / {} ({:.2}%)", nonzero_count, len, nonzero_pct);
        println!("  Difference peak:    {:.2} dBFS ({:.8} linear)", diff_peak_db, diff_peak);
        println!("  Difference RMS:     {:.2} dBFS ({:.8} linear)", diff_rms_db, diff_rms);
        println!("  SNR (signal/diff):  {:.2} dB", snr_db);
        println!();

        // Per-frequency-band analysis of the difference
        let fft_size = 8192;
        let sr = before.sample_rate as f64;
        let bin_hz = sr / fft_size as f64;

        let diff_spectrum = compute_spectrum_db(&diff, fft_size);

        let bands = [
            ("Sub-bass   (20-60 Hz)", 20.0, 60.0),
            ("Bass       (60-250 Hz)", 60.0, 250.0),
            ("Low-mid    (250-2k Hz)", 250.0, 2000.0),
            ("High-mid   (2k-6k Hz)", 2000.0, 6000.0),
            ("Presence   (6k-14k Hz)", 6000.0, 14000.0),
            ("Air/HF     (14k-20k Hz)", 14000.0, 20000.0),
            ("Ultra-HF   (20k+ Hz)", 20000.0, sr / 2.0),
        ];

        println!("  Difference energy by frequency band:");
        for (name, lo, hi) in &bands {
            let bin_lo = (*lo / bin_hz).ceil() as usize;
            let bin_hi = (*hi / bin_hz).ceil() as usize;
            let bin_hi = bin_hi.min(diff_spectrum.len());
            if bin_lo >= bin_hi { continue; }

            // Average dB in this band
            let avg_db: f32 = diff_spectrum[bin_lo..bin_hi].iter().sum::<f32>()
                / (bin_hi - bin_lo) as f32;
            let bar_len = ((avg_db + 100.0) / 2.0).clamp(0.0, 40.0) as usize;
            let bar: String = "█".repeat(bar_len);
            println!("    {:<25} {:>7.1} dB  {}", name, avg_db, bar);
        }
    }
    println!();
}
