//! L'interface web, embarquée dans l'exécutable : rien à installer à côté.

use std::sync::OnceLock;

use crate::util::{fnv1a64, hex64};

pub struct Asset {
    pub path: &'static str,
    pub mime: &'static str,
    pub body: &'static [u8],
}

macro_rules! asset {
    ($path:literal, $mime:literal, $file:literal) => {
        Asset { path: $path, mime: $mime, body: include_bytes!(concat!("../web/", $file)) }
    };
}

pub static ASSETS: &[Asset] = &[
    asset!("/", "text/html; charset=utf-8", "index.html"),
    asset!("/static/app.css", "text/css; charset=utf-8", "app.css"),
    asset!("/static/app.js", "text/javascript; charset=utf-8", "app.js"),
    asset!("/static/theme.js", "text/javascript; charset=utf-8", "theme.js"),
    asset!("/static/i18n.js", "text/javascript; charset=utf-8", "i18n.js"),
    asset!("/static/clip.js", "text/javascript; charset=utf-8", "clip.js"),
    asset!("/static/pieces.js", "text/javascript; charset=utf-8", "pieces.js"),
    asset!("/static/vendor/mount.js", "text/javascript; charset=utf-8", "vendor/ascii/mount.js"),
    asset!("/static/vendor/banner.js", "text/javascript; charset=utf-8", "vendor/ascii/banner.js"),
    asset!("/static/fonts/paper-mono.woff2", "font/woff2", "fonts/paper-mono.woff2"),
    asset!("/static/fonts/ascii-rest-mono.woff2", "font/woff2", "fonts/ascii-rest-mono.woff2"),
    asset!("/favicon.svg", "image/svg+xml", "favicon.svg"),
    asset!("/static/licenses.txt", "text/plain; charset=utf-8", "LICENSES.txt"),
];

/// L'asset et son ETag (empreinte du contenu, calculée une fois).
pub fn find(path: &str) -> Option<(&'static Asset, &'static str)> {
    static TAGS: OnceLock<Vec<String>> = OnceLock::new();
    let tags = TAGS.get_or_init(|| ASSETS.iter().map(|a| format!("\"{}\"", hex64(fnv1a64(a.body)))).collect());
    let i = ASSETS.iter().position(|a| a.path == path)?;
    Some((&ASSETS[i], tags[i].as_str()))
}
