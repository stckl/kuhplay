//! Demo-Show für den ersten Start (aus `app.rs` ausgelagert, TODO.md
//! Phase 0b). Öffentlich, damit die GUI-Smoke-Tests (`tests/gui_smoke.rs`)
//! dieselbe Show headless rendern können.

use std::time::Duration;

use crate::core::{
    ControlAction, Cue, CueType, GroupMode, HtmlSource, LoopMode, Slide, SlideShow, SurfaceGeometry,
};

/// Demo-Show für den ersten Start.
pub fn demo_cues() -> Vec<Cue> {
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
