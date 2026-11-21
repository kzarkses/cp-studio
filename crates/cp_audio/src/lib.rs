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

pub mod decode;
pub mod feed;
pub mod peaks;
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

/// Un fichier ne depasse pas 1,5 Go en memoire (23 min en stereo a 48 kHz).
const MAX_FILE_BYTES: usize = 1_500_000_000;

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
    pub busy_ns: u64,
    /// Le plus long temps de tirage d'un port, en frames.
    pub frames_pulled: u64,
    pub active_voices: usize,
    pub clips: usize,
    pub bytes: usize,
    pub decoding: usize,
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

fn on_audio(is_post: bool, frames: i32, srate: f64) {
    let Some(core) = HOOK_CORE.get() else { return };
    if is_post {
        core.tick(frames);
    } else {
        core.set_srate(srate);
    }
}

pub struct AudioHost {
    reaper: Option<Reaper>,
    ctl: Ctl,
    pool: Pool,
    clips: Vec<Option<ClipEntry>>,
    clip_gens: Vec<u16>,
    ports: Vec<PortOut>,
    decoder: Decoder,
    tickets: Vec<(u32, Ticket)>,
    /// Les decodages en cours : ticket -> clip (pour y poser les pics).
    in_flight: Vec<(u32, ClipH)>,
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
}

impl AudioHost {
    /// Dans REAPER : `Some(reaper)`, le crochet est enregistre. En test : `None`,
    /// et le test tire les ports lui-meme ([`AudioHost::pull`]).
    pub fn new(reaper: Option<Reaper>, srate: f64) -> AudioHost {
        let ctl = Ctl::new(if srate > 1.0 { srate } else { 48000.0 });
        let mut hook = false;
        if let Some(r) = &reaper {
            if HOOK_CORE.set(ctl.core().clone()).is_ok() {
                hook = AudioHook::register(r, on_audio);
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
            tickets: Vec::new(),
            in_flight: Vec::new(),
            next_ticket: 1,
            decoding: 0,
            voices: Vec::new(),
            hook,
            max_frames: usize::MAX,
            preview_srate: 0.0,
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
                let ok = p.preview.as_mut().is_some_and(|pv| pv.play_hardware(0));
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

    /// Frames deja decodes d'un clip vivant.
    pub fn clip_decoded(&self, h: ClipH) -> Option<i64> {
        self.clip(h)
            .map(|(c, _)| (c.samples().len() / c.nch().max(1)) as i64)
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
            Some(s) => self.submit(t, Box::new(s)),
            None => self
                .tickets
                .push((t, Ticket::Failed("REAPER cannot open this file".into()))),
        }
        t
    }

    /// Une source quelconque (tests).
    pub fn load_reader(&mut self, r: Box<dyn Reader>) -> u32 {
        let t = self.new_ticket();
        self.submit(t, r);
        t
    }

    /// Lit la forme (fil principal), prepare un clip qui se remplira, et
    /// confie le decodage au fil de travail.
    fn submit(&mut self, t: u32, mut src: Box<dyn Reader>) {
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

    /// Le clip est-il entierement decode ?
    pub fn clip_complete(&self, h: ClipH) -> bool {
        self.clip(h).is_some_and(|(c, _)| c.complete())
    }

    /// Le battement du fil principal : les decodages finis entrent au vivier,
    /// les voix finies sont rendues, la memoire liberable est liberee.
    pub fn poll(&mut self) {
        while let Some(m) = self.decoder.try_msg() {
            match m {
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
        let clip = self.clip(h)?.0.clone();
        if !self.ensure_port(port) {
            return None;
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
            At::Now,
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
        Some(v)
    }

    /// Arrete une voix, avec un fondu (secondes), et la rend.
    pub fn stop(&mut self, v: VoiceH, fade_s: f64) {
        self.voices.retain(|&x| x != v);
        self.ctl.voice_release_with(v, fade_s.max(0.0));
    }

    pub fn set(&mut self, v: VoiceH, param: Param, value: f64) -> bool {
        self.ctl.send(v, At::Now, VoiceOp::Set { param, value })
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
            bytes: self.pool.bytes_resident(),
            decoding: self.decoding,
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
