// A made-up catalogue for the browser preview: generic names and drawn shapes, nothing taken from a game.
import type { CatalogueView, GameCatalogue, GameEntry, GameStage, Named, StagesView } from "../../api";

type L = Record<string, string>;
const W = (fr: string, en: string, es: string, ja: string): L => ({ fr, en, es, ja });

const HEAD = [W("Bonnet", "Beanie", "Gorro", "ニット帽"), W("Casquette", "Cap", "Gorra", "キャップ"), W("Bandeau", "Headband", "Cinta", "ヘアバンド"), W("Chapeau", "Hat", "Sombrero", "ハット"), W("Visière", "Visor", "Visera", "サンバイザー")];
const CLOTHES = [W("Sweat", "Hoodie", "Sudadera", "パーカー"), W("Tee-shirt", "T-shirt", "Camiseta", "Tシャツ"), W("Veste", "Jacket", "Chaqueta", "ジャケット"), W("Maillot", "Jersey", "Maillot", "ジャージ"), W("Gilet", "Vest", "Chaleco", "ベスト")];
const SHOES = [W("Baskets", "Sneakers", "Zapatillas", "スニーカー"), W("Bottines", "Boots", "Botines", "ブーツ"), W("Sandales", "Sandals", "Sandalias", "サンダル"), W("Montantes", "High-tops", "Botas altas", "ハイカット")];
const WEAPONS = [W("Lanceur", "Blaster", "Lanzador", "ブラスター"), W("Rouleau", "Roller", "Rodillo", "ローラー"), W("Pinceau", "Brush", "Pincel", "ブラシ"), W("Seau", "Bucket", "Cubo", "バケツ"), W("Fusil", "Rifle", "Rifle", "ライフル"), W("Arc", "Bow", "Arco", "ボウ")];
const ADJ = [W("Rapide", "Swift", "Rápido", "すばやい"), W("Calme", "Calm", "Tranquilo", "おだやかな"), W("Joyeux", "Cheerful", "Alegre", "陽気な"), W("Rusé", "Sly", "Astuto", "ずる賢い"), W("Fidèle", "Loyal", "Leal", "忠実な"), W("Lumineux", "Bright", "Brillante", "まぶしい"), W("Têtu", "Stubborn", "Terco", "頑固な"), W("Discret", "Quiet", "Discreto", "ひかえめな")];
const SUBJ = [W("Pilote", "Pilot", "Piloto", "パイロット"), W("Peintre", "Painter", "Pintor", "ペインター"), W("Rêveur", "Dreamer", "Soñador", "夢見る人"), W("Gardien", "Keeper", "Guardián", "番人"), W("Voyageur", "Traveller", "Viajero", "旅人"), W("Artiste", "Artist", "Artista", "アーティスト"), W("Capitaine", "Captain", "Capitán", "キャプテン"), W("Rival", "Rival", "Rival", "ライバル")];
const BRANDS = ["Nordo", "Kappa Line", "Velo", "Orbe", "Tamis", "Zenith"];
const ABILITIES = [W("Vitesse", "Speed", "Velocidad", "スピード"), W("Défense", "Defence", "Defensa", "防御"), W("Économie", "Saver", "Ahorro", "節約"), W("Saut", "Jump", "Salto", "ジャンプ"), W("Recharge", "Recharge", "Recarga", "回復"), W("Ouverture", "Opener", "Apertura", "開幕"), W("Revanche", "Comeback", "Remontada", "逆転")];
/** The made-up abilities a sub slot can hold: the first five (the last two are main-only, as in the real game some are). */
export const MOCK_SUB_ABILITIES = ["a0", "a1", "a2", "a3", "a4"];
/** The made-up abilities a main slot can hold: all seven. */
export const MOCK_MAIN_ABILITIES = ["a0", "a1", "a2", "a3", "a4", "a5", "a6"];
const KINDS = [W("Tir", "Shooter", "Tirador", "シューター"), W("Rouleau", "Roller", "Rodillo", "ローラー"), W("Précision", "Charger", "Precisión", "チャージャー"), W("Seau", "Slosher", "Cubo", "スロッシャー")];
const SUBS = [W("Bombe", "Bomb", "Bomba", "ボム"), W("Balise", "Beacon", "Baliza", "ビーコン"), W("Mur", "Wall", "Muro", "ウォール"), W("Mine", "Mine", "Mina", "トラップ")];
const SPECIALS = [W("Tornade", "Tornado", "Tornado", "トルネード"), W("Bouclier", "Shield", "Escudo", "シールド"), W("Vague", "Wave", "Ola", "ウェーブ"), W("Fusée", "Rocket", "Cohete", "ロケット")];

const SKIN = ["#ffe0c4", "#f6cfa8", "#e9b48a", "#d29a6c", "#b57c52", "#8e5b38", "#6d4228", "#4e2e1c", "#f3dccf"];
const EYES = ["#3a2a20", "#5b3c1f", "#2f6db5", "#2fa36b", "#9b3fb5", "#c23b3b", "#d18f1d", "#1d8f8f", "#7a7a7a", "#e05fa0", "#4c4cc9", "#2b2b2b", "#8a5a2b", "#1fa0c9", "#9cc93b", "#c96b1f", "#5a1f7a", "#e0c21f", "#1f7a4c", "#7a1f2b", "#3b9bc9"];

const named = (list: L[], prefix: string): Record<string, Named> =>
  Object.fromEntries(list.map((n, i) => [`${prefix}${i}`, { name: n, icon: `mock/${prefix}/${i}` }]));

function entry(id: number, row: string, name: L, extra: Partial<GameEntry> = {}): GameEntry {
  return {
    id, row, name, icon: `mock/${row}`, icon_approx: false, order: id, season: (id % 5) + 1, selectable: true, how_to_get: null, species: null,
    brand: null, rarity: id % 4, ability: null, class: null, sub: null, special: null, points: null, variations: null, unavailable: null, ...extra,
  };
}

function numbered(kind: string, bases: L[], n: number, extra: (i: number) => Partial<GameEntry> = () => ({})): GameEntry[] {
  return Array.from({ length: n }, (_, i) => {
    const b = bases[i % bases.length];
    const k = Math.floor(i / bases.length) + 1;
    const name = Object.fromEntries(Object.entries(b).map(([lang, w]) => [lang, `${w} ${k}`]));
    return entry(i, `${kind}/${i}`, name, extra(i));
  });
}

export function catalogue(): CatalogueView {
  const cat: GameCatalogue = {
    schema: 1, cache_key: "mock", build_ms: 12,
    categories: {
      style: [0, 1, 2, 3].map((i) => entry(i, `style/${i}`, W(`Style ${i + 1}`, `Style ${i + 1}`, `Estilo ${i + 1}`, `スタイル ${i + 1}`), { species: i < 2 ? "inkling" : "octoling" })),
      skin: SKIN.map((c, i) => entry(i, `skin/${i}`, W("", "", "", ""), { colors: [c], icon: null })),
      eyes: EYES.map((c, i) => entry(i, `eyes/${i}`, W("", "", "", ""), { colors: [c], icon: null })),
      hair: Array.from({ length: 10 }, (_, i) => entry(i, `hair/${i}`, W("", "", "", ""), { species: i < 6 ? "inkling" : "octoling" })),
      eyebrows: Array.from({ length: 6 }, (_, i) => entry(i, `brow/${i}`, W("", "", "", ""))),
      legwear: Array.from({ length: 8 }, (_, i) => entry(i, `leg/${i}`, W(`Bas ${i + 1}`, `Legwear ${i + 1}`, `Medias ${i + 1}`, `ボトムス ${i + 1}`), { icon: null, variations: i % 3 === 0 ? 3 : 1 })),
      head: numbered("head", HEAD, 45, (i) => ({ brand: `b${i % BRANDS.length}`, ability: `a${i % ABILITIES.length}`, unavailable: i === 7 ? "game_drops" : null })),
      clothes: numbered("clothes", CLOTHES, 45, (i) => ({ brand: `b${(i + 2) % BRANDS.length}`, ability: `a${(i + 1) % ABILITIES.length}` })),
      shoes: numbered("shoes", SHOES, 36, (i) => ({ brand: `b${(i + 4) % BRANDS.length}`, ability: `a${(i + 3) % ABILITIES.length}` })),
      weapon: numbered("weapon", WEAPONS, 36, (i) => ({ class: `k${i % KINDS.length}`, sub: `s${i % SUBS.length}`, special: `p${(i + 1) % SPECIALS.length}` })),
      banner: Array.from({ length: 24 }, (_, i) => entry(i, `banner/Npl_Catalog_Season0${(i % 3) + 1}_${String(i).padStart(4, "0")}`, W("", "", "", ""), { class: `Npl_Catalog_Season0${(i % 3) + 1}` })),
      adjective: ADJ.map((w, i) => entry(i, `adj/${i}`, w, { icon: null })),
      subject: SUBJ.map((w, i) => entry(i, `subj/${i}`, w, { icon: null })),
      badge: Array.from({ length: 30 }, (_, i) => entry(i, `badge/${i}`, W(`Badge ${i + 1}`, `Badge ${i + 1}`, `Insignia ${i + 1}`, `バッジ ${i + 1}`), { class: ["Challenge", "Season", "Club"][i % 3], unavailable: "badge_record" })),
    },
    brands: Object.fromEntries(BRANDS.map((n, i) => [`b${i}`, { name: { fr: n, en: n, es: n, ja: n }, icon: `mock/brand/${i}` }])),
    abilities: named(ABILITIES, "a"),
    subs: named(SUBS, "s"),
    specials: named(SPECIALS, "p"),
    classes: named(KINDS, "k"),
    stages: STAGES,
    report: [],
  };
  return { catalogue: cat, origin: "game", reason: null, from_cache: true, image: "D:\\Games\\Kalamar\\work\\game\\romfs.bin", name_max_chars: 10 };
}

// ---- stages: made-up places with drawn pictures (gradients and shapes), in the scene-id style of the engine's list --
const STAGE_NAMES: [string, L, boolean][] = [
  ["Vss_Docks00", W("Les Docks", "The Docks", "Los Muelles", "ドック"), true],
  ["Vss_Park", W("Le Parc", "The Park", "El Parque", "パーク"), true],
  ["Vss_Market01", W("Marché couvert", "Covered Market", "Mercado cubierto", "屋根つき市場"), true],
  ["Vss_GlassTower", W("Tour de verre", "Glass Tower", "Torre de cristal", "ガラスの塔"), true],
  ["Vss_Garden02", W("Jardin suspendu", "Hanging Garden", "Jardín colgante", "空中庭園"), true],
  ["Vss_Quarry", W("Carrière rouge", "Red Quarry", "Cantera roja", "赤い採石場"), true],
  ["Vss_LiftBridge00", W("Pont levant", "Lift Bridge", "Puente levadizo", "跳ね橋"), true],
  ["Vss_MistStation", W("Gare des Brumes", "Mist Station", "Estación de la Bruma", "霧の駅"), true],
  ["Vss_SaltDunes03", W("Dunes salées", "Salt Dunes", "Dunas saladas", "塩の砂丘"), true],
  ["Vss_SkatePark", W("Rampe des Flots", "Wave Ramp Skatepark", "Rampa de las Olas", "波のスケートパーク"), true],
  ["Vss_LanternHarbor", W("Port des Lanternes", "Lantern Harbour", "Puerto de los Faroles", "灯籠の港"), true],
  ["Vss_Lab00", W("Laboratoire", "The Lab", "El Laboratorio", "研究所"), false],
];
export const STAGES: GameStage[] = STAGE_NAMES.map(([scene, name, pic], i) => ({ scene, id: i + 1, name, image: pic ? `mock/stage/${scene}` : null, listed: true, order: i + 1 }));
export const STAGE_IDS = STAGE_NAMES.map(([s]) => s);

/** `gamedata_stages`: the made-up stages, or (`none`) what the launcher gets without game data. */
export function stagesView(none: boolean): StagesView {
  return none
    ? { stages: [], origin: "neutral", reason: "no romfs.bin and index.json found under D:\\Games\\Kalamar\\work\\splatoon3-11.3.0", sizes: [480, 160] }
    : { stages: STAGES, origin: "game", reason: null, sizes: [480, 160] };
}

/** A made-up 16:9 picture of a place: a sky, a skyline, a floor in perspective and two colors of paint. */
function stagePicture(key: string): string {
  let h = 7;
  for (const c of key) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  let seed = h || 1;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) % 10000) / 10000;
  const sky = h % 360;
  const ground = (sky + 150 + Math.floor(rnd() * 60)) % 360;
  const inkA = ["#ff3190", "#ffd631", "#ff7b24", "#9b5cff"][h % 4];
  const inkB = ["#1ff2c0", "#4fa6ff", "#7cff4f", "#2ee6e6"][(h >> 3) % 4];
  const horizon = 120 + Math.floor(rnd() * 40);
  let city = "";
  for (let x = -10; x < 480; ) {
    const w = 22 + Math.floor(rnd() * 46), top = horizon - 20 - Math.floor(rnd() * 90);
    city += `<rect x="${x}" y="${top}" width="${w}" height="${horizon - top + 8}" fill="hsl(${ground} 22% ${18 + Math.floor(rnd() * 14)}%)"/>`;
    if (rnd() > 0.5) city += `<rect x="${x + 5}" y="${top + 8}" width="${Math.max(4, w - 10)}" height="3" fill="hsl(${sky} 90% 80%)" opacity=".5"/>`;
    x += w + Math.floor(rnd() * 8);
  }
  let ink = "";
  for (let i = 0; i < 9; i++) {
    const x = 40 + rnd() * 400, y = horizon + 40 + rnd() * (230 - horizon), rx = 18 + rnd() * 50;
    ink += `<ellipse cx="${x.toFixed(0)}" cy="${y.toFixed(0)}" rx="${rx.toFixed(0)}" ry="${(rx * 0.38).toFixed(0)}" fill="${i % 2 ? inkA : inkB}" opacity=".88"/>`;
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 480 270" preserveAspectRatio="xMidYMid slice">`
    + `<defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${sky} 70% 58%)"/><stop offset="1" stop-color="hsl(${(sky + 30) % 360} 65% 84%)"/></linearGradient>`
    + `<linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${ground} 18% 52%)"/><stop offset="1" stop-color="hsl(${ground} 22% 30%)"/></linearGradient></defs>`
    + `<rect width="480" height="270" fill="url(#s)"/><circle cx="${(60 + rnd() * 360).toFixed(0)}" cy="${(30 + rnd() * 40).toFixed(0)}" r="26" fill="#fff6d0" opacity=".75"/>`
    + city
    + `<polygon points="0,${horizon + 6} 480,${horizon - 4} 480,270 0,270" fill="url(#g)"/>`
    + `<polygon points="150,${horizon + 30} 330,${horizon + 26} 470,270 10,270" fill="hsl(${ground} 12% 64%)" opacity=".55"/>`
    + ink + `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/** A drawn placeholder for an icon key: a colored shape (banners: a wide gradient; stages: a made-up place). */
export function iconDataUrl(key: string): string {
  if (key.startsWith("mock/stage/")) return stagePicture(key.replace(/@\d+$/, ""));
  let h = 0;
  for (const c of key) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = h % 360;
  const hue2 = (hue + 70) % 360;
  if (key.includes("banner/")) {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 350 100"><defs><linearGradient id="g" x1="0" x2="1"><stop offset="0" stop-color="hsl(${hue} 70% 45%)"/><stop offset="1" stop-color="hsl(${hue2} 75% 55%)"/></linearGradient></defs><rect width="350" height="100" fill="url(#g)"/><circle cx="${60 + (h % 200)}" cy="${30 + (h % 40)}" r="42" fill="hsl(${hue2} 90% 70%)" opacity=".35"/></svg>`;
    return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  }
  const r = 26 + (h % 10);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect x="${50 - r}" y="${50 - r}" width="${2 * r}" height="${2 * r}" rx="${r / 2.2}" fill="hsl(${hue} 70% 55%)"/><circle cx="62" cy="40" r="${r / 2.6}" fill="hsl(${hue2} 85% 75%)"/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
