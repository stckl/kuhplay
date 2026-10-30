//! Headless GUI-Smoke-Tests (QLab 5 Workspace).
//!
//! Rendert den kompletten Workspace (Menüleiste, Masthead, Toolbox,
//! Sidebar, Cue-Tabelle, Inspector, Footer) in einer echten
//! [`egui::Context`] – ohne Fenster und GPU (egui läuft „headless").
//! Fängt Laufzeit-Panics im UI-Code, die reine Domain-Unit-Tests nicht
//! sehen, z. B. falsche `egui_extras`-Nutzung wie
//! `TableRow::response()` vor dem ersten `col()`.

use std::time::Duration;

use kuhplay::core::{ContinueMode, Cue, CueStatus, CueType};
use kuhplay::engine::{spawn_mock_engine, EngineCommand, EngineEvent};
use kuhplay::gui::{
    cue_table, footer, icons, inspector, masthead, sidebar, theme::palette, toolbox, InspectorTab,
    SettingsPage, ShowApp, SidebarTab,
};

/// Rendert ein komplettes Frame mit exakt dem Panel-Aufbau der App
/// (`ShowApp::draw_workspace`, Reihenfolge siehe docs/UI.md §2).
/// Rückgabe: das gerenderte `FullOutput` (für Text-Assertions).
fn render_frame(ctx: &egui::Context, app: &mut ShowApp) -> egui::FullOutput {
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    icons::install(ctx);
    ctx.run(raw_input, |ctx| app.draw_workspace(ctx))
}

/// App mit Demo-Show + Mock-Engine (wie beim echten Start).
fn demo_app() -> ShowApp {
    ShowApp::new(spawn_mock_engine())
}

#[test]
fn full_workspace_renders_without_panic() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.toolbox_visible = true; // alle Panels sichtbar
                                   // Mehrere Frames: erster Frame ist egui-Sizing-Pass, danach regulär.
    for _ in 0..3 {
        render_frame(&ctx, &mut app);
    }
}

#[test]
fn workspace_renders_in_show_mode() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.set_show_mode(true);
    assert!(app.show_mode);
    // Show-Mode: Inspector read-only, Cue-Tabelle nicht editierbar.
    for _ in 0..2 {
        render_frame(&ctx, &mut app);
    }
    app.set_show_mode(false);
    render_frame(&ctx, &mut app);
}

#[test]
fn active_cues_tab_renders_with_running_cues() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.sidebar_tab = SidebarTab::ActiveCues;

    render_frame(&ctx, &mut app);
    assert_eq!(app.carts.running_count(), 0);

    // Zwei Cues feuern → Active-Cues-Tab zeigt 2 Einträge (render ohne Panic).
    app.go_now();
    app.go_now();
    assert_eq!(app.carts.running_count(), 2);
    render_frame(&ctx, &mut app);

    // Einzel-Stop (wie der ■-Button in der Sidebar): nur der erste Cue stoppt.
    let id = app.carts.active().cues()[0].id;
    let commands = app.stop_running(id);
    assert_eq!(commands, vec![EngineCommand::Stop { cue_id: id }]);
    app.dispatch(commands);
    assert_eq!(app.carts.running_count(), 1);
    render_frame(&ctx, &mut app);

    // Pause/Resume über die Sidebar-Logik (zweiter Cue ist noch aktiv).
    let id2 = app.carts.active().cues()[1].id;
    let commands = app.pause_running(id2);
    assert_eq!(commands.len(), 1);
    app.dispatch(commands);
    render_frame(&ctx, &mut app);
    let commands = app.resume_running(id2);
    assert_eq!(commands.len(), 1);
    app.dispatch(commands);
    render_frame(&ctx, &mut app);
}

#[test]
fn lists_and_carts_tab_renders() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.sidebar_tab = SidebarTab::ListsCarts;
    render_frame(&ctx, &mut app);

    // Zweiten Cart anlegen (wie „+ New Cart") → rendert.
    let n = app.carts.cart_count() + 1;
    app.carts.add_cart(format!("Cart {n}"));
    assert_eq!(app.carts.cart_count(), 2);
    render_frame(&ctx, &mut app);
}

#[test]
fn inspector_renders_every_cue_type_both_tabs() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    // Jeden Cue selektieren → alle Inspector-Zweige (Audio, Video, Fade,
    // Group, Control, Html, SlideShow, HtmlData, Stop) in beiden Tabs.
    let len = app.carts.active().len();
    for i in 0..len {
        app.carts.active_mut().select(Some(i));
        app.ui.inspector_tab = InspectorTab::Basics;
        render_frame(&ctx, &mut app);
        app.ui.inspector_tab = InspectorTab::Triggers;
        render_frame(&ctx, &mut app);
    }

    // Ohne Selektion (Platzhalter-Pfad).
    app.carts.active_mut().select(None);
    render_frame(&ctx, &mut app);
}

#[test]
fn edit_actions_work_like_qlab() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    let initial_len = app.carts.active().len();

    // Add (wie Masthead-Toolbar / Menü „Cues"): nach Selektion, auto-nummeriert.
    app.carts.active_mut().select(Some(0));
    app.add_cue(CueType::Video);
    assert_eq!(app.carts.active().len(), initial_len + 1);
    assert_eq!(app.carts.selected_index(), Some(1));
    assert_eq!(
        app.carts.active().cues()[1].number,
        (initial_len + 1).to_string()
    );
    render_frame(&ctx, &mut app);

    // Move up/down (wie Kontextmenü / ⌘↑⌘↓).
    app.move_selected(-1);
    assert_eq!(app.carts.selected_index(), Some(0));
    app.move_selected(1);
    assert_eq!(app.carts.selected_index(), Some(1));

    // Arm/Disarm (wie Doppelklick / Kontextmenü).
    app.set_selected_armed(false);
    assert!(!app.carts.selected().unwrap().is_armed);
    app.set_selected_armed(true);
    assert!(app.carts.selected().unwrap().is_armed);

    // Delete (wie Menü „Delete Cue").
    app.delete_selected();
    assert_eq!(app.carts.active().len(), initial_len);
    render_frame(&ctx, &mut app);
}

#[test]
fn show_mode_toggles() {
    let mut app = demo_app();
    assert!(!app.show_mode);
    app.toggle_show_mode();
    assert!(app.show_mode);
    app.toggle_show_mode();
    assert!(!app.show_mode);
}

#[test]
fn go_chained_follows_auto_continue() {
    let mut app = demo_app();
    // Auto-Continue auf den ersten Cue → ein GO feuert beide Cues.
    app.carts.active_mut().cues_mut()[0].continue_mode = ContinueMode::AutoContinue;
    app.go_now();
    assert_eq!(app.carts.running_count(), 2);
    assert_eq!(
        app.carts.cursor(),
        2,
        "Playhead rückt über die Kette hinaus"
    );
}

#[test]
fn phosphor_fonts_install_idempotent_and_glyphs_exist() {
    let ctx = egui::Context::default();
    icons::install(&ctx);
    icons::install(&ctx); // idempotent – kein Doppel-Register

    // Jedes Icon muss ein nicht-leeres Phosphor-Glyph haben.
    let all = [
        icons::Icon::Audio,
        icons::Icon::Video,
        icons::Icon::Fade,
        icons::Icon::Group,
        icons::Icon::Stop,
        icons::Icon::Script,
        icons::Icon::Control,
        icons::Icon::Html,
        icons::Icon::SlideShow,
        icons::Icon::HtmlData,
        icons::Icon::Playing,
        icons::Icon::Paused,
        icons::Icon::Error,
        icons::Icon::Playhead,
        icons::Icon::ContinueOff,
        icons::Icon::AutoContinue,
        icons::Icon::AutoFollow,
        icons::Icon::Reset,
        icons::Icon::Pause,
        icons::Icon::Resume,
        icons::Icon::Panic,
    ];
    for icon in all {
        assert!(
            !icon.glyph().is_empty(),
            "Icon {icon:?} hat kein Phosphor-Glyph"
        );
    }

    // Nach install rendert ein Icon-Frame fehlerfrei (Font tatsächlich geladen).
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            for icon in all {
                icons::draw(ui, icon, 24.0, egui::Color32::WHITE);
            }
        });
    });
}

#[test]
fn all_icons_render_without_panic() {
    // Jedes Icon muss sich in einer echten egui-Context zeichnen lassen
    // (Painter-Code ist die häufigste Panic-Quelle bei selbst gemalten Icons).
    let ctx = egui::Context::default();
    // Mappings abdecken (jeder Cue-Typ / Status / Continue-Mode).
    for cue_type in CueType::ALL {
        let _ = icons::Icon::for_cue_type(cue_type);
    }
    for mode in ContinueMode::ALL {
        let _ = icons::Icon::for_continue_mode(mode);
    }
    // Jeder Status hat ein definiertes Icon (oder keines).
    for status in [
        CueStatus::Standby,
        CueStatus::PreWait,
        CueStatus::Playing,
        CueStatus::Paused,
        CueStatus::PostWait,
        CueStatus::Completed,
        CueStatus::Stopped,
        CueStatus::Error,
    ] {
        let _ = icons::Icon::for_status(status);
    }

    icons::install(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for icon in icons::Icon::ALL {
                    let icon = *icon;
                    icons::draw(ui, icon, 24.0, egui::Color32::WHITE);
                    icons::button(ui, icon, egui::Color32::WHITE, egui::vec2(28.0, 28.0), true);
                    icons::stacked_button(
                        ui,
                        icon,
                        "Label",
                        egui::Color32::WHITE,
                        None,
                        egui::vec2(64.0, 52.0),
                        true,
                    );
                    // Disabled-Variante (z. B. Show-Mode).
                    icons::button(
                        ui,
                        icon,
                        egui::Color32::WHITE,
                        egui::vec2(28.0, 28.0),
                        false,
                    );
                }
            });
        });
    });
}

#[test]
fn workspace_settings_video_page_renders_and_edits_stages_and_routes() {
    // Video-Seite (Workspace Settings → Video, QLab-5-Tabs): rendert ohne
    // Panic; Stage/Route-Verwaltung (anlegen, Editor oeffnen, loeschen)
    // funktioniert gegen Mock-Engine + RouteManager.
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.workspace_settings_open = true;
    app.ui.workspace_settings_page = SettingsPage::Video;
    for _ in 0..3 {
        render_frame(&ctx, &mut app);
    }
    // Default-Setup: pro Display eine Route, Stage 1 mit Region.
    assert_eq!(app.routes.len(), app.displays.len());
    assert!(app.stages.stages()[0]
        .regions
        .iter()
        .any(|r| r.route.is_some()));

    // Leere Stage anlegen (wie das „New Stage“-Menü).
    let before = app.stages.len();
    app.add_empty_stage();
    assert_eq!(app.stages.len(), before + 1);

    // Stage mit Ausgabe anlegen: neue Route + Full-Canvas-Region.
    let routes_before = app.routes.len();
    app.add_stage_with_output();
    assert_eq!(app.routes.len(), routes_before + 1);
    let last = app.stages.stages().last().cloned().unwrap();
    assert_eq!(last.regions.len(), 1);
    assert!(last.regions[0].route.is_some());

    // Stage-Editor oeffnen (wie der Edit-Button) und rendern.
    app.stage_editor_open = Some(last.id);
    for _ in 0..2 {
        render_frame(&ctx, &mut app);
    }
    assert_eq!(app.stage_editor_open, Some(last.id));
    app.stage_editor_open = None;

    // Route anlegen (ohne Device) und Route-Editor oeffnen.
    let route_id = app.add_route();
    assert!(app.routes.find(route_id).is_some());
    app.route_editor_open = Some(route_id);
    for _ in 0..2 {
        render_frame(&ctx, &mut app);
    }
    app.route_editor_open = None;

    // Multi-Output-Assistent: 3 Projektoren mit 100 px Überlapp.
    app.multi_output_wizard = Some(Default::default());
    for _ in 0..2 {
        render_frame(&ctx, &mut app);
    }
    app.add_multi_output_stage(&kuhplay::gui::app::MultiOutputWizard {
        cols: 3,
        rows: 1,
        width: 1920,
        height: 1080,
        overlap_x: 100,
        overlap_y: 100,
    });
    app.multi_output_wizard = None;
    let last = app.stages.stages().last().cloned().unwrap();
    assert_eq!(last.regions.len(), 3, "eine Region je Projektor");
    assert!((last.geometry.width - (3.0 * 1920.0 - 2.0 * 100.0)).abs() < 0.5);

    // Route löschen → Regionen verlieren die Zuweisung: die Region der
    // Multi-Output-Stage auf die neue Route umwidmen.
    let target = last.regions[0].route;
    for stage in app.stages.stages_mut() {
        for region in &mut stage.regions {
            if region.route == target {
                region.route = Some(route_id);
            }
        }
    }
    assert!(app
        .stages
        .stages()
        .iter()
        .any(|s| s.regions.iter().any(|r| r.route == Some(route_id))));
    app.remove_route(route_id);
    assert!(app.routes.find(route_id).is_none());
    assert!(app
        .stages
        .stages()
        .iter()
        .all(|s| s.regions.iter().all(|r| r.route != Some(route_id))));

    // Stage löschen (nur ohne genutzte Cues) – Stage 1 bleibt (Demo-Cues).
    assert!(!app.remove_stage("Stage 1"), "Cues nutzen Stage 1");
    render_frame(&ctx, &mut app);
}

/// Prüft, ob ein Text im gerenderten Frame vorkommt (Galley-Suche).
fn output_has_text(out: &egui::FullOutput, text: &str) -> bool {
    out.shapes.iter().any(
        |shape| matches!(shape.shape, egui::Shape::Text(ref t) if t.galley.job.text.contains(text)),
    )
}

/// In-Memory-`eframe::Storage` für Persistenz-Tests.
struct MemStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }

    fn flush(&mut self) {}
}

#[test]
fn ui_state_persists_via_storage() {
    let mut app = demo_app();
    app.ui.toolbox_visible = false;
    app.ui.sidebar_tab = SidebarTab::ActiveCues;
    app.ui.workspace_settings_open = true;
    app.ui.workspace_settings_page = SettingsPage::Video;

    let mut storage = MemStorage(std::collections::HashMap::new());
    app.save_ui_state(&mut storage);

    // Frische App lädt den gespeicherten Zustand (wie nach einem Neustart).
    let mut loaded = demo_app();
    assert!(loaded.ui.toolbox_visible, "Default: Toolbox sichtbar");
    loaded.load_ui_state(&storage);
    assert_eq!(loaded.ui, app.ui);
}

#[test]
fn footer_shows_cue_count_and_broken_indicator() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx); // Footer malt Icon-Glyphen (Phosphor-Font nötig).
                          // Demo-Show: 11 Cues in 1 Liste; Media-Dateien fehlen (/media/… existiert
                          // nicht) → kaputte Cues werden im Footer gezählt (QLab: Warnzähler).
    let total = app.carts.active().len();
    let broken = app.broken_count();
    assert!(broken > 0, "Demo-Show ohne Media-Dateien hat kaputte Cues");
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            footer::draw(ui, &mut app);
        });
    });
    assert!(output_has_text(&out, &format!("{total} cues in 1 list")));
    assert!(output_has_text(&out, &format!("{broken} broken")));
}

#[test]
fn jump_to_first_broken_selects_the_cue() {
    let mut app = demo_app();
    assert!(app.carts.selected_index().is_none());
    assert!(app.broken_count() > 0);
    app.jump_to_first_broken();
    let selected = app
        .carts
        .selected()
        .expect("kaputter Cue selektiert")
        .clone();
    let problems = app.problems_of(&selected);
    assert!(!problems.is_empty(), "selektierter Cue ist kaputt");
}

#[test]
fn cue_problems_reports_missing_file_unknown_stage_and_bad_json() {
    use kuhplay::core::Cue;

    let mut app = demo_app();
    let list = app.carts.active_mut();
    list.add(Cue::new("t1", "Fehlt", CueType::Audio).with_target("/nonexistent/file.wav"));
    list.add(Cue::new("t2", "Stage", CueType::Video).with_stage("Stage 42"));
    list.add(Cue::new("t3", "JSON", CueType::HtmlData).with_data_json("{invalid"));
    let problems = |app: &mut ShowApp, n: &str| {
        let cue = app
            .carts
            .active()
            .cues()
            .iter()
            .find(|c| c.number == n)
            .unwrap()
            .clone();
        app.problems_of(&cue)
    };
    let p1 = problems(&mut app, "t1");
    assert!(p1
        .iter()
        .any(|p| p.contains("file not found: /nonexistent/file.wav")));
    let p2 = problems(&mut app, "t2");
    assert!(p2.iter().any(|p| p == "unknown stage: Stage 42"));
    let p3 = problems(&mut app, "t3");
    assert!(p3.iter().any(|p| p == "no target cue"));
    assert!(p3.iter().any(|p| p == "invalid JSON payload"));
    // Gesunder Cue (Fade ohne Datei-Abhängigkeit) meldet keine Probleme.
    let healthy = Cue::new("t4", "Fade", CueType::Fade);
    assert!(app.problems_of(&healthy).is_empty());
    // Stream-URLs werden nicht auf Existenz geprüft (kein False-Positive).
    let stream = Cue::new("t5", "Stream", CueType::Audio).with_target("http://example.com/x.wav");
    assert!(app.problems_of(&stream).is_empty());
}

#[test]
fn group_children_problems_roll_up_to_the_group() {
    use kuhplay::core::Cue;

    let mut app = demo_app();
    let group = Cue::new_group(
        "g1",
        "Gruppe",
        kuhplay::core::GroupMode::Simultaneous,
        vec![Cue::new("g1.1", "Kind", CueType::Audio).with_target("/nonexistent/kid.wav")],
    );
    app.carts.active_mut().add(group);
    let problems = app.problems_of(&app.carts.active().cues().last().unwrap().clone());
    assert!(
        problems
            .iter()
            .any(|p| p.contains("g1.1") && p.contains("file not found")),
        "Kinder-Problem rollt auf die Gruppe hoch: {problems:?}"
    );
}

#[test]
fn toolbox_renders_groups_and_type_buttons() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::SidePanel::left("toolbox")
            .resizable(true)
            .default_width(140.0)
            .show(ctx, |ui| {
                toolbox::draw(ui, &mut app);
            });
    });
    // Gruppen-Header + alle Cue-Typ-Labels (einzige Button-Quelle, Phase 1).
    for expected in [
        "MEDIA",
        "Audio",
        "Video",
        "HTML5",
        "Slideshow",
        "Fade",
        "Control",
        "Group",
        "Stop",
        "Script",
        "Data",
        "HTML Data",
    ] {
        assert!(output_has_text(&out, expected), "Toolbox fehlt: {expected}");
    }
}

#[test]
fn masthead_renders_go_standby_and_notes() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    // Zwei Frames rendern: Im ersten Frame ist das Clip-Rect des Panels noch
    // auf ~1 Zeile (interact_size) beschränkt – TextEdit malt seinen Hint-Text
    // erst, sobald das Panel seine echte Höhe kennt (ab Frame 2).
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            masthead::draw(ui, &mut app);
        });
    });
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            masthead::draw(ui, &mut app);
        });
    });
    assert!(output_has_text(&out, "GO"));
    assert!(
        output_has_text(&out, "1 • House to Half"),
        "Standby: Q# • Name"
    );
    assert!(output_has_text(&out, "Notes (standby cue)"));
}

#[test]
fn sidebar_renders_tabs() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::SidePanel::right("sidebar")
            .default_width(250.0)
            .show(ctx, |ui| {
                sidebar::draw(ui, &mut app);
            });
    });
    assert!(output_has_text(&out, "Lists & Carts"));
    assert!(output_has_text(&out, "Active Cues (0)"));
}

/// Globaler Transport sitzt im Masthead neben GO (docs/UI.md §5.1).
#[test]
fn masthead_renders_transport_and_panic() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            masthead::draw(ui, &mut app);
        });
    });
    // Transport-Icons (Phosphor-Glyphs): Reset, Pause, Resume, Panic.
    for glyph in ["\u{E038}", "\u{E39E}", "\u{E3D0}", "\u{E4E0}"] {
        assert!(
            output_has_text(&out, glyph),
            "Transport-Icon fehlt: {glyph:?}"
        );
    }
    assert!(output_has_text(&out, "Panic"));
}

#[test]
fn workspace_settings_window_renders_nav_and_pages() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.workspace_settings_open = true;
    // egui::Window braucht 2 Frames (Frame 0 = Sizing-Pass ohne Text-Shapes);
    // die Assertions laufen auf dem letzten Frame.
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    // Default-Seite General: Platzhalter.
    assert!(output_has_text(&out, "Workspace Settings"));
    for page in ["General", "Audio", "Video", "Network", "MIDI", "Key Map"] {
        assert!(output_has_text(&out, page), "Nav fehlt: {page}");
    }
    assert!(!output_has_text(&out, "not implemented yet"));
    // Seite Video: drei Reiter wie QLab (Stages/Routing/Devices).
    app.ui.workspace_settings_page = SettingsPage::Video;
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "Video Outputs (Stages)"));
    assert!(output_has_text(&out, "Output Routing"));
}

// ------------------------------------------------------------------
// Phase 2 (TODO.md): Status-Semantik (QLab), Broken-✕ + Tooltip,
// Cue-Color als Zeilen-Tönung + Streifen, Fortschrittsbalken,
// Pre-Wait-Countdown
// ------------------------------------------------------------------

/// Rendert nur die Cue-Tabelle (CentralPanel, 1440×900) und liefert das
/// FullOutput (für Shape-/Text-/Farb-Assertions).
fn render_table_only(ctx: &egui::Context, app: &mut ShowApp) -> egui::FullOutput {
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    icons::install(ctx);
    ctx.run(raw_input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let row_meta = app.cue_row_meta();
            let _ = cue_table::draw(
                ui,
                app.carts.active_mut(),
                !app.show_mode,
                &row_meta,
                &mut app.ui.cue_columns,
                &app.ui.find,
            );
        });
    })
}

/// Rendert die Cue-Tabelle mit manuell vorgegebenen Anzeige-Metadaten
/// (für deterministische Restzeit-/Countdown-Assertions).
fn render_table_with_meta(
    ctx: &egui::Context,
    app: &mut ShowApp,
    meta: &std::collections::HashMap<uuid::Uuid, cue_table::CueRowMeta>,
) -> egui::FullOutput {
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    icons::install(ctx);
    ctx.run(raw_input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let _ = cue_table::draw(
                ui,
                app.carts.active_mut(),
                !app.show_mode,
                meta,
                &mut app.ui.cue_columns,
                &app.ui.find,
            );
        });
    })
}

/// Prüft, ob ein Text-Shape mit exakt diesem Text im Frame vorkommt
/// (im Gegensatz zu [`output_has_text`]: kein Substring-Match).
fn output_has_exact_text(out: &egui::FullOutput, text: &str) -> bool {
    out.shapes
        .iter()
        .any(|shape| matches!(shape.shape, egui::Shape::Text(ref t) if t.galley.job.text == text))
}

/// Prüft, ob ein Rect-Shape mit exakter Füllfarbe im Frame vorkommt.
fn output_has_rect_fill(out: &egui::FullOutput, fill: egui::Color32) -> bool {
    out.shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == fill))
}

/// Liefert die (in die Galley gebrannte) Textfarbe des ersten Text-Shapes,
/// dessen Text `needle` enthält.
fn output_text_color(out: &egui::FullOutput, needle: &str) -> Option<egui::Color32> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text.contains(needle) => {
            t.galley.job.sections.first().map(|sec| sec.format.color)
        }
        _ => None,
    })
}

/// App mit manuellen Engine-Kanälen (deterministische Events, wie
/// `hotkey_app` in tests/gui_interaction.rs).
fn manual_app() -> (
    ShowApp,
    crossbeam_channel::Receiver<EngineCommand>,
    crossbeam_channel::Sender<EngineEvent>,
) {
    use kuhplay::engine::{
        new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
        EngineHandle,
    };
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
    let handle = EngineHandle {
        commands: cmd_tx,
        events: evt_rx,
        frames: new_frame_store(),
        opacities: new_opacity_store(),
        stage_frames: new_stage_frame_store(),
        route_frames: new_route_frame_store(),
        audio_voices: Default::default(),
    };
    let app = ShowApp::new(handle);
    // Startup-Commands (ConfigureStage aller Stages) verwerfen.
    while cmd_rx.try_recv().is_ok() {}
    (app, cmd_rx, evt_tx)
}

#[test]
fn broken_cue_shows_red_x_and_healthy_cue_does_not() {
    let ctx = egui::Context::default();
    // Demo-Show: Media-Dateien fehlen → kaputte Cues → rotes ✕ (Phosphor x).
    let mut app = demo_app();
    assert!(app.broken_count() > 0);
    let out = render_table_only(&ctx, &mut app);
    assert!(
        output_has_text(&out, "\u{E4F6}"),
        "kaputter Cue zeigt rotes ✕ in der Status-Spalte"
    );

    // Gesunde Liste (Fade-Cues ohne Datei-Abhängigkeit): kein ✕.
    let mut healthy = demo_app();
    {
        let list = healthy.carts.active_mut();
        list.cues_mut().clear();
        list.add(Cue::new("1", "Fade 1", CueType::Fade));
        list.add(Cue::new("2", "Fade 2", CueType::Fade));
    }
    assert_eq!(healthy.broken_count(), 0);
    let out = render_table_only(&ctx, &mut healthy);
    assert!(
        !output_has_text(&out, "\u{E4F6}"),
        "gesunde Cues zeigen kein ✕"
    );
}

#[test]
fn cue_color_paints_row_tint_and_left_strip() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.carts.active_mut().cues_mut()[0].color = Some([200, 60, 60]);
    let out = render_table_only(&ctx, &mut app);
    // Streifen am linken Rand (volle Deckkraft) …
    assert!(
        output_has_rect_fill(&out, egui::Color32::from_rgb(200, 60, 60)),
        "Cue-Color-Streifen am linken Zeilenrand fehlt"
    );
    // … und Tönung der ganzen Zeile (niedrige Deckkraft, QLab-Style).
    assert!(
        output_has_rect_fill(
            &out,
            egui::Color32::from_rgb(200, 60, 60).gamma_multiply(palette::ROW_TINT)
        ),
        "Cue-Color-Zeilen-Tönung fehlt"
    );
}

#[test]
fn cursor_row_has_highlight_background() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    let out = render_table_only(&ctx, &mut app);
    assert!(
        output_has_rect_fill(&out, palette::CURSOR_ROW),
        "Playhead-Zeile ist nicht dezent hervorgehoben"
    );
}

#[test]
fn pre_wait_cue_counts_down_in_pre_wait_cell() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    // Cue 2 auf Pre-Wait mit 65 s (statisch „01:05.000", Countdown
    // „01:04.99x" – in dieser Show eindeutig).
    let cue_id = {
        let list = app.carts.active_mut();
        list.cues_mut()[1].status = CueStatus::PreWait;
        list.cues_mut()[1].pre_wait = Duration::from_secs(65);
        list.cues()[1].id
    };
    let out = render_table_only(&ctx, &mut app);
    let cue = app
        .carts
        .active()
        .cues()
        .iter()
        .find(|c| c.id == cue_id)
        .unwrap()
        .clone();
    let remaining = app
        .pre_wait_remaining(&cue)
        .expect("Pre-Wait-Restzeit wird getrackt");
    assert!(
        remaining <= Duration::from_secs(65) && remaining > Duration::from_secs(64),
        "Restzeit zählt herunter: {remaining:?}"
    );
    assert!(
        output_has_text(&out, "01:04"),
        "Pre-Wait-Zelle zeigt die Restzeit (Countdown)"
    );
    // Anderer Cue (Standby): keine Restzeit.
    let other = app.carts.active().cues()[0].clone();
    assert_eq!(app.pre_wait_remaining(&other), None);
}

#[test]
fn playing_and_post_wait_cues_show_progress_bars() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, evt_tx) = manual_app();
    // Gesunder Fade-Cue mit Duration + Post-Wait. Die Duration ist winzig
    // (1 ms), damit die Gesamtzeit nach einem kurzen Sleep die Duration
    // übersteigt – sonst wäre post_elapsed = elapsed − duration = 0.
    let cue_id = {
        let list = app.carts.active_mut();
        list.cues_mut().clear();
        let mut cue = Cue::new("1", "Fade", CueType::Fade);
        cue.duration = Duration::from_millis(1);
        cue.post_wait = Duration::from_secs(5);
        let id = cue.id;
        list.add(cue);
        id
    };
    // Started → Playing (Startzeitpunkt gesetzt).
    evt_tx
        .send(EngineEvent::Started {
            cue_id,
            duration: None,
        })
        .unwrap();
    app.poll_engine_events();
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::Playing);
    let out = render_table_only(&ctx, &mut app);
    assert!(
        output_has_rect_fill(&out, kuhplay::gui::theme::progress_fill(palette::PLAYING)),
        "Playing: Fortschrittsbalken in der Action-Spalte fehlt"
    );

    // Finished → Post-Wait (Post-Wait > 0): Balken in der Post-Wait-Zelle.
    evt_tx.send(EngineEvent::Finished { cue_id }).unwrap();
    app.poll_engine_events();
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::PostWait);
    // Post-Wait-Elapsed = Gesamtzeit − Duration; kurz warten, damit die
    // Gesamtzeit die (winzige) Duration sicher übersteigt.
    std::thread::sleep(Duration::from_millis(50));
    let out = render_table_only(&ctx, &mut app);
    assert!(
        output_has_rect_fill(&out, kuhplay::gui::theme::progress_fill(palette::POST_WAIT)),
        "Post-Wait: Fortschrittsbalken in der Post-Wait-Zelle fehlt"
    );
}

/// QLab-5-Header: Die Spalten heißen „Pre-wait“, „Duration“, „Post-wait“
/// (verifiziert gegen das QLab-5-Handbuch; „Action" ist die alte
/// QLab-4-Bezeichnung und darf nicht mehr als Header auftauchen).
#[test]
fn cue_table_header_shows_qlab_column_titles() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    // Zwei Frames (erster Frame ist egui-Sizing-Pass).
    render_table_only(&ctx, &mut app);
    let out = render_table_only(&ctx, &mut app);
    for title in [
        "Type",
        "Q#",
        "Name",
        "Target",
        "Pre-wait",
        "Duration",
        "Post-wait",
    ] {
        assert!(output_has_exact_text(&out, title), "Header „{title}“ fehlt");
    }
    assert!(
        !output_has_exact_text(&out, "Action"),
        "QLab 5: Duration-Spalte heißt „Duration“, nicht „Action“"
    );
}

/// Laufender Cue: Die Duration-Zelle zeigt die RESTZEIT (Duration −
/// elapsed), QLab-Verhalten. Meta manuell → deterministisch.
#[test]
fn duration_cell_shows_remaining_time_for_playing_cue() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, evt_tx) = manual_app();
    let cue_id = {
        let list = app.carts.active_mut();
        list.cues_mut().clear();
        let mut cue = Cue::new("1", "Fade", CueType::Fade);
        cue.duration = Duration::from_secs(3);
        let id = cue.id;
        list.add(cue);
        id
    };
    // Playing-Status (wie ein Engine-Event es meldet).
    evt_tx
        .send(EngineEvent::Started {
            cue_id,
            duration: None,
        })
        .unwrap();
    app.poll_engine_events();
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::Playing);

    // Meta manuell: 1 s elapsed → Restzeit exakt 2 s.
    let mut meta = std::collections::HashMap::new();
    meta.insert(
        cue_id,
        cue_table::CueRowMeta {
            problems: vec![],
            elapsed: Some(Duration::from_secs(1)),
            pre_wait_remaining: None,
            media_duration: None,
        },
    );
    render_table_with_meta(&ctx, &mut app, &meta);
    let out = render_table_with_meta(&ctx, &mut app, &meta);
    assert!(
        output_has_text(&out, "00:02.000"),
        "Duration-Zelle zeigt die Restzeit 00:02.000 (nicht die volle Duration)"
    );
}

/// Post-Wait: Die Post-wait-Zelle zählt herunter (Post-wait − post-elapsed).
/// Meta manuell → deterministisch.
#[test]
fn post_wait_cell_shows_countdown() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, evt_tx) = manual_app();
    let cue_id = {
        let list = app.carts.active_mut();
        list.cues_mut().clear();
        let mut cue = Cue::new("1", "Fade", CueType::Fade);
        cue.duration = Duration::from_millis(1);
        cue.post_wait = Duration::from_secs(5);
        let id = cue.id;
        list.add(cue);
        id
    };
    // Finished → Post-Wait (Post-Wait > 0).
    evt_tx
        .send(EngineEvent::Started {
            cue_id,
            duration: None,
        })
        .unwrap();
    app.poll_engine_events();
    evt_tx.send(EngineEvent::Finished { cue_id }).unwrap();
    app.poll_engine_events();
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::PostWait);

    // Meta manuell: 50 ms elapsed → post-elapsed 49 ms → Rest 4,951 s.
    let mut meta = std::collections::HashMap::new();
    meta.insert(
        cue_id,
        cue_table::CueRowMeta {
            problems: vec![],
            elapsed: Some(Duration::from_millis(50)),
            pre_wait_remaining: None,
            media_duration: None,
        },
    );
    render_table_with_meta(&ctx, &mut app, &meta);
    let out = render_table_with_meta(&ctx, &mut app, &meta);
    assert!(
        output_has_text(&out, "00:04.951"),
        "Post-Wait-Zelle zählt herunter: 00:04.951 (5 s − 49 ms)"
    );
}

/// Spalten-Menü: Eine ausgeblendete Spalte (hier „Target") wird nicht
/// gerendert (QLab: Spalten konfigurierbar).
#[test]
fn hidden_column_is_not_rendered() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.cue_columns.hidden[4] = true; // Target
    render_table_only(&ctx, &mut app);
    let out = render_table_only(&ctx, &mut app);
    assert!(
        !output_has_exact_text(&out, "Target"),
        "ausgeblendete Spalte „Target“ wird nicht gerendert"
    );
    // Andere Header bleiben sichtbar.
    assert!(
        output_has_exact_text(&out, "Duration"),
        "„Duration“ bleibt sichtbar"
    );
}

#[test]
fn standby_display_is_neutral_not_red() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    // Zwei Frames (Panel-Clip im ersten Frame auf ~1 Zeile beschränkt).
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            masthead::draw(ui, &mut app);
        });
    });
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            masthead::draw(ui, &mut app);
        });
    });
    // QLab-Phase-2-Semantik: Standby (auch armiert) ist neutral – Rot wäre
    // „kaputt". Der Standby-Text darf nicht in einer Statusfarbe stehen.
    let color = output_text_color(&out, "1 • House to Half").expect("Standby-Text gerendert");
    assert_eq!(
        color,
        palette::TEXT,
        "Standby-Anzeige ist neutral (kein Rot/Amber)"
    );
}

// ------------------------------------------------------------------
// Phase 4 (TODO.md): Inspector-Restrukturierung – Kopfzeile,
// typabhängige Tabs, Timecode-Zeitfelder, Submodule
// ------------------------------------------------------------------

/// Rendert nur den Inspector (isoliert von Cue-Tabelle/Masthead, damit
/// Text-Assertions eindeutig sind).
fn render_inspector_only(ctx: &egui::Context, app: &mut ShowApp) -> egui::FullOutput {
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 400.0),
        )),
        ..Default::default()
    };
    icons::install(ctx);
    ctx.run(raw_input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            inspector::draw(ui, app);
        });
    })
}

/// Jeder Cue-Typ rendert in jedem seiner typabhängigen Tabs ohne Panic
/// (QLab 5: Tabs je nach Cue-Typ).
#[test]
fn inspector_renders_type_specific_tabs() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    let len = app.carts.active().len();
    for i in 0..len {
        app.carts.active_mut().select(Some(i));
        let cue_type = app.carts.active().cues()[i].cue_type;
        for tab in inspector::tabs_for(Some(cue_type)) {
            app.ui.inspector_tab = *tab;
            render_frame(&ctx, &mut app);
            let out = render_inspector_only(&ctx, &mut app);
            assert!(
                output_has_text(&out, tab.label()),
                "Tab-Leiste zeigt „{}“ für {:?}",
                tab.label(),
                cue_type
            );
        }
    }
    // Ohne Auswahl: Default-Tabs (Basics/Triggers).
    app.carts.active_mut().select(None);
    for tab in inspector::tabs_for(None) {
        app.ui.inspector_tab = *tab;
        render_frame(&ctx, &mut app);
    }
    let out = render_inspector_only(&ctx, &mut app);
    assert!(output_has_text(&out, "No cue selected"));
}

/// Kopfzeile: einzelner Cue zeigt „Q# • Name“, Mehrfachauswahl
/// „N cues selected" (QLab 5).
#[test]
fn inspector_header_shows_cue_or_selection_count() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    app.carts.active_mut().select(Some(0));
    let expected = {
        let cue = &app.carts.active().cues()[0];
        format!("{} • {}", cue.number, cue.name)
    };
    let out = render_inspector_only(&ctx, &mut app);
    assert!(
        output_has_text(&out, &expected),
        "Kopfzeile zeigt „{expected}“"
    );
    assert!(!output_has_text(&out, "cues selected"));

    // Mehrfachauswahl (⌘-Klick-Äquivalent: toggle_select_id auf 2 Cues).
    let id0 = app.carts.active().cues()[0].id;
    let id1 = app.carts.active().cues()[1].id;
    app.carts.active_mut().select_id(Some(id0));
    app.carts.active_mut().toggle_select_id(id1);
    let out = render_inspector_only(&ctx, &mut app);
    assert!(
        output_has_text(&out, "2 cues selected"),
        "Kopfzeile zählt die Mehrfachauswahl"
    );
}

/// Zeitfelder im Inspector zeigen Timecode `MM:SS.mmm` (wie die Cue-Liste),
/// nicht mehr nackte Sekunden.
#[test]
fn inspector_time_fields_show_timecode() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    let idx = app
        .carts
        .active()
        .cues()
        .iter()
        .position(|c| c.cue_type == CueType::Audio)
        .expect("demo app hat audio cue");
    app.carts.active_mut().select(Some(idx));
    {
        let cue = &mut app.carts.active_mut().cues_mut()[idx];
        cue.duration = Duration::from_secs_f64(65.5);
        cue.pre_wait = Duration::from_secs_f64(2.25);
        cue.post_wait = Duration::from_secs_f64(0.5);
    }
    app.ui.inspector_tab = InspectorTab::Basics;
    let out = render_inspector_only(&ctx, &mut app);
    assert!(
        output_has_text(&out, "01:05.500"),
        "Duration als Timecode MM:SS.mmm"
    );
    assert!(
        output_has_text(&out, "00:02.250"),
        "Pre Wait als Timecode MM:SS.mmm"
    );
    assert!(
        output_has_text(&out, "00:00.500"),
        "Post Wait als Timecode MM:SS.mmm"
    );
    assert!(
        !output_has_text(&out, "65.5"),
        "keine nackten Sekunden mehr im Zeitfeld"
    );
}

/// Show-Mode: Inspector bleibt sichtbar und rendert read-only
/// (Inhalt zeichnen, Widgets disabled).
#[test]
fn inspector_renders_readonly_in_show_mode() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.carts.active_mut().select(Some(0));
    app.set_show_mode(true);
    app.ui.inspector_tab = InspectorTab::Basics;
    let out = render_inspector_only(&ctx, &mut app);
    assert!(output_has_text(&out, "Basics"), "Tabs rendern im Show-Mode");
    assert!(
        output_has_text(&out, "Q#"),
        "Basics-Inhalt rendert im Show-Mode"
    );
}

// ------------------------------------------------------------------
// Phase 5: Shortcuts-Fenster + Find-Leiste
// ------------------------------------------------------------------

/// „Keyboard Shortcuts"-Fenster rendert die zentrale Tabelle (Help-Menü).
#[test]
fn shortcuts_window_renders_central_table() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.shortcuts_open = true;
    // Fenster brauchen 2 Pässe (erster = Sizing) – daher mehrere Frames.
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(
        output_has_text(&out, "Keyboard Shortcuts"),
        "Fenster-Titel fehlt"
    );
    assert!(output_has_text(&out, "Playback"), "Gruppe Playback fehlt");
    assert!(output_has_exact_text(&out, "GO"), "Aktion GO fehlt");
    assert!(output_has_exact_text(&out, "Space"), "Taste Space fehlt");
    assert!(output_has_exact_text(&out, "Esc"), "Taste Esc fehlt");
    // Datei-Gruppe (im sichtbaren Bereich): Kürzel „⌘N“ aus der Tabelle.
    // (⌘F/⌘1/⌫ liegen tiefer in der ScrollArea und sind in den Unit-Tests
    // von `shortcuts.rs` abgedeckt.)
    assert!(output_has_exact_text(&out, "⌘N"), "⌘N fehlt");
    assert!(output_has_exact_text(&out, "File"), "Gruppe File fehlt");
}

/// Find-Leiste über der Cue-Liste: sichtbar, Treffer-Anzeige, kein Panic
/// beim Abdunkeln der Nicht-Treffer (Phase 5).
#[test]
fn find_bar_renders_above_cue_list_when_open() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.find.open = true;
    // Leerer Query: Hint-Text sichtbar.
    let out = render_frame(&ctx, &mut app);
    assert!(
        output_has_text(&out, "Find cue by number"),
        "Find-Leiste mit Hint-Text fehlt"
    );

    // Query ohne Treffer: „No matches“.
    app.ui.find.query = "zzzz-no-match".to_string();
    let out = render_frame(&ctx, &mut app);
    assert!(
        output_has_exact_text(&out, "No matches"),
        "Treffer-Anzeige „No matches“ fehlt"
    );

    // Mit Treffern: Anzeige „0/N“ (current startet bei 0; „House to Half“ matcht).
    app.ui.find.query = "house".to_string();
    let out = render_frame(&ctx, &mut app);
    assert!(
        out.shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.starts_with("0/"))
        ),
        "Treffer-Anzeige „0/N“ fehlt"
    );
}

// ------------------------------------------------------------------
// Phase 6: Show-Mode-Polish, Leerzustände, Active-Cues-Details
// ------------------------------------------------------------------

/// Show-Mode muss sofort erkennbar sein (TODO.md Phase 6): eingefärbter
/// Footer + „SHOW"-Label + farbiger Masthead-Rahmen.
#[test]
fn show_mode_is_immediately_visible() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    // Edit-Mode: kein SHOW-Label, keine Show-Mode-Füllung.
    let out = render_frame(&ctx, &mut app);
    assert!(
        !output_has_exact_text(&out, "SHOW"),
        "kein SHOW-Label im Edit-Mode"
    );
    assert!(
        !output_has_rect_fill(&out, palette::SHOW_MODE_BG),
        "Footer im Edit-Mode nicht eingefärbt"
    );

    // Show-Mode: Footer eingefärbt + SHOW-Pill + Masthead-Rahmen.
    app.set_show_mode(true);
    render_frame(&ctx, &mut app);
    let out = render_frame(&ctx, &mut app);
    assert!(
        output_has_rect_fill(&out, palette::SHOW_MODE_BG),
        "Footer im Show-Mode eingefärbt (SHOW_MODE_BG)"
    );
    assert!(
        output_has_exact_text(&out, "SHOW"),
        "SHOW-Label fehlt im Show-Mode"
    );
    app.set_show_mode(false);
}

/// Leere Cue-Liste zeigt einen Hinweis (TODO.md Phase 6).
#[test]
fn empty_cue_list_shows_drop_hint() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.carts.active_mut().cues_mut().clear();
    app.carts.active_mut().select(None);
    let out = render_frame(&ctx, &mut app);
    assert!(
        output_has_text(&out, "Drag files here or use the Toolbox to add cues"),
        "Leerzustand-Hinweis fehlt"
    );
}

/// Inspector ohne Selektion: „No cue selected" zentriert (TODO.md Phase 6).
#[test]
fn inspector_without_selection_shows_centered_hint() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.carts.active_mut().select(None);
    let out = render_frame(&ctx, &mut app);
    assert!(
        output_has_exact_text(&out, "No cue selected"),
        "Inspector-Leerzustand fehlt"
    );
    // Zentriert: mindestens eine „No cue selected“-Textshape (der Leerzustand
    // im Tab-Inhalt; die Kopfzeile zeigt den Text linksbündig) liegt mit ihrer
    // Mitte in der mittleren Zone des Inspector-Panels (volle Breite 1440 px
    // abzüglich Seitenleisten → Mitte ≈ 600–700).
    let centered = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "No cue selected" => Some(
                egui::Rect::from_min_size(t.pos, t.galley.rect.size())
                    .center()
                    .x,
            ),
            _ => None,
        })
        .any(|x| (500.0..830.0).contains(&x));
    assert!(centered, "„No cue selected“ nicht zentriert im Inspector");
}

/// Active-Cues-Tab (TODO.md Phase 6): Typ-Icon + Nummer/Name + Zeiten;
/// Buttons min. 28 px (Konstante geprüft).
#[test]
fn active_cues_rows_show_type_icon_name_and_times() {
    let button_height = kuhplay::gui::theme::size::ACTIVE_CUE_BUTTON.y;
    assert!(
        button_height >= 28.0,
        "Pause/Resume/Stop-Buttons müssen min. 28 px hoch sein"
    );
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.sidebar_tab = SidebarTab::ActiveCues;
    app.go_now();
    assert_eq!(app.carts.running_count(), 1);
    let out = render_frame(&ctx, &mut app);
    // Nummer + Name der laufenden Cue in der Zeile.
    let first = &app.carts.active().cues()[0];
    assert!(
        output_has_text(&out, &format!("{}  {}", first.number, first.name)),
        "Active-Cue-Zeile zeigt „Q#  Name“"
    );
}

/// Hilfsfunktion: obere linke Ecke des ersten Text-Shapes mit exakt `text`.
fn text_pos(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text == text => Some(t.pos),
        _ => None,
    })
}

/// Footer ist die unterste Zeile, auch unter dem Inspector (docs/UI.md §2).
#[test]
fn footer_sits_below_inspector() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.inspector_visible = true;
    app.carts.select(Some(0));
    render_frame(&ctx, &mut app);
    let out = render_frame(&ctx, &mut app);
    let tab = text_pos(&out, "Basics").expect("Inspector-Tab sichtbar");
    let footer = text_pos(&out, "Show").expect("Footer sichtbar");
    assert!(footer.y > tab.y, "Footer {footer:?} über Inspector {tab:?}");
}

/// Toolbox: Icon und Label überlappen nicht (docs/UI.md §1.6).
#[test]
fn toolbox_icon_does_not_overlap_label() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    render_frame(&ctx, &mut app);
    let out = render_frame(&ctx, &mut app);
    let glyph = icons::Icon::for_cue_type(CueType::Audio).glyph();
    let (icon_right, label_left) = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t),
            _ => None,
        })
        .fold((None, None), |(icon, label), t| {
            if t.galley.job.text == glyph && icon.is_none() {
                (Some(t.pos.x + t.galley.size().x), label)
            } else if t.galley.job.text == "Audio" && label.is_none() {
                (icon, Some(t.pos.x))
            } else {
                (icon, label)
            }
        });
    assert!(icon_right.expect("Icon") <= label_left.expect("Label"));
}

/// Palette gilt auch, wenn das System hell ist (docs/UI.md §3).
#[test]
fn theme_stays_dark_on_light_system() {
    let ctx = egui::Context::default();
    kuhplay::gui::theme::apply_theme(&ctx);
    let raw_input = egui::RawInput {
        system_theme: Some(egui::Theme::Light),
        ..Default::default()
    };
    let _ = ctx.run(raw_input, |_| {});
    assert!(ctx.style().visuals.dark_mode);
    assert_eq!(ctx.style().visuals.panel_fill, palette::PANEL);
    assert_eq!(ctx.style().visuals.selection.bg_fill, palette::SELECTION);
}

/// Laufender Fade zeigt Restzeit/Fortschritt, obwohl die Engine für Fades
/// kein `Started` meldet.
#[test]
fn running_fade_cue_has_elapsed_time() {
    let mut app = demo_app();
    app.go_now();
    let cue = app.carts.active().cues()[0].clone();
    assert_eq!(cue.cue_type, CueType::Fade);
    assert_eq!(cue.status, CueStatus::Playing);
    assert!(app.phase_elapsed_of(&cue).is_some());
}

/// Jedes Kürzel ist im UI-Font darstellbar, also keine Kästchen.
#[test]
fn shortcut_labels_use_available_glyphs() {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let font = egui::FontId::proportional(13.5);
    let mut missing = Vec::new();
    for shortcut in kuhplay::gui::shortcuts::ALL {
        for c in shortcut.display().chars() {
            if !ctx.fonts(|f| f.has_glyph(&font, c)) {
                missing.push((shortcut.display(), c));
            }
        }
    }
    assert!(missing.is_empty(), "Glyphs fehlen: {missing:?}");
}

/// Jede Settings-Seite zeigt echten Inhalt statt eines Platzhalters.
#[test]
fn workspace_settings_pages_show_content() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.workspace_settings_open = true;
    for (page, expected) in [
        (SettingsPage::General, "Show Mode"),
        (SettingsPage::Audio, "Output"),
        (SettingsPage::Network, "Web Remote"),
        (SettingsPage::Midi, "Go"),
        (SettingsPage::KeyMap, "Space"),
    ] {
        app.ui.workspace_settings_page = page;
        let mut out = render_frame(&ctx, &mut app);
        for _ in 0..2 {
            out = render_frame(&ctx, &mut app);
        }
        assert!(
            output_has_text(&out, expected),
            "{page:?}: {expected} fehlt"
        );
    }
}

/// MIDI startet im Hintergrund und liefert irgendwann ein Ergebnis.
#[test]
fn midi_start_finishes_in_background() {
    let mut app = demo_app();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while app.midi_starting() {
        assert!(std::time::Instant::now() < deadline, "MIDI-Start hängt");
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.poll_midi_start();
    }
}

/// Gewähltes Audio-Gerät wird aktiv und mit der Show gespeichert.
#[test]
fn audio_output_is_applied_and_saved_with_show() {
    let mut app = demo_app();
    app.set_audio_output(Some("Interface".to_string()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.audio_output() != Some("Interface") {
        assert!(
            std::time::Instant::now() < deadline,
            "Gerät nicht übernommen"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.poll_engine_events();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.kuhplay");
    app.save_show(&path);
    let doc = std::fs::read_to_string(&path).unwrap();
    let doc = kuhplay::core::ShowDocument::from_json(&doc).unwrap();
    assert_eq!(doc.audio_output.as_deref(), Some("Interface"));
}

/// Ausgabegeräte: Das Standardgerät steht auch in der Auswahlliste.
#[test]
fn default_output_is_listed() {
    use kuhplay::engine::audio_backend::{default_output_name, output_devices};
    if let Some(name) = default_output_name() {
        assert!(output_devices().iter().any(|d| d.name == name));
    }
}

/// Kanal-Zuordnung wird mit der Show gespeichert und wieder geladen.
#[test]
fn audio_patch_is_saved_and_loaded_with_show() {
    let mut app = demo_app();
    let mut patch = app.audio_patch.clone();
    patch.channels[0].name = "Mains L".to_string();
    // Matrix: Kanal 1 -> Ausgang 3 am Standardgerät UND Ausgang 1 am Interface.
    patch.channels[0].outputs = vec![
        kuhplay::core::ChannelOutput {
            device: None,
            output: 3,
        },
        kuhplay::core::ChannelOutput {
            device: Some("Interface".to_string()),
            output: 1,
        },
    ];
    app.set_audio_patch(patch.clone());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.kuhplay");
    app.save_show(&path);
    let mut loaded = demo_app();
    loaded.load_show(&path).unwrap();
    // Das Legacy-Feld `output` wird nicht serialisiert (nur `outputs` zählt).
    for channel in &mut patch.channels {
        channel.output = 0;
    }
    assert_eq!(loaded.audio_patch, patch);
}

/// Audio-Tab zeigt die Routing-Matrix, Settings die Kanal-Tabelle.
#[test]
fn audio_routing_ui_renders() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.inspector_visible = true;
    app.carts.select(Some(2));
    app.ui.inspector_tab = InspectorTab::AudioLevels;
    render_frame(&ctx, &mut app);
    let out = render_frame(&ctx, &mut app);
    assert!(output_has_text(&out, "File 1"), "Matrix fehlt");
    assert!(output_has_text(&out, "Routing"));

    app.ui.workspace_settings_open = true;
    app.ui.workspace_settings_page = SettingsPage::Audio;
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "CHANNELS"));
    assert!(
        output_has_text(&out, "System Default"),
        "Matrix-Spalten fehlen"
    );
    assert!(output_has_text(&out, "Ch 1"), "Kanalzeilen fehlen");
}

/// Vorschau-Modus: ein Fenster pro Route; Displays öffnet die Device-Ausgabe.
#[test]
fn output_mode_preview_shows_window_per_route() {
    use kuhplay::core::{OutputRoute, Region, RouteManager};
    use kuhplay::gui::OutputMode;
    let ctx = egui::Context::default();
    let mut app = demo_app();
    // Route mit Full-Canvas-Region der Stage 1 (wie das Default-Setup).
    let route = OutputRoute::new("Beamer", 1920, 1080);
    let mut manager = RouteManager::new();
    manager.add(route.clone()).unwrap();
    app.routes = manager;
    let stage = app.stages.stages()[0].name.clone();
    if let Some(s) = app.stages.find_mut(&stage) {
        let mut region = Region::full_stage(s.geometry.width, s.geometry.height);
        region.route = Some(route.id);
        s.regions = vec![region];
    }
    app.configure_routes();

    // Vorschau: ein Fenster pro Route (Headless: Inhalt, nicht Titel).
    app.set_output_mode(OutputMode::Preview);
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "No output"));

    // Displays: randlose Fenster auf den belegten Displays (Route ohne
    // passendes Device zeigt nichts) – kein "No output"-Platzhalter mehr.
    app.set_output_mode(OutputMode::Displays);
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(!output_has_text(&out, "No output"));
    app.set_output_mode(OutputMode::Off);
    let out = render_frame(&ctx, &mut app);
    assert!(!output_has_text(&out, "No output"));
}

/// Langer Dateipfad: Das Feld lässt in der linken Spalte Platz für den Button.
#[test]
fn long_target_path_leaves_room_for_browse_button() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.inspector_visible = true;
    app.carts.select(Some(1));
    app.ui.inspector_tab = InspectorTab::Basics;
    let long = format!("/{}/clip.mp4", "sehr-langer-ordner/".repeat(30));
    app.carts.selected_mut().unwrap().target_path = Some(long);
    for _ in 0..3 {
        render_frame(&ctx, &mut app);
    }
    let out = render_frame(&ctx, &mut app);
    let pre_wait = text_pos(&out, "Pre Wait").expect("mittlere Spalte sichtbar");
    let field_right = out
        .shapes
        .iter()
        .find(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.starts_with("/sehr")),
        )
        .expect("Pfad sichtbar")
        .clip_rect
        .right();
    assert!(
        field_right + 32.0 < pre_wait.x,
        "Feld endet bei {field_right}, {pre_wait:?}"
    );
}

/// Stages: Umbenennen zieht die Cues mit, genutzte Stages bleiben.
#[test]
fn stages_can_be_renamed_and_removed_safely() {
    let mut app = demo_app();
    assert!(app.carts.stage_in_use("Stage 1"));
    assert!(!app.remove_stage("Stage 1"), "Cues nutzen sie noch");
    app.ensure_stage("Stage 1");
    app.rename_stage("Stage 1", "Hauptbild");
    assert!(app.stages.find("Hauptbild").is_some());
    assert!(app.stages.find("Stage 1").is_none());
    assert!(app.carts.stage_in_use("Hauptbild"));
    assert_eq!(
        app.carts.active().cues()[1].stage.as_deref(),
        Some("Hauptbild")
    );

    app.ensure_stage("Frei");
    app.rename_stage("Frei", "Hauptbild");
    assert!(
        app.stages.find("Frei").is_some(),
        "vergebener Name bleibt abgelehnt"
    );
    assert!(app.remove_stage("Frei"));
    assert!(app.stages.find("Frei").is_none());
}

/// Video-Einstellungen: Stages-Seite zeigt die Stage und die Aktionen.
#[test]
fn stages_page_renders() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.workspace_settings_open = true;
    app.ui.workspace_settings_page = SettingsPage::Video;
    app.ui.video_tab = kuhplay::gui::workspace_settings::VideoTab::Stages;
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "+ New Stage"));
    assert!(output_has_text(&out, "Delete"));
}

#[test]
fn inspector_renders_waveform_for_audio_cue() {
    let ctx = egui::Context::default();
    let mut app = demo_app();

    // Sinus-WAV als Target eines Audio-Cues.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sine.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..4000 {
        let s = ((i as f32 / 8000.0) * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.8;
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();

    // Ersten Audio-Cue selektieren und Target setzen.
    let idx = app
        .carts
        .active()
        .cues()
        .iter()
        .position(|c| c.cue_type == CueType::Audio)
        .expect("demo app hat audio cue");
    app.carts.active_mut().select(Some(idx));
    app.carts.active_mut().cues_mut()[idx].target_path = Some(path.display().to_string());

    // Frame rendern (Waveform-Zweig zeichnen) – zweimal (zweiter Lauf: Cache-Hit).
    render_frame(&ctx, &mut app);
    render_frame(&ctx, &mut app);

    // Peaks landen im App-Cache.
    let w = app.waveform_for(&path).expect("dekodierbar");
    assert!(w.peak() > 0.7, "sinus-peak zu klein: {}", w.peak());
}

#[test]
fn masthead_renders_audition_badge() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.set_audition_mode(true);
    // Badge rendern (zweimal – Cache-/State-Pfade abdecken).
    render_frame(&ctx, &mut app);
    render_frame(&ctx, &mut app);
}

#[test]
fn show_save_load_roundtrip() {
    let mut app = demo_app();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("demo.kuhplay");

    let cues_before = app.carts.active().len();
    app.save_show(&path);
    assert!(path.is_file(), "Show-Datei muss geschrieben werden");

    // Show verändern, dann zurückladen → Zustand wiederhergestellt.
    app.new_show();
    assert_eq!(app.carts.active().len(), 0);
    app.load_show(&path).expect("Show muss laden");
    assert_eq!(app.carts.active().len(), cues_before);
    assert_eq!(app.show_name, "Demo Show");
}

/// viewport_window::draw rendert im Headless-Fallback (Embedded) ein Overlay
/// mit Inhalt (ab dem zweiten Frame, wie egui::Window) und hält das open-Flag.
#[test]
fn viewport_window_embedded_fallback_renders() {
    use kuhplay::gui::viewport_window::{draw, WindowSpec};
    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut open = true;
    let mut counter = 0u32;
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    let mut render = |ctx: &egui::Context| {
        ctx.run(raw_input.clone(), |ctx| {
            draw(
                ctx,
                &WindowSpec::new(
                    "test-viewport",
                    "Test Fenster",
                    [300.0, 200.0],
                    [200.0, 100.0],
                ),
                &mut open,
                &mut counter,
                |_, ui, counter| {
                    ui.label("Viewport-Inhalt");
                    *counter += 1;
                },
            );
        })
    };
    let _ = render(&ctx);
    let out = render(&ctx);
    assert!(
        output_has_text(&out, "Viewport-Inhalt"),
        "Inhalt fehlt im Fallback"
    );
    assert_eq!(counter, 2, "Content läuft pro Frame einmal");
    assert!(open, "Flag bleibt ohne Close-Request offen");
}

/// Output-Routing-Tab: Device-Karte rendert die Route als Rechteck mit
/// Bereichsangabe (nur wenn im Testprozess Displays erkannt werden).
#[test]
fn output_routing_device_map_renders_route_area() {
    use kuhplay::core::{DeviceArea, DeviceRef, OutputRoute};
    use kuhplay::gui::workspace_settings::VideoTab;
    let ctx = egui::Context::default();
    let mut app = demo_app();
    if app.displays.is_empty() {
        return; // headless ohne WindowServer-Displays
    }
    let d = app.displays[0].clone();
    let mut route = OutputRoute::new("LED Wand", 800, 600);
    route.device = Some(DeviceRef::Display {
        id: d.id,
        name: d.name.clone(),
        width: d.width,
        height: d.height,
    });
    route.device_area = DeviceArea::Part {
        x: 100,
        y: 50,
        width: 800,
        height: 600,
    };
    app.routes.add(route).unwrap();
    app.configure_routes();
    app.ui.workspace_settings_open = true;
    app.ui.workspace_settings_page = SettingsPage::Video;
    app.ui.video_tab = VideoTab::Routing;
    let mut out = egui::FullOutput::default();
    for _ in 0..3 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(
        output_has_text(&out, "LED Wand"),
        "Route fehlt auf der Karte/Tabelle"
    );
    assert!(
        output_has_text(&out, "800×600 px"),
        "Part-Bereichsangabe fehlt"
    );
}

/// Multi-Output 2×2: 4 Regionen im Raster (row-major), Stage-Größe
/// 2w−overlap_x × 2h−overlap_y, 4 Routen.
#[test]
fn multi_output_wizard_2x2_grid() {
    let mut app = demo_app();
    let routes_before = app.routes.len();
    app.add_multi_output_stage(&kuhplay::gui::app::MultiOutputWizard {
        cols: 2,
        rows: 2,
        width: 1920,
        height: 1080,
        overlap_x: 100,
        overlap_y: 50,
    });
    let stage = app.stages.stages().last().cloned().unwrap();
    assert_eq!(stage.regions.len(), 4, "eine Region je Projektor (2×2)");
    assert!((stage.geometry.width - (2.0 * 1920.0 - 100.0)).abs() < 0.5);
    assert!((stage.geometry.height - (2.0 * 1080.0 - 50.0)).abs() < 0.5);
    let mut pos: Vec<(f32, f32)> = stage.regions.iter().map(|r| (r.x, r.y)).collect();
    pos.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap()
            .then(a.0.partial_cmp(&b.0).unwrap())
    });
    assert_eq!(
        pos,
        vec![(0.0, 0.0), (1820.0, 0.0), (0.0, 1030.0), (1820.0, 1030.0)],
        "Raster-Positionen row-major"
    );
    assert_eq!(
        app.routes.len(),
        routes_before + 4,
        "eine Route je Projektor"
    );
    assert!(stage.regions.iter().all(|r| r.route.is_some()));
}

/// Active-Cues-Zeile: Uhr-Text liegt vollständig innerhalb der Zeile (kein
/// Clipping unten), und `seek_running` schickt EngineCommand::Seek an die Engine.
#[test]
fn active_cue_row_not_clipped_and_seek_dispatches() {
    let ctx = egui::Context::default();
    let (mut app, cmd_rx, evt_tx) = manual_app();
    icons::install(&ctx);
    // Demo-Cues feuern (Mock-Engine): zwei laufende Cues.
    app.go_now();
    app.go_now();
    // Gestartet-Event mit Medien-Dauer → media_durations.
    let running: Vec<_> = app
        .carts
        .active()
        .cues()
        .iter()
        .filter(|c| c.status.is_active())
        .map(|c| c.id)
        .collect();
    assert!(!running.is_empty());
    for id in &running {
        evt_tx
            .send(EngineEvent::Started {
                cue_id: *id,
                duration: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
    }
    app.poll_engine_events();
    assert!(
        running
            .iter()
            .all(|id| app.carts.find_cue_any(*id).is_some_and(
                |c| app.media_duration_of(c) == Some(std::time::Duration::from_secs(10))
            )),
        "media_durations muss aus dem Started-Event übernommen werden"
    );
    app.ui.sidebar_visible = true;
    app.ui.sidebar_tab = SidebarTab::ActiveCues;
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(raw.clone(), |ctx| app.draw_workspace(ctx));
    let out = ctx.run(raw, |ctx| app.draw_workspace(ctx));
    // Zeilen-Rechtecke (Fill #2D2D34) und Text-Shapes im Sidebar-Bereich (x > 1200).
    let mut row_rects: Vec<egui::Rect> = Vec::new();
    for s in &out.shapes {
        if let egui::Shape::Rect(r) = &s.shape {
            if r.rect.height() == 48.0 && r.rect.width() > 100.0 {
                row_rects.push(r.rect);
            }
        }
    }
    assert!(!row_rects.is_empty(), "keine Active-Cue-Zeilen gefunden");
    for s in &out.shapes {
        if let egui::Shape::Text(t) = &s.shape {
            let txt = &t.galley.job.text;
            let bottom = t.pos.y + t.galley.size().y;
            if t.pos.x > 1200.0 && txt.chars().any(|c| c == ':' || c == '−') {
                let row = row_rects.iter().find(|r| r.contains(t.pos));
                if let Some(row) = row {
                    assert!(
                        bottom <= row.bottom() + 0.5,
                        "Uhr {:?} ragt aus der Zeile (bottom {bottom:.1} > {:.1})",
                        txt,
                        row.bottom()
                    );
                }
            }
        }
    }
    // seek_running → EngineCommand::Seek mit Position.
    while cmd_rx.try_recv().is_ok() {}
    app.seek_running(running[0], std::time::Duration::from_millis(2500));
    let mut saw_seek = false;
    while let Ok(cmd) = cmd_rx.try_recv() {
        if let EngineCommand::Seek { cue_id, position } = cmd {
            assert_eq!(cue_id, running[0]);
            assert_eq!(position, std::time::Duration::from_millis(2500));
            saw_seek = true;
        }
    }
    assert!(saw_seek, "kein EngineCommand::Seek nach seek_running");
}

/// Kill-Switch: Cue mit kill_others stoppt beim GO alle anderen laufenden
/// Media-Cues (Video/Diashow/HTML) per EngineCommand::Stop.
#[test]
fn kill_others_stops_other_running_media_cues() {
    let (mut app, cmd_rx, _evt_tx) = manual_app();
    // Drei Video-Cues anlegen (ID jeweils per Diff, add_cue liefert keine ID).
    let mut ids = Vec::new();
    for _ in 0..3 {
        let before: std::collections::HashSet<_> =
            app.carts.active().cues().iter().map(|c| c.id).collect();
        app.add_cue(CueType::Video);
        let after: std::collections::HashSet<_> =
            app.carts.active().cues().iter().map(|c| c.id).collect();
        ids.push(*after.difference(&before).next().unwrap());
    }
    let (c1, c2, c3) = (ids[0], ids[1], ids[2]);
    app.go_now();
    app.go_now();
    if let Some(cue) = app
        .carts
        .active_mut()
        .cues_mut()
        .iter_mut()
        .find(|c| c.id == c3)
    {
        cue.kill_others = true;
    }
    // Status beider alter Cues auf Playing setzen (simuliert laufend).
    for id in [c1, c2] {
        let _ = app.carts.set_status(id, CueStatus::Playing);
    }
    while cmd_rx.try_recv().is_ok() {}
    let idx3 = app
        .carts
        .active()
        .cues()
        .iter()
        .position(|c| c.id == c3)
        .unwrap();
    app.carts.set_cursor(idx3); // Playhead auf den Kill-Cue
    app.go_now();
    let mut stopped = Vec::new();
    while let Ok(cmd) = cmd_rx.try_recv() {
        if let EngineCommand::Stop { cue_id } = cmd {
            stopped.push(cue_id);
        }
    }
    assert!(
        stopped.contains(&c1) && stopped.contains(&c2),
        "beide laufenden Cues müssen gestoppt werden: {stopped:?}"
    );
    assert!(
        !stopped.contains(&c3),
        "der neue Cue selbst darf nicht gestoppt werden"
    );
}

/// Test-Grid (Footer-Icon): setzt region.grid auf allen Stages und wird im
/// Show-Mode gezwungen abgeschaltet.
#[test]
fn test_grid_toggle_and_show_mode_forces_off() {
    let mut app = demo_app();
    app.set_test_grid(true);
    assert!(app.ui.test_grid);
    assert!(app
        .stages
        .stages()
        .iter()
        .flat_map(|s| s.regions.iter())
        .all(|r| r.grid));
    app.set_test_grid(false);
    assert!(app
        .stages
        .stages()
        .iter()
        .flat_map(|s| s.regions.iter())
        .all(|r| !r.grid));
    // Show-Mode: Grid immer aus.
    app.set_test_grid(true);
    app.set_show_mode(true);
    assert!(!app.ui.test_grid);
    assert!(app
        .stages
        .stages()
        .iter()
        .flat_map(|s| s.regions.iter())
        .all(|r| !r.grid));
    app.set_show_mode(false);
}
