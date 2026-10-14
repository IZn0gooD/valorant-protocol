//! `protocol` : double-cliquez, le carnet s'ouvre. `protocol --help` pour le reste.

use std::fs;
use std::io::{self, Write};
use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::exit;

use protocol::i18n::{self, Lang};
use protocol::{api, mcp, platform, search, store::Store, tr, util};

/// Le carnet, à côté du programme.
const CARNET: &str = "protocol.carnet";
/// L'ancien format : un dossier de fichiers Markdown, repris au premier lancement.
const LEGACY_DIR: &str = "notes";

const HELP: &str = "\
protocol — carnet de coach Valorant : protocoles, setups, exécutions, lineups… map par map.

Tout le carnet tient dans un seul fichier, protocol.carnet, à côté du programme.
Rien n'est écrit ailleurs sur le poste.

UTILISATION
  protocol [options]             ouvre le carnet (un double-clic fait pareil)
  protocol serve [options]       serveur seul : pas de fenêtre, pas d'arrêt automatique
  protocol search <mots…>        cherche dans les notes (filtres : map:ascent type:retake side:def #tag)
  protocol export [--map <map> [--pinned] [--format md|discord|texte]] [--format zip] [-o <fichier>]
                                 sans --map : tout le carnet en JSON, ou en fichiers Markdown
                                 (--format zip) ; avec --map : une map en Markdown, prête pour
                                 Discord ou en texte brut ; --pinned : la fiche match
  protocol mcp [--write]         serveur MCP (stdio) pour un assistant IA : outils, ressources,
                                 consignes prêtes ; --write autorise l'écriture, même fenêtre
                                 ouverte, sans rien d'irréversible (corbeille, version d'avant)
  protocol --version | --help

OPTIONS
  --carnet <fichier>   le fichier du carnet (défaut : protocol.carnet à côté du programme,
                       ou PROTOCOL_CARNET) ; un ancien dossier « notes » voisin est repris
  --port <n>           port local (défaut : 7373)
  --lan                partage le carnet en lecture seule sur le réseau local (bootcamp, LAN) ;
                       le lien d'invitation à donner aux joueurs s'affiche au lancement
  --read-only          aucune modification possible, même depuis ce poste
  --cors <origine>     autorise une autre appli web locale à appeler l'API (répétable),
                       ex. --cors http://localhost:5173
  --browser <mode>     app (fenêtre dédiée au profil temporaire, par défaut) | default
                       (navigateur habituel, qui garde son historique) | none

LANGUE
  Français ou anglais : le réglage du carnet, sinon PROTOCOL_LANG=fr|en, sinon la langue du système.
";

const HELP_EN: &str = "\
protocol — a Valorant coach's notebook: protocols, setups, executes, lineups… map by map.

The whole notebook fits in a single file, protocol.carnet, next to the program.
Nothing is written anywhere else on the computer.

USAGE
  protocol [options]             opens the notebook (a double-click does the same)
  protocol serve [options]       server only: no window, no automatic shutdown
  protocol search <words…>       searches the notes (filters: map:ascent type:retake side:def #tag)
  protocol export [--map <map> [--pinned] [--format md|discord|text]] [--format zip] [-o <file>]
                                 without --map: the whole notebook as JSON, or as Markdown files
                                 (--format zip); with --map: one map as Markdown, ready for
                                 Discord or as plain text; --pinned: the match sheet
  protocol mcp [--write]         MCP server (stdio) for an AI assistant: tools, resources,
                                 ready-made prompts; --write allows writing, even with the window
                                 open, with nothing irreversible (trash, previous version kept)
  protocol --version | --help

OPTIONS
  --carnet <file>      the notebook file (default: protocol.carnet next to the program,
                       or PROTOCOL_CARNET); an old \"notes\" folder next to it is imported
  --port <n>           local port (default: 7373)
  --lan                shares the notebook read-only on the local network (bootcamp, LAN);
                       the invitation link to give your players is shown at startup
  --read-only          no changes possible, even from this computer
  --cors <origin>      lets another local web app call the API (repeatable),
                       e.g. --cors http://localhost:5173
  --browser <mode>     app (dedicated window with a temporary profile, default) | default
                       (your usual browser, which keeps its history) | none

LANGUAGE
  French or English: the notebook's setting, otherwise PROTOCOL_LANG=fr|en, otherwise the system language.
";

#[derive(Debug, PartialEq)]
enum Cmd {
    Run,
    Serve,
    Search { query: String, limit: usize },
    Export { map: Option<String>, out: Option<PathBuf>, format: Format, pinned: bool },
    Mcp { write: bool },
    Help,
    Version,
}

/// Le format d'un export.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    Markdown,
    Discord,
    Text,
    /// Tout le carnet en fichiers Markdown (archive ZIP).
    Zip,
    /// Tout le carnet en JSON (par défaut, sans --map).
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Browser {
    App,
    Default,
    Off,
}

#[derive(Debug)]
struct Args {
    cmd: Cmd,
    carnet: Option<PathBuf>,
    port: Option<u16>,
    lan: bool,
    read_only: bool,
    cors: Vec<String>,
    browser: Browser,
}

fn take(args: &[String], i: &mut usize, name: &str, inline: Option<String>) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v);
    }
    let v = args.get(*i).cloned().ok_or_else(|| format!("{name} attend une valeur"))?;
    *i += 1;
    Ok(v)
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args { cmd: Cmd::Run, carnet: None, port: None, lan: false, read_only: false, cors: Vec::new(), browser: Browser::App };
    let (mut command, mut words) = (None::<String>, Vec::<String>::new());
    let (mut map, mut out, mut write, mut limit, mut help, mut version) = (None, None, false, 20usize, false, false);
    let (mut format, mut pinned) = (None::<Format>, false);
    let mut i = 0;
    while i < args.len() {
        let raw = args[i].clone();
        i += 1;
        let (flag, inline) = match raw.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (raw.clone(), None),
        };
        match flag.as_str() {
            "-h" | "--help" => help = true,
            "-V" | "--version" => version = true,
            // `--data` : l'ancien nom de l'option.
            "--carnet" | "--data" => a.carnet = Some(PathBuf::from(take(args, &mut i, &flag, inline)?)),
            "--port" => {
                let v = take(args, &mut i, "--port", inline)?;
                a.port = Some(v.parse().map_err(|_| format!("port invalide : {v}"))?);
            }
            "--lan" => a.lan = true,
            "--read-only" => a.read_only = true,
            "--cors" => {
                let v = take(args, &mut i, "--cors", inline)?.trim_end_matches('/').to_ascii_lowercase();
                if !(v.starts_with("http://") || v.starts_with("https://")) {
                    return Err(format!("origine invalide : {v} (ex. http://localhost:5173)"));
                }
                a.cors.push(v);
            }
            "--browser" => {
                a.browser = match take(args, &mut i, "--browser", inline)?.as_str() {
                    "app" => Browser::App,
                    "default" => Browser::Default,
                    "none" | "off" => Browser::Off,
                    other => return Err(format!("--browser : app, default ou none, pas « {other} »")),
                }
            }
            "--map" => map = Some(take(args, &mut i, "--map", inline)?),
            "-o" | "--out" => out = Some(PathBuf::from(take(args, &mut i, "--out", inline)?)),
            "--pinned" => pinned = true,
            "--format" => {
                format = Some(match take(args, &mut i, "--format", inline)?.as_str() {
                    "md" | "markdown" => Format::Markdown,
                    "discord" => Format::Discord,
                    "texte" | "text" | "txt" => Format::Text,
                    "zip" => Format::Zip,
                    "json" => Format::Json,
                    other => return Err(format!("--format : md, discord, texte, zip ou json, pas « {other} »")),
                })
            }
            "--json" => format = Some(Format::Json),
            "--write" => write = true,
            "--limit" => {
                let v = take(args, &mut i, "--limit", inline)?;
                limit = v.parse().map_err(|_| format!("--limit invalide : {v}"))?;
            }
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("option inconnue : {f}")),
            _ if command.is_none() => command = Some(raw),
            _ => words.push(raw),
        }
    }
    a.cmd = match command.as_deref() {
        _ if help => Cmd::Help,
        _ if version => Cmd::Version,
        None => Cmd::Run,
        Some("serve") => Cmd::Serve,
        Some("search") if words.is_empty() => return Err("search : que faut-il chercher ?".into()),
        Some("search") => Cmd::Search { query: words.join(" "), limit: limit.clamp(1, 200) },
        Some("export") => {
            let format = match (&map, format) {
                (None, None | Some(Format::Json)) if !pinned => Format::Json,
                (None, Some(Format::Zip)) if !pinned => Format::Zip,
                (None, _) => return Err("export : md, discord, texte et --pinned s'appliquent à une map (--map <map>)".into()),
                (Some(_), None | Some(Format::Markdown)) => Format::Markdown,
                (Some(_), Some(f @ (Format::Discord | Format::Text))) => f,
                (Some(_), Some(_)) => return Err("export : zip et json concernent tout le carnet (sans --map)".into()),
            };
            Cmd::Export { map, out, format, pinned }
        }
        Some("mcp") => Cmd::Mcp { write },
        Some("help") => Cmd::Help,
        Some("version") => Cmd::Version,
        Some(other) => return Err(format!("commande inconnue : {other}")),
    };
    Ok(a)
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".protocol-probe-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

/// `--carnet`, sinon `PROTOCOL_CARNET`, sinon `protocol.carnet` à côté du
/// programme : le carnet voyage avec la clé USB. Un dossier désigne le
/// `protocol.carnet` qu'il contient.
fn carnet_path(a: &Args) -> PathBuf {
    let given = a.carnet.clone().or_else(|| {
        ["PROTOCOL_CARNET", "PROTOCOL_DATA"].iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
    });
    let p = match given {
        Some(p) if p.is_dir() => p.join(CARNET),
        Some(p) => p,
        None => exe_dir().join(CARNET),
    };
    std::path::absolute(&p).unwrap_or(p)
}

fn dir_of(carnet: &Path) -> PathBuf {
    carnet.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// Le profil de navigateur de la fenêtre : à côté du carnet (rien sur le poste),
/// sauf si le dossier est en lecture seule (CD, partage protégé).
fn profile_dir(carnet: &Path) -> PathBuf {
    let stem = carnet.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "protocol".into());
    let dir = dir_of(carnet);
    if writable(&dir) {
        dir.join(format!(".{stem}-session"))
    } else {
        let tag = util::hex64(util::fnv1a64(carnet.to_string_lossy().to_lowercase().as_bytes()));
        std::env::temp_dir().join(format!("protocol-session-{}", &tag[..8]))
    }
}

/// Les ports où chercher (et prendre) la place de ce carnet : celui imposé, sinon
/// 7373 puis un port propre à ce carnet.
fn candidate_ports(a: &Args, carnet: &Path) -> Vec<u16> {
    a.port.map_or_else(|| protocol::instance_ports(carnet).to_vec(), |p| vec![p])
}

/// L'adresse de ce poste sur le réseau local (aucun paquet n'est envoyé).
fn lan_address() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// Les deux-points d'un message : « carnet : x » en français, « notebook: x » en anglais.
fn colon(lang: Lang) -> &'static str {
    tr!(lang, " : ", ": ")
}

/// Un message d'erreur de la ligne de commande (écrit en français), dans la langue voulue.
fn die(lang: Lang, msg: &str) -> ! {
    eprintln!("protocol{}{}", colon(lang), i18n::localize(lang, msg));
    exit(1)
}

fn fail(gui: bool, msg: &str) -> ! {
    platform::alert("protocol", msg, gui);
    exit(1)
}

/// Ouvre le carnet pour l'application ; un carnet absent est créé (à partir de
/// l'ancien dossier `notes` voisin s'il existe).
/// `lang` : la langue des messages (le carnet n'est pas encore ouvert) et d'un carnet créé.
fn open_carnet(carnet: &Path, read_only: bool, gui: bool, lang: Lang) -> Store {
    let dir = dir_of(carnet);
    let exists = carnet.exists();
    let shown = platform::display_path(carnet);
    if !exists && !read_only && !writable(&dir) {
        let dir = platform::display_path(&dir);
        fail(gui, &match lang {
            Lang::Fr => format!(
                "Impossible de créer le carnet dans {dir} : ce dossier n'est pas modifiable.\n\
                 Copiez protocol.exe sur une clé USB, le bureau ou un dossier personnel, puis relancez-le."
            ),
            Lang::En => format!(
                "Cannot create the notebook in {dir}: this folder is read-only.\n\
                 Copy protocol.exe to a USB stick, the desktop or a personal folder, then run it again."
            ),
        });
    }
    let legacy = (!exists).then(|| dir.join(LEGACY_DIR)).filter(|d| d.is_dir());
    let cause = |e: io::Error| i18n::localize(lang, &e.to_string());
    match Store::open_in(carnet, read_only, legacy.as_deref(), lang) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::ResourceBusy => fail(
            gui,
            tr!(
                lang,
                "Ce carnet est déjà ouvert par un autre programme protocol.\nFermez-le, ou utilisez la fenêtre déjà ouverte.",
                "This notebook is already open in another protocol program.\nClose it, or use the window that is already open."
            ),
        ),
        // Dossier en lecture seule (CD, partage protégé) : on lit sans rien écrire.
        Err(e) if !read_only && exists && e.kind() == io::ErrorKind::PermissionDenied => Store::open(carnet, true)
            .unwrap_or_else(|e| fail(gui, &format!("{} ({shown}){}{}", tr!(lang, "carnet illisible", "unreadable notebook"), colon(lang), cause(e)))),
        Err(e) => fail(gui, &format!("{} ({shown}){}{}", tr!(lang, "carnet inaccessible", "notebook not accessible"), colon(lang), cause(e))),
    }
}

fn run(a: &Args, serve_only: bool) -> ! {
    // Double-clic : la console ne sert à rien, on la ferme tout de suite ; les
    // erreurs passeront par une boîte de dialogue.
    let gui = !serve_only && platform::launched_from_explorer();
    if gui {
        platform::hide_console();
    }
    let carnet = carnet_path(a);
    let ports = candidate_ports(a, &carnet);
    let profile = (a.browser == Browser::App).then(|| profile_dir(&carnet));
    // Déjà ouvert sur ce carnet : on montre la fenêtre, sans second serveur.
    if !serve_only {
        for &port in &ports {
            if let Some(other) = protocol::instance_on(port)
                && protocol::same_carnet(&other, &carnet)
            {
                if a.browser != Browser::Off {
                    let _ = platform::show_again(&format!("http://127.0.0.1:{port}/"), profile.as_deref());
                }
                exit(0);
            }
        }
    }
    // Avant l'ouverture du carnet, la langue du système (ou de PROTOCOL_LANG).
    let lang = i18n::resolve(None);
    let host = if a.lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
    let listener = ports.iter().find_map(|&p| TcpListener::bind((host, p)).ok()).unwrap_or_else(|| {
        // Ports pris par d'autres programmes : on en prend un libre (sauf port imposé).
        if a.port.is_some() {
            fail(gui, &match lang {
                Lang::Fr => format!("le port {} est indisponible", ports[0]),
                Lang::En => format!("port {} is not available", ports[0]),
            });
        }
        TcpListener::bind((host, 0))
            .unwrap_or_else(|e| fail(gui, &format!("{}{}{e}", tr!(lang, "aucun port disponible", "no port available"), colon(lang))))
    });
    let store = open_carnet(&carnet, a.read_only, gui, lang);
    // Le carnet ouvert : sa langue à lui.
    let lang = store.lang();
    let notice = store.notice().map(|n| i18n::localize(lang, n));
    let shown = platform::display_path(store.path());
    // Partage réseau : un lien d'invitation, impossible à deviner, pour les autres postes.
    let lan_token = a.lan.then(util::random_token);
    let cfg = api::Config { lan: a.lan, lan_token: lan_token.clone(), cors: a.cors.clone(), auto_exit: !serve_only };
    let server = protocol::start(listener, api::App::new(store, cfg))
        .unwrap_or_else(|e| fail(gui, &format!("{}{}{e}", tr!(lang, "démarrage impossible", "could not start"), colon(lang))));
    let url = format!("http://127.0.0.1:{}/", server.port);
    let port = server.port;
    let mut out = io::stdout();
    let _ = writeln!(out, "protocol {} — {}", env!("CARGO_PKG_VERSION"), tr!(lang, "carnet de coach Valorant", "Valorant coach's notebook"));
    let _ = writeln!(out, "  interface{}{url}", colon(lang));
    if let Some(token) = &lan_token {
        let line = match (lan_address(), lang) {
            (Some(ip), Lang::Fr) => format!("lien à donner aux joueurs (lecture seule) : http://{ip}:{port}/?t={token}"),
            (Some(ip), Lang::En) => format!("link to give your players (read-only): http://{ip}:{port}/?t={token}"),
            (None, Lang::Fr) => format!("réseau local : adresse introuvable (lien : http://<adresse-ip>:{port}/?t={token})"),
            (None, Lang::En) => format!("local network: address not found (link: http://<ip-address>:{port}/?t={token})"),
        };
        let _ = writeln!(out, "  {line}");
    }
    let _ = writeln!(out, "  {}{}{shown}", tr!(lang, "carnet", "notebook"), colon(lang));
    if let Some(n) = notice {
        let _ = writeln!(out, "  {n}");
    }
    let bye = if serve_only {
        tr!(lang, "Ctrl+C pour arrêter.", "Press Ctrl+C to stop.")
    } else {
        tr!(lang, "Le carnet s'arrête seul quand sa fenêtre est fermée.", "The notebook stops on its own when its window is closed.")
    };
    let _ = writeln!(out, "  {bye}");
    if !serve_only && a.browser != Browser::Off {
        // Un profil laissé par un arrêt brutal : la fenêtre repart d'un profil vide.
        if let Some(p) = &profile {
            let _ = fs::remove_dir_all(p);
        }
        let app = server.app.clone();
        if let Err(e) = platform::open_window(&url, profile.as_deref(), move || app.request_quit()) {
            let line = match lang {
                Lang::Fr => format!("navigateur introuvable ({e}) : ouvrez {url}"),
                Lang::En => format!("browser not found ({e}): open {url}"),
            };
            let _ = writeln!(io::stderr(), "  {line}");
        }
    }
    loop {
        std::thread::park();
    }
}

/// Le carnet en lecture seule, pour la ligne de commande : la fenêtre de protocol
/// peut rester ouverte pendant ce temps.
fn open_existing(a: &Args, lang: Lang) -> Store {
    let carnet = carnet_path(a);
    let shown = platform::display_path(&carnet);
    if !carnet.exists() {
        match lang {
            Lang::Fr => eprintln!(
                "protocol : aucun carnet ici ({shown}). Lancez protocol une fois : il le crée (et reprend l'ancien dossier « notes » s'il existe)."
            ),
            Lang::En => eprintln!(
                "protocol: no notebook here ({shown}). Run protocol once: it creates one (and imports the old \"notes\" folder if there is one)."
            ),
        }
        exit(1);
    }
    Store::open(&carnet, true).unwrap_or_else(|e| {
        let e = i18n::localize(lang, &e.to_string());
        match lang {
            Lang::Fr => eprintln!("protocol : carnet illisible ({shown}) : {e}"),
            Lang::En => eprintln!("protocol: unreadable notebook ({shown}): {e}"),
        }
        exit(1)
    })
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // La langue de la ligne de commande : PROTOCOL_LANG, sinon celle du système ; une
    // commande qui lit le carnet passe ensuite à la langue de celui-ci.
    let lang = i18n::resolve(None);
    let a = parse_args(&raw).unwrap_or_else(|e| {
        let e = i18n::localize(lang, &e);
        match lang {
            Lang::Fr => eprintln!("protocol : {e}\n(protocol --help pour l'aide)"),
            Lang::En => eprintln!("protocol: {e}\n(protocol --help for help)"),
        }
        exit(2)
    });
    match &a.cmd {
        Cmd::Help => print!("{}", tr!(lang, HELP, HELP_EN)),
        Cmd::Version => println!("protocol {}", env!("CARGO_PKG_VERSION")),
        Cmd::Run => run(&a, false),
        Cmd::Serve => run(&a, true),
        Cmd::Search { query, limit } => {
            let store = open_existing(&a, lang);
            let lang = store.lang();
            let q = search::parse(query);
            let (total, hits) = search::search(store.notes(), store.maps(), &q, *limit);
            let mut out = io::stdout().lock();
            for h in &hits {
                let snippet = h
                    .snippet
                    .replace("<mark>", "")
                    .replace("</mark>", "")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&#39;", "'")
                    .replace("&amp;", "&");
                let _ = writeln!(out, "{}/{}  {}  [{}]\n    {snippet}", h.note.map, h.note.id, h.note.title, h.note.kind());
            }
            let _ = match (total, lang) {
                (_, Lang::Fr) => writeln!(out, "{total} résultat(s)"),
                (1, Lang::En) => writeln!(out, "1 result"),
                (_, Lang::En) => writeln!(out, "{total} results"),
            };
        }
        Cmd::Export { map, out, format, pinned } => {
            let mut store = open_existing(&a, lang);
            // L'export parle la langue du carnet (repères, titres de sections).
            let lang = store.lang();
            let bytes: Vec<u8> = match (map, format) {
                (Some(m), f) => {
                    let text = match f {
                        Format::Discord => api::share_map(&store, m, *pinned, lang).map(|s| s.discord),
                        Format::Text => api::share_map(&store, m, *pinned, lang).map(|s| s.text),
                        _ => api::export_map_markdown(&store, m, *pinned, lang),
                    };
                    text.unwrap_or_else(|| die(lang, &format!("map introuvable : {m}"))).into_bytes()
                }
                (None, Format::Zip) => {
                    let top = out.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "protocol".into());
                    store.export_zip(&top).unwrap_or_else(|e| die(lang, &format!("export impossible : {e}")))
                }
                (None, _) => api::export_value(&store).to_json().into_bytes(),
            };
            let res = match out {
                Some(p) => fs::write(p, bytes),
                None => io::stdout().lock().write_all(&bytes),
            };
            if let Err(e) = res {
                die(lang, &format!("export impossible : {e}"));
            }
        }
        Cmd::Mcp { write } => {
            let carnet = carnet_path(&a);
            // Un paquet MCPB passe l'interrupteur « autoriser l'écriture » par l'environnement.
            let write = *write || std::env::var("PROTOCOL_MCP_WRITE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("true"));
            if let Err(e) = mcp::run(&carnet, write, a.port) {
                let hint = if e.kind() == io::ErrorKind::NotFound {
                    tr!(
                        lang,
                        " (ouvrez protocol une fois pour créer le carnet, ou autorisez l'écriture)",
                        " (open protocol once to create the notebook, or allow writing)"
                    )
                } else {
                    ""
                };
                let (c, e) = (colon(lang), i18n::localize(lang, &e.to_string()));
                eprintln!("protocol mcp{c}{e}{c}{}{hint}", platform::display_path(&carnet));
                exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Args, String> {
        parse_args(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn cli() {
        assert_eq!(parse("").unwrap().cmd, Cmd::Run);
        let a = parse("serve --port 8000 --lan --carnet=C:/notes/equipe.carnet").unwrap();
        assert_eq!((a.cmd, a.port, a.lan, a.carnet), (Cmd::Serve, Some(8000), true, Some(PathBuf::from("C:/notes/equipe.carnet"))));
        assert_eq!(parse("--data D:/x.carnet").unwrap().carnet, Some(PathBuf::from("D:/x.carnet")), "l'ancien nom de l'option marche encore");
        assert_eq!(parse("search retake b --limit 5").unwrap().cmd, Cmd::Search { query: "retake b".into(), limit: 5 });
        assert_eq!(
            parse("export --map ascent -o a.md").unwrap().cmd,
            Cmd::Export { map: Some("ascent".into()), out: Some("a.md".into()), format: Format::Markdown, pinned: false }
        );
        assert_eq!(
            parse("export --map=bind --pinned --format discord").unwrap().cmd,
            Cmd::Export { map: Some("bind".into()), out: None, format: Format::Discord, pinned: true }
        );
        assert_eq!(parse("export --format zip -o sauvegarde.zip").unwrap().cmd, Cmd::Export {
            map: None,
            out: Some("sauvegarde.zip".into()),
            format: Format::Zip,
            pinned: false
        });
        assert_eq!(parse("export").unwrap().cmd, Cmd::Export { map: None, out: None, format: Format::Json, pinned: false });
        assert!(parse("export --format texte").is_err());
        assert!(parse("export --map bind --format zip").is_err());
        assert!(parse("export --map bind --format pdf").is_err());
        assert_eq!(parse("mcp --write").unwrap().cmd, Cmd::Mcp { write: true });
        assert_eq!(parse("--cors http://localhost:5173/").unwrap().cors, vec!["http://localhost:5173"]);
        assert_eq!(parse("serve --help").unwrap().cmd, Cmd::Help);
        assert!(parse("--port abc").is_err());
        assert!(parse("--bogus").is_err());
        assert!(parse("search").is_err());
        assert!(parse("--cors localhost").is_err());
        assert!(parse("--browser chrome").is_err());
    }

    #[test]
    fn the_carnet_sits_next_to_the_program() {
        let a = parse("").unwrap();
        let p = carnet_path(&a);
        assert_eq!(p.file_name().and_then(|n| n.to_str()), Some(CARNET));
        let d = std::env::temp_dir();
        let a = parse(&format!("--carnet {}", d.display())).unwrap();
        assert_eq!(carnet_path(&a), std::path::absolute(d.join(CARNET)).unwrap(), "un dossier désigne le carnet qu'il contient");
        let ports = candidate_ports(&a, &carnet_path(&a));
        assert_eq!(ports[0], protocol::DEFAULT_PORT);
        assert!((7400..7900).contains(&ports[1]));
    }
}
