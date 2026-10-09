//! Carrying out the core's effects, the CAPTCHA map and the release rule.

use std::sync::Arc;

use pumbo_common::gate::{self, Hold};
use pumbo_common::map::map_block_entity_nbt;
use pumbo_filter_core::gate::Effect;

use super::api::{self, BlockEntityType, BlockPos, DataComponent, Hand, ItemStack, Player, Server};
use super::state;
use crate::PARTNER;

/// Runs `f` on the player an effect is for: the event's own player first (in
/// the leave handler the server no longer finds them by UUID), then the server.
fn with_player(server: &Server, current: Option<&Player>, uuid: &str, f: impl FnOnce(&Player)) {
    if let Some(p) = current
        && api::uuid_of(p) == uuid
    {
        f(p);
    } else if let Some(p) = api::api_uuid(uuid).and_then(|u| server.get_player_by_uuid(u)) {
        f(&p);
    }
}

/// What the other gate plugin says about a player; its absence is "free".
pub fn partner_holding(uuid: &str) -> Hold {
    match api::ipc::send_ipc_message(PARTNER, &gate::holding_request(uuid)) {
        Ok(Ok(answer)) => gate::parse_holding(&answer),
        _ => Hold::Free,
    }
}

pub fn apply(server: &Server, current: Option<&Player>, effects: Vec<Effect>) {
    for e in effects {
        match e {
            Effect::Log(line) => api::info(&format!("PumboFilter: {line}")),
            Effect::Hold(uuid) => with_player(server, current, &uuid, |p| api::hold(server, p)),
            Effect::Finish(uuid) => {
                // The core marked the player free before this; release them
                // unless PumboAuth still holds them (then PumboAuth releases).
                if partner_holding(&uuid) != Hold::Holding {
                    with_player(server, current, &uuid, |p| api::release(server, p));
                }
            }
            Effect::Unhold(uuid) => with_player(server, current, &uuid, |p| api::release(server, p)),
            Effect::Hover(uuid) => with_player(server, current, &uuid, api::hover),
            Effect::QueryClient(uuid) => {
                let mut info = None;
                with_player(server, current, &uuid, |p| {
                    if let Some(java) = p.as_java() {
                        let s = java.get_settings();
                        // Pumpkin fills in these values when the client never sent its settings.
                        let defaults = s.locale == "en_us"
                            && s.view_distance == 8
                            && s.chat_colors
                            && !s.text_filtering
                            && !s.server_listing;
                        info = Some((java.get_brand(), !defaults));
                    }
                });
                let Some((brand, sent)) = info else { continue };
                let fx = state::with(|rt| rt.gate.client_info(&uuid, &brand, sent)).unwrap_or_default();
                apply(server, current, fx);
            }
            other => {
                let uuid = match &other {
                    Effect::Teleport(u, _)
                    | Effect::ResetFall(u)
                    | Effect::Say(u, _)
                    | Effect::ActionBar(u, _)
                    | Effect::TakeMap(u)
                    | Effect::Kick(u, _) => u.clone(),
                    Effect::Title { uuid, .. } | Effect::ShowMap { uuid, .. } | Effect::SendMap { uuid, .. } => {
                        uuid.clone()
                    }
                    _ => continue,
                };
                with_player(server, current, &uuid, |p| on_player(p, &uuid, other));
            }
        }
    }
}

fn on_player(p: &Player, uuid: &str, e: Effect) {
    match e {
        Effect::Teleport(_, pos) => api::teleport(p, pos),
        Effect::ResetFall(_) => api::reset_fall(p),
        Effect::Say(_, t) => api::say(p, &t),
        Effect::ActionBar(_, t) => p.show_actionbar(api::component(&t)),
        Effect::Title { title, subtitle, stay_ticks, .. } => {
            p.send_title_animation(5, stay_ticks, 10);
            p.show_subtitle(api::component(&subtitle));
            p.show_title(api::component(&title));
        }
        Effect::ShowMap { map_id, pixels, .. } => {
            give_map(p, uuid, map_id);
            send_map(p, map_id, &pixels);
        }
        Effect::SendMap { map_id, pixels, .. } => send_map(p, map_id, &pixels),
        Effect::TakeMap(_) => take_map(p, uuid),
        Effect::Kick(_, t) => api::kick(p, &t),
        _ => {}
    }
}

/// A `filled_map` showing map `map_id`.
fn map_item(map_id: i32) -> ItemStack {
    let item = ItemStack::new("minecraft:filled_map", 1);
    item.set_component(DataComponent::MapId, &varint(map_id));
    item
}

fn varint(v: i32) -> Vec<u8> {
    let mut v = v as u32;
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

/// Puts the map into the main hand, only if it is empty: a real item is never
/// taken out of the inventory for it.
fn give_map(p: &Player, uuid: &str, map_id: i32) {
    if p.get_item_in_hand(Hand::Right).is_some() {
        return;
    }
    let slot = p.get_inventory().get_selected_slot();
    p.set_item_in_hand(Hand::Right, Some(map_item(map_id)));
    state::with(|rt| rt.map_slots.insert(uuid.to_string(), slot));
}

/// Takes the map back out of the slot it went into (only a filled map: the
/// slot was empty and the player could not move items since).
fn take_map(p: &Player, uuid: &str) {
    let Some(slot) = state::with(|rt| rt.map_slots.remove(uuid)).flatten() else { return };
    let inventory = p.get_inventory().as_inventory();
    let ours = inventory.get_item(u32::from(slot)).is_some_and(|item| {
        let key = item.get_registry_key();
        key.strip_prefix("minecraft:").unwrap_or(&key) == "filled_map"
    });
    if ours {
        inventory.set_item(u32::from(slot), None);
    }
}

/// Sends the pixels of a map through a map block entity at the bottom of the
/// player's chunk (the only way on released Pumpkin; the server broadcasts it).
fn send_map(p: &Player, map_id: i32, pixels: &Arc<Vec<u8>>) {
    let world = p.get_world();
    let (x, _, z) = p.get_position();
    let pos = BlockPos { x: (x.floor() as i32) & !15, y: world.get_min_y(), z: (z.floor() as i32) & !15 };
    let entity = match world.get_block_entity(pos) {
        Some(BlockEntityType::MapBlockEntity(m)) => Some(m),
        _ => {
            if world.set_block_entity_nbt(pos, &map_block_entity_nbt(map_id, pixels)).is_err() {
                return;
            }
            match world.get_block_entity(pos) {
                Some(BlockEntityType::MapBlockEntity(m)) => Some(m),
                _ => None,
            }
        }
    };
    let Some(map) = entity else {
        api::warn("PumboFilter: cannot place the CAPTCHA map (chunk not loaded?)");
        return;
    };
    map.set_map_id(map_id);
    map.set_colors(pixels);
    map.update();
}
