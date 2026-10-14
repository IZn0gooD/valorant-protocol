//! Partager un protocole : le même texte, mis en forme pour l'endroit où on le colle.
//!
//! - **Discord** : le Markdown de la note, avec ce que Discord n'affiche pas
//!   remplacé. Un tableau devient un bloc de code aux colonnes alignées, une case
//!   à cocher ☐ ou ☑, un encadré `> [!WARNING]` une citation titrée, une image sa
//!   légende, un lien vers un fichier son seul texte, un titre trop profond une
//!   ligne en gras.
//! - **Texte** : aucune marque, pour un message ou un courriel sans mise en forme.
//!   Puces « • », tableaux alignés, liens suivis de leur adresse.
//!
//! Le reste du texte n'est pas retouché : ce que le coach a écrit reste tel quel.
//! Chaque ligne est parcourue en temps linéaire : une ligne piégée (un fichier
//! importé, synchronisé) ne peut pas faire attendre le carnet.

use std::collections::HashMap;

use pulldown_cmark::{Alignment, BlockQuoteKind, Event, HeadingLevel, LinkType, Parser, Tag, TagEnd};

use crate::i18n::Lang;
use crate::render;

/// Une section d'un document partagé : une note dans l'export d'une map.
pub struct Part<'a> {
    pub title: &'a str,
    pub meta: String,
    pub body: &'a str,
}

/// Le titre d'une note désignée par `[[cible]]`, s'il est connu.
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<String>;

fn char_width(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0x20D0..=0x20FF | 0xFE00..=0xFE0F => 0,
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F004
        | 0x1F0CF
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F200..=0x1F2FF
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F7E0..=0x1F7EB
        | 0x1F90C..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Largeur d'affichage approchée en police à chasse fixe : 2 pour les idéogrammes
/// et les émojis (un symbole suivi du sélecteur U+FE0F est dessiné en émoji),
/// 0 pour les marques combinantes.
pub fn width(s: &str) -> usize {
    let mut total = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let w = char_width(c);
        if w == 1 && chars.peek() == Some(&'\u{FE0F}') {
            chars.next();
            total += 2;
        } else {
            total += w;
        }
    }
    total
}

/// Un tableau en colonnes alignées, une ligne de traits sous l'en-tête.
fn grid(rows: &[Vec<String>], aligns: &[Alignment]) -> Vec<String> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut w = vec![1; cols];
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(width(c));
        }
    }
    let mut out = Vec::with_capacity(rows.len() + 1);
    for (k, r) in rows.iter().enumerate() {
        let mut line = String::new();
        for (i, &wi) in w.iter().enumerate() {
            let c = r.get(i).map_or("", String::as_str);
            let pad = wi.saturating_sub(width(c));
            let (left, right) = match aligns.get(i) {
                Some(Alignment::Right) => (pad, 0),
                Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                _ => (0, pad),
            };
            if i > 0 {
                line.push_str("  ");
            }
            line.push_str(&" ".repeat(left));
            line.push_str(c);
            line.push_str(&" ".repeat(right));
        }
        out.push(line.trim_end().to_string());
        if k == 0 && rows.len() > 1 {
            out.push(w.iter().map(|&n| "─".repeat(n)).collect::<Vec<_>>().join("  "));
        }
    }
    out
}

fn callout(kind: BlockQuoteKind, lang: Lang) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "ℹ Note",
        BlockQuoteKind::Tip => lang.pick("✔ Astuce", "✔ Tip"),
        BlockQuoteKind::Important => "❗ Important",
        BlockQuoteKind::Warning => lang.pick("⚠ Attention", "⚠ Warning"),
        BlockQuoteKind::Caution => lang.pick("⛔ Danger", "⛔ Caution"),
    }
}

/// Le début du texte qui remplace une image : `[image : légende]` (`[image: caption]`).
fn image_open(lang: Lang) -> &'static str {
    lang.pick("[image : ", "[image: ")
}

/// Le retrait d'une ligne, en colonnes : une tabulation mène au multiple de 4 suivant.
fn indent(line: &str) -> usize {
    let mut col = 0;
    for c in line.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - col % 4,
            _ => break,
        }
    }
    col
}

// ── Discord ───────────────────────────────────────────────────────────────────

/// Les cellules d'une ligne de tableau GFM, séparées par les `|` non échappés.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = match t.strip_suffix('|') {
        Some(s) if !s.ends_with('\\') => s,
        _ => t,
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// La ligne de séparation d'un tableau (`|---|:--:|`) : l'alignement de chaque colonne.
fn delimiter(line: &str) -> Option<Vec<Alignment>> {
    if !line.contains('|') || indent(line) > 3 {
        return None;
    }
    cells(line)
        .iter()
        .map(|c| {
            let core = c.trim_matches(':');
            (!core.is_empty() && core.chars().all(|x| x == '-')).then(|| match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => Alignment::Center,
                (false, true) => Alignment::Right,
                (true, false) => Alignment::Left,
                (false, false) => Alignment::None,
            })
        })
        .collect()
}

/// Une puce ou un numéro de liste en tête de texte (`- `, `* `, `1. `).
fn list_marker(t: &str) -> bool {
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    (t.starts_with(['-', '*', '+']) && t[1..].starts_with([' ', '\t']))
        || (digits > 0 && digits < 10 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")))
}

/// Une ligne qui ouvre un autre bloc : elle termine un tableau.
fn starts_block(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(['#', '>']) || t.starts_with("```") || t.starts_with("~~~") || list_marker(t)
}

/// Le texte d'une cellule, sans marques : `**A** [site](u)` → `A site`.
fn inline_plain(s: &str, resolve: Resolve, lang: Lang) -> String {
    // Lue comme la cellule d'un tableau d'une colonne : aucun bloc possible (`1.`, `-`),
    // et `_x_` ou `\|` y sont compris comme dans le tableau d'origine.
    let src = format!("| {} |\n|---|\n", s.trim());
    let mut p = Plain::new(resolve, lang);
    for ev in Parser::new_ext(&src, render::options()) {
        p.event(ev);
    }
    p.finish().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Le texte en ligne, sans marques ni liens : pour un titre écrit `# **Retake** B`.
pub(crate) fn plain_inline(s: &str) -> String {
    // Lu comme une cellule de tableau : ses `|` non échappés le sont, pour rester du texte.
    let mut esc = String::with_capacity(s.len());
    let mut escaped = false;
    for c in s.chars() {
        if c == '|' && !escaped {
            esc.push('\\');
        }
        escaped = c == '\\' && !escaped;
        esc.push(c);
    }
    inline_plain(&esc, &|_| None, Lang::Fr)
}

/// Une ouverture de bloc de code clôturé : la marque et sa longueur.
pub(crate) fn fence_open(line: &str) -> Option<(char, usize)> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3 && (c == '~' || !t[n..].contains('`'))).then_some((c, n))
}

pub(crate) fn fence_close(line: &str, c: char, n: usize) -> bool {
    let t = line.trim_start();
    indent(line) <= 3 && t.chars().take_while(|&x| x == c).count() >= n && t.trim_start_matches(c).trim().is_empty()
}

/// Un titre ATX (`## Rôles`) : son niveau et son texte.
pub(crate) fn heading(line: &str) -> Option<(usize, &str)> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let n = t.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&n) || !(t.len() == n || t[n..].starts_with([' ', '\t'])) {
        return None;
    }
    let text = t[n..].trim();
    // `## Rôles ##` : la clôture de dièses ne fait pas partie du titre.
    let closed = text.trim_end_matches('#');
    let text = if closed.len() < text.len() && (closed.is_empty() || closed.ends_with([' ', '\t'])) { closed.trim_end() } else { text };
    Some((n, text))
}

/// `> [!WARNING]` : la marque de citation et le type d'encadré.
fn alert(line: &str) -> Option<(&str, BlockQuoteKind)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('>')?;
    let prefix = &line[..line.len() - rest.len()];
    let tag = rest.trim().strip_prefix("[!")?.strip_suffix(']')?;
    let kind = match tag.to_ascii_uppercase().as_str() {
        "NOTE" => BlockQuoteKind::Note,
        "TIP" => BlockQuoteKind::Tip,
        "IMPORTANT" => BlockQuoteKind::Important,
        "WARNING" => BlockQuoteKind::Warning,
        "CAUTION" => BlockQuoteKind::Caution,
        _ => return None,
    };
    Some((prefix, kind))
}

fn is_rule(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && ['-', '*', '_'].iter().any(|&c| t.chars().all(|x| x == c))
}

/// Une ligne de paragraphe : un `---` qui la suit la souligne (un titre) au lieu de tracer un trait.
fn is_para(line: &str) -> bool {
    let t = line.trim_start();
    !t.is_empty() && indent(line) <= 3 && heading(line).is_none() && !t.starts_with('>') && !is_rule(line) && !list_marker(t)
}

/// `- [ ] veto` → `- ☐ veto`, citations et retraits compris.
fn task(line: &str) -> String {
    let b = line.as_bytes();
    let mut k = 0;
    loop {
        while k < b.len() && matches!(b[k], b' ' | b'\t') {
            k += 1;
        }
        if k < b.len() && b[k] == b'>' {
            k += 1;
            continue;
        }
        break;
    }
    let m = k;
    if k < b.len() && matches!(b[k], b'-' | b'*' | b'+') {
        k += 1;
    } else {
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k == m || k - m > 9 || k >= b.len() || !matches!(b[k], b'.' | b')') {
            return line.to_string();
        }
        k += 1;
    }
    let s = k;
    while k < b.len() && b[k] == b' ' {
        k += 1;
    }
    if k == s || k + 3 > b.len() || b[k] != b'[' || b[k + 2] != b']' || !matches!(b[k + 1], b' ' | b'x' | b'X') {
        return line.to_string();
    }
    let after = &line[k + 3..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return line.to_string();
    }
    format!("{}{}{after}", &line[..k], if b[k + 1] == b' ' { "☐" } else { "☑" })
}

/// Pour chaque ouvrant d'une ligne (crochet ou parenthèse), la position de son fermant :
/// un seul passage, échappements `\` compris.
fn bracket_pairs(s: &str, open: char, close: char) -> HashMap<usize, usize> {
    let (mut stack, mut out, mut esc) = (Vec::new(), HashMap::new(), false);
    for (i, c) in s.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' => esc = true,
            c if c == open => stack.push(i),
            c if c == close => {
                if let Some(o) = stack.pop() {
                    out.insert(o, i);
                }
            }
            _ => {}
        }
    }
    out
}

/// Les suites de backticks d'une ligne, et pour chacune la fin du code en ligne
/// qu'elle ouvre (s'il se ferme) : un seul passage.
fn code_spans(s: &str) -> HashMap<usize, usize> {
    let b = s.as_bytes();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            runs.push((i, n));
            i += n;
        } else {
            i += 1;
        }
    }
    // Un code ouvert par n backticks se ferme à la prochaine suite de n backticks.
    let mut out = HashMap::new();
    let mut open: HashMap<usize, usize> = HashMap::new(); // longueur → début ouvert
    for &(at, n) in &runs {
        match open.remove(&n) {
            Some(start) => {
                out.insert(start, at + n);
                // Les suites ouvertes entre les deux sont du texte du code : on les oublie.
                open.retain(|_, s| *s < start);
            }
            None => {
                open.insert(n, at);
            }
        }
    }
    out
}

/// Images, liens internes et liens vers des fichiers d'une ligne, hors code :
/// `![p](x.png)` → `[image : p]`, `[[retake-b]]` → le titre de la note,
/// `[setup](setup.md)` → `setup` (Discord ne sait pas l'ouvrir).
fn inline_discord(line: &str, resolve: Resolve, lang: Lang) -> String {
    // Paires de crochets, de parenthèses et de backticks calculées une fois, et une
    // recherche de « ]] » ratée n'est pas refaite : la ligne est parcourue en temps linéaire.
    let squares = bracket_pairs(line, '[', ']');
    let parens = bracket_pairs(line, '(', ')');
    let codes = code_spans(line);
    let at = |rest: &str| line.len() - rest.len();
    let mut no_wiki_end = false;
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            let n = rest.len() - rest.trim_start_matches('`').len();
            let end = codes.get(&at(rest)).map_or(n, |&e| e - at(rest));
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        if c == '\\' {
            let next = rest[1..].chars().next().map_or(1, |x| 1 + x.len_utf8());
            out.push_str(&rest[..next]);
            rest = &rest[next..];
            continue;
        }
        if !no_wiki_end && let Some(r) = rest.strip_prefix("![[") {
            match r.find("]]") {
                Some(e) => {
                    let inner = &r[..e];
                    let name = inner.split('|').next().unwrap_or("").trim();
                    out.push_str(&format!("{}{name}]", image_open(lang)));
                    rest = &rest[3 + inner.len() + 2..];
                    continue;
                }
                None => no_wiki_end = true,
            }
        }
        if !no_wiki_end && let Some(r) = rest.strip_prefix("[[") {
            match r.find("]]") {
                Some(e) => {
                    let inner = &r[..e];
                    let (target, label) = match inner.split_once('|') {
                        Some((t, l)) => (t.trim(), l.trim()),
                        None => (inner.trim(), ""),
                    };
                    let shown = if label.is_empty() { resolve(target).unwrap_or_else(|| target.to_string()) } else { label.to_string() };
                    out.push_str(&shown);
                    rest = &rest[2 + inner.len() + 2..];
                    continue;
                }
                None => no_wiki_end = true,
            }
        }
        let image = rest.starts_with("![");
        if (image || c == '[')
            && let Some(close) = squares.get(&(at(rest) + usize::from(image))).map(|&j| j - at(rest))
        {
            let label = rest[1 + usize::from(image)..close].trim();
            let tail = &rest[close + 1..];
            // Un lien se termine par `(adresse)` ; un lien par référence par `[ref]`.
            let dest =
                if tail.starts_with('(') { parens.get(&at(tail)).map(|&j| (j - at(tail) + 1, &tail[1..j - at(tail)])) } else { None };
            if image {
                let skip = dest
                    .map(|(n, _)| n)
                    .or_else(|| tail.starts_with('[').then(|| squares.get(&at(tail)).map(|&j| j - at(tail) + 1)).flatten());
                if let Some(skip) = skip {
                    out.push_str(&if label.is_empty() { "[image]".to_string() } else { format!("{}{label}]", image_open(lang)) });
                    rest = &tail[skip..];
                    continue;
                }
            } else if let Some((n, d)) = dest {
                let d = d.trim().trim_start_matches('<');
                if ["http://", "https://", "mailto:"].iter().any(|p| d.starts_with(p)) {
                    // Un lien vers le web : Discord l'affiche tel quel.
                    out.push_str(&rest[..close + 1 + n]);
                } else {
                    out.push_str(label);
                }
                rest = &tail[n..];
                continue;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Retire les commentaires `<!-- -->` d'une ligne, hors code en ligne (`` `<!--` ``
/// n'en est pas un) ; `open` suit ceux qui débordent sur plusieurs lignes.
/// `None` : la ligne n'était qu'un commentaire. Un seul passage sur la ligne.
fn uncomment(line: &str, open: &mut bool) -> Option<String> {
    let codes = code_spans(line);
    let b = line.as_bytes();
    let mut out = String::new();
    let mut seen = *open;
    let (mut i, mut kept) = (0, 0); // position lue, début du texte gardé
    loop {
        if *open {
            match line[i..].find("-->") {
                Some(j) => {
                    i += j + 3;
                    kept = i;
                    *open = false;
                }
                None => {
                    kept = b.len();
                    break;
                }
            }
        }
        if i >= b.len() {
            break;
        }
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            i = codes.get(&i).copied().unwrap_or(i + n);
            continue;
        }
        if b[i..].starts_with(b"<!--") {
            out.push_str(&line[kept..i]);
            i += 4;
            *open = true;
            seen = true;
            continue;
        }
        i += 1;
    }
    out.push_str(&line[kept.min(b.len())..]);
    if seen && out.trim().is_empty() { None } else { Some(out) }
}

/// Le niveau du titre le moins profond d'un texte (hors code).
fn top_heading(body: &str) -> Option<usize> {
    let mut fence = None;
    let mut top: Option<usize> = None;
    for line in body.lines() {
        if let Some((c, n)) = fence {
            if fence_close(line, c, n) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = fence_open(line) {
            fence = Some(f);
        } else if let Some((n, _)) = heading(line) {
            top = Some(top.map_or(n, |t| t.min(n)));
        }
    }
    top
}

/// Un bloc `~~~` que Discord n'affiche pas : on l'écrit en ```` ``` ```` si son contenu le permet.
fn tilde_to_backticks(lines: &[&str], from: usize, n: usize) -> bool {
    lines[from..].iter().take_while(|l| !fence_close(l, '~', n)).all(|l| !l.contains("```"))
}

/// Le corps d'une note prêt pour Discord. Les titres sont décalés pour que le
/// moins profond tombe au niveau `level` (jamais remontés) ; Discord n'en
/// affiche que trois, les suivants deviennent du gras.
pub fn discord_body(body: &str, level: usize, resolve: Resolve, lang: Lang) -> String {
    let shift = top_heading(body).map_or(0, |t| level.saturating_sub(t));
    let lines: Vec<&str> = body.lines().collect();
    // Chaque ligne produite, et si elle doit rester telle quelle (contenu d'un bloc de code).
    let mut out: Vec<(String, bool)> = Vec::new();
    // Bloc de code ouvert : sa marque, sa longueur, et s'il est réécrit en backticks.
    let mut fence: Option<(char, usize, bool)> = None;
    let mut comment = false;
    // La ligne précédente : du texte de paragraphe ? une citation ?
    let (mut para, mut quote) = (false, false);
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        i += 1;
        if let Some((c, n, convert)) = fence {
            if fence_close(raw, c, n) {
                fence = None;
                out.push((if convert { "```".to_string() } else { raw.to_string() }, false));
            } else {
                out.push((raw.to_string(), true));
            }
            continue;
        }
        let Some(line) = uncomment(raw, &mut comment) else { continue };
        let (was_para, was_quote) = (para, quote);
        para = is_para(&line);
        quote = line.trim_start().starts_with('>');
        if let Some((c, n)) = fence_open(&line) {
            let convert = c == '~' && tilde_to_backticks(&lines, i, n);
            fence = Some((c, n, convert));
            out.push((if convert { format!("```{}", line.trim_start().trim_start_matches('~')) } else { line }, false));
            para = false;
            continue;
        }
        // Un tableau : l'en-tête, la ligne de séparation, puis les lignes jusqu'au prochain bloc.
        if indent(&line) <= 3
            && line.contains('|')
            && heading(&line).is_none()
            && let Some(aligns) = lines.get(i).and_then(|l| delimiter(l))
            && cells(&line).len() == aligns.len()
        {
            let mut rows = vec![cells(&line)];
            i += 1;
            while i < lines.len()
                && !lines[i].trim().is_empty()
                && !starts_block(lines[i])
                && !is_rule(lines[i])
                && !lines[i].trim_start().starts_with("<!--")
            {
                rows.push(cells(lines[i]));
                i += 1;
            }
            let rows: Vec<Vec<String>> = rows
                .into_iter()
                .map(|r| {
                    let mut r: Vec<String> = r.iter().map(|c| inline_plain(c, resolve, lang)).collect();
                    r.resize(aligns.len(), String::new());
                    r
                })
                .collect();
            out.push(("```".into(), false));
            out.extend(grid(&rows, &aligns).into_iter().map(|l| (l, true)));
            out.push(("```".into(), false));
            para = false;
            continue;
        }
        if let Some((n, text)) = heading(&line) {
            let text = inline_discord(text, resolve, lang);
            let n = n + shift;
            out.push((if n <= 3 { format!("{} {text}", "#".repeat(n)) } else { format!("**{text}**") }, false));
            continue;
        }
        // Un encadré n'ouvre qu'une citation : au milieu d'une autre, c'est du texte.
        if !was_quote && let Some((prefix, kind)) = alert(&line) {
            out.push((format!("{prefix} **{}**", callout(kind, lang)), false));
            continue;
        }
        if is_rule(&line) && !was_para {
            out.push(("──────────".into(), false));
            continue;
        }
        let mut text = inline_discord(&task(&line), resolve, lang);
        // Un `\` final (saut de ligne forcé) : Discord passe déjà à la ligne, il afficherait le `\`.
        let backslashes = text.len() - text.trim_end_matches('\\').len();
        if backslashes % 2 == 1 {
            text.pop();
        }
        out.push((text, false));
    }
    // Un bloc laissé ouvert (pulldown le ferme en fin de note) est refermé ici :
    // sinon il avalerait la section suivante une fois les notes mises bout à bout.
    if let Some((c, n, convert)) = fence {
        out.push((if convert { "```".to_string() } else { c.to_string().repeat(n) }, false));
    }
    tidy(out)
}

/// Lignes vides compactées (une au plus) et retirées aux deux bouts ; les lignes
/// d'un bloc de code restent telles quelles (un schéma ASCII a besoin de ses blancs).
fn tidy(lines: Vec<(String, bool)>) -> String {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (l, verbatim) in lines {
        if verbatim {
            out.push(l);
            continue;
        }
        let l = l.trim_end().to_string();
        if l.is_empty() && out.last().is_none_or(|p: &String| p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

fn meta_line(meta: &str) -> String {
    format!("*{}*", meta.replace('*', "\\*"))
}

/// Un document entier pour Discord : titre, repères, corps, puis chaque section.
pub fn discord(title: &str, meta: &str, intro: &str, parts: &[Part], resolve: Resolve, lang: Lang) -> String {
    let mut out = vec![format!("# {}", title.trim())];
    if !meta.is_empty() {
        out.push(meta_line(meta));
    }
    let body = discord_body(intro, 2, resolve, lang);
    if !body.is_empty() {
        out.push(String::new());
        out.push(body);
    }
    for p in parts {
        out.push(String::new());
        out.push(format!("## {}", p.title.trim()));
        if !p.meta.is_empty() {
            out.push(meta_line(&p.meta));
        }
        let body = discord_body(p.body, 3, resolve, lang);
        if !body.is_empty() {
            out.push(String::new());
            out.push(body);
        }
    }
    out.join("\n")
}

// ── Texte brut ────────────────────────────────────────────────────────────────

/// Un conteneur qui préfixe ses lignes : citation (`│ `) ou élément de liste
/// (la puce sur la première ligne, un retrait sur les suivantes).
struct Frame {
    first: String,
    rest: String,
    used: bool,
}

/// Une image ouverte : où commence son texte, et le nom du fichier d'une image `![[…]]`.
struct OpenImage {
    at: usize,
    file: Option<String>,
}

struct Plain<'r> {
    resolve: Resolve<'r>,
    lang: Lang,
    /// Les lignes produites, et si elles doivent rester telles quelles (code).
    lines: Vec<(String, bool)>,
    text: String,
    cell: Option<String>,
    frames: Vec<Frame>,
    lists: Vec<Option<u64>>,
    table: Option<(Vec<Alignment>, Vec<Vec<String>>)>,
    /// Liens ouverts : adresse, début du texte du lien, afficher l'adresse ?, cible interne
    links: Vec<(String, usize, bool, Option<String>)>,
    images: Vec<OpenImage>,
    /// `[1] ` d'une note de bas de page, à placer devant son premier paragraphe.
    footnote: Option<String>,
    blank: bool,
}

impl<'r> Plain<'r> {
    fn new(resolve: Resolve<'r>, lang: Lang) -> Self {
        Plain {
            resolve,
            lang,
            lines: Vec::new(),
            text: String::new(),
            cell: None,
            frames: Vec::new(),
            lists: Vec::new(),
            table: None,
            links: Vec::new(),
            images: Vec::new(),
            footnote: None,
            blank: false,
        }
    }

    fn buf(&mut self) -> &mut String {
        self.cell.as_mut().unwrap_or(&mut self.text)
    }

    fn prefix(&mut self) -> String {
        let mut prefix = String::new();
        for f in &mut self.frames {
            prefix.push_str(if f.used { &f.rest } else { &f.first });
            f.used = true;
        }
        prefix
    }

    fn emit(&mut self, line: &str) {
        let l = format!("{}{line}", self.prefix());
        self.lines.push((l.trim_end().to_string(), false));
    }

    fn emit_code(&mut self, line: &str) {
        let l = format!("{}{line}", self.prefix());
        self.lines.push((l, true));
    }

    /// La ligne vide due avant un bloc : elle garde les barres de citation, pas les puces.
    fn gap(&mut self) {
        if self.blank && !self.lines.is_empty() {
            let prefix: String = self.frames.iter().map(|f| f.rest.as_str()).collect();
            self.lines.push((prefix.trim_end().to_string(), false));
        }
        self.blank = false;
    }

    fn flush(&mut self) {
        let t = std::mem::take(&mut self.text);
        if t.trim().is_empty() {
            return;
        }
        self.gap();
        for l in t.trim_matches('\n').split('\n') {
            self.emit(l.trim());
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            // Un bloc HTML arrive ligne à ligne : ses commentaires sont retirés à sa fin.
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) | Event::Html(t) => self.buf().push_str(&t),
            Event::InlineHtml(h) => {
                let h = render::strip_comments(&h);
                self.buf().push_str(&h);
            }
            Event::SoftBreak | Event::HardBreak => {
                let br = if self.cell.is_some() { ' ' } else { '\n' };
                self.buf().push(br);
            }
            Event::Rule => {
                self.flush();
                self.gap();
                self.emit("──────────");
                self.blank = true;
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "☑" } else { "☐" };
                match self.frames.last_mut() {
                    Some(f) if !f.used && f.rest.chars().all(|c| c == ' ') => {
                        // Une puce cède sa place à la case ; un numéro la garde devant elle.
                        f.first = if f.first.starts_with(['•', '◦']) { format!("{mark} ") } else { format!("{}{mark} ", f.first) };
                        f.rest = " ".repeat(width(&f.first));
                    }
                    _ => {
                        let m = format!("{mark} ");
                        self.buf().push_str(&m);
                    }
                }
            }
            Event::FootnoteReference(label) => {
                let r = format!("[{label}]");
                self.buf().push_str(&r);
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush();
                if let Some(f) = self.footnote.take() {
                    self.text.push_str(&f);
                }
            }
            Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::List(_) | Tag::Table(_) => {
                self.flush();
                match tag {
                    Tag::List(start) => self.lists.push(start),
                    Tag::Table(aligns) => self.table = Some((aligns, Vec::new())),
                    _ => {}
                }
            }
            Tag::BlockQuote(kind) => {
                self.flush();
                self.gap();
                self.frames.push(Frame { first: "│ ".into(), rest: "│ ".into(), used: false });
                if let Some(k) = kind {
                    self.emit(callout(k, self.lang));
                }
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ if depth > 1 => "◦ ".into(),
                    _ => "• ".into(),
                };
                let rest = " ".repeat(width(&marker));
                self.frames.push(Frame { first: marker, rest, used: false });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some((_, rows)) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.cell = Some(String::new()),
            Tag::Link { link_type, dest_url, .. } => {
                let start = self.buf().len();
                let web = dest_url.starts_with("http://") || dest_url.starts_with("https://");
                let show = web && !matches!(link_type, LinkType::Autolink | LinkType::Email | LinkType::WikiLink { .. });
                let wiki = matches!(link_type, LinkType::WikiLink { has_pothole: false }).then(|| dest_url.to_string());
                self.links.push((dest_url.to_string(), start, show, wiki));
            }
            Tag::Image { link_type, dest_url, .. } => {
                // `![[capture.png|300]]` : ce qui suit la barre est une taille, pas une légende.
                let file = matches!(link_type, LinkType::WikiLink { .. })
                    .then(|| dest_url.split('|').next().unwrap_or("").rsplit('/').next().unwrap_or("").trim().to_string());
                let at = self.buf().len();
                let open = image_open(self.lang);
                self.buf().push_str(open);
                self.images.push(OpenImage { at, file });
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.footnote = Some(format!("[{label}] "));
            }
            Tag::Strikethrough => self.buf().push_str("~~"),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::FootnoteDefinition => {
                self.flush();
                self.footnote = None;
                self.blank = true;
            }
            TagEnd::Heading(level) => {
                let t = std::mem::take(&mut self.text);
                let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if !t.is_empty() {
                    self.gap();
                    // Sans gras ni taille, les grands titres se distinguent en capitales.
                    let t = if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) { t.to_uppercase() } else { t };
                    self.emit(&t);
                }
                self.blank = true;
            }
            TagEnd::CodeBlock => {
                let t = std::mem::take(&mut self.text);
                if !t.trim().is_empty() {
                    self.gap();
                    for l in t.trim_end_matches('\n').split('\n') {
                        self.emit_code(l);
                    }
                }
                self.blank = true;
            }
            TagEnd::HtmlBlock => {
                let t = render::strip_comments(&std::mem::take(&mut self.text));
                if !t.trim().is_empty() {
                    self.gap();
                    for l in t.trim_end_matches('\n').split('\n') {
                        self.emit(l.trim_end());
                    }
                }
                self.blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.frames.pop();
                self.blank = true;
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank = true;
                }
            }
            TagEnd::Item => {
                self.flush();
                self.frames.pop();
            }
            TagEnd::TableCell => {
                let c = self.cell.take().unwrap_or_default();
                let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(row) = self.table.as_mut().and_then(|(_, rows)| rows.last_mut()) {
                    row.push(c);
                }
            }
            TagEnd::Table => {
                if let Some((aligns, rows)) = self.table.take() {
                    self.gap();
                    for l in grid(&rows, &aligns) {
                        self.emit_code(&l);
                    }
                }
                self.blank = true;
            }
            TagEnd::Link => {
                if let Some((dest, start, show, wiki)) = self.links.pop() {
                    let title = wiki.and_then(|w| (self.resolve)(&w));
                    let buf = self.buf();
                    let start = start.min(buf.len());
                    if let Some(t) = title {
                        buf.truncate(start);
                        buf.push_str(&t);
                    } else if show && buf[start..].trim() != dest {
                        buf.push_str(&format!(" ({dest})"));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(img) = self.images.pop() {
                    let open = image_open(self.lang);
                    let buf = self.buf();
                    let label_at = (img.at + open.len()).min(buf.len());
                    match img.file {
                        Some(file) => {
                            buf.truncate(img.at);
                            buf.push_str(&format!("{open}{file}]"));
                        }
                        // Pas de légende : « [image] ».
                        None if buf.len() == label_at => {
                            buf.truncate(img.at);
                            buf.push_str("[image]");
                        }
                        None => buf.push(']'),
                    }
                }
            }
            TagEnd::Strikethrough => self.buf().push_str("~~"),
            _ => {}
        }
    }

    fn finish(mut self) -> String {
        self.flush();
        tidy(self.lines)
    }
}

/// Le corps d'une note en texte brut.
pub fn plain_body(body: &str, resolve: Resolve, lang: Lang) -> String {
    let mut p = Plain::new(resolve, lang);
    for ev in Parser::new_ext(body, render::options()) {
        p.event(ev);
    }
    p.finish()
}

fn underline(title: &str, c: &str) -> String {
    c.repeat(width(title).clamp(3, 60))
}

/// Un document entier en texte brut : titre souligné, repères, corps, sections.
pub fn plain(title: &str, meta: &str, intro: &str, parts: &[Part], resolve: Resolve, lang: Lang) -> String {
    let title = title.trim();
    let mut out = vec![title.to_string(), underline(title, "═")];
    if !meta.is_empty() {
        out.push(meta.to_string());
    }
    let body = plain_body(intro, resolve, lang);
    if !body.is_empty() {
        out.push(String::new());
        out.push(body);
    }
    for p in parts {
        let t = p.title.trim();
        out.push(String::new());
        out.push(t.to_string());
        out.push(underline(t, "─"));
        if !p.meta.is_empty() {
            out.push(p.meta.clone());
        }
        let body = plain_body(p.body, resolve, lang);
        if !body.is_empty() {
            out.push(String::new());
            out.push(body);
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    fn titles(t: &str) -> Option<String> {
        (t == "retake-b").then(|| "Retake B à trois".to_string())
    }

    #[test]
    fn tables_become_aligned_code_for_discord() {
        let d = discord_body("## Rôles\n| Joueur | Agent |\n|---|--:|\n| **Kai** | Sova |\n| Léa | `Omen` |\n\nfin", 2, &none, Lang::Fr);
        assert_eq!(d, "## Rôles\n```\nJoueur  Agent\n──────  ─────\nKai      Sova\nLéa      Omen\n```\n\nfin");
    }

    #[test]
    fn a_table_ends_at_the_next_block() {
        let d = discord_body("| a | b |\n|---|---|\n| 1 |\n- suite", 2, &none, Lang::Fr);
        assert_eq!(d, "```\na  b\n─  ─\n1\n```\n- suite");
        // Une ligne qui n'est pas un tableau reste telle quelle.
        assert_eq!(discord_body("a | b\nrien", 2, &none, Lang::Fr), "a | b\nrien");
        assert_eq!(discord_body("| a |\n---", 2, &none, Lang::Fr), "| a |\n---");
        // Un titre suivi d'une ligne de traits reste un titre, comme pour pulldown.
        assert_eq!(discord_body("## Rôles | Joueurs\n|---|---|", 2, &none, Lang::Fr), "## Rôles | Joueurs\n|---|---|");
    }

    #[test]
    fn a_rule_or_a_comment_ends_a_table_like_pulldown() {
        for src in ["| a | b |\n|---|---|\n| 1 | 2 |\n---\nsuite", "| a | b |\n|---|---|\n| 1 | 2 |\n<!-- x -->\nsuite"] {
            let d = discord_body(src, 2, &none, Lang::Fr);
            let code = d.split("```").nth(1).unwrap_or("");
            assert!(!code.contains("suite") && !code.contains("---"), "{d}");
        }
    }

    #[test]
    fn underscore_emphasis_is_removed_in_discord_tables() {
        let d = discord_body("| a | b |\n|---|---|\n| _Kai_ | __Sova__ |\n| ~~Moe~~ | x\\|y |", 2, &none, Lang::Fr);
        assert_eq!(d, "```\na        b\n───────  ────\nKai      Sova\n~~Moe~~  x|y\n```");
    }

    #[test]
    fn discord_keeps_text_but_converts_what_it_cannot_show() {
        let src = "Intro **gras** [vlr](https://vlr.gg)\n\n- [ ] veto\n- [x] pick\n> [!WARNING]\n> pas de rush\n\n#### Détail\n\n---\n\n![lineup](../assets/a.png) ![[b.png]] [[retake-b]] [[bind/setup|setup A]] `[[code]]`\n<!-- note cachée -->\n```\n| pas | un tableau |\n|---|---|\n```";
        let d = discord_body(src, 2, &titles, Lang::Fr);
        let want = "Intro **gras** [vlr](https://vlr.gg)\n\n- ☐ veto\n- ☑ pick\n> **⚠ Attention**\n> pas de rush\n\n**Détail**\n\n──────────\n\n[image : lineup] [image : b.png] Retake B à trois setup A `[[code]]`\n```\n| pas | un tableau |\n|---|---|\n```";
        assert_eq!(d, want);
    }

    #[test]
    fn discord_details() {
        // Un lien vers un fichier du carnet : son texte seul ; un lien web reste un lien.
        assert_eq!(
            discord_body("voir [setup](setup-a.md) et [site](https://x.y/a_(b))", 2, &none, Lang::Fr),
            "voir setup et [site](https://x.y/a_(b))"
        );
        // `![x]` sans adresse ni référence reste du texte, comme pour pulldown.
        assert_eq!(discord_body("![important] et ![[c.png|300]]", 2, &none, Lang::Fr), "![important] et [image : c.png]");
        // Un saut de ligne forcé par `\` : le `\` disparaît (Discord passe déjà à la ligne).
        assert_eq!(discord_body("ligne\\\nsuite \\\\", 2, &none, Lang::Fr), "ligne\nsuite \\\\");
        // `~~~` devient ``` ; un encadré au milieu d'une citation reste du texte.
        assert_eq!(discord_body("~~~sh\necho\n~~~", 2, &none, Lang::Fr), "```sh\necho\n```");
        assert_eq!(discord_body("> a\n> [!NOTE]", 2, &none, Lang::Fr), "> a\n> [!NOTE]");
        // `---` sous un titre, une liste ou une citation est un trait ; sous un paragraphe, un titre souligné.
        assert_eq!(discord_body("## T\n---\n- a\n---", 2, &none, Lang::Fr), "## T\n──────────\n- a\n──────────");
        assert_eq!(discord_body("Titre\n---", 2, &none, Lang::Fr), "Titre\n---");
        // Un commentaire écrit dans du code en ligne n'en est pas un.
        assert_eq!(discord_body("Tape `<!--` puis\nla suite", 2, &none, Lang::Fr), "Tape `<!--` puis\nla suite");
    }

    #[test]
    fn code_blocks_keep_their_blank_lines_and_close() {
        let d = discord_body("```\n+---+\n\n\n|   |  \n", 2, &none, Lang::Fr);
        assert_eq!(d, "```\n+---+\n\n\n|   |  \n```");
        let parts = [
            Part { title: "A", meta: String::new(), body: "```\ncl_crosshair 1\n" },
            Part { title: "B", meta: String::new(), body: "| a | b |\n|---|---|\n| 1 | 2 |" },
        ];
        let d = discord("Ascent", "", "", &parts, &none, Lang::Fr);
        assert!(d.contains("cl_crosshair 1\n```\n\n## B"), "{d}");
    }

    #[test]
    fn long_lines_stay_linear() {
        for line in [
            "![[".repeat(50_000),
            "![".repeat(50_000),
            "[".repeat(80_000),
            "`` `".repeat(30_000),
            "<!--`".repeat(30_000),
            "<!--a-->x".repeat(30_000),
        ] {
            let t = std::time::Instant::now();
            let _ = discord_body(&line, 2, &none, Lang::Fr);
            assert!(t.elapsed() < std::time::Duration::from_millis(1500), "{:?}", t.elapsed());
        }
    }

    #[test]
    fn headings_shift_to_the_requested_level() {
        assert_eq!(discord_body("# A\n## B\n### C", 2, &none, Lang::Fr), "## A\n### B\n**C**");
        assert_eq!(discord_body("## A\n### B", 2, &none, Lang::Fr), "## A\n### B");
        assert_eq!(discord_body("## A ##\n####### pas un titre", 3, &none, Lang::Fr), "### A\n####### pas un titre");
    }

    #[test]
    fn whole_documents() {
        let parts = [Part { title: "Retake B", meta: "Retake · Défense".into(), body: "## Calls\n- stack" }];
        let d = discord("Ascent", "1 note", "## Défauts\nlent", &parts, &none, Lang::Fr);
        assert_eq!(d, "# Ascent\n*1 note*\n\n## Défauts\nlent\n\n## Retake B\n*Retake · Défense*\n\n### Calls\n- stack");
        let p = plain("Ascent", "1 note", "## Défauts\nlent", &parts, &none, Lang::Fr);
        let want = format!(
            "Ascent\n{}\n1 note\n\nDÉFAUTS\n\nlent\n\nRetake B\n{}\nRetake · Défense\n\nCALLS\n\n• stack",
            "═".repeat(6),
            "─".repeat(8)
        );
        assert_eq!(p, want);
    }

    #[test]
    fn plain_text_has_no_markup() {
        let src = "## Rôles\n\n| Joueur | Agent |\n|---|---|\n| **Kai** | Sova |\n\n1. fumée\n2. flash\n   - [x] vérifiée\n\n> [!TIP]\n> rester **groupés**\n\nvoir [vlr](https://vlr.gg), <https://x.y>, [[retake-b]] et ![carte](a.png)\n\n```\ncode   gardé\n```";
        let p = plain_body(src, &titles, Lang::Fr);
        let want = "RÔLES\n\nJoueur  Agent\n──────  ─────\nKai     Sova\n\n1. fumée\n2. flash\n   ☑ vérifiée\n\n│ ✔ Astuce\n│ rester groupés\n\nvoir vlr (https://vlr.gg), https://x.y, Retake B à trois et [image : carte]\n\ncode   gardé";
        assert_eq!(p, want);
    }

    #[test]
    fn plain_keeps_strikes_footnotes_numbers_and_code() {
        assert_eq!(plain_body("~~rush B~~ puis A", &none, Lang::Fr), "~~rush B~~ puis A");
        assert_eq!(plain_body("Voir[^1].\n\n[^1]: la source", &none, Lang::Fr), "Voir[1].\n\n[1] la source");
        assert_eq!(plain_body("1. [ ] fumée\n2. [x] flash", &none, Lang::Fr), "1. ☐ fumée\n2. ☑ flash");
        assert_eq!(plain_body("![[c.png|300]]", &none, Lang::Fr), "[image : c.png]");
        assert_eq!(plain_body("```\na\n\n\nb\n```", &none, Lang::Fr), "a\n\n\nb");
    }

    #[test]
    fn loose_lists_and_nested_quotes() {
        let p = plain_body("- a\n\n- b\n\n  suite\n\n> un\n>\n> deux\n\nfin", &none, Lang::Fr);
        assert_eq!(p, "• a\n\n• b\n\n  suite\n\n│ un\n│\n│ deux\n\nfin");
        assert_eq!(plain_body("![](x.png)", &none, Lang::Fr), "[image]");
    }

    #[test]
    fn widths_count_what_a_monospace_font_shows() {
        assert_eq!(width("Léa"), 3);
        assert_eq!(width("e\u{301}"), 1);
        assert_eq!(width("🔥a"), 3);
        assert_eq!(width("✅🚀🫡"), 6);
        assert_eq!(width("⚠\u{FE0F}"), 2);
        assert_eq!(grid(&[vec!["🔥".into(), "x".into()], vec!["ab".into(), "y".into()]], &[]), vec!["🔥  x", "──  ─", "ab  y"]);
    }

    #[test]
    fn cells_respect_escaped_pipes() {
        assert_eq!(cells("| a \\| b | c |"), vec!["a \\| b", "c"]);
        assert_eq!(inline_plain("a \\| b", &none, Lang::Fr), "a | b");
        assert_eq!(inline_plain("1.", &none, Lang::Fr), "1.");
        assert_eq!(inline_plain("- moi", &none, Lang::Fr), "- moi");
        assert_eq!(plain_inline("**Retake** _B_ [x](y)"), "Retake B x");
        assert_eq!(plain_inline("Rôles | Joueurs \\| fin"), "Rôles | Joueurs | fin");
    }
}
