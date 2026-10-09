//! The gate of one server: who is checked, the state of every player, and the
//! effects the platform layer carries out.
//!
//! The gate never moves a player to another world. A checked player is held
//! (dark screen on the client only, hidden from others, protected, every action
//! cancelled) and falls high above their own place ([`crate::place`]); a
//! CAPTCHA is answered at the real place, where a player who walks away is put
//! back ([`ANCHOR_RADIUS`]). Teleports are only sent once the previous one is
//! confirmed, which is when the server reports movement: the first movement
//! after the spawn, and a movement at the new height after a teleport up.
//!
//! Every call takes the time (`now` in Unix ms) and returns [`Effect`]s; nothing
//! here talks to a host.

use std::collections::HashMap;
use std::sync::Arc;

use pumbo_common::gate::Hold;
use pumbo_common::id::{ip_key_str, name_key};
use pumbo_common::lang::Lang;
use pumbo_common::random::random_u64;
use pumbo_common::rich::{Line, Segment, Text};
use pumbo_common::store::Store;
use pumbo_common::style;
use pumbo_common::text::{Args, Color, Named, Style};

use crate::attack::{AttackLevel, AttackMonitor, Checks, ReconnectGate, checks_for};
use crate::captcha::{self, Pool};
use crate::client::{self, clean_brand};
use crate::config::{CaptchaMode, CheckMode, FilterSettings};
use crate::physics::{FallCheck, FallStatus};
use crate::place::{self, Place, Pos, Recovery, ReturnRecord, Returns};
use crate::verified::VerifiedCache;

/// A player who did not move this long after joining (a bot, or a player in a
/// vehicle) does not start falling.
pub const SPAWN_WAIT_MS: u64 = 10_000;
/// Map ids for CAPTCHA images, far above the ids of real maps.
const MAP_ID_BASE: i32 = 2_000_000_000;
const MAP_RESENDS_MS: [u64; 2] = [1000, 3000];
const PROGRESS_EVERY_MS: u64 = 500;
const PURGE_EVERY_MS: u64 = 3_600_000;
/// A player who stands somewhere (CAPTCHA) and moves farther than this is put
/// back. Not Pumpkin's movement lock: that answers every movement with a
/// teleport, and the client answers every teleport with a movement (a loop).
pub const ANCHOR_RADIUS: f64 = 1.0;

/// What the layer knows about a player when they join.
#[derive(Debug, Clone, PartialEq)]
pub struct Joining {
    pub uuid: String,
    pub name: String,
    /// Address without the port.
    pub ip: String,
    /// A Java client (Bedrock players skip the checks).
    pub java: bool,
    /// Has the bypass permission.
    pub bypass: bool,
    /// Authenticated by Mojang ([`pumbo_common::id::is_authenticated_profile`]).
    pub premium: bool,
    /// Nothing in the main hand (a CAPTCHA map only ever goes into an empty hand).
    pub hand_empty: bool,
    /// Where the server put the player.
    pub place: Place,
    /// Client locale (`pl_pl`): the player's messages use that language when
    /// the gate has it ([`Gate::others`]).
    pub locale: Option<String>,
    /// Checks the platform asks for instead of the configured mode (clients
    /// through ViaProxy on PumboProx).
    pub mode: Option<CheckMode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Dark screen on the client, hidden from other players.
    Hold(String),
    /// Teleport within the player's own world.
    Teleport(String, Pos),
    ResetFall(String),
    Say(String, Text),
    Title {
        uuid: String,
        title: Text,
        subtitle: Text,
        stay_ticks: i32,
    },
    ActionBar(String, Text),
    /// Put a CAPTCHA map into the (empty) main hand and send its pixels.
    ShowMap {
        uuid: String,
        map_id: i32,
        pixels: Arc<Vec<u8>>,
    },
    /// Send the pixels of a map again.
    SendMap {
        uuid: String,
        map_id: i32,
        pixels: Arc<Vec<u8>>,
    },
    /// Take the CAPTCHA map out of the hand again.
    TakeMap(String),
    /// Ask the client for its brand and settings ([`Gate::client_info`]).
    QueryClient(String),
    Kick(String, Text),
    /// The gate is done with the player: release them unless the other gate
    /// plugin still holds them.
    Finish(String),
    /// Release at once (the plugin is being unloaded).
    Unhold(String),
    /// Stop the fall on the client only (no gravity), while the plugin is
    /// unloaded and cannot put the player back.
    Hover(String),
    Log(String),
}

/// Answer to a connection before the player is in the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Allow,
    Refuse(Text),
}

#[derive(Debug, Clone)]
enum Phase {
    /// Waiting for the first movement after the spawn. `put_back`: the player
    /// spawned in the air above their place and goes back first.
    Spawning {
        since: u64,
        put_back: bool,
    },
    Falling {
        check: Box<FallCheck>,
        since: u64,
    },
    Captcha {
        answer: String,
        attempts_left: u32,
        until: u64,
        map: Option<(i32, Arc<Vec<u8>>)>,
        resend: Vec<u64>,
    },
    Free,
}

#[derive(Debug, Clone)]
struct Player {
    name: String,
    ip: String,
    real: Place,
    checks: Checks,
    phase: Phase,
    hand_empty: bool,
    map_given: bool,
    client_due: Option<u64>,
    progress_at: u64,
    /// Index into [`Gate::others`].
    lang: Option<usize>,
}

impl Player {
    fn checked(&self) -> bool {
        self.checks.gravity || self.checks.captcha || self.checks.captcha_on_fail
    }

    fn in_air(&self) -> bool {
        matches!(self.phase, Phase::Falling { .. } | Phase::Spawning { put_back: true, .. })
    }
}

/// Counters for `/pumbofilter stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub checked: u64,
    pub passed: u64,
    pub failed: u64,
    pub put_back: u64,
}

pub struct Gate {
    pub cfg: FilterSettings,
    pub lang: Lang,
    /// Other languages, for players whose client uses one of them (empty:
    /// everyone gets `lang`).
    pub others: Vec<Lang>,
    /// The platform has a virtual limbo (PumboProx): the fall happens there,
    /// so no way back is stored and the gravity check needs no database.
    pub limbo: bool,
    store: Option<Store>,
    /// Why the database is not available (checks run without the cache and
    /// without the gravity check, which needs return records).
    pub store_error: Option<String>,
    pub attack: AttackMonitor,
    reconnect: ReconnectGate,
    pool: Pool,
    players: HashMap<String, Player>,
    next_map: u32,
    last_attack_log: u64,
    last_purge: u64,
    pub stats: Stats,
}

fn key(uuid: &str) -> String {
    uuid.trim().to_lowercase()
}

impl Gate {
    pub fn new(cfg: FilterSettings, lang: Lang, store: pumbo_common::store::Result<Store>) -> Self {
        let (store, store_error) = match store {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let pool = Pool::new(cfg.captcha.pool_size as usize, cfg.captcha.style(), random_u64());
        Self {
            cfg,
            lang,
            others: Vec::new(),
            limbo: false,
            store,
            store_error,
            attack: AttackMonitor::new(),
            reconnect: ReconnectGate::default(),
            pool,
            players: HashMap::new(),
            next_map: 0,
            last_attack_log: 0,
            last_purge: 0,
            stats: Stats::default(),
        }
    }

    /// New settings and messages (`reload`); players keep their state.
    pub fn reload(&mut self, cfg: FilterSettings, lang: Lang) {
        if cfg.captcha.style() != self.cfg.captcha.style() || cfg.captcha.pool_size != self.cfg.captcha.pool_size {
            self.pool.set_style(cfg.captcha.pool_size as usize, cfg.captcha.style());
            self.pool.start_refresh();
        }
        self.cfg = cfg;
        self.lang = lang;
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    /// Runs `f` with `lang` set to the language of the player (swapped in and
    /// back out, so the messages below keep using `self.lang`).
    fn as_player<R>(&mut self, lang: Option<usize>, f: impl FnOnce(&mut Self) -> R) -> R {
        let Some(i) = lang.filter(|i| *i < self.others.len()) else { return f(self) };
        self.swap_lang(i);
        let r = f(self);
        self.swap_lang(i);
        r
    }

    fn swap_lang(&mut self, i: usize) {
        if let Some(other) = self.others.get_mut(i) {
            std::mem::swap(&mut self.lang, other);
        }
    }

    fn lang_of(&self, uuid: &str) -> Option<usize> {
        self.players.get(uuid).and_then(|p| p.lang)
    }

    /// Players already online when the plugin loads: they are free.
    pub fn adopt(&mut self, uuid: &str, name: &str, ip: &str, place: Place) {
        let p = Player {
            name: name.to_string(),
            ip: ip.to_string(),
            real: place,
            checks: Checks { gravity: false, captcha: false, captcha_on_fail: false },
            phase: Phase::Free,
            hand_empty: false,
            map_given: false,
            client_due: None,
            progress_at: 0,
            lang: None,
        };
        self.players.insert(key(uuid), p);
    }

    /// What this plugin knows about a player (the `holding` answer).
    pub fn holding(&self, uuid: &str) -> Hold {
        match self.players.get(&key(uuid)) {
            None => Hold::Unknown,
            Some(p) if matches!(p.phase, Phase::Free) => Hold::Free,
            Some(_) => Hold::Holding,
        }
    }

    /// Whether the gate holds the player: damage and actions are cancelled
    /// only for them.
    pub fn is_held(&self, uuid: &str) -> bool {
        self.holding(uuid) == Hold::Holding
    }

    /// Whether the player is at the CAPTCHA.
    pub fn in_captcha(&self, uuid: &str) -> bool {
        self.players.get(&key(uuid)).is_some_and(|p| matches!(p.phase, Phase::Captcha { .. }))
    }

    /// Players the gate holds right now.
    pub fn held(&self) -> Vec<String> {
        self.players.iter().filter(|(_, p)| !matches!(p.phase, Phase::Free)).map(|(u, _)| u.clone()).collect()
    }

    pub fn level(&self, now: u64) -> AttackLevel {
        self.attack.level(now, &self.cfg.attack)
    }

    fn verified(&self, name: &str, ip: &str, now: u64) -> bool {
        self.cfg.filter.verified_cache
            && self.store.as_ref().is_some_and(|s| VerifiedCache::new(s).is_verified(name, ip, now).unwrap_or(false))
    }

    fn kick_text(&self, key: &str) -> Text {
        Text::parse(&self.lang.get(key))
    }

    /// A connection before it enters the world. `online`: the UUID of a player
    /// with the same UUID or nickname who is already on the server.
    pub fn pre_login(&mut self, name: &str, ip: &str, online: Option<&str>, now: u64) -> (Entry, Vec<Effect>) {
        let level = self.attack.on_connection(now, &self.cfg.attack);
        if let Some(old) = online {
            // One player never plays on two connections: two saves of one data
            // file could undo each other or copy items.
            if self.is_held(old) {
                let fx = vec![Effect::Kick(key(old), self.kick_text("kick-other-connection"))];
                return (Entry::Refuse(self.kick_text("kick-join-again")), fx);
            }
            return (Entry::Refuse(self.kick_text("kick-already-online")), Vec::new());
        }
        if level == AttackLevel::Reconnect && !self.cfg.filter.is_whitelisted(name, ip) && !self.verified(name, ip, now)
        {
            let gate_key = format!("{}|{}", name_key(name), ip_key_str(ip));
            let a = &self.cfg.attack;
            if !self.reconnect.check(&gate_key, now, a.reconnect_min_seconds, a.reconnect_max_seconds) {
                self.attack.record_block();
                return (Entry::Refuse(self.kick_text("kick-reconnect")), Vec::new());
            }
        }
        (Entry::Allow, Vec::new())
    }

    /// Why a player skips the checks, if they do.
    fn skip_reason(&self, j: &Joining, now: u64) -> Option<&'static str> {
        let level = self.level(now);
        if !j.java {
            Some("Bedrock client")
        } else if j.bypass {
            Some("bypass permission")
        } else if self.cfg.filter.is_whitelisted(&j.name, &j.ip) {
            Some("whitelist")
        } else if j.premium && self.cfg.filter.skip_premium && level == AttackLevel::Normal {
            Some("premium")
        } else if level == AttackLevel::Normal && self.verified(&j.name, &j.ip, now) {
            Some("verified recently")
        } else {
            None
        }
    }

    /// A player joins (the spawn teleport is not confirmed yet: no teleport here).
    pub fn join(&mut self, j: Joining, now: u64) -> Vec<Effect> {
        let lang = pumbo_common::lang::index_for_locale(&self.others, j.locale.as_deref());
        self.as_player(lang, |g| g.join_as(j, lang, now))
    }

    fn join_as(&mut self, j: Joining, lang: Option<usize>, now: u64) -> Vec<Effect> {
        let uuid = key(&j.uuid);
        let mut fx = Vec::new();
        let stored = if self.limbo { None } else { self.store.as_ref().map(|s| Returns::new(s).get(&uuid)) };
        let record = match stored {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                fx.push(Effect::Log(format!("cannot read the return record of {}: {e}", j.name)));
                None
            }
            None => None,
        };
        let recovery = place::recovery(record.as_ref(), &j.place);
        if recovery == Recovery::Stale
            && let Some(s) = &self.store
            && let Err(e) = Returns::new(s).remove(&uuid)
        {
            fx.push(Effect::Log(format!("cannot remove the return record of {}: {e}", j.name)));
        }
        let (real, put_back) = match recovery {
            Recovery::PutBack(p) => (p, true),
            _ => (j.place.clone(), false),
        };
        if put_back {
            fx.push(Effect::Log(format!("{} joined in the air above their place, putting them back", j.name)));
            self.stats.put_back += 1;
        }
        let skip = self.skip_reason(&j, now);
        let mut checks = match skip {
            Some(_) => Checks { gravity: false, captcha: false, captcha_on_fail: false },
            None => {
                let mode = j.mode.unwrap_or_else(|| self.cfg.filter.mode_for(self.level(now)));
                checks_for(mode, self.cfg.gravity.enabled, self.cfg.captcha.enabled)
            }
        };
        if checks.gravity && self.store.is_none() && !self.limbo {
            // Without the database there is no way back after a crash: no fall.
            checks = Checks {
                gravity: false,
                captcha: self.cfg.captcha.enabled && (checks.captcha || checks.captcha_on_fail),
                captcha_on_fail: false,
            };
        }
        if let (Some(why), true) = (skip, self.cfg.gravity.debug) {
            fx.push(Effect::Log(format!("{} skips the checks: {why}", j.name)));
        }
        let checked = checks.gravity || checks.captcha;
        let mut p = Player {
            name: j.name.clone(),
            ip: j.ip.clone(),
            real,
            checks,
            phase: Phase::Free,
            hand_empty: j.hand_empty,
            map_given: false,
            client_due: None,
            progress_at: 0,
            lang,
        };
        if !checked && !put_back {
            self.players.insert(uuid, p);
            return fx;
        }
        p.phase = Phase::Spawning { since: now, put_back };
        fx.push(Effect::Hold(uuid.clone()));
        if checked {
            self.stats.checked += 1;
            if self.cfg.client_check.enabled {
                p.client_due = Some(now + u64::from(self.cfg.client_check.check_ticks) * 50);
            }
            fx.push(Effect::Say(uuid.clone(), style::info(&self.lang, "check-start", &Args::new())));
            // The whole check, with time to appear in the void; `pass` clears it.
            let stay = i32::try_from(self.cfg.gravity.falling_check_ticks).unwrap_or(i32::MAX).saturating_add(60);
            fx.push(self.title(&uuid, "check-title", "check-subtitle", stay));
        }
        self.players.insert(uuid, p);
        fx
    }

    fn title(&self, uuid: &str, title: &str, subtitle: &str, stay_ticks: i32) -> Effect {
        Effect::Title {
            uuid: uuid.to_string(),
            title: Text::parse(&self.lang.get(title)),
            subtitle: Text::parse(&self.lang.get(subtitle)),
            stay_ticks,
        }
    }

    /// The server reported a movement of the player (after its own handling).
    pub fn moved(&mut self, uuid: &str, x: f64, y: f64, z: f64, now: u64) -> Vec<Effect> {
        let uuid = key(uuid);
        self.as_player(self.lang_of(&uuid), |g| g.moved_as(&uuid, x, y, z, now))
    }

    fn moved_as(&mut self, uuid: &str, x: f64, y: f64, z: f64, now: u64) -> Vec<Effect> {
        enum Next {
            Nothing,
            PutBack { done: bool },
            Start,
            Passed,
            Failed,
            Progress(u32),
            Anchor,
        }
        let uuid = key(uuid);
        let debug = self.cfg.gravity.debug;
        let Some(p) = self.players.get_mut(&uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        let next = match &mut p.phase {
            Phase::Free => Next::Nothing,
            Phase::Captcha { .. } => {
                let r = p.real.pos;
                let far = (x - r.x).powi(2) + (y - r.y).powi(2) + (z - r.z).powi(2) > ANCHOR_RADIUS * ANCHOR_RADIUS;
                if far { Next::Anchor } else { Next::Nothing }
            }
            Phase::Spawning { put_back: true, .. } => {
                // The spawn teleport is confirmed: back to the real place first.
                p.phase = Phase::Spawning { since: now, put_back: false };
                Next::PutBack { done: !p.checked() }
            }
            Phase::Spawning { put_back: false, .. } => Next::Start,
            Phase::Falling { check, .. } => {
                if !check.armed() && y < place::air_floor(&p.real.dimension) {
                    // A movement from before the teleport up, reported late.
                    return fx;
                }
                let status = check.on_move(x, y, z);
                if debug && matches!(status, FallStatus::Passed | FallStatus::FailedY | FallStatus::FailedXz) {
                    fx.push(Effect::Log(format!("gravity check of {}: {}", p.name, check.summary())));
                }
                match status {
                    FallStatus::Passed => Next::Passed,
                    FallStatus::FailedY | FallStatus::FailedXz => Next::Failed,
                    FallStatus::Waiting | FallStatus::InProgress if now >= p.progress_at => {
                        p.progress_at = now + PROGRESS_EVERY_MS;
                        Next::Progress((check.progress() * 100.0).round() as u32)
                    }
                    FallStatus::Waiting | FallStatus::InProgress => Next::Nothing,
                }
            }
        };
        let real = p.real.pos;
        match next {
            Next::Nothing => {}
            Next::PutBack { done } => {
                fx.push(Effect::Teleport(uuid.clone(), real));
                fx.push(Effect::ResetFall(uuid.clone()));
                if done {
                    fx.extend(self.finish(&uuid));
                }
            }
            Next::Start => fx.extend(self.start(&uuid, now)),
            Next::Passed => {
                fx.push(Effect::Teleport(uuid.clone(), real));
                fx.push(Effect::ResetFall(uuid.clone()));
                fx.extend(self.pass(&uuid, now));
            }
            Next::Failed => fx.extend(self.fall_failed(&uuid, now, true)),
            Next::Anchor => {
                fx.push(Effect::Teleport(uuid.clone(), real));
                fx.push(Effect::ResetFall(uuid.clone()));
            }
            Next::Progress(pct) => {
                let text = style::info(&self.lang, "check-progress", &Args::new().arg(pct));
                fx.push(Effect::ActionBar(uuid.clone(), text));
            }
        }
        fx
    }

    /// Starts the first check (the player's spawn is confirmed).
    fn start(&mut self, uuid: &str, now: u64) -> Vec<Effect> {
        let Some(p) = self.players.get(uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        if p.checks.gravity {
            let record = ReturnRecord { place: p.real.clone(), at_ms: now };
            let saved = match &self.store {
                _ if self.limbo => Ok(()),
                Some(s) => Returns::new(s).put(uuid, &record).map_err(|e| e.to_string()),
                None => Err("the database is not available".to_string()),
            };
            match saved {
                Ok(()) => {
                    let start = place::fall_start(&p.real);
                    let check = FallCheck::new((start.x, start.y, start.z), self.cfg.gravity.fall_params())
                        .with_floor(place::air_floor(&p.real.dimension));
                    fx.push(Effect::Teleport(uuid.to_string(), start));
                    if let Some(p) = self.players.get_mut(uuid) {
                        p.phase = Phase::Falling { check: Box::new(check), since: now };
                    }
                    return fx;
                }
                Err(e) => {
                    // Never move a player without the way back on disk.
                    fx.push(Effect::Log(format!(
                        "cannot store the return record of {}, no gravity check: {e}",
                        p.name
                    )));
                    if self.cfg.captcha.enabled {
                        fx.extend(self.start_captcha(uuid, now));
                    } else {
                        fx.extend(self.pass(uuid, now));
                    }
                    return fx;
                }
            }
        }
        if p.checks.captcha {
            return self.start_captcha(uuid, now);
        }
        self.pass(uuid, now)
    }

    /// The gravity check failed (or timed out). `confirmed`: the player is in
    /// the air and their last teleport is confirmed, so they can be moved.
    fn fall_failed(&mut self, uuid: &str, now: u64, confirmed: bool) -> Vec<Effect> {
        let Some(p) = self.players.get(uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        if confirmed && p.checks.captcha_on_fail {
            fx.push(Effect::Teleport(uuid.to_string(), p.real.pos));
            fx.push(Effect::ResetFall(uuid.to_string()));
            fx.extend(self.start_captcha(uuid, now));
            return fx;
        }
        // Kicked in the air: the leave handler puts the player back before the
        // server saves them.
        self.stats.failed += 1;
        fx.push(Effect::Kick(uuid.to_string(), self.kick_text("kick-falling-failed")));
        fx
    }

    fn start_captcha(&mut self, uuid: &str, now: u64) -> Vec<Effect> {
        let code = self.pool.pick(random_u64()).unwrap_or_else(|| {
            // An empty pool (just started): draw one now.
            Arc::new(captcha::generate(random_u64(), &self.cfg.captcha.style()))
        });
        self.next_map = self.next_map.wrapping_add(1);
        let map_id = MAP_ID_BASE.saturating_add(i32::try_from(self.next_map % 100_000).unwrap_or(0));
        let attempts = self.cfg.captcha.attempts;
        let until = now + u64::from(self.cfg.captcha.timeout_seconds) * 1000;
        let lang = &self.lang;
        let Some(p) = self.players.get_mut(uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        let stay = i32::try_from(self.cfg.captcha.timeout_seconds.saturating_mul(20)).unwrap_or(600);
        let as_map = self.cfg.captcha.mode == CaptchaMode::Map && p.hand_empty;
        let pixels = Arc::new(code.pixels.clone());
        if as_map {
            p.map_given = true;
            fx.push(Effect::ShowMap { uuid: uuid.to_string(), map_id, pixels: pixels.clone() });
            fx.push(Effect::Say(uuid.to_string(), style::info(lang, "captcha-start", &Args::new().arg(attempts))));
            fx.push(Effect::Title {
                uuid: uuid.to_string(),
                title: Text::parse(&lang.get("captcha-title")),
                subtitle: Text::parse(&lang.get("captcha-subtitle")),
                stay_ticks: 100,
            });
        } else {
            fx.push(Effect::Say(
                uuid.to_string(),
                style::info(lang, "captcha-start-title", &Args::new().arg(attempts)),
            ));
            fx.push(Effect::Title {
                uuid: uuid.to_string(),
                title: colored_code(&code.answer, random_u64()),
                subtitle: Text::parse(&lang.get("captcha-subtitle-title")),
                stay_ticks: stay,
            });
        }
        p.phase = Phase::Captcha {
            answer: code.answer.clone(),
            attempts_left: attempts,
            until,
            map: as_map.then_some((map_id, pixels)),
            resend: if as_map { MAP_RESENDS_MS.iter().map(|d| now + d).collect() } else { Vec::new() },
        };
        fx
    }

    /// An answer to the CAPTCHA (chat or `/captcha`).
    fn answer(&mut self, uuid: &str, input: &str, now: u64) -> Vec<Effect> {
        let ignore_case = self.cfg.captcha.ignore_case;
        let Some(p) = self.players.get_mut(uuid) else { return Vec::new() };
        let Phase::Captcha { answer, attempts_left, .. } = &mut p.phase else { return Vec::new() };
        if captcha::matches(answer, input, ignore_case) {
            let mut fx = Vec::new();
            if p.map_given {
                p.map_given = false;
                fx.push(Effect::TakeMap(uuid.to_string()));
            }
            fx.extend(self.pass(uuid, now));
            return fx;
        }
        *attempts_left = attempts_left.saturating_sub(1);
        if *attempts_left == 0 {
            self.stats.failed += 1;
            return vec![Effect::Kick(uuid.to_string(), self.kick_text("kick-captcha-failed"))];
        }
        let left = *attempts_left;
        vec![Effect::Say(uuid.to_string(), style::error(&self.lang, "captcha-wrong", &Args::new().arg(left)))]
    }

    /// The player passed every check.
    fn pass(&mut self, uuid: &str, now: u64) -> Vec<Effect> {
        let ttl = self.cfg.filter.verified_ttl_ms();
        let Some(p) = self.players.get(uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        if p.checked() {
            self.stats.passed += 1;
            if self.cfg.filter.verified_cache
                && let Some(s) = &self.store
                && let Err(e) = VerifiedCache::new(s).add(&p.name, &p.ip, now + ttl)
            {
                fx.push(Effect::Log(format!("cannot remember {} as verified: {e}", p.name)));
            }
            fx.push(Effect::Say(uuid.to_string(), style::success(&self.lang, "check-passed", &Args::new())));
            fx.push(Effect::ActionBar(uuid.to_string(), Text::default()));
            fx.push(Effect::Title {
                uuid: uuid.to_string(),
                title: Text::default(),
                subtitle: Text::default(),
                stay_ticks: 0,
            });
        }
        fx.extend(self.finish(uuid));
        fx
    }

    /// Marks the player free first, then tells the layer (release rule).
    fn finish(&mut self, uuid: &str) -> Vec<Effect> {
        let Some(p) = self.players.get_mut(uuid) else { return Vec::new() };
        p.phase = Phase::Free;
        p.client_due = None;
        vec![Effect::Finish(uuid.to_string())]
    }

    /// Chat from a player; `true` = cancel it.
    pub fn chat(&mut self, uuid: &str, message: &str, now: u64) -> (bool, Vec<Effect>) {
        let uuid = key(uuid);
        self.as_player(self.lang_of(&uuid), |g| g.chat_as(uuid, message, now))
    }

    fn chat_as(&mut self, uuid: String, message: &str, now: u64) -> (bool, Vec<Effect>) {
        match self.players.get(&uuid).map(|p| &p.phase) {
            None | Some(Phase::Free) => (false, Vec::new()),
            Some(Phase::Captcha { .. }) => (true, self.answer(&uuid, message, now)),
            Some(_) => (true, vec![Effect::Say(uuid.clone(), style::error(&self.lang, "check-wait", &Args::new()))]),
        }
    }

    /// A command from a player (without the slash); `true` = cancel it. Held
    /// players can only use `/captcha`.
    pub fn command(&mut self, uuid: &str, line: &str, now: u64) -> (bool, Vec<Effect>) {
        let uuid = key(uuid);
        self.as_player(self.lang_of(&uuid), |g| g.command_as(uuid, line, now))
    }

    fn command_as(&mut self, uuid: String, line: &str, now: u64) -> (bool, Vec<Effect>) {
        let (name, args) = pumbo_common::command::split_command(line);
        let held = self.is_held(&uuid);
        if name == "captcha" {
            let in_captcha = matches!(self.players.get(&uuid).map(|p| &p.phase), Some(Phase::Captcha { .. }));
            if in_captcha {
                if args.is_empty() {
                    return (true, vec![Effect::Say(uuid, style::usage(&self.lang, "/captcha <code>"))]);
                }
                return (true, self.answer(&uuid, &args.join(""), now));
            }
            let key = if held { "check-wait" } else { "captcha-none" };
            return (true, vec![Effect::Say(uuid, style::error(&self.lang, key, &Args::new()))]);
        }
        if held {
            return (true, vec![Effect::Say(uuid, style::error(&self.lang, "check-wait", &Args::new()))]);
        }
        (false, Vec::new())
    }

    /// The client's brand (raw, as the host reports it) and whether it sent its
    /// settings, after [`Effect::QueryClient`].
    pub fn client_info(&mut self, uuid: &str, brand: &str, settings_sent: bool) -> Vec<Effect> {
        let uuid = key(uuid);
        self.as_player(self.lang_of(&uuid), |g| g.client_info_as(uuid, brand, settings_sent))
    }

    fn client_info_as(&mut self, uuid: String, brand: &str, settings_sent: bool) -> Vec<Effect> {
        let Some(p) = self.players.get(&uuid) else { return Vec::new() };
        if matches!(p.phase, Phase::Free) {
            return Vec::new();
        }
        let brand = clean_brand(brand);
        match client::check(&self.cfg.client_check, &brand, settings_sent) {
            Ok(()) => Vec::new(),
            Err(why) => {
                self.stats.failed += 1;
                let reason = why.reason(u64::from(self.cfg.client_check.check_ticks));
                vec![
                    Effect::Log(format!(
                        "client check failed for {}: {reason}; brand \"{brand}\", settings sent: {settings_sent}",
                        p.name
                    )),
                    Effect::Kick(uuid, self.kick_text(why.message_key())),
                ]
            }
        }
    }

    /// Timers: waits, time limits, map re-sends, client checks, the CAPTCHA
    /// pool, attack summaries and the hourly clean-up.
    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        let mut fx = Vec::new();
        for uuid in self.held() {
            fx.extend(self.as_player(self.lang_of(&uuid), |g| g.tick_player(&uuid, now)));
        }
        self.pool.work(2);
        let every = u64::from(self.cfg.attack.log_interval_seconds) * 1000;
        if now.saturating_sub(self.last_attack_log) >= every {
            self.last_attack_log = now;
            let (connections, blocked) = self.attack.take_window();
            if self.level(now) != AttackLevel::Normal {
                fx.push(Effect::Log(format!(
                    "attack mode: {connections} connections and {blocked} blocked in the last {} s",
                    self.cfg.attack.log_interval_seconds
                )));
            }
        }
        if now.saturating_sub(self.last_purge) >= PURGE_EVERY_MS {
            self.last_purge = now;
            self.reconnect.purge(now, self.cfg.attack.reconnect_max_seconds);
            if let Some(s) = &self.store {
                let purged = VerifiedCache::new(s).purge(now).and_then(|_| Returns::new(s).purge(now));
                if let Err(e) = purged {
                    fx.push(Effect::Log(format!("clean-up of the database failed: {e}")));
                }
            }
        }
        fx
    }

    fn tick_player(&mut self, uuid: &str, now: u64) -> Vec<Effect> {
        enum Due {
            Nothing,
            SpawnWait { put_back: bool },
            FallLimit { confirmed: bool },
            CaptchaLimit,
            Resend(i32, Arc<Vec<u8>>),
        }
        let fall_limit = u64::from(self.cfg.gravity.falling_check_ticks + self.cfg.gravity.falling_grace_ticks) * 50;
        let mut fx = Vec::new();
        let Some(p) = self.players.get_mut(uuid) else { return Vec::new() };
        if p.client_due.is_some_and(|t| t <= now) {
            p.client_due = None;
            fx.push(Effect::QueryClient(uuid.to_string()));
        }
        let fallback = p.checks.captcha || p.checks.captcha_on_fail;
        let due = match &mut p.phase {
            Phase::Spawning { since, put_back } if now.saturating_sub(*since) >= SPAWN_WAIT_MS => {
                Due::SpawnWait { put_back: *put_back }
            }
            Phase::Falling { check, since } if now.saturating_sub(*since) >= fall_limit => {
                Due::FallLimit { confirmed: check.armed() }
            }
            Phase::Captcha { until, .. } if now >= *until => Due::CaptchaLimit,
            Phase::Captcha { map: Some((id, pixels)), resend, .. } if resend.first().is_some_and(|t| *t <= now) => {
                resend.remove(0);
                Due::Resend(*id, pixels.clone())
            }
            _ => Due::Nothing,
        };
        match due {
            Due::Nothing => {}
            Due::SpawnWait { put_back } => {
                // No movement at all: a bot, or a player in a vehicle. Never
                // teleport without a movement (the spawn may be unconfirmed).
                if !put_back && fallback && self.cfg.captcha.enabled {
                    fx.extend(self.start_captcha(uuid, now));
                } else {
                    self.stats.failed += 1;
                    fx.push(Effect::Kick(uuid.to_string(), self.kick_text("kick-timeout")));
                }
            }
            Due::FallLimit { confirmed } => fx.extend(self.fall_failed(uuid, now, confirmed)),
            Due::CaptchaLimit => {
                self.stats.failed += 1;
                fx.push(Effect::Kick(uuid.to_string(), self.kick_text("kick-captcha-failed")));
            }
            Due::Resend(map_id, pixels) => fx.push(Effect::SendMap { uuid: uuid.to_string(), map_id, pixels }),
        }
        fx
    }

    /// The player left. Runs before the server saves the player: a player in
    /// the air goes back to their place first.
    pub fn leave(&mut self, uuid: &str) -> Vec<Effect> {
        let uuid = key(uuid);
        let Some(p) = self.players.remove(&uuid) else { return Vec::new() };
        let mut fx = Vec::new();
        if p.in_air() {
            fx.push(Effect::Teleport(uuid.clone(), p.real.pos));
            fx.push(Effect::ResetFall(uuid.clone()));
        }
        if p.map_given {
            fx.push(Effect::TakeMap(uuid));
        }
        fx
    }

    /// The plugin is unloaded. Pumpkin lets no plugin teleport or disconnect a
    /// player while it is being unloaded, so held players are released where
    /// they are and told to join again; a player in the air stops falling on
    /// the client (and cannot be hurt by the fall). The return record stays:
    /// their next join, with PumboFilter loaded, puts them back. A server stop
    /// is different: players leave before plugins are unloaded, and the leave
    /// puts them back.
    pub fn unload(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        for uuid in self.held() {
            let Some(p) = self.players.remove(&uuid) else { continue };
            if p.in_air() {
                fx.push(Effect::Hover(uuid.clone()));
            }
            fx.push(Effect::ResetFall(uuid.clone()));
            if p.map_given {
                fx.push(Effect::TakeMap(uuid.clone()));
            }
            fx.push(Effect::Unhold(uuid.clone()));
            fx.push(Effect::Say(uuid, style::warn(&self.lang, "reload-join-again", &Args::new())));
        }
        fx
    }

    /// Adds a nick + address pair to the verified cache (`verify`).
    pub fn verify(&self, name: &str, ip: &str, now: u64) -> pumbo_common::store::Result<()> {
        let store = self.store.as_ref().ok_or_else(|| pumbo_common::store::StoreError::new("no database"))?;
        VerifiedCache::new(store).add(name, ip, now + self.cfg.filter.verified_ttl_ms())
    }

    pub fn unverify(&self, name: &str) -> pumbo_common::store::Result<u64> {
        let store = self.store.as_ref().ok_or_else(|| pumbo_common::store::StoreError::new("no database"))?;
        VerifiedCache::new(store).remove_name(name)
    }

    pub fn online_count(&self) -> usize {
        self.players.len()
    }
}

/// The CAPTCHA code for a title, every character in another colour.
fn colored_code(code: &str, seed: u64) -> Text {
    const COLORS: [Named; 6] =
        [Named::Gold, Named::Yellow, Named::Aqua, Named::Green, Named::LightPurple, Named::White];
    let mut line = Line::new();
    let mut rng = pumbo_common::random::Rng::new(seed);
    for ch in code.chars() {
        let color = COLORS.get(rng.below(COLORS.len() as u32) as usize).copied().unwrap_or(Named::White);
        line = line.with(Segment::new(ch.to_string(), Style::colored(Color::Named(color)).bolded()));
    }
    Text::from(line)
}

#[cfg(test)]
mod tests;
