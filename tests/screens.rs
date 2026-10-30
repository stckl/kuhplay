//! Integrationstests für das **Screen-Feature** (QLab 5 „Video → Screens").
//!
//! * **Geometrie:** Screens als frei definierbare Rechtecke im globalen
//!   Display-Koordinatenraum – Teile eines Displays, über mehrere
//!   Displays gespannt, mehrere Screens pro Display.
//! * **Drehung:** 90/180/270° vertauscht den Fußabdruck (Breite/Höhe),
//!   die Drehung ist eine Display-Eigenschaft (Inhalt dreht sich mit).
//! * **Engine-Bridge:** `SetTestPattern` wird von Mock- und Real-Engine
//!   entgegengenommen (kein Panic, Event-Loop bleibt antworten).
//! * **Stage→Screen:** Zuordnung per ID, Screen-Label/-Spec für das
//!   Testbild, Screen-Löschen (die GUI löst dangling References auf).

use std::time::{Duration, Instant};

use kuhplay::core::{DisplayGeometry, Screen, ScreenManager, ScreenRotation, Stage, StageGeometry};
use kuhplay::engine::{spawn_mock_engine, spawn_real_engine, EngineCommand, EngineEvent};

// ---------------------------------------------------------------------
// Test-Double für Displays (plattform-unabhängig)
// ---------------------------------------------------------------------

/// Test-Display (nur die geometrischen Felder, siehe [`DisplayGeometry`]).
struct TestDisplay {
    name: String,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl DisplayGeometry for TestDisplay {
    fn display_name(&self) -> &str {
        &self.name
    }
    fn display_x(&self) -> i32 {
        self.x
    }
    fn display_y(&self) -> i32 {
        self.y
    }
    fn display_width(&self) -> u32 {
        self.w
    }
    fn display_height(&self) -> u32 {
        self.h
    }
}

fn display(name: &str, x: i32, y: i32, w: u32, h: u32) -> TestDisplay {
    TestDisplay {
        name: name.to_string(),
        x,
        y,
        w,
        h,
    }
}

/// Wartet, bis `cond` erfüllt ist (Polling alle 5 ms).
fn wait_until(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cond() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------------------------------------------------------------------
// Geometrie
// ---------------------------------------------------------------------

#[test]
fn screen_covers_part_of_a_display() {
    // Display 1920×1080 bei (0,0); Screen = rechte Hälfte (AoI-Setup).
    let s = Screen::new("Right", 960, 0, 960, 1080);
    assert_eq!(s.rect(), (960, 0, 960, 1080));
    assert!(s.intersects(0, 0, 1920, 1080), "überlappt das Display");
    assert!(!s.covers(0, 0, 1920, 1080), "deckt Display nur teilweise");
    assert!(s.covers(960, 0, 960, 1080), "deckt eigene Fläche");
    // Außerhalb des Displays: kein Schnitt.
    assert!(!s.intersects(0, 0, 960, 1080));
}

#[test]
fn screen_spans_multiple_displays() {
    // Zwei Displays nebeneinander: (0,0) 1920×1080 und (1920,0) 1920×1080.
    // Ein Screen über beide (z. B. breite LED-Wand).
    let s = Screen::new("Wide", 960, 0, 1920, 1080);
    assert!(
        s.intersects(0, 0, 1920, 1080),
        "linkes Display angeschnitten"
    );
    assert!(
        s.intersects(1920, 0, 1920, 1080),
        "rechtes Display angeschnitten"
    );
    assert!(!s.covers(0, 0, 1920, 1080));
    assert!(!s.covers(1920, 0, 1920, 1080));
    // Bounding-Box aller Displays für den Editor.
    let displays = [
        display("Left", 0, 0, 1920, 1080),
        display("Right", 1920, 0, 1920, 1080),
    ];
    assert_eq!(ScreenManager::bounding_box(&displays), (0, 0, 3840, 1080));
}

#[test]
fn multiple_screens_per_display_do_not_overlap() {
    // Display 1920×1080: zwei Screens (links/rechts) + einer darüber.
    let left = Screen::new("Left", 0, 0, 960, 1080);
    let right = Screen::new("Right", 960, 0, 960, 1080);
    let top = Screen::new("Top", 0, 0, 1920, 270);
    assert!(!left.intersects(right.x, right.y, right.width, right.height));
    assert!(
        left.intersects(top.x, top.y, top.width, top.height),
        "Top überlappt Left"
    );
    let mut m = ScreenManager::new();
    m.add(left.clone()).unwrap();
    m.add(right.clone()).unwrap();
    m.add(top.clone()).unwrap();
    assert_eq!(m.len(), 3);
    assert_eq!(m.find(left.id).unwrap().name, "Left");
    // Doppelte Namen sind erlaubt (nur IDs eindeutig).
    let dup = Screen::new("Left", 0, 1080, 100, 100);
    m.add(dup).unwrap();
    assert_eq!(
        m.find_by_name("Left").unwrap().id,
        left.id,
        "erster Treffer"
    );
}

#[test]
fn screen_rotation_swaps_footprint() {
    // 1920×1080-Canvas auf einem 90°-gedrehten Screen (hochkant montierter
    // Projektor / LED-Wand): Fußabdruck 1080×1920.
    let mut s = Screen::new("Portrait", 0, 0, 1920, 1080);
    assert_eq!(s.rotation.rotated_size(s.width, s.height), (1920, 1080));
    s.rotation = ScreenRotation::Deg90;
    assert_eq!(s.rotation.rotated_size(s.width, s.height), (1080, 1920));
    s.rotation = ScreenRotation::Deg180;
    assert_eq!(s.rotation.rotated_size(s.width, s.height), (1920, 1080));
    s.rotation = ScreenRotation::Deg270;
    assert_eq!(s.rotation.rotated_size(s.width, s.height), (1080, 1920));
    // Drehung ist umkehrbar (zyklisch).
    let mut r = ScreenRotation::Deg0;
    for _ in 0..4 {
        r = r.next();
    }
    assert_eq!(r, ScreenRotation::Deg0);
    // Testbild-Beschriftung trägt Name + (nominal) Größe.
    s.rotation = ScreenRotation::Deg0;
    assert_eq!(s.label(), "Portrait 1920x1080");
    assert_eq!(s.pattern_spec().label, "Portrait 1920x1080");
}

#[test]
fn manager_from_displays_creates_one_screen_per_display() {
    let displays = [
        display("Built-in", 0, 0, 1512, 982),
        display("Beamer", 1512, 0, 1920, 1080),
    ];
    let m = ScreenManager::from_displays(&displays);
    assert_eq!(m.len(), 2);
    assert_eq!(
        m.names(),
        vec!["Built-in".to_string(), "Beamer".to_string()]
    );
    assert_eq!(m.screens()[0].rect(), (0, 0, 1512, 982));
    assert_eq!(m.screens()[1].rect(), (1512, 0, 1920, 1080));
    // Default: keine Drehung, kein Testbild.
    assert_eq!(m.screens()[0].rotation, ScreenRotation::Deg0);
    assert!(!m.screens()[0].test_pattern);
}

// ---------------------------------------------------------------------
// Stage → Screen
// ---------------------------------------------------------------------

#[test]
fn stage_screen_assignment_roundtrip() {
    let screen = Screen::new("Main", 0, 0, 1920, 1080);
    let mut stage = Stage::new("Stage 1", StageGeometry::new(1920.0, 1080.0));
    assert!(stage.screen.is_none(), "Default: kein Screen");
    stage.screen = Some(screen.id);
    // Persistenz (Show-Datei): Screen ist ein Legacy-Feld – es wird gelesen
    // (siehe unten), aber nicht mehr geschrieben; stattdessen migriert
    // `migrate_legacy_screens` die Zuordnung in Regionen + Routen.
    let json = serde_json::to_string(&stage).unwrap();
    let back: Stage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.screen, None, "Legacy-Feld wird nicht mehr gespeichert");
    // Legacy-Shows mit screen-Feld laden weiter.
    let legacy = format!(
        r#"{{"id":"{}","name":"Old","geometry":{{"x":0.0,"y":0.0,"width":1920.0,"height":1080.0,"scale":1.0}},"layers":0,"screen":"{}"}}"#,
        stage.id, screen.id
    );
    let old: Stage = serde_json::from_str(&legacy).unwrap();
    assert_eq!(old.screen, Some(screen.id));
}

#[test]
fn deleting_screen_leaves_no_dangling_stage_reference_in_manager() {
    // Screen löschen → Stage-Zuordnung ist fachlich ungültig; die GUI
    // setzt `stage.screen = None`. Hier: Verhalten des Managers prüfen.
    let mut m = ScreenManager::new();
    let s = Screen::new("Tmp", 0, 0, 100, 100);
    let id = s.id;
    m.add(s).unwrap();
    assert!(m.remove(id));
    assert!(m.find(id).is_none(), "Screen weg");
    assert!(!m.remove(id), "doppelt löschen");
    // Ein Stage, der noch die ID hält, findet keinen Screen mehr
    // (GUI löst das als „— kein —" auf).
    let stage_screen: Option<uuid::Uuid> = Some(id);
    assert!(stage_screen.and_then(|sid| m.find(sid)).is_none());
}

// ---------------------------------------------------------------------
// Engine-Bridge (Testbild-Kommando)
// ---------------------------------------------------------------------

#[test]
fn mock_engine_accepts_set_test_pattern_and_stays_alive() {
    let engine = spawn_mock_engine();
    let screen = Screen::new("Main", 0, 0, 1920, 1080);
    let mut with_pattern = screen.clone();
    with_pattern.test_pattern = true;
    // Testbild an …
    engine
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: Some(with_pattern.pattern_spec()),
        })
        .expect("send pattern on");
    // … und aus.
    engine
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: None,
        })
        .expect("send pattern off");
    // Event-Loop antwortet weiterhin (kein Panic, kein Drop).
    engine.send(EngineCommand::Ping(7)).expect("send ping");
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(EngineEvent::Pong(7)) = engine.try_recv_event() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("engine must answer Ping with Pong after SetTestPattern");
}

#[test]
fn real_engine_set_test_pattern_marks_stage_dirty() {
    // Real-Engine ohne Media-Hardware: Compositor zeichnet das Testbild
    // in das Stage-Canvas (1-px-Rand + Raster), sobald die Stage
    // dem Screen zugeordnet ist.
    let engine = spawn_real_engine();
    let screen = Screen::new("TestScreen", 0, 0, 64, 64);
    let mut stage = Stage::new("PatternStage", StageGeometry::new(64.0, 64.0));
    stage.screen = Some(screen.id);
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("send configure");

    // Testbild an → Canvas hat den 1-px-Rand exakt am Bildrand.
    let spec = screen.pattern_spec();
    engine
        .commands
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: Some(spec),
        })
        .expect("send pattern on");
    let ok = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            let Some(frame) = store.get("PatternStage") else {
                return false;
            };
            let inside = (2 * 64 + 20) * 4;
            frame.rgba[20 * 4] == 255 // Rand oben
                && frame.rgba[inside] < 100 // direkt daneben dunkel
        },
        Duration::from_secs(3),
    );
    assert!(ok, "Testbild im Stage-Canvas");

    // Testbild aus → Canvas schwarz (keine Surfaces auf der Stage).
    engine
        .commands
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: None,
        })
        .expect("send pattern off");
    let ok = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            let Some(frame) = store.get("PatternStage") else {
                return false;
            };
            frame
                .rgba
                .chunks_exact(4)
                .all(|c| c[0] < 10 && c[1] < 10 && c[2] < 10)
        },
        Duration::from_secs(3),
    );
    assert!(ok, "Testbild aus → schwarzer Canvas");
}

#[test]
fn real_engine_ignores_pattern_for_unassigned_screen() {
    // Screen ohne zugeordnete Stage: Kommando ist harmlos (kein Canvas,
    // kein Fehler, Engine bleibt produktiv).
    let engine = spawn_real_engine();
    let screen = Screen::new("Lonely", 0, 0, 64, 64);
    engine
        .commands
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: Some(screen.pattern_spec()),
        })
        .expect("send pattern");
    engine.commands.send(EngineCommand::Ping(1)).expect("ping");
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(EngineEvent::Pong(1)) = engine.events.try_recv() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("engine must stay alive for unassigned screen pattern");
}

// ---------------------------------------------------------------------
// LED-Wand-Testbild (Integration)
// ---------------------------------------------------------------------

#[test]
fn real_engine_led_wall_pattern_renders_checkerboard_and_scale() {
    use kuhplay::core::TestPatternKind;
    let engine = spawn_real_engine();
    let mut screen = Screen::new("LEDWall", 0, 0, 128, 128);
    screen.test_pattern = true;
    screen.test_pattern_kind = TestPatternKind::LedWall;
    screen.color = [255, 64, 64]; // Rot
    screen.checker_size = 64; // 2×2 Kacheln
    let mut stage = Stage::new("LedStage", StageGeometry::new(128.0, 128.0));
    stage.screen = Some(screen.id);
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("configure");

    let spec = screen.pattern_spec();
    engine
        .commands
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: Some(spec),
        })
        .expect("pattern on");

    let ok = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            let Some(frame) = store.get("LedStage") else {
                return false;
            };
            // 1px-Rahmen oben bei y=0, Mitte x=64: weiß
            let i_frame = 64 * 4; // (y·128 + x)·4 mit y=0, x=64
            let rahmen_ok = frame.rgba[i_frame] > 200 && frame.rgba[i_frame + 1] > 200;
            // Kachel (0,0) bei (12,12): abgedunkeltes Rot (70% → ~178)
            let i_k0 = (12 * 128 + 12) * 4;
            let k0_ok = frame.rgba[i_k0] > 140 && frame.rgba[i_k0] < 220;
            // Kachel (1,0) bei (76,12): dunkles Rot (18% → ~45)
            let i_k1 = (12 * 128 + 76) * 4;
            let k1_ok = frame.rgba[i_k1] > 20 && frame.rgba[i_k1] < 80;
            // Pixelskala oben bei x=64, y=3: weiß (Markierung)
            let i_tick = (3 * 128 + 64) * 4;
            let tick_ok = frame.rgba[i_tick] > 200;
            rahmen_ok && k0_ok && k1_ok && tick_ok
        },
        Duration::from_secs(3),
    );
    assert!(ok, "LED-Wand-Testbild: Rahmen + Schachbrett + Pixelskala");
}

#[test]
fn real_engine_led_wall_checker_size_adjustable() {
    use kuhplay::core::TestPatternKind;
    let engine = spawn_real_engine();
    let mut screen = Screen::new("SmallTiles", 0, 0, 128, 128);
    screen.test_pattern = true;
    screen.test_pattern_kind = TestPatternKind::LedWall;
    screen.color = [255, 255, 255]; // Weiß
    screen.checker_size = 32; // 4×4 Kacheln
    let mut stage = Stage::new("SmallStage", StageGeometry::new(128.0, 128.0));
    stage.screen = Some(screen.id);
    engine
        .commands
        .send(EngineCommand::ConfigureStage { stage })
        .expect("configure");
    let spec = screen.pattern_spec();
    engine
        .commands
        .send(EngineCommand::SetTestPattern {
            screen: screen.id,
            pattern: Some(spec),
        })
        .expect("pattern on");

    let ok = wait_until(
        || {
            let store = engine.stage_frames.lock().unwrap();
            let Some(frame) = store.get("SmallStage") else {
                return false;
            };
            // Bei 32px Kacheln: Kachel (0,0) bei (28,28) heller (70% Weiß = 178)
            let i_k0 = (28 * 128 + 28) * 4;
            let k0_ok = frame.rgba[i_k0] > 140 && frame.rgba[i_k0] < 220;
            // Kachel (1,0) bei (60,28) dunkler (18% Weiß = 45)
            let i_k1 = (28 * 128 + 60) * 4;
            let k1_ok = frame.rgba[i_k1] > 20 && frame.rgba[i_k1] < 80;
            k0_ok && k1_ok
        },
        Duration::from_secs(3),
    );
    assert!(ok, "LED-Wand mit 32px Kacheln: heller/dunkler Wechsel");
}
