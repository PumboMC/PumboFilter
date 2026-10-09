//! Where the gravity check happens on a server without a virtual limbo: high
//! above the player's own place, in their own world, and the stored way back.
//!
//! The player never changes worlds, so the dimension in their data file is
//! always the real one. The only thing the check changes on the server is the
//! position, for the time of the fall. Before the teleport up, the real place is
//! written to table `returns` (key: UUID, value [`ReturnRecord`] as JSON,
//! durably). The record stays until a later join finds the player at a normal
//! height again: if the server saved the player in the air and then crashed,
//! the next join puts them back.

use pumbo_common::store::{Durability, Result, Store, WriteExt, json_or_default};
use serde::{Deserialize, Serialize};

pub const TABLE: &str = "returns";

/// How far above the build limit the fall starts. 300 ticks of free fall are
/// about 650 blocks, so the player stays far above every block.
pub const START_ABOVE_TOP: f64 = 1000.0;
/// From this height above the build limit a stored player counts as "in the
/// air above their place" (no normal play happens that high).
pub const AIR_ABOVE_TOP: f64 = 64.0;
/// Records older than this are dropped (players who never came back).
pub const KEEP_MS: u64 = 30 * 24 * 3_600_000;

/// A position with rotation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

/// A place in a world: the world's id and dimension, and the position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    pub world: String,
    pub dimension: String,
    pub pos: Pos,
}

/// Highest block height of a dimension (exclusive). Unknown dimensions get the
/// largest height Minecraft allows.
pub fn build_top(dimension: &str) -> f64 {
    match dimension.strip_prefix("minecraft:").unwrap_or(dimension) {
        "overworld" => 320.0,
        "the_nether" | "nether" | "the_end" | "end" => 256.0,
        _ => 2032.0,
    }
}

/// Where the fall starts for a player standing at `place`.
pub fn fall_start(place: &Place) -> Pos {
    Pos { y: build_top(&place.dimension) + START_ABOVE_TOP, ..place.pos }
}

/// The height from which a player counts as in the air above the world.
pub fn air_floor(dimension: &str) -> f64 {
    build_top(dimension) + AIR_ABOVE_TOP
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReturnRecord {
    pub place: Place,
    pub at_ms: u64,
}

/// What a join with a stored record means.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    /// No record: nothing to do.
    None,
    /// The player is in the air above their place (the server saved them during
    /// the check): put them back here before anything else.
    PutBack(Place),
    /// The file was saved normally after the check: drop the record.
    Stale,
}

/// Decides what a join means for a stored record.
pub fn recovery(record: Option<&ReturnRecord>, now: &Place) -> Recovery {
    match record {
        None => Recovery::None,
        Some(r) if r.place.world == now.world && now.pos.y >= air_floor(&r.place.dimension) => {
            Recovery::PutBack(r.place.clone())
        }
        Some(_) => Recovery::Stale,
    }
}

/// Return records on top of the filter's store.
#[derive(Debug, Clone, Copy)]
pub struct Returns<'a> {
    store: &'a Store,
}

impl<'a> Returns<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub fn get(&self, uuid: &str) -> Result<Option<ReturnRecord>> {
        let raw = self.store.read(|tx| tx.get(TABLE, uuid))?;
        Ok(raw.map(|b| json_or_default(&b)))
    }

    /// Writes the record durably: it must be on disk before the player is moved.
    pub fn put(&self, uuid: &str, record: &ReturnRecord) -> Result<()> {
        self.store.write_with(Durability::Immediate, |tx| tx.put_json(TABLE, uuid, record))
    }

    pub fn remove(&self, uuid: &str) -> Result<()> {
        self.store.remove(TABLE, uuid).map(|_| ())
    }

    /// Drops records older than [`KEEP_MS`]. Returns how many went.
    pub fn purge(&self, now_ms: u64) -> Result<u64> {
        self.store.write(|tx| {
            tx.retain(TABLE, &mut |_, v| now_ms.saturating_sub(json_or_default::<ReturnRecord>(v).at_ms) < KEEP_MS)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(world: &str, y: f64) -> Place {
        Place {
            world: world.into(),
            dimension: "minecraft:overworld".into(),
            pos: Pos { x: 10.5, y, z: -3.25, yaw: 90.0, pitch: 0.0 },
        }
    }

    #[test]
    fn heights() {
        assert_eq!(build_top("minecraft:overworld"), 320.0);
        assert_eq!(build_top("minecraft:the_nether"), 256.0);
        assert_eq!(build_top("the_end"), 256.0);
        assert_eq!(build_top("custom:tall"), 2032.0);
        let start = fall_start(&place("w", 64.0));
        assert_eq!((start.x, start.y, start.z, start.yaw), (10.5, 1320.0, -3.25, 90.0));
        assert_eq!(air_floor("minecraft:overworld"), 384.0);
    }

    #[test]
    fn recovery_rules() {
        let rec = ReturnRecord { place: place("w", 64.0), at_ms: 1 };
        assert_eq!(recovery(None, &place("w", 1200.0)), Recovery::None);
        assert_eq!(recovery(Some(&rec), &place("w", 1200.0)), Recovery::PutBack(place("w", 64.0)));
        assert_eq!(recovery(Some(&rec), &place("w", 70.0)), Recovery::Stale);
        // another world: the file was saved normally (the player changed worlds later)
        assert_eq!(recovery(Some(&rec), &place("nether", 1200.0)), Recovery::Stale);
    }

    #[test]
    fn records_are_stored_and_purged() {
        let store = Store::in_memory();
        let r = Returns::new(&store);
        let rec = ReturnRecord { place: place("w", 64.0), at_ms: 1000 };
        r.put("u1", &rec).unwrap();
        assert_eq!(r.get("u1").unwrap(), Some(rec.clone()));
        r.put("u2", &ReturnRecord { at_ms: KEEP_MS + 5000, ..rec }).unwrap();
        assert_eq!(r.purge(KEEP_MS + 2000).unwrap(), 1);
        assert_eq!(r.get("u1").unwrap(), None);
        r.remove("u2").unwrap();
        assert_eq!(r.get("u2").unwrap(), None);
    }
}
