//! La vue pilotee hors de REAPER : les vraies polices (GDI), des gestes, et
//! des planches PNG dans `%TEMP%` (cp_session_*.png).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png};
use cp_session::{
    AudioSrc, Clip, Engine, Input, LaneClip, Motion, Note, RecPoll, Session, TrackInfo,
};
use cp_sys::font::GdiGlyphs;
use cp_ui::{Button, Ev, IconCache, Mods, Theme};

use super::*;

/// Un moteur qui accepte tout et ne joue rien.
struct Null {
    now: i64,
}

impl Engine for Null {
    fn now(&self) -> i64 {
        self.now
    }
    fn srate(&self) -> f64 {
        48000.0
    }
    fn has_lanes(&self) -> bool {
        true
    }
    fn ports(&self) -> u32 {
        128
    }
    fn route(&mut self, _: u32, _: &str) -> bool {
        true
    }
    fn lane_play(&mut self, _: u32, _: &LaneClip, _: f64, _: f64, _: i64) -> bool {
        true
    }
    fn lane_update(&mut self, _: u32, _: &LaneClip) -> bool {
        true
    }
    fn lane_stop(&mut self, _: u32, _: i64) -> bool {
        true
    }
    fn lane_cancel(&mut self, _: u32) {}
    fn lane_rate(&mut self, _: u32, _: f64, _: i64) {}
    fn audio_src(&mut self, _: &str) -> Option<Result<AudioSrc, String>> {
        Some(Ok(AudioSrc {
            clip: 1,
            frames: 96000,
            srate: 48000.0,
            file_bpm: Some(120.0),
        }))
    }
    fn voice_play(
        &mut self,
        _: u32,
        _: &AudioSrc,
        _: i64,
        _: i64,
        _: bool,
        _: f64,
        _: f64,
        _: i64,
    ) -> Option<u32> {
        Some(1)
    }
    fn voice_stop(&mut self, _: u32, _: Option<i64>, _: f64) {}
    fn voice_keep(&mut self, _: u32) {}
    fn voice_rate(&mut self, _: u32, _: f64) {}
    fn midi_rec_start(&mut self, _: i32, _: i64) -> u32 {
        1
    }
    fn midi_rec_stop(&mut self, _: u32, _: i64) {}
    fn midi_rec_take(&mut self, _: u32) -> Option<Vec<(i64, [u8; 3])>> {
        None
    }
    fn audio_rec_start(&mut self, _: i32, _: i32, _: i64) -> u32 {
        1
    }
    fn audio_rec_stop(&mut self, _: u32, _: i64) {}
    fn audio_rec_poll(&mut self, _: u32) -> RecPoll {
        RecPoll::Recording
    }
    fn save_take(&mut self, _: u32, _: &str) -> Option<String> {
        None
    }
}

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

    fn draw(&mut self, v: &mut View, ses: &Session, strips: &[Strip], now: i64) {
        let (w, h) = (self.s.w as f32, self.s.h as f32);
        let mut cv = Canvas::new(&mut self.s, &mut self.k);
        v.draw(
            &mut cv,
            &mut self.t,
            &mut self.icons,
            &self.th,
            ses,
            strips,
            now,
            0.0,
            None,
            w,
            h,
        );
    }

    fn shot(&mut self, v: &mut View, ses: &Session, strips: &[Strip], now: i64, name: &str) {
        self.draw(v, ses, strips, now);
        let p = std::env::temp_dir().join(format!("cp_session_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
    }
}

fn track(g: &str, name: &str, input: Input, color: Option<(u8, u8, u8)>) -> TrackInfo {
    TrackInfo {
        guid: g.into(),
        name: name.into(),
        color,
        armed: false,
        input,
        instrument: false,
        sound: None,
    }
}

fn riff(n: usize) -> Vec<Note> {
    (0..n)
        .map(|k| Note {
            start: k as f64 * 0.5,
            len: 0.4,
            pitch: 48 + (k % 7) as u8,
            vel: 100,
            chan: 0,
            prob: 100,
        })
        .collect()
}

fn session(e: &mut Null) -> (Session, Vec<Strip>) {
    let mut s = Session::new();
    s.set_tracks(
        e,
        vec![
            track("A", "Drums", Input::Audio(0, 1), Some((0xD0, 0x60, 0x50))),
            track("B", "Bass", Input::Midi(-1), Some((0x50, 0x90, 0xD8))),
            track("C", "Keys", Input::Midi(-1), None),
            track("D", "Guitar", Input::Audio(2, -1), Some((0x6C, 0xC0, 0x5A))),
        ],
    );
    s.sync_clock(e, 0, 0.0, 24000.0, 4.0, false);
    let mut c = Clip::audio(s.fresh_id(), "C:/loops/Beat 120bpm.wav");
    c.bars = 2.0;
    s.put(0, 0, c);
    let mut c = Clip::audio(s.fresh_id(), "C:/loops/Fill.wav");
    c.bars = 1.0;
    c.color = Some(2);
    s.put(0, 1, c);
    for k in 0..3 {
        let mut c = Clip::midi(s.fresh_id(), 1.0 + k as f64, riff(6 + 2 * k));
        c.name = format!("Bass {}", k + 1);
        s.put(1, k, c);
    }
    let mut c = Clip::midi(s.fresh_id(), 4.0, riff(12));
    c.name = "Chords".into();
    c.color = Some(7);
    s.put(2, 0, c);
    s.cols[3].track.armed = true;
    s.cols[1].motion = Motion::MarchWrap;
    let strips = vec![
        Strip {
            vol: 1.0,
            pan: 0.0,
            input: "In 1/2".into(),
            meter: [0.5, 0.45],
            fx: vec![("ReaComp".into(), true), ("ReaEQ".into(), false)],
            ..Default::default()
        },
        Strip {
            vol: 0.5,
            pan: -0.3,
            input: "MIDI All".into(),
            midi_out: "Hydrasynth".into(),
            monitor: 1,
            meter: [0.2, 0.2],
            fx: vec![("Vital".into(), true)],
            ..Default::default()
        },
        Strip {
            vol: 0.8,
            pan: 0.4,
            mute: true,
            input: "MIDI All".into(),
            auto: 4,
            ..Default::default()
        },
        Strip {
            vol: 1.2,
            solo: true,
            input: "In 3".into(),
            monitor: 2,
            meter: [0.9, 0.0],
            ..Default::default()
        },
    ];
    (s, strips)
}

fn center(v: &View, ses: &Session, th: &Theme, want: Hit) -> (f32, f32) {
    let (w, h) = v.size;
    let mut y = 0.0;
    while y < h {
        let mut x = 0.0;
        while x < w {
            if v.hit(ses, th, x, y) == want {
                return (x + 3.0, y + 3.0);
            }
            x += 2.0;
        }
        y += 2.0;
    }
    panic!("{want:?} introuvable");
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

fn up(x: f32, y: f32) -> Ev {
    Ev::Up {
        x,
        y,
        b: Button::Left,
        mods: Mods::default(),
    }
}

#[test]
fn la_grille_au_repos_et_en_jeu() {
    let mut e = Null { now: 0 };
    let (mut ses, strips) = session(&mut e);
    let mut v = View::new();
    let mut b = Bench::new(620, 640);
    b.shot(&mut v, &ses, &strips, 0, "01_au_repos");
    // Deux cases lancees, une qui attend, une prise qui attend.
    ses.click(&mut e, 0, 0);
    ses.click(&mut e, 1, 1);
    e.now = 30_000;
    ses.tick(&mut e);
    ses.click(&mut e, 2, 0);
    ses.record(&mut e, 3, 0);
    e.now = 60_000;
    ses.tick(&mut e);
    ses.sel = (1, 1);
    b.shot(&mut v, &ses, &strips, 60_000, "02_en_jeu");
}

#[test]
fn les_gestes_d_une_case() {
    let mut e = Null { now: 0 };
    let (ses, strips) = session(&mut e);
    let mut v = View::new();
    let mut b = Bench::new(620, 640);
    b.draw(&mut v, &ses, &strips, 0);
    let th = Theme::dark(1.0);
    let m = Mods::default();
    // La bande gauche lance.
    let (x, y) = center(
        &v,
        &ses,
        &th,
        Hit::Cell {
            col: 1,
            scene: 0,
            strip: true,
        },
    );
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::Click { col: 1, scene: 0 })
    );
    // La bande d'une case vide d'une piste armee enregistre ; pas armee, un
    // message.
    let (x, y) = center(
        &v,
        &ses,
        &th,
        Hit::Cell {
            col: 3,
            scene: 2,
            strip: true,
        },
    );
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::Record { col: 3, scene: 2 })
    );
    let (x, y) = center(
        &v,
        &ses,
        &th,
        Hit::Cell {
            col: 2,
            scene: 3,
            strip: true,
        },
    );
    assert!(matches!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::Hint(_))
    ));
    // Le contenu : un clic ouvre l'editeur.
    let (x, y) = center(
        &v,
        &ses,
        &th,
        Hit::Cell {
            col: 1,
            scene: 0,
            strip: false,
        },
    );
    v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Left, m));
    assert_eq!(
        v.event(&ses, &strips, &th, &up(x + 20.0, y)),
        Some(Act::Open { col: 1, scene: 0 })
    );
    // Glisser vers une autre case : deplacer ; avec Ctrl : copier.
    let (tx, ty) = center(
        &v,
        &ses,
        &th,
        Hit::Cell {
            col: 2,
            scene: 2,
            strip: false,
        },
    );
    v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Left, m));
    v.event(
        &ses,
        &strips,
        &th,
        &Ev::Move {
            x: tx + 20.0,
            y: ty,
            mods: m,
        },
    );
    assert!(v.cell_drag().is_some());
    b.shot(&mut v, &ses, &strips, 0, "03_glisser");
    assert_eq!(
        v.event(&ses, &strips, &th, &up(tx + 20.0, ty)),
        Some(Act::Move {
            from: (1, 0),
            to: (2, 2),
            copy: false
        })
    );
    let ctrl = Mods {
        ctrl: true,
        ..Default::default()
    };
    v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Left, ctrl));
    v.event(
        &ses,
        &strips,
        &th,
        &Ev::Move {
            x: tx + 20.0,
            y: ty,
            mods: ctrl,
        },
    );
    assert!(matches!(
        v.event(&ses, &strips, &th, &up(tx + 20.0, ty)),
        Some(Act::Move { copy: true, .. })
    ));
    // Alt+clic efface ; clic droit : le menu.
    let alt = Mods {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Left, alt)),
        Some(Act::Erase { col: 1, scene: 0 })
    );
    assert!(matches!(
        v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Right, m)),
        Some(Act::CellMenu {
            col: 1,
            scene: 0,
            ..
        })
    ));
    // Glisser hors de la fenetre : vers l'arrangement.
    v.event(&ses, &strips, &th, &down(x + 20.0, y, Button::Left, m));
    v.event(
        &ses,
        &strips,
        &th,
        &Ev::Move {
            x: -40.0,
            y,
            mods: m,
        },
    );
    assert!(matches!(
        v.event(&ses, &strips, &th, &up(-40.0, y)),
        Some(Act::DropOut {
            col: 1,
            scene: 0,
            ..
        })
    ));
}

#[test]
fn scenes_arrets_en_tetes_et_barre() {
    let mut e = Null { now: 0 };
    let (ses, strips) = session(&mut e);
    let mut v = View::new();
    let mut b = Bench::new(620, 640);
    b.draw(&mut v, &ses, &strips, 0);
    let th = Theme::dark(1.0);
    let m = Mods::default();
    let (x, y) = center(&v, &ses, &th, Hit::Scene(2));
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::Scene {
            scene: 2,
            keep: false
        })
    );
    let (x, y) = center(&v, &ses, &th, Hit::StopCol(1));
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::StopCol(1))
    );
    let (x, y) = center(&v, &ses, &th, Hit::StopAll);
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::StopAll)
    );
    let (x, y) = center(&v, &ses, &th, Hit::Arm(2));
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::Arm { col: 2, add: false })
    );
    let (x, y) = center(&v, &ses, &th, Hit::Ghost);
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::NewTrack)
    );
    let (x, y) = center(&v, &ses, &th, Hit::Tool(Tool::Clock));
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::ToggleClock)
    );
    // Un en-tete glisse sur un autre : la piste se deplace.
    let (x, y) = center(&v, &ses, &th, Hit::Header(0));
    let (x2, _) = center(&v, &ses, &th, Hit::Header(2));
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        Some(Act::SelectTrack(0))
    );
    v.event(&ses, &strips, &th, &Ev::Move { x: x2, y, mods: m });
    assert_eq!(
        v.event(&ses, &strips, &th, &up(x2, y)),
        Some(Act::MoveTrack { from: 0, to: 2 })
    );
    // Le fader suit la souris, et le geste fini se dit.
    let (x, y) = center(&v, &ses, &th, Hit::Strip(1, Part::Fader));
    v.event(&ses, &strips, &th, &down(x, y + 10.0, Button::Left, m));
    let a = v.event(
        &ses,
        &strips,
        &th,
        &Ev::Move {
            x,
            y: y - 10.0,
            mods: m,
        },
    );
    let Some(Act::SetVol {
        col: 1,
        vol,
        done: false,
    }) = a
    else {
        panic!("{a:?}")
    };
    assert!(vol > 0.5);
    assert!(matches!(
        v.event(&ses, &strips, &th, &up(x, y - 10.0)),
        Some(Act::SetVol {
            col: 1,
            done: true,
            ..
        })
    ));
    // Chaque chose survolee a un nom.
    for h in [
        Hit::Tool(Tool::Quant),
        Hit::Header(1),
        Hit::Cell {
            col: 1,
            scene: 0,
            strip: false,
        },
        Hit::Scene(0),
        Hit::Strip(0, Part::Mute),
    ] {
        assert!(v.label(&ses, h).is_some(), "{h:?}");
    }
}

#[test]
fn le_clavier() {
    let mut e = Null { now: 0 };
    let (mut ses, strips) = session(&mut e);
    let mut v = View::new();
    let mut b = Bench::new(620, 640);
    b.draw(&mut v, &ses, &strips, 0);
    let th = Theme::dark(1.0);
    let key = |v: &mut View, ses: &Session, vk: u32, m: Mods| v.key(ses, &th, vk, m);
    let m = Mods::default();
    use cp_ui::vk::*;
    assert_eq!(
        key(&mut v, &ses, RIGHT, m),
        Some(Act::Select { col: 1, scene: 0 })
    );
    ses.sel = (1, 0);
    assert_eq!(
        key(&mut v, &ses, RETURN, m),
        Some(Act::Click { col: 1, scene: 0 })
    );
    ses.sel = (1, 5);
    assert_eq!(key(&mut v, &ses, RETURN, m), Some(Act::StopCol(1)));
    assert_eq!(
        key(&mut v, &ses, RETURN, Mods { shift: true, ..m }),
        Some(Act::Scene {
            scene: 5,
            keep: false
        })
    );
    assert_eq!(
        key(&mut v, &ses, NEXT, m),
        Some(Act::Select { col: 1, scene: 2 })
    );
    assert_eq!(
        key(&mut v, &ses, DELETE, m),
        Some(Act::Erase { col: 1, scene: 5 })
    );
    assert_eq!(
        key(&mut v, &ses, 0x5A, Mods { ctrl: true, ..m }),
        Some(Act::UndoErase)
    );
    assert_eq!(
        key(
            &mut v,
            &ses,
            0x49,
            Mods {
                ctrl: true,
                shift: true,
                ..m
            }
        ),
        Some(Act::Capture)
    );
    // Espace n'est pas a la Session : REAPER le garde.
    assert_eq!(key(&mut v, &ses, SPACE, m), None);
}

#[test]
fn beaucoup_de_colonnes_defilent() {
    let mut e = Null { now: 0 };
    let mut ses = Session::new();
    let tracks: Vec<TrackInfo> = (0..20)
        .map(|k| {
            track(
                &format!("T{k}"),
                &format!("Track {}", k + 1),
                Input::Midi(-1),
                None,
            )
        })
        .collect();
    ses.set_tracks(&mut e, tracks);
    assert_eq!(ses.cols[19].port, 20);
    let mut v = View::new();
    let mut b = Bench::new(620, 480);
    b.draw(&mut v, &ses, &[], 0);
    let th = Theme::dark(1.0);
    for _ in 0..30 {
        v.event(
            &ses,
            &[],
            &th,
            &Ev::Wheel {
                x: 300.0,
                y: 100.0,
                dx: 0.0,
                dy: -1.0,
                mods: Mods::default(),
            },
        );
    }
    let l = v.layout(&th, 620.0, 480.0);
    assert_eq!(v.scroll_col, 21 - l.cols_vis);
    b.shot(&mut v, &ses, &[], 0, "04_defilement");
}

/// L'onglet de la vue ancree : en bas a gauche, sur la ligne d'etat (les
/// outils ne bougent pas) ; un clic ne fait rien, un glisser passe la main a
/// REAPER, un clic droit ouvre le menu de la vue.
#[test]
fn l_onglet_de_la_vue_ancree() {
    let mut e = Null { now: 0 };
    let (ses, strips) = session(&mut e);
    let mut v = View::new();
    let mut b = Bench::new(620, 640);
    let th = Theme::dark(1.0);
    let m = Mods::default();
    b.draw(&mut v, &ses, &strips, 0);
    let (tx, _) = center(&v, &ses, &th, Hit::Tool(Tool::Clock));
    let (x, y) = (12.0, 630.0);
    assert_ne!(v.hit(&ses, &th, x, y), Hit::DockHandle);
    v.dock.shown = true;
    b.draw(&mut v, &ses, &strips, 0);
    let r = v.tab_rect();
    assert!(r.0 < 10.0 && r.1 + r.3 == 640.0);
    assert_eq!(v.hit(&ses, &th, x, y), Hit::DockHandle);
    assert!(v.on_tab(x, y) && !v.on_tab(300.0, 1.0));
    assert_eq!(center(&v, &ses, &th, Hit::Tool(Tool::Clock)).0, tx);
    assert!(
        v.label(&ses, Hit::DockHandle)
            .unwrap()
            .contains("another docker")
    );
    // Un clic : rien.
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Left, m)),
        None
    );
    let mv =
        |v: &mut View, x: f32, y: f32| v.event(&ses, &strips, &th, &Ev::Move { x, y, mods: m });
    assert_eq!(mv(&mut v, x + 2.0, y), None);
    assert_eq!(v.event(&ses, &strips, &th, &up(x + 2.0, y)), None);
    // Un glisser : une fois.
    v.event(&ses, &strips, &th, &down(x, y, Button::Left, m));
    assert_eq!(mv(&mut v, x + 12.0, y + 6.0), Some(Act::DockGrab));
    assert_eq!(mv(&mut v, x + 40.0, y + 30.0), None);
    assert!(!v.dragging());
    // Le clic droit : le menu de la vue.
    assert_eq!(
        v.event(&ses, &strips, &th, &down(x, y, Button::Right, m)),
        Some(Act::ViewMenu { x, y })
    );
}
