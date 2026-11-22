//! L'accordeur : la hauteur d'une note tenue (YIN), et son nom.
//!
//! YIN (de Cheveigne et Kawahara, 2002) : la difference cumulee normalisee
//! d'un signal avec lui-meme decale ; sa premiere vallee sous le seuil est la
//! periode. Une passe grossiere sur le signal decime (vers 12 kHz : une
//! guitare monte a 1,3 kHz), puis un affinage au taux plein autour de la
//! periode trouvee, avec une interpolation parabolique : moins d'un centieme
//! de demi-ton d'erreur.

/// Une hauteur entendue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pitch {
    /// Hz.
    pub freq: f64,
    /// 0..1 : 1 = parfaitement periodique.
    pub clarity: f32,
    /// La valeur efficace du signal.
    pub level: f32,
}

/// Le niveau sous lequel on n'ecoute pas (environ -50 dBFS efficaces).
pub const MIN_LEVEL: f32 = 0.003;

fn diff(x: &[f64], tau: usize, w: usize) -> f64 {
    let mut s = 0.0;
    for j in 0..w {
        let d = x[j] - x[j + tau];
        s += d * d;
    }
    s
}

/// La hauteur de `x` (au taux `sr`), entre `fmin` et `fmax` Hz. `None` : trop
/// faible, ou rien de periodique.
pub fn detect(x: &[f32], sr: f64, fmin: f64, fmax: f64) -> Option<Pitch> {
    if x.len() < 256 || sr <= 0.0 || fmin <= 0.0 || fmax <= fmin {
        return None;
    }
    let n = x.len();
    let mean = x.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let level =
        (x.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n as f64).sqrt() as f32;
    if level < MIN_LEVEL {
        return None;
    }
    // La passe grossiere, sur le signal decime (moyenne de `dec` echantillons :
    // un passe-bas suffisant pour trouver la periode).
    let dec = ((sr / 12000.0).round() as usize).max(1);
    let xs: Vec<f64> = x
        .chunks_exact(dec)
        .map(|c| c.iter().map(|&v| v as f64 - mean).sum::<f64>() / dec as f64)
        .collect();
    let srd = sr / dec as f64;
    let tmin = ((srd / fmax).floor() as usize).max(2);
    let tmax = (srd / fmin).ceil() as usize;
    if tmax * 2 >= xs.len() {
        return None;
    }
    let w = xs.len() - tmax - 1;
    let mut d = vec![0.0f64; tmax + 2];
    for (tau, v) in d.iter_mut().enumerate().take(tmax + 2).skip(1) {
        *v = diff(&xs, tau, w);
    }
    // La difference cumulee normalisee.
    let mut cm = vec![1.0f64; tmax + 2];
    let mut run = 0.0;
    for tau in 1..tmax + 2 {
        run += d[tau];
        cm[tau] = if run > 0.0 {
            d[tau] * tau as f64 / run
        } else {
            1.0
        };
    }
    // La premiere vallee sous le seuil (sinon la plus basse).
    const THRESH: f64 = 0.15;
    let mut best = None;
    let mut tau = tmin;
    while tau <= tmax {
        if cm[tau] < THRESH {
            while tau < tmax && cm[tau + 1] < cm[tau] {
                tau += 1;
            }
            best = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau_c = match best {
        Some(t) => t,
        None => {
            let t = (tmin..=tmax).min_by(|&a, &b| cm[a].total_cmp(&cm[b]))?;
            if cm[t] > 0.4 {
                return None;
            }
            t
        }
    };
    let clarity = (1.0 - cm[tau_c]).clamp(0.0, 1.0) as f32;
    // L'affinage au taux plein, sur `m` periodes (une note aigue n'a qu'une
    // courte periode : en mesurer plusieurs divise l'erreur d'autant).
    let xf: Vec<f64> = x.iter().map(|&v| v as f64 - mean).collect();
    let full = tau_c * dec;
    let m = ((n / 3) / full.max(1))
        .min((tau_c / 2).saturating_sub(1))
        .max(1);
    let center = full * m;
    let half = m * dec + 2;
    let lo = center.saturating_sub(half).max(2);
    let hi = center + half;
    if hi + 2 >= n {
        return None;
    }
    let wf = n - hi - 2;
    let df: Vec<(usize, f64)> = (lo..=hi + 1).map(|t| (t, diff(&xf, t, wf))).collect();
    let (k, _) = df
        .iter()
        .enumerate()
        .skip(1)
        .take(df.len().saturating_sub(2))
        .min_by(|a, b| a.1.1.total_cmp(&b.1.1))?;
    let (y0, y1, y2) = (df[k - 1].1, df[k].1, df[k + 1].1);
    let den = y0 - 2.0 * y1 + y2;
    let off = if den.abs() > 1e-12 {
        (0.5 * (y0 - y2) / den).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let period = (df[k].0 as f64 + off) / m as f64;
    Some(Pitch {
        freq: sr / period,
        clarity,
        level,
    })
}

/// La note la plus proche (MIDI) et l'ecart en centiemes, pour un La donne.
pub fn note_of(freq: f64, a4: f64) -> (i32, f32) {
    let m = 69.0 + 12.0 * (freq / a4).log2();
    let r = m.round();
    (r as i32, ((m - r) * 100.0) as f32)
}

/// « C », « C# »..., et l'octave (La 440 = A4).
pub fn note_name(midi: i32) -> (&'static str, i32) {
    const N: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    (N[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

/// La corde a vide la plus proche d'une hauteur (rang dans `strings`, de la
/// grave a l'aigue), si elle est a moins d'un ton.
pub fn nearest_string(midi: f64, strings: &[i32]) -> Option<usize> {
    strings
        .iter()
        .enumerate()
        .map(|(i, &s)| (i, (midi - s as f64).abs()))
        .filter(|&(_, d)| d <= 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une corde : dents de scie qui s'eteignent, un peu de bruit.
    fn string(f: f64, sr: f64, n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (s >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                let ph = (i as f64 * f / sr).fract();
                let env = (-(i as f64) / sr * 1.5).exp();
                ((2.0 * ph - 1.0) * 0.3 * env) as f32 + noise * 0.01
            })
            .collect()
    }

    #[test]
    fn les_cordes_a_vide_au_centieme() {
        for sr in [44100.0, 48000.0, 96000.0] {
            let n = (sr * 0.09) as usize;
            for (k, &midi) in [40, 45, 50, 55, 59, 64, 33, 76].iter().enumerate() {
                let f = 440.0 * 2f64.powf((midi as f64 - 69.0) / 12.0);
                // Desaccordee de -7 centiemes.
                let f = f * 2f64.powf(-7.0 / 1200.0);
                let p = detect(&string(f, sr, n, k as u32 + 1), sr, 30.0, 1400.0)
                    .unwrap_or_else(|| panic!("{midi} a {sr}"));
                let (m, c) = note_of(p.freq, 440.0);
                assert_eq!(m, midi, "{midi} a {sr} Hz : {} Hz", p.freq);
                assert!((c + 7.0).abs() < 1.0, "{midi} a {sr} : {c} centiemes");
                assert!(p.clarity > 0.8, "{midi} : clarte {}", p.clarity);
            }
        }
    }

    #[test]
    fn le_silence_et_le_bruit_ne_disent_rien() {
        let sr = 48000.0;
        assert!(detect(&vec![0.0; 4096], sr, 30.0, 1400.0).is_none());
        let mut s = 7u32;
        let noise: Vec<f32> = (0..4096)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                ((s >> 9) as f32 / (1u32 << 23) as f32 - 0.5) * 0.5
            })
            .collect();
        if let Some(p) = detect(&noise, sr, 30.0, 1400.0) {
            assert!(p.clarity < 0.7, "du bruit pris pour une note : {p:?}");
        }
        // Trop faible : rien.
        let quiet: Vec<f32> = string(110.0, sr, 4096, 3)
            .iter()
            .map(|v| v * 0.005)
            .collect();
        assert!(detect(&quiet, sr, 30.0, 1400.0).is_none());
    }

    #[test]
    fn noms_et_cordes() {
        assert_eq!(note_name(45), ("A", 2));
        assert_eq!(note_name(61), ("C#", 4));
        let (m, c) = note_of(441.0, 440.0);
        assert_eq!(m, 69);
        assert!((c - 3.93).abs() < 0.05);
        let std = [40, 45, 50, 55, 59, 64];
        assert_eq!(nearest_string(45.2, &std), Some(1));
        assert_eq!(nearest_string(70.0, &std), None);
    }
}
