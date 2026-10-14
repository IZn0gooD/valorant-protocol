//! Écrire sans garder le carnet. La fenêtre de protocol le tient tant qu'elle est
//! ouverte, et un serveur MCP tourne parfois des heures dans un assistant : chaque
//! écriture passe donc par la fenêtre si elle est ouverte (son API locale, sur
//! 127.0.0.1), sinon par le carnet ouvert le temps de l'opération, puis relâché.

use std::io;
use std::path::Path;

use crate::http;
use crate::i18n::{self, Lang};
use crate::json::{self, Json, obj};
use crate::store::{NoteInput, Store, StoreError};
use crate::tr;
use crate::util::percent_encode;

/// Une modification demandée par l'assistant.
pub(super) enum Change {
    CreateNote {
        map: String,
        input: NoteInput,
    },
    /// `backup` : la version d'avant va d'abord à la corbeille.
    SaveNote {
        map: String,
        id: String,
        input: NoteInput,
        base: Option<String>,
        backup: bool,
    },
    Trash {
        map: String,
        id: String,
    },
    Restore {
        key: String,
    },
    CreateMap {
        title: String,
    },
    /// Une présentation réécrite laisse toujours la précédente à la corbeille.
    SaveMap {
        map: String,
        title: Option<String>,
        pool: Option<bool>,
        overview: Option<String>,
        base: Option<String>,
    },
}

/// Ce qu'une modification a produit. `map`/`id` : où se trouve maintenant la note
/// (un déplacement ou une restauration peuvent changer son nom).
#[derive(Debug, Default)]
pub(super) struct Done {
    pub map: String,
    pub id: Option<String>,
    pub title: String,
    /// La clé d'une entrée de la corbeille : la note retirée, ou la version d'avant.
    pub trash: Option<String>,
    /// Par où l'écriture est passée : la fenêtre ouverte, ou le carnet lui-même.
    pub via_window: bool,
}

/// Pourquoi une écriture n'a pas eu lieu, dit pour l'assistant.
pub(super) enum Refused {
    /// La note (ou la présentation) a changé depuis sa lecture ; voici sa version actuelle.
    Conflict(String),
    /// Le carnet est tenu par un autre programme, sans fenêtre joignable.
    Busy,
    Other(String),
}

/// Un carnet neuf : créé (cartes par défaut comprises), puis relâché aussitôt.
pub(super) fn create(carnet: &Path) -> io::Result<()> {
    let mut s = Store::open(carnet, false)?;
    s.settle();
    Ok(())
}

/// Applique une modification par le chemin disponible : la fenêtre si elle est
/// ouverte sur ce carnet, sinon le carnet lui-même. `lang` : la langue des refus.
pub(super) fn apply(carnet: &Path, port: Option<u16>, change: &Change, lang: Lang) -> Result<Done, Refused> {
    if let Some(p) = crate::running_instance(carnet, port) {
        return via_window(p, change, lang);
    }
    match via_carnet(carnet, change, lang) {
        // La fenêtre s'est ouverte entre-temps : on passe par elle.
        Err(Refused::Busy) => match crate::running_instance(carnet, port) {
            Some(p) => via_window(p, change, lang),
            None => Err(Refused::Busy),
        },
        r => r,
    }
}

fn refused(e: StoreError, lang: Lang) -> Refused {
    match e {
        StoreError::Conflict(cur) => Refused::Conflict(cur.version.clone()),
        StoreError::MapConflict(cur) => Refused::Conflict(cur.version.clone()),
        other => Refused::Other(i18n::localize(lang, &other.to_string())),
    }
}

fn via_carnet(carnet: &Path, change: &Change, lang: Lang) -> Result<Done, Refused> {
    let mut s = match Store::open(carnet, false) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::ResourceBusy => return Err(Refused::Busy),
        Err(e) => {
            return Err(Refused::Other(format!("{}{}{e}", tr!(lang, "carnet inaccessible", "notebook unreachable"), super::colon(lang))));
        }
    };
    let done = on_store(&mut s, change).map_err(|e| refused(e, lang));
    // Un compactage lancé par l'écriture finit avant qu'on relâche le carnet.
    s.settle();
    done
}

fn on_store(s: &mut Store, change: &Change) -> Result<Done, StoreError> {
    let note = |n: &crate::store::Note, trash: Option<String>| Done {
        map: n.map.clone(),
        id: Some(n.id.clone()),
        title: n.title.clone(),
        trash,
        via_window: false,
    };
    match change {
        Change::CreateNote { map, input } => s.create_note(map, input.clone()).map(|n| note(&n, None)),
        Change::SaveNote { map, id, input, base, backup: true } => {
            s.save_note_backed_up(map, id, input.clone(), base.as_deref()).map(|(n, k)| note(&n, Some(k)))
        }
        Change::SaveNote { map, id, input, base, backup: false } => {
            s.save_note(map, id, input.clone(), base.as_deref()).map(|n| note(&n, None))
        }
        Change::Trash { map, id } => {
            let title = s.note(map, id).map(|n| n.title.clone()).unwrap_or_default();
            s.trash_note(map, id).map(|k| Done { map: map.clone(), id: Some(id.clone()), title, trash: Some(k), via_window: false })
        }
        Change::Restore { key } => {
            let (map, id) = s.restore(key)?;
            let title = match &id {
                Some(i) => s.note(&map, i).map(|n| n.title.clone()).unwrap_or_default(),
                None => s.map(&map).map(|m| m.title.clone()).unwrap_or_default(),
            };
            Ok(Done { map, id, title, trash: None, via_window: false })
        }
        Change::CreateMap { title } => s.create_map(title).map(|m| Done { map: m.id.clone(), title: m.title.clone(), ..Done::default() }),
        Change::SaveMap { map, title, pool, overview, base } => s
            .save_map_backed_up(map, title.as_deref(), pool.map(Some), overview.as_deref(), base.as_deref())
            .map(|(m, k)| Done { map: m.id.clone(), title: m.title.clone(), trash: k, ..Done::default() }),
    }
}

/// Les champs d'une note, tels que l'API de la fenêtre les attend.
fn note_body(input: &NoteInput, base: Option<&str>, backup: bool) -> Json {
    let mut v = obj([]);
    let mut text = |k: &str, val: &Option<String>| {
        if let Some(s) = val {
            v.push(k, s.as_str().into());
        }
    };
    text("title", &input.title);
    text("kind", &input.kind);
    text("side", &input.side);
    text("body", &input.body);
    text("map", &input.map);
    if let Some(t) = &input.tags {
        v.push("tags", t.as_slice().into());
    }
    if let Some(p) = input.pin {
        v.push("pin", p.into());
    }
    if let Some(b) = base {
        v.push("baseVersion", b.into());
    }
    if backup {
        v.push("backup", true.into());
    }
    v
}

fn via_window(port: u16, change: &Change, lang: Lang) -> Result<Done, Refused> {
    let seg = |s: &str| percent_encode(s);
    let (method, path, body) = match change {
        Change::CreateNote { map, input } => ("POST", format!("/api/notes/{}", seg(map)), Some(note_body(input, None, false))),
        Change::SaveNote { map, id, input, base, backup } => {
            ("PUT", format!("/api/notes/{}/{}", seg(map), seg(id)), Some(note_body(input, base.as_deref(), *backup)))
        }
        Change::Trash { map, id } => ("DELETE", format!("/api/notes/{}/{}", seg(map), seg(id)), None),
        Change::Restore { key } => ("POST", "/api/trash/restore".to_string(), Some(obj([("path", key.as_str().into())]))),
        Change::CreateMap { title } => ("POST", "/api/maps".to_string(), Some(obj([("title", title.as_str().into())]))),
        Change::SaveMap { map, title, pool, overview, base } => {
            let mut v = obj([("backup", true.into())]);
            if let Some(t) = title {
                v.push("title", t.as_str().into());
            }
            if let Some(p) = pool {
                v.push("pool", (*p).into());
            }
            if let Some(o) = overview {
                v.push("overview", o.as_str().into());
            }
            if let Some(b) = base {
                v.push("baseVersion", b.as_str().into());
            }
            ("PUT", format!("/api/maps/{}", seg(map)), Some(v))
        }
    };
    let sent = body.map(|b| b.to_json());
    let (status, text) = http::local_request(port, method, &path, sent.as_deref()).map_err(|e| {
        Refused::Other(format!(
            "{} ({e}){}{}",
            tr!(lang, "la fenêtre de protocol ne répond pas", "the protocol window is not responding"),
            super::colon(lang),
            tr!(lang, "réessayez", "try again")
        ))
    })?;
    let v = json::parse(&text).unwrap_or(Json::Null);
    match status {
        200 | 201 => {}
        409 => return Err(Refused::Conflict(v.get("current").and_then(|c| c.str_field("version")).unwrap_or("").to_string())),
        _ => {
            let why = v
                .str_field("error")
                .map(|e| i18n::localize(lang, e))
                .unwrap_or_else(|| format!("{} ({status})", tr!(lang, "refus de la fenêtre", "refused by the window")));
            return Err(Refused::Other(why));
        }
    }
    let s = |k: &str| v.str_field(k).map(str::to_string);
    Ok(match change {
        Change::Trash { map, id } => {
            Done { map: map.clone(), id: Some(id.clone()), title: String::new(), trash: s("trash"), via_window: true }
        }
        Change::Restore { .. } => Done { map: s("map").unwrap_or_default(), id: s("id"), via_window: true, ..Done::default() },
        Change::CreateMap { .. } | Change::SaveMap { .. } => Done {
            map: s("id").unwrap_or_default(),
            title: s("title").unwrap_or_default(),
            trash: s("backup"),
            via_window: true,
            ..Done::default()
        },
        Change::CreateNote { .. } | Change::SaveNote { .. } => Done {
            map: s("map").unwrap_or_default(),
            id: s("id"),
            title: s("title").unwrap_or_default(),
            trash: s("backup"),
            via_window: true,
        },
    })
}
