// Contrôle statique des traductions de l'interface : chaque `t("…")` et `tp(n, "…", "…")`
// des fichiers web, et chaque texte marqué `data-i18n` / `data-i18n-attr` d'index.html,
// doit avoir son entrée anglaise dans web/i18n.js ; aucune entrée ne doit être inutilisée.
//
//   node tests/ui/i18n-check.cjs        (code de sortie 1 au moindre écart)

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const WEB = path.join(__dirname, "..", "..", "web");
const SOURCES = ["app.js", "clip.js", "pieces.js", "theme.js", "i18n.js"];
const problems = [];

// ── Le dictionnaire, lu en exécutant i18n.js dans un faux navigateur anglais ──
function loadDict() {
  const doc = {
    documentElement: { lang: "en" },
    readyState: "loading",
    addEventListener() {},
    querySelectorAll: () => [],
  };
  const sandbox = { document: doc, Intl, Date, Math, Number, String, Object, Array, console };
  sandbox.globalThis = sandbox;
  sandbox.window = sandbox;
  vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(path.join(WEB, "i18n.js"), "utf8"), sandbox, { filename: "i18n.js" });
  const I = sandbox.I18N;
  if (!I || !I.dict) throw new Error("i18n.js n'expose pas I18N.dict");
  return { I, dict: I.dict };
}

// ── Les clés utilisées ──
const LIT = String.raw`("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')`;
const T_CALL = new RegExp(String.raw`(?<![\w.$])t\(\s*` + LIT, "g");
const T_ANY = /(?<![\w.$])t\(\s*([^\s)])/g;
const TP_CALL = new RegExp(String.raw`(?<![\w.$])tp\(\s*[^,]+?,\s*` + LIT + String.raw`\s*,\s*` + LIT, "g");
const TP_ANY = /(?<![\w.$])tp\(/g;
const lit = (s) => vm.runInNewContext(s);
const lineOf = (src, i) => src.slice(0, i).split("\n").length;

const used = new Map(); // clé → { plural, where, many }
function use(key, where, plural = false, many = null) {
  const u = used.get(key);
  if (u && u.plural !== plural) problems.push(`${where} : « ${key} » sert à la fois en t() et en tp()`);
  if (!u) used.set(key, { plural, where, many });
}

for (const file of SOURCES) {
  const src = fs.readFileSync(path.join(WEB, file), "utf8");
  // Les définitions de i18n.js (function t(fr, vars)…) ne sont pas des appels à contrôler.
  const own = file === "i18n.js";
  let m;
  T_CALL.lastIndex = 0;
  const literalAt = new Set();
  while ((m = T_CALL.exec(src))) {
    literalAt.add(m.index);
    use(lit(m[1]), `${file}:${lineOf(src, m.index)}`);
  }
  T_ANY.lastIndex = 0;
  while ((m = T_ANY.exec(src))) {
    if (literalAt.has(m.index) || own) continue;
    problems.push(`${file}:${lineOf(src, m.index)} : t() sans texte littéral (clé invisible au contrôle)`);
  }
  TP_CALL.lastIndex = 0;
  const tpAt = new Set();
  while ((m = TP_CALL.exec(src))) {
    tpAt.add(m.index);
    use(lit(m[1]), `${file}:${lineOf(src, m.index)}`, true, lit(m[2]));
  }
  TP_ANY.lastIndex = 0;
  while ((m = TP_ANY.exec(src))) {
    if (tpAt.has(m.index) || own) continue;
    problems.push(`${file}:${lineOf(src, m.index)} : tp() sans textes littéraux`);
  }
}

// index.html : le texte des éléments `data-i18n`, les attributs listés par `data-i18n-attr`.
{
  const html = fs.readFileSync(path.join(WEB, "index.html"), "utf8");
  const decode = (s) => s.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
  for (const m of html.matchAll(/<(\w+)([^>]*?)\sdata-i18n(?:\s[^>]*)?>([^<]*)<\/\1>/g)) {
    use(decode(m[3].trim()), `index.html:${lineOf(html, m.index)}`);
  }
  for (const m of html.matchAll(/<\w+[^>]*\sdata-i18n-attr="([^"]+)"[^>]*>/g)) {
    for (const attr of m[1].split(/\s+/)) {
      const v = m[0].match(new RegExp(`\\s${attr}="([^"]*)"`));
      if (!v) problems.push(`index.html:${lineOf(html, m.index)} : attribut ${attr} absent`);
      else use(decode(v[1]), `index.html:${lineOf(html, m.index)}`);
    }
  }
}

// ── Les écarts ──
const { I, dict } = loadDict();
const vars = (s) => [...String(s).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
let missing = 0;
for (const [key, u] of used) {
  const e = dict[key];
  if (!key) {
    problems.push(`${u.where} : clé vide`);
    continue;
  }
  if (e === undefined) {
    missing++;
    problems.push(`${u.where} : pas d'anglais pour « ${key} »${u.plural ? " (pluriel)" : ""}`);
    continue;
  }
  if (u.plural) {
    if (!Array.isArray(e) || e.length !== 2 || !e[0] || !e[1]) {
      problems.push(`${u.where} : « ${key} » attend [singulier, pluriel]`);
      continue;
    }
    for (const [fr, en] of [[key, e[0]], [u.many, e[1]]]) {
      if (vars(fr) !== vars(en)) problems.push(`${u.where} : variables différentes entre « ${fr} » et « ${en} »`);
    }
  } else {
    if (typeof e !== "string" || !e) {
      problems.push(`${u.where} : « ${key} » attend un texte non vide`);
      continue;
    }
    if (vars(key) !== vars(e)) problems.push(`${u.where} : variables différentes entre « ${key} » et « ${e} »`);
  }
}
const unused = Object.keys(dict).filter((k) => !used.has(k));
for (const k of unused) problems.push(`i18n.js : entrée inutilisée « ${k} »`);

// Le repli : une clé inconnue revient en français, jamais vide.
if (I.t("clé absente du dictionnaire") !== "clé absente du dictionnaire") problems.push("i18n.js : t() ne replie pas sur le français");

console.log(`${used.size} textes utilisés, ${Object.keys(dict).length} entrées anglaises, ${missing} manques, ${unused.length} inutilisées`);
for (const p of problems) console.log(`  ${p}`);
console.log(problems.length ? `ÉCHEC : ${problems.length} écart(s)` : "OK");
process.exit(problems.length ? 1 : 0);
