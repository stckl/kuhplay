//! Clock-Trigger: Start zu fester Uhrzeit – einmalig mit Datum oder
//! wiederkehrend (täglich / an bestimmten Wochentagen).
//!
//! Datum und Uhrzeit gelten in der **Ortszeit** des Rechners (inkl.
//! Sommerzeit, über `chrono::Local`). Fällt eine Uhrzeit in die
//! Zeitumstellung, gilt: gibt es sie nicht (Frühjahr), fällt der Termin an
//! diesem Tag aus; gibt es sie doppelt (Herbst), zählt das erste Mal.

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Wochentag-Bitmaske (Mo=1 … So=64).
pub type Weekdays = u8;

pub const MON: u8 = 1;
pub const TUE: u8 = 2;
pub const WED: u8 = 4;
pub const THU: u8 = 8;
pub const FRI: u8 = 16;
pub const SAT: u8 = 32;
pub const SUN: u8 = 64;
pub const ALL_DAYS: u8 = 0x7F;

/// Datum (optional) + Uhrzeit + Wochentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockTrigger {
    /// Einmalig-Datum (JJJJ-MM-TT); `None` = wiederkehrend.
    #[serde(default)]
    pub date: Option<(u16, u8, u8)>,
    /// Uhrzeit (Stunde, Minute, Sekunde).
    pub time: (u8, u8, u8),
    /// Wochentage (Bitmaske); nur bei `date: None`. 0/ALL_DAYS = täglich.
    #[serde(default)]
    pub weekdays: Weekdays,
}

impl ClockTrigger {
    pub fn daily(h: u8, m: u8, s: u8) -> Self {
        Self {
            date: None,
            time: (h, m, s),
            weekdays: ALL_DAYS,
        }
    }

    pub fn at_date(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Self {
        Self {
            date: Some((y, mo, d)),
            time: (h, mi, s),
            weekdays: ALL_DAYS,
        }
    }

    pub fn label(&self) -> String {
        let (h, m, s) = self.time;
        let t = format!("{h:02}:{m:02}:{s:02}");
        match self.date {
            Some((y, mo, d)) => format!("{y:04}-{mo:02}-{d:02} {t} (einmalig)"),
            None => {
                let days = weekday_names(self.weekdays);
                if self.weekdays == ALL_DAYS || self.weekdays == 0 {
                    format!("täglich {t}")
                } else {
                    format!("{days} {t}")
                }
            }
        }
    }

    /// Nächste Auslösung nach `now` in Ortszeit; `None` = nie (einmalig und
    /// schon vorbei).
    pub fn next_after(&self, now: SystemTime) -> Option<SystemTime> {
        self.next_after_in(&DateTime::<Local>::from(now))
            .map(SystemTime::from)
    }

    /// Wie [`Self::next_after`] in der Zeitzone von `now` (Tests: feste
    /// Zeitzonen statt der Rechner-Ortszeit).
    pub fn next_after_in<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        let (h, m, s) = self.time;
        let tod = NaiveTime::from_hms_opt(h.into(), m.into(), s.into())?;
        let tz = now.timezone();
        let at = |date: NaiveDate| tz.from_local_datetime(&date.and_time(tod)).earliest();
        if let Some((y, mo, d)) = self.date {
            let date = NaiveDate::from_ymd_opt(y.into(), mo.into(), d.into())?;
            return at(date).filter(|t| t > now);
        }
        // Wiederkehrend: heute oder an einem der nächsten 7 Tage.
        let today = now.date_naive();
        (0..8u64)
            .filter_map(|add| today.checked_add_days(Days::new(add)))
            .filter(|date| self.allows(*date))
            .filter_map(at)
            .find(|t| t > now)
    }

    /// Ist der Wochentag von `date` freigegeben (0/ALL_DAYS = täglich)?
    fn allows(&self, date: NaiveDate) -> bool {
        let bit = 1u8 << date.weekday().num_days_from_monday();
        self.weekdays == 0 || self.weekdays & bit != 0
    }
}

pub fn weekday_names(mask: Weekdays) -> String {
    const NAMES: [(&str, u8); 7] = [
        ("Mo", MON),
        ("Di", TUE),
        ("Mi", WED),
        ("Do", THU),
        ("Fr", FRI),
        ("Sa", SAT),
        ("So", SUN),
    ];
    let mut out: Vec<&str> = NAMES
        .iter()
        .filter(|(_, b)| mask & b != 0)
        .map(|(n, _)| *n)
        .collect();
    if out.is_empty() {
        out.push("täglich");
    }
    out.join(", ")
}

/// Heutiges Datum in Ortszeit als (Jahr, Monat, Tag) – Vorgabe im Editor.
pub fn today_local() -> (u16, u8, u8) {
    let today = Local::now().date_naive();
    (today.year() as u16, today.month() as u8, today.day() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Weekday};

    /// Mitteleuropäische Winterzeit (UTC+1) als feste Zeitzone.
    fn cet() -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> {
        cet().with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn daily_trigger_finds_tomorrow_if_today_passed() {
        let next = ClockTrigger::daily(8, 0, 0)
            .next_after_in(&at(2026, 11, 2, 12, 0))
            .unwrap();
        assert_eq!(next, at(2026, 11, 3, 8, 0), "8:00 heute vorbei → morgen");
    }

    #[test]
    fn time_is_local_not_utc() {
        // 20:00 Ortszeit (UTC+1) ist 19:00 UTC – nicht 20:00 UTC.
        let next = ClockTrigger::daily(20, 0, 0)
            .next_after_in(&at(2026, 11, 2, 12, 0))
            .unwrap();
        assert_eq!(next.naive_utc().format("%H:%M").to_string(), "19:00");
    }

    #[test]
    fn weekday_trigger_skips_to_saturday() {
        // 2026-11-02 ist Montag.
        let ct = ClockTrigger {
            date: None,
            time: (9, 0, 0),
            weekdays: SAT,
        };
        let next = ct.next_after_in(&at(2026, 11, 2, 12, 0)).unwrap();
        assert_eq!(next.weekday(), Weekday::Sat);
        assert_eq!(next, at(2026, 11, 7, 9, 0), "Montag → Samstag");
    }

    #[test]
    fn one_shot_date_trigger() {
        let ct = ClockTrigger::at_date(2026, 11, 5, 20, 15, 0);
        let next = ct.next_after_in(&at(2026, 11, 2, 12, 0)).unwrap();
        assert_eq!(next, at(2026, 11, 5, 20, 15));
        // nach dem Termin: nie wieder
        assert!(ct.next_after_in(&at(2026, 11, 6, 0, 0)).is_none());
    }

    #[test]
    fn invalid_time_or_date_never_fires() {
        let now = at(2026, 11, 2, 12, 0);
        assert!(ClockTrigger::daily(25, 0, 0).next_after_in(&now).is_none());
        assert!(ClockTrigger::at_date(2026, 2, 30, 8, 0, 0)
            .next_after_in(&now)
            .is_none());
    }
}
