//! Des matrices du reseau aux notes.
//!
//! Le decodage de Basic Pitch (note_creation.py, output_to_notes_polyphonic +
//! get_pitch_bends), porte a la lettre, avec les memes valeurs par defaut :
//!   1. les onsets sont completes par les MONTEES franches de la matrice des
//!      notes (infer_onsets) ;
//!   2. chaque pic d'onset au-dessus du seuil ouvre une note, du plus tard au
//!      plus tot, qui dure tant que la matrice des notes reste au-dessus du
//!      seuil de trame (11 trames de tolerance) ; ce qu'elle couvre est retire
//!      de l'energie restante, avec les deux demi-tons voisins ;
//!   3. « l'astuce melodia » : ce qui reste d'energie au-dessus du seuil
//!      devient des notes SANS onset, du plus fort au plus faible -- c'est ce
//!      qui rattrape un legato (hammer-on, pull-off) dont l'attaque ne se voit
//!      pas. Le plus fort se cherche dans un tas plutot qu'en rebalayant la
//!      matrice : meme choix (valeur, puis la plus petite place), sans le cout
//!      quadratique ;
//!   4. le bend de chaque note : la case de contour la plus forte, a +-25
//!      tiers de demi-ton, ponderee par une gaussienne (ecart-type 5), trame
//!      par trame.
//!
//! Ajout a Basic Pitch : `peak`, la force de l'onset qui a ouvert la note (0
//! pour une note de l'astuce melodia) : une note sans attaque est jouee liee.

use std::collections::BinaryHeap;

use crate::analyze::Posteriors;
use crate::model::{CONTOUR, HOP, MIDI_OFFSET, NOTES, SR};

#[derive(Clone, Copy, Debug)]
pub struct NoteParams {
    /// DEFAULT_ONSET_THRESHOLD
    pub onset_thresh: f32,
    /// DEFAULT_FRAME_THRESHOLD
    pub frame_thresh: f32,
    /// En trames (11 = 127,7 ms).
    pub min_len: usize,
    pub energy_tol: usize,
    pub infer_onsets: bool,
    pub melodia: bool,
    /// MIDI, bornes comprises.
    pub min_pitch: i32,
    pub max_pitch: i32,
}

impl Default for NoteParams {
    fn default() -> Self {
        NoteParams {
            onset_thresh: 0.5,
            frame_thresh: 0.3,
            min_len: 11,
            energy_tol: 11,
            infer_onsets: true,
            melodia: true,
            min_pitch: 21,
            max_pitch: 108,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Trames, [start, end).
    pub start: usize,
    pub end: usize,
    /// MIDI.
    pub pitch: i32,
    /// Moyenne de la matrice des notes, 0..1.
    pub amp: f32,
    /// Force de l'onset (0 : note melodia, jouee liee).
    pub peak: f32,
    /// Tiers de demi-ton, une valeur par trame.
    pub bends: Vec<i32>,
}

impl Note {
    /// Le debut en secondes.
    pub fn t0(&self) -> f64 {
        self.start as f64 * HOP as f64 / SR as f64
    }
    pub fn t1(&self) -> f64 {
        self.end as f64 * HOP as f64 / SR as f64
    }
}

/// np.round(ms / 1000 x 22050 / 256), arrondi au pair.
pub fn min_len_frames(ms: f64) -> usize {
    let ms = if ms >= 0.0 { ms } else { 0.0 };
    let x = ms / 1000.0 * SR as f64 / HOP as f64;
    let r = x.round();
    // L'arrondi bancaire de numpy : a egale distance, le pair.
    let r = if (x - x.trunc() - 0.5).abs() < 1e-12 && r % 2.0 != 0.0 {
        r - 1.0
    } else {
        r
    };
    r as usize
}

#[derive(PartialEq)]
struct Cell {
    v: f64,
    idx: usize,
}

impl Eq for Cell {}

impl PartialOrd for Cell {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Cell {
    /// Le tas sort le PLUS GRAND : la plus forte valeur, et a valeur egale la
    /// plus petite place (np.argmax rend la premiere).
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.v
            .partial_cmp(&o.v)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(o.idx.cmp(&self.idx))
    }
}

fn bends_for(p: &Posteriors, n: &mut Note) {
    const TOL: i32 = 25;
    let wl = (2 * TOL + 1) as usize;
    let mut g = [0f64; 51];
    for (i, v) in g.iter_mut().enumerate().take(wl) {
        let x = (i as f64 - TOL as f64) / 5.0;
        *v = (-0.5 * x * x).exp();
    }
    let fi = 3 * (n.pitch - MIDI_OFFSET);
    let fs = (fi - TOL).max(0);
    let fe = (CONTOUR as i32).min(fi + TOL + 1);
    let gs = (TOL - fi).max(0);
    let shift = TOL - (TOL - fi).max(0);
    n.bends.clear();
    for t in n.start..n.end {
        let row = &p.contour[t * CONTOUR..(t + 1) * CONTOUR];
        let (mut best, mut bv) = (0i32, -1f64);
        for j in 0..(fe - fs).max(0) {
            let v = row[(fs + j) as usize] as f64 * g[(gs + j) as usize];
            if v > bv {
                bv = v;
                best = j;
            }
        }
        n.bends.push(best - shift);
    }
}

pub fn make_notes(p: &Posteriors, prm: &NoteParams) -> Vec<Note> {
    let mut out = Vec::new();
    let fr = p.frames;
    if fr < 3 {
        return out;
    }
    let lo = (prm.min_pitch - MIDI_OFFSET).max(0) as usize;
    let hi = (prm.max_pitch - MIDI_OFFSET).min(NOTES as i32 - 1).max(0) as usize;
    // Les deux matrices, bornees en hauteur (constrain_frequency).
    let mut frames = p.note.clone();
    let mut onsets = p.onset.clone();
    for t in 0..fr {
        for f in 0..NOTES {
            if f < lo || f > hi {
                frames[t * NOTES + f] = 0.0;
                onsets[t * NOTES + f] = 0.0;
            }
        }
    }
    // 1. Onsets deduits des montees (get_infered_onsets, n_diff = 2).
    if prm.infer_onsets {
        let mut diff = vec![0f32; fr * NOTES];
        let (mut dmax, mut omax) = (0f32, 0f32);
        for &v in &onsets {
            omax = omax.max(v);
        }
        for t in 2..fr {
            for f in 0..NOTES {
                let x = frames[t * NOTES + f];
                let d1 = x - frames[(t - 1) * NOTES + f];
                let d2 = x - frames[(t - 2) * NOTES + f];
                let d = d1.min(d2).max(0.0);
                diff[t * NOTES + f] = d;
                dmax = dmax.max(d);
            }
        }
        if dmax > 0.0 {
            for (o, d) in onsets.iter_mut().zip(&diff) {
                *o = o.max(omax * d / dmax);
            }
        }
    }
    let mut rem: Vec<f64> = frames.iter().map(|&v| v as f64).collect();
    let clear3 = |rem: &mut Vec<f64>, t: usize, f: usize| {
        rem[t * NOTES + f] = 0.0;
        if f < NOTES - 1 {
            rem[t * NOTES + f + 1] = 0.0;
        }
        if f > 0 {
            rem[t * NOTES + f - 1] = 0.0;
        }
    };
    let mean = |a: usize, b: usize, f: usize| -> f64 {
        if b <= a {
            return 0.0;
        }
        let s: f64 = (a..b).map(|t| frames[t * NOTES + f] as f64).sum();
        s / (b - a) as f64
    };
    let ft = prm.frame_thresh as f64;

    // 2. Les pics d'onset, du plus tard au plus tot (np.where a l'envers).
    for t0 in (1..=fr - 2).rev() {
        for f in (0..NOTES).rev() {
            let v = onsets[t0 * NOTES + f];
            if v < prm.onset_thresh || v.is_nan() {
                continue;
            }
            if !(v > onsets[(t0 - 1) * NOTES + f] && v > onsets[(t0 + 1) * NOTES + f]) {
                continue;
            }
            let (mut i, mut k) = (t0 + 1, 0usize);
            while i < fr - 1 && k < prm.energy_tol {
                if rem[i * NOTES + f] < ft {
                    k += 1;
                } else {
                    k = 0;
                }
                i += 1;
            }
            i -= k;
            if i <= t0 || i - t0 <= prm.min_len {
                continue;
            }
            for t in t0..i {
                clear3(&mut rem, t, f);
            }
            out.push(Note {
                start: t0,
                end: i,
                pitch: f as i32 + MIDI_OFFSET,
                amp: mean(t0, i, f) as f32,
                peak: v,
                bends: Vec::new(),
            });
        }
    }

    // 3. L'astuce melodia : l'energie restante, de la plus forte a la plus
    //    faible.
    if prm.melodia {
        let mut heap = BinaryHeap::new();
        for (idx, &v) in rem.iter().enumerate() {
            if v > ft {
                heap.push(Cell { v, idx });
            }
        }
        while let Some(c) = heap.pop() {
            if rem[c.idx] != c.v {
                continue; // deja effacee
            }
            let (i_mid, f) = (c.idx / NOTES, c.idx % NOTES);
            rem[c.idx] = 0.0;
            let (mut i, mut k) = (i_mid + 1, 0usize);
            while i < fr - 1 && k < prm.energy_tol {
                if rem[i * NOTES + f] < ft {
                    k += 1;
                } else {
                    k = 0;
                }
                clear3(&mut rem, i, f);
                i += 1;
            }
            let i_end = i as i64 - 1 - k as i64;
            let (mut i, mut k) = (i_mid as i64 - 1, 0usize);
            while i > 0 && k < prm.energy_tol {
                if rem[i as usize * NOTES + f] < ft {
                    k += 1;
                } else {
                    k = 0;
                }
                clear3(&mut rem, i as usize, f);
                i -= 1;
            }
            let i_start = i + 1 + k as i64;
            if i_end - i_start <= prm.min_len as i64 || i_start < 0 {
                continue;
            }
            let (a, b) = (i_start as usize, i_end as usize);
            out.push(Note {
                start: a,
                end: b,
                pitch: f as i32 + MIDI_OFFSET,
                amp: mean(a, b, f) as f32,
                peak: 0.0,
                bends: Vec::new(),
            });
        }
    }

    // 4. Les bends.
    for n in &mut out {
        bends_for(p, n);
    }
    out
}
