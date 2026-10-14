// Remplit un carnet de test via l'API, puis parcourt l'interface dans Chrome
// headless : captures d'écran, erreurs console, quelques interactions.
const puppeteer = require(process.env.PUPPETEER || "puppeteer");
const path = require("path");
const fs = require("fs");

const BASE = process.env.BASE || "http://127.0.0.1:7399";
const OUT = process.env.OUT || path.join(__dirname, "shots");
fs.mkdirSync(OUT, { recursive: true });

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

const NOTES = [
  ["ascent", { title: "Retake B à trois", kind: "retake", side: "def", tags: ["b", "retake"], pin: true, body:
`## Conditions
- 3 joueurs en vie minimum, au moins une fumée
- spike posé côté **marché** ou **arrière-site**

## Regroupement
- point de regroupement : sous *heaven*, côté marché

## Ordre d'entrée
1. Omen fume l'arrière-site et le main
2. Sova drone + flèche de reconnaissance
3. entrée à deux par le marché, le troisième couvre heaven

## Calls
- « marché clean » → on pousse
- « il défuse » → tout le monde sort, pas de trade gratuit

> [!WARNING]
> Pas de retake sans fumée : on joue la sauvegarde.
` }],
  ["ascent", { title: "Exécution A rapide", kind: "execute", side: "atk", tags: ["a", "rush"], pin: true, body:
`## Timing
- départ à 1:30, exécution à 1:15

## Ordre des utilitaires
1. fumée CT + fumée heaven
2. flash au-dessus du mur générique
3. molly default

## Entrée
| Ordre | Joueur | Agent | Zone à nettoyer |
|---|---|---|---|
| 1 | Kai | Jett | site, puis tree |
| 2 | Lyn | Breach | wine |
| 3 | Moe | Sova | rafters |

## Post-plant
- spike : default
- positions : tree, generator, main
` }],
  ["ascent", { title: "Setup 2-1-2", kind: "setup", side: "def", tags: ["défaut"], body:
`## Positions
| Joueur | Agent | Position | Utilitaire | Rotation |
|---|---|---|---|---|
| Kai | Killjoy | A site | tourelle + alarmbot | reste |
| Lyn | Sova | mid | drone | vers B |
| Moe | Omen | B main | fumée | vers mid |

## Rotations
- contact A : mid tourne par market
- contact B : Omen tient, Sova rejoint
` }],
  ["ascent", { title: "Lineup Sova B main", kind: "lineup", side: "atk", tags: ["sova", "b"], body:
`## Agent
- Sova, flèche de reconnaissance

## Placement
- où se placer : coin gauche de B lobby
- où viser : haut de l'antenne, deux rebonds
` }],
  ["ascent", { title: "Anti-strat : équipe Nova", kind: "antistrat", tags: ["nova"], body:
`## Tendances
- en défense : stack B dès le 3e round
- en attaque : rush A sur les rounds bonus

## Notre réponse
- garder un joueur info mid
` }],
  ["bind", { title: "Défaut attaque", kind: "protocole", side: "atk", pin: true, body:
`## Objectif
Prendre les infos sans perdre de joueur avant 1:00.

## Rôles
| Joueur | Agent | Rôle | Utilitaire |
|---|---|---|---|
| Kai | Raze | entry | boom bot hookah |
| Lyn | Skye | info | chien A short |
` }],
  ["bind", { title: "Retake A short", kind: "retake", side: "def", body: "## Ordre d'entrée\n1. flash showers\n2. fumée tp\n" }],
  ["general", { title: "Communication", kind: "protocole", pin: true, tags: ["comms"], body:
`## Règles
- un seul call à la fois, le plus court possible
- infos : **nombre + lieu + utilitaire** (« deux B main, Sova »)
- après une mort : une phrase, puis silence

## Temps morts
- [x] situation économique
- [ ] adaptation adverse
- [ ] plan des 3 prochains rounds

> [!TIP]
> Un call clair vaut mieux qu'un call complet.

> [!IMPORTANT]
> Le coach ne parle que pendant les temps morts.

> [!CAUTION]
> Aucun reproche en plein round.

> [!NOTE]
> Revoir les comms à chaque VOD.
` }],
  ["general", { title: "Économie", kind: "protocole", tags: ["éco"], body: "## Règles d'achat\n- moins de 3900 après un round perdu : éco complète\n- bonus : on garde l'arme si on gagne le pistol\n" }],
];

(async () => {
  const st = await api("GET", "/api/state");
  if (st.total < 3) {
    for (const [map, n] of NOTES) await api("POST", `/api/notes/${map}`, n);
    await api("PUT", "/api/maps/ascent", { pool: true, overview: "## Défauts\n- attaque : 2 A, 1 mid, 2 B\n- défense : 2-1-2, Killjoy A\n\n## Callouts\n`heaven` `market` `tree` `wine` `generator`\n" });
    for (const m of ["bind", "haven", "lotus", "sunset", "split", "corrode"]) await api("PUT", `/api/maps/${m}`, { pool: true });
  }
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox", "--hide-scrollbars"] });
  const page = await browser.newPage();
  const problems = [];
  page.on("console", (m) => { if (["error", "warning"].includes(m.type())) problems.push(`console.${m.type()}: ${m.text()}`); });
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("requestfailed", (r) => problems.push(`requestfailed: ${r.url()} ${r.failure()?.errorText}`));
  page.on("response", (r) => { if (r.status() >= 400) problems.push(`HTTP ${r.status()} ${r.url()}`); });

  const shot = async (name, opts = {}) => {
    await new Promise((r) => setTimeout(r, opts.wait ?? 700));
    await page.screenshot({ path: path.join(OUT, `${name}.png`), fullPage: !!opts.full });
    console.log("capture", name);
  };
  // Changement d'ancre : pas de vraie navigation, on attend le rendu de la vue.
  const go = async (hash, selector) => {
    await page.evaluate((h) => { location.hash = h; }, hash);
    if (selector) await page.waitForSelector(selector, { timeout: 10000 });
    await new Promise((r) => setTimeout(r, 400));
  };
  const theme = async (t) => fetch(`${BASE}/api/settings`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ theme: t }) });

  await page.setViewport({ width: 1440, height: 1000 });
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });
  await theme("dark");
  await page.reload({ waitUntil: "networkidle0" });
  await shot("01-accueil-sombre", { wait: 2600 });
  await shot("01b-accueil-sombre-page", { full: true });
  await page.goto(BASE + "/#/map/ascent", { waitUntil: "networkidle0" });
  await shot("02-map-ascent", { wait: 1500 });
  await shot("02b-map-ascent-page", { full: true });
  await page.click('label:has(input[name="f-kind"][value="retake"])');
  await shot("03-map-filtre-retake", { wait: 300 });
  await page.goto(BASE + "/#/note/ascent/retake-b-a-trois", { waitUntil: "networkidle0" });
  await shot("04-note", { full: true });
  // Recherche au clavier
  await page.keyboard.press("/");
  await page.keyboard.type("fumee heaven");
  await shot("05-recherche", { wait: 600 });
  await page.keyboard.press("Enter");
  await new Promise((r) => setTimeout(r, 500));
  console.log("après entrée :", await page.evaluate(() => location.hash));
  // Édition : on tape, l'enregistrement automatique part tout seul.
  await go("#/edit/ascent/setup-2-1-2", "#f-body");
  await page.focus("#f-body");
  await page.keyboard.down("Control");
  await page.keyboard.press("End");
  await page.keyboard.up("Control");
  await page.keyboard.type("\n## Faiblesses connues\n- le mid tombe si Sova tourne trop tôt");
  await page.keyboard.press("Enter");
  await page.keyboard.type("deuxième point");
  await shot("06-edition", { wait: 2200 });
  const status = await page.$eval("#status", (e) => e.textContent);
  console.log("statut éditeur :", status);
  await go("#/sheet/ascent", ".sheet");
  await shot("07-fiche-match", { full: true });
  await go("#/new/ascent", "#newform");
  await shot("08-nouvelle-note");
  await go("#/help", ".keys");
  await shot("09-aide", { full: true });
  // Thème clair
  await theme("light");
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });
  await page.reload({ waitUntil: "networkidle0" });
  await shot("10-accueil-clair", { wait: 2600 });
  await go("#/note/ascent/execution-a-rapide", "#body");
  await shot("11-note-claire", { full: true });
  // Mobile
  await page.setViewport({ width: 390, height: 844, isMobile: true, deviceScaleFactor: 2 });
  await theme("dark");
  await page.goto(BASE + "/#/map/ascent", { waitUntil: "networkidle0" });
  await page.reload({ waitUntil: "networkidle0" });
  await shot("12-mobile-map", { wait: 1200 });
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });
  await shot("13-mobile-accueil", { wait: 1500 });

  await browser.close();
  console.log(problems.length ? "PROBLÈMES :\n" + [...new Set(problems)].join("\n") : "aucune erreur console / réseau");
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
