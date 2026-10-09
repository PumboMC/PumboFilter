//! Automatic bans of addresses whose connections keep failing the checks
//! (`auto-ban`, off by default). With PumboBans the platform layer asks it
//! for a temporary IP ban; without it the address is refused from memory.

use std::collections::HashMap;
use std::net::IpAddr;

use pumbo_common::config::Check;
use pumbo_common::id::{Cidr, ip_key, parse_ip};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AutoBanCfg {
    pub enabled: bool,
    /// Failed checks from one address (IPv6: its /64) within the window.
    pub failures: u32,
    pub window_minutes: u32,
    pub ban_minutes: u32,
    pub reason: String,
    /// Networks never banned automatically (`10.0.0.0/8`, `2001:db8::/32`).
    pub whitelist: Vec<String>,
}

impl Default for AutoBanCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            failures: 20,
            window_minutes: 10,
            ban_minutes: 60,
            reason: "automatic: bot".into(),
            whitelist: Vec::new(),
        }
    }
}

impl AutoBanCfg {
    pub fn validate(&mut self, c: &mut Check<'_>, section: &str) {
        c.clamp(&format!("{section}.failures"), &mut self.failures, 1, 100_000);
        c.clamp(&format!("{section}.window-minutes"), &mut self.window_minutes, 1, 1440);
        c.clamp(&format!("{section}.ban-minutes"), &mut self.ban_minutes, 1, 525_600);
        let bad: Vec<String> = self.whitelist.iter().filter(|w| Cidr::parse(w).is_none()).cloned().collect();
        for w in bad {
            c.warn(&format!("{section}.whitelist"), format!("\"{w}\" is not an address or range, ignored"));
        }
    }

    fn whitelisted(&self, ip: IpAddr) -> bool {
        self.whitelist.iter().filter_map(|w| Cidr::parse(w)).any(|c| c.contains(ip))
    }
}

/// Failed checks per address and the addresses blocked in memory.
#[derive(Debug, Default)]
pub struct Strikes {
    failures: HashMap<String, Vec<u64>>,
    blocked: HashMap<String, u64>,
}

impl Strikes {
    /// A check failed for `address`. True when the address has just reached
    /// the limit: ban it (it is also blocked here, in case there is no PumboBans).
    pub fn fail(&mut self, cfg: &AutoBanCfg, address: &str, now: u64) -> bool {
        let Some(ip) = parse_ip(address).filter(|ip| cfg.enabled && !cfg.whitelisted(*ip)) else { return false };
        let window = u64::from(cfg.window_minutes) * 60_000;
        let key = ip_key(ip);
        let list = self.failures.entry(key.clone()).or_default();
        list.retain(|t| now.saturating_sub(*t) < window);
        list.push(now);
        if list.len() < cfg.failures as usize {
            return false;
        }
        self.failures.remove(&key);
        self.blocked.insert(key, now + u64::from(cfg.ban_minutes) * 60_000);
        true
    }

    /// Whether the address is blocked in memory right now.
    pub fn blocked(&self, address: &str, now: u64) -> bool {
        parse_ip(address).and_then(|ip| self.blocked.get(&ip_key(ip))).is_some_and(|until| *until > now)
    }

    /// Forgets old failures and ended blocks.
    pub fn purge(&mut self, cfg: &AutoBanCfg, now: u64) {
        let window = u64::from(cfg.window_minutes) * 60_000;
        self.failures.retain(|_, l| l.last().is_some_and(|t| now.saturating_sub(*t) < window));
        self.blocked.retain(|_, until| *until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bans_after_the_limit_within_the_window() {
        let cfg = AutoBanCfg { enabled: true, failures: 3, whitelist: vec!["10.0.0.0/8".into()], ..Default::default() };
        let mut s = Strikes::default();
        assert!(!s.fail(&cfg, "1.2.3.4", 0));
        assert!(!s.fail(&cfg, "1.2.3.4", 1000));
        // the first failure is out of the window
        assert!(!s.fail(&cfg, "1.2.3.4", 600_000));
        assert!(s.fail(&cfg, "1.2.3.4", 600_500));
        assert!(s.blocked("1.2.3.4", 600_600) && !s.blocked("1.2.3.5", 600_600));
        assert!(!s.blocked("1.2.3.4", 600_500 + 3_600_000));
        for t in 0..10 {
            assert!(!s.fail(&cfg, "10.1.2.3", t));
        }
        // one /64 counts as one address
        assert!(!s.fail(&cfg, "2001:db8::1", 0) && !s.fail(&cfg, "2001:db8::2", 0) && s.fail(&cfg, "2001:db8::3", 0));
        let off = AutoBanCfg { failures: 1, ..Default::default() };
        assert!(!s.fail(&off, "5.5.5.5", 0));
        s.purge(&cfg, 10_000_000);
        assert!(!s.blocked("1.2.3.4", 10_000_000));
    }
}
