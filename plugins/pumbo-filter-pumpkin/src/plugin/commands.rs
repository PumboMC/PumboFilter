//! `/pumbofilter` (console and players) and `/captcha` (only for Tab: a held
//! player's `/captcha` is handled and cancelled in the command event).

use pumbo_common::clock::now_ms;
use pumbo_common::rich::Text;
use pumbo_filter_core::commands::{self, Outcome, Sender};

use super::api::{self, Arg, CommandError, CommandHandler, CommandSender, ConsumedArgs, Server};
use super::state;
use crate::pumpkin_node;

pub struct Admin {
    pub with_args: bool,
}

impl CommandHandler for Admin {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let raw = if self.with_args {
            match args.get_value("args") {
                Arg::Simple(s) | Arg::Msg(s) => s,
                _ => String::new(),
            }
        } else {
            String::new()
        };
        let args: Vec<String> = raw.split_whitespace().map(String::from).collect();
        let player = sender.as_player();
        // Permissions are read from the host before the state is taken.
        let nodes: Vec<(String, bool)> = commands::tree()
            .subs()
            .iter()
            .map(|s| {
                let node = commands::tree().permission(s);
                let ok = player.as_ref().is_none_or(|p| p.has_permission(&pumpkin_node(&node)));
                (node, ok)
            })
            .collect();
        let allowed = |n: &str| nodes.iter().any(|(node, ok)| node == n && *ok);
        let s = Sender { console: player.is_none(), allowed: &allowed };
        let out = state::with(|rt| {
            let out = commands::run(&mut rt.gate, &s, &args, api::PLATFORM, now_ms());
            match out {
                Outcome::Reply(t) => t,
                Outcome::Reload => super::reload(rt),
            }
        })
        .unwrap_or_else(|| Text::parse("&cPumboFilter is busy, try again."));
        api::reply(&sender, &out);
        Ok(1)
    }
}

pub struct Captcha;

impl CommandHandler for Captcha {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let text = state::with(|rt| {
            pumbo_common::style::error(&rt.gate.lang, "command-players-only", &pumbo_common::text::Args::new())
        })
        .unwrap_or_default();
        api::reply(&sender, &text);
        Ok(1)
    }
}
