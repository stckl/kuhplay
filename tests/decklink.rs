//! Integrationstests für die DeckLink-Ausgabe (`src/engine/decklink.rs` +
// Ohne decklink-Feature sind die Live-Tests cfg-entfernt → Imports ungenutzt.
#![cfg_attr(not(feature = "decklink"), allow(unused_imports))]
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
        let t0 = std::time::Instant::now();
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
        pace_frame(t0, 40);
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
        let t0 = std::time::Instant::now();
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

/// H5: Schlaft nur die Restzeit des Frame-Intervalls (statt festem Sleep), damit
/// das Schieben der Frames das Bild-Taktbudget nicht reißt (sonst late/dropped).
fn pace_frame(t0: std::time::Instant, frame_ms: u64) {
    let el = t0.elapsed();
    let budget = Duration::from_millis(frame_ms);
    if el < budget {
        std::thread::sleep(budget - el);
    }
}

/// H5: SMPTE-Farbalken (8 vertikale Balken, 75% RGB-Werte) als RGBA 1920×1080.
fn color_bars_rgba() -> Vec<u8> {
    let colors = [
        [235u8, 235, 235], // weiß
        [235, 235, 0],     // gelb
        [0, 235, 235],     // cyan
        [0, 235, 0],       // grün
        [235, 0, 235],     // magenta
        [235, 0, 0],       // rot
        [0, 0, 235],       // blau
        [16, 16, 16],      // schwarz (Referenzpegel)
    ];
    let (w, h) = (1920usize, 1080usize);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = colors[x * colors.len() / w];
            let o = (y * w + x) * 4;
            rgba[o] = c[0];
            rgba[o + 1] = c[1];
            rgba[o + 2] = c[2];
            rgba[o + 3] = 255;
        }
    }
    rgba
}

/// H5 (Host, ohne Hardware): Farbalken nach UYVY (BT.709) konvertieren und die
/// Y/Cb/Cr-Werte der Balkenmitten gegen die Sollwerte rechnen – das ist die
/// Farbkontrolle des Testbilds auf Pixel-Ebene.
#[test]
fn farbalken_uyvy_farben_passen() {
    use kuhplay::engine::decklink::{rgba_to_uyvy422_cs, ColorSpace};
    let rgba = color_bars_rgba();
    let (w, h) = (1920usize, 1080usize);
    let mut uyvy = vec![0u8; w * h * 2];
    rgba_to_uyvy422_cs(
        &VideoFrame::new(w as u32, h as u32, rgba.clone()),
        &mut uyvy,
        ColorSpace::Bt709,
    );
    let rgb_bars = [
        [235u8, 235, 235],
        [235, 235, 0],
        [0, 235, 235],
        [0, 235, 0],
        [235, 0, 235],
        [235, 0, 0],
        [0, 0, 235],
        [16, 16, 16],
    ];
    for (i, &[r, g, b]) in rgb_bars.iter().enumerate() {
        let x = (2 * i + 1) * w / 16; // Balkenmitte
        let y = h / 2;
        let o = (y * w + x) * 2; // UYVY: 2 Pixel = 4 Byte
        let (u, y0, v) = (uyvy[o], uyvy[o + 1], uyvy[o + 2]);
        // BT.709 Studio-Range, normiert 0..1 (wie rgba_to_uyvy422_cs Bt709)
        let ef = |r: u8, g: u8, b: u8| -> (u8, u8, u8) {
            let (r, g, b) = (
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
            );
            let (kr, kg, kb) = (0.2126, 0.7152, 0.0722);
            let y = 16.0 + 219.0 * (kr * r + kg * g + kb * b);
            let cb = 128.0
                + 224.0 * (-kr / (2.0 * (1.0 - kb)) * r - kg / (2.0 * (1.0 - kb)) * g + 0.5 * b);
            let cr = 128.0
                + 224.0 * (0.5 * r - kg / (2.0 * (1.0 - kr)) * g - kb / (2.0 * (1.0 - kr)) * b);
            (y as u8, cb as u8, cr as u8)
        };
        let (ey, ecb, ecr) = ef(r, g, b);
        assert!(
            (i32::from(y0) - i32::from(ey)).abs() <= 3
                && (i32::from(u) - i32::from(ecb)).abs() <= 3
                && (i32::from(v) - i32::from(ecr)).abs() <= 3,
            "Balken {i} RGB({r},{g},{b}): got YUV({y0},{u},{v}), erwartet ({ey},{ecb},{ecr})"
        );
    }
}

/// H5 Dauerlauf (UltraStudio Monitor 3G): mindestens 30 Minuten
/// (KUHPLAY_DECKLINK_SOAK_SECS, Default 1800) SMPTE-Farbalken per Scheduled
/// Playback; danach dropped == 0 und alle Frames raus. Zusätzlich Sichtprüfung
/// am angeschlossenen Monitor: Balken müssen sauber und farbrichtig stehen.
/// Key/Fill kann diese Karte nicht (nur Monitor-Ausgang) – offen notiert.
#[cfg(feature = "decklink")]
#[test]
#[ignore = "Dauerlauf: echte Hardware + KUHPLAY_DECKLINK_LIVE=1"]
fn live_native_soak_color_bars() {
    if std::env::var("KUHPLAY_DECKLINK_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    use kuhplay::engine::decklink_native::NativeOutput;
    let fps = 25u32;
    let secs: u64 = std::env::var("KUHPLAY_DECKLINK_SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800); // H5: mindestens 30 Minuten
    let config = DecklinkConfig {
        device: std::env::var("KUHPLAY_DECKLINK_DEVICE").unwrap_or_default(),
        width: 1920,
        height: 1080,
        fps,
    };
    let mut out = NativeOutput::open(&config).expect("open");
    out.start_scheduled().expect("scheduled");
    let frames = secs * u64::from(fps);
    let bars = color_bars_rgba();
    let frame_ms = 1000 / u64::from(fps);
    for i in 0..frames {
        let t0 = std::time::Instant::now();
        let mut rgba = bars.clone();
        // bewegter Marker (1 s wandert über das Bild) gegen Einfrieren
        let mx = ((i * 8) % 1920) as usize;
        for y in 0..64usize {
            for x in mx..(mx + 64).min(1920) {
                let o = (y * 1920 + x) * 4;
                rgba[o] = 255;
                rgba[o + 1] = 255;
                rgba[o + 2] = 255;
                rgba[o + 3] = 255;
            }
        }
        out.push_frame(&VideoFrame::new(1920, 1080, rgba))
            .expect("push");
        pace_frame(t0, frame_ms);
    }
    std::thread::sleep(Duration::from_secs(2)); // Queue auslaufen lassen
    let (late, dropped, queued) = out.stats();
    println!(
        "soak: secs={secs} frames={} late={late} dropped={dropped} queued={queued}",
        out.frames_pushed()
    );
    println!(
        "SICHTPRÜFUNG: am Ausgang müssen 8 saubere Farbalken + wandernder weißer Marker stehen."
    );
    assert_eq!(out.frames_pushed(), frames);
    assert_eq!(dropped, 0, "keine Bildaussetzer (dropped) erwartet");
    println!(
        "OK: Dauerlauf {secs} s ohne Aussetzer auf {}",
        out.device_name()
    );
}

/// H5: zusätzlich 1080p29,97 und 1080p50 (soweit die Karte den Modus kann),
/// je 10 s Farbalken per Scheduled Playback, dropped == 0.
#[cfg(feature = "decklink")]
#[test]
#[ignore = "braucht echte Hardware + KUHPLAY_DECKLINK_LIVE=1"]
fn live_native_modes_2997_50() {
    if std::env::var("KUHPLAY_DECKLINK_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    use kuhplay::engine::decklink_native::NativeOutput;
    let bars = color_bars_rgba();
    for (fps, num, den) in [(2997u32, 30000u32, 1001u32), (50, 50000, 1000)] {
        let config = DecklinkConfig {
            device: std::env::var("KUHPLAY_DECKLINK_DEVICE").unwrap_or_default(),
            width: 1920,
            height: 1080,
            fps,
        };
        let mut out = match NativeOutput::open(&config) {
            Ok(o) => o,
            Err(e) => {
                println!("1080p{fps}: open fehlgeschlagen ({e}) – übersprungen");
                continue;
            }
        };
        let kann = out
            .modes()
            .iter()
            .any(|m| m.width == 1920 && m.height == 1080 && m.fps_num == num && m.fps_den == den);
        if !kann {
            println!("1080p{fps}: Modus nicht von der Karte unterstützt – übersprungen");
            continue;
        }
        out.start_scheduled().expect("scheduled");
        let frames = 10 * u64::from(num) / u64::from(den); // 10 s
        let frame_ms = 1000 * u64::from(den) / u64::from(num);
        for _ in 0..frames {
            let t0 = std::time::Instant::now();
            out.push_frame(&VideoFrame::new(1920, 1080, bars.clone()))
                .expect("push");
            pace_frame(t0, frame_ms);
        }
        std::thread::sleep(Duration::from_secs(2));
        let (late, dropped, queued) = out.stats();
        println!(
            "1080p{fps}: frames={} late={late} dropped={dropped} queued={queued}",
            out.frames_pushed()
        );
        assert_eq!(out.frames_pushed(), frames);
        assert_eq!(dropped, 0, "1080p{fps}: keine Aussetzer erwartet");
        println!("OK: 1080p{fps} (10 s) sauber auf {}", out.device_name());
    }
}
