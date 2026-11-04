//! Vorladen von Video-Cues (`EngineCommand::Load`): ffmpeg startet pausiert,
//! beim Play ist das erste Bild sofort da – kein Schwarz zwischen Video-Cues.
//! Braucht ffmpeg/ffprobe (sonst SKIP).

use std::time::{Duration, Instant};

use kuhplay::core::{LoopMode, Stage, StageGeometry, SurfaceGeometry};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, FrameStore, MediaEngine, RealMediaEngine, StageFrameStore,
};

/// 3-s-Testvideo (helles Testbild, 320×180) – `None` ohne ffmpeg.
fn test_video(dir: &std::path::Path) -> Option<String> {
    let path = dir.join("hell.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=25:duration=3",
        ])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| path.display().to_string())
}

fn engine() -> (RealMediaEngine, FrameStore, StageFrameStore) {
    let frames = new_frame_store();
    let stages = new_stage_frame_store();
    let (tx, _rx) = crossbeam_channel::unbounded();
    let mut e = RealMediaEngine::new(
        frames.clone(),
        new_opacity_store(),
        stages.clone(),
        new_route_frame_store(),
        tx,
    );
    e.handle(EngineCommand::ConfigureStage {
        stage: Stage::new("Main", StageGeometry::new(320.0, 180.0)),
    })
    .unwrap();
    (e, frames, stages)
}

fn play(cue_id: uuid::Uuid, path: &str, pre_wait: Duration) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: Some(path.to_string()),
        duration: None,
        pre_wait,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: None,
        stage: Some("Main".into()),
        layer: 1,
        surface: Some(SurfaceGeometry::fit()),
        in_point: None,
        out_point: None,
    }
}

/// Stage-Mitte dunkel (nichts zu sehen)?
fn stage_dark(stages: &StageFrameStore) -> bool {
    stages.lock().unwrap().get("Main").is_none_or(|s| {
        let i = (90 * 320 + 160) * 4;
        s.rgba[i..i + 3].iter().all(|&c| c < 8)
    })
}

fn tick_for(e: &mut RealMediaEngine, d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        e.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn preloaded_video_shows_its_first_frame_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = test_video(dir.path()) else {
        println!("SKIP: ffmpeg fehlt");
        return;
    };
    let (mut e, frames, stages) = engine();
    let cue = uuid::Uuid::new_v4();
    e.handle(EngineCommand::Load {
        cue_id: cue,
        path: Some(path.clone()),
    })
    .unwrap();
    tick_for(&mut e, Duration::from_secs(2)); // ffmpeg hat vorgestartet
    assert!(stage_dark(&stages), "vorgeladen, aber noch nicht sichtbar");

    let go = Instant::now();
    e.handle(play(cue, &path, Duration::ZERO)).unwrap();
    e.tick();
    assert!(
        frames.lock().unwrap().contains_key(&cue),
        "erstes Bild bereit"
    );
    assert!(
        !stage_dark(&stages),
        "Bild steht im ersten Tick auf der Stage"
    );
    assert!(
        go.elapsed() < Duration::from_millis(100),
        "{:?}",
        go.elapsed()
    );

    // Läuft weiter (neue Bilder kommen).
    let first = frames.lock().unwrap().get(&cue).cloned().unwrap();
    tick_for(&mut e, Duration::from_millis(300));
    let later = frames.lock().unwrap().get(&cue).cloned().unwrap();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &later),
        "Video läuft weiter"
    );
}

#[test]
fn preloaded_video_stays_hidden_during_pre_wait() {
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = test_video(dir.path()) else {
        println!("SKIP: ffmpeg fehlt");
        return;
    };
    let (mut e, _frames, stages) = engine();
    let cue = uuid::Uuid::new_v4();
    e.handle(EngineCommand::Load {
        cue_id: cue,
        path: Some(path.clone()),
    })
    .unwrap();
    tick_for(&mut e, Duration::from_secs(2));
    e.handle(play(cue, &path, Duration::from_millis(800)))
        .unwrap();
    tick_for(&mut e, Duration::from_millis(400));
    assert!(stage_dark(&stages), "in der Pre-Wait noch nichts zu sehen");
    tick_for(&mut e, Duration::from_millis(800));
    assert!(!stage_dark(&stages), "nach der Pre-Wait sichtbar");
}
