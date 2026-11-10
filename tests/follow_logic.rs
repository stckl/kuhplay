//! Regressionstests Review Block B (docs/TODO-review-2026-10-31.md):
//! * B6: Auto-Continue feuert nicht doppelt (Finished + PostWaitElapsed bei
//!   Post-Wait 0 → beide `Completed`)
//! * B7: Sofort-Cues (Memo/MIDI/…) mit AutoContinue führen die Kette synchron
//!   weiter, statt dass der Folge-Cue in Standby stehen bleibt

use std::time::Duration;

use kuhplay::core::{ContinueMode, Cue, CueList, CueStatus, CueType};

fn wait_cue(number: &str, secs: u64) -> Cue {
    let mut cue = Cue::new(number, "wait", CueType::Wait);
    cue.duration = Duration::from_secs(secs);
    cue
}

fn memo_cue(number: &str) -> Cue {
    Cue::new(number, "memo", CueType::Memo)
}

#[test]
fn b6_auto_continue_feuert_nur_bei_echter_statusaenderung() {
    let mut list = CueList::new();
    let mut c1 = wait_cue("1", 0);
    c1.continue_mode = ContinueMode::AutoContinue;
    c1.status = CueStatus::Playing;
    let id1 = c1.id;
    list.add(c1);
    list.add(wait_cue("2", 30));
    list.add(wait_cue("3", 30));

    // Erster Abschluss (Finished → Completed): Folge-Cue 2 feuert.
    let (res, cmds) = list.set_status_with_follow(id1, CueStatus::Completed);
    assert!(res.is_ok());
    assert!(!cmds.is_empty(), "erster Abschluss muss Cue 2 feuern");
    assert_eq!(list.cues()[1].status, CueStatus::Playing);
    assert_eq!(list.cues()[2].status, CueStatus::Standby);

    // Zweiter Event (PostWaitElapsed → Completed, Status bereits Completed):
    // darf NICHT erneut feuern (Review B6: früher lief Cue 3 los).
    let (res, cmds) = list.set_status_with_follow(id1, CueStatus::Completed);
    assert!(res.is_ok());
    assert!(
        cmds.is_empty(),
        "wiederholtes Completed darf nicht doppelt feuern"
    );
    assert_eq!(
        list.cues()[2].status,
        CueStatus::Standby,
        "Cue 3 bleibt Standby"
    );
}

#[test]
fn b6_auto_follow_feuert_nur_einmal_beim_start() {
    let mut list = CueList::new();
    let mut c1 = wait_cue("1", 30);
    c1.continue_mode = ContinueMode::AutoFollow;
    c1.status = CueStatus::Standby;
    let id1 = c1.id;
    list.add(c1);
    list.add(wait_cue("2", 30));
    list.add(wait_cue("3", 30));

    let (_, cmds) = list.set_status_with_follow(id1, CueStatus::Playing);
    assert!(!cmds.is_empty(), "Auto-Follow feuert Cue 2 beim Start");
    let (_, cmds) = list.set_status_with_follow(id1, CueStatus::Playing);
    assert!(
        cmds.is_empty(),
        "wiederholtes Playing darf nicht erneut feuern"
    );
    assert_eq!(list.cues()[2].status, CueStatus::Standby);
}

#[test]
fn b7_sofort_cue_mit_auto_continue_feuert_following_cue() {
    let mut list = CueList::new();
    let mut c1 = memo_cue("1");
    c1.continue_mode = ContinueMode::AutoContinue;
    list.add(c1);
    list.add(wait_cue("2", 30));
    list.add(wait_cue("3", 30));

    list.go().expect("GO");
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "Memo sofort fertig"
    );
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Playing,
        "Review B7: Folge-Cue muss feuern, nicht Standby bleiben"
    );
    assert_eq!(list.cues()[2].status, CueStatus::Standby);
}

#[test]
fn b7_kette_von_sofort_cues_lazy() {
    // Memo → Memo → Wait, beide Memos mit AutoContinue: Kette läuft bis zum Wait.
    let mut list = CueList::new();
    let mut c1 = memo_cue("1");
    c1.continue_mode = ContinueMode::AutoContinue;
    let mut c2 = memo_cue("2");
    c2.continue_mode = ContinueMode::AutoContinue;
    list.add(c1);
    list.add(c2);
    list.add(wait_cue("3", 30));

    list.go().expect("GO");
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
    assert_eq!(list.cues()[1].status, CueStatus::Completed);
    assert_eq!(list.cues()[2].status, CueStatus::Playing);
}

#[test]
fn b7_trigger_by_number_haelt_kette_ohne_playhead() {
    let mut list = CueList::new();
    list.add(wait_cue("1", 30));
    let mut c2 = memo_cue("2");
    c2.continue_mode = ContinueMode::AutoContinue;
    list.add(c2);
    list.add(wait_cue("3", 30));
    let cursor_before = list.cursor();

    let outcome = list.go_to_number("2");
    assert!(outcome.is_some());
    assert_eq!(list.cues()[1].status, CueStatus::Completed);
    assert_eq!(
        list.cues()[2].status,
        CueStatus::Playing,
        "Kette nach GoTo-Sofort-Cue"
    );
    assert_eq!(
        list.cursor(),
        cursor_before,
        "Playhead darf sich bei GoTo nicht bewegen"
    );
}

#[test]
fn b7_endlosketten_guard() {
    // 150 Memo-Cues, alle AutoContinue: Kette muss bei max. 100 stoppen,
    // ohne Panic/Endlosschleife.
    let mut list = CueList::new();
    for i in 0..150 {
        let mut c = memo_cue(&format!("{i}"));
        c.continue_mode = ContinueMode::AutoContinue;
        list.add(c);
    }
    list.go().expect("GO");
    let completed = list
        .cues()
        .iter()
        .filter(|c| c.status == CueStatus::Completed)
        .count();
    assert!(
        completed <= 101,
        "Guard greift: {completed} Completed-Cues (max 1 + 100 Kette)"
    );
}

// --- Review Block D (11b): GoTo/Timecode dispatchen Gruppen wie go() ---

use kuhplay::core::{GroupMode, Timecode};
use kuhplay::engine::EngineCommand as EC;

fn audio(number: &str) -> Cue {
    Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
}

#[test]
fn d11b_goto_auf_gruppe_feuert_kinder() {
    let mut list = CueList::new();
    list.add(Cue::new_group(
        "1",
        "grp",
        GroupMode::Timeline,
        vec![audio("1.1"), audio("1.2")],
    ));
    list.add(audio("2"));
    // Vorher: GoTo "1" setzte die Gruppe auf Completed, Kinder blieben Standby.
    let outcome = list.go_to_number("1").expect("GoTo 1");
    assert!(
        outcome
            .commands
            .iter()
            .filter(|c| matches!(c, EC::Play { .. }))
            .count()
            == 2,
        "beide Kinder müssen Play-Commands bekommen"
    );
    let group = &list.cues()[0];
    assert_eq!(group.status, CueStatus::Playing);
    assert!(
        group
            .children
            .iter()
            .all(|c| c.status == CueStatus::Playing),
        "Kinder sind Playing"
    );
}

#[test]
fn d11b_timecode_auf_gruppe_feuert_kinder() {
    let mut list = CueList::new();
    let mut group = Cue::new_group(
        "1",
        "grp",
        GroupMode::Timeline,
        vec![audio("1.1"), audio("1.2")],
    );
    group.tc_trigger = Some(Timecode::from_hmsf(0, 0, 10, 0, kuhplay::core::Fps::Fps25));
    list.add(group);
    // Timecode 10s erreicht den Trigger → Gruppe feuert Kinder (nicht nur Completed).
    let commands =
        list.receive_timecode(Timecode::from_hmsf(0, 0, 10, 0, kuhplay::core::Fps::Fps25));
    assert_eq!(
        commands
            .iter()
            .filter(|c| matches!(c, EC::Play { .. }))
            .count(),
        2
    );
    assert!(list.cues()[0]
        .children
        .iter()
        .all(|c| c.status == CueStatus::Playing));
}

#[test]
fn d11b_goto_auf_control_cue_fuehrt_aktion_aus() {
    use kuhplay::core::ControlAction;
    let mut list = CueList::new();
    list.add(audio("1"));
    let mut control = Cue::new("2", "panic-all", CueType::Control);
    control.control_action = ControlAction::Stop;
    list.add(control);
    list.go().unwrap(); // Cue 1 Playing
    let outcome = list.go_to_number("2").expect("GoTo 2");
    assert!(
        outcome.commands.iter().any(|c| matches!(c, EC::StopAll)),
        "Control-Cue muss StopAll feuern"
    );
}

// --- Review G0: Control-Cue-Zyklen dürfen nicht abstürzen (Stack Overflow) ---

use kuhplay::core::ControlAction;

fn control_start(number: &str, target: &str) -> Cue {
    let mut c = Cue::new(number, "ctl", CueType::Control);
    c.control_action = ControlAction::Start;
    c.control_target = Some(target.to_string());
    c
}

#[test]
fn g0_control_zwei_cue_zyklus_stürzt_nicht_ab() {
    let mut list = CueList::new();
    list.add(control_start("1", "2"));
    list.add(control_start("2", "1"));
    let _ = list.go().expect("GO");
    // Zyklus stoppt kontrolliert: beide Cues Completed, kein Absturz (pre-fix: Stack Overflow).
    let statuses: Vec<_> = list.cues().iter().map(|c| c.status).collect();
    assert_eq!(
        statuses,
        vec![CueStatus::Completed, CueStatus::Completed],
        "2er-Zyklus endet kontrolliert"
    );
}

#[test]
fn g0_control_selbststart_stürzt_nicht_ab() {
    let mut list = CueList::new();
    list.add(control_start("1", "1"));
    let _ = list.go().expect("GO");
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "Selbststart stoppt sofort"
    );
}

#[test]
fn g0_control_lange_kette_ueber_tiefen_guard() {
    // 40 Control-Cues, jeder startet den nächsten → Depth-Guard bei 32 → Error, kein Absturz.
    let mut list = CueList::new();
    for i in 0..40 {
        list.add(control_start(&format!("{i}"), &format!("{}", i + 1)));
    }
    let _ = list.go().expect("GO");
    let statuses: Vec<_> = list.cues().iter().map(|c| c.status).collect();
    assert!(
        statuses.contains(&CueStatus::Error) || statuses.iter().all(|s| *s == CueStatus::Completed),
        "Kette endet kontrolliert: {statuses:?}"
    );
}
