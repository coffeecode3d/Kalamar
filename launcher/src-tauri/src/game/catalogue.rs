//! The game catalogue: every customisation category, built at run time from the player's own RomFS.
//!
//! Sources (all inside `romfs.bin`): `RSDB/*.rstbl.byml.zs` parameter tables for ids and filter fields,
//! `Mals/<region>.Product.*.sarc.zs` message archives for display names, `UI/Icon/**` and the creation-screen layout
//! archives for icons. Nothing produced here is ever written outside the per-user cache directory.
//!
//! # Stages
//!
//! The versus stages are the rows of `RSDB/VersusSceneInfo` (`__RowId` is the scene name the engine, the community
//! protocol and the result files use, `Vss_<Name>[digits]`; `Id` the engine's stage number; `IsDisplayInStageList`;
//! `DisplayOrder`). Their names are the labels of `CommonMsg/VS/VSStageName.msbt` and their pictures, the 640x360
//! screenshots of the game's stage menus, are `UI/Icon/StageL/Vss_<label>.bntx.zs` (ASTC 4x4 sRGB). Labels and
//! pictures name a stage without the scene's version digits (a made-up `Vss_Plaza03` -> `Plaza`), as the engine's
//! `--list-match-options` does. The pictures are served scaled (see [`super::icons::SIZES`]).
//!
//! # Selectability rules (`Entry::selectable`)
//!
//! * Gear (head, clothes, shoes): `HowToGet != "Impossible"`. Impossible rows are the Salmon Run uniforms (`*_COP*`),
//!   the story-mode suits (`*_MSN*`), Side Order gear (`*_SDODR*`) and the INVISIBLE placeholder; every other value
//!   (`Shop`, `Catalog`, `Uroko`, `Other`) is gear a player can own and wear.
//! * Main weapons: `Type == "Versus"`. The other types are Salmon Run (`Coop`), story mode (`Mission`, `Rival`) and
//!   Side Order (`Other`) actors that have neither a shop entry nor a locker icon.
//! * Hair, eyebrows, legwear: `Order >= 0` and the row id has no `Sdodr` marker. `Order = -1` rows are story or NPC
//!   looks (commander, Octoling soldiers), `*_SdodrCstm*` rows are Side Order duplicates.
//! * Banners, title words, badges: every row is a player-obtainable item (`HowToGet` is never `Impossible` there).
//! * Style, skin tone, eye color have no table; their ranges come from the creation screen layout (see below). The
//!   skin and eye colors come from the creation screen's color button animation (see [`choice_colours`]).

use super::bflan;
use super::bntx;
use super::byml::{self, Byml};
use super::msbt::{self, Msbt};
use super::romfs::RomFs;
use super::sarc::Sarc;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

pub const SCHEMA: u32 = 6;
/// UI languages and the message archive each one is read from.
pub const LANGUAGES: [(&str, &str); 4] = [("fr", "EUfr"), ("en", "EUen"), ("es", "USes"), ("ja", "JPja")];

pub type Names = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    /// The id the game stores (and `profile.json` uses).
    pub id: i64,
    /// Row id in the parameter table (internal label, used as the display fallback).
    pub row: String,
    /// Display names by UI language; empty when the game has no text for the entry.
    pub name: Names,
    /// Icon key (`rom/UI/Icon/...` or `lay/<layout>/<texture>`), resolved by [`super::icons`].
    pub icon: Option<String>,
    /// True when the icon was matched by shape because the game's layout has no explicit link (see README).
    pub icon_approx: bool,
    pub order: i64,
    pub season: Option<i64>,
    pub selectable: bool,
    pub how_to_get: Option<String>,
    /// `inkling` or `octoling` for creation options that depend on the species.
    pub species: Option<String>,
    pub brand: Option<String>,
    pub rarity: Option<i64>,
    pub ability: Option<String>,
    /// Weapon class, banner series or badge category.
    pub class: Option<String>,
    pub sub: Option<String>,
    pub special: Option<String>,
    pub points: Option<i64>,
    /// Legwear: number of color variations (`VariationNum`).
    pub variations: Option<i64>,
    /// Skin tone, eye color: the color(s) the creation screen paints on the choice button, as `#rrggbb` (a second
    /// color for two-tone eyes); empty when the game data did not give them.
    pub colors: Vec<String>,
    /// Why a not-selectable entry cannot be chosen although the game has it, when the save documentation says so
    /// (`game_drops`, `badge_record`, `outside_range`); shown to the user.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Named {
    pub name: Names,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CategoryReport {
    pub category: String,
    pub rows: usize,
    pub selectable: usize,
    pub with_icon: usize,
    pub with_name: usize,
    pub note: String,
}

/// A versus stage (see the module's notes).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Stage {
    /// The scene name: the stage id of the engine's list, the protocol (`rules.stage`) and the result files.
    pub scene: String,
    /// The engine's stage number (`Id`).
    pub id: i64,
    /// Display names by UI language; empty when the game has no text for the stage.
    pub name: Names,
    /// Picture key (`rom/UI/Icon/StageL/...`), to be asked at one of the icon sizes (`<key>@480`, `<key>@160`).
    pub image: Option<String>,
    /// Shown in the game's stage lists (`IsDisplayInStageList`); a stage that is not is still named and pictured.
    pub listed: bool,
    pub order: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalogue {
    pub schema: u32,
    pub cache_key: String,
    pub build_ms: u64,
    pub categories: BTreeMap<String, Vec<Entry>>,
    pub brands: BTreeMap<String, Named>,
    pub abilities: BTreeMap<String, Named>,
    pub subs: BTreeMap<String, Named>,
    pub specials: BTreeMap<String, Named>,
    pub classes: BTreeMap<String, Named>,
    #[serde(default)]
    pub stages: Vec<Stage>,
    pub report: Vec<CategoryReport>,
}

pub const CATEGORY_IDS: [&str; 14] =
    ["style", "skin", "eyes", "hair", "eyebrows", "legwear", "head", "clothes", "shoes", "weapon", "banner", "adjective", "subject", "badge"];

// ---------------------------------------------------------------------------------------------------------------

/// A decompressed SARC and its index.
type Archive = (Vec<u8>, Sarc);
/// (language, message file) -> parsed messages (None when absent).
type MsgCache = HashMap<(String, String), Option<Rc<Msbt>>>;
use std::rc::Rc;

struct Source<'a> {
    rom: &'a RomFs,
    tables: HashMap<String, String>, // table name -> path
    mals: HashMap<&'static str, String>,
    msgs: RefCell<MsgCache>,
    archives: RefCell<HashMap<String, Option<Rc<Archive>>>>,
    layouts: RefCell<HashMap<String, Vec<String>>>,
}

impl<'a> Source<'a> {
    fn new(rom: &'a RomFs) -> Self {
        let mut tables = HashMap::new();
        let mut mals = HashMap::new();
        for p in rom.paths() {
            if let Some(rest) = p.strip_prefix("RSDB/")
                && let Some(name) = rest.split(".Product.").next()
                && p.ends_with(".rstbl.byml.zs")
            {
                tables.insert(name.to_string(), p.clone());
            }
            if let Some(rest) = p.strip_prefix("Mals/") {
                for (_, region) in LANGUAGES {
                    if rest.starts_with(&format!("{region}.Product.")) && rest.ends_with(".sarc.zs") {
                        mals.insert(region, p.clone());
                    }
                }
            }
        }
        Source { rom, tables, mals, msgs: RefCell::new(HashMap::new()), archives: RefCell::new(HashMap::new()), layouts: RefCell::new(HashMap::new()) }
    }

    fn table(&self, name: &str) -> Result<Vec<Byml>, String> {
        let path = self.tables.get(name).ok_or_else(|| format!("table {name} not found in RSDB"))?;
        let doc = byml::parse(&self.rom.read(path)?).map_err(|e| format!("{path}: {e}"))?;
        match doc {
            Byml::Array(rows) => Ok(rows),
            _ => Err(format!("{path}: not an array of rows")),
        }
    }

    /// A message file from the language archive, e.g. `CommonMsg/Gear/GearName_Head.msbt`.
    fn msg(&self, lang: &str, file: &str) -> Option<Rc<Msbt>> {
        let key = (lang.to_string(), file.to_string());
        if let Some(v) = self.msgs.borrow().get(&key) {
            return v.clone();
        }
        let region = LANGUAGES.iter().find(|(l, _)| *l == lang)?.1;
        let parsed = self.mals.get(region).and_then(|path| {
            let mut arch = self.archives.borrow_mut();
            let entry = arch
                .entry(path.clone())
                .or_insert_with(|| {
                    let data = self.rom.read(path).ok()?;
                    let sarc = Sarc::parse(&data).ok()?;
                    Some(Rc::new((data, sarc)))
                })
                .clone()?;
            let bytes = entry.1.find(&entry.0, file)?;
            msbt::parse(bytes).ok().map(Rc::new)
        });
        self.msgs.borrow_mut().insert(key, parsed.clone());
        parsed
    }

    /// Names of one label across all languages (only languages that have the label).
    fn names(&self, file: &str, label: &str) -> Names {
        self.names_with(file, label, |m| m.plain())
    }

    fn names_with(&self, file: &str, label: &str, f: impl Fn(&msbt::Message) -> String) -> Names {
        let mut out = Names::new();
        for (lang, _) in LANGUAGES {
            if let Some(m) = self.msg(lang, file).and_then(|m| m.get(label).map(&f)) && !m.is_empty() {
                out.insert(lang.to_string(), m);
            }
        }
        out
    }

    fn icon_if(&self, path_no_ext: &str) -> Option<String> {
        self.rom.contains(&format!("{path_no_ext}.bntx.zs")).then(|| format!("rom/{path_no_ext}"))
    }
}

fn gyml_row(path: &str) -> &str {
    // "Work/Gyml/PoisonMist.spl__WeaponInfoSub.gyml" -> "PoisonMist"
    path.rsplit('/').next().unwrap_or(path).split('.').next().unwrap_or(path)
}

fn season_of(r: &Byml) -> Option<i64> {
    r.int_of("Season")
}

fn base(r: &Byml) -> Entry {
    Entry {
        id: r.int_of("Id").unwrap_or(-1),
        row: r.str_of("__RowId").unwrap_or("").to_string(),
        order: r.int_of("Order").unwrap_or(0),
        season: season_of(r),
        how_to_get: r.str_of("HowToGet").map(str::to_string),
        ..Default::default()
    }
}

fn report(cat: &str, entries: &[Entry], note: &str) -> CategoryReport {
    CategoryReport {
        category: cat.into(),
        rows: entries.len(),
        selectable: entries.iter().filter(|e| e.selectable).count(),
        with_icon: entries.iter().filter(|e| e.selectable && e.icon.is_some()).count(),
        with_name: entries.iter().filter(|e| e.selectable && !e.name.is_empty()).count(),
        note: note.into(),
    }
}

// ---- creation options ------------------------------------------------------------------------------------------

const CREATION_LAYOUT_HEAD: &str = "BtnSelectHead_00";
const CREATION_LAYOUT_BROW: &str = "BtnSelectEyebrow_00";
const CREATION_LAYOUT_TUTO: &str = "Tuto_MakePlayer_00";

/// Hair icon by shape: row id -> `HeadBtn_NN` texture of the creation screen. The layout lists 26 silhouettes but
/// stores no link to the hair rows; these pairs were matched by comparing the silhouettes with the Japanese row
/// labels (long, short, dumpling, wave, twin tails, ...). They are flagged `icon_approx`.
const HAIR_ICON: [(i64, u32); 24] = [
    (0, 10), (1, 8), (2, 4), (3, 3), (4, 11), (5, 1), (6, 6), (7, 7), (8, 0), (9, 9), (10, 12), (11, 13), (12, 19), (13, 23), (14, 25), (15, 29),
    (30, 14), (31, 16), (32, 29), (33, 17), (34, 20), (35, 28), (36, 27), (37, 15),
];

fn layout_has_texture(src: &Source, layout: &str, tex: &str) -> bool {
    let mut cache = src.layouts.borrow_mut();
    cache.entry(layout.to_string()).or_insert_with(|| layout_textures(src.rom, layout)).iter().any(|t| t == tex)
}

/// Texture names of a layout archive (`__Combined.bntx`).
pub fn layout_textures(rom: &RomFs, layout: &str) -> Vec<String> {
    let path = format!("Layout/{layout}.Nin_NX_NVN.blarc.zs");
    let Ok(data) = rom.read(&path) else { return vec![] };
    let Ok(s) = Sarc::parse(&data) else { return vec![] };
    let Some(b) = s.find(&data, "timg/__Combined.bntx") else { return vec![] };
    bntx::parse(b).map(|t| t.into_iter().map(|x| x.name).collect()).unwrap_or_default()
}

/// Number of buttons of a kind on the creation screen (`L_BtnSkin_00..`), counted from the layout's pane names.
fn layout_button_count(rom: &RomFs, layout: &str, prefix: &str) -> usize {
    let path = format!("Layout/{layout}.Nin_NX_NVN.blarc.zs");
    let Ok(data) = rom.read(&path) else { return 0 };
    let Ok(s) = Sarc::parse(&data) else { return 0 };
    let Some(b) = s.find(&data, &format!("blyt/{layout}.bflyt")) else { return 0 };
    let needle = prefix.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while let Some(pos) = b[i..].windows(needle.len()).position(|w| w == needle) {
        let at = i + pos;
        // a pane name is `prefix` + two digits + NUL padding
        let tail = &b[at + needle.len()..(at + needle.len() + 3).min(b.len())];
        if tail.len() == 3 && tail[0].is_ascii_digit() && tail[1].is_ascii_digit() && tail[2] == 0 {
            let idx = ((tail[0] - b'0') * 10 + (tail[1] - b'0')) as usize;
            n = n.max(idx + 1);
        }
        i = at + needle.len();
    }
    n
}

const COLOUR_LAYOUT: &str = "BtnSelectColor_00";
const COLOUR_ANIM: &str = "Variation";
/// Frame of choice 0 in the color button's Variation animation: skin tone i is frame i, eye color i is frame 20 + i
/// (the frames in between hold the last skin tone). Checked against the iris colors of the player model's
/// `M_Eye_Alb.NN` textures.
const SKIN_FRAME0: f32 = 0.0;
const EYES_FRAME0: f32 = 20.0;

/// Keyframe tracks of the color button's Variation animation; empty when the archive or the animation is missing.
fn colour_tracks(rom: &RomFs) -> Vec<bflan::Track> {
    let path = format!("Layout/{COLOUR_LAYOUT}.Nin_NX_NVN.blarc.zs");
    let Ok(data) = rom.read(&path) else { return vec![] };
    let Ok(s) = Sarc::parse(&data) else { return vec![] };
    let Some(b) = s.find(&data, &format!("anim/{COLOUR_LAYOUT}_{COLOUR_ANIM}.bflan")) else { return vec![] };
    bflan::parse(b).unwrap_or_default()
}

/// White color (`FLMC` targets 4..6) of a part at a frame, as `#rrggbb`.
fn white_colour(tracks: &[bflan::Track], pane: &str, frame: f32) -> Option<String> {
    let mut rgb = [0u8; 3];
    for (i, c) in rgb.iter_mut().enumerate() {
        let t = tracks.iter().find(|t| t.name == pane && &t.tag == b"FLMC" && t.target == 4 + i as u8)?;
        *c = bflan::value_at(&t.keys, frame)?.round().clamp(0.0, 255.0) as u8;
    }
    Some(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
}

/// The color(s) of one choice button. `P_Btn_00` is the button face; `P_Btn_02` is a second-color mask the
/// animation slides into view (its texture SRT leaves its frame-0 value) for the two-tone eye colors.
fn choice_colours(tracks: &[bflan::Track], frame: f32) -> Vec<String> {
    let Some(main) = white_colour(tracks, "P_Btn_00", frame) else { return vec![] };
    let mut out = vec![main];
    let mask = tracks.iter().find(|t| t.name == "P_Btn_02" && &t.tag == b"FLTS");
    let shown = mask.is_some_and(|t| bflan::value_at(&t.keys, frame) != bflan::value_at(&t.keys, 0.0));
    if shown && let Some(second) = white_colour(tracks, "P_Btn_02", frame) {
        out.push(second);
    }
    out
}

fn creation(src: &Source, cats: &mut BTreeMap<String, Vec<Entry>>, notes: &mut BTreeMap<&'static str, String>) {
    let rom = src.rom;

    // Style: four looks, two species. The ids follow the order of the four kind buttons on the creation screen
    // (L_BtnKind_00..03); the numeric values the save stores for them have not been verified.
    let kinds = layout_button_count(rom, CREATION_LAYOUT_TUTO, "L_BtnKind_");
    let mut styles = Vec::new();
    for i in 0..kinds.max(4) as i64 {
        let octo = i >= 2;
        let tex = if octo { "IkaTaco_01^t" } else { "IkaTaco_00^t" };
        styles.push(Entry {
            id: i,
            row: format!("Kind_{i:02}"),
            icon: layout_has_texture(src, CREATION_LAYOUT_TUTO, tex).then(|| format!("lay/{CREATION_LAYOUT_TUTO}/{tex}")),
            order: i,
            selectable: true,
            species: Some(if octo { "octoling" } else { "inkling" }.into()),
            ..Default::default()
        });
    }
    cats.insert("style".into(), styles);
    notes.insert("style", format!("{kinds} kind buttons (L_BtnKind_*) on the creation screen; buttons 0 and 1 sit under the Inkling heading, 2 and 3 under the Octoling one (pane positions); ids follow that order = the save's ModelType (SquidF, SquidM, OctopusF, OctopusM); no names in the game"));

    // Skin tone and eye color: the creation screen has L_BtnSkin_00..08 and L_BtnEyes_00..20, parts of
    // BtnSelectColor_00 whose color comes from that part's Variation animation (one frame per choice).
    let tracks = colour_tracks(rom);
    for (cat, prefix, fallback, frame0) in [("skin", "L_BtnSkin_", 9usize, SKIN_FRAME0), ("eyes", "L_BtnEyes_", 21usize, EYES_FRAME0)] {
        let n = match layout_button_count(rom, CREATION_LAYOUT_TUTO, prefix) {
            0 => fallback,
            n => n,
        };
        let v: Vec<Entry> = (0..n as i64)
            .map(|i| Entry { id: i, row: format!("{cat}_{i:02}"), order: i, selectable: true, colors: choice_colours(&tracks, frame0 + i as f32), ..Default::default() })
            .collect();
        let coloured = v.iter().filter(|e| !e.colors.is_empty()).count();
        cats.insert(cat.into(), v);
        notes.insert(
            if cat == "skin" { "skin" } else { "eyes" },
            format!("{n} buttons ({prefix}*) on the creation screen, ids are indices; colours of {coloured} from {COLOUR_LAYOUT} {COLOUR_ANIM} frames {frame0}+id"),
        );
    }

    // Hair.
    let mut hair = Vec::new();
    if let Ok(rows) = src.table("HairInfo") {
        for r in &rows {
            let mut e = base(r);
            let row = e.row.clone();
            let squid = r.bool_of("IsSquid").unwrap_or(false);
            e.species = Some(if squid { "inkling" } else { "octoling" }.into());
            e.selectable = e.order >= 0 && !row.contains("Sdodr");
            e.name = Names::new();
            if let Some(&(_, n)) = HAIR_ICON.iter().find(|(id, _)| *id == e.id) {
                let tex = format!("HeadBtn_{n:02}^t");
                if layout_has_texture(src, CREATION_LAYOUT_HEAD, &tex) {
                    e.icon = Some(format!("lay/{CREATION_LAYOUT_HEAD}/{tex}"));
                    e.icon_approx = true;
                }
            }
            hair.push(e);
        }
    }
    hair.sort_by_key(|e| (e.species.clone(), e.order));
    cats.insert("hair".into(), hair);
    notes.insert("hair", "HairInfo; icons are creation-screen silhouettes matched by shape (the layout has no id link), flagged icon_approx; no names in the game".into());

    // Eyebrows: EyebrowBtn_NN textures use the row id as NN (0..3 Inkling, 20..23 Octoling).
    let mut brows = Vec::new();
    if let Ok(rows) = src.table("EyebrowInfo") {
        for r in &rows {
            let mut e = base(r);
            let squid = r.bool_of("IsSquid").unwrap_or(false);
            e.species = Some(if squid { "inkling" } else { "octoling" }.into());
            e.selectable = e.order >= 0 && !e.row.contains("Sdodr");
            let tex = format!("EyebrowBtn_{:02}^d", e.id);
            if layout_has_texture(src, CREATION_LAYOUT_BROW, &tex) {
                e.icon = Some(format!("lay/{CREATION_LAYOUT_BROW}/{tex}"));
            }
            brows.push(e);
        }
    }
    brows.sort_by_key(|e| (e.species.clone(), e.order));
    cats.insert("eyebrows".into(), brows);
    notes.insert("eyebrows", "EyebrowInfo; icon EyebrowBtn_<id> from the creation screen; no names in the game".into());

    // Legwear: BottomInfo. The creation screen builds each look from several layered masks (Bottoms_00..23) with no
    // readable id link, so no icon is assigned.
    let mut legs = Vec::new();
    if let Ok(rows) = src.table("BottomInfo") {
        for r in &rows {
            let mut e = base(r);
            e.selectable = e.order >= 0;
            e.variations = r.int_of("VariationNum");
            legs.push(e);
        }
    }
    legs.sort_by_key(|e| e.order);
    cats.insert("legwear".into(), legs);
    notes.insert("legwear", "BottomInfo; creation-screen icons are layered masks without a readable id link: no icon; no names in the game".into());
}

// ---- gear ------------------------------------------------------------------------------------------------------

fn gear(src: &Source, table: &str, prefix: &str, msg: &str, brands: &mut BTreeMap<String, Named>, abilities: &mut BTreeMap<String, Named>) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rows) = src.table(table) else { return out };
    let file = format!("CommonMsg/Gear/{msg}.msbt");
    for r in &rows {
        let mut e = base(r);
        let key = e.row.split_once('_').map(|x| x.1).unwrap_or(&e.row).to_string();
        e.name = src.names(&file, &key);
        e.selectable = e.how_to_get.as_deref() != Some("Impossible");
        e.icon = src.icon_if(&format!("UI/Icon/Gear/{}", e.row));
        let brand = r.str_of("Brand").unwrap_or("").to_string();
        let skill = r.str_of("Skill").unwrap_or("").to_string();
        e.rarity = r.int_of("Rarity");
        if !brand.is_empty() && brand != "None" {
            brands.entry(brand.clone()).or_insert_with(|| Named { name: src.names("CommonMsg/Gear/GearBrandName.msbt", &brand), icon: src.icon_if(&format!("UI/Icon/Brand/{brand}")) });
            e.brand = Some(brand);
        }
        if !skill.is_empty() && skill != "None" {
            abilities.entry(skill.clone()).or_insert_with(|| Named { name: src.names("CommonMsg/Gear/GearPowerName.msbt", &skill), icon: src.icon_if(&format!("UI/Icon/Skill/{skill}")) });
            e.ability = Some(skill);
        }
        let _ = prefix;
        out.push(e);
    }
    out.sort_by_key(|e| (e.id, e.row.clone()));
    out
}

// ---- weapons ---------------------------------------------------------------------------------------------------

fn weapons(src: &Source, subs: &mut BTreeMap<String, Named>, specials: &mut BTreeMap<String, Named>, classes: &mut BTreeMap<String, Named>) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rows) = src.table("WeaponInfoMain") else { return out };
    for r in &rows {
        let mut e = base(r);
        e.how_to_get = r.str_of("Type").map(str::to_string);
        e.selectable = r.str_of("Type") == Some("Versus");
        e.name = src.names("CommonMsg/Weapon/WeaponName_Main.msbt", &e.row);
        e.icon = src.icon_if(&format!("UI/Icon/Wpn/Wst_{}", e.row));
        let class = e.row.split('_').next().unwrap_or("").to_string();
        classes.entry(class.clone()).or_insert_with(|| Named { name: src.names("CommonMsg/Weapon/WeaponTypeName.msbt", &class), icon: None });
        e.class = Some(class);
        e.points = r.int_of("SpecialPoint");
        e.order = r.int_of("DebugDispOrder").unwrap_or(0);
        if let Some(sub) = r.str_of("SubWeapon").filter(|s| !s.is_empty()) {
            let row = gyml_row(sub).to_string();
            subs.entry(row.clone()).or_insert_with(|| Named { name: src.names("CommonMsg/Weapon/WeaponName_Sub.msbt", &row), icon: src.icon_if(&format!("UI/Icon/SubSpe/Wsb_{row}01")) });
            e.sub = Some(row);
        }
        if let Some(sp) = r.str_of("SpecialWeapon").filter(|s| !s.is_empty()) {
            let row = gyml_row(sp).to_string();
            specials.entry(row.clone()).or_insert_with(|| Named { name: src.names("CommonMsg/Weapon/WeaponName_Special.msbt", &row), icon: src.icon_if(&format!("UI/Icon/SubSpe/Wsp_{row}01")) });
            e.special = Some(row);
        }
        out.push(e);
    }
    out.sort_by_key(|e| (e.order, e.id));
    out
}

// ---- splashtag ------------------------------------------------------------------------------------------------

fn banners(src: &Source) -> Vec<Entry> {
    let Ok(rows) = src.table("NamePlateBgInfo") else { return vec![] };
    let mut out: Vec<Entry> = rows
        .iter()
        .map(|r| {
            let mut e = base(r);
            e.selectable = e.how_to_get.as_deref() != Some("Impossible");
            e.icon = src.icon_if(&format!("UI/Icon/Npl/{}", e.row));
            // series: Npl_Catalog_Season01_Lv01 -> Npl_Catalog_Season01
            e.class = Some(e.row.rsplit_once('_').map(|x| x.0).unwrap_or(&e.row).to_string());
            e
        })
        .collect();
    out.sort_by_key(|e| (e.order, e.id));
    out
}

fn title_words(src: &Source, table: &str, file: &str, suffix: &str) -> Vec<Entry> {
    let Ok(rows) = src.table(table) else { return vec![] };
    let mut out: Vec<Entry> = rows
        .iter()
        .map(|r| {
            let mut e = base(r);
            e.row = format!("{:04}", e.id);
            e.selectable = e.how_to_get.as_deref() != Some("Impossible");
            // Subjects have two forms per word (`_0` default, `_1` the other grammatical gender); the default
            // form is shown. Adjectives carry gender tags whose first variant is the default form.
            e.name = src.names(file, &format!("{:04}{suffix}", e.id));
            e
        })
        .collect();
    out.sort_by_key(|e| e.id);
    out
}

fn badges(src: &Source) -> Vec<Entry> {
    let Ok(rows) = src.table("BadgeInfo") else { return vec![] };
    // Placeholder tags in the message texts stand for a subject: weapon (1), enemy (7), stage (14), brand (15).
    let weapons: HashMap<i64, String> = src.table("WeaponInfoMain").unwrap_or_default().iter().filter_map(|r| Some((r.int_of("Id")?, r.str_of("__RowId")?.to_string()))).collect();
    let specials: HashMap<i64, String> = src.table("WeaponInfoSpecial").unwrap_or_default().iter().filter_map(|r| Some((r.int_of("Id")?, r.str_of("__RowId")?.to_string()))).collect();
    let mut out = Vec::new();
    for r in &rows {
        let name = r.str_of("Name").unwrap_or("").to_string();
        let category = r.str_of("Category").unwrap_or("").to_string();
        let label_ex = r.str_of("MsgLabelEx").unwrap_or("").to_string();
        let (s1i, s1s) = (r.int_of("Sub1_Int").unwrap_or(0), r.str_of("Sub1_Str").unwrap_or("").to_string());
        let mut e = Entry { id: r.int_of("Id").unwrap_or(-1), row: name.clone(), order: r.int_of("Order").unwrap_or(0), selectable: !name.is_empty(), class: Some(category.clone()), ..Default::default() };
        e.icon = src.icon_if(&format!("UI/Icon/Badge/Badge_{name}"));
        let file = "CommonMsg/Badge/BadgeMsg.msbt";
        for (lang, _) in LANGUAGES {
            let Some(m) = src.msg(lang, file) else { continue };
            // Plain badges are named by their own label; level badges by the shared level text of MsgLabelEx with
            // the subject (weapon, special weapon, brand...) substituted.
            let text = if let Some(msg) = m.get(&name) {
                msg.plain()
            } else if let Some(msg) = m.get(&label_ex) {
                msg.render(&mut |group, kind| {
                    if group != 4 {
                        return None;
                    }
                    match (category.as_str(), kind) {
                        ("WeaponLevel", 1) => weapons.get(&s1i).and_then(|row| src.msg(lang, "CommonMsg/Weapon/WeaponName_Main.msbt")?.text(row)),
                        ("WinCount_WeaponSp", 1) => specials.get(&s1i).and_then(|row| src.msg(lang, "CommonMsg/Weapon/WeaponName_Special.msbt")?.text(row)),
                        ("GearTotalRarity", 15) => src.msg(lang, "CommonMsg/Gear/GearBrandName.msbt")?.text(&format!("B{s1i:02}")),
                        ("CoopBossKillNum", _) | ("CoopRareEnemyKillNum", _) => src.msg(lang, "CommonMsg/Coop/CoopEnemy.msbt")?.text(&s1s),
                        ("CoopGrade_Normal", _) => src.msg(lang, "CommonMsg/Coop/CoopStageName.msbt")?.text(&s1s),
                        _ => None,
                    }
                })
            } else {
                String::new()
            };
            let text = text.trim_start_matches([' ', ':']).trim().to_string();
            if !text.is_empty() {
                e.name.insert(lang.to_string(), text);
            }
        }
        out.push(e);
    }
    out.sort_by_key(|e| (e.order, e.id));
    out
}

// ---- stages ------------------------------------------------------------------------------------------------------

const STAGE_MSG: &str = "CommonMsg/VS/VSStageName.msbt";
const STAGE_PICTURES: &str = "UI/Icon/StageL";

/// The label a scene's name and picture go by (made-up names): `Vss_Plaza03` -> `Plaza`, `Vss_Dock` -> `Dock`.
pub fn stage_label(scene: &str) -> &str {
    scene.strip_prefix("Vss_").unwrap_or(scene).trim_end_matches(|c: char| c.is_ascii_digit())
}

fn stages(src: &Source) -> Vec<Stage> {
    let Ok(rows) = src.table("VersusSceneInfo") else { return vec![] };
    let mut out: Vec<Stage> = rows
        .iter()
        .filter_map(|r| {
            let scene = r.str_of("__RowId").filter(|s| s.starts_with("Vss_") && s.len() <= 64)?.to_string();
            let (whole, label) = (&scene[4..], stage_label(&scene));
            // Like the engine's list: the label of the whole name first, else the name without its digits.
            let mut name = src.names(STAGE_MSG, whole);
            for (lang, text) in src.names(STAGE_MSG, label) {
                name.entry(lang).or_insert(text);
            }
            let image = src.icon_if(&format!("{STAGE_PICTURES}/{scene}")).or_else(|| src.icon_if(&format!("{STAGE_PICTURES}/Vss_{label}")));
            let id = r.int_of("Id").unwrap_or(-1);
            Some(Stage { id, name, image, listed: r.bool_of("IsDisplayInStageList").unwrap_or(true), order: r.int_of("DisplayOrder").unwrap_or(id), scene })
        })
        .collect();
    out.sort_by(|a, b| (a.order, a.id, &a.scene).cmp(&(b.order, b.id, &b.scene)));
    out
}

fn stage_report(stages: &[Stage]) -> CategoryReport {
    CategoryReport {
        category: "stage".into(),
        rows: stages.len(),
        selectable: stages.iter().filter(|s| s.listed).count(),
        with_icon: stages.iter().filter(|s| s.listed && s.image.is_some()).count(),
        with_name: stages.iter().filter(|s| s.listed && !s.name.is_empty()).count(),
        note: format!("VersusSceneInfo; names {STAGE_MSG} by label (scene without Vss_ and its digits), pictures {STAGE_PICTURES}/Vss_<label>; selectable = shown in the game's stage lists"),
    }
}

/// The keys of every stage picture at every served size, for pre-extraction.
pub fn stage_picture_keys(c: &Catalogue) -> Vec<String> {
    c.stages.iter().filter_map(|s| s.image.as_deref()).flat_map(|k| super::icons::SIZES.iter().map(move |w| super::icons::sized(k, *w))).collect()
}

// ---- constraints of the runtime's save writer --------------------------------------------------------------------

/// Gear the game drops from its ownership maps at load (`engine/docs/save-profile.md`: head 3007, clothes 1030
/// and 6003, shoes 2011, 3009 and 3012; all `HowToGet: Shop`, reason not found). They cannot be equipped from a profile.
pub const DROPPED_GEAR: [(&str, i64); 6] = [("head", 3007), ("clothes", 1030), ("clothes", 6003), ("shoes", 2011), ("shoes", 3009), ("shoes", 3012)];
/// Skin tones 0..=8 and eye colors 0..=20 are the only values the runtime accepts.
pub const SKIN_MAX: i64 = 8;
pub const EYES_MAX: i64 = 20;

/// The main ability a player only has while a Splatfest runs (Ability Doubler, the Splatfest Tee's own). No Splatfest
/// runs here, so the gear that carries it is not offered, which also keeps it out of the main abilities a part can take
/// (`profile::part_main_abilities`); the engine strips it from the save's records (`docs/save-profile.md`).
pub const FEST_ONLY_ABILITY: &str = "ExSkillDouble";

/// Marks what the save writer cannot apply as unselectable with a reason: the six dropped gear ids, every badge (the
/// save's record of an acquired badge is unknown, so only already-owned badges are accepted and none can be
/// promised) and skin or eye indices outside the accepted ranges.
pub fn apply_save_constraints(cat: &mut Catalogue) {
    for (category, id) in DROPPED_GEAR {
        if let Some(e) = cat.categories.get_mut(category).and_then(|v| v.iter_mut().find(|e| e.id == id)) {
            e.selectable = false;
            e.unavailable = Some("game_drops".into());
        }
    }
    for part in ["head", "clothes", "shoes"] {
        for e in cat.categories.get_mut(part).into_iter().flatten().filter(|e| e.ability.as_deref() == Some(FEST_ONLY_ABILITY)) {
            e.selectable = false;
            e.unavailable = Some("fest_only".into());
        }
    }
    for e in cat.categories.get_mut("badge").into_iter().flatten() {
        e.selectable = false;
        e.unavailable = Some("badge_record".into());
    }
    for (category, max) in [("skin", SKIN_MAX), ("eyes", EYES_MAX)] {
        for e in cat.categories.get_mut(category).into_iter().flatten().filter(|e| e.id > max) {
            e.selectable = false;
            e.unavailable = Some("outside_range".into());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------

pub fn build(rom: &RomFs) -> Catalogue {
    let started = Instant::now();
    let src = Source::new(rom);
    let mut cat = Catalogue { schema: SCHEMA, cache_key: rom.cache_key.clone(), ..Default::default() };
    let mut notes: BTreeMap<&'static str, String> = BTreeMap::new();

    creation(&src, &mut cat.categories, &mut notes);
    let (mut brands, mut abilities) = (BTreeMap::new(), BTreeMap::new());
    cat.categories.insert("head".into(), gear(&src, "GearInfoHead", "Hed", "GearName_Head", &mut brands, &mut abilities));
    cat.categories.insert("clothes".into(), gear(&src, "GearInfoClothes", "Clt", "GearName_Clothes", &mut brands, &mut abilities));
    cat.categories.insert("shoes".into(), gear(&src, "GearInfoShoes", "Shs", "GearName_Shoes", &mut brands, &mut abilities));
    cat.brands = brands;
    cat.abilities = abilities;
    cat.categories.insert("weapon".into(), weapons(&src, &mut cat.subs, &mut cat.specials, &mut cat.classes));
    cat.categories.insert("banner".into(), banners(&src));
    cat.categories.insert("adjective".into(), title_words(&src, "BynameAdjectiveInfo", "CommonMsg/Byname/BynameAdjective.msbt", ""));
    cat.categories.insert("subject".into(), title_words(&src, "BynameSubjectInfo", "CommonMsg/Byname/BynameSubject.msbt", "_0"));
    cat.categories.insert("badge".into(), badges(&src));
    cat.stages = stages(&src);

    apply_save_constraints(&mut cat);
    for id in CATEGORY_IDS {
        let v = cat.categories.get(id).map(Vec::as_slice).unwrap_or(&[]);
        cat.report.push(report(id, v, notes.get(id).map(String::as_str).unwrap_or("")));
    }
    cat.report.push(stage_report(&cat.stages));
    cat.build_ms = started.elapsed().as_millis() as u64;
    cat
}

/// The built-in catalogue used when no game data is available: only the creation options whose id ranges are known
/// (no names, no icons, no gear, weapons or splashtag items). Contains nothing taken from the game.
pub fn neutral() -> Catalogue {
    fn plain(ids: impl IntoIterator<Item = i64>, species: impl Fn(i64) -> Option<&'static str>) -> Vec<Entry> {
        ids.into_iter().map(|id| Entry { id, row: format!("#{id}"), order: id, selectable: true, species: species(id).map(str::to_string), ..Default::default() }).collect()
    }
    let octo = |id: i64| Some(if id >= 30 || (20..30).contains(&id) { "octoling" } else { "inkling" });
    let mut c = Catalogue { schema: SCHEMA, cache_key: "neutral".into(), ..Default::default() };
    c.categories.insert("style".into(), plain(0..4, |i| Some(if i >= 2 { "octoling" } else { "inkling" })));
    c.categories.insert("skin".into(), plain(0..9, |_| None));
    c.categories.insert("eyes".into(), plain(0..21, |_| None));
    c.categories.insert("hair".into(), plain((0..16).chain(30..38), octo));
    c.categories.insert("eyebrows".into(), plain((0..4).chain(20..24), octo));
    c.categories.insert("legwear".into(), plain(0..9, |_| None));
    for id in CATEGORY_IDS {
        c.categories.entry(id.to_string()).or_default();
        let v = c.categories.get(id).cloned().unwrap_or_default();
        c.report.push(report(id, &v, "built-in neutral list"));
    }
    c
}

/// Every icon key referenced by the catalogue (entries and lookup tables), for pre-extraction.
pub fn icon_keys(c: &Catalogue) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for e in c.categories.values().flatten().filter(|e| e.selectable) {
        if let Some(i) = &e.icon {
            seen.insert(i.clone());
        }
    }
    for n in c.brands.values().chain(c.abilities.values()).chain(c.subs.values()).chain(c.specials.values()) {
        if let Some(i) = &n.icon {
            seen.insert(i.clone());
        }
    }
    seen.extend(stage_picture_keys(c));
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_constraints_mark_dropped_gear_badges_and_ranges() {
        let mut c = neutral();
        let mk = |id| Entry { id, selectable: true, ..Default::default() };
        c.categories.insert("head".into(), vec![mk(3007), mk(1000)]);
        c.categories.insert("shoes".into(), vec![mk(2011), mk(3009), mk(3012), mk(4024)]);
        c.categories.insert("clothes".into(), vec![mk(1030), mk(6003), mk(8031), Entry { ability: Some(FEST_ONLY_ABILITY.into()), ..mk(26000) }]);
        c.categories.insert("badge".into(), vec![mk(1)]);
        c.categories.get_mut("skin").unwrap().push(mk(9));
        apply_save_constraints(&mut c);
        let sel = |cat: &str| c.categories[cat].iter().filter(|e| e.selectable).map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(sel("head"), vec![1000]);
        assert_eq!(sel("shoes"), vec![4024]);
        assert_eq!(sel("clothes"), vec![8031]);
        assert!(sel("badge").is_empty());
        assert_eq!(c.categories["badge"][0].unavailable.as_deref(), Some("badge_record"));
        assert_eq!(c.categories["head"][0].unavailable.as_deref(), Some("game_drops"));
        assert_eq!(c.categories["clothes"][3].unavailable.as_deref(), Some("fest_only"));
        assert_eq!(sel("skin").len(), 9);
        assert_eq!(c.categories["skin"][9].unavailable.as_deref(), Some("outside_range"));
    }

    #[test]
    fn neutral_catalogue_has_the_known_creation_ranges() {
        let c = neutral();
        assert_eq!(c.categories["style"].len(), 4);
        assert_eq!(c.categories["skin"].len(), 9);
        assert_eq!(c.categories["eyes"].len(), 21);
        assert_eq!(c.categories["hair"].len(), 24);
        assert_eq!(c.categories["eyebrows"].len(), 8);
        assert_eq!(c.categories["legwear"].len(), 9);
        assert!(c.categories["head"].is_empty());
        assert!(c.categories.values().flatten().all(|e| e.icon.is_none() && e.name.is_empty()));
        assert!(icon_keys(&c).is_empty());
    }

    // ---- end-to-end on a synthetic RomFS (no game data) -------------------------------------------------------

    use crate::game::byml::{Byml, build as byml_build};
    use crate::game::msbt::build as msbt_build;
    use crate::game::romfs::raw_frame;
    use crate::game::sarc::build as sarc_build;

    fn row(items: Vec<(&str, Byml)>) -> Byml {
        Byml::Dict(items.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    fn s(v: &str) -> Byml {
        Byml::Str(v.into())
    }
    fn i(v: i32) -> Byml {
        Byml::I32(v)
    }

    /// Writes a tiny RomFS image + index into a temp dir and opens it.
    fn synthetic_rom() -> (RomFs, std::path::PathBuf) {
        let table = |rows: Vec<Byml>| raw_frame(&byml_build::encode(&Byml::Array(rows)));
        let mut files: Vec<(String, Vec<u8>)> = vec![
            (
                "RSDB/GearInfoHead.Product.b30.rstbl.byml.zs".into(),
                table(vec![
                    row(vec![("Id", i(1000)), ("__RowId", s("Hed_CAP000")), ("Brand", s("B07")), ("Skill", s("SubSpec_Up")), ("HowToGet", s("Shop")), ("Rarity", i(2)), ("Season", i(1))]),
                    row(vec![("Id", i(20001)), ("__RowId", s("Hed_COP001")), ("Brand", s("B97")), ("Skill", s("None")), ("HowToGet", s("Impossible")), ("Rarity", i(2)), ("Season", i(0))]),
                ]),
            ),
            (
                "RSDB/WeaponInfoMain.Product.b30.rstbl.byml.zs".into(),
                table(vec![
                    row(vec![
                        ("Id", i(0)),
                        ("__RowId", s("Shooter_Short_00")),
                        ("Type", s("Versus")),
                        ("SubWeapon", s("Work/Gyml/Bomb_Curling.spl__WeaponInfoSub.gyml")),
                        ("SpecialWeapon", s("Work/Gyml/SpUltraStamp.spl__WeaponInfoSpecial.gyml")),
                        ("SpecialPoint", i(190)),
                        ("DebugDispOrder", i(5)),
                    ]),
                    row(vec![("Id", i(20900)), ("__RowId", s("Blaster_Bear_Coop")), ("Type", s("Coop")), ("SubWeapon", s("")), ("SpecialWeapon", s("")), ("SpecialPoint", i(200)), ("DebugDispOrder", i(9))]),
                ]),
            ),
            (
                "RSDB/HairInfo.Product.b99.rstbl.byml.zs".into(),
                table(vec![
                    row(vec![("Id", i(0)), ("__RowId", s("Har_SQD000")), ("Order", i(20)), ("IsSquid", Byml::Bool(true))]),
                    row(vec![("Id", i(830)), ("__RowId", s("Har_OCT000_SdodrCstm00")), ("Order", i(120)), ("IsSquid", Byml::Bool(false))]),
                    row(vec![("Id", i(600)), ("__RowId", s("Har_OCT_RVL000")), ("Order", i(-1)), ("IsSquid", Byml::Bool(false))]),
                ]),
            ),
            ("RSDB/BynameAdjectiveInfo.Product.b30.rstbl.byml.zs".into(), table(vec![row(vec![("Id", i(3)), ("HowToGet", s("Shop")), ("Season", i(0))])])),
            ("RSDB/BynameSubjectInfo.Product.b30.rstbl.byml.zs".into(), table(vec![row(vec![("Id", i(3)), ("HowToGet", s("Shop")), ("Season", i(0))])])),
            ("UI/Icon/Gear/Hed_CAP000.bntx.zs".into(), raw_frame(b"BNTX")),
            // Stages (made-up scenes): a label without the digits, a name in one language, a stage the lists hide.
            (
                "RSDB/VersusSceneInfo.Product.b30.rstbl.byml.zs".into(),
                table(vec![
                    row(vec![("Id", i(3)), ("__RowId", s("Vss_Plaza03")), ("DisplayOrder", i(2)), ("IsDisplayInStageList", Byml::Bool(true))]),
                    row(vec![("Id", i(9)), ("__RowId", s("Vss_Hidden00")), ("DisplayOrder", i(9)), ("IsDisplayInStageList", Byml::Bool(false))]),
                    row(vec![("Id", i(1)), ("__RowId", s("Vss_Dock")), ("DisplayOrder", i(1)), ("IsDisplayInStageList", Byml::Bool(true))]),
                    row(vec![("Id", i(50)), ("__RowId", s("Cop_Nope")), ("DisplayOrder", i(0)), ("IsDisplayInStageList", Byml::Bool(true))]),
                ]),
            ),
            ("UI/Icon/StageL/Vss_Plaza.bntx.zs".into(), raw_frame(&crate::game::bntx::build::rgba8("p", 200, 100, &[40u8, 90, 160, 255].repeat(200 * 100)))),
            ("UI/Icon/StageL/Vss_Hidden00.bntx.zs".into(), raw_frame(b"BNTX")),
        ];
        // Color button animation (made-up colors): face color changes at frame 20, second color shown from 35.
        let anim = crate::game::bflan::build::encode(&[
            ("P_Btn_00", vec![(*b"FLMC", 0, 4, vec![(0.0, 255.0), (20.0, 1.0)]), (*b"FLMC", 0, 5, vec![(0.0, 0.0)]), (*b"FLMC", 0, 6, vec![(0.0, 16.0)])]),
            (
                "P_Btn_02",
                vec![(*b"FLMC", 0, 4, vec![(0.0, 0.0)]), (*b"FLMC", 0, 5, vec![(0.0, 0.0)]), (*b"FLMC", 0, 6, vec![(0.0, 255.0)]), (*b"FLTS", 1, 0, vec![(0.0, 1.0), (35.0, 0.5)])],
            ),
        ]);
        let blarc = sarc_build::encode(&[("anim/BtnSelectColor_00_Variation.bflan", &anim)]);
        files.push(("Layout/BtnSelectColor_00.Nin_NX_NVN.blarc.zs".into(), raw_frame(&blarc)));
        // Variant tag: "champion" + {"", "ne"}.
        let mut params = Vec::new();
        params.extend_from_slice(&0u16.to_le_bytes());
        params.extend_from_slice(&4u16.to_le_bytes());
        params.extend_from_slice(&msbt_build::text("ne"));
        params.extend_from_slice(&[0; 4]);
        let mut adjective = msbt_build::text("champion");
        adjective.extend_from_slice(&msbt_build::tag(201, 5, &params));
        for (region, head, brand, ability, adj, subj) in [
            ("EUfr", "Casquette", "Marque sept", "Bonus", adjective.clone(), "Adepte du Marqueur"),
            ("EUen", "Cap", "Brand seven", "Bonus EN", msbt_build::text("champion"), "Sploosh User"),
        ] {
            let msg = |entries: &[(&str, Vec<u8>)]| msbt_build::encode(entries);
            let head_m = msg(&[("CAP000", msbt_build::text(head)), ("COP001", msbt_build::text("Uniform"))]);
            let brand_m = msg(&[("B07", msbt_build::text(brand))]);
            let power_m = msg(&[("SubSpec_Up", msbt_build::text(ability))]);
            let adj_m = msg(&[("0003", adj)]);
            let subj_m = msg(&[("0003_0", msbt_build::text(subj)), ("0003_1", vec![])]);
            let wpn_m = msg(&[("Shooter_Short_00", msbt_build::text("Splash"))]);
            let stage_m = if region == "EUfr" {
                msg(&[("Plaza", msbt_build::text("La Place")), ("Dock", msbt_build::text("Le Quai"))])
            } else {
                msg(&[("Plaza", msbt_build::text("The Square"))])
            };
            let sarc = sarc_build::encode(&[
                ("CommonMsg/VS/VSStageName.msbt", &stage_m),
                ("CommonMsg/Gear/GearName_Head.msbt", &head_m),
                ("CommonMsg/Gear/GearBrandName.msbt", &brand_m),
                ("CommonMsg/Gear/GearPowerName.msbt", &power_m),
                ("CommonMsg/Byname/BynameAdjective.msbt", &adj_m),
                ("CommonMsg/Byname/BynameSubject.msbt", &subj_m),
                ("CommonMsg/Weapon/WeaponName_Main.msbt", &wpn_m),
            ]);
            // The build's name tag differs between builds (b20, b30...): names are found whatever the tag, even a mixed one.
            let tag = if region == "EUfr" { "b99" } else { "b30" };
            files.push((format!("Mals/{region}.Product.{tag}.sarc.zs"), raw_frame(&sarc)));
        }
        let dir = std::env::temp_dir().join(format!("s3l-cat-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut image, mut index) = (Vec::new(), Vec::new());
        for (path, bytes) in &files {
            index.push(format!(r#"{{"path":"{path}","offset":{},"size":{}}}"#, image.len(), bytes.len()));
            image.extend_from_slice(bytes);
        }
        std::fs::write(dir.join("romfs.bin"), &image).unwrap();
        std::fs::write(dir.join("index.json"), format!(r#"{{"romfs_sha256":"00ff","files":[{}]}}"#, index.join(","))).unwrap();
        (RomFs::open(&dir).unwrap(), dir)
    }

    #[test]
    fn builds_a_catalogue_from_a_synthetic_romfs() {
        let (rom, dir) = synthetic_rom();
        let c = build(&rom);
        // gear: Impossible rows are listed but not selectable
        let head = &c.categories["head"];
        assert_eq!(head.len(), 2);
        let cap = head.iter().find(|e| e.row == "Hed_CAP000").unwrap();
        assert!(cap.selectable);
        assert_eq!(cap.name["fr"], "Casquette");
        assert_eq!(cap.name["en"], "Cap");
        assert_eq!(cap.icon.as_deref(), Some("rom/UI/Icon/Gear/Hed_CAP000"));
        assert_eq!((cap.brand.as_deref(), cap.ability.as_deref(), cap.rarity), (Some("B07"), Some("SubSpec_Up"), Some(2)));
        assert_eq!(c.brands["B07"].name["fr"], "Marque sept");
        assert_eq!(c.abilities["SubSpec_Up"].name["en"], "Bonus EN");
        let cop = head.iter().find(|e| e.row == "Hed_COP001").unwrap();
        assert!(!cop.selectable);
        assert_eq!(cop.icon, None); // no icon file in the image
        // weapons: only Versus rows are selectable; sub and special come from the Gyml paths
        let w = &c.categories["weapon"];
        assert_eq!(w.iter().filter(|e| e.selectable).count(), 1);
        let shooter = w.iter().find(|e| e.selectable).unwrap();
        assert_eq!(
            (shooter.class.as_deref(), shooter.sub.as_deref(), shooter.special.as_deref(), shooter.points),
            (Some("Shooter"), Some("Bomb_Curling"), Some("SpUltraStamp"), Some(190))
        );
        assert_eq!(shooter.name["fr"], "Splash");
        // hair: Side Order duplicates and Order = -1 rows are not selectable; species from IsSquid
        let hair = &c.categories["hair"];
        assert_eq!(hair.iter().filter(|e| e.selectable).map(|e| e.id).collect::<Vec<_>>(), vec![0]);
        assert_eq!(hair.iter().find(|e| e.id == 0).unwrap().species.as_deref(), Some("inkling"));
        // titles: id 3 is read as label 0003; the French adjective keeps its default (masculine) form
        assert_eq!(c.categories["adjective"][0].name["fr"], "champion");
        assert_eq!(c.categories["adjective"][0].name["en"], "champion");
        assert_eq!(c.categories["subject"][0].name["fr"], "Adepte du Marqueur");
        // creation options without a table still have their id ranges
        assert_eq!(c.categories["skin"].len(), 9);
        assert_eq!(c.categories["eyes"].len(), 21);
        // their colors come from the color button animation: skin i = frame i, eyes i = frame 20 + i
        let colours = |cat: &str, id: usize| c.categories[cat][id].colors.clone();
        assert_eq!(colours("skin", 0), vec!["#ff0010"]);
        assert_eq!(colours("skin", 8), vec!["#ff0010"]);
        assert_eq!(colours("eyes", 0), vec!["#010010"]);
        assert_eq!(colours("eyes", 14), vec!["#010010"]);
        assert_eq!(colours("eyes", 15), vec!["#010010", "#0000ff"]);
        assert!(neutral().categories["skin"].iter().all(|e| e.colors.is_empty()));
        // stages: Vss_ scenes only, in the game's display order, named and pictured by their label
        let st = &c.stages;
        assert_eq!(st.iter().map(|s| s.scene.as_str()).collect::<Vec<_>>(), vec!["Vss_Dock", "Vss_Plaza03", "Vss_Hidden00"]);
        let plaza = &st[1];
        assert_eq!((plaza.id, plaza.listed, plaza.name["fr"].as_str(), plaza.name["en"].as_str()), (3, true, "La Place", "The Square"));
        assert_eq!(plaza.image.as_deref(), Some("rom/UI/Icon/StageL/Vss_Plaza"));
        assert_eq!((st[0].name.len(), st[0].name["fr"].as_str(), st[0].image.as_deref()), (1, "Le Quai", None), "one language, no picture");
        assert!(!st[2].listed && st[2].name.is_empty());
        assert_eq!(st[2].image.as_deref(), Some("rom/UI/Icon/StageL/Vss_Hidden00"), "a picture under the whole scene name");
        let sr = c.report.iter().find(|r| r.category == "stage").unwrap();
        assert_eq!((sr.rows, sr.selectable, sr.with_icon, sr.with_name), (3, 2, 1, 2));
        assert_eq!(
            stage_picture_keys(&c),
            vec!["rom/UI/Icon/StageL/Vss_Plaza@480", "rom/UI/Icon/StageL/Vss_Plaza@160", "rom/UI/Icon/StageL/Vss_Hidden00@480", "rom/UI/Icon/StageL/Vss_Hidden00@160"]
        );
        assert!(icon_keys(&c).contains(&"rom/UI/Icon/StageL/Vss_Plaza@160".to_string()));
        assert!(neutral().stages.is_empty());
        // report and cache key
        assert_eq!(c.report.iter().find(|r| r.category == "head").unwrap().selectable, 1);
        assert_eq!(c.cache_key, rom.cache_key);
        // the catalogue survives a JSON round trip (this is what the cache stores)
        let back: Catalogue = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.categories["head"], c.categories["head"]);
        drop(rom);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn service_caches_the_catalogue_by_key() {
        let (rom, dir) = synthetic_rom();
        drop(rom);
        let data = std::env::temp_dir().join(format!("s3l-data-{}", crate::util::random_hex(4)));
        let svc = crate::game::service::GameService::default();
        let first = svc.load(&dir, &data, false).unwrap();
        assert!(!first.from_cache);
        // Building the cache decoded the stage picture at both sizes (the other one is not a real texture).
        assert_eq!(first.store.stats().0, 2);
        let small = first.store.cached("rom/UI/Icon/StageL/Vss_Plaza@160").expect("miniature cached");
        assert_eq!((u32::from_be_bytes(small[16..20].try_into().unwrap()), u32::from_be_bytes(small[20..24].try_into().unwrap())), (160, 80));
        let big = first.store.cached("rom/UI/Icon/StageL/Vss_Plaza@480").expect("picture cached");
        assert_eq!(u32::from_be_bytes(big[16..20].try_into().unwrap()), 200, "never enlarged");
        assert_eq!(crate::game::service::cache_stage_pictures(&first), 0, "already there");
        let second = svc.load(&dir, &data, false).unwrap();
        assert!(second.from_cache);
        assert_eq!(second.catalogue.categories["head"].len(), 2);
        let forced = svc.load(&dir, &data, true).unwrap();
        assert!(!forced.from_cache);
        // A different image (new key) drops the stale cache directory.
        std::fs::write(dir.join("index.json"), std::fs::read_to_string(dir.join("index.json")).unwrap().replace("00ff", "ab12")).unwrap();
        let changed = svc.load(&dir, &data, false).unwrap();
        assert!(!changed.from_cache);
        assert_eq!(std::fs::read_dir(crate::game::service::cache_root(&data)).unwrap().count(), 1);
        drop((first, second, forced, changed));
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_damaged_or_foreign_cache_is_rebuilt() {
        let (rom, dir) = synthetic_rom();
        let key = rom.cache_key.clone();
        drop(rom);
        let data = std::env::temp_dir().join(format!("s3l-data-{}", crate::util::random_hex(4)));
        let svc = crate::game::service::GameService::default();
        assert!(!svc.load(&dir, &data, false).unwrap().from_cache);
        let file = crate::game::service::cache_root(&data).join(&key).join("catalogue.json");
        let good = std::fs::read(&file).unwrap();
        assert!(svc.load(&dir, &data, false).unwrap().from_cache);
        // Garbage, a cut file, another schema, a malformed icon key, a file too large to be a catalogue: rebuilt.
        let mut foreign: serde_json::Value = serde_json::from_slice(&good).unwrap();
        foreign["categories"]["head"][0]["icon"] = "rom/UI/Icon/../../../secret".into();
        let mut old: serde_json::Value = serde_json::from_slice(&good).unwrap();
        old["schema"] = (SCHEMA + 1).into();
        for bad in [b"\xFF\xFEgarbage".to_vec(), good[..good.len() / 2].to_vec(), serde_json::to_vec(&foreign).unwrap(), serde_json::to_vec(&old).unwrap()] {
            std::fs::write(&file, &bad).unwrap();
            let l = svc.load(&dir, &data, false).unwrap();
            assert!(!l.from_cache);
            assert_eq!(l.catalogue.categories["head"].len(), 2);
            assert!(svc.load(&dir, &data, false).unwrap().from_cache, "the rebuilt file is read back");
        }
        std::fs::File::options().write(true).open(&file).unwrap().set_len(33 << 20).unwrap();
        assert!(!svc.load(&dir, &data, false).unwrap().from_cache);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_build_that_ended_the_launcher_is_not_retried_at_every_start() {
        let (rom, dir) = synthetic_rom();
        let key = rom.cache_key.clone();
        drop(rom);
        let data = std::env::temp_dir().join(format!("s3l-data-{}", crate::util::random_hex(4)));
        let cache = crate::game::service::cache_root(&data).join(&key);
        std::fs::create_dir_all(&cache).unwrap();
        let svc = crate::game::service::GameService::default();
        // One unfinished build (the process died during it): built again, the marker cleared.
        std::fs::write(cache.join("building"), "1").unwrap();
        assert!(!svc.load(&dir, &data, false).unwrap().from_cache);
        assert!(!cache.join("building").exists());
        // Two in a row: the game data is not read again at start, the neutral catalogue will be shown instead.
        std::fs::remove_file(cache.join("catalogue.json")).unwrap();
        std::fs::write(cache.join("building"), crate::game::service::MAX_BUILD_ATTEMPTS.to_string()).unwrap();
        let Err(e) = svc.load(&dir, &data, false) else { panic!("no build expected") };
        assert!(e.contains("the last 2 builds of the catalogue from this game data did not finish"), "{e}");
        assert!(!cache.join("catalogue.json").exists());
        // An explicit rebuild tries again.
        assert!(!svc.load(&dir, &data, true).unwrap().from_cache);
        assert!(!cache.join("building").exists());
        assert!(svc.load(&dir, &data, false).unwrap().from_cache);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_crafted_checksum_cannot_move_the_cache_out_of_its_directory() {
        let (rom, dir) = synthetic_rom();
        drop(rom);
        let index = std::fs::read_to_string(dir.join("index.json")).unwrap().replace("00ff", "/../../../../../");
        std::fs::write(dir.join("index.json"), index).unwrap();
        let top = std::env::temp_dir().join(format!("s3l-trav-{}", crate::util::random_hex(4)));
        let data = top.join("a").join("b").join("c").join("d").join("e").join("data");
        let svc = crate::game::service::GameService::default();
        let l = svc.load(&dir, &data, false).unwrap();
        assert!(l.store.dir().starts_with(crate::game::service::cache_root(&data)));
        fn catalogues(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    catalogues(&p, out);
                } else if p.file_name().is_some_and(|n| n == "catalogue.json") {
                    out.push(p);
                }
            }
        }
        let mut found = Vec::new();
        catalogues(&top, &mut found);
        assert_eq!(found, vec![crate::game::service::cache_root(&data).join(&l.rom.cache_key).join("catalogue.json")]);
        drop(l);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(top).unwrap();
    }

    #[test]
    fn stage_labels_drop_the_prefix_and_the_version_digits() {
        assert_eq!(stage_label("Vss_Plaza03"), "Plaza");
        assert_eq!(stage_label("Vss_Dock"), "Dock");
        assert_eq!(stage_label("Vss_Twin2Towers00"), "Twin2Towers");
        assert_eq!(stage_label("Other"), "Other");
    }

    #[test]
    fn gyml_rows() {
        assert_eq!(gyml_row("Work/Gyml/PoisonMist.spl__WeaponInfoSub.gyml"), "PoisonMist");
        assert_eq!(gyml_row("Bomb_Suction"), "Bomb_Suction");
    }

    #[test]
    fn report_counts_only_selectable() {
        let v = vec![
            Entry { selectable: true, icon: Some("x".into()), name: [("en".to_string(), "A".to_string())].into(), ..Default::default() },
            Entry { selectable: false, icon: Some("y".into()), ..Default::default() },
        ];
        let r = report("t", &v, "");
        assert_eq!((r.rows, r.selectable, r.with_icon, r.with_name), (2, 1, 1, 1));
    }

    #[test]
    fn icon_keys_are_unique_and_selectable_only() {
        let mut c = Catalogue::default();
        c.categories.insert("a".into(), vec![
            Entry { selectable: true, icon: Some("rom/a".into()), ..Default::default() },
            Entry { selectable: true, icon: Some("rom/a".into()), ..Default::default() },
            Entry { selectable: false, icon: Some("rom/b".into()), ..Default::default() },
        ]);
        c.brands.insert("B00".into(), Named { icon: Some("rom/c".into()), ..Default::default() });
        assert_eq!(icon_keys(&c), vec!["rom/a".to_string(), "rom/c".to_string()]);
    }
}
