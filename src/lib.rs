//! protocol — carnet de coach Valorant.
//!
//! Un seul exécutable : un petit serveur local qui sert l'interface (HTML/CSS/JS
//! embarqués) et une API JSON, au-dessus d'un seul fichier de carnet rangé à côté
//! du programme. Rien n'est écrit ailleurs sur le poste.

pub mod api;
pub mod assets;
pub mod carnet;
pub mod http;
pub mod i18n;
pub mod json;
pub mod mcp;
pub mod note;
pub mod platform;
pub mod render;
pub mod search;
pub mod share;
pub mod store;
pub mod util;
pub mod zip;

use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Le port de la fenêtre de protocol, quand il est libre.
pub const DEFAULT_PORT: u16 = 7373;

/// Les ports où chercher (et prendre) la place d'un carnet : 7373, puis un port propre
/// à ce carnet si 7373 est occupé par autre chose.
pub fn instance_ports(carnet: &Path) -> [u16; 2] {
    let h = util::fnv1a64(platform::display_path(carnet).to_lowercase().as_bytes());
    [DEFAULT_PORT, 7400 + (h % 500) as u16]
}

/// Le carnet que sert une fenêtre de protocol sur ce port, s'il y en a une.
pub fn instance_on(port: u16) -> Option<String> {
    let (status, body) = http::local_request(port, "GET", "/api/ping", None).ok()?;
    let v = json::parse(&body).ok().filter(|_| status == 200)?;
    (v.str_field("app") == Some("protocol")).then(|| v.str_field("carnet").unwrap_or("").to_string())
}

/// Le même fichier de carnet ? (Sous Windows, sans tenir compte de la casse.)
pub fn same_carnet(shown: &str, carnet: &Path) -> bool {
    let b = platform::display_path(carnet);
    if cfg!(windows) { shown.eq_ignore_ascii_case(&b) } else { shown == b }
}

/// Le port d'une fenêtre de protocol déjà ouverte sur ce carnet (`hint` : un port
/// imposé au lancement).
pub fn running_instance(carnet: &Path, hint: Option<u16>) -> Option<u16> {
    let ports = hint.map_or_else(|| instance_ports(carnet).to_vec(), |p| vec![p]);
    ports.into_iter().find(|&p| instance_on(p).is_some_and(|c| same_carnet(&c, carnet)))
}

pub struct Server {
    pub port: u16,
    pub app: Arc<api::App>,
}

/// Lance le serveur sur `listener` : un thread d'acceptation, plus un veilleur
/// qui arrête le programme quand l'interface est fermée (si `auto_exit`).
pub fn start(listener: TcpListener, app: api::App) -> io::Result<Server> {
    let port = listener.local_addr()?.port();
    app.port.store(port, std::sync::atomic::Ordering::Relaxed);
    let app = Arc::new(app);
    let for_handler = app.clone();
    let handler: Arc<http::Handler> = Arc::new(move |req| api::handle(&for_handler, req));
    let for_errors = app.clone();
    let localize: Arc<http::Localize> = Arc::new(move |msg| i18n::localize(for_errors.lang_now(), msg));
    thread::Builder::new().name("http".into()).spawn(move || http::serve(listener, handler, localize, api::body_limit))?;
    let watcher = app.clone();
    thread::Builder::new().name("veilleur".into()).spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(api::TICK_MS));
            if watcher.tick() {
                watcher.exit();
            }
        }
    })?;
    Ok(Server { port, app })
}
