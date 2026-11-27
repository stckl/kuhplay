//! Geometry-Tab über die echte Oberfläche: Stage für Gruppen-Kinder setzen,
//! auch für mehrere markierte Cues auf einmal; bleibt nach Speichern/Laden.

use kuhplay::core::{Cue, CueType, GroupMode};
use kuhplay::gui::ShowApp;

fn frame(
    app: &mut ShowApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    t: f64,
) -> egui::FullOutput {
    let raw = egui::RawInput {
        events,
        time: Some(t),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        )),
        ..Default::default()
    };
    ctx.run(raw, |ctx| {
        app.swap_text_field_registry();
        app.handle_hotkeys(ctx);
        app.draw_workspace(ctx);
        app.refresh_text_field_gate(ctx);
    })
}

fn click(app: &mut ShowApp, ctx: &egui::Context, p: egui::Pos2, t: &mut f64) {
    let button = |pressed| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    for ev in [egui::Event::PointerMoved(p), button(true), button(false)] {
        frame(app, ctx, vec![ev], *t);
        *t += 0.05;
    }
    frame(app, ctx, vec![], *t);
    *t += 0.5;
}

fn text_positions(out: &egui::FullOutput, needle: &str) -> Vec<egui::Pos2> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == needle => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn stage_for_selected_group_children_is_set_and_saved() {
    let mut app = ShowApp::new(kuhplay::engine::spawn_mock_engine());
    app.new_show();
    let a = Cue::new("1.1", "a", CueType::Video).with_target("/a.mp4");
    let b = Cue::new("1.2", "b", CueType::Video).with_target("/b.mp4");
    let (a_id, b_id) = (a.id, b.id);
    app.carts_mut()
        .active_mut()
        .add(Cue::new_group("1", "G", GroupMode::Sequential, vec![a, b]));
    app.carts_mut().active_mut().select_id(Some(a_id));
    app.carts_mut().active_mut().toggle_select_id(b_id);
    app.ui.inspector_tab = kuhplay::gui::app::InspectorTab::Geometry;
    let ctx = egui::Context::default();
    kuhplay::gui::theme::apply_theme(&ctx);
    let mut t = 0.0;
    for _ in 0..3 {
        frame(&mut app, &ctx, vec![], t);
        t += 0.1;
    }
    let out = frame(&mut app, &ctx, vec![], t);
    t += 0.1;
    let combo = text_positions(&out, "— none —");
    click(&mut app, &ctx, combo[0], &mut t);
    let out = frame(&mut app, &ctx, vec![], t);
    t += 0.1;
    let item = *text_positions(&out, "Stage 1")
        .last()
        .expect("Stage 1 im Menü");
    click(&mut app, &ctx, item, &mut t);

    for id in [a_id, b_id] {
        let cue = app.carts().active().find_cue(id).unwrap();
        assert_eq!(cue.stage.as_deref(), Some("Stage 1"), "{}", cue.name);
    }
    let path = std::env::temp_dir().join(format!("kuhplay-geo-{}.kuhplay", std::process::id()));
    app.save_show(&path);
    let mut loaded = ShowApp::new(kuhplay::engine::spawn_mock_engine());
    loaded.load_show(&path).unwrap();
    for id in [a_id, b_id] {
        let cue = loaded.carts().active().find_cue(id).unwrap();
        assert_eq!(
            cue.stage.as_deref(),
            Some("Stage 1"),
            "geladen: {}",
            cue.name
        );
    }
    let _ = std::fs::remove_file(path);
}
