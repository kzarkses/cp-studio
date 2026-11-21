//! Le navigateur pilote pour de vrai : un faux arbre de fichiers, le VRAI
//! moteur audio (sans REAPER), les vraies polices (GDI). Les vues sont
//! ecrites en PNG dans `%TEMP%` pour etre regardees.

use std::path::{Path, PathBuf};

use cp_audio::{AudioHost, ClipH};
use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Scratch, Surface, png};
use cp_sys::font::GdiGlyphs;
use cp_ui::{Button, Ev, IconCache, Mods, Theme, vk};

use super::*;
use crate::draw::Gfx;
use crate::model::Shortcut;

/// Un hote : le vrai moteur, des fichiers synthetiques (la duree depend du nom).
struct Fake {
    a: AudioHost,
    tickets: Vec<(u32, String)>,
    next: u32,
    inserted: Vec<(String, Option<(f64, f64)>)>,
    saved: Vec<(String, String)>,
    /// L'item « choisi dans REAPER » : son fichier.
    item: Option<(String, f64)>,
    fx_added: Vec<String>,
    rate: f64,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            a: AudioHost::new(None, 48000.0),
            tickets: Vec::new(),
            next: 1,
            inserted: Vec::new(),
            saved: Vec::new(),
            item: Some(("C:\\orig.wav".into(), 0.0)),
            fx_added: Vec::new(),
            rate: 1.0,
        }
    }

    /// Fait avancer l'horloge et tire le port 0 comme REAPER.
    fn run_audio(&mut self, frames: usize) -> Vec<f64> {
        let t = self.a.core().clock_now() as f64 / 48000.0;
        let mut out = vec![0.0; frames * 2];
        self.a.pull(0, t, 2, &mut out);
        self.a.core().tick(frames as i32);
        self.a.poll();
        out
    }
}

fn synth(path: &str) -> Vec<f32> {
    let n = 2400 + (path.len() * 1997) % 40000;
    let f = 110.0 + (path.len() % 7) as f32 * 55.0;
    let mut v = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / 48000.0;
        let env = (-t * 6.0).exp();
        let s = (t * f * std::f32::consts::TAU).sin() * env * 0.8;
        v.push(s);
        v.push(s * 0.6);
    }
    v
}

impl Host for Fake {
    fn load(&mut self, path: &str) -> u32 {
        let t = self.next;
        self.next += 1;
        self.tickets.push((t, path.to_string()));
        t
    }
    fn poll(&mut self, ticket: u32) -> Loaded {
        let Some(i) = self.tickets.iter().position(|(t, _)| *t == ticket) else {
            return Loaded::Failed("unknown".into());
        };
        let (_, p) = self.tickets.remove(i);
        match self.a.load_samples(synth(&p), 2, 48000.0) {
            Ok(c) => Loaded::Ready(c.0),
            Err(e) => Loaded::Failed(e),
        }
    }
    fn info(&mut self, clip: u32) -> Option<ClipInfo> {
        let decoded = self.a.clip_decoded(ClipH(clip));
        self.a
            .clip_info(ClipH(clip))
            .map(|(frames, nch, srate)| ClipInfo {
                frames,
                nch: nch as u32,
                srate,
                decoded: decoded.unwrap_or(frames),
            })
    }
    fn release(&mut self, clip: u32) {
        self.a.release(ClipH(clip));
    }
    fn peaks(
        &mut self,
        clip: u32,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        min: &mut [f32],
        max: &mut [f32],
    ) -> usize {
        self.a.peaks(ClipH(clip), ch, start, fpp, min, max)
    }
    fn play(&mut self, clip: u32, start: i64, end: i64, looped: bool, gain: f64) -> u32 {
        self.a
            .play(0, ClipH(clip), start, end, looped, gain)
            .map_or(0, |v| v.0)
    }
    fn stop(&mut self, voice: u32, fade_s: f64) {
        self.a.stop(cp_audio::VoiceH(voice), fade_s);
    }
    fn voice(&mut self, voice: u32) -> Option<(f64, i32)> {
        self.a.voice(cp_audio::VoiceH(voice))
    }
    fn set_gain(&mut self, _voice: u32, _gain: f64) {}
    fn insert(&mut self, path: &str, section: Option<(f64, f64)>) -> Result<(), String> {
        self.inserted.push((path.to_string(), section));
        Ok(())
    }
    fn save(&mut self, key: &str, value: &str) {
        self.saved.push((key.into(), value.into()));
    }
    fn project_bpm(&mut self) -> f64 {
        100.0
    }
    fn set_rate(&mut self, _voice: u32, rate: f64) {
        self.rate = rate;
    }
    fn swap_begin(&mut self) -> Result<(String, f64), String> {
        self.item.clone().ok_or_else(|| "no item".into())
    }
    fn swap(&mut self, path: &str, offset: f64) -> Result<(), String> {
        self.item = Some((path.to_string(), offset));
        Ok(())
    }
    fn installed_fx(&mut self) -> Vec<String> {
        vec![
            "VST3: Pro-Q 3 (FabFilter)".into(),
            "JS: Saturation".into(),
            "VST3: ReaEQ (Cockos)".into(),
            "CLAP: Surge XT".into(),
        ]
    }
    fn add_fx(&mut self, name: &str) -> Result<(), String> {
        self.fx_added.push(name.to_string());
        Ok(())
    }
}

/// Un faux dossier de sons.
fn library() -> PathBuf {
    let root = std::env::temp_dir().join("cp_browser_lib");
    let dirs = [
        "Drums/Kicks",
        "Drums/Snares",
        "Drums/Hats",
        "Synths/Pads",
        "Synths/Leads",
        "FX",
    ];
    for d in dirs {
        let _ = std::fs::create_dir_all(root.join(d));
    }
    let files = [
        ("Drums/Kicks", 18),
        ("Drums/Snares", 9),
        ("Drums/Hats", 6),
        ("Synths/Pads", 4),
        ("FX", 5),
    ];
    for (d, n) in files {
        for i in 1..=n {
            let p = root.join(d).join(format!(
                "{} {:02}.wav",
                d.rsplit('/').next().unwrap().trim_end_matches('s'),
                i
            ));
            if !p.exists() {
                std::fs::write(&p, b"RIFF").unwrap();
            }
        }
    }
    root
}

struct Screen {
    s: Surface,
    k: Scratch,
    t: TextCache,
    icons: IconCache,
    th: Theme,
}

impl Screen {
    fn new(w: i32, h: i32) -> Screen {
        Screen {
            s: Surface::new(w, h),
            k: Scratch::default(),
            t: TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas"]))),
            icons: IconCache::default(),
            th: Theme::dark(1.0),
        }
    }

    fn shot(&mut self, b: &mut Browser, h: &mut Fake, name: &str) -> PathBuf {
        {
            let mut cv = Canvas::new(&mut self.s, &mut self.k);
            let mut g = Gfx {
                text: &mut self.t,
                icons: &mut self.icons,
                th: &self.th,
            };
            b.draw(&mut cv, &mut g, h);
        }
        let p = std::env::temp_dir().join(format!("cp_browser_{name}.png"));
        std::fs::write(&p, png::encode(&self.s)).unwrap();
        p
    }
}

fn shortcuts(root: &Path) -> Vec<Shortcut> {
    vec![
        Shortcut {
            target: "x.ReaperFileList".into(),
            title: Some("---------- SOUNDS".into()),
        },
        Shortcut {
            target: root.to_string_lossy().into_owned(),
            title: Some("Sound Library".into()),
        },
        Shortcut {
            target: root.join("FX").to_string_lossy().into_owned(),
            title: None,
        },
    ]
}

fn key(b: &mut Browser, h: &mut Fake, k: u32) {
    b.event(
        h,
        &Ev::Key {
            vk: k,
            mods: Mods::default(),
            text: None,
        },
    );
}

#[test]
fn parcourir_ecouter_et_choisir_une_section() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    let mut sc = Screen::new(420, 640);
    b.layout(420.0, 640.0, 1.0);
    // Ouvrir la bibliotheque, puis Drums, puis Kicks, au clavier.
    let lib = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "Sound Library"))
        .unwrap();
    b.sel = Some(lib);
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN); // Drums
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN); // Hats
    key(&mut b, &mut h, vk::DOWN); // Kicks
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN); // Kick 01 : se charge et joue
    b.tick(&mut h);
    assert!(b.wave.has(), "la forme d'onde est la");
    assert!(b.animating(), "une voix joue");
    let mut heard = 0.0f64;
    for _ in 0..20 {
        heard += h.run_audio(256).iter().map(|x| x.abs()).sum::<f64>();
        b.tick(&mut h);
    }
    assert!(heard > 1.0, "on entend le fichier");
    assert!(b.wave.playhead.is_some());
    sc.shot(&mut b, &mut h, "1_ecoute");

    // Une section a la souris dans la forme d'onde : elle est jouee seule.
    let y = b.lay.wave_y + 20.0;
    b.event(
        &mut h,
        &Ev::Down {
            x: 100.0,
            y,
            b: Button::Left,
            dbl: false,
            mods: Mods::default(),
        },
    );
    b.event(
        &mut h,
        &Ev::Move {
            x: 180.0,
            y,
            mods: Mods::default(),
        },
    );
    b.event(
        &mut h,
        &Ev::Up {
            x: 180.0,
            y,
            b: Button::Left,
            mods: Mods::default(),
        },
    );
    let (a, z) = b.wave.sel.expect("une section");
    assert!(z > a);
    // Zoomer sur la section.
    for _ in 0..6 {
        b.event(
            &mut h,
            &Ev::Wheel {
                x: 140.0,
                y,
                dx: 0.0,
                dy: 1.0,
                mods: Mods::default(),
            },
        );
    }
    b.tick(&mut h);
    for _ in 0..4 {
        h.run_audio(256);
        b.tick(&mut h);
    }
    sc.shot(&mut b, &mut h, "2_section");

    // Entree : insere le fichier avec sa section (en secondes).
    key(&mut b, &mut h, vk::RETURN);
    let (p, sec) = h.inserted.last().cloned().unwrap();
    assert!(p.ends_with("Kick 01.wav"), "{p}");
    let (s0, s1) = sec.unwrap();
    assert!((s0 - a as f64 / 48000.0).abs() < 1e-12 && (s1 - z as f64 / 48000.0).abs() < 1e-12);
}

#[test]
fn les_en_tetes_restent_colles_en_haut() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    let mut sc = Screen::new(420, 400);
    b.layout(420.0, 400.0, 1.0);
    let lib = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "Sound Library"))
        .unwrap();
    b.sel = Some(lib);
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN);
    key(&mut b, &mut h, vk::RIGHT); // Drums ouvert
    key(&mut b, &mut h, vk::DOWN);
    key(&mut b, &mut h, vk::DOWN);
    key(&mut b, &mut h, vk::RIGHT); // Kicks ouvert (18 fichiers)
    // Defiler au milieu des kicks.
    b.scroll.pos = 9.0 * b.lay.row_h;
    b.scroll.clamp();
    let st = b.sticky();
    let names: Vec<&str> = st.iter().map(|&i| b.tree.nodes[i].name.as_str()).collect();
    assert_eq!(names, vec!["Sound Library", "Drums", "Kicks"]);
    sc.shot(&mut b, &mut h, "3_entetes");
}

#[test]
fn chercher_trouve_dans_ce_qui_est_ouvert() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    let mut sc = Screen::new(420, 400);
    b.layout(420.0, 400.0, 1.0);
    let fx = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "FX"))
        .unwrap();
    b.sel = Some(fx);
    key(&mut b, &mut h, vk::RIGHT);
    b.event(
        &mut h,
        &Ev::Key {
            vk: 0x46,
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
            text: None,
        },
    );
    assert_eq!(b.focus, Focus::Search);
    for c in "fx 0".chars() {
        b.event(&mut h, &Ev::Char(c));
    }
    assert_eq!(b.hits.len(), 5, "les cinq FX");
    sc.shot(&mut b, &mut h, "4_recherche");
    // Bas : on passe dans la liste et on ecoute le premier.
    key(&mut b, &mut h, vk::DOWN);
    assert_eq!(b.focus, Focus::List);
    b.tick(&mut h);
    assert!(b.wave.has());
}

#[test]
fn l_etat_survit_a_un_rechargement() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    b.layout(420.0, 400.0, 1.0);
    let lib = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "Sound Library"))
        .unwrap();
    b.sel = Some(lib);
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN);
    key(&mut b, &mut h, vk::RIGHT);
    let rows = b.rows.len();
    let s = b.save_state();
    let mut b2 = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    b2.layout(420.0, 400.0, 1.0);
    b2.restore_state(&s);
    assert_eq!(b2.rows.len(), rows);
    assert_eq!(b2.sel, b.sel);
}

#[test]
fn v2_collections_hasard_echange_effets_tempo() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    let data = std::env::temp_dir().join("cp_browser_data");
    let _ = std::fs::remove_file(data.join("collections.txt"));
    b.data_dir = Some(data.clone());
    b.load_collections();
    let mut sc = Screen::new(460, 560);
    b.layout(460.0, 560.0, 1.0);
    let fx = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "FX"))
        .unwrap();
    b.sel = Some(fx);
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN); // FX 01
    b.tick(&mut h);
    // Collections : 1 et 4 sur FX 01, 1 sur FX 02.
    key(&mut b, &mut h, 0x31);
    key(&mut b, &mut h, 0x34);
    key(&mut b, &mut h, vk::DOWN);
    key(&mut b, &mut h, 0x31);
    let text = std::fs::read_to_string(data.join("collections.txt")).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    let red = b.tree.collections[0];
    assert_eq!(b.tree.nodes[red].children.as_ref().unwrap().len(), 2);
    // Relue a froid.
    let mut b2 = Browser::new(&shortcuts(&root), Path::new("."), None, Vec::new());
    b2.data_dir = Some(data.clone());
    b2.load_collections();
    assert_eq!(b2.collections.len(), 2);

    // Le hasard : un fichier du meme dossier.
    key(&mut b, &mut h, 0x52);
    let p = b.selected_path_pub().unwrap();
    assert!(p.starts_with(root.join("FX")), "{}", p.display());

    // Le remplacement a chaud : Q, une fleche, Echap rend l'original.
    key(&mut b, &mut h, 0x51);
    assert!(b.swapping());
    key(&mut b, &mut h, vk::UP);
    assert_ne!(h.item.as_ref().unwrap().0, "C:\\orig.wav");
    key(&mut b, &mut h, vk::ESCAPE);
    assert!(!b.swapping());
    assert_eq!(h.item.as_ref().unwrap().0, "C:\\orig.wav");

    // Les effets : la racine se remplit a l'ouverture, Entree ajoute.
    let fxr = b
        .rows
        .iter()
        .position(|&r| r == Row::Node(b.tree.fx_root))
        .unwrap();
    b.sel = Some(fxr);
    key(&mut b, &mut h, vk::RIGHT);
    assert!(b.tree.nodes[b.tree.fx_root].children.is_some());
    key(&mut b, &mut h, vk::DOWN); // CLAP (1)
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN); // Surge XT
    key(&mut b, &mut h, vk::RETURN);
    assert_eq!(h.fx_added, vec!["CLAP: Surge XT".to_string()]);
    sc.shot(&mut b, &mut h, "5_v2");

    // Le tempo : un fichier « 120 bpm » joue a 100/120 dans un projet a 100.
    assert_eq!(v2::bpm_from_name("Loop 120 bpm.wav"), Some(120.0));
    assert!((v2::sync_rate(120.0, 100.0).unwrap() - 100.0 / 120.0).abs() < 1e-12);
}
