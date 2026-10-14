//! Le carnet sur le disque : **un seul fichier**, rangé à côté du programme.
//!
//! C'est un journal. Chaque modification ajoute un enregistrement à la fin du
//! fichier ; rien n'est jamais réécrit en place. Un arrêt brutal (coupure de
//! courant, clé USB arrachée) laisse au pire un dernier enregistrement incomplet,
//! que la lecture reconnaît (longueur, CRC-32) et écarte. De temps en temps,
//! l'état courant est recopié dans un fichier neuf qui remplace l'ancien : le
//! journal ne grossit pas sans fin.
//!
//! ```text
//! en-tête          32 octets : « protocol carnet\n », version du format, identifiant
//! enregistrement   marque (4) · longueur (4) · CRC-32 (4) · opérations…
//! opération        un octet de type, puis ses champs : u32 de longueur + octets, ou i64
//! ```
//!
//! Un enregistrement est atomique : toutes ses opérations s'appliquent, ou
//! aucune. Ce module ne connaît que les octets ; le sens (notes, maps, images)
//! est l'affaire de `store`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::util::crc32;

pub const MAGIC: &[u8; 16] = b"protocol carnet\n";
pub const FORMAT: u32 = 1;
pub const HEADER_LEN: u64 = 32;
const MARK: [u8; 4] = [0x00, b'p', b'r', 0x1E];
const RECORD_HEAD: u64 = 12;
/// Plus gros enregistrement admis : une image (16 Mo au plus) et de la marge.
pub const MAX_RECORD: u64 = 64 << 20;

// ── Opérations ────────────────────────────────────────────────────────────────

/// Ce qu'un enregistrement contient, à écrire.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Le texte complet d'une note (Markdown avec son en-tête).
    Note { map: String, id: String, ts: i64, text: String },
    NoteDel { map: String, id: String },
    /// Une map : son en-tête (titre, pool, rang) et sa présentation, en Markdown.
    Map { id: String, text: String },
    MapDel { id: String },
    /// Une image, sous son chemin dans le carnet (`assets/3f2a….png`).
    Asset { name: String, data: Vec<u8> },
    AssetDel { name: String },
    /// Un élément de la corbeille : une note (`id` non vide) ou la présentation d'une map.
    Trash { key: String, map: String, id: String, ts: i64, text: String },
    TrashDel { key: String },
    Setting { key: String, value: String },
}

/// Une opération lue dans un enregistrement, sans copie. Une image n'est pas
/// chargée : on garde sa position dans l'enregistrement (`at`, relative au début
/// des opérations) et sa longueur.
#[derive(Debug, PartialEq)]
pub enum Entry<'a> {
    Note { map: &'a str, id: &'a str, ts: i64, text: &'a str },
    NoteDel { map: &'a str, id: &'a str },
    Map { id: &'a str, text: &'a str },
    MapDel { id: &'a str },
    Asset { name: &'a str, at: usize, len: usize },
    AssetDel { name: &'a str },
    Trash { key: &'a str, map: &'a str, id: &'a str, ts: i64, text: &'a str },
    TrashDel { key: &'a str },
    Setting { key: &'a str, value: &'a str },
}

fn put(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u32).to_le_bytes());
    out.extend_from_slice(field);
}

/// Les opérations d'un enregistrement, mises bout à bout.
pub fn encode(ops: &[Op]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Note { map, id, ts, text } => {
                out.push(b'N');
                put(&mut out, map.as_bytes());
                put(&mut out, id.as_bytes());
                out.extend_from_slice(&ts.to_le_bytes());
                put(&mut out, text.as_bytes());
            }
            Op::NoteDel { map, id } => {
                out.push(b'n');
                put(&mut out, map.as_bytes());
                put(&mut out, id.as_bytes());
            }
            Op::Map { id, text } => {
                out.push(b'M');
                put(&mut out, id.as_bytes());
                put(&mut out, text.as_bytes());
            }
            Op::MapDel { id } => {
                out.push(b'm');
                put(&mut out, id.as_bytes());
            }
            Op::Asset { name, data } => {
                out.push(b'A');
                put(&mut out, name.as_bytes());
                put(&mut out, data);
            }
            Op::AssetDel { name } => {
                out.push(b'a');
                put(&mut out, name.as_bytes());
            }
            Op::Trash { key, map, id, ts, text } => {
                out.push(b'T');
                put(&mut out, key.as_bytes());
                put(&mut out, map.as_bytes());
                put(&mut out, id.as_bytes());
                out.extend_from_slice(&ts.to_le_bytes());
                put(&mut out, text.as_bytes());
            }
            Op::TrashDel { key } => {
                out.push(b't');
                put(&mut out, key.as_bytes());
            }
            Op::Setting { key, value } => {
                out.push(b'S');
                put(&mut out, key.as_bytes());
                put(&mut out, value.as_bytes());
            }
        }
    }
    out
}

/// Où commencent les octets d'une image dans un enregistrement qui ne contient
/// qu'elle (`encode(&[Op::Asset { name, .. }])`).
pub fn asset_data_offset(name: &str) -> u64 {
    1 + 4 + name.len() as u64 + 4
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self) -> Option<(usize, &'a [u8])> {
        let n = u32::from_le_bytes(self.buf.get(self.pos..self.pos + 4)?.try_into().ok()?) as usize;
        let start = self.pos + 4;
        let field = self.buf.get(start..start.checked_add(n)?)?;
        self.pos = start + n;
        Some((start, field))
    }
    fn str(&mut self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes()?.1).ok()
    }
    fn i64(&mut self) -> Option<i64> {
        let v = i64::from_le_bytes(self.buf.get(self.pos..self.pos + 8)?.try_into().ok()?);
        self.pos += 8;
        Some(v)
    }
}

/// Lit les opérations d'un enregistrement. Une erreur ici, sur un enregistrement
/// dont le CRC est bon, signale un format plus récent que ce programme.
pub fn decode(payload: &[u8]) -> io::Result<Vec<Entry<'_>>> {
    let mut c = Cursor { buf: payload, pos: 0 };
    let mut out = Vec::new();
    let unknown = || io::Error::new(io::ErrorKind::InvalidData, "ce carnet a été écrit par une version plus récente de protocol");
    while c.pos < payload.len() {
        let tag = payload[c.pos];
        c.pos += 1;
        let e = match tag {
            b'N' => (|| Some(Entry::Note { map: c.str()?, id: c.str()?, ts: c.i64()?, text: c.str()? }))(),
            b'n' => (|| Some(Entry::NoteDel { map: c.str()?, id: c.str()? }))(),
            b'M' => (|| Some(Entry::Map { id: c.str()?, text: c.str()? }))(),
            b'm' => (|| Some(Entry::MapDel { id: c.str()? }))(),
            b'A' => (|| {
                let name = c.str()?;
                let (at, data) = c.bytes()?;
                Some(Entry::Asset { name, at, len: data.len() })
            })(),
            b'a' => (|| Some(Entry::AssetDel { name: c.str()? }))(),
            b'T' => (|| Some(Entry::Trash { key: c.str()?, map: c.str()?, id: c.str()?, ts: c.i64()?, text: c.str()? }))(),
            b't' => (|| Some(Entry::TrashDel { key: c.str()? }))(),
            b'S' => (|| Some(Entry::Setting { key: c.str()?, value: c.str()? }))(),
            _ => None,
        };
        out.push(e.ok_or_else(unknown)?);
    }
    Ok(out)
}

// ── Le fichier ────────────────────────────────────────────────────────────────

/// Ce que la lecture a dû écarter.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Recovery {
    /// Octets incomplets en fin de fichier (arrêt brutal pendant une écriture), retirés.
    pub dropped_tail: u64,
    /// Octets illisibles au milieu du fichier, sautés (support abîmé).
    pub skipped: u64,
    /// Copie du fichier d'origine, gardée quand des octets ont été sautés.
    pub backup: Option<PathBuf>,
}

/// `protocol.carnet` → `protocol.carnet.<suffixe>`, dans le même dossier.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

fn transient(e: &io::Error) -> bool {
    cfg!(windows) && (e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(5 | 32 | 33)))
}

/// Un antivirus ou un indexeur tient parfois le fichier un instant (Windows).
pub fn retry<T>(mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut delay = 15;
    for _ in 0..7 {
        match f() {
            Err(e) if transient(&e) => {
                std::thread::sleep(Duration::from_millis(delay));
                delay *= 2;
            }
            r => return r,
        }
    }
    f()
}

fn busy() -> io::Error {
    io::Error::new(io::ErrorKind::ResourceBusy, "ce carnet est déjà ouvert par un autre programme protocol")
}

/// Ouvre le fichier du carnet. L'écrivain est seul à pouvoir écrire ; d'autres
/// programmes peuvent lire en même temps (recherche en ligne de commande…).
fn open_file(path: &Path, writable: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(writable);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        o.share_mode(if writable { FILE_SHARE_READ } else { FILE_SHARE_READ | FILE_SHARE_WRITE });
    }
    // Un antivirus ou un indexeur ouvre parfois le fichier juste après une écriture :
    // on attend un peu avant de conclure qu'un autre programme protocol le tient.
    let sharing = |e: &io::Error| cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33));
    let mut attempt = 0;
    let f = loop {
        match o.open(path) {
            Err(e) if sharing(&e) && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) if sharing(&e) => return Err(busy()),
            r => break r?,
        }
    };
    #[cfg(not(windows))]
    if writable && f.try_lock().is_err() {
        return Err(busy());
    }
    Ok(f)
}

fn new_id() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.finish()
}

fn header(id: u64) -> [u8; HEADER_LEN as usize] {
    let mut h = [0u8; HEADER_LEN as usize];
    h[..16].copy_from_slice(MAGIC);
    h[16..20].copy_from_slice(&FORMAT.to_le_bytes());
    h[20..28].copy_from_slice(&id.to_le_bytes());
    h
}

fn check_header(h: &[u8]) -> io::Result<u64> {
    if h.len() < HEADER_LEN as usize || &h[..16] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "ce fichier n'est pas un carnet protocol"));
    }
    let format = u32::from_le_bytes(h[16..20].try_into().unwrap_or_default());
    if format != FORMAT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "ce carnet a été écrit par une version plus récente de protocol"));
    }
    Ok(u64::from_le_bytes(h[20..28].try_into().unwrap_or_default()))
}

enum Rec {
    Ok(Vec<u8>),
    End,
    Bad,
}

/// L'enregistrement qui commence à `pos`, s'il est entier et intact.
fn read_record(r: &mut impl Read, pos: u64, len: u64) -> io::Result<Rec> {
    if pos >= len {
        return Ok(Rec::End);
    }
    if len - pos < RECORD_HEAD {
        return Ok(Rec::Bad);
    }
    let mut head = [0u8; RECORD_HEAD as usize];
    r.read_exact(&mut head)?;
    let n = u32::from_le_bytes(head[4..8].try_into().unwrap_or_default()) as u64;
    if head[..4] != MARK || n > MAX_RECORD || n > len - pos - RECORD_HEAD {
        return Ok(Rec::Bad);
    }
    let mut payload = vec![0u8; n as usize];
    r.read_exact(&mut payload)?;
    if crc32(&payload) != u32::from_le_bytes(head[8..12].try_into().unwrap_or_default()) {
        return Ok(Rec::Bad);
    }
    Ok(Rec::Ok(payload))
}

/// Après un passage illisible : le prochain enregistrement intact, s'il y en a un.
fn find_next(file: &File, from: u64, len: u64) -> io::Result<Option<u64>> {
    let mut f = file;
    f.seek(SeekFrom::Start(from))?;
    let mut rest = Vec::new();
    f.take(len - from).read_to_end(&mut rest)?;
    let mut i = 1;
    while i + MARK.len() <= rest.len() {
        if rest[i..i + 4] == MARK {
            let pos = from + i as u64;
            let mut r = &rest[i..];
            if let Rec::Ok(_) = read_record(&mut r, pos, len)? {
                return Ok(Some(pos));
            }
        }
        i += 1;
    }
    Ok(None)
}

/// Un fichier temporaire propre à ce programme : deux programmes qui créeraient
/// le même carnet au même instant ne mélangent pas leurs écritures.
fn tmp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    sibling(path, &format!("tmp-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// Un remplacement interrompu (arrêt pendant un compactage) est réparé avant
/// toute lecture : l'ancien fichier reprend sa place, ou le neuf garde la sienne.
/// Les fichiers temporaires abandonnés disparaissent (ceux encore ouverts par un
/// autre programme résistent, et c'est voulu).
fn recover_swap(path: &Path) -> io::Result<()> {
    let prev = sibling(path, "prev");
    if prev.exists() {
        if path.exists() {
            let _ = fs::remove_file(&prev);
        } else {
            retry(|| fs::rename(&prev, path))?;
        }
    }
    let stem = format!("{}.tmp-", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).or_else(|| Some(PathBuf::from(".")))
        && let Ok(entries) = fs::read_dir(dir)
    {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&stem) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}

/// Met `tmp` à la place de `path` en gardant l'ancien jusqu'au bout : sur une clé
/// en FAT32, remplacer un fichier n'est pas atomique.
fn swap_in(path: &Path, tmp: &Path) -> io::Result<()> {
    let prev = sibling(path, "prev");
    if path.exists() {
        retry(|| fs::rename(path, &prev))?;
    }
    if let Err(e) = retry(|| fs::rename(tmp, path)) {
        let _ = retry(|| fs::rename(&prev, path));
        return Err(e);
    }
    let _ = fs::remove_file(&prev);
    Ok(())
}

/// Un carnet neuf, écrit à côté (`protocol.carnet.tmp`) puis mis en place d'un
/// seul geste : création, import d'un ancien dossier, compactage.
pub struct Writer {
    tmp: PathBuf,
    out: BufWriter<File>,
    len: u64,
}

impl Writer {
    pub fn create(path: &Path) -> io::Result<Writer> {
        let tmp = tmp_path(path);
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&tmp)?;
        let mut out = BufWriter::with_capacity(1 << 16, file);
        out.write_all(&header(new_id()))?;
        Ok(Writer { tmp, out, len: HEADER_LEN })
    }

    pub fn position(&self) -> u64 {
        self.len
    }

    /// Ajoute un enregistrement ; renvoie la position de ses opérations.
    pub fn record(&mut self, payload: &[u8]) -> io::Result<u64> {
        if payload.len() as u64 > MAX_RECORD {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "enregistrement trop gros"));
        }
        self.out.write_all(&MARK)?;
        self.out.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.out.write_all(&crc32(payload).to_le_bytes())?;
        self.out.write_all(payload)?;
        let at = self.len + RECORD_HEAD;
        self.len += RECORD_HEAD + payload.len() as u64;
        Ok(at)
    }

    /// Recopie des enregistrements entiers, tels quels.
    fn copy(&mut self, src: &mut File, from: u64, n: u64) -> io::Result<()> {
        src.seek(SeekFrom::Start(from))?;
        let copied = io::copy(&mut src.take(n), &mut self.out)?;
        if copied != n {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "carnet raccourci pendant la copie"));
        }
        self.len += n;
        Ok(())
    }

    fn seal(self) -> io::Result<PathBuf> {
        let file = self.out.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(self.tmp)
    }

    /// Termine un carnet neuf et le met en place (il ne doit pas encore exister).
    pub fn install(self, path: &Path) -> io::Result<()> {
        let tmp = self.seal()?;
        if path.exists() {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "un carnet existe déjà à cet endroit"));
        }
        swap_in(path, &tmp)
    }

    /// Abandon : le fichier temporaire disparaît.
    pub fn discard(self) {
        let tmp = self.tmp.clone();
        drop(self.out);
        let _ = fs::remove_file(tmp);
    }
}

pub struct Journal {
    path: PathBuf,
    /// `None` le temps d'un remplacement du fichier.
    file: Option<File>,
    end: u64,
    writable: bool,
    id: u64,
}

impl Journal {
    /// Ouvre le carnet et passe chaque enregistrement intact à `apply` (avec la
    /// position de ses opérations dans le fichier). En écriture, une fin
    /// incomplète est retirée ; un passage illisible au milieu est sauté, après
    /// avoir gardé une copie du fichier.
    pub fn open(path: &Path, writable: bool, mut apply: impl FnMut(u64, &[u8]) -> io::Result<()>) -> io::Result<(Journal, Recovery)> {
        if writable {
            recover_swap(path)?;
        }
        let file = open_file(path, writable)?;
        let len = file.metadata()?.len();
        let mut head = vec![0u8; HEADER_LEN.min(len) as usize];
        (&file).read_exact(&mut head)?;
        let id = check_header(&head)?;
        let mut rec = Recovery::default();
        let mut pos = HEADER_LEN;
        let mut reader = BufReader::with_capacity(1 << 16, &file);
        loop {
            match read_record(&mut reader, pos, len)? {
                Rec::Ok(payload) => {
                    apply(pos + RECORD_HEAD, &payload)?;
                    pos += RECORD_HEAD + payload.len() as u64;
                }
                Rec::End => break,
                Rec::Bad => match find_next(&file, pos, len)? {
                    Some(next) => {
                        rec.skipped += next - pos;
                        pos = next;
                        let mut f = &file;
                        f.seek(SeekFrom::Start(pos))?;
                        reader = BufReader::with_capacity(1 << 16, &file);
                    }
                    None => {
                        rec.dropped_tail = len - pos;
                        break;
                    }
                },
            }
        }
        drop(reader);
        if writable && rec.skipped > 0 {
            // Avant de toucher à un fichier abîmé, on en garde une copie intacte (par
            // notre propre accès : le fichier reste fermé aux autres écrivains).
            let backup = sibling(path, &format!("endommage-{}", crate::util::stamp_utc(crate::util::unix_now())));
            let mut src = &file;
            src.seek(SeekFrom::Start(0))?;
            let mut out = File::create(&backup)?;
            io::copy(&mut src.take(len), &mut out)?;
            out.sync_all()?;
            rec.backup = Some(backup);
        }
        if writable && rec.dropped_tail > 0 {
            file.set_len(pos)?;
            file.sync_all()?;
        }
        Ok((Journal { path: path.to_path_buf(), file: Some(file), end: pos, writable, id }, rec))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fin des données valides : là où le prochain enregistrement s'écrira.
    pub fn end(&self) -> u64 {
        self.end
    }

    fn file(&mut self) -> io::Result<&mut File> {
        self.file.as_mut().ok_or_else(|| io::Error::other("carnet indisponible"))
    }

    /// Ajoute un enregistrement et attend qu'il soit sur le disque. Renvoie la
    /// position de ses opérations.
    pub fn append(&mut self, payload: &[u8]) -> io::Result<u64> {
        if !self.writable {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "carnet ouvert en lecture seule"));
        }
        if payload.len() as u64 > MAX_RECORD {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "enregistrement trop gros"));
        }
        let mut rec = Vec::with_capacity(RECORD_HEAD as usize + payload.len());
        rec.extend_from_slice(&MARK);
        rec.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        rec.extend_from_slice(&crc32(payload).to_le_bytes());
        rec.extend_from_slice(payload);
        let end = self.end;
        let f = self.file()?;
        let res = (|| {
            f.seek(SeekFrom::Start(end))?;
            f.write_all(&rec)?;
            f.sync_data()
        })();
        if let Err(e) = res {
            // Disque plein, clé retirée… : rien de ce morceau ne doit rester derrière.
            let _ = f.set_len(end);
            return Err(e);
        }
        self.end += rec.len() as u64;
        Ok(end + RECORD_HEAD)
    }

    /// Relit `len` octets à la position `at` (une image).
    pub fn read_at(&mut self, at: u64, len: u64) -> io::Result<Vec<u8>> {
        let f = self.file()?;
        f.seek(SeekFrom::Start(at))?;
        let mut buf = vec![0u8; len as usize];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Lecture seule : les enregistrements ajoutés depuis par l'écrivain. `false`
    /// si le fichier a été remplacé (compactage) : il faut alors tout relire.
    pub fn follow(&mut self, mut apply: impl FnMut(u64, &[u8]) -> io::Result<()>) -> io::Result<bool> {
        let mut head = [0u8; HEADER_LEN as usize];
        if File::open(&self.path).and_then(|mut f| f.read_exact(&mut head)).is_err() || check_header(&head).ok() != Some(self.id) {
            return Ok(false);
        }
        let f = self.file.as_ref().ok_or_else(|| io::Error::other("carnet indisponible"))?;
        let len = f.metadata()?.len();
        let mut r = f;
        r.seek(SeekFrom::Start(self.end))?;
        let mut reader = BufReader::new(r);
        let mut pos = self.end;
        // Un enregistrement en cours d'écriture attendra le prochain passage.
        while let Rec::Ok(payload) = read_record(&mut reader, pos, len)? {
            apply(pos + RECORD_HEAD, &payload)?;
            pos += RECORD_HEAD + payload.len() as u64;
        }
        self.end = pos;
        Ok(true)
    }

    /// Termine un compactage : les enregistrements écrits depuis l'instantané
    /// (`upto`) sont recopiés à la suite, puis le fichier neuf remplace l'ancien.
    /// Renvoie le décalage à appliquer aux positions de ces enregistrements.
    pub fn replace(&mut self, mut w: Writer, upto: u64) -> io::Result<i64> {
        let shift = w.position() as i64 - upto as i64;
        let end = self.end;
        if let Err(e) = w.copy(self.file()?, upto, end - upto) {
            w.discard();
            return Err(e);
        }
        let new_end = w.position();
        let tmp = w.tmp.clone();
        let tmp = match w.seal() {
            Ok(t) => t,
            Err(e) => {
                let _ = fs::remove_file(tmp);
                return Err(e);
            }
        };
        // Le fichier doit être fermé pour être remplacé.
        self.file = None;
        let swapped = swap_in(&self.path, &tmp);
        if swapped.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        let file = open_file(&self.path, true)?;
        let mut head = [0u8; HEADER_LEN as usize];
        (&file).read_exact(&mut head)?;
        self.id = check_header(&head)?;
        self.file = Some(file);
        swapped?;
        self.end = new_end;
        Ok(shift)
    }
}

/// Lit un carnet dans un autre fil d'exécution (compactage) : un accès en lecture
/// qui laisse le carnet ouvert en écriture ailleurs.
pub fn reader(path: &Path) -> io::Result<File> {
    open_file(path, false)
}

pub fn read_exact_at(f: &mut File, at: u64, len: u64) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(at))?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    fn ops() -> Vec<Op> {
        vec![
            Op::Map { id: "ascent".into(), text: "---\ntitle: Ascent\n---\n".into() },
            Op::Note { map: "ascent".into(), id: "retake-b".into(), ts: 42, text: "## Calls\n".into() },
            Op::Asset { name: "assets/a.png".into(), data: b"\x89PNG".to_vec() },
            Op::Trash { key: "k".into(), map: "ascent".into(), id: "x".into(), ts: -1, text: "é".into() },
            Op::Setting { key: "theme".into(), value: "dark".into() },
            Op::NoteDel { map: "ascent".into(), id: "x".into() },
            Op::MapDel { id: "m".into() },
            Op::AssetDel { name: "n".into() },
            Op::TrashDel { key: "k".into() },
        ]
    }

    #[test]
    fn operations_round_trip() {
        let payload = encode(&ops());
        let got = decode(&payload).unwrap();
        assert_eq!(got.len(), 9);
        assert_eq!(got[1], Entry::Note { map: "ascent", id: "retake-b", ts: 42, text: "## Calls\n" });
        let Entry::Asset { at, len, .. } = got[2] else { panic!() };
        assert_eq!(&payload[at..at + len], b"\x89PNG");
        let only = encode(&[Op::Asset { name: "assets/a.png".into(), data: b"xyz".to_vec() }]);
        assert_eq!(&only[asset_data_offset("assets/a.png") as usize..], b"xyz");
        // Une opération inconnue (format plus récent) ou tronquée est refusée.
        assert!(decode(b"Z").is_err());
        assert!(decode(&payload[..payload.len() - 1]).is_err());
    }

    fn create(path: &Path, records: &[Vec<Op>]) {
        let mut w = Writer::create(path).unwrap();
        for r in records {
            w.record(&encode(r)).unwrap();
        }
        w.install(path).unwrap();
    }

    fn load(path: &Path, writable: bool) -> (Journal, Recovery, Vec<String>) {
        let mut seen = Vec::new();
        let (j, rec) = Journal::open(path, writable, |_, p| {
            for e in decode(p)? {
                if let Entry::Note { id, text, .. } = e {
                    seen.push(format!("{id}:{text}"));
                }
            }
            Ok(())
        })
        .unwrap();
        (j, rec, seen)
    }

    fn note(id: &str, text: &str) -> Vec<Op> {
        vec![Op::Note { map: "m".into(), id: id.into(), ts: 0, text: text.into() }]
    }

    #[test]
    fn appends_survive_and_a_torn_tail_is_dropped() {
        let d = temp_dir("journal");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("c.carnet");
        create(&path, &[note("a", "1")]);
        let (mut j, rec, seen) = load(&path, true);
        assert_eq!((rec, seen), (Recovery::default(), vec!["a:1".to_string()]));
        j.append(&encode(&note("b", "2"))).unwrap();
        let end = j.end();
        drop(j);
        // Arrêt brutal en pleine écriture : la moitié d'un enregistrement traîne à la fin.
        let half = {
            let mut r = MARK.to_vec();
            r.extend_from_slice(&100u32.to_le_bytes());
            r.extend_from_slice(&[0; 20]);
            r
        };
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&half).unwrap();
        drop(f);
        // Une lecture seule ne touche à rien.
        let (_, rec, seen) = load(&path, false);
        assert_eq!((rec.dropped_tail, seen.len()), (half.len() as u64, 2));
        assert_eq!(fs::metadata(&path).unwrap().len(), end + half.len() as u64);
        // L'écrivain retire la fin incomplète, et repart de là.
        let (mut j, rec, seen) = load(&path, true);
        assert_eq!((rec.dropped_tail, rec.skipped, seen), (half.len() as u64, 0, vec!["a:1".to_string(), "b:2".to_string()]));
        assert_eq!(fs::metadata(&path).unwrap().len(), end);
        j.append(&encode(&note("c", "3"))).unwrap();
        drop(j);
        assert_eq!(load(&path, false).2.len(), 3);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_record_is_skipped_and_the_file_kept() {
        let d = temp_dir("damage");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("c.carnet");
        create(&path, &[note("a", "premier"), note("b", "abîmé"), note("c", "dernier")]);
        let mut bytes = fs::read(&path).unwrap();
        let at = bytes.windows(5).position(|w| w == "abîmé".as_bytes()[..5].as_ref()).unwrap();
        bytes[at] ^= 0x55;
        fs::write(&path, &bytes).unwrap();
        let (_, rec, seen) = load(&path, true);
        assert!(rec.skipped > 0, "{rec:?}");
        assert_eq!(seen, vec!["a:premier".to_string(), "c:dernier".to_string()]);
        let backup = rec.backup.expect("copie gardée");
        assert_eq!(fs::read(&backup).unwrap(), bytes, "la copie est le fichier d'origine");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn one_writer_at_a_time_readers_welcome() {
        let d = temp_dir("lock");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("c.carnet");
        create(&path, &[note("a", "1")]);
        let (mut w, _, _) = load(&path, true);
        let second = Journal::open(&path, true, |_, _| Ok(()));
        assert_eq!(second.err().map(|e| e.kind()), Some(io::ErrorKind::ResourceBusy));
        // Un lecteur suit ce que l'écrivain ajoute.
        let (mut r, _, _) = load(&path, false);
        w.append(&encode(&note("b", "2"))).unwrap();
        let mut got = Vec::new();
        assert!(r.follow(|_, p| {
            got.push(decode(p)?.len());
            Ok(())
        })
        .unwrap());
        assert_eq!(got, vec![1]);
        drop((w, r));
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_briefly_held_elsewhere_is_waited_for() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = temp_dir("held");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("c.carnet");
        create(&path, &[note("a", "1")]);
        // Comme un antivirus : lecture seule, sans partager l'écriture, relâchée peu après.
        let holder = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            drop(holder);
        });
        let (_, _, seen) = load(&path, true);
        assert_eq!(seen, vec!["a:1".to_string()]);
        release.join().unwrap();
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn interrupted_swaps_are_repaired() {
        let d = temp_dir("swap");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("c.carnet");
        create(&path, &[note("a", "1")]);
        // Coupure entre les deux renommages : seul l'ancien fichier (« prev ») existe.
        fs::rename(&path, sibling(&path, "prev")).unwrap();
        fs::write(sibling(&path, "tmp-1-0"), b"inacheve").unwrap();
        let (_, _, seen) = load(&path, true);
        assert_eq!(seen, vec!["a:1".to_string()]);
        assert!(!sibling(&path, "prev").exists() && !sibling(&path, "tmp-1-0").exists());
        // Un fichier qui n'est pas un carnet est refusé, sans être modifié.
        let other = d.join("autre.carnet");
        fs::write(&other, b"bonjour").unwrap();
        assert!(Journal::open(&other, true, |_, _| Ok(())).is_err());
        assert_eq!(fs::read(&other).unwrap(), b"bonjour");
        let _ = fs::remove_dir_all(&d);
    }
}
