//! Script-Cue (QLab 5): JavaScript in eigener WebView ausführen.
//! Mock-Engine: sofort Finished. Core: RunScript-Kommando, Empty-Script → Error.

use crossbeam_channel::unbounded;
use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine};

#[test]
fn script_cue_triggers_run_script_and_plays_until_finished() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "js", CueType::Script);
    cue.script = "console.log(1);".to_string();
    list.add(cue);

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    assert!(
        matches!(&outcome.commands[0], EngineCommand::RunScript { script, .. } if script == "console.log(1);"),
        "script cue must produce RunScript with its source"
    );
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
}

#[test]
fn empty_script_cue_goes_to_error() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty", CueType::Script));
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn mock_engine_finishes_script_immediately() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::RunScript {
            cue_id: id,
            script: "1+1".to_string(),
        })
        .unwrap();
    let events: Vec<_> = rx.try_iter().collect();
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::PostWaitElapsed { cue_id } if *cue_id == id)));
}

#[test]
fn script_serde_legacy_default() {
    let mut cue = Cue::new("1", "s", CueType::Script);
    cue.script = "x()".to_string();
    let json = serde_json::to_string(&cue).unwrap();
    let back: Cue = serde_json::from_str(&json).unwrap();
    assert_eq!(back.script, "x()");
    // Alte Show ohne script-Feld → leerer String.
    let old_json = json.replace(",\"script\":\"x()\"", "");
    let back_old: Cue = serde_json::from_str(&old_json).unwrap();
    assert!(back_old.script.is_empty());
}
