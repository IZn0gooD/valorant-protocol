//! Tests de bout en bout : un vrai serveur sur un port libre, de vraies requêtes HTTP.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use protocol::api::{App, Config};
use protocol::json::{self, Json};
use protocol::store::Store;

struct Res {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Res {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Json {
        json::parse(std::str::from_utf8(&self.body).expect("utf-8")).expect("json")
    }
}

struct Server {
    port: u16,
    dir: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("protocol-it-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn server_on(dir: PathBuf, cors: &[&str]) -> Server {
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.carnet"), false).unwrap();
    let cfg = Config { lan: false, lan_token: None, cors: cors.iter().map(|s| s.to_string()).collect(), auto_exit: false };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let s = protocol::start(listener, App::new(store, cfg)).unwrap();
    Server { port: s.port, dir }
}

fn server(cors: &[&str]) -> Server {
    server_on(temp_dir(), cors)
}

impl Server {
    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Res {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let mut head = format!("{method} {path} HTTP/1.1\r\n");
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            head.push_str(&format!("Host: 127.0.0.1:{}\r\n", self.port));
        }
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
        s.write_all(head.as_bytes()).unwrap();
        // Le serveur peut répondre (413…) avant d'avoir tout lu : on n'échoue pas sur l'envoi.
        let _ = s.write_all(body);
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("réponse sans en-têtes");
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        let headers = lines.filter_map(|l| l.split_once(": ").map(|(k, v)| (k.to_string(), v.to_string()))).collect();
        Res { status, headers, body: raw[split + 4..].to_vec() }
    }
    fn get(&self, path: &str) -> Res {
        self.send("GET", path, &[], b"")
    }
    fn json(&self, method: &str, path: &str, body: &str) -> Res {
        self.send(method, path, &[("Content-Type", "application/json")], body.as_bytes())
    }
}

#[test]
fn interface_is_served_with_strict_headers() {
    let s = server(&[]);
    let r = s.get("/");
    assert_eq!(r.status, 200);
    assert!(r.header("Content-Security-Policy").unwrap().contains("script-src 'self'"));
    assert_eq!(r.header("X-Frame-Options"), Some("DENY"));
    assert_eq!(r.header("X-Content-Type-Options"), Some("nosniff"));
    let etag = r.header("ETag").unwrap().to_string();
    let again = s.send("GET", "/", &[("If-None-Match", &etag)], b"");
    assert_eq!(again.status, 304);
    for asset in [
        "/static/app.js",
        "/static/app.css",
        "/static/vendor/mount.js",
        "/static/vendor/banner.js",
        "/static/pieces.js",
        "/static/clip.js",
        "/static/fonts/paper-mono.woff2",
        "/favicon.svg",
        "/static/licenses.txt",
    ] {
        assert_eq!(s.get(asset).status, 200, "{asset}");
    }
    assert_eq!(s.get("/static/nope.js").status, 404);
}

#[test]
fn foreign_hosts_and_origins_are_refused() {
    let s = server(&["http://localhost:5173"]);
    assert_eq!(s.send("GET", "/api/state", &[("Host", "evil.example:80")], b"").status, 421);
    assert_eq!(s.send("GET", "/api/state", &[("Host", &format!("127.0.0.1:{}", s.port + 1))], b"").status, 421);
    let evil = s.send(
        "POST",
        "/api/notes/ascent",
        &[("Content-Type", "application/json"), ("Origin", "https://evil.example")],
        b"{\"title\":\"x\"}",
    );
    assert_eq!(evil.status, 403);
    let cross_site = s.send(
        "POST",
        "/api/notes/ascent",
        &[("Content-Type", "application/json"), ("Sec-Fetch-Site", "cross-site")],
        b"{\"title\":\"x\"}",
    );
    assert_eq!(cross_site.status, 403);
    // Un formulaire d'un autre site ne peut envoyer que des types « simples ».
    assert_eq!(s.send("POST", "/api/notes/ascent", &[("Content-Type", "text/plain")], b"{\"title\":\"x\"}").status, 415);
    assert_eq!(s.send("POST", "/api/notes/ascent", &[("Content-Type", "application/x-www-form-urlencoded")], b"title=x").status, 415);
    // Une origine autorisée par --cors passe, avec les en-têtes CORS.
    let ok = s.send(
        "POST",
        "/api/notes/ascent",
        &[("Content-Type", "application/json"), ("Origin", "http://localhost:5173")],
        "{\"title\":\"Défaut A\"}".as_bytes(),
    );
    assert_eq!(ok.status, 201);
    assert_eq!(ok.header("Access-Control-Allow-Origin"), Some("http://localhost:5173"));
    let pre = s.send(
        "OPTIONS",
        "/api/notes/ascent/defaut-a",
        &[("Origin", "http://localhost:5173"), ("Access-Control-Request-Method", "PUT")],
        b"",
    );
    assert_eq!(pre.status, 204);
    assert!(pre.header("Access-Control-Allow-Methods").unwrap().contains("PUT"));
    assert_eq!(s.send("OPTIONS", "/api/state", &[("Origin", "https://evil.example")], b"").status, 403);
    // Même origine (l'interface elle-même) : autorisé.
    let own = format!("http://127.0.0.1:{}", s.port);
    assert_eq!(s.send("POST", "/api/heartbeat", &[("Content-Type", "application/json"), ("Origin", &own)], b"{}").status, 200);
}

#[test]
fn notes_round_trip_with_conflicts() {
    let s = server(&[]);
    let created =
        s.json("POST", "/api/notes/ascent", r###"{"title":"Retake B à trois","kind":"retake","side":"défense","tags":["b","#Retake"]}"###);
    assert_eq!(created.status, 201, "{}", String::from_utf8_lossy(&created.body));
    let n = created.json();
    assert_eq!(n.str_field("id"), Some("retake-b-a-trois"));
    assert_eq!(n.str_field("side"), Some("def"));
    assert!(n.str_field("body").unwrap().contains("## Conditions"), "modèle du type retake");
    // Le texte complet de la note, en-tête compris, tel que le carnet le garde.
    let text = |s: &Server| s.get("/api/share/ascent/retake-b-a-trois").json().str_field("markdown").unwrap().to_string();
    assert!(text(&s).starts_with("---\ntitle: Retake B à trois\ntype: retake\nside: def\ntags: [b, Retake]\n"), "{}", text(&s));

    let v1 = n.str_field("version").unwrap().to_string();
    let body = r###"{"body":"## Regroupement\n- au marché, fumée **heaven**\n\n| Joueur | Agent |\n|---|---|\n| Kai | Omen |\n","baseVersion":"@V@"}"###.replace("@V@", &v1);
    let saved = s.json("PUT", "/api/notes/ascent/retake-b-a-trois", &body);
    assert_eq!(saved.status, 200, "{}", String::from_utf8_lossy(&saved.body));
    let html = saved.json().str_field("html").unwrap().to_string();
    assert!(html.contains("<strong>heaven</strong>") && html.contains("<table>"), "{html}");

    // Une deuxième fenêtre restée sur l'ancienne version ne peut pas écraser.
    let stale = s.json("PUT", "/api/notes/ascent/retake-b-a-trois", &format!(r#"{{"body":"vieux","baseVersion":"{v1}"}}"#));
    assert_eq!(stale.status, 409);
    assert!(stale.json().get("current").unwrap().str_field("body").unwrap().contains("marché"));

    // If-Match marche aussi, et le Markdown brut est accepté tel quel.
    let v2 = s.get("/api/notes/ascent/retake-b-a-trois").json().str_field("version").unwrap().to_string();
    let raw = "---\ntitle: Retake B à trois\ntype: retake\naliases: [rb]\n---\n\nversion brute\n";
    let put = s.send(
        "PUT",
        "/api/notes/ascent/retake-b-a-trois",
        &[("Content-Type", "text/markdown"), ("If-Match", &format!("\"{v2}\""))],
        raw.as_bytes(),
    );
    assert_eq!(put.status, 200);
    assert_eq!(text(&s), raw, "le texte est gardé à l'identique");
    // Une mise à jour partielle garde la clé inconnue (aliases) de l'en-tête.
    let pin = s.json("PUT", "/api/notes/ascent/retake-b-a-trois", r#"{"pin":true}"#);
    assert_eq!(pin.status, 200);
    let t = text(&s);
    assert!(t.contains("pin: true") && t.contains("aliases: [rb]") && t.contains("version brute"), "{t}");

    // ETag de l'état : 304 tant que rien ne change.
    let st = s.get("/api/state");
    let tag = st.header("ETag").unwrap().to_string();
    assert_eq!(s.send("GET", "/api/state", &[("If-None-Match", &tag)], b"").status, 304);
    assert_eq!(st.json().get("pinned").unwrap().as_array().unwrap().len(), 2, "bienvenue + retake");
}

#[test]
fn search_trash_and_export() {
    let s = server(&[]);
    s.json("POST", "/api/notes/bind", r#"{"title":"Exécution A short","kind":"execute","body":"Fumée hookah, flash showers.\n"}"#);
    s.json("POST", "/api/notes/bind", r#"{"title":"Setup B","kind":"setup","side":"def","body":"Rien à voir.\n"}"#);
    let hits = s.get("/api/search?q=fumee+HOOKAH").json();
    assert_eq!(hits.get("total"), Some(&Json::Num(1.0)));
    let hit = &hits.get("hits").unwrap().as_array().unwrap()[0];
    assert_eq!(hit.str_field("id"), Some("execution-a-short"));
    assert!(hit.str_field("snippet").unwrap().contains("<mark>Fumée</mark>"));
    assert_eq!(s.get("/api/search?q=type%3Asetup+side%3Adef").json().get("total"), Some(&Json::Num(1.0)));

    let del = s.send("DELETE", "/api/notes/bind/setup-b", &[], b"");
    assert_eq!(del.status, 200);
    let path = del.json().str_field("trash").unwrap().to_string();
    assert_eq!(s.get("/api/notes/bind/setup-b").status, 404);
    assert_eq!(s.get("/api/trash").json().get("items").unwrap().as_array().unwrap().len(), 1);
    let back = s.json("POST", "/api/trash/restore", &format!(r#"{{"path":"{path}"}}"#)).json();
    assert_eq!(back.str_field("id"), Some("setup-b"));
    assert_eq!(s.get("/api/notes/bind/setup-b").status, 200);

    let all = s.get("/api/export.json");
    assert!(all.header("Content-Disposition").unwrap().starts_with("attachment"));
    let maps = all.json();
    let bind = maps.get("maps").unwrap().as_array().unwrap().iter().find(|m| m.str_field("id") == Some("bind")).unwrap().clone();
    assert_eq!(bind.get("notes").unwrap().as_array().unwrap().len(), 2);
    let md = String::from_utf8(s.get("/api/export/bind.md").body).unwrap();
    assert!(md.starts_with("# Bind — protocol") && md.contains("## Exécution A short") && md.contains("Fumée hookah"), "{md}");
}

#[test]
fn images_are_checked_and_served() {
    let s = server(&[]);
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR fake but well-formed enough";
    let up = s.send("POST", "/api/assets", &[("Content-Type", "image/png")], png);
    assert_eq!(up.status, 201);
    let url = up.json().str_field("url").unwrap().to_string();
    assert!(up.json().str_field("markdown").unwrap().starts_with("../assets/"));
    let img = s.get(&url);
    assert_eq!((img.status, img.header("Content-Type")), (200, Some("image/png")));
    assert_eq!(img.body, png);
    // Un faux PNG (en réalité du HTML) est refusé.
    assert_eq!(s.send("POST", "/api/assets", &[("Content-Type", "image/png")], b"<script>alert(1)</script>").status, 415);
    assert_eq!(s.send("POST", "/api/assets", &[("Content-Type", "image/svg+xml")], b"<svg/>").status, 415);
    // Les notes elles-mêmes ne sont pas servies comme fichiers, ni rien hors du carnet.
    for bad in ["/files/general/bienvenue.md", "/files/..%2F..%2FCargo.toml", "/files/.trash/x.png", "/files/assets/..%5C..%5Cx.png"] {
        assert_eq!(s.get(bad).status, 404, "{bad}");
    }
}

#[test]
fn bad_input_is_rejected_cleanly() {
    let s = server(&[]);
    assert_eq!(s.json("POST", "/api/notes/ascent", "{pas du json").status, 400);
    assert_eq!(s.json("POST", "/api/notes/..%2F..", r#"{"title":"x"}"#).status, 400);
    assert_eq!(s.json("POST", "/api/notes/nulle-part", r#"{"title":"x"}"#).status, 404);
    assert_eq!(s.json("PUT", "/api/notes/ascent/..%2F..%2Fx", r#"{"body":"x"}"#).status, 400);
    assert_eq!(s.send("POST", "/api/notes/ascent", &[("Content-Type", "application/json")], &[0xff, 0xfe]).status, 400);
    let huge = format!(r#"{{"title":"x","body":"{}"}}"#, "a".repeat(3 << 20));
    assert_eq!(s.json("POST", "/api/notes/ascent", &huge).status, 413);
    assert_eq!(s.send("PATCH", "/api/notes/ascent/x", &[], b"").status, 405);
    assert_eq!(s.get("/api/inconnu").status, 404);
    let rendered =
        s.json("POST", "/api/render", r#"{"map":"ascent","body":"<img src=x onerror=alert(1)> [a](javascript:alert(1))"}"#).json();
    let html = rendered.str_field("html").unwrap();
    assert!(!html.contains("<img") && html.contains("href=\"#\""), "{html}");
}

#[test]
fn maps_can_be_added_pooled_and_removed() {
    let s = server(&[]);
    let m = s.json("POST", "/api/maps", r#"{"title":"District"}"#);
    assert_eq!(m.status, 201);
    assert_eq!(s.json("POST", "/api/maps", r#"{"title":"district"}"#).status, 400);
    let pooled = s.json("PUT", "/api/maps/ascent", r###"{"pool":true,"overview":"## Défauts\n- 2 A, 1 mid, 2 B\n"}"###).json();
    assert_eq!(pooled.get("pool"), Some(&Json::Bool(true)));
    assert!(pooled.str_field("overviewHtml").unwrap().contains("<h2>Défauts</h2>"));
    let detail = s.get("/api/maps/ascent").json();
    assert_eq!(detail.get("map").unwrap().get("pool"), Some(&Json::Bool(true)));
    assert_eq!(s.send("DELETE", "/api/maps/district", &[], b"").status, 200);
    assert_eq!(s.get("/api/maps/district").status, 404);
    assert_eq!(s.send("DELETE", "/api/maps/general", &[], b"").status, 400, "une map avec des notes reste");
    // Présentation modifiée ailleurs depuis son ouverture : 409, avec la version du disque.
    let stale = s.json("PUT", "/api/maps/ascent", r#"{"overview":"vieux","baseVersion":"0000000000000000"}"#);
    assert_eq!(stale.status, 409);
    assert!(stale.json().get("current").unwrap().str_field("overview").unwrap().contains("Défauts"));
}

#[test]
fn a_restart_never_reuses_an_etag() {
    let dir = temp_dir();
    let first = server_on(dir.clone(), &[]);
    let tag1 = first.get("/api/state").header("ETag").unwrap().to_string();
    // Le même carnet rouvert (en lecture : le premier serveur le tient en écriture).
    let store = Store::open(&dir.join("test.carnet"), true).unwrap();
    let cfg = Config { lan: false, lan_token: None, cors: Vec::new(), auto_exit: false };
    let second = protocol::start(TcpListener::bind("127.0.0.1:0").unwrap(), App::new(store, cfg)).unwrap();
    let second = Server { port: second.port, dir: temp_dir() };
    let r = second.send("GET", "/api/state", &[("If-None-Match", &tag1)], b"");
    assert_eq!(r.status, 200, "un ETag de la session précédente ne doit pas valoir 304");
    assert_eq!(r.json().get("readOnly"), Some(&Json::Bool(true)));
    drop(first);
}

#[test]
fn the_whole_carnet_exports_as_markdown_files() {
    let s = server(&[]);
    assert_eq!(s.json("POST", "/api/notes/ascent", r#"{"title":"Retake B","kind":"retake"}"#).status, 201);
    let z = s.get("/api/export.zip");
    assert_eq!((z.status, z.header("Content-Type")), (200, Some("application/zip")));
    assert!(z.header("Content-Disposition").unwrap().contains(".zip"));
    assert_eq!(&z.body[..4], b"PK\x03\x04");
    let text = String::from_utf8_lossy(&z.body);
    assert!(text.contains("/ascent/retake-b.md") && text.contains("/general/bienvenue.md") && text.contains("title: Retake B"));
    // Le carnet entier, un seul fichier : Summit fait partie des maps, « général » ne compte pas.
    let st = s.get("/api/state").json();
    assert_eq!(st.get("mapCount"), Some(&Json::Num(13.0)));
    assert!(st.str_field("carnet").unwrap().ends_with("test.carnet"));
}

#[test]
fn markdown_files_import_and_notes_share_in_every_format() {
    let s = server(&[]);
    assert_eq!(s.json("POST", "/api/notes/ascent", r#"{"title":"Setup A","kind":"setup"}"#).status, 201);
    // Un fichier .md importé tel quel, en-tête compris ; jamais d'écrasement.
    let md = "---\ntitle: Retake B\ntype: retake\nside: def\ntags: [b]\n---\n## Rôles\n| Joueur | Agent |\n|---|---|\n| **Kai** | Sova |\n\n- [ ] stack [[setup-a]]\n";
    let a = s.send("POST", "/api/notes/ascent?name=fichier", &[("Content-Type", "text/markdown; charset=utf-8")], md.as_bytes());
    assert_eq!(a.status, 201);
    assert_eq!(a.json().str_field("id"), Some("retake-b"));
    let again = s.send("POST", "/api/notes/ascent", &[("Content-Type", "text/markdown")], md.as_bytes()).json();
    assert_eq!(again.str_field("id"), Some("retake-b-2"));
    let plain =
        s.send("POST", "/api/notes/ascent?name=Ex%C3%A9cution%20A", &[("Content-Type", "text/markdown")], "pas d'en-tête".as_bytes());
    assert_eq!(plain.json().str_field("title"), Some("Exécution A"));
    assert_eq!(s.send("POST", "/api/notes/ascent", &[("Content-Type", "text/plain")], b"x").status, 415);

    let r = s.get("/api/share/ascent/retake-b");
    assert_eq!(r.status, 200);
    let j = r.json();
    let discord = j.str_field("discord").unwrap();
    assert!(discord.starts_with("# Retake B\n*Retake · Défense · Ascent · #b*\n\n## Rôles\n```\nJoueur  Agent\n"), "{discord}");
    assert!(discord.contains("- ☐ stack Setup A"), "{discord}");
    let text = j.str_field("text").unwrap();
    assert!(text.contains("RÔLES") && text.contains("Kai     Sova") && text.contains("☐ stack Setup A") && !text.contains("**"), "{text}");
    assert!(j.str_field("markdown").unwrap().starts_with("---\ntitle: Retake B\n"));
    let html = j.str_field("html").unwrap();
    assert!(html.starts_with("<h1>Retake B</h1>\n<p><em>Retake · Défense · Ascent · #b</em></p>") && html.contains("<table>"), "{html}");
    assert_eq!(j.str_field("filename"), Some("retake-b.md"));
    let etag = r.header("ETag").unwrap().to_string();
    assert_eq!(s.send("GET", "/api/share/ascent/retake-b", &[("If-None-Match", &etag)], b"").status, 304);

    // Une map entière, ou sa fiche match (les seules notes épinglées).
    assert_eq!(s.json("PUT", "/api/notes/ascent/retake-b", r#"{"pin":true}"#).status, 200);
    assert_eq!(s.send("GET", "/api/share/ascent/retake-b", &[("If-None-Match", &etag)], b"").status, 200, "une écriture change l'ETag");
    let sheet = s.get("/api/share/ascent?scope=pinned").json();
    assert_eq!(sheet.str_field("title"), Some("Ascent — fiche match"));
    let d = sheet.str_field("discord").unwrap();
    assert!(
        d.starts_with("# Ascent — fiche match\n*1 note épinglée*") && d.contains("## Retake B\n*Retake · Défense · #b*\n\n### Rôles"),
        "{d}"
    );
    assert!(!d.contains("Setup A\n"), "{d}");
    let all = s.get("/api/share/ascent").json();
    assert!(all.str_field("discord").unwrap().contains("## Setup A") && all.str_field("text").unwrap().contains("Exécution A"));
    assert_eq!(all.str_field("filename"), Some("protocol-ascent.md"));
    assert_eq!(s.get("/api/share/nulle-part").status, 404);
    assert_eq!(s.get("/api/share/ascent/absente").status, 404);
}

#[test]
fn the_carnet_switches_to_english() {
    let s = server(&[]);
    assert_eq!(s.json("POST", "/api/settings", r#"{"lang":"en"}"#).status, 200);
    let st = s.get("/api/state").json();
    assert_eq!((st.str_field("lang"), st.str_field("langSetting")), (Some("en"), Some("en")));
    let kinds = st.get("kinds").and_then(Json::as_array).unwrap();
    let label = |id: &str| kinds.iter().find(|k| k.str_field("id") == Some(id)).and_then(|k| k.str_field("label")).unwrap().to_string();
    assert_eq!((label("execute"), label("protocole")), ("Execute".to_string(), "Protocol".to_string()));
    let sides = st.get("sides").and_then(Json::as_array).unwrap();
    assert_eq!(sides.iter().filter_map(|x| x.str_field("label")).collect::<Vec<_>>(), ["Attack", "Defense"]);
    // Les erreurs de l'API, et celles refusées avant elle, dans la langue retenue.
    let e = s.get("/api/notes/ascent/absente");
    assert_eq!((e.status, e.json().str_field("error")), (404, Some("note not found: ascent/absente")));
    assert_eq!(s.get("/api/nope").json().str_field("error"), Some("unknown route"));
    assert_eq!(s.json("POST", "/api/notes/ascent", "{").json().str_field("error"), Some("invalid JSON at byte 1: “\"” expected"));
    let raw = s.send("POST", "/api/notes/ascent", &[("Transfer-Encoding", "chunked")], b"");
    assert_eq!((raw.status, raw.json().str_field("error")), (501, Some("transfer-encoding not supported")));
    // La page, et une note neuve : son modèle en anglais.
    assert!(String::from_utf8_lossy(&s.get("/").body).contains("<html lang=\"en\""));
    let n = s.json("POST", "/api/notes/ascent", r#"{"title":"Exec A","kind":"execute","side":"atk"}"#).json();
    assert!(n.str_field("body").unwrap().starts_with("## Timing\n- Start:\n- Execute at:"), "{:?}", n.str_field("body"));
    let shared = s.get("/api/share/ascent/exec-a").json();
    assert!(shared.str_field("discord").unwrap().starts_with("# Exec A\n*Execute · Attack · Ascent*"), "{:?}", shared.str_field("discord"));
    let sheet = s.get("/api/share/ascent?scope=pinned").json();
    assert_eq!((sheet.str_field("title"), sheet.str_field("filename")), (Some("Ascent — match sheet"), Some("protocol-ascent-sheet.md")));
    let md = String::from_utf8(s.get("/api/export/ascent.md").body).unwrap();
    assert!(md.starts_with("# Ascent — protocol\n\n_Exported on ") && md.contains("*Execute · Attack*"), "{md}");
    // Retour au français : rien d'autre ne change.
    assert_eq!(s.json("POST", "/api/settings", r#"{"lang":"fr"}"#).status, 200);
    assert_eq!(s.get("/api/nope").json().str_field("error"), Some("route inconnue"));
    assert_eq!(s.json("POST", "/api/settings", r#"{"lang":"klingon"}"#).json().str_field("error"), Some("lang : fr, en ou auto"));
}
