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
    thumbs_asked: std::collections::HashSet<String>,
    stretches: usize,
    /// Les longs fichiers, charges par le vrai moteur (en flux) : (ticket
    /// du navigateur, ticket du moteur).
    real: Vec<(u32, u32)>,
    /// La poignee de la vue tiree (le glisser passe a REAPER).
    grabbed: usize,
}

/// Un long fichier synthetique : a gauche un La a 0,25, a droite la seconde
/// jouee (0,9 * (s mod 1000) / 1000) : ce qu'on entend dit ou l'on est.
struct LongSrc {
    frames: usize,
}

impl cp_audio::decode::Reader for LongSrc {
    fn channels(&mut self) -> i32 {
        2
    }
    fn sample_rate(&mut self) -> f64 {
        48000.0
    }
    fn length(&mut self) -> f64 {
        self.frames as f64 / 48000.0
    }
    fn read(&mut self, time_s: f64, _srate: f64, nch: usize, out: &mut [f64]) -> usize {
        let start = (time_s * 48000.0).round() as usize;
        let want = out.len() / nch;
        let got = want.min(self.frames.saturating_sub(start));
        for i in 0..got {
            let f = start + i;
            let t = f as f64 / 48000.0;
            out[i * nch] = (t * 440.0 * std::f64::consts::TAU).sin() * 0.25;
            out[i * nch + 1] = 0.9 * ((f / 48000) % 1000) as f64 / 1000.0;
        }
        got
    }
}

/// Les tickets de cuisson du vrai moteur, distingues de ceux du faux
/// chargement.
const STRETCH_T: u32 = 1 << 30;

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
            thumbs_asked: std::collections::HashSet::new(),
            stretches: 0,
            real: Vec::new(),
            grabbed: 0,
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
    fn dock_grab(&mut self) {
        self.grabbed += 1;
    }
    fn load(&mut self, path: &str) -> u32 {
        let t = self.next;
        self.next += 1;
        if path.contains("Long") {
            // Vingt minutes : le vrai moteur les lit en flux.
            let at = self.a.load_reader(Box::new(LongSrc {
                frames: 48000 * 60 * 20,
            }));
            self.real.push((t, at));
            return t;
        }
        self.tickets.push((t, path.to_string()));
        t
    }
    fn poll(&mut self, ticket: u32) -> Loaded {
        // Une cuisson : le vrai moteur.
        if ticket & STRETCH_T != 0 {
            self.a.poll();
            return match self.a.ticket(ticket & !STRETCH_T) {
                cp_audio::Load::Pending => Loaded::Pending,
                cp_audio::Load::Ready(c) => Loaded::Ready(c.0),
                cp_audio::Load::Failed(e) => Loaded::Failed(e),
            };
        }
        if let Some(&(_, at)) = self.real.iter().find(|x| x.0 == ticket) {
            self.a.poll();
            return match self.a.ticket(at) {
                cp_audio::Load::Pending => Loaded::Pending,
                cp_audio::Load::Ready(c) => Loaded::Ready(c.0),
                cp_audio::Load::Failed(e) => Loaded::Failed(e),
            };
        }
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
    fn stretch(&mut self, clip: u32, start: i64, end: i64, tempo: f64) -> u32 {
        self.stretches += 1;
        let t = self.a.stretch_with(
            ClipH(clip),
            start,
            end,
            tempo,
            1.0,
            Box::new(cp_audio::stretch::LinearShifter::new(0)),
        );
        t | STRETCH_T
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
    fn thumb(&mut self, path: &str, min: &mut [f32], max: &mut [f32]) -> i32 {
        self.a.poll();
        let st = if self.thumbs_asked.insert(path.to_string()) {
            self.a
                .thumb_reader(path, Box::new(SynthReader(synth(path))))
        } else {
            self.a.thumb(path)
        };
        match st {
            cp_audio::Thumb::Pending => 0,
            cp_audio::Thumb::Failed => -1,
            cp_audio::Thumb::Ready(d) => {
                let n = min.len();
                for i in 0..n {
                    let e = d[i * d.len() / n];
                    min[i] = e[0];
                    max[i] = e[1];
                }
                1
            }
        }
    }
}

/// Une matiere synthetique lue comme un fichier (stereo, 48 kHz).
struct SynthReader(Vec<f32>);

impl cp_audio::decode::Reader for SynthReader {
    fn channels(&mut self) -> i32 {
        2
    }
    fn sample_rate(&mut self) -> f64 {
        48000.0
    }
    fn length(&mut self) -> f64 {
        (self.0.len() / 2) as f64 / 48000.0
    }
    fn read(&mut self, time_s: f64, _sr: f64, nch: usize, out: &mut [f64]) -> usize {
        let start = (time_s * 48000.0).round() as usize;
        let frames = (self.0.len() / 2)
            .saturating_sub(start)
            .min(out.len() / nch);
        for f in 0..frames {
            for c in 0..nch {
                out[f * nch + c] = self.0[(start + f) * 2 + c.min(1)] as f64;
            }
        }
        frames
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
                skin: None,
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

fn places(root: &Path) -> Places {
    Places {
        recordings: None,
        folders: Places::from_shortcuts(&shortcuts(root), Path::new("."), None),
    }
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
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
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
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
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
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
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
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
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
    let mut b2 = Browser::new(places(&root), Path::new("."), Vec::new());
    b2.layout(420.0, 400.0, 1.0);
    b2.restore_state(&s);
    assert_eq!(b2.rows.len(), rows);
    assert_eq!(b2.sel, b.sel);
}

#[test]
fn v2_collections_hasard_echange_effets_tempo() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
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
    let mut b2 = Browser::new(places(&root), Path::new("."), Vec::new());
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

#[test]
fn la_vue_samples_montre_une_forme_d_onde_par_fichier() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
    let mut sc = Screen::new(460, 520);
    b.layout(460.0, 520.0, 1.0);
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
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, 0x57); // W
    assert!(b.samples_view);
    assert_eq!(b.lay.row_h, 44.0);
    // Dessiner jusqu'a ce que les vignettes visibles soient pretes.
    let t0 = std::time::Instant::now();
    loop {
        sc.shot(&mut b, &mut h, "6_samples");
        if !b.thumbs_pending.get() {
            break;
        }
        assert!(t0.elapsed().as_secs() < 20, "vignettes trop lentes");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Le navigateur habille du theme COURANT de la machine, sur le vrai dossier
/// d'enregistrement (lance a la main : `--ignored`) : `cp_browser_theme.png`.
#[test]
#[ignore]
fn habille_du_vrai_theme() {
    let res = PathBuf::from(std::env::var("APPDATA").unwrap()).join("REAPER");
    let ini = std::fs::read_to_string(res.join("reaper.ini")).unwrap_or_default();
    let get = |k: &str| {
        ini.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .map(|s| s.trim().to_string())
    };
    let theme =
        cp_theme::ThemeFiles::open(Path::new(&get("lastthemefn5").unwrap())).expect("theme");
    let rec = get("defrecpath").map(PathBuf::from);
    let mut h = Fake::new();
    let places = Places {
        recordings: rec,
        folders: vec![model::Place {
            path: res.join("Data"),
            title: Some("REAPER Data".into()),
        }],
    };
    let mut b = Browser::new(places, Path::new("."), Vec::new());
    let tb = |name: &str| {
        let (img, s) = theme.image(name, 1.0)?;
        Some(cp_theme::Frames::split(&img, 3)?.rescaled(s, 1.0))
    };
    let skin = crate::draw::Skin {
        play: tb("transport_play"),
        play_on: tb("transport_play_on"),
        repeat_off: tb("transport_repeat_off"),
        repeat_on: tb("transport_repeat_on"),
    };
    b.theme_btn_w = skin.play.as_ref().map(|f| f.w() as f32);
    b.looping = true;
    let mut sc = Screen::new(420, 520);
    sc.th = Theme::reaper(1.0, &|k| theme.color(k));
    b.layout(420.0, 520.0, 1.0);
    b.sel = Some(1);
    {
        let mut cv = Canvas::new(&mut sc.s, &mut sc.k);
        let mut g = Gfx {
            text: &mut sc.t,
            icons: &mut sc.icons,
            th: &sc.th,
            skin: Some(&skin),
        };
        b.draw(&mut cv, &mut g, &mut h);
    }
    let p = std::env::temp_dir().join("cp_browser_theme.png");
    std::fs::write(&p, png::encode(&sc.s)).unwrap();
    println!("{}", p.display());
}

/// Le calage au tempo sans changer la hauteur : la premiere ecoute joue a la
/// vitesse de calage (la hauteur suit), la version etiree se cuit, puis
/// l'ecoute passe sur elle au meme endroit, a vitesse 1 ; la tete de lecture
/// reste celle du fichier. Couper le calage revient au fichier.
#[test]
fn le_calage_au_tempo_garde_la_hauteur() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
    b.layout(460.0, 520.0, 1.0);
    // Un fichier « 120 bpm » dans un projet a 100, montre et joue en boucle.
    let path = "C:/Loops/Loop 120 bpm.wav";
    let t = h.load(path);
    let Loaded::Ready(clip) = h.poll(t) else {
        panic!("chargement");
    };
    let info = h.info(clip).unwrap();
    b.wave.set_clip(clip, info, path);
    b.wave.layout(b.lay.w);
    b.sync = true;
    b.looping = true;
    b.play_wave(&mut h, None);
    for _ in 0..4 {
        b.tick(&mut h);
        h.run_audio(512);
    }
    let r = 100.0 / 120.0;
    assert!((h.rate - r).abs() < 1e-9, "d'abord la vitesse : {}", h.rate);
    assert_eq!(h.stretches, 1, "une cuisson demandee");
    // La cuisson finit : l'ecoute passe sur la version etiree.
    let t0 = std::time::Instant::now();
    while b.voice_map.is_none() {
        b.tick(&mut h);
        h.run_audio(512);
        assert!(t0.elapsed().as_secs() < 10, "cuisson trop lente");
    }
    let st = b.stretched.clone().unwrap();
    assert!((st.tempo - r).abs() < 1e-9 && st.clip != 0);
    // La tete de lecture reste dans le fichier.
    h.run_audio(4800);
    b.tick(&mut h);
    let ph = b.wave.playhead.unwrap();
    assert!(ph >= 0.0 && ph <= b.wave.info.frames as f64, "{ph}");
    // Couper le calage : retour au fichier, a vitesse 1.
    key(&mut b, &mut h, 0x54);
    assert!(b.voice_map.is_none());
    assert!((h.rate - 1.0).abs() < 1e-9 || !b.sync);
}

/// Les boutons de la barre s'expliquent en infobulle (rien a l'ecran) ; la
/// liste n'en a pas.
#[test]
fn la_barre_s_explique_en_infobulle() {
    let root = library();
    let b = {
        let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
        b.layout(460.0, 520.0, 1.0);
        b
    };
    let l = b.lay;
    let at = |x: f32| b.tip_at(x + 2.0, l.bar_h * 0.5).map(|t| t.0);
    assert!(at(l.sync_x).unwrap().contains("keeping the pitch"));
    assert!(at(l.view_x).unwrap().contains("Samples view"));
    assert!(at(l.star_x).unwrap().contains("Favorite"));
    assert!(at(l.play_x).unwrap().contains("Space"));
    assert!(b.tip_at(10.0, l.list_y + 10.0).is_none());
}

#[test]
fn un_long_fichier_s_ecoute_en_flux_dans_le_navigateur() {
    // Vingt minutes : le navigateur l'ecoute des le debut, saute au milieu
    // par un clic dans la forme d'onde, et l'on entend la bonne seconde ;
    // aucun bloc sans sa page (les blocs sont tires au rythme de REAPER).
    let root = std::env::temp_dir().join("cp_browser_long");
    let _ = std::fs::create_dir_all(&root);
    let file = root.join("Long Take 100 bpm.wav");
    if !file.exists() {
        std::fs::write(&file, b"RIFF").unwrap();
    }
    let pl = Places {
        recordings: None,
        folders: Places::from_shortcuts(
            &[Shortcut {
                target: root.to_string_lossy().into_owned(),
                title: Some("Long".into()),
            }],
            Path::new("."),
            None,
        ),
    };
    let mut h = Fake::new();
    let mut b = Browser::new(pl, Path::new("."), Vec::new());
    b.layout(420.0, 640.0, 1.0);
    let top = b
        .rows
        .iter()
        .position(|&r| matches!(r, Row::Node(id) if b.tree.nodes[id].name == "Long"))
        .unwrap();
    b.sel = Some(top);
    key(&mut b, &mut h, vk::RIGHT);
    key(&mut b, &mut h, vk::DOWN);
    // Au rythme de REAPER : un bloc de 512 toutes les 10,7 ms.
    let period = std::time::Duration::from_secs_f64(512.0 / 48000.0);
    let run = |b: &mut Browser, h: &mut Fake, blocks: usize| -> Vec<f64> {
        let mut out = Vec::new();
        for _ in 0..blocks {
            b.tick(h);
            out.extend(h.run_audio(512));
            std::thread::sleep(period);
        }
        out
    };
    let a = run(&mut b, &mut h, 60);
    assert!(b.wave.has(), "la forme d'onde est la");
    let heard_l = a.chunks_exact(2).map(|x| x[0].abs()).fold(0.0, f64::max);
    assert!(heard_l > 0.2, "on entend le debut ({heard_l})");
    // Un clic au milieu de la forme d'onde : on y saute.
    let y = b.lay.wave_y + 20.0;
    let x = b.lay.w * 0.5;
    for ev in [
        Ev::Down {
            x,
            y,
            b: Button::Left,
            dbl: false,
            mods: Mods::default(),
        },
        Ev::Up {
            x,
            y,
            b: Button::Left,
            mods: Mods::default(),
        },
    ] {
        b.event(&mut h, &ev);
    }
    let after = run(&mut b, &mut h, 60);
    let right: Vec<f64> = after.chunks_exact(2).map(|x| x[1]).collect();
    let tail = &right[right.len() - 4800..];
    let v = tail.iter().copied().fold(0.0, f64::max);
    let sec = (v / 0.9 * 1000.0).round() as i64;
    assert!(
        (400..=800).contains(&sec),
        "apres le clic on entend la seconde {sec} (un saut vers le milieu des 20 min)"
    );
    eprintln!("navigateur : apres le clic au milieu, la seconde {sec}");
    // Le calage au tempo (T) : un long fichier ne se cuit pas ; l'ecoute
    // continue, calee par la vitesse.
    key(&mut b, &mut h, 0x54);
    let synced = run(&mut b, &mut h, 40);
    assert!(b.sync, "le calage est pris");
    let loud = synced
        .chunks_exact(2)
        .map(|x| x[0].abs())
        .fold(0.0, f64::max);
    assert!(loud > 0.2, "l'ecoute continue ({loud})");
    let st = h.a.stats();
    assert_eq!(st.streams, 1, "lu en flux");
    assert_eq!(st.stream_misses, 0, "aucun bloc sans sa page");
    assert!(st.bytes < 64 << 20, "{} Mo", st.bytes >> 20);
}

#[test]
fn un_depot_de_l_explorateur_montre_et_fait_ecouter() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
    b.layout(420.0, 640.0, 1.0);
    // Un fichier de la bibliotheque : son dossier s'ouvre, il est choisi et
    // s'ecoute.
    let kick = root.join("Drums").join("Snares").join("Snare 03.wav");
    b.drop_paths(&mut h, std::slice::from_ref(&kick));
    assert_eq!(b.selected_path_pub().as_deref(), Some(kick.as_path()));
    b.tick(&mut h);
    assert!(b.animating(), "il joue");
    // Un dossier hors des siens : ajoute, ouvert, choisi.
    let other = std::env::temp_dir().join("cp_browser_drop");
    let _ = std::fs::create_dir_all(&other);
    let f = other.join("Dropped 01.wav");
    if !f.exists() {
        std::fs::write(&f, b"RIFF").unwrap();
    }
    b.drop_paths(&mut h, std::slice::from_ref(&other));
    assert_eq!(b.selected_path_pub().as_deref(), Some(other.as_path()));
    assert!(h.saved.iter().any(|(k, _)| k == "folders"), "retenu");
    // Puis un fichier de ce dossier.
    b.drop_paths(&mut h, std::slice::from_ref(&f));
    assert_eq!(b.selected_path_pub().as_deref(), Some(f.as_path()));
}

#[test]
fn un_depot_relit_un_dossier_deja_lu_et_ignore_la_casse() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
    b.layout(420.0, 640.0, 1.0);
    // Le dossier des caisses claires, lu une fois.
    let snares = root.join("Drums").join("Snares");
    b.drop_paths(&mut h, std::slice::from_ref(&snares.join("Snare 01.wav")));
    // Un fichier tout neuf, depose ensuite : le dossier est relu.
    let fresh = snares.join("Snare 99 tout neuf.wav");
    std::fs::write(&fresh, b"RIFF").unwrap();
    b.drop_paths(&mut h, std::slice::from_ref(&fresh));
    assert_eq!(b.selected_path_pub().as_deref(), Some(fresh.as_path()));
    // La casse de Windows : le meme fichier, ecrit autrement.
    let shout = PathBuf::from(fresh.to_string_lossy().to_uppercase());
    b.drop_paths(&mut h, std::slice::from_ref(&shout));
    assert!(
        b.selected_path_pub().is_some_and(|p| p
            .to_string_lossy()
            .eq_ignore_ascii_case(&fresh.to_string_lossy())),
        "pas de dossier en double pour une casse differente"
    );
    let n_places = h.saved.iter().filter(|(k, _)| k == "folders").count();
    // Un fichier qui n'est pas un son : rien n'est ajoute.
    let txt = std::env::temp_dir().join("cp_browser_drop_note.txt");
    std::fs::write(&txt, b"x").unwrap();
    b.drop_paths(&mut h, std::slice::from_ref(&txt));
    assert_eq!(
        h.saved.iter().filter(|(k, _)| k == "folders").count(),
        n_places
    );
    let _ = std::fs::remove_file(&fresh);
    let _ = std::fs::remove_file(&txt);
}

/// La poignee de la vue ancree : a gauche de la barre (la recherche se
/// pousse), une infobulle ; un clic ne fait rien, un glisser passe la main
/// a REAPER.
#[test]
fn la_poignee_de_la_vue_ancree() {
    let root = library();
    let mut h = Fake::new();
    let mut b = Browser::new(places(&root), Path::new("."), Vec::new());
    b.layout(460.0, 520.0, 1.0);
    let x0 = b.lay.search_x;
    assert_eq!(b.lay.handle_w, 0.0);
    b.dock_handle = true;
    b.layout(460.0, 520.0, 1.0);
    let hw = b.lay.handle_w;
    assert!(hw > 40.0 && (b.lay.search_x - x0 - hw).abs() < 0.5);
    let y = b.lay.bar_h * 0.5;
    assert!(b.tip_at(10.0, y).unwrap().0.contains("another docker"));
    let mods = Mods::default();
    let ev = |b: &mut Browser, h: &mut Fake, e: Ev| {
        b.event(h, &e);
    };
    // Un clic : rien.
    ev(
        &mut b,
        &mut h,
        Ev::Down {
            x: 10.0,
            y,
            b: Button::Left,
            dbl: false,
            mods,
        },
    );
    ev(&mut b, &mut h, Ev::Move { x: 11.0, y, mods });
    ev(
        &mut b,
        &mut h,
        Ev::Up {
            x: 11.0,
            y,
            b: Button::Left,
            mods,
        },
    );
    assert_eq!(h.grabbed, 0);
    // Un glisser : une fois.
    ev(
        &mut b,
        &mut h,
        Ev::Down {
            x: 10.0,
            y,
            b: Button::Left,
            dbl: false,
            mods,
        },
    );
    ev(
        &mut b,
        &mut h,
        Ev::Move {
            x: 30.0,
            y: y + 12.0,
            mods,
        },
    );
    ev(
        &mut b,
        &mut h,
        Ev::Move {
            x: 60.0,
            y: y + 40.0,
            mods,
        },
    );
    assert_eq!(h.grabbed, 1);
    let mut sc = Screen::new(460, 520);
    sc.shot(&mut b, &mut h, "poignee");
}
