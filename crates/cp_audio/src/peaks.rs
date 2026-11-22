//! Les pics d'un clip, pour dessiner sa forme d'onde a tout zoom.
//!
//! Une pyramide : le niveau 0 tient le min et le max de chaque bloc de
//! [`BASE`] frames, chaque niveau suivant regroupe 4 blocs du precedent. Une
//! requete choisit le niveau dont le bloc fait au plus le quart d'un pixel :
//! l'erreur de bord reste sous le demi-pixel, et un pixel ne combine qu'une
//! poignee de blocs, quel que soit le zoom. Sous [`BASE`] * 4 frames par pixel,
//! on lit la matiere elle-meme. Calculee une fois, sur le fil de travail.

use cp_engine::ClipData;

pub const BASE: usize = 16;
const FACTOR: usize = 4;

struct Level {
    block: usize,
    /// Par bloc, par canal : (min, max).
    mm: Vec<[f32; 2]>,
}

pub struct Peaks {
    nch: usize,
    frames: usize,
    levels: Vec<Level>,
}

/// Les pics qui se construisent au fil d'une lecture (un long fichier lu en
/// flux : un passage sur tout le fichier, en tache de fond). Le bloc du
/// niveau 0 grandit avec la longueur : au plus 2^21 blocs (21 Mo pour deux
/// heures en stereo).
pub struct PeakBuilder {
    block: usize,
    nch: usize,
    frames: usize,
    mm: Vec<[f32; 2]>,
}

impl PeakBuilder {
    pub fn new(total_frames: usize, nch: usize) -> PeakBuilder {
        let mut block = BASE;
        while total_frames / block > 1 << 21 {
            block *= FACTOR;
        }
        PeakBuilder {
            block,
            nch: nch.max(1),
            frames: 0,
            mm: Vec::new(),
        }
    }

    /// La suite de la matiere (entrelacee).
    pub fn push(&mut self, samples: &[f32]) {
        let nch = self.nch;
        for fr in samples.chunks_exact(nch) {
            let b = self.frames / self.block;
            if self.mm.len() < (b + 1) * nch {
                self.mm.resize((b + 1) * nch, [f32::MAX, f32::MIN]);
            }
            for (c, &v) in fr.iter().enumerate() {
                let e = &mut self.mm[b * nch + c];
                e[0] = e[0].min(v);
                e[1] = e[1].max(v);
            }
            self.frames += 1;
        }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Les pics de ce qui est lu jusqu'ici.
    pub fn snapshot(&self) -> Peaks {
        Peaks::from_level0(self.mm.clone(), self.block, self.nch, self.frames)
    }
}

impl Peaks {
    pub fn build(samples: &[f32], nch: usize) -> Peaks {
        let frames = samples.len().checked_div(nch).unwrap_or(0);
        let nb = frames.div_ceil(BASE);
        let mut mm = vec![[f32::MAX, f32::MIN]; nb * nch];
        for (f, fr) in samples.chunks_exact(nch.max(1)).enumerate() {
            let b = f / BASE;
            for (c, &v) in fr.iter().enumerate() {
                let e = &mut mm[b * nch + c];
                e[0] = e[0].min(v);
                e[1] = e[1].max(v);
            }
        }
        Peaks::from_level0(mm, BASE, nch, frames)
    }

    /// La pyramide au-dessus d'un niveau 0 (blocs de `block` frames).
    fn from_level0(mm: Vec<[f32; 2]>, block: usize, nch: usize, frames: usize) -> Peaks {
        let mut levels = vec![Level { block, mm }];
        loop {
            let prev = levels.last().expect("un niveau au moins");
            let pb = prev.mm.len() / nch.max(1);
            if pb <= 1 {
                break;
            }
            let nb = pb.div_ceil(FACTOR);
            let mut mm = vec![[f32::MAX, f32::MIN]; nb * nch];
            for b in 0..pb {
                for c in 0..nch {
                    let s = prev.mm[b * nch + c];
                    let e = &mut mm[(b / FACTOR) * nch + c];
                    e[0] = e[0].min(s[0]);
                    e[1] = e[1].max(s[1]);
                }
            }
            let block = prev.block * FACTOR;
            levels.push(Level { block, mm });
        }
        Peaks {
            nch,
            frames,
            levels,
        }
    }

    pub fn bytes(&self) -> usize {
        self.levels.iter().map(|l| l.mm.len() * 8).sum()
    }

    /// Les frames couvertes (moins que le clip tant qu'un passage en flux
    /// n'est pas fini).
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Le min et le max de chaque pixel : le pixel `i` couvre les frames
    /// `[start + i * fpp, start + (i + 1) * fpp)`. `ch = None` fond les canaux.
    /// Sous une frame par pixel, min = max = la matiere interpolee a ce point.
    /// Rend le nombre de pixels qui tombent dans le clip (les autres valent 0).
    pub fn query(
        &self,
        clip: &ClipData,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        out_min: &mut [f32],
        out_max: &mut [f32],
    ) -> usize {
        let n = out_min.len().min(out_max.len());
        let nch = self.nch;
        let data = clip.samples();
        // Un clip en flux : la matiere des pages en memoire ; ailleurs, le
        // niveau 0.
        let pg = clip.paged().map(|p| p.read());
        let resident = |f: usize| match &pg {
            Some(r) => r.has_frame(f as i64),
            None => f < data.len() / nch.max(1),
        };
        let at = |s: usize| match &pg {
            Some(r) => r.sample(s),
            None => data[s],
        };
        let (c0, c1) = match ch {
            Some(c) if c < nch => (c, c + 1),
            _ => (0, nch),
        };
        let total = self.frames as f64;
        let mut valid = 0;
        for i in 0..n {
            let a = start + i as f64 * fpp;
            let b = a + fpp;
            out_min[i] = 0.0;
            out_max[i] = 0.0;
            if fpp <= 0.0 || b <= 0.0 || a >= total {
                continue;
            }
            valid += 1;
            let fa = a.max(0.0).floor() as usize;
            let fb = (b.ceil() as usize).min(self.frames).max(fa + 1);
            let len = fb - fa;
            // Le plus gros niveau dont le bloc tient quatre fois dans le pixel
            // (un clip en flux hors de ses pages : le niveau 0).
            let lvl = self
                .levels
                .iter()
                .rev()
                .find(|l| l.block * FACTOR <= len)
                .or_else(|| {
                    (!resident(fa) || !resident(fb.saturating_sub(1))).then(|| &self.levels[0])
                });
            if fpp <= 1.0 && lvl.is_none() {
                // La matiere, interpolee lineairement entre deux echantillons.
                let x = a.max(0.0);
                let f0 = (x.floor() as usize).min(self.frames - 1);
                let f1 = (f0 + 1).min(self.frames - 1);
                let t = (x - f0 as f64) as f32;
                let mut v = 0.0f32;
                for c in c0..c1 {
                    let s0 = at(f0 * nch + c);
                    let s1 = at(f1 * nch + c);
                    v += s0 + (s1 - s0) * t;
                }
                v /= (c1 - c0).max(1) as f32;
                out_min[i] = v;
                out_max[i] = v;
                continue;
            }
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            match lvl {
                None => {
                    for f in fa..fb {
                        for c in c0..c1 {
                            let v = at(f * nch + c);
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                    }
                }
                Some(l) => {
                    let ba = fa / l.block;
                    let bb = fb.div_ceil(l.block).min(l.mm.len() / nch);
                    for blk in ba..bb {
                        for c in c0..c1 {
                            let e = l.mm[blk * nch + c];
                            lo = lo.min(e[0]);
                            hi = hi.max(e[1]);
                        }
                    }
                }
            }
            if lo <= hi {
                out_min[i] = lo;
                out_max[i] = hi;
            }
        }
        valid
    }
}

/// Les pics d'un clip qui se remplit encore : lus a la volee sur ce qui est
/// deja decode, en au plus 64 lectures par pixel (une approximation qui ne
/// dure que le temps du decodage). Au-dela du decode : 0.
pub fn approx(
    clip: &ClipData,
    ch: Option<usize>,
    start: f64,
    fpp: f64,
    out_min: &mut [f32],
    out_max: &mut [f32],
) -> usize {
    let n = out_min.len().min(out_max.len());
    let nch = clip.nch().max(1);
    let data = clip.samples();
    let avail = data.len() / nch;
    let total = clip.frames() as f64;
    let (c0, c1) = match ch {
        Some(c) if c < nch => (c, c + 1),
        _ => (0, nch),
    };
    let mut valid = 0;
    for i in 0..n {
        out_min[i] = 0.0;
        out_max[i] = 0.0;
        let a = start + i as f64 * fpp;
        let b = a + fpp.max(1.0);
        if fpp <= 0.0 || b <= 0.0 || a >= total {
            continue;
        }
        valid += 1;
        let fa = a.max(0.0).floor() as usize;
        let fb = (b.ceil() as usize).min(avail);
        if fa >= fb {
            continue;
        }
        let step = ((fb - fa) / 64).max(1);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut f = fa;
        while f < fb {
            for c in c0..c1 {
                let v = data[f * nch + c];
                lo = lo.min(v);
                hi = hi.max(v);
            }
            f += step;
        }
        if lo <= hi {
            out_min[i] = lo;
            out_max[i] = hi;
        }
    }
    valid
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_engine::Pool;

    fn clip(samples: Vec<f32>, nch: usize) -> (Pool, i32) {
        let mut p = Pool::new();
        let id = p.add(samples, nch, 48000.0).unwrap();
        (p, id)
    }

    #[test]
    fn chaque_zoom_rend_le_vrai_min_max_a_un_bloc_pres() {
        // Une matiere pseudo-aleatoire, deux canaux.
        let frames = 100_003usize;
        let mut s = Vec::with_capacity(frames * 2);
        let mut x: u32 = 12345;
        for _ in 0..frames * 2 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s.push((x >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0);
        }
        let pk = Peaks::build(&s, 2);
        let (pool, id) = clip(s.clone(), 2);
        let c = pool.get(id).unwrap();
        for &fpp in &[2.0, 17.0, 64.0, 300.0, 5000.0, 99_000.0] {
            let n = 40;
            let (mut lo, mut hi) = (vec![0.0; n], vec![0.0; n]);
            let start = 1234.5;
            pk.query(c, Some(1), start, fpp, &mut lo, &mut hi);
            for i in 0..n {
                let a = start + i as f64 * fpp;
                let b = a + fpp;
                if a >= frames as f64 {
                    assert_eq!((lo[i], hi[i]), (0.0, 0.0));
                    continue;
                }
                // La verite : la matiere elle-meme sur le pixel...
                let fa = a.floor() as usize;
                let fb = (b.ceil() as usize).min(frames);
                let mut t = (f32::MAX, f32::MIN);
                for f in fa..fb {
                    t.0 = t.0.min(s[f * 2 + 1]);
                    t.1 = t.1.max(s[f * 2 + 1]);
                }
                // ... et elle elargie d'un quart de pixel de chaque cote (le
                // bloc du niveau choisi) : la reponse est entre les deux.
                let m = (fpp / 4.0).ceil() as usize + BASE;
                let mut w = (f32::MAX, f32::MIN);
                for f in fa.saturating_sub(m)..(fb + m).min(frames) {
                    w.0 = w.0.min(s[f * 2 + 1]);
                    w.1 = w.1.max(s[f * 2 + 1]);
                }
                assert!(lo[i] <= t.0 && lo[i] >= w.0, "fpp {fpp} pixel {i}");
                assert!(hi[i] >= t.1 && hi[i] <= w.1, "fpp {fpp} pixel {i}");
            }
        }
    }

    #[test]
    fn sous_une_frame_par_pixel_on_lit_la_matiere() {
        let s: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let pk = Peaks::build(&s, 1);
        let (pool, id) = clip(s, 1);
        let c = pool.get(id).unwrap();
        let (mut lo, mut hi) = (vec![0.0; 8], vec![0.0; 8]);
        let n = pk.query(c, None, 10.0, 0.25, &mut lo, &mut hi);
        assert_eq!(n, 8);
        for i in 0..8 {
            assert_eq!(lo[i], 10.0 + i as f32 * 0.25);
            assert_eq!(hi[i], lo[i]);
        }
    }

    #[test]
    fn la_pyramide_coute_moins_d_un_cinquieme_du_clip() {
        let s = vec![0.5f32; 1_000_000 * 2];
        let pk = Peaks::build(&s, 2);
        assert!(pk.bytes() * 5 < s.len() * 4, "{} octets", pk.bytes());
    }
}
