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

/// Feste Ortszeit (wie `ClockTrigger` sie rechnet) als SystemTime.
fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> std::time::SystemTime {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(y, mo, d, h, mi, s)
        .earliest()
        .expect("gültige Ortszeit")
        .into()
}

#[test]
fn clock_trigger_fires_once_when_its_time_is_reached() {
    let ct = ClockTrigger::at_date(2026, 11, 5, 20, 15, 0);
    let mut app = app_with_cue(vec![CueTrigger::Clock(ct)]);
    app.fire_clock_triggers_at(at(2026, 11, 5, 20, 14, 59));
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::Standby);
    app.fire_clock_triggers_at(at(2026, 11, 5, 20, 15, 0));
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "Clock feuert zur Uhrzeit"
    );
    // Wieder bereit machen: dieselbe Occurrence feuert nicht erneut.
    app.carts.active_mut().cues_mut()[0].status = CueStatus::Standby;
    app.fire_clock_triggers_at(at(2026, 11, 5, 20, 15, 1));
    app.fire_clock_triggers_at(at(2026, 11, 5, 20, 15, 2));
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Standby,
        "kein Doppel-Feuer"
    );
}

#[test]
fn clock_trigger_in_the_past_does_not_fire_when_show_is_opened() {
    // Termin war gestern: Show öffnen darf den Cue nicht nachträglich starten.
    let ct = ClockTrigger::at_date(2026, 11, 4, 20, 15, 0);
    let mut app = app_with_cue(vec![CueTrigger::Clock(ct)]);
    app.fire_clock_triggers_at(at(2026, 11, 5, 9, 0, 0));
    app.fire_clock_triggers_at(at(2026, 11, 5, 9, 0, 1));
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::Standby);
}

#[test]
fn daily_clock_trigger_fires_every_day() {
    let mut app = app_with_cue(vec![CueTrigger::Clock(ClockTrigger::daily(8, 0, 0))]);
    for day in [5, 6] {
        app.carts.active_mut().cues_mut()[0].status = CueStatus::Standby;
        app.fire_clock_triggers_at(at(2026, 11, day, 7, 59, 59));
        app.fire_clock_triggers_at(at(2026, 11, day, 8, 0, 0));
        assert_eq!(
            app.carts.active().cues()[0].status,
            CueStatus::Playing,
            "täglich 8:00 am {day}."
        );
    }
}

#[test]
fn osc_trigger_fires_matching_cue() {
    let mut app = app_with_cue(vec![CueTrigger::Osc("/kuhplay/cue/5".into())]);
    app.handle_remote_command(RemoteCommand::OscAddress {
        address: "/kuhplay/cue/6".into(),
    });
    assert_eq!(app.carts.active().cues()[0].status, CueStatus::Standby);
    app.handle_remote_command(RemoteCommand::OscAddress {
        address: "/kuhplay/cue/5".into(),
    });
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "OSC-Trigger feuert"
    );
}

#[test]
fn trigger_fires_cue_without_number() {
    let mut app = app_with_cue(vec![CueTrigger::Midi {
        channel: 1,
        note: 60,
    }]);
    app.carts.active_mut().cues_mut()[0].number.clear();
    app.handle_remote_command(RemoteCommand::MidiNote {
        channel: 1,
        note: 60,
    });
    assert_eq!(
        app.carts.active().cues()[0].status,
        CueStatus::Playing,
        "Cue ohne Nummer feuert per Trigger"
    );
}

#[test]
fn trigger_fires_cue_in_inactive_cart() {
    let mut app = app_with_cue(vec![CueTrigger::Midi {
        channel: 1,
        note: 61,
    }]);
    // Cue liegt in Cart 1, aktiv ist ein neuer zweiter Cart.
    app.carts.add_cart("Cart 2");
    app.carts.set_active(1).unwrap();
    app.handle_remote_command(RemoteCommand::MidiNote {
        channel: 1,
        note: 61,
    });
    assert_eq!(
        app.carts.cart(0).cues()[0].status,
        CueStatus::Playing,
        "Trigger feuert im eigenen Cart"
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

/// Uhrzeit erreicht = GO auf den Cue: Playhead springt hin und rückt weiter,
/// auch wenn der Cue schon gespielt hat.
#[test]
fn clock_trigger_acts_like_go_on_its_cue() {
    let mut app = app_with_cue(vec![CueTrigger::Clock(ClockTrigger::daily(8, 0, 0))]);
    let mut before = Cue::new("1", "davor", CueType::Wait);
    before.duration = Duration::from_millis(50);
    app.carts.active_mut().insert(0, before);
    app.carts
        .active_mut()
        .add(Cue::new("6", "danach", CueType::Wait));
    app.carts.active_mut().cues_mut()[1].status = CueStatus::Completed;
    app.fire_clock_triggers_at(at(2026, 11, 5, 7, 59, 59));
    app.fire_clock_triggers_at(at(2026, 11, 5, 8, 0, 0));
    let list = app.carts.active();
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Playing,
        "Cue mit Uhr-Trigger läuft"
    );
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Standby,
        "Cue davor bleibt"
    );
    assert_eq!(list.cursor(), 2, "Playhead rückt wie bei GO weiter");
}
