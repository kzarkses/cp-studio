//! L'hote audio sans REAPER : les ports sont tires par leur table virtuelle,
//! comme REAPER le fait, et l'horloge avance comme le crochet la fait avancer.

use super::*;
use crate::decode::tests::Fake;

/// Un bloc comme REAPER le joue : on tire le port, puis le passage « post ».
fn block(h: &mut AudioHost, port: usize, frames: usize) -> Vec<f64> {
    let t = h.core().clock_now() as f64 / h.core().srate();
    let mut out = vec![0.0; frames * 2];
    assert_eq!(h.pull(port, t, 2, &mut out), frames);
    h.core().tick(frames as i32);
    out
}

#[test]
fn a_vitesse_un_la_sortie_est_la_matiere_au_bit_pres() {
    let mut h = AudioHost::new(None, 48000.0);
    let frames = 5000;
    let s: Vec<f32> = (0..frames * 2)
        .map(|i| ((i * 7919) % 2000) as f32 / 1000.0 - 1.0)
        .collect();
    let c = h.load_samples(s.clone(), 2, 48000.0).unwrap();
    // Un bloc de silence d'abord : le lancement tombe au debut du suivant.
    block(&mut h, 0, 64);
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    let mut got = Vec::new();
    for _ in 0..100 {
        got.extend(block(&mut h, 0, 64));
    }
    // Aucun fondu impose sur un depart sur voix neuve : la matiere passe telle
    // quelle (f32 -> f64 est exact).
    for i in 0..frames * 2 {
        assert_eq!(got[i], s[i] as f64, "echantillon {i}");
    }
    assert!(got[frames * 2..].iter().all(|&x| x == 0.0));
    h.poll();
    assert!(h.voice(v).is_none(), "la voix finie est rendue");
    let st = h.stats();
    assert_eq!(
        (st.over_pulls, st.under_pulls, st.rewinds, st.skips),
        (0, 0, 0, 0)
    );
}

#[test]
fn une_section_joue_exactement_sa_matiere() {
    let mut h = AudioHost::new(None, 48000.0);
    let s: Vec<f32> = (0..4000).map(|i| (i as f32 + 1.0) / 4000.0).collect();
    let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
    block(&mut h, 0, 32);
    h.play(0, c, 1000, 1100, false, 1.0).unwrap();
    let mut got = Vec::new();
    for _ in 0..8 {
        got.extend(block(&mut h, 0, 32));
    }
    let heard: Vec<f64> = got.chunks(2).map(|f| f[0]).filter(|&x| x != 0.0).collect();
    assert_eq!(heard.len(), 100);
    for (k, &x) in heard.iter().enumerate() {
        assert_eq!(x, s[1000 + k] as f64);
    }
}

#[test]
fn le_decodage_passe_par_le_fil_et_entre_au_vivier() {
    let mut h = AudioHost::new(None, 48000.0);
    let t = h.load_reader(Box::new(Fake {
        frames: 48000 * 2 + 5,
        nch: 2,
        srate: 48000.0,
    }));
    let t0 = std::time::Instant::now();
    let c = loop {
        h.poll();
        match h.ticket(t) {
            Load::Pending => {}
            Load::Ready(c) => break c,
            Load::Failed(e) => panic!("{e}"),
        }
        assert!(t0.elapsed().as_secs() < 10);
        std::thread::yield_now();
    };
    assert_eq!(h.clip_info(c), Some((48000 * 2 + 5, 2, 48000.0)));
    let (mut lo, mut hi) = (vec![0.0; 10], vec![0.0; 10]);
    assert_eq!(h.peaks(c, Some(0), 0.0, 1000.0, &mut lo, &mut hi), 10);
    assert!(hi[9] > lo[0]);
    // Un ticket fini est oublie apres lecture.
    assert!(matches!(h.ticket(t), Load::Failed(_)));
}

#[test]
fn un_handle_perime_ne_joue_jamais_un_autre_son() {
    let mut h = AudioHost::new(None, 48000.0);
    let a = h.load_samples(vec![0.5; 100], 1, 48000.0).unwrap();
    h.release(a);
    h.poll();
    let b = h.load_samples(vec![0.25; 100], 1, 48000.0).unwrap();
    assert_eq!(a.id(), b.id(), "l'emplacement est reutilise");
    assert!(h.play(0, a, 0, 0, false, 1.0).is_none());
    assert!(h.clip_info(a).is_none());
    assert!(h.play(0, b, 0, 0, false, 1.0).is_some());
}

#[test]
fn la_boucle_de_test_tourne_sans_rupture() {
    let mut h = AudioHost::new(None, 48000.0);
    let (s, frames) = testloop::synth(48000.0);
    let c = h.load_samples(s.clone(), 2, 48000.0).unwrap();
    block(&mut h, 0, 128);
    h.play(0, c, 0, 0, true, 1.0).unwrap();
    // Deux tours et demi, par blocs de tailles variees.
    let mut got = Vec::new();
    let sizes = [128usize, 64, 441, 32, 1000, 7];
    let mut k = 0;
    while got.len() < frames * 2 * 5 / 2 {
        got.extend(block(&mut h, 0, sizes[k % sizes.len()]));
        k += 1;
    }
    for i in 0..frames * 2 * 5 / 2 {
        assert_eq!(got[i], s[i % (frames * 2)] as f64, "echantillon {i}");
    }
}

#[test]
fn un_long_fichier_s_ecoute_avant_d_etre_decode() {
    let mut h = AudioHost::new(None, 48000.0);
    let frames = 48000 * 90; // une minute et demie
    let t = h.load_reader(Box::new(Fake {
        frames,
        nch: 2,
        srate: 48000.0,
    }));
    let t0 = std::time::Instant::now();
    let c = loop {
        h.poll();
        match h.ticket(t) {
            Load::Pending => {}
            Load::Ready(c) => break c,
            Load::Failed(e) => panic!("{e}"),
        }
        assert!(t0.elapsed().as_secs() < 10);
        std::thread::yield_now();
    };
    // La longueur annoncee est la vraie des le debut.
    assert_eq!(h.clip_info(c).map(|i| i.0), Some(frames as i64));
    block(&mut h, 0, 64);
    h.play(0, c, 0, 0, false, 1.0).unwrap();
    let mut got = Vec::new();
    for _ in 0..40 {
        got.extend(block(&mut h, 0, 512));
    }
    // Ce qu'on entend est le fichier, echantillon pour echantillon.
    for (i, x) in got.iter().enumerate() {
        let (f, ch) = (i / 2, i % 2);
        let want = ((f * 4 + ch) as f64 / 1e7) as f32;
        assert_eq!(*x, want as f64, "echantillon {i}");
    }
    // La fin du decodage pose les pics exacts.
    while !h.clip_complete(c) || h.stats().decoding > 0 {
        h.poll();
        std::thread::yield_now();
        assert!(t0.elapsed().as_secs() < 30);
    }
    let (mut lo, mut hi) = (vec![0.0; 4], vec![0.0; 4]);
    assert_eq!(
        h.peaks(c, Some(0), 0.0, frames as f64 / 4.0, &mut lo, &mut hi),
        4
    );
    assert!(hi[3] > hi[0]);
}

#[test]
fn une_section_de_n_echantillons_rend_n_echantillons_de_1_a_4096() {
    // A3 : la matiere de la section, exactement, sans fondu impose, quelle que
    // soit sa taille, a tout endroit d'un bloc.
    let s: Vec<f32> = (0..20000)
        .map(|i| ((i % 251) as f32 + 1.0) / 256.0)
        .collect();
    let mut h = AudioHost::new(None, 48000.0);
    let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
    let mut sizes: Vec<usize> = (1..=64).collect();
    sizes.extend([
        100, 127, 128, 129, 255, 256, 257, 1000, 2047, 2048, 4095, 4096,
    ]);
    for (k, &n) in sizes.iter().enumerate() {
        let start = 1000 + k * 37;
        // Un bloc de taille variable avant : le depart tombe n'importe ou.
        block(&mut h, 0, 1 + (k * 13) % 97);
        let v = h
            .play(0, c, start as i64, (start + n) as i64, false, 1.0)
            .unwrap();
        let mut heard = Vec::new();
        while heard.len() < n + 200 {
            heard.extend(block(&mut h, 0, 64).chunks(2).map(|f| f[0]));
        }
        let got: Vec<f64> = heard.into_iter().filter(|&x| x != 0.0).collect();
        assert_eq!(got.len(), n, "section de {n}");
        for (i, &x) in got.iter().enumerate() {
            assert_eq!(x, s[start + i] as f64, "section de {n}, echantillon {i}");
        }
        h.poll();
        assert!(
            h.voice(v).is_none(),
            "la voix d'une section finie est rendue"
        );
    }
}

// ---------------------------------------------------------------------------
// La vitesse et la lecture a l'envers (A4)
// ---------------------------------------------------------------------------

/// Joue `c` (mono) de bout en bout a `rate`, avec l'interpolation `interp`
/// (0 Hermite, 1 sinc) ; rend le canal gauche.
fn render_at(c_samples: &[f32], rate: f64, interp: f64, frames_out: usize) -> Vec<f64> {
    let mut h = AudioHost::new(None, 48000.0);
    let c = h.load_samples(c_samples.to_vec(), 1, 48000.0).unwrap();
    block(&mut h, 0, 64);
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    assert!(h.set(v, Param::Rate, rate));
    assert!(h.set(v, Param::Interp, interp));
    let mut got = Vec::new();
    while got.len() < frames_out {
        got.extend(block(&mut h, 0, 256).chunks(2).map(|f| f[0]));
    }
    got.truncate(frames_out);
    got
}

fn rms(v: &[f64]) -> f64 {
    (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

fn sine(f: f64, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (std::f64::consts::TAU * f * i as f64 / 48000.0).sin() as f32)
        .collect()
}

/// Plus vite que 1, ce qui passe au-dessus du nouveau Nyquist doit
/// disparaitre : un La a 18 kHz joue a vitesse 2 (36 kHz) ne laisse rien
/// avec le sinc ; Hermite le replie a 12 kHz.
#[test]
fn le_sinc_ne_replie_pas() {
    let s = sine(18000.0, 48000);
    let skip = 2000;
    let n = 16000;
    let hq = render_at(&s, 2.0, 1.0, skip + n);
    let lo = render_at(&s, 2.0, 0.0, skip + n);
    let (a, b) = (rms(&hq[skip..]), rms(&lo[skip..]));
    let db = |x: f64| 20.0 * (x / std::f64::consts::FRAC_1_SQRT_2).log10();
    eprintln!(
        "repli a vitesse 2 : sinc {:.1} dB, Hermite {:.1} dB",
        db(a),
        db(b)
    );
    assert!(db(a) < -70.0, "sinc : {:.1} dB", db(a));
    assert!(db(b) > db(a) + 30.0);
}

/// En bande, le sinc rend la sinusoide attendue : un 1 kHz joue a 1,5
/// donne exactement un 1,5 kHz.
#[test]
fn le_sinc_est_juste_dans_la_bande() {
    let s = sine(1000.0, 48000);
    let skip = 1000;
    let n = 20000;
    let want: Vec<f64> = (0..skip + n)
        .map(|i| (std::f64::consts::TAU * 1000.0 * (i as f64 * 1.5) / 48000.0).sin())
        .collect();
    let err = |got: &[f64]| {
        let e: Vec<f64> = got[skip..]
            .iter()
            .zip(&want[skip..])
            .map(|(a, b)| a - b)
            .collect();
        20.0 * (rms(&e) / std::f64::consts::FRAC_1_SQRT_2).log10()
    };
    let hq = err(&render_at(&s, 1.5, 1.0, skip + n));
    let lo = err(&render_at(&s, 1.5, 0.0, skip + n));
    // Un 12 kHz aussi (pres du haut de la bande).
    let s2 = sine(12000.0, 48000);
    let want2: Vec<f64> = (0..skip + n)
        .map(|i| (std::f64::consts::TAU * 12000.0 * (i as f64 * 0.75) / 48000.0).sin())
        .collect();
    let got2 = render_at(&s2, 0.75, 1.0, skip + n);
    let e2: Vec<f64> = got2[skip..]
        .iter()
        .zip(&want2[skip..])
        .map(|(a, b)| a - b)
        .collect();
    let hq2 = 20.0 * (rms(&e2) / std::f64::consts::FRAC_1_SQRT_2).log10();
    let lo2v = render_at(&s2, 0.75, 0.0, skip + n);
    let e2l: Vec<f64> = lo2v[skip..]
        .iter()
        .zip(&want2[skip..])
        .map(|(a, b)| a - b)
        .collect();
    let lo2 = 20.0 * (rms(&e2l) / std::f64::consts::FRAC_1_SQRT_2).log10();
    eprintln!(
        "erreur : 1 kHz x1,5 sinc {hq:.1} dB / Hermite {lo:.1} dB ; 12 kHz x0,75 sinc {hq2:.1} dB / Hermite {lo2:.1} dB"
    );
    assert!(hq < -90.0, "{hq}");
    assert!(hq2 < -70.0, "{hq2}");
    // A 1 kHz, Hermite est deja a la limite du f32 ; a 12 kHz, non.
    assert!(lo < -60.0 && hq2 < lo2 - 60.0);
}

/// A l'envers, une section de N echantillons rend ses N echantillons, du
/// dernier au premier, au bit pres ; en boucle, elle recommence par sa fin.
#[test]
fn a_l_envers_au_bit_pres() {
    let mut h = AudioHost::new(None, 48000.0);
    let s: Vec<f32> = (0..4000).map(|i| (i as f32 + 1.0) / 4000.0).collect();
    let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
    block(&mut h, 0, 32);
    let v = h.play(0, c, 1000, 1100, false, 1.0).unwrap();
    assert!(h.set(v, Param::Reverse, 1.0));
    let mut got = Vec::new();
    for _ in 0..8 {
        got.extend(block(&mut h, 0, 32));
    }
    let heard: Vec<f64> = got.chunks(2).map(|f| f[0]).filter(|&x| x != 0.0).collect();
    assert_eq!(heard.len(), 100);
    for (k, &x) in heard.iter().enumerate() {
        assert_eq!(x, s[1099 - k] as f64, "{k}");
    }
    // En boucle : 1099..1000, puis de nouveau 1099...
    let v = h.play(0, c, 1000, 1100, true, 1.0).unwrap();
    assert!(h.set(v, Param::Reverse, 1.0));
    let mut got = Vec::new();
    for _ in 0..10 {
        got.extend(block(&mut h, 0, 32));
    }
    let heard: Vec<f64> = got.chunks(2).map(|f| f[0]).filter(|&x| x != 0.0).collect();
    assert!(heard.len() >= 250);
    for (k, &x) in heard.iter().take(250).enumerate() {
        assert_eq!(x, s[1099 - (k % 100)] as f64, "{k}");
    }
    h.stop(v, 0.0);
    // Une section d'UN echantillon, a l'envers : un echantillon.
    block(&mut h, 0, 32);
    let v = h.play(0, c, 2000, 2001, false, 1.0).unwrap();
    assert!(h.set(v, Param::Reverse, 1.0));
    let mut got = Vec::new();
    for _ in 0..3 {
        got.extend(block(&mut h, 0, 32));
    }
    let heard: Vec<f64> = got.chunks(2).map(|f| f[0]).filter(|&x| x != 0.0).collect();
    assert_eq!(heard, vec![s[2000] as f64]);
}

/// Dix minutes a une vitesse quelconque, en boucle : la tete est ou le
/// calcul exact la met (aucune derive mesurable).
#[test]
fn dix_minutes_sans_derive() {
    let mut h = AudioHost::new(None, 48000.0);
    let span = 48000i64;
    let s: Vec<f32> = (0..span).map(|i| (i % 97) as f32 / 97.0).collect();
    let c = h.load_samples(s, 1, 48000.0).unwrap();
    block(&mut h, 0, 64);
    let v = h.play(0, c, 0, 0, true, 1.0).unwrap();
    let rate = 1.37;
    assert!(h.set(v, Param::Rate, rate));
    let blocks = 10 * 60 * 48000 / 4096;
    for _ in 0..blocks {
        block(&mut h, 0, 4096);
    }
    h.poll();
    let (pos, _) = h.voice(v).unwrap();
    let frames = (blocks * 4096) as f64;
    let want = (rate * frames).rem_euclid(span as f64);
    let err = (pos - want).abs().min(span as f64 - (pos - want).abs());
    eprintln!("10 min a 1,37 : tete {pos:.6}, attendu {want:.6}, ecart {err:.2e} echantillon");
    assert!(err < 1e-3, "{err}");
}

/// Le cout du sinc : une voix stereo a vitesse 1,5, une seconde de son.
#[test]
fn le_sinc_tient_le_temps_reel() {
    let mut h = AudioHost::new(None, 48000.0);
    let s: Vec<f32> = (0..96000 * 2)
        .map(|i| ((i * 31) % 200) as f32 / 100.0 - 1.0)
        .collect();
    let c = h.load_samples(s, 2, 48000.0).unwrap();
    block(&mut h, 0, 64);
    let v = h.play(0, c, 0, 0, true, 1.0).unwrap();
    h.set(v, Param::Rate, 1.5);
    h.set(v, Param::Interp, 1.0);
    block(&mut h, 0, 512);
    let t0 = std::time::Instant::now();
    for _ in 0..(48000 / 512) {
        block(&mut h, 0, 512);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "sinc stereo x1,5 : {ms:.1} ms pour 1 s de son ({:.1} % d'un coeur)",
        ms / 10.0
    );
    // Sans optimisation de compilation, large : un dixieme du temps reel.
    assert!(ms < 100.0, "{ms} ms");
}
