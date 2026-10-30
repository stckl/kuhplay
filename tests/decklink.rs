//! Integrationstests für die Decklink-Ausgabe (`src/engine/decklink.rs`):
//! * ffmpeg-Args (rawvideo-Input → decklink-Muxer)
//! * Frame-Pipe gegen einen echten Subprocess (`cat` als Sink)
//! * Frame-Validierung (falsche Größe → Fehler)
//! * Lifecycle gegen ffmpeg mit `-f null -` (gleiches rawvideo-Setup wie
//!   Decklink, aber ohne Hardware – skipped, wenn ffmpeg fehlt)

use std::time::Duration;

use kuhplay::core::{Stage, StageGeometry};
use kuhplay::engine::decklink::{ffmpeg_decklink_args, DecklinkConfig, DecklinkOutput};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, MediaEngine, RealMediaEngine, VideoFrame,
};

#[test]
fn ffmpeg_args_decklink_sink() {
    let config = DecklinkConfig {
        device: "DeckLink SDI".to_string(),
        width: 1920,
        height: 1080,
        fps: 25,
        pix_fmt: "uyvy422".to_string(),
    };
    let args = ffmpeg_decklink_args(&config);
    assert_eq!(
        args,
        vec![
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            "1920x1080",
            "-r",
            "25",
            "-i",
            "-",
            "-f",
            "decklink",
            "-pix_fmt",
            "uyvy422",
            "DeckLink SDI",
        ]
    );
}

#[cfg(unix)]
#[test]
fn frame_pipe_gegen_echten_subprocess() {
    // `cat` als Sink: liest stdin, bis wir den Prozess droppen.
    let mut out = DecklinkOutput::spawn("cat", &[], 4, 4).unwrap();
    for i in 0..3u8 {
        let frame = VideoFrame::new(4, 4, vec![i; 4 * 4 * 4]);
        out.push_frame(&frame).unwrap();
    }
    assert_eq!(out.frames_pushed(), 3);
    assert_eq!(out.bytes_pushed(), 3 * 4 * 4 * 4);
    assert!(out.is_running());
    // Drop killt `cat` (kein Zombie) – danach ist der Prozess weg.
    let pid = out_pid(&out);
    drop(out);
    // Prozess existiert nicht mehr (kill + wait im Drop).
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!alive, "Subprocess {pid} wurde nicht beendet");
}

/// PID des Subprocess (für den Drop-Test).
#[cfg(unix)]
fn out_pid(out: &DecklinkOutput) -> u32 {
    out.pid()
}

#[cfg(unix)]
#[test]
fn falsche_frame_groesse_wird_abgelehnt() {
    let mut out = DecklinkOutput::spawn("cat", &[], 4, 4).unwrap();
    let wrong = VideoFrame::new(8, 8, vec![0; 8 * 8 * 4]);
    assert!(out.push_frame(&wrong).is_err());
    // Zähler bleibt unangetastet.
    assert_eq!(out.frames_pushed(), 0);
    // Richtige Größe geht wieder durch.
    let ok = VideoFrame::new(4, 4, vec![0; 4 * 4 * 4]);
    assert!(out.push_frame(&ok).is_ok());
    assert_eq!(out.frames_pushed(), 1);
}

#[test]
fn ffmpeg_null_sink_lifecycle() {
    // Gleiches rawvideo-Input-Setup wie Decklink, aber als Null-Sink –
    // läuft ohne Hardware. Skipped, wenn ffmpeg nicht installiert ist.
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("ffmpeg nicht gefunden – Test übersprungen");
        return;
    }
    let config = DecklinkConfig {
        width: 64,
        height: 64,
        fps: 25,
        ..Default::default()
    };
    // Decklink-Muxer durch null-Sink ersetzen: Output-Teil ab dem
    // zweiten „-f" (Output-Format) abschneiden, alles davor (rawvideo-
    // Input von stdin) bleibt gleich.
    let mut args = ffmpeg_decklink_args(&config);
    let idx = args.iter().rposition(|a| a == "-f").unwrap();
    args.truncate(idx);
    args.extend(["-f".to_string(), "null".to_string(), "-".to_string()]);

    let mut out = DecklinkOutput::spawn("ffmpeg", &args, 64, 64).unwrap();
    for _ in 0..5 {
        out.push_frame(&VideoFrame::new(64, 64, vec![0; 64 * 64 * 4]))
            .unwrap();
    }
    assert_eq!(out.frames_pushed(), 5);
    std::thread::sleep(Duration::from_millis(200));
    assert!(out.is_running(), "ffmpeg mit null-Sink sollte laufen");
}

// ---------------------------------------------------------------------
// Engine-Anbindung (RealMediaEngine → DecklinkOutput)
// ---------------------------------------------------------------------

/// Engine mit Decklink-Sink (`cat` als Pipe-Senke, ohne Hardware).
#[cfg(unix)]
fn engine_with_decklink(width: u32, height: u32) -> RealMediaEngine {
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    );
    engine.with_decklink(
        DecklinkOutput::spawn("cat", &[], width, height).unwrap(),
        "Stage 1".to_string(),
        25,
    )
}

#[cfg(unix)]
#[test]
fn engine_pusht_stage_frames_mit_decklink() {
    let mut engine = engine_with_decklink(4, 4);
    // Stage 4×4 konfigurieren (Canvas == Decklink-Größe).
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Stage 1", StageGeometry::new(4.0, 4.0)),
        })
        .unwrap();
    // Mehrere Ticks mit Real-Time-Abstand (Pacing: 25 fps → alle 40 ms).
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let pushed = engine.decklink_frames_pushed();
    assert!(pushed >= 2, "erwartet ≥2 Frames auf Decklink, got {pushed}");
}

#[cfg(unix)]
#[test]
fn engine_decklink_skaliert_bei_size_mismatch() {
    let mut engine = engine_with_decklink(4, 4);
    // Stage 8×8 ≠ Decklink 4×4 → Frames werden per Nearest-Neighbor skaliert.
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Stage 1", StageGeometry::new(8.0, 8.0)),
        })
        .unwrap();
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let pushed = engine.decklink_frames_pushed();
    assert!(pushed >= 2, "erwartet ≥2 skalierte Frames, got {pushed}");
}

#[test]
fn engine_ohne_decklink_pusht_nichts() {
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    );
    for _ in 0..3 {
        engine.tick();
    }
    assert_eq!(engine.decklink_frames_pushed(), 0);
}

#[test]
fn decklink_config_from_env() {
    // Nicht gesetzt → deaktiviert.
    std::env::remove_var("KUHPLAY_DECKLINK_DEVICE");
    assert!(DecklinkConfig::from_env().is_none());

    std::env::set_var("KUHPLAY_DECKLINK_DEVICE", "DeckLink Mini Monitor");
    std::env::set_var("KUHPLAY_DECKLINK_STAGE", "Stage 2");
    std::env::set_var("KUHPLAY_DECKLINK_WIDTH", "1280");
    std::env::set_var("KUHPLAY_DECKLINK_HEIGHT", "720");
    std::env::set_var("KUHPLAY_DECKLINK_FPS", "50");
    let (cfg, stage) = DecklinkConfig::from_env().unwrap();
    assert_eq!(cfg.device, "DeckLink Mini Monitor");
    assert_eq!(cfg.width, 1280);
    assert_eq!(cfg.height, 720);
    assert_eq!(cfg.fps, 50);
    assert_eq!(cfg.pix_fmt, "uyvy422");
    assert_eq!(stage, "Stage 2");

    std::env::remove_var("KUHPLAY_DECKLINK_DEVICE");
    std::env::remove_var("KUHPLAY_DECKLINK_STAGE");
    std::env::remove_var("KUHPLAY_DECKLINK_WIDTH");
    std::env::remove_var("KUHPLAY_DECKLINK_HEIGHT");
    std::env::remove_var("KUHPLAY_DECKLINK_FPS");
}
