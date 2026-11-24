//! CP Session dans REAPER : la grille des clips, a la place de
//! l'arrangement, dans un docker ou flottante.
//!
//! - Les colonnes sont les pistes montrees dans la console ; chacune joue sur
//!   son port du moteur, dans sa piste (ses effets, son volume, ses envois).
//! - Les clips vivent dans le projet (`ProjExtState`, section
//!   `CP_Session2`) ; un projet de l'ancienne Session est repris.
//! - Les clips MIDI jouent par les lanes du chargeur (l'extension) : sans
//!   elle (un chargeur plus ancien, REAPER pas redemarre), la Session le dit.
//!
//! Le modele est `cp_session`, la vue `cp_sessionview` ; ici, le lien a
//! REAPER et au moteur.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use cp_abi::{
    LaneNote, PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK, VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL,
    WM_CP_FRAME,
};
use cp_gfx::IRect;
use cp_session::clip::{LENGTHS, length_label};
use cp_session::{
    AudioSrc, COLORS, CellState, Clip, ClockMode, Engine, Input, Kind, LaneClip, Motion, Quant,
    RecPoll, Session, TempoMode, TrackInfo,
};
use cp_sessionview::{Act, Hit, Strip, View};
use cp_sys::module::{AudioLoad, HostRef, key_event};
use cp_sys::reaper::Reaper;
use cp_sys::sess::{SessTrack, input_label, midi_out_label};
use cp_sys::transport::play;
use cp_sys::ui::{self, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, Theme};

use crate::gtab::{LOOK, selftest};
use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

const TITLE: &str = "Session";
const IDENT: &str = "cp_studio_session";

// ---------------------------------------------------------------------------
// Le moteur, vu de la Session
// ---------------------------------------------------------------------------

/// Un fichier dans le moteur.
#[derive(Clone, Debug, Default)]
struct AudioEntry {
    ticket: u32,
    clip: u32,
    frames: i64,
    srate: f64,
    file_bpm: Option<f64>,
    err: Option<String>,
}

/// Les fichiers des cases, charges une fois ; les regions etirees, cuites
/// une fois.
#[derive(Default)]
pub(crate) struct AudioCache {
    files: HashMap<String, AudioEntry>,
    /// (clip, debut, fin, tempo en millioniemes) -> la version cuite.
    baked: HashMap<(u32, i64, i64, i64), AudioEntry>,
}

/// Le moteur du chargeur, tel que la Session le demande.
struct Eng<'a> {
    host: HostRef,
    r: Reaper,
    cache: &'a mut AudioCache,
}

impl Engine for Eng<'_> {
    fn now(&self) -> i64 {
        self.host.audio_now()
    }
    fn srate(&self) -> f64 {
        let s = self.host.audio_srate();
        if s > 1.0 { s } else { 48000.0 }
    }
    fn has_lanes(&self) -> bool {
        self.host.ext().is_some()
    }
    fn ports(&self) -> u32 {
        self.host.ports()
    }
    fn route(&mut self, port: u32, track: &str) -> bool {
        self.host.audio_route(port, Some(track))
    }
    fn lane_play(&mut self, port: u32, clip: &LaneClip, from: f64, fpb: f64, at: i64) -> bool {
        let notes = lane_notes(clip);
        self.host.lane_play(
            port,
            clip.id,
            &notes,
            clip.len,
            clip.brace,
            clip.looped,
            from,
            fpb,
            Some(at),
        )
    }
    fn lane_update(&mut self, port: u32, clip: &LaneClip) -> bool {
        let notes = lane_notes(clip);
        self.host
            .lane_update(port, clip.id, &notes, clip.len, clip.brace, clip.looped)
    }
    fn lane_stop(&mut self, port: u32, at: i64) -> bool {
        self.host.lane_stop(port, Some(at))
    }
    fn lane_cancel(&mut self, port: u32) {
        self.host.lane_cancel(port);
    }
    fn lane_rate(&mut self, port: u32, fpb: f64, at: i64) {
        self.host.lane_rate(port, fpb, Some(at));
    }
    fn audio_src(&mut self, path: &str) -> Option<Result<AudioSrc, String>> {
        let host = self.host;
        let e = self.cache.files.entry(path.to_string()).or_default();
        if let Some(err) = &e.err {
            return Some(Err(err.clone()));
        }
        if e.clip == 0 {
            if e.ticket == 0 {
                if !std::path::Path::new(path).is_file() {
                    e.err = Some("file not found".into());
                    return Some(Err("file not found".into()));
                }
                e.ticket = host.audio_load(path);
                e.file_bpm = self.r.file_tempo(path);
            }
            match host.audio_poll(e.ticket) {
                AudioLoad::Pending => return None,
                AudioLoad::Failed(m) => {
                    e.err = Some(m.clone());
                    return Some(Err(m));
                }
                AudioLoad::Ready(c) => {
                    e.clip = c;
                    if let Some(i) = host.audio_info(c) {
                        e.frames = i.frames;
                        e.srate = i.srate;
                    }
                }
            }
        }
        Some(Ok(AudioSrc {
            clip: e.clip,
            frames: e.frames,
            srate: if e.srate > 1.0 { e.srate } else { 48000.0 },
            file_bpm: e.file_bpm,
        }))
    }
    fn voice_play(
        &mut self,
        port: u32,
        src: &AudioSrc,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
        rate: f64,
        at: i64,
    ) -> Option<u32> {
        let v = self
            .host
            .audio_play_at(port, src.clip, start, end, looped, gain, Some(at));
        if v == 0 {
            return None;
        }
        if (rate - 1.0).abs() > 1e-9 {
            // Hors vitesse 1 : l'interpolation haute qualite.
            self.host.audio_set(v, 19, 1.0);
            self.host.audio_set(v, 0, rate);
        }
        Some(v)
    }
    fn voice_stop(&mut self, voice: u32, at: Option<i64>, fade_s: f64) {
        match at {
            Some(a) => self.host.audio_stop_at(voice, a, fade_s),
            None => self.host.audio_stop(voice, fade_s),
        }
    }
    fn voice_keep(&mut self, voice: u32) {
        if self.host.ext().is_some() {
            self.host.audio_stop_at(voice, i64::MAX, 0.005);
        }
    }
    fn voice_rate(&mut self, voice: u32, rate: f64) {
        self.host.audio_set(voice, 0, rate);
    }
    fn release_clip(&mut self, clip: u32) {
        self.host.audio_release(clip);
    }
    fn voice_seek(&mut self, voice: u32, pos: f64) {
        // Param::Pos : un saut croise sur 5 ms dans le moteur.
        self.host.audio_set(voice, 7, pos.max(0.0));
    }
    fn midi_rec_start(&mut self, dev: i32, at: i64) -> u32 {
        self.host.midi_rec_start(dev, Some(at))
    }
    fn midi_rec_stop(&mut self, id: u32, at: i64) {
        self.host.midi_rec_stop(id, Some(at));
    }
    fn midi_rec_take(&mut self, id: u32) -> Option<Vec<(i64, [u8; 3])>> {
        let evs = self.host.midi_rec_take(id)?;
        Some(evs.into_iter().map(|e| (e.frame, e.msg)).collect())
    }
    fn audio_rec_start(&mut self, ch0: i32, ch1: i32, at: i64) -> u32 {
        self.host.rec_start(ch0, ch1, Some(at))
    }
    fn audio_rec_stop(&mut self, id: u32, at: i64) {
        self.host.rec_stop(id, Some(at));
    }
    fn audio_rec_poll(&mut self, id: u32) -> RecPoll {
        let (st, v) = self.host.rec_state(id);
        match st {
            cp_abi::REC_RECORDING => RecPoll::Recording,
            cp_abi::REC_READY => {
                let clip = v as u32;
                let (frames, srate) = self
                    .host
                    .audio_info(clip)
                    .map_or((0, 48000.0), |i| (i.frames, i.srate));
                RecPoll::Ready(AudioSrc {
                    clip,
                    frames,
                    srate,
                    file_bpm: None,
                })
            }
            _ => RecPoll::Failed,
        }
    }
    fn stretched(&mut self, src: &AudioSrc, start: i64, end: i64, tempo: f64) -> Option<AudioSrc> {
        let host = self.host;
        let key = (src.clip, start, end, (tempo * 1e6).round() as i64);
        let e = self.cache.baked.entry(key).or_default();
        if e.err.is_some() {
            return None;
        }
        if e.clip == 0 {
            if e.ticket == 0 {
                e.ticket = host.audio_stretch(src.clip, start, end, tempo, 1.0);
                if e.ticket == 0 {
                    e.err = Some("refused".into());
                    return None;
                }
            }
            match host.audio_poll(e.ticket) {
                AudioLoad::Pending => return None,
                AudioLoad::Failed(m) => {
                    e.err = Some(m);
                    return None;
                }
                AudioLoad::Ready(c) => {
                    e.clip = c;
                    if let Some(i) = host.audio_info(c) {
                        e.frames = i.frames;
                        e.srate = i.srate;
                    }
                }
            }
        }
        Some(AudioSrc {
            clip: e.clip,
            frames: e.frames,
            srate: if e.srate > 1.0 { e.srate } else { src.srate },
            file_bpm: None,
        })
    }
    fn save_take(&mut self, clip: u32, name: &str) -> Option<String> {
        let dir = self
            .r
            .record_path()
            .or_else(|| self.r.project_dir())
            .unwrap_or_else(std::env::temp_dir);
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path: PathBuf = dir.join(format!("{} {stamp}.wav", safe.trim()));
        let p = path.to_string_lossy().to_string();
        if !self.host.clip_save(clip, &p) {
            return None;
        }
        // Le clip de la prise sert tel quel : pas de second decodage.
        let (frames, srate) = self
            .host
            .audio_info(clip)
            .map_or((0, 48000.0), |i| (i.frames, i.srate));
        self.cache.files.insert(
            p.clone(),
            AudioEntry {
                ticket: 0,
                clip,
                frames,
                srate,
                file_bpm: None,
                err: None,
            },
        );
        Some(p)
    }
}

fn lane_notes(c: &LaneClip) -> Vec<LaneNote> {
    c.notes
        .iter()
        .map(|&(start, len, chan, pitch, vel, prob)| LaneNote {
            start,
            len,
            chan,
            pitch,
            vel,
            // La probabilite : le premier octet de reserve (100 : toujours).
            pad: [prob.min(100), 0, 0, 0, 0],
        })
        .collect()
}

/// L'entree d'une piste, pour la Session.
fn input_of(v: i32) -> Input {
    if v < 0 {
        return Input::None;
    }
    if v & 4096 != 0 {
        let dev = (v >> 5) & 63;
        // 63 : toutes ; 62 : le clavier virtuel (pas une entree materielle :
        // les prises lisent toutes les entrees).
        return Input::Midi(if dev >= 62 { -1 } else { dev });
    }
    let c = v & 1023;
    if c >= 512 {
        return Input::None;
    }
    if v & 1024 != 0 {
        Input::Audio(c, c + 1)
    } else {
        Input::Audio(c, -1)
    }
}

// ---------------------------------------------------------------------------
// L'application
// ---------------------------------------------------------------------------

pub(crate) struct SWin {
    pub hwnd: Hwnd,
    pub place: Place,
    frame: crate::frame::Frame,
    theme: Theme,
    theme_key: Vec<Option<(u8, u8, u8)>>,
    tracking: bool,
    closing: bool,
    tip: Option<ui::Tooltip>,
    hover: (Hit, Instant),
}

pub(crate) struct SPart {
    pub ses: Session,
    pub view: View,
    pub win: Option<SWin>,
    pub place: Place,
    cache: AudioCache,
    strips: Vec<Strip>,
    /// Le projet lu (son onglet, son fichier).
    proj: Option<(usize, String)>,
    state_seen: i32,
    saved: u64,
    dirty_at: Option<Instant>,
    flash: Option<(String, Instant)>,
    t0: Instant,
    ticks: u32,
    /// Les noms des entrees et sorties (relus de temps en temps).
    ins_audio: Vec<String>,
    ins_midi: Vec<(i32, String)>,
    outs_midi: Vec<(i32, String)>,
    /// La case ouverte dans l'editeur.
    pub editing: Option<(String, usize)>,
}

impl SPart {
    pub fn new(r: &Reaper) -> SPart {
        let place = Place::from_key(&r.ext_get(SECTION, "ses_place").unwrap_or_default())
            .unwrap_or(Place::Arrange);
        let mut view = View::new();
        if let Some(h) = r
            .ext_get(SECTION, "ses_mixer")
            .and_then(|v| v.parse::<f32>().ok())
        {
            view.mixer_h = h.max(0.0);
        }
        SPart {
            ses: Session::new(),
            view,
            win: None,
            place,
            cache: AudioCache::default(),
            strips: Vec::new(),
            proj: None,
            state_seen: i32::MIN,
            saved: 0,
            dirty_at: None,
            flash: None,
            t0: Instant::now(),
            ticks: 0,
            ins_audio: Vec::new(),
            ins_midi: Vec::new(),
            outs_midi: Vec::new(),
            editing: None,
        }
    }
}

impl App {
    fn ses_keep(&self, key: &str, value: &str) {
        if !selftest() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    /// Le moteur et la Session, ensemble (une methode du modele qui commande
    /// le moteur).
    fn ses_with<T>(&mut self, f: impl FnOnce(&mut Session, &mut dyn Engine) -> T) -> T {
        let mut e = Eng {
            host: self.host,
            r: self.r,
            cache: &mut self.ses.cache,
        };
        f(&mut self.ses.ses, &mut e)
    }

    fn ses_say(&mut self, s: impl Into<String>) {
        self.ses.flash = Some((s.into(), Instant::now()));
        self.ses.view.mark();
    }

    // -----------------------------------------------------------------------
    // Le projet
    // -----------------------------------------------------------------------

    /// Le projet a change (un autre onglet, un autre fichier) : tout ce qui
    /// joue se tait, la grille se relit.
    fn ses_project(&mut self) {
        let id = self.r.project_id();
        if self.ses.proj.as_ref() == Some(&id) {
            return;
        }
        if self.ses.proj.is_some() {
            self.ses_with(|s, e| s.panic(e));
            self.ses_save_now();
        }
        self.ses.proj = Some(id);
        let r = self.r;
        let mut s = Session::new();
        let has_new = r.proj_ext_get(cp_session::SECTION, "grid").is_some()
            || r.proj_ext_get(cp_session::SECTION, "cfg").is_some();
        s.load(&|k| r.proj_ext_get(cp_session::SECTION, k));
        // L'ancienne Session : ses cases, par le GUID de la piste de chaque
        // colonne.
        let mut imported = 0;
        if !has_new {
            if let Some(old) = r.proj_ext_get("CP_Session", "grid") {
                imported = s.import_old(&old, &|t| {
                    r.proj_ext_get("CP_Loop", &format!("dest{t}"))
                        .filter(|g| !g.is_empty())
                });
            }
        }
        // Ce qui jouait (ports, voix) reste au moteur ; la nouvelle grille
        // ne joue rien.
        let tracks = self.ses_tracks();
        let mut e = Eng {
            host: self.host,
            r: self.r,
            cache: &mut self.ses.cache,
        };
        s.set_tracks(&mut e, tracks);
        s.apply_pending_motion();
        self.ses.ses = s;
        self.ses.saved = self.ses.ses.dirty;
        if imported > 0 {
            self.ses_say(format!("{imported} clips taken from the old Session"));
            self.ses.saved = u64::MAX;
            self.ses.dirty_at = Some(Instant::now());
        }
        self.ses.view.mark();
    }

    /// Les pistes qui sont des colonnes : celles que la console montre (pas
    /// les pistes de son de la Session).
    fn ses_tracks(&mut self) -> Vec<TrackInfo> {
        let all = self.r.session_tracks(false);
        // Les pistes de son : marquees « session:sound:<piste> ».
        let sounds: HashMap<String, String> = all
            .iter()
            .filter_map(|t| {
                let m = t.mark.as_deref()?.strip_prefix("session:sound:")?;
                Some((m.to_string(), t.guid.clone()))
            })
            .collect();
        all.into_iter()
            .filter(|t| {
                t.show
                    && !t
                        .mark
                        .as_deref()
                        .is_some_and(|m| m.starts_with("session:sound"))
            })
            .map(|t| TrackInfo {
                sound: sounds.get(&t.guid).cloned(),
                guid: t.guid,
                name: if t.name.is_empty() {
                    format!("Track {}", t.index + 1)
                } else {
                    t.name
                },
                color: t.color,
                armed: t.armed,
                input: input_of(t.input),
                instrument: t.instrument,
            })
            .collect()
    }

    /// Une colonne dont la piste porte un instrument et qui a des cases
    /// audio : une piste de son, soeur juste apres elle (l'instrument
    /// avalerait le son ; dans un dossier, l'enfant passerait par lui).
    fn ses_sound_tracks(&mut self) {
        let need: Vec<(String, String)> = self
            .ses
            .ses
            .cols
            .iter()
            .filter(|c| !c.held && c.track.instrument && c.track.sound.is_none())
            .filter(|c| {
                self.ses
                    .ses
                    .cells
                    .iter()
                    .any(|(k, v)| k.0 == c.track.guid && !v.is_midi())
            })
            .map(|c| (c.track.guid.clone(), c.track.name.clone()))
            .collect();
        if need.is_empty() {
            return;
        }
        let r = self.r;
        r.undo_begin();
        for (g, name) in need {
            let all = r.session_tracks(false);
            let Some(t) = all.iter().find(|t| t.guid == g) else {
                continue;
            };
            // Juste apres la piste (et apres ses enfants, si c'est un
            // dossier : la soeur ne doit pas y entrer).
            let mut idx = t.index + 1;
            if t.depth > 0 {
                let mut d = t.depth;
                for x in all.iter().skip((t.index + 1) as usize) {
                    idx = x.index + 1;
                    d += x.depth;
                    if d <= 0 {
                        break;
                    }
                }
            }
            if let Some(s) = r.insert_track(idx, &format!("{name} audio")) {
                r.set_track_ext(&s, "CP", &format!("session:sound:{g}"));
                self.host.info(&format!(
                    "Session : piste de son pour « {name} » (elle porte un instrument)"
                ));
            }
        }
        r.undo_end("CP Session: sound track", -1);
    }

    /// Les tranches, dans l'ordre des colonnes.
    fn ses_strips(&mut self, fx: bool) {
        let tr: HashMap<String, SessTrack> = self
            .r
            .session_tracks(fx)
            .into_iter()
            .map(|t| (t.guid.clone(), t))
            .collect();
        let old = std::mem::take(&mut self.ses.strips);
        self.ses.strips = self
            .ses
            .ses
            .cols
            .iter()
            .enumerate()
            .map(|(i, c)| match tr.get(&c.track.guid) {
                Some(t) => Strip {
                    vol: t.vol,
                    pan: t.pan,
                    mute: t.mute,
                    solo: t.solo,
                    auto: t.auto,
                    input: input_label(t.input, &self.ses.ins_audio, &self.ses.ins_midi),
                    midi_out: midi_out_label(t.midi_out, &self.ses.outs_midi),
                    monitor: t.monitor,
                    meter: t.peak,
                    fx: if fx {
                        t.fx.clone()
                    } else {
                        old.get(i).map(|s| s.fx.clone()).unwrap_or_default()
                    },
                },
                None => Strip::default(),
            })
            .collect();
    }

    fn ses_save_now(&mut self) {
        if selftest() && self.ses.proj.as_ref().is_some_and(|p| p.1.is_empty()) {
            // L'auto-test ecrit aussi (un projet sans nom).
        }
        for (k, v) in self.ses.ses.save() {
            self.r.proj_ext_set(cp_session::SECTION, k, &v);
        }
        self.r.mark_dirty();
        self.ses.saved = self.ses.ses.dirty;
        self.ses.dirty_at = None;
    }

    // -----------------------------------------------------------------------
    // Le battement
    // -----------------------------------------------------------------------

    /// L'horloge de la Session suit REAPER : le tempo, la signature, et en
    /// Follow la position du transport.
    fn ses_clock(&mut self) {
        let r = self.r;
        let h = self.host;
        let ps = r.play_state();
        let playing = ps & play::PLAYING != 0 && ps & play::PAUSED == 0;
        // Le frame du moteur et la position du bloc qu'il prepare, lus
        // ensemble (le meme bloc).
        let mut now = h.audio_now();
        let mut pos = 0.0;
        for _ in 0..3 {
            pos = if playing {
                r.play_position2()
            } else {
                r.cursor_position()
            };
            let n2 = h.audio_now();
            if n2 == now {
                break;
            }
            now = n2;
        }
        let qn = r.time_to_qn(pos);
        let (_, (num, den), bpm) = r.measure_at(pos);
        let rate = if playing {
            r.play_rate().max(0.01)
        } else {
            1.0
        };
        let srate = {
            let s = h.audio_srate();
            if s > 1.0 { s } else { 48000.0 }
        };
        let fpb = srate * 60.0 / (bpm.max(1.0) * rate);
        let qpb = num as f64 * 4.0 / den.max(1) as f64;
        self.ses_with(|s, e| s.sync_clock(e, now, qn, fpb, qpb, playing));
    }

    pub(crate) fn ses_tick(&mut self) {
        self.ses.ticks = self.ses.ticks.wrapping_add(1);
        let open = self.ses.win.is_some();
        // La Session joue meme fenetre fermee (des clips lances) : son
        // battement continue tant que quelque chose vit.
        let alive = open || self.ses_alive();
        if !alive {
            return;
        }
        self.ses_project();
        // Les pistes : a chaque changement du projet, et une fois par seconde.
        let sc = self.r.state_count();
        if sc != self.ses.state_seen || self.ses.ticks % 30 == 0 {
            self.ses.state_seen = sc;
            self.ses_sound_tracks();
            let tr = self.ses_tracks();
            self.ses_with(|s, e| {
                s.set_tracks(e, tr);
                s.apply_pending_motion();
            });
            if self.ses.ticks % 90 == 0 || self.ses.ins_audio.is_empty() {
                self.ses.ins_audio = self.r.audio_inputs();
                self.ses.ins_midi = self.r.midi_inputs();
                self.ses.outs_midi = self.r.midi_outputs();
            }
            self.ses.view.mark();
        }
        self.ses_clock();
        self.ses_with(|s, e| s.tick(e));
        // Arrange Rec fini (le transport s'est arrete) : ce qui a joue
        // s'ecrit dans l'arrangement.
        if !self.ses.ses.arr_rec && !self.ses.ses.arr_log.is_empty() {
            self.host.info(&format!(
                "Session : Arrange Rec fini, {} entree(s) : {:?}",
                self.ses.ses.arr_log.len(),
                self.ses
                    .ses
                    .arr_log
                    .iter()
                    .map(|p| (p.start_b, p.end_b))
                    .collect::<Vec<_>>()
            ));
        }
        let log = self.ses.ses.take_arrange();
        if !log.is_empty() {
            self.ses_write_arrange(log);
        }
        if let Some(f) = self.ses.ses.flash.take() {
            self.ses_say(f.0);
        }
        // L'enregistrement dans le projet, un peu apres le dernier changement.
        if self.ses.ses.dirty != self.ses.saved {
            let t = *self.ses.dirty_at.get_or_insert_with(Instant::now);
            if t.elapsed().as_secs_f64() > 0.4 {
                self.ses_save_now();
            }
        }
        if let Some((_, t)) = &self.ses.flash {
            if t.elapsed().as_secs_f64() > 2.5 {
                self.ses.flash = None;
                self.ses.view.mark();
            }
        }
        if !open {
            return;
        }
        // Les tranches : les vumetres a chaque battement, les effets moins
        // souvent.
        let fx = self.ses.ticks % 15 == 0 || self.ses.strips.len() != self.ses.ses.cols.len();
        self.ses_strips(fx);
        if self.ses.ticks % 10 == 0 {
            let key: Vec<_> = LOOK.iter().map(|k| self.r.theme_color(k)).collect();
            if let Some(w) = &self.ses.win {
                if w.theme_key != key {
                    let sc = w.theme.scale;
                    let th = self.reaper_theme(sc);
                    if let Some(w) = &mut self.ses.win {
                        w.theme = th;
                        w.theme_key = key;
                    }
                    self.ses.view.mark();
                }
            }
            let mh = format!("{}", self.ses.view.mixer_h.round());
            if self.r.ext_get(SECTION, "ses_mixer").as_deref() != Some(mh.as_str()) {
                self.ses_keep("ses_mixer", &mh);
            }
        }
        self.ses_tip();
        let shown = self.ses.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd));
        if !shown {
            return;
        }
        if self.ses_animating() {
            self.ses_want_frames(true);
        } else {
            self.ses.view.mark();
            self.ses_render(false);
        }
    }

    /// Quelque chose joue, attend ou enregistre.
    fn ses_alive(&self) -> bool {
        self.ses.ses.cols.iter().any(|c| {
            c.run.play.is_some()
                || c.run.next.is_some()
                || c.run.rec.is_some()
                || c.run.wait.is_some()
                || c.run.rec_wait.is_some()
        })
    }

    fn ses_animating(&self) -> bool {
        self.ses_alive() || self.ses.view.dragging()
    }

    fn ses_tip(&mut self) {
        let h = self.ses.view.hover;
        let dragging = self.ses.view.dragging();
        let Some(w) = self.ses.win.as_mut() else {
            return;
        };
        if h != w.hover.0 {
            w.hover = (h, Instant::now());
            if let Some(t) = &mut w.tip {
                t.hide();
            }
            return;
        }
        let shown = w.tip.as_ref().is_some_and(|t| t.is_shown());
        if dragging && shown {
            if let Some(t) = &mut w.tip {
                t.hide();
            }
        }
        if shown || dragging || w.hover.1.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        let Some(text) = self.ses.view.label(&self.ses.ses, h) else {
            return;
        };
        let (sx, sy) = ui::cursor_pos();
        let Some(w) = self.ses.win.as_mut() else {
            return;
        };
        if w.tip.is_none() {
            w.tip = ui::Tooltip::new(w.hwnd);
        }
        if let Some(t) = &mut w.tip {
            t.show(&text, sx + 12, sy + 20);
        }
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn ses_act(&mut self, a: Act) {
        let r = self.r;
        match a {
            Act::Click { col, scene } => self.ses_with(|s, e| s.click(e, col, scene)),
            Act::Record { col, scene } => {
                self.ses_with(|s, e| {
                    s.record(e, col, scene);
                });
            }
            Act::StopRec(col) => {
                self.ses_with(|s, e| {
                    s.stop_rec(e, col);
                });
            }
            Act::Open { col, scene } => self.ses_open_cell(col, scene),
            Act::Erase { col, scene } => {
                self.ses_with(|s, e| s.erase(e, col, scene));
            }
            Act::UndoErase => {
                if !self.ses.ses.undo_erase() {
                    self.ses_say("Nothing to restore");
                }
            }
            Act::Move { from, to, copy } => self.ses_with(|s, e| s.move_cell(e, from, to, copy)),
            Act::DropOut { col, scene, copy } => self.ses_to_arrange(col, scene, copy),
            Act::DropGhost { col, scene } => {
                if let Some(g) = self.ses_new_track(None) {
                    let clip = self.ses.ses.clip(col, scene).cloned();
                    self.ses_refresh_tracks();
                    if let (Some(mut c), Some(to)) = (clip, self.ses.ses.col_of(&g)) {
                        c.id = self.ses.ses.fresh_id();
                        self.ses.ses.put(to, scene, c);
                    }
                }
            }
            Act::CellMenu { col, scene, x, y } => self.ses_cell_menu(col, scene, x, y),
            Act::HeaderMenu { col, x, y } => self.ses_header_menu(col, x, y),
            Act::SelectTrack(col) => {
                if let Some(c) = self.ses.ses.cols.get(col) {
                    r.select_only_track(&c.track.guid.clone());
                }
                self.ses.ses.sel.0 = col;
            }
            Act::Rename(col) => self.ses_rename_track(col),
            Act::MoveTrack { from, to } => self.ses_move_track(from, to),
            Act::Arm { col, add } => self.ses_arm(col, add),
            Act::Scene { scene, keep } => {
                self.ses_with(|s, e| s.launch_scene(e, scene, keep));
                self.ses.ses.sel.1 = scene;
                // Maj+Entree : la scene suivante est choisie.
                let n = self.ses.ses.scenes;
                if keep || self.last_mods.shift {
                    self.ses.ses.sel.1 = (scene + 1).min(n.saturating_sub(1));
                }
            }
            Act::StopCol(col) => self.ses_with(|s, e| s.stop_col(e, col)),
            Act::StopAll => self.ses_with(|s, e| s.stop_all(e)),
            Act::Panic => self.ses_with(|s, e| s.panic(e)),
            Act::Capture => {
                self.ses.ses.capture_scene();
            }
            Act::ToggleClock => {
                self.ses_with(|s, e| s.panic(e));
                self.ses.ses.clock = match self.ses.ses.clock {
                    ClockMode::Free => ClockMode::Follow,
                    ClockMode::Follow => ClockMode::Free,
                };
                self.ses.ses.tl.running = false;
                self.ses.ses.dirty += 1;
            }
            Act::QuantMenu { x, y } => self.ses_quant_menu(x, y),
            Act::RecMenu { x, y } => self.ses_rec_menu(x, y),
            Act::Overdub(col) => {
                let col = if self
                    .ses
                    .ses
                    .cols
                    .get(col)
                    .is_some_and(|c| c.run.play.is_some())
                {
                    col
                } else {
                    // La premiere colonne MIDI qui joue.
                    self.ses
                        .ses
                        .cols
                        .iter()
                        .position(|c| {
                            c.run.play.as_ref().is_some_and(|p| p.audio.is_none())
                                && matches!(c.track.input, Input::Midi(_))
                        })
                        .unwrap_or(col)
                };
                self.ses_with(|s, e| {
                    s.overdub(e, col);
                });
            }
            Act::ArrangeRec => self.ses_arrange_rec(),
            Act::BackToArrange => self.ses_back_to_arrange(),
            Act::Help { x, y } => self.ses_help(x, y),
            Act::NewTrack => {
                self.ses_new_track(None);
                self.ses_refresh_tracks();
            }
            Act::Select { col, scene } => {
                self.ses.ses.sel = (col, scene);
            }
            Act::Hint(m) => self.ses_say(m),
            Act::DockGrab => {
                if let Some(h) = self.ses.win.as_ref().map(|w| w.hwnd) {
                    self.dock_grab(h);
                }
            }
            Act::ViewMenu { x, y } => {
                if let Some(h) = self.ses.win.as_ref().map(|w| w.hwnd) {
                    let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
                    self.ses_place_menu_at(h, sx, sy);
                }
            }
            Act::SetVol { col, vol, done } => {
                if let Some(g) = self.ses_guid(col) {
                    r.set_track_value(&g, "D_VOL", vol);
                    if done {
                        r.undo_point("CP Session: volume");
                    }
                }
                self.ses_strips(false);
            }
            Act::SetPan { col, pan, done } => {
                if let Some(g) = self.ses_guid(col) {
                    r.set_track_value(&g, "D_PAN", pan);
                    if done {
                        r.undo_point("CP Session: pan");
                    }
                }
                self.ses_strips(false);
            }
            Act::Mute(col) => {
                if let Some(g) = self.ses_guid(col) {
                    let m = r.track_value(&g, "B_MUTE").unwrap_or(0.0);
                    r.set_track_value(&g, "B_MUTE", if m != 0.0 { 0.0 } else { 1.0 });
                    r.undo_point("CP Session: mute");
                }
                self.ses_strips(false);
            }
            Act::Solo { col, exclusive } => {
                if let Some(g) = self.ses_guid(col) {
                    let s = r.track_value(&g, "I_SOLO").unwrap_or(0.0);
                    if exclusive {
                        r.set_all_tracks_value("I_SOLO", 0.0);
                    }
                    r.set_track_value(&g, "I_SOLO", if s != 0.0 && !exclusive { 0.0 } else { 2.0 });
                    r.undo_point("CP Session: solo");
                }
                self.ses_strips(false);
            }
            Act::AutoMenu { col, x, y } => self.ses_auto_menu(col, x, y),
            Act::InputMenu { col, x, y } => self.ses_input_menu(col, x, y),
            Act::MidiOutMenu { col, x, y } => self.ses_midi_out_menu(col, x, y),
            Act::Monitor(col) => {
                if let Some(g) = self.ses_guid(col) {
                    let m = r.track_value(&g, "I_RECMON").unwrap_or(0.0) as i32;
                    r.set_track_value(&g, "I_RECMON", ((m + 1) % 3) as f64);
                    r.undo_point("CP Session: monitoring");
                }
                self.ses_strips(false);
            }
            Act::OpenFx { col, fx } => {
                if let Some(g) = self.ses_guid(col) {
                    r.track_fx_show(&g, fx as i32);
                }
            }
            Act::FxMenu { col, fx, x, y } => self.ses_fx_menu(col, fx, x, y),
            Act::AddFx(col) => {
                if let Some(g) = self.ses_guid(col) {
                    if self.fxb.st.set.steal {
                        self.fxb_open_for(&g);
                    } else {
                        r.select_only_track(&g);
                        r.track_fx_chain(&g);
                    }
                }
            }
        }
        self.ses.view.mark();
    }

    /// La piste de la colonne sous un point de l'ecran, si la Session est
    /// la (un effet lache depuis le navigateur d'effets).
    pub(crate) fn ses_track_at_screen(&self, sx: i32, sy: i32) -> Option<String> {
        let w = self.ses.win.as_ref()?;
        if !ui::is_shown(w.hwnd) {
            return None;
        }
        let (cx, cy) = ui::screen_to_client(w.hwnd, sx, sy);
        let rc = ui::client_rect(w.hwnd);
        if cx < 0 || cy < 0 || cx >= rc.right - rc.left || cy >= rc.bottom - rc.top {
            return None;
        }
        let col = match self
            .ses
            .view
            .hit(&self.ses.ses, &w.theme, cx as f32, cy as f32)
        {
            Hit::Cell { col, .. }
            | Hit::Header(col)
            | Hit::Arm(col)
            | Hit::StopCol(col)
            | Hit::Strip(col, _) => col,
            _ => return None,
        };
        self.ses_guid(col)
    }

    fn ses_guid(&self, col: usize) -> Option<String> {
        let c = self.ses.ses.cols.get(col)?;
        (!c.held).then(|| c.track.guid.clone())
    }

    fn ses_refresh_tracks(&mut self) {
        let tr = self.ses_tracks();
        self.ses_with(|s, e| s.set_tracks(e, tr));
        self.ses.state_seen = self.r.state_count();
    }

    /// Une piste neuve, a la fin du projet. Rend son GUID.
    fn ses_new_track(&mut self, name: Option<&str>) -> Option<String> {
        let r = self.r;
        let n = r.session_tracks(false).len() as i32;
        r.undo_begin();
        let g = r.insert_track(n, name.unwrap_or(""));
        r.undo_end("CP Session: new track", -1);
        g
    }

    fn ses_rename_track(&mut self, col: usize) {
        let Some(g) = self.ses_guid(col) else { return };
        let cur = self.ses.ses.cols[col].track.name.clone();
        if let Some(v) = self
            .r
            .user_inputs("Rename track", 1, "Name:,extrawidth=150", &cur)
        {
            let r = self.r;
            r.undo_begin();
            r.set_track_name(&g, v.trim());
            r.undo_end("CP Session: rename track", -1);
            self.ses_refresh_tracks();
        }
    }

    fn ses_move_track(&mut self, from: usize, to: usize) {
        let (Some(g), Some(t)) = (self.ses_guid(from), self.ses_guid(to)) else {
            return;
        };
        let r = self.r;
        let all = r.session_tracks(false);
        let Some(ti) = all.iter().find(|x| x.guid == t).map(|x| x.index) else {
            return;
        };
        let before = if to > from { ti + 1 } else { ti };
        r.undo_begin();
        r.reorder_tracks(&[g], before);
        r.undo_end("CP Session: move track", -1);
        self.ses_refresh_tracks();
    }

    /// L'armement : un clic arme seulement cette piste, avec l'ecoute de
    /// son entree ; Ctrl garde les autres ; une piste armee se desarme.
    fn ses_arm(&mut self, col: usize, add: bool) {
        let Some(g) = self.ses_guid(col) else { return };
        let r = self.r;
        let armed = r.track_value(&g, "I_RECARM").unwrap_or(0.0) != 0.0;
        if armed {
            r.set_track_value(&g, "I_RECARM", 0.0);
        } else {
            if !add {
                r.set_all_tracks_value("I_RECARM", 0.0);
            }
            r.set_track_value(&g, "I_RECARM", 1.0);
            if r.track_value(&g, "I_RECMON").unwrap_or(0.0) == 0.0 {
                r.set_track_value(&g, "I_RECMON", 1.0);
            }
            // Une piste sans entree : toutes les entrees MIDI.
            if r.track_value(&g, "I_RECINPUT").unwrap_or(-1.0) < 0.0 {
                r.set_track_value(&g, "I_RECINPUT", (4096 | (63 << 5)) as f64);
            }
        }
        self.ses_refresh_tracks();
        self.ses_strips(false);
    }

    /// Ouvre une case dans l'editeur ; une case vide recoit un clip MIDI de
    /// la longueur de Rec.
    fn ses_open_cell(&mut self, col: usize, scene: usize) {
        if self.ses.ses.cols.get(col).is_none_or(|c| c.held) {
            return;
        }
        if self.ses.ses.clip(col, scene).is_none() {
            let bars = self.ses.ses.rec_bars.max(1) as f64;
            self.ses.ses.new_midi(col, scene, bars);
        }
        self.ses.ses.sel = (col, scene);
        let Some(clip) = self.ses.ses.clip(col, scene).cloned() else {
            return;
        };
        match &clip.kind {
            Kind::Midi(_) => {
                let g = self.ses.ses.cols[col].track.guid.clone();
                self.ed_open_cell(&g, scene, &clip);
            }
            Kind::Audio(a) => {
                self.ses_say(a.path.clone());
            }
        }
    }

    /// L'editeur a change les notes d'une case.
    pub(crate) fn ses_cell_notes(
        &mut self,
        guid: &str,
        scene: usize,
        notes: Vec<cp_session::Note>,
    ) {
        let Some(col) = self.ses.ses.col_of(guid) else {
            return;
        };
        if let Some(c) = self.ses.ses.clip_mut(col, scene) {
            c.kind = Kind::Midi(notes);
        } else {
            return;
        }
        self.ses_with(|s, e| s.clip_changed(e, col, scene));
        self.ses.view.mark();
    }

    /// Ou en est le clip d'une case dans sa lane (pour la tete de l'editeur).
    pub(crate) fn ses_cell_pos(&self, guid: &str, scene: usize) -> Option<f64> {
        let col = self.ses.ses.col_of(guid)?;
        let p = self.ses.ses.cols[col].run.play.as_ref()?;
        if p.scene != scene {
            return None;
        }
        self.ses
            .ses
            .progress(col, self.host.audio_now())
            .map(|(_, pos)| pos)
    }

    /// Des fichiers deposes sur la grille : le premier dans la case visee,
    /// les autres dans les cases vides en dessous. Sur la colonne fantome :
    /// une piste neuve.
    fn ses_drop(&mut self, files: Vec<String>, x: f32, y: f32) {
        let audio: Vec<String> = files
            .into_iter()
            .filter(|f| {
                let e = std::path::Path::new(f)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                matches!(
                    e.as_str(),
                    "wav"
                        | "aif"
                        | "aiff"
                        | "flac"
                        | "mp3"
                        | "ogg"
                        | "m4a"
                        | "wv"
                        | "w64"
                        | "caf"
                        | "opus"
                        | "aac"
                )
            })
            .collect();
        if audio.is_empty() {
            return;
        }
        let th = self.ses_theme();
        let hit = self.ses.view.hit(&self.ses.ses, &th, x, y);
        let (col, scene) = match hit {
            Hit::Cell { col, scene, .. } => (col, scene),
            Hit::Ghost | Hit::None => {
                let name = std::path::Path::new(&audio[0])
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string());
                let Some(g) = self.ses_new_track(name.as_deref()) else {
                    return;
                };
                self.ses_refresh_tracks();
                let Some(c) = self.ses.ses.col_of(&g) else {
                    return;
                };
                (c, 0)
            }
            _ => return,
        };
        // Le passage choisi dans le navigateur (le glisser le garde).
        let section = crate::DROP_SECTION.lock().ok().and_then(|mut s| s.take());
        let mut s = scene;
        for (k, f) in audio.iter().enumerate() {
            if k > 0 {
                while self.ses.ses.clip(col, s).is_some() {
                    s += 1;
                }
            }
            let id = self.ses.ses.fresh_id();
            let mut clip = Clip::audio(id, f);
            if let (Some((p, a, b)), Kind::Audio(au)) = (&section, &mut clip.kind) {
                if p == f && b > a {
                    au.offs = *a;
                    au.len = b - a;
                }
            }
            self.ses.ses.put(col, s, clip);
            // Charge tout de suite (le lancement n'attendra pas).
            let mut e = Eng {
                host: self.host,
                r: self.r,
                cache: &mut self.ses.cache,
            };
            let _ = e.audio_src(f);
            s += 1;
        }
        self.ses.ses.sel = (col, scene);
        self.ses.view.mark();
    }

    // -----------------------------------------------------------------------
    // Vers l'arrangement
    // -----------------------------------------------------------------------

    /// Une case glissee dans l'arrangement : un item sur la piste sous la
    /// souris (sinon celle de la colonne), au curseur d'edition cale par le
    /// magnetisme.
    fn ses_to_arrange(&mut self, col: usize, scene: usize, _copy: bool) {
        let Some(clip) = self.ses.ses.clip(col, scene).cloned() else {
            return;
        };
        let r = self.r;
        let track = match r.track_under_mouse() {
            Some(t) => t,
            None => match self.ses_guid(col) {
                Some(g) => g,
                None => return,
            },
        };
        let pos = r.snap_time(
            r.position_under_mouse()
                .unwrap_or_else(|| r.cursor_position()),
        );
        r.undo_begin();
        let ok = match &clip.kind {
            Kind::Audio(a) => r.insert_audio_item(&track, &a.path, pos).is_some(),
            Kind::Midi(notes) => {
                let qpb = self.ses.ses.qpb();
                let len = clip.len_beats(qpb);
                let q0 = r.time_to_qn(pos);
                let t1 = r.qn_to_time(q0 + len);
                match r.create_midi_item_on(&track, &clip.name, pos, t1) {
                    Some(item) => {
                        let ns: Vec<cp_sys::items::EdNote> = notes
                            .iter()
                            .filter(|n| n.start < len)
                            .map(|n| cp_sys::items::EdNote {
                                qn0: q0 + n.start,
                                qn1: q0 + (n.start + n.len).min(len),
                                pitch: n.pitch as i32,
                                chan: n.chan as i32,
                                vel: n.vel as i32,
                                sel: false,
                                mute: false,
                            })
                            .collect();
                        r.set_ed_notes(&item, &ns, None, "")
                    }
                    None => false,
                }
            }
        };
        r.undo_end("CP Session: clip to the arrangement", -1);
        if !ok {
            self.ses_say("The clip could not be placed");
        }
    }

    /// Arrange Rec : en Follow, le transport part (il enregistre si une
    /// piste est armee) ; ce que la Session joue est note, puis ecrit dans
    /// l'arrangement quand le transport s'arrete. Recliquer l'arrete.
    fn ses_arrange_rec(&mut self) {
        let r = self.r;
        let ps = r.play_state();
        if self.ses.ses.arr_rec && ps & play::PLAYING != 0 {
            r.main_on_command(1016);
            return;
        }
        if self.ses.ses.clock != ClockMode::Follow {
            self.ses_with(|s, e| s.panic(e));
            self.ses.ses.clock = ClockMode::Follow;
            self.ses.ses.dirty += 1;
        }
        self.ses.ses.arr_rec = true;
        self.ses.ses.arr_log.clear();
        if ps & play::PLAYING == 0 {
            let armed = self.ses.ses.cols.iter().any(|c| c.track.armed);
            r.main_on_command(if armed { 1013 } else { 1007 });
        }
        self.ses_say("Arrange Rec: what the Session plays goes into the arrangement");
    }

    /// Le journal d'Arrange Rec ecrit : un item par clip joue, sur sa piste,
    /// la ou il a joue. Le MIDI deplie passe par passe ; l'audio, un item
    /// par passe (la region, a son taux). Un pas d'annulation.
    fn ses_write_arrange(&mut self, log: Vec<cp_session::Played>) {
        let r = self.r;
        r.undo_begin();
        let mut n = 0;
        for p in log {
            let Some(end) = p.end_b else { continue };
            let dur = end - p.start_b;
            let t0 = r.qn_to_time(p.start_b);
            let t1 = r.qn_to_time(end);
            match &p.clip.kind {
                Kind::Midi(_) => {
                    let Some(item) = r.create_midi_item_on(&p.track, &p.clip.name, t0, t1) else {
                        continue;
                    };
                    let notes: Vec<cp_sys::items::EdNote> =
                        cp_session::unfold(&p.clip, p.from, p.span, dur)
                            .into_iter()
                            .map(|x| cp_sys::items::EdNote {
                                qn0: p.start_b + x.start,
                                qn1: p.start_b + x.start + x.len,
                                pitch: x.pitch as i32,
                                chan: x.chan as i32,
                                vel: x.vel as i32,
                                sel: false,
                                mute: false,
                            })
                            .collect();
                    r.set_ed_notes(&item, &notes, None, "");
                    n += 1;
                }
                Kind::Audio(a) => {
                    let Some((s0, s1, sr, rate, _)) = p.audio else {
                        continue;
                    };
                    let region = (s1 - s0) as f64 / sr.max(1.0) / rate.max(1e-6);
                    let mut k = 0.0;
                    while k < dur - 1e-6 {
                        let q0 = p.start_b + k;
                        let q1 = (q0 + p.len).min(end);
                        let (a0, a1) = (r.qn_to_time(q0), r.qn_to_time(q1));
                        if let Some(item) = r.insert_audio_item(&p.track, &a.path, a0) {
                            r.set_item_value(&item, "D_LENGTH", (a1 - a0).min(region).max(0.001));
                            r.set_active_take_value(&item, "D_STARTOFFS", s0 as f64 / sr.max(1.0));
                            r.set_active_take_value(&item, "D_PLAYRATE", rate);
                            r.set_item_value(&item, "D_VOL", a.gain);
                            n += 1;
                        }
                        k += p.len.max(1e-3);
                    }
                }
            }
        }
        r.undo_end("CP Session: Arrange Rec", -1);
        r.refresh_arrange();
        self.ses_say(format!("Arrange Rec: {n} item(s) written"));
    }

    /// Back to Arrangement : la Session s'arrete, les pistes rendent leurs
    /// items a l'arrangement.
    fn ses_back_to_arrange(&mut self) {
        self.ses_with(|s, e| s.stop_all(e));
        self.ses_say("Back to the arrangement");
    }

    // -----------------------------------------------------------------------
    // Les menus
    // -----------------------------------------------------------------------

    fn ses_menu(&self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let Some(h) = self.ses.win.as_ref().map(|w| w.hwnd) else {
            return 0;
        };
        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
        ui::popup_menu(h, sx, sy, items)
    }

    fn ses_cell_menu(&mut self, col: usize, scene: usize, x: f32, y: f32) {
        let Some(clip) = self.ses.ses.clip(col, scene).cloned() else {
            // Une case vide : un clip neuf, ou y coller.
            let items = [Some(item(1, "New MIDI clip", false, true))];
            if self.ses_menu(x, y, &items) == 1 {
                self.ses_open_cell(col, scene);
            }
            return;
        };
        let audio = !clip.is_midi();
        let (tempo, looped) = match &clip.kind {
            Kind::Audio(a) => (a.tempo, a.looped),
            Kind::Midi(_) => (TempoMode::Repitch, true),
        };
        let len_txt = format!("Length: {} bar(s)...", length_label(clip.bars));
        let items = [
            Some(item(1, "Edit in the Editor", false, !audio)),
            Some(item(2, "Rename clip...", false, true)),
            Some(item(3, "Color...", false, true)),
            Some(item(4, &len_txt, false, true)),
            Some(item(5, "Time signature...", false, true)),
            None,
            Some(item(
                10,
                "Tempo: repitch",
                audio && tempo == TempoMode::Repitch,
                audio,
            )),
            Some(item(
                11,
                "Tempo: stretch",
                audio && tempo == TempoMode::Stretch,
                audio,
            )),
            Some(item(
                12,
                "Tempo: don't follow",
                audio && tempo == TempoMode::None,
                audio,
            )),
            Some(item(13, "Source tempo...", false, audio)),
            Some(item(16, "Region and gain...", false, audio)),
            Some(item(
                14,
                "Playback: one-shot each pass",
                audio && !looped,
                audio,
            )),
            Some(item(
                15,
                "Playback: loop the material",
                audio && looped,
                audio,
            )),
            None,
            Some(item(20, "Launch from the start", !clip.phase_lock, true)),
            Some(item(
                21,
                "Launch in phase (phase lock)",
                clip.phase_lock,
                true,
            )),
            None,
            Some(item(30, "Stop this track", false, true)),
            Some(item(31, "Clear cell", false, true)),
        ];
        let k = self.ses_menu(x, y, &items);
        let r = self.r;
        let mut changed = true;
        match k {
            1 => {
                self.ses_open_cell(col, scene);
                changed = false;
            }
            2 => {
                if let Some(v) = r.user_inputs("Rename clip", 1, "Name:,extrawidth=150", &clip.name)
                {
                    if let Some(c) = self.ses.ses.clip_mut(col, scene) {
                        c.name = v.trim().to_string();
                    }
                }
            }
            3 => {
                let mut items: Vec<Option<MenuItem<'_>>> = COLORS
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| {
                        Some(item(i as u32 + 1, n, clip.color == Some(i as u8 + 1), true))
                    })
                    .collect();
                items.push(None);
                items.push(Some(item(100, "Track color", clip.color.is_none(), true)));
                let c = self.ses_menu(x, y, &items);
                if c > 0 {
                    if let Some(cl) = self.ses.ses.clip_mut(col, scene) {
                        cl.color = (c <= 8).then_some(c as u8);
                    }
                }
            }
            4 => {
                let labels: Vec<String> = LENGTHS.iter().map(|&b| length_label(b)).collect();
                let items: Vec<Option<MenuItem<'_>>> = labels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        Some(item(
                            i as u32 + 1,
                            l,
                            (LENGTHS[i] - clip.bars).abs() < 1e-9,
                            true,
                        ))
                    })
                    .collect();
                let c = self.ses_menu(x, y, &items);
                if c > 0 {
                    if let Some(cl) = self.ses.ses.clip_mut(col, scene) {
                        cl.bars = LENGTHS[(c - 1) as usize];
                    }
                }
            }
            5 => {
                let sigs: [(u32, u32); 7] =
                    [(2, 4), (3, 4), (4, 4), (5, 4), (6, 8), (7, 8), (12, 8)];
                let labels: Vec<String> = sigs.iter().map(|(n, d)| format!("{n}/{d}")).collect();
                let mut items: Vec<Option<MenuItem<'_>>> = vec![
                    Some(item(100, "Follow the project", clip.sig.is_none(), true)),
                    None,
                ];
                for (i, l) in labels.iter().enumerate() {
                    items.push(Some(item(i as u32 + 1, l, clip.sig == Some(sigs[i]), true)));
                }
                let c = self.ses_menu(x, y, &items);
                if c > 0 {
                    if let Some(cl) = self.ses.ses.clip_mut(col, scene) {
                        cl.sig = (c != 100).then(|| sigs[(c - 1) as usize]);
                    }
                }
            }
            10..=12 => {
                if let Some(Kind::Audio(a)) = self.ses.ses.clip_mut(col, scene).map(|c| &mut c.kind)
                {
                    a.tempo = match k {
                        10 => TempoMode::Repitch,
                        11 => TempoMode::Stretch,
                        _ => TempoMode::None,
                    };
                }
            }
            13 => {
                let cur = match &clip.kind {
                    Kind::Audio(a) => a.src_bpm.map(|b| format!("{b}")).unwrap_or_default(),
                    _ => String::new(),
                };
                if let Some(v) = r.user_inputs("Source tempo", 1, "BPM (empty: find it):", &cur) {
                    let b = v
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|b| (20.0..=400.0).contains(b));
                    if let Some(Kind::Audio(a)) =
                        self.ses.ses.clip_mut(col, scene).map(|c| &mut c.kind)
                    {
                        a.src_bpm = b;
                    }
                    if let Some(c) = self.ses.ses.clip_mut(col, scene) {
                        // La passe se recalcule.
                        c.bars = 0.0;
                    }
                }
            }
            16 => {
                if let Kind::Audio(a) = &clip.kind {
                    let gdb = if a.gain > 1e-6 {
                        20.0 * a.gain.log10()
                    } else {
                        -60.0
                    };
                    let vals = format!("{:.3},{:.3},{:.1}", a.offs, a.len, gdb);
                    if let Some(v) = r.user_inputs(
                        "Region and gain",
                        3,
                        "Start (s):,Length (s; 0 = to the end):,Gain (dB):",
                        &vals,
                    ) {
                        let f: Vec<f64> = v
                            .split(',')
                            .map(|x| x.trim().parse().unwrap_or(f64::NAN))
                            .collect();
                        if let Some(Kind::Audio(au)) =
                            self.ses.ses.clip_mut(col, scene).map(|c| &mut c.kind)
                        {
                            if f.first().is_some_and(|x| x.is_finite()) {
                                au.offs = f[0].max(0.0);
                            }
                            if f.get(1).is_some_and(|x| x.is_finite()) {
                                au.len = f[1].max(0.0);
                            }
                            if f.get(2).is_some_and(|x| x.is_finite()) {
                                au.gain = 10f64.powf(f[2].clamp(-60.0, 24.0) / 20.0);
                            }
                        }
                        // La passe se recalcule sur la region.
                        if let Some(c) = self.ses.ses.clip_mut(col, scene) {
                            c.bars = 0.0;
                        }
                    }
                }
            }
            14 | 15 => {
                if let Some(Kind::Audio(a)) = self.ses.ses.clip_mut(col, scene).map(|c| &mut c.kind)
                {
                    a.looped = k == 15;
                }
            }
            20 | 21 => {
                if let Some(c) = self.ses.ses.clip_mut(col, scene) {
                    c.phase_lock = k == 21;
                }
            }
            30 => {
                self.ses_with(|s, e| s.stop_col(e, col));
                changed = false;
            }
            31 => {
                self.ses_with(|s, e| s.erase(e, col, scene));
                changed = false;
            }
            _ => changed = false,
        }
        if changed {
            self.ses_with(|s, e| s.clip_changed(e, col, scene));
        }
    }

    fn ses_header_menu(&mut self, col: usize, x: f32, y: f32) {
        let Some(c) = self.ses.ses.cols.get(col).cloned() else {
            return;
        };
        if c.held {
            // Une colonne sans piste : ses clips vont sur une piste choisie.
            let tracks = self.ses_tracks();
            let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();
            let mut items = vec![Some(item(0, "Give these clips a track:", false, false))];
            for (i, n) in names.iter().enumerate() {
                items.push(Some(item(i as u32 + 1, n, false, true)));
            }
            let k = self.ses_menu(x, y, &items);
            if k > 0 {
                let to = tracks[(k - 1) as usize].guid.clone();
                let keys: Vec<(String, usize)> = self
                    .ses
                    .ses
                    .cells
                    .keys()
                    .filter(|k| k.0 == c.track.guid)
                    .cloned()
                    .collect();
                for key in keys {
                    if let Some(clip) = self.ses.ses.cells.remove(&key) {
                        let mut s = key.1;
                        while self.ses.ses.cells.contains_key(&(to.clone(), s)) {
                            s += 1;
                        }
                        self.ses.ses.cells.insert((to.clone(), s), clip);
                    }
                }
                self.ses.ses.dirty += 1;
                self.ses_refresh_tracks();
            }
            return;
        }
        let mut items = vec![
            Some(item(1, "Rename track...", false, true)),
            Some(item(2, "Hide this column", false, true)),
            None,
        ];
        for (i, m) in Motion::ALL.iter().enumerate() {
            items.push(Some(item(10 + i as u32, m.label(), c.motion == *m, true)));
        }
        items.push(None);
        items.push(Some(item(3, "Show every hidden column", false, true)));
        items.push(Some(item(4, "Delete this track", false, true)));
        let k = self.ses_menu(x, y, &items);
        let r = self.r;
        match k {
            1 => self.ses_rename_track(col),
            2 => {
                r.undo_begin();
                r.set_track_value(&c.track.guid, "B_SHOWINMIXER", 0.0);
                r.undo_end("CP Session: hide column", -1);
                self.ses_refresh_tracks();
            }
            3 => {
                r.undo_begin();
                r.set_all_tracks_value("B_SHOWINMIXER", 1.0);
                r.undo_end("CP Session: show every column", -1);
                self.ses_refresh_tracks();
            }
            4 => {
                self.ses_with(|s, e| s.stop_col(e, col));
                r.undo_begin();
                r.delete_track(&c.track.guid);
                r.undo_end("CP Session: delete track", -1);
                self.ses_refresh_tracks();
            }
            k @ 10..=16 => {
                if let Some(cc) = self.ses.ses.cols.get_mut(col) {
                    cc.motion = Motion::ALL[(k - 10) as usize];
                }
                self.ses.ses.dirty += 1;
            }
            _ => {}
        }
    }

    fn ses_quant_menu(&mut self, x: f32, y: f32) {
        let q = self.ses.ses.q;
        let items: Vec<Option<MenuItem<'_>>> = Quant::ALL
            .iter()
            .enumerate()
            .map(|(i, v)| Some(item(i as u32 + 1, v.label(), q == *v, true)))
            .collect();
        let k = self.ses_menu(x, y, &items);
        if k > 0 {
            self.ses.ses.q = Quant::ALL[(k - 1) as usize];
            self.ses.ses.dirty += 1;
        }
    }

    fn ses_rec_menu(&mut self, x: f32, y: f32) {
        let cur = self.ses.ses.rec_bars;
        let opts = [1u32, 2, 4, 8, 16];
        let labels: Vec<String> = opts.iter().map(|b| format!("{b} bar(s)")).collect();
        let items: Vec<Option<MenuItem<'_>>> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| Some(item(i as u32 + 1, l, cur == opts[i], true)))
            .collect();
        let k = self.ses_menu(x, y, &items);
        if k > 0 {
            self.ses.ses.rec_bars = opts[(k - 1) as usize];
            self.ses.ses.dirty += 1;
        }
    }

    fn ses_auto_menu(&mut self, col: usize, x: f32, y: f32) {
        let Some(g) = self.ses_guid(col) else { return };
        let r = self.r;
        let cur = r.track_value(&g, "I_AUTOMODE").unwrap_or(0.0) as i32;
        let names = [
            "Trim/Read",
            "Read",
            "Touch",
            "Write",
            "Latch",
            "Latch Preview",
        ];
        let items: Vec<Option<MenuItem<'_>>> = names
            .iter()
            .enumerate()
            .map(|(i, n)| Some(item(i as u32 + 1, n, cur == i as i32, true)))
            .collect();
        let k = self.ses_menu(x, y, &items);
        if k > 0 {
            r.set_track_value(&g, "I_AUTOMODE", (k - 1) as f64);
            r.undo_point("CP Session: automation mode");
            self.ses_strips(false);
        }
    }

    fn ses_input_menu(&mut self, col: usize, x: f32, y: f32) {
        let Some(g) = self.ses_guid(col) else { return };
        let r = self.r;
        let cur = r.track_value(&g, "I_RECINPUT").unwrap_or(-1.0) as i32;
        let ins = r.audio_inputs();
        let mids = r.midi_inputs();
        let mut vals: Vec<i32> = vec![-1];
        let mut labels: Vec<String> = vec!["No input".into()];
        for (i, n) in ins.iter().enumerate() {
            vals.push(i as i32);
            labels.push(format!("Audio: {n}"));
        }
        for i in (0..ins.len()).step_by(2) {
            if i + 1 < ins.len() {
                vals.push(1024 | i as i32);
                labels.push(format!("Audio stereo: {} / {}", ins[i], ins[i + 1]));
            }
        }
        vals.push(4096 | (63 << 5));
        labels.push("MIDI: all inputs".into());
        for (d, n) in &mids {
            vals.push(4096 | (d << 5));
            labels.push(format!("MIDI: {n}"));
        }
        vals.push(4096 | (62 << 5));
        labels.push("MIDI: virtual keyboard".into());
        let items: Vec<Option<MenuItem<'_>>> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| Some(item(i as u32 + 1, l, vals[i] == cur, true)))
            .collect();
        let k = self.ses_menu(x, y, &items);
        if k > 0 {
            r.set_track_value(&g, "I_RECINPUT", vals[(k - 1) as usize] as f64);
            r.undo_point("CP Session: input");
            self.ses_refresh_tracks();
            self.ses_strips(false);
        }
    }

    fn ses_midi_out_menu(&mut self, col: usize, x: f32, y: f32) {
        let Some(g) = self.ses_guid(col) else { return };
        let r = self.r;
        let cur = r.track_value(&g, "I_MIDIHWOUT").unwrap_or(-1.0) as i32;
        let outs = r.midi_outputs();
        let mut vals = vec![-1];
        let mut labels = vec!["No MIDI output".to_string()];
        for (d, n) in &outs {
            vals.push(d << 5);
            labels.push(n.clone());
        }
        let items: Vec<Option<MenuItem<'_>>> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| Some(item(i as u32 + 1, l, vals[i] == cur, true)))
            .collect();
        let k = self.ses_menu(x, y, &items);
        if k > 0 {
            r.set_track_value(&g, "I_MIDIHWOUT", vals[(k - 1) as usize] as f64);
            r.undo_point("CP Session: MIDI output");
            self.ses_strips(false);
        }
    }

    fn ses_fx_menu(&mut self, col: usize, fx: usize, x: f32, y: f32) {
        let Some(g) = self.ses_guid(col) else { return };
        let on = self
            .ses
            .strips
            .get(col)
            .and_then(|s| s.fx.get(fx))
            .is_some_and(|f| f.1);
        let items = [
            Some(item(1, "Open", false, true)),
            Some(item(2, "Bypass", !on, true)),
            Some(item(3, "Delete", false, true)),
            None,
            Some(item(4, "Open this track's FX chain", false, true)),
        ];
        let r = self.r;
        match self.ses_menu(x, y, &items) {
            1 => r.track_fx_show(&g, fx as i32),
            2 => {
                r.track_fx_enable(&g, fx as i32, !on);
                r.undo_point("CP Session: bypass");
            }
            3 => {
                r.undo_begin();
                r.track_fx_delete(&g, fx as i32);
                r.undo_end("CP Session: delete FX", -1);
            }
            4 => r.track_fx_chain(&g),
            _ => {}
        }
        self.ses_strips(true);
    }

    fn ses_help(&mut self, x: f32, y: f32) {
        let lines = [
            "Cell, left strip: launch / stop / record (armed track)",
            "Cell: click to edit, drag to move (Ctrl: copy), Alt+click: erase",
            "Drag a cell out of the window: into the arrangement",
            "Scene triangle: launch the row (Ctrl: keep the others)",
            "Arrows: choose a cell; Enter: launch; Shift+Enter: scene and next",
            "Ctrl+Enter: scene, keep the others; Delete: erase; Ctrl+Z: restore",
            "Ctrl+Shift+I: capture the playing clips into an empty scene",
            "Drop audio files on a cell, or on + for a new track",
        ];
        let items: Vec<Option<MenuItem<'_>>> = lines
            .iter()
            .map(|l| Some(item(0, l, false, false)))
            .collect();
        self.ses_menu(x, y, &items);
    }

    fn ses_theme(&self) -> Theme {
        self.ses
            .win
            .as_ref()
            .map_or_else(|| Theme::dark(1.0), |w| w.theme.clone())
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    pub(crate) fn ses_start(&mut self) {
        self.queue(WinOp::SesOpen);
    }

    pub(crate) fn ses_in_band(&self) -> bool {
        let Some(w) = &self.ses.win else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(w.hwnd)
    }

    pub(crate) fn ses_open(&mut self) {
        if let Some(w) = &self.ses.win {
            match w.place {
                Place::Dock => self.r.dock_activate(w.hwnd),
                Place::Float => win::show(w.hwnd),
                Place::Arrange => {
                    let h = w.hwnd;
                    if !self.ses_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true) {
                        self.ses_render(true);
                    }
                }
            }
            return;
        }
        let place = self.ses.place;
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 900, 640, mode) else {
            self.host
                .error("la fenetre de la Session n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange && !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.ses.place = Place::Dock;
            self.ses_open();
            return;
        }
        self.ses_adopt(hwnd);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.ses_render(true);
        }
        self.ses_keep("ses_open", "1");
        if self.host.ext().is_none() {
            self.ses_say("MIDI clips need REAPER to restart (new loader)");
        }
    }

    pub(crate) fn ses_adopt(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.ses.win = Some(SWin {
            hwnd,
            place: self.ses.place,
            frame: crate::frame::Frame::default(),
            theme: self.reaper_theme(scale),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            closing: false,
            tip: None,
            hover: (Hit::None, Instant::now()),
        });
        cp_sys::shell::accept_files(hwnd, true);
        self.ses.state_seen = i32::MIN;
        self.ses.view.mark();
        self.ses_tick();
        self.ses_render(true);
    }

    pub(crate) fn ses_shutdown(&mut self) {
        let h = self.ses.win.as_ref().map_or(0, |w| w.hwnd.0);
        if let Some(w) = &self.ses.win {
            self.host.frames(w.hwnd, false);
        }
        self.r.ext_set(SECTION, "ses_hwnd", &h.to_string(), false);
        if self.ses.ses.dirty != self.ses.saved {
            self.ses_save_now();
        }
        // Ce qui joue s'arrete : le module suivant repart d'une grille
        // relue (les voix et les lanes ne lui appartiendraient plus).
        self.ses_with(|s, e| s.panic(e));
    }

    pub(crate) fn ses_close(&mut self, user: bool) {
        let in_band = self.ses_in_band();
        if let Some(w) = &mut self.ses.win {
            w.closing = true;
            let h = w.hwnd;
            if in_band {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.ses.win = None;
        if user {
            self.ses_keep("ses_open", "0");
        }
    }

    pub(crate) fn ses_set_place(&mut self, p: Place) {
        let reopen = self.ses.win.as_ref().is_some_and(|w| w.place != p);
        self.ses.place = p;
        self.ses_keep("ses_place", p.key());
        if reopen {
            self.ses_close(false);
        }
        self.ses_open();
    }

    pub(crate) fn ses_place_menu_at(&mut self, owner: Hwnd, sx: i32, sy: i32) {
        let p = self.ses.place;
        let open = self.ses.win.is_some();
        let items = [
            Some(item(1, "Show in arrange view", p == Place::Arrange, true)),
            Some(item(2, "Dock in docker", p == Place::Dock, true)),
            Some(item(3, "Floating window", p == Place::Float, true)),
            None,
            Some(item(
                20,
                "Docker handle: auto",
                self.handle_mode() == 0,
                true,
            )),
            Some(item(
                21,
                "Docker handle: always",
                self.handle_mode() == 1,
                true,
            )),
            Some(item(
                22,
                "Docker handle: never",
                self.handle_mode() == 2,
                true,
            )),
            None,
            Some(item(4, "Close the Session", false, open)),
        ];
        match ui::popup_menu(owner, sx, sy, &items) {
            k @ 20..=22 => self.set_handle_mode((k - 20) as u8),
            1 => self.queue(WinOp::SesPlace(Place::Arrange)),
            2 => self.queue(WinOp::SesPlace(Place::Dock)),
            3 => self.queue(WinOp::SesPlace(Place::Float)),
            4 => self.queue(WinOp::SesClose),
            _ => {}
        }
    }

    fn ses_want_frames(&mut self, on: bool) {
        if let Some(w) = &mut self.ses.win {
            if on {
                w.frame.wake();
            }
            if w.frame.frames_on != on {
                w.frame.frames_on = on;
                self.host.frames(w.hwnd, on);
            }
        }
    }

    fn ses_kick(&mut self) {
        let on = self.ses.win.as_ref().is_some_and(|w| w.frame.frames_on);
        if !on {
            self.ses_render(false);
        }
        self.ses_want_frames(true);
    }

    pub(crate) fn ses_render(&mut self, force: bool) {
        let Some(w) = self.ses.win.as_mut() else {
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
            let th = self.reaper_theme(scale);
            let Some(w) = self.ses.win.as_mut() else {
                return;
            };
            w.theme = th;
            self.ses.view.mark();
        }
        let Some(w) = self.ses.win.as_mut() else {
            return;
        };
        if w.frame.resize(cw, ch) || force {
            self.ses.view.mark();
        }
        if !self.ses.view.take_dirty() {
            w.frame.show(w.hwnd, &[], &[]);
            return;
        }
        let damage = vec![IRect::new(0, 0, cw, ch)];
        let now = self.host.audio_now();
        let t = self.ses.t0.elapsed().as_secs_f64();
        let msg = self.ses.flash.as_ref().map(|f| f.0.clone());
        let (view, ses, strips) = (&mut self.ses.view, &self.ses.ses, &self.ses.strips);
        let (text, icons, th) = (&mut self.text, &mut self.icons, &w.theme);
        w.frame.paint(&damage, &mut self.scratch, |cv| {
            view.draw(
                cv,
                text,
                icons,
                th,
                ses,
                strips,
                now,
                t,
                msg.as_deref(),
                cw as f32,
                ch as f32,
            );
        });
        w.frame.show(w.hwnd, &damage, &[]);
    }

    fn ses_feed(&mut self, ev: Ev) {
        let th = self.ses_theme();
        let a = self
            .ses
            .view
            .event(&self.ses.ses, &self.ses.strips, &th, &ev);
        if let Some(a) = a {
            self.ses_act(a);
        }
        self.ses_kick();
    }

    pub(crate) fn ses_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.ses.win.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        match m.msg {
            cp_sys::shell::WM_DROPFILES => {
                let (files, dx, dy) = cp_sys::shell::take_drop(m.wp);
                self.ses_drop(files, dx as f32, dy as f32);
                self.ses_kick();
                Some(0)
            }
            WM_CP_FRAME => {
                if self.ses_animating() {
                    self.ses.view.mark();
                }
                self.ses_render(false);
                let idle = self.ses.win.as_ref().is_none_or(|w| w.frame.idle());
                if !self.ses_animating() && idle {
                    self.ses_want_frames(false);
                }
                Some(0)
            }
            WM_CP_ACCEL => {
                let Some(k) = key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                Some(match k.message {
                    msg::KEYDOWN | msg::SYSKEYDOWN => {
                        let th = self.ses_theme();
                        match self.ses.view.key(&self.ses.ses, &th, vk, md) {
                            Some(a) => {
                                self.last_mods = md;
                                self.ses_act(a);
                                self.ses_kick();
                                1
                            }
                            None => 0,
                        }
                    }
                    _ => 0,
                })
            }
            msg::SIZE => {
                self.ses.view.mark();
                self.ses_render(true);
                Some(0)
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                let shown = self.ses.win.as_mut().is_some_and(|w| w.frame.repaint(hwnd));
                if !shown {
                    self.ses_render(true);
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001),
            // L'onglet de la vue ancree : le curseur de REAPER pour
            // deplacer ; ailleurs, celui de la fenetre.
            msg::SETCURSOR if (m.lp & 0xFFFF) == 1 => {
                let (cx, cy) = Self::mouse_over(hwnd)?;
                if self.ses.view.on_tab(cx, cy) {
                    ui::set_pointer(cp_ui::Pointer::ItemMove);
                    Some(1)
                } else {
                    None
                }
            }
            msg::MOUSEMOVE => {
                if let Some(w) = &mut self.ses.win {
                    if !w.tracking {
                        w.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.ses_feed(Ev::Move { x, y, mods: md });
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(w) = &mut self.ses.win {
                    w.tracking = false;
                }
                if !self.ses.view.dragging() {
                    self.ses_feed(Ev::Leave);
                }
                Some(0)
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                ui::capture(hwnd);
                let b = match m.msg {
                    msg::LBUTTONDOWN | msg::LBUTTONDBLCLK => Button::Left,
                    _ => Button::Right,
                };
                self.last_mods = md;
                self.ses_feed(Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                });
                Some(0)
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                let b = if m.msg == msg::LBUTTONUP {
                    Button::Left
                } else {
                    Button::Right
                };
                self.ses_feed(Ev::Up { x, y, b, mods: md });
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
                self.ses_feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
                Some(0)
            }
            msg::CLOSE => {
                self.queue(WinOp::SesClose);
                Some(0)
            }
            msg::DESTROY => {
                let ours = self.ses.win.as_ref().is_some_and(|w| w.closing);
                if !ours {
                    self.ses.win = None;
                }
                None
            }
            _ => None,
        }
    }

    /// L'auto-test, en deux temps (l'action appelee deux fois par le
    /// chargeur) : 1) un clip MIDI dans la piste « CPS test synth », une
    /// case audio dans « CPS test audio », lances ; 2) ce qui a sonne, puis
    /// tout s'arrete.
    pub(crate) fn ses_selftest(&mut self) {
        let r = self.r;
        let prev = r.ext_get(SECTION, "selftest_session").unwrap_or_default();
        // 3) Arrange Rec : le transport joue, la case du synthe joue ; 4)
        // l'arret ; 5) ce qui a ete ecrit dans l'arrangement.
        if prev.starts_with("report") {
            let Some(cs) = self
                .ses
                .ses
                .cols
                .iter()
                .position(|c| c.track.name == "CPS test synth")
            else {
                r.ext_set(SECTION, "selftest_session", "arr_done items=-1", false);
                return;
            };
            self.ses.ses.q = Quant::Off;
            // Depuis le debut du projet, sans la boucle de REAPER (rendue a
            // la fin).
            let rep = r.repeat();
            r.ext_set(
                SECTION,
                "selftest_session_repeat",
                if rep { "1" } else { "0" },
                false,
            );
            if rep {
                r.main_on_command(crate::tview::cmd::REPEAT);
            }
            r.set_edit_cursor(0.0);
            self.ses_arrange_rec();
            self.ses_with(|s, e| {
                s.launch(e, cs, 0);
            });
            let c = &self.ses.ses.cols[cs];
            self.host.info(&format!(
                "Session (auto-test) : Arrange Rec lance ; horloge {:?} arr_rec {} attente {:?} suivante {} transport {}",
                self.ses.ses.clock,
                self.ses.ses.arr_rec,
                c.run.wait,
                c.run.next.is_some(),
                r.play_state()
            ));
            r.ext_set(SECTION, "selftest_session", "arr_started", false);
            return;
        }
        if prev.starts_with("arr_started") {
            let s = &self.ses.ses;
            self.host.info(&format!(
                "Session (auto-test) : avant l'arret ; journal {} ; joue {:?} ; temps {:?}",
                s.arr_log.len(),
                s.cols
                    .iter()
                    .map(|c| c.run.play.as_ref().map(|p| p.start_b))
                    .collect::<Vec<_>>(),
                s.tl
            ));
            r.main_on_command(1016);
            r.ext_set(SECTION, "selftest_session", "arr_stopped", false);
            return;
        }
        if prev.starts_with("arr_stopped") {
            if r.ext_get(SECTION, "selftest_session_repeat").as_deref() == Some("1") && !r.repeat()
            {
                r.main_on_command(crate::tview::cmd::REPEAT);
            }
            let snap = r.snapshot_tracks(&[], &[], &|_| true);
            let synth = snap.iter().find(|t| t.name == "CPS test synth");
            let items = synth.map_or(0, |t| t.items.len());
            let notes = synth
                .and_then(|t| t.items.first())
                .and_then(|it| r.ed_notes(&it.guid))
                .map_or(0, |n| n.len());
            r.ext_set(
                SECTION,
                "selftest_session",
                &format!("arr_done items={items} notes={notes}"),
                false,
            );
            return;
        }
        let first = !prev.starts_with("launched");
        if first {
            self.ses.proj = None;
            self.ses_project();
        }
        self.ses_refresh_tracks();
        let find = |s: &Session, name: &str| s.cols.iter().position(|c| c.track.name == name);
        let (Some(cs), Some(ca)) = (
            find(&self.ses.ses, "CPS test synth"),
            find(&self.ses.ses, "CPS test audio"),
        ) else {
            r.ext_set(SECTION, "selftest_session", "report pistes=absentes", false);
            return;
        };
        if first {
            let notes: Vec<cp_session::Note> = (0..8)
                .map(|k| cp_session::Note {
                    start: k as f64 * 0.5,
                    len: 0.4,
                    pitch: 60 + (k % 4) as u8 * 4,
                    vel: 110,
                    chan: 0,
                    prob: 100,
                })
                .collect();
            let id = self.ses.ses.fresh_id();
            self.ses.ses.put(cs, 0, Clip::midi(id, 1.0, notes));
            let wav = r
                .ext_get(SECTION, "selftest_session_wav")
                .unwrap_or_default();
            let id = self.ses.ses.fresh_id();
            let mut c = Clip::audio(id, &wav);
            c.bars = 1.0;
            if let Kind::Audio(a) = &mut c.kind {
                a.tempo = TempoMode::None;
                a.looped = true;
            }
            self.ses.ses.put(ca, 0, c);
            // Le fichier se charge d'abord ; le lancement attend.
            self.ses.ses.q = Quant::Off;
            self.ses_with(|s, e| {
                s.launch(e, cs, 0);
                s.launch(e, ca, 0);
            });
            r.ext_set(SECTION, "selftest_session", "launched", false);
            return;
        }
        let ps = self.ses.ses.cols[cs].port;
        let pa = self.ses.ses.cols[ca].port;
        let lane = self.host.lane_query(ps);
        let sent = self.host.midi_sent(ps);
        let gs = self.ses.ses.cols[cs].track.guid.clone();
        let ga = self.ses.ses.cols[ca].track.guid.clone();
        let synth = r.track_peak(&gs);
        let lv = self.host.audio_levels(pa);
        let audio = r.track_peak(&ga).max(lv[0]).max(lv[1]);
        let line = self.ses_state_line();
        self.ses_with(|s, e| s.panic(e));
        let stopped = self.ses.ses.cols.iter().all(|c| c.run.play.is_none());
        r.ext_set(
            SECTION,
            "selftest_session",
            &format!(
                "report lanes={} lane_state={} midi_sent={} synth_peak={:.4} audio_peak={:.4} stopped={} {}",
                self.host.ext().is_some() as u8,
                lane.map_or(-1, |l| l.state),
                sent,
                synth,
                audio,
                stopped as u8,
                line
            ),
            false,
        );
    }

    /// L'etat de la Session pour l'auto-test (une ligne).
    pub(crate) fn ses_state_line(&self) -> String {
        let s = &self.ses.ses;
        let cells: Vec<String> = s
            .cols
            .iter()
            .enumerate()
            .flat_map(|(i, _)| {
                (0..s.scenes).filter_map(move |k| {
                    let st = s.state(i, k);
                    (st != CellState::Empty).then(|| format!("{i}:{k}:{st:?}"))
                })
            })
            .collect();
        format!(
            "cols={} clips={} lanes={} cells={}",
            s.cols.len(),
            s.cells.len(),
            self.host.ext().is_some() as u8,
            cells.join(",")
        )
    }
}

fn item(id: u32, text: &str, checked: bool, enabled: bool) -> MenuItem<'_> {
    MenuItem {
        id,
        text,
        checked,
        enabled,
    }
}
