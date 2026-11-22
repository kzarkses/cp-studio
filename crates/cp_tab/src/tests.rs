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
            ..Default::default()
        },
        TabNote {
            qn0: 9.02,
            qn1: 9.74,
            pitch: 55,
            legato: false,
            vel: 90,
            ..Default::default()
        },
        TabNote {
            qn0: 11.0,
            qn1: 13.5,
            pitch: 45,
            legato: false,
            vel: 90,
            ..Default::default()
        },
        TabNote {
            qn0: 11.0,
            qn1: 13.5,
            pitch: 52,
            legato: false,
            vel: 90,
            ..Default::default()
        },
        TabNote {
            qn0: 13.5,
            qn1: 14.0,
            pitch: 28,
            legato: false,
            vel: 90,
            ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
    let back = doc::from_midi(&midi, &[], &t, &side);
    assert_eq!(back, tab);
    // Une note ecrite dans REAPER sur un canal hors des cordes : replacee.
    let odd = vec![MidiQn {
        qn0: 2.0,
        qn1: 2.5,
        pitch: 41,
        chan: 9,
        vel: 100,
    }];
    let n = doc::from_midi(&odd, &[], &t, &Sidecar::default());
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
        ..Default::default()
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

#[test]
fn la_tab_d_une_prise_suit_son_item() {
    use crate::doc::{TNote, Tech, tab_from_text, tab_to_text};
    use std::collections::HashSet;
    let doc = vec![
        TNote {
            qn0: 8.0,
            qn1: 8.5,
            pitch: 45,
            string: 1,
            fret: 0,
            vel: 90,
            tech: Tech::Plain,
            locked: false,
            ..Default::default()
        },
        TNote {
            qn0: 8.5,
            qn1: 9.0,
            pitch: 47,
            string: 1,
            fret: 2,
            vel: 80,
            tech: Tech::Hammer,
            locked: true,
            ..Default::default()
        },
    ];
    let mut sup = HashSet::new();
    sup.insert(((9.25f64 * 960.0).round() as i64, 50));
    let txt = tab_to_text(&doc, &sup, "0.5 0.3", 8.0);
    // Relue 4 noires plus loin : tout s'est deplace avec l'item.
    let (d2, s2, det) = tab_from_text(&txt, 12.0).expect("relue");
    assert_eq!(det, "0.5 0.3");
    assert_eq!(d2.len(), 2);
    assert!((d2[0].qn0 - 12.0).abs() < 1e-6 && (d2[1].qn1 - 13.0).abs() < 1e-6);
    assert_eq!(d2[1].tech, Tech::Hammer);
    assert!(d2[1].locked && !d2[0].locked);
    assert!(s2.contains(&((13.25f64 * 960.0).round() as i64, 50)));
    assert!(tab_from_text("autre chose", 0.0).is_none());
}

/// Le MIDI modifie par Cedric : une note effacee ne revient ni en fantome ni
/// par un decodage ; une note ajoutee est tenue.
#[test]
fn le_midi_modifie_fait_foi() {
    let t = Tuning::standard();
    let (tab, _) = doc::finger(
        &[tn(0.0, 1.0, 64, false), tn(1.0, 1.0, 67, false)],
        4,
        120.0,
        &t,
    );
    // Dans l'editeur : la seconde note effacee, une autre ajoutee.
    let mut now: Vec<TNote> = vec![tab[0].clone()];
    let mut added = tab[0].clone();
    added.qn0 = 2.0;
    added.qn1 = 3.0;
    added.pitch = 69;
    now.push(added);
    let mut sup = std::collections::HashSet::new();
    doc::reconcile(&tab, &mut now, &mut sup);
    assert!(sup.contains(&tab[1].key()));
    assert!(now[1].locked && !now[0].locked);
    // Le fantome de la note effacee (le meme son, detecte un peu plus tard).
    let mut g = doc::ghosts(&[tn(1.01, 1.0, 67, false)], &now, 4, &t);
    assert_eq!(g.len(), 1);
    doc::drop_suppressed(&mut g, &sup, 4);
    assert!(g.is_empty());
    // Un nouveau decodage ne la ramene pas, et garde l'ajoutee.
    let merged = doc::merge(&now, tab.clone(), &sup, None);
    assert!(merged.iter().all(|n| n.pitch != 67));
    assert!(merged.iter().any(|n| n.pitch == 69));
}

/// Les bends et les glisses font l'aller-retour par le MIDI (pitch bends du
/// canal de la corde, a la resolution de 14 bits), par le fichier de la tab
/// et s'ecrivent en MusicXML.
#[test]
fn bends_et_glisses_font_l_aller_retour() {
    use crate::doc::{CcKind, Tech, midi_events};
    let t = Tuning::standard();
    // Sol (corde 3, case 7 = D4 62) : approche d'en dessous de 2, puis bend
    // d'un ton relache ; ensuite une glisse 7 -> 9 sur la meme corde.
    let a = TNote {
        qn0: 4.0,
        qn1: 6.0,
        pitch: 62,
        string: 2,
        fret: 7,
        vel: 90,
        bend: vec![
            (0.0, -2.0),
            (0.1, 0.0),
            (0.5, 0.0),
            (0.8, 2.0),
            (1.3, 2.0),
            (1.6, 0.0),
        ],
        ..Default::default()
    };
    let b = TNote {
        qn0: 6.0,
        qn1: 7.0,
        pitch: 64,
        string: 2,
        fret: 9,
        vel: 90,
        tech: Tech::Slide,
        ..Default::default()
    };
    let docs = vec![a.clone(), b.clone()];
    assert_eq!(a.slide_in(), -2);
    let bm = a.bend_mark().expect("le bend");
    assert_eq!(bm.amount, 2.0);
    assert!(bm.release.is_some());
    let notes = doc::to_midi(&docs, &t);
    let ev = midi_events(&docs, &t);
    // Le MIDI : 14 bits, etendue de 12 demi-tons.
    let q14 = |v: f32| {
        let x = (8192.0 + v / doc::BEND_RANGE * 8192.0)
            .round()
            .clamp(0.0, 16383.0);
        (x - 8192.0) / 8192.0 * doc::BEND_RANGE
    };
    let bends: Vec<(f64, u8, f32)> = ev
        .iter()
        .filter_map(|e| match e.kind {
            CcKind::Bend(v) => Some((e.qn + e.tick as f64 / 960.0, e.chan, q14(v))),
            _ => None,
        })
        .collect();
    // La glisse ne se repince pas : la pedale legato l'entoure.
    assert!(
        ev.iter()
            .any(|e| e.kind == CcKind::Legato(true) && (e.qn - 6.0).abs() < 1e-9)
    );
    let side = doc::Sidecar::of(&docs, &Default::default());
    let back = doc::from_midi(&notes, &bends, &t, &side);
    assert_eq!(back.len(), 2);
    assert_eq!(back[1].tech, Tech::Slide);
    assert!(back[1].bend.is_empty(), "{:?}", back[1].bend);
    // La courbe de la premiere revient, sans la rampe de la glisse.
    assert_eq!(back[0].slide_in(), -2, "{:?}", back[0].bend);
    let bm2 = back[0].bend_mark().expect("le bend relu");
    assert_eq!(bm2.amount, 2.0);
    assert!(bm2.release.is_some());
    for &(q, v) in &a.bend {
        let w = doc::value_at(&back[0].bend, q);
        assert!(
            (w - v).abs() < 0.1,
            "a {q} : {w} pour {v} ({:?})",
            back[0].bend
        );
    }
    assert!(
        back[0].bend.iter().all(|p| p.0 < 2.0 - 0.1),
        "{:?}",
        back[0].bend
    );
    // Le fichier de la tab.
    let txt = doc::tab_to_text(&docs, &Default::default(), "", 4.0);
    let (d2, _, _) = doc::tab_from_text(&txt, 4.0).unwrap();
    assert_eq!(d2[0].bend.len(), a.bend.len());
    assert_eq!(d2[1].tech, Tech::Slide);
    // Le MusicXML.
    let spec = ScoreSpec {
        title: "t".into(),
        origin_qn: 4.0,
        sig: (4, 4),
        bpm: 120.0,
        grid: 4,
        tuning: t.clone(),
    };
    let (xml, _) = crate::score::write_musicxml(&docs, &spec, 0);
    assert!(xml.contains("<scoop/>"), "{xml}");
    assert!(xml.contains("<bend-alter>2</bend-alter>"), "{xml}");
    assert!(xml.contains("<release/>"), "{xml}");
    assert!(xml.contains("<slide type=\"start\""), "{xml}");
    assert!(xml.contains("<slide type=\"stop\""), "{xml}");
}

/// Une glisse detectee reste sur sa corde au doigte.
#[test]
fn une_glisse_reste_sur_sa_corde() {
    use crate::doc::Tech;
    let t = Tuning::standard();
    let n = vec![
        TabNote {
            qn0: 0.0,
            qn1: 1.0,
            pitch: 57,
            vel: 90,
            ..Default::default()
        },
        TabNote {
            qn0: 1.0,
            qn1: 2.0,
            pitch: 64,
            vel: 90,
            slid: true,
            ..Default::default()
        },
    ];
    let (tab, _) = doc::finger(&n, 4, 120.0, &t);
    assert_eq!(tab.len(), 2);
    assert_eq!(tab[0].string, tab[1].string, "{tab:?}");
    assert_eq!(tab[1].tech, Tech::Slide);
}

/// Des notes simples sur la corde de Sol (doigtees a la main).
fn tri_note(q: f64, len: f64, fret: i32) -> TNote {
    TNote {
        qn0: q,
        qn1: q + len,
        pitch: 55 + fret,
        string: 3,
        fret,
        vel: 90,
        ..Default::default()
    }
}

#[test]
fn les_triolets_se_reconnaissent_et_s_ecrivent() {
    let third = 1.0 / 3.0;
    let mut tab = vec![
        // Temps 1 : trois croches de triolet (un peu en avance, en retard).
        tri_note(0.01, third, 0),
        tri_note(third - 0.02, third, 2),
        tri_note(2.0 * third + 0.015, third, 4),
        // Temps 2 : deux croches droites.
        tri_note(1.0, 0.5, 5),
        tri_note(1.5, 0.5, 7),
        // Temps 3 : le shuffle, une noire de triolet puis une croche.
        tri_note(2.0, 2.0 * third, 5),
        tri_note(2.0 + 2.0 * third, third, 7),
        // Temps 4 : quatre doubles croches.
        tri_note(3.0, 0.25, 0),
        tri_note(3.25, 0.25, 2),
        tri_note(3.5, 0.25, 4),
        tri_note(3.75, 0.25, 5),
    ];
    doc::sort(&mut tab);
    let e = engrave(&tab, 0.0, (4, 4), 4);
    assert_eq!(e.triplets.iter().copied().collect::<Vec<_>>(), vec![0, 2]);
    assert_eq!(e.grid, 12);
    assert_eq!(e.measures, 1);
    let sum: i64 = e.signs.iter().map(|s| s.dur).sum();
    assert_eq!(sum, e.mlen);
    let beat = |b: i64| -> Vec<(&str, u8, bool, bool)> {
        e.signs
            .iter()
            .filter(|s| s.start / 12 == b)
            .map(|s| (s.ty, s.tuplet, s.tup_start, s.tup_stop))
            .collect()
    };
    assert_eq!(
        beat(0),
        vec![
            ("eighth", 3, true, false),
            ("eighth", 3, false, false),
            ("eighth", 3, false, true)
        ]
    );
    assert_eq!(beat(1), vec![("eighth", 0, false, false); 2]);
    assert_eq!(
        beat(2),
        vec![("quarter", 3, true, false), ("eighth", 3, false, true)]
    );
    assert_eq!(beat(3), vec![("16th", 0, false, false); 4]);
    // Le MusicXML porte le 3 : sept signes de triolet, deux crochets.
    let spec = ScoreSpec {
        title: "t".into(),
        origin_qn: 0.0,
        sig: (4, 4),
        bpm: 100.0,
        grid: 4,
        tuning: Tuning::standard(),
    };
    let (x, _) = crate::score::write_musicxml(&tab, &spec, 0);
    assert_eq!(x.matches("<actual-notes>3</actual-notes>").count(), 2 * 5);
    assert_eq!(x.matches("<tuplet type=\"start\"").count(), 2 * 2);
    assert!(x.contains("<divisions>12</divisions>"));
}

#[test]
fn des_doubles_croches_jouees_ne_deviennent_pas_des_triolets() {
    // Le jeu d'un humain : +-25 ms a 100 BPM (0,04 noire).
    let jit = [0.03, -0.035, 0.02, -0.04, 0.0, 0.04, -0.02, 0.035];
    let mut tab = Vec::new();
    for b in 0..8 {
        for k in 0..4 {
            let q = b as f64 + k as f64 * 0.25 + jit[(b * 4 + k) % 8];
            tab.push(tri_note(q.max(0.0), 0.2, (k * 2) as i32));
        }
    }
    // Et des croches simples, et des noires pointees.
    tab.push(tri_note(8.0, 0.5, 0));
    tab.push(tri_note(8.52, 0.5, 2));
    tab.push(tri_note(9.0, 1.5, 0));
    tab.push(tri_note(10.5, 0.5, 2));
    doc::sort(&mut tab);
    let e = engrave(&tab, 0.0, (4, 4), 4);
    assert!(e.triplets.is_empty(), "{:?}", e.triplets);
    assert_eq!(e.grid, 4);
}

/// Une note sur une corde (0 = Mi grave) de l'accord standard.
fn on_string(q: f64, len: f64, string: usize, fret: i32) -> TNote {
    let open = Tuning::standard().strings[string];
    TNote {
        qn0: q,
        qn1: q + len,
        pitch: open + fret,
        string,
        fret,
        vel: 90,
        ..Default::default()
    }
}

#[test]
fn une_basse_tenue_sous_la_melodie_fait_une_deuxieme_voix() {
    let mut tab = vec![
        // La basse : deux blanches (Mi, La), a vide.
        on_string(0.0, 2.0, 0, 0),
        on_string(2.0, 2.0, 1, 0),
    ];
    // La melodie : des croches seules au-dessus.
    for k in 0..8 {
        tab.push(on_string(k as f64 * 0.5, 0.5, 3 + (k % 2), 2));
    }
    // Mesure 2 : des accords plaques qui resonnent, une seule voix.
    for b in 0..4 {
        let q = 4.0 + b as f64;
        tab.push(on_string(q, 2.0, 0, 0));
        tab.push(on_string(q, 2.0, 2, 2));
        tab.push(on_string(q, 2.0, 3, 2));
    }
    doc::sort(&mut tab);
    let e = engrave(&tab, 0.0, (4, 4), 4);
    assert_eq!(e.two_voices.iter().copied().collect::<Vec<_>>(), vec![0]);
    let sum = |m: i64, v: u8| -> i64 {
        e.signs
            .iter()
            .filter(|s| s.measure == m && s.voice == v)
            .map(|s| s.dur)
            .sum()
    };
    assert_eq!(sum(0, 1), e.mlen);
    assert_eq!(sum(0, 2), e.mlen);
    assert_eq!(sum(1, 2), 0, "la mesure 2 n'a qu'une voix");
    assert_eq!(sum(1, 1), e.mlen);
    // La basse garde ses deux blanches.
    let v2: Vec<(&str, usize)> = e
        .signs
        .iter()
        .filter(|s| s.voice == 2)
        .map(|s| (s.ty, e.events[s.ev].notes.len()))
        .collect();
    assert_eq!(v2, vec![("half", 1), ("half", 1)]);
    // La melodie : huit croches.
    let v1: Vec<&str> = e
        .signs
        .iter()
        .filter(|s| s.measure == 0 && s.voice == 1)
        .map(|s| s.ty)
        .collect();
    assert_eq!(v1, vec!["eighth"; 8]);
    // Le MusicXML : les voix 2 et 6, trois retours au debut de la mesure 1.
    let spec = ScoreSpec {
        title: "t".into(),
        origin_qn: 0.0,
        sig: (4, 4),
        bpm: 100.0,
        grid: 4,
        tuning: Tuning::standard(),
    };
    let (x, _) = crate::score::write_musicxml(&tab, &spec, 0);
    assert!(x.contains("<voice>2</voice>") && x.contains("<voice>6</voice>"));
    let m1 = &x[x.find("<measure number=\"1\"").unwrap()..x.find("<measure number=\"2\"").unwrap()];
    assert_eq!(m1.matches("<backup>").count(), 3);
    let m2 = &x[x.find("<measure number=\"2\"").unwrap()..];
    let m2 = m2.split("<measure number=\"3\"").next().unwrap();
    assert_eq!(m2.matches("<backup>").count(), 1);
}

#[test]
fn les_techniques_de_jeu_se_gardent_et_s_ecrivent() {
    let mut tab = vec![
        on_string(0.0, 0.5, 0, 3),
        on_string(0.5, 0.5, 0, 3),
        on_string(1.0, 0.5, 0, 5),
        on_string(2.0, 1.0, 2, 2),
        on_string(3.0, 1.0, 3, 12),
    ];
    for n in &mut tab[..3] {
        n.marks = TNote::PALM;
    }
    tab[2].marks |= TNote::DEAD;
    tab[3].marks = TNote::VIBRATO | TNote::RING;
    tab[4].marks = TNote::HARMONIC;
    // Le pense-bete du MIDI et le fichier de la tab les gardent.
    let side = Sidecar::of(&tab, &Default::default());
    let back = Sidecar::from_text(&side.to_text());
    assert_eq!(back.marks, side.marks);
    assert_eq!(back.marks.len(), 5);
    let txt = doc::tab_to_text(&tab, &Default::default(), "", 0.0);
    let (re, _, _) = doc::tab_from_text(&txt, 0.0).unwrap();
    assert_eq!(
        re.iter().map(|n| n.marks).collect::<Vec<_>>(),
        tab.iter().map(|n| n.marks).collect::<Vec<_>>()
    );
    // Le MusicXML : un seul « P.M. » pour le passage, la tete x, le
    // losange et l'harmonique, l'onde du vibrato.
    let spec = ScoreSpec {
        title: "t".into(),
        origin_qn: 0.0,
        sig: (4, 4),
        bpm: 100.0,
        grid: 4,
        tuning: Tuning::standard(),
    };
    let (x, _) = crate::score::write_musicxml(&tab, &spec, 0);
    assert_eq!(x.matches("<words>P.M.</words>").count(), 1);
    assert_eq!(x.matches("<words>let ring</words>").count(), 1);
    assert_eq!(x.matches("<notehead>x</notehead>").count(), 2);
    assert_eq!(x.matches("<notehead>diamond</notehead>").count(), 2);
    assert_eq!(x.matches("<harmonic><natural/></harmonic>").count(), 1);
    assert_eq!(x.matches("<wavy-line type=\"start\"").count(), 1);
}
