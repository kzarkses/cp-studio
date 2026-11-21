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
