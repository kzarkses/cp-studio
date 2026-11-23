//! L'arrangeur pilote hors de REAPER : deux pistes de guitare, de vrais
//! riffs doigtes par le moteur, les vraies polices (GDI), des gestes, et des
//! planches PNG dans `%TEMP%` (cp_tabview_*.png).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png};
use cp_sys::font::GdiGlyphs;
use cp_tab::score::TabNote;
use cp_ui::{Button, Ev, IconCache, Mods, Theme, vk};

use super::*;

struct Bench {
    s: Surface,
    k: Scratch,
    t: TextCache,
    icons: IconCache,
    th: Theme,
}

impl Bench {
    fn new(w: i32, h: i32) -> Bench {
        Bench {
            s: Surface::new(w, h),
            k: Scratch::default(),
            t: TextCache::new(Box::new(GdiGlyphs::new(&[
                "Segoe UI",
                "Consolas",
                "Segoe UI",
                "Segoe UI Symbol",
            ]))),
            icons: IconCache::default(),
            th: Theme::reaper(1.0, &|k| match k {
                "genlist_bg" => Some((0x32, 0x32, 0x32)),
                "genlist_fg" => Some((0xc8, 0xc8, 0xc8)),
                "col_main_bg2" => Some((0x3e, 0x3e, 0x3e)),
                "col_toolbar_text_on" => Some((0x1a, 0xbc, 0x98)),
                "genlist_selbg" => Some((0x46, 0x61, 0x68)),
                "col_main_editbk" => Some((0x2a, 0x2a, 0x2a)),
                "playcursor_color" => Some((0xfe, 0xc1, 0x3a)),
                _ => None,
            }),
        }
    }

    fn draw(&mut self, v: &mut Arr) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(&mut cv, &mut self.t, &mut self.icons, &self.th, w, h);
    }

    fn shot(&mut self, v: &mut Arr, name: &str) {
        self.draw(v);
        let p = std::env::temp_dir().join(format!("cp_tabview_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

const OPEN: [i32; 6] = [64, 59, 55, 50, 45, 40];

/// Un item : (mesure, double croche, duree, [(corde 1..6, case)], marque) ;
/// marque « L » legato, « G » fantome.
type Raw = (usize, f64, f64, &'static [(usize, i32)], &'static str);

fn item(guid: &str, name: &str, bar0: usize, bars: usize, raw: &[Raw]) -> Item {
    let o = bar0 as f64 * 4.0;
    let mut notes = Vec::new();
    let mut cands = Vec::new();
    for &(m, t, d, ch, mark) in raw {
        for &(s, f) in ch {
            let n = TabNote {
                qn0: o + m as f64 * 4.0 + t / 4.0 + 0.01,
                qn1: o + m as f64 * 4.0 + (t + d) / 4.0,
                pitch: OPEN[s - 1] + f,
                legato: mark == "L",
                vel: 90,
                ..Default::default()
            };
            if mark == "G" {
                cands.push(n);
            } else {
                notes.push(n);
            }
        }
    }
    let detect = Detect::default();
    let tuning = detect.tuning();
    let (doc, dropped) = cp_tab::doc::finger(&notes, 4, 112.0, &tuning);
    let ghosts = cp_tab::doc::ghosts(&cands, &doc, 4, &tuning);
    let qn1 = o + bars as f64 * 4.0;
    let frames = 600usize;
    let frame_qn: Vec<f64> = (0..frames)
        .map(|f| o + f as f64 * (qn1 - o) / frames as f64)
        .collect();
    let mut env = vec![0.02f32; frames];
    for n in &doc {
        let f0 = ((n.qn0 - o) / (qn1 - o) * frames as f64) as usize;
        for k in 0..40 {
            if f0 + k < frames {
                env[f0 + k] = env[f0 + k].max(0.85 * (-(k as f32) / 12.0).exp());
            }
        }
    }
    let mut it = Item {
        guid: guid.into(),
        take: format!("{guid}-take"),
        name: name.into(),
        qn0: o,
        qn1,
        origin_qn: o,
        sig: (4, 4),
        bpm: 112.0,
        takes: vec![
            TakeInfo {
                name: "take 1".into(),
                done: true,
            },
            TakeInfo {
                name: "take 2".into(),
                done: true,
            },
        ],
        active: 1,
        state: Some(State::Ready),
        midi: true,
        frame_qn,
        env,
        onset: vec![0.0; frames],
        detect,
        dropped,
        ..Item::default()
    };
    it.ghosts = ghosts;
    it.set_doc(doc);
    it
}

fn arranger() -> Arr {
    const EM: &[(usize, i32)] = &[(6, 0), (5, 2), (4, 2), (3, 0), (2, 0), (1, 0)];
    const C: &[(usize, i32)] = &[(5, 3), (4, 2), (3, 0), (2, 1), (1, 0)];
    const G: &[(usize, i32)] = &[(6, 3), (5, 2), (4, 0), (3, 0), (2, 0), (1, 3)];
    const D: &[(usize, i32)] = &[(4, 0), (3, 2), (2, 3), (1, 2)];
    let mut rhythm_a: Vec<Raw> = Vec::new();
    for (m, ch) in [(0usize, EM), (1, C)] {
        for (t, d) in [
            (0.0, 4.0),
            (4.0, 2.0),
            (6.0, 2.0),
            (8.0, 4.0),
            (12.0, 2.0),
            (14.0, 2.0),
        ] {
            rhythm_a.push((m, t, d, ch, ""));
        }
    }
    let mut rhythm_b: Vec<Raw> = Vec::new();
    for (m, ch) in [(0usize, G), (1, D)] {
        for (t, d) in [(0.0, 4.0), (4.0, 2.0), (6.0, 2.0), (8.0, 4.0), (12.0, 2.0)] {
            rhythm_b.push((m, t, d, ch, ""));
        }
    }
    rhythm_b.push((1, 15.0, 1.0, &[(3, 4)], "G"));
    let lead: [Raw; 16] = [
        (0, 0.0, 2.0, &[(3, 0)], ""),
        (0, 2.0, 2.0, &[(3, 2)], ""),
        (0, 4.0, 1.0, &[(2, 0)], ""),
        (0, 5.0, 1.0, &[(2, 1)], "L"),
        (0, 6.0, 1.0, &[(2, 3)], "L"),
        (0, 7.0, 1.0, &[(1, 0)], ""),
        (0, 8.0, 2.0, &[(1, 3)], ""),
        (0, 10.0, 2.0, &[(1, 2)], ""),
        (0, 12.0, 2.0, &[(1, 0)], ""),
        (0, 14.0, 1.0, &[(2, 3)], ""),
        (0, 15.0, 1.0, &[(2, 1)], ""),
        (1, 0.0, 4.0, &[(2, 0)], ""),
        (1, 4.0, 1.0, &[(3, 2)], "G"),
        (1, 5.0, 1.0, &[(3, 0)], ""),
        (1, 6.0, 2.0, &[(4, 2)], ""),
        (1, 8.0, 4.0, &[(3, 0)], ""),
    ];
    let mut v = Arr::new();
    v.bpm = 112.0;
    v.tracks = vec![
        Track {
            head: Head {
                guid: "T1".into(),
                tab_guid: Some("T1tab".into()),
                num: 1,
                name: "Rhythm".into(),
                vol_db: -3.2,
                tab_vol_db: -9.0,
                synth: Some(Synth {
                    sound: Sound::Guitar,
                    tone: 0.55,
                    sustain: 1.2,
                    mono: true,
                }),
                lanes: Lanes([true, true, true, true]),
                ..Head::default()
            },
            items: vec![
                item("A", "Rhythm", 8, 2, &rhythm_a),
                item("B", "Rhythm 2", 10, 2, &rhythm_b),
            ],
        },
        Track {
            head: Head {
                guid: "T2".into(),
                tab_guid: Some("T2tab".into()),
                num: 3,
                name: "Lead".into(),
                armed: true,
                input: "In 2".into(),
                monitor: 1,
                vol_db: -1.5,
                tab_vol_db: -12.0,
                meter: 0.62,
                synth: Some(Synth {
                    sound: Sound::Piano,
                    tone: 0.4,
                    sustain: 2.0,
                    mono: true,
                }),
                lanes: Lanes([true, false, false, true]),
                ..Head::default()
            },
            items: vec![item("C", "Lead", 8, 2, &lead)],
        },
    ];
    v.scroll = 31.6;
    v.playhead = Some(46.5);
    v.rec = vec![Rec {
        track: 1,
        qn0: 40.0,
        env: (0..60)
            .map(|k| (40.0 + k as f64 * 0.1, 0.3 + 0.5 * ((k % 7) as f32 / 7.0)))
            .collect(),
    }];
    v.sel = Some(Sel {
        t: 0,
        i: Some(0),
        n: None,
    });
    v
}

fn click(v: &mut Arr, x: f32, y: f32) -> Option<Act> {
    let a = v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: Mods::default(),
    });
    let b = v.event(&Ev::Up {
        x,
        y,
        b: Button::Left,
        mods: Mods::default(),
    });
    a.or(b)
}

fn dbl(v: &mut Arr, x: f32, y: f32) -> Option<Act> {
    let a = v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: true,
        mods: Mods::default(),
    });
    v.event(&Ev::Up {
        x,
        y,
        b: Button::Left,
        mods: Mods::default(),
    });
    a
}

fn key(v: &mut Arr, k: u32) -> Option<Act> {
    v.event(&Ev::Key {
        vk: k,
        mods: Mods::default(),
        text: None,
    })
}

fn center(r: (f32, f32, f32, f32)) -> (f32, f32) {
    (r.0 + r.2 * 0.5, r.1 + r.3 * 0.5)
}

fn find(v: &Arr, f: impl Fn(&Hit) -> bool) -> Option<(f32, f32)> {
    v.boxes
        .iter()
        .rev()
        .find(|b| f(&b.4))
        .map(|b| center((b.0, b.1, b.2, b.3)))
}

#[test]
fn l_arrangeur_se_dessine() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.shot(&mut v, "1_pistes");
    // Les deux pistes ont leurs cibles : en-tetes, couches, notes.
    assert!(v.rect_of(Hit::Arm(1)).is_some());
    assert!(v.rect_of(Hit::HScroll).is_some());
    assert!(v.rect_of(Hit::Help).is_some());
    assert!(find(&v, |h| matches!(h, Hit::Note { t: 0, i: 0, .. })).is_some());
    assert!(find(&v, |h| matches!(h, Hit::Note { t: 1, i: 0, .. })).is_some());
    // Les mesures d'un item sont pleines.
    let e = v.tracks[0].items[0].engraved().clone();
    for m in 0..e.measures {
        let sum: i64 = e
            .signs
            .iter()
            .filter(|s| s.measure == m)
            .map(|s| s.dur)
            .sum();
        assert_eq!(sum, e.mlen);
    }
    // Sans piste : l'invitation a en creer une.
    let mut empty = Arr::new();
    b.shot(&mut empty, "2_vide");
    assert!(empty.rect_of(Hit::AddTrack).is_some());
}

#[test]
fn corriger_une_note_dans_un_item() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.draw(&mut v);
    // Une note de la tab du second item de la premiere piste.
    let (x, y) = find(&v, |h| matches!(h, Hit::Note { t: 0, i: 1, .. })).expect("une note");
    assert_eq!(click(&mut v, x, y), None);
    let s = v.sel.expect("choisie");
    assert_eq!((s.t, s.i), (0, Some(1)));
    let n = s.n.expect("une note");
    // 1 puis 2 vite : la case 12, ecrite dans l'item MIDI de cet item.
    assert_eq!(key(&mut v, 0x31), Some(Act::Edited { t: 0, i: 1 }));
    assert_eq!(key(&mut v, 0x32), Some(Act::Edited { t: 0, i: 1 }));
    let it = &v.tracks[0].items[1];
    let note = &it.doc[v.sel.unwrap().n.unwrap_or(n)];
    assert_eq!(note.fret, 12);
    assert!(note.locked);
    // Effacer la note, puis (rien de choisi dans l'item) l'item entier.
    let k = note.key();
    assert_eq!(key(&mut v, vk::DELETE), Some(Act::Edited { t: 0, i: 1 }));
    assert!(v.tracks[0].items[1].suppressed.contains(&k));
    v.select(Some(Sel {
        t: 0,
        i: Some(1),
        n: None,
    }));
    assert_eq!(
        key(&mut v, vk::DELETE),
        Some(Act::DeleteItem { t: 0, i: 1 })
    );
    b.shot(&mut v, "3_corrige");
}

#[test]
fn fantomes_midi_et_couches() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.draw(&mut v);
    // Un fantome garde d'un clic.
    let g0 = v.tracks[0].items[1].ghosts.len();
    let (gx, gy) = find(&v, |h| matches!(h, Hit::Ghost { t: 0, i: 1, .. })).expect("un fantome");
    assert_eq!(click(&mut v, gx, gy), Some(Act::Edited { t: 0, i: 1 }));
    assert_eq!(v.tracks[0].items[1].ghosts.len(), g0 - 1);
    b.draw(&mut v);
    // Double-clic sur le MIDI : l'editeur MIDI.
    let (mx, my) = find(&v, |h| {
        *h == Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Midi,
        }
    })
    .expect("le MIDI");
    assert_eq!(dbl(&mut v, mx, my), Some(Act::OpenMidi { t: 0, i: 0 }));
    // Cacher la couche MIDI (le menu de la piste) : la piste raccourcit.
    let h0 = draw::track_h(&v.tracks[0], 1.0, v.vzoom);
    assert_eq!(v.toggle_lane(0, Lane::Midi), Some(Act::Lanes { t: 0 }));
    assert!(draw::track_h(&v.tracks[0], 1.0, v.vzoom) < h0);
    assert_eq!(v.tracks[0].head.lanes.to_text(), "1011");
    b.shot(&mut v, "4_sans_midi");
}

/// Les gestes de REAPER chez Cedric, et la barre horizontale.
#[test]
fn naviguer_comme_dans_reaper() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    v.scroll = 8.0;
    b.draw(&mut v);
    let l = v.lay;
    let (mx, my) = (l.sx + l.sw * 0.5, l.top + l.ruler_h + 60.0);
    let wheel = |v: &mut Arr, dx: f32, dy: f32, m: Mods| {
        v.event(&Ev::Wheel {
            x: mx,
            y: my,
            dx,
            dy,
            mods: m,
        })
    };
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    // Maj + molette : zoom, la noire sous la souris ne bouge pas.
    let q = v.qn_of(mx);
    let z0 = v.zoom;
    wheel(&mut v, 0.0, 1.0, shift);
    assert!(v.zoom > z0);
    assert!((v.qn_of(mx) - q).abs() < 1e-3);
    // Alt + molette : dans le temps, vers le haut la vue avance (comme
    // REAPER chez Cedric) ; la molette seule : verticalement.
    let s0 = v.scroll;
    wheel(&mut v, 0.0, 1.0, alt);
    assert!(v.scroll > s0);
    let s1 = v.scroll;
    wheel(&mut v, 0.0, -1.0, Mods::default());
    assert_eq!(v.scroll, s1);
    // Ctrl + molette : la hauteur des couches audio et MIDI.
    let h0 = draw::track_h(&v.tracks[0], 1.0, v.vzoom);
    wheel(&mut v, 0.0, 2.0, ctrl);
    assert!(draw::track_h(&v.tracks[0], 1.0, v.vzoom) > h0);
    b.draw(&mut v);
    // Le bouton du milieu sur la regle : la vue suit la main.
    let ry = v.lay.top + v.lay.ruler_h * 0.5;
    let q = v.qn_of(mx);
    v.event(&Ev::Down {
        x: mx,
        y: ry,
        b: Button::Middle,
        dbl: false,
        mods: Mods::default(),
    });
    v.event(&Ev::Move {
        x: mx - 120.0,
        y: ry + 2.0,
        mods: Mods::default(),
    });
    assert!(
        (v.qn_of(mx - 120.0) - q).abs() < 1e-3,
        "la noire saisie suit"
    );
    // ... et vers le haut, elle zoome autour d'elle.
    let z1 = v.zoom;
    v.event(&Ev::Move {
        x: mx - 120.0,
        y: ry - 60.0,
        mods: Mods::default(),
    });
    assert!(v.zoom > z1);
    assert!((v.qn_of(mx - 120.0) - q).abs() < 1e-3);
    v.event(&Ev::Up {
        x: mx - 120.0,
        y: ry - 60.0,
        b: Button::Middle,
        mods: Mods::default(),
    });
    // La barre horizontale : un clic au bout ramene la fin.
    b.draw(&mut v);
    let r = v.rect_of(Hit::HScroll).unwrap();
    let s2 = v.scroll;
    click(&mut v, r.0 + r.2 - 2.0, r.1 + r.3 * 0.5);
    assert!(v.scroll > s2);
    // Les infobulles : les mots sont la, pas a l'ecran.
    assert!(v.label(Hit::Help).unwrap().contains("Middle drag"));
    assert!(v.label(Hit::Slider(0)).unwrap().contains("Lower"));
    b.shot(&mut v, "5_navigation");
}

#[test]
fn en_tete_et_ecoute() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.draw(&mut v);
    let (x, y) = center(v.rect_of(Hit::Arm(0)).unwrap());
    assert_eq!(click(&mut v, x, y), Some(Act::Arm { t: 0 }));
    // Le fader de la tab, tire vers la droite.
    let r = v.rect_of(Hit::Fader { t: 0, tab: true }).unwrap();
    let a = click(&mut v, r.0 + r.2 * 0.9, r.1 + r.3 * 0.5);
    match a {
        Some(Act::Volume {
            t: 0,
            tab: true,
            db,
        }) => assert!(db > 0.0, "{db}"),
        other => panic!("{other:?}"),
    }
    // Le son de la tab.
    let (x, y) = center(
        v.rect_of(Hit::SoundPick {
            t: 0,
            s: Sound::Bass,
        })
        .unwrap(),
    );
    assert_eq!(
        click(&mut v, x, y),
        Some(Act::Sound {
            t: 0,
            s: Sound::Bass
        })
    );
    assert_eq!(v.tracks[0].head.synth.unwrap().sound, Sound::Bass);
    // L'ecoute : la tab seule.
    let (x, y) = center(v.rect_of(Hit::Listen(Listen::Tab)).unwrap());
    assert_eq!(click(&mut v, x, y), Some(Act::Listen));
    assert_eq!(v.listen, Listen::Tab);
    // R arme la piste choisie ; Espace joue.
    assert_eq!(key(&mut v, 0x52), Some(Act::Arm { t: 0 }));
    assert_eq!(key(&mut v, vk::SPACE), Some(Act::Play));
}

#[test]
fn reglages_d_un_item_et_selection_retrouvee() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.draw(&mut v);
    // Le curseur de sensibilite de l'item choisi.
    let r = v.rect_of(Hit::Slider(0)).unwrap();
    let a = click(&mut v, r.0 + r.2 * 0.2, r.1 + r.3 * 0.5);
    assert_eq!(
        a,
        Some(Act::Detect {
            t: 0,
            i: 0,
            range: None
        })
    );
    assert!(v.tracks[0].items[0].detect.onset < 0.5);
    // Une prise de l'inspecteur.
    let (x, y) = center(v.rect_of(Hit::Take(0)).unwrap());
    assert_eq!(click(&mut v, x, y), Some(Act::SetTake { t: 0, i: 0, k: 0 }));
    // Le module rafraichit les pistes (un item s'est insere avant) : la
    // selection suit son item.
    v.select(Some(Sel {
        t: 0,
        i: Some(1),
        n: Some(2),
    }));
    let k = v.tracks[0].items[1].doc[2].key();
    let mut extra = v.tracks[0].items[0].clone();
    extra.guid = "Z".into();
    v.tracks[0].items.insert(0, extra);
    v.reselect();
    let s = v.sel.unwrap();
    assert_eq!(s.i, Some(2));
    assert_eq!(v.tracks[0].items[2].doc[s.n.unwrap()].key(), k);
}

#[test]
fn les_reglages_se_relisent() {
    let d = Detect {
        onset: 0.38,
        frame: 0.27,
        min_ms: 58.0,
        grid: 8,
        legato: false,
        ghosts: true,
        tuning: 1,
    };
    assert_eq!(Detect::from_text(&d.to_text()), d);
    assert_eq!(Detect::from_text("rien de lisible"), Detect::default());
    assert_eq!(Lanes::from_text("0000").to_text(), "0001");
    assert_eq!(Lanes::from_text("bad"), Lanes::default());
    assert_eq!(Listen::from_key(Listen::Original.key()), Listen::Original);
}

/// Glisser une prise audio : dans le temps (calee a la double croche), puis
/// vers l'autre guitare ; un simple clic place seulement le curseur.
#[test]
fn glisser_une_prise() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    b.draw(&mut v);
    let audio = |h: &Hit| {
        *h == Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Audio,
        }
    };
    let (x, y) = find(&v, audio).expect("la prise audio");
    // Un clic : le curseur, et l'item de la couche cliquee (ici l'audio) dans
    // REAPER et l'editeur ; sur la couche MIDI, son MIDI.
    assert!(matches!(
        click(&mut v, x, y),
        Some(Act::ClickItem {
            t: 0,
            i: 0,
            lane: Lane::Audio,
            ..
        })
    ));
    let midi = |h: &Hit| {
        *h == Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Midi,
        }
    };
    let (mx, my) = find(&v, midi).expect("la prise MIDI");
    assert!(matches!(
        click(&mut v, mx, my),
        Some(Act::ClickItem {
            t: 0,
            i: 0,
            lane: Lane::Midi,
            ..
        })
    ));
    // La couche cliquee a le focus : son item est montre choisi.
    assert_eq!(v.focus_lane, Lane::Midi);
    b.shot(&mut v, "10_focus_midi");
    click(&mut v, x, y);
    assert_eq!(v.focus_lane, Lane::Audio);
    let q0 = v.tracks[0].items[0].qn0;
    let px = v.lay.ppq;
    let m = Mods::default();
    v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: m,
    });
    v.event(&Ev::Move {
        x: x + px * 1.1,
        y,
        mods: m,
    });
    b.draw(&mut v);
    // Vers la guitare du dessous.
    let r1 = v.rect_of(Hit::Arm(1)).unwrap();
    v.event(&Ev::Move {
        x: x + px * 1.1,
        y: r1.1 + 2.0,
        mods: m,
    });
    b.shot(&mut v, "6_glisser");
    let a = v.event(&Ev::Up {
        x: x + px * 1.1,
        y: r1.1 + 2.0,
        b: Button::Left,
        mods: m,
    });
    match a {
        Some(Act::MoveItem {
            t: 0,
            i: 0,
            qn0,
            to: 1,
        }) => {
            assert!((qn0 - (q0 + 1.0)).abs() < 1e-9, "{qn0} (depart {q0})");
        }
        other => panic!("{other:?}"),
    }
}

/// Comme dans l'arrangement : la prise glissee se dessine entiere (son onde)
/// a sa nouvelle place, se cale par le magnetisme de REAPER, Ctrl la copie ;
/// une prise pas encore transcrite montre sa baguette, qui la transcrit.
#[test]
fn glisser_comme_l_arrangement_et_transcrire_sur_demande() {
    let mut b = Bench::new(1600, 760);
    let mut v = arranger();
    // Le magnetisme de REAPER : ici, la mesure entiere.
    v.snapper = Some(Box::new(|q: f64| (q / 4.0).round() * 4.0));
    // La forme d'onde de REAPER sur la premiere prise.
    {
        let it = &mut v.tracks[0].items[0];
        it.peaks = (0..800)
            .map(|k| {
                let a = ((k as f32 * 0.07).sin() * 0.6).abs() * (1.0 - (k % 100) as f32 / 140.0);
                (-a, a)
            })
            .collect();
    }
    // La seconde prise de la seconde guitare n'est pas transcrite.
    {
        let it = &mut v.tracks[1].items[0];
        it.state = Some(State::Idle);
        it.doc.clear();
        it.ghosts.clear();
    }
    b.draw(&mut v);
    let wand = |h: &Hit| matches!(h, Hit::Transcribe { t: 1, i: 0 });
    let (wx, wy) = find(&v, wand).expect("la baguette de la prise pas transcrite");
    assert_eq!(click(&mut v, wx, wy), Some(Act::Transcribe { t: 1, i: 0 }));
    assert!(v.rect_of(Hit::Auto).is_some());
    // Glisser par la couche audio, d'une noire et demie : la mesure la plus
    // proche (le magnetisme), Ctrl : une copie.
    let audio = |h: &Hit| {
        *h == Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Audio,
        }
    };
    let (x, y) = find(&v, audio).expect("la prise audio");
    let q0 = v.tracks[0].items[0].qn0;
    let px = v.lay.ppq;
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: ctrl,
    });
    v.event(&Ev::Move {
        x: x + px * 3.0,
        y,
        mods: ctrl,
    });
    b.shot(&mut v, "7_glisser_onde");
    let a = v.event(&Ev::Up {
        x: x + px * 3.0,
        y,
        b: Button::Left,
        mods: ctrl,
    });
    match a {
        Some(Act::CopyItem {
            t: 0,
            i: 0,
            qn0,
            to: 0,
        }) => assert!(
            (qn0 - ((q0 + 3.0) / 4.0).round() * 4.0).abs() < 1e-9,
            "{qn0}"
        ),
        other => panic!("{other:?}"),
    }
}

/// Une longue prise : `qn` noires, `n` notes doigtees (0 : pas transcrite),
/// `g` fantomes, la forme d'onde de REAPER a 16000 colonnes.
fn long_item(guid: &str, qn0: f64, qn: f64, n: usize, g: usize) -> Item {
    let mut notes = Vec::new();
    for k in 0..n {
        let q = qn0 + k as f64 * qn / n.max(1) as f64;
        notes.push(TabNote {
            qn0: q + 0.01,
            qn1: q + 0.5,
            pitch: 40 + ((k * 7) % 30) as i32,
            legato: false,
            vel: 90,
            ..Default::default()
        });
    }
    let mut cands = Vec::new();
    for k in 0..g {
        let q = qn0 + 0.25 + k as f64 * qn / g.max(1) as f64;
        cands.push(TabNote {
            qn0: q,
            qn1: q + 0.25,
            pitch: 45 + ((k * 5) % 20) as i32,
            legato: false,
            vel: 60,
            ..Default::default()
        });
    }
    let detect = Detect::default();
    let tuning = detect.tuning();
    let (doc, dropped) = cp_tab::doc::finger(&notes, 4, 120.0, &tuning);
    let ghosts = cp_tab::doc::ghosts(&cands, &doc, 4, &tuning);
    let peaks: Vec<(f32, f32)> = (0..16000)
        .map(|k| {
            let a = 0.2 + 0.7 * ((k as f32 * 0.013).sin().abs());
            (-a, a * 0.9)
        })
        .collect();
    let mut it = Item {
        guid: guid.into(),
        take: format!("{guid}-take"),
        name: "Long".into(),
        qn0,
        qn1: qn0 + qn,
        origin_qn: qn0,
        sig: (4, 4),
        bpm: 120.0,
        takes: vec![TakeInfo {
            name: "take 1".into(),
            done: n > 0,
        }],
        state: Some(if n > 0 { State::Ready } else { State::Idle }),
        midi: n > 0,
        peaks,
        detect,
        dropped,
        ..Item::default()
    };
    it.ghosts = ghosts;
    if n > 0 {
        it.set_doc(doc);
    }
    it
}

/// Le cout d'une image, comme chez Cedric (2123 x 1310, une longue prise pas
/// transcrite avec ses fantomes, une prise transcrite). `cargo test -p
/// cp_tabview --release -- --ignored --nocapture banc`
#[test]
#[ignore]
fn banc_image_longue_prise() {
    let mut b = Bench::new(2123, 1310);
    let mut v = arranger();
    v.tracks[0].items = vec![long_item("L", 0.0, 283.0, 0, 69)];
    v.tracks[1].items = vec![long_item("M", 0.0, 283.0, 178, 69)];
    v.tracks[1].head.lanes = Lanes([true, true, true, true]);
    v.rec.clear();
    for (name, zoom) in [("loin", 6.0f32), ("moyen", 30.0), ("pres", 120.0)] {
        v.zoom = zoom;
        v.scroll = 40.0;
        b.draw(&mut v);
        let t = std::time::Instant::now();
        let n = 10;
        for k in 0..n {
            v.playhead = Some(41.0 + k as f64 * 0.1);
            v.mark();
            b.draw(&mut v);
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("banc {name} (zoom {zoom}) : {ms:.2} ms par image");
    }
    b.shot(&mut v, "8_banc");
}

#[test]
#[ignore]
fn banc_par_couche() {
    let mut b = Bench::new(2123, 1310);
    let mut v = arranger();
    v.tracks[0].items = vec![long_item("L", 0.0, 283.0, 0, 69)];
    v.tracks[1].items = vec![long_item("M", 0.0, 283.0, 178, 69)];
    v.rec.clear();
    v.zoom = std::env::var("BANC_ZOOM")
        .ok()
        .and_then(|z| z.parse().ok())
        .unwrap_or(30.0);
    v.scroll = 40.0;
    for (name, l) in [
        ("rien", [false, false, false, false]),
        ("audio", [true, false, false, false]),
        ("midi", [false, true, false, false]),
        ("partition", [false, false, true, false]),
        ("tab", [false, false, false, true]),
    ] {
        for t in &mut v.tracks {
            t.head.lanes = Lanes(l);
        }
        b.draw(&mut v);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            v.mark();
            b.draw(&mut v);
        }
        let ms = t.elapsed().as_secs_f64() * 100.0;
        println!("banc {name} : {ms:.2} ms");
    }
}

#[test]
#[ignore]
fn banc_par_prise() {
    let mut b = Bench::new(2123, 1310);
    for (name, n) in [("pas transcrite", 0usize), ("transcrite", 178)] {
        let mut v = arranger();
        v.tracks[0].items = vec![long_item("L", 0.0, 283.0, n, 69)];
        v.tracks[1].items.clear();
        v.rec.clear();
        v.zoom = 30.0;
        v.scroll = 40.0;
        for (ln, l) in [
            ("partition", [false, false, true, false]),
            ("tab", [false, false, false, true]),
        ] {
            v.tracks[0].head.lanes = Lanes(l);
            v.tracks[1].head.lanes = Lanes([false, false, false, false]);
            b.draw(&mut v);
            let t = std::time::Instant::now();
            for _ in 0..10 {
                v.mark();
                b.draw(&mut v);
            }
            let ms = t.elapsed().as_secs_f64() * 100.0;
            println!("banc {name} {ln} : {ms:.2} ms");
        }
    }
}

/// La lecture ne redessine pas la scene : la tete est un trait par-dessus,
/// une image n'envoie que les colonnes qu'elle quitte et prend ; un survol
/// ne redessine que sa cible.
#[test]
fn la_lecture_ne_redessine_pas_la_scene() {
    use cp_gfx::layers::Layers;
    let mut b = Bench::new(2123, 1310);
    let mut v = arranger();
    v.tracks[0].items = vec![long_item("L", 0.0, 283.0, 0, 69)];
    v.rec.clear();
    v.zoom = 30.0;
    v.scroll = 40.0;
    let mut l = Layers::default();
    l.resize(2123, 1310);
    b.draw(&mut v);
    l.scene.px.copy_from_slice(&b.s.px);
    let whole = v.take_damage(2123, 1310);
    assert_eq!(whole.len(), 1);
    l.compose(&whole, &v.strokes(&b.th));
    l.drawn();
    let t = std::time::Instant::now();
    let mut sent = 0i64;
    for k in 0..60 {
        v.set_playhead(Some(42.0 + k as f64 * 0.05), true);
        assert!(!v.needs_draw(), "la tete de lecture salit la scene");
        let rects = l.compose(&v.take_damage(2123, 1310), &v.strokes(&b.th));
        sent += rects.iter().map(|r| r.w as i64 * r.h as i64).sum::<i64>();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / 60.0;
    println!(
        "une image de lecture : {ms:.3} ms, {} pixels envoyes",
        sent / 60
    );
    // Deux colonnes d'un pixel au plus par image (la scene fait 2,8 Mpx).
    assert!(sent / 60 < 4 * 1310, "{}", sent / 60);
    // Un survol : seulement ses deux cibles.
    let (x, y) = find(&v, |h| matches!(h, Hit::Transcribe { .. })).expect("la baguette");
    v.event(&Ev::Move {
        x,
        y,
        mods: Mods::default(),
    });
    let d = v.take_damage(2123, 1310);
    assert!(!d.is_empty());
    let area: i64 = d.iter().map(|r| r.w as i64 * r.h as i64).sum();
    assert!(area < 2123 * 1310 / 20, "le survol redessine {area} pixels");
}

/// Une prise comme dans l'arrangement : son bord se rogne (cale par le
/// magnetisme, a la mesure ici), sa poignee de fondu se tire, les touches au
/// survol (Q, D, S, A, E, Z chez Cedric) visent la prise sous la souris, et le
/// curseur est celui de REAPER pour chaque geste.
#[test]
fn rogner_fondre_et_editer_au_survol() {
    use cp_ui::{HoverEdit as H, Pointer as P};
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    v.snapper = Some(Box::new(|q: f64| (q / 4.0).round() * 4.0));
    b.draw(&mut v);
    let (q0, q1) = (v.tracks[0].items[0].qn0, v.tracks[0].items[0].qn1);
    // Le bord gauche de la couche audio.
    let audio = v
        .rect_of(Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Audio,
        })
        .expect("la couche audio");
    let (xa, ym) = (v.x_of(q0) + 1.0, audio.1 + audio.3 * 0.6);
    assert_eq!(
        v.hit(xa, ym),
        Hit::ItemEdge {
            t: 0,
            i: 0,
            left: true
        }
    );
    assert_eq!(v.pointer(xa, ym, Mods::default()), P::TrimLeft);
    // Tirer le bord d'une mesure et demie : il tombe sur la mesure.
    v.event(&Ev::Down {
        x: xa,
        y: ym,
        b: Button::Left,
        dbl: false,
        mods: Mods::default(),
    });
    let x2 = xa + v.lay.ppq * 5.0;
    v.event(&Ev::Move {
        x: x2,
        y: ym,
        mods: Mods::default(),
    });
    assert_eq!(v.pointer(x2, ym, Mods::default()), P::TrimLeft);
    b.shot(&mut v, "9_rognage");
    let a = v.event(&Ev::Up {
        x: x2,
        y: ym,
        b: Button::Left,
        mods: Mods::default(),
    });
    assert_eq!(
        a,
        Some(Act::TrimItem {
            t: 0,
            i: 0,
            qn0: q0 + 4.0,
            qn1: q1
        })
    );
    // La poignee du fondu d'entree : en haut, au debut (fondu nul).
    let yt = audio.1 + 4.0;
    assert_eq!(
        v.hit(v.x_of(q0) + 2.0, yt),
        Hit::ItemFade {
            t: 0,
            i: 0,
            fade_in: true
        }
    );
    // Au corps de la prise : la fleche au repos (Ctrl : la copie), deplacer
    // des que la prise bouge, pas a l'appui.
    let xm = v.x_of((q0 + q1) * 0.5);
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(v.pointer(xm, ym, Mods::default()), P::Arrow);
    assert_eq!(v.pointer(xm, ym, ctrl), P::ItemCopy);
    v.event(&Ev::Down {
        x: xm,
        y: ym,
        b: Button::Left,
        dbl: false,
        mods: Mods::default(),
    });
    assert_eq!(v.pointer(xm, ym, Mods::default()), P::Arrow);
    v.event(&Ev::Move {
        x: xm + 30.0,
        y: ym,
        mods: Mods::default(),
    });
    assert_eq!(v.pointer(xm + 30.0, ym, Mods::default()), P::ItemMove);
    v.event(&Ev::Up {
        x: xm,
        y: ym,
        b: Button::Left,
        mods: Mods::default(),
    });
    // Les touches au survol, la souris au milieu de la prise.
    v.event(&Ev::Move {
        x: xm,
        y: ym,
        mods: Mods::default(),
    });
    let q = v.qn_of(xm);
    let qs = (q / 4.0).round() * 4.0;
    assert_eq!(
        v.hover_edit(H::Split),
        Some(Act::SplitItem { t: 0, i: 0, qn: qs })
    );
    assert_eq!(
        v.hover_edit(H::TrimRight),
        Some(Act::TrimItem {
            t: 0,
            i: 0,
            qn0: q0,
            qn1: qs
        })
    );
    assert_eq!(
        v.hover_edit(H::FadeIn),
        Some(Act::FadeItem {
            t: 0,
            i: 0,
            fade_in: true,
            len: qs - q0
        })
    );
    assert_eq!(
        v.hover_edit(H::Delete),
        Some(Act::DeleteItem { t: 0, i: 0 })
    );
    // Ailleurs que sur une prise : rien.
    v.event(&Ev::Move {
        x: 5.0,
        y: 5.0,
        mods: Mods::default(),
    });
    assert_eq!(v.hover_edit(H::Split), None);
    // Les noms des scripts de Cedric.
    assert_eq!(
        H::of_name("Script: nvk_FOLDER_ITEMS - Trim left edge.lua"),
        Some(H::TrimLeft)
    );
    assert_eq!(
        H::of_name("nvk_FOLDER_ITEMS - Trim right edge.lua"),
        Some(H::TrimRight)
    );
    assert_eq!(H::of_name("nvk_FOLDER_ITEMS - Split.lua"), Some(H::Split));
    assert_eq!(H::of_name("CP_DeleteUnderMouse.lua"), Some(H::Delete));
    assert_eq!(
        H::of_name("LKC - HOVER EDIT D - Fadein.lua"),
        Some(H::FadeIn)
    );
    assert_eq!(
        H::of_name("LKC - HOVER EDIT G - Fadeout.lua"),
        Some(H::FadeOut)
    );
    assert_eq!(H::of_name("Item: Split items at time selection"), None);
    // Chez lui, la virgule efface un point d'enveloppe : pas une prise.
    assert_eq!(
        H::of_name("Custom: me2beats_Delete envelope at mouse.lua"),
        None
    );
}

/// Une vue jamais dessinee (ouverture, rechargement du module) redessine
/// toute sa fenetre : sa taille vient de la fenetre, pas du dernier dessin
/// (0 x 0 : la vue restait noire, 2026-11-22).
#[test]
fn une_vue_neuve_se_dessine_entiere() {
    let mut v = arranger();
    assert_eq!(
        v.take_damage(800, 600),
        vec![cp_gfx::IRect::new(0, 0, 800, 600)]
    );
}

/// Le clic pose le curseur la ou la ligne a la souris le montre : calee par
/// le magnetisme si la ligne l'est, libre avec Maj (ses reglages chez
/// Cedric : guidelines2 = 0x71).
#[test]
fn le_clic_suit_la_ligne_a_la_souris() {
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    v.snapper = Some(Box::new(|q: f64| (q / 4.0).round() * 4.0));
    v.guide = cp_ui::Guide::from_reaper(0x71, None);
    b.draw(&mut v);
    let q0 = v.tracks[0].items[0].qn0;
    let audio = v
        .rect_of(Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Audio,
        })
        .expect("la couche audio");
    let x = v.x_of(q0 + 2.6);
    let y = audio.1 + audio.3 * 0.6;
    for (m, want) in [
        (Mods::default(), ((q0 + 2.6) / 4.0).round() * 4.0),
        (
            Mods {
                shift: true,
                ..Mods::default()
            },
            v.qn_of(x),
        ),
    ] {
        v.event(&Ev::Move { x, y, mods: m });
        assert_eq!(v.guide_q, Some(want));
        v.event(&Ev::Down {
            x,
            y,
            b: Button::Left,
            dbl: false,
            mods: m,
        });
        let a = v.event(&Ev::Up {
            x,
            y,
            b: Button::Left,
            mods: m,
        });
        match a {
            Some(Act::ClickItem { q, .. }) => assert_eq!(q, want),
            other => panic!("{other:?}"),
        }
    }
}

/// Les boutons des couches dans l'en-tete, et une guitare glissee par son
/// en-tete a la place d'une autre (comme les pistes de REAPER).
#[test]
fn couches_et_ordre_des_guitares() {
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    b.draw(&mut v);
    // Les quatre boutons de la premiere guitare.
    for lane in Lane::ALL {
        assert!(
            v.rect_of(Hit::LaneToggle { t: 0, lane }).is_some(),
            "{lane:?}"
        );
    }
    let was = v.tracks[0].head.lanes.on(Lane::Midi);
    let (x, y) = center(
        v.rect_of(Hit::LaneToggle {
            t: 0,
            lane: Lane::Midi,
        })
        .unwrap(),
    );
    assert_eq!(click(&mut v, x, y), Some(Act::Lanes { t: 0 }));
    assert_eq!(v.tracks[0].head.lanes.on(Lane::Midi), !was);
    b.shot(&mut v, "11_couches");
    // Glisser l'en-tete de la premiere guitare sur la seconde.
    b.draw(&mut v);
    let h0 = v.rect_of(Hit::Header(0)).unwrap();
    let h1 = v.rect_of(Hit::Header(1)).unwrap();
    let (x0, y0) = (h0.0 + 20.0, h0.1 + 6.0);
    let y1 = h1.1 + h1.3 * 0.5;
    v.event(&Ev::Down {
        x: x0,
        y: y0,
        b: Button::Left,
        dbl: false,
        mods: Mods::default(),
    });
    v.event(&Ev::Move {
        x: x0,
        y: y1,
        mods: Mods::default(),
    });
    assert_eq!(v.track_at(y1), Some(1));
    assert_eq!(
        v.pointer(x0, y1, Mods::default()),
        cp_ui::Pointer::TrackDrag
    );
    b.shot(&mut v, "12_glisser_guitare");
    let a = v.event(&Ev::Up {
        x: x0,
        y: y1,
        b: Button::Left,
        mods: Mods::default(),
    });
    assert_eq!(a, Some(Act::MoveTrack { t: 0, to: 1 }));
    // Un simple clic sur l'en-tete : rien ne bouge.
    assert_eq!(click(&mut v, x0, y0), None);
}

/// Le magnetisme suit la grille de REAPER au zoom de CETTE vue : une
/// double croche reste une double croche tant que ses lignes sont a 8 px ou
/// plus ; en dessous, REAPER double le pas. (SnapToGrid prenait le zoom de
/// l'arrangement de REAPER, cache : toutes les deux mesures.)
#[test]
fn le_magnetisme_suit_la_grille_a_notre_zoom() {
    let g = SnapGrid {
        on: true,
        div_qn: 0.25,
        min_px: 8.0,
    };
    assert_eq!(g.step(40.0), 0.25);
    assert_eq!(g.step(20.0), 0.5);
    assert_eq!(g.step(6.0), 2.0);
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    v.zoom = 64.0;
    v.grid = Some(g);
    v.guide = cp_ui::Guide::from_reaper(0x71, None);
    b.draw(&mut v);
    let x = v.x_of(40.0 + 0.3);
    assert!((v.cursor_q(x, Mods::default()) - 40.25).abs() < 1e-9);
    // Magnetisme coupe dans REAPER : la ou est la souris.
    v.grid = Some(SnapGrid { on: false, ..g });
    assert!((v.cursor_q(x, Mods::default()) - v.qn_of(x)).abs() < 1e-9);
}

/// Une image faite de zones redessinees et de traits deplaces est celle
/// qu'aurait donnee un dessin entier : la souris qui passe sur une selection
/// de temps, avec sa ligne translucide, ne laisse aucune trainee. Deux
/// causes, le 2026-11-22 : un decoupage retire une fois de trop (la suite de
/// l'image sortait de la zone redessinee) et un trait translucide pose deux
/// fois la ou deux zones se recouvraient.
#[test]
fn le_survol_ne_laisse_pas_de_trainee() {
    use cp_gfx::layers::Layers;
    let (w, h) = (1600, 900);
    let mut b = Bench::new(w, h);
    let mut v = arranger();
    v.rec.clear();
    v.range = Some((4.0, 14.0));
    v.guide = cp_ui::Guide::from_reaper(0x71, Some(cp_gfx::rgb(255, 255, 255).with_alpha(120)));
    v.snapper = Some(Box::new(|q: f64| (q * 4.0).round() / 4.0));
    v.edit_cursor = Some(6.0);
    // Comme `Frame::paint` : la vue entiere, coupee a chaque zone.
    let paint = |b: &mut Bench, v: &mut Arr, l: &mut Layers, d: &[cp_gfx::IRect]| {
        for r in d {
            let mut cv = Canvas::new(&mut l.scene, &mut b.k);
            if cv.push_clip(r.x as f32, r.y as f32, r.w as f32, r.h as f32) {
                v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th, w as f32, h as f32);
            }
        }
    };
    let mut l = Layers::default();
    l.resize(w, h);
    let d = v.take_damage(w, h);
    paint(&mut b, &mut v, &mut l, &d);
    l.drawn();
    l.compose(&d, &v.strokes(&b.th));
    let audio = v
        .rect_of(Hit::Item {
            t: 0,
            i: 0,
            lane: Lane::Audio,
        })
        .expect("la couche audio");
    let mut bad = Vec::new();
    // La souris traverse la vue en diagonale, sur la selection et hors d'elle,
    // et repasse sur la prise.
    for k in 0..240 {
        let x = 40.0 + k as f32 * 6.3;
        let y = if k % 3 == 0 {
            audio.1 + audio.3 * 0.5
        } else {
            20.0 + (k as f32 * 3.7) % 860.0
        };
        v.event(&Ev::Move {
            x,
            y,
            mods: Mods::default(),
        });
        let d = v.take_damage(w, h);
        paint(&mut b, &mut v, &mut l, &d);
        let st = v.strokes(&b.th);
        l.compose(&d, &st);
        let mut r = Layers::default();
        r.resize(w, h);
        v.mark();
        let full = v.take_damage(w, h);
        paint(&mut b, &mut v, &mut r, &full);
        r.compose(&full, &st);
        let n = (0..l.out.px.len())
            .filter(|&i| l.out.px[i] != r.out.px[i])
            .count();
        if n > 0 {
            bad.push(format!(
                "pas {k} (souris {x:.0},{y:.0}, survol {:?}) : {n} px",
                v.hover
            ));
            // Repartir de l'image juste : chaque faute comptee une fois.
            l.out.px.copy_from_slice(&r.out.px);
            l.scene.px.copy_from_slice(&r.scene.px);
        }
    }
    assert!(
        bad.is_empty(),
        "{} images fausses :\n{}",
        bad.len(),
        bad[..bad.len().min(12)].join("\n")
    );
}

/// Le clic droit glisse : le lasso de l'arrangement. Il choisit les items
/// qu'il touche (la couche audio : l'item audio ; le MIDI, la partition, la
/// tab : l'item MIDI), Maj en plus, Ctrl en inversant ; un clic droit sans
/// bouger ne change rien.
#[test]
fn le_lasso_choisit_les_prises() {
    use cp_ui::Pointer as P;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    v.tracks[0].items[0].midi = true;
    b.draw(&mut v);
    let rect = |v: &Arr, lane| {
        v.rect_of(Hit::Item { t: 0, i: 0, lane })
            .expect("la couche")
    };
    let right = |v: &mut Arr, a: (f32, f32), z: (f32, f32), mods: Mods| {
        v.event(&Ev::Down {
            x: a.0,
            y: a.1,
            b: Button::Right,
            dbl: false,
            mods,
        });
        v.event(&Ev::Move {
            x: z.0,
            y: z.1,
            mods,
        });
        v.event(&Ev::Up {
            x: z.0,
            y: z.1,
            b: Button::Right,
            mods,
        })
    };
    let au = rect(&v, Lane::Audio);
    let x0 = v.x_of(v.tracks[0].items[0].qn0) + 6.0;
    // Dans la couche audio seulement.
    let a = right(
        &mut v,
        (x0, au.1 + 3.0),
        (x0 + 50.0, au.1 + au.3 - 3.0),
        Mods::default(),
    );
    assert_eq!(
        a,
        Some(Act::SelectItems {
            items: vec![(0, 0, true, false)],
            pick: Pick::Replace
        })
    );
    assert_eq!(v.tracks[0].items[0].picked, (true, false));
    // Maj : la tab en plus (l'item MIDI).
    let tab = rect(&v, Lane::Tab);
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let a = right(&mut v, (x0, tab.1 + 3.0), (x0 + 50.0, tab.1 + 10.0), shift);
    assert_eq!(
        a,
        Some(Act::SelectItems {
            items: vec![(0, 0, false, true)],
            pick: Pick::Add
        })
    );
    assert_eq!(v.tracks[0].items[0].picked, (true, true));
    // Ctrl : l'audio s'eteint, le MIDI reste.
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    v.event(&Ev::Down {
        x: x0,
        y: au.1 + 3.0,
        b: Button::Right,
        dbl: false,
        mods: ctrl,
    });
    v.event(&Ev::Move {
        x: x0 + 60.0,
        y: au.1 + 20.0,
        mods: ctrl,
    });
    assert_eq!(v.pointer(x0 + 60.0, au.1 + 20.0, ctrl), P::Marquee);
    b.shot(&mut v, "10_lasso");
    v.event(&Ev::Up {
        x: x0 + 60.0,
        y: au.1 + 20.0,
        b: Button::Right,
        mods: ctrl,
    });
    assert_eq!(v.tracks[0].items[0].picked, (false, true));
    // Un clic droit sans bouger : rien.
    let a = right(
        &mut v,
        (x0, au.1 + 3.0),
        (x0 + 1.0, au.1 + 3.0),
        Mods::default(),
    );
    assert_eq!(a, None);
    assert_eq!(v.tracks[0].items[0].picked, (false, true));
    // Un clic sur la prise la choisit seule, sa couche allumee.
    v.event(&Ev::Down {
        x: x0 + 20.0,
        y: au.1 + au.3 * 0.6,
        b: Button::Left,
        dbl: false,
        mods: Mods::default(),
    });
    v.event(&Ev::Up {
        x: x0 + 20.0,
        y: au.1 + au.3 * 0.6,
        b: Button::Left,
        mods: Mods::default(),
    });
    assert_eq!(v.tracks[0].items[0].picked, (true, false));
}

/// Solo comme dans REAPER : seul, Alt (sans le routage), Ctrl (toutes les
/// pistes), Ctrl + Alt (exclusif).
#[test]
fn solo_et_ses_modificateurs() {
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    b.draw(&mut v);
    let (x, y) = find(&v, |h| matches!(h, Hit::Solo(0))).expect("le solo");
    for (ctrl, alt, how) in [
        (false, false, SoloHow::Toggle),
        (false, true, SoloHow::IgnoreRouting),
        (true, false, SoloHow::All),
        (true, true, SoloHow::Exclusive),
    ] {
        let mods = Mods {
            ctrl,
            alt,
            ..Mods::default()
        };
        let a = v.event(&Ev::Down {
            x,
            y,
            b: Button::Left,
            dbl: false,
            mods,
        });
        v.event(&Ev::Up {
            x,
            y,
            b: Button::Left,
            mods,
        });
        assert_eq!(a, Some(Act::Solo { t: 0, how }));
    }
}

/// Les bends et les glisses se dessinent : la fleche du bend et sa valeur,
/// le relachement, le trait d'une glisse, celui d'une approche.
#[test]
fn bends_et_glisses_se_dessinent() {
    use cp_tab::doc::{TNote, Tech};
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let note = |q: f64, len: f64, fret: i32| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch: 55 + fret,
        string: 3,
        fret,
        vel: 96,
        ..Default::default()
    };
    let mut d = vec![
        // Une approche d'en dessous.
        TNote {
            bend: vec![(0.0, -2.0), (0.1, 0.0)],
            ..note(0.0, 1.0, 7)
        },
        // Un bend d'un ton, relache.
        TNote {
            bend: vec![(0.0, 0.0), (0.3, 2.0), (1.2, 2.0), (1.6, 0.0)],
            ..note(1.0, 2.0, 7)
        },
        note(3.0, 1.0, 5),
        // Une glisse de 5 a 7.
        TNote {
            tech: Tech::Slide,
            ..note(4.0, 1.0, 7)
        },
    ];
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    b.shot(&mut v, "11_bends");
    assert_eq!(
        crate::play_text(&v.tracks[0].items[0].doc[1]),
        " \u{00b7} bend full and release"
    );
    assert_eq!(crate::bend_label(1.0), "\u{00bd}");
    assert_eq!(crate::bend_label(3.0), "1\u{00bd}");
    assert_eq!(crate::bend_label(0.5), "\u{00bc}");
}

/// Le clic droit sur une note ouvre sa palette : un bend, son relachement,
/// une glisse depuis la note d'avant, une approche s'y posent en un clic ;
/// Echap la ferme. Alt + clic efface une note ; une note glissee
/// verticalement dans la tablature change de corde.
#[test]
fn la_palette_d_une_note_et_ses_gestes() {
    use cp_tab::doc::{TNote, Tech};
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let note = |q: f64, string: usize, fret: i32| {
        let open = [40, 45, 50, 55, 59, 64][string];
        TNote {
            qn0: q0 + q,
            qn1: q0 + q + 1.0,
            pitch: open + fret,
            string,
            fret,
            vel: 96,
            ..Default::default()
        }
    };
    let d = vec![note(0.0, 3, 5), note(1.0, 3, 7), note(3.0, 4, 3)];
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    b.draw(&mut v);
    // La case d'une note dans la tablature (la plus basse de ses cibles).
    let tab_box = |v: &Arr, n: usize| {
        v.boxes
            .iter()
            .filter(|x| x.4 == Hit::Note { t: 0, i: 0, n })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|x| (x.0 + x.2 * 0.5, x.1 + x.3 * 0.5))
            .expect("la note dans la tab")
    };
    let none = Mods::default();
    let click = |v: &mut Arr, (x, y): (f32, f32), b: Button, mods: Mods| {
        let a = v.event(&Ev::Down {
            x,
            y,
            b,
            dbl: false,
            mods,
        });
        let u = v.event(&Ev::Up { x, y, b, mods });
        a.or(u)
    };
    let pick = |v: &mut Arr, b: &mut Bench, n: usize, m: Mark| {
        let p = tab_box(v, n);
        click(v, p, Button::Right, Mods::default());
        assert!(v.palette.is_some(), "la palette ne s'ouvre pas");
        b.draw(v);
        let r = v.rect_of(Hit::Mark(m)).expect("la marque");
        let a = click(
            v,
            (r.0 + r.2 * 0.5, r.1 + r.3 * 0.5),
            Button::Left,
            Mods::default(),
        );
        assert!(v.palette.is_none());
        b.draw(v);
        a
    };
    assert_eq!(
        pick(&mut v, &mut b, 1, Mark::Full),
        Some(Act::Edited { t: 0, i: 0 })
    );
    let n1 = || 1usize;
    assert_eq!(
        v.tracks[0].items[0].doc[n1()].bend_mark().map(|x| x.amount),
        Some(2.0)
    );
    pick(&mut v, &mut b, 1, Mark::Release);
    assert!(
        v.tracks[0].items[0].doc[1]
            .bend_mark()
            .unwrap()
            .release
            .is_some()
    );
    pick(&mut v, &mut b, 1, Mark::Slide);
    assert_eq!(v.tracks[0].items[0].doc[1].tech, Tech::Slide);
    pick(&mut v, &mut b, 1, Mark::InBelow);
    let nb = &v.tracks[0].items[0].doc[1];
    assert_eq!(nb.slide_in(), -3);
    assert!(nb.bend_mark().is_some(), "le bend reste : {:?}", nb.bend);
    // L'approche se pince : la glisse depuis la note d'avant tombe.
    assert_eq!(nb.tech, Tech::Plain);
    assert!(nb.locked);
    // La palette, dessinee (planche), puis Echap la ferme.
    let p1 = tab_box(&v, 1);
    click(&mut v, p1, Button::Right, none);
    b.shot(&mut v, "12_palette");
    v.event(&Ev::Key {
        vk: vk::ESCAPE,
        mods: none,
        text: None,
    });
    assert!(v.palette.is_none());
    // La note d'apres (si sur la corde de Si, case 3) glissee sur la corde de
    // Sol : case 7, meme hauteur.
    b.draw(&mut v);
    let (x, y) = tab_box(&v, 2);
    let gy = v
        .boxes
        .iter()
        .find(|x| x.4 == Hit::TabLine { t: 0, i: 0, s: 3 })
        .map(|x| x.1 + x.3 * 0.5)
        .expect("la corde de sol");
    v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: none,
    });
    v.event(&Ev::Move {
        x,
        y: gy,
        mods: none,
    });
    v.event(&Ev::Up {
        x,
        y: gy,
        b: Button::Left,
        mods: none,
    });
    let c = &v.tracks[0].items[0].doc[2];
    assert_eq!((c.string, c.fret, c.pitch), (3, 7, 62));
    // Alt + clic : la premiere note s'efface.
    b.draw(&mut v);
    let p = tab_box(&v, 0);
    let a = click(
        &mut v,
        p,
        Button::Left,
        Mods {
            alt: true,
            ..Mods::default()
        },
    );
    assert_eq!(a, Some(Act::Edited { t: 0, i: 0 }));
    assert_eq!(v.tracks[0].items[0].doc.len(), 2);
}

/// Une surface reduite de moitie (pour regarder une page entiere).
fn half(s: &Surface) -> Surface {
    let mut o = Surface::new(s.w / 2, s.h / 2);
    for y in 0..o.h {
        for x in 0..o.w {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = s.get(2 * x + dx, 2 * y + dy);
                for (k, a) in acc.iter_mut().enumerate() {
                    *a += (p >> (8 * k)) & 0xFF;
                }
            }
            let mut v = 0u32;
            for (k, a) in acc.iter().enumerate() {
                v |= (a / 4) << (8 * k);
            }
            o.px[(y * o.w + x) as usize] = v;
        }
    }
    o
}

/// La tablature en pages : la partition sur la tab, des systemes de
/// quelques mesures (moins quand les notes se serrent), les bends et les
/// glisses comme dans la vue, plusieurs pages numerotees ; puis le PDF.
#[test]
fn la_tablature_s_imprime() {
    use cp_tab::doc::Tech;
    const EM: &[(usize, i32)] = &[(6, 0), (5, 2), (4, 2), (3, 0), (2, 0), (1, 0)];
    const C: &[(usize, i32)] = &[(5, 3), (4, 2), (3, 0), (2, 1), (1, 0)];
    const G: &[(usize, i32)] = &[(6, 3), (5, 2), (4, 0), (3, 0), (2, 0), (1, 3)];
    const RUN: [&[(usize, i32)]; 8] = [
        &[(1, 0)],
        &[(1, 3)],
        &[(1, 5)],
        &[(2, 3)],
        &[(2, 5)],
        &[(2, 6)],
        &[(3, 5)],
        &[(3, 7)],
    ];
    const MEL: [&[(usize, i32)]; 4] = [&[(3, 7)], &[(2, 8)], &[(2, 10)], &[(1, 12)]];
    let bars = 26;
    let mut raw: Vec<Raw> = Vec::new();
    for m in 0..bars {
        match m % 4 {
            0 => {
                for (t, ch) in [(0.0, EM), (4.0, C), (8.0, G), (12.0, EM)] {
                    raw.push((m, t, 4.0, ch, ""));
                }
            }
            1 => {
                for k in 0..8 {
                    raw.push((m, k as f64 * 2.0, 2.0, MEL[k % 4], ""));
                }
            }
            2 => {
                for k in 0..16 {
                    let mark = if k % 4 == 1 { "L" } else { "" };
                    raw.push((m, k as f64, 1.0, RUN[k % 8], mark));
                }
            }
            _ => {
                // Une blanche, un soupir, une noire.
                raw.push((m, 0.0, 8.0, &[(2, 8)], ""));
                raw.push((m, 12.0, 4.0, &[(2, 10)], ""));
            }
        }
    }
    let mut it = item("P", "Riff.wav", 0, bars, &raw);
    // Des bends (relache ou non), une approche, une glisse.
    let mut k = 0;
    for n in 0..it.doc.len() {
        let q = it.doc[n].qn0;
        let m = (q / 4.0) as usize;
        if m % 4 == 3 && (q % 4.0) < 0.5 {
            it.doc[n].set_bend(Some(if k % 2 == 0 { 2.0 } else { 1.0 }), k % 2 == 0);
            k += 1;
        }
        if m % 4 == 1 && (q % 4.0) < 0.2 {
            it.doc[n].set_slide_in(-2);
        }
        if m % 4 == 1 && (q % 4.0 - 1.0).abs() < 0.2 {
            it.doc[n].tech = Tech::Slide;
        }
    }
    it.touch();
    let mut b = Bench::new(10, 10);
    let lanes = Lanes([true, false, true, true]);
    let mut n = 0;
    let (measures, systems) = print::render(&it, lanes, &mut b.t, &mut |s| {
        n += 1;
        if n <= 2 {
            let p = std::env::temp_dir().join(format!("cp_tabview_13_print_p{n}.png"));
            std::fs::write(&p, png::encode(&half(s))).unwrap();
        }
        if n == 1 {
            // Le premier systeme a pleine taille.
            let mut top = Surface::new(s.w, 1100);
            top.px.copy_from_slice(&s.px[..(s.w * 1100) as usize]);
            let p = std::env::temp_dir().join("cp_tabview_13_print_zoom.png");
            std::fs::write(&p, png::encode(&top)).unwrap();
        }
    });
    assert_eq!(measures, bars as i64);
    assert!(n >= 2, "{n} page(s)");
    // Les doubles croches demandent plus de place que les accords.
    let eng = it.engraved().clone();
    let need = print::need_ppq(&eng);
    assert!(need[2] > need[0] * 2.0, "{need:?}");
    let (first_w, w) = print::avail(0.0);
    let bq = vec![4.0; need.len()];
    let room = vec![0.0; need.len()];
    let sys = print::systems(&need, &bq, &room, first_w, w);
    assert_eq!(sys.len(), systems);
    assert_eq!(sys.last().map(|s| s.m1), Some(bars as i64));
    assert!(sys.windows(2).all(|w| w[0].m1 == w[1].m0));
    let narrow = sys.iter().find(|s| (s.m0..s.m1).contains(&2)).unwrap();
    assert!(narrow.m1 - narrow.m0 < 6, "{sys:?}");
    // Chaque mesure a sa largeur : les doubles croches plus larges que les
    // accords du meme systeme.
    let first = &sys[0];
    assert!(first.m1 - first.m0 >= 3, "{sys:?}");
    assert!(first.widths[2] > first.widths[0] * 1.5, "{sys:?}");
    assert!((first.width() - first_w).abs() < 1.0);
    // Le PDF : ses pages, son en-tete.
    let p = print::pdf(&it, lanes, &mut b.t);
    assert_eq!(p.pages, n);
    assert!(p.pdf.starts_with(b"%PDF-1.4"));
    assert!(p.pdf.len() < n * 600_000, "{} octets", p.pdf.len());
    std::fs::write(std::env::temp_dir().join("cp_tabview_13_print.pdf"), &p.pdf).unwrap();
}

/// La touche d'ecoute de Cedric (², SWS/BR « Play from mouse cursor position
/// and solo track under mouse ») vise la date sous la souris et la guitare
/// sous elle ; sur la regle, aucune guitare ; hors de la zone qui defile
/// (les en-tetes), rien : la touche reste a REAPER.
#[test]
fn l_ecoute_vise_la_souris() {
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    b.draw(&mut v);
    let l = v.lay;
    let x = l.sx + 200.0;
    let row = |t: usize| {
        (0..2000)
            .map(|k| k as f32)
            .find(|&y| v.track_at(y) == Some(t))
            .unwrap()
            + 8.0
    };
    let (q, t) = v.play_target(x, row(0)).unwrap();
    assert_eq!(t, Some(0));
    assert!((q - v.qn_of(x)).abs() < 1e-9);
    assert_eq!(v.play_target(x, row(1)).map(|p| p.1), Some(Some(1)));
    assert_eq!(v.play_target(x, l.top + 2.0).map(|p| p.1), Some(None));
    assert!(v.play_target(l.sx - 5.0, row(0)).is_none());
    assert!(v.play_target(x, l.bottom - 1.0).is_none());
}

/// Le survol ne redessine que ses zones : chaque cible survolee (les
/// en-tetes, l'inspecteur, les prises, la barre) doit donner, redessinee
/// par zones, la meme image qu'un dessin complet. Sinon le survol
/// n'apparait qu'au prochain dessin complet (« il faut attendre »).
#[test]
fn chaque_survol_se_redessine_tout_de_suite() {
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    let (w, h) = (1600, 900);
    b.draw(&mut v);
    let _ = v.take_damage(w, h);
    let mut seen: Vec<Hit> = Vec::new();
    let targets: Vec<(f32, f32, Hit)> = v
        .boxes
        .iter()
        .map(|bx| (bx.0 + bx.2 * 0.5, bx.1 + bx.3 * 0.5, bx.4))
        .collect();
    let mut bad: Vec<String> = Vec::new();
    for (x, y, _) in targets {
        let hit = v.hit(x, y);
        if seen.contains(&hit) {
            continue;
        }
        seen.push(hit);
        let prev = v.hover;
        v.event(&Ev::Move {
            x,
            y,
            mods: Mods::default(),
        });
        // Ce que fait la fenetre : chaque zone redessinee seule.
        for r in v.take_damage(w, h) {
            let mut cv = Canvas::new(&mut b.s, &mut b.k);
            if cv.push_clip(r.x as f32, r.y as f32, r.w as f32, r.h as f32) {
                v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th, w as f32, h as f32);
            }
        }
        let partial = b.s.px.clone();
        let mut full = Surface::new(w, h);
        {
            let mut cv = Canvas::new(&mut full, &mut b.k);
            v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th, w as f32, h as f32);
        }
        let _ = v.take_damage(w, h);
        let diff: Vec<usize> = (0..partial.len())
            .filter(|&k| partial[k] != full.px[k])
            .collect();
        if !diff.is_empty() {
            let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
            for &k in &diff {
                let (px, py) = (k as i32 % w, k as i32 / w);
                x0 = x0.min(px);
                y0 = y0.min(py);
                x1 = x1.max(px);
                y1 = y1.max(py);
            }
            bad.push(format!(
                "{prev:?} -> {hit:?} en ({x:.0},{y:.0}) : {} px de ({x0},{y0}) a ({x1},{y1}) ; {:08x} au lieu de {:08x}",
                diff.len(),
                partial[diff[0]],
                full.px[diff[0]]
            ));
            // Repartir d'une image juste.
            b.s.px.copy_from_slice(&full.px);
        }
    }
    assert!(
        bad.is_empty(),
        "{} survols en retard :\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// Une guitare entiere en une partition : ses items bout a bout (un
/// morceau fait de plusieurs prises), le nom de la piste ; un trou entre
/// deux items devient des mesures de silence ; un item sans tablature ne
/// compte pas.
#[test]
fn une_guitare_entiere_s_exporte() {
    let v = arranger();
    let tr = &v.tracks[0];
    let n: usize = tr.items.iter().map(|it| it.doc.len()).sum();
    let mut it = print::whole_track(tr).unwrap();
    assert_eq!(it.name, "Rhythm");
    assert_eq!(it.doc.len(), n);
    assert!((it.origin_qn - 32.0).abs() < 1e-9);
    // Les items A (mesures 9 et 10) et B (11 et 12) : quatre mesures.
    assert_eq!(it.engraved().measures, 4);
    // B deux mesures plus loin : deux mesures de silence entre eux.
    let mut tr2 = tr.clone();
    let b = &mut tr2.items[1];
    let moved: Vec<_> = b
        .doc
        .iter()
        .cloned()
        .map(|mut x| {
            x.qn0 += 8.0;
            x.qn1 += 8.0;
            x
        })
        .collect();
    b.qn0 += 8.0;
    b.qn1 += 8.0;
    b.set_doc(moved);
    // Dans le desordre : l'ordre du temps decide.
    tr2.items.swap(0, 1);
    tr2.items.push(Item::default());
    let mut it2 = print::whole_track(&tr2).unwrap();
    assert!((it2.origin_qn - 32.0).abs() < 1e-9);
    let eng = it2.engraved().clone();
    assert_eq!(eng.measures, 6);
    let empty: Vec<i64> = (0..eng.measures)
        .filter(|&m| {
            eng.signs
                .iter()
                .filter(|s| s.measure == m)
                .all(|s| eng.events[s.ev].notes.is_empty())
        })
        .collect();
    assert_eq!(empty, vec![2, 3]);
    // Le PDF de la guitare entiere.
    let mut bench = Bench::new(10, 10);
    let p = print::pdf(&it2, tr2.head.lanes, &mut bench.t);
    assert_eq!(p.measures, 6);
    assert!(p.pdf.starts_with(b"%PDF"));
    // Rien a exporter : aucun item transcrit.
    let mut none = tr.clone();
    for it in &mut none.items {
        it.set_doc(Vec::new());
    }
    assert!(print::whole_track(&none).is_none());
}

/// Un temps de triolets, un shuffle, des croches et des doubles croches :
/// la partition porte ses 3 (sur la ligature, ou avec un crochet).
#[test]
fn les_triolets_se_dessinent() {
    use cp_tab::doc::TNote;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let note = |q: f64, len: f64, fret: i32| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch: 55 + fret,
        string: 3,
        fret,
        vel: 96,
        ..Default::default()
    };
    let t = 1.0 / 3.0;
    let mut d = vec![
        note(0.0, t, 0),
        note(t, t, 2),
        note(2.0 * t, t, 4),
        note(1.0, 0.5, 5),
        note(1.5, 0.5, 7),
        note(2.0, 2.0 * t, 5),
        note(2.0 + 2.0 * t, t, 7),
        note(3.0, 0.25, 0),
        note(3.25, 0.25, 2),
        note(3.5, 0.25, 4),
        note(3.75, 0.25, 5),
        // Un triolet avec un silence au milieu : le crochet.
        note(4.0, t, 7),
        note(4.0 + 2.0 * t, t, 5),
    ];
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    b.shot(&mut v, "14_triolets");
    let eng = v.tracks[0].items[0].engraved().clone();
    assert_eq!(
        eng.triplets.iter().copied().collect::<Vec<_>>(),
        vec![0, 2, 4]
    );
}

/// Une basse tenue sous une melodie : deux voix, hampes en haut pour la
/// melodie, en bas pour la basse.
#[test]
fn deux_voix_se_dessinent() {
    use cp_tab::doc::TNote;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let open = [40, 45, 50, 55, 59, 64];
    let note = |q: f64, len: f64, string: usize, fret: i32| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch: open[string] + fret,
        string,
        fret,
        vel: 96,
        ..Default::default()
    };
    let mut d = vec![note(0.0, 2.0, 0, 0), note(2.0, 2.0, 1, 0)];
    for k in 0..8 {
        d.push(note(k as f64 * 0.5, 0.5, 3 + (k % 2), 2));
    }
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    b.shot(&mut v, "15_deux_voix");
    let eng = v.tracks[0].items[0].engraved().clone();
    assert!(eng.two_voices.contains(&0));
}

/// Une melodie en Si bemol majeur : l'armure (deux bemols) apres la cle,
/// les notes nommees en bemols, le Mi naturel avec son becarre ; et la meme
/// armure sur chaque ligne du PDF.
#[test]
fn l_armure_et_les_bemols() {
    use cp_tab::doc::TNote;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let note = |q: f64, len: f64, pitch: i32| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch,
        string: 3,
        fret: pitch - 55,
        vel: 96,
        ..Default::default()
    };
    // Si b, Do, Re, Mib, Fa, Sol, La, Si b ; puis l'arpege, puis un Mi
    // naturel et le Mi b qui revient.
    let mut d = Vec::new();
    let mut q = 0.0;
    for p in [58, 60, 62, 63, 65, 67, 69, 70] {
        d.push(note(q, 0.5, p));
        q += 0.5;
    }
    for p in [58, 62, 65, 70, 65, 58] {
        d.push(note(q, 1.0, p));
        q += 1.0;
    }
    d.push(note(q, 0.5, 64));
    d.push(note(q + 0.5, 0.5, 63));
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    v.tracks[0].items.truncate(1);
    b.shot(&mut v, "16_armure");
    let eng = v.tracks[0].items[0].engraved().clone();
    assert_eq!(eng.key.fifths, -2, "{:?}", eng.key);
    let it = v.tracks[0].items[0].clone();
    let p = print::pdf(&it, v.tracks[0].head.lanes, &mut b.t);
    assert!(p.pages >= 1);
}

/// Les techniques de jeu : P.M. sur un passage, une note morte, un vibrato
/// laisse sonner, une harmonique ; la palette pose le vibrato (et sa courbe,
/// pour l'entendre) puis l'enleve.
#[test]
fn les_techniques_de_jeu_se_dessinent() {
    use cp_tab::doc::TNote;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let open = [40, 45, 50, 55, 59, 64];
    let note = |q: f64, len: f64, string: usize, fret: i32, marks: u8| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch: open[string] + fret,
        string,
        fret,
        vel: 96,
        marks,
        ..Default::default()
    };
    let mut d = vec![
        note(0.0, 0.5, 0, 3, TNote::PALM),
        note(0.5, 0.5, 0, 3, TNote::PALM),
        note(1.0, 0.5, 0, 3, TNote::PALM | TNote::DEAD),
        note(1.5, 0.5, 0, 5, TNote::PALM),
        note(2.0, 2.0, 4, 8, TNote::VIBRATO | TNote::RING),
        note(4.0, 1.0, 3, 12, TNote::HARMONIC),
        note(5.0, 1.0, 2, 7, 0),
    ];
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    b.shot(&mut v, "17_techniques");
    // La palette : le vibrato sur la derniere note, entendu (une courbe),
    // puis enleve avec elle.
    let n = v.tracks[0].items[0].doc.len() - 1;
    assert!(v.apply_mark(0, 0, n, Mark::Vibrato).is_some());
    let nt = &v.tracks[0].items[0].doc[n];
    assert!(nt.has(TNote::VIBRATO) && nt.bend.len() > 4);
    assert!(nt.bend_mark().is_none(), "un vibrato n'est pas un bend");
    assert!(v.mark_on(0, 0, n, Mark::Vibrato));
    v.apply_mark(0, 0, n, Mark::Vibrato);
    let nt = &v.tracks[0].items[0].doc[n];
    assert!(!nt.has(TNote::VIBRATO) && nt.bend.is_empty());
    // Morte et harmonique s'excluent.
    v.apply_mark(0, 0, n, Mark::Dead);
    v.apply_mark(0, 0, n, Mark::Harmonic);
    let nt = &v.tracks[0].items[0].doc[n];
    assert!(nt.has(TNote::HARMONIC) && !nt.has(TNote::DEAD));
    // La palette tient ses 14 icones.
    assert_eq!(Mark::ALL.len(), 14);
}

/// La partition cachee : la tablature porte son rythme dessous (hampes,
/// ligatures, silences, triolets), dans la fenetre et dans le PDF.
#[test]
fn la_tab_seule_porte_son_rythme() {
    use cp_tab::doc::TNote;
    let mut b = Bench::new(1600, 900);
    let mut v = arranger();
    v.rec.clear();
    let q0 = v.tracks[0].items[0].qn0;
    let note = |q: f64, len: f64, fret: i32| TNote {
        qn0: q0 + q,
        qn1: q0 + q + len,
        pitch: 55 + fret,
        string: 3,
        fret,
        vel: 96,
        ..Default::default()
    };
    let t = 1.0 / 3.0;
    let mut d = vec![
        note(0.0, 1.0, 5),
        note(1.0, 0.5, 7),
        note(1.5, 0.5, 5),
        note(2.0, 0.25, 7),
        note(2.25, 0.25, 9),
        note(2.5, 0.25, 7),
        note(2.75, 0.25, 5),
        note(3.0, t, 7),
        note(3.0 + t, t, 9),
        note(3.0 + 2.0 * t, t, 10),
        note(5.0, 1.5, 12),
    ];
    cp_tab::doc::sort(&mut d);
    v.tracks[0].items[0].set_doc(d);
    v.tracks[0].items[0].ghosts.clear();
    let h0 = draw::track_h(&v.tracks[0], 1.0, 1.0);
    v.tracks[0].head.lanes.0[Lane::Score as usize] = false;
    let h1 = draw::track_h(&v.tracks[0], 1.0, 1.0);
    // Sans la partition (118 px) mais avec le rythme (26 px).
    assert_eq!(
        h0 - h1,
        (draw::lane_h(Lane::Score) + 1.0 - draw::RHYTHM_H).round()
    );
    b.shot(&mut v, "18_rythme_tab");
    let it = v.tracks[0].items[0].clone();
    let p = print::pdf(&it, v.tracks[0].head.lanes, &mut b.t);
    assert!(p.pages >= 1);
}

#[test]
fn le_chiffrage_qui_change_se_dessine_et_s_imprime() {
    // 4/4 deux mesures, 3/4 deux, 6/8 deux, puis 4/4 : des croches d'un
    // bout a l'autre.
    const MEL: [&[(usize, i32)]; 4] = [&[(3, 7)], &[(2, 8)], &[(2, 10)], &[(1, 12)]];
    let mut raw: Vec<Raw> = Vec::new();
    for m in 0..6 {
        for k in 0..8 {
            raw.push((m, k as f64 * 2.0, 2.0, MEL[k % 4], ""));
        }
    }
    let meter = cp_tab::meter::Meter::new(vec![
        (0.0, (4, 4)),
        (8.0, (3, 4)),
        (14.0, (6, 8)),
        (20.0, (4, 4)),
    ]);
    let mut it = item("M", "Meter.wav", 0, 6, &raw);
    it.meter = Some(meter.clone());
    it.touch();
    let e = it.engraved().clone();
    assert_eq!(e.bars, vec![0, 16, 32, 44, 56, 68, 80, 96]);
    for m in 0..e.measures {
        let sum: i64 = e
            .signs
            .iter()
            .filter(|s| s.measure == m && s.voice == 1)
            .map(|s| s.dur)
            .sum();
        assert_eq!(sum, e.bar_len(m), "mesure {m}");
    }
    // L'ecran : les barres du projet, numerotees dans l'ordre.
    let mut v = arranger();
    v.meter = meter;
    v.tracks[0].items = vec![it.clone()];
    v.scroll = 0.0;
    let mut b = Bench::new(1600, 760);
    b.shot(&mut v, "14_chiffrages");
    let bars: Vec<(i64, f64)> = v.view_bars().iter().map(|b| (b.index, b.qn)).collect();
    assert!(bars.len() >= 6, "{bars:?}");
    assert_eq!(
        &bars[..6],
        &[
            (0, 0.0),
            (1, 4.0),
            (2, 8.0),
            (3, 11.0),
            (4, 14.0),
            (5, 17.0)
        ]
    );
    // Le papier : le nouveau chiffrage au debut de sa mesure.
    let lanes = Lanes([true, false, true, true]);
    let mut n = 0;
    let (measures, _) = print::render(&it, lanes, &mut b.t, &mut |s| {
        n += 1;
        if n == 1 {
            let mut top = Surface::new(s.w, 1300);
            top.px.copy_from_slice(&s.px[..(s.w * 1300) as usize]);
            let p = std::env::temp_dir().join("cp_tabview_14_print_chiffrages.png");
            std::fs::write(&p, png::encode(&top)).unwrap();
        }
    });
    assert_eq!(measures, 7);
}
