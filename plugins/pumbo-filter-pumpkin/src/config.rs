//! `config.yml`: the language and the core sections.

use pumbo_common::config::{Check, Settings};
use pumbo_filter_core::config::{AttackCfg, CaptchaCfg, ClientCheckCfg, FilterCfg, FilterSettings, GravityCfg};
use serde::{Deserialize, Serialize};

/// The file written at the first start.
pub const TEMPLATE: &str = include_str!("../assets/config.yml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub language: String,
    pub filter: FilterCfg,
    pub gravity: GravityCfg,
    pub captcha: CaptchaCfg,
    pub client_check: ClientCheckCfg,
    pub attack: AttackCfg,
}

impl Default for Config {
    fn default() -> Self {
        let s = FilterSettings::default();
        Self {
            language: "en".into(),
            filter: s.filter,
            gravity: s.gravity,
            captcha: s.captcha,
            client_check: s.client_check,
            attack: s.attack,
        }
    }
}

impl Config {
    pub fn settings(&self) -> FilterSettings {
        FilterSettings {
            filter: self.filter.clone(),
            gravity: self.gravity.clone(),
            captcha: self.captcha.clone(),
            client_check: self.client_check.clone(),
            attack: self.attack.clone(),
        }
    }
}

impl Settings for Config {
    fn validate(&mut self, check: &mut Check<'_>) {
        let lang = self.language.trim().to_lowercase();
        check.ensure("language", &mut self.language, "en".into(), |_| {
            !lang.is_empty()
                && lang.len() <= 16
                && lang.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_')
        });
        self.language = self.language.trim().to_lowercase();
        let mut s = self.settings();
        s.validate(check);
        (self.filter, self.gravity, self.captcha, self.client_check, self.attack) =
            (s.filter, s.gravity, s.captcha, s.client_check, s.attack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumbo_common::config;

    #[test]
    fn template_gives_exactly_the_defaults() {
        let (c, w) = config::load::<Config>(TEMPLATE);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn bad_values_fall_back() {
        let (c, w) = config::load::<Config>("language: PL\ncaptcha:\n  mode: dialog\n  attempts: 0\n");
        assert_eq!(c.language, "pl");
        assert_eq!(c.captcha.mode, pumbo_filter_core::config::CaptchaMode::Map);
        assert_eq!(c.captcha.attempts, 1);
        assert_eq!(w.len(), 2, "{w:?}");
    }
}
