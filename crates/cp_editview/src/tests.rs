//! L'editeur pilote hors de REAPER : un riff de guitare (canal = corde), un
//! clip audio, les vraies polices (GDI), les gestes de REAPER, et des
//! planches PNG dans `%TEMP%` (cp_editview_*.png).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png};
use cp_roll::Roll;
use cp_sys::font::GdiGlyphs;
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

    fn draw(&mut self, v: &mut Ed) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(&mut cv, &mut self.t, &mut self.icons, &self.th, w, h);
    }

    fn shot(&mut self, v: &mut Ed, name: &str) {
        self.draw(v);
        let p = std::env::temp_dir().join(format!("cp_editview_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

/// Le riff de la maquette : quatre mesures, le canal est la corde.
fn riff() -> Ed {
    const N: [(i32, i32, f64, f64, i32); 27] = [
        (40, 5, 0.0, 0.5, 96),
        (40, 5, 0.5, 0.5, 78),
        (40, 5, 1.0, 0.5, 84),
        (43, 5, 1.5, 1.0, 110),
        (50, 4, 1.5, 1.0, 104),
        (45, 4, 2.5, 1.0, 108),
        (52, 3, 2.5, 1.0, 100),
        (40, 5, 3.5, 0.5, 80),
        (40, 5, 4.0, 1.5, 118),
        (47, 4, 4.0, 1.5, 112),
        (52, 3, 4.0, 1.5, 106),
        (50, 4, 5.5, 1.0, 98),
        (57, 3, 5.5, 1.0, 92),
        (48, 4, 6.5, 1.5, 104),
        (55, 3, 6.5, 1.5, 96),
        (59, 1, 8.0, 0.5, 88),
        (62, 1, 8.5, 0.5, 90),
        (64, 0, 9.0, 1.0, 112),
        (67, 0, 10.0, 0.5, 96),
        (64, 0, 10.5, 0.5, 86),
        (62, 1, 11.0, 1.0, 94),
        (69, 0, 12.0, 0.75, 116),
        (67, 0, 12.75, 0.25, 72),
        (64, 0, 13.0, 0.5, 84),
        (71, 0, 13.5, 1.0, 104),
        (74, 0, 14.5, 0.5, 98),
        (71, 0, 15.0, 1.0, 90),
    ];
    let roll = Roll::from_notes(
        N.iter()
            .map(|&(p, c, q, l, v)| (16.0 + q, l, p, c, v, (4.0..5.5).contains(&q), false)),
    );
    let mut e = Ed::new();
    e.open_midi(
        Midi {
            roll,
            name: "Rhythm".into(),
            item: "{ITEM}".into(),
            qn0: 16.0,
            qn1: 32.0,
        },
        true,
    );
    e.playhead = Some(25.0);
    e
}

fn m0() -> Mods {
    Mods::default()
}

fn down(v: &mut Ed, x: f32, y: f32, b: Button, m: Mods) -> Vec<Act> {
    v.event(&Ev::Down {
        x,
        y,
        b,
        dbl: false,
        mods: m,
    })
}

fn mv(v: &mut Ed, x: f32, y: f32, m: Mods) -> Vec<Act> {
    v.event(&Ev::Move { x, y, mods: m })
}

fn up(v: &mut Ed, x: f32, y: f32, b: Button, m: Mods) -> Vec<Act> {
    v.event(&Ev::Up { x, y, b, mods: m })
}

fn key(v: &mut Ed, k: u32, m: Mods) -> Vec<Act> {
    v.event(&Ev::Key {
        vk: k,
        mods: m,
        text: None,
    })
}

/// Le centre d'une note de hauteur `p` commencant a `q`.
fn note_xy(v: &Ed, p: i32, q: f64) -> (u64, f32, f32) {
    let n = v
        .midi()
        .unwrap()
        .roll
        .notes
        .iter()
        .find(|n| n.pitch == p && (n.qn0 - q).abs() < 1e-9)
        .copied()
        .expect("la note");
    let (x, y, w, h) = v.note_rect(&n).unwrap();
    (n.id, x + w * 0.5, y + h * 0.5)
}

fn chans(v: &Ed) -> Vec<(i32, i32)> {
    let mut c: Vec<(i32, i32)> = v
        .midi()
        .unwrap()
        .roll
        .notes
        .iter()
        .map(|n| (n.pitch, n.chan))
        .collect();
    c.sort();
    c.dedup();
    c
}

#[test]
fn l_editeur_se_dessine() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.shot(&mut v, "1_fit");
    // Fit : les notes de 40 a 74, deux rangs de marge, le reste replie.
    assert_eq!(v.rows.first(), Some(&76));
    assert_eq!(v.rows.last(), Some(&38));
    assert!(v.lay.fold_top && v.lay.fold_bot);
    assert!(v.rect_of(Hit::Help).is_some() && v.rect_of(Hit::Rows).is_some());
    // Seulement les rangs joues.
    let (x, y) = {
        let r = v.rect_of(Hit::Rows).unwrap();
        (r.0 + 4.0, r.1 + 4.0)
    };
    down(&mut v, x, y, Button::Left, m0());
    up(&mut v, x, y, Button::Left, m0());
    b.shot(&mut v, "2_notes");
    assert_eq!(v.rows.len(), 16);
    assert!(!v.lay.fold_top);
    // Les infobulles portent les mots.
    assert!(v.label(Hit::Help).unwrap().contains("Middle drag"));
    // Rien d'ouvert : un ecran vide.
    let mut e = Ed::new();
    b.shot(&mut e, "0_vide");
}

#[test]
fn deplacer_copier_garde_le_canal() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let before = chans(&v);
    // Glisser la note de Mi (64) d'une noire a droite et d'un ton plus haut.
    let (id, x, y) = note_xy(&v, 64, 25.0);
    assert!(matches!(
        down(&mut v, x, y, Button::Left, m0()).first(),
        Some(Act::Audition { pitch: 64, .. })
    ));
    let ppq = v.lay.ppq;
    let rh = v.lay.rh;
    mv(&mut v, x + ppq * 1.02, y - rh * 2.0, m0());
    let a = up(&mut v, x + ppq * 1.02, y - rh * 2.0, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: move notes")), "{a:?}");
    let n = *v.midi().unwrap().roll.get(id).unwrap();
    assert_eq!((n.pitch, n.qn0, n.chan), (66, 26.0, 0));
    // Ctrl + glisser : la copie reste, l'original part.
    b.draw(&mut v);
    let (_, x, y) = note_xy(&v, 66, 26.0);
    let ctrl = Mods { ctrl: true, ..m0() };
    down(&mut v, x, y, Button::Left, ctrl);
    mv(&mut v, x + ppq * 2.0, y, ctrl);
    let a = up(&mut v, x + ppq * 2.0, y, Button::Left, ctrl);
    assert!(a.contains(&Act::Commit("CP Editor: copy notes")));
    let r = &v.midi().unwrap().roll;
    assert!(r.notes.iter().any(|n| n.pitch == 66 && n.qn0 == 26.0));
    assert!(
        r.notes
            .iter()
            .any(|n| n.pitch == 66 && n.qn0 == 28.0 && n.chan == 0)
    );
    // Aucun canal n'a change.
    let after = chans(&v);
    for c in &after {
        assert!(before.iter().any(|x| x.1 == c.1), "{c:?}");
    }
}

#[test]
fn poser_allonger_effacer() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let n0 = v.midi().unwrap().roll.notes.len();
    // Une note neuve dans une case vide, glissee sur deux doubles croches.
    let q = 30.25;
    let x = v.x_of(q) + 2.0;
    let i = v.row_of(60).unwrap();
    let y = v.row_y(i) + v.lay.rh * 0.5;
    assert_eq!(v.hit(x, y), Hit::Grid);
    down(&mut v, x, y, Button::Left, m0());
    let x2 = v.x_of(q + 0.4);
    mv(&mut v, x2, y, m0());
    let a = up(&mut v, x2, y, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: insert note")));
    let r = &v.midi().unwrap().roll;
    assert_eq!(r.notes.len(), n0 + 1);
    let nn = r.notes.iter().find(|n| n.pitch == 60).unwrap();
    assert!(
        (nn.qn0 - 30.25).abs() < 1e-9 && (nn.len - 0.5).abs() < 1e-9,
        "{nn:?}"
    );
    // Le bord droit d'une note : sa longueur.
    b.draw(&mut v);
    let (id, _, y) = note_xy(&v, 74, 30.5);
    let n = *v.midi().unwrap().roll.get(id).unwrap();
    let (nx, _, nw, _) = v.note_rect(&n).unwrap();
    let xr = nx + nw - 1.0;
    assert!(matches!(
        v.hit(xr, y),
        Hit::Note {
            part: Part::Right,
            ..
        }
    ));
    down(&mut v, xr, y, Button::Left, m0());
    let x2 = xr + v.lay.ppq * 0.5;
    mv(&mut v, x2, y, m0());
    let a = up(&mut v, x2, y, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: note length")));
    assert!((v.midi().unwrap().roll.get(id).unwrap().len - 1.0).abs() < 1e-9);
    // Alt + clic : effacer.
    b.draw(&mut v);
    let (_, x, y) = note_xy(&v, 69, 28.0);
    let alt = Mods { alt: true, ..m0() };
    down(&mut v, x, y, Button::Left, alt);
    let a = up(&mut v, x, y, Button::Left, alt);
    assert!(a.contains(&Act::Commit("CP Editor: erase notes")));
    assert!(!v.midi().unwrap().roll.notes.iter().any(|n| n.pitch == 69));
    // Clic droit sur une note : la supprimer.
    b.draw(&mut v);
    let (_, x, y) = note_xy(&v, 71, 29.5);
    down(&mut v, x, y, Button::Right, m0());
    let a = up(&mut v, x, y, Button::Right, m0());
    assert_eq!(a, vec![Act::Commit("CP Editor: delete note")]);
    b.shot(&mut v, "3_gestes");
}

#[test]
fn lasso_touches_velocite() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    // Le lasso (clic droit glisse) sur la premiere mesure, tout le clavier.
    let (x0, y0) = (v.x_of(15.9), v.lay.gy + 1.0);
    let (x1, y1) = (v.x_of(19.9), v.lay.gy + v.lay.gh - 1.0);
    down(&mut v, x0, y0, Button::Right, m0());
    mv(&mut v, x1, y1, m0());
    up(&mut v, x1, y1, Button::Right, m0());
    assert_eq!(v.midi().unwrap().roll.sel_count(), 8);
    // Fleche haut : un demi-ton, un pas d'annulation.
    let a = key(&mut v, vk::UP, m0());
    assert!(a.contains(&Act::Commit("CP Editor: transpose")));
    // Ctrl+D : la copie tombe sur la mesure suivante.
    let a = key(&mut v, 0x44, Mods { ctrl: true, ..m0() });
    assert!(a.contains(&Act::Commit("CP Editor: duplicate")));
    assert!(
        v.midi()
            .unwrap()
            .roll
            .notes
            .iter()
            .filter(|n| n.sel)
            .all(|n| n.qn0 >= 20.0)
    );
    // Supprimer.
    let a = key(&mut v, vk::DELETE, m0());
    assert!(a.contains(&Act::Commit("CP Editor: delete notes")));
    // La velocite d'une note : tiree vers le haut.
    b.draw(&mut v);
    let n = v
        .midi()
        .unwrap()
        .roll
        .notes
        .iter()
        .find(|n| n.pitch == 64)
        .copied()
        .unwrap();
    let x = v.x_of(n.qn0) + 1.0;
    let yb = v.lay.vel_y + v.lay.vel_h - 20.0;
    assert_eq!(v.hit(x, yb), Hit::Vel(n.id));
    down(&mut v, x, yb, Button::Left, m0());
    let y2 = v.lay.vel_y + 6.0;
    mv(&mut v, x, y2, m0());
    let a = up(&mut v, x, y2, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: velocity")));
    assert!(v.midi().unwrap().roll.get(n.id).unwrap().vel > 120);
    // Alt + fleche : la note suivante, entendue.
    let a = key(&mut v, vk::RIGHT, Mods { alt: true, ..m0() });
    assert!(matches!(a.first(), Some(Act::Audition { .. })));
    // Les transformations.
    v.midi_mut().unwrap().roll.select_all();
    assert_eq!(v.transform(51), vec![Act::Commit("CP Editor: length")]);
    assert!(
        v.transform_menu()
            .iter()
            .flatten()
            .any(|x| x.1.contains("REAPER"))
    );
}

#[test]
fn naviguer_comme_dans_reaper() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let (mx, my) = (v.lay.sx + v.lay.sw * 0.5, v.lay.gy + 50.0);
    let q = v.qn_of(mx);
    let z = v.zoom;
    v.event(&Ev::Wheel {
        x: mx,
        y: my,
        dx: 0.0,
        dy: 1.0,
        mods: Mods {
            shift: true,
            ..m0()
        },
    });
    assert!(v.zoom > z && (v.qn_of(mx) - q).abs() < 1e-3);
    // Ctrl + molette : des rangs plus hauts, qui debordent.
    let rh = v.lay.rh;
    for _ in 0..4 {
        v.event(&Ev::Wheel {
            x: mx,
            y: my,
            dx: 0.0,
            dy: 1.0,
            mods: Mods { ctrl: true, ..m0() },
        });
    }
    b.draw(&mut v);
    assert!(v.lay.rh > rh && v.lay.content_h > v.lay.gh);
    // La molette seule : les rangs defilent.
    let vs = v.vscroll;
    v.event(&Ev::Wheel {
        x: mx,
        y: my,
        dx: 0.0,
        dy: -1.0,
        mods: m0(),
    });
    assert!(v.vscroll > vs);
    // Home : tout recadre.
    key(&mut v, vk::HOME, m0());
    b.draw(&mut v);
    assert!(v.lay.content_h <= v.lay.gh + 0.5);
    // La bande repliee du haut s'ouvre d'une octave.
    let r0 = v.rows.len();
    let y = v.lay.gy - v.lay.fold_h * 0.5;
    assert_eq!(v.hit(mx, y), Hit::FoldTop);
    down(&mut v, mx, y, Button::Left, m0());
    up(&mut v, mx, y, Button::Left, m0());
    b.draw(&mut v);
    assert_eq!(v.rows.len(), r0 + 12);
}

#[test]
fn le_clip_audio() {
    let mut b = Bench::new(1280, 560);
    let mut e = Ed::new();
    let wave = |n: usize| -> Vec<(f32, f32)> {
        (0..n)
            .map(|k| {
                let t = k as f32 / n as f32;
                let env = (1.0 - (t * 8.0).fract()).powf(2.0) * 0.8;
                (-env, env)
            })
            .collect()
    };
    e.open_audio(
        Audio {
            name: "Guitar 1 \u{00b7} Audio-03".into(),
            item: "{A}".into(),
            qn0: 8.0,
            qn1: 24.0,
            gain_db: -3.0,
            fade_in: 0.5,
            fade_out: 2.0,
            pitch: 0.0,
            rate: 1.0,
            reverse: false,
            peaks: Vec::new(),
            pk: (0.0, 0.0, 0),
        },
        true,
    );
    b.draw(&mut e);
    let (q0, q1, cols) = e.peaks_wanted().expect("des cretes");
    assert!(cols > 100);
    e.set_peaks(q0, q1, wave(cols));
    b.shot(&mut e, "4_audio");
    assert!(e.peaks_wanted().is_none());
    // Rogner le debut d'une mesure (calee).
    let x = e.x_of(8.0);
    let y = e.lay.gy + e.lay.gh * 0.6;
    assert_eq!(e.hit(x, y), Hit::EdgeL);
    down(&mut e, x, y, Button::Left, m0());
    let x2 = x + e.lay.ppq * 4.1;
    let a = mv(&mut e, x2, y, m0());
    assert_eq!(a, vec![Act::AudioLive]);
    let a = up(&mut e, x2, y, Button::Left, m0());
    assert_eq!(a, vec![Act::AudioCommit("CP Editor: trim")]);
    assert!((e.audio().unwrap().qn0 - 12.0).abs() < 1e-9);
    // La ligne du gain, tiree vers le bas.
    b.draw(&mut e);
    let gy = e.gain_y(-3.0);
    let gx = e.x_of(16.0);
    assert_eq!(e.hit(gx, gy), Hit::Gain);
    down(&mut e, gx, gy, Button::Left, m0());
    mv(&mut e, gx, gy + 40.0, m0());
    up(&mut e, gx, gy + 40.0, Button::Left, m0());
    assert!(e.audio().unwrap().gain_db < -3.0);
}
