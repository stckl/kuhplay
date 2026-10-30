//! Integrationstests für das Video-Routing (QLab 5: Stage → Region →
//! Output Route → Device).
//!
//! Phase 1: Migration alter Shows. Das feste JSON-Beispiel
//! `tests/fixtures/legacy-screen-show.kuhplay` enthält eine Show im alten
//! Format (Screens + `Stage::screen`); es bleibt unverändert, damit die
//! Migration auch künftig geprüft bleibt.

use kuhplay::core::{DeviceArea, DeviceRef, DisplayRefInfo, ShowDocument, StageGeometry};

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
