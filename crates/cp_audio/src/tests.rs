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

/// Les niveaux d'un port pour les vumetres : la crete depuis la derniere
/// lecture, par canal, remise a zero en la lisant.
#[test]
fn les_niveaux_d_un_port() {
    let mut h = AudioHost::new(None, 48000.0);
    let s: Vec<f32> = (0..48000)
        .flat_map(|i| {
            let x = (std::f64::consts::TAU * 440.0 * i as f64 / 48000.0).sin() as f32;
            [x, x * 0.25]
        })
        .collect();
    let c = h.load_samples(s, 2, 48000.0).unwrap();
    block(&mut h, 0, 64);
    assert_eq!(h.port_levels(0), [0.0, 0.0]);
    h.play(0, c, 0, 0, true, 0.5).unwrap();
    for _ in 0..20 {
        block(&mut h, 0, 512);
    }
    let [l, r] = h.port_levels(0);
    assert!(
        (l - 0.5).abs() < 0.01 && (r - 0.125).abs() < 0.01,
        "{l} {r}"
    );
    // Lue : remise a zero.
    assert_eq!(h.port_levels(0), [0.0, 0.0]);
    // Un port jamais ouvert : rien.
    assert_eq!(h.port_levels(5), [0.0, 0.0]);
}

// ---------------------------------------------------------------------------
// La robustesse (A9)
// ---------------------------------------------------------------------------

/// Un generateur reproductible.
struct Lcg(u64);

impl Lcg {
    fn f(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn n(&mut self, k: usize) -> usize {
        (self.f() * k as f64) as usize % k.max(1)
    }
}

/// Une session de `hours` heures de son, tiree comme REAPER (des blocs de
/// 1024 sur quatre ports), avec des lancements, des arrets, des reglages et
/// des clips charges puis liberes au hasard. A la fin : aucune perte
/// comptee, aucune fuite (le vivier revient a vide), aucune derive (une voix
/// temoin en boucle a vitesse 1 est ou le compte exact la met), rien de non
/// fini dans la sortie. Rend le cout mesure par voix.
fn session(hours: f64, max_voices: usize) -> f64 {
    let mut h = AudioHost::new(None, 48000.0);
    // Comme dans REAPER : l'horloge est celle du crochet des le depart.
    h.core().use_external_clock();
    let mut r = Lcg(0x5EED);
    let mk = |r: &mut Lcg, n: usize, nch: usize| -> Vec<f32> {
        let f = 50.0 + r.f() * 2000.0;
        (0..n * nch)
            .map(|i| ((i / nch) as f64 * f * std::f64::consts::TAU / 48000.0).sin() as f32 * 0.3)
            .collect()
    };
    // La temoin : 12 345 frames en boucle a vitesse 1, sur le port 0.
    let span = 12_345i64;
    let wit_clip = h
        .load_samples(mk(&mut r, span as usize, 1), 1, 48000.0)
        .unwrap();
    block(&mut h, 0, 1024);
    let witness = h.play(0, wit_clip, 0, 0, true, 0.0).unwrap();
    let start_block = h.core().clock_now();
    let mut clips: Vec<ClipH> = Vec::new();
    let mut voices: Vec<VoiceH> = Vec::new();
    let blocks = (hours * 3600.0 * 48000.0 / 1024.0) as usize;
    let t0 = std::time::Instant::now();
    let mut voice_blocks = 0u64;
    let mut out = vec![0.0f64; 1024 * 2];
    for b in 0..blocks {
        // Environ deux evenements par seconde de son.
        if b % 23 == 0 {
            match r.n(8) {
                0 | 1 if clips.len() < 24 => {
                    let n = 2000 + r.n(200_000);
                    let nch = 1 + r.n(2);
                    if let Ok(c) =
                        h.load_samples(mk(&mut r, n, nch), nch, [44100.0, 48000.0, 96000.0][r.n(3)])
                    {
                        clips.push(c);
                    }
                }
                2 if !clips.is_empty() => {
                    let c = clips.swap_remove(r.n(clips.len()));
                    h.release(c);
                }
                3..=5 if !clips.is_empty() && voices.len() < max_voices => {
                    let c = clips[r.n(clips.len())];
                    let port = 1 + r.n(3);
                    if let Some(v) = h.play(port, c, 0, 0, r.f() < 0.5, 0.2) {
                        h.set(v, Param::Rate, 0.5 + r.f() * 1.5);
                        h.set(v, Param::Interp, (r.f() < 0.5) as i32 as f64);
                        h.set(v, Param::Reverse, (r.f() < 0.2) as i32 as f64);
                        h.set(v, Param::FadeIn, r.f() * 0.01);
                        voices.push(v);
                    }
                }
                6 if !voices.is_empty() => {
                    let v = voices.swap_remove(r.n(voices.len()));
                    h.stop(v, r.f() * 0.05);
                }
                _ => {
                    if let Some(&v) = voices.get(r.n(voices.len().max(1))) {
                        h.set(v, Param::Pan, r.f() * 2.0 - 1.0);
                        h.set(v, Param::Gain, r.f() * 0.3);
                    }
                }
            }
        }
        let t = h.core().clock_now() as f64 / 48000.0;
        for port in 0..4 {
            out.iter_mut().for_each(|x| *x = 0.0);
            assert_eq!(h.pull(port, t, 2, &mut out), 1024);

            if b % 997 == 0 {
                assert!(out.iter().all(|x| x.is_finite()), "bloc {b} port {port}");
            }
        }
        h.core().tick(1024);
        voice_blocks += h.core().active_voices() as u64;
        if b % 50 == 0 {
            h.poll();
            voices.retain(|&v| h.voice(v).is_some());
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    // La temoin : la ou le compte exact la met.
    h.poll();
    let (pos, _) = h.voice(witness).unwrap();
    let played = h.core().clock_now() - start_block;
    let want = played.rem_euclid(span) as f64;
    assert!(
        (pos - want).abs() < 1e-6 || (pos - want).abs() > span as f64 - 1.0,
        "derive : {pos} au lieu de {want}"
    );
    // Aucune perte comptee.
    let st = h.stats();
    assert_eq!(
        (
            st.over_pulls,
            st.under_pulls,
            st.rewinds,
            st.skips,
            st.dropped_commands,
            st.busy_renders
        ),
        (0, 0, 0, 0, 0, 0),
        "{st:?}"
    );
    // Tout arrete, tout libere : le vivier est vide.
    h.stop(witness, 0.0);
    for v in voices.drain(..) {
        h.stop(v, 0.0);
    }
    for c in clips.drain(..) {
        h.release(c);
    }
    h.release(wit_clip);
    for _ in 0..8 {
        let t = h.core().clock_now() as f64 / 48000.0;
        for port in 0..4 {
            h.pull(port, t, 2, &mut out);
        }
        h.core().tick(1024);
        h.poll();
    }
    let st = h.stats();
    assert_eq!(
        (st.clips, st.bytes, st.active_voices),
        (0, 0, 0),
        "fuite : {st:?}"
    );
    // Le cout : secondes de calcul par seconde de son et par voix.
    let audio_s = blocks as f64 * 1024.0 / 48000.0;
    let mean_voices = voice_blocks as f64 / blocks.max(1) as f64;
    let per_voice = secs / audio_s / mean_voices.max(1.0) * 100.0;
    eprintln!(
        "session de {hours} h : {secs:.1} s de calcul, {mean_voices:.1} voix en moyenne, {per_voice:.3} % d'un coeur par voix"
    );
    per_voice
}

/// Une minute de son tres chargee (jusqu'a 40 voix) : dans la suite des
/// tests.
#[test]
fn une_session_chargee() {
    session(1.0 / 60.0, 40);
}

/// Huit heures (l'exigence A9) : `cargo test -p cp_audio --release --
/// --ignored huit_heures`.
#[test]
#[ignore]
fn huit_heures_de_session() {
    session(8.0, 6);
}

// ---------------------------------------------------------------------------
// L'etirement en temps reel (A5)
// ---------------------------------------------------------------------------

/// Une voix au tempo 0,5 (l'imitation d'etireur : lecture lineaire, un
/// amorcage de 1000 frames) : la sortie dure le double, suit la matiere au
/// bon rythme des le premier echantillon, et la voix finit quand l'etireur
/// s'est vide ; l'etireur revient au vivier.
#[test]
fn le_tempo_en_temps_reel() {
    let mut h = AudioHost::new(None, 48000.0);
    h.set_shift_factory(Box::new(|| Box::new(cp_engine::shift::LinearRt::new(1000))));
    let n = 4800usize;
    let s: Vec<f32> = (0..n).map(|i| (i as f32 + 1.0) / n as f32).collect();
    let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
    block(&mut h, 0, 512);
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    assert!(h.set(v, Param::Tempo, 0.5));
    assert_eq!(h.shifters_in_use(), (1, 1));
    let mut got = Vec::new();
    for _ in 0..40 {
        got.extend(block(&mut h, 0, 512).chunks(2).map(|f| f[0]));
    }
    let heard: Vec<f64> = got.iter().copied().skip_while(|&x| x == 0.0).collect();
    let len = heard.iter().rposition(|&x| x != 0.0).unwrap() + 1;
    // Le double, a l'amorcage et a l'interpolation pres.
    assert!((len as i64 - 2 * n as i64).abs() <= 4, "{len}");
    // Le rythme : la sortie k vaut la matiere k / 2.
    for k in [0usize, 1, 2, 100, 2001, 9000] {
        let want = s[k / 2] as f64
            + if k % 2 == 1 {
                (s[(k / 2 + 1).min(n - 1)] - s[k / 2]) as f64 * 0.5
            } else {
                0.0
            };
        assert!(
            (heard[k] - want).abs() < 1e-4,
            "{k} : {} au lieu de {want}",
            heard[k]
        );
    }
    h.poll();
    assert!(h.voice(v).is_none(), "la voix a fini");
    assert_eq!(h.shifters_in_use(), (1, 0), "l'etireur est rendu");
    // Une boucle a 1,5 tourne sans fin, avec le meme etireur.
    let v = h.play(0, c, 0, 0, true, 1.0).unwrap();
    h.set(v, Param::Tempo, 1.5);
    for _ in 0..200 {
        let b = block(&mut h, 0, 512);
        assert!(b.iter().all(|x| x.is_finite()));
    }
    h.poll();
    assert!(h.voice(v).is_some());
    assert_eq!(h.shifters_in_use(), (1, 1));
    let st = h.stats();
    assert_eq!(
        (st.under_pulls, st.over_pulls, st.dropped_commands),
        (0, 0, 0)
    );
}

/// Une boucle dont la fin ne rejoint pas le debut (1000 frames d'un La :
/// 9,17 periodes) claque a chaque tour ; avec un fondu enchaine de 5 ms au
/// point de boucle, plus aucun saut. Sans fondu, le son reste celui de
/// toujours.
#[test]
fn le_fondu_au_point_de_boucle() {
    let s = sine(440.0, 48000);
    let run = |xf: f64, interp: f64| -> f64 {
        let mut h = AudioHost::new(None, 48000.0);
        let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
        block(&mut h, 0, 64);
        let v = h.play(0, c, 4800, 5800, true, 1.0).unwrap();
        h.set(v, Param::LoopXfade, xf);
        h.set(v, Param::Interp, interp);
        let mut got = Vec::new();
        for _ in 0..12 {
            got.extend(block(&mut h, 0, 256).chunks(2).map(|f| f[0]));
        }
        // Le plus grand saut d'un echantillon au suivant, hors du depart.
        got.windows(2)
            .skip(100)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max)
    };
    let natural = 2.0 * std::f64::consts::PI * 440.0 / 48000.0;
    let (hard, soft) = (run(0.0, 0.0), run(0.005, 0.0));
    eprintln!(
        "saut au point de boucle : {hard:.3} sans fondu, {soft:.4} avec (une sinusoide : {natural:.4})"
    );
    assert!(hard > 0.3, "{hard}");
    assert!(soft < natural * 1.3, "{soft}");
    // A vitesse 1,3 en sinc, aussi.
    let mut h = AudioHost::new(None, 48000.0);
    let c = h.load_samples(s.clone(), 1, 48000.0).unwrap();
    block(&mut h, 0, 64);
    let v = h.play(0, c, 4800, 5800, true, 1.0).unwrap();
    h.set(v, Param::LoopXfade, 0.005);
    h.set(v, Param::Rate, 1.3);
    h.set(v, Param::Interp, 1.0);
    let mut got = Vec::new();
    for _ in 0..12 {
        got.extend(block(&mut h, 0, 256).chunks(2).map(|f| f[0]));
    }
    let worst = got
        .windows(2)
        .skip(100)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    eprintln!("a 1,3 en sinc : {worst:.4}");
    assert!(worst < natural * 1.3 * 1.3 + 0.01, "{worst}");
}

// ---------------------------------------------------------------------------
// L'enregistrement (A8)
// ---------------------------------------------------------------------------

/// Une prise de 4000 frames (latence 300) devient un clip de 4000 frames
/// exactement, a la matiere jouee a son debut ; une prise a des entrees
/// differentes pendant qu'une autre ecoute est refusee.
#[test]
fn une_prise_devient_un_clip() {
    let mut h = AudioHost::new(None, 48000.0);
    let id = h.rec_start_lat(0, 1, Some(1000), 300).unwrap();
    assert!(
        h.rec_start_lat(2, 3, None, 0).is_none(),
        "autres entrees refusees"
    );
    h.rec_stop(id, Some(5000));
    let ring = h.rec_ring().unwrap();
    let mut t = 0i64;
    while t < 7000 {
        let l: Vec<f64> = (0..512).map(|k| (t + k) as f64 / 10000.0).collect();
        ring.push_block(t, &l, None);
        t += 512;
        if matches!(h.rec_state(id), RecState::Recording(_) | RecState::Unknown) {
            h.poll();
        }
    }
    let RecState::Ready(c) = h.rec_state(id) else {
        panic!("{:?}", h.rec_state(id));
    };
    let (frames, nch, _) = h.clip_info(c).unwrap();
    assert_eq!((frames, nch), (4000, 2));
    let s = h.clip_samples(c).unwrap();
    assert!((s[0] - 0.13).abs() < 1e-6, "{}", s[0]);
    // Plus de prise : l'entree n'est plus copiee.
    assert_eq!(ring.inputs(), (-1, -1));
}

/// Les pages d'un clip en flux.
fn pages_of(h: &AudioHost, c: ClipH) -> std::sync::Arc<cp_sys::paged::Paged> {
    h.clip(c).unwrap().0.paged().unwrap().clone()
}

/// Ce que la tete lira au prochain bloc est-il la ? (En vrai, le flux a dix
/// secondes d'avance ; ici on tire les blocs bien plus vite que le temps.)
fn wait_page(h: &mut AudioHost, c: ClipH, frame: i64) {
    let p = pages_of(h, c);
    let t0 = std::time::Instant::now();
    while !p.has(p.page_of(frame)) {
        h.poll();
        assert!(
            t0.elapsed().as_secs() < 10,
            "page {} jamais venue",
            p.page_of(frame)
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn fake_at(f: usize, ch: usize) -> f64 {
    ((f * 4 + ch) as f64 / 1e7) as f32 as f64
}

#[test]
fn deux_heures_s_ecoutent_en_flux() {
    let mut h = AudioHost::new(None, 48000.0);
    let frames = 48000 * 7200;
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
    let ready_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(h.clip_streamed(c));
    assert_eq!(h.clip_info(c).map(|i| i.0), Some(frames as i64));
    assert_eq!(h.clip_decoded(c), Some(frames as i64));
    block(&mut h, 0, 64);

    // Du debut : tout de suite (le debut est toujours la), au bit pres.
    h.play(0, c, 0, 0, false, 1.0).unwrap();
    let mut got = Vec::new();
    for b in 0..400 {
        wait_page(&mut h, c, (b + 1) * 512);
        got.extend(block(&mut h, 0, 512));
    }
    for (i, x) in got.iter().enumerate() {
        assert_eq!(*x, fake_at(i / 2, i % 2), "echantillon {i}");
    }

    // Un saut a une heure : une voix neuve, son depart attend sa page.
    let v0 = h.voices[0];
    h.stop(v0, 0.0);
    let at = 48000 * 3600 + 12345;
    let v = h.play(0, c, at, 0, false, 1.0).unwrap();
    wait_page(&mut h, c, at);
    let mut heard = Vec::new();
    for _ in 0..200 {
        let out = block(&mut h, 0, 512);
        if let Some((pos, _)) = h.voice(v) {
            if pos > at as f64 {
                heard.extend(out);
            }
        }
        let head = h.voice(v).map_or(at, |(p, _)| p as i64);
        wait_page(&mut h, c, head + 512);
    }
    // Ce qu'on entend apres le depart est le fichier a partir d'une heure.
    let first = heard.iter().position(|&x| x != 0.0).expect("du son");
    assert_eq!(first % 2, 0);
    for (i, x) in heard[first..].iter().enumerate() {
        let (f, ch) = (at as usize + i / 2, i % 2);
        assert_eq!(*x, fake_at(f, ch), "apres le saut, echantillon {i}");
    }
    let p = pages_of(&h, c);
    assert_eq!(p.misses(), 0, "aucun bloc n'a lu une page absente");
    // Les pages loin de la tete sont parties ; le debut reste.
    for _ in 0..50 {
        h.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(p.has(0) && p.has(2), "le debut reste");
    // Autour de l ancienne tete (30 s), de la nouvelle, et le debut : pas plus.
    assert!(p.resident() <= 45, "{} pages en memoire", p.resident());
    assert!(!p.has(2000));
    let st = h.stats();
    assert_eq!(st.streams, 1);
    assert!(
        st.bytes < 64 << 20,
        "deux heures en {} Mo seulement",
        st.bytes >> 20
    );
    eprintln!(
        "flux : pret en {ready_ms:.1} ms, {} pages en memoire ({} Mo)",
        p.resident(),
        st.bytes >> 20
    );

    // Les pics arrivent pendant le passage, puis couvrent tout.
    let t1 = std::time::Instant::now();
    let (mut lo, mut hi) = (vec![0.0; 8], vec![0.0; 8]);
    loop {
        h.poll();
        let n = h.peaks(c, Some(0), 0.0, frames as f64 / 8.0, &mut lo, &mut hi);
        if n == 8 && hi[7] > 0.0 {
            break;
        }
        assert!(t1.elapsed().as_secs() < 120, "pics : {n} pixels");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    eprintln!("pics de deux heures en {:.1} s", t1.elapsed().as_secs_f64());
    for i in 1..8 {
        assert!(hi[i] > hi[i - 1], "la forme monte comme la matiere");
    }
    // Au plus pres, la matiere elle-meme (pages en memoire), ou le niveau
    // fin ailleurs : jamais rien de faux.
    let (mut lo, mut hi) = (vec![0.0; 4], vec![0.0; 4]);
    h.peaks(c, Some(0), at as f64, 0.5, &mut lo, &mut hi);
    assert!((hi[0] - fake_at(at as usize, 0) as f32).abs() < 1e-4);

    // Rendu : la voix s'arrete, le clip part, les pages aussi.
    h.stop(v, 0.0);
    h.release(c);
    let t2 = std::time::Instant::now();
    while h.stats().streams > 0 || h.stats().bytes > 0 {
        block(&mut h, 0, 512);
        h.poll();
        assert!(t2.elapsed().as_secs() < 10, "le clip ne part pas");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn un_fichier_court_reste_en_memoire_et_le_flux_se_regle() {
    // Sous le seuil, rien ne change ; un seuil bas met un petit fichier en
    // flux, et il joue pareil, a l'envers compris.
    let mut h = AudioHost::new(None, 48000.0);
    h.set_stream_above(1 << 20);
    let frames = 48000 * 20;
    let t = h.load_reader(Box::new(Fake {
        frames,
        nch: 2,
        srate: 48000.0,
    }));
    let c = loop {
        h.poll();
        match h.ticket(t) {
            Load::Pending => std::thread::yield_now(),
            Load::Ready(c) => break c,
            Load::Failed(e) => panic!("{e}"),
        }
    };
    assert!(h.clip_streamed(c));
    // Le cuire est refuse, en le disant.
    let tk = h.stretch(c, 0, 0, 0.8, 1.0);
    match h.ticket(tk) {
        Load::Failed(e) => assert!(e.contains("streamed"), "{e}"),
        _ => panic!("une cuisson d'un clip en flux"),
    }
    block(&mut h, 0, 64);
    let start = 48000 * 15;
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    for _ in 0..4 {
        block(&mut h, 0, 512);
    }
    // Une voix qui joue saute a 15 s et repart a l envers.
    h.set(v, Param::Pos, start as f64);
    h.set(v, Param::Reverse, 1.0);
    wait_page(&mut h, c, start);
    let mut got = Vec::new();
    let mut pos = start;
    for _ in 0..300 {
        wait_page(&mut h, c, (pos - 600).max(0));
        got.extend(block(&mut h, 0, 512));
        pos = h.voice(v).map_or(pos, |(p, _)| p as i64);
    }
    assert!(pos < start - 100_000, "la tete recule : {pos}");
    let p = pages_of(&h, c);
    assert_eq!(
        p.misses(),
        0,
        "a l'envers aussi, les pages sont venues a temps"
    );
}

#[test]
fn le_flux_tient_le_temps_reel() {
    // Des blocs tires au rythme de REAPER (512 frames toutes les 10,7 ms)
    // pendant trois secondes, avec un saut au hasard toutes les demi-secondes
    // (une voix neuve, l'ancienne s'eteint en 10 ms) : aucun bloc ne lit une
    // page absente.
    let mut h = AudioHost::new(None, 48000.0);
    let frames = 48000 * 7200;
    let t = h.load_reader(Box::new(Fake {
        frames,
        nch: 2,
        srate: 48000.0,
    }));
    let c = loop {
        h.poll();
        match h.ticket(t) {
            Load::Pending => std::thread::yield_now(),
            Load::Ready(c) => break c,
            Load::Failed(e) => panic!("{e}"),
        }
    };
    block(&mut h, 0, 64);
    let mut v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    let mut seed = 0x9e37_79b9_u64;
    let t0 = std::time::Instant::now();
    let period = std::time::Duration::from_secs_f64(512.0 / 48000.0);
    let mut next = t0;
    let mut jumps = 0;
    for b in 0..280 {
        if b % 47 == 46 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let at = ((seed >> 33) % (frames as u64 - 48000 * 20)) as i64;
            h.stop(v, 0.01);
            v = h.play(0, c, at, 0, false, 1.0).unwrap();
            jumps += 1;
        }
        block(&mut h, 0, 512);
        h.poll();
        next += period;
        if let Some(d) = next.checked_duration_since(std::time::Instant::now()) {
            std::thread::sleep(d);
        }
    }
    let p = pages_of(&h, c);
    assert!(jumps >= 5);
    assert_eq!(p.misses(), 0, "{jumps} sauts en temps reel");
}

#[test]
fn une_prise_midi_par_l_hote() {
    // Le crochet imite : il date les messages et publie sa progression.
    let mut h = AudioHost::new(None, 48000.0);
    block(&mut h, 0, 64);
    let t0 = h.core().clock_now();
    let id = h.midi_rec_start_lat(-1, Some(t0 + 1000), 171);
    h.midi_rec_stop(id, Some(t0 + 10_000));
    let ring = MIDI_RING.get().expect("anneau").clone();
    assert!(ring.inputs() > 0, "le crochet ecoute");
    ring.push(t0 + 1171 + 300, 0, &[0x90, 60, 100]);
    ring.push(t0 + 1171 + 4800, 0, &[0x80, 60, 0]);
    ring.advance(t0 + 5000);
    h.poll();
    assert!(h.midi_rec_take(id).is_none(), "pas finie");
    assert_eq!(h.midi_rec_count(id).0, Some(2));
    ring.advance(t0 + 10_200);
    h.poll();
    let ev = h.midi_rec_take(id).expect("finie");
    assert_eq!(
        ev.iter().map(|e| (e.frame, e.msg)).collect::<Vec<_>>(),
        vec![(300, [0x90, 60, 100]), (4800, [0x80, 60, 0])]
    );
    assert_eq!(ring.inputs(), 0, "plus de prise : le crochet n'ecoute plus");
    assert!(h.midi_rec_take(id).is_none(), "donnee une fois");
}

/// Une source qui note le fil ou elle est detruite.
struct Tracked {
    inner: Fake,
    dropped_on: std::sync::Arc<std::sync::Mutex<Option<std::thread::ThreadId>>>,
}

impl decode::Reader for Tracked {
    fn channels(&mut self) -> i32 {
        self.inner.channels()
    }
    fn sample_rate(&mut self) -> f64 {
        self.inner.sample_rate()
    }
    fn length(&mut self) -> f64 {
        self.inner.length()
    }
    fn read(&mut self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize {
        self.inner.read(time_s, srate, nch, out)
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Ok(mut d) = self.dropped_on.lock() {
            *d = Some(std::thread::current().id());
        }
    }
}

#[test]
fn les_sources_sont_detruites_sur_le_fil_principal_a_la_fermeture() {
    // Un flux en cours et un decodage en cours quand l'hote est lache (REAPER
    // se ferme) : les deux sources reviennent et sont detruites ici, jamais
    // sur les fils de travail.
    for streamed in [true, false] {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        {
            let mut h = AudioHost::new(None, 48000.0);
            if streamed {
                h.set_stream_above(1 << 20);
            }
            let t = h.load_reader(Box::new(Tracked {
                inner: Fake {
                    frames: 48000 * 600,
                    nch: 2,
                    srate: 48000.0,
                },
                dropped_on: seen.clone(),
            }));
            if streamed {
                let t0 = std::time::Instant::now();
                while matches!(h.ticket(t), Load::Pending) {
                    h.poll();
                    assert!(t0.elapsed().as_secs() < 10);
                    std::thread::yield_now();
                }
            }
        }
        assert_eq!(
            *seen.lock().unwrap(),
            Some(std::thread::current().id()),
            "en flux : {streamed}"
        );
    }
}

/// Un hote dont le clip de `secs` secondes est lu en flux (un seuil bas), et
/// le clip, pret.
fn streamed_host(secs: usize, real_frames: Option<usize>) -> (AudioHost, ClipH) {
    struct Short(Fake, usize);
    impl decode::Reader for Short {
        fn channels(&mut self) -> i32 {
            self.0.channels()
        }
        fn sample_rate(&mut self) -> f64 {
            self.0.sample_rate()
        }
        fn length(&mut self) -> f64 {
            self.0.length()
        }
        fn read(&mut self, t: f64, sr: f64, nch: usize, out: &mut [f64]) -> usize {
            // Une source plus courte qu'annoncee : rien au-dela de `self.1`.
            let start = (t * 48000.0).floor() as usize;
            if start >= self.1 {
                return 0;
            }
            let n = (out.len() / nch).min(self.1 - start);
            self.0.read(t, sr, nch, &mut out[..n * nch])
        }
    }
    let mut h = AudioHost::new(None, 48000.0);
    h.set_stream_above(1 << 20);
    let frames = 48000 * secs;
    let f = Fake {
        frames,
        nch: 2,
        srate: 48000.0,
    };
    let t = h.load_reader(Box::new(Short(f, real_frames.unwrap_or(frames))));
    let c = loop {
        h.poll();
        match h.ticket(t) {
            Load::Pending => std::thread::yield_now(),
            Load::Ready(c) => break c,
            Load::Failed(e) => panic!("{e}"),
        }
    };
    block(&mut h, 0, 64);
    (h, c)
}

/// Tire `blocks` blocs de 512 au rythme de REAPER.
fn realtime(h: &mut AudioHost, blocks: usize) {
    let period = std::time::Duration::from_secs_f64(512.0 / 48000.0);
    let mut next = std::time::Instant::now();
    for _ in 0..blocks {
        block(h, 0, 512);
        h.poll();
        next += period;
        if let Some(d) = next.checked_duration_since(std::time::Instant::now()) {
            std::thread::sleep(d);
        }
    }
}

#[test]
fn a_l_envers_le_flux_ne_tourne_pas_a_vide() {
    // Une voix a l'envers depuis 300 s, a la vitesse 6 (elle quitte vite la
    // place demandee au depart) : le flux pose les pages qu'il faut (six par
    // seconde, plus son avance), pas des centaines (il chargeait et lachait
    // la meme page sans fin).
    let (mut h, c) = streamed_host(600, None);
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    realtime(&mut h, 4);
    h.set(v, Param::Pos, (48000 * 300) as f64);
    h.set(v, Param::Reverse, 1.0);
    h.set(v, Param::Rate, 6.0);
    let p = pages_of(&h, c);
    let t0 = std::time::Instant::now();
    while !p.has(p.page_of(48000 * 300)) {
        h.poll();
        assert!(t0.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    realtime(&mut h, 280);
    let f0 = p.fills();
    realtime(&mut h, 190);
    let fills = p.fills() - f0;
    eprintln!(
        "a l'envers x6 : {fills} pages posees en 2 s, {} manquees",
        p.misses()
    );
    assert!(fills <= 24, "{fills} pages posees en 2 s");
    assert_eq!(p.misses(), 0);
    let pos = h.voice(v).map_or(0.0, |x| x.0);
    assert!(pos < (48000 * 280) as f64, "la tete recule : {pos}");
}

#[test]
fn deux_voix_loin_l_une_de_l_autre() {
    // Deux voix du meme clip a 60 s d'ecart : chacune garde ses pages.
    let (mut h, c) = streamed_host(600, None);
    let p = pages_of(&h, c);
    let a = 48000 * 100;
    let b = 48000 * 160;
    let _v1 = h.play(0, c, a, 0, false, 0.5).unwrap();
    let _v2 = h.play(0, c, b, 0, false, 0.5).unwrap();
    let t0 = std::time::Instant::now();
    while !(p.has(p.page_of(a)) && p.has(p.page_of(b))) {
        h.poll();
        assert!(t0.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    realtime(&mut h, 280);
    assert_eq!(p.misses(), 0, "aucune page manquee pour l'une ou l'autre");
}

#[test]
fn une_boucle_au_milieu_du_fichier() {
    // Une boucle de 12 s a partir de 100 s, jouee a la vitesse 4 : plusieurs
    // retours en 4 s, le debut de la boucle toujours pret.
    let (mut h, c) = streamed_host(600, None);
    let p = pages_of(&h, c);
    let (a, z) = (48000 * 100, 48000 * 112);
    let v = h.play(0, c, a, z, true, 1.0).unwrap();
    h.set(v, Param::Rate, 4.0);
    let t0 = std::time::Instant::now();
    while !p.has(p.page_of(a)) {
        h.poll();
        assert!(t0.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    realtime(&mut h, 375);
    let pos = h.voice(v).map_or(0.0, |x| x.0);
    assert!(pos >= a as f64 && pos < z as f64, "dans la boucle : {pos}");
    assert_eq!(p.misses(), 0, "les retours trouvent leur page");
}

#[test]
fn une_source_plus_courte_qu_annoncee() {
    // Annoncee 60 s, elle s'arrete a 58,5 s : la fin est du silence, et le
    // flux continue ailleurs (il s'arretait pour de bon).
    let (mut h, c) = streamed_host(60, Some(48000 * 58 + 24000));
    let p = pages_of(&h, c);
    let v = h.play(0, c, 48000 * 57, 0, false, 1.0).unwrap();
    realtime(&mut h, 300);
    h.stop(v, 0.0);
    let v2 = h.play(0, c, 48000 * 20, 0, false, 1.0).unwrap();
    let t0 = std::time::Instant::now();
    while !p.has(p.page_of(48000 * 21)) {
        h.poll();
        assert!(t0.elapsed().as_secs() < 5, "le flux s'est arrete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let got = block(&mut h, 0, 512);
    assert!(h.voice(v2).is_some());
    assert!(got.iter().any(|&x| x != 0.0) || h.voice(v2).is_some_and(|x| x.1 != 2));
}

#[test]
fn un_etireur_revient_toujours_au_vivier() {
    // Vingt lancements dates, etires, annules avant leur depart : chacun
    // rendait son etireur seulement en sonnant jusqu'au bout. Au-dela de
    // 16, le tempo d'une voix neuve etait ignore sans un mot.
    let mut h = AudioHost::new(None, 48000.0);
    h.set_shift_factory(Box::new(|| Box::new(cp_engine::shift::LinearRt::new(1000))));
    let c = h.load_samples(vec![0.5; 48000], 1, 48000.0).unwrap();
    block(&mut h, 0, 512);
    for _ in 0..20 {
        let at = h.core().clock_now() + 48000;
        let v = h.play_at(0, c, 0, 0, false, 1.0, Some(at)).unwrap();
        assert!(h.set(v, Param::Tempo, 0.8));
        block(&mut h, 0, 512);
        h.stop(v, 0.0);
        block(&mut h, 0, 512);
        h.poll();
    }
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    assert!(h.set(v, Param::Tempo, 0.8));
    let (made, lent) = h.shifters_in_use();
    assert!(
        made <= 2,
        "{made} etireurs crees : les anciens ne revenaient pas"
    );
    assert_eq!(lent, 1, "la voix neuve a le sien");
    // Une vitesse infinie est refusee (elle menait a du NaN).
    h.set(v, Param::Rate, f64::INFINITY);
    let out = block(&mut h, 0, 512);
    assert!(out.iter().all(|x| x.is_finite()));
}

#[test]
fn un_saut_vers_une_page_absente_ne_tombe_pas_dans_le_silence() {
    // Une voix qui joue saute 200 s plus loin (page absente) : le saut
    // attend sa page, aucun bloc ne lit une page manquante.
    let (mut h, c) = streamed_host(600, None);
    let p = pages_of(&h, c);
    let v = h.play(0, c, 0, 0, false, 1.0).unwrap();
    realtime(&mut h, 30);
    let far = 48000 * 200;
    assert!(!p.has(p.page_of(far)));
    assert!(h.set(v, Param::Pos, far as f64));
    realtime(&mut h, 60);
    let pos = h.voice(v).map_or(0.0, |x| x.0);
    assert!(pos > far as f64, "la voix a saute : {pos}");
    assert_eq!(p.misses(), 0, "aucun bloc dans le silence");
}

#[test]
fn un_saut_d_une_voix_etiree_ne_claque_pas() {
    // Un La grave etire (tempo 0,8), puis des sauts a des phases diverses :
    // l'ancienne sortie de l'etireur croise la nouvelle en 5 ms ; d'un
    // echantillon au suivant, jamais plus que la pente d'un sinus et ce
    // fondu (une coupure seche faisait jusqu'a 1).
    let mut h = AudioHost::new(None, 48000.0);
    h.set_shift_factory(Box::new(|| Box::new(cp_engine::shift::LinearRt::new(1000))));
    let n = 48000 * 4;
    let s: Vec<f32> = (0..n)
        .map(|i| (std::f64::consts::TAU * 100.0 * i as f64 / 48000.0).sin() as f32 * 0.5)
        .collect();
    let c = h.load_samples(s, 1, 48000.0).unwrap();
    block(&mut h, 0, 512);
    let v = h.play(0, c, 0, 0, true, 1.0).unwrap();
    assert!(h.set(v, Param::Tempo, 0.8));
    let mut worst = 0.0f64;
    let mut prev: Option<f64> = None;
    for jump in 0..6 {
        for _ in 0..12 {
            for f in block(&mut h, 0, 512).chunks(2) {
                if let Some(p) = prev {
                    worst = worst.max((f[0] - p).abs());
                }
                prev = Some(f[0]);
            }
        }
        // Un saut a une phase differente a chaque fois (en frames source).
        h.set(v, Param::Pos, (48000 + jump * 4801 + 117) as f64);
    }
    eprintln!("voix etiree, sauts : pas le plus grand {worst:.4}");
    assert!(worst < 0.03, "un saut claque : {worst:.3}");
}
