use super::*;

/// Un riff de guitare : le canal est la corde (comme dans Guitar Tab).
fn riff() -> Roll {
    Roll::from_notes([
        (0.0, 0.5, 40, 5, 96, false, false),
        (0.5, 0.5, 47, 4, 90, false, false),
        (1.0, 0.5, 52, 3, 100, false, false),
        (1.5, 0.25, 55, 2, 80, false, false),
        (4.0, 1.0, 64, 0, 110, false, false),
    ])
}

fn chans(r: &Roll) -> Vec<(i32, i32)> {
    r.notes.iter().map(|n| (n.pitch, n.chan)).collect()
}

#[test]
fn le_canal_ne_change_jamais() {
    let mut r = riff();
    r.select_all();
    r.duplicate(4.0, 0);
    r.select_all();
    r.copy();
    r.paste(16.0);
    r.select_all();
    r.transpose(2);
    r.move_sel(0.5, 1);
    r.split_at(1.2);
    r.select_all();
    r.glue();
    r.select_all();
    r.quantize(0.25, 1.0, 0.0);
    // Chaque note garde le canal de sa corde d'origine (hauteur d'origine
    // + 3 demi-tons apres transposition et deplacement).
    let want: std::collections::HashMap<i32, i32> = [(40, 5), (47, 4), (52, 3), (55, 2), (64, 0)]
        .into_iter()
        .collect();
    for (p, c) in chans(&r) {
        assert_eq!(want.get(&(p - 3)), Some(&c), "hauteur {p}, canal {c}");
    }
}

#[test]
fn la_selection() {
    let mut r = riff();
    assert_eq!(r.select_box(0.0, 1.1, 40, 50, false), 2);
    assert_eq!(r.sel_count(), 2);
    let id = r.notes[3].id;
    r.toggle(id);
    assert_eq!(r.sel_count(), 3);
    r.select_row(64, false);
    assert_eq!(r.selected(), vec![r.notes[4].id]);
    r.select_only(r.notes[0].id);
    r.select_range_to(r.notes[3].id);
    assert_eq!(r.sel_count(), 4);
    assert_eq!(r.select_measure(4.5, 4.0), 1);
    assert_eq!(r.select_from(1.0), 3);
    r.invert_sel();
    assert_eq!(r.sel_count(), 2);
}

#[test]
fn deplacer_allonger_etirer() {
    let mut r = riff();
    r.select_box(0.0, 0.6, 0, 127, false);
    r.move_sel(1.0, 12);
    let ps: Vec<i32> = r.notes.iter().filter(|n| n.sel).map(|n| n.pitch).collect();
    assert_eq!(ps, vec![52, 59]);
    r.resize_sel(0.25);
    assert!(
        r.notes
            .iter()
            .filter(|n| n.sel)
            .all(|n| (n.len - 0.75).abs() < 1e-9)
    );
    r.trim_start_sel(0.5);
    let n = r.notes.iter().find(|n| n.sel).unwrap();
    assert!((n.qn0 - 1.5).abs() < 1e-9 && (n.end() - 1.75).abs() < 1e-9);
    // Etirer du double autour de 1.5.
    r.stretch_sel(1.5, 2.0);
    let n = r.notes.iter().filter(|n| n.sel).nth(1).unwrap();
    assert!((n.qn0 - 2.5).abs() < 1e-9, "{}", n.qn0);
}

#[test]
fn quantifier_avec_force_et_swing() {
    let mut r = Roll::from_notes([
        (0.1, 0.25, 60, 0, 100, false, false),
        (0.6, 0.25, 62, 0, 100, false, false),
    ]);
    r.quantize(0.5, 0.5, 0.0);
    assert!((r.notes[0].qn0 - 0.05).abs() < 1e-9);
    r.quantize(0.5, 1.0, 0.0);
    assert!((r.notes[0].qn0 - 0.0).abs() < 1e-9);
    assert!((r.notes[1].qn0 - 0.5).abs() < 1e-9);
    // Swing : le pas impair est retarde.
    r.quantize(0.5, 1.0, 0.5);
    assert!((r.notes[1].qn0 - 0.625).abs() < 1e-9, "{}", r.notes[1].qn0);
}

#[test]
fn la_duplication_tombe_sur_l_unite() {
    // Quatre doubles croches sur le temps 1 : la copie sur le temps 2.
    let mut r = Roll::from_notes((0..4).map(|k| (k as f64 * 0.25, 0.2, 60, 0, 100, true, false)));
    assert!((r.dup_offset(4.0, 0.25) - 1.0).abs() < 1e-9);
    // Une double croche seule : le pas de grille.
    let mut one = Roll::from_notes([(0.5, 0.2, 60, 0, 100, true, false)]);
    assert!((one.dup_offset(4.0, 0.25) - 0.25).abs() < 1e-9);
    // Une mesure pleine : la mesure suivante.
    let mut bar =
        Roll::from_notes((0..16).map(|k| (k as f64 * 0.25, 0.25, 60, 0, 100, true, false)));
    assert!((bar.dup_offset(4.0, 0.25) - 4.0).abs() < 1e-9);
    let d = r.dup_offset(4.0, 0.25);
    assert_eq!(r.duplicate(d, 0), 4);
    assert_eq!(r.notes.len(), 8);
    assert_eq!(r.sel_count(), 4);
    assert!(r.notes.iter().filter(|n| n.sel).all(|n| n.qn0 >= 1.0));
    one.select_all();
    bar.select_all();
}

#[test]
fn legato_glue_couper() {
    let mut r = Roll::from_notes([
        (0.0, 0.25, 60, 3, 100, false, false),
        (1.0, 0.25, 62, 3, 80, false, false),
        (2.0, 0.25, 64, 3, 60, false, false),
    ]);
    assert_eq!(r.legato(), 2);
    assert!((r.notes[0].len - 1.0).abs() < 1e-9);
    // Deux notes de meme hauteur qui se touchent : une seule, vel moyenne.
    let mut g = Roll::from_notes([
        (0.0, 1.0, 60, 2, 100, true, false),
        (1.0, 1.0, 60, 2, 60, true, false),
        (1.0, 1.0, 64, 2, 90, true, false),
    ]);
    assert_eq!(g.glue(), 2);
    let n = g.notes.iter().find(|n| n.pitch == 60).unwrap();
    assert!((n.len - 2.0).abs() < 1e-9 && n.vel == 80 && n.chan == 2);
    assert_eq!(g.split_at(0.5), 1);
    assert_eq!(g.notes.len(), 3);
}

#[test]
fn la_gamme() {
    let s = Scale { root: 4, kind: 2 }; // Mi mineur
    assert!(s.contains(64) && s.contains(66) && !s.contains(65));
    assert_eq!(s.snap(65), 64);
    assert_eq!(s.step(64, 1), 66);
    assert_eq!(s.step(64, -1), 62);
    assert_eq!(s.name(), "E minor");
    let mut r = Roll::from_notes([(0.0, 1.0, 65, 0, 100, true, false)]);
    r.scale = Some(s);
    assert_eq!(r.snap_to_scale(), 1);
    assert_eq!(r.notes[0].pitch, 64);
    r.transpose_scale(2);
    assert_eq!(r.notes[0].pitch, 67);
    assert_eq!(note_name(60), "C4");
}

#[test]
fn velocite() {
    let mut r = riff();
    r.select_all();
    r.vel_add(10);
    assert_eq!(r.notes[4].vel, 120);
    r.vel_ramp(40, 120);
    assert_eq!(r.notes[0].vel, 40);
    assert_eq!(r.notes[4].vel, 120);
    r.vel_scale(0.5, 64);
    assert_eq!(r.notes[0].vel, 52);
    let mut a = riff();
    let mut b = riff();
    a.humanize(7, 0.05, 10);
    b.humanize(7, 0.05, 10);
    assert_eq!(a.notes, b.notes, "meme graine, meme jeu");
}

#[test]
fn renverser_et_miroir() {
    let mut r = Roll::from_notes([
        (0.0, 1.0, 60, 0, 100, false, false),
        (2.0, 1.0, 67, 0, 100, false, false),
    ]);
    r.reverse();
    assert!((r.notes[0].qn0 - 0.0).abs() < 1e-9 && r.notes[0].pitch == 67);
    r.invert_pitch();
    assert_eq!(r.notes[0].pitch, 60);
}

#[test]
fn copier_glisser_laisse_la_copie() {
    let mut r = riff();
    let id = r.notes[0].id;
    r.select_only(id);
    assert_eq!(r.stamp_sel(), 1);
    r.move_sel(2.0, 0);
    assert_eq!(r.notes.len(), 6);
    assert!(
        r.notes
            .iter()
            .any(|n| n.pitch == 40 && n.qn0 == 0.0 && !n.sel)
    );
    assert!(
        r.notes
            .iter()
            .any(|n| n.pitch == 40 && n.qn0 == 2.0 && n.sel)
    );
}

#[test]
fn muet_grille_arpege_gardent_le_canal() {
    // Un accord de trois notes sur trois canaux (trois cordes).
    let mut r = Roll::from_notes([
        (0.0, 1.0, 40, 5, 100, true, false),
        (0.0, 1.0, 47, 4, 100, true, false),
        (0.0, 1.0, 52, 3, 100, true, false),
    ]);
    assert_eq!(r.mute_sel(None), 3);
    assert!(r.notes.iter().all(|n| n.mute));
    r.mute_sel(Some(false));
    assert!(r.notes.iter().all(|n| !n.mute));
    // Couper sur la grille de la double croche : 4 morceaux par note.
    assert_eq!(r.split_grid(0.25), 9);
    assert_eq!(r.notes.len(), 12);
    let mut c: Vec<i32> = r.notes.iter().map(|n| n.chan).collect();
    c.sort_unstable();
    c.dedup();
    assert_eq!(
        c,
        vec![3, 4, 5],
        "le canal de chaque morceau est celui de sa note"
    );
    // L'arpege : la note la plus aigue avance d'une noire, la plus grave reste.
    let mut a = Roll::from_notes([
        (0.0, 1.0, 40, 5, 100, true, false),
        (0.0, 1.0, 47, 4, 100, true, false),
        (0.0, 1.0, 52, 3, 100, true, false),
    ]);
    let top = a.notes.iter().find(|n| n.pitch == 52).unwrap().id;
    a.arpeggiate(top, 1.0);
    let at = |p: i32| a.notes.iter().find(|n| n.pitch == p).unwrap().qn0;
    assert!(at(40).abs() < 1e-9 && (at(52) - 1.0).abs() < 1e-9);
    assert!((at(47) - 7.0 / 12.0).abs() < 1e-9);
    // Sans la gamme : un demi-ton hors de la gamme reste ou on le met.
    let mut s = Roll::from_notes([(0.0, 1.0, 60, 2, 100, true, false)]);
    s.scale = Some(crate::Scale { root: 0, kind: 1 });
    s.move_sel_free(0.0, 1);
    assert_eq!((s.notes[0].pitch, s.notes[0].chan), (61, 2));
}

#[test]
fn accords_arpeges_euclides_gardent_le_canal() {
    // Deux notes sur le canal 3 (une corde) : des accords majeurs.
    let mut r = Roll::from_notes([
        (0.0, 1.0, 60, 3, 100, true, false),
        (2.0, 1.0, 62, 3, 90, true, false),
    ]);
    assert_eq!(r.chordify(1), 4);
    let mut p: Vec<(i32, i32)> = r.notes.iter().map(|n| (n.pitch, n.chan)).collect();
    p.sort();
    assert_eq!(
        p,
        vec![(60, 3), (62, 3), (64, 3), (66, 3), (67, 3), (69, 3)]
    );
    // En Do majeur, la triade du Re est mineure (Re Fa La).
    let mut r = Roll::from_notes([(0.0, 1.0, 62, 5, 100, true, false)]);
    r.scale = Some(Scale { root: 0, kind: 1 });
    r.chordify(0);
    let mut p: Vec<i32> = r.notes.iter().map(|n| n.pitch).collect();
    p.sort();
    assert_eq!(p, vec![62, 65, 69]);
    assert!(r.notes.iter().all(|n| n.chan == 5));

    // L'accord arpege : Do Mi Sol sur une noire, en doubles croches,
    // montant : Do Mi Sol Do.
    let mut r = Roll::from_notes([
        (4.0, 1.0, 60, 2, 100, true, false),
        (4.0, 1.0, 64, 2, 80, true, false),
        (4.0, 1.0, 67, 2, 90, true, false),
    ]);
    assert_eq!(r.arp(0.25, ArpMode::Up, 0.9, 1, 1), 4);
    let seq: Vec<(f64, i32, i32)> = r.notes.iter().map(|n| (n.qn0, n.pitch, n.chan)).collect();
    assert_eq!(
        seq,
        vec![(4.0, 60, 2), (4.25, 64, 2), (4.5, 67, 2), (4.75, 60, 2)]
    );
    assert!(
        r.notes
            .iter()
            .all(|n| n.vel == 90 && (n.len - 0.225).abs() < 1e-9)
    );
    // Monte et descend, sur deux octaves.
    let mut r = Roll::from_notes([
        (0.0, 2.0, 60, 0, 100, true, false),
        (0.0, 2.0, 64, 0, 100, true, false),
    ]);
    r.arp(0.25, ArpMode::UpDown, 1.0, 2, 1);
    let ps: Vec<i32> = r.notes.iter().map(|n| n.pitch).collect();
    assert_eq!(ps, vec![60, 64, 72, 76, 72, 64, 60, 64]);

    // Trois coups sur huit cases (le tresillo), le premier sur le temps.
    let mut r = Roll::new();
    assert_eq!(r.euclid(0.0, 2.0, 36, 9, 110, 8, 3, 0), 3);
    let at: Vec<f64> = r.notes.iter().map(|n| n.qn0).collect();
    assert_eq!(at, vec![0.0, 0.75, 1.5]);
    assert!(r.notes.iter().all(|n| n.chan == 9 && n.sel));
}
