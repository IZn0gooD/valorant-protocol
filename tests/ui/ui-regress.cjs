// Non-régression des bugs reproduits par la revue JavaScript (réseau ralenti à la demande).
const puppeteer = require(process.env.PUPPETEER || "puppeteer");
const fs = require("fs");
const path = require("path");

const BASE = process.env.BASE || "http://127.0.0.1:7399";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (ok, label) => {
  console.log(`${ok ? "ok  " : "ÉCHEC"} ${label}`);
  if (!ok) failures++;
};
async function api(method, p, body) {
  const res = await fetch(BASE + p, { method, headers: body ? { "Content-Type": "application/json" } : {}, body: body ? JSON.stringify(body) : undefined });
  return res.json();
}
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
  // Rejouable : les notes d'un passage précédent (dont celle déplacée vers Bind) partent à la corbeille.
  for (const id of ["note-a", "note-b", "tags-virgule"]) await api("DELETE", `/api/notes/ascent/${id}`).catch(() => {});
  const leftovers = (await api("GET", "/api/notes?map=bind")).notes.filter((n) => /^note-a(-\d+)?$/.test(n.id));
  for (const n of leftovers) await api("DELETE", `/api/notes/bind/${n.id}`).catch(() => {});
  await api("POST", "/api/notes/ascent", { title: "Note A", body: "texte de A\n" });
  await api("POST", "/api/notes/ascent", { title: "Note B", body: "texte de B\n" });
  await api("POST", "/api/notes/ascent", { title: "Tags virgule", tags: ["A, B site", "retake"], body: "corps\n" });

  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.setRequestInterception(true);
  const slow = new Map(); // motif → délai, pour la prochaine requête qui correspond
  page.on("request", async (req) => {
    for (const [pattern, ms] of slow) {
      if (req.url().includes(pattern) && (req.method() !== "GET" || pattern.startsWith("GET "))) {
        slow.delete(pattern);
        await sleep(ms);
        break;
      }
      if (pattern.startsWith("GET ") && req.method() === "GET" && req.url().includes(pattern.slice(4))) {
        slow.delete(pattern);
        await sleep(ms);
        break;
      }
    }
    req.continue().catch(() => {});
  });
  await page.setViewport({ width: 1400, height: 950 });
  const go = async (hash, selector) => {
    await page.evaluate((h) => { location.hash = h; }, hash);
    if (selector) await page.waitForSelector(selector, { timeout: 10000 });
    await sleep(300);
  };
  await page.goto(BASE + "/", { waitUntil: "networkidle0" });

  // 1. Image collée dans A, envoi lent, on passe à B : rien n'arrive dans B.
  await go("#/edit/ascent/note-a", "#f-body");
  slow.set("/api/assets", 1500);
  await page.evaluate(async () => {
    const png = Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([png], "capture.png", { type: "image/png" }));
    document.querySelector("#f-body").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await sleep(100);
  await go("#/edit/ascent/note-b", "#f-body");
  await sleep(2500);
  check(!(await raw("ascent", "note-b")).includes("../assets/"), "image de A jamais insérée dans B");
  const toastText = await page.$eval("#toast", (t) => t.textContent);
  check(toastText.includes("après la fermeture"), "l'image envoyée trop tard est signalée");

  // 2. Vue lente (éditeur) puis navigation vers une map : la map reste affichée.
  await go("#/note/ascent/note-a", "#body");
  slow.set("GET /api/notes/ascent/note-a", 1200);
  await page.keyboard.press("e");
  await sleep(100);
  await go("#/map/ascent", "#cards");
  await sleep(1800);
  check((await page.$("#f-body")) === null && (await page.$("#cards")) !== null, "un éditeur lent n'écrase pas la map affichée après lui");

  // 3. Déplacement vers une autre map, puis [terminer] tout de suite.
  await go("#/edit/ascent/note-a", "#f-map");
  await page.select("#f-map", "bind");
  await page.click("#done");
  await page.waitForFunction(() => location.hash.startsWith("#/note/"), { timeout: 6000 }).catch(() => {});
  await sleep(500);
  const h3 = await page.evaluate(() => location.hash);
  check(h3 === "#/note/bind/note-a" && (await raw("bind", "note-a")) && !(await raw("ascent", "note-a")), `déplacement puis terminer → ${h3}`);

  // 4. Recherche au clavier : Échap ne téléporte pas, Entrée n'ouvre pas un vieux résultat.
  await go("#/map/ascent", "#cards");
  await page.keyboard.press("/");
  await page.keyboard.type("texte");
  await sleep(500);
  await page.keyboard.press("Escape");
  await sleep(400);
  const back = await page.evaluate(() => location.hash);
  await go("#/map/bind", "#cards");
  await page.keyboard.press("/");
  await page.keyboard.press("Escape");
  await sleep(300);
  const stay = await page.evaluate(() => location.hash);
  await page.keyboard.press("/");
  await page.keyboard.press("Enter");
  await sleep(400);
  const stay2 = await page.evaluate(() => location.hash);
  check(back === "#/map/ascent" && stay === "#/map/bind" && stay2 === "#/map/bind", `recherche : Échap → ${back}, puis ${stay}, Entrée à vide → ${stay2}`);

  // 5. Une étiquette contenant une virgule survit à une simple correction du texte.
  await go("#/edit/ascent/tags-virgule", "#f-body");
  await page.focus("#f-body");
  await page.keyboard.type(" corrigé");
  await sleep(1800);
  const t5 = (await raw("ascent", "tags-virgule"));
  check(t5.includes('tags: ["A, B site", retake]') && t5.includes("corrigé"), "étiquette « A, B site » intacte");

  // 6. Fermeture rapide après une frappe : pas de faux brouillon à la réouverture.
  await page.keyboard.type(" encore");
  await sleep(150);
  await page.reload({ waitUntil: "networkidle0" });
  await page.waitForSelector("#f-body", { timeout: 10000 });
  await sleep(600);
  const msgs = await page.$eval("#msgs", (m) => m.textContent.trim());
  check((await raw("ascent", "tags-virgule")).includes("encore") && msgs === "", `fermeture rapide : texte enregistré, aucun faux brouillon (${JSON.stringify(msgs)})`);

  // ── Revue de l'interface refaite (2026-10-13) ──

  // 7. Échap avec l'aide-mémoire ouvert : le menu se ferme, l'éditeur reste.
  await go("#/edit/ascent/note-b", "#f-body");
  await page.click('[popovertarget="cheat-menu"]');
  await page.waitForSelector("#cheat-menu:popover-open", { timeout: 3000 });
  await sleep(150);
  await page.keyboard.press("Escape");
  await sleep(300);
  check(
    (await page.evaluate(() => location.hash)) === "#/edit/ascent/note-b" && !(await page.$("#cheat-menu:popover-open")),
    "Échap ferme l'aide-mémoire sans quitter l'éditeur",
  );

  // 8. Échap sur un bouton de l'éditeur : on termine l'édition (vers la note), pas l'accueil.
  await page.focus('[data-md="bold"]');
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => location.hash === "#/note/ascent/note-b", { timeout: 5000 }).catch(() => {});
  check((await page.evaluate(() => location.hash)) === "#/note/ascent/note-b", "Échap sur un bouton de l'éditeur → retour à la note");

  // 9. Entrée sur « Importer des fichiers .md » ouvre le sélecteur, sans soumettre le formulaire.
  await go("#/new/ascent", "#newform");
  await page.focus('#newform [data-act="import"]');
  await page.evaluate(() => {
    window.__picker = 0;
    document.querySelector("#importFile").addEventListener("click", () => window.__picker++);
  });
  // L'interception du sélecteur s'active de façon asynchrone : on l'arme avant d'appuyer.
  const waiting = page.waitForFileChooser({ timeout: 3000 }).catch(() => null);
  await sleep(150);
  await page.keyboard.press("Enter");
  const chooser = await waiting;
  await chooser?.cancel();
  await sleep(200);
  const why = await page.evaluate(() => [
    location.hash,
    document.activeElement?.outerHTML.slice(0, 60),
    document.querySelector("#n-err")?.hidden,
    document.querySelector("#n-err")?.textContent,
    `clics sur le champ de fichier : ${window.__picker}`,
  ]);
  check(!!chooser && why[2], `Entrée sur « Importer » ouvre le sélecteur de fichiers${chooser && why[2] ? "" : ` (sélecteur : ${!!chooser}, état : ${JSON.stringify(why)})`}`);

  // 10. Le thème ne vide pas une nouvelle note ; quitter la page garde un brouillon.
  await page.type("#n-title", "Brouillon de test");
  await page.type("#n-body", "un protocole collé");
  await page.click('[data-act="theme"]');
  await sleep(400);
  const kept = await page.evaluate(() => [document.querySelector("#n-title")?.value, document.querySelector("#n-body")?.value]);
  check(kept[0] === "Brouillon de test" && kept[1] === "un protocole collé", "changer de thème garde la note en cours");
  await page.click('[data-act="theme"]');
  await go("#/map/ascent", "#cards");
  await go("#/new/ascent", "#newform");
  const back2 = await page.evaluate(() => [document.querySelector("#n-title").value, document.querySelector("#n-body").value, document.querySelector("#n-msgs").textContent]);
  check(back2[0] === "Brouillon de test" && back2[1] === "un protocole collé" && back2[2].includes("Brouillon repris"), "brouillon de nouvelle note repris au retour");
  await page.click('[data-act="new-draft-drop"]');
  await sleep(200);
  check(
    (await page.evaluate(() => document.querySelector("#n-body").value === "" && localStorage.getItem("protocol-draft:new") === null)),
    "« Effacer » vide le brouillon",
  );

  // 11. Un rafraîchissement ne déplace pas le focus (ni vers la barre latérale, ni hors de la page).
  await go("#/map/ascent", "#cards");
  await page.focus("#cards .card");
  const focused = await page.evaluate(() => document.activeElement.getAttribute("href"));
  // Une autre fenêtre modifie une autre note : la page se rafraîchit.
  await elsewhere("general", "communication", (t) => t + "\n- ajout externe\n");
  await sleep(6500);
  const after = await page.evaluate(() => [document.activeElement.getAttribute("href"), !!document.activeElement.closest("#page")]);
  check(after[0] === focused && after[1], `focus gardé sur la carte après un rafraîchissement (${after[0]})`);

  // 12. Menus : un lien choisi referme le menu ; Tab le referme et rend le focus au bouton.
  await page.click('[popovertarget="map-more"]');
  await page.waitForSelector("#map-more:popover-open", { timeout: 3000 });
  await page.click('#map-more a[download]');
  await sleep(250);
  check(!(await page.$("#map-more:popover-open")), "« Télécharger la map » referme le menu");
  await page.click('[popovertarget="map-more"]');
  await page.waitForSelector("#map-more:popover-open", { timeout: 3000 });
  await sleep(150);
  await page.keyboard.press("Tab");
  await sleep(200);
  check(
    !(await page.$("#map-more:popover-open")) && (await page.evaluate(() => document.activeElement?.getAttribute("popovertarget"))) === "map-more",
    "Tab referme le menu et rend le focus à son bouton",
  );

  // ── Interface en pièces « ui » d'ascii.rest (2026-10-13) ──

  // 13. Un titre de note très long ne fait pas déborder l'arborescence : il s'abrège, les comptes restent visibles.
  for (const n of (await api("GET", "/api/notes?map=lotus")).notes.filter((x) => x.id.startsWith("un-protocole-au-titre"))) {
    await api("DELETE", `/api/notes/lotus/${n.id}`).catch(() => {});
  }
  const long = await api("POST", "/api/notes/lotus", { title: "Un protocole au titre vraiment très long pour la barre latérale", body: "x\n" });
  await go(`#/note/lotus/${long.id}`, "#body");
  const tree = await page.evaluate(() => {
    const side = document.querySelector("#side");
    const edge = side.getBoundingClientRect().right;
    const counts = [...document.querySelectorAll("#nav .t .n")];
    const title = document.querySelector('#nav .t.d2 a[aria-current="page"]');
    return {
      over: side.scrollWidth - side.clientWidth,
      hidden: counts.filter((c) => c.getBoundingClientRect().right > edge).length,
      cut: title ? title.scrollWidth > title.clientWidth : false,
    };
  });
  check(tree.over <= 0 && tree.hidden === 0 && tree.cut, `arborescence : titre long abrégé, comptes visibles (${JSON.stringify(tree)})`);
  await api("DELETE", `/api/notes/lotus/${long.id}`).catch(() => {});

  console.log(errors.length ? "ERREURS JS :\n" + errors.join("\n") : "aucune erreur JavaScript");
  console.log(failures ? `${failures} échec(s)` : "toutes les non-régressions passent");
  await browser.close();
  process.exit(failures ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
