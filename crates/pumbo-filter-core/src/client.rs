//! Client check: real clients send their brand (`vanilla`, `fabric`, ...) and
//! their settings shortly after joining; many bots send neither.

use crate::config::ClientCheckCfg;

/// Why a client failed the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFailure {
    NoBrand,
    /// The brand contains a blocked word (`pattern` as configured).
    BlockedBrand {
        brand: String,
        pattern: String,
    },
    NoSettings,
}

impl ClientFailure {
    /// Message key of the kick.
    pub fn message_key(&self) -> &'static str {
        match self {
            ClientFailure::NoBrand | ClientFailure::BlockedBrand { .. } => "kick-client-brand",
            ClientFailure::NoSettings => "kick-client-settings",
        }
    }

    /// Reason for the log.
    pub fn reason(&self, after_ticks: u64) -> String {
        match self {
            ClientFailure::NoBrand => format!("no brand after {after_ticks} ticks"),
            ClientFailure::BlockedBrand { brand, pattern } => {
                format!("blocked brand \"{brand}\" (matches \"{pattern}\")")
            }
            ClientFailure::NoSettings => format!("no client settings after {after_ticks} ticks"),
        }
    }
}

/// Client settings as the host reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettings {
    pub locale: String,
    pub view_distance: u8,
    pub chat_colors: bool,
    pub text_filtering: bool,
    pub server_listing: bool,
}

impl ClientSettings {
    /// Whether these are the values Pumpkin fills in when the client never sent
    /// its settings (a real client almost never has exactly these).
    pub fn is_pumpkin_default(&self) -> bool {
        self.locale == "en_us"
            && self.view_distance == 8
            && self.chat_colors
            && !self.text_filtering
            && !self.server_listing
    }
}

/// Checks the brand (already cleaned with [`clean_brand`]) and whether the
/// client sent its settings.
pub fn check(cfg: &ClientCheckCfg, brand: &str, settings_sent: bool) -> Result<(), ClientFailure> {
    if cfg.check_brand {
        if brand.is_empty() {
            return Err(ClientFailure::NoBrand);
        }
        let lower = brand.to_lowercase();
        if let Some(pattern) = cfg.blocked_brands.iter().find(|b| !b.is_empty() && lower.contains(&b.to_lowercase())) {
            return Err(ClientFailure::BlockedBrand { brand: brand.to_string(), pattern: pattern.clone() });
        }
    }
    if cfg.check_settings && !settings_sent {
        return Err(ClientFailure::NoSettings);
    }
    Ok(())
}

/// Strips the VarInt length prefix that some hosts leave on the client brand,
/// and control characters.
pub fn clean_brand(raw: &str) -> String {
    let bytes = raw.as_bytes();
    // A VarInt prefix: bytes with the high bit set followed by a final byte,
    // whose decoded value equals the remaining length.
    let mut value: usize = 0;
    let mut shift = 0u32;
    for (i, b) in bytes.iter().enumerate().take(3) {
        value |= usize::from(b & 0x7F) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            let rest = bytes.len().saturating_sub(i + 1);
            if value == rest
                && rest > 0
                && let Some(s) = raw.get(i + 1..)
            {
                return s.to_string();
            }
            break;
        }
    }
    raw.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_prefix() {
        assert_eq!(clean_brand("\u{b}sonda-brand"), "sonda-brand");
        assert_eq!(clean_brand("vanilla"), "vanilla");
        assert_eq!(clean_brand("van\u{1}illa"), "vanilla");
        assert_eq!(clean_brand(""), "");
    }

    #[test]
    fn checks_brand_and_settings() {
        let mut cfg = ClientCheckCfg { blocked_brands: vec!["Bot".into(), String::new()], ..ClientCheckCfg::default() };
        assert_eq!(check(&cfg, "vanilla", true), Ok(()));
        assert_eq!(check(&cfg, "", true), Err(ClientFailure::NoBrand));
        let blocked = check(&cfg, "MineBot 1.0", true).unwrap_err();
        assert_eq!(blocked, ClientFailure::BlockedBrand { brand: "MineBot 1.0".into(), pattern: "Bot".into() });
        assert_eq!(blocked.message_key(), "kick-client-brand");
        assert!(blocked.reason(5).contains("matches \"Bot\""));
        let no_settings = check(&cfg, "fabric", false).unwrap_err();
        assert_eq!(no_settings, ClientFailure::NoSettings);
        assert_eq!(no_settings.message_key(), "kick-client-settings");
        assert_eq!(no_settings.reason(60), "no client settings after 60 ticks");
        cfg.check_brand = false;
        cfg.check_settings = false;
        assert_eq!(check(&cfg, "", false), Ok(()));
    }

    #[test]
    fn pumpkin_defaults() {
        let s = ClientSettings {
            locale: "en_us".into(),
            view_distance: 8,
            chat_colors: true,
            text_filtering: false,
            server_listing: false,
        };
        assert!(s.is_pumpkin_default());
        assert!(!ClientSettings { locale: "pl_pl".into(), ..s.clone() }.is_pumpkin_default());
        assert!(!ClientSettings { server_listing: true, ..s }.is_pumpkin_default());
    }
}
