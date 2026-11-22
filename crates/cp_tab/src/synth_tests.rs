//! Des cordes pincees synthetiques (Karplus-Strong) dont la hauteur suit une
//! courbe : un marteau, une glisse, un bend, une glisse d'approche. Le VRAI
//! reseau les ecoute ; les tests verifient ce que la detection en tire.

use crate::analyze::{Posteriors, Progress, analyze};
use crate::model::SR;

/// Une corde : pincee a `t0` (s), sa hauteur (MIDI, fractionnaire) suit
/// `curve` (points (s, hauteur), lineaire entre eux) jusqu'a `t1`.
pub(crate) struct Voice {
    pub t0: f64,
    pub t1: f64,
    pub curve: Vec<(f64, f64)>,
}

fn pitch_at(curve: &[(f64, f64)], t: f64) -> f64 {
    let Some(first) = curve.first() else {
        return 60.0;
    };
    if t <= first.0 {
        return first.1;
    }
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let k = if b.0 > a.0 {
                (t - a.0) / (b.0 - a.0)
            } else {
                1.0
            };
            return a.1 + (b.1 - a.1) * k;
        }
    }
    curve.last().map_or(60.0, |p| p.1)
}

/// Le son, mono, a 22 050 Hz.
pub(crate) fn render(voices: &[Voice], secs: f64) -> Vec<f32> {
    let sr = SR as f64;
    let n = (secs * sr) as usize;
    let mut out = vec![0f32; n];
    let mut seed: u32 = 0x1234_5678;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f64 / u32::MAX as f64) * 2.0 - 1.0
    };
    for v in voices {
        const BUF: usize = 4096;
        let mut buf = vec![0f64; BUF];
        // L'excitation : un bruit adouci, la longueur de la premiere periode.
        let f0 = 440.0 * 2f64.powf((pitch_at(&v.curve, v.t0) - 69.0) / 12.0);
        let l0 = (sr / f0) as usize;
        let mut lp = 0.0;
        for b in buf.iter_mut().take(l0.min(BUF)) {
            lp += 0.6 * (rnd() - lp);
            *b = lp;
        }
        let mut pos = l0.min(BUF - 1);
        let mut prev = 0.0;
        let s0 = (v.t0 * sr) as usize;
        let s1 = ((v.t1 * sr) as usize).min(n);
        for (k, o) in out.iter_mut().enumerate().take(s1).skip(s0) {
            let t = k as f64 / sr;
            let f = 440.0 * 2f64.powf((pitch_at(&v.curve, t) - 69.0) / 12.0);
            let len = (sr / f - 0.5).clamp(2.0, (BUF - 2) as f64);
            let mut rp = pos as f64 - len;
            if rp < 0.0 {
                rp += BUF as f64;
            }
            let i0 = rp.floor() as usize % BUF;
            let i1 = (i0 + 1) % BUF;
            let fr = rp - rp.floor();
            let y = buf[i0] + (buf[i1] - buf[i0]) * fr;
            let fb = 0.5 * (y + prev) * 0.996;
            prev = y;
            buf[pos] = fb;
            pos = (pos + 1) % BUF;
            // Un court fondu a la fin (la note etouffee).
            let fade = ((s1 - k) as f64 / (0.01 * sr)).min(1.0);
            *o += (y * 0.5 * fade) as f32;
        }
    }
    out
}

pub(crate) fn listen(audio: &[f32]) -> Posteriors {
    analyze(audio, 0, &Progress::default()).expect("analyse")
}

#[cfg(test)]
mod probe {
    use super::*;
    use crate::model::{CONTOUR, HOP, MIDI_OFFSET, NOTES};
    use crate::notes::{NoteParams, make_notes};

    fn show(name: &str, p: &Posteriors) {
        let notes = make_notes(p, &NoteParams::default());
        println!("== {name} : {} notes", notes.len());
        for n in &notes {
            println!(
                "  {:>4}..{:>4} ({:.3}..{:.3} s) p{} amp {:.2} peak {:.2}",
                n.start,
                n.end,
                n.t0(),
                n.t1(),
                n.pitch,
                n.amp,
                n.peak
            );
        }
        // La hauteur fine la plus forte, trame par trame (en demi-tons).
        let mut line = String::new();
        for t in 0..p.frames {
            let row = &p.contour[t * CONTOUR..(t + 1) * CONTOUR];
            let (mut bi, mut bv) = (0usize, 0f32);
            for (j, &v) in row.iter().enumerate() {
                if v > bv {
                    bv = v;
                    bi = j;
                }
            }
            let nrow = &p.note[t * NOTES..(t + 1) * NOTES];
            let (mut ni, mut nv) = (0usize, 0f32);
            for (j, &v) in nrow.iter().enumerate() {
                if v > nv {
                    nv = v;
                    ni = j;
                }
            }
            if bv > 0.15 || nv > 0.2 {
                line += &format!(
                    "{t:>4} {:.3}s fin {:.2} ({:.2})  note {} ({:.2})\n",
                    t as f64 * HOP as f64 / SR as f64,
                    bi as f64 / 3.0 + MIDI_OFFSET as f64,
                    bv,
                    ni as i32 + MIDI_OFFSET,
                    nv
                );
            }
        }
        println!("{line}");
    }

    #[test]
    #[ignore]
    fn sonde_des_glisses() {
        // Un marteau : 57 puis 59, sans nouvelle attaque.
        let hammer = vec![Voice {
            t0: 0.2,
            t1: 1.2,
            curve: vec![(0.2, 57.0), (0.6, 57.0), (0.601, 59.0)],
        }];
        // Une glisse 57 -> 62 en 80 ms, case par case.
        let mut c = vec![(0.2, 57.0), (0.6, 57.0)];
        for k in 1..=5 {
            let t = 0.6 + k as f64 * 0.016;
            c.push((t - 0.015, 57.0 + (k - 1) as f64));
            c.push((t, 57.0 + k as f64));
        }
        let slide = vec![Voice {
            t0: 0.2,
            t1: 1.3,
            curve: c,
        }];
        // Un bend d'un ton en 150 ms, tenu, relache.
        let bend = vec![Voice {
            t0: 0.2,
            t1: 1.6,
            curve: vec![
                (0.2, 57.0),
                (0.45, 57.0),
                (0.6, 59.0),
                (1.0, 59.0),
                (1.15, 57.0),
            ],
        }];
        // Une glisse d'approche : pincee trois cases dessous, 60 ms.
        let approach = vec![Voice {
            t0: 0.2,
            t1: 1.2,
            curve: vec![
                (0.2, 54.0),
                (0.214, 54.0),
                (0.215, 55.0),
                (0.229, 55.0),
                (0.23, 56.0),
                (0.244, 56.0),
                (0.245, 57.0),
            ],
        }];
        for (name, v) in [
            ("marteau", hammer),
            ("glisse", slide),
            ("bend", bend),
            ("approche", approach),
        ] {
            let audio = render(&v, 1.8);
            show(name, &listen(&audio));
        }
    }
}

#[cfg(test)]
mod calibre {
    use super::*;
    use crate::model::{CONTOUR, MIDI_OFFSET};

    #[test]
    #[ignore]
    fn sonde_du_la_440() {
        let sr = SR as f64;
        let audio: Vec<f32> = (0..(1.0 * sr) as usize)
            .map(|i| (0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr).sin()) as f32)
            .collect();
        let p = listen(&audio);
        let t = p.frames / 2;
        let row = &p.contour[t * CONTOUR..(t + 1) * CONTOUR];
        let mut v: Vec<(usize, f32)> = row.iter().copied().enumerate().collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (b, x) in v.iter().take(4) {
            println!(
                "case {b} = {:.2} : {:.3}",
                *b as f64 / 3.0 + MIDI_OFFSET as f64,
                x
            );
        }
    }
}

#[cfg(test)]
mod glisses {
    use super::*;
    use crate::cache::Analysis;
    use crate::glide::{Shaped, shape};
    use crate::notes::{NoteParams, make_notes};

    /// Le son ecoute, les notes, puis leur forme (par le contour en octets,
    /// comme dans le cache).
    fn shapes(v: &[Voice], secs: f64) -> Vec<Shaped> {
        let audio = render(v, secs);
        let post = listen(&audio);
        let an = Analysis::new(&post, &audio);
        assert!(an.has_contour());
        let notes = make_notes(&an.posteriors(), &NoteParams::default());
        let mut s = shape(&notes, &an.glide_contour());
        s.sort_by_key(|x| x.note.start);
        s
    }

    fn dump(s: &[Shaped]) -> String {
        s.iter()
            .map(|x| {
                format!(
                    "{}..{} p{} bend {:?} in {} slid {}",
                    x.note.start, x.note.end, x.note.pitch, x.bend, x.slide_in, x.slid
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn un_bend_et_son_relachement_font_une_seule_note() {
        let s = shapes(
            &[Voice {
                t0: 0.2,
                t1: 1.6,
                curve: vec![
                    (0.2, 57.0),
                    (0.45, 57.0),
                    (0.6, 59.0),
                    (1.0, 59.0),
                    (1.15, 57.0),
                ],
            }],
            1.8,
        );
        assert_eq!(s.len(), 1, "{}", dump(&s));
        let n = &s[0];
        assert_eq!(n.note.pitch, 57);
        let top = n.bend.iter().map(|p| p.1).fold(0f32, f32::max);
        assert!((top - 2.0).abs() <= 0.35, "{}", dump(&s));
        // Relache : la courbe revient pres de 0 a la fin.
        let last = n.bend.last().unwrap().1;
        assert!(last.abs() <= 0.4, "{}", dump(&s));
        assert_eq!(n.slide_in, 0);
    }

    #[test]
    fn une_glisse_passe_par_les_cases() {
        let mut c = vec![(0.2, 57.0), (0.6, 57.0)];
        for k in 1..=5 {
            let t = 0.6 + k as f64 * 0.016;
            c.push((t - 0.015, 57.0 + (k - 1) as f64));
            c.push((t, 57.0 + k as f64));
        }
        let s = shapes(
            &[Voice {
                t0: 0.2,
                t1: 1.3,
                curve: c,
            }],
            1.5,
        );
        assert_eq!(s.len(), 2, "{}", dump(&s));
        assert_eq!((s[0].note.pitch, s[1].note.pitch), (57, 62));
        assert!(s[1].slid, "{}", dump(&s));
        assert!(s[0].bend.is_empty() && s[1].bend.is_empty(), "{}", dump(&s));
        // La note d'avant sonne jusqu'a l'arrivee.
        assert_eq!(s[0].note.end, s[1].note.start);
    }

    #[test]
    fn une_glisse_d_approche_vient_d_en_dessous() {
        let s = shapes(
            &[Voice {
                t0: 0.2,
                t1: 1.2,
                curve: vec![
                    (0.2, 54.0),
                    (0.229, 54.0),
                    (0.23, 55.0),
                    (0.259, 55.0),
                    (0.26, 56.0),
                    (0.289, 56.0),
                    (0.29, 57.0),
                ],
            }],
            1.4,
        );
        assert_eq!(s.len(), 1, "{}", dump(&s));
        assert_eq!(s[0].note.pitch, 57);
        assert!((-3..=-2).contains(&s[0].slide_in), "{}", dump(&s));
        // La note commence au pincement (0,2 s = trame 17).
        assert!(s[0].note.start <= 20, "{}", dump(&s));
    }

    #[test]
    fn un_marteau_et_des_notes_pincees_restent_simples() {
        // Un marteau d'un ton (d'un coup), puis deux notes pincees.
        let s = shapes(
            &[
                Voice {
                    t0: 0.2,
                    t1: 1.0,
                    curve: vec![(0.2, 57.0), (0.6, 57.0), (0.601, 59.0)],
                },
                Voice {
                    t0: 1.1,
                    t1: 1.5,
                    curve: vec![(1.1, 52.0)],
                },
                Voice {
                    t0: 1.5,
                    t1: 2.0,
                    curve: vec![(1.5, 55.0)],
                },
            ],
            2.2,
        );
        assert_eq!(s.len(), 4, "{}", dump(&s));
        for x in &s {
            assert!(x.bend.is_empty(), "{}", dump(&s));
            assert!(!x.slid, "{}", dump(&s));
            assert_eq!(x.slide_in, 0, "{}", dump(&s));
        }
    }

    #[test]
    fn un_accord_tenu_ne_bouge_pas() {
        let v: Vec<Voice> = [45.0, 52.0, 57.0, 61.0, 64.0]
            .iter()
            .map(|&p| Voice {
                t0: 0.2,
                t1: 1.4,
                curve: vec![(0.2, p)],
            })
            .collect();
        let s = shapes(&v, 1.6);
        assert!(s.len() >= 4, "{}", dump(&s));
        for x in &s {
            assert!(
                x.bend.is_empty() && !x.slid && x.slide_in == 0,
                "{}",
                dump(&s)
            );
        }
    }
}
