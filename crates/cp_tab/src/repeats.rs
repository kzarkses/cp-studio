//! Les reprises : un passage joue plusieurs fois de suite s'ecrit une fois,
//! entre deux barres de reprise (« x3 » au-dessus quand il se joue plus de
//! deux fois), comme dans les methodes de guitare.
//!
//! Deux mesures sont les memes si tout ce qui s'y ecrit est pareil : les
//! signes (place dans la mesure, duree, liaisons, triolets) et leurs notes
//! (corde, case, technique, marques, bend). Un passage de une a quatre
//! mesures se reprend s'il revient tel quel au moins une fois juste apres,
//! sans note liee par-dessus ses bords ; on prend celui qui couvre le plus
//! de mesures. Les mesures de silence ne se reprennent pas.
//!
//! La partition reprise est une prise COMPACTEE : les copies retirees, les
//! notes d'apres ramenees d'autant, les mesures reecrites une a une (un
//! changement par mesure : les mesures partielles restent ce qu'elles
//! sont). Elle se met en pages comme une autre.

use crate::doc::TNote;
use crate::engrave::Engraved;
use crate::meter::Meter;

/// Les mesures `[m0, m0 + len)` jouees `times` fois.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Repeat {
    pub m0: i64,
    pub len: i64,
    pub times: u32,
}

/// L'empreinte d'une mesure ; `None` pour une mesure de silence. Un
/// changement de tempo dans la mesure la rend unique (une reprise le
/// perdrait).
fn bar_key(eng: &Engraved, notes: &[TNote], tempos: &[(f64, f64)], m: i64) -> Option<String> {
    let b0 = eng.bar_start(m);
    let mut key = format!("{:?}|{}|", eng.sig_of(m), eng.bar_len(m));
    let (q0, q1) = (eng.qn(b0), eng.qn(eng.bar_start(m + 1)));
    // Le tempo du debut (celui de la tete de la partition) ne compte pas.
    for t in tempos
        .iter()
        .filter(|t| t.0 >= q0 - 1e-9 && t.0 < q1 - 1e-9 && t.0 > eng.origin_qn + 1e-9)
    {
        key += &format!("T{m}:{}:{};", t.0, t.1);
    }
    let mut any = false;
    for sg in eng.signs.iter().filter(|s| s.measure == m) {
        let ev = &eng.events[sg.ev];
        any |= !ev.notes.is_empty();
        let mut ns: Vec<String> = ev
            .notes
            .iter()
            .filter_map(|&i| notes.get(i))
            .map(|n| {
                let bend: Vec<(i64, i32)> = n
                    .bend
                    .iter()
                    .map(|&(q, st)| ((q * 96.0).round() as i64, (st * 4.0).round() as i32))
                    .collect();
                format!(
                    "{}:{}:{:?}:{}:{:?}",
                    n.string, n.fret, n.tech, n.marks, bend
                )
            })
            .collect();
        ns.sort();
        key += &format!(
            "{}/{}/{}/{}/{}/{}/{}/{}/{}/{:?}/{:?}/{:?};",
            sg.voice,
            sg.start - b0,
            sg.dur,
            sg.ty,
            sg.dots,
            sg.tuplet,
            sg.tie_in,
            sg.tie_out,
            sg.first,
            ev.lig_in,
            ev.lig_out,
            ns
        );
    }
    any.then_some(key)
}

/// Les reprises d'une partition (`tempos` : ses changements de tempo, en
/// noires du projet).
pub fn find(eng: &Engraved, notes: &[TNote], tempos: &[(f64, f64)]) -> Vec<Repeat> {
    let n = eng.measures.max(0);
    let keys: Vec<Option<String>> = (0..n).map(|m| bar_key(eng, notes, tempos, m)).collect();
    // Une note qui arrive liee au debut de la mesure, ou qui en part liee a
    // sa fin.
    let ties_in = |m: i64| {
        eng.signs
            .iter()
            .any(|s| s.measure == m && s.tie_in && s.start == eng.bar_start(m))
    };
    let ties_out = |m: i64| {
        eng.signs
            .iter()
            .any(|s| s.measure == m && s.tie_out && s.start + s.dur == eng.bar_start(m + 1))
    };
    let mut out = Vec::new();
    let mut m = 0i64;
    while m < n {
        // (mesures couvertes, longueur, fois)
        let mut best: Option<(i64, i64, u32)> = None;
        for len in 1..=4i64 {
            if m + 2 * len > n {
                break;
            }
            let block = &keys[m as usize..(m + len) as usize];
            if block.iter().any(Option::is_none) {
                continue;
            }
            // Une note liee par-dessus un bord du passage : pas de reprise.
            if ties_in(m) || ties_out(m + len - 1) {
                continue;
            }
            let mut k = 1u32;
            while m + (k as i64 + 1) * len <= n {
                let a = (m + k as i64 * len) as usize;
                if keys[a..a + len as usize] != *block {
                    break;
                }
                k += 1;
            }
            if k >= 2 {
                let cover = len * k as i64;
                if best.is_none_or(|b| cover > b.0) {
                    best = Some((cover, len, k));
                }
            }
        }
        match best {
            Some((cover, len, times)) => {
                out.push(Repeat { m0: m, len, times });
                m += cover;
            }
            None => m += 1,
        }
    }
    out
}

/// Une prise compactee : ses notes, ses mesures, et ses reprises (en
/// mesures de la prise compactee).
#[derive(Clone, Debug)]
pub struct Compact {
    pub notes: Vec<TNote>,
    pub meter: Meter,
    pub repeats: Vec<Repeat>,
}

/// Retire les copies des reprises : les notes d'apres reculent d'autant (et
/// les changements de tempo avec elles).
pub fn compact(eng: &Engraved, notes: &[TNote], tempos: &[(f64, f64)], reps: &[Repeat]) -> Compact {
    let n = eng.measures.max(0);
    let g = eng.grid.max(1) as f64;
    let skipped = |m: i64| {
        reps.iter()
            .any(|r| m >= r.m0 + r.len && m < r.m0 + r.len * r.times as i64)
    };
    // Le debut de chaque mesure gardee, en noires, une fois compactee.
    let mut new_start: Vec<Option<f64>> = vec![None; n as usize];
    let mut changes: Vec<(f64, (i32, i32))> = Vec::new();
    let mut at = eng.origin_qn;
    let mut new_m = 0i64;
    let mut map: Vec<i64> = vec![-1; n as usize];
    for m in 0..n {
        if skipped(m) {
            continue;
        }
        new_start[m as usize] = Some(at);
        map[m as usize] = new_m;
        changes.push((at, eng.sig_of(m)));
        at += eng.bar_len(m) as f64 / g;
        new_m += 1;
    }
    // Les tempos : celui d'avant la prise reste, ceux d'une mesure gardee
    // reculent avec elle.
    let mut tout: Vec<(f64, f64)> = tempos
        .iter()
        .copied()
        .filter(|t| t.0 < eng.origin_qn + 1e-9)
        .collect();
    for &(q, bpm) in tempos.iter().filter(|t| t.0 >= eng.origin_qn + 1e-9) {
        let c = eng.cell_of(q);
        let m = eng.measure_of(c);
        if let Some(s) = new_start.get(m as usize).copied().flatten() {
            tout.push((q + s - eng.qn(eng.bar_start(m)), bpm));
        }
    }
    let mut out: Vec<TNote> = Vec::new();
    for nt in notes {
        // La case ou la partition a pose la note (triolets compris) : sinon
        // une note un peu en avance changeait de mesure, et se perdait.
        let c = eng.cell_of(nt.qn0);
        if c < 0 {
            continue;
        }
        let m = eng.measure_of(c);
        let Some(s) = new_start.get(m as usize).copied().flatten() else {
            continue;
        };
        let d = s - eng.qn(eng.bar_start(m));
        let mut x = nt.clone();
        x.qn0 += d;
        x.qn1 += d;
        out.push(x);
    }
    let repeats = reps
        .iter()
        .map(|r| Repeat {
            m0: map[r.m0 as usize],
            len: r.len,
            times: r.times,
        })
        .collect();
    Compact {
        notes: out,
        meter: Meter::new(changes).with_tempos(tout),
        repeats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engrave::engrave;

    fn note(q: f64, len: f64, string: usize, fret: i32) -> TNote {
        TNote {
            qn0: q,
            qn1: q + len,
            pitch: [40, 45, 50, 55, 59, 64][string] + fret,
            string,
            fret,
            vel: 90,
            ..Default::default()
        }
    }

    /// Une mesure de croches : un motif.
    fn bar(m: usize, frets: [i32; 4]) -> Vec<TNote> {
        (0..8)
            .map(|k| note(m as f64 * 4.0 + k as f64 * 0.5, 0.5, 3, frets[k % 4]))
            .collect()
    }

    #[test]
    fn un_riff_repris_s_ecrit_une_fois() {
        // A A A B C D C D : A x3, puis CD x2 (B seule).
        let (a, b, c, d) = ([0, 2, 4, 5], [7, 5, 4, 2], [0, 0, 2, 2], [4, 4, 5, 5]);
        let mut notes = Vec::new();
        for (m, f) in [a, a, a, b, c, d, c, d].into_iter().enumerate() {
            notes.extend(bar(m, f));
        }
        crate::doc::sort(&mut notes);
        let eng = engrave(&notes, 0.0, (4, 4), 4);
        let reps = find(&eng, &notes, &[]);
        assert_eq!(
            reps,
            vec![
                Repeat {
                    m0: 0,
                    len: 1,
                    times: 3
                },
                Repeat {
                    m0: 4,
                    len: 2,
                    times: 2
                },
            ]
        );
        let c = compact(&eng, &notes, &[], &reps);
        // Il reste A, B, C, D : quatre mesures, les reprises renumerotees.
        assert_eq!(c.notes.len(), 32);
        assert_eq!(
            c.repeats,
            vec![
                Repeat {
                    m0: 0,
                    len: 1,
                    times: 3
                },
                Repeat {
                    m0: 2,
                    len: 2,
                    times: 2
                },
            ]
        );
        let e2 =
            crate::engrave::engrave_meter(&c.notes, 0.0, &c.meter, 4, crate::key::detect(&c.notes));
        assert_eq!(e2.measures, 4);
        // B est maintenant la deuxieme mesure.
        let first_b = c.notes.iter().find(|n| n.fret == 7).unwrap();
        assert!((first_b.qn0 - 4.0).abs() < 1e-9);
        // En MusicXML : deux barres qui ouvrent, deux qui ferment (x3, x2).
        let spec = crate::score::ScoreSpec {
            title: "R".into(),
            origin_qn: 0.0,
            sig: (4, 4),
            meter: Some(c.meter.clone()),
            repeats: c.repeats.clone(),
            bpm: 100.0,
            grid: 4,
            tuning: crate::fret::Tuning::standard(),
        };
        let (xml, st) = crate::score::write_musicxml(&c.notes, &spec, 0);
        assert_eq!(st.measures, 4);
        assert_eq!(xml.matches("direction=\"forward\"").count(), 2);
        assert!(xml.contains("direction=\"backward\" times=\"3\""));
        assert!(xml.contains("direction=\"backward\" times=\"2\""));
    }

    #[test]
    fn une_note_liee_au_bord_empeche_la_reprise() {
        // Une ronde tenue quatre mesures : les mesures du milieu se
        // ressemblent (liees des deux cotes), mais une reprise couperait la
        // liaison.
        let notes = vec![note(0.0, 16.0, 2, 0)];
        let eng = engrave(&notes, 0.0, (4, 4), 4);
        assert_eq!(eng.measures, 4);
        assert!(find(&eng, &notes, &[]).is_empty());
        // Le silence ne se reprend pas.
        let eng = engrave(
            &[note(0.0, 1.0, 2, 0), note(16.0, 1.0, 2, 0)],
            0.0,
            (4, 4),
            4,
        );
        assert!(find(&eng, &[note(0.0, 1.0, 2, 0), note(16.0, 1.0, 2, 0)], &[]).is_empty());
    }
}
