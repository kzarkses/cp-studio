//! La vue pilotee hors de REAPER : les vraies polices (GDI), des gestes, et
//! des planches PNG dans `%TEMP%` (cp_fxbrowser_*.png).

use cp_fxlist::{Catalog, IniTexts, Rule, Src, Store, Tab};
use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png, rgb};
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
            t: TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas"]))),
            icons: IconCache::default(),
            th: Theme::dark(1.0),
        }
    }

    fn draw(&mut self, v: &mut View, cat: &Catalog, st: &Store, ch: &Chain) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(
            &mut cv,
            &mut self.t,
            &mut self.icons,
            &self.th,
            cat,
            st,
            ch,
            None,
            w,
            h,
        );
    }

    fn shot(&mut self, v: &mut View, cat: &Catalog, st: &Store, ch: &Chain, name: &str) {
        self.draw(v, cat, st, ch);
        let p = std::env::temp_dir().join(format!("cp_fxbrowser_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }

    fn ev(
        &mut self,
        v: &mut View,
        cat: &Catalog,
        st: &mut Store,
        ch: &Chain,
        ev: Ev,
    ) -> Option<Act> {
        v.event(cat, st, ch, &self.th, &mut self.t, &ev)
    }
}

fn none() -> Mods {
    Mods::default()
}

fn click(x: f32, y: f32, mods: Mods, dbl: bool) -> [Ev; 2] {
    [
        Ev::Down {
            x,
            y,
            b: Button::Left,
            dbl,
            mods,
        },
        Ev::Up {
            x,
            y,
            b: Button::Left,
            mods,
        },
    ]
}

/// Un petit catalogue : des effets, des dossiers, des categories.
fn catalog() -> Catalog {
    let mut installed: Vec<(String, String)> = Vec::new();
    for (n, d) in [
        ("Pro-Q 3", "FabFilter"),
        ("Pro-C 2", "FabFilter"),
        ("Pro-R 2", "FabFilter"),
        ("Saturn 2", "FabFilter"),
        ("ValhallaVintageVerb", "Valhalla DSP, LLC"),
        ("ValhallaDelay", "Valhalla DSP, LLC"),
        ("Decapitator", "Soundtoys"),
        ("EchoBoy", "Soundtoys"),
        ("kHs Haas", "Kilohearts"),
        ("kHs Delay", "Kilohearts"),
        ("MTremolo", "MeldaProduction"),
        ("ReaEQ", "Cockos"),
        ("ReaComp", "Cockos"),
        ("ReaDelay", "Cockos"),
    ] {
        installed.push((format!("VST3: {n} ({d})"), String::new()));
    }
    installed.push(("VST3i: Serum (Xfer Records)".into(), String::new()));
    installed.push(("VSTi: ReaSynth (Cockos)".into(), String::new()));
    installed.push(("JS: MIDI Arpeggiator".into(), "midi/midi_arp".into()));
    installed.push((
        "CLAP: Surge XT Effects (Surge Synth Team)".into(),
        String::new(),
    ));
    let vst = "[x]
FabFilter_Pro_Q_3.vst3=0,1,Pro-Q 3 (FabFilter)
ValhallaVintageVerb.vst3=0,1,ValhallaVintageVerb (Valhalla DSP, LLC)
EchoBoy.vst3=0,1,EchoBoy (Soundtoys)
"
    .to_string();
    let folders = "[Folder0]
Item0=C:\\VST3\\FabFilter Pro-Q 3.vst3
Type0=3
Item1=C:\\VST3\\EchoBoy.vst3
Type1=3
Nb=2
[Folder1]
Item0=C:\\VST3\\ValhallaVintageVerb.vst3
Type0=3
Nb=1
[Folders]
Id0=0
Id1=1
Name0=Favorites
Name1=Reverbs
NbFolders=2
"
    .to_string();
    let tags = "[category]
FabFilter_Pro_Q_3.vst3=EQ
ValhallaVintageVerb.vst3=Reverb
EchoBoy.vst3=Delay
"
    .to_string();
    Catalog::build(
        &installed,
        &IniTexts {
            vst: vec![vst],
            clap: Vec::new(),
            jsfx: String::new(),
            folders,
            tags,
        },
        &["Master_Chain.RfxChain".into()],
    )
}

fn chain() -> Chain {
    Chain {
        track: Some("Lead synth".into()),
        color: Some(rgb(0x5B, 0xA0, 0xE0)),
        new_track: false,
        fx: vec![
            ChainFx {
                name: "VST3i: Serum (Xfer Records)".into(),
                on: true,
                detached: false,
            },
            ChainFx {
                name: "VST3: Pro-Q 3 (FabFilter)".into(),
                on: true,
                detached: false,
            },
            ChainFx {
                name: "VST3: EchoBoy (Soundtoys)".into(),
                on: false,
                detached: true,
            },
        ],
    }
}

fn setup(w: i32, h: i32) -> (Bench, View, Catalog, Store, Chain) {
    let b = Bench::new(w, h);
    let cat = catalog();
    let mut st = Store::default();
    st.tabs.push(Tab {
        name: "cool".into(),
        items: vec![
            "kHs Haas (Kilohearts)".into(),
            "kHs Delay (Kilohearts)".into(),
        ],
        rule: None,
    });
    st.tabs.push(Tab {
        name: "Delays".into(),
        items: Vec::new(),
        rule: Some(Rule {
            query: "delay".into(),
            src: "all".into(),
        }),
    });
    let mut v = View::new();
    v.refresh(&cat, &st);
    (b, v, cat, st, chain())
}

fn row_y(v: &View, b: &Bench, st: &Store, r: usize) -> f32 {
    let l = v.layout(&b.th, st, v.size.0, v.size.1);
    l.list.y + r as f32 * l.row + l.row * 0.5 - v.scroll
}

#[test]
fn planches() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.shot(&mut v, &cat, &st, &ch, "large");
    // Un survol, une recherche, une selection.
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let x = l.list.x + 80.0;
    let y = row_y(&v, &b, &st, 2);
    for e in click(x, y, none(), false) {
        b.ev(&mut v, &cat, &mut st, &ch, e);
    }
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Move {
            x,
            y: y + 22.0,
            mods: none(),
        },
    );
    b.shot(&mut v, &cat, &st, &ch, "selection");
    for c in "pro".chars() {
        b.ev(&mut v, &cat, &mut st, &ch, Ev::Char(c));
    }
    b.shot(&mut v, &cat, &st, &ch, "search");
    // Etroite : la chaine passe sous la liste ; plus etroite : la barre de
    // cote se cache.
    let (mut b, mut v, cat, st, ch) = setup(480, 560);
    b.shot(&mut v, &cat, &st, &ch, "narrow");
    let (mut b, mut v, cat, st, mut ch) = setup(360, 560);
    ch.new_track = true;
    b.shot(&mut v, &cat, &st, &ch, "tiny");
}

#[test]
fn la_recherche_filtre_et_echap_la_vide() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let all = v.rows.len();
    for c in "delay -rea".chars() {
        b.ev(&mut v, &cat, &mut st, &ch, Ev::Char(c));
    }
    let names: Vec<&str> = v
        .rows
        .iter()
        .map(|&i| cat.plugins[i].label.as_str())
        .collect();
    assert_eq!(names, vec!["EchoBoy", "kHs Delay", "ValhallaDelay"]);
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Key {
            vk: vk::ESCAPE,
            mods: none(),
            text: None,
        },
    );
    assert_eq!(v.rows.len(), all);
    assert!(v.search.text.is_empty());
}

#[test]
fn tab_passe_d_une_liste_a_l_autre() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let tab = Ev::Key {
        vk: vk::TAB,
        mods: none(),
        text: None,
    };
    assert_eq!(
        b.ev(&mut v, &cat, &mut st, &ch, tab.clone()),
        Some(Act::Changed)
    );
    assert_eq!(v.src, Src::Fav);
    assert_eq!(st.set.src, "fav");
    let favs: Vec<&str> = v
        .rows
        .iter()
        .map(|&i| cat.plugins[i].label.as_str())
        .collect();
    assert_eq!(favs, vec!["EchoBoy", "Pro-Q 3"]);
    for _ in 0..4 {
        b.ev(&mut v, &cat, &mut st, &ch, tab.clone());
    }
    assert_eq!(v.src, Src::Tab("cool".into()));
    assert_eq!(v.rows.len(), 2);
    b.ev(&mut v, &cat, &mut st, &ch, tab.clone());
    // L'onglet a regle : tous les delais.
    assert_eq!(v.src, Src::Tab("Delays".into()));
    assert_eq!(v.rows.len(), 4);
    // Et on revient au debut ; Maj+Tab recule.
    b.ev(&mut v, &cat, &mut st, &ch, tab);
    assert_eq!(v.src, Src::All);
    let back = Ev::Key {
        vk: vk::TAB,
        mods: Mods {
            shift: true,
            ..none()
        },
        text: None,
    };
    b.ev(&mut v, &cat, &mut st, &ch, back);
    assert_eq!(v.src, Src::Tab("Delays".into()));
}

#[test]
fn selection_double_clic_et_entree() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let x = l.list.x + 80.0;
    for e in click(x, row_y(&v, &b, &st, 1), none(), false) {
        b.ev(&mut v, &cat, &mut st, &ch, e);
    }
    let ctrl = Mods {
        ctrl: true,
        ..none()
    };
    for e in click(x, row_y(&v, &b, &st, 3), ctrl, false) {
        b.ev(&mut v, &cat, &mut st, &ch, e);
    }
    assert_eq!(v.selection(), vec![v.rows[1], v.rows[3]]);
    let shift = Mods {
        shift: true,
        ..none()
    };
    for e in click(x, row_y(&v, &b, &st, 5), shift, false) {
        b.ev(&mut v, &cat, &mut st, &ch, e);
    }
    assert_eq!(v.selection(), v.rows[3..=5].to_vec());
    // Entree : la selection a la chaine.
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Key {
            vk: vk::RETURN,
            mods: none(),
            text: None,
        },
    );
    assert_eq!(
        a,
        Some(Act::Add {
            plugins: v.rows[3..=5].to_vec(),
            at: None
        })
    );
    // Double-clic sur une autre rangee : elle seule.
    let y = row_y(&v, &b, &st, 7);
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(x, y, none(), false)[0].clone(),
    );
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(x, y, none(), false)[1].clone(),
    );
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(x, y, none(), true)[0].clone(),
    );
    assert_eq!(
        a,
        Some(Act::Add {
            plugins: vec![v.rows[7]],
            at: None
        })
    );
    // Les fleches.
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Key {
            vk: vk::DOWN,
            mods: none(),
            text: None,
        },
    );
    assert_eq!(v.selection(), vec![v.rows[8]]);
}

#[test]
fn glisser_vers_la_chaine_un_onglet_ou_dehors() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let c = l.chain.unwrap();
    assert!(l.chain_right);
    let x = l.list.x + 80.0;
    let y = row_y(&v, &b, &st, 2);
    let p = v.rows[2];
    let drag = |b: &mut Bench, v: &mut View, st: &mut Store, tx: f32, ty: f32| -> Option<Act> {
        b.ev(
            v,
            &cat,
            st,
            &ch,
            Ev::Down {
                x,
                y,
                b: Button::Left,
                dbl: false,
                mods: none(),
            },
        );
        b.ev(
            v,
            &cat,
            st,
            &ch,
            Ev::Move {
                x: x + 20.0,
                y: y + 20.0,
                mods: none(),
            },
        );
        b.ev(
            v,
            &cat,
            st,
            &ch,
            Ev::Move {
                x: tx,
                y: ty,
                mods: none(),
            },
        );
        b.ev(
            v,
            &cat,
            st,
            &ch,
            Ev::Up {
                x: tx,
                y: ty,
                b: Button::Left,
                mods: none(),
            },
        )
    };
    // Entre le 1er et le 2e effet de la chaine.
    let ty = c.y + l.chain_head + l.chain_row * 1.0 + 2.0;
    assert_eq!(
        drag(&mut b, &mut v, &mut st, c.x + 40.0, ty),
        Some(Act::Add {
            plugins: vec![p],
            at: Some(1)
        })
    );
    // Sous le dernier : a la fin.
    assert_eq!(
        drag(&mut b, &mut v, &mut st, c.x + 40.0, c.bottom() - 10.0),
        Some(Act::Add {
            plugins: vec![p],
            at: Some(3)
        })
    );
    // Sur l'onglet « cool ».
    b.draw(&mut v, &cat, &st, &ch);
    let (cx, cw) = v.chip_x[Src::CHIPS.len()];
    assert_eq!(
        drag(
            &mut b,
            &mut v,
            &mut st,
            cx + cw * 0.5,
            l.chips.y + l.chips.h * 0.5
        ),
        Some(Act::ToTab {
            tab: "cool".into(),
            plugins: vec![p]
        })
    );
    // Hors de la fenetre.
    assert_eq!(
        drag(&mut b, &mut v, &mut st, -50.0, 200.0),
        Some(Act::DropOut { plugins: vec![p] })
    );
}

#[test]
fn la_chaine_se_reordonne_et_ses_gestes() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let c = l.chain.unwrap();
    let ry = |i: usize| c.y + l.chain_head + l.chain_row * (i as f32 + 0.5);
    let x = c.x + 50.0;
    // Le 1er glisse sous le 3e.
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Down {
            x,
            y: ry(0),
            b: Button::Left,
            dbl: false,
            mods: none(),
        },
    );
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Move {
            x,
            y: ry(2),
            mods: none(),
        },
    );
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Up {
            x,
            y: ry(2) + l.chain_row * 0.4,
            b: Button::Left,
            mods: none(),
        },
    );
    assert_eq!(
        a,
        Some(Act::FxMove {
            from: 0,
            to: 3,
            copy: false
        })
    );
    // A sa place : rien.
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Down {
            x,
            y: ry(1),
            b: Button::Left,
            dbl: false,
            mods: none(),
        },
    );
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Move {
            x,
            y: ry(1) + 8.0,
            mods: none(),
        },
    );
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Up {
            x,
            y: ry(1) + 8.0,
            b: Button::Left,
            mods: none(),
        },
    );
    assert_eq!(a, None);
    let alt = Mods {
        alt: true,
        ..none()
    };
    assert_eq!(
        b.ev(
            &mut v,
            &cat,
            &mut st,
            &ch,
            click(x, ry(2), alt, false)[0].clone()
        ),
        Some(Act::FxDelete(2))
    );
    assert_eq!(
        b.ev(
            &mut v,
            &cat,
            &mut st,
            &ch,
            click(x, ry(1), none(), true)[0].clone()
        ),
        Some(Act::FxOpen(1))
    );
    // Les boutons du survol.
    let isz = (l.chain_row - 6.0).max(8.0);
    let del_x = c.right() - 6.0 - isz * 0.5;
    assert_eq!(v.hit(&b.th, &st, del_x, ry(0)), Hit::FxDelete(0));
    assert_eq!(v.hit(&b.th, &st, del_x - isz - 4.0, ry(0)), Hit::FxPower(0));
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(del_x - isz - 4.0, ry(0), none(), false)[0].clone(),
    );
    assert_eq!(a, Some(Act::FxToggle(0)));
}

#[test]
fn la_barre_de_cote_ouvre_ses_groupes() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let sd = l.side.unwrap();
    // Les dossiers sont ouverts : « Favorites », « Reverbs ».
    let y = |k: usize| sd.y + l.side_row * (k as f32 + 0.5);
    assert_eq!(v.hit(&b.th, &st, sd.x + 30.0, y(0)), Hit::Group(0));
    assert_eq!(v.hit(&b.th, &st, sd.x + 30.0, y(2)), Hit::Side(2));
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(sd.x + 30.0, y(2), none(), false)[0].clone(),
    );
    assert_eq!(v.src, Src::Folder("Reverbs".into()));
    assert_eq!(v.rows.len(), 1);
    // Les categories s'ouvrent.
    let k = v
        .side
        .iter()
        .position(|e| *e == SideEntry::Head(1))
        .unwrap();
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        click(sd.x + 30.0, y(k), none(), false)[0].clone(),
    );
    assert!(st.set.open_groups[1]);
    assert!(
        v.side
            .iter()
            .any(|e| *e == SideEntry::Item(Src::Cat("Delay".into()), 1))
    );
}

#[test]
fn la_molette_du_de_change_son_compte() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    let (x, y) = (l.dice.x + 5.0, l.dice.y + 5.0);
    let w = |dy: f32| Ev::Wheel {
        x,
        y,
        dx: 0.0,
        dy,
        mods: none(),
    };
    assert_eq!(b.ev(&mut v, &cat, &mut st, &ch, w(1.0)), Some(Act::Changed));
    assert_eq!(st.set.dice_count, 4);
    for _ in 0..20 {
        b.ev(&mut v, &cat, &mut st, &ch, w(1.0));
    }
    assert_eq!(st.set.dice_count, 12);
    assert_eq!(
        b.ev(
            &mut v,
            &cat,
            &mut st,
            &ch,
            click(x, y, none(), false)[0].clone()
        ),
        Some(Act::Dice)
    );
}

#[test]
fn espace_va_a_reaper_hors_de_la_recherche() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    b.draw(&mut v, &cat, &st, &ch);
    assert_eq!(v.key_use(vk::SPACE, none()), KeyUse::Text);
    let l = v.layout(&b.th, &st, 760.0, 520.0);
    for e in click(l.list.x + 60.0, row_y(&v, &b, &st, 0), none(), false) {
        b.ev(&mut v, &cat, &mut st, &ch, e);
    }
    assert_eq!(v.key_use(vk::SPACE, none()), KeyUse::NotMine);
    // Une lettre revient a la recherche.
    assert_eq!(v.key_use(0x45, none()), KeyUse::Text);
    b.ev(&mut v, &cat, &mut st, &ch, Ev::Char('e'));
    assert!(v.focus_search);
    assert_eq!(v.key_use(vk::SPACE, none()), KeyUse::Text);
}

#[test]
fn l_onglet_de_la_vue_ancree() {
    let (mut b, mut v, cat, mut st, ch) = setup(760, 520);
    v.dock.shown = true;
    b.draw(&mut v, &cat, &st, &ch);
    let r = v.tab_rect();
    assert!(r.0 < 10.0 && r.1 + r.3 == 520.0);
    assert_eq!(v.hit(&b.th, &st, 12.0, 510.0), Hit::DockHandle);
    assert_ne!(v.hit(&b.th, &st, 300.0, 1.0), Hit::DockHandle);
    b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Down {
            x: 12.0,
            y: 510.0,
            b: Button::Left,
            dbl: false,
            mods: none(),
        },
    );
    let a = b.ev(
        &mut v,
        &cat,
        &mut st,
        &ch,
        Ev::Move {
            x: 24.0,
            y: 502.0,
            mods: none(),
        },
    );
    assert_eq!(a, Some(Act::DockGrab));
}
