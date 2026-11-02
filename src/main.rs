//! Einstiegspunkt: initialisiert Logging und Engine-Thread und öffnet das
//! eframe-Fenster. bewusst schlank – die Logik lebt in [`kuhplay::gui`].

use kuhplay::engine::{spawn_mock_engine, spawn_real_engine, EngineHandle};
use kuhplay::gui::ShowApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kuhplay=info".into()),
        )
        .init();

    // Web-Remote mit PIN: Der gespeicherte PIN wird beim ersten Start angelegt.
    // Vor allen Threads, `set_var` ist danach nicht mehr sicher.
    if std::env::var_os("KUHPLAY_REMOTE_PIN").is_none() {
        std::env::set_var(
            "KUHPLAY_REMOTE_PIN",
            kuhplay::remote::web::load_or_create_pin(),
        );
    }

    // Engine-Thread starten (Kommunikation läuft danach über Kanäle).
    // Default: produktive RealMediaEngine (echtes Audio via cpal/symphonia,
    // Video via ffmpeg). `KUHPLAY_ENGINE=mock` erzwingt die virtuelle Engine
    // (für Entwicklung ohne Audio-Hardware/Medien).
    let engine_is_mock = std::env::var("KUHPLAY_ENGINE").as_deref() == Ok("mock");
    let engine: EngineHandle = if engine_is_mock {
        tracing::info!("using mock engine (KUHPLAY_ENGINE=mock)");
        spawn_mock_engine()
    } else {
        tracing::info!("using real media engine (cpal/symphonia + ffmpeg)");
        spawn_real_engine()
    };

    // Programm-Icon (icon.png aus dem Projekt-Root, eingebettet).
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("KuhPlay — Show Control")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1024.0, 640.0]);
    if let Some(icon) = kuhplay::gui::app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "KuhPlay",
        options,
        Box::new(|cc| {
            // Theme + Phosphor-Icons einmalig setzen (CreationContext,
            // nicht jeden Frame – docs/TODO-offen.md Phase 0b).
            kuhplay::gui::theme::apply_theme(&cc.egui_ctx);
            // UI-Layout aus dem Persistenz-Storage laden (falls vorhanden).
            let mut app = match cc.storage {
                Some(storage) => ShowApp::new_with_storage(engine, storage),
                None => ShowApp::new(engine),
            };
            app.engine_label = if engine_is_mock {
                "mock (virtual)".to_string()
            } else {
                "real (cpal/ffmpeg)".to_string()
            };
            app.apply_startup_output();
            #[cfg(target_os = "macos")]
            app.install_native_menu();
            Ok(Box::new(app))
        }),
    )
}
