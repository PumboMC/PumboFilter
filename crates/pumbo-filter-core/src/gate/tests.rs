use pumbo_common::lang::{COMMON, Lang};
use pumbo_common::store::Store;

use super::*;
use crate::config::CheckMode;
use crate::physics::ideal_reports;

const U: &str = "0b4f1c2a-0000-3000-8000-000000000001";
const T0: u64 = 1_000_000;

fn lang() -> Lang {
    Lang::load(&[COMMON, crate::LANG], "en", Some("prefix: \"F » \"\n")).0
}

fn gate_with(cfg: FilterSettings) -> Gate {
    Gate::new(cfg, lang(), Ok(Store::in_memory()))
}

fn gate() -> Gate {
    gate_with(FilterSettings::default())
}

fn ground() -> Place {
    Place {
        world: "world-0".into(),
        dimension: "minecraft:overworld".into(),
        pos: Pos { x: 12.5, y: 64.0, z: -7.5, yaw: 45.0, pitch: 10.0 },
    }
}

fn joining(place: Place) -> Joining {
    Joining {
        uuid: U.into(),
        name: "Steve".into(),
        ip: "1.2.3.4".into(),
        java: true,
        bypass: false,
        premium: false,
        hand_empty: true,
        place,
        locale: None,
        mode: None,
    }
}

fn has(fx: &[Effect], f: impl Fn(&Effect) -> bool) -> bool {
    fx.iter().any(f)
}

fn teleports(fx: &[Effect]) -> Vec<Pos> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Teleport(_, p) => Some(*p),
            _ => None,
        })
        .collect()
}

fn kicked(fx: &[Effect]) -> Option<String> {
    fx.iter().find_map(|e| match e {
        Effect::Kick(_, t) => Some(t.plain()),
        _ => None,
    })
}

fn said(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Say(_, t) => Some(t.plain()),
            _ => None,
        })
        .collect()
}

fn record(g: &Gate) -> Option<ReturnRecord> {
    Returns::new(g.store().unwrap()).get(U).unwrap()
}

/// Joins and confirms the spawn; returns the effects of the first movement.
fn join_and_spawn(g: &mut Gate) -> Vec<Effect> {
    let fx = g.join(joining(ground()), T0);
    assert!(teleports(&fx).is_empty(), "no teleport before the spawn is confirmed");
    g.moved(U, 12.5, 64.0, -7.5, T0 + 500)
}

/// Feeds a vanilla free fall from the start; returns every effect.
fn fall(g: &mut Gate, start_y: f64) -> Vec<Effect> {
    let mut fx = Vec::new();
    for (i, y) in ideal_reports(start_y, 200).into_iter().enumerate() {
        fx.extend(g.moved(U, 12.5, y, -7.5, T0 + 1000 + i as u64 * 50));
        if has(&fx, |e| matches!(e, Effect::Finish(_) | Effect::Kick(..) | Effect::ShowMap { .. })) {
            break;
        }
    }
    fx
}

#[test]
fn teleport_waits_for_spawn_move() {
    let mut g = gate();
    let fx = g.join(joining(ground()), T0);
    assert!(has(&fx, |e| matches!(e, Effect::Hold(_))));
    assert!(teleports(&fx).is_empty());
    assert_eq!(g.holding(U), Hold::Holding);
    assert!(record(&g).is_none(), "nothing stored before the teleport");
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 500);
    let t = teleports(&fx);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].x, t[0].y, t[0].z), (12.5, 1320.0, -7.5));
    // the way back is on disk before the teleport is sent
    assert_eq!(record(&g).unwrap().place, ground());
}

#[test]
fn real_client_passes_and_goes_back_to_its_place() {
    let mut g = gate();
    join_and_spawn(&mut g);
    let fx = fall(&mut g, 1320.0);
    let t = teleports(&fx);
    assert_eq!(t.last().map(|p| (p.x, p.y, p.z)), Some((12.5, 64.0, -7.5)));
    assert!(has(&fx, |e| matches!(e, Effect::ResetFall(_))));
    assert!(has(&fx, |e| matches!(e, Effect::Finish(_))));
    assert!(said(&fx).iter().any(|s| s.contains("Check passed")));
    assert_eq!(g.holding(U), Hold::Free);
    assert!(!g.is_held(U));
    // verified now: the next join is not held and nothing is moved
    g.leave(U);
    let fx = g.join(joining(ground()), T0 + 60_000);
    assert!(fx.iter().all(|e| matches!(e, Effect::Log(_))), "{fx:?}");
    assert_eq!(g.holding(U), Hold::Free);
    // the record of the first check is dropped by this join (normal height)
    assert!(record(&g).is_none());
}

#[test]
fn leave_mid_fall_puts_player_back() {
    let mut g = gate();
    join_and_spawn(&mut g);
    for (i, y) in ideal_reports(1320.0, 20).into_iter().enumerate() {
        g.moved(U, 12.5, y, -7.5, T0 + 1000 + i as u64 * 50);
    }
    let fx = g.leave(U);
    let t = teleports(&fx);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0], ground().pos);
    assert!(has(&fx, |e| matches!(e, Effect::ResetFall(_))));
    // the record stays until a join sees the player at a normal height
    assert!(record(&g).is_some());
    assert_eq!(g.holding(U), Hold::Unknown);
}

#[test]
fn recovery_runs_even_when_gate_is_skipped() {
    let mut g = gate();
    // a crash after an autosave in the air: record on disk, file in the air
    Returns::new(g.store().unwrap()).put(U, &ReturnRecord { place: ground(), at_ms: T0 }).unwrap();
    g.verify("Steve", "1.2.3.4", T0).unwrap();
    let mut sky = ground();
    sky.pos.y = 1250.0;
    let fx = g.join(joining(sky), T0 + 10);
    assert!(has(&fx, |e| matches!(e, Effect::Hold(_))));
    assert!(teleports(&fx).is_empty());
    assert!(g.is_held(U), "protected while in the air");
    let fx = g.moved(U, 12.5, 1249.9, -7.5, T0 + 600);
    assert_eq!(teleports(&fx), vec![ground().pos]);
    assert!(has(&fx, |e| matches!(e, Effect::ResetFall(_))));
    assert!(has(&fx, |e| matches!(e, Effect::Finish(_))));
    assert!(!g.is_held(U));
}

#[test]
fn recovery_before_a_check_waits_for_the_next_move() {
    let mut g = gate();
    Returns::new(g.store().unwrap()).put(U, &ReturnRecord { place: ground(), at_ms: T0 }).unwrap();
    let mut sky = ground();
    sky.pos.y = 900.0;
    g.join(joining(sky), T0);
    let fx = g.moved(U, 12.5, 899.0, -7.5, T0 + 500);
    assert_eq!(teleports(&fx), vec![ground().pos]);
    // the teleport back is not confirmed yet: no teleport up in the same step
    assert_eq!(teleports(&fx).len(), 1);
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 900);
    assert_eq!(teleports(&fx).len(), 1);
    assert_eq!(teleports(&fx)[0].y, 1320.0);
}

#[test]
fn stale_return_record_is_dropped() {
    let mut g = gate();
    Returns::new(g.store().unwrap()).put(U, &ReturnRecord { place: ground(), at_ms: T0 }).unwrap();
    g.verify("Steve", "1.2.3.4", T0).unwrap();
    let fx = g.join(joining(ground()), T0 + 5);
    assert!(!has(&fx, |e| matches!(e, Effect::Hold(_))));
    assert!(record(&g).is_none());
}

#[test]
fn no_teleport_without_return_record() {
    let cfg = FilterSettings::default();
    let mut g = Gate::new(cfg, lang(), Err(pumbo_common::store::StoreError::new("disk full")));
    g.join(joining(ground()), T0);
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 500);
    assert!(teleports(&fx).is_empty(), "never moved without the way back");
    assert!(has(&fx, |e| matches!(e, Effect::ShowMap { .. })), "{fx:?}");
}

#[test]
fn failed_fall_goes_back_and_shows_the_captcha() {
    let mut g = gate();
    join_and_spawn(&mut g);
    let mut fx = Vec::new();
    for i in 0..40 {
        fx.extend(g.moved(U, 12.5, 1320.0 - 0.5 - f64::from(i) * 0.1, -7.5, T0 + 1000 + i as u64 * 50));
    }
    assert_eq!(teleports(&fx).last().copied(), Some(ground().pos));
    let map = fx.iter().find_map(|e| match e {
        Effect::ShowMap { map_id, pixels, .. } => Some((*map_id, pixels.len())),
        _ => None,
    });
    assert_eq!(map.map(|m| m.1), Some(128 * 128));
    assert!(map.unwrap().0 >= 2_000_000_000);
    assert!(g.is_held(U));
}

fn captcha_answer(g: &Gate) -> String {
    match &g.players.get(U).unwrap().phase {
        Phase::Captcha { answer, .. } => answer.clone(),
        other => panic!("not in the CAPTCHA: {other:?}"),
    }
}

#[test]
fn captcha_answers() {
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { check_mode: CheckMode::OnlyCaptcha, ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg);
    let fx = join_and_spawn(&mut g);
    assert!(teleports(&fx).is_empty(), "CAPTCHA at the real place");
    // standing still (the client's answer to a teleport) is not answered with a teleport
    assert!(g.moved(U, 12.5, 64.0, -7.5, T0 + 600).is_empty());
    assert!(g.moved(U, 13.0, 64.0, -7.0, T0 + 650).is_empty());
    // walking away is
    assert_eq!(teleports(&g.moved(U, 14.0, 64.0, -7.5, T0 + 700)), vec![ground().pos]);
    let answer = captcha_answer(&g);
    let (cancel, fx) = g.chat(U, "zzzzz", T0 + 2000);
    assert!(cancel);
    assert!(said(&fx).iter().any(|s| s.contains("Wrong code. Attempts left: 1.")));
    let (cancel, fx) = g.command(U, &format!("captcha {}", answer.to_lowercase()), T0 + 2500);
    assert!(cancel);
    assert!(has(&fx, |e| matches!(e, Effect::TakeMap(_))));
    assert!(has(&fx, |e| matches!(e, Effect::Finish(_))));
    assert!(!g.is_held(U));
    // a free player's chat is not touched
    assert_eq!(g.chat(U, "hello", T0 + 3000), (false, Vec::new()));
}

#[test]
fn captcha_attempts_and_time_run_out() {
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { check_mode: CheckMode::OnlyCaptcha, ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg.clone());
    join_and_spawn(&mut g);
    g.chat(U, "zzzzz", T0 + 1000);
    let (_, fx) = g.chat(U, "yyyyy", T0 + 1100);
    assert!(kicked(&fx).unwrap().contains("Wrong code"));
    let mut g = gate_with(cfg);
    join_and_spawn(&mut g);
    let fx = g.tick(T0 + 500 + 31_000);
    assert!(kicked(&fx).unwrap().contains("Wrong code"));
}

#[test]
fn map_only_into_empty_hand() {
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { check_mode: CheckMode::OnlyCaptcha, ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg);
    let mut j = joining(ground());
    j.hand_empty = false;
    g.join(j, T0);
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 500);
    assert!(!has(&fx, |e| matches!(e, Effect::ShowMap { .. })));
    let title = fx.iter().find_map(|e| match e {
        Effect::Title { title, .. } => Some(title.plain()),
        _ => None,
    });
    assert_eq!(title, Some(captcha_answer(&g)));
    // nothing to take back at the end
    let (_, fx) = g.chat(U, &captcha_answer(&g), T0 + 900);
    assert!(!has(&fx, |e| matches!(e, Effect::TakeMap(_))));
}

#[test]
fn map_is_taken_back_on_leave() {
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { check_mode: CheckMode::OnlyCaptcha, ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg);
    join_and_spawn(&mut g);
    let fx = g.leave(U);
    assert!(has(&fx, |e| matches!(e, Effect::TakeMap(_))));
    assert!(teleports(&fx).is_empty(), "the CAPTCHA is at the real place");
}

#[test]
fn damage_protection_only_while_held() {
    let mut g = gate();
    assert!(!g.is_held(U), "unknown player");
    g.join(joining(ground()), T0);
    assert!(g.is_held(U));
    g.moved(U, 12.5, 64.0, -7.5, T0 + 500);
    assert!(g.is_held(U));
    fall(&mut g, 1320.0);
    assert!(!g.is_held(U), "free after passing");
    g.leave(U);
    assert!(!g.is_held(U));
    // skipped players are never protected
    let mut j = joining(ground());
    j.bypass = true;
    g.join(j, T0 + 10_000);
    assert!(!g.is_held(U));
    assert_eq!(g.holding(U), Hold::Free);
}

#[test]
fn second_connection_is_refused() {
    let mut g = gate();
    g.join(joining(ground()), T0);
    // the old connection is in the gate: it goes, the new one joins again later
    let (entry, fx) = g.pre_login("Steve", "1.2.3.4", Some(U), T0 + 100);
    assert!(matches!(entry, Entry::Refuse(t) if t.plain().contains("Join again")));
    assert!(kicked(&fx).unwrap().contains("another place"));
    // a free old connection keeps playing
    let mut g = gate();
    let (entry, fx) = g.pre_login("Steve", "1.2.3.4", Some(U), T0);
    assert!(matches!(entry, Entry::Refuse(t) if t.plain().contains("already on the server")));
    assert!(fx.is_empty());
    assert_eq!(g.pre_login("Alex", "1.2.3.5", None, T0).0, Entry::Allow);
}

#[test]
fn no_movement_after_spawn() {
    // GRAVITY_THEN_CAPTCHA: a player who never moves gets the CAPTCHA, unmoved
    let mut g = gate();
    g.join(joining(ground()), T0);
    let fx = g.tick(T0 + SPAWN_WAIT_MS);
    assert!(teleports(&fx).is_empty());
    assert!(has(&fx, |e| matches!(e, Effect::ShowMap { .. })));
    // ONLY_GRAVITY: kicked
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { check_mode: CheckMode::OnlyGravity, ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg);
    g.join(joining(ground()), T0);
    let fx = g.tick(T0 + SPAWN_WAIT_MS);
    assert!(kicked(&fx).unwrap().contains("took too long"));
}

#[test]
fn unconfirmed_teleport_up_is_never_followed_by_another() {
    let mut g = gate();
    join_and_spawn(&mut g);
    // the client never confirms the teleport up: no movement arrives
    let fx = g.tick(T0 + 500 + 228 * 50);
    assert!(teleports(&fx).is_empty(), "{fx:?}");
    assert!(kicked(&fx).is_some());
    // the leave after the kick puts the player back
    assert_eq!(teleports(&g.leave(U)), vec![ground().pos]);
}

#[test]
fn confirmed_but_too_slow_goes_back_to_the_captcha() {
    let mut g = gate();
    join_and_spawn(&mut g);
    for (i, y) in ideal_reports(1320.0, 10).into_iter().enumerate() {
        g.moved(U, 12.5, y, -7.5, T0 + 1000 + i as u64 * 50);
    }
    let fx = g.tick(T0 + 500 + 228 * 50);
    assert_eq!(teleports(&fx), vec![ground().pos]);
    assert!(has(&fx, |e| matches!(e, Effect::ShowMap { .. })));
}

#[test]
fn late_ground_movement_is_ignored() {
    let mut g = gate();
    join_and_spawn(&mut g);
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 600);
    assert!(fx.is_empty());
    let fx = fall(&mut g, 1320.0);
    assert!(has(&fx, |e| matches!(e, Effect::Finish(_))), "{fx:?}");
}

#[test]
fn held_players_cannot_chat_or_use_commands() {
    let mut g = gate();
    g.join(joining(ground()), T0);
    let (cancel, fx) = g.chat(U, "hi", T0);
    assert!(cancel && said(&fx)[0].contains("Finish the check first"));
    let (cancel, fx) = g.command(U, "login secret", T0);
    assert!(cancel && said(&fx)[0].contains("Finish the check first"));
    let (cancel, fx) = g.command(U, "captcha abc", T0);
    assert!(cancel && said(&fx)[0].contains("Finish the check first"));
}

#[test]
fn unload_disconnects_and_keeps_the_way_back() {
    let mut g = gate();
    join_and_spawn(&mut g);
    let fx = g.unload();
    assert!(teleports(&fx).is_empty(), "no teleports while the plugin is unloaded");
    assert!(kicked(&fx).is_none(), "no kicks either");
    assert!(has(&fx, |e| matches!(e, Effect::Hover(_))));
    assert!(has(&fx, |e| matches!(e, Effect::ResetFall(_))));
    assert!(has(&fx, |e| matches!(e, Effect::Unhold(_))));
    assert!(said(&fx)[0].contains("join the server again"));
    // the next join (plugin loaded again) puts the player back
    assert!(record(&g).is_some());
    let mut g2 = Gate::new(FilterSettings::default(), lang(), Ok(Store::in_memory()));
    Returns::new(g2.store().unwrap()).put(U, &record(&g).unwrap()).unwrap();
    let mut sky = ground();
    sky.pos.y = 1300.0;
    g2.join(joining(sky), T0 + 5000);
    assert_eq!(teleports(&g2.moved(U, 12.5, 1299.0, -7.5, T0 + 5500)), vec![ground().pos]);
}

#[test]
fn client_check() {
    let mut g = gate();
    let fx = g.join(joining(ground()), T0);
    assert!(!has(&fx, |e| matches!(e, Effect::QueryClient(_))));
    let fx = g.tick(T0 + 60 * 50);
    assert!(has(&fx, |e| matches!(e, Effect::QueryClient(_))));
    assert!(g.client_info(U, "\u{7}vanilla", true).is_empty());
    let fx = g.client_info(U, "", true);
    assert!(kicked(&fx).unwrap().contains("not supported"));
    assert!(has(&fx, |e| matches!(e, Effect::Log(l) if l.contains("no brand"))));
}

#[test]
fn skips() {
    let mut g = gate();
    for (f, why) in [
        (Box::new(|j: &mut Joining| j.java = false) as Box<dyn Fn(&mut Joining)>, "bedrock"),
        (Box::new(|j: &mut Joining| j.bypass = true), "bypass"),
        (Box::new(|j: &mut Joining| j.premium = true), "premium"),
    ] {
        let mut j = joining(ground());
        f(&mut j);
        let fx = g.join(j, T0);
        assert!(!has(&fx, |e| matches!(e, Effect::Hold(_))), "{why}");
        g.leave(U);
    }
    // premium players are checked during an attack
    g.attack.forced = crate::attack::Forced::On;
    let mut j = joining(ground());
    j.premium = true;
    assert!(has(&g.join(j, T0), |e| matches!(e, Effect::Hold(_))));
    let cfg = FilterSettings {
        filter: crate::config::FilterCfg { whitelist: vec!["steve@1.2.3.4".into()], ..Default::default() },
        ..Default::default()
    };
    let mut g = gate_with(cfg);
    assert!(!has(&g.join(joining(ground()), T0), |e| matches!(e, Effect::Hold(_))));
}

#[test]
fn reconnect_gate_during_a_flood() {
    let mut g = gate();
    let cfg = g.cfg.attack.clone();
    for i in 0..cfg.reconnect_threshold {
        g.pre_login(&format!("Bot{i}"), "9.9.9.9", None, T0);
    }
    let (entry, _) = g.pre_login("Steve", "1.2.3.4", None, T0 + 10);
    assert!(matches!(entry, Entry::Refuse(t) if t.plain().contains("join again")));
    let (entry, _) = g.pre_login("Steve", "1.2.3.4", None, T0 + 3000);
    assert_eq!(entry, Entry::Allow);
}

#[test]
fn messages_are_complete() {
    assert_eq!(pumbo_common::lang::check_bundle(&crate::LANG), Vec::<String>::new());
}

#[test]
fn limbo_falls_without_a_way_back_on_disk() {
    let mut g = Gate::new(FilterSettings::default(), lang(), Err(pumbo_common::store::StoreError::new("none")));
    g.limbo = true;
    let fx = g.join(joining(ground()), T0);
    assert!(has(&fx, |e| matches!(e, Effect::Hold(_))));
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 100);
    let up = teleports(&fx);
    assert_eq!(up.len(), 1, "{fx:?}");
    assert!(up[0].y > 1000.0);
    let mut g = gate();
    g.limbo = true;
    g.join(joining(ground()), T0);
    g.moved(U, 12.5, 64.0, -7.5, T0 + 100);
    assert_eq!(Returns::new(g.store().unwrap()).get(U).unwrap(), None);
}

#[test]
fn platform_mode_wins_over_the_config() {
    let mut g = gate();
    let j = Joining { mode: Some(CheckMode::OnlyCaptcha), ..joining(ground()) };
    g.join(j, T0);
    let fx = g.moved(U, 12.5, 64.0, -7.5, T0 + 100);
    assert!(teleports(&fx).is_empty(), "{fx:?}");
    assert!(has(&fx, |e| matches!(e, Effect::ShowMap { .. })));
}

#[test]
fn messages_in_the_language_of_the_client() {
    let mut g = gate();
    g.others = vec![Lang::load(&[COMMON, crate::LANG], "pl", Some("prefix: \"F » \"\n")).0];
    let fx = g.join(Joining { locale: Some("pl_pl".into()), ..joining(ground()) }, T0);
    let said = |fx: &[Effect]| {
        fx.iter().find_map(|e| match e {
            Effect::Say(_, t) => Some(t.plain()),
            _ => None,
        })
    };
    assert!(said(&fx).unwrap().contains("Sprawdzamy"), "{fx:?}");
    let (_, fx) = g.chat(U, "hi", T0 + 10);
    assert!(said(&fx).unwrap().contains("Najpierw"), "{fx:?}");
    assert_eq!(g.lang.code(), "en");
    let other = "0b4f1c2a-0000-3000-8000-000000000002";
    let fx = g.join(Joining { uuid: other.into(), name: "Alex".into(), ..joining(ground()) }, T0);
    assert!(said(&fx).unwrap().contains("Checking"), "{fx:?}");
}
