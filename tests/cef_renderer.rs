//! Echter CEF-Renderer durch die Real-Engine (HTML5-Cue auf einer Stage).
//!
//! Läuft, wenn das Renderer-Bundle gebaut ist – sonst sauberer SKIP:
//!
//! ```text
//! cargo build --features cef && cargo run --features cef --bin kuhplay-cef-bundle
//! cargo test --features cef --test cef_renderer -- --nocapture
//! ```
//!
//! Prüft: Bild in Stage-Größe, Alpha (transparent + teiltransparent),
//! laufende Animation mit Bildrate, Prozess und Shared Memory weg nach Stop.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kuhplay::core::{HtmlSource, Stage, StageGeometry};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, MediaEngine, RealMediaEngine,
};

/// Renderer aus `target/<profil>/cef/` (von `kuhplay-cef-bundle`) oder
/// `KUHPLAY_CEF_BIN`.
fn renderer() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KUHPLAY_CEF_BIN") {
        return Some(PathBuf::from(path));
    }
    // Test-Binary liegt in target/<profil>/deps/.
    let profile_dir = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .to_path_buf();
    let bin = if cfg!(target_os = "macos") {
        profile_dir.join("cef/kuhplay-cef.app/Contents/MacOS/kuhplay-cef")
    } else {
        profile_dir.join(format!("cef/kuhplay-cef{}", std::env::consts::EXE_SUFFIX))
    };
    bin.is_file().then_some(bin)
}

const PAGE: &str = r#"<html><body style="margin:0;background:transparent">
<div style="position:absolute;left:0;top:0;width:25%;height:100%;background:rgba(255,0,0,0.5)"></div>
<div id="box" style="position:absolute;top:40%;width:10%;height:20%;background:#0080ff;
  animation:move 1s linear infinite alternate"></div>
<style>@keyframes move { from { left: 30% } to { left: 85% } }</style>
</body></html>"#;

#[test]
fn html_cue_renders_with_alpha_and_animation_through_the_engine() {
    let Some(bin) = renderer() else {
        println!("SKIP: CEF-Renderer nicht gebaut (kuhplay-cef-bundle ausführen)");
        return;
    };
    let frames = new_frame_store();
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        frames.clone(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_renderer_bin(bin);
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(1280.0, 720.0)),
        })
        .unwrap();
    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::Inline(PAGE.into()),
            stage: Some("Main".to_string()),
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // Erstes Bild in Stage-Größe abwarten (CEF-Start dauert etwas).
    let deadline = Instant::now() + Duration::from_secs(20);
    let first = loop {
        engine.tick();
        let frame = frames.lock().unwrap().get(&cue_id).cloned();
        if let Some(frame) = frame.filter(|f| (f.width, f.height) == (1280, 720)) {
            break frame;
        }
        assert!(Instant::now() < deadline, "kein CEF-Bild in Stage-Größe");
        std::thread::sleep(Duration::from_millis(5));
    };
    let px = |x: usize, y: usize| {
        let i = (y * 1280 + x) * 4;
        [
            first.rgba[i],
            first.rgba[i + 1],
            first.rgba[i + 2],
            first.rgba[i + 3],
        ]
    };
    let red = px(100, 100);
    assert!(
        red[0] > 240 && red[1] < 15 && (120..=135).contains(&red[3]),
        "halbtransparentes Rot (straight alpha): {red:?}"
    );
    assert_eq!(px(700, 50)[3], 0, "Hintergrund voll transparent");

    // Animation: neue Bilder kommen laufend an.
    let (mut changes, start) = (0u32, Instant::now());
    let mut last = std::sync::Arc::as_ptr(&first);
    while start.elapsed() < Duration::from_secs(3) {
        engine.tick();
        if let Some(frame) = frames.lock().unwrap().get(&cue_id) {
            if std::sync::Arc::as_ptr(frame) != last {
                last = std::sync::Arc::as_ptr(frame);
                changes += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let fps = changes as f64 / start.elapsed().as_secs_f64();
    println!("CEF über die Engine: {fps:.1} Bilder/s bei 1280x720");
    // Release: volle CEF-Bildrate (60). Debug: Pixel-Umrechnung unoptimiert,
    // auf CI-VMs ~9 fps – hier zählt nur, dass die Animation weiterläuft.
    let min = if cfg!(debug_assertions) { 5.0 } else { 50.0 };
    assert!(fps >= min, "mindestens {min} Bilder/s, waren {fps:.1}");

    // Stop: Renderer beendet, Shared-Memory-Datei aufgeräumt.
    engine.handle(EngineCommand::Stop { cue_id }).unwrap();
    engine.tick();
    let leftover = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .contains(&cue_id.to_string())
        });
    assert!(!leftover, "Shared-Memory-Datei nach Stop gelöscht");
}
