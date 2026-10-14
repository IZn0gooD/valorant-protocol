// Le presse-papiers, dans les deux sens.
//
// Copier : texte et HTML d'un seul geste. Discord prend le texte (du Markdown),
// Word, Google Docs ou Notion prennent le HTML mis en forme. Des solutions de
// repli quand l'API asynchrone manque (carnet ouvert en http sur le réseau).
//
// Coller : le HTML de Word, Google Docs, Notion, Excel ou d'une page web devient
// du Markdown propre (titres, gras, listes, tableaux, liens).

// Les textes visibles passent par i18n.js, chargé avant les modules de la page.
const { t } = globalThis.I18N;

// ── Copier ────────────────────────────────────────────────────────────────────

/**
 * L'ancienne méthode : un évènement « copy » dont on remplit soi-même les données.
 * Safari n'active la commande que s'il existe une sélection : on en pose une,
 * invisible, le temps de la copie, puis on rend celle de l'utilisateur.
 */
function legacyCopy(text, html) {
  let done = false;
  const onCopy = (e) => {
    e.clipboardData.setData("text/plain", text);
    if (html) e.clipboardData.setData("text/html", html);
    e.preventDefault();
    done = true;
  };
  const sel = getSelection();
  const saved = sel?.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
  const holder = document.createElement("span");
  holder.className = "sr";
  holder.textContent = t("copie");
  document.body.append(holder);
  try {
    const range = document.createRange();
    range.selectNodeContents(holder);
    sel?.removeAllRanges();
    sel?.addRange(range);
    document.addEventListener("copy", onCopy, true);
    document.execCommand("copy");
  } catch {
  } finally {
    document.removeEventListener("copy", onCopy, true);
    sel?.removeAllRanges();
    if (saved) sel.addRange(saved);
    holder.remove();
  }
  return done;
}

/**
 * Écrit dans le presse-papiers `text`, et `html` s'il est fourni. `later` : une
 * promesse du HTML définitif (images embarquées) que le navigateur attend sans
 * perdre le geste de l'utilisateur ; `html` sert aux méthodes de repli.
 * `true` si la copie est faite.
 */
export async function copy({ text, html, later }) {
  if (html && typeof ClipboardItem === "function" && navigator.clipboard?.write) {
    const blob = (h) => new Blob([h], { type: "text/html" });
    try {
      await navigator.clipboard.write([
        new ClipboardItem({
          "text/plain": new Blob([text], { type: "text/plain" }),
          "text/html": later ? later.then(blob, () => blob(html)) : blob(html),
        }),
      ]);
      return true;
    } catch {}
  }
  if (legacyCopy(text, html)) return true;
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {}
  }
  return false;
}

const CALLOUTS = { note: t("ℹ Note"), tip: t("✔ Astuce"), important: t("❗ Important"), warning: t("⚠ Attention"), caution: t("⛔ Danger") };
const ALIGN = { al: "left", ac: "center", ar: "right" };

/**
 * Le HTML d'une note pour Word, Docs ou Notion : liens internes réduits à leur
 * texte, adresses rendues absolues, encadrés titrés, tableaux bordés et alignés,
 * cases à cocher en ☐ / ☑. Rien n'est téléchargé : voir `inlineImages`.
 */
export function richHtml(html, origin) {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  for (const a of doc.querySelectorAll("a[href]")) {
    const href = a.getAttribute("href");
    if (href.startsWith("#")) a.replaceWith(...a.childNodes);
    else if (href.startsWith("/")) a.setAttribute("href", origin + href);
  }
  for (const img of doc.querySelectorAll("img[src]")) {
    const src = img.getAttribute("src");
    if (src.startsWith("/")) img.setAttribute("src", origin + src);
  }
  for (const box of doc.querySelectorAll('input[type="checkbox"]')) box.replaceWith(box.checked ? "☑ " : "☐ ");
  // Le titre d'un encadré vient d'une règle CSS dans le carnet : ailleurs, il faut l'écrire.
  for (const q of doc.querySelectorAll('blockquote[class^="markdown-alert-"]')) {
    const p = doc.createElement("p");
    const b = doc.createElement("strong");
    b.textContent = CALLOUTS[q.className.slice("markdown-alert-".length)] ?? "Note";
    p.append(b);
    q.prepend(p);
  }
  for (const c of doc.querySelectorAll("th[class], td[class]")) {
    if (ALIGN[c.className]) c.setAttribute("align", ALIGN[c.className]);
  }
  // Tableaux bordés par les attributs d'antan, que Word et Docs comprennent encore :
  // un attribut `style` tomberait sous la CSP de la page (le presse-papiers analyse
  // ce HTML dans son contexte).
  const out = doc.body.innerHTML
    .replace(/<table>/g, '<table border="1" cellpadding="6" cellspacing="0">')
    .replace(/<(th|td)(?=[\s>])/g, '<$1 valign="top"');
  return `<meta charset="utf-8">${out}`;
}

const MAX_INLINE = 8 * 1024 * 1024;

function dataUrl(blob) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error);
    r.readAsDataURL(blob);
  });
}

/** Le HTML contient-il des images du carnet ? */
export const hasLocalImages = (html, origin) => html.includes(`src="${origin}/files/`);

/**
 * Les images du carnet embarquées dans le HTML (adresses `data:`) tant qu'elles
 * tiennent dans 8 Mo : le document collé ne dépend plus du carnet ouvert. Appelé
 * au moment de la copie seulement : afficher une note ne télécharge rien.
 */
export async function inlineImages(html, origin) {
  const doc = new DOMParser().parseFromString(html, "text/html");
  let budget = MAX_INLINE;
  for (const img of doc.querySelectorAll("img[src]")) {
    const src = img.getAttribute("src");
    if (budget <= 0 || !src.startsWith(`${origin}/files/`)) continue;
    try {
      const res = await fetch(src);
      if (!res.ok || Number(res.headers.get("Content-Length")) > budget) {
        res.body?.cancel();
        continue;
      }
      const blob = await res.blob();
      if (blob.size > budget) continue;
      img.setAttribute("src", await dataUrl(blob));
      budget -= blob.size;
    } catch {}
  }
  return `<meta charset="utf-8">${doc.body.innerHTML}`;
}

// ── Coller ────────────────────────────────────────────────────────────────────

const SKIP = new Set(["SCRIPT", "STYLE", "TEMPLATE", "HEAD", "META", "LINK", "TITLE", "NOSCRIPT", "SVG", "BUTTON", "SELECT", "TEXTAREA", "IFRAME", "OBJECT"]);
const BLOCK = new Set([
  "ADDRESS", "ARTICLE", "ASIDE", "BLOCKQUOTE", "CENTER", "DD", "DIV", "DL", "DT", "FIELDSET", "FIGCAPTION", "FIGURE", "FOOTER", "FORM",
  "H1", "H2", "H3", "H4", "H5", "H6", "HEADER", "HR", "LI", "MAIN", "NAV", "OL", "P", "PRE", "SECTION", "TABLE", "TBODY", "TD", "TFOOT", "TH",
  "THEAD", "TR", "UL",
]);
const BLOCKS = `${[...BLOCK].join(",").toLowerCase()},[data-md-list]`;

/** Du HTML qui porte une vraie mise en forme (et pas seulement des couleurs de code). */
export function looksRich(html) {
  return (
    /<(h[1-6]|li|table|blockquote|pre|b|strong|em|i|s|del|a)[\s>]/i.test(html) ||
    /font-weight\s*:\s*(bold|[6-9]00)|font-style\s*:\s*italic|mso-list/i.test(html)
  );
}

/** Du texte qui est déjà du Markdown : on le garde tel quel. */
export function looksLikeMarkdown(text) {
  return /^(#{1,6} |\s*[-*+] \S|\s*\d+[.)] \S|>|\|.*\|\s*$)/m.test(text) || /\*\*[^*\n]+\*\*|\[[^\]\n]+\]\([^)\s]+\)/.test(text);
}

// Word, Google Docs, Excel : leur HTML fait foi, même si leur texte brut ressemble à du Markdown.
const OFFICE = /id="docs-internal-guid|urn:schemas-microsoft-com:office|class="?Mso|Excel\.Sheet/i;
// Les puces d'un texte brut. Pas le tiret cadratin : en français, il ouvre une réplique.
const BULLETS = "•◦▪▫‣●○■□–·";
const HAS_BULLET = new RegExp(`^[ \\t]*[${BULLETS}][ \\t]+`, "m");

/** Un texte brut venu de Word ou d'un message : ses puces « • » deviennent des listes. */
export function tidyPlain(text) {
  return text
    .replace(/\r\n?/g, "\n")
    .replace(/\xa0/g, " ")
    .replace(new RegExp(`^([ \\t]*)[${BULLETS}][ \\t]+`, "gm"), (_, pad) => `${pad.replace(/\t/g, "  ")}- `);
}

/** Les données d'un « coller » en Markdown, ou `null` s'il vaut mieux laisser faire le navigateur. */
export function pastedMarkdown(data) {
  const text = data.getData("text/plain") || "";
  const html = data.getData("text/html") || "";
  // Un éditeur de code (VS Code…) copie un HTML coloré en `white-space: pre` : c'est du code, à garder tel quel.
  const code = /white-space\s*:\s*pre/i.test(html) && !/<(h[1-6]|li|table|blockquote)[\s>]/i.test(html);
  if (html && !code && looksRich(html) && (OFFICE.test(html) || !looksLikeMarkdown(text))) {
    const md = htmlToMarkdown(html);
    if (md.trim()) return md;
  }
  return HAS_BULLET.test(text) ? tidyPlain(text) : null;
}

// Les `style` du HTML collé sont lus, jamais appliqués : renommés avant l'analyse, ils
// n'ont pas à passer la CSP de la page (qui les refuserait, en le signalant à la console).
// Les blocs <style> (Word en met toujours un) sont retirés pour la même raison.
const STYLE_ATTR = "data-pasted-style";
const defuse = (html) =>
  html
    .replace(/<style\b[\s\S]*?<\/style\s*>/gi, "")
    .replace(/<[a-z][^>"']*(?:"[^"]*"[^>"']*|'[^']*'[^>"']*)*>/gi, (tag) => tag.replace(/(\s)style(\s*=)/gi, `$1${STYLE_ATTR}$2`));
const styleOf = (el) => (el.getAttribute?.(STYLE_ATTR) || "").toLowerCase();
/** La valeur d'une propriété CSS d'un style : `font-weight`, et pas `mso-bidi-font-weight`. */
const prop = (st, name) => st.match(new RegExp(`(?:^|;)\\s*${name}\\s*:\\s*([^;]*)`))?.[1].trim() ?? "";

/** Les marques qu'un élément pose : `true`, `false` (retirée explicitement) ou `undefined` (héritée). */
function marks(el) {
  const tag = el.tagName.toUpperCase();
  const st = styleOf(el);
  const fw = prop(st, "font-weight");
  const fs = prop(st, "font-style");
  let bold = /^(bold|bolder|[6-9]00)$/.test(fw) ? true : /^(normal|lighter|[1-4]00)$/.test(fw) ? false : undefined;
  // Google Docs enveloppe tout dans un <b style="font-weight:normal"> : ce n'est pas du gras.
  if ((tag === "B" || tag === "STRONG") && bold !== false) bold = true;
  let italic = fs === "italic" || fs === "oblique" ? true : fs === "normal" ? false : undefined;
  if ((tag === "I" || tag === "EM") && italic !== false) italic = true;
  const deco = `${prop(st, "text-decoration")} ${prop(st, "text-decoration-line")}`;
  const strike = ["S", "DEL", "STRIKE"].includes(tag) || /line-through/.test(deco) ? true : undefined;
  return { bold, italic, strike, code: ["CODE", "KBD", "SAMP", "TT"].includes(tag) };
}

const NONE = { bold: false, italic: false, strike: false };
const ORDER = ["bold", "italic", "strike"];
const escText = (s) => s.replace(/\\/g, "\\\\").replace(/([*`])/g, "\\$1");
const WORD = /^[\p{L}\p{N}]/u;
const LEAD_PUNCT = /^[\p{P}\p{S}]+/u;
const TRAIL_PUNCT = /[\p{P}\p{S}]+$/u;

/**
 * Le texte en ligne d'un nœud, en « morceaux » : du texte et l'état de ses marques.
 * On n'écrit les `**` qu'au moment où l'état change : deux morceaux gras voisins
 * (Google Docs en découpe beaucoup) donnent un seul `**…**`.
 */
function collect(node, ctx, st, out) {
  if (node.nodeType === 3) {
    const t = node.data.replace(/[\s\xa0]+/g, " ");
    if (t) out.push({ t: escText(t), st });
    return;
  }
  if (node.nodeType !== 1) return;
  const el = node;
  const tag = el.tagName.toUpperCase();
  if (SKIP.has(tag) || prop(styleOf(el), "mso-list") === "ignore") return;
  if (tag === "BR") return void out.push({ br: true });
  if (tag === "INPUT") {
    if (el.type === "checkbox") out.push({ t: el.checked ? "[x] " : "[ ] ", st: NONE, raw: true });
    return;
  }
  if (tag === "IMG") {
    // Les émojis de Discord ou de Slack sont des images : on garde leur texte.
    const alt = (el.getAttribute("alt") || "").trim();
    if (alt && (/emoji/i.test(el.className) || /^:[\w~+-]+:$/.test(alt) || [...alt].length <= 2)) out.push({ t: escText(alt), st });
    return;
  }
  const m = marks(el);
  const next = ctx.heading ? NONE : { bold: m.bold ?? st.bold, italic: m.italic ?? st.italic, strike: m.strike ?? st.strike };
  if (m.code) {
    const t = el.textContent.replace(/\s+/g, " ").trim();
    if (t) out.push({ t: t.includes("`") ? `\`\` ${t} \`\`` : `\`${t}\``, st: next, raw: true });
    return;
  }
  if (tag === "A") {
    const href = el.getAttribute("href") || "";
    const label = el.textContent.replace(/\s+/g, " ").trim();
    if (/^(https?:|mailto:)/i.test(href) && label) {
      const url = href.replace(/[()\s]/g, encodeURIComponent);
      out.push({ t: label === href ? `<${href}>` : `[${escText(label).replace(/[[\]]/g, "\\$&")}](${url})`, st: next, raw: true });
      return;
    }
  }
  for (const c of el.childNodes) collect(c, ctx, next, out);
}

/** Les morceaux en Markdown, marques ouvertes et fermées au plus juste. */
function emit(runs) {
  let s = "";
  const stack = []; // { mark, delim, start }
  // Ferme les marques au-delà des `k` premières ; `next` : le texte qui suivra.
  const close = (k, next = "") => {
    if (stack.length <= k) return;
    let trail = s.match(/\s*$/)[0];
    s = s.slice(0, s.length - trail.length);
    // Fermée sur une ponctuation et collée à une lettre, une marque ne se fermerait
    // pas (`**Note:**suite`) : la ponctuation passe dehors (`**Note**:suite`).
    if (!trail && WORD.test(next)) {
      const p = s.match(TRAIL_PUNCT)?.[0] ?? "";
      if (p && s.length - p.length > stack[stack.length - 1].start) {
        s = s.slice(0, -p.length);
        trail = p;
      }
    }
    while (stack.length > k) s += stack.pop().delim;
    s += trail;
  };
  for (const r of runs) {
    if (r.br) {
      close(0);
      s += "\n";
      continue;
    }
    if (!r.raw && !r.t.trim()) {
      s += r.t;
      continue;
    }
    const want = ORDER.filter((k) => r.st[k]);
    let k = 0;
    while (k < stack.length && want.includes(stack[k].mark)) k++;
    const lead = r.raw ? "" : r.t.match(/^\s*/)[0];
    let body = r.t.slice(lead.length);
    close(k, lead ? " " : body);
    s += lead;
    const opening = want.filter((m) => !stack.some((x) => x.mark === m));
    // Ouverte juste après une lettre, une marque suivie d'une ponctuation ne s'ouvrirait
    // pas (`mot**(x)**`) : la ponctuation passe devant.
    if (opening.length && !r.raw && WORD.test(s.slice(-1))) {
      const p = body.match(LEAD_PUNCT)?.[0] ?? "";
      if (p && p.length < body.length) {
        s += p;
        body = body.slice(p.length);
      }
    }
    for (const m of opening) {
      // Collée à un `*`, une marque s'écrit avec `_` : `**a*b***_c_` et non `**a*b****c*`.
      const alt = s.endsWith("*");
      const delim = m === "strike" ? "~~" : m === "bold" ? (alt ? "__" : "**") : alt ? "_" : "*";
      s += delim;
      stack.push({ mark: m, delim, start: s.length });
    }
    s += body;
  }
  close(0);
  return s;
}

/** Espaces compactés, lignes nettoyées. */
function clean(s) {
  return s
    .split("\n")
    .map((l) => l.replace(/[ \t]+/g, " ").trim())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

const runsOf = (nodes, ctx) => {
  const runs = [];
  for (const n of nodes) collect(n, ctx, ctx.heading ? NONE : ctx.base, runs);
  return runs;
};

/** Une ligne de texte qui commencerait par une marque de bloc la voit échappée. */
function guardLine(l) {
  if (/^(#{1,6}(\s|$)|[-+](\s|$)|>|~~~|```)/.test(l)) return `\\${l}`;
  const n = l.match(/^(\d{1,9})([.)])(\s|$)/);
  if (n) return `${n[1]}\\${l.slice(n[1].length)}`;
  // Une définition de lien `[1]: …`, ou `===` / `---` sous une ligne (un titre), ou `___` (un trait).
  if (/^\[[^\]]*\]:/.test(l) || /^(=+|-+|_+)\s*$/.test(l)) return `\\${l}`;
  return l;
}
const guard = (s) => s.split("\n").map(guardLine).join("\n");

const isBlock = (n) => n.nodeType === 1 && (BLOCK.has(n.tagName.toUpperCase()) || !!n.dataset?.mdList);
/** Un élément en ligne qui contient des blocs (le <b> qui enveloppe un collage Google Docs) : un simple conteneur. */
const wrapsBlocks = (n) => n.nodeType === 1 && !isBlock(n) && !SKIP.has(n.tagName.toUpperCase()) && !!n.querySelector(BLOCKS);

function wordList(items, ctx) {
  const counters = [];
  return items
    .map((p) => {
      const ordered = p.dataset.mdList[0] === "o";
      const level = Math.max(1, Number(p.dataset.mdList.slice(1)) || 1);
      counters.length = level;
      counters[level - 1] = (counters[level - 1] || 0) + 1;
      const marker = ordered ? `${counters[level - 1]}. ` : "- ";
      // Quatre espaces par niveau : assez pour s'imbriquer sous « 1. » comme sous « 10. ».
      return `${"    ".repeat(level - 1)}${marker}${guard(clean(emit(runsOf(p.childNodes, ctx))).replace(/\n/g, " "))}`;
    })
    .join("\n");
}

function children(el, ctx) {
  const out = [];
  let para = [];
  const flush = () => {
    const t = clean(emit(para));
    if (t) out.push(guard(t));
    para = [];
  };
  const kids = [...el.childNodes];
  for (let i = 0; i < kids.length; i++) {
    const c = kids[i];
    if (wrapsBlocks(c)) {
      // Ses marques (un <strong> autour de paragraphes) passent aux blocs qu'il contient.
      flush();
      const m = marks(c);
      const base = { bold: m.bold ?? ctx.base.bold, italic: m.italic ?? ctx.base.italic, strike: m.strike ?? ctx.base.strike };
      out.push(...children(c, { ...ctx, base }));
      continue;
    }
    if (!isBlock(c)) {
      collect(c, ctx, ctx.base, para);
      continue;
    }
    flush();
    if (c.dataset.mdList) {
      // Une liste de Word : des paragraphes successifs, chacun avec son niveau.
      const items = [];
      while (i < kids.length && (kids[i].dataset?.mdList || (kids[i].nodeType === 3 && !kids[i].data.trim()))) {
        if (kids[i].dataset?.mdList) items.push(kids[i]);
        i++;
      }
      i--;
      out.push(wordList(items, ctx));
      continue;
    }
    out.push(...block(c, ctx));
  }
  flush();
  return out;
}

/** Une liste ; `pad` : le retrait de ses éléments. Une sous-liste se cale sur le texte de son parent. */
function list(el, ctx, pad) {
  const ordered = el.tagName.toUpperCase() === "OL";
  let n = Number(el.getAttribute("start")) || 1;
  const lines = [];
  let inner = `${pad}  `;
  for (const li of el.children) {
    const tag = li.tagName.toUpperCase();
    if (tag === "UL" || tag === "OL") {
      lines.push(...list(li, ctx, inner));
      continue;
    }
    if (tag !== "LI") continue;
    let marker = ordered ? `${n++}. ` : "- ";
    inner = pad + " ".repeat(marker.length);
    // Les cases à cocher de Google Docs.
    if (li.getAttribute("role") === "checkbox") marker += li.getAttribute("aria-checked") === "true" ? "[x] " : "[ ] ";
    const runs = [];
    const nested = [];
    for (const c of li.childNodes) {
      const t = c.nodeType === 1 ? c.tagName.toUpperCase() : "";
      if (t === "UL" || t === "OL") nested.push(...list(c, ctx, inner));
      else if (t === "P" || t === "DIV") {
        if (runs.length) runs.push({ br: true });
        collect(c, ctx, ctx.base, runs);
      } else if (isBlock(c)) nested.push(...block(c, ctx).map((b) => b.replace(/^(?=.)/gm, inner)));
      else collect(c, ctx, ctx.base, runs);
    }
    const text = guard(clean(emit(runs))).replace(/\n/g, `\n${inner}`);
    lines.push(`${pad}${marker}${text}`.trimEnd(), ...nested);
  }
  return lines;
}

function table(el, ctx) {
  const rows = [...el.querySelectorAll("tr")].filter((tr) => tr.closest("table") === el);
  const spans = []; // colonne → lignes encore couvertes par un `rowspan` d'au-dessus
  const grid = rows.map((tr) => {
    const out = [];
    let col = 0;
    const skip = () => {
      while (spans[col] > 0) {
        out.push("");
        spans[col]--;
        col++;
      }
    };
    for (const c of tr.children) {
      if (!/^(TD|TH)$/i.test(c.tagName)) continue;
      skip();
      const t = children(c, ctx).join(" ").replace(/\n+/g, " ").replace(/\|/g, "\\|");
      const cs = Math.max(1, Number(c.getAttribute("colspan")) || 1);
      const rs = Math.max(1, Number(c.getAttribute("rowspan")) || 1);
      for (let k = 0; k < cs; k++) {
        out.push(k ? "" : t);
        if (rs > 1) spans[col] = rs - 1;
        col++;
      }
    }
    skip();
    return out;
  });
  const width = Math.max(0, ...grid.map((r) => r.length));
  if (!width) return [];
  const line = (r) => `| ${Array.from({ length: width }, (_, i) => r[i] || "").join(" | ")} |`;
  return [[line(grid[0]), `|${" --- |".repeat(width)}`, ...grid.slice(1).map(line)].join("\n")];
}

function block(el, ctx) {
  const tag = el.tagName.toUpperCase();
  if (SKIP.has(tag)) return [];
  if (/^H[1-6]$/.test(tag)) {
    const t = clean(emit(runsOf(el.childNodes, { ...ctx, heading: true }))).replace(/\n+/g, " ");
    return t ? [`${"#".repeat(Number(tag[1]))} ${t}`] : [];
  }
  if (tag === "UL" || tag === "OL") {
    const lines = list(el, ctx, "");
    return lines.length ? [lines.join("\n")] : [];
  }
  if (tag === "TABLE") return table(el, ctx);
  if (tag === "HR") return ["---"];
  if (tag === "PRE") {
    const t = el.textContent.replace(/\n$/, "");
    if (!t.trim()) return [];
    // Une clôture plus longue que tout ce que le code contient.
    const longest = Math.max(0, ...[...t.matchAll(/`+/g)].map((m) => m[0].length));
    const fence = "`".repeat(Math.max(3, longest + 1));
    return [`${fence}\n${t}\n${fence}`];
  }
  if (tag === "BLOCKQUOTE") {
    const inner = children(el, ctx).join("\n\n");
    return inner ? [inner.replace(/^/gm, "> ").replace(/^> $/gm, ">")] : [];
  }
  return children(el, ctx);
}

/** Les listes de Word sont des paragraphes stylés : on les repère avant la conversion. */
function markWordLists(root) {
  for (const p of root.querySelectorAll("p")) {
    const level = prop(styleOf(p), "mso-list").match(/^l\d+\s+level(\d+)/);
    if (!level && !/MsoListParagraph/i.test(p.className)) continue;
    const marker = [...p.querySelectorAll("span")].find((s) => prop(styleOf(s), "mso-list") === "ignore");
    const ordered = /^\s*(\d+|[a-z]|[ivxlc]+)[.)]/i.test(marker?.textContent || "");
    p.dataset.mdList = `${ordered ? "o" : "u"}${level ? level[1] : 1}`;
  }
}

/** Du HTML collé, en Markdown. */
export function htmlToMarkdown(html) {
  const doc = new DOMParser().parseFromString(defuse(html), "text/html");
  markWordLists(doc.body);
  return children(doc.body, { base: NONE }).join("\n\n").trim();
}

/**
 * Un titre pour un texte collé : son premier `# Titre` (retiré du texte, la page
 * l'affiche déjà), sinon sa première ligne si elle est courte.
 */
export function guessTitle(md) {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const i = lines.findIndex((l) => l.trim());
  if (i < 0) return { title: "", body: md };
  const first = lines[i].trim();
  const h = first.match(/^#{1,2}\s+(.+?)(?:\s+#+)?\s*$/);
  if (h) {
    const rest = lines.slice(i + 1).join("\n").replace(/^\n+/, "");
    return { title: h[1].replace(/[*_`\\]/g, "").trim(), body: rest };
  }
  const plain = first.replace(/^[-*+>]\s+|^\d+[.)]\s+/, "").replace(/[*_`#|\\]/g, "").trim();
  return { title: plain.length <= 80 ? plain : `${plain.slice(0, 77).trimEnd()}…`, body: md };
}
