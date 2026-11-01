//! Concurrency- & Stress-Tests: Race-Condition- und Thread-Safety-Prüfung.
//!
//! Ziel: Unter Last (hunderte gleichzeitige Play/Stop/Go-Kommandos über die
//! MPSC-Channels) dürfen weder Deadlocks noch Message-Drops auftreten, und der
//! Engine-Thread muss sich sauber herunterfahren lassen.
//!
//! Verifikation "keine Drops": Jedes `Ping(n)` muss ein `Pong(n)` erzeugen –
//! bei 1000 Pings von 4 Threads müssen exakt 1000 eindeutige Pongs ankommen.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use uuid::Uuid;

use kuhplay::core::LoopMode;
use kuhplay::engine::{spawn_mock_engine_with_join, EngineCommand, EngineEvent};

/// Wartet mit Timeout darauf, dass der Engine-Thread terminiert.
fn assert_thread_terminates(join: std::thread::JoinHandle<()>) {
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = join.join();
        let _ = done_tx.send(());
    });
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("engine thread must terminate after the handle is dropped");
}

#[test]
fn ping_storm_1000_messages_no_drops_no_deadlock() {
    let (engine, join) = spawn_mock_engine_with_join();

    // 4 Threads senden je 250 Pings = 1000 Commands gleichzeitig.
    let mut threads = Vec::new();
    for t in 0..4u64 {
        let tx = engine.commands.clone();
        threads.push(std::thread::spawn(move || {
            for i in 0..250u64 {
                tx.send(EngineCommand::Ping(t * 1000 + i))
                    .expect("unbounded channel send must not fail");
            }
        }));
    }
    for thread in threads {
        thread.join().expect("sender threads must finish");
    }

    // Alle 1000 Pongs müssen ankommen – eindeutig, ohne Duplikate/Verluste.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut pongs = HashSet::new();
    while pongs.len() < 1000 && Instant::now() < deadline {
        match engine.events.recv_timeout(Duration::from_millis(100)) {
            Ok(EngineEvent::Pong(n)) => {
                assert!(
                    pongs.insert(n),
                    "duplicate Pong({n}) – message processed twice"
                );
            }
            Ok(_) => {}
            Err(_) => {} // Timeout → Schleife prüft Deadline
        }
    }
    assert_eq!(
        pongs.len(),
        1000,
        "every Ping must produce exactly one Pong"
    );

    drop(engine);
    assert_thread_terminates(join);
}

#[test]
fn play_stop_storm_keeps_engine_alive_and_consistent() {
    let (engine, join) = spawn_mock_engine_with_join();

    // 4 Threads feuern gemischte Play/Stop/StopAll-Kommandos.
    let mut threads = Vec::new();
    for _ in 0..4 {
        let tx = engine.commands.clone();
        threads.push(std::thread::spawn(move || {
            for i in 0..100u32 {
                let cue_id = Uuid::new_v4();
                let _ = tx.send(EngineCommand::Play {
                    cue_id,
                    path: None,
                    duration: Some(Duration::from_millis(20)),
                    pre_wait: Duration::ZERO,
                    post_wait: Duration::ZERO,
                    loop_mode: LoopMode::Once,
                    slices: Vec::new(),
                    route: None,
                    timecode_in: None,
                    stage: None,
                    layer: 0,
                    surface: None,
                });
                if i % 2 == 0 {
                    let _ = tx.send(EngineCommand::Stop { cue_id });
                }
                if i % 10 == 0 {
                    let _ = tx.send(EngineCommand::StopAll);
                }
                if i % 25 == 0 {
                    let _ = tx.send(EngineCommand::Ping(i as u64));
                }
            }
        }));
    }
    for thread in threads {
        thread.join().expect("worker threads must finish");
    }

    // Der Engine-Thread lebt und verarbeitet weiterhin Kommandos (kein Deadlock).
    engine.send(EngineCommand::Ping(9999)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15); // CI-Runner teils stark ausgelastet
    let mut got_pong = false;
    while Instant::now() < deadline {
        if let Some(EngineEvent::Pong(9999)) = engine.try_recv_event() {
            got_pong = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(got_pong, "engine must answer Ping after the command storm");

    // StopAll räumt garantiert auf: Danach ist ein Ping sofort beantwortbar
    // und der Thread fährt sauber herunter.
    engine.send(EngineCommand::StopAll).unwrap();
    drop(engine);
    assert_thread_terminates(join);
}

#[test]
fn rapid_go_burst_over_channel_matches_domain_expectations() {
    // Simuliert einen Operator, der GO hämmert: 200 aufeinanderfolgende
    // Play-Commands über denselben Kanal, danach StopAll.
    let (engine, join) = spawn_mock_engine_with_join();

    let mut cue_ids = Vec::new();
    for _ in 0..200 {
        let cue_id = Uuid::new_v4();
        cue_ids.push(cue_id);
        engine
            .send(EngineCommand::Play {
                cue_id,
                path: None,
                duration: Some(Duration::from_millis(50)),
                pre_wait: Duration::ZERO,
                post_wait: Duration::ZERO,
                loop_mode: LoopMode::Once,
                slices: Vec::new(),
                route: None,
                timecode_in: None,
                stage: None,
                layer: 0,
                surface: None,
            })
            .unwrap();
    }
    engine.send(EngineCommand::StopAll).unwrap();

    // Jedes Play wird mit Stopped beantwortet (StopAll), kein Drop.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stopped = HashSet::new();
    while stopped.len() < 200 && Instant::now() < deadline {
        if let Some(EngineEvent::Stopped { cue_id }) = engine.try_recv_event() {
            stopped.insert(cue_id);
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    for id in &cue_ids {
        assert!(stopped.contains(id), "missing Stopped for cue {id}");
    }

    drop(engine);
    assert_thread_terminates(join);
}
