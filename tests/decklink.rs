//! Integrationstests für die DeckLink-Ausgabe (`src/engine/decklink.rs` +
//! `decklink_native.rs`): Engine-Anbindung über die Capture-Senke (G6: kein
//! ffmpeg-Ausgabepfad mehr), Config-from-env, plus Live-Hardware-Test
//! (nur mit KUHPLAY_DECKLINK_LIVE=1 und --ignored, siehe G8).

use std::time::Duration;

use kuhplay::core::{OutputRoute, Region, RouteManager, Stage, StageGeometry};
use kuhplay::engine::decklink::{DecklinkConfig, DecklinkSink};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, MediaEngine, RealMediaEngine, VideoFrame,
};

// ---------------------------------------------------------------------------
// Engine-Anbindung (RealMediaEngine → DecklinkSink::Capture, ohne Hardware)
// ---------------------------------------------------------------------------

/// Engine mit Capture-Senke (zählt Frames, keine Hardware nötig).
fn engine_with_decklink() -> (RealMediaEngine, std::sync::Arc<std::sync::Mutex<u64>>) {
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    );
    let (sink, counter) = DecklinkSink::capture();
    (
        engine.with_decklink(sink, "Stage 1".to_string(), 25),
        counter,
    )
}

#[test]
fn engine_pusht_stage_frames_mit_decklink() {
    let (mut engine, counter) = engine_with_decklink();
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Stage 1", StageGeometry::new(4.0, 4.0)),
        })
        .unwrap();
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let pushed = engine.decklink_frames_pushed();
    assert!(
        pushed >= 2,
        "erwartet >=2 Frames auf Decklink, got {pushed}"
    );
    assert_eq!(pushed, *counter.lock().unwrap());
}

#[test]
fn engine_decklink_skaliert_bei_size_mismatch() {
    let (mut engine, _counter) = engine_with_decklink();
    // Stage 8×8 ≠ Capture-Senke – Skalierung passiert vor dem Push (Size-Check
    // greift nur bei Native; Capture zählt jedes Frame).
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Stage 1", StageGeometry::new(8.0, 8.0)),
        })
        .unwrap();
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(engine.decklink_frames_pushed() >= 2);
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
fn engine_pusht_route_bild_mit_decklink() {
    // DeckLink bekommt das Route-Bild (Stage → Region → Route).
    let (mut engine, _counter) = engine_with_decklink();
    let route = OutputRoute::new("SDI", 8, 8);
    let mut routes = RouteManager::new();
    routes.add(route.clone()).unwrap();
    let mut stage = Stage::new("Stage 1", StageGeometry::new(8.0, 8.0));
    let mut region = Region::full_stage(8.0, 8.0);
    region.route = Some(route.id);
    stage.regions = vec![region];
    engine
        .handle(EngineCommand::ConfigureStage { stage })
        .unwrap();
    engine
        .handle(EngineCommand::ConfigureRoutes { routes })
        .unwrap();
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let pushed = engine.decklink_frames_pushed();
    assert!(
        pushed >= 2,
        "erwartet >=2 Route-Bilder auf Decklink, got {pushed}"
    );
}

// ---------------------------------------------------------------------------
// Config aus Umgebungsvariablen
// ---------------------------------------------------------------------------

#[test]
fn decklink_config_from_env() {
    std::env::set_var("KUHPLAY_DECKLINK_DEVICE", "UltraStudio Monitor 3G");
    std::env::set_var("KUHPLAY_DECKLINK_WIDTH", "1920");
    std::env::set_var("KUHPLAY_DECKLINK_HEIGHT", "1080");
    std::env::set_var("KUHPLAY_DECKLINK_FPS", "25");
    let (config, stage) = DecklinkConfig::from_env().expect("env gesetzt");
    assert_eq!(config.device, "UltraStudio Monitor 3G");
    assert_eq!((config.width, config.height, config.fps), (1920, 1080, 25));
    assert_eq!(stage, "Stage 1");
    std::env::remove_var("KUHPLAY_DECKLINK_DEVICE");
    std::env::remove_var("KUHPLAY_DECKLINK_WIDTH");
    std::env::remove_var("KUHPLAY_DECKLINK_HEIGHT");
    std::env::remove_var("KUHPLAY_DECKLINK_FPS");
    assert!(
        DecklinkConfig::from_env().is_none(),
        "ohne DEVICE deaktiviert"
    );
}

// ---------------------------------------------------------------------------
// Live-Hardware-Tests (G8): nur mit KUHPLAY_DECKLINK_LIVE=1 und -- --ignored
// ---------------------------------------------------------------------------

/// G5/G8: Meldung, wenn Treiber fehlt bzw. 0 Geräte (z. B. nach macOS-Update).
#[test]
fn decklink_hint_bei_fehlendem_treiber() {
    if kuhplay::engine::decklink_native::available() {
        return; // auf dem Mac mit Karte überspringen
    }
    let (avail, devices, hint) = kuhplay::engine::decklink_native::devices_with_hint();
    assert!(!avail);
    assert!(devices.is_empty());
    assert!(hint.unwrap().contains("Desktop Video"));
}

/// G3/G4/G8 Live-Test (macOS/Linux, Feature `decklink`): UltraStudio Monitor 3G,
/// listet Geräte + Modi der Karte, startet Scheduled Playback und pusht 5 s
/// Testbalken; zählt late/dropped.
#[cfg(all(not(target_os = "windows"), feature = "decklink"))]
#[test]
#[ignore = "braucht echte Hardware + KUHPLAY_DECKLINK_LIVE=1"]
fn live_native_output_test_pattern() {
    if std::env::var("KUHPLAY_DECKLINK_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    use kuhplay::engine::decklink_native::{devices_with_hint, NativeOutput};
    let (avail, devices, hint) = devices_with_hint();
    println!("available={avail} devices={devices:?} hint={hint:?}");
    assert!(avail, "Treiber fehlt: {hint:?}");
    assert!(!devices.is_empty(), "Treiber da, aber 0 Geräte: {hint:?}");
    let config = DecklinkConfig {
        device: std::env::var("KUHPLAY_DECKLINK_DEVICE").unwrap_or_default(),
        width: 1920,
        height: 1080,
        fps: 25,
    };
    let mut out = NativeOutput::open(&config).expect("open");
    println!("geöffnet: {}", out.device_name());
    let modes = out.modes();
    println!("modi: {modes:?}");
    assert!(!modes.is_empty(), "Karte meldet keine Modi");
    // G4: Scheduled Playback starten (Karte als Taktgeber)
    out.start_scheduled().expect("start_scheduled");
    let frames = 125; // 5 s @ 25 fps
    for i in 0..frames {
        let mut rgba = vec![0u8; (1920 * 1080 * 4) as usize];
        let bar_x = (i * 16) % 1920;
        for y in 0..1080 {
            for x in bar_x..(bar_x + 100).min(1920) {
                let o = ((y * 1920 + x) * 4) as usize;
                rgba[o] = 235;
                rgba[o + 1] = 235;
                rgba[o + 2] = 235;
                rgba[o + 3] = 255;
            }
        }
        out.push_frame(&VideoFrame::new(1920, 1080, rgba))
            .expect("push");
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_millis(500)); // auslaufen lassen
    assert_eq!(out.frames_pushed(), frames as u64);
    let (late, dropped, queued) = out.stats();
    println!("stats: late={late} dropped={dropped} queued={queued}");
    assert_eq!(dropped, 0, "keine verlorenen Frames erwartet");
    println!(
        "OK: {frames} Frames auf {} ausgegeben (scheduled)",
        out.device_name()
    );
}

/// G8 Live-Test Windows (COM): wie der native Test, über decklink_com.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "braucht echte DeckLink-Hardware + KUHPLAY_DECKLINK_LIVE=1"]
fn live_com_output_test_pattern() {
    if std::env::var("KUHPLAY_DECKLINK_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    use kuhplay::engine::decklink_native::{devices_with_hint, NativeOutput};
    let (avail, devices, hint) = devices_with_hint();
    println!("available={avail} devices={devices:?} hint={hint:?}");
    assert!(avail, "Treiber fehlt: {hint:?}");
    assert!(!devices.is_empty());
    let config = DecklinkConfig {
        device: std::env::var("KUHPLAY_DECKLINK_DEVICE").unwrap_or_default(),
        width: 1920,
        height: 1080,
        fps: 25,
    };
    let mut out = NativeOutput::open(&config).expect("open");
    out.start_scheduled().expect("start_scheduled");
    let frames = 125;
    for i in 0..frames {
        let mut rgba = vec![0u8; (1920 * 1080 * 4) as usize];
        let bar_x = (i * 16) % 1920;
        for y in 0..1080 {
            for x in bar_x..(bar_x + 100).min(1920) {
                let o = ((y * 1920 + x) * 4) as usize;
                rgba[o] = 235;
                rgba[o + 1] = 235;
                rgba[o + 2] = 235;
                rgba[o + 3] = 255;
            }
        }
        out.push_frame(&VideoFrame::new(1920, 1080, rgba))
            .expect("push");
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(out.frames_pushed(), frames as u64);
    let (late, dropped, queued) = out.stats();
    println!("stats: late={late} dropped={dropped} queued={queued}");
    println!(
        "OK: {frames} Frames auf {} ausgegeben (scheduled)",
        out.device_name()
    );
}

/// G8 Dauerlauf auf dem Mac (UltraStudio Monitor 3G): 30 s rotierende
/// Farbalken, Scheduled Playback; danach dropped == 0 und alle Frames raus.
/// Key/Fill kann diese Karte nicht (nur Monitor-Ausgang) – offen notiert.
#[cfg(all(not(target_os = "windows"), feature = "decklink"))]
#[test]
#[ignore = "Dauerlauf: echte Hardware + KUHPLAY_DECKLINK_LIVE=1"]
fn live_native_soak_color_bars() {
    if std::env::var("KUHPLAY_DECKLINK_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    use kuhplay::engine::decklink_native::NativeOutput;
    let config = DecklinkConfig {
        device: std::env::var("KUHPLAY_DECKLINK_DEVICE").unwrap_or_default(),
        width: 1920,
        height: 1080,
        fps: 25,
    };
    let mut out = NativeOutput::open(&config).expect("open");
    out.start_scheduled().expect("scheduled");
    let frames = 750; // 30 s @ 25 fps
    let colors = [
        [235u8, 235, 235], // weiß
        [235, 235, 0],     // gelb
        [0, 235, 235],     // cyan
        [0, 235, 0],       // grün
        [235, 0, 235],     // magenta
        [235, 0, 0],       // rot
        [0, 0, 235],       // blau
        [0, 0, 0],         // schwarz
    ];
    for i in 0..frames {
        let c = colors[(i / 25) % colors.len()]; // 1 s pro Farbe
        let rgba = vec![c[0], c[1], c[2], 255].repeat(1920 * 1080);
        out.push_frame(&VideoFrame::new(1920, 1080, rgba))
            .expect("push");
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_secs(2)); // Queue auslaufen lassen
    let (late, dropped, queued) = out.stats();
    println!(
        "soak: frames={} late={late} dropped={dropped} queued={queued}",
        out.frames_pushed()
    );
    assert_eq!(out.frames_pushed(), frames as u64);
    assert_eq!(dropped, 0, "keine Bildaussetzer (dropped) erwartet");
    println!(
        "OK: Dauerlauf 30 s ohne Aussetzer auf {}",
        out.device_name()
    );
}
