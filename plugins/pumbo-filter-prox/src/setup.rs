//! Host-independent parts of the PumboProx layer: config and message files,
//! where the virtual world puts a player.

use pumbo_common::config::{self, Check, Settings, Warning};
use pumbo_common::lang::{Bundle, COMMON, Lang};
use pumbo_filter_core::autoban::AutoBanCfg;
use pumbo_filter_core::config::{
    AttackCfg, CaptchaCfg, CheckMode, ClientCheckCfg, FilterCfg, FilterSettings, GravityCfg,
};
use pumbo_filter_core::place::{Place, Pos};
use serde::{Deserialize, Serialize};

/// Plugin id in the manifest (`pumbo-filter.yml`); `pumbo:attack-state` belongs to it.
pub const PLUGIN_ID: &str = "pumbo-filter";

/// The config file with comments, for the admin to copy into `plugins/pumbo-filter/`.
pub const CONFIG_TEMPLATE: &str = include_str!("../assets/config.yml");

/// Messages of the PumboProx layer (prefix, the automatic block).
pub const LANG: Bundle = Bundle {
    name: "filter-prox",
    files: &[("en", include_str!("../lang/en.yml")), ("pl", include_str!("../lang/pl.yml"))],
};

/// Every message bundle, in load order.
pub const BUNDLES: [Bundle; 3] = [COMMON, pumbo_filter_core::LANG, LANG];

/// Where a player stands in the virtual world: on an invisible platform in the
/// void. The fall starts high above it ([`pumbo_filter_core::place::fall_start`]).
pub fn spawn() -> Place {
    Place {
        world: PLUGIN_ID.into(),
        // The void of the proxy has the height of the End.
        dimension: "minecraft:the_end".into(),
        pos: Pos { x: 0.5, y: 64.0, z: 0.5, yaw: 0.0, pitch: 0.0 },
    }
}

/// Blocks of the platform under [`spawn`] (barriers: nothing to see).
pub fn platform() -> impl Iterator<Item = (i32, i32, i32)> {
    (-1..=1).flat_map(|x| (-1..=1).map(move |z| (x, 63, z)))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ViaCfg {
    pub check_mode: CheckMode,
}

impl Default for ViaCfg {
    fn default() -> Self {
        Self { check_mode: CheckMode::OnlyCaptcha }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub language: String,
    pub per_player_language: bool,
    pub filter: FilterCfg,
    pub gravity: GravityCfg,
    pub captcha: CaptchaCfg,
    pub client_check: ClientCheckCfg,
    pub attack: AttackCfg,
    pub viaproxy: ViaCfg,
    pub auto_ban: AutoBanCfg,
}

impl Default for Config {
    fn default() -> Self {
        let s = FilterSettings::default();
        Self {
            language: "en".into(),
            per_player_language: true,
            filter: s.filter,
            gravity: s.gravity,
            captcha: s.captcha,
            client_check: s.client_check,
            attack: s.attack,
            viaproxy: ViaCfg::default(),
            auto_ban: AutoBanCfg::default(),
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
        self.language = self.language.trim().to_lowercase();
        let ok = !self.language.is_empty()
            && self.language.len() <= 16
            && self.language.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_');
        check.ensure("language", &mut self.language, "en".into(), |_| ok);
        let mut s = self.settings();
        s.validate(check);
        (self.filter, self.gravity, self.captcha, self.client_check, self.attack) =
            (s.filter, s.gravity, s.captcha, s.client_check, s.attack);
        self.auto_ban.validate(check, "auto-ban");
    }
}

/// Reads `config.yml` from the read-only config folder (`/config` in the
/// plugin); a missing file means the defaults.
pub fn load_config(dir: &str) -> (Config, Vec<Warning>, bool) {
    let (cfg, mut w, found) = match std::fs::read_to_string(format!("{dir}/config.yml")) {
        Ok(text) => {
            let (cfg, w) = config::load::<Config>(&text);
            (cfg, w, true)
        }
        Err(_) => (Config::default(), Vec::new(), false),
    };
    w.extend(config::old_files(dir));
    (cfg, w, found)
}

/// The configured language and, with `per-player-language`, every other one
/// with bundled messages or a file in `lang/` (overrides of the bundled ones).
pub fn load_langs(dir: &str, cfg: &Config) -> (Lang, Vec<Lang>, Vec<Warning>) {
    let lang_dir = format!("{dir}/lang");
    let mut codes: Vec<String> = Lang::builtin_codes(&BUNDLES).iter().map(|c| (*c).to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(&lang_dir) {
        for e in entries.flatten() {
            if let Some(code) = e.file_name().to_string_lossy().strip_suffix(".yml") {
                codes.push(code.to_string());
            }
        }
    }
    codes.retain(|c| *c != cfg.language);
    codes.sort();
    codes.dedup();
    let mut warnings = Vec::new();
    let mut load = |code: &str| {
        let user = std::fs::read_to_string(format!("{lang_dir}/{code}.yml")).ok();
        let (lang, w) = Lang::load(&BUNDLES, code, user.as_deref());
        warnings.extend(w.into_iter().map(|w| Warning { message: format!("lang/{code}.yml: {}", w.message), ..w }));
        lang
    };
    let default = load(&cfg.language);
    let others = if cfg.per_player_language { codes.iter().map(|c| load(c)).collect() } else { Vec::new() };
    (default, others, warnings)
}

#[cfg(test)]
mod tests {
    use pumbo_common::lang::check_bundle;

    use super::*;

    #[test]
    fn built_in_language_files_are_current() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/lang");
        let written = Lang::write_templates(&BUNDLES, &dir);
        assert!(written.is_empty(), "rewritten from the message bundles, commit them: {written:?}");
    }

    #[test]
    fn template_is_the_default() {
        let (cfg, w) = config::load::<Config>(CONFIG_TEMPLATE);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn messages_and_languages() {
        assert_eq!(check_bundle(&LANG), Vec::<String>::new());
        let dir = std::env::temp_dir().join(format!("pumbo-filter-prox-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let d = dir.to_string_lossy().to_string();
        let (cfg, w, found) = load_config(&d);
        assert!(w.is_empty() && !found && cfg == Config::default());
        let (default, others, w) = load_langs(&d, &cfg);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(default.code(), "en");
        assert_eq!(others.iter().map(|l| l.code().to_string()).collect::<Vec<_>>(), ["pl"]);
        assert!(others[0].get("kick-bot-blocked").contains("adresu"));
        let single = Config { language: "pl".into(), per_player_language: false, ..Config::default() };
        let (default, others, _) = load_langs(&d, &single);
        assert_eq!((default.code(), others.len()), ("pl", 0));
        let _ = std::fs::remove_dir(&dir);
    }
}
