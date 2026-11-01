//! Panic: blendet über die eingestellte Zeit aus, ein zweites Panic stoppt sofort.

use std::time::{Duration, Instant};

use kuhplay::core::{Cue, CueList, CueType};
use kuhplay::engine::spawn_mock_engine;
use kuhplay::gui::ShowApp;
use kuhplay::remote::RemoteCommand;

fn app_with_running_cue() -> ShowApp {
    let mut app = ShowApp::new(spawn_mock_engine());
    let mut list = CueList::new();
    for number in ["1", "2"] {
        let mut cue = Cue::new(number, "Audio", CueType::Audio);
        cue.target_path = Some("/test/audio.wav".to_string());
        cue.duration = Duration::from_secs(60);
        list.add(cue);
    }
    *app.carts.active_mut() = list;
    app.go_now();
    app.poll_engine_events();
    assert_eq!(app.carts.running_count(), 1, "ein Cue muss laufen");
    app
}

fn wait_until(app: &mut ShowApp, timeout: Duration, cond: impl Fn(&ShowApp) -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        app.poll_engine_events();
        if cond(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn panic_fades_then_stops() {
    let mut app = app_with_running_cue();
    app.panic_duration = Duration::from_millis(200);
    app.panic();
    app.poll_engine_events();
    assert!(
        app.carts.running_count() > 0,
        "läuft während des Ausblendens weiter"
    );
    assert!(
        wait_until(&mut app, Duration::from_secs(3), |a| a
            .carts
            .running_count()
            == 0),
        "nach der Ausblendzeit steht alles"
    );
}

#[test]
fn second_panic_stops_at_once() {
    let mut app = app_with_running_cue();
    app.panic_duration = Duration::from_secs(30);
    app.panic();
    app.panic();
    assert_eq!(app.carts.running_count(), 0, "zweites Panic stoppt sofort");
}

#[test]
fn zero_duration_stops_at_once() {
    let mut app = app_with_running_cue();
    app.panic_duration = Duration::ZERO;
    app.panic();
    assert_eq!(app.carts.running_count(), 0);
}

#[test]
fn go_is_ignored_while_fading() {
    let mut app = app_with_running_cue();
    app.panic_duration = Duration::from_secs(30);
    app.panic();
    let before = app.carts.cursor();
    app.go_now();
    assert_eq!(
        app.carts.cursor(),
        before,
        "während des Ausblendens startet nichts"
    );
    app.panic();
}

#[test]
fn duration_can_be_set_remotely_and_is_saved_with_the_show() {
    let mut app = ShowApp::new(spawn_mock_engine());
    app.handle_remote_command(RemoteCommand::SetPanicDuration { ms: 2500 });
    assert_eq!(app.panic_duration, Duration::from_millis(2500));
    assert_eq!(app.build_remote_workspace().panic_duration_ms, 2500);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("panic.kuhplay");
    app.save_show(&path);
    let mut other = ShowApp::new(spawn_mock_engine());
    other.load_show(&path).unwrap();
    assert_eq!(other.panic_duration, Duration::from_millis(2500));
}
