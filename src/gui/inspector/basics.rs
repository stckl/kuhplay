//! Tab „Basics" – dreispaltig, damit bei Default-Höhe des Inspectors alles
//! ohne Scrollen sichtbar ist (docs/UI.md §5.5):
//!
//! * Links: Q# / Name / Target (mit „…"-Button) / Dateistatus bzw.
//!   typ-spezifische Felder (Control: Action; Group: Mode/Kinder).
//! * Mitte: Pre Wait / Duration / Post Wait / Continue.
//! * Rechts: Type + Armed, Color (Swatches), Notes.

use std::time::Duration;

use egui::RichText;

use crate::core::{ContinueMode, ControlAction, CueType, GroupMode, CUE_COLORS};

use super::theme::palette;
use super::{icons, pick_media_file, theme, time_field};
use crate::gui::app::ShowApp;

/// Tab „Basics".
pub fn draw_basics(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Vorschläge fürs Control-Ziel: alle Cues mit Nummer (inkl. Gruppen-Kinder).
    let suggestions: Vec<(uuid::Uuid, String, String)> = {
        fn walk(cues: &[crate::core::Cue], out: &mut Vec<(uuid::Uuid, String, String)>) {
            for cue in cues {
                if !cue.number.is_empty() {
                    out.push((cue.id, cue.number.clone(), cue.name.clone()));
                }
                walk(&cue.children, out);
            }
        }
        let mut out = Vec::new();
        walk(app.carts.active().cues(), &mut out);
        out
    };
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    ui.columns(3, |uis| {
        let [left, middle, right] = uis else {
            return;
        };

        // Feste Feldbreite aus der Spalte: Die Breite im Grid wächst mit dem
        // Inhalt, lange Pfade schöben den „…"-Button sonst aus der Spalte.
        let field_width = (left.available_width()
            - theme::size::FORM_LABEL_WIDTH
            - theme::size::SPACE_MEDIUM_LARGE)
            .max(120.0);
        theme::form_grid("inspector_basics_left").show(left, |ui| {
            theme::field_label(ui, "Q#");
            let r = ui.add(
                egui::TextEdit::singleline(&mut cue.number)
                    .desired_width(theme::size::FORM_NUMBER_WIDTH),
            );
            app.text_field_ids.insert(r.id);
            ui.end_row();

            theme::field_label(ui, "Name");
            let r = ui.add(egui::TextEdit::singleline(&mut cue.name).desired_width(field_width));
            app.text_field_ids.insert(r.id);
            ui.end_row();

            // Target-Zeile: je nach Typ (Target prominent in Basics).
            theme::field_label(ui, "Target");
            match cue.cue_type {
                CueType::Audio | CueType::Video | CueType::Image => {
                    ui.horizontal(|ui| {
                        let mut path_text = cue.target_path.clone().unwrap_or_default();
                        let path_resp = ui
                            .add(
                                egui::TextEdit::singleline(&mut path_text)
                                    .desired_width(field_width - 32.0),
                            )
                            .on_hover_text("File path or stream URL");
                        app.text_field_ids.insert(path_resp.id);
                        if path_resp.changed() {
                            cue.target_path = if path_text.trim().is_empty() {
                                None
                            } else {
                                Some(path_text)
                            };
                        }
                        if ui
                            .small_button("…")
                            .on_hover_text("Choose media file (native dialog)")
                            .clicked()
                        {
                            if let Some(path) = pick_media_file() {
                                cue.target_path = Some(path);
                            }
                        }
                    });
                }
                CueType::Control => {
                    let own = cue.id;
                    ui.horizontal(|ui| {
                        let target = cue.control_target.get_or_insert_with(String::new);
                        let r = ui
                            .add(
                                egui::TextEdit::singleline(target)
                                    .hint_text("all")
                                    .desired_width(field_width - 70.0),
                            )
                            .on_hover_text(
                                "Target cue number (empty = all) – type, pick from ▾ \
                                 or drag a cue here from the cue list",
                            );
                        app.text_field_ids.insert(r.id);
                        // Cue aus der Liste hierher gezogen → seine Nummer.
                        if let Some(id) = super::dropped_cue(ui, &r) {
                            if let Some((_, number, _)) = suggestions
                                .iter()
                                .find(|(sid, ..)| *sid == id && *sid != own)
                            {
                                *target = number.clone();
                            }
                        }
                        // Vorschläge: passend zum Getippten (Nummer oder Name).
                        let query = target.trim().to_lowercase();
                        ui.menu_button("▾", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(260.0)
                                .show(ui, |ui| {
                                    if ui.button("— all —").clicked() {
                                        target.clear();
                                        ui.close_menu();
                                    }
                                    for (id, number, name) in &suggestions {
                                        if *id == own {
                                            continue;
                                        }
                                        let hit = query.is_empty()
                                            || number.to_lowercase().contains(&query)
                                            || name.to_lowercase().contains(&query);
                                        if hit && ui.button(format!("{number}  {name}")).clicked() {
                                            *target = number.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });
                        })
                        .response
                        .on_hover_text("Choose the target cue");
                        // Aufgelöstes Ziel anzeigen (oder Warnung).
                        let number = target.trim();
                        if !number.is_empty() {
                            match suggestions.iter().find(|(_, n, _)| n == number) {
                                Some((_, _, name)) => {
                                    ui.label(RichText::new(name).color(palette::TEXT_DIM));
                                }
                                None => {
                                    ui.label(RichText::new("unknown").color(palette::ERROR));
                                }
                            }
                        }
                    });
                    // Leer = „alle" bzw. der Cue selbst (wie ohne Ziel).
                    if cue
                        .control_target
                        .as_deref()
                        .is_some_and(|t| t.trim().is_empty())
                    {
                        cue.control_target = None;
                    }
                }
                _ => {
                    ui.label(RichText::new("—").weak());
                }
            }
            ui.end_row();

            // Dateistatus (nur Audio/Video).
            if matches!(cue.cue_type, CueType::Audio | CueType::Video) {
                theme::field_label(ui, "File");
                match &cue.target_path {
                    Some(path) if path.starts_with("http://") || path.starts_with("https://") => {
                        ui.label(RichText::new("stream url").weak());
                    }
                    Some(path) if std::path::Path::new(path).is_file() => {
                        ui.colored_label(theme::palette::PLAYING, "exists");
                    }
                    Some(_) => {
                        ui.colored_label(theme::palette::ERROR, "missing");
                    }
                    None => {
                        ui.label(RichText::new("no target").weak());
                    }
                }
                ui.end_row();
            }

            // Control: Aktion direkt unter dem Target.
            if cue.cue_type == CueType::Control {
                theme::field_label(ui, "Action");
                let mut action = cue.control_action;
                egui::ComboBox::from_id_salt("control_action_combo")
                    .selected_text(action.label())
                    .show_ui(ui, |ui| {
                        for a in ControlAction::ALL {
                            ui.selectable_value(&mut action, a, a.label());
                        }
                    });
                if action != cue.control_action {
                    cue.control_action = action;
                }
                ui.end_row();
            }

            // Group: Mode + Kinder (Basics).
            if cue.cue_type == CueType::Group {
                theme::field_label(ui, "Group Mode");
                let mut mode = cue.group_mode;
                egui::ComboBox::from_id_salt("group_mode_combo")
                    .selected_text(mode.label())
                    .show_ui(ui, |ui| {
                        for m in GroupMode::ALL {
                            ui.selectable_value(&mut mode, m, m.label());
                        }
                    });
                if mode != cue.group_mode {
                    cue.group_mode = mode;
                }
                ui.end_row();

                theme::field_label(ui, "Children");
                ui.vertical(|ui| {
                    if cue.children.is_empty() {
                        ui.label(RichText::new("no children").weak());
                    }
                    for child in &cue.children {
                        ui.horizontal(|ui| {
                            icons::draw(
                                ui,
                                icons::Icon::for_cue_type(child.cue_type),
                                14.0,
                                theme::palette::TEXT,
                            );
                            ui.label(format!("{}  {}", child.number, child.name));
                        });
                    }
                });
                ui.end_row();
            }
        });

        theme::form_grid("inspector_basics_middle").show(middle, |ui| {
            // Zeiten als Timecode `MM:SS.mmm` (wie die Cue-Liste).
            theme::field_label(ui, "Pre Wait");
            time_field(ui, &mut cue.pre_wait);
            ui.end_row();

            theme::field_label(ui, "Duration");
            time_field(ui, &mut cue.duration);
            ui.end_row();

            theme::field_label(ui, "Post Wait");
            time_field(ui, &mut cue.post_wait);
            ui.end_row();

            theme::field_label(ui, "Continue");
            let mut mode = cue.continue_mode;
            egui::ComboBox::from_id_salt("continue_mode_combo")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m in ContinueMode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
            if mode != cue.continue_mode {
                cue.continue_mode = mode;
            }
            ui.end_row();
        });

        right.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Type").color(theme::palette::TEXT_DIM));
                egui::ComboBox::from_id_salt("cue_type_combo")
                    .selected_text(cue.cue_type.label())
                    .show_ui(ui, |ui| {
                        for cue_type in CueType::ALL {
                            ui.selectable_value(&mut cue.cue_type, cue_type, cue_type.label());
                        }
                    });
                ui.add_space(theme::size::SPACE_MEDIUM);
                ui.checkbox(&mut cue.is_armed, "Armed")
                    .on_hover_text("Armed: GO includes this cue (disarmed cues are skipped)");
                if matches!(cue.cue_type, CueType::Video | CueType::SlideShow) {
                    ui.label("At end:");
                    ui.selectable_value(&mut cue.hold_last_frame, false, "Black");
                    ui.selectable_value(&mut cue.hold_last_frame, true, "Hold last frame")
                        .on_hover_text(
                            "The last frame stays until it is stopped or replaced on its layer",
                        );
                }
            });
            if matches!(
                cue.cue_type,
                CueType::Video
                    | CueType::SlideShow
                    | CueType::Image
                    | CueType::Html
                    | CueType::Text
                    | CueType::Audio
            ) {
                ui.horizontal(|ui| {
                    let others = if cue.cue_type == CueType::Audio {
                        "other running audio cues"
                    } else {
                        "other running cues on the same stage"
                    };
                    ui.label(RichText::new("On start").color(theme::palette::TEXT_DIM))
                        .on_hover_text(format!(
                            "What happens to the {others} when this cue starts"
                        ));
                    // 0 = Keep, 1 = Kill, 2 = Fade out
                    let mut mode = match (cue.kill_others, cue.kill_others_fade.is_zero()) {
                        (false, _) => 0,
                        (true, true) => 1,
                        (true, false) => 2,
                    };
                    let before = mode;
                    ui.selectable_value(&mut mode, 0, "Keep others");
                    ui.selectable_value(&mut mode, 1, "Kill others")
                        .on_hover_text(format!("Stop the {others} immediately"));
                    ui.selectable_value(&mut mode, 2, "Fade out others")
                        .on_hover_text(format!(
                            "Fade out the {others} (picture and sound), then stop them"
                        ));
                    if mode != before {
                        cue.kill_others = mode != 0;
                        cue.kill_others_fade = match mode {
                            2 => Duration::from_secs(2),
                            _ => Duration::ZERO,
                        };
                    }
                    if mode == 2 {
                        let mut fade = cue.kill_others_fade;
                        if super::time_field(ui, &mut fade)
                            .on_hover_text("Fade-out time")
                            .changed()
                        {
                            // 0 hieße „Kill" – kürzester Fade 0,1 s.
                            cue.kill_others_fade = fade.max(Duration::from_millis(100));
                        }
                    }
                });
            }

            // „Cue Color": Paletten-Swatches + Entfernen.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color").color(theme::palette::TEXT_DIM));
                ui.spacing_mut().item_spacing.x = theme::size::SPACE_SMALL;
                for rgb in CUE_COLORS {
                    let selected = cue.color == Some(rgb);
                    let fill = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let btn = egui::Button::new("")
                        .fill(fill)
                        .min_size(egui::vec2(18.0, 18.0));
                    let btn = if selected {
                        btn.stroke(egui::Stroke::new(2.0_f32, palette::TEXT))
                    } else {
                        btn
                    };
                    if ui.add(btn).clicked() {
                        cue.color = Some(rgb);
                    }
                }
                ui.add_space(theme::size::SPACE_SMALL);
                if cue.color.is_some() && ui.small_button("None").clicked() {
                    cue.color = None;
                }
            });

            // Notes füllen den Rest der Spalte (rechts in Basics).
            let r = ui.add(
                egui::TextEdit::multiline(&mut cue.notes)
                    .hint_text("Notes…")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            app.text_field_ids.insert(r.id);
        });
    });
}
