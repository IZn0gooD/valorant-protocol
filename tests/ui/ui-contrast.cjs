// Contrastes WCAG AA, mesurés : pour chaque texte de l'interface, la couleur
// calculée par le navigateur contre le fond réellement visible (fonds
// semi-transparents composés jusqu'à la page), dans les deux thèmes.
// Seuils : 4,5:1, ou 3:1 pour un grand texte (24 px, ou 18,66 px en gras).
const puppeteer = require(process.env.PUPPETEER || "puppeteer");

const BASE = process.env.BASE || "http://127.0.0.1:7399";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// [vue, sélecteur, pseudo-élément facultatif, script préalable facultatif]
// Le script préalable ouvre un menu ou pose un message : on mesure les vraies classes.
const TOAST = `document.querySelector("#toast").innerHTML = '<div class="toast-box"><span class="mark">[ OK ]</span><span class="msg">Texte copié.</span><button class="btn small" type="button"><span>Annuler</span></button></div><div class="toast-box"><span class="mark alert">[ !! ]</span><span class="msg">Copie impossible.</span></div>'`;
const NOTICE = `document.querySelector("#msgs").innerHTML = '<div class="notice box warn" role="alert"><span class="box-title">Conflit</span><p class="q">Cette note a été modifiée ailleurs.</p></div>'`;
const CHECKS = [
  ["#/", ".lede"],
  ["#/", ".intro .box-hint"],
  ["#/", ".intro .btn.primary > span"],
  ["#/", ".intro .btn:not(.primary) > span"],
  ["#/", ".intro .btn:not(.primary)", "::before"],
  ["#/", "#barEnd .btn.primary > span"],
  ["#/", '#barEnd [data-act="theme"] > span'],
  ["#/", ".search input", "::placeholder"],
  ["#/", ".search .hint"],
  ["#/", ".sh h2"],
  ["#/", ".sh .n"],
  ["#/", ".card-title"],
  ["#/", ".card .excerpt"],
  ["#/", ".card .box-title"],
  ["#/", ".card .box-hint"],
  ["#/", ".card .meta > span:first-child"],
  ["#/", ".meta .atk"],
  ["#/", ".meta .def"],
  ["#/", ".tile .name"],
  ["#/", ".tile .info"],
  ["#/", ".tile .box-hint"],
  ["#/", ".leaders .t-title"],
  ["#/", ".leaders .m"],
  ["#/", '.root[aria-current="page"]'],
  ["#/", ".t a.dir"],
  ["#/", ".t a.dir", "::after"],
  ["#/", ".t .n"],
  ["#/", ".tree .cap > span"],
  ["#/", ".tree .foot-line"],
  ["#/", ".side-foot .btn > span"],
  ["#/map/ascent", ".crumbs a"],
  ["#/map/ascent", '.crumbs [aria-current="page"]'],
  ["#/map/ascent", ".page-title"],
  ["#/map/ascent", ".page-head .meta > span"],
  ["#/map/ascent", ".row > .lbl"],
  ["#/map/ascent", ".radio:not(:has(input:checked)) > span:first-of-type"],
  ["#/map/ascent", ".radio:not(:has(input:checked)) > span:first-of-type", "::before"],
  ["#/map/ascent", ".radio:has(input:checked) > span:first-of-type"],
  ["#/map/ascent", ".radio:has(input:checked) > span:first-of-type", "::before"],
  ["#/map/ascent", ".radio .n"],
  ["#/map/ascent", ".radio.atk > span:first-of-type"],
  ["#/map/ascent", ".radio.def > span:first-of-type"],
  ["#/map/ascent", ".panel .box-title"],
  ["#/map/ascent", ".panel .box-tag .btn > span"],
  ["#/map/ascent", ".panel .docs li"],
  ["#/map/ascent", ".docs :not(pre) > code"],
  ["#/map/ascent", ".card .box-tag"],
  ["#/map/ascent", '.t a.dir[aria-current="page"]'],
  ["#/map/ascent", ".t.d2 a"],
  ["#/map/ascent", ".t .pin"],
  ["#/map/ascent", ".menu-item b", "", 'document.querySelector("#map-more").showPopover()'],
  ["#/map/ascent", ".menu-item small"],
  ["#/note/ascent/retake-b-a-trois", ".note-title"],
  ["#/note/ascent/retake-b-a-trois", ".page-head .meta > span:last-child"],
  ["#/note/ascent/retake-b-a-trois", ".tags a"],
  ["#/note/ascent/retake-b-a-trois", "#tools .btn.primary > span"],
  ["#/note/ascent/retake-b-a-trois", ".split .btn > span"],
  ["#/note/ascent/retake-b-a-trois", '.toggle[aria-pressed="true"] > span'],
  ["#/note/ascent/retake-b-a-trois", '.toggle[aria-pressed="true"] > span', "::before"],
  ["#/note/ascent/retake-b-a-trois", ".docs li"],
  ["#/note/ascent/retake-b-a-trois", ".docs h2"],
  ["#/note/ascent/retake-b-a-trois", ".markdown-alert-warning", "::before"],
  ["#/note/ascent/retake-b-a-trois", ".markdown-alert-warning p"],
  ["#/note/ascent/retake-b-a-trois", ".pager small"],
  ["#/note/ascent/retake-b-a-trois", ".pager a > span"],
  ["#/note/ascent/retake-b-a-trois", ".menu-item.danger b", "", 'document.querySelector("#note-more").showPopover()'],
  ["#/note/ascent/retake-b-a-trois", ".toast-box .msg", "", TOAST],
  ["#/note/ascent/retake-b-a-trois", ".toast-box .mark"],
  ["#/note/ascent/retake-b-a-trois", ".toast-box .mark.alert"],
  ["#/note/ascent/retake-b-a-trois", ".toast-box .btn > span"],
  ["#/note/ascent/retake-b-a-trois", ".offline", "", 'document.querySelector("#offline").hidden = false'],
  ["#/note/ascent/execution-a-rapide", ".docs th"],
  ["#/note/ascent/execution-a-rapide", ".docs td"],
  ["#/note/general/communication", ".docs .check"],
  ["#/note/general/communication", ".docs .check.on"],
  ["#/note/general/communication", ".markdown-alert-tip", "::before"],
  ["#/note/general/communication", ".markdown-alert-important", "::before"],
  ["#/note/general/communication", ".markdown-alert-caution", "::before"],
  ["#/note/general/communication", ".markdown-alert-note", "::before"],
  ["#/edit/ascent/setup-2-1-2", ".status"],
  ["#/edit/ascent/setup-2-1-2", "#f-title"],
  ["#/edit/ascent/setup-2-1-2", ".meta-form .box-title"],
  ["#/edit/ascent/setup-2-1-2", ".meta-form .box-hint"],
  ["#/edit/ascent/setup-2-1-2", ".row > label"],
  ["#/edit/ascent/setup-2-1-2", "#f-kind"],
  ["#/edit/ascent/setup-2-1-2", "#f-tags"],
  ["#/edit/ascent/setup-2-1-2", ".toolbar .btn > span"],
  ["#/edit/ascent/setup-2-1-2", ".toolbar .box-hint"],
  ["#/edit/ascent/setup-2-1-2", ".pane .box-title"],
  ["#/edit/ascent/setup-2-1-2", "#f-body"],
  ["#/edit/ascent/setup-2-1-2", ".editor-hint"],
  ["#/edit/ascent/setup-2-1-2", ".notice .box-title", "", NOTICE],
  ["#/edit/ascent/setup-2-1-2", ".notice .q"],
  ["#/new/ascent", ".formbox .box-title"],
  ["#/new/ascent", ".formbox .box-hint"],
  ["#/new/ascent", ".row-help"],
  ["#/new/ascent", "#n-title", "::placeholder"],
  ["#/new/ascent", "#n-body", "::placeholder"],
  ["#/search/fumee", ".hit.sel .hit-title"],
  ["#/search/fumee", ".hit:not(.sel) .hit-title"],
  ["#/search/fumee", ".hit .meta > span"],
  ["#/search/fumee", ".hit-snip"],
  ["#/search/fumee", "mark"],
  ["#/search/fumee", ".search-sum"],
  ["#/sheet/ascent", ".sheet-card .box-title"],
  ["#/sheet/ascent", ".sheet-card h2 a"],
  ["#/trash", ".trash .title, .empty-line"],
  ["#/help", ".help-page .box-title"],
  ["#/help", ".keys dd"],
  ["#/help", "kbd"],
  ["#/help", ".help-page code"],
  ["#/help", ".help-page .radio:has(input:checked) > span:first-of-type"],
  ["#/help", ".help-page .radio:not(:has(input:checked)) > span:first-of-type"],
  ["#/help", ".help-page .muted"],
  ["#/help", "#mcp-desktop"],
];

(async () => {
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  await page.setViewport({ width: 1400, height: 950 });
  let failures = 0;
  let total = 0;
  for (const theme of ["dark", "light"]) {
    await page.goto(BASE + "/", { waitUntil: "networkidle0" });
    await ((t) => fetch(`${BASE}/api/settings`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ theme: t }) }))(theme);
    // Le thème est gardé dans le carnet et posé au chargement : on recharge, et on vérifie qu'il a bien pris.
    await page.reload({ waitUntil: "networkidle0" });
    const active = await page.evaluate(() => document.documentElement.dataset.theme);
    if (active !== theme) throw new Error(`thème ${theme} demandé, ${active} actif`);
    let view = null;
    for (const [hash, sel, pseudo, prep] of CHECKS) {
      if (hash !== view) {
        await page.evaluate((h) => {
          location.hash = h;
        }, hash);
        await sleep(700);
        view = hash;
      }
      if (prep) {
        await page.evaluate(prep);
        await sleep(150);
      }
      const res = await page.evaluate(
        (sel, pseudo) => {
          const parse = (c) => {
            const m = c.match(/rgba?\(([^)]+)\)/);
            if (!m) return null;
            const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
            return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
          };
          const over = (top, under) => ({
            r: top.r * top.a + under.r * (1 - top.a),
            g: top.g * top.a + under.g * (1 - top.a),
            b: top.b * top.a + under.b * (1 - top.a),
            a: 1,
          });
          const lum = ({ r, g, b }) => {
            const f = (v) => {
              v /= 255;
              return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
            };
            return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
          };
          const el = document.querySelector(sel);
          if (!el) return { missing: true };
          // Le fond visible : les fonds des ancêtres, composés depuis la page.
          const chain = [];
          for (let n = el; n && n.nodeType === 1; n = n.parentElement) chain.push(n);
          let bg = { r: 255, g: 255, b: 255, a: 1 };
          for (const n of chain.reverse()) {
            const c = parse(getComputedStyle(n).backgroundColor);
            if (c && c.a > 0) bg = over(c, bg);
          }
          const cs = getComputedStyle(el, pseudo || null);
          if (pseudo === "::before" || pseudo === "::after") {
            const b = parse(cs.backgroundColor);
            if (b && b.a > 0) bg = over(b, bg);
          }
          let fg = parse(cs.color);
          if (fg.a < 1) fg = over(fg, bg);
          const [hi, lo] = [lum(fg), lum(bg)].sort((x, y) => y - x);
          const ratio = (hi + 0.05) / (lo + 0.05);
          const size = parseFloat(cs.fontSize);
          const bold = Number(cs.fontWeight) >= 700;
          const large = size >= 24 || (bold && size >= 18.66);
          return { ratio, need: large ? 3 : 4.5, size, weight: cs.fontWeight };
        },
        sel,
        pseudo || "",
      );
      total++;
      const label = `${theme.padEnd(5)} ${hash.padEnd(34)} ${sel}${pseudo || ""}`;
      if (res.missing) {
        console.log(`ABSENT ${label}`);
        failures++;
      } else if (res.ratio < res.need) {
        console.log(`ÉCHEC  ${label} : ${res.ratio.toFixed(2)}:1 (il faut ${res.need}:1, ${res.size}px/${res.weight})`);
        failures++;
      } else if (process.env.VERBOSE) {
        console.log(`ok     ${label} : ${res.ratio.toFixed(2)}:1`);
      }
    }
  }
  console.log(failures ? `${failures} échec(s) sur ${total} mesures` : `contrastes AA : ${total} mesures, toutes conformes`);
  await browser.close();
  process.exit(failures ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
