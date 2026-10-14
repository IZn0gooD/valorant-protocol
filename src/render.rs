//! Markdown → HTML sûr. Le HTML brut est affiché comme du texte (jamais
//! interprété), les commentaires `<!-- -->` disparaissent, et chaque lien est
//! filtré : `javascript:` et consorts ne passent pas.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use crate::util::{percent_decode, percent_encode};

pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_FOOTNOTES
}

/// Retire les commentaires HTML d'un fragment.
pub(crate) fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i + 4..].find("-->") {
            Some(j) => rest = &rest[i + 4 + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Le schéma d'une URL (`https`, `javascript`…) s'il y en a un.
fn scheme(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let head = &url[..colon];
    let valid = !head.is_empty()
        && head.starts_with(|c: char| c.is_ascii_alphabetic())
        && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    // `C:/…` n'est pas un schéma, et un `:` après `/`, `?` ou `#` non plus.
    (valid && head.len() > 1).then(|| head.to_ascii_lowercase())
}

/// Résout un chemin relatif au dossier de la map, sans jamais sortir de la racine.
/// `("ascent", "../assets/x.png")` → `assets/x.png`.
pub fn resolve_rel(map: &str, rel: &str) -> Option<String> {
    let rel = percent_decode(rel, false).unwrap_or_else(|| rel.to_string());
    let mut parts: Vec<String> = if rel.starts_with(['/', '\\']) { Vec::new() } else { vec![map.to_string()] };
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s if s.starts_with('.') => return None,
            s => parts.push(s.to_string()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn file_url(rel_path: &str) -> String {
    let segs: Vec<String> = rel_path.split('/').map(percent_encode).collect();
    format!("/files/{}", segs.join("/"))
}

/// `[[retake-b]]`, `[[bind/setup-a|setup]]`, `retake-b.md`, `../bind/x.md#calls` → route interne.
fn note_route(map: &str, target: &str, wiki: bool) -> Option<String> {
    let target = target.split('#').next().unwrap_or("").trim();
    if target.is_empty() {
        return None;
    }
    let path = if wiki {
        let t = target.strip_suffix(".md").unwrap_or(target);
        if t.contains('/') { resolve_rel("", &format!("/{t}"))? } else { format!("{map}/{t}") }
    } else {
        let t = target.strip_suffix(".md").or_else(|| target.strip_suffix(".MD"))?;
        resolve_rel(map, t)?
    };
    let (m, id) = path.rsplit_once('/')?;
    if m.contains('/') {
        return None;
    }
    Some(format!("#/note/{}/{}", percent_encode(m), percent_encode(id)))
}

fn safe_link(map: &str, dest: &str, link_type: LinkType) -> String {
    if let LinkType::WikiLink { .. } = link_type {
        return note_route(map, dest, true).unwrap_or_else(|| "#".into());
    }
    if link_type == LinkType::Email {
        // pulldown-cmark préfixe déjà `mailto:`.
        return dest.to_string();
    }
    let d = dest.trim();
    if d.starts_with('#') {
        return d.to_string();
    }
    match scheme(d).as_deref() {
        Some("http" | "https" | "mailto") => d.to_string(),
        Some(_) => "#".into(),
        None => note_route(map, d, false).or_else(|| resolve_rel(map, d).map(|p| file_url(&p))).unwrap_or_else(|| "#".into()),
    }
}

fn safe_image(map: &str, dest: &str, link_type: LinkType) -> String {
    let d = dest.trim();
    if let LinkType::WikiLink { .. } = link_type {
        // `![[capture.png]]` : les images collées vivent dans `assets/` à la racine.
        let name = d.split('|').next().unwrap_or(d);
        let rel = if name.contains('/') { resolve_rel("", &format!("/{name}")) } else { resolve_rel("", &format!("/assets/{name}")) };
        return rel.map(|p| file_url(&p)).unwrap_or_default();
    }
    match scheme(d).as_deref() {
        Some("https" | "http") => d.to_string(),
        Some("data") if d[5..].trim_start().to_ascii_lowercase().starts_with("image/") => d.to_string(),
        Some(_) => String::new(),
        None => resolve_rel(map, d).map(|p| file_url(&p)).unwrap_or_default(),
    }
}

/// Les événements du document, assainis.
fn events<'a>(body: &'a str, map: &str) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    let mut raw_block: Option<String> = None;
    for ev in Parser::new_ext(body, options()) {
        if let Some(buf) = raw_block.as_mut() {
            match ev {
                Event::Html(h) | Event::Text(h) => buf.push_str(&h),
                Event::End(TagEnd::HtmlBlock) => {
                    let text = strip_comments(&raw_block.take().unwrap_or_default());
                    if !text.trim().is_empty() {
                        out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced("html".into()))));
                        out.push(Event::Text(CowStr::from(text)));
                        out.push(Event::End(TagEnd::CodeBlock));
                    }
                }
                _ => {}
            }
            continue;
        }
        out.push(match ev {
            Event::Start(Tag::HtmlBlock) => {
                raw_block = Some(String::new());
                continue;
            }
            Event::InlineHtml(h) | Event::Html(h) => {
                let text = strip_comments(&h);
                if text.is_empty() {
                    continue;
                }
                Event::Text(CowStr::from(text))
            }
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                Event::Start(Tag::Link { dest_url: CowStr::from(safe_link(map, &dest_url, link_type)), link_type, title, id })
            }
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                Event::Start(Tag::Image { dest_url: CowStr::from(safe_image(map, &dest_url, link_type)), link_type, title, id })
            }
            Event::InlineMath(m) | Event::DisplayMath(m) => Event::Code(m),
            // Un retour à la ligne s'affiche comme tel, comme sur Discord et dans Obsidian :
            // un coach écrit ligne à ligne, il ne s'attend pas à voir ses lignes fusionnées.
            Event::SoftBreak => Event::HardBreak,
            other => other,
        });
    }
    out
}

/// HTML prêt à insérer dans la page. `map` sert à résoudre les liens relatifs.
pub fn to_html(body: &str, map: &str) -> String {
    let mut s = String::with_capacity(body.len() * 3 / 2);
    html::push_html(&mut s, events(body, map).into_iter());
    // Alignement des tableaux en classes : la CSP de la page refuse les `style=""`.
    s.replace(" style=\"text-align: left\"", " class=\"al\"")
        .replace(" style=\"text-align: center\"", " class=\"ac\"")
        .replace(" style=\"text-align: right\"", " class=\"ar\"")
}

/// Le texte lisible, sans balisage : pour la recherche et les extraits.
pub fn plain_text(body: &str) -> String {
    let mut s = String::with_capacity(body.len());
    let mut raw_block = false;
    for ev in Parser::new_ext(body, options()) {
        match ev {
            Event::Start(Tag::HtmlBlock) => raw_block = true,
            Event::End(TagEnd::HtmlBlock) => raw_block = false,
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) if !raw_block => s.push_str(&t),
            Event::SoftBreak => s.push(' '),
            Event::HardBreak => s.push('\n'),
            Event::End(TagEnd::TableCell) => s.push_str(" · "),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::TableRow
                | TagEnd::TableHead
                | TagEnd::BlockQuote(_),
            ) => {
                if !s.ends_with('\n') {
                    s.push('\n');
                }
            }
            _ => {}
        }
    }
    // Espaces et lignes vides compactés, ` · ` des bords de lignes de tableau retirés ;
    // une ligne de cellules vides (un modèle pas encore rempli) disparaît.
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let line = line.trim().trim_matches(|c: char| c == '·' || c.is_whitespace());
        if !line.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

/// Les premières lignes non vides du Markdown brut : la vignette « ascii » des cartes.
pub fn preview(body: &str, max_lines: usize, max_cols: usize) -> String {
    let clean = strip_comments(body);
    clean
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("|---") && !l.trim_start().starts_with("| ---"))
        .take(max_lines)
        .map(|l| crate::util::clip_chars(&l.replace('\t', "  "), max_cols))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_is_shown_not_run() {
        let h = to_html("a <script>alert(1)</script> b\n\n<div onclick=x>bloc</div>\n", "ascent");
        assert!(!h.contains("<script"), "{h}");
        assert!(!h.contains("<div"), "{h}");
        assert!(h.contains("&lt;script&gt;"), "{h}");
        assert!(h.contains("&lt;div onclick=x&gt;bloc&lt;/div&gt;"), "{h}");
    }

    #[test]
    fn comments_vanish() {
        let h = to_html("<!-- indice -->\n\ntexte <!-- en ligne --> fin\n", "ascent");
        assert!(!h.contains("indice") && !h.contains("en ligne") && !h.contains("&lt;!--"), "{h}");
        assert!(h.contains("texte  fin"), "{h}");
        assert_eq!(preview("<!-- x -->\n## A\n|---|\nb", 5, 10), "## A\nb");
    }

    #[test]
    fn dangerous_links_are_neutered() {
        for src in [
            "[x](javascript:alert(1))",
            "[x](JaVaScRiPt:alert(1))",
            "[x](vbscript:msgbox)",
            "[x](data:text/html,<b>)",
            "[x](file:///C:/Windows)",
            "<javascript:alert(1)>",
        ] {
            let h = to_html(src, "ascent");
            let hrefs: Vec<&str> = h.split("href=\"").skip(1).map(|r| r.split('"').next().unwrap_or("")).collect();
            assert!(!hrefs.is_empty() && hrefs.iter().all(|u| *u == "#"), "{src} → {h}");
        }
        let img = to_html("![x](javascript:alert(1))", "ascent");
        assert!(img.contains("src=\"\""), "{img}");
    }

    #[test]
    fn links_resolve_to_routes_and_files() {
        let h = to_html(
            "[a](retake-b.md) [b](../bind/setup%20a.md#x) [[exec-rapide]] [[bind/defaut|défaut]] [c](https://vlr.gg) ![p](../assets/1.png) ![[2.png]] [d](../../hors.md)",
            "ascent",
        );
        assert!(h.contains("href=\"#/note/ascent/retake-b\""), "{h}");
        assert!(h.contains("href=\"#/note/bind/setup%20a\""), "{h}");
        assert!(h.contains("href=\"#/note/ascent/exec-rapide\""), "{h}");
        assert!(h.contains("href=\"#/note/bind/defaut\""), "{h}");
        assert!(h.contains("href=\"https://vlr.gg\""), "{h}");
        assert!(h.contains("src=\"/files/assets/1.png\""), "{h}");
        assert!(h.contains("src=\"/files/assets/2.png\""), "{h}");
        assert!(h.contains("<a href=\"#\">d</a>"), "{h}");
    }

    #[test]
    fn line_breaks_are_kept() {
        let h = to_html("Quand : spike posé\nCall : stack B\n\n- a\n  suite", "x");
        assert!(h.contains("Quand : spike posé<br />\nCall : stack B"), "{h}");
        assert!(h.contains("a<br />\nsuite"), "{h}");
    }

    #[test]
    fn tables_use_classes() {
        let h = to_html("| a | b |\n|:--|--:|\n| 1 | 2 |\n", "x");
        assert!(h.contains("class=\"al\"") && h.contains("class=\"ar\"") && !h.contains("style="), "{h}");
    }

    #[test]
    fn resolution_stays_inside() {
        assert_eq!(resolve_rel("ascent", "../assets/x.png").as_deref(), Some("assets/x.png"));
        assert_eq!(resolve_rel("ascent", "img/x.png").as_deref(), Some("ascent/img/x.png"));
        assert_eq!(resolve_rel("ascent", "../../x"), None);
        assert_eq!(resolve_rel("ascent", "../.git/config"), None);
        assert_eq!(resolve_rel("ascent", "/assets/x.png").as_deref(), Some("assets/x.png"));
        assert_eq!(resolve_rel("ascent", "..\\..\\x"), None);
    }

    #[test]
    fn plain_text_reads_well() {
        let p = plain_text("# Retake B\n\n- **fumée** sur `site`\n- flash\n\n| J | A |\n|---|---|\n| moi | Omen |\n|  |  |\n|  | B |\n");
        assert_eq!(p, "Retake B\nfumée sur site\nflash\nJ · A\nmoi · Omen\nB");
    }
}
