//! Typ-spezifische Tabs für Medien-/Daten-Cues (TODO.md Phase 4):
//! „Source" (HTML5), „Slides" (Diashow), „Data" (HTML-Data).

use std::path::PathBuf;

use egui::RichText;

use crate::core::{CueType, HtmlSource, HtmlSourceKind, Slide, SlideShow};
use crate::engine::EngineCommand;

use super::{pick_html_file, pick_image_files, theme};
use crate::gui::app::ShowApp;

/// Tab „Source" (HTML5-Cue): Quellen-Editor (Datei/URL/Inline) mit
/// Live-Validierung und Test-Button für die JS-Bridge (`EvalJs` an die
/// laufende WebView).
pub fn draw_source(ui: &mut egui::Ui, app: &mut ShowApp) {
    let engine = &app.engine;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Html {
        ui.label(RichText::new("Source only for HTML5 cues").weak());
        return;
    }
    theme::form_grid("inspector_source_grid").show(ui, |ui| {
        theme::field_label(ui, "HTML Source");
        let mut kind = cue
            .html_source
            .as_ref()
            .map(|s| s.kind())
            .unwrap_or(HtmlSourceKind::Url);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("html_source_kind_combo")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for k in HtmlSource::ALL_KINDS {
                        ui.selectable_value(&mut kind, k, k.label());
                    }
                });
            // Quellen-Art wechseln (Inhalt bleibt erhalten).
            match &mut cue.html_source {
                Some(source) if source.kind() == kind => {}
                Some(source) => {
                    let text = source.edit_text();
                    *source = match kind {
                        HtmlSourceKind::File => HtmlSource::file(text),
                        HtmlSourceKind::Url => HtmlSource::url(text),
                        HtmlSourceKind::Inline => HtmlSource::inline(text),
                    };
                }
                None => {
                    cue.html_source = Some(match kind {
                        HtmlSourceKind::File => HtmlSource::file(""),
                        HtmlSourceKind::Url => HtmlSource::url("https://"),
                        HtmlSourceKind::Inline => HtmlSource::inline("<html><body></body></html>"),
                    });
                }
            }
            if let Some(source) = &mut cue.html_source {
                ui.horizontal(|ui| {
                    let mut text = source.edit_text();
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(360.0));
                    app.text_field_ids.insert(r.id);
                    if r.changed() {
                        source.set_edit_text(text);
                    }
                    if matches!(kind, HtmlSourceKind::File)
                        && ui
                            .small_button("…")
                            .on_hover_text("Choose HTML file (native dialog)")
                            .clicked()
                    {
                        if let Some(path) = pick_html_file() {
                            *source = HtmlSource::file(path);
                        }
                    }
                });
            }
        });
        ui.end_row();

        theme::field_label(ui, "Source valid");
        match &cue.html_source {
            Some(source) => match source.validate() {
                Ok(()) => {
                    ui.colored_label(theme::palette::PLAYING, "ok");
                }
                Err(err) => {
                    ui.colored_label(theme::palette::ERROR, err.to_string());
                }
            },
            None => {
                ui.colored_label(theme::palette::ERROR, "no source");
            }
        }
        ui.end_row();

        theme::field_label(ui, "JS Test");
        ui.horizontal(|ui| {
            let r = ui.text_edit_singleline(&mut cue.js_test_script);
            app.text_field_ids.insert(r.id);
            if ui
                .button("Send JS")
                .on_hover_text("Send JS to the running WebView (EvalJs)")
                .clicked()
            {
                let script = cue.js_test_script.clone();
                if let Err(err) = engine.send(EngineCommand::EvalJs {
                    cue_id: cue.id,
                    script,
                }) {
                    tracing::warn!(%err, "inspector: eval_js send failed");
                }
            }
        });
        ui.end_row();
    });
}

/// Tab „Slides" (Diashow-Cue): Slide-Liste (Pfad + Hold-Zeit) mit
/// Add/Remove, Crossfade, Auto-Advance/Loop und Live-Validierung.
pub fn draw_slides(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::SlideShow {
        ui.label(RichText::new("Slides only for slideshow cues").weak());
        return;
    }
    if cue.slideshow.is_none() {
        cue.slideshow = Some(SlideShow::default());
    }
    let Some(show) = &mut cue.slideshow else {
        return;
    };
    ui.columns(2, |uis| {
        let [left, right] = uis else {
            return;
        };
        left.vertical(|ui| {
            let mut remove: Option<usize> = None;
            for (i, slide) in show.slides.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("#{}", i + 1));
                    let mut path = slide.path.display().to_string();
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut path)
                            .desired_width(ui.available_width() - 150.0),
                    );
                    app.text_field_ids.insert(r.id);
                    if r.changed() {
                        slide.path = PathBuf::from(path);
                    }
                    let mut secs = slide.hold.as_secs_f64();
                    if ui
                        .add(
                            egui::DragValue::new(&mut secs)
                                .speed(0.1)
                                .range(0.1..=3600.0),
                        )
                        .changed()
                    {
                        slide.hold = std::time::Duration::from_secs_f64(secs.max(0.1));
                    }
                    ui.label("s");
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                show.slides.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button("+ Add Slide").clicked() {
                    show.slides.push(Slide {
                        path: PathBuf::new(),
                        hold: std::time::Duration::from_secs(3),
                    });
                }
                if ui
                    .button("+ Add Images…")
                    .on_hover_text("Choose image files (native dialog)")
                    .clicked()
                {
                    for path in pick_image_files() {
                        show.slides.push(Slide {
                            path,
                            hold: std::time::Duration::from_secs(3),
                        });
                    }
                }
            });
        });

        theme::form_grid("inspector_slides_grid").show(right, |ui| {
            theme::field_label(ui, "Crossfade (s)");
            let mut secs = show.crossfade.as_secs_f64();
            if ui
                .add(egui::DragValue::new(&mut secs).speed(0.1).range(0.0..=60.0))
                .changed()
            {
                show.crossfade = std::time::Duration::from_secs_f64(secs.max(0.0));
            }
            ui.end_row();

            theme::field_label(ui, "Auto-Advance");
            ui.checkbox(&mut show.auto_advance, "advance automatically");
            ui.end_row();

            theme::field_label(ui, "Loop Slides");
            ui.checkbox(&mut show.loop_slides, "restart after the last slide");
            ui.end_row();

            theme::field_label(ui, "Slides valid");
            match show.validate() {
                Ok(()) => {
                    ui.colored_label(theme::palette::PLAYING, "ok");
                }
                Err(err) => {
                    ui.colored_label(theme::palette::ERROR, err.to_string());
                }
            }
            ui.end_row();
        });
    });
}

/// Tab „Data" (HTML-Data-Cue): Ziel-HTML (HTML5-Cues des Carts), JSON-Editor
/// mit Live-Validierung und Test-Push (`PushJson`).
pub fn draw_data(ui: &mut egui::Ui, app: &mut ShowApp) {
    let targets = app.html_targets();
    let engine = &app.engine;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::HtmlData {
        ui.label(RichText::new("Data only for HTML data cues").weak());
        return;
    }
    theme::form_grid("inspector_data_grid").show(ui, |ui| {
        theme::field_label(ui, "Target HTML");
        let mut target = cue.data_target;
        let selected_label = target
            .and_then(|id| {
                targets
                    .iter()
                    .find(|(tid, _)| *tid == id)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| "select target…".to_string());
        egui::ComboBox::from_id_salt("html_data_target_combo")
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for (id, label) in &targets {
                    ui.selectable_value(&mut target, Some(*id), label.as_str());
                }
            });
        if target != cue.data_target {
            cue.data_target = target;
        }
        ui.end_row();

        theme::field_label(ui, "JSON Payload");
        let r = ui.text_edit_multiline(&mut cue.data_json_text);
        app.text_field_ids.insert(r.id);
        ui.end_row();

        theme::field_label(ui, "JSON valid");
        match cue.data_payload() {
            Ok(_) => {
                ui.colored_label(theme::palette::PLAYING, "ok");
            }
            Err(err) => {
                ui.colored_label(theme::palette::ERROR, err.to_string());
            }
        }
        ui.end_row();

        theme::field_label(ui, "Send Test");
        let command = cue.data_target.and_then(|target| {
            let payload = cue.data_payload().ok()?;
            Some(EngineCommand::PushJson { target, payload })
        });
        let button = ui
            .add_enabled(command.is_some(), egui::Button::new("Push JSON to Target"))
            .on_hover_text("Send JSON payload to the target WebView (PushJson)")
            .on_disabled_hover_text("Needs a target and valid JSON");
        if button.clicked() {
            if let Some(command) = command {
                if let Err(err) = engine.send(command) {
                    tracing::warn!(%err, "inspector: push_json send failed");
                }
            }
        }
        ui.end_row();
    });
}
