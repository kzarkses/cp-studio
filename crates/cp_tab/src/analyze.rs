//! Un fichier entier par le reseau, et ce qu'on en garde.
//!
//! LE DECOUPAGE N'EST PAS CELUI DE BASIC PITCH, ET C'EST VOULU. Basic Pitch
//! avance de 36 164 echantillons (141,27 trames) et garde 142 trames par
//! fenetre : ses trames ne tombent pas sur une grille reguliere, et il recolle
//! les temps avec un decalage empirique. Ici on avance de 142 trames EXACTEMENT
//! (36 352 echantillons) et on garde les trames 15..156 de chaque fenetre : la
//! trame globale g est centree sur l'echantillon g x 256, sans correction. Le
//! contexte de part et d'autre d'une trame gardee reste de 15 trames au moins,
//! comme chez Basic Pitch.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::model::{CONTOUR, HOP, NOTES, WIN_FRAMES, WIN_SAMPLES, Work, run_window};

pub const KEEP_FROM: usize = 15;
pub const KEEP_FRAMES: usize = 142;
pub const WIN_HOP: usize = KEEP_FRAMES * HOP;

/// Les trois matrices du reseau sur tout le fichier. Trame g = echantillon
/// g x 256 (22 050 Hz).
#[derive(Clone, Debug, Default)]
pub struct Posteriors {
    pub frames: usize,
    /// frames x 88
    pub note: Vec<f32>,
    /// frames x 88
    pub onset: Vec<f32>,
    /// frames x 264
    pub contour: Vec<f32>,
}

pub fn window_count(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let frames = n / HOP + 1;
    frames.div_ceil(KEEP_FRAMES)
}

/// Ou en est une analyse (lu depuis un autre fil).
#[derive(Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    /// Mis a vrai par l'appelant : l'analyse s'arrete a la fenetre suivante.
    pub cancel: AtomicBool,
}

/// Analyse `audio` (mono, 22 050 Hz) sur `threads` fils (0 : autant que de
/// coeurs). `None` si l'analyse a ete annulee.
pub fn analyze(audio: &[f32], threads: usize, progress: &Progress) -> Option<Posteriors> {
    let n = audio.len();
    let frames = if n == 0 { 0 } else { n / HOP + 1 };
    let nwin = window_count(n);
    progress.total.store(nwin, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let mut out = Posteriors {
        frames,
        note: vec![0.0; frames * NOTES],
        onset: vec![0.0; frames * NOTES],
        contour: vec![0.0; frames * CONTOUR],
    };
    if nwin == 0 {
        return Some(out);
    }
    let threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        threads
    }
    .clamp(1, nwin);
    let next = AtomicUsize::new(0);
    // Chaque fil rend ses fenetres ; l'assemblage se fait apres.
    type Done = (usize, Vec<f32>, Vec<f32>, Vec<f32>);
    let worker = || -> Vec<Done> {
        let mut w = Work::new();
        let mut input = vec![0f32; WIN_SAMPLES];
        let mut mine = Vec::new();
        loop {
            if progress.cancel.load(Ordering::Relaxed) {
                break;
            }
            let win = next.fetch_add(1, Ordering::Relaxed);
            if win >= nwin {
                break;
            }
            // Fenetre win : echantillons [win x 36352 - 15 x 256, + 43844).
            let s0 = (win * WIN_HOP) as i64 - (KEEP_FROM * HOP) as i64;
            for (i, v) in input.iter_mut().enumerate() {
                let j = s0 + i as i64;
                *v = if j >= 0 && (j as usize) < n {
                    audio[j as usize]
                } else {
                    0.0
                };
            }
            let mut note = vec![0f32; WIN_FRAMES * NOTES];
            let mut onset = vec![0f32; WIN_FRAMES * NOTES];
            let mut contour = vec![0f32; WIN_FRAMES * CONTOUR];
            run_window(&mut w, &input, &mut note, &mut onset, &mut contour);
            mine.push((win, note, onset, contour));
            progress.done.fetch_add(1, Ordering::Relaxed);
        }
        mine
    };
    let results: Vec<Done> = std::thread::scope(|s| {
        let handles: Vec<_> = (1..threads).map(|_| s.spawn(worker)).collect();
        let mut all = worker();
        for h in handles {
            if let Ok(v) = h.join() {
                all.extend(v);
            }
        }
        all
    });
    if progress.cancel.load(Ordering::Relaxed) {
        return None;
    }
    for (win, note, onset, contour) in results {
        for j in 0..KEEP_FRAMES {
            let g = win * KEEP_FRAMES + j;
            if g >= frames {
                break;
            }
            let src = KEEP_FROM + j;
            out.note[g * NOTES..(g + 1) * NOTES]
                .copy_from_slice(&note[src * NOTES..(src + 1) * NOTES]);
            out.onset[g * NOTES..(g + 1) * NOTES]
                .copy_from_slice(&onset[src * NOTES..(src + 1) * NOTES]);
            out.contour[g * CONTOUR..(g + 1) * CONTOUR]
                .copy_from_slice(&contour[src * CONTOUR..(src + 1) * CONTOUR]);
        }
    }
    Some(out)
}
