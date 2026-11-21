//! Le rasteriseur : la couverture EXACTE de chaque pixel par un polygone.
//!
//! Methode de l'accumulation d'aire signee (celle de font-rs / stb_truetype 2) :
//! chaque segment depose, dans les pixels qu'il traverse, l'aire signee qu'il
//! ferme a sa droite ; une somme courante par ligne donne la couverture. Aucun
//! sur-echantillonnage : la couverture d'un bord est l'aire vraie du pixel
//! sous le bord. La couverture finale est `min(1, |somme|)` : des formes de
//! meme sens qui se recouvrent s'unissent (traits), une forme de sens inverse
//! troue (anneaux).

use crate::path::Pt;

/// Le tampon d'accumulation, garde d'une image a l'autre (aucune allocation
/// en regime).
#[derive(Default)]
pub struct Raster {
    acc: Vec<f32>,
    w: usize,
    h: usize,
}

impl Raster {
    /// Prepare un champ de `w x h` pixels (plus une colonne de garde).
    pub fn reset(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        let n = (w + 2) * h;
        if self.acc.len() < n {
            self.acc.resize(n, 0.0);
        }
        self.acc[..n].fill(0.0);
    }

    fn stride(&self) -> usize {
        self.w + 2
    }

    /// Ajoute un segment (coordonnees relatives au champ). Il est coupe en x a
    /// [0, w] : ce qui depasse a gauche est rabattu sur le bord gauche (sa
    /// contribution a l'enroulement est la meme), ce qui depasse a droite ne
    /// touche aucun pixel visible et est rabattu sur le bord droit.
    pub fn line(&mut self, p0: Pt, p1: Pt) {
        let wf = self.w as f32;
        // Decoupe aux verticales x = 0 et x = w.
        let mut pts = [p0, p0, p0, p1];
        let mut n = 0;
        pts[n] = p0;
        n += 1;
        let cross = |a: Pt, b: Pt, x: f32| -> Option<Pt> {
            if (a.x < x) != (b.x < x) && (b.x - a.x).abs() > 1e-12 {
                let t = (x - a.x) / (b.x - a.x);
                Some(Pt {
                    x,
                    y: a.y + (b.y - a.y) * t,
                })
            } else {
                None
            }
        };
        let mut cuts: [Option<Pt>; 2] = [cross(p0, p1, 0.0), cross(p0, p1, wf)];
        if let (Some(a), Some(b)) = (cuts[0], cuts[1]) {
            // Dans l'ordre du parcours.
            let da = (a.x - p0.x).abs();
            let db = (b.x - p0.x).abs();
            if db < da {
                cuts = [Some(b), Some(a)];
            }
        }
        for c in cuts.into_iter().flatten() {
            pts[n] = c;
            n += 1;
        }
        pts[n] = p1;
        n += 1;
        for i in 0..n - 1 {
            let (mut a, mut b) = (pts[i], pts[i + 1]);
            a.x = a.x.clamp(0.0, wf);
            b.x = b.x.clamp(0.0, wf);
            self.line_inside(a, b);
        }
    }

    fn line_inside(&mut self, p0: Pt, p1: Pt) {
        if (p0.y - p1.y).abs() <= 1e-9 {
            return;
        }
        let (dir, p0, p1) = if p0.y < p1.y {
            (1.0f32, p0, p1)
        } else {
            (-1.0f32, p1, p0)
        };
        let hf = self.h as f32;
        if p1.y <= 0.0 || p0.y >= hf {
            return;
        }
        let stride = self.stride();
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        let ys = p0.y.max(0.0);
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let y0 = ys as usize;
        let y1 = (p1.y.ceil() as usize).min(self.h);
        for y in y0..y1 {
            let row = y * stride;
            let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let xaf = xa.floor();
            let xai = xaf as usize;
            let xbc = xb.ceil();
            let xbi = xbc as usize;
            if xbi <= xai + 1 {
                let xmf = 0.5 * (x + xnext) - xaf;
                self.acc[row + xai] += d - d * xmf;
                self.acc[row + xai + 1] += d * xmf;
            } else {
                let s = (xb - xa).recip();
                let x0f = xa - xaf;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = xb - xbc + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.acc[row + xai] += d * a0;
                if xbi == xai + 2 {
                    self.acc[row + xai + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.acc[row + xai + 1] += d * (a1 - a0);
                    for xi in xai + 2..xbi - 1 {
                        self.acc[row + xi] += d * s;
                    }
                    let a2 = a1 + (xbi - xai - 3) as f32 * s;
                    self.acc[row + xbi - 1] += d * (1.0 - a2 - am);
                }
                self.acc[row + xbi] += d * am;
            }
            x = xnext;
        }
    }

    /// Ligne par ligne, la couverture (0..255) de chaque pixel : `f(y, x0,
    /// couvertures)` recoit des plages contigues non nulles.
    pub fn sweep(&self, mut f: impl FnMut(usize, usize, &[u8])) {
        let stride = self.stride();
        let mut buf = vec![0u8; self.w];
        for y in 0..self.h {
            let row = &self.acc[y * stride..y * stride + self.w];
            let mut sum = 0.0f32;
            let mut run_start: Option<usize> = None;
            for (x, &a) in row.iter().enumerate() {
                sum += a;
                let c = (sum.abs().min(1.0) * 255.0 + 0.5) as u8;
                buf[x] = c;
                match (c > 0, run_start) {
                    (true, None) => run_start = Some(x),
                    (false, Some(s)) => {
                        f(y, s, &buf[s..x]);
                        run_start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = run_start {
                f(y, s, &buf[s..self.w]);
            }
        }
    }

    /// La couverture totale (somme des couvertures, en pixels) : pour les tests.
    pub fn total(&self) -> f64 {
        let stride = self.stride();
        let mut t = 0.0f64;
        for y in 0..self.h {
            let mut sum = 0.0f32;
            for x in 0..self.w {
                sum += self.acc[y * stride + x];
                t += sum.abs().min(1.0) as f64;
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{Path, pt};

    fn fill(path: &Path, w: usize, h: usize) -> Raster {
        let mut r = Raster::default();
        r.reset(w, h);
        let mut polys = Vec::new();
        path.flatten(0.02, &mut polys);
        for p in &polys {
            for s in p.windows(2) {
                r.line(s[0], s[1]);
            }
        }
        r
    }

    #[test]
    fn l_aire_d_un_disque_est_pi_r_carre() {
        for &(cx, cy, rad) in &[
            (50.0f32, 50.0f32, 30.0f32),
            (40.3, 41.7, 12.25),
            (10.5, 10.5, 3.0),
        ] {
            let mut p = Path::new();
            p.circle(cx, cy, rad);
            let got = fill(&p, 100, 100).total();
            let want = std::f64::consts::PI * (rad as f64).powi(2);
            // Le polygone inscrit (48 cotes au moins) perd au plus 0,3 %.
            assert!(
                (got - want).abs() / want < 3e-3,
                "r {rad} : {got} contre {want}"
            );
        }
    }

    #[test]
    fn un_rectangle_a_cheval_couvre_son_aire_exacte() {
        let mut p = Path::new();
        p.rect(10.25, 20.5, 30.5, 10.75);
        let got = fill(&p, 64, 64).total();
        assert!((got - 30.5 * 10.75).abs() < 1e-3, "{got}");
    }

    #[test]
    fn ce_qui_depasse_du_champ_compte_pour_l_enroulement() {
        // Un rectangle qui deborde a gauche et a droite : seule sa part visible
        // est couverte, et pleinement.
        let mut p = Path::new();
        p.rect(-50.0, 2.0, 200.0, 4.0);
        let r = fill(&p, 20, 10);
        assert!((r.total() - 20.0 * 4.0).abs() < 1e-3, "{}", r.total());
    }

    #[test]
    fn un_trou_de_sens_inverse_troue() {
        let mut p = Path::new();
        p.circle(50.0, 50.0, 40.0)
            .ellipse_rev(50.0, 50.0, 20.0, 20.0);
        let got = fill(&p, 100, 100).total();
        let want = std::f64::consts::PI * (1600.0 - 400.0);
        assert!((got - want).abs() / want < 2e-3, "{got} contre {want}");
    }

    #[test]
    fn un_trait_couvre_longueur_fois_epaisseur_plus_ses_bouts() {
        let mut line = Path::new();
        line.move_to(10.0, 30.0).line_to(90.0, 30.0);
        let mut st = Path::new();
        line.stroke(6.0, &mut st);
        let got = fill(&st, 100, 60).total();
        let want = 80.0 * 6.0 + std::f64::consts::PI * 9.0;
        assert!((got - want).abs() / want < 1e-2, "{got} contre {want}");
        let _ = pt(0.0, 0.0);
    }
}
