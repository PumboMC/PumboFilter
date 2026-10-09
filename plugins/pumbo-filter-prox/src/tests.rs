//! The plugin on the fake host of the SDK (`pumbo_sdk::testing`), virtual
//! world included.

use pumbo_filter_core::physics::ideal_reports;
use pumbo_sdk::bindings::pumbo::prox::types::ClientSettings;
use pumbo_sdk::testing::{self, Sent, Virtual, block_on};
use pumbo_sdk::virtual_world::Position;
use pumbo_sdk::{Connection, Plugin};

use super::*;

type Call = (PlayerId, Virtual);

/// The virtual world calls so far, emptied.
fn take() -> Vec<Call> {
    testing::with(|h| std::mem::take(&mut h.virtual_calls))
}

const MANIFEST: &str = include_str!("../pumbo-filter.yml");

fn start(config: Option<&str>) -> PumboFilter {
    testing::reset();
    let dir = std::env::temp_dir().join(format!(
        "pumbo-filter-prox-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("config.yml"));
    if let Some(c) = config {
        std::fs::write(dir.join("config.yml"), c).unwrap();
    }
    let d = dir.to_string_lossy().to_string();
    let p = PumboFilter { state: RefCell::new(None), config_dir: d.clone(), data_dir: d };
    p.start(Ok(Store::in_memory())).unwrap();
    p
}

fn settings(locale: &str) -> ClientSettings {
    testing::settings(locale)
}

/// A player as the proxy shows it to gates: no server yet, settings and brand
/// from the configuration phase.
fn joining(name: &str, locale: &str) -> PlayerId {
    let id = testing::add_player(name, None);
    testing::with(|h| {
        let p = h.players.get_mut(&id).unwrap();
        p.settings = Some(settings(locale));
        p.in_virtual = false;
    });
    id
}

fn info(id: PlayerId) -> PlayerInfo {
    players::get(id).unwrap()
}

/// Plain text of a JSON component or any other text.
fn plain(t: &Text) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        if let Some(s) = v["text"].as_str() {
            out.push_str(s);
        }
        for e in v["extra"].as_array().into_iter().flatten() {
            walk(e, out);
        }
    }
    match t {
        Text::Json(s) => {
            let mut out = String::new();
            walk(&serde_json::from_str(s).unwrap(), &mut out);
            out
        }
        other => testing::render(other),
    }
}

fn messages(id: PlayerId) -> Vec<String> {
    testing::sent_to(id)
        .into_iter()
        .filter_map(|s| match s {
            Sent::Message(t) => Some(plain(&t)),
            _ => None,
        })
        .collect()
}

fn kicked(id: PlayerId) -> Option<String> {
    testing::sent_to(id).into_iter().find_map(|s| match s {
        Sent::Kick(t) => Some(plain(&t)),
        _ => None,
    })
}

fn input(p: &PumboFilter, batch: Vec<Input>) {
    block_on(p.on_virtual_input(batch));
}

fn moved(id: PlayerId, x: f64, y: f64, z: f64) -> Input {
    Input::Moved((id, Position { x, y, z, yaw: 0.0, pitch: 0.0 }, false))
}

fn teleports(calls: &[Call]) -> Vec<Position> {
    calls
        .iter()
        .filter_map(|c| match c {
            (_, Virtual::Teleport(p, _)) => Some(*p),
            _ => None,
        })
        .collect()
}

/// Spawn confirmed, then the vanilla free fall from the start point.
fn fall(p: &PumboFilter, id: PlayerId) -> Vec<Call> {
    input(p, vec![Input::Loaded(id), moved(id, 0.5, 64.0, 0.5)]);
    let calls = take();
    let up = teleports(&calls);
    assert_eq!(up.len(), 1, "{calls:?}");
    let start = up[0];
    assert!(start.y > 1000.0, "{start:?}");
    let reports = ideal_reports(start.y, 140);
    input(p, reports.iter().map(|y| moved(id, start.x, *y, start.z)).collect());
    take()
}

#[test]
fn manifest_declares_the_gate_and_its_events() {
    let m = pumbo_common::config::parse_yaml(MANIFEST).unwrap();
    assert_eq!(m["id"].as_str(), Some(setup::PLUGIN_ID));
    assert_eq!(m["gate"]["priority"].as_i64(), Some(100));
    assert_eq!(m["short-alias"].as_str(), Some(pumbo_filter_core::commands::ALIAS));
    let events: Vec<&str> = m["events"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    for e in ["pre-login", "gate", "virtual-input", "disconnect", "timer", "command"] {
        assert!(events.contains(&e), "{e}");
    }
    assert_eq!(m["publishes"][0].as_str(), Some("pumbo:attack-state@1.0"));
    assert_eq!(m["uses"][0]["service"].as_str(), Some(pumbo_common::bans::SERVICE));
    let nodes: Vec<&str> = m["permissions"].as_array().unwrap().iter().filter_map(|p| p["node"].as_str()).collect();
    for s in cmds::tree().subs() {
        assert!(nodes.contains(&cmds::tree().permission(s).as_str()), "{}", s.name);
    }
    assert!(nodes.contains(&BYPASS));
}

#[test]
fn new_player_falls_in_the_void_and_goes_on() {
    let p = start(None);
    let id = joining("Steve", "en_us");
    assert_eq!(block_on(p.on_gate(info(id))), GateReply::Hold);
    let calls = take();
    assert!(
        matches!(&calls[..], [(i, Virtual::Enter { at, commands, .. })] if *i == id && at.y == 64.0 && commands == &["captcha"]),
        "{calls:?}"
    );
    // At once: the proxy keeps them until the world shows.
    assert!(messages(id).iter().any(|m| m.contains("Checking your connection")), "{:?}", messages(id));
    assert!(testing::sent_to(id).iter().any(|s| matches!(s, Sent::Title(..))));
    let after = fall(&p, id);
    assert!(after.contains(&(id, Virtual::Release)), "{after:?}");
    assert!(messages(id).iter().any(|m| m.contains("Check passed")));
    assert!(kicked(id).is_none());
    // Verified now: the next join passes at once, without the world.
    block_on(p.on_disconnect(id));
    let again = joining("Steve", "en_us");
    assert_eq!(block_on(p.on_gate(info(again))), GateReply::Pass);
    assert!(take().is_empty());
}

#[test]
fn the_first_message_is_in_the_clients_language() {
    let p = start(None);
    let pl = joining("Polak", "pl_pl");
    block_on(p.on_gate(info(pl)));
    assert!(messages(pl).iter().any(|m| m.contains("Sprawdzamy połączenie")), "{:?}", messages(pl));
    // No settings (a bot, ViaProxy): the configured language.
    let bot = joining("Bot", "pl_pl");
    testing::with(|h| h.players.get_mut(&bot).unwrap().settings = None);
    block_on(p.on_gate(info(bot)));
    assert!(messages(bot).iter().any(|m| m.contains("Checking your connection")), "{:?}", messages(bot));
}

#[test]
fn premium_and_bypass_skip_the_world() {
    let p = start(None);
    let id = joining("Notch", "en_us");
    testing::with(|h| h.players.get_mut(&id).unwrap().online_mode = true);
    assert_eq!(block_on(p.on_gate(info(id))), GateReply::Pass);
    let other = joining("Mod", "en_us");
    testing::grant(other, BYPASS);
    assert_eq!(block_on(p.on_gate(info(other))), GateReply::Pass);
    assert!(take().is_empty());
}

#[test]
fn held_players_cannot_use_commands_or_chat() {
    let p = start(None);
    let id = joining("Steve", "pl_pl");
    block_on(p.on_gate(info(id)));
    input(&p, vec![Input::Loaded(id), Input::Command((id, "server lobby".into())), Input::Chat((id, "hi".into()))]);
    let said = messages(id);
    assert!(said.iter().filter(|m| m.contains("Najpierw dokończ sprawdzanie")).count() == 2, "{said:?}");
    assert!(said.iter().any(|m| m.contains("Sprawdzamy połączenie")), "{said:?}");
    assert!(!take().contains(&(id, Virtual::Release)));
}

#[test]
fn a_bad_fall_ends_in_the_captcha_on_the_map() {
    let p = start(None);
    let id = joining("Bot", "en_us");
    block_on(p.on_gate(info(id)));
    input(&p, vec![Input::Loaded(id), moved(id, 0.5, 64.0, 0.5)]);
    let start = teleports(&take())[0];
    // Hovering instead of falling.
    input(&p, (0..30).map(|i| moved(id, start.x, start.y - f64::from(i) * 0.01, start.z)).collect());
    let calls = take();
    assert!(calls.contains(&(id, Virtual::ShowMap(v::Hand::Main))), "{calls:?}");
    assert_eq!(teleports(&calls).last().map(|t| t.y), Some(64.0));
    input(&p, vec![Input::Chat((id, "nope".into()))]);
    assert!(messages(id).iter().any(|m| m.contains("Wrong code")));
    input(&p, vec![Input::Command((id, "captcha nope".into()))]);
    assert!(kicked(id).unwrap().contains("Wrong code"));
}

#[test]
fn missing_client_brand_and_settings_kick() {
    let p = start(Some("client-check:\n  check-ticks: 1\n"));
    let id = joining("Bot", "en_us");
    testing::with(|h| {
        let pl = h.players.get_mut(&id).unwrap();
        pl.brand = None;
        pl.settings = None;
    });
    block_on(p.on_gate(info(id)));
    std::thread::sleep(std::time::Duration::from_millis(60));
    block_on(p.on_timer(1));
    assert!(kicked(id).is_some_and(|k| k.contains("not supported") || k.contains("settings")), "{:?}", kicked(id));
}

#[test]
fn clients_through_viaproxy_get_the_captcha_only() {
    let p = start(None);
    let id = joining("Old", "en_us");
    testing::with(|h| h.players.get_mut(&id).unwrap().connection.route = Route::Viaproxy);
    block_on(p.on_gate(info(id)));
    take();
    let titles = || testing::sent_to(id).iter().filter(|s| matches!(s, Sent::Title(..))).count();
    assert_eq!(titles(), 1, "the check title, at once");
    // The spawn is confirmed before the loading screen goes: the CAPTCHA
    // title waits for it.
    input(&p, vec![moved(id, 0.5, 64.0, 0.5)]);
    let calls = take();
    assert!(teleports(&calls).is_empty() && calls.contains(&(id, Virtual::ShowMap(v::Hand::Main))), "{calls:?}");
    assert_eq!(titles(), 1);
    input(&p, vec![Input::Loaded(id)]);
    assert_eq!(titles(), 2);
}

#[test]
fn failed_bots_are_banned_through_pumbobans_or_blocked() {
    let p = start(Some("auto-ban:\n  enabled: true\n  failures: 1\n"));
    let calls = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    testing::provide(pumbo_common::bans::SERVICE, 1, 0, move |method, payload| {
        seen.borrow_mut().push((method.to_string(), String::from_utf8(payload).unwrap()));
        Ok(br#"{"ok":true,"id":1}"#.to_vec())
    });
    let id = joining("Bot", "en_us");
    testing::with(|h| h.players.get_mut(&id).unwrap().connection.address = "6.6.6.6".into());
    block_on(p.on_gate(info(id)));
    input(&p, vec![Input::Loaded(id), Input::Command((id, "captcha x".into()))]);
    // Not in the CAPTCHA yet: no kick. Fail the fall by timing out instead.
    input(&p, vec![moved(id, 0.5, 64.0, 0.5)]);
    let start = teleports(&take())[0];
    input(&p, (0..30).map(|i| moved(id, start.x, start.y - f64::from(i) * 0.01, start.z)).collect());
    input(&p, vec![Input::Chat((id, "x".into())), Input::Chat((id, "y".into()))]);
    assert!(kicked(id).is_some());
    let made = calls.borrow().clone();
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        made[0].1.contains("\"op\":\"punish\"") && made[0].1.contains("6.6.6.6") && made[0].1.contains("\"ip\":true")
    );
    // Blocked in memory as well.
    let e = PreLoginEvent {
        connection: Connection { address: "6.6.6.6".into(), ..info(id).connection },
        name: "Bot2".into(),
        claimed_uuid: None,
    };
    assert!(matches!(block_on(p.on_pre_login(e)), PreLoginReply::Deny(_)));
}

#[test]
fn attack_mode_is_published() {
    let p = start(Some("attack:\n  threshold: 3\n"));
    let id = joining("X", "en_us");
    let conn = info(id).connection;
    for i in 0..4 {
        let e = PreLoginEvent { connection: conn.clone(), name: format!("Bot{i}"), claimed_uuid: None };
        block_on(p.on_pre_login(e));
    }
    let published = testing::with(|h| h.published.clone());
    assert_eq!(published.len(), 1, "{published:?}");
    assert_eq!(published[0].0, "pumbo:attack-state@1.0");
    let state: AttackState = pumbo_sdk::service::from_cbor(&published[0].1).unwrap();
    assert!(state.active);
}

#[test]
fn admin_commands_from_the_console() {
    let p = start(None);
    let mut e = testing::command(0, "stats", &[]);
    e.player = None;
    block_on(p.on_command(e));
    let logs = testing::with(|h| h.logs.clone());
    assert!(logs.iter().any(|(_, l)| l.contains("Held now")), "{logs:?}");
    let names: Vec<String> = testing::with(|h| h.commands.iter().map(|c| c.name.clone()).collect());
    for n in ["stats", "verify", "unverify", "attack", "help"] {
        assert!(names.contains(&n.to_string()), "{names:?}");
    }
    assert!(!names.contains(&"reload".to_string()));
}
