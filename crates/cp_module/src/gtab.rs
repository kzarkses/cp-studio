//! CP Guitar Tab dans REAPER : un ARRANGEUR de guitares.
//!
//! Une piste de guitare, ce sont trois pistes REAPER :
//! - un DOSSIER « Guitar 1 » (role `bus`) : replie, il tient sur une ligne ;
//!   son Mute et son Solo valent pour la guitare entiere ;
//! - la piste AUDIO (role `guitar`), armee : les prises s'y enregistrent ;
//! - la piste TAB (role `tab`) : les items MIDI de la tablature (une corde
//!   par canal) et le synthe de Guitar Tab.
//!
//! Deux enfants et pas un : un enfant passe par son dossier, donc si la
//! guitare etait le dossier, couper l'original couperait la tab, et un ampli
//! sur la guitare traiterait le synthe.
//!
//! Chaque item audio a son item MIDI sur la piste tab, au meme endroit
//! (`CPGT_midi` sur l'audio, `CPGT_src` et `CPGT_take` sur le MIDI). AUCUNE
//! DETTE DE RANGEMENT : un enregistrement ne cree jamais de piste ; un item
//! audio efface emporte son MIDI ; un item deplace entraine le sien ; une
//! copie recoit le sien.
//!
//! L'analyse d'une prise (le reseau, une fois) est en cache dans
//! `Guitar Tab/cache/<cle du son>.cpga`, sa tablature dans
//! `<GUID de la prise>.cptab` : changer de prise change la tab, et revenir la
//! retrouve.
//!
//! Le fil de travail ne survit jamais au module : un rechargement l'annule et
//! l'attend.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Instant;

use cp_abi::{
    PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK, VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL, WM_CP_FRAME,
};
use cp_gfx::{Canvas, Surface};
use cp_sys::items::QnNote;
use cp_sys::module::key_event;
use cp_sys::pcm::CppSource;
use cp_sys::tab::ItemAudio;
use cp_sys::tracks::{ItemSnap, TakeSnap, TrackSnap};
use cp_sys::transport::play;
use cp_sys::ui::{self, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_tab::doc::{self, MidiQn, Sidecar};
use cp_tab::model::{HOP, SR};
use cp_tab::score::{ScoreSpec, TabNote, write_musicxml};
use cp_tab::{Analysis, NoteParams, Posteriors, Progress, analyze, make_notes};
use cp_tabview::{
    Act as GAct, Arr, Detect, Head, Item, Lane, Lanes, Listen, Rec, Sel, Sound, State, Synth,
    TakeInfo, Track,
};
use cp_ui::{Button, Ev, Theme};

use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

const TITLE: &str = "Guitar Tab";
const IDENT: &str = "cp_studio_guitartab";

/// Les donnees privees lues sur chaque piste.
const TKEYS: [&str; 6] = [
    "CPGT_role",
    "CPGT_tab",
    "CPGT_bus",
    "CPGT_lanes",
    "CPGT_tuning",
    "CPGT_pan",
];
const K_ROLE: usize = 0;
const K_TAB: usize = 1;
const K_BUS: usize = 2;
const K_LANES: usize = 3;
const K_TUNING: usize = 4;
const K_PAN: usize = 5;
/// ... et sur chaque item.
const IKEYS: [&str; 4] = ["CPGT_midi", "CPGT_src", "CPGT_take", "CPGT_aoffs"];
const I_MIDI: usize = 0;
const I_SRC: usize = 1;
const I_TAKE: usize = 2;
/// Sur l'item MIDI : le decalage de la prise audio (s) quand il a ete pose.
/// Un rognage du debut de l'audio decale d'autant le MIDI dans sa source,
/// pour que les notes restent ou elles sonnent.
const I_AOFFS: usize = 3;

/// Le synthe : le fichier installe, son nom dans la chaine.
const SYNTH_SRC: &str = include_str!("cp_guitar_tab_synth.jsfx");
const SYNTH_FILE: &str = "cp_guitar_tab_synth.jsfx";
const SYNTH_ADD: &str = "JS:CP_Studio/cp_guitar_tab_synth.jsfx";
const SYNTH_NAME: &str = "CP Guitar Tab Synth";

/// La fenetre.
pub(crate) struct GWin {
    pub hwnd: Hwnd,
    pub place: Place,
    surf: Surface,
    present: Vec<u32>,
    theme: Theme,
    theme_key: Vec<Option<(u8, u8, u8)>>,
    tracking: bool,
    frames_on: bool,
    closing: bool,
    /// L'infobulle, la cible survolee et depuis quand, le texte montre.
    tip: Option<ui::Tooltip>,
    hover: (cp_tabview::Hit, Instant),
    tip_text: String,
}

/// Ce qu'il reste a faire a un item reconstruit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Todo {
    Nothing,
    Ghosts,
    Decode,
}

/// L'analyse d'un son, gardee en memoire.
struct Ana {
    post: Posteriors,
    frames: usize,
    env: Vec<f32>,
    onset: Vec<f32>,
}

/// Ce que rend le fil : la source (detruite sur le fil principal), l'analyse.
struct Out {
    src: Option<CppSource>,
    result: Result<(Posteriors, Analysis), String>,
}

pub(crate) struct Job {
    handle: Option<JoinHandle<Out>>,
    progress: Arc<Progress>,
    item: String,
    take: String,
    akey: String,
    /// L'analyse porte sur tout le fichier ; l'item en prend sa tranche.
    fkey: String,
    src: Src,
    name: String,
    started: Instant,
    last_pct: i32,
}

pub(crate) struct GPart {
    pub arr: Arr,
    pub win: Option<GWin>,
    pub place: Place,
    anas: HashMap<String, Ana>,
    /// Les analyses de fichiers entiers, par [`file_key`].
    files: HashMap<String, Analysis>,
    job: Option<Job>,
    /// Les prises a transcrire : (item, prise).
    queue: VecDeque<(String, String)>,
    /// Les prises dont la transcription a echoue (et pourquoi).
    failed: HashMap<String, String>,
    /// L'empreinte de chaque item MIDI telle qu'on l'a ecrite ou lue.
    midi_seen: HashMap<String, String>,
    /// Les items a reconstruire au prochain rafraichissement.
    force: HashSet<String>,
    /// ... et a redecoder avec leur analyse neuve (puis reecrire).
    redecode: HashSet<String>,
    since_sync: u32,
    recording: bool,
    after_rec: u32,
    /// Un item a choisir des que possible (apres un rechargement).
    pub pending: Option<String>,
    last_sel: Option<String>,
    /// La migration des pistes de la V1 est faite pour ce projet.
    migrated: Option<String>,
    synth_ok: bool,
    /// L'auto-test attend la transcription de cette prise.
    selftest_take: Option<String>,
}

impl GPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> GPart {
        let mut arr = Arr::new();
        arr.listen = Listen::from_key(&r.ext_get(SECTION, "gtab_listen").unwrap_or_default());
        arr.split = r.ext_get(SECTION, "gtab_split").as_deref() == Some("1");
        GPart {
            arr,
            win: None,
            place: r
                .ext_get(SECTION, "gtab_place")
                .and_then(|v| Place::from_key(&v))
                .unwrap_or(Place::Arrange),
            anas: HashMap::new(),
            files: HashMap::new(),
            job: None,
            queue: VecDeque::new(),
            failed: HashMap::new(),
            midi_seen: HashMap::new(),
            force: HashSet::new(),
            redecode: HashSet::new(),
            since_sync: 1000,
            recording: false,
            after_rec: 0,
            pending: None,
            last_sel: None,
            migrated: None,
            synth_ok: false,
            selftest_take: None,
        }
    }
}

pub(crate) const LOOK: [&str; 8] = [
    "genlist_bg",
    "genlist_fg",
    "genlist_selbg",
    "col_main_bg2",
    "col_main_text",
    "col_main_text2",
    "col_main_editbk",
    "col_toolbar_text_on",
];

pub(crate) fn selftest() -> bool {
    std::env::var("CP_STUDIO_SELFTEST").is_ok()
}

fn work(mut src: CppSource, item: ItemAudio, progress: Arc<Progress>) -> Out {
    let sr = src.sample_rate();
    let nch = src.channels().max(1) as usize;
    if sr <= 0.0 {
        return Out {
            src: Some(src),
            result: Err("the file has no sample rate".into()),
        };
    }
    // La section jouee par l'item, en secondes du fichier.
    let want = (item.len * item.rate * sr).max(0.0) as usize;
    let block = 65536usize;
    let mut buf = vec![0f64; block * nch];
    let mut mono: Vec<f32> = Vec::with_capacity(want);
    let mut at = 0usize;
    while at < want {
        if progress.cancel.load(Ordering::Relaxed) {
            return Out {
                src: Some(src),
                result: Err("cancelled".into()),
            };
        }
        let n = block.min(want - at);
        let t = item.offs + at as f64 / sr;
        let got = src.read(t, sr, nch, &mut buf[..n * nch]);
        for f in 0..got {
            let mut s = 0f64;
            for c in 0..nch {
                s += buf[f * nch + c];
            }
            mono.push((s / nch as f64) as f32);
        }
        if got < n {
            break;
        }
        at += n;
    }
    let audio = cp_tab::resample::resample(&mono, sr, SR as f64);
    drop(mono);
    let Some(mut post) = analyze(&audio, 0, &progress) else {
        return Out {
            src: Some(src),
            result: Err("cancelled".into()),
        };
    };
    let an = Analysis::new(&post, &audio);
    // Le contour (les bends) n'est pas garde : pas utilise, et le plus gros.
    post.contour = Vec::new();
    Out {
        src: Some(src),
        result: Ok((post, an)),
    }
}

/// La cle d'un son : le fichier, le decalage, la longueur, la vitesse (FNV-1a).
/// Le son qu'un item joue dans son fichier.
struct Src {
    path: String,
    /// Decalage dans le fichier (s), longueur de l'item (s), vitesse.
    offs: f64,
    len: f64,
    rate: f64,
}

/// La cle de l'analyse d'un FICHIER : son chemin, sa taille et sa date (un
/// fichier reecrit est analyse de nouveau).
fn file_key(path: &str) -> String {
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map_or(0, |m| m.len());
    let date = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let mut h: u64 = 0xcbf29ce484222325;
    let s = format!("{}|{size}|{date}", path.to_lowercase());
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("f{h:016x}")
}

/// La tranche de l'analyse d'un fichier qu'un item joue.
fn slice_of(an: &Analysis, src: &Src) -> Analysis {
    let fps = SR as f64 / HOP as f64;
    let f0 = (src.offs.max(0.0) * fps).round() as usize;
    let n = (src.len.max(0.0) * src.rate.max(1e-6) * fps).ceil() as usize;
    an.slice(f0, n)
}

fn ana_of(an: &Analysis) -> Ana {
    Ana {
        post: an.posteriors(),
        frames: an.frames,
        env: an.env.iter().map(|&v| v as f32 / 255.0).collect(),
        onset: an.onset_curve(),
    }
}

fn sound_key(path: &str, offs: f64, len: f64, rate: f64) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    let s = format!("{}|{:.4}|{:.4}|{:.6}", path.to_lowercase(), offs, len, rate);
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("s{h:016x}")
}

fn db_of(vol: f64) -> f32 {
    if vol <= 0.000001 {
        -150.0
    } else {
        (20.0 * vol.log10()) as f32
    }
}

fn vol_of(db: f32) -> f64 {
    if db <= -60.0 {
        0.0
    } else {
        10f64.powf(db as f64 / 20.0)
    }
}

/// « In 1 », « In 1/2 », « MIDI », « None ».
fn input_label(v: i32) -> String {
    if v < 0 {
        return "None".into();
    }
    if v & 4096 != 0 {
        return "MIDI".into();
    }
    let i = (v & 1023) + 1;
    if v & 1024 != 0 {
        format!("In {i}/{}", i + 1)
    } else if v & 2048 != 0 {
        format!("In {i}+")
    } else {
        format!("In {i}")
    }
}

impl App {
    // -----------------------------------------------------------------------
    // Les fichiers
    // -----------------------------------------------------------------------

    fn gtab_dir(&self) -> PathBuf {
        if selftest() {
            return std::env::temp_dir().join("cp_selftest").join("Guitar Tab");
        }
        self.r
            .project_dir()
            .or_else(|| self.r.resource_path().map(|p| p.join("CP_Studio")))
            .unwrap_or_default()
            .join("Guitar Tab")
    }

    /// Retient un reglage (jamais pendant l'auto-test : il tourne avec la
    /// configuration de Cedric).
    fn gtab_keep(&self, key: &str, value: &str) {
        if !selftest() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    fn gtab_cache(&self, name: &str, ext: &str) -> PathBuf {
        let n: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.gtab_dir().join("cache").join(format!("{n}.{ext}"))
    }

    /// L'analyse d'un son : en memoire, sinon en cache (ou dans l'ancien cache
    /// de la V1, par GUID d'item).
    /// L'analyse du son d'un item. D'abord celle de tout son FICHIER, dont on
    /// tire la tranche que l'item joue : couper, rogner ou deplacer un item ne
    /// relance donc jamais le reseau. Sinon l'ancien cache par son d'item.
    fn gtab_ana(&mut self, akey: &str, item: &str, src: &Src) -> bool {
        if self.gt.anas.contains_key(akey) {
            return true;
        }
        let read = |p: PathBuf| std::fs::read(p).ok().and_then(|b| Analysis::from_bytes(&b));
        let fkey = file_key(&src.path);
        if !self.gt.files.contains_key(&fkey) {
            if let Some(an) = read(self.gtab_cache(&fkey, "cpga")) {
                self.gt.files.insert(fkey.clone(), an);
            }
        }
        let an = match self.gt.files.get(&fkey) {
            Some(f) => Some(slice_of(f, src)),
            None => {
                read(self.gtab_cache(akey, "cpga")).or_else(|| read(self.gtab_cache(item, "cpga")))
            }
        };
        let Some(an) = an else {
            return false;
        };
        self.gt.anas.insert(akey.to_string(), ana_of(&an));
        true
    }

    fn gtab_has_ana(&self, akey: &str, path: &str) -> bool {
        self.gt.anas.contains_key(akey)
            || self.gt.files.contains_key(&file_key(path))
            || self.gtab_cache(&file_key(path), "cpga").exists()
            || self.gtab_cache(akey, "cpga").exists()
    }

    /// La tablature d'une prise, gardee a cote de son analyse.
    fn gtab_save_tab(&self, it: &Item) {
        let txt = doc::tab_to_text(&it.doc, &it.suppressed, &it.detect.to_text(), it.qn0);
        let p = self.gtab_cache(&it.take, "cptab");
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, txt);
    }

    fn gtab_load_tab(&self, take: &str, at_qn: f64) -> Option<doc::TabFile> {
        let s = std::fs::read_to_string(self.gtab_cache(take, "cptab")).ok()?;
        doc::tab_from_text(&s, at_qn)
    }

    // -----------------------------------------------------------------------
    // Le decodage d'un item
    // -----------------------------------------------------------------------

    pub(crate) fn gtab_running(&self) -> bool {
        self.gt.job.is_some() || !self.gt.queue.is_empty()
    }

    /// Les notes du decodage, en noires du projet (`ghost` : le seuil des
    /// fantomes).
    fn gtab_notes(&self, it: &Item, ghost: bool) -> Vec<TabNote> {
        let Some(ana) = self.gt.anas.get(&it.akey) else {
            return Vec::new();
        };
        let d = &it.detect;
        let (lo, hi) = d.tuning().range();
        let prm = NoteParams {
            onset_thresh: if ghost { d.ghost_onset() } else { d.onset },
            frame_thresh: d.frame,
            min_len: cp_tab::notes::min_len_frames(d.min_ms),
            melodia: d.legato,
            min_pitch: lo,
            max_pitch: hi,
            ..NoteParams::default()
        };
        let n = it.frame_qn.len();
        if n == 0 {
            return Vec::new();
        }
        make_notes(&ana.post, &prm)
            .into_iter()
            .filter(|x| x.start < n)
            .map(|x| TabNote {
                qn0: it.frame_qn[x.start],
                qn1: it.frame_qn[x.end.min(n - 1)].max(it.frame_qn[x.start] + 1e-3),
                pitch: x.pitch,
                legato: x.peak == 0.0,
                vel: (40.0 + x.amp.clamp(0.0, 1.0) * 87.0) as u8,
            })
            .collect()
    }

    /// Redecode un item : les corrections restent (notes tenues, notes
    /// effacees) ; `range` : seulement ce passage, dont les notes deviennent
    /// tenues.
    fn gtab_decode(&mut self, t: usize, i: usize, range: Option<(f64, f64)>) {
        let Some(it) = self.gt.arr.item(t, i) else {
            return;
        };
        let tuning = it.detect.tuning();
        let grid = it.detect.grid;
        let notes = self.gtab_notes(it, false);
        let (mut fresh, dropped) = doc::finger(&notes, grid, it.bpm, &tuning);
        if let Some((a, b)) = range {
            for f in &mut fresh {
                if f.qn0 >= a && f.qn0 < b {
                    f.locked = true;
                }
            }
        }
        let merged = doc::merge(&it.doc, fresh, &it.suppressed, range);
        let ghosts = if it.detect.ghosts {
            doc::ghosts(&self.gtab_notes(it, true), &merged, grid, &tuning)
        } else {
            Vec::new()
        };
        if let Some(it) = self.gt.arr.item_mut(t, i) {
            it.dropped = dropped;
            it.ghosts = ghosts;
            it.set_doc(merged);
        }
    }

    /// Les fantomes seuls (la tablature vient du MIDI).
    fn gtab_ghosts(&mut self, t: usize, i: usize) {
        let Some(it) = self.gt.arr.item(t, i) else {
            return;
        };
        let g = if it.detect.ghosts {
            doc::ghosts(
                &self.gtab_notes(it, true),
                &it.doc,
                it.detect.grid,
                &it.detect.tuning(),
            )
        } else {
            Vec::new()
        };
        if let Some(it) = self.gt.arr.item_mut(t, i) {
            it.ghosts = g;
        }
    }

    /// L'accordage d'une piste a change : chaque note tenue garde sa hauteur
    /// sur une corde qui la joue ; le reste est redoigte.
    fn gtab_retune(&mut self, t: usize) {
        let Some(tr) = self.gt.arr.tracks.get(t) else {
            return;
        };
        let k = tr.head.tuning;
        let guid = tr.head.guid.clone();
        self.r.set_track_ext(&guid, "CPGT_tuning", &k.to_string());
        let n = tr.items.len();
        self.r.undo_begin();
        for i in 0..n {
            let Some(it) = self.gt.arr.item_mut(t, i) else {
                continue;
            };
            it.detect.tuning = k;
            let tun = it.detect.tuning();
            let mut kept = Vec::new();
            for mut nt in it.doc.iter().filter(|n| n.locked).cloned() {
                let ok = |s: usize| {
                    tun.strings
                        .get(s)
                        .is_some_and(|&o| (0..=tun.frets).contains(&(nt.pitch - o)))
                };
                if !ok(nt.string) {
                    if let Some(s) = (0..tun.strings.len())
                        .filter(|&s| ok(s))
                        .min_by_key(|&s| (s as i64 - nt.string as i64).abs())
                    {
                        nt.string = s;
                    }
                }
                nt.fret = nt.pitch - tun.strings.get(nt.string).copied().unwrap_or(40);
                kept.push(nt);
            }
            it.set_doc(kept);
            self.gtab_decode(t, i, None);
            self.gtab_write(t, i, "CP Guitar Tab: tuning");
        }
        self.r.undo_end("CP Guitar Tab: tuning", 4);
    }

    // -----------------------------------------------------------------------
    // Les pistes REAPER
    // -----------------------------------------------------------------------

    /// Installe le synthe dans `Effects/CP_Studio/` (une fois par session, et
    /// s'il a change).
    fn gtab_install_synth(&mut self) {
        if self.gt.synth_ok {
            return;
        }
        let Some(res) = self.r.resource_path() else {
            return;
        };
        let dir = res.join("Effects").join("CP_Studio");
        let p = dir.join(SYNTH_FILE);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(SYNTH_SRC) {
            let _ = std::fs::create_dir_all(&dir);
            if std::fs::write(&p, SYNTH_SRC).is_err() {
                self.host
                    .warn("Guitar Tab : le synthe n'a pas pu etre installe");
                return;
            }
        }
        self.gt.synth_ok = true;
    }

    /// La piste tab d'une guitare (sa piste audio `a`) : la cree si elle
    /// manque. Rend son GUID.
    fn gtab_tab_track(&mut self, a: &str) -> Option<String> {
        let r = self.r;
        if let Some(t) = r
            .track_ext(a, "CPGT_tab")
            .filter(|g| r.track_value(g, "IP_TRACKNUMBER").is_some())
        {
            return Some(t);
        }
        let idx = r.track_value(a, "IP_TRACKNUMBER")? as i32 - 1;
        let depth = r.track_value(a, "I_FOLDERDEPTH")? as i32;
        let name = r
            .snapshot_tracks(&[], &[], &|_| false)
            .into_iter()
            .find(|t| t.guid == a)
            .map(|t| t.name)
            .unwrap_or_else(|| "Guitar".into());
        let base = name.trim_end_matches(" \u{b7} Audio").to_string();
        let t = r.insert_track(idx + 1, &format!("{base} \u{b7} Tab"))?;
        // Elle prend la fin de dossier que portait l'audio.
        r.set_track_value(
            &t,
            "I_FOLDERDEPTH",
            if depth == 1 { 0.0 } else { depth as f64 },
        );
        if depth != 1 {
            r.set_track_value(a, "I_FOLDERDEPTH", 0.0);
        }
        r.set_track_ext(&t, "CPGT_role", "tab");
        r.set_track_ext(&t, "CPGT_tab", a);
        r.set_track_ext(a, "CPGT_tab", &t);
        self.gtab_install_synth();
        if self.gt.synth_ok {
            match r.track_fx_add(&t, SYNTH_ADD) {
                Some(fx) => {
                    // Le son de la guitare, une note par corde.
                    r.set_track_fx_param(&t, fx, 0, 0.0);
                    let found = r.track_fx_find(&t, SYNTH_NAME).is_some();
                    self.host.info(&format!(
                        "Guitar Tab : synthe pose sur la piste tab (effet {fx}, nomme : {found})"
                    ));
                }
                None => self
                    .host
                    .warn("Guitar Tab : REAPER n'a pas pu poser le synthe sur la piste tab"),
            }
        }
        Some(t)
    }

    /// Fait d'une piste audio une guitare : un dossier au-dessus d'elle, sa
    /// piste tab dessous. Rend le GUID de la piste audio.
    fn gtab_make_guitar(&mut self, a: &str) -> Option<String> {
        let r = self.r;
        if r.track_ext(a, "CPGT_role").as_deref() == Some("guitar") {
            return Some(a.to_string());
        }
        let idx = r.track_value(a, "IP_TRACKNUMBER")? as i32 - 1;
        let depth = r.track_value(a, "I_FOLDERDEPTH")? as i32;
        let name = r
            .snapshot_tracks(&[], &[], &|_| false)
            .into_iter()
            .find(|t| t.guid == a)
            .map(|t| t.name)
            .unwrap_or_default();
        let name = if name.trim().is_empty() {
            format!("Guitar {}", idx + 1)
        } else {
            name
        };
        r.undo_begin();
        r.set_track_ext(a, "CPGT_role", "guitar");
        if depth == 1 {
            // Deja un dossier : la tab devient son premier enfant.
            self.gtab_tab_track(a);
        } else {
            let f = r.insert_track(idx, &name)?;
            r.set_track_value(&f, "I_FOLDERDEPTH", 1.0);
            r.set_track_ext(&f, "CPGT_role", "bus");
            r.set_track_ext(a, "CPGT_bus", &f);
            r.set_track_value(a, "I_FOLDERDEPTH", depth as f64);
            self.gtab_tab_track(a);
        }
        r.undo_end("CP Guitar Tab: make guitar track", -1);
        r.refresh_arrange();
        Some(a.to_string())
    }

    /// Une guitare neuve a la fin du projet : dossier, audio armee, tab.
    fn gtab_new_guitar(&mut self) -> Option<String> {
        let r = self.r;
        let all = r.snapshot_tracks(&TKEYS, &[], &|_| false);
        let n = all.len() as i32;
        let num = all
            .iter()
            .filter(|t| t.ext[K_ROLE].as_deref() == Some("guitar"))
            .count()
            + 1;
        r.undo_begin();
        let name = format!("Guitar {num}");
        let f = r.insert_track(n, &name)?;
        let a = r.insert_track(n + 1, &format!("{name} \u{b7} Audio"))?;
        r.set_track_value(&f, "I_FOLDERDEPTH", 1.0);
        r.set_track_value(&a, "I_FOLDERDEPTH", -1.0);
        r.set_track_ext(&f, "CPGT_role", "bus");
        r.set_track_ext(&a, "CPGT_role", "guitar");
        r.set_track_ext(&a, "CPGT_bus", &f);
        r.set_track_value(&a, "I_RECINPUT", 0.0);
        r.set_track_value(&a, "I_RECARM", 1.0);
        self.gtab_tab_track(&a);
        r.undo_end("CP Guitar Tab: new guitar track", -1);
        r.refresh_arrange();
        Some(a)
    }

    // -----------------------------------------------------------------------
    // L'item MIDI d'un item
    // -----------------------------------------------------------------------

    /// Ecrit la tablature d'un item dans son item MIDI (le cree s'il manque).
    fn gtab_write(&mut self, t: usize, i: usize, undo: &str) {
        let r = self.r;
        let Some(tr) = self.gt.arr.tracks.get(t) else {
            return;
        };
        let a = tr.head.guid.clone();
        let Some(it) = tr.items.get(i).cloned() else {
            return;
        };
        let Some(tab) = self.gtab_tab_track(&a) else {
            return;
        };
        let mut midi = r
            .item_ext(&it.guid, "CPGT_midi")
            .filter(|g| r.item_ext(g, "CPGT_src").as_deref() == Some(it.guid.as_str()));
        if midi.is_none() {
            let (t0, t1) = (r.qn_to_time(it.qn0), r.qn_to_time(it.qn1));
            midi = r.create_midi_item_on(&tab, &format!("Tab: {}", it.name), t0, t1);
            if let Some(g) = &midi {
                r.set_item_ext(&it.guid, "CPGT_midi", g);
                r.set_item_ext(g, "CPGT_src", &it.guid);
                if let Some(a) = r.item_audio(&it.guid) {
                    r.set_item_ext(g, "CPGT_aoffs", &format!("{:.9}", a.offs));
                }
            }
        }
        let Some(g) = midi else {
            self.gt.arr.status = "The MIDI item could not be created".into();
            return;
        };
        let tuning = it.detect.tuning();
        let notes: Vec<QnNote> = doc::to_midi(&it.doc, &tuning)
            .into_iter()
            .map(|m| QnNote {
                qn0: m.qn0,
                qn1: m.qn1,
                pitch: m.pitch,
                chan: m.chan as i32,
                vel: m.vel as i32,
            })
            .collect();
        r.set_midi_notes(&g, &notes, undo);
        r.set_item_ext(&g, "CPGT_take", &it.take);
        if let Some(h) = r.midi_hash(&g) {
            self.gt.midi_seen.insert(g.clone(), h);
        }
        self.gtab_save_tab(&it);
        if let Some(x) = self.gt.arr.item_mut(t, i) {
            x.midi = true;
        }
    }

    /// La tablature lue dans l'item MIDI (Cedric l'a modifie dans REAPER).
    fn gtab_doc_from_midi(&self, midi: &str, it: &Item) -> Option<Vec<cp_tab::doc::TNote>> {
        let notes = self.r.midi_notes(midi)?;
        let side = match self.gtab_load_tab(&it.take, it.qn0) {
            Some((d, sup, _)) => Sidecar::of(&d, &sup),
            None => Sidecar::from_text(&self.r.item_ext(midi, "CPGT_side").unwrap_or_default()),
        };
        // Seules les notes que l'item joue (un rognage cache les autres).
        let m: Vec<MidiQn> = notes
            .iter()
            .filter(|n| n.qn0 >= it.qn0 - 1e-6 && n.qn0 < it.qn1 - 1e-6)
            .map(|n| MidiQn {
                qn0: n.qn0,
                qn1: n.qn1,
                pitch: n.pitch,
                chan: n.chan.clamp(0, 15) as u8,
                vel: n.vel.clamp(1, 127) as u8,
            })
            .collect();
        Some(doc::from_midi(&m, &it.detect.tuning(), &side))
    }

    // -----------------------------------------------------------------------
    // Le rafraichissement : REAPER -> l'arrangeur
    // -----------------------------------------------------------------------

    /// Relit les pistes de guitare, entretient les items MIDI (aucune dette),
    /// met en file ce qui n'est pas transcrit.
    pub(crate) fn gtab_sync(&mut self) {
        let r = self.r;
        self.gtab_migrate();
        let snap = r.snapshot_tracks(&TKEYS, &IKEYS, &|e| {
            matches!(e[K_ROLE].as_deref(), Some("guitar") | Some("tab"))
        });
        let by: HashMap<&str, &TrackSnap> = snap.iter().map(|t| (t.guid.as_str(), t)).collect();
        let (bq, sig, bpm) = r.measure_at(0.0);
        let _ = bq;
        self.gt.arr.sig = sig;
        self.gt.arr.bpm = bpm;
        // Les items d'avant, par GUID : on les garde s'ils n'ont pas change.
        let mut old: HashMap<String, Item> = HashMap::new();
        let mut old_heads: HashMap<String, Head> = HashMap::new();
        for tr in std::mem::take(&mut self.gt.arr.tracks) {
            for it in tr.items {
                old.insert(it.guid.clone(), it);
            }
            old_heads.insert(tr.head.guid.clone(), tr.head);
        }
        let mut live: HashSet<String> = HashSet::new();
        let mut tracks: Vec<Track> = Vec::new();
        let mut to_write: Vec<(String, String)> = Vec::new();
        let mut to_fix: Vec<(String, f64, f64, i32)> = Vec::new();
        let mut to_offs: Vec<(String, f64)> = Vec::new();
        let mut to_mark: Vec<(String, f64)> = Vec::new();
        let mut todos: Vec<(String, Todo)> = Vec::new();
        for a in snap
            .iter()
            .filter(|t| t.ext[K_ROLE].as_deref() == Some("guitar"))
        {
            let tab = a.ext[K_TAB].as_deref().and_then(|g| by.get(g)).copied();
            let bus = a.ext[K_BUS].as_deref().and_then(|g| {
                by.get(g)
                    .copied()
                    .or_else(|| snap.iter().find(|t| t.guid == g))
            });
            let prev = old_heads.get(&a.guid);
            let synth = tab.and_then(|t| self.gtab_synth_of(&t.guid));
            let head = Head {
                guid: a.guid.clone(),
                tab_guid: tab.map(|t| t.guid.clone()),
                num: bus.unwrap_or(a).index + 1,
                name: bus.map_or_else(|| a.name.clone(), |b| b.name.clone()),
                armed: a.armed,
                input: input_label(a.input),
                monitor: a.monitor,
                mute: bus.map_or(a.mute, |b| b.mute),
                solo: bus.map_or(a.solo, |b| b.solo),
                vol_db: db_of(a.vol),
                tab_vol_db: tab.map_or(0.0, |t| db_of(t.vol)),
                meter: prev.map_or(0.0, |h| h.meter),
                synth,
                tuning: a.ext[K_TUNING]
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0)
                    .min(cp_tabview::TUNINGS.len() - 1),
                lanes: a.ext[K_LANES]
                    .as_deref()
                    .map(Lanes::from_text)
                    .unwrap_or_default(),
            };
            // Les items MIDI de la piste tab, par GUID.
            let tab_items: HashMap<&str, &ItemSnap> = tab
                .map(|t| t.items.iter().map(|i| (i.guid.as_str(), i)).collect())
                .unwrap_or_default();
            let mut items = Vec::new();
            for is in &a.items {
                let Some(tk) = is.take() else {
                    continue;
                };
                if tk.midi || tk.path.is_empty() {
                    continue;
                }
                live.insert(is.guid.clone());
                // L'item MIDI, s'il est vraiment le sien.
                let midi = is.ext[I_MIDI]
                    .as_deref()
                    .and_then(|g| tab_items.get(g).copied())
                    .filter(|m| m.ext[I_SRC].as_deref() == Some(is.guid.as_str()));
                let (it, mut todo) = self.gtab_item(&head, is, tk, midi, old.remove(&is.guid));
                let redo = it.state == Some(State::Ready) && self.gt.redecode.remove(&is.guid);
                if redo {
                    todo = Todo::Decode;
                }
                if todo != Todo::Nothing {
                    todos.push((is.guid.clone(), todo));
                }
                if let Some(m) = midi {
                    // L'item MIDI suit son item audio (place, lane).
                    if (m.pos - is.pos).abs() > 1e-6
                        || (m.len - is.len).abs() > 1e-6
                        || m.lane != is.lane
                    {
                        to_fix.push((m.guid.clone(), is.pos, is.len, is.lane));
                    }
                    // Le debut de l'audio rogne : le MIDI se decale dans sa
                    // source d'autant, ses notes restent ou elles sonnent.
                    match m.ext[I_AOFFS]
                        .as_deref()
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        Some(a0) => {
                            let want = ((tk.offs - a0) / tk.rate.max(1e-6)).max(0.0);
                            let cur = m.take().map_or(0.0, |x| x.offs);
                            if (want - cur).abs() > 1e-6 {
                                to_offs.push((m.guid.clone(), want));
                            }
                        }
                        None => {
                            let cur = m.take().map_or(0.0, |x| x.offs);
                            to_mark.push((m.guid.clone(), tk.offs - cur * tk.rate));
                        }
                    }
                }
                let needs_midi = it.state == Some(State::Ready)
                    && (redo
                        || midi.is_none()
                        || midi.and_then(|m| m.ext[I_TAKE].as_deref()) != Some(tk.guid.as_str()));
                if needs_midi {
                    to_write.push((is.guid.clone(), tk.guid.clone()));
                }
                items.push(it);
            }
            items.sort_by(|x, y| x.qn0.total_cmp(&y.qn0));
            // Les lanes fixes : la piste tab suit la piste audio.
            if let Some(t) = tab {
                if a.freemode == 2 && (t.freemode != 2 || t.lanes != a.lanes) {
                    r.set_track_value(&t.guid, "I_FREEMODE", 2.0);
                    r.set_track_value(&t.guid, "I_NUMFIXEDLANES", a.lanes as f64);
                }
                if a.freemode == 2 && t.lane_plays != a.lane_plays {
                    for (k, &p) in a.lane_plays.iter().enumerate() {
                        r.set_track_value(&t.guid, &format!("C_LANEPLAYS:{k}"), p as f64);
                    }
                }
            }
            tracks.push(Track { head, items });
        }
        // Aucune dette : un item MIDI dont l'audio n'est plus la s'en va (sans
        // pas d'annulation : il se refait depuis sa tablature gardee si
        // l'audio revient).
        let mut cleaned = 0;
        for t in snap
            .iter()
            .filter(|t| t.ext[K_ROLE].as_deref() == Some("tab"))
        {
            for m in &t.items {
                let Some(src) = m.ext[I_SRC].as_deref() else {
                    continue;
                };
                let mine = snap
                    .iter()
                    .flat_map(|x| x.items.iter())
                    .find(|x| x.guid == src)
                    .is_some_and(|x| x.ext[I_MIDI].as_deref() == Some(m.guid.as_str()));
                if !live.contains(src) || !mine {
                    r.delete_item(&m.guid);
                    self.gt.midi_seen.remove(&m.guid);
                    cleaned += 1;
                }
            }
        }
        for (g, pos, len, lane) in to_fix {
            r.set_item_value(&g, "D_POSITION", pos);
            r.set_item_value(&g, "D_LENGTH", len);
            r.set_item_value(&g, "I_FIXEDLANE", lane as f64);
        }
        for (g, offs) in to_offs {
            r.set_active_take_value(&g, "D_STARTOFFS", offs);
        }
        for (g, a0) in to_mark {
            r.set_item_ext(&g, "CPGT_aoffs", &format!("{a0:.9}"));
        }
        if cleaned > 0 {
            r.refresh_arrange();
        }
        self.gt.arr.tracks = tracks;
        self.gt.arr.reselect();
        // Les items reconstruits : leurs fantomes, ou un decodage neuf.
        for (g, todo) in todos {
            if let Some((t, i)) = self.gtab_find(&g) {
                match todo {
                    Todo::Decode => self.gtab_decode(t, i, None),
                    Todo::Ghosts => self.gtab_ghosts(t, i),
                    Todo::Nothing => {}
                }
            }
        }
        // Les MIDI a ecrire (transcriptions finies, prises changees, copies).
        for (ig, tg) in to_write {
            if let Some((t, i)) = self.gtab_find(&ig) {
                if self.gt.arr.tracks[t].items[i].take == tg {
                    self.gtab_write(t, i, "CP Guitar Tab: transcription");
                }
            }
        }
        // Les items a transcrire, dans l'ordre du temps.
        if !self.gt.recording {
            for tr in &self.gt.arr.tracks {
                for it in &tr.items {
                    if it.state == Some(State::Waiting)
                        && !self.gt.queue.iter().any(|(_, t)| *t == it.take)
                        && self.gt.job.as_ref().is_none_or(|j| j.take != it.take)
                    {
                        self.gt.queue.push_back((it.guid.clone(), it.take.clone()));
                    }
                }
            }
        }
        self.gt.arr.mark();
    }

    /// Le synthe de la piste tab, s'il y est.
    fn gtab_synth_of(&self, tab: &str) -> Option<Synth> {
        let r = self.r;
        let fx = r.track_fx_find(tab, SYNTH_NAME)?;
        let p = |k: i32| r.track_fx_param(tab, fx, k).unwrap_or(0.0);
        Some(Synth {
            sound: Sound::from_index(p(0).round().max(0.0) as usize),
            tone: p(1) as f32,
            sustain: p(2) as f32,
            mono: p(3) >= 0.5,
        })
    }

    /// Un item de l'arrangeur, construit (ou garde) depuis REAPER.
    fn gtab_item(
        &mut self,
        head: &Head,
        is: &ItemSnap,
        tk: &TakeSnap,
        midi: Option<&ItemSnap>,
        old: Option<Item>,
    ) -> (Item, Todo) {
        let r = self.r;
        let qn0 = r.time_to_qn(is.pos);
        let qn1 = r.time_to_qn(is.pos + is.len);
        let akey = sound_key(&tk.path, tk.offs, is.len, tk.rate);
        let midi_hash = midi.and_then(|m| m.take()).and_then(|t| t.hash.clone());
        let midi_take_ok = midi.and_then(|m| m.ext[I_TAKE].as_deref()) == Some(tk.guid.as_str());
        let forced = self.gt.force.remove(&is.guid);
        // L'item n'a pas change : on le garde tel quel (le cas courant).
        if let Some(mut o) = old {
            let same = o.take == tk.guid
                && o.akey == akey
                && (o.qn0 - qn0).abs() < 1e-6
                && (o.qn1 - qn1).abs() < 1e-6
                && o.detect.tuning == head.tuning;
            let midi_same = match (&midi_hash, midi.map(|m| &m.guid)) {
                (Some(h), Some(g)) => self.gt.midi_seen.get(g) == Some(h),
                _ => true,
            };
            if same && !forced && (o.state != Some(State::Ready) || midi_same || !midi_take_ok) {
                let was = o.state.clone();
                o.state = self.gtab_state(&o.take, &akey).or(o.state);
                // Il vient d'etre transcrit (analyse chargee ailleurs).
                if was != Some(State::Ready) && o.state == Some(State::Ready) {
                    self.gt.force.insert(is.guid.clone());
                }
                return (o, Todo::Nothing);
            }
            if same && !forced && midi_take_ok {
                // Modifie dans REAPER : relu.
                if let (Some(m), Some(d)) = (
                    midi,
                    self.gtab_doc_from_midi(&midi.map(|m| m.guid.clone()).unwrap_or_default(), &o),
                ) {
                    o.set_doc(d);
                    if let Some(h) = midi_hash {
                        self.gt.midi_seen.insert(m.guid.clone(), h);
                    }
                    self.gt.force.remove(&is.guid);
                    return (o, Todo::Ghosts);
                }
            }
        }
        let (origin_qn, sig, bpm) = r.measure_at(is.pos);
        let takes: Vec<TakeInfo> = is
            .takes
            .iter()
            .map(|x| TakeInfo {
                name: take_name(&x.name),
                done: !x.midi
                    && self.gtab_has_ana(&sound_key(&x.path, x.offs, is.len, x.rate), &x.path),
            })
            .collect();
        let mut it = Item::new(is.guid.clone(), tk.guid.clone(), akey.clone());
        it.name = take_name(&tk.name);
        it.qn0 = qn0;
        it.qn1 = qn1;
        it.origin_qn = origin_qn;
        it.sig = sig;
        it.bpm = bpm;
        it.takes = takes;
        it.active = is.active;
        it.midi = midi.is_some();
        it.detect.tuning = head.tuning;
        let src = Src {
            path: tk.path.clone(),
            offs: tk.offs,
            len: is.len,
            rate: tk.rate,
        };
        let ready = self.gtab_ana(&akey, &is.guid, &src);
        if !ready {
            it.state = self.gtab_state(&tk.guid, &akey).or(Some(State::Waiting));
            return (it, Todo::Nothing);
        }
        it.state = Some(State::Ready);
        // Le temps de chaque trame de l'analyse.
        if let Some(ana) = self.gt.anas.get(&akey) {
            it.frame_qn = (0..ana.frames)
                .map(|f| r.time_to_qn(is.pos + (f * HOP) as f64 / SR as f64 / tk.rate))
                .collect();
            it.env = ana.env.clone();
            it.onset = ana.onset.clone();
        }
        // La tablature : le MIDI s'il est celui de cette prise, sinon celle
        // gardee pour la prise, sinon un decodage neuf.
        let kept = self.gtab_load_tab(&tk.guid, qn0);
        if let Some((_, sup, det)) = &kept {
            it.suppressed = sup.clone();
            if !det.is_empty() {
                it.detect = Detect::from_text(det);
                it.detect.tuning = head.tuning;
            }
        } else if let Some(d) = midi.and_then(|m| r.item_ext(&m.guid, "CPGT_detect")) {
            it.detect = Detect::from_text(&d);
            it.detect.tuning = head.tuning;
        }
        let from_midi = if midi_take_ok
            || (midi.is_some() && midi.and_then(|m| m.ext[I_TAKE].as_deref()).is_none())
        {
            midi.and_then(|m| self.gtab_doc_from_midi(&m.guid, &it))
        } else {
            None
        };
        // Les fantomes (et le decodage neuf) se font une fois l'item en place.
        match (from_midi, kept) {
            (Some(d), _) => {
                it.set_doc(d);
                if let (Some(m), Some(h)) = (midi, midi_hash) {
                    self.gt.midi_seen.insert(m.guid.clone(), h);
                }
                (it, Todo::Ghosts)
            }
            (None, Some((d, _, _))) => {
                it.set_doc(d);
                (it, Todo::Ghosts)
            }
            (None, None) => (it, Todo::Decode),
        }
    }

    /// Ou en est la transcription d'une prise (None : rien de special).
    fn gtab_state(&self, take: &str, akey: &str) -> Option<State> {
        if let Some(j) = &self.gt.job {
            if j.take == take {
                return Some(State::Busy(j.last_pct.max(0)));
            }
        }
        if let Some(e) = self.gt.failed.get(take) {
            return Some(State::Failed(e.clone()));
        }
        if self.gt.anas.contains_key(akey) {
            return Some(State::Ready);
        }
        None
    }

    fn gtab_find(&self, item: &str) -> Option<(usize, usize)> {
        self.gt
            .arr
            .tracks
            .iter()
            .enumerate()
            .find_map(|(t, tr)| tr.items.iter().position(|i| i.guid == item).map(|i| (t, i)))
    }

    /// Les pistes de la V1 (« Guitar Tab: <nom> », une par transcription) :
    /// leurs items MIDI rejoignent la piste tab de leur guitare, puis les
    /// pistes vides s'en vont. Une fois par projet.
    fn gtab_migrate(&mut self) {
        let r = self.r;
        let proj = r
            .project_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if self.gt.migrated.as_deref() == Some(proj.as_str()) {
            return;
        }
        self.gt.migrated = Some(proj);
        let snap = r.snapshot_tracks(&TKEYS, &IKEYS, &|_| true);
        let olds: Vec<&TrackSnap> = snap
            .iter()
            .filter(|t| {
                t.ext[K_ROLE].is_none()
                    && t.name.starts_with("Guitar Tab:")
                    && t.items.iter().any(|i| i.ext[I_SRC].is_some())
            })
            .collect();
        if olds.is_empty() {
            return;
        }
        r.undo_begin();
        let mut moved = 0;
        for old in &olds {
            for m in &old.items {
                let Some(src) = m.ext[I_SRC].as_deref() else {
                    continue;
                };
                let Some(at) = snap.iter().find(|t| t.items.iter().any(|i| i.guid == src)) else {
                    continue;
                };
                let Some(a) = self.gtab_make_guitar(&at.guid) else {
                    continue;
                };
                let Some(tab) = self.gtab_tab_track(&a) else {
                    continue;
                };
                if r.move_item_to_track(&m.guid, &tab) {
                    let take = at
                        .items
                        .iter()
                        .find(|i| i.guid == src)
                        .and_then(|i| i.take())
                        .map(|t| t.guid.clone())
                        .unwrap_or_default();
                    r.set_item_ext(&m.guid, "CPGT_take", &take);
                    moved += 1;
                }
            }
        }
        for old in &olds {
            let empty = r
                .snapshot_tracks(&[], &[], &|_| true)
                .into_iter()
                .find(|t| t.guid == old.guid)
                .is_some_and(|t| t.items.is_empty());
            if empty {
                r.delete_track(&old.guid);
            }
        }
        r.undo_end("CP Guitar Tab: tidy the old tab tracks", -1);
        r.refresh_arrange();
        self.host.info(&format!(
            "Guitar Tab : {moved} items MIDI de la V1 ranges sous leur guitare"
        ));
    }

    // -----------------------------------------------------------------------
    // La transcription
    // -----------------------------------------------------------------------

    /// Lance le reseau sur la prochaine prise de la file.
    fn gtab_next_job(&mut self) {
        while self.gt.job.is_none() {
            let Some((item, take)) = self.gt.queue.pop_front() else {
                return;
            };
            let Some(snap_take) = self
                .gtab_find(&item)
                .and_then(|(t, i)| self.gt.arr.item(t, i))
                .filter(|x| x.take == take)
                .map(|x| (x.akey.clone(), x.name.clone()))
            else {
                continue;
            };
            let Some(audio) = self.r.item_audio(&item) else {
                continue;
            };
            let Some(mut src) = cp_sys::audio::open_file(&self.r, &audio.path) else {
                self.gt
                    .failed
                    .insert(take, "REAPER cannot open the file".into());
                continue;
            };
            // Tout le fichier : chaque item qui en joue une partie (apres une
            // coupe, un rognage) y prendra sa tranche sans relancer le reseau.
            let whole = src.length();
            let progress = Arc::new(Progress::default());
            let p2 = progress.clone();
            let a2 = if whole > 0.0 {
                ItemAudio {
                    offs: 0.0,
                    len: whole,
                    rate: 1.0,
                    ..audio.clone()
                }
            } else {
                audio.clone()
            };
            let item_src = Src {
                path: audio.path.clone(),
                offs: if whole > 0.0 { audio.offs } else { 0.0 },
                len: audio.len,
                rate: audio.rate,
            };
            let handle = std::thread::Builder::new()
                .name("cp_guitar_tab".into())
                .spawn(move || work(src, a2, p2))
                .ok();
            if handle.is_none() {
                self.host
                    .error("CP Guitar Tab : le fil de travail n'a pas demarre");
                return;
            }
            self.host.info(&format!(
                "Guitar Tab : transcription de \u{ab} {} \u{bb} ({:.1} s)",
                snap_take.1, audio.len
            ));
            self.gt.job = Some(Job {
                handle,
                progress,
                item,
                take,
                akey: snap_take.0,
                fkey: file_key(&audio.path),
                src: item_src,
                name: snap_take.1,
                started: Instant::now(),
                last_pct: 0,
            });
            self.gtab_set_state();
        }
    }

    /// Recopie l'etat de la file dans les items.
    fn gtab_set_state(&mut self) {
        let busy = self.gt.job.as_ref().map(|j| (j.take.clone(), j.last_pct));
        for tr in &mut self.gt.arr.tracks {
            for it in &mut tr.items {
                if it.state == Some(State::Ready) {
                    continue;
                }
                it.state = Some(match &busy {
                    Some((t, p)) if *t == it.take => State::Busy(*p),
                    _ => match self.gt.failed.get(&it.take) {
                        Some(e) => State::Failed(e.clone()),
                        None => State::Waiting,
                    },
                });
            }
        }
        self.gt.arr.busy = self.gt.job.as_ref().map(|j| {
            let more = self.gt.queue.len();
            if more > 0 {
                format!(
                    "Transcribing \u{201c}{}\u{201d}\u{2026} {} % (+{more})",
                    j.name, j.last_pct
                )
            } else {
                format!(
                    "Transcribing \u{201c}{}\u{201d}\u{2026} {} %",
                    j.name, j.last_pct
                )
            }
        });
        self.gt.arr.mark();
    }

    /// La transcription est finie : l'analyse en cache, la tablature, le MIDI.
    fn gtab_finish(&mut self, job: Job, post: Posteriors, an: Analysis) {
        let elapsed = job.started.elapsed().as_secs_f64();
        drop(post);
        let path = self.gtab_cache(&job.fkey, "cpga");
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&path, an.to_bytes());
        let mine = slice_of(&an, &job.src);
        self.gt.files.insert(job.fkey.clone(), an);
        self.gt.anas.insert(job.akey.clone(), ana_of(&mine));
        self.gt.failed.remove(&job.take);
        // Le decodage avec la nouvelle analyse (les corrections tenues
        // restent), puis le MIDI : par le rafraichissement.
        self.gt.force.insert(job.item.clone());
        self.gt.redecode.insert(job.item.clone());
        self.gtab_sync();
        if let Some((t, i)) = self.gtab_find(&job.item) {
            let it = &self.gt.arr.tracks[t].items[i];
            self.host.info(&format!(
                "Guitar Tab : {} notes en {elapsed:.1} s, {} fantomes, MIDI {}",
                it.doc.len(),
                it.ghosts.len(),
                if it.midi { "pose" } else { "ABSENT" }
            ));
            if selftest() && self.gt.selftest_take.as_deref() == Some(job.take.as_str()) {
                self.gtab_selftest_report(t, i);
            }
        }
        self.gtab_set_state();
        self.gtab_render(true);
    }

    /// Ce que l'auto-test lit : les notes, la partition ecrite a part.
    fn gtab_selftest_report(&mut self, t: usize, i: usize) {
        let it = self.gt.arr.tracks[t].items[i].clone();
        let (file, st) = self.gtab_export_item(&it);
        let pitches: Vec<String> = it
            .doc
            .iter()
            .take(16)
            .map(|n| n.pitch.to_string())
            .collect();
        let line = format!(
            "notes={} chords={} measures={} dropped={} midi={} pitches={} file={}",
            st.notes,
            st.chords,
            st.measures,
            st.dropped,
            it.midi as u8,
            pitches.join(","),
            file.map(|p| p.display().to_string()).unwrap_or_default()
        );
        self.r.ext_set(SECTION, "selftest_gtab", &line, false);
        // L'editeur sur le MIDI du riff : transposer, verifier, annuler.
        let r = self.r;
        let midi = r
            .item_ext(&it.guid, "CPGT_midi")
            .filter(|m| r.item_ext(m, "CPGT_src").as_deref() == Some(it.guid.as_str()));
        let ed = match midi {
            Some(m) => self.ed_selftest(&m),
            None => "pas de MIDI".into(),
        };
        self.r.ext_set(SECTION, "selftest_editor", &ed, false);
    }

    /// Ecrit la partition d'un item (MusicXML) dans `Guitar Tab/<nom>.musicxml`.
    fn gtab_export_item(&self, it: &Item) -> (Option<PathBuf>, cp_tab::score::Stats) {
        let spec = ScoreSpec {
            title: it.name.clone(),
            origin_qn: it.origin_qn,
            sig: it.sig,
            bpm: it.bpm,
            grid: it.detect.grid,
            tuning: it.detect.tuning(),
        };
        let (xml, st) = write_musicxml(&it.doc, &spec, it.dropped);
        let dir = self.gtab_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = free_path(&dir, &clean_name(&it.name));
        (
            std::fs::write(&path, xml.as_bytes())
                .is_ok()
                .then_some(path),
            st,
        )
    }

    // -----------------------------------------------------------------------
    // Les actions
    // -----------------------------------------------------------------------

    /// L'icone et l'action : ouvrir la fenetre. L'item choisi dans REAPER :
    /// sa piste devient une guitare (s'il le faut), et il est transcrit.
    pub(crate) fn gtab_start(&mut self) {
        self.queue(WinOp::GtabOpen);
        let r = self.r;
        let sel = r.selected_item_guid();
        let audio = sel.map(|g| r.item_ext(&g, "CPGT_src").unwrap_or(g));
        if let Some(a) = audio {
            if let Some(tr) = r
                .snapshot_tracks(&[], &[], &|_| true)
                .into_iter()
                .find(|t| {
                    t.items
                        .iter()
                        .any(|i| i.guid == a && i.take().is_some_and(|k| !k.midi))
                })
            {
                self.gtab_make_guitar(&tr.guid);
                if selftest() {
                    // L'auto-test juge le reseau : le meme riff revient a
                    // chaque passage, son analyse en cache est oubliee.
                    if let Some((is, tk)) = tr
                        .items
                        .iter()
                        .find(|i| i.guid == a)
                        .and_then(|i| Some((i, i.take()?)))
                    {
                        let k = sound_key(&tk.path, tk.offs, is.len, tk.rate);
                        self.gt.anas.remove(&k);
                        let fk = file_key(&tk.path);
                        self.gt.files.remove(&fk);
                        let _ = std::fs::remove_file(self.gtab_cache(&fk, "cpga"));
                        let _ = std::fs::remove_file(self.gtab_cache(&k, "cpga"));
                        let _ = std::fs::remove_file(self.gtab_cache(&is.guid, "cpga"));
                        self.gt.selftest_take = Some(tk.guid.clone());
                    }
                }
                self.gt.pending = Some(a);
            }
        }
        self.gtab_sync();
        self.gtab_select_pending();
        self.gt.arr.mark();
    }

    /// L'action : la piste choisie dans REAPER devient une guitare.
    pub(crate) fn gtab_make_selected(&mut self) {
        let r = self.r;
        let sel = r
            .snapshot_tracks(&[], &[], &|_| false)
            .into_iter()
            .find(|t| t.selected);
        match sel {
            Some(t) => {
                self.gtab_make_guitar(&t.guid);
                self.queue(WinOp::GtabOpen);
                self.gtab_sync();
            }
            None => self.gt.arr.status = "Select a track in REAPER first.".into(),
        }
    }

    pub(crate) fn gtab_edit_settings(&mut self) {
        self.queue(WinOp::GtabOpen);
    }

    fn gtab_select_pending(&mut self) {
        let Some(g) = self.gt.pending.clone() else {
            return;
        };
        if let Some((t, i)) = self.gtab_find(&g) {
            self.gt.pending = None;
            self.gt.arr.select(Some(Sel {
                t,
                i: Some(i),
                n: None,
            }));
            let it = &self.gt.arr.tracks[t].items[i];
            let v = self.gt.arr.view_qn().max(8.0);
            if it.qn0 < self.gt.arr.scroll || it.qn0 > self.gt.arr.scroll + v * 0.8 {
                self.gt.arr.scroll = (it.qn0 - 2.0).max(0.0);
            }
        }
    }

    /// Ce qu'on entend : la tab coupee (Original), l'original coupe (Tab), ou
    /// les deux (et, sur demande, l'original a gauche, la tab a droite).
    fn gtab_apply_listen(&mut self) {
        let r = self.r;
        let l = self.gt.arr.listen;
        let split = self.gt.arr.split && l == Listen::Both;
        for tr in &self.gt.arr.tracks {
            let a = &tr.head.guid;
            r.set_track_value(a, "B_MUTE", if l == Listen::Tab { 1.0 } else { 0.0 });
            if let Some(t) = &tr.head.tab_guid {
                r.set_track_value(t, "B_MUTE", if l == Listen::Original { 1.0 } else { 0.0 });
                let saved = r.track_ext(a, "CPGT_pan");
                if split && saved.is_none() {
                    let pa = r.track_value(a, "D_PAN").unwrap_or(0.0);
                    let pt = r.track_value(t, "D_PAN").unwrap_or(0.0);
                    r.set_track_ext(a, "CPGT_pan", &format!("{pa} {pt}"));
                    r.set_track_value(a, "D_PAN", -1.0);
                    r.set_track_value(t, "D_PAN", 1.0);
                } else if !split {
                    if let Some(s) = saved {
                        let v: Vec<f64> = s
                            .split_whitespace()
                            .filter_map(|x| x.parse().ok())
                            .collect();
                        r.set_track_value(a, "D_PAN", v.first().copied().unwrap_or(0.0));
                        r.set_track_value(t, "D_PAN", v.get(1).copied().unwrap_or(0.0));
                        r.set_track_ext(a, "CPGT_pan", "");
                    }
                }
            }
        }
        let _ = K_PAN;
        self.gtab_keep("gtab_listen", l.key());
        self.gtab_keep("gtab_split", if self.gt.arr.split { "1" } else { "0" });
    }

    fn gtab_act(&mut self, a: GAct) {
        let r = self.r;
        let head = |me: &App, t: usize| me.gt.arr.tracks.get(t).map(|x| x.head.clone());
        match a {
            GAct::Seek(q) => r.set_edit_cursor(r.qn_to_time(q)),
            GAct::Play => r.main_on_command(40044), // Transport : lecture/arret
            GAct::Record => r.main_on_command(1013), // Transport : enregistrer
            GAct::Undo => r.main_on_command(40029),
            GAct::Edited { t, i } => self.gtab_write(t, i, "CP Guitar Tab: edit"),
            GAct::Detect { t, i, range } => {
                let k = self
                    .gt
                    .arr
                    .item(t, i)
                    .map(|x| x.akey.clone())
                    .unwrap_or_default();
                let g = self
                    .gt
                    .arr
                    .item(t, i)
                    .map(|x| x.guid.clone())
                    .unwrap_or_default();
                let _ = g;
                if self.gt.anas.contains_key(&k) {
                    self.gtab_decode(t, i, range);
                    self.gtab_write(t, i, "CP Guitar Tab: detection");
                }
            }
            GAct::Retune { t } => self.gtab_retune(t),
            GAct::Transcribe { t, i } => {
                if let Some(it) = self.gt.arr.item(t, i) {
                    let (g, tk) = (it.guid.clone(), it.take.clone());
                    self.gt.failed.remove(&tk);
                    self.gt.anas.remove(&it.akey.clone());
                    let _ = std::fs::remove_file(self.gtab_cache(&it.akey, "cpga"));
                    if let Some(x) = self.gt.arr.item_mut(t, i) {
                        x.state = Some(State::Waiting);
                    }
                    self.gt.queue.retain(|(_, k)| *k != tk);
                    self.gt.queue.push_front((g, tk));
                    self.gtab_next_job();
                }
            }
            GAct::DeleteItem { t, i } => {
                if let Some(it) = self.gt.arr.item(t, i).cloned() {
                    r.undo_begin();
                    if let Some(m) = r
                        .item_ext(&it.guid, "CPGT_midi")
                        .filter(|g| r.item_ext(g, "CPGT_src").as_deref() == Some(it.guid.as_str()))
                    {
                        r.delete_item(&m);
                    }
                    r.delete_item(&it.guid);
                    r.undo_end("CP Guitar Tab: delete item", 4);
                    r.refresh_arrange();
                    self.gt.arr.select(Some(Sel {
                        t,
                        i: None,
                        n: None,
                    }));
                    self.gtab_sync();
                }
            }
            GAct::OpenMidi { t, i } => {
                let g = self.gt.arr.item(t, i).map(|x| x.guid.clone());
                let m = g.and_then(|g| {
                    r.item_ext(&g, "CPGT_midi")
                        .filter(|m| r.item_ext(m, "CPGT_src").as_deref() == Some(g.as_str()))
                });
                // Le MIDI s'ouvre dans l'editeur de CP Studio.
                match m {
                    Some(m) => self.ed_open_item(&m),
                    _ => self.gt.arr.status = "This item has no MIDI yet.".into(),
                }
            }
            GAct::SetTake { t, i, k } => {
                if let Some(g) = self.gt.arr.item(t, i).map(|x| x.guid.clone()) {
                    r.undo_begin();
                    r.set_active_take(&g, k);
                    r.undo_end("CP Guitar Tab: take", 4);
                    r.refresh_arrange();
                    self.gtab_sync();
                }
            }
            GAct::Arm { t } => {
                if let Some(h) = head(self, t) {
                    r.set_track_value(&h.guid, "I_RECARM", if h.armed { 0.0 } else { 1.0 });
                    self.gtab_sync();
                }
            }
            GAct::Monitor { t } => {
                if let Some(h) = head(self, t) {
                    r.set_track_value(&h.guid, "I_RECMON", ((h.monitor + 1) % 3) as f64);
                    self.gtab_sync();
                }
            }
            GAct::Mute { t } | GAct::Solo { t } => {
                if let Some(h) = head(self, t) {
                    let target = r.track_ext(&h.guid, "CPGT_bus").unwrap_or(h.guid.clone());
                    match a {
                        GAct::Mute { .. } => {
                            r.set_track_value(&target, "B_MUTE", if h.mute { 0.0 } else { 1.0 });
                        }
                        _ => {
                            r.set_track_value(&target, "I_SOLO", if h.solo { 0.0 } else { 1.0 });
                        }
                    }
                    self.gtab_sync();
                }
            }
            GAct::InputMenu { t, x, y } => self.gtab_input_menu(t, x, y),
            GAct::Volume { t, tab, db } => {
                if let Some(h) = head(self, t) {
                    let g = if tab {
                        h.tab_guid.clone()
                    } else {
                        Some(h.guid.clone())
                    };
                    if let Some(g) = g {
                        r.set_track_value(&g, "D_VOL", vol_of(db));
                    }
                }
            }
            GAct::Sound { t, s } => {
                if let Some(tab) = head(self, t).and_then(|h| h.tab_guid) {
                    if let Some(fx) = r.track_fx_find(&tab, SYNTH_NAME) {
                        r.set_track_fx_param(&tab, fx, 0, s.index() as f64);
                    }
                }
            }
            GAct::Synth { t, p, v } => {
                if let Some(tab) = head(self, t).and_then(|h| h.tab_guid) {
                    if let Some(fx) = r.track_fx_find(&tab, SYNTH_NAME) {
                        r.set_track_fx_param(&tab, fx, p as i32, v as f64);
                    }
                }
            }
            GAct::Lanes { t } => {
                if let Some(h) = head(self, t) {
                    r.set_track_ext(&h.guid, "CPGT_lanes", &h.lanes.to_text());
                }
            }
            GAct::Listen => self.gtab_apply_listen(),
            GAct::AddTrack => self.gtab_add_menu(),
            GAct::TrackMenu { t, x, y } => self.gtab_track_menu(t, x, y),
            GAct::ViewMenu { x, y } => self.gtab_place_menu(x, y),
            GAct::MoveItem { t, i, qn0, to } => {
                // La prise et son MIDI bougent ensemble, en un pas
                // d'annulation ; vers une autre guitare, le MIDI suit sur sa
                // piste tab.
                let Some(it) = self.gt.arr.item(t, i).cloned() else {
                    return;
                };
                let dest = head(self, to);
                let pos = r.qn_to_time(qn0);
                let midi = r
                    .item_ext(&it.guid, "CPGT_midi")
                    .filter(|g| r.item_ext(g, "CPGT_src").as_deref() == Some(it.guid.as_str()));
                r.undo_begin();
                r.set_item_value(&it.guid, "D_POSITION", pos);
                if let Some(m) = &midi {
                    r.set_item_value(m, "D_POSITION", pos);
                }
                if to != t {
                    if let Some(d) = &dest {
                        r.move_item_to_track(&it.guid, &d.guid);
                        if let (Some(m), Some(tab)) = (&midi, self.gtab_tab_track(&d.guid)) {
                            r.move_item_to_track(m, &tab);
                        }
                    }
                }
                r.undo_end("CP Guitar Tab: move take", -1);
                r.refresh_arrange();
                self.gtab_sync();
            }
            GAct::Export { t, i } => {
                if let Some(it) = self.gt.arr.item(t, i).cloned() {
                    let (p, _) = self.gtab_export_item(&it);
                    self.gt.arr.status = match p {
                        Some(p) => format!("Exported: {}", p.display()),
                        None => "Export failed".into(),
                    };
                }
            }
        }
    }

    fn gtab_menu(&self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let Some(h) = self.gt.win.as_ref().map(|w| w.hwnd) else {
            return 0;
        };
        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
        ui::popup_menu(h, sx, sy, items)
    }

    fn gtab_input_menu(&mut self, t: usize, x: f32, y: f32) {
        let r = self.r;
        let Some(h) = self.gt.arr.tracks.get(t).map(|x| x.head.clone()) else {
            return;
        };
        let cur = r.track_value(&h.guid, "I_RECINPUT").unwrap_or(0.0) as i32;
        let ins = r.audio_inputs();
        let mut labels: Vec<(String, i32)> = Vec::new();
        for (i, n) in ins.iter().enumerate() {
            labels.push((format!("Input {} (mono) \u{2014} {n}", i + 1), i as i32));
        }
        for i in (0..ins.len().saturating_sub(1)).step_by(2) {
            labels.push((
                format!("Inputs {}/{} (stereo)", i + 1, i + 2),
                1024 | i as i32,
            ));
        }
        labels.push(("No input".into(), -1));
        let items: Vec<Option<MenuItem<'_>>> = labels
            .iter()
            .enumerate()
            .map(|(k, (s, v))| {
                Some(MenuItem {
                    id: k as u32 + 1,
                    text: s,
                    checked: *v == cur,
                    enabled: true,
                })
            })
            .collect();
        let pick = self.gtab_menu(x, y, &items);
        if pick > 0 {
            if let Some(&(_, v)) = labels.get(pick as usize - 1) {
                r.set_track_value(&h.guid, "I_RECINPUT", v as f64);
                self.gtab_sync();
            }
        }
    }

    fn gtab_add_menu(&mut self) {
        let r = self.r;
        // La piste choisie dans REAPER, si elle n'est pas deja une guitare.
        let sel = r
            .snapshot_tracks(&TKEYS, &[], &|_| false)
            .into_iter()
            .find(|t| t.selected && t.ext[K_ROLE].is_none());
        let made = match sel {
            Some(s) => {
                let use_it = format!("Use the selected track \u{201c}{}\u{201d}", s.name);
                let items = [
                    Some(MenuItem {
                        id: 1,
                        text: "New guitar track",
                        checked: false,
                        enabled: true,
                    }),
                    Some(MenuItem {
                        id: 2,
                        text: &use_it,
                        checked: false,
                        enabled: true,
                    }),
                ];
                let (x, y) = self
                    .gt
                    .arr
                    .rect_of(cp_tabview::Hit::AddTrack)
                    .map_or((40.0, 40.0), |b| (b.0, b.1 + b.3));
                match self.gtab_menu(x, y, &items) {
                    1 => self.gtab_new_guitar(),
                    2 => self.gtab_make_guitar(&s.guid),
                    _ => None,
                }
            }
            None => self.gtab_new_guitar(),
        };
        if let Some(a) = made {
            self.gtab_sync();
            if let Some(t) = self.gt.arr.tracks.iter().position(|x| x.head.guid == a) {
                self.gt.arr.select(Some(Sel {
                    t,
                    i: None,
                    n: None,
                }));
            }
            self.gtab_apply_listen();
        }
    }

    fn gtab_track_menu(&mut self, t: usize, x: f32, y: f32) {
        let r = self.r;
        let Some(h) = self.gt.arr.tracks.get(t).map(|x| x.head.clone()) else {
            return;
        };
        // Les couches de la piste, cochees quand elles sont montrees.
        let lane = |id: u32, l: Lane| {
            Some(MenuItem {
                id,
                text: l.name(),
                checked: h.lanes.on(l),
                enabled: true,
            })
        };
        let items = [
            lane(10, Lane::Audio),
            lane(11, Lane::Midi),
            lane(12, Lane::Score),
            lane(13, Lane::Tab),
            None,
            Some(MenuItem {
                id: 1,
                text: "Rename\u{2026}",
                checked: false,
                enabled: true,
            }),
            Some(MenuItem {
                id: 2,
                text: "Show in arrange view",
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: 3,
                text: "Delete guitar track",
                checked: false,
                enabled: true,
            }),
        ];
        let bus = r.track_ext(&h.guid, "CPGT_bus");
        match self.gtab_menu(x, y, &items) {
            k @ 10..=13 => {
                if let Some(a) = self.gt.arr.toggle_lane(t, Lane::ALL[(k - 10) as usize]) {
                    self.gtab_act(a);
                }
            }
            1 => {
                if let Some(n) = r.user_inputs("Rename guitar track", 1, "Name", &h.name) {
                    let n = n.trim().to_string();
                    if !n.is_empty() {
                        r.undo_begin();
                        match &bus {
                            Some(b) => {
                                r.set_track_name(b, &n);
                                r.set_track_name(&h.guid, &format!("{n} \u{b7} Audio"));
                            }
                            None => {
                                r.set_track_name(&h.guid, &n);
                            }
                        }
                        if let Some(tg) = &h.tab_guid {
                            r.set_track_name(tg, &format!("{n} \u{b7} Tab"));
                        }
                        r.undo_end("CP Guitar Tab: rename", 1);
                        self.gtab_sync();
                    }
                }
            }
            2 => {
                r.select_only_track(bus.as_deref().unwrap_or(&h.guid));
                let c = self.cmd_view_arrange;
                cp_sys::module::Module::on_action(self, c);
            }
            3 => {
                r.undo_begin();
                if let Some(tg) = &h.tab_guid {
                    r.delete_track(tg);
                }
                r.delete_track(&h.guid);
                if let Some(b) = &bus {
                    r.delete_track(b);
                }
                r.undo_end("CP Guitar Tab: delete guitar track", -1);
                r.refresh_arrange();
                self.gt.arr.select(None);
                self.gtab_sync();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Le battement
    // -----------------------------------------------------------------------

    pub(crate) fn gtab_tick(&mut self) {
        // La transcription en cours.
        if let Some(j) = &mut self.gt.job {
            let (done, total) = (
                j.progress.done.load(Ordering::Relaxed),
                j.progress.total.load(Ordering::Relaxed),
            );
            if let Some(pct) = (done * 100).checked_div(total).map(|p| p as i32) {
                if pct != j.last_pct {
                    j.last_pct = pct;
                    self.gtab_set_state();
                }
            }
            if self
                .gt
                .job
                .as_ref()
                .and_then(|j| j.handle.as_ref())
                .is_some_and(|h| h.is_finished())
            {
                if let Some(mut job) = self.gt.job.take() {
                    let out = job.handle.take().and_then(|h| h.join().ok());
                    match out {
                        Some(Out {
                            src,
                            result: Ok((post, an)),
                        }) => {
                            drop(src);
                            self.gtab_finish(job, post, an);
                        }
                        Some(Out {
                            src,
                            result: Err(e),
                        }) => {
                            drop(src);
                            if e != "cancelled" {
                                self.gt.failed.insert(job.take.clone(), e);
                            }
                            self.gtab_set_state();
                        }
                        None => {
                            self.gt
                                .failed
                                .insert(job.take.clone(), "the analysis stopped".into());
                            self.gtab_set_state();
                        }
                    }
                }
            }
        }
        // Fenetre fermee, Guitar Tab ne fait rien : ni transcription, ni MIDI
        // pose, ni rafraichissement (decide par Cedric le 2026-11-22 : « ca
        // reanalyse tout le temps, meme quand je ne suis pas dans Guitar
        // Tab »). Une transcription deja lancee finit, rien d'autre ne part ;
        // tout se rattrape a l'ouverture.
        let open = self.gt.win.is_some();
        if !open {
            self.gt.recording = false;
            self.gt.arr.rec.clear();
            self.gt.since_sync = 1000;
            return;
        }
        self.gtab_next_job();
        let r = self.r;
        let ps = r.play_state();
        let playing = ps & (play::PLAYING | play::RECORDING) != 0;
        let rec = ps & play::RECORDING != 0;
        // L'enregistrement : une prise rouge qui grandit sur chaque guitare
        // armee ; a l'arret, les prises neuves sont transcrites.
        if rec && !self.gt.recording {
            self.gt.recording = true;
            let q = r.time_to_qn(r.play_position());
            self.gt.arr.rec = self
                .gt
                .arr
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.head.armed)
                .map(|(t, _)| Rec {
                    track: t,
                    qn0: q,
                    env: Vec::new(),
                })
                .collect();
        } else if !rec && self.gt.recording {
            self.gt.recording = false;
            self.gt.arr.rec.clear();
            self.gt.after_rec = 4;
            self.gt.arr.mark();
        }
        if rec && open {
            let q = r.time_to_qn(r.play_position());
            for k in 0..self.gt.arr.rec.len() {
                let t = self.gt.arr.rec[k].track;
                if let Some(g) = self.gt.arr.tracks.get(t).map(|x| x.head.guid.clone()) {
                    let p = r.track_peak(&g);
                    self.gt.arr.rec[k].env.push((q, p));
                }
            }
        }
        if self.gt.after_rec > 0 {
            self.gt.after_rec -= 1;
            if self.gt.after_rec == 0 {
                self.gt.since_sync = 1000;
            }
        }
        // Le rafraichissement, fenetre ouverte.
        self.gt.since_sync += 1;
        if self.gt.since_sync >= 15 && !rec {
            self.gt.since_sync = 0;
            self.gtab_sync();
            if open {
                self.gtab_follow_selection(playing);
                self.gtab_select_pending();
                // Le theme de REAPER.
                let key: Vec<_> = LOOK.iter().map(|k| r.theme_color(k)).collect();
                if let Some(w) = &mut self.gt.win {
                    if w.theme_key != key {
                        w.theme = Theme::reaper(w.theme.scale, &|k| r.theme_color(k));
                        w.theme_key = key;
                        self.gt.arr.mark();
                    }
                }
            }
        }
        if !open {
            return;
        }
        // La tete de lecture et les vumetres.
        let pos = if playing {
            r.play_position()
        } else {
            r.cursor_position()
        };
        self.gt.arr.set_playhead(Some(r.time_to_qn(pos)), playing);
        let meters: Vec<(usize, f32)> = self
            .gt
            .arr
            .tracks
            .iter()
            .enumerate()
            .map(|(t, x)| (t, r.track_peak(&x.head.guid)))
            .collect();
        for (t, m) in meters {
            let h = &mut self.gt.arr.tracks[t].head;
            let m = m.min(1.2);
            let nv = if m > h.meter { m } else { h.meter * 0.85 };
            if (nv - h.meter).abs() > 0.004 {
                h.meter = nv;
                self.gt.arr.mark();
            }
        }
        let shown = self.gt.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd));
        if !shown {
            return;
        }
        self.gtab_tip();
        if playing || self.gt.job.is_some() {
            self.gtab_want_frames(true);
        } else if self.gt.arr.take_dirty() {
            self.gt.arr.mark();
            self.gtab_render(false);
        }
    }

    /// L'infobulle : apres 0,6 s immobile sur une cible, sous elle. Pendant
    /// qu'on tire un fader : sa valeur, tout de suite.
    fn gtab_tip(&mut self) {
        let h = self.gt.arr.hovered();
        let dragging = self.gt.arr.dragging();
        let live = dragging && matches!(h, cp_tabview::Hit::Fader { .. });
        let Some(w) = self.gt.win.as_mut() else {
            return;
        };
        if h != w.hover.0 {
            w.hover = (h, Instant::now());
            if let Some(t) = &mut w.tip {
                t.hide();
            }
            if !live {
                return;
            }
        }
        let shown = w.tip.as_ref().is_some_and(|t| t.is_shown());
        if !live {
            if dragging && shown {
                if let Some(t) = &mut w.tip {
                    t.hide();
                }
            }
            if shown || dragging || w.hover.1.elapsed().as_secs_f64() < 0.6 {
                return;
            }
        }
        let (Some(text), Some((x, y, bw, bh))) = (self.gt.arr.label(h), self.gt.arr.rect_of(h))
        else {
            return;
        };
        // Pendant le geste : seulement si le texte change.
        if live && shown && w.tip_text == text {
            return;
        }
        if w.tip.is_none() {
            w.tip = ui::Tooltip::new(w.hwnd);
        }
        // L'aide est au bord droit : sa bulle s'ouvre vers la gauche.
        let tx = if h == cp_tabview::Hit::Help {
            (x + bw - 420.0 * w.theme.scale).max(0.0)
        } else {
            x
        };
        let (sx, sy) = ui::client_to_screen(w.hwnd, tx as i32, (y + bh) as i32 + 6);
        if let Some(t) = &mut w.tip {
            t.show(&text.replace('\n', "\r\n"), sx, sy);
        }
        w.tip_text = text;
    }

    /// Choisir dans REAPER un item d'une guitare (son audio ou son MIDI) le
    /// choisit ici.
    fn gtab_follow_selection(&mut self, playing: bool) {
        let r = self.r;
        let sel_now = r.selected_item_guid();
        let changed = sel_now != self.gt.last_sel;
        self.gt.last_sel = sel_now.clone();
        if playing || !changed {
            return;
        }
        if let Some(s) = sel_now {
            let audio = r.item_ext(&s, "CPGT_src").unwrap_or(s);
            if self.gtab_find(&audio).is_some() {
                self.gt.pending = Some(audio);
            }
        }
    }

    pub(crate) fn gtab_cancel(&mut self) {
        if let Some(mut j) = self.gt.job.take() {
            j.progress.cancel.store(true, Ordering::Relaxed);
            if let Some(h) = j.handle.take() {
                let _ = h.join();
            }
            // Elle reprendra au prochain module.
            self.gt.queue.push_front((j.item, j.take));
        }
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    /// La fenetre est-elle a la place de l'arrangement ?
    pub(crate) fn gtab_in_band(&self) -> bool {
        let Some(w) = &self.gt.win else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(w.hwnd)
    }

    pub(crate) fn gtab_open(&mut self) {
        if let Some(w) = &self.gt.win {
            match w.place {
                Place::Dock => self.r.dock_activate(w.hwnd),
                Place::Float => win::show(w.hwnd),
                Place::Arrange => {
                    let h = w.hwnd;
                    if !self.gtab_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true) {
                        self.gtab_render(true);
                    }
                }
            }
            return;
        }
        let place = self.gt.place;
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 1400, 700, mode) else {
            self.host
                .error("la fenetre de Guitar Tab n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange && !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.gt.place = Place::Dock;
            self.gtab_open();
            return;
        }
        self.gtab_adopt(hwnd);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.gtab_render(true);
        }
        self.gt.since_sync = 1000;
        self.gtab_keep("gtab_open", "1");
    }

    /// Reprend la fenetre (a sa creation, ou apres un rechargement du module).
    pub(crate) fn gtab_adopt(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.gt.win = Some(GWin {
            hwnd,
            place: self.gt.place,
            surf: Surface::default(),
            present: Vec::new(),
            theme: Theme::reaper(scale, &|k| r.theme_color(k)),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            frames_on: false,
            closing: false,
            tip: None,
            hover: (cp_tabview::Hit::None, Instant::now()),
            tip_text: String::new(),
        });
        self.gtab_sync();
        self.gt.arr.mark();
        self.gtab_render(true);
    }

    /// Avant un rechargement (ou la fermeture de REAPER) : la fenetre reste
    /// au chargeur, l'item choisi est retenu.
    pub(crate) fn gtab_shutdown(&mut self) {
        self.gtab_cancel();
        if let Some(w) = &self.gt.win {
            self.host.frames(w.hwnd, false);
            self.r
                .ext_set(SECTION, "gtab_hwnd", &w.hwnd.0.to_string(), false);
        } else {
            self.r.ext_set(SECTION, "gtab_hwnd", "", false);
        }
        if let Some((t, i)) = self.gt.arr.sel_item() {
            if let Some(g) = self.gt.arr.item(t, i).map(|x| x.guid.clone()) {
                self.gtab_keep("gtab_take", &g);
            }
        }
    }

    pub(crate) fn gtab_close(&mut self, user: bool) {
        let in_band = self.gtab_in_band();
        if let Some(w) = &mut self.gt.win {
            w.closing = true;
            let h = w.hwnd;
            if in_band {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.gt.win = None;
        if user {
            self.gtab_keep("gtab_open", "0");
        }
    }

    pub(crate) fn gtab_set_place(&mut self, p: Place) {
        let reopen = self.gt.win.as_ref().is_some_and(|w| w.place != p);
        self.gt.place = p;
        self.gtab_keep("gtab_place", p.key());
        if reopen {
            self.gtab_close(false);
        }
        self.gtab_open();
    }

    pub(crate) fn gtab_place_menu_at(&mut self, owner: Hwnd, sx: i32, sy: i32) {
        let p = self.gt.place;
        let open = self.gt.win.is_some();
        let items = [
            Some(MenuItem {
                id: 1,
                text: "Show in arrange view",
                checked: p == Place::Arrange,
                enabled: true,
            }),
            Some(MenuItem {
                id: 2,
                text: "Dock in docker",
                checked: p == Place::Dock,
                enabled: true,
            }),
            Some(MenuItem {
                id: 3,
                text: "Floating window",
                checked: p == Place::Float,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: 4,
                text: "Close Guitar Tab",
                checked: false,
                enabled: open,
            }),
        ];
        match ui::popup_menu(owner, sx, sy, &items) {
            1 => self.queue(WinOp::GtabPlace(Place::Arrange)),
            2 => self.queue(WinOp::GtabPlace(Place::Dock)),
            3 => self.queue(WinOp::GtabPlace(Place::Float)),
            4 => self.queue(WinOp::GtabClose),
            _ => {}
        }
    }

    fn gtab_place_menu(&mut self, x: f32, y: f32) {
        let Some(h) = self.gt.win.as_ref().map(|w| w.hwnd) else {
            return;
        };
        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
        self.gtab_place_menu_at(h, sx, sy);
    }

    fn gtab_want_frames(&mut self, on: bool) {
        if let Some(w) = &mut self.gt.win {
            if w.frames_on != on {
                w.frames_on = on;
                self.host.frames(w.hwnd, on);
            }
        }
    }

    pub(crate) fn gtab_render(&mut self, force: bool) {
        let Some(w) = self.gt.win.as_mut() else {
            return;
        };
        if !win::is_window(w.hwnd) {
            return;
        }
        let rc = ui::client_rect(w.hwnd);
        let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
        if cw <= 0 || ch <= 0 {
            return;
        }
        let scale = ui::dpi(w.hwnd) as f32 / 96.0;
        if (scale - w.theme.scale).abs() > 0.01 {
            let r = self.r;
            w.theme = Theme::reaper(scale, &|k| r.theme_color(k));
        }
        let resized = w.surf.resize(cw, ch);
        let dirty = self.gt.arr.take_dirty();
        if !(dirty || force || resized) {
            return;
        }
        {
            let mut cv = Canvas::new(&mut w.surf, &mut self.scratch);
            self.gt.arr.draw(
                &mut cv,
                &mut self.text,
                &mut self.icons,
                &w.theme,
                cw as f32,
                ch as f32,
            );
        }
        ui::present(
            w.hwnd,
            &w.surf.px,
            cw,
            ch,
            &[(0, 0, cw, ch)],
            &mut w.present,
        );
    }

    fn gtab_feed(&mut self, ev: Ev) {
        if let Some(a) = self.gt.arr.event(&ev) {
            self.gtab_act(a);
        }
        self.gtab_render(false);
    }

    /// Les messages de la fenetre.
    pub(crate) fn gtab_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.gt.win.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        match m.msg {
            WM_CP_FRAME => {
                let r = self.r;
                let ps = r.play_state();
                let playing = ps & (play::PLAYING | play::RECORDING) != 0;
                let pos = if playing {
                    r.play_position()
                } else {
                    r.cursor_position()
                };
                self.gt.arr.set_playhead(Some(r.time_to_qn(pos)), playing);
                if playing {
                    self.gt.arr.mark();
                }
                self.gtab_render(false);
                if !playing && self.gt.job.is_none() {
                    self.gtab_want_frames(false);
                }
                Some(0)
            }
            WM_CP_ACCEL => {
                let Some(k) = key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                let wanted = self.gt.arr.wants_key(vk, md);
                Some(match k.message {
                    msg::KEYDOWN | msg::SYSKEYDOWN if wanted => {
                        self.gtab_feed(Ev::Key {
                            vk,
                            mods: md,
                            text: None,
                        });
                        1
                    }
                    msg::KEYUP | msg::SYSKEYUP if wanted => 1,
                    _ => 0,
                })
            }
            msg::SIZE => {
                self.gt.arr.mark();
                self.gtab_render(true);
                Some(0)
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                if let Some(w) = &mut self.gt.win {
                    let (cw, ch) = (w.surf.w, w.surf.h);
                    if cw > 0 && ch > 0 {
                        ui::present(hwnd, &w.surf.px, cw, ch, &[(0, 0, cw, ch)], &mut w.present);
                    } else {
                        self.gtab_render(true);
                    }
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001),
            msg::MOUSEMOVE => {
                if let Some(w) = &mut self.gt.win {
                    if !w.tracking {
                        w.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.gtab_feed(Ev::Move { x, y, mods: md });
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(w) = &mut self.gt.win {
                    w.tracking = false;
                }
                self.gtab_feed(Ev::Leave);
                Some(0)
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                let left = matches!(m.msg, msg::LBUTTONDOWN | msg::LBUTTONDBLCLK);
                if left {
                    ui::capture(hwnd);
                }
                self.gtab_feed(Ev::Down {
                    x,
                    y,
                    b: if left { Button::Left } else { Button::Right },
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                });
                Some(0)
            }
            msg::MBUTTONDOWN | msg::MBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                ui::capture(hwnd);
                self.gtab_feed(Ev::Down {
                    x,
                    y,
                    b: Button::Middle,
                    dbl: false,
                    mods: md,
                });
                Some(0)
            }
            msg::MBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                self.gtab_feed(Ev::Up {
                    x,
                    y,
                    b: Button::Middle,
                    mods: md,
                });
                Some(0)
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                self.gtab_feed(Ev::Up {
                    x,
                    y,
                    b: if m.msg == msg::LBUTTONUP {
                        Button::Left
                    } else {
                        Button::Right
                    },
                    mods: md,
                });
                Some(0)
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                let (cx, cy) = ui::screen_to_client(hwnd, x as i32, y as i32);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                let (dx, dy) = if m.msg == msg::MOUSEWHEEL {
                    (0.0, d)
                } else {
                    (d, 0.0)
                };
                self.gtab_feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
                Some(0)
            }
            msg::CLOSE => {
                self.queue(WinOp::GtabClose);
                Some(0)
            }
            msg::DESTROY => {
                let ours = self.gt.win.as_ref().is_some_and(|w| w.closing);
                if !ours {
                    self.gt.win = None;
                }
                None
            }
            _ => None,
        }
    }
}

/// Le nom d'une prise, sans l'extension de son fichier.
fn take_name(name: &str) -> String {
    let mut n = name.to_string();
    for ext in [
        ".wav", ".flac", ".mp3", ".aif", ".aiff", ".ogg", ".opus", ".wv",
    ] {
        if n.to_lowercase().ends_with(ext) {
            n.truncate(n.len() - ext.len());
            break;
        }
    }
    if n.trim().is_empty() {
        "Guitar".into()
    } else {
        n
    }
}

/// Un nom de fichier sans caracteres interdits.
fn clean_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let t = t.trim().trim_end_matches('.').to_string();
    if t.is_empty() { "Guitar Tab".into() } else { t }
}

/// Un chemin libre : « nom.musicxml », sinon « nom (2).musicxml »...
fn free_path(dir: &std::path::Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.musicxml"));
    if !first.exists() {
        return first;
    }
    for i in 2..1000 {
        let p = dir.join(format!("{stem} ({i}).musicxml"));
        if !p.exists() {
            return p;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_de_fichier() {
        assert_eq!(take_name("02-210831_1211.wav"), "02-210831_1211");
        assert_eq!(take_name(".wav"), "Guitar");
        assert_eq!(clean_name("a/b:c?"), "a_b_c_");
    }

    #[test]
    fn entrees_cles_et_volumes() {
        assert_eq!(input_label(0), "In 1");
        assert_eq!(input_label(1024 | 2), "In 3/4");
        assert_eq!(input_label(-1), "None");
        assert_eq!(input_label(4096 | 63 << 5), "MIDI");
        let k = sound_key("C:\\a.wav", 0.5, 4.0, 1.0);
        assert_eq!(k, sound_key("c:\\A.WAV", 0.5, 4.0, 1.0));
        assert_ne!(k, sound_key("C:\\a.wav", 0.6, 4.0, 1.0));
        assert!((db_of(vol_of(-6.0)) + 6.0).abs() < 1e-3);
        assert_eq!(vol_of(-80.0), 0.0);
    }
}
