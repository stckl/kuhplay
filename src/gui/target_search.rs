//! File Target Search („Tools → File Target Search"): Fenster, das
//! ALLE Datei-Ziele der Show über alle Carts auflistet (Audio/Video-Pfade,
//! HTML-Dateien, Diashow-Slides) – mit Filter und Klick zum Sprung auf den
//! Cue. Nützlich beim Umzug einer Show auf einen anderen Rechner (fehlende
//! Pfade finden) und um zu sehen, welcher Cue welche Datei benutzt.
//!
//! Reiner Lesezugriff – Umbenennen/Neu-Zielen von Pfaden ist offen
//! (siehe docs/BACKLOG.md).

use egui::{Context, RichText};
use uuid::Uuid;

use crate::core::{CartManager, Cue, CueType};

use super::app::ShowApp;
use super::theme;
use super::viewport_window::{self, WindowSpec};

/// Ein Datei-Ziel eines Cues (Zeile in der Suchliste).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    /// Index des Carts (für den Sprung).
    pub cart_index: usize,
    /// Name des Carts.
    pub cart_name: String,
    /// Cue-ID (für Selektion nach dem Sprung).
    pub cue_id: Uuid,
    /// Cue-Nummer (Anzeige).
    pub number: String,
    /// Cue-Name (Anzeige).
    pub cue_name: String,
    /// Datei-Pfad.
    pub path: String,
    /// Ziel-Art (Audio/Video/HTML/Slide).
    pub kind: &'static str,
}

/// Sammelt alle Datei-Ziele über alle Carts (inkl. Gruppen-Kinder, rekursiv).
pub fn collect_file_targets(carts: &CartManager) -> Vec<FileTarget> {
    let mut out = Vec::new();
    for cart_index in 0..carts.cart_count() {
        let cart = carts.cart(cart_index);
        let cart_name = cart.name().to_string();
        for cue in cart.cues() {
            collect_from_cue(cue, cart_index, &cart_name, &mut out);
        }
    }
    out
}

/// Rekursive Hilfsfunktion: Ziele eines Cues + seiner Kinder sammeln.
fn collect_from_cue(cue: &Cue, cart_index: usize, cart_name: &str, out: &mut Vec<FileTarget>) {
    let base = |kind: &'static str, path: &str| FileTarget {
        cart_index,
        cart_name: cart_name.to_string(),
        cue_id: cue.id,
        number: cue.number.clone(),
        cue_name: cue.name.clone(),
        path: path.to_string(),
        kind,
    };
    match cue.cue_type {
        CueType::Audio | CueType::Video => {
            if let Some(path) = &cue.target_path {
                out.push(base(
                    if cue.cue_type == CueType::Audio {
                        "Audio"
                    } else {
                        "Video"
                    },
                    path,
                ));
            }
        }
        CueType::Html => {
            if let Some(crate::core::HtmlSource::File(path)) = &cue.html_source {
                out.push(base("HTML", &path.display().to_string()));
            }
        }
        CueType::SlideShow => {
            if let Some(show) = &cue.slideshow {
                for slide in &show.slides {
                    out.push(base("Slide", &slide.path.display().to_string()));
                }
            }
        }
        _ => {}
    }
    for child in &cue.children {
        collect_from_cue(child, cart_index, cart_name, out);
    }
}

/// Filter: case-insensitive Substring auf Pfad, Cue-Name oder Q#.
pub fn matches(target: &FileTarget, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    target.path.to_lowercase().contains(&q)
        || target.cue_name.to_lowercase().contains(&q)
        || target.number.to_lowercase().contains(&q)
}

/// Zeichnet das Fenster „File Target Search" (echtes OS-Fenster, in
/// Headless-Tests Overlay-Fallback), solange `app.ui.target_search_open`.
pub fn draw(ctx: &Context, app: &mut ShowApp) {
    if !app.ui.target_search_open {
        return;
    }
    let mut open = true;
    viewport_window::draw(
        ctx,
        &WindowSpec::new(
            "kuhplay-target-search",
            "File Target Search",
            [640.0, 420.0],
            [420.0, 240.0],
        ),
        &mut open,
        app,
        |_ctx, ui, app| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Filter").color(theme::palette::TEXT_DIM));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut app.ui.target_search_query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Path, cue name or Q# …"),
                );
                app.text_field_ids.insert(r.id);
            });
            ui.separator();

            let targets = collect_file_targets(&app.carts);
            let shown: Vec<&FileTarget> = targets
                .iter()
                .filter(|t| matches(t, &app.ui.target_search_query))
                .collect();
            ui.label(
                RichText::new(format!("{} of {} file targets", shown.len(), targets.len()))
                    .small()
                    .color(theme::palette::TEXT_DIM),
            );
            egui::ScrollArea::vertical().show(ui, |ui| {
                for target in shown {
                    let label = format!(
                        "{}  Q{} {}  [{}]  {}",
                        target.cart_name, target.number, target.cue_name, target.kind, target.path
                    );
                    let response = ui.add(
                        egui::Label::new(RichText::new(label).monospace())
                            .sense(egui::Sense::click()),
                    );
                    if response.clicked() {
                        jump_to_target(app, target);
                    }
                    if response.hovered() {
                        response.highlight();
                    }
                }
                if targets.is_empty() {
                    ui.label(
                        RichText::new("No file targets in this show")
                            .color(theme::palette::TEXT_FAINT),
                    );
                }
            });
        },
    );
    if !open {
        app.ui.target_search_open = false;
    }
}

/// Springt zum Cue eines Datei-Ziels: Cart aktivieren, Cue selektieren,
/// Playhead setzen (Top-Level) – „jump to cue".
fn jump_to_target(app: &mut ShowApp, target: &FileTarget) {
    if app.carts.active_index() != target.cart_index {
        let _ = app.carts.set_active(target.cart_index);
    }
    let list = app.carts.active_mut();
    list.select_id(Some(target.cue_id));
    if let Some(index) = list
        .cues()
        .iter()
        .position(|c| c.id == target.cue_id || c.children.iter().any(|k| k.id == target.cue_id))
    {
        list.set_cursor(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CartManager, Cue, CueList, CueType, HtmlSource, Slide, SlideShow};
    use std::path::PathBuf;

    fn carts_with_targets() -> CartManager {
        let mut carts = CartManager::new();
        let mut list = CueList::new();
        list.add(Cue::new("1", "audio", CueType::Audio).with_target("/media/a.mp3"));
        let mut html = Cue::new("2", "web", CueType::Html);
        html.html_source = Some(HtmlSource::file("/media/page.html"));
        list.add(html);
        let mut show = Cue::new("3", "slides", CueType::SlideShow);
        show.slideshow = Some(SlideShow {
            slides: vec![
                Slide {
                    path: PathBuf::from("/img/1.png"),
                    hold: std::time::Duration::from_secs(2),
                },
                Slide {
                    path: PathBuf::from("/img/2.png"),
                    hold: std::time::Duration::from_secs(2),
                },
            ],
            ..Default::default()
        });
        list.add(show);
        // Gruppen-Kind mit Ziel.
        let mut child = Cue::new("4.1", "kind", CueType::Video).with_target("/media/v.mp4");
        let group = Cue::new_group("4", "g", crate::core::GroupMode::Simultaneous, vec![child]);
        child = group.children[0].clone();
        let mut with_group = CueList::new();
        with_group.add(group);
        let _ = child;
        // Cart 0 (Default) bekommt die Haupt-Liste, Cart 1 die Gruppe.
        for cue in list.cues() {
            carts.active_mut().add(cue.clone());
        }
        carts.add_cart("Extra");
        for cue in with_group.cues() {
            carts.active_mut().add(cue.clone());
        }
        carts
    }

    #[test]
    fn collects_all_file_targets_across_carts_and_groups() {
        let carts = carts_with_targets();
        let targets = collect_file_targets(&carts);
        let paths: Vec<&str> = targets.iter().map(|t| t.path.as_str()).collect();
        assert!(paths.contains(&"/media/a.mp3"));
        assert!(paths.contains(&"/media/page.html"));
        assert!(paths.contains(&"/img/1.png"));
        assert!(paths.contains(&"/img/2.png"));
        assert!(paths.contains(&"/media/v.mp4"), "Gruppen-Kind mitzählen");
        // Cart-Zuordnung stimmt.
        let video = targets.iter().find(|t| t.path == "/media/v.mp4").unwrap();
        assert_eq!(video.cart_index, 1);
        assert_eq!(video.cart_name, "Extra");
        assert_eq!(video.number, "4.1");
    }

    #[test]
    fn filter_matches_path_name_and_number_case_insensitive() {
        let carts = carts_with_targets();
        let targets = collect_file_targets(&carts);
        let t = &targets[0];
        assert!(matches(t, ""));
        assert!(matches(t, "MEDIA"));
        assert!(matches(t, &t.cue_name.to_uppercase()));
        assert!(matches(t, &t.number));
        assert!(!matches(t, "zzz-nicht-drin"));
    }
}
