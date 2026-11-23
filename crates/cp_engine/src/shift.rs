//! L'etirement EN TEMPS REEL (A5) : une voix passe par un etireur (celui de
//! REAPER, elastique, dans REAPER) pour changer son tempo sans changer sa
//! hauteur, ou sa hauteur sans changer son tempo, pendant qu'elle joue.
//!
//! Le moteur ne connait pas REAPER : il recoit l'etireur comme un objet
//! [`RtShift`], fabrique et detruit sur le fil principal. Le fil audio ne
//! fait que l'appeler (sous `try_lock` : jamais d'attente) ; le dernier
//! detenteur est toujours le fil principal, qui le garde dans un vivier :
//! une voix qui le lache ne le libere jamais.
//!
//! L'etireur garde la sortie tant qu'il n'a pas assez de matiere (son
//! amorcage) : au depart, la voix lui donne de la matiere jusqu'a ce qu'il
//! rende le bloc demande. La premiere sortie est la premiere matiere,
//! a l'heure ; le premier bloc coute plus cher, c'est tout.

use std::sync::{Arc, Mutex};

/// Un etireur temps reel. `feed` et `take` ne doivent ni allouer ni
/// attendre : ils tournent dans le fil audio.
pub trait RtShift: Send {
    /// Taux et canaux, et remise a zero (fil principal, avant le pret).
    fn setup(&mut self, srate: f64, nch: usize);
    /// 1,5 = la matiere passe une fois et demie plus vite, la hauteur reste.
    fn set_tempo(&mut self, tempo: f64);
    /// Rapport de hauteur (2 = une octave au-dessus), la duree reste.
    fn set_shift(&mut self, shift: f64);
    fn reset(&mut self);
    fn feed(&mut self, input: &[f64], nch: usize);
    /// Plus de matiere : ce qui reste doit sortir.
    fn flush(&mut self);
    fn take(&mut self, out: &mut [f64], nch: usize) -> usize;
}

/// Frames donnees a l'etireur par morceau.
pub const FEED: usize = 256;
/// La reserve de sortie d'un etireur (frames).
const OUT: usize = 8192;

/// Un etireur prete a une voix, avec ses tampons (alloues sur le fil
/// principal).
pub struct ShiftState {
    pub sh: Box<dyn RtShift>,
    inbuf: Vec<f64>,
    out: Vec<f64>,
    /// Frames pretes dans `out`, a partir de `head`.
    out_n: usize,
    head: usize,
    nch: usize,
    /// La matiere est finie (une voix « une fois ») : l'etireur a ete vide.
    pub flushed: bool,
    /// Les reglages poses, pour ne les reposer qu'a un changement.
    tempo: f64,
    shift: f64,
}

impl std::fmt::Debug for ShiftState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShiftState")
            .field("out_n", &self.out_n)
            .field("flushed", &self.flushed)
            .field("tempo", &self.tempo)
            .field("shift", &self.shift)
            .finish()
    }
}

/// Ce que le moteur tient : partage avec le vivier du fil principal.
pub type ShiftSlot = Arc<Mutex<ShiftState>>;

impl ShiftState {
    /// Fil principal.
    pub fn new(mut sh: Box<dyn RtShift>, srate: f64) -> ShiftSlot {
        sh.setup(srate, 2);
        Arc::new(Mutex::new(ShiftState {
            sh,
            inbuf: vec![0.0; FEED * 2],
            out: vec![0.0; OUT * 2],
            out_n: 0,
            head: 0,
            nch: 2,
            flushed: false,
            tempo: 1.0,
            shift: 1.0,
        }))
    }

    /// Un nouveau depart : tout ce qui etait en route est oublie.
    pub fn restart(&mut self) {
        self.sh.reset();
        self.out_n = 0;
        self.head = 0;
        self.flushed = false;
    }

    /// Pose les reglages s'ils ont change.
    pub fn tune(&mut self, tempo: f64, shift: f64) {
        if tempo != self.tempo {
            self.sh.set_tempo(tempo);
            self.tempo = tempo;
        }
        if shift != self.shift {
            self.sh.set_shift(shift);
            self.shift = shift;
        }
    }

    /// Remplit `dst` (stereo entrelace, `frames` frames) depuis l'etireur ;
    /// `src(buf)` donne la matiere suivante (stereo entrelace, au plus
    /// `FEED` frames) et rend combien il en a mis (0 : la matiere est
    /// finie). Rend le nombre de frames rendus : moins que `frames` quand
    /// l'etireur s'est vide apres la fin de la matiere.
    pub fn produce(
        &mut self,
        dst: &mut [f32],
        frames: usize,
        mut src: impl FnMut(&mut [f64]) -> usize,
    ) -> usize {
        let nch = self.nch;
        let mut done = 0;
        // Deux bornes : jamais une boucle infinie si l'etireur ne rend rien,
        // et jamais plus de matiere qu'il n'en faut (le tempo, plus
        // l'amorcage) : un etireur qui ne rend rien ne fait pas avaler des
        // centaines de milliers de frames au fil audio.
        let mut guard = 0;
        let budget = frames as f64 * self.tempo.max(1.0) * 2.0 + 32768.0;
        let mut fed = 0usize;
        while done < frames && guard < 4096 {
            guard += 1;
            if self.out_n > 0 {
                let k = self.out_n.min(frames - done);
                for f in 0..k {
                    for c in 0..2 {
                        dst[(done + f) * 2 + c] =
                            self.out[(self.head + f) * nch + c.min(nch - 1)] as f32;
                    }
                }
                self.head += k;
                self.out_n -= k;
                done += k;
                continue;
            }
            // La reserve est vide : ce qui est pret, sinon de la matiere.
            self.head = 0;
            let got = self.sh.take(&mut self.out[..OUT * nch], nch);
            if got > 0 {
                self.out_n = got;
                continue;
            }
            if self.flushed {
                break;
            }
            if fed as f64 > budget {
                break;
            }
            let n = src(&mut self.inbuf[..FEED * 2]);
            fed += n;
            if n == 0 {
                self.sh.flush();
                self.flushed = true;
            } else {
                self.sh.feed(&self.inbuf[..n * 2], 2);
            }
        }
        done
    }
}

/// Un etireur d'imitation (les tests hors de REAPER) : la matiere lue a
/// `tempo` (interpolation lineaire), un retard fixe d'amorcage pendant
/// lequel il ne rend rien, la hauteur ignoree.
pub struct LinearRt {
    tempo: f64,
    prime: usize,
    input: Vec<f64>,
    base: f64,
    pos: f64,
    fed: usize,
    flushed: bool,
}

impl LinearRt {
    /// `prime` : les frames qu'il veut avant de rendre quoi que ce soit.
    pub fn new(prime: usize) -> LinearRt {
        LinearRt {
            tempo: 1.0,
            prime,
            input: Vec::with_capacity(1 << 20),
            base: 0.0,
            pos: 0.0,
            fed: 0,
            flushed: false,
        }
    }
}

impl RtShift for LinearRt {
    fn setup(&mut self, _srate: f64, _nch: usize) {
        self.reset();
    }
    fn set_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
    }
    fn set_shift(&mut self, _shift: f64) {}
    fn reset(&mut self) {
        self.input.clear();
        self.base = 0.0;
        self.pos = 0.0;
        self.fed = 0;
        self.flushed = false;
    }
    fn feed(&mut self, input: &[f64], _nch: usize) {
        // (Une imitation : elle peut allouer, elle n'est jamais dans REAPER.)
        self.input.extend_from_slice(input);
        self.fed += input.len() / 2;
    }
    fn flush(&mut self) {
        self.flushed = true;
    }
    fn take(&mut self, out: &mut [f64], _nch: usize) -> usize {
        if self.fed < self.prime && !self.flushed {
            return 0;
        }
        let avail = self.input.len() / 2;
        let mut k = 0;
        while k < out.len() / 2 {
            let x = self.pos - self.base;
            let i = x.floor() as usize;
            if i + 1 >= avail && !(self.flushed && i < avail) {
                break;
            }
            let t = x - i as f64;
            for c in 0..2 {
                let a = self.input[i * 2 + c];
                let b = self.input.get((i + 1) * 2 + c).copied().unwrap_or(a);
                out[k * 2 + c] = a + (b - a) * t;
            }
            self.pos += self.tempo;
            k += 1;
        }
        k
    }
}
