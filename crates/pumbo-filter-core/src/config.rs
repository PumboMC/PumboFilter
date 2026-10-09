//! Configuration sections of the filter: `filter`, `gravity`, `captcha`,
//! `client-check` and `attack`.
//!
//! The platform layer puts these sections into its own config struct and calls
//! each section's `validate` from its `Settings::validate`, passing the section
//! name used in its file (warnings name options as `<section>.<option>`).

use pumbo_common::config::Check;
use serde::{Deserialize, Serialize};

use crate::attack::AttackLevel;

/// Which checks a player goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckMode {
    /// Gravity check and CAPTCHA, both must pass.
    Always,
    OnlyGravity,
    OnlyCaptcha,
    /// Gravity check first; CAPTCHA only for players who fail it.
    GravityThenCaptcha,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaMode {
    Map,
    Title,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct FilterCfg {
    pub check_mode: CheckMode,
    pub attack_check_mode: CheckMode,
    pub verified_cache: bool,
    pub verified_ttl_hours: u32,
    /// Players authenticated by Mojang (version 4 UUID with a signed skin) skip
    /// the checks, except during an attack.
    pub skip_premium: bool,
    /// `nick@ip` pairs that skip the checks.
    pub whitelist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GravityCfg {
    pub enabled: bool,
    pub falling_check_ticks: u32,
    pub falling_grace_ticks: u32,
    pub max_y_difference: f64,
    pub max_y_errors: u32,
    pub max_xz_errors: u32,
    pub debug: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CaptchaCfg {
    pub enabled: bool,
    pub mode: CaptchaMode,
    pub attempts: u32,
    pub timeout_seconds: u32,
    pub length: u32,
    pub alphabet: String,
    pub ignore_case: bool,
    pub pool_size: u32,
    pub regenerate_minutes: u32,
    pub curves: u32,
    pub noise_dots: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ClientCheckCfg {
    pub enabled: bool,
    pub check_brand: bool,
    pub check_settings: bool,
    pub check_ticks: u32,
    pub blocked_brands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AttackCfg {
    pub enabled: bool,
    pub window_seconds: u32,
    pub threshold: u32,
    pub reconnect_threshold: u32,
    pub reconnect_min_seconds: u32,
    pub reconnect_max_seconds: u32,
    pub cooldown_seconds: u32,
    pub log_interval_seconds: u32,
}

impl Default for FilterCfg {
    fn default() -> Self {
        Self {
            check_mode: CheckMode::GravityThenCaptcha,
            attack_check_mode: CheckMode::Always,
            verified_cache: true,
            verified_ttl_hours: 12,
            skip_premium: true,
            whitelist: Vec::new(),
        }
    }
}

impl Default for GravityCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            falling_check_ticks: 128,
            falling_grace_ticks: 100,
            max_y_difference: 0.01,
            max_y_errors: 10,
            max_xz_errors: 10,
            debug: false,
        }
    }
}

impl Default for CaptchaCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: CaptchaMode::Map,
            attempts: 2,
            timeout_seconds: 30,
            length: 3,
            alphabet: "acdefhkmnprstuvwxy234678".into(),
            ignore_case: true,
            pool_size: 200,
            regenerate_minutes: 60,
            curves: 3,
            noise_dots: 220,
        }
    }
}

impl Default for ClientCheckCfg {
    fn default() -> Self {
        Self { enabled: true, check_brand: true, check_settings: true, check_ticks: 60, blocked_brands: Vec::new() }
    }
}

impl Default for AttackCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            window_seconds: 5,
            threshold: 25,
            reconnect_threshold: 80,
            reconnect_min_seconds: 2,
            reconnect_max_seconds: 120,
            cooldown_seconds: 60,
            log_interval_seconds: 10,
        }
    }
}

/// Every section of the filter, as a platform layer puts them in its file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct FilterSettings {
    pub filter: FilterCfg,
    pub gravity: GravityCfg,
    pub captcha: CaptchaCfg,
    pub client_check: ClientCheckCfg,
    pub attack: AttackCfg,
}

impl FilterSettings {
    /// Validates every section under its usual name.
    pub fn validate(&mut self, c: &mut Check<'_>) {
        self.filter.validate(c, "filter");
        self.gravity.validate(c, "gravity");
        self.captcha.validate(c, "captcha");
        self.client_check.validate(c, "client-check");
        self.attack.validate(c, "attack");
    }
}

fn opt(section: &str, name: &str) -> String {
    if section.is_empty() { name.to_string() } else { format!("{section}.{name}") }
}

impl FilterCfg {
    pub fn validate(&mut self, _c: &mut Check<'_>, _section: &str) {}

    /// Check mode for the current attack level.
    pub fn mode_for(&self, level: AttackLevel) -> CheckMode {
        if level == AttackLevel::Normal { self.check_mode } else { self.attack_check_mode }
    }

    /// Whether `nick@ip` is on the whitelist (nickname case-insensitive).
    pub fn is_whitelisted(&self, name: &str, ip: &str) -> bool {
        self.whitelist
            .iter()
            .any(|w| w.split_once('@').is_some_and(|(n, i)| n.trim().eq_ignore_ascii_case(name) && i.trim() == ip))
    }

    /// How long a passed player stays verified.
    pub fn verified_ttl_ms(&self) -> u64 {
        u64::from(self.verified_ttl_hours) * 3_600_000
    }
}

impl GravityCfg {
    pub fn validate(&mut self, c: &mut Check<'_>, section: &str) {
        let d = Self::default();
        c.clamp(&opt(section, "falling-check-ticks"), &mut self.falling_check_ticks, 2, 240);
        c.clamp(&opt(section, "falling-grace-ticks"), &mut self.falling_grace_ticks, 0, 200);
        c.clamp(&opt(section, "max-y-errors"), &mut self.max_y_errors, 1, 10_000);
        c.clamp(&opt(section, "max-xz-errors"), &mut self.max_xz_errors, 1, 10_000);
        c.ensure(&opt(section, "max-y-difference"), &mut self.max_y_difference, d.max_y_difference, |v| {
            v.is_finite() && *v > 0.0
        });
    }

    pub fn fall_params(&self) -> crate::physics::FallParams {
        crate::physics::FallParams {
            ticks: self.falling_check_ticks,
            max_y_difference: self.max_y_difference,
            max_y_errors: self.max_y_errors,
            max_xz_errors: self.max_xz_errors,
        }
    }
}

impl CaptchaCfg {
    pub fn validate(&mut self, c: &mut Check<'_>, section: &str) {
        c.clamp(&opt(section, "attempts"), &mut self.attempts, 1, 100);
        c.clamp(&opt(section, "timeout-seconds"), &mut self.timeout_seconds, 3, 3600);
        c.clamp(&opt(section, "length"), &mut self.length, 1, 6);
        c.clamp(&opt(section, "pool-size"), &mut self.pool_size, 1, 5000);
        c.clamp(&opt(section, "regenerate-minutes"), &mut self.regenerate_minutes, 1, 100_000);
        c.clamp(&opt(section, "curves"), &mut self.curves, 0, 20);
        c.clamp(&opt(section, "noise-dots"), &mut self.noise_dots, 0, 4000);
        // Keep only characters the font can draw, each once.
        let mut seen: Vec<char> = Vec::new();
        for ch in self.alphabet.chars() {
            let ch = ch.to_ascii_lowercase();
            if crate::font::glyph(ch).is_some() && !seen.contains(&ch) {
                seen.push(ch);
            }
        }
        if seen.is_empty() {
            let default = Self::default().alphabet;
            c.invalid(&opt(section, "alphabet"), &self.alphabet, &default);
            self.alphabet = default;
        } else {
            self.alphabet = seen.into_iter().collect();
        }
    }

    pub fn style(&self) -> crate::captcha::Style {
        crate::captcha::Style {
            length: self.length,
            alphabet: self.alphabet.chars().collect(),
            curves: self.curves,
            noise_dots: self.noise_dots,
        }
    }
}

impl ClientCheckCfg {
    pub fn validate(&mut self, c: &mut Check<'_>, section: &str) {
        c.clamp(&opt(section, "check-ticks"), &mut self.check_ticks, 1, 6000);
    }
}

impl AttackCfg {
    pub fn validate(&mut self, c: &mut Check<'_>, section: &str) {
        let d = Self::default();
        c.clamp(&opt(section, "window-seconds"), &mut self.window_seconds, 1, 600);
        c.clamp(&opt(section, "log-interval-seconds"), &mut self.log_interval_seconds, 1, 3600);
        if self.reconnect_max_seconds <= self.reconnect_min_seconds {
            c.invalid(&opt(section, "reconnect-max-seconds"), &self.reconnect_max_seconds, &d.reconnect_max_seconds);
            self.reconnect_min_seconds = d.reconnect_min_seconds;
            self.reconnect_max_seconds = d.reconnect_max_seconds;
        }
    }
}

#[cfg(test)]
mod tests {
    use pumbo_common::config::{self, Settings, Warning};

    use super::*;

    type Cfg = FilterSettings;

    impl Settings for FilterSettings {
        fn validate(&mut self, c: &mut Check<'_>) {
            FilterSettings::validate(self, c);
        }
    }

    fn names(w: &[Warning]) -> Vec<String> {
        let mut n: Vec<String> = w.iter().filter_map(|w| w.option.clone()).collect();
        n.sort();
        n
    }

    #[test]
    fn defaults_are_valid() {
        let (c, w) = config::load::<Cfg>("");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Cfg::default());
    }

    #[test]
    fn invalid_values_fall_back_per_option() {
        let text = "gravity:\n  falling-check-ticks: 1\n  max-y-errors: x\n  max-y-difference: 0\ncaptcha:\n  alphabet: \"!!\"\n  attempts: 0\nfilter:\n  check-mode: NOPE\nattack:\n  reconnect-min-seconds: 50\n  reconnect-max-seconds: 10\nclient-check:\n  check-ticks: 0\n";
        let (c, w) = config::load::<Cfg>(text);
        assert_eq!(c.gravity.falling_check_ticks, 2);
        assert!(c.filter.skip_premium);
        assert_eq!(c.gravity.max_y_errors, 10);
        assert!((c.gravity.max_y_difference - 0.01).abs() < f64::EPSILON);
        assert_eq!(c.captcha.attempts, 1);
        assert_eq!(c.captcha.alphabet, CaptchaCfg::default().alphabet);
        assert_eq!(c.filter.check_mode, CheckMode::GravityThenCaptcha);
        assert_eq!((c.attack.reconnect_min_seconds, c.attack.reconnect_max_seconds), (2, 120));
        assert_eq!(c.client_check.check_ticks, 1);
        assert_eq!(
            names(&w),
            vec![
                "attack.reconnect-max-seconds",
                "captcha.alphabet",
                "captcha.attempts",
                "client-check.check-ticks",
                "filter.check-mode",
                "gravity.falling-check-ticks",
                "gravity.max-y-difference",
                "gravity.max-y-errors",
            ]
        );
    }

    #[test]
    fn alphabet_is_cleaned() {
        let (c, w) = config::load::<Cfg>("captcha:\n  alphabet: AAb!c\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.captcha.alphabet, "abc");
        assert_eq!(c.captcha.style().alphabet, vec!['a', 'b', 'c']);
    }

    #[test]
    fn whitelist_and_modes() {
        let f = FilterCfg { whitelist: vec!["Steve@1.2.3.4".into(), "broken".into()], ..FilterCfg::default() };
        assert!(f.is_whitelisted("steve", "1.2.3.4"));
        assert!(!f.is_whitelisted("steve", "1.2.3.5"));
        assert!(!f.is_whitelisted("broken", ""));
        assert_eq!(f.mode_for(AttackLevel::Normal), CheckMode::GravityThenCaptcha);
        assert_eq!(f.mode_for(AttackLevel::Active), CheckMode::Always);
        assert_eq!(f.verified_ttl_ms(), 12 * 3_600_000);
        assert_eq!(GravityCfg::default().fall_params().ticks, 128);
    }
}
