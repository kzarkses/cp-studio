//! La fenetre pilotee hors de REAPER : un vrai riff doigte par le moteur, les
//! vraies polices (GDI), des gestes, et des planches PNG dans `%TEMP%`
//! (cp_tabview_*.png).

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

    fn draw(&mut self, v: &mut TabView) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(&mut cv, &mut self.t, &mut self.icons, &self.th, w, h);
    }

    fn shot(&mut self, v: &mut TabView, name: &str) {
        self.draw(v);
        let p = std::env::temp_dir().join(format!("cp_tabview_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

type Raw = (usize, f64, f64, &'static [(usize, i32)], &'static str);

/// Le riff de la maquette (mesures 12 a 15), doigte par le moteur ; les
/// notes « manquees » sont des fantomes.
fn riff() -> TabView {
    let open = [64, 59, 55, 50, 45, 40];
    let o = 44.0; // mesure 12 = noire 44
    let mut notes = Vec::new();
    let mut ghost_cands = Vec::new();
    let raw: [Raw; 29] = [
        (0, 0.0, 4.0, &[(6, 0), (5, 2), (4, 2)], ""),
        (0, 4.0, 2.0, &[(3, 0)], ""),
        (0, 6.0, 2.0, &[(3, 2)], ""),
        (0, 8.0, 1.0, &[(2, 0)], ""),
        (0, 9.0, 1.0, &[(2, 1)], "L"),
        (0, 10.0, 1.0, &[(2, 3)], ""),
        (0, 11.0, 1.0, &[(1, 0)], ""),
        (0, 12.0, 2.0, &[(1, 3)], ""),
        (0, 14.0, 2.0, &[(1, 2)], ""),
        (1, 0.0, 2.0, &[(1, 0)], ""),
        (1, 2.0, 1.0, &[(2, 3)], ""),
        (1, 3.0, 1.0, &[(2, 1)], "L"),
        (1, 4.0, 2.0, &[(2, 0)], ""),
        (1, 6.0, 1.0, &[(3, 2)], "G"),
        (1, 7.0, 1.0, &[(3, 0)], ""),
        (1, 8.0, 4.0, &[(5, 3), (4, 2), (3, 0), (2, 1)], ""),
        (1, 12.0, 2.0, &[(4, 2)], ""),
        (1, 14.0, 1.0, &[(4, 4)], "G"),
        (1, 15.0, 1.0, &[(3, 0)], ""),
        (2, 0.0, 4.0, &[(4, 0), (3, 2), (2, 3), (1, 2)], ""),
        (2, 4.0, 1.0, &[(1, 2)], ""),
        (2, 5.0, 1.0, &[(1, 3)], "L"),
        (2, 6.0, 1.0, &[(1, 2)], "L"),
        (2, 7.0, 1.0, &[(1, 0)], ""),
        (2, 8.0, 2.0, &[(2, 3)], ""),
        (2, 10.0, 2.0, &[(2, 0)], ""),
        (2, 12.0, 4.0, &[(3, 2)], ""),
        (3, 0.0, 8.0, &[(6, 0), (5, 2), (4, 2), (3, 0)], ""),
        (3, 8.0, 2.0, &[(4, 2)], ""),
    ];
    for (m, t, d, ch, mark) in raw {
        for &(s, f) in ch {
            let n = TabNote {
                qn0: o + m as f64 * 4.0 + t / 4.0 + 0.01,
                qn1: o + m as f64 * 4.0 + (t + d) / 4.0,
                pitch: open[s - 1] + f,
                legato: mark == "L",
                vel: 90,
            };
            if mark == "G" {
                ghost_cands.push(n);
            } else {
                notes.push(n);
            }
        }
    }
    let mut v = TabView::new();
    let tuning = v.detect.tuning();
    let (doc, dropped) = cp_tab::doc::finger(&notes, 4, 112.0, &tuning);
    v.ghosts = cp_tab::doc::ghosts(&ghost_cands, &doc, 4, &tuning);
    v.dropped = dropped;
    // Le son et les attaques : un pic par note.
    let frames = 2000usize;
    let q_of = |f: usize| 40.0 + f as f64 * (24.0 / frames as f64);
    let mut env = vec![0.02f32; frames];
    let mut onset = vec![0.03f32; frames];
    for n in doc.iter().chain(v.ghosts.iter()) {
        let f0 = ((n.qn0 - 40.0) / 24.0 * frames as f64) as usize;
        let ghost = v.ghosts.iter().any(|g| g.key() == n.key());
        for k in 0..60 {
            if f0 + k < frames {
                let a = if ghost { 0.3 } else { 0.8 };
                env[f0 + k] = env[f0 + k].max(a * (-(k as f32) / 18.0).exp());
            }
        }
        if f0 < frames {
            onset[f0] = onset[f0].max(if ghost { 0.37 } else { 0.85 });
        }
    }
    v.set_take(Take {
        name: "02-210831_1211.wav".into(),
        track: "Guitar".into(),
        len_s: 86.3,
        qn0: 40.0,
        qn1: 64.0,
        origin_qn: 40.0,
        first_bar: 11,
        sig: (4, 4),
        bpm: 112.0,
        midi: Some("Guitar Tab: 02-210831".into()),
        frame_qn: (0..frames).map(q_of).collect(),
        env,
        onset,
    });
    v.set_doc(doc);
    v.scroll = 43.6;
    v.playhead = Some(50.3);
    v
}

fn click(v: &mut TabView, x: f32, y: f32) -> Option<Act> {
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

fn key(v: &mut TabView, k: u32) -> Option<Act> {
    v.event(&Ev::Key {
        vk: k,
        mods: Mods::default(),
        text: None,
    })
}

fn center(r: (f32, f32, f32, f32)) -> (f32, f32) {
    (r.0 + r.2 * 0.5, r.1 + r.3 * 0.5)
}

#[test]
fn la_fenetre_se_dessine_et_se_corrige() {
    let mut b = Bench::new(1560, 640);
    let mut v = riff();
    assert_eq!(v.ghosts.len(), 2, "deux fantomes");
    assert_eq!(v.dropped, 0);
    b.shot(&mut v, "1_riff");
    // Les mesures sont pleines.
    let e = v.engraved().clone();
    for m in 0..e.measures {
        let sum: i64 = e
            .signs
            .iter()
            .filter(|s| s.measure == m)
            .map(|s| s.dur)
            .sum();
        assert_eq!(sum, e.mlen);
    }
    // Un fantome garde d'un clic : il entre dans la tablature, tenu.
    let n0 = v.doc.len();
    let (gx, gy) = center(draw::ghost_rect(&v, 0).expect("un fantome a l'ecran"));
    assert_eq!(click(&mut v, gx, gy), Some(Act::Edited));
    assert_eq!(v.doc.len(), n0 + 1);
    assert_eq!(v.ghosts.len(), 1);
    let i = v.sel.unwrap();
    assert!(v.doc[i].locked);
    // Taper une case : 1 puis 2 vite = 12.
    b.draw(&mut v);
    let s0 = v.doc[i].string;
    assert_eq!(key(&mut v, 0x31), Some(Act::Edited));
    assert_eq!(key(&mut v, 0x32), Some(Act::Edited));
    let open = v.detect.tuning().strings[s0];
    assert_eq!((v.doc[i].fret, v.doc[i].pitch), (12, open + 12));
    // Meme note sur la corde voisine : la hauteur ne change pas.
    let p = v.doc[i].pitch;
    if key(&mut v, vk::DOWN).is_some() {
        assert_eq!(v.doc[i].pitch, p);
        assert_ne!(v.doc[i].string, s0);
    }
    // Effacer : la note part, et un nouveau decodage ne la ramenera pas.
    let k = v.doc[i].key();
    assert_eq!(key(&mut v, vk::DELETE), Some(Act::Edited));
    assert!(v.suppressed.contains(&k));
    b.shot(&mut v, "2_corrige");
}

#[test]
fn glisser_une_note_la_cale_sur_la_grille() {
    let mut b = Bench::new(1560, 640);
    let mut v = riff();
    b.draw(&mut v);
    let i = 3;
    let q0 = v.doc[i].qn0;
    let (x, y) = center(draw::note_rect(&v, i).expect("note visible"));
    let m = Mods::default();
    v.event(&Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: m,
    });
    let dx = v.lay.ppq * 0.25 * 2.0; // deux doubles croches
    v.event(&Ev::Move {
        x: x + dx,
        y,
        mods: m,
    });
    let a = v.event(&Ev::Up {
        x: x + dx,
        y,
        b: Button::Left,
        mods: m,
    });
    assert_eq!(a, Some(Act::Edited));
    let j = v.sel.unwrap();
    let moved = v.doc[j].qn0;
    let cells = (moved - 40.0) * 4.0;
    assert!((cells - cells.round()).abs() < 1e-9, "sur la grille");
    assert!((moved - (q0 + 0.5)).abs() < 0.13, "{q0} -> {moved}");
}

#[test]
fn les_reglages_et_le_passage() {
    let mut b = Bench::new(1560, 640);
    let mut v = riff();
    b.draw(&mut v);
    // Le curseur de sensibilite : un decodage, sur tout le morceau.
    let r = v
        .boxes
        .iter()
        .find(|x| x.4 == Hit::Slider(0))
        .copied()
        .unwrap();
    let a = click(&mut v, r.0 + r.2 * 0.2, r.1 + r.3 * 0.5);
    assert_eq!(a, Some(Act::Detect { range: None }));
    assert!(v.detect.onset < 0.5);
    // Un passage choisi sur la regle : le reglage ne s'applique qu'a lui.
    let ry = v.lay.ruler_y + 8.0;
    let (xa, xb) = (v.x_of(48.0), v.x_of(52.0));
    let m = Mods::default();
    v.event(&Ev::Down {
        x: xa,
        y: ry,
        b: Button::Left,
        dbl: false,
        mods: m,
    });
    v.event(&Ev::Move {
        x: xb,
        y: ry,
        mods: m,
    });
    v.event(&Ev::Up {
        x: xb,
        y: ry,
        b: Button::Left,
        mods: m,
    });
    let (qa, qb) = v.range.expect("un passage");
    assert!((qa - 48.0).abs() < 0.3 && (qb - 52.0).abs() < 0.3);
    b.draw(&mut v);
    let r = v
        .boxes
        .iter()
        .find(|x| x.4 == Hit::Slider(1))
        .copied()
        .unwrap();
    let a = click(&mut v, r.0 + r.2 * 0.6, r.1 + r.3 * 0.5);
    assert_eq!(
        a,
        Some(Act::Detect {
            range: Some((qa, qb))
        })
    );
    // Les modes d'affichage.
    v.mode = Mode::Tab;
    b.shot(&mut v, "3_tab");
    v.mode = Mode::Score;
    b.shot(&mut v, "4_partition");
    // Espace : la lecture de REAPER.
    assert_eq!(key(&mut v, vk::SPACE), Some(Act::Play));
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
}
