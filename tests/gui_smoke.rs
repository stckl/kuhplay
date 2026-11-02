//! Headless GUI-Smoke-Tests (Workspace).
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
fn edit_actions_work() {
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
fn auto_continue_fires_next_cue_only_after_first_completes() {
    // Semantik: Auto-Continue feuert den Folge-Cue erst beim
    // Abschluss des laufenden Cues (Finished), nicht beim GO.
    let (mut app, _cmd_rx, evt_tx) = manual_app();
    app.carts.active_mut().cues_mut()[0].continue_mode = ContinueMode::AutoContinue;
    let first_id = app.carts.active().cues()[0].id;
    app.go_now();
    assert_eq!(app.carts.running_count(), 1, "beim GO nur ein Cue");
    assert_eq!(app.carts.cursor(), 1);
    let second_id = app.carts.active().cues()[1].id;
    assert_eq!(
        app.carts.active().cues()[1].status,
        CueStatus::Standby,
        "Folge-Cue steht noch"
    );

    // Erster Cue abgeschlossen → Auto-Continue feuert den zweiten.
    evt_tx
        .send(EngineEvent::Finished { cue_id: first_id })
        .unwrap();
    app.poll_engine_events();
    assert_eq!(app.carts.cursor(), 2, "Playhead rückt weiter");
    assert_eq!(
        app.carts.find_cue_any(second_id).unwrap().status,
        CueStatus::Playing,
        "Auto-Continue hat den Folge-Cue gefeuert"
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
    // Video-Seite (Workspace Settings → Video, Tabs): rendert ohne
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
    if app.displays.is_empty() {
        // CI-Runner ohne Display: keine Default-Route möglich.
        println!("SKIP Route-Assert: keine Displays in dieser Umgebung");
    } else {
        assert!(app.stages.stages()[0]
            .regions
            .iter()
            .any(|r| r.route.is_some()));
    }

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

    let route_id = app.add_route();
    assert!(app.routes.find(route_id).is_some());

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
fn workspace_templates_save_apply_delete_and_persist() {
    let mut app = demo_app();
    app.ui.toolbox_visible = false;
    app.ui.sidebar_tab = SidebarTab::ActiveCues;

    // Aktuelles Layout als Template speichern (Rename bei gleichem Namen).
    app.save_workspace_template("Stage".to_string());
    app.save_workspace_template("Setup".to_string());
    app.save_workspace_template("Stage".to_string()); // ersetzt, nicht doppelt
    assert_eq!(app.workspace_templates.len(), 2);
    // Rename-Semantik: „Stage" wurde ans Ende geschoben.
    assert_eq!(app.workspace_templates[0].name, "Setup");
    assert_eq!(app.workspace_templates[1].name, "Stage");
    assert!(!app.workspace_templates[1].layout.toolbox_visible);

    // Leerer Name wird ignoriert.
    app.save_workspace_template("   ".to_string());
    assert_eq!(app.workspace_templates.len(), 2);

    // Persistenz: save → frische App lädt die Templates.
    let mut storage = MemStorage(std::collections::HashMap::new());
    app.save_ui_state(&mut storage);
    app.save_ui_templates(&mut storage);
    let mut loaded = demo_app();
    assert!(loaded.workspace_templates.is_empty());
    let templates_json =
        eframe::Storage::get_string(&storage, kuhplay::gui::state::UI_TEMPLATES_STORAGE_KEY)
            .expect("Templates persistiert");
    loaded.workspace_templates = kuhplay::gui::state::templates_from_json(&templates_json);
    assert_eq!(loaded.workspace_templates, app.workspace_templates);

    // Apply: Layout wird wiederhergestellt, Find-Laufzeitzustand bleibt.
    loaded.ui.toolbox_visible = true;
    loaded.ui.find.query = "abc".to_string();
    loaded.apply_workspace_template(1); // „Stage"
    assert!(!loaded.ui.toolbox_visible, "Template-Layout angewendet");
    assert_eq!(loaded.ui.find.query, "abc", "Find bleibt Laufzeit-Zustand");

    // Delete.
    loaded.delete_workspace_template(0); // löscht „Setup"
    assert_eq!(loaded.workspace_templates.len(), 1);
    assert_eq!(loaded.workspace_templates[0].name, "Stage");
}

#[test]
fn footer_shows_cue_count_and_broken_indicator() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx); // Footer malt Icon-Glyphen (Phosphor-Font nötig).
                          // Demo-Show: 11 Cues in 1 Liste; Media-Dateien fehlen (/media/… existiert
                          // nicht) → kaputte Cues werden im Footer gezählt (Warnzähler).
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

/// Die Toolbar liegt im Masthead und lässt sich ausblenden (docs/UI.md §5.1).
#[test]
fn masthead_toolbar_can_be_hidden() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    let audio_icon = kuhplay::gui::icons::Icon::for_cue_type(kuhplay::core::CueType::Audio).glyph();
    let draw = |app: &mut ShowApp| {
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
                masthead::draw(ui, app);
            });
        })
    };
    let out = draw(&mut app);
    assert!(
        output_has_text(&out, audio_icon),
        "Toolbar zeigt die Cue-Typen"
    );
    app.ui.toolbar_visible = false;
    let out = draw(&mut app);
    assert!(
        !output_has_text(&out, audio_icon),
        "Toolbar ist ausgeblendet"
    );
}

/// Der globale Transport sitzt oben in der Sidebar (docs/UI.md §5.4).
#[test]
fn sidebar_renders_transport_and_panic() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    icons::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::SidePanel::right("sidebar").show(ctx, |ui| {
            kuhplay::gui::sidebar::draw(ui, &mut app);
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
    // Seite Video: zwei Reiter (Stages/Devices).
    app.ui.workspace_settings_page = SettingsPage::Video;
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "Video Outputs (Stages)"));
    assert!(!output_has_text(&out, "Output Routing"));
    assert!(output_has_text(&out, "Output Devices"));
}

// ------------------------------------------------------------------
// Phase 2 (docs/TODO-offen.md): Status-Semantik, Broken-✕ + Tooltip,
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
        decklink_stats: std::sync::Arc::new(std::sync::Mutex::new(
            kuhplay::engine::decklink::DecklinkStats::default(),
        )),
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

    // Gesunde Liste (Wait-Cues ohne Datei-Abhängigkeit): kein ✕.
    let mut healthy = demo_app();
    {
        let list = healthy.carts.active_mut();
        list.cues_mut().clear();
        list.add(Cue::new("1", "Wait 1", CueType::Wait));
        list.add(Cue::new("2", "Wait 2", CueType::Wait));
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
    // … und Tönung der ganzen Zeile (niedrige Deckkraft).
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

/// Header: Die Spalten heißen „Pre-wait“, „Duration“, „Post-wait“
/// („Action" ist die alte
/// Bezeichnung und darf nicht mehr als Header auftauchen).
#[test]
fn cue_table_header_shows_column_titles() {
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
        "Duration-Spalte heißt „Duration“, nicht „Action“"
    );
}

/// Laufender Cue: Die Duration-Zelle zeigt die RESTZEIT (Duration −
/// elapsed). Meta manuell → deterministisch.
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
            ..Default::default()
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
            ..Default::default()
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
/// gerendert (Spalten konfigurierbar).
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
    // Phase-2-Semantik: Standby (auch armiert) ist neutral – Rot wäre
    // „kaputt". Der Standby-Text darf nicht in einer Statusfarbe stehen.
    let color = output_text_color(&out, "1 • House to Half").expect("Standby-Text gerendert");
    assert_eq!(
        color,
        palette::TEXT,
        "Standby-Anzeige ist neutral (kein Rot/Amber)"
    );
}

// ------------------------------------------------------------------
// Phase 4 (docs/TODO-offen.md): Inspector-Restrukturierung – Kopfzeile,
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
/// (Tabs je nach Cue-Typ).
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
/// „N cues selected".
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

/// Show-Mode muss sofort erkennbar sein (docs/TODO-offen.md Phase 6): eingefärbter
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

/// Leere Cue-Liste zeigt einen Hinweis (docs/TODO-offen.md Phase 6).
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

/// Inspector ohne Selektion: „No cue selected" zentriert (docs/TODO-offen.md Phase 6).
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
        .any(|x| (500.0..900.0).contains(&x));
    assert!(centered, "„No cue selected“ nicht zentriert im Inspector");
}

/// Active-Cues-Tab (docs/TODO-offen.md Phase 6): Typ-Icon + Nummer/Name + Zeiten;
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
    app.ui.toolbar_visible = false;
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
    let devices = output_devices();
    #[cfg(target_os = "linux")]
    if !std::path::Path::new("/dev/snd").exists() {
        // Linux-CI-Runner: ALSA ohne Soundkarte – sauber überspringen.
        println!("SKIP default_output_is_listed: kein /dev/snd (CI-Runner ohne Soundkarte)");
        return;
    }
    if devices.is_empty() {
        // CI-Runner ohne Audio-Ausgabegerät: sauber überspringen.
        println!("SKIP default_output_is_listed: kein Audio-Gerät in dieser Umgebung");
        return;
    }
    if let Some(name) = default_output_name() {
        assert!(devices.iter().any(|d| d.name == name));
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
fn output_mode_preview_shows_window_per_stage() {
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

    // Vorschau: ein Fenster pro Stage (Headless: Inhalt, nicht Titel).
    app.set_output_mode(OutputMode::Preview);
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "No picture"));

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
/// Media-Cues (Video/Diashow/HTML), sobald sein eigenes Bild steht.
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
        if let EngineCommand::StopWhenVisible { cue_id, stops, .. } = cmd {
            assert_eq!(cue_id, c3, "die alten warten auf das Bild des neuen");
            stopped = stops;
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
    assert!(app.stages.stages().iter().all(|s| s.grid));
    app.set_test_grid(false);
    assert!(app.stages.stages().iter().all(|s| !s.grid));
    // Show-Mode: Grid immer aus.
    app.set_test_grid(true);
    app.set_show_mode(true);
    assert!(!app.ui.test_grid);
    assert!(app.stages.stages().iter().all(|s| !s.grid));
    app.set_show_mode(false);
}

/// Copy/Paste Cue Properties: Properties des selektierten Cues landen in der
/// Zwischenablage und werden per Paste auf andere Cues gemergt (ID/Nummer bleiben).
#[test]
fn copy_paste_cue_properties() {
    let (mut app, _cmd_rx, _evt_tx) = manual_app();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let before: std::collections::HashSet<_> =
            app.carts.active().cues().iter().map(|c| c.id).collect();
        app.add_cue(CueType::Audio);
        let after: std::collections::HashSet<_> =
            app.carts.active().cues().iter().map(|c| c.id).collect();
        ids.push(*after.difference(&before).next().unwrap());
    }
    // Cue 1: Properties setzen (Name, Pre-Wait) und selektieren → kopieren.
    {
        let cue = app
            .carts
            .active_mut()
            .cues_mut()
            .iter_mut()
            .find(|c| c.id == ids[0])
            .unwrap();
        cue.name = "Quelle".to_string();
        cue.pre_wait = Duration::from_secs(2);
        app.carts.active_mut().select_id(Some(ids[0]));
    }
    app.copy_cue_properties();
    assert!(app.has_properties_clipboard());
    // Cue 2 selektieren → pasten.
    app.carts.active_mut().select_id(Some(ids[1]));
    let applied = app.paste_cue_properties();
    assert_eq!(applied, 1);
    let pasted = app
        .carts
        .active()
        .cues()
        .iter()
        .find(|c| c.id == ids[1])
        .unwrap();
    assert_eq!(pasted.name, "Quelle");
    assert_eq!(pasted.pre_wait, Duration::from_secs(2));
    assert_ne!(pasted.id, ids[0], "ID darf nicht übernommen werden");
}

#[test]
fn target_search_window_lists_file_targets_and_filters() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.target_search_open = true;
    // Fenster brauchen 2 Pässe (erster = Sizing).
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(
        output_has_text(&out, "File Target Search"),
        "Fenster-Titel fehlt"
    );
    assert!(output_has_text(&out, "file targets"), "Zähler-Zeile fehlt");
    // Demo-Show hat Datei-Ziele (z. B. intro.mp4).
    assert!(
        output_has_text(&out, "intro.mp4"),
        "Datei-Ziel intro.mp4 fehlt"
    );

    // Filter auf einen nicht vorhandenen Pfad → 0 Treffer.
    app.ui.target_search_query = "gibt-es-nicht-xyz".to_string();
    out = render_frame(&ctx, &mut app);
    assert!(
        output_has_text(&out, "0 of"),
        "Filter muss Trefferzahl auf 0 drücken"
    );

    // Filter auf „intro" → genau der eine Treffer bleibt sichtbar.
    app.ui.target_search_query = "intro".to_string();
    out = render_frame(&ctx, &mut app);
    assert!(
        output_has_text(&out, "1 of"),
        "Filter 'intro' muss 1 Treffer liefern"
    );
    assert!(output_has_text(&out, "intro.mp4"));
}

#[test]
fn status_window_shows_show_mode_cart_and_running_section() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.status_window_open = true;
    // Fenster brauchen 2 Pässe (erster = Sizing).
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(
        output_has_text(&out, "Workspace Status"),
        "Fenster-Titel fehlt"
    );
    assert!(output_has_text(&out, "Demo Show"), "Show-Name fehlt");
    assert!(output_has_text(&out, "Mode: EDIT"), "Mode-Zeile fehlt");
    assert!(output_has_text(&out, "Active cart:"), "Cart-Zeile fehlt");
    assert!(output_has_text(&out, "Playhead:"), "Playhead-Zeile fehlt");
    assert!(
        output_has_text(&out, "Running cues"),
        "Running-Cues-Section fehlt"
    );
    assert!(
        output_has_text(&out, "cues in active list"),
        "Cue-Count-Zeile fehlt"
    );
    // Broken-Status-Zeile (Demo-Show kann kaputte Cues enthalten).
    assert!(
        output_has_text(&out, "broken cue"),
        "Broken-Cues-Zeile fehlt"
    );

    // Im Show-Mode wechselt die Anzeige.
    app.show_mode = true;
    out = render_frame(&ctx, &mut app);
    assert!(output_has_text(&out, "Mode: SHOW"));
}

#[test]
fn audition_window_shows_playing_cue_or_empty_state() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.ui.audition_window_open = true;

    // Leerzustand: kein laufender visueller Cue.
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    assert!(output_has_text(&out, "Audition"), "Fenster-Titel fehlt");
    assert!(
        output_has_text(&out, "No video cue playing"),
        "Leerzustand fehlt"
    );

    // Video-Cue auf Playing setzen + Frame in den Frame-Store legen.
    let (cue_id, cue_number) = {
        let list = app.carts.active_mut();
        let idx = list
            .cues()
            .iter()
            .position(|c| c.cue_type == CueType::Video)
            .expect("Demo-Show hat einen Video-Cue");
        list.cues_mut()[idx].status = CueStatus::Playing;
        let cue = &list.cues()[idx];
        (cue.id, cue.number.clone())
    };
    app.engine.frames.lock().unwrap().insert(
        cue_id,
        std::sync::Arc::new(kuhplay::engine::VideoFrame::new(
            4,
            4,
            vec![200u8; 4 * 4 * 4],
        )),
    );
    out = render_frame(&ctx, &mut app);
    assert!(
        output_has_text(&out, &format!("Q{cue_number}")),
        "Laufender Cue muss im Audition-Fenster erscheinen: Q# fehlt"
    );
    assert!(output_has_text(&out, "Intro Video"), "Cue-Name fehlt");
}

#[test]
fn demo_show_contains_timetable_html_and_data_cues() {
    let app = demo_app();
    let list = app.carts.active();
    let html = list
        .cues()
        .iter()
        .find(|c| c.cue_type == CueType::Html && c.name.contains("Timetable"))
        .expect("Demo-Show hat einen Timetable-HTML5-Cue");
    let Some(kuhplay::core::HtmlSource::Inline(src)) = &html.html_source else {
        panic!("Timetable-Quelle muss inline eingebettet sein");
    };
    assert!(
        src.contains("window.kuhplay"),
        "Bridge-Hook fehlt im Template"
    );
    assert!(
        src.contains("onShowControlData"),
        "Data-Hook fehlt im Template"
    );
    assert!(src.contains("100vw"), "Template muss in vw/vh skalieren");

    let data = list
        .cues()
        .iter()
        .find(|c| c.cue_type == CueType::HtmlData && c.name.contains("Timetable"))
        .expect("Demo-Show hat einen Timetable-Data-Cue");
    assert_eq!(
        data.data_target,
        Some(html.id),
        "Data-Cue zielt auf den HTML-Cue"
    );
    // Payload ist valides JSON mit blocks/speakers (zeitplan.json).
    let payload = data.data_payload().expect("Data-Payload parst als JSON");
    let blocks = payload
        .as_array()
        .expect("zeitplan.json ist ein Array von Blöcken");
    assert!(!blocks.is_empty());
    assert!(blocks[0].get("speakers").is_some());
}

#[test]
fn stage_editor_shows_display_placements_section() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    // Stage-Editor für Stage 1 öffnen.
    let stage_id = app.stages.stages()[0].id;
    app.stage_editor_open = Some(stage_id);
    // Placement anlegen (Display-ID 1 – im Testprozess ohne echte Displays
    // rendert die Map den „No displays"-Hinweis, die Section selbst aber da).
    {
        let st = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
            .unwrap();
        st.placements.push(kuhplay::core::StagePlacement {
            display: 1,
            x: -100,
            y: 20,
            width: 2000,
            height: 1080,
        });
    }
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    // section_label rendert in Großbuchstaben.
    assert!(
        output_has_text(&out, "PLACEMENT"),
        "Placement-Ansicht fehlt im Stage-Editor"
    );
    if app.displays.is_empty() {
        // CI/Runner ohne Display: Hinweis-Zeile.
        assert!(
            output_has_text(&out, "No displays detected"),
            "Display-Hinweis fehlt (keine Displays in dieser Umgebung)"
        );
    } else {
        // Mit Displays: Combo mit Pixelmaßen, kein Button zum Platzieren.
        assert!(!output_has_text(&out, "Place stage on this display"));
        // Jedes Display steht mit seinen Pixelmaßen in der Ansicht.
        let display_label = out.shapes.iter().any(|shape| {
            matches!(shape.shape, egui::Shape::Text(ref t)
                if t.galley.job.text.contains('×') && t.galley.job.text.ends_with(')'))
        });
        assert!(display_label, "Display fehlt in der Placement-Ansicht");
    }
}

#[test]
fn f1_show_mode_badge_und_footer_toggles_und_pause_gelb() {
    // F1.1: Pausiert ist gelb.
    assert_eq!(
        kuhplay::gui::theme::palette::PAUSED,
        egui::Color32::from_rgb(255, 195, 0)
    );
    let ctx = egui::Context::default();
    let mut app = demo_app();
    app.show_mode = true;
    app.ui.status_window_open = true;
    let mut out = render_frame(&ctx, &mut app);
    for _ in 0..2 {
        out = render_frame(&ctx, &mut app);
    }
    // F1.6: Nebenfenster zeigen SHOW MODE im Footer.
    assert!(
        output_has_text(&out, "SHOW MODE"),
        "SHOW-MODE-Badge im Status-Fenster fehlt"
    );
    // F1.4: Footer hat Toggles für Toolbox, Masthead und Toolbar.
    for (icon, name) in [
        (kuhplay::gui::icons::Icon::PanelToolbox, "Toolbox"),
        (kuhplay::gui::icons::Icon::PanelMasthead, "Masthead"),
        (kuhplay::gui::icons::Icon::PanelToolbar, "Toolbar"),
    ] {
        assert!(
            output_has_text(&out, icon.glyph()),
            "{name}-Toggle-Icon fehlt"
        );
    }
    // F1.5: Masthead-Höhe persistiert im UiState.
    assert!(app.ui.masthead_height >= 48.0);
}

/// Das Dreieck am Zeilenanfang markiert den Playhead deutlich (gefüllte Form in Akzentfarbe).
#[test]
fn playhead_row_shows_filled_triangle() {
    let ctx = egui::Context::default();
    let mut app = demo_app();
    render_frame(&ctx, &mut app);
    let out = render_frame(&ctx, &mut app);
    fn has_triangle(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Path(path) => path.points.len() == 3 && path.fill == palette::ACCENT,
            egui::Shape::Vec(shapes) => shapes.iter().any(has_triangle),
            _ => false,
        }
    }
    assert!(
        out.shapes.iter().any(|s| has_triangle(&s.shape)),
        "Playhead-Dreieck fehlt"
    );
}

/// Active-Cue-Zeile: Pause, Typ-Icon und Stop liegen auf der Zeilenmitte, ein
/// langer Name bleibt innerhalb der Zeile und läuft nicht in den Stop-Button.
#[test]
fn active_cue_row_centers_buttons_and_contains_long_names() {
    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut app = demo_app();
    let long_name = "Ein sehr langer Cue-Name ".repeat(8);
    let index = app.carts.cursor();
    app.carts.active_mut().cues_mut()[index].name = long_name;
    app.go_now();
    assert_eq!(app.carts.running_count(), 1);
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

    let row = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect.height() == 48.0 && r.rect.width() > 100.0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("Active-Cue-Zeile fehlt");

    let mut icons_in_row = 0;
    let mut texts_in_row = 0;
    for s in &out.shapes {
        let egui::Shape::Text(t) = &s.shape else {
            continue;
        };
        let rect = egui::Rect::from_min_size(t.pos, t.galley.rect.size());
        if !row.contains(rect.center()) {
            continue;
        }
        texts_in_row += 1;
        assert!(
            rect.right() <= row.right(),
            "Text „{}“ ragt aus der Zeile: {rect:?} / {row:?}",
            t.galley.job.text
        );
        // Icon-Fonts malen genau ein Zeichen aus dem privaten Unicode-Bereich.
        if t.galley
            .job
            .text
            .chars()
            .all(|c| ('\u{E000}'..='\u{F8FF}').contains(&c))
        {
            icons_in_row += 1;
            assert!(
                (rect.center().y - row.center().y).abs() <= 1.5,
                "Icon nicht mittig: {rect:?} / {row:?}"
            );
        }
    }
    assert_eq!(icons_in_row, 3, "Pause, Typ-Icon und Stop");
    assert!(texts_in_row >= 5, "auch Name und Zeit stehen in der Zeile");
}

/// Toolbar: alle Buttons sind schwarz, gleich groß und liegen auf einer Linie.
/// Die Toolbox endet wie die Cue-Liste über dem Inspector.
#[test]
fn toolbar_buttons_share_one_line_and_toolbox_ends_above_inspector() {
    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut app = demo_app();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(raw.clone(), |ctx| app.draw_workspace(ctx));
    let out = ctx.run(raw, |ctx| app.draw_workspace(ctx));

    let buttons: Vec<egui::Rect> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r)
                if r.fill == egui::Color32::BLACK && r.rect.size() == egui::vec2(32.0, 32.0) =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(buttons.len(), 19, "ein Button pro Cue-Typ");
    assert!(buttons.iter().all(|b| b.top() == buttons[0].top()));

    let toolbox = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("toolbox"))
        .expect("Toolbox-Panel");
    let sidebar = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("sidebar"))
        .expect("Sidebar-Panel");
    assert_eq!(toolbox.rect.bottom(), sidebar.rect.bottom());
    assert_eq!(toolbox.rect.top(), sidebar.rect.top());
}

/// Die Farbe eines Ausgangs lässt sich ändern und geht an die Engine.
#[test]
fn route_color_can_be_changed() {
    let mut app = demo_app();
    let id = app.add_route();
    app.set_route_color(id, [1, 2, 3]);
    assert_eq!(app.routes.find(id).map(|r| r.color), Some([1, 2, 3]));
}

/// „Add Key Region“ legt ein Key-Gegenstück mit gleichem Crop an.
#[test]
fn add_key_region_button_creates_a_key_with_the_same_crop() {
    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut app = demo_app();
    let stage = app.stages.find("Stage 1").cloned().unwrap();
    if app.displays.is_empty() || stage.regions.is_empty() {
        return; // headless ohne Displays
    }
    app.stage_editor_open = Some(stage.id);
    let mut time = 1.0;
    let mut frame = |app: &mut ShowApp, events: Vec<egui::Event>| {
        time += 0.05;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 1600.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        ctx.run(raw, |ctx| app.draw_workspace(ctx))
    };
    let _ = frame(&mut app, vec![]);
    let out = frame(&mut app, vec![]);
    assert!(
        !output_has_text(&out, "Key/Fill"),
        "kein eigener Key/Fill-Bereich"
    );
    let button = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "+ Add Key Region" => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()).center())
            }
            _ => None,
        })
        .expect("Button fehlt");
    let press = |pressed: bool| egui::Event::PointerButton {
        pos: button,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = frame(&mut app, vec![egui::Event::PointerMoved(button)]);
    let _ = frame(&mut app, vec![press(true)]);
    let _ = frame(&mut app, vec![press(false)]);
    let _ = frame(&mut app, vec![]);

    let stage = app.stages.find("Stage 1").cloned().unwrap();
    assert_eq!(stage.regions.len(), 2);
    let key = &stage.regions[1];
    assert!(key.key);
    assert_eq!(key.width, stage.regions[0].width);
    assert!(app.routes.find(key.route.unwrap()).unwrap().alpha);
}

/// Bedienung im Stage-Editor über echte Zeigerereignisse: Region ziehen, dann
/// der 1:1-Button zurück auf Original-Pixelgröße. Stage-Einstellungen sind weg.
#[test]
fn stage_editor_moves_region_and_one_to_one_button_resets_it() {
    use kuhplay::core::DeviceArea;
    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut app = demo_app();
    if app.displays.is_empty() {
        return; // headless ohne Displays
    }
    let stage = app.stages.find("Stage 1").cloned().unwrap();
    let route_id = stage.regions[0].route.unwrap();
    let route = app.routes.find(route_id).cloned().unwrap();
    let (w, h) = (route.width, route.height);
    // Kleiner als das Display, damit Platz zum Verschieben bleibt.
    app.routes.find_mut(route_id).unwrap().device_area = DeviceArea::Part {
        x: 0,
        y: 0,
        width: w / 2,
        height: h / 2,
    };
    app.stage_editor_open = Some(stage.id);

    let mut time = 1.0;
    let mut frame = |app: &mut ShowApp, events: Vec<egui::Event>| {
        time += 0.05;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 1600.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        ctx.run(raw, |ctx| app.draw_workspace(ctx))
    };
    let position = |out: &egui::FullOutput, wanted: &str| {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text.starts_with(wanted) => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()).center())
            }
            _ => None,
        })
    };
    let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };

    let _ = frame(&mut app, vec![]);
    let out = frame(&mut app, vec![]);
    for gone in [
        "Layer",
        "Keep Rendering",
        "Soft Edge",
        "Place stage on this display",
    ] {
        assert!(
            !output_has_text(&out, gone),
            "„{gone}“ gehört nicht mehr zur Stage"
        );
    }
    for wanted in [
        "Resolution",
        "Position",
        "Crop",
        "Edge Blends",
        "Rotation",
        "Mirror",
    ] {
        assert!(output_has_text(&out, wanted), "„{wanted}“ fehlt");
    }

    // Region an ihrer Beschriftung ziehen.
    let label = position(&out, "A  ").expect("Region in der Placement-Ansicht");
    let target = label + egui::vec2(40.0, 20.0);
    let _ = frame(&mut app, vec![egui::Event::PointerMoved(label)]);
    let _ = frame(&mut app, vec![press(label, true)]);
    let _ = frame(
        &mut app,
        vec![egui::Event::PointerMoved(label + egui::vec2(10.0, 5.0))],
    );
    let _ = frame(&mut app, vec![egui::Event::PointerMoved(target)]);
    let _ = frame(&mut app, vec![press(target, false)]);
    let moved = app.routes.find(route_id).unwrap().device_area;
    let DeviceArea::Part { x, y, width, .. } = moved else {
        panic!("Region muss ein Teilbereich bleiben: {moved:?}");
    };
    assert!(
        x > 0 && y > 0,
        "Ziehen muss die Region verschieben: {moved:?}"
    );
    assert_eq!(width, w / 2, "Verschieben ändert die Größe nicht");

    // 1:1: Original-Pixelgröße oben links, auch wenn die Stage nicht zum Display passt.
    let out = frame(&mut app, vec![]);
    assert!(!output_has_text(&out, "Fill Display"));
    let button = position(&out, "1:1").expect("1:1-Button");
    let _ = frame(&mut app, vec![egui::Event::PointerMoved(button)]);
    let _ = frame(&mut app, vec![press(button, true)]);
    let _ = frame(&mut app, vec![press(button, false)]);
    let _ = frame(&mut app, vec![]);
    let route = app.routes.find(route_id).unwrap();
    let display = app
        .displays
        .iter()
        .map(kuhplay::core::DisplayRefInfo::from)
        .find(|d| {
            matches!(&route.device, Some(kuhplay::core::DeviceRef::Display { id, .. }) if *id == d.id)
        })
        .unwrap();
    let size = display.pixel_size();
    assert_eq!(
        route.device_area.rect(size.0, size.1),
        (
            0,
            0,
            stage.geometry.width as u32,
            stage.geometry.height as u32
        )
    );
    assert_eq!(
        (route.width, route.height),
        (stage.geometry.width as u32, stage.geometry.height as u32),
        "Raster 1:1"
    );
}

/// Video-Cue: Die Waveform erscheint, und ein Klick darauf setzt den Startpunkt,
/// ohne das Ende zu verkürzen.
#[test]
fn video_cue_waveform_click_sets_in_and_keeps_the_end() {
    use kuhplay::engine::video_backend::VideoBackend;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mkv");
    let made = std::process::Command::new(VideoBackend::ffmpeg_binary())
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=s=64x64:r=25:d=4")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"])
        .args(["-c:v", "mpeg4", "-c:a", "ac3"])
        .arg(&path)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        return; // ffmpeg fehlt
    }

    let ctx = egui::Context::default();
    icons::install(&ctx);
    let mut app = demo_app();
    let index = app
        .carts
        .active()
        .cues()
        .iter()
        .position(|c| c.cue_type == CueType::Video)
        .expect("Video-Cue");
    app.carts.active_mut().select(Some(index));
    app.carts.active_mut().cues_mut()[index].target_path = Some(path.display().to_string());
    app.ui.inspector_visible = true;
    app.ui.inspector_tab = kuhplay::gui::InspectorTab::TimeLoops;

    let mut time = 1.0;
    let mut frame = |app: &mut ShowApp, events: Vec<egui::Event>| {
        time += 0.05;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        ctx.run(raw, |ctx| app.draw_workspace(ctx))
    };
    // Warten, bis die Berechnung im Hintergrund fertig ist.
    let mut out = frame(&mut app, vec![]);
    for _ in 0..200 {
        if !output_has_text(&out, "reading audio…") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
        out = frame(&mut app, vec![]);
    }
    let _ = frame(&mut app, vec![]);
    let out = frame(&mut app, vec![]);
    assert!(
        !output_has_text(&out, "no waveform"),
        "Video braucht eine Waveform"
    );
    let label = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "Waveform" => Some(t.pos),
            _ => None,
        })
        .expect("Waveform-Zeile");

    let target = label + egui::vec2(500.0, 12.0);
    let press = |pressed: bool| egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = frame(&mut app, vec![egui::Event::PointerMoved(target)]);
    let _ = frame(&mut app, vec![press(true)]);
    let _ = frame(&mut app, vec![press(false)]);
    let _ = frame(&mut app, vec![]);

    let cue = &app.carts.active().cues()[index];
    assert_eq!(cue.slices.len(), 1, "Klick muss den Startpunkt setzen");
    assert!(cue.slices[0].start > Duration::ZERO);
    assert!(
        cue.slices[0].end > Duration::from_secs(3),
        "Das Ende bleibt am Clip-Ende: {:?}",
        cue.slices[0]
    );
}

/// Die Cue-Liste kennt die Dauer eines Videos, bevor es gespielt wurde; bei In/Out
/// zeigt sie die Länge des Abschnitts.
#[test]
fn cue_list_knows_the_duration_before_playing() {
    use kuhplay::engine::video_backend::VideoBackend;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mkv");
    let made = std::process::Command::new(VideoBackend::ffmpeg_binary())
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=s=64x64:r=25:d=4")
        .args(["-c:v", "mpeg4"])
        .arg(&path)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        return; // ffmpeg fehlt
    }
    let mut app = demo_app();
    let index = app
        .carts
        .active()
        .cues()
        .iter()
        .position(|c| c.cue_type == CueType::Video)
        .expect("Video-Cue");
    let cue = &mut app.carts.active_mut().cues_mut()[index];
    cue.duration = Duration::ZERO;
    cue.target_path = Some(path.display().to_string());
    let id = cue.id;

    let start = std::time::Instant::now();
    let listed = loop {
        if let Some(listed) = app.cue_row_meta().get(&id).and_then(|m| m.media_duration) {
            break listed;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "keine Dauer in der Liste"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!((3.5..4.5).contains(&listed.as_secs_f32()), "{listed:?}");

    app.carts.active_mut().cues_mut()[index]
        .slices
        .push(kuhplay::core::MediaSlice::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            1,
        ));
    let listed = app.cue_row_meta().get(&id).and_then(|m| m.media_duration);
    assert_eq!(listed, Some(Duration::from_secs(1)));
}

#[test]
fn live_repaint_delay_taktet_bei_laufenden_cues_eng() {
    // ~60 fps während Cues laufen (flüssige Animationen wie in OBS Studio),
    // im Leerlauf 500 ms.
    assert_eq!(
        kuhplay::gui::app::live_repaint_delay(true),
        Duration::from_millis(16)
    );
    assert_eq!(
        kuhplay::gui::app::live_repaint_delay(false),
        Duration::from_millis(500)
    );
    let mut app = demo_app();
    assert!(!app.any_cue_running(), "Demo-Show startet im Leerlauf");
    app.carts.active_mut().cues_mut()[0].status = CueStatus::Playing;
    assert!(app.any_cue_running());
}
