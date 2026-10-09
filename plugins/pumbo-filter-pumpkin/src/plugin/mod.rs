//! The WebAssembly plugin: loading, registration with Pumpkin, other plugins.

mod api;
mod commands;
mod effects;
mod events;
mod state;

use std::collections::HashMap;

use pumbo_common::clock::now_ms;
use pumbo_common::config::{self, Warning};
use pumbo_common::gate::{self, Hold};
use pumbo_common::lang::Lang;
use pumbo_common::rich::Text;
use pumbo_common::store::Store;
use pumbo_common::style;
use pumbo_common::text::Args;
use pumbo_filter_core::commands::{reloaded, tree};
use pumbo_filter_core::gate::Gate;

use api::{
    ArgumentType, Command, CommandNode, Context, EventPriority, Permission, PermissionDefault, PermissionLevel, Plugin,
    PluginMetadata, SchedulerExt, StringType,
};

use crate::config::{Config, TEMPLATE};
use crate::{BUNDLES, PLUGIN_NAME, pumpkin_node};

fn load_config(dir: &str) -> (Config, Vec<Warning>) {
    let (text, warning) = config::read_or_create(&format!("{dir}/config.yml"), TEMPLATE);
    let (cfg, mut warnings) = config::load::<Config>(&text);
    warnings.extend(warning);
    warnings.extend(config::old_files(dir));
    (cfg, warnings)
}

fn load_lang(dir: &str, code: &str) -> (Lang, Vec<Warning>) {
    let template = Lang::template(&BUNDLES, code);
    let (text, warning) = config::read_or_create(&format!("{dir}/lang/{code}.yml"), &template);
    let (lang, warnings) = Lang::load(&BUNDLES, code, Some(&text));
    let mut warnings: Vec<Warning> =
        warnings.into_iter().map(|w| Warning { message: format!("lang/{code}.yml: {}", w.message), ..w }).collect();
    warnings.extend(warning);
    (lang, warnings)
}

/// `/pumbofilter reload`: config and messages (the database stays).
fn reload(rt: &mut state::Rt) -> Text {
    let (cfg, mut warnings) = load_config(&rt.dir);
    let (lang, more) = load_lang(&rt.dir, &cfg.language);
    warnings.extend(more);
    if let Some(w) = warnings.iter().find(|w| w.fatal) {
        api::warn(&format!("PumboFilter: reload refused, the current settings stay: {w}"));
        return style::error(&rt.gate.lang, "command-reload-failed", &Args::new().arg(style::value(&w.message)));
    }
    for w in &warnings {
        api::warn(&format!("PumboFilter: config: {w}"));
    }
    rt.gate.reload(cfg.settings(), lang);
    reloaded(&rt.gate, warnings.len())
}

pub struct PumboFilter;

impl Plugin for PumboFilter {
    fn new() -> Self {
        PumboFilter
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Patryk Skoczylas".into()],
            description: "Anti-bot checks: gravity check, map CAPTCHA, client check, attack mode".into(),
            dependencies: vec![],
            permissions: vec![api::permissions::FS_READ_DATA.into(), api::permissions::FS_WRITE_DATA.into()],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let dir = context.get_data_folder();
        let _ = std::fs::create_dir_all(format!("{dir}/lang"));
        let (cfg, mut warnings) = load_config(&dir);
        let (lang, more) = load_lang(&dir, &cfg.language);
        warnings.extend(more);
        for w in &warnings {
            api::warn(&format!("PumboFilter: config: {w}"));
        }
        let store = Store::open(format!("{dir}/filter.redb"));
        let mut gate = Gate::new(cfg.settings(), lang, store);
        if let Some(why) = &gate.store_error {
            api::error(&format!(
                "PumboFilter: the database is not available, running without the verified cache and the gravity check: {why}"
            ));
        }

        // Players already online (a reload of the plugin) are free.
        let server = context.get_server();
        let mut entities = HashMap::new();
        for p in server.get_all_players() {
            let uuid = api::uuid_of(&p);
            entities.insert(api::entity_id(&p), uuid.clone());
            gate.adopt(&uuid, &p.get_name(), &pumbo_common::id::strip_port(&p.get_ip()), api::place_of(&p));
        }
        state::install(state::Rt { dir, gate, entities, map_slots: HashMap::new() });

        // A second registration of the same node (after a reload) is an error
        // in Pumpkin; the node is there either way.
        let admin = PermissionDefault::Op(PermissionLevel::Three);
        let mut nodes: Vec<(String, String, PermissionDefault)> = tree()
            .subs()
            .iter()
            .map(|s| (pumpkin_node(&tree().permission(s)), format!("PumboFilter: /pumbofilter {}", s.name), admin))
            .collect();
        nodes.push(("pumbofilter:command".into(), "PumboFilter: see /pumbofilter".into(), admin));
        nodes.push(("pumbofilter:captcha".into(), "PumboFilter: answer the CAPTCHA".into(), PermissionDefault::Allow));
        nodes.push(("pumbofilter:bypass".into(), "PumboFilter: skip the checks".into(), PermissionDefault::Deny));
        for (node, description, default) in nodes {
            let _ = context.register_permission(&Permission { node, description, default, children: vec![] });
        }

        let command = Command::new(
            &[PLUGIN_NAME.into(), pumbo_filter_core::commands::ALIAS.into()],
            "PumboFilter: anti-bot checks",
        )
        .then(
            CommandNode::argument("args", &ArgumentType::String(StringType::Greedy))
                .execute(commands::Admin { with_args: true }),
        )
        .execute(commands::Admin { with_args: false });
        context.register_command(command, "pumbofilter:command");
        let captcha = Command::new(&["captcha".into()], "PumboFilter: answer the CAPTCHA")
            .then(CommandNode::argument("code", &ArgumentType::String(StringType::Greedy)).execute(commands::Captcha));
        context.register_command(captcha, "pumbofilter:captcha");

        context.register_event_handler::<api::AsyncPlayerPreLoginEvent, _>(
            events::PreLogin,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::PlayerJoinEvent, _>(events::Join, EventPriority::Highest, true)?;
        context.register_event_handler::<api::PlayerLeaveEvent, _>(events::Leave, EventPriority::Highest, true)?;
        context.register_event_handler::<api::PlayerMoveEvent, _>(events::Move, EventPriority::Normal, false)?;
        context.register_event_handler::<api::PlayerChatEvent, _>(events::Chat, EventPriority::Highest, true)?;
        context.register_event_handler::<api::PlayerCommandSendEvent, _>(
            events::CommandSend,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::EntityDamageEvent, _>(events::Damage, EventPriority::Highest, true)?;
        context.register_event_handler::<api::PlayerInteractEvent, _>(
            events::Interact,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::PlayerInteractEntityEvent, _>(
            events::InteractEntity,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::BlockBreakEvent, _>(events::Break, EventPriority::Highest, true)?;
        context.register_event_handler::<api::BlockPlaceEvent, _>(events::Place, EventPriority::Highest, true)?;
        context.register_event_handler::<api::PlayerDropItemEvent, _>(events::Drop, EventPriority::Highest, true)?;
        context.register_event_handler::<api::InventoryClickEvent, _>(events::Click, EventPriority::Highest, true)?;
        context.register_event_handler::<api::InventoryCreativeEvent, _>(
            events::Creative,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::PlayerItemHeldEvent, _>(
            events::HeldSlot,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::PlayerItemConsumeEvent, _>(
            events::Consume,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<api::PlayerSwapHandsEvent, _>(
            events::SwapHands,
            EventPriority::Highest,
            true,
        )?;

        // One repeating task for every timer (the API never frees task closures).
        context.schedule_repeating_task(1, 1, |server| {
            let fx = state::with(|rt| rt.gate.tick(now_ms())).unwrap_or_default();
            effects::apply(&server, None, fx);
        });

        api::info(&format!("PumboFilter {} loaded for {}", env!("CARGO_PKG_VERSION"), api::PLATFORM));
        Ok(())
    }

    fn on_unload(&self, context: Context) -> Result<(), String> {
        // Pumpkin lets no plugin teleport or disconnect players while it is
        // being unloaded (those calls stop the plugin): held players are
        // released where they are and asked to join again; a player in the air
        // stops falling on the client, and the stored way back puts them back
        // at their next join. Then the database is closed.
        let fx = state::with(|rt| rt.gate.unload()).unwrap_or_default();
        let held = fx.iter().filter(|e| matches!(e, pumbo_filter_core::gate::Effect::Unhold(_))).count();
        effects::apply(&context.get_server(), None, fx);
        api::info(&format!("PumboFilter: unloaded, {held} players in the check were released"));
        state::uninstall();
        Ok(())
    }

    fn handle_ipc_message(&self, _sender: String, message: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(gate::answer(&message, "PumboFilter", env!("CARGO_PKG_VERSION"), |uuid| {
            state::with(|rt| rt.gate.holding(uuid)).unwrap_or(Hold::Unknown)
        }))
    }
}

crate::papi::register_plugin!(PumboFilter);
