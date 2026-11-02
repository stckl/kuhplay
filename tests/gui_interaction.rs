//! Headless Interaktionstests für die Cue-Tabelle.
//!
//! Simuliert echte Maus-Events (`PointerMoved` → `PointerButton(pressed)` →
//! `PointerButton(released)` über drei Frames) gegen die Tabelle und prüft:
//!
//! * Klick auf eine Zeile **selektiert** den Cue (Inspector-Anzeige),
//! * Klick in die **Status-Spalte** setzt den Playhead ohne Selektion,
//! * **Doppelklick** öffnet den Inline-Editor (Enter/Klick daneben
//!   committen, Esc bricht ab),
//! * **⌘-Klick** toggelt die Mehrfachselektion, **⇧-Klick** wählt einen
//!   Bereich,
//! * **Rechtsklick** öffnet das Kontextmenü (Aktionen kommen als
//!   `CueTableActions` zurück; die Tabelle ändert selbst nichts),
//! * **Drag & Drop** meldet die Umordnung als Aktion.
//!
//! ## Regression: „Klick auf Cue passiert meist nichts“
//!
//! egui_extras-Tabellenzellen sind default `Sense::hover()` – `Response`'s
//! `clicked`-Feld wird nur für click-sensed Widgets gesetzt, daher feuert
//! `row.response().clicked()` ohne `TableBuilder::sense(Sense::click())`
//! nie. Dieser Test schlägt vor dem Fix fehl.

use std::time::Duration;

use kuhplay::core::{CartManager, ContinueMode, Cue, CueStatus, CueType, GroupMode};
use kuhplay::engine::{EngineCommand, EngineEvent};
use kuhplay::gui::state::{CueColumnLayout, FindState};
use kuhplay::gui::{cue_table, ShowApp};

/// Vier einfache Audio-Cues.
fn demo_list() -> CartManager {
    let mut carts = CartManager::new();
    for i in 1..=4 {
        carts
            .active_mut()
            .add(Cue::new(i.to_string(), format!("Cue {i}"), CueType::Audio));
    }
    carts.select(None);
    carts
}

/// RawInput mit Screen 800×600, expliziter Zeit und Zusatz-Events.
fn raw_input(events: Vec<egui::Event>, time: f64) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    }
}

/// Rendert die Cue-Tabelle allein in einem CentralPanel (800×600).
///
/// Ohne Anzeige-Metadaten (leere Map) – die Klick-Tests brauchen weder
/// Broken-✕ noch Fortschrittsbalken (siehe Phase-2-Tests in gui_smoke.rs).
fn render_table(
    ctx: &egui::Context,
    carts: &mut CartManager,
    editable: bool,
    events: Vec<egui::Event>,
    time: f64,
) {
    kuhplay::gui::icons::install(ctx);
    let _ = ctx.run(raw_input(events, time), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut columns = CueColumnLayout::default();
            let _ = cue_table::draw(
                ui,
                carts.active_mut(),
                editable,
                &std::collections::HashMap::new(),
                &mut columns,
                &FindState::default(),
            );
        });
    });
}

/// Simuliert einen kompletten Klick (Move → Press → Release, 3 Frames).
fn click_button(
    ctx: &egui::Context,
    carts: &mut CartManager,
    pos: egui::Pos2,
    button: egui::PointerButton,
    time: &mut f64,
) {
    *time += 0.05;
    render_table(
        ctx,
        carts,
        true,
        vec![egui::Event::PointerMoved(pos)],
        *time,
    );
    *time += 0.05;
    render_table(
        ctx,
        carts,
        true,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        *time,
    );
    *time += 0.05;
    render_table(
        ctx,
        carts,
        true,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        *time,
    );
}

/// Klick mit der linken Maustaste.
fn click(ctx: &egui::Context, carts: &mut CartManager, pos: egui::Pos2, time: &mut f64) {
    click_button(ctx, carts, pos, egui::PointerButton::Primary, time);
}

/// Sucht per y-Scan die Bildschirm-Y-Position, an der ein Klick in der
/// Name-Spalte (x = 250) die Zeile `target` selektiert (self-calibrating,
/// unabhängig von exakten Panel-Margins).
fn find_row_y(
    ctx: &egui::Context,
    carts: &mut CartManager,
    target: usize,
    time: &mut f64,
) -> Option<f32> {
    for y in (24..220).step_by(2) {
        carts.select(None);
        click(ctx, carts, egui::pos2(250.0, y as f32), time);
        if carts.selected_index() == Some(target) {
            return Some(y as f32);
        }
    }
    None
}

/// RawInput mit gesetzten Modifikatoren (⌘/⇧ – `InputState.modifiers`
/// speist sich aus `RawInput.modifiers`, nicht aus den Events).
fn raw_input_mod(
    events: Vec<egui::Event>,
    time: f64,
    modifiers: egui::Modifiers,
) -> egui::RawInput {
    egui::RawInput {
        modifiers,
        ..raw_input(events, time)
    }
}

/// Rendert die Tabelle (frisches Spalten-Layout) und liefert den Frame.
fn render_table_out(
    ctx: &egui::Context,
    carts: &mut CartManager,
    editable: bool,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    kuhplay::gui::icons::install(ctx);
    let mut columns = CueColumnLayout::default();
    ctx.run(raw_input(events, time), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let _ = cue_table::draw(
                ui,
                carts.active_mut(),
                editable,
                &std::collections::HashMap::new(),
                &mut columns,
                &FindState::default(),
            );
        });
    })
}

/// Rendert die Tabelle mit persistiertem Spalten-Layout und Modifikatoren
/// (für Tests, die Spaltenzustände bzw. ⌘/⇧ über Frames prüfen).
fn render_table_cols(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    editable: bool,
    events: Vec<egui::Event>,
    time: f64,
    modifiers: egui::Modifiers,
) {
    kuhplay::gui::icons::install(ctx);
    let _ = ctx.run(raw_input_mod(events, time, modifiers), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let _ = cue_table::draw(
                ui,
                carts.active_mut(),
                editable,
                &std::collections::HashMap::new(),
                columns,
                &FindState::default(),
            );
        });
    });
}

/// Rendert die Tabelle (editierbar) und liefert die gemeldeten Aktionen.
fn render_actions(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    events: Vec<egui::Event>,
    time: f64,
) -> (egui::FullOutput, cue_table::CueTableActions) {
    kuhplay::gui::icons::install(ctx);
    let mut actions = cue_table::CueTableActions::default();
    let out = ctx.run(raw_input(events, time), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            actions = cue_table::draw(
                ui,
                carts.active_mut(),
                true,
                &std::collections::HashMap::new(),
                columns,
                &FindState::default(),
            );
        });
    });
    (out, actions)
}

/// True, wenn Aktionen gemeldet wurden (mindestens ein Feld gesetzt).
fn actions_nonempty(a: &cue_table::CueTableActions) -> bool {
    !a.delete.is_empty()
        || !a.duplicate.is_empty()
        || !a.group.is_empty()
        || a.arm.is_some()
        || a.color.is_some()
        || a.move_to.is_some()
        || a.move_into_group.is_some()
}

/// Dreifach-Klick (Move → Press → Release) mit Modifikatoren und
/// persistiertem Spalten-Layout.
fn click_button_mod(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    pos: egui::Pos2,
    button: egui::PointerButton,
    modifiers: egui::Modifiers,
    time: &mut f64,
) {
    *time += 0.05;
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![egui::Event::PointerMoved(pos)],
        *time,
        modifiers,
    );
    *time += 0.05;
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers,
        }],
        *time,
        modifiers,
    );
    *time += 0.05;
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers,
        }],
        *time,
        modifiers,
    );
    // Modifikatoren nach dem Klick zurücksetzen (nächster Frame „normal“).
    *time += 0.05;
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![],
        *time,
        egui::Modifiers::NONE,
    );
}

/// ⌘-Klick (Mehrfachselektion toggeln).
fn click_command(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    pos: egui::Pos2,
    time: &mut f64,
) {
    click_button_mod(
        ctx,
        carts,
        columns,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::COMMAND,
        time,
    );
}

/// ⇧-Klick (Bereich selektieren).
fn click_shift(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    pos: egui::Pos2,
    time: &mut f64,
) {
    click_button_mod(
        ctx,
        carts,
        columns,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::SHIFT,
        time,
    );
}

/// Baut ein Key-Event (gedrückt).
fn key_press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Öffnet das Kontextmenü an Position `y` (Rechtsklick) und klickt den
/// Eintrag `item`. Rückgabe: erste nicht-leere Aktionen-Meldung (oder
/// `None`, wenn der Eintrag nur ein Untermenü öffnet).
fn context_menu_click_at(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    y: f32,
    item: &str,
    time: &mut f64,
) -> Option<cue_table::CueTableActions> {
    // Rechtsklick auf die Zeile → Kontextmenü öffnen.
    click_button_mod(
        ctx,
        carts,
        columns,
        egui::pos2(250.0, y),
        egui::PointerButton::Secondary,
        egui::Modifiers::NONE,
        time,
    );
    // Menü rendern und die Position des Eintrags finden (bis zu 3 Versuche).
    let mut menu_pos = None;
    for _ in 0..3 {
        *time += 0.05;
        let (out, _) = render_actions(ctx, carts, columns, vec![], *time);
        if let Some(p) = find_text_center(&out, item) {
            menu_pos = Some(p);
            break;
        }
    }
    let menu_pos =
        menu_pos.unwrap_or_else(|| panic!("Kontextmenü-Eintrag „{item}“ nicht gefunden"));

    // Klick auf den Eintrag (Move → Press → Release, echter Klick).
    click_menu_item(ctx, carts, columns, menu_pos, time)
}

/// Öffnet das Kontextmenü von Zeile `row` (Rechtsklick auf die Name-Spalte,
/// kalibriert per y-Scan) und klickt den Eintrag `item`.
fn context_menu_click(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    row: usize,
    item: &str,
    time: &mut f64,
) -> Option<cue_table::CueTableActions> {
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![],
        *time,
        egui::Modifiers::NONE,
    );
    render_table_cols(
        ctx,
        carts,
        columns,
        true,
        vec![],
        *time,
        egui::Modifiers::NONE,
    );
    let y = find_row_y(ctx, carts, row, time).expect("Zeile muss selektierbar sein");
    context_menu_click_at(ctx, carts, columns, y, item, time)
}

/// Klickt einen (bereits lokalisierten) Menü-Eintrag und liefert die erste
/// nicht-leere Aktionen-Meldung der Klick-Frames.
fn click_menu_item(
    ctx: &egui::Context,
    carts: &mut CartManager,
    columns: &mut CueColumnLayout,
    menu_pos: egui::Pos2,
    time: &mut f64,
) -> Option<cue_table::CueTableActions> {
    let mut actions = None;
    let frames = [
        vec![egui::Event::PointerMoved(menu_pos)],
        vec![egui::Event::PointerButton {
            pos: menu_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: menu_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    for events in frames {
        *time += 0.05;
        let (_, a) = render_actions(ctx, carts, columns, events, *time);
        if actions.is_none() && actions_nonempty(&a) {
            actions = Some(a);
        }
    }
    actions
}

#[test]
fn clicking_rows_selects_cues() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    // Warmlaufen (egui-Sizing-Pass).
    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);

    let y0 = find_row_y(&ctx, &mut carts, 0, &mut time)
        .expect("Klick auf Zeile 0 muss den Cue selektieren (Inspector-Anzeige)");
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time)
        .expect("Klick auf Zeile 1 muss den Cue selektieren");
    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time)
        .expect("Klick auf Zeile 2 muss den Cue selektieren");
    let y3 = find_row_y(&ctx, &mut carts, 3, &mut time)
        .expect("Klick auf Zeile 3 muss den Cue selektieren");
    assert!(
        y0 < y1 && y1 < y2 && y2 < y3,
        "Zeilen-Bänder müssen aufsteigend sein: {y0} < {y1} < {y2} < {y3}"
    );
}

#[test]
fn clicking_status_column_sets_playhead_without_selecting() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);

    // y-Band von Zeile 1 finden (über Selektion in der Name-Spalte).
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");

    // Klick in die Status-Spalte (x ≈ 28): Playhead setzen, NICHT selektieren.
    carts.select(None);
    carts.set_cursor(0);
    click(&ctx, &mut carts, egui::pos2(28.0, y1), &mut time);
    assert_eq!(
        carts.cursor(),
        1,
        "Klick in der Status-Spalte muss den Playhead setzen"
    );
    assert_eq!(
        carts.selected_index(),
        None,
        "Klick in der Status-Spalte darf nicht selektieren"
    );
}

/// Doppelklick auf die Name-Zelle öffnet den Inline-Editor; Tippen hängt
/// am Textende (Cursor-at-End), Enter committen.
#[test]
fn double_click_opens_inline_editor_and_enter_commits() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);

    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");

    // egui zählt einen Release als Doppelklick (count == 2), wenn er < 0,3 s
    // nach dem letzten und >= 0,6 s nach dem vorletzten Release liegt.
    // Der y-Scan hinterlässt Klick-Releases im 0,15-s-Takt (→ Triple-Clicks),
    // daher zuerst „beruhigen": 0,5 s verstreichen lassen.
    time += 0.5;
    render_table(&ctx, &mut carts, true, vec![], time);

    // Doppelklick: zwei Klicks im Abstand von 0,15 s (innerhalb des
    // Double-Click-Zeitfensters von 0,3 s).
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);

    // Tippen (Editor hat Fokus, Cursor am Textende) + Enter = Commit.
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("X".to_string())],
        time,
    );
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![key_press(egui::Key::Enter, egui::Modifiers::NONE)],
        time,
    );

    assert_eq!(
        carts.active().cues()[1].name,
        "Cue 2X",
        "Doppelklick öffnet den Inline-Editor; Enter committen den neuen Namen"
    );
}

/// Esc verwirft die Änderung und schließt den Editor.
#[test]
fn escape_cancels_inline_editor() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");

    time += 0.5;
    render_table(&ctx, &mut carts, true, vec![], time);
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);

    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("X".to_string())],
        time,
    );
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![key_press(egui::Key::Escape, egui::Modifiers::NONE)],
        time,
    );

    assert_eq!(
        carts.active().cues()[1].name,
        "Cue 2",
        "Esc verwirft die Änderung"
    );
    // Editor ist zu: weiteres Tippen landet nicht mehr im Cue.
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("Y".to_string())],
        time,
    );
    assert_eq!(
        carts.active().cues()[1].name,
        "Cue 2",
        "Editor nach Esc geschlossen (weiteres Tippen wirkungslos)"
    );
}

/// Klick auf eine andere Zeile: Der Editor verliert den Fokus und wird
/// übernommen; der Klick selektiert zugleich die Zeile.
#[test]
fn click_elsewhere_commits_inline_editor() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");
    let y3 = find_row_y(&ctx, &mut carts, 3, &mut time).expect("Zeile 3 muss selektierbar sein");

    time += 0.5;
    render_table(&ctx, &mut carts, true, vec![], time);
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);
    click(&ctx, &mut carts, egui::pos2(250.0, y1), &mut time);
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("X".to_string())],
        time,
    );

    // Klick auf Zeile 3: Editor verliert Fokus → Commit; Zeile 3 selektiert.
    click(&ctx, &mut carts, egui::pos2(250.0, y3), &mut time);

    assert_eq!(
        carts.active().cues()[1].name,
        "Cue 2X",
        "Fokusverlust (Klick daneben) committen den Editor"
    );
    assert_eq!(
        carts.selected_index(),
        Some(3),
        "Der Klick selektiert zugleich die andere Zeile"
    );
}

/// Doppelklick auf die Duration-Zelle: Timecode-Edit („5“ = 5 s,
/// akzeptiert nackte Sekunden). Die Spalte wird über den Header-Text
/// kalibriert.
#[test]
fn double_click_edits_duration_inline_as_timecode() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    let out = render_table_out(&ctx, &mut carts, true, vec![], time);
    let x_dur = find_text_center(&out, "Duration")
        .expect("Header „Duration“ muss gerendert sein")
        .x;
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");

    time += 0.5;
    render_table(&ctx, &mut carts, true, vec![], time);
    click(&ctx, &mut carts, egui::pos2(x_dur, y1), &mut time);
    click(&ctx, &mut carts, egui::pos2(x_dur, y1), &mut time);

    // Alten Timecode löschen (Cursor steht am Ende; 32× Backspace reicht
    // für jeden realistischen Timecode) und „5“ tippen.
    for _ in 0..32 {
        render_table(
            &ctx,
            &mut carts,
            true,
            vec![key_press(egui::Key::Backspace, egui::Modifiers::NONE)],
            time,
        );
    }
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("5".to_string())],
        time,
    );
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![key_press(egui::Key::Enter, egui::Modifiers::NONE)],
        time,
    );

    assert_eq!(
        carts.active().cues()[1].duration,
        Duration::from_secs(5),
        "Duration-Editor parst „5“ als 5 Sekunden (Timecode)"
    );
}

/// Ungültiger Timecode („abc“) wird verworfen – der alte Wert bleibt.
#[test]
fn invalid_timecode_keeps_old_duration() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    let out = render_table_out(&ctx, &mut carts, true, vec![], time);
    let x_dur = find_text_center(&out, "Duration")
        .expect("Header „Duration“ muss gerendert sein")
        .x;
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 muss selektierbar sein");
    let original = carts.active().cues()[1].duration;

    time += 0.5;
    render_table(&ctx, &mut carts, true, vec![], time);
    click(&ctx, &mut carts, egui::pos2(x_dur, y1), &mut time);
    click(&ctx, &mut carts, egui::pos2(x_dur, y1), &mut time);

    for _ in 0..32 {
        render_table(
            &ctx,
            &mut carts,
            true,
            vec![key_press(egui::Key::Backspace, egui::Modifiers::NONE)],
            time,
        );
    }
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![egui::Event::Text("abc".to_string())],
        time,
    );
    render_table(
        &ctx,
        &mut carts,
        true,
        vec![key_press(egui::Key::Enter, egui::Modifiers::NONE)],
        time,
    );

    assert_eq!(
        carts.active().cues()[1].duration,
        original,
        "Ungültiger Timecode wird verworfen (alter Wert bleibt)"
    );
}

#[test]
fn right_click_opens_context_menu_without_panic() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;

    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);

    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time).expect("Zeile 2 muss selektierbar sein");

    // Rechtsklick (Sekundärbutton): Kontextmenü öffnet sich.
    carts.select(None);
    click_button(
        &ctx,
        &mut carts,
        egui::pos2(250.0, y2),
        egui::PointerButton::Secondary,
        &mut time,
    );
    // Menü rendert in den Folgeframes fehlerfrei (Menü-Inhalt wird gezeichnet).
    render_table(&ctx, &mut carts, true, vec![], time);
    render_table(&ctx, &mut carts, true, vec![], time);

    assert_eq!(
        carts.selected_index(),
        None,
        "Rechtsklick darf nicht selektieren"
    );
}

/// ⌘-Klick baut eine Mehrfachselektion auf und toggelt sie wieder ab.
#[test]
fn command_click_toggles_multi_selection() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let y0 = find_row_y(&ctx, &mut carts, 0, &mut time).expect("Zeile 0 selektierbar");
    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time).expect("Zeile 2 selektierbar");
    let id0 = carts.active().cues()[0].id;
    let id2 = carts.active().cues()[2].id;

    // Einfacher Klick: nur Zeile 0 selektiert.
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y0),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        &mut time,
    );
    assert_eq!(
        carts.active().selection_len(),
        1,
        "Einfachklick: 1 selektiert"
    );

    // ⌘-Klick auf Zeile 2: Mehrfachselektion.
    click_command(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y2),
        &mut time,
    );
    assert_eq!(
        carts.active().selection_len(),
        2,
        "⌘-Klick erweitert die Selektion"
    );
    assert!(
        carts.active().is_selected(id0) && carts.active().is_selected(id2),
        "beide Zeilen selektiert"
    );

    // Erneuter ⌘-Klick auf Zeile 0: toggelt Zeile 0 wieder aus.
    click_command(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y0),
        &mut time,
    );
    assert_eq!(
        carts.active().selection_len(),
        1,
        "erneuter ⌘-Klick toggelt die Zeile ab"
    );
    assert!(!carts.active().is_selected(id0), "Zeile 0 abgewählt");
    assert!(carts.active().is_selected(id2), "Zeile 2 bleibt selektiert");
}

/// ⇧-Klick selektiert den Bereich vom Anker bis zur geklickten Zeile.
#[test]
fn shift_click_selects_range() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let y1 = find_row_y(&ctx, &mut carts, 1, &mut time).expect("Zeile 1 selektierbar");
    let y3 = find_row_y(&ctx, &mut carts, 3, &mut time).expect("Zeile 3 selektierbar");
    let id0 = carts.active().cues()[0].id;

    // Einfacher Klick auf Zeile 1 (setzt den Anker), dann ⇧-Klick auf
    // Zeile 3 → Bereich 1..=3 ersetzt die Selektion.
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y1),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        &mut time,
    );
    click_shift(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y3),
        &mut time,
    );

    assert_eq!(
        carts.active().selection_len(),
        3,
        "⇧-Klick selektiert den Bereich 1..=3"
    );
    assert!(
        !carts.active().is_selected(id0),
        "Zeile 0 ist nicht Teil des Bereichs"
    );
    for i in 1..=3 {
        assert!(
            carts.active().is_selected(carts.active().cues()[i].id),
            "Zeile {i} ist selektiert"
        );
    }
}

/// Kontextmenü „Duplicate Cue“ meldet die Duplizierung per ID.
#[test]
fn context_menu_duplicate_requests_duplicate() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    let id1 = carts.active().cues()[1].id;
    let actions = context_menu_click(
        &ctx,
        &mut carts,
        &mut columns,
        1,
        "Duplicate Cue",
        &mut time,
    )
    .expect("Duplicate meldet Aktionen");

    assert_eq!(actions.duplicate, vec![id1], "Duplizieren per ID gemeldet");
    assert_eq!(carts.active().len(), 4, "draw dupliziert nicht selbst");
}

/// Kontextmenü „Group Cue“ meldet die Gruppierung per ID.
#[test]
fn context_menu_group_requests_group() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    let id2 = carts.active().cues()[2].id;
    let actions = context_menu_click(&ctx, &mut carts, &mut columns, 2, "Group Cue", &mut time)
        .expect("Group meldet Aktionen");

    assert_eq!(actions.group, vec![id2], "Gruppieren per ID gemeldet");
    assert_eq!(carts.active().len(), 4, "draw gruppiert nicht selbst");
}

/// Kontextmenü „Disarm Cue“ (Cues sind default armiert) meldet
/// `arm = Some((ids, false))`.
#[test]
fn context_menu_disarm_requests_arm_false() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    let id0 = carts.active().cues()[0].id;
    assert!(carts.active().cues()[0].is_armed, "Cues sind default armed");
    let actions = context_menu_click(&ctx, &mut carts, &mut columns, 0, "Disarm Cue", &mut time)
        .expect("Disarm meldet Aktionen");

    assert_eq!(
        actions.arm,
        Some((vec![id0], false)),
        "Disarm meldet (ids, false)"
    );
    assert!(
        carts.active().cues()[0].is_armed,
        "draw ändert armed nicht selbst"
    );
}

/// Kontextmenü „Set Color ▸ No Color“ meldet das Löschen der Farbe.
#[test]
fn context_menu_set_color_no_color_requests_color_none() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    let id3 = carts.active().cues()[3].id;
    // Erst das Untermenü „Set Color“ öffnen …
    if let Some(a) = context_menu_click(&ctx, &mut carts, &mut columns, 3, "Set Color", &mut time) {
        assert!(
            !actions_nonempty(&a),
            "„Set Color“ öffnet nur das Untermenü"
        );
    }
    // … dann „No Color“ klicken.
    let mut menu_pos = None;
    for _ in 0..3 {
        time += 0.05;
        let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
        if let Some(p) = find_text_center(&out, "No Color") {
            menu_pos = Some(p);
            break;
        }
    }
    let menu_pos = menu_pos.expect("Untermenü zeigt „No Color“");
    let actions = click_menu_item(&ctx, &mut carts, &mut columns, menu_pos, &mut time)
        .expect("No Color meldet Aktionen");
    assert_eq!(
        actions.color,
        Some((vec![id3], None)),
        "No Color meldet (ids, None)"
    );
}

/// Kontextmenü auf einer Zeile innerhalb einer Mehrfachselektion wirkt auf
/// die GANZE Selektion und zeigt den Plural („Disarm Cues“).
#[test]
fn context_menu_applies_to_effective_multi_selection() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let y0 = find_row_y(&ctx, &mut carts, 0, &mut time).expect("Zeile 0 selektierbar");
    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time).expect("Zeile 2 selektierbar");
    let id0 = carts.active().cues()[0].id;
    let id2 = carts.active().cues()[2].id;

    // Selektion {0, 2} aufbauen, dann Rechtsklick auf Zeile 2 (ohne
    // erneuten Selektions-Klick – die Selektion darf nicht zurückgesetzt
    // werden).
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y0),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        &mut time,
    );
    click_command(
        &ctx,
        &mut carts,
        &mut columns,
        egui::pos2(250.0, y2),
        &mut time,
    );
    assert_eq!(carts.active().selection_len(), 2);

    let actions =
        context_menu_click_at(&ctx, &mut carts, &mut columns, y2, "Disarm Cues", &mut time)
            .expect("Disarm Cues meldet Aktionen (Plural bei Mehrfachselektion)");

    assert_eq!(
        actions.arm,
        Some((vec![id0, id2], false)),
        "Aktion gilt für die effektive Mehrfachselektion"
    );
}

/// Header-Rechtsklick → Spalten-Menü: „Target“ ausblenden versteckt die
/// Spalte (Spalten konfigurierbar).
#[test]
fn header_context_menu_hides_column() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    assert!(
        find_text_center(&out, "Target").is_some(),
        "Header „Target“ ist sichtbar"
    );

    // Rechtsklick auf den Header (y-Scan: Menü mit „Reset Column Widths“).
    let mut header_y = None;
    for y in (8..26).step_by(2) {
        click_button_mod(
            &ctx,
            &mut carts,
            &mut columns,
            egui::pos2(250.0, y as f32),
            egui::PointerButton::Secondary,
            egui::Modifiers::NONE,
            &mut time,
        );
        time += 0.05;
        let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
        if find_text_center(&out, "Reset Column Widths").is_some() {
            header_y = Some(y as f32);
            break;
        }
    }
    let _ = header_y.expect("Header-Kontextmenü muss sich öffnen lassen");

    // „Target“-Checkbox im Menü klicken (letzter Treffer: das Menü liegt
    // über dem Header, der denselben Text zeigt).
    let mut menu_pos = None;
    for _ in 0..3 {
        time += 0.05;
        let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
        if let Some(p) = find_text_center_last(&out, "Target") {
            menu_pos = Some(p);
            break;
        }
    }
    let menu_pos = menu_pos.expect("Spalten-Menü zeigt „Target“");
    click_menu_item(&ctx, &mut carts, &mut columns, menu_pos, &mut time);

    assert!(
        columns.hidden[4],
        "Spalten-Layout merkt sich: Target (Index 4) ausgeblendet"
    );
    // Menü schließen (Checkboxen schließen es nicht von selbst) – sonst
    // rendert das offene Menü den „Target“-Eintrag weiter mit.
    render_actions(
        &ctx,
        &mut carts,
        &mut columns,
        vec![key_press(egui::Key::Escape, egui::Modifiers::NONE)],
        time,
    );
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    assert!(
        find_text_center(&out, "Target").is_none(),
        "Header „Target“ ist nach dem Ausblenden nicht mehr gerendert"
    );
}

/// Aufklapp-Dreieck einer Gruppe klappt die Kinder aus/ein.
#[test]
fn triangle_click_collapses_and_expands_group() {
    let ctx = egui::Context::default();
    let mut carts = CartManager::new();
    {
        let list = carts.active_mut();
        let mut group = Cue::new_group(
            "1".to_string(),
            "Meine Gruppe".to_string(),
            GroupMode::Simultaneous,
            vec![Cue::new(
                "1.1".to_string(),
                "Kind-Cue".to_string(),
                CueType::Audio,
            )],
        );
        group.collapsed = false;
        list.add(group);
        list.add(Cue::new(
            "2".to_string(),
            "Nach der Gruppe".to_string(),
            CueType::Audio,
        ));
    }
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    assert!(
        find_text_center(&out, "Kind-Cue").is_some(),
        "aufgeklappt: Kind-Cue sichtbar"
    );
    // Dreieck-Position über das Caret-Down-Glyph finden (einzige Gruppe;
    // aufgeklappt zeigt das Dreieck nach unten). `find_text_center_last`:
    // Der Playhead der Cursor-Zeile nutzt dasselbe Caret-Right-Glyph wie
    // das eingeklappte Dreieck (beides ein Rechtspfeil) und wird in
    // der Status-Spalte zuerst gemalt – der letzte Treffer ist das Dreieck
    // in der Type-Spalte.
    let caret = find_text_center_last(&out, "\u{E136}").expect("Aufklapp-Dreieck gerendert");

    // Klick auf das Dreieck → einklappen.
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        caret,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        &mut time,
    );
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    assert!(
        find_text_center(&out, "Kind-Cue").is_none(),
        "eingeklappt: Kind-Cue versteckt"
    );
    assert!(
        find_text_center(&out, "\u{E13A}").is_some(),
        "eingeklappt: Dreieck zeigt nach rechts"
    );
    assert!(
        carts.active().cues()[0].collapsed,
        "collapsed-Flag gesetzt (Group only)"
    );

    // Erneuter Klick auf das Dreieck → wieder aufklappen. Hier kollidiert
    // das Glyph mit dem Playhead der Cursor-Zeile (siehe oben) – der letzte
    // Treffer ist das Dreieck in der Type-Spalte.
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    let caret = find_text_center_last(&out, "\u{E13A}").expect("Dreieck gerendert (eingeklappt)");
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        caret,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        &mut time,
    );
    let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
    assert!(
        find_text_center(&out, "Kind-Cue").is_some(),
        "wieder aufgeklappt: Kind-Cue sichtbar"
    );
    assert!(
        !carts.active().cues()[0].collapsed,
        "collapsed-Flag zurückgesetzt"
    );
}

/// Drag & Drop: Zeile 0 auf Zeile 2 ziehen meldet `move_to = (0, 2)`
/// (die Tabelle sortiert nicht selbst um).
#[test]
fn drag_and_drop_reorder_requests_move() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;

    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    render_table_cols(
        &ctx,
        &mut carts,
        &mut columns,
        true,
        vec![],
        time,
        egui::Modifiers::NONE,
    );
    let y0 = find_row_y(&ctx, &mut carts, 0, &mut time).expect("Zeile 0 selektierbar");
    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time).expect("Zeile 2 selektierbar");

    // Press auf Zeile 0, dann (gedrückt) zur Zeile 2 bewegen, loslassen.
    time += 0.05;
    render_actions(
        &ctx,
        &mut carts,
        &mut columns,
        vec![egui::Event::PointerMoved(egui::pos2(250.0, y0))],
        time,
    );
    time += 0.05;
    render_actions(
        &ctx,
        &mut carts,
        &mut columns,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(250.0, y0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        time,
    );
    // Drag beginnen (Bewegung über die Schwelle) und zur Zeile 2 ziehen.
    time += 0.05;
    render_actions(
        &ctx,
        &mut carts,
        &mut columns,
        vec![egui::Event::PointerMoved(egui::pos2(250.0, y2))],
        time,
    );
    time += 0.05;
    let (_, actions) = render_actions(
        &ctx,
        &mut carts,
        &mut columns,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(250.0, y2),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        time,
    );

    assert_eq!(
        actions.move_to,
        Some((0, 2)),
        "Drag & Drop meldet die Umordnung (von 0 auf 2)"
    );
    assert_eq!(
        carts.active().cues()[0].number,
        "1",
        "draw sortiert nicht selbst um (Reihenfolge unverändert)"
    );
}

#[test]
fn cue_color_can_be_set_and_cleared() {
    use kuhplay::core::CUE_COLORS;
    use kuhplay::engine::spawn_mock_engine;
    use kuhplay::gui::ShowApp;

    let mut app = ShowApp::new(spawn_mock_engine());
    app.carts_mut().select(Some(0));

    // Farbe setzen („Cue Color").
    app.set_selected_color(Some(CUE_COLORS[0]));
    assert_eq!(app.carts().active().cues()[0].color, Some(CUE_COLORS[0]));
    // Andere Farbe wählen.
    app.set_selected_color(Some(CUE_COLORS[4]));
    assert_eq!(app.carts().active().cues()[0].color, Some(CUE_COLORS[4]));
    // Farbe entfernen.
    app.set_selected_color(None);
    assert_eq!(app.carts().active().cues()[0].color, None);
}

#[test]
fn waveform_preview_fuer_audio_cue() {
    use kuhplay::engine::spawn_mock_engine;
    use kuhplay::engine::waveform::WAVEFORM_BUCKETS;
    use kuhplay::gui::ShowApp;

    // Sinus-WAV schreiben.
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

    let mut app = ShowApp::new(spawn_mock_engine());
    let w = app.waveform_for(&path).expect("dekodierbar");
    assert_eq!(w.bucket_count(), WAVEFORM_BUCKETS);
    assert!(w.peak() > 0.7);
    // Cache: zweite Abfrage liefert dieselbe Instanz.
    let w2 = app.waveform_for(&path).unwrap();
    assert!(std::sync::Arc::ptr_eq(&w, &w2));
    // Nicht dekodierbar → None (und gecacht, kein erneuter Versuch).
    let missing = dir.path().join("missing.wav");
    assert!(app.waveform_for(&missing).is_none());
    assert!(app.waveform_for(&missing).is_none());
}

#[test]
fn audition_mode_strips_stage_from_video_commands() {
    use kuhplay::core::{HtmlSource, LoopMode, SlideShow};
    use kuhplay::engine::{
        new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
        EngineCommand, EngineHandle,
    };
    use kuhplay::gui::ShowApp;
    use std::time::Duration;
    use uuid::Uuid;

    // EngineHandle mit eigenen Kanälen – der Test liest die Commands selbst.
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (_evt_tx, evt_rx) = crossbeam_channel::unbounded();
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
    let mut app = ShowApp::new(handle);
    // Startup-Commands (ConfigureStage aller Stages) verwerfen.
    while cmd_rx.try_recv().is_ok() {}

    let play = EngineCommand::Play {
        cue_id: Uuid::new_v4(),
        path: Some("/tmp/video.mp4".to_string()),
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: vec![],
        route: None,
        timecode_in: None,
        stage: Some("Stage 1".to_string()),
        layer: 1,
        surface: None,
    };

    // Audition AUS → Stage bleibt erhalten.
    app.set_audition_mode(false);
    app.dispatch(vec![play.clone()]);
    match cmd_rx.try_recv().unwrap() {
        EngineCommand::Play { stage, .. } => assert_eq!(stage.as_deref(), Some("Stage 1")),
        other => panic!("erwartet Play, got {other:?}"),
    }

    // Audition AN → Stage wird entfernt, Rest des Commands unverändert.
    app.set_audition_mode(true);
    app.dispatch(vec![play]);
    match cmd_rx.try_recv().unwrap() {
        EngineCommand::Play {
            stage, path, layer, ..
        } => {
            assert_eq!(stage, None);
            assert_eq!(path.as_deref(), Some("/tmp/video.mp4"));
            assert_eq!(layer, 1);
        }
        other => panic!("erwartet Play, got {other:?}"),
    }

    // PlayHtml: Stage wird entfernt.
    let html = EngineCommand::PlayHtml {
        cue_id: Uuid::new_v4(),
        source: HtmlSource::Inline("<h1>hi</h1>".to_string()),
        stage: Some("Stage 1".to_string()),
        layer: 2,
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
    };
    app.dispatch(vec![html]);
    match cmd_rx.try_recv().unwrap() {
        EngineCommand::PlayHtml { stage, .. } => assert_eq!(stage, None),
        other => panic!("erwartet PlayHtml, got {other:?}"),
    }

    // PlaySlideShow: Stage wird entfernt.
    let show = EngineCommand::PlaySlideShow {
        cue_id: Uuid::new_v4(),
        show: SlideShow {
            slides: vec![],
            crossfade: Duration::ZERO,
            auto_advance: true,
            loop_slides: false,
            transition: Default::default(),
            default_hold: std::time::Duration::from_secs(3),
        },
        stage: Some("Stage 1".to_string()),
        layer: 3,
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        surface: None,
    };
    app.dispatch(vec![show]);
    match cmd_rx.try_recv().unwrap() {
        EngineCommand::PlaySlideShow { stage, .. } => assert_eq!(stage, None),
        other => panic!("erwartet PlaySlideShow, got {other:?}"),
    }

    // Andere Commands (z. B. StopAll) bleiben unverändert.
    app.dispatch(vec![EngineCommand::StopAll]);
    assert!(matches!(cmd_rx.try_recv().unwrap(), EngineCommand::StopAll));
}

// ------------------------------------------------------------------
// Phase-0-Tests (TODO.md Items 1–6 + 8): Cart-Events über alle Carts,
// Hotkey-Gate (Textfeld vs. Slider), Esc während Texteingabe, ⌘↑/⌘↓,
// Fehleranzeige im Footer, eindeutige Q-Nummern, Delete stoppt Cue.
// ------------------------------------------------------------------

/// App mit eigener EngineHandle (der Test liest Commands/Events selbst).
fn hotkey_app() -> (
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

/// Key-Event (gedrückt) mit optionalem ⌘-Modifier.
/// Key-Event mit ⌥⌘.
fn alt_cmd_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            alt: true,
            ..Default::default()
        },
    }
}

/// Key-Event mit ⇧⌘.
fn shift_cmd_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        },
    }
}

fn key_event(key: egui::Key, command: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command,
            ..Default::default()
        },
    }
}

/// Führt `handle_hotkeys` in einem egui-Frame aus (mit Zusatz-Events).
fn run_hotkeys(app: &mut ShowApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) {
    // egui liest `modifiers` aus RawInput (nicht aus dem Key-Event) – daher
    // spiegeln wir die Modifier des Key-Events ins RawInput.
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let mut raw = raw_input(events, time);
    raw.modifiers = modifiers;
    let _ = ctx.run(raw, |ctx| {
        app.handle_hotkeys(ctx);
    });
}

/// Rendert den Masthead UND führt dabei die Hotkeys aus (Reihenfolge wie in
/// `ShowApp::update`: Hotkeys vor den Panels – so prüft der Test, ob ein
/// fokussierter Button das Space-Event zusätzlich konsumieren würde).
fn render_masthead_with_hotkeys(
    app: &mut ShowApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    time: f64,
) {
    let _ = ctx.run(raw_input(events, time), |ctx| {
        app.handle_hotkeys(ctx);
        egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
            kuhplay::gui::masthead::draw(ui, app);
        });
    });
}

/// Sucht in den gezeichneten Shapes die Mitte des Textes `text`.
fn find_text_center(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            if t.galley.job.text == text {
                return Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()).center());
            }
        }
    }
    None
}

/// Wie [`find_text_center`], liefert aber den LETZTEN Treffer (Menüs werden
/// nach dem Haupt-Layer gezeichnet – wichtig, wenn derselbe Text auch im
/// Header vorkommt, z. B. „Target“).
fn find_text_center_last(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    let mut found = None;
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            if t.galley.job.text == text {
                found = Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()).center());
            }
        }
    }
    found
}

/// Simuliert einen Klick (Move → Press → Release) über eine Render-Closure.
fn click_via<R>(
    ctx: &egui::Context,
    pos: egui::Pos2,
    time: &mut f64,
    mut render: impl FnMut(&egui::Context, Vec<egui::Event>, f64) -> R,
) {
    *time += 0.05;
    render(ctx, vec![egui::Event::PointerMoved(pos)], *time);
    *time += 0.05;
    render(
        ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        *time,
    );
    *time += 0.05;
    render(
        ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        *time,
    );
}

#[test]
fn engine_events_update_cues_in_inactive_carts() {
    let (mut app, cmd_rx, evt_tx) = hotkey_app();

    // Cart 2 anlegen (wird aktiv) + zwei Cues dort; „A" mit Auto-Continue.
    app.carts_mut().add_cart("Cart 2");
    let (id_a, id_b) = {
        let list = app.carts_mut().active_mut();
        list.add(Cue::new("A", "A", CueType::Audio));
        list.cues_mut()[0].continue_mode = ContinueMode::AutoContinue;
        list.add(Cue::new("B", "B", CueType::Audio));
        (list.cues()[0].id, list.cues()[1].id)
    };

    // „A" starten (Cart 2 aktiv), dann auf Cart 1 wechseln.
    app.go_now();
    assert_eq!(app.carts().active().cues()[0].status, CueStatus::Playing);
    app.carts_mut().set_active(0).unwrap();

    // Engine-Event: Finished für „A" → Status landet in Cart 2 (nicht-aktiv),
    // Auto-Continue feuert „B" in derselben Liste (Cart 2) – Semantik
    // (Auto-Continue erst nach Abschluss, Auto-Follow schon beim Start).
    evt_tx.send(EngineEvent::Finished { cue_id: id_a }).unwrap();
    app.poll_engine_events();

    assert_eq!(
        app.carts().find_cue_any(id_a).unwrap().status,
        CueStatus::Completed,
        "Engine-Event erreicht den Cue im nicht-aktiven Cart"
    );
    assert_eq!(
        app.carts().find_cue_any(id_b).unwrap().status,
        CueStatus::Playing,
        "Auto-Continue feuert in der Liste des Cues (Cart 2)"
    );
    assert!(
        cmd_rx
            .try_iter()
            .any(|c| matches!(c, EngineCommand::Play { cue_id, .. } if cue_id == id_b)),
        "Play-Command für den Folge-Cue wurde dispatched"
    );
    // Cart 1 unangetastt.
    assert_eq!(app.carts().active().cues()[0].status, CueStatus::Standby);
}

#[test]
fn escape_panics_when_no_text_field_focused() {
    let (mut app, cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();

    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Escape, false)],
        1.0,
    );
    assert!(
        matches!(cmd_rx.try_recv(), Ok(EngineCommand::StopAll)),
        "Esc = Panic (StopAll an die Engine)"
    );
}

#[test]
fn escape_does_not_panic_while_editing_text() {
    let (mut app, cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    kuhplay::gui::icons::install(&ctx);
    let mut text = String::new();
    let mut time = 1.0;

    // Textfeld rendern (Warmup) + für das Hotkey-Gate registrieren.
    let render_edit = |ctx: &egui::Context,
                       app: &mut ShowApp,
                       text: &mut String,
                       events: Vec<egui::Event>,
                       time: f64|
     -> egui::FullOutput {
        ctx.run(raw_input(events, time), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(text).hint_text("edit me"));
                app.text_field_ids.insert(r.id);
            });
        })
    };
    render_edit(&ctx, &mut app, &mut text, vec![], time);
    render_edit(&ctx, &mut app, &mut text, vec![], time);
    let out = render_edit(&ctx, &mut app, &mut text, vec![], time);
    let pos = find_text_center(&out, "edit me").expect("Hinweis-Text sichtbar");

    // Klick ins Textfeld → es bekommt den Fokus (wie ein echter Nutzer).
    click_via(&ctx, pos, &mut time, |ctx, events, time| {
        render_edit(ctx, &mut app, &mut text, events, time);
    });
    assert!(
        ctx.wants_keyboard_input(),
        "Test-Setup: Textfeld muss Fokus haben"
    );

    // Registry-Swap + Gate-Refresh (wie update()): Hotkeys erkennen das
    // fokussierte Textfeld (Cache – Esc löscht den Fokus in egui's begin_pass).
    app.swap_text_field_registry();
    app.refresh_text_field_gate(&ctx);
    assert!(
        app.text_field_focused(&ctx),
        "Textfeld als fokussiert erkannt"
    );

    // Esc während der Texteingabe → kein Panic (Esc räumt nur den Fokus auf).
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Escape, false)],
        time,
    );
    assert!(
        cmd_rx.try_recv().is_err(),
        "Esc im Textfeld darf nicht panicen"
    );

    // Space während der Texteingabe → tippt ins Feld, kein GO.
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Space, false)],
        time,
    );
    app.flush_pending_go();
    assert_eq!(
        app.carts().cursor(),
        0,
        "Space im Textfeld darf kein GO auslösen"
    );
}

#[test]
fn space_triggers_go_exactly_once() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    assert_eq!(app.carts().cursor(), 0);

    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Space, false)],
        1.0,
    );
    app.flush_pending_go();
    assert_eq!(
        app.carts().cursor(),
        1,
        "Space = GO (Playhead rückt um genau einen Cue vor)"
    );
    assert!(app.carts().running_count() >= 1);

    // Erneutes Space → genau ein weiteres GO (kein Doppel-Trigger).
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Space, false)],
        1.1,
    );
    app.flush_pending_go();
    assert_eq!(
        app.carts().cursor(),
        2,
        "zweites Space = genau ein weiteres GO"
    );
}

#[test]
fn space_go_fires_with_focused_slider() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    let time = 1.0;
    let mut value = 0.5f64;

    // DragValue rendern (Warmup) + Widget-Id merken.
    let render_slider = |ctx: &egui::Context,
                         value: &mut f64,
                         events: Vec<egui::Event>,
                         time: f64|
     -> (egui::FullOutput, egui::Id) {
        let mut id = None;
        let out = ctx.run(raw_input(events, time), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                id = Some(ui.add(egui::DragValue::new(value)).id);
            });
        });
        (out, id.unwrap())
    };
    render_slider(&ctx, &mut value, vec![], time);
    let (_, slider_id) = render_slider(&ctx, &mut value, vec![], time);

    // Fokus auf den Slider setzen (wie ein Klick es täte – DragValue fordert
    // Fokus beim Klick an, genau wie Textfelder).
    ctx.memory_mut(|m| m.request_focus(slider_id));
    assert!(
        ctx.wants_keyboard_input(),
        "Test-Setup: Slider muss Fokus haben"
    );

    // Registry-Swap + Gate-Refresh: keine Textfelder registriert → Slider
    // zählt nicht als Textfeld.
    app.swap_text_field_registry();
    app.refresh_text_field_gate(&ctx);
    assert!(
        !app.text_field_focused(&ctx),
        "ein fokussierter Slider ist kein Textfeld"
    );

    // Space → GO (ein fokussierter Slider darf nicht blockieren).
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Space, false)],
        time,
    );
    app.flush_pending_go();
    assert_eq!(
        app.carts().cursor(),
        1,
        "Space löst GO aus, auch wenn ein Slider fokussiert ist"
    );
}

#[test]
fn space_go_after_clicking_go_button_fires_exactly_once() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    kuhplay::gui::icons::install(&ctx);
    let mut time = 1.0;

    // Masthead rendern (Warmup ×2) + „GO"-Button finden.
    let render_masthead = |ctx: &egui::Context,
                           app: &mut ShowApp,
                           events: Vec<egui::Event>,
                           time: f64|
     -> egui::FullOutput {
        ctx.run(raw_input(events, time), |ctx| {
            egui::TopBottomPanel::top("masthead").show(ctx, |ui| {
                kuhplay::gui::masthead::draw(ui, app);
            });
        })
    };
    render_masthead(&ctx, &mut app, vec![], time);
    render_masthead(&ctx, &mut app, vec![], time);
    let out = render_masthead(&ctx, &mut app, vec![], time);
    let go_pos = find_text_center(&out, "GO").expect("GO-Button sichtbar");

    // Klick auf GO → ein GO feuert.
    click_via(&ctx, go_pos, &mut time, |ctx, events, time| {
        render_masthead(ctx, &mut app, events, time);
    });
    assert_eq!(
        app.carts().cursor(),
        1,
        "Klick auf GO startet den ersten Cue"
    );

    // Space – im selben Frame wie das Masthead-Render (ein fokussierter Button
    // darf nicht zusätzlich feuern, wenn das globale GO gemeint ist).
    app.swap_text_field_registry(); // Notes-Feld des Mastheads registrieren
    app.refresh_text_field_gate(&ctx); // GO-Klick hat keinen Fokus gesetzt
    let before = app.carts().cursor();
    render_masthead_with_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Space, false)],
        time,
    );
    app.flush_pending_go();
    assert_eq!(
        app.carts().cursor(),
        before + 1,
        "Space = genau ein weiteres GO (kein Doppel-Trigger)"
    );
}

#[test]
fn cmd_arrows_move_selected_cue_plain_arrows_move_playhead() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();

    // Cue 1 selektieren (Nummer „2"), Playhead unabhängig auf Index 2.
    app.carts_mut().select(Some(1));
    app.carts_mut().set_cursor(2);
    assert_eq!(app.carts().cursor(), 2);

    // ⌥⌘↑ → selektierter Cue rutscht nach oben.
    run_hotkeys(
        &mut app,
        &ctx,
        vec![alt_cmd_key_event(egui::Key::ArrowUp)],
        1.0,
    );
    assert_eq!(
        app.carts().active().cues()[0].number,
        "2",
        "⌥⌘↑ verschiebt den selektierten Cue nach oben"
    );
    assert_eq!(
        app.carts().selected_index(),
        Some(0),
        "Selektion folgt dem Cue"
    );
    assert_eq!(app.carts().cursor(), 2, "Playhead bleibt beim Verschieben");

    // ⌥⌘↓ → zurück.
    run_hotkeys(
        &mut app,
        &ctx,
        vec![alt_cmd_key_event(egui::Key::ArrowDown)],
        1.1,
    );
    assert_eq!(app.carts().active().cues()[1].number, "2");
    assert_eq!(app.carts().selected_index(), Some(1));

    // Nackter ↓ → Playhead +1, Selektion bleibt.
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::ArrowDown, false)],
        1.2,
    );
    assert_eq!(app.carts().cursor(), 3, "nackter Pfeil bewegt den Playhead");
    assert_eq!(
        app.carts().selected_index(),
        Some(1),
        "Selektion bleibt bei Playhead-Bewegung"
    );
}

#[test]
fn last_error_is_set_dismissed_and_shown_in_footer() {
    let (mut app, _cmd_rx, evt_tx) = hotkey_app();
    let ctx = egui::Context::default();
    kuhplay::gui::icons::install(&ctx);

    // Fehler auslösen: globaler Engine-Fehler (wie ein echter Engine-Error).
    evt_tx
        .send(EngineEvent::Error {
            cue_id: None,
            message: "demo failure".to_string(),
        })
        .unwrap();
    app.poll_engine_events();
    assert!(app.last_error().is_some(), "Fehler wird gemerkt");

    // Footer rendert die Fehlerzeile (Warn-Indikator + Schließen-Button).
    let render_footer = |ctx: &egui::Context, app: &mut ShowApp| -> egui::FullOutput {
        ctx.run(raw_input(vec![], 1.0), |ctx| {
            egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
                kuhplay::gui::footer::draw(ui, app);
            });
        })
    };
    let out = render_footer(&ctx, &mut app);
    assert!(
        find_text_center(&out, "demo failure").is_some(),
        "Footer zeigt den Warn-Indikator"
    );
    assert!(
        find_text_center(&out, "×").is_some(),
        "Footer zeigt den Schließen-Button"
    );

    // Dismiss → Fehleranzeige verschwindet.
    app.dismiss_error();
    assert!(app.last_error().is_none());
    let out = render_footer(&ctx, &mut app);
    assert!(
        find_text_center(&out, "demo failure").is_none(),
        "Fehleranzeige verschwindet nach Dismiss"
    );
}

#[test]
fn add_cue_numbers_are_unique_after_delete() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let list_len = app.carts().active().len();

    // Löschen → Lücke; neue Cues dürfen keine Nummer doppelt vergeben.
    app.carts_mut().select(Some(1));
    app.delete_selected();
    assert_eq!(app.carts().active().len(), list_len - 1);

    app.add_cue(CueType::Audio);
    let numbers: Vec<&str> = app
        .carts()
        .active()
        .cues()
        .iter()
        .map(|c| c.number.as_str())
        .collect();
    let mut sorted = numbers.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        numbers.len(),
        "keine doppelten Q-Nummern: {numbers:?}"
    );
    // höchste vorhandene Nummer + 1 (Demo-Max = 13 → „14").
    assert_eq!(app.carts().active().cues().last().unwrap().number, "14");
}

#[test]
fn delete_selected_stops_running_cue() {
    let (mut app, cmd_rx, _evt) = hotkey_app();

    // Cue 0 selektieren und „laufen" lassen (wie ein Engine-Event es meldet).
    app.carts_mut().select(Some(0));
    let id0 = app.carts().active().cues()[0].id;
    app.carts_mut()
        .active_mut()
        .set_status(id0, CueStatus::Playing)
        .unwrap();
    let before = app.carts().active().len();

    app.delete_selected();

    assert_eq!(app.carts().active().len(), before - 1, "Cue ist gelöscht");
    assert!(app.carts().find_cue_any(id0).is_none(), "Cue ist weg");
    assert!(
        cmd_rx
            .try_iter()
            .any(|c| matches!(c, EngineCommand::Stop { cue_id } if cue_id == id0)),
        "laufender Cue wird vor dem Löschen gestoppt (Engine Stop)"
    );
}

/// App verarbeitet die Kontextmenü-Aktionen (Phase 3): Duplicate, Arm,
/// Farbe, Group und Delete-per-ID (auch Group-Kind, Item 7).
#[test]
fn app_processes_context_menu_actions_end_to_end() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    {
        let list = app.carts_mut().active_mut();
        list.cues_mut().clear();
        list.add(Cue::new("1", "Alpha", CueType::Audio));
        list.add(Cue::new("2", "Beta", CueType::Audio));
        list.add(Cue::new("3", "Gamma", CueType::Audio));
    }
    let id1 = app.carts().active().cues()[0].id;
    let id2 = app.carts().active().cues()[1].id;
    let id3 = app.carts().active().cues()[2].id;

    // Duplicate: Kopie mit neuer ID nach dem Original, Kopie selektiert.
    let copies = app.duplicate_cues(&[id2]);
    assert_eq!(copies.len(), 1, "eine Kopie");
    assert_eq!(app.carts().active().len(), 4, "Kopie eingefügt");
    assert_eq!(
        app.carts().active().cues()[2].name,
        "Beta",
        "Kopie steht nach dem Original"
    );
    assert_ne!(app.carts().active().cues()[2].id, id2, "Kopie hat neue ID");
    assert_eq!(
        app.carts().active().selection().to_vec(),
        vec![copies[0]],
        "Kopie ist selektiert"
    );

    // Arm/Disarm per ID (Mehrfachselektion, wie das Kontextmenü meldet).
    app.set_cues_armed(&[id1, id3], false);
    let list = app.carts().active();
    assert!(!list.cues()[0].is_armed, "Alpha disarmed");
    assert!(
        list.cues()[1].is_armed,
        "Beta bleibt armiert (nicht betroffen)"
    );
    assert!(!list.cues()[3].is_armed, "Gamma disarmed");

    // Farbe setzen und löschen.
    app.set_cues_color(&[id1], Some([255, 0, 0]));
    assert_eq!(app.carts().active().cues()[0].color, Some([255, 0, 0]));
    app.set_cues_color(&[id1], None);
    assert_eq!(app.carts().active().cues()[0].color, None, "Farbe gelöscht");

    // Group: Alpha + Beta wandern in eine neue Gruppe (selektiert).
    let group_id = app.group_cues(&[id1, id2]).expect("Gruppierung möglich");
    let list = app.carts().active();
    assert_eq!(list.len(), 3, "zwei Cues in die Gruppe gewandert");
    let group = list
        .cues()
        .iter()
        .find(|c| c.id == group_id)
        .expect("Gruppe in der Liste");
    assert_eq!(group.cue_type, CueType::Group);
    assert_eq!(group.children.len(), 2, "Gruppe hat 2 Kinder");
    assert_eq!(
        list.selection().to_vec(),
        vec![group_id],
        "neue Gruppe ist selektiert"
    );

    // Delete per ID – auch ein Group-Kind (Item 7).
    let child_id = group.children[0].id;
    app.delete_cue_by_id(child_id);
    let list = app.carts().active();
    assert_eq!(list.len(), 3, "Top-Level-Anzahl unverändert");
    let group = list
        .cues()
        .iter()
        .find(|c| c.id == group_id)
        .expect("Gruppe noch da");
    assert_eq!(group.children.len(), 1, "Kind per ID gelöscht");
    assert!(
        app.carts().find_cue_any(child_id).is_none(),
        "Kind ist überall weg"
    );
}

#[test]
fn context_menu_delete_requests_delete_without_removing() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;
    kuhplay::gui::icons::install(&ctx);

    // Rendern-Variante mit Rückgabe des Lösch-Requests.
    let render =
        |ctx: &egui::Context, carts: &mut CartManager, events: Vec<egui::Event>, time: f64| {
            let mut request = None;
            let out = ctx.run(raw_input(events, time), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut columns = CueColumnLayout::default();
                    request = Some(cue_table::draw(
                        ui,
                        carts.active_mut(),
                        true,
                        &std::collections::HashMap::new(),
                        &mut columns,
                        &FindState::default(),
                    ));
                });
            });
            (out, request)
        };
    render(&ctx, &mut carts, vec![], time);
    render(&ctx, &mut carts, vec![], time);
    let id2 = carts.active().cues()[2].id;
    let y2 = find_row_y(&ctx, &mut carts, 2, &mut time).expect("Zeile 2 muss selektierbar sein");

    // Rechtsklick auf Zeile 2 → Kontextmenü öffnen.
    click_button(
        &ctx,
        &mut carts,
        egui::pos2(250.0, y2),
        egui::PointerButton::Secondary,
        &mut time,
    );
    // Menü rendern und „Delete Cue"-Position finden (bis zu 3 Versuche).
    let mut menu_pos = None;
    for _ in 0..3 {
        time += 0.05;
        let (out, _) = render(&ctx, &mut carts, vec![], time);
        if let Some(p) = find_text_center(&out, "Delete Cue") {
            menu_pos = Some(p);
            break;
        }
    }
    let menu_pos = menu_pos.expect("Kontextmenü zeigt „Delete Cue");

    // Klick auf „Delete Cue" (echter Klick: Move → Press → Release).
    let mut request = None;
    for events in [
        vec![egui::Event::PointerMoved(menu_pos)],
        vec![egui::Event::PointerButton {
            pos: menu_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: menu_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        time += 0.05;
        let (_, r) = render(&ctx, &mut carts, events, time);
        // `draw` liefert immer `Some(actions)` (ggf. leer) – nur nicht-leere
        // Aktionen übernehmen, sonst gewinnt der Move-Frame ohne Aktion.
        if r.as_ref().is_some_and(actions_nonempty) {
            request = r;
        }
    }

    assert_eq!(
        request.as_ref().map(|a| a.delete.clone()),
        Some(vec![id2]),
        "draw meldet den Lösch-Request für die Cue-ID von Zeile 2"
    );
    assert_eq!(carts.active().len(), 4, "draw löscht nicht selbst");
    assert_eq!(
        carts.active().cues()[2].number,
        "3",
        "Reihenfolge ist unverändert (Löschen macht der Aufrufer)"
    );
}

// ------------------------------------------------------------------
// Phase 1 (TODO.md): Footer-Warnindikator (Klick springt zum ersten
// kaputten Cue), Toolbox-Buttons legen Cues an, Zahnrad/Panel-Toggles
// im Footer, Menü „Workspace → Workspace Settings …".
// ------------------------------------------------------------------

/// Rendert ein einzelnes Panel (Footer / Toolbox / Menüleiste) mit ShowApp.
fn render_panel(
    ctx: &egui::Context,
    app: &mut ShowApp,
    events: Vec<egui::Event>,
    time: f64,
    panel: impl Fn(&egui::Context, &mut ShowApp),
) -> egui::FullOutput {
    kuhplay::gui::icons::install(ctx);
    ctx.run(raw_input(events, time), |ctx| panel(ctx, app))
}

/// Rendert den Footer (wie in `ShowApp::update`).
fn render_footer(
    ctx: &egui::Context,
    app: &mut ShowApp,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    render_panel(ctx, app, events, time, |ctx, app| {
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            kuhplay::gui::footer::draw(ui, app);
        });
    })
}

/// Rendert die Toolbox (linkes SidePanel, wie in `ShowApp::update`).
fn render_toolbox(
    ctx: &egui::Context,
    app: &mut ShowApp,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    render_panel(ctx, app, events, time, |ctx, app| {
        egui::SidePanel::left("toolbox")
            .resizable(true)
            .default_width(140.0)
            .show(ctx, |ui| {
                kuhplay::gui::toolbox::draw(ui, app);
            });
    })
}

#[test]
fn footer_warning_indicator_click_jumps_to_first_broken() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    // Demo-Show: Media-Dateien fehlen → kaputte Cues, Indikator „N broken".
    let broken = app.broken_count();
    assert!(broken > 0, "Demo-Show hat kaputte Cues");
    assert!(app.carts.selected_index().is_none());
    let mut time = 1.0;
    let label = format!("{broken} broken");
    let pos = find_text_center(&render_footer(&ctx, &mut app, vec![], time), &label)
        .expect("Warn-Indikator gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_footer(ctx, &mut app, events, t);
    });
    let selected = app
        .carts
        .selected()
        .expect("Klick selektiert kaputten Cue")
        .clone();
    let problems = app.problems_of(&selected);
    assert!(
        !problems.is_empty(),
        "selektierter Cue ist kaputt: {problems:?}"
    );
}

#[test]
fn footer_panel_toggles_flip_visibility() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    assert!(app.ui.sidebar_visible && app.ui.inspector_visible);
    let mut time = 1.0;
    // Sidebar-Toggle (Phosphor sidebar, U+EAB6).
    let pos = find_text_center(&render_footer(&ctx, &mut app, vec![], time), "\u{EAB6}")
        .expect("Sidebar-Toggle gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_footer(ctx, &mut app, events, t);
    });
    assert!(!app.ui.sidebar_visible, "Sidebar-Toggle blendet aus");
    // Inspector-Toggle (Phosphor sliders, U+E432).
    let pos = find_text_center(&render_footer(&ctx, &mut app, vec![], time), "\u{E432}")
        .expect("Inspector-Toggle gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_footer(ctx, &mut app, events, t);
    });
    assert!(!app.ui.inspector_visible, "Inspector-Toggle blendet aus");
}

#[test]
fn footer_gear_opens_workspace_settings() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    assert!(!app.ui.workspace_settings_open);
    let mut time = 1.0;
    // Zahnrad (Phosphor gear, U+E270) rechts im Footer.
    let pos = find_text_center(&render_footer(&ctx, &mut app, vec![], time), "\u{E270}")
        .expect("Zahnrad gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_footer(ctx, &mut app, events, t);
    });
    assert!(
        app.ui.workspace_settings_open,
        "Zahnrad öffnet Workspace Settings"
    );
}

#[test]
fn toolbox_button_adds_cue() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    let before = app.carts.active().len();
    let mut time = 1.0;
    let pos = find_text_center(&render_toolbox(&ctx, &mut app, vec![], time), "Audio")
        .expect("Toolbox-Button Audio gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_toolbox(ctx, &mut app, events, t);
    });
    assert_eq!(app.carts.active().len(), before + 1, "Cue wurde angelegt");
    let added = app.carts.active().cues().last().unwrap();
    assert_eq!(added.cue_type, CueType::Audio);
    assert!(
        added.name.starts_with("New Cue"),
        "auto-Name: {}",
        added.name
    );
    assert_eq!(
        app.carts.selected_index(),
        Some(before),
        "neuer Cue selektiert"
    );
}

#[test]
fn workspace_menu_opens_workspace_settings() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    assert!(!app.ui.workspace_settings_open);
    let mut time = 1.0;
    let render_menu = |ctx: &egui::Context,
                       app: &mut ShowApp,
                       events: Vec<egui::Event>,
                       t: f64|
     -> egui::FullOutput {
        render_panel(ctx, app, events, t, |ctx, app| {
            egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
                kuhplay::gui::menu_bar::draw(ui, app);
            });
        })
    };
    // Menü Workspace öffnen (Klick), dann Eintrag Workspace Settings.
    let pos = find_text_center(&render_menu(&ctx, &mut app, vec![], time), "Workspace")
        .expect("Menü Workspace gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_menu(ctx, &mut app, events, t);
    });
    let pos = find_text_center(
        &render_menu(&ctx, &mut app, vec![], time),
        "Workspace Settings…  ⌘,",
    )
    .expect("Menü-Eintrag Workspace Settings gerendert (Kürzel aus der zentralen Tabelle)");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_menu(ctx, &mut app, events, t);
    });
    assert!(
        app.ui.workspace_settings_open,
        "Menü öffnet Workspace Settings"
    );
}

// ------------------------------------------------------------------
// Phase 5: Zentrale Shortcut-Tabelle, Find (⌘F/⌘G), ⌫/⌘D/⌘0-9
// ------------------------------------------------------------------

/// Find-Matching: Nummer, Name und Notes, case-insensitive (Phase 5).
#[test]
fn find_hits_match_number_name_and_notes_case_insensitive() {
    let mut carts = demo_list(); // 4 Cues: Nummern „1“..„4“, Namen „Cue 1“..„Cue 4“
    carts.active_mut().cues_mut()[2].notes = "Spotlight SOLO".to_string();

    let hits = cue_table::find_hits(carts.active(), "cue");
    assert_eq!(hits.len(), 4, "Name matcht alle 4 Cues");

    let hits = cue_table::find_hits(carts.active(), "3");
    assert_eq!(
        hits.len(),
        1,
        "„3“ matcht Nummer UND Name von Cue 3 (derselbe Cue)"
    );
    assert_eq!(hits[0].list_index, 2);

    let hits = cue_table::find_hits(carts.active(), "spotlight");
    assert_eq!(hits.len(), 1, "Notes matchen case-insensitive");
    assert_eq!(hits[0].id, carts.active().cues()[2].id);

    assert!(
        cue_table::find_hits(carts.active(), "   ").is_empty(),
        "leerer Suchbegriff matcht nichts"
    );
    assert!(!cue_table::cue_matches(&carts.active().cues()[0], ""));
}

/// Enter/⌘G = nächster Treffer: Playhead + Selektion springen, wrappt am Ende.
#[test]
fn find_next_hit_cycles_cursor_and_selection() {
    let mut carts = demo_list();
    let mut find = FindState {
        open: true,
        query: "cue".to_string(),
        current: 0,
        query_prev: String::new(),
    };
    for expected in [0, 1, 2, 3, 0] {
        cue_table::find_next_hit(carts.active_mut(), &mut find);
        assert_eq!(
            carts.active().cursor(),
            expected,
            "Playhead auf Treffer {expected}"
        );
        assert_eq!(
            carts.active().selected_index(),
            Some(expected),
            "Selektion folgt dem Treffer"
        );
    }
    assert_eq!(find.current, 5, "Zähler zählt mit und wrappt via Modulo");

    // Query-Wechsel setzt den Zähler zurück.
    find.query = "3".to_string();
    cue_table::find_next_hit(carts.active_mut(), &mut find);
    assert_eq!(
        carts.active().cursor(),
        2,
        "neuer Suchbegriff startet vorne"
    );
}

/// ⌘F toggelt die Find-Leiste, ⌘G springt zum nächsten Treffer.
#[test]
fn cmd_f_toggles_find_bar_and_cmd_g_advances_to_next_match() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    // Deterministisch: alle Demo-Cues einheitlich benennen.
    for (i, cue) in app
        .carts_mut()
        .active_mut()
        .cues_mut()
        .iter_mut()
        .enumerate()
    {
        cue.name = format!("Cue {}", i + 1);
    }
    assert!(!app.ui.find.open);

    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::F, true)], 1.0);
    assert!(app.ui.find.open, "⌘F öffnet die Find-Leiste");

    app.ui.find.query = "cue".to_string();
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::G, true)], 1.1);
    assert_eq!(app.carts().cursor(), 0, "erster ⌘G → erster Treffer");
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::G, true)], 1.2);
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::G, true)], 1.3);
    assert_eq!(app.carts().cursor(), 2, "dritter ⌘G → dritter Treffer");
    assert_eq!(app.carts().selected_index(), Some(2), "Selektion folgt");

    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::F, true)], 1.4);
    assert!(!app.ui.find.open, "erneutes ⌘F schließt die Leiste");
    assert!(
        app.ui.find.query.is_empty(),
        "Query wird beim Schließen geleert"
    );
}

/// ⌫ löscht den selektierten Cue – nur im Edit-Mode (TODO.md Phase 5).
#[test]
fn delete_key_removes_selected_cue_only_in_edit_mode() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    let before = app.carts().active().len();

    app.carts_mut().select(Some(0));
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Backspace, false)],
        1.0,
    );
    assert_eq!(
        app.carts().active().len(),
        before - 1,
        "⌫ löscht den selektierten Cue (Edit-Mode)"
    );

    app.set_show_mode(true);
    app.carts_mut().select(Some(0));
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Backspace, false)],
        1.1,
    );
    assert_eq!(
        app.carts().active().len(),
        before - 1,
        "im Show-Mode löscht ⌫ nicht (nur Edit-Mode)"
    );
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Backspace, true)],
        1.2,
    );
    assert_eq!(
        app.carts().active().len(),
        before - 1,
        "⌘⌫ löscht im Show-Mode ebenfalls nicht"
    );
    app.set_show_mode(false);
}

/// ⇧⌘D dupliziert die Selektion; ⌘0–⌘9 legen Cues des Typs an (Cues-Menü).
#[test]
fn cmd_d_duplicates_selection_and_cmd_digits_add_typed_cues() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    let before = app.carts().active().len();

    app.carts_mut().select(Some(0));
    run_hotkeys(&mut app, &ctx, vec![shift_cmd_key_event(egui::Key::D)], 1.0);
    assert_eq!(
        app.carts().active().len(),
        before + 1,
        "⇧⌘D dupliziert den selektierten Cue"
    );

    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::Num1, true)], 1.1);
    assert_eq!(
        app.carts().active().selected().map(|c| c.cue_type),
        Some(CueType::Audio),
        "⌘1 = neuer Audio-Cue, direkt selektiert (Cues-Menü)"
    );
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::Num0, true)], 1.2);
    assert_eq!(
        app.carts().active().selected().map(|c| c.cue_type),
        Some(CueType::Group),
        "⌘0 = neuer Group-Cue, direkt selektiert (Cues-Menü)"
    );

    // Im Show-Mode legen die Ziffern keine Cues an.
    app.set_show_mode(true);
    let n = app.carts().active().len();
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::Num1, true)], 1.3);
    assert_eq!(app.carts().active().len(), n, "⌘1 im Show-Mode wirkungslos");
    app.set_show_mode(false);
}

/// `[`/`]`/Esc dispatchen Pause All / Resume All / Panic.
#[test]
fn pause_resume_panic_keys_dispatch_from_table() {
    let (mut app, cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();

    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::OpenBracket, false)],
        1.0,
    );
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::CloseBracket, false)],
        1.1,
    );
    // Keine laufenden Cues → Pause/Resume senden keine Commands.
    assert!(
        cmd_rx.try_recv().is_err(),
        "Pause/Resume ohne laufende Cues: keine Commands"
    );

    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::Escape, false)],
        1.2,
    );
    assert!(
        matches!(cmd_rx.try_recv(), Ok(EngineCommand::StopAll)),
        "Esc = Panic All → EngineCommand::StopAll"
    );
}

/// Help-Menü enthält „Keyboard Shortcuts“ und öffnet das Fenster.
#[test]
fn help_menu_opens_keyboard_shortcuts_window() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    assert!(!app.ui.shortcuts_open);
    let mut time = 1.0;
    let render_menu = |ctx: &egui::Context,
                       app: &mut ShowApp,
                       events: Vec<egui::Event>,
                       t: f64|
     -> egui::FullOutput {
        render_panel(ctx, app, events, t, |ctx, app| {
            egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
                kuhplay::gui::menu_bar::draw(ui, app);
            });
        })
    };
    let pos = find_text_center(&render_menu(&ctx, &mut app, vec![], time), "Help")
        .expect("Menü Help gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_menu(ctx, &mut app, events, t);
    });
    let pos = find_text_center(
        &render_menu(&ctx, &mut app, vec![], time),
        "Keyboard Shortcuts",
    )
    .expect("Menü-Eintrag Keyboard Shortcuts gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_menu(ctx, &mut app, events, t);
    });
    assert!(
        app.ui.shortcuts_open,
        "Help → Keyboard Shortcuts öffnet das Fenster"
    );
}

/// Menüs zeigen die Kürzel aus der zentralen Tabelle (Phase 5).
#[test]
fn menus_show_shortcuts_from_central_table() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx, _evt_tx) = hotkey_app();
    let mut time = 1.0;
    let render_menu = |ctx: &egui::Context,
                       app: &mut ShowApp,
                       events: Vec<egui::Event>,
                       t: f64|
     -> egui::FullOutput {
        render_panel(ctx, app, events, t, |ctx, app| {
            egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
                kuhplay::gui::menu_bar::draw(ui, app);
            });
        })
    };
    // File-Menü: Kürzel aus der Tabelle.
    let pos = find_text_center(&render_menu(&ctx, &mut app, vec![], time), "File")
        .expect("Menü File gerendert");
    click_via(&ctx, pos, &mut time, |ctx, events, t| {
        render_menu(ctx, &mut app, events, t);
    });
    let out = render_menu(&ctx, &mut app, vec![], time);
    for expected in [
        "New Show  ⌘N",
        "Open Show…  ⌘O",
        "Save Show  ⌘S",
        "Save Show As…  Shift+⌘S",
        "Quit  ⌘Q",
    ] {
        assert!(
            find_text_center(&out, expected).is_some(),
            "File-Menü zeigt „{expected}“"
        );
    }
}

// ------------------------------------------------------------------
// Phase 6: Datei-Drop aus dem Finder legt passende Cues an
// ------------------------------------------------------------------

/// Drag & Drop von Dateien (TODO.md Phase 6): Endung → Cue-Typ, Datei als
/// Target, neuer Cue selektiert; im Show-Mode und bei unbekannten
/// Endungen passiert nichts.
#[test]
fn dropped_files_create_matching_cues() {
    use std::path::Path;
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let before = app.carts().active().len();

    let id = app
        .add_cue_from_file(Path::new("/media/sound.mp3"))
        .expect("mp3 → Audio-Cue");
    let list = app.carts().active();
    assert_eq!(list.len(), before + 1);
    let cue = &list.cues()[list.len() - 1];
    assert_eq!(cue.id, id);
    assert_eq!(cue.cue_type, CueType::Audio);
    assert_eq!(cue.target_path.as_deref(), Some("/media/sound.mp3"));
    assert_eq!(cue.name, "sound.mp3", "Dateiname als Cue-Name");
    assert_eq!(
        list.selected_index(),
        Some(list.len() - 1),
        "neuer Cue selektiert"
    );

    app.add_cue_from_file(Path::new("/media/clip.MP4"))
        .expect("mp4 → Video");
    assert_eq!(
        app.carts().active().cues().last().unwrap().cue_type,
        CueType::Video
    );
    app.add_cue_from_file(Path::new("/slides/bild.png"))
        .expect("png → Slideshow");
    assert_eq!(
        app.carts().active().cues().last().unwrap().cue_type,
        CueType::SlideShow
    );

    let n = app.carts().active().len();
    assert!(
        app.add_cue_from_file(Path::new("/etc/notes.txt")).is_none(),
        "unbekannte Endung → None"
    );
    assert_eq!(app.carts().active().len(), n);

    // Show-Mode: keine neuen Cues per Drop.
    app.set_show_mode(true);
    assert!(app
        .add_cue_from_file(Path::new("/media/sound.mp3"))
        .is_none());
    assert_eq!(
        app.carts().active().len(),
        n,
        "im Show-Mode legt Drop nichts an"
    );
    app.set_show_mode(false);
}

/// macOS: Ctrl-Klick gilt als Rechtsklick, auch wenn Ctrl vor dem Loslassen endet.
#[test]
fn ctrl_click_becomes_right_click() {
    use egui::{Event, Modifiers, PointerButton};
    let button = |pressed, modifiers| Event::PointerButton {
        pos: egui::pos2(1.0, 1.0),
        button: PointerButton::Primary,
        pressed,
        modifiers,
    };
    let is_secondary = |event: &Event| {
        matches!(
            event,
            Event::PointerButton {
                button: PointerButton::Secondary,
                ..
            }
        )
    };
    let mut active = false;
    let mut events = vec![
        button(true, Modifiers::CTRL),
        button(false, Modifiers::NONE),
        button(true, Modifiers::NONE),
        button(false, Modifiers::NONE),
    ];
    kuhplay::gui::app::ctrl_click_to_secondary(&mut events, &mut active);
    assert!(is_secondary(&events[0]) && is_secondary(&events[1]));
    assert!(
        !is_secondary(&events[2]) && !is_secondary(&events[3]),
        "normaler Klick bleibt"
    );
}

/// ⌘↑/⌘↓ wählen Cues aus, ⇧⌘↑/⇧⌘↓ bewegen den Playhead, ⌘D löscht Nummern, F setzt das Flag.
#[test]
fn selection_playhead_number_and_flag_keys() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    app.carts_mut().select(Some(1));
    let cursor = app.carts().cursor();
    let order: Vec<String> = app
        .carts()
        .active()
        .cues()
        .iter()
        .map(|c| c.number.clone())
        .collect();

    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::ArrowUp, true)],
        1.0,
    );
    assert_eq!(
        app.carts().selected_index(),
        Some(0),
        "⌘↑ wählt den vorherigen Cue"
    );
    run_hotkeys(
        &mut app,
        &ctx,
        vec![key_event(egui::Key::ArrowDown, true)],
        1.1,
    );
    assert_eq!(
        app.carts().selected_index(),
        Some(1),
        "⌘↓ wählt den nächsten Cue"
    );
    let after: Vec<String> = app
        .carts()
        .active()
        .cues()
        .iter()
        .map(|c| c.number.clone())
        .collect();
    assert_eq!(order, after, "⌘↑/⌘↓ verschieben nichts");
    assert_eq!(
        app.carts().cursor(),
        cursor,
        "⌘↑/⌘↓ bewegen den Playhead nicht"
    );

    run_hotkeys(
        &mut app,
        &ctx,
        vec![shift_cmd_key_event(egui::Key::ArrowDown)],
        1.2,
    );
    assert_eq!(app.carts().cursor(), cursor + 1, "⇧⌘↓ bewegt den Playhead");
    assert_eq!(
        app.carts().selected_index(),
        Some(1),
        "⇧⌘↓ ändert die Auswahl nicht"
    );

    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::F, false)], 1.3);
    assert!(app.carts().active().cues()[1].flagged, "F setzt das Flag");
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::D, true)], 1.4);
    assert_eq!(
        app.carts().active().cues()[1].number,
        "",
        "⌘D löscht die Nummer"
    );
    assert_eq!(
        app.carts().active().len(),
        order.len(),
        "⌘D legt keinen Cue an"
    );
}

/// ⌘3 ist Video, ⌘7 Fade, ⌘9 MIDI; ⌘2 und ⌘4 bleiben frei.
#[test]
fn digit_keys_create_the_agreed_cue_types() {
    let (mut app, _cmd_rx, _evt) = hotkey_app();
    let ctx = egui::Context::default();
    for (i, (key, cue_type)) in [
        (egui::Key::Num3, CueType::Video),
        (egui::Key::Num5, CueType::Text),
        (egui::Key::Num6, CueType::Light),
        (egui::Key::Num7, CueType::Fade),
        (egui::Key::Num8, CueType::Network),
        (egui::Key::Num9, CueType::Midi),
    ]
    .into_iter()
    .enumerate()
    {
        run_hotkeys(
            &mut app,
            &ctx,
            vec![key_event(key, true)],
            1.0 + i as f64 / 10.0,
        );
        assert_eq!(
            app.carts().active().selected().map(|c| c.cue_type),
            Some(cue_type)
        );
    }
    let count = app.carts().active().len();
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::Num2, true)], 2.0);
    run_hotkeys(&mut app, &ctx, vec![key_event(egui::Key::Num4, true)], 2.1);
    assert_eq!(app.carts().active().len(), count, "⌘2 und ⌘4 sind frei");
}

/// Rendert die Tabelle und liefert die Mitte des Zeilentexts `text`.
fn row_text_center(
    ctx: &egui::Context,
    carts: &mut CartManager,
    text: &str,
    time: f64,
) -> egui::Pos2 {
    let mut columns = CueColumnLayout::default();
    let (out, _) = render_actions(ctx, carts, &mut columns, vec![], time);
    find_text_center(&out, text).unwrap_or_else(|| panic!("Text „{text}“ nicht gefunden"))
}

/// Klicks direkt auf den Zeilentext erreichen die Zeile (Labels dürfen sie
/// nicht als Textauswahl abfangen).
#[test]
fn click_on_row_text_selects_the_row() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;
    let pos = row_text_center(&ctx, &mut carts, "Cue 2", time);
    let pos = row_text_center(&ctx, &mut carts, "Cue 2", time).lerp(pos, 0.5);
    click(&ctx, &mut carts, pos, &mut time);
    assert_eq!(carts.selected_index(), Some(1));
}

/// Rechtsklick auf den Zeilentext öffnet das Kontextmenü.
#[test]
fn right_click_on_row_text_opens_context_menu() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut columns = CueColumnLayout::default();
    let mut time = 1.0;
    let _ = row_text_center(&ctx, &mut carts, "Cue 2", time);
    let pos = row_text_center(&ctx, &mut carts, "Cue 2", time);
    click_button_mod(
        &ctx,
        &mut carts,
        &mut columns,
        pos,
        egui::PointerButton::Secondary,
        egui::Modifiers::NONE,
        &mut time,
    );
    let mut found = false;
    for _ in 0..3 {
        time += 0.05;
        let (out, _) = render_actions(&ctx, &mut carts, &mut columns, vec![], time);
        found |= find_text_center(&out, "Set Playhead Here").is_some();
    }
    assert!(found, "Rechtsklick auf den Text muss das Menü öffnen");
}

/// Der Klick auf die Nummer setzt den Playhead, GO startet dann genau diesen Cue.
#[test]
fn click_on_number_text_moves_playhead_and_go_starts_that_cue() {
    let ctx = egui::Context::default();
    let mut carts = demo_list();
    let mut time = 1.0;
    let _ = row_text_center(&ctx, &mut carts, "3", time);
    let pos = row_text_center(&ctx, &mut carts, "3", time);
    click(&ctx, &mut carts, pos, &mut time);
    assert_eq!(carts.cursor(), 2, "Klick auf die Nummer setzt den Playhead");
    assert_eq!(carts.go_chained().map(|o| o.index), Some(2));
}
