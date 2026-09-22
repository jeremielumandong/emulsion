#!/usr/bin/env node
// Build the community recipe library shipped in
// crates/emulsion-recipes/library/ from two public collections:
//
//   Open Fuji Recipes      https://github.com/matthieurobin/open-fuji-recipes
//   fujifilm-recipes (FP1) https://github.com/akirichev/fujifilm-recipes
//
// Usage: node scripts/gen-recipe-library.js <open-fuji-recipes dir> <fujifilm-recipes dir>
//
// Recipes are grouped into one bundle per film simulation family. Names are
// made unique across the built-in sets and the library; X RAW Studio profiles
// whose settings exactly match an Open Fuji recipe are dropped so the
// original creator keeps the credit.
const fs = require("fs");
const path = require("path");

const [openDir, fp1Dir] = process.argv.slice(2);
if (!openDir || !fp1Dir) {
  console.error("usage: gen-recipe-library.js <open-fuji-recipes dir> <fujifilm-recipes dir>");
  process.exit(2);
}
const OPEN = path.join(openDir, "src/stores/data/recipes.js");
const FP1_DIR = path.join(fp1Dir, "X-T4/X-T4_0100");
const OUT = path.join(__dirname, "..", "crates/emulsion-recipes/library");

// Names already taken by starter_set() and cameras::presets().
const BUILTIN = [
  "Chrome Street", "Negative Summer", "Slide Punch", "Portrait Soft", "Cinema Flat",
  "Bleach Street", "Nostalgic Amber", "Acros Grain", "Faithful", "Sepia Memory",
  "Polaroid SX-70", "Instax Mini", "Disposable camera", "Kodak Gold 200", "Fuji Superia 400",
  "Kodak Portra 400", "CineStill 800T", "Ilford HP5 Plus", "Kodak Tri-X 400", "Lomo cross-process",
  "Old print", "Expired film", "Colour negative strip", "Early digicam", "Point and shoot '98",
];

const OPEN_FILM = {
  "ACROS": "acros", "ACROS +G": "acros-g", "ACROS +R": "acros-r", "ACROS +Y": "acros-ye", "ACROS +B": "acros",
  "ASTIA/Soft": "astia", "Classic Chrome": "classic-chrome", "Classic Neg": "classic-negative",
  "ETERNA/Cinema": "eterna", "MONOCHROME": "monochrome", "MONOCHROME +G": "monochrome-g",
  "MONOCHROME +R": "monochrome-r", "MONOCHROME +Y": "monochrome-ye", "Pro Neg Hi": "pro-neg-hi",
  "Pro Neg Std": "pro-neg-std", "PROVIA/Standard": "provia", "SEPIA": "sepia", "VELVIA/Vivid": "velvia",
};
const FP1_FILM = {
  provia: "provia", velvia: "velvia", astia: "astia", classic: "classic-chrome", classicnega: "classic-negative",
  negastd: "pro-neg-std", negahi: "pro-neg-hi", eterna: "eterna", bleachbypass: "eterna-bleach-bypass",
  nostalgic: "nostalgic-negative", reala: "reala-ace", acros: "acros", acrosye: "acros-ye", acrosr: "acros-r",
  acrosg: "acros-g", b: "monochrome", by: "monochrome-ye", br: "monochrome-r", bg: "monochrome-g", sepia: "sepia",
};
const family = (sim) => {
  if (sim.startsWith("acros") || sim.startsWith("monochrome") || sim === "sepia") return "monochrome";
  if (sim.startsWith("pro-neg")) return "pro-neg";
  if (sim.startsWith("eterna")) return "eterna";
  return sim;
};
const GROUPS = {
  "classic-chrome": ["Classic Chrome", "Community recipes built on Classic Chrome: muted, documentary colour with restrained saturation."],
  "classic-negative": ["Classic Negative", "Community recipes built on Classic Negative: consumer-film colour with a hard tone curve."],
  "provia": ["Provia", "Community recipes built on Provia / Standard: neutral, general-purpose colour."],
  "velvia": ["Velvia", "Community recipes built on Velvia / Vivid: saturated slide-film colour."],
  "astia": ["Astia", "Community recipes built on Astia / Soft: gentle contrast with soft skin tones."],
  "pro-neg": ["Pro Neg", "Community recipes built on Pro Neg. Std and Pro Neg. Hi: portrait negative film, flat or with a little punch."],
  "eterna": ["Eterna", "Community recipes built on Eterna / Cinema: low-contrast, desaturated motion-picture colour."],
  "monochrome": ["Black and white", "Community recipes built on Acros, Monochrome and Sepia, with and without colour filters."],
};

const q = (s) => JSON.stringify(s);
const tone = (v) => Number(v).toFixed(1);
const ev = (v) => (v === 0 || v === "0" || v == null ? "" : String(v).replace(/\/1$/, ""));
const strength = (v) => (v.startsWith("strong") ? "strong" : v.startsWith("weak") ? "weak" : "off");

function fromOpen(r) {
  const sim = OPEN_FILM[r.film];
  if (!sim) throw new Error("film " + r.film);
  const notes = ["From the Open Fuji Recipes community list."];
  let dr;
  switch (r.dr) {
    case "DR100": dr = "dr100"; break;
    case "DR200": dr = "dr200"; break;
    case "DR400": dr = "dr400"; break;
    case "AUTO": dr = "dr200"; notes.push("Dynamic range: Auto in camera (DR200 here)."); break;
    case "WEAK": dr = "dr200"; notes.push("D-Range Priority: Weak in camera (DR200 here)."); break;
    case "STRONG": dr = "dr400"; notes.push("D-Range Priority: Strong in camera (DR400 here)."); break;
    default: throw new Error("dr " + r.dr);
  }
  const wb = { preset: "auto", kelvin: null, red: r.wbr, blue: r.wbb };
  const w = r.wb.toLowerCase();
  if (/^\d+k$/.test(w)) { wb.preset = "kelvin"; wb.kelvin = parseInt(w, 10); }
  else if (w === "fl light 1") wb.preset = "fluorescent-1";
  else if (w === "fl light 2") wb.preset = "fluorescent-2";
  else if (w === "c1") notes.push("Custom (measured) white balance in camera; Auto here.");
  else if (["auto", "daylight", "shade", "cloudy", "incandescent", "underwater"].includes(w)) wb.preset = w;
  else throw new Error("wb " + w);
  const grain = r.grain.toLowerCase();
  return {
    name: r.name.trim(),
    author: r.creator,
    source_url: "https://openfujirecipes.com/",
    license: "Community-shared camera settings, collected by Open Fuji Recipes",
    notes: notes.join(" "),
    sensor: [r.sensor === 4 ? "X-Trans IV" : "X-Trans III"],
    tags: [sim, r.type === "blackWhite" ? "black and white" : "colour", "x-trans " + r.sensor],
    film_simulation: sim,
    dynamic_range: dr,
    grain: { strength: strength(grain), size: grain.endsWith(" l") ? "large" : "small" },
    cce: strength(r.ccfx),
    ccb: strength(r.ccfxb),
    wb,
    highlight: r["h-tone"], shadow: r["s-tone"], color: r.color, sharpness: r.sharp,
    noise_reduction: r.nr, clarity: r.clarity, exposure: ev(r.ev),
    origin: "open",
  };
}

// "P0P33" is +0.33 EV, "M1P00" is -1 EV.
function fp1Exposure(v) {
  if (!v || v === "0") return "";
  const m = /^([PM])(\d+)P(\d+)$/.exec(v);
  if (!m) return v;
  const sign = m[1] === "P" ? "+" : "-";
  const whole = parseInt(m[2], 10);
  const frac = parseInt(m[3], 10) / 100;
  const thirds = { 0: "", 0.33: "1/3", 0.67: "2/3", 0.5: "1/2" }[frac];
  if (thirds === undefined) return sign + (whole + frac);
  if (whole === 0) return thirds ? sign + thirds : "";
  return sign + whole + (thirds ? " " + thirds : "");
}

function fromFp1(file) {
  const xml = fs.readFileSync(file, "utf8");
  const tag = (n) => {
    const m = new RegExp("<" + n + ">([^<]*)</" + n + ">").exec(xml);
    return m ? m[1].trim() : null;
  };
  const label = /label="([^"]*)"/.exec(xml)[1];
  const simRaw = tag("FilmSimulation");
  const sim = FP1_FILM[simRaw.toLowerCase()];
  if (!sim) throw new Error("fp1 film " + simRaw);
  const wb = { preset: "auto", kelvin: null, red: parseInt(tag("WBShiftR"), 10), blue: parseInt(tag("WBShiftB"), 10) };
  const notes = ["X RAW Studio profile from the fujifilm-recipes collection by Alexander Kirichev (recipes from Fuji X Weekly and F16)."];
  switch (tag("WhiteBalance")) {
    case "Auto": break;
    case "INVALID": notes.push("White balance as shot in camera; Auto here."); break;
    case "Daylight": wb.preset = "daylight"; break;
    case "Shade": wb.preset = "shade"; break;
    case "Cloudy": wb.preset = "cloudy"; break;
    case "FLight1": wb.preset = "fluorescent-1"; break;
    case "FLight2": wb.preset = "fluorescent-2"; break;
    case "FLight3": wb.preset = "fluorescent-3"; break;
    case "Incandescent": wb.preset = "incandescent"; break;
    case "Underwater": wb.preset = "underwater"; break;
    case "Temperature": wb.preset = "kelvin"; wb.kelvin = parseInt(tag("WBColorTemp"), 10); break;
    default: throw new Error("fp1 wb " + tag("WhiteBalance"));
  }
  const drRaw = tag("DynamicRange");
  const bw = /^(acros|monochrome|sepia)/.test(sim);
  return {
    name: label,
    author: "Alexander Kirichev (fujifilm-recipes)",
    source_url: "https://github.com/akirichev/fujifilm-recipes",
    license: "Community-shared camera settings, provided as is",
    notes: notes.join(" "),
    sensor: ["X-Trans IV"],
    tags: [sim, bw ? "black and white" : "colour", "x-trans 4", "x raw studio"],
    film_simulation: sim,
    dynamic_range: drRaw === "400" ? "dr400" : drRaw === "200" ? "dr200" : "dr100",
    grain: {
      strength: strength(tag("GrainEffect").toLowerCase()),
      size: tag("GrainEffectSize").toLowerCase() === "large" ? "large" : "small",
    },
    cce: strength((tag("ChromeEffect") || "off").toLowerCase()),
    ccb: strength((tag("ColorChromeBlue") || "off").toLowerCase()),
    wb,
    highlight: +tag("HighlightTone"), shadow: +tag("ShadowTone"), color: +tag("Color"), sharpness: +tag("Sharpness"),
    noise_reduction: +tag("NoisReduction"), clarity: +tag("Clarity"), exposure: fp1Exposure(tag("ExposureBias")),
    origin: "fp1",
  };
}

const settingsKey = (r) => JSON.stringify([
  r.film_simulation, r.dynamic_range, r.grain, r.cce, r.ccb, r.wb, r.highlight, r.shadow,
  r.color, r.sharpness, r.noise_reduction, r.clarity, r.exposure,
]);

const src = fs.readFileSync(OPEN, "utf8").replace(/^[\s\S]*?export default/, "");
const open = eval(src).map(fromOpen); // eslint-disable-line no-eval
const fp1Files = fs.readdirSync(FP1_DIR).filter((f) => f.endsWith(".FP1")).sort();
let fp1 = fp1Files.map((f) => fromFp1(path.join(FP1_DIR, f)));

const openKeys = new Map(open.map((r) => [settingsKey(r), r]));
const dropped = [];
fp1 = fp1.filter((r) => {
  const o = openKeys.get(settingsKey(r));
  if (o) { dropped.push(`${r.name} == ${o.name} (${o.author})`); return false; }
  return true;
});

const all = [...open, ...fp1];
const count = new Map(BUILTIN.map((n) => [n.toLowerCase(), 1]));
for (const r of all) count.set(r.name.toLowerCase(), (count.get(r.name.toLowerCase()) || 0) + 1);
for (const r of all) {
  if (count.get(r.name.toLowerCase()) > 1) {
    r.name = `${r.name} · ${r.origin === "fp1" ? "X RAW Studio" : r.author}`;
  }
}
const seen = new Set(BUILTIN.map((n) => n.toLowerCase()));
for (const r of all) {
  let n = r.name;
  let i = 2;
  // Two recipes of the same name by the same author: tell them apart by
  // sensor generation first, then by number.
  if (seen.has(n.toLowerCase())) n = `${r.name} (${r.sensor[0]})`;
  while (seen.has(n.toLowerCase())) n = `${r.name} ${i++}`;
  r.name = n;
  seen.add(n.toLowerCase());
}
all.sort((a, b) => a.name.localeCompare(b.name, "en"));

function emit(r) {
  const lines = [
    "[[recipes]]",
    `name = ${q(r.name)}`,
    `author = ${q(r.author)}`,
    `source_url = ${q(r.source_url)}`,
    `license = ${q(r.license)}`,
    `notes = ${q(r.notes)}`,
    `sensor = [${r.sensor.map(q).join(", ")}]`,
    `tags = [${r.tags.map(q).join(", ")}]`,
    `film_simulation = ${q(r.film_simulation)}`,
    `dynamic_range = ${q(r.dynamic_range)}`,
  ];
  if (r.grain.strength !== "off") lines.push(`grain = { strength = ${q(r.grain.strength)}, size = ${q(r.grain.size)} }`);
  if (r.cce !== "off") lines.push(`color_chrome_effect = ${q(r.cce)}`);
  if (r.ccb !== "off") lines.push(`color_chrome_fx_blue = ${q(r.ccb)}`);
  const wbParts = [`preset = ${q(r.wb.preset)}`];
  if (r.wb.kelvin != null) wbParts.push(`kelvin = ${r.wb.kelvin}`);
  wbParts.push(`red = ${r.wb.red}`, `blue = ${r.wb.blue}`);
  lines.push(`white_balance = { ${wbParts.join(", ")} }`);
  for (const k of ["highlight", "shadow", "color", "sharpness", "noise_reduction", "clarity"]) {
    if (r[k] !== 0) lines.push(`${k} = ${tone(r[k])}`);
  }
  if (r.exposure) lines.push(`exposure_compensation = ${q(r.exposure)}`);
  return lines.join("\n");
}

fs.mkdirSync(OUT, { recursive: true });
for (const f of fs.readdirSync(OUT)) if (f.endsWith(".toml")) fs.unlinkSync(path.join(OUT, f));
const summary = [];
for (const [key, [name, notes]] of Object.entries(GROUPS)) {
  const rs = all.filter((r) => family(r.film_simulation) === key);
  const head =
    "# Generated by scripts/gen-recipe-library.js from Open Fuji Recipes and\n" +
    "# akirichev/fujifilm-recipes; do not edit by hand.\n" +
    `name = ${q(name)}\nauthor = "Fujifilm recipe community"\nnotes = ${q(notes)}\nversion = 1\n\n`;
  fs.writeFileSync(path.join(OUT, key + ".toml"), head + rs.map(emit).join("\n\n") + "\n");
  summary.push(`${key}: ${rs.length}`);
}
console.log(`open ${open.length}, fp1 ${fp1.length}, dropped duplicates ${dropped.length}`);
for (const d of dropped) console.log("  " + d);
console.log(summary.join("\n"));
console.log("renamed: " + all.filter((r) => r.name.includes(" · ")).map((r) => r.name).join(" | "));
