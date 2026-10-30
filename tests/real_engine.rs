//! Integrationstests für die **RealMediaEngine** (echtes Audio-/Video-Playout).
//!
//! Im Gegensatz zu den Mock-Engine-Tests (`tests/timing_deterministic.rs`)
//! laufen diese Tests gegen reale Backends:
//!
//! * **Audio:** symphonia-Dekodierung + cpal-Ausgabe (echte Soundkarte).
//! * **Video:** ffmpeg-Subprocess (rawvideo/rgba) – Frame-Landung im
//!   `FrameStore`.
//! * **Diashow:** echte Bild-Dateien (PNG, via `image` erzeugt).
//! * **Stages/Compositor:** `ConfigureStage` + Surface mit Area of Interest
//!   → komponiertes Stage-Canvas im `StageFrameStore`.
//!
//! Hardware-abhängige Tests **skppen selbstständig**, wenn kein Audio-Device
//! (cpal) bzw. kein ffmpeg vorhanden ist – so bleibt die Suite auf
//! CI-Maschinen ohne Media-Hardware grün.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use uuid::Uuid;

use kuhplay::core::{
    Crop, FadeCurve, FadeParam, LoopMode, Slide, SlideShow, Stage, StageGeometry, SurfaceGeometry,
};
use kuhplay::engine::{spawn_real_engine, EngineCommand, EngineEvent, EngineHandle};

/// Tick-Intervall der Real-Engine (50 ms) – Events landen spätestens
/// kurz danach.
const TICK: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------

/// Wartet, bis ein Event mit `pred` eintrifft. Rückgabe: das Event.
fn wait_for_event(
    rx: &Receiver<EngineEvent>,
    pred: impl Fn(&EngineEvent) -> bool,
    timeout: Duration,
) -> Option<EngineEvent> {
    let deadline = Instant::now() + timeout;
    loop {
        while let Ok(event) = rx.try_recv() {
            if pred(&event) {
                return Some(event);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wartet, bis `cond` erfüllt ist (Polling alle 10 ms).
fn wait_until(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cond() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// True, wenn ein Audio-Ausgabegerät verfügbar ist (cpal).
fn audio_device_available() -> bool {
    use cpal::traits::HostTrait;
    cpal::default_host().default_output_device().is_some()
}

/// True, wenn ffmpeg im PATH verfügbar ist.
fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Schreibt eine WAV-Datei (Sinus, mono, 16-bit, 44.1 kHz).
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
        let t = i as f32 / 44100.0;
        let sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.3;
        writer
            .write_sample((sample * i16::MAX as f32) as i16)
            .expect("wav sample");
    }
    writer.finalize().expect("wav finalize");
}

/// Erzeugt ein MP4 (ffmpeg `testsrc`, yuv420p).
/// Rückgabe: `Ok(())` bei Erfolg, sonst `Err` mit der ffmpeg-stderr.
fn write_test_video(path: &Path, seconds: f32, w: u32, h: u32) -> Result<(), String> {
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
        .map_err(|e| format!("ffmpeg spawn failed: {e}"))?;
    if !out.status.success() || !path.exists() {
        return Err(format!(
            "ffmpeg failed (status {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Erzeugt ein PNG: linke Hälfte grün, rechte Hälfte rot (für AoI-Tests).
fn write_half_green_half_red_png(path: &Path, w: u32, h: u32) {
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let px = if x < w / 2 {
                image::Rgba([0, 255, 0, 255])
            } else {
                image::Rgba([255, 0, 0, 255])
            };
            img.put_pixel(x, y, px);
        }
    }
    img.save(path).expect("png save");
}

/// Erzeugt ein einfarbiges PNG.
fn write_solid_png(path: &Path, w: u32, h: u32, rgba: [u8; 4]) {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
    img.save(path).expect("png save");
}

/// Baut ein `Play`-Kommando (Audio/Video, abhängig vom Datei-Inhalt).
fn play_cmd(cue_id: Uuid, path: &Path, duration: Option<Duration>) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: Some(path.display().to_string()),
        duration,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: None,
        stage: None,
        layer: 0,
        surface: None,
    }
}

/// Baut ein `PlaySlideShow`-Kommando.
fn slideshow_cmd(
    cue_id: Uuid,
    slides: Vec<Slide>,
    stage: Option<&str>,
    surface: Option<SurfaceGeometry>,
) -> EngineCommand {
    let show = SlideShow::new(slides, Duration::ZERO, true).expect("valid slideshow");
    EngineCommand::PlaySlideShow {
        cue_id,
        show,
        stage: stage.map(str::to_string),
        layer: 0,
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        surface,
    }
}

fn slide(path: &Path, hold: Duration) -> Slide {
    Slide::new(path.display().to_string(), hold).expect("valid slide")
}

/// Liefert alle Events bis zum Timeout (zum Debuggen/Assertions).
fn collect_events(rx: &Receiver<EngineEvent>, timeout: Duration) -> Vec<EngineEvent> {
    let deadline = Instant::now() + timeout;
    let mut events = Vec::new();
    loop {
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        if Instant::now() >= deadline {
            return events;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------
// Tests (ohne Hardware-Abhängigkeit)
// ---------------------------------------------------------------------

#[test]
fn ping_is_answered_with_pong() {
    let engine = spawn_real_engine();
    engine
        .commands
        .send(EngineCommand::Ping(42))
        .expect("send ping");
    let pong = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Pong(42)),
        Duration::from_secs(2),
    );
    assert!(pong.is_some(), "engine must answer Ping with Pong(42)");
}

#[test]
fn missing_media_file_reports_error_not_panic() {
    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    let missing = PathBuf::from("/nonexistent/kuhplay-definitely-missing.wav");
    engine
        .commands
        .send(play_cmd(cue_id, &missing, None))
        .expect("send play");
    let err = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Error { cue_id: Some(id), .. } if *id == cue_id),
        Duration::from_secs(3),
    );
    let Some(EngineEvent::Error { message, .. }) = err else {
        panic!("expected Error event for missing file");
    };
    assert!(
        message.contains("not found"),
        "error should mention missing file, got: {message}"
    );
}

#[test]
fn configure_stage_valid_reports_configured() {
    let engine = spawn_real_engine();
    let stage = Stage::new("TestStage", StageGeometry::new(64.0, 64.0));
    engine
        .commands
        .send(EngineCommand::ConfigureStage {
            stage: stage.clone(),
        })
        .expect("send configure");
    let evt = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::StageConfigured { stage_name } if stage_name == "TestStage"),
        Duration::from_secs(2),
    );
    assert!(evt.is_some(), "expected StageConfigured event");
}

#[test]
fn configure_stage_invalid_geometry_reports_error() {
    let engine = spawn_real_engine();
    let stage = Stage::new("BadStage", StageGeometry::new(0.0, 64.0));
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("send configure");
    let evt = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Error { cue_id: None, .. }),
        Duration::from_secs(2),
    );
    assert!(
        evt.is_some(),
        "expected Error event for invalid stage geometry"
    );
}

#[test]
fn slideshow_with_missing_image_reports_error() {
    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    let missing = PathBuf::from("/nonexistent/kuhplay-definitely-missing-slide.png");
    engine
        .commands
        .send(slideshow_cmd(
            cue_id,
            vec![slide(&missing, Duration::from_secs(1))],
            None,
            None,
        ))
        .expect("send slideshow");
    let evt = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Error { cue_id: Some(id), .. } if *id == cue_id),
        Duration::from_secs(3),
    );
    assert!(
        evt.is_some(),
        "expected Error event for missing slide image (graceful, no panic)"
    );
}

// ---------------------------------------------------------------------
// Audio-Tests (brauchen cpal-Device)
// ---------------------------------------------------------------------

#[test]
fn audio_play_started_then_finished_at_eof() {
    if !audio_device_available() {
        eprintln!("SKIP: no audio output device");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("tone.wav");
    write_wav(&wav, 0.5); // 500 ms

    let engine: EngineHandle = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue_id, &wav, None)) // duration None → EOF
        .expect("send play");

    let started = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id),
        Duration::from_secs(3),
    );
    assert!(started.is_some(), "expected Started event");

    let finished = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Finished { cue_id: id } if *id == cue_id),
        Duration::from_secs(5),
    );
    assert!(
        finished.is_some(),
        "expected Finished event after natural EOF (~500 ms media)"
    );
}

#[test]
fn audio_pause_resume_stop_events() {
    if !audio_device_available() {
        eprintln!("SKIP: no audio output device");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("loop.wav");
    write_wav(&wav, 3.0); // 3 s

    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    let mut cmd = play_cmd(cue_id, &wav, None);
    if let EngineCommand::Play { loop_mode, .. } = &mut cmd {
        *loop_mode = LoopMode::Infinite; // läuft, bis wir stoppen
    }
    engine.commands.send(cmd).expect("send play");

    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id),
            Duration::from_secs(3),
        )
        .is_some(),
        "expected Started"
    );

    engine
        .commands
        .send(EngineCommand::Pause { cue_id })
        .expect("send pause");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Paused { cue_id: id } if *id == cue_id),
            Duration::from_secs(2),
        )
        .is_some(),
        "expected Paused"
    );

    engine
        .commands
        .send(EngineCommand::Resume { cue_id })
        .expect("send resume");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Resumed { cue_id: id } if *id == cue_id),
            Duration::from_secs(2),
        )
        .is_some(),
        "expected Resumed"
    );

    engine
        .commands
        .send(EngineCommand::Stop { cue_id })
        .expect("send stop");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Stopped { cue_id: id } if *id == cue_id),
            Duration::from_secs(2),
        )
        .is_some(),
        "expected Stopped"
    );
}

// ---------------------------------------------------------------------
// Video-Tests (brauchen ffmpeg)
// ---------------------------------------------------------------------

#[test]
fn video_play_produces_frames_and_finishes() {
    if !ffmpeg_available() {
        eprintln!("SKIP: ffmpeg not available");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let mp4 = dir.path().join("test.mp4");
    write_test_video(&mp4, 1.0, 320, 240).expect("ffmpeg could not create test video");

    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue_id, &mp4, None))
        .expect("send play");

    // Frame muss im FrameStore landen (320×240 laut ffprobe).
    let got_frame = wait_until(
        || {
            engine
                .frames
                .lock()
                .unwrap()
                .get(&cue_id)
                .is_some_and(|f| f.width == 320 && f.height == 240)
        },
        Duration::from_secs(5),
    );
    assert!(got_frame, "expected decoded 320x240 frame in FrameStore");

    // Natürliches Ende nach ~1 s → Finished.
    let finished = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::Finished { cue_id: id } if *id == cue_id),
        Duration::from_secs(6),
    );
    assert!(finished.is_some(), "expected Finished after video EOF");
}

// ---------------------------------------------------------------------
// Diashow-Tests (image-Crate, kein Device nötig)
// ---------------------------------------------------------------------

#[test]
fn slideshow_auto_advances_and_finishes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut slides = Vec::new();
    for i in 0..3 {
        let p = dir.path().join(format!("slide{i}.png"));
        write_solid_png(&p, 32, 32, [10 * i as u8, 100, 200, 255]);
        slides.push(slide(&p, Duration::from_millis(250)));
    }

    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(slideshow_cmd(cue_id, slides, None, None))
        .expect("send slideshow");

    // Frame des aktuellen Slides liegt im FrameStore (während der Wiedergabe;
    // nach Finished räumt die Engine den Store auf).
    let frame_seen = wait_until(
        || engine.frames.lock().unwrap().contains_key(&cue_id),
        Duration::from_secs(2),
    );
    assert!(frame_seen, "expected slide frame in FrameStore");

    // Alle Events einsammeln (Started + initiale SlideChanged(0) + 2 Wechsel
    // à 250 ms + Finished nach ~750 ms).
    let events = collect_events(&engine.events, Duration::from_secs(4));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id)),
        "expected Started"
    );
    // Wechsel auf Slide 1 und 2 (Auto-Advance nach je 250 ms).
    for expected_index in [1, 2] {
        assert!(
            events.iter().any(|e| matches!(
                e,
                EngineEvent::SlideChanged { cue_id: id, index, .. }
                    if *id == cue_id && *index == expected_index
            )),
            "expected SlideChanged to index {expected_index}"
        );
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id: id } if *id == cue_id)),
        "expected Finished after last slide"
    );
}

#[test]
fn slideshow_manual_slide_nav() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut slides = Vec::new();
    for i in 0..2 {
        let p = dir.path().join(format!("nav{i}.png"));
        write_solid_png(&p, 16, 16, [50, 60 * i as u8, 70, 255]);
        // Lange Hold-Zeit: ohne manuelles Nav kein Auto-Advance.
        slides.push(slide(&p, Duration::from_secs(30)));
    }
    // Auto-Advance aus → nur manuelles Schalten.
    let show = SlideShow::new(slides, Duration::ZERO, false).expect("valid slideshow");
    let cue_id = Uuid::new_v4();

    let engine = spawn_real_engine();
    engine
        .commands
        .send(EngineCommand::PlaySlideShow {
            cue_id,
            show,
            stage: None,
            layer: 0,
            duration: None,
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            surface: None,
        })
        .expect("send slideshow");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id),
            Duration::from_secs(3),
        )
        .is_some(),
        "expected Started"
    );

    engine
        .commands
        .send(EngineCommand::SlideNav {
            cue_id,
            nav: kuhplay::core::SlideNav::Next,
        })
        .expect("send next");
    let changed = wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::SlideChanged { cue_id: id, index: 1, .. } if *id == cue_id),
        Duration::from_secs(2),
    );
    assert!(
        changed.is_some(),
        "expected SlideChanged to index 1 via SlideNav"
    );
}

// ---------------------------------------------------------------------
// Stage / Compositor / Surface (Area of Interest) – End-to-End
// ---------------------------------------------------------------------

#[test]
fn stage_canvas_composites_surface_with_area_of_interest() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 64×64: links grün, rechts rot.
    let png = dir.path().join("halves.png");
    write_half_green_half_red_png(&png, 64, 64);

    let engine = spawn_real_engine();

    // Stage 64×64 konfigurieren.
    let stage = Stage::new("AoIStage", StageGeometry::new(64.0, 64.0));
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("send configure");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::StageConfigured { stage_name } if stage_name == "AoIStage"),
            Duration::from_secs(2),
        )
        .is_some(),
        "expected StageConfigured"
    );

    // Surface: volle Canvas, aber AoI = linke Hälfte (grün).
    let mut surface = SurfaceGeometry::full_canvas();
    surface.area_of_interest = Some(Crop {
        left: 0.0,
        top: 0.0,
        right: 0.5,
        bottom: 1.0,
    });

    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(slideshow_cmd(
            cue_id,
            vec![slide(&png, Duration::from_secs(30))],
            Some("AoIStage"),
            Some(surface),
        ))
        .expect("send slideshow");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id),
            Duration::from_secs(3),
        )
        .is_some(),
        "expected Started"
    );

    // Canvas muss erscheinen und ausschließlich grün sein (AoI-Crop
    // schneidet die rote rechte Hälfte weg).
    let ok = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            let Some(frame) = store.get("AoIStage") else {
                return false;
            };
            if frame.width != 64 || frame.height != 64 {
                return false;
            }
            let px = &frame.rgba;
            // Jeder Pixel grün (0,255,0), kein Rot-Anteil.
            px.chunks_exact(4)
                .all(|c| c[1] > 200 && c[0] < 55 && c[2] < 55)
        },
        Duration::from_secs(5),
    );
    assert!(
        ok,
        "expected 64x64 all-green stage canvas (AoI crop removed the red half)"
    );
}

#[test]
fn stage_canvas_reflects_opacity_fade() {
    let dir = tempfile::tempdir().expect("tempdir");
    let png = dir.path().join("green.png");
    write_solid_png(&png, 32, 32, [0, 255, 0, 255]);

    let engine = spawn_real_engine();
    let stage = Stage::new("FadeStage", StageGeometry::new(64.0, 64.0));
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("send configure");
    wait_for_event(
        &engine.events,
        |e| matches!(e, EngineEvent::StageConfigured { stage_name } if stage_name == "FadeStage"),
        Duration::from_secs(2),
    );

    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(slideshow_cmd(
            cue_id,
            vec![slide(&png, Duration::from_secs(30))],
            Some("FadeStage"),
            Some(SurfaceGeometry::full_canvas()),
        ))
        .expect("send slideshow");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Started { cue_id: id } if *id == cue_id),
            Duration::from_secs(3),
        )
        .is_some(),
        "expected Started"
    );

    // Opacity-Fade 1.0 → 0.5 über 400 ms.
    engine
        .commands
        .send(EngineCommand::Fade {
            cue_id,
            param: FadeParam::Opacity,
            from: 1.0,
            target: 0.5,
            duration: Duration::from_millis(400),
            curve: FadeCurve::Linear,
        })
        .expect("send fade");

    // Opacity-Store konvergiert gegen 0.5.
    let converged = wait_until(
        || {
            engine
                .opacities
                .lock()
                .unwrap()
                .get(&cue_id)
                .is_some_and(|o| (*o - 0.5).abs() < 0.06)
        },
        Duration::from_secs(4),
    );
    assert!(converged, "expected opacity store to converge to ~0.5");

    // Canvas-Pixel: grün über schwarz mit alpha 0.5 → Grün-Kanal ≈ 127.
    let blended = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            store.get("FadeStage").is_some_and(|f| {
                let center = ((32 * f.width + 32) * 4) as usize;
                let g = f.rgba[center + 1];
                (110..=145).contains(&g)
            })
        },
        Duration::from_secs(4),
    );
    assert!(
        blended,
        "expected stage canvas pixel green channel ≈127 after opacity fade (got blend over black)"
    );
}

#[test]
fn stop_all_stops_running_playbacks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut slides = Vec::new();
    for i in 0..2 {
        let p = dir.path().join(format!("stop{i}.png"));
        write_solid_png(&p, 16, 16, [i as u8 * 40, 90, 120, 255]);
        slides.push(slide(&p, Duration::from_secs(30)));
    }
    let engine = spawn_real_engine();
    let ids: Vec<Uuid> = (0..2).map(|_| Uuid::new_v4()).collect();
    for id in &ids {
        engine
            .commands
            .send(slideshow_cmd(*id, slides.clone(), None, None))
            .expect("send slideshow");
    }
    // Beide Started-Events abwarten (einzeln wait_for_event würde Events des
    // jeweils anderen Cues verwerfen → Race).
    let both_started = wait_until(
        || {
            let mut seen: Vec<Uuid> = Vec::new();
            while let Ok(event) = engine.events.try_recv() {
                if let EngineEvent::Started { cue_id } = event {
                    seen.push(cue_id);
                }
            }
            ids.iter().all(|id| seen.contains(id))
        },
        Duration::from_secs(3),
    );
    assert!(both_started, "expected Started for both slideshows");
    engine
        .commands
        .send(EngineCommand::StopAll)
        .expect("stop all");
    let stopped: Vec<_> = collect_events(&engine.events, Duration::from_secs(2))
        .into_iter()
        .filter(|e| matches!(e, EngineEvent::Stopped { .. }))
        .collect();
    assert_eq!(
        stopped.len(),
        2,
        "StopAll must emit Stopped for each running playback"
    );
    // Nach StopAll keine Frames mehr in den Stores.
    let frames_empty = wait_until(
        || {
            ids.iter()
                .all(|id| !engine.frames.lock().unwrap().contains_key(id))
        },
        Duration::from_secs(2),
    );
    assert!(frames_empty, "frame store must be cleared after StopAll");
    // Engine lebt weiter (kein Crash): Ping wird beantwortet.
    engine
        .commands
        .send(EngineCommand::Ping(7))
        .expect("send ping");
    assert!(
        wait_for_event(
            &engine.events,
            |e| matches!(e, EngineEvent::Pong(7)),
            Duration::from_secs(2),
        )
        .is_some(),
        "engine must still answer Ping after StopAll"
    );
}

/// Kleiner Guard: `TICK` ist dokumentiert als 50 ms – wenn die Engine
/// schneller/langsamer tickt, schlagen Timeouts in den Tests fehl.
#[test]
fn tick_constant_is_50ms() {
    assert_eq!(TICK, Duration::from_millis(50));
}

/// Unbekanntes Ausgabegerät: Fehler-Event, kein Absturz, Engine läuft weiter.
#[test]
fn unknown_audio_output_reports_error() {
    let engine = kuhplay::engine::spawn_real_engine();
    engine
        .send(EngineCommand::SetAudioOutput {
            device: Some("kuhplay-gibt-es-nicht".to_string()),
        })
        .unwrap();
    engine.send(EngineCommand::Ping(7)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut error, mut pong) = (false, false);
    while !(error && pong) {
        assert!(Instant::now() < deadline, "error={error} pong={pong}");
        match engine.try_recv_event() {
            Some(EngineEvent::Error { message, .. }) => error = message.contains("not found"),
            Some(EngineEvent::Pong(7)) => pong = true,
            Some(EngineEvent::AudioOutputChanged { .. }) => panic!("Gerät gibt es nicht"),
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// Video-Cue mit Tonspur: Der Engine legt zusätzlich zur Video-Wiedergabe
/// eine Audio-Voice an (gleiche Datei, gleicher Seek) → `audio_voices`.
#[test]
fn video_cue_with_audio_track_registers_audio_voice() {
    if !ffmpeg_available() {
        eprintln!("SKIP: ffmpeg not available");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let mp4 = dir.path().join("with-audio.mp4");
    // testsrc (Video) + sine (Audio, aac im mp4-Container).
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1.0:size=320x240:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1.0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&mp4)
        .output()
        .expect("ffmpeg spawn");
    assert!(
        out.status.success(),
        "ffmpeg: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let engine = spawn_real_engine();
    let cue_id = Uuid::new_v4();
    engine
        .commands
        .send(play_cmd(cue_id, &mp4, None))
        .expect("send play");

    let has_voice = wait_until(|| engine.has_audio_voice(cue_id), Duration::from_secs(5));
    assert!(
        has_voice,
        "Video-Cue muss eine Audio-Voice bekommen (Tonspur)"
    );
    // Bild läuft parallel weiter.
    let got_frame = wait_until(
        || engine.frames.lock().unwrap().get(&cue_id).is_some(),
        Duration::from_secs(5),
    );
    assert!(got_frame, "Video-Frame fehlt weiterhin");

    // Stop räumt die Voice wieder auf.
    engine.send(EngineCommand::Stop { cue_id }).expect("stop");
    let gone = wait_until(|| !engine.has_audio_voice(cue_id), Duration::from_secs(2));
    assert!(gone, "Stop muss die Audio-Voice austragen");
}
