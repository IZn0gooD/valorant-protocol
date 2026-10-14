//! Une note = un fichier Markdown avec un en-tête YAML (« front matter »), le
//! format qu'Obsidian, VS Code, GitHub, Hugo ou Astro lisent déjà.
//!
//! On ne lit qu'un sous-ensemble de YAML : `clé: valeur`, listes `[a, b]` ou en
//! tirets, blocs `|` et `>`. Toute clé inconnue, ou écrite sous une forme qu'on
//! ne sait pas lire, est **conservée telle quelle** à la réécriture : un en-tête
//! enrichi ailleurs (Obsidian, script…) ne perd rien en passant ici.

use std::collections::HashSet;

use crate::i18n::Lang;

/// Les types de note proposés, avec leur modèle de départ. Les champs sans suffixe
/// sont le français (langue de référence) ; `*_en` l'anglais. L'id ne se traduit pas.
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    pub plural: &'static str,
    pub template: &'static str,
    pub label_en: &'static str,
    pub plural_en: &'static str,
    pub template_en: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind {
        id: "protocole",
        label: "Protocole",
        plural: "Protocoles",
        template: "## Objectif\n\n\n## Déclencheur\n- Quand :\n- Call :\n\n## Rôles\n| Joueur | Agent | Rôle | Utilitaire |\n|---|---|---|---|\n|  |  |  |  |\n\n## Déroulé\n1. \n2. \n3. \n\n## Si ça déraille\n- \n",
        label_en: "Protocol",
        plural_en: "Protocols",
        template_en: "## Goal\n\n\n## Trigger\n- When:\n- Call:\n\n## Roles\n| Player | Agent | Role | Utility |\n|---|---|---|---|\n|  |  |  |  |\n\n## Steps\n1. \n2. \n3. \n\n## If it goes wrong\n- \n",
    },
    Kind {
        id: "setup",
        label: "Setup",
        plural: "Setups",
        template: "## Positions\n| Joueur | Agent | Position | Utilitaire | Rotation |\n|---|---|---|---|---|\n|  |  |  |  |  |\n\n## Infos à prendre\n- \n\n## Rotations\n- Contact A :\n- Contact B :\n\n## Faiblesses connues\n- \n",
        label_en: "Setup",
        plural_en: "Setups",
        template_en: "## Positions\n| Player | Agent | Position | Utility | Rotation |\n|---|---|---|---|---|\n|  |  |  |  |  |\n\n## Info to gather\n- \n\n## Rotations\n- Contact A:\n- Contact B:\n\n## Known weaknesses\n- \n",
    },
    Kind {
        id: "execute",
        label: "Exécution",
        plural: "Exécutions",
        template: "## Timing\n- Départ :\n- Exécution à :\n\n## Ordre des utilitaires\n1. \n2. \n3. \n\n## Entrée\n| Ordre | Joueur | Agent | Zone à nettoyer |\n|---|---|---|---|\n| 1 |  |  |  |\n| 2 |  |  |  |\n\n## Post-plant\n- Spike :\n- Positions :\n",
        label_en: "Execute",
        plural_en: "Executes",
        template_en: "## Timing\n- Start:\n- Execute at:\n\n## Utility order\n1. \n2. \n3. \n\n## Entry\n| Order | Player | Agent | Area to clear |\n|---|---|---|---|\n| 1 |  |  |  |\n| 2 |  |  |  |\n\n## Post-plant\n- Spike:\n- Positions:\n",
    },
    Kind {
        id: "retake",
        label: "Retake",
        plural: "Retakes",
        template: "## Conditions\n- Joueurs en vie :\n- Utilitaire restant :\n\n## Regroupement\n- Point de regroupement :\n\n## Ordre d'entrée\n1. \n2. \n\n## Utilitaires\n- \n\n## Calls\n- \n",
        label_en: "Retake",
        plural_en: "Retakes",
        template_en: "## Conditions\n- Players alive:\n- Utility left:\n\n## Regroup\n- Regroup point:\n\n## Entry order\n1. \n2. \n\n## Utility\n- \n\n## Calls\n- \n",
    },
    Kind {
        id: "lineup",
        label: "Lineup",
        plural: "Lineups",
        template: "## Agent\n- \n\n## Placement\n- Où se placer :\n- Où viser :\n- Lancer :\n\n## Effet\n- \n\n<!-- Collez une capture d'écran (Ctrl+V) pour l'ajouter ici. -->\n",
        label_en: "Lineup",
        plural_en: "Lineups",
        template_en: "## Agent\n- \n\n## Placement\n- Stand at:\n- Aim at:\n- Throw:\n\n## Effect\n- \n\n<!-- Paste a screenshot (Ctrl+V) to add it here. -->\n",
    },
    Kind {
        id: "antistrat",
        label: "Anti-strat",
        plural: "Anti-strats",
        template: "## Adversaire\n- Équipe :\n- Match :\n\n## Tendances\n- En attaque :\n- En défense :\n\n## Joueurs clés\n- \n\n## Notre réponse\n- \n",
        label_en: "Anti-strat",
        plural_en: "Anti-strats",
        template_en: "## Opponent\n- Team:\n- Match:\n\n## Tendencies\n- On attack:\n- On defense:\n\n## Key players\n- \n\n## Our answer\n- \n",
    },
    Kind { id: "note", label: "Note", plural: "Notes", template: "", label_en: "Note", plural_en: "Notes", template_en: "" },
];

impl Kind {
    /// Le nom affiché, dans la langue voulue.
    pub fn label(&self, lang: Lang) -> &'static str {
        lang.pick(self.label, self.label_en)
    }
    pub fn plural(&self, lang: Lang) -> &'static str {
        lang.pick(self.plural, self.plural_en)
    }
    /// Le modèle de départ, dans la langue voulue.
    pub fn template(&self, lang: Lang) -> &'static str {
        lang.pick(self.template, self.template_en)
    }
}

pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// Ramène les variantes saisies à la main (« Exécution », « anti strat »…) à un id
/// connu ; un type inconnu est gardé tel qu'il a été écrit.
pub fn normalize_kind(raw: &str) -> String {
    let f = crate::util::fold(raw.trim()).replace([' ', '_'], "-");
    let id = match f.as_str() {
        "" => "",
        "protocole" | "protocol" | "protocoles" | "protocols" | "regle" | "regles" => "protocole",
        "setup" | "setups" | "def-setup" => "setup",
        "execute" | "exec" | "execution" | "executions" | "executes" | "strat" | "strats" => "execute",
        "retake" | "retakes" => "retake",
        "lineup" | "lineups" | "line-up" | "line-ups" => "lineup",
        "antistrat" | "anti-strat" | "anti-strats" | "antistrats" | "anti" => "antistrat",
        "note" | "notes" => "note",
        _ => return raw.trim().to_string(),
    };
    id.to_string()
}

/// `atk` / `def` / vide (les deux).
pub fn normalize_side(raw: &str) -> String {
    let f = crate::util::fold(raw.trim());
    match f.as_str() {
        "atk" | "att" | "attaque" | "attack" | "attacker" | "attackers" | "a" | "t" => "atk",
        "def" | "defense" | "defence" | "defender" | "defenders" | "d" | "ct" => "def",
        _ => "",
    }
    .to_string()
}

/// Une entrée d'en-tête gardée mot pour mot : sa clé (si la ligne en a une) et ses lignes.
#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    pub key: Option<String>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub side: Option<String>,
    pub tags: Vec<String>,
    pub pin: bool,
    pub updated: Option<String>,
    /// `_map.md` seulement : dans le pool de maps actif ?
    pub pool: Option<bool>,
    /// `_map.md` seulement : rang d'affichage.
    pub order: Option<i64>,
    /// Entrées inconnues (ou illisibles pour nous), conservées telles quelles.
    pub extra: Vec<Extra>,
}

impl FrontMatter {
    pub fn extra_lines(&self) -> Vec<&str> {
        self.extra.iter().flat_map(|e| e.lines.iter().map(String::as_str)).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub fm: FrontMatter,
    pub body: String,
}

/// Les seules clés interprétées. Pas d'alias : une clé `kind` ou `tag` écrite par
/// un autre outil reste à lui, intacte.
const KNOWN: [&str; 8] = ["title", "type", "side", "tags", "pin", "updated", "pool", "order"];

pub fn parse(text: &str) -> Document {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n");
    let mut lines = text.split('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Document { fm: FrontMatter::default(), body: text };
    }
    let mut head: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        let t = line.trim_end();
        if t == "---" || t == "..." {
            closed = true;
            break;
        }
        head.push(line);
    }
    if !closed {
        // En-tête jamais refermé : on ne touche à rien, tout est du contenu.
        return Document { fm: FrontMatter::default(), body: text };
    }
    let rest: Vec<&str> = lines.collect();
    let body = rest.join("\n").trim_start_matches('\n').to_string();

    let mut entries: Vec<Extra> = Vec::new();
    for line in head {
        let top_level = !line.starts_with([' ', '\t', '-', '#']) && !line.trim().is_empty();
        let key = if top_level { line.split_once(':').map(|(k, _)| k.trim().to_string()) } else { None };
        match (key, entries.last_mut()) {
            (Some(k), _) => entries.push(Extra { key: Some(k), lines: vec![line.to_string()] }),
            (None, Some(e)) if e.key.is_some() && !line.starts_with('#') && (line.trim().is_empty() || !top_level) => {
                e.lines.push(line.to_string())
            }
            _ => entries.push(Extra { key: None, lines: vec![line.to_string()] }),
        }
    }
    // Les lignes vides de fin d'entrée lui sont rendues comme lignes isolées.
    let mut split: Vec<Extra> = Vec::new();
    for mut e in entries {
        let mut trailing = Vec::new();
        while e.lines.len() > 1 && e.lines.last().is_some_and(|l| l.trim().is_empty()) {
            trailing.push(e.lines.pop().unwrap_or_default());
        }
        split.push(e);
        split.extend(trailing.into_iter().rev().map(|l| Extra { key: None, lines: vec![l] }));
    }

    let mut fm = FrontMatter::default();
    for e in split {
        let Some(k) = e.key.as_deref().and_then(|k| KNOWN.into_iter().find(|c| *c == k)) else {
            fm.extra.push(e);
            continue;
        };
        let inline = e.lines[0].split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
        let more = &e.lines[1..];
        let value = if k == "tags" {
            more.iter().all(|l| l.trim_start().starts_with('-')).then(String::new)
        } else if more.is_empty() {
            Some(scalar(inline))
        } else {
            block_scalar(inline, more)
        };
        let Some(v) = value else {
            // Une forme qu'on ne sait pas lire (carte, liste…) : gardée telle quelle.
            fm.extra.push(e);
            continue;
        };
        match k {
            "title" => fm.title = Some(v.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty()),
            "type" => fm.kind = Some(normalize_kind(&v)).filter(|s| !s.is_empty()),
            "side" => fm.side = Some(normalize_side(&v)).filter(|s| !s.is_empty()),
            "tags" => fm.tags = tags(inline, more),
            "pin" => fm.pin = boolean(&v).unwrap_or(false),
            "updated" => fm.updated = Some(v).filter(|s| !s.is_empty()),
            "pool" => fm.pool = boolean(&v),
            "order" => fm.order = v.trim().parse().ok(),
            _ => fm.extra.push(e),
        }
    }
    Document { fm, body }
}

/// Un scalaire en bloc (`|`, `>`, `|-`, `>+2`…) : ses lignes, désindentées.
fn block_scalar(indicator: &str, lines: &[String]) -> Option<String> {
    let indicator = indicator.split(" #").next().unwrap_or("").trim();
    let mut chars = indicator.chars();
    let style = chars.next()?;
    if !matches!(style, '|' | '>') || !chars.all(|c| matches!(c, '+' | '-') || c.is_ascii_digit()) {
        return None;
    }
    // Indentation comptée en espaces et tabulations seulement : les coupes tombent sur de l'ASCII.
    let indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start_matches([' ', '\t']).len()).min()?;
    if indent == 0 {
        return None;
    }
    let body: Vec<&str> = lines.iter().map(|l| l.get(indent..).unwrap_or("")).collect();
    Some(if style == '|' {
        body.join("\n").trim_end().to_string()
    } else {
        body.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
    })
}

fn boolean(s: &str) -> Option<bool> {
    match crate::util::fold(s).as_str() {
        "true" | "yes" | "on" | "oui" | "1" => Some(true),
        "false" | "no" | "off" | "non" | "0" => Some(false),
        _ => None,
    }
}

/// Un scalaire YAML : `"double"` (avec échappements), `'simple'` ou nu (commentaire `#` retiré).
fn scalar(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return out,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('0') => out.push('\0'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            out.push(c);
                        }
                    }
                    Some(other) => out.push(other),
                    None => {}
                },
                c => out.push(c),
            }
        }
        return out;
    }
    if let Some(inner) = raw.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    out.push('\'');
                } else {
                    return out;
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    match raw.find(" #") {
        Some(i) => raw[..i].trim_end().to_string(),
        None if raw.starts_with('#') => String::new(),
        None => raw.to_string(),
    }
}

/// Découpe une liste en flux `[a, "b, c"]` en respectant les guillemets.
fn flow_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            // Un guillemet n'ouvre une chaîne qu'en tête d'élément : `[C'est, x]` reste deux éléments.
            (None, '"' | '\'') if cur.trim().is_empty() => {
                quote = Some(c);
                cur.push(c);
            }
            (None, ',') => items.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    items.push(cur);
    items.iter().map(|s| scalar(s)).filter(|s| !s.is_empty()).collect()
}

fn tags(inline: &str, more: &[String]) -> Vec<String> {
    let mut out: Vec<String> = if let Some(inner) = inline.strip_prefix('[') {
        flow_items(inner.trim_end().strip_suffix(']').unwrap_or(inner))
    } else if !inline.is_empty() {
        let s = scalar(inline);
        if s.contains(',') {
            s.split(',').map(|t| t.trim().to_string()).collect()
        } else {
            s.split_whitespace().map(str::to_string).collect()
        }
    } else {
        Vec::new()
    };
    for line in more {
        if let Some(item) = line.trim_start().strip_prefix('-') {
            out.push(scalar(item));
        }
    }
    clean_tags(out)
}

/// Sans `#` de tête, sans doublon (insensible à la casse), sans vide ; 30 au plus.
/// Linéaire : un en-tête de 100 000 étiquettes ne gèle pas le serveur.
pub fn clean_tags<I: IntoIterator<Item = String>>(tags: I) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for t in tags {
        if out.len() >= 30 {
            break;
        }
        let t = t.trim().trim_start_matches('#').trim();
        if t.is_empty() || t.contains('\n') {
            continue;
        }
        let t: String = t.chars().take(40).collect();
        if seen.insert(t.to_lowercase()) {
            out.push(t);
        }
    }
    out
}

// ── Écriture ──────────────────────────────────────────────────────────────────

/// Ce que YAML lirait comme un nombre : `12`, `1_000`, `12:30`, `.inf`, `0x1F`…
fn looks_numeric(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-']).unwrap_or(s).to_ascii_lowercase();
    if matches!(t.as_str(), ".inf" | ".nan") {
        return true;
    }
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0o")).or_else(|| t.strip_prefix("0b")) {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    }
    t.chars().any(|c| c.is_ascii_digit()) && t.chars().all(|c| c.is_ascii_digit() || "_.:e+-".contains(c))
}

fn needs_quotes(s: &str, in_flow: bool) -> bool {
    let Some(first) = s.chars().next() else { return true };
    if s != s.trim() || "-?:,[]{}#&*!|>'\"%@`.".contains(first) {
        return true;
    }
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') || s.chars().any(char::is_control) {
        return true;
    }
    if in_flow && s.contains([',', '[', ']', '{', '}']) {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~" | "y" | "n") {
        return true;
    }
    // Nombre, date, heure : YAML les typerait.
    s.parse::<f64>().is_ok()
        || looks_numeric(s)
        || s.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && s.contains([':', '-'])
            && s.len() <= 25
            && s.chars().all(|c| c.is_ascii_digit() || "-:TZ.+ ".contains(c))
}

fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn yaml_scalar(s: &str, in_flow: bool) -> String {
    if needs_quotes(s, in_flow) { quoted(s) } else { s.to_string() }
}

pub fn render(doc: &Document) -> String {
    let fm = &doc.fm;
    let mut out = String::from("---\n");
    let mut emitted: Vec<&str> = Vec::new();
    let mut emit = |key: &'static str, value: String, out: &mut String| {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&value);
        out.push('\n');
        emitted.push(key);
    };
    if let Some(t) = &fm.title {
        emit("title", yaml_scalar(t, false), &mut out);
    }
    if let Some(k) = fm.kind.as_deref().filter(|k| !k.is_empty()) {
        emit("type", yaml_scalar(k, false), &mut out);
    }
    if let Some(s) = fm.side.as_deref().filter(|s| !s.is_empty()) {
        emit("side", yaml_scalar(s, false), &mut out);
    }
    if !fm.tags.is_empty() {
        let items: Vec<String> = fm.tags.iter().map(|t| yaml_scalar(t, true)).collect();
        emit("tags", format!("[{}]", items.join(", ")), &mut out);
    }
    if fm.pin {
        emit("pin", "true".into(), &mut out);
    }
    if let Some(p) = fm.pool {
        emit("pool", p.to_string(), &mut out);
    }
    if let Some(o) = fm.order {
        emit("order", o.to_string(), &mut out);
    }
    if let Some(u) = fm.updated.as_deref().map(str::trim) {
        // Une vraie date reste nue (Obsidian y voit une date) ; autre chose est protégé.
        let value = if crate::util::parse_iso(u).is_some() { u.to_string() } else { yaml_scalar(u, false) };
        emit("updated", value, &mut out);
    }
    for e in &fm.extra {
        // Une clé reprise ci-dessus remplace son ancienne forme : jamais deux fois la même clé.
        if e.key.as_deref().is_some_and(|k| emitted.contains(&k)) {
            continue;
        }
        for line in &e.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("---\n");
    let body = doc.body.replace("\r\n", "\n");
    let body = body.trim_start_matches('\n');
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(body.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_and_keeps_unknown() {
        let src = "---\ntitle: \"Retake B: 3v3\"\ntype: Exécution\nside: Défense\ntags:\n  - retake\n  - '#B site'\naliases: [rb]\ncssclasses:\n  - wide\n# commentaire\npin: oui\nupdated: 2026-10-12T10:00:00Z\n---\n\n# Corps\n\ntexte\n";
        let d = parse(src);
        assert_eq!(d.fm.title.as_deref(), Some("Retake B: 3v3"));
        assert_eq!(d.fm.kind.as_deref(), Some("execute"));
        assert_eq!(d.fm.side.as_deref(), Some("def"));
        assert_eq!(d.fm.tags, vec!["retake", "B site"]);
        assert!(d.fm.pin);
        assert_eq!(d.fm.extra_lines(), vec!["aliases: [rb]", "cssclasses:", "  - wide", "# commentaire"]);
        assert_eq!(d.body, "# Corps\n\ntexte\n");

        let again = parse(&render(&d));
        assert_eq!(again, d, "\n{}", render(&d));
    }

    #[test]
    fn no_or_broken_front_matter_is_all_body() {
        assert_eq!(parse("# Juste un titre\n").body, "# Juste un titre\n");
        let broken = "---\ntitle: x\npas de fin\n";
        let d = parse(broken);
        assert_eq!(d.fm, FrontMatter::default());
        assert_eq!(d.body, broken);
    }

    #[test]
    fn crlf_and_bom() {
        let d = parse("\u{feff}---\r\ntitle: Setup A\r\ntags: a, b\r\n---\r\ncorps\r\n");
        assert_eq!(d.fm.title.as_deref(), Some("Setup A"));
        assert_eq!(d.fm.tags, vec!["a", "b"]);
        assert_eq!(d.body, "corps\n");
    }

    #[test]
    fn quoting_survives_round_trip() {
        let titles = [
            "yes",
            "12",
            "2026-10-12",
            "- tiret",
            "a: b",
            "a #b",
            "\"guillemets\"",
            "#tag",
            "fin:",
            "  espaces ",
            "ligne\nsuivante",
            "Retake B (3 joueurs)",
            "C'est l'heure",
            "C'est",
            "[crochets]",
            ".inf",
            "0x1F",
            "1_000",
            "12:30",
            ".cache",
        ];
        for t in titles {
            let d = Document {
                fm: FrontMatter { title: Some(t.to_string()), tags: vec![t.trim().to_string(), "x, y".into()], ..Default::default() },
                body: "b\n".into(),
            };
            let back = parse(&render(&d));
            let expected = t.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(back.fm.title.as_deref(), Some(expected.as_str()), "{}", render(&d));
            assert_eq!(back.fm.tags, clean_tags(vec![t.trim().to_string(), "x, y".into()]), "{}", render(&d));
        }
        for n in [".inf", "0x1F", "1_000", "12:30", "-.nan"] {
            assert!(needs_quotes(n, false), "{n}");
        }
    }

    #[test]
    fn block_scalars_and_odd_shapes_never_duplicate_keys() {
        let d = parse("---\ntitle: >\n  Retake B\n  à trois\ntype:\n  nom: retake\nupdated: 'note: a revoir'\n---\ncorps\n");
        assert_eq!(d.fm.title.as_deref(), Some("Retake B à trois"));
        assert_eq!(d.fm.kind, None, "une carte n'est pas un type : on la garde telle quelle");
        let out = render(&d);
        assert_eq!(out.matches("title:").count(), 1, "{out}");
        assert_eq!(out.matches("type:").count(), 1, "{out}");
        assert!(out.contains("updated: \"note: a revoir\""), "{out}");
        // L'interface fixe le type : l'ancienne forme disparaît, sans doublon.
        let mut d2 = d.clone();
        d2.fm.kind = Some("retake".into());
        let out2 = render(&d2);
        assert_eq!(out2.matches("type:").count(), 1, "{out2}");
        assert!(out2.contains("type: retake") && !out2.contains("nom: retake"), "{out2}");
        assert_eq!(parse(&out2).fm.kind.as_deref(), Some("retake"));
    }

    #[test]
    fn aliases_belong_to_other_tools() {
        let d = parse("---\nkind: retake\ntag: b\nmodified: 2026-01-01\ntags: [a]\n---\n");
        assert_eq!(d.fm.kind, None);
        assert_eq!(d.fm.tags, vec!["a"]);
        assert_eq!(d.fm.extra_lines(), vec!["kind: retake", "tag: b", "modified: 2026-01-01"]);
        assert_eq!(parse(&render(&d)), d);
    }

    #[test]
    fn tags_are_linear_and_capped() {
        let many: Vec<String> = (0..200_000).map(|i| format!("t{i}")).collect();
        let t0 = std::time::Instant::now();
        let out = clean_tags(many);
        assert_eq!(out.len(), 30);
        assert!(t0.elapsed() < std::time::Duration::from_millis(500), "{:?}", t0.elapsed());
        assert_eq!(clean_tags(vec!["B".into(), "b".into(), "#b".into(), " ".into()]), vec!["B"]);
    }

    #[test]
    fn map_fields() {
        let d = parse("---\ntitle: Ascent\npool: true\norder: 2\n---\n");
        assert_eq!(d.fm.pool, Some(true));
        assert_eq!(d.fm.order, Some(2));
        assert_eq!(parse(&render(&d)), d);
    }

    #[test]
    fn kinds_and_sides() {
        assert_eq!(normalize_kind("Anti strat"), "antistrat");
        assert_eq!(normalize_kind("Line-up"), "lineup");
        assert_eq!(normalize_kind("Perso Stuff"), "Perso Stuff", "un type inconnu reste tel quel");
        assert_eq!(normalize_side("Attaque"), "atk");
        assert_eq!(normalize_side("CT"), "def");
        assert_eq!(normalize_side("les deux"), "");
        assert!(KINDS.iter().all(|k| kind(k.id).is_some() && normalize_kind(k.id) == k.id));
    }

    #[test]
    fn kinds_speak_both_languages() {
        let exec = kind("execute").unwrap();
        assert_eq!((exec.label(Lang::Fr), exec.label(Lang::En)), ("Exécution", "Execute"));
        assert_eq!((exec.plural(Lang::Fr), exec.plural(Lang::En)), ("Exécutions", "Executes"));
        let proto = kind("protocole").unwrap();
        assert!(proto.template(Lang::Fr).starts_with("## Objectif"));
        assert!(proto.template(Lang::En).starts_with("## Goal") && proto.template(Lang::En).contains("| Player | Agent | Role | Utility |"));
        for k in KINDS {
            // Un nom anglais saisi à la main retombe sur le même type ; un modèle a autant de sections dans les deux langues.
            assert_eq!(normalize_kind(k.label(Lang::En)), k.id, "{}", k.id);
            assert_eq!(k.template(Lang::Fr).matches("\n## ").count(), k.template(Lang::En).matches("\n## ").count(), "{}", k.id);
            assert_eq!(k.template(Lang::Fr).matches('|').count(), k.template(Lang::En).matches('|').count(), "{}", k.id);
            assert!(!k.template(Lang::En).contains(" :"), "pas d'espace avant les deux-points en anglais : {}", k.id);
        }
    }
}
