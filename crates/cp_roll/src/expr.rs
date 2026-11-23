//! L'expression des notes : les evenements de canal qui suivent une note
//! quand elle bouge, se copie, se coupe ou s'efface (l'option « Move CC
//! events with notes » de REAPER). Ce sont les pitch bends et la pedale
//! legato (CC 68) : Guitar Tab y ecrit ses bends et ses glisses, sur le canal
//! de la corde.
//!
//! Un evenement appartient a la derniere note commencee sur son canal (un
//! canal joue comme une corde : une note a la fois). Les autres CC, le
//! reglage de l'etendue (RPN 0) et ce qui precede la premiere note d'un
//! canal restent ou ils sont.

use std::collections::{BTreeMap, HashMap};

use crate::Note;

/// Un evenement de canal, en noires du projet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ev {
    pub qn: f64,
    pub chan: i32,
    /// 0xE0 : pitch bend (`d1` poids faible, `d2` poids fort) ; 0xB0 : CC
    /// `d1`, valeur `d2` ; les autres tels quels.
    pub status: i32,
    pub d1: i32,
    pub d2: i32,
    pub sel: bool,
    pub mute: bool,
}

impl Ev {
    /// La valeur d'un pitch bend, de -1 a 1 (presque).
    pub fn bend(&self) -> Option<f32> {
        (self.status == 0xE0).then(|| {
            (((self.d2.clamp(0, 127) << 7) | self.d1.clamp(0, 127)) - 8192) as f32 / 8192.0
        })
    }

    /// Un pitch bend de `v` (-1..1).
    pub fn pitch_bend(qn: f64, chan: i32, v: f32) -> Ev {
        let raw = ((v.clamp(-1.0, 1.0) * 8192.0).round() as i32 + 8192).clamp(0, 16383);
        Ev {
            qn,
            chan,
            status: 0xE0,
            d1: raw & 127,
            d2: raw >> 7,
            sel: false,
            mute: false,
        }
    }

    /// Un CC.
    pub fn cc(qn: f64, chan: i32, num: i32, val: i32) -> Ev {
        Ev {
            qn,
            chan,
            status: 0xB0,
            d1: num,
            d2: val,
            sel: false,
            mute: false,
        }
    }

    /// Il suit sa note : un pitch bend, la pedale legato.
    pub fn follows(&self) -> bool {
        self.status == 0xE0 || (self.status == 0xB0 && self.d1 == 68)
    }
}

/// Un evenement pose juste avant sa note (Guitar Tab : un tic avant) est a
/// elle : la tolerance, en noires (4 tics a 960 par noire).
pub const TOL: f64 = 4.0 / 960.0;

/// Un tic a 960 par noire : la place d'un evenement juste avant sa note.
const TICK: f64 = 1.0 / 960.0;

/// Les evenements d'une prise, rattaches aux notes lues.
#[derive(Clone, Debug)]
pub struct Expr {
    /// Par note lue : son debut a la lecture, ses evenements (dans l'ordre).
    owned: HashMap<u64, (f64, Vec<Ev>)>,
    /// Ceux qui suivent sans note avant eux sur leur canal : ils restent.
    free: Vec<Ev>,
    /// L'etendue des pitch bends par canal, en demi-tons (RPN 0 ; 2 sans
    /// reglage, la valeur du General MIDI).
    range: [f32; 16],
}

impl Default for Expr {
    fn default() -> Expr {
        Expr {
            owned: HashMap::new(),
            free: Vec::new(),
            range: [2.0; 16],
        }
    }
}

impl Expr {
    /// Les evenements d'une prise (tous les CC : le reglage de l'etendue s'y
    /// lit), donnes aux notes lues.
    pub fn new(notes: &[Note], evs: &[Ev]) -> Expr {
        let mut sorted: Vec<&Ev> = evs.iter().collect();
        sorted.sort_by(|a, b| a.qn.total_cmp(&b.qn));
        // RPN 0 : CC 101 = 0 et CC 100 = 0, puis CC 6 (demi-tons) et 38
        // (cents).
        let mut range = [2.0f32; 16];
        let mut rpn = [(-1i32, -1i32); 16];
        for e in sorted.iter().filter(|e| e.status == 0xB0) {
            let c = e.chan.clamp(0, 15) as usize;
            match e.d1 {
                101 => rpn[c].0 = e.d2,
                100 => rpn[c].1 = e.d2,
                6 if rpn[c] == (0, 0) => range[c] = e.d2 as f32,
                38 if rpn[c] == (0, 0) => range[c] = range[c].floor() + e.d2 as f32 / 100.0,
                _ => {}
            }
        }
        let mut by: BTreeMap<i32, Vec<&Note>> = BTreeMap::new();
        for n in notes {
            by.entry(n.chan).or_default().push(n);
        }
        for v in by.values_mut() {
            v.sort_by(|a, b| a.qn0.total_cmp(&b.qn0));
        }
        let mut owned: HashMap<u64, (f64, Vec<Ev>)> = HashMap::new();
        let mut free = Vec::new();
        for e in sorted.into_iter().filter(|e| e.follows()) {
            let owner = by.get(&e.chan).and_then(|v| {
                let k = v.partition_point(|n| n.qn0 <= e.qn + TOL);
                k.checked_sub(1).map(|i| v[i])
            });
            match owner {
                Some(n) => owned
                    .entry(n.src)
                    .or_insert_with(|| (n.anchor, Vec::new()))
                    .1
                    .push(*e),
                None => free.push(*e),
            }
        }
        Expr { owned, free, range }
    }

    /// Rien ne suit les notes.
    pub fn is_empty(&self) -> bool {
        self.owned.is_empty() && self.free.is_empty()
    }

    /// L'etendue des pitch bends du canal (demi-tons).
    pub fn range(&self, chan: i32) -> f32 {
        self.range[chan.clamp(0, 15) as usize]
    }

    /// Les evenements qui suivent, pour les notes d'apres l'edition : ceux
    /// de chaque note lue, decales de ce que son debut a bouge ; une copie
    /// emporte les siens, une note effacee les emporte avec elle. Les
    /// morceaux d'une note coupee (meme origine, meme ancre) les portent une
    /// seule fois. Ce qui tombait avant le debut d'une note rognee se resume
    /// a la valeur en vigueur a son debut.
    pub fn carry(&self, notes: &[Note]) -> Vec<Ev> {
        let mut out = self.free.clone();
        // (origine, ancre) -> le premier debut du groupe ; dans l'ordre des
        // notes (une sortie reproductible).
        let mut groups: Vec<(u64, f64, f64)> = Vec::new();
        let mut at: HashMap<(u64, u64), usize> = HashMap::new();
        for n in notes.iter().filter(|n| self.owned.contains_key(&n.src)) {
            let key = (n.src, n.anchor.to_bits());
            match at.get(&key) {
                Some(&k) => groups[k].2 = groups[k].2.min(n.qn0),
                None => {
                    at.insert(key, groups.len());
                    groups.push((n.src, n.anchor, n.qn0));
                }
            }
        }
        for (src, anchor, start) in groups {
            let Some((q0, evs)) = self.owned.get(&src) else {
                continue;
            };
            let d = anchor - q0;
            let mut before_bend: Option<Ev> = None;
            let mut before_cc: Option<Ev> = None;
            for e in evs {
                let q = e.qn + d;
                let moved = Ev {
                    qn: q.max(0.0),
                    ..*e
                };
                if q < start - TOL {
                    if e.status == 0xE0 {
                        before_bend = Some(moved);
                    } else {
                        before_cc = Some(moved);
                    }
                } else {
                    out.push(moved);
                }
            }
            for b in [before_bend, before_cc].into_iter().flatten() {
                out.push(Ev {
                    qn: (start - TICK).max(0.0),
                    ..b
                });
            }
        }
        out.sort_by(|a, b| a.qn.total_cmp(&b.qn));
        out
    }

    /// Les evenements que porte la note `n` seule (sans ceux qui ne
    /// suivent aucune note), au temps du projet.
    pub fn own(&self, n: &Note) -> Vec<Ev> {
        let free = &self.free;
        self.carry(std::slice::from_ref(n))
            .into_iter()
            .filter(|e| !free.contains(e))
            .collect()
    }

    /// La note `src` porte desormais `evs` (temps du projet) : une copie
    /// qu'on modifie prend sa propre expression.
    pub fn adopt(&mut self, src: u64, anchor: f64, evs: Vec<Ev>) {
        self.owned.insert(src, (anchor, evs));
    }

    /// La hauteur de la note `n` au fil du temps : ses pitch bends (pas sa
    /// pedale) deviennent la courbe `pts`, (noires depuis son debut,
    /// demi-tons) ; vide, la note reste droite. La courbe revient a zero a la
    /// fin de la note (la suivante sur le canal part droite). Les demi-tons
    /// au-dela de l'etendue du canal sont rabotes.
    pub fn set_bend(&mut self, n: &Note, pts: &[(f64, f32)]) {
        let r = self.range(n.chan).max(0.01);
        let e = self
            .owned
            .entry(n.src)
            .or_insert_with(|| (n.anchor, Vec::new()));
        // Le temps du projet = celui retenu + d.
        let d = n.anchor - e.0;
        e.1.retain(|x| x.status != 0xE0);
        if !pts.is_empty() {
            for &(dq, st) in pts {
                e.1.push(Ev::pitch_bend(n.qn0 + dq - d, n.chan, st / r));
            }
            e.1.push(Ev::pitch_bend(n.end() - d, n.chan, 0.0));
        }
        e.1.sort_by(|a, b| a.qn.total_cmp(&b.qn));
    }

    /// La courbe de hauteur d'une note, pour la dessiner : (temps du projet,
    /// demi-tons) de son debut a sa fin ; rien si elle reste droite.
    pub fn curve(&self, n: &Note) -> Vec<(f64, f32)> {
        let Some((q0, evs)) = self.owned.get(&n.src) else {
            return Vec::new();
        };
        let d = n.anchor - q0;
        let r = self.range(n.chan);
        let mut v0 = 0.0f32;
        let mut pts: Vec<(f64, f32)> = Vec::new();
        for e in evs {
            let Some(b) = e.bend() else { continue };
            let q = e.qn + d;
            if q <= n.qn0 + TOL {
                v0 = b * r;
            } else if q < n.end() {
                pts.push((q, b * r));
            }
        }
        if v0.abs() < 1e-3 && pts.iter().all(|p| p.1.abs() < 1e-3) {
            return Vec::new();
        }
        let mut out = vec![(n.qn0, v0)];
        out.extend(pts);
        let last = out.last().map_or(v0, |p| p.1);
        out.push((n.end(), last));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Roll;

    /// Un bend pose sur une note : sa courbe se relit telle quelle, il
    /// revient a zero a sa fin, la pedale reste ; une courbe vide la rend
    /// droite.
    #[test]
    fn un_bend_pose_sur_une_note() {
        let r = Roll::from_notes([
            (0.0, 1.0, 64, 0, 100, false, false),
            (1.0, 1.0, 67, 0, 100, false, false),
        ]);
        let n = r.notes[0];
        let mut x = Expr::new(&r.notes, &[Ev::cc(0.0, 0, 68, 127)]);
        x.set_bend(&n, &[(0.0, 0.0), (0.25, 1.0), (0.5, 2.0)]);
        let c = x.curve(&n);
        assert_eq!(c.len(), 4, "{c:?}");
        assert!((c[2].1 - 2.0).abs() < 0.01);
        assert!((c[3].0 - 1.0).abs() < 1e-9 && (c[3].1 - 2.0).abs() < 0.01);
        let all = x.carry(&r.notes);
        // Le retour a zero, a la fin de la note.
        let back = all
            .iter()
            .filter_map(|e| e.bend().map(|b| (e.qn, b)))
            .next_back()
            .expect("des bends");
        assert!((back.0 - 1.0).abs() < 1e-9 && back.1.abs() < 1e-3);
        assert!(all.iter().any(|e| e.status == 0xB0 && e.d1 == 68));
        // La note suivante n'a rien.
        assert!(x.curve(&r.notes[1]).is_empty());
        x.set_bend(&n, &[]);
        assert!(x.curve(&n).is_empty());
        assert!(x.carry(&r.notes).iter().all(|e| e.status != 0xE0));
    }

    /// Une guitare : deux notes sur la corde 1 (canal 0), la premiere avec
    /// un bend d'un ton, la seconde droite ; une note sur le canal 1. Le
    /// reglage de l'etendue (12) avant tout.
    fn take() -> (Roll, Vec<Ev>) {
        let r = Roll::from_notes([
            (0.0, 1.0, 64, 0, 100, false, false),
            (2.0, 1.0, 67, 0, 100, false, false),
            (0.0, 2.0, 50, 1, 100, false, false),
        ]);
        let mut evs = vec![
            Ev::cc(0.0, 0, 101, 0),
            Ev::cc(0.0, 0, 100, 0),
            Ev::cc(0.0, 0, 6, 12),
            Ev::cc(0.0, 0, 38, 0),
        ];
        // Un tic avant la note : le depart a 0 ; puis la montee ; le retour
        // a 0 apres la fin.
        evs.push(Ev::pitch_bend(0.0, 0, 0.0));
        for k in 1..=4 {
            evs.push(Ev::pitch_bend(
                0.5 + k as f64 * 0.1,
                0,
                2.0 / 12.0 * k as f32 / 4.0,
            ));
        }
        evs.push(Ev::pitch_bend(1.5, 0, 0.0));
        // La pedale legato de la seconde note (un tic avant, un tic apres).
        evs.push(Ev::cc(2.0 - TICK, 0, 68, 127));
        evs.push(Ev::cc(2.0 + TICK, 0, 68, 0));
        // Un CC 1 qui ne suit rien.
        evs.push(Ev::cc(0.25, 0, 1, 64));
        (r, evs)
    }

    fn bends(evs: &[Ev]) -> Vec<(f64, f32)> {
        evs.iter()
            .filter_map(|e| e.bend().map(|b| (e.qn, b * 12.0)))
            .collect()
    }

    fn id_of(r: &Roll, q: f64, p: i32) -> u64 {
        r.notes
            .iter()
            .find(|n| (n.qn0 - q).abs() < 1e-9 && n.pitch == p)
            .unwrap()
            .id
    }

    #[test]
    fn l_etendue_se_lit() {
        let (r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        assert_eq!(x.range(0), 12.0);
        assert_eq!(x.range(1), 2.0);
    }

    #[test]
    fn rien_ne_bouge_rien_ne_change() {
        let (r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let mut want: Vec<Ev> = evs.iter().filter(|e| e.follows()).copied().collect();
        want.sort_by(|a, b| a.qn.total_cmp(&b.qn));
        assert_eq!(x.carry(&r.notes), want);
    }

    #[test]
    fn le_bend_suit_sa_note() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let before = bends(&x.carry(&r.notes));
        let id = id_of(&r, 0.0, 64);
        r.select_only(id);
        r.move_sel(1.0, 3);
        let after = bends(&x.carry(&r.notes));
        assert_eq!(after.len(), before.len());
        for (a, b) in before.iter().zip(&after) {
            assert!((b.0 - a.0 - 1.0).abs() < 1e-9, "{a:?} -> {b:?}");
            // La hauteur ne change pas la courbe : un bend est relatif.
            assert_eq!(a.1, b.1);
        }
        // La pedale de la seconde note n'a pas bouge.
        let ped: Vec<f64> = x
            .carry(&r.notes)
            .iter()
            .filter(|e| e.status == 0xB0 && e.d1 == 68)
            .map(|e| e.qn)
            .collect();
        assert_eq!(ped, vec![2.0 - TICK, 2.0 + TICK]);
    }

    #[test]
    fn une_copie_emporte_son_bend_une_note_effacee_le_sien() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let n0 = bends(&x.carry(&r.notes)).len();
        let id = id_of(&r, 0.0, 64);
        r.select_only(id);
        r.duplicate(4.0, 0);
        let both = bends(&x.carry(&r.notes));
        assert_eq!(both.len(), 2 * n0);
        assert!(both.iter().any(|b| (b.0 - 4.9).abs() < 1e-9));
        // La copie effacee : retour au debut ; l'original efface : plus rien.
        r.delete_sel();
        assert_eq!(bends(&x.carry(&r.notes)).len(), n0);
        r.select_only(id);
        r.delete_sel();
        assert!(bends(&x.carry(&r.notes)).is_empty());
    }

    #[test]
    fn une_note_coupee_garde_une_seule_courbe() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let before = bends(&x.carry(&r.notes));
        let id = id_of(&r, 0.0, 64);
        r.select_only(id);
        assert_eq!(r.split_at(0.75), 1);
        assert_eq!(bends(&x.carry(&r.notes)), before);
        // Un deplacement des deux morceaux ensemble l'emmene une fois.
        r.clear_sel();
        for n in r.notes.iter_mut().filter(|n| n.chan == 0 && n.qn0 < 1.0) {
            n.sel = true;
        }
        r.move_sel(0.5, 0);
        let moved = bends(&x.carry(&r.notes));
        assert_eq!(moved.len(), before.len());
    }

    #[test]
    fn un_debut_rogne_garde_la_valeur_en_vigueur() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let id = id_of(&r, 0.0, 64);
        r.select_only(id);
        r.trim_start_sel(0.75);
        let b = bends(&x.carry(&r.notes));
        // Ce qui precedait le nouveau debut : un seul point, juste avant.
        let first = b.first().unwrap();
        assert!((first.0 - (0.75 - TICK)).abs() < 1e-9, "{b:?}");
        assert!((first.1 - 1.0).abs() < 1e-2, "{b:?}");
        assert!(b.iter().skip(1).all(|p| p.0 >= 0.75));
    }

    #[test]
    fn quantifier_et_humaniser_emmenent_le_bend() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let id = id_of(&r, 0.0, 64);
        r.select_only(id);
        r.nudge(0.1);
        r.quantize(0.25, 1.0, 0.0);
        let n = *r.get(id).unwrap();
        assert!((n.anchor - n.qn0).abs() < 1e-9);
        r.humanize(7, 0.05, 0);
        let n = *r.get(id).unwrap();
        assert!((n.anchor - n.qn0).abs() < 1e-9);
        // Copier-coller : la copie porte la courbe a sa place.
        r.copy();
        r.paste(8.0);
        let c = bends(&x.carry(&r.notes));
        assert!(c.iter().any(|b| (b.0 - 8.9).abs() < 1e-6), "{c:?}");
    }

    #[test]
    fn la_courbe_se_dessine_sur_la_note() {
        let (mut r, evs) = take();
        let x = Expr::new(&r.notes, &evs);
        let id = id_of(&r, 0.0, 64);
        let n = *r.get(id).unwrap();
        let c = x.curve(&n);
        assert_eq!(c.first(), Some(&(0.0, 0.0)));
        // Le sommet : un ton (2 demi-tons, a l'arrondi du pitch bend pres).
        let top = c.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((top - 2.0).abs() < 0.01, "{c:?}");
        assert!((c.last().unwrap().0 - 1.0).abs() < 1e-9);
        // La note droite n'a pas de courbe.
        let flat = *r.get(id_of(&r, 2.0, 67)).unwrap();
        assert!(x.curve(&flat).is_empty());
        // Deplacee, la courbe la suit.
        r.select_only(id);
        r.move_sel(2.5, 0);
        let m = *r.get(id).unwrap();
        let cm = x.curve(&m);
        assert_eq!(cm.len(), c.len());
        assert!((cm[1].0 - c[1].0 - 2.5).abs() < 1e-9);
    }

    #[test]
    fn un_bend_aller_retour_au_pas_pres() {
        for v in [-1.0f32, -0.5, 0.0, 0.25, 0.999] {
            let e = Ev::pitch_bend(0.0, 0, v);
            assert!((e.bend().unwrap() - v).abs() < 1.0 / 8192.0 + 1e-6, "{v}");
        }
    }
}
