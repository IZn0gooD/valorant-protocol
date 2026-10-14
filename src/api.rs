//! L'application : routes, sécurité des requêtes et réponses JSON.
//!
//! Modèle de sécurité (serveur local) :
//! - en-tête `Host` vérifié : un site tiers ne peut pas viser le carnet par
//!   « DNS rebinding » ;
//! - toute modification exige une requête venue de ce poste (boucle locale)
//!   et de la même origine (ou d'une origine autorisée par `--cors`) ;
//! - les corps JSON / Markdown / image ne sont pas des types « simples » : un
//!   formulaire d'un autre site ne peut pas les envoyer sans pré-vérification ;
//! - en partage réseau (`--lan`), un autre poste doit présenter le jeton du lien
//!   d'invitation, et ne fait que lire ;
//! - le Markdown est rendu sans HTML actif (voir `render`), et la page impose
//!   une CSP stricte.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::assets;
use crate::http::{Request, Response};
use crate::i18n::{self, Lang};
use crate::json::{self, Json, obj};
use crate::note;
use crate::platform;
use crate::render;
use crate::search;
use crate::share::{self, Part};
use crate::tr;
use crate::store::{self, MapInfo, Note, NoteInput, Store, StoreError};
use crate::util::{self, escape_html, iso_utc, percent_decode, unix_now};

pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob: https:; \
font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const LAN_COOKIE: &str = "protocol_lan";

pub struct Config {
    /// Ouvert au réseau local (en lecture seule pour les autres postes).
    pub lan: bool,
    /// Le jeton du lien d'invitation, exigé des autres postes en mode réseau.
    pub lan_token: Option<String>,
    /// Origines autorisées à appeler l'API depuis un navigateur (`http://localhost:5173`).
    pub cors: Vec<String>,
    /// S'arrêter seul quand plus aucune fenêtre n'est ouverte.
    pub auto_exit: bool,
}

pub struct App {
    store: Mutex<Store>,
    /// Le fichier du carnet ne change jamais : le lire ne demande pas le verrou.
    carnet: PathBuf,
    pub cfg: Config,
    pub port: AtomicU16,
    idle_ticks: AtomicU32,
    bye: AtomicBool,
    quit: AtomicBool,
}

/// Le veilleur passe toutes les 250 ms. Il ne compte que le temps où la machine
/// est éveillée : une mise en veille ne fait pas fermer le carnet.
pub const TICK_MS: u64 = 250;
const IDLE_TICKS: u32 = 150_000 / TICK_MS as u32; // 2 min 30 sans signe de vie
const BYE_TICKS: u32 = 20_000 / TICK_MS as u32; // 20 s après la fermeture d'une fenêtre (d'autres onglets ont le temps de se signaler)

impl App {
    pub fn new(store: Store, cfg: Config) -> App {
        App {
            carnet: store.path().to_path_buf(),
            store: Mutex::new(store),
            cfg,
            port: AtomicU16::new(0),
            idle_ticks: AtomicU32::new(0),
            bye: AtomicBool::new(false),
            quit: AtomicBool::new(false),
        }
    }

    /// Le carnet. Un thread qui a paniqué ne bloque pas les autres : chaque écriture
    /// est complète ou absente, l'état reste cohérent.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn carnet(&self) -> &Path {
        &self.carnet
    }

    /// La langue retenue : celle du réglage du carnet, sinon celle du système.
    pub fn lang(&self) -> Lang {
        self.store().lang()
    }

    /// Comme `lang`, sans attendre : carnet occupé (export en cours…), la langue du
    /// système. Pour les refus envoyés par la boucle d'acceptation, qui ne doit pas bloquer.
    pub fn lang_now(&self) -> Lang {
        match self.store.try_lock() {
            Ok(s) => s.lang(),
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().lang(),
            Err(std::sync::TryLockError::WouldBlock) => i18n::resolve(None),
        }
    }

    /// Demande l'arrêt (fenêtre fermée, bouton « Quitter »).
    pub fn request_quit(&self) {
        self.quit.store(true, Ordering::Relaxed);
    }

    fn touch(&self) {
        self.idle_ticks.store(0, Ordering::Relaxed);
        self.bye.store(false, Ordering::Relaxed);
    }

    /// Appelé par le veilleur : `true` s'il est temps de s'arrêter. Termine au
    /// passage un compactage du carnet fini en arrière-plan.
    pub fn tick(&self) -> bool {
        if self.quit.load(Ordering::Relaxed) {
            return true;
        }
        if let Ok(mut s) = self.store.try_lock() {
            s.poll();
        }
        if !self.cfg.auto_exit {
            return false;
        }
        let t = self.idle_ticks.fetch_add(1, Ordering::Relaxed) + 1;
        (self.bye.load(Ordering::Relaxed) && t >= BYE_TICKS) || t >= IDLE_TICKS
    }

    /// Quitte proprement : on attend qu'aucune écriture ne soit en cours, puis la
    /// fenêtre et son profil de navigateur temporaire disparaissent.
    pub fn exit(&self) -> ! {
        let guard = self.store();
        drop(guard);
        platform::end_session();
        std::process::exit(0)
    }
}

/// Taille de corps admise selon la route, lue avant le corps. Un autre poste ne
/// modifie rien : il n'a rien à envoyer.
pub fn body_limit(method: &str, path: &str, loopback: bool) -> usize {
    match (method, path) {
        ("GET" | "HEAD" | "OPTIONS", _) => 1024,
        _ if !loopback => 0,
        ("POST", "/api/assets") => 16 << 20,
        _ => 2 << 20,
    }
}

pub fn handle(app: &App, req: Request) -> Response {
    let mut res = route(app, &req);
    // Les messages d'erreur sont écrits en français : traduits ici, à la sortie, une fois
    // le carnet relâché (la langue se lit sous son verrou).
    if res.status >= 400 && res.headers.iter().any(|(k, v)| *k == "Content-Type" && v.starts_with("application/json")) {
        res = localize_error(res, app.lang());
    }
    for (k, v) in [
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("X-Frame-Options", "DENY"),
        ("Cross-Origin-Resource-Policy", "same-origin"),
        ("Cache-Control", "no-store"),
    ] {
        if !res.has_header(k) {
            res = res.header(k, v);
        }
    }
    if let Some(origin) = cors_origin(app, &req) {
        res = res.header("Access-Control-Allow-Origin", origin).header("Vary", "Origin").header("Access-Control-Expose-Headers", "ETag");
    }
    res
}

/// Le champ `error` d'une réponse JSON, dans la langue `lang` (le reste est intact).
fn localize_error(mut res: Response, lang: Lang) -> Response {
    if lang == Lang::Fr {
        return res;
    }
    let Ok(Json::Obj(mut pairs)) = std::str::from_utf8(&res.body).map_err(|e| e.to_string()).and_then(json::parse) else {
        return res;
    };
    let mut changed = false;
    for (k, v) in pairs.iter_mut() {
        if let (true, Json::Str(msg)) = (k == "error", &*v) {
            let en = i18n::localize(lang, msg);
            changed |= en != *msg;
            *v = Json::Str(en);
        }
    }
    if changed {
        res.body = Json::Obj(pairs).to_json().into_bytes().into();
    }
    res
}

fn host_ok(app: &App, req: &Request) -> bool {
    let Some(host) = req.header("host") else { return false };
    let host = host.to_ascii_lowercase();
    // Sans port explicite, c'est le port 80 (les navigateurs l'omettent).
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) if !n.is_empty() && !n.ends_with(':') && !p.contains(']') => (n, p),
        _ => (host.as_str(), "80"),
    };
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) || port.parse::<u16>().ok() != Some(app.port.load(Ordering::Relaxed)) {
        return false;
    }
    match name {
        "127.0.0.1" | "localhost" | "[::1]" => true,
        // Sur le réseau local : des adresses IP seulement, jamais un nom (anti « rebinding »).
        _ if app.cfg.lan => {
            name.parse::<std::net::Ipv4Addr>().is_ok()
                || name.strip_prefix('[').and_then(|n| n.strip_suffix(']')).is_some_and(|n| n.parse::<std::net::Ipv6Addr>().is_ok())
        }
        _ => false,
    }
}

fn cors_origin(app: &App, req: &Request) -> Option<String> {
    let o = req.header("origin")?.to_ascii_lowercase();
    app.cfg.cors.contains(&o).then_some(o)
}

fn write_allowed(app: &App, req: &Request) -> Result<(), Response> {
    if !req.from_loopback() {
        return Err(Response::error(403, "les modifications se font depuis le poste du coach"));
    }
    match req.header("origin") {
        Some(o) => {
            let o = o.to_ascii_lowercase();
            let own = format!("http://{}", req.header("host").unwrap_or("").to_ascii_lowercase());
            if o != own && !app.cfg.cors.contains(&o) {
                return Err(Response::error(403, "origine refusée"));
            }
        }
        // Pas d'origine : un outil en ligne de commande, ou une navigation directe.
        None if req.header("sec-fetch-site").is_some_and(|s| s != "same-origin" && s != "none") => {
            return Err(Response::error(403, "origine refusée"));
        }
        None => {}
    }
    Ok(())
}

fn cookie(req: &Request, name: &str) -> Option<String> {
    req.header("cookie")?.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// Un autre poste du réseau : il doit présenter le jeton du lien d'invitation,
/// dans l'adresse (`?t=…`, la première fois) ou dans le cookie posé alors.
fn lan_guest(app: &App, req: &Request) -> Result<Option<Response>, Response> {
    if req.from_loopback() {
        return Ok(None);
    }
    let Some(token) = app.cfg.lan_token.as_deref() else {
        return Err(Response::error(403, "carnet non partagé"));
    };
    if cookie(req, LAN_COOKIE).is_some_and(|c| util::same_secret(&c, token)) {
        return Ok(None);
    }
    if req.param("t").is_some_and(|t| util::same_secret(&t, token)) {
        // On retire le jeton de l'adresse (historique, captures d'écran) et on le garde en cookie.
        let res = Response::empty(302)
            .header("Location", req.path.clone())
            .header("Set-Cookie", format!("{LAN_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"));
        return Ok(Some(res));
    }
    let text = tr!(
        app.lang(),
        "protocol : ouvrez le lien d'invitation donné par le coach (il contient ?t=…).",
        "protocol: open the invitation link the coach gave you (it contains ?t=…)."
    );
    Err(Response::new(403, "text/plain; charset=utf-8", text))
}

fn route(app: &App, req: &Request) -> Response {
    if !host_ok(app, req) {
        return Response::error(421, "hôte non autorisé");
    }
    match lan_guest(app, req) {
        Ok(Some(redirect)) => return redirect,
        Ok(None) => {}
        Err(refused) => return refused,
    }
    if req.method == "OPTIONS" {
        return match cors_origin(app, req) {
            Some(o) => Response::empty(204)
                .header("Access-Control-Allow-Origin", o)
                .header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE")
                .header("Access-Control-Allow-Headers", "Content-Type, If-Match, If-None-Match")
                .header("Access-Control-Max-Age", "600")
                .header("Vary", "Origin"),
            None => Response::error(403, "origine refusée"),
        };
    }
    let read = matches!(req.method.as_str(), "GET" | "HEAD");
    if !read && let Err(r) = write_allowed(app, req) {
        return r;
    }
    // Un simple ping (autre instance, script) n'est pas une fenêtre ouverte.
    if req.path != "/api/bye" && req.path != "/api/ping" {
        app.touch();
    }
    let segs: Vec<&str> = req.path[1..].split('/').collect();
    match segs.as_slice() {
        ["api", rest @ ..] => api(app, req, rest),
        ["files", rest @ ..] if read => files(app, req, rest),
        _ if read => static_asset(app, req),
        _ => Response::error(405, "méthode non autorisée"),
    }
}

fn etag_match(req: &Request, etag: &str) -> bool {
    req.header("if-none-match").is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
}

/// Le thème choisi, enregistré dans le carnet (le navigateur, lui, ne garde rien).
fn theme(app: &App) -> Option<&'static str> {
    match app.store().setting(store::SETTING_THEME) {
        Some("dark") => Some("dark"),
        Some("light") => Some("light"),
        _ => None,
    }
}

fn static_asset(app: &App, req: &Request) -> Response {
    let path = if req.path == "/index.html" { "/" } else { req.path.as_str() };
    let Some((a, etag)) = assets::find(path) else {
        return Response::error(404, "introuvable");
    };
    if !a.mime.starts_with("text/html") {
        if etag_match(req, etag) {
            return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
        }
        return Response::embedded(200, a.mime, a.body).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    // La page porte le thème enregistré dès son premier affichage : pas d'éclair blanc.
    // Et sa langue : l'interface lit `lang` sur la racine du document.
    let theme = theme(app);
    let lang = app.lang().code();
    let etag = format!("{}-{}-{lang}\"", etag.trim_end_matches('"'), theme.unwrap_or("auto"));
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let html = String::from_utf8_lossy(a.body);
    let root = match theme {
        Some(t) => format!("<html lang=\"{lang}\" data-theme=\"{t}\">"),
        None => format!("<html lang=\"{lang}\">"),
    };
    let html = html.replacen("<html lang=\"fr\">", &root, 1);
    Response::new(200, a.mime, html).header("ETag", etag).header("Cache-Control", "no-cache").header("Content-Security-Policy", CSP)
}

/// Une image du carnet. Son nom vient de son empreinte : il ne change jamais de contenu.
fn files(app: &App, req: &Request, rest: &[&str]) -> Response {
    let rel = rest.join("/");
    let etag = format!("\"f{}\"", util::hex64(util::fnv1a64(rel.as_bytes())));
    let Some((bytes, mime)) = app.store().asset(&rel) else {
        return Response::error(404, "image introuvable");
    };
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let mut res = Response::new(200, mime, bytes).header("ETag", etag).header("Cache-Control", "no-cache");
    if mime == "image/svg+xml" {
        res = res.header("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; sandbox");
    }
    res
}

// ── JSON ──────────────────────────────────────────────────────────────────────

pub fn note_meta(n: &Note) -> Json {
    obj([
        ("map", n.map.as_str().into()),
        ("id", n.id.as_str().into()),
        ("title", n.title.as_str().into()),
        ("kind", n.kind().into()),
        ("side", n.side().into()),
        ("tags", n.doc.fm.tags.as_slice().into()),
        ("pin", n.doc.fm.pin.into()),
        ("updated", iso_utc(n.updated_ts).into()),
        ("excerpt", n.excerpt.as_str().into()),
        ("preview", n.preview.as_str().into()),
        ("version", n.version.as_str().into()),
    ])
}

pub fn note_full(n: &Note) -> Json {
    let mut j = note_meta(n);
    j.push("body", n.doc.body.as_str().into());
    // L'aperçu sans le `# Titre` d'ouverture qui sert de titre à la note : la page l'affiche déjà.
    j.push("html", render::to_html(n.shown_body(), &n.map).into());
    j
}

fn map_json(m: &MapInfo, full: bool) -> Json {
    let mut j = obj([
        ("id", m.id.as_str().into()),
        ("title", m.title.as_str().into()),
        ("pool", m.pool.into()),
        ("order", m.order.into()),
        ("count", m.count.into()),
        ("pinned", m.pinned.into()),
        ("updated", (m.updated_ts > 0).then(|| iso_utc(m.updated_ts)).into()),
        ("team", m.is_team().into()),
    ]);
    if full {
        j.push("overview", m.doc.body.as_str().into());
        j.push("overviewHtml", render::to_html(&m.doc.body, &m.id).into());
        j.push("version", m.version.as_str().into());
    }
    j
}

fn store_error(e: StoreError) -> Response {
    let msg = e.to_string();
    match e {
        StoreError::NotFound(_) => Response::error(404, &msg),
        StoreError::Invalid(_) => Response::error(400, &msg),
        StoreError::ReadOnly => Response::error(403, &msg),
        StoreError::Conflict(cur) => Response::json(409, &obj([("error", msg.into()), ("current", note_full(&cur))])),
        StoreError::MapConflict(cur) => Response::json(409, &obj([("error", msg.into()), ("current", map_json(&cur, true))])),
        StoreError::Io(_) => {
            eprintln!("protocol : {msg}");
            Response::error(500, &msg)
        }
    }
}

fn json_body(req: &Request) -> Result<Json, Response> {
    if req.content_type() != "application/json" {
        return Err(Response::error(415, "corps JSON attendu (Content-Type: application/json)"));
    }
    let text = req.body_str().ok_or_else(|| Response::error(400, "corps illisible (UTF-8 attendu)"))?;
    json::parse(text).map_err(|e| Response::error(400, &e))
}

fn note_input(v: &Json) -> NoteInput {
    let s = |k: &str| v.str_field(k).map(str::to_string);
    let tags = match v.get("tags") {
        Some(Json::Arr(a)) => Some(a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()),
        Some(Json::Str(s)) => Some(s.split(',').map(|t| t.trim().to_string()).collect()),
        _ => None,
    };
    NoteInput {
        title: s("title"),
        kind: s("kind").or_else(|| s("type")),
        side: s("side"),
        tags,
        pin: v.get("pin").and_then(Json::as_bool),
        body: s("body"),
        map: s("map"),
    }
}

/// La version de référence d'une écriture : `baseVersion` ou `If-Match`, sauf `force`.
fn base_version(req: &Request, v: Option<&Json>) -> Option<String> {
    if v.and_then(|v| v.get("force")).and_then(Json::as_bool) == Some(true) {
        return None;
    }
    v.and_then(|v| v.str_field("baseVersion"))
        .map(str::to_string)
        .or_else(|| req.header("if-match").map(|h| h.trim_start_matches("W/").trim_matches('"').to_string()))
        .filter(|b| !b.is_empty() && b != "*")
}

fn ok() -> Response {
    Response::json(200, &obj([("ok", true.into())]))
}

// ── Routes ────────────────────────────────────────────────────────────────────

fn api(app: &App, req: &Request, rest: &[&str]) -> Response {
    let dec = |s: &str| percent_decode(s, false).unwrap_or_default();
    let local = req.from_loopback();
    match (req.method.as_str(), rest) {
        ("GET" | "HEAD", ["ping"]) => ping(app, req),
        ("GET" | "HEAD", ["state"]) => state(app, req),
        ("GET" | "HEAD", ["maps", map]) => map_detail(app, req, &dec(map)),
        ("POST", ["maps"]) => create_map(app, req),
        ("PUT", ["maps", map]) => save_map(app, req, &dec(map)),
        ("DELETE", ["maps", map]) => app.store().delete_map(&dec(map)).map_or_else(store_error, |_| ok()),
        ("GET" | "HEAD", ["notes"]) => list_notes(app, req),
        ("POST", ["notes", map]) => create_note(app, req, &dec(map)),
        ("GET" | "HEAD", ["notes", map, id]) => get_note(app, req, &dec(map), &dec(id)),
        ("PUT", ["notes", map, id]) => save_note(app, req, &dec(map), &dec(id)),
        ("DELETE", ["notes", map, id]) => match app.store().trash_note(&dec(map), &dec(id)) {
            Ok(path) => Response::json(200, &obj([("trash", path.into())])),
            Err(e) => store_error(e),
        },
        ("GET" | "HEAD", ["search"]) => search_api(app, req),
        ("POST", ["render"]) => render_api(req),
        ("POST", ["assets"]) => upload(app, req),
        // La corbeille reste l'affaire du coach, même pour un invité du réseau.
        ("GET" | "HEAD", ["trash"]) if local => trash(app),
        ("POST", ["trash", "restore"]) => restore(app, req),
        ("GET" | "HEAD", ["export.json"]) => export_json(app),
        ("GET" | "HEAD", ["export.zip"]) if local => export_zip(app),
        ("GET" | "HEAD", ["export", file]) if file.ends_with(".md") => export_md(app, &dec(file.trim_end_matches(".md"))),
        ("GET" | "HEAD", ["share", map]) => share_map_api(app, req, &dec(map)),
        ("GET" | "HEAD", ["share", map, id]) => share_note_api(app, req, &dec(map), &dec(id)),
        ("POST", ["reveal"]) => reveal(app),
        ("POST", ["settings"]) => settings(app, req),
        ("DELETE", ["notice"]) => app.store().clear_notice().map_or_else(store_error, |_| ok()),
        ("POST", ["heartbeat"]) => ok(),
        ("POST", ["bye"]) => {
            app.bye.store(true, Ordering::Relaxed);
            app.idle_ticks.store(0, Ordering::Relaxed);
            ok()
        }
        ("POST", ["quit"]) => {
            app.quit.store(true, Ordering::Relaxed);
            ok()
        }
        ("GET" | "HEAD", ["trash"]) => Response::error(403, "la corbeille se consulte sur le poste du coach"),
        ("GET" | "HEAD", ["export.zip"]) => Response::error(403, "le carnet entier s'exporte depuis le poste du coach"),
        (
            _,
            [
                "ping" | "state" | "maps" | "notes" | "search" | "render" | "assets" | "trash" | "export.json" | "export.zip" | "export"
                | "share" | "reveal" | "settings" | "notice" | "heartbeat" | "bye" | "quit",
                ..,
            ],
        ) => Response::error(405, "méthode non autorisée"),
        _ => Response::error(404, "route inconnue"),
    }
}

fn ping(app: &App, req: &Request) -> Response {
    let mut j = obj([("app", "protocol".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    if req.from_loopback() {
        j.push("carnet", platform::display_path(app.carnet()).into());
    }
    Response::json(200, &j)
}

fn state(app: &App, req: &Request) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let local = req.from_loopback();
    let etag = format!("W/\"s{}-{}-{}\"", s.boot, s.generation, local as u8);
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let mut recent: Vec<&Arc<Note>> = s.notes().iter().collect();
    recent.sort_by_key(|n| std::cmp::Reverse(n.updated_ts));
    recent.truncate(10);
    let lang_setting = s.setting(i18n::SETTING).unwrap_or("").to_string();
    let lang = i18n::resolve(Some(&lang_setting));
    let kinds: Vec<Json> = note::KINDS
        .iter()
        .map(|k| {
            obj([
                ("id", k.id.into()),
                ("label", k.label(lang).into()),
                ("plural", k.plural(lang).into()),
                ("template", k.template(lang).into()),
            ])
        })
        .collect();
    let mut j = obj([
        ("app", "protocol".into()),
        ("version", env!("CARGO_PKG_VERSION").into()),
        // La langue retenue, et le réglage d'où elle vient (vide : automatique).
        ("lang", lang.code().into()),
        ("langSetting", lang_setting.as_str().into()),
        ("generation", s.generation.into()),
        ("readOnly", (s.read_only || !local).into()),
        ("lan", app.cfg.lan.into()),
        ("local", local.into()),
        ("maps", s.maps().iter().map(|m| map_json(m, false)).collect::<Vec<_>>().into()),
        ("kinds", kinds.into()),
        (
            "sides",
            ["atk", "def"].map(|id| obj([("id", id.into()), ("label", side_label(id, lang).unwrap_or(id).into())])).to_vec().into(),
        ),
        ("pinned", s.notes().iter().filter(|n| n.doc.fm.pin).take(60).map(|n| note_meta(n)).collect::<Vec<_>>().into()),
        ("recent", recent.into_iter().map(|n| note_meta(n)).collect::<Vec<_>>().into()),
        ("total", s.notes().len().into()),
        // Les maps de jeu : la map de l'équipe (`général`) n'en est pas une.
        ("mapCount", s.maps().iter().filter(|m| !m.is_team()).count().into()),
        ("trash", if local { s.trash_count() } else { 0 }.into()),
    ]);
    if local {
        j.push("carnet", platform::display_path(s.path()).into());
        j.push("carnetSize", s.size().into());
        // L'avis est enregistré en français : montré dans la langue retenue.
        j.push("notice", s.notice().map(|n| i18n::localize(lang, n)).into());
        // Le programme lui-même : l'aide en tire la configuration MCP exacte de ce poste.
        if let Ok(exe) = std::env::current_exe() {
            j.push("exe", platform::display_path(&exe).into());
        }
    }
    Response::json(200, &j).header("ETag", etag).header("Cache-Control", "no-cache")
}

fn map_detail(app: &App, req: &Request, map: &str) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let Some(m) = s.map(map) else {
        return Response::error(404, &format!("map introuvable : {map}"));
    };
    let etag = format!("W/\"m{}-{}\"", s.boot, s.generation);
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let notes: Vec<Json> = s.notes_of(map).map(|n| note_meta(n)).collect();
    Response::json(200, &obj([("map", map_json(m, true)), ("notes", notes.into())]))
        .header("ETag", etag)
        .header("Cache-Control", "no-cache")
}

fn create_map(app: &App, req: &Request) -> Response {
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match app.store().create_map(v.str_field("title").unwrap_or("")) {
        Ok(m) => Response::json(201, &map_json(&m, true)),
        Err(e) => store_error(e),
    }
}

fn save_map(app: &App, req: &Request, map: &str) -> Response {
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let pool = match v.get("pool") {
        Some(Json::Bool(b)) => Some(Some(*b)),
        Some(Json::Null) => Some(None),
        _ => None,
    };
    let base = base_version(req, Some(&v));
    // `"backup": true` : une présentation réécrite laisse la précédente à la corbeille.
    if v.get("backup").and_then(Json::as_bool) == Some(true) {
        return match app.store().save_map_backed_up(map, v.str_field("title"), pool, v.str_field("overview"), base.as_deref()) {
            Ok((m, key)) => {
                let mut j = map_json(&m, true);
                j.push("backup", key.into());
                Response::json(200, &j)
            }
            Err(e) => store_error(e),
        };
    }
    match app.store().save_map(map, v.str_field("title"), pool, v.str_field("overview"), base.as_deref()) {
        Ok(m) => Response::json(200, &map_json(&m, true)),
        Err(e) => store_error(e),
    }
}

fn list_notes(app: &App, req: &Request) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let map = req.param("map");
    let notes: Vec<Json> = s.notes().iter().filter(|n| map.as_deref().is_none_or(|m| n.map == m)).map(|n| note_meta(n)).collect();
    Response::json(200, &obj([("notes", notes.into())]))
}

fn create_note(app: &App, req: &Request, map: &str) -> Response {
    // Un fichier Markdown importé tel quel (en-tête compris) ; `?name=` : le nom du fichier, titre de secours.
    if matches!(req.content_type().as_str(), "text/markdown" | "text/x-markdown") {
        let Some(text) = req.body_str() else {
            return Response::error(400, "corps illisible (UTF-8 attendu)");
        };
        let hint = req.param("name");
        return match app.store().import_note(map, text, hint.as_deref()) {
            Ok(n) => Response::json(201, &note_full(&n)),
            Err(e) => store_error(e),
        };
    }
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match app.store().create_note(map, note_input(&v)) {
        Ok(n) => Response::json(201, &note_full(&n)),
        Err(e) => store_error(e),
    }
}

fn get_note(app: &App, req: &Request, map: &str, id: &str) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    // `[[Retake B à trois]]` désigne la note par son titre : on essaie aussi son nom simplifié.
    let Some(n) = s.note(map, id).or_else(|| s.note(map, &util::slugify(id))) else {
        return Response::error(404, &format!("note introuvable : {map}/{id}"));
    };
    drop(s);
    let etag = format!("\"{}\"", n.version);
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    Response::json(200, &note_full(&n)).header("ETag", etag).header("Cache-Control", "no-cache")
}

fn save_note(app: &App, req: &Request, map: &str, id: &str) -> Response {
    let result = match req.content_type().as_str() {
        "text/markdown" | "text/x-markdown" => {
            let Some(text) = req.body_str() else {
                return Response::error(400, "corps illisible (UTF-8 attendu)");
            };
            let base = base_version(req, None);
            app.store().put_raw(map, id, text, base.as_deref())
        }
        _ => {
            let v = match json_body(req) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let base = base_version(req, Some(&v));
            // `"backup": true` : la version d'avant va d'abord à la corbeille (écritures d'un assistant).
            if v.get("backup").and_then(Json::as_bool) == Some(true) {
                return match app.store().save_note_backed_up(map, id, note_input(&v), base.as_deref()) {
                    Ok((n, key)) => {
                        let mut j = note_full(&n);
                        j.push("backup", key.into());
                        Response::json(200, &j)
                    }
                    Err(e) => store_error(e),
                };
            }
            app.store().save_note(map, id, note_input(&v), base.as_deref())
        }
    };
    match result {
        Ok(n) => Response::json(200, &note_full(&n)),
        Err(e) => store_error(e),
    }
}

fn search_api(app: &App, req: &Request) -> Response {
    let q = req.param("q").unwrap_or_default();
    let limit = req.param("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(40).clamp(1, 200);
    let query = search::parse(&q);
    let mut s = app.store();
    let _ = s.refresh(false);
    let (total, hits) = if query.is_empty() { (0, Vec::new()) } else { search::search(s.notes(), s.maps(), &query, limit) };
    drop(s);
    let hits: Vec<Json> = hits
        .iter()
        .map(|h| {
            let mut j = note_meta(&h.note);
            j.push("score", h.score.into());
            j.push("snippet", h.snippet.as_str().into());
            j
        })
        .collect();
    Response::json(200, &obj([("q", q.into()), ("total", total.into()), ("hits", hits.into())]))
}

fn render_api(req: &Request) -> Response {
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let html = render::to_html(v.str_field("body").unwrap_or(""), v.str_field("map").unwrap_or(store::TEAM_MAP));
    Response::json(200, &obj([("html", html.into())]))
}

fn image_ext(content_type: &str, b: &[u8]) -> Option<&'static str> {
    let (ext, ok) = match content_type {
        "image/png" => ("png", b.starts_with(b"\x89PNG\r\n\x1a\n")),
        "image/jpeg" => ("jpg", b.starts_with(&[0xFF, 0xD8, 0xFF])),
        "image/gif" => ("gif", b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")),
        "image/webp" => ("webp", b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP"),
        "image/avif" => ("avif", b.len() > 12 && &b[4..8] == b"ftyp" && matches!(&b[8..12], b"avif" | b"avis")),
        "image/bmp" => ("bmp", b.starts_with(b"BM")),
        _ => return None,
    };
    ok.then_some(ext)
}

fn upload(app: &App, req: &Request) -> Response {
    let Some(ext) = image_ext(&req.content_type(), &req.body) else {
        return Response::error(415, "image PNG, JPEG, GIF, WebP, AVIF ou BMP attendue");
    };
    match app.store().save_asset(&req.body, ext) {
        Ok(name) => Response::json(
            201,
            &obj([
                ("name", name.as_str().into()),
                ("markdown", format!("../{}/{name}", store::ASSETS_DIR).into()),
                ("url", format!("/files/{}/{name}", store::ASSETS_DIR).into()),
            ]),
        ),
        Err(e) => store_error(e),
    }
}

fn trash(app: &App) -> Response {
    let items: Vec<Json> = app
        .store()
        .trash()
        .into_iter()
        .map(|t| {
            obj([
                ("path", t.path.into()),
                ("map", t.map.into()),
                ("id", t.id.into()),
                ("title", t.title.into()),
                ("deleted", (t.deleted_ts > 0).then(|| iso_utc(t.deleted_ts)).into()),
            ])
        })
        .collect();
    Response::json(200, &obj([("items", items.into())]))
}

fn restore(app: &App, req: &Request) -> Response {
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match app.store().restore(v.str_field("path").unwrap_or("")) {
        Ok((map, id)) => Response::json(200, &obj([("map", map.into()), ("id", id.into())])),
        Err(e) => store_error(e),
    }
}

/// Tout le carnet en un seul JSON : pour les autres outils (tableaux de bord, scripts…).
pub fn export_value(s: &Store) -> Json {
    let maps: Vec<Json> = s
        .maps()
        .iter()
        .map(|m| {
            let notes: Vec<Json> = s
                .notes_of(&m.id)
                .map(|n| {
                    obj([
                        ("id", n.id.as_str().into()),
                        ("title", n.title.as_str().into()),
                        ("kind", n.kind().into()),
                        ("side", n.side().into()),
                        ("tags", n.doc.fm.tags.as_slice().into()),
                        ("pin", n.doc.fm.pin.into()),
                        ("updated", iso_utc(n.updated_ts).into()),
                        ("body", n.doc.body.as_str().into()),
                    ])
                })
                .collect();
            obj([
                ("id", m.id.as_str().into()),
                ("title", m.title.as_str().into()),
                ("pool", m.pool.into()),
                ("overview", m.doc.body.as_str().into()),
                ("notes", notes.into()),
            ])
        })
        .collect();
    obj([
        ("app", "protocol".into()),
        ("version", env!("CARGO_PKG_VERSION").into()),
        ("exported", iso_utc(unix_now()).into()),
        ("maps", maps.into()),
    ])
}

fn export_json(app: &App) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let body = export_value(&s).to_json();
    drop(s);
    let day = &util::stamp_utc(unix_now())[..8];
    Response::new(200, "application/json; charset=utf-8", body)
        .header("Content-Disposition", format!("attachment; filename=\"protocol-{day}.json\""))
}

/// Les titres d'un texte Markdown descendus de `by` niveaux, hors blocs de code
/// clôturés et commentaires HTML. Un bloc ou un commentaire laissé ouvert en fin de
/// texte est refermé : mises bout à bout dans un export, les notes ne débordent pas
/// les unes sur les autres.
fn demote(body: &str, by: usize) -> String {
    let mut fence: Option<(char, usize)> = None;
    let mut comment = false;
    let mut out = Vec::new();
    for line in body.lines() {
        // Un commentaire HTML ouvert en tête de ligne court jusqu'à la ligne qui contient « --> ».
        if comment {
            comment = !line.contains("-->");
            out.push(line.to_string());
            continue;
        }
        if fence.is_none() {
            let t = line.trim_start();
            if line.len() - t.len() <= 3 && t.starts_with("<!--") && !t[4..].contains("-->") {
                comment = true;
                out.push(line.to_string());
                continue;
            }
        }
        if let Some((c, n)) = fence {
            if share::fence_close(line, c, n) {
                fence = None;
            }
            out.push(line.to_string());
            continue;
        }
        if let Some(f) = share::fence_open(line) {
            fence = Some(f);
            out.push(line.to_string());
            continue;
        }
        match share::heading(line) {
            Some((n, _)) => out.push(format!("{}{}", "#".repeat(by.min(6 - n)), line.trim_start())),
            None => out.push(line.to_string()),
        }
    }
    if let Some((c, n)) = fence {
        out.push(c.to_string().repeat(n));
    }
    if comment {
        out.push("-->".into());
    }
    out.join("\n")
}

/// Le nom d'un côté (`atk`, `def`) ; `None` pour les deux.
pub fn side_label(side: &str, lang: Lang) -> Option<&'static str> {
    match side {
        "atk" => Some(tr!(lang, "Attaque", "Attack")),
        "def" => Some(tr!(lang, "Défense", "Defense")),
        _ => None,
    }
}

/// « Retake · Défense · Ascent · #b #utilitaire » : les repères d'une note, en clair.
pub fn meta_line(n: &Note, map_title: Option<&str>, lang: Lang) -> String {
    let mut parts = vec![note::kind(n.kind()).map_or_else(|| n.kind().to_string(), |k| k.label(lang).to_string())];
    parts.extend(side_label(n.side(), lang).map(str::to_string));
    parts.extend(map_title.map(str::to_string));
    if !n.doc.fm.tags.is_empty() {
        parts.push(n.doc.fm.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "));
    }
    parts.join(" · ")
}

/// Les notes d'une map à exporter : toutes, ou les seules épinglées (la fiche match).
fn notes_for(s: &Store, map: &str, pinned_only: bool) -> Vec<Arc<Note>> {
    s.notes_of(map).filter(|n| !pinned_only || n.doc.fm.pin).cloned().collect()
}

/// Le titre de chaque note, par (map, id) : de quoi nommer les `[[liens]]` sans le carnet.
type Titles = HashMap<(String, String), String>;

fn titles(s: &Store) -> Titles {
    s.notes().iter().map(|n| ((n.map.clone(), n.id.clone()), n.title.clone())).collect()
}

/// Le titre de la note qu'un `[[lien]]` écrit dans `map` désigne, si elle existe.
fn wiki_title(titles: &Titles, map: &str, target: &str) -> Option<String> {
    let t = target.split('#').next()?.trim();
    let t = t.strip_suffix(".md").unwrap_or(t);
    let (m, id) = t.rsplit_once('/').unwrap_or((map, t));
    // `[[#section]]`, `[[]]` : aucune note (sinon `slugify("")` = « note » en désignerait une).
    if id.trim().is_empty() {
        return None;
    }
    let key = |id: String| (m.to_string(), id);
    titles.get(&key(id.to_string())).or_else(|| titles.get(&key(util::slugify(id)))).cloned()
}

fn html_head(title: &str, meta: &str, level: u8) -> String {
    let mut h = format!("<h{level}>");
    escape_html(title, &mut h);
    h.push_str(&format!("</h{level}>\n"));
    if !meta.is_empty() {
        h.push_str("<p><em>");
        escape_html(meta, &mut h);
        h.push_str("</em></p>\n");
    }
    h
}

/// Une note ou une map, prête à coller ailleurs, dans chaque format.
pub struct Shared {
    pub title: String,
    pub filename: String,
    /// Le Markdown que Discord sait afficher (tableaux alignés en bloc de code…).
    pub discord: String,
    /// Sans aucune marque.
    pub text: String,
    /// Le Markdown complet, en-tête compris, tel que le carnet l'écrit : pour un autre
    /// carnet ou Obsidian. Pour une map, l'export en un seul fichier.
    pub markdown: String,
    /// Mis en forme, pour Word, Google Docs, Notion.
    pub html: String,
}

impl Shared {
    fn json(&self) -> Json {
        obj([
            ("title", self.title.as_str().into()),
            ("filename", self.filename.as_str().into()),
            ("discord", self.discord.as_str().into()),
            ("text", self.text.as_str().into()),
            ("markdown", self.markdown.as_str().into()),
            ("html", self.html.as_str().into()),
        ])
    }
}

/// Une note et ce qu'il faut pour la partager, copiés sous le verrou du carnet :
/// la mise en forme se fait ensuite sans le tenir.
pub struct NoteExport {
    note: Arc<Note>,
    map_title: String,
    titles: Titles,
}

impl NoteExport {
    pub fn take(s: &Store, note: Arc<Note>) -> NoteExport {
        let map_title = s.map(&note.map).map_or_else(|| note.map.clone(), |m| m.title.clone());
        NoteExport { titles: titles(s), map_title, note }
    }

    /// La note dans chaque format, ses repères (type, côté) dans la langue `lang`.
    pub fn shared(&self, lang: Lang) -> Shared {
        let n = &self.note;
        let meta = meta_line(n, Some(&self.map_title), lang);
        let resolve = |t: &str| wiki_title(&self.titles, &n.map, t);
        let body = n.shown_body();
        Shared {
            title: n.title.clone(),
            filename: format!("{}.md", n.id),
            discord: share::discord(&n.title, &meta, body, &[], &resolve, lang),
            text: share::plain(&n.title, &meta, body, &[], &resolve, lang),
            markdown: n.text.clone(),
            html: html_head(&n.title, &meta, 1) + &render::to_html(body, &n.map),
        }
    }
}

/// Une map, ses notes et leurs titres, copiés sous le verrou du carnet.
pub struct MapExport {
    map: MapInfo,
    notes: Vec<Arc<Note>>,
    titles: Titles,
    pinned_only: bool,
}

impl MapExport {
    /// `pinned_only` : la fiche match, réduite aux notes épinglées.
    pub fn take(s: &Store, map: &str, pinned_only: bool) -> Option<MapExport> {
        Some(MapExport { map: s.map(map)?.clone(), notes: notes_for(s, map, pinned_only), titles: titles(s), pinned_only })
    }

    /// La map entière en un seul Markdown : à coller sur Discord, imprimer, archiver.
    pub fn markdown(&self, lang: Lang) -> String {
        let m = &self.map;
        let what = if self.pinned_only { tr!(lang, "fiche match", "match sheet") } else { "protocol" };
        let day = &iso_utc(unix_now())[..10];
        let exported = tr!(lang, "Exporté le", "Exported on");
        let mut out = format!("# {} — {what}\n\n_{exported} {day}._\n", m.title);
        if !m.doc.body.trim().is_empty() {
            out.push_str(tr!(lang, "\n## Présentation\n\n", "\n## Overview\n\n"));
            out.push_str(demote(m.doc.body.trim(), 1).as_str());
            out.push('\n');
        }
        for n in &self.notes {
            out.push_str(&format!("\n---\n\n## {}\n\n", n.title));
            let mut meta = meta_line(n, None, lang);
            if n.doc.fm.pin && !self.pinned_only {
                meta.push_str(tr!(lang, " · épinglée", " · pinned"));
            }
            out.push_str(&format!("*{}*\n\n", meta.replace('*', "\\*")));
            out.push_str(demote(n.shown_body().trim(), 1).as_str());
            out.push('\n');
        }
        out
    }

    /// La map dans chaque format : sa présentation, puis ses notes.
    pub fn shared(&self, lang: Lang) -> Shared {
        let m = &self.map;
        let map = m.id.as_str();
        let metas: Vec<String> = self.notes.iter().map(|n| meta_line(n, None, lang)).collect();
        let parts: Vec<Part> =
            self.notes.iter().zip(&metas).map(|(n, meta)| Part { title: &n.title, meta: meta.clone(), body: n.shown_body() }).collect();
        let count = self.notes.len();
        let (title, meta, filename) = if self.pinned_only {
            let meta = match (count, lang) {
                (0, _) => tr!(lang, "Aucune note épinglée", "No pinned notes").to_string(),
                (1, _) => tr!(lang, "1 note épinglée", "1 pinned note").to_string(),
                (c, Lang::Fr) => format!("{c} notes épinglées"),
                (c, Lang::En) => format!("{c} pinned notes"),
            };
            let (sheet, file) = (tr!(lang, "fiche match", "match sheet"), tr!(lang, "fiche", "sheet"));
            (format!("{} — {sheet}", m.title), meta, format!("protocol-{}-{file}.md", util::slugify(map)))
        } else {
            let meta = if count > 1 { format!("{count} notes") } else { format!("{count} note") };
            (m.title.clone(), meta, format!("protocol-{}.md", util::slugify(map)))
        };
        let resolve = |t: &str| wiki_title(&self.titles, map, t);
        let mut html = html_head(&title, &meta, 1) + &render::to_html(&m.doc.body, map);
        for (n, meta) in self.notes.iter().zip(&metas) {
            html.push_str(&html_head(&n.title, meta, 2));
            html.push_str(&render::to_html(&demote(n.shown_body(), 1), map));
        }
        Shared {
            discord: share::discord(&title, &meta, &m.doc.body, &parts, &resolve, lang),
            text: share::plain(&title, &meta, &m.doc.body, &parts, &resolve, lang),
            markdown: self.markdown(lang),
            title,
            filename,
            html,
        }
    }
}

/// Une map entière en un seul Markdown (`pinned_only` : la fiche match), dans la langue `lang`.
pub fn export_map_markdown(s: &Store, map: &str, pinned_only: bool, lang: Lang) -> Option<String> {
    MapExport::take(s, map, pinned_only).map(|e| e.markdown(lang))
}

/// Une map dans chaque format de partage (`pinned_only` : la fiche match), dans la langue `lang`.
pub fn share_map(s: &Store, map: &str, pinned_only: bool, lang: Lang) -> Option<Shared> {
    MapExport::take(s, map, pinned_only).map(|e| e.shared(lang))
}

fn export_md(app: &App, map: &str) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let (export, lang) = (MapExport::take(&s, map, false), s.lang());
    drop(s);
    match export {
        Some(e) => Response::new(200, "text/markdown; charset=utf-8", e.markdown(lang))
            .header("Content-Disposition", format!("attachment; filename=\"protocol-{}.md\"", util::slugify(map))),
        None => Response::error(404, &format!("map introuvable : {map}")),
    }
}

fn share_note_api(app: &App, req: &Request, map: &str, id: &str) -> Response {
    let mut s = app.store();
    let _ = s.refresh(false);
    let Some(n) = s.note(map, id).or_else(|| s.note(map, &util::slugify(id))) else {
        return Response::error(404, &format!("note introuvable : {map}/{id}"));
    };
    // La génération change à chaque écriture : titre de map ou de note lié compris.
    let etag = format!("W/\"x{}-{}\"", s.boot, s.generation);
    if etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let (export, lang) = (NoteExport::take(&s, n), s.lang());
    drop(s);
    Response::json(200, &export.shared(lang).json()).header("ETag", etag).header("Cache-Control", "no-cache")
}

fn share_map_api(app: &App, req: &Request, map: &str) -> Response {
    let pinned = req.param("scope").as_deref() == Some("pinned");
    let mut s = app.store();
    let _ = s.refresh(false);
    // Le Markdown d'une map porte sa date d'export : l'ETag change avec le jour.
    let day = &iso_utc(unix_now())[..10];
    let etag = format!("W/\"x{}-{}-{}-{day}\"", s.boot, s.generation, pinned as u8);
    if s.map(map).is_some() && etag_match(req, &etag) {
        return Response::empty(304).header("ETag", etag).header("Cache-Control", "no-cache");
    }
    let (export, lang) = (MapExport::take(&s, map, pinned), s.lang());
    drop(s);
    match export {
        Some(e) => Response::json(200, &e.shared(lang).json()).header("ETag", etag).header("Cache-Control", "no-cache"),
        None => Response::error(404, &format!("map introuvable : {map}")),
    }
}

/// Montre le fichier du carnet dans l'explorateur.
fn reveal(app: &App) -> Response {
    match platform::reveal(app.carnet()) {
        Ok(()) => ok(),
        Err(e) => Response::error(500, &format!("impossible d'ouvrir l'explorateur : {e}")),
    }
}

/// Tout le carnet en fichiers Markdown (archive ZIP) : pour Obsidian, ou une sauvegarde lisible partout.
fn export_zip(app: &App) -> Response {
    let day = &util::stamp_utc(unix_now())[..8];
    let top = format!("protocol-{day}");
    match app.store().export_zip(&top) {
        Ok(bytes) => Response::new(200, "application/zip", bytes).header("Content-Disposition", format!("attachment; filename=\"{top}.zip\"")),
        Err(e) => Response::error(500, &format!("export impossible : {e}")),
    }
}

/// Les réglages de l'interface, gardés dans le carnet : `{"theme": "dark", "lang": "en"}`.
fn settings(app: &App, req: &Request) -> Response {
    let v = match json_body(req) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Some(l) = v.str_field("lang") {
        let l = match l {
            "fr" | "en" => l,
            "" | "auto" => "",
            _ => return Response::error(400, "lang : fr, en ou auto"),
        };
        if let Err(e) = app.store().set_setting(i18n::SETTING, l) {
            return store_error(e);
        }
    }
    if let Some(t) = v.str_field("theme") {
        let t = match t {
            "dark" | "light" => t,
            "" | "auto" => "",
            _ => return Response::error(400, "thème : dark, light ou auto"),
        };
        if let Err(e) = app.store().set_setting(store::SETTING_THEME, t) {
            return store_error(e);
        }
    }
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_store;
    use std::fs;

    #[test]
    fn demote_skips_code() {
        assert_eq!(demote("# A\n```\n# pas un titre\n```\n## B\n#tag", 2), "### A\n```\n# pas un titre\n```\n#### B\n#tag");
        assert_eq!(demote("###### six", 2), "###### six");
        assert_eq!(demote("````\n```\n# dedans\n````\n# A", 1), "````\n```\n# dedans\n````\n## A");
        assert_eq!(demote("    # code indenté\n# A", 1), "    # code indenté\n## A");
        // Trois backticks suivis de texte sur la même ligne : du code en ligne, pas un bloc.
        let src = "```bind``` est la commande\n# Titre\n```\n# shell\n```";
        assert_eq!(demote(src, 1), "```bind``` est la commande\n## Titre\n```\n# shell\n```");
    }

    #[test]
    fn map_export_does_not_let_a_note_swallow_the_next() {
        for a in ["---\ntitle: A\n---\n```\ncl_crosshair 1\n", "---\ntitle: A\n---\nTexte\n\n<!-- TODO : finir\n"] {
            let (d, mut s) = temp_store("seal");
            s.import_note("ascent", a, None).unwrap();
            s.import_note("ascent", "---\ntitle: B\n---\nContenu de B\n", None).unwrap();
            let html = render::to_html(&export_map_markdown(&s, "ascent", false, Lang::Fr).unwrap(), "ascent");
            let discord = share_map(&s, "ascent", false, Lang::Fr).unwrap().discord;
            drop(s);
            let _ = fs::remove_dir_all(&d);
            assert!(html.contains("<h2>B</h2>") && html.contains("<p>Contenu de B</p>"), "{html}");
            assert!(discord.contains("\n## B\n"), "{discord}");
        }
    }

    #[test]
    fn exports_speak_the_requested_language() {
        let (d, mut s) = temp_store("lang");
        s.save_map("ascent", None, None, Some("Défauts lents"), None).unwrap();
        s.import_note("ascent", "---\ntitle: Retake B\ntype: retake\nside: def\npin: true\n---\n> [!TIP]\n> stack\n\n![plan](p.png)\n", None).unwrap();
        s.import_note("ascent", "---\ntitle: Exec A\ntype: execute\nside: atk\n---\nx\n", None).unwrap();
        let en = export_map_markdown(&s, "ascent", false, Lang::En).unwrap();
        let fr = export_map_markdown(&s, "ascent", false, Lang::Fr).unwrap();
        let sheet = share_map(&s, "ascent", true, Lang::En).unwrap();
        let empty = share_map(&s, "bind", true, Lang::En).unwrap();
        let note = NoteExport::take(&s, s.note("ascent", "retake-b").unwrap()).shared(Lang::En);
        drop(s);
        let _ = fs::remove_dir_all(&d);
        assert!(en.starts_with("# Ascent — protocol\n\n_Exported on ") && en.contains("\n## Overview\n"), "{en}");
        assert!(en.contains("*Retake · Defense · pinned*") && en.contains("*Execute · Attack*"), "{en}");
        assert!(fr.contains("_Exporté le ") && fr.contains("## Présentation") && fr.contains("*Retake · Défense · épinglée*"), "{fr}");
        assert_eq!((sheet.title.as_str(), sheet.filename.as_str()), ("Ascent — match sheet", "protocol-ascent-sheet.md"));
        assert!(sheet.discord.starts_with("# Ascent — match sheet\n*1 pinned note*"), "{}", sheet.discord);
        assert!(empty.text.contains("No pinned notes"), "{}", empty.text);
        assert!(note.discord.starts_with("# Retake B\n*Retake · Defense · Ascent*") && note.discord.contains("**✔ Tip**"), "{}", note.discord);
        assert!(note.text.contains("[image: plan]") && note.html.contains("<em>Retake · Defense · Ascent</em>"), "{}", note.text);
    }

    #[test]
    fn an_anchor_alone_names_no_note() {
        let (d, mut s) = temp_store("anchor");
        s.import_note("ascent", "---\ntitle: Note\n---\nx\n", None).unwrap();
        let t = titles(&s);
        let got = (wiki_title(&t, "ascent", "#calls"), wiki_title(&t, "ascent", ""), wiki_title(&t, "ascent", "note#x"));
        drop(s);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(got, (None, None, Some("Note".to_string())));
    }

    #[test]
    fn a_title_taken_from_the_text_is_not_shown_twice() {
        let (d, mut s) = temp_store("h1");
        // Textes venus d'ailleurs, sans en-tête : leur `# Titre` donne le titre de la note.
        for (id, text) in [("a", "# Retake B #\n\ncorps"), ("b", "\n#\tTitre **fort**\ncorps"), ("c", "  # Titre\ncorps")] {
            s.put_raw("ascent", id, text, None).unwrap();
        }
        let got: Vec<(String, String)> = ["a", "b", "c"]
            .iter()
            .map(|id| {
                let n = s.note("ascent", id).unwrap();
                (n.title.clone(), n.shown_body().to_string())
            })
            .collect();
        let shared = NoteExport::take(&s, s.note("ascent", "a").unwrap()).shared(Lang::Fr);
        drop(s);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(got[0], ("Retake B".to_string(), "corps".to_string()));
        assert_eq!(got[1], ("Titre fort".to_string(), "corps".to_string()));
        assert_eq!(got[2], ("Titre".to_string(), "corps".to_string()));
        assert!(!shared.html.contains("<h1>Retake B #</h1>") && shared.html.matches("Retake B").count() == 1, "{}", shared.html);
        assert!(shared.markdown.contains("# Retake B #"), "le fichier, lui, garde son titre");
    }

    #[test]
    fn image_magic() {
        assert_eq!(image_ext("image/png", b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_ext("image/png", b"<svg>"), None);
        assert_eq!(image_ext("image/svg+xml", b"<svg>"), None);
        assert_eq!(image_ext("image/webp", b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
    }

    fn request(peer: &str, method: &str, target: &str, headers: &[(&str, &str)]) -> Request {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect(),
            body: Vec::new(),
            peer: peer.parse().unwrap(),
        }
    }

    #[test]
    fn lan_guests_need_the_invitation_and_only_read() {
        let (d, s) = temp_store("lan");
        let cfg = Config { lan: true, lan_token: Some("a".repeat(32)), cors: Vec::new(), auto_exit: false };
        let app = App::new(s, cfg);
        app.port.store(7373, Ordering::Relaxed);
        let host = [("Host", "192.168.1.20:7373")];
        // Sans jeton : refusé, même pour lire.
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/state", &host)).status, 403);
        // Le lien d'invitation pose le cookie et retire le jeton de l'adresse.
        let invite = handle(&app, request("192.168.1.50:5000", "GET", &format!("/?t={}", "a".repeat(32)), &host));
        assert_eq!(invite.status, 302);
        assert!(invite.headers.iter().any(|(k, v)| *k == "Set-Cookie" && v.contains("HttpOnly")));
        let cookie = format!("{LAN_COOKIE}={}", "a".repeat(32));
        let with = [("Host", "192.168.1.20:7373"), ("Cookie", cookie.as_str())];
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/state", &with)).status, 200);
        // Un invité lit, mais n'écrit pas et ne voit pas la corbeille.
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/trash", &with)).status, 403);
        assert_eq!(handle(&app, request("192.168.1.50:5000", "DELETE", "/api/notes/general/bienvenue", &with)).status, 403);
        assert_eq!(body_limit("POST", "/api/assets", false), 0);
        // Un faux jeton, ou un nom d'hôte (rebinding), ne passent pas.
        let wrong = [("Host", "192.168.1.20:7373"), ("Cookie", "protocol_lan=bbbb")];
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/state", &wrong)).status, 403);
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/state", &[("Host", "evil.example:7373")])).status, 421);
        // Le carnet entier ne s'exporte pas vers un autre poste.
        assert_eq!(handle(&app, request("192.168.1.50:5000", "GET", "/api/export.zip", &with)).status, 403);
        // Le poste du coach, lui, n'a pas besoin du jeton.
        assert_eq!(handle(&app, request("127.0.0.1:5000", "GET", "/api/trash", &[("Host", "127.0.0.1:7373")])).status, 200);
        drop(app);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn host_header_is_strict() {
        let (d, s) = temp_store("host");
        let app = App::new(s, Config { lan: false, lan_token: None, cors: Vec::new(), auto_exit: false });
        app.port.store(7373, Ordering::Relaxed);
        let get = |host: &str| handle(&app, request("127.0.0.1:5000", "GET", "/api/ping", &[("Host", host)])).status;
        assert_eq!(get("127.0.0.1:7373"), 200);
        assert_eq!(get("localhost:7373"), 200);
        assert_eq!(get("[::1]:7373"), 200);
        assert_eq!(get("127.0.0.1:+7373"), 421);
        assert_eq!(get("127.0.0.1:7374"), 421);
        assert_eq!(get("127.0.0.1"), 421, "pas de port = 80");
        app.port.store(80, Ordering::Relaxed);
        assert_eq!(get("127.0.0.1"), 200);
        drop(app);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_theme_is_kept_in_the_carnet_and_set_on_the_page() {
        let (d, s) = temp_store("theme");
        let app = App::new(s, Config { lan: false, lan_token: None, cors: Vec::new(), auto_exit: false });
        app.port.store(7373, Ordering::Relaxed);
        let host = [("Host", "127.0.0.1:7373")];
        let page = |app: &App| handle(app, request("127.0.0.1:5000", "GET", "/", &host));
        let before = page(&app);
        assert!(String::from_utf8_lossy(&before.body).contains("<html lang=\"fr\">"));
        let mut req = request("127.0.0.1:5000", "POST", "/api/settings", &[("Host", "127.0.0.1:7373"), ("Content-Type", "application/json")]);
        req.body = br#"{"theme":"dark"}"#.to_vec();
        assert_eq!(handle(&app, req).status, 200);
        let after = page(&app);
        assert!(String::from_utf8_lossy(&after.body).contains("<html lang=\"fr\" data-theme=\"dark\">"));
        let etag = |r: &Response| r.headers.iter().find(|(k, _)| *k == "ETag").map(|(_, v)| v.clone());
        assert_ne!(etag(&before), etag(&after), "la page change avec le thème : pas de vieille copie");
        assert_eq!(app.store().setting(store::SETTING_THEME), Some("dark"));
        drop(app);
        let _ = fs::remove_dir_all(&d);
    }
}
