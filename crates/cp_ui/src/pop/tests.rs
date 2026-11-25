//! Le popover pilote hors de REAPER : une fausse police a chasse fixe, des
//! gestes, et le dessin dans une surface (rien ne doit paniquer, des pixels
//! changent).

use cp_gfx::text::{Font, Glyph, GlyphSource, Metrics, TextCache};
use cp_gfx::{Canvas, Scratch, Surface};

use super::*;
use crate::{Mods, Theme};

struct Fixed;

impl GlyphSource for Fixed {
    fn metrics(&mut self, f: Font) -> Metrics {
        let px = f.px as f32;
        Metrics {
            ascent: px * 0.8,
            descent: px * 0.2,
            line: px * 1.2,
        }
    }
    fn glyph(&mut self, f: Font, _ch: char) -> Option<Glyph> {
        let w = (f.px as i32 / 2).max(1);
        Some(Glyph {
            w,
            h: 2,
            left: 0,
            top: 2,
            advance: w as f32 + 1.0,
            mask: vec![255; (w * 2) as usize],
        })
    }
}

fn text() -> TextCache {
    TextCache::new(Box::new(Fixed))
}

fn m() -> Mods {
    Mods::default()
}

fn down(x: f32, y: f32) -> Ev {
    Ev::Down {
        x,
        y,
        b: Button::Left,
        dbl: false,
        mods: m(),
    }
}

fn up(x: f32, y: f32) -> Ev {
    Ev::Up {
        x,
        y,
        b: Button::Left,
        mods: m(),
    }
}

fn mv(x: f32, y: f32) -> Ev {
    Ev::Move { x, y, mods: m() }
}

fn key(k: u32) -> Ev {
    Ev::Key {
        vk: k,
        mods: m(),
        text: None,
    }
}

fn click(p: &mut Pop, r: R) -> Option<Out> {
    let (x, y) = (r.x + r.w * 0.5, r.cy());
    p.event(&mv(x, y));
    let a = p.event(&down(x, y));
    let b = p.event(&up(x, y));
    a.or(b)
}

fn tap(p: &mut Pop, h: Hit) -> Option<Out> {
    let r = rect(p, h);
    click(p, r)
}

fn rect(p: &Pop, h: Hit) -> R {
    p.hit_rect(h).unwrap_or_else(|| panic!("{h:?} absent"))
}

fn menu() -> Vec<Entry> {
    vec![
        Entry::head("FX"),
        Entry::item(1, "Open").icon(Icon::Maximize).hint("Dbl"),
        Entry::check(2, "Detached", true),
        Entry::item(3, "Off").enabled(false),
        Entry::sub(
            "After insert",
            vec![
                Entry::check(10, "None", false),
                Entry::check(11, "All", true),
            ],
        ),
        Entry::Sep,
        Entry::item(4, "Delete").danger(),
    ]
}

/// Le pixel (x, y) de la surface, sans l'alpha.
fn px(s: &Surface, x: i32, y: i32) -> u32 {
    s.get(x, y) & 0x00FF_FFFF
}

fn draw(p: &Pop, th: &Theme, t: &mut TextCache, w: i32, h: i32) -> Surface {
    let mut s = Surface::new(w, h);
    let mut k = Scratch::default();
    let mut icons = IconCache::default();
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.clear(cp_gfx::rgb(10, 10, 10));
        p.draw(&mut cv, th, t, &mut icons);
    }
    s
}

#[test]
fn a_menu_picks_on_a_click_of_its_own_not_on_the_release_that_opened_it() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::menu(7, menu(), 100.0, 80.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    let open = rect(&p, Hit::Item(0, 1));
    // Le relachement du clic droit qui a ouvert le menu, sans bouger.
    assert_eq!(p.event(&up(100.0, 80.0)), None);
    assert_eq!(click(&mut p, open), Some(Out::Pick(1)));
    // Un choix eteint ne se prend pas.
    let mut p = Pop::menu(7, menu(), 100.0, 80.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    assert!(p.hit_rect(Hit::Item(0, 3)).is_none());
    assert_eq!(p.ctx, 7);
}

#[test]
fn a_submenu_opens_on_hover_and_its_choice_comes_back() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::menu(1, menu(), 100.0, 80.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    let sub = rect(&p, Hit::Item(0, 4));
    p.event(&mv(sub.x + 10.0, sub.cy()));
    p.layout(&th, &mut t, 800.0, 600.0);
    assert_eq!(p.rects().len(), 2, "le sous-menu est ouvert");
    let all = rect(&p, Hit::Item(1, 1));
    assert!(all.x >= sub.right() - 10.0, "a droite du menu");
    assert_eq!(click(&mut p, all), Some(Out::Pick(11)));
}

#[test]
fn the_keyboard_walks_a_menu() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::menu(1, menu(), 100.0, 80.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    // Bas : « Open » ; bas : « Detached » (le titre et « Off » se sautent).
    p.event(&key(vk::DOWN));
    p.event(&key(vk::DOWN));
    assert_eq!(p.event(&key(vk::RETURN)), Some(Out::Pick(2)));
    let mut p = Pop::menu(1, menu(), 100.0, 80.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    p.event(&key(vk::UP)); // « Delete »
    p.event(&key(vk::UP)); // le sous-menu
    p.event(&key(vk::RIGHT));
    p.layout(&th, &mut t, 800.0, 600.0);
    assert_eq!(p.rects().len(), 2);
    // Echap ferme le sous-menu, puis le menu.
    assert_eq!(p.event(&key(vk::ESCAPE)), None);
    assert_eq!(p.event(&key(vk::ESCAPE)), Some(Out::Closed));
}

#[test]
fn a_click_outside_closes_and_the_menu_stays_in_the_view() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::menu(1, menu(), 790.0, 590.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    let b = p.rects()[0];
    assert!(
        b.right() <= 800.0 && b.bottom() <= 600.0 && b.x >= 0.0 && b.y >= 0.0,
        "{b:?}"
    );
    assert_eq!(p.event(&down(5.0, 5.0)), Some(Out::Closed));
    assert!(Out::Closed.closes() && !Out::Toggle(1).closes());
}

#[test]
fn a_name_is_typed_over_the_selection() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::ask(3, "Save the preset as", "Old", "Save", 50.0, 50.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    assert!(p.wants_text());
    for c in "Night".chars() {
        p.event(&Ev::Char(c));
    }
    assert_eq!(p.event(&key(vk::RETURN)), Some(Out::Text("Night".into())));
    // Vide : rien ; Annuler ferme.
    let mut p = Pop::ask(3, "Name", "", "Save", 50.0, 50.0);
    p.layout(&th, &mut t, 800.0, 600.0);
    assert_eq!(p.event(&key(vk::RETURN)), None);
    let cancel = rect(&p, Hit::Cancel);
    assert_eq!(click(&mut p, cancel), Some(Out::Closed));
    let mut c = Pop::confirm(4, "Delete this preset?", "Delete", true, 50.0, 50.0);
    c.layout(&th, &mut t, 800.0, 600.0);
    let ok = rect(&c, Hit::Ok);
    assert_eq!(click(&mut c, ok), Some(Out::Yes));
}

fn panel() -> Panel {
    Panel {
        title: "Settings".into(),
        icon: Icon::Settings,
        sections: vec![
            Section {
                label: "Parameters".into(),
                icon: Icon::ListX,
                rows: vec![
                    Row::Head("Hidden parameters".into()),
                    Row::Checks {
                        id: 1,
                        items: vec![
                            ("MIDI".into(), true, "2086".into()),
                            ("mix".into(), false, "4".into()),
                        ],
                        add: "Add a word".into(),
                        removable: true,
                    },
                    Row::Toggle {
                        id: 2,
                        label: "Show values".into(),
                        on: true,
                    },
                ],
            },
            Section {
                label: "ROLL".into(),
                icon: Icon::Dices,
                rows: vec![
                    Row::Range {
                        id: 3,
                        label: "Base span".into(),
                        lo: 0.25,
                        hi: 0.75,
                        text: "0.25 – 0.75".into(),
                    },
                    Row::Slider {
                        id: 4,
                        label: "Bypass".into(),
                        v: 0.1,
                        text: "10 %".into(),
                    },
                    Row::Seg {
                        id: 5,
                        label: "Slots".into(),
                        opts: (1..=8).map(|n| n.to_string()).collect(),
                        cur: 3,
                    },
                    Row::Step {
                        id: 6,
                        label: "How many".into(),
                        text: "4".into(),
                    },
                    Row::Chips {
                        id: 7,
                        label: "Randomize".into(),
                        chips: vec![("Shape".into(), true), ("Rate".into(), false)],
                    },
                ],
            },
        ],
    }
}

#[test]
fn the_panel_gives_back_each_setting() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::panel(9, panel(), 760.0, 40.0);
    p.layout(&th, &mut t, 1000.0, 700.0);
    assert_eq!(tap(&mut p, Hit::Check(1, 1)), Some(Out::Check(1, 1)));
    assert_eq!(tap(&mut p, Hit::Remove(1, 0)), Some(Out::Remove(1, 0)));
    assert_eq!(tap(&mut p, Hit::Toggle(2)), Some(Out::Toggle(2)));
    // Un mot tape dans la saisie, Entree l'ajoute.
    tap(&mut p, Hit::AddField(1));
    assert!(p.wants_text());
    for c in "Gain".chars() {
        p.event(&Ev::Char(c));
    }
    assert_eq!(p.event(&key(vk::RETURN)), Some(Out::Add(1, "Gain".into())));
    // Echap rend d'abord le clavier, puis ferme.
    assert_eq!(p.event(&key(vk::ESCAPE)), None);
    assert!(!p.wants_text());
    // La section ROLL.
    tap(&mut p, Hit::Rail(1));
    assert_eq!(p.section(), Some(1));
    p.layout(&th, &mut t, 1000.0, 700.0);
    assert_eq!(tap(&mut p, Hit::Seg(5, 7)), Some(Out::Seg(5, 7)));
    assert_eq!(tap(&mut p, Hit::Plus(6)), Some(Out::Step(6, 1)));
    assert_eq!(tap(&mut p, Hit::Chip(7, 1)), Some(Out::Chip(7, 1)));
    // Le curseur suit la souris, et dit quand le geste finit.
    let tr = rect(&p, Hit::Track(4));
    p.event(&down(tr.x, tr.cy()));
    assert!(matches!(
        p.event(&mv(tr.x + tr.w * 0.5, tr.cy())),
        Some(Out::Slide { id: 4, v, done: false }) if (v - 0.5).abs() < 1e-3
    ));
    assert!(
        matches!(p.event(&up(tr.right() + 50.0, tr.cy())), Some(Out::Slide { v, done: true, .. }) if v == 1.0)
    );
    // L'etendue : la poignee la plus proche (la haute), jamais sous la basse.
    let tr = rect(&p, Hit::Track(3));
    p.event(&down(tr.x + tr.w * 0.7, tr.cy()));
    let o = p.event(&mv(tr.x + tr.w * 0.1, tr.cy()));
    assert_eq!(
        o,
        Some(Out::Range {
            id: 3,
            lo: 0.25,
            hi: 0.25,
            done: false
        })
    );
    p.event(&up(tr.x, tr.cy()));
    assert_eq!(tap(&mut p, Hit::Close), Some(Out::Closed));
}

#[test]
fn the_panel_fits_a_narrow_view_and_scrolls() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut p = Pop::panel(9, panel(), 300.0, 10.0);
    p.layout(&th, &mut t, 380.0, 220.0);
    let b = p.rects()[0];
    assert!(
        b.x >= 0.0 && b.right() <= 380.0 && b.bottom() <= 220.0,
        "{b:?}"
    );
    // Le rail en icones seules.
    assert!(rect(&p, Hit::Rail(0)).w < 60.0);
    // La liste deborde : la molette la fait defiler.
    let g0 = p.stamp();
    let c = b.right() - 40.0;
    p.event(&Ev::Wheel {
        x: c,
        y: b.bottom() - 30.0,
        dx: 0.0,
        dy: -1.0,
        mods: m(),
    });
    assert!(p.stamp() > g0);
}

#[test]
fn every_kind_draws() {
    let th = Theme::dark(1.0);
    let mut t = text();
    let mut menu_p = Pop::menu(1, menu(), 40.0, 40.0);
    menu_p.layout(&th, &mut t, 600.0, 500.0);
    let sub = rect(&menu_p, Hit::Item(0, 4));
    menu_p.event(&mv(sub.x + 5.0, sub.cy()));
    menu_p.layout(&th, &mut t, 600.0, 500.0);
    let mut ask = Pop::ask(2, "Name", "Lead", "Save", 40.0, 40.0);
    ask.layout(&th, &mut t, 600.0, 500.0);
    let mut conf = Pop::confirm(3, "Delete this preset?", "Delete", true, 40.0, 40.0);
    conf.layout(&th, &mut t, 600.0, 500.0);
    let mut pan = Pop::panel(4, panel(), 560.0, 20.0);
    pan.layout(&th, &mut t, 600.0, 500.0);
    for p in [&menu_p, &ask, &conf, &pan] {
        let s = draw(p, &th, &mut t, 600, 500);
        let b = p.rects()[0];
        let (cx, cy) = ((b.x + b.w * 0.5) as i32, (b.y + 4.0) as i32);
        assert_ne!(px(&s, cx, cy), 0x000A_0A0A, "le corps est peint");
        assert_eq!(px(&s, 2, 498), 0x000A_0A0A, "rien hors du popover");
    }
}
