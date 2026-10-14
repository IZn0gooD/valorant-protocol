//! Les outils : lire le carnet, et l'écrire avec `--write`. Chaque outil décrit ses
//! entrées et sa sortie (schémas JSON) et rend un texte pour l'assistant, doublé des
//! mêmes données structurées. Un échec (note introuvable, version périmée) revient dans
//! le résultat, pour que l'assistant le voie et se corrige. Tout ce que lit l'assistant
//! (descriptions, textes, erreurs) suit la langue du carnet ; les noms restent fixes.

use std::collections::HashMap;
use std::sync::Arc;

use super::link::{self, Change, Done, Refused};
use super::prompts::find_note;
use super::resources::{describe, map_uri, note_uri, plain, sheet_markdown, sheet_uri};
use super::{INFO, Session, WRITES_PER_MINUTE, colon};
use crate::api;
use crate::i18n::Lang;
use crate::json::{Json, obj};
use crate::note;
use crate::search;
use crate::store::{MapInfo, Note, NoteInput, Store, TEAM_MAP};
use crate::tr;
use crate::util::{fold, iso_utc};

/// Bornes des entrées : un protocole, pas un livre.
const MAX_TITLE: usize = 200;
const MAX_MAP_TITLE: usize = 60;
const MAX_BODY: usize = 1 << 20;
const MAX_APPEND: usize = 256 << 10;
const MAX_QUERY: usize = 500;
const MAX_TAGS: usize = 30;

// ── Schémas ───────────────────────────────────────────────────────────────────

fn string(description: &str) -> Json {
    obj([("type", "string".into()), ("description", description.into())])
}

fn bounded(description: &str, max: usize) -> Json {
    let mut s = string(description);
    s.push("maxLength", max.into());
    s
}

fn integer(description: &str, min: i64, max: i64) -> Json {
    obj([("type", "integer".into()), ("description", description.into()), ("minimum", min.into()), ("maximum", max.into())])
}

fn boolean(description: &str) -> Json {
    obj([("type", "boolean".into()), ("description", description.into())])
}

fn choice(description: &str, values: &[&str]) -> Json {
    let mut s = string(description);
    s.push("enum", values.iter().map(|v| Json::from(*v)).collect::<Vec<_>>().into());
    s
}

fn array(items: Json, description: &str) -> Json {
    obj([("type", "array".into()), ("description", description.into()), ("items", items)])
}

fn nullable(ty: &str, description: &str) -> Json {
    obj([("type", vec![Json::from(ty), Json::from("null")].into()), ("description", description.into())])
}

/// Un objet ; `closed` (les entrées) : aucune propriété en plus de celles décrites.
fn object(props: Vec<(&str, Json)>, required: &[&str], closed: bool) -> Json {
    let properties = Json::Obj(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let mut o = obj([("type", "object".into()), ("properties", properties)]);
    o.push("required", required.iter().map(|r| Json::from(*r)).collect::<Vec<_>>().into());
    if closed {
        o.push("additionalProperties", false.into());
    }
    o
}

fn input(props: Vec<(&str, Json)>, required: &[&str]) -> Json {
    object(props, required, true)
}

fn output(props: Vec<(&str, Json)>, required: &[&str]) -> Json {
    object(props, required, false)
}

fn kinds() -> Vec<&'static str> {
    note::KINDS.iter().map(|k| k.id).collect()
}

fn kind_input(l: Lang) -> Json {
    choice(tr!(l, "type de note", "note type"), &kinds())
}

fn side_input(l: Lang) -> Json {
    choice(
        tr!(l, "côté : atk (attaque), def (défense) ou both (les deux)", "side: atk (attack), def (defense) or both"),
        &["atk", "def", "both"],
    )
}

fn note_props(l: Lang) -> Vec<(&'static str, Json)> {
    vec![
        ("map", string(tr!(l, "id de la map", "map id"))),
        ("id", string(tr!(l, "id de la note", "note id"))),
        ("title", string(tr!(l, "titre", "title"))),
        ("type", string(tr!(l, "type de note", "note type"))),
        ("side", string(tr!(l, "atk (attaque), def (défense), ou vide (les deux)", "atk (attack), def (defense), or empty (both)"))),
        ("tags", array(string(tr!(l, "étiquette", "tag")), tr!(l, "étiquettes", "tags"))),
        ("pinned", boolean(tr!(l, "épinglée : dans la fiche match", "pinned: part of the match sheet"))),
        ("updated", string(tr!(l, "dernière modification, ISO 8601 UTC", "last change, ISO 8601 UTC"))),
        ("uri", string(tr!(l, "la ressource MCP de la note", "the note's MCP resource"))),
    ]
}

const NOTE_REQUIRED: &[&str] = &["map", "id", "title", "type", "side", "tags", "pinned", "updated", "uri"];

fn note_schema(l: Lang) -> Json {
    output(note_props(l), NOTE_REQUIRED)
}

fn note_detail_schema(l: Lang) -> Json {
    let mut props = note_props(l);
    props.push(("version", string(tr!(l, "version de la note (à redonner à update_note)", "note version (pass it back to update_note)"))));
    props.push(("body", string(tr!(l, "le corps de la note, en Markdown", "the note body, in Markdown"))));
    let mut required = NOTE_REQUIRED.to_vec();
    required.extend(["version", "body"]);
    output(props, &required)
}

fn notes_schema(l: Lang) -> Json {
    array(note_schema(l), "notes")
}

fn map_props(l: Lang) -> Vec<(&'static str, Json)> {
    vec![
        ("id", string(tr!(l, "id de la map", "map id"))),
        ("title", string(tr!(l, "nom affiché", "display name"))),
        ("notes", integer(tr!(l, "nombre de notes", "number of notes"), 0, i64::MAX)),
        ("pinned", integer(tr!(l, "notes épinglées", "pinned notes"), 0, i64::MAX)),
        ("pool", nullable("boolean", tr!(l, "dans le pool de maps (null : non précisé)", "in the map pool (null: not specified)"))),
        ("team", boolean(tr!(l, "la map de l'équipe (general), pas une map de jeu", "the team map (general), not an in-game map"))),
        (
            "updated",
            nullable(
                "string",
                tr!(l, "dernière modification d'une de ses notes, ISO 8601 UTC", "last change to one of its notes, ISO 8601 UTC"),
            ),
        ),
        ("uri", string(tr!(l, "la ressource MCP de la map", "the map's MCP resource"))),
    ]
}

const MAP_REQUIRED: &[&str] = &["id", "title", "notes", "pinned", "pool", "team", "updated", "uri"];

fn map_schema(l: Lang) -> Json {
    output(map_props(l), MAP_REQUIRED)
}

fn map_detail_schema(l: Lang) -> Json {
    let mut props = map_props(l);
    props.push(("overview", string(tr!(l, "présentation de la map, en Markdown", "map overview, in Markdown"))));
    props.push((
        "version",
        string(tr!(l, "version de la présentation (à redonner à update_map)", "overview version (pass it back to update_map)")),
    ));
    let mut required = MAP_REQUIRED.to_vec();
    required.extend(["overview", "version"]);
    output(props, &required)
}

fn written_schema(l: Lang) -> Json {
    output(
        vec![
            ("note", note_schema(l)),
            ("version", string(tr!(l, "nouvelle version de la note", "new note version"))),
            (
                "backup",
                nullable(
                    "string",
                    tr!(
                        l,
                        "clé, dans la corbeille, de la version d'avant (null : rien de réécrit)",
                        "trash key of the previous version (null: nothing was rewritten)"
                    ),
                ),
            ),
        ],
        &["note", "version", "backup"],
    )
}

fn map_written_schema(l: Lang) -> Json {
    output(
        vec![
            ("map", map_detail_schema(l)),
            (
                "backup",
                nullable("string", tr!(l, "clé, dans la corbeille, de la présentation d'avant", "trash key of the previous overview")),
            ),
        ],
        &["map", "backup"],
    )
}

// ── Données ───────────────────────────────────────────────────────────────────

fn note_data(n: &Note) -> Json {
    obj([
        ("map", n.map.as_str().into()),
        ("id", n.id.as_str().into()),
        ("title", n.title.as_str().into()),
        ("type", n.kind().into()),
        ("side", n.side().into()),
        ("tags", n.doc.fm.tags.as_slice().into()),
        ("pinned", n.doc.fm.pin.into()),
        ("updated", iso_utc(n.updated_ts).into()),
        ("uri", note_uri(&n.map, &n.id).into()),
    ])
}

fn notes_data<'a>(notes: impl IntoIterator<Item = &'a Arc<Note>>) -> Json {
    notes.into_iter().map(|n| note_data(n)).collect::<Vec<_>>().into()
}

fn map_data(m: &MapInfo) -> Json {
    obj([
        ("id", m.id.as_str().into()),
        ("title", m.title.as_str().into()),
        ("notes", m.count.into()),
        ("pinned", m.pinned.into()),
        ("pool", m.pool.into()),
        ("team", m.is_team().into()),
        ("updated", (m.updated_ts > 0).then(|| iso_utc(m.updated_ts)).into()),
        ("uri", map_uri(&m.id).into()),
    ])
}

fn map_detail(m: &MapInfo) -> Json {
    let mut j = map_data(m);
    j.push("overview", m.doc.body.as_str().into());
    j.push("version", m.version.as_str().into());
    j
}

fn line(n: &Note, l: Lang) -> String {
    format!("- {}/{} — {} ({})", n.map, n.id, n.title, describe(n, l))
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

/// « épinglée(s) » / « pinned », accordé au nombre.
fn pinned_count(n: usize, l: Lang) -> String {
    count(n, tr!(l, "épinglée", "pinned"), tr!(l, "épinglées", "pinned"))
}

/// La fiche match d'une map, nommée dans la langue voulue.
fn sheet_title(map_title: &str, l: Lang) -> String {
    format!("{map_title} — {}", tr!(l, "fiche match", "match sheet"))
}

// ── Définitions ───────────────────────────────────────────────────────────────

struct Def {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input: Json,
    output: Json,
    write: bool,
    destructive: bool,
    idempotent: bool,
}

fn read(name: &'static str, title: &'static str, description: &'static str, input: Json, output: Json) -> Def {
    Def { name, title, description, input, output, write: false, destructive: false, idempotent: true }
}

fn write(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input: Json,
    output: Json,
    destructive: bool,
    idempotent: bool,
) -> Def {
    Def { name, title, description, input, output, write: true, destructive, idempotent }
}

fn map_arg(l: Lang) -> Json {
    string(tr!(l, "id de la map (ex. ascent, bind, general)", "map id (e.g. ascent, bind, general)"))
}

fn note_arg(l: Lang) -> Json {
    string(tr!(l, "id de la note (voir list_notes ou search_notes)", "note id (see list_notes or search_notes)"))
}

fn tag_list(l: Lang, description: &str) -> Json {
    array(string(tr!(l, "étiquette, sans le #", "tag, without the #")), description)
}

fn defs(l: Lang) -> Vec<Def> {
    vec![
        read(
            "list_maps",
            tr!(l, "Lister les maps", "List maps"),
            tr!(
                l,
                "Liste les maps du carnet : nombre de notes, notes épinglées, pool. La map « general » regroupe les règles de toute l'équipe.",
                "Lists the notebook's maps: number of notes, pinned notes, map pool. The \"general\" map holds team-wide rules."
            ),
            input(vec![], &[]),
            output(vec![("maps", array(map_schema(l), "maps"))], &["maps"]),
        ),
        read(
            "get_map",
            tr!(l, "Lire une map", "Read a map"),
            tr!(
                l,
                "Une map : sa présentation (défauts, priorités) et la liste de ses notes.",
                "A map: its overview (defaults, priorities) and the list of its notes."
            ),
            input(vec![("map", map_arg(l))], &["map"]),
            output(vec![("map", map_detail_schema(l)), ("notes", notes_schema(l))], &["map", "notes"]),
        ),
        read(
            "list_notes",
            tr!(l, "Lister les notes", "List notes"),
            tr!(
                l,
                "Liste les notes, filtrables par map, type, côté, étiquette ou épinglage. Paginé : redonnez nextCursor pour la suite.",
                "Lists notes, filterable by map, type, side, tag or pinned state. Paginated: pass nextCursor back for the next page."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("type", kind_input(l)),
                    (
                        "side",
                        choice(
                            tr!(
                                l,
                                "côté : atk ou def (les notes valables des deux côtés sont incluses)",
                                "side: atk or def (notes valid for both sides are included)"
                            ),
                            &["atk", "def"],
                        ),
                    ),
                    ("tag", string(tr!(l, "une étiquette, sans le #", "a tag, without the #"))),
                    (
                        "pinned",
                        boolean(tr!(
                            l,
                            "seulement les notes épinglées (true) ou non épinglées (false)",
                            "only pinned (true) or unpinned (false) notes"
                        )),
                    ),
                    ("limit", integer(tr!(l, "notes par page (50 par défaut)", "notes per page (default 50)"), 1, 200)),
                    ("cursor", string(tr!(l, "suite d'une page précédente (nextCursor)", "continuation of a previous page (nextCursor)"))),
                ],
                &[],
            ),
            output(
                vec![
                    ("notes", notes_schema(l)),
                    ("total", integer(tr!(l, "notes qui répondent aux filtres", "notes matching the filters"), 0, i64::MAX)),
                    (
                        "nextCursor",
                        string(tr!(
                            l,
                            "à redonner pour la page suivante (absent : dernière page)",
                            "pass back for the next page (absent: last page)"
                        )),
                    ),
                ],
                &["notes", "total"],
            ),
        ),
        read(
            "read_note",
            tr!(l, "Lire une note", "Read a note"),
            tr!(
                l,
                "Lit une note complète, en Markdown, avec sa version : à redonner à update_note pour la modifier sans écraser un autre changement.",
                "Reads a full note, in Markdown, with its version: pass it to update_note to edit the note without overwriting another change."
            ),
            input(vec![("map", map_arg(l)), ("id", note_arg(l))], &["map", "id"]),
            note_detail_schema(l),
        ),
        read(
            "search_notes",
            tr!(l, "Chercher dans les notes", "Search notes"),
            tr!(
                l,
                "Recherche plein texte, sans tenir compte des accents ni de la casse. Filtres dans la requête : map:ascent type:retake \
                 side:def #tag \"expression exacte\".",
                "Full-text search, ignoring accents and case. Filters in the query: map:ascent type:retake side:def #tag \"exact phrase\"."
            ),
            input(
                vec![
                    (
                        "query",
                        bounded(
                            tr!(l, "les mots à chercher, et des filtres éventuels", "the words to search for, plus optional filters"),
                            MAX_QUERY,
                        ),
                    ),
                    ("limit", integer(tr!(l, "résultats (20 par défaut)", "results (default 20)"), 1, 50)),
                ],
                &["query"],
            ),
            output(
                vec![
                    ("total", integer(tr!(l, "résultats en tout", "total results"), 0, i64::MAX)),
                    (
                        "hits",
                        array(
                            output(
                                vec![
                                    ("note", note_schema(l)),
                                    ("snippet", string(tr!(l, "extrait, termes en **gras**", "excerpt, terms in **bold**"))),
                                ],
                                &["note", "snippet"],
                            ),
                            tr!(l, "résultats", "results"),
                        ),
                    ),
                ],
                &["total", "hits"],
            ),
        ),
        read(
            "match_sheet",
            tr!(l, "Fiche match", "Match sheet"),
            tr!(
                l,
                "La fiche match d'une map : ses notes épinglées, et les règles épinglées de l'équipe — ce qu'on relit avant de jouer.",
                "A map's match sheet: its pinned notes, plus the team's pinned rules — what you review before playing."
            ),
            input(vec![("map", map_arg(l))], &["map"]),
            output(
                vec![
                    ("map", string(tr!(l, "id de la map", "map id"))),
                    ("title", string(tr!(l, "nom de la map", "map name"))),
                    ("notes", notes_schema(l)),
                    ("team", notes_schema(l)),
                    ("markdown", string(tr!(l, "la fiche entière, en Markdown", "the whole sheet, in Markdown"))),
                ],
                &["map", "title", "notes", "team", "markdown"],
            ),
        ),
        read(
            "list_tags",
            tr!(l, "Lister les étiquettes", "List tags"),
            tr!(
                l,
                "Les étiquettes utilisées, avec leur nombre de notes (pour une map, ou tout le carnet).",
                "The tags in use, with their number of notes (for one map, or the whole notebook)."
            ),
            input(vec![("map", map_arg(l))], &[]),
            output(
                vec![(
                    "tags",
                    array(
                        output(
                            vec![
                                ("tag", string(tr!(l, "étiquette", "tag"))),
                                ("count", integer(tr!(l, "notes qui la portent", "notes carrying it"), 1, i64::MAX)),
                            ],
                            &["tag", "count"],
                        ),
                        tr!(l, "étiquettes, de la plus utilisée à la moins utilisée", "tags, from most to least used"),
                    ),
                )],
                &["tags"],
            ),
        ),
        read(
            "recent_notes",
            tr!(l, "Notes récentes", "Recent notes"),
            tr!(
                l,
                "Les dernières notes modifiées, de la plus récente à la plus ancienne.",
                "The most recently changed notes, newest first."
            ),
            input(vec![("limit", integer(tr!(l, "nombre de notes (10 par défaut)", "number of notes (default 10)"), 1, 50))], &[]),
            output(vec![("notes", notes_schema(l))], &["notes"]),
        ),
        read(
            "format_note",
            tr!(l, "Mettre en forme pour partager", "Format for sharing"),
            tr!(
                l,
                "Une note, une map entière ou une fiche match mise en forme pour être collée : Discord (tableaux alignés, titres en gras), \
                 texte simple, ou Markdown complet (en-tête compris).",
                "A note, a whole map or a match sheet formatted for pasting: Discord (aligned tables, bold headings), plain text, \
                 or full Markdown (front matter included)."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("id", string(tr!(l, "id de la note ; absent : toute la map", "note id; absent: the whole map"))),
                    (
                        "pinned_only",
                        boolean(tr!(
                            l,
                            "sans id : seulement les notes épinglées (la fiche match)",
                            "without id: only the pinned notes (the match sheet)"
                        )),
                    ),
                    (
                        "format",
                        choice(
                            tr!(l, "discord (par défaut), text ou markdown", "discord (default), text or markdown"),
                            &["discord", "text", "markdown"],
                        ),
                    ),
                ],
                &["map"],
            ),
            output(
                vec![
                    ("format", string(tr!(l, "le format rendu", "the format returned"))),
                    ("title", string(tr!(l, "ce qui est mis en forme", "what was formatted"))),
                    ("text", string(tr!(l, "le texte, prêt à coller", "the text, ready to paste"))),
                ],
                &["format", "title", "text"],
            ),
        ),
        read(
            "list_trash",
            tr!(l, "Voir la corbeille", "View the trash"),
            tr!(
                l,
                "Le contenu de la corbeille : notes retirées et versions d'avant, avec la clé qui permet de les restaurer.",
                "What is in the trash: removed notes and previous versions, with the key to restore them."
            ),
            input(vec![], &[]),
            output(
                vec![(
                    "items",
                    array(
                        output(
                            vec![
                                ("key", string(tr!(l, "clé à donner à restore_from_trash", "key to pass to restore_from_trash"))),
                                ("map", string(tr!(l, "map d'origine", "original map"))),
                                (
                                    "id",
                                    nullable(
                                        "string",
                                        tr!(l, "id de la note (null : la présentation d'une map)", "note id (null: a map overview)"),
                                    ),
                                ),
                                ("title", string(tr!(l, "titre", "title"))),
                                ("deleted", string(tr!(l, "mis à la corbeille le, ISO 8601 UTC", "moved to the trash on, ISO 8601 UTC"))),
                            ],
                            &["key", "map", "id", "title", "deleted"],
                        ),
                        tr!(l, "éléments, du plus récent au plus ancien", "items, newest first"),
                    ),
                )],
                &["items"],
            ),
        ),
        write(
            "create_note",
            tr!(l, "Créer une note", "Create a note"),
            tr!(
                l,
                "Crée une note dans une map. Sans corps, elle reçoit le modèle de son type. Rend son id et sa version.",
                "Creates a note in a map. Without a body, it gets its type's template. Returns its id and version."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("title", bounded(tr!(l, "titre", "title"), MAX_TITLE)),
                    ("type", kind_input(l)),
                    ("side", side_input(l)),
                    ("tags", tag_list(l, tr!(l, "étiquettes", "tags"))),
                    ("pinned", boolean(tr!(l, "épinglée : dans la fiche match", "pinned: part of the match sheet"))),
                    (
                        "body",
                        string(tr!(
                            l,
                            "le corps, en Markdown (absent : le modèle du type)",
                            "the body, in Markdown (absent: the type's template)"
                        )),
                    ),
                ],
                &["map", "title"],
            ),
            written_schema(l),
            false,
            false,
        ),
        write(
            "update_note",
            tr!(l, "Modifier une note", "Edit a note"),
            tr!(
                l,
                "Modifie une note : titre, corps, type, côté, étiquettes (un champ absent reste tel quel). Exige la version lue par \
                 read_note : si la note a changé depuis, rien n'est écrasé. La version d'avant va à la corbeille.",
                "Edits a note: title, body, type, side, tags (an absent field stays as is). Requires the version read by read_note: \
                 if the note changed since, nothing is overwritten. The previous version goes to the trash."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("id", note_arg(l)),
                    ("base_version", string(tr!(l, "la version rendue par read_note", "the version returned by read_note"))),
                    ("title", bounded(tr!(l, "nouveau titre", "new title"), MAX_TITLE)),
                    (
                        "body",
                        string(tr!(l, "nouveau corps, en Markdown (remplace l'ancien)", "new body, in Markdown (replaces the old one)")),
                    ),
                    ("type", kind_input(l)),
                    ("side", side_input(l)),
                    ("tags", tag_list(l, tr!(l, "nouvelles étiquettes (remplacent les anciennes)", "new tags (replace the old ones)"))),
                ],
                &["map", "id", "base_version"],
            ),
            written_schema(l),
            true,
            false,
        ),
        write(
            "append_to_note",
            tr!(l, "Compléter une note", "Append to a note"),
            tr!(
                l,
                "Ajoute un passage à la fin d'une note, sans toucher au reste.",
                "Adds a passage at the end of a note, leaving the rest untouched."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("id", note_arg(l)),
                    ("text", bounded(tr!(l, "le passage à ajouter, en Markdown", "the passage to add, in Markdown"), MAX_APPEND)),
                ],
                &["map", "id", "text"],
            ),
            written_schema(l),
            false,
            false,
        ),
        write(
            "set_pinned",
            tr!(l, "Épingler une note", "Pin a note"),
            tr!(
                l,
                "Épingle ou désépingle une note : les notes épinglées forment la fiche match de leur map.",
                "Pins or unpins a note: pinned notes make up their map's match sheet."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("id", note_arg(l)),
                    ("pinned", boolean(tr!(l, "true : épingler ; false : désépingler", "true: pin; false: unpin"))),
                ],
                &["map", "id", "pinned"],
            ),
            written_schema(l),
            false,
            true,
        ),
        write(
            "move_note",
            tr!(l, "Déplacer une note", "Move a note"),
            tr!(
                l,
                "Déplace une note vers une autre map (son id peut changer si la place est prise).",
                "Moves a note to another map (its id may change if it is already taken)."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("id", note_arg(l)),
                    ("to_map", string(tr!(l, "id de la map d'arrivée", "id of the destination map"))),
                ],
                &["map", "id", "to_map"],
            ),
            written_schema(l),
            false,
            false,
        ),
        write(
            "trash_note",
            tr!(l, "Mettre une note à la corbeille", "Move a note to the trash"),
            tr!(
                l,
                "Met une note à la corbeille ; restore_from_trash (ou « Restaurer » dans protocol) la remet. Aucune suppression définitive.",
                "Moves a note to the trash; restore_from_trash (or \"Restore\" in protocol) brings it back. Nothing is ever permanently deleted."
            ),
            input(vec![("map", map_arg(l)), ("id", note_arg(l))], &["map", "id"]),
            output(
                vec![
                    ("trash", string(tr!(l, "clé de la note dans la corbeille", "the note's key in the trash"))),
                    ("map", string("map")),
                    ("id", string("id")),
                    ("title", string(tr!(l, "titre", "title"))),
                ],
                &["trash", "map", "id", "title"],
            ),
            true,
            false,
        ),
        write(
            "restore_from_trash",
            tr!(l, "Restaurer depuis la corbeille", "Restore from the trash"),
            tr!(
                l,
                "Restaure un élément de la corbeille (voir list_trash). Une version d'avant revient à côté de la note actuelle, sous son \
                 propre titre ; une note retirée retrouve sa map.",
                "Restores an item from the trash (see list_trash). A previous version comes back next to the current note, under its \
                 own title; a removed note returns to its map."
            ),
            input(
                vec![(
                    "key",
                    string(tr!(
                        l,
                        "la clé donnée par list_trash, trash_note ou update_note",
                        "the key given by list_trash, trash_note or update_note"
                    )),
                )],
                &["key"],
            ),
            output(
                vec![
                    ("map", string("map")),
                    (
                        "id",
                        nullable(
                            "string",
                            tr!(
                                l,
                                "id de la note restaurée (null : une présentation de map)",
                                "id of the restored note (null: a map overview)"
                            ),
                        ),
                    ),
                    ("title", string(tr!(l, "titre", "title"))),
                    ("uri", string(tr!(l, "la ressource MCP restaurée", "the restored MCP resource"))),
                ],
                &["map", "id", "title", "uri"],
            ),
            false,
            false,
        ),
        write(
            "create_map",
            tr!(l, "Créer une map", "Create a map"),
            tr!(l, "Crée une map (son id vient de son nom).", "Creates a map (its id is derived from its name)."),
            input(vec![("title", bounded(tr!(l, "nom de la map", "map name"), MAX_MAP_TITLE))], &["title"]),
            map_written_schema(l),
            false,
            false,
        ),
        write(
            "update_map",
            tr!(l, "Modifier une map", "Edit a map"),
            tr!(
                l,
                "Renomme une map, la met dans le pool ou l'en sort, ou réécrit sa présentation. Réécrire la présentation exige la version \
                 rendue par get_map ; la présentation d'avant va à la corbeille.",
                "Renames a map, adds it to or removes it from the map pool, or rewrites its overview. Rewriting the overview requires \
                 the version returned by get_map; the previous overview goes to the trash."
            ),
            input(
                vec![
                    ("map", map_arg(l)),
                    ("title", bounded(tr!(l, "nouveau nom", "new name"), MAX_MAP_TITLE)),
                    ("in_pool", boolean(tr!(l, "dans le pool de maps", "in the map pool"))),
                    (
                        "overview",
                        string(tr!(
                            l,
                            "nouvelle présentation, en Markdown (remplace l'ancienne)",
                            "new overview, in Markdown (replaces the old one)"
                        )),
                    ),
                    (
                        "base_version",
                        string(tr!(
                            l,
                            "la version rendue par get_map (exigée avec overview)",
                            "the version returned by get_map (required with overview)"
                        )),
                    ),
                ],
                &["map"],
            ),
            map_written_schema(l),
            true,
            false,
        ),
    ]
}

pub(super) fn list(session: &Session) -> Json {
    let lang = session.lang();
    let tools: Vec<Json> = defs(lang).iter().filter(|d| !d.write || session.write).map(|d| def_json(session, d)).collect();
    obj([("tools", tools.into())])
}

fn def_json(session: &Session, d: &Def) -> Json {
    let mut j = obj([("name", d.name.into())]);
    if session.at_least("2025-06-18") {
        j.push("title", d.title.into());
    }
    j.push("description", d.description.into());
    j.push("inputSchema", d.input.clone());
    if session.at_least("2025-06-18") {
        j.push("outputSchema", d.output.clone());
    }
    if session.at_least("2025-03-26") {
        let mut a = obj([("title", d.title.into()), ("readOnlyHint", (!d.write).into())]);
        if d.write {
            a.push("destructiveHint", d.destructive.into());
            a.push("idempotentHint", d.idempotent.into());
        }
        a.push("openWorldHint", false.into());
        j.push("annotations", a);
    }
    j
}

// ── Appel ─────────────────────────────────────────────────────────────────────

/// Ce que rend un outil : un texte pour l'assistant, les mêmes données structurées, et
/// des liens vers les ressources concernées.
struct Reply {
    text: String,
    data: Json,
    links: Vec<(String, String, String)>,
}

impl Reply {
    fn new(text: String, data: Json) -> Reply {
        Reply { text, data, links: Vec::new() }
    }

    fn link(mut self, uri: String, name: &str, title: &str) -> Reply {
        self.links.push((uri, name.to_string(), title.to_string()));
        self
    }

    fn json(self, session: &Session) -> Json {
        let mut content = vec![obj([("type", "text".into()), ("text", self.text.into())])];
        if session.at_least("2025-06-18") {
            for (uri, name, title) in self.links {
                content.push(obj([
                    ("type", "resource_link".into()),
                    ("uri", uri.into()),
                    ("name", name.into()),
                    ("title", title.into()),
                    ("mimeType", "text/markdown".into()),
                ]));
            }
        }
        let mut r = obj([("content", content.into())]);
        if session.at_least("2025-06-18") {
            r.push("structuredContent", self.data);
        }
        r.push("isError", false.into());
        r
    }
}

fn failure(message: &str) -> Json {
    obj([("content", vec![obj([("type", "text".into()), ("text", message.into())])].into()), ("isError", true.into())])
}

/// Les arguments d'un appel, lus avec indulgence (espaces retirés) mais bornés ; les
/// refus sont dits dans la langue du carnet.
struct Args<'a> {
    v: &'a Json,
    l: Lang,
}

impl Args<'_> {
    fn opt(&self, k: &str) -> Option<&str> {
        self.v.str_field(k).map(str::trim).filter(|s| !s.is_empty())
    }

    fn missing(&self, k: &str) -> String {
        format!("{}{}{k}", tr!(self.l, "argument requis", "missing required argument"), colon(self.l))
    }

    fn need(&self, k: &str) -> Result<&str, String> {
        self.opt(k).ok_or_else(|| self.missing(k))
    }

    /// Un texte long (corps de note) : gardé tel quel, sous une taille bornée.
    fn long(&self, k: &str, max: usize) -> Result<Option<String>, String> {
        match self.v.get(k) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Str(s)) if s.len() > max => Err(format!(
                "{k} {}{}{} {}",
                tr!(self.l, "trop long", "too long"),
                colon(self.l),
                max >> 10,
                tr!(self.l, "Ko au plus", "KB at most")
            )),
            Some(Json::Str(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!("{k}{}{}", colon(self.l), tr!(self.l, "un texte est attendu", "a string is expected"))),
        }
    }

    fn short(&self, k: &str, max_chars: usize) -> Result<Option<&str>, String> {
        match self.opt(k) {
            Some(s) if s.chars().count() > max_chars => Err(format!(
                "{k} {}{}{max_chars} {}",
                tr!(self.l, "trop long", "too long"),
                colon(self.l),
                tr!(self.l, "caractères au plus", "characters at most")
            )),
            other => Ok(other),
        }
    }

    fn flag(&self, k: &str) -> Result<Option<bool>, String> {
        match self.v.get(k) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("{k}{}{}", colon(self.l), tr!(self.l, "true ou false attendu", "true or false expected"))),
        }
    }

    fn count(&self, k: &str, default: usize, max: usize) -> usize {
        self.v.get(k).and_then(Json::as_f64).map_or(default, |v| v.clamp(1.0, max as f64) as usize)
    }

    fn kind(&self) -> Result<Option<String>, String> {
        let Some(raw) = self.opt("type") else { return Ok(None) };
        let k = note::normalize_kind(raw);
        if note::kind(&k).is_none() {
            return Err(format!("{}{}{raw} ({})", tr!(self.l, "type inconnu", "unknown type"), colon(self.l), kinds().join(", ")));
        }
        Ok(Some(k))
    }

    /// `atk`, `def`, ou `both` (les deux, rendu par une chaîne vide).
    fn side(&self) -> Result<Option<String>, String> {
        match self.opt("side") {
            None => Ok(None),
            Some(s) if fold(s) == "both" || fold(s) == "les deux" => Ok(Some(String::new())),
            Some(s) => match note::normalize_side(s).as_str() {
                "" => Err(format!(
                    "{}{}{s} ({})",
                    tr!(self.l, "côté inconnu", "unknown side"),
                    colon(self.l),
                    tr!(self.l, "atk, def ou both", "atk, def or both")
                )),
                side => Ok(Some(side.to_string())),
            },
        }
    }

    fn tags(&self) -> Result<Option<Vec<String>>, String> {
        let l = self.l;
        match self.v.get("tags") {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Arr(a)) if a.len() > MAX_TAGS => Err(format!("{MAX_TAGS} {}", tr!(l, "étiquettes au plus", "tags at most"))),
            Some(Json::Arr(a)) => a
                .iter()
                .map(|t| {
                    t.as_str()
                        .map(|s| s.trim().trim_start_matches('#').to_string())
                        .ok_or_else(|| tr!(l, "tags : des textes sont attendus", "tags: strings are expected").to_string())
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(tr!(l, "tags : une liste est attendue", "tags: an array is expected").into()),
        }
    }
}

pub(super) fn call(session: &mut Session, params: &Json) -> Result<Json, super::Fail> {
    let lang = session.lang();
    let name = params.str_field("name").unwrap_or("");
    let args = match params.get("arguments") {
        None | Some(Json::Null) => obj([]),
        Some(a @ Json::Obj(_)) => a.clone(),
        Some(_) => return Err(super::Fail::invalid(tr!(lang, "arguments : un objet est attendu", "arguments: an object is expected"))),
    };
    let defs = defs(lang);
    let Some(def) = defs.iter().find(|d| d.name == name) else {
        return Err(super::Fail::invalid(format!("{}{}{name}", tr!(lang, "outil inconnu", "unknown tool"), colon(lang))));
    };
    if def.write && !session.write {
        return Err(super::Fail::invalid(match lang {
            Lang::Fr => format!("l'outil {name} écrit dans le carnet : il faut lancer « protocol mcp --write »"),
            Lang::En => format!("the {name} tool writes to the notebook: start the server with \"protocol mcp --write\""),
        }));
    }
    let a = Args { v: &args, l: lang };
    let result = if !def.write {
        let s = &mut *session.live.store();
        let _ = s.refresh(false);
        read_tool(s, name, &a)
    } else if !session.spend_write() {
        Err(match lang {
            Lang::Fr => format!("trop d'écritures en une minute ({WRITES_PER_MINUTE} au plus) : attendez un peu avant de recommencer"),
            Lang::En => format!("too many writes in one minute ({WRITES_PER_MINUTE} at most): wait a moment before trying again"),
        })
    } else {
        write_tool(session, name, &a)
    };
    Ok(match result {
        Ok(reply) => reply.json(session),
        Err(message) => failure(&message),
    })
}

// ── Lecture ───────────────────────────────────────────────────────────────────

fn no_map(id: &str, l: Lang) -> String {
    format!("{}{}{id} ({})", tr!(l, "map introuvable", "map not found"), colon(l), tr!(l, "voir list_maps", "see list_maps"))
}

fn no_note(map: &str, id: &str, l: Lang) -> String {
    format!(
        "{}{}{map}/{id} ({})",
        tr!(l, "note introuvable", "note not found"),
        colon(l),
        tr!(l, "voir list_notes ou search_notes", "see list_notes or search_notes")
    )
}

fn read_tool(s: &Store, name: &str, a: &Args) -> Result<Reply, String> {
    let l = a.l;
    let notes_word = |n: usize| count(n, "note", "notes");
    match name {
        "list_maps" => {
            let lines: Vec<String> = s
                .maps()
                .iter()
                .map(|m| {
                    let pool = match m.pool {
                        Some(true) => tr!(l, " · dans le pool", " · in the pool"),
                        Some(false) => tr!(l, " · hors pool", " · out of the pool"),
                        None => "",
                    };
                    let pinned = if m.pinned > 0 { format!(", {}", pinned_count(m.pinned, l)) } else { String::new() };
                    format!("- {} — {}{}{}{pinned}{pool}", m.id, m.title, colon(l), notes_word(m.count))
                })
                .collect();
            let text = if lines.is_empty() { tr!(l, "Aucune map.", "No maps.").to_string() } else { lines.join("\n") };
            Ok(Reply::new(text, obj([("maps", s.maps().iter().map(map_data).collect::<Vec<_>>().into())])))
        }
        "get_map" => {
            let id = a.need("map")?;
            let m = s.map(id).ok_or_else(|| no_map(id, l))?;
            let notes: Vec<&Arc<Note>> = s.notes_of(id).collect();
            let overview = if m.doc.body.trim().is_empty() {
                tr!(l, "_Pas encore de présentation._", "_No overview yet._")
            } else {
                m.doc.body.trim()
            };
            let list = if notes.is_empty() {
                tr!(l, "_Aucune note._", "_No notes._").to_string()
            } else {
                notes.iter().map(|n| line(n, l)).collect::<Vec<_>>().join("\n")
            };
            let text = format!(
                "# {}\n{}, {} · version {}\n\n## {}\n\n{overview}\n\n## Notes\n\n{list}",
                m.title,
                notes_word(m.count),
                pinned_count(m.pinned, l),
                m.version,
                tr!(l, "Présentation", "Overview"),
            );
            Ok(Reply::new(text, obj([("map", map_detail(m)), ("notes", notes_data(notes))])).link(map_uri(&m.id), &m.id, &m.title).link(
                sheet_uri(&m.id),
                &format!("{}-fiche", m.id),
                &sheet_title(&m.title, l),
            ))
        }
        "list_notes" => {
            let map = a.opt("map");
            if let Some(m) = map
                && s.map(m).is_none()
            {
                return Err(no_map(m, l));
            }
            let kind = a.kind()?;
            let side = match a.opt("side") {
                None => None,
                Some(raw) => Some(note::normalize_side(raw)).filter(|x| !x.is_empty()).map(Some).ok_or(format!(
                    "{}{}{raw} ({})",
                    tr!(l, "côté inconnu", "unknown side"),
                    colon(l),
                    tr!(l, "atk ou def", "atk or def")
                ))?,
            };
            let tag = a.opt("tag").map(|t| fold(t.trim_start_matches('#')));
            let pinned = a.flag("pinned")?;
            let limit = a.count("limit", 50, 200);
            let start = match a.opt("cursor") {
                None => 0,
                Some(c) => c.parse::<usize>().map_err(|_| {
                    tr!(
                        l,
                        "cursor invalide : redonnez le nextCursor de la page précédente",
                        "invalid cursor: pass back the previous page's nextCursor"
                    )
                    .to_string()
                })?,
            };
            let all: Vec<&Arc<Note>> = s
                .notes()
                .iter()
                .filter(|n| map.is_none_or(|m| n.map == m))
                .filter(|n| kind.as_deref().is_none_or(|k| n.kind() == k))
                .filter(|n| side.as_deref().is_none_or(|sd| n.side().is_empty() || n.side() == sd))
                .filter(|n| tag.as_ref().is_none_or(|t| n.folded_tags.contains(t)))
                .filter(|n| pinned.is_none_or(|p| n.doc.fm.pin == p))
                .collect();
            let total = all.len();
            let end = (start + limit).min(total);
            let page = all.get(start.min(total)..end).unwrap_or(&[]);
            let mut text = if total == 0 {
                tr!(l, "Aucune note.", "No notes.").to_string()
            } else {
                format!("{}{}\n", notes_word(total), colon(l).trim_end())
            };
            text.push_str(&page.iter().map(|n| line(n, l)).collect::<Vec<_>>().join("\n"));
            let mut data = obj([("notes", notes_data(page.iter().copied())), ("total", total.into())]);
            if end < total {
                text.push_str(&format!("\n… {} = {end}", tr!(l, "suite avec cursor", "more with cursor")));
                data.push("nextCursor", end.to_string().into());
            }
            Ok(Reply::new(text, data))
        }
        "read_note" => {
            let (map, id) = (a.need("map")?, a.need("id")?);
            let n = find_note(s, map, id).ok_or_else(|| no_note(map, id, l))?;
            let text = format!(
                "# {}\n{}/{} · {} · {} {} · version {}\n\n---\n\n{}",
                n.title,
                n.map,
                n.id,
                describe(&n, l),
                tr!(l, "modifiée le", "updated"),
                &iso_utc(n.updated_ts)[..10],
                n.version,
                n.doc.body
            );
            let mut data = note_data(&n);
            data.push("version", n.version.as_str().into());
            data.push("body", n.doc.body.as_str().into());
            Ok(Reply::new(text, data))
        }
        "search_notes" => {
            let q = a.short("query", MAX_QUERY)?.ok_or_else(|| a.missing("query"))?;
            let limit = a.count("limit", 20, 50);
            let (total, hits) = search::search(s.notes(), s.maps(), &search::parse(q), limit);
            let lines: Vec<String> = hits
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    format!(
                        "{}. {}/{} — {} ({})\n   {}",
                        i + 1,
                        h.note.map,
                        h.note.id,
                        h.note.title,
                        describe(&h.note, l),
                        plain(&h.snippet)
                    )
                })
                .collect();
            let text = match (l, total) {
                (Lang::Fr, 0) => format!("Aucun résultat pour « {q} »."),
                (Lang::En, 0) => format!("No results for \"{q}\"."),
                (Lang::Fr, _) => format!("{} pour « {q} » :\n{}", count(total, "résultat", "résultats"), lines.join("\n")),
                (Lang::En, _) => format!("{} for \"{q}\":\n{}", count(total, "result", "results"), lines.join("\n")),
            };
            let hits: Vec<Json> = hits.iter().map(|h| obj([("note", note_data(&h.note)), ("snippet", plain(&h.snippet).into())])).collect();
            Ok(Reply::new(text, obj([("total", total.into()), ("hits", hits.into())])))
        }
        "match_sheet" => {
            let id = a.need("map")?;
            let m = s.map(id).ok_or_else(|| no_map(id, l))?;
            let markdown = sheet_markdown(s, id, l).unwrap_or_default();
            let pinned = |map: &str| s.notes_of(map).filter(|n| n.doc.fm.pin).cloned().collect::<Vec<_>>();
            let team = if id == TEAM_MAP { Vec::new() } else { pinned(TEAM_MAP) };
            let data = obj([
                ("map", m.id.as_str().into()),
                ("title", m.title.as_str().into()),
                ("notes", notes_data(&pinned(id))),
                ("team", notes_data(&team)),
                ("markdown", markdown.as_str().into()),
            ]);
            Ok(Reply::new(markdown.clone(), data).link(sheet_uri(&m.id), &format!("{}-fiche", m.id), &sheet_title(&m.title, l)))
        }
        "list_tags" => {
            let map = a.opt("map");
            if let Some(m) = map
                && s.map(m).is_none()
            {
                return Err(no_map(m, l));
            }
            let mut counts: HashMap<String, (String, usize)> = HashMap::new();
            for n in s.notes().iter().filter(|n| map.is_none_or(|m| n.map == m)) {
                for t in &n.doc.fm.tags {
                    counts.entry(fold(t)).or_insert_with(|| (t.clone(), 0)).1 += 1;
                }
            }
            let mut tags: Vec<(String, usize)> = counts.into_values().collect();
            tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| fold(&a.0).cmp(&fold(&b.0))));
            let text = if tags.is_empty() {
                tr!(l, "Aucune étiquette.", "No tags.").to_string()
            } else {
                tags.iter().map(|(t, c)| format!("#{t} ({c})")).collect::<Vec<_>>().join(", ")
            };
            let data: Vec<Json> = tags.iter().map(|(t, c)| obj([("tag", t.as_str().into()), ("count", (*c).into())])).collect();
            Ok(Reply::new(text, obj([("tags", data.into())])))
        }
        "recent_notes" => {
            let limit = a.count("limit", 10, 50);
            let mut notes: Vec<&Arc<Note>> = s.notes().iter().collect();
            notes.sort_by(|a, b| b.updated_ts.cmp(&a.updated_ts));
            notes.truncate(limit);
            let text = if notes.is_empty() {
                tr!(l, "Aucune note.", "No notes.").to_string()
            } else {
                notes
                    .iter()
                    .map(|n| format!("{} · {} {}", line(n, l), tr!(l, "modifiée le", "updated"), &iso_utc(n.updated_ts)[..10]))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(Reply::new(text, obj([("notes", notes_data(notes))])))
        }
        "format_note" => {
            let map = a.need("map")?;
            if s.map(map).is_none() {
                return Err(no_map(map, l));
            }
            let format = a.opt("format").unwrap_or("discord");
            let shared = match a.opt("id") {
                Some(id) => {
                    let n = find_note(s, map, id).ok_or_else(|| no_note(map, id, l))?;
                    api::NoteExport::take(s, n).shared(l)
                }
                None => api::share_map(s, map, a.flag("pinned_only")?.unwrap_or(false), l).ok_or_else(|| no_map(map, l))?,
            };
            let text = match format {
                "discord" => shared.discord,
                "text" => shared.text,
                "markdown" => shared.markdown,
                other => {
                    return Err(format!(
                        "{}{}{other} ({})",
                        tr!(l, "format inconnu", "unknown format"),
                        colon(l),
                        tr!(l, "discord, text ou markdown", "discord, text or markdown")
                    ));
                }
            };
            Ok(Reply::new(text.clone(), obj([("format", format.into()), ("title", shared.title.into()), ("text", text.into())])))
        }
        "list_trash" => {
            let items = s.trash();
            let text = if items.is_empty() {
                tr!(l, "La corbeille est vide.", "The trash is empty.").to_string()
            } else {
                items
                    .iter()
                    .map(|t| match l {
                        Lang::Fr => format!("- {} — {} (map {}, le {})", t.path, t.title, t.map, &iso_utc(t.deleted_ts)[..10]),
                        Lang::En => format!("- {} — {} (map {}, on {})", t.path, t.title, t.map, &iso_utc(t.deleted_ts)[..10]),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let data: Vec<Json> = items
                .iter()
                .map(|t| {
                    obj([
                        ("key", t.path.as_str().into()),
                        ("map", t.map.as_str().into()),
                        ("id", t.id.as_deref().into()),
                        ("title", t.title.as_str().into()),
                        ("deleted", iso_utc(t.deleted_ts).into()),
                    ])
                })
                .collect();
            Ok(Reply::new(text, obj([("items", data.into())])))
        }
        _ => Err(format!("{}{}{name}", tr!(l, "outil inconnu", "unknown tool"), colon(l))),
    }
}

// ── Écriture ──────────────────────────────────────────────────────────────────

/// Ce qui a changé depuis la lecture, pour le message de conflit.
#[derive(Clone, Copy)]
enum What {
    Note,
    Map,
    Item,
    Overview,
}

fn why(r: Refused, what: What, l: Lang) -> String {
    match r {
        Refused::Conflict(v) => match l {
            Lang::Fr => {
                let what = match what {
                    What::Note => "la note",
                    What::Map => "la map",
                    What::Item => "l'élément",
                    What::Overview => "la présentation",
                };
                format!(
                    "{what} a changé depuis sa lecture (version actuelle : {v}) : relisez-la, puis recommencez avec cette version — rien n'a été écrasé"
                )
            }
            Lang::En => {
                let what = match what {
                    What::Note => "the note",
                    What::Map => "the map",
                    What::Item => "the item",
                    What::Overview => "the overview",
                };
                format!(
                    "{what} changed since it was read (current version: {v}): read it again, then retry with that version — nothing was overwritten"
                )
            }
        },
        Refused::Busy => tr!(
            l,
            "le carnet est tenu par un autre programme protocol, sans fenêtre joignable : réessayez dans un instant",
            "the notebook is held by another protocol program, with no reachable window: try again in a moment"
        )
        .into(),
        Refused::Other(m) => m,
    }
}

impl Session {
    fn apply(&self, change: &Change, l: Lang) -> Result<Done, Refused> {
        let done = link::apply(&self.carnet, self.port, change, l)?;
        let at = format!("{}/{}", done.map, done.id.as_deref().unwrap_or(""));
        let message = match (l, done.via_window) {
            (Lang::Fr, true) => format!("écrit par la fenêtre ouverte : {at}"),
            (Lang::Fr, false) => format!("écrit dans le carnet : {at}"),
            (Lang::En, true) => format!("written through the open window: {at}"),
            (Lang::En, false) => format!("written to the notebook: {at}"),
        };
        self.live.log(INFO, &message);
        Ok(done)
    }

    /// La note telle que le carnet la garde maintenant, relue par le suiveur.
    fn note_after(&self, done: &Done, text: String, l: Lang) -> Result<Reply, String> {
        let s = &mut *self.live.store();
        let _ = s.refresh(true);
        let id = done.id.as_deref().unwrap_or("");
        let n = s.note(&done.map, id).ok_or_else(|| match l {
            Lang::Fr => format!("écrit, mais la note {}/{id} est introuvable à la relecture", done.map),
            Lang::En => format!("written, but note {}/{id} could not be found when reading it back", done.map),
        })?;
        let data = obj([("note", note_data(&n)), ("version", n.version.as_str().into()), ("backup", done.trash.as_deref().into())]);
        Ok(Reply::new(text, data).link(note_uri(&n.map, &n.id), &n.id, &n.title))
    }

    fn map_after(&self, done: &Done, text: String, l: Lang) -> Result<Reply, String> {
        let s = &mut *self.live.store();
        let _ = s.refresh(true);
        let m = s.map(&done.map).ok_or_else(|| match l {
            Lang::Fr => format!("écrit, mais la map {} est introuvable à la relecture", done.map),
            Lang::En => format!("written, but map {} could not be found when reading it back", done.map),
        })?;
        let data = obj([("map", map_detail(m)), ("backup", done.trash.as_deref().into())]);
        Ok(Reply::new(text, data).link(map_uri(&m.id), &m.id, &m.title))
    }

    fn current(&self, map: &str, id: &str, l: Lang) -> Result<Arc<Note>, String> {
        let s = &mut *self.live.store();
        let _ = s.refresh(true);
        s.note(map, id).ok_or_else(|| no_note(map, id, l))
    }
}

fn backup_line(done: &Done, l: Lang) -> String {
    match (&done.trash, l) {
        (Some(k), Lang::Fr) => {
            format!(
                " La version d'avant est à la corbeille (clé {k}) : restore_from_trash, ou « Restaurer » dans protocol, la remet à côté."
            )
        }
        (Some(k), Lang::En) => {
            format!(
                " The previous version is in the trash (key {k}): restore_from_trash, or \"Restore\" in protocol, puts it back alongside."
            )
        }
        (None, _) => String::new(),
    }
}

fn write_tool(session: &mut Session, name: &str, a: &Args) -> Result<Reply, String> {
    let l = a.l;
    let at = |done: &Done| format!("{}/{} — {}", done.map, done.id.as_deref().unwrap_or(""), done.title);
    match name {
        "create_note" => {
            let map = a.need("map")?.to_string();
            let title = a.short("title", MAX_TITLE)?.ok_or_else(|| a.missing("title"))?.to_string();
            let input = NoteInput {
                title: Some(title),
                kind: a.kind()?,
                side: a.side()?,
                tags: a.tags()?,
                pin: a.flag("pinned")?,
                body: a.long("body", MAX_BODY)?,
                map: None,
            };
            let done = session.apply(&Change::CreateNote { map, input }, l).map_err(|r| why(r, What::Map, l))?;
            let text = format!("{}{}{}.", tr!(l, "Note créée", "Note created"), colon(l), at(&done));
            session.note_after(&done, text, l)
        }
        "update_note" => {
            let (map, id) = (a.need("map")?.to_string(), a.need("id")?.to_string());
            let base = a.need("base_version")?.to_string();
            let input = NoteInput {
                title: a.short("title", MAX_TITLE)?.map(str::to_string),
                kind: a.kind()?,
                side: a.side()?,
                tags: a.tags()?,
                pin: None,
                body: a.long("body", MAX_BODY)?,
                map: None,
            };
            if input.title.is_none() && input.kind.is_none() && input.side.is_none() && input.tags.is_none() && input.body.is_none() {
                return Err(tr!(
                    l,
                    "rien à changer : donnez title, body, type, side ou tags",
                    "nothing to change: give title, body, type, side or tags"
                )
                .into());
            }
            let done = session
                .apply(&Change::SaveNote { map, id, input, base: Some(base), backup: true }, l)
                .map_err(|r| why(r, What::Note, l))?;
            let text = match l {
                Lang::Fr => format!("Note mise à jour : {}.{}", at(&done), backup_line(&done, l)),
                Lang::En => format!("Note updated: {}.{}", at(&done), backup_line(&done, l)),
            };
            session.note_after(&done, text, l)
        }
        "append_to_note" => {
            let (map, id) = (a.need("map")?, a.need("id")?);
            let add = a.long("text", MAX_APPEND)?.filter(|t| !t.trim().is_empty()).ok_or_else(|| a.missing("text"))?;
            // Lu puis réécrit avec sa version : un changement fait entre-temps n'est jamais perdu.
            for attempt in 0..2 {
                let cur = session.current(map, id, l)?;
                let body = if cur.doc.body.trim().is_empty() {
                    format!("{}\n", add.trim())
                } else {
                    format!("{}\n\n{}\n", cur.doc.body.trim_end(), add.trim())
                };
                let input = NoteInput { body: Some(body), ..NoteInput::default() };
                match session
                    .apply(&Change::SaveNote { map: map.into(), id: id.into(), input, base: Some(cur.version.clone()), backup: false }, l)
                {
                    Err(Refused::Conflict(_)) if attempt == 0 => continue,
                    Err(r) => return Err(why(r, What::Note, l)),
                    Ok(done) => {
                        let text = match l {
                            Lang::Fr => format!("Passage ajouté à la fin de {}.", at(&done)),
                            Lang::En => format!("Passage appended to the end of {}.", at(&done)),
                        };
                        return session.note_after(&done, text, l);
                    }
                }
            }
            Err(tr!(l, "la note change sans arrêt : réessayez dans un instant", "the note keeps changing: try again in a moment").into())
        }
        "set_pinned" => {
            let (map, id) = (a.need("map")?.to_string(), a.need("id")?.to_string());
            let pinned = a.flag("pinned")?.ok_or_else(|| a.missing("pinned"))?;
            session.current(&map, &id, l)?;
            let input = NoteInput { pin: Some(pinned), ..NoteInput::default() };
            let done =
                session.apply(&Change::SaveNote { map, id, input, base: None, backup: false }, l).map_err(|r| why(r, What::Note, l))?;
            let what = match (l, pinned) {
                (Lang::Fr, true) => "épinglée : elle est dans la fiche match",
                (Lang::Fr, false) => "désépinglée",
                (Lang::En, true) => "pinned: it is now in the match sheet",
                (Lang::En, false) => "unpinned",
            };
            let text = match l {
                Lang::Fr => format!("{} : {what}.", at(&done)),
                Lang::En => format!("{}: {what}.", at(&done)),
            };
            session.note_after(&done, text, l)
        }
        "move_note" => {
            let (map, id) = (a.need("map")?.to_string(), a.need("id")?.to_string());
            let to = a.need("to_map")?.to_string();
            let from = format!("{map}/{id}");
            let input = NoteInput { map: Some(to), ..NoteInput::default() };
            let done =
                session.apply(&Change::SaveNote { map, id, input, base: None, backup: false }, l).map_err(|r| why(r, What::Note, l))?;
            let text = match l {
                Lang::Fr => format!("{from} déplacée : la voici en {}.", at(&done)),
                Lang::En => format!("{from} moved: it is now {}.", at(&done)),
            };
            session.note_after(&done, text, l)
        }
        "trash_note" => {
            let (map, id) = (a.need("map")?.to_string(), a.need("id")?.to_string());
            let title = session.current(&map, &id, l)?.title.clone();
            let done = session.apply(&Change::Trash { map: map.clone(), id: id.clone() }, l).map_err(|r| why(r, What::Note, l))?;
            let key = done.trash.clone().unwrap_or_default();
            let _ = session.live.store().refresh(true);
            let text = match l {
                Lang::Fr => {
                    format!("« {title} » est à la corbeille (clé {key}) : restore_from_trash, ou « Restaurer » dans protocol, la remet.")
                }
                Lang::En => {
                    format!("\"{title}\" is in the trash (key {key}): restore_from_trash, or \"Restore\" in protocol, brings it back.")
                }
            };
            let data = obj([("trash", key.into()), ("map", map.into()), ("id", id.into()), ("title", title.into())]);
            Ok(Reply::new(text, data))
        }
        "restore_from_trash" => {
            let key = a.need("key")?.to_string();
            let done = session.apply(&Change::Restore { key }, l).map_err(|r| why(r, What::Item, l))?;
            let s = &mut *session.live.store();
            let _ = s.refresh(true);
            let (title, uri) = match &done.id {
                Some(id) => (s.note(&done.map, id).map(|n| n.title.clone()).unwrap_or_default(), note_uri(&done.map, id)),
                None => (s.map(&done.map).map(|m| m.title.clone()).unwrap_or_default(), map_uri(&done.map)),
            };
            let at = done.id.as_deref().map_or(done.map.clone(), |id| format!("{}/{id}", done.map));
            let text = match l {
                Lang::Fr => format!("Restauré : {at} — {title}."),
                Lang::En => format!("Restored: {at} — {title}."),
            };
            let data = obj([
                ("map", done.map.as_str().into()),
                ("id", done.id.as_deref().into()),
                ("title", title.as_str().into()),
                ("uri", uri.as_str().into()),
            ]);
            Ok(Reply::new(text, data).link(uri.clone(), done.id.as_deref().unwrap_or(&done.map), &title))
        }
        "create_map" => {
            let title = a.short("title", MAX_MAP_TITLE)?.ok_or_else(|| a.missing("title"))?.to_string();
            let done = session.apply(&Change::CreateMap { title }, l).map_err(|r| why(r, What::Map, l))?;
            let text = match l {
                Lang::Fr => format!("Map créée : {} — {}.", done.map, done.title),
                Lang::En => format!("Map created: {} — {}.", done.map, done.title),
            };
            session.map_after(&done, text, l)
        }
        "update_map" => {
            let map = a.need("map")?.to_string();
            let title = a.short("title", MAX_MAP_TITLE)?.map(str::to_string);
            let pool = a.flag("in_pool")?;
            let overview = a.long("overview", MAX_BODY)?;
            let base = a.opt("base_version").map(str::to_string);
            if overview.is_some() && base.is_none() {
                return Err(tr!(
                    l,
                    "base_version requise pour réécrire la présentation : prenez celle rendue par get_map",
                    "base_version is required to rewrite the overview: use the one returned by get_map"
                )
                .into());
            }
            if title.is_none() && pool.is_none() && overview.is_none() {
                return Err(tr!(
                    l,
                    "rien à changer : donnez title, in_pool ou overview",
                    "nothing to change: give title, in_pool or overview"
                )
                .into());
            }
            let done = session.apply(&Change::SaveMap { map, title, pool, overview, base }, l).map_err(|r| why(r, What::Overview, l))?;
            let text = match l {
                Lang::Fr => format!("Map mise à jour : {} — {}.{}", done.map, done.title, backup_line(&done, l)),
                Lang::En => format!("Map updated: {} — {}.{}", done.map, done.title, backup_line(&done, l)),
            };
            session.map_after(&done, text, l)
        }
        _ => Err(format!("{}{}{name}", tr!(l, "outil inconnu", "unknown tool"), colon(l))),
    }
}
