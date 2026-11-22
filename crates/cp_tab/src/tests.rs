//! La chaine verifiee hors de REAPER. Le reseau est confronte au VRAI modele
//! (onnxruntime, tests/fixtures/bp_window.bin, tools/make_fixture.py) sur un
//! signal refait ici au bit pres.

use crate::analyze::{Progress, analyze};
use crate::fret::{Spot, Tuning, assign};
use crate::model::{CONTOUR, NOTES, SR, WIN_FRAMES, WIN_SAMPLES, Work, run_window};
use crate::notes::{NoteParams, make_notes, min_len_frames};
use crate::resample;
use crate::score::{ScoreSpec, TabNote, musicxml};

/// Le signal de tools/make_fixture.py : trois dents de scie (110, 165,
/// 330 Hz) qui se chevauchent, en operations exactes.
fn test_signal() -> Vec<f32> {
    let mut x = vec![0f64; WIN_SAMPLES];
    let notes = [
        (110i64, 0i64, 11025i64, 0.3f64),
        (165, 8820, 26460, 0.25),
        (330, 22050, 41895, 0.2),
    ];
    for (f, a, b, amp) in notes {
        for (i, v) in x.iter_mut().enumerate() {
            let i = i as i64;
            if i >= a && i < b {
                let ph = ((i * f) % SR as i64) as f64 / SR as f64;
                let saw = 2.0 * ph - 1.0;
                let env = 1.0 - (i - a) as f64 / (b - a) as f64;
                *v += amp * saw * env;
            }
        }
    }
    x.iter().map(|&v| v as f32).collect()
}

struct Fixture {
    frames: Vec<usize>,
    sums: [f64; 3],
    mats: [Vec<f32>; 3],
}

fn fixture() -> Fixture {
    let b = include_bytes!("../tests/fixtures/bp_window.bin");
    assert_eq!(&b[..4], b"CPBF");
    let kept = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let mut at = 8;
    let widths = [NOTES, NOTES, CONTOUR];
    let mut sums = [0f64; 3];
    let mut mats: [Vec<f32>; 3] = Default::default();
    for k in 0..3 {
        sums[k] = f64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        at += 8;
        let n = kept * widths[k];
        mats[k] = b[at..at + n * 4]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        at += n * 4;
    }
    assert_eq!(at, b.len());
    Fixture {
        frames: (0..WIN_FRAMES).step_by(4).collect(),
        sums,
        mats,
    }
}

fn max_diff(ours: &[f32], width: usize, fx: &Fixture, k: usize) -> f32 {
    let mut m = 0f32;
    for (row, &t) in fx.frames.iter().enumerate() {
        for f in 0..width {
            let d = (ours[t * width + f] - fx.mats[k][row * width + f]).abs();
            m = m.max(d);
        }
    }
    m
}

#[test]
fn le_reseau_rend_ce_que_rend_onnxruntime() {
    let x = test_signal();
    let mut w = Work::new();
    let mut note = vec![0f32; WIN_FRAMES * NOTES];
    let mut onset = vec![0f32; WIN_FRAMES * NOTES];
    let mut contour = vec![0f32; WIN_FRAMES * CONTOUR];
    run_window(&mut w, &x, &mut note, &mut onset, &mut contour);
    let fx = fixture();
    let dn = max_diff(&note, NOTES, &fx, 0);
    let do_ = max_diff(&onset, NOTES, &fx, 1);
    let dc = max_diff(&contour, CONTOUR, &fx, 2);
    let s = |v: &[f32]| v.iter().map(|&x| x as f64).sum::<f64>();
    eprintln!(
        "ecarts max : note {dn:e}, onset {do_:e}, contour {dc:e} ; sommes {:.4}/{:.4} {:.4}/{:.4} {:.4}/{:.4}",
        s(&note),
        fx.sums[0],
        s(&onset),
        fx.sums[1],
        s(&contour),
        fx.sums[2]
    );
    assert!(
        dn < 1e-4 && do_ < 1e-4 && dc < 1e-4,
        "le portage s'ecarte du modele"
    );
    for (k, v) in [&note, &onset, &contour].iter().enumerate() {
        assert!(
            (s(v) - fx.sums[k]).abs() < 1e-2,
            "somme de la sortie {k} : {} contre {}",
            s(v),
            fx.sums[k]
        );
    }
}

#[test]
fn les_trois_notes_du_signal_sont_retrouvees() {
    let x = test_signal();
    let p = Progress::default();
    let post = analyze(&x, 2, &p).expect("pas annulee");
    assert_eq!(post.frames, WIN_SAMPLES / 256 + 1);
    let notes = make_notes(&post, &NoteParams::default());
    let found: Vec<(i32, f64, f64)> = notes.iter().map(|n| (n.pitch, n.t0(), n.t1())).collect();
    eprintln!("notes : {found:?}");
    // A2 (110 Hz) des 0 s, E3 (165 Hz) a 0,4 s, E4 (330 Hz) a 1,0 s.
    for (pitch, t) in [(45, 0.0), (52, 0.4), (64, 1.0)] {
        assert!(
            found
                .iter()
                .any(|&(p, t0, _)| p == pitch && (t0 - t).abs() < 0.08),
            "note {pitch} a {t} s introuvable"
        );
    }
}

#[test]
fn min_len_arrondit_comme_numpy() {
    assert_eq!(min_len_frames(127.7), 11);
    assert_eq!(min_len_frames(0.0), 0);
}

#[test]
fn le_reechantillonnage_garde_la_hauteur_et_l_amplitude() {
    for from in [44100.0, 48000.0, 96000.0] {
        let n = (from as usize) / 2;
        let f = 440.0;
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / from).sin() as f32 * 0.5)
            .collect();
        let y = resample::resample(&x, from, SR as f64);
        assert!((y.len() as f64 - n as f64 * SR as f64 / from).abs() <= 1.0);
        // Au milieu (loin des bords) : l'amplitude et la frequence.
        let mid = &y[y.len() / 4..3 * y.len() / 4];
        let peak = mid.iter().fold(0f32, |m, &v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{from} : crete {peak}");
        let crossings = mid.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let secs = mid.len() as f64 / SR as f64;
        let measured = crossings as f64 / secs;
        assert!((measured - f).abs() < 3.0, "{from} : {measured} Hz");
    }
    // Un son au-dessus du nouveau Nyquist disparait (pas de repliement).
    let x: Vec<f32> = (0..48000)
        .map(|i| (2.0 * std::f64::consts::PI * 15000.0 * i as f64 / 48000.0).sin() as f32)
        .collect();
    let y = resample::resample(&x, 48000.0, SR as f64);
    let mid = &y[y.len() / 4..3 * y.len() / 4];
    let peak = mid.iter().fold(0f32, |m, &v| m.max(v.abs()));
    assert!(peak < 0.01, "15 kHz replie : crete {peak}");
}

#[test]
fn le_doigte_reste_en_position() {
    let t = Tuning::standard();
    // La gamme de do majeur, une octave a partir de C3 (48), vite : une
    // position, pas un saut par note.
    let scale = [48, 50, 52, 53, 55, 57, 59, 60];
    let groups: Vec<Vec<i32>> = scale.iter().map(|&p| vec![p]).collect();
    let gaps = vec![0.15; scale.len()];
    let r = assign(&groups, &gaps, &t);
    let frets: Vec<i32> = r.iter().map(|g| g[0].unwrap().fret).collect();
    let fretted: Vec<i32> = frets.iter().copied().filter(|&f| f > 0).collect();
    let span = fretted.iter().max().unwrap() - fretted.iter().min().unwrap();
    eprintln!(
        "gamme : {:?}",
        r.iter().map(|g| g[0].unwrap()).collect::<Vec<_>>()
    );
    assert!(span <= 4, "la gamme saute de position : {frets:?}");
    // Un accord de mi majeur ouvert : six cordes distinctes.
    let e = vec![vec![40, 47, 52, 56, 59, 64]];
    let r = assign(&e, &[0.0], &t);
    let spots: Vec<Spot> = r[0].iter().map(|s| s.unwrap()).collect();
    let mut strings: Vec<usize> = spots.iter().map(|s| s.string).collect();
    strings.sort();
    strings.dedup();
    assert_eq!(strings.len(), 6);
    assert_eq!(spots.iter().map(|s| s.fret).max(), Some(2));
    // Sept notes ensemble : six se jouent, une tombe.
    let r = assign(&[vec![40, 45, 50, 55, 59, 64, 69]], &[0.0], &t);
    assert_eq!(r[0].iter().filter(|s| s.is_some()).count(), 6);
    // Une note rejouee reste a sa place.
    let r = assign(&[vec![57], vec![57], vec![57]], &[0.0, 0.1, 0.1], &t);
    assert!(r[0][0] == r[1][0] && r[1][0] == r[2][0]);
}

/// La somme des durees de chaque mesure, par portee.
fn measure_sums(xml: &str) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for m in xml.split("<measure number=").skip(1) {
        let body = m.split("</measure>").next().unwrap();
        let mut halves = body.split("<backup>");
        let sum = |s: &str| -> i64 {
            s.split("<note>")
                .skip(1)
                .filter(|n| !n.starts_with("<chord/>"))
                .map(|n| {
                    let d = n.split("<duration>").nth(1).unwrap();
                    d.split('<').next().unwrap().parse::<i64>().unwrap()
                })
                .sum()
        };
        let a = sum(halves.next().unwrap());
        let b = sum(halves.next().unwrap_or(""));
        out.push((a, b));
    }
    out
}

#[test]
fn la_partition_tient_dans_ses_mesures() {
    let spec = ScoreSpec {
        title: "Riff & test".into(),
        origin_qn: 8.0,
        sig: (4, 4),
        bpm: 112.0,
        grid: 4,
        tuning: Tuning::standard(),
    };
    // Une noire, une croche pointee, un accord qui deborde sur la mesure
    // suivante, une note trop grave (repliee d'une octave).
    let notes = vec![
        TabNote {
            qn0: 8.0,
            qn1: 9.0,
            pitch: 52,
            legato: false,
            vel: 90,
        },
        TabNote {
            qn0: 9.02,
            qn1: 9.74,
            pitch: 55,
            legato: false,
            vel: 90,
        },
        TabNote {
            qn0: 11.0,
            qn1: 13.5,
            pitch: 45,
            legato: false,
            vel: 90,
        },
        TabNote {
            qn0: 11.0,
            qn1: 13.5,
            pitch: 52,
            legato: false,
            vel: 90,
        },
        TabNote {
            qn0: 13.5,
            qn1: 14.0,
            pitch: 28,
            legato: false,
            vel: 90,
        },
    ];
    let (xml, st) = musicxml(&notes, &spec);
    std::fs::write(std::env::temp_dir().join("cp_tab_test.musicxml"), &xml).unwrap();
    assert_eq!(st.measures, 2);
    assert_eq!(st.chords, 4);
    assert_eq!(st.notes, 5);
    assert_eq!(st.dropped, 0);
    for (i, (a, b)) in measure_sums(&xml).into_iter().enumerate() {
        assert_eq!((a, b), (16, 16), "mesure {} : {a} / {b}", i + 1);
    }
    // L'accord coupe a la barre est lie ; le titre est echappe ; la tablature
    // porte corde et case ; la note trop grave est repliee (E2 = corde 6 a vide).
    assert!(xml.contains("<tie type=\"start\"/>") && xml.contains("<tie type=\"stop\"/>"));
    assert!(xml.contains("Riff &amp; test"));
    assert!(xml.contains("<technical><string>6</string><fret>0</fret></technical>"));
    assert!(xml.contains("<sign>TAB</sign>"));
    assert_eq!(xml.matches("<measure ").count(), 2);
    assert_eq!(
        xml.matches("<note>").count(),
        xml.matches("</note>").count()
    );
}

#[test]
fn un_marteau_et_un_tire_s_ecrivent_sur_la_meme_corde() {
    let spec = ScoreSpec {
        title: "legato".into(),
        origin_qn: 0.0,
        sig: (4, 4),
        bpm: 100.0,
        grid: 4,
        tuning: Tuning::standard(),
    };
    // G3 (corde de sol a vide) puis A3 sans attaque (marteau), puis G3 sans
    // attaque (tire) ; enfin une note attaquee (pas de lie).
    let n = |q: f64, p: i32, legato: bool| TabNote {
        qn0: q,
        qn1: q + 0.5,
        pitch: p,
        legato,
        vel: 90,
    };
    let notes = vec![
        n(0.0, 55, false),
        n(0.5, 57, true),
        n(1.0, 55, true),
        n(1.5, 57, false),
    ];
    let (xml, st) = musicxml(&notes, &spec);
    assert_eq!(st.legato, 2);
    assert!(xml.contains("<hammer-on number=\"1\" type=\"start\">H</hammer-on>"));
    assert!(xml.contains("<hammer-on number=\"1\" type=\"stop\"/>"));
    assert!(xml.contains("<pull-off number=\"1\" type=\"start\">P</pull-off>"));
    assert_eq!(xml.matches("<slur type=\"start\"").count(), 2 * 2);
}

// ---------------------------------------------------------------------------
// La tablature editable
// ---------------------------------------------------------------------------

use crate::cache::Analysis;
use crate::doc::{self, MidiQn, Sidecar, TNote, Tech};
use crate::engrave::engrave;

fn tn(q: f64, len: f64, p: i32, legato: bool) -> TabNote {
    TabNote {
        qn0: q,
        qn1: q + len,
        pitch: p,
        legato,
        vel: 90,
    }
}

#[test]
fn la_tablature_se_met_en_mesures() {
    let t = Tuning::standard();
    let notes = vec![
        tn(0.0, 1.0, 52, false),
        tn(1.02, 0.7, 55, false),
        tn(3.0, 2.5, 45, false),
        tn(3.0, 2.5, 52, false),
        tn(5.5, 0.25, 57, false),
        tn(5.75, 0.25, 59, true),
    ];
    let (tab, dropped) = doc::finger(&notes, 4, 112.0, &t);
    assert_eq!(dropped, 0);
    assert_eq!(tab.len(), 6);
    let e = engrave(&tab, 0.0, (4, 4), 4);
    assert_eq!(e.measures, 2);
    assert_eq!(e.chords, 5);
    // Chaque mesure est pleine, exactement.
    for m in 0..e.measures {
        let sum: i64 = e
            .signs
            .iter()
            .filter(|s| s.measure == m)
            .map(|s| s.dur)
            .sum();
        assert_eq!(sum, e.mlen, "mesure {m}");
    }
    // L'accord qui deborde est coupe a la barre et lie.
    assert!(e.signs.iter().any(|s| s.tie_out) && e.signs.iter().any(|s| s.tie_in));
    // Chaque note garde son temps reel (pas la grille).
    assert!((tab[1].qn0 - 1.02).abs() < 1e-9);
}

#[test]
fn les_fantomes_ne_doublent_pas_la_tablature() {
    let t = Tuning::standard();
    let (tab, _) = doc::finger(
        &[tn(0.0, 0.5, 57, false), tn(1.0, 0.5, 60, false)],
        4,
        100.0,
        &t,
    );
    // Le decodage sensible retrouve les deux notes, plus une double croche
    // entre elles.
    let cands = vec![
        tn(0.01, 0.5, 57, false),
        tn(0.5, 0.25, 59, false),
        tn(1.0, 0.5, 60, false),
    ];
    let g = doc::ghosts(&cands, &tab, 4, &t);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].pitch, 59);
    assert_eq!(t.strings[g[0].string] + g[0].fret, 59);
}

#[test]
fn l_aller_retour_avec_le_midi_garde_cordes_liaisons_et_notes_tenues() {
    let t = Tuning::standard();
    let (mut tab, _) = doc::finger(
        &[
            tn(0.0, 0.5, 55, false),
            tn(0.5, 0.5, 57, true),
            tn(1.0, 1.0, 64, false),
        ],
        4,
        100.0,
        &t,
    );
    assert_eq!(tab[1].tech, Tech::Hammer);
    tab[2].locked = true;
    let midi = doc::to_midi(&tab, &t);
    // La corde la plus aigue est le canal 1 (0).
    assert!(
        midi.iter()
            .any(|m| m.pitch == 64 && (m.chan == 0 || m.chan == 1))
    );
    let side = Sidecar::from_text(&Sidecar::of(&tab, &Default::default()).to_text());
    let back = doc::from_midi(&midi, &t, &side);
    assert_eq!(back, tab);
    // Une note ecrite dans REAPER sur un canal hors des cordes : replacee.
    let odd = vec![MidiQn {
        qn0: 2.0,
        qn1: 2.5,
        pitch: 41,
        chan: 9,
        vel: 100,
    }];
    let n = doc::from_midi(&odd, &t, &Sidecar::default());
    assert_eq!((n[0].string, n[0].fret), (0, 1));
}

#[test]
fn un_nouveau_decodage_respecte_les_corrections() {
    let note = |q: f64, p: i32, s: usize, f: i32, locked: bool| TNote {
        qn0: q,
        qn1: q + 0.5,
        pitch: p,
        string: s,
        fret: f,
        vel: 90,
        tech: Tech::Plain,
        locked,
    };
    let old = vec![
        note(0.0, 57, 3, 2, true),
        note(1.0, 59, 4, 0, false),
        note(4.0, 64, 5, 0, false),
    ];
    let fresh = vec![
        note(0.0, 57, 2, 7, false),
        note(2.0, 62, 4, 3, false),
        note(3.0, 60, 4, 1, false),
    ];
    let mut sup = std::collections::HashSet::new();
    sup.insert(fresh[2].key());
    let out = doc::merge(&old, fresh, &sup, Some((0.0, 3.5)));
    let keys: Vec<(f64, i32, bool)> = out.iter().map(|t| (t.qn0, t.pitch, t.locked)).collect();
    // La note tenue reste (pas son double detecte) ; la non tenue du passage
    // part ; l'effacee ne revient pas ; hors du passage, rien ne bouge.
    assert_eq!(
        keys,
        vec![(0.0, 57, true), (2.0, 62, false), (4.0, 64, false)]
    );
}

#[test]
fn l_analyse_se_relit_du_cache() {
    let p = crate::analyze::Posteriors {
        frames: 3,
        note: (0..3 * 88).map(|i| (i % 7) as f32 / 7.0).collect(),
        onset: (0..3 * 88).map(|i| (i % 5) as f32 / 5.0).collect(),
        contour: Vec::new(),
    };
    let audio: Vec<f32> = (0..800).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
    let a = Analysis::new(&p, &audio);
    let b = Analysis::from_bytes(&a.to_bytes()).unwrap();
    assert_eq!(a, b);
    assert!(Analysis::from_bytes(b"CPGA").is_none());
    let back = b.posteriors();
    assert!((back.note[6] - 6.0 / 7.0).abs() < 0.003);
    assert_eq!(b.onset_curve().len(), 3);
    // Sans contour, le decodage marche (sans bends).
    let _ = crate::make_notes(&back, &crate::NoteParams::default());
}
