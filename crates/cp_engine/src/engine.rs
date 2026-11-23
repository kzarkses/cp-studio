//! Le moteur : des ports (une colonne = un port), leurs voix, une horloge
//! (ancien `cp_engine`).
//!
//! L'horloge ne DEMANDE jamais l'heure, elle COMPTE ses echantillons : `tick`
//! l'avance d'un bloc, APRES le rendu du bloc (le passage « post » du crochet
//! audio). `clock` est donc l'index absolu du prochain echantillon a produire.
//!
//! Partage :
//! - fil principal ([`Ctl`]) : les mots de propriete, les producteurs des
//!   anneaux de commandes ;
//! - fil audio ([`Core::render_port`], [`Core::tick`]) : les voix, les listes de
//!   port, l'horloge. Chaque port est sous son propre verrou, pris en
//!   `try_lock` : il n'est jamais dispute en marche normale (REAPER rend un port
//!   une fois par bloc, sur un fil), et le fil audio n'attend JAMAIS. Le fil
//!   principal ne le prend que pour `port_reset`, quand le port ne rend plus.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex, MutexGuard};

use cp_sys::rt::{Consumer, Producer, spsc};

use crate::pool::ClipData;
use crate::types::*;
use crate::voice::{Voice, VoicePub};

/// Ce qu'on demande a une voix.
#[derive(Clone, Debug)]
pub enum VoiceOp {
    Play {
        clip: Arc<ClipData>,
        rate: f64,
        gain: f64,
        mode: PlayMode,
    },
    /// `fade_s` = 0 et `At::Now` : coupure au debut du bloc.
    Stop {
        fade_s: f64,
    },
    Set {
        param: Param,
        value: f64,
    },
    /// A la fin de cette voix, `next` (du meme port) demarre exactement la.
    Queue {
        next: VoiceH,
        xfade_s: f64,
    },
    /// Prete (ou reprend, `None`) un etireur temps reel a la voix.
    Shifter(Option<crate::shift::ShiftSlot>),
    /// Un point d'enveloppe en attente (rien ne s'entend avant `Param::EnvPts`).
    EnvPt {
        idx: i32,
        t: f64,
        g: f64,
        curve: f32,
    },
}

pub(crate) enum Cmd {
    Alloc {
        local: usize,
        gen_: u16,
    },
    Free {
        local: usize,
        gen_: u16,
        new_gen: u16,
        fade_s: f64,
    },
    Voice {
        local: usize,
        gen_: u16,
        at: At,
        op: VoiceOp,
    },
    PortGain(f32),
    Panic,
}

struct PortAudio {
    port: usize,
    cmds: Consumer<Cmd>,
    voices: Box<[Voice]>,
    /// Les voix vivantes de ce port : le fil audio ne parcourt qu'elles.
    vlist: [i16; MAX_PORT_VOICES],
    vcount: usize,
    gain: f32,
    gain_target: f32,
    last_clock: Frame,
    consumed: i32,
}

struct PortSlot {
    audio: Mutex<PortAudio>,
    pubs: Box<[VoicePub]>,
    claims: Box<[AtomicU32]>,
    used: AtomicBool,
    /// Diagnostic : plus (moins) de frames rendus que le bloc n'en avait. Le
    /// moteur suppose que REAPER lit un port exactement une fois par bloc.
    over_pulls: AtomicU32,
    under_pulls: AtomicU32,
}

/// La moitie partagee : ce que le fil audio appelle, et ce que tout le monde lit.
pub struct Core {
    ports: Box<[PortSlot]>,
    srate: AtomicU64,
    clock: AtomicI64,
    blocks: AtomicI64,
    dropped: AtomicU32,
    busy: AtomicU32,
    last_tick: AtomicI32,
    clock_external: AtomicBool,
    clock_master: AtomicI32,
    last_master_frames: AtomicI32,
}

/// La moitie du fil principal.
pub struct Ctl {
    core: Arc<Core>,
    producers: Vec<Producer<Cmd>>,
    cursors: [usize; MAX_PORTS],
}

fn lock_blocking<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Ctl {
    pub fn new(srate: f64) -> Ctl {
        // La table du sinc, construite ici (fil principal) : le fil audio
        // ne fait que la lire.
        crate::sinc::init();
        let mut producers = Vec::with_capacity(MAX_PORTS);
        let mut ports = Vec::with_capacity(MAX_PORTS);
        for port in 0..MAX_PORTS {
            let (p, c) = spsc::<Cmd>(CMD_RING_CAP);
            producers.push(p);
            ports.push(PortSlot {
                audio: Mutex::new(PortAudio {
                    port,
                    cmds: c,
                    voices: (0..MAX_PORT_VOICES).map(|_| Voice::new()).collect(),
                    vlist: [-1; MAX_PORT_VOICES],
                    vcount: 0,
                    gain: 1.0,
                    gain_target: 1.0,
                    last_clock: -1,
                    consumed: 0,
                }),
                pubs: (0..MAX_PORT_VOICES).map(|_| VoicePub::new()).collect(),
                claims: (0..MAX_PORT_VOICES).map(|_| AtomicU32::new(1)).collect(),
                used: AtomicBool::new(false),
                over_pulls: AtomicU32::new(0),
                under_pulls: AtomicU32::new(0),
            });
        }
        let core = Arc::new(Core {
            ports: ports.into_boxed_slice(),
            srate: AtomicU64::new(if srate > 1.0 { srate } else { 48000.0 }.to_bits()),
            clock: AtomicI64::new(0),
            blocks: AtomicI64::new(0),
            dropped: AtomicU32::new(0),
            busy: AtomicU32::new(0),
            last_tick: AtomicI32::new(0),
            clock_external: AtomicBool::new(false),
            clock_master: AtomicI32::new(-1),
            last_master_frames: AtomicI32::new(0),
        });
        Ctl {
            core,
            producers,
            cursors: [0; MAX_PORTS],
        }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    /// Le taux est une entree, pas une constante : REAPER peut changer de
    /// peripherique sous nos pieds.
    pub fn set_srate(&self, srate: f64) {
        self.core.set_srate(srate);
    }

    fn post(&mut self, port: usize, c: Cmd) -> bool {
        match self.producers[port].push(c) {
            Ok(()) => true,
            Err(_) => {
                self.core.dropped.fetch_add(1, Relaxed);
                false
            }
        }
    }

    /// Reserve une voix sur un port. N'ecrit PAS dans la voix : pose le mot de
    /// propriete, puis demande au fil audio de preparer l'emplacement.
    pub fn voice_alloc(&mut self, port: usize) -> Option<VoiceH> {
        if port >= MAX_PORTS {
            return None;
        }
        for k in 0..MAX_PORT_VOICES {
            let i = (self.cursors[port] + k) % MAX_PORT_VOICES;
            let slot = &self.core.ports[port].claims[i];
            let cur = slot.load(Acquire);
            if claim_owned(cur) {
                continue;
            }
            let g = next_gen(claim_gen(cur));
            // On PREND d'abord, on demande ensuite : un emplacement ne peut pas
            // etre pris deux fois, meme si la commande se perd.
            slot.store(g as u32 | CLAIM_OWNED, Release);
            if !self.post(port, Cmd::Alloc { local: i, gen_: g }) {
                self.core.ports[port].claims[i].store(g as u32, Release);
                return None;
            }
            self.core.ports[port].used.store(true, Relaxed);
            self.cursors[port] = i + 1;
            return Some(VoiceH::new(port, i, g));
        }
        None
    }

    /// Rend la voix (fondu de 5 ms). La generation avance tout de suite : toute
    /// commande ulterieure portant l'ancien handle est refusee a la frontiere.
    /// L'emplacement reste possede jusqu'a ce que le fil audio l'ait eteint.
    pub fn voice_release(&mut self, h: VoiceH) {
        self.voice_release_with(h, 0.005);
    }

    /// Comme [`Ctl::voice_release`], avec un fondu de sortie choisi (secondes).
    pub fn voice_release_with(&mut self, h: VoiceH, fade_s: f64) {
        if !self.core.voice_valid(h) {
            return;
        }
        let (port, local) = (h.port(), h.local());
        let ng = next_gen(h.generation());
        self.core.ports[port].claims[local].store(ng as u32 | CLAIM_OWNED, Release);
        self.post(
            port,
            Cmd::Free {
                local,
                gen_: h.generation(),
                new_gen: ng,
                fade_s,
            },
        );
    }

    /// Envoie une demande a une voix, sur l'anneau de SON port. Faux si le
    /// handle est perime ou l'anneau plein.
    pub fn send(&mut self, h: VoiceH, at: At, op: VoiceOp) -> bool {
        if !self.core.voice_valid(h) {
            return false;
        }
        self.post(
            h.port(),
            Cmd::Voice {
                local: h.local(),
                gen_: h.generation(),
                at,
                op,
            },
        )
    }

    pub fn port_gain(&mut self, port: usize, gain: f64) -> bool {
        port < MAX_PORTS && self.post(port, Cmd::PortGain(gain as f32))
    }

    /// Tout couper, sans clic.
    pub fn panic(&mut self) {
        for p in 0..MAX_PORTS {
            if self.core.ports[p].used.load(Relaxed) {
                self.post(p, Cmd::Panic);
            }
        }
    }

    /// Rend au port toutes ses voix, immediatement. A n'appeler QUE quand le
    /// port ne rend plus (apres l'arret de son apercu) : sans ca, une voix en
    /// fondu resterait eternellement en STOPPING.
    pub fn port_reset(&mut self, port: usize) {
        if port >= MAX_PORTS {
            return;
        }
        let ps = &self.core.ports[port];
        let mut a = lock_blocking(&ps.audio);
        a.cmds.clear();
        a.vcount = 0;
        a.last_clock = -1;
        a.consumed = 0;
        a.gain = 1.0;
        a.gain_target = 1.0;
        ps.used.store(false, Relaxed);
        for i in 0..MAX_PORT_VOICES {
            let cur = ps.claims[i].load(Relaxed);
            if !claim_owned(cur) {
                continue;
            }
            let g = next_gen(claim_gen(cur));
            let v = &mut a.voices[i];
            v.reset();
            v.gen_ = g;
            v.publish(&ps.pubs[i]);
            ps.claims[i].store(g as u32, Release);
        }
    }
}

impl Core {
    /// Le taux du peripherique. Appelable depuis le fil audio (le crochet le
    /// lit a chaque bloc) : une simple ecriture atomique.
    pub fn set_srate(&self, srate: f64) {
        if srate > 1.0 && srate != self.srate() {
            self.srate.store(srate.to_bits(), Release);
        }
    }
    pub fn srate(&self) -> f64 {
        f64::from_bits(self.srate.load(Acquire))
    }
    pub fn clock_now(&self) -> Frame {
        self.clock.load(Acquire)
    }
    pub fn block_index(&self) -> Frame {
        self.blocks.load(Acquire)
    }
    pub fn last_block_frames(&self) -> i32 {
        self.last_tick.load(Relaxed)
    }
    pub fn dropped_commands(&self) -> u32 {
        self.dropped.load(Relaxed)
    }
    /// Rendus refuses parce que le port etait deja en cours de rendu.
    pub fn busy_renders(&self) -> u32 {
        self.busy.load(Relaxed)
    }
    pub fn port_over_pulls(&self, p: usize) -> u32 {
        self.ports.get(p).map_or(0, |x| x.over_pulls.load(Relaxed))
    }
    pub fn port_under_pulls(&self, p: usize) -> u32 {
        self.ports.get(p).map_or(0, |x| x.under_pulls.load(Relaxed))
    }

    fn claim(&self, h: VoiceH) -> Option<u32> {
        let c = self.ports.get(h.port())?.claims[h.local()].load(Acquire);
        (claim_owned(c) && claim_gen(c) == h.generation()).then_some(c)
    }

    pub fn voice_valid(&self, h: VoiceH) -> bool {
        self.claim(h).is_some()
    }

    /// Le port d'une voix, `None` si le handle est perime.
    pub fn voice_port(&self, h: VoiceH) -> Option<usize> {
        self.claim(h).map(|_| h.port())
    }

    /// Frame absolu du premier echantillon reellement audible, -1 si la voix n'a
    /// pas encore demarre. Note par la voix elle-meme : sans course.
    pub fn voice_started_at(&self, h: VoiceH) -> Frame {
        if !self.voice_valid(h) {
            return -1;
        }
        self.ports[h.port()].pubs[h.local()].started.load(Relaxed)
    }

    /// (position en frames source, etat), tels que publies en fin de bloc.
    pub fn voice_query(&self, h: VoiceH) -> Option<(f64, i32)> {
        self.claim(h)?;
        let p = &self.ports[h.port()].pubs[h.local()];
        Some((p.pos(), p.state.load(Relaxed)))
    }

    pub fn active_voices(&self) -> usize {
        self.ports
            .iter()
            .flat_map(|p| p.pubs.iter())
            .filter(|p| p.state.load(Relaxed) != state::IDLE)
            .count()
    }

    pub fn owned_voices(&self) -> usize {
        self.ports
            .iter()
            .flat_map(|p| p.claims.iter())
            .filter(|c| claim_owned(c.load(Relaxed)))
            .count()
    }

    /// Taille de la liste que le fil audio parcourt pour ce port. Une autopsie,
    /// a lire quand le fil audio est arrete.
    pub fn port_list_size(&self, port: usize) -> Option<usize> {
        Some(lock_blocking(&self.ports.get(port)?.audio).vcount)
    }

    // -----------------------------------------------------------------------
    // Fil audio
    // -----------------------------------------------------------------------

    /// Avance l'horloge d'un bloc. A APPELER APRES le rendu du bloc (passage
    /// « post ») : l'appeler avant ferait tout partir un bloc trop tot.
    /// L'horloge viendra de [`Core::tick`] (le crochet audio de REAPER) des
    /// le premier bloc : aucun port ne sert d'horloge, meme tire avant le
    /// premier battement (sinon il l'avancerait par morceaux, au milieu du
    /// bloc, et chaque port compterait une perte au demarrage).
    pub fn use_external_clock(&self) {
        self.clock_external.store(true, Relaxed);
    }

    pub fn tick(&self, frames: i32) {
        if frames <= 0 {
            return;
        }
        self.clock_external.store(true, Relaxed);
        self.last_tick.store(frames, Relaxed);
        self.clock.fetch_add(frames as Frame, AcqRel);
        self.blocks.fetch_add(1, AcqRel);
        // Les lanes MIDI travailleront ici sur le bloc A VENIR (pas encore
        // portees).
    }

    /// Remplit `out` (entrelace, `nch` canaux) pour un port ; ecrase le tampon.
    /// Rend le frame absolu du premier echantillon, ou `None` si refuse.
    pub fn render_port(
        &self,
        port: usize,
        out: &mut [f32],
        frames: usize,
        nch: usize,
    ) -> Option<Frame> {
        if port >= MAX_PORTS || frames == 0 || nch < 1 || out.len() < frames * nch {
            return None;
        }
        let out = &mut out[..frames * nch];
        let ps = &self.ports[port];
        ps.used.store(true, Relaxed);
        let Ok(mut a) = ps.audio.try_lock() else {
            out.fill(0.0);
            self.busy.fetch_add(1, Relaxed);
            return None;
        };
        let srate = self.srate();

        // --- ou sommes-nous ?
        if !self.clock_external.load(Relaxed) {
            // Aucun crochet audio : un port fait office d'horloge. Il avance du
            // bloc PRECEDENT, en entrant.
            let _ = self
                .clock_master
                .compare_exchange(-1, port as i32, Relaxed, Relaxed);
            if self.clock_master.load(Relaxed) == port as i32 {
                let last = self.last_master_frames.load(Relaxed);
                if last > 0 {
                    self.clock.fetch_add(last as Frame, AcqRel);
                    self.blocks.fetch_add(1, AcqRel);
                }
                self.last_master_frames.store(frames as i32, Relaxed);
            }
        }
        let clk = self.clock.load(Acquire);
        if a.last_clock != clk {
            if a.last_clock >= 0 && a.consumed > 0 {
                let el = clk - a.last_clock;
                if el > 0 && el < 4 * a.consumed as Frame {
                    if a.consumed as Frame > el {
                        ps.over_pulls.fetch_add(1, Relaxed);
                    } else if (a.consumed as Frame) < el {
                        ps.under_pulls.fetch_add(1, Relaxed);
                    }
                }
            }
            a.last_clock = clk;
            a.consumed = 0;
        }
        let block_start = clk + a.consumed as Frame;
        a.consumed += frames as i32;

        // --- commandes
        drain(&mut a, ps, block_start, srate);

        out.fill(0.0);
        let a = &mut *a;

        // --- phase 1 : chaque voix vivante, EXACTEMENT une fois
        for k in 0..a.vcount {
            let v = &mut a.voices[a.vlist[k] as usize];
            if v.state == state::IDLE {
                continue;
            }
            v.render(out, frames, nch, block_start, srate);
        }

        // --- phase 2 : enchainements, publication, emplacements rendus
        let mut k = 0;
        while k < a.vcount {
            let i = a.vlist[k] as usize;
            let (ended, ni, xf) = {
                let v = &mut a.voices[i];
                let r = (v.ended_at, v.next_voice, v.xfade_len);
                if v.ended_at >= 0 {
                    v.next_voice = -1;
                    v.ended_at = -1;
                }
                r
            };
            if ended >= 0 && ni >= 0 && (ni as usize) < MAX_PORT_VOICES {
                let nv = &mut a.voices[ni as usize];
                if nv.port == port as i32
                    && nv.clip.is_some()
                    && !nv.free_pending
                    && (nv.state == state::SCHEDULED || nv.state == state::IDLE)
                {
                    // xfade_len avance le depart de la suivante (un vrai fondu
                    // croise, ou la sortante continue, n'est pas fait).
                    nv.start_at = ended - xf as Frame;
                    if nv.start_at < block_start {
                        nv.start_at = block_start;
                    }
                    nv.state = state::SCHEDULED;
                    nv.ended_at = -1;
                    nv.render(out, frames, nch, block_start, srate);
                }
            }

            let v = &mut a.voices[i];
            v.publish(&ps.pubs[i]);

            // L'emplacement n'est rendu QUE lorsque la voix s'est tue.
            if v.free_pending && v.state == state::IDLE {
                v.port = -1;
                v.clip = None;
                // L'etireur retourne au vivier quoi qu'il arrive (le fil
                // principal le tient encore : jamais une liberation ici).
                v.shifter = None;
                v.free_pending = false;
                v.publish(&ps.pubs[i]);
                a.vlist[k] = a.vlist[a.vcount - 1];
                a.vcount -= 1;
                ps.claims[i].fetch_and(!CLAIM_OWNED, Release);
                continue;
            }
            k += 1;
        }

        // --- gain du port
        if a.gain != a.gain_target || a.gain != 1.0 {
            let mut g = a.gain;
            let step = (a.gain_target - a.gain) / frames as f32;
            for f in out.chunks_exact_mut(nch) {
                g += step;
                for s in f {
                    *s *= g;
                }
            }
            a.gain = a.gain_target;
        }

        Some(block_start)
    }
}

/// Une voix programmee s'ANNULE, elle ne s'eteint pas : sinon elle rendait 5 ms
/// du debut de sa matiere en fondu de sortie (un bip a chaque stop).
fn stop_now(v: &mut Voice, fade_frames: i32) {
    if v.state == state::SCHEDULED {
        v.state = state::IDLE;
        v.clip = None;
        v.shifter = None;
        v.tail_len = 0;
        return;
    }
    // Deja en fondu : on garde celui qui court.
    if v.state != state::PLAYING {
        return;
    }
    v.state = state::STOPPING;
    v.stop_at = Frame::MAX;
    v.fade_out_len = fade_frames;
    v.fade_out_pos = 0;
}

fn clamp1(c: f64) -> f64 {
    c.clamp(-1.0, 1.0)
}

fn drain(a: &mut PortAudio, ps: &PortSlot, block_start: Frame, srate: f64) {
    // Borne dure : jamais un drainage sans fin dans le fil audio.
    let mut guard = CMD_RING_CAP;
    while guard > 0 {
        let Some(c) = a.cmds.pop() else { break };
        apply(a, ps, c, block_start, srate);
        guard -= 1;
    }
}

fn apply(a: &mut PortAudio, ps: &PortSlot, c: Cmd, block_start: Frame, srate: f64) {
    let five_ms = (0.005 * srate) as i32;
    match c {
        Cmd::Panic => {
            for k in 0..a.vcount {
                let v = &mut a.voices[a.vlist[k] as usize];
                if v.state != state::IDLE {
                    stop_now(v, five_ms);
                }
            }
        }
        Cmd::PortGain(g) => a.gain_target = g,
        Cmd::Alloc { local, gen_ } => {
            let port = a.port as i32;
            let v = &mut a.voices[local];
            v.reset();
            v.port = port;
            v.gen_ = gen_;
            v.publish(&ps.pubs[local]);
            if !a.vlist[..a.vcount].contains(&(local as i16)) && a.vcount < MAX_PORT_VOICES {
                a.vlist[a.vcount] = local as i16;
                a.vcount += 1;
            }
        }
        Cmd::Free {
            local,
            gen_,
            new_gen,
            fade_s,
        } => {
            let v = &mut a.voices[local];
            if v.gen_ != gen_ {
                return;
            }
            v.gen_ = new_gen;
            v.free_pending = true;
            v.next_voice = -1;
            stop_now(v, (fade_s * srate) as i32);
        }
        Cmd::Voice {
            local,
            gen_,
            at,
            op,
        } => {
            if a.voices[local].gen_ != gen_ {
                return; // handle perime
            }
            let at_f = match at {
                At::Now => block_start,
                At::Frame(f) => f,
            };
            // La generation d'une voix CIBLE (enchainement), lue avant
            // d'emprunter la voix elle-meme.
            let target_gen = match &op {
                VoiceOp::Queue { next, .. } if next.port() == a.port => {
                    Some(a.voices[next.local()].gen_)
                }
                _ => None,
            };
            let xf = (0.005 * srate) as i32;
            let v = &mut a.voices[local];
            match op {
                VoiceOp::Play {
                    clip,
                    rate,
                    gain,
                    mode,
                } => {
                    // Un lancement sur une voix qui sonne n'est pas une coupure
                    // seche : meme clip, l'ancienne tete croise avec la
                    // nouvelle ; autre clip, un fondu d'entree minimal.
                    let live = v.state == state::PLAYING || v.state == state::STOPPING;
                    let same = v.clip.as_ref().is_some_and(|c| Arc::ptr_eq(c, &clip));
                    if live && same {
                        v.tail_pos = v.pos;
                        v.tail_len = xf;
                        v.tail_i = 0;
                        v.fade_in_cur = v.fade_in_len;
                    } else {
                        v.tail_len = 0;
                        v.fade_in_cur = if live && v.fade_in_len < xf {
                            xf
                        } else {
                            v.fade_in_len
                        };
                    }
                    v.clip = Some(clip);
                    v.mode = mode;
                    v.rate = if rate > 0.0 { rate } else { 1.0 };
                    v.gain = gain as f32;
                    v.gain_target = gain as f32;
                    v.pos = v.loop_start as f64;
                    v.sh_fresh = true;
                    v.start_at = at_f;
                    v.stop_at = Frame::MAX;
                    v.ended_at = -1;
                    v.started_at = -1;
                    v.fade_in_pos = 0;
                    v.fade_out_pos = 0;
                    v.env_st = 0;
                    v.env_x = 0.0;
                    v.env_lv = 0.0;
                    v.envp_i = 0;
                    v.state = state::SCHEDULED;
                }
                VoiceOp::EnvPt { idx, t, g, curve } => v.env_point(idx, t, g, curve as f64),
                VoiceOp::Stop { fade_s } => {
                    let fade = (fade_s * srate) as i32;
                    if at == At::Now && fade > 0 {
                        stop_now(v, fade);
                    } else {
                        // Arret DATE : il tombe exactement sur le frame demande.
                        // Une voix programmee apres sa propre coupure s'annule.
                        if v.state == state::SCHEDULED && at_f <= v.start_at {
                            v.state = state::IDLE;
                            v.clip = None;
                            return;
                        }
                        v.stop_at = at_f;
                        v.fade_out_len = fade;
                    }
                }
                VoiceOp::Set { param, value: x } => set_param(v, param, x, srate, xf),
                // L'etireur prete : il repart a zero au prochain bloc. (Celui
                // qu'il remplace n'est jamais lache ici le dernier : le vivier
                // du fil principal le tient.)
                VoiceOp::Shifter(s) => {
                    v.shifter = s;
                    v.sh_fresh = true;
                }
                VoiceOp::Queue { next, xfade_s } => {
                    if target_gen == Some(next.generation()) {
                        v.next_voice = next.local() as i32;
                        v.xfade_len = (xfade_s * srate) as i32;
                    }
                }
            }
        }
    }
}

fn set_param(v: &mut Voice, param: Param, x: f64, srate: f64, xf: i32) {
    match param {
        Param::Rate => {
            // Une vitesse infinie menait a une position NaN, puis a du NaN
            // dans la sortie de REAPER.
            if x > 0.0 && x.is_finite() {
                v.rate = x;
            }
        }
        Param::Gain => v.gain_target = x as f32,
        Param::Pan => v.pan = x as f32,
        Param::LoopStart => v.loop_start = x as Frame,
        Param::LoopEnd => v.loop_end = x as Frame,
        Param::FadeIn => {
            v.fade_in_len = (x * srate) as i32;
            // Un fondu pose avant le depart vaut pour ce lancement-la aussi.
            if v.state == state::SCHEDULED {
                v.fade_in_cur = v.fade_in_len;
            }
        }
        Param::FadeOut => v.fade_out_len = (x * srate) as i32,
        // Sur une voix qui SONNE, un deplacement est un saut : l'ancienne tete
        // croise avec la nouvelle pendant 5 ms.
        Param::Pos => {
            let np = if x > 0.0 { x } else { 0.0 };
            if v.state == state::PLAYING || v.state == state::STOPPING {
                v.tail_pos = v.pos;
                v.tail_len = xf;
                v.tail_i = 0;
            }
            v.pos = np;
            v.sh_fresh = true;
        }
        Param::Loop => {
            v.mode = if x != 0.0 {
                PlayMode::Loop
            } else {
                PlayMode::Once
            }
        }
        Param::Env => {
            v.env_on = x != 0.0;
            v.envp_on = false;
        }
        Param::EnvPts => {
            v.env_commit((x + 0.5) as i32);
        }
        Param::Interp => v.interp = if x >= 0.5 { 1 } else { 0 },
        Param::Tempo => v.tempo = if x > 0.01 { x.min(20.0) } else { 1.0 },
        Param::LoopXfade => v.loop_xf = x.clamp(0.0, 10.0),
        Param::Shift => v.pshift = if x > 0.01 { x.min(20.0) } else { 1.0 },
        Param::Reverse => {
            let rev = x != 0.0;
            // Retourne en jouant, une voix etiree repart de zero dans
            // l'etireur (sinon sa reserve joue encore dans l'ancien sens).
            if rev != v.reverse
                && v.shifter.is_some()
                && (v.state == state::PLAYING || v.state == state::STOPPING)
            {
                v.sh_fresh = true;
            }
            v.reverse = rev;
            // Pose avant le depart : de la fin de la region.
            v.rev_arm = v.reverse && v.state == state::SCHEDULED;
        }
        Param::Attack => v.env_a = (if x > 0.0 { x } else { 0.0 }) as f32,
        Param::Decay => v.env_d = (if x > 0.0 { x } else { 0.0 }) as f32,
        Param::Sustain => v.env_s = x.clamp(0.0, 1.0) as f32,
        Param::Release => v.env_r = (if x > 0.0 { x } else { 0.0 }) as f32,
        Param::ACurve => v.env_ka = 8f64.powf(-clamp1(x)) as f32,
        Param::DCurve => v.env_kd = 8f64.powf(clamp1(x)) as f32,
        Param::RCurve => v.env_kr = 8f64.powf(clamp1(x)) as f32,
        // Relacher : depuis le niveau courant, en niveau x R secondes. Une voix
        // pas encore partie s'annule ; sans enveloppe, un arret doux de 5 ms.
        Param::Gate => {
            if x != 0.0 {
                return;
            }
            if !v.env_on {
                stop_now(v, (0.005 * srate) as i32);
                return;
            }
            if v.state == state::SCHEDULED {
                stop_now(v, 0);
                return;
            }
            if v.state != state::PLAYING || v.env_st == 3 {
                return;
            }
            v.env_st = 3;
            v.env_rl0 = v.env_lv;
            v.env_x = 0.0;
            let rl = v.env_rl0 as f64 * v.env_r as f64 * srate;
            v.env_rxi = 1.0 / if rl > 1.0 { rl } else { 1.0 };
        }
    }
}
