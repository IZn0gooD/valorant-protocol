//! JSON sans dépendance : une valeur, sa sérialisation et un analyseur strict
//! (profondeur bornée, échappements `\uXXXX` et paires de substitution).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// `obj([("a", 1.into()), ("b", "x".into())])`
pub fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            // La dernière occurrence gagne, comme JSON.parse.
            Json::Obj(pairs) => pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    /// Champ texte (`None` si absent ou d'un autre type).
    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Json::as_str)
    }
    /// Ajoute une paire à un objet (ignoré pour les autres types).
    pub fn push(&mut self, key: &str, value: Json) {
        if let Json::Obj(pairs) = self {
            pairs.push((key.to_string(), value));
        }
    }

    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if !n.is_finite() {
                    out.push_str("null");
                } else if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{n}");
                }
            }
            Json::Str(s) => write_str(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub fn to_json(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

macro_rules! from_num {
    ($($t:ty),*) => {$(
        impl From<$t> for Json {
            fn from(v: $t) -> Json { Json::Num(v as f64) }
        }
    )*};
}
from_num!(i32, i64, u32, u64, usize, f64);

impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}
impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_string())
    }
}
impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::Str(v)
    }
}
impl From<&String> for Json {
    fn from(v: &String) -> Json {
        Json::Str(v.clone())
    }
}
impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}
impl From<&[String]> for Json {
    fn from(v: &[String]) -> Json {
        Json::Arr(v.iter().map(Json::from).collect())
    }
}

// ── Analyse ───────────────────────────────────────────────────────────────────

const MAX_DEPTH: u32 = 64;

pub fn parse(src: &str) -> Result<Json, String> {
    let mut p = Parser { s: src, b: src.as_bytes(), i: 0, depth: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.err("contenu après la valeur"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("JSON invalide à l'octet {} : {what}", self.i)
    }
    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.b.get(self.i) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("« {} » attendu", c as char)))
        }
    }
    fn value(&mut self) -> Result<Json, String> {
        match self.b.get(self.i) {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("valeur attendue")),
            None => Err(self.err("fin inattendue")),
        }
    }
    fn nested(&mut self, f: fn(&mut Self) -> Result<Json, String>) -> Result<Json, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("imbrication trop profonde"));
        }
        let v = f(self);
        self.depth -= 1;
        v
    }
    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.s[self.i..].starts_with(word) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("littéral inconnu"))
        }
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let from = p.i;
            while p.b.get(p.i).is_some_and(u8::is_ascii_digit) {
                p.i += 1;
            }
            p.i > from
        };
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        if self.b.get(self.i) == Some(&b'0') {
            self.i += 1;
        } else if !digits(self) {
            return Err(self.err("chiffre attendu"));
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !digits(self) {
                return Err(self.err("chiffre attendu après le point"));
            }
        }
        if let Some(b'e' | b'E') = self.b.get(self.i) {
            self.i += 1;
            if let Some(b'+' | b'-') = self.b.get(self.i) {
                self.i += 1;
            }
            if !digits(self) {
                return Err(self.err("exposant invalide"));
            }
        }
        self.s[start..self.i].parse::<f64>().map(Json::Num).map_err(|_| self.err("nombre invalide"))
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("\\u incomplet"))?;
        if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(self.err("\\u invalide"));
        }
        let v = u32::from_str_radix(h, 16).map_err(|_| self.err("\\u invalide"))?;
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let from = self.i;
            while let Some(&c) = self.b.get(self.i) {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // On ne s'arrête que sur des octets ASCII : la tranche reste de l'UTF-8 valide.
            out.push_str(&self.s[from..self.i]);
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = *self.b.get(self.i).ok_or_else(|| self.err("échappement incomplet"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.s[self.i..].starts_with("\\u") {
                                    return Err(self.err("paire de substitution incomplète"));
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err("paire de substitution invalide"));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).ok_or_else(|| self.err("point de code invalide"))?);
                        }
                        _ => return Err(self.err("échappement inconnu")),
                    }
                }
                Some(_) => return Err(self.err("caractère de contrôle dans une chaîne")),
                None => return Err(self.err("chaîne non terminée")),
            }
        }
    }
    fn array(&mut self) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("« , » ou « ] » attendu")),
            }
        }
    }
    fn object(&mut self) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut pairs = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            self.eat(b':')?;
            self.ws();
            pairs.push((k, self.value()?));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(self.err("« , » ou « } » attendu")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let src = r#"{"a":1,"b":[true,false,null],"c":"é\"\\\n\u0001","d":-2.5e3,"e":{}}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.get("a"), Some(&Json::Num(1.0)));
        assert_eq!(v.get("d"), Some(&Json::Num(-2500.0)));
        assert_eq!(v.str_field("c"), Some("é\"\\\n\u{1}"));
        assert_eq!(v.to_json(), r#"{"a":1,"b":[true,false,null],"c":"é\"\\\n\u0001","d":-2500,"e":{}}"#);
        assert_eq!(parse(&v.to_json()).unwrap(), v);
    }

    #[test]
    fn unicode_escapes() {
        assert_eq!(parse(r#""é😀""#).unwrap(), Json::Str("é😀".into()));
        assert!(parse(r#""\ud83d""#).is_err());
        assert!(parse(r#""\ud83dA""#).is_err());
        assert!(parse(r#""\uzzzz""#).is_err());
        assert!(parse(r#""\u+041""#).is_err());
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "01", "1.", "-", "tru", "\"a", "[1] x", "{\"a\":1,}", "\"\t\""] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert!(parse(&deep).is_err());
        let ok = "[".repeat(60) + &"]".repeat(60);
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn numbers_render() {
        assert_eq!(Json::from(3usize).to_json(), "3");
        assert_eq!(Json::Num(0.25).to_json(), "0.25");
        assert_eq!(Json::Num(f64::NAN).to_json(), "null");
        assert_eq!(obj([("x", Json::from(Some("a"))), ("y", Json::from(None::<&str>))]).to_json(), r#"{"x":"a","y":null}"#);
    }
}
