//! Integrationstests für das Video-Routing (Stage → Region →
//! Output Route → Device).
//!
//! Phase 1: Migration alter Shows. Das feste JSON-Beispiel
//! `tests/fixtures/legacy-screen-show.kuhplay` enthält eine Show im alten
//! Format (Screens + `Stage::screen`); es bleibt unverändert, damit die
//! Migration auch künftig geprüft bleibt.

use std::sync::Arc;

use kuhplay::core::{
    DeviceArea, DeviceRef, DisplayRefInfo, OutputRoute, Region, ShowDocument, StageGeometry,
};

fn display(id: u32, name: &str, x: i32, w: u32, h: u32, scale: f32) -> DisplayRefInfo {
    DisplayRefInfo {
        id,
        name: name.to_string(),
        x,
        y: 0,
        width: w,
        height: h,
        scale_factor: scale,
        frequency: 60.0,
    }
}

/// Lädt das fest committete Legacy-Beispiel (altes Screen-Format).
fn load_legacy_fixture() -> ShowDocument {
    let json = std::fs::read_to_string("tests/fixtures/legacy-screen-show.kuhplay")
        .expect("fixture existiert im Repo");
    ShowDocument::from_json(&json).expect("Legacy-Show lädt")
}

#[test]
fn legacy_fixture_file_migrates_to_routes_and_regions() {
    let mut doc = load_legacy_fixture();
    assert!(
        doc.needs_screen_migration(),
        "Fixture ist noch im alten Format"
    );
    assert_eq!(doc.screens.len(), 1);
    assert_eq!(doc.routes.len(), 0);

    // Auf dem Rechner dieser Show: ein Display, das den Screen deckt.
    doc.migrate_legacy_screens(&[display(7, "Main", 0, 1920, 1080, 1.0)]);

    // Screen wurde zu einer Route auf dem Display.
    assert_eq!(doc.routes.len(), 1);
    let route = doc.routes.routes()[0].clone();
    assert_eq!(route.name, "Beamer Links");
    assert_eq!((route.width, route.height), (1920, 1080));
    assert_eq!(route.device_area, DeviceArea::Full);
    assert!(route.test_pattern, "Testbild-Einstellung übernommen");
    assert_eq!(route.color, [255, 64, 64]);
    assert!(matches!(
        route.device,
        Some(DeviceRef::Display { id: 7, .. })
    ));

    // Stage 1 hat jetzt eine Region über die ganze Fläche auf die Route.
    let stage = doc.stages.find("Stage 1").expect("Stage 1");
    assert_eq!(stage.screen, None);
    assert_eq!(stage.geometry, StageGeometry::new(1920.0, 1080.0));
    assert_eq!(stage.regions.len(), 1);
    assert_eq!(stage.regions[0].route, Some(route.id));
    assert!((stage.regions[0].width - 1920.0).abs() < 0.5);

    // Speichern schreibt nur noch das neue Modell …
    let saved = doc.to_json().unwrap();
    assert!(!saved.contains("\"screens\""));
    assert!(!saved.contains("\"screen\""));
    assert!(saved.contains("\"routes\""));
    // … und das gespeicherte Ergebnis lädt ohne erneute Migration.
    let reloaded = ShowDocument::from_json(&saved).unwrap();
    assert!(!reloaded.needs_screen_migration());
    assert_eq!(reloaded.routes.len(), 1);
    assert_eq!(reloaded.stages.find("Stage 1").unwrap().regions.len(), 1);
}

#[test]
fn legacy_fixture_without_matching_display_keeps_running() {
    let mut doc = load_legacy_fixture();
    // Anderer Rechner: kein Display passt zur Screen-Geometrie.
    doc.migrate_legacy_screens(&[display(1, "Laptop", -3000, 1512, 982, 2.0)]);
    assert_eq!(doc.routes.len(), 1);
    let route = doc.routes.routes()[0].clone();
    assert_eq!(route.device, None, "fehlendes Gerät");
    // Die Show läuft weiter und die Route lässt sich neu zuweisen.
    assert!(route.validate().is_ok());
    let stage = doc.stages.find("Stage 1").unwrap();
    assert_eq!(stage.regions.len(), 1, "Region bleibt an der Route");
}

// ---------------------------------------------------------------------
// Performance (Phase 3): Bilder pro Sekunde im Release-Build.
// Aufruf: `cargo test --release --test video_routing -- --ignored --nocapture`
// ---------------------------------------------------------------------

#[test]
#[ignore = "Nur auf Anfrage im Release-Build messen"]
fn route_render_benchmark_1080p_and_5120x1440() {
    use kuhplay::engine::frame::VideoFrame;
    use kuhplay::engine::route_renderer::{auto_blends, render_route};
    use std::time::Instant;

    /// Stage-Frame mit halbbewegtem Inhalt (harte Kanten, keine Trivial-
    /// Optimierung durch gleiche Zeilen).
    fn noisy_frame(w: u32, h: u32, seed: u32) -> Arc<VideoFrame> {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        let mut x = seed;
        for _ in 0..w * h {
            // xorshift für pseudo-zufällige, aber reproduzierbare Pixel.
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            rgba.extend_from_slice(&[(x & 0xff) as u8, ((x >> 8) & 0xff) as u8, 0, 255]);
        }
        Arc::new(VideoFrame::new(w, h, rgba))
    }

    for (name, w, h, stage_w, stage_h) in [
        ("1080p", 1920u32, 1080u32, 1920.0f32, 1080.0f32),
        ("5120x1440", 5120, 1440, 5120.0, 1440.0),
    ] {
        let route = OutputRoute::new(name, w, h);
        let mut stage =
            kuhplay::core::Stage::new("S", kuhplay::core::StageGeometry::new(stage_w, stage_h));
        let mut region = Region::full_stage(stage_w, stage_h);
        region.route = Some(route.id);
        stage.regions = vec![region];
        let blends = auto_blends(&stage.regions);
        let frames: std::collections::HashMap<String, Arc<VideoFrame>> =
            std::iter::once(("S".to_string(), noisy_frame(w, h, name.len() as u32))).collect();
        // Warmlaufen (Allocationen), dann 30 Bilder messen.
        for _ in 0..5 {
            let _ = render_route(&route, std::slice::from_ref(&stage), &frames);
        }
        let _ = &blends; // Blend-Berechnung ist Teil des Renderpfads
        let runs = 30u32;
        let start = Instant::now();
        for _ in 0..runs {
            let _ = render_route(&route, std::slice::from_ref(&stage), &frames);
        }
        let elapsed = start.elapsed().as_secs_f32();
        let fps = runs as f32 / elapsed;
        println!("{name}: {fps:.1} frames/s (route {w}x{h})");
        assert!(fps > 0.0);
    }
}
