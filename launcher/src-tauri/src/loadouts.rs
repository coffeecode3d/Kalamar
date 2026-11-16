//! Loadout presets: named snapshots of the outfit and weapon (`head`, `clothes`, `shoes`, `weapon` and the chosen sub
//! and main abilities) that the Locker can apply to the profile in one click. Appearance, name and splashtag are not part of a
//! preset.
//!
//! The presets live in `loadouts.json` beside `profile.json` in the launcher data directory:
//! `{ "schema": 1, "presets": [ { "name": "Ranked", "head": 1000, "clothes": 8031, "shoes": 4024, "weapon": 10,
//! "sub_abilities": { "head": ["NameA", null, null] }, "main_abilities": { "head": "NameB" } } ] }`. A missing, oversized or damaged file means "no presets":
//! it never blocks the Locker. Applying a preset is done by the frontend (it copies the five fields into the profile
//! being edited, which then goes through `profile_save`), so nothing here touches `profile.json`.

use crate::error::{AppError, AppResult, FieldError};
use crate::game::catalogue::Catalogue;
use crate::profile::{self, MainAbilities, Profile, SubAbilities};
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LOADOUTS_SCHEMA: u32 = 1;
pub const MAX_PRESETS: usize = 30;
pub const NAME_MAX_CHARS: usize = 24;

/// The loadout part of a profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Loadout {
    pub head: Option<i64>,
    pub clothes: Option<i64>,
    pub shoes: Option<i64>,
    pub weapon: Option<i64>,
    pub sub_abilities: SubAbilities,
    /// Added without a new schema number: a preset saved before it loads with no main ability chosen.
    pub main_abilities: MainAbilities,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    #[serde(flatten)]
    pub loadout: Loadout,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct FileModel {
    schema: u32,
    presets: Vec<Preset>,
}

impl Default for FileModel {
    fn default() -> Self {
        FileModel { schema: LOADOUTS_SCHEMA, presets: Vec::new() }
    }
}

pub fn loadouts_path(data_dir: &Path) -> PathBuf {
    data_dir.join("loadouts.json")
}

/// Trims and checks a preset name: 1..=24 characters, no control or hidden formatting characters.
pub fn validate_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name is required".into());
    }
    if n.chars().count() > NAME_MAX_CHARS {
        return Err(format!("name is limited to {NAME_MAX_CHARS} characters"));
    }
    if n.chars().any(|c| util::unsafe_char(c) || c == '<' || c == '>') {
        return Err("name contains characters that are not allowed".into());
    }
    Ok(n.to_string())
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn name_error(message: String) -> AppError {
    AppError::Validation(vec![FieldError { key: "name".into(), message }])
}

/// Loads the presets; anything unreadable gives an empty list. Entries past the limit and entries with an invalid or
/// duplicate name are dropped, so a hand-edited file cannot break the invariants.
pub fn load(data_dir: &Path) -> Vec<Preset> {
    let Some(text) = util::read_text_limited(&loadouts_path(data_dir), 1 << 20).ok().flatten() else { return Vec::new() };
    let Ok(file) = serde_json::from_str::<FileModel>(&text) else { return Vec::new() };
    let mut out: Vec<Preset> = Vec::new();
    for mut p in file.presets {
        let Ok(name) = validate_name(&p.name) else { continue };
        if out.iter().any(|o| same_name(&o.name, &name)) || out.len() >= MAX_PRESETS {
            continue;
        }
        p.name = name;
        out.push(p);
    }
    out
}

fn store(data_dir: &Path, presets: &[Preset]) -> AppResult<()> {
    let file = FileModel { schema: LOADOUTS_SCHEMA, presets: presets.to_vec() };
    let text = serde_json::to_string_pretty(&file).map_err(|e| AppError::Internal(e.to_string()))?;
    util::atomic_write(&loadouts_path(data_dir), text.as_bytes())
}

/// Checks a loadout's ids and ability names against the catalogue exactly as a profile is checked (by validating a
/// profile that carries only these fields), and returns the cleaned loadout.
pub fn validate_loadout(l: &Loadout, cat: &Catalogue, sub_capable: Option<&[String]>, main_capable: Option<&[String]>) -> AppResult<Loadout> {
    let probe = Profile {
        name: "preset".into(),
        head: l.head,
        clothes: l.clothes,
        shoes: l.shoes,
        weapon: l.weapon,
        sub_abilities: l.sub_abilities.clone(),
        main_abilities: l.main_abilities.clone(),
        ..Default::default()
    };
    let clean = profile::validate_profile(&probe, cat, sub_capable, main_capable)?;
    Ok(Loadout { head: clean.head, clothes: clean.clothes, shoes: clean.shoes, weapon: clean.weapon, sub_abilities: clean.sub_abilities, main_abilities: clean.main_abilities })
}

/// Adds a preset, or replaces the one with the same name (case-insensitive; it keeps its position and takes the new
/// spelling). The caller has already validated the loadout.
pub fn upsert(presets: &mut Vec<Preset>, name: &str, loadout: Loadout) -> AppResult<()> {
    let name = validate_name(name).map_err(name_error)?;
    if let Some(p) = presets.iter_mut().find(|p| same_name(&p.name, &name)) {
        *p = Preset { name, loadout };
        return Ok(());
    }
    if presets.len() >= MAX_PRESETS {
        return Err(AppError::InvalidInput(format!("at most {MAX_PRESETS} presets can be saved")));
    }
    presets.push(Preset { name, loadout });
    Ok(())
}

/// Renames a preset; the new name must not belong to another preset (changing only the case of its own is fine).
pub fn rename_in(presets: &mut [Preset], from: &str, to: &str) -> AppResult<()> {
    let to = validate_name(to).map_err(name_error)?;
    let Some(i) = presets.iter().position(|p| same_name(&p.name, from)) else {
        return Err(AppError::InvalidInput(format!("no preset named {from}")));
    };
    if presets.iter().enumerate().any(|(k, p)| k != i && same_name(&p.name, &to)) {
        return Err(name_error("another preset already has this name".into()));
    }
    presets[i].name = to;
    Ok(())
}

pub fn delete_in(presets: &mut Vec<Preset>, name: &str) -> AppResult<()> {
    let before = presets.len();
    presets.retain(|p| !same_name(&p.name, name));
    if presets.len() == before {
        return Err(AppError::InvalidInput(format!("no preset named {name}")));
    }
    Ok(())
}

/// Each of these loads the file, changes it, writes it back atomically and returns the new list.
pub fn save_preset(data_dir: &Path, name: &str, loadout: Loadout) -> AppResult<Vec<Preset>> {
    let mut list = load(data_dir);
    upsert(&mut list, name, loadout)?;
    store(data_dir, &list)?;
    Ok(list)
}

pub fn rename_preset(data_dir: &Path, from: &str, to: &str) -> AppResult<Vec<Preset>> {
    let mut list = load(data_dir);
    rename_in(&mut list, from, to)?;
    store(data_dir, &list)?;
    Ok(list)
}

pub fn delete_preset(data_dir: &Path, name: &str) -> AppResult<Vec<Preset>> {
    let mut list = load(data_dir);
    delete_in(&mut list, name)?;
    store(data_dir, &list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::catalogue::{Entry, neutral};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s3l-loadouts-{tag}-{}", util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn l(head: i64) -> Loadout {
        Loadout { head: Some(head), weapon: Some(10), ..Default::default() }
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(validate_name("  Ranked ").unwrap(), "Ranked");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"a".repeat(24)).is_ok());
        assert!(validate_name(&"a".repeat(25)).is_err());
        assert!(validate_name(&"\u{e9}".repeat(24)).is_ok()); // characters, not bytes
        assert!(validate_name("a\u{202E}b").is_err());
        assert!(validate_name("<b>").is_err());
    }

    #[test]
    fn missing_or_damaged_file_means_no_presets() {
        let d = tmp("none");
        assert!(load(&d).is_empty());
        std::fs::write(loadouts_path(&d), "{ not json").unwrap();
        assert!(load(&d).is_empty());
        std::fs::write(loadouts_path(&d), "[1,2]").unwrap();
        assert!(load(&d).is_empty());
    }

    #[test]
    fn save_overwrite_rename_delete_roundtrip() {
        let d = tmp("round");
        save_preset(&d, "Ranked", l(1)).unwrap();
        save_preset(&d, " Turf ", l(2)).unwrap();
        // The same name in another case overwrites in place and takes the new spelling.
        let list = save_preset(&d, "RANKED", l(3)).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "RANKED");
        assert_eq!(list[0].loadout.head, Some(3));
        assert_eq!(load(&d), list);
        // Renaming to another preset's name is refused; to its own name in another case is fine.
        assert!(rename_preset(&d, "Turf", "ranked").is_err());
        let list = rename_preset(&d, "turf", "Salmon").unwrap();
        assert_eq!(list[1].name, "Salmon");
        assert_eq!(rename_preset(&d, "Salmon", "SALMON").unwrap()[1].name, "SALMON");
        assert!(rename_preset(&d, "ghost", "x").is_err());
        let list = delete_preset(&d, "ranked").unwrap();
        assert_eq!(list.len(), 1);
        assert!(delete_preset(&d, "ranked").is_err());
        assert_eq!(load(&d).len(), 1);
        let text = std::fs::read_to_string(loadouts_path(&d)).unwrap();
        assert!(text.contains("\"schema\": 1"));
    }

    #[test]
    fn at_most_thirty_presets() {
        let mut list = Vec::new();
        for i in 0..MAX_PRESETS {
            upsert(&mut list, &format!("p{i}"), l(i as i64)).unwrap();
        }
        assert!(upsert(&mut list, "one more", l(1)).is_err());
        upsert(&mut list, "P3", l(99)).unwrap(); // overwriting still works at the limit
        assert_eq!(list.len(), MAX_PRESETS);
    }

    #[test]
    fn a_hand_edited_file_is_cleaned_on_load() {
        let d = tmp("edit");
        let text = r#"{"schema":1,"presets":[{"name":"A","head":1},{"name":"a","head":2},{"name":"  ","head":3},{"name":"B","weapon":5}]}"#;
        std::fs::write(loadouts_path(&d), text).unwrap();
        let list = load(&d);
        assert_eq!(list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(list[1].loadout.weapon, Some(5));
    }

    #[test]
    fn loadout_is_checked_against_the_catalogue() {
        let mut cat = neutral();
        cat.categories.insert("head".into(), vec![Entry { id: 1000, selectable: true, ..Default::default() }]);
        cat.categories.insert("weapon".into(), vec![Entry { id: 10, selectable: true, ..Default::default() }]);
        let ok = Loadout { head: Some(1000), weapon: Some(10), sub_abilities: SubAbilities { head: Some(vec![None, None, None]), ..Default::default() }, ..Default::default() };
        assert_eq!(validate_loadout(&ok, &cat, None, None).unwrap(), ok);
        let bad = Loadout { head: Some(7), ..ok.clone() };
        assert!(matches!(validate_loadout(&bad, &cat, None, None), Err(AppError::Validation(_))));
        let bad_subs = Loadout { sub_abilities: SubAbilities { head: Some(vec![None]), ..Default::default() }, ..ok };
        assert!(validate_loadout(&bad_subs, &cat, None, None).is_err());
    }

    #[test]
    fn a_preset_keeps_its_main_abilities_and_they_are_checked() {
        let mut cat = neutral();
        cat.categories.insert("head".into(), vec![Entry { id: 1000, selectable: true, ability: Some("Alpha".into()), ..Default::default() }, Entry { id: 1001, selectable: true, ability: Some("Beta".into()), ..Default::default() }]);
        let set = vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()];
        let ok = Loadout { head: Some(1000), main_abilities: MainAbilities { head: Some("Beta".into()), ..Default::default() }, ..Default::default() };
        assert_eq!(validate_loadout(&ok, &cat, None, Some(&set)).unwrap(), ok);
        let bad = Loadout { main_abilities: MainAbilities { head: Some("Gamma".into()), ..Default::default() }, ..ok.clone() };
        assert!(validate_loadout(&bad, &cat, None, Some(&set)).is_err(), "no head piece has it as its main ability");
        // stored and read back; a preset written before the member loads with none chosen
        let d = tmp("main");
        save_preset(&d, "Main", ok.clone()).unwrap();
        assert_eq!(load(&d)[0].loadout.main_abilities.head.as_deref(), Some("Beta"));
        std::fs::write(loadouts_path(&d), r#"{"schema":1,"presets":[{"name":"Old","head":1000,"sub_abilities":{"head":["Alpha",null,null]}}]}"#).unwrap();
        assert_eq!(load(&d)[0].loadout.main_abilities, MainAbilities::default());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
