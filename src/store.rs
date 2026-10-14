//! Le carnet : maps, notes, images et corbeille, dans **un seul fichier** rangé à
//! côté du programme (`protocol.carnet`, voir `carnet` pour le format). Rien
//! n'est écrit ailleurs sur le poste.
//!
//! Une note reste un texte Markdown à en-tête (« front matter »), le format
//! qu'Obsidian, VS Code ou GitHub lisent : le carnet s'exporte à tout moment en
//! fichiers `.md` (archive ZIP), et un ancien dossier `notes/` est repris tel quel
//! au premier lancement, sans y toucher.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::carnet::{self, Entry, Journal, Op, Recovery, Writer};
use crate::i18n::{self, Lang};
use crate::note::{self, Document, FrontMatter};
use crate::render;
use crate::share;
use crate::tr;
use crate::util::{self, fnv1a64, fold, hex64};
use crate::zip::Zip;

pub const TEAM_MAP: &str = "general";
/// Les images collées : `assets/<empreinte>.png`, comme dans l'ancien dossier.
pub const ASSETS_DIR: &str = "assets";
/// Ancien format (un dossier par map) : la présentation d'une map, la corbeille.
pub const LEGACY_MAP_FILE: &str = "_map.md";
pub const LEGACY_TRASH_DIR: &str = ".trash";
const MAX_NOTE_BYTES: usize = 4 * 1024 * 1024;
/// En deçà, le journal n'est pas compacté : réécrire ne vaudrait pas la peine.
const COMPACT_MIN: u64 = 2 << 20;

/// `général` pour l'équipe, puis les maps de compétition. On en ajoute ou en
/// retire librement ; une map ajoutée à cette liste par une version suivante
/// (Summit) apparaît une fois dans les carnets existants.
pub const DEFAULT_MAPS: &[(&str, &str)] = &[
    (TEAM_MAP, "Général"),
    ("abyss", "Abyss"),
    ("ascent", "Ascent"),
    ("bind", "Bind"),
    ("breeze", "Breeze"),
    ("corrode", "Corrode"),
    ("fracture", "Fracture"),
    ("haven", "Haven"),
    ("icebox", "Icebox"),
    ("lotus", "Lotus"),
    ("pearl", "Pearl"),
    ("split", "Split"),
    ("summit", "Summit"),
    ("sunset", "Sunset"),
];

/// Les maps que proposait la première version : un ancien dossier repris ne voit
/// apparaître que les maps sorties depuis, jamais une map qu'on y avait retirée.
const FIRST_DEFAULTS: &[&str] =
    &[TEAM_MAP, "abyss", "ascent", "bind", "breeze", "corrode", "fracture", "haven", "icebox", "lotus", "pearl", "split", "sunset"];

pub const IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("svg", "image/svg+xml"),
];

/// Réglages gardés dans le carnet (rien dans le navigateur, rien sur le poste).
pub const SETTING_THEME: &str = "theme";
const SETTING_NOTICE: &str = "notice";
const SETTING_DEFAULTS: &str = "defaults";

#[derive(Debug)]
pub struct Note {
    pub map: String,
    pub id: String,
    pub doc: Document,
    /// Le texte tel qu'il est enregistré (en-tête compris).
    pub text: String,
    pub title: String,
    /// Empreinte du texte : sert de jeton anti-écrasement.
    pub version: String,
    pub updated_ts: i64,
    pub plain: String,
    pub folded_plain: String,
    pub folded_title: String,
    pub folded_tags: Vec<String>,
    pub preview: String,
    pub excerpt: String,
}

impl Note {
    pub fn kind(&self) -> &str {
        self.doc.fm.kind.as_deref().unwrap_or("note")
    }
    pub fn side(&self) -> &str {
        self.doc.fm.side.as_deref().unwrap_or("")
    }

    /// Le corps à afficher : sans le `# Titre` qui l'ouvre quand c'est lui qui donne
    /// son titre à la note (un fichier écrit ailleurs, sans en-tête) ; la page l'affiche déjà.
    pub fn shown_body(&self) -> &str {
        if self.doc.fm.title.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            return &self.doc.body;
        }
        match first_heading_at(&self.doc.body) {
            Some((end, t)) if t == self.title => self.doc.body[end..].trim_start_matches(['\n', '\r']),
            _ => &self.doc.body,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MapInfo {
    pub id: String,
    pub title: String,
    pub pool: Option<bool>,
    pub order: Option<i64>,
    pub doc: Document,
    pub version: String,
    pub count: usize,
    pub pinned: usize,
    pub updated_ts: i64,
}

impl MapInfo {
    /// La map de l'équipe (`général`) n'est pas une map de jeu : elle ne compte pas parmi elles.
    pub fn is_team(&self) -> bool {
        self.id == TEAM_MAP
    }
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    Invalid(String),
    Conflict(Arc<Note>),
    MapConflict(Box<MapInfo>),
    ReadOnly,
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(m) | StoreError::Invalid(m) => f.write_str(m),
            StoreError::Conflict(_) => f.write_str("cette note a changé depuis son ouverture (autre fenêtre)"),
            StoreError::MapConflict(_) => f.write_str("cette présentation a changé depuis son ouverture (autre fenêtre)"),
            StoreError::ReadOnly => f.write_str("carnet ouvert en lecture seule"),
            StoreError::Io(e) => write!(f, "erreur disque : {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Ce que l'interface (ou une API) veut changer dans une note ; `None` = inchangé.
#[derive(Debug, Default, Clone)]
pub struct NoteInput {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub side: Option<String>,
    pub tags: Option<Vec<String>>,
    pub pin: Option<bool>,
    pub body: Option<String>,
    /// Déplacer vers une autre map.
    pub map: Option<String>,
}

pub struct TrashItem {
    /// Clé de l'élément, à renvoyer pour le restaurer.
    pub path: String,
    pub map: String,
    /// `None` : la présentation d'une map supprimée.
    pub id: Option<String>,
    pub title: String,
    pub deleted_ts: i64,
}

/// Le `# Titre` qui ouvre un texte (première ligne non vide, titre de niveau 1) :
/// la fin de sa ligne et son texte, sans marques (`# **Retake** B #` → « Retake B »).
fn first_heading_at(body: &str) -> Option<(usize, String)> {
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        let l = line.trim_end_matches(['\n', '\r']);
        at += line.len();
        if l.trim().is_empty() {
            continue;
        }
        let (level, text) = share::heading(l)?;
        let title = share::plain_inline(text);
        return (level == 1 && !title.is_empty()).then_some((at, title));
    }
    None
}

fn first_heading(body: &str) -> Option<String> {
    first_heading_at(body).map(|(_, t)| t)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Le titre d'un texte de note : son en-tête, sinon son `# Titre`, sinon `id`.
fn title_of(doc: &Document, id: &str) -> String {
    doc.fm.title.clone().filter(|t| !t.trim().is_empty()).or_else(|| first_heading(&doc.body)).unwrap_or_else(|| id.to_string())
}

/// Une note prête à l'emploi : analysée, repliée pour la recherche, résumée.
/// `ts` : le moment de son enregistrement (repli quand l'en-tête n'a pas de date).
pub fn build_note(map: &str, id: &str, text: String, ts: i64) -> Note {
    let doc = note::parse(&text);
    let title = title_of(&doc, id);
    let updated_ts = doc.fm.updated.as_deref().and_then(util::parse_iso).map_or(ts, |h| h.max(ts));
    let plain = render::plain_text(&doc.body);
    let excerpt = util::clip_chars(&plain.split('\n').collect::<Vec<_>>().join(" · "), 200);
    Note {
        map: map.to_string(),
        id: id.to_string(),
        folded_title: fold(&title),
        folded_tags: doc.fm.tags.iter().map(|t| fold(t)).collect(),
        folded_plain: fold(&plain),
        preview: render::preview(&doc.body, 10, 60),
        excerpt,
        plain,
        title,
        version: hex64(fnv1a64(text.as_bytes())),
        updated_ts,
        text,
        doc,
    }
}

fn map_text(title: &str, team: bool) -> String {
    let doc = Document { fm: FrontMatter { title: Some(title.into()), order: team.then_some(0), ..Default::default() }, body: String::new() };
    note::render(&doc)
}

/// Le titre d'une map par défaut : celui de la map de l'équipe se traduit, les maps
/// du jeu sont des noms propres.
fn default_title(id: &str, title: &'static str, lang: Lang) -> &'static str {
    if id == TEAM_MAP { lang.pick(title, "General") } else { title }
}

/// La note d'accueil d'un carnet neuf, dans sa langue : (id, texte).
fn welcome_note(lang: Lang) -> (&'static str, String) {
    let doc = Document {
        fm: FrontMatter {
            title: Some(tr!(lang, "Bienvenue dans protocol", "Welcome to protocol").into()),
            kind: Some("note".into()),
            tags: vec![tr!(lang, "aide", "help").into()],
            pin: true,
            updated: Some(util::iso_utc(util::unix_now())),
            ..Default::default()
        },
        body: tr!(lang, include_str!("../web/welcome.md"), include_str!("../web/welcome.en.md")).to_string(),
    };
    (tr!(lang, "bienvenue", "welcome"), note::render(&doc))
}

/// Un chemin d'image dans le carnet (`assets/x.png`, `ascent/plan.png`) : des
/// segments simples, une extension d'image connue.
fn valid_asset(name: &str) -> bool {
    let segs: Vec<&str> = name.split('/').collect();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    (1..=4).contains(&segs.len())
        && segs.iter().all(|s| {
            !s.is_empty() && !s.starts_with('.') && !s.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        })
        && IMAGE_TYPES.iter().any(|(e, _)| *e == ext)
}

fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    IMAGE_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

#[derive(Debug, Clone)]
struct MapRec {
    doc: Document,
    text: String,
    version: String,
}

impl MapRec {
    fn new(text: &str) -> MapRec {
        MapRec { doc: note::parse(text), version: hex64(fnv1a64(text.as_bytes())), text: text.to_string() }
    }
}

#[derive(Debug, Clone)]
struct TrashRec {
    map: String,
    id: Option<String>,
    ts: i64,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AssetLoc {
    at: u64,
    len: u64,
}

/// L'état du carnet en mémoire, tel que le journal le décrit.
#[derive(Default)]
struct State {
    notes: HashMap<(String, String), Arc<Note>>,
    /// Pendant la lecture : la dernière version de chaque note, analysée à la fin
    /// seulement (les versions précédentes du journal ne coûtent rien).
    raw: HashMap<(String, String), (i64, String)>,
    loading: bool,
    maps: HashMap<String, MapRec>,
    assets: HashMap<String, AssetLoc>,
    trash: BTreeMap<String, TrashRec>,
    settings: HashMap<String, String>,
}

impl State {
    fn apply(&mut self, at: u64, payload: &[u8]) -> io::Result<()> {
        for e in carnet::decode(payload)? {
            self.entry(e, at);
        }
        Ok(())
    }

    /// Une opération du journal. Ce qui ne passerait pas les contrôles d'une
    /// écriture (nom bizarre, texte démesuré) est ignoré plutôt qu'accepté.
    fn entry(&mut self, e: Entry<'_>, at: u64) {
        match e {
            Entry::Note { map, id, ts, text } => {
                if !util::is_safe_name(map) || !util::is_safe_name(id) || text.len() > MAX_NOTE_BYTES {
                    return;
                }
                let key = (map.to_string(), id.to_string());
                if self.loading {
                    self.raw.insert(key, (ts, text.to_string()));
                } else {
                    self.notes.insert(key, Arc::new(build_note(map, id, text.to_string(), ts)));
                }
            }
            Entry::NoteDel { map, id } => {
                let key = (map.to_string(), id.to_string());
                self.raw.remove(&key);
                self.notes.remove(&key);
            }
            Entry::Map { id, text } => {
                if util::is_safe_name(id) && !id.eq_ignore_ascii_case(ASSETS_DIR) {
                    self.maps.insert(id.to_string(), MapRec::new(text));
                }
            }
            Entry::MapDel { id } => {
                self.maps.remove(id);
            }
            Entry::Asset { name, at: rel, len } => {
                if valid_asset(name) {
                    self.assets.insert(name.to_string(), AssetLoc { at: at + rel as u64, len: len as u64 });
                }
            }
            Entry::AssetDel { name } => {
                self.assets.remove(name);
            }
            Entry::Trash { key, map, id, ts, text } => {
                let rec = TrashRec { map: map.to_string(), id: (!id.is_empty()).then(|| id.to_string()), ts, text: text.to_string() };
                self.trash.insert(key.to_string(), rec);
            }
            Entry::TrashDel { key } => {
                self.trash.remove(key);
            }
            Entry::Setting { key, value } => {
                if value.is_empty() {
                    self.settings.remove(key);
                } else {
                    self.settings.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    fn finish_load(&mut self) {
        for ((map, id), (ts, text)) in self.raw.drain() {
            let n = build_note(&map, &id, text, ts);
            self.notes.insert((map, id), Arc::new(n));
        }
        self.loading = false;
    }

    /// Ce qu'occuperait l'état courant, réécrit seul : de quoi juger le journal.
    fn live_bytes(&self) -> u64 {
        let notes: usize = self.notes.values().map(|n| n.text.len() + n.map.len() + n.id.len() + 40).sum();
        let maps: usize = self.maps.iter().map(|(k, m)| k.len() + m.text.len() + 30).sum();
        let trash: usize = self.trash.iter().map(|(k, t)| k.len() + t.map.len() + t.text.len() + 60).sum();
        let settings: usize = self.settings.iter().map(|(k, v)| k.len() + v.len() + 30).sum();
        let assets: u64 = self.assets.iter().map(|(k, a)| a.len + k.len() as u64 + 30).sum();
        (notes + maps + trash + settings) as u64 + assets + carnet::HEADER_LEN
    }

    /// L'état courant en enregistrements d'environ 1 Mo (les images à part).
    fn snapshot(&self) -> (Vec<Vec<u8>>, Vec<(String, AssetLoc)>) {
        let mut ops: Vec<Op> = Vec::new();
        let mut maps: Vec<(&String, &MapRec)> = self.maps.iter().collect();
        maps.sort_by_key(|(k, _)| k.as_str());
        for (id, m) in maps {
            ops.push(Op::Map { id: id.clone(), text: m.text.clone() });
        }
        let mut notes: Vec<&Arc<Note>> = self.notes.values().collect();
        notes.sort_by(|a, b| (&a.map, &a.id).cmp(&(&b.map, &b.id)));
        for n in notes {
            ops.push(Op::Note { map: n.map.clone(), id: n.id.clone(), ts: n.updated_ts, text: n.text.clone() });
        }
        for (key, t) in &self.trash {
            ops.push(Op::Trash { key: key.clone(), map: t.map.clone(), id: t.id.clone().unwrap_or_default(), ts: t.ts, text: t.text.clone() });
        }
        let mut settings: Vec<(&String, &String)> = self.settings.iter().collect();
        settings.sort();
        for (k, v) in settings {
            ops.push(Op::Setting { key: k.clone(), value: v.clone() });
        }
        let mut records = Vec::new();
        let mut batch: Vec<Op> = Vec::new();
        let mut size = 0;
        for op in ops {
            size += match &op {
                Op::Note { text, .. } | Op::Map { text, .. } | Op::Trash { text, .. } => text.len() + 64,
                _ => 64,
            };
            batch.push(op);
            if size >= 1 << 20 {
                records.push(carnet::encode(&batch));
                batch.clear();
                size = 0;
            }
        }
        if !batch.is_empty() {
            records.push(carnet::encode(&batch));
        }
        let mut assets: Vec<(String, AssetLoc)> = self.assets.iter().map(|(k, v)| (k.clone(), *v)).collect();
        assets.sort_by_key(|(_, a)| a.at);
        (records, assets)
    }
}

type Compacted = io::Result<(Writer, HashMap<String, AssetLoc>)>;

struct Compaction {
    job: JoinHandle<Compacted>,
    /// La fin du journal au moment de l'instantané.
    upto: u64,
}

pub struct Store {
    journal: Journal,
    state: State,
    pub read_only: bool,
    maps: Vec<MapInfo>,
    sorted: Vec<Arc<Note>>,
    /// Identifiant de ce lancement : un ETag d'une session précédente ne resservira pas.
    pub boot: String,
    /// Augmente à chaque changement.
    pub generation: u64,
    followed: Option<Instant>,
    compaction: Option<Compaction>,
    /// Après un compactage manqué, on attend que le journal grossisse encore.
    retry_at: u64,
}

/// Un carnet neuf : les maps par défaut et la note de bienvenue, dans la langue `lang`.
fn create_new(path: &Path, lang: Lang) -> io::Result<()> {
    let mut ops: Vec<Op> = DEFAULT_MAPS
        .iter()
        .map(|(id, title)| Op::Map { id: (*id).into(), text: map_text(default_title(id, title, lang), *id == TEAM_MAP) })
        .collect();
    let (welcome, text) = welcome_note(lang);
    ops.push(Op::Note { map: TEAM_MAP.into(), id: welcome.into(), ts: util::unix_now(), text });
    ops.push(Op::Setting { key: SETTING_DEFAULTS.into(), value: DEFAULT_MAPS.iter().map(|(id, _)| *id).collect::<Vec<_>>().join(",") });
    let mut w = Writer::create(path)?;
    if let Err(e) = w.record(&carnet::encode(&ops)) {
        w.discard();
        return Err(e);
    }
    w.install(path)
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or_else(util::unix_now)
}

/// `retake-b.20261012T101010Z.md` (ou `-2.md`) → (`retake-b`, horodatage).
fn legacy_trash_name(name: &str) -> Option<(&str, i64)> {
    let (id, stamp) = name.strip_suffix(".md")?.rsplit_once('.')?;
    let compact = stamp.split('-').next().unwrap_or(stamp);
    let ts = (compact.len() == 16 && compact.is_ascii())
        .then(|| {
            format!("{}-{}-{}T{}:{}:{}Z", &compact[0..4], &compact[4..6], &compact[6..8], &compact[9..11], &compact[11..13], &compact[13..15])
        })
        .as_deref()
        .and_then(util::parse_iso)
        .unwrap_or(0);
    Some((id, ts))
}

fn sorted_dir(dir: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut v: Vec<fs::DirEntry> = fs::read_dir(dir)?.flatten().collect();
    v.sort_by_key(|e| e.file_name());
    Ok(v)
}

/// Reprend un ancien dossier de notes (une map = un dossier, une note = un `.md`)
/// dans un carnet neuf. Le dossier n'est jamais modifié.
fn import_legacy(path: &Path, dir: &Path) -> io::Result<()> {
    let mut w = Writer::create(path)?;
    let res = (|| -> io::Result<(usize, usize, usize)> {
        let (mut maps, mut notes, mut images) = (0, 0, 0);
        let put = |w: &mut Writer, op: Op| w.record(&carnet::encode(&[op])).map(|_| ());
        for entry in sorted_dir(dir)? {
            let Ok(name) = entry.file_name().into_string() else { continue };
            // `file_type` ne suit pas les liens : rien hors du dossier n'est lu.
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if name == ASSETS_DIR {
                for f in sorted_dir(&entry.path())? {
                    let Ok(file) = f.file_name().into_string() else { continue };
                    let key = format!("{ASSETS_DIR}/{file}");
                    if f.file_type().is_ok_and(|t| t.is_file()) && valid_asset(&key) {
                        put(&mut w, Op::Asset { name: key, data: fs::read(f.path())? })?;
                        images += 1;
                    }
                }
                continue;
            }
            if name == LEGACY_TRASH_DIR {
                for m in sorted_dir(&entry.path())? {
                    let Ok(map) = m.file_name().into_string() else { continue };
                    if !util::is_safe_name(&map) || !m.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    for f in sorted_dir(&m.path())? {
                        let Ok(file) = f.file_name().into_string() else { continue };
                        let Some((id, ts)) = legacy_trash_name(&file) else { continue };
                        if !f.file_type().is_ok_and(|t| t.is_file()) || (id != "_map" && !util::is_safe_name(id)) {
                            continue;
                        }
                        let text = util::decode_text(&fs::read(f.path())?);
                        let key = format!("{map}/{}", file.trim_end_matches(".md"));
                        let id = if id == "_map" { String::new() } else { id.to_string() };
                        put(&mut w, Op::Trash { key, map: map.clone(), id, ts, text })?;
                    }
                }
                continue;
            }
            if !util::is_safe_name(&name) || name.eq_ignore_ascii_case(ASSETS_DIR) {
                continue;
            }
            let mdir = entry.path();
            let map_file = mdir.join(LEGACY_MAP_FILE);
            let text = if map_file.is_file() { util::decode_text(&fs::read(&map_file)?) } else { map_text(&capitalize(&name), name == TEAM_MAP) };
            put(&mut w, Op::Map { id: name.clone(), text })?;
            // « général » (les règles d'équipe) ne compte pas parmi les maps.
            maps += (name != TEAM_MAP) as usize;
            for f in sorted_dir(&mdir)? {
                let Ok(file) = f.file_name().into_string() else { continue };
                let Ok(meta) = f.metadata() else { continue };
                if !meta.is_file() || file == LEGACY_MAP_FILE {
                    continue;
                }
                if let Some(id) = file.strip_suffix(".md") {
                    if util::is_safe_name(id) && meta.len() as usize <= MAX_NOTE_BYTES {
                        let text = util::decode_text(&fs::read(f.path())?);
                        put(&mut w, Op::Note { map: name.clone(), id: id.to_string(), ts: mtime_of(&meta), text })?;
                        notes += 1;
                    }
                } else {
                    // Une image rangée à côté des notes (`![](plan.png)`) garde son chemin.
                    let key = format!("{name}/{file}");
                    if valid_asset(&key) && meta.len() <= 16 << 20 {
                        put(&mut w, Op::Asset { name: key, data: fs::read(f.path())? })?;
                        images += 1;
                    }
                }
            }
        }
        // Les maps que proposait l'ancienne version sont déjà là (ou ont été retirées
        // exprès) : seules les nouvelles (Summit) seront ajoutées à l'ouverture.
        put(&mut w, Op::Setting { key: SETTING_DEFAULTS.into(), value: FIRST_DEFAULTS.join(",") })?;
        Ok((maps, notes, images))
    })();
    match res {
        Ok((maps, notes, images)) => {
            let shown = crate::platform::display_path(dir);
            let what = [
                (notes > 0).then(|| if notes > 1 { format!("{notes} notes") } else { "1 note".into() }),
                (maps > 0).then(|| if maps > 1 { format!("{maps} maps") } else { "1 map".into() }),
                (images > 0).then(|| if images > 1 { format!("{images} images") } else { "1 image".into() }),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            let notice = format!(
                "Le contenu de l'ancien dossier de notes ({what}) a été repris dans {}. Ce dossier n'est plus utilisé et n'a pas été modifié : \
                 vous pouvez le supprimer une fois vos notes vérifiées ({shown}).",
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            );
            if let Err(e) = w.record(&carnet::encode(&[Op::Setting { key: SETTING_NOTICE.into(), value: notice }])) {
                w.discard();
                return Err(e);
            }
            w.install(path)
        }
        Err(e) => {
            w.discard();
            Err(e)
        }
    }
}

impl Store {
    /// Ouvre le carnet ; s'il n'existe pas encore (et qu'on peut écrire), en crée un neuf.
    pub fn open(path: &Path, read_only: bool) -> io::Result<Store> {
        Self::open_or_import(path, read_only, None)
    }

    /// Comme `open`, mais un carnet absent est d'abord rempli avec l'ancien dossier
    /// de notes `legacy` s'il existe (ce dossier n'est jamais modifié).
    pub fn open_or_import(path: &Path, read_only: bool, legacy: Option<&Path>) -> io::Result<Store> {
        Self::open_in(path, read_only, legacy, i18n::resolve(None))
    }

    /// Comme `open_or_import` ; un carnet créé ici l'est dans la langue `lang` (note
    /// d'accueil, nom de la map de l'équipe). Un carnet existant ne change pas.
    pub fn open_in(path: &Path, read_only: bool, legacy: Option<&Path>, lang: Lang) -> io::Result<Store> {
        if !path.exists() {
            if read_only {
                return Err(io::Error::new(io::ErrorKind::NotFound, "aucun carnet à cet endroit"));
            }
            match legacy.filter(|d| d.is_dir()) {
                Some(dir) => import_legacy(path, dir)?,
                None => create_new(path, lang)?,
            }
        }
        let (journal, state, recovery) = Self::load(path, read_only)?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let mut store = Store {
            journal,
            state,
            read_only,
            maps: Vec::new(),
            sorted: Vec::new(),
            boot: hex64(fnv1a64(&(nanos ^ std::process::id() as u128).to_le_bytes()))[..8].to_string(),
            generation: 1,
            followed: None,
            compaction: None,
            retry_at: 0,
        };
        store.rebuild();
        if !read_only {
            store.ensure_defaults().map_err(|e| io::Error::other(e.to_string()))?;
            if let Some(msg) = recovery_notice(&recovery) {
                store.set_setting(SETTING_NOTICE, &msg).map_err(|e| io::Error::other(e.to_string()))?;
            }
            if recovery.skipped > 0 {
                // Le passage illisible ne doit pas être relu à chaque lancement.
                store.compact_now()?;
            }
            store.maybe_compact();
        }
        Ok(store)
    }

    fn load(path: &Path, read_only: bool) -> io::Result<(Journal, State, Recovery)> {
        let mut state = State { loading: true, ..Default::default() };
        let (journal, recovery) = Journal::open(path, !read_only, |at, p| state.apply(at, p))?;
        state.finish_load();
        Ok((journal, state, recovery))
    }

    /// Le fichier du carnet.
    pub fn path(&self) -> &Path {
        self.journal.path()
    }

    /// Taille du fichier du carnet, en octets.
    pub fn size(&self) -> u64 {
        self.journal.end()
    }

    fn writable(&self) -> Result<()> {
        if self.read_only { Err(StoreError::ReadOnly) } else { Ok(()) }
    }

    /// Écrit un enregistrement, puis l'applique comme à la lecture du journal.
    fn commit(&mut self, ops: Vec<Op>) -> Result<()> {
        self.writable()?;
        self.poll();
        let payload = carnet::encode(&ops);
        let at = self.journal.append(&payload)?;
        self.state.apply(at, &payload)?;
        self.rebuild();
        self.generation += 1;
        self.maybe_compact();
        Ok(())
    }

    /// Lecture seule : reprend ce qu'un autre programme a écrit depuis (au plus une
    /// fois par seconde, sauf `force`). Sans effet sur le carnet qu'on écrit soi-même.
    pub fn refresh(&mut self, force: bool) -> io::Result<()> {
        if !self.read_only || (!force && self.followed.is_some_and(|t| t.elapsed() < Duration::from_secs(1))) {
            return Ok(());
        }
        self.followed = Some(Instant::now());
        let mut changed = false;
        let Store { journal, state, .. } = self;
        let same_file = journal.follow(|at, p| {
            changed = true;
            state.apply(at, p)
        })?;
        if !same_file {
            let (journal, state, _) = Self::load(self.journal.path(), true)?;
            self.journal = journal;
            self.state = state;
            changed = true;
        }
        if changed {
            self.rebuild();
            self.generation += 1;
        }
        Ok(())
    }

    /// Les maps de la liste par défaut que ce carnet n'a encore jamais proposées.
    fn ensure_defaults(&mut self) -> Result<()> {
        let offered: Vec<String> =
            self.state.settings.get(SETTING_DEFAULTS).map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default();
        let missing: Vec<&(&str, &str)> = DEFAULT_MAPS.iter().filter(|(id, _)| !offered.iter().any(|o| o == id)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let lang = self.lang();
        let mut ops: Vec<Op> = missing
            .iter()
            .filter(|(id, _)| !self.state.maps.contains_key(*id))
            .map(|(id, title)| Op::Map { id: (*id).into(), text: map_text(default_title(id, title, lang), *id == TEAM_MAP) })
            .collect();
        let mut all = offered;
        all.extend(missing.iter().map(|(id, _)| id.to_string()));
        ops.push(Op::Setting { key: SETTING_DEFAULTS.into(), value: all.join(",") });
        self.commit(ops)
    }

    fn rebuild(&mut self) {
        let mut sorted: Vec<Arc<Note>> = self.state.notes.values().cloned().collect();
        let kind_rank = |k: &str| note::KINDS.iter().position(|x| x.id == k).unwrap_or(note::KINDS.len());
        sorted.sort_by(|a, b| {
            (&a.map, !a.doc.fm.pin, kind_rank(a.kind()), &a.folded_title, &a.id).cmp(&(
                &b.map,
                !b.doc.fm.pin,
                kind_rank(b.kind()),
                &b.folded_title,
                &b.id,
            ))
        });
        // Une note dont la map manque (carnet abîmé) reste visible : sa map est recréée à l'affichage.
        let mut ids: HashSet<&str> = self.state.maps.keys().map(String::as_str).collect();
        ids.extend(sorted.iter().map(|n| n.map.as_str()));
        let empty = MapRec::new("");
        let mut maps: Vec<MapInfo> = ids
            .into_iter()
            .map(|id| {
                let rec = self.state.maps.get(id).unwrap_or(&empty);
                let (mut count, mut pinned, mut updated_ts) = (0, 0, 0);
                for n in sorted.iter().filter(|n| n.map == id) {
                    count += 1;
                    pinned += n.doc.fm.pin as usize;
                    updated_ts = updated_ts.max(n.updated_ts);
                }
                MapInfo {
                    id: id.to_string(),
                    title: rec.doc.fm.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| capitalize(id)),
                    pool: rec.doc.fm.pool,
                    order: rec.doc.fm.order,
                    doc: rec.doc.clone(),
                    version: rec.version.clone(),
                    count,
                    pinned,
                    updated_ts,
                }
            })
            .collect();
        maps.sort_by(|a, b| {
            let key = |m: &MapInfo| (m.id != TEAM_MAP, m.order.unwrap_or(i64::MAX), fold(&m.title));
            key(a).cmp(&key(b))
        });
        self.sorted = sorted;
        self.maps = maps;
    }

    pub fn maps(&self) -> &[MapInfo] {
        &self.maps
    }

    pub fn map(&self, id: &str) -> Option<&MapInfo> {
        self.maps.iter().find(|m| m.id == id)
    }

    /// Toutes les notes, triées par map puis épinglées, type et titre.
    pub fn notes(&self) -> &[Arc<Note>] {
        &self.sorted
    }

    pub fn notes_of<'a>(&'a self, map: &'a str) -> impl Iterator<Item = &'a Arc<Note>> {
        self.sorted.iter().filter(move |n| n.map == map)
    }

    pub fn note(&self, map: &str, id: &str) -> Option<Arc<Note>> {
        self.state.notes.get(&(map.to_string(), id.to_string())).cloned()
    }

    /// Une map du carnet, sous son nom exact.
    fn check_map(&self, map: &str) -> Result<()> {
        if !util::is_safe_name(map) || map.eq_ignore_ascii_case(ASSETS_DIR) {
            return Err(StoreError::Invalid(format!("nom de map invalide : {map}")));
        }
        if !self.state.maps.contains_key(map) && self.map(map).is_none() {
            return Err(StoreError::NotFound(format!("map introuvable : {map}")));
        }
        Ok(())
    }

    /// Un nom libre dans la map, sans tenir compte de la casse : l'export en
    /// fichiers ne doit pas produire deux noms que Windows confondrait.
    fn taken(&self, map: &str, id: &str) -> bool {
        self.state.notes.keys().any(|(m, i)| m == map && i.eq_ignore_ascii_case(id))
    }

    fn unique_id(&self, map: &str, base: &str) -> String {
        let mut id = base.to_string();
        let mut n = 2;
        while self.taken(map, &id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    fn apply(fm: &mut FrontMatter, body: &mut String, input: &NoteInput) {
        if let Some(t) = &input.title {
            let t = t.trim();
            fm.title = (!t.is_empty()).then(|| t.chars().take(200).collect());
        }
        if let Some(k) = &input.kind {
            fm.kind = Some(note::normalize_kind(k)).filter(|k| !k.is_empty());
        }
        if let Some(s) = &input.side {
            fm.side = Some(note::normalize_side(s)).filter(|s| !s.is_empty());
        }
        if let Some(t) = &input.tags {
            fm.tags = note::clean_tags(t.iter().cloned());
        }
        if let Some(p) = input.pin {
            fm.pin = p;
        }
        if let Some(b) = &input.body {
            *body = b.replace("\r\n", "\n");
        }
        fm.updated = Some(util::iso_utc(util::unix_now()));
    }

    fn put_note(&mut self, map: &str, id: &str, text: String) -> Result<Arc<Note>> {
        if text.len() > MAX_NOTE_BYTES {
            return Err(StoreError::Invalid("note trop longue (4 Mo au plus)".into()));
        }
        self.commit(vec![Op::Note { map: map.into(), id: id.into(), ts: util::unix_now(), text }])?;
        self.note(map, id).ok_or_else(|| StoreError::NotFound(id.to_string()))
    }

    pub fn create_note(&mut self, map: &str, input: NoteInput) -> Result<Arc<Note>> {
        self.writable()?;
        self.check_map(map)?;
        // Le titre par défaut et le modèle du type : dans la langue du carnet.
        let lang = self.lang();
        let title = input.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(tr!(lang, "Nouvelle note", "New note")).to_string();
        let id = self.unique_id(map, &util::slugify(&title));
        let kind = input.kind.as_deref().map(note::normalize_kind).filter(|k| !k.is_empty()).unwrap_or_else(|| "note".into());
        let template = note::kind(&kind).map(|k| k.template(lang)).unwrap_or("");
        let mut doc = Document { fm: FrontMatter::default(), body: template.to_string() };
        let input = NoteInput { title: Some(title), kind: Some(kind), map: None, ..input };
        Self::apply(&mut doc.fm, &mut doc.body, &input);
        self.put_note(map, &id, note::render(&doc))
    }

    /// Crée une note à partir d'un fichier Markdown venu d'ailleurs (Obsidian, un
    /// autre carnet, un export) : son en-tête est repris tel quel, clés inconnues
    /// comprises. Le titre vient de l'en-tête, sinon du `# Titre` qui ouvre le texte
    /// (retiré du corps : la page l'affiche déjà), sinon de `hint` (le nom du fichier).
    /// Jamais d'écrasement : le nom est rendu unique dans la map.
    pub fn import_note(&mut self, map: &str, text: &str, hint: Option<&str>) -> Result<Arc<Note>> {
        self.writable()?;
        self.check_map(map)?;
        let mut doc = note::parse(text);
        if doc.fm.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
            doc.fm.title = match first_heading_at(&doc.body) {
                Some((end, t)) => {
                    doc.body = doc.body[end..].trim_start_matches(['\n', '\r']).to_string();
                    Some(t)
                }
                None => hint.map(str::trim).filter(|h| !h.is_empty()).map(str::to_string),
            };
        }
        let fallback = tr!(self.lang(), "Note importée", "Imported note");
        let title: String = doc.fm.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(fallback).chars().take(200).collect();
        doc.fm.title = Some(title.clone());
        doc.fm.kind = doc.fm.kind.as_deref().map(note::normalize_kind).filter(|k| !k.is_empty());
        doc.fm.side = doc.fm.side.as_deref().map(note::normalize_side).filter(|s| !s.is_empty());
        doc.fm.updated = Some(util::iso_utc(util::unix_now()));
        let id = self.unique_id(map, &util::slugify(&title));
        self.put_note(map, &id, note::render(&doc))
    }

    /// Enregistre une note. `base` = la version que le client avait chargée : si la
    /// note a changé depuis (autre fenêtre), on refuse plutôt qu'écraser.
    pub fn save_note(&mut self, map: &str, id: &str, input: NoteInput, base: Option<&str>) -> Result<Arc<Note>> {
        self.writable()?;
        if !util::is_safe_name(id) {
            return Err(StoreError::Invalid(format!("nom de note invalide : {id}")));
        }
        self.check_map(map)?;
        let cur = self.note(map, id).ok_or_else(|| StoreError::NotFound(format!("note introuvable : {map}/{id}")))?;
        // Déplacement demandé : la map cible doit exister avant qu'on écrive quoi que ce soit.
        let target = input.map.as_deref().map(str::trim).filter(|m| !m.is_empty() && *m != map).map(str::to_string);
        if let Some(t) = &target {
            self.check_map(t)?;
        }
        if base.is_some_and(|b| b != cur.version) {
            return Err(StoreError::Conflict(cur));
        }
        let mut doc = cur.doc.clone();
        Self::apply(&mut doc.fm, &mut doc.body, &input);
        let text = note::render(&doc);
        match target {
            None => self.put_note(map, id, text),
            Some(t) => {
                // L'arrivée et le départ dans le même enregistrement : jamais deux exemplaires, jamais zéro.
                let new_id = self.unique_id(&t, id);
                self.commit(vec![Op::Note { map: t.clone(), id: new_id.clone(), ts: util::unix_now(), text }, Op::NoteDel {
                    map: map.into(),
                    id: id.into(),
                }])?;
                self.note(&t, &new_id).ok_or(StoreError::NotFound(new_id))
            }
        }
    }

    /// Remplace (ou crée) une note avec un texte Markdown complet, en-tête compris :
    /// pour les outils externes qui préfèrent envoyer le fichier tel quel.
    pub fn put_raw(&mut self, map: &str, id: &str, text: &str, base: Option<&str>) -> Result<Arc<Note>> {
        self.writable()?;
        if !util::is_safe_name(id) || id.contains('~') {
            return Err(StoreError::Invalid(format!("nom de note refusé : {id}")));
        }
        self.check_map(map)?;
        match self.note(map, id) {
            Some(cur) => {
                if base.is_some_and(|b| b != cur.version) {
                    return Err(StoreError::Conflict(cur));
                }
            }
            None if base.is_some() => return Err(StoreError::NotFound(format!("note introuvable : {map}/{id}"))),
            // Création : pas de second nom que seule la casse distinguerait.
            None if self.taken(map, id) => return Err(StoreError::Invalid(format!("nom de note refusé : {id}"))),
            None => {}
        }
        self.put_note(map, id, text.replace("\r\n", "\n"))
    }

    /// Une copie de sauvegarde pour la corbeille, sans rien retirer du carnet. Son titre
    /// la signale, et une fois restaurée elle ne se confond pas avec la note d'après.
    fn backup_op(&self, map: &str, id: &str, mut doc: Document, title: &str) -> (String, Op) {
        let suffix = tr!(self.lang(), "version d'avant l'assistant", "version before the assistant");
        doc.fm.title = Some(format!("{title} — {suffix}").chars().take(200).collect());
        doc.fm.pin = false;
        let key = self.trash_key(map, &format!("{id}~avant-assistant"));
        let op = Op::Trash { key: key.clone(), map: map.into(), id: id.into(), ts: util::unix_now(), text: note::render(&doc) };
        (key, op)
    }

    /// Comme `save_note` (sans déplacement), mais la version d'avant va d'abord à la
    /// corbeille, dans le même enregistrement : ce qu'un assistant réécrit reste
    /// récupérable. Renvoie la note et la clé de sa copie.
    pub fn save_note_backed_up(&mut self, map: &str, id: &str, input: NoteInput, base: Option<&str>) -> Result<(Arc<Note>, String)> {
        self.writable()?;
        if !util::is_safe_name(id) {
            return Err(StoreError::Invalid(format!("nom de note invalide : {id}")));
        }
        self.check_map(map)?;
        let cur = self.note(map, id).ok_or_else(|| StoreError::NotFound(format!("note introuvable : {map}/{id}")))?;
        if input.map.as_deref().map(str::trim).is_some_and(|m| !m.is_empty() && m != map) {
            return Err(StoreError::Invalid("une note se déplace à part, sans copie de sauvegarde".into()));
        }
        if base.is_some_and(|b| b != cur.version) {
            return Err(StoreError::Conflict(cur));
        }
        let mut doc = cur.doc.clone();
        Self::apply(&mut doc.fm, &mut doc.body, &input);
        let text = note::render(&doc);
        if text.len() > MAX_NOTE_BYTES {
            return Err(StoreError::Invalid("note trop longue (4 Mo au plus)".into()));
        }
        let (key, backup) = self.backup_op(map, id, cur.doc.clone(), &cur.title);
        self.commit(vec![backup, Op::Note { map: map.into(), id: id.into(), ts: util::unix_now(), text }])?;
        let n = self.note(map, id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        Ok((n, key))
    }

    fn trash_key(&self, map: &str, name: &str) -> String {
        let stamp = util::stamp_utc(util::unix_now());
        let mut key = format!("{map}/{name}.{stamp}");
        let mut n = 2;
        while self.state.trash.contains_key(&key) {
            key = format!("{map}/{name}.{stamp}-{n}");
            n += 1;
        }
        key
    }

    /// Met la note à la corbeille ; renvoie la clé qui permet de la restaurer.
    pub fn trash_note(&mut self, map: &str, id: &str) -> Result<String> {
        self.writable()?;
        let cur = self.note(map, id).ok_or_else(|| StoreError::NotFound(format!("note introuvable : {map}/{id}")))?;
        let key = self.trash_key(map, id);
        self.commit(vec![
            Op::Trash { key: key.clone(), map: map.into(), id: id.into(), ts: util::unix_now(), text: cur.text.clone() },
            Op::NoteDel { map: map.into(), id: id.into() },
        ])?;
        Ok(key)
    }

    pub fn trash(&self) -> Vec<TrashItem> {
        let overview = tr!(self.lang(), "présentation de", "overview of");
        let mut items: Vec<TrashItem> = self
            .state
            .trash
            .iter()
            .map(|(key, t)| {
                let title = match &t.id {
                    Some(id) => title_of(&note::parse(&t.text), id),
                    None => format!("{overview} {}", self.map(&t.map).map_or_else(|| title_of(&note::parse(&t.text), &t.map), |m| m.title.clone())),
                };
                TrashItem { path: key.clone(), map: t.map.clone(), id: t.id.clone(), title, deleted_ts: t.ts }
            })
            .collect();
        items.sort_by(|a, b| b.deleted_ts.cmp(&a.deleted_ts).then(a.path.cmp(&b.path)));
        items
    }

    pub fn trash_count(&self) -> usize {
        self.state.trash.len()
    }

    /// Sort un élément de la corbeille. Renvoie (map, id) — l'id change si la place
    /// est prise ; `None` pour la présentation d'une map.
    pub fn restore(&mut self, key: &str) -> Result<(String, Option<String>)> {
        self.writable()?;
        let rec = self.state.trash.get(key).cloned().ok_or_else(|| StoreError::NotFound("élément introuvable dans la corbeille".into()))?;
        let mut ops = Vec::new();
        match &rec.id {
            Some(id) => {
                // Sa map a disparu depuis : elle revient avec la note.
                if !self.state.maps.contains_key(&rec.map) {
                    ops.push(Op::Map { id: rec.map.clone(), text: map_text(&capitalize(&rec.map), rec.map == TEAM_MAP) });
                }
                let new_id = self.unique_id(&rec.map, id);
                ops.push(Op::Note { map: rec.map.clone(), id: new_id.clone(), ts: util::unix_now(), text: rec.text });
                ops.push(Op::TrashDel { key: key.into() });
                self.commit(ops)?;
                Ok((rec.map, Some(new_id)))
            }
            None => {
                if self.state.maps.contains_key(&rec.map) {
                    return Err(StoreError::Invalid("cette map existe déjà".into()));
                }
                ops.push(Op::Map { id: rec.map.clone(), text: rec.text });
                ops.push(Op::TrashDel { key: key.into() });
                self.commit(ops)?;
                Ok((rec.map, None))
            }
        }
    }

    pub fn create_map(&mut self, title: &str) -> Result<MapInfo> {
        self.writable()?;
        let title = title.trim();
        if title.is_empty() {
            return Err(StoreError::Invalid("donnez un nom à la map".into()));
        }
        let id = util::slugify(title);
        if id == ASSETS_DIR {
            return Err(StoreError::Invalid("ce nom est réservé".into()));
        }
        if self.state.maps.keys().any(|k| k.eq_ignore_ascii_case(&id)) {
            return Err(StoreError::Invalid(format!("la map « {id} » existe déjà")));
        }
        let title: String = title.chars().take(60).collect();
        self.commit(vec![Op::Map { id: id.clone(), text: map_text(&title, false) }])?;
        self.map(&id).cloned().ok_or(StoreError::NotFound(id))
    }

    /// Titre, appartenance au pool, présentation : `None` = inchangé.
    pub fn save_map(
        &mut self,
        map: &str,
        title: Option<&str>,
        pool: Option<Option<bool>>,
        overview: Option<&str>,
        base: Option<&str>,
    ) -> Result<MapInfo> {
        self.writable()?;
        self.check_map(map)?;
        let cur = self.map(map).cloned().ok_or_else(|| StoreError::NotFound(format!("map introuvable : {map}")))?;
        if base.is_some_and(|b| b != cur.version) {
            return Err(StoreError::MapConflict(Box::new(cur)));
        }
        let mut doc = cur.doc.clone();
        if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
            doc.fm.title = Some(t.chars().take(60).collect());
        }
        if let Some(p) = pool {
            doc.fm.pool = p;
        }
        if let Some(o) = overview {
            doc.body = o.replace("\r\n", "\n");
        }
        if doc.fm.title.is_none() {
            doc.fm.title = Some(capitalize(map));
        }
        self.commit(vec![Op::Map { id: map.into(), text: note::render(&doc) }])?;
        self.map(map).cloned().ok_or_else(|| StoreError::NotFound(map.to_string()))
    }

    /// Comme `save_map`, mais une présentation réécrite laisse la précédente à la
    /// corbeille (en note de la map, à restaurer à côté), dans le même enregistrement.
    /// Renvoie la map, et la clé de la copie s'il y en a une.
    pub fn save_map_backed_up(
        &mut self,
        map: &str,
        title: Option<&str>,
        pool: Option<Option<bool>>,
        overview: Option<&str>,
        base: Option<&str>,
    ) -> Result<(MapInfo, Option<String>)> {
        let Some(overview) = overview else {
            return self.save_map(map, title, pool, None, base).map(|m| (m, None));
        };
        self.writable()?;
        self.check_map(map)?;
        let cur = self.map(map).cloned().ok_or_else(|| StoreError::NotFound(format!("map introuvable : {map}")))?;
        if base.is_some_and(|b| b != cur.version) {
            return Err(StoreError::MapConflict(Box::new(cur)));
        }
        let old = Document { fm: FrontMatter::default(), body: cur.doc.body.clone() };
        let what = tr!(self.lang(), "Présentation de", "Overview of");
        let (key, backup) = self.backup_op(map, "presentation", old, &format!("{what} {}", cur.title));
        let mut doc = cur.doc.clone();
        if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
            doc.fm.title = Some(t.chars().take(60).collect());
        }
        if let Some(p) = pool {
            doc.fm.pool = p;
        }
        doc.body = overview.replace("\r\n", "\n");
        if doc.fm.title.is_none() {
            doc.fm.title = Some(capitalize(map));
        }
        self.commit(vec![backup, Op::Map { id: map.into(), text: note::render(&doc) }])?;
        let m = self.map(map).cloned().ok_or_else(|| StoreError::NotFound(map.to_string()))?;
        Ok((m, Some(key)))
    }

    /// Supprime une map **vide** : sa présentation part à la corbeille.
    pub fn delete_map(&mut self, map: &str) -> Result<()> {
        self.writable()?;
        self.check_map(map)?;
        if self.notes_of(map).next().is_some() {
            return Err(StoreError::Invalid("la map contient encore des notes".into()));
        }
        let text = self.state.maps.get(map).map(|m| m.text.clone()).unwrap_or_else(|| map_text(&capitalize(map), false));
        let key = self.trash_key(map, "_map");
        self.commit(vec![
            Op::Trash { key, map: map.into(), id: String::new(), ts: util::unix_now(), text },
            Op::MapDel { id: map.into() },
        ])
    }

    /// Range une image sous le nom de son empreinte (les doublons ne prennent pas
    /// de place). Renvoie son nom (`3f2a….png`), à citer `../assets/<nom>`.
    pub fn save_asset(&mut self, bytes: &[u8], ext: &str) -> Result<String> {
        self.writable()?;
        if !IMAGE_TYPES.iter().any(|(e, _)| *e == ext) {
            return Err(StoreError::Invalid(format!("type d'image refusé : {ext}")));
        }
        let hash = hex64(fnv1a64(bytes));
        for n in 1..1000 {
            let name = if n == 1 { format!("{hash}.{ext}") } else { format!("{hash}-{n}.{ext}") };
            let key = format!("{ASSETS_DIR}/{name}");
            match self.state.assets.get(&key).copied() {
                Some(loc) if loc.len == bytes.len() as u64 && self.journal.read_at(loc.at, loc.len)? == bytes => return Ok(name),
                // Même empreinte, autre image : on ne la remplace pas, on prend le nom suivant.
                Some(_) => continue,
                None => {
                    self.commit(vec![Op::Asset { name: key, data: bytes.to_vec() }])?;
                    return Ok(name);
                }
            }
        }
        Err(StoreError::Invalid("impossible de nommer l'image".into()))
    }

    /// Une image du carnet (`assets/x.png`, chemin d'URL encodé) et son type.
    pub fn asset(&mut self, rel: &str) -> Option<(Vec<u8>, &'static str)> {
        let name = rel.split('/').map(|s| util::percent_decode(s, false)).collect::<Option<Vec<_>>>()?.join("/");
        if !valid_asset(&name) {
            return None;
        }
        let loc = *self.state.assets.get(&name)?;
        let mime = mime_of(&name)?;
        self.journal.read_at(loc.at, loc.len).ok().map(|b| (b, mime))
    }

    pub fn setting(&self, key: &str) -> Option<&str> {
        self.state.settings.get(key).map(String::as_str)
    }

    /// La langue du carnet : son réglage `lang`, sinon celle du système.
    pub fn lang(&self) -> Lang {
        i18n::resolve(self.setting(i18n::SETTING))
    }

    /// Enregistre un réglage (vide : l'efface). Sans effet s'il ne change pas.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        if self.setting(key).unwrap_or("") == value {
            return Ok(());
        }
        self.commit(vec![Op::Setting { key: key.into(), value: value.into() }])
    }

    /// Un message à montrer une fois (ancien dossier repris, carnet réparé…).
    pub fn notice(&self) -> Option<&str> {
        self.setting(SETTING_NOTICE)
    }

    pub fn clear_notice(&mut self) -> Result<()> {
        self.set_setting(SETTING_NOTICE, "")
    }

    /// Tout le carnet en fichiers Markdown, dans une archive ZIP : une map = un
    /// dossier, une note = un `.md`, les images dans `assets/`. De quoi l'ouvrir dans
    /// Obsidian ou le garder en sauvegarde lisible partout.
    pub fn export_zip(&mut self, top: &str) -> io::Result<Vec<u8>> {
        let mut z = Zip::new(util::unix_now());
        for m in &self.maps {
            let text = self.state.maps.get(&m.id).map(|r| r.text.as_str()).unwrap_or("");
            z.add(&format!("{top}/{}/{LEGACY_MAP_FILE}", m.id), text.as_bytes())?;
        }
        for n in &self.sorted {
            z.add(&format!("{top}/{}/{}.md", n.map, n.id), n.text.as_bytes())?;
        }
        let mut assets: Vec<(String, AssetLoc)> = self.state.assets.iter().map(|(k, v)| (k.clone(), *v)).collect();
        assets.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, loc) in assets {
            let data = self.journal.read_at(loc.at, loc.len)?;
            z.add(&format!("{top}/{name}"), &data)?;
        }
        z.finish()
    }

    // ── Compactage ────────────────────────────────────────────────────────────

    /// Le journal contient-il assez de versions périmées pour être réécrit ?
    fn maybe_compact(&mut self) {
        if self.read_only || self.compaction.is_some() || self.journal.end() < self.retry_at {
            return;
        }
        let live = self.state.live_bytes();
        let garbage = self.journal.end().saturating_sub(live);
        if garbage >= (live / 2).max(COMPACT_MIN) {
            self.start_compaction();
        }
    }

    /// L'instantané est pris ici ; l'écriture du fichier neuf (images comprises) se
    /// fait dans un autre fil, sans bloquer le carnet.
    fn start_compaction(&mut self) {
        let (records, assets) = self.state.snapshot();
        let path = self.path().to_path_buf();
        let upto = self.journal.end();
        let job = std::thread::Builder::new().name("compactage".into()).spawn(move || -> Compacted {
            let mut w = Writer::create(&path)?;
            let res = (|| -> io::Result<HashMap<String, AssetLoc>> {
                for r in &records {
                    w.record(r)?;
                }
                let mut src = carnet::reader(&path)?;
                let mut moved = HashMap::new();
                for (name, loc) in assets {
                    let data = carnet::read_exact_at(&mut src, loc.at, loc.len)?;
                    let at = w.record(&carnet::encode(&[Op::Asset { name: name.clone(), data }]))?;
                    moved.insert(name.clone(), AssetLoc { at: at + carnet::asset_data_offset(&name), len: loc.len });
                }
                Ok(moved)
            })();
            match res {
                Ok(moved) => Ok((w, moved)),
                Err(e) => {
                    w.discard();
                    Err(e)
                }
            }
        });
        match job {
            Ok(job) => self.compaction = Some(Compaction { job, upto }),
            Err(e) => {
                eprintln!("protocol : compactage impossible ({e})");
                self.retry_at = self.journal.end() + COMPACT_MIN;
            }
        }
    }

    /// Termine un compactage fini (appelé régulièrement, et avant chaque écriture).
    pub fn poll(&mut self) {
        if !self.compaction.as_ref().is_some_and(|c| c.job.is_finished()) {
            return;
        }
        let Some(c) = self.compaction.take() else { return };
        let res = match c.job.join() {
            Ok(Ok((w, moved))) => self.journal.replace(w, c.upto).map(|shift| {
                for (name, loc) in self.state.assets.iter_mut() {
                    if loc.at >= c.upto {
                        loc.at = (loc.at as i64 + shift) as u64;
                    } else if let Some(m) = moved.get(name) {
                        *loc = *m;
                    }
                }
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(io::Error::other("le compactage s'est interrompu")),
        };
        if let Err(e) = res {
            eprintln!("protocol : compactage abandonné ({e}) ; le carnet reste tel quel");
            self.retry_at = self.journal.end() + COMPACT_MIN;
        }
    }

    /// Attend la fin d'un compactage en cours et l'installe : avant de refermer un
    /// carnet ouvert le temps d'une écriture (sinon son fichier neuf resterait à côté).
    pub fn settle(&mut self) {
        while self.compaction.as_ref().is_some_and(|c| !c.job.is_finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.poll();
    }

    /// Compacte tout de suite et attend la fin (réparation, tests).
    pub fn compact_now(&mut self) -> io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        if self.compaction.is_none() {
            self.start_compaction();
        }
        while self.compaction.as_ref().is_some_and(|c| !c.job.is_finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let before = self.journal.end();
        self.poll();
        if self.retry_at > before { Err(io::Error::other("compactage abandonné")) } else { Ok(()) }
    }
}

fn recovery_notice(r: &Recovery) -> Option<String> {
    if r.skipped > 0 {
        let backup = r.backup.as_ref().and_then(|b| b.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some(format!(
            "Une partie du carnet était illisible ({} octets, support abîmé ?) et a été écartée. Une copie du fichier d'origine a été gardée à côté : {backup}.",
            r.skipped
        ))
    } else if r.dropped_tail > 0 {
        Some("protocol a été arrêté pendant un enregistrement : la toute dernière modification n'avait pas pu être écrite en entier et a été écartée.".into())
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("protocol-test-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// Un carnet neuf dans un dossier temporaire.
    pub(crate) fn temp_store(tag: &str) -> (PathBuf, Store) {
        let d = temp_dir(tag);
        fs::create_dir_all(&d).unwrap();
        let s = Store::open(&d.join("protocol.carnet"), false).unwrap();
        (d, s)
    }

    fn reopen(s: Store) -> Store {
        let path = s.path().to_path_buf();
        drop(s);
        Store::open(&path, false).unwrap()
    }

    #[test]
    fn a_new_carnet_is_one_file_with_every_map() {
        let (d, s) = temp_store("seed");
        assert_eq!(s.maps().len(), DEFAULT_MAPS.len());
        assert_eq!(s.maps()[0].id, TEAM_MAP);
        assert!(s.map("summit").is_some(), "Summit (juin 2026) fait partie des maps");
        assert_eq!(s.maps().iter().filter(|m| !m.is_team()).count(), 13);
        assert!(s.note(TEAM_MAP, "bienvenue").is_some_and(|n| n.doc.fm.pin));
        let files: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(files, vec!["protocol.carnet"], "un seul fichier, rien d'autre");
        // En lecture seule, rien n'est créé.
        assert!(Store::open(&d.join("absent.carnet"), true).is_err());
        assert!(!d.join("absent.carnet").exists());
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_english_carnet_greets_and_templates_in_english() {
        let d = temp_dir("english");
        fs::create_dir_all(&d).unwrap();
        let mut s = Store::open_in(&d.join("protocol.carnet"), false, None, Lang::En).unwrap();
        let welcome = s.note(TEAM_MAP, "welcome").expect("la note d'accueil anglaise");
        assert_eq!((welcome.title.as_str(), welcome.doc.fm.tags.as_slice()), ("Welcome to protocol", ["help".to_string()].as_slice()));
        assert!(welcome.doc.body.contains("## Getting started") && s.note(TEAM_MAP, "bienvenue").is_none());
        assert_eq!(s.map(TEAM_MAP).unwrap().title, "General", "l'id reste « general »");
        assert_eq!(s.map("ascent").unwrap().title, "Ascent");
        // Les notes suivent la langue du carnet (son réglage), pas celle de sa création.
        s.set_setting(i18n::SETTING, "en").unwrap();
        let n = s.create_note("ascent", NoteInput { kind: Some("retake".into()), ..Default::default() }).unwrap();
        assert_eq!(n.title, "New note");
        assert!(n.doc.body.starts_with("## Conditions\n- Players alive:"), "{}", n.doc.body);
        assert_eq!(s.import_note("ascent", "## Section", None).unwrap().title, "Imported note");
        s.delete_map("bind").unwrap();
        assert!(s.trash().iter().any(|t| t.title == "overview of Bind"));
        s.set_setting(i18n::SETTING, "fr").unwrap();
        let n = s.create_note("ascent", NoteInput { kind: Some("retake".into()), ..Default::default() }).unwrap();
        assert_eq!(n.title, "Nouvelle note");
        assert!(n.doc.body.contains("- Joueurs en vie :"), "{}", n.doc.body);
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn everything_survives_a_restart() {
        let (d, mut s) = temp_store("restart");
        let n = s.create_note("ascent", NoteInput { title: Some("Retake B".into()), kind: Some("retake".into()), ..Default::default() }).unwrap();
        s.save_note("ascent", "retake-b", NoteInput { body: Some("fumée heaven\n".into()), ..Default::default() }, Some(&n.version)).unwrap();
        let img = s.save_asset(b"\x89PNG-1", "png").unwrap();
        s.save_map("bind", None, Some(Some(true)), Some("## Défauts\n"), None).unwrap();
        s.set_setting(SETTING_THEME, "dark").unwrap();
        let key = s.trash_note(TEAM_MAP, "bienvenue").unwrap();
        let version = s.note("ascent", "retake-b").unwrap().version.clone();
        let mut s = reopen(s);
        let n = s.note("ascent", "retake-b").unwrap();
        assert_eq!((n.doc.body.as_str(), n.version.as_str()), ("fumée heaven\n", version.as_str()), "même texte, même version");
        assert_eq!(s.asset(&format!("assets/{img}")), Some((b"\x89PNG-1".to_vec(), "image/png")));
        assert_eq!(s.map("bind").unwrap().pool, Some(true));
        assert_eq!(s.setting(SETTING_THEME), Some("dark"));
        assert_eq!(s.trash().len(), 1);
        assert_eq!(s.restore(&key).unwrap(), (TEAM_MAP.to_string(), Some("bienvenue".to_string())));
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn compaction_keeps_everything_and_shrinks_the_file() {
        let (d, mut s) = temp_store("compact");
        s.create_note("ascent", NoteInput { title: Some("Exec A".into()), ..Default::default() }).unwrap();
        let png = vec![7u8; 300_000];
        let img = s.save_asset(&png, "png").unwrap();
        // Des centaines d'enregistrements successifs : c'est l'enregistrement automatique.
        for i in 0..400 {
            let body = format!("version {i}\n{}", "texte ".repeat(400));
            s.save_note("ascent", "exec-a", NoteInput { body: Some(body), ..Default::default() }, None).unwrap();
        }
        let before = fs::metadata(s.path()).unwrap().len();
        // Une écriture pendant le compactage est recopiée à la suite.
        if s.compaction.is_none() {
            s.start_compaction();
        }
        let late = s.save_asset(b"\x89PNG-tard", "png").unwrap();
        s.save_note("ascent", "exec-a", NoteInput { body: Some("finale\n".into()), ..Default::default() }, None).unwrap();
        s.compact_now().unwrap();
        let after = fs::metadata(s.path()).unwrap().len();
        assert!(after < before / 4, "{before} → {after}");
        assert_eq!(s.note("ascent", "exec-a").unwrap().doc.body, "finale\n");
        assert_eq!(s.asset(&format!("assets/{img}")).unwrap().0, png);
        assert_eq!(s.asset(&format!("assets/{late}")).unwrap().0, b"\x89PNG-tard");
        let mut s = reopen(s);
        assert_eq!(s.note("ascent", "exec-a").unwrap().doc.body, "finale\n");
        assert_eq!(s.asset(&format!("assets/{late}")).unwrap().0, b"\x89PNG-tard");
        assert_eq!(s.asset(&format!("assets/{img}")).unwrap().0, png);
        let leftovers: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, vec!["protocol.carnet"]);
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_second_writer_is_refused_a_reader_follows() {
        let (d, mut s) = temp_store("two");
        let path = s.path().to_path_buf();
        assert_eq!(Store::open(&path, false).err().map(|e| e.kind()), Some(io::ErrorKind::ResourceBusy));
        let mut reader = Store::open(&path, true).unwrap();
        s.create_note("bind", NoteInput { title: Some("Setup B".into()), ..Default::default() }).unwrap();
        reader.refresh(true).unwrap();
        assert!(reader.note("bind", "setup-b").is_some(), "le lecteur voit la note créée ailleurs");
        assert!(matches!(reader.create_note("bind", NoteInput::default()), Err(StoreError::ReadOnly)));
        drop((s, reader));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_old_notes_folder_is_imported_untouched() {
        let d = temp_dir("legacy");
        let old = d.join("notes");
        for dir in ["general", "ascent", "_brouillons", "assets", ".trash/ascent"] {
            fs::create_dir_all(old.join(dir)).unwrap();
        }
        fs::write(old.join("general/_map.md"), "---\ntitle: Général\norder: 0\n---\n").unwrap();
        fs::write(old.join("ascent/_map.md"), "---\ntitle: Ascent\npool: true\n---\n## Défauts\n").unwrap();
        fs::write(old.join("ascent/retake-b.md"), "---\ntitle: Retake B\ntags: [b]\naliases: [rb]\n---\n![plan](plan.png) ![x](../assets/1.png)\n")
            .unwrap();
        fs::write(old.join("ascent/vieux.md"), b"---\ntitle: D\xe9faut\n---\nfum\xe9e\n").unwrap();
        fs::write(old.join("ascent/plan.png"), b"\x89PNG-plan").unwrap();
        fs::write(old.join("assets/1.png"), b"\x89PNG-1").unwrap();
        fs::write(old.join(".trash/ascent/exec.20261012T101010Z.md"), "---\ntitle: Exec\n---\n").unwrap();
        fs::write(old.join("_brouillons/x.md"), "ignoré : nom de dossier refusé").unwrap();
        let listing = |p: &Path| {
            let mut v = Vec::new();
            let mut stack = vec![p.to_path_buf()];
            while let Some(dir) = stack.pop() {
                for e in fs::read_dir(dir).unwrap().flatten() {
                    if e.path().is_dir() {
                        stack.push(e.path());
                    } else {
                        v.push((e.path(), fs::read(e.path()).unwrap()));
                    }
                }
            }
            v.sort();
            v
        };
        let before = listing(&old);
        let path = d.join("protocol.carnet");
        let mut s = Store::open_or_import(&path, false, Some(&old)).unwrap();
        assert_eq!(listing(&old), before, "l'ancien dossier n'est pas modifié");
        assert_eq!(s.map("ascent").unwrap().pool, Some(true));
        assert_eq!(s.map("ascent").unwrap().doc.body, "## Défauts\n");
        let n = s.note("ascent", "retake-b").unwrap();
        assert!(n.text.contains("aliases: [rb]"), "en-tête inconnu conservé");
        assert_eq!(s.note("ascent", "vieux").unwrap().title, "Défaut", "un fichier en Windows-1252 est relu");
        assert_eq!(s.asset("assets/1.png").unwrap().0, b"\x89PNG-1");
        assert_eq!(s.asset("ascent/plan.png").unwrap().0, b"\x89PNG-plan");
        assert_eq!(s.trash().len(), 1);
        // Les maps que l'ancienne version ne connaissait pas sont ajoutées, pas les autres.
        assert!(s.map("summit").is_some());
        assert!(s.map("bind").is_none(), "une map absente de l'ancien dossier (retirée) ne revient pas");
        assert!(s.notice().is_some_and(|m| m.contains("2 notes")), "{:?}", s.notice());
        s.clear_notice().unwrap();
        // Rouvert : l'import n'est pas refait, Summit n'est pas proposée deux fois.
        s.delete_map("summit").unwrap();
        let s = reopen(s);
        assert!(s.map("summit").is_none() && s.notice().is_none());
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn imports_keep_their_header_and_never_overwrite() {
        let (d, mut s) = temp_store("import");
        let a = s
            .import_note("ascent", "---\ntitle: Retake B\ntype: Retakes\nside: défense\naliases: [rb]\n---\n## Calls\n- stack B\n", None)
            .unwrap();
        assert_eq!((a.id.as_str(), a.title.as_str(), a.kind(), a.side()), ("retake-b", "Retake B", "retake", "def"));
        assert!(a.text.contains("aliases: [rb]") && a.text.contains("## Calls"), "{}", a.text);
        // Sans en-tête : le premier `# Titre` devient le titre et quitte le corps.
        let b = s.import_note("ascent", "\r\n# Retake B\r\n\r\ntexte\r\n", Some("ignoré")).unwrap();
        assert_eq!((b.id.as_str(), b.title.as_str(), b.doc.body.as_str()), ("retake-b-2", "Retake B", "texte\n"));
        // Sinon le nom du fichier, sinon « Note importée ».
        assert_eq!(s.import_note("ascent", "juste du texte", Some("Exécution A")).unwrap().id, "execution-a");
        assert_eq!(s.import_note("ascent", "## Section", None).unwrap().title, "Note importée");
        assert!(matches!(s.import_note("..", "x", None), Err(StoreError::Invalid(_))));
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn each_launch_has_its_own_etag_space() {
        let (d, a) = temp_store("boot");
        let a = reopen(a);
        let boot = a.boot.clone();
        std::thread::sleep(Duration::from_millis(2));
        let b = reopen(a);
        assert_ne!(boot, b.boot);
        drop(b);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn create_save_conflict_move_trash_restore() {
        let (d, mut s) = temp_store("crud");
        let n = s
            .create_note(
                "ascent",
                NoteInput { title: Some("Retake B".into()), kind: Some("retake".into()), side: Some("défense".into()), ..Default::default() },
            )
            .unwrap();
        assert_eq!((n.id.as_str(), n.kind(), n.side()), ("retake-b", "retake", "def"));
        assert!(n.doc.body.contains("## Conditions"), "le modèle du type est appliqué");
        let twin = s.create_note("ascent", NoteInput { title: Some("Retake B".into()), ..Default::default() }).unwrap();
        assert_eq!(twin.id, "retake-b-2");
        let saved = s
            .save_note(
                "ascent",
                "retake-b",
                NoteInput { body: Some("nouveau".into()), tags: Some(vec!["#b".into(), "B".into()]), ..Default::default() },
                Some(&n.version),
            )
            .unwrap();
        assert_eq!(saved.doc.body, "nouveau\n");
        assert_eq!(saved.doc.fm.tags, vec!["b"]);
        // L'ancienne version ne peut plus écraser la nouvelle.
        let stale = s.save_note("ascent", "retake-b", NoteInput { body: Some("vieux".into()), ..Default::default() }, Some(&n.version));
        assert!(matches!(stale, Err(StoreError::Conflict(_))));
        // Un déplacement vers une map qui n'existe pas n'écrit rien.
        let size = s.size();
        assert!(
            s.save_note("ascent", "retake-b", NoteInput { map: Some("nulle-part".into()), body: Some("x".into()), ..Default::default() }, None)
                .is_err()
        );
        assert_eq!(s.size(), size);
        let moved = s.save_note("ascent", "retake-b-2", NoteInput { map: Some("bind".into()), ..Default::default() }, None).unwrap();
        assert_eq!((moved.map.as_str(), moved.id.as_str()), ("bind", "retake-b-2"));
        assert!(s.note("ascent", "retake-b-2").is_none());
        // Le Markdown brut : gardé à l'identique ; une création ne double pas un nom à la casse près.
        let raw = "---\ntitle: Brut\n---\ntel quel\n";
        assert_eq!(s.put_raw("ascent", "brut", raw, None).unwrap().text, raw);
        assert!(s.put_raw("ascent", "BRUT", raw, None).is_err());
        let t = s.trash_note("ascent", "retake-b").unwrap();
        assert!(s.note("ascent", "retake-b").is_none());
        assert_eq!(s.trash().len(), 1);
        assert_eq!(s.restore(&t).unwrap(), ("ascent".into(), Some("retake-b".into())));
        assert!(s.note("ascent", "retake-b").is_some());
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_odd_names_and_read_only() {
        let (d, mut s) = temp_store("safety");
        for bad in ["..", "../x", ".trash", "assets", "a/b", "C:", "Ascent", "nulle-part"] {
            assert!(s.create_note(bad, NoteInput::default()).is_err(), "{bad}");
        }
        assert!(s.save_note("ascent", "../../x", NoteInput::default(), None).is_err());
        assert!(s.save_note("ascent", "absente", NoteInput::default(), None).is_err());
        assert!(s.put_raw("ascent", "RETAKE~1", "x", None).is_err());
        assert!(s.asset("../Cargo.toml").is_none());
        assert!(s.asset("general/bienvenue.md").is_none());
        assert!(s.asset(".trash/x.png").is_none());
        assert!(s.save_asset(b"x", "exe").is_err());
        let name = s.save_asset(b"\x89PNG", "png").unwrap();
        assert_eq!(s.save_asset(b"\x89PNG", "png").unwrap(), name, "une image déjà rangée n'est pas recopiée");
        let path = s.path().to_path_buf();
        drop(s);
        let mut ro = Store::open(&path, true).unwrap();
        assert!(matches!(ro.create_note("ascent", NoteInput::default()), Err(StoreError::ReadOnly)));
        drop(ro);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn maps_pool_delete_and_restore() {
        let (d, mut s) = temp_store("maps");
        let m = s.create_map("District 9").unwrap();
        assert_eq!(m.id, "district-9");
        assert!(s.create_map("district 9").is_err());
        let m = s.save_map("ascent", None, Some(Some(true)), Some("## Défauts\n"), None).unwrap();
        assert_eq!((m.pool, m.doc.body.as_str(), m.title.as_str()), (Some(true), "## Défauts\n", "Ascent"));
        // Une présentation changée ailleurs : conflit, pas d'écrasement.
        assert!(matches!(s.save_map("ascent", None, None, Some("x"), Some("ancienne")), Err(StoreError::MapConflict(_))));
        s.delete_map("district-9").unwrap();
        assert!(s.map("district-9").is_none());
        let back = s.trash().into_iter().find(|t| t.id.is_none()).unwrap();
        assert_eq!(back.title, "présentation de District 9");
        assert_eq!(s.restore(&back.path).unwrap(), ("district-9".into(), None));
        assert_eq!(s.map("district-9").unwrap().title, "District 9");
        assert!(s.delete_map(TEAM_MAP).is_err(), "la map contient la note de bienvenue");
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn exports_a_zip_of_markdown_files() {
        let (d, mut s) = temp_store("zip");
        s.create_note("ascent", NoteInput { title: Some("Retake B".into()), ..Default::default() }).unwrap();
        let img = s.save_asset(b"\x89PNG-zip", "png").unwrap();
        let zip = s.export_zip("protocol").unwrap();
        let names = crate::zip::tests::names(&zip);
        assert!(names.contains(&"protocol/ascent/retake-b.md".to_string()), "{names:?}");
        assert!(names.contains(&"protocol/general/_map.md".to_string()));
        assert!(names.contains(&format!("protocol/assets/{img}")));
        drop(s);
        let _ = fs::remove_dir_all(&d);
    }
}
