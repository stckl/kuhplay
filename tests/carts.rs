//! Tests für Carts ([`CartManager`]): mehrere unabhängige Cue-Listen,
//! Umschalten, Isolation der Zustände und Fehlerfälle.

use std::time::Duration;

use kuhplay::core::{CartManager, Cue, CueList, CueStatus, CueType, LoopMode};
use kuhplay::engine::EngineCommand;

fn audio(number: &str) -> Cue {
    Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
}

#[test]
fn new_cart_becomes_active_and_is_named() {
    let mut mgr = CartManager::new();
    assert_eq!(mgr.cart_count(), 1);
    assert_eq!(mgr.cart_name(0), "Cart 1");
    assert_eq!(mgr.active_index(), 0);

    let idx = mgr.add_cart("Intro");
    assert_eq!(idx, 1);
    assert_eq!(mgr.active_index(), 1, "new cart becomes active");
    assert_eq!(mgr.cart_name(1), "Intro");
}

#[test]
fn go_only_affects_the_active_cart() {
    let mut mgr = CartManager::new();
    mgr.active_mut().add(audio("1"));
    mgr.add_cart("Cart 2");
    mgr.active_mut().add(audio("A"));

    // Cart 2 aktiv.
    let outcome = mgr.go().expect("GO in active cart");
    assert_eq!(mgr.cues()[outcome.index].number, "A");
    assert_eq!(mgr.cues()[0].status, CueStatus::Playing);

    // Cart 1: unangetastet.
    mgr.set_active(0).unwrap();
    assert_eq!(mgr.cues()[0].status, CueStatus::Standby);
    assert_eq!(mgr.cursor(), 0);
    let outcome = mgr.go().unwrap();
    assert_eq!(mgr.cues()[outcome.index].number, "1");
}

#[test]
fn switching_carts_preserves_each_carts_state() {
    let mut mgr = CartManager::new();
    mgr.active_mut()
        .add(audio("1").with_loop(LoopMode::Infinite));
    let _ = mgr.go().unwrap(); // Cart 1: "1" läuft
    assert_eq!(mgr.cues()[0].status, CueStatus::Playing);

    // Wechsel zu Cart 2 und zurück: Zustand bleibt erhalten.
    mgr.add_cart("Cart 2");
    assert!(mgr.is_empty(), "Cart 2 is empty");
    mgr.set_active(0).unwrap();
    assert_eq!(mgr.cues()[0].status, CueStatus::Playing, "state preserved");
    assert_eq!(mgr.cursor(), 1, "playhead preserved");
    assert_eq!(mgr.running_count(), 1);
}

#[test]
fn stop_all_running_and_reset_only_touch_the_active_cart() {
    let mut mgr = CartManager::new();
    mgr.active_mut().add(audio("1"));
    mgr.add_cart("Cart 2");
    mgr.active_mut().add(audio("A"));

    let _ = mgr.go().unwrap(); // Cart 2: "A" läuft
    mgr.stop_all_running();
    assert_eq!(mgr.cues()[0].status, CueStatus::Stopped);

    mgr.set_active(0).unwrap();
    assert_eq!(
        mgr.cues()[0].status,
        CueStatus::Standby,
        "Cart 1 untouched by panic"
    );

    mgr.reset();
    assert_eq!(mgr.cursor(), 0);
}

#[test]
fn last_cart_cannot_be_removed() {
    let mut mgr = CartManager::new();
    assert!(mgr.remove_cart(0).is_err(), "the last cart must survive");
    assert_eq!(mgr.cart_count(), 1);
}

#[test]
fn remove_cart_adjusts_active_index() {
    let mut mgr = CartManager::new();
    mgr.add_cart("Cart 2");
    mgr.add_cart("Cart 3");
    assert_eq!(mgr.active_index(), 2);

    mgr.remove_cart(2).unwrap(); // aktiven (letzten) entfernen
    assert_eq!(mgr.active_index(), 1, "active index clamped");
    assert_eq!(mgr.cart_count(), 2);

    assert!(mgr.remove_cart(99).is_err(), "out of bounds");
}

#[test]
fn set_active_validates_bounds() {
    let mut mgr = CartManager::new();
    assert!(mgr.set_active(0).is_ok());
    assert!(mgr.set_active(1).is_err());
    assert_eq!(mgr.active_index(), 0);
}

#[test]
fn selection_and_standby_delegate_to_active_cart() {
    let mut mgr = CartManager::new();
    mgr.active_mut().add(audio("1"));
    mgr.active_mut().add(audio("2"));

    let standby = mgr.standby().expect("standby cue");
    assert_eq!(standby.number, "1");

    mgr.select(Some(1));
    assert_eq!(mgr.selected_index(), Some(1));
    assert_eq!(mgr.selected().unwrap().number, "2");
    assert!(mgr.selected_mut().is_some());

    // GO rückt Playhead und Selektion weiter.
    let outcome = mgr.go().unwrap();
    assert_eq!(outcome.index, 0);
    assert!(matches!(outcome.commands[0], EngineCommand::Play { .. }));
    assert_eq!(mgr.cursor(), 1);
    assert_eq!(mgr.standby().unwrap().number, "2");
}

#[test]
fn cue_list_with_custom_name_roundtrips_via_serde() {
    let mut list = CueList::with_name("Outro");
    list.add(audio("1").with_duration(Duration::from_millis(500)));
    let json = serde_json::to_string(&list).unwrap();
    let back: CueList = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name(), "Outro");
    assert_eq!(back.len(), 1);
}

#[test]
fn cart_manager_serde_roundtrip_preserves_carts_and_active() {
    let mut mgr = CartManager::new();
    mgr.active_mut().add(audio("1"));
    mgr.add_cart("Cart 2");
    mgr.active_mut().add(audio("A"));

    let json = serde_json::to_string(&mgr).unwrap();
    let back: CartManager = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cart_count(), 2);
    assert_eq!(back.active_index(), 1);
    assert_eq!(back.cart_name(0), "Cart 1");
    assert_eq!(back.cues()[0].number, "A");
}
