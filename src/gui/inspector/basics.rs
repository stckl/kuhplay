//! Tab „Basics" – dreispaltig, damit bei Default-Höhe des Inspectors alles
//! ohne Scrollen sichtbar ist (docs/UI.md §5.5):
//!
//! * Links: Q# / Name / Target (mit „…"-Button) / Dateistatus bzw.
//!   typ-spezifische Felder (Control: Action; Group: Mode/Kinder).
//! * Mitte: Pre Wait / Duration / Post Wait / Continue.
//! * Rechts: Type + Armed, Color (Swatches), Notes.

use egui::RichText;

use crate::core::{ContinueMode, ControlAction, CueType, GroupMode, CUE_COLORS};

use super::theme::palette;
use super::{icons, pick_media_file, theme, time_field};
use crate::gui::app::ShowApp;

/// Tab „Basics".
pub fn draw_basics(ui: &mut egui::Ui, app: &mut ShowApp) {
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

            // Target-Zeile: je nach Typ (QLab: Target prominent in Basics).
            theme::field_label(ui, "Target");
            match cue.cue_type {
                CueType::Audio | CueType::Video => {
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
                    ui.horizontal(|ui| {
                        if let Some(target) = &mut cue.control_target {
                            let r = ui
                                .text_edit_singleline(target)
                                .on_hover_text("Target cue number (empty = all)");
                            app.text_field_ids.insert(r.id);
                        } else if ui.small_button("add target").clicked() {
                            cue.control_target = Some(String::new());
                        }
                    });
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

            // Group: Mode + Kinder (QLab-Basics).
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
            });

            // QLab „Cue Color": Paletten-Swatches + Entfernen.
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
                        btn.stroke(egui::Stroke::new(2.0, palette::TEXT))
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

            // Notes füllen den Rest der Spalte (QLab: rechts in Basics).
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
