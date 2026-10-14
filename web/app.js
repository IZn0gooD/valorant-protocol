// protocol : l'interface. Pas de framework : du HTML échappé, quelques vues et
// l'API locale. Le décor suit les pièces « ui » d'ascii.rest : cadres, contrôles
// entre crochets, séparateurs, arborescence ; ses modules dessinent le titre de
// l'accueil et la mini-carte. Le copier-coller passe par clip.js.

import { mount } from "./vendor/mount.js";
import { banner } from "./vendor/banner.js";
import * as minimap from "./pieces.js";
import { copy as clipCopy, richHtml, inlineImages, hasLocalImages, pastedMarkdown, guessTitle } from "./clip.js";

// ── Outils ────────────────────────────────────────────────────────────────────

// Les textes passent par i18n.js (chargé avant ce module) : la clé est la phrase française.
const { t, tp, ago, fmtSize } = globalThis.I18N;

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const enc = encodeURIComponent;
const ascii = (s) => String(s).normalize("NFD").replace(/\p{M}/gu, "");
const norm = (s) => String(s ?? "").replace(/\r\n/g, "\n").trim();
const byteSize = (s) => new Blob([s]).size;
const cap = (s) => (s ? s[0].toUpperCase() + s.slice(1) : "");
const page = $("#page");
const find = $("#find");

class ApiError extends Error {
  constructor(status, message, data) {
    super(message);
    this.status = status;
    this.data = data;
  }
}

function setOffline(off) {
  $("#offline").hidden = !off;
}

/** Annonce aux lecteurs d'écran (sans rien afficher). */
function announce(msg) {
  const a = $("#announce");
  a.textContent = "";
  requestAnimationFrame(() => (a.textContent = msg));
}

async function api(method, path, body) {
  const init = { method, headers: {} };
  if (body !== undefined) {
    if (body instanceof Blob) {
      init.body = body;
      init.headers["Content-Type"] = body.type || "application/octet-stream";
    } else {
      init.body = JSON.stringify(body);
      init.headers["Content-Type"] = "application/json";
    }
  }
  let res;
  try {
    res = await fetch(path, init);
  } catch {
    setOffline(true);
    throw new ApiError(0, t("le carnet ne répond pas"));
  }
  setOffline(false);
  const data = (res.headers.get("Content-Type") || "").includes("json") ? await res.json() : await res.text();
  if (!res.ok) throw new ApiError(res.status, (data && data.error) || t("erreur {status}", { status: res.status }), data);
  return data;
}

// Lecture avec ETag : une réponse 304 réutilise la copie locale (bornée).
const etags = new Map();
const ETAGS_MAX = 200;
async function cached(path, force = false) {
  const c = etags.get(path);
  let res;
  try {
    res = await fetch(path, { headers: c && !force ? { "If-None-Match": c.tag } : {} });
  } catch {
    setOffline(true);
    throw new ApiError(0, t("le carnet ne répond pas"));
  }
  setOffline(false);
  if (res.status === 304 && c) {
    await res.arrayBuffer().catch(() => {}); // corps vide, lu pour clore proprement la requête
    return { data: c.data, fresh: false };
  }
  const data = await res.json();
  if (!res.ok) throw new ApiError(res.status, data.error || t("erreur {status}", { status: res.status }), data);
  const tag = res.headers.get("ETag");
  if (tag) {
    etags.delete(path);
    etags.set(path, { tag, data });
    if (etags.size > ETAGS_MAX) etags.delete(etags.keys().next().value);
  }
  return { data, fresh: true };
}

/**
 * Refait une partie de l'écran sans perdre l'élément qui avait le focus : on note
 * comment le retrouver (sélecteur, zone de l'écran, rang parmi ses semblables).
 */
function focusKey() {
  const a = document.activeElement;
  if (!a || a === document.body || a === page) return null;
  let sel = null;
  if (a.id) sel = `#${CSS.escape(a.id)}`;
  for (const k of ["act", "copy", "md", "block"]) {
    if (!sel && a.dataset?.[k]) sel = `[data-${k}="${CSS.escape(a.dataset[k])}"]`;
  }
  if (!sel && a.getAttribute?.("popovertarget")) sel = `[popovertarget="${CSS.escape(a.getAttribute("popovertarget"))}"]`;
  if (!sel && a.name && a.value !== undefined) sel = `input[name="${CSS.escape(a.name)}"][value="${CSS.escape(a.value)}"]`;
  const href = a.getAttribute?.("href");
  if (!sel && href) sel = `[href="${CSS.escape(href)}"]`;
  if (!sel) return null;
  // Le même lien peut figurer dans la barre latérale et dans la page : on retient la zone et le rang.
  const root = a.closest("#page, #side, .bar") ?? document.body;
  return { sel, root, i: $$(sel, root).indexOf(a) };
}
function refocus(key) {
  const a = document.activeElement;
  // Le focus est encore là (rien n'a été redessiné sous lui) : on n'y touche pas.
  if (!key || (a && a !== document.body && a.isConnected)) return;
  const all = $$(key.sel, key.root.isConnected ? key.root : document);
  (all[key.i] ?? all[0])?.focus({ preventScroll: true });
}
async function keepFocus(fn) {
  const key = focusKey();
  await fn();
  refocus(key);
}

// ── État ──────────────────────────────────────────────────────────────────────

const S = {
  state: null,
  route: { view: "home" },
  mapData: null,
  note: null,
  sig: null,
  lastMap: null,
  cleanups: [],
  editor: null,
  beforeSearch: "#/",
  hits: [],
  hitIndex: 0,
  filters: {},
  toastActions: {},
  toastTimer: 0,
  share: null,
  pendingPaste: null,
};

/** Une vue asynchrone dont la route a été remplacée entre-temps ne dessine plus rien. */
const stale = (r) => r !== S.route;

async function loadState(force = false) {
  const { data, fresh } = await cached("/api/state", force);
  S.state = data;
  return fresh;
}
const mapsList = () => S.state?.maps ?? [];
const kinds = () => S.state?.kinds ?? [];
const kindOf = (id) => kinds().find((k) => k.id === id);
const kindLabel = (id) => kindOf(id)?.label ?? cap(id);
const mapTitle = (id) => mapsList().find((m) => m.id === id)?.title ?? id;
const readOnly = () => !!S.state?.readOnly;
const noteHref = (n) => `#/note/${enc(n.map)}/${enc(n.id)}`;
// Le libellé d'un côté vient du serveur, déjà dans la langue du carnet ; sinon, le nôtre.
const sideWord = (s) =>
  s === "atk" || s === "def" ? (S.state?.sides?.find((x) => x.id === s)?.label ?? (s === "atk" ? t("Attaque") : t("Défense"))) : "";
const sidesFilter = () => [["", t("Tous")], ["atk", sideWord("atk")], ["def", sideWord("def")]];
const sidesForm = () => [["", t("Les deux")], ["atk", sideWord("atk")], ["def", sideWord("def")]];

// ── Pièces d'interface (d'après les pièces « ui » d'ascii.rest) ───────────────

/** [ libellé ] : un bouton. `label` est du HTML déjà sûr. */
const btn = (label, attrs = "", cls = "") => `<button class="btn${cls ? ` ${cls}` : ""}" type="button"${attrs ? ` ${attrs}` : ""}><span>${label}</span></button>`;
/** Un lien qui a l'allure d'un bouton ; `href` est brut, échappé ici. */
const linkBtn = (href, label, attrs = "", cls = "") => `<a class="btn${cls ? ` ${cls}` : ""}" href="${esc(href)}"${attrs ? ` ${attrs}` : ""}><span>${label}</span></a>`;
/** [x] libellé : une bascule (`aria-pressed`). */
const toggleBtn = (label, on, attrs = "") => `<button class="toggle" type="button" aria-pressed="${!!on}"${attrs ? ` ${attrs}` : ""}><span>${label}</span></button>`;
/** La marque d'un menu déroulant, au bout d'un libellé : « Plus v ». */
const DOWN = '<span aria-hidden="true"> v</span>';

/** ── Titre (3) ──────────── [ action ] : le séparateur qui ouvre une section. */
const sh = (id, title, n = null, action = "") =>
  `<div class="sh"><h2 id="${id}">${esc(title)}</h2>${n ? `<span class="n">(${n})</span>` : ""}<span class="fill" aria-hidden="true"></span>${action}</div>`;

/** Les repères d'une note, séparés par des points : Ascent · Retake · Défense · épinglée. */
function facts(n, { map = false, kind = true, pin = false } = {}) {
  const side = sideWord(n.side);
  return [
    map ? `<span>${esc(mapTitle(n.map))}</span>` : "",
    kind ? `<span>${esc(kindLabel(n.kind))}</span>` : "",
    side ? `<span class="${n.side}">${side}</span>` : "",
    pin && n.pin ? `<span class="pin">${t("épinglée")}</span>` : "",
  ].join("");
}
const factsText = (n, { map = false } = {}) => [map ? mapTitle(n.map) : "", kindLabel(n.kind), sideWord(n.side)].filter(Boolean).join(" · ");

/** Une note dans son cadre : le type dans le trait du haut, la date dans celui du bas. */
function card(n, { withMap = false, pinTag = true } = {}) {
  const meta = facts(n, { map: withMap, kind: false });
  return (
    `<a class="card box" href="${esc(noteHref(n))}">` +
    `<span class="card-title">${esc(n.title)}</span>` +
    (meta ? `<span class="meta">${meta}</span>` : "") +
    `<span class="excerpt">${n.excerpt ? esc(n.excerpt) : t("Note vide.")}</span>` +
    `<span class="box-title">${esc(kindLabel(n.kind))}</span>` +
    (pinTag && n.pin ? `<span class="box-tag">${t("épinglée")}</span>` : "") +
    `<span class="box-hint">${esc(t("modifiée {ago}", { ago: ago(n.updated) }))}</span></a>`
  );
}

/** ( ) choix, (*) pour celui qui est pris : de vrais boutons radio derrière leur libellé. */
function radios(name, items, checked) {
  return items
    .map(
      ([value, label, n]) =>
        `<label class="radio${sideWord(value) ? ` ${value}` : ""}"><input type="radio" name="${esc(name)}" value="${esc(value)}"${value === checked ? " checked" : ""}>` +
        `<span>${esc(label)}</span>${n !== undefined ? `<span class="n">${n}</span>` : ""}</label>`,
    )
    .join("");
}
/** Une ligne « libellé   (*) a   ( ) b », groupée pour les lecteurs d'écran. */
const radioRow = (name, label, items, checked, describedBy = "") =>
  `<div class="row" role="radiogroup" aria-labelledby="${name}-l"${describedBy ? ` aria-describedby="${describedBy}"` : ""}>` +
  `<span class="lbl" id="${name}-l">${esc(label)}</span><div class="choices">${radios(name, items, checked)}</div></div>`;
/** [ champ ] ou [ liste  v ] : la commande d'une ligne de formulaire. */
const field = (control, select = false) => `<div class="input${select ? " select" : ""}">${control}</div>`;
const row = (label, id, control) => `<div class="row"><label for="${id}">${esc(label)}</label>${control}</div>`;
const options = (items, selected) => items.map(([v, label]) => `<option value="${esc(v)}"${v === selected ? " selected" : ""}>${esc(label)}</option>`).join("");

function crumbs(...parts) {
  const items = [`<a href="#/">${t("Accueil")}</a>`];
  parts.forEach(([label, href], i) => {
    items.push(href && i < parts.length - 1 ? `<a href="${esc(href)}">${esc(label)}</a>` : `<span aria-current="page">${esc(label)}</span>`);
  });
  return `<nav class="crumbs" aria-label="${esc(t("Fil d'Ariane"))}">${items.join('<span class="sep" aria-hidden="true">/</span>')}</nav>`;
}

/** Un menu déroulant (API popover) : boutons d'action ou liens. */
function menu(id, label, items) {
  const body = items
    .filter(Boolean)
    .map((it) => {
      if (it === "sep") return '<div class="menu-sep" role="separator"></div>';
      const inner = `<b>${esc(it.title)}</b>${it.hint ? `<small>${esc(it.hint)}</small>` : ""}`;
      const cls = `menu-item${it.cls ? ` ${it.cls}` : ""}`;
      return it.href
        ? `<a class="${cls}" role="menuitem" href="${esc(it.href)}"${it.download ? " download" : ""}>${inner}</a>`
        : `<button class="${cls}" type="button" role="menuitem" ${it.attrs || ""}>${inner}</button>`;
    })
    .join("");
  return `<div class="menu" id="${id}" popover role="menu" aria-label="${esc(label)}">${body}</div>`;
}
const menuButton = (id, label, content, popup = "menu") =>
  `<button class="btn" type="button" popovertarget="${id}" aria-haspopup="${popup}" aria-expanded="false" aria-label="${esc(label)}" title="${esc(label)}"><span>${content}</span></button>`;

const COPY_FORMATS = [
  { f: "discord", title: t("Pour Discord"), hint: t("Markdown, tableaux alignés en bloc de code") },
  { f: "rich", title: t("Pour Word, Docs ou Notion"), hint: t("Titres, listes et tableaux mis en forme") },
  { f: "text", title: t("Texte simple"), hint: t("Sans aucune mise en forme") },
  { f: "markdown", title: t("Fichier Markdown complet"), hint: t("En-tête compris : pour un autre carnet ou Obsidian") },
];
/** [ Copier ][ v ] : la copie la plus courante d'un clic, les autres formats dans le menu. */
function copyControl(what) {
  return (
    `<span class="split" role="group" aria-label="${esc(t("Copier"))}">` +
    btn(esc(what), `data-copy="auto" title="${esc(t("Copier pour Discord, Word ou Docs (touche C)"))}"`) +
    menuButton("copy-menu", t("Choisir le format de copie"), '<span aria-hidden="true">v</span>') +
    `</span>` +
    menu(
      "copy-menu",
      t("Copier pour…"),
      COPY_FORMATS.map((c) => ({ title: c.title, hint: c.hint, attrs: `data-copy="${c.f}"` })),
    )
  );
}

// ── Art ascii (ascii.rest) ────────────────────────────────────────────────────

function teardown() {
  for (const stop of S.cleanups.splice(0)) {
    try {
      stop();
    } catch {}
  }
}

function art(el, piece, { label, width, mono } = {}) {
  // Un rendu concurrent a déjà remplacé la page : rien à animer.
  if (!el?.isConnected) return;
  const canvas = !!piece.meta.palette && !mono;
  const target = document.createElement(canvas ? "canvas" : "pre");
  target.className = "art";
  target.setAttribute("role", "img");
  target.setAttribute("aria-label", label || piece.meta.name);
  if (canvas) {
    if (width) target.style.width = width;
  } else {
    target.style.setProperty("--cols", piece.meta.cols);
    target.style.setProperty("--rows", piece.meta.rows);
  }
  el.replaceChildren(target);
  try {
    S.cleanups.push(mount(target, piece));
  } catch (e) {
    console.warn("protocol : animation indisponible", e);
  }
}

const GLINT = { dark: ["#f97316", "#f778ba"], light: ["#c2410c", "#be185d"] };
const SHADOW = { dark: "#3a414c", light: "#c4cad3" };

/** Le titre de l'accueil en lettres-blocs ; si la police ne sait pas l'écrire, du texte simple. */
function bannerInto(el, text, { effect = "still", cellPx = 8 } = {}) {
  if (!el) return;
  const word = ascii(text).toUpperCase().replace(/[^A-Z0-9 .,!?'\-+=/:_]/g, "").trim();
  let piece;
  try {
    if (!word) throw new Error("rien à dessiner");
    piece = banner(word, { font: "block", effect, color: GLINT, shadowColor: SHADOW });
  } catch {
    el.textContent = text;
    return;
  }
  art(el, piece, { label: text, width: `min(100%, ${piece.meta.cols * cellPx}px)` });
}

function enhanceDocs(root) {
  if (!root) return;
  for (const a of $$("a[href]", root)) {
    const href = a.getAttribute("href");
    // Tout ce qui sort de l'interface s'ouvre à côté : la fenêtre du carnet ne se ferme pas.
    if (/^https?:|^mailto:|^\/files\/|^\/static\//.test(href)) {
      a.target = "_blank";
      a.rel = "noopener noreferrer";
    }
  }
  for (const img of $$("img", root)) img.loading = "lazy";
  for (const tbl of $$("table", root)) {
    if (tbl.parentElement?.classList.contains("table-wrap")) continue;
    const w = document.createElement("div");
    w.className = "table-wrap";
    tbl.replaceWith(w);
    w.append(tbl);
  }
  // Les cases d'une checklist s'écrivent [ ] et [x] ; la vraie case reste pour les lecteurs d'écran.
  for (const box of $$('input[type="checkbox"]', root)) {
    if (box.classList.contains("sr")) continue;
    const g = document.createElement("span");
    g.className = box.checked ? "check on" : "check";
    g.setAttribute("aria-hidden", "true");
    g.textContent = box.checked ? "[x] " : "[ ] ";
    box.classList.add("sr");
    box.before(g);
    box.closest("li")?.classList.add("task");
  }
}

// ── Barre du haut et navigation ───────────────────────────────────────────────

const isDark = () => document.documentElement.dataset.theme === "dark";

function renderChrome() {
  const st = S.state;
  if (!st) return;
  const key = focusKey();
  const r = S.route;
  const cur = r.map || "";
  const ro = readOnly();
  const dark = isDark();
  const themeLabel = dark ? t("Passer en thème clair") : t("Passer en thème sombre");
  $("#barEnd").innerHTML =
    (ro
      ? `<span class="ro" title="${esc(t("Les modifications se font sur le poste du coach"))}">${t("lecture seule")}</span>`
      : linkBtn(
          `#/new/${enc(cur)}`,
          `<span aria-hidden="true">+ </span><span class="label">${t("Nouvelle note")}</span>`,
          `title="${esc(t("Nouvelle note (touche N)"))}"`,
          "primary",
        )) +
    btn(
      // En petit écran, seul le mode reste (« sombre ») : le mot « thème » est dans .label.
      t("<span class=\"label\">thème </span>{mode}", { mode: dark ? t("clair") : t("sombre") }),
      `data-act="theme" aria-label="${esc(themeLabel)}" title="${esc(themeLabel)}"`,
    );

  // La navigation est une arborescence : les maps, et sous la map ouverte, sa fiche et ses notes.
  const maps = mapsList();
  const current = (on) => (on ? ' aria-current="page"' : "");
  const mapRows = (m, last) => {
    const open = m.id === cur;
    let s =
      `<li class="t${last ? " last" : ""}${open ? " here" : ""}"><a class="dir" href="#/map/${esc(enc(m.id))}"${current(open && r.view === "map")}>${esc(m.title)}</a>` +
      `<span class="n" aria-label="${tp(m.count, "{n} note", "{n} notes")}">${m.count}</span></li>`;
    const notes = open && S.mapData?.map?.id === m.id ? S.mapData.notes : null;
    if (!notes) return s;
    const kids = [
      { href: `#/sheet/${enc(m.id)}`, title: t("Fiche match"), on: r.view === "sheet" },
      ...notes.map((n) => ({ href: noteHref(n), title: n.title, on: (r.view === "note" || r.view === "edit") && r.id === n.id, pin: n.pin })),
    ];
    kids.forEach((k, i) => {
      s +=
        `<li class="t d2${i === kids.length - 1 ? " last" : ""}${last ? "" : " cont"}${k.on ? " here" : ""}"><a href="${esc(k.href)}"${current(k.on)}>${esc(k.title)}</a>` +
        (k.pin ? `<span class="pin" title="${t("Épinglée")}"><span aria-hidden="true">*</span><span class="sr">${t("épinglée")}</span></span>` : "") +
        "</li>";
    });
    return s;
  };
  const tree = (list, labelled = "") => `<ul${labelled ? ` aria-labelledby="${labelled}"` : ""}>${list.map((m, i) => mapRows(m, i === list.length - 1)).join("")}</ul>`;
  const team = maps.filter((m) => m.team);
  const others = maps.filter((m) => !m.team);
  const groups = others.some((m) => m.pool === true)
    ? [[t("pool actif"), others.filter((m) => m.pool === true)], [t("hors pool"), others.filter((m) => m.pool !== true)]]
    : [[t("maps"), others]];
  let html = `<div class="grp"><a class="root" href="#/"${current(r.view === "home")}>${t("Accueil")}</a>${team.length ? tree(team) : ""}</div>`;
  groups.forEach(([label, list], i) => {
    if (!list.length && i > 0) return;
    const add = i === 0 && !ro ? linkBtn("#/new-map", "+", `aria-label="${t("Ajouter une map")}" title="${t("Ajouter une map")}"`, "small") : "";
    html += `<div class="grp"><p class="cap"><span id="g-${i}">${label}</span><span class="fill" aria-hidden="true"></span>${add}</p>${list.length ? tree(list, `g-${i}`) : ""}</div>`;
  });
  // Comme la commande `tree` : le compte en dernière ligne.
  html += `<p class="foot-line">${tp(others.length, "{n} map", "{n} maps")}, ${tp(st.total, "{n} note", "{n} notes")}</p>`;
  $("#nav").innerHTML = html;
  $("#foot").innerHTML =
    (ro || !st.local ? "" : linkBtn("#/trash", `${t("Corbeille")}${st.trash ? ` (${st.trash})` : ""}`, current(r.view === "trash").trim(), "small")) +
    linkBtn("#/help", t("Aide"), current(r.view === "help").trim(), "small") +
    (st.local ? btn(t("Quitter"), 'data-act="quit"', "small") : "");
  refocus(key);
}

// En petit écran, la navigation est un tiroir : fermé, il est `inert` (hors du
// parcours au clavier et des lecteurs d'écran) ; ouvert, il prend le focus.
const narrow = matchMedia("(width <= 900px)");
function setDrawer(open) {
  const side = $("#side");
  const was = side.classList.contains("open");
  side.classList.toggle("open", open);
  side.inert = narrow.matches && !open;
  $("#scrim").hidden = !open;
  $("#menu").setAttribute("aria-expanded", String(open));
  if (open === was) return;
  if (open) ($('[aria-current="page"]', side) || $("a, button", side))?.focus();
  else if (side.contains(document.activeElement) || document.activeElement === document.body) $("#menu").focus();
}
narrow.addEventListener("change", () => setDrawer(false));
setDrawer(false);

// ── Vues ──────────────────────────────────────────────────────────────────────

function viewHome() {
  const st = S.state;
  S.mapData = null;
  renderChrome();
  // `général` (les règles de l'équipe) n'est pas une map de jeu : elle a sa place à part.
  const maps = mapsList().filter((m) => !m.team);
  const team = mapsList().filter((m) => m.team);
  const pinned = st.pinned || [];
  const recent = (st.recent || []).slice(0, 8);
  const ro = readOnly();
  const tile = (m) =>
    `<a class="tile box" href="#/map/${esc(enc(m.id))}"><span class="name">${esc(m.title)}</span>` +
    `<span class="info">${tp(m.count, "{n} note", "{n} notes")}${m.pinned ? ` · ${tp(m.pinned, "{n} épinglée", "{n} épinglées")}` : ""}</span>` +
    `${m.pool === true ? '<span class="box-hint">pool</span>' : ""}</a>`;
  const line = (n) =>
    `<li><a href="${esc(noteHref(n))}"><span class="t-title">${esc(n.title)}</span><span class="dots" aria-hidden="true"></span>` +
    `<span class="m">${esc(factsText(n, { map: true }))} · ${esc(ago(n.updated))}</span></a></li>`;
  page.innerHTML = `
    <h1 class="sr">${t("Accueil")}</h1>
    ${st.notice ? `<div id="home-notice">${notice("", t("Carnet"), esc(st.notice), btn(t("Compris"), 'data-act="notice-ok"', "small"))}</div>` : ""}
    <section class="intro box round" aria-label="protocol">
      <div class="intro-text">
        <div class="brand-banner" id="brand" aria-hidden="true"></div>
        <p class="lede">${t("Les protocoles de l'équipe, rangés map par map : setups, exécutions, retakes, lineups, anti-strats.")} ${tp(
          st.total,
          "{n} note",
          "{n} notes",
        )}, ${tp(maps.length, "{n} map", "{n} maps")}.</p>
        ${ro ? "" : `<div class="actions">${linkBtn("#/new/", t("Nouvelle note"), "", "primary")}${linkBtn("#/new/?paste=1", t("Coller un protocole"))}</div>`}
      </div>
      <div class="radar" id="radar"></div>
      <span class="box-hint">${ro ? t("/ rechercher") : t("n nouvelle note · ctrl+v coller un texte · / rechercher")}</span>
    </section>
    <section aria-labelledby="h-pinned">
      ${sh("h-pinned", t("Épinglées"), pinned.length)}
      ${
        pinned.length
          ? `<div class="cards">${pinned.map((n) => card(n, { withMap: true, pinTag: false })).join("")}</div>`
          : `<p class="empty-line box dashed">${t("Épinglez les protocoles clés : ils s'affichent ici et dans la fiche match de leur map, prêts pour un temps mort.")}</p>`
      }
    </section>
    <section aria-labelledby="h-maps">
      ${sh("h-maps", t("Maps"), maps.length, ro ? "" : linkBtn("#/new-map", t("Ajouter une map"), "", "small"))}
      <div class="tiles">${maps.map(tile).join("")}</div>
    </section>
    ${team.length ? `<section aria-labelledby="h-team">${sh("h-team", t("Équipe"))}<div class="tiles">${team.map(tile).join("")}</div></section>` : ""}
    ${recent.length ? `<section aria-labelledby="h-recent">${sh("h-recent", t("Modifiées récemment"))}<ul class="leaders">${recent.map(line).join("")}</ul></section>` : ""}`;
  art($("#radar"), minimap, { label: t("Mini-carte : un radar balaie une map, les contacts apparaissent à son passage") });
  bannerInto($("#brand"), "protocol", { effect: "glint", cellPx: 7 });
}

function filteredNotes(notes, f) {
  return notes.filter((n) => (!f.kind || n.kind === f.kind) && (!f.side || !n.side || n.side === f.side));
}

function renderCards(mapId) {
  const notes = S.mapData?.notes ?? [];
  const shown = filteredNotes(notes, S.filters[mapId] ?? {});
  const box = $("#cards");
  if (!box) return;
  box.innerHTML = shown.map((n) => card(n)).join("");
  $("#nomatch").hidden = shown.length > 0 || notes.length === 0;
}

function emptyMap(m, ro) {
  const id = enc(m.id);
  return `<div class="empty box dashed"><h3>${t("Aucune note sur {map} pour l'instant", { map: esc(m.title) })}</h3>
    <p>${t("Partez d'un modèle (setup, exécution, retake…) ou reprenez un texte existant : copiez-le depuis Discord, Word ou Google Docs, puis faites <kbd>Ctrl</kbd> + <kbd>V</kbd> sur cette page.")}</p>
    ${
      ro
        ? ""
        : `<div class="actions">${linkBtn(`#/new/${id}`, t("Écrire un protocole"), "", "primary")}${linkBtn(`#/new/${id}?paste=1`, t("Coller un texte"))}${btn(
            t("Importer des fichiers .md"),
            'data-act="import"',
          )}</div>`
    }</div>`;
}

function overviewPanel(m, ro, { compact = false } = {}) {
  const href = `#/edit-map/${enc(m.id)}`;
  if (m.overview.trim()) {
    return `<section class="panel box" aria-labelledby="h-overview"><h2 class="box-title" id="h-overview">${t("Présentation")}</h2>${
      ro || compact ? "" : `<span class="box-tag">${linkBtn(href, t("Modifier"), `aria-label="${esc(t("Modifier la présentation"))}"`, "small")}</span>`
    }<div class="docs" id="overview">${m.overviewHtml}</div></section>`;
  }
  return ro || compact
    ? ""
    : `<p class="empty-line box dashed">${t("Pas encore de présentation : défauts, callouts, plan de jeu, compositions.")} <a class="lnk" href="${esc(href)}">${t("L'écrire")}</a></p>`;
}

async function viewMap(r) {
  const { data } = await cached(`/api/maps/${enc(r.map)}`);
  if (stale(r)) return;
  S.mapData = data;
  S.sig = JSON.stringify(data);
  const m = data.map;
  renderChrome();
  const notes = data.notes;
  const ro = readOnly();
  const id = enc(m.id);
  const f = (S.filters[m.id] ??= { kind: "", side: "" });
  const counts = {};
  for (const n of notes) counts[n.kind] = (counts[n.kind] || 0) + 1;
  const tabs = [["", t("Tout"), notes.length], ...kinds().filter((k) => counts[k.id]).map((k) => [k.id, k.plural, counts[k.id]])];
  for (const k of Object.keys(counts)) if (!kindOf(k)) tabs.push([k, cap(k), counts[k]]);
  if (!tabs.some(([v]) => v === f.kind)) f.kind = "";
  const more = [
    !ro && { title: t("Coller un protocole"), hint: t("Une note à partir d'un texte copié"), href: `#/new/${id}?paste=1` },
    !ro && { title: t("Importer des fichiers .md"), hint: t("Obsidian, un autre carnet… (ou glissez-les ici)"), attrs: 'data-act="import"' },
    { title: t("Télécharger la map (.md)"), hint: t("Présentation et notes en un seul fichier"), href: `/api/export/${id}.md`, download: true },
    !ro &&
      !m.team && {
        title: m.pool === true ? t("Retirer du pool") : t("Mettre dans le pool"),
        hint: t("Les maps du pool passent en tête de la liste"),
        attrs: 'data-act="pool"',
      },
  ];
  const stats = [
    tp(notes.length, "{n} note", "{n} notes"),
    tp(m.pinned, "{n} épinglée", "{n} épinglées"),
    m.pool === true ? t("pool actif") : "",
    m.updated ? t("mise à jour {ago}", { ago: ago(m.updated) }) : "",
  ];
  page.innerHTML = `
    ${crumbs([m.title])}
    <header class="page-head">
      <h1 class="page-title">${esc(m.title)}</h1>
      <p class="meta">${stats
        .filter(Boolean)
        .map((x) => `<span>${esc(x)}</span>`)
        .join("")}</p>
      <div class="actions">
        ${linkBtn(`#/sheet/${id}`, t("Fiche match"), `title="${esc(t("Fiche match (touche F)"))}"`)}
        ${copyControl(t("Copier la map"))}
        ${ro ? "" : linkBtn(`#/edit-map/${id}`, t("Présentation"), `title="${esc(t("Modifier la présentation (touche E)"))}"`)}
        ${menuButton("map-more", t("Plus d'actions"), `${t("Plus")}${DOWN}`)}
      </div>
      ${menu("map-more", t("Plus d'actions"), more)}
    </header>
    ${overviewPanel(m, ro)}
    <section aria-labelledby="h-notes">
      ${sh("h-notes", t("Notes"), notes.length, ro ? "" : linkBtn(`#/new/${id}`, t("Nouvelle note"), "", "small"))}
      ${notes.length ? `<div class="filters">${radioRow("f-kind", t("Type"), tabs, f.kind)}${radioRow("f-side", t("Côté"), sidesFilter(), f.side)}</div>` : ""}
      <div class="cards" id="cards"></div>
      <p class="empty-line box dashed" id="nomatch" hidden>${t("Aucune note pour ce filtre.")} <button class="lnk" type="button" data-act="clear-filters">${t("Tout afficher")}</button></p>
      ${notes.length ? "" : emptyMap(m, ro)}
    </section>`;
  renderCards(m.id);
  enhanceDocs($("#overview"));
  prefetchShare(`/api/share/${id}`);
}

async function viewNote(r) {
  const [{ data: n }, md] = await Promise.all([cached(`/api/notes/${enc(r.map)}/${enc(r.id)}`), cached(`/api/maps/${enc(r.map)}`).catch(() => null)]);
  if (stale(r)) return;
  S.mapData = md?.data ?? null;
  S.note = n;
  renderChrome();
  const ro = readOnly();
  const siblings = S.mapData?.notes ?? [];
  const i = siblings.findIndex((x) => x.id === n.id);
  const prev = i > 0 ? siblings[i - 1] : null;
  const next = i >= 0 && i < siblings.length - 1 ? siblings[i + 1] : null;
  const more = [
    { title: t("Télécharger (.md)"), hint: t("La note en fichier Markdown, en-tête compris"), attrs: 'data-act="download-note"' },
    !ro && "sep",
    !ro && { title: t("Supprimer"), hint: t("Vers la corbeille, annulable"), attrs: 'data-act="delete"', cls: "danger" },
  ];
  const tags = n.tags.map((tag) => `<a href="#/search/${esc(enc("#" + tag))}">#${esc(tag)}</a>`).join(" ");
  page.innerHTML = `
    ${crumbs([mapTitle(n.map), `#/map/${enc(n.map)}`], [n.title])}
    <header class="page-head">
      <h1 class="note-title">${esc(n.title)}</h1>
      <p class="meta">${facts(n, { pin: ro })}${tags ? `<span class="tags">${tags}</span>` : ""}<span>${esc(t("modifiée {ago}", { ago: ago(n.updated) }))}</span></p>
      <div class="actions" id="tools">
        ${ro ? "" : linkBtn(`#/edit/${enc(n.map)}/${enc(n.id)}`, t("Modifier"), `title="${esc(t("Modifier (touche E)"))}"`, "primary")}
        ${copyControl(t("Copier"))}
        ${ro ? "" : toggleBtn(t("Épinglée"), n.pin, `data-act="pin" title="${esc(t("Les notes épinglées vont sur l'accueil et dans la fiche match"))}"`)}
        ${menuButton("note-more", t("Plus d'actions"), `${t("Plus")}${DOWN}`)}
      </div>
      ${menu("note-more", t("Plus d'actions"), more)}
    </header>
    <article class="docs note-body" id="body">${n.html || `<p class="muted">${t("Note vide.")}</p>`}</article>
    <nav class="pager" aria-label="${esc(t("Notes voisines"))}">${
      prev ? `<a href="${esc(noteHref(prev))}" rel="prev"><small>&lt; ${t("précédente")}</small><span>${esc(prev.title)}</span></a>` : "<span></span>"
    }${next ? `<a class="next" href="${esc(noteHref(next))}" rel="next"><small>${t("suivante")} &gt;</small><span>${esc(next.title)}</span></a>` : ""}</nav>`;
  enhanceDocs($("#body"));
  prefetchShare(`/api/share/${enc(n.map)}/${enc(n.id)}`);
}

const templateHint = (id) => {
  const heads = [...(kindOf(id)?.template || "").matchAll(/^##\s+(.+)$/gm)].map((m) => m[1]);
  return heads.length ? t("Modèle prêt à remplir : {sections}.", { sections: heads.join(", ") }) : t("Note libre, sans modèle.");
};

function viewNew(r) {
  S.mapData = null;
  renderChrome();
  if (readOnly()) return viewReadOnly();
  const maps = mapsList();
  const map = maps.some((m) => m.id === r.map) ? r.map : S.lastMap && maps.some((m) => m.id === S.lastMap) ? S.lastMap : maps[0]?.id;
  const kindItems = kinds().map((k) => [k.id, k.label]);
  const pasted = S.pendingPaste;
  S.pendingPaste = null;
  page.innerHTML = `
    ${crumbs(...(map ? [[mapTitle(map), `#/map/${enc(map)}`]] : []), [t("Nouvelle note")])}
    <div id="n-msgs"></div>
    <form class="formbox box round" id="newform" autocomplete="off" novalidate aria-labelledby="n-h">
      <h1 class="box-title" id="n-h">${t("Nouvelle note")}</h1>
      ${row(
        t("Titre"),
        "n-title",
        field(`<input id="n-title" name="title" maxlength="200" placeholder="${esc(t("Ex. Retake B à trois, exécution rapide A…"))}" aria-describedby="n-title-help">`),
      )}
      <p class="row-help" id="n-title-help">${t("Facultatif si vous collez un texte : son premier titre sera repris.")}</p>
      ${row(t("Map"), "n-map", field(`<select id="n-map" name="map">${options(maps.map((m) => [m.id, m.title]), map)}</select>`, true))}
      ${radioRow("side", t("Côté"), sidesForm(), "")}
      ${radioRow("kind", t("Type"), kindItems, kindItems[0]?.[0], "kind-help")}
      <p class="row-help" id="kind-help">${esc(templateHint(kindItems[0]?.[0]))}</p>
      <div class="row top"><label for="n-body">${t("Contenu")}</label><textarea class="area" id="n-body" name="body" rows="9" spellcheck="true" placeholder="${esc(
        t("Collez ici un protocole existant : Discord, Word, Google Docs, Notion…"),
      )}&#10;${esc(t("Laissez vide pour partir du modèle du type."))}" aria-describedby="n-body-help"></textarea></div>
      <p class="row-help" id="n-body-help">${t("Facultatif. Titres, listes, gras et tableaux copiés depuis Word ou Docs sont convertis ; vous pouvez aussi déposer des fichiers .md sur cette page.")}</p>
      <p class="form-error" id="n-err" role="alert" hidden></p>
      <div class="form-actions">${btn(t("Importer des fichiers .md"), 'data-act="import"')}<button class="btn primary" type="submit"><span>${t("Créer la note")}</span></button></div>
      <span class="box-hint">${t("tab champ suivant · entrée créer")}</span>
    </form>`;
  const body = $("#n-body");
  const draft = pasted ? null : readDraft(NEW_DRAFT);
  if (pasted) {
    body.value = pasted;
    fillTitleFrom(pasted);
    storeNewDraft();
  } else if (draft && (draft.body.trim() || draft.title?.trim())) {
    // Un texte saisi puis quitté (autre page, thème, fermeture) : il revient.
    body.value = draft.body;
    $("#n-title").value = draft.title || "";
    $("#n-msgs").innerHTML = notice(
      "",
      t("Brouillon repris"),
      esc(t("Le texte saisi {ago} est revenu.", { ago: ago(draft.at) })),
      btn(t("Effacer"), 'data-act="new-draft-drop"', "small"),
    );
  }
  updateKindHint();
  (r.paste || pasted ? body : $("#n-title")).focus();
}

/** Le titre d'un texte collé, proposé si le champ est encore vide. */
function fillTitleFrom(md) {
  const input = $("#n-title");
  if (input && !input.value.trim()) input.value = guessTitle(md).title;
}

// Le brouillon d'une nouvelle note, sur ce poste : un protocole collé ne se perd pas.
const NEW_DRAFT = "protocol-draft:new";
function storeNewDraft() {
  const title = $("#n-title")?.value ?? "";
  const body = $("#n-body")?.value ?? "";
  try {
    if (title.trim() || body.trim()) localStorage.setItem(NEW_DRAFT, JSON.stringify({ title, body, at: Date.now() }));
    else localStorage.removeItem(NEW_DRAFT);
  } catch {}
}
function clearNewDraft() {
  try {
    localStorage.removeItem(NEW_DRAFT);
  } catch {}
}

function updateKindHint() {
  const hint = $("#kind-help");
  if (!hint) return;
  const body = $("#n-body")?.value.trim();
  const kind = $('#newform input[name="kind"]:checked')?.value;
  hint.textContent = body ? t("Le texte collé remplace le modèle du type.") : templateHint(kind);
}

function viewNewMap() {
  S.mapData = null;
  renderChrome();
  if (readOnly()) return viewReadOnly();
  page.innerHTML = `
    ${crumbs([t("Nouvelle map")])}
    <form class="formbox box round" id="mapform" autocomplete="off" novalidate aria-labelledby="m-h">
      <h1 class="box-title" id="m-h">${t("Nouvelle map")}</h1>
      ${row(t("Nom"), "m-title", field(`<input id="m-title" placeholder="${esc(t("Ex. District"))}" maxlength="60" required aria-describedby="m-help">`))}
      <p class="row-help" id="m-help">${t("Une map est un dossier du carnet : elle apparaît dans la liste dès sa création.")}</p>
      <p class="form-error" id="m-err" role="alert" hidden></p>
      <div class="form-actions"><button class="btn primary" type="submit"><span>${t("Créer la map")}</span></button></div>
      <span class="box-hint">${t("entrée créer")}</span>
    </form>`;
  $("#m-title").focus();
}

// Les blocs prêts : leur texte est un gabarit Markdown, écrit dans la langue de l'interface.
const BLOCKS = [
  {
    id: "roles",
    title: t("Tableau des rôles"),
    hint: t("Joueur, agent, rôle, utilitaire"),
    text: `| ${t("Joueur")} | ${t("Agent")} | ${t("Rôle")} | ${t("Utilitaire")} |\n|---|---|---|---|\n|  |  |  |  |\n|  |  |  |  |\n`,
  },
  {
    id: "timing",
    title: t("Timings"),
    hint: t("Départ, exécution, post-plant"),
    text: `## ${t("Timing")}\n- ${t("Départ :")}\n- ${t("Exécution à :")}\n- ${t("Post-plant :")}\n`,
  },
  { id: "utils", title: t("Ordre des utilitaires"), hint: t("Des étapes numérotées"), text: `## ${t("Ordre des utilitaires")}\n1. \n2. \n3. \n` },
  { id: "checklist", title: t("Checklist"), hint: t("Des cases à cocher"), text: "- [ ] \n- [ ] \n- [ ] \n" },
  { id: "calls", title: t("Calls"), hint: t("Ce que l'IGL annonce"), text: `## ${t("Calls")}\n- \n` },
  { id: "warning", title: t("Encadré « attention »"), hint: t("Un point à ne pas rater"), text: "> [!WARNING]\n> \n" },
  { id: "tip", title: t("Encadré « astuce »"), hint: t("Un conseil"), text: "> [!TIP]\n> \n" },
  { id: "rule", title: t("Séparateur"), hint: t("Une ligne entre deux parties"), text: "---\n" },
  { id: "template", title: t("Modèle du type"), hint: t("Les sections de départ du type choisi"), text: null },
];

function toolbar() {
  // Gras et italique s'écrivent G et I (B et I en anglais) ; leur nom complet reste lu et affiché au survol.
  const tool = (cmd, label, title, name = "") =>
    `<button class="btn" type="button" data-md="${cmd}" title="${esc(title)}"${name ? ` aria-label="${esc(name)}"` : ""}><span>${label}</span></button>`;
  const sep = '<span class="sep" aria-hidden="true">│</span>';
  return `<div class="toolbar box" role="toolbar" aria-label="${esc(t("Mise en forme"))}">
      ${tool("h2", t("Titre"), t("Titre de section"))}${tool("bold", `<b>${t("G")}</b>`, t("Gras (Ctrl+B)"), t("Gras"))}${tool(
        "italic",
        `<i>${t("I")}</i>`,
        t("Italique (Ctrl+I)"),
        t("Italique"),
      )}${sep}
      ${tool("list", t("Liste"), t("Liste à puces"))}${tool("ol", t("Étapes"), t("Étapes (liste numérotée)"))}${tool("check", t("Cases"), t("Cases à cocher"))}${sep}
      ${tool("table", t("Tableau"), t("Tableau des rôles"))}${tool("callout", t("Encadré"), t("Encadré « attention »"))}${tool(
        "link",
        t("Lien"),
        t("Lien vers une autre note"),
      )}${tool("image", t("Image"), t("Image (ou Ctrl+V d'une capture)"))}
      <span class="end">
        ${menuButton("blocks-menu", t("Insérer un bloc prêt"), `${t("Insérer")}${DOWN}`)}
        ${menuButton("cheat-menu", t("Aide-mémoire Markdown"), t("Aide"), "dialog")}
      </span>
      <span class="box-hint">${t("ctrl+b gras · ctrl+i italique · ctrl+entrée terminer")}</span>
    </div>
    ${menu(
      "blocks-menu",
      t("Insérer un bloc"),
      BLOCKS.map((b) => ({ title: b.title, hint: b.hint, attrs: `data-block="${b.id}"` })),
    )}
    <div class="menu" id="cheat-menu" popover role="dialog" aria-label="${esc(t("Aide-mémoire Markdown"))}"><p class="menu-title">${t("Aide-mémoire")}</p><div class="cheats">
      <code>## ${t("Rôles")}</code><span>${t("Titre de section")}</span>
      <code>**important**</code><span>${t("Gras")}</span>
      <code>*${t("nuance")}*</code><span>${t("Italique")}</span>
      <code>- ${t("fumée heaven")}</code><span>${t("Liste à puces")}</span>
      <code>1. flash</code><span>${t("Étapes numérotées")}</span>
      <code>- [ ] veto</code><span>${t("Case à cocher")}</span>
      <code>| A | B |</code><span>${t("Tableau")}</span>
      <code>[[retake-b]]</code><span>${t("Lien vers une note")}</span>
      <code>&gt; [!WARNING]</code><span>${t("Encadré « attention »")}</span>
      <code>---</code><span>${t("Séparateur")}</span>
    </div></div>`;
}

const FILE_INPUT = '<input type="file" id="f-file" accept="image/png,image/jpeg,image/gif,image/webp,image/avif,image/bmp" multiple hidden>';
const EDIT_HINT = `<p class="editor-hint">${t(
  "Enregistrement automatique. Collez une capture (<kbd>Ctrl</kbd> + <kbd>V</kbd>) pour ajouter une image ; un texte copié depuis Word ou Docs garde sa mise en forme. <kbd>Ctrl</kbd> + <kbd>Entrée</kbd> pour terminer.",
)}</p>`;
/** L'en-tête de l'éditeur : le titre en grand, l'état de l'enregistrement, [ Terminer ]. */
const editorHead = (title, label, max, doneHref) => `
    <div class="editor-head">
      <div class="title-field"><label class="sr" for="f-title">${esc(label)}</label><input class="title-input" id="f-title" value="${esc(title)}" placeholder="${esc(label)}" maxlength="${max}"></div>
      <div class="actions">
        <span class="status ok" id="status">${t("Enregistré")}</span>
        <a class="btn primary" href="${esc(doneHref)}" id="done" title="${esc(t("Terminer (Ctrl+Entrée)"))}"><span>${t("Terminer")}</span></a>
      </div>
    </div>`;
const editorPanes = (what) => `
    <div class="editor" id="editor">
      <div class="pane box"><label class="box-title" for="f-body">${esc(what)}</label><span class="box-tag">${toggleBtn(
        t("Aperçu"),
        true,
        `data-act="preview" title="${esc(t("Afficher ou masquer l'aperçu"))}"`,
      )}</span><textarea id="f-body" spellcheck="true"></textarea></div>
      <div class="pane box" id="previewPane"><span class="box-title" id="pv-h">${t("Aperçu")}</span><div class="docs preview" id="preview" role="region" aria-labelledby="pv-h"></div></div>
    </div>`;

async function viewEdit(r) {
  if (readOnly()) return viewReadOnly();
  const [{ data: n }, md] = await Promise.all([cached(`/api/notes/${enc(r.map)}/${enc(r.id)}`, true), cached(`/api/maps/${enc(r.map)}`).catch(() => null)]);
  if (stale(r)) return;
  S.mapData = md?.data ?? null;
  S.lastMap = n.map;
  S.note = n;
  renderChrome();
  const kindItems = kinds().map((k) => [k.id, k.label]);
  if (!kindOf(n.kind)) kindItems.push([n.kind, cap(n.kind)]);
  page.innerHTML = `
    ${crumbs([mapTitle(n.map), `#/map/${enc(n.map)}`], [n.title, noteHref(n)], [t("Modifier")])}
    <h1 class="sr">${t("Modifier : {title}", { title: esc(n.title) })}</h1>
    <div id="msgs"></div>
    ${editorHead(n.title, t("Titre de la note"), 200, noteHref(n))}
    <div class="meta-form box" role="group" aria-labelledby="meta-h">
      <h2 class="box-title" id="meta-h">${t("Propriétés")}</h2>
      <span class="box-tag">${toggleBtn(t("Épinglée"), n.pin, `id="f-pin" title="${esc(t("Les notes épinglées vont sur l'accueil et dans la fiche match"))}"`)}</span>
      ${row(t("Type"), "f-kind", field(`<select id="f-kind">${options(kindItems, n.kind)}</select>`, true))}
      ${row(t("Map"), "f-map", field(`<select id="f-map">${options(mapsList().map((m) => [m.id, m.title]), n.map)}</select>`, true))}
      ${radioRow("e-side", t("Côté"), sidesForm(), n.side)}
      ${row(
        t("Tags"),
        "f-tags",
        field(`<input id="f-tags" value="${esc(n.tags.join(", "))}" placeholder="${esc(t("retake, b, utilitaire"))}" aria-describedby="f-tags-help">`),
      )}
      <span class="box-hint" id="f-tags-help">${t("tags séparés par des virgules")}</span>
    </div>
    ${toolbar()}
    ${editorPanes(t("Contenu (Markdown)"))}
    ${EDIT_HINT}
    ${FILE_INPUT}`;
  $("#f-body").value = n.body; // par .value : le parseur HTML mangerait un saut de ligne initial
  S.editor = new Editor({ mode: "note", target: n });
  $("#f-body").focus();
}

async function viewEditMap(r) {
  if (readOnly()) return viewReadOnly();
  const { data } = await cached(`/api/maps/${enc(r.map)}`, true);
  if (stale(r)) return;
  S.mapData = data;
  renderChrome();
  const m = data.map;
  page.innerHTML = `
    ${crumbs([m.title, `#/map/${enc(m.id)}`], [t("Présentation")])}
    <h1 class="sr">${t("Présentation : {map}", { map: esc(m.title) })}</h1>
    <div id="msgs"></div>
    ${editorHead(m.title, t("Nom de la map"), 60, `#/map/${enc(m.id)}`)}
    <div class="meta-form stack box" role="group" aria-labelledby="meta-h">
      <h2 class="box-title" id="meta-h">${t("Propriétés")}</h2>
      ${
        m.team
          ? ""
          : `<span class="box-tag">${toggleBtn(t("Dans le pool"), m.pool === true, `id="f-pool" title="${esc(t("Les maps du pool passent en tête de la liste"))}"`)}</span>`
      }
      <p>${t("La présentation s'affiche en tête de la map et de sa fiche match : défauts, callouts, plan de jeu, compositions.")}</p>
      ${!m.team && m.count === 0 ? `<p class="actions" id="del">${btn(t("Supprimer la map"), 'data-act="delete-map"', "danger")}</p>` : ""}
    </div>
    ${toolbar()}
    ${editorPanes(t("Présentation (Markdown)"))}
    ${EDIT_HINT}
    ${FILE_INPUT}`;
  $("#f-body").value = m.overview;
  S.editor = new Editor({ mode: "map", target: m });
  $("#f-body").focus();
}

async function viewSheet(r) {
  const { data } = await cached(`/api/maps/${enc(r.map)}`);
  if (stale(r)) return;
  const pinned = data.notes.filter((n) => n.pin);
  const others = data.notes.filter((n) => !n.pin);
  const full = await Promise.all(pinned.map((n) => cached(`/api/notes/${enc(n.map)}/${enc(n.id)}`).then((x) => x.data)));
  if (stale(r)) return;
  S.mapData = data;
  S.sig = JSON.stringify(data);
  renderChrome();
  const m = data.map;
  const sheetCard = (n) => {
    const meta = facts(n, { kind: false });
    return (
      `<article class="sheet-card box"><span class="box-title">${esc(kindLabel(n.kind))}</span><h2><a href="${esc(noteHref(n))}">${esc(n.title)}</a></h2>` +
      `${meta ? `<p class="meta">${meta}</p>` : ""}<div class="docs">${n.html}</div></article>`
    );
  };
  page.innerHTML = `
    ${crumbs([m.title, `#/map/${enc(m.id)}`], [t("Fiche match")])}
    <header class="page-head">
      <h1 class="page-title">${t("Fiche match : {map}", { map: esc(m.title) })}</h1>
      <p class="meta"><span>${tp(full.length, "{n} note épinglée", "{n} notes épinglées")}</span><span>${t("imprimable sur une page recto verso")}</span></p>
      <div class="actions">
        ${btn(t("Imprimer"), 'data-act="print"')}
        ${copyControl(t("Copier la fiche"))}
        ${linkBtn(`#/map/${enc(m.id)}`, t("Retour à la map"))}
      </div>
    </header>
    ${m.overview.trim() ? overviewPanel(m, true, { compact: true }) : ""}
    ${
      full.length
        ? `<div class="sheet">${full.map(sheetCard).join("")}</div>`
        : `<p class="empty-line box dashed">${t(
            "Aucune note épinglée sur {map}. Épinglez les protocoles à garder sous les yeux pendant un temps mort : ils s'afficheront ici en entier.",
            { map: esc(m.title) },
          )}</p>`
    }
    ${
      others.length
        ? `<section class="noprint" aria-labelledby="h-others">${sh("h-others", t("Autres notes"), others.length)}<ul class="leaders">${others
            .map(
              (n) =>
                `<li><a href="${esc(noteHref(n))}"><span class="t-title">${esc(n.title)}</span><span class="dots" aria-hidden="true"></span><span class="m">${esc(factsText(n))}</span></a></li>`,
            )
            .join("")}</ul></section>`
        : ""
    }`;
  for (const d of $$(".docs", page)) enhanceDocs(d);
  prefetchShare(`/api/share/${enc(m.id)}?scope=pinned`);
}

let searchSeq = 0;
async function runSearch(q) {
  const seq = ++searchSeq;
  const sum = $("#sum");
  const hits = $("#hits");
  if (!sum || !hits) return;
  if (!q.trim()) {
    S.hits = [];
    sum.innerHTML = t(
      'Tapez un mot, ou un filtre : <code>map:ascent</code> <code>type:retake</code> <code>side:def</code> <code>#tag</code> <code>"expression exacte"</code>',
    );
    hits.innerHTML = "";
    find.setAttribute("aria-expanded", "false");
    find.removeAttribute("aria-activedescendant");
    return;
  }
  const res = await api("GET", `/api/search?q=${enc(q)}&limit=60`);
  if (seq !== searchSeq || !hits.isConnected) return;
  S.hits = res.hits;
  S.hitIndex = 0;
  sum.innerHTML = res.total
    ? `${tp(res.total, "{n} résultat pour « {q} »", "{n} résultats pour « {q} »", { q: esc(q) })}${
        res.total > res.hits.length ? ` ${t("(les {n} meilleurs)", { n: res.hits.length })}` : ""
      }. ${t("<kbd>↑</kbd> <kbd>↓</kbd> puis <kbd>Entrée</kbd> pour ouvrir.")}`
    : t("Rien pour « {q} ». Un autre mot ? Un filtre en trop ?", { q: esc(q) });
  hits.innerHTML = res.hits
    .map(
      (h, i) =>
        `<a class="hit${i === 0 ? " sel" : ""}" id="hit-${i}" role="option" aria-selected="${i === 0}" href="${esc(noteHref(h))}">` +
        `<span class="hit-top"><span class="hit-title">${esc(h.title)}</span><span class="meta">${facts(h, { map: true })}</span></span><span class="hit-snip">${h.snippet}</span></a>`,
    )
    .join("");
  find.setAttribute("aria-expanded", String(res.hits.length > 0));
  if (res.hits.length) find.setAttribute("aria-activedescendant", "hit-0");
  else find.removeAttribute("aria-activedescendant");
}

function moveHit(d) {
  const items = $$(".hit", page);
  if (!items.length) return;
  S.hitIndex = (S.hitIndex + d + items.length) % items.length;
  items.forEach((el, i) => {
    el.classList.toggle("sel", i === S.hitIndex);
    el.setAttribute("aria-selected", String(i === S.hitIndex));
  });
  items[S.hitIndex].scrollIntoView({ block: "nearest" });
  find.setAttribute("aria-activedescendant", items[S.hitIndex].id);
}

async function viewSearch(r) {
  S.mapData = null;
  renderChrome();
  // Si l'on tape encore dans le champ, c'est lui qui fait foi.
  const q = document.activeElement === find ? find.value : r.q;
  if (find.value !== q) find.value = q;
  r.q = q;
  page.innerHTML = `
    ${crumbs([t("Recherche")])}
    <div class="page-head"><h1 class="page-title">${t("Recherche")}</h1></div>
    <p class="search-sum" id="sum" aria-live="polite"></p>
    <div class="hits" id="hits" role="listbox" aria-label="${esc(t("Résultats"))}"></div>`;
  await runSearch(q);
}

async function viewTrash(r) {
  S.mapData = null;
  renderChrome();
  const { items } = await api("GET", "/api/trash");
  if (stale(r)) return;
  page.innerHTML = `
    ${crumbs([t("Corbeille")])}
    <div class="page-head"><h1 class="page-title">${t("Corbeille")}</h1><p class="lede">${t(
      "Les notes supprimées attendent ici, dans le dossier <code>.trash</code> du carnet. Rien ne s'efface tout seul.",
    )}</p></div>
    ${
      items.length
        ? `<div class="trash"><p class="tree-root">.trash/</p><ul>${items
            .map(
              (it, i) =>
                `<li class="t${i === items.length - 1 ? " last" : ""}"><span class="title">${esc(it.title)}</span><span class="dots" aria-hidden="true"></span>` +
                `<span class="m">${esc(mapTitle(it.map))}${it.deleted ? ` · ${esc(t("supprimée {ago}", { ago: ago(it.deleted) }))}` : ""}</span>${btn(
                  t("Restaurer"),
                  `data-restore="${esc(it.path)}"`,
                  "small",
                )}</li>`,
            )
            .join("")}</ul><p class="foot-line">${tp(items.length, "{n} fichier", "{n} fichiers")}</p></div>`
        : `<p class="empty-line box dashed">${t("La corbeille est vide.")}</p>`
    }`;
}

/** La configuration MCP de ce carnet, avec les vrais chemins de ce poste ; `write` ajoute --write. */
function mcpConfig(st, write) {
  const exe = st.exe || t("C:/chemin/protocol.exe");
  const args = ["mcp", ...(st.carnet ? ["--carnet", st.carnet] : []), ...(write ? ["--write"] : [])];
  const desktop = JSON.stringify({ mcpServers: { protocol: { command: exe, args } } }, null, 2);
  const quote = (s) => (/[\s"]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s);
  const code = `claude mcp add --scope user protocol -- ${[exe, ...args].map(quote).join(" ")}`;
  return { desktop, code };
}

/** L'aide « Assistant IA (MCP) » : ce que l'assistant peut faire, et sa configuration à copier. */
function mcpSection(st) {
  const m = mcpConfig(st, S.mcpWrite);
  return `<section class="box" aria-labelledby="h-mcp"><h2 class="box-title" id="h-mcp">${t("Assistant IA (MCP)")}</h2>
        <p>${t(
          "Claude Desktop, Claude Code ou tout client MCP peuvent travailler avec ce carnet : chercher, lire une note, sortir la fiche match, et lancer des consignes prêtes (résumer un protocole, en chercher les failles, préparer ou débriefer un match, ranger un texte collé). protocol ne contacte aucune IA : c'est l'assistant installé sur ce poste qui lance <code>protocol mcp</code> et lui parle directement, sans réseau ni port ouvert.",
        )}</p>
        <p>${t(
          "<strong>Lecture seule</strong> par défaut. <strong>Avec écriture</strong>, l'assistant peut aussi créer, compléter, épingler, déplacer ou retirer des notes, même pendant que cette fenêtre est ouverte. Rien n'est irréversible : une note retirée va à la corbeille, une note réécrite y laisse sa version d'avant, et la corbeille ne se vide qu'ici.",
        )}</p>
        ${radioRow("mcp-mode", t("Accès"), [["read", t("Lecture seule")], ["write", t("Avec écriture")]], S.mcpWrite ? "write" : "read")}
        <p>${t(
          "Claude Desktop : ajoutez ceci à son fichier de configuration (<code>claude_desktop_config.json</code>, par Settings › Developer › Edit Config), puis redémarrez-le.",
        )}</p>
        <pre class="code" id="mcp-desktop">${esc(m.desktop)}</pre>
        <p class="actions">${btn(t("Copier pour Claude Desktop"), 'data-act="copy-mcp" data-from="mcp-desktop"', "small")}</p>
        <p>${t("Claude Code : lancez cette commande une fois.")}</p>
        <pre class="code" id="mcp-code">${esc(m.code)}</pre>
        <p class="actions">${btn(t("Copier pour Claude Code"), 'data-act="copy-mcp" data-from="mcp-code"', "small")}</p>
        <p class="muted">${t("Sur une clé USB, la lettre du lecteur change parfois d'un poste à l'autre : revenez alors copier la configuration.")}</p>
      </section>`;
}

function viewHelp() {
  S.mapData = null;
  renderChrome();
  const st = S.state;
  const origin = location.origin;
  const keys = [
    [["/"], t("Rechercher, puis ↑ ↓ et Entrée")],
    [["N"], t("Nouvelle note")],
    [["E"], t("Modifier la note ou la présentation affichée")],
    [["F"], t("Fiche match de la map en cours")],
    [["C"], t("Copier la note, la map ou la fiche affichée")],
    [["[", "]"], t("Map précédente, map suivante")],
    [["Ctrl", "V"], t("Sur une page : crée une note à partir du texte copié")],
    [["Ctrl", "S"], t("Enregistrer (c'est aussi automatique)")],
    [["Ctrl", t("Entrée")], t("Terminer l'édition")],
    [[t("Échap")], t("Revenir en arrière")],
    [["?"], t("Cette page")],
  ];
  // La langue de l'interface : réglage du carnet (automatique : celle du système).
  const langRow = readOnly()
    ? ""
    : `<section class="box" aria-labelledby="h-lang"><h2 class="box-title" id="h-lang">Langue / Language</h2>${radioRow(
        "ui-lang",
        t("Interface"),
        [
          ["auto", t("Auto")],
          ["fr", "Français"],
          ["en", "English"],
        ],
        st.langSetting === "fr" || st.langSetting === "en" ? st.langSetting : "auto",
      )}</section>`;
  page.innerHTML = `
    ${crumbs([t("Aide")])}
    <div class="page-head"><h1 class="page-title">${t("Aide et raccourcis")}</h1></div>
    <div class="help-page">
      <section class="box" aria-labelledby="h-keys"><h2 class="box-title" id="h-keys">${t("Raccourcis clavier")}</h2><dl class="keys">${keys
        .map(([k, what]) => `<dt>${k.map((x) => `<kbd>${esc(x)}</kbd>`).join(" ")}</dt><dd>${esc(what)}</dd>`)
        .join("")}</dl></section>
      ${langRow}
      <section class="box" aria-labelledby="h-copy"><h2 class="box-title" id="h-copy">${t("Copier, coller, importer")}</h2>
        <p>${t(
          "<strong>Copier</strong> une note, une map ou une fiche match met deux versions dans le presse-papiers : Discord reçoit du Markdown (les tableaux deviennent un bloc aligné), Word, Google Docs et Notion reçoivent le texte mis en forme. Le bouton <code>[&nbsp;v&nbsp;]</code> à côté propose aussi le texte simple et le fichier Markdown complet.",
        )}</p>
        <p>${t(
          "<strong>Coller</strong> un protocole existant suffit à créer une note : <kbd>Ctrl</kbd> + <kbd>V</kbd> sur une page du carnet, ou dans le champ Contenu d'une nouvelle note. La mise en forme de Word ou Docs est convertie, et le premier titre devient celui de la note.",
        )}</p>
        <p>${t(
          "<strong>Importer</strong> des fichiers <code>.md</code> (Obsidian, un autre carnet) : déposez-les sur une map, ou passez par le menu <code>[&nbsp;Plus&nbsp;v&nbsp;]</code> de la map. Leur en-tête est conservé ; une note existante n'est jamais écrasée.",
        )}</p>
      </section>
      <section class="box" aria-labelledby="h-search"><h2 class="box-title" id="h-search">${t("Recherche")}</h2>
        <p>${t(
          'Tous les mots doivent apparaître, accents et majuscules ignorés. Filtres : <code>map:ascent</code>, <code>type:retake</code>, <code>side:def</code>, <code>#tag</code>, <code>"expression exacte"</code>.',
        )}</p>
      </section>
      <section class="box" aria-labelledby="h-files"><h2 class="box-title" id="h-files">${t("Votre carnet")}</h2>
        <p>${t(
          "Tout le carnet tient dans un seul fichier, <code>protocol.carnet</code>, rangé à côté du programme : maps, notes, images collées, corbeille et réglages. Pour l'emporter ou le sauvegarder, copiez-le avec <code>protocol.exe</code> (clé USB, dossier partagé, OneDrive).",
        )}</p>
        <p>${t("Rien n'est écrit ailleurs sur le poste : la fenêtre utilise un profil de navigateur temporaire, rangé à côté du carnet et effacé à la fermeture.")}</p>
        <p>${t(
          "Pour travailler avec Obsidian, VS Code ou un autre outil, exportez le carnet en fichiers Markdown (une map = un dossier, une note = un fichier <code>.md</code>) ; dans l'autre sens, déposez des fichiers <code>.md</code> sur une map.",
        )}</p>
        ${
          st.local && st.carnet
            ? `<p>${t("Carnet : {path} ({size})", { path: `<code>${esc(st.carnet)}</code>`, size: esc(fmtSize(st.carnetSize)) })}</p><p class="actions">${btn(
                t("Afficher le carnet"),
                'data-act="reveal-root"',
                "small",
              )}${linkBtn("/api/export.zip", t("Exporter en fichiers Markdown (.zip)"), "download", "small")}</p>`
            : ""
        }
      </section>
      <section class="box" aria-labelledby="h-tools"><h2 class="box-title" id="h-tools">${t("Avec d'autres outils")}</h2>
        <p>${t("Tout passe par une petite API JSON locale :")}</p>
        <pre class="code">GET  ${esc(origin)}/api/state                  ${t("maps, notes épinglées, récentes")}
GET  ${esc(origin)}/api/maps/ascent            ${t("une map et ses notes")}
GET  ${esc(origin)}/api/notes/ascent/retake-b
PUT  ${esc(origin)}/api/notes/ascent/retake-b  ${t("JSON, ou Markdown brut (text/markdown)")}
POST ${esc(origin)}/api/notes/ascent           ${t('créer : {"title": "…", "kind": "retake"} ou un fichier .md')}
GET  ${esc(origin)}/api/share/ascent/retake-b  ${t("la note pour Discord, en texte, en HTML")}
GET  ${esc(origin)}/api/search?q=retake+b
GET  ${esc(origin)}/api/export.json            ${t("tout le carnet")}</pre>
        <p>${t(
          "En ligne de commande : <code>protocol search retake b</code>, <code>protocol export --map ascent --format discord</code>, <code>protocol serve --lan</code> pour partager le carnet en lecture seule sur le réseau local.",
        )}</p>
      </section>
      ${st.local ? mcpSection(st) : ""}
      <section class="box" aria-labelledby="h-credits"><h2 class="box-title" id="h-credits">${t("Crédits")}</h2>
        <p>${t(
          "Direction artistique d'après <a href=\"https://ascii.rest\">ascii.rest</a> (bas3line, licence MIT) : ses pièces « ui » (cadres, contrôles de formulaire, séparateurs, arborescence) inspirent l'interface, ses modules <code>banner</code> et <code>mount</code> dessinent le titre et la mini-carte. Police <a href=\"https://github.com/paper-design/paper-mono\">Paper Mono</a> de paper.design (OFL). Traits et blocs : JetBrains Mono (OFL). Markdown : pulldown-cmark (MIT). <a href=\"/static/licenses.txt\">Licences complètes</a>.",
        )}</p>
        <p class="muted">${t(
          "protocol est un logiciel libre (licence MIT). Il n'est ni approuvé ni soutenu par Riot Games et ne reflète pas les opinions de Riot Games ni de quiconque participe officiellement à la production ou à la gestion de ses propriétés. Riot Games, Valorant et les propriétés associées sont des marques commerciales ou déposées de Riot Games, Inc.",
        )}</p>
        <p class="muted">protocol ${esc(st.version)}</p>
      </section>
    </div>`;
  enhanceDocs(page);
}

function viewReadOnly() {
  page.innerHTML = `${crumbs([t("Lecture seule")])}<div class="empty box"><h1 class="box-title">${t("Lecture seule")}</h1><p>${t(
    "Ce carnet est ouvert en lecture seule ici : les modifications se font sur le poste du coach.",
  )}</p></div>`;
}

function viewNotFound(r, message) {
  S.mapData = null;
  renderChrome();
  page.innerHTML = `${crumbs([t("Introuvable")])}<div class="empty box"><h1 class="box-title">${t("404 · introuvable")}</h1><p class="q">${esc(
    cap(message || t("cette page n'existe pas.")),
  )}</p><p class="muted">${t("Elle a peut-être été renommée, déplacée ou mise à la corbeille.")}</p><div class="actions">${linkBtn(
    "#/",
    t("Retour à l'accueil"),
    "",
    "primary",
  )}</div></div>`;
}

function viewError(e) {
  page.innerHTML = `${crumbs([t("Erreur")])}<div class="empty box"><h1 class="box-title">${t("Erreur")}</h1><p class="form-error">${esc(
    cap(e.message || String(e)),
  )}</p><div class="actions">${linkBtn("#/", t("Retour à l'accueil"))}</div></div>`;
}

// ── Copier, télécharger, importer ─────────────────────────────────────────────

/**
 * Le texte à copier se prépare dès l'affichage : le clic écrit aussitôt dans le
 * presse-papiers. Les images, elles, ne sont téléchargées qu'au moment de copier.
 */
function prefetchShare(url) {
  const entry = { url, ready: null, promise: null };
  entry.promise = (async () => {
    const { data } = await cached(url);
    return { data, html: richHtml(data.html, location.origin) };
  })();
  entry.promise.then(
    (v) => (entry.ready = v),
    () => {},
  );
  S.share = entry;
}

const COPIED = {
  auto: t("Copié : prêt pour Discord, Word ou Docs."),
  discord: t("Copié pour Discord."),
  rich: t("Copié avec la mise en forme."),
  text: t("Texte copié."),
  markdown: t("Markdown copié, en-tête compris."),
};

async function doCopy(format) {
  const sh = S.share;
  if (!sh) return;
  let v = sh.ready;
  if (!v) {
    try {
      v = await sh.promise;
    } catch (e) {
      return toast(esc(t("Copie impossible : {error}", { error: e.message })), { icon: "alert" });
    }
  }
  const { data, html } = v;
  const payload = {
    auto: { text: data.discord, html },
    discord: { text: data.discord },
    rich: { text: data.text, html },
    text: { text: data.text },
    markdown: { text: data.markdown },
  }[format] ?? { text: data.discord, html };
  // Les captures du carnet partent embarquées dans le document collé (Word, Docs) : le
  // navigateur attend leur téléchargement sans perdre le geste de l'utilisateur.
  if (payload.html && hasLocalImages(payload.html, location.origin)) payload.later = inlineImages(payload.html, location.origin);
  if (!(await clipCopy(payload))) return manualCopy(payload.text);
  const long =
    (format === "auto" || format === "discord") && [...data.discord].length > 2000 ? ` ${t("Plus de 2 000 caractères : Discord l'enverra en fichier.")}` : "";
  toast(`${esc(COPIED[format] ?? COPIED.auto)}${long}`, { icon: "check" });
}

/** Dernier recours : le texte sélectionné dans une boîte, à copier soi-même. */
function manualCopy(text) {
  let d = $("#manual");
  if (!d) {
    d = document.createElement("dialog");
    d.id = "manual";
    d.className = "manual";
    d.setAttribute("aria-labelledby", "manual-h");
    document.body.append(d);
  }
  d.innerHTML = `<div class="box"><h2 class="box-title" id="manual-h">${t("Copier à la main")}</h2><p>${t(
    "Le navigateur refuse la copie automatique ici. Le texte est sélectionné : faites <kbd>Ctrl</kbd> + <kbd>C</kbd>.",
  )}</p><textarea class="area" readonly aria-label="${esc(t("Texte à copier"))}"></textarea><div class="form-actions">${btn(
    t("Fermer"),
    'data-act="manual-close"',
    "primary",
  )}</div></div>`;
  const ta = $("textarea", d);
  ta.value = text;
  d.showModal();
  ta.focus();
  ta.select();
}

function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

/** La map où importer : celle de la page, ou celle choisie dans le formulaire. */
const importTarget = () => $("#n-map")?.value || S.route.map || S.lastMap || mapsList()[0]?.id;

async function importFiles(list, map) {
  const files = [...list].filter((f) => /\.(md|markdown|txt)$/i.test(f.name));
  if (!files.length) return toast(esc(t("Seuls les fichiers .md (ou .txt) s'importent ici.")), { icon: "alert" });
  if (!map) return;
  let done = 0;
  let last = null;
  const refused = [];
  for (const f of files) {
    if (f.size > 2 * 1024 * 1024) {
      refused.push(`${f.name} (${t("trop lourd")})`);
      continue;
    }
    try {
      const text = await f.text();
      last = await api("POST", `/api/notes/${enc(map)}?name=${enc(f.name.replace(/\.[^.]+$/, ""))}`, new Blob([text], { type: "text/markdown" }));
      done++;
    } catch (e) {
      refused.push(`${f.name} (${e.message})`);
    }
  }
  await loadState().catch(() => {});
  if (done === 1 && files.length === 1) location.hash = noteHref(last);
  else if (S.route.view === "map" && S.route.map === map) await keepFocus(() => render({ keepScroll: true }));
  else if (done) location.hash = `#/map/${enc(map)}`;
  const msg = done ? tp(done, "{n} note importée dans {map}.", "{n} notes importées dans {map}.", { map: esc(mapTitle(map)) }) : t("Aucune note importée.");
  toast(`${msg}${refused.length ? ` ${t("Refusé : {files}.", { files: esc(refused.join(", ")) })}` : ""}`, { icon: done ? "check" : "alert" });
}

// ── Éditeur (note ou présentation de map) ─────────────────────────────────────

function readDraft(key) {
  try {
    const d = JSON.parse(localStorage.getItem(key) || "null");
    return d && typeof d.body === "string" && Number.isFinite(d.at) ? d : null;
  } catch {
    return null;
  }
}

/** Le paragraphe qui précède `pos` : de quoi séparer un bloc inséré par une ligne vide. */
function leadFor(before) {
  return before === "" || before.endsWith("\n\n") ? "" : before.endsWith("\n") ? "\n" : "\n\n";
}

/** Un message dans un cadre titré, avec ses choix : brouillon, conflit. `text` est du HTML sûr. */
const notice = (cls, title, text, buttons) =>
  `<div class="notice box${cls ? ` ${cls}` : ""}" role="alert"><span class="box-title">${esc(title)}</span><p class="q">${text}</p><div class="actions">${buttons}</div></div>`;

class Editor {
  constructor({ mode, target }) {
    this.mode = mode;
    this.map = mode === "note" ? target.map : target.id;
    this.id = mode === "note" ? target.id : null;
    this.version = target.version;
    this.kind = mode === "note" ? target.kind : "";
    // Le formulaire est capturé une fois pour toutes : après la sortie de l'éditeur,
    // il n'ira jamais lire ou écrire celui d'une autre page.
    const q = (id) => document.getElementById(id);
    this.el = {
      title: q("f-title"),
      body: q("f-body"),
      kind: q("f-kind"),
      sides: $$('input[name="e-side"]', page),
      map: q("f-map"),
      tags: q("f-tags"),
      pin: q("f-pin"),
      pool: q("f-pool"),
      status: q("status"),
      msgs: q("msgs"),
      done: q("done"),
      preview: q("preview"),
      previewPane: q("previewPane"),
      editor: q("editor"),
      file: q("f-file"),
      previewBtn: $('[data-act="preview"]', page),
    };
    this.dirty = false;
    this.saving = null;
    this.again = false;
    this.disposed = false;
    this.current = null; // version du disque en conflit, en attente d'un choix
    this.touched = new Set(); // métadonnées modifiées depuis le dernier enregistrement
    this.timer = 0;
    this.previewTimer = 0;
    this.draftTimer = 0;
    this.previewSeq = 0;
    this.previewed = null;
    this.statusKey = `ok|${t("Enregistré")}`;
    this.draftKey = this.keyFor();
    this.showPreview = matchMedia("(min-width: 1100px)").matches;
    this.applyPreview();

    const el = this.el;
    el.title.addEventListener("input", () => this.changed());
    el.body.addEventListener("input", () => this.changed(true));
    el.tags?.addEventListener("input", () => this.changed(false, "tags"));
    for (const name of ["kind", "map"]) {
      el[name]?.addEventListener("change", () => {
        if (name === "kind") this.kind = el.kind.value;
        this.changed(false, name);
      });
    }
    for (const r of el.sides) r.addEventListener("change", () => this.changed(false, "side"));
    for (const name of ["pin", "pool"]) {
      const b = el[name];
      b?.addEventListener("click", () => {
        b.setAttribute("aria-pressed", String(b.getAttribute("aria-pressed") !== "true"));
        this.changed(false, name);
      });
    }
    el.done.addEventListener("click", (e) => {
      e.preventDefault();
      finishEditing();
    });
    el.body.addEventListener("keydown", (e) => this.onKey(e));
    el.body.addEventListener("paste", (e) => this.onPaste(e));
    el.body.addEventListener("dragover", (e) => {
      if ([...(e.dataTransfer?.items || [])].some((i) => i.kind === "file")) {
        e.preventDefault();
        el.body.classList.add("drop");
      }
    });
    el.body.addEventListener("dragleave", () => el.body.classList.remove("drop"));
    el.body.addEventListener("drop", (e) => {
      const files = [...(e.dataTransfer?.files || [])].filter((f) => f.type.startsWith("image/"));
      el.body.classList.remove("drop");
      if (!files.length) return;
      e.preventDefault();
      files.forEach((f) => this.upload(f));
    });
    el.file?.addEventListener("change", (e) => {
      [...e.target.files].forEach((f) => this.upload(f));
      e.target.value = "";
    });

    // Un brouillon d'une session précédente ? S'il est déjà sur le disque, on l'oublie.
    const draft = readDraft(this.draftKey);
    if (draft) {
      if (norm(draft.body) === norm(el.body.value) && norm(draft.title ?? el.title.value) === norm(el.title.value)) {
        this.clearDraft();
      } else {
        this.draft = draft;
        el.msgs.innerHTML = notice(
          "warn",
          t("Brouillon"),
          esc(t("Un brouillon non enregistré existe ({ago}).", { ago: ago(draft.at) })),
          btn(t("Le reprendre"), 'data-act="draft-restore"', "small") + btn(t("L'oublier"), 'data-act="draft-drop"', "small"),
        );
      }
    }
    this.renderPreview();
  }

  keyFor() {
    return this.mode === "note" ? `protocol-draft:${this.map}/${this.id}` : `protocol-draft:map/${this.map}`;
  }

  url() {
    return this.mode === "note" ? `/api/notes/${enc(this.map)}/${enc(this.id)}` : `/api/maps/${enc(this.map)}`;
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.timer);
    clearTimeout(this.previewTimer);
    clearTimeout(this.draftTimer);
  }

  side() {
    return this.el.sides.find((r) => r.checked)?.value ?? "";
  }

  /** Ce qu'on envoie : titre et corps, plus les seules métadonnées touchées. */
  values(fields) {
    const el = this.el;
    if (this.mode === "map") {
      const v = { title: el.title.value, overview: el.body.value };
      if (fields.has("pool") && el.pool) v.pool = el.pool.getAttribute("aria-pressed") === "true";
      return v;
    }
    const v = { title: el.title.value, body: el.body.value };
    if (fields.has("kind")) v.kind = el.kind.value;
    if (fields.has("side")) v.side = this.side();
    if (fields.has("map")) v.map = el.map.value;
    if (fields.has("tags"))
      v.tags = el.tags.value
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
    if (fields.has("pin")) v.pin = el.pin.getAttribute("aria-pressed") === "true";
    return v;
  }

  setStatus(text, cls = "") {
    if (this.disposed) return;
    const key = `${cls}|${text}`;
    if (key === this.statusKey) return;
    this.statusKey = key;
    this.el.status.textContent = text;
    this.el.status.className = `status ${cls}`;
    if (cls === "err") announce(text);
  }

  changed(bodyChanged = false, field = null) {
    if (this.disposed) return;
    if (field) this.touched.add(field);
    this.dirty = true;
    this.setStatus(t("Modifications…"), "busy");
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.save(), 900);
    if (bodyChanged) {
      clearTimeout(this.previewTimer);
      this.previewTimer = setTimeout(() => this.renderPreview(), 180);
    }
    clearTimeout(this.draftTimer);
    this.draftTimer = setTimeout(() => this.storeDraft(), 400);
  }

  storeDraft() {
    try {
      const d = { body: this.el.body.value, title: this.el.title.value, base: this.version, at: Date.now() };
      localStorage.setItem(this.draftKey, JSON.stringify(d));
    } catch {}
  }

  clearDraft() {
    try {
      localStorage.removeItem(this.draftKey);
    } catch {}
  }

  save() {
    clearTimeout(this.timer);
    if (this.current) return Promise.resolve(); // conflit : on attend un choix
    if (this.saving) {
      this.again = true;
      return this.saving;
    }
    if (!this.dirty) return Promise.resolve();
    if (!this.el.title.value.trim()) {
      this.setStatus(t("Un titre est nécessaire"), "err");
      return Promise.resolve();
    }
    const fields = new Set(this.touched);
    const v = this.values(fields);
    this.touched.clear();
    this.dirty = false;
    this.setStatus(t("Enregistrement…"), "busy");
    const url = this.url();
    this.saving = (async () => {
      try {
        const res = await api("PUT", url, { ...v, baseVersion: this.version });
        this.version = res.version;
        if (!this.dirty) this.clearDraft();
        if (this.mode === "note" && (res.map !== this.map || res.id !== this.id)) this.moved(res);
        this.setStatus(this.dirty ? t("Modifications…") : t("Enregistré"), this.dirty ? "busy" : "ok");
        this.refreshChrome();
      } catch (e) {
        for (const f of fields) this.touched.add(f);
        this.dirty = true;
        if (e.status === 409 && e.data?.current) {
          this.conflict(e.data.current);
        } else {
          this.setStatus(t("Non enregistré : {error}", { error: e.message }), "err");
          this.storeDraft();
          if (e.status === 0 && !this.disposed) this.timer = setTimeout(() => this.save(), 4000);
        }
      } finally {
        this.saving = null;
        if (this.again) {
          this.again = false;
          if (this.dirty && !this.disposed) this.save();
        }
      }
    })();
    return this.saving;
  }

  /** Attend la fin de tout enregistrement, y compris celui relancé à la volée. */
  async flush() {
    clearTimeout(this.timer);
    while (this.saving) await this.saving.catch(() => {});
    if (this.dirty && !this.current) await this.save();
    while (this.saving) await this.saving.catch(() => {});
  }

  /** La note a changé de map : l'adresse suit, si l'éditeur est encore à l'écran. */
  moved(res) {
    const oldKey = this.draftKey;
    etags.delete(`/api/notes/${enc(this.map)}/${enc(this.id)}`);
    this.map = res.map;
    this.id = res.id;
    this.draftKey = this.keyFor();
    try {
      const d = localStorage.getItem(oldKey);
      if (d) {
        localStorage.setItem(this.draftKey, d);
        localStorage.removeItem(oldKey);
      }
    } catch {}
    S.lastMap = res.map;
    if (S.editor === this) {
      history.replaceState(null, "", `#/edit/${enc(res.map)}/${enc(res.id)}`);
      S.route = parse(location.hash);
    }
    if (this.el.done.isConnected) this.el.done.href = noteHref(res);
  }

  refreshChrome() {
    loadState()
      .then(async (fresh) => {
        if (!fresh || this.disposed) return;
        const md = await cached(`/api/maps/${enc(this.map)}`).catch(() => null);
        if (this.disposed || S.editor !== this) return;
        if (md) S.mapData = md.data;
        renderChrome();
      })
      .catch(() => {});
  }

  /** À la fermeture de la page : un dernier envoi qui survit au déchargement. */
  flushKeepalive() {
    if (!this.dirty || this.disposed) return;
    this.storeDraft();
    if (!this.el.title.value.trim() || this.current) return;
    const body = JSON.stringify({ ...this.values(this.touched), baseVersion: this.version });
    // Le quota keepalive se compte en octets (64 Ko), pas en caractères.
    if (byteSize(body) < 60000) {
      fetch(this.url(), { method: "PUT", keepalive: true, headers: { "Content-Type": "application/json" }, body }).catch(() => {});
    }
  }

  conflict(current) {
    this.current = current;
    clearTimeout(this.timer);
    this.storeDraft();
    this.setStatus(t("Conflit"), "err");
    if (this.disposed) return;
    this.el.msgs.innerHTML = notice(
      "warn",
      t("Conflit"),
      this.mode === "note"
        ? t("Cette note a été modifiée ailleurs (autre éditeur, synchronisation) depuis son ouverture.")
        : t("Cette présentation a été modifiée ailleurs (autre éditeur, synchronisation) depuis son ouverture."),
      btn(t("Garder ma version"), 'data-act="keep-mine"', "small") + btn(t("Prendre la version du disque"), 'data-act="take-theirs"', "small"),
    );
    $('[data-act="keep-mine"]', this.el.msgs)?.focus();
  }

  keepMine() {
    if (!this.current) return;
    this.version = this.current.version;
    this.current = null;
    this.el.msgs.innerHTML = "";
    this.dirty = true;
    this.save();
    this.el.body.focus();
  }

  takeTheirs() {
    if (!this.current) return;
    const cur = this.current;
    this.current = null;
    this.load(cur);
    this.el.msgs.innerHTML = "";
    // Le brouillon (ma version) est gardé : une erreur de clic reste rattrapable.
    this.setStatus(t("Version du disque reprise"), "ok");
    this.el.body.focus();
  }

  load(n) {
    const el = this.el;
    el.title.value = n.title;
    if (this.mode === "map") {
      el.body.value = n.overview ?? "";
      el.pool?.setAttribute("aria-pressed", String(n.pool === true));
    } else {
      el.body.value = n.body;
      el.tags.value = n.tags.join(", ");
      for (const r of el.sides) r.checked = r.value === n.side;
      if (kindOf(n.kind)) el.kind.value = n.kind;
      this.kind = n.kind;
      el.pin.setAttribute("aria-pressed", String(!!n.pin));
    }
    this.version = n.version;
    this.dirty = false;
    this.touched.clear();
    this.previewed = null;
    this.renderPreview();
  }

  /** Le fichier a changé ailleurs pendant qu'on ne tapait pas : on recharge sans rien perdre. */
  async checkRemote() {
    if (this.saving || this.dirty || this.current || this.disposed) return;
    try {
      const { data } = await cached(this.mode === "note" ? this.url() : `/api/maps/${enc(this.map)}`, true);
      const n = this.mode === "note" ? data : data.map;
      if (n.version !== this.version && !this.dirty && !this.saving && !this.current && !this.disposed) {
        const caret = this.el.body.selectionStart;
        this.load(n);
        this.el.body.setSelectionRange(caret, caret);
        this.setStatus(t("Rechargée : modifiée ailleurs"), "ok");
        announce(t("Rechargée : modifiée ailleurs"));
      }
    } catch {}
  }

  applyPreview() {
    this.el.editor.classList.toggle("solo", !this.showPreview);
    this.el.previewPane.hidden = !this.showPreview;
    this.el.previewBtn?.setAttribute("aria-pressed", String(this.showPreview));
  }

  togglePreview() {
    this.showPreview = !this.showPreview;
    this.previewed = null;
    this.applyPreview();
    this.renderPreview();
  }

  async renderPreview() {
    if (!this.showPreview || this.disposed) return;
    const body = this.el.body.value;
    if (body === this.previewed) return;
    const seq = ++this.previewSeq;
    try {
      const { html } = await api("POST", "/api/render", { map: this.map, body });
      if (seq !== this.previewSeq || this.disposed) return;
      this.previewed = body;
      this.el.preview.innerHTML = html;
      enhanceDocs(this.el.preview);
    } catch {}
  }

  insert(text) {
    const ta = this.el.body;
    ta.focus();
    // execCommand garde l'historique d'annulation (Ctrl+Z) ; setRangeText en secours.
    if (!document.execCommand("insertText", false, text)) {
      ta.setRangeText(text, ta.selectionStart, ta.selectionEnd, "end");
      this.changed(true);
    }
  }

  wrap(before, after, placeholder) {
    const ta = this.el.body;
    const s = ta.selectionStart;
    const e = ta.selectionEnd;
    const sel = ta.value.slice(s, e) || placeholder;
    this.insert(before + sel + after);
    ta.setSelectionRange(s + before.length, s + before.length + sel.length);
  }

  /** Préfixe chaque ligne sélectionnée ; `make(i)` donne le préfixe de la i-ième. */
  prefixLines(make, already) {
    const ta = this.el.body;
    const v = ta.value;
    const ls = v.lastIndexOf("\n", ta.selectionStart - 1) + 1;
    let le = v.indexOf("\n", ta.selectionEnd);
    if (le < 0) le = v.length;
    const out = v
      .slice(ls, le)
      .split("\n")
      .map((l, i) => (already.test(l) ? l : make(i) + l))
      .join("\n");
    ta.setSelectionRange(ls, le);
    this.insert(out);
  }

  block(text) {
    const ta = this.el.body;
    this.insert(leadFor(ta.value.slice(0, ta.selectionStart)) + text);
  }

  insertBlock(id) {
    const b = BLOCKS.find((x) => x.id === id);
    if (!b) return;
    const text = b.text ?? (kindOf(this.kind)?.template || kindOf("protocole")?.template || "");
    if (text) this.block(text);
  }

  command(cmd) {
    switch (cmd) {
      case "h2":
        return this.prefixLines(() => "## ", /^#{1,6} /);
      case "bold":
        return this.wrap("**", "**", t("important"));
      case "italic":
        return this.wrap("*", "*", t("nuance"));
      case "list":
        return this.prefixLines(() => "- ", /^\s*[-*+] /);
      case "ol":
        return this.prefixLines((i) => `${i + 1}. `, /^\s*\d+[.)] /);
      case "check":
        return this.prefixLines(() => "- [ ] ", /^\s*[-*+] \[[ xX]\] /);
      case "table":
        return this.insertBlock("roles");
      case "callout":
        return this.insertBlock("warning");
      case "link":
        return this.wrap("[[", "]]", t("id-de-la-note"));
      case "image":
        return this.el.file?.click();
    }
  }

  onKey(e) {
    const mod = (e.ctrlKey || e.metaKey) && !e.getModifierState?.("AltGraph");
    const key = e.key.toLowerCase();
    if (mod && key === "b") {
      e.preventDefault();
      this.wrap("**", "**", t("important"));
    } else if (mod && key === "i") {
      e.preventDefault();
      this.wrap("*", "*", t("nuance"));
    } else if (e.key === "Enter" && !e.shiftKey && !e.isComposing && !mod) {
      // Une liste se poursuit toute seule ; un élément vide la termine.
      const ta = this.el.body;
      const pos = ta.selectionStart;
      if (pos !== ta.selectionEnd) return;
      const ls = ta.value.lastIndexOf("\n", pos - 1) + 1;
      const line = ta.value.slice(ls, pos);
      const m = line.match(/^(\s*)([-*+] \[[ xX]\] |[-*+] |(\d+)([.)]) |> )/);
      if (!m) return;
      e.preventDefault();
      if (line.trim() === m[2].trim()) {
        ta.setSelectionRange(ls, pos);
        this.insert("");
        return;
      }
      let marker = m[2];
      if (m[3]) marker = `${Number(m[3]) + 1}${m[4]} `;
      else marker = marker.replace(/\[[xX]\]/, "[ ]");
      this.insert(`\n${m[1]}${marker}`);
    }
  }

  onPaste(e) {
    const cd = e.clipboardData;
    if (!cd) return;
    const files = [...(cd.files || [])].filter((f) => f.type.startsWith("image/"));
    // Une copie de cellules Excel ou de Word contient texte ET image : le texte l'emporte.
    if (files.length && !cd.getData("text/plain")) {
      e.preventDefault();
      files.forEach((f) => this.upload(f));
      return;
    }
    // Ctrl+Maj+V : le navigateur colle le texte brut, sans conversion.
    if (plainPaste()) return;
    const md = pastedMarkdown(cd);
    if (md == null) return;
    e.preventDefault();
    this.insert(md);
  }

  async upload(file) {
    if (file.size > 15 * 1024 * 1024) {
      this.setStatus(t("Image trop lourde (15 Mo au plus)"), "err");
      return;
    }
    const at = this.el.body.selectionEnd; // l'image ira là où l'on a collé, même si le curseur bouge
    this.setStatus(t("Envoi de l'image…"), "busy");
    try {
      const r = await api("POST", "/api/assets", file);
      const alt = (file.name || t("capture")).replace(/\.[a-z0-9]+$/i, "").replace(/[[\]]/g, "") || t("capture");
      const md = `![${alt}](${r.markdown})`;
      if (this.disposed) {
        toast(t("Image envoyée après la fermeture de l'éditeur, à coller dans la note : {md}", { md: `<code>${esc(md)}</code>` }), { icon: "alert" });
        return;
      }
      const ta = this.el.body;
      const pos = Math.min(at, ta.value.length);
      ta.setRangeText(`${leadFor(ta.value.slice(0, pos))}${md}\n`, pos, pos, "preserve");
      this.changed(true);
    } catch (e) {
      this.setStatus(t("Image refusée : {error}", { error: e.message }), "err");
    }
  }
}

async function finishEditing() {
  const ed = S.editor;
  if (!ed) return;
  await ed.flush();
  if (S.editor !== ed) return; // déjà quitté entre-temps
  if (ed.dirty || ed.current) {
    announce(t("Non enregistré : l'éditeur reste ouvert"));
    return;
  }
  location.hash = ed.mode === "note" ? `#/note/${enc(ed.map)}/${enc(ed.id)}` : `#/map/${enc(ed.map)}`;
}

// ── Routage ───────────────────────────────────────────────────────────────────

function parse(hash) {
  const [path, query = ""] = hash.replace(/^#\/?/, "").split("?");
  const params = new URLSearchParams(query);
  const p = path.split("/").map((s) => {
    try {
      return decodeURIComponent(s);
    } catch {
      return s;
    }
  });
  switch (p[0]) {
    case "":
      return { view: "home" };
    case "map":
      return p[1] ? { view: "map", map: p[1] } : { view: "notfound" };
    case "note":
      return p[2] ? { view: "note", map: p[1], id: p[2] } : { view: "notfound" };
    case "edit":
      return p[2] ? { view: "edit", map: p[1], id: p[2] } : { view: "notfound" };
    case "edit-map":
      return p[1] ? { view: "editMap", map: p[1] } : { view: "notfound" };
    case "new":
      return { view: "new", map: p[1] || "", paste: params.has("paste") };
    case "new-map":
      return { view: "newMap" };
    case "sheet":
      return p[1] ? { view: "sheet", map: p[1] } : { view: "notfound" };
    case "search":
      return { view: "search", q: p.slice(1).join("/") };
    case "trash":
      return { view: "trash" };
    case "help":
      return { view: "help" };
    default:
      return { view: "notfound" };
  }
}

const VIEWS = {
  home: viewHome,
  map: viewMap,
  note: viewNote,
  edit: viewEdit,
  editMap: viewEditMap,
  new: viewNew,
  newMap: viewNewMap,
  sheet: viewSheet,
  search: viewSearch,
  trash: viewTrash,
  help: viewHelp,
  notfound: (r) => viewNotFound(r),
};

function titleFor(r) {
  const fixed = { home: "", search: t("Recherche"), trash: t("Corbeille"), help: t("Aide"), new: t("Nouvelle note"), newMap: t("Nouvelle map") }[r.view];
  if (fixed !== undefined) return fixed ? `${fixed} · protocol` : "protocol";
  if (r.view === "note" || r.view === "edit") return `${S.note && S.note.id === r.id ? S.note.title : r.id} · protocol`;
  if (r.view === "sheet") return `${t("Fiche match {map}", { map: mapTitle(r.map) })} · protocol`;
  if (r.map) return `${mapTitle(r.map)} · protocol`;
  return "protocol";
}

async function render({ keepScroll = false } = {}) {
  const r = S.route;
  const y = window.scrollY;
  teardown();
  S.share = null;
  try {
    await VIEWS[r.view](r);
  } catch (e) {
    if (stale(r)) return;
    if (e.status === 404) viewNotFound(r, e.message);
    else viewError(e);
  }
  if (stale(r)) return;
  document.title = titleFor(r);
  if (keepScroll) window.scrollTo(0, y);
}

let navSeq = 0;
async function navigate() {
  const seq = ++navSeq;
  setDrawer(false);
  if (S.editor) {
    const ed = S.editor;
    S.editor = null;
    try {
      await ed.flush();
    } catch {}
    ed.dispose();
    if (ed.dirty) toast(esc(t("Modifications non enregistrées : un brouillon est gardé sur ce poste.")), { icon: "alert" });
  }
  if (seq !== navSeq) return;
  const r = parse(location.hash);
  if (r.view !== "search") {
    if (find.value) find.value = "";
    S.hits = [];
    S.hitIndex = 0;
    find.setAttribute("aria-expanded", "false");
    find.removeAttribute("aria-activedescendant");
  }
  S.route = r;
  await render();
  if (stale(r)) return;
  if (r.view !== "search") {
    window.scrollTo(0, 0);
    if (!["edit", "editMap", "new", "newMap"].includes(r.view) && document.activeElement !== find) page.focus({ preventScroll: true });
  }
}

/** Échap : un cran plus haut dans le carnet (jamais en dehors). */
function up() {
  const r = S.route;
  if (["note", "sheet", "editMap"].includes(r.view)) location.hash = `#/map/${enc(r.map)}`;
  else if (r.view === "new" && r.map) location.hash = `#/map/${enc(r.map)}`;
  else if (r.view === "search") location.hash = S.beforeSearch || "#/";
  else if (r.view !== "home") location.hash = "#/";
}

function cycleMap(d) {
  const maps = mapsList();
  if (!maps.length) return;
  const i = maps.findIndex((m) => m.id === S.route.map);
  const j = i < 0 ? (d > 0 ? 0 : maps.length - 1) : (i + d + maps.length) % maps.length;
  location.hash = `#/map/${enc(maps[j].id)}`;
}

// ── Messages ──────────────────────────────────────────────────────────────────

const toastEl = $("#toast");
function closeToast() {
  clearTimeout(S.toastTimer);
  toastEl.innerHTML = "";
  S.toastActions = {};
}
function armToast(ms) {
  clearTimeout(S.toastTimer);
  S.toastTimer = setTimeout(closeToast, ms);
}
/**
 * Un message en bas d'écran, comme une ligne de journal de démarrage : [ OK ] ou [ !! ].
 * `actions` : { clé: { label, run } } ; `icon: "alert"` pour un échec.
 */
function toast(html, { actions = {}, icon: tone = "check", ms = 10000 } = {}) {
  const buttons = Object.entries(actions)
    .map(([k, a]) => btn(esc(a.label), `data-toast="${esc(k)}"`, "small"))
    .join("");
  const mark = tone === "alert" ? '<span class="mark alert" aria-hidden="true">[ !! ]</span>' : '<span class="mark" aria-hidden="true">[ OK ]</span>';
  toastEl.innerHTML = `<div class="toast-box">${mark}<span class="msg">${html}</span>${buttons}${btn(
    "x",
    `data-toast="close" aria-label="${esc(t("Fermer le message"))}"`,
    "small",
  )}</div>`;
  S.toastActions = Object.fromEntries(Object.entries(actions).map(([k, a]) => [k, a.run]));
  armToast(ms);
}
// Survolé ou atteint au clavier, le message reste affiché.
for (const ev of ["mouseenter", "focusin"]) toastEl.addEventListener(ev, () => clearTimeout(S.toastTimer));
for (const ev of ["mouseleave", "focusout"]) toastEl.addEventListener(ev, () => toastEl.innerHTML && armToast(4000));

function toggleTheme() {
  const next = isDark() ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  // Gardé dans le carnet : le navigateur de la fenêtre, lui, oublie tout à la fermeture.
  if (!readOnly()) api("POST", "/api/settings", { theme: next }).catch(() => {});
  // Un formulaire en cours (éditeur, nouvelle note) n'est pas redessiné : seule la barre change.
  if (S.editor || ["new", "newMap"].includes(S.route.view)) renderChrome();
  else keepFocus(() => render({ keepScroll: true }));
}

// ── Actions ───────────────────────────────────────────────────────────────────

async function act(name, el) {
  const r = S.route;
  switch (name) {
    case "theme":
      return toggleTheme();
    case "copy-mcp": {
      const text = $(`#${el.dataset.from}`)?.textContent || "";
      if (!(await clipCopy({ text }))) return manualCopy(text);
      return toast(esc(t("Configuration copiée.")), { icon: "check" });
    }
    case "quit":
      if (S.editor) await S.editor.flush();
      await api("POST", "/api/quit").catch(() => {});
      $("#offline").textContent = t("Le carnet est fermé. À bientôt !");
      setOffline(true);
      return;
    case "pool": {
      const m = S.mapData.map;
      await api("PUT", `/api/maps/${enc(m.id)}`, { pool: m.pool !== true });
      await loadState();
      return keepFocus(() => render({ keepScroll: true }));
    }
    case "pin": {
      const n = S.note;
      await api("PUT", `/api/notes/${enc(n.map)}/${enc(n.id)}`, { pin: !n.pin });
      await loadState();
      await keepFocus(() => render({ keepScroll: true }));
      return announce(n.pin ? t("Note désépinglée") : t("Note épinglée"));
    }
    case "reveal-root":
      return api("POST", "/api/reveal", {}).catch((e) => toast(esc(e.message), { icon: "alert" }));
    case "notice-ok":
      await api("DELETE", "/api/notice");
      await loadState();
      $("#home-notice")?.remove();
      return page.focus({ preventScroll: true });
    case "download-note": {
      const v = S.share?.ready ?? (await S.share?.promise.catch(() => null));
      if (v) download(v.data.filename, v.data.markdown);
      return;
    }
    case "delete": {
      // Pas de confirmation : la note part à la corbeille, et le message propose d'annuler.
      const n = S.note;
      const res = await api("DELETE", `/api/notes/${enc(n.map)}/${enc(n.id)}`);
      etags.delete(`/api/notes/${enc(n.map)}/${enc(n.id)}`);
      await loadState();
      location.hash = `#/map/${enc(n.map)}`;
      toast(t("« {title} » est dans la corbeille.", { title: esc(n.title) }), {
        icon: "trash",
        actions: {
          undo: {
            label: t("Annuler"),
            icon: "undo",
            run: async () => {
              const back = await api("POST", "/api/trash/restore", { path: res.trash });
              await loadState();
              location.hash = `#/note/${enc(back.map)}/${enc(back.id)}`;
            },
          },
        },
      });
      return;
    }
    case "delete-map":
      $("#del").innerHTML = `<span class="confirm"><span class="q">${t("Supprimer cette map vide ?")}</span>${btn(
        t("Oui, supprimer"),
        'data-act="delete-map-yes"',
        "danger",
      )}${btn(t("Non"), 'data-act="delete-map-no"')}</span>`;
      return $('[data-act="delete-map-no"]').focus();
    case "delete-map-no":
      $("#del").innerHTML = btn(t("Supprimer la map"), 'data-act="delete-map"', "danger");
      return $('[data-act="delete-map"]').focus();
    case "delete-map-yes": {
      await api("DELETE", `/api/maps/${enc(r.map)}`);
      const ed = S.editor;
      if (ed) {
        S.editor = null;
        ed.dispose();
        ed.clearDraft();
      }
      await loadState();
      location.hash = "#/";
      return toast(esc(t("Map supprimée. Sa présentation est dans la corbeille.")), { icon: "trash" });
    }
    case "clear-filters": {
      const f = S.filters[r.map];
      if (f) {
        f.kind = "";
        f.side = "";
      }
      await render({ keepScroll: true });
      // Le bouton « Tout afficher » a disparu avec le message : le focus va au filtre « Tout ».
      return $('input[name="f-kind"]:checked')?.focus();
    }
    case "import":
      return $("#importFile").click();
    case "preview":
      return S.editor?.togglePreview();
    case "keep-mine":
      return S.editor?.keepMine();
    case "take-theirs":
      return S.editor?.takeTheirs();
    case "draft-restore": {
      const ed = S.editor;
      if (!ed?.draft) return;
      ed.el.body.value = ed.draft.body;
      if (typeof ed.draft.title === "string" && ed.draft.title.trim()) ed.el.title.value = ed.draft.title;
      // Base du brouillon : si le disque a bougé depuis, l'enregistrement le dira (conflit).
      if (typeof ed.draft.base === "string" && ed.draft.base) ed.version = ed.draft.base;
      ed.draft = null;
      ed.el.msgs.innerHTML = "";
      ed.el.body.focus();
      return ed.changed(true);
    }
    case "draft-drop":
      S.editor?.clearDraft();
      if (S.editor) {
        S.editor.draft = null;
        S.editor.el.msgs.innerHTML = "";
        S.editor.el.body.focus();
      }
      return;
    case "new-draft-drop":
      clearNewDraft();
      $("#n-title").value = "";
      $("#n-body").value = "";
      $("#n-msgs").innerHTML = "";
      updateKindHint();
      return $("#n-title").focus();
    case "print":
      return window.print();
    case "manual-close":
      return el?.closest("dialog")?.close();
  }
}

document.addEventListener("click", async (e) => {
  if (e.target.closest("#menu")) return setDrawer(!$("#side").classList.contains("open"));
  if (e.target.closest("#scrim")) return setDrawer(false);
  // Un choix fait dans un menu le referme, que ce soit une action ou un lien (téléchargement…).
  const open = e.target.closest(".menu-item")?.closest(".menu[popover]");
  if (open?.matches(":popover-open")) open.hidePopover();
  const el = e.target.closest("[data-act],[data-restore],[data-md],[data-block],[data-copy],[data-toast]");
  if (!el) return;
  try {
    if (el.dataset.act) await act(el.dataset.act, el);
    else if (el.dataset.copy) await doCopy(el.dataset.copy);
    else if (el.dataset.restore) {
      const back = await api("POST", "/api/trash/restore", { path: el.dataset.restore });
      await loadState();
      location.hash = back.id ? `#/note/${enc(back.map)}/${enc(back.id)}` : `#/map/${enc(back.map)}`;
    } else if (el.dataset.md) {
      S.editor?.command(el.dataset.md);
    } else if (el.dataset.block) {
      S.editor?.insertBlock(el.dataset.block);
    } else if (el.dataset.toast) {
      const run = S.toastActions[el.dataset.toast];
      closeToast();
      await run?.();
    }
  } catch (err) {
    toast(esc(cap(err.message || String(err))), { icon: "alert" });
  }
});

document.addEventListener("change", (e) => {
  const el = e.target;
  if (el.id === "importFile") {
    const files = [...el.files];
    el.value = "";
    if (files.length) importFiles(files, importTarget());
    return;
  }
  if (!(el instanceof HTMLInputElement) || el.type !== "radio") return;
  if (el.name === "kind") return updateKindHint();
  if (el.name === "ui-lang") {
    // La langue se garde dans le carnet ; la page se recharge pour tout redessiner.
    api("POST", "/api/settings", { lang: el.value })
      .then(() => location.reload())
      .catch((err) => toast(esc(cap(err.message || String(err))), { icon: "alert" }));
    return;
  }
  if (el.name === "mcp-mode") {
    S.mcpWrite = el.value === "write";
    const m = mcpConfig(S.state, S.mcpWrite);
    $("#mcp-desktop").textContent = m.desktop;
    $("#mcp-code").textContent = m.code;
    return;
  }
  if (S.route.view !== "map") return;
  const f = S.filters[S.route.map];
  if (!f) return;
  if (el.name === "f-kind") f.kind = el.value;
  else if (el.name === "f-side") f.side = el.value;
  else return;
  renderCards(S.route.map);
});

let newDraftTimer = 0;
document.addEventListener("input", (e) => {
  const id = e.target.id;
  if (id === "n-body") updateKindHint();
  if (id === "n-body" || id === "n-title") {
    clearTimeout(newDraftTimer);
    newDraftTimer = setTimeout(storeNewDraft, 300);
  }
});

document.addEventListener("submit", async (e) => {
  e.preventDefault();
  const form = e.target;
  const btn = $('button[type="submit"]', form);
  if (!btn || btn.disabled) return;
  const fail = (id, msg, field) => {
    const p = $(id);
    p.textContent = msg;
    p.hidden = false;
    field?.setAttribute("aria-invalid", "true");
    field?.focus();
  };
  btn.disabled = true;
  btn.setAttribute("aria-busy", "true");
  try {
    if (form.id === "newform") {
      const data = new FormData(form);
      const map = String(data.get("map") || "");
      let title = String(data.get("title") || "").trim();
      let body = norm(data.get("body"));
      if (body) {
        // Le texte collé : son premier titre, s'il devient celui de la note, quitte le corps.
        const g = guessTitle(body);
        if (!title) title = g.title;
        if (title === g.title && g.body !== body) body = norm(g.body);
      }
      if (!title) return fail("#n-err", t("Donnez un titre à la note, ou collez un texte qui en a un."), $("#n-title"));
      try {
        const n = await api("POST", `/api/notes/${enc(map)}`, {
          title,
          kind: String(data.get("kind") || ""),
          side: String(data.get("side") || ""),
          ...(body ? { body } : {}),
        });
        S.lastMap = map;
        clearTimeout(newDraftTimer);
        clearNewDraft();
        await loadState();
        if (body) {
          location.hash = noteHref(n);
          toast(esc(t("Note créée à partir du texte collé.")), {
            actions: { edit: { label: t("Modifier"), icon: "edit", run: () => (location.hash = `#/edit/${enc(n.map)}/${enc(n.id)}`) } },
          });
        } else {
          location.hash = `#/edit/${enc(n.map)}/${enc(n.id)}`;
        }
      } catch (err) {
        fail("#n-err", cap(err.message));
      }
    } else if (form.id === "mapform") {
      try {
        const m = await api("POST", "/api/maps", { title: $("#m-title").value });
        await loadState();
        location.hash = `#/map/${enc(m.id)}`;
      } catch (err) {
        fail("#m-err", cap(err.message), $("#m-title"));
      }
    }
  } finally {
    if (btn.isConnected) {
      btn.disabled = false;
      btn.removeAttribute("aria-busy");
    }
  }
});

// ── Menus ─────────────────────────────────────────────────────────────────────

/** Place un menu sous son bouton (au-dessus s'il manque de place), sans sortir de l'écran. */
function placeMenu(m) {
  const trigger = $(`[popovertarget="${CSS.escape(m.id)}"]`);
  if (!trigger) return;
  const r = trigger.getBoundingClientRect();
  const w = m.offsetWidth;
  const h = m.offsetHeight; // borné à la hauteur de l'écran par la CSS (le reste défile dans le menu)
  let left = r.left;
  if (left + w > innerWidth - 8) left = Math.max(8, r.right - w);
  let top = r.bottom + 6;
  if (top + h > innerHeight - 8 && r.top - h - 6 > 8) top = r.top - h - 6;
  top = Math.max(8, Math.min(top, innerHeight - 8 - h));
  m.style.left = `${Math.round(left)}px`;
  m.style.top = `${Math.round(top)}px`;
}

document.addEventListener(
  "toggle",
  (e) => {
    const m = e.target;
    if (!(m instanceof HTMLElement) || !m.classList.contains("menu")) return;
    const trigger = $(`[popovertarget="${CSS.escape(m.id)}"]`);
    trigger?.setAttribute("aria-expanded", String(e.newState === "open"));
    if (e.newState === "open") {
      placeMenu(m);
      $(".menu-item", m)?.focus();
    } else if (trigger?.isConnected && (m.contains(document.activeElement) || document.activeElement === document.body)) {
      // Refermé (Échap, choix fait) : le focus revient au bouton qui l'avait ouvert.
      trigger.focus({ preventScroll: true });
    }
  },
  true,
);

document.addEventListener("keydown", (e) => {
  const m = e.target.closest?.(".menu[popover]");
  if (!m) return;
  if (e.key === "Tab") {
    // Tab quitte le menu : il se referme, le focus revient à son bouton.
    e.preventDefault();
    if (m.matches(":popover-open")) m.hidePopover();
    $(`[popovertarget="${CSS.escape(m.id)}"]`)?.focus();
    return;
  }
  if (e.key === " " && e.target.matches("a.menu-item")) {
    // Espace active un lien du menu, comme un bouton, au lieu de faire défiler la page.
    e.preventDefault();
    e.target.click();
    return;
  }
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
  const items = $$(".menu-item", m);
  if (!items.length) return;
  e.preventDefault();
  const i = items.indexOf(document.activeElement);
  const j = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : (i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
  items[j].focus();
});

const closeMenus = () => $$(".menu[popover]").forEach((m) => m.matches(":popover-open") && m.hidePopover());
addEventListener("resize", closeMenus);
addEventListener("scroll", closeMenus, { passive: true });

// ── Clavier ───────────────────────────────────────────────────────────────────

document.addEventListener("keydown", (e) => {
  const tg = e.target;
  // AltGr (Ctrl+Alt sous Windows) sert à taper [ ] en AZERTY : ce n'est pas un raccourci.
  const altGr = !!e.getModifierState?.("AltGraph");
  const mod = (e.ctrlKey || e.metaKey) && !altGr;
  if (mod && e.key.toLowerCase() === "s") {
    // Enregistrer la page HTML n'a pas de sens ici : Ctrl+S enregistre la note, où que soit le focus.
    e.preventDefault();
    S.editor?.save();
    return;
  }
  if (mod && e.key === "Enter" && S.editor) {
    e.preventDefault();
    finishEditing();
    return;
  }
  // Entrée crée la note (ou la map), même depuis un choix de type ou de côté ;
  // sur un autre bouton du formulaire (« Importer… »), Entrée garde son rôle.
  const form = tg.closest?.("#newform, #mapform");
  if (form && e.key === "Enter" && !e.isComposing && !mod && tg.matches?.("input:not([type=submit],[type=button],[type=file]), select")) {
    e.preventDefault();
    form.requestSubmit();
    return;
  }
  if (tg.closest?.(".menu[popover], dialog")) return;
  const typing = tg instanceof HTMLInputElement || tg instanceof HTMLTextAreaElement || tg instanceof HTMLSelectElement || tg.isContentEditable;
  if (typing) {
    if (e.key === "Escape" && !e.isComposing && tg !== find && S.editor) {
      e.preventDefault();
      finishEditing();
    }
    return;
  }
  if (mod || (e.altKey && !altGr)) return;
  const r = S.route;
  switch (e.key) {
    case "/":
      e.preventDefault();
      find.focus();
      find.select();
      break;
    case "n":
    case "N":
      if (!readOnly()) location.hash = `#/new/${enc(r.map || "")}`;
      break;
    case "e":
    case "E":
      if (readOnly()) break;
      if (r.view === "note") location.hash = `#/edit/${enc(r.map)}/${enc(r.id)}`;
      else if (r.view === "map") location.hash = `#/edit-map/${enc(r.map)}`;
      break;
    case "f":
    case "F":
      if (r.map) location.hash = `#/sheet/${enc(r.map)}`;
      break;
    case "c":
    case "C":
      if (S.share) doCopy("auto");
      break;
    case "?":
      location.hash = "#/help";
      break;
    case "[":
      cycleMap(-1);
      break;
    case "]":
      cycleMap(1);
      break;
    case "Escape":
      // Un menu ou l'aide-mémoire ouvert : le navigateur le referme, rien d'autre.
      if ($(":popover-open")) break;
      if ($("#side").classList.contains("open")) setDrawer(false);
      else if (toastEl.innerHTML) closeToast();
      else if (S.editor) finishEditing();
      else up();
      break;
  }
});

let searchTimer = 0;
find.addEventListener("input", () => {
  const q = find.value;
  const target = `#/search/${enc(q)}`;
  if (S.route.view === "search") {
    history.replaceState(null, "", target);
    S.route.q = q;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => runSearch(q).catch(() => {}), 70);
  } else if (location.hash.startsWith("#/search/")) {
    // Navigation déjà en cours (l'éditeur s'enregistre) : on met l'adresse à jour sans empiler.
    history.replaceState(null, "", target);
  } else {
    S.beforeSearch = location.hash || "#/";
    location.hash = target;
  }
});
find.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") {
    e.preventDefault();
    moveHit(1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    moveHit(-1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const h = S.route.view === "search" ? S.hits[S.hitIndex] : null;
    if (h) {
      find.blur();
      location.hash = noteHref(h);
    }
  } else if (e.key === "Escape") {
    e.preventDefault();
    find.blur();
    if (S.route.view === "search") {
      find.value = "";
      location.hash = S.beforeSearch || "#/";
    }
  }
});

$("#skip").addEventListener("click", (e) => {
  e.preventDefault();
  page.focus();
});

// ── Coller et déposer sur une page ────────────────────────────────────────────

const PASTE_VIEWS = ["home", "map", "note", "sheet"];
const DROP_VIEWS = ["map", "new"];

// Ctrl+Maj+V : le collage qui suit reste brut, dans l'éditeur comme dans « Nouvelle note ».
let plainUntil = 0;
document.addEventListener(
  "keydown",
  (e) => {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "v") plainUntil = Date.now() + 800;
  },
  true,
);
function plainPaste() {
  const on = Date.now() < plainUntil;
  plainUntil = 0;
  return on;
}

// Ctrl+V ailleurs que dans un champ : le texte copié devient une nouvelle note.
// Dans le champ « Contenu » d'une nouvelle note : la mise en forme est convertie.
document.addEventListener("paste", (e) => {
  const tg = e.target;
  const cd = e.clipboardData;
  if (!cd) return;
  if (tg.id === "n-body") {
    // Ctrl+Maj+V : collage brut, comme partout ailleurs.
    const md = plainPaste() ? null : pastedMarkdown(cd);
    if (md != null) {
      e.preventDefault();
      if (!document.execCommand("insertText", false, md)) tg.setRangeText(md, tg.selectionStart, tg.selectionEnd, "end");
    }
    setTimeout(() => {
      fillTitleFrom(tg.value);
      updateKindHint();
      storeNewDraft();
    });
    return;
  }
  if (readOnly() || !PASTE_VIEWS.includes(S.route.view)) return;
  if (tg instanceof HTMLInputElement || tg instanceof HTMLTextAreaElement || tg.isContentEditable) return;
  const files = [...(cd.files || [])].filter((f) => /\.(md|markdown|txt)$/i.test(f.name));
  if (files.length) {
    e.preventDefault();
    importFiles(files, S.route.map || S.lastMap || mapsList()[0]?.id);
    return;
  }
  const md = pastedMarkdown(cd) ?? cd.getData("text/plain");
  if (!md?.trim()) return;
  e.preventDefault();
  S.pendingPaste = md;
  location.hash = `#/new/${enc(S.route.map || "")}?paste=1`;
});

let dropDepth = 0;
const dropHint = () => $("#dropHint");
function showDrop(on) {
  if (!on) {
    dropHint()?.remove();
    return;
  }
  if (dropHint()) return;
  const d = document.createElement("div");
  d.id = "dropHint";
  d.className = "drop-hint";
  d.textContent = t("Déposez vos fichiers .md pour les importer dans {map}", { map: mapTitle(importTarget()) });
  document.body.append(d);
}
const carriesFiles = (e) => [...(e.dataTransfer?.types || [])].includes("Files");
const dropAllowed = (e) => carriesFiles(e) && !readOnly() && DROP_VIEWS.includes(S.route.view) && !e.target.closest?.("#f-body");

document.addEventListener("dragenter", (e) => {
  if (!dropAllowed(e)) return;
  dropDepth++;
  showDrop(true);
});
document.addEventListener("dragleave", (e) => {
  if (!carriesFiles(e)) return;
  dropDepth = Math.max(0, dropDepth - 1);
  // Sortie de la fenêtre (rien en dessous) : le compteur repart de zéro, même si un
  // élément survolé a été redessiné entre-temps sans signaler son départ.
  if (!e.relatedTarget) dropDepth = 0;
  if (!dropDepth) showDrop(false);
});
// Un fichier lâché ailleurs que sur une map ne doit jamais remplacer la page du carnet.
document.addEventListener("dragover", (e) => {
  if (!carriesFiles(e)) return;
  e.preventDefault();
  if (!dropAllowed(e) && !e.target.closest?.("#f-body")) e.dataTransfer.dropEffect = "none";
});
document.addEventListener("drop", (e) => {
  dropDepth = 0;
  showDrop(false);
  if (!carriesFiles(e) || e.defaultPrevented) return;
  e.preventDefault();
  if (dropAllowed(e)) importFiles(e.dataTransfer.files, importTarget());
});
addEventListener("dragend", () => {
  dropDepth = 0;
  showDrop(false);
});

// ── Cycle de vie : présence, fraîcheur, fermeture ─────────────────────────────

function heartbeat() {
  fetch("/api/heartbeat", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" }).then(
    (res) => {
      res.arrayBuffer().catch(() => {}); // réponse lue jusqu'au bout : la requête est close
      setOffline(false);
    },
    () => setOffline(true),
  );
}

let refreshing = false;
/** Le disque a changé (autre fenêtre, Obsidian, synchro) : on ne redessine que si la vue est concernée. */
async function refresh() {
  if (refreshing || !S.state) return;
  refreshing = true;
  try {
    if (!(await loadState())) return;
    const r = S.route;
    const v = r.view;
    if (["edit", "editMap", "new", "newMap"].includes(v)) {
      renderChrome();
      if (v === "edit" || v === "editMap") await S.editor?.checkRemote();
      return;
    }
    if (v === "search") {
      renderChrome();
      await runSearch(r.q);
      return;
    }
    if (v === "note") {
      const res = await cached(`/api/notes/${enc(r.map)}/${enc(r.id)}`).catch(() => null);
      if (stale(r)) return;
      if (res && S.note && res.data.version === S.note.version) return renderChrome();
    }
    if (v === "map" || v === "sheet") {
      const res = await cached(`/api/maps/${enc(r.map)}`).catch(() => null);
      if (stale(r)) return;
      if (res && JSON.stringify(res.data) === S.sig) {
        S.mapData = res.data;
        return renderChrome();
      }
    }
    // Un menu ouvert ou une boîte de copie : on ne redessine pas sous les doigts.
    if (stale(r) || $(".menu:popover-open") || $("dialog[open]")) return;
    await keepFocus(() => render({ keepScroll: true }));
  } catch {
  } finally {
    refreshing = false;
  }
}

addEventListener("hashchange", () => navigate());
addEventListener("focus", () => refresh());
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    heartbeat();
    refresh();
  }
});
addEventListener("pagehide", (e) => {
  S.editor?.flushKeepalive();
  // Mise en cache pour un retour arrière : la fenêtre n'est pas fermée, on ne dit pas au revoir.
  if (!e.persisted) navigator.sendBeacon("/api/bye");
});
addEventListener("pageshow", (e) => {
  if (e.persisted) heartbeat();
});
setInterval(heartbeat, 15000);
setInterval(() => {
  if (!document.hidden) refresh();
}, 5000);

(async function init() {
  try {
    await loadState(true);
  } catch (e) {
    page.innerHTML = `<p class="form-error">${esc(cap(e.message))}</p>`;
    return;
  }
  S.route = parse(location.hash);
  await render();
  heartbeat();
})();
