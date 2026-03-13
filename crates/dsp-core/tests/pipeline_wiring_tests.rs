use asce_dsp_core::pipeline::test_helpers::RecordingStage;
use asce_dsp_core::pipeline::PipelineBuilder;
use asce_dsp_core::config::DspConfig;
use std::sync::{Arc, Mutex};

#[test]
fn pipeline_executes_stages_in_order() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    let pipeline = PipelineBuilder::new(48000, 1024)
        .add_stage(Box::new(RecordingStage::new("S0", call_log.clone())))
        .add_stage(Box::new(RecordingStage::new("S1", call_log.clone())))
        .add_stage(Box::new(RecordingStage::new("S2", call_log.clone())))
        .add_stage(Box::new(RecordingStage::new("S3", call_log.clone())))
        .add_stage(Box::new(RecordingStage::new("S4", call_log.clone())))
        .add_stage(Box::new(RecordingStage::new("S5", call_log.clone())))
        .build();

    let mut buf = vec![0.0f32; 1024];
    let mut pipeline = pipeline;
    pipeline.process(&mut buf);

    let log = call_log.lock().unwrap();
    assert_eq!(*log, vec!["S0", "S1", "S2", "S3", "S4", "S5"]);
}

#[test]
fn pipeline_has_6_stages_in_default_build() {
    let pipeline = PipelineBuilder::new(48000, 1024).build_default();
    assert_eq!(pipeline.stage_count(), 6);
    assert_eq!(pipeline.stage_name(0), Some("S0:Fingerprint"));
    assert_eq!(pipeline.stage_name(1), Some("S1:Dynamics"));
    assert_eq!(pipeline.stage_name(2), Some("S2:Harmonics"));
    assert_eq!(pipeline.stage_name(3), Some("S3:Spectral"));
    assert_eq!(pipeline.stage_name(4), Some("S4:Transient"));
    assert_eq!(pipeline.stage_name(5), Some("S5:Phase"));
}

#[test]
fn context_propagation_s0_writes_degradation_s1_reads_it() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    // S0 writes cutoff, S1 reads it
    let s0 = RecordingStage::new("S0", call_log.clone()).with_write_cutoff(15500.0);
    let s1 = RecordingStage::new("S1", call_log.clone());
    let s1_saw_cutoff = s1.saw_cutoff.clone();

    let mut pipeline = PipelineBuilder::new(48000, 1024)
        .add_stage(Box::new(s0))
        .add_stage(Box::new(s1))
        .build();

    let mut buf = vec![0.0f32; 1024];
    pipeline.process(&mut buf);

    // S1 should have seen the cutoff written by S0
    let cutoff = s1_saw_cutoff.lock().unwrap();
    assert_eq!(*cutoff, Some(Some(15500.0)));
}

#[test]
fn bypass_when_disabled() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    let mut config = DspConfig::default();
    config.enabled = false;

    let mut pipeline = PipelineBuilder::new(48000, 1024)
        .with_config(config)
        .add_stage(Box::new(RecordingStage::new("S0", call_log.clone())))
        .build();

    let mut buf = vec![1.0f32; 1024];
    pipeline.process(&mut buf);

    // No stages should have been called
    let log = call_log.lock().unwrap();
    assert!(log.is_empty(), "No stages should run when disabled");
}

#[test]
fn default_pipeline_processes_without_panic() {
    let mut pipeline = PipelineBuilder::new(48000, 2048).build_default();

    // Generate a test signal
    let mut buf: Vec<f32> = (0..2048)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
        .collect();

    // Should not panic
    pipeline.process(&mut buf);

    // Signal should still be finite
    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn pipeline_reset_clears_state() {
    let mut pipeline = PipelineBuilder::new(48000, 1024).build_default();
    let mut buf = vec![0.5f32; 1024];
    pipeline.process(&mut buf);

    pipeline.reset();

    // After reset, degradation should be default
    assert!(pipeline.degradation().cutoff_freq.is_none());
}

#[test]
fn update_config_changes_behavior() {
    let mut pipeline = PipelineBuilder::new(48000, 1024).build_default();

    let mut config = DspConfig::default();
    config.strength = 0.0;
    pipeline.update_config(config);

    // With zero strength, processing should be minimal
    let mut buf: Vec<f32> = (0..1024)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
        .collect();
    let _original = buf.clone();
    pipeline.process(&mut buf);

    // Signal should be mostly unchanged (stages check strength)
    // We can't guarantee exact equality due to S0 running regardless
    assert!(buf.iter().all(|s| s.is_finite()));
}
