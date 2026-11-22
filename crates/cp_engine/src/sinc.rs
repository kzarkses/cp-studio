//! L'interpolation de haute qualite : un sinc a fenetre de Kaiser, lu dans
//! une table (l'algorithme de J. O. Smith, « bandlimited interpolation »).
//!
//! Hermite (4 points) reste le defaut : rapide, et c'est lui que l'oracle
//! compare a l'ancien C++ au bit pres. Le sinc se choisit par voix
//! (`Param::Interp`) : pour l'ecoute exigeante et tout rendu.
//!
//! - 16 passages par zero de chaque cote (32 points a vitesse 1 ou moins),
//!   la table echantillonnee 512 fois par passage, interpolee lineairement
//!   entre deux points ;
//! - fenetre de Kaiser, beta 9,5 : les lobes a environ -95 dB ;
//! - la coupure a 0,94 du Nyquist (la bande de transition) ;
//! - plus vite que 1 (la hauteur monte), le noyau s'elargit d'autant : la
//!   coupure suit le nouveau Nyquist, rien ne se replie. Borne a 8 (256
//!   points).
//!
//! La table se construit sur le fil principal ([`init`], appele par le
//! moteur a sa creation) : le fil audio ne fait que la lire.

use std::sync::OnceLock;

/// Passages par zero de chaque cote.
pub const NZ: usize = 16;
/// Points de la table par passage.
const L: usize = 512;
/// La coupure, en fraction du Nyquist.
const ROLLOFF: f64 = 0.94;
/// La fenetre de Kaiser.
const BETA: f64 = 9.5;
/// La vitesse au-dela de laquelle le noyau ne s'elargit plus.
pub const MAX_STEP: f64 = 8.0;

pub struct Kernel {
    /// h(x) pour x = k / L, k = 0..NZ*L (une aile ; le noyau est pair).
    h: Box<[f32]>,
    /// h[k+1] - h[k].
    d: Box<[f32]>,
}

static KERNEL: OnceLock<Kernel> = OnceLock::new();

/// Construit la table (fil principal ; une fois).
pub fn init() -> &'static Kernel {
    KERNEL.get_or_init(Kernel::new)
}

/// La table, si elle est construite (le fil audio ne construit rien).
#[inline]
pub fn kernel() -> Option<&'static Kernel> {
    KERNEL.get()
}

/// Bessel modifiee de premiere espece, ordre 0 (pour Kaiser).
fn i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
    }
    sum
}

impl Kernel {
    fn new() -> Kernel {
        let n = NZ * L + 1;
        let ib = i0(BETA);
        let h: Vec<f32> = (0..n)
            .map(|k| {
                let x = k as f64 / L as f64;
                let s = if x == 0.0 {
                    ROLLOFF
                } else {
                    let a = std::f64::consts::PI * ROLLOFF * x;
                    ROLLOFF * a.sin() / a
                };
                let r = x / NZ as f64;
                let w = if r >= 1.0 {
                    0.0
                } else {
                    i0(BETA * (1.0 - r * r).sqrt()) / ib
                };
                (s * w) as f32
            })
            .collect();
        let d: Vec<f32> = (0..n)
            .map(|k| if k + 1 < n { h[k + 1] - h[k] } else { -h[k] })
            .collect();
        Kernel {
            h: h.into_boxed_slice(),
            d: d.into_boxed_slice(),
        }
    }

    /// Le noyau a `x` passages par zero de son centre (x >= 0).
    #[inline]
    fn at(&self, x: f64) -> f32 {
        let xi = x * L as f64;
        let k = xi as usize;
        if k >= NZ * L {
            return 0.0;
        }
        self.h[k] + (xi - k as f64) as f32 * self.d[k]
    }

    /// La valeur a la position `p` (frames source), lue a la vitesse `step`
    /// (frames source par frame de sortie) : la somme des echantillons
    /// ponderes par le noyau. `get(n)` rend l'echantillon `n` du canal (0
    /// hors de la matiere). Rend (gauche, droite) pour `nch` = 1 ou 2.
    #[inline]
    pub fn read(&self, p: f64, step: f64, get: impl Fn(i64) -> (f32, f32)) -> (f32, f32) {
        let fs = if step > 1.0 {
            1.0 / step.min(MAX_STEP)
        } else {
            1.0
        };
        let i0 = p.floor() as i64;
        let reach = (NZ as f64 / fs).ceil() as i64;
        let (mut l, mut r) = (0f32, 0f32);
        // L'aile gauche (la position et avant), puis la droite.
        for n in (i0 - reach + 1)..=(i0 + reach) {
            let x = (p - n as f64).abs() * fs;
            if x >= NZ as f64 {
                continue;
            }
            let w = self.at(x);
            if w == 0.0 {
                continue;
            }
            let (a, b) = get(n);
            l += a * w;
            r += b * w;
        }
        (l * fs as f32, r * fs as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_noyau_est_un_passe_bas_de_gain_un() {
        let k = init();
        // La somme des poids a une position quelconque vaut 1 (le continu
        // passe), a vitesse 1 et plus vite.
        for &step in &[0.5, 1.0, 1.7, 3.0] {
            for &p in &[100.0, 100.25, 100.5, 100.9] {
                let (l, _) = k.read(p, step, |_| (1.0, 1.0));
                assert!((l - 1.0).abs() < 2e-3, "step {step} p {p} : {l}");
            }
        }
        // Le centre et les passages par zero.
        assert!((k.at(0.0) - ROLLOFF as f32).abs() < 1e-6);
        assert_eq!(k.at(NZ as f64), 0.0);
    }
}
