//! Players who passed the checks recently, by nickname and address, so that they
//! are not checked again on every join.
//!
//! Table `verified`: key `<name_key>|<ip_key>`, value the expiry time (Unix ms,
//! [`WriteExt::put_u64`]). Written with eventual durability: losing a few
//! entries in a crash only means a few extra checks.

use pumbo_common::id::{ip_key_str, name_key};
use pumbo_common::store::{Durability, ReadExt, Result, Store, WriteExt, u64_or_zero};

pub const TABLE: &str = "verified";

/// Storage key of a nickname + address pair.
pub fn key(name: &str, ip: &str) -> String {
    format!("{}|{}", name_key(name), ip_key_str(ip))
}

/// The verified cache on top of a plugin's store.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedCache<'a> {
    store: &'a Store,
}

impl<'a> VerifiedCache<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Marks a pair as verified until `expires_ms`.
    pub fn add(&self, name: &str, ip: &str, expires_ms: u64) -> Result<()> {
        let key = key(name, ip);
        self.store.write_with(Durability::Eventual, |tx| tx.put_u64(TABLE, &key, expires_ms))
    }

    pub fn is_verified(&self, name: &str, ip: &str, now_ms: u64) -> Result<bool> {
        let key = key(name, ip);
        Ok(self.store.read(|tx| tx.get_u64(TABLE, &key))?.is_some_and(|until| until > now_ms))
    }

    /// Forgets every address of a nickname. Returns how many entries went.
    pub fn remove_name(&self, name: &str) -> Result<u64> {
        let prefix = format!("{}|", name_key(name));
        self.store.write(|tx| tx.retain(TABLE, &mut |k, _| !k.starts_with(&prefix)))
    }

    /// Removes expired entries and returns how many went. Writes durably, so
    /// that the eventual writes before it reach the disk as well.
    pub fn purge(&self, now_ms: u64) -> Result<u64> {
        self.store.write(|tx| tx.retain(TABLE, &mut |_, v| u64_or_zero(v) > now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_expire_and_are_per_address() {
        let store = Store::in_memory();
        let v = VerifiedCache::new(&store);
        v.add("Bob", "1.2.3.4", 2000).unwrap();
        assert!(v.is_verified("bob", "1.2.3.4", 1500).unwrap());
        assert!(v.is_verified("BOB", "1.2.3.4:5555", 1500).unwrap());
        assert!(!v.is_verified("bob", "1.2.3.4", 2000).unwrap());
        assert!(!v.is_verified("bob", "5.6.7.8", 1500).unwrap());
        v.add("Bob", "5.6.7.8", 9000).unwrap();
        assert_eq!(v.purge(5000).unwrap(), 1);
        assert!(v.is_verified("bob", "5.6.7.8", 5000).unwrap());
        assert_eq!(v.remove_name("Bob").unwrap(), 1);
        assert!(!v.is_verified("bob", "5.6.7.8", 5000).unwrap());
    }

    #[test]
    fn ipv6_counts_by_64() {
        let store = Store::in_memory();
        let v = VerifiedCache::new(&store);
        v.add("Amy", "[2001:db8:0:1::5]:40000", 100).unwrap();
        assert!(v.is_verified("amy", "2001:db8:0:1:ffff::9", 50).unwrap());
        assert!(!v.is_verified("amy", "2001:db8:0:2::5", 50).unwrap());
        assert_eq!(key("Amy", "2001:db8:0:1::5"), "amy|2001:db8:0:1::/64");
    }
}
