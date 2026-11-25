//! La vue hors de REAPER : les vraies polices (GDI), des gestes, et des
//! planches PNG dans `%TEMP%` (cp_fxc_*.png).

use std::collections::BTreeMap;

use cp_fxc::links::{Chain, ChainFx, Ctx, plan};
use cp_fxc::model::{PState, Track, pkey};
use cp_fxc::roll::Settings;
use cp_fxc::slots::fx_kind;
use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png};
use cp_sys::font::GdiGlyphs;
use cp_ui::{Button, Ev, IconCache, Mods, Theme};

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
            t: TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas"]))),
            icons: IconCache::default(),
            th: Theme::dark(1.0),
        }
    }
    fn shot(&mut self, v: &mut View, d: &Data, name: &str) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        v.layout(d, &self.th, w, h);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(&mut cv, &mut self.t, &mut self.icons, &self.th);
        let p = std::env::temp_dir().join(format!("cp_fxc_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

struct World {
    t: Track,
    fx: Vec<FxRow>,
    bank: Bank,
    sg: Js,
    seq: Js,
    roll: Settings,
    src: BTreeMap<i32, f64>,
}

fn world() -> World {
    let mut t = Track::default();
    let mut fx = Vec::new();
    for (g, name, n) in [
        ("{A}", "Pro-Q 3", 12),
        ("{B}", "Valhalla Delay", 9),
        ("{C}", "Reverb", 6),
    ] {
        let params = (0..n)
            .map(|i| ParamRow {
                key: pkey(g, i),
                index: i,
                name: format!("Param {i}"),
                live: 0.3 + 0.05 * i as f64,
                text: format!("{:.1} dB", i as f64 * 1.5),
                steps: 0,
            })
            .collect();
        fx.push(FxRow {
            guid: g.into(),
            name: name.into(),
            enabled: g != "{B}",
            detached: g == "{C}",
            params,
            hidden: if g == "{A}" { 2080 } else { 0 },
        });
    }
    t.params.insert(
        pkey("{A}", 1),
        PState {
            sel: true,
            base: 0.4,
            range: 0.6,
            x: true,
            y: false,
            ..PState::default()
        },
    );
    let mut ps = PState {
        sel: true,
        base: 0.6,
        range: 0.3,
        x: false,
        y: false,
        ..PState::default()
    };
    ps.add_source(2).unwrap();
    ps.add_source(211).unwrap();
    ps.srcs[1].depth = -0.4;
    t.params.insert(pkey("{A}", 3), ps);
    t.pad.x = 0.7;
    t.pad.y = 0.35;
    t.pad.zone = [0.1, 0.9, 0.2, 0.8];
    t.macro_names[0] = "Cutoff".into();
    let mut bank = Bank {
        slots: vec![Slot::default(); 8],
        ext: true,
    };
    for (i, s) in bank.slots.iter_mut().enumerate() {
        s.on = i < 3;
        s.shape = (i % 8) as u8;
        s.rate_hz = 1.0;
        s.rate_n = 0.5;
        s.len = 16;
        s.steps = [31; 16];
        s.pho = 0.3;
    }
    bank.slots[2].shape = 7;
    let mut sg = Js {
        raw: vec![0.0; 147],
        norm: vec![0.5; 147],
        enabled: true,
    };
    sg.raw[0] = 1.0;
    sg.raw[17] = 1.0;
    sg.raw[2] = 440.0;
    sg.raw[18] = 1.0;
    sg.raw[41] = 1.0;
    sg.raw[42] = 2000.0;
    let mut seq = Js {
        raw: vec![0.0; 230],
        norm: vec![0.5; 230],
        enabled: true,
    };
    seq.raw[0] = 1.0;
    for l in 0..9 {
        let b = panels::lane_base(l);
        seq.raw[b + 16] = 16.0;
        for k in 0..16 {
            seq.raw[b + k] = match l {
                0 => (k % 3 != 0) as i32 as f64,
                1 => (k as f64 % 8.0) - 2.0,
                3 => 100.0,
                4 => 50.0,
                5 => 100.0,
                7 | 8 => k as f64 / 15.0,
                _ => 0.0,
            };
        }
    }
    seq.raw[220] = 5.0;
    World {
        t,
        fx,
        bank,
        sg,
        seq,
        roll: Settings::default(),
        src: BTreeMap::new(),
    }
}

fn chain() -> Chain {
    Chain {
        fx: vec![
            ChainFx {
                guid: "{A}".into(),
                name: "VST3: Pro-Q 3".into(),
                kind: Some(fx_kind("VST3: Pro-Q 3")),
                nparams: 12,
            },
            ChainFx {
                guid: "{BR}".into(),
                name: "JS: FX Constellation Bridge".into(),
                kind: Some(fx_kind("JS: FX Constellation Bridge")),
                nparams: 35,
            },
        ],
    }
}

fn data<'a>(w: &'a World, p: &'a cp_fxc::links::Plan) -> Data<'a> {
    Data {
        track: Some("Synth lead"),
        locked: true,
        t: &w.t,
        fx: &w.fx,
        plan: p,
        bridge: true,
        engine_runs: false,
        bank: Some(&w.bank),
        gbank: None,
        sg: Some(&w.sg),
        seq: Some(&w.seq),
        macros: [0.2, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        preset: "Big one",
        modified: true,
        snapshot_label: "Snapshot2".into(),
        has_preset: true,
        morph: None,
        roll: &w.roll,
        links: 3,
        modulated: 1,
        src: &w.src,
        msg: None,
        kb_note: None,
        guids: Guids::default(),
        capture: 2,
    }
}

fn none() -> Mods {
    Mods::default()
}

fn down(x: f32, y: f32, b: Button, mods: Mods) -> Ev {
    Ev::Down {
        x,
        y,
        b,
        dbl: false,
        mods,
    }
}

fn up(x: f32, y: f32, b: Button, mods: Mods) -> Ev {
    Ev::Up { x, y, b, mods }
}

fn mv(x: f32, y: f32, mods: Mods) -> Ev {
    Ev::Move { x, y, mods }
}

fn rect(v: &View, c: &Ctl) -> R {
    v.items().iter().rev().find(|i| &i.ctl == c).unwrap().r
}

#[test]
fn the_three_tabs_draw() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let mut b = Bench::new(1500, 820);
    let mut v = View::new();
    b.shot(&mut v, &d, "params");
    v.tab = 1;
    b.shot(&mut v, &d, "soundgen");
    v.tab = 2;
    v.kb_open = true;
    b.shot(&mut v, &d, "seq");
    v.tab = 0;
    v.sel_slot = 3;
    b.shot(&mut v, &d, "step");
    v.sel_slot = 202;
    b.shot(&mut v, &d, "env");
    let mut small = Bench::new(820, 640);
    small.shot(&mut v, &d, "narrow");
}

#[test]
fn every_control_is_where_it_is_drawn() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.0);
    let mut v = View::new();
    v.layout(&d, &th, 1500.0, 820.0);
    for it in v
        .items()
        .iter()
        .filter(|i| i.ctl != Ctl::None && i.r.w > 4.0 && i.r.h > 4.0)
    {
        let (x, y) = (it.r.cx(), it.r.cy());
        if !(0.0..1500.0).contains(&x) || !(0.0..820.0).contains(&y) {
            continue;
        }
        // Une case a moitie sortie de sa zone qui defile : son centre est
        // coupe.
        let clipped = it.clip.is_some_and(|z| {
            v.items()
                .iter()
                .find(|c| c.look == Look::Clip(z))
                .is_some_and(|c| !c.r.has(x, y))
        });
        if clipped {
            continue;
        }
        let got = v.hit(x, y);
        // Une pastille posee sur un bouton l'emporte : elle est dessinee
        // par-dessus. L'en-tete d'un effet porte son nom (meme controle).
        if got != it.ctl
            && !matches!(
                got,
                Ctl::MacroDot(_) | Ctl::TileDot(_) | Ctl::AxisDot(_) | Ctl::DockHandle
            )
            && !matches!(it.ctl, Ctl::FxHead(_) | Ctl::Pool(_))
        {
            panic!("{:?} at ({x}, {y}) gives {got:?}", it.ctl);
        }
    }
}

#[test]
fn gestures_become_acts() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.0);
    let mut v = View::new();
    v.layout(&d, &th, 1500.0, 820.0);
    // ROLL : un clic.
    let r = rect(&v, &Ctl::Roll);
    assert_eq!(v.event(&down(r.cx(), r.cy(), Button::Left, none())), None);
    assert!(matches!(
        v.event(&up(r.cx(), r.cy(), Button::Left, none())),
        Some(Act::Click(Ctl::Roll, _))
    ));
    // Le pad : appuyer, tirer, lacher (y vers le haut).
    let r = rect(&v, &Ctl::Pad);
    let a = v.event(&down(
        r.x + r.w * 0.25,
        r.y + r.h * 0.25,
        Button::Left,
        none(),
    ));
    assert!(
        matches!(a, Some(Act::PadPress(x, y)) if (x - 0.25).abs() < 0.01 && (y - 0.75).abs() < 0.01),
        "{a:?}"
    );
    assert!(matches!(
        v.event(&mv(r.cx(), r.cy(), none())),
        Some(Act::PadDrag(..))
    ));
    assert_eq!(
        v.event(&up(r.cx(), r.cy(), Button::Left, none())),
        Some(Act::PadRelease)
    );
    // Un bouton de parametre : glisser vers le haut = la base monte.
    let k = Ctl::Knob(pkey("{A}", 1));
    let r = rect(&v, &k);
    v.event(&down(r.cx(), r.cy(), Button::Left, none()));
    let a = v.event(&mv(r.cx(), r.cy() - 50.0, none()));
    assert!(
        matches!(a, Some(Act::Value(ref c, x)) if c == &k && (x - 0.6).abs() < 1e-6),
        "{a:?}"
    );
    assert_eq!(
        v.event(&up(r.cx(), r.cy() - 50.0, Button::Left, none())),
        Some(Act::End(k.clone()))
    );
    // Alt + glisser : la profondeur (0.6 au depart, 25 px vers le bas).
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    v.event(&down(r.cx(), r.cy(), Button::Left, alt));
    let a = v.event(&mv(r.cx(), r.cy() + 25.0, alt));
    assert!(
        matches!(a, Some(Act::Depth(_, x)) if (x - 0.5).abs() < 1e-6),
        "{a:?}"
    );
    v.event(&up(r.cx(), r.cy() + 25.0, Button::Left, alt));
    // Une pastille de source tiree sur ce bouton.
    let dot = rect(&v, &Ctl::TileDot(1));
    v.event(&down(dot.cx(), dot.cy(), Button::Left, none()));
    v.event(&mv(r.cx(), r.cy(), none()));
    assert!(v.carrying());
    let a = v.event(&up(r.cx(), r.cy(), Button::Left, none()));
    assert_eq!(a, Some(Act::Drop(1, k)));
    // Une zone tiree au clic droit sur le pad.
    let r = rect(&v, &Ctl::Pad);
    v.event(&down(
        r.x + r.w * 0.2,
        r.y + r.h * 0.2,
        Button::Right,
        none(),
    ));
    v.event(&mv(r.x + r.w * 0.6, r.y + r.h * 0.7, none()));
    let a = v.event(&up(r.x + r.w * 0.6, r.y + r.h * 0.7, Button::Right, none()));
    assert!(
        matches!(a, Some(Act::Zone(z)) if (z[0] - 0.2).abs() < 0.01 && (z[1] - 0.6).abs() < 0.01 && (z[2] - 0.3).abs() < 0.01 && (z[3] - 0.8).abs() < 0.01),
        "{a:?}"
    );
}

#[test]
fn painting_fills_skipped_steps_and_bool_lanes_toggle() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.0);
    let mut v = View::new();
    v.tab = 2;
    v.layout(&d, &th, 1500.0, 820.0);
    // Une voie a interrupteurs : le premier pas touche pose son inverse,
    // et toute la peinture pose cette valeur.
    let lane = Ctl::SeqLane(0);
    let r = rect(&v, &lane);
    let cw = r.w / 16.0;
    let first = match v.event(&down(r.x + cw * 0.5, r.cy(), Button::Left, none())) {
        Some(Act::Step(c, 0, x)) if c == lane => x,
        a => panic!("{a:?}"),
    };
    assert!(first == 0.0 || first == 1.0);
    // Une souris rapide : du pas 0 au pas 5 d'un coup, les pas 1..4 aussi.
    let a = v.event(&mv(r.x + cw * 5.5, r.y + 2.0, none()));
    assert!(matches!(a, Some(Act::Step(_, 5, x)) if x == first), "{a:?}");
    let filled: Vec<u8> = v
        .take_pending()
        .into_iter()
        .map(|a| match a {
            Act::Step(_, k, x) => {
                assert_eq!(x, first);
                k
            }
            a => panic!("{a:?}"),
        })
        .collect();
    assert_eq!(filled, vec![1, 2, 3, 4]);
    assert_eq!(
        v.event(&up(r.x + cw * 5.5, r.y + 2.0, Button::Left, none())),
        Some(Act::End(lane))
    );
}

#[test]
fn the_seam_follows_the_mouse_at_any_scale() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.5);
    let mut v = View::new();
    v.layout(&d, &th, 2200.0, 1200.0);
    let r = rect(&v, &Ctl::Seam(0));
    let w0 = v.left_w;
    v.event(&down(r.cx(), r.cy(), Button::Left, none()));
    v.event(&mv(r.cx() + 30.0, r.cy(), none()));
    // 30 pixels a l'echelle 1,5 = 20 points.
    assert!(
        (v.left_w - (w0 + 20.0)).abs() < 0.01,
        "{} -> {}",
        w0,
        v.left_w
    );
    v.event(&up(r.cx() + 30.0, r.cy(), Button::Left, none()));
}

#[test]
fn a_lost_mouse_ends_the_gesture() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.0);
    let mut v = View::new();
    v.layout(&d, &th, 1500.0, 820.0);
    let r = rect(&v, &Ctl::Pad);
    v.event(&down(r.cx(), r.cy(), Button::Left, none()));
    // Un clic droit pendant le geste ne fait rien.
    assert_eq!(v.event(&down(r.cx(), r.cy(), Button::Right, none())), None);
    assert!(v.dragging());
    // La fenetre perd la souris : le pad est relache.
    v.cancel();
    assert!(!v.dragging());
    assert_eq!(v.take_pending(), vec![Act::PadRelease]);
}

/// La vue habillee du theme COURANT de la machine (lance a la main :
/// `--ignored`) : `cp_fxc_theme_*.png`.
#[test]
#[ignore]
fn habillee_du_vrai_theme() {
    let res = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("REAPER");
    let ini = std::fs::read_to_string(res.join("reaper.ini")).unwrap_or_default();
    let get = |k: &str| {
        ini.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .map(|s| s.trim().to_string())
    };
    let theme = cp_theme::ThemeFiles::open(std::path::Path::new(&get("lastthemefn5").unwrap()))
        .expect("theme");
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let mut b = Bench::new(1600, 860);
    b.th = Theme::reaper(1.0, &|k| theme.color(k));
    let mut v = View::new();
    v.sel_slot = 1;
    b.shot(&mut v, &d, "theme_params");
    v.tab = 1;
    b.shot(&mut v, &d, "theme_soundgen");
    v.tab = 2;
    v.kb_open = true;
    b.shot(&mut v, &d, "theme_seq");
    v.tab = 0;
    v.kb_open = false;
    let mut narrow = Bench::new(1100, 760);
    narrow.th = Theme::reaper(1.0, &|k| theme.color(k));
    narrow.shot(&mut v, &d, "theme_narrow");
}

#[test]
fn a_moving_pad_repaints_only_the_pad() {
    let mut w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let th = Theme::dark(1.0);
    let mut v = View::new();
    {
        let d = data(&w, &p);
        v.layout(&d, &th, 1500.0, 820.0);
        v.layout(&d, &th, 1500.0, 820.0);
    }
    // Rien n'a change : rien a repeindre.
    assert_eq!(v.damage(), Some(vec![]));
    let pad = rect(&v, &Ctl::Pad);
    w.t.pad.x = 0.2;
    w.t.pad.y = 0.8;
    let d = data(&w, &p);
    v.layout(&d, &th, 1500.0, 820.0);
    let dmg = v.damage().expect("une partie seulement");
    let area: f32 = dmg.iter().map(|r| r.w * r.h).sum();
    assert!(!dmg.is_empty());
    // Le pad, sa carte et ses pastilles X / Y : bien moins que la fenetre.
    assert!(area < 0.2 * 1500.0 * 820.0, "{area} {dmg:?}");
    assert!(dmg.iter().any(|r| r.x <= pad.x && r.right() >= pad.right()));
}

#[test]
fn a_full_frame_costs_little() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let mut b = Bench::new(1600, 860);
    let mut v = View::new();
    // Une premiere image (les glyphes et les icones se mettent en cache).
    b.shot(&mut v, &d, "perf");
    let t0 = std::time::Instant::now();
    let n = 20;
    for _ in 0..n {
        v.layout(&d, &b.th, 1600.0, 860.0);
        let mut cv = Canvas::new(&mut b.s, &mut b.k);
        v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    eprintln!("image complete : {ms:.2} ms");
    // En test (non optimise), large : la vraie mesure est dans REAPER.
    assert!(ms < 200.0, "{ms} ms");
}

#[test]
#[ignore]
fn where_the_frame_time_goes() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let mut b = Bench::new(1600, 860);
    let mut v = View::new();
    b.shot(&mut v, &d, "perf");
    let n = 30;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        v.layout(&d, &b.th, 1600.0, 860.0);
    }
    let lay = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let mut cv = Canvas::new(&mut b.s, &mut b.k);
        v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th);
    }
    let full = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let pad = rect(&v, &Ctl::Pad);
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let mut cv = Canvas::new(&mut b.s, &mut b.k);
        cv.push_clip(pad.x - 6.0, pad.y - 6.0, pad.w + 12.0, pad.h + 12.0);
        v.draw(&mut cv, &mut b.t, &mut b.icons, &b.th);
    }
    let part = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    eprintln!(
        "mise en page {lay:.2} ms, dessin complet {full:.2} ms, dessin du pad seul {part:.2} ms, {} elements",
        v.items().len()
    );
}

#[test]
#[ignore]
fn what_costs_in_a_frame() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let mut b = Bench::new(1600, 860);
    let mut v = View::new();
    b.shot(&mut v, &d, "perf");
    let mut cv = Canvas::new(&mut b.s, &mut b.k);
    let prof = v.draw_profile(&mut cv, &mut b.t, &mut b.icons, &b.th);
    for (k, t, n) in prof {
        eprintln!("{k:>10} {t:7.2} ms  x{n}");
    }
}

#[test]
fn typing_in_the_search_repaints_it() {
    let w = world();
    let p = plan(&w.t, &chain(), &Ctx::default());
    let d = data(&w, &p);
    let th = Theme::dark(1.0);
    let mut v = View::new();
    v.layout(&d, &th, 1500.0, 820.0);
    let r = rect(&v, &Ctl::Search);
    v.event(&down(r.cx(), r.cy(), Button::Left, none()));
    v.event(&up(r.cx(), r.cy(), Button::Left, none()));
    v.layout(&d, &th, 1500.0, 820.0);
    v.event(&Ev::Char('P'));
    v.layout(&d, &th, 1500.0, 820.0);
    // « P » et « Pa » gardent les memes parametres : seul le champ change.
    v.event(&Ev::Char('a'));
    v.layout(&d, &th, 1500.0, 820.0);
    let dmg = v.damage().expect("une partie");
    assert!(
        dmg.iter().any(|d| d.x <= r.x && d.right() >= r.right()),
        "{dmg:?}"
    );
    // Ctrl+Z dans la recherche ne va pas a REAPER.
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(v.key_use(0x5A, ctrl), KeyUse::Eat);
    // AltGr (Ctrl + Alt) tape un caractere.
    let altgr = Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    };
    assert_eq!(v.key_use(0x32, altgr), KeyUse::Text);
}
