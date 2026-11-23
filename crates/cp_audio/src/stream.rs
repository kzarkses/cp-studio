//! Le flux disque (A2) : un long fichier s'ecoute sans etre decode en entier.
//!
//! Un fil a lui (`cp_stream`) suit la tete de chaque clip en flux
//! ([`cp_sys::paged::Paged`]) : il charge les pages devant elle (dans le sens
//! de lecture), lache celles qui sont loin, et garde toujours le debut du
//! fichier (un depart du debut est immediat). Quand aucune page n'est
//! urgente, il fait un passage sur tout le fichier pour les pics (la forme
//! d'onde se dessine a tout zoom), publies en cours de route.
//!
//! Comme pour le decodage, la source est OUVERTE et DETRUITE sur le fil
//! principal ; ce fil ne fait que la lire.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cp_sys::paged::{HEADS, Paged};

use crate::decode::{Reader, Shape};
use crate::peaks::{PeakBuilder, Peaks};

/// Devant la tete : charge ; derriere : garde un peu (un retour en arriere,
/// la lecture a l'envers). Au-dela de la garde : lache. En secondes.
pub const AHEAD_S: f64 = 10.0;
pub const BEHIND_S: f64 = 2.0;
const KEEP_AHEAD_S: f64 = 20.0;
const KEEP_BEHIND_S: f64 = 8.0;
/// Le debut, toujours la.
pub const PIN_S: f64 = 3.0;
/// Une tete sans nouvelles depuis une seconde ne fait plus charger (sa voix
/// est partie) ; ses pages restent 30 s (on rejoue souvent au meme endroit).
const LIVE_S: f64 = 1.0;
const RECENT_S: f64 = 30.0;

/// Un clip a suivre.
pub struct StreamJob {
    /// Le clip (sa poignee brute), pour les messages.
    pub key: u32,
    pub src: Box<dyn Reader>,
    pub shape: Shape,
    pub pages: Arc<Paged>,
}

/// Ce que le fil dit au fil principal.
pub enum SMsg {
    /// Le debut est la : on peut jouer.
    Ready { key: u32 },
    /// Les pics de ce qui a ete lu (tout, si `done`).
    Peaks {
        key: u32,
        peaks: Arc<Peaks>,
        done: bool,
    },
    /// Le clip est parti (plus personne ne le tient) : la source revient
    /// pour etre detruite.
    Gone { key: u32, src: Box<dyn Reader> },
    /// Une lecture a echoue : rien n'est venu pour une page.
    Error { key: u32, error: String },
}

enum Cmd {
    Add(StreamJob),
    Wake,
}

/// Une tete, telle que le flux l'a vue.
#[derive(Clone, Copy)]
struct Seen {
    pos: i64,
    stamp: u64,
    when: Instant,
    /// Elle recule (lecture a l'envers).
    back: bool,
}

struct Live {
    job: StreamJob,
    heads: [Seen; HEADS + 1],
    ready_sent: bool,
    pass: Option<PeakBuilder>,
    last_snap: Instant,
    buf: Vec<f64>,
    failed: bool,
}

impl Live {
    fn pages_of(&self, s: f64) -> usize {
        ((s * self.job.shape.srate) / self.job.pages.page_frames() as f64).ceil() as usize
    }

    /// Les nouvelles des tetes : qui vit, dans quel sens.
    fn look(&mut self) {
        let now = Instant::now();
        for (seen, (pos, stamp)) in self.heads.iter_mut().zip(self.job.pages.heads()) {
            if stamp != seen.stamp {
                if seen.pos >= 0 && pos != seen.pos {
                    seen.back = pos < seen.pos;
                }
                *seen = Seen {
                    pos,
                    stamp,
                    when: now,
                    back: seen.back,
                };
            }
        }
    }

    /// Les tetes vues depuis moins de `s` secondes.
    fn heads_within(&self, s: f64) -> impl Iterator<Item = &Seen> {
        self.heads
            .iter()
            .filter(move |h| h.pos >= 0 && h.when.elapsed().as_secs_f64() < s)
    }

    /// La page manquante la plus urgente et sa distance (en pages) a la tete
    /// la plus proche.
    fn wanted(&self) -> Option<(usize, usize)> {
        let p = &self.job.pages;
        let n = p.pages();
        if n == 0 || self.failed {
            return None;
        }
        // Le debut d'abord, tant qu'il n'est pas la.
        let pin = self.pages_of(PIN_S).clamp(1, n);
        if let Some(k) = (0..pin).find(|&k| !p.has(k)) {
            return Some((k, 0));
        }
        let (fwd0, bwd0) = (self.pages_of(AHEAD_S), self.pages_of(BEHIND_S));
        let mut best: Option<(usize, usize)> = None;
        for h in self.heads_within(LIVE_S) {
            let hk = p.page_of(h.pos);
            let (fwd, bwd) = if h.back { (bwd0, fwd0) } else { (fwd0, bwd0) };
            // Devant (dans le sens de lecture), puis derriere, du plus pres
            // au plus loin.
            for d in 0..=fwd.max(bwd) {
                if best.is_some_and(|b| b.1 <= d) {
                    break;
                }
                let ahead = if h.back {
                    hk.checked_sub(d)
                } else {
                    Some(hk + d)
                };
                let behind = if h.back {
                    Some(hk + d)
                } else {
                    hk.checked_sub(d)
                };
                let hit = (d <= fwd)
                    .then_some(ahead)
                    .flatten()
                    .filter(|&k| k < n && !p.has(k))
                    .or_else(|| {
                        (d <= bwd && d > 0)
                            .then_some(behind)
                            .flatten()
                            .filter(|&k| k < n && !p.has(k))
                    });
                if let Some(k) = hit {
                    best = Some((k, d));
                    break;
                }
            }
        }
        best
    }

    /// La page `k` est-elle pres d'une tete vivante ?
    fn near_live(&self, k: usize) -> bool {
        let a = self.pages_of(AHEAD_S);
        self.heads_within(LIVE_S)
            .any(|h| k.abs_diff(self.job.pages.page_of(h.pos)) <= a)
    }

    /// Lit la page `k` de la source.
    fn read_page(&mut self, k: usize) -> Option<Vec<f32>> {
        let sh = self.job.shape;
        let p = &self.job.pages;
        let len = p.page_len(k);
        let want = len / sh.nch.max(1);
        self.buf.resize(len, 0.0);
        let f0 = k * p.page_frames();
        let got = self
            .job
            .src
            .read(f0 as f64 / sh.srate, sh.srate, sh.nch, &mut self.buf[..len]);
        if got == 0 && want > 0 {
            return None;
        }
        let mut v: Vec<f32> = self.buf[..len].iter().map(|&x| x as f32).collect();
        // Une source qui rend moins que demande : le reste en silence.
        for x in v.iter_mut().skip(got.min(want) * sh.nch) {
            *x = 0.0;
        }
        Some(v)
    }

    /// Lache ce qui est loin de toutes les tetes recentes (jamais le debut).
    fn evict(&self) {
        let p = &self.job.pages;
        let pin = self.pages_of(PIN_S).max(1) as i64;
        let (fa, bk) = (
            self.pages_of(KEEP_AHEAD_S) as i64,
            self.pages_of(KEEP_BEHIND_S) as i64,
        );
        let spans: Vec<(i64, i64)> = self
            .heads_within(RECENT_S)
            .map(|h| {
                let hk = p.page_of(h.pos) as i64;
                if h.back {
                    (hk - fa, hk + bk)
                } else {
                    (hk - bk, hk + fa)
                }
            })
            .collect();
        let far: Vec<usize> = p
            .resident_pages()
            .filter(|&k| {
                let k = k as i64;
                k >= pin && !spans.iter().any(|&(lo, hi)| k >= lo && k <= hi)
            })
            .collect();
        for k in far {
            p.evict(k);
        }
        p.reclaim();
    }
}

fn worker(cmds: Receiver<Cmd>, out: Sender<SMsg>) {
    let mut live: Vec<Live> = Vec::new();
    loop {
        // Attendre seulement si rien n'est urgent ni en cours.
        let busy = live.iter().any(|l| {
            l.wanted().is_some() || (l.pass.is_some() && !l.job.pages.is_abandoned() && !l.failed)
        });
        let wait = if busy {
            Duration::ZERO
        } else {
            Duration::from_millis(4)
        };
        match cmds.recv_timeout(wait) {
            Ok(Cmd::Add(job)) => {
                let total = job.shape.frames;
                live.push(Live {
                    pass: Some(PeakBuilder::new(total, job.shape.nch)),
                    job,
                    heads: [Seen {
                        pos: -1,
                        stamp: 0,
                        when: Instant::now(),
                        back: false,
                    }; HEADS + 1],
                    ready_sent: false,
                    last_snap: Instant::now(),
                    buf: Vec::new(),
                    failed: false,
                });
                continue;
            }
            Ok(Cmd::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // Le `Streamer` est lache : les sources reviennent au fil
                // principal, qui les detruit.
                for l in live {
                    let _ = out.send(SMsg::Gone {
                        key: l.job.key,
                        src: l.job.src,
                    });
                }
                return;
            }
        }
        // Les clips partis : la source revient au fil principal.
        let mut i = 0;
        while i < live.len() {
            if Arc::strong_count(&live[i].job.pages) == 1 {
                let l = live.swap_remove(i);
                let _ = out.send(SMsg::Gone {
                    key: l.job.key,
                    src: l.job.src,
                });
            } else {
                i += 1;
            }
        }
        // Les tetes de chaque clip, et ce qui est loin d'elles.
        for l in &mut live {
            l.look();
            l.evict();
        }
        // La page la plus urgente, tous clips confondus.
        let urgent = live
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.wanted().map(|(k, d)| (d, i, k)))
            .min();
        if let Some((_, i, k)) = urgent {
            let l = &mut live[i];
            match cp_sys::guard(|| l.read_page(k)).flatten() {
                Some(v) => {
                    l.job.pages.fill(k, v);
                    if !l.ready_sent && l.job.pages.has(0) {
                        l.ready_sent = true;
                        let _ = out.send(SMsg::Ready { key: l.job.key });
                    }
                }
                None => {
                    l.failed = true;
                    let _ = out.send(SMsg::Error {
                        key: l.job.key,
                        error: format!("the file returned no samples at page {k}"),
                    });
                }
            }
            continue;
        }
        // Sinon, les pics : une page de passage, pour un clip qu'on regarde.
        let Some(l) = live
            .iter_mut()
            .find(|l| l.pass.is_some() && !l.job.pages.is_abandoned() && !l.failed)
        else {
            continue;
        };
        let pf = l.job.pages.page_frames();
        let k = l.pass.as_ref().map_or(0, |b| b.frames() / pf);
        let page = if k < l.job.pages.pages() {
            cp_sys::guard(|| l.read_page(k)).flatten()
        } else {
            None
        };
        let Some(v) = page else {
            // Fini (ou plus rien a lire) : les pics complets.
            if let Some(b) = l.pass.take() {
                let _ = out.send(SMsg::Peaks {
                    key: l.job.key,
                    peaks: Arc::new(b.snapshot()),
                    done: true,
                });
            }
            continue;
        };
        if let Some(b) = l.pass.as_mut() {
            b.push(&v);
        }
        // La page lue sert aussi a la lecture si elle est pres d'une tete.
        if l.near_live(k) {
            l.job.pages.fill(k, v);
        }
        if l.last_snap.elapsed() > Duration::from_millis(500) {
            l.last_snap = Instant::now();
            if let Some(b) = &l.pass {
                let _ = out.send(SMsg::Peaks {
                    key: l.job.key,
                    peaks: Arc::new(b.snapshot()),
                    done: false,
                });
            }
        }
    }
}

/// Le fil de flux. Il s'arrete quand le `Streamer` est lache.
pub struct Streamer {
    tx: Option<Sender<Cmd>>,
    rx: Receiver<SMsg>,
    thread: Option<JoinHandle<()>>,
}

impl Streamer {
    pub fn new() -> Streamer {
        let (tx, cmds) = channel::<Cmd>();
        let (out, rx) = channel::<SMsg>();
        let thread = std::thread::Builder::new()
            .name("cp_stream".into())
            .spawn(move || worker(cmds, out))
            .ok();
        Streamer {
            tx: Some(tx),
            rx,
            thread,
        }
    }

    /// Rend le travail s'il n'a pas pu etre confie.
    pub fn add(&self, job: StreamJob) -> Result<(), Box<StreamJob>> {
        match &self.tx {
            Some(t) => t.send(Cmd::Add(job)).map_err(|e| match e.0 {
                Cmd::Add(j) => Box::new(j),
                Cmd::Wake => unreachable!("on a envoye un clip"),
            }),
            None => Err(Box::new(job)),
        }
    }

    /// Une tete a bouge (un depart, un saut) : regarder tout de suite.
    pub fn wake(&self) {
        if let Some(t) = &self.tx {
            let _ = t.send(Cmd::Wake);
        }
    }

    pub fn try_msg(&self) -> Option<SMsg> {
        self.rx.try_recv().ok()
    }
}

impl Default for Streamer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Les sources rendues sont detruites ici, sur le fil principal.
        while let Ok(m) = self.rx.try_recv() {
            drop(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::tests::Fake;
    use std::sync::Mutex;
    use std::thread::ThreadId;

    struct Tracked(Fake, Arc<Mutex<Option<ThreadId>>>);

    impl Reader for Tracked {
        fn channels(&mut self) -> i32 {
            self.0.channels()
        }
        fn sample_rate(&mut self) -> f64 {
            self.0.sample_rate()
        }
        fn length(&mut self) -> f64 {
            self.0.length()
        }
        fn read(&mut self, t: f64, sr: f64, nch: usize, out: &mut [f64]) -> usize {
            self.0.read(t, sr, nch, out)
        }
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            *self.1.lock().unwrap() = Some(std::thread::current().id());
        }
    }

    #[test]
    fn a_l_arret_les_sources_reviennent() {
        // Un clip encore tenu (ses pages vivent) quand le fil s'arrete : sa
        // source revient et meurt sur le fil qui lache le `Streamer`.
        let seen = Arc::new(Mutex::new(None));
        let pages = Arc::new(Paged::new(48000 * 60, 2, 48000));
        {
            let st = Streamer::new();
            let mut src = Tracked(
                Fake {
                    frames: 48000 * 60,
                    nch: 2,
                    srate: 48000.0,
                },
                seen.clone(),
            );
            let shape = crate::decode::shape(&mut src).unwrap();
            assert!(
                st.add(StreamJob {
                    key: 1,
                    src: Box::new(src),
                    shape,
                    pages: pages.clone(),
                })
                .is_ok()
            );
            let t0 = std::time::Instant::now();
            while !pages.has(0) {
                assert!(t0.elapsed().as_secs() < 5);
                std::thread::yield_now();
            }
        }
        assert_eq!(*seen.lock().unwrap(), Some(std::thread::current().id()));
    }
}
