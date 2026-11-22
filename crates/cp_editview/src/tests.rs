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
            ..Midi::default()
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

/// Chaque note a son rang dans la vue.
fn all_notes_on_screen(v: &Ed) -> bool {
    v.midi().unwrap().roll.notes.iter().all(|n| {
        let i = v.row_of(n.pitch).unwrap();
        let y = v.row_y(i);
        y >= v.lay.gy - 0.5 && y + v.lay.rh <= v.lay.gy + v.lay.gh + 0.5
    })
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
    // Tout le clavier, cadre sur les notes (40 a 74) a l'ouverture.
    assert_eq!(v.rows.first(), Some(&127));
    assert_eq!(v.rows.last(), Some(&0));
    assert!(all_notes_on_screen(&v));
    assert!(v.rect_of(Hit::Help).is_some() && v.rect_of(Hit::Rows).is_some());
    assert!(v.rect_of(Hit::Hear).is_some());
    // Seulement les rangs joues.
    let (x, y) = {
        let r = v.rect_of(Hit::Rows).unwrap();
        (r.0 + 4.0, r.1 + 4.0)
    };
    down(&mut v, x, y, Button::Left, m0());
    up(&mut v, x, y, Button::Left, m0());
    b.shot(&mut v, "2_notes");
    assert_eq!(v.rows.len(), 16);
    assert!(all_notes_on_screen(&v));
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
    // Clic droit sur une note : le menu (comme REAPER), la note choisie.
    b.draw(&mut v);
    let (id, x, y) = note_xy(&v, 71, 29.5);
    down(&mut v, x, y, Button::Right, m0());
    let a = up(&mut v, x, y, Button::Right, m0());
    assert!(a.contains(&Act::ContextMenu { x, y }), "{a:?}");
    assert!(v.midi().unwrap().roll.get(id).unwrap().sel);
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
    // Le cadrage demande : les notes reviennent toutes a l'ecran.
    v.vscroll = 0.0;
    b.draw(&mut v);
    assert!(!all_notes_on_screen(&v));
    v.fit();
    b.draw(&mut v);
    assert!(all_notes_on_screen(&v));
}

/// Le defaut du 2026-11-22 : glisser une note vers le haut l'envoyait en C9
/// (la vue se recadrait sur la note pendant le geste, qui remontait encore).
/// La camera ne bouge plus pendant un geste ; au bord, elle defile.
#[test]
fn glisser_ne_recadre_pas() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let (rh, vs) = (v.lay.rh, v.vscroll);
    let (id, x, y) = note_xy(&v, 74, 30.5);
    down(&mut v, x, y, Button::Left, m0());
    // Trois rangs plus haut, image apres image.
    for k in 1..=6 {
        mv(&mut v, x, y - rh * 0.5 * k as f32, m0());
        b.draw(&mut v);
    }
    assert_eq!(v.midi().unwrap().roll.get(id).unwrap().pitch, 77);
    assert_eq!((v.lay.rh, v.vscroll), (rh, vs), "la vue n'a pas bouge");
    // Au-dessus de la grille : la vue defile, la note suit, sans zoom.
    let ya = v.lay.gy - 30.0;
    mv(&mut v, x, ya, m0());
    for _ in 0..10 {
        v.tick();
        b.draw(&mut v);
    }
    let p = v.midi().unwrap().roll.get(id).unwrap().pitch;
    assert!(v.vscroll < vs && p > 77 && p < 127, "{p}");
    assert_eq!(v.lay.rh, rh);
    let a = up(&mut v, x, ya, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: move notes")));
    // Relu (le meme item) : la camera reste ou elle est.
    let m = v.midi().unwrap().clone();
    let vs2 = v.vscroll;
    v.open_midi(m, false);
    b.draw(&mut v);
    assert_eq!(v.vscroll, vs2);
}

/// La table souris de REAPER (ROADMAP_Editeur.md §2) au-dela du clic et du
/// glisser simples.
#[test]
fn la_table_souris_de_reaper() {
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    // Un clic : la note choisie, le curseur la ou l'on clique (la ligne de
    // grille la plus proche), pas au debut de la note (Cedric, 2026-11-22).
    let (id, x, y) = note_xy(&v, 64, 25.0);
    down(&mut v, x, y, Button::Left, m0());
    let a = up(&mut v, x, y, Button::Left, m0());
    let want = (v.qn_of(x) / v.grid).round() * v.grid;
    assert!(want > 25.0, "le clic est dans la note, apres son debut");
    assert!(a.contains(&Act::Seek(want)), "{a:?}");
    // Maj+Win : double la longueur ; Ctrl+Win : la divise.
    let sw = Mods {
        shift: true,
        win: true,
        ..m0()
    };
    down(&mut v, x, y, Button::Left, sw);
    let a = up(&mut v, x, y, Button::Left, sw);
    assert!(a.contains(&Act::Commit("CP Editor: double length")));
    assert!((v.midi().unwrap().roll.get(id).unwrap().len - 2.0).abs() < 1e-9);
    let cw = Mods {
        ctrl: true,
        win: true,
        ..m0()
    };
    down(&mut v, x, y, Button::Left, cw);
    up(&mut v, x, y, Button::Left, cw);
    assert!((v.midi().unwrap().roll.get(id).unwrap().len - 1.0).abs() < 1e-9);
    // Ctrl + glisser sur la grille vide : la selection copiee, une mesure
    // plus loin.
    let n0 = v.midi().unwrap().roll.notes.len();
    let (gx, gy) = (
        v.x_of(26.1),
        v.row_y(v.row_of(48).unwrap()) + v.lay.rh * 0.5,
    );
    assert_eq!(v.hit(gx, gy), Hit::Grid);
    let ctrl = Mods { ctrl: true, ..m0() };
    down(&mut v, gx, gy, Button::Left, ctrl);
    let gx2 = gx + v.lay.ppq * 4.0;
    mv(&mut v, gx2, gy, ctrl);
    let a = up(&mut v, gx2, gy, Button::Left, ctrl);
    assert!(a.contains(&Act::Commit("CP Editor: copy notes")), "{a:?}");
    let r = &v.midi().unwrap().roll;
    assert_eq!(r.notes.len(), n0 + 1);
    assert!(
        r.notes
            .iter()
            .any(|n| n.pitch == 64 && (n.qn0 - 29.0).abs() < 1e-9 && n.chan == 0)
    );
    // Un clic Ctrl sur la grille : rien de choisi, le curseur sous la souris.
    b.draw(&mut v);
    down(&mut v, gx, gy, Button::Left, ctrl);
    let a = up(&mut v, gx, gy, Button::Left, ctrl);
    assert!(matches!(a.as_slice(), [Act::Seek(_)]));
    assert_eq!(v.midi().unwrap().roll.sel_count(), 0);
    // Maj+Ctrl+Alt glisser : un accord (trois rangs dans le meme pas).
    let sca = Mods {
        shift: true,
        ctrl: true,
        alt: true,
        win: false,
    };
    let (px, py) = (
        v.x_of(30.05),
        v.row_y(v.row_of(42).unwrap()) + v.lay.rh * 0.5,
    );
    down(&mut v, px, py, Button::Left, sca);
    let py2 = py - v.lay.rh * 2.0;
    mv(&mut v, px + 6.0, py2, sca);
    let a = up(&mut v, px + 6.0, py2, Button::Left, sca);
    assert!(a.contains(&Act::Commit("CP Editor: paint chords")), "{a:?}");
    let chord: Vec<i32> = v
        .midi()
        .unwrap()
        .roll
        .notes
        .iter()
        .filter(|n| n.sel)
        .map(|n| n.pitch)
        .collect();
    assert_eq!(chord, vec![42, 43, 44]);
    // Clic droit sur la grille : le menu.
    down(&mut v, gx, gy, Button::Right, m0());
    let a = up(&mut v, gx, gy, Button::Right, m0());
    assert!(matches!(a.as_slice(), [Act::ContextMenu { .. }]));
    // L'ecoute coupee : plus d'Audition.
    v.hear = false;
    b.draw(&mut v);
    let (_, x, y) = note_xy(&v, 64, 25.0);
    let a = down(&mut v, x, y, Button::Left, m0());
    assert!(!a.iter().any(|x| matches!(x, Act::Audition { .. })));
    up(&mut v, x, y, Button::Left, m0());
}

/// Les raccourcis : les defauts de REAPER, puis reaper-kb.ini (les lignes de
/// Cedric), puis l'action rendue en operation.
#[test]
fn les_raccourcis_de_reaper() {
    use crate::keys::{Bind, KeyMap, Named, named};
    // Trois defauts (la vraie liste vient de REAPER).
    let defs = [(0x802E, 40667, 1), (0x8026, 40138, 1), (0x51, 40469, 1)];
    let ini = concat!(
        "KEY 1 32806 40177 32060\t\t # MIDI Editor : HAUT : OVERRIDE DEFAULT : Edit: Move notes up one semitone\n",
        "KEY 1 81 40009 32060\t\t # MIDI Editor : Q : OVERRIDE DEFAULT : Quantize...\n",
        "KEY 255 248 40432 32060\t\t # MIDI Editor : Mousewheel\n",
        "KEY 1 8 0 32060\n",
        "KEY 9 68 _RS7d3c_fbfb 32060\t\t # MIDI Editor : Ctrl+D : Script\n",
        "KEY 1 90 _f988 32060\t\t # MIDI Editor : Z : Custom: Delete note under mouse cursor\n",
        "KEY 1 90 40001 0\n",
        "SCR 4 32060 RS7d3c_fbfb \"Custom: zaibuyidao_Duplicate Musical.lua\" \"x/y.lua\"\n",
        "ACT 0 32060 \"f988\" \"Custom: Delete note under mouse cursor\" 40001 40002\n",
    );
    let km = KeyMap::build(&defs, ini);
    assert_eq!(km.get(vk::DELETE, true, m0()), Some(&Bind::Cmd(40667)));
    assert_eq!(
        km.get(vk::UP, true, m0()),
        Some(&Bind::Cmd(40177)),
        "remplace le defaut"
    );
    assert_eq!(km.get(0x51, false, m0()), Some(&Bind::Cmd(40009)));
    assert_eq!(km.get(vk::UP, false, m0()), None, "le pave numerique");
    let d = km.get(0x44, false, Mods { ctrl: true, ..m0() }).cloned();
    assert_eq!(
        d,
        Some(Bind::Named(
            "Custom: zaibuyidao_Duplicate Musical.lua".into()
        ))
    );
    let z = km.get(0x5A, false, m0()).cloned();
    assert_eq!(
        z,
        Some(Bind::Named("Custom: Delete note under mouse cursor".into()))
    );
    assert_eq!(
        named("Custom: zaibuyidao_Duplicate Musical.lua"),
        Some(Named::Duplicate)
    );
    assert_eq!(
        named(
            "Script: FTC_MeMagic (2-3) Horizontally zoom to item + Vertically zoom to all notes in item.lua"
        ),
        Some(Named::Fit)
    );
    // Les actions, sur le riff.
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let sel = |v: &Ed| -> Vec<i32> {
        v.midi()
            .unwrap()
            .roll
            .notes
            .iter()
            .filter(|n| n.sel)
            .map(|n| n.pitch)
            .collect()
    };
    let s0 = sel(&v);
    let a = v.command(40177).unwrap();
    assert!(a.contains(&Act::Commit("CP Editor: transpose")));
    assert_eq!(sel(&v), s0.iter().map(|p| p + 1).collect::<Vec<_>>());
    assert_eq!(v.command(40016), Some(vec![Act::Main(40044)]));
    v.command(40197);
    assert!((v.grid - 0.5).abs() < 1e-9);
    assert!(
        v.command(40055)
            .unwrap()
            .contains(&Act::Commit("CP Editor: mute"))
    );
    assert!(
        v.midi()
            .unwrap()
            .roll
            .notes
            .iter()
            .filter(|n| n.sel)
            .all(|n| n.mute)
    );
    assert_eq!(v.command(99999), None);
    let a = v.named_cmd(Named::Duplicate);
    assert!(a.contains(&Act::Commit("CP Editor: duplicate")));
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

/// Le cout d'une image de l'editeur, ancre en bas comme chez Cedric
/// (2123 x 560), et pendant le glisser d'une note. `cargo test -p
/// cp_editview --release -- --ignored --nocapture banc`
#[test]
#[ignore]
fn banc_image_editeur() {
    let mut b = Bench::new(2123, 560);
    let mut v = riff();
    b.draw(&mut v);
    let t = std::time::Instant::now();
    for _ in 0..20 {
        v.mark();
        b.draw(&mut v);
    }
    println!(
        "banc editeur : {:.2} ms par image",
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
    // Un glisser : 40 mouvements, une image chacun (comme avant).
    let id = v.midi().unwrap().roll.notes[3].id;
    let (x, y, _, _) = v
        .rect_of(Hit::Note {
            id,
            part: Part::Body,
        })
        .unwrap();
    let (x, y) = (x + 4.0, y + 3.0);
    down(&mut v, x, y, Button::Left, m0());
    let t = std::time::Instant::now();
    for k in 0..40 {
        v.event(&Ev::Move {
            x: x + k as f32 * 3.0,
            y: y - k as f32,
            mods: m0(),
        });
        b.draw(&mut v);
    }
    println!(
        "banc glisser : {:.2} ms par mouvement",
        t.elapsed().as_secs_f64() * 1000.0 / 40.0
    );
}

/// Une vue jamais dessinee redessine toute sa fenetre (elle restait noire).
#[test]
fn une_vue_neuve_se_dessine_entiere() {
    let mut v = riff();
    assert_eq!(
        v.take_damage(640, 480),
        vec![cp_gfx::IRect::new(0, 0, 640, 480)]
    );
}

/// Le survol ne redessine que ses zones : chaque cible survolee doit donner,
/// redessinee par zones, la meme image qu'un dessin complet (sinon il
/// n'apparait qu'au prochain dessin complet).
#[test]
fn chaque_survol_se_redessine_tout_de_suite() {
    let (w, h) = (1400, 760);
    let mut b = Bench::new(w, h);
    let mut v = riff();
    b.draw(&mut v);
    let _ = v.take_damage(w, h);
    let targets: Vec<(f32, f32)> = v
        .boxes
        .iter()
        .map(|bx| (bx.0 + bx.2 * 0.5, bx.1 + bx.3 * 0.5))
        .collect();
    let mut seen: Vec<Hit> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for (x, y) in targets {
        let hit = v.hit(x, y);
        if seen.contains(&hit) {
            continue;
        }
        seen.push(hit);
        let prev = v.hover;
        mv(&mut v, x, y, m0());
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
                "{prev:?} -> {hit:?} en ({x:.0},{y:.0}) : {} px de ({x0},{y0}) a ({x1},{y1})",
                diff.len()
            ));
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

/// Le Mi (64, corde 1) du riff porte un bend d'un ton (Guitar Tab : canal
/// de la corde, etendue 12) : il se dessine au-dessus de la note, et la suit
/// quand on la glisse.
#[test]
fn le_bend_se_dessine_et_suit_la_note() {
    use cp_roll::expr::{Ev as Xe, Expr};
    let mut b = Bench::new(1280, 760);
    let mut v = riff();
    b.draw(&mut v);
    let (id, x, y) = note_xy(&v, 64, 25.0);
    let rh = v.lay.rh;
    let ppq = v.lay.ppq;
    // Ou la courbe passe : aux quatre cinquiemes de la note, deux rangs
    // au-dessus de son milieu (quelques pixels autour).
    let probe = |v: &Ed, b: &Bench| -> Vec<u32> {
        let n = *v.midi().unwrap().roll.get(id).unwrap();
        let (nx, ny, nw, nh) = v.note_rect(&n).unwrap();
        let px = (nx + nw * 0.8) as i32;
        let py = (ny + nh * 0.5 - 2.0 * rh).round() as i32;
        (-2..=2).map(|d| b.s.get(px, py + d)).collect()
    };
    let flat = probe(&v, &b);
    let mut evs = vec![
        Xe::cc(16.0, 0, 101, 0),
        Xe::cc(16.0, 0, 100, 0),
        Xe::cc(16.0, 0, 6, 12),
        Xe::pitch_bend(25.0 - 1.0 / 960.0, 0, 0.0),
    ];
    for k in 1..=8 {
        let t = k as f32 / 8.0;
        evs.push(Xe::pitch_bend(25.0 + 0.4 * t as f64, 0, 2.0 / 12.0 * t));
    }
    evs.push(Xe::pitch_bend(26.5, 0, 0.0));
    {
        let m = v.midi_mut().unwrap();
        m.expr = Expr::new(&m.roll.notes, &evs);
    }
    v.mark();
    b.shot(&mut v, "8_bend");
    assert_ne!(probe(&v, &b), flat, "le bend se voit au-dessus de la note");
    // Glisser la note d'une noire : la courbe suit, et l'ecriture aussi.
    down(&mut v, x, y, Button::Left, m0());
    mv(&mut v, x + ppq * 1.02, y, m0());
    let a = up(&mut v, x + ppq * 1.02, y, Button::Left, m0());
    assert!(a.contains(&Act::Commit("CP Editor: move notes")), "{a:?}");
    b.draw(&mut v);
    let moved = probe(&v, &b);
    // Sans la courbe, au meme endroit : le fond.
    let bare = {
        let m = v.midi_mut().unwrap();
        let keep = std::mem::take(&mut m.expr);
        v.mark();
        b.draw(&mut v);
        let p = probe(&v, &b);
        v.midi_mut().unwrap().expr = keep;
        p
    };
    assert_ne!(moved, bare, "la courbe a suivi la note");
    let m = v.midi().unwrap();
    let out = m.expr.carry(&m.roll.notes);
    let top = out
        .iter()
        .filter(|e| e.bend().is_some_and(|x| x > 0.15))
        .map(|e| e.qn)
        .fold(f64::MIN, f64::max);
    assert!((top - 26.4).abs() < 1e-9, "{top}");
}
