//! Taktgeber-Abstraktion für deterministische Timing-Tests.
//!
//! Die Engine misst Zeit ausschließlich über [`Clock`]. In Produktion läuft
//! eine [`RealClock`] (monotone Wanduhr); in Tests eine [`VirtualClock`], die
//! der Test kontrolliert (`advance`) – so lassen sich Pre-Wait, Post-Wait,
//! Fades und Loop-Grenzen auf die **exakte Millisekunde** prüfen, ohne
//! `sleep()`-basierte Flaky-Tests.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Zeitquelle der Engine. Liefert die seit Engine-Start vergangene Zeit.
pub trait Clock: Send {
    /// Vergangene Zeit seit Engine-Start.
    fn now(&self) -> Duration;
}

/// Reale, monotone Uhr (Produktion).
#[derive(Debug)]
pub struct RealClock {
    start: Instant,
}

impl RealClock {
    /// Neue Uhr, gestartet ab jetzt.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Virtuelle Uhr (Tests). Läuft nur, wenn der Test sie vorstellt.
/// Clone-Handle teilen sich denselben Zeitstand – so kann der Test die Uhr
/// vorstellen, während die Engine (ggf. im selben Thread) arbeitet.
#[derive(Debug, Clone, Default)]
pub struct VirtualClock {
    time: Arc<Mutex<Duration>>,
}

impl VirtualClock {
    /// Neue virtuelle Uhr bei `t = 0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stellt die Uhr um `by` vor (Test-seitig).
    pub fn advance(&self, by: Duration) {
        let mut time = self.time.lock().unwrap_or_else(|e| e.into_inner());
        *time += by;
    }

    /// Setzt die Uhr auf einen absoluten Zeitpunkt (Test-seitig).
    pub fn set(&self, t: Duration) {
        let mut time = self.time.lock().unwrap_or_else(|e| e.into_inner());
        *time = t;
    }

    /// Aktueller virtueller Zeitpunkt.
    pub fn time(&self) -> Duration {
        *self.time.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Duration {
        self.time()
    }
}
