//! Chasing mit der Real-Engine: Ein Cue mit `timecode_in` folgt dem externen
//! Timecode – pausiert, wenn er anhält, und läuft mit ihm weiter.
//! Braucht ffmpeg/ffprobe (sonst SKIP).

use std::time::{Duration, Instant};

use kuhplay::core::{Fps, LoopMode, Timecode};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, EngineEvent, MediaEngine, RealMediaEngine,
};

fn test_video(dir: &std::path::Path) -> Option<String> {
    let path = dir.join("chase.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x90:rate=25:duration=20",
        ])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| path.display().to_string())
}

#[test]
fn chasing_cue_pauses_when_timecode_stops_and_resumes_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = test_video(dir.path()) else {
        println!("SKIP: ffmpeg fehlt");
        return;
    };
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut e = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        tx,
    );
    let cue = uuid::Uuid::new_v4();
    let start = Timecode::from_duration(Duration::from_secs(3600), Fps::Fps25);
    e.handle(EngineCommand::Play {
        cue_id: cue,
        path: Some(path),
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: Some(start),
        stage: None,
        layer: 0,
        surface: None,
        in_point: None,
        out_point: None,
    })
    .unwrap();
    let events = |rx: &crossbeam_channel::Receiver<EngineEvent>| -> Vec<EngineEvent> {
        rx.try_iter().collect()
    };
    // Timecode läuft 1 s ab 01:00:00:00 (alle 40 ms ein Wert).
    let mut tc = Duration::from_secs(3600);
    let run = Instant::now();
    while run.elapsed() < Duration::from_secs(1) {
        e.handle(EngineCommand::InjectTimecode {
            tc: Timecode::from_duration(tc, Fps::Fps25),
        })
        .unwrap();
        e.tick();
        tc += Duration::from_millis(40);
        std::thread::sleep(Duration::from_millis(40));
    }
    let before = events(&rx);
    assert!(
        !before
            .iter()
            .any(|ev| matches!(ev, EngineEvent::Paused { cue_id } if *cue_id == cue)),
        "läuft, solange Timecode kommt"
    );
    // Timecode steht 600 ms → Cue pausiert.
    let stop = Instant::now();
    while stop.elapsed() < Duration::from_millis(600) {
        e.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    let paused = events(&rx);
    assert!(
        paused
            .iter()
            .any(|ev| matches!(ev, EngineEvent::Paused { cue_id } if *cue_id == cue)),
        "pausiert, wenn der Timecode anhält: {paused:?}"
    );
    // Timecode läuft weiter → Cue setzt fort.
    e.handle(EngineCommand::InjectTimecode {
        tc: Timecode::from_duration(tc, Fps::Fps25),
    })
    .unwrap();
    e.tick();
    let resumed = events(&rx);
    assert!(
        resumed
            .iter()
            .any(|ev| matches!(ev, EngineEvent::Resumed { cue_id } if *cue_id == cue)),
        "läuft mit dem Timecode weiter: {resumed:?}"
    );
    e.handle(EngineCommand::StopAll).unwrap();
}
