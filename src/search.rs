//! Recherche plein texte, insensible aux accents et à la casse. Tous les mots
//! doivent apparaître (titre, tags, type ou texte). Filtres : `map:ascent`,
//! `type:retake`, `side:def`, `#tag`, et `"expression exacte"`.

use std::sync::Arc;

use crate::note;
use crate::store::{MapInfo, Note};
use crate::util::{escape_html, fold, unix_now};

#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub terms: Vec<String>,
    pub map: Option<String>,
    pub kind: Option<String>,
    pub side: Option<String>,
    pub tags: Vec<String>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.map.is_none() && self.kind.is_none() && self.side.is_none() && self.tags.is_empty()
    }
}

pub fn parse(q: &str) -> Query {
    let folded = fold(q);
    let mut tokens: Vec<(String, bool)> = Vec::new();
    let mut chars = folded.chars().peekable();
    while tokens.len() < 12 {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&c) = chars.peek() else { break };
        if c == '"' {
            chars.next();
            let phrase: String = chars.by_ref().take_while(|&c| c != '"').collect();
            tokens.push((phrase.trim().to_string(), true));
        } else {
            let mut word = String::new();
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                word.push(c);
            }
            tokens.push((word, false));
        }
    }
    let mut out = Query::default();
    for (tok, quoted) in tokens {
        if tok.is_empty() {
            continue;
        }
        if quoted {
            out.terms.push(tok);
            continue;
        }
        let filter = |prefixes: &[&str]| prefixes.iter().find_map(|p| tok.strip_prefix(p)).map(str::to_string);
        if let Some(v) = filter(&["map:", "m:"]) {
            out.map = Some(v).filter(|v| !v.is_empty());
        } else if let Some(v) = filter(&["type:", "t:"]) {
            out.kind = Some(note::normalize_kind(&v)).filter(|v| !v.is_empty());
        } else if let Some(v) = filter(&["side:", "cote:", "s:"]) {
            out.side = Some(note::normalize_side(&v)).filter(|v| !v.is_empty());
        } else if let Some(v) = filter(&["#", "tag:"]) {
            if !v.is_empty() {
                out.tags.push(v);
            }
        } else {
            out.terms.push(tok);
        }
    }
    out
}

pub struct Hit {
    pub note: Arc<Note>,
    pub score: f64,
    pub snippet: String,
}

fn kind_words(id: &str) -> String {
    // Les noms des deux langues : « execute » comme « exécution » trouvent le type.
    note::kind(id)
        .map(|k| fold(&format!("{} {} {} {} {}", k.id, k.label, k.plural, k.label_en, k.plural_en)))
        .unwrap_or_else(|| fold(id))
}

/// Les meilleurs résultats d'abord. Renvoie aussi le nombre total de résultats.
pub fn search(notes: &[Arc<Note>], maps: &[MapInfo], q: &Query, limit: usize) -> (usize, Vec<Hit>) {
    let now = unix_now();
    let mut scored: Vec<(f64, &Arc<Note>)> = Vec::new();
    for n in notes {
        if let Some(m) = &q.map {
            let title = maps.iter().find(|x| x.id == n.map).map(|x| fold(&x.title)).unwrap_or_default();
            if !fold(&n.map).starts_with(m.as_str()) && !title.starts_with(m.as_str()) {
                continue;
            }
        }
        if q.kind.as_deref().is_some_and(|k| n.kind() != k) || q.side.as_deref().is_some_and(|s| n.side() != s) {
            continue;
        }
        if !q.tags.iter().all(|t| n.folded_tags.iter().any(|nt| nt.starts_with(t.as_str()))) {
            continue;
        }
        let extra = format!("{} {}", kind_words(n.kind()), fold(&n.map));
        let mut score = 0.0;
        let mut all = true;
        for t in &q.terms {
            let mut s = 0.0;
            if let Some(pos) = n.folded_title.find(t.as_str()) {
                s += 10.0;
                if pos == 0 || n.folded_title[..pos].ends_with([' ', '-', '(', '\'']) {
                    s += 4.0;
                }
                if n.folded_title == *t {
                    s += 6.0;
                }
            }
            if n.folded_tags.iter().any(|tag| tag.contains(t.as_str())) {
                s += 6.0;
            }
            if extra.contains(t.as_str()) {
                s += 3.0;
            }
            let hits = n.folded_plain.matches(t.as_str()).take(10).count();
            if hits > 0 {
                s += 1.0 + hits as f64 * 0.3;
            }
            if s == 0.0 {
                all = false;
                break;
            }
            score += s;
        }
        if !all {
            continue;
        }
        if n.doc.fm.pin {
            score += 1.5;
        }
        // Les notes récentes passent légèrement devant (sur 30 jours).
        score += (30 - ((now - n.updated_ts) / 86_400).clamp(0, 30)) as f64 / 30.0;
        scored.push((score, n));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.folded_title.cmp(&b.1.folded_title)));
    let total = scored.len();
    let hits = scored.into_iter().take(limit).map(|(score, n)| Hit { snippet: snippet(n, &q.terms), score, note: n.clone() }).collect();
    (total, hits)
}

/// Un extrait autour de la première occurrence, mots trouvés entre `<mark>`.
/// Le texte replié et l'original ont le même nombre de caractères (voir
/// `util::fold_char`) : une position dans l'un vaut pour l'autre.
pub fn snippet(n: &Note, terms: &[String]) -> String {
    let first = terms.iter().filter_map(|t| n.folded_plain.find(t.as_str())).min();
    let mut out = String::new();
    let Some(byte_pos) = first else {
        escape_html(&n.excerpt, &mut out);
        return out;
    };
    let orig: Vec<char> = n.plain.chars().collect();
    let folded: Vec<char> = n.folded_plain.chars().collect();
    if orig.len() != folded.len() {
        escape_html(&n.excerpt, &mut out);
        return out;
    }
    let at = n.folded_plain[..byte_pos].chars().count();
    let mut start = at.saturating_sub(60);
    while start > 0 && start < at && !orig[start - 1].is_whitespace() {
        start += 1;
    }
    let end = (at + 140).min(orig.len());
    let window: String = folded[start..end].iter().collect();
    let mut marks: Vec<(usize, usize)> = Vec::new();
    for t in terms {
        let len = t.chars().count();
        for (b, _) in window.match_indices(t.as_str()) {
            let s = window[..b].chars().count();
            marks.push((s, s + len));
        }
    }
    marks.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in marks {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    if start > 0 {
        out.push('…');
    }
    let mut m = merged.iter().peekable();
    let mut open = false;
    for (i, c) in orig[start..end].iter().enumerate() {
        if !open && m.peek().is_some_and(|r| r.0 == i) {
            out.push_str("<mark>");
            open = true;
        }
        if *c == '\n' {
            out.push_str(" · ");
        } else {
            escape_html(c.encode_utf8(&mut [0; 4]), &mut out);
        }
        if open && m.peek().is_some_and(|r| r.1 == i + 1) {
            out.push_str("</mark>");
            open = false;
            m.next();
        }
    }
    if open {
        out.push_str("</mark>");
    }
    if end < orig.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::build_note;

    fn note(map: &str, id: &str, src: &str) -> Arc<Note> {
        Arc::new(build_note(map, id, src.to_string(), crate::util::unix_now()))
    }

    #[test]
    fn parses_filters_and_phrases() {
        let q = parse(r#"Retake map:Ascent type:Exécution side:Défense #B "fumée heaven""#);
        assert_eq!(
            q,
            Query {
                terms: vec!["retake".into(), "fumee heaven".into()],
                map: Some("ascent".into()),
                kind: Some("execute".into()),
                side: Some("def".into()),
                tags: vec!["b".into()],
            }
        );
        assert!(parse("   ").is_empty());
    }

    #[test]
    fn ranks_and_highlights() {
        let notes = vec![
            note(
                "ascent",
                "retake-b",
                "---\ntitle: Retake B\ntype: retake\nside: def\ntags: [b]\n---\nRegroupement au marché, fumée sur l'arrière-site.\n",
            ),
            note("bind", "setup-a", "---\ntitle: Setup A\ntype: setup\n---\nSi retake, on joue la fumée haute.\n"),
            note("bind", "exec-b", "---\ntitle: Exécution B\ntype: execute\n---\nRien à voir.\n"),
        ];
        let (total, hits) = search(&notes, &[], &parse("retake fumee"), 10);
        assert_eq!(total, 2);
        assert_eq!(hits[0].note.id, "retake-b", "le titre pèse plus lourd que le texte");
        assert!(hits[0].snippet.contains("<mark>fumée</mark>"), "{}", hits[0].snippet);
        let (total, _) = search(&notes, &[], &parse("map:bind"), 10);
        assert_eq!(total, 2);
        let (_, hits) = search(&notes, &[], &parse("exécutions"), 10);
        assert_eq!(hits[0].note.id, "exec-b", "le type compte, au pluriel comme au singulier");
        let (total, _) = search(&notes, &[], &parse("#b side:def"), 10);
        assert_eq!(total, 1);
        let (total, _) = search(&notes, &[], &parse("retake introuvable"), 10);
        assert_eq!(total, 0);
    }

    #[test]
    fn snippet_escapes_and_marks() {
        let n = note("x", "y", &format!("{}Fumée & co < 3 {}\n", "début ".repeat(20), "fin ".repeat(40)));
        let s = snippet(&n, &["fumee".into()]);
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.contains("<mark>Fumée</mark> &amp; co &lt; 3"), "{s}");
    }
}
