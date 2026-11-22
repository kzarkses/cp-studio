//! La clock MIDI d'un autre (ancien `cp_extclock`, ABI 3.9) : une machine
//! maitresse envoie 24 impulsions par noire (0xF8), Start, Continue, Stop et
//! la position (SPP). On en tire le TEMPO et la POSITION du maitre.
//!
//! Porte operation par operation de `reference/cp_native/src/core/
//! cp_extclock.cpp`, et compare a lui au bit pres par `cp_oracle`
//! (`tests/extclock.rs`). Ne pas « simplifier » une expression sans relancer
//! l'oracle.
//!
//! LE TEMPO EST UNE PENTE, PAS UN INTERVALLE. Une impulsion arrive tous les
//! 20 ms a 120 BPM, decalee d'une a deux ms par l'USB et le pilote : un
//! intervalle isole se trompe de 5 a 10 %. On ajuste une DROITE (moindres
//! carres) sur les instants des 384 dernieres impulsions (seize noires) : sa
//! pente est le tempo, a quelques milliemes de BPM, et elle debruite aussi la
//! phase. Un changement de tempo se voit sur une droite COURTE (la derniere
//! noire) qui s'ecarte de la longue plus que son bruit ne l'explique, douze
//! impulsions de suite.
//!
//! LA POSITION EST CELLE DE L'IMPULSION SUIVANTE : apres Start, le maitre est
//! a zero a la prochaine impulsion ; apres SPP + Continue, a la position du
//! SPP. Une impulsion perdue (un trou de 2 a 4 intervalles) est remplie ;
//! mais un trou de deux intervalles est aussi un tempo divise par deux : il
//! reste EN SUSPENS jusqu'a l'impulsion suivante, qui tranche.
//!
//! Le fil audio donne les messages dates en echantillons du moteur ; le fil
//! principal lit un instantane (seqlock).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

/// La fenetre longue : seize noires.
pub const RING: usize = 384;
/// La fenetre courte : une noire.
pub const SHORT: usize = 24;

pub const PRESENT: i32 = 1;
pub const RUNNING: i32 = 2;
pub const HAS_POS: i32 = 4;
pub const SETTLED: i32 = 8;
pub const WAITING: i32 = 16;

const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 400.0;

/// Ce que le fil principal lit, d'un coup.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snap {
    /// La pente longue (0 = jamais vu).
    pub bpm: f64,
    pub bpm_err: f64,
    /// La pente de la derniere noire.
    pub bpm_short: f64,
    /// L'ecart-type des impulsions autour de la droite.
    pub jitter_ms: f64,
    /// La largeur de la fenetre, en noires.
    pub beats: f64,
    /// L'instant AJUSTE de la derniere impulsion (echantillons).
    pub t_last: f64,
    /// La position du maitre a cette impulsion (impulsions).
    pub pos_last: f64,
    /// La position de la prochaine (figee a l'arret).
    pub pos_next: f64,
    /// Echantillons par impulsion.
    pub spt: f64,
    pub flags: i32,
    pub starts: u32,
    pub stops: u32,
    pub ticks: u32,
}

const PUB_FIELDS: usize = 14;

pub struct ExtClock {
    srate: f64,
    ring: [f64; RING],
    head: usize,
    count: usize,
    spt: f64,
    have_spt: bool,
    t_fit: f64,
    t_seen: f64,
    seen: bool,
    pos_next: i64,
    pos_last: f64,
    has_pos: bool,
    running: bool,
    waiting: bool,
    pend: bool,
    t_pend: f64,
    t_pend_from: f64,
    n_pend: i64,
    dev_run: i32,
    jitter: f64,
    bpm_short: f64,
    bpm_err: f64,
    starts: u32,
    stops: u32,
    ticks: u32,
    snap: Snap,
    seq: AtomicU32,
    publ: [AtomicU64; PUB_FIELDS],
}

impl Default for ExtClock {
    fn default() -> ExtClock {
        ExtClock::new()
    }
}

impl ExtClock {
    pub fn new() -> ExtClock {
        let srate = 48000.0;
        ExtClock {
            srate,
            ring: [0.0; RING],
            head: 0,
            count: 0,
            // Une graine a 120 BPM : la position entre Start et la deuxieme
            // impulsion s'extrapole avec QUELQUE CHOSE. Jamais publiee comme
            // un tempo (have_spt reste faux).
            spt: srate * 60.0 / (120.0 * 24.0),
            have_spt: false,
            t_fit: 0.0,
            t_seen: 0.0,
            seen: false,
            pos_next: 0,
            pos_last: 0.0,
            has_pos: false,
            running: false,
            waiting: false,
            pend: false,
            t_pend: 0.0,
            t_pend_from: 0.0,
            n_pend: 0,
            dev_run: 0,
            jitter: 0.0,
            bpm_short: 0.0,
            bpm_err: 0.0,
            starts: 0,
            stops: 0,
            ticks: 0,
            snap: Snap::default(),
            seq: AtomicU32::new(0),
            publ: std::array::from_fn(|_| AtomicU64::new(0f64.to_bits())),
        }
    }

    pub fn set_srate(&mut self, sr: f64) {
        // (NaN compris, comme `!(sr > 1.0)` en C++.)
        if sr.is_nan() || sr <= 1.0 || sr == self.srate {
            return;
        }
        // Les instants de la fenetre sont a l'ANCIEN taux ; la pente se
        // convertit.
        self.spt *= sr / self.srate;
        self.srate = sr;
        self.count = 0;
        self.dev_run = 0;
    }

    /// Oublie tout, sauf la pente (bonne graine pour la suite).
    pub fn reset(&mut self) {
        self.head = 0;
        self.count = 0;
        self.seen = false;
        self.pos_next = 0;
        self.pos_last = 0.0;
        self.has_pos = false;
        self.running = false;
        self.waiting = false;
        self.pend = false;
        self.dev_run = 0;
        self.jitter = 0.0;
        self.bpm_short = 0.0;
        self.bpm_err = 0.0;
    }

    fn push(&mut self, t: f64) {
        self.ring[self.head] = t;
        self.head = (self.head + 1) % RING;
        if self.count < RING {
            self.count += 1;
        }
    }

    /// Une impulsion entre dans la fenetre, avec `fill` impulsions perdues
    /// avant elle (depuis `from`).
    fn accept(&mut self, t: f64, fill: i64, from: f64) {
        if fill > 0 && self.count > 0 {
            let step = (t - from) / (fill + 1) as f64;
            for j in 1..=fill {
                self.push(from + step * j as f64);
            }
            if self.running && !self.waiting {
                self.pos_next += fill;
            }
        }
        self.push(t);
        if self.running {
            self.pos_last = self.pos_next as f64;
            self.pos_next += 1;
            self.waiting = false;
        }
        self.fit();
    }

    fn settle_pending(&mut self) {
        if !self.pend {
            return;
        }
        self.pend = false;
        self.accept(self.t_pend, 0, 0.0);
    }

    /// Une impulsion (0xF8) a l'instant `t` (echantillons du moteur).
    pub fn tick(&mut self, t: f64) {
        self.ticks = self.ticks.wrapping_add(1);
        let lo = self.srate * 60.0 / (MAX_BPM * 24.0);
        let hi = self.srate * 60.0 / (MIN_BPM * 24.0);
        // La meme impulsion vue deux fois (deux entrees sur le meme cable).
        if self.seen && (t - self.t_seen) < 0.3 * (if self.have_spt { self.spt } else { lo }) {
            return;
        }
        // Le trou en suspens se juge ici.
        if self.pend {
            self.pend = false;
            let k2 = (t - self.t_pend) / self.spt;
            let fill = if (k2 - 1.0).abs() < 0.35 {
                self.n_pend
            } else {
                0
            };
            self.accept(self.t_pend, fill, self.t_pend_from);
        }
        if self.seen {
            let dt = t - self.t_seen;
            if self.have_spt {
                let k = dt / self.spt;
                if k > 4.5 {
                    // Le maitre s'etait arrete : nouvelle fenetre.
                    self.count = 0;
                    self.dev_run = 0;
                } else if k > 1.5 {
                    let r = (k + 0.5).floor();
                    if (k - r).abs() < 0.15 {
                        self.pend = true;
                        self.n_pend = r as i64 - 1;
                        self.t_pend = t;
                        self.t_pend_from = self.t_seen;
                        self.t_seen = t;
                        return;
                    }
                }
            } else if dt > hi {
                self.count = 0;
            }
        }
        self.t_seen = t;
        self.seen = true;
        self.accept(t, 0, 0.0);
    }

    fn fit(&mut self) {
        let m = self.count;
        if m == 0 {
            return;
        }
        let newest = (self.head + RING - 1) % RING;
        if m == 1 {
            self.t_fit = self.ring[newest];
            return;
        }
        let lo = self.srate * 60.0 / (MAX_BPM * 24.0);
        let hi = self.srate * 60.0 / (MIN_BPM * 24.0);
        // La droite longue (instants relatifs a la plus ancienne).
        let oldest = (self.head + RING - m) % RING;
        let t0 = self.ring[oldest];
        let xbar = 0.5 * (m - 1) as f64;
        let (mut sy, mut sxy) = (0.0f64, 0.0f64);
        for k in 0..m {
            let y = self.ring[(oldest + k) % RING] - t0;
            sy += y;
            sxy += (k as f64 - xbar) * y;
        }
        let mf = m as f64;
        let sxx = mf * (mf * mf - 1.0) / 12.0;
        let b = sxy / sxx;
        if !(b >= lo && b <= hi) {
            // Une pente impossible : on repart de la derniere impulsion.
            self.count = 1;
            self.dev_run = 0;
            self.t_fit = self.ring[newest];
            return;
        }
        let ybar = sy / mf;
        self.spt = b;
        self.have_spt = true;
        self.t_fit = t0 + ybar + b * ((m - 1) as f64 - xbar);
        if m < 8 {
            self.bpm_err = 60.0 * self.srate / (24.0 * b);
            return;
        }
        // Le bruit autour de la droite.
        let a = ybar - b * xbar;
        let mut ss = 0.0f64;
        for k in 0..m {
            let r = (self.ring[(oldest + k) % RING] - t0) - (a + b * k as f64);
            ss += r * r;
        }
        self.jitter = (ss / (m - 2) as f64).sqrt();
        let bpm = 60.0 * self.srate / (24.0 * b);
        self.bpm_err = bpm * (self.jitter / sxx.sqrt()) / b;
        // La droite courte : la derniere noire.
        let ns = m.min(SHORT);
        let so = (self.head + RING - ns) % RING;
        let ts0 = self.ring[so];
        let xs = 0.5 * (ns - 1) as f64;
        let mut sxys = 0.0f64;
        for k in 0..ns {
            sxys += (k as f64 - xs) * (self.ring[(so + k) % RING] - ts0);
        }
        let nsf = ns as f64;
        let sxxs = nsf * (nsf * nsf - 1.0) / 12.0;
        let bs = sxys / sxxs;
        if bs > 0.0 {
            self.bpm_short = 60.0 * self.srate / (24.0 * bs);
        }
        // Un changement de tempo : la fenetre longue repart des douze
        // dernieres.
        if m >= 2 * SHORT && bs > 0.0 {
            let rel = (bs - b).abs() / b;
            let mut thr = 4.0 * self.jitter / (b * sxxs.sqrt());
            if thr < 0.002 {
                thr = 0.002;
            }
            if rel > thr {
                self.dev_run += 1;
                if self.dev_run >= 12 {
                    self.dev_run = 0;
                    self.count = 12;
                    self.fit();
                }
            } else {
                self.dev_run = 0;
            }
        }
    }

    pub fn start(&mut self, _t: f64) {
        self.settle_pending();
        self.starts = self.starts.wrapping_add(1);
        self.running = true;
        self.has_pos = true;
        self.waiting = true;
        self.pos_next = 0;
        self.pos_last = 0.0;
    }

    pub fn cont(&mut self, _t: f64) {
        self.settle_pending();
        self.starts = self.starts.wrapping_add(1);
        self.running = true;
        self.waiting = true;
    }

    pub fn stop(&mut self, _t: f64) {
        self.settle_pending();
        self.stops = self.stops.wrapping_add(1);
        self.running = false;
        self.waiting = false;
    }

    /// La position (Song Position Pointer), en doubles croches.
    pub fn spp(&mut self, sixteenths: i32) {
        self.settle_pending();
        self.pos_next = sixteenths.max(0) as i64 * 6;
        self.has_pos = true;
        if self.running {
            self.waiting = true;
        }
    }

    /// Un octet MIDI temps reel (0xF8, 0xFA, 0xFB, 0xFC) a l'instant `t`.
    pub fn realtime(&mut self, status: u8, t: f64) {
        match status {
            0xF8 => self.tick(t),
            0xFA => self.start(t),
            0xFB => self.cont(t),
            0xFC => self.stop(t),
            _ => {}
        }
    }

    /// Fin de bloc : la presence, puis la publication de l'instantane.
    pub fn end_block(&mut self, now: f64) {
        if self.pend && (now - self.t_pend) > 2.5 * self.spt {
            self.settle_pending();
        }
        let mut gone = 0.5 * self.srate;
        if gone < 4.0 * self.spt {
            gone = 4.0 * self.spt;
        }
        let present = self.seen && (now - self.t_seen) < gone;
        if !present {
            self.dev_run = 0;
        }
        let mut flags = 0;
        if present {
            flags |= PRESENT;
        }
        if self.running {
            flags |= RUNNING;
        }
        if self.has_pos {
            flags |= HAS_POS;
        }
        if self.count >= 2 * SHORT {
            flags |= SETTLED;
        }
        if self.waiting {
            flags |= WAITING;
        }
        self.snap = Snap {
            bpm: if self.have_spt {
                60.0 * self.srate / (24.0 * self.spt)
            } else {
                0.0
            },
            bpm_err: self.bpm_err,
            bpm_short: self.bpm_short,
            jitter_ms: self.jitter * 1000.0 / self.srate,
            beats: self.count as f64 / 24.0,
            t_last: self.t_fit,
            pos_last: self.pos_last,
            pos_next: self.pos_next as f64,
            spt: self.spt,
            flags,
            starts: self.starts,
            stops: self.stops,
            ticks: self.ticks,
        };
        let s = &self.snap;
        let v = [
            s.bpm,
            s.bpm_err,
            s.bpm_short,
            s.jitter_ms,
            s.beats,
            s.t_last,
            s.pos_last,
            s.pos_next,
            s.spt,
            s.flags as f64,
            s.starts as f64,
            s.stops as f64,
            s.ticks as f64,
            0.0,
        ];
        let q = self.seq.load(Ordering::Relaxed);
        self.seq.store(q.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        for (p, x) in self.publ.iter().zip(v) {
            p.store(x.to_bits(), Ordering::Relaxed);
        }
        self.seq.store(q.wrapping_add(2), Ordering::Release);
    }

    /// Fil principal : l'instantane publie (rien tant qu'aucune publication).
    pub fn read(&self) -> Option<Snap> {
        for _ in 0..16 {
            let s0 = self.seq.load(Ordering::Acquire);
            if s0 == 0 {
                return None;
            }
            if s0 & 1 != 0 {
                continue;
            }
            let g = |i: usize| f64::from_bits(self.publ[i].load(Ordering::Relaxed));
            let o = Snap {
                bpm: g(0),
                bpm_err: g(1),
                bpm_short: g(2),
                jitter_ms: g(3),
                beats: g(4),
                t_last: g(5),
                pos_last: g(6),
                pos_next: g(7),
                spt: g(8),
                flags: g(9) as i32,
                starts: g(10) as u32,
                stops: g(11) as u32,
                ticks: g(12) as u32,
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s0 {
                return Some(o);
            }
        }
        None
    }

    /// L'etat sans passer par la publication (les tests).
    pub fn live(&self) -> &Snap {
        &self.snap
    }
}

/// La position du maitre, en noires, a l'instant `t` (echantillons du
/// moteur).
pub fn qn_at(s: &Snap, t: f64) -> f64 {
    if s.flags & HAS_POS == 0 {
        return 0.0;
    }
    if s.flags & RUNNING == 0 || s.flags & WAITING != 0 || s.spt.is_nan() || s.spt <= 0.0 {
        return s.pos_next / 24.0;
    }
    // Sur la droite, a partir de la derniere impulsion ajustee ; bornee a
    // deux noires (un maitre disparu sans Stop).
    let f = ((t - s.t_last) / s.spt).clamp(-48.0, 48.0);
    (s.pos_last + f) / 24.0
}
