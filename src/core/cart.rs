//! Carts: mehrere unabhängige Cue-Listen (QLab-Pendant: Cart-Fenster).
//!
//! Ein [`CartManager`] verwaltet benannte [`CueList`]s. GO, Selektion und
//! Playhead wirken immer auf den **aktiven Cart**; alle anderen Carts bleiben
//! unangetastt (Isolation).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::error::{ShowError, ShowResult};
use crate::core::{Cue, CueList, CueStatus, GoOutcome};
use crate::engine::EngineCommand;

/// Verwaltet mehrere Cue-Listen (Carts) mit einem aktiven Cart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartManager {
    carts: Vec<CueList>,
    active: usize,
}

impl Default for CartManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CartManager {
    /// Neuer Manager mit einem Cart („Cart 1").
    pub fn new() -> Self {
        Self {
            carts: vec![CueList::with_name("Cart 1")],
            active: 0,
        }
    }

    /// Fügt einen Cart hinzu und macht ihn aktiv. Rückgabe: Index.
    pub fn add_cart(&mut self, name: impl Into<String>) -> usize {
        self.carts.push(CueList::with_name(name));
        let index = self.carts.len() - 1;
        self.active = index;
        index
    }

    /// Entfernt einen Cart. Der letzte Cart kann nicht entfernt werden.
    pub fn remove_cart(&mut self, index: usize) -> ShowResult<()> {
        if self.carts.len() <= 1 {
            return Err(ShowError::InvalidParameter(
                "the last cart cannot be removed".to_string(),
            ));
        }
        if index >= self.carts.len() {
            return Err(ShowError::InvalidParameter(format!(
                "cart index {index} out of bounds ({} carts)",
                self.carts.len()
            )));
        }
        self.carts.remove(index);
        if self.active >= self.carts.len() {
            self.active = self.carts.len() - 1;
        }
        Ok(())
    }

    /// Setzt den aktiven Cart.
    pub fn set_active(&mut self, index: usize) -> ShowResult<()> {
        if index >= self.carts.len() {
            return Err(ShowError::InvalidParameter(format!(
                "cart index {index} out of bounds ({} carts)",
                self.carts.len()
            )));
        }
        self.active = index;
        Ok(())
    }

    /// Index des aktiven Carts.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Cart nach Index (z. B. für UI-Caches über alle Carts).
    pub fn cart(&self, index: usize) -> &CueList {
        &self.carts[index]
    }

    /// Aktiver Cart (nur lesend).
    pub fn active(&self) -> &CueList {
        &self.carts[self.active]
    }

    /// Aktiver Cart (mutabel).
    /// Cart per Index (mutabel).
    pub fn cart_mut(&mut self, index: usize) -> &mut CueList {
        &mut self.carts[index]
    }

    pub fn active_mut(&mut self) -> &mut CueList {
        &mut self.carts[self.active]
    }

    /// Anzahl der Carts.
    pub fn cart_count(&self) -> usize {
        self.carts.len()
    }

    /// Name eines Carts.
    pub fn cart_name(&self, index: usize) -> &str {
        self.carts[index].name()
    }

    // ------------------------------------------------------------------
    // Delegation an den aktiven Cart (GO, Playhead, Selektion, Status)
    // ------------------------------------------------------------------

    /// GO im aktiven Cart.
    pub fn go(&mut self) -> Option<GoOutcome> {
        self.active_mut().go()
    }

    /// GO im aktiven Cart inkl. Auto-Continue-Ketten (QLab „Cue Sequences").
    pub fn go_chained(&mut self) -> Option<GoOutcome> {
        self.active_mut().go_chained()
    }

    /// GO auf einen Cue per Q-Nummer im aktiven Cart (Remote-Steuerung).
    pub fn go_to_number(&mut self, number: &str) -> Option<GoOutcome> {
        self.active_mut().go_to_number(number)
    }

    /// Setzt den Armed-Zustand eines Cues per Q-Nummer im aktiven Cart
    /// (Remote-Steuerung).
    pub fn set_armed_by_number(&mut self, number: &str, armed: bool) -> bool {
        self.active_mut().set_armed_by_number(number, armed)
    }

    /// Anzahl Cues eines beliebigen Carts (für Sidebar „Lists & Carts").
    pub fn cart_cue_count(&self, index: usize) -> usize {
        self.carts[index].len()
    }

    /// Cues des aktiven Carts (nur lesend).
    pub fn cues(&self) -> &[Cue] {
        self.active().cues()
    }

    /// Cues des aktiven Carts (mutabel).
    pub fn cues_mut(&mut self) -> &mut Vec<Cue> {
        self.active_mut().cues_mut()
    }

    /// Anzahl der Cues im aktiven Cart.
    pub fn len(&self) -> usize {
        self.active().len()
    }

    /// `true`, wenn der aktive Cart leer ist.
    pub fn is_empty(&self) -> bool {
        self.active().is_empty()
    }

    /// Playhead des aktiven Carts.
    pub fn cursor(&self) -> usize {
        self.active().cursor()
    }

    /// Setzt den Playhead des aktiven Carts (Operator-Eingriff).
    pub fn set_cursor(&mut self, index: usize) {
        self.active_mut().set_cursor(index);
    }

    /// Anzahl aktiver Cues (Pre-Wait/Playing/Paused/Post-Wait).
    pub fn running_count(&self) -> usize {
        self.active().running_count()
    }

    /// Nächster Standby-Cue des aktiven Carts.
    pub fn standby(&self) -> Option<&Cue> {
        self.active().standby()
    }

    /// Nächster Standby-Cue des aktiven Carts (mutabel).
    pub fn standby_mut(&mut self) -> Option<&mut Cue> {
        self.active_mut().standby_mut()
    }

    /// Selektion im aktiven Cart.
    pub fn select(&mut self, index: Option<usize>) {
        self.active_mut().select(index);
    }

    /// Selektion im aktiven Cart per Cue-ID (auch Group-Kind, Phase 3).
    pub fn select_id(&mut self, id: Option<Uuid>) {
        self.active_mut().select_id(id);
    }

    /// Alle selektierten Cue-IDs im aktiven Cart (Mehrfachauswahl).
    pub fn selection(&self) -> Vec<Uuid> {
        self.active().selection().to_vec()
    }

    /// Index des selektierten Cues.
    pub fn selected_index(&self) -> Option<usize> {
        self.active().selected_index()
    }

    /// Selektierter Cue (nur lesend).
    pub fn selected(&self) -> Option<&Cue> {
        self.active().selected()
    }

    /// Selektierter Cue (mutabel).
    pub fn selected_mut(&mut self) -> Option<&mut Cue> {
        self.active_mut().selected_mut()
    }

    /// Sucht einen Cue rekursiv im aktiven Cart.
    pub fn find_cue(&self, id: Uuid) -> Option<&Cue> {
        self.active().find_cue(id)
    }

    /// Sucht einen Cue rekursiv über ALLE Carts (z. B. für Engine-Events,
    /// die auch Cues in nicht-aktiven Carts betreffen können).
    /// Benennt eine Stage in allen Cues um (auch in Gruppen).
    pub fn rename_stage(&mut self, old: &str, new: &str) {
        fn walk(cues: &mut [Cue], old: &str, new: &str) {
            for cue in cues {
                if cue.stage.as_deref() == Some(old) {
                    cue.stage = Some(new.to_string());
                }
                walk(&mut cue.children, old, new);
            }
        }
        for cart in &mut self.carts {
            walk(cart.cues_mut(), old, new);
        }
    }

    /// `true`, wenn irgendein Cue die Stage nutzt (auch in Gruppen).
    pub fn stage_in_use(&self, name: &str) -> bool {
        fn used(cues: &[Cue], name: &str) -> bool {
            cues.iter()
                .any(|cue| cue.stage.as_deref() == Some(name) || used(&cue.children, name))
        }
        self.carts.iter().any(|cart| used(cart.cues(), name))
    }

    /// Sucht einen Cue über alle Carts, veränderbar.
    pub fn find_cue_any_mut(&mut self, id: Uuid) -> Option<&mut Cue> {
        self.carts.iter_mut().find_map(|cart| cart.find_cue_mut(id))
    }

    /// Sucht einen Cue über alle Carts (auch in Gruppen).
    /// Externer Timecode an alle Carts: feuert Timecode-Trigger-Cues
    /// (siehe [`CueList::receive_timecode`]). Rückgabe: alle Commands.
    pub fn receive_timecode(&mut self, tc: crate::core::Timecode) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        for index in 0..self.cart_count() {
            let list = self.cart_mut(index);
            commands.extend(list.receive_timecode(tc));
        }
        commands
    }

    pub fn find_cue_any(&self, id: Uuid) -> Option<&Cue> {
        self.carts.iter().find_map(|cart| cart.find_cue(id))
    }

    /// Setzt den Status eines Cues (State-Machine) – der Cue wird per UUID
    /// über ALLE Carts gesucht (Engine-Events betreffen auch Cues in
    /// nicht-aktiven Carts).
    pub fn set_status(&mut self, id: Uuid, status: CueStatus) -> ShowResult<()> {
        for cart in &mut self.carts {
            if cart.find_cue(id).is_some() {
                return cart.set_status(id, status);
            }
        }
        Err(ShowError::CueNotFound(id.to_string()))
    }

    /// Setzt den Status und triggert ggf. Auto-Follow – im Cart, der den
    /// Cue enthält (Auto-Follow feuert in der Liste des Cues, nicht im
    /// aktiven Cart).
    pub fn set_status_with_follow(
        &mut self,
        id: Uuid,
        status: CueStatus,
    ) -> (ShowResult<()>, Vec<EngineCommand>) {
        for cart in &mut self.carts {
            if cart.find_cue(id).is_some() {
                return cart.set_status_with_follow(id, status);
            }
        }
        (Err(ShowError::CueNotFound(id.to_string())), Vec::new())
    }

    /// Stoppt alle laufenden Cues des aktiven Carts (Panic).
    pub fn stop_all_running(&mut self) {
        self.active_mut().stop_all_running();
    }

    /// Stoppt einen einzelnen laufenden Cue (inkl. Kinder) – über alle Carts
    /// gesucht (Remote „Stop Cue" darf auch Cues anderer Carts treffen).
    pub fn stop_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        for cart in &mut self.carts {
            if cart.find_cue(id).is_some() {
                return cart.stop_running(id);
            }
        }
        Vec::new()
    }

    /// Pausiert einen laufenden Cue (inkl. Kinder) – über alle Carts gesucht.
    pub fn pause_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        for cart in &mut self.carts {
            if cart.find_cue(id).is_some() {
                return cart.pause_running(id);
            }
        }
        Vec::new()
    }

    /// Setzt einen pausierten Cue fort (inkl. Kinder) – über alle Carts gesucht.
    pub fn resume_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        for cart in &mut self.carts {
            if cart.find_cue(id).is_some() {
                return cart.resume_running(id);
            }
        }
        Vec::new()
    }

    /// Reset des aktiven Carts (Playhead + alle Status zurück).
    pub fn reset(&mut self) {
        self.active_mut().reset();
    }

    /// Reset ALLER Carts (QLab Sidebar: Reset wirkt workspace-weit).
    pub fn reset_all(&mut self) {
        for cart in &mut self.carts {
            cart.reset();
        }
    }

    /// Pausiert alle laufenden Cues in ALLEN Carts (QLab: „Pause All").
    pub fn pause_all_running(&mut self) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        for cart in &mut self.carts {
            commands.extend(cart.pause_all_running());
        }
        commands
    }

    /// Setzt alle pausierten Cues in ALLEN Carts fort (QLab: „Resume All").
    pub fn resume_all_running(&mut self) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        for cart in &mut self.carts {
            commands.extend(cart.resume_all_running());
        }
        commands
    }

    /// Stoppt alle laufenden Cues in ALLEN Carts hart (QLab: „Panic" wirkt workspace-weit).
    /// Das zugehörige `EngineCommand::StopAll` wird vom Aufrufer separat gesendet.
    pub fn stop_all_running_all(&mut self) {
        for cart in &mut self.carts {
            cart.stop_all_running();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ContinueMode, CueType};

    fn audio(number: &str) -> Cue {
        Cue::new(number, number, CueType::Audio)
    }

    #[test]
    fn add_switch_and_remove_carts() {
        let mut mgr = CartManager::new();
        assert_eq!(mgr.cart_count(), 1);

        let idx = mgr.add_cart("Cart 2");
        assert_eq!(idx, 1);
        assert_eq!(mgr.active_index(), 1, "new cart becomes active");

        mgr.set_active(0).unwrap();
        assert_eq!(mgr.active_index(), 0);
        assert!(mgr.set_active(99).is_err());

        mgr.remove_cart(1).unwrap();
        assert_eq!(mgr.cart_count(), 1);
        assert!(mgr.remove_cart(0).is_err(), "last cart must not be removed");
    }

    #[test]
    fn go_delegates_to_active_cart() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        let other = mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("A"));

        // Cart 2 aktiv: GO triggert "A", nicht "1".
        let outcome = mgr.go().expect("go in active cart");
        assert_eq!(mgr.cues()[outcome.index].number, "A");
        let _ = other;

        // Cart 1 bleibt unangetastt.
        mgr.set_active(0).unwrap();
        assert_eq!(mgr.cues()[0].status, CueStatus::Standby);
    }

    #[test]
    fn status_events_reach_cues_in_inactive_carts() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("A"));
        let id_a = mgr.active().cues()[0].id;

        // "A" starten (Cart 2 aktiv) und auf Cart 1 wechseln.
        let _ = mgr.go().unwrap();
        mgr.set_active(0).unwrap();

        // Engine-Event (Finished → Completed) für "A": Status landet in Cart 2.
        mgr.set_status(id_a, CueStatus::Completed).unwrap();
        assert!(
            mgr.find_cue(id_a).is_none(),
            "aktiver Cart 1 kennt den Cue nicht"
        );
        let found = mgr
            .find_cue_any(id_a)
            .expect("find_cue_any findet über alle Carts");
        assert_eq!(found.status, CueStatus::Completed);

        // Unbekannte UUID → Fehler (wie zuvor).
        assert!(mgr
            .set_status(Uuid::new_v4(), CueStatus::Completed)
            .is_err());
    }

    #[test]
    fn auto_follow_fires_in_the_cues_own_cart() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("A"));
        mgr.active_mut().add(audio("B"));
        let id_a = mgr.active().cues()[0].id;
        let id_b = mgr.active().cues()[1].id;
        mgr.active_mut().cues_mut()[0].continue_mode = ContinueMode::AutoFollow;

        // "A" läuft (Cart 2 aktiv), dann Cart 1 aktivieren.
        // Auto-Follow feuert synchron in go(): "B" startet sofort – in Cart 2
        // (nicht im aktiven Cart 1). Review B6/B7.
        let outcome = mgr.go().unwrap();
        mgr.set_active(0).unwrap();
        assert!(
            outcome
                .commands
                .iter()
                .any(|c| matches!(c, EngineCommand::Play { cue_id, .. } if *cue_id == id_b)),
            "Auto-Follow triggert B beim Start (in go())"
        );
        assert_eq!(mgr.find_cue_any(id_b).unwrap().status, CueStatus::Playing);
        // Wiederholtes Playing-Event → kein Doppel-Feuern.
        let (result, commands) = mgr.set_status_with_follow(id_a, CueStatus::Playing);
        assert!(result.is_ok());
        assert!(
            commands.is_empty(),
            "kein Doppel-Feuern beim wiederholten Start"
        );
        // Cart 1 wurde nicht berührt.
        assert_eq!(mgr.active().cues()[0].status, CueStatus::Standby);
    }

    #[test]
    fn stop_running_reaches_inactive_carts() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("A"));
        let id_a = mgr.active().cues()[0].id;

        let _ = mgr.go().unwrap(); // "A" läuft in Cart 2
        mgr.set_active(0).unwrap(); // Cart 1 aktiv

        // Stop (z. B. Remote „Stop Cue") trifft den Cue in Cart 2.
        let commands = mgr.stop_running(id_a);
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, EngineCommand::Stop { cue_id } if *cue_id == id_a)),
            "Stop-Command für den Cue in Cart 2"
        );
        assert_eq!(mgr.find_cue_any(id_a).unwrap().status, CueStatus::Stopped);

        // Unbekannte UUID → keine Commands, kein Fehler.
        assert!(mgr.stop_running(Uuid::new_v4()).is_empty());
    }

    #[test]
    fn carts_are_isolated() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("2"));

        let _ = mgr.go().unwrap(); // "2" läuft
        assert_eq!(mgr.cues()[0].status, CueStatus::Playing);

        mgr.set_active(0).unwrap();
        assert_eq!(
            mgr.cues()[0].status,
            CueStatus::Standby,
            "other cart untouched"
        );
        assert_eq!(mgr.cursor(), 0);
    }

    #[test]
    fn cart_manager_serde_roundtrip() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.add_cart("Cart 2");
        let json = serde_json::to_string(&mgr).unwrap();
        let back: CartManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cart_count(), 2);
        assert_eq!(back.active_index(), 1);
        assert_eq!(back.cart_name(0), "Cart 1");
    }

    #[test]
    fn running_controls_delegate_to_active_cart() {
        let mut mgr = CartManager::new();
        mgr.active_mut().add(audio("1"));
        mgr.active_mut().add(audio("2"));
        mgr.add_cart("Cart 2");
        mgr.active_mut().add(audio("A"));
        mgr.active_mut().add(audio("B"));

        let _ = mgr.go().unwrap(); // "A" läuft
        let _ = mgr.go().unwrap(); // "B" läuft
        assert_eq!(mgr.running_count(), 2);

        // Einzel-Stop wirkt nur im aktiven Cart.
        let id_a = mgr.cues()[0].id;
        let commands = mgr.stop_running(id_a);
        assert_eq!(commands.len(), 1);
        assert_eq!(mgr.cues()[0].status, CueStatus::Stopped);
        assert_eq!(mgr.running_count(), 1);

        // Playhead setzen + Pause/Resume-Delegation.
        mgr.set_cursor(0);
        assert_eq!(mgr.cursor(), 0);
        let id_b = mgr.cues()[1].id;
        assert_eq!(mgr.pause_running(id_b).len(), 1);
        assert_eq!(mgr.cues()[1].status, CueStatus::Paused);
        assert_eq!(mgr.resume_running(id_b).len(), 1);
        assert_eq!(mgr.cues()[1].status, CueStatus::Playing);

        // Anderer Cart unangetastet.
        mgr.set_active(0).unwrap();
        assert_eq!(mgr.cues()[0].status, CueStatus::Standby);
    }
}
