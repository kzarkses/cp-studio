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
    // Alt + molette : dans le temps ; la molette seule : verticalement.
    let s0 = v.scroll;
    wheel(&mut v, 0.0, -1.0, alt);
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
    // Un clic : le curseur.
    assert!(matches!(click(&mut v, x, y), Some(Act::Seek(_))));
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
