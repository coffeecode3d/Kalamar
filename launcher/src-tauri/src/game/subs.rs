//! Which abilities a gear piece's sub slots and main slot can hold.
//!
//! The catalogue (`catalogue.rs`) names the abilities from the player's own RomFS but knows no enum values. The game's
//! ability enum numbers the abilities a sub slot can hold from 0 and the ones only a main slot has from
//! [`SUB_ABILITY_END`] (`docs/save-profile.md`, "Sub abilities"); a main slot takes any ability of the enum from 0, the
//! empty value (-1) excepted (`docs/save-profile.md`, "Main abilities"), and which of them a part may take is what the
//! game's own pieces of that part have (`profile::validate_profile`, from the catalogue). The enum itself, name to
//! value, is what the install wizard's keys step reads from the player's `main` and writes into `s3save.keys.json`
//! (`kit-tools/profile/s3save.py keys`, member `abilities`). This module reads that one member and nothing else of the
//! file: the key table next to it is never parsed into a value, kept or logged.
//!
//! No ability name is written in this launcher: the set is whatever the player's own game says.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// First enum value of the abilities only a main slot has (the engine's `SUB_ABILITY_END`).
pub const SUB_ABILITY_END: i64 = 100;
/// The sub slots of one gear piece.
pub const SUB_SLOTS: usize = 3;
/// The largest keys file read (the one with the RomFS option tables is a few MB).
const MAX_KEYS_BYTES: u64 = 64 << 20;

/// The only member of the keys file this module deserializes; serde skips every other member without keeping it.
#[derive(Deserialize)]
struct KeysAbilities {
    #[serde(default)]
    abilities: BTreeMap<String, i64>,
}

/// The ability names whose enum value is in `range`, in enum order, from the text of a keys file. `None` when the file
/// has no usable `abilities` member or none in the range.
fn names_in(text: &str, range: std::ops::Range<i64>) -> Option<Vec<String>> {
    let k: KeysAbilities = serde_json::from_str(text).ok()?;
    let mut list: Vec<(i64, String)> = k.abilities.into_iter().filter(|(n, v)| range.contains(v) && valid_name(n)).map(|(n, v)| (v, n)).collect();
    if list.is_empty() {
        return None;
    }
    list.sort();
    Some(list.into_iter().map(|(_, n)| n).collect())
}

/// The sub-capable ability names (enum value 0 up to [`SUB_ABILITY_END`] excluded), in enum order, from the text of a
/// keys file. `None` when the file has no usable `abilities` member.
pub fn sub_capable_from_json(text: &str) -> Option<Vec<String>> {
    names_in(text, 0..SUB_ABILITY_END)
}

/// The main-capable ability names (every enum value from 0: the sub-capable ones and the main-only ones), in enum
/// order, from the text of a keys file. `None` when the file has no usable `abilities` member.
pub fn main_capable_from_json(text: &str) -> Option<Vec<String>> {
    names_in(text, 0..i64::MAX)
}

fn read_keys(keys: &Path) -> Option<String> {
    crate::util::read_text_limited(keys, MAX_KEYS_BYTES).ok().flatten()
}

/// The sub-capable abilities of the install whose keys file is at `keys`; `None` without the file or its enum.
pub fn read_sub_capable(keys: &Path) -> Option<Vec<String>> {
    sub_capable_from_json(&read_keys(keys)?)
}

/// The main-capable abilities of the install whose keys file is at `keys`; `None` without the file or its enum.
pub fn read_main_capable(keys: &Path) -> Option<Vec<String>> {
    main_capable_from_json(&read_keys(keys)?)
}

/// An ability name as the engine's enum spells it: a short identifier.
pub fn valid_name(n: &str) -> bool {
    (1..=64).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sub_capable_set_is_the_enum_values_below_100_in_enum_order() {
        // Made-up names; the shape is the keys file's: the key table beside the enum is skipped.
        let text = r#"{"table": "00ff", "names": ["x"], "abilities": {"None": -1, "Zeta": 2, "Alpha": 0, "MainOnlyA": 100, "Mid": 99, "MainOnlyB": 111, "Beta": 1}}"#;
        assert_eq!(sub_capable_from_json(text).unwrap(), ["Alpha", "Beta", "Zeta", "Mid"]);
    }

    #[test]
    fn the_main_capable_set_is_every_enum_value_from_0_in_enum_order() {
        let text = r#"{"table": "00ff", "abilities": {"None": -1, "Zeta": 2, "Alpha": 0, "MainOnlyA": 100, "Mid": 99, "MainOnlyB": 111, "Beta": 1}}"#;
        assert_eq!(main_capable_from_json(text).unwrap(), ["Alpha", "Beta", "Zeta", "Mid", "MainOnlyA", "MainOnlyB"]);
        assert!(main_capable_from_json(r#"{"abilities": {"None": -1}}"#).is_none(), "the empty value alone is no set");
        assert!(main_capable_from_json(r#"{"abilities": {"bad name": 100}}"#).is_none());
    }

    #[test]
    fn a_file_without_the_enum_gives_nothing() {
        assert!(sub_capable_from_json(r#"{"table": "00"}"#).is_none());
        assert!(sub_capable_from_json(r#"{"abilities": {"None": -1, "OnlyMain": 100}}"#).is_none());
        assert!(sub_capable_from_json("not json").is_none());
        assert!(sub_capable_from_json(r#"{"abilities": {"bad name": 3}}"#).is_none());
        assert!(read_sub_capable(Path::new("Z:/no/such/keys.json")).is_none());
        assert!(main_capable_from_json(r#"{"table": "00"}"#).is_none());
        assert!(read_main_capable(Path::new("Z:/no/such/keys.json")).is_none());
    }

    #[test]
    fn a_keys_file_on_disk_is_read_by_path() {
        let d = std::env::temp_dir().join(format!("s3l-subs-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("s3save.keys.json");
        std::fs::write(&f, r#"{"abilities": {"A": 5, "B": 100}}"#).unwrap();
        assert_eq!(read_sub_capable(&f).unwrap(), ["A"]);
        assert_eq!(read_main_capable(&f).unwrap(), ["A", "B"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
