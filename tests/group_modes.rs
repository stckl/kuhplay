//! Tests für die erweiterten Group-Modi: Timeline (zeitversetzte Armierung),
//! Sequential/Shuffle (Kinder nacheinander in der Gruppe, optional wiederholt)
//! und Shuffle (Mischung nach Shuffle-ID).

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
// Sequential / Shuffle
// ---------------------------------------------------------------------------

fn kids(n: usize) -> Vec<Cue> {
    (0..n).map(|i| audio(&format!("1.{i}"))).collect()
}

/// Nummer des gerade laufenden Kinds.
fn playing(list: &CueList) -> Option<String> {
    list.cues()[0]
        .children
        .iter()
        .find(|c| c.status == CueStatus::Playing)
        .map(|c| c.number.clone())
}

/// Meldet das laufende Kind fertig (wie das Engine-Event) und liefert es.
fn finish_current(list: &mut CueList) -> Option<String> {
    let child = list.cues()[0]
        .children
        .iter()
        .find(|c| c.status == CueStatus::Playing)
        .map(|c| (c.id, c.number.clone()))?;
    let _ = list.set_status_with_follow(child.0, CueStatus::Completed);
    Some(child.1)
}

/// Spielt eine Play-through-Gruppe einen Durchgang lang; liefert die Reihenfolge.
fn play_pass(list: &mut CueList, n: usize) -> Vec<String> {
    (0..n).filter_map(|_| finish_current(list)).collect()
}

#[test]
fn sequential_go_plays_all_children_inside_the_group() {
    let mut list = CueList::new();
    list.add(Cue::new_group("1", "Seq", GroupMode::Sequential, kids(3)));
    list.add(audio("2"));

    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands.len(),
        1,
        "first child starts with the group"
    );
    assert_eq!(list.len(), 2, "children stay inside the group");
    assert_eq!(list.cues()[0].children.len(), 3);
    assert_eq!(list.cursor(), 1, "playhead on the cue after the group");

    assert_eq!(play_pass(&mut list, 3), ["1.0", "1.1", "1.2"]);
    assert_eq!(list.cues()[0].status, CueStatus::Completed, "group closes");
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Standby,
        "next cue untouched"
    );
}

#[test]
fn go_per_child_needs_a_go_for_every_child() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "Seq", GroupMode::Sequential, kids(3));
    group.group_go_per_child = true;
    list.add(group);
    list.add(audio("2"));

    let _ = list.go().unwrap();
    assert_eq!(playing(&list).as_deref(), Some("1.0"));
    assert_eq!(list.cursor(), 0, "playhead stays on the group");
    finish_current(&mut list);
    assert_eq!(playing(&list), None, "no automatic next child");

    let outcome = list.go().unwrap();
    assert_eq!(outcome.index, 0);
    assert_eq!(playing(&list).as_deref(), Some("1.1"));
    finish_current(&mut list);
    let _ = list.go().unwrap();
    assert_eq!(playing(&list).as_deref(), Some("1.2"));
    assert_eq!(
        list.cursor(),
        1,
        "after the last child the playhead moves on"
    );
    finish_current(&mut list);
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

#[test]
fn sequential_group_repeats_count_times() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "Seq", GroupMode::Sequential, kids(2));
    group.loop_mode = LoopMode::Count(2);
    list.add(group);

    let _ = list.go().unwrap();
    assert_eq!(play_pass(&mut list, 2), ["1.0", "1.1"]);
    assert_eq!(list.cues()[0].status, CueStatus::Playing, "second pass");
    assert_eq!(list.cues()[0].group_iterations_done, 1);
    assert_eq!(play_pass(&mut list, 2), ["1.0", "1.1"]);
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "passes used up"
    );
    assert!(list.go().is_none());
}

#[test]
fn sequential_and_shuffle_groups_can_repeat_endlessly() {
    for mode in [GroupMode::Sequential, GroupMode::Shuffle] {
        let mut group = Cue::new_group("1", "Playlist", mode, kids(6));
        group.loop_mode = LoopMode::Infinite;
        let preview: Vec<String> = group
            .shuffled_children()
            .iter()
            .map(|c| c.number.clone())
            .collect();
        let mut list = CueList::new();
        list.add(group);
        let _ = list.go().unwrap();
        let passes: Vec<Vec<String>> = (0..4).map(|_| play_pass(&mut list, 6)).collect();
        assert_eq!(
            list.cues()[0].status,
            CueStatus::Playing,
            "{mode:?}: endless"
        );
        assert!(
            playing(&list).is_some(),
            "{mode:?}: next pass runs by itself"
        );
        assert_eq!(list.cues()[0].group_iterations_done, 4);
        for pass in &passes {
            let mut sorted = pass.clone();
            sorted.sort();
            assert_eq!(
                sorted,
                preview
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>(),
                "{mode:?}: every child once per pass"
            );
        }
        if mode == GroupMode::Shuffle {
            assert_eq!(passes[0], preview, "first pass = shown shuffle");
            assert!(passes.windows(2).any(|w| w[0] != w[1]), "passes reshuffled");
        } else {
            assert!(
                passes.iter().all(|p| *p == passes[0]),
                "list order each pass"
            );
        }
    }
}

#[test]
fn shuffle_order_is_reproducible_for_the_same_shuffle_id() {
    let group = Cue::new_group("1", "Shuffle", GroupMode::Shuffle, kids(8));
    let run = |group: Cue| {
        let mut group = group;
        group.loop_mode = LoopMode::Count(3);
        let mut list = CueList::new();
        list.add(group);
        let _ = list.go().unwrap();
        (0..3).map(|_| play_pass(&mut list, 8)).collect::<Vec<_>>()
    };
    let a = run(group.clone());
    assert_eq!(a, run(group.clone()), "same show day → same passes");
    let mut sorted = a[0].clone();
    sorted.sort();
    assert_eq!(sorted, (0..8).map(|i| format!("1.{i}")).collect::<Vec<_>>());
}

#[test]
fn new_shuffle_rolls_a_new_fixed_order() {
    let mut group = Cue::new_group("1", "Shuffle", GroupMode::Shuffle, kids(8));
    let order = |g: &Cue| -> Vec<String> {
        g.shuffled_children()
            .iter()
            .map(|c| c.number.clone())
            .collect()
    };
    let before = order(&group);
    assert_eq!(before, order(&group), "fixed order stays the same");
    let changed = (0..5).any(|_| {
        group.new_shuffle();
        order(&group) != before
    });
    assert!(changed, "New Shuffle must change the order");
    let fixed = order(&group);

    // GO spielt genau die Vorschau-Reihenfolge, auch nach Speichern/Laden.
    let json = serde_json::to_string(&group).unwrap();
    let group: Cue = serde_json::from_str(&json).unwrap();
    let mut list = CueList::new();
    list.add(group);
    let _ = list.go().unwrap();
    assert_eq!(play_pass(&mut list, 8), fixed);
}

#[test]
fn reset_rearms_a_running_group_in_place() {
    for mode in [GroupMode::Sequential, GroupMode::Shuffle] {
        let mut list = CueList::new();
        list.add(Cue::new_group("1", "G", mode, kids(3)));
        let _ = list.go().unwrap();
        finish_current(&mut list);
        list.reset();
        assert_eq!(list.len(), 1);
        assert!(list.cues()[0]
            .children
            .iter()
            .all(|c| c.status == CueStatus::Standby));
        assert!(list.go().is_some(), "{mode:?}: startable again");
        assert!(playing(&list).is_some());
    }
}

#[test]
fn old_shows_load_simultaneous_and_loop_groups() {
    let mut timeline = Cue::new_group("1", "G", GroupMode::Timeline, kids(1));
    let json = serde_json::to_string(&timeline)
        .unwrap()
        .replace("\"Timeline\"", "\"Simultaneous\"");
    timeline = serde_json::from_str(&json).unwrap();
    assert_eq!(timeline.group_mode, GroupMode::Timeline);
    let seq = Cue::new_group("1", "G", GroupMode::Sequential, kids(1));
    let json = serde_json::to_string(&seq)
        .unwrap()
        .replace("\"Sequential\"", "\"Loop\"");
    let seq: Cue = serde_json::from_str(&json).unwrap();
    assert_eq!(seq.group_mode, GroupMode::Sequential);
}

#[test]
fn skip_in_group_jumps_to_the_next_child() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "Seq", GroupMode::Sequential, kids(3));
    group.loop_mode = LoopMode::Count(2);
    let group_id = group.id;
    list.add(group);
    let _ = list.go().unwrap();
    assert_eq!(playing(&list).as_deref(), Some("1.0"));

    let commands = list.skip_in_group(group_id);
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, EngineCommand::Stop { .. })),
        "current child is stopped"
    );
    assert_eq!(playing(&list).as_deref(), Some("1.1"));
    // Das spätere Stopped-Event der Engine bringt die Gruppe nicht durcheinander.
    let skipped = list.cues()[0].children[0].id;
    assert!(list.set_status(skipped, CueStatus::Stopped).is_err());

    let _ = list.skip_in_group(group_id);
    let _ = list.skip_in_group(group_id); // nach dem letzten: zweiter Durchgang
    assert_eq!(list.cues()[0].group_iterations_done, 1);
    assert_eq!(playing(&list).as_deref(), Some("1.0"));
    for _ in 0..3 {
        let _ = list.skip_in_group(group_id);
    }
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
    assert!(
        list.skip_in_group(group_id).is_empty(),
        "nothing left to skip"
    );
}

#[test]
fn group_fade_in_fades_started_children_from_silence() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "T", GroupMode::Timeline, kids(2));
    group.group_fade_in = Duration::from_secs(2);
    list.add(group);
    let outcome = list.go().unwrap();
    let fades: Vec<_> = outcome
        .commands
        .iter()
        .filter_map(|c| match c {
            EngineCommand::Fade {
                target_cue,
                from,
                volume,
                duration,
                stop_when_done,
                ..
            } => Some((*target_cue, *from, *volume, *duration, *stop_when_done)),
            _ => None,
        })
        .collect();
    assert_eq!(fades.len(), 2, "one fade per started child");
    for (target, from, volume, duration, stop) in fades {
        assert!(target.is_some());
        assert_eq!(from, Some(0.0));
        assert_eq!(volume, Some(1.0));
        assert_eq!(duration, Duration::from_secs(2));
        assert!(!stop);
    }
}

#[test]
fn stopping_a_group_with_fade_out_fades_its_children() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "T", GroupMode::Timeline, kids(2));
    group.group_fade_out = Duration::from_secs(3);
    let group_id = group.id;
    list.add(group);
    let _ = list.go().unwrap();
    let commands = list.stop_running(group_id);
    let fades = commands
        .iter()
        .filter(|c| {
            matches!(c, EngineCommand::Fade { volume: Some(v), stop_when_done: true, duration, .. }
                if *v == 0.0 && *duration == Duration::from_secs(3))
        })
        .count();
    assert_eq!(fades, 2, "children fade out and stop: {commands:?}");
    assert!(!commands
        .iter()
        .any(|c| matches!(c, EngineCommand::Stop { cue_id } if *cue_id != group_id)));
}

/// Engine meldet nach `Finished` noch `PostWaitElapsed` (Post-Wait 0): nach
/// dem Re-Arm einer Endlos-Gruppe darf das nicht das zweite Kind zusätzlich
/// starten.
#[test]
fn late_post_wait_event_does_not_start_a_second_child_after_rearm() {
    let mut list = CueList::new();
    let mut group = Cue::new_group("1", "Seq", GroupMode::Sequential, kids(3));
    group.loop_mode = LoopMode::Infinite;
    list.add(group);
    let _ = list.go().unwrap();
    for _ in 0..3 {
        let id = list.cues()[0]
            .children
            .iter()
            .find(|c| c.status == CueStatus::Playing)
            .unwrap()
            .id;
        let _ = list.set_status_with_follow(id, CueStatus::Completed); // Finished
        let _ = list.set_status_with_follow(id, CueStatus::Completed); // PostWaitElapsed
    }
    let active: Vec<String> = list.cues()[0]
        .children
        .iter()
        .filter(|c| c.status.is_active())
        .map(|c| c.number.clone())
        .collect();
    assert_eq!(active, ["1.0"], "second pass plays one child at a time");
    assert!(list.cues()[0].children[1..]
        .iter()
        .all(|c| c.status == CueStatus::Standby));
}

#[test]
fn skip_in_a_paused_group_plays_the_next_child() {
    let mut list = CueList::new();
    let group = Cue::new_group("1", "Seq", GroupMode::Sequential, kids(3));
    let group_id = group.id;
    list.add(group);
    let _ = list.go().unwrap();
    let _ = list.pause_running(group_id);
    assert_eq!(list.cues()[0].status, CueStatus::Paused);
    let commands = list.skip_in_group(group_id);
    assert!(commands
        .iter()
        .any(|c| matches!(c, EngineCommand::Play { .. })));
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    assert_eq!(playing(&list).as_deref(), Some("1.1"));
    assert_eq!(list.cues()[0].children[0].status, CueStatus::Completed);
}

#[test]
fn drop_places_put_cues_exactly_before_after_or_into() {
    use kuhplay::core::DropPlace;
    let mut list = CueList::new();
    let (a, b, c) = (audio("1"), audio("2"), audio("3"));
    let ids = [a.id, b.id, c.id];
    let group = Cue::new_group("4", "G", GroupMode::Sequential, kids(2));
    let gid = group.id;
    list.add(a);
    list.add(b);
    list.add(c);
    list.add(group);
    let numbers = |l: &CueList| {
        l.cues()
            .iter()
            .map(|c| c.number.clone())
            .collect::<Vec<_>>()
    };
    // 3 vor 1, dann 1 hinter 2.
    assert!(list.move_ids_to(&[ids[2]], ids[0], DropPlace::Before));
    assert_eq!(numbers(&list), ["3", "1", "2", "4"]);
    assert!(list.move_ids_to(&[ids[0]], ids[1], DropPlace::After));
    assert_eq!(numbers(&list), ["3", "2", "1", "4"]);
    // In die Gruppe: hinten bzw. vorne.
    assert!(list.move_ids_to(&[ids[1]], gid, DropPlace::Into));
    assert!(list.move_ids_to(&[ids[2]], gid, DropPlace::IntoFirst));
    let kids: Vec<String> = list.cues()[1]
        .children
        .iter()
        .map(|c| c.number.clone())
        .collect();
    assert_eq!(kids, ["3", "1.0", "1.1", "2"]);
}
