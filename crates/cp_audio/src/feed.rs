//! Ce que REAPER tire : un port du moteur, converti en `ReaSample`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

use cp_engine::Core;
use cp_engine::types::MAX_CHANS;
use cp_sys::pcm::{Block, PortRender};

/// Le rendu se fait par morceaux de cette taille, dans un tampon sur la pile :
/// aucune allocation, aucun verrou dans le fil audio.
const CHUNK: usize = 512;

/// Les compteurs d'un port, lus par le fil principal.
#[derive(Default)]
pub struct FeedStats {
    pub calls: AtomicU64,
    /// Un temps deja rendu, redemande (REAPER recommence un rendu).
    pub rewinds: AtomicU32,
    /// Un temps saute.
    pub skips: AtomicU32,
    pub last_len: AtomicU32,
    /// Frames rendus en tout : le temps mur pendant lequel le port a ete tire.
    pub frames: AtomicU64,
    /// Temps passe dans le rendu, en nanosecondes.
    pub busy_ns: AtomicU64,
    /// Le dernier temps demande (bits d'un f64), pour la continuite.
    last_time: AtomicU64,
    /// La crete de chaque canal depuis la derniere lecture (bits d'un f32
    /// positif : l'ordre des entiers est celui des nombres).
    pub peak: [AtomicU32; 2],
}

impl FeedStats {
    /// Les cretes (gauche, droite) depuis la derniere lecture, remises a 0.
    pub fn take_peaks(&self) -> [f32; 2] {
        [
            f32::from_bits(self.peak[0].swap(0, Relaxed)),
            f32::from_bits(self.peak[1].swap(0, Relaxed)),
        ]
    }
}

pub struct PortFeed {
    pub core: Arc<Core>,
    pub port: usize,
    pub stats: Arc<FeedStats>,
}

impl PortRender for PortFeed {
    fn render(&self, b: &mut Block<'_>) {
        let t0 = Instant::now();
        let st = &self.stats;
        let srate = if b.srate > 1.0 {
            b.srate
        } else {
            self.core.srate()
        };

        // --- la demande est-elle contigue ? (on compte, on ne corrige pas)
        let calls = st.calls.fetch_add(1, Relaxed);
        if calls > 0 {
            let last = f64::from_bits(st.last_time.load(Relaxed));
            let expected = last + st.last_len.load(Relaxed) as f64 / srate;
            let d = b.time_s - expected;
            let tol = 0.5 / srate;
            if d < -tol {
                st.rewinds.fetch_add(1, Relaxed);
            } else if d > tol {
                st.skips.fetch_add(1, Relaxed);
            }
        }
        st.last_time.store(b.time_s.to_bits(), Relaxed);
        st.last_len.store(b.frames as u32, Relaxed);

        // --- le rendu, par morceaux
        let nch = b.nch;
        let chans = nch.min(MAX_CHANS);
        let mut tmp = [0.0f32; CHUNK * MAX_CHANS];
        let mut pk = [0.0f32; 2];
        let mut done = 0;
        while done < b.frames {
            let n = (b.frames - done).min(CHUNK);
            let ok = self
                .core
                .render_port(self.port, &mut tmp[..n * chans], n, chans)
                .is_some();
            let out = &mut b.out[done * nch..(done + n) * nch];
            if ok {
                for i in 0..n {
                    for c in 0..nch {
                        out[i * nch + c] = if c < chans {
                            tmp[i * chans + c] as f64
                        } else {
                            0.0
                        };
                    }
                    for c in 0..chans.min(2) {
                        pk[c] = pk[c].max(tmp[i * chans + c].abs());
                    }
                }
            } else {
                out.fill(0.0);
            }
            done += n;
        }
        for (c, &v) in pk.iter().enumerate() {
            // Un canal mono se lit des deux cotes.
            let v = if chans == 1 { pk[0] } else { v };
            if v.is_finite() && v > 0.0 {
                st.peak[c].fetch_max(v.to_bits(), Relaxed);
            }
        }
        st.frames.fetch_add(b.frames as u64, Relaxed);
        st.busy_ns
            .fetch_add(t0.elapsed().as_nanos() as u64, Relaxed);
    }

    fn channels(&self) -> i32 {
        MAX_CHANS as i32
    }

    fn sample_rate(&self) -> f64 {
        self.core.srate()
    }
}
