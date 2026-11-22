//! Un zip en lecture : le repertoire central, puis chaque fichier a la
//! demande (stocke ou DEFLATE). Assez pour un `.ReaperThemeZip`.

use std::collections::HashMap;

use crate::inflate::inflate;

/// Plus gros fichier qu'on accepte de decompresser (une image de theme fait
/// quelques Ko ; un fichier malforme ne doit pas remplir la memoire).
const MAX_FILE: usize = 64 << 20;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    method: u16,
    flags: u16,
    csize: usize,
    usize_: usize,
    local: usize,
}

pub struct Zip {
    data: Vec<u8>,
    pub entries: Vec<Entry>,
    /// Nom normalise (voir [`norm`]) vers l'index dans `entries`.
    index: HashMap<String, usize>,
}

fn u16_at(d: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(p..p + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?))
}

/// Un nom de fichier comparable : barres obliques normales, minuscules.
pub fn norm(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

impl Zip {
    pub fn parse(data: Vec<u8>) -> Option<Zip> {
        // La fin du repertoire central : cherchee depuis la fin (un
        // commentaire de 64 Ko au plus la suit).
        let n = data.len();
        if n < 22 {
            return None;
        }
        let lo = n.saturating_sub(22 + 0xFFFF);
        let eocd = (lo..=n - 22)
            .rev()
            .find(|&p| u32_at(&data, p) == Some(0x0605_4b50))?;
        let count = u16_at(&data, eocd + 10)? as usize;
        let mut p = u32_at(&data, eocd + 16)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(&data, p)? != 0x0201_4b50 {
                return None;
            }
            let flags = u16_at(&data, p + 8)?;
            let method = u16_at(&data, p + 10)?;
            let csize = u32_at(&data, p + 20)? as usize;
            let usize_ = u32_at(&data, p + 24)? as usize;
            let nl = u16_at(&data, p + 28)? as usize;
            let el = u16_at(&data, p + 30)? as usize;
            let cl = u16_at(&data, p + 32)? as usize;
            let local = u32_at(&data, p + 42)? as usize;
            let name = String::from_utf8_lossy(data.get(p + 46..p + 46 + nl)?).into_owned();
            entries.push(Entry {
                name,
                method,
                flags,
                csize,
                usize_,
                local,
            });
            p += 46 + nl + el + cl;
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (norm(&e.name), i))
            .collect();
        Some(Zip {
            data,
            entries,
            index,
        })
    }

    /// Un fichier par son nom (sans egard a la casse ni aux barres).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.index.get(&norm(name)).map(|&i| &self.entries[i])
    }

    pub fn read_entry(&self, e: &Entry) -> Option<Vec<u8>> {
        if e.flags & 1 != 0 || e.usize_ > MAX_FILE {
            return None; // chiffre, ou demesure
        }
        let d = &self.data;
        if u32_at(d, e.local)? != 0x0403_4b50 {
            return None;
        }
        let nl = u16_at(d, e.local + 26)? as usize;
        let el = u16_at(d, e.local + 28)? as usize;
        let start = e.local + 30 + nl + el;
        let raw = d.get(start..start + e.csize)?;
        let out = match e.method {
            0 => raw.to_vec(),
            8 => inflate(raw, e.usize_.max(1))?,
            _ => return None,
        };
        (out.len() == e.usize_).then_some(out)
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.read_entry(self.find(name)?)
    }
}
