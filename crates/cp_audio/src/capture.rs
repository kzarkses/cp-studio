//! L'entree de la carte son, pour l'accordeur : le crochet audio copie
//! l'entree demandee dans un anneau, le fil principal en lit les derniers
//! echantillons. Sans verrou : le fil audio ne fait qu'ecrire des atomiques.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

use cp_sys::audio::AudioHook;

const N: usize = 1 << 15;

#[allow(clippy::declare_interior_mutable_const)]
const ZERO: AtomicU32 = AtomicU32::new(0);
static RING: [AtomicU32; N] = [ZERO; N];
/// Le nombre d'echantillons ecrits depuis le depart.
static WPOS: AtomicUsize = AtomicUsize::new(0);
/// Les entrees capturees (-1 : rien).
static CH0: AtomicI32 = AtomicI32::new(-1);
static CH1: AtomicI32 = AtomicI32::new(-1);

/// Dans le crochet (passage « pre ») : copie l'entree demandee.
pub(crate) fn on_block(frames: i32) {
    let c0 = CH0.load(Ordering::Relaxed);
    if c0 < 0 {
        return;
    }
    let Some(a) = AudioHook::input(c0, frames) else {
        return;
    };
    let b = AudioHook::input(CH1.load(Ordering::Relaxed), frames);
    let mut w = WPOS.load(Ordering::Relaxed);
    for (k, &x) in a.iter().enumerate() {
        let v = match b {
            Some(b) => (x + b[k]) * 0.5,
            None => x,
        } as f32;
        RING[w % N].store(v.to_bits(), Ordering::Relaxed);
        w = w.wrapping_add(1);
    }
    WPOS.store(w, Ordering::Release);
}

/// Choisit l'entree (`ch0` < 0 : arrete) et rend ses derniers echantillons
/// dans `out` ; rend combien. Un changement d'entree repart de zero.
pub fn read(ch0: i32, ch1: i32, out: &mut [f32]) -> usize {
    let (o0, o1) = (CH0.load(Ordering::Relaxed), CH1.load(Ordering::Relaxed));
    if o0 != ch0 || o1 != ch1 {
        CH1.store(ch1, Ordering::Relaxed);
        CH0.store(ch0, Ordering::Relaxed);
        WPOS.store(0, Ordering::Release);
        return 0;
    }
    if ch0 < 0 {
        return 0;
    }
    let w = WPOS.load(Ordering::Acquire);
    let n = out.len().min(w).min(N - 4096);
    let start = w - n;
    for (k, o) in out[..n].iter_mut().enumerate() {
        *o = f32::from_bits(RING[(start + k) % N].load(Ordering::Relaxed));
    }
    n
}
