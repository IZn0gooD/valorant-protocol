use super::*;
use crate::store::NoteInput;
use std::sync::atomic::AtomicU64;

/// Ce que le serveur écrit, ligne par ligne.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn messages(&self) -> Vec<Json> {
        let raw = self.0.lock().unwrap().clone();
        String::from_utf8(raw).unwrap().lines().map(|l| json::parse(l).unwrap()).collect()
    }
    fn notifications(&self, method: &str) -> Vec<Json> {
        self.messages().into_iter().filter(|m| m.str_field("method") == Some(method)).collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!("protocol-mcp-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Un carnet d'essai en français (réglage `lang` fixé : les tests ne dépendent pas de la
/// langue du système) : deux notes sur Ascent (dont une épinglée), une règle d'équipe épinglée.
fn carnet(tag: &str) -> (PathBuf, PathBuf) {
    carnet_in(tag, "fr")
}

/// Le même carnet d'essai, réglé sur la langue donnée.
fn carnet_in(tag: &str, lang: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(tag);
    let path = dir.join("essai.carnet");
    let mut s = Store::open(&path, false).unwrap();
    s.set_setting(crate::i18n::SETTING, lang).unwrap();
    s.create_note(
        "ascent",
        NoteInput {
            title: Some("Retake B à trois".into()),
            kind: Some("retake".into()),
            side: Some("def".into()),
            tags: Some(vec!["b".into(), "retake".into()]),
            pin: Some(true),
            body: Some("## Regroupement\n- au marché, fumée **heaven**\n".into()),
            map: None,
        },
    )
    .unwrap();
    s.create_note(
        "ascent",
        NoteInput {
            title: Some("Exécution A rapide".into()),
            kind: Some("execute".into()),
            side: Some("atk".into()),
            ..NoteInput::default()
        },
    )
    .unwrap();
    s.create_note(
        "general",
        NoteInput { title: Some("Communication".into()), pin: Some(true), body: Some("- calls courts\n".into()), ..NoteInput::default() },
    )
    .unwrap();
    s.settle();
    (dir, path)
}

fn session_on(path: &Path, write: bool, port: Option<u16>) -> (Session, Sink) {
    let sink = Sink::default();
    let s = Session::open(path, write, port, Out::new(sink.clone())).unwrap();
    (s, sink)
}

fn ask(s: &mut Session, method: &str, params: Json) -> Json {
    static ID: AtomicU64 = AtomicU64::new(1);
    let id = ID.fetch_add(1, Ordering::Relaxed);
    s.handle(&obj([("jsonrpc", "2.0".into()), ("id", id.into()), ("method", method.into()), ("params", params)])).unwrap()
}

fn result(r: &Json) -> Json {
    r.get("result").cloned().unwrap_or_else(|| panic!("erreur inattendue : {}", r.to_json()))
}

fn error_code(r: &Json) -> f64 {
    r.get("error").and_then(|e| e.get("code")).and_then(Json::as_f64).unwrap_or_else(|| panic!("erreur attendue : {}", r.to_json()))
}

fn init(s: &mut Session, version: &str) -> Json {
    let r = result(&ask(
        s,
        "initialize",
        obj([
            ("protocolVersion", version.into()),
            ("capabilities", obj([])),
            ("clientInfo", obj([("name", "essai".into()), ("version", "1".into())])),
        ]),
    ));
    s.handle(&obj([("jsonrpc", "2.0".into()), ("method", "notifications/initialized".into())]));
    r
}

fn call(s: &mut Session, tool: &str, args: Json) -> Json {
    result(&ask(s, "tools/call", obj([("name", tool.into()), ("arguments", args)])))
}

fn text_of(r: &Json) -> String {
    r.get("content").and_then(|c| c.as_array()).and_then(|c| c.first()).and_then(|c| c.str_field("text")).unwrap_or("").to_string()
}

fn ok(r: &Json) -> &Json {
    assert_eq!(r.get("isError"), Some(&Json::Bool(false)), "{}", r.to_json());
    r.get("structuredContent").unwrap_or_else(|| panic!("pas de données structurées : {}", r.to_json()))
}

fn failed(r: &Json) -> String {
    assert_eq!(r.get("isError"), Some(&Json::Bool(true)), "échec attendu : {}", r.to_json());
    text_of(r)
}

/// Une valeur respecte-t-elle un schéma JSON ? (Le sous-ensemble qu'emploient nos schémas :
/// types, enum, propriétés requises ou interdites, éléments, bornes.)
fn conforms(v: &Json, schema: &Json, at: &str) -> Result<(), String> {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Json::Str(s) => vec![s.as_str()],
            Json::Arr(a) => a.iter().filter_map(Json::as_str).collect(),
            _ => vec![],
        };
        let fits = types.iter().any(|t| match (*t, v) {
            ("string", Json::Str(_))
            | ("boolean", Json::Bool(_))
            | ("null", Json::Null)
            | ("object", Json::Obj(_))
            | ("array", Json::Arr(_)) => true,
            ("integer", Json::Num(n)) => n.fract() == 0.0,
            ("number", Json::Num(_)) => true,
            _ => false,
        });
        if !fits {
            return Err(format!("{at} : type {types:?} attendu, trouvé {}", v.to_json()));
        }
    }
    if let Some(Json::Arr(e)) = schema.get("enum")
        && !e.contains(v)
    {
        return Err(format!("{at} : {} hors de l'énumération", v.to_json()));
    }
    if let (Json::Num(n), Some(min)) = (v, schema.get("minimum").and_then(Json::as_f64))
        && *n < min
    {
        return Err(format!("{at} : {n} < {min}"));
    }
    if let Json::Obj(pairs) = v {
        if let Some(req) = schema.get("required").and_then(Json::as_array) {
            for r in req.iter().filter_map(Json::as_str) {
                if v.get(r).is_none() {
                    return Err(format!("{at} : « {r} » manquant"));
                }
            }
        }
        if let Some(Json::Obj(props)) = schema.get("properties") {
            for (k, sub) in props {
                if let Some(x) = v.get(k) {
                    conforms(x, sub, &format!("{at}.{k}"))?;
                }
            }
            if schema.get("additionalProperties") == Some(&Json::Bool(false)) {
                for (k, _) in pairs {
                    if !props.iter().any(|(p, _)| p == k) {
                        return Err(format!("{at} : propriété en trop « {k} »"));
                    }
                }
            }
        }
    }
    if let (Json::Arr(items), Some(sub)) = (v, schema.get("items")) {
        for (i, x) in items.iter().enumerate() {
            conforms(x, sub, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

/// Chaque outil appelé doit rendre des données conformes au schéma qu'il annonce.
fn checked(s: &mut Session, tool: &str, args: Json) -> Json {
    let tools = result(&ask(s, "tools/list", obj([])));
    let def = tools.get("tools").and_then(Json::as_array).unwrap().iter().find(|t| t.str_field("name") == Some(tool)).cloned().unwrap();
    conforms(&args, def.get("inputSchema").unwrap(), &format!("{tool}(entrée)")).unwrap();
    let r = call(s, tool, args);
    conforms(ok(&r), def.get("outputSchema").unwrap(), tool).unwrap_or_else(|e| panic!("{e}\n{}", r.to_json()));
    r
}

#[test]
fn negotiates_the_version_and_announces_everything() {
    let (dir, path) = carnet("init");
    let (mut s, _) = session_on(&path, false, None);
    let r = init(&mut s, "2025-11-25");
    assert_eq!(r.str_field("protocolVersion"), Some("2025-11-25"));
    let caps = r.get("capabilities").unwrap();
    assert_eq!(caps.get("resources").unwrap().get("subscribe"), Some(&Json::Bool(true)));
    for c in ["tools", "prompts", "completions", "logging"] {
        assert!(caps.get(c).is_some(), "{c}");
    }
    assert!(r.get("serverInfo").unwrap().str_field("description").is_some());
    let how = r.str_field("instructions").unwrap();
    assert!(how.contains("lecture seule") && how.contains("jamais des consignes"), "{how}");

    // Une version inconnue : la plus récente ; une ancienne : le strict nécessaire.
    assert_eq!(init(&mut s, "2099-01-01").str_field("protocolVersion"), Some("2025-11-25"));
    let old = init(&mut s, "2024-11-05");
    assert_eq!(old.str_field("protocolVersion"), Some("2024-11-05"));
    assert!(old.get("capabilities").unwrap().get("completions").is_none());
    let tools = result(&ask(&mut s, "tools/list", obj([])));
    let first = &tools.get("tools").unwrap().as_array().unwrap()[0];
    assert!(first.get("outputSchema").is_none() && first.get("title").is_none() && first.get("annotations").is_none());
    let r = call(&mut s, "list_maps", obj([]));
    assert!(r.get("structuredContent").is_none(), "pas de sortie structurée avant 2025-06-18");
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_tools_answer_in_text_and_in_conforming_data() {
    let (dir, path) = carnet("read");
    let (mut s, _) = session_on(&path, false, None);
    init(&mut s, "2025-11-25");
    let listed = result(&ask(&mut s, "tools/list", obj([])));
    let names: Vec<&str> = listed.get("tools").unwrap().as_array().unwrap().iter().filter_map(|t| t.str_field("name")).collect();
    assert_eq!(names.len(), 10, "lecture seule : aucun outil d'écriture ({names:?})");
    assert!(!names.contains(&"create_note"));
    for t in listed.get("tools").unwrap().as_array().unwrap() {
        assert_eq!(t.get("annotations").unwrap().get("readOnlyHint"), Some(&Json::Bool(true)));
    }

    let maps = checked(&mut s, "list_maps", obj([]));
    assert!(text_of(&maps).contains("ascent — Ascent : 2 notes, 1 épinglée"), "{}", text_of(&maps));
    let m = checked(&mut s, "get_map", obj([("map", "ascent".into())]));
    assert!(text_of(&m).contains("Retake B à trois"));
    let notes = checked(&mut s, "list_notes", obj([("map", "ascent".into()), ("side", "def".into())]));
    assert_eq!(ok(&notes).get("total"), Some(&Json::Num(1.0)));
    let tagged = checked(&mut s, "list_notes", obj([("tag", "#B".into()), ("pinned", true.into())]));
    assert_eq!(ok(&tagged).get("total"), Some(&Json::Num(1.0)));
    let page = checked(&mut s, "list_notes", obj([("limit", 2.into())]));
    assert_eq!(ok(&page).str_field("nextCursor"), Some("2"));
    let rest = checked(&mut s, "list_notes", obj([("limit", 2.into()), ("cursor", "2".into())]));
    assert!(ok(&rest).get("nextCursor").is_none());

    let n = checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())]));
    assert!(ok(&n).str_field("body").unwrap().contains("heaven"));
    assert!(text_of(&n).contains("version "));
    // Par son titre aussi.
    assert!(
        ok(&checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "Retake B à trois".into())])))
            .str_field("version")
            .is_some()
    );

    let found = checked(&mut s, "search_notes", obj([("query", "fumee type:retake".into())]));
    assert_eq!(ok(&found).get("total"), Some(&Json::Num(1.0)));
    assert!(text_of(&found).contains("**"), "termes marqués : {}", text_of(&found));
    let sheet = checked(&mut s, "match_sheet", obj([("map", "ascent".into())]));
    let md = ok(&sheet).str_field("markdown").unwrap();
    assert!(md.contains("Retake B à trois") && md.contains("Communication") && !md.contains("Exécution A rapide"), "{md}");
    let tags = checked(&mut s, "list_tags", obj([]));
    assert!(text_of(&tags).contains("#b (1)"));
    checked(&mut s, "recent_notes", obj([("limit", 2.into())]));
    let discord = checked(&mut s, "format_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())]));
    assert!(text_of(&discord).contains("Retake B à trois"));
    checked(&mut s, "format_note", obj([("map", "ascent".into()), ("pinned_only", true.into()), ("format", "text".into())]));
    let trash = checked(&mut s, "list_trash", obj([]));
    assert_eq!(text_of(&trash), "La corbeille est vide.");
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failures_are_said_so_the_assistant_can_correct_itself() {
    let (dir, path) = carnet("fail");
    let (mut s, _) = session_on(&path, false, None);
    init(&mut s, "2025-11-25");
    assert!(failed(&call(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "nope".into())]))).contains("introuvable"));
    assert!(failed(&call(&mut s, "get_map", obj([("map", "pearl-2".into())]))).contains("list_maps"));
    assert!(failed(&call(&mut s, "list_notes", obj([("type", "cuisine".into())]))).contains("type inconnu"));
    assert!(failed(&call(&mut s, "read_note", obj([("map", "ascent".into())]))).contains("argument requis : id"));
    assert!(failed(&call(&mut s, "format_note", obj([("map", "ascent".into()), ("format", "pdf".into())]))).contains("format inconnu"));
    // Erreurs du protocole : outil inconnu, outil d'écriture sans --write, arguments mal formés.
    assert_eq!(error_code(&ask(&mut s, "tools/call", obj([("name", "rm_rf".into())]))), -32602.0);
    let w = ask(
        &mut s,
        "tools/call",
        obj([("name", "create_note".into()), ("arguments", obj([("map", "ascent".into()), ("title", "x".into())]))]),
    );
    assert!(w.get("error").unwrap().str_field("message").unwrap().contains("--write"));
    assert_eq!(error_code(&ask(&mut s, "tools/call", obj([("name", "list_maps".into()), ("arguments", "x".into())]))), -32602.0);
    assert_eq!(error_code(&ask(&mut s, "nope/nope", obj([]))), -32601.0);
    assert_eq!(error_code(&ask(&mut s, "logging/setLevel", obj([("level", "bavard".into())]))), -32602.0);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_write_is_recoverable_and_the_carnet_stays_free() {
    let (dir, path) = carnet("write");
    let (mut s, _) = session_on(&path, true, Some(1));
    init(&mut s, "2025-11-25");
    assert_eq!(result(&ask(&mut s, "tools/list", obj([]))).get("tools").unwrap().as_array().unwrap().len(), 19);

    let created = checked(
        &mut s,
        "create_note",
        obj([
            ("map", "ascent".into()),
            ("title", "Setup B 2-1-2".into()),
            ("type", "setup".into()),
            ("side", "def".into()),
            ("tags", vec![Json::from("#b")].into()),
        ]),
    );
    let note = ok(&created).get("note").unwrap().clone();
    assert_eq!(note.str_field("id"), Some("setup-b-2-1-2"));
    assert_eq!(note.get("tags"), Some(&vec![Json::from("b")].into()));
    // Pendant ce temps, le carnet n'est pas tenu : la fenêtre pourrait l'ouvrir.
    drop(Store::open(&path, false).expect("carnet libre entre deux écritures"));

    // Modifier exige la version lue ; une version périmée n'écrase rien.
    let v1 = ok(&checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())])))
        .str_field("version")
        .unwrap()
        .to_string();
    let stale = call(
        &mut s,
        "update_note",
        obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("base_version", "0000".into()), ("body", "écrasé".into())]),
    );
    assert!(failed(&stale).contains("rien n'a été écrasé"));
    let upd = checked(
        &mut s,
        "update_note",
        obj([
            ("map", "ascent".into()),
            ("id", "retake-b-a-trois".into()),
            ("base_version", v1.as_str().into()),
            ("body", "## Regroupement\n- sous heaven\n".into()),
        ]),
    );
    let backup = ok(&upd).str_field("backup").unwrap().to_string();
    assert!(text_of(&upd).contains("corbeille"));
    let trash = checked(&mut s, "list_trash", obj([]));
    assert!(text_of(&trash).contains("Retake B à trois — version d'avant l'assistant"), "{}", text_of(&trash));
    // La version d'avant revient à côté, sous son propre titre.
    let back = checked(&mut s, "restore_from_trash", obj([("key", backup.as_str().into())]));
    assert_eq!(ok(&back).str_field("id"), Some("retake-b-a-trois-2"));
    let old = checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois-2".into())]));
    assert!(ok(&old).str_field("body").unwrap().contains("au marché"));
    assert_eq!(ok(&old).get("pinned"), Some(&Json::Bool(false)), "une copie restaurée n'est pas épinglée");

    let app = checked(
        &mut s,
        "append_to_note",
        obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("text", "## Calls\n- « marché clean »".into())]),
    );
    assert_eq!(ok(&app).get("backup"), Some(&Json::Null));
    let now = checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())]));
    assert!(ok(&now).str_field("body").unwrap().ends_with("- sous heaven\n\n## Calls\n- « marché clean »\n"));
    checked(&mut s, "set_pinned", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("pinned", false.into())]));
    let moved = checked(&mut s, "move_note", obj([("map", "ascent".into()), ("id", "setup-b-2-1-2".into()), ("to_map", "bind".into())]));
    assert_eq!(ok(&moved).get("note").unwrap().str_field("map"), Some("bind"));
    let gone = checked(&mut s, "trash_note", obj([("map", "bind".into()), ("id", "setup-b-2-1-2".into())]));
    let key = ok(&gone).str_field("trash").unwrap().to_string();
    assert!(failed(&call(&mut s, "read_note", obj([("map", "bind".into()), ("id", "setup-b-2-1-2".into())]))).contains("introuvable"));
    checked(&mut s, "restore_from_trash", obj([("key", key.as_str().into())]));
    checked(&mut s, "read_note", obj([("map", "bind".into()), ("id", "setup-b-2-1-2".into())]));

    let map = checked(&mut s, "create_map", obj([("title", "Carte d'essai".into())]));
    assert_eq!(ok(&map).get("map").unwrap().str_field("id"), Some("carte-d-essai"));
    assert!(failed(&call(&mut s, "update_map", obj([("map", "ascent".into()), ("overview", "x".into())]))).contains("base_version"));
    let mv = ok(&checked(&mut s, "get_map", obj([("map", "ascent".into())]))).get("map").unwrap().str_field("version").unwrap().to_string();
    let um = checked(
        &mut s,
        "update_map",
        obj([
            ("map", "ascent".into()),
            ("overview", "Priorité : le contrôle du mid.".into()),
            ("base_version", mv.as_str().into()),
            ("in_pool", true.into()),
        ]),
    );
    assert!(ok(&um).str_field("backup").is_some());
    assert_eq!(ok(&um).get("map").unwrap().get("pool"), Some(&Json::Bool(true)));

    // Rien à changer, ou trop long : refusé avant d'écrire.
    assert!(
        failed(&call(
            &mut s,
            "update_note",
            obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("base_version", "x".into())])
        ))
        .contains("rien à changer")
    );
    let huge = "x".repeat(MAX_MESSAGE.min((1 << 20) + 1));
    assert!(
        failed(&call(&mut s, "create_note", obj([("map", "ascent".into()), ("title", "Énorme".into()), ("body", huge.into())])))
            .contains("trop long")
    );
    drop(s);
    drop(Store::open(&path, false).expect("rien ne reste verrouillé"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn writes_are_paced() {
    let (dir, path) = carnet("pace");
    let (mut s, _) = session_on(&path, true, Some(1));
    init(&mut s, "2025-11-25");
    let pin =
        |s: &mut Session, p: bool| call(s, "set_pinned", obj([("map", "ascent".into()), ("id", "exec-a".into()), ("pinned", p.into())]));
    // Les écritures refusées (note introuvable) comptent aussi : le rythme protège le carnet.
    for i in 0..WRITES_PER_MINUTE {
        assert!(failed(&pin(&mut s, i % 2 == 0)).contains("introuvable"));
    }
    assert!(failed(&pin(&mut s, true)).contains("trop d'écritures"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resources_are_listed_read_and_followed() {
    let (dir, path) = carnet("res");
    {
        let mut w = Store::open(&path, false).unwrap();
        for i in 0..120 {
            w.create_note("bind", NoteInput { title: Some(format!("Lineup {i}")), kind: Some("lineup".into()), ..NoteInput::default() })
                .unwrap();
        }
        w.settle();
    }
    let (mut s, sink) = session_on(&path, false, None);
    init(&mut s, "2025-11-25");
    let first = result(&ask(&mut s, "resources/list", obj([])));
    let page = first.get("resources").unwrap().as_array().unwrap();
    assert_eq!(page.len(), 100);
    assert_eq!(page[0].str_field("uri"), Some("protocol://guide"));
    let cursor = first.str_field("nextCursor").unwrap().to_string();
    let second = result(&ask(&mut s, "resources/list", obj([("cursor", cursor.as_str().into())])));
    assert!(second.get("nextCursor").is_none() && !second.get("resources").unwrap().as_array().unwrap().is_empty());
    assert_eq!(error_code(&ask(&mut s, "resources/list", obj([("cursor", "page-2".into())]))), -32602.0);
    let templates = result(&ask(&mut s, "resources/templates/list", obj([])));
    assert_eq!(templates.get("resourceTemplates").unwrap().as_array().unwrap().len(), 4);

    let read = |s: &mut Session, uri: &str| {
        let r = result(&ask(s, "resources/read", obj([("uri", uri.into())])));
        r.get("contents").unwrap().as_array().unwrap()[0].str_field("text").unwrap().to_string()
    };
    assert!(read(&mut s, "protocol://note/ascent/retake-b-a-trois").starts_with("---\ntitle: Retake B à trois\n"));
    assert!(read(&mut s, "protocol://sheet/ascent").contains("Communication"));
    assert!(read(&mut s, "protocol://map/ascent").contains("Exécution A rapide"));
    assert!(read(&mut s, "protocol://guide").contains("fiche match"));
    assert!(read(&mut s, "protocol://types").contains("## Retake (`retake`)"));
    assert!(read(&mut s, "protocol://search/fum%C3%A9e%20heaven").contains("Retake B à trois"));
    let missing = ask(&mut s, "resources/read", obj([("uri", "protocol://note/ascent/nope".into())]));
    assert_eq!(error_code(&missing), -32002.0);
    assert_eq!(missing.get("error").unwrap().get("data").unwrap().str_field("uri"), Some("protocol://note/ascent/nope"));
    assert_eq!(error_code(&ask(&mut s, "resources/read", obj([("uri", "file:///C:/Windows/win.ini".into())]))), -32002.0);

    // Abonnement : un changement fait ailleurs (la fenêtre) est signalé ; un désabonnement, plus.
    let uri = "protocol://note/ascent/retake-b-a-trois";
    result(&ask(&mut s, "resources/subscribe", obj([("uri", uri.into())])));
    let mut seen = Seen::default();
    seen.tick(&s.live);
    sink.clear();
    {
        let mut w = Store::open(&path, false).unwrap();
        w.save_note("ascent", "retake-b-a-trois", NoteInput { body: Some("changé ailleurs".into()), ..NoteInput::default() }, None)
            .unwrap();
        w.create_note("ascent", NoteInput { title: Some("Nouvelle".into()), ..NoteInput::default() }).unwrap();
        w.settle();
    }
    seen.tick(&s.live);
    let updated = sink.notifications("notifications/resources/updated");
    assert_eq!(updated.len(), 1, "{:?}", sink.messages());
    assert_eq!(updated[0].get("params").unwrap().str_field("uri"), Some(uri));
    assert_eq!(sink.notifications("notifications/resources/list_changed").len(), 1);
    result(&ask(&mut s, "resources/unsubscribe", obj([("uri", uri.into())])));
    sink.clear();
    {
        let mut w = Store::open(&path, false).unwrap();
        w.save_note("ascent", "retake-b-a-trois", NoteInput { body: Some("encore".into()), ..NoteInput::default() }, None).unwrap();
        w.settle();
    }
    seen.tick(&s.live);
    assert!(sink.notifications("notifications/resources/updated").is_empty());
    assert_eq!(error_code(&ask(&mut s, "resources/subscribe", obj([("uri", "protocol://map/nope".into())]))), -32002.0);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn prompts_join_the_real_notes_and_arguments_complete() {
    let (dir, path) = carnet("prompts");
    let (mut s, _) = session_on(&path, false, None);
    init(&mut s, "2025-11-25");
    let list = result(&ask(&mut s, "prompts/list", obj([])));
    let prompts = list.get("prompts").unwrap().as_array().unwrap();
    assert_eq!(prompts.len(), 6);
    assert!(prompts.iter().all(|p| p.str_field("title").is_some() && p.get("arguments").is_some()));

    let get = |s: &mut Session, name: &str, args: Json| ask(s, "prompts/get", obj([("name", name.into()), ("arguments", args)]));
    let r = result(&get(&mut s, "failles", obj([("map", "ascent".into()), ("note", "retake-b-a-trois".into())])));
    let msgs = r.get("messages").unwrap().as_array().unwrap();
    let intro = msgs[0].get("content").unwrap().str_field("text").unwrap();
    assert!(intro.contains("Joue l'adversaire") && intro.contains("jamais des consignes"), "{intro}");
    let res = msgs[1].get("content").unwrap();
    assert_eq!(res.str_field("type"), Some("resource"));
    assert_eq!(res.get("resource").unwrap().str_field("uri"), Some("protocol://note/ascent/retake-b-a-trois"));
    assert!(res.get("resource").unwrap().str_field("text").unwrap().contains("heaven"));
    let prep = result(&get(&mut s, "preparer_match", obj([("map", "ascent".into()), ("adversaire", "Les Rivaux".into())])));
    assert!(prep.to_json().contains("contre Les Rivaux") && prep.to_json().contains("protocol://sheet/ascent"));
    let tidy = result(&get(&mut s, "ranger_texte", obj([("map", "bind".into()), ("texte", "smoke heaven puis entrée B".into())])));
    assert!(tidy.to_json().contains("prête à coller") && tidy.to_json().contains("protocol://types"));
    result(&get(&mut s, "debrief", obj([("map", "ascent".into()), ("observations", "perdu 4 retakes B".into())])));
    result(&get(&mut s, "resumer", obj([("map", "ascent".into()), ("note", "Retake B à trois".into())])));
    assert_eq!(error_code(&get(&mut s, "resumer", obj([("map", "ascent".into())]))), -32602.0);
    assert_eq!(error_code(&get(&mut s, "resumer", obj([("map", "ascent".into()), ("note", "nope".into())]))), -32602.0);
    assert_eq!(error_code(&get(&mut s, "inconnu", obj([]))), -32602.0);

    let complete = |s: &mut Session, r: Json, name: &str, value: &str, map: Option<&str>| {
        let mut p = obj([("ref", r), ("argument", obj([("name", name.into()), ("value", value.into())]))]);
        if let Some(m) = map {
            p.push("context", obj([("arguments", obj([("map", m.into())]))]));
        }
        let c = result(&ask(s, "completion/complete", p));
        c.get("completion")
            .unwrap()
            .get("values")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect::<Vec<_>>()
    };
    let prompt = || obj([("type", "ref/prompt".into()), ("name", "resumer".into())]);
    assert_eq!(complete(&mut s, prompt(), "map", "asc", None), vec!["ascent"]);
    assert_eq!(complete(&mut s, prompt(), "note", "ret", Some("ascent")), vec!["retake-b-a-trois"]);
    assert!(complete(&mut s, prompt(), "note", "commu", None).contains(&"general/communication".to_string()));
    let template = obj([("type", "ref/resource".into()), ("uri", "protocol://note/{map}/{id}".into())]);
    assert_eq!(complete(&mut s, template, "id", "exec", Some("ascent")), vec!["execution-a-rapide"]);
    drop(s);

    // Avec --write, ranger un texte propose de l'enregistrer.
    let (mut w, _) = session_on(&path, true, Some(1));
    init(&mut w, "2025-11-25");
    let tidy = result(&ask(
        &mut w,
        "prompts/get",
        obj([("name", "ranger_texte".into()), ("arguments", obj([("map", "bind".into()), ("texte", "x".into())]))]),
    ));
    assert!(tidy.to_json().contains("create_note"));
    drop(w);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_transport_is_bounded_and_batches_follow_the_version() {
    let (dir, path) = carnet("transport");
    let (s, sink) = session_on(&path, false, None);
    let mut input = Vec::new();
    input.extend_from_slice(&vec![b'x'; MAX_MESSAGE + 10]);
    input.extend_from_slice(b"\n\xff\xfe\n{pas du json\n[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]\n\n");
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}");
    s.serve(io::Cursor::new(input)).unwrap();
    let out = sink.messages();
    let codes: Vec<f64> = out.iter().filter_map(|m| m.get("error")).filter_map(|e| e.get("code")).filter_map(Json::as_f64).collect();
    assert_eq!(codes, vec![-32600.0, -32700.0, -32700.0, -32600.0], "{out:?}");
    assert!(out.iter().any(|m| m.get("id") == Some(&Json::Num(2.0)) && m.get("result").is_some()), "dernière ligne sans fin de ligne");

    // En 2025-03-26, les lots sont pris en charge.
    let (s, sink) = session_on(&path, false, None);
    let lines = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\"}}\n\
                 [{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}]\n";
    s.serve(io::Cursor::new(lines.as_bytes().to_vec())).unwrap();
    let batch = sink.messages().pop().unwrap();
    assert_eq!(batch.as_array().map(<[Json]>::len), Some(1), "{}", batch.to_json());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_log_follows_the_requested_level() {
    let (dir, path) = carnet("log");
    let (mut s, sink) = session_on(&path, true, Some(1));
    init(&mut s, "2025-11-25");
    call(&mut s, "set_pinned", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("pinned", false.into())]));
    assert!(sink.notifications("notifications/message").is_empty(), "niveau warning par défaut");
    result(&ask(&mut s, "logging/setLevel", obj([("level", "info".into())])));
    call(&mut s, "set_pinned", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("pinned", true.into())]));
    let logs = sink.notifications("notifications/message");
    assert_eq!(logs.len(), 1);
    assert!(logs[0].get("params").unwrap().str_field("data").unwrap().contains("dans le carnet : ascent/retake-b-a-trois"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn with_the_window_open_writes_go_through_it() {
    let (dir, path) = carnet("window");
    // La fenêtre : le vrai serveur de protocol, qui tient le carnet en écriture.
    let store = Store::open(&path, false).unwrap();
    let cfg = crate::api::Config { lan: false, lan_token: None, cors: Vec::new(), auto_exit: false };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let server = crate::start(listener, crate::api::App::new(store, cfg)).unwrap();
    assert!(Store::open(&path, false).is_err(), "la fenêtre tient le carnet");

    let (mut s, sink) = session_on(&path, true, Some(server.port));
    init(&mut s, "2025-11-25");
    result(&ask(&mut s, "logging/setLevel", obj([("level", "info".into())])));
    let created = checked(
        &mut s,
        "create_note",
        obj([("map", "ascent".into()), ("title", "Anti-strat Rivaux".into()), ("type", "antistrat".into())]),
    );
    assert_eq!(ok(&created).get("note").unwrap().str_field("id"), Some("anti-strat-rivaux"));
    assert!(sink.notifications("notifications/message")[0].to_json().contains("par la fenêtre ouverte"));
    // La fenêtre la voit aussitôt.
    assert!(server.app.store().note("ascent", "anti-strat-rivaux").is_some());

    // Même sécurité qu'en direct : version exigée, version d'avant gardée.
    let v = ok(&checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())])))
        .str_field("version")
        .unwrap()
        .to_string();
    let upd = checked(
        &mut s,
        "update_note",
        obj([
            ("map", "ascent".into()),
            ("id", "retake-b-a-trois".into()),
            ("base_version", v.as_str().into()),
            ("title", "Retake B (révisé)".into()),
        ]),
    );
    let key = ok(&upd).str_field("backup").unwrap().to_string();
    assert!(server.app.store().trash().iter().any(|t| t.path == key && t.title.contains("version d'avant l'assistant")));
    let stale = call(
        &mut s,
        "update_note",
        obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("base_version", v.as_str().into()), ("body", "x".into())]),
    );
    assert!(failed(&stale).contains("rien n'a été écrasé"));
    let gone = checked(&mut s, "trash_note", obj([("map", "ascent".into()), ("id", "anti-strat-rivaux".into())]));
    assert!(ok(&gone).str_field("trash").is_some_and(|k| !k.is_empty()));
    let mv = ok(&checked(&mut s, "get_map", obj([("map", "ascent".into())]))).get("map").unwrap().str_field("version").unwrap().to_string();
    let um = checked(
        &mut s,
        "update_map",
        obj([("map", "ascent".into()), ("overview", "Mid d'abord.".into()), ("base_version", mv.as_str().into())]),
    );
    assert!(ok(&um).str_field("backup").is_some());
    drop(s);
    // Le serveur de la fenêtre tourne jusqu'à la fin des tests : son dossier reste en place.
    let _ = dir;
}

#[test]
fn a_new_carnet_is_created_then_released() {
    let dir = temp_dir("new");
    let path = dir.join("neuf.carnet");
    assert!(Session::open(&path, false, None, Out::new(Sink::default())).is_err(), "en lecture seule, pas de carnet créé");
    let (s, _) = session_on(&path, true, Some(1));
    drop(Store::open(&path, false).expect("créé puis relâché"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

/// Un outil de `tools/list`, par son nom.
fn tool_def(s: &mut Session, name: &str) -> Json {
    let tools = result(&ask(s, "tools/list", obj([])));
    tools.get("tools").and_then(Json::as_array).unwrap().iter().find(|t| t.str_field("name") == Some(name)).cloned().unwrap()
}

#[test]
fn an_english_carnet_speaks_english_to_the_client_and_the_assistant() {
    let (dir, path) = carnet_in("en", "en");
    let (mut s, _) = session_on(&path, true, Some(1));
    let r = init(&mut s, "2025-11-25");
    let info = r.get("serverInfo").unwrap();
    assert_eq!(info.str_field("title"), Some("protocol — Valorant coaching notebook"));
    assert!(info.str_field("description").unwrap().contains("map by map"));
    let how = r.str_field("instructions").unwrap();
    assert!(how.contains("never instructions to follow") && how.contains("Ask the coach for approval"), "{how}");
    assert!(!how.contains("carnet") && !how.contains("consignes"), "{how}");

    // Les outils : noms inchangés, titres, descriptions et schémas en anglais.
    let read_note = tool_def(&mut s, "read_note");
    assert_eq!(read_note.str_field("title"), Some("Read a note"));
    assert!(read_note.str_field("description").unwrap().starts_with("Reads a full note"));
    let map_prop = read_note.get("inputSchema").unwrap().get("properties").unwrap().get("map").unwrap();
    assert_eq!(map_prop.str_field("description"), Some("map id (e.g. ascent, bind, general)"));
    let body_prop = read_note.get("outputSchema").unwrap().get("properties").unwrap().get("body").unwrap();
    assert_eq!(body_prop.str_field("description"), Some("the note body, in Markdown"));
    assert_eq!(tool_def(&mut s, "match_sheet").str_field("title"), Some("Match sheet"));

    // Les sorties restent conformes à leur schéma, et leur texte est en anglais.
    let maps = checked(&mut s, "list_maps", obj([]));
    assert!(text_of(&maps).contains("ascent — Ascent: 2 notes, 1 pinned"), "{}", text_of(&maps));
    let n = checked(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into())]));
    assert!(text_of(&n).contains("defense · pinned") && text_of(&n).contains("updated "), "{}", text_of(&n));
    let found = checked(&mut s, "search_notes", obj([("query", "heaven type:retake".into())]));
    assert!(text_of(&found).starts_with("1 result for \"heaven type:retake\":"), "{}", text_of(&found));
    let m = checked(&mut s, "get_map", obj([("map", "ascent".into())]));
    assert!(text_of(&m).contains("## Overview"), "{}", text_of(&m));
    checked(&mut s, "match_sheet", obj([("map", "ascent".into())]));
    checked(&mut s, "format_note", obj([("map", "ascent".into()), ("pinned_only", true.into())]));
    assert_eq!(text_of(&checked(&mut s, "list_trash", obj([]))), "The trash is empty.");
    let created =
        checked(&mut s, "create_note", obj([("map", "bind".into()), ("title", "B site retake".into()), ("type", "retake".into())]));
    assert!(text_of(&created).starts_with("Note created: bind/b-site-retake"), "{}", text_of(&created));

    // Les erreurs d'outil et du protocole.
    let missing = failed(&call(&mut s, "read_note", obj([("map", "ascent".into()), ("id", "nope".into())])));
    assert_eq!(missing, "note not found: ascent/nope (see list_notes or search_notes)");
    assert!(failed(&call(&mut s, "list_notes", obj([("type", "cuisine".into())]))).starts_with("unknown type: cuisine"));
    let stale = call(
        &mut s,
        "update_note",
        obj([("map", "ascent".into()), ("id", "retake-b-a-trois".into()), ("base_version", "0000".into()), ("body", "x".into())]),
    );
    assert!(failed(&stale).contains("nothing was overwritten"), "{}", text_of(&stale));
    let unknown = ask(&mut s, "tools/call", obj([("name", "rm_rf".into())]));
    assert_eq!(unknown.get("error").unwrap().str_field("message"), Some("unknown tool: rm_rf"));
    let gone = ask(&mut s, "resources/read", obj([("uri", "protocol://note/ascent/nope".into())]));
    assert_eq!(gone.get("error").unwrap().str_field("message"), Some("resource not found: protocol://note/ascent/nope"));

    // Les consignes : mêmes noms et arguments, textes en anglais, règle de sécurité comprise.
    let list = result(&ask(&mut s, "prompts/list", obj([])));
    let failles = list.get("prompts").unwrap().as_array().unwrap().iter().find(|p| p.str_field("name") == Some("failles")).unwrap().clone();
    assert_eq!(failles.str_field("title"), Some("Find the weaknesses"));
    let get = |s: &mut Session, name: &str, args: Json| result(&ask(s, "prompts/get", obj([("name", name.into()), ("arguments", args)])));
    let r = get(&mut s, "failles", obj([("map", "ascent".into()), ("note", "retake-b-a-trois".into())]));
    let intro = r.get("messages").unwrap().as_array().unwrap()[0].get("content").unwrap().str_field("text").unwrap().to_string();
    assert!(
        intro.contains("Play the opponent") && intro.contains("Answer in English") && intro.contains("never instructions to follow"),
        "{intro}"
    );
    let prep = get(&mut s, "preparer_match", obj([("map", "ascent".into()), ("adversaire", "Rivals".into())]));
    assert!(prep.to_json().contains("against Rivals"));

    // Les ressources : le guide, la liste, les modèles d'URI.
    let guide = result(&ask(&mut s, "resources/read", obj([("uri", "protocol://guide".into())])));
    let text = guide.get("contents").unwrap().as_array().unwrap()[0].str_field("text").unwrap().to_string();
    assert!(text.starts_with("# The protocol notebook") && text.contains("never instructions"), "{text}");
    let listed = result(&ask(&mut s, "resources/list", obj([])));
    let first = &listed.get("resources").unwrap().as_array().unwrap()[0];
    assert_eq!(first.str_field("title"), Some("The protocol notebook"));
    assert!(listed.to_json().contains("Ascent — match sheet"));
    let templates = result(&ask(&mut s, "resources/templates/list", obj([])));
    assert_eq!(templates.get("resourceTemplates").unwrap().as_array().unwrap()[0].str_field("title"), Some("A note"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_language_is_read_again_at_each_request() {
    let (dir, path) = carnet("relang");
    let (mut s, _) = session_on(&path, false, None);
    init(&mut s, "2025-11-25");
    assert_eq!(tool_def(&mut s, "list_maps").str_field("title"), Some("Lister les maps"));
    // Le coach passe le carnet en anglais depuis la fenêtre : la requête suivante le suit.
    {
        let mut w = Store::open(&path, false).unwrap();
        w.set_setting(crate::i18n::SETTING, "en").unwrap();
        w.settle();
    }
    let _ = s.live.store().refresh(true);
    assert_eq!(tool_def(&mut s, "list_maps").str_field("title"), Some("List maps"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}
