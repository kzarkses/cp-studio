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
