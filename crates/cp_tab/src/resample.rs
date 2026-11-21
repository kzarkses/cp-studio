//! Le son au format du modele : mono, 22 050 Hz.
//!
//! Un sinc fenetre (Kaiser, beta 8,6, 24 passages par zero), coupe a 97 % du
//! plus petit Nyquist, lu dans une table (512 phases par echantillon, entre
//! lesquelles on interpole lineairement). Le modele a ete entraine sur des
//! sons reechantillonnes proprement (librosa) : un repliement au-dessus de
//! 11 kHz lui montrerait des harmoniques qui n'existent pas.

use crate::model::SR;

const ZERO_CROSSINGS: f64 = 24.0;
const PHASES: usize = 512;
const BETA: f64 = 8.6;

/// Bessel I0 (serie), pour la fenetre de Kaiser.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k as f64 * k as f64);
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
    }
    sum
}

/// Les canaux entrelaces `nch` -> mono (moyenne).
pub fn mixdown(input: &[f32], nch: usize) -> Vec<f32> {
    let nch = nch.max(1);
    if nch == 1 {
        return input.to_vec();
    }
    input
        .chunks_exact(nch)
        .map(|f| f.iter().sum::<f32>() / nch as f32)
        .collect()
}

/// Reechantillonne un signal mono de `from` Hz a `to` Hz.
pub fn resample(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    if x.is_empty() || from <= 0.0 || to <= 0.0 {
        return Vec::new();
    }
    if (from - to).abs() < 1e-9 {
        return x.to_vec();
    }
    let ratio = to / from;
    // La coupure, en cycles par echantillon d'ENTREE.
    let fc = 0.5 * ratio.min(1.0) * 0.97;
    let half = ZERO_CROSSINGS / (2.0 * fc); // demi-largeur, en echantillons d'entree
    let n_tab = (half * PHASES as f64).ceil() as usize + 2;
    let mut table = vec![0f64; n_tab];
    let i0b = bessel_i0(BETA);
    for (k, t) in table.iter_mut().enumerate() {
        let d = k as f64 / PHASES as f64;
        if d > half {
            *t = 0.0;
            continue;
        }
        let a = 2.0 * fc * d;
        let sinc = if a.abs() < 1e-12 {
            1.0
        } else {
            (std::f64::consts::PI * a).sin() / (std::f64::consts::PI * a)
        };
        let r = d / half;
        let w = bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / i0b;
        *t = 2.0 * fc * sinc * w;
    }
    let h = |d: f64| -> f64 {
        let p = d.abs() * PHASES as f64;
        let i = p as usize;
        if i + 1 >= n_tab {
            return 0.0;
        }
        let f = p - i as f64;
        table[i] * (1.0 - f) + table[i + 1] * f
    };
    let n_out = ((x.len() as f64) * ratio).floor() as usize;
    let mut out = Vec::with_capacity(n_out);
    let reach = half.ceil() as i64;
    for i in 0..n_out {
        let t = i as f64 / ratio;
        let c = t.floor() as i64;
        let mut s = 0f64;
        for j in (c - reach)..=(c + reach + 1) {
            if j < 0 || j >= x.len() as i64 {
                continue;
            }
            s += x[j as usize] as f64 * h(t - j as f64);
        }
        out.push(s as f32);
    }
    out
}

/// Ce que le modele attend : mono, 22 050 Hz.
pub fn for_model(interleaved: &[f32], nch: usize, sr: f64) -> Vec<f32> {
    resample(&mixdown(interleaved, nch), sr, SR as f64)
}
