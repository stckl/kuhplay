//! Tests für Video-Stages: Geometrie-Validierung (Skalierung/Cropping),
//! StageManager, Serde-Roundtrips und das Engine-Kommando `ConfigureStage`.

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Crop, Cue, CueType, Stage, StageGeometry, StageManager};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine};

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

// ---------------------------------------------------------------------------
// Domain: Validierung
// ---------------------------------------------------------------------------

#[test]
fn geometry_validation_rules() {
    assert!(StageGeometry::new(1920.0, 1080.0).validate().is_ok());
    assert!(
        StageGeometry::new(0.0, 1080.0).validate().is_err(),
        "width > 0"
    );
    assert!(
        StageGeometry::new(1920.0, -5.0).validate().is_err(),
        "height > 0"
    );

    let mut g = StageGeometry::new(100.0, 100.0);
    g.scale = 0.0;
    assert!(g.validate().is_err(), "scale > 0");
    g.scale = 1.0;
    g.crop = Some(Crop {
        left: 0.2,
        top: 0.0,
        right: 0.8,
        bottom: 1.0,
    });
    assert!(g.validate().is_ok(), "valid crop");
    g.crop = Some(Crop {
        left: 0.9,
        top: 0.0,
        right: 0.1,
        bottom: 1.0,
    });
    assert!(g.validate().is_err(), "left < right");
    g.crop = Some(Crop {
        left: 0.0,
        top: 0.0,
        right: 1.5,
        bottom: 1.0,
    });
    assert!(g.validate().is_err(), "crop within 0.0–1.0");
}

#[test]
fn stage_requires_a_name() {
    let stage = Stage::new("  ", StageGeometry::new(100.0, 100.0));
    assert!(stage.validate().is_err(), "empty stage name rejected");
    assert!(Stage::new("Main", StageGeometry::new(100.0, 100.0))
        .validate()
        .is_ok());
}

#[test]
fn stage_manager_starts_with_default_stage_and_finds_by_name() {
    let mut mgr = StageManager::new();
    assert_eq!(mgr.len(), 1);
    let main = mgr.find("Stage 1").expect("default stage exists");
    assert_eq!(main.geometry.width, 1920.0);
    assert_eq!(main.geometry.height, 1080.0);

    mgr.add(Stage::new("Beamer", StageGeometry::new(1280.0, 720.0)));
    assert_eq!(mgr.len(), 2);
    assert!(mgr.find("Beamer").is_some());
    assert!(mgr.find("Unbekannt").is_none());

    mgr.find_mut("Beamer").unwrap().layers = 4;
    assert_eq!(mgr.find("Beamer").unwrap().layers, 4);
}

#[test]
fn stage_serde_roundtrip() {
    let mut geometry = StageGeometry::new(1280.0, 720.0);
    geometry.x = 100.0;
    geometry.scale = 1.25;
    geometry.crop = Some(Crop {
        left: 0.1,
        top: 0.0,
        right: 0.9,
        bottom: 1.0,
    });
    let mut stage = Stage::new("Beamer", geometry);
    stage.layers = 3;

    let json = serde_json::to_string(&stage).unwrap();
    let back: Stage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "Beamer");
    assert_eq!(back.geometry.scale, 1.25);
    assert_eq!(back.geometry.crop.unwrap().left, 0.1);
    assert_eq!(back.layers, 3);
}

#[test]
fn video_cue_play_command_carries_stage_and_layer() {
    let cue = Cue::new("1", "Video", CueType::Video)
        .with_target("/media/a.mp4")
        .with_duration(Duration::from_secs(5))
        .with_stage("Beamer")
        .with_layer(2);
    match cue.build_trigger_command() {
        Some(EngineCommand::Play { stage, layer, .. }) => {
            assert_eq!(stage.as_deref(), Some("Beamer"));
            assert_eq!(layer, 2);
        }
        other => panic!("expected Play, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Engine: ConfigureStage
// ---------------------------------------------------------------------------

#[test]
fn configure_stage_emits_stage_configured_for_valid_geometry() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);

    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(1920.0, 1080.0)),
        })
        .unwrap();
    assert!(engine.has_stage("Main"));
    assert!(drain(&rx).iter().any(|e| matches!(
        e,
        EngineEvent::StageConfigured { stage_name } if stage_name == "Main"
    )));
}

#[test]
fn configure_stage_with_invalid_geometry_emits_error() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);

    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Broken", StageGeometry::new(0.0, 1080.0)),
        })
        .unwrap();
    assert!(
        !engine.has_stage("Broken"),
        "invalid stage must not be stored"
    );
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Error { cue_id: None, .. })),
        "invalid geometry must produce a global Error event: {events:?}"
    );

    // Engine bleibt arbeitsfähig.
    engine.handle(EngineCommand::Ping(1)).unwrap();
    assert!(drain(&rx).contains(&EngineEvent::Pong(1)));
}

#[test]
fn video_playback_with_stage_reference_runs_normally() {
    // Die Mock-Engine speichert stage/layer am Playback (eine produktive
    // Engine routet das Video auf die Stage); Wiedergabe bleibt unverändert.
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();

    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_millis(100)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: kuhplay::core::LoopMode::Once,
            slices: Vec::new(),
            route: None,
            timecode_in: None,
            stage: Some("Beamer".to_string()),
            layer: 1,
            surface: None,
            in_point: None,
            out_point: None,
        })
        .unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // Finished
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == id)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

/// Das alte Key/Fill-Paar einer Stage wird zur Key-Region mit dem Crop der Fill-Region.
#[test]
fn old_key_fill_pair_becomes_a_key_region() {
    let (fill, key) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let mut stage = kuhplay::core::Stage::new("S", kuhplay::core::StageGeometry::new(100.0, 100.0));
    let mut region = kuhplay::core::Region::new(10.0, 0.0, 50.0, 100.0);
    region.route = Some(fill);
    stage.regions = vec![region];
    stage.key_fill = Some(kuhplay::core::KeyFillRoutes { fill, key });

    assert_eq!(stage.migrate_key_fill(), Some(key));
    assert!(stage.key_fill.is_none());
    assert_eq!(stage.regions.len(), 2);
    let added = &stage.regions[1];
    assert!(added.key);
    assert_eq!((added.x, added.width), (10.0, 50.0));
    assert_eq!(stage.fill_and_key_routes(), (Some(fill), Some(key)));
}
