//! Un tampon qui se remplit pendant qu'on le lit : UN ecrivain ajoute a la
//! fin, des lecteurs voient le debut deja publie.
//!
//! C'est ce qui fait qu'un long fichier s'ecoute des que sa premiere seconde
//! est decodee : la voix lit le debut pendant que le fil de travail ecrit la
//! suite. Le debut publie ne change JAMAIS plus : le lire sans verrou est sur.
//! L'ecrivain est unique par construction (une seule poignee, `&mut self`), et
//! il n'ecrit que derriere la longueur publiee.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct GrowBuf {
    data: Box<[UnsafeCell<f32>]>,
    len: AtomicUsize,
    /// Plus personne ne veut la suite : l'ecrivain peut s'arreter.
    abandoned: AtomicBool,
    /// L'ecrivain a fini : la longueur publiee est la longueur finale.
    done: AtomicBool,
}

// SAFETY: les cases [0, len) ne sont plus jamais ecrites une fois publiees
// (Release sur `len` apres l'ecriture, Acquire a la lecture) ; les cases
// [len, cap) ne sont touchees que par l'unique ecrivain. Aucun acces ne se
// chevauche donc entre fils.
unsafe impl Sync for GrowBuf {}
// SAFETY: des f32 et des atomiques.
unsafe impl Send for GrowBuf {}

/// Un `Vec<f32>` vu comme des cellules, sans copie.
fn cells(v: Vec<f32>) -> Box<[UnsafeCell<f32>]> {
    let b: Box<[f32]> = v.into_boxed_slice();
    let p = Box::into_raw(b) as *mut [UnsafeCell<f32>];
    // SAFETY: `UnsafeCell<f32>` est `repr(transparent)` sur `f32` : meme
    // taille, meme alignement ; la boite change de type, pas de proprietaire.
    unsafe { Box::from_raw(p) }
}

impl GrowBuf {
    /// Un tampon deja plein (une matiere entiere en memoire).
    pub fn full(v: Vec<f32>) -> GrowBuf {
        let n = v.len();
        GrowBuf {
            data: cells(v),
            len: AtomicUsize::new(n),
            abandoned: AtomicBool::new(false),
            done: AtomicBool::new(true),
        }
    }

    /// Un tampon vide de capacite `cap`, et son unique ecrivain.
    pub fn growing(cap: usize) -> (Arc<GrowBuf>, GrowWriter) {
        // `vec![0.0; n]` demande des pages deja nulles au systeme : rien
        // n'est touche (ni ecrit) avant le decodage, meme pour 500 Mo.
        let b = Arc::new(GrowBuf {
            data: cells(vec![0.0f32; cap]),
            len: AtomicUsize::new(0),
            abandoned: AtomicBool::new(false),
            done: AtomicBool::new(false),
        });
        (b.clone(), GrowWriter { b })
    }

    /// Le debut publie.
    pub fn filled(&self) -> &[f32] {
        let n = self.len.load(Ordering::Acquire);
        // SAFETY: [0, n) est publie et ne sera plus jamais ecrit ; UnsafeCell<f32>
        // a la disposition d'un f32.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const f32, n) }
    }

    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    pub fn is_full(&self) -> bool {
        self.len.load(Ordering::Acquire) == self.data.len()
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    pub fn abandon(&self) {
        self.abandoned.store(true, Ordering::Relaxed);
    }

    pub fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Relaxed)
    }
}

/// L'unique ecrivain d'un [`GrowBuf`].
pub struct GrowWriter {
    b: Arc<GrowBuf>,
}

impl GrowWriter {
    /// Ajoute ce qui tient ; rend le nombre de valeurs ajoutees.
    pub fn push(&mut self, v: &[f32]) -> usize {
        let len = self.b.len.load(Ordering::Relaxed);
        let n = v.len().min(self.b.data.len() - len);
        for (i, &x) in v[..n].iter().enumerate() {
            // SAFETY: la case len + i est au-dela de la longueur publiee :
            // aucun lecteur ne la voit, et cet ecrivain est le seul.
            unsafe { *self.b.data[len + i].get() = x };
        }
        self.b.len.store(len + n, Ordering::Release);
        n
    }

    /// La longueur publiee.
    pub fn len(&self) -> usize {
        self.b.len.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn buf(&self) -> &Arc<GrowBuf> {
        &self.b
    }

    /// Plus rien ne viendra : la longueur publiee devient la longueur finale.
    pub fn finish(self) {
        self.b.done.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_lecteurs_voient_un_debut_qui_ne_change_plus() {
        let (b, mut w) = GrowBuf::growing(10);
        assert!(b.filled().is_empty());
        assert_eq!(w.push(&[1.0, 2.0, 3.0]), 3);
        let before: Vec<f32> = b.filled().to_vec();
        assert_eq!(w.push(&[4.0; 20]), 7, "pas plus que la capacite");
        assert_eq!(&b.filled()[..3], &before[..]);
        assert!(b.is_full());
    }

    #[test]
    fn un_lecteur_sur_un_autre_fil() {
        let (b, mut w) = GrowBuf::growing(1 << 16);
        let r = b.clone();
        let t = std::thread::spawn(move || {
            let mut seen = 0;
            while seen < 1 << 16 {
                let f = r.filled();
                for (i, &x) in f.iter().enumerate().skip(seen) {
                    assert_eq!(x, i as f32);
                }
                seen = f.len();
            }
        });
        let v: Vec<f32> = (0..1 << 16).map(|i| i as f32).collect();
        for c in v.chunks(1000) {
            w.push(c);
        }
        t.join().unwrap();
    }
}
