//! Petits outils sans dépendance : dates UTC, empreintes, encodage d'URL,
//! normalisation des accents et fabrication de noms de fichiers sûrs.

use std::time::{SystemTime, UNIX_EPOCH};

// ── Dates ─────────────────────────────────────────────────────────────────────

/// Secondes Unix maintenant.
pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Secondes Unix d'un `SystemTime` (0 s'il précède l'époque).
pub fn unix_of(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// Algorithmes de Howard Hinnant (« chrono-Compatible Low-Level Date Algorithms »).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-12T14:03:09Z`
pub fn iso_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem / 60) % 60, rem % 60)
}

/// `20261012T140309Z` : horodatage compact pour les noms de fichiers.
pub fn stamp_utc(secs: i64) -> String {
    iso_utc(secs).replace(['-', ':'], "")
}

/// Lit une date ISO 8601 tolérante : `2026-10-12`, `2026-10-12 14:03`,
/// `2026-10-12T14:03:09.123+02:00`, `…Z`. Renvoie des secondes Unix UTC.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim().trim_matches(|c| c == '"' || c == '\'');
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = s.get(from..from + len)?;
        if part.bytes().all(|c| c.is_ascii_digit()) { part.parse().ok() } else { None }
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in_month = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, mo as u32, d as u32) * 86_400;
    if b.len() == 10 {
        return Some(secs);
    }
    if b[10] != b'T' && b[10] != b't' && b[10] != b' ' {
        return None;
    }
    let (h, mi) = (num(11, 2)?, num(14, 2)?);
    if b.get(13) != Some(&b':') || h > 23 || mi > 59 {
        return None;
    }
    secs += h * 3600 + mi * 60;
    let mut i = 16;
    if b.get(16) == Some(&b':') {
        let sec = num(17, 2)?;
        if sec > 60 {
            return None;
        }
        secs += sec;
        i = 19;
        if b.get(19) == Some(&b'.') {
            i = 20;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    match b.get(i) {
        None => Some(secs),
        Some(b'Z') | Some(b'z') if i + 1 == b.len() => Some(secs),
        Some(&sign @ (b'+' | b'-')) => {
            // ±HH, ±HHMM ou ±HH:MM, et rien après.
            let oh = num(i + 1, 2)?;
            let (om, end) = match b.get(i + 3) {
                None => (0, i + 3),
                Some(b':') => (num(i + 4, 2)?, i + 6),
                Some(_) => (num(i + 3, 2)?, i + 5),
            };
            if end != b.len() || oh > 23 || om > 59 {
                return None;
            }
            let off = oh * 3600 + om * 60;
            Some(if sign == b'+' { secs - off } else { secs + off })
        }
        _ => None,
    }
}

// ── Empreintes ────────────────────────────────────────────────────────────────

/// FNV-1a 64 bits : versions de fichiers, ETags et noms d'images. Pas un hachage
/// cryptographique, et il n'a pas besoin de l'être ici.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn hex64(v: u64) -> String {
    format!("{v:016x}")
}

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32 (IEEE, celui du ZIP et du PNG) : détecte un enregistrement abîmé ou
/// coupé en plein milieu dans le carnet, et sert aux archives ZIP.
pub fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |c, &b| CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

/// Un texte venu d'ailleurs : UTF-8 (sans BOM), sinon Windows-1252, l'encodage des
/// fichiers enregistrés par un vieux Bloc-notes. Jamais d'erreur, jamais de perte.
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // 0x80-0x9F : les caractères propres à Windows-1252 ; le reste suit Latin-1.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•',
        '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { HIGH[(b - 0x80) as usize] } else { b as char }).collect()
}

/// Date et heure au format MS-DOS (celui des archives ZIP), en UTC.
pub fn dos_datetime(secs: i64) -> (u16, u16) {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    let time = (((rem / 3600) << 11) | (((rem / 60) % 60) << 5) | ((rem % 60) / 2)) as u16;
    let date = if y < 1980 { (1 << 5) | 1 } else { (((y - 1980).min(127)) << 9 | (m as i64) << 5 | d as i64) as u16 };
    (time, date)
}

/// 128 bits tirés de l'aléa du système (les clés de `RandomState` en viennent),
/// en hexadécimal : de quoi faire un lien d'invitation impossible à deviner.
pub fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&hex64(h.finish()));
    }
    out
}

/// Comparaison à temps constant (pour les jetons).
pub fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── Encodage d'URL ────────────────────────────────────────────────────────────

/// Décode `%XX` (et `+` en espace si `plus`). `None` si l'UTF-8 obtenu est invalide.
pub fn percent_decode(s: &str, plus: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Encode un segment de chemin d'URL (tout sauf les non réservés RFC 3986).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// Valeur d'un paramètre de requête (`a=1&b=2`).
pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(k, true)? == key { percent_decode(v, true) } else { None }
    })
}

// ── Texte ─────────────────────────────────────────────────────────────────────

/// Minuscule sans accent, **caractère pour caractère** : la position d'un
/// caractère dans le texte replié est la même que dans l'original, ce qui permet
/// de surligner un extrait trouvé dans la version repliée.
pub fn fold_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'æ' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' | 'đ' | 'ð' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ğ' | 'ĝ' | 'ġ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => 'i',
        'ł' | 'ľ' | 'ĺ' | 'ļ' => 'l',
        'ñ' | 'ń' | 'ň' | 'ņ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' | 'œ' => 'o',
        'ř' | 'ŕ' => 'r',
        'š' | 'ś' | 'ş' | 'ș' | 'ß' => 's',
        'ť' | 'ţ' | 'ț' | 'þ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ž' | 'ź' | 'ż' => 'z',
        '\u{2019}' | '\u{2018}' | '\u{02bc}' => '\'',
        '\u{00a0}' | '\u{202f}' | '\u{2009}' => ' ',
        '\u{2013}' | '\u{2014}' => '-',
        other => other,
    }
}

pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

/// Noms de périphériques Windows : réservés quelle que soit l'extension (`nul.txt`).
fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
        || ["com", "lpt"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|n| n.chars().count() == 1 && n.chars().all(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')))
        })
}

/// `Retake B (3 joueurs)` → `retake-b-3-joueurs`. Toujours un nom de fichier
/// valide sur Windows, macOS et Linux.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().map(fold_char) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out = out.trim_matches('-').to_string();
    if out.len() > 60 {
        let cut = out[..60].rfind('-').filter(|&i| i >= 20).unwrap_or(60);
        out.truncate(cut);
        out = out.trim_end_matches('-').to_string();
    }
    if out.is_empty() {
        out.push_str("note");
    }
    if is_windows_reserved(&out) {
        out.push_str("-note");
    }
    out
}

/// Un nom (dossier de map ou fichier de note) qu'on accepte de lire ou d'écrire :
/// pas de séparateur, pas de caractère interdit sous Windows, pas de nom caché.
pub fn is_safe_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=120).contains(&n)
        && !s.starts_with(['.', '_', ' '])
        && !s.ends_with(['.', ' '])
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        && !is_windows_reserved(s)
}

/// Coupe à `max` caractères (pas octets), avec `…` si coupé.
pub fn clip_chars(s: &str, max: usize) -> String {
    let mut it = s.char_indices();
    match it.nth(max) {
        None => s.to_string(),
        Some((i, _)) => {
            let mut out = s[..i].trim_end().to_string();
            out.push('…');
            out
        }
    }
}

/// Échappe pour HTML (texte et valeur d'attribut).
pub fn escape_html(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_791_804_189), "2026-10-12T11:23:09Z");
        assert_eq!(parse_iso("2026-10-12T11:23:09Z"), Some(1_791_804_189));
        assert_eq!(parse_iso("2026-10-12T13:23:09+02:00"), Some(1_791_804_189));
        assert_eq!(parse_iso("2026-10-12 11:23"), Some(1_791_804_180));
        assert_eq!(parse_iso("2026-10-12T11:23:09.250Z"), Some(1_791_804_189));
        assert_eq!(parse_iso("2026-10-12"), Some(1_791_763_200));
        assert_eq!(parse_iso("2024-02-29"), Some(1_709_164_800));
        assert_eq!(parse_iso("12/10/2026"), None);
        assert_eq!(parse_iso("2026-13-01"), None);
        assert_eq!(parse_iso("2026-10-12T11:23:09Zjunk"), None);
        assert_eq!(parse_iso("2026-02-31"), None);
        assert_eq!(parse_iso("2025-02-29"), None);
        assert_eq!(parse_iso("2026-10-12T11:23:09+02:00junk"), None);
        assert_eq!(parse_iso("2026-10-12T13:23:09+0200"), Some(1_791_804_189));
        assert_eq!(stamp_utc(1_791_804_189), "20261012T112309Z");
        for secs in [-86_400 * 400, -1, 0, 951_782_400, 4_102_444_800] {
            assert_eq!(parse_iso(&iso_utc(secs)), Some(secs));
        }
    }

    #[test]
    fn folding_is_one_to_one() {
        let s = "Défense — Retake B, œil ÉCO’";
        assert_eq!(fold(s).chars().count(), s.chars().count());
        assert_eq!(fold(s), "defense - retake b, oil eco'");
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Retake B (3 joueurs)"), "retake-b-3-joueurs");
        assert_eq!(slugify("  Écran — fumée   CT  "), "ecran-fumee-ct");
        assert_eq!(slugify("!!!"), "note");
        assert_eq!(slugify("con"), "con-note");
        let long = slugify(&"exécution rapide ".repeat(10));
        assert!(long.len() <= 60 && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn safe_names() {
        assert!(is_safe_name("ascent"));
        assert!(is_safe_name("Retake B"));
        assert!(is_safe_name("général"));
        for bad in
            ["", ".", "..", ".git", "_map", "a/b", "a\\b", "c:", "nul", "nul.x", "com0", "lpt9", "conin$", "com¹", "a ", "a.", "x\u{0}y"]
        {
            assert!(!is_safe_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("Retake%20B", false).as_deref(), Some("Retake B"));
        assert_eq!(percent_decode("a+b", true).as_deref(), Some("a b"));
        assert_eq!(percent_decode("%C3%A9", false).as_deref(), Some("é"));
        assert_eq!(percent_decode("%FF", false), None);
        assert_eq!(percent_decode("%4", false), None);
        assert_eq!(percent_decode("%+F", false), None);
        assert_eq!(percent_encode("général B"), "g%C3%A9n%C3%A9ral%20B");
        assert_eq!(query_param("q=retake+b&limit=5", "q").as_deref(), Some("retake b"));
        assert_eq!(query_param("q=retake", "limit"), None);
    }

    #[test]
    fn tokens() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(same_secret(&a, &a.clone()) && !same_secret(&a, &b) && !same_secret("a", "ab"));
    }

    #[test]
    fn checksums_and_legacy_text() {
        // Valeurs de référence du CRC-32 IEEE.
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(decode_text("Défaut\n".as_bytes()), "Défaut\n");
        assert_eq!(decode_text(b"\xEF\xBB\xBFtitre"), "titre");
        // Un fichier du Bloc-notes en Windows-1252 : é, ’ et € retrouvés.
        assert_eq!(decode_text(b"D\xe9faut \x92ok\x92 5\x80"), "Défaut ’ok’ 5€");
        assert_eq!(dos_datetime(1_791_804_189), ((11 << 11 | 23 << 5 | 4) as u16, ((2026 - 1980) << 9 | 10 << 5 | 12) as u16));
    }

    #[test]
    fn clip() {
        assert_eq!(clip_chars("abc", 5), "abc");
        assert_eq!(clip_chars("éèàù", 2), "éè…");
    }
}
