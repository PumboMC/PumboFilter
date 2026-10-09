//! PumboFilter core: the anti-bot logic, without platform code.
//!
//! - [`gate`]: the state of every player, the order of the checks and the
//!   effects a platform layer carries out (hold, teleport, map, kick, finish)
//! - [`place`]: where the gravity check happens on a server without a virtual
//!   limbo (high above the player's own place) and the stored way back
//! - [`physics`]: gravity check, reported positions against the vanilla free fall
//! - [`captcha`] and [`font`]: CAPTCHA codes drawn on a map image, with a pool
//! - [`client`]: client brand and settings check
//! - [`attack`]: attack mode (connection rate), reconnect gate, which checks run
//! - [`autoban`]: automatic bans of addresses that keep failing
//! - [`verified`]: players who passed recently (stored)
//! - [`commands`]: `/pumbofilter` and its subcommands
//! - [`config`]: the `filter`, `gravity`, `captcha`, `client-check` and
//!   `attack` sections
//!
//! Time comes in as `now` arguments; nothing here talks to a host.

pub mod attack;
pub mod autoban;
pub mod captcha;
pub mod client;
pub mod commands;
pub mod config;
pub mod font;
pub mod gate;
pub mod physics;
pub mod place;
pub mod verified;

/// Plugin id: permissions `pumbo.filter.<action>`, bundle name.
pub const ID: &str = "filter";

use pumbo_common::lang::Bundle;

/// Player-facing messages of the filter.
pub const LANG: Bundle =
    Bundle { name: "filter", files: &[("en", include_str!("../lang/en.yml")), ("pl", include_str!("../lang/pl.yml"))] };

#[cfg(test)]
mod tests {
    use pumbo_common::lang::{COMMON, Lang, check_bundle};

    use super::*;

    #[test]
    fn messages_are_complete_in_every_language() {
        assert_eq!(check_bundle(&LANG), Vec::<String>::new());
        let (pl, w) = Lang::load(&[COMMON, LANG], "pl", None);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(pl.fmt("captcha-wrong", &["2"]), "Zły kod. Zostało prób: 2.");
    }
}
