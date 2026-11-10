//! Broken-Cue-Erkennung („broken cue" – rotes ✕ in der Status-Spalte +
//! Warnungs-Zähler im Footer). Reine UI-Funktion: prüft einen Cue gegen den
//! App-Zustand (Dateisystem, Stages, Targets), ohne die Engine zu fragen.
//!
//! Das Ergebnis wird in [`ShowApp`](super::app::ShowApp) gecacht
//! (Neuberechnung max. alle 2 s – nicht jeden Frame das Dateisystem
//! abfragen).

use std::path::Path;

use crate::core::{Cue, CueList, CueType, HtmlSource};

use super::app::ShowApp;

/// Ermittelt alle „kaputten" Gründe eines Cues („broken cue"):
/// fehlende Datei, unbekannte Stage, fehlendes/unbekanntes Target,
/// ungültiges JSON. Gruppen-Kinder werden rekursiv mitgeprüft (Präfix mit
/// der Kinder-Nummer, kaputtes Kind → Gruppe kaputt).
///
/// Targets werden über alle Carts gesucht (ein Ziel in einem anderen Cart
/// würde zwar nicht feuern, wird aber nicht als „kaputt" gemeldet – keine
/// False-Positives).
pub fn cue_problems(cue: &Cue, app: &ShowApp) -> Vec<String> {
    let mut problems = Vec::new();
    match cue.cue_type {
        CueType::Fade => {
            // Kaputt, wenn: kein Ziel / Ziel existiert nicht / weder Volume noch
            // Opacity aktiv / nur ein Parameter aktiv, den das Ziel nicht hat.
            let target = cue.fade_target.and_then(|id| app.carts.find_cue_any(id));
            if cue.fade_target.is_none() {
                problems.push("no fade target".to_string());
            } else if target.is_none() {
                problems.push("fade target does not exist".to_string());
            }
            if cue.fade_volume.is_none() && cue.fade_opacity.is_none() {
                problems.push("neither volume nor opacity set".to_string());
            }
            if let Some(t) = target {
                let supports_volume = matches!(t.cue_type, CueType::Audio | CueType::Video);
                let supports_opacity = matches!(
                    t.cue_type,
                    CueType::Video
                        | CueType::SlideShow
                        | CueType::Image
                        | CueType::Html
                        | CueType::Text
                        | CueType::Group
                );
                if cue.fade_volume.is_some() && !supports_volume {
                    problems.push("target has no audio".to_string());
                }
                if cue.fade_opacity.is_some() && !supports_opacity {
                    problems.push("target has no opacity".to_string());
                }
            }
        }
        CueType::Audio | CueType::Video => {
            match &cue.target_path {
                None => problems.push("no target file".to_string()),
                Some(path) => check_file(path, &mut problems),
            }
            check_stage(cue, app, &mut problems);
        }
        CueType::Html => {
            match &cue.html_source {
                None => problems.push("no HTML source".to_string()),
                Some(HtmlSource::File(path)) => {
                    check_file(&path.display().to_string(), &mut problems);
                }
                // URL/Inline brauchen keine Datei.
                Some(HtmlSource::Url(_)) | Some(HtmlSource::Inline(_)) => {}
            }
            check_stage(cue, app, &mut problems);
            // Startdaten (Initial data) gegen die Felder des Templates prüfen.
            if !cue.data_json_text.trim().is_empty() {
                match serde_json::from_str::<serde_json::Value>(&cue.data_json_text) {
                    Ok(payload) => {
                        if let Some(Ok(schema)) = app.template_schema_of(cue.id) {
                            problems.extend(
                                schema
                                    .validate(&payload)
                                    .into_iter()
                                    .map(|p| format!("initial data: {p}")),
                            );
                        }
                    }
                    Err(_) => problems.push("initial data: invalid JSON".to_string()),
                }
            }
        }
        CueType::Image => {
            check_stage(cue, app, &mut problems);
            match cue.target_path.as_deref().filter(|p| !p.trim().is_empty()) {
                None => problems.push("no image file".to_string()),
                Some(path) => check_file(path, &mut problems),
            }
        }
        CueType::SlideShow => {
            check_stage(cue, app, &mut problems);
            match &cue.slideshow {
                None => problems.push("no slideshow defined".to_string()),
                Some(show) => {
                    for slide in &show.slides {
                        check_file(&slide.path.display().to_string(), &mut problems);
                    }
                }
            }
        }
        CueType::HtmlData => {
            match cue.data_target {
                None => problems.push("no target cue".to_string()),
                Some(id) => {
                    let ok = app
                        .carts
                        .find_cue_any(id)
                        .is_some_and(|c| c.cue_type == CueType::Html);
                    if !ok {
                        problems.push("target cue not found".to_string());
                    }
                }
            }
            // Daten gegen die Felder des Ziel-Templates prüfen (Pflichtfelder).
            if let (Some(Ok(schema)), Ok(payload)) = (
                cue.data_target.and_then(|id| app.template_schema_of(id)),
                serde_json::from_str::<serde_json::Value>(&cue.data_json_text),
            ) {
                problems.extend(schema.validate(&payload));
            }
            if !cue.data_json_text.trim().is_empty()
                && serde_json::from_str::<serde_json::Value>(&cue.data_json_text).is_err()
            {
                problems.push("invalid JSON payload".to_string());
            }
        }
        CueType::Text => {
            if cue.text_content.is_none() {
                problems.push("no text configured".to_string());
            }
            check_stage(cue, app, &mut problems);
        }
        CueType::Network => match &cue.network {
            None => problems.push("no network message configured".to_string()),
            Some(n) => {
                if n.validate().is_err() {
                    problems.push("network target missing host or port".to_string());
                }
            }
        },
        CueType::Midi => match &cue.midi {
            None => problems.push("no MIDI message configured".to_string()),
            Some(m) => {
                if m.validate().is_err() {
                    problems.push("MIDI channel/data out of range".to_string());
                }
            }
        },
        CueType::Timecode => {
            if cue.tc_trigger.is_none() {
                problems.push("no timecode trigger configured".to_string());
            }
        }
        CueType::Script => {
            if cue.script.trim().is_empty() {
                problems.push("empty script".to_string());
            }
        }
        CueType::Companion => match &cue.companion {
            None => problems.push("no companion trigger configured".to_string()),
            Some(c) => {
                if c.validate().is_err() {
                    problems.push("companion host/port invalid".to_string());
                }
            }
        },
        CueType::Light => match &cue.light {
            None => problems.push("no DMX values configured".to_string()),
            Some(l) => {
                if l.validate().is_err() {
                    problems.push("DMX universe/channels invalid".to_string());
                }
            }
        },
        CueType::Control => {
            // Ziel-Cue-Nummer prüfen (Aktionen ohne Ziel gelten für alle /
            // den Cue selbst und sind nie „kaputt").
            if let Some(target) = &cue.control_target {
                if !number_exists(app, target) {
                    problems.push(format!("target cue not found: {target}"));
                }
            }
        }
        // Fade/Stop/Script/Group ohne Kinder-Check: nichts zu prüfen.
        _ => {}
    }
    // Gruppen: Kinder mitprüfen (Präfix mit Kinder-Nummer).
    for child in &cue.children {
        for problem in cue_problems(child, app) {
            problems.push(format!("{}: {problem}", child.number));
        }
    }
    problems
}

/// Prüft eine Datei (URLs/Streams werden nicht geprüft – kein lokaler Pfad).
fn check_file(path: &str, problems: &mut Vec<String>) {
    if path.contains("://") {
        return;
    }
    if !Path::new(path).exists() {
        problems.push(format!("file not found: {path}"));
    }
}

/// Hinweise, die den Cue nicht kaputt machen: Er startet, bleibt aber unsichtbar.
pub fn cue_warnings(cue: &Cue, app: &ShowApp) -> Vec<String> {
    let visual = matches!(
        cue.cue_type,
        CueType::Video | CueType::Html | CueType::SlideShow | CueType::Image | CueType::Text
    );
    let Some(stage) = cue.stage.as_deref().filter(|_| visual) else {
        return Vec::new();
    };
    match app.stages.find(stage) {
        Some(found) if !found.regions.iter().any(|r| r.route.is_some()) => {
            vec![format!("stage \"{stage}\" has no output route")]
        }
        _ => Vec::new(),
    }
}

/// Prüft die Stage eines Video-Cues (unbekannte Stage = broken).
fn check_stage(cue: &Cue, app: &ShowApp, problems: &mut Vec<String>) {
    if let Some(stage) = &cue.stage {
        if app.stages.find(stage).is_none() {
            problems.push(format!("unknown stage: {stage}"));
        }
    }
}

/// `true`, wenn irgendein Cue (in irgendeinem Cart, inkl. Gruppen-Kinder)
/// die Nummer `number` trägt.
fn number_exists(app: &ShowApp, number: &str) -> bool {
    (0..app.carts.cart_count()).any(|i| list_has_number(app.carts.cart(i), number))
}

/// Sucht eine Cue-Nummer in einer Liste (inkl. Gruppen-Kinder).
fn list_has_number(list: &CueList, number: &str) -> bool {
    list.cues()
        .iter()
        .any(|c| c.number == number || c.children.iter().any(|ch| ch.number == number))
}
