//! La boucle de test : deux mesures a 120 BPM, synthetisees.
//!
//! Elle sert a entendre le moteur dans REAPER sans dependre d'un fichier. Les
//! sons sont poses dans un tampon CIRCULAIRE : la queue d'un coup qui depasse la
//! fin de la boucle revient au debut. La jointure est donc continue par
//! construction ; un clic a la boucle serait celui du moteur, pas de la matiere.

use std::f64::consts::TAU;

/// Rend (matiere stereo entrelacee, frames) au taux `srate`.
pub fn synth(srate: f64) -> (Vec<f32>, usize) {
    let beat = 0.5; // 120 BPM
    let frames = (8.0 * beat * srate).round() as usize;
    let mut l = vec![0.0f64; frames];
    let mut r = vec![0.0f64; frames];
    let mut add = |start_s: f64, dur_s: f64, pan: f64, f: &mut dyn FnMut(f64) -> f64| {
        let s0 = (start_s * srate).round() as usize;
        let n = (dur_s * srate) as usize;
        for i in 0..n {
            let v = f(i as f64 / srate);
            let k = (s0 + i) % frames;
            l[k] += v * (1.0 - pan.max(0.0));
            r[k] += v * (1.0 + pan.min(0.0));
        }
    };

    // Grosse caisse sur chaque temps : un sinus qui descend de 120 a 45 Hz.
    for b in 0..8 {
        let mut ph = 0.0;
        add(b as f64 * beat, 0.9, 0.0, &mut |t| {
            let f = 45.0 + 75.0 * (-t / 0.03).exp();
            ph += TAU * f / srate;
            let a = (t / 0.002).min(1.0) * (-t / 0.18).exp();
            0.8 * a * ph.sin()
        });
    }
    // Charleston a contretemps : un bruit derive (aigu), tres court.
    let mut x: u32 = 0x1234_5678;
    for b in 0..8 {
        let mut prev = 0.0;
        let pan = if b % 2 == 0 { -0.3 } else { 0.3 };
        add(b as f64 * beat + beat / 2.0, 0.2, pan, &mut |t| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (x >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0;
            let hp = n - prev;
            prev = n;
            0.12 * hp * (-t / 0.03).exp()
        });
    }
    // Une basse : la, la, do, sol, une note par demi-mesure.
    for (i, &f) in [55.0, 55.0, 65.41, 49.0].iter().enumerate() {
        let start = i as f64 * 2.0 * beat;
        let dur = 2.0 * beat - 0.05;
        let mut ph = 0.0;
        add(start, dur, 0.0, &mut |t| {
            ph += TAU * f / srate;
            let a = (t / 0.005).min(1.0) * ((dur - t) / 0.02).clamp(0.0, 1.0);
            0.25 * a * (ph.sin() + 0.3 * (2.0 * ph).sin())
        });
    }

    // Crete a -6 dB.
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f64, |m, v| m.max(v.abs()))
        .max(1e-9);
    let g = 0.5 / peak;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        out.push((l[i] * g) as f32);
        out.push((r[i] * g) as f32);
    }
    (out, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_boucle_est_continue_a_sa_jointure() {
        let (s, frames) = synth(48000.0);
        assert_eq!(frames, 192_000);
        // Le saut a la jointure (dernier -> premier) ne depasse pas le plus
        // grand saut d'un echantillon au suivant dans la boucle elle-meme.
        let mut max_step = 0.0f32;
        for i in 1..frames {
            for c in 0..2 {
                max_step = max_step.max((s[i * 2 + c] - s[(i - 1) * 2 + c]).abs());
            }
        }
        for c in 0..2 {
            let j = (s[c] - s[(frames - 1) * 2 + c]).abs();
            assert!(j <= max_step, "jointure {j} > {max_step}");
        }
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 1e-3);
    }
}
