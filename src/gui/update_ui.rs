//! Update-Dialog + Zustandsmaschine (Block U4).
//!
//! Show-Sicherheit: während Cues laufen oder im Show-Mode werden weder
//! HinweiseInstallationen ausgeführt – der Dialog zeigt dann nur einen Hinweis.
//! Unsigierte Updates (keine gültige Ed25519-Signatur gegen den gepinnten
//! Schlüssel) werden nie installiert.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::gui::app::ShowApp;

/// Laufzeit-Zustand der Update-Prüfung (nicht persistiert).
#[derive(Debug, Clone)]
pub enum UpdateUiState {
    Idle,
    Checking,
    UpToDate,
    Available(crate::update::UpdateCheck),
    Failed(String),
    Downloading,
    ReadyToInstall(PathBuf),
}

/// Ergebnis des Hintergrund-Checks.
pub enum UpdateCheckResult {
    UpToDate,
    Available(crate::update::UpdateCheck),
    Failed(String),
}

/// Letztes Update-Check-Ergebnis als JSON – wird vom Web-Remote unter
/// `/api/update` ausgespielt (nur Hinweis, keine Installation über Remote).
static LAST_UPDATE_INFO: OnceLock<Mutex<Option<serde_json::Value>>> = OnceLock::new();

fn last_info() -> &'static Mutex<Option<serde_json::Value>> {
    LAST_UPDATE_INFO.get_or_init(|| Mutex::new(None))
}

pub fn set_last_update_info(info: serde_json::Value) {
    *last_info().lock().unwrap() = Some(info);
}

pub fn last_update_info() -> Option<serde_json::Value> {
    last_info().lock().unwrap().clone()
}

impl ShowApp {
    /// Startet eine Hintergrund-Prüfung (Block U4). Doppelte Checks laufen nicht.
    pub fn start_update_check(&mut self) {
        if matches!(
            self.update_state,
            UpdateUiState::Checking | UpdateUiState::Downloading
        ) {
            return;
        }
        self.update_state = UpdateUiState::Checking;
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            let res = match crate::update::fetch_update_check(&crate::update::feed_base()) {
                Ok(check) => {
                    if crate::update::version_newer(
                        &check.manifest.version,
                        crate::update::current_version(),
                    ) {
                        UpdateCheckResult::Available(check)
                    } else {
                        UpdateCheckResult::UpToDate
                    }
                }
                Err(e) => UpdateCheckResult::Failed(e),
            };
            let _ = tx.send(res);
        });
        self.update_rx = Some(rx);
    }

    /// Ergebnis des Hintergrund-Threads abholen (einmal pro Frame).
    pub fn poll_update_results(&mut self) {
        let Some(rx) = &self.update_rx else { return };
        let mut done = false;
        while let Ok(res) = rx.try_recv() {
            match res {
                UpdateCheckResult::UpToDate => {
                    self.update_state = UpdateUiState::UpToDate;
                    set_last_update_info(serde_json::json!({
                        "current": crate::update::current_version(),
                        "build": crate::update::build_info(),
                        "update_available": false,
                    }));
                }
                UpdateCheckResult::Available(check) => {
                    set_last_update_info(serde_json::json!({
                        "current": crate::update::current_version(),
                        "build": crate::update::build_info(),
                        "update_available": true,
                        "latest": check.manifest.version,
                        "verified": check.verified,
                    }));
                    self.update_state = UpdateUiState::Available(check);
                }
                UpdateCheckResult::Failed(e) => {
                    // Prüfungsfehler (z. B. offline) stören nie – nur als Status.
                    self.update_state = UpdateUiState::Failed(e);
                }
            }
            done = true;
        }
        if done {
            self.update_rx = None;
        }
    }

    /// Startet Download + Verifikation des Update-Pakets (Hintergrund-Thread).
    fn start_update_download(&mut self) {
        let UpdateUiState::Available(check) = self.update_state.clone() else {
            return;
        };
        if !check.verified {
            return; // unsigniert → nie installieren
        }
        self.update_state = UpdateUiState::Downloading;
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            let res = (|| -> Result<PathBuf, String> {
                let asset =
                    crate::update::select_asset(&check.manifest, crate::update::current_platform())
                        .ok_or_else(|| {
                            format!(
                                "Kein Paket für {} im Manifest",
                                crate::update::current_platform()
                            )
                        })?;
                let dir = std::env::temp_dir().join("kuhplay-update");
                crate::update::download_asset(asset, &dir)
            })();
            let _ = tx.send(res);
        });
        self.update_dl_rx = Some(rx);
    }

    /// Download-Ergebnis abholen; bei Erfolg → ReadyToInstall.
    pub fn poll_update_download(&mut self) {
        let Some(rx) = &self.update_dl_rx else { return };
        let mut done = false;
        while let Ok(res) = rx.try_recv() {
            match res {
                Ok(path) => self.update_state = UpdateUiState::ReadyToInstall(path),
                Err(e) => self.update_state = UpdateUiState::Failed(e),
            }
            done = true;
        }
        if done {
            self.update_dl_rx = None;
        }
    }

    /// true, wenn gerade Cues laufen oder Show-Mode aktiv ist (Update-Sperre).
    fn update_blocked_by_show(&self) -> bool {
        self.show_mode || self.carts.running_count() > 0
    }

    /// Update-Dialog zeichnen (Block U4).
    pub fn draw_update_window(&mut self, ctx: &egui::Context) {
        let mut open = !matches!(self.update_state, UpdateUiState::Idle)
            && !matches!(self.update_state, UpdateUiState::UpToDate);
        // UpToDate nur anzeigen, wenn der Nutzer explizit geprüft hat (manueller Check).
        if matches!(self.update_state, UpdateUiState::UpToDate) && !self.ui.update_dialog_open {
            return;
        }
        egui::Window::new("Update")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                match &self.update_state {
                    UpdateUiState::Idle | UpdateUiState::UpToDate => {
                        ui.label(format!(
                            "KuhPlay {} ist aktuell.",
                            crate::update::current_version()
                        ));
                    }
                    UpdateUiState::Checking => {
                        ui.label("Prüfe auf Updates …");
                    }
                    UpdateUiState::Downloading => {
                        ui.label("Lade Update herunter und prüfe Signatur …");
                    }
                    UpdateUiState::Failed(e) => {
                        ui.label(format!("Update-Prüfung fehlgeschlagen: {e}"));
                    }
                    UpdateUiState::Available(check) => {
                        ui.label(format!(
                            "Update verfügbar: {} (installiert: {})",
                            check.manifest.version,
                            crate::update::current_version()
                        ));
                        if check.verified {
                            ui.label("Signatur geprüft (Ed25519).");
                        } else {
                            ui.label(
                                "⚠ Manifest ist nicht signiert – Installation wird verweigert.",
                            );
                        }
                        if !check.manifest.notes.is_empty() {
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    ui.label(&check.manifest.notes);
                                });
                        }
                        ui.separator();
                        if self.update_blocked_by_show() {
                            ui.label(
                                "Während Show-Mode oder laufenden Cues wird kein Update \
                                 heruntergeladen oder installiert.",
                            );
                        } else if ui
                            .add_enabled(
                                check.verified,
                                egui::Button::new("Download & installieren"),
                            )
                            .clicked()
                        {
                            self.start_update_download();
                        }
                    }
                    UpdateUiState::ReadyToInstall(path) => {
                        let path = path.clone();
                        ui.label(format!(
                            "Update-Paket geprüft und geladen: {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        ui.label(
                            "Die Installation startet das Paket-Installationsprogramm; \
                             KuhPlay wird danach beendet und muss neu gestartet werden.",
                        );
                        if ui.button("Show speichern & installieren").clicked() {
                            self.save_show_dialog(ctx);
                            self.install_update_and_exit(path.clone());
                        }
                        if ui.button("Ohne Speichern installieren").clicked() {
                            self.install_update_and_exit(path);
                        }
                    }
                }
                ui.separator();
                if ui.button("Schließen").clicked() {
                    self.ui.update_dialog_open = false;
                }
            });
        if !open {
            self.ui.update_dialog_open = false;
            if matches!(
                self.update_state,
                UpdateUiState::UpToDate | UpdateUiState::Failed(_)
            ) {
                self.update_state = UpdateUiState::Idle;
            }
        }
    }

    /// Installer starten und App beenden (Update nach Bestätigung).
    fn install_update_and_exit(&self, path: PathBuf) {
        if let Err(e) = crate::update::run_installer(&path) {
            tracing::warn!("Update-Installer konnte nicht gestartet werden: {e}");
        }
        std::process::exit(0);
    }
}
