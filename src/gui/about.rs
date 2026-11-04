//! „About KuhPlay": Inhalt des eigenen About-Fensters und Texte für das
//! native About-Panel (macOS-Menü).

use egui::RichText;

use super::theme;

/// Untertitel unter dem Namen.
pub const TAGLINE: &str = "Show control & cue playback for live events";

/// Copyright-Zeile.
pub const COPYRIGHT: &str = "© 2026 Michael Stöckle";

/// Projektseite (GitHub).
pub fn website() -> String {
    format!(
        "https://github.com/{}/{}",
        crate::update::REPO_OWNER,
        crate::update::REPO_NAME
    )
}

/// Was KuhPlay kann (eine Zeile je Bereich).
const FEATURES: [(&str, &str); 5] = [
    (
        "Media",
        "Audio, video, slideshows, text and HTML5 templates",
    ),
    (
        "Outputs",
        "Stages with regions, edge blending, DeckLink SDI",
    ),
    (
        "Sync",
        "LTC/MTC timecode chase, clock and timecode triggers",
    ),
    ("Remote", "OSC, MIDI, web remote and Bitfocus Companion"),
    ("Show", "Cue lists, carts, groups, fades and auto-continue"),
];

/// Bausteine von Dritten mit Lizenz.
const COMPONENTS: [(&str, &str); 6] = [
    ("egui / eframe", "MIT"),
    ("Chromium Embedded Framework", "BSD-3-Clause"),
    ("ffmpeg / ffprobe", "LGPL v2.1+"),
    ("cpal / symphonia", "Apache-2.0 / MPL-2.0"),
    ("Phosphor Icons", "MIT"),
    ("Blackmagic DeckLink SDK", "Blackmagic EULA"),
];

/// Credits für das native About-Panel (macOS).
pub fn credits() -> String {
    let mut text = String::from("Built with:\n");
    for (name, license) in COMPONENTS {
        text.push_str(&format!("{name} ({license})\n"));
    }
    text.push_str("\nSee LICENSES.md for details.");
    text
}

/// Programm-Icon als Textur (einmal geladen, im egui-Kontext gecacht).
fn icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("kuhplay_about_icon");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let icon = super::app_icon()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let tex = ctx.load_texture("kuhplay_about_icon", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Zeichnet den Inhalt; `true`, wenn „Close" geklickt wurde.
pub fn draw(ui: &mut egui::Ui) -> bool {
    let mut close = false;
    ui.vertical_centered(|ui| {
        if let Some(tex) = icon_texture(ui.ctx()) {
            let size = tex.size_vec2();
            let height = 150.0;
            ui.add(
                egui::Image::new(&tex)
                    .fit_to_exact_size(egui::vec2(size.x / size.y * height, height)),
            );
        }
        ui.label(RichText::new("KuhPlay").size(26.0).strong());
        ui.label(RichText::new(TAGLINE).color(theme::palette::TEXT_DIM));
        ui.label(RichText::new(crate::update::build_info()).small());
    });
    ui.add_space(theme::size::SPACE_SMALL);
    ui.separator();

    egui::Grid::new("about_features")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for (area, text) in FEATURES {
                ui.label(RichText::new(area).strong());
                ui.label(text);
                ui.end_row();
            }
        });
    ui.separator();

    ui.label(RichText::new("Built with").strong());
    egui::Grid::new("about_components")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for (name, license) in COMPONENTS {
                ui.label(RichText::new(name).small());
                ui.label(
                    RichText::new(license)
                        .small()
                        .color(theme::palette::TEXT_DIM),
                );
                ui.end_row();
            }
        });
    ui.label(
        RichText::new("License texts: LICENSES.md, LICENSE-ffmpeg.txt (in the package)")
            .small()
            .color(theme::palette::TEXT_DIM),
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label(RichText::new(COPYRIGHT).small());
        ui.label(
            RichText::new("· MIT")
                .small()
                .color(theme::palette::TEXT_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
            ui.hyperlink_to("GitHub", website())
                .on_hover_text("Releases and updates");
        });
    });
    close
}
