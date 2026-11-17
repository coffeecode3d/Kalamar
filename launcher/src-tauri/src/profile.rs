//! Player profile (name + appearance + gear + weapon + splashtag), its validation against the game catalogue and its
//! conversion to the runtime's `save_profile` schema (`engine/docs/save-profile.md`).
//!
//! The chosen profile lives in `profile.json` in the launcher data directory (schema below). At launch the launcher
//! converts it ([`to_runtime`]), writes the result atomically to `<userdata>\launcher-save-profile.json` and passes
//! `TOTK_SAVE_PROFILE=<that file>`: the runtime applies it to the encrypted save while starting. What it did is read
//! back from the run log ([`parse_log`]).
//!
//! ```json
//! { "schema": 1, "name": "Kay",
//!   "style": 0, "skin": 2, "eyes": 5, "hair": 12, "eyebrows": 1, "legwear": 3,
//!   "head": 1000, "clothes": 8031, "shoes": 4024, "weapon": 10,
//!   "banner": 11001, "title": { "adjective": 142, "subject": 0 }, "badges": [1000000, null, null] }
//! ```
//!
//! Every number is the id the game stores (see `game::catalogue`); `null` means "leave unchanged / none".
//!
//! `sub_abilities` (added without a new schema number: a file without it loads with nothing chosen, and an older
//! launcher ignores it) holds, per gear part, the three sub-ability names the player chose, `null` for an empty slot:
//! `"sub_abilities": {"head": ["NameA", null, "NameA"]}`. A part that is absent was never edited and the save keeps
//! what it has; a part with three `null` is an explicit "empty", which clears what an earlier launch wrote.
//!
//! `main_abilities` (added the same way) holds, per gear part, the main ability name the player chose for the piece worn
//! there: `"main_abilities": {"head": "NameA", "clothes": null, "shoes": null}`. `null` was never chosen: the save
//! keeps the piece's main ability as it is (its own, unless an earlier launch changed it). A main slot is never empty.

use crate::error::{AppError, AppResult, FieldError};
use crate::game::catalogue::Catalogue;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROFILE_SCHEMA: u32 = 1;
/// Conservative name limit: the console game's nickname limit is 10 characters; no documentation says otherwise.
pub const NAME_MAX_CHARS: usize = 10;
pub const BADGE_SLOTS: usize = 3;
/// The gear parts that have sub-ability slots, in the profile's and the runtime's names.
pub const GEAR_PARTS: [&str; 3] = ["head", "clothes", "shoes"];
pub const SUB_SLOTS: usize = crate::game::subs::SUB_SLOTS;

/// The sub abilities the player chose for the piece worn on each part: three slots of an ability name (the game's
/// enum name) or `None` for an empty slot. A part left at `None` was never edited.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SubAbilities {
    pub head: Option<Vec<Option<String>>>,
    pub clothes: Option<Vec<Option<String>>>,
    pub shoes: Option<Vec<Option<String>>>,
}

impl SubAbilities {
    pub fn part(&self, part: &str) -> Option<&Vec<Option<String>>> {
        match part {
            "head" => self.head.as_ref(),
            "clothes" => self.clothes.as_ref(),
            "shoes" => self.shoes.as_ref(),
            _ => None,
        }
    }
}

/// The main ability the player chose for the piece worn on each part: an ability name (the game's enum name), or
/// `None` when it was never chosen (the save keeps the piece's main ability as it is).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MainAbilities {
    pub head: Option<String>,
    pub clothes: Option<String>,
    pub shoes: Option<String>,
}

impl MainAbilities {
    pub fn part(&self, part: &str) -> Option<&String> {
        match part {
            "head" => self.head.as_ref(),
            "clothes" => self.clothes.as_ref(),
            "shoes" => self.shoes.as_ref(),
            _ => None,
        }
    }

    fn set(&mut self, part: &str, v: Option<String>) {
        match part {
            "head" => self.head = v,
            "clothes" => self.clothes = v,
            _ => self.shoes = v,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Title {
    pub adjective: Option<i64>,
    pub subject: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Profile {
    pub schema: u32,
    pub name: String,
    pub style: Option<i64>,
    pub skin: Option<i64>,
    pub eyes: Option<i64>,
    pub hair: Option<i64>,
    pub eyebrows: Option<i64>,
    pub legwear: Option<i64>,
    /// Color variation of the legwear (0-based; most legwear has one).
    pub legwear_variation: Option<i64>,
    pub head: Option<i64>,
    pub clothes: Option<i64>,
    pub shoes: Option<i64>,
    pub weapon: Option<i64>,
    pub banner: Option<i64>,
    pub title: Title,
    pub badges: [Option<i64>; BADGE_SLOTS],
    pub sub_abilities: SubAbilities,
    pub main_abilities: MainAbilities,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            schema: PROFILE_SCHEMA,
            name: String::new(),
            style: None,
            skin: None,
            eyes: None,
            hair: None,
            eyebrows: None,
            legwear: None,
            legwear_variation: None,
            head: None,
            clothes: None,
            shoes: None,
            weapon: None,
            banner: None,
            title: Title::default(),
            badges: [None; BADGE_SLOTS],
            sub_abilities: SubAbilities::default(),
            main_abilities: MainAbilities::default(),
        }
    }
}

impl Profile {
    /// (catalogue category, chosen id) for every single-id field.
    fn choices(&self) -> Vec<(&'static str, &'static str, Option<i64>)> {
        vec![
            ("style", "style", self.style),
            ("skin", "skin", self.skin),
            ("eyes", "eyes", self.eyes),
            ("hair", "hair", self.hair),
            ("eyebrows", "eyebrows", self.eyebrows),
            ("legwear", "legwear", self.legwear),
            ("head", "head", self.head),
            ("clothes", "clothes", self.clothes),
            ("shoes", "shoes", self.shoes),
            ("weapon", "weapon", self.weapon),
            ("banner", "banner", self.banner),
            ("title.adjective", "adjective", self.title.adjective),
            ("title.subject", "subject", self.title.subject),
        ]
    }
}

pub fn validate_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name is required".into());
    }
    if n.chars().count() > NAME_MAX_CHARS {
        return Err(format!("name is limited to {NAME_MAX_CHARS} characters"));
    }
    if n.chars().any(|c| c.is_control() || c == '<' || c == '>') {
        return Err("name contains characters that are not allowed".into());
    }
    Ok(n.to_string())
}

fn entry<'a>(cat: &'a Catalogue, category: &str, id: i64) -> Option<&'a crate::game::catalogue::Entry> {
    cat.categories.get(category)?.iter().find(|e| e.id == id)
}

fn has(cat: &Catalogue, category: &str, id: i64) -> bool {
    cat.categories.get(category).is_some_and(|v| v.iter().any(|e| e.id == id && e.selectable))
}

/// The main abilities the game's own pieces of a part have (`Entry::ability` of the part's selectable entries): the
/// ones a main slot of that part may take. `None` when the catalogue names none (no game data).
pub fn part_main_abilities(cat: &Catalogue, part: &str) -> Option<std::collections::BTreeSet<String>> {
    let set: std::collections::BTreeSet<String> = cat.categories.get(part)?.iter().filter(|e| e.selectable).filter_map(|e| e.ability.clone()).collect();
    (!set.is_empty()).then_some(set)
}

/// `p` without what the Splatfest-only ability's withdrawal took away (`catalogue::FEST_ONLY_ABILITY`): that ability as
/// a main ability, and a worn piece that carries it. A profile saved by an earlier launcher may hold either; they are
/// dropped like the badges, so the rest of the profile still applies (the part then keeps the save's piece, whose
/// record the engine corrects).
pub fn without_fest_only(p: &Profile, cat: &Catalogue) -> Profile {
    use crate::game::catalogue::FEST_ONLY_ABILITY;
    let mut q = p.clone();
    for part in GEAR_PARTS {
        if q.main_abilities.part(part).is_some_and(|n| n.trim() == FEST_ONLY_ABILITY) {
            q.main_abilities.set(part, None);
        }
    }
    let fest = |part: &str, id: Option<i64>| id.is_some_and(|id| entry(cat, part, id).is_some_and(|e| e.unavailable.as_deref() == Some("fest_only")));
    if fest("head", q.head) {
        q.head = None;
    }
    if fest("clothes", q.clothes) {
        q.clothes = None;
    }
    if fest("shoes", q.shoes) {
        q.shoes = None;
    }
    q
}

/// Validates against the catalogue and returns the cleaned-up profile (current schema, trimmed name). `sub_capable`
/// and `main_capable`: the abilities a sub slot and a main slot can hold (`game::subs`), when the install's keys file
/// could be read; without them the names are only checked for shape. A main ability must also be one that a piece of
/// that part has in the game ([`part_main_abilities`]), as the engine requires.
pub fn validate_profile(p: &Profile, cat: &Catalogue, sub_capable: Option<&[String]>, main_capable: Option<&[String]>) -> AppResult<Profile> {
    let p = &without_fest_only(p, cat);
    let mut errors = Vec::new();
    let name = match validate_name(&p.name) {
        Ok(n) => n,
        Err(message) => {
            errors.push(FieldError { key: "name".into(), message });
            String::new()
        }
    };
    for (field, category, value) in p.choices() {
        if let Some(id) = value
            && !has(cat, category, id)
        {
            errors.push(FieldError { key: field.into(), message: format!("{id} is not a selectable {category} in the game data") });
        }
    }
    // Hair and eyebrows must belong to the style's species (the runtime refuses the whole profile otherwise).
    if let Some(style) = p.style
        && let Some(species) = entry(cat, "style", style).and_then(|e| e.species.as_deref())
    {
        for (field, category, value) in [("hair", "hair", p.hair), ("eyebrows", "eyebrows", p.eyebrows)] {
            if let Some(id) = value
                && let Some(e) = entry(cat, category, id)
                && e.species.as_deref().is_some_and(|s| s != species)
            {
                errors.push(FieldError { key: field.into(), message: format!("{id} is a {} look but the style is {species}", e.species.as_deref().unwrap_or("?")) });
            }
        }
    }
    if let (Some(v), Some(leg)) = (p.legwear_variation, p.legwear) {
        let max = entry(cat, "legwear", leg).and_then(|e| e.variations).unwrap_or(1).max(1);
        if v < 0 || v >= max {
            errors.push(FieldError { key: "legwear_variation".into(), message: format!("variation {v} does not exist (0..{})", max - 1) });
        }
    }
    let mut sub_abilities = SubAbilities::default();
    for part in GEAR_PARTS {
        let Some(slots) = p.sub_abilities.part(part) else { continue };
        if slots.len() != SUB_SLOTS {
            errors.push(FieldError { key: format!("sub_abilities.{part}"), message: format!("a piece has exactly {SUB_SLOTS} sub-ability slots, not {}", slots.len()) });
            continue;
        }
        let mut clean = Vec::with_capacity(SUB_SLOTS);
        for (k, slot) in slots.iter().enumerate() {
            let name = slot.as_deref().map(str::trim).filter(|n| !n.is_empty());
            match name {
                Some(n) if !crate::game::subs::valid_name(n) => {
                    errors.push(FieldError { key: format!("sub_abilities.{part}.{k}"), message: "not an ability name".into() });
                }
                Some(n) if sub_capable.is_some_and(|set| !set.iter().any(|s| s == n)) => {
                    errors.push(FieldError { key: format!("sub_abilities.{part}.{k}"), message: format!("{n} is not an ability a sub slot can hold") });
                }
                _ => {}
            }
            clean.push(name.map(str::to_string));
        }
        match part {
            "head" => sub_abilities.head = Some(clean),
            "clothes" => sub_abilities.clothes = Some(clean),
            _ => sub_abilities.shoes = Some(clean),
        }
    }
    let mut main_abilities = MainAbilities::default();
    for part in GEAR_PARTS {
        let Some(n) = p.main_abilities.part(part).map(|n| n.trim()).filter(|n| !n.is_empty()) else { continue };
        let key = format!("main_abilities.{part}");
        if !crate::game::subs::valid_name(n) {
            errors.push(FieldError { key, message: "not an ability name".into() });
        } else if main_capable.is_some_and(|set| !set.iter().any(|s| s == n)) {
            errors.push(FieldError { key, message: format!("{n} is not an ability a main slot can hold") });
        } else if part_main_abilities(cat, part).is_some_and(|set| !set.contains(n)) {
            errors.push(FieldError { key, message: format!("{n} is not the main ability of any {part} piece of the game") });
        } else {
            main_abilities.set(part, Some(n.to_string()));
        }
    }
    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    // Badges cannot be applied (see `game::catalogue::apply_save_constraints`): an old selection is dropped.
    Ok(Profile { schema: PROFILE_SCHEMA, name, badges: [None; BADGE_SLOTS], sub_abilities, main_abilities, ..p.clone() })
}

pub fn profile_path(data_dir: &Path) -> PathBuf {
    data_dir.join("profile.json")
}

/// Loads `profile.json`. A file from an older launcher (string ids) or a damaged one only keeps its name.
pub fn load_profile(data_dir: &Path) -> Profile {
    let Some(text) = util::read_text_limited(&profile_path(data_dir), 1 << 20).ok().flatten() else {
        return Profile::default();
    };
    if let Ok(p) = serde_json::from_str::<Profile>(&text) {
        return p;
    }
    let name = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("name")?.as_str().map(str::to_string)).unwrap_or_default();
    Profile { name, ..Default::default() }
}

pub fn save_profile(data_dir: &Path, p: &Profile) -> AppResult<()> {
    let text = serde_json::to_string_pretty(p).map_err(|e| AppError::Internal(e.to_string()))?;
    util::atomic_write(&profile_path(data_dir), text.as_bytes())
}

// ---- conversion to the runtime schema -------------------------------------------------------------------------

/// The four looks, by launcher style id. The launcher's ids are the order of the creation screen's four "kind" buttons
/// (`L_BtnKind_00..03`: 0 and 1 under the Inkling heading, 2 and 3 under the Octoling one, by pane position), and the
/// save's `Coordinates/ModelType` is main's `spl::PlayerModelType` enum "SquidF, SquidM, OctopusF, OctopusM" = 0..3, so
/// launcher style id == save value == index into this table.
pub const STYLE_NAMES: [&str; 4] = ["inkling_f", "inkling_m", "octoling_f", "octoling_m"];

pub fn style_name(id: i64) -> Option<&'static str> {
    usize::try_from(id).ok().and_then(|i| STYLE_NAMES.get(i)).copied()
}

#[cfg(test)]
pub fn style_id(name: &str) -> Option<i64> {
    STYLE_NAMES.iter().position(|n| *n == name).map(|i| i as i64)
}

/// What the user chose to do at launch (stored in `config.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyOptions {
    /// `skip_first_launch`: the game boots straight to the hub instead of the intro, character creation and tutorial.
    pub skip_first_launch: bool,
    /// `unlock_all`: the save owns every weapon, gear, banner, title word and emote.
    pub unlock_all: bool,
    /// `chunks: "max"`: a full stock of ability chunks (every ability a sub slot can hold). Off by default.
    pub unlock_chunks: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        ApplyOptions { skip_first_launch: true, unlock_all: true, unlock_chunks: false }
    }
}

/// The runtime applies appearance as one block (style, hair, eyebrows, legwear, skin, eyes): a profile without one of
/// them is refused whole, so the launcher does not pass it and says which fields are missing.
pub fn missing_appearance(p: &Profile) -> Vec<&'static str> {
    [("style", p.style), ("hair", p.hair), ("eyebrows", p.eyebrows), ("legwear", p.legwear), ("skin", p.skin), ("eyes", p.eyes)]
        .into_iter()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// Fields the runtime needs and the profile leaves empty.
    Incomplete(Vec<&'static str>),
    /// A value outside what the runtime accepts (`key`, reason).
    Invalid(&'static str, String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Incomplete(k) => write!(f, "missing: {}", k.join(", ")),
            ConvertError::Invalid(k, why) => write!(f, "{k}: {why}"),
        }
    }
}

/// Pure mapping from the launcher profile to the runtime's `profile.json`. `null` splashtag, gear, weapon and (with an
/// existing save) appearance fields are left out: "unchanged" in the save. Badges are never sent (the save's badge
/// record is unknown). A **new** save (`save_exists` false) needs the whole appearance, else
/// [`ConvertError::Incomplete`] names what is missing; with a save every appearance field is optional.
pub fn to_runtime(p: &Profile, opts: ApplyOptions, save_exists: bool) -> Result<serde_json::Value, ConvertError> {
    use crate::game::catalogue::{DROPPED_GEAR, EYES_MAX, SKIN_MAX};
    use serde_json::{Map, Value, json};
    let missing = missing_appearance(p);
    if !save_exists && !missing.is_empty() {
        return Err(ConvertError::Incomplete(missing));
    }
    let complete = missing.is_empty();
    let name = validate_name(&p.name).map_err(|m| ConvertError::Invalid("name", m))?;
    if let Some(skin) = p.skin
        && !(0..=SKIN_MAX).contains(&skin)
    {
        return Err(ConvertError::Invalid("skin", format!("{skin} is outside 0..{SKIN_MAX}")));
    }
    if let Some(eyes) = p.eyes
        && !(0..=EYES_MAX).contains(&eyes)
    {
        return Err(ConvertError::Invalid("eyes", format!("{eyes} is outside 0..{EYES_MAX}")));
    }
    if p.legwear_variation.is_some_and(|v| v < 0) {
        return Err(ConvertError::Invalid("legwear_variation", "negative".into()));
    }
    let mut o = Map::new();
    o.insert("version".into(), json!(1));
    o.insert("name".into(), json!(name));
    if let Some(id) = p.style {
        let style = style_name(id).ok_or_else(|| ConvertError::Invalid("style", format!("{id} is not one of the four styles")))?;
        o.insert("style".into(), json!(style));
    }
    for (key, v) in [("hair", p.hair), ("eyebrows", p.eyebrows), ("legwear", p.legwear), ("skin", p.skin), ("eyes", p.eyes)] {
        if let Some(id) = v {
            o.insert(key.into(), json!(id));
        }
    }
    // A complete character always gets a variation (0 by default); a partial one leaves the save's own.
    match (p.legwear_variation, complete) {
        (Some(v), _) => {
            o.insert("legwear_variation".into(), json!(v));
        }
        (None, true) => {
            o.insert("legwear_variation".into(), json!(0));
        }
        (None, false) => {}
    }
    // The runtime counts an absent `skip_first_launch` as true only with the whole appearance, so a partial profile
    // never ends the first launch of a save that is still in it; asking for "no" is always sent.
    if complete || !opts.skip_first_launch {
        o.insert("skip_first_launch".into(), json!(opts.skip_first_launch));
    }
    o.insert("unlock_all".into(), json!(opts.unlock_all));

    let mut tag = Map::new();
    for (key, v) in [("banner", p.banner), ("adjective", p.title.adjective), ("subject", p.title.subject)] {
        if let Some(id) = v {
            tag.insert(key.into(), json!(id));
        }
    }
    if !tag.is_empty() {
        o.insert("splashtag".into(), Value::Object(tag));
    }
    let mut gear = Map::new();
    for (key, v) in [("head", p.head), ("clothes", p.clothes), ("shoes", p.shoes)] {
        if let Some(id) = v {
            if DROPPED_GEAR.contains(&(key, id)) {
                return Err(ConvertError::Invalid(key, format!("{id} is dropped by the game after loading the save")));
            }
            gear.insert(key.into(), json!(id));
        }
    }
    if !gear.is_empty() {
        o.insert("gear".into(), Value::Object(gear));
    }
    if let Some(w) = p.weapon {
        o.insert("weapon".into(), json!(w));
    }
    // Sub abilities go to the piece chosen for the part. A part never edited is left out (the save keeps its own); one
    // edited to "empty" is sent as three `null`, which the runtime writes as three empty slots (that is how an earlier
    // launch's abilities are cleared). A part without a chosen piece is left out too: the runtime would apply it to
    // whatever the save wears, and the player did not choose that piece here.
    let mut subs = Map::new();
    for (part, piece) in [("head", p.head), ("clothes", p.clothes), ("shoes", p.shoes)] {
        let (Some(_), Some(slots)) = (piece, p.sub_abilities.part(part)) else { continue };
        if slots.len() != SUB_SLOTS {
            return Err(ConvertError::Invalid("sub_abilities", format!("{part}: {SUB_SLOTS} slots expected, {} given", slots.len())));
        }
        if slots.iter().flatten().any(|n| !crate::game::subs::valid_name(n)) {
            return Err(ConvertError::Invalid("sub_abilities", format!("{part}: not an ability name")));
        }
        subs.insert(part.into(), json!(slots));
    }
    if !subs.is_empty() {
        o.insert("gear_abilities".into(), Value::Object(subs));
    }
    // A main ability goes to the piece chosen for the part, like the sub abilities; a part never chosen is left out
    // (the save keeps the piece's main ability as it is).
    let mut mains = Map::new();
    for (part, piece) in [("head", p.head), ("clothes", p.clothes), ("shoes", p.shoes)] {
        let (Some(_), Some(name)) = (piece, p.main_abilities.part(part)) else { continue };
        // A choice made before the Splatfest-only ability was withdrawn: left out, the engine gives the piece its own.
        if name == crate::game::catalogue::FEST_ONLY_ABILITY {
            continue;
        }
        if !crate::game::subs::valid_name(name) {
            return Err(ConvertError::Invalid("main_abilities", format!("{part}: not an ability name")));
        }
        mains.insert(part.into(), json!(name));
    }
    if !mains.is_empty() {
        o.insert("main_abilities".into(), Value::Object(mains));
    }
    if opts.unlock_chunks {
        o.insert("chunks".into(), json!("max"));
    }
    Ok(Value::Object(o))
}

/// Whether the user data directory already holds a save: `<profile dir>\save\<uid>\save.dat` (the runtime keeps one
/// folder per account). The runtime's own rule is the same file; a commit still waiting in `save-tx` is not counted.
pub fn save_exists(profile_dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(profile_dir.join("save")) else { return false };
    rd.flatten().any(|e| e.path().join("save.dat").is_file())
}

/// File the runtime profile is written to, next to the user data of the game root.
pub fn runtime_profile_path(userdata: &Path) -> PathBuf {
    userdata.join("launcher-save-profile.json")
}

/// Environment of a launch that applies a profile: the file to apply and the player name (`player_name` /
/// `SPLAT3_NICKNAME`, the nickname the game shows).
pub fn profile_env(file: &Path, name: &str) -> Vec<(String, String)> {
    let mut env = vec![("TOTK_SAVE_PROFILE".to_string(), file.display().to_string())];
    let n = name.trim();
    if !n.is_empty() {
        env.push(("TOTK_PLAYER_NAME".into(), n.to_string()));
        env.push(("SPLAT3_NICKNAME".into(), n.to_string()));
    }
    env
}

/// Converts, writes the runtime profile atomically and returns the environment to add to the launch.
pub fn prepare_launch(p: &Profile, opts: ApplyOptions, userdata: &Path, save_exists: bool) -> Result<Vec<(String, String)>, ConvertError> {
    let value = to_runtime(p, opts, save_exists)?;
    let file = runtime_profile_path(userdata);
    let text = serde_json::to_string_pretty(&value).map_err(|e| ConvertError::Invalid("profile", e.to_string()))?;
    util::atomic_write(&file, text.as_bytes()).map_err(|e| ConvertError::Invalid("profile", e.to_string()))?;
    Ok(profile_env(&file, &p.name))
}

// ---- reading back what the runtime did -----------------------------------------------------------------------

/// A refusal of the runtime, mapped to the profile field it is about so the UI can say it next to that field.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Refusal {
    /// Profile field key (`hair`, `eyebrows`, `style`, `legwear`, `legwear_variation`, `skin`, `eyes`, `weapon`, `gear`
    /// with the id, `splashtag`, `badges`, `appearance`) or empty when it is about the save itself.
    pub field: String,
    /// Stable code the UI translates (`species`, `unselectable`, `gear_dropped`, ...); `other` keeps only `raw`.
    pub code: String,
    /// The id the runtime names, when it names one.
    pub id: Option<i64>,
    /// `species`: the species of the item (`inkling` or `octoling`).
    pub species: Option<String>,
    /// The style the runtime checked against (`inkling_f`...).
    pub style: Option<String>,
    /// The offending value is the save's own (a partial profile changed the style but not the hair): give a matching one.
    pub from_save: bool,
    /// The runtime's sentence, untranslated and unmodified.
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Feedback {
    /// `applied` (the save was changed, backup made), `already` (0 changes: nothing written, no backup), `new_save`
    /// (no save yet: the profile goes into the one the game creates, then it restarts), `restarting`,
    /// `needs_appearance` (new save without the whole appearance: not applied), `failed` (refused or not written),
    /// `none` (no `save profile:` line in the log).
    pub state: String,
    pub changes: Option<u32>,
    pub backup: Option<String>,
    /// The runtime's reason when `failed`.
    pub reason: Option<String>,
    pub refusal: Option<Refusal>,
    /// `needs_appearance`: the fields the profile lacks.
    pub missing: Vec<String>,
    /// Backups the runtime deleted after this launch (`deleted N older backups`), and how many it keeps.
    pub deleted_backups: Option<u32>,
    pub keep_backups: Option<u32>,
    /// The `save profile:` lines themselves, shortened.
    pub lines: Vec<String>,
}

fn first_int(s: &str) -> Option<i64> {
    let digits: String = s.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    digits.parse().ok()
}

/// The id in parentheses of a runtime sentence: `fail(why, id)` prints `why (id); the save is left as it is`.
fn trailing_id(msg: &str) -> Option<i64> {
    let end = msg.rfind("); the save is left")?;
    let start = msg[..end].rfind(" (")?;
    msg[start + 2..end].parse().ok()
}

/// Maps one refusal sentence of `runtime/save_profile.c` (`fail()` and `check_species()`) to a [`Refusal`].
pub fn classify_refusal(msg: &str) -> Refusal {
    let mk = |field: &str, code: &str| Refusal { field: field.into(), code: code.into(), id: trailing_id(msg), raw: msg.to_string(), ..Default::default() };
    for what in ["hair", "eyebrows"] {
        if let Some(rest) = msg.strip_prefix(what).and_then(|r| r.strip_prefix(' ')) {
            let id = first_int(rest);
            let style = rest.split("style ").nth(1).map(|s| s.split([' ', ';', '(']).next().unwrap_or("").to_string()).filter(|s| !s.is_empty());
            let (code, species) = if rest.contains("belongs to Octolings") {
                ("species", Some("octoling".to_string()))
            } else if rest.contains("belongs to Inklings") {
                ("species", Some("inkling".to_string()))
            } else {
                ("unselectable", None)
            };
            return Refusal { field: what.into(), code: code.into(), id, species, style, from_save: msg.contains("it is the save's value"), raw: msg.to_string() };
        }
    }
    if let Some(rest) = msg.strip_prefix("gear_abilities") {
        // `gear_abilities.<part>[<slot>]: ...`, `gear_abilities.<part> must be ...`, `gear_abilities has an unknown part`,
        // `gear_abilities must be an object`, `gear_abilities: the save does not own the equipped gear (...)`.
        let part = rest.strip_prefix('.').and_then(|r| r.split(['[', ' ', ':']).next()).filter(|p| GEAR_PARTS.contains(p));
        let field = part.map_or_else(|| "sub_abilities".to_string(), |p| format!("sub_abilities.{p}"));
        let code = if rest.contains("one only a main slot has") {
            "sub_main_only"
        } else if rest.contains("has no ability named") || rest.contains("is not a value of the game's ability enum") {
            "sub_unknown"
        } else if rest.contains("does not own the equipped gear") {
            "sub_not_owned"
        } else {
            "sub_invalid"
        };
        return Refusal { field, code: code.into(), id: trailing_id(msg), raw: msg.to_string(), ..Default::default() };
    }
    if let Some(rest) = msg.strip_prefix("main_abilities") {
        // `main_abilities.<part>: ...`, `main_abilities.<part> must be ...`, `main_abilities has an unknown part`,
        // `main_abilities must be an object`, `main_abilities: the save does not own the equipped gear (...)`.
        let part = rest.strip_prefix('.').and_then(|r| r.split([' ', ':']).next()).filter(|p| GEAR_PARTS.contains(p));
        let field = part.map_or_else(|| "main_abilities".to_string(), |p| format!("main_abilities.{p}"));
        let code = if rest.contains("is not the main ability of any") {
            "main_part"
        } else if rest.contains("has no ability named") || rest.contains("is not a value of the game's ability enum") {
            "main_unknown"
        } else if rest.contains("does not own the equipped gear") {
            "main_not_owned"
        } else if rest.contains("is never empty") {
            "main_empty"
        } else {
            "main_invalid"
        };
        return Refusal { field, code: code.into(), id: trailing_id(msg), raw: msg.to_string(), ..Default::default() };
    }
    if msg.starts_with("chunks must be") {
        return mk("chunks", "chunks");
    }
    if msg.starts_with("the ability names were not found") {
        return mk("", "ability_names");
    }
    let table: [(&str, &str, &str); 14] = [
        ("style must be", "style", "style"),
        ("hair, eyebrows, legwear", "appearance", "integers"),
        ("legwear is not a selectable", "legwear", "legwear"),
        ("legwear_variation out of range", "legwear_variation", "legwear_variation"),
        ("skin must be", "skin", "skin_range"),
        ("eyes must be", "eyes", "eyes_range"),
        ("weapon is not a Versus", "weapon", "weapon"),
        ("weapon must be", "weapon", "weapon"),
        ("the game removes this gear", "gear", "gear_dropped"),
        ("gear id is not a player gear", "gear", "gear_unknown"),
        ("gear ids must be", "gear", "gear_unknown"),
        ("splashtag id is not in", "splashtag", "splashtag"),
        ("splashtag ids must be", "splashtag", "splashtag"),
        ("badge not owned", "badges", "badge"),
    ];
    for (prefix, field, code) in table {
        if msg.starts_with(prefix) {
            return mk(field, code);
        }
    }
    if msg.starts_with("the save has no") || msg.contains("ownership maps are missing") || (msg.contains("does not hold the ") && msg.contains(" layout")) {
        return mk("", "save_layout");
    }
    if msg.contains("does not pass its check") {
        return mk("", "save_check");
    }
    mk("", "other")
}

/// Reads the `save profile:` lines of a run log (`rt_log` output). Formats: `runtime/save_profile.c` and
/// `docs/save-profile.md`.
pub fn parse_log(text: &str) -> Feedback {
    let mut f = Feedback { state: "none".into(), ..Default::default() };
    let (mut applied, mut already, mut new_save, mut restarting, mut needs) = (false, false, false, false, false);
    let mut changes = 0u32;
    for line in text.lines() {
        let Some(at) = line.find("save profile: ") else { continue };
        let msg = &line[at + "save profile: ".len()..];
        f.lines.push(line[at..].chars().take(400).collect());
        if let Some(pos) = msg.find(" applied to ") {
            let rest = &msg[pos..];
            if rest.contains(": 0 changes (") {
                already = true;
                restarting = false; // the restarted game found the save already done
            } else {
                applied = true;
                if let Some(open) = rest.find(" (")
                    && let Some(n) = rest[open + 2..].split_whitespace().next().and_then(|n| n.parse::<u32>().ok())
                {
                    changes += n;
                }
                if let Some(b) = rest.find("backup ") {
                    let b = &rest[b + "backup ".len()..];
                    let end = b.find("):").or_else(|| b.find(')')).unwrap_or(b.len());
                    f.backup = Some(b[..end].to_string());
                }
            }
        } else if msg.contains(" already matches ") {
            already = true;
            restarting = false;
        } else if let Some(rest) = msg.strip_prefix("deleted ") {
            f.deleted_backups = first_int(rest).and_then(|n| u32::try_from(n).ok());
            f.keep_backups = rest.split("newest ").nth(1).and_then(first_int).and_then(|n| u32::try_from(n).ok());
        } else if msg.starts_with("no save yet, and a new save needs the whole appearance") {
            needs = true;
            f.missing = msg
                .split("lacks ")
                .nth(1)
                .map(|r| r.split(';').next().unwrap_or("").split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                .unwrap_or_default();
        } else if msg.starts_with("no save yet") {
            new_save = true;
        } else if msg.starts_with("restarting") {
            restarting = true;
        } else if (msg.contains("left as it is") || msg.contains("profile off") || msg.contains("cannot write") || msg.contains("cannot back")) && f.reason.is_none() {
            f.reason = Some(msg.to_string());
            f.refusal = Some(classify_refusal(msg));
        }
    }
    if applied {
        f.changes = Some(changes);
    }
    f.state = if f.reason.is_some() && !applied {
        "failed"
    } else if restarting {
        "restarting"
    } else if applied {
        "applied"
    } else if already {
        "already"
    } else if needs {
        "needs_appearance"
    } else if new_save {
        "new_save"
    } else {
        "none"
    }
    .into();
    if already && !applied {
        f.changes = Some(0);
    }
    f
}

/// The backups the runtime keeps of the save before it changes it: `<profile dir>\save-profile-backup\yyyymmdd-hhmmss`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct BackupInfo {
    pub dir: String,
    pub exists: bool,
    pub count: usize,
    /// Name of the newest folder (`yyyymmdd-hhmmss`).
    pub newest: Option<String>,
}

fn is_backup_name(n: &str) -> bool {
    n.len() == 15 && n.as_bytes()[8] == b'-' && n.bytes().enumerate().all(|(i, b)| i == 8 || b.is_ascii_digit())
}

pub fn backup_info(profile_dir: &Path) -> BackupInfo {
    let dir = profile_dir.join("save-profile-backup");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| is_backup_name(n)).collect())
        .unwrap_or_default();
    names.sort();
    BackupInfo { dir: dir.display().to_string(), exists: dir.is_dir(), count: names.len(), newest: names.last().cloned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::catalogue::{Entry, neutral};

    fn cat_with_gear() -> Catalogue {
        let mut c = neutral();
        c.categories.insert("head".into(), vec![Entry { id: 1000, selectable: true, ..Default::default() }, Entry { id: 20001, selectable: false, ..Default::default() }]);
        c.categories.insert("badge".into(), vec![Entry { id: 7, selectable: true, ..Default::default() }, Entry { id: 8, selectable: true, ..Default::default() }]);
        c
    }

    #[test]
    fn name_rules() {
        assert_eq!(validate_name("  Zoé ").unwrap(), "Zoé");
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("12345678901").is_err());
        assert!(validate_name("1234567890").is_ok());
        assert!(validate_name("ééééééééé").is_ok()); // counted in characters, not bytes
        assert!(validate_name("a\u{7}b").is_err());
        assert!(validate_name("<b>").is_err());
    }

    #[test]
    fn profile_validation_reports_each_field() {
        let cat = cat_with_gear();
        let ok = Profile { name: "Kay".into(), style: Some(1), hair: Some(12), head: Some(1000), badges: [Some(7), Some(8), None], ..Default::default() };
        assert!(validate_profile(&ok, &cat, None, None).unwrap().badges.iter().all(Option::is_none)); // badges are dropped
        let bad = Profile { name: "".into(), hair: Some(99), head: Some(20001), shoes: Some(5), badges: [Some(7), Some(7), Some(404)], title: Title { adjective: Some(1), subject: None }, ..Default::default() };
        match validate_profile(&bad, &cat, None, None).unwrap_err() {
            AppError::Validation(v) => assert_eq!(
                v.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
                vec!["name", "hair", "head", "shoes", "title.adjective"]
            ),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn json_shape_matches_the_documented_schema() {
        let p = Profile { name: "Kay".into(), style: Some(0), badges: [Some(1), None, None], title: Title { adjective: Some(142), subject: Some(0) }, ..Default::default() };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["title"]["adjective"], 142);
        assert_eq!(v["badges"], serde_json::json!([1, null, null]));
        assert!(v["weapon"].is_null());
        let back: Profile = serde_json::from_value(v).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn old_or_broken_files_only_keep_the_name() {
        let d = std::env::temp_dir().join(format!("s3l-prof-{}", util::random_hex(4)));
        assert_eq!(load_profile(&d), Profile::default());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(profile_path(&d), r#"{"name":"Old","style":"style_1","hairstyle":"hair_2"}"#).unwrap();
        let p = load_profile(&d);
        assert_eq!(p.name, "Old");
        assert_eq!(p.style, None);
        std::fs::write(profile_path(&d), "{{").unwrap();
        assert_eq!(load_profile(&d).name, "");
        let p = Profile { name: "Ink".into(), legwear: Some(2), ..Default::default() };
        save_profile(&d, &p).unwrap();
        assert_eq!(load_profile(&d), p);
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn full() -> Profile {
        Profile {
            name: "Kay".into(), style: Some(2), skin: Some(2), eyes: Some(18), hair: Some(33), eyebrows: Some(21), legwear: Some(1), legwear_variation: Some(2),
            head: Some(1018), clothes: Some(5001), shoes: Some(6003), weapon: Some(50), banner: Some(11020),
            title: Title { adjective: Some(200), subject: Some(300) }, badges: [Some(5), None, None], ..Default::default()
        }
    }

    #[test]
    fn style_ids_match_the_save_enum() {
        // ModelType: SquidF 0, SquidM 1, OctopusF 2, OctopusM 3 (docs/save-profile.md); launcher id == save value.
        assert_eq!(style_name(0), Some("inkling_f"));
        assert_eq!(style_name(1), Some("inkling_m"));
        assert_eq!(style_name(2), Some("octoling_f"));
        assert_eq!(style_name(3), Some("octoling_m"));
        assert_eq!(style_name(4), None);
        assert_eq!(style_name(-1), None);
        for (i, n) in STYLE_NAMES.iter().enumerate() {
            assert_eq!(style_id(n), Some(i as i64));
        }
        assert_eq!(style_id("nope"), None);
    }

    #[test]
    fn full_profile_maps_to_the_example_shape() {
        let v = to_runtime(&full(), ApplyOptions { skip_first_launch: true, unlock_all: false, unlock_chunks: false }, false).unwrap();
        // tools/profile/profile.example.json, field for field
        assert_eq!(
            v,
            serde_json::json!({
                "version": 1, "name": "Kay", "style": "octoling_f", "hair": 33, "eyebrows": 21, "legwear": 1, "legwear_variation": 2,
                "skin": 2, "eyes": 18, "skip_first_launch": true, "unlock_all": false,
                "splashtag": {"banner": 11020, "adjective": 200, "subject": 300},
                "gear": {"head": 1018, "clothes": 5001, "shoes": 6003}, "weapon": 50
            })
        );
        assert!(v["splashtag"].get("badges").is_none()); // badges are never sent
    }

    #[test]
    fn nulls_are_left_out_and_appearance_is_all_or_nothing() {
        let mut p = full();
        p.head = None;
        p.weapon = None;
        p.banner = None;
        p.title = Title::default();
        p.legwear_variation = None;
        let v = to_runtime(&p, ApplyOptions::default(), false).unwrap();
        assert!(v.get("splashtag").is_none() && v.get("weapon").is_none());
        assert_eq!(v["gear"], serde_json::json!({"clothes": 5001, "shoes": 6003}));
        assert_eq!(v["legwear_variation"], 0);
        assert_eq!(v["unlock_all"], true);
        p.clothes = None;
        p.shoes = None;
        assert!(to_runtime(&p, ApplyOptions::default(), false).unwrap().get("gear").is_none());
        let mut q = full();
        q.hair = None;
        q.skin = None;
        assert_eq!(to_runtime(&q, ApplyOptions::default(), false).unwrap_err(), ConvertError::Incomplete(vec!["hair", "skin"]));
        assert_eq!(missing_appearance(&Profile::default()).len(), 6);
    }

    #[test]
    fn values_the_runtime_would_refuse_are_refused_here() {
        let mut p = full();
        p.skin = Some(9);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), false), Err(ConvertError::Invalid("skin", _))));
        let mut p = full();
        p.eyes = Some(21);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), false), Err(ConvertError::Invalid("eyes", _))));
        let mut p = full();
        p.style = Some(7);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), false), Err(ConvertError::Invalid("style", _))));
        let mut p = full();
        p.shoes = Some(3009);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), false), Err(ConvertError::Invalid("shoes", _))));
        let mut p = full();
        p.name = "".into();
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), false), Err(ConvertError::Invalid("name", _))));
    }

    #[test]
    fn species_and_variation_are_validated_against_the_catalogue() {
        let mut cat = cat_with_gear();
        let mk = |id, species: &str, variations| Entry { id, selectable: true, species: Some(species.into()), variations, ..Default::default() };
        cat.categories.insert("style".into(), vec![mk(0, "inkling", None), mk(2, "octoling", None)]);
        cat.categories.insert("hair".into(), vec![mk(3, "inkling", None), mk(33, "octoling", None)]);
        cat.categories.insert("legwear".into(), vec![mk(1, "", Some(3))]);
        let ok = Profile { name: "K".into(), style: Some(2), hair: Some(33), legwear: Some(1), legwear_variation: Some(2), ..Default::default() };
        assert!(validate_profile(&ok, &cat, None, None).is_ok());
        let bad = Profile { hair: Some(3), legwear_variation: Some(3), ..ok.clone() };
        match validate_profile(&bad, &cat, None, None).unwrap_err() {
            AppError::Validation(v) => assert_eq!(v.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["hair", "legwear_variation"]),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn env_and_file_for_a_launch() {
        let env = profile_env(Path::new("C:/g/userdata/launcher-save-profile.json"), " Kay ");
        assert_eq!(env[0], ("TOTK_SAVE_PROFILE".to_string(), "C:/g/userdata/launcher-save-profile.json".to_string()));
        assert_eq!(env[1], ("TOTK_PLAYER_NAME".to_string(), "Kay".to_string()));
        assert_eq!(env[2], ("SPLAT3_NICKNAME".to_string(), "Kay".to_string()));
        assert_eq!(profile_env(Path::new("p.json"), "  ").len(), 1);
        let d = std::env::temp_dir().join(format!("s3l-prep-{}", util::random_hex(4)));
        let env = prepare_launch(&full(), ApplyOptions::default(), &d, false).unwrap();
        let text = std::fs::read_to_string(runtime_profile_path(&d)).unwrap();
        assert!(text.contains("\"octoling_f\""));
        assert_eq!(env[0].1, runtime_profile_path(&d).display().to_string());
        assert!(prepare_launch(&Profile::default(), ApplyOptions::default(), &d, false).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn log_lines_of_every_outcome() {
        let applied = "[host] save profile: C:/u/p.json applied to C:/u/profile/save/x/save.dat (14 changes, backup C:/u/profile/save-profile-backup/20261110-040000):\n   ModelType=2";
        let f = parse_log(applied);
        assert_eq!((f.state.as_str(), f.changes), ("applied", Some(14)));
        assert_eq!(f.backup.as_deref(), Some("C:/u/profile/save-profile-backup/20261110-040000"));
        assert_eq!(parse_log("save profile: C:/p.json already matches C:/s/save.dat").state, "already");
        let zero = parse_log("[host] save profile: C:/u/p.json applied to C:/u/profile/save/x/save.dat: 0 changes (the save already matches; nothing written, no backup)");
        assert_eq!((zero.state.as_str(), zero.changes, zero.backup), ("already", Some(0), None));
        let first = "save profile: no save yet; the profile goes into the save the game creates, then the game restarts";
        assert_eq!(parse_log(first).state, "new_save");
        let both = format!("{first}\nsave profile: C:/p applied to C:/s (30 changes, backup C:/b):\nsave profile: restarting so the game loads the new save");
        let f = parse_log(&both);
        assert_eq!((f.state.as_str(), f.changes), ("restarting", Some(30)));
        let after = format!("{both}
save profile: C:/p already matches C:/s");
        assert_eq!(parse_log(&after).state, "applied"); // the restarted process confirmed it
        let bad = parse_log("save profile: hair is not a hairstyle of that style (3); the save is left as it is");
        assert_eq!(bad.state, "failed");
        assert!(bad.reason.unwrap().contains("hairstyle"));
        assert_eq!(parse_log("[host] nothing here").state, "none");
        assert!(parse_log("save profile: cannot read C:/x.json; the save is left as it is").reason.is_some());
    }

    #[test]
    fn partial_profiles_are_accepted_only_on_an_existing_save() {
        let weapon_only = Profile { name: "Kay".into(), weapon: Some(50), ..Default::default() };
        // new save: refused, naming the six fields
        assert_eq!(
            to_runtime(&weapon_only, ApplyOptions::default(), false).unwrap_err(),
            ConvertError::Incomplete(vec!["style", "hair", "eyebrows", "legwear", "skin", "eyes"])
        );
        // existing save: only what the profile sets; no skip_first_launch=true for a partial profile (the runtime
        // would count it as ending the first launch), no appearance, no variation
        let v = to_runtime(&weapon_only, ApplyOptions::default(), true).unwrap();
        assert_eq!(v, serde_json::json!({"version": 1, "name": "Kay", "unlock_all": true, "weapon": 50}));
        // asking for "no" is always sent
        let v = to_runtime(&weapon_only, ApplyOptions { skip_first_launch: false, unlock_all: false, unlock_chunks: false }, true).unwrap();
        assert_eq!(v["skip_first_launch"], false);
        // one appearance field alone is fine on an existing save, and out-of-range values are still refused
        let eyes = Profile { name: "Kay".into(), eyes: Some(4), ..Default::default() };
        assert_eq!(to_runtime(&eyes, ApplyOptions::default(), true).unwrap()["eyes"], 4);
        let bad = Profile { eyes: Some(40), ..eyes };
        assert!(matches!(to_runtime(&bad, ApplyOptions::default(), true), Err(ConvertError::Invalid("eyes", _))));
        // a complete profile behaves as before on an existing save
        let v = to_runtime(&full(), ApplyOptions::default(), true).unwrap();
        assert_eq!(v["skip_first_launch"], true);
        assert_eq!(v["legwear_variation"], 2);
    }

    #[test]
    fn save_detection_looks_for_save_dat_under_save_uid() {
        let d = std::env::temp_dir().join(format!("s3l-save-{}", util::random_hex(4)));
        assert!(!save_exists(&d));
        std::fs::create_dir_all(d.join("save").join("14661551906a0791439812cc30d2ab74")).unwrap();
        assert!(!save_exists(&d)); // an empty account folder is no save
        std::fs::write(d.join("save").join("14661551906a0791439812cc30d2ab74").join("save.dat"), b"x").unwrap();
        assert!(save_exists(&d));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn backups_are_counted_by_their_folder_names() {
        let d = std::env::temp_dir().join(format!("s3l-bak-{}", util::random_hex(4)));
        assert_eq!(backup_info(&d).count, 0);
        let b = d.join("save-profile-backup");
        for n in ["20261110-040000", "20261110-050101", "notes", "2026-11-10"] {
            std::fs::create_dir_all(b.join(n)).unwrap();
        }
        std::fs::write(b.join("20261111-000000"), b"a file, not a backup").unwrap();
        let i = backup_info(&d);
        assert_eq!((i.count, i.newest.as_deref(), i.exists), (2, Some("20261110-050101"), true));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn new_log_lines_of_the_final_runtime() {
        let zero = "save profile: p.json applied to s/save.dat: 0 changes (the save already matches; nothing written, no backup)";
        let f = parse_log(zero);
        assert_eq!((f.state.as_str(), f.changes, f.backup.as_deref()), ("already", Some(0), None));
        let one = "save profile: p.json applied to s/save.dat (1 changes, backup C:/u/save-profile-backup/20261110-050101):
  WeaponId 10 -> 50
save profile: deleted 2 older backups (the newest 10 are kept)";
        let f = parse_log(one);
        assert_eq!((f.state.as_str(), f.changes, f.deleted_backups, f.keep_backups), ("applied", Some(1), Some(2), Some(10)));
        let new = "save profile: no save yet, and a new save needs the whole appearance: the profile lacks hair, skin; the profile is not applied, the game creates its own character";
        let f = parse_log(new);
        assert_eq!(f.state, "needs_appearance");
        assert_eq!(f.missing, vec!["hair", "skin"]);
    }

    #[test]
    fn refusals_are_mapped_to_the_field_they_are_about() {
        let r = classify_refusal("hair 33 belongs to Octolings, not to the style inkling_f (it is the save's value: give a matching one in the profile too); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str(), r.id), ("hair", "species", Some(33)));
        assert_eq!((r.species.as_deref(), r.style.as_deref(), r.from_save), (Some("octoling"), Some("inkling_f"), true));
        let r = classify_refusal("eyebrows 2 belongs to Inklings, not to the style octoling_m (2); the save is left as it is");
        assert_eq!((r.field.as_str(), r.species.as_deref(), r.style.as_deref(), r.from_save), ("eyebrows", Some("inkling"), Some("octoling_m"), false));
        let r = classify_refusal("hair 99 is not a selectable one of the style inkling_m (99); the save is left as it is");
        assert_eq!((r.code.as_str(), r.id), ("unselectable", Some(99)));
        let r = classify_refusal("the game removes this gear from the save when it loads it; do not equip it (3007); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str(), r.id), ("gear", "gear_dropped", Some(3007)));
        let r = classify_refusal("weapon is not a Versus main weapon (5555); the save is left as it is");
        assert_eq!((r.field.as_str(), r.id), ("weapon", Some(5555)));
        assert_eq!(classify_refusal("skin must be 0..8 (9); the save is left as it is").field, "skin");
        assert_eq!(classify_refusal("legwear_variation out of range (7); the save is left as it is").field, "legwear_variation");
        assert_eq!(classify_refusal("the save has no UserName (0); the save is left as it is").code, "save_layout");
        // The engine's wording names the game version it was written for: any version is the same refusal.
        for v in ["11.2.0", "11.3.0", "11.4.0"] {
            assert_eq!(classify_refusal(&format!("save.dat does not hold the {v} layout; the save is left as it is")).code, "save_layout");
        }
        let r = classify_refusal("something new; the save is left as it is");
        assert_eq!((r.code.as_str(), r.raw.as_str()), ("other", "something new; the save is left as it is"));
        // end to end through the log
        let f = parse_log("[x] save profile: hair 33 belongs to Octolings, not to the style inkling_f (33); the save is left as it is");
        assert_eq!(f.state, "failed");
        assert_eq!(f.refusal.unwrap().field, "hair");
    }

    // ---- sub abilities and the chunk stock ----------------------------------------------------------------------

    fn slots(a: [Option<&str>; 3]) -> Option<Vec<Option<String>>> {
        Some(a.iter().map(|x| x.map(str::to_string)).collect())
    }
    fn subs_profile() -> Profile {
        let mut p = full();
        p.sub_abilities.head = slots([Some("Alpha"), None, Some("Alpha")]);
        p.sub_abilities.clothes = slots([None, None, None]);
        p
    }

    #[test]
    fn sub_abilities_are_emitted_for_the_parts_with_a_piece_and_an_edit() {
        let v = to_runtime(&subs_profile(), ApplyOptions::default(), true).unwrap();
        assert_eq!(v["gear_abilities"], serde_json::json!({"head": ["Alpha", null, "Alpha"], "clothes": [null, null, null]}));
        assert!(v.get("chunks").is_none(), "off by default");
        // shoes were never edited: absent, so the save keeps its own
        assert!(v["gear_abilities"].get("shoes").is_none());
        // a part without a chosen piece is left out, whatever was stored for it
        let mut p = subs_profile();
        p.head = None;
        assert!(to_runtime(&p, ApplyOptions::default(), true).unwrap()["gear_abilities"].get("head").is_none());
        // no edit at all: no member
        assert!(to_runtime(&full(), ApplyOptions::default(), true).unwrap().get("gear_abilities").is_none());
        let mut only = full();
        only.head = None;
        only.clothes = None;
        only.shoes = None;
        only.sub_abilities.head = slots([Some("Alpha"), None, None]);
        assert!(to_runtime(&only, ApplyOptions::default(), true).unwrap().get("gear_abilities").is_none());
    }

    #[test]
    fn three_empty_slots_are_sent_as_three_nulls_so_that_they_clear() {
        // The engine leaves a part that is absent (or null) as the save has it and writes three empty slots for
        // [null, null, null]: the only way to take earlier abilities off a piece.
        let mut p = full();
        p.sub_abilities.shoes = slots([Some("Beta"), Some("Alpha"), None]);
        assert_eq!(to_runtime(&p, ApplyOptions::default(), true).unwrap()["gear_abilities"]["shoes"], serde_json::json!(["Beta", "Alpha", null]));
        p.sub_abilities.shoes = slots([None, None, None]);
        assert_eq!(to_runtime(&p, ApplyOptions::default(), true).unwrap()["gear_abilities"]["shoes"], serde_json::json!([null, null, null]));
    }

    #[test]
    fn the_chunk_stock_is_max_when_asked() {
        let on = ApplyOptions { unlock_chunks: true, ..ApplyOptions::default() };
        assert_eq!(to_runtime(&full(), on, true).unwrap()["chunks"], "max");
        assert!(to_runtime(&full(), ApplyOptions::default(), true).unwrap().get("chunks").is_none());
        assert!(!ApplyOptions::default().unlock_chunks);
    }

    #[test]
    fn a_malformed_sub_ability_part_is_refused_before_launch() {
        let mut p = subs_profile();
        p.sub_abilities.head = Some(vec![Some("Alpha".into())]);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), true), Err(ConvertError::Invalid("sub_abilities", _))));
        p.sub_abilities.head = slots([Some("bad name"), None, None]);
        assert!(matches!(to_runtime(&p, ApplyOptions::default(), true), Err(ConvertError::Invalid("sub_abilities", _))));
    }

    #[test]
    fn sub_abilities_are_validated_against_the_sub_capable_set() {
        let cat = cat_with_gear();
        let set = vec!["Alpha".to_string(), "Beta".to_string()];
        let ok = Profile { name: "K".into(), head: Some(1000), sub_abilities: SubAbilities { head: slots([Some(" Alpha "), None, Some("Beta")]), ..Default::default() }, ..Default::default() };
        let clean = validate_profile(&ok, &cat, Some(&set), None).unwrap();
        assert_eq!(clean.sub_abilities.head, slots([Some("Alpha"), None, Some("Beta")]), "names are trimmed");
        let bad = Profile {
            sub_abilities: SubAbilities { head: slots([Some("MainOnly"), None, None]), clothes: Some(vec![None, None]), shoes: slots([Some("a-b"), None, Some("Alpha")]) },
            ..ok.clone()
        };
        match validate_profile(&bad, &cat, Some(&set), None).unwrap_err() {
            AppError::Validation(v) => assert_eq!(v.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["sub_abilities.head.0", "sub_abilities.clothes", "sub_abilities.shoes.0"]),
            e => panic!("{e:?}"),
        }
        // without the list (no keys file) the shape is all that can be checked
        assert!(validate_profile(&Profile { sub_abilities: SubAbilities { head: slots([Some("MainOnly"), None, None]), ..Default::default() }, ..ok }, &cat, None, None).is_ok());
    }

    #[test]
    fn a_profile_saved_before_sub_abilities_loads_with_none_chosen() {
        let d = std::env::temp_dir().join(format!("s3l-subs-prof-{}", util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            profile_path(&d),
            r#"{"schema":1,"name":"Old","style":1,"head":1000,"clothes":8031,"shoes":4024,"weapon":10,"banner":11001,"title":{"adjective":142,"subject":0},"badges":[null,null,null]}"#,
        )
        .unwrap();
        let p = load_profile(&d);
        assert_eq!((p.name.as_str(), p.head, p.sub_abilities.clone()), ("Old", Some(1000), SubAbilities::default()));
        let complete = Profile { hair: Some(1), eyebrows: Some(1), legwear: Some(1), skin: Some(1), eyes: Some(1), ..p.clone() };
        assert!(to_runtime(&complete, ApplyOptions::default(), true).unwrap().get("gear_abilities").is_none());
        // and the new member survives a save and a load, with its nulls
        let q = Profile { sub_abilities: SubAbilities { shoes: slots([None, Some("Alpha"), None]), ..Default::default() }, ..p };
        save_profile(&d, &q).unwrap();
        assert!(std::fs::read_to_string(profile_path(&d)).unwrap().contains("sub_abilities"));
        assert_eq!(load_profile(&d), q);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_runtimes_sub_ability_refusals_are_mapped() {
        let r = classify_refusal("gear_abilities.head[1]: ability 104 is one only a main slot has, not a sub slot (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("sub_abilities.head", "sub_main_only"));
        let r = classify_refusal("gear_abilities.shoes[0]: the game has no ability named \"Nope\" (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("sub_abilities.shoes", "sub_unknown"));
        let r = classify_refusal("gear_abilities.clothes[2]: 55 is not a value of the game's ability enum (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("sub_abilities.clothes", "sub_unknown"));
        for m in [
            "gear_abilities.head must be an array of 3 abilities (0); the save is left as it is",
            "gear_abilities.head[0] must be an ability name, an enum value or null (0); the save is left as it is",
            "gear_abilities has an unknown part \"hat\" (head, clothes, shoes) (0); the save is left as it is",
            "gear_abilities must be an object {\"head\": [a, b, c], \"clothes\": ..., \"shoes\": ...} (0); the save is left as it is",
        ] {
            assert_eq!(classify_refusal(m).code, "sub_invalid", "{m}");
        }
        assert_eq!(classify_refusal("gear_abilities has an unknown part \"hat\" (head, clothes, shoes) (0); the save is left as it is").field, "sub_abilities");
        let r = classify_refusal("gear_abilities: the save does not own the equipped gear (give it in \"gear\" too) (1018); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str(), r.id), ("sub_abilities", "sub_not_owned", Some(1018)));
        let r = classify_refusal("chunks must be 0..999 (1200); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("chunks", "chunks"));
        assert_eq!(classify_refusal("chunks must be \"max\" or a count (0); the save is left as it is").field, "chunks");
        assert_eq!(classify_refusal("the ability names were not found in main (0); the save is left as it is").code, "ability_names");
        // an ownership record the runtime cannot find is still a save layout problem
        assert_eq!(classify_refusal("the save has no HaveSkillChipMap where 11.2.0 has it (0); the save is left as it is").code, "save_layout");
        // end to end through the log
        let f = parse_log("[x] save profile: gear_abilities.head[1]: ability 104 is one only a main slot has, not a sub slot (0); the save is left as it is");
        assert_eq!((f.state.as_str(), f.refusal.unwrap().code.as_str()), ("failed", "sub_main_only"));
    }

    // ---- main abilities -----------------------------------------------------------------------------------------

    /// Made-up abilities: every part's pieces have "Alpha" and "Beta" as main ability; "HeadOnly" exists on a head
    /// piece only and "ShoesOnly" on a shoes piece only; "Unused" is in the enum but on no piece.
    fn cat_with_mains() -> Catalogue {
        let mut c = neutral();
        let piece = |id, ability: &str| Entry { id, selectable: true, ability: Some(ability.into()), ..Default::default() };
        c.categories.insert("head".into(), vec![piece(1018, "Alpha"), piece(1019, "Beta"), piece(1020, "HeadOnly"), Entry { id: 1021, selectable: false, ability: Some("Hidden".into()), ..Default::default() }]);
        c.categories.insert("clothes".into(), vec![piece(5001, "Alpha"), piece(5002, "Beta")]);
        c.categories.insert("shoes".into(), vec![piece(6003, "Beta"), piece(6004, "ShoesOnly")]);
        c
    }
    fn main_set() -> Vec<String> {
        ["Alpha", "Beta", "HeadOnly", "ShoesOnly", "Unused", "Hidden"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn main_abilities_are_emitted_for_the_parts_with_a_piece_and_a_choice() {
        let mut p = full();
        p.main_abilities = MainAbilities { head: Some("Alpha".into()), clothes: None, shoes: Some("Beta".into()) };
        let v = to_runtime(&p, ApplyOptions::default(), true).unwrap();
        assert_eq!(v["main_abilities"], serde_json::json!({"head": "Alpha", "shoes": "Beta"}), "clothes never chosen: absent");
        // a part without a chosen piece is left out: the runtime would apply it to whatever the save wears
        p.head = None;
        assert!(to_runtime(&p, ApplyOptions::default(), true).unwrap()["main_abilities"].get("head").is_none());
        // no choice at all: no member
        assert!(to_runtime(&full(), ApplyOptions::default(), true).unwrap().get("main_abilities").is_none());
        // the sub abilities are independent of it
        let mut q = subs_profile();
        q.main_abilities.clothes = Some("Beta".into());
        let v = to_runtime(&q, ApplyOptions::default(), true).unwrap();
        assert_eq!((v["main_abilities"].clone(), v["gear_abilities"]["clothes"].clone()), (serde_json::json!({"clothes": "Beta"}), serde_json::json!([null, null, null])));
        // the Splatfest-only ability, chosen with an older launcher: left out, the rest of the profile still applies
        q.main_abilities.clothes = Some(crate::game::catalogue::FEST_ONLY_ABILITY.into());
        assert!(to_runtime(&q, ApplyOptions::default(), true).unwrap().get("main_abilities").is_none());
        q.main_abilities.clothes = Some("bad name".into());
        assert!(matches!(to_runtime(&q, ApplyOptions::default(), true), Err(ConvertError::Invalid("main_abilities", _))));
    }

    #[test]
    fn main_abilities_are_validated_against_the_enum_and_the_parts_own_pieces() {
        let cat = cat_with_mains();
        let set = main_set();
        let ok = Profile { name: "K".into(), head: Some(1018), main_abilities: MainAbilities { head: Some(" HeadOnly ".into()), clothes: Some("Alpha".into()), shoes: Some("ShoesOnly".into()) }, ..Default::default() };
        let clean = validate_profile(&ok, &cat, None, Some(&set)).unwrap();
        assert_eq!(clean.main_abilities, MainAbilities { head: Some("HeadOnly".into()), clothes: Some("Alpha".into()), shoes: Some("ShoesOnly".into()) }, "names are trimmed");
        // an empty name is no choice
        let empty = Profile { main_abilities: MainAbilities { head: Some("  ".into()), ..Default::default() }, ..ok.clone() };
        assert_eq!(validate_profile(&empty, &cat, None, Some(&set)).unwrap().main_abilities, MainAbilities::default());
        // another part's own main ability, one no selectable piece has, one outside the enum, a malformed name
        let bad = Profile { main_abilities: MainAbilities { head: Some("ShoesOnly".into()), clothes: Some("Unused".into()), shoes: Some("Nope".into()) }, ..ok.clone() };
        match validate_profile(&bad, &cat, None, Some(&set)).unwrap_err() {
            AppError::Validation(v) => assert_eq!(v.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["main_abilities.head", "main_abilities.clothes", "main_abilities.shoes"]),
            e => panic!("{e:?}"),
        }
        let hidden = Profile { main_abilities: MainAbilities { head: Some("Hidden".into()), ..Default::default() }, ..ok.clone() };
        assert!(validate_profile(&hidden, &cat, None, Some(&set)).is_err(), "only the selectable pieces count");
        let shape = Profile { main_abilities: MainAbilities { head: Some("a-b".into()), ..Default::default() }, ..ok.clone() };
        assert!(validate_profile(&shape, &cat, None, None).is_err());
        // The Splatfest-only ability and the piece that carries it, kept from an earlier launcher: dropped, not refused.
        let mut fest_cat = cat.clone();
        fest_cat.categories.entry("clothes".into()).or_default().push(crate::game::catalogue::Entry { id: 26000, selectable: true, ability: Some(crate::game::catalogue::FEST_ONLY_ABILITY.into()), ..Default::default() });
        crate::game::catalogue::apply_save_constraints(&mut fest_cat);
        let old = Profile { clothes: Some(26000), main_abilities: MainAbilities { head: Some("HeadOnly".into()), clothes: Some(crate::game::catalogue::FEST_ONLY_ABILITY.into()), shoes: None }, ..ok.clone() };
        let clean = validate_profile(&old, &fest_cat, None, Some(&set)).unwrap();
        assert_eq!((clean.clothes, clean.main_abilities.clone()), (None, MainAbilities { head: Some("HeadOnly".into()), clothes: None, shoes: None }));
        // without the keys file the part's pieces still decide; without game data only the shape is checked
        assert!(validate_profile(&bad, &cat, None, None).is_err());
        assert!(validate_profile(&Profile { head: Some(1000), ..bad.clone() }, &cat_with_gear(), None, None).is_ok());
        assert_eq!(part_main_abilities(&cat, "shoes").unwrap().into_iter().collect::<Vec<_>>(), ["Beta", "ShoesOnly"]);
        assert!(part_main_abilities(&cat_with_gear(), "head").is_none());
    }

    #[test]
    fn a_profile_saved_before_main_abilities_loads_with_none_chosen() {
        let d = std::env::temp_dir().join(format!("s3l-main-prof-{}", util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(profile_path(&d), r#"{"schema":1,"name":"Old","head":1000,"sub_abilities":{"head":["Alpha",null,null]}}"#).unwrap();
        let p = load_profile(&d);
        assert_eq!((p.name.as_str(), p.main_abilities.clone()), ("Old", MainAbilities::default()));
        assert_eq!(p.sub_abilities.head, slots([Some("Alpha"), None, None]));
        let q = Profile { main_abilities: MainAbilities { shoes: Some("Beta".into()), ..Default::default() }, ..p };
        save_profile(&d, &q).unwrap();
        assert!(std::fs::read_to_string(profile_path(&d)).unwrap().contains("main_abilities"));
        assert_eq!(load_profile(&d), q);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_runtimes_main_ability_refusals_are_mapped() {
        let r = classify_refusal("main_abilities.shoes: ability 105 is not the main ability of any shoes piece of the game (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("main_abilities.shoes", "main_part"));
        let r = classify_refusal("main_abilities.head: the game has no ability named \"Nope\" (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("main_abilities.head", "main_unknown"));
        let r = classify_refusal("main_abilities.clothes: 55 is not a value of the game's ability enum (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("main_abilities.clothes", "main_unknown"));
        let r = classify_refusal("main_abilities.head: a main slot is never empty (null leaves the piece's own) (0); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str()), ("main_abilities.head", "main_empty"));
        for m in [
            "main_abilities.head must be an ability name or an enum value (0); the save is left as it is",
            "main_abilities has an unknown part \"hat\" (head, clothes, shoes) (0); the save is left as it is",
            "main_abilities must be an object {\"head\": a, \"clothes\": b, \"shoes\": c} (0); the save is left as it is",
        ] {
            assert_eq!(classify_refusal(m).code, "main_invalid", "{m}");
        }
        assert_eq!(classify_refusal("main_abilities has an unknown part \"hat\" (head, clothes, shoes) (0); the save is left as it is").field, "main_abilities");
        let r = classify_refusal("main_abilities: the save does not own the equipped gear (give it in \"gear\" too) (1018); the save is left as it is");
        assert_eq!((r.field.as_str(), r.code.as_str(), r.id), ("main_abilities", "main_not_owned", Some(1018)));
        let f = parse_log("[x] save profile: main_abilities.head: ability 105 is not the main ability of any head piece of the game (0); the save is left as it is");
        assert_eq!((f.state.as_str(), f.refusal.unwrap().code.as_str()), ("failed", "main_part"));
    }
}
