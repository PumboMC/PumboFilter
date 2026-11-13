//! What PumboFilter shows on Pumpkin, kept in `tests/snapshots/chat.txt`: the
//! replies to `/pumbofilter`, what a checked player sees in chat, in titles
//! and above the hotbar, and the console. A screen starts with
//! `== <screen> <who>: <command or event>`; every chat message is one line of
//! its JSON text component, other places say what they are (`title:`,
//! `actionbar:`, `kick:`), and the console gets its plain text indented. The
//! screens carry the numbers of the chat UI mockups (F1-F6, S2, S4).
//! `PUMBO_SNAPSHOT_WRITE=1` writes the file anew.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::fmt::Write as _;
use std::path::PathBuf;

use pumbo_common::lang::Lang;
use pumbo_common::rich::{self, Text};
use pumbo_common::store::Store;
use pumbo_filter_core::commands::{self, Outcome, Sender};
use pumbo_filter_core::config::{CheckMode, FilterSettings};
use pumbo_filter_core::gate::{Effect, Gate, Joining};
use pumbo_filter_core::physics::ideal_reports;
use pumbo_filter_core::place::{Place, Pos};
use pumbo_filter_pumpkin::BUNDLES;

const STEVE: &str = "0b4f1c2a-0000-3000-8000-000000000001";
const T0: u64 = 1_000_000;
const PLATFORM: &str = "Pumpkin 0.2.2 (MC 26.3)";

#[derive(Default)]
struct Snapshot(String);

impl Snapshot {
    fn screen(&mut self, screen: &str, who: &str, what: &str) {
        writeln!(self.0, "\n== {screen} {who}: {what}").unwrap();
    }

    fn chat(&mut self, t: &Text) {
        writeln!(self.0, "{}", rich::json(t)).unwrap();
    }

    fn other(&mut self, place: &str, t: &Text) {
        writeln!(self.0, "{place}: {}", rich::json(t)).unwrap();
    }

    fn console(&mut self, t: &Text) {
        for line in t.plain().lines() {
            writeln!(self.0, "  {line}").unwrap();
        }
    }

    /// What a player sees of the effects (moves, holds and maps are left out).
    fn effects(&mut self, fx: &[Effect]) {
        for e in fx {
            match e {
                Effect::Say(_, t) => self.chat(t),
                Effect::Title { title, subtitle, .. } => {
                    self.other("title", title);
                    self.other("subtitle", subtitle);
                }
                Effect::ActionBar(_, t) => self.other("actionbar", t),
                Effect::Kick(_, t) => self.other("kick", t),
                _ => {}
            }
        }
    }
}

fn lang(code: &str, file: Option<&str>) -> Lang {
    let (lang, warnings) = Lang::load(&BUNDLES, code, file);
    assert!(warnings.is_empty(), "{warnings:?}");
    lang
}

fn gate(code: &str, mode: CheckMode) -> Gate {
    let mut cfg = FilterSettings::default();
    cfg.filter.check_mode = mode;
    Gate::new(cfg, lang(code, None), Ok(Store::in_memory()))
}

fn joining(name: &str) -> Joining {
    let pos = Pos { x: 12.5, y: 64.0, z: -7.5, yaw: 0.0, pitch: 0.0 };
    Joining {
        uuid: STEVE.into(),
        name: name.into(),
        ip: "203.0.113.7".into(),
        java: true,
        bypass: false,
        premium: false,
        hand_empty: true,
        place: Place { world: "world-0".into(), dimension: "minecraft:overworld".into(), pos },
        locale: None,
        mode: None,
    }
}

fn run(g: &mut Gate, console: bool, allowed: &dyn Fn(&str) -> bool, line: &str) -> Text {
    let args: Vec<String> = line.split_whitespace().map(String::from).collect();
    let sender = Sender { console, allowed };
    match commands::run(g, &sender, &args, PLATFORM, T0) {
        Outcome::Reply(t) => t,
        Outcome::Reload => commands::reloaded(g, 0),
    }
}

/// The admin screens of `/pumbofilter`.
fn admin(s: &mut Snapshot) {
    let all = |_: &str| true;
    let none = |_: &str| false;
    let mut g = gate("en", CheckMode::OnlyGravity);
    s.screen("F1", "admin", "/pumbofilter help");
    s.chat(&run(&mut g, false, &all, "help"));
    s.screen("F1", "console", "pumbofilter help");
    s.console(&run(&mut g, true, &all, "help"));

    g.stats.checked = 5_310;
    g.stats.passed = 5_102;
    g.stats.failed = 208;
    g.attack.blocked = 1_932;
    s.screen("F2", "admin", "/pumbofilter stats");
    s.chat(&run(&mut g, false, &all, "stats"));
    s.screen("F2", "console", "pumbofilter stats");
    s.console(&run(&mut g, true, &all, "stats"));

    s.screen("F6", "admin", "/pumbofilter attack on");
    s.chat(&run(&mut g, false, &all, "attack on"));
    s.screen("F6", "admin", "/pumbofilter stats");
    s.chat(&run(&mut g, false, &all, "stats"));

    for (screen, line) in [
        ("S2", "verify Steve 203.0.113.7"),
        ("S2", "verify Steve nope"),
        ("S2", "unverify Steve"),
        ("S2", "attack"),
        ("S2", "attack maybe"),
        ("S2", "atack"),
        ("S2", "version"),
    ] {
        s.screen(screen, "admin", &format!("/pumbofilter {line}"));
        s.chat(&run(&mut g, false, &all, line));
    }
    s.screen("S2", "admin", "reload with 2 warnings");
    s.chat(&commands::reloaded(&g, 2));
    s.screen("S2", "admin", "/pumbofilter reload");
    s.chat(&run(&mut g, false, &all, "reload"));
    s.screen("S2", "player", "/pumbofilter stats");
    s.chat(&run(&mut g, false, &none, "stats"));
    s.screen("S2", "player", "/pumbofilter help");
    s.chat(&run(&mut g, false, &none, "help"));
    s.screen("S2", "console", "pumbofilter verify Steve 203.0.113.7");
    s.console(&run(&mut g, true, &all, "verify Steve 203.0.113.7"));
}

/// A player through the gravity check: chat, title, the bar above the hotbar.
fn gravity(s: &mut Snapshot, code: &str) {
    let mut g = gate(code, CheckMode::OnlyGravity);
    s.screen("F3", &format!("player ({code})"), "join");
    let mut fx = g.join(joining("Steve"), T0);
    fx.extend(g.moved(STEVE, 12.5, 64.0, -7.5, T0 + 500));
    s.effects(&fx);
    s.screen("F5", &format!("player ({code})"), "falling");
    let mut shown = 0;
    for (i, y) in ideal_reports(1320.0, 200).into_iter().enumerate() {
        let fx = g.moved(STEVE, 12.5, y, -7.5, T0 + 1000 + i as u64 * 50);
        let done = fx.iter().any(|e| matches!(e, Effect::Finish(_) | Effect::Kick(..)));
        if done {
            s.screen("F3", &format!("player ({code})"), "check passed");
            s.effects(&fx);
            break;
        }
        // the first two bars and no more: the rest only counts up
        if shown < 2 && fx.iter().any(|e| matches!(e, Effect::ActionBar(..))) {
            s.effects(&fx);
            shown += 1;
        }
    }
    s.screen("F3", &format!("player ({code})"), "chat while checked");
    let mut g = gate(code, CheckMode::OnlyGravity);
    g.join(joining("Steve"), T0);
    s.effects(&g.chat(STEVE, "hi", T0 + 100).1);
}

/// A player through the CAPTCHA: the map, wrong codes up to the kick.
fn captcha(s: &mut Snapshot, code: &str) {
    let mut g = gate(code, CheckMode::OnlyCaptcha);
    s.screen("F3", &format!("player ({code})"), "captcha");
    let mut fx = g.join(joining("Steve"), T0);
    fx.extend(g.moved(STEVE, 12.5, 64.0, -7.5, T0 + 500));
    s.effects(&fx);
    for n in 1..=g.cfg.captcha.attempts {
        s.screen("F3", &format!("player ({code})"), &format!("wrong code {n}"));
        let fx = g.chat(STEVE, "nope", T0 + 1000 * u64::from(n)).1;
        s.effects(&fx);
        if fx.iter().any(|e| matches!(e, Effect::Kick(..))) {
            break;
        }
    }
}

/// S4: the server's own prefix in front of what a player sees.
fn own_prefix(s: &mut Snapshot) {
    let mut cfg = FilterSettings::default();
    cfg.filter.check_mode = CheckMode::OnlyCaptcha;
    let lang = lang("en", Some("prefix: \"&#7cc0ffSkyBlock &8» \"\n"));
    let mut g = Gate::new(cfg, lang, Ok(Store::in_memory()));
    s.screen("S4", "player", "captcha with prefix: SkyBlock");
    let mut fx = g.join(joining("Steve"), T0);
    fx.extend(g.moved(STEVE, 12.5, 64.0, -7.5, T0 + 500));
    fx.extend(g.chat(STEVE, "nope", T0 + 1000).1);
    s.effects(&fx);
}

#[test]
fn chat_screens() {
    let mut s = Snapshot::default();
    s.0.push_str("PumboFilter on Pumpkin: chat screens. Written by tests/chat.rs (PUMBO_SNAPSHOT_WRITE=1).\n");
    admin(&mut s);
    for code in ["en", "pl"] {
        gravity(&mut s, code);
        captcha(&mut s, code);
    }
    own_prefix(&mut s);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/chat.txt");
    if std::env::var_os("PUMBO_SNAPSHOT_WRITE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &s.0).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if old != s.0 {
        let first = old.lines().zip(s.0.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "tests/snapshots/chat.txt differs from line {} on (PUMBO_SNAPSHOT_WRITE=1 writes it anew):\n- {}\n+ {}",
            first + 1,
            old.lines().nth(first).unwrap_or(""),
            s.0.lines().nth(first).unwrap_or("")
        );
    }
}
