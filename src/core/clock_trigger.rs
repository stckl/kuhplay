//! Clock-Trigger: Start zu fester Uhrzeit – einmalig mit Datum oder
//! wiederkehrend (täglich / an bestimmten Wochentagen).

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

// --- Zivilkalender (Howard Hinnant) ---

/// Tage seit 1970-01-01 → (Jahr, Monat, Tag).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (Jahr, Monat, Tag) → Tage seit 1970-01-01.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Wochentag (0=So … 6=Sa) aus Tagen seit Epoche.
pub fn weekday_from_days(z: i64) -> u32 {
    ((z + 4).rem_euclid(7)) as u32 // 1970-01-01 war Donnerstag
}

fn weekday_bit(wd: u32) -> u8 {
    // wd: 0=So..6=Sa → Bitmaske So=64 … Mo=1
    match wd {
        0 => SUN,
        1 => MON,
        2 => TUE,
        3 => WED,
        4 => THU,
        5 => FRI,
        _ => SAT,
    }
}

/// SystemTime → (Tage seit Epoche, Sekunde des Tages, Wochentag-Bit).
pub fn split_system_time(t: SystemTime) -> (i64, u64, u8) {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let days = d.as_secs() / 86_400;
    let secs = d.as_secs() % 86_400;
    (
        days as i64,
        secs,
        weekday_bit(weekday_from_days(days as i64)),
    )
}

impl ClockTrigger {
    /// Nächste Auslösung nach `now` (SystemTime) als SystemTime; None = nie (einmalig in Vergangenheit).
    pub fn next_after(&self, now: SystemTime) -> Option<SystemTime> {
        let (days, secs, _wd) = split_system_time(now);
        let (h, m, s) = self.time;
        let tod = h as u64 * 3600 + m as u64 * 60 + s as u64;
        if let Some((y, mo, d)) = self.date {
            let day = days_from_civil(y as i64, mo as u32, d as u32);
            let at = day * 86_400 + tod as i64;
            let now_s = days * 86_400 + secs as i64;
            return if at > now_s {
                Some(UNIX_EPOCH + Duration::from_secs(at as u64))
            } else {
                None
            };
        }
        // wiederkehrend: heute oder die nächsten 8 Tage
        for add in 0..8i64 {
            let day = days + add;
            let wd = weekday_bit(weekday_from_days(day));
            let allowed = self.weekdays == 0 || self.weekdays & wd != 0;
            if !allowed {
                continue;
            }
            let at = day * 86_400 + tod as i64;
            let now_s = days * 86_400 + secs as i64;
            if at > now_s {
                return Some(UNIX_EPOCH + Duration::from_secs(at as u64));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_trigger_finds_tomorrow_if_today_passed() {
        // 2026-11-02 12:00:00 UTC = Montag
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000); // ~2027; genauer unten
        let _ = now;
        // exakt: 2026-11-02 12:00 UTC
        let days = days_from_civil(2026, 11, 2);
        let now = UNIX_EPOCH + Duration::from_secs((days * 86_400 + 12 * 3600) as u64);
        let ct = ClockTrigger::daily(8, 0, 0);
        let next = ct.next_after(now).unwrap();
        let (d2, s2, _) = split_system_time(next);
        assert_eq!(d2, days + 1, "8:00 heute vorbei → morgen");
        assert_eq!(s2, 8 * 3600);
    }

    #[test]
    fn weekday_trigger_skips_to_saturday() {
        // 2026-11-02 ist Montag
        let days = days_from_civil(2026, 11, 2);
        let now = UNIX_EPOCH + Duration::from_secs((days * 86_400 + 12 * 3600) as u64);
        let ct = ClockTrigger {
            date: None,
            time: (9, 0, 0),
            weekdays: SAT,
        };
        let next = ct.next_after(now).unwrap();
        let (d2, _, wd) = split_system_time(next);
        assert_eq!(wd, SAT);
        assert_eq!(d2 - days, 5, "Montag → Samstag = 5 Tage");
    }

    #[test]
    fn one_shot_date_trigger() {
        let days = days_from_civil(2026, 11, 2);
        let now = UNIX_EPOCH + Duration::from_secs((days * 86_400 + 12 * 3600) as u64);
        let ct = ClockTrigger::at_date(2026, 11, 5, 20, 15, 0);
        let next = ct.next_after(now).unwrap();
        let (d2, s2, _) = split_system_time(next);
        assert_eq!(d2, days_from_civil(2026, 11, 5));
        assert_eq!(s2, 20 * 3600 + 15 * 60);
        // nach dem Termin: nie wieder
        let later =
            UNIX_EPOCH + Duration::from_secs(((days_from_civil(2026, 11, 6)) * 86_400) as u64);
        assert!(ct.next_after(later).is_none());
    }
}
