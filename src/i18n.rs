//! Les langues de protocol : le français (la langue de référence) et l'anglais.
//!
//! La langue retenue vient, dans l'ordre : du réglage du carnet (`lang` : `fr`, `en`, ou
//! vide pour « automatique »), de la variable `PROTOCOL_LANG`, puis de la langue de
//! l'interface du système — le français pour un système en français, l'anglais sinon.

/// Le réglage du carnet qui fixe la langue (vide : automatique).
pub const SETTING: &str = "lang";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    Fr,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// `fr`, `en`, `fr-FR`, `en_US.UTF-8`… ; autre chose : `None`.
    pub fn parse(s: &str) -> Option<Lang> {
        let s = s.trim().to_ascii_lowercase();
        if s.starts_with("fr") {
            Some(Lang::Fr)
        } else if s.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    /// Le texte de cette langue.
    pub fn pick<'a>(self, fr: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Fr => fr,
            Lang::En => en,
        }
    }
}

/// `tr!(lang, "français", "english")` : le texte dans la langue donnée.
#[macro_export]
macro_rules! tr {
    ($lang:expr, $fr:expr, $en:expr) => {
        $crate::i18n::Lang::pick($lang, $fr, $en)
    };
}

/// Un message du carnet (erreur, avis), écrit en français, dans la langue voulue : le
/// français tel quel, l'anglais d'après la table des messages connus. Un message
/// inconnu (une erreur du système, un texte venu d'ailleurs) reste tel quel.
pub fn localize(lang: Lang, message: &str) -> String {
    match lang {
        Lang::Fr => message.to_string(),
        Lang::En => english(message, 0).unwrap_or_else(|| message.to_string()),
    }
}

/// Les messages connus, en français puis en anglais. `{}` : un détail repris tel quel
/// (un nom, un nombre) ; `{*}` : un détail qui est lui-même un message (une cause),
/// traduit à son tour. Les messages sans détail sont cherchés d'abord, mot pour mot.
const MESSAGES: &[(&str, &str)] = &[
    // Carnet (store)
    ("cette note a changé depuis son ouverture (autre fenêtre)", "this note has changed since you opened it (another window)"),
    ("cette présentation a changé depuis son ouverture (autre fenêtre)", "this overview has changed since you opened it (another window)"),
    ("carnet ouvert en lecture seule", "notebook opened read-only"),
    ("aucun carnet à cet endroit", "no notebook at this location"),
    ("note trop longue (4 Mo au plus)", "note too long (4 MB at most)"),
    ("une note se déplace à part, sans copie de sauvegarde", "a note is moved on its own, without a backup copy"),
    ("élément introuvable dans la corbeille", "item not found in the trash"),
    ("cette map existe déjà", "this map already exists"),
    ("donnez un nom à la map", "give the map a name"),
    ("ce nom est réservé", "this name is reserved"),
    ("la map contient encore des notes", "the map still contains notes"),
    ("impossible de nommer l'image", "could not name the image"),
    ("le compactage s'est interrompu", "compaction was interrupted"),
    ("compactage abandonné", "compaction abandoned"),
    (
        "protocol a été arrêté pendant un enregistrement : la toute dernière modification n'avait pas pu être écrite en entier et a été écartée.",
        "protocol was stopped while saving: the very last change could not be written in full and was set aside.",
    ),
    // Fichier du carnet (carnet, zip)
    ("ce carnet a été écrit par une version plus récente de protocol", "this notebook was written by a newer version of protocol"),
    ("ce carnet est déjà ouvert par un autre programme protocol", "this notebook is already open in another protocol program"),
    ("ce fichier n'est pas un carnet protocol", "this file is not a protocol notebook"),
    ("enregistrement trop gros", "record too large"),
    ("carnet raccourci pendant la copie", "notebook shortened during the copy"),
    ("un carnet existe déjà à cet endroit", "a notebook already exists at this location"),
    ("carnet indisponible", "notebook unavailable"),
    ("carnet trop volumineux pour une archive ZIP", "notebook too large for a ZIP archive"),
    // API
    ("les modifications se font depuis le poste du coach", "changes can only be made from the coach's computer"),
    ("origine refusée", "origin refused"),
    ("carnet non partagé", "notebook not shared"),
    ("hôte non autorisé", "host not allowed"),
    ("méthode non autorisée", "method not allowed"),
    ("introuvable", "not found"),
    ("image introuvable", "image not found"),
    ("route inconnue", "unknown route"),
    ("corps JSON attendu (Content-Type: application/json)", "JSON body expected (Content-Type: application/json)"),
    ("corps illisible (UTF-8 attendu)", "unreadable body (UTF-8 expected)"),
    ("la corbeille se consulte sur le poste du coach", "the trash can only be viewed on the coach's computer"),
    ("le carnet entier s'exporte depuis le poste du coach", "the whole notebook can only be exported from the coach's computer"),
    ("image PNG, JPEG, GIF, WebP, AVIF ou BMP attendue", "PNG, JPEG, GIF, WebP, AVIF or BMP image expected"),
    ("lang : fr, en ou auto", "lang: fr, en or auto"),
    ("thème : dark, light ou auto", "theme: dark, light or auto"),
    // HTTP
    ("requête trop lente", "request too slow"),
    ("en-têtes trop longs", "headers too long"),
    ("requête incomplète", "incomplete request"),
    ("en-têtes illisibles", "unreadable headers"),
    ("ligne de requête invalide", "invalid request line"),
    ("cible invalide", "invalid target"),
    ("en-tête invalide", "invalid header"),
    ("transfer-encoding non pris en charge", "transfer-encoding not supported"),
    ("en-tête host en double", "duplicate host header"),
    ("content-length invalide", "invalid content-length"),
    ("content-length en double", "duplicate content-length"),
    ("lecture seule depuis le réseau : les modifications se font sur le poste du coach", "read-only from the network: changes are made on the coach's computer"),
    ("contenu trop volumineux", "content too large"),
    ("corps incomplet", "incomplete body"),
    ("serveur occupé", "server busy"),
    ("réponse HTTP sans en-têtes", "HTTP response without headers"),
    ("réponse HTTP illisible", "unreadable HTTP response"),
    // JSON (la cause de « JSON invalide à l'octet … »)
    ("contenu après la valeur", "content after the value"),
    ("valeur attendue", "value expected"),
    ("fin inattendue", "unexpected end"),
    ("imbrication trop profonde", "nesting too deep"),
    ("littéral inconnu", "unknown literal"),
    ("chiffre attendu", "digit expected"),
    ("chiffre attendu après le point", "digit expected after the decimal point"),
    ("exposant invalide", "invalid exponent"),
    ("nombre invalide", "invalid number"),
    ("\\u incomplet", "incomplete \\u"),
    ("\\u invalide", "invalid \\u"),
    ("échappement incomplet", "incomplete escape"),
    ("paire de substitution incomplète", "incomplete surrogate pair"),
    ("paire de substitution invalide", "invalid surrogate pair"),
    ("point de code invalide", "invalid code point"),
    ("échappement inconnu", "unknown escape"),
    ("caractère de contrôle dans une chaîne", "control character in a string"),
    ("chaîne non terminée", "unterminated string"),
    ("« , » ou « ] » attendu", "“,” or “]” expected"),
    ("« , » ou « } » attendu", "“,” or “}” expected"),
    // Ligne de commande
    ("search : que faut-il chercher ?", "search: what should I look for?"),
    ("export : md, discord, texte et --pinned s'appliquent à une map (--map <map>)", "export: md, discord, text and --pinned apply to a map (--map <map>)"),
    ("export : zip et json concernent tout le carnet (sans --map)", "export: zip and json cover the whole notebook (without --map)"),
    // ── Avec détail ──
    ("erreur disque : {*}", "disk error: {*}"),
    ("JSON invalide à l'octet {} : {*}", "invalid JSON at byte {}: {*}"),
    ("« {} » attendu", "“{}” expected"),
    ("nom de map invalide : {}", "invalid map name: {}"),
    ("map introuvable : {}", "map not found: {}"),
    ("nom de note invalide : {}", "invalid note name: {}"),
    ("nom de note refusé : {}", "note name refused: {}"),
    ("note introuvable : {}", "note not found: {}"),
    ("la map « {} » existe déjà", "the map “{}” already exists"),
    ("type d'image refusé : {}", "image type refused: {}"),
    ("impossible d'ouvrir l'explorateur : {*}", "could not open the file explorer: {*}"),
    ("export impossible : {*}", "export failed: {*}"),
    (
        "Le contenu de l'ancien dossier de notes ({}) a été repris dans {}. Ce dossier n'est plus utilisé et n'a pas été modifié : vous pouvez le supprimer une fois vos notes vérifiées ({}).",
        "The content of the old notes folder ({}) was imported into {}. That folder is no longer used and was not modified: you can delete it once you have checked your notes ({}).",
    ),
    (
        "Une partie du carnet était illisible ({} octets, support abîmé ?) et a été écartée. Une copie du fichier d'origine a été gardée à côté : {}.",
        "Part of the notebook was unreadable ({} bytes, damaged drive?) and was set aside. A copy of the original file was kept next to it: {}.",
    ),
    ("{} attend une valeur", "{} expects a value"),
    ("port invalide : {}", "invalid port: {}"),
    ("origine invalide : {} (ex. http://localhost:5173)", "invalid origin: {} (e.g. http://localhost:5173)"),
    ("--browser : app, default ou none, pas « {} »", "--browser: app, default or none, not “{}”"),
    ("--format : md, discord, texte, zip ou json, pas « {} »", "--format: md, discord, text, zip or json, not “{}”"),
    ("--limit invalide : {}", "invalid --limit: {}"),
    ("option inconnue : {}", "unknown option: {}"),
    ("commande inconnue : {}", "unknown command: {}"),
];

/// La traduction anglaise d'un message connu. `depth` borne la traduction des causes.
fn english(message: &str, depth: u8) -> Option<String> {
    if let Some((_, en)) = MESSAGES.iter().find(|(fr, _)| !fr.contains('{') && *fr == message) {
        return Some((*en).to_string());
    }
    MESSAGES.iter().filter(|(fr, _)| fr.contains('{')).find_map(|(fr, en)| {
        let mut details = captures(fr, message)?.into_iter();
        let mut out = String::new();
        for (i, piece) in split(en).0.into_iter().enumerate() {
            if i > 0 {
                let (detail, nested) = details.next()?;
                let cause = if nested && depth < 3 { english(detail, depth + 1) } else { None };
                out.push_str(cause.as_deref().unwrap_or(detail));
            }
            out.push_str(piece);
        }
        Some(out)
    })
}

/// Les morceaux fixes d'un modèle, entre ses `{}` / `{*}`, et pour chaque détail
/// s'il se traduit à son tour (`{*}`).
fn split(pattern: &str) -> (Vec<&str>, Vec<bool>) {
    let (mut pieces, mut nested) = (Vec::new(), Vec::new());
    let mut rest = pattern;
    loop {
        let next = [rest.find("{}").map(|i| (i, 2)), rest.find("{*}").map(|i| (i, 3))].into_iter().flatten().min();
        match next {
            Some((i, n)) => {
                pieces.push(&rest[..i]);
                nested.push(n == 3);
                rest = &rest[i + n..];
            }
            None => {
                pieces.push(rest);
                return (pieces, nested);
            }
        }
    }
}

/// Les détails d'un message écrit selon `pattern` (et, pour chacun, s'il se traduit
/// à son tour) ; `None` si le message ne suit pas le modèle. Chaque détail s'arrête au
/// premier endroit où reprend le texte fixe ; le dernier va jusqu'à la fin.
fn captures<'m>(pattern: &str, message: &'m str) -> Option<Vec<(&'m str, bool)>> {
    let (pieces, nested) = split(pattern);
    let mut rest = message.strip_prefix(pieces[0])?;
    let mut out = Vec::new();
    for (k, piece) in pieces[1..].iter().enumerate() {
        let last = k + 2 == pieces.len();
        let end = if last {
            rest.strip_suffix(piece).map(str::len)?
        } else if piece.is_empty() {
            return None;
        } else {
            rest.find(piece)?
        };
        let detail = &rest[..end];
        if detail.is_empty() {
            return None;
        }
        out.push((detail, nested.get(k).copied().unwrap_or(false)));
        rest = &rest[end + piece.len()..];
    }
    Some(out)
}

/// La langue de l'interface du système.
pub fn system() -> Lang {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // SAFETY : fonction sans argument ni effet de bord, présente sur tout Windows.
        let id = unsafe { GetUserDefaultUILanguage() };
        // L'identifiant de langue principal (les 10 bits du bas) : 0x0c = français.
        if id & 0x3ff == 0x0c { Lang::Fr } else { Lang::En }
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .map_or(Lang::En, |v| Lang::parse(&v).unwrap_or(Lang::En))
    }
}

/// La langue retenue pour un carnet, d'après son réglage (`None` ou vide : automatique).
pub fn resolve(setting: Option<&str>) -> Lang {
    setting
        .and_then(Lang::parse)
        .or_else(|| std::env::var("PROTOCOL_LANG").ok().as_deref().and_then(Lang::parse))
        .unwrap_or_else(system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_wins_then_variants_are_read() {
        assert_eq!(resolve(Some("en")), Lang::En);
        assert_eq!(resolve(Some("fr")), Lang::Fr);
        assert_eq!(Lang::parse("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::parse("fr-CA"), Some(Lang::Fr));
        assert_eq!(Lang::parse("de"), None);
        assert_eq!(tr!(Lang::En, "carnet", "notebook"), "notebook");
    }

    #[test]
    fn known_messages_are_translated_details_kept() {
        let en = |m: &str| localize(Lang::En, m);
        assert_eq!(en("carnet ouvert en lecture seule"), "notebook opened read-only");
        assert_eq!(en("route inconnue"), "unknown route");
        // « préfixe : détail » : le préfixe traduit, le détail gardé.
        assert_eq!(en("map introuvable : nulle-part"), "map not found: nulle-part");
        assert_eq!(en("note introuvable : ascent/retake-b"), "note not found: ascent/retake-b");
        assert_eq!(en("la map « district-9 » existe déjà"), "the map “district-9” already exists");
        assert_eq!(en("--limit attend une valeur"), "--limit expects a value");
        // Une cause connue se traduit aussi ; un nom qui ressemble à un message, non.
        assert_eq!(en("erreur disque : carnet ouvert en lecture seule"), "disk error: notebook opened read-only");
        assert_eq!(en("map introuvable : introuvable"), "map not found: introuvable");
        assert_eq!(en("JSON invalide à l'octet 3 : « : » attendu"), "invalid JSON at byte 3: “:” expected");
        assert_eq!(en("JSON invalide à l'octet 9 : « , » ou « ] » attendu"), "invalid JSON at byte 9: “,” or “]” expected");
        assert_eq!(en("erreur disque : Accès refusé. (os error 5)"), "disk error: Accès refusé. (os error 5)");
        // Les avis à plusieurs détails.
        let notice = "Le contenu de l'ancien dossier de notes (2 notes, 1 map) a été repris dans protocol.carnet. Ce dossier n'est plus \
                      utilisé et n'a pas été modifié : vous pouvez le supprimer une fois vos notes vérifiées (C:\\x\\notes).";
        assert_eq!(
            en(notice),
            "The content of the old notes folder (2 notes, 1 map) was imported into protocol.carnet. That folder is no longer used and \
             was not modified: you can delete it once you have checked your notes (C:\\x\\notes)."
        );
        // Inconnu : tel quel ; en français : tel quel.
        assert_eq!(en("quelque chose d'imprévu"), "quelque chose d'imprévu");
        assert_eq!(en("map introuvable : "), "map introuvable : ");
        assert_eq!(localize(Lang::Fr, "route inconnue"), "route inconnue");
        // Chaque modèle a autant de détails, dans le même ordre, dans les deux langues.
        for (fr, en) in MESSAGES {
            assert_eq!(split(fr).1, split(en).1, "{fr}");
        }
    }
}
