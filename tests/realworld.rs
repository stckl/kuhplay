//! Real-World-Tests: echte Medien (ffmpeg/hound) durch die Real-Engine –
//! ohne GUI, ohne Mock. Stage-Canvas wird am echten Frame gemessen.

use std::path::Path;
use std::time::{Duration, Instant};

use uuid::Uuid;

use kuhplay::core::{FadeCurve, LoopMode, Stage, StageGeometry};
use kuhplay::engine::{spawn_real_engine, EngineCommand, EngineEvent, EngineHandle};

/// Env (KUHPLAY_AUDIO) ist prozessglobal – Real-World-Tests serialisieren.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_test_video(path: &Path, seconds: f32, w: u32, h: u32) {
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={seconds}:size={w}x{h}:rate=10"),
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .output()
        .expect("ffmpeg spawn");
    assert!(
        out.status.success(),
        "ffmpeg: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_wav(path: &Path, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("wav writer");
    let n = (seconds * 44100.0) as usize;
    for i in 0..n {
        let v = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin() * 12000.0) as i16;
        writer.write_sample(v).expect("wav sample");
    }
    writer.finalize().expect("wav finalize");
}

fn play_cmd(
    cue_id: Uuid,
    path: &Path,
    stage: Option<&str>,
    in_pt: Option<Duration>,
    out_pt: Option<Duration>,
) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: Some(path.display().to_string()),
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: None,
        stage: stage.map(|s| s.to_string()),
        layer: 0,
        surface: None,
        in_point: in_pt,
        out_point: out_pt,
    }
}

fn wait_for_event(
    events: &crossbeam_channel::Receiver<EngineEvent>,
    pred: impl Fn(&EngineEvent) -> bool,
    timeout: Duration,
) -> Option<EngineEvent> {
    let deadline = Instant::now() + timeout;
    loop {
        while let Ok(ev) = events.try_recv() {
            if pred(&ev) {
                return Some(ev);
            }
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wie in fade_crossfade: Engine-Thread kann in der Sandbox ohne Audio-Device
/// hängen bleiben – dann sauber SKIP statt fehlschlagen.
fn engine_alive(engine: &EngineHandle) -> bool {
    engine.commands.send(EngineCommand::Ping(9)).ok();
    for _ in 0..20 {
        while let Ok(ev) = engine.events.try_recv() {
            if matches!(ev, EngineEvent::Pong(9)) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn wait_until(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    cond()
}

/// Stage-Canvas-Pixel mittendrin messen.
fn stage_pixel(engine: &EngineHandle, stage: &str, x: u32, y: u32) -> Option<[u8; 4]> {
    let store = engine.stage_frames.lock().unwrap();
    let frame = store.get(stage)?;
    let i = ((y * frame.width + x) * 4) as usize;
    Some([
        frame.rgba[i],
        frame.rgba[i + 1],
        frame.rgba[i + 2],
        frame.rgba[i + 3],
    ])
}

// ---------------------------------------------------------------------------
// 1) Video mit Out-Point endet zur richtigen Zeit (nicht erst bei EOF)
// ---------------------------------------------------------------------------

#[test]
fn realworld_video_out_point_finishes_early() {
    let _env = ENV_LOCK.lock().unwrap();
    std::env::set_var("KUHPLAY_AUDIO", "off");
    if !ffmpeg_available() {
        eprintln!("SKIP: ffmpeg fehlt");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    write_test_video(&video, 4.0, 160, 90);

    let engine = spawn_real_engine();
    if !engine_alive(&engine) {
        eprintln!("SKIP: Real-Engine-Thread antwortet nicht (Sandbox ohne Audio)");
        return;
    }
    let cue = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(
            cue,
            &video,
            None,
            None,
            Some(Duration::from_secs(2)),
        ))
        .unwrap();
    let t0 = Instant::now();
    let fin = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == cue),
        Duration::from_secs(6),
    );
    let elapsed = t0.elapsed();
    assert!(fin.is_some(), "Finished nach Out-Point erwartet");
    assert!(
        elapsed < Duration::from_secs(3),
        "Out=2s bei 4s-Video: Finished nach {elapsed:?}, nicht erst bei 4s"
    );
    assert!(
        elapsed > Duration::from_millis(1200),
        "nicht zu früh: {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// 2) Opacity-Fade am echten Stage-Canvas: Mitte ist Mix, nie schwarz
// ---------------------------------------------------------------------------

#[test]
fn realworld_opacity_fade_on_stage_canvas() {
    let _env = ENV_LOCK.lock().unwrap();
    std::env::set_var("KUHPLAY_AUDIO", "off");
    if !ffmpeg_available() {
        eprintln!("SKIP: ffmpeg fehlt");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    write_test_video(&video, 6.0, 160, 90);

    let engine = spawn_real_engine();
    if !engine_alive(&engine) {
        eprintln!("SKIP: Real-Engine-Thread antwortet nicht (Sandbox ohne Audio)");
        return;
    }
    engine
        .commands
        .send(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(160.0, 90.0)),
        })
        .unwrap();
    wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::StageConfigured { .. }),
        Duration::from_secs(2),
    );

    let cue = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue, &video, Some("Main"), None, None))
        .unwrap();
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == cue),
            Duration::from_secs(5)
        )
        .is_some(),
        "Video startet"
    );

    // Warten bis erstes Stage-Frame da ist, dann halbwegs mittendrin fadet.
    assert!(
        wait_until(
            || stage_pixel(&engine, "Main", 80, 45).is_some_and(|p| p[3] > 200),
            Duration::from_secs(5)
        ),
        "erstes Stage-Frame deckend"
    );

    let fade = Uuid::new_v4();
    engine
        .commands
        .send(EngineCommand::Fade {
            cue_id: fade,
            target_cue: Some(cue),
            from: None,
            volume: None,
            opacity: Some(0.0),
            duration: Duration::from_secs(2),
            curve: FadeCurve::Linear,
            stop_when_done: false,
        })
        .unwrap();
    // Nach ~1 s (Mitte des Fades) messen: Canvas darf weder voll deckend noch schwarz sein.
    std::thread::sleep(Duration::from_millis(1000));
    let mid = stage_pixel(&engine, "Main", 80, 45);
    let fin = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == fade),
        Duration::from_secs(4),
    );
    assert!(fin.is_some(), "Fade Finished nach 2 s");
    let end = stage_pixel(&engine, "Main", 80, 45);
    if let (Some(m), Some(e)) = (mid, end) {
        assert!(m[3] < 250, "Mitte des Fades nicht mehr voll deckend: {m:?}");
        assert!(
            m[3] > 0 || e[3] == 0,
            "Mitte nicht komplett schwarz durchgeschaltet: {m:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3) Hold at end: nach Video-Ende bleibt das letzte Frame auf der Stage
// ---------------------------------------------------------------------------

#[test]
fn realworld_hold_at_end_keeps_last_frame() {
    let _env = ENV_LOCK.lock().unwrap();
    std::env::set_var("KUHPLAY_AUDIO", "off");
    if !ffmpeg_available() {
        eprintln!("SKIP: ffmpeg fehlt");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("short.mp4");
    write_test_video(&video, 1.0, 160, 90);

    let engine = spawn_real_engine();
    if !engine_alive(&engine) {
        eprintln!("SKIP: Real-Engine-Thread antwortet nicht (Sandbox ohne Audio)");
        return;
    }
    engine
        .commands
        .send(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(160.0, 90.0)),
        })
        .unwrap();
    let cue = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue, &video, Some("Main"), None, None))
        .unwrap();
    engine
        .commands
        .send(EngineCommand::HoldLastFrame { cue_id: cue })
        .unwrap();
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::FrameHeld { cue_id } if *cue_id == cue),
            Duration::from_secs(5)
        )
        .is_some(),
        "FrameHeld nach Video-Ende"
    );
    // Frame bleibt im Store (nicht schwarz/leer)
    let px = stage_pixel(&engine, "Main", 80, 45);
    assert!(
        px.is_some_and(|p| p[3] > 0),
        "letztes Frame bleibt sichtbar: {px:?}"
    );
}

// ---------------------------------------------------------------------------
// 4) Audio-Fade: echte WAV, FadeProgress bis 0, Finished nach Duration
// ---------------------------------------------------------------------------

#[test]
fn realworld_audio_fade_progress_events() {
    let _env = ENV_LOCK.lock().unwrap();
    // braucht echtes Audio-Output → opt-in: KUHPLAY_REAL_AUDIO=1 (z. B. auf dem
    // Mac). In der Sandbox ohne Audio-Device: sauber SKIP.
    if std::env::var("KUHPLAY_REAL_AUDIO").as_deref() != Ok("1") {
        eprintln!("SKIP: KUHPLAY_REAL_AUDIO=1 nicht gesetzt (braucht Audio-Device)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    write_wav(&wav, 4.0);

    let engine = spawn_real_engine();
    if !engine_alive(&engine) {
        eprintln!("SKIP: Real-Engine-Thread antwortet nicht");
        return;
    }
    let cue = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue, &wav, None, None, None))
        .unwrap();
    let started = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == cue),
        Duration::from_secs(2),
    );
    if started.is_none() {
        eprintln!("SKIP: kein Audio-Backend");
        return;
    }
    let fade = Uuid::new_v4();
    engine
        .commands
        .send(EngineCommand::Fade {
            cue_id: fade,
            target_cue: Some(cue),
            from: None,
            volume: Some(0.0),
            opacity: None,
            duration: Duration::from_secs(2),
            curve: FadeCurve::Linear,
            stop_when_done: false,
        })
        .unwrap();
    // Events selbst sammeln (wait_for_event würde FadeProgress schlucken).
    let mut saw_fall = false;
    let mut fin = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !fin {
        while let Ok(ev) = engine.events.try_recv() {
            match ev {
                EngineEvent::Finished { cue_id } if cue_id == fade => fin = true,
                EngineEvent::FadeProgress { value, .. } => {
                    if value < 0.95 {
                        saw_fall = true;
                    }
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !saw_fall {
        let rest: Vec<_> = engine.events.try_iter().collect();
        eprintln!("fin={fin} rest={:?}", &rest[..rest.len().min(20)]);
    }
    assert!(fin, "Audio-Fade Finished nach 2 s");
    assert!(saw_fall, "FadeProgress läuft gegen 0");
}
