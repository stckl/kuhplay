//! Tests für die erweiterten Group-Modi: Timeline (zeitversetzte Armierung),
//! Loop (Kinder-Kette wiederholt sich) und Shuffle (deterministische Mischung).

use std::time::Duration;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, GroupMode, LoopMode};
use kuhplay::engine::EngineCommand;

fn audio(number: &str) -> Cue {
    Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
}

/// Pre-Waits der Play-Commands in Reihenfolge.
fn pre_waits(commands: &[EngineCommand]) -> Vec<Duration> {
    commands
        .iter()
        .map(|c| match c {
            EngineCommand::Play { pre_wait, .. } => *pre_wait,
            other => panic!("expected Play, got {other:?}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

#[test]
fn timeline_triggers_all_children_with_staggered_pre_waits() {
    let mut list = CueList::new();
    let group = Cue::new_group(
        "1",
        "Timeline",
        GroupMode::Timeline,
        vec![
            audio("1.1"), // pre_wait 0
            audio("1.2").with_pre_wait(Duration::from_millis(100)),
            audio("1.3").with_pre_wait(Duration::from_millis(250)),
        ],
    );
    list.add(group);

    let outcome = list.go().expect("timeline group must trigger");
    assert_eq!(outcome.commands.len(), 3, "all children fire in one GO");
    assert_eq!(
        pre_waits(&outcome.commands),
        vec![
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(250)
        ],
        "timeline staggers children via their individual pre-waits"
    );

    let group = &list.cues()[0];
    assert_eq!(group.status, CueStatus::Playing);
    // pre_wait = 0 → sofort Playing; pre_wait > 0 → PreWait-Phase.
    assert_eq!(group.children[0].status, CueStatus::Playing);
    assert_eq!(group.children[1].status, CueStatus::PreWait);
    assert_eq!(group.children[2].status, CueStatus::PreWait);
}

#[test]
fn timeline_closes_when_all_children_complete() {
    let mut list = CueList::new();
    list.add(Cue::new_group(
        "1",
        "Timeline",
        GroupMode::Timeline,
        vec![
            audio("1.1"),
            audio("1.2").with_pre_wait(Duration::from_millis(50)),
        ],
    ));
    let _ = list.go().unwrap();

    let id1 = list.cues()[0].children[0].id; // pre_wait 0 → Playing
    let id2 = list.cues()[0].children[1].id; // pre_wait 50 ms → PreWait
    assert_eq!(list.cues()[0].children[1].status, CueStatus::PreWait);
    list.set_status(id1, CueStatus::Completed).unwrap();
    assert_eq!(list.cues()[0].status, CueStatus::Playing, "still open");
    // Realistischer Event-Flow: PreWait →(Started)→ Playing →(Finished)→ Completed.
    list.set_status(id2, CueStatus::Playing).unwrap();
    list.set_status(id2, CueStatus::Completed).unwrap();
    assert_eq!(list.cues()[0].status, CueStatus::Completed, "closed");
}

// ---------------------------------------------------------------------------
// Loop
// ---------------------------------------------------------------------------

#[test]
fn loop_group_repeats_children_until_iterations_exhausted() {
    let mut list = CueList::new();
    let mut group = Cue::new_group(
        "1",
        "Loop",
        GroupMode::Loop,
        vec![audio("1.1"), audio("1.2")],
    );
    group.loop_mode = LoopMode::Count(2);
    list.add(group);

    // --- Erste Runde ---
    let _ = list.go().unwrap(); // splicet Kinder
    assert_eq!(list.len(), 3);
    let _ = list.go().unwrap(); // Kind 1.1
    let id1 = list.cues()[1].id;
    list.set_status(id1, CueStatus::Completed).unwrap();
    let _ = list.go().unwrap(); // Kind 1.2
    let id2 = list.cues()[2].id;
    list.set_status(id2, CueStatus::Completed).unwrap();

    // Re-Arm: Kinder zurück auf Standby, Playhead auf erstes Kind.
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Playing,
        "group loops again"
    );
    assert_eq!(list.cues()[0].group_iterations_done, 1);
    assert_eq!(list.cues()[1].status, CueStatus::Standby, "child re-armed");
    assert_eq!(list.cues()[2].status, CueStatus::Standby, "child re-armed");
    assert_eq!(list.cursor(), 1, "playhead back at first child");

    // --- Zweite (letzte) Runde ---
    let _ = list.go().unwrap();
    let id1 = list.cues()[1].id;
    list.set_status(id1, CueStatus::Completed).unwrap();
    let _ = list.go().unwrap();
    let id2 = list.cues()[2].id;
    list.set_status(id2, CueStatus::Completed).unwrap();

    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "iterations exhausted → group completes"
    );
    assert!(list.go().is_none(), "stream exhausted");
}

#[test]
fn loop_group_with_count_1_behaves_like_sequential() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "Loop", GroupMode::Loop, vec![audio("1.1")]);
    group.loop_mode = LoopMode::Count(1);
    list.add(group);

    let _ = list.go().unwrap(); // splicet
    let _ = list.go().unwrap(); // Kind
    let id1 = list.cues()[1].id;
    list.set_status(id1, CueStatus::Completed).unwrap();
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "Count(1) → keine Wiederholung"
    );
}

// ---------------------------------------------------------------------------
// Shuffle
// ---------------------------------------------------------------------------

#[test]
fn shuffle_splices_a_permutation_of_all_children() {
    let mut list = CueList::new();
    let children: Vec<Cue> = (0..8).map(|i| audio(&format!("1.{i}"))).collect();
    list.add(Cue::new_group("1", "Shuffle", GroupMode::Shuffle, children));

    let _ = list.go().unwrap();
    assert_eq!(list.len(), 9, "group + 8 spliced children");

    let mut numbers: Vec<String> = list.cues()[1..].iter().map(|c| c.number.clone()).collect();
    numbers.sort();
    let expected: Vec<String> = (0..8).map(|i| format!("1.{i}")).collect();
    assert_eq!(
        numbers, expected,
        "shuffle must contain every child exactly once"
    );
}

#[test]
fn shuffle_is_deterministic_for_the_same_group_id() {
    // Gleiche Group-ID → gleiche Reihenfolge (reproduzierbare Shows).
    let make_group = || {
        Cue::new_group(
            "1",
            "Shuffle",
            GroupMode::Shuffle,
            (0..8).map(|i| audio(&format!("1.{i}"))).collect(),
        )
    };
    let group_a = make_group();
    let mut group_b = make_group();
    group_b.id = group_a.id; // gleiche ID → gleicher Seed

    let mut list_a = CueList::new();
    list_a.add(group_a);
    let mut list_b = CueList::new();
    list_b.add(group_b);

    let _ = list_a.go().unwrap();
    let _ = list_b.go().unwrap();

    let order_a: Vec<&str> = list_a.cues()[1..]
        .iter()
        .map(|c| c.number.as_str())
        .collect();
    let order_b: Vec<&str> = list_b.cues()[1..]
        .iter()
        .map(|c| c.number.as_str())
        .collect();
    assert_eq!(
        order_a, order_b,
        "same group id must produce the same shuffle"
    );
}

#[test]
fn shuffle_group_closes_like_sequential_after_last_child() {
    let mut list = CueList::new();
    list.add(Cue::new_group(
        "1",
        "Shuffle",
        GroupMode::Shuffle,
        vec![audio("1.1"), audio("1.2")],
    ));
    let _ = list.go().unwrap(); // splicet
    assert_eq!(list.cues()[0].status, CueStatus::Playing);

    for i in 1..3 {
        let _ = list.go().unwrap();
        let id = list.cues()[i].id;
        list.set_status(id, CueStatus::Completed).unwrap();
    }
    assert_eq!(list.cues()[0].status, CueStatus::Completed, "group closes");
}
