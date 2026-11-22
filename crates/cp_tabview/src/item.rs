//! Les corrections de la tablature d'un item : la case, la corde, les
//! liaisons, effacer, ajouter, garder un fantome, deplacer sur la grille.
//! Toute note touchee devient TENUE : un nouveau decodage la respecte.

use cp_tab::doc::{self, TNote, Tech};
use cp_tab::engrave::{Engraved, engrave};

use crate::Item;

impl Item {
    /// Un item neuf : son item audio, sa prise, la cle de son analyse.
    pub fn new(guid: String, take: String, akey: String) -> Item {
        Item {
            guid,
            take,
            akey,
            ..Item::default()
        }
    }

    /// La partition est a refaire.
    pub fn touch(&mut self) {
        self.eng_ok = false;
    }

    /// Une nouvelle tablature (decodage, relecture du MIDI).
    pub fn set_doc(&mut self, d: Vec<TNote>) {
        self.doc = d;
        self.eng_ok = false;
    }

    /// La partition (refaite si la tablature a change).
    pub fn engraved(&mut self) -> &Engraved {
        if !self.eng_ok {
            self.eng = engrave(&self.doc, self.origin_qn, self.sig, self.detect.grid);
            self.eng_ok = true;
        }
        &self.eng
    }

    /// Une position calee sur la grille de l'item.
    pub fn snap(&self, qn: f64) -> f64 {
        let g = self.detect.grid.max(1) as f64;
        let o = self.origin_qn;
        o + ((qn - o) * g).round() / g
    }

    /// Change la case d'une note (meme corde, autre hauteur).
    pub fn set_fret(&mut self, n: usize, fret: i32) -> Option<()> {
        let t = self.detect.tuning();
        let note = self.doc.get_mut(n)?;
        let open = *t.strings.get(note.string)?;
        if !(0..=t.frets).contains(&fret) {
            return None;
        }
        note.fret = fret;
        note.pitch = open + fret;
        note.locked = true;
        self.eng_ok = false;
        Some(())
    }

    /// La meme note sur la corde voisine (`dir` +1 : plus aigue).
    pub fn change_string(&mut self, n: usize, dir: i32) -> Option<()> {
        let t = self.detect.tuning();
        let note = self.doc.get(n)?;
        let mut s = note.string as i32 + dir;
        while s >= 0 && (s as usize) < t.strings.len() {
            let f = note.pitch - t.strings[s as usize];
            if (0..=t.frets).contains(&f) {
                let note = &mut self.doc[n];
                note.string = s as usize;
                note.fret = f;
                note.locked = true;
                self.eng_ok = false;
                return Some(());
            }
            s += dir;
        }
        None
    }

    /// Passe une note sur une autre corde, a la meme hauteur (si elle s'y
    /// joue). Vrai si elle a change.
    pub fn set_string(&mut self, n: usize, s: usize) -> bool {
        let t = self.detect.tuning();
        let Some(note) = self.doc.get_mut(n) else {
            return false;
        };
        let Some(&open) = t.strings.get(s) else {
            return false;
        };
        let f = note.pitch - open;
        if note.string == s || !(0..=t.frets).contains(&f) {
            return false;
        }
        note.string = s;
        note.fret = f;
        note.locked = true;
        self.eng_ok = false;
        true
    }

    pub fn set_tech(&mut self, n: usize, tech: Tech) -> Option<()> {
        let note = self.doc.get_mut(n)?;
        note.tech = if note.tech == tech { Tech::Plain } else { tech };
        note.locked = true;
        self.eng_ok = false;
        Some(())
    }

    /// Efface une note (un nouveau decodage ne la ramene pas). Rend la note a
    /// choisir ensuite.
    pub fn delete_note(&mut self, n: usize) -> Option<Option<usize>> {
        if n >= self.doc.len() {
            return None;
        }
        let note = self.doc.remove(n);
        self.suppressed.insert(note.key());
        self.eng_ok = false;
        Some((!self.doc.is_empty()).then(|| n.min(self.doc.len() - 1)))
    }

    /// Une note neuve, tenue, sur une corde, a une position (calee). Rend son
    /// rang.
    pub fn add_note(&mut self, qn: f64, string: usize, fret: i32) -> Option<usize> {
        let t = self.detect.tuning();
        let open = *t.strings.get(string)?;
        let q0 = self.snap(qn);
        let n = TNote {
            qn0: q0,
            qn1: q0 + 2.0 / self.detect.grid.max(1) as f64,
            pitch: open + fret,
            string,
            fret,
            vel: 96,
            tech: Tech::Plain,
            locked: true,
            ..Default::default()
        };
        let k = n.key();
        self.doc.push(n);
        doc::sort(&mut self.doc);
        self.eng_ok = false;
        self.doc.iter().position(|x| x.key() == k)
    }

    /// Garde une note fantome : elle entre dans la tablature, tenue.
    pub fn keep_ghost(&mut self, g: usize) -> Option<usize> {
        if g >= self.ghosts.len() {
            return None;
        }
        let mut n = self.ghosts.remove(g);
        n.locked = true;
        let k = n.key();
        self.suppressed.remove(&k);
        self.doc.push(n);
        doc::sort(&mut self.doc);
        self.eng_ok = false;
        self.doc.iter().position(|x| x.key() == k)
    }

    /// Garde les fantomes d'un passage (ou tous). Rend combien.
    pub fn keep_ghosts(&mut self, range: Option<(f64, f64)>) -> usize {
        let inside = |t: &TNote| range.is_none_or(|(a, b)| t.qn0 >= a && t.qn0 < b);
        let (keep, rest): (Vec<TNote>, Vec<TNote>) = std::mem::take(&mut self.ghosts)
            .into_iter()
            .partition(inside);
        self.ghosts = rest;
        let k = keep.len();
        for mut n in keep {
            n.locked = true;
            self.suppressed.remove(&n.key());
            self.doc.push(n);
        }
        if k > 0 {
            doc::sort(&mut self.doc);
            self.eng_ok = false;
        }
        k
    }

    /// Deplace une note (garde sa duree).
    pub fn move_note(&mut self, n: usize, qn0: f64) -> bool {
        let Some(note) = self.doc.get_mut(n) else {
            return false;
        };
        if (note.qn0 - qn0).abs() < 1e-9 {
            return false;
        }
        let len = note.qn1 - note.qn0;
        note.qn0 = qn0;
        note.qn1 = qn0 + len;
        note.locked = true;
        self.eng_ok = false;
        true
    }

    /// Retrie apres un deplacement ; rend le nouveau rang de la note `n`.
    pub fn resort(&mut self, n: usize) -> Option<usize> {
        let k = self.doc.get(n).map(TNote::key);
        doc::sort(&mut self.doc);
        self.eng_ok = false;
        k.and_then(|k| self.doc.iter().position(|t| t.key() == k))
    }

    /// Les noires d'une mesure de l'item.
    pub fn bar_qn(&self) -> f64 {
        self.sig.0.max(1) as f64 * 4.0 / self.sig.1.max(1) as f64
    }
}
