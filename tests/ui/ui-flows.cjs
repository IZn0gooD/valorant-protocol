// Parcours interactifs : création, épinglage, suppression + annulation,
// conflit d'édition, thème. Chaque étape vérifie son résultat.
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
// Le texte complet d'une note (en-tête compris), tel que le carnet le garde ; null si elle n'existe pas.
const raw = async (map, id) => {
  const res = await fetch(`${BASE}/api/share/${map}/${id}`);
  return res.ok ? (await res.json()).markdown : null;
};
// Une autre fenêtre (ou un script) réécrit la note entière.
const elsewhere = async (map, id, change) => {
  const res = await fetch(`${BASE}/api/notes/${map}/${id}`, { method: "PUT", headers: { "Content-Type": "text/markdown" }, body: change(await raw(map, id)) });
  if (!res.ok) throw new Error(`PUT ${map}/${id} → ${res.status}`);
};

(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox", "--hide-scrollbars"] });
  const page = await browser.newPage();
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => { if (m.type() === "error" && !m.text().includes("409")) problems.push(`console.error: ${m.text()}`); });
  await page.setViewport({ width: 1400, height: 950 });
  const go = async (hash, selector) => {
    await page.evaluate((h) => { location.hash = h; }, hash);
    if (selector) await page.waitForSelector(selector, { timeout: 10000 });
    await sleep(300);
  };
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });

  // 1. Création par le formulaire
  await go("#/new/bind", "#newform");
  await page.type("#n-title", "Post-plant B hookah");
  await page.click('label:has(input[name="kind"][value="execute"])');
  await page.click('label:has(input[name="side"][value="atk"])');
  // Entrée depuis un bouton radio : la note est créée quand même.
  await page.keyboard.press("Enter");
  await page.waitForSelector("#f-body", { timeout: 10000 });
  const hash = await page.evaluate(() => location.hash);
  check(hash === "#/edit/bind/post-plant-b-hookah", `création → éditeur (${hash})`);
  const body = await page.$eval("#f-body", (t) => t.value);
  check(body.includes("## Timing"), "modèle « exécution » pré-rempli");
  check((await raw("bind", "post-plant-b-hookah")) !== null, "note enregistrée dans le carnet");

  // 2. Titre modifié + épinglé depuis l'éditeur, puis terminer
  await page.focus("#f-title");
  await page.keyboard.down("Control");
  await page.keyboard.press("a");
  await page.keyboard.up("Control");
  await page.keyboard.type("Post-plant B (hookah)");
  await page.click("#f-pin");
  await sleep(1600);
  check((await page.$eval("#status", (e) => e.textContent)) === "Enregistré", "enregistrement automatique");
  await page.focus(process.env.FOCUS || "#f-body");
  await page.keyboard.down("Control");
  await page.keyboard.press("Enter");
  await page.keyboard.up("Control");
  await page.waitForSelector(".note-title", { timeout: 10000 });
  check((await page.$eval(".note-title", (e) => e.textContent)) === "Post-plant B (hookah)", "Ctrl+Entrée → lecture, titre à jour");
  const text = await raw("bind", "post-plant-b-hookah");
  check(text.includes("title: Post-plant B (hookah)") && text.includes("pin: true") && text.includes("side: atk"), "en-tête enregistré dans le carnet");

  // 3. Désépingler depuis la page de lecture
  await page.click('[data-act="pin"]');
  await page.waitForFunction(() => document.querySelector('[data-act="pin"]')?.getAttribute("aria-pressed") === "false", { timeout: 5000 });
  check(!(await raw("bind", "post-plant-b-hookah")).includes("pin: true"), "désépinglée");

  // 4. Supprimer (menu « Plus d'actions », sans confirmation), puis annuler depuis le message
  await page.click('[popovertarget="note-more"]');
  await page.waitForSelector("#note-more:popover-open", { timeout: 3000 });
  await page.click('#note-more [data-act="delete"]');
  await page.waitForSelector('#toast:not([hidden]) [data-toast="undo"]', { timeout: 5000 });
  await sleep(300);
  check((await raw("bind", "post-plant-b-hookah")) === null, "supprimée → corbeille");
  await page.screenshot({ path: path.join(OUT, "20-suppression-annuler.png") });
  await page.click('[data-toast="undo"]');
  await page.waitForFunction(() => location.hash === "#/note/bind/post-plant-b-hookah", { timeout: 5000 });
  check((await raw("bind", "post-plant-b-hookah")) !== null, "annulation → restaurée");

  // 5. Conflit : la note change ailleurs (autre fenêtre) pendant qu'on l'édite
  await go("#/edit/bind/post-plant-b-hookah", "#f-body");
  await elsewhere("bind", "post-plant-b-hookah", (t) => t.replace("## Timing", "## Timing (modifié dans une autre fenêtre)"));
  await page.focus("#f-body");
  await page.keyboard.type("x");
  await page.waitForSelector('[data-act="keep-mine"]', { timeout: 6000 }).catch(() => {});
  check(!!(await page.$('[data-act="keep-mine"]')), "conflit détecté, choix proposé");
  await page.screenshot({ path: path.join(OUT, "21-conflit.png") });
  await page.click('[data-act="take-theirs"]');
  await sleep(300);
  check((await page.$eval("#f-body", (t) => t.value)).includes("modifié dans une autre fenêtre"), "version enregistrée reprise");

  // 6. Modification faite ailleurs sans conflit : rechargée toute seule
  await elsewhere("bind", "post-plant-b-hookah", (t) => t.replace("(modifié dans une autre fenêtre)", "(re-modifié)"));
  await sleep(6500);
  check((await page.$eval("#f-body", (t) => t.value)).includes("(re-modifié)"), "modification faite ailleurs reprise dans l'éditeur");

  // 7. Thème
  await go("#/", ".intro");
  const before = await page.evaluate(() => document.documentElement.dataset.theme);
  await page.click('[data-act="theme"]');
  await sleep(300);
  const after = await page.evaluate(() => document.documentElement.dataset.theme);
  check(before !== after, `thème ${before} → ${after}`);
  // Gardé dans le carnet, pas dans le navigateur : il survit au rechargement.
  await sleep(300);
  await page.reload({ waitUntil: "networkidle0" });
  check((await page.evaluate(() => document.documentElement.dataset.theme)) === after, "thème gardé dans le carnet");
  await page.click('[data-act="theme"]');
  await sleep(300);

  // 8. Raccourcis : / puis Échap, [ ] entre maps, ? aide
  await go("#/map/ascent", "#cards");
  await page.keyboard.press("]");
  await sleep(500);
  const next = await page.evaluate(() => location.hash);
  await page.keyboard.press("?");
  await sleep(500);
  check(next !== "#/map/ascent" && (await page.evaluate(() => location.hash)) === "#/help", `raccourcis ] (${next}) et ?`);

  // 9. Aide › Assistant IA (MCP) : la configuration exacte de ce poste, en lecture
  //    seule par défaut, puis avec écriture (Claude Desktop et Claude Code).
  await page.waitForSelector("#mcp-desktop", { timeout: 5000 });
  const readCfg = JSON.parse(await page.$eval("#mcp-desktop", (e) => e.textContent)).mcpServers.protocol;
  check(
    /protocol(\.exe)?$/i.test(readCfg.command) && readCfg.args[0] === "mcp" && readCfg.args.includes("--carnet") && !readCfg.args.includes("--write"),
    `MCP : configuration de ce poste (${readCfg.args.slice(0, 2).join(" ")} …)`,
  );
  await page.evaluate(() => document.querySelector('input[name="mcp-mode"][value="write"]').click());
  const writeCfg = JSON.parse(await page.$eval("#mcp-desktop", (e) => e.textContent)).mcpServers.protocol;
  const cli = await page.$eval("#mcp-code", (e) => e.textContent);
  check(writeCfg.args.includes("--write") && cli.startsWith("claude mcp add --scope user protocol -- ") && cli.endsWith("--write"), "MCP : avec écriture, pour Claude Desktop et Claude Code");

  console.log(problems.length ? "PROBLÈMES :\n" + problems.join("\n") : "aucune erreur console");
  console.log(failures ? `${failures} échec(s)` : "tous les parcours passent");
  await browser.close();
  process.exit(failures ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
