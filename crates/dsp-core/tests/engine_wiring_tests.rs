use asce_dsp_core::engine::{CirrusEngineBuilder, test_helpers::RecordingModule};
use asce_dsp_core::config::EngineConfig;
use std::sync::{Arc, Mutex};

#[test]
fn engine_executes_modules_in_order() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .add_module(Box::new(RecordingModule::new("M0", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M1", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M2", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M3", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M4", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M5", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M6", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M7", call_log.clone())))
        .build();

    let mut buf = vec![0.0f32; 1024];
    engine.process(&mut buf);

    let log = call_log.lock().unwrap();
    assert_eq!(*log, vec!["M0", "M1", "M2", "M3", "M4", "M5", "M6", "M7"]);
}

#[test]
fn default_engine_has_8_modules() {
    let engine = CirrusEngineBuilder::new(48000, 1024).build_default();
    assert_eq!(engine.module_count(), 8);
    assert_eq!(engine.module_name(0), Some("M0:Orchestrator"));
    assert_eq!(engine.module_name(1), Some("M1:DamagePosterior"));
    assert_eq!(engine.module_name(2), Some("M2:TriLattice"));
    assert_eq!(engine.module_name(3), Some("M3:Factorizer"));
    assert_eq!(engine.module_name(4), Some("M4:Solver"));
    assert_eq!(engine.module_name(5), Some("M5:Reprojection"));
    assert_eq!(engine.module_name(6), Some("M6:SafetyMixer"));
    assert_eq!(engine.module_name(7), Some("M7:Governor"));
}

#[test]
fn context_propagation_m0_writes_damage_m1_reads_it() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    // M0 writes cutoff mean = 15500, M1 reads it
    let m0 = RecordingModule::new("M0", call_log.clone()).with_write_cutoff(15500.0);
    let m1 = RecordingModule::new("M1", call_log.clone());
    let m1_saw_cutoff = m1.saw_cutoff_mean.clone();

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .add_module(Box::new(m0))
        .add_module(Box::new(m1))
        .build();

    let mut buf = vec![0.0f32; 1024];
    engine.process(&mut buf);

    let cutoff = m1_saw_cutoff.lock().unwrap();
    assert_eq!(*cutoff, Some(15500.0));
}

#[test]
fn bypass_when_disabled_no_modules_run() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    let mut config = EngineConfig::default();
    config.enabled = false;

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .add_module(Box::new(RecordingModule::new("M0", call_log.clone())))
        .add_module(Box::new(RecordingModule::new("M1", call_log.clone())))
        .build();

    let mut buf = vec![1.0f32; 1024];
    engine.process(&mut buf);

    let log = call_log.lock().unwrap();
    assert!(log.is_empty(), "No modules should run when disabled");

    // Audio should pass through unchanged
    assert!(buf.iter().all(|&s| (s - 1.0).abs() < f32::EPSILON));
}

#[test]
fn reset_clears_damage_and_frame_index() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = vec![0.5f32; 1024];
    engine.process(&mut buf);
    engine.process(&mut buf);

    assert!(engine.context().frame_index > 0);

    engine.reset();

    assert_eq!(engine.context().frame_index, 0);
    // Damage should be back to default (lossless)
    assert!((engine.context().damage.cutoff.mean - 20000.0).abs() < 0.01);
}

#[test]
fn update_config_propagates_to_context() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut config = EngineConfig::default();
    config.strength = 0.1;
    config.hf_reconstruction = 0.2;
    engine.update_config(config);

    assert!((engine.context().config.strength - 0.1).abs() < f32::EPSILON);
    assert!((engine.context().config.hf_reconstruction - 0.2).abs() < f32::EPSILON);
}

#[test]
fn frame_index_increments_each_process() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();
    let mut buf = vec![0.0f32; 1024];

    assert_eq!(engine.context().frame_index, 0);
    engine.process(&mut buf);
    assert_eq!(engine.context().frame_index, 1);
    engine.process(&mut buf);
    assert_eq!(engine.context().frame_index, 2);
}

#[test]
fn dry_buffer_captures_input() {
    let call_log = Arc::new(Mutex::new(Vec::new()));

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .add_module(Box::new(RecordingModule::new("M0", call_log.clone())))
        .build();

    let mut buf: Vec<f32> = (0..1024).map(|i| i as f32 * 0.001).collect();
    let original = buf.clone();
    engine.process(&mut buf);

    // Dry buffer should contain the original input
    let dry = &engine.context().dry_buffer[..1024];
    for i in 0..1024 {
        assert!(
            (dry[i] - original[i]).abs() < f32::EPSILON,
            "dry_buffer[{i}] mismatch"
        );
    }
}

#[test]
fn multiple_process_calls_accumulate_frame_index() {
    let mut engine = CirrusEngineBuilder::new(48000, 512).build_default();
    let mut buf = vec![0.0f32; 512];

    for _ in 0..10 {
        engine.process(&mut buf);
    }
    assert_eq!(engine.context().frame_index, 10);
}
