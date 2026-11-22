//! CP Guitar Tab dans REAPER : une APPLICATION, comme le Browser.
//!
//! Trois choses vivent dans le projet :
//! 1. l'item AUDIO (la prise) : la source, jamais modifiee ;
//! 2. l'ANALYSE : le reseau ecoute la prise une fois (sur un fil de travail),
//!    et ce qu'il a entendu est garde (`Guitar Tab/cache/<GUID>.cpga`) ; les
//!    reglages de detection relisent ceci en quelques millisecondes ;
//! 3. l'item MIDI : la TABLATURE, sur une piste « Guitar Tab: <nom> » sous la
//!    guitare, une corde par canal MIDI. C'est lui que Cedric corrige, dans la
//!    fenetre ou dans l'editeur MIDI de REAPER : la fenetre relit l'item quand
//!    son empreinte change. Ce que le MIDI ne porte pas (liaisons, notes
//!    tenues, notes effacees) est dans ses donnees privees (`P_EXT`).
//!
//! Le lien : l'item audio porte le GUID de l'item MIDI (`CPGT_midi`), l'item
//! MIDI celui de l'audio (`CPGT_src`). Rouvrir le projet retrouve tout.
//!
//! Enregistrer avec la fenetre ouverte : a l'arret, la prise est transcrite.
//!
//! Le fil ne survit jamais au module : un rechargement l'annule et l'attend.

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
use cp_sys::transport::play;
use cp_sys::ui::{self, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_tab::doc::{self, MidiQn, Sidecar};
use cp_tab::model::{HOP, SR};
use cp_tab::score::{ScoreSpec, TabNote, write_musicxml};
use cp_tab::{Analysis, NoteParams, Posteriors, Progress, analyze, make_notes};
use cp_tabview::{Act as GAct, Detect, TabView, Take};
use cp_ui::{Button, Ev, Theme};

use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

const TITLE: &str = "Guitar Tab";
const IDENT: &str = "cp_studio_guitartab";

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
}

/// La prise montree.
pub(crate) struct Cur {
    pub audio: String,
    item: ItemAudio,
    pub midi: Option<String>,
    post: Posteriors,
    frame_qn: Vec<f64>,
}

/// Ce que rend le fil : la source (detruite sur le fil principal), l'analyse.
struct Out {
    src: Option<CppSource>,
    result: Result<(Posteriors, Analysis), String>,
}

pub(crate) struct Job {
    handle: Option<JoinHandle<Out>>,
    progress: Arc<Progress>,
    audio: String,
    item: ItemAudio,
    started: Instant,
    last_pct: i32,
}

pub(crate) struct GPart {
    pub tv: TabView,
    pub win: Option<GWin>,
    pub place: Place,
    pub cur: Option<Cur>,
    job: Option<Job>,
    /// L'empreinte de l'item MIDI telle qu'on l'a ecrite ou lue.
    midi_seen: Option<String>,
    since_sync: u32,
    recording: bool,
    /// Battements a attendre apres un enregistrement (REAPER cree l'item).
    after_rec: u32,
    /// Une prise a montrer des que possible (apres un rechargement, au
    /// demarrage).
    pub pending: Option<String>,
    /// Le dernier item choisi vu dans REAPER (on ne cherche que s'il change).
    last_sel: Option<String>,
}

impl GPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> GPart {
        let mut tv = TabView::new();
        tv.detect = r
            .ext_get(SECTION, "gtab_detect")
            .map(|s| Detect::from_text(&s))
            .unwrap_or_default();
        GPart {
            tv,
            win: None,
            place: r
                .ext_get(SECTION, "gtab_place")
                .and_then(|v| Place::from_key(&v))
                .unwrap_or(Place::Arrange),
            cur: None,
            job: None,
            midi_seen: None,
            since_sync: 0,
            recording: false,
            after_rec: 0,
            pending: None,
            last_sel: None,
        }
    }
}

const LOOK: [&str; 8] = [
    "genlist_bg",
    "genlist_fg",
    "genlist_selbg",
    "col_main_bg2",
    "col_main_text",
    "col_main_text2",
    "col_main_editbk",
    "col_toolbar_text_on",
];

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

impl App {
    // -----------------------------------------------------------------------
    // Les fichiers
    // -----------------------------------------------------------------------

    fn gtab_dir(&self) -> PathBuf {
        if std::env::var("CP_STUDIO_SELFTEST").is_ok() {
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
        if std::env::var("CP_STUDIO_SELFTEST").is_err() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    fn gtab_cache_path(&self, audio: &str) -> PathBuf {
        let name: String = audio
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.gtab_dir().join("cache").join(format!("{name}.cpga"))
    }

    // -----------------------------------------------------------------------
    // L'etat
    // -----------------------------------------------------------------------

    pub(crate) fn gtab_running(&self) -> bool {
        self.gt.job.is_some()
    }

    /// Les notes du decodage, en noires du projet (`ghost` : le seuil des
    /// fantomes).
    fn gtab_notes(&self, ghost: bool) -> Vec<TabNote> {
        let Some(cur) = &self.gt.cur else {
            return Vec::new();
        };
        let d = &self.gt.tv.detect;
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
        let n = cur.frame_qn.len();
        if n == 0 {
            return Vec::new();
        }
        make_notes(&cur.post, &prm)
            .into_iter()
            .filter(|x| x.start < n)
            .map(|x| TabNote {
                qn0: cur.frame_qn[x.start],
                qn1: cur.frame_qn[x.end.min(n - 1)].max(cur.frame_qn[x.start] + 1e-3),
                pitch: x.pitch,
                legato: x.peak == 0.0,
                vel: (40.0 + x.amp.clamp(0.0, 1.0) * 87.0) as u8,
            })
            .collect()
    }

    /// Redecode avec les reglages : les corrections restent (notes tenues,
    /// notes effacees) ; `range` : seulement ce passage, dont les notes
    /// deviennent tenues (un reglage choisi a la main pour lui).
    fn gtab_decode(&mut self, range: Option<(f64, f64)>) {
        if self.gt.cur.is_none() {
            return;
        }
        let t0 = Instant::now();
        let tuning = self.gt.tv.detect.tuning();
        let grid = self.gt.tv.detect.grid;
        let bpm = self.gt.tv.take.as_ref().map_or(120.0, |t| t.bpm);
        let notes = self.gtab_notes(false);
        let (mut fresh, dropped) = doc::finger(&notes, grid, bpm, &tuning);
        if let Some((a, b)) = range {
            for f in &mut fresh {
                if f.qn0 >= a && f.qn0 < b {
                    f.locked = true;
                }
            }
        }
        let merged = doc::merge(&self.gt.tv.doc, fresh, &self.gt.tv.suppressed, range);
        let ghosts = if self.gt.tv.detect.ghosts {
            doc::ghosts(&self.gtab_notes(true), &merged, grid, &tuning)
        } else {
            Vec::new()
        };
        self.gt.tv.dropped = dropped;
        self.gt.tv.ghosts = ghosts;
        self.gt.tv.set_doc(merged);
        self.r
            .ext_set(SECTION, "gtab_detect", &self.gt.tv.detect.to_text(), true);
        self.gtab_write_midi("CP Guitar Tab: detection");
        self.gt.tv.status = format!("Re-read in {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    }

    /// L'accordage a change : chaque note tenue garde sa hauteur sur une corde
    /// qui la joue ; le reste est redoigte par un nouveau decodage.
    fn gtab_retune(&mut self) {
        let t = self.gt.tv.detect.tuning();
        let mut kept = Vec::new();
        for mut n in self.gt.tv.doc.iter().filter(|n| n.locked).cloned() {
            let ok = |s: usize| {
                t.strings
                    .get(s)
                    .is_some_and(|&o| (0..=t.frets).contains(&(n.pitch - o)))
            };
            if !ok(n.string) {
                if let Some(s) = (0..t.strings.len())
                    .filter(|&s| ok(s))
                    .min_by_key(|&s| (s as i64 - n.string as i64).abs())
                {
                    n.string = s;
                }
            }
            n.fret = n.pitch - t.strings.get(n.string).copied().unwrap_or(40);
            kept.push(n);
        }
        self.gt.tv.set_doc(kept);
        self.gtab_decode(None);
    }

    // -----------------------------------------------------------------------
    // L'item MIDI
    // -----------------------------------------------------------------------

    /// Ecrit la tablature dans l'item MIDI (le cree s'il n'existe pas).
    fn gtab_write_midi(&mut self, undo: &str) {
        let r = self.r;
        let Some(cur) = &self.gt.cur else { return };
        let audio = cur.audio.clone();
        let item = cur.item.clone();
        let mut midi = cur.midi.clone().filter(|g| r.item_exists(g));
        if midi.is_none() {
            let name = take_name(&item.name);
            midi = r.create_midi_item(
                item.track,
                &format!("Guitar Tab: {name}"),
                item.pos,
                item.pos + item.len,
            );
            if let Some(g) = &midi {
                r.set_item_ext(&audio, "CPGT_midi", g);
                r.set_item_ext(g, "CPGT_src", &audio);
            }
        }
        let Some(g) = midi else {
            self.gt.tv.status = "The MIDI item could not be created".into();
            return;
        };
        let tuning = self.gt.tv.detect.tuning();
        let notes: Vec<QnNote> = doc::to_midi(&self.gt.tv.doc, &tuning)
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
        r.set_item_ext(
            &g,
            "CPGT_side",
            &Sidecar::of(&self.gt.tv.doc, &self.gt.tv.suppressed).to_text(),
        );
        r.set_item_ext(&g, "CPGT_detect", &self.gt.tv.detect.to_text());
        self.gt.midi_seen = r.midi_hash(&g);
        if let Some(t) = &mut self.gt.tv.take {
            t.midi = r.item_track_name(&g);
        }
        if let Some(c) = &mut self.gt.cur {
            c.midi = Some(g);
        }
    }

    /// Relit l'item MIDI (Cedric l'a modifie dans REAPER, ou l'a annule).
    fn gtab_read_midi(&mut self) {
        let r = self.r;
        let Some(g) = self.gt.cur.as_ref().and_then(|c| c.midi.clone()) else {
            return;
        };
        let Some(notes) = r.midi_notes(&g) else {
            return;
        };
        let side = Sidecar::from_text(&r.item_ext(&g, "CPGT_side").unwrap_or_default());
        let midi: Vec<MidiQn> = notes
            .iter()
            .map(|n| MidiQn {
                qn0: n.qn0,
                qn1: n.qn1,
                pitch: n.pitch,
                chan: n.chan.clamp(0, 15) as u8,
                vel: n.vel.clamp(1, 127) as u8,
            })
            .collect();
        let d = doc::from_midi(&midi, &self.gt.tv.detect.tuning(), &side);
        self.gt.tv.suppressed = side.suppressed;
        self.gt.tv.set_doc(d);
        self.gt.midi_seen = r.midi_hash(&g);
    }

    // -----------------------------------------------------------------------
    // La prise
    // -----------------------------------------------------------------------

    /// La prise montree dans la fenetre (son axe du temps, son son).
    fn gtab_set_take(
        &mut self,
        audio: String,
        item: ItemAudio,
        post: Posteriors,
        an: &Analysis,
        midi: Option<String>,
    ) {
        let r = self.r;
        let frames = an.frames;
        let frame_qn: Vec<f64> = (0..frames)
            .map(|f| r.time_to_qn(item.pos + (f * HOP) as f64 / SR as f64 / item.rate))
            .collect();
        let (origin_qn, sig, bpm) = r.measure_at(item.pos);
        let bar_qn = sig.0.max(1) as f64 * 4.0 / sig.1.max(1) as f64;
        let take = Take {
            name: item.name.clone(),
            track: item.track_name.clone(),
            len_s: item.len,
            qn0: r.time_to_qn(item.pos),
            qn1: r.time_to_qn(item.pos + item.len),
            origin_qn,
            first_bar: (origin_qn / bar_qn).round() as i32 + 1,
            sig,
            bpm,
            midi: midi.as_ref().and_then(|g| r.item_track_name(g)),
            frame_qn: frame_qn.clone(),
            env: an.env.iter().map(|&v| v as f32 / 255.0).collect(),
            onset: an.onset_curve(),
        };
        self.gt.cur = Some(Cur {
            audio,
            item,
            midi,
            post,
            frame_qn,
        });
        self.gt.tv.set_take(take);
    }

    /// Montre une prise deja transcrite (son analyse en cache, son item
    /// MIDI). Faux si elle ne l'a jamais ete.
    fn gtab_load(&mut self, audio: &str) -> bool {
        let r = self.r;
        let Some(item) = r.item_audio(audio) else {
            return false;
        };
        let Some(an) = std::fs::read(self.gtab_cache_path(audio))
            .ok()
            .and_then(|b| Analysis::from_bytes(&b))
        else {
            return false;
        };
        let midi = r.item_ext(audio, "CPGT_midi").filter(|g| r.item_exists(g));
        if let Some(d) = midi.as_ref().and_then(|g| r.item_ext(g, "CPGT_detect")) {
            self.gt.tv.detect = Detect::from_text(&d);
        }
        self.gt.tv.doc.clear();
        self.gt.tv.ghosts.clear();
        self.gt.tv.suppressed.clear();
        let post = an.posteriors();
        self.gtab_set_take(audio.to_string(), item, post, &an, midi.clone());
        if midi.is_some() {
            self.gtab_read_midi();
            // Les fantomes viennent du decodage sensible, pas du MIDI.
            if self.gt.tv.detect.ghosts {
                let t = self.gt.tv.detect.tuning();
                let g = self.gt.tv.detect.grid;
                self.gt.tv.ghosts = doc::ghosts(&self.gtab_notes(true), &self.gt.tv.doc, g, &t);
            }
        } else {
            self.gtab_decode(None);
        }
        self.gt.tv.net_note =
            "Analysis read from the cache: no need to run the network again.".into();
        true
    }

    /// Lance le reseau sur un item audio (un fil de travail).
    fn gtab_transcribe(&mut self, audio: &str) {
        if let Some(j) = &self.gt.job {
            j.progress.cancel.store(true, Ordering::Relaxed);
            self.gt.tv.busy = Some("Cancelling…".into());
            return;
        }
        let Some(mut item) = self.r.item_audio(audio) else {
            self.gt.tv.busy = None;
            self.gt.tv.status = "Select an audio item first (a guitar recording).".into();
            return;
        };
        if item.track_name.is_empty() {
            item.track_name = format!("Track {}", item.track);
        }
        let Some(src) = cp_sys::audio::open_file(&self.r, &item.path) else {
            self.gt.tv.status = format!("REAPER cannot open {}", item.path);
            return;
        };
        let progress = Arc::new(Progress::default());
        let p2 = progress.clone();
        let it2 = item.clone();
        let handle = std::thread::Builder::new()
            .name("cp_guitar_tab".into())
            .spawn(move || work(src, it2, p2))
            .ok();
        if handle.is_none() {
            self.host
                .error("CP Guitar Tab : le fil de travail n'a pas demarre");
            return;
        }
        self.host.info(&format!(
            "Guitar Tab : transcription de \u{ab} {} \u{bb} ({:.1} s)",
            item.name, item.len
        ));
        self.gt.tv.busy = Some(format!("Transcribing \u{201c}{}\u{201d}…", item.name));
        self.gt.tv.mark();
        self.gt.job = Some(Job {
            handle,
            progress,
            audio: audio.to_string(),
            item,
            started: Instant::now(),
            last_pct: -1,
        });
    }

    /// La transcription est finie : l'analyse en cache, la tablature, le MIDI.
    fn gtab_finish(&mut self, job: Job, post: Posteriors, an: Analysis) {
        let r = self.r;
        let elapsed = job.started.elapsed().as_secs_f64();
        let path = self.gtab_cache_path(&job.audio);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&path, an.to_bytes());
        let same = self.gt.cur.as_ref().is_some_and(|c| c.audio == job.audio);
        let midi = r
            .item_ext(&job.audio, "CPGT_midi")
            .filter(|g| r.item_exists(g));
        if !same {
            self.gt.tv.doc.clear();
            self.gt.tv.ghosts.clear();
            self.gt.tv.suppressed.clear();
        }
        self.gtab_set_take(job.audio.clone(), job.item.clone(), post, &an, midi.clone());
        if !same && midi.is_some() {
            // Une prise deja transcrite autrefois : ses corrections d'abord.
            self.gtab_read_midi();
        }
        self.gtab_decode(None);
        self.gt.tv.busy = None;
        self.gt.tv.net_note = format!(
            "The network ran once ({elapsed:.1} s for {}:{:02}). These settings only re-read its output.",
            (job.item.len / 60.0) as i64,
            (job.item.len % 60.0) as i64
        );
        let nd = self.gt.tv.doc.len();
        let has_midi = self.gt.cur.as_ref().is_some_and(|c| c.midi.is_some());
        self.host.info(&format!(
            "Guitar Tab : {nd} notes en {elapsed:.1} s, {} fantomes, MIDI {}",
            self.gt.tv.ghosts.len(),
            if has_midi { "pose" } else { "ABSENT" }
        ));
        // L'auto-test lit le resultat (et la partition, ecrite a part).
        if std::env::var("CP_STUDIO_SELFTEST").is_ok() {
            let (file, stats) = self.gtab_export_to(&take_name(&job.item.name));
            let pitches: Vec<String> = self
                .gt
                .tv
                .doc
                .iter()
                .take(16)
                .map(|n| n.pitch.to_string())
                .collect();
            let line = format!(
                "notes={} chords={} measures={} dropped={} midi={} pitches={} file={}",
                stats.notes,
                stats.chords,
                stats.measures,
                stats.dropped,
                has_midi as u8,
                pitches.join(","),
                file.map(|p| p.display().to_string()).unwrap_or_default()
            );
            r.ext_set(SECTION, "selftest_gtab", &line, false);
        }
        self.gtab_render(true);
    }

    /// Ecrit la partition (MusicXML) dans `Guitar Tab/<nom>.musicxml`.
    fn gtab_export_to(&mut self, stem: &str) -> (Option<PathBuf>, cp_tab::score::Stats) {
        let Some(t) = self.gt.tv.take.clone() else {
            return (None, Default::default());
        };
        let spec = ScoreSpec {
            title: stem.to_string(),
            origin_qn: t.origin_qn,
            sig: t.sig,
            bpm: t.bpm,
            grid: self.gt.tv.detect.grid,
            tuning: self.gt.tv.detect.tuning(),
        };
        let (xml, st) = write_musicxml(&self.gt.tv.doc, &spec, self.gt.tv.dropped);
        let dir = self.gtab_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = free_path(&dir, &clean_name(stem));
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

    /// L'icone et l'action : ouvrir la fenetre ; montrer la tablature de
    /// l'item choisi, ou le transcrire s'il n'en a pas.
    pub(crate) fn gtab_start(&mut self) {
        self.queue(WinOp::GtabOpen);
        let sel = self.r.selected_item_guid();
        // L'item MIDI d'une tablature : sa prise.
        let audio = sel.map(|g| self.r.item_ext(&g, "CPGT_src").unwrap_or(g));
        match audio {
            Some(a) if self.gt.cur.as_ref().is_some_and(|c| c.audio == a) => {
                if self.gt.job.is_some() {
                    self.gtab_transcribe(&a); // annule
                }
            }
            Some(a) => {
                if !self.gtab_load(&a) {
                    self.gtab_transcribe(&a);
                }
            }
            None if self.gt.cur.is_none() => {
                self.gt.tv.status =
                    "Select an audio item (a guitar recording), then press Transcribe.".into();
            }
            None => {}
        }
        self.gt.tv.mark();
    }

    /// Relance le reseau (l'item choisi, sinon la prise montree).
    fn gtab_retranscribe(&mut self) {
        let sel = self
            .r
            .selected_item_guid()
            .map(|g| self.r.item_ext(&g, "CPGT_src").unwrap_or(g));
        let target = sel.or_else(|| self.gt.cur.as_ref().map(|c| c.audio.clone()));
        match target {
            Some(a) => self.gtab_transcribe(&a),
            None => self.gt.tv.status = "Select an audio item (a guitar recording) first.".into(),
        }
    }

    pub(crate) fn gtab_edit_settings(&mut self) {
        // Les reglages sont dans la fenetre.
        self.queue(WinOp::GtabOpen);
    }

    fn gtab_act(&mut self, a: GAct) {
        let r = self.r;
        match a {
            GAct::Seek(q) => r.set_edit_cursor(r.qn_to_time(q)),
            GAct::Play => r.main_on_command(40044), // Transport : lecture/arret
            GAct::Undo => r.main_on_command(40029),
            GAct::Edited => self.gtab_write_midi("CP Guitar Tab: edit"),
            GAct::Detect { range } => self.gtab_decode(range),
            GAct::Retune => self.gtab_retune(),
            GAct::Transcribe => self.gtab_retranscribe(),
            GAct::Export => {
                let stem = self
                    .gt
                    .cur
                    .as_ref()
                    .map(|c| take_name(&c.item.name))
                    .unwrap_or_else(|| "Guitar Tab".into());
                let (p, _) = self.gtab_export_to(&stem);
                self.gt.tv.status = match p {
                    Some(p) => format!("Exported: {}", p.display()),
                    None => "Export failed".into(),
                };
            }
            GAct::ViewMenu { x, y } => self.gtab_place_menu(x, y),
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
                    self.gt.tv.busy = Some(format!(
                        "Transcribing \u{201c}{}\u{201d}… {pct} %",
                        j.item.name
                    ));
                    self.gt.tv.mark();
                }
            }
            if j.handle.as_ref().is_some_and(|h| h.is_finished()) {
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
                            self.gt.tv.busy = None;
                            self.gt.tv.status = format!("Transcription stopped: {e}");
                        }
                        None => {
                            self.gt.tv.busy = None;
                            self.gt.tv.status = "The analysis stopped unexpectedly".into();
                        }
                    }
                    self.gt.tv.mark();
                }
            }
        }
        if self.gt.win.is_none() {
            return;
        }
        if self.gt.job.is_none() {
            if let Some(g) = self.gt.pending.take() {
                if self.r.item_exists(&g) {
                    self.gtab_load(&g);
                }
            }
        }
        let r = self.r;
        // La tete de lecture.
        let ps = r.play_state();
        let playing = ps & (play::PLAYING | play::RECORDING) != 0;
        let pos = if playing {
            r.play_position()
        } else {
            r.cursor_position()
        };
        self.gt.tv.set_playhead(Some(r.time_to_qn(pos)), playing);
        // Enregistrer avec la fenetre ouverte : la prise est transcrite a
        // l'arret.
        let rec = ps & play::RECORDING != 0;
        if rec && !self.gt.recording {
            self.gt.recording = true;
            self.gt.tv.busy = Some("Recording… the tab appears when you stop".into());
        } else if !rec && self.gt.recording {
            self.gt.recording = false;
            self.gt.tv.busy = None;
            self.gt.after_rec = 4;
        }
        if self.gt.after_rec > 0 {
            self.gt.after_rec -= 1;
            if self.gt.after_rec == 0 {
                if let Some(g) = r.selected_item_guid() {
                    if r.item_audio(&g).is_some()
                        && self.gt.cur.as_ref().is_none_or(|c| c.audio != g)
                    {
                        self.gtab_transcribe(&g);
                    }
                }
            }
        }
        // L'item MIDI modifie dans REAPER (ou une annulation) : relu.
        self.gt.since_sync += 1;
        if self.gt.since_sync >= 10 && self.gt.job.is_none() {
            self.gt.since_sync = 0;
            if let Some(g) = self.gt.cur.as_ref().and_then(|c| c.midi.clone()) {
                let h = r.midi_hash(&g);
                if h.is_some() && h != self.gt.midi_seen {
                    self.gtab_read_midi();
                }
            }
            // Choisir dans REAPER un item deja transcrit le montre.
            let sel_now = r.selected_item_guid();
            let changed = sel_now != self.gt.last_sel;
            self.gt.last_sel = sel_now.clone();
            if !playing && changed {
                if let Some(sel) = sel_now {
                    let audio = r.item_ext(&sel, "CPGT_src").unwrap_or(sel);
                    let other = self.gt.cur.as_ref().is_none_or(|c| {
                        c.audio != audio && c.midi.as_deref() != Some(audio.as_str())
                    });
                    if other && r.item_ext(&audio, "CPGT_midi").is_some() {
                        self.gtab_load(&audio);
                    }
                }
            }
            // Le theme de REAPER.
            let key: Vec<_> = LOOK.iter().map(|k| r.theme_color(k)).collect();
            if let Some(w) = &mut self.gt.win {
                if w.theme_key != key {
                    w.theme = Theme::reaper(w.theme.scale, &|k| r.theme_color(k));
                    w.theme_key = key;
                    self.gt.tv.mark();
                }
            }
        }
        let shown = self.gt.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd));
        if !shown {
            return;
        }
        if playing || self.gt.job.is_some() {
            self.gtab_want_frames(true);
        } else if self.gt.tv.take_dirty() {
            self.gt.tv.mark();
            self.gtab_render(false);
        }
    }

    pub(crate) fn gtab_cancel(&mut self) {
        if let Some(mut j) = self.gt.job.take() {
            j.progress.cancel.store(true, Ordering::Relaxed);
            if let Some(h) = j.handle.take() {
                let _ = h.join();
            }
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
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 1400, 640, mode) else {
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
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.gt.win = Some(GWin {
            hwnd,
            place,
            surf: Surface::default(),
            present: Vec::new(),
            theme: Theme::reaper(scale, &|k| r.theme_color(k)),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            frames_on: false,
            closing: false,
        });
        self.gt.tv.mark();
        self.gtab_render(true);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.gtab_render(true);
        }
        self.gtab_keep("gtab_open", "1");
    }

    /// Reprend la fenetre apres un rechargement du module.
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
        });
        self.gt.tv.mark();
        self.gtab_render(true);
    }

    /// Avant un rechargement (ou la fermeture de REAPER) : la fenetre reste
    /// au chargeur, la prise est retenue.
    pub(crate) fn gtab_shutdown(&mut self) {
        self.gtab_cancel();
        if let Some(w) = &self.gt.win {
            self.host.frames(w.hwnd, false);
            self.r
                .ext_set(SECTION, "gtab_hwnd", &w.hwnd.0.to_string(), false);
        } else {
            self.r.ext_set(SECTION, "gtab_hwnd", "", false);
        }
        if let Some(c) = &self.gt.cur {
            let a = c.audio.clone();
            self.gtab_keep("gtab_take", &a);
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
        let dirty = self.gt.tv.take_dirty();
        if !(dirty || force || resized) {
            return;
        }
        {
            let mut cv = Canvas::new(&mut w.surf, &mut self.scratch);
            self.gt.tv.draw(
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
        if let Some(a) = self.gt.tv.event(&ev) {
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
                self.gt.tv.set_playhead(Some(r.time_to_qn(pos)), playing);
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
                let wanted = self.gt.tv.wants_key(vk, md);
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
                self.gt.tv.mark();
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
}
