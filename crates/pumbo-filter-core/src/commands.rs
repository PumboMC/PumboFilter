//! `/pumbofilter` and its subcommands (permissions `pumbo.filter.<action>`).

use std::time::Duration;

use pumbo_common::command::{Commands, Dispatch, Sub};
use pumbo_common::help::{Help, page_arg};
use pumbo_common::id::parse_ip;
use pumbo_common::rich::{Line, Text};
use pumbo_common::style;
use pumbo_common::text::Args;

use crate::ID;
use crate::attack::{AttackLevel, Forced};
use crate::gate::Gate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Reload,
    Stats,
    Verify,
    Unverify,
    Attack,
    Version,
}

/// The command shown in usage lines and the help.
pub const LABEL: &str = "/pumbofilter";
/// Short command for [`LABEL`], on PumboProx (`short-alias`) and Pumpkin.
pub const ALIAS: &str = "pf";

pub fn tree() -> Commands<Action> {
    Commands::new(ID)
        .label(LABEL)
        .short_alias(format!("/{ALIAS}"))
        .with(Sub::new("reload", "reload", Action::Reload).description("help-reload"))
        .with(Sub::new("stats", "stats", Action::Stats).description("help-stats"))
        .with(
            Sub::new("verify", "verify", Action::Verify)
                .usage("<nick> <ip>", 2)
                .description("help-verify")
                .details("help-verify-details"),
        )
        .with(
            Sub::new("unverify", "unverify", Action::Unverify)
                .usage("<nick>", 1)
                .description("help-unverify")
                .details("help-unverify-details"),
        )
        .with(
            Sub::new("attack", "attack", Action::Attack)
                .usage("<on|off|auto>", 1)
                .description("help-attack")
                .details("help-attack-details"),
        )
        .with(Sub::new("version", "version", Action::Version).description("version-description"))
}

/// Who runs a command.
pub struct Sender<'a> {
    pub console: bool,
    /// Whether the sender has a permission node (`pumbo.filter.<action>`).
    pub allowed: &'a dyn Fn(&str) -> bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Reply(Text),
    /// Read the config and messages again, then reply with
    /// [`reloaded`].
    Reload,
}

/// Reply after a reload with `warnings` problems in the files.
pub fn reloaded(gate: &Gate, warnings: usize) -> Text {
    if warnings == 0 {
        style::success(&gate.lang, "command-reloaded", &Args::new())
    } else {
        style::warn(&gate.lang, "reloaded-warnings", &Args::new().arg(style::value(warnings)))
    }
}

fn help(gate: &Gate, sender: &Sender, page: usize) -> Text {
    let help = Help::from_commands("PumboFilter", &tree(), &gate.lang)
        .version(env!("CARGO_PKG_VERSION"))
        .section(gate.lang.get("help-section"));
    if sender.console { help.console(&gate.lang, sender.allowed) } else { help.chat(&gate.lang, page, sender.allowed) }
}

/// Runs `/pumbofilter <args>`. `platform` is shown by `version`.
pub fn run(gate: &mut Gate, sender: &Sender, args: &[String], platform: &str, now: u64) -> Outcome {
    let tree = tree();
    let lang = gate.lang.clone();
    let reply = |t: Text| Outcome::Reply(t);
    match tree.dispatch(args, sender.allowed) {
        Dispatch::Help => reply(help(gate, sender, page_arg(args))),
        Dispatch::Unknown { name } => reply(style::unknown_subcommand(&lang, &name, &tree.help_line())),
        Dispatch::NoPermission { .. } => reply(style::error(&lang, "command-no-permission", &Args::new())),
        Dispatch::Usage { usage } => reply(style::usage(&lang, &usage)),
        Dispatch::Run { sub, args } => match sub.handler {
            Action::Reload => Outcome::Reload,
            Action::Version => reply(style::version(
                "PumboFilter",
                env!("CARGO_PKG_VERSION"),
                [(lang.get("version-platform"), platform.to_string())],
            )),
            Action::Stats => reply(stats(gate, now)),
            Action::Verify => {
                let (Some(nick), Some(ip)) = (args.first(), args.get(1)) else {
                    return reply(style::usage(&lang, &tree.usage(sub)));
                };
                if parse_ip(ip).is_none() {
                    return reply(style::error(&lang, "invalid-ip", &Args::new().arg(style::value(ip))));
                }
                match gate.verify(nick, ip, now) {
                    Ok(()) => {
                        let ttl = Duration::from_millis(gate.cfg.filter.verified_ttl_ms());
                        let a = Args::new()
                            .arg(style::value(nick))
                            .arg(style::value(ip))
                            .arg(style::value(style::duration(&lang, ttl)));
                        reply(style::success(&lang, "verify-done", &a))
                    }
                    Err(e) => reply(style::error(&lang, "database-error", &Args::new().arg(style::value(e)))),
                }
            }
            Action::Unverify => {
                let Some(nick) = args.first() else { return reply(style::usage(&lang, &tree.usage(sub))) };
                match gate.unverify(nick) {
                    Ok(n) => reply(style::success(
                        &lang,
                        "unverify-done",
                        &Args::new().arg(style::value(n)).arg(style::value(nick)),
                    )),
                    Err(e) => reply(style::error(&lang, "database-error", &Args::new().arg(style::value(e)))),
                }
            }
            Action::Attack => {
                let forced = match args.first().map(|a| a.to_lowercase()).as_deref() {
                    Some("on") => Forced::On,
                    Some("off") => Forced::Off,
                    Some("auto") => Forced::Auto,
                    _ => return reply(style::error(&lang, "attack-usage", &Args::new())),
                };
                gate.attack.forced = forced;
                let level = level_word(gate, gate.level(now));
                reply(style::success(&lang, "attack-set", &Args::new().arg(style::value(level))))
            }
        },
    }
}

fn level_word(gate: &Gate, level: AttackLevel) -> String {
    gate.lang.get(match level {
        AttackLevel::Normal => "attack-normal",
        AttackLevel::Active => "attack-active",
        AttackLevel::Reconnect => "attack-reconnect",
    })
}

fn stats(gate: &mut Gate, now: u64) -> Text {
    let lang = gate.lang.clone();
    let s = gate.stats;
    let window = gate.cfg.attack.window_seconds;
    let rate = gate.attack.current_rate(now, &gate.cfg.attack);
    let level = level_word(gate, gate.level(now));
    let db = match &gate.store_error {
        None => lang.get("stats-ok"),
        Some(e) => e.clone(),
    };
    let n = |v: u64| style::value(style::number(&lang, v));
    let mut text = style::info(
        &lang,
        "stats-held",
        &Args::new().arg(n(gate.held().len() as u64)).arg(n(gate.online_count() as u64)),
    );
    let lines = [
        lang.format(
            "stats-checks",
            &Args::new().arg(n(s.checked)).arg(n(s.passed)).arg(n(s.failed)).arg(n(s.put_back)),
        ),
        lang.format(
            "stats-attack",
            &Args::new()
                .arg(style::value(level))
                .arg(n(rate as u64))
                .arg(n(u64::from(window)))
                .arg(n(gate.attack.blocked)),
        ),
        lang.format("stats-database", &Args::new().arg(style::value(db))),
    ];
    for l in lines {
        text.push(Line::parse(&format!("{}{l}", style::code(style::INFO))));
    }
    text
}

#[cfg(test)]
mod tests {
    use pumbo_common::lang::{COMMON, Lang};
    use pumbo_common::store::Store;

    use super::*;
    use crate::config::FilterSettings;

    fn gate() -> Gate {
        let (lang, _) = Lang::load(&[COMMON, crate::LANG], "en", Some("prefix: \"F » \"\n"));
        Gate::new(FilterSettings::default(), lang, Ok(Store::in_memory()))
    }

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn reply(o: Outcome) -> String {
        match o {
            Outcome::Reply(t) => t.plain(),
            Outcome::Reload => "RELOAD".into(),
        }
    }

    #[test]
    fn admin_commands() {
        let mut g = gate();
        let all = |_: &str| true;
        let s = Sender { console: true, allowed: &all };
        assert!(reply(run(&mut g, &s, &args(""), "test", 0)).contains("/pumbofilter verify <nick> <ip>"));
        assert_eq!(reply(run(&mut g, &s, &args("reload"), "test", 0)), "RELOAD");
        assert!(reply(run(&mut g, &s, &args("verify Steve 1.2.3.4"), "test", 0)).contains("Steve from 1.2.3.4"));
        assert!(reply(run(&mut g, &s, &args("verify Steve nope"), "test", 0)).contains("Invalid address"));
        assert!(reply(run(&mut g, &s, &args("unverify steve"), "test", 0)).contains("Removed 1 entries"));
        assert!(reply(run(&mut g, &s, &args("attack on"), "test", 0)).contains("Attack mode is now on"));
        assert!(reply(run(&mut g, &s, &args("attack maybe"), "test", 0)).contains("on, off or auto"));
        assert!(reply(run(&mut g, &s, &args("stats"), "test", 0)).contains("Database: ok"));
        assert!(reply(run(&mut g, &s, &args("version"), "test", 0)).contains("Platform: test"));
        assert!(reply(run(&mut g, &s, &args("relaod"), "test", 0)).contains("Unknown subcommand relaod"));
        assert!(reply(run(&mut g, &s, &args("verify"), "test", 0)).contains("Usage: /pumbofilter verify"));
        let none = |_: &str| false;
        let p = Sender { console: false, allowed: &none };
        assert!(reply(run(&mut g, &p, &args("stats"), "test", 0)).contains("permission"));
        assert!(reply(run(&mut g, &p, &args(""), "test", 0)).contains("no commands"));
    }
}
