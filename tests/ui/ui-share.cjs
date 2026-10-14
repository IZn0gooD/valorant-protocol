// Copier, coller, importer : le presse-papiers est relu après chaque copie, des
// collages Word et Google Docs sont simulés, des fichiers .md importés et déposés.
// Aussi : téléchargement d'une note, menus au clavier, tiroir de navigation mobile.
// À lancer après ui-check.cjs (qui remplit le carnet).
const puppeteer = require(process.env.PUPPETEER || "puppeteer");
const fs = require("fs");
const os = require("os");
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
  await browser.defaultBrowserContext().overridePermissions(BASE, ["clipboard-read", "clipboard-write", "clipboard-sanitized-write"]);
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
    await sleep(350);
  };
  const clip = () =>
    page.evaluate(async () => {
      const out = {};
      // Le presse-papiers de Windows rend le texte avec des fins de ligne CRLF.
      for (const item of await navigator.clipboard.read()) for (const t of item.types) out[t] = (await (await item.getType(t)).text()).replace(/\r\n/g, "\n");
      return out;
    });
  const toastText = () => page.$eval("#toast", (t) => t.textContent.trim());
  const pasteInto = (selector, html, text) =>
    page.evaluate(
      (sel, h, t) => {
        const el = sel === "body" ? document.body : document.querySelector(sel);
        el.focus?.();
        const dt = new DataTransfer();
        if (h) dt.setData("text/html", h);
        if (t) dt.setData("text/plain", t);
        el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      },
      selector,
      html,
      text,
    );

  await page.goto(BASE + "/", { waitUntil: "networkidle0" });

  // 1. Copier une note : Markdown pour Discord et HTML pour Word, d'un seul clic.
  await go("#/note/ascent/execution-a-rapide", "#body");
  await sleep(400);
  await page.click('[data-copy="auto"]');
  await sleep(300);
  let c = await clip();
  const plain = c["text/plain"] || "";
  check(plain.startsWith("# Exécution A rapide\n*Exécution · Attaque · Ascent · #a #rush*"), "copie : titre et repères en tête");
  check(plain.includes("```\nOrdre  Joueur  Agent   Zone à nettoyer\n"), "copie : tableau aligné pour Discord");
  check((c["text/html"] || "").includes("<h1>Exécution A rapide</h1>") && c["text/html"].includes('border="1"'), "copie : HTML mis en forme pour Word");
  check((await toastText()).includes("Copié"), "copie : message de confirmation");

  // 1 bis. Une capture du carnet part embarquée dans le HTML copié (téléchargée au moment de copier).
  const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");
  const asset = await (await fetch(`${BASE}/api/assets`, { method: "POST", headers: { "Content-Type": "image/png" }, body: png })).json();
  const created = await (
    await fetch(`${BASE}/api/notes/ascent`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Lineup avec image", kind: "lineup", body: `## Placement\n![capture](${asset.markdown})\n` }),
    })
  ).json();
  await go(`#/note/ascent/${created.id}`, "#body img");
  let fetchedImages = 0;
  const onRequest = (r) => r.url().includes("/files/") && fetchedImages++;
  page.on("request", onRequest);
  await sleep(400);
  const beforeCopy = fetchedImages;
  await page.click('[data-copy="auto"]');
  await sleep(600);
  page.off("request", onRequest);
  c = await clip();
  check((c["text/html"] || "").includes('src="data:image/png;base64,'), "copie : la capture est embarquée dans le HTML");
  check(beforeCopy === 0 && fetchedImages >= 1, `l'image n'est téléchargée qu'au moment de copier (${beforeCopy} avant, ${fetchedImages} au total)`);
  check((c["text/plain"] || "").includes("[image : capture]"), "copie : la capture devient sa légende pour Discord");
  await go("#/note/ascent/execution-a-rapide", "#body");
  await sleep(300);

  // 2. Le menu « Copier pour… » au clavier : le texte simple, sans marques ni HTML.
  await page.click('[popovertarget="copy-menu"]');
  await page.waitForSelector("#copy-menu:popover-open", { timeout: 3000 });
  await sleep(150); // l'évènement « toggle » qui place le focus arrive au tour suivant
  check((await page.evaluate(() => document.activeElement?.dataset.copy)) === "discord", "menu : le premier format a le focus");
  await page.screenshot({ path: path.join(OUT, "34-menu-copier.png") });
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await sleep(300);
  c = await clip();
  check((c["text/plain"] || "").includes("1. fumée CT + fumée heaven") && !c["text/plain"].includes("**") && !c["text/html"], "texte simple : sans marques ni HTML");
  check(!(await page.$("#copy-menu:popover-open")), "le menu se referme après le choix");

  // 3. La touche C copie la note affichée.
  await page.evaluate(() => navigator.clipboard.writeText("vide"));
  await page.evaluate(() => document.querySelector("#page").focus());
  await page.keyboard.press("c");
  await sleep(300);
  c = await clip();
  check((c["text/plain"] || "").startsWith("# Exécution A rapide"), "touche C : la note est copiée");

  // 4. La fiche match ne copie que les notes épinglées.
  await go("#/sheet/ascent", ".sheet");
  await sleep(400);
  await page.click('[data-copy="auto"]');
  await sleep(300);
  c = await clip();
  const sheet = c["text/plain"] || "";
  check(sheet.startsWith("# Ascent — fiche match") && sheet.includes("## Retake B à trois") && !sheet.includes("## Setup 2-1-2"), "fiche match copiée, épinglées seulement");

  // 5. Coller un texte de Google Docs dans « Nouvelle note » : Markdown, titre repris.
  await go("#/new/bind", "#newform");
  const gdocs =
    '<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1"><h1 dir="ltr"><span style="font-size:20pt;font-weight:400">Retake B long</span></h1>' +
    '<p dir="ltr"><span style="font-weight:700">Quand :</span><span style="font-weight:400"> spike posé </span><span style="font-weight:700">default</span></p>' +
    '<ul><li dir="ltr"><p dir="ltr"><span>fumée tp</span></p></li><li dir="ltr"><p dir="ltr"><span>flash showers</span></p></li></ul>' +
    "<table><tbody><tr><td><p><span>Joueur</span></p></td><td><p><span>Agent</span></p></td></tr><tr><td><p><span>Kai</span></p></td><td><p><span>Omen</span></p></td></tr></tbody></table></b>";
  await pasteInto("#n-body", gdocs, "Retake B long\nQuand : spike posé default\nfumée tp\nflash showers\nJoueur\tAgent\nKai\tOmen");
  await sleep(200);
  const md = await page.$eval("#n-body", (t) => t.value);
  const want = "# Retake B long\n\n**Quand :** spike posé **default**\n\n- fumée tp\n- flash showers\n\n| Joueur | Agent |\n| --- | --- |\n| Kai | Omen |";
  check(md === want, `collage Google Docs → Markdown${md === want ? "" : ` : ${JSON.stringify(md)}`}`);
  check((await page.$eval("#n-title", (t) => t.value)) === "Retake B long", "titre repris du texte collé");
  await page.click('label:has(input[name="kind"][value="retake"])');
  check((await page.$eval("#kind-help", (p) => p.textContent)).includes("remplace le modèle"), "l'aide signale que le texte remplace le modèle");
  await page.screenshot({ path: path.join(OUT, "30-coller-google-docs.png") });
  await page.click('#newform button[type="submit"]');
  await page.waitForFunction(() => location.hash === "#/note/bind/retake-b-long", { timeout: 6000 }).catch(() => {});
  await page.waitForSelector("#body table", { timeout: 6000 }).catch(() => {});
  const saved = (await raw("bind", "retake-b-long")) || "";
  check(saved.includes("title: Retake B long") && !saved.includes("# Retake B long") && saved.includes("**Quand :**"), "note créée : le titre a quitté le corps");
  check((await toastText()).includes("Note créée"), "message après création, avec « Modifier »");

  // 6. Ctrl+V sur une page (hors champ) : une liste Word ouvre une nouvelle note pré-remplie.
  await go("#/map/ascent", "#cards");
  const word =
    '<p class=MsoNormal><b>Exécution B split</b></p>' +
    "<p class=MsoListParagraphCxSpFirst style='mso-list:l0 level1 lfo1'><span style='mso-list:Ignore'>1.<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp; </span></span>smoke CT</p>" +
    "<p class=MsoListParagraphCxSpLast style='mso-list:l0 level1 lfo1'><span style='mso-list:Ignore'>2.<span>&nbsp;</span></span>flash B main</p>";
  await pasteInto("body", word, "Exécution B split\n1.\tsmoke CT\n2.\tflash B main");
  await page.waitForFunction(() => location.hash === "#/new/ascent?paste=1", { timeout: 5000 }).catch(() => {});
  await page.waitForSelector("#n-body", { timeout: 5000 });
  await sleep(200);
  const pre = await page.$eval("#n-body", (t) => t.value);
  check(pre === "**Exécution B split**\n\n1. smoke CT\n2. flash B main", `Ctrl+V sur une map → nouvelle note pré-remplie (${JSON.stringify(pre)})`);
  check((await page.$eval("#n-title", (t) => t.value)) === "Exécution B split", "titre proposé");
  check(await page.evaluate(() => document.activeElement?.id === "n-body"), "le focus est dans le contenu");

  // 7. Éditeur : un collage riche devient du Markdown ; Ctrl+Maj+V reste brut.
  await go("#/edit/ascent/setup-2-1-2", "#f-body");
  await page.evaluate(() => {
    const t = document.querySelector("#f-body");
    t.focus();
    t.setSelectionRange(t.value.length, t.value.length);
  });
  await pasteInto("#f-body", "<h2>Notes VOD</h2><ul><li><strong>round 4</strong> : rotation trop tôt</li><li>round 9 : <em>bon</em> retake</li></ul>", "Notes VOD\nround 4 : rotation trop tôt\nround 9 : bon retake");
  await sleep(200);
  const ed = await page.$eval("#f-body", (t) => t.value);
  check(ed.endsWith("## Notes VOD\n\n- **round 4** : rotation trop tôt\n- round 9 : *bon* retake"), `collage riche dans l'éditeur (${JSON.stringify(ed.slice(-80))})`);
  const before = ed;
  await page.evaluate(() => {
    document.querySelector("#f-body").dispatchEvent(new KeyboardEvent("keydown", { key: "V", ctrlKey: true, shiftKey: true, bubbles: true }));
  });
  await pasteInto("#f-body", "<h2>Brut</h2>", "Brut");
  await sleep(150);
  check((await page.$eval("#f-body", (t) => t.value)) === before, "Ctrl+Maj+V : le navigateur colle le texte brut, sans conversion");
  await sleep(1500);

  // 7 bis. Le convertisseur sur des cas variés (Notion, Excel, pages web).
  const cases = await page.evaluate(async () => {
    const { htmlToMarkdown, guessTitle, pastedMarkdown } = await import("/static/clip.js");
    const run = (h) => htmlToMarkdown(h);
    const paste = (html, text) => {
      const dt = new DataTransfer();
      if (html) dt.setData("text/html", html);
      if (text) dt.setData("text/plain", text);
      return pastedMarkdown(dt);
    };
    return {
      msoBidi: run("<p><b style='mso-bidi-font-weight:normal'>Exécution</b> rapide <span style='mso-bidi-font-weight:bold;mso-bidi-font-style:italic'>B</span></p>"),
      overlap: run("<p><b>a<i>b</i></b><i>c</i></p>"),
      punct: run("<p><b>Note:</b>suite et mot<b>(x)</b></p>"),
      rowspan: run("<table><tr><td rowspan=2>Site</td><td>A</td></tr><tr><td>B</td></tr><tr><td>Zed</td><td>C</td></tr></table>"),
      fence: run("<pre>a ``` b\n\n\nc</pre>"),
      wordStyle: run("<html><head><style>p.MsoNormal{margin:0}</style></head><body><p class=MsoNormal>texte</p></body></html>"),
      office: paste('<html xmlns:o="urn:schemas-microsoft-com:office:office"><body><h1>Plan</h1><p>1. Smoke CT</p></body></html>', "Plan\n1. Smoke CT"),
      vscode: paste('<div style="color: #d4d4d4;background-color: #1e1e1e;white-space: pre;"><div><span style="font-style: italic;">// commentaire</span></div><div>    code();</div></div>', "// commentaire\n    code();"),
      dialogue: paste("", "— Bonjour\n— Salut"),
      sharpTitle: guessTitle("# Plan C#\ntexte").title,
      strongBlocks: run("<strong><p>un</p><p>deux</p></strong>"),
      emoji: run('<p>gg <img class="emoji" alt="🔥" src="x.png"></p>'),
      longList: run(`<ol>${"<li>x</li>".repeat(9)}<li>dix<ul><li>sous</li></ul></li></ol>`).split("\n").slice(-2).join("\n"),
      itemGuard: run("<ul><li>1. smoke</li><li># note</li></ul>"),
      nested: run("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>"),
      ordered: run("<ol><li>x<ol><li>y</li></ol></li><li>z</li></ol>"),
      excel: run("<table><tr><td>Joueur</td><td>Agent</td></tr><tr><td>Kai</td><td colspan=2>Sova</td></tr></table>"),
      guards: run("<p>- pas une liste</p><p>1. pas non plus</p><p>#1 priorité</p>"),
      link: run('<p>voir <a href="https://vlr.gg/x">VLR</a> et <a href="#/note/a/b">interne</a></p>'),
      spaces: run("<p><b>gras </b>suite <i> nuance</i>.</p>"),
      breaks: run("<p>ligne 1<br>ligne 2</p><p><br></p><p>après</p>"),
      strike: run('<p><span style="text-decoration:line-through">vieux</span> neuf</p>'),
      code: run("<p>tape <code>/ff</code></p><pre>ligne 1\n  ligne 2</pre>"),
      checks: run('<ul><li role="checkbox" aria-checked="true">veto</li><li><input type="checkbox"> pick</li></ul>'),
      title: guessTitle("**Retake A** rapide\nsuite").title,
    };
  });
  const expect = {
    msoBidi: "**Exécution** rapide B",
    overlap: "**a*b***_c_",
    punct: "**Note**:suite et mot(**x)**",
    rowspan: "| Site | A |\n| --- | --- |\n|  | B |\n| Zed | C |",
    fence: "````\na ``` b\n\n\nc\n````",
    wordStyle: "texte",
    office: "# Plan\n\n1\\. Smoke CT",
    vscode: null,
    dialogue: null,
    sharpTitle: "Plan C#",
    strongBlocks: "**un**\n\n**deux**",
    emoji: "gg 🔥",
    longList: "10. dix\n    - sous",
    itemGuard: "- 1\\. smoke\n- \\# note",
    nested: "- a\n  - b\n- c",
    ordered: "1. x\n   1. y\n2. z",
    excel: "| Joueur | Agent |  |\n| --- | --- | --- |\n| Kai | Sova |  |",
    guards: "\\- pas une liste\n\n1\\. pas non plus\n\n#1 priorité",
    link: "voir [VLR](https://vlr.gg/x) et interne",
    spaces: "**gras** suite *nuance*.",
    breaks: "ligne 1\nligne 2\n\naprès",
    strike: "~~vieux~~ neuf",
    code: "tape `/ff`\n\n```\nligne 1\n  ligne 2\n```",
    checks: "- [x] veto\n- [ ] pick",
    title: "Retake A rapide",
  };
  for (const [k, v] of Object.entries(expect)) check(cases[k] === v, `conversion « ${k} »${cases[k] === v ? "" : ` : ${JSON.stringify(cases[k])}`}`);

  // 8. Importer des fichiers .md : en-tête conservé, jamais d'écrasement.
  await go("#/map/haven", "#cards");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "protocol-import-"));
  const f1 = path.join(dir, "Retake B Haven.md");
  fs.writeFileSync(f1, "---\ntitle: Retake B (Haven)\ntype: retake\nside: def\naliases: [rb]\n---\n## Calls\n- stack B\n");
  const f2 = path.join(dir, "garage.md");
  fs.writeFileSync(f2, "# Défaut garage\n\n- 1 joueur garage\n");
  await (await page.$("#importFile")).uploadFile(f1, f2);
  // Le message d'import marque la fin : le carnet a pu garder des notes d'un passage précédent.
  await page.waitForFunction(() => document.querySelector("#toast")?.textContent.includes("importée"), { timeout: 6000 }).catch(() => {});
  await page.waitForFunction(() => document.querySelectorAll("#cards .card").length >= 2, { timeout: 6000 }).catch(() => {});
  const titles = await page.$$eval("#cards .card-title", (els) => els.map((e) => e.textContent));
  check(titles.includes("Retake B (Haven)") && titles.includes("Défaut garage"), `import de 2 fichiers .md (${titles.join(", ")})`);
  check(((await raw("haven", "retake-b-haven")) || "").includes("aliases: [rb]"), "en-tête inconnu conservé");
  check(!((await raw("haven", "defaut-garage")) || "").includes("# Défaut garage"), "le # Titre du fichier devient le titre de la note");
  check((await toastText()).includes("2 notes importées"), "message d'import");

  // 9. Déposer un fichier .md sur la map.
  const drag = (type) =>
    page.evaluate((t) => {
      const dt = new DataTransfer();
      dt.items.add(new File(["# Lineup Viper\n\n- mur A\n"], "viper.md", { type: "text/markdown" }));
      document.querySelector("#page").dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true }));
    }, type);
  await drag("dragenter");
  await drag("dragover");
  check(!!(await page.$("#dropHint")), "zone de dépôt affichée");
  await page.screenshot({ path: path.join(OUT, "31-depot-md.png") });
  await drag("drop");
  await page.waitForFunction(() => location.hash === "#/note/haven/lineup-viper", { timeout: 6000 }).catch(() => {});
  check(!!(await raw("haven", "lineup-viper")), "fichier déposé → note importée et ouverte");
  check(!(await page.$("#dropHint")), "zone de dépôt retirée");

  // 10. Télécharger une note (.md), depuis le menu « Plus d'actions ».
  const dl = fs.mkdtempSync(path.join(os.tmpdir(), "protocol-dl-"));
  const cdp = await browser.target().createCDPSession();
  await cdp.send("Browser.setDownloadBehavior", { behavior: "allow", downloadPath: dl });
  await go("#/note/ascent/retake-b-a-trois", "#body");
  await sleep(400);
  await page.click('[popovertarget="note-more"]');
  await page.waitForSelector("#note-more:popover-open", { timeout: 3000 });
  await page.screenshot({ path: path.join(OUT, "32-menu-plus.png") });
  await page.click('#note-more [data-act="download-note"]');
  const saved2 = path.join(dl, "retake-b-a-trois.md");
  for (let i = 0; i < 40 && !fs.existsSync(saved2); i++) await sleep(100);
  check(fs.existsSync(saved2) && fs.readFileSync(saved2, "utf8").startsWith("---\ntitle: Retake B à trois"), "note téléchargée en .md, en-tête compris");

  // 11. Menus au clavier : flèches, puis Échap rend le focus au bouton.
  await page.focus('[popovertarget="note-more"]');
  await page.keyboard.press("Enter");
  await page.waitForSelector("#note-more:popover-open", { timeout: 3000 });
  await sleep(150);
  await page.keyboard.press("ArrowDown");
  const second = await page.evaluate(() => document.activeElement?.dataset.act);
  await page.keyboard.press("Escape");
  await sleep(250);
  const back = await page.evaluate(() => document.activeElement?.getAttribute("popovertarget"));
  check(second === "delete" && back === "note-more" && !(await page.$("#note-more:popover-open")), `menu au clavier (${second}, focus rendu à ${back})`);

  // 12. Tiroir de navigation en petit écran.
  await page.setViewport({ width: 390, height: 844 });
  await go("#/map/ascent", "#cards");
  await page.click("#menu");
  await sleep(350);
  const opened = await page.evaluate(() => document.querySelector("#side").classList.contains("open") && document.querySelector("#side").contains(document.activeElement));
  await page.screenshot({ path: path.join(OUT, "33-tiroir-mobile.png") });
  await page.keyboard.press("Escape");
  await sleep(300);
  const closed = await page.evaluate(() => !document.querySelector("#side").classList.contains("open") && document.activeElement?.id === "menu");
  check(opened && closed, "tiroir : s'ouvre, focus dedans, Échap le ferme et rend le focus");
  await page.click("#menu");
  await sleep(350);
  await page.click('#side a[href="#/map/bind"]');
  await page.waitForFunction(() => location.hash === "#/map/bind", { timeout: 5000 }).catch(() => {});
  await sleep(300);
  check(await page.evaluate(() => !document.querySelector("#side").classList.contains("open")), "tiroir refermé après navigation");

  console.log(problems.length ? "PROBLÈMES :\n" + [...new Set(problems)].join("\n") : "aucune erreur console");
  console.log(failures ? `${failures} échec(s)` : "copier, coller, importer : tout passe");
  await browser.close();
  process.exit(failures ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
