//! Le navigateur V2 : collections colorees, hasard, calage au tempo,
//! remplacement a chaud, effets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{self, Kind};
use crate::{Browser, Host, Loaded, Row};

/// Le tempo ecrit dans un nom de fichier : « 120bpm », « 120 BPM »,
/// « bpm120 », ou un nombre seul de 60 a 200 entre separateurs dans le nom
/// d'une boucle (« drums_loop_95_Am »).
pub fn bpm_from_name(name: &str) -> Option<f64> {
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| name.to_lowercase());
    let b = stem.as_bytes();
    let num_at = |i: usize| -> Option<(f64, usize)> {
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
            j += 1;
        }
        let v: f64 = stem[i..j].parse().ok()?;
        Some((v, j))
    };
    // « 120bpm » / « 120 bpm » / « 120-bpm »
    if let Some(k) = stem.find("bpm") {
        // le nombre juste avant
        let mut e = k;
        while e > 0 && matches!(b[e - 1], b' ' | b'_' | b'-') {
            e -= 1;
        }
        let mut s = e;
        while s > 0 && (b[s - 1].is_ascii_digit() || b[s - 1] == b'.') {
            s -= 1;
        }
        if s < e {
            if let Ok(v) = stem[s..e].parse::<f64>() {
                if (40.0..=300.0).contains(&v) {
                    return Some(v);
                }
            }
        }
        // ou juste apres
        let mut a = k + 3;
        while a < b.len() && matches!(b[a], b' ' | b'_' | b'-') {
            a += 1;
        }
        if a < b.len() && b[a].is_ascii_digit() {
            if let Some((v, _)) = num_at(a) {
                if (40.0..=300.0).contains(&v) {
                    return Some(v);
                }
            }
        }
    }
    // Un nombre seul, dans le nom d'une boucle.
    if stem.contains("loop") {
        let sep = |c: u8| matches!(c, b' ' | b'_' | b'-' | b'.' | b'(' | b')' | b'[' | b']');
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit() && (i == 0 || sep(b[i - 1])) {
                let mut j = i;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j == b.len() || sep(b[j]) {
                    if let Ok(v) = stem[i..j].parse::<f64>() {
                        if (60.0..=200.0).contains(&v) {
                            return Some(v);
                        }
                    }
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    None
}

/// La vitesse qui cale `file_bpm` sur `project_bpm`, ramenee par octaves
/// (x 0,5 / x 2) entre 0,75 et 1,5.
pub fn sync_rate(file_bpm: f64, project_bpm: f64) -> Option<f64> {
    if file_bpm <= 0.0 || project_bpm <= 0.0 {
        return None;
    }
    let mut r = project_bpm / file_bpm;
    while r >= 1.5 {
        r /= 2.0;
    }
    while r < 0.75 {
        r *= 2.0;
    }
    Some(r)
}

/// La version etiree d'un fichier (le calage au tempo sans changer la
/// hauteur) : sa region, sa vitesse, son ticket de cuisson puis son clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretched {
    /// Le clip du fichier.
    pub src: u32,
    /// La region etiree (frames du fichier ; `b` = 0 : jusqu'au bout).
    pub a: i64,
    pub b: i64,
    pub tempo: f64,
    pub ticket: u32,
    /// 0 tant qu'elle cuit.
    pub clip: u32,
    pub failed: bool,
}

/// Au-dela, on n'etire pas (la cuisson et la memoire) : le calage se fait
/// par la vitesse, la hauteur suit.
const STRETCH_MAX_S: f64 = 600.0;

/// Un etat de remplacement a chaud : le fichier et le decalage d'origine de
/// l'item.
#[derive(Clone, Debug)]
pub struct Swap {
    pub orig_path: String,
    pub orig_offset: f64,
}

impl Browser {
    // -----------------------------------------------------------------------
    // Le tempo
    // -----------------------------------------------------------------------

    /// La version etiree du fichier montre a `tempo` (sa region : la
    /// selection, sinon tout le fichier) : (clip, debut de la region) si elle
    /// est cuite ; sinon la demande (une fois) et rend rien.
    pub fn stretch_for(&mut self, h: &mut dyn Host, tempo: f64) -> Option<(u32, i64)> {
        let (ra, rb) = self.wave.sel.unwrap_or((0, 0));
        let src = self.wave.clip;
        if let Some(s) = &self.stretched {
            if s.src == src && s.a == ra && s.b == rb && (s.tempo - tempo).abs() < 1e-9 {
                let ready = s.clip != 0 && h.info(s.clip).is_some_and(|i| i.complete());
                return ready.then_some((s.clip, ra));
            }
        }
        if let Some(old) = self.stretched.take() {
            if old.clip != 0 {
                h.release(old.clip);
            }
        }
        let i = self.wave.info;
        if i.srate > 0.0 && i.frames as f64 / i.srate > STRETCH_MAX_S {
            return None;
        }
        let t = h.stretch(src, ra, rb, tempo);
        self.stretched = Some(Stretched {
            src,
            a: ra,
            b: rb,
            tempo,
            ticket: t,
            clip: 0,
            failed: t == 0,
        });
        None
    }

    /// La cuisson avance ; quand elle est prete et que le fichier joue
    /// encore cale (la hauteur changee), l'ecoute passe sur la version
    /// etiree, au meme endroit.
    pub fn tick_stretch(&mut self, h: &mut dyn Host) {
        let Some(s) = self.stretched.as_mut() else {
            return;
        };
        if s.ticket != 0 {
            match h.poll(s.ticket) {
                Loaded::Pending => {}
                Loaded::Ready(c) => {
                    s.ticket = 0;
                    s.clip = c;
                }
                Loaded::Failed(_) => {
                    s.ticket = 0;
                    s.failed = true;
                }
            }
        }
        let s = s.clone();
        if s.clip == 0 || self.voice == 0 || self.voice_map.is_some() {
            return;
        }
        if s.src != self.wave.clip || self.voice_path != self.wave.path {
            return;
        }
        if !h.info(s.clip).is_some_and(|i| i.complete()) {
            return;
        }
        if self.sync_rate(h).is_none_or(|r| (r - s.tempo).abs() > 1e-9) {
            return;
        }
        if self.wave.sel.unwrap_or((0, 0)) != (s.a, s.b) {
            return;
        }
        let Some((pos, st)) = h.voice(self.voice) else {
            return;
        };
        if st == 0 {
            return;
        }
        let off = ((pos - s.a as f64) / s.tempo).round().max(0.0) as i64;
        h.stop(self.voice, 0.005);
        self.voice = h.play(s.clip, off, 0, self.looping, self.gain());
        self.voice_map = (self.voice != 0).then_some((s.a, s.tempo));
        self.dirty = true;
    }

    /// Le calage vient de changer : l'ecoute repart du meme endroit du
    /// fichier, calee ou non.
    pub fn resync(&mut self, h: &mut dyn Host) {
        if self.voice == 0 {
            return;
        }
        let pos = h.voice(self.voice).map(|(p, _)| match self.voice_map {
            Some((ra, r)) => ra as f64 + p * r,
            None => p,
        });
        self.play_wave(h, pos.map(|p| p as i64));
    }

    /// La vitesse de calage du fichier montre, si le calage est actif et le
    /// tempo du fichier connu.
    pub fn sync_rate(&self, h: &mut dyn Host) -> Option<f64> {
        if !self.sync || !self.wave.has() {
            return None;
        }
        let fb = bpm_from_name(&model::display_name(Path::new(&self.wave.path)))?;
        sync_rate(fb, h.project_bpm())
    }

    // -----------------------------------------------------------------------
    // Les collections
    // -----------------------------------------------------------------------

    fn collections_file(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("collections.txt"))
    }

    /// Relit les collections (au demarrage).
    pub fn load_collections(&mut self) {
        let Some(f) = self.collections_file() else {
            return;
        };
        let text = std::fs::read_to_string(f).unwrap_or_default();
        self.collections.clear();
        for line in text.lines() {
            if let Some((m, p)) = line.split_once('\t') {
                if let Ok(mask) = m.trim().parse::<u8>() {
                    if mask != 0 {
                        self.collections.insert(PathBuf::from(p), mask);
                    }
                }
            }
        }
        self.apply_collections();
    }

    fn save_collections(&self) {
        let Some(f) = self.collections_file() else {
            return;
        };
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut v: Vec<(&PathBuf, &u8)> = self.collections.iter().collect();
        v.sort();
        let text: String = v
            .iter()
            .map(|(p, m)| format!("{m}\t{}\n", p.to_string_lossy()))
            .collect();
        let _ = std::fs::write(f, text);
    }

    fn apply_collections(&mut self) {
        for n in 1..=7u8 {
            let mut paths: Vec<PathBuf> = self
                .collections
                .iter()
                .filter(|(_, m)| **m & (1 << (n - 1)) != 0)
                .map(|(p, _)| p.clone())
                .collect();
            paths.sort_by_key(|p| model::display_name(p).to_lowercase());
            self.tree.set_collection(n, &paths);
        }
        self.rebuild();
    }

    /// Le masque des collections d'un chemin (bit n-1 = collection n).
    pub fn collection_mask(&self, p: &Path) -> u8 {
        self.collections.get(p).copied().unwrap_or(0)
    }

    pub fn toggle_collection(&mut self, n: u8) {
        let Some(p) = self.selected_path_pub() else {
            return;
        };
        let bit = 1u8 << (n - 1);
        let m = self.collections.entry(p.clone()).or_insert(0);
        *m ^= bit;
        if *m == 0 {
            self.collections.remove(&p);
        }
        self.save_collections();
        self.apply_collections();
        self.status = format!("{} collection", model::COLLECTION_NAMES[(n - 1) as usize]);
    }

    // -----------------------------------------------------------------------
    // Le hasard
    // -----------------------------------------------------------------------

    fn rand(&mut self) -> u64 {
        // xorshift64*, graine posee a la construction.
        let mut x = self.seed;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Un fichier au hasard : parmi les resultats d'une recherche, sinon dans
    /// le dossier de la ligne choisie (et ses sous-dossiers deja ouverts).
    pub fn random_pick(&mut self, h: &mut dyn Host, insert: bool) {
        let candidates: Vec<usize> = if !self.search.text.trim().is_empty() {
            (0..self.rows.len())
                .filter(|&i| self.row_is_file(self.rows[i]))
                .collect()
        } else {
            let scope = match self.sel.and_then(|i| self.rows.get(i).copied()) {
                Some(Row::Node(id)) if self.tree.is_container(id) => Some(id),
                Some(Row::Node(id)) => self.tree.nodes[id].parent,
                _ => None,
            };
            let Some(scope) = scope else { return };
            // Ouvrir le dossier s'il ne l'est pas : le hasard voit ses fichiers.
            if !self.tree.nodes[scope].expanded {
                self.tree.load(scope);
                self.tree.nodes[scope].expanded = true;
                self.rebuild();
            }
            let under = |b: &Browser, id: usize| {
                let mut p = b.tree.nodes[id].parent;
                while let Some(x) = p {
                    if x == scope {
                        return true;
                    }
                    p = b.tree.nodes[x].parent;
                }
                false
            };
            (0..self.rows.len())
                .filter(|&i| match self.rows[i] {
                    Row::Node(id) => self.tree.nodes[id].kind == Kind::File && under(self, id),
                    _ => false,
                })
                .collect()
        };
        if candidates.is_empty() {
            self.status = "No file to pick from".into();
            return;
        }
        let k = candidates[(self.rand() % candidates.len() as u64) as usize];
        self.stop(h);
        self.select_row_pub(k, h, !insert);
        if insert {
            self.insert_selected_pub(h);
        }
    }

    // -----------------------------------------------------------------------
    // Le remplacement a chaud
    // -----------------------------------------------------------------------

    pub fn swapping(&self) -> bool {
        self.swap.is_some()
    }

    /// Q : commence (le fichier choisi remplace celui de l'item choisi dans
    /// REAPER), ou finit en gardant.
    pub fn swap_toggle(&mut self, h: &mut dyn Host) {
        if self.swap.take().is_some() {
            self.status = "Hot swap kept".into();
            return;
        }
        match h.swap_begin() {
            Ok((p, off)) => {
                self.swap = Some(Swap {
                    orig_path: p,
                    orig_offset: off,
                });
                self.stop(h);
                self.swap_to_selected(h);
                self.status =
                    "Hot swap: arrows change the item's file, Enter or Q keeps, Esc reverts".into();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Le fichier choisi devient celui de l'item.
    pub fn swap_to_selected(&mut self, h: &mut dyn Host) {
        let Some(p) = self.selected_path_pub() else {
            return;
        };
        let Some(r) = self.sel.and_then(|i| self.rows.get(i).copied()) else {
            return;
        };
        if !self.row_is_file(r) {
            return;
        }
        let ps = p.to_string_lossy().into_owned();
        let off = if self.wave.path == ps {
            self.wave.sel_seconds().map_or(0.0, |(a, _)| a)
        } else {
            0.0
        };
        if let Err(e) = h.swap(&ps, off) {
            self.error = Some(e);
        }
    }

    /// Echap : l'item retrouve son fichier.
    pub fn swap_cancel(&mut self, h: &mut dyn Host) {
        if let Some(s) = self.swap.take() {
            match h.swap(&s.orig_path, s.orig_offset) {
                Ok(()) => self.status = "Hot swap reverted".into(),
                Err(e) => self.error = Some(e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Les effets
    // -----------------------------------------------------------------------

    pub fn ensure_fx(&mut self, h: &mut dyn Host) {
        if self.tree.nodes[self.tree.fx_root].children.is_none() {
            let list = h.installed_fx();
            self.tree.set_fx(&list);
        }
    }

    pub fn add_selected_fx(&mut self, h: &mut dyn Host) -> bool {
        let Some(Row::Node(id)) = self.sel.and_then(|i| self.rows.get(i).copied()) else {
            return false;
        };
        if self.tree.nodes[id].kind != Kind::Fx {
            return false;
        }
        let full = self.tree.nodes[id].path.to_string_lossy().into_owned();
        match h.add_fx(&full) {
            Ok(()) => self.status = format!("Added {}", self.tree.nodes[id].name),
            Err(e) => self.error = Some(e),
        }
        self.mark();
        true
    }
}

/// La graine du hasard : l'heure.
pub fn seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678);
    t | 1
}

/// Les collections : chemin -> masque.
pub type Collections = HashMap<PathBuf, u8>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_tempo_dans_le_nom() {
        assert_eq!(bpm_from_name("Drum Loop 120bpm.wav"), Some(120.0));
        assert_eq!(bpm_from_name("bass_95_BPM_Am.wav"), Some(95.0));
        assert_eq!(bpm_from_name("BPM128 synth.wav"), Some(128.0));
        assert_eq!(bpm_from_name("perc_loop_174_dry.wav"), Some(174.0));
        assert_eq!(bpm_from_name("Kick 01.wav"), None);
        assert_eq!(bpm_from_name("loop 2024 take 3.wav"), None);
        assert_eq!(
            bpm_from_name("pad 140.wav"),
            None,
            "un nombre seul hors d'une boucle ne compte pas"
        );
    }

    #[test]
    fn la_vitesse_reste_entre_0_75_et_1_5() {
        assert_eq!(sync_rate(120.0, 120.0), Some(1.0));
        assert_eq!(
            sync_rate(60.0, 120.0),
            Some(1.0),
            "une moitie de tempo se joue a 1"
        );
        assert_eq!(sync_rate(100.0, 120.0), Some(1.2));
        assert!((sync_rate(174.0, 87.0).unwrap() - 1.0).abs() < 1e-12);
        let r = sync_rate(95.0, 140.0).unwrap();
        assert!((0.75..1.5).contains(&r));
    }
}
