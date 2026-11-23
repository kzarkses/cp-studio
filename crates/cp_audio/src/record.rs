//! L'enregistrement (A8) : la prise audio d'une entree de la carte son,
//! calee a l'echantillon sur la ligne du temps du moteur.
//!
//! Le crochet audio (passage « pre ») copie l'entree choisie dans un anneau
//! et note, pour chaque bloc, le frame du moteur ou il commence. Le fil
//! principal vide l'anneau dans les prises en cours. Sans verrou : le fil
//! audio n'ecrit que des atomiques.
//!
//! LA LATENCE. Ce que le musicien joue en entendant le frame F du moteur lui
//! arrive F + sortie plus tard (la latence de sortie), puis rentre par
//! l'entree encore entree plus tard : l'echantillon capture au frame G du
//! moteur appartient au temps G - (entree + sortie). Une prise retire cet
//! ecart, et tombe la ou elle a ete jouee.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};

/// L'anneau : 2^20 nombres, 524 288 frames stereo (11 s a 48 kHz) ; le fil
/// principal le vide a chaque battement.
const RN: usize = 1 << 20;
/// Les en-tetes de bloc gardes (bien plus que l'anneau n'en tient).
const HN: usize = 4096;

/// Ce que le crochet ecrit et que le fil principal lit.
pub struct RecRing {
    buf: Box<[AtomicU32]>,
    /// Nombres ecrits depuis le depart.
    w: AtomicUsize,
    /// Par bloc : son frame du moteur, la position d'ecriture a son debut et
    /// son nombre de frames (trois cases par bloc).
    heads: Box<[AtomicU64]>,
    /// Blocs ecrits depuis le depart.
    hn: AtomicUsize,
    /// Les entrees capturees (-1 : rien ; la seconde -1 : mono, dupliquee).
    ch: [AtomicI32; 2],
}

impl RecRing {
    /// Pour le journal : blocs ecrits, frame et taille du dernier.
    pub fn last_block(&self) -> (usize, i64, usize) {
        let hn = self.hn.load(Acquire);
        if hn == 0 {
            return (0, 0, 0);
        }
        let i = ((hn - 1) % HN) * 3;
        (
            hn,
            self.heads[i].load(Relaxed) as i64,
            self.heads[i + 2].load(Relaxed) as usize,
        )
    }

    pub fn new() -> Arc<RecRing> {
        Arc::new(RecRing {
            buf: (0..RN).map(|_| AtomicU32::new(0)).collect(),
            w: AtomicUsize::new(0),
            heads: (0..HN * 3).map(|_| AtomicU64::new(0)).collect(),
            hn: AtomicUsize::new(0),
            ch: [AtomicI32::new(-1), AtomicI32::new(-1)],
        })
    }

    pub fn inputs(&self) -> (i32, i32) {
        (self.ch[0].load(Relaxed), self.ch[1].load(Relaxed))
    }

    pub fn set_inputs(&self, c0: i32, c1: i32) {
        self.ch[1].store(c1, Relaxed);
        self.ch[0].store(c0, Relaxed);
    }

    /// Fil audio : un bloc de l'entree (gauche, et droite ou rien), qui
    /// commence au frame `frame` du moteur.
    pub fn push_block(&self, frame: i64, l: &[f64], r: Option<&[f64]>) {
        let n = l.len();
        let w0 = self.w.load(Relaxed);
        let mut w = w0;
        for (k, &x) in l.iter().enumerate() {
            let a = x as f32;
            let b = r.and_then(|r| r.get(k)).map_or(a, |&x| x as f32);
            self.buf[w % RN].store(a.to_bits(), Relaxed);
            self.buf[(w + 1) % RN].store(b.to_bits(), Relaxed);
            w = w.wrapping_add(2);
        }
        let h = self.hn.load(Relaxed);
        let i = (h % HN) * 3;
        self.heads[i].store(frame as u64, Relaxed);
        self.heads[i + 1].store(w0 as u64, Relaxed);
        self.heads[i + 2].store(n as u64, Relaxed);
        self.w.store(w, Release);
        self.hn.store(h.wrapping_add(1), Release);
    }
}

/// L'etat d'une prise.
#[derive(Clone, Debug, PartialEq)]
pub enum Take {
    /// En cours : les frames deja pris.
    Recording(usize),
    /// Finie : sa matiere (stereo entrelace), a ranger comme un clip.
    Done(Vec<f32>),
    Failed(String),
}

struct Rec {
    id: u32,
    /// Le temps du moteur ou la prise commence (et finit), latence retiree.
    start: i64,
    stop: Option<i64>,
    /// La latence entree + sortie, en frames.
    lat: i64,
    data: Vec<f32>,
    failed: Option<String>,
}

/// Le cote fil principal : les prises en cours, nourries de l'anneau.
pub struct Recorder {
    ring: Arc<RecRing>,
    /// Les blocs deja lus.
    hr: usize,
    recs: Vec<Rec>,
    next_id: u32,
}

impl Recorder {
    pub fn new(ring: Arc<RecRing>) -> Recorder {
        Recorder {
            hr: ring.hn.load(Acquire),
            ring,
            recs: Vec::new(),
            next_id: 1,
        }
    }

    pub fn ring(&self) -> &Arc<RecRing> {
        &self.ring
    }

    /// Une prise qui commence au temps `start` du moteur ; `lat` : la
    /// latence entree + sortie (frames).
    pub fn start(&mut self, start: i64, lat: i64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.recs.push(Rec {
            id,
            start,
            stop: None,
            lat: lat.max(0),
            data: Vec::new(),
            failed: None,
        });
        id
    }

    /// La prise s'arrete au temps `at` du moteur (exclu).
    pub fn stop(&mut self, id: u32, at: i64) {
        if let Some(r) = self.recs.iter_mut().find(|r| r.id == id) {
            r.stop = Some(at.max(r.start));
        }
    }

    pub fn active(&self) -> usize {
        self.recs.len()
    }

    /// Vide l'anneau dans les prises ; rend celles qui sont finies (ou en
    /// echec), retirees.
    pub fn drain(&mut self) -> Vec<(u32, Take)> {
        let ring = &self.ring;
        let hn = ring.hn.load(Acquire);
        let w_now = ring.w.load(Acquire);
        // Trop de retard : l'anneau a deja reecrit ce qu'on n'a pas lu.
        if hn.wrapping_sub(self.hr) > HN {
            for r in &mut self.recs {
                r.failed = Some("recording overrun (the main thread was too slow)".into());
            }
            self.hr = hn;
        }
        while self.hr != hn {
            let i = (self.hr % HN) * 3;
            let frame = ring.heads[i].load(Relaxed) as i64;
            let w0 = ring.heads[i + 1].load(Relaxed) as usize;
            let n = ring.heads[i + 2].load(Relaxed) as usize;
            self.hr = self.hr.wrapping_add(1);
            if w_now.wrapping_sub(w0) > RN {
                for r in &mut self.recs {
                    r.failed = Some("recording overrun".into());
                }
                continue;
            }
            for r in &mut self.recs {
                if r.failed.is_some() {
                    continue;
                }
                // Le temps de chaque frame du bloc, latence retiree.
                let t0 = frame - r.lat;
                let a = (r.start - t0).max(0) as usize;
                let b = match r.stop {
                    Some(s) => ((s - t0).max(0) as usize).min(n),
                    None => n,
                };
                for k in a..b {
                    let w = w0 + k * 2;
                    r.data.push(f32::from_bits(ring.buf[w % RN].load(Relaxed)));
                    r.data
                        .push(f32::from_bits(ring.buf[(w + 1) % RN].load(Relaxed)));
                }
            }
            // L'ecrivain a-t-il rattrape ce bloc PENDANT la copie (le fil
            // principal gele une dizaine de secondes) ? Alors ce qui vient
            // d'etre copie peut etre ecrase : la prise est perdue, et le dit.
            let w_after = ring.w.load(Acquire);
            if w_after.wrapping_sub(w0) > RN - 2 * n {
                for r in &mut self.recs {
                    r.failed
                        .get_or_insert_with(|| "recording overrun".to_string());
                }
            }
        }
        // Les prises finies : leur fin est passee (latence comprise).
        let last = if hn == 0 {
            i64::MIN
        } else {
            let i = ((hn - 1) % HN) * 3;
            ring.heads[i].load(Relaxed) as i64 + ring.heads[i + 2].load(Relaxed) as i64
        };
        let mut out = Vec::new();
        self.recs.retain_mut(|r| {
            if let Some(e) = r.failed.take() {
                out.push((r.id, Take::Failed(e)));
                return false;
            }
            match r.stop {
                // (Aucun bloc encore : `last` est au plus bas, jamais fini.)
                Some(s) if hn > 0 && last.saturating_sub(r.lat) >= s => {
                    out.push((r.id, Take::Done(std::mem::take(&mut r.data))));
                    false
                }
                _ => true,
            }
        });
        out
    }

    /// Les frames deja pris par une prise en cours.
    pub fn frames(&self, id: u32) -> Option<usize> {
        self.recs
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.data.len() / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une entree qui dit son propre temps : l'echantillon capture au frame
    /// G vaut G (gauche) et -G (droite), en milliemes.
    fn block(ring: &RecRing, frame: i64, n: usize) {
        let l: Vec<f64> = (0..n).map(|k| (frame + k as i64) as f64 / 1000.0).collect();
        let r: Vec<f64> = l.iter().map(|x| -x).collect();
        ring.push_block(frame, &l, Some(&r));
    }

    #[test]
    fn une_prise_tombe_a_sa_place_latence_retiree() {
        let ring = RecRing::new();
        let mut rec = Recorder::new(ring.clone());
        // 300 frames de latence entree + sortie ; la prise de 1000 a 5000.
        let id = rec.start(1000, 300);
        rec.stop(id, 5000);
        let mut t = 0i64;
        let mut done = None;
        while t < 8000 {
            block(&ring, t, 256);
            t += 256;
            for (i, take) in rec.drain() {
                assert_eq!(i, id);
                done = Some(take);
            }
        }
        let Some(Take::Done(d)) = done else {
            panic!("pas de prise : {done:?}");
        };
        // Exactement 4000 frames ; la premiere est celle capturee a 1300
        // (jouee a 1000), la derniere a 5299.
        assert_eq!(d.len(), 4000 * 2);
        assert!(
            (d[0] - 1.3).abs() < 1e-6 && (d[1] + 1.3).abs() < 1e-6,
            "{}",
            d[0]
        );
        assert!((d[d.len() - 2] - 5.299).abs() < 1e-5);
        // Aucun trou : chaque frame suit la precedente.
        for k in 1..4000 {
            assert!((d[k * 2] - d[(k - 1) * 2] - 0.001).abs() < 1e-5, "{k}");
        }
    }

    #[test]
    fn mono_deux_prises_et_retard_trop_grand() {
        let ring = RecRing::new();
        let mut rec = Recorder::new(ring.clone());
        let a = rec.start(0, 0);
        let b = rec.start(500, 0);
        rec.stop(a, 1000);
        rec.stop(b, 700);
        let l: Vec<f64> = (0..1024).map(|k| k as f64).collect();
        ring.push_block(0, &l, None);
        let mut got = rec.drain();
        got.sort_by_key(|x| x.0);
        assert_eq!(got.len(), 2);
        match (&got[0].1, &got[1].1) {
            (Take::Done(x), Take::Done(y)) => {
                assert_eq!(x.len(), 2000);
                assert_eq!(y.len(), 400);
                // Mono : la droite est la gauche.
                assert_eq!((y[0], y[1]), (500.0, 500.0));
            }
            o => panic!("{o:?}"),
        }
        // Le fil principal endormi plus longtemps que l'anneau : un echec
        // dit, pas une prise trouee.
        let c = rec.start(1024, 0);
        let mut f = 1024i64;
        for _ in 0..HN + 10 {
            ring.push_block(f, &l[..64], None);
            f += 64;
        }
        rec.stop(c, f);
        let got = rec.drain();
        assert!(matches!(got.first(), Some((_, Take::Failed(_)))), "{got:?}");
    }
}

#[cfg(test)]
mod more {
    use super::*;

    /// Aucun bloc encore : une prise arretee n'est pas finie (elle l'etait,
    /// vide, par un debordement).
    #[test]
    fn sans_bloc_une_prise_attend() {
        let ring = RecRing::new();
        let mut rec = Recorder::new(ring.clone());
        let id = rec.start(1000, 300);
        rec.stop(id, 2000);
        assert!(rec.drain().is_empty());
        assert_eq!(rec.frames(id), Some(0));
    }
}
