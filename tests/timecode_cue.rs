//! Timecode-Cue (QLab 5): feuert, wenn externer Timecode den Trigger erreicht.
//! Eingang z. B. OSC `/kuhplay/timecode "01:00:00:00" 25` → RemoteCommand →
//! CueList::receive_timecode feuert alle armed Standby-Cues mit Trigger ≤ tc.

use std::time::Duration;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, Fps, Timecode};
use kuhplay::engine::EngineCommand;
use kuhplay::remote::osc::osc_message_to_command;
use kuhplay::remote::RemoteCommand;

fn tc(h: u64, m: u64, s: u64, f: u64) -> Timecode {
    Timecode::from_hmsf(h, m, s, f, Fps::Fps25)
}

#[test]
fn timecode_trigger_fires_when_tc_reaches_it() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "at 10s", CueType::Timecode);
    cue.tc_trigger = Some(tc(0, 0, 10, 0));
    cue.duration = Duration::from_secs(2); // manuelles GO wäre Wait 2 s
    list.add(cue);
    // Disarmed cue darf nicht feuern.
    let mut disarmed = Cue::new("2", "disarmed", CueType::Timecode);
    disarmed.tc_trigger = Some(tc(0, 0, 5, 0));
    disarmed.is_armed = false;
    list.add(disarmed);

    // TC 9 s → noch nichts.
    let cmds = list.receive_timecode(tc(0, 0, 9, 0));
    assert!(cmds.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Standby);

    // TC 10 s → Cue 1 feuert (Wait-Kommando wegen duration), Cue 2 bleibt Standby.
    let cmds = list.receive_timecode(tc(0, 0, 10, 0));
    assert_eq!(cmds.len(), 1);
    assert!(
        matches!(&cmds[0], EngineCommand::Wait { duration, .. } if *duration == Duration::from_secs(2))
    );
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Standby,
        "disarmed feuert nicht"
    );
}

#[test]
fn timecode_cue_manual_go_is_wait_or_instant() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "tc", CueType::Timecode);
    cue.tc_trigger = Some(tc(0, 0, 10, 0));
    cue.duration = Duration::from_secs(3);
    list.add(cue);
    let outcome = list.go().unwrap();
    assert!(
        matches!(&outcome.commands[0], EngineCommand::Wait { duration, .. } if *duration == Duration::from_secs(3))
    );

    // Ohne duration → Sofort-Cue (kein Kommando).
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "tc0", CueType::Timecode);
    cue.tc_trigger = Some(tc(0, 0, 10, 0));
    list.add(cue);
    let outcome = list.go().unwrap();
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

#[test]
fn osc_timecode_address_parses_to_receive_timecode() {
    use rosc::OscType;
    let msg = rosc::OscMessage {
        addr: "/kuhplay/timecode".to_string(),
        args: vec![OscType::String("01:00:10:00".to_string()), OscType::Int(25)],
    };
    let cmd = osc_message_to_command(&msg, "/kuhplay").expect("OSC /timecode parst");
    match cmd {
        RemoteCommand::ReceiveTimecode { tc } => {
            assert_eq!(tc.to_duration(), Duration::from_secs(3600 + 10));
            assert_eq!(tc.fps, Fps::Fps25);
        }
        other => panic!("expected ReceiveTimecode, got {other:?}"),
    }
    // Ungültiger Timecode → None.
    let bad = rosc::OscMessage {
        addr: "/kuhplay/timecode".to_string(),
        args: vec![OscType::String("99:99:99:99".to_string())],
    };
    assert!(osc_message_to_command(&bad, "/kuhplay").is_none());
}

#[test]
fn timecode_parse_hmsf_variants() {
    assert_eq!(
        Timecode::parse_hmsf("01:02:03:12", Fps::Fps25),
        Some(tc(1, 2, 3, 12))
    );
    assert_eq!(
        Timecode::parse_hmsf("02:03:12", Fps::Fps25),
        Some(tc(0, 2, 3, 12))
    );
    assert!(
        Timecode::parse_hmsf("01:02:03:25", Fps::Fps25).is_none(),
        "Frame ≥ fps"
    );
    assert!(Timecode::parse_hmsf("kaputt", Fps::Fps25).is_none());
    assert_eq!(Fps::from_fps(24), Fps::Fps24);
    assert_eq!(Fps::from_fps(30), Fps::Fps30);
    assert_eq!(Fps::from_fps(99), Fps::Fps25);
}
