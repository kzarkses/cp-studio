//! Le moteur audio DANS REAPER.
//!
//! `cp_engine` ne connait ni REAPER ni Windows. Ce crate le branche :
//! - chaque port (colonne) a son APERCU PERMANENT : une `PortSource` tiree par
//!   REAPER vers une sortie materielle, jamais arretee, qui rend du silence quand
//!   rien ne joue. On ne cree ni ne detruit rien au lancement d'un son ;
//! - le CROCHET AUDIO fait avancer l'horloge au passage « post », apres le rendu
//!   du bloc, et donne au moteur le taux du peripherique au passage « pre » ;
//! - un FIL DE TRAVAIL decode les fichiers (par REAPER, tous ses formats) et
//!   calcule leurs pics ; le fil principal range le resultat dans le vivier.
//!
//! [`AudioHost`] vit sur le fil principal. Il vit dans la petite DLL fixe : une
//! retouche de l'interface (le module recharge a chaud) ne coupe jamais le son.

pub mod capture;
pub mod decode;
pub mod feed;
pub mod midirec;
pub mod peaks;
pub mod record;
pub mod stream;
pub mod stretch;
pub mod testloop;

use std::sync::{Arc, OnceLock};

use cp_engine::types::{MAX_CHANS, MAX_PORTS, state};
use cp_engine::{At, ClipData, Core, Ctl, Param, PlayMode, Pool, VoiceOp};
use cp_sys::audio::{AudioHook, Preview};
use cp_sys::pcm::PortSource;
use cp_sys::reaper::Reaper;

pub use cp_engine::VoiceH;
use decode::{Decoder, Job, Reader};
use feed::{FeedStats, PortFeed};
use peaks::Peaks;

/// Un clip, tel que le module le designe : 16 bits de generation, 16 bits
/// d'emplacement. Un emplacement du vivier est reutilise apres liberation ; la
/// generation fait qu'un handle perime ne joue jamais un autre son.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipH(pub u32);

impl ClipH {
    fn new(id: i32, generation: u16) -> ClipH {
        ClipH((generation as u32) << 16 | (id as u32 & 0xFFFF))
    }
    fn id(self) -> i32 {
        (self.0 & 0xFFFF) as i32
    }
    fn generation(self) -> u16 {
        (self.0 >> 16) as u16
    }
}

/// Ou en est un chargement.
#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Pending,
    Ready(ClipH),
    Failed(String),
}

struct ClipEntry {
    generation: u16,
    /// Les pics exacts, une fois le clip complet. Avant : approximes a la
    /// volee sur ce qui est deja decode.
    peaks: Option<Arc<Peaks>>,
}

/// Une vignette de forme d'onde (vue « samples ») : min et max de tranches.
#[derive(Clone, Debug)]
pub enum Thumb {
    Pending,
    Ready(Arc<Vec<[f32; 2]>>),
    Failed,
}

/// Les vignettes : 200 tranches, pour les fichiers de 10 minutes au plus.
pub const THUMB_POINTS: usize = 200;
const THUMB_MAX_SECONDS: f64 = 600.0;
const THUMBS_KEPT: usize = 4000;

/// Une matiere decodee en entier ne depasse pas 1,5 Go (23 min en stereo a
/// 48 kHz) : seule une source sans acces au hasard (une cuisson) y est tenue.
const MAX_FILE_BYTES: usize = 1_500_000_000;

/// Au-dela, un fichier se lit EN FLUX (A2), sans limite de longueur : 256 Mo
/// decodes, 11 min en stereo a 48 kHz.
const STREAM_ABOVE_BYTES: usize = 256 << 20;

/// Un ticket : en attente, pret, ou en echec.
#[derive(Clone, Debug)]
enum Ticket {
    Pending(ClipH),
    Ready(ClipH),
    Failed(String),
}

struct PortOut {
    preview: Option<Preview>,
    stats: Arc<FeedStats>,
    feed_for_tests: Option<PortSource>,
}

/// Ce que le statut affiche.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub srate: f64,
    pub blocks: i64,
    pub last_block: i32,
    pub hook: bool,
    pub ports_open: usize,
    pub calls: u64,
    pub rewinds: u32,
    pub skips: u32,
    pub over_pulls: u32,
    pub under_pulls: u32,
    pub busy_renders: u32,
    pub dropped_commands: u32,
    /// Les clips lus en flux, et les blocs qui ont lu une page absente.
    pub streams: usize,
    pub stream_misses: u64,
    pub busy_ns: u64,
    /// Le plus long temps de tirage d'un port, en frames.
    pub frames_pulled: u64,
    pub active_voices: usize,
    pub clips: usize,
    pub bytes: usize,
    pub decoding: usize,
    /// Ports revenus a la sortie materielle (leur piste avait disparu).
    pub routes_lost: u32,
}

impl Stats {
    /// La part du fil audio passee dans nos rendus, en pour cent, sur le temps
    /// ou les ports ont ete tires.
    pub fn load_percent(&self) -> f64 {
        let wall = self.frames_pulled as f64 / self.srate.max(1.0);
        if wall <= 0.0 {
            0.0
        } else {
            self.busy_ns as f64 / 1e9 / wall * 100.0
        }
    }
}

static HOOK_CORE: OnceLock<Arc<Core>> = OnceLock::new();

/// La clock MIDI externe : son calcul (fil audio, sous `try_lock`), le
/// peripherique ecoute (-1 : aucun) et l'acces aux entrees MIDI.
static CLOCK: OnceLock<std::sync::Mutex<cp_engine::extclock::ExtClock>> = OnceLock::new();
static CLOCK_DEV: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static MIDI_IN: OnceLock<cp_sys::midi_in::MidiInputs> = OnceLock::new();
/// Les notes jouees sur les entrees MIDI (la saisie pas a pas de
/// l'editeur) : ecoutees si `NOTES_N` > 0 (le nombre d'entrees), posees par
/// le crochet dans un anneau que le fil principal vide.
static NOTES_N: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
const NOTES_CAP: usize = 1024;
static NOTES: [std::sync::atomic::AtomicU32; NOTES_CAP] =
    [const { std::sync::atomic::AtomicU32::new(0) }; NOTES_CAP];
static NOTES_W: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Fil audio : les notes (et leurs relachements) des entrees MIDI.
fn notes_block(m: &cp_sys::midi_in::MidiInputs, n: i32) {
    use std::sync::atomic::Ordering::{Relaxed, Release};
    let mut w = NOTES_W.load(Relaxed);
    for dev in 0..n.min(64) {
        m.for_each(dev, |_, msg| {
            if msg.len() >= 3 && matches!(msg[0] & 0xF0, 0x80 | 0x90) {
                let v = (msg[0] as u32)
                    | (msg[1] as u32) << 8
                    | (msg[2] as u32) << 16
                    | (dev as u32) << 24;
                NOTES[w % NOTES_CAP].store(v, Relaxed);
                w = w.wrapping_add(1);
            }
        });
    }
    NOTES_W.store(w, Release);
}

/// L'anneau de l'enregistrement, rempli par le crochet (pose a la premiere
/// prise).
static REC_RING: OnceLock<Arc<record::RecRing>> = OnceLock::new();

/// L'anneau de la prise MIDI, rempli par le crochet (pose a la premiere
/// prise).
static MIDI_RING: OnceLock<Arc<midirec::MidiRing>> = OnceLock::new();

/// Fil audio : les messages courts des `n` premieres entrees MIDI, dates au
/// frame du moteur.
fn midirec_block(
    m: &cp_sys::midi_in::MidiInputs,
    ring: &midirec::MidiRing,
    n: i32,
    base: i64,
    srate: f64,
) {
    for dev in 0..n.min(64) {
        m.for_each(dev, |off, msg| {
            let f = base + (off as f64 * srate / 1_024_000.0).round() as i64;
            ring.push(f, dev as u8, msg);
        });
    }
}

/// Sa publication, lue par le fil principal sans jamais prendre le verrou
/// du fil audio.
static CLOCK_PUB: OnceLock<Arc<cp_engine::extclock::Published>> = OnceLock::new();

fn on_audio(is_post: bool, frames: i32, srate: f64) {
    if !is_post {
        capture::on_block(frames);
    }
    let Some(core) = HOOK_CORE.get() else { return };
    if is_post {
        core.tick(frames);
    } else {
        core.set_srate(srate);
        // L'enregistrement : l'entree choisie, datee au frame du bloc.
        if let Some(ring) = REC_RING.get() {
            let (c0, c1) = ring.inputs();
            if c0 >= 0 {
                if let Some(l) = AudioHook::input(c0, frames) {
                    ring.push_block(core.clock_now(), l, AudioHook::input(c1, frames));
                }
            }
        }
        if let Some(mr) = MIDI_RING.get() {
            let n = mr.inputs();
            if n > 0 {
                if let Some(m) = MIDI_IN.get() {
                    midirec_block(m, mr, n, core.clock_now(), srate);
                }
            }
            mr.advance(core.clock_now() + frames as i64);
        }
        let nin = NOTES_N.load(std::sync::atomic::Ordering::Relaxed);
        if nin > 0 {
            if let Some(m) = MIDI_IN.get() {
                notes_block(m, nin);
            }
        }
        let dev = CLOCK_DEV.load(std::sync::atomic::Ordering::Relaxed);
        if dev >= 0 {
            if let (Some(m), Some(c)) = (MIDI_IN.get(), CLOCK.get()) {
                clock_block(m, c, dev, core.clock_now() as f64, frames, srate);
            }
        }
    }
}

/// Un bloc de la clock externe : les messages temps reel de l'entree,
/// dates en echantillons du moteur, puis la publication.
fn clock_block(
    m: &cp_sys::midi_in::MidiInputs,
    c: &std::sync::Mutex<cp_engine::extclock::ExtClock>,
    dev: i32,
    base: f64,
    frames: i32,
    srate: f64,
) {
    let Ok(mut ck) = c.try_lock() else { return };
    ck.set_srate(srate);
    m.for_each(dev, |off, msg| {
        let t = base + off as f64 * srate / 1_024_000.0;
        match msg.first().copied() {
            Some(s @ (0xF8 | 0xFA | 0xFB | 0xFC)) => ck.realtime(s, t),
            Some(0xF2) if msg.len() >= 3 => {
                ck.spp((msg[1] as i32 & 0x7F) | ((msg[2] as i32 & 0x7F) << 7))
            }
            _ => {}
        }
    });
    ck.end_block(base + frames as f64);
}

pub struct AudioHost {
    reaper: Option<Reaper>,
    ctl: Ctl,
    pool: Pool,
    clips: Vec<Option<ClipEntry>>,
    clip_gens: Vec<u16>,
    ports: Vec<PortOut>,
    decoder: Decoder,
    /// Le fil de flux (au premier long fichier), et les voix qui jouent un
    /// clip en flux (un saut le previent).
    streamer: Option<stream::Streamer>,
    streamed: Vec<(VoiceH, Arc<cp_sys::paged::Paged>)>,
    stream_above: usize,
    tickets: Vec<(u32, Ticket)>,
    /// Les decodages en cours : ticket -> clip (pour y poser les pics).
    in_flight: Vec<(u32, ClipH)>,
    thumbs: std::collections::HashMap<String, Thumb>,
    next_ticket: u32,
    decoding: usize,
    /// Les voix lancees par `play`, rendues des qu'elles ont fini de sonner.
    voices: Vec<VoiceH>,
    hook: bool,
    max_frames: usize,
    /// Le taux annonce a REAPER quand les apercus ont ete lances. REAPER le lit
    /// au lancement : s'il change (peripherique change, ou taux inconnu au
    /// demarrage), on relance les apercus, sinon REAPER reechantillonnerait.
    preview_srate: f64,
    /// Ports revenus a la sortie materielle : leur piste avait disparu.
    routes_lost: u32,
    /// Les etireurs temps reel : le vivier (le dernier detenteur de chacun,
    /// le fil audio ne libere jamais rien), et a quelle voix chacun est
    /// prete.
    shifters: Vec<cp_engine::shift::ShiftSlot>,
    lent: Vec<(VoiceH, cp_engine::shift::ShiftSlot)>,
    /// Fabrique un etireur hors de REAPER (les tests).
    shift_factory: Option<Box<dyn Fn() -> Box<dyn cp_engine::shift::RtShift> + Send>>,
    /// L'enregistrement (cree a la premiere prise), et les prises finies.
    rec: Option<record::Recorder>,
    rec_done: Vec<(u32, Result<ClipH, String>)>,
    /// Les notes MIDI deja lues.
    notes_r: usize,
    /// La prise MIDI (creee a la premiere), et les prises finies.
    mrec: Option<midirec::MidiRecorder>,
    mrec_done: Vec<(u32, Vec<midirec::MidiEv>)>,
}

/// Ou en est une prise.
#[derive(Clone, Debug, PartialEq)]
pub enum RecState {
    /// En cours : les frames deja pris.
    Recording(usize),
    Ready(ClipH),
    Failed(String),
    Unknown,
}

/// Au plus tant d'etireurs temps reel (elastique coute 2 a 3 % du fil audio
/// par voix).
const MAX_SHIFTERS: usize = 16;

impl AudioHost {
    /// Dans REAPER : `Some(reaper)`, le crochet est enregistre. En test : `None`,
    /// et le test tire les ports lui-meme ([`AudioHost::pull`]).
    pub fn new(reaper: Option<Reaper>, srate: f64) -> AudioHost {
        let ctl = Ctl::new(if srate > 1.0 { srate } else { 48000.0 });
        let mut hook = false;
        let ck = cp_engine::extclock::ExtClock::new();
        let _ = CLOCK_PUB.set(ck.publication());
        let _ = CLOCK.set(std::sync::Mutex::new(ck));
        if let Some(r) = &reaper {
            let _ = MIDI_IN.set(cp_sys::midi_in::MidiInputs::new(r));
            if HOOK_CORE.set(ctl.core().clone()).is_ok() {
                hook = AudioHook::register(r, on_audio);
            }
            if hook {
                ctl.core().use_external_clock();
            }
        }
        AudioHost {
            reaper,
            ctl,
            pool: Pool::new(),
            clips: (0..cp_engine::types::MAX_CLIPS).map(|_| None).collect(),
            clip_gens: vec![0; cp_engine::types::MAX_CLIPS],
            ports: (0..MAX_PORTS)
                .map(|_| PortOut {
                    preview: None,
                    stats: Arc::new(FeedStats::default()),
                    feed_for_tests: None,
                })
                .collect(),
            decoder: Decoder::new(),
            streamer: None,
            streamed: Vec::new(),
            stream_above: STREAM_ABOVE_BYTES,
            tickets: Vec::new(),
            in_flight: Vec::new(),
            thumbs: std::collections::HashMap::new(),
            next_ticket: 1,
            decoding: 0,
            voices: Vec::new(),
            hook,
            max_frames: usize::MAX,
            preview_srate: 0.0,
            routes_lost: 0,
            shifters: Vec::new(),
            lent: Vec::new(),
            shift_factory: None,
            rec: None,
            rec_done: Vec::new(),
            notes_r: 0,
            mrec: None,
            mrec_done: Vec::new(),
        }
    }

    pub fn core(&self) -> &Arc<Core> {
        self.ctl.core()
    }

    /// Arrete les apercus et retire le crochet (fermeture de REAPER).
    pub fn shutdown(&mut self) {
        for p in &mut self.ports {
            if let Some(pv) = p.preview.as_mut() {
                pv.stop();
            }
        }
        if let (Some(r), true) = (&self.reaper, self.hook) {
            AudioHook::unregister(r);
            self.hook = false;
        }
    }

    /// Ouvre le port : son apercu permanent dans REAPER (sortie materielle 1/2).
    /// Sans REAPER, une source que le test tire.
    fn ensure_port(&mut self, port: usize) -> bool {
        if port >= MAX_PORTS {
            return false;
        }
        let core = self.ctl.core().clone();
        let core_srate = core.srate();
        let p = &mut self.ports[port];
        let stats = p.stats.clone();
        let feed = move || PortSource::new(Box::new(PortFeed { core, port, stats }));
        match &self.reaper {
            Some(r) => {
                if p.preview.is_none() {
                    let src = feed();
                    p.preview = Some(Preview::new(r, src));
                }
                // Un apercu qui joue deja (sur la sortie, ou DANS une piste)
                // reste ou il est.
                let ok = p
                    .preview
                    .as_mut()
                    .is_some_and(|pv| pv.is_playing() || pv.play_hardware(0));
                if ok {
                    self.preview_srate = core_srate;
                }
                ok
            }
            None => {
                if p.feed_for_tests.is_none() {
                    p.feed_for_tests = Some(feed());
                }
                true
            }
        }
    }

    /// Fait jouer un port DANS une piste (son GUID) : la colonne passe par
    /// ses effets, son volume, ses envois. Rien : la sortie materielle 1/2.
    pub fn route_port(&mut self, port: usize, track: Option<&str>) -> bool {
        if !self.ensure_port(port) {
            return false;
        }
        let Some(r) = self.reaper else {
            return track.is_none();
        };
        let Some(pv) = self.ports[port].preview.as_mut() else {
            return false;
        };
        match track {
            Some(g) => pv.play_track(&r, g),
            None => pv.play_hardware(0),
        }
    }

    /// La piste ou joue un port (son GUID).
    pub fn port_route(&self, port: usize) -> Option<String> {
        self.ports
            .get(port)?
            .preview
            .as_ref()?
            .routed()
            .map(str::to_string)
    }

    /// Les cretes d'un port (gauche, droite) depuis la derniere lecture, pour
    /// les vumetres.
    pub fn port_levels(&self, port: usize) -> [f32; 2] {
        self.ports
            .get(port)
            .map_or([0.0; 2], |p| p.stats.take_peaks())
    }

    /// Une prise audio : les entrees `ch0` et `ch1` de la carte (-1 : mono),
    /// a partir du temps `start` du moteur (rien : maintenant), la latence
    /// d'entree et de sortie retiree. Refusee si une autre prise en cours
    /// ecoute d'autres entrees.
    pub fn rec_start(&mut self, ch0: i32, ch1: i32, start: Option<i64>) -> Option<u32> {
        let lat = self.reaper.map_or(0, |r| {
            // REAPER annonce 0 entree au crochet : le vrai nombre, relu.
            AudioHook::refresh_inputs(&r);
            let (i, o) = AudioHook::io_latency(&r);
            (i + o) as i64
        });
        self.rec_start_lat(ch0, ch1, start, lat)
    }

    /// [`Self::rec_start`] avec une latence donnee (les tests).
    pub fn rec_start_lat(
        &mut self,
        ch0: i32,
        ch1: i32,
        start: Option<i64>,
        lat: i64,
    ) -> Option<u32> {
        if ch0 < 0 {
            return None;
        }
        let rec = self.rec.get_or_insert_with(|| {
            let ring = REC_RING.get_or_init(record::RecRing::new).clone();
            record::Recorder::new(ring)
        });
        let ring = rec.ring().clone();
        if rec.active() > 0 && ring.inputs() != (ch0, ch1) {
            return None;
        }
        ring.set_inputs(ch0, ch1);
        let at = start.unwrap_or_else(|| self.ctl.core().clock_now());
        Some(rec.start(at, lat))
    }

    /// Une prise MIDI sur l'entree `dev` (-1 : toutes), qui commence au
    /// frame `start` du moteur (rien : maintenant) ; la latence de sortie est
    /// retiree (on joue ce qu'on entend).
    pub fn midi_rec_start(&mut self, dev: i32, start: Option<i64>) -> u32 {
        let lat = self
            .reaper
            .map_or(0, |r| AudioHook::io_latency(&r).1 as i64);
        self.midi_rec_start_lat(dev, start, lat)
    }

    /// [`Self::midi_rec_start`] avec une latence donnee (les tests).
    pub fn midi_rec_start_lat(&mut self, dev: i32, start: Option<i64>, lat: i64) -> u32 {
        let n = self.reaper.map_or(1, |r| {
            r.midi_inputs().iter().map(|x| x.0 + 1).max().unwrap_or(0)
        });
        let rec = self.mrec.get_or_insert_with(|| {
            midirec::MidiRecorder::new(MIDI_RING.get_or_init(midirec::MidiRing::new).clone())
        });
        rec.ring().set_inputs(n.max(1));
        let at = start.unwrap_or_else(|| self.ctl.core().clock_now());
        rec.start(at, lat, dev)
    }

    /// La prise MIDI s'arrete au temps `at` du moteur (rien : maintenant).
    pub fn midi_rec_stop(&mut self, id: u32, at: Option<i64>) {
        let at = at.unwrap_or_else(|| self.ctl.core().clock_now());
        if let Some(r) = self.mrec.as_mut() {
            r.stop(id, at);
        }
    }

    /// Les messages d'une prise MIDI finie (donnes une fois), dates depuis
    /// son debut ; `None` tant qu'elle court.
    pub fn midi_rec_take(&mut self, id: u32) -> Option<Vec<midirec::MidiEv>> {
        let i = self.mrec_done.iter().position(|(k, _)| *k == id)?;
        Some(self.mrec_done.swap_remove(i).1)
    }

    /// [`Self::midi_rec_take`] seulement si la prise tient dans `cap`
    /// messages : `Err(n)` sinon (rien n'est oublie).
    pub fn midi_rec_take_if(
        &mut self,
        id: u32,
        cap: usize,
    ) -> Result<Option<Vec<midirec::MidiEv>>, usize> {
        match self.mrec_done.iter().find(|(k, _)| *k == id) {
            None => Ok(None),
            Some((_, v)) if v.len() > cap => Err(v.len()),
            Some(_) => Ok(self.midi_rec_take(id)),
        }
    }

    /// Les messages deja pris d'une prise MIDI en cours, et les messages
    /// perdus depuis le depart (anneau deborde).
    pub fn midi_rec_count(&self, id: u32) -> (Option<usize>, usize) {
        self.mrec
            .as_ref()
            .map_or((None, 0), |r| (r.count(id), r.lost))
    }

    /// Le battement de la prise MIDI : l'anneau vide dans les prises ; plus
    /// aucune prise, le crochet cesse d'ecouter.
    fn midi_rec_poll(&mut self) {
        if let Some(r) = self.mrec.as_mut() {
            let done = r.drain();
            self.mrec_done.extend(done);
            if r.active() == 0 {
                r.ring().set_inputs(0);
            }
        }
    }

    /// La prise s'arrete au temps `at` du moteur (rien : maintenant).
    pub fn rec_stop(&mut self, id: u32, at: Option<i64>) {
        let at = at.unwrap_or_else(|| self.ctl.core().clock_now());
        if let Some(r) = self.rec.as_mut() {
            r.stop(id, at);
        }
    }

    pub fn rec_state(&self, id: u32) -> RecState {
        if let Some((_, r)) = self.rec_done.iter().find(|(i, _)| *i == id) {
            return match r {
                Ok(c) => RecState::Ready(*c),
                Err(e) => RecState::Failed(e.clone()),
            };
        }
        match self.rec.as_ref().and_then(|r| r.frames(id)) {
            Some(n) => RecState::Recording(n),
            None => RecState::Unknown,
        }
    }

    /// Les tests : l'anneau de l'enregistrement, a remplir a la main.
    pub fn rec_ring(&self) -> Option<Arc<record::RecRing>> {
        self.rec.as_ref().map(|r| r.ring().clone())
    }

    /// Vide l'anneau dans les prises ; une prise finie devient un clip.
    fn rec_poll(&mut self) {
        let Some(rec) = self.rec.as_mut() else { return };
        let done = rec.drain();
        let idle = rec.active() == 0;
        let ring = rec.ring().clone();
        let sr = self.ctl.core().srate();
        for (id, take) in done {
            let r = match take {
                record::Take::Done(d) if d.is_empty() => Err("empty take".to_string()),
                record::Take::Done(d) => self.load_samples(d, 2, sr),
                record::Take::Failed(e) => Err(e),
                record::Take::Recording(_) => continue,
            };
            self.rec_done.push((id, r));
        }
        // Plus rien a prendre : l'entree n'est plus copiee.
        if idle {
            ring.set_inputs(-1, -1);
        }
        if self.rec_done.len() > 64 {
            self.rec_done.drain(..self.rec_done.len() - 64);
        }
    }

    /// Ecouter (ou plus) les notes jouees sur toutes les entrees MIDI : la
    /// saisie pas a pas de l'editeur.
    pub fn midi_listen(&mut self, on: bool) {
        let n = if on {
            self.reaper.map_or(0, |r| {
                r.midi_inputs().iter().map(|x| x.0 + 1).max().unwrap_or(0)
            })
        } else {
            0
        };
        self.notes_r = NOTES_W.load(std::sync::atomic::Ordering::Acquire);
        NOTES_N.store(n, std::sync::atomic::Ordering::Relaxed);
    }

    /// Les notes arrivees depuis le dernier appel : (statut, note, velocite)
    /// dans les octets bas, l'entree dans le haut.
    pub fn midi_notes(&mut self) -> Vec<u32> {
        let w = NOTES_W.load(std::sync::atomic::Ordering::Acquire);
        let from = if w.wrapping_sub(self.notes_r) > NOTES_CAP {
            w - NOTES_CAP
        } else {
            self.notes_r
        };
        let out = (from..w)
            .map(|i| NOTES[i % NOTES_CAP].load(std::sync::atomic::Ordering::Relaxed))
            .collect();
        self.notes_r = w;
        out
    }

    /// La clock MIDI externe : ecouter l'entree MIDI `dev` (son rang dans
    /// les entrees de REAPER), ou rien (-1). Repart de zero.
    pub fn set_clock_device(&mut self, dev: i32) {
        if let Some(c) = CLOCK.get() {
            if let Ok(mut ck) = c.lock() {
                ck.reset();
            }
        }
        CLOCK_DEV.store(dev, std::sync::atomic::Ordering::Relaxed);
    }

    /// L'entree MIDI ecoutee (-1 : aucune).
    pub fn clock_device(&self) -> i32 {
        CLOCK_DEV.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// L'etat de la clock externe et la position du maitre en noires
    /// maintenant (au bloc en cours).
    pub fn clock_read(&self) -> Option<(cp_engine::extclock::Snap, f64)> {
        let s = CLOCK_PUB.get()?.read()?;
        let now = self.ctl.core().clock_now() as f64;
        Some((s, cp_engine::extclock::qn_at(&s, now)))
    }

    /// Une piste de sortie effacee (ou deplacee dans un autre projet) : le
    /// port revient a la sortie materielle. Rend le nombre de ports rendus.
    fn check_routes(&mut self) -> usize {
        let Some(r) = self.reaper else {
            return 0;
        };
        let mut k = 0;
        for p in &mut self.ports {
            if let Some(pv) = p.preview.as_mut() {
                if !pv.track_alive(&r) {
                    pv.play_hardware(0);
                    k += 1;
                }
            }
        }
        self.routes_lost += k as u32;
        k
    }

    fn port_source(&self, port: usize) -> Option<&PortSource> {
        let p = self.ports.get(port)?;
        p.preview
            .as_ref()
            .map(|pv| pv.source())
            .or(p.feed_for_tests.as_ref())
    }

    /// Tire un bloc d'un port par sa table virtuelle, comme REAPER (tests).
    /// Rend le nombre de frames rendus.
    pub fn pull(&mut self, port: usize, time_s: f64, nch: usize, out: &mut [f64]) -> usize {
        self.ensure_port(port);
        let srate = self.ctl.core().srate();
        self.port_source(port)
            .map_or(0, |s| s.pull(time_s, srate, nch, out))
    }

    // -----------------------------------------------------------------------
    // Le vivier
    // -----------------------------------------------------------------------

    fn store(
        &mut self,
        samples: Vec<f32>,
        nch: usize,
        srate: f64,
        peaks: Peaks,
    ) -> Result<ClipH, String> {
        let id = self
            .pool
            .add(samples, nch, srate)
            .map_err(|e| format!("{e:?}"))?;
        let g = &mut self.clip_gens[id as usize];
        *g = match g.wrapping_add(1) {
            0 => 1,
            n => n,
        };
        let generation = *g;
        self.clips[id as usize] = Some(ClipEntry {
            generation,
            peaks: Some(Arc::new(peaks)),
        });
        Ok(ClipH::new(id, generation))
    }

    fn new_generation(&mut self, id: i32) -> u16 {
        let g = &mut self.clip_gens[id as usize];
        *g = match g.wrapping_add(1) {
            0 => 1,
            n => n,
        };
        *g
    }

    /// Range une matiere deja en memoire (le son de test, les tests).
    pub fn load_samples(
        &mut self,
        samples: Vec<f32>,
        nch: usize,
        srate: f64,
    ) -> Result<ClipH, String> {
        let peaks = Peaks::build(&samples, nch);
        self.store(samples, nch, srate, peaks)
    }

    fn clip(&self, h: ClipH) -> Option<(&Arc<ClipData>, &ClipEntry)> {
        let e = self.clips.get(usize::try_from(h.id()).ok()?)?.as_ref()?;
        if e.generation != h.generation() {
            return None;
        }
        Some((self.pool.get(h.id())?, e))
    }

    /// La matiere d'un clip vivant (entrelacee).
    pub fn clip_samples(&self, h: ClipH) -> Option<&[f32]> {
        self.clip(h).map(|(c, _)| c.samples())
    }

    /// (frames, canaux, taux) d'un clip vivant.
    pub fn clip_info(&self, h: ClipH) -> Option<(i64, usize, f64)> {
        self.clip(h).map(|(c, _)| (c.frames(), c.nch(), c.srate()))
    }

    /// Frames deja decodes d'un clip vivant (un clip en flux se joue
    /// partout : toutes).
    pub fn clip_decoded(&self, h: ClipH) -> Option<i64> {
        self.clip(h).map(|(c, _)| {
            if c.paged().is_some() {
                c.frames()
            } else {
                (c.samples().len() / c.nch().max(1)) as i64
            }
        })
    }

    /// Le clip est-il lu en flux ?
    pub fn clip_streamed(&self, h: ClipH) -> bool {
        self.clip(h).is_some_and(|(c, _)| c.paged().is_some())
    }

    /// La taille au-dela de laquelle un fichier se lit en flux (les tests).
    pub fn set_stream_above(&mut self, bytes: usize) {
        self.stream_above = bytes;
    }

    /// Plus aucun lancement ; la matiere part quand plus rien ne la joue.
    pub fn release(&mut self, h: ClipH) {
        if self.clip(h).is_some() {
            self.pool.retire(h.id());
            self.clips[h.id() as usize] = None;
        }
    }

    pub fn peaks(
        &self,
        h: ClipH,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        out_min: &mut [f32],
        out_max: &mut [f32],
    ) -> usize {
        match self.clip(h) {
            Some((c, e)) => match &e.peaks {
                Some(p) => p.query(c, ch, start, fpp, out_min, out_max),
                None => peaks::approx(c, ch, start, fpp, out_min, out_max),
            },
            None => 0,
        }
    }

    // -----------------------------------------------------------------------
    // Le decodage
    // -----------------------------------------------------------------------

    fn new_ticket(&mut self) -> u32 {
        let t = self.next_ticket;
        self.next_ticket = self.next_ticket.wrapping_add(1).max(1);
        t
    }

    /// Ouvre un fichier par REAPER et le donne au fil de travail. Rend un ticket.
    pub fn load_file(&mut self, path: &str) -> u32 {
        let t = self.new_ticket();
        let src = self
            .reaper
            .as_ref()
            .and_then(|r| cp_sys::audio::open_file(r, path));
        match src {
            Some(s) => self.submit(t, Box::new(s), true),
            None => self
                .tickets
                .push((t, Ticket::Failed("REAPER cannot open this file".into()))),
        }
        t
    }

    /// La region `[start, end)` du clip `h` (`end` = 0 : jusqu'au bout),
    /// etiree par l'etireur de REAPER (`tempo`, rapport de hauteur `shift`)
    /// et cuite sur le fil de travail : un ticket, comme un fichier. Le clip
    /// cuit se joue a vitesse 1, a la duree exacte.
    pub fn stretch(&mut self, h: ClipH, start: i64, end: i64, tempo: f64, shift: f64) -> u32 {
        if self.clip_streamed(h) {
            let t = self.new_ticket();
            self.tickets.push((
                t,
                Ticket::Failed("a streamed file cannot be stretched".into()),
            ));
            return t;
        }
        let sh = self
            .reaper
            .as_ref()
            .and_then(|r| r.pitch_shift())
            .map(|p| Box::new(p) as Box<dyn stretch::Shifter>);
        match sh {
            Some(s) => self.stretch_with(h, start, end, tempo, shift, s),
            None => {
                let t = self.new_ticket();
                self.tickets
                    .push((t, Ticket::Failed("no time-stretch engine".into())));
                t
            }
        }
    }

    /// [`Self::stretch`] avec un etireur donne (les tests).
    pub fn stretch_with(
        &mut self,
        h: ClipH,
        start: i64,
        end: i64,
        tempo: f64,
        shift: f64,
        sh: Box<dyn stretch::Shifter>,
    ) -> u32 {
        let t = self.new_ticket();
        let Some(clip) = self.clip(h).map(|c| c.0.clone()) else {
            self.tickets
                .push((t, Ticket::Failed("the clip is gone".into())));
            return t;
        };
        if clip.paged().is_some() {
            self.tickets.push((
                t,
                Ticket::Failed("a streamed file cannot be stretched".into()),
            ));
            return t;
        }
        if !self.clip_complete(h) {
            self.tickets
                .push((t, Ticket::Failed("the clip is still loading".into())));
            return t;
        }
        let r = stretch::StretchReader::new(clip, start, end, tempo, shift, sh);
        self.submit(t, Box::new(r), false);
        t
    }

    /// Une source quelconque (tests).
    pub fn load_reader(&mut self, r: Box<dyn Reader>) -> u32 {
        let t = self.new_ticket();
        self.submit(t, r, true);
        t
    }

    /// Lit la forme (fil principal), prepare un clip qui se remplira, et
    /// confie le decodage au fil de travail.
    /// Un long fichier (`streamable` : une source qu'on peut lire n'importe
    /// ou) se lit en flux.
    fn submit(&mut self, t: u32, mut src: Box<dyn Reader>, streamable: bool) {
        // On ne garde que les derniers tickets : un ticket jamais relu ne
        // s'accumule pas.
        if self.tickets.len() > 64 {
            let n = self.tickets.len() - 64;
            self.tickets.drain(..n);
        }
        let sh = match decode::shape(src.as_mut()) {
            Ok(sh) => sh,
            Err(e) => {
                self.tickets.push((t, Ticket::Failed(e)));
                return;
            }
        };
        if streamable && sh.frames.saturating_mul(sh.nch * 4) > self.stream_above {
            self.submit_stream(t, src, sh);
            return;
        }
        let max_frames = (MAX_FILE_BYTES / 4 / sh.nch).min(self.max_frames);
        let added = self
            .pool
            .add_growing(sh.frames as i64, sh.nch, sh.srate, max_frames as i64);
        let (id, w) = match added {
            Ok(x) => x,
            Err(cp_engine::PoolError::TooLong) => {
                self.tickets.push((
                    t,
                    Ticket::Failed(format!(
                        "file too long for now ({:.0} min, the limit is {:.0} min)",
                        sh.frames as f64 / sh.srate / 60.0,
                        max_frames as f64 / sh.srate / 60.0
                    )),
                ));
                return;
            }
            Err(e) => {
                self.tickets.push((t, Ticket::Failed(format!("{e:?}"))));
                return;
            }
        };
        let generation = self.new_generation(id);
        self.clips[id as usize] = Some(ClipEntry {
            generation,
            peaks: None,
        });
        let h = ClipH::new(id, generation);
        let job = Job {
            ticket: t,
            src,
            shape: sh,
            w,
        };
        match self.decoder.submit(job) {
            Ok(()) => {
                self.decoding += 1;
                self.in_flight.push((t, h));
                self.tickets.push((t, Ticket::Pending(h)));
            }
            Err(_) => {
                self.release(h);
                self.tickets
                    .push((t, Ticket::Failed("decoder thread is gone".into())));
            }
        }
    }

    /// Un long fichier : un clip en pages, que le fil de flux remplit autour
    /// de la tete. Pret des que son debut est la.
    fn submit_stream(&mut self, t: u32, src: Box<dyn Reader>, sh: decode::Shape) {
        let page = decode::chunk_frames(sh.srate);
        let (id, pages) = match self
            .pool
            .add_paged(sh.frames as i64, sh.nch, sh.srate, page)
        {
            Ok(x) => x,
            Err(e) => {
                self.tickets.push((t, Ticket::Failed(format!("{e:?}"))));
                return;
            }
        };
        let generation = self.new_generation(id);
        self.clips[id as usize] = Some(ClipEntry {
            generation,
            peaks: None,
        });
        let h = ClipH::new(id, generation);
        let job = stream::StreamJob {
            key: h.0,
            src,
            shape: sh,
            pages,
        };
        let st = self.streamer.get_or_insert_with(stream::Streamer::new);
        match st.add(job) {
            Ok(()) => self.tickets.push((t, Ticket::Pending(h))),
            Err(_) => {
                self.release(h);
                self.tickets
                    .push((t, Ticket::Failed("stream thread is gone".into())));
            }
        }
    }

    /// Les nouvelles du fil de flux.
    fn stream_poll(&mut self) {
        let Some(st) = &self.streamer else { return };
        let mut msgs = Vec::new();
        while let Some(m) = st.try_msg() {
            msgs.push(m);
        }
        for m in msgs {
            match m {
                stream::SMsg::Ready { key } => {
                    for e in &mut self.tickets {
                        if matches!(e.1, Ticket::Pending(h) if h.0 == key) {
                            e.1 = Ticket::Ready(ClipH(key));
                        }
                    }
                }
                stream::SMsg::Peaks { key, peaks, .. } => {
                    let h = ClipH(key);
                    if let Some(Some(e)) = self.clips.get_mut(h.id() as usize) {
                        if e.generation == h.generation() {
                            e.peaks = Some(peaks);
                        }
                    }
                }
                // La source est detruite ICI, sur le fil principal.
                stream::SMsg::Gone { src, .. } => drop(src),
                stream::SMsg::Error { key, error } => {
                    let mut pending = false;
                    for e in &mut self.tickets {
                        if matches!(e.1, Ticket::Pending(h) if h.0 == key) {
                            e.1 = Ticket::Failed(error.clone());
                            pending = true;
                        }
                    }
                    if pending {
                        self.release(ClipH(key));
                    }
                }
            }
        }
    }

    /// L'etat d'un ticket. Un ticket fini (pret ou en echec) est oublie apres
    /// avoir ete lu une fois.
    pub fn ticket(&mut self, t: u32) -> Load {
        let Some(i) = self.tickets.iter().position(|(k, _)| *k == t) else {
            return Load::Failed("unknown ticket".into());
        };
        match &self.tickets[i].1 {
            Ticket::Pending(_) => Load::Pending,
            _ => match self.tickets.remove(i).1 {
                Ticket::Ready(h) => Load::Ready(h),
                Ticket::Failed(e) => Load::Failed(e),
                Ticket::Pending(_) => Load::Pending,
            },
        }
    }

    /// La vignette d'un fichier : demandee au premier appel (basse priorite),
    /// rendue quand elle est prete.
    pub fn thumb(&mut self, path: &str) -> Thumb {
        if let Some(t) = self.thumbs.get(path) {
            return t.clone();
        }
        if self.thumbs.len() >= THUMBS_KEPT {
            self.thumbs.clear();
        }
        let src = self
            .reaper
            .as_ref()
            .and_then(|r| cp_sys::audio::open_file(r, path));
        let Some(src) = src else {
            self.thumbs.insert(path.to_string(), Thumb::Failed);
            return Thumb::Failed;
        };
        self.thumb_reader(path, Box::new(src))
    }

    /// Une vignette depuis une source quelconque (tests).
    pub fn thumb_reader(&mut self, path: &str, mut src: Box<dyn Reader>) -> Thumb {
        let st = match decode::shape(src.as_mut()) {
            Ok(sh) if (sh.frames as f64) / sh.srate <= THUMB_MAX_SECONDS => {
                let job = decode::ThumbJob {
                    path: path.to_string(),
                    src,
                    shape: sh,
                    points: THUMB_POINTS,
                };
                if self.decoder.submit_thumb(job) {
                    Thumb::Pending
                } else {
                    Thumb::Failed
                }
            }
            _ => Thumb::Failed,
        };
        self.thumbs.insert(path.to_string(), st.clone());
        st
    }

    /// Le clip est-il entierement decode ?
    pub fn clip_complete(&self, h: ClipH) -> bool {
        self.clip(h).is_some_and(|(c, _)| c.complete())
    }

    /// Le battement du fil principal : les decodages finis entrent au vivier,
    /// les voix finies sont rendues, la memoire liberable est liberee.
    pub fn poll(&mut self) {
        self.check_routes();
        self.rec_poll();
        self.midi_rec_poll();
        self.stream_poll();
        while let Some(m) = self.decoder.try_msg() {
            match m {
                decode::Msg::Thumb { path, src, data } => {
                    drop(src);
                    let st = if data.is_empty() {
                        Thumb::Failed
                    } else {
                        Thumb::Ready(Arc::new(data))
                    };
                    self.thumbs.insert(path, st);
                }
                decode::Msg::First { ticket } => {
                    if let Some(e) = self.tickets.iter_mut().find(|(k, _)| *k == ticket) {
                        if let Ticket::Pending(h) = e.1 {
                            e.1 = Ticket::Ready(h);
                        }
                    }
                }
                decode::Msg::Done {
                    ticket,
                    src,
                    peaks,
                    error,
                } => {
                    self.decoding = self.decoding.saturating_sub(1);
                    // La source est detruite ICI, sur le fil principal.
                    drop(src);
                    let h = self
                        .in_flight
                        .iter()
                        .position(|(k, _)| *k == ticket)
                        .map(|i| self.in_flight.remove(i).1);
                    if let Some(e) = error {
                        if let Some(t) = self.tickets.iter_mut().find(|(k, _)| *k == ticket) {
                            t.1 = Ticket::Failed(e);
                        }
                        if let Some(h) = h {
                            self.release(h);
                        }
                        continue;
                    }
                    // Les pics exacts rejoignent le clip, s'il vit encore.
                    if let (Some(p), Some(h)) = (peaks, h) {
                        if let Some(Some(e)) = self.clips.get_mut(h.id() as usize) {
                            if e.generation == h.generation() {
                                e.peaks = Some(Arc::new(p));
                            }
                        }
                    }
                }
            }
        }
        let core = self.ctl.core().clone();
        let mut finished = Vec::new();
        self.voices.retain(|&v| match core.voice_query(v) {
            None => false,
            Some((_, st)) => {
                let done = st == state::IDLE && core.voice_started_at(v) >= 0;
                if done {
                    finished.push(v);
                }
                !done
            }
        });
        for v in finished {
            self.ctl.voice_release(v);
        }
        let live = &self.voices;
        self.streamed.retain(|(v, _)| live.contains(v));
        // Les etireurs des voix rendues reviennent au vivier.
        self.reclaim_shifters();
        self.pool.collect();
        let sr = self.ctl.core().srate();
        if self.preview_srate > 0.0 && sr != self.preview_srate {
            for p in &mut self.ports {
                if let Some(pv) = p.preview.as_mut().filter(|pv| pv.is_playing()) {
                    pv.stop();
                    pv.play_hardware(0);
                }
            }
            self.preview_srate = sr;
        }
    }

    // -----------------------------------------------------------------------
    // Le jeu
    // -----------------------------------------------------------------------

    /// Joue `[start, end)` (frames du clip ; `end` = 0 : jusqu'au bout), une
    /// fois ou en boucle, sur un port. Rend la voix.
    pub fn play(
        &mut self,
        port: usize,
        h: ClipH,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
    ) -> Option<VoiceH> {
        self.play_at(port, h, start, end, looped, gain, None)
    }

    /// [`Self::play`], au temps du moteur `at` (une frame absolue ; `None` :
    /// tout de suite) : un lancement quantifie, a l'echantillon.
    #[allow(clippy::too_many_arguments)]
    pub fn play_at(
        &mut self,
        port: usize,
        h: ClipH,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
        when: Option<i64>,
    ) -> Option<VoiceH> {
        let clip = self.clip(h)?.0.clone();
        if !self.ensure_port(port) {
            return None;
        }
        // Un clip en flux : sa page de depart est demandee tout de suite ;
        // si elle n'est pas la, le depart attend 50 ms (une page se lit en
        // quelques millisecondes) plutot que de commencer dans le silence.
        let mut at = when.map_or(At::Now, At::Frame);
        let clip_pages = clip.paged().cloned();
        if let Some(p) = clip.paged() {
            let s = start.max(0);
            p.hint(s);
            if !p.has(p.page_of(s)) {
                if let Some(st) = &self.streamer {
                    st.wake();
                }
                let core = self.ctl.core();
                let soon = core.clock_now() + (0.05 * core.srate().max(1.0)) as i64;
                at = At::Frame(when.map_or(soon, |w| w.max(soon)));
            }
        }
        let v = self.ctl.voice_alloc(port)?;
        let sets = [
            (Param::LoopStart, start.max(0) as f64),
            (Param::LoopEnd, end.max(0) as f64),
        ];
        for (param, value) in sets {
            self.ctl.send(v, At::Now, VoiceOp::Set { param, value });
        }
        let ok = self.ctl.send(
            v,
            at,
            VoiceOp::Play {
                clip,
                rate: 1.0,
                gain,
                mode: if looped {
                    PlayMode::Loop
                } else {
                    PlayMode::Once
                },
            },
        );
        if !ok {
            self.ctl.voice_release(v);
            return None;
        }
        self.voices.push(v);
        if let Some(p) = clip_pages {
            self.streamed.push((v, p));
        }
        Some(v)
    }

    /// Arrete une voix, avec un fondu (secondes), et la rend.
    pub fn stop(&mut self, v: VoiceH, fade_s: f64) {
        self.voices.retain(|&x| x != v);
        self.ctl.voice_release_with(v, fade_s.max(0.0));
    }

    pub fn set(&mut self, v: VoiceH, param: Param, value: f64) -> bool {
        // Un saut dans un clip en flux : sa page est demandee tout de suite.
        if param == Param::Pos {
            if let Some((_, p)) = self.streamed.iter().find(|(w, _)| *w == v) {
                p.hint(value as i64);
                if let Some(st) = &self.streamer {
                    st.wake();
                }
            }
        }
        // Le tempo ou la hauteur a part : la voix emprunte un etireur.
        if matches!(param, Param::Tempo | Param::Shift)
            && (value - 1.0).abs() > 1e-9
            && !self.lent.iter().any(|(w, _)| *w == v)
        {
            if let Some(slot) = self.free_shifter() {
                if self
                    .ctl
                    .send(v, At::Now, VoiceOp::Shifter(Some(slot.clone())))
                {
                    self.lent.push((v, slot));
                }
            }
        }
        self.ctl.send(v, At::Now, VoiceOp::Set { param, value })
    }

    /// Un etireur libre du vivier (personne d'autre ne le tient), ou un
    /// neuf. Fil principal.
    fn free_shifter(&mut self) -> Option<cp_engine::shift::ShiftSlot> {
        if let Some(s) = self.shifters.iter().find(|s| Arc::strong_count(s) == 1) {
            return Some(s.clone());
        }
        if self.shifters.len() >= MAX_SHIFTERS {
            return None;
        }
        let sh: Box<dyn cp_engine::shift::RtShift> = match (&self.shift_factory, &self.reaper) {
            (Some(f), _) => f(),
            (None, Some(r)) => Box::new(stretch::RtElastique(r.pitch_shift()?)),
            (None, None) => return None,
        };
        let slot = cp_engine::shift::ShiftState::new(sh, self.ctl.core().srate());
        self.shifters.push(slot.clone());
        Some(slot)
    }

    /// Les tests : la fabrique des etireurs temps reel hors de REAPER.
    pub fn set_shift_factory(
        &mut self,
        f: Box<dyn Fn() -> Box<dyn cp_engine::shift::RtShift> + Send>,
    ) {
        self.shift_factory = Some(f);
    }

    /// Les etireurs pretes a des voix finies reviennent au vivier.
    fn reclaim_shifters(&mut self) {
        let core = self.ctl.core().clone();
        self.lent.retain(|(v, _)| core.voice_query(*v).is_some());
    }

    /// Combien d'etireurs temps reel existent, et combien sont pretes.
    pub fn shifters_in_use(&self) -> (usize, usize) {
        (self.shifters.len(), self.lent.len())
    }

    /// (position en frames du clip, etat) d'une voix vivante.
    pub fn voice(&self, v: VoiceH) -> Option<(f64, i32)> {
        self.ctl.core().voice_query(v)
    }

    pub fn stats(&self) -> Stats {
        let core = self.ctl.core();
        let mut s = Stats {
            srate: core.srate(),
            blocks: core.block_index(),
            last_block: core.last_block_frames(),
            hook: self.hook,
            busy_renders: core.busy_renders(),
            dropped_commands: core.dropped_commands(),
            active_voices: core.active_voices(),
            clips: self.pool.loaded_count(),
            bytes: self.pool.bytes_resident() + self.pool.bytes_streamed(),
            decoding: self.decoding,
            streams: self.pool.streams().0,
            stream_misses: self.pool.streams().1,
            routes_lost: self.routes_lost,
            ..Stats::default()
        };
        for (i, p) in self.ports.iter().enumerate() {
            let open =
                p.preview.as_ref().is_some_and(|pv| pv.is_playing()) || p.feed_for_tests.is_some();
            if !open {
                continue;
            }
            s.ports_open += 1;
            use std::sync::atomic::Ordering::Relaxed;
            s.calls += p.stats.calls.load(Relaxed);
            s.rewinds += p.stats.rewinds.load(Relaxed);
            s.skips += p.stats.skips.load(Relaxed);
            s.busy_ns += p.stats.busy_ns.load(Relaxed);
            s.frames_pulled = s.frames_pulled.max(p.stats.frames.load(Relaxed));
            s.over_pulls += core.port_over_pulls(i);
            s.under_pulls += core.port_under_pulls(i);
        }
        s
    }

    /// Nombre de canaux maximum d'un clip.
    pub const MAX_CHANS: usize = MAX_CHANS;
}

impl Drop for AudioHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests;
