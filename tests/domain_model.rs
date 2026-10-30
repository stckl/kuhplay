//! Domain-Tests: Status-State-Machine, Group-Kaskadierung (simultan/sequenziell),
//! ID-Generierung, Reordering und GO-Stress – alles ohne Engine/Hardware.

use kuhplay::core::{Cue, CueList, CueStatus, CueType, GroupMode, ShowError};
use kuhplay::engine::EngineCommand;

fn audio(number: &str) -> Cue {
    Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
}

// ---------------------------------------------------------------------------
// State-Machine
// ---------------------------------------------------------------------------

#[test]
fn state_machine_accepts_full_lifecycle() {
    use CueStatus::*;
    // Standby -> PreWait -> Playing -> Paused -> Playing -> PostWait -> Completed
    assert!(Standby.can_transition_to(PreWait));
    assert!(PreWait.can_transition_to(Playing));
    assert!(Playing.can_transition_to(Paused));
    assert!(Paused.can_transition_to(Playing));
    assert!(Playing.can_transition_to(PostWait));
    assert!(PostWait.can_transition_to(Completed));
    assert!(Completed.can_transition_to(Standby));
    // Direkt-Start ohne Pre-Wait und Abschluss ohne Post-Wait.
    assert!(Standby.can_transition_to(Playing));
    assert!(Playing.can_transition_to(Completed));
    // Stop aus jeder aktiven Phase.
    assert!(PreWait.can_transition_to(Stopped));
    assert!(Playing.can_transition_to(Stopped));
    assert!(Paused.can_transition_to(Stopped));
    assert!(PostWait.can_transition_to(Stopped));
    assert!(Stopped.can_transition_to(Standby));
    // Fehler + Recovery
    assert!(Playing.can_transition_to(Error));
    assert!(Error.can_transition_to(Standby));
    // Sofort-Cues
    assert!(Standby.can_transition_to(Completed));
    // Operator-Override: Reset/Panic darf jederzeit nach Standby zwingen.
    assert!(Playing.can_transition_to(Standby));
    assert!(Paused.can_transition_to(Standby));
    assert!(PreWait.can_transition_to(Standby));
    assert!(PostWait.can_transition_to(Standby));
}

#[test]
fn state_machine_rejects_invalid_transitions() {
    use CueStatus::*;
    let invalid = [
        (Standby, Paused),
        (Standby, PostWait),
        (PreWait, Paused),
        (PreWait, Completed),
        (Completed, Playing),
        (Completed, Paused),
        (Completed, PreWait),
        (Paused, Completed),
        (Paused, PostWait),
        (Stopped, Playing),
        (Stopped, Completed),
        (Error, Playing),
        (Error, Completed),
    ];
    for (from, to) in invalid {
        assert!(
            !from.can_transition_to(to),
            "{from:?} -> {to:?} must be rejected"
        );
    }
}

#[test]
fn cue_list_set_status_enforces_state_machine() {
    let mut list = CueList::new();
    list.add(audio("1"));
    let id = list.cues()[0].id;

    // Standby -> Paused ist ungültig und darf den Zustand nicht verändern.
    let err = list.set_status(id, CueStatus::Paused).unwrap_err();
    assert!(matches!(err, ShowError::InvalidStateTransition { .. }));
    assert_eq!(list.cues()[0].status, CueStatus::Standby);

    // Gültiger Weg: Standby -> PreWait -> Playing -> Completed.
    list.set_status(id, CueStatus::PreWait).unwrap();
    list.set_status(id, CueStatus::Playing).unwrap();
    list.set_status(id, CueStatus::Completed).unwrap();
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

// ---------------------------------------------------------------------------
// Group-Cues: Kaskadierung
// ---------------------------------------------------------------------------

#[test]
fn group_simultaneous_cascades_to_all_armed_children_at_once() {
    let mut list = CueList::new();
    let mut unarmed_child = audio("1.3");
    unarmed_child.is_armed = false;
    list.add(Cue::new_group(
        "1",
        "Scene",
        GroupMode::Simultaneous,
        vec![audio("1.1"), audio("1.2"), unarmed_child],
    ));

    let outcome = list.go().expect("group must trigger");
    // Zwei armed Kinder → zwei Play-Commands in EINEM Go.
    assert_eq!(outcome.commands.len(), 2);
    assert!(outcome
        .commands
        .iter()
        .all(|c| matches!(c, EngineCommand::Play { .. })));

    let group = &list.cues()[0];
    assert_eq!(group.status, CueStatus::Playing);
    assert_eq!(group.children[0].status, CueStatus::Playing);
    assert_eq!(group.children[1].status, CueStatus::Playing);
    assert_eq!(
        group.children[2].status,
        CueStatus::Standby,
        "unarmed child must stay untouched"
    );
    assert_eq!(group.children[0].parent_group, Some(group.id));
}

#[test]
fn group_simultaneous_closes_when_all_triggered_children_finish() {
    let mut list = CueList::new();
    let mut unarmed_child = audio("1.3");
    unarmed_child.is_armed = false;
    list.add(Cue::new_group(
        "1",
        "Scene",
        GroupMode::Simultaneous,
        vec![audio("1.1"), audio("1.2"), unarmed_child],
    ));
    let _ = list.go().unwrap();

    let group = &list.cues()[0];
    let id1 = group.children[0].id;
    let id2 = group.children[1].id;

    list.set_status(id1, CueStatus::Completed).unwrap();
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Playing,
        "group stays open while a child runs"
    );
    list.set_status(id2, CueStatus::Completed).unwrap();
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "group closes when all triggered children are finished (unarmed child must not block)"
    );
}

#[test]
fn group_sequential_splices_children_into_the_stream() {
    let mut list = CueList::new();
    let group = Cue::new_group(
        "1",
        "Scene",
        GroupMode::Sequential,
        vec![audio("1.1"), audio("1.2"), audio("1.3")],
    );
    let group_id = group.id;
    list.add(group);

    // GO auf die Gruppe: keine Commands, Kinder werden eingespielt.
    let outcome = list.go().expect("group must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.len(), 4, "group + 3 spliced children");
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    assert_eq!(list.cursor(), 1, "playhead must point at first child");
    assert_eq!(list.cues()[1].parent_group, Some(group_id));

    // Jedes weitere GO triggert das nächste Kind der Reihe nach.
    let id1 = list.cues()[1].id;
    let o1 = list.go().unwrap();
    assert_eq!(o1.index, 1);
    assert_eq!(o1.commands.len(), 1);
    list.set_status(id1, CueStatus::Completed).unwrap();

    let id2 = list.cues()[2].id;
    let o2 = list.go().unwrap();
    assert_eq!(o2.index, 2);
    list.set_status(id2, CueStatus::Completed).unwrap();

    let id3 = list.cues()[3].id;
    let o3 = list.go().unwrap();
    assert_eq!(o3.index, 3);
    list.set_status(id3, CueStatus::Completed).unwrap();

    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "group closes after last child finished"
    );
    assert!(list.go().is_none(), "stream must be exhausted");
}

#[test]
fn group_without_armed_children_finishes_immediately() {
    let mut list = CueList::new();
    let mut child = audio("1.1");
    child.is_armed = false;
    list.add(Cue::new_group(
        "1",
        "Empty",
        GroupMode::Simultaneous,
        vec![child],
    ));

    let outcome = list.go().expect("group must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
    // Playhead trotzdem vorgerückt.
    assert!(list.go().is_none());
}

#[test]
fn nested_group_is_treated_as_instant_child() {
    // Dokumentierte V2-Grenze: Groups kaskadieren eine Ebene tief. Eine Gruppe
    // als Kind einer Simultaneous-Gruppe hat kein eigenes Engine-Command und
    // gilt als Sofort-Cue (ihre Kinder werden in V2 nicht rekursiv getriggert).
    let mut list = CueList::new();
    let inner = Cue::new_group(
        "1.1",
        "Inner",
        GroupMode::Simultaneous,
        vec![audio("1.1.1")],
    );
    list.add(Cue::new_group(
        "1",
        "Outer",
        GroupMode::Simultaneous,
        vec![inner],
    ));

    let outcome = list.go().unwrap();
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
    assert_eq!(list.cues()[0].children[0].status, CueStatus::Completed);
}

// ---------------------------------------------------------------------------
// IDs, Reordering, Stress
// ---------------------------------------------------------------------------

#[test]
fn cue_ids_are_unique_and_stable_across_clones() {
    let a = audio("1");
    let b = audio("2");
    assert_ne!(a.id, b.id, "IDs must be unique");
    let cloned = a.clone();
    assert_eq!(a.id, cloned.id, "ID must be stable across clones");
}

#[test]
fn reordering_moves_selection_and_cursor_with_the_cue() {
    let mut list = CueList::new();
    list.add(audio("1"));
    list.add(audio("2"));
    list.add(audio("3"));
    // GO triggert "1": Playhead rückt auf 1, Selektion auf den getriggerten Cue.
    let _ = list.go();
    assert_eq!(list.cursor(), 1);
    assert_eq!(list.selected_index(), Some(0));

    // "3" (Index 2) über "2" (Playhead-Position) schieben.
    list.select(Some(2));
    assert!(list.move_up(2));
    assert_eq!(list.cues()[1].number, "3");
    assert_eq!(list.cursor(), 2, "playhead follows its cue \"2\" (1 → 2)");
    assert_eq!(
        list.selected_index(),
        Some(1),
        "selection follows its cue \"3\" (2 → 1)"
    );

    // "3" (Index 1) wieder nach unten: Playhead folgt "2" (2 → 1).
    assert!(list.move_down(1));
    assert_eq!(list.cues()[2].number, "3");
    assert_eq!(list.cursor(), 1, "playhead follows its cue \"2\" (2 → 1)");
    assert_eq!(
        list.selected_index(),
        Some(2),
        "selection follows its cue \"3\" (1 → 2)"
    );

    // Grenzen
    assert!(!list.move_up(0));
    assert!(!list.move_down(2));
}

#[test]
fn go_stress_500_cues_no_gaps_no_duplicates() {
    let mut list = CueList::new();
    for i in 0..500 {
        list.add(audio(&i.to_string()));
    }
    let mut triggered = Vec::new();
    while let Some(outcome) = list.go() {
        assert_eq!(outcome.commands.len(), 1);
        triggered.push(outcome.index);
    }
    assert_eq!(triggered.len(), 500);
    assert_eq!(list.cursor(), 500);
    assert_eq!(list.running_count(), 500);
    // Indizes lückenlos und aufsteigend.
    for (expected, actual) in triggered.iter().enumerate() {
        assert_eq!(*actual, expected);
    }
}
