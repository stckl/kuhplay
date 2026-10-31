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
