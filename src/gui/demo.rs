//! Demo-Show für den ersten Start (aus `app.rs` ausgelagert, docs/TODO-offen.md
//! Phase 0b). Öffentlich, damit die GUI-Smoke-Tests (`tests/gui_smoke.rs`)
//! dieselbe Show headless rendern können.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::{
    cue_type_for_file, ControlAction, Cue, CueType, GroupMode, HtmlSource, LoopMode, Slide,
    SlideShow, SurfaceGeometry,
};

/// Baut eine Show aus den Dateien direkt in `dir`: je Video, Ton und HTML-Seite
/// ein Cue, alle Bilder zusammen eine Diashow, am Ende ein Stop-Cue.
/// Leer, wenn der Ordner nichts Abspielbares enthält.
pub fn cues_from_folder(dir: &Path) -> Vec<Cue> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    let file_name = |path: &Path| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let is_html = |path: &Path| {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
    };
    let of_type = |wanted: CueType| {
        files
            .iter()
            .filter(move |path| cue_type_for_file(path) == Some(wanted))
    };

    let mut cues = Vec::new();
    let number = |cues: &Vec<Cue>| (cues.len() + 1).to_string();
    for path in of_type(CueType::Video) {
        cues.push(
            Cue::new(number(&cues), file_name(path), CueType::Video)
                .with_target(path.to_string_lossy())
                .with_stage("Stage 1")
                .with_layer(1)
                .with_surface(SurfaceGeometry::fit()),
        );
    }
    for path in of_type(CueType::Audio) {
        cues.push(
            Cue::new(number(&cues), file_name(path), CueType::Audio)
                .with_target(path.to_string_lossy()),
        );
    }
    let slides: Vec<Slide> = of_type(CueType::Image)
        .filter_map(|path| Slide::new(path.clone(), Duration::from_secs(5)).ok())
        .collect();
    if let Ok(show) = SlideShow::new(slides, Duration::from_millis(500), true) {
        cues.push(
            Cue::new(number(&cues), "Images", CueType::SlideShow)
                .with_slideshow(show)
                .with_stage("Stage 1")
                .with_layer(1)
                .with_surface(SurfaceGeometry::fit()),
        );
    }
    for path in files.iter().filter(|path| is_html(path)) {
        cues.push(
            Cue::new(number(&cues), file_name(path), CueType::Html)
                .with_html_source(HtmlSource::file(path))
                .with_stage("Stage 1")
                .with_layer(2)
                .with_surface(SurfaceGeometry::fit()),
        );
    }
    if !cues.is_empty() {
        cues.push(Cue::new(number(&cues), "Blackout", CueType::Stop));
    }
    cues
}

/// Demo-Show für den ersten Start. Ist `KUHPLAY_MEDIA_DIR` gesetzt und enthält
/// der Ordner Medien, wird die Show aus seinen echten Dateien gebaut.
pub fn demo_cues() -> Vec<Cue> {
    if let Some(dir) = std::env::var_os("KUHPLAY_MEDIA_DIR") {
        let cues = cues_from_folder(Path::new(&dir));
        if !cues.is_empty() {
            return cues;
        }
    }
    // Medien-Verzeichnis: Umgebungsvariable `KUHPLAY_MEDIA_DIR`, Default "/media".
    let media = std::env::var("KUHPLAY_MEDIA_DIR").unwrap_or_else(|_| "/media".to_string());
    let media = media.trim_end_matches('/');
    // Diashow-Verzeichnis: `KUHPLAY_SLIDES_DIR` oder `<media>/slides`.
    let slides = std::env::var("KUHPLAY_SLIDES_DIR").unwrap_or_else(|_| format!("{media}/slides"));
    let slides = slides.trim_end_matches('/');
    let media_path = |name: &str| format!("{media}/{name}");
    let slide_path = |name: &str| format!("{slides}/{name}");
    // Vollflächige Surface (Default: gesamtes Stage-Canvas).
    let full_surface = SurfaceGeometry::fit();

    // HTML5-Cue (Inline-Quelle, keine Datei nötig) – die Data-Cue "8" zielt auf ihn.
    let html = Cue::new("6", "Lower Third (HTML5)", CueType::Html)
        .with_html_source(HtmlSource::inline(
            "<html><body style=\"background:#181818;color:#fff;font-family:sans-serif;\
             display:flex;align-items:flex-end;padding:48px\">\
             <h1 style=\"font-size:64px;margin:0\">Welcome!</h1></body></html>",
        ))
        .with_stage("Stage 1")
        .with_layer(2)
        .with_surface(full_surface)
        .with_js_test_script("document.querySelector('h1').style.color = '#2ecc71';");
    let html_id = html.id;
    let timetable = Cue::new("12", "Timetable (HTML5)", CueType::Html)
        .with_html_source(HtmlSource::inline(include_str!(
            "../../assets/demo/timetable.html"
        )))
        .with_stage("Stage 1")
        .with_layer(3)
        .with_surface(full_surface);
    let timetable_id = timetable.id;
    vec![
        Cue::new("1", "House to Half", CueType::Fade)
            .with_fade_target(0.5)
            .with_duration(Duration::from_secs(3)),
        Cue::new("2", "Intro Video", CueType::Video)
            .with_target(media_path("intro.mp4"))
            .with_duration(Duration::from_secs(12))
            .with_stage("Stage 1")
            .with_layer(1)
            .with_surface(full_surface),
        Cue::new("3", "Welcome Jingle", CueType::Audio)
            .with_target(media_path("welcome.wav"))
            .with_pre_wait(Duration::from_secs(1)),
        Cue::new("4", "Walk-in Loop", CueType::Audio)
            .with_target(media_path("walk-in.mp3"))
            .with_loop(LoopMode::Infinite),
        Cue::new_group(
            "5",
            "Scene: Entrance",
            GroupMode::Timeline,
            vec![
                Cue::new("5.1", "Lights", CueType::Fade)
                    .with_fade_target(1.0)
                    .with_duration(Duration::from_secs(2)),
                Cue::new("5.2", "Spot Jingle", CueType::Audio)
                    .with_target(media_path("spot.wav"))
                    .with_pre_wait(Duration::from_secs(1))
                    .with_duration(Duration::from_secs(4)),
            ],
        ),
        html,
        Cue::new("7", "Photo Slideshow", CueType::SlideShow)
            .with_slideshow(
                SlideShow::new(
                    vec![
                        Slide::new(slide_path("01-welcome.jpg"), Duration::from_secs(4))
                            .expect("demo slide is valid"),
                        Slide::new(slide_path("02-artist.jpg"), Duration::from_secs(5))
                            .expect("demo slide is valid"),
                        Slide::new(slide_path("03-outro.jpg"), Duration::from_secs(4))
                            .expect("demo slide is valid"),
                    ],
                    Duration::from_millis(500),
                    true,
                )
                .expect("demo slideshow is valid"),
            )
            .with_stage("Stage 1")
            .with_layer(1)
            .with_surface(full_surface),
        Cue::new("8", "Score Update (JSON)", CueType::HtmlData)
            .with_data_target(html_id)
            .with_data_json(r#"{"score":{"home":2,"guest":1},"clock":"12:34"}"#),
        // Timetable-Template (assets/demo/timetable.html, als Inline eingebettet,
        // damit es ohne Datei-Pfad überall läuft). Daten kommen über Cue 13.
        timetable,
        Cue::new("9", "Next Slide", CueType::Control)
            .with_control(ControlAction::NextSlide)
            .with_control_target("7"),
        Cue::new("10", "Blackout", CueType::Stop),
        Cue::new("11", "Reset Show", CueType::Control).with_control(ControlAction::Reset),
        // Timetable-Daten (assets/demo/zeitplan.json) an den HTML5-Cue 12 pushen.
        Cue::new("13", "Timetable Data (JSON)", CueType::HtmlData)
            .with_data_target(timetable_id)
            .with_data_json(include_str!("../../assets/demo/zeitplan.json")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_from_folder_uses_the_real_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "b.mp4",
            "a.mov",
            "song.mp3",
            "one.jpg",
            "two.png",
            "clock.html",
            "x.pptx",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub.mp4")).unwrap();

        let cues = cues_from_folder(dir.path());
        let summary: Vec<(&str, CueType)> =
            cues.iter().map(|c| (c.name.as_str(), c.cue_type)).collect();
        assert_eq!(
            summary,
            vec![
                ("a.mov", CueType::Video),
                ("b.mp4", CueType::Video),
                ("song.mp3", CueType::Audio),
                ("Images", CueType::SlideShow),
                ("clock.html", CueType::Html),
                ("Blackout", CueType::Stop),
            ]
        );
        let numbers: Vec<&str> = cues.iter().map(|c| c.number.as_str()).collect();
        assert_eq!(numbers, ["1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn cues_from_empty_or_missing_folder_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(cues_from_folder(dir.path()).is_empty());
        assert!(cues_from_folder(&dir.path().join("gibt-es-nicht")).is_empty());
    }
}
