//! [`CueList`]: lineare Cue-Liste mit Playhead, GO-Logik und Group-Kaskadierung.
//!
//! Die Liste kennt weder UI noch Engine – sie erzeugt lediglich
//! [`EngineCommand`]s ([`GoOutcome`]), die der Aufrufer an die Engine sendet.
//!
//! ## Playhead-Modell
//!
//! * `cursor` zeigt auf das *nächste* Standby-Cue (ab dort wird gesucht).
//! * [`CueList::go`] triggert das nächste armed Standby-Cue ab `cursor` und
//!   rückt den Playhead automatisch weiter.
//! * **Sequential/Loop/Shuffle-Gruppen** splicen ihre Kinder an die Cursor-
//!   Position in die Liste ein (jedes GO triggert das nächste Kind).
//! * **Simultaneous/Timeline-Gruppen** triggern alle armed Kinder in einem GO.
//! * **Control-Cues** wirken auf die Liste selbst (Start/Stop/Pause/Load/
//!   Reset/Devamp/GoTo/Arm/Disarm) und sind Sofort-Cues.

/// Maximale Verschachtelung von Control-Cue-Aktionen (Zyklus-Guard, Review G0).
const MAX_CONTROL_DEPTH: u32 = 32;

thread_local! {
    /// Rekursionstiefe laufender Control-Cue-Aktionen (Zyklus-Erkennung).
    static CONTROL_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::error::{ShowError, ShowResult};
use crate::core::slideshow::SlideNav;
use crate::core::Timecode;
use crate::core::{ContinueMode, ControlAction, Cue, CueStatus, CueType, GroupMode};
use crate::engine::EngineCommand;

fn default_cart_name() -> String {
    "Cart 1".to_string()
}

/// Ausgangszustand eines GO: Index des getriggerten Cues + erzeugte Commands.
#[derive(Debug, Clone)]
pub struct GoOutcome {
    /// Index des getriggerten (Top-Level-)Cues in der Liste.
    pub index: usize,
    /// Engine-Commands, die an die Playback-Engine gesendet werden sollen.
    pub commands: Vec<EngineCommand>,
}

/// Lineare Cue-Liste mit Playhead und GO-Trigger (Command-Pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueList {
    cues: Vec<Cue>,
    /// Selektion: UUIDs der selektierten Cues (inkl. Group-Kinder,
    /// docs/TODO-offen.md Phase 3 Item 7). Reihenfolge = Klick-Reihenfolge; die
    /// „primäre" Selektion (Inspector) ist der zuletzt geklickte Cue.
    /// UUID-basiert: Selektion überlebt Einfügen/Löschen/Verschieben.
    #[serde(default)]
    selection: Vec<Uuid>,
    /// Anker für ⇧-Klick (Bereichsauswahl); wird bei jedem Klick gesetzt.
    #[serde(default)]
    selection_anchor: Option<Uuid>,
    /// Playhead: Index des nächsten zu triggernden Cues.
    cursor: usize,
    /// Name des Carts (für [`crate::core::CartManager`]).
    #[serde(default = "default_cart_name")]
    name: String,
}

impl Default for CueList {
    fn default() -> Self {
        Self {
            cues: Vec::new(),
            selection: Vec::new(),
            selection_anchor: None,
            cursor: 0,
            name: default_cart_name(),
        }
    }
}

impl CueList {
    /// Neue, leere Cue-Liste.
    pub fn new() -> Self {
        Self::default()
    }

    /// Neue, leere Cue-Liste mit Namen (Cart).
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Name des Carts.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Hängt einen Cue an.
    pub fn add(&mut self, cue: Cue) {
        self.cues.push(cue);
    }

    /// Fügt einen Cue an `index` ein (geklemmt auf `0..=len`).
    /// Der Playhead wird verschoben, wenn er auf oder hinter der
    /// Einfügeposition liegt (zeigt dann weiter auf denselben Cue).
    /// Die Selektion ist UUID-basiert und braucht keine Korrektur.
    pub fn insert(&mut self, index: usize, cue: Cue) {
        let index = index.min(self.cues.len());
        self.cues.insert(index, cue);
        if self.cursor >= index {
            self.cursor += 1;
        }
    }

    /// Entfernt einen Cue (Top-Level) anhand seines Index.
    pub fn remove(&mut self, index: usize) -> ShowResult<()> {
        if index >= self.cues.len() {
            return Err(ShowError::CueNotFound(format!("index {index}")));
        }
        let removed_id = self.cues[index].id;
        self.cues.remove(index);
        // Playhead hinter dem entfernten Cue korrigieren; die Selektion
        // verliert die entfernte ID (UUIDs sonst stabil).
        if self.cursor > index {
            self.cursor -= 1;
        }
        self.selection.retain(|id| *id != removed_id);
        if self.selection_anchor == Some(removed_id) {
            self.selection_anchor = None;
        }
        Ok(())
    }

    /// Entfernt einen Cue anhand seiner ID – Top-Level-Cue oder Group-Kind
    /// (docs/TODO-offen.md Phase 3 Item 7: Kinder sind selektierbar/löschbar).
    /// Rückgabe: `true`, wenn ein Cue entfernt wurde.
    pub fn remove_by_id(&mut self, id: Uuid) -> bool {
        if let Some(index) = self.index_of_id(id) {
            let _ = self.remove(index);
            return true;
        }
        // Group-Kind: aus der Kinder-Liste der Eltern-Gruppe entfernen.
        for cue in &mut self.cues {
            if let Some(pos) = cue.children.iter().position(|c| c.id == id) {
                cue.children.remove(pos);
                self.selection.retain(|sid| *sid != id);
                if self.selection_anchor == Some(id) {
                    self.selection_anchor = None;
                }
                return true;
            }
        }
        false
    }

    /// Nächste freie Q-Nummer (höchste vorhandene Nummer + 1).
    ///
    /// Rekursive Suche über alle Cues (inkl. Group-Kinder); die führenden
    /// Ziffern der Nummer werden verglichen („5.1" zählt als 5). Nicht-
    /// numerische Nummern werden ignoriert. Bei leerer Liste → 1. Anders
    /// als `len() + 1` entstehen so auch nach Löschen keine doppelten
    /// Nummern (dieselbe Nummer wird nie zweimal vergeben).
    pub fn next_free_number(&self) -> u32 {
        fn leading_number(number: &str) -> Option<u32> {
            let digits: String = number.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse().ok()
            }
        }
        fn scan(cues: &[Cue], max: &mut u32) {
            for cue in cues {
                if let Some(n) = leading_number(&cue.number) {
                    *max = (*max).max(n);
                }
                scan(&cue.children, max);
            }
        }
        let mut max = 0;
        scan(&self.cues, &mut max);
        max.saturating_add(1)
    }

    /// Anzahl der Top-Level-Cues.
    pub fn len(&self) -> usize {
        self.cues.len()
    }

    /// `true`, wenn die Liste leer ist.
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    /// Alle Top-Level-Cues (nur lesend).
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// Alle Top-Level-Cues (mutabel, z. B. für UI-Editing).
    pub fn cues_mut(&mut self) -> &mut Vec<Cue> {
        &mut self.cues
    }

    /// Playhead-Position (Index des nächsten Cues).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Setzt den Playhead direkt (Operator-Eingriff: Klick auf die Cue-Nummer
    /// oder Pfeiltasten in der UI). Wird auf `0..=len` geklemmt – `len`
    /// bedeutet „Playhead hinter dem letzten Cue" (GO triggert nichts).
    ///
    /// Beendete Cues ab dem Playhead werden wieder bereit: GO läuft von hier in
    /// Listenreihenfolge weiter und springt nicht über schon gespielte Cues.
    /// Laufende Cues bleiben unberührt.
    pub fn set_cursor(&mut self, index: usize) {
        self.cursor = index.min(self.cues.len());
        for cue in self.cues.iter_mut().skip(self.cursor) {
            if matches!(
                cue.status,
                CueStatus::Completed | CueStatus::Stopped | CueStatus::Error
            ) {
                cue.reset_status_recursive();
            }
        }
    }

    /// Anzahl aktiver Cues (Pre-Wait/Playing/Paused/Post-Wait).
    pub fn running_count(&self) -> usize {
        self.cues.iter().filter(|c| c.status.is_active()).count()
    }

    /// Nächster Standby-Cue ab dem Playhead (für "Next:"-Anzeige).
    pub fn standby(&self) -> Option<&Cue> {
        self.cues
            .iter()
            .skip(self.cursor)
            .find(|c| c.is_armed && c.status == CueStatus::Standby)
    }

    /// Nächster Standby-Cue ab dem Playhead (mutabel, z. B. für das
    /// Notes-Feld im Masthead).
    pub fn standby_mut(&mut self) -> Option<&mut Cue> {
        self.cues
            .iter_mut()
            .skip(self.cursor)
            .find(|c| c.is_armed && c.status == CueStatus::Standby)
    }

    // ------------------------------------------------------------------
    // Selektion (UUID-basiert, Mehrfachauswahl; docs/TODO-offen.md Phase 3)
    // ------------------------------------------------------------------

    /// Setzt die Selektion auf einen einzelnen Top-Level-Cue
    /// (`None` = Selektion löschen).
    pub fn select(&mut self, index: Option<usize>) {
        match index.filter(|i| *i < self.cues.len()) {
            Some(i) => {
                let id = self.cues[i].id;
                self.select_id(Some(id));
            }
            None => self.select_id(None),
        }
    }

    /// Setzt die Selektion auf einen einzelnen Cue per ID (auch Group-Kind;
    /// `None` = Selektion löschen).
    pub fn select_id(&mut self, id: Option<Uuid>) {
        self.selection = match id {
            Some(id) if self.find_cue(id).is_some() => vec![id],
            _ => Vec::new(),
        };
        self.selection_anchor = id.filter(|_| !self.selection.is_empty());
    }

    /// ⌘-Klick: Cue zur Selektion hinzufügen/entfernen.
    pub fn toggle_select_id(&mut self, id: Uuid) {
        if self.find_cue(id).is_none() {
            return;
        }
        if let Some(pos) = self.selection.iter().position(|s| *s == id) {
            self.selection.remove(pos);
        } else {
            self.selection.push(id);
        }
        self.selection_anchor = Some(id);
    }

    /// ⇧-Klick: Bereich von Anker bis `id` selektieren (Top-Level-Bereich).
    /// Ohne Anker wie ein einfacher Klick.
    pub fn select_range_to(&mut self, id: Uuid) {
        let (a, b) = match (
            self.selection_anchor.and_then(|a| self.index_of_id(a)),
            self.index_of_id(id),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                self.select_id(Some(id));
                return;
            }
        };
        let (lo, hi) = (a.min(b), a.max(b));
        let mut ids = Vec::new();
        for cue in &self.cues[lo..=hi] {
            ids.push(cue.id);
        }
        self.selection = ids;
    }

    /// Alle selektierten Cue-IDs (Klick-Reihenfolge, primär zuletzt).
    pub fn selection(&self) -> &[Uuid] {
        &self.selection
    }

    /// Anzahl der selektierten Cues.
    pub fn selection_len(&self) -> usize {
        self.selection.len()
    }

    /// Ist der Cue (Top-Level oder Group-Kind) selektiert?
    pub fn is_selected(&self, id: Uuid) -> bool {
        self.selection.contains(&id)
    }

    /// ID der primären Selektion (zuletzt geklickter Cue; Inspector-Quelle).
    pub fn primary_selection(&self) -> Option<Uuid> {
        self.selection.last().copied()
    }

    /// Index der primären Selektion in der Top-Level-Liste: direkt, wenn
    /// Top-Level-Cue; bei einem Group-Kind der Index der Eltern-Gruppe.
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.primary_selection()?;
        Some(
            self.index_of_id(id)
                .unwrap_or_else(|| self.parent_group_index(id).unwrap_or(0)),
        )
    }

    /// Einfügeposition für „Add Cue" (nach der Selektion; bei einem
    /// selektierten Group-Kind nach der Eltern-Gruppe).
    pub fn selected_insert_index(&self) -> usize {
        self.selected_index()
            .map(|i| (i + 1).min(self.cues.len()))
            .unwrap_or(self.cues.len())
    }

    /// Primär selektierter Cue (nur lesend; auch Group-Kind).
    pub fn selected(&self) -> Option<&Cue> {
        self.primary_selection().and_then(|id| self.find_cue(id))
    }

    /// Primär selektierter Cue (mutabel; auch Group-Kind).
    pub fn selected_mut(&mut self) -> Option<&mut Cue> {
        let id = self.primary_selection()?;
        self.find_cue_mut(id)
    }

    /// Index eines Top-Level-Cues anhand seiner ID.
    pub fn index_of_id(&self, id: Uuid) -> Option<usize> {
        self.cues.iter().position(|c| c.id == id)
    }

    /// Index der Eltern-Gruppe eines Group-Kinds (Top-Level-Index).
    fn parent_group_index(&self, child_id: Uuid) -> Option<usize> {
        self.cues
            .iter()
            .position(|c| c.children.iter().any(|ch| ch.id == child_id))
    }

    /// Sucht einen Cue rekursiv (inkl. Group-Kinder und eingespielter Kinder) per ID.
    pub fn find_cue(&self, id: Uuid) -> Option<&Cue> {
        self.cues.iter().find_map(|c| c.find(id))
    }

    /// Mutable Variante von [`CueList::find_cue`].
    pub fn find_cue_mut(&mut self, id: Uuid) -> Option<&mut Cue> {
        self.cues.iter_mut().find_map(|c| c.find_mut(id))
    }

    /// Verschiebt einen Cue um eine Position nach oben.
    /// Der Playhead folgt seinem Cue (die UUID-Selektion braucht keine
    /// Korrektur). Rückgabe: `true` bei Erfolg.
    pub fn move_up(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.cues.len() {
            return false;
        }
        self.cues.swap(index - 1, index);
        if self.cursor == index {
            self.cursor = index - 1;
        } else if self.cursor == index - 1 {
            self.cursor = index;
        }
        true
    }

    /// Verschiebt einen Cue um eine Position nach unten (siehe [`CueList::move_up`]).
    pub fn move_down(&mut self, index: usize) -> bool {
        if index + 1 >= self.cues.len() {
            return false;
        }
        self.cues.swap(index, index + 1);
        if self.cursor == index {
            self.cursor = index + 1;
        } else if self.cursor == index + 1 {
            self.cursor = index;
        }
        true
    }

    // ------------------------------------------------------------------
    // Edit-Operationen (Kontextmenü / Drag & Drop; docs/TODO-offen.md Phase 3)
    // ------------------------------------------------------------------

    /// Verschiebt einen Top-Level-Cue an die Position `to` (Drag & Drop
    /// zum Umsortieren). Der Playhead folgt dem verschobenen Cue.
    pub fn move_to(&mut self, from: usize, to: usize) {
        if from >= self.cues.len() {
            return;
        }
        let cue = self.cues.remove(from);
        let to = to.min(self.cues.len());
        self.cues.insert(to, cue);
        // Playhead-Korrektur: folgt dem Cue, sonst Positionen angleichen.
        if self.cursor == from {
            self.cursor = to;
        } else {
            let mut c = self.cursor;
            if c > from {
                c -= 1;
            }
            if c >= to {
                c += 1;
            }
            self.cursor = c.min(self.cues.len());
        }
    }

    /// Verschiebt einen Top-Level-Cue als letztes Kind in eine Gruppe
    /// (Drag & Drop auf eine Gruppen-Zeile).
    pub fn move_into_group(&mut self, cue_index: usize, group_index: usize) -> bool {
        if cue_index >= self.cues.len() || group_index >= self.cues.len() {
            return false;
        }
        if cue_index == group_index || self.cues[group_index].cue_type != CueType::Group {
            return false;
        }
        let cue = self.cues.remove(cue_index);
        if self.cursor > cue_index {
            self.cursor -= 1;
        }
        // Gruppen-Index nach dem Entfernen korrigieren.
        let group_index = if group_index > cue_index {
            group_index - 1
        } else {
            group_index
        };
        let moved_id = cue.id;
        self.cues[group_index].children.push(cue);
        self.select_id(Some(moved_id));
        true
    }

    /// Dupliziert einen Top-Level-Cue (tiefe Kopie inkl. Group-Kinder mit
    /// frischen UUIDs, „Duplicate"). Rückgabe: Index der Kopie.
    pub fn duplicate(&mut self, index: usize) -> Option<usize> {
        if index >= self.cues.len() {
            return None;
        }
        let mut copy = self.cues[index].clone();
        reassign_ids(&mut copy);
        copy.number = self.next_free_number().to_string();
        copy.status = CueStatus::Standby;
        copy.parent_group = None;
        copy.group_iterations_done = 0;
        let new_id = copy.id;
        let at = (index + 1).min(self.cues.len());
        self.insert(at, copy);
        self.select_id(Some(new_id));
        Some(at)
    }

    /// Dupliziert mehrere Cues per ID (Kontextmenü „Duplicate" bei
    /// Mehrfachauswahl). Rückgabe: IDs der Kopien (in List-Reihenfolge).
    pub fn duplicate_ids(&mut self, ids: &[Uuid]) -> Vec<Uuid> {
        // Indizes aufsteigend sammeln, dann ABSTEIGEND duplizieren – so
        // bleiben die früheren Indizes bis zur jeweiligen Operation gültig.
        let mut indices: Vec<usize> = ids.iter().filter_map(|id| self.index_of_id(*id)).collect();
        indices.sort_unstable();
        indices.dedup();
        let mut copies = Vec::new();
        for index in indices.into_iter().rev() {
            if let Some(new_index) = self.duplicate(index) {
                copies.push(self.cues[new_index].id);
            }
        }
        copies.reverse(); // wieder List-Reihenfolge
        self.selection = copies.clone();
        self.selection_anchor = copies.last().copied();
        copies
    }

    /// Gruppiert Top-Level-Cues per ID zu einem neuen Group-Cue
    /// („Group Cues"; Kontextmenü). Die Gruppe landet an der Position des
    /// ersten gruppierten Cues und wird selektiert. Kind-Nummern bleiben
    /// erhalten. Rückgabe: ID der neuen Gruppe.
    pub fn group_ids(&mut self, ids: &[Uuid]) -> Option<Uuid> {
        let mut indices: Vec<usize> = ids.iter().filter_map(|id| self.index_of_id(*id)).collect();
        indices.sort_unstable();
        indices.dedup();
        if indices.is_empty() {
            return None;
        }
        // Cues in umgekehrter Reihenfolge entnehmen (Indizes bleiben gültig).
        let mut children = Vec::new();
        for index in indices.iter().rev() {
            children.push(self.cues.remove(*index));
        }
        children.reverse(); // wieder List-Reihenfolge
        let at = indices[0];
        let number = self.next_free_number().to_string();
        let group = Cue::new_group(number, "New Group", GroupMode::Simultaneous, children);
        let group_id = group.id;
        self.insert(at, group);
        self.select_id(Some(group_id));
        Some(group_id)
    }

    /// Group-Cue ein-/ausklappen (Aufklapp-Dreieck, docs/TODO-offen.md Phase 3).
    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) {
        if let Some(cue) = self.cues.get_mut(index) {
            if cue.cue_type == CueType::Group {
                cue.collapsed = collapsed;
            }
        }
    }

    /// Group-Cue eingeklappt?
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.cues
            .get(index)
            .is_some_and(|c| c.cue_type == CueType::Group && c.collapsed)
    }

    // ------------------------------------------------------------------
    // GO / Trigger (Command-Pattern)
    // ------------------------------------------------------------------

    /// GO: Triggert das nächste armed Standby-Cue ab dem Playhead.
    ///
    /// Der Playhead rückt automatisch weiter. Liefert den
    /// Index des getriggerten Cues und die zu sendenden Engine-Commands.
    /// `None`, wenn kein Cue in Standby ist.
    pub fn go(&mut self) -> Option<GoOutcome> {
        let index = self
            .cues
            .iter()
            .enumerate()
            .skip(self.cursor)
            .find(|(_, c)| c.is_armed && c.status == CueStatus::Standby)
            .map(|(i, _)| i)?;
        let id = self.cues[index].id;
        self.cursor = index + 1;
        self.select_id(Some(id));

        let mut commands = Vec::new();
        self.trigger_at(index, &mut commands);
        // Follow-Kette synchron weiterführen, wenn der getriggerte Cue sie selbst
        // anstößt (Sofort-Cue + AutoContinue bzw. Playing + AutoFollow, Review B6/B7).
        // Iterativ mit Guard in continue_chain_from – kein rekursiver Aufruf aus
        // go_single, sonst hebelt der sich selbst aus. Playhead folgt der Kette.
        if let Some(last) = self.continue_chain_from(index, &mut commands) {
            self.cursor = self.cursor.max(last + 1);
        }

        Some(GoOutcome { index, commands })
    }

    /// Wie [`CueList::go`], folgt aber **Auto-Continue-Ketten**
    /// („Cue Sequences"): Hat der getriggerte Cue `AutoContinue`, wird der
    /// nächste Cue sofort mitgetriggert – rekursiv, bis ein Cue ohne
    /// Auto-Continue erreicht ist (Safety-Guard: max. 100 Verkettungen).
    /// Rückgabe: Index des ersten Cues + alle gesammelten Commands.
    pub fn go_chained(&mut self) -> Option<GoOutcome> {
        // Beim GO feuert nur der Standby-Cue. Auto-Continue
        // feuert den Folge-Cue erst, wenn der laufende Cue abgeschlossen ist
        // (inkl. Post-Wait → `set_status_with_follow` bei Completed);
        // Auto-Follow feuert beim Start (Playing). Deshalb hier kein Chaining.
        self.go()
    }

    /// Externer Timecode (LTC/MTC/OSC) erreicht die Liste: alle armed
    /// Standby-Cues mit [`Cue::tc_trigger`] ≤ `tc` feuern
    /// („timecode trigger"). Rückgabe: die dabei erzeugten Engine-Commands.
    /// Top-Level-Cues only (Gruppen-Kinder feuern über ihre Gruppe).
    pub fn receive_timecode(&mut self, tc: Timecode) -> Vec<EngineCommand> {
        let tc_dur = tc.to_duration();
        let mut indices = Vec::new();
        for (index, cue) in self.cues.iter().enumerate() {
            if !cue.is_armed || cue.status != CueStatus::Standby {
                continue;
            }
            if let Some(trigger) = cue.tc_trigger {
                if trigger.to_duration() <= tc_dur {
                    indices.push(index);
                }
            }
        }
        let mut commands = Vec::new();
        for index in indices {
            // Dispatch wie in go() (Review 11b): Gruppen feuern ihre Kinder.
            self.trigger_at(index, &mut commands);
        }
        commands
    }

    /// GO auf einen bestimmten Cue per Q-Nummer (z. B. „5.1") – für die
    /// Remote-Steuerung. Triggert den Cue direkt (wie die Control-Action
    /// `Start`), ohne den Playhead zu bewegen. Liefert `None`, wenn kein
    /// Cue mit dieser Nummer existiert oder der Cue nicht triggerbar ist
    /// (nicht armed / nicht in Standby).
    pub fn go_to_number(&mut self, number: &str) -> Option<GoOutcome> {
        let index = self.find_index_by_number(Some(number))?;
        let mut commands = Vec::new();
        if !self.trigger_by_number(number, &mut commands) {
            return None;
        }
        let id = self.cues[index].id;
        self.select_id(Some(id));
        Some(GoOutcome { index, commands })
    }

    /// Setzt den Armed-Zustand eines Cues per Q-Nummer (Remote-Steuerung).
    /// Rückgabe: `false`, wenn kein Cue mit dieser Nummer existiert.
    pub fn set_armed_by_number(&mut self, number: &str, armed: bool) -> bool {
        match self.find_index_by_number(Some(number)) {
            Some(index) => {
                self.cues[index].is_armed = armed;
                true
            }
            None => false,
        }
    }

    /// Triggert einen einzelnen (Nicht-Group-)Cue.
    fn go_single(&mut self, index: usize, commands: &mut Vec<EngineCommand>) {
        {
            // Daten vor dem &mut-Borrow sammeln (Borrow-Checker).
            let fade_group_children: Vec<Uuid> = if self.cues[index].cue_type == CueType::Fade {
                self.cues[index]
                    .fade_target
                    .and_then(|id| self.find_cue(id))
                    .filter(|t| t.cue_type == CueType::Group)
                    .map(|t| {
                        t.children
                            .iter()
                            .filter(|c| c.status.is_active())
                            .map(|c| c.id)
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let cue = &mut self.cues[index];
            // Ungültige Konfiguration (z. B. HTML-Cue ohne Quelle, Data-Cue mit
            // kaputtem JSON) → kontrollierter Error-Zustand, kein Panic.
            if cue.validate_trigger().is_err() {
                let _ = cue.transition_status(CueStatus::Error);
                return;
            }
            let command = cue.build_trigger_command();
            match &command {
                // Sofort-Cues (Stop, leere Container, JSON-Push, Network, MIDI,
                // DMX): direkt abgeschlossen – das Kommando geht an die Engine.
                Some(EngineCommand::StopAll)
                | Some(EngineCommand::PushJson { .. })
                | Some(EngineCommand::SendNetwork { .. })
                | Some(EngineCommand::SendMidi { .. })
                | Some(EngineCommand::SendDmx { .. })
                | Some(EngineCommand::SendHttpPost { .. })
                | None => {
                    let _ = cue.transition_status(CueStatus::Completed);
                }
                Some(_) => {
                    let next = if cue.pre_wait > Duration::ZERO {
                        CueStatus::PreWait
                    } else {
                        CueStatus::Playing
                    };
                    let _ = cue.transition_status(next);
                }
            };
            if let Some(command) = command {
                // Fade auf eine Gruppe: wirkt auf alle laufenden Kinder – pro
                // Kind ein Fade-Kommando mit der ID des Fade-Cues; die Engine
                // zählt mit und meldet Finished erst nach dem letzten Kind.
                let mut group_children_cmds: Vec<EngineCommand> = Vec::new();
                if cue.cue_type == CueType::Fade && !fade_group_children.is_empty() {
                    if let EngineCommand::Fade {
                        from,
                        volume,
                        opacity,
                        duration,
                        curve,
                        stop_when_done,
                        ..
                    } = &command
                    {
                        for child_id in &fade_group_children {
                            group_children_cmds.push(EngineCommand::Fade {
                                cue_id: cue.id,
                                target_cue: Some(*child_id),
                                from: *from,
                                volume: *volume,
                                opacity: *opacity,
                                duration: *duration,
                                curve: *curve,
                                stop_when_done: *stop_when_done,
                            });
                        }
                    }
                }
                if !group_children_cmds.is_empty() {
                    commands.extend(group_children_cmds);
                } else {
                    commands.push(command);
                }
                push_volume_command(cue, commands);
                push_initial_html_data(cue, commands);
            }
        }
    }

    /// Triggert einen Group-Cue (Kaskadierung je nach [`GroupMode`]).
    fn go_group(&mut self, index: usize, mode: GroupMode, commands: &mut Vec<EngineCommand>) {
        let group_id = self.cues[index].id;
        match mode {
            GroupMode::Simultaneous | GroupMode::Timeline => {
                // Alle armed Kinder werden in EINEM Go getriggert. Timeline
                // staffelt zusätzlich über die individuellen Pre-Waits der
                // Kinder (stecken bereits in den Commands).
                let cue = &mut self.cues[index];
                let mut any_running = false;
                for child in &mut cue.children {
                    if !child.is_armed || child.status != CueStatus::Standby {
                        continue;
                    }
                    child.parent_group = Some(group_id);
                    if child.validate_trigger().is_err() {
                        // Ungültige Konfiguration → Kind geht in Error (kein Panic).
                        let _ = child.transition_status(CueStatus::Error);
                        continue;
                    }
                    match child.build_trigger_command() {
                        // Sofort-Cues (Stop/JSON/Network/MIDI/DMX) innerhalb
                        // einer Gruppe gelten direkt als abgeschlossen.
                        Some(
                            cmd @ (EngineCommand::StopAll
                            | EngineCommand::PushJson { .. }
                            | EngineCommand::SendNetwork { .. }
                            | EngineCommand::SendMidi { .. }
                            | EngineCommand::SendDmx { .. }
                            | EngineCommand::SendHttpPost { .. }),
                        ) => {
                            let _ = child.transition_status(CueStatus::Completed);
                            commands.push(cmd);
                        }
                        Some(cmd) => {
                            let next = if child.pre_wait > Duration::ZERO {
                                CueStatus::PreWait
                            } else {
                                CueStatus::Playing
                            };
                            let _ = child.transition_status(next);
                            commands.push(cmd);
                            push_volume_command(child, commands);
                            any_running = true;
                        }
                        None => {
                            let _ = child.transition_status(CueStatus::Completed);
                        }
                    }
                }
                let next = if any_running {
                    CueStatus::Playing
                } else {
                    CueStatus::Completed
                };
                let _ = cue.transition_status(next);
                self.close_group_if_done(group_id);
            }
            GroupMode::Sequential | GroupMode::Loop | GroupMode::Shuffle => {
                // Kinder werden an die Cursor-Position eingespielt (jedes GO
                // triggert das nächste Kind). Shuffle mischt vorher
                // deterministisch (seed-basiert, reproduzierbar).
                let mut children = std::mem::take(&mut self.cues[index].children);
                if mode == GroupMode::Shuffle {
                    shuffle_deterministic(&mut children, group_id_seed(group_id));
                }
                let _ = self.cues[index].transition_status(CueStatus::Playing);
                for (offset, mut child) in children.into_iter().enumerate() {
                    child.parent_group = Some(group_id);
                    self.cues.insert(index + 1 + offset, child);
                }
                self.cursor = index + 1;
            }
        }
    }

    /// Triggert einen Control-Cue (Sofort-Cue, wirkt auf die Liste selbst).
    fn go_control(&mut self, index: usize, commands: &mut Vec<EngineCommand>) {
        // Review G0: Vor der Aktion als Playing markieren, damit ein Zyklus
        // (Control A startet B, B startet A) an der Standby-Pruefung in
        // trigger_by_number stoppt statt endlos zu rekursieren (Stack Overflow).
        let _ = self.cues[index].transition_status(CueStatus::Playing);
        let over_depth = CONTROL_DEPTH.with(|d| {
            let depth = d.get() + 1;
            d.set(depth);
            depth > MAX_CONTROL_DEPTH
        });
        if over_depth {
            let _ = self.cues[index].transition_status(CueStatus::Error);
            CONTROL_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
            return;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.go_control_inner(index, commands)
        }));
        CONTROL_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        if result.is_err() {
            let _ = self.cues[index].transition_status(CueStatus::Error);
        }
    }

    fn go_control_inner(&mut self, index: usize, commands: &mut Vec<EngineCommand>) {
        let (action, target_number) = {
            let cue = &self.cues[index];
            (cue.control_action, cue.control_target.clone())
        };
        match action {
            ControlAction::Start => {
                if let Some(number) = target_number {
                    self.trigger_by_number(&number, commands);
                }
            }
            ControlAction::Stop => match self.find_index_by_number(target_number.as_deref()) {
                Some(t) => {
                    let id = self.cues[t].id;
                    commands.push(EngineCommand::Stop { cue_id: id });
                }
                None => commands.push(EngineCommand::StopAll),
            },
            ControlAction::Pause => match self.find_index_by_number(target_number.as_deref()) {
                Some(t) => {
                    let id = self.cues[t].id;
                    commands.push(EngineCommand::Pause { cue_id: id });
                }
                None => {
                    // Ohne Ziel: alle laufenden Cues pausieren – nicht der
                    // Control-Cue selbst (der ist durch die G0-Markierung Playing).
                    let self_id = self.cues[index].id;
                    let running: Vec<Uuid> = self
                        .cues
                        .iter()
                        .filter(|c| c.status == CueStatus::Playing && c.id != self_id)
                        .map(|c| c.id)
                        .collect();
                    for id in running {
                        commands.push(EngineCommand::Pause { cue_id: id });
                    }
                }
            },
            ControlAction::Load => {
                let (cue_id, path) = match self.find_index_by_number(target_number.as_deref()) {
                    Some(t) => {
                        let c = &self.cues[t];
                        (c.id, c.target_path.clone())
                    }
                    None => {
                        let c = &self.cues[index];
                        (c.id, c.target_path.clone())
                    }
                };
                commands.push(EngineCommand::Load { cue_id, path });
            }
            ControlAction::Reset => {
                // Reset der gesamten Show (Playhead + alle Status).
                self.reset();
            }
            ControlAction::Devamp => match self.find_index_by_number(target_number.as_deref()) {
                Some(t) => {
                    let id = self.cues[t].id;
                    commands.push(EngineCommand::Devamp { cue_id: id });
                }
                None => commands.push(EngineCommand::DevampAll),
            },
            ControlAction::GoTo => match self.find_index_by_number(target_number.as_deref()) {
                Some(t) => {
                    // Playhead auf den Ziel-Cue setzen (nächstes GO triggert ihn).
                    self.cursor = t;
                }
                None => {
                    // Unbekanntes Ziel → Cue geht in den Fehlerzustand.
                    let _ = self.cues[index].transition_status(CueStatus::Error);
                    return;
                }
            },
            ControlAction::Arm | ControlAction::Disarm => {
                let armed = action == ControlAction::Arm;
                let target = match self.find_index_by_number(target_number.as_deref()) {
                    Some(t) => Some(t),
                    // Ohne Ziel: der Control-Cue selbst.
                    None if target_number.is_none() => Some(index),
                    None => None,
                };
                if let Some(t) = target {
                    self.cues[t].is_armed = armed;
                }
            }
            ControlAction::NextSlide | ControlAction::PrevSlide => {
                // Manuelles Schalten einer Diashow (Ziel erforderlich).
                let nav = if action == ControlAction::NextSlide {
                    SlideNav::Next
                } else {
                    SlideNav::Prev
                };
                match self.find_index_by_number(target_number.as_deref()) {
                    Some(t) => {
                        let id = self.cues[t].id;
                        commands.push(EngineCommand::SlideNav { cue_id: id, nav });
                    }
                    None => {
                        // Unbekanntes/fehlendes Ziel → Cue geht in den Fehlerzustand.
                        let _ = self.cues[index].transition_status(CueStatus::Error);
                        return;
                    }
                }
            }
        }
        // Control-Cues sind Sofort-Cues: Playing → Completed (G0-Markierung oben).
        // Nach Reset steht der Cue wieder auf Standby – dann ebenfalls abschließen.
        if matches!(
            self.cues[index].status,
            CueStatus::Playing | CueStatus::Standby
        ) {
            let _ = self.cues[index].transition_status(CueStatus::Completed);
        }
    }

    /// Triggert einen Cue direkt anhand seiner Nummer (Control-Action `Start`).
    /// Rückgabe: `true`, wenn der Cue getriggert wurde (ggf. in den
    /// Fehlerzustand bei ungültiger Konfiguration).
    fn trigger_by_number(&mut self, number: &str, commands: &mut Vec<EngineCommand>) -> bool {
        match self.find_index_by_number(Some(number)) {
            Some(index) => self.trigger_index(index, commands),
            None => false,
        }
    }

    /// Startet einen Top-Level-Cue per ID unabhängig vom Playhead (Cue-Trigger
    /// Hotkey/OSC/MIDI/Timecode/Clock) – auch ohne oder mit doppelter Nummer.
    /// Rückgabe: Engine-Kommandos.
    pub fn trigger_id(&mut self, id: Uuid) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        if let Some(index) = self.cues.iter().position(|c| c.id == id) {
            self.trigger_index(index, &mut commands);
        }
        commands
    }

    /// Triggert den Top-Level-Cue `index` samt Follow-Kette, ohne den Playhead
    /// zu bewegen (siehe [`Self::trigger_by_number`]).
    fn trigger_index(&mut self, index: usize, commands: &mut Vec<EngineCommand>) -> bool {
        if !self.cues[index].is_armed || self.cues[index].status != CueStatus::Standby {
            return false;
        }
        if self.cues[index].validate_trigger().is_err() {
            let id = self.cues[index].id;
            let _ = self.set_status(id, CueStatus::Error);
            return true;
        }
        // Dispatch wie in go() (Review 11b): Gruppe → Kinder, Control → Aktion,
        // sonst go_single – bisher feuerten Gruppen/Control bei GoTo/Timecode
        // keine Kinder.
        self.trigger_at(index, commands);
        // Follow-Kette ab diesem Cue – ohne Playhead-Bewegung (Review B6/B7).
        let _ = self.continue_chain_from(index, commands);
        true
    }

    /// Sucht einen Top-Level-Cue anhand seiner Nummer.
    fn find_index_by_number(&self, number: Option<&str>) -> Option<usize> {
        number
            .filter(|n| !n.is_empty())
            .and_then(|n| self.cues.iter().position(|c| c.number == n))
    }

    // ------------------------------------------------------------------
    // Status-Feedback (Engine → Domain)
    // ------------------------------------------------------------------

    /// Setzt den Status eines Cues (State-Machine wird erzwungen).
    /// Bei Children wird nach dem Update die Eltern-Gruppe geprüft
    /// (Close-if-done bzw. Loop-Re-Arm).
    pub fn set_status(&mut self, id: Uuid, status: CueStatus) -> ShowResult<()> {
        let parent_group = {
            let cue = self
                .find_cue_mut(id)
                .ok_or_else(|| ShowError::CueNotFound(id.to_string()))?;
            cue.transition_status(status)?;
            cue.parent_group
        };
        if let Some(group_id) = parent_group {
            self.close_group_if_done(group_id);
        }
        Ok(())
    }

    /// Wie [`CueList::set_status`], triggert aber zusätzlich die
    /// Continue-Modi am richtigen Zeitpunkt:
    /// * **Auto-Follow** feuert den nächsten Cue, sobald dieser Cue **startet**
    ///   (`Playing` nach Pre-Wait) – „starts as soon as this cue starts".
    /// * **Auto-Continue** feuert den nächsten Cue, wenn dieser Cue
    ///   **abgeschlossen** ist (`Completed` inkl. Post-Wait) –
    ///   „starts when this cue completes".
    ///
    /// Rückgabe: Status-Ergebnis + die dabei erzeugten Engine-Commands.
    pub fn set_status_with_follow(
        &mut self,
        id: Uuid,
        status: CueStatus,
    ) -> (ShowResult<()>, Vec<EngineCommand>) {
        // Nur bei echter Statusänderung folgen: `transition_status` ist idempotent
        // (same→same = Ok ohne Änderung). Ohne diese Prüfung feuert z. B. bei
        // Post-Wait 0 sowohl `Finished` (→ Completed) als auch `PostWaitElapsed`
        // (→ Completed) und der Folge-Cue würde doppelt gefeuert (Review B6).
        let prev = self.find_cue(id).map(|c| c.status);
        let result = self.set_status(id, status);
        let changed = result.is_ok() && prev.is_some_and(|p| p != status);
        let mut commands = Vec::new();
        let follow = changed
            && self
                .find_cue(id)
                .map(|c| match status {
                    CueStatus::Playing => c.continue_mode == ContinueMode::AutoFollow,
                    CueStatus::Completed => c.continue_mode == ContinueMode::AutoContinue,
                    _ => false,
                })
                .unwrap_or(false);
        if follow {
            if let Some(outcome) = self.go() {
                commands = outcome.commands;
            }
        }
        (result, commands)
    }

    /// Triggert den Cue an `index` (Dispatch wie in [`CueList::go`], aber ohne
    /// Cursor-/Selektions-Update – für Auto-Continue-Ketten ab Mit-Cue).
    fn trigger_at(&mut self, index: usize, commands: &mut Vec<EngineCommand>) {
        let cue_type = self.cues[index].cue_type;
        let group_mode = self.cues[index].group_mode;
        match cue_type {
            CueType::Group => self.go_group(index, group_mode, commands),
            CueType::Control => self.go_control(index, commands),
            _ => self.go_single(index, commands),
        }
    }

    /// Follow-Kette ab Cue `index`: feuert den nächsten armed Standby-Cue, solange
    /// der aktuelle Cue seine Folge selbst feuert – `Completed` + AutoContinue bzw.
    /// `Playing` + AutoFollow. Läuft synchron in der Liste, weil Sofort-Cues
    /// (Memo/MIDI/Network/Light/Companion) kein Engine-Event erzeugen (Review B7)
    /// und weil `go()` den Status Playing schon direkt setzt (das spätere
    /// Engine-`Started`-Event ist dann keine Statusänderung mehr und feuert dank
    /// der Änderungsprüfung in `set_status_with_follow` nicht doppelt, Review B6).
    /// Schutz vor Endlosketten: max. 100 Verkettungen.
    /// Rückgabe: Index des letzten per Kette gefeuerten Cues (für den Playhead).
    fn continue_chain_from(
        &mut self,
        index: usize,
        commands: &mut Vec<EngineCommand>,
    ) -> Option<usize> {
        const MAX_CHAIN: usize = 100;
        let mut current = index;
        let mut last_fired: Option<usize> = None;
        for _ in 0..MAX_CHAIN {
            let Some(cue) = self.cues.get(current) else {
                return last_fired;
            };
            let should_follow = matches!(
                (cue.status, cue.continue_mode),
                (CueStatus::Completed, ContinueMode::AutoContinue)
                    | (CueStatus::Playing, ContinueMode::AutoFollow)
            );
            if !should_follow {
                return last_fired;
            }
            let Some(next) = self
                .cues
                .iter()
                .enumerate()
                .skip(current + 1)
                .find(|(_, c)| c.is_armed && c.status == CueStatus::Standby)
                .map(|(i, _)| i)
            else {
                return last_fired;
            };
            self.trigger_at(next, commands);
            last_fired = Some(next);
            match self.cues[next].status {
                // Folge-Cue läuft oder ist sofort fertig → Kette läuft weiter.
                CueStatus::Playing | CueStatus::Completed => current = next,
                _ => return last_fired,
            }
        }
        last_fired
    }

    /// Schließt eine Gruppe, wenn alle getriggerten Kinder abgeschlossen
    /// sind. [`GroupMode::Loop`]-Gruppen armen ihre Kinder erneut, bis die
    /// Iterationen aufgebraucht sind.
    fn close_group_if_done(&mut self, group_id: Uuid) {
        // Informationen sammeln (immutabel), dann mutieren.
        let mut any_triggered = false;
        let mut all_finished = true;
        let mut child_indices = Vec::new();
        if let Some(group) = self.find_cue(group_id) {
            for child in &group.children {
                // Unarmed Standby-Kinder blockieren nicht (nie getriggert).
                if child.status == CueStatus::Standby && !child.is_armed {
                    continue;
                }
                any_triggered = true;
                if child.status != CueStatus::Completed {
                    all_finished = false;
                }
            }
        }
        for (i, cue) in self.cues.iter().enumerate() {
            if cue.parent_group == Some(group_id) {
                if cue.status == CueStatus::Standby && !cue.is_armed {
                    continue;
                }
                any_triggered = true;
                child_indices.push(i);
                if cue.status != CueStatus::Completed {
                    all_finished = false;
                }
            }
        }
        if !any_triggered || !all_finished {
            return;
        }

        // Loop-Modus: Kinder erneut einreihen, bis Iterationen aufgebraucht.
        let (mode, loop_mode, iterations_done) = match self.find_cue(group_id) {
            Some(g) => (g.group_mode, g.loop_mode, g.group_iterations_done),
            None => return,
        };
        if mode == GroupMode::Loop && iterations_done + 1 < loop_mode.total_iterations() {
            let first_child = child_indices.first().copied();
            for i in &child_indices {
                let _ = self.cues[*i].transition_status(CueStatus::Standby);
            }
            if let Some(g) = self.find_cue_mut(group_id) {
                g.group_iterations_done += 1;
            }
            if let Some(first) = first_child {
                self.cursor = first;
            }
            return;
        }

        if let Some(group) = self.find_cue_mut(group_id) {
            if group.status == CueStatus::Playing {
                let _ = group.transition_status(CueStatus::Completed);
            }
        }
    }

    /// Stoppt alle laufenden Cues (Panic) → Status `Stopped`.
    pub fn stop_all_running(&mut self) {
        for cue in &mut self.cues {
            cue.stop_running_recursive();
        }
    }

    /// Pausiert alle laufenden Cues („Pause All").
    /// Rückgabe: `Pause`-Commands für alle pausierten Cues.
    pub fn pause_all_running(&mut self) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        for cue in &mut self.cues {
            cue.pause_all_collect(&mut commands);
        }
        commands
    }

    /// Setzt alle pausierten Cues fort („Resume All").
    /// Rückgabe: `Resume`-Commands für alle fortgesetzten Cues.
    pub fn resume_all_running(&mut self) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        for cue in &mut self.cues {
            cue.resume_all_collect(&mut commands);
        }
        commands
    }

    /// Stoppt einen einzelnen aktiven Cue (rekursiv inkl. Group-Kinder).
    /// Rückgabe: die zu sendenden `Stop`-Commands (leer, wenn der Cue nicht
    /// aktiv oder unbekannt ist). Status geht lokal sofort auf `Stopped`
    /// (UI-Feedback); das Engine-`Stopped`-Event ist idempotent.
    pub fn stop_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        let parent_group = match self.find_cue_mut(id) {
            Some(cue) => cue.stop_running_collect(&mut commands),
            None => return commands,
        };
        if let Some(group_id) = parent_group {
            self.close_group_if_done(group_id);
        }
        commands
    }

    /// Pausiert einen laufenden Cue (`Playing` → `Paused`, rekursiv inkl.
    /// Group-Kinder). Rückgabe: die zu sendenden `Pause`-Commands.
    pub fn pause_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        let parent_group = match self.find_cue_mut(id) {
            Some(cue) => cue.pause_running_collect(&mut commands),
            None => return commands,
        };
        if let Some(group_id) = parent_group {
            self.close_group_if_done(group_id);
        }
        commands
    }

    /// Setzt einen pausierten Cue fort (`Paused` → `Playing`, rekursiv).
    /// Rückgabe: die zu sendenden `Resume`-Commands.
    pub fn resume_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        let mut commands = Vec::new();
        let parent_group = match self.find_cue_mut(id) {
            Some(cue) => cue.resume_running_collect(&mut commands),
            None => return commands,
        };
        if let Some(group_id) = parent_group {
            self.close_group_if_done(group_id);
        }
        commands
    }

    /// Reset: Playhead auf 0, alle Cues (inkl. Kinder) → Standby.
    pub fn reset(&mut self) {
        self.cursor = 0;
        for cue in &mut self.cues {
            cue.reset_status_recursive();
        }
    }
}

/// HTML-Cue mit Startdaten (`data_json_text`): Daten direkt nach dem Start
/// mitschicken – die Engine stellt sie nach dem Laden zu, vor dem Trigger
/// „in", die Seite ist also gleich befüllt. Ungültiges JSON meldet die
/// Problemanzeige; hier wird es übergangen.
fn push_initial_html_data(cue: &Cue, commands: &mut Vec<EngineCommand>) {
    if cue.cue_type != CueType::Html || cue.data_json_text.trim().is_empty() {
        return;
    }
    if let Ok(payload) = serde_json::from_str(&cue.data_json_text) {
        commands.push(EngineCommand::PushJson {
            target: cue.id,
            payload,
        });
    }
}

/// Hängt ein `SetVolume`-Command an, wenn die Cue-Lautstärke ≠ 1.0 ist
/// (Audio/Video-Cues). Die Engine setzt die Lautstärke unmittelbar nach
/// dem `Play`-Command – so startet das Medium mit korrektem Pegel.
fn push_volume_command(cue: &Cue, commands: &mut Vec<EngineCommand>) {
    if matches!(cue.cue_type, CueType::Audio | CueType::Video)
        && (cue.volume - 1.0).abs() > f32::EPSILON
    {
        commands.push(EngineCommand::SetVolume {
            cue_id: cue.id,
            volume: cue.volume.clamp(0.0, 1.0),
        });
    }
}

/// Deterministischer Seed aus einer Group-UUID (für [`GroupMode::Shuffle`]).
fn group_id_seed(id: Uuid) -> u64 {
    let v = id.as_u128();
    (v as u64) ^ ((v >> 64) as u64)
}

/// Vergibt einem ( geklonten ) Cue und allen Kindern frische UUIDs
/// („Duplicate": Kopien dürfen nie dieselbe ID teilen – sonst würde
/// Engine-Feedback/Selektion auf Original und Kopie wirken).
fn reassign_ids(cue: &mut Cue) {
    cue.id = Uuid::new_v4();
    for child in &mut cue.children {
        reassign_ids(child);
    }
}

/// Deterministisches Fisher-Yates-Shuffle (xorshift64* PRNG).
///
/// Bewusst *deterministisch* (Seed = Group-ID): Eine Shuffle-Gruppe muss an
/// jedem Show-Tag dieselbe Reihenfolge produzieren – reproduzierbare Shows.
fn shuffle_deterministic<T>(items: &mut [T], seed: u64) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::LoopMode;

    fn audio(number: &str) -> Cue {
        Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
    }

    #[test]
    fn html_cue_sends_its_initial_data_right_after_start() {
        let mut list = CueList::new();
        let mut cue = Cue::new("1", "Zeitplan", CueType::Html)
            .with_html_source(crate::core::HtmlSource::inline("<html></html>"));
        cue.data_json_text = r#"{"speakers": []}"#.to_string();
        let id = cue.id;
        list.add(cue);
        let outcome = list.go().expect("GO");
        let kinds: Vec<&str> = outcome
            .commands
            .iter()
            .map(|c| match c {
                EngineCommand::PlayHtml { .. } => "play",
                EngineCommand::PushJson { target, .. } if target == &id => "data",
                _ => "other",
            })
            .collect();
        let play = kinds.iter().position(|k| *k == "play").expect("PlayHtml");
        let data = kinds.iter().position(|k| *k == "data").expect("PushJson");
        assert!(play < data, "Daten nach dem Start: {kinds:?}");
    }

    #[test]
    fn go_triggers_next_standby_cue_and_advances_playhead() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));

        let outcome = list.go().expect("GO must trigger");
        assert_eq!(outcome.index, 0);
        assert_eq!(outcome.commands.len(), 1);
        assert!(matches!(outcome.commands[0], EngineCommand::Play { .. }));
        assert_eq!(list.cues()[0].status, CueStatus::Playing);
        assert_eq!(list.cursor(), 1);
        assert_eq!(list.selected_index(), Some(0));

        let outcome = list.go().expect("second GO must trigger");
        assert_eq!(outcome.index, 1);
        assert!(list.go().is_none(), "stream exhausted");
    }

    #[test]
    fn go_with_pre_wait_enters_pre_wait_state() {
        let mut list = CueList::new();
        list.add(audio("1").with_pre_wait(Duration::from_millis(500)));
        let _ = list.go().unwrap();
        assert_eq!(list.cues()[0].status, CueStatus::PreWait);
        // Engine-Started bewegt den Cue weiter nach Playing.
        let id = list.cues()[0].id;
        list.set_status(id, CueStatus::Playing).unwrap();
        assert_eq!(list.cues()[0].status, CueStatus::Playing);
    }

    #[test]
    fn next_free_number_is_highest_plus_one() {
        let mut list = CueList::new();
        assert_eq!(list.next_free_number(), 1, "leere Liste startet bei 1");
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(audio("3"));
        assert_eq!(list.next_free_number(), 4);
        // Löschen erzeugt eine Lücke – die Nummer wird NICHT neu vergeben.
        list.remove(1).unwrap();
        assert_eq!(list.next_free_number(), 4);
        // Group-Kinder zählen mit („5.1" → 5).
        list.add(Cue::new_group(
            "5",
            "g",
            GroupMode::Timeline,
            vec![audio("5.1")],
        ));
        assert_eq!(list.next_free_number(), 6);
        // Nicht-numerische Nummern werden ignoriert.
        let mut list2 = CueList::new();
        list2.add(audio("A"));
        assert_eq!(list2.next_free_number(), 1);
    }

    #[test]
    fn stop_cue_finishes_instantly() {
        let mut list = CueList::new();
        list.add(Cue::new("1", "Blackout", CueType::Stop));
        let outcome = list.go().expect("stop cue must trigger");
        assert_eq!(outcome.commands, vec![EngineCommand::StopAll]);
        assert_eq!(list.cues()[0].status, CueStatus::Completed);
    }

    #[test]
    fn control_cue_reset_resets_the_whole_show() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(Cue::new("3", "Reset", CueType::Control).with_control(ControlAction::Reset));

        let _ = list.go().unwrap(); // "1" läuft
        assert_eq!(list.cues()[0].status, CueStatus::Playing);

        let _ = list.go().unwrap(); // "2" läuft
        let outcome = list.go().unwrap(); // Reset-Cue
        assert!(
            outcome.commands.is_empty(),
            "Reset erzeugt keine Engine-Commands"
        );
        assert_eq!(outcome.index, 2);
        assert_eq!(
            list.cues()[2].status,
            CueStatus::Completed,
            "Reset-Cue selbst abgeschlossen"
        );
        assert_eq!(list.cursor(), 0, "Playhead zurückgesetzt");
        assert!(list
            .cues()
            .iter()
            .all(|c| c.status == CueStatus::Standby || c.number == "3"));
    }

    #[test]
    fn control_cue_goto_moves_playhead_to_target() {
        let mut list = CueList::new();
        list.add(
            Cue::new("1", "Jump to 3", CueType::Control)
                .with_control(ControlAction::GoTo)
                .with_control_target("3"),
        );
        list.add(audio("2"));
        list.add(audio("3"));

        let outcome = list.go().unwrap(); // GoTo-Cue (Sofort-Cue)
        assert!(outcome.commands.is_empty());
        assert_eq!(outcome.index, 0);
        assert_eq!(list.cues()[0].status, CueStatus::Completed);
        assert_eq!(list.cursor(), 2, "Playhead steht auf Ziel-Cue \"3\"");

        let outcome = list.go().unwrap();
        assert_eq!(
            outcome.index, 2,
            "nächster GO triggert das Ziel (überspringt \"2\")"
        );
    }

    #[test]
    fn control_cue_goto_with_unknown_target_errors() {
        let mut list = CueList::new();
        list.add(
            Cue::new("1", "Jump nowhere", CueType::Control)
                .with_control(ControlAction::GoTo)
                .with_control_target("99"),
        );
        let _ = list.go().unwrap();
        assert_eq!(list.cues()[0].status, CueStatus::Error);
    }

    #[test]
    fn disarmed_cues_are_skipped_by_go() {
        let mut list = CueList::new();
        let mut disarmed = audio("1");
        disarmed.is_armed = false;
        list.add(disarmed);
        list.add(audio("2"));

        let outcome = list.go().expect("GO must skip disarmed cue");
        assert_eq!(outcome.index, 1);
        assert_eq!(list.cues()[0].status, CueStatus::Standby);
    }

    #[test]
    fn sequential_group_splices_children_and_closes_after_last() {
        let mut list = CueList::new();
        let group = Cue::new_group(
            "1",
            "Seq",
            GroupMode::Sequential,
            vec![audio("1.1"), audio("1.2")],
        );
        let group_id = group.id;
        list.add(group);

        let outcome = list.go().unwrap();
        assert!(outcome.commands.is_empty());
        assert_eq!(list.len(), 3);
        assert_eq!(list.cues()[0].status, CueStatus::Playing);
        assert_eq!(list.cursor(), 1);

        let id1 = list.cues()[1].id;
        let _ = list.go().unwrap();
        list.set_status(id1, CueStatus::Completed).unwrap();
        assert_eq!(
            list.cues()[0].status,
            CueStatus::Playing,
            "group still open"
        );

        let id2 = list.cues()[2].id;
        let _ = list.go().unwrap();
        list.set_status(id2, CueStatus::Completed).unwrap();
        assert_eq!(list.cues()[0].status, CueStatus::Completed, "group closed");
        assert_eq!(
            list.find_cue(group_id).unwrap().status,
            CueStatus::Completed
        );
    }

    #[test]
    fn loop_group_rearms_children_until_iterations_exhausted() {
        let mut list = CueList::new();
        let mut group = Cue::new_group(
            "1",
            "Loop",
            GroupMode::Loop,
            vec![audio("1.1"), audio("1.2")],
        );
        group.loop_mode = LoopMode::Count(2);
        list.add(group);

        // Erste Runde.
        let _ = list.go().unwrap(); // splicet
        let _ = list.go().unwrap(); // Kind 1.1
        let id1 = list.cues()[1].id;
        list.set_status(id1, CueStatus::Completed).unwrap();
        let _ = list.go().unwrap(); // Kind 1.2
        let id2 = list.cues()[2].id;
        list.set_status(id2, CueStatus::Completed).unwrap();

        // Alle Kinder fertig → Loop-Gruppen re-armen (Iteration 1 von 2).
        assert_eq!(
            list.cues()[0].status,
            CueStatus::Playing,
            "group stays open in loop mode"
        );
        assert_eq!(list.cues()[0].group_iterations_done, 1);
        assert_eq!(list.cues()[1].status, CueStatus::Standby, "child re-armed");
        assert_eq!(list.cursor(), 1, "playhead back at first child");

        // Zweite (und letzte) Runde.
        let _ = list.go().unwrap();
        let id1 = list.cues()[1].id;
        list.set_status(id1, CueStatus::Completed).unwrap();
        let _ = list.go().unwrap();
        let id2 = list.cues()[2].id;
        list.set_status(id2, CueStatus::Completed).unwrap();
        assert_eq!(
            list.cues()[0].status,
            CueStatus::Completed,
            "iterations exhausted → group closes"
        );
    }

    #[test]
    fn shuffle_group_splices_all_children_in_a_permutation() {
        let mut list = CueList::new();
        let children: Vec<Cue> = (0..6).map(|i| audio(&format!("1.{i}"))).collect();
        list.add(Cue::new_group("1", "Shuffle", GroupMode::Shuffle, children));

        let _ = list.go().unwrap();
        assert_eq!(list.len(), 7);
        let mut numbers: Vec<&str> = list.cues()[1..].iter().map(|c| c.number.as_str()).collect();
        numbers.sort();
        assert_eq!(
            numbers,
            vec!["1.0", "1.1", "1.2", "1.3", "1.4", "1.5"],
            "shuffle must be a permutation of all children"
        );
    }

    #[test]
    fn stop_all_running_marks_active_cues_stopped() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        let _ = list.go().unwrap();
        let _ = list.go().unwrap();
        list.stop_all_running();
        assert!(list.cues().iter().all(|c| c.status == CueStatus::Stopped));
        assert_eq!(list.running_count(), 0);
        // Reset erholt.
        list.reset();
        assert!(list.cues().iter().all(|c| c.status == CueStatus::Standby));
    }

    #[test]
    fn set_status_rejects_invalid_transition() {
        let mut list = CueList::new();
        list.add(audio("1"));
        let id = list.cues()[0].id;
        let err = list.set_status(id, CueStatus::Paused).unwrap_err();
        assert!(matches!(err, ShowError::InvalidStateTransition { .. }));
        assert!(list.set_status(Uuid::new_v4(), CueStatus::Playing).is_err());
    }

    #[test]
    fn cue_list_serde_roundtrip_with_name() {
        let mut list = CueList::with_name("Intro");
        list.add(audio("1").with_loop(LoopMode::Count(2)));
        let json = serde_json::to_string(&list).expect("serialize");
        let back: CueList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name(), "Intro");
        assert_eq!(back.len(), 1);
        assert_eq!(back.cues()[0].loop_mode, LoopMode::Count(2));
    }

    #[test]
    fn set_cursor_moves_playhead_and_clamps_to_end() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));

        list.set_cursor(1);
        assert_eq!(list.cursor(), 1);
        let outcome = list.go().unwrap();
        assert_eq!(outcome.index, 1, "GO triggert den Cue am Playhead");

        list.set_cursor(99);
        assert_eq!(list.cursor(), 2, "geklemmt auf len (hinter dem Ende)");
        assert!(list.go().is_none(), "Playhead am Ende → kein GO");

        list.set_cursor(0);
        assert_eq!(list.cursor(), 0);
    }

    #[test]
    fn go_continues_in_order_after_playhead_was_set_back() {
        let mut list = CueList::new();
        for number in ["1", "2", "3", "4"] {
            list.add(audio(number));
        }
        list.go();
        list.go();
        list.go();
        for cue in &mut list.cues_mut()[..3] {
            cue.status = CueStatus::Completed;
        }

        list.set_cursor(0);
        let order: Vec<usize> = (0..4).filter_map(|_| list.go().map(|o| o.index)).collect();
        assert_eq!(order, [0, 1, 2, 3]);
    }

    #[test]
    fn set_cursor_keeps_running_cues_and_cues_before_the_playhead() {
        let mut list = CueList::new();
        for number in ["1", "2", "3"] {
            list.add(audio(number));
        }
        list.cues_mut()[0].status = CueStatus::Completed;
        list.cues_mut()[2].status = CueStatus::Playing;

        list.set_cursor(1);
        assert_eq!(list.cues()[0].status, CueStatus::Completed);
        assert_eq!(list.cues()[2].status, CueStatus::Playing);
    }

    #[test]
    fn go_starts_finished_cue_when_playhead_is_set_back_on_it() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(audio("3"));
        list.go();
        list.go();
        list.cues_mut()[0].status = CueStatus::Completed;
        list.cues_mut()[1].status = CueStatus::Stopped;

        list.set_cursor(0);
        assert_eq!(list.standby().map(|c| c.number.as_str()), Some("1"));
        assert_eq!(list.go().map(|o| o.index), Some(0));
    }

    #[test]
    fn insert_shifts_playhead_and_selection_behind_insert_position() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(audio("3"));
        list.set_cursor(1);
        list.select(Some(1));

        // Einfügen an Position 2 (hinter Selektion/Playhead): nichts verschiebt sich.
        list.insert(2, audio("new"));
        assert_eq!(list.len(), 4);
        assert_eq!(list.cursor(), 1);
        assert_eq!(list.selected_index(), Some(1));

        // Einfügen an Position 1 (Playhead/Selektion zeigen darauf): beide rücken mit.
        list.insert(1, audio("new2"));
        assert_eq!(list.len(), 5);
        assert_eq!(list.cursor(), 2, "Playhead folgt dem verschobenen Cue");
        assert_eq!(list.selected_index(), Some(2));
        assert_eq!(list.cues()[1].number, "new2");

        // Einfügen ans Ende.
        list.insert(99, audio("tail"));
        assert_eq!(list.cues().last().unwrap().number, "tail");
    }

    #[test]
    fn stop_running_stops_single_cue_and_returns_stop_command() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        let _ = list.go().unwrap();
        let _ = list.go().unwrap();
        assert_eq!(list.running_count(), 2);

        let id1 = list.cues()[0].id;
        let commands = list.stop_running(id1);
        assert_eq!(commands, vec![EngineCommand::Stop { cue_id: id1 }]);
        assert_eq!(list.cues()[0].status, CueStatus::Stopped);
        assert_eq!(
            list.cues()[1].status,
            CueStatus::Playing,
            "anderer Cue läuft weiter"
        );
        assert_eq!(list.running_count(), 1);

        // Engine-Feedback (Stopped-Event) ist idempotent – kein State-Fehler.
        list.set_status(id1, CueStatus::Stopped).unwrap();

        // Unbekannte ID → leere Commands, kein Panic.
        assert!(list.stop_running(Uuid::new_v4()).is_empty());
    }

    #[test]
    fn stop_running_group_stops_children_recursively() {
        let mut list = CueList::new();
        let group = Cue::new_group(
            "1",
            "Scene",
            GroupMode::Simultaneous,
            vec![audio("1.1"), audio("1.2")],
        );
        let group_id = group.id;
        list.add(group);

        let _ = list.go().unwrap();
        assert_eq!(
            list.running_count(),
            1,
            "Gruppe zählt als ein laufender Cue"
        );

        let commands = list.stop_running(group_id);
        assert_eq!(commands.len(), 3, "Gruppe + 2 Kinder bekommen je ein Stop");
        assert!(commands.contains(&EngineCommand::Stop { cue_id: group_id }));
        let group = list.find_cue(group_id).unwrap();
        assert_eq!(group.status, CueStatus::Stopped);
        assert!(group
            .children
            .iter()
            .all(|c| c.status == CueStatus::Stopped));
        assert_eq!(list.running_count(), 0);
    }

    #[test]
    fn pause_and_resume_running_roundtrip() {
        let mut list = CueList::new();
        list.add(audio("1"));
        let _ = list.go().unwrap();
        let id = list.cues()[0].id;

        let commands = list.pause_running(id);
        assert_eq!(commands, vec![EngineCommand::Pause { cue_id: id }]);
        assert_eq!(list.cues()[0].status, CueStatus::Paused);
        assert_eq!(
            list.running_count(),
            1,
            "pausiert zählt weiterhin als aktiv"
        );

        let commands = list.resume_running(id);
        assert_eq!(commands, vec![EngineCommand::Resume { cue_id: id }]);
        assert_eq!(list.cues()[0].status, CueStatus::Playing);

        // Pause im Standby → keine Commands, kein Zustandswechsel.
        let idle = audio("2");
        let idle_id = idle.id;
        list.add(idle);
        assert!(list.pause_running(idle_id).is_empty());
        assert!(list.resume_running(idle_id).is_empty());
        assert_eq!(list.cues()[1].status, CueStatus::Standby);
    }

    #[test]
    fn go_dispatches_set_volume_when_cue_volume_differs_from_full() {
        let mut list = CueList::new();
        let mut quiet = audio("1");
        quiet.volume = 0.5;
        list.add(quiet);
        let id = list.cues()[0].id;

        let outcome = list.go().unwrap();
        assert_eq!(outcome.commands.len(), 2, "Play + SetVolume");
        assert!(matches!(outcome.commands[0], EngineCommand::Play { .. }));
        assert_eq!(
            outcome.commands[1],
            EngineCommand::SetVolume {
                cue_id: id,
                volume: 0.5
            }
        );

        // Volle Lautstärke → kein zusätzliches Command.
        let mut list = CueList::new();
        list.add(audio("1"));
        let outcome = list.go().unwrap();
        assert_eq!(outcome.commands.len(), 1);
    }

    #[test]
    fn go_chained_follows_auto_continue_sequence() {
        // GO feuert nur den ersten Cue; Auto-Continue feuert
        // den Folge-Cue erst beim Abschluss (Completed) des laufenden Cues.
        let mut list = CueList::new();
        list.add(
            audio("1")
                .with_target("/media/a.wav")
                .with_continue_mode(ContinueMode::AutoContinue),
        );
        list.add(
            audio("2")
                .with_target("/media/b.wav")
                .with_continue_mode(ContinueMode::AutoContinue),
        );
        list.add(audio("3").with_target("/media/c.wav"));

        let outcome = list.go_chained().expect("GO triggert ersten Cue");
        assert_eq!(outcome.index, 0);
        assert_eq!(outcome.commands.len(), 1, "beim GO nur ein Cue");
        assert_eq!(list.cues()[0].status, CueStatus::Playing);
        assert_eq!(list.cues()[1].status, CueStatus::Standby);

        // Cue 1 abgeschlossen → Auto-Continue feuert Cue 2.
        let id1 = list.cues()[0].id;
        let (result, commands) = list.set_status_with_follow(id1, CueStatus::Completed);
        result.unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(list.cues()[1].status, CueStatus::Playing);
        assert_eq!(list.cues()[2].status, CueStatus::Standby);

        // Cue 2 abgeschlossen → Auto-Continue feuert Cue 3.
        let id2 = list.cues()[1].id;
        let (_, commands) = list.set_status_with_follow(id2, CueStatus::Completed);
        assert_eq!(commands.len(), 1);
        assert_eq!(list.cues()[2].status, CueStatus::Playing);

        // Ohne Auto-Continue: nur der erste Cue, kein Weiterfeuern.
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        let outcome = list.go_chained().unwrap();
        assert_eq!(outcome.commands.len(), 1);
        let id1 = list.cues()[0].id;
        let (_, commands) = list.set_status_with_follow(id1, CueStatus::Completed);
        assert!(commands.is_empty());
        assert_eq!(list.cues()[1].status, CueStatus::Standby);
    }

    #[test]
    fn auto_follow_fires_next_cue_when_cue_starts() {
        // Auto-Follow feuert den Folge-Cue, sobald der Cue STARTET.
        let mut list = CueList::new();
        list.add(audio("1").with_continue_mode(ContinueMode::AutoFollow));
        list.add(audio("2"));

        // Auto-Follow feuert synchron in go(), sobald der Cue Playing ist
        // (Review B6/B7: das spätere Engine-Started-Event ist keine Änderung mehr).
        let outcome = list.go().unwrap();
        let id1 = list.cues()[0].id;
        assert!(
            outcome
                .commands
                .iter()
                .any(|c| matches!(c, EngineCommand::Play { cue_id, .. } if *cue_id != id1)),
            "Auto-Follow triggert Cue 2 direkt in go()"
        );
        assert_eq!(list.cues()[1].status, CueStatus::Playing);
        assert_eq!(list.cursor(), 2);

        // Wiederholtes Playing (Engine-Started-Event) → kein Doppel-Feuern.
        let (result, commands) = list.set_status_with_follow(id1, CueStatus::Playing);
        result.unwrap();
        assert!(
            commands.is_empty(),
            "kein Doppel-Feuern beim wiederholten Start"
        );

        // Beim Abschluss (Completed) feuert Auto-Follow NICHT mehr.
        let (_, commands) = list.set_status_with_follow(id1, CueStatus::Completed);
        assert!(commands.is_empty(), "kein Doppel-Feuern beim Abschluss");

        // Cue ohne Auto-Follow → keine Commands beim Start.
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        let _ = list.go().unwrap();
        let id1 = list.cues()[0].id;
        let (_, commands) = list.set_status_with_follow(id1, CueStatus::Playing);
        assert!(commands.is_empty());
        assert_eq!(list.cues()[1].status, CueStatus::Standby);
    }

    #[test]
    fn standby_mut_edits_notes_of_next_cue() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.set_cursor(1);
        let notes = &mut list.standby_mut().expect("Cue 2 steht bereit").notes;
        notes.push_str("Bitte Mikro checken");
        assert_eq!(list.cues()[1].notes, "Bitte Mikro checken");
    }

    #[test]
    fn pause_all_and_resume_all_return_commands_for_every_running_cue() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(audio("3"));
        let _ = list.go().unwrap();
        let _ = list.go().unwrap();
        // Zwei laufen, einer steht bereit.
        let commands = list.pause_all_running();
        assert_eq!(commands.len(), 2);
        assert!(commands
            .iter()
            .all(|c| matches!(c, EngineCommand::Pause { .. })));
        assert_eq!(list.running_count(), 2, "pausiert zählt als aktiv");
        assert!(list
            .cues()
            .iter()
            .take(2)
            .all(|c| c.status == CueStatus::Paused));

        let commands = list.resume_all_running();
        assert_eq!(commands.len(), 2);
        assert!(commands
            .iter()
            .all(|c| matches!(c, EngineCommand::Resume { .. })));
        assert!(list
            .cues()
            .iter()
            .take(2)
            .all(|c| c.status == CueStatus::Playing));
    }

    // ------------------------------------------------------------------
    // Phase 3 (docs/TODO-offen.md): UUID-Selektion, Duplicate, Group, Move, Collapse
    // ------------------------------------------------------------------

    #[test]
    fn selection_is_uuid_based_and_supports_children() {
        let mut list = CueList::new();
        list.add(audio("1"));
        let group = Cue::new_group("2", "g", GroupMode::Simultaneous, vec![audio("2.1")]);
        let child_id = group.children[0].id;
        list.add(group);
        list.add(audio("3"));

        // Einfachauswahl per Index (Top-Level).
        list.select(Some(2));
        assert_eq!(list.selected_index(), Some(2));
        assert_eq!(list.selection_len(), 1);

        // Group-Kind selektierbar (docs/TODO-offen.md Item 7, UUID-Selektion).
        list.select_id(Some(child_id));
        assert!(list.is_selected(child_id));
        assert_eq!(list.selected().unwrap().number, "2.1");
        // selected_index zeigt auf die Eltern-Gruppe.
        assert_eq!(list.selected_index(), Some(1));
        // selected_mut editiert das Kind.
        list.selected_mut().unwrap().name = "renamed".to_string();
        assert_eq!(list.cues()[1].children[0].name, "renamed");

        // Selektion überlebt Verschieben (UUID, kein Index-Fixup nötig).
        list.move_up(2); // „3" und Gruppe tauschen → Gruppe jetzt Index 2
        assert!(list.is_selected(child_id));
        assert_eq!(list.selected_index(), Some(2), "Eltern-Gruppe mitgewandert");
    }

    #[test]
    fn cmd_and_shift_click_build_multi_selection() {
        let mut list = CueList::new();
        for i in 1..=5 {
            list.add(audio(&i.to_string()));
        }
        let id = |list: &CueList, i: usize| list.cues()[i].id;

        list.select_id(Some(id(&list, 1)));
        list.toggle_select_id(id(&list, 3));
        assert_eq!(list.selection_len(), 2);
        assert!(list.is_selected(id(&list, 1)));
        assert!(list.is_selected(id(&list, 3)));
        assert_eq!(list.primary_selection(), Some(id(&list, 3)));

        // ⌘-Klick auf bereits selektierten Cue entfernt ihn wieder.
        list.toggle_select_id(id(&list, 1));
        assert_eq!(list.selection_len(), 1);

        // ⇧-Klick: Bereich Anker (3) bis 4 → 3 und 4 selektiert.
        list.select_id(Some(id(&list, 3)));
        list.select_range_to(id(&list, 4));
        assert_eq!(list.selection_len(), 2);
        assert!(list.is_selected(id(&list, 3)) && list.is_selected(id(&list, 4)));

        // ⇧-Klick rückwärts (Anker 4, Ziel 1) selektiert 1..=4.
        list.select_id(Some(id(&list, 4)));
        list.select_range_to(id(&list, 1));
        assert_eq!(list.selection_len(), 4);

        // Unbekannte ID wird nicht selektiert.
        list.toggle_select_id(Uuid::new_v4());
        assert_eq!(list.selection_len(), 4);
    }

    #[test]
    fn remove_by_id_removes_toplevel_and_child() {
        let mut list = CueList::new();
        list.add(audio("1"));
        let group = Cue::new_group("2", "g", GroupMode::Simultaneous, vec![audio("2.1")]);
        let child_id = group.children[0].id;
        list.add(group);

        // Kind entfernen.
        assert!(list.remove_by_id(child_id));
        assert_eq!(list.cues()[1].children.len(), 0);
        assert!(!list.remove_by_id(child_id), "zweites Mal: nicht gefunden");

        // Top-Level entfernen; Selektion verliert die ID.
        let id1 = list.cues()[0].id;
        list.select_id(Some(id1));
        assert!(list.remove_by_id(id1));
        assert_eq!(list.len(), 1);
        assert_eq!(list.selection_len(), 0);
        assert!(!list.remove_by_id(Uuid::new_v4()));
    }

    #[test]
    fn duplicate_creates_deep_copy_with_fresh_ids_and_unique_number() {
        let mut list = CueList::new();
        list.add(audio("1"));
        let group = Cue::new_group(
            "2",
            "g",
            GroupMode::Simultaneous,
            vec![audio("2.1").with_target("/media/a.wav")],
        );
        list.add(group);

        let new_index = list.duplicate(1).expect("Gruppe duplizierbar");
        assert_eq!(new_index, 2);
        assert_eq!(list.len(), 3);

        let orig = &list.cues()[1];
        let copy = &list.cues()[2];
        assert_ne!(orig.id, copy.id, "frische ID");
        assert_ne!(
            orig.children[0].id, copy.children[0].id,
            "Kinder bekommen ebenfalls frische IDs"
        );
        assert_eq!(copy.number, "3", "nächste freie Nummer");
        assert_eq!(copy.name, orig.name);
        assert_eq!(
            copy.children[0].target_path,
            Some("/media/a.wav".to_string())
        );
        assert_eq!(copy.status, CueStatus::Standby);
        // Kopie ist selektiert.
        assert_eq!(list.primary_selection(), Some(copy.id));
    }

    #[test]
    fn duplicate_ids_duplicates_whole_selection_in_order() {
        let mut list = CueList::new();
        for i in 1..=4 {
            list.add(audio(&i.to_string()));
        }
        let ids = vec![list.cues()[0].id, list.cues()[2].id];
        let copies = list.duplicate_ids(&ids);
        assert_eq!(list.len(), 6);
        assert_eq!(copies.len(), 2);
        // Kopien landen jeweils rechts neben dem Original (absteigend
        // dupliziert → „1"-Kopie bekommt die höhere Nummer).
        assert_eq!(list.cues()[1].number, "6");
        assert_eq!(list.cues()[4].number, "5");
        // Selektion = die Kopien (in List-Reihenfolge).
        assert_eq!(list.selection(), copies.as_slice());
    }

    #[test]
    fn group_ids_wraps_selection_into_a_new_group() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(audio("2"));
        list.add(audio("3"));
        list.add(audio("4"));

        let group_id = list
            .group_ids(&[list.cues()[1].id, list.cues()[2].id])
            .expect("Gruppierung möglich");
        assert_eq!(list.len(), 3, "2 Cues in die Gruppe gewandert");
        let group = &list.cues()[1];
        assert_eq!(group.id, group_id);
        assert_eq!(group.cue_type, CueType::Group);
        assert_eq!(group.children.len(), 2);
        assert_eq!(group.children[0].number, "2");
        assert_eq!(group.children[1].number, "3");
        assert_eq!(group.number, "5", "Gruppe bekommt nächste freie Nummer");
        // Reihenfolge: 1, Gruppe, 4.
        assert_eq!(list.cues()[0].number, "1");
        assert_eq!(list.cues()[2].number, "4");
        // Gruppe ist selektiert.
        assert_eq!(list.primary_selection(), Some(group_id));
        // GO triggert die Gruppe.
        let _ = list.go().unwrap();
        let _ = list.go().unwrap();
        assert_eq!(list.cues()[1].status, CueStatus::Playing);
    }

    #[test]
    fn move_to_reorders_and_playhead_follows() {
        let mut list = CueList::new();
        for i in 1..=4 {
            list.add(audio(&i.to_string()));
        }
        list.set_cursor(3);

        // „1" (Index 0) auf Index 2 verschieben → [2, 3, 1, 4].
        list.move_to(0, 2);
        let numbers: Vec<&str> = list.cues().iter().map(|c| c.number.as_str()).collect();
        assert_eq!(numbers, vec!["2", "3", "1", "4"]);
        assert_eq!(list.cursor(), 3, "Playhead folgt ‚4‘ (bleibt derselbe Cue)");

        // Playhead auf den verschobenen Cue: folgt mit.
        list.set_cursor(2); // „1"
        list.move_to(2, 0);
        let numbers: Vec<&str> = list.cues().iter().map(|c| c.number.as_str()).collect();
        assert_eq!(numbers, vec!["1", "2", "3", "4"]);
        assert_eq!(list.cursor(), 0, "Playhead folgt dem verschobenen Cue");
    }

    #[test]
    fn move_into_group_appends_cue_as_last_child() {
        let mut list = CueList::new();
        list.add(audio("1"));
        list.add(Cue::new_group(
            "2",
            "g",
            GroupMode::Simultaneous,
            vec![audio("2.1")],
        ));
        list.add(audio("3"));

        assert!(list.move_into_group(2, 1), "‚3‘ in Gruppe ‚2‘ legen");
        assert_eq!(list.len(), 2, "Top-Level geschrumpft");
        let group = &list.cues()[1];
        assert_eq!(group.children.len(), 2);
        assert_eq!(group.children[1].number, "3", "als letztes Kind angehängt");
        // Selektion = verschobenes Kind.
        assert_eq!(list.selected().unwrap().number, "3");

        // Nicht-Gruppe als Ziel → abgelehnt.
        assert!(!list.move_into_group(0, 0));
        // Selbst hineinlegen → abgelehnt.
        assert!(!list.move_into_group(1, 1));
    }

    #[test]
    fn group_collapse_is_stored_on_the_group_cue() {
        let mut list = CueList::new();
        list.add(Cue::new_group(
            "1",
            "g",
            GroupMode::Simultaneous,
            vec![audio("1.1")],
        ));
        assert!(!list.is_collapsed(0));
        list.set_collapsed(0, true);
        assert!(list.is_collapsed(0));
        // Nur Gruppen kollabierbar.
        list.add(audio("2"));
        list.set_collapsed(1, true);
        assert!(!list.is_collapsed(1));
        // Serde: collapsed ist abwärtskompatibel (Default false).
        let json = serde_json::to_string(&list).unwrap();
        let back: CueList = serde_json::from_str(&json).unwrap();
        assert!(back.is_collapsed(0));
        let old_json = json.replace(",\"collapsed\":true", "");
        let back_old: CueList = serde_json::from_str(&old_json).unwrap();
        assert!(!back_old.is_collapsed(0), "fehlendes Feld → Default");
    }

    #[test]
    fn wait_cue_triggers_engine_wait_command() {
        let mut list = CueList::new();
        let mut wait = Cue::new("1", "Pause", CueType::Wait);
        wait.duration = Duration::from_millis(1500);
        wait.pre_wait = Duration::from_millis(200);
        list.add(wait);

        let outcome = list.go().expect("GO must trigger the wait cue");
        assert_eq!(outcome.commands.len(), 1);
        assert!(
            matches!(&outcome.commands[0], EngineCommand::Wait { duration, pre_wait, .. }
                if *duration == Duration::from_millis(1500) && *pre_wait == Duration::from_millis(200)),
            "wait cue must produce EngineCommand::Wait with its durations"
        );
        // Pre-Wait > 0 → Status PreWait (wie normale Playbacks).
        assert_eq!(list.cues()[0].status, CueStatus::PreWait);
    }

    #[test]
    fn memo_cue_completes_instantly_without_command() {
        let mut list = CueList::new();
        list.add(Cue::new("1", "Memo", CueType::Memo));

        let outcome = list.go().expect("GO must trigger the memo cue");
        assert!(
            outcome.commands.is_empty(),
            "memo cue produces no engine command"
        );
        assert_eq!(
            list.cues()[0].status,
            CueStatus::Completed,
            "memo cue is completed immediately"
        );
    }

    #[test]
    fn text_cue_triggers_play_html_with_generated_inline_html() {
        let mut list = CueList::new();
        let mut cue = Cue::new("1", "Titel", CueType::Text);
        cue.stage = Some("Stage 1".to_string());
        cue.layer = 3;
        cue.text_content = Some(crate::core::TextContent {
            text: "Hallo <Welt>".to_string(),
            ..Default::default()
        });
        list.add(cue);

        let outcome = list.go().expect("GO must trigger the text cue");
        assert_eq!(outcome.commands.len(), 1);
        match &outcome.commands[0] {
            EngineCommand::PlayHtml {
                source,
                stage,
                layer,
                ..
            } => {
                assert_eq!(stage.as_deref(), Some("Stage 1"));
                assert_eq!(*layer, 3);
                match source {
                    crate::core::HtmlSource::Inline(html) => {
                        assert!(html.contains("Hallo &lt;Welt&gt;"));
                    }
                    other => panic!("expected inline html, got {other:?}"),
                }
            }
            other => panic!("expected PlayHtml, got {other:?}"),
        }
        assert_eq!(list.cues()[0].status, CueStatus::Playing);
    }

    #[test]
    fn text_cue_without_content_goes_to_error_not_panic() {
        let mut list = CueList::new();
        list.add(Cue::new("1", "leer", CueType::Text));
        let outcome = list.go().expect("GO must trigger");
        assert!(outcome.commands.is_empty());
        assert_eq!(list.cues()[0].status, CueStatus::Error);
    }

    #[test]
    fn wait_and_memo_serde_roundtrip_and_legacy() {
        // Alte Shows kennen Wait/Memo nicht – Serde muss abwärtskompatibel sein.
        let mut wait = Cue::new("1", "w", CueType::Wait);
        wait.duration = Duration::from_secs(2);
        let json = serde_json::to_string(&wait).unwrap();
        assert!(json.contains("\"Wait\""));
        let back: Cue = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cue_type, CueType::Wait);
        assert_eq!(back.duration, Duration::from_secs(2));

        assert_eq!(CueType::Wait.label(), "Wait");
        assert_eq!(CueType::Memo.label(), "Memo");
        assert_eq!(CueType::Text.label(), "Text");
        assert!(CueType::ALL.contains(&CueType::Wait));
        assert!(CueType::ALL.contains(&CueType::Memo));
        assert!(CueType::ALL.contains(&CueType::Text));

        // Text-Cue: text_content ist abwärtskompatibel (fehlt in alten Shows).
        let mut text = Cue::new("2", "t", CueType::Text);
        text.text_content = Some(crate::core::TextContent::default());
        let json = serde_json::to_string(&text).unwrap();
        let back: Cue = serde_json::from_str(&json).unwrap();
        assert!(back.text_content.is_some());
        let old_json = json.replace(",\"text_content\":{\"text\":\"\",\"font_size\":64.0,\"color\":[255,255,255],\"bg_color\":null,\"align\":\"Left\",\"valign\":\"Top\"}", "");
        let back_old: Cue = serde_json::from_str(&old_json).unwrap();
        assert!(back_old.text_content.is_none(), "fehlendes Feld → None");
    }
}
