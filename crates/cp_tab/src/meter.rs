//! Les mesures d'un projet : ses changements de chiffrage, et les barres qui
//! en sortent.
//!
//! Comme REAPER : les mesures partent de la noire 0 ; un changement de
//! chiffrage ouvre une mesure neuve a sa place (si elle tombe au milieu d'une
//! mesure, celle-ci est raccourcie : une mesure partielle). Le tempo n'y est
//! pour rien : tout est en noires.

/// Les changements de chiffrage : (noire du projet, chiffrage), tries ; le
/// premier vaut aussi avant lui.
#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub changes: Vec<(f64, (i32, i32))>,
    /// Les changements de tempo : (noire, BPM), tries. Ils ne changent pas
    /// les mesures (tout est en noires) ; la partition les ecrit.
    pub tempos: Vec<(f64, f64)>,
}

/// Une mesure : son rang depuis le debut, sa premiere noire, sa longueur
/// en noires, son chiffrage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub index: i64,
    pub qn: f64,
    pub len: f64,
    pub sig: (i32, i32),
}

impl Bar {
    pub fn end(&self) -> f64 {
        self.qn + self.len
    }
    /// La longueur d'un temps (la noire pour 4/4, la croche pour 6/8).
    pub fn beat(&self) -> f64 {
        4.0 / self.sig.1.max(1) as f64
    }
}

/// La tolerance sur les positions : un chiffrage pose un millionieme de
/// noire apres une barre est SUR la barre (pas une mesure-copeau).
const EPS: f64 = 1e-6;

/// La longueur d'une mesure de ce chiffrage, en noires.
pub fn bar_len(sig: (i32, i32)) -> f64 {
    (sig.0.max(1) as f64 * 4.0 / sig.1.max(1) as f64).max(0.25)
}

impl Default for Meter {
    fn default() -> Meter {
        Meter::uniform((4, 4))
    }
}

impl Meter {
    /// Un seul chiffrage, depuis le debut.
    pub fn uniform(sig: (i32, i32)) -> Meter {
        Meter {
            changes: vec![(0.0, sig)],
            tempos: Vec::new(),
        }
    }

    /// Un seul chiffrage, dont les mesures partent de `origin` (une prise
    /// seule, sans projet).
    pub fn single(origin: f64, sig: (i32, i32)) -> Meter {
        Meter {
            changes: vec![(origin, sig)],
            tempos: Vec::new(),
        }
    }

    /// Des changements quelconques : tries, un seul par place (le dernier
    /// l'emporte), un chiffrage nul ignore.
    pub fn new(mut changes: Vec<(f64, (i32, i32))>) -> Meter {
        changes.retain(|c| c.1.0 > 0 && c.1.1 > 0 && c.0.is_finite());
        changes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<(f64, (i32, i32))> = Vec::new();
        for c in changes {
            match out.last_mut() {
                Some(l) if (l.0 - c.0).abs() < EPS => *l = c,
                // Un meme chiffrage repose ailleurs ouvre quand meme une
                // mesure (comme dans REAPER) : on le garde.
                _ => out.push(c),
            }
        }
        if out.is_empty() {
            out.push((0.0, (4, 4)));
        }
        Meter {
            changes: out,
            tempos: Vec::new(),
        }
    }

    /// Les changements de tempo du projet (tries ; un tempo qui ne change
    /// pas, au dixieme pres, n'en est pas un).
    pub fn with_tempos(mut self, mut t: Vec<(f64, f64)>) -> Meter {
        t.retain(|x| x.0.is_finite() && x.1 > 1.0);
        t.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<(f64, f64)> = Vec::new();
        for x in t {
            match out.last_mut() {
                Some(l) if (l.0 - x.0).abs() < EPS => *l = x,
                Some(l) if (l.1 - x.1).abs() < 0.05 => {}
                _ => out.push(x),
            }
        }
        self.tempos = out;
        self
    }

    /// Les changements de tempo a ecrire dans `(q0, q1)` : (noire, BPM).
    pub fn tempo_marks(&self, q0: f64, q1: f64) -> Vec<(f64, f64)> {
        self.tempos
            .iter()
            .copied()
            .filter(|t| t.0 > q0 + EPS && t.0 < q1 - EPS)
            .collect()
    }

    /// Un seul chiffrage partout ?
    pub fn is_uniform(&self) -> bool {
        self.changes.len() <= 1
    }

    /// Le debut du segment `i` (le premier part de -infini pour la recherche,
    /// mais ses mesures se comptent depuis sa place) et sa fin.
    fn seg(&self, i: usize) -> (f64, f64, (i32, i32)) {
        let (q, s) = self.changes[i];
        let end = self.changes.get(i + 1).map_or(f64::INFINITY, |c| c.0);
        (q, end, s)
    }

    /// Le nombre de mesures du segment `i` (les partielles comprises).
    fn seg_bars(&self, i: usize) -> i64 {
        let (q, end, s) = self.seg(i);
        if end.is_infinite() {
            return i64::MAX / 4;
        }
        ((end - q) / bar_len(s) - EPS).ceil().max(0.0) as i64
    }

    /// La mesure qui contient la noire `qn`.
    pub fn bar_at(&self, qn: f64) -> Bar {
        let i = self
            .changes
            .iter()
            .rposition(|c| c.0 <= qn + EPS)
            .unwrap_or(0);
        let (q0, end, s) = self.seg(i);
        let l = bar_len(s);
        let before: i64 = (0..i).map(|j| self.seg_bars(j)).sum();
        let k = ((qn - q0) / l + EPS).floor() as i64;
        let start = q0 + k as f64 * l;
        Bar {
            index: before + k,
            qn: start,
            len: (start + l).min(end) - start,
            sig: s,
        }
    }

    /// Les mesures, de celle qui contient `q0` jusqu'a couvrir `q1`.
    pub fn bars(&self, q0: f64, q1: f64) -> Vec<Bar> {
        let mut out = Vec::new();
        let mut b = self.bar_at(q0);
        loop {
            out.push(b);
            if b.end() >= q1 - EPS || out.len() > 100_000 {
                break;
            }
            b = self.next(&b);
        }
        out
    }

    /// La mesure qui suit `b` : celle qui commence a sa fin (un changement
    /// tout pres de la fin est sur la fin).
    pub fn next(&self, b: &Bar) -> Bar {
        let mut n = self.bar_at(b.end() + EPS);
        n.index = b.index + 1;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_seul_chiffrage() {
        let m = Meter::uniform((4, 4));
        let b = m.bar_at(9.5);
        assert_eq!((b.index, b.qn, b.len, b.sig), (2, 8.0, 4.0, (4, 4)));
        let bs = m.bars(3.0, 12.0);
        assert_eq!(
            bs.iter().map(|b| b.qn).collect::<Vec<_>>(),
            vec![0.0, 4.0, 8.0]
        );
        assert_eq!(Meter::uniform((6, 8)).bar_at(4.0).qn, 3.0);
    }

    #[test]
    fn des_changements_et_une_mesure_partielle() {
        // 4/4 deux mesures, 3/4 a la noire 8, puis 4/4 a la noire 12,5 : la
        // seconde mesure de 3/4 est partielle (11 -> 12,5).
        let m = Meter::new(vec![(0.0, (4, 4)), (8.0, (3, 4)), (12.5, (4, 4))]);
        let bs = m.bars(0.0, 20.0);
        let got: Vec<(i64, f64, f64, (i32, i32))> =
            bs.iter().map(|b| (b.index, b.qn, b.len, b.sig)).collect();
        assert_eq!(
            got,
            vec![
                (0, 0.0, 4.0, (4, 4)),
                (1, 4.0, 4.0, (4, 4)),
                (2, 8.0, 3.0, (3, 4)),
                (3, 11.0, 1.5, (3, 4)),
                (4, 12.5, 4.0, (4, 4)),
                (5, 16.5, 4.0, (4, 4)),
            ]
        );
        assert_eq!(m.bar_at(13.0).index, 4);
        assert_eq!(m.bar_at(11.2).len, 1.5);
        // Des changements desordonnes, en double, ou nuls : remis en ordre.
        let m2 = Meter::new(vec![
            (8.0, (3, 4)),
            (0.0, (4, 4)),
            (8.0, (3, 4)),
            (4.0, (0, 4)),
        ]);
        assert_eq!(m2.changes, vec![(0.0, (4, 4)), (8.0, (3, 4))]);
    }
}
