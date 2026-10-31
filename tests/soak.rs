//! Soak-Test: GUI-Level-Lasttest ohne echte Medien/Hardware.
//!
//! Mischt über 120 Iterationen GO/Panic/Pause/Resume/Reset/GoTo/Arm/Scrub
//! (als RemoteCommands wie vom Web-Remote/Companion/OSC) mit Engine-Polling
//! und gelegentlichem egui-Render. Darf nicht paniken; am Ende muss Panic
//! alle Cues sauber stoppen.

use std::time::Duration;

use kuhplay::engine::spawn_mock_engine;
use kuhplay::gui::{icons, ShowApp};
use kuhplay::remote::RemoteCommand;

fn render_frame(ctx: &egui::Context, app: &mut ShowApp) {
    icons::install(ctx); // idempotent – Phosphor-Icons registrieren
    let raw_input = egui::RawInput::default();
    let _ = ctx.run(raw_input, |ctx| app.draw_workspace(ctx));
}

#[test]
fn soak_mixed_remote_commands_and_renders_stay_stable() {
    let ctx = egui::Context::default();
    let mut app = ShowApp::new(spawn_mock_engine());

    let commands = [
        RemoteCommand::Go,
        RemoteCommand::PauseAll,
        RemoteCommand::ResumeAll,
        RemoteCommand::GoTo {
            cue: "1".to_string(),
        },
        RemoteCommand::Go,
        RemoteCommand::Reset,
        RemoteCommand::Go,
        RemoteCommand::Panic,
    ];

    for i in 0..120 {
        // Abwechselnd Remote-Command (wie Web-Remote/Companion) und direktes Go.
        match i % 4 {
            0 => app.handle_remote_command(commands[i % commands.len()].clone()),
            1 => app.go_now(),
            2 => {
                app.handle_remote_command(RemoteCommand::Arm {
                    cue: "2".to_string(),
                    armed: i % 8 == 2,
                });
            }
            _ => {
                // Scrub eines laufenden Cues (Throttle-Pfad wie Active-Cues-Balken).
                if let Some(cue) = app.carts.active().cues().first() {
                    let id = cue.id;
                    app.scrub_throttle(id, 0.5, false);
                    app.scrub_throttle(id, 0.25, true);
                }
            }
        }
        app.poll_engine_events();
        // Gelegentlich das komplette Workspace rendern (Panics in Panels zählen).
        if i % 10 == 0 {
            render_frame(&ctx, &mut app);
        }
        // Kurze Pause, damit der Mock-Engine-Thread Events liefern kann.
        std::thread::sleep(Duration::from_millis(2));
    }

    // Finaler Zustand: Panic räumt alles ab, kein Cue bleibt aktiv.
    app.handle_remote_command(RemoteCommand::Panic);
    app.poll_engine_events();
    render_frame(&ctx, &mut app);
    let still_active = app
        .carts
        .active()
        .cues()
        .iter()
        .filter(|c| c.status.is_active())
        .count();
    assert_eq!(still_active, 0, "nach Panic dürfen keine Cues aktiv sein");
}
