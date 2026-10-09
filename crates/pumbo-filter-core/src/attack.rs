//! Rate limits that live in memory: attack-mode detection and the reconnect gate.

use std::collections::{HashMap, VecDeque};

use crate::config::{AttackCfg, CheckMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackLevel {
    Normal,
    /// Stricter checks for everyone.
    Active,
    /// Unknown players additionally have to reconnect once.
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forced {
    Auto,
    On,
    Off,
}

/// Counts connections in a sliding window and switches attack mode on and off.
#[derive(Debug, Clone)]
pub struct AttackMonitor {
    events: VecDeque<u64>,
    active_until: u64,
    reconnect_until: u64,
    pub forced: Forced,
    pub total_connections: u64,
    pub blocked: u64,
    /// Connections and blocks since the last summary log line.
    pub window_connections: u64,
    pub window_blocked: u64,
}

impl Default for AttackMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl AttackMonitor {
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            active_until: 0,
            reconnect_until: 0,
            forced: Forced::Auto,
            total_connections: 0,
            blocked: 0,
            window_connections: 0,
            window_blocked: 0,
        }
    }

    fn prune(&mut self, now_ms: u64, cfg: &AttackCfg) {
        let window = u64::from(cfg.window_seconds) * 1000;
        while self.events.front().is_some_and(|t| now_ms.saturating_sub(*t) > window) {
            self.events.pop_front();
        }
    }

    /// Records a connection attempt and returns the resulting level.
    pub fn on_connection(&mut self, now_ms: u64, cfg: &AttackCfg) -> AttackLevel {
        self.total_connections += 1;
        self.window_connections += 1;
        if cfg.enabled {
            self.prune(now_ms, cfg);
            // Bounded memory even under a flood.
            if self.events.len() < 100_000 {
                self.events.push_back(now_ms);
            }
            let n = u32::try_from(self.events.len()).unwrap_or(u32::MAX);
            let cooldown = u64::from(cfg.cooldown_seconds) * 1000;
            if cfg.threshold > 0 && n >= cfg.threshold {
                self.active_until = now_ms + cooldown;
            }
            if cfg.reconnect_threshold > 0 && n >= cfg.reconnect_threshold {
                self.reconnect_until = now_ms + cooldown;
            }
        }
        self.level(now_ms, cfg)
    }

    pub fn level(&self, now_ms: u64, cfg: &AttackCfg) -> AttackLevel {
        match self.forced {
            Forced::On => {
                if now_ms < self.reconnect_until && cfg.enabled {
                    AttackLevel::Reconnect
                } else {
                    AttackLevel::Active
                }
            }
            Forced::Off => AttackLevel::Normal,
            Forced::Auto if !cfg.enabled => AttackLevel::Normal,
            Forced::Auto => {
                if now_ms < self.reconnect_until {
                    AttackLevel::Reconnect
                } else if now_ms < self.active_until {
                    AttackLevel::Active
                } else {
                    AttackLevel::Normal
                }
            }
        }
    }

    pub fn current_rate(&mut self, now_ms: u64, cfg: &AttackCfg) -> usize {
        self.prune(now_ms, cfg);
        self.events.len()
    }

    pub fn record_block(&mut self) {
        self.blocked += 1;
        self.window_blocked += 1;
    }

    /// Takes the counters for a summary log line.
    pub fn take_window(&mut self) -> (u64, u64) {
        let r = (self.window_connections, self.window_blocked);
        self.window_connections = 0;
        self.window_blocked = 0;
        r
    }
}

/// Remembers the first attempt of unknown players during an attack. A player who
/// comes back within the allowed delay is let through.
#[derive(Debug, Default, Clone)]
pub struct ReconnectGate {
    seen: HashMap<String, u64>,
}

const GATE_CAPACITY: usize = 50_000;

impl ReconnectGate {
    /// Returns true when the player may continue, false when they must reconnect.
    pub fn check(&mut self, key: &str, now_ms: u64, min_s: u32, max_s: u32) -> bool {
        let min = u64::from(min_s) * 1000;
        let max = u64::from(max_s) * 1000;
        if let Some(first) = self.seen.get(key).copied() {
            let age = now_ms.saturating_sub(first);
            if age >= min && age <= max {
                self.seen.remove(key);
                return true;
            }
            if age > max {
                self.seen.insert(key.to_string(), now_ms);
            }
            return false;
        }
        if self.seen.len() >= GATE_CAPACITY {
            self.seen.retain(|_, t| now_ms.saturating_sub(*t) <= max);
            if self.seen.len() >= GATE_CAPACITY {
                // Still full: refuse to track more, the player just has to retry later.
                return false;
            }
        }
        self.seen.insert(key.to_string(), now_ms);
        false
    }

    pub fn purge(&mut self, now_ms: u64, max_s: u32) {
        let max = u64::from(max_s) * 1000;
        self.seen.retain(|_, t| now_ms.saturating_sub(*t) <= max);
    }
}

/// Which checks a player goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checks {
    pub gravity: bool,
    pub captcha: bool,
    /// CAPTCHA only when the gravity check fails.
    pub captcha_on_fail: bool,
}

pub fn checks_for(mode: CheckMode, gravity_enabled: bool, captcha_enabled: bool) -> Checks {
    let (g, c, fallback) = match mode {
        CheckMode::Always => (true, true, false),
        CheckMode::OnlyGravity => (true, false, false),
        CheckMode::OnlyCaptcha => (false, true, false),
        CheckMode::GravityThenCaptcha => (true, false, true),
        CheckMode::Never => (false, false, false),
    };
    let gravity = g && gravity_enabled;
    let captcha_wanted = c || (fallback && !gravity);
    Checks {
        gravity,
        captcha: captcha_wanted && captcha_enabled,
        captcha_on_fail: fallback && gravity && captcha_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AttackCfg {
        AttackCfg {
            threshold: 5,
            reconnect_threshold: 10,
            window_seconds: 1,
            cooldown_seconds: 10,
            ..AttackCfg::default()
        }
    }

    #[test]
    fn attack_mode_switches_on_and_off() {
        let c = cfg();
        let mut m = AttackMonitor::new();
        for i in 0..4 {
            assert_eq!(m.on_connection(1000 + i, &c), AttackLevel::Normal);
        }
        assert_eq!(m.on_connection(1005, &c), AttackLevel::Active);
        for i in 0..5 {
            m.on_connection(1010 + i, &c);
        }
        assert_eq!(m.level(1020, &c), AttackLevel::Reconnect);
        // cooldown of 10 s after the last busy moment
        assert_eq!(m.level(1014 + 9_000, &c), AttackLevel::Reconnect);
        assert_eq!(m.level(1014 + 10_001, &c), AttackLevel::Normal);
        // slow connections never trigger it
        let mut slow = AttackMonitor::new();
        for i in 0..50 {
            assert_eq!(slow.on_connection(i * 2000, &c), AttackLevel::Normal);
        }
    }

    #[test]
    fn forced_and_disabled() {
        let mut c = cfg();
        let mut m = AttackMonitor::new();
        m.forced = Forced::On;
        assert_eq!(m.level(0, &c), AttackLevel::Active);
        m.forced = Forced::Off;
        for i in 0..20 {
            m.on_connection(i, &c);
        }
        assert_eq!(m.level(20, &c), AttackLevel::Normal);
        m.forced = Forced::Auto;
        c.enabled = false;
        assert_eq!(m.level(20, &c), AttackLevel::Normal);
        assert_eq!(m.total_connections, 20);
    }

    #[test]
    fn reconnect_gate() {
        let mut g = ReconnectGate::default();
        assert!(!g.check("bob@1.2.3.4", 0, 2, 60));
        // too fast
        assert!(!g.check("bob@1.2.3.4", 500, 2, 60));
        assert!(g.check("bob@1.2.3.4", 3_000, 2, 60));
        // too late starts over
        assert!(!g.check("amy@1.2.3.4", 0, 2, 60));
        assert!(!g.check("amy@1.2.3.4", 61_000, 2, 60));
        assert!(g.check("amy@1.2.3.4", 64_000, 2, 60));
    }

    #[test]
    fn check_selection() {
        let both = checks_for(CheckMode::Always, true, true);
        assert_eq!(both, Checks { gravity: true, captcha: true, captcha_on_fail: false });
        let fallback = checks_for(CheckMode::GravityThenCaptcha, true, true);
        assert_eq!(fallback, Checks { gravity: true, captcha: false, captcha_on_fail: true });
        // gravity disabled: the fallback CAPTCHA becomes the only check
        let no_grav = checks_for(CheckMode::GravityThenCaptcha, false, true);
        assert_eq!(no_grav, Checks { gravity: false, captcha: true, captcha_on_fail: false });
        let none = checks_for(CheckMode::Always, false, false);
        assert_eq!(none, Checks { gravity: false, captcha: false, captcha_on_fail: false });
        assert_eq!(
            checks_for(CheckMode::Never, true, true),
            Checks { gravity: false, captcha: false, captcha_on_fail: false }
        );
    }
}
