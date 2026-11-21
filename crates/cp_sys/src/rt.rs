//! Les primitives temps reel : ce que le fil audio partage avec les autres sans
//! verrou ni allocation.
//!
//! [`spsc`] : un anneau a UN producteur et UN consommateur, de capacite fixe,
//! alloue une fois. Le producteur et le consommateur sont deux poignees
//! distinctes, chacune unique (`&mut self`) : le type garantit qu'il n'y a jamais
//! deux producteurs ni deux consommateurs, ce que l'ancien C++ ne tenait que par
//! convention.
//!
//! Les barrieres sont celles de l'ancien `cp_ring.h` : acquire/release explicites,
//! correctes aussi sur ARM. Les curseurs vivent sur deux lignes de cache
//! distinctes (sans quoi producteur et consommateur se disputent la meme ligne a
//! chaque operation, ce qui se mesure sur les vieux CPU).

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
struct Padded<T>(T);

struct Ring<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    /// Prochain emplacement a ecrire (producteur).
    tail: Padded<AtomicUsize>,
    /// Prochain emplacement a lire (consommateur).
    head: Padded<AtomicUsize>,
}

// SAFETY: un emplacement n'est touche que par le producteur (avant que `tail`
// ne le publie) ou par le consommateur (apres l'avoir vu publie, avant que
// `head` ne le rende) ; les deux poignees sont uniques. Les valeurs traversent
// les fils, donc `T: Send` suffit.
unsafe impl<T: Send> Sync for Ring<T> {}
// SAFETY: idem.
unsafe impl<T: Send> Send for Ring<T> {}

impl<T> Drop for Ring<T> {
    fn drop(&mut self) {
        let head = *self.head.0.get_mut();
        let tail = *self.tail.0.get_mut();
        let mut i = head;
        while i != tail {
            // SAFETY: les emplacements entre head et tail sont initialises et
            // plus personne ne les tient (on est le dernier proprietaire).
            unsafe { self.slots[i & self.mask].get_mut().assume_init_drop() };
            i = i.wrapping_add(1);
        }
    }
}

/// Le cote qui ecrit.
pub struct Producer<T> {
    ring: Arc<Ring<T>>,
}

/// Le cote qui lit.
pub struct Consumer<T> {
    ring: Arc<Ring<T>>,
}

/// Cree un anneau d'au moins `capacity` places (arrondi a la puissance de deux
/// superieure). La seule allocation de sa vie.
pub fn spsc<T: Send>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let cap = capacity.max(1).next_power_of_two();
    let slots = (0..cap)
        .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
        .collect();
    let ring = Arc::new(Ring {
        slots,
        mask: cap - 1,
        tail: Padded(AtomicUsize::new(0)),
        head: Padded(AtomicUsize::new(0)),
    });
    (Producer { ring: ring.clone() }, Consumer { ring })
}

impl<T: Send> Producer<T> {
    /// Rend la valeur si l'anneau est plein.
    pub fn push(&mut self, v: T) -> Result<(), T> {
        let r = &*self.ring;
        let tail = r.tail.0.load(Ordering::Relaxed);
        let head = r.head.0.load(Ordering::Acquire);
        if tail.wrapping_sub(head) > r.mask {
            return Err(v);
        }
        // SAFETY: l'emplacement `tail` est libre (le consommateur l'a rendu,
        // `head` acquis ci-dessus le prouve) et seul ce producteur ecrit.
        unsafe { (*r.slots[tail & r.mask].get()).write(v) };
        r.tail.0.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Indicatif : jamais une decision.
    pub fn len(&self) -> usize {
        let r = &*self.ring;
        r.tail
            .0
            .load(Ordering::Relaxed)
            .wrapping_sub(r.head.0.load(Ordering::Relaxed))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.ring.mask + 1
    }
}

impl<T: Send> Consumer<T> {
    pub fn pop(&mut self) -> Option<T> {
        let r = &*self.ring;
        let head = r.head.0.load(Ordering::Relaxed);
        let tail = r.tail.0.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: l'emplacement `head` a ete publie par le producteur (`tail`
        // acquis) ; on le lit une seule fois avant de le rendre.
        let v = unsafe { (*r.slots[head & r.mask].get()).assume_init_read() };
        r.head.0.store(head.wrapping_add(1), Ordering::Release);
        Some(v)
    }

    /// Vide l'anneau (les valeurs sont detruites ici, sur le fil qui appelle).
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }

    pub fn len(&self) -> usize {
        let r = &*self.ring;
        r.tail
            .0
            .load(Ordering::Relaxed)
            .wrapping_sub(r.head.0.load(Ordering::Relaxed))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordre_plein_vide_et_tour_complet() {
        let (mut p, mut c) = spsc::<u32>(4);
        assert_eq!(p.capacity(), 4);
        for round in 0..10 {
            for i in 0..4 {
                assert!(p.push(round * 10 + i).is_ok());
            }
            assert_eq!(p.push(99), Err(99), "plein a 4");
            for i in 0..4 {
                assert_eq!(c.pop(), Some(round * 10 + i));
            }
            assert_eq!(c.pop(), None);
        }
    }

    #[test]
    fn rien_ne_fuit_ni_n_est_detruit_deux_fois() {
        let token = Arc::new(());
        {
            let (mut p, mut c) = spsc::<Arc<()>>(8);
            for _ in 0..6 {
                p.push(token.clone()).unwrap();
            }
            drop(c.pop());
            assert_eq!(Arc::strong_count(&token), 6);
            c.clear();
            assert_eq!(Arc::strong_count(&token), 1);
            for _ in 0..3 {
                p.push(token.clone()).unwrap();
            }
            assert_eq!(Arc::strong_count(&token), 4);
        }
        assert_eq!(Arc::strong_count(&token), 1, "l'anneau detruit ses restes");
    }

    #[test]
    fn deux_fils_un_million_de_valeurs_dans_l_ordre() {
        let (mut p, mut c) = spsc::<u64>(256);
        const N: u64 = 1_000_000;
        let t = std::thread::spawn(move || {
            let mut i = 0;
            while i < N {
                if p.push(i).is_ok() {
                    i += 1;
                } else {
                    std::hint::spin_loop();
                }
            }
        });
        let mut expect = 0;
        while expect < N {
            if let Some(v) = c.pop() {
                assert_eq!(v, expect);
                expect += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        t.join().unwrap();
        assert!(c.pop().is_none());
    }
}
