//! Cue-Trigger (Hotkey/OSC/MIDI/Timecode/Clock) – feuern unabhängig vom Playhead.

use std::time::Duration;

use kuhplay::core::clock_trigger::{ClockTrigger, SAT};
use kuhplay::core::{Cue, CueStatus, CueTrigger, CueType, Fps, Timecode};
use kuhplay::engine::spawn_mock_engine;
use kuhplay::gui::ShowApp;
use kuhplay::remote::RemoteCommand;

fn app_with_cue(triggers: Vec<CueTrigger>) -> ShowApp {
    let mut app = ShowApp::new(spawn_mock_engine());
    app.carts.active_mut().cues_mut().clear();
    let mut cue = Cue::new("5", "getriggert", CueType::Wait);
    cue.duration = Duration::from_millis(50);
    cue.triggers = triggers;
    app.carts.active_mut().add(cue);
    app
}

#[test]
fn clock_trigger_fires_once_per_occurrence() {
    // One-shot in der Vergangenheit → feuert genau einmal.
    let ct = ClockTrigger::at_date(2020, 1, 1, 8, 0, 0);
    let mut app = app_with_cue(vec![CueTrigger::Clock(ct)]);
    app.fire_clock_triggers();
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "Clock feuert"
    );
    app.fire_clock_triggers();
    app.fire_clock_triggers();
    // weiterhin Playing (nicht doppelt neu gestartet → kein Reset auf Standby)
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "kein Doppel-Feuer"
    );
}

#[test]
fn clock_trigger_weekday_saturday() {
    // heute egal; Samstag 23:59 in der Zukunft darf nicht feuern.
    let now_days = {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86_400;
        d as i64
    };
    let ct = ClockTrigger {
        date: None,
        time: (23, 59, 0),
        weekdays: SAT,
    };
    let next =
        ct.next_after(std::time::UNIX_EPOCH + Duration::from_secs((now_days * 86_400) as u64));
    assert!(next.is_some());
    let mut app = app_with_cue(vec![CueTrigger::Clock(ct)]);
    app.fire_clock_triggers();
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Standby,
        "zukünftiger Samstag feuert nicht"
    );
}

#[test]
fn midi_trigger_fires_matching_cue() {
    let mut app = app_with_cue(vec![CueTrigger::Midi {
        channel: 2,
        note: 64,
    }]);
    app.handle_remote_command(RemoteCommand::MidiNote {
        channel: 2,
        note: 64,
    });
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "MIDI-Trigger feuert"
    );
    // andere Note feuert nicht:
    let mut app2 = app_with_cue(vec![CueTrigger::Midi {
        channel: 2,
        note: 64,
    }]);
    app2.handle_remote_command(RemoteCommand::MidiNote {
        channel: 2,
        note: 65,
    });
    assert_eq!(
        app2.carts.active().cues()[0].status,
        CueStatus::Standby,
        "falsche Note feuert nicht"
    );
}

#[test]
fn timecode_trigger_fires_at_threshold() {
    let trig = Timecode::from_duration(Duration::from_secs(10), Fps::Fps25);
    let mut app = app_with_cue(vec![CueTrigger::Timecode(trig)]);
    app.receive_timecode(Timecode::from_duration(Duration::from_secs(5), Fps::Fps25));
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Standby,
        "vor Schwelle nicht feuern"
    );
    app.receive_timecode(Timecode::from_duration(Duration::from_secs(10), Fps::Fps25));
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "bei Schwelle feuern"
    );
}

#[test]
fn triggers_serialize_roundtrip() {
    let cue = Cue {
        triggers: vec![
            CueTrigger::Hotkey("J".into()),
            CueTrigger::Midi {
                channel: 1,
                note: 36,
            },
            CueTrigger::Clock(ClockTrigger::daily(7, 30, 0)),
        ],
        ..Cue::new("1", "t", CueType::Wait)
    };
    let json = serde_json::to_string(&cue).unwrap();
    let back: Cue = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.triggers, cue.triggers,
        "Trigger über Show-Datei persistiert"
    );
}
