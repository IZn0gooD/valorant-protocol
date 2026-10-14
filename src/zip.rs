//! Une archive ZIP minimale, fichiers stockés sans compression : l'export du
//! carnet en fichiers Markdown, que l'Explorateur Windows, macOS ou Obsidian
//! ouvrent sans rien installer.

use std::io;

use crate::util::{crc32, dos_datetime};

pub struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    time: u16,
    date: u16,
}

impl Zip {
    /// `now` : la date (secondes Unix) donnée à chaque fichier.
    pub fn new(now: i64) -> Zip {
        let (time, date) = dos_datetime(now);
        Zip { out: Vec::new(), central: Vec::new(), count: 0, time, date }
    }

    /// Ajoute un fichier (`dossier/nom.md`, chemins en UTF-8).
    pub fn add(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        // Format ZIP classique : 65 535 fichiers et 4 Go au plus, très au-delà d'un carnet.
        if self.count == u16::MAX || (self.out.len() + data.len() + self.central.len()) as u64 > u32::MAX as u64 - (1 << 20) {
            return Err(io::Error::other("carnet trop volumineux pour une archive ZIP"));
        }
        let offset = self.out.len() as u32;
        let crc = crc32(data);
        let n = name.as_bytes();
        let size = (data.len() as u32).to_le_bytes();
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&10u16.to_le_bytes()); // version nécessaire : 1.0 (fichier stocké)
            v.extend_from_slice(&0x0800u16.to_le_bytes()); // noms en UTF-8
            v.extend_from_slice(&0u16.to_le_bytes()); // méthode : stocké
            v.extend_from_slice(&self.time.to_le_bytes());
            v.extend_from_slice(&self.date.to_le_bytes());
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&size);
            v.extend_from_slice(&size);
            v.extend_from_slice(&(n.len() as u16).to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // champ supplémentaire : aucun
        };
        self.out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut self.out);
        self.out.extend_from_slice(n);
        self.out.extend_from_slice(data);
        self.central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        self.central.extend_from_slice(&20u16.to_le_bytes()); // créé par : version 2.0
        common(&mut self.central);
        self.central.extend_from_slice(&0u16.to_le_bytes()); // commentaire
        self.central.extend_from_slice(&0u16.to_le_bytes()); // disque
        self.central.extend_from_slice(&0u16.to_le_bytes()); // attributs internes
        self.central.extend_from_slice(&0u32.to_le_bytes()); // attributs externes
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(n);
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<Vec<u8>> {
        let (cd_offset, cd_size) = (self.out.len() as u32, self.central.len() as u32);
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]); // disque, disque du répertoire
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&cd_size.to_le_bytes());
        self.out.extend_from_slice(&cd_offset.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes()); // commentaire
        Ok(self.out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn u16_at(b: &[u8], i: usize) -> usize {
        u16::from_le_bytes([b[i], b[i + 1]]) as usize
    }
    fn u32_at(b: &[u8], i: usize) -> usize {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
    }

    /// Les noms du répertoire central, relus comme le ferait un outil d'archive.
    pub(crate) fn names(zip: &[u8]) -> Vec<String> {
        let eocd = zip.len() - 22;
        assert_eq!(u32_at(zip, eocd), 0x0605_4b50);
        let (count, mut at) = (u16_at(zip, eocd + 10), u32_at(zip, eocd + 16));
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(zip, at), 0x0201_4b50);
            let n = u16_at(zip, at + 28);
            let local = u32_at(zip, at + 42);
            assert_eq!(u32_at(zip, local), 0x0403_4b50, "l'en-tête local est bien là où le répertoire l'annonce");
            out.push(String::from_utf8(zip[at + 46..at + 46 + n].to_vec()).unwrap());
            at += 46 + n + u16_at(zip, at + 30) + u16_at(zip, at + 32);
        }
        out
    }

    #[test]
    fn a_readable_archive() {
        let mut z = Zip::new(1_791_804_189);
        z.add("protocol/général/bienvenue.md", "Bonjour".as_bytes()).unwrap();
        z.add("protocol/assets/a.png", b"\x89PNG").unwrap();
        let bytes = z.finish().unwrap();
        assert_eq!(names(&bytes), vec!["protocol/général/bienvenue.md", "protocol/assets/a.png"]);
        // Le contenu est stocké tel quel, avec son CRC.
        let at = bytes.windows(7).position(|w| w == b"Bonjour").unwrap();
        assert_eq!(u32_at(&bytes, 14), crc32(b"Bonjour") as usize);
        assert!(at > 30);
    }
}
