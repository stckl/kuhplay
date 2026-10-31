//! Inspector-Tab „Text" (Text-Cue): Text-Editor mit Schriftgröße,
//! Text-/Hintergrundfarbe und Ausrichtung. Der Cue rendert den Inhalt als
//! Inline-HTML über die HTML5-Pipeline (siehe `TextContent::to_html`).

use egui::RichText;

use crate::core::{CueType, TextAlign, TextContent, TextVAlign};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Text" (Text-Cue): mehrzeiliges Textfeld + Styling-Optionen.
pub fn draw_text(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Text {
        ui.label(RichText::new("Text only for text cues").weak());
        return;
    }
    // Text-Inhalt anlegen, falls noch keiner existiert (z. B. Typ per
    // Basics-Combo umgestellt).
    if cue.text_content.is_none() {
        cue.text_content = Some(TextContent::default());
    }
    let Some(text) = &mut cue.text_content else {
        return;
    };

    theme::form_grid("inspector_text_grid").show(ui, |ui| {
        theme::field_label(ui, "Font Size");
        ui.add(
            egui::DragValue::new(&mut text.font_size)
                .speed(1.0)
                .range(4.0..=400.0)
                .suffix(" px"),
        );
        ui.end_row();

        theme::field_label(ui, "Text Color");
        color_edit(ui, &mut text.color);
        ui.end_row();

        theme::field_label(ui, "Background");
        ui.horizontal(|ui| {
            let mut has_bg = text.bg_color.is_some();
            if ui.checkbox(&mut has_bg, "opaque").changed() {
                text.bg_color = if has_bg { Some([0, 0, 0]) } else { None };
            }
            if let Some(bg) = &mut text.bg_color {
                color_edit(ui, bg);
            }
        });
        ui.end_row();

        theme::field_label(ui, "Align");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("text_align_combo")
                .selected_text(text.align.label())
                .show_ui(ui, |ui| {
                    for a in TextAlign::ALL {
                        ui.selectable_value(&mut text.align, a, a.label());
                    }
                });
            egui::ComboBox::from_id_salt("text_valign_combo")
                .selected_text(text.valign.label())
                .show_ui(ui, |ui| {
                    for v in TextVAlign::ALL {
                        ui.selectable_value(&mut text.valign, v, v.label());
                    }
                });
        });
        ui.end_row();
    });

    ui.add_space(theme::size::SPACE_MEDIUM);
    theme::field_label(ui, "Text");
    let mut body = text.text.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut body)
            .desired_width(f32::INFINITY)
            .desired_rows(6)
            .font(egui::TextStyle::Monospace),
    );
    app.text_field_ids.insert(response.id);
    if response.changed() {
        text.text = body;
    }
}

/// Kleiner RGB-Farbeditor (Farb-Button mit Picker-Popup) für Text- und
/// Hintergrundfarbe.
fn color_edit(ui: &mut egui::Ui, rgb: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        let mut c = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        if egui::color_picker::color_edit_button_srgba(
            ui,
            &mut c,
            egui::color_picker::Alpha::Opaque,
        )
        .changed()
        {
            let (r, g, b, _) = c.to_tuple();
            *rgb = [r, g, b];
        }
        ui.label(
            RichText::new(format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
                .monospace()
                .color(theme::palette::TEXT_DIM),
        );
    });
}
