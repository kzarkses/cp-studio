//! Le transport pilote hors de REAPER : les vraies polices (GDI), des gestes,
//! et des planches PNG dans `%TEMP%` (cp_transport_*.png).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png, rgba};
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

    fn draw(&mut self, t: &mut Transport) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        t.draw(&mut cv, &mut self.t, &mut self.icons, &self.th, None, w, h);
    }

    fn shot(&mut self, t: &mut Transport, name: &str) {
        self.draw(t);
        let p = std::env::temp_dir().join(format!("cp_transport_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

fn center(t: &Transport, h: Hit) -> (f32, f32) {
    let (x, y, w, hh) = t.rect_of(h).unwrap_or_else(|| panic!("{h:?} absent"));
    (x + w * 0.5, y + hh * 0.5)
}

fn down(x: f32, y: f32, b: Button, dbl: bool) -> Ev {
    Ev::Down {
        x,
        y,
        b,
        dbl,
        mods: Mods::default(),
    }
}

fn up(x: f32, y: f32) -> Ev {
    Ev::Up {
        x,
        y,
        b: Button::Left,
        mods: Mods::default(),
    }
}

fn sample() -> Transport {
    let mut t = Transport::new();
    t.st = Status {
        play: PLAYING,
        beats: "17.3.52".into(),
        time: "0:33.512".into(),
        bpm: 112.0,
        sig: (4, 4),
        rate: 1.0,
        repeat: true,
        metronome: true,
    };
    let mut img = Surface::new(90, 30);
    for y in 4..26 {
        for x in 4..26 {
            img.px[(y * 90 + x) as usize] = 0xFF60A0E0;
        }
    }
    t.items = vec![
        Item {
            id: "_RSdbf6".into(),
            name: "Custom: BirdBird_Global Sampler.lua".into(),
            glyph: Glyph::Image(fit_icon(&img, 20)),
            on: false,
            open: false,
        },
        Item {
            id: "_CPSTUDIO_GUITARTAB".into(),
            name: "CP Studio: Guitar Tab".into(),
            glyph: Glyph::Icon(Icon::Guitar),
            on: false,
            open: true,
        },
        Item {
            id: "40001".into(),
            name: "Track: Insert new track".into(),
            glyph: Glyph::Text(initials("Track: Insert new track")),
            on: false,
            open: false,
        },
    ];
    t.views = vec![
        ViewTab {
            name: "Arrange".into(),
            active: true,
            open: false,
        },
        ViewTab {
            name: "Browser".into(),
            active: false,
            open: true,
        },
    ];
    t
}

#[test]
fn la_barre_se_dessine_et_chaque_cible_a_sa_place() {
    let mut b = Bench::new(2560, 36);
    let mut t = sample();
    b.shot(&mut t, "playing");
    // Chaque cible existe, sans chevauchement, dans la barre.
    let all = [
        Hit::Start,
        Hit::Stop,
        Hit::Play,
        Hit::Pause,
        Hit::Record,
        Hit::Repeat,
        Hit::Counter,
        Hit::Bpm,
        Hit::Sig,
        Hit::Tap,
        Hit::Metronome,
        Hit::Rate,
        Hit::Item(0),
        Hit::Item(1),
        Hit::Item(2),
        Hit::Add,
        Hit::View(0),
        Hit::View(1),
    ];
    let rects: Vec<_> = all.iter().map(|&h| (h, t.rect_of(h).unwrap())).collect();
    for (i, (ha, a)) in rects.iter().enumerate() {
        assert!(
            a.0 >= 0.0 && a.0 + a.2 <= 2560.0 && a.1 >= 0.0 && a.1 + a.3 <= 36.0,
            "{ha:?} hors de la barre"
        );
        for (hb, bb) in rects.iter().skip(i + 1) {
            let disjoint = a.0 + a.2 <= bb.0 || bb.0 + bb.2 <= a.0;
            assert!(disjoint, "{ha:?} chevauche {hb:?}");
        }
        assert_eq!(t.hit(a.0 + a.2 * 0.5, a.1 + a.3 * 0.5), *ha);
    }
    // Le selecteur est colle a droite.
    let v = t.rect_of(Hit::View(1)).unwrap();
    assert!(v.0 + v.2 > 2540.0);
}

#[test]
fn le_transport_part_a_l_appui_les_icones_au_relachement() {
    let mut b = Bench::new(1600, 36);
    let mut t = sample();
    b.draw(&mut t);
    let (x, y) = center(&t, Hit::Play);
    assert_eq!(t.event(&down(x, y, Button::Left, false)), Some(Act::Play));
    assert_eq!(t.event(&up(x, y)), None);
    let (x, y) = center(&t, Hit::Item(1));
    assert_eq!(t.event(&down(x, y, Button::Left, false)), None);
    assert_eq!(t.event(&up(x, y)), Some(Act::Item(1)));
    // Un appui sur une icone relache ailleurs ne lance rien.
    let (x2, y2) = center(&t, Hit::Item(2));
    t.event(&down(x, y, Button::Left, false));
    assert_eq!(t.event(&up(x2, y2)), None);
    // Les vues : un clic, puis le clic droit pour la place.
    let (x, y) = center(&t, Hit::View(1));
    t.event(&down(x, y, Button::Left, false));
    assert_eq!(t.event(&up(x, y)), Some(Act::View(1)));
    assert_eq!(
        t.event(&down(x, y, Button::Right, false)),
        Some(Act::ViewMenu { i: 1, x, y })
    );
    let (x, y) = center(&t, Hit::Item(0));
    assert_eq!(
        t.event(&down(x, y, Button::Right, false)),
        Some(Act::ItemMenu { i: 0, x, y })
    );
    let (x, y) = center(&t, Hit::Add);
    t.event(&down(x, y, Button::Left, false));
    assert_eq!(t.event(&up(x, y)), Some(Act::AddItem));
}

#[test]
fn le_tempo_se_glisse_se_tourne_et_se_tape() {
    let mut b = Bench::new(1600, 36);
    let mut t = sample();
    b.draw(&mut t);
    let (x, y) = center(&t, Hit::Bpm);
    assert_eq!(t.event(&down(x, y, Button::Left, false)), None);
    assert!(t.dragging());
    // 8 px vers le haut : +2 BPM (0,25 par pixel).
    let a = t.event(&Ev::Move {
        x,
        y: y - 8.0,
        mods: Mods::default(),
    });
    assert_eq!(
        a,
        Some(Act::SetBpm {
            bpm: 114.0,
            done: false
        })
    );
    let a = t.event(&up(x, y - 8.0));
    assert_eq!(
        a,
        Some(Act::SetBpm {
            bpm: 114.0,
            done: true
        })
    );
    assert!(!t.dragging());
    // Un clic sans bouger ne change rien.
    t.event(&down(x, y, Button::Left, false));
    assert_eq!(t.event(&up(x, y)), None);
    // La molette : un BPM par cran, un dixieme avec Maj.
    let w = |dy: f32, shift: bool| Ev::Wheel {
        x,
        y,
        dx: 0.0,
        dy,
        mods: Mods {
            shift,
            ..Mods::default()
        },
    };
    assert_eq!(
        t.event(&w(-1.0, false)),
        Some(Act::SetBpm {
            bpm: 111.0,
            done: true
        })
    );
    match t.event(&w(1.0, true)) {
        Some(Act::SetBpm { bpm, done: true }) => assert!((bpm - 112.1).abs() < 1e-9),
        a => panic!("{a:?}"),
    }
    // Le double-clic ouvre la saisie.
    assert_eq!(t.event(&down(x, y, Button::Left, true)), Some(Act::EditBpm));
    // La vitesse : double-clic = 1.0.
    let (x, y) = center(&t, Hit::Rate);
    assert_eq!(
        t.event(&down(x, y, Button::Left, true)),
        Some(Act::SetRate {
            rate: 1.0,
            done: true
        })
    );
}

#[test]
fn les_initiales_et_le_tempo_s_ecrivent_court() {
    assert_eq!(initials("Custom: BirdBird_Global Sampler.lua"), "GS");
    assert_eq!(initials("Track: Insert new track"), "IN");
    assert_eq!(initials("Metronome"), "ME");
    assert_eq!(fmt_bpm(120.0), "120");
    assert_eq!(fmt_bpm(120.5), "120.5");
    assert_eq!(fmt_bpm(97.254), "97.25");
}

#[test]
fn une_icone_de_reaper_garde_sa_premiere_vignette() {
    // Trois vignettes de 30 px : rouge, vert, bleu. Seule la rouge reste.
    let mut s = Surface::new(90, 30);
    for y in 0..30 {
        for x in 0..90 {
            s.px[(y * 90 + x) as usize] = match x / 30 {
                0 => 0xFFFF0000,
                1 => 0xFF00FF00,
                _ => 0xFF0000FF,
            };
        }
    }
    let f = fit_icon(&s, 20);
    assert_eq!((f.w, f.h), (20, 20));
    for &p in &f.px {
        assert_eq!(p, 0xFFFF0000);
    }
    // Une image carree plus petite que la taille reste a sa taille, centree.
    let small = Surface::new(10, 10);
    let f = fit_icon(&small, 20);
    assert_eq!((f.w, f.h), (20, 20));
}

#[test]
fn arrete_et_en_enregistrement() {
    let mut b = Bench::new(2560, 36);
    let mut t = sample();
    t.st.play = 0;
    t.st.repeat = false;
    t.st.metronome = false;
    b.shot(&mut t, "stopped");
    t.st.play = PLAYING | RECORDING;
    t.st.rate = 1.25;
    t.views[0].active = false;
    t.views[1].active = true;
    b.shot(&mut t, "recording");
    // Une barre etroite : les icones cedent avant le selecteur.
    let mut b = Bench::new(900, 36);
    b.shot(&mut t, "narrow");
    assert!(t.rect_of(Hit::View(1)).is_some());
    let _ = rgba(0, 0, 0, 0);
}

/// Le transport habille du theme COURANT de la machine (lance a la main :
/// `--ignored`), a 100 % et 200 %, dans tous ses etats : planches
/// `cp_transport_skin_*.png`.
#[test]
#[ignore]
fn habille_du_vrai_theme() {
    let res = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("REAPER");
    let ini = std::fs::read_to_string(res.join("reaper.ini")).unwrap_or_default();
    let path = ini
        .lines()
        .find_map(|l| l.strip_prefix("lastthemefn5="))
        .unwrap()
        .trim()
        .to_string();
    let theme = cp_theme::ThemeFiles::open(std::path::Path::new(&path)).expect("theme");
    let face = theme
        .font_face("trans_font")
        .unwrap_or_else(|| "Segoe UI".into());
    let icons_dir = res.join("Data").join("toolbar_icons");
    let tools = cp_theme::ThemeFiles::dir(&icons_dir);
    for scale in [1.0f32, 2.0] {
        let sk = Skin::build(&theme, scale, &|k| theme.color(k), 2, 1);
        let w = (1500.0 * scale) as i32;
        let h = (36.0 * scale) as i32;
        let mut b = Bench::new(w, h * 3);
        b.th = Theme::dark(scale);
        b.t = TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas", &face])));
        let mut t = Transport::new();
        let gs = theme
            .image("toolbar_misc_guitar", scale)
            .or_else(|| tools.image("toolbar_misc_guitar", scale))
            .map(|(i, s)| cp_theme::Frames::toolbar(&i).rescaled(s, scale));
        let ai = tools
            .image(
                "AI_Function_Sample_View_Show_SampleTrack_Waveform_DF",
                scale,
            )
            .map(|(i, s)| cp_theme::Frames::toolbar(&i).rescaled(s, scale));
        t.items = vec![
            Item {
                id: "a".into(),
                name: "Global Sampler".into(),
                glyph: ai.map_or(Glyph::Text("GS".into()), Glyph::Frames),
                on: false,
                open: false,
            },
            Item {
                id: "b".into(),
                name: "Guitar Tab".into(),
                glyph: gs.map_or(Glyph::Icon(Icon::Guitar), Glyph::Frames),
                on: false,
                open: false,
            },
            Item {
                id: "c".into(),
                name: "Lucide".into(),
                glyph: Glyph::Icon(Icon::Guitar),
                on: false,
                open: false,
            },
            Item {
                id: "d".into(),
                name: "Some Script".into(),
                glyph: Glyph::Text("SS".into()),
                on: false,
                open: false,
            },
        ];
        t.views = vec![
            ViewTab {
                name: "Arrange".into(),
                active: true,
                open: false,
            },
            ViewTab {
                name: "Browser".into(),
                active: false,
                open: true,
            },
        ];
        // Trois bandes : a l'arret ; en lecture (boucle, metronome, survol) ;
        // en enregistrement, pause, vitesse changee, icone allumee.
        let mut full = Surface::new(w, h * 3);
        for band in 0..3 {
            let mut st = Status {
                beats: "2.1.00".into(),
                time: "0:02.142".into(),
                bpm: 112.0,
                ..Status::default()
            };
            match band {
                1 => {
                    st.play = PLAYING;
                    st.repeat = true;
                    st.metronome = true;
                }
                2 => {
                    st.play = RECORDING | PLAYING | PAUSED;
                    st.rate = 1.25;
                    t.items[1].on = true;
                }
                _ => {}
            }
            t.set_status(st);
            let mut s = Surface::new(w, h);
            {
                let mut k = Scratch::default();
                let mut cv = Canvas::new(&mut s, &mut k);
                t.draw(
                    &mut cv,
                    &mut b.t,
                    &mut b.icons,
                    &b.th,
                    Some(&sk),
                    w as f32,
                    h as f32,
                );
            }
            if band == 1 {
                let (x, y) = center(&t, Hit::Item(0));
                t.event(&Ev::Move {
                    x,
                    y,
                    mods: Mods::default(),
                });
                let mut k = Scratch::default();
                let mut cv = Canvas::new(&mut s, &mut k);
                t.draw(
                    &mut cv,
                    &mut b.t,
                    &mut b.icons,
                    &b.th,
                    Some(&sk),
                    w as f32,
                    h as f32,
                );
            }
            for y in 0..h {
                let o = ((band * h + y) * w) as usize;
                full.px[o..o + w as usize]
                    .copy_from_slice(&s.px[(y * w) as usize..((y + 1) * w) as usize]);
            }
        }
        let p =
            std::env::temp_dir().join(format!("cp_transport_skin_{}.png", (scale * 100.0) as i32));
        std::fs::write(&p, png::encode(&full)).unwrap();
        println!("{}", p.display());
    }
}

#[test]
fn l_accordeur_dans_le_compteur_ou_a_sa_place() {
    let mut b = Bench::new(1600, 36);
    let mut t = sample();
    let a = TunerView {
        note: "A".into(),
        octave: 2,
        cents: -4.0,
        freq: 109.7,
        string: "string 5".into(),
    };
    // A l'arret, une note entendue : le compteur devient l'accordeur.
    t.st.play = 0;
    t.tuner = Some(a.clone());
    assert!(t.tuner_in_clock());
    b.shot(&mut t, "tuner_clock");
    // En lecture, le compteur revient.
    t.st.play = PLAYING;
    assert!(!t.tuner_in_clock());
    // A sa place : toujours la, au centre, meme sans note.
    t.tuner_mode = TunerMode::Slot;
    t.st.play = 0;
    t.tuner = None;
    b.draw(&mut t);
    let (x, _) = center(&t, Hit::Tuner);
    assert!(x > 500.0 && x < 1300.0, "au centre : {x}");
    t.tuner = Some(TunerView { cents: 1.0, ..a });
    b.shot(&mut t, "tuner_slot");
    assert!(t.label(Hit::Tuner).is_some());
    assert_eq!(TunerMode::from_key(TunerMode::Slot.key()), TunerMode::Slot);
    assert_eq!(TunerMode::from_key("?"), TunerMode::Clock);
}
