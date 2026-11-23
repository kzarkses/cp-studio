//! La grille et ce qui la fait jouer.
//!
//! Chaque geste devient des commandes DATEES au moteur, postees tout de
//! suite : une frontiere tombe a l'echantillon meme si le fil principal
//! s'arrete. Le battement (`tick`) ne fait que constater (une case attendue
//! qui part, une prise finie) et preparer ce qui depend de ce qui vient de
//! jouer (la passe suivante d'un one-shot, une suite de lecture).

use std::collections::HashMap;

use crate::clip::{
    Clip, Kind, Note, TempoMode, TempoWhy, bars_for, tempo_from_length, tempo_from_name,
};
use crate::time::{ClockMode, LATENCY_S, Quant, Timeline};

// ---------------------------------------------------------------------------
// Le moteur, vu de la Session
// ---------------------------------------------------------------------------

/// Un clip MIDI tel que la lane le joue.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneClip {
    pub id: u64,
    /// (debut, longueur, canal, hauteur, velocite), en noires.
    pub notes: Vec<(f64, f64, u8, u8, u8)>,
    pub len: f64,
    pub brace: (f64, f64),
    pub looped: bool,
}

/// Un fichier audio pret dans le moteur.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSrc {
    pub clip: u32,
    pub frames: i64,
    pub srate: f64,
    /// Le tempo lu dans le fichier (REAPER), s'il en declare un.
    pub file_bpm: Option<f64>,
}

/// Ou en est une prise audio.
#[derive(Clone, Debug, PartialEq)]
pub enum RecPoll {
    Recording,
    Ready(AudioSrc),
    Failed,
}

/// Ce que la Session demande au moteur (le chargeur dans REAPER, un faux
/// dans les essais). Les frames sont celles du moteur.
pub trait Engine {
    fn now(&self) -> i64;
    fn srate(&self) -> f64;
    /// Le moteur sait jouer des clips MIDI (l'extension du chargeur).
    fn has_lanes(&self) -> bool;
    /// Combien de ports.
    fn ports(&self) -> u32;
    /// Fait jouer un port dans la piste `track` (GUID).
    fn route(&mut self, port: u32, track: &str) -> bool;
    fn lane_play(&mut self, port: u32, clip: &LaneClip, from: f64, fpb: f64, at: i64) -> bool;
    fn lane_update(&mut self, port: u32, clip: &LaneClip) -> bool;
    fn lane_stop(&mut self, port: u32, at: i64) -> bool;
    /// Retire les lancements et arrets qui attendent encore.
    fn lane_cancel(&mut self, port: u32);
    fn lane_rate(&mut self, port: u32, fpb: f64, at: i64);
    /// Un fichier : `None` tant qu'il se charge.
    fn audio_src(&mut self, path: &str) -> Option<Result<AudioSrc, String>>;
    #[allow(clippy::too_many_arguments)]
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
    ) -> Option<u32>;
    /// Arrete une voix (`at` : a ce frame, le fondu fini ; `None` :
    /// maintenant). Une voix qui attend son depart s'annule.
    fn voice_stop(&mut self, voice: u32, at: Option<i64>, fade_s: f64);
    /// Annule un arret date pas encore atteint.
    fn voice_keep(&mut self, voice: u32);
    fn voice_rate(&mut self, voice: u32, rate: f64);
    fn midi_rec_start(&mut self, dev: i32, at: i64) -> u32;
    fn midi_rec_stop(&mut self, id: u32, at: i64);
    /// Les messages d'une prise finie (frame depuis son debut, octets) ;
    /// `None` tant qu'elle court.
    fn midi_rec_take(&mut self, id: u32) -> Option<Vec<(i64, [u8; 3])>>;
    fn audio_rec_start(&mut self, ch0: i32, ch1: i32, at: i64) -> u32;
    fn audio_rec_stop(&mut self, id: u32, at: i64);
    fn audio_rec_poll(&mut self, id: u32) -> RecPoll;
    /// Ecrit une prise sur le disque ; rend son chemin.
    fn save_take(&mut self, clip: u32, name: &str) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Les colonnes
// ---------------------------------------------------------------------------

/// Ce qu'une colonne fait a la fin d'une passe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    Stay,
    /// Une passe, puis la colonne se tait.
    OneShot,
    /// La case suivante, et revient en haut.
    MarchWrap,
    /// La case suivante, et reste sur la derniere.
    MarchStay,
    /// La case suivante, et se tait apres la derniere.
    MarchStop,
    Random,
    /// Au hasard, jamais la meme.
    ExclusiveRandom,
}

impl Motion {
    pub const ALL: [Motion; 7] = [
        Motion::Stay,
        Motion::OneShot,
        Motion::MarchWrap,
        Motion::MarchStay,
        Motion::MarchStop,
        Motion::Random,
        Motion::ExclusiveRandom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Motion::Stay => "Stay",
            Motion::OneShot => "One shot",
            Motion::MarchWrap => "March & wrap",
            Motion::MarchStay => "March & stay",
            Motion::MarchStop => "March & stop",
            Motion::Random => "Random",
            Motion::ExclusiveRandom => "Exclusive random",
        }
    }
    /// Le petit signe de l'en-tete.
    pub fn tag(self) -> &'static str {
        match self {
            Motion::Stay => "",
            Motion::OneShot => "1x",
            Motion::MarchWrap => "M>",
            Motion::MarchStay => "M=",
            Motion::MarchStop => "M.",
            Motion::Random => "R?",
            Motion::ExclusiveRandom => "R!",
        }
    }
    pub fn key(self) -> u8 {
        Motion::ALL.iter().position(|m| *m == self).unwrap_or(0) as u8
    }
    pub fn of_key(k: u8) -> Motion {
        Motion::ALL.get(k as usize).copied().unwrap_or_default()
    }
}

/// L'entree d'une piste (son `I_RECINPUT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Input {
    #[default]
    None,
    /// Une ou deux entrees de la carte (`ch1` < 0 : mono).
    Audio(i32, i32),
    /// Une entree MIDI (-1 : toutes).
    Midi(i32),
}

/// Une piste telle que REAPER la donne, a chaque relecture.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrackInfo {
    pub guid: String,
    pub name: String,
    pub color: Option<(u8, u8, u8)>,
    pub armed: bool,
    pub input: Input,
    /// La piste porte un instrument (elle avale l'audio qu'on y verse).
    pub instrument: bool,
}

/// Ce qui joue (ou va partir) dans une colonne.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub scene: usize,
    pub clip: u64,
    /// Le frame du depart (la frontiere).
    pub start_f: i64,
    /// La noire du depart sur la ligne de temps.
    pub start_b: f64,
    /// La position de depart dans le clip.
    pub from: f64,
    /// Ce qui boucle, en noires du clip, et la longueur du clip.
    pub span: (f64, f64),
    pub len: f64,
    /// Une case audio : sa voix, et ce qu'il faut pour la relancer.
    pub voice: Option<u32>,
    pub audio: Option<AudioPlay>,
    /// La passe pour laquelle une suite (Motion) est deja datee.
    pub motion_for: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioPlay {
    pub src: AudioSrc,
    pub start: i64,
    pub end: i64,
    pub looped: bool,
    pub gain: f64,
    pub rate: f64,
    /// La passe suivante d'un one-shot deja programmee (son frame).
    pub next_pass: i64,
    /// La voix de la passe d'avant (un one-shot qui sonne encore).
    pub prev: Option<u32>,
}

/// Une prise en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    pub scene: usize,
    pub midi: bool,
    pub id: u32,
    pub start_f: i64,
    pub end_f: i64,
    pub beats: f64,
    /// La prise a ete arretee a la main avant sa fin.
    pub cut: bool,
}

/// Un overdub : une prise par passe sur le clip MIDI qui joue.
#[derive(Clone, Debug, PartialEq)]
pub struct Overdub {
    /// Les prises en cours ou finies pas encore fondues : (id, debut).
    pub takes: Vec<(u32, i64, i64)>,
    pub stopping: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ColRun {
    pub play: Option<Play>,
    /// Un lancement qui attend sa frontiere.
    pub next: Option<Play>,
    /// Un arret qui attend sa frontiere.
    pub stop_at: Option<i64>,
    /// Une case qui attend : le transport (Follow) ou son fichier.
    pub wait: Option<usize>,
    pub rec: Option<Take>,
    /// Une prise qui attend le transport : (scene).
    pub rec_wait: Option<usize>,
    pub dub: Option<Overdub>,
    /// Les passes jouees depuis le lancement.
    pub passes: u32,
}

/// Une colonne : une piste.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Column {
    pub track: TrackInfo,
    /// La piste a disparu mais la colonne garde des clips.
    pub held: bool,
    pub port: u32,
    pub motion: Motion,
    pub run: ColRun,
}

/// Ce qu'une case montre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    Empty,
    Stopped,
    /// Attend sa frontiere.
    Queued,
    Playing,
    /// Joue, et s'arretera a la frontiere.
    Stopping,
    /// Attend le transport (Follow) ou son fichier.
    Waiting,
    /// Une prise attend sa frontiere.
    RecQueued,
    Recording,
    Overdub,
}

/// Un message bref pour la barre d'etat.
#[derive(Clone, Debug, PartialEq)]
pub struct Flash(pub String);

// ---------------------------------------------------------------------------
// La Session
// ---------------------------------------------------------------------------

pub struct Session {
    pub cols: Vec<Column>,
    /// Les clips, par (piste, scene).
    pub cells: HashMap<(String, usize), Clip>,
    pub scenes: usize,
    pub q: Quant,
    pub rec_bars: u32,
    pub clock: ClockMode,
    pub tl: Timeline,
    /// La case choisie (clavier).
    pub sel: (usize, usize),
    pub next_id: u64,
    /// La derniere case effacee (Ctrl+Z).
    pub erased: Option<(String, usize, Clip)>,
    /// Les ports deja donnes : port -> piste.
    slots: Vec<Option<String>>,
    pub flash: Option<Flash>,
    /// Un compteur qui monte a chaque changement a enregistrer.
    pub dirty: u64,
    rng: u64,
    /// Les noires par minute de la ligne de temps (pour l'affichage).
    pub bpm: f64,
    /// Le frame du moteur au dernier battement.
    pub now_seen: i64,
    /// Les suites de lecture lues avant les colonnes.
    pub pending_motion: Option<String>,
}

/// Au moins tant de scenes, et une rangee vide sous la derniere pleine.
pub const MIN_SCENES: usize = 8;

impl Default for Session {
    fn default() -> Session {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Session {
        Session {
            cols: Vec::new(),
            cells: HashMap::new(),
            scenes: MIN_SCENES,
            q: Quant::Bar,
            rec_bars: 1,
            clock: ClockMode::Free,
            tl: Timeline::default(),
            sel: (0, 0),
            next_id: 1_000_001,
            erased: None,
            slots: Vec::new(),
            flash: None,
            dirty: 0,
            rng: 0x2545_F491_4F6C_DD1D,
            bpm: 120.0,
            now_seen: 0,
            pending_motion: None,
        }
    }

    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn say(&mut self, s: impl Into<String>) {
        self.flash = Some(Flash(s.into()));
    }

    fn touch(&mut self) {
        self.dirty += 1;
        self.fit_scenes();
    }

    /// Le nombre de scenes : au moins 8, et une rangee vide en bas.
    pub fn fit_scenes(&mut self) {
        let last = self.cells.keys().map(|k| k.1 + 1).max().unwrap_or(0);
        self.scenes = self.scenes.max(MIN_SCENES).max(last + 1);
    }

    pub fn clip(&self, col: usize, scene: usize) -> Option<&Clip> {
        let c = self.cols.get(col)?;
        self.cells.get(&(c.track.guid.clone(), scene))
    }

    pub fn clip_mut(&mut self, col: usize, scene: usize) -> Option<&mut Clip> {
        let g = self.cols.get(col)?.track.guid.clone();
        self.cells.get_mut(&(g, scene))
    }

    pub fn col_of(&self, guid: &str) -> Option<usize> {
        self.cols.iter().position(|c| c.track.guid == guid)
    }

    /// Les noires d'une mesure du projet.
    pub fn qpb(&self) -> f64 {
        self.tl.qpb
    }

    // -----------------------------------------------------------------------
    // Les colonnes suivent les pistes
    // -----------------------------------------------------------------------

    /// Les pistes du projet, dans leur ordre. Une colonne garde ce qui joue
    /// d'une relecture a l'autre ; une piste disparue qui porte des clips
    /// reste, « tenue », a la fin.
    pub fn set_tracks(&mut self, e: &mut dyn Engine, tracks: Vec<TrackInfo>) {
        let mut old: Vec<Column> = std::mem::take(&mut self.cols);
        let mut cols = Vec::with_capacity(tracks.len());
        for t in tracks {
            let c = match old.iter().position(|c| c.track.guid == t.guid) {
                Some(i) => {
                    let mut c = old.swap_remove(i);
                    c.track = t;
                    c.held = false;
                    c
                }
                None => Column {
                    track: t,
                    ..Default::default()
                },
            };
            cols.push(c);
        }
        // Les pistes disparues : leurs voix se taisent ; leurs clips restent.
        for mut c in old {
            self.silence(e, &mut c);
            if self.cells.keys().any(|k| k.0 == c.track.guid) {
                c.held = true;
                c.run = ColRun::default();
                cols.push(c);
            } else if let Some(s) = self
                .slots
                .iter_mut()
                .find(|s| s.as_deref() == Some(&c.track.guid))
            {
                *s = None;
            }
        }
        // Les colonnes tenues dont la piste n'est plus la, a la fin.
        let held: Vec<String> = self
            .cells
            .keys()
            .map(|k| k.0.clone())
            .filter(|g| !cols.iter().any(|c: &Column| &c.track.guid == g))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for g in held {
            cols.push(Column {
                track: TrackInfo {
                    guid: g,
                    ..Default::default()
                },
                held: true,
                ..Default::default()
            });
        }
        self.cols = cols;
        for i in 0..self.cols.len() {
            if self.cols[i].held {
                continue;
            }
            let g = self.cols[i].track.guid.clone();
            let p = self.port_for(e, &g);
            self.cols[i].port = p;
        }
        if self.sel.0 >= self.cols.len() {
            self.sel.0 = self.cols.len().saturating_sub(1);
        }
    }

    /// Le port d'une piste : le meme tant que la piste vit ; 0 = aucun.
    fn port_for(&mut self, e: &mut dyn Engine, guid: &str) -> u32 {
        let n = e.ports().max(2) as usize;
        if self.slots.len() < n {
            self.slots.resize(n, None);
        }
        if let Some(p) = self.slots.iter().position(|s| s.as_deref() == Some(guid)) {
            return p as u32;
        }
        // Le port 0 est celui du navigateur.
        if let Some(p) = (1..n).find(|&p| self.slots[p].is_none()) {
            self.slots[p] = Some(guid.to_string());
            e.route(p as u32, guid);
            return p as u32;
        }
        0
    }

    /// Les voix et la lane d'une colonne, coupees tout de suite.
    fn silence(&self, e: &mut dyn Engine, c: &mut Column) {
        if c.port == 0 {
            return;
        }
        let now = e.now();
        e.lane_cancel(c.port);
        e.lane_stop(c.port, now);
        for p in [c.run.play.take(), c.run.next.take()].into_iter().flatten() {
            if let Some(v) = p.voice {
                e.voice_stop(v, None, 0.005);
            }
        }
        c.run.stop_at = None;
    }

    // -----------------------------------------------------------------------
    // Le temps
    // -----------------------------------------------------------------------

    /// La ligne de temps de REAPER (a chaque battement) : le frame du
    /// moteur MAINTENANT, la noire du projet a ce frame, les frames par
    /// noire, les noires par mesure, le transport joue. En Free, seuls le
    /// tempo et la signature comptent.
    pub fn sync_clock(
        &mut self,
        e: &mut dyn Engine,
        now: i64,
        qn: f64,
        fpb: f64,
        qpb: f64,
        playing: bool,
    ) {
        let fpb = if fpb.is_finite() && fpb > 1.0 {
            fpb
        } else {
            self.tl.fpb
        };
        self.bpm = 60.0 * e.srate().max(1.0) / fpb;
        self.tl.qpb = if qpb > 0.0 { qpb } else { 4.0 };
        match self.clock {
            ClockMode::Follow => {
                let was = self.tl.running;
                let changed = (fpb - self.tl.fpb).abs() > 1e-6;
                self.tl.f0 = now;
                self.tl.b0 = qn;
                self.tl.fpb = fpb;
                self.tl.running = playing;
                if changed && playing {
                    self.retime(e, now);
                }
                if playing && !was {
                    self.start_waiting(e);
                }
                if !playing && was {
                    self.transport_stopped(e);
                }
            }
            ClockMode::Free => {
                if (fpb - self.tl.fpb).abs() > 1e-6 {
                    // Re-ancre la ou l'on est : rien ne saute.
                    if self.tl.running {
                        let b = self.tl.beat(now);
                        self.tl.f0 = now;
                        self.tl.b0 = b;
                    }
                    self.tl.fpb = fpb;
                    if self.tl.running {
                        self.retime(e, now);
                    }
                }
            }
        }
    }

    /// Un autre tempo : chaque lane qui joue suit, les cases audio prennent
    /// leur nouveau taux.
    fn retime(&mut self, e: &mut dyn Engine, now: i64) {
        let fpb = self.tl.fpb;
        let srate = e.srate();
        for c in &mut self.cols {
            let port = c.port;
            for p in [c.run.play.as_mut(), c.run.next.as_mut()]
                .into_iter()
                .flatten()
            {
                match (&mut p.audio, p.voice) {
                    (Some(a), Some(v)) => {
                        let pass_s = p.len * fpb / srate.max(1.0);
                        if let Some(r) = rate_for_pass(a, pass_s) {
                            a.rate = r;
                            e.voice_rate(v, r);
                        }
                    }
                    _ => e.lane_rate(port, fpb, now),
                }
            }
        }
    }

    /// Le temps n'est plus a l'arret : demarre-t-il ici ? (Free)
    fn ensure_running(&mut self, now: i64, lat: i64) -> bool {
        if self.tl.running {
            return true;
        }
        match self.clock {
            ClockMode::Free => {
                self.tl.f0 = now + lat;
                self.tl.b0 = 0.0;
                self.tl.running = true;
                true
            }
            ClockMode::Follow => false,
        }
    }

    fn lat(&self, e: &dyn Engine) -> i64 {
        (LATENCY_S * e.srate().max(1.0)) as i64
    }

    /// Plus rien ne joue ni n'attend : l'horloge libre s'arrete (le prochain
    /// lancement part tout de suite).
    fn idle_check(&mut self) {
        if self.clock == ClockMode::Free
            && self.cols.iter().all(|c| {
                c.run.play.is_none()
                    && c.run.next.is_none()
                    && c.run.rec.is_none()
                    && c.run.wait.is_none()
                    && c.run.rec_wait.is_none()
            })
        {
            self.tl.running = false;
        }
    }

    // -----------------------------------------------------------------------
    // Lancer, arreter
    // -----------------------------------------------------------------------

    /// Le geste sur une case : la lancer ; recliquee pendant qu'elle attend,
    /// l'annuler ; recliquee pendant qu'elle joue, l'arreter a la frontiere.
    pub fn click(&mut self, e: &mut dyn Engine, col: usize, scene: usize) {
        let Some(c) = self.cols.get(col) else { return };
        if c.run.next.as_ref().is_some_and(|p| p.scene == scene) || c.run.wait == Some(scene) {
            self.cancel_next(e, col);
            self.say("Launch cancelled");
            return;
        }
        if c.run.play.as_ref().is_some_and(|p| p.scene == scene) && c.run.stop_at.is_none() {
            self.stop_col(e, col);
            return;
        }
        self.launch(e, col, scene);
    }

    /// Lance une case a la prochaine frontiere de Q (une case qui joue deja
    /// n'est pas relancee).
    pub fn launch(&mut self, e: &mut dyn Engine, col: usize, scene: usize) -> bool {
        let Some(c) = self.cols.get(col) else {
            return false;
        };
        if c.held || c.port == 0 {
            self.say("This column has no track");
            return false;
        }
        let Some(clip) = self.cells.get(&(c.track.guid.clone(), scene)).cloned() else {
            return false;
        };
        if clip.is_empty() {
            self.say("Nothing to play in this clip");
            return false;
        }
        if clip.is_midi() && !e.has_lanes() {
            self.say("MIDI clips need REAPER to restart (new loader)");
            return false;
        }
        // Deja la, sans arret prevu : rien a faire (une scene relancee ne
        // l'arrete pas).
        if c.run.play.as_ref().is_some_and(|p| p.scene == scene)
            && c.run.stop_at.is_none()
            && c.run.next.is_none()
        {
            return true;
        }
        let now = e.now();
        let lat = self.lat(e);
        if !self.ensure_running(now, lat) {
            self.cancel_next(e, col);
            self.cols[col].run.wait = Some(scene);
            return true;
        }
        let q = self.q.beats(self.tl.qpb);
        let (tb, tf) = self.tl.target(now, q, lat);
        self.launch_at(e, col, scene, clip, tb, tf)
    }

    /// Programme `clip` dans la colonne a la frontiere (tb, tf).
    fn launch_at(
        &mut self,
        e: &mut dyn Engine,
        col: usize,
        scene: usize,
        mut clip: Clip,
        tb: f64,
        tf: i64,
    ) -> bool {
        let now = e.now();
        let lat = self.lat(e);
        self.cancel_next(e, col);
        let port = self.cols[col].port;
        let qpb = self.tl.qpb;
        let fpb = self.tl.fpb;
        let srate = e.srate().max(1.0);
        let mut play = Play {
            scene,
            clip: clip.id,
            start_f: tf,
            start_b: tb,
            from: 0.0,
            span: clip.span(qpb),
            len: clip.len_beats(qpb),
            voice: None,
            audio: None,
            motion_for: i64::MIN,
        };
        match &clip.kind {
            Kind::Midi(_) => {
                let lc = lane_clip(&clip, qpb);
                play.from = if clip.phase_lock {
                    let (a, b) = play.span;
                    a + (tb - a).rem_euclid((b - a).max(1e-9))
                } else {
                    play.span.0
                };
                if !e.lane_play(port, &lc, play.from, fpb, tf) {
                    self.say("The engine refused the clip");
                    return false;
                }
                // Une case audio qui jouait se tait a la frontiere.
                if let Some(v) = self.cols[col].run.play.as_ref().and_then(|p| p.voice) {
                    e.voice_stop(v, Some(tf), 0.005);
                }
            }
            Kind::Audio(a) => {
                let src = match e.audio_src(&a.path) {
                    None => {
                        self.cols[col].run.wait = Some(scene);
                        return true;
                    }
                    Some(Err(err)) => {
                        self.say(format!("Cannot play {}: {err}", clip.name));
                        return false;
                    }
                    Some(Ok(s)) => s,
                };
                let (bpm, why) = source_tempo(&clip, &src, qpb, self.bpm);
                let _ = why;
                let region_s = region_seconds(a, &src);
                let mut rate = match (a.tempo, bpm) {
                    (TempoMode::None, _) | (_, None) => 1.0,
                    (_, Some(b)) => self.bpm / b,
                };
                // La passe : la plus courte longueur qui contient la region.
                if clip.bars <= 0.0 {
                    let region_beats = region_s / rate * self.bpm / 60.0;
                    clip.bars = bars_for(region_beats, clip.qpb(qpb));
                    if let Some(c) = self.clip_mut(col, scene) {
                        c.bars = clip.bars;
                    }
                    self.touch();
                }
                play.len = clip.len_beats(qpb);
                play.span = (0.0, play.len);
                let pass_s = play.len * fpb / srate;
                let mut ap = AudioPlay {
                    src: src.clone(),
                    start: (a.offs * src.srate) as i64,
                    end: if a.len > 0.0 {
                        ((a.offs + a.len) * src.srate) as i64
                    } else {
                        src.frames
                    },
                    looped: a.looped,
                    gain: a.gain,
                    rate,
                    next_pass: i64::MIN,
                    prev: None,
                };
                if a.tempo != TempoMode::None {
                    if let Some(r) = rate_for_pass(&ap, pass_s) {
                        rate = r;
                        ap.rate = r;
                    }
                }
                // Un lancement tardif (dans la tolerance) entre en phase.
                let late = (now + lat - tf).max(0);
                let start = ap.start + (late as f64 * rate * src.srate / srate) as i64;
                let at = tf.max(now + lat);
                let v = e.voice_play(
                    port,
                    &src,
                    start.min(ap.end - 1).max(0),
                    ap.end,
                    ap.looped,
                    ap.gain,
                    rate,
                    at,
                );
                let Some(v) = v else {
                    self.say("The engine refused the sound");
                    return false;
                };
                play.voice = Some(v);
                if !ap.looped {
                    ap.next_pass = tf + (play.len * fpb).round() as i64;
                }
                play.audio = Some(ap);
                // Ce qui jouait se tait a la frontiere.
                let old = self.cols[col]
                    .run
                    .play
                    .as_ref()
                    .map(|p| (p.voice, p.audio.is_some()));
                match old {
                    Some((Some(ov), _)) => e.voice_stop(ov, Some(tf), 0.005),
                    Some((None, false)) => {
                        e.lane_stop(port, tf);
                    }
                    _ => {}
                }
            }
        }
        // La case precedente d'un one-shot sonne peut-etre encore : elle
        // aussi se tait a la frontiere.
        if let Some(pv) = self.cols[col]
            .run
            .play
            .as_ref()
            .and_then(|p| p.audio.as_ref())
            .and_then(|a| a.prev)
        {
            e.voice_stop(pv, Some(tf), 0.005);
        }
        let run = &mut self.cols[col].run;
        run.wait = None;
        run.next = Some(play);
        true
    }

    /// Retire ce qui attend sa frontiere dans la colonne : un lancement (sa
    /// voix s'annule, la lane l'oublie) et un arret. Ce qui joue continue.
    fn cancel_next(&mut self, e: &mut dyn Engine, col: usize) {
        let Some(c) = self.cols.get_mut(col) else {
            return;
        };
        c.run.wait = None;
        let had = c.run.next.is_some() || c.run.stop_at.is_some();
        if let Some(n) = c.run.next.take() {
            if let Some(v) = n.voice {
                e.voice_stop(v, None, 0.0);
            }
        }
        if had && c.port != 0 {
            e.lane_cancel(c.port);
            if let Some(p) = &c.run.play {
                if let Some(v) = p.voice {
                    e.voice_keep(v);
                }
                if let Some(pv) = p.audio.as_ref().and_then(|a| a.prev) {
                    e.voice_keep(pv);
                }
            }
        }
        c.run.stop_at = None;
    }

    /// Arrete une colonne a la prochaine frontiere de Q (tout de suite si le
    /// temps ne bat pas).
    pub fn stop_col(&mut self, e: &mut dyn Engine, col: usize) {
        let now = e.now();
        let lat = self.lat(e);
        self.cancel_next(e, col);
        let Some(c) = self.cols.get_mut(col) else {
            return;
        };
        if c.run.play.is_none() {
            return;
        }
        // Un arret n'a pas de tolerance : la prochaine frontiere.
        let at = if self.tl.running {
            let q = self.q.beats(self.tl.qpb);
            self.tl.next_after(now, q, lat)
        } else {
            now
        };
        self.stop_at_frame(e, col, at);
    }

    /// Arrete toutes les colonnes a la frontiere.
    pub fn stop_all(&mut self, e: &mut dyn Engine) {
        for i in 0..self.cols.len() {
            self.stop_col(e, i);
            self.cols[i].run.wait = None;
            self.cols[i].run.rec_wait = None;
        }
    }

    /// Tout coupe, tout de suite : voix, lanes, prises, attentes.
    pub fn panic(&mut self, e: &mut dyn Engine) {
        let now = e.now();
        for i in 0..self.cols.len() {
            let mut c = std::mem::take(&mut self.cols[i]);
            self.silence(e, &mut c);
            if let Some(t) = c.run.rec.take() {
                if t.midi {
                    e.midi_rec_stop(t.id, now);
                } else {
                    e.audio_rec_stop(t.id, now);
                }
            }
            if let Some(d) = c.run.dub.take() {
                for (id, _, _) in d.takes {
                    e.midi_rec_stop(id, now);
                }
            }
            c.run = ColRun::default();
            self.cols[i] = c;
        }
        self.tl.running = self.clock == ClockMode::Follow && self.tl.running;
        self.idle_check();
    }

    /// Lance une scene : chaque colonne qui y a un clip le lance, les
    /// autres s'arretent (`keep_others` : elles continuent).
    pub fn launch_scene(&mut self, e: &mut dyn Engine, scene: usize, keep_others: bool) {
        for i in 0..self.cols.len() {
            if self.cols[i].held {
                continue;
            }
            let has = self.clip(i, scene).is_some_and(|c| !c.is_empty());
            if has {
                self.launch(e, i, scene);
            } else if !keep_others {
                self.stop_col(e, i);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Les prises
    // -----------------------------------------------------------------------

    /// Une prise dans une case vide d'une colonne armee : elle part a la
    /// frontiere de Rec mesures (tout de suite si rien ne joue), dure Rec
    /// mesures, puis le clip tourne.
    pub fn record(&mut self, e: &mut dyn Engine, col: usize, scene: usize) -> bool {
        let Some(c) = self.cols.get(col) else {
            return false;
        };
        if c.held || c.port == 0 {
            return false;
        }
        if c.run.rec.is_some() || c.run.rec_wait.is_some() {
            return self.stop_rec(e, col);
        }
        let input = c.track.input;
        let midi = match input {
            Input::Midi(_) => true,
            Input::Audio(..) => false,
            Input::None => {
                self.say("This track has no input");
                return false;
            }
        };
        if midi && !e.has_lanes() {
            self.say("MIDI takes need REAPER to restart (new loader)");
            return false;
        }
        let now = e.now();
        let lat = self.lat(e);
        if !self.ensure_running(now, lat) {
            self.cols[col].run.rec_wait = Some(scene);
            return true;
        }
        let qpb = self.tl.qpb;
        let lb = self.rec_bars.max(1) as f64 * qpb;
        let (sb, sf) = self.tl.target(now, lb, lat);
        let (sb, sf) = if sf < now + lat {
            // Une prise ne commence jamais dans le passe : la frontiere
            // suivante.
            let b = sb + lb;
            (b, self.tl.frame(b))
        } else {
            (sb, sf)
        };
        let ef = self.tl.frame(sb + lb);
        let id = match input {
            Input::Midi(dev) => {
                let id = e.midi_rec_start(dev, sf);
                e.midi_rec_stop(id, ef);
                id
            }
            Input::Audio(a, b) => {
                let id = e.audio_rec_start(a, b, sf);
                e.audio_rec_stop(id, ef);
                id
            }
            Input::None => 0,
        };
        if id == 0 {
            self.say("The engine refused the take");
            return false;
        }
        // Ce qui jouait dans la colonne s'arrete au debut de la prise (la
        // meme frontiere : pas de silence entre les deux).
        self.cancel_next(e, col);
        let c = &mut self.cols[col];
        if let Some(p) = &c.run.play {
            match p.voice {
                Some(v) => e.voice_stop(v, Some(sf), 0.005),
                None => {
                    e.lane_stop(c.port, sf);
                }
            }
            c.run.stop_at = Some(sf);
        }
        c.run.rec = Some(Take {
            scene,
            midi,
            id,
            start_f: sf,
            end_f: ef,
            beats: lb,
            cut: false,
        });
        true
    }

    /// Recliquee : une prise qui attend s'annule, une prise qui court finit
    /// maintenant (arrondie a la mesure du dessus).
    pub fn stop_rec(&mut self, e: &mut dyn Engine, col: usize) -> bool {
        let now = e.now();
        let Some(c) = self.cols.get_mut(col) else {
            return false;
        };
        if c.run.rec_wait.take().is_some() {
            return true;
        }
        let Some(t) = c.run.rec.as_mut() else {
            return false;
        };
        if now < t.start_f {
            // Pas encore partie : annulee.
            if t.midi {
                e.midi_rec_stop(t.id, now);
                let _ = e.midi_rec_take(t.id);
            } else {
                e.audio_rec_stop(t.id, now);
            }
            c.run.rec = None;
            c.run.stop_at = None;
            if let Some(p) = &c.run.play {
                match p.voice {
                    Some(v) => e.voice_keep(v),
                    None => e.lane_cancel(c.port),
                }
            }
            self.say("Take cancelled");
            return true;
        }
        if now >= t.end_f {
            return true;
        }
        let qpb = self.tl.qpb;
        let el = (now - t.start_f) as f64 / self.tl.fpb;
        let bars = (el / qpb).ceil().max(1.0);
        t.beats = bars * qpb;
        t.end_f = now;
        t.cut = true;
        if t.midi {
            e.midi_rec_stop(t.id, now);
        } else {
            e.audio_rec_stop(t.id, now);
        }
        true
    }

    /// L'overdub sur la colonne : les notes jouees rejoignent le clip MIDI
    /// qui tourne, passe par passe. Recliquer l'arrete.
    pub fn overdub(&mut self, e: &mut dyn Engine, col: usize) -> bool {
        let now = e.now();
        let lat = self.lat(e);
        let Some(c) = self.cols.get_mut(col) else {
            return false;
        };
        if let Some(d) = c.run.dub.as_mut() {
            d.stopping = true;
            if let Some((id, _, end)) = d.takes.last_mut() {
                if *end > now {
                    *end = now;
                    e.midi_rec_stop(*id, now);
                }
            }
            return true;
        }
        let Input::Midi(dev) = c.track.input else {
            self.say("Overdub needs a MIDI input on this track");
            return false;
        };
        let Some(p) = &c.run.play else {
            self.say("Nothing plays in this column");
            return false;
        };
        if p.audio.is_some() {
            self.say("Overdub works on MIDI clips");
            return false;
        }
        let start = now + lat;
        let end = next_pass_end(p, self.tl.fpb, start);
        let id = e.midi_rec_start(dev, start);
        if id == 0 {
            return false;
        }
        e.midi_rec_stop(id, end);
        c.run.dub = Some(Overdub {
            takes: vec![(id, start, end)],
            stopping: false,
        });
        true
    }

    // -----------------------------------------------------------------------
    // Le battement
    // -----------------------------------------------------------------------

    /// A chaque battement du fil principal.
    pub fn tick(&mut self, e: &mut dyn Engine) {
        let now = e.now();
        self.now_seen = now;
        let lat = self.lat(e);
        let fpb = self.tl.fpb;
        for i in 0..self.cols.len() {
            // --- un lancement atteint sa frontiere
            let port = self.cols[i].port;
            let run = &mut self.cols[i].run;
            if run.next.as_ref().is_some_and(|n| n.start_f <= now) {
                let n = run.next.take();
                if let Some(old) = run.play.take() {
                    // Deja date a la frontiere ; si l'arret s'est perdu, il
                    // part ici.
                    let at = n.as_ref().map_or(now, |n| n.start_f);
                    if let Some(v) = old.voice {
                        e.voice_stop(v, Some(at), 0.005);
                    }
                    if let Some(v) = old.audio.and_then(|a| a.prev) {
                        e.voice_stop(v, Some(at), 0.005);
                    }
                }
                run.play = n;
                run.stop_at = None;
                run.passes = 0;
            }
            // --- un arret atteint sa frontiere
            if run.stop_at.is_some_and(|s| s <= now) {
                run.stop_at = None;
                run.play = None;
                run.dub = None;
            }
            // --- les passes d'un one-shot audio, et le compte des passes
            if let Some(p) = run.play.as_mut() {
                let pass = (p.len * fpb).max(1.0);
                run.passes = ((now - p.start_f).max(0) as f64 / pass) as u32;
                if let Some(a) = p.audio.as_mut() {
                    if !a.looped
                        && a.next_pass > i64::MIN
                        && run.stop_at.is_none()
                        && run.next.is_none()
                    {
                        // Programmee un peu avant sa frontiere.
                        while a.next_pass <= now + (0.25 * e.srate()) as i64 {
                            let at = a.next_pass.max(now + lat);
                            if let Some(v) = e
                                .voice_play(port, &a.src, a.start, a.end, false, a.gain, a.rate, at)
                            {
                                a.prev = p.voice;
                                p.voice = Some(v);
                            }
                            a.next_pass += pass.round() as i64;
                        }
                    }
                }
            }
        }
        self.tick_takes(e);
        self.tick_dub(e);
        self.tick_motion(e);
        // --- ce qui attendait son fichier
        for i in 0..self.cols.len() {
            if let Some(s) = self.cols[i].run.wait {
                if self.tl.running {
                    let audio_ready = match self.clip(i, s).map(|c| c.kind.clone()) {
                        Some(Kind::Audio(a)) => e.audio_src(&a.path).is_some(),
                        _ => true,
                    };
                    if audio_ready {
                        self.cols[i].run.wait = None;
                        self.launch(e, i, s);
                    }
                }
            }
        }
        let _ = (now, lat);
        self.idle_check();
    }

    fn tick_takes(&mut self, e: &mut dyn Engine) {
        let now = e.now();
        let lat = self.lat(e);
        for i in 0..self.cols.len() {
            let Some(t) = self.cols[i].run.rec.clone() else {
                continue;
            };
            if now < t.end_f {
                continue;
            }
            let guid = self.cols[i].track.guid.clone();
            if t.midi {
                let Some(evs) = e.midi_rec_take(t.id) else {
                    continue;
                };
                self.cols[i].run.rec = None;
                let notes = notes_of(&evs, self.tl.fpb, t.beats);
                if notes.is_empty() {
                    self.say("Nothing played");
                    continue;
                }
                let id = self.fresh_id();
                let mut clip = Clip::midi(id, t.beats / self.tl.qpb, notes);
                clip.name = format!("Take {}", t.scene + 1);
                self.cells.insert((guid, t.scene), clip.clone());
                self.touch();
                self.say(format!("Captured into {}", clip.name));
                // Le clip tourne, en phase avec la fin de la prise.
                if !t.cut {
                    let tb = self.tl.beat(t.end_f);
                    self.launch_at(e, i, t.scene, clip, tb, t.end_f);
                } else {
                    let at = now + lat;
                    let tb = self.tl.beat(at);
                    self.launch_at(e, i, t.scene, clip, tb, at);
                }
            } else {
                match e.audio_rec_poll(t.id) {
                    RecPoll::Recording => continue,
                    RecPoll::Failed => {
                        self.cols[i].run.rec = None;
                        self.say("The take failed");
                    }
                    RecPoll::Ready(src) => {
                        self.cols[i].run.rec = None;
                        let name = format!("{} take {}", self.cols[i].track.name, t.scene + 1);
                        let Some(path) = e.save_take(src.clip, &name) else {
                            self.say("The take could not be written");
                            continue;
                        };
                        let id = self.fresh_id();
                        let mut clip = Clip::audio(id, &path);
                        clip.bars = t.beats / self.tl.qpb;
                        if let Kind::Audio(a) = &mut clip.kind {
                            a.src_bpm = Some(self.bpm);
                            a.looped = true;
                        }
                        self.cells.insert((guid, t.scene), clip.clone());
                        self.touch();
                        self.say(format!("Captured into {}", clip.name));
                        let at = now + lat;
                        let tb = self.tl.beat(at);
                        // Entre la ou en serait la boucle lancee a la fin de
                        // la prise.
                        let late = (at - t.end_f).max(0);
                        self.launch_at(e, i, t.scene, clip, tb, at - late);
                    }
                }
            }
        }
    }

    fn tick_dub(&mut self, e: &mut dyn Engine) {
        let now = e.now();
        for i in 0..self.cols.len() {
            let Some(mut d) = self.cols[i].run.dub.take() else {
                continue;
            };
            let Some(p) = self.cols[i].run.play.clone() else {
                // Le clip s'est arrete : les prises se ferment et se fondent.
                for (id, _, end) in &d.takes {
                    if *end > now {
                        e.midi_rec_stop(*id, now);
                    }
                }
                continue;
            };
            // La prise suivante s'enchaine a la fin de la passe.
            if !d.stopping {
                if let Some(&(_, _, end)) = d.takes.last() {
                    if end <= now + (0.25 * e.srate()) as i64 {
                        let Input::Midi(dev) = self.cols[i].track.input else {
                            continue;
                        };
                        let nend = next_pass_end(&p, self.tl.fpb, end + 1);
                        let id = e.midi_rec_start(dev, end);
                        if id != 0 {
                            e.midi_rec_stop(id, nend);
                            d.takes.push((id, end, nend));
                        }
                    }
                }
            }
            // Les prises finies rejoignent le clip.
            let mut k = 0;
            let mut merged = false;
            while k < d.takes.len() {
                let (id, start, end) = d.takes[k];
                if end > now {
                    k += 1;
                    continue;
                }
                let Some(evs) = e.midi_rec_take(id) else {
                    k += 1;
                    continue;
                };
                d.takes.remove(k);
                let fpb = self.tl.fpb;
                let span = p.span;
                let len = p.len;
                let rel: Vec<(i64, [u8; 3])> = evs;
                let mut add = Vec::new();
                for n in notes_of(&rel, fpb, f64::MAX) {
                    // Les noires jouees depuis le lancement du clip.
                    let u = (start - p.start_f) as f64 / fpb + n.start;
                    let pos = crate::clip_pos(span, len, p.from, u);
                    let max = span.1 - pos;
                    add.push(Note {
                        start: pos,
                        len: n.len.min(max.max(0.01)),
                        ..n
                    });
                }
                if !add.is_empty() {
                    if let Some(c) = self.clip_mut(i, p.scene) {
                        if let Kind::Midi(notes) = &mut c.kind {
                            notes.extend(add);
                            notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                            merged = true;
                        }
                    }
                }
            }
            if merged {
                let qpb = self.tl.qpb;
                if let Some(c) = self.clip(i, p.scene).cloned() {
                    e.lane_update(self.cols[i].port, &lane_clip(&c, qpb));
                }
                self.touch();
            }
            if !(d.stopping && d.takes.is_empty()) {
                self.cols[i].run.dub = Some(d);
            }
        }
    }

    fn tick_motion(&mut self, e: &mut dyn Engine) {
        let now = e.now();
        let fpb = self.tl.fpb;
        for i in 0..self.cols.len() {
            let m = self.cols[i].motion;
            if m == Motion::Stay {
                continue;
            }
            let run = &self.cols[i].run;
            let Some(p) = run.play.clone() else { continue };
            if run.next.is_some() || run.stop_at.is_some() || run.rec.is_some() {
                continue;
            }
            let end = next_pass_end(&p, fpb, now + 1);
            let span_beats = (p.span.1 - p.span.0).max(1e-9);
            let lead = (fpb * 1.0).min(span_beats * fpb * 0.5) as i64;
            if end - now > lead || p.motion_for == end {
                continue;
            }
            if let Some(pp) = self.cols[i].run.play.as_mut() {
                pp.motion_for = end;
            }
            let filled: Vec<usize> = (0..self.scenes)
                .filter(|&s| self.clip(i, s).is_some_and(|c| !c.is_empty()))
                .collect();
            let cur = p.scene;
            let after = filled.iter().copied().find(|&s| s > cur);
            let target = match m {
                Motion::Stay => None,
                Motion::OneShot => {
                    self.stop_at_frame(e, i, end);
                    continue;
                }
                Motion::MarchWrap => after.or(filled.first().copied()),
                Motion::MarchStay => after.or(Some(cur)),
                Motion::MarchStop => {
                    if after.is_none() {
                        self.stop_at_frame(e, i, end);
                        continue;
                    }
                    after
                }
                Motion::Random => {
                    if filled.is_empty() {
                        None
                    } else {
                        Some(filled[self.rand(filled.len())])
                    }
                }
                Motion::ExclusiveRandom => {
                    let others: Vec<usize> = filled.iter().copied().filter(|&s| s != cur).collect();
                    if others.is_empty() {
                        Some(cur)
                    } else {
                        Some(others[self.rand(others.len())])
                    }
                }
            };
            let Some(t) = target else { continue };
            if t == cur {
                continue;
            }
            if let Some(clip) = self.clip(i, t).cloned() {
                let tb = self.tl.beat(end);
                self.launch_at(e, i, t, clip, tb, end);
            }
        }
    }

    fn stop_at_frame(&mut self, e: &mut dyn Engine, col: usize, at: i64) {
        let c = &mut self.cols[col];
        if let Some(p) = &c.run.play {
            match p.voice {
                Some(v) => e.voice_stop(v, Some(at), 0.005),
                None => {
                    e.lane_stop(c.port, at);
                }
            }
            if let Some(pv) = p.audio.as_ref().and_then(|a| a.prev) {
                e.voice_stop(pv, Some(at), 0.005);
            }
            c.run.stop_at = Some(at);
        }
    }

    fn rand(&mut self, n: usize) -> usize {
        // xorshift : pas besoin de mieux pour une suite de lecture.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x % n.max(1) as u64) as usize
    }

    /// Quand le transport s'arrete (Follow) : tout se tait ; ce qui jouait
    /// est oublie (comme Ableton : relancer demande un geste).
    fn transport_stopped(&mut self, e: &mut dyn Engine) {
        for i in 0..self.cols.len() {
            let mut c = std::mem::take(&mut self.cols[i]);
            self.silence(e, &mut c);
            c.run.wait = None;
            self.cols[i] = c;
        }
    }

    /// Le transport part (Follow) : les cases et les prises qui attendaient
    /// se programment.
    fn start_waiting(&mut self, e: &mut dyn Engine) {
        for i in 0..self.cols.len() {
            if let Some(s) = self.cols[i].run.wait.take() {
                self.launch(e, i, s);
            }
            if let Some(s) = self.cols[i].run.rec_wait.take() {
                self.record(e, i, s);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Ce qu'une case montre
    // -----------------------------------------------------------------------

    pub fn state(&self, col: usize, scene: usize) -> CellState {
        let Some(c) = self.cols.get(col) else {
            return CellState::Empty;
        };
        let r = &c.run;
        if let Some(t) = &r.rec {
            if t.scene == scene {
                return if t.start_f > self.now_seen {
                    CellState::RecQueued
                } else {
                    CellState::Recording
                };
            }
        }
        if r.rec_wait == Some(scene) {
            return CellState::RecQueued;
        }
        if r.wait == Some(scene) {
            return CellState::Waiting;
        }
        if r.next.as_ref().is_some_and(|n| n.scene == scene) {
            return CellState::Queued;
        }
        if r.play.as_ref().is_some_and(|p| p.scene == scene) {
            if r.dub.is_some() {
                return CellState::Overdub;
            }
            return if r.stop_at.is_some() {
                CellState::Stopping
            } else {
                CellState::Playing
            };
        }
        if self.clip(col, scene).is_some() {
            CellState::Stopped
        } else {
            CellState::Empty
        }
    }

    /// Ou en est la passe de ce qui joue dans la colonne (0..1), et la
    /// position dans le clip.
    pub fn progress(&self, col: usize, now: i64) -> Option<(f64, f64)> {
        let p = self.cols.get(col)?.run.play.as_ref()?;
        if now < p.start_f {
            return None;
        }
        let u = (now - p.start_f) as f64 / self.tl.fpb;
        let pos = crate::clip_pos(p.span, p.len, p.from, u);
        let (a, b) = p.span;
        Some((((pos - a) / (b - a).max(1e-9)).clamp(0.0, 1.0), pos))
    }

    // -----------------------------------------------------------------------
    // Editer la grille
    // -----------------------------------------------------------------------

    /// Pose un clip dans une case (le remplace s'il y en avait un).
    pub fn put(&mut self, col: usize, scene: usize, clip: Clip) {
        let Some(c) = self.cols.get(col) else { return };
        let g = c.track.guid.clone();
        self.cells.insert((g, scene), clip);
        self.touch();
    }

    /// Un clip MIDI vide de la longueur par defaut (le clic sur une case
    /// vide l'ouvre dans l'editeur).
    pub fn new_midi(&mut self, col: usize, scene: usize, bars: f64) -> Option<u64> {
        self.cols.get(col)?;
        let id = self.fresh_id();
        let mut c = Clip::midi(id, bars, Vec::new());
        c.name = format!("Clip {}", scene + 1);
        self.put(col, scene, c);
        Some(id)
    }

    /// Efface une case (Ctrl+Z la rend). Ce qui y jouait se tait.
    pub fn erase(&mut self, e: &mut dyn Engine, col: usize, scene: usize) {
        let Some(c) = self.cols.get(col) else { return };
        let g = c.track.guid.clone();
        let playing = c.run.play.as_ref().is_some_and(|p| p.scene == scene)
            || c.run.next.as_ref().is_some_and(|p| p.scene == scene);
        if playing {
            let mut col_ = std::mem::take(&mut self.cols[col]);
            self.silence(e, &mut col_);
            self.cols[col] = col_;
        }
        if let Some(clip) = self.cells.remove(&(g.clone(), scene)) {
            self.erased = Some((g, scene, clip));
            self.touch();
        }
    }

    /// Rend la derniere case effacee (si sa place est libre).
    pub fn undo_erase(&mut self) -> bool {
        let Some((g, s, c)) = self.erased.take() else {
            return false;
        };
        if self.cells.contains_key(&(g.clone(), s)) {
            self.erased = Some((g, s, c));
            return false;
        }
        self.cells.insert((g, s), c);
        self.touch();
        true
    }

    /// Deplace (ou copie) une case. Une case pleine a l'arrivee : les deux
    /// s'echangent (une copie la remplace, Ctrl+Z ne la rend pas). Un clip
    /// deplace dans sa colonne continue de jouer ; dans une autre, il
    /// arrive arrete.
    pub fn move_cell(
        &mut self,
        e: &mut dyn Engine,
        from: (usize, usize),
        to: (usize, usize),
        copy: bool,
    ) {
        if from == to {
            return;
        }
        let (Some(a), Some(b)) = (self.cols.get(from.0), self.cols.get(to.0)) else {
            return;
        };
        let (ga, gb) = (a.track.guid.clone(), b.track.guid.clone());
        let Some(src) = self.cells.get(&(ga.clone(), from.1)).cloned() else {
            return;
        };
        if copy {
            let mut c = src;
            c.id = self.fresh_id();
            if let Some(old) = self.cells.insert((gb, to.1), c) {
                self.erased = Some((self.cols[to.0].track.guid.clone(), to.1, old));
            }
            self.touch();
            return;
        }
        let dst = self.cells.remove(&(gb.clone(), to.1));
        self.cells.remove(&(ga.clone(), from.1));
        self.cells.insert((gb.clone(), to.1), src);
        if let Some(d) = dst {
            self.cells.insert((ga.clone(), from.1), d);
        }
        // Ce qui joue suit sa case dans sa colonne ; ailleurs, il se tait.
        for (col, s_from, s_to) in [(from.0, from.1, to.1), (to.0, to.1, from.1)] {
            let same = from.0 == to.0;
            let run = &mut self.cols[col].run;
            for p in [run.play.as_mut(), run.next.as_mut()].into_iter().flatten() {
                if p.scene == s_from {
                    if same {
                        p.scene = s_to;
                    } else {
                        p.scene = usize::MAX;
                    }
                }
            }
            if same {
                break;
            }
        }
        for col in [from.0, to.0] {
            let gone = self.cols[col]
                .run
                .play
                .as_ref()
                .is_some_and(|p| p.scene == usize::MAX);
            if gone {
                let mut c = std::mem::take(&mut self.cols[col]);
                self.silence(e, &mut c);
                self.cols[col] = c;
            }
        }
        self.touch();
    }

    /// Le clip d'une case a change (l'editeur, un menu) : s'il joue, la lane
    /// prend ses nouvelles notes sans relance.
    pub fn clip_changed(&mut self, e: &mut dyn Engine, col: usize, scene: usize) {
        self.touch();
        let Some(c) = self.cols.get(col) else { return };
        let Some(p) = &c.run.play else { return };
        if p.scene != scene || p.audio.is_some() {
            return;
        }
        let port = c.port;
        if let Some(clip) = self.clip(col, scene).cloned() {
            let qpb = self.tl.qpb;
            e.lane_update(port, &lane_clip(&clip, qpb));
            if let Some(pp) = self.cols[col].run.play.as_mut() {
                pp.span = clip.span(qpb);
                pp.len = clip.len_beats(qpb);
            }
        }
    }

    /// Capture la scene : les clips qui jouent (pas ceux qu'on enregistre)
    /// sont copies dans la premiere rangee entierement vide sous la case
    /// choisie (en revenant en haut). Rien n'est ecrase ; rien ne se
    /// relance.
    pub fn capture_scene(&mut self) -> Option<usize> {
        let playing: Vec<(usize, usize)> = self
            .cols
            .iter()
            .enumerate()
            .filter(|(_, c)| c.run.rec.is_none())
            .filter_map(|(i, c)| c.run.play.as_ref().map(|p| (i, p.scene)))
            .filter(|(_, s)| *s != usize::MAX)
            .collect();
        if playing.is_empty() {
            self.say("Nothing plays");
            return None;
        }
        let n = self.scenes + 1;
        let row = (1..=n)
            .map(|k| (self.sel.1 + k) % n)
            .find(|&s| (0..self.cols.len()).all(|c| self.clip(c, s).is_none()))?;
        for (c, s) in playing {
            if let Some(mut clip) = self.clip(c, s).cloned() {
                clip.id = self.fresh_id();
                self.put(c, row, clip);
            }
        }
        self.say(format!("Scene captured into row {}", row + 1));
        Some(row)
    }
}

// ---------------------------------------------------------------------------
// Les calculs
// ---------------------------------------------------------------------------

/// Le clip tel que la lane le joue (les notes de probabilite sont jouees
/// toujours : la lane ne tire pas encore au sort).
pub fn lane_clip(c: &Clip, qpb: f64) -> LaneClip {
    let len = c.len_beats(qpb);
    LaneClip {
        id: c.id,
        notes: c
            .notes()
            .iter()
            .filter(|n| n.start < len)
            .map(|n| (n.start, n.len, n.chan, n.pitch, n.vel))
            .collect(),
        len,
        brace: c.span(qpb),
        looped: true,
    }
}

/// La region d'une case audio, en secondes.
fn region_seconds(a: &crate::clip::Audio, src: &AudioSrc) -> f64 {
    let total = src.frames as f64 / src.srate.max(1.0);
    let len = if a.len > 0.0 { a.len } else { total - a.offs };
    len.clamp(0.0, (total - a.offs).max(0.0))
}

/// Le tempo d'une source, et d'ou il vient.
pub fn source_tempo(c: &Clip, src: &AudioSrc, qpb: f64, project: f64) -> (Option<f64>, TempoWhy) {
    let Kind::Audio(a) = &c.kind else {
        return (None, TempoWhy::Unknown);
    };
    if let Some(b) = a.src_bpm {
        return (Some(b), TempoWhy::Set);
    }
    if let Some(b) = src.file_bpm {
        return (Some(b), TempoWhy::File);
    }
    let name = std::path::Path::new(&a.path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let secs = region_seconds(a, src);
    if let Some(b) = tempo_from_name(&name) {
        // « Assez long pour ETRE une boucle a ce tempo » : un kick de 0,3 s
        // nomme 128 n'est pas repitche.
        if secs * b / 60.0 >= 0.9 {
            return (Some(b), TempoWhy::Name);
        }
    }
    if let Some(b) = tempo_from_length(secs, qpb, project) {
        return (Some(b), TempoWhy::Guess);
    }
    (None, TempoWhy::Unknown)
}

/// Le taux qui fait remplir EXACTEMENT la passe a la region, si elle en est
/// a 2 % pres (au-dela, c'est une queue, pas une erreur de longueur).
fn rate_for_pass(a: &AudioPlay, pass_s: f64) -> Option<f64> {
    let region_s = (a.end - a.start) as f64 / a.src.srate.max(1.0);
    if region_s <= 0.0 || pass_s <= 0.0 {
        return None;
    }
    let played = region_s / a.rate;
    if ((played - pass_s) / pass_s).abs() <= 0.02 {
        Some(region_s / pass_s)
    } else {
        None
    }
}

/// La fin de la passe en cours (ou de la suivante) apres le frame `after`.
fn next_pass_end(p: &Play, fpb: f64, after: i64) -> i64 {
    let first = (p.span.1 - p.from).max(0.0);
    let span = (p.span.1 - p.span.0).max(1e-9);
    let first_end = p.start_f + (first * fpb).round() as i64;
    if after <= first_end {
        return first_end;
    }
    let k = ((after - first_end) as f64 / (span * fpb)).ceil();
    first_end + (k * span * fpb).round() as i64
}

/// Les notes d'une prise (frames depuis son debut) : un debut et sa fin
/// font une note ; une note encore tenue finit a `beats`. Le canal 16 est
/// celui des apercus (jamais pris).
pub fn notes_of(evs: &[(i64, [u8; 3])], fpb: f64, beats: f64) -> Vec<Note> {
    let mut open: HashMap<(u8, u8), (f64, u8)> = HashMap::new();
    let mut out = Vec::new();
    let mut last = 0.0f64;
    for &(f, m) in evs {
        let st = m[0] & 0xF0;
        let ch = m[0] & 0x0F;
        if ch == 15 {
            continue;
        }
        let b = f as f64 / fpb;
        last = last.max(b);
        let on = st == 0x90 && m[2] > 0;
        let off = st == 0x80 || (st == 0x90 && m[2] == 0);
        if on {
            // Une hauteur rejouee ferme la precedente.
            if let Some((s, v)) = open.remove(&(ch, m[1])) {
                out.push(note(s, b, m[1], v, ch));
            }
            open.insert((ch, m[1]), (b, m[2]));
        } else if off {
            if let Some((s, v)) = open.remove(&(ch, m[1])) {
                out.push(note(s, b, m[1], v, ch));
            }
        }
    }
    let end = if beats.is_finite() { beats } else { last };
    for ((ch, p), (s, v)) in open {
        out.push(note(s, end.max(s + 0.05), p, v, ch));
    }
    out.retain(|n| n.start < end.max(1e-9) || !beats.is_finite());
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    out
}

fn note(s: f64, e: f64, pitch: u8, vel: u8, chan: u8) -> Note {
    let len = e - s;
    Note {
        start: s.max(0.0),
        len: if len < 0.02 { 0.05 } else { len },
        pitch,
        vel: vel.max(1),
        chan,
        prob: 100,
    }
}
