//! La matiere d'un long fichier, en PAGES : seules celles autour de la tete de
//! lecture sont en memoire (la lecture en flux, A2).
//!
//! Un fichier de deux heures fait 2,8 Go en stereo a 48 kHz : on ne le decode
//! pas en entier. Il est coupe en pages (une seconde chacune quand le taux est
//! entier) ; un fil de flux (`cp_audio::stream`) charge celles qui sont devant
//! la tete et lache celles qui sont loin. La voix lit la page si elle est la,
//! du silence sinon (compte : `misses`).
//!
//! Le fil audio ne libere jamais rien et n'attend jamais. Une page lachee est
//! d'abord DECROCHEE (son pointeur passe a nul), puis liberee par le fil de
//! flux seulement quand plus aucun lecteur n'est en train de lire : chaque
//! lecture se fait dans une [`PagedRead`], qui compte ses lecteurs. Le schema
//! (decrocher, puis lire le compte ; compter, puis lire le pointeur) est celui
//! de Dekker : les quatre acces sont `SeqCst`, et un lecteur qui tient un
//! pointeur est forcement compte quand on regarde.

use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

/// Une position en frames (comme `cp_engine::Frame`).
pub type Frame = i64;

/// Les tetes suivies : une par voix (huit voix d'un meme clip au plus avant
/// qu'elles se partagent une place), plus la demande d'un depart.
pub const HEADS: usize = 8;

pub struct Paged {
    /// Frames par page.
    page: usize,
    nch: usize,
    frames: Frame,
    slots: Box<[AtomicPtr<f32>]>,
    /// Les lectures en cours.
    readers: AtomicUsize,
    /// Les pages decrochees qui attendent qu'aucune lecture ne les tienne :
    /// (page, pointeur). Jamais touche par le fil audio.
    grave: Mutex<Vec<(usize, usize)>>,
    /// Les tetes : la derniere frame lue par chaque voix (-1 : aucune), et
    /// combien de fois elle a ete posee (le flux voit ainsi qu'elle vit). La
    /// derniere place est la demande d'un depart ou d'un saut.
    heads: [AtomicI64; HEADS + 1],
    stamps: [AtomicU64; HEADS + 1],
    /// Les blocs qui ont lu une page absente (du silence a la place).
    misses: AtomicU64,
    resident: AtomicUsize,
    /// Plus personne ne veut l'afficher (le clip est retire).
    abandoned: AtomicBool,
}

impl std::fmt::Debug for Paged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paged")
            .field("page", &self.page)
            .field("nch", &self.nch)
            .field("frames", &self.frames)
            .field("resident", &self.resident())
            .finish()
    }
}

impl Paged {
    /// `frames` frames de `nch` canaux, en pages de `page` frames.
    pub fn new(frames: Frame, nch: usize, page: usize) -> Paged {
        let page = page.max(1);
        let n = (frames.max(0) as usize).div_ceil(page);
        Paged {
            page,
            nch: nch.max(1),
            frames: frames.max(0),
            slots: (0..n)
                .map(|_| AtomicPtr::new(std::ptr::null_mut()))
                .collect(),
            readers: AtomicUsize::new(0),
            grave: Mutex::new(Vec::new()),
            heads: std::array::from_fn(|_| AtomicI64::new(-1)),
            stamps: std::array::from_fn(|_| AtomicU64::new(0)),
            misses: AtomicU64::new(0),
            resident: AtomicUsize::new(0),
            abandoned: AtomicBool::new(false),
        }
    }

    pub fn page_frames(&self) -> usize {
        self.page
    }
    pub fn pages(&self) -> usize {
        self.slots.len()
    }
    pub fn frames(&self) -> Frame {
        self.frames
    }
    pub fn nch(&self) -> usize {
        self.nch
    }
    /// La page qui tient la frame `f`.
    pub fn page_of(&self, f: Frame) -> usize {
        (f.max(0) as usize / self.page).min(self.slots.len().saturating_sub(1))
    }
    /// Les echantillons (entrelaces) de la page `k`.
    pub fn page_len(&self, k: usize) -> usize {
        let a = k * self.page;
        let b = ((k + 1) * self.page).min(self.frames as usize);
        b.saturating_sub(a) * self.nch
    }
    pub fn has(&self, k: usize) -> bool {
        self.slots
            .get(k)
            .is_some_and(|s| !s.load(Ordering::Acquire).is_null())
    }
    pub fn resident(&self) -> usize {
        self.resident.load(Ordering::Relaxed)
    }
    pub fn bytes(&self) -> usize {
        self.resident() * self.page * self.nch * size_of::<f32>()
    }
    /// Chaque tete : (frame, nombre de poses). Une tete jamais posee vaut -1.
    pub fn heads(&self) -> [(Frame, u64); HEADS + 1] {
        std::array::from_fn(|i| {
            (
                self.heads[i].load(Ordering::Relaxed),
                self.stamps[i].load(Ordering::Relaxed),
            )
        })
    }
    /// Demander une place avant d'y jouer (un depart, un saut).
    pub fn hint(&self, f: Frame) {
        self.heads[HEADS].store(f.clamp(0, self.frames), Ordering::Relaxed);
        self.stamps[HEADS].fetch_add(1, Ordering::Relaxed);
    }
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }
    pub fn abandon(&self) {
        self.abandoned.store(true, Ordering::Relaxed);
    }
    pub fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Relaxed)
    }

    /// Une lecture : tant qu'elle vit, aucune page qu'elle voit n'est liberee.
    pub fn read(&self) -> PagedRead<'_> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        PagedRead {
            p: self,
            span: self.page * self.nch,
            last: Cell::new((usize::MAX, std::ptr::null())),
            missed: Cell::new(false),
        }
    }

    // --- le fil de flux (un seul ecrivain)

    /// Pose la page `k` (sa longueur doit etre `page_len(k)`). Rend faux si
    /// elle y etait deja ou si la longueur ne va pas.
    pub fn fill(&self, k: usize, data: Vec<f32>) -> bool {
        if k >= self.slots.len() || data.len() != self.page_len(k) || self.has(k) {
            return false;
        }
        let b: Box<[f32]> = data.into_boxed_slice();
        let ptr = Box::into_raw(b) as *mut f32;
        self.slots[k].store(ptr, Ordering::SeqCst);
        self.resident.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Decroche la page `k` ; elle sera liberee par [`Self::reclaim`].
    pub fn evict(&self, k: usize) {
        let Some(s) = self.slots.get(k) else { return };
        let old = s.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !old.is_null() {
            self.resident.fetch_sub(1, Ordering::Relaxed);
            if let Ok(mut g) = self.grave.lock() {
                g.push((k, old as usize));
            }
        }
    }

    /// Libere les pages decrochees si aucune lecture n'est en cours. Rend le
    /// nombre de pages liberees.
    pub fn reclaim(&self) -> usize {
        let Ok(mut g) = self.grave.lock() else {
            return 0;
        };
        if g.is_empty() || self.readers.load(Ordering::SeqCst) != 0 {
            return 0;
        }
        let n = g.len();
        for (k, ptr) in g.drain(..) {
            // SAFETY: decroche avant la lecture du compte (SeqCst), et aucune
            // lecture en cours : plus personne ne tient ce pointeur.
            unsafe { free_page(ptr as *mut f32, self.page_len(k)) };
        }
        n
    }

    /// Les pages en memoire.
    pub fn resident_pages(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.slots.len()).filter(|&k| self.has(k))
    }
}

/// # Safety
/// `ptr` vient de `Box::into_raw` d'une boite de `len` f32, et plus personne
/// ne le lit.
unsafe fn free_page(ptr: *mut f32, len: usize) {
    // SAFETY: voir la fonction.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

impl Drop for Paged {
    fn drop(&mut self) {
        // Plus aucune reference : ni lecture, ni flux.
        for (k, s) in self.slots.iter().enumerate() {
            let p = s.swap(std::ptr::null_mut(), Ordering::SeqCst);
            if !p.is_null() {
                // SAFETY: `&mut self` : plus personne ne lit.
                unsafe { free_page(p, self.page_len(k)) };
            }
        }
        let grave: Vec<(usize, usize)> = match self.grave.get_mut() {
            Ok(g) => std::mem::take(g),
            Err(_) => Vec::new(),
        };
        for (k, ptr) in grave {
            // SAFETY: idem.
            unsafe { free_page(ptr as *mut f32, self.page_len(k)) };
        }
    }
}

/// Une lecture en cours (un bloc de voix, une requete de pics).
pub struct PagedRead<'a> {
    p: &'a Paged,
    /// Echantillons par page.
    span: usize,
    /// La derniere page lue : son numero et son pointeur.
    last: Cell<(usize, *const f32)>,
    missed: Cell<bool>,
}

impl PagedRead<'_> {
    /// L'echantillon d'index `s` (entrelace, comme `samples()[s]`) ; 0 si sa
    /// page n'est pas la.
    #[inline]
    pub fn sample(&self, s: usize) -> f32 {
        let k = s / self.span;
        let (lk, lp) = self.last.get();
        let ptr = if lk == k {
            lp
        } else {
            let Some(slot) = self.p.slots.get(k) else {
                return 0.0;
            };
            let ptr = slot.load(Ordering::SeqCst) as *const f32;
            if ptr.is_null() {
                self.missed.set(true);
                return 0.0;
            }
            self.last.set((k, ptr));
            ptr
        };
        let off = s - k * self.span;
        if off >= self.p.page_len(k) {
            return 0.0;
        }
        // SAFETY: la page est posee (non nulle) et comptee par cette lecture :
        // elle ne sera pas liberee avant sa fin ; `off` est dans sa longueur.
        unsafe { *ptr.add(off) }
    }

    /// La page de la frame `f` est-elle lisible ?
    pub fn has_frame(&self, f: Frame) -> bool {
        self.p.has(self.p.page_of(f))
    }

    /// Fin d'un bloc de voix : la tete lue, pour le flux. `voice` distingue
    /// les voix (deux voix d'un meme clip, un fondu pendant un saut).
    pub fn played(&self, voice: usize, head: Frame) {
        let i = voice % HEADS;
        self.p.heads[i].store(head.clamp(0, self.p.frames), Ordering::Relaxed);
        self.p.stamps[i].fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for PagedRead<'_> {
    fn drop(&mut self) {
        if self.missed.get() {
            self.p.misses.fetch_add(1, Ordering::Relaxed);
        }
        self.p.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(p: &Paged, k: usize) -> Vec<f32> {
        (0..p.page_len(k)).map(|i| (k * 1000 + i) as f32).collect()
    }

    #[test]
    fn les_pages_se_lisent_et_le_reste_est_silence() {
        let p = Paged::new(2500, 2, 1000);
        assert_eq!(p.pages(), 3);
        assert_eq!(p.page_len(2), 1000);
        assert!(p.fill(0, page(&p, 0)));
        assert!(p.fill(2, page(&p, 2)));
        assert!(!p.fill(2, page(&p, 2)), "deja la");
        assert!(!p.fill(1, vec![0.0; 3]), "mauvaise longueur");
        {
            let r = p.read();
            assert_eq!(r.sample(0), 0.0);
            assert_eq!(r.sample(5), 5.0);
            assert_eq!(r.sample(2000), 0.0, "page 1 absente");
            assert_eq!(r.sample(4000 + 7), 2007.0);
            assert_eq!(r.sample(4999), 2999.0);
            r.played(3, 1234);
        }
        assert_eq!(p.misses(), 1);
        assert_eq!(p.heads()[3], (1234, 1));
        p.hint(99);
        assert_eq!(p.heads()[HEADS], (99, 1));
        assert_eq!(p.resident(), 2);
    }

    #[test]
    fn une_page_lue_n_est_pas_liberee() {
        let p = Paged::new(3000, 1, 1000);
        p.fill(1, page(&p, 1));
        let r = p.read();
        assert_eq!(r.sample(1500), 1500.0);
        p.evict(1);
        assert!(!p.has(1));
        assert_eq!(p.reclaim(), 0, "une lecture est en cours");
        // Le pointeur deja vu reste lisible jusqu'a la fin de la lecture.
        assert_eq!(r.sample(1501), 1501.0);
        drop(r);
        assert_eq!(p.reclaim(), 1);
        assert_eq!(p.read().sample(1500), 0.0);
    }

    #[test]
    fn deux_fils_sans_accident() {
        // Un lecteur qui lit sans arret pendant qu'un ecrivain pose et
        // decroche les pages : jamais une valeur fausse, jamais un plantage.
        let p = std::sync::Arc::new(Paged::new(64 * 256, 1, 256));
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let (p2, s2) = (p.clone(), stop.clone());
        let reader = std::thread::spawn(move || {
            let mut n = 0u64;
            while !s2.load(Ordering::Relaxed) {
                let r = p2.read();
                for s in (0..64 * 256).step_by(37) {
                    let v = r.sample(s);
                    assert!(v == 0.0 || v == s as f32, "{s} -> {v}");
                }
                n += 1;
            }
            n
        });
        for round in 0..2000 {
            let k = round % 64;
            let v: Vec<f32> = (0..256).map(|i| (k * 256 + i) as f32).collect();
            p.fill(k, v);
            p.evict((k + 32) % 64);
            p.reclaim();
        }
        stop.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0);
        while p.reclaim() > 0 {}
    }
}
