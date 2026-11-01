//! Audition-Fenster („Audition Window"): eigenes Fenster, das das
//! Bild des gerade laufenden Video-/HTML5-/Diashow-Cues zeigt – unabhängig
//! von Stages/Outputs (Vorschau im Audition Mode oder parallel zur Show).
//!
//! Das Fenster malt das neueste Frame aus dem Engine-Frame-Store
//! (`EngineHandle::latest_frame`), skaliert seitenrichtig.

use egui::{Color32, ColorImage, Context, RichText, TextureOptions};

use crate::core::{Cue, CueStatus, CueType};

use super::app::ShowApp;
use super::theme;
use super::viewport_window::{self, WindowSpec};

/// Zeichnet das Audition-Fenster, solange `app.ui.audition_window_open`.
pub fn draw(ctx: &Context, app: &mut ShowApp) {
    if !app.ui.audition_window_open {
        return;
    }
    let mut open = true;
    viewport_window::draw(
        ctx,
        &WindowSpec::new(
            "kuhplay-audition",
            "Audition",
            [480.0, 300.0],
            [240.0, 160.0],
        )
        .show_mode_badge(app.show_mode),
        &mut open,
        app,
        |ctx, ui, app| {
            // Ersten laufenden visuellen Cue über alle Carts suchen.
            let playing = find_playing_visual_cue(app);
            ui.horizontal(|ui| match &playing {
                Some((label, status, _)) => {
                    ui.label(RichText::new(label).strong());
                    ui.label(
                        RichText::new(status.label())
                            .small()
                            .color(theme::status_color(*status)),
                    );
                }
                None => {
                    ui.label(
                        RichText::new("No video cue playing").color(theme::palette::TEXT_FAINT),
                    );
                }
            });
            ui.separator();

            let Some((_label, _status, cue_id)) = playing else {
                return;
            };
            let frame = app.engine.latest_frame(cue_id);
            let Some(frame) = frame else {
                ui.label(RichText::new("waiting for frame…").color(theme::palette::TEXT_FAINT));
                return;
            };
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            let tex = ctx.load_texture(
                format!("kuhplay-audition-{cue_id}"),
                image,
                TextureOptions::LINEAR,
            );
            // Seitenrichtig in den verfügbaren Bereich einpassen.
            let avail = ui.available_size();
            let aspect = frame.width as f32 / frame.height.max(1) as f32;
            let size = if avail.x / avail.y > aspect {
                egui::vec2(avail.y * aspect, avail.y)
            } else {
                egui::vec2(avail.x, avail.x / aspect)
            };
            let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
            ui.painter().image(
                tex.id(),
                rect.shrink(1.0),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        },
    );
    if !open {
        app.ui.audition_window_open = false;
    }
}

/// Sucht den ersten laufenden visuellen Cue (Video/HTML5/Diashow) über alle
/// Carts inkl. Gruppen-Kinder. Rückgabe: (Anzeige-Label, Status, Cue-ID).
fn find_playing_visual_cue(app: &ShowApp) -> Option<(String, CueStatus, uuid::Uuid)> {
    for cart_index in 0..app.carts.cart_count() {
        let cart = app.carts.cart(cart_index);
        for cue in cart.cues() {
            if let Some(found) = find_in_cue(cue) {
                return Some(found);
            }
        }
    }
    None
}

/// Rekursive Suche im Cue + Kindern.
fn find_in_cue(cue: &Cue) -> Option<(String, CueStatus, uuid::Uuid)> {
    if cue.status.is_active()
        && matches!(
            cue.cue_type,
            CueType::Video | CueType::Html | CueType::SlideShow
        )
    {
        return Some((
            format!("Q{} • {}", cue.number, cue.name),
            cue.status,
            cue.id,
        ));
    }
    cue.children.iter().find_map(find_in_cue)
}
