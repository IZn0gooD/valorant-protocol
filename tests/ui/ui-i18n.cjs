// L'interface en anglais : le carnet passe en anglais par l'API, puis chaque vue
// (accueil, map, note, éditeur, nouvelle note, recherche, fiche match, corbeille,
// aide) est parcourue. On y vérifie des textes clés anglais et on traque les restes
// de français — hors contenu des notes : titres de notes et de maps, corps, extraits,
// tags. À la fin, le sélecteur de langue de l'aide repasse le carnet en français.
//
// À lancer après ui-check.cjs (il s'appuie sur les notes de la map « ascent »).
const puppeteer = require(process.env.PUPPETEER || "puppeteer");
const fs = require("fs");
const path = require("path");

const BASE = process.env.BASE || "http://127.0.0.1:7399";
const OUT = process.env.OUT || path.join(__dirname, "shots");
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (ok, label) => {
  console.log(`${ok ? "ok  " : "ÉCHEC"} ${label}`);
  if (!ok) failures++;
};

async function api(method, p, body) {
  const res = await fetch(BASE + p, {
    method,
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`${method} ${p} → ${res.status} ${JSON.stringify(data)}`);
  return data;
}

// Des traces de français : lettres accentuées, ou mots qui n'existent pas en anglais.
const FRENCH = new RegExp(
  "[àâçéèêëîïôûùœÀÂÇÉÈÊÎÔÛŒ«»]|\\b(" +
    [
      "le", "la", "les", "des", "une", "du", "au", "aux", "et", "ou", "pour", "avec", "dans", "sur", "est", "pas", "ici", "puis",
      "carnet", "corbeille", "accueil", "aide", "modifier", "copier", "coller", "rechercher", "recherche", "fiche", "nouvelle",
      "supprimer", "enregistrer", "brouillon", "tout", "tous", "aucune", "retour", "titre", "contenu", "plus", "fermer", "quitter",
      "imprimer", "restaurer", "terminer", "aperçu", "propriétés", "côté", "attaque", "défense", "épinglée", "épinglées",
      "thème", "clair", "sombre", "touche", "entrée", "échap", "outils", "raccourcis", "lecture", "seule", "écriture",
    ].join("|") +
    ")\\b",
  "i",
);
// Ce qui reste volontairement bilingue ou en français : le sélecteur de langue.
const ALLOWED = ["Langue / Language", "Français"];

(async () => {
  const before = await api("GET", "/api/state");
  await api("POST", "/api/settings", { lang: "en" });
  const st = await api("GET", "/api/state");
  check(st.lang === "en" && st.langSetting === "en", `réglage lang=en par l'API (${st.lang}, « ${st.langSetting} »)`);

  // Le contenu du carnet (titres, tags) n'est pas l'interface : on l'écarte de la traque.
  const ascent = await api("GET", "/api/maps/ascent");
  const notes = ascent.notes;
  const note = notes.find((n) => n.pin) ?? notes[0];
  const content = new Set();
  for (const m of st.maps) content.add(m.title);
  for (const n of [...notes, ...(st.recent || []), ...(st.pinned || [])]) {
    content.add(n.title);
    for (const tag of n.tags || []) content.add(`#${tag}`);
  }
  for (const m of st.maps) {
    if (m.id === "ascent") continue;
    const d = await api("GET", `/api/maps/${encodeURIComponent(m.id)}`).catch(() => null);
    for (const n of d?.notes || []) content.add(n.title);
  }
  const trash = await api("GET", "/api/trash").catch(() => ({ items: [] }));
  for (const it of trash.items) content.add(it.title);
  // Les libellés venus du serveur (types, côtés) : traduits par lui ; signalés à part s'ils restent en français.
  const server = new Set([...st.kinds.flatMap((k) => [k.label, k.plural]), ...(st.sides || []).map((s) => s.label)]);

  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox", "--hide-scrollbars"] });
  const page = await browser.newPage();
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") problems.push(`console.error: ${m.text()}`);
  });
  await page.setViewport({ width: 1400, height: 950 });
  const go = async (hash, selector) => {
    await page.evaluate((h) => {
      location.hash = h;
    }, hash);
    if (selector) await page.waitForSelector(selector, { timeout: 10000 });
    await sleep(400);
  };
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });
  check((await page.evaluate(() => document.documentElement.lang)) === "en", "<html lang=\"en\">");
  check((await page.evaluate(() => globalThis.I18N?.lang)) === "en", "i18n.js en anglais");

  const serverFrench = new Set();
  /** L'interface visible, plus les textes cachés (menus, title, aria-label, placeholder), contenu des notes retiré. */
  async function uiText() {
    const parts = await page.evaluate(() => {
      const skip = ".docs, .note-body, .excerpt, .card-title, .t-title, .hit-title, .hit-snip, .tags, .page-title, .note-title, .art, .brand-banner, pre.code, code, kbd, textarea, #f-title, .crumbs span[aria-current], #nav a, .trash .title, .tile .name";
      const out = [];
      const clone = document.body.cloneNode(true);
      for (const el of clone.querySelectorAll(skip)) el.remove();
      // innerText ne voit que ce qui s'affiche : on le prend sur le vrai document, élément par élément.
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const el = n.parentElement;
        if (!el || el.closest(skip) || !n.data.trim()) continue;
        // Les menus fermés et le dialogue de copie font partie de l'interface, même cachés.
        if (!el.checkVisibility() && !el.closest(".menu, dialog, .sr")) continue;
        out.push(n.data.trim());
      }
      for (const el of document.querySelectorAll("[title], [aria-label], [placeholder]")) {
        if (el.closest(".docs, .art")) continue;
        for (const a of ["title", "aria-label", "placeholder"]) {
          const v = el.getAttribute(a);
          if (v) out.push(v);
        }
      }
      return out;
    });
    return parts;
  }
  async function frenchIn() {
    const found = [];
    for (let s of await uiText()) {
      for (const a of ALLOWED) s = s.split(a).join(" ");
      for (const c of [...content].sort((a, b) => b.length - a.length)) if (c && s.includes(c)) s = s.split(c).join(" ");
      let rest = s;
      for (const v of server) {
        if (v && rest.includes(v)) {
          if (FRENCH.test(v)) serverFrench.add(v);
          rest = rest.split(v).join(" ");
        }
      }
      if (FRENCH.test(rest)) found.push(rest.trim().slice(0, 90));
    }
    return [...new Set(found)];
  }
  async function noFrench(label) {
    const uniq = await frenchIn();
    check(uniq.length === 0, `${label} : aucun reste de français${uniq.length ? ` → ${uniq.map((x) => `« ${x} »`).join(", ")}` : ""}`);
  }
  const has = async (texts, label) => {
    const body = await page.evaluate(() => document.body.innerText + "\n" + [...document.querySelectorAll("[title],[aria-label],[placeholder]")].map((e) => [e.title, e.getAttribute("aria-label"), e.placeholder].join(" ")).join("\n"));
    const miss = texts.filter((t) => !body.includes(t));
    check(miss.length === 0, `${label} : textes anglais présents${miss.length ? ` (manque : ${miss.join(", ")})` : ""}`);
  };
  const shot = (name) => page.screenshot({ path: path.join(OUT, `en-${name}.png`) });

  // Accueil
  await go("#/", "#radar");
  await has(["Pinned", "Maps", "New note", "Paste a protocol", "Help", "Quit", "Switch to"], "accueil");
  const lede = await page.$eval(".lede", (p) => p.textContent);
  check(/\d+ notes?, \d+ maps?\.$/.test(lede.trim()), `accueil : pluriels anglais (${lede.trim().slice(-22)})`);
  // Témoin : un texte français glissé dans la page doit être repéré par la traque.
  await page.evaluate(() => {
    const p = document.createElement("p");
    p.id = "i18n-probe";
    p.textContent = "Modifier la présentation";
    document.querySelector("#page").append(p);
  });
  check((await frenchIn()).some((x) => x.includes("Modifier")), "témoin : un reste de français est bien repéré");
  await page.evaluate(() => document.querySelector("#i18n-probe")?.remove());
  await noFrench("accueil");
  await shot("accueil");

  // Map
  await go("#/map/ascent", "#cards");
  await has(["Match sheet", "Copy the map", "Overview", "More", "Type", "Side", "All", "pinned"], "map");
  await noFrench("map");
  await shot("map");

  // Note
  await go(`#/note/ascent/${encodeURIComponent(note.id)}`, "#body");
  await has(["Edit", "Copy", "Pinned", "edited"], "note");
  await noFrench("note");
  await shot("note");

  // Éditeur
  await go(`#/edit/ascent/${encodeURIComponent(note.id)}`, "#f-body");
  await has(["Saved", "Done", "Properties", "Preview", "Bold (Ctrl+B)", "Insert", "comma-separated tags", "Content (Markdown)"], "éditeur");
  check((await page.$eval('[data-md="bold"] b', (b) => b.textContent)) === "B", "éditeur : gras s'écrit B");
  await noFrench("éditeur");
  await shot("editeur");
  await page.click('[data-act="preview"]'); // rien à enregistrer : on sort proprement
  await go("#/", "#radar");

  // Nouvelle note
  await go("#/new/ascent", "#newform");
  await has(["New note", "Create note", "Title", "Content", "Import .md files", "E.g. 3-man B retake"], "nouvelle note");
  await noFrench("nouvelle note");
  await shot("nouvelle-note");

  // Recherche
  await go("#/search/retake", "#hits .hit");
  await has(["Search", "result", "for “retake”", "to open."], "recherche");
  await noFrench("recherche");
  await go("#/search/", "#sum");
  await has(["Type a word, or a filter", "exact phrase"], "recherche vide");

  // Fiche match
  await go("#/sheet/ascent", ".sheet, .empty-line");
  await has(["Match sheet: ", "Print", "Copy the sheet", "Back to the map"], "fiche match");
  await noFrench("fiche match");
  await shot("fiche-match");

  // Corbeille
  await go("#/trash", ".trash, .empty-line");
  const bin = await page.evaluate(() => document.body.innerText);
  check(/Restore|The bin is empty\./.test(bin) && bin.includes("Deleted notes wait here"), "corbeille : textes anglais présents");
  await noFrench("corbeille");

  // Introuvable
  await go("#/nulle-part", ".empty");
  await has(["404 · not found", "Back to home"], "page introuvable");

  // Aide, et son sélecteur de langue
  await go("#/help", "#h-keys");
  await has(["Help and shortcuts", "Keyboard shortcuts", "Copy, paste, import", "Your notebook", "With other tools", "AI assistant (MCP)", "Credits", "Esc"], "aide");
  const sizeLine = await page.evaluate(() => [...document.querySelectorAll("#h-files ~ p")].map((p) => p.textContent).find((t) => t.startsWith("Notebook:")) || "");
  check(/\(\d+(\.\d)? (KB|MB)\)$/.test(sizeLine), `aide : taille du carnet en anglais (${sizeLine.slice(-12)})`);
  const picked = await page.$eval('input[name="ui-lang"]:checked', (i) => i.value).catch(() => null);
  check(picked === "en", `aide : sélecteur de langue sur English (${picked})`);
  await noFrench("aide");
  await shot("aide");

  // Un menu ouvert : ses entrées sont traduites.
  await go("#/map/ascent", "#cards");
  await page.click('[popovertarget="map-more"]');
  await sleep(200);
  await has(["Paste a protocol", "Download the map (.md)"], "menu « More »");
  await page.keyboard.press("Escape");

  // Retour au français par le sélecteur : la page se recharge en français.
  await go("#/help", "#h-keys");
  await Promise.all([page.waitForNavigation({ waitUntil: "networkidle0" }), page.click('label:has(input[name="ui-lang"][value="fr"])')]);
  await page.waitForSelector("#h-keys", { timeout: 10000 });
  check((await page.evaluate(() => document.documentElement.lang)) === "fr", "sélecteur : Français → page rechargée en français");
  check((await page.$eval("#h-keys", (h) => h.textContent)) === "Raccourcis clavier", "aide de nouveau en français");
  check((await api("GET", "/api/state")).langSetting === "fr", "réglage du carnet : fr");

  // Le réglage d'avant le test revient s'il donnait le français (automatique compris).
  if (before.lang === "fr") await api("POST", "/api/settings", { lang: before.langSetting || "auto" });
  check((await api("GET", "/api/state")).lang === "fr", "carnet rendu en français");

  await browser.close();
  if (serverFrench.size) console.log(`note : libellés du serveur encore en français : ${[...serverFrench].map((x) => `« ${x} »`).join(", ")}`);
  check(problems.length === 0, problems.length ? `erreurs console :\n  ${problems.join("\n  ")}` : "aucune erreur console");
  console.log(failures ? `${failures} échec(s)` : "interface anglaise : tout passe");
  process.exit(failures ? 1 : 0);
})().catch(async (e) => {
  console.error(e);
  await fetch(BASE + "/api/settings", { method: "POST", headers: { "Content-Type": "application/json" }, body: '{"lang":"fr"}' }).catch(() => {});
  process.exit(1);
});
