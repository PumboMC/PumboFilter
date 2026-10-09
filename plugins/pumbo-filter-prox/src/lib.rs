//! PumboFilter for the PumboProx proxy: anti-bot checks in the proxy's
//! virtual world, before any server sees the player.
//!
//! All rules live in `pumbo-filter-core` (the same gate as on Pumpkin, in its
//! limbo mode: no way back on disk); this crate connects it to the proxy:
//!
//! - `on-pre-login`: attack mode (connection rate), the reconnect gate, the
//!   in-memory block of addresses that failed too often (`auto-ban`),
//! - `on-gate` (gate `filter`, priority 100, after login): players who skip the
//!   checks pass; the others enter the virtual world, stand on an invisible
//!   platform, fall in the void for the gravity check and answer a CAPTCHA on
//!   a map in their hand. Passing releases them to the next gate (PumboAuth,
//!   priority 200) or to a server, in the same connection,
//! - `on-virtual-input`: movement, chat and commands of held players (the
//!   proxy runs no proxy command for them, so `/server` cannot skip the check),
//! - the client check reads the brand and settings the client sent to the proxy,
//! - commands `/pumbofilter <sub>` (`/pumbo filter <sub>`), also from the console,
//! - for other plugins: the topic `pumbo:attack-state@1.0`; with PumboBans,
//!   automatic IP bans through `pumbobans:punish@1.0`.

pub mod setup;

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};

use pumbo_common::clock::now_ms;
use pumbo_common::id::{Uuid, strip_port};
use pumbo_common::rich::{Text as Rich, json};
use pumbo_common::store::{Store, StoreError};
use pumbo_common::style;
use pumbo_common::text::Args;
use pumbo_filter_core::attack::AttackLevel;
use pumbo_filter_core::autoban::Strikes;
use pumbo_filter_core::commands::{self as cmds, Outcome, Sender};
use pumbo_filter_core::gate::{Effect, Entry, Gate, Joining};
use pumbo_filter_core::place::Pos;
use pumbo_sdk::bindings::pumbo::prox::types::{Route, TitleTimes};
use pumbo_sdk::contracts::{ATTACK_STATE, AttackState};
use pumbo_sdk::virtual_world::{self as v, Input};
use pumbo_sdk::{
    Command, CommandEvent, GateReply, PlayerId, PlayerInfo, PreLoginEvent, PreLoginReply, QueryContext, Text, bus, log,
    permissions, players, scheduler, services,
};

use crate::setup::Config;

/// Gate ticks (timeouts, client checks, the CAPTCHA pool).
const TICK_MS: u64 = 50;
/// Standard subcommands the proxy runs itself under `/pumbo filter`.
const HOST_SUBCOMMANDS: &[&str] = &["reload", "version", "debug"];
/// Command names in the tree of the virtual world.
const WORLD_COMMANDS: &[&str] = &["captcha"];
const BYPASS: &str = "pumbo.filter.bypass";
const BUSY: &str = "&cPumboFilter is busy, please join again.";

/// A player the gate knows about.
#[derive(Debug, Default)]
struct Seen {
    uuid: String,
    ip: String,
    /// The world shows (`loaded`).
    loaded: bool,
    /// CAPTCHA titles from before the world showed: the proxy would send them
    /// while the loading screen still covers them.
    later: Vec<Effect>,
}

struct State {
    gate: Gate,
    cfg: Config,
    players: HashMap<PlayerId, Seen>,
    by_uuid: HashMap<String, PlayerId>,
    strikes: Strikes,
    ticks: u64,
    /// Last published attack level.
    level: AttackLevel,
}

pub struct PumboFilter {
    state: RefCell<Option<State>>,
    /// Read-only config folder (`plugins/pumbo-filter/`).
    pub config_dir: String,
    /// Data folder (`plugins/data/pumbo-filter/`), holds `filter.redb`.
    pub data_dir: String,
}

impl Default for PumboFilter {
    fn default() -> Self {
        PumboFilter { state: RefCell::new(None), config_dir: "/config".into(), data_dir: "/data".into() }
    }
}

/// The gate's key for a player: UUID with dashes, lowercase.
fn uuid_of(p: &PlayerInfo) -> String {
    Uuid::from_high_low(p.profile.id.high, p.profile.id.low).to_string()
}

fn text(t: &Rich) -> Text {
    Text::Json(json(t))
}

fn reply(to: Option<PlayerId>, t: &Rich) {
    if t.is_empty() {
        return;
    }
    match to {
        Some(id) => players::send_message(id, text(t)),
        None => log::info(&t.plain()),
    }
}

fn level_number(l: AttackLevel) -> u8 {
    match l {
        AttackLevel::Normal => 0,
        AttackLevel::Active => 1,
        AttackLevel::Reconnect => 2,
    }
}

thread_local! {
    /// One world for every held player: each gets its own view of it.
    static WORLD: OnceCell<v::World> = const { OnceCell::new() };
}

fn position(p: Pos) -> v::Position {
    v::Position { x: p.x, y: p.y, z: p.z, yaw: p.yaw, pitch: p.pitch }
}

/// Into the virtual world, made at the first call (the barrier platform).
fn enter(id: PlayerId) -> Result<(), String> {
    WORLD.with(|w| {
        let world = w.get_or_init(|| {
            let opts = v::WorldOptions { time: 18_000, light: 15, game_mode: v::GameMode::Adventure, view_distance: 2 };
            let world = v::World::new(opts);
            for (x, y, z) in setup::platform() {
                let _ = world.set_block(v::BlockPos { x, y, z }, "minecraft:barrier");
            }
            world
        });
        let commands: Vec<String> = WORLD_COMMANDS.iter().map(|c| (*c).to_string()).collect();
        v::enter(id, world, position(setup::spawn().pos), &commands)
    })
}

impl PumboFilter {
    /// Runs `f` with the state; `None` when it is missing or borrowed. Never
    /// `.await` inside `f`.
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> Option<R> {
        self.state.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f))
    }

    /// Loads config, messages and the database, registers the commands.
    pub fn start(&self, store: Result<Store, StoreError>) -> Result<(), String> {
        let (cfg, mut warnings, found) = setup::load_config(&self.config_dir);
        if !found {
            log::info("PumboFilter: no config.yml in plugins/pumbo-filter/, using the defaults");
        }
        let (lang, others, w) = setup::load_langs(&self.config_dir, &cfg);
        warnings.extend(w);
        for w in &warnings {
            log::warn(&format!("PumboFilter: config: {w}"));
        }
        let mut gate = Gate::new(cfg.settings(), lang, store);
        gate.others = others;
        gate.limbo = true;
        if let Some(why) = &gate.store_error {
            log::error(&format!("PumboFilter: the database is not available, no verified cache: {why}"));
        }
        let tree = cmds::tree();
        for s in tree.subs().iter().filter(|s| !HOST_SUBCOMMANDS.contains(&s.name)) {
            let spec = Command::new(s.name)
                .permission(&tree.permission(s))
                .usage(&format!("/pumbofilter {} {}", s.name, s.usage))
                .umbrella();
            if let Err(e) = spec.register() {
                log::warn(&format!("PumboFilter: /pumbofilter {} not registered: {e}", s.name));
            }
        }
        if let Err(e) = Command::new("help").usage("/pumbofilter help [page]").umbrella().register() {
            log::warn(&format!("PumboFilter: /pumbofilter help not registered: {e}"));
        }
        scheduler::every(TICK_MS);
        *self.state.borrow_mut() = Some(State {
            gate,
            cfg,
            players: HashMap::new(),
            by_uuid: HashMap::new(),
            strikes: Strikes::default(),
            ticks: 0,
            level: AttackLevel::Normal,
        });
        log::info(&format!("PumboFilter {} loaded (gate \"filter\")", env!("CARGO_PKG_VERSION")));
        Ok(())
    }

    /// The error of a file that is not valid YAML stops the reload (the
    /// current settings stay).
    fn reload(&self) -> Result<(), String> {
        let (cfg, mut warnings, _) = setup::load_config(&self.config_dir);
        let (lang, others, w) = setup::load_langs(&self.config_dir, &cfg);
        warnings.extend(w);
        if let Some(w) = warnings.iter().find(|w| w.fatal) {
            log::warn(&format!("PumboFilter: reload refused, the current settings stay: {w}"));
            return Err(w.message.clone());
        }
        for w in &warnings {
            log::warn(&format!("PumboFilter: config: {w}"));
        }
        self.with(|s| {
            s.gate.reload(cfg.settings(), lang);
            s.gate.others = others;
            s.cfg = cfg;
        });
        Ok(())
    }

    /// Carries out the gate's effects. Returns the addresses to ban (they
    /// failed too often), which needs an async call.
    fn apply(&self, effects: Vec<Effect>) -> Vec<String> {
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        let mut bans = Vec::new();
        while let Some(e) = queue.pop_front() {
            if let Effect::Log(line) = &e {
                log::info(&format!("PumboFilter: {line}"));
                continue;
            }
            let uuid = match &e {
                Effect::Hold(u)
                | Effect::Teleport(u, _)
                | Effect::ResetFall(u)
                | Effect::Say(u, _)
                | Effect::ActionBar(u, _)
                | Effect::TakeMap(u)
                | Effect::QueryClient(u)
                | Effect::Kick(u, _)
                | Effect::Finish(u)
                | Effect::Unhold(u)
                | Effect::Hover(u) => u.clone(),
                Effect::Title { uuid, .. } | Effect::ShowMap { uuid, .. } | Effect::SendMap { uuid, .. } => {
                    uuid.clone()
                }
                Effect::Log(_) => continue,
            };
            // The proxy keeps messages, titles and bars until it sends the world,
            // but the loading screen may still cover it: a CAPTCHA title waits
            // for `loaded`, so the time to read the code is not lost.
            let target = self
                .with(|s| {
                    let id = *s.by_uuid.get(&uuid)?;
                    let seen = s.players.get_mut(&id)?;
                    if !seen.loaded && matches!(e, Effect::Title { .. }) && s.gate.in_captcha(&uuid) {
                        seen.later.push(e.clone());
                        return Some(None);
                    }
                    Some(Some((id, seen.ip.clone())))
                })
                .flatten();
            let Some(Some((id, ip))) = target else { continue };
            match e {
                Effect::Teleport(_, pos) => {
                    v::teleport(id, position(pos));
                }
                Effect::Say(_, t) => reply(Some(id), &t),
                Effect::ActionBar(_, t) => players::send_action_bar(id, text(&t)),
                Effect::Title { title, subtitle, stay_ticks, .. } => {
                    let stay = u32::try_from(stay_ticks).unwrap_or(0);
                    players::send_title(
                        id,
                        text(&title),
                        text(&subtitle),
                        TitleTimes { fade_in: 5, stay, fade_out: 10 },
                    );
                }
                Effect::ShowMap { pixels, .. } => v::show_map(id, &v::MapImage::new(&pixels), v::Hand::Main),
                Effect::TakeMap(_) => v::clear_inventory(id),
                Effect::QueryClient(_) => {
                    let Some(info) = players::get(id) else { continue };
                    let brand = info.brand.unwrap_or_default();
                    let sent = info.settings.is_some();
                    let fx = self.with(|s| s.gate.client_info(&uuid, &brand, sent)).unwrap_or_default();
                    queue.extend(fx);
                }
                Effect::Kick(_, t) => {
                    // Every kick of the gate is a failed check.
                    let ban = self.with(|s| s.strikes.fail(&s.cfg.auto_ban, &ip, now_ms())).unwrap_or(false);
                    if ban {
                        bans.push(ip);
                    }
                    players::kick(id, text(&t));
                }
                Effect::Finish(_) | Effect::Unhold(_) => {
                    if let Err(err) = v::release(id) {
                        log::warn(&format!("PumboFilter: cannot release {uuid}: {err}"));
                    }
                }
                // The proxy sends the map when the world shows; nothing falls
                // anywhere else.
                Effect::SendMap { .. } | Effect::ResetFall(_) | Effect::Hover(_) => {}
                // The gate's own answer; logged above.
                Effect::Hold(_) | Effect::Log(_) => {}
            }
        }
        bans
    }

    /// A temporary IP ban through PumboBans; without it the address stays
    /// blocked in memory ([`Strikes`]).
    async fn ban(&self, ip: String) {
        use pumbo_common::bans::{METHOD, Request, SERVICE};
        let Some(cfg) = self.with(|s| s.cfg.auto_ban.clone()) else { return };
        let req = Request::Punish {
            kind: "ban".into(),
            target: ip.clone(),
            duration: Some(format!("{}m", cfg.ban_minutes)),
            reason: Some(cfg.reason.clone()),
            ip: true,
            silent: true,
        };
        if services::lookup(SERVICE).is_none() {
            log::info(&format!("PumboFilter: {ip} blocked for {} min after failed checks", cfg.ban_minutes));
            return;
        }
        let opts = services::CallOptions { timeout_ms: Some(1000), player: None, ctx: None };
        match services::call(SERVICE.into(), METHOD.into(), req.to_bytes(), opts).await {
            Ok(answer) if String::from_utf8_lossy(&answer).contains("\"ok\":true") => {
                log::info(&format!("PumboFilter: {ip} banned by PumboBans after failed checks"));
            }
            Ok(answer) => log::warn(&format!(
                "PumboFilter: PumboBans refused the ban of {ip} ({}), blocked in memory",
                String::from_utf8_lossy(&answer)
            )),
            Err(e) => log::warn(&format!("PumboFilter: PumboBans not reached ({e:?}), {ip} blocked in memory")),
        }
    }

    async fn ban_all(&self, ips: Vec<String>) {
        for ip in ips {
            self.ban(ip).await;
        }
    }

    /// Publishes `pumbo:attack-state` when the level changed.
    fn publish_level(&self) {
        let changed = self.with(|s| {
            let now = s.gate.level(now_ms());
            (now != s.level).then(|| {
                s.level = now;
                now
            })
        });
        if let Some(Some(level)) = changed {
            let state = AttackState { active: level != AttackLevel::Normal, level: level_number(level) };
            if let Err(e) = bus::publish_value(&ATTACK_STATE.versioned(), &state) {
                log::debug(&format!("PumboFilter: attack state not published: {e}"));
            }
        }
    }

    fn forget(&self, id: PlayerId) {
        let fx = self
            .with(|s| {
                let seen = s.players.remove(&id)?;
                s.by_uuid.remove(&seen.uuid);
                Some(s.gate.leave(&seen.uuid))
            })
            .flatten()
            .unwrap_or_default();
        // A player who left needs nothing more (no file to put right).
        drop(fx);
    }
}

impl pumbo_sdk::Plugin for PumboFilter {
    async fn init(&self) -> Result<(), String> {
        let store = Store::open(format!("{}/filter.redb", self.data_dir));
        self.start(store)
    }

    async fn on_reload(&self) -> Result<(), String> {
        self.reload()
    }

    async fn on_pre_login(&self, e: PreLoginEvent) -> PreLoginReply {
        let ip = strip_port(&e.connection.address);
        let now = now_ms();
        let decision = self.with(|s| {
            if s.strikes.blocked(&ip, now) {
                return Entry::Refuse(Rich::parse(&s.gate.lang.get("kick-bot-blocked")));
            }
            // The proxy itself refuses a second connection of an online player.
            s.gate.pre_login(&e.name, &ip, None, now).0
        });
        self.publish_level();
        match decision {
            Some(Entry::Allow) => PreLoginReply::Allow,
            Some(Entry::Refuse(t)) => PreLoginReply::Deny(text(&t)),
            None => PreLoginReply::Deny(Text::Legacy(BUSY.into())),
        }
    }

    async fn on_gate(&self, p: PlayerInfo) -> GateReply {
        let uuid = uuid_of(&p);
        let ip = strip_port(&p.connection.address);
        let bypass = permissions::has(p.id, BYPASS, &QueryContext::Global);
        let via = p.connection.route == Route::Viaproxy;
        let joined = self.with(|s| {
            let j = Joining {
                uuid: uuid.clone(),
                name: p.profile.name.clone(),
                ip: ip.clone(),
                java: true,
                bypass,
                premium: p.online_mode,
                hand_empty: true,
                place: setup::spawn(),
                // The proxy reads the client's settings before the gates; none
                // (bots, ViaProxy) means the configured language.
                locale: p.settings.as_ref().map(|c| c.locale.clone()),
                mode: via.then_some(s.cfg.viaproxy.check_mode),
            };
            s.players.insert(p.id, Seen { uuid: uuid.clone(), ip: ip.clone(), ..Seen::default() });
            s.by_uuid.insert(uuid.clone(), p.id);
            s.gate.join(j, now_ms())
        });
        let Some(fx) = joined else { return GateReply::Deny(Text::Legacy(BUSY.into())) };
        if !fx.iter().any(|e| matches!(e, Effect::Hold(_))) {
            self.apply(fx);
            return GateReply::Pass;
        }
        if let Err(e) = enter(p.id) {
            log::warn(&format!("PumboFilter: {} cannot enter the virtual world: {e}", p.profile.name));
            self.forget(p.id);
            return GateReply::Deny(Text::Legacy(BUSY.into()));
        }
        let bans = self.apply(fx);
        self.ban_all(bans).await;
        GateReply::Hold
    }

    async fn on_virtual_input(&self, batch: Vec<Input>) {
        let now = now_ms();
        let mut bans = Vec::new();
        for input in batch {
            let fx = match input {
                Input::Moved((id, pos, _)) => self
                    .with(|s| {
                        let uuid = s.players.get(&id)?.uuid.clone();
                        Some(s.gate.moved(&uuid, pos.x, pos.y, pos.z, now))
                    })
                    .flatten(),
                Input::Loaded(id) => self
                    .with(|s| {
                        let seen = s.players.get_mut(&id)?;
                        seen.loaded = true;
                        Some(std::mem::take(&mut seen.later))
                    })
                    .flatten(),
                Input::Chat((id, msg)) => self
                    .with(|s| {
                        let uuid = s.players.get(&id)?.uuid.clone();
                        Some(s.gate.chat(&uuid, &msg, now).1)
                    })
                    .flatten(),
                Input::Command((id, line)) => self
                    .with(|s| {
                        let uuid = s.players.get(&id)?.uuid.clone();
                        Some(s.gate.command(&uuid, &line, now).1)
                    })
                    .flatten(),
                Input::Left(id) => {
                    self.forget(id);
                    None
                }
                _ => None,
            };
            if let Some(fx) = fx {
                bans.extend(self.apply(fx));
            }
        }
        self.ban_all(bans).await;
    }

    async fn on_disconnect(&self, p: PlayerId) {
        self.forget(p);
    }

    async fn on_timer(&self, _timer: u64) {
        let now = now_ms();
        let (fx, second) = self
            .with(|s| {
                s.ticks += 1;
                let second = s.ticks % 20 == 0;
                if second {
                    let cfg = s.cfg.auto_ban.clone();
                    s.strikes.purge(&cfg, now);
                }
                (s.gate.tick(now), second)
            })
            .unwrap_or_default();
        let bans = self.apply(fx);
        self.ban_all(bans).await;
        if second {
            self.publish_level();
        }
    }

    async fn on_command(&self, e: CommandEvent) {
        let mut words = vec![e.name.clone()];
        words.extend(e.args);
        let nodes: Vec<String> = cmds::tree().subs().iter().map(|s| cmds::tree().permission(s)).collect();
        let granted: BTreeSet<String> = match e.player {
            Some(id) => nodes.into_iter().filter(|n| permissions::has(id, n, &QueryContext::Current)).collect(),
            None => nodes.into_iter().collect(),
        };
        let allowed = |n: &str| granted.contains(n);
        let sender = Sender { console: e.player.is_none(), allowed: &allowed };
        let out = self.with(|s| cmds::run(&mut s.gate, &sender, &words, "PumboProx", now_ms()));
        match out {
            Some(Outcome::Reply(t)) => reply(e.player, &t),
            Some(Outcome::Reload) => {
                let result = self.reload();
                let t = self.with(|s| match &result {
                    Ok(()) => cmds::reloaded(&s.gate, 0),
                    Err(err) => {
                        style::error(&s.gate.lang, "command-reload-failed", &Args::new().arg(style::value(err)))
                    }
                });
                if let Some(t) = t {
                    reply(e.player, &t);
                }
            }
            None => reply(e.player, &Rich::parse("&cPumboFilter is busy, please try again.")),
        }
        self.publish_level();
    }
}

pumbo_sdk::plugin!(PumboFilter);
pumbo_sdk::embed!(
    manifest = "pumbo-filter.yml",
    config = "assets/config.yml",
    lang = ["assets/lang/en.yml", "assets/lang/pl.yml"],
);

#[cfg(test)]
mod tests;
