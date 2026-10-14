//! Serveur HTTP/1.1 minimal sur `std::net` : une connexion = une requête
//! (`Connection: close`), des limites strictes, aucune dépendance. Il ne sert
//! que l'interface locale et l'API.

use std::borrow::Cow;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::json::{Json, obj};
use crate::util;

const MAX_HEAD: usize = 16 * 1024;
const MAX_HEADERS: usize = 100;
const MAX_CONNECTIONS: usize = 128;
/// Temps total accordé pour recevoir une requête (en-têtes et corps) : un client
/// trop lent ne garde pas une place indéfiniment.
const READ_DEADLINE: Duration = Duration::from_secs(20);
/// Temps total pour vider ce qu'un client envoie encore après la réponse.
const DRAIN_DEADLINE: Duration = Duration::from_secs(1);

pub struct Request {
    pub method: String,
    /// Chemin brut (encore encodé), sans la requête.
    pub path: String,
    pub query: String,
    /// Noms en minuscules.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub peer: SocketAddr,
}

pub fn is_loopback(peer: &SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v| v.is_loopback()),
    }
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn param(&self, key: &str) -> Option<String> {
        util::query_param(&self.query, key)
    }
    /// `application/json; charset=utf-8` → `application/json`
    pub fn content_type(&self) -> String {
        self.header("content-type").map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase()).unwrap_or_default()
    }
    pub fn from_loopback(&self) -> bool {
        is_loopback(&self.peer)
    }
    pub fn body_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.body).ok()
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    /// `Borrowed` pour les fichiers embarqués : servis sans copie.
    pub body: Cow<'static, [u8]>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response { status, headers: vec![("Content-Type", content_type.to_string())], body: Cow::Owned(body.into()) }
    }
    pub fn embedded(status: u16, content_type: &str, body: &'static [u8]) -> Response {
        Response { status, headers: vec![("Content-Type", content_type.to_string())], body: Cow::Borrowed(body) }
    }
    pub fn json(status: u16, value: &Json) -> Response {
        Response::new(status, "application/json; charset=utf-8", value.to_json())
    }
    pub fn error(status: u16, message: &str) -> Response {
        Response::json(status, &obj([("error", message.into())]))
    }
    pub fn empty(status: u16) -> Response {
        Response { status, headers: Vec::new(), body: Cow::Borrowed(&[]) }
    }
    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Response {
        let value: String = value.into();
        // Jamais de retour à la ligne dans une valeur d'en-tête (injection).
        let value = value.replace(['\r', '\n'], " ");
        self.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        self.headers.push((name, value));
        self
    }
    pub fn has_header(&self, name: &str) -> bool {
        self.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[derive(Debug)]
pub enum ReadError {
    Closed,
    Bad(u16, &'static str),
    Io(io::Error),
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Lecture bornée par une échéance totale, et pas seulement par appel.
fn read_until(stream: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Result<usize, ReadError> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ReadError::Bad(408, "requête trop lente"));
        }
        let _ = stream.set_read_timeout(Some(left.min(Duration::from_secs(15))));
        match stream.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(ReadError::Io(e)),
        }
    }
}

/// Taille de corps admise selon (méthode, chemin, requête venue de ce poste).
pub type Limit = fn(&str, &str, bool) -> usize;

/// Lit une requête complète, dans les limites de taille et de temps.
pub fn read_request(stream: &mut TcpStream, peer: SocketAddr, limit: Limit) -> Result<Request, ReadError> {
    let deadline = Instant::now() + READ_DEADLINE;
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 8192];
    let mut from = 0;
    let head_end = loop {
        // On ne cherche la fin des en-têtes que dans ce qui vient d'arriver (et 3 octets avant).
        if let Some(i) = find(&buf[from..], b"\r\n\r\n").map(|i| i + from) {
            if i > MAX_HEAD {
                return Err(ReadError::Bad(431, "en-têtes trop longs"));
            }
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(ReadError::Bad(431, "en-têtes trop longs"));
        }
        from = buf.len().saturating_sub(3);
        match read_until(stream, &mut chunk, deadline)? {
            0 => return Err(if buf.is_empty() { ReadError::Closed } else { ReadError::Bad(400, "requête incomplète") }),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| ReadError::Bad(400, "en-têtes illisibles"))?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target), Some(version), None) = (start.next(), start.next(), start.next(), start.next()) else {
        return Err(ReadError::Bad(400, "ligne de requête invalide"));
    };
    if !version.starts_with("HTTP/1.") || method.len() > 10 || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(ReadError::Bad(400, "ligne de requête invalide"));
    }
    if !target.starts_with('/') || target.len() > 4096 {
        return Err(ReadError::Bad(400, "cible invalide"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            return Err(ReadError::Bad(400, "en-tête invalide"));
        };
        if !is_token(k) || headers.len() >= MAX_HEADERS {
            return Err(ReadError::Bad(400, "en-tête invalide"));
        }
        headers.push((k.to_ascii_lowercase(), v.trim().to_string()));
    }
    let get = |name: &str| headers.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect::<Vec<_>>();
    if !get("transfer-encoding").is_empty() {
        return Err(ReadError::Bad(501, "transfer-encoding non pris en charge"));
    }
    if get("host").len() > 1 {
        return Err(ReadError::Bad(400, "en-tête host en double"));
    }
    let len = match get("content-length").as_slice() {
        [] => 0,
        [one] if !one.is_empty() && one.bytes().all(|b| b.is_ascii_digit()) => {
            one.parse::<usize>().map_err(|_| ReadError::Bad(400, "content-length invalide"))?
        }
        [_] => return Err(ReadError::Bad(400, "content-length invalide")),
        _ => return Err(ReadError::Bad(400, "content-length en double")),
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    // Décidé avant de lire le corps : un poste du réseau n'écrit rien, il n'envoie donc rien.
    let max = limit(method, path, is_loopback(&peer));
    if len > max {
        return Err(if max == 0 {
            ReadError::Bad(403, "lecture seule depuis le réseau : les modifications se font sur le poste du coach")
        } else {
            ReadError::Bad(413, "contenu trop volumineux")
        });
    }
    let expect_continue = get("expect").iter().any(|v| v.eq_ignore_ascii_case("100-continue"));
    let mut body = buf[head_end + 4..].to_vec();
    if body.len() < len && expect_continue {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").map_err(ReadError::Io)?;
    }
    while body.len() < len {
        match read_until(stream, &mut chunk, deadline)? {
            0 => return Err(ReadError::Bad(400, "corps incomplet")),
            n => body.extend_from_slice(&chunk[..n.min(len - body.len())]),
        }
    }
    body.truncate(len);
    Ok(Request { method: method.to_string(), path: path.to_string(), query: query.to_string(), headers, body, peer })
}

/// Une requête à un serveur de ce poste (127.0.0.1, rien d'autre), sans connexion
/// persistante : de quoi parler à une fenêtre de protocol déjà ouverte. Renvoie (code,
/// corps). Délais courts : une fenêtre absente se voit tout de suite.
pub fn local_request(port: u16, method: &str, path: &str, json_body: Option<&str>) -> io::Result<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(400))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n");
    if let Some(b) = json_body {
        head.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", b.len()));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(json_body.unwrap_or("").as_bytes())?;
    let mut raw = Vec::new();
    // Une réponse démesurée n'est pas lue en entier.
    (&mut s).take(64 << 20).read_to_end(&mut raw)?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| io::Error::other("réponse HTTP sans en-têtes"))?;
    let status = String::from_utf8_lossy(&raw[..split])
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| io::Error::other("réponse HTTP illisible"))?;
    // Ce serveur répond toujours avec `Content-Length` et ferme ensuite : le reste est le corps.
    Ok((status, String::from_utf8_lossy(&raw[split + 4..]).into_owned()))
}

pub fn write_response(stream: &mut TcpStream, res: &Response, head_only: bool) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", res.status, reason(res.status));
    for (k, v) in &res.headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    let no_body = head_only || res.status == 304 || res.status == 204;
    if res.status != 304 && res.status != 204 {
        head.push_str(&format!("Content-Length: {}\r\n", res.body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    if no_body || res.body.len() > 64 * 1024 {
        stream.write_all(head.as_bytes())?;
        if !no_body {
            stream.write_all(&res.body)?;
        }
    } else {
        // Petite réponse : un seul envoi.
        let mut all = head.into_bytes();
        all.extend_from_slice(&res.body);
        stream.write_all(&all)?;
    }
    stream.flush()
}

pub type Handler = dyn Fn(Request) -> Response + Send + Sync;

/// Traduit un message d'erreur (écrit en français) avant de l'envoyer : pour les
/// requêtes refusées avant d'atteindre l'application (trop grosses, malformées…).
pub type Localize = dyn Fn(&str) -> String + Send + Sync;

fn handle(mut stream: TcpStream, handler: &Handler, localize: &Localize, limit: Limit) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_nodelay(true);
    let Ok(peer) = stream.peer_addr() else { return };
    match read_request(&mut stream, peer, limit) {
        Ok(req) => {
            let head_only = req.method == "HEAD";
            let res = handler(req);
            let _ = write_response(&mut stream, &res, head_only);
        }
        Err(ReadError::Bad(code, msg)) => {
            let _ = write_response(&mut stream, &Response::error(code, &localize(msg)), false);
        }
        Err(_) => return,
    }
    // Fermeture propre : on vide ce que le client aurait encore envoyé, sinon
    // Windows coupe la connexion (RST) et le client peut perdre la réponse.
    let _ = stream.shutdown(Shutdown::Write);
    let until = Instant::now() + DRAIN_DEADLINE;
    let mut sink = [0u8; 8192];
    while let Ok(1..) = read_until(&mut stream, &mut sink, until) {}
}

/// Une place de connexion, rendue quoi qu'il arrive (fin normale, panique, thread
/// jamais lancé) : le compteur ne fuit pas.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Boucle d'acceptation : un thread par connexion, nombre de connexions borné.
pub fn serve(listener: TcpListener, handler: Arc<Handler>, localize: Arc<Localize>, limit: Limit) {
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else {
            thread::sleep(Duration::from_millis(20));
            continue;
        };
        if active.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            let _ = write_response(&mut stream, &Response::error(503, &localize("serveur occupé")), false);
            continue;
        }
        active.fetch_add(1, Ordering::AcqRel);
        let slot = Slot(active.clone());
        let handler = handler.clone();
        let localize = localize.clone();
        let spawned = thread::Builder::new().stack_size(1 << 20).spawn(move || {
            let _slot = slot;
            handle(stream, &*handler, &*localize, limit);
        });
        if spawned.is_err() {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn roundtrip(raw: &[u8], limit: Limit) -> Result<Request, ReadError> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let raw = raw.to_vec();
        let client = thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(&raw).unwrap();
            let _ = s.shutdown(Shutdown::Write);
            let mut out = Vec::new();
            let _ = s.read_to_end(&mut out);
        });
        let (mut stream, peer) = listener.accept().unwrap();
        let r = read_request(&mut stream, peer, limit);
        drop(stream);
        client.join().unwrap();
        r
    }

    fn big(_: &str, _: &str, _: bool) -> usize {
        1 << 20
    }
    fn tiny(_: &str, _: &str, _: bool) -> usize {
        4
    }

    #[test]
    fn parses_a_request() {
        let r = roundtrip(b"PUT /api/notes/ascent/retake-b?x=1 HTTP/1.1\r\nHost: 127.0.0.1:7373\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}", big).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.query.as_str()), ("PUT", "/api/notes/ascent/retake-b", "x=1"));
        assert_eq!(r.header("host"), Some("127.0.0.1:7373"));
        assert_eq!(r.content_type(), "application/json");
        assert_eq!(r.body, b"{\"a\":1}");
        assert!(r.from_loopback());
    }

    #[test]
    fn rejects_bad_requests() {
        let cases: [(&[u8], u16); 9] = [
            (b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n", 501),
            (b"GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab", 400),
            (b"GET / HTTP/1.1\r\nContent-Length: +2\r\n\r\nab", 400),
            (b"GET / HTTP/1.1\r\nHost: a:1\r\nHost: b:2\r\n\r\n", 400),
            (b"GET http://evil/ HTTP/1.1\r\n\r\n", 400),
            (b"get / HTTP/1.1\r\n\r\n", 400),
            (b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n", 400),
            (b"POST /x HTTP/1.1\r\nContent-Length: 10\r\n\r\n0123456789", 413),
            (b"POST /x HTTP/1.1\r\nContent-Length: 4\r\n\r\nab", 400),
        ];
        for (raw, code) in cases {
            match roundtrip(raw, tiny) {
                Err(ReadError::Bad(c, _)) => assert_eq!(c, code, "{}", String::from_utf8_lossy(raw)),
                other => panic!("{} → {:?}", String::from_utf8_lossy(raw), other.map(|r| r.path)),
            }
        }
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD + 10));
        assert!(matches!(roundtrip(long.as_bytes(), big), Err(ReadError::Bad(431, _))));
    }

    #[test]
    fn a_panicking_handler_gives_its_slot_back() {
        let active = Arc::new(AtomicUsize::new(1));
        let slot = Slot(active.clone());
        let r = thread::spawn(move || {
            let _slot = slot;
            panic!("exprès");
        })
        .join();
        assert!(r.is_err());
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn header_values_cannot_inject() {
        let r = Response::empty(200).header("X-Test", "a\r\nSet-Cookie: x=1");
        assert_eq!(r.headers[0].1, "a  Set-Cookie: x=1");
    }
}
