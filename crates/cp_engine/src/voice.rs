//! Une voix : un clip, une position, un rendez-vous (ancien `cp_voice`).
//!
//! La voix est la SEULE chose que le moteur connait : pas de scene, pas de
//! colonne, pas de case. Une politique de session se decrit avec « joue au frame
//! N », « arrete au frame M » et « enchaine sur la voix H ».
//!
//! Elle appartient au FIL AUDIO, integralement. Le fil principal lit ce qu'elle
//! publie en fin de bloc (`VoicePub`), avec au plus un bloc de retard.
//!
//! Le rendu suit l'ancien C++ operation par operation, dans le meme ordre et
//! avec les memes types (f32 / f64) : c'est ce qui permet a `cp_oracle` de
//! comparer les deux moteurs bit pour bit. Ne pas « simplifier » une expression
//! sans relancer l'oracle.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering::Relaxed};

use crate::curve;
use crate::pool::ClipData;
use crate::shift::{FEED, ShiftSlot};
use crate::sinc;

/// Le plus grand morceau qu'une voix etiree rend d'un coup (les ports
/// rendent par 512).
const MAX_SH: usize = 2048;
use crate::types::{Frame, MAX_ENV_PTS, PlayMode, state};

/// Sous ce gain, on force zero. Les denormaux coutent 100 a 400 cycles sur un
/// Athlon 64 ou un P4 : on les tue a la source plutot que de toucher au MXCSR du
/// fil audio de REAPER.
const DENORMAL_FLOOR: f32 = 1e-15;

/// Un point d'enveloppe dessinee, date en temps SOURCE.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct EnvPt {
    /// secondes source
    pub t: f32,
    /// gain lineaire 0..4
    pub g: f32,
    /// en attente : la courbe brute ; actif : le facteur de shp
    pub k: f32,
}

/// Ce que la voix publie pour le fil principal, une fois par bloc.
pub(crate) struct VoicePub {
    pub pos: AtomicU64,
    pub state: AtomicI32,
    pub started: AtomicI64,
    pub clip: AtomicI32,
}

impl VoicePub {
    pub fn new() -> VoicePub {
        VoicePub {
            pos: AtomicU64::new(0f64.to_bits()),
            state: AtomicI32::new(state::IDLE),
            started: AtomicI64::new(-1),
            clip: AtomicI32::new(-1),
        }
    }
    pub fn pos(&self) -> f64 {
        f64::from_bits(self.pos.load(Relaxed))
    }
}

pub(crate) struct Voice {
    // --- identite
    pub state: i32,
    pub gen_: u16,
    pub port: i32,
    // --- matiere
    pub clip: Option<Arc<ClipData>>,
    pub pos: f64,
    // --- rendez-vous
    pub start_at: Frame,
    pub stop_at: Frame,
    pub mode: PlayMode,
    // --- transformation
    pub rate: f64,
    pub gain: f32,
    pub gain_target: f32,
    pub pan: f32,
    pub loop_start: Frame,
    pub loop_end: Frame,
    /// Le fondu enchaine au point de boucle (secondes source).
    pub loop_xf: f64,
    // --- fondus
    pub fade_in_len: i32,
    pub fade_in_cur: i32,
    pub fade_in_pos: i32,
    pub fade_out_len: i32,
    pub fade_out_pos: i32,
    // --- ADSR : les regles de l'instrument du kit, etage par etage
    pub env_on: bool,
    pub env_a: f32,
    pub env_d: f32,
    pub env_s: f32,
    pub env_r: f32,
    pub env_ka: f32,
    pub env_kd: f32,
    pub env_kr: f32,
    pub env_st: i32,
    pub env_x: f64,
    pub env_lv: f32,
    pub env_rl0: f32,
    pub env_rxi: f64,
    // --- enveloppe A POINTS : deux tableaux, le rendu lit l'un, les commandes
    // remplissent l'autre ; une validation les echange.
    envp: Box<[[EnvPt; MAX_ENV_PTS]; 2]>,
    pub envp_on: bool,
    pub envp_n: i32,
    pub envp_act: usize,
    pub envp_rx: i32,
    pub envp_i: i32,
    // --- croisement sur saut de tete
    pub tail_pos: f64,
    pub tail_len: i32,
    pub tail_i: i32,
    // --- enchainement
    pub next_voice: i32,
    pub xfade_len: i32,
    pub started_at: Frame,
    pub ended_at: Frame,
    pub free_pending: bool,
    // --- lecture
    /// 0 = Hermite, 1 = sinc.
    pub interp: u8,
    /// La tete recule.
    pub reverse: bool,
    /// A l'envers depuis le depart : la tete passe a la fin de la region au
    /// premier bloc joue.
    pub rev_arm: bool,
    // --- etirement en temps reel
    /// L'etireur prete par le fil principal (jamais le dernier a le tenir).
    pub shifter: Option<ShiftSlot>,
    pub tempo: f64,
    pub pshift: f64,
    /// Un nouveau depart (lancement, saut) : l'etireur repart a zero.
    pub sh_fresh: bool,
}

impl Voice {
    pub fn new() -> Voice {
        let mut v = Voice {
            state: state::IDLE,
            gen_: 1,
            port: -1,
            clip: None,
            pos: 0.0,
            start_at: 0,
            stop_at: Frame::MAX,
            mode: PlayMode::Once,
            rate: 1.0,
            gain: 1.0,
            gain_target: 1.0,
            pan: 0.0,
            loop_start: 0,
            loop_end: 0,
            fade_in_len: 0,
            fade_in_cur: 0,
            fade_in_pos: 0,
            fade_out_len: 0,
            fade_out_pos: 0,
            env_on: false,
            env_a: 0.0,
            env_d: 0.0,
            env_s: 1.0,
            env_r: 0.0,
            env_ka: 1.0,
            env_kd: 1.0,
            env_kr: 1.0,
            env_st: 0,
            env_x: 0.0,
            env_lv: 0.0,
            env_rl0: 0.0,
            env_rxi: 1.0,
            envp: Box::new([[EnvPt::default(); MAX_ENV_PTS]; 2]),
            envp_on: false,
            envp_n: 0,
            envp_act: 0,
            envp_rx: 0,
            envp_i: 0,
            tail_pos: 0.0,
            tail_len: 0,
            tail_i: 0,
            next_voice: -1,
            xfade_len: 0,
            started_at: -1,
            ended_at: -1,
            free_pending: false,
            interp: 0,
            reverse: false,
            rev_arm: false,
            shifter: None,
            tempo: 1.0,
            pshift: 1.0,
            sh_fresh: true,
            loop_xf: 0.0,
        };
        v.reset();
        v
    }

    /// Remet tout a zero sauf la generation et les tableaux de points (ancien
    /// `Voice::reset`). Ne publie pas : l'appelant le fait (il tient la table).
    pub fn reset(&mut self) {
        self.interp = 0;
        self.reverse = false;
        self.rev_arm = false;
        self.shifter = None;
        self.tempo = 1.0;
        self.pshift = 1.0;
        self.sh_fresh = true;
        self.loop_xf = 0.0;
        self.state = state::IDLE;
        self.port = -1;
        self.clip = None;
        self.pos = 0.0;
        self.start_at = 0;
        self.stop_at = Frame::MAX;
        self.mode = PlayMode::Once;
        self.rate = 1.0;
        self.gain = 1.0;
        self.gain_target = 1.0;
        self.pan = 0.0;
        self.loop_start = 0;
        self.loop_end = 0;
        self.fade_in_len = 0;
        self.fade_in_cur = 0;
        self.fade_in_pos = 0;
        self.fade_out_len = 0;
        self.fade_out_pos = 0;
        self.env_on = false;
        self.env_a = 0.0;
        self.env_d = 0.0;
        self.env_s = 1.0;
        self.env_r = 0.0;
        self.env_ka = 1.0;
        self.env_kd = 1.0;
        self.env_kr = 1.0;
        self.env_st = 0;
        self.env_x = 0.0;
        self.env_lv = 0.0;
        self.env_rl0 = 0.0;
        self.env_rxi = 1.0;
        self.envp_on = false;
        self.envp_n = 0;
        self.envp_act = 0;
        self.envp_rx = 0;
        self.envp_i = 0;
        self.tail_pos = 0.0;
        self.tail_len = 0;
        self.tail_i = 0;
        self.next_voice = -1;
        self.xfade_len = 0;
        self.started_at = -1;
        self.ended_at = -1;
        self.free_pending = false;
    }

    pub fn publish(&self, p: &VoicePub) {
        p.pos.store(self.pos.to_bits(), Relaxed);
        p.state.store(self.state, Relaxed);
        p.started.store(self.started_at, Relaxed);
        let c = if self.state == state::IDLE {
            -1
        } else {
            self.clip.as_ref().map_or(-1, |c| c.id())
        };
        p.clip.store(c, Relaxed);
    }

    /// Un point dans le tableau EN ATTENTE : rien ne s'entend avant `env_commit`.
    pub fn env_point(&mut self, idx: i32, t: f64, g: f64, c: f64) {
        if idx < 0 || idx as usize >= MAX_ENV_PTS {
            return;
        }
        let p = &mut self.envp[self.envp_act ^ 1][idx as usize];
        p.t = (if t > 0.0 { t } else { 0.0 }) as f32;
        // `clamp` = les ternaires du C++, NaN compris (NaN reste NaN).
        p.g = g.clamp(0.0, 4.0) as f32;
        p.k = c.clamp(-1.0, 1.0) as f32; // brute jusqu'a la validation
        self.envp_rx += 1;
    }

    /// Valide le lot. Un lot incomplet ou desordonne est REFUSE : la voix garde
    /// l'enveloppe d'avant plutot que de jouer une forme a moitie ecrite.
    pub fn env_commit(&mut self, n: i32) -> bool {
        let rx = self.envp_rx;
        self.envp_rx = 0;
        if n <= 0 {
            self.envp_on = false;
            self.envp_n = 0;
            return true;
        }
        if n as usize > MAX_ENV_PTS || rx != n {
            return false;
        }
        let n = n as usize;
        let s = &mut self.envp[self.envp_act ^ 1];
        for i in 1..n {
            if s[i].t < s[i - 1].t {
                return false;
            }
        }
        for i in 0..n {
            s[i].k = if i + 1 < n {
                curve::factor(s[i].k, s[i + 1].g >= s[i].g)
            } else {
                1.0
            };
        }
        self.envp_act ^= 1;
        self.envp_n = n as i32;
        self.envp_i = 0;
        self.envp_on = true;
        self.env_on = false;
        true
    }

    /// Rend et ADDITIONNE dans `out` (entrelace, `nch` canaux, `frames`
    /// images). `block_start` = frame absolu du premier echantillon du tampon.
    pub fn render(
        &mut self,
        out: &mut [f32],
        frames: usize,
        nch: usize,
        block_start: Frame,
        engine_srate: f64,
    ) {
        if self.state == state::IDLE {
            return;
        }
        let Some(c) = self.clip.clone() else {
            self.die();
            return;
        };
        if c.frames() <= 0 {
            self.die();
            return;
        }
        let frames_i = frames as Frame;

        // Ou commence-t-on dans ce bloc ? Le rendez-vous n'est pas arrondi au
        // bloc : il devient un decalage a l'interieur du bloc.
        let mut off: i32 = 0;
        if self.state == state::SCHEDULED {
            if self.start_at >= block_start + frames_i {
                return;
            }
            off = if self.start_at > block_start {
                (self.start_at - block_start) as i32
            } else {
                0
            };
            self.state = state::PLAYING;
            self.started_at = block_start + off as Frame;
            self.fade_in_pos = 0;
        }

        // Coupure datee, elle aussi dans le bloc.
        let mut n: i32 = frames as i32;
        if self.stop_at < block_start + frames_i {
            let rel = self.stop_at - block_start;
            n = if rel < 0 { 0 } else { rel as i32 };
            if n < off {
                n = off;
            }
        }

        // Un arret DATE avec fondu : le fondu COMMENCE assez tot pour toucher
        // zero exactement au rendez-vous.
        if self.state == state::PLAYING && self.fade_out_len > 0 {
            let to_stop = self.stop_at.saturating_sub(block_start);
            if to_stop <= self.fade_out_len as Frame {
                self.state = state::STOPPING;
                let done = self.fade_out_len as Frame - if to_stop > 0 { to_stop } else { 0 };
                self.fade_out_pos = if done > 0 { done as i32 } else { 0 };
            }
        }
        if n <= off {
            if self.stop_at <= block_start {
                self.state = state::IDLE;
                self.clip = None;
                self.ended_at = self.stop_at;
            }
            return;
        }

        let cf = c.frames();
        let src_end: Frame = if self.loop_end > 0 && self.loop_end <= cf {
            self.loop_end
        } else {
            cf
        };
        let src_start: Frame = if self.loop_start >= 0 && self.loop_start < src_end {
            self.loop_start
        } else {
            0
        };
        let span = src_end - src_start;
        let cnch = c.nch();
        let data = c.samples();

        // Balance, pas panoramique a puissance constante : au centre, la matiere
        // passe a l'identique.
        let pnorm = self.pan.clamp(-1.0, 1.0);
        let gl: f32 = if pnorm > 0.0 { 1.0 - pnorm } else { 1.0 };
        let gr: f32 = if pnorm < 0.0 { 1.0 + pnorm } else { 1.0 };

        // La matiere garde son taux ; c'est la LECTURE qui s'adapte.
        let sr_ratio = if engine_srate > 1.0 && c.srate() > 1.0 {
            c.srate() / engine_srate
        } else {
            1.0
        };
        let step = self.rate * sr_ratio;
        let unity = step > 0.99999999 && step < 1.00000001;
        let looping = self.mode == PlayMode::Loop && span > 0;

        // --- l'etat mutable descend en local
        let mut p = self.pos;
        // A l'envers depuis le depart : la tete part de la fin de la region,
        // a la place symetrique de son depart.
        if self.rev_arm && span > 0 {
            p = (src_start + src_end - 1) as f64 - (p - src_start as f64);
            p = p.clamp(src_start as f64, (src_end - 1) as f64);
            self.rev_arm = false;
        }
        let rev = self.reverse;
        let dir = if rev { -step } else { step };
        // Le sinc (choisi, et sa table construite) hors vitesse 1.
        let hq = if self.interp == 1 && !unity {
            sinc::kernel()
        } else {
            None
        };
        let avail = (data.len() / cnch.max(1)) as Frame;
        let tap = |m: Frame| -> (f32, f32) {
            let m = if looping {
                src_start + (m - src_start).rem_euclid(span)
            } else if m < src_start || m >= src_end {
                return (0.0, 0.0);
            } else {
                m
            };
            if m < 0 || m >= avail {
                return (0.0, 0.0);
            }
            let s = m as usize * cnch;
            let l = data[s];
            (l, if cnch > 1 { data[s + 1] } else { l })
        };
        // Le fondu enchaine au point de boucle : sa longueur en frames
        // source (bornee par la matiere avant la boucle et par sa moitie).
        let xf = if looping && self.loop_xf > 0.0 {
            ((self.loop_xf * c.srate().max(1.0)) as Frame)
                .min(src_start)
                .min(span / 2)
                .max(0)
        } else {
            0
        };
        // La matiere hors de la region (le partenaire du fondu, avant le
        // debut de la boucle), lue sur tout le clip.
        let tap_all = |m: Frame| -> (f32, f32) {
            if m < 0 || m >= avail.min(cf) {
                return (0.0, 0.0);
            }
            let s = m as usize * cnch;
            let l = data[s];
            (l, if cnch > 1 { data[s + 1] } else { l })
        };
        let read_plain = |q: f64| -> (f32, f32) {
            match hq {
                Some(k) => k.read(q, step, tap),
                None => read_at(data, cnch, cf, q, unity, looping, src_start, src_end),
            }
        };
        // Avec un fondu au point de boucle, la matiere se lit sans
        // repliement (le fondu fait la couture) : la vraie matiere de part et
        // d'autre de la boucle.
        let read_free = |q: f64| -> (f32, f32) {
            match hq {
                Some(k) => k.read(q, step, tap_all),
                None if unity => tap_all(q.floor() as Frame),
                None => {
                    let i0 = q.floor() as Frame;
                    let ft = (q - i0 as f64) as f32;
                    let (a0, a1, a2, a3) = (
                        tap_all(i0 - 1),
                        tap_all(i0),
                        tap_all(i0 + 1),
                        tap_all(i0 + 2),
                    );
                    (
                        hermite(a0.0, a1.0, a2.0, a3.0, ft),
                        hermite(a0.1, a1.1, a2.1, a3.1, ft),
                    )
                }
            }
        };
        let read = |q: f64| -> (f32, f32) {
            if xf <= 0 {
                return read_plain(q);
            }
            let (l, r) = read_free(q);
            if q < (src_end - xf) as f64 || q >= src_end as f64 {
                return (l, r);
            }
            // Dans les `xf` dernieres frames : la fin descend, la matiere
            // d'avant la boucle monte (a puissance constante).
            let t = ((q - (src_end - xf) as f64) / xf as f64) as f32;
            let a = (t * std::f32::consts::FRAC_PI_2).cos();
            let b = (t * std::f32::consts::FRAC_PI_2).sin();
            let (pl, pr) = read_free(q - span as f64);
            (l * a + pl * b, r * a + pr * b)
        };
        // --- l'etirement : le bloc entier sort d'abord de l'etireur, nourri
        // de matiere lue a la tete (meme boucle, meme sens, memes bornes).
        let stretching = self.shifter.is_some() && (self.tempo != 1.0 || self.pshift != 1.0);
        let mut sbuf = [0f32; MAX_SH * 2];
        let mut s_frames = 0usize;
        let mut s_ended = false;
        if stretching && n > off {
            let want = ((n - off) as usize).min(MAX_SH);
            if let Some(slot) = self.shifter.clone() {
                if let Ok(mut sh) = slot.try_lock() {
                    if self.sh_fresh {
                        sh.restart();
                        self.sh_fresh = false;
                    }
                    sh.tune(self.tempo, self.pshift);
                    let mut src_done = sh.flushed;
                    let got = sh.produce(&mut sbuf, want, |buf| {
                        let mut k = 0;
                        while k < FEED && !src_done {
                            if rev && p < src_start as f64 {
                                if looping && span > 0 {
                                    p += span as f64;
                                    if p < src_start as f64 {
                                        let d = p - src_start as f64;
                                        p = src_start as f64
                                            + (d - (d / span as f64).floor() * span as f64);
                                    }
                                } else {
                                    src_done = true;
                                    break;
                                }
                            }
                            if p >= src_end as f64 {
                                if looping && span > 0 {
                                    p -= span as f64;
                                    if p >= src_end as f64 {
                                        let d = p - src_start as f64;
                                        p = src_start as f64
                                            + (d - (d / span as f64).floor() * span as f64);
                                    }
                                } else {
                                    src_done = true;
                                    break;
                                }
                            }
                            let (l, r) = read(p);
                            buf[k * 2] = l as f64;
                            buf[k * 2 + 1] = r as f64;
                            p += dir;
                            k += 1;
                        }
                        k
                    });
                    s_frames = got;
                    s_ended = got < want && sh.flushed;
                }
            }
        }
        let mut st = self.state;
        let mut fip = self.fade_in_pos;
        let mut fop = self.fade_out_pos;
        let mut g = self.gain;
        let fil = self.fade_in_cur;
        let fol = self.fade_out_len;
        let mut tp = self.tail_pos;
        let mut ti = self.tail_i;
        let tl = self.tail_len;

        let gstep: f32 = if n > off {
            (self.gain_target - g) / (n - off) as f32
        } else {
            0.0
        };

        let eon = self.env_on;
        let mut est = self.env_st;
        let mut ex = self.env_x;
        let mut elv = self.env_lv;
        let ainc = if eon {
            let a = self.env_a as f64 * engine_srate;
            1.0 / if a > 1.0 { a } else { 1.0 }
        } else {
            0.0
        };
        let dinc = if eon {
            let d = self.env_d as f64 * engine_srate;
            1.0 / if d > 1.0 { d } else { 1.0 }
        } else {
            0.0
        };
        let rxi = self.env_rxi;
        let (esus, eka, ekd, ekr, rl0) = (
            self.env_s,
            self.env_ka,
            self.env_kd,
            self.env_kr,
            self.env_rl0,
        );
        let mut relk = 0.0f64;
        if eon && !looping {
            let rl = self.env_r as f64 * engine_srate * step;
            relk = 1.0 / if rl > 1.0 { rl } else { 1.0 };
        }
        let pon = self.envp_on && self.envp_n > 0;
        let ept: &[EnvPt] = &self.envp[self.envp_act][..self.envp_n.max(0) as usize];
        let mut epi = self.envp_i;
        let inv_csr = if c.srate() > 1.0 {
            1.0 / c.srate()
        } else {
            1.0 / if engine_srate > 1.0 {
                engine_srate
            } else {
                48000.0
            }
        };

        let mut ended_here: Frame = -1;

        let mut i = off;
        while i < n {
            // --- la voix etiree : le bloc deja rendu par l'etireur
            let (mut l, mut r);
            if stretching {
                let k = (i - off) as usize;
                if k >= s_frames {
                    if s_ended {
                        st = state::IDLE;
                        ended_here = block_start + i as Frame;
                        break;
                    }
                    // L'etireur occupe ou en retard : un silence, pas une
                    // attente.
                    (l, r) = (0.0, 0.0);
                } else {
                    (l, r) = (sbuf[k * 2], sbuf[k * 2 + 1]);
                }
            } else {
                // --- debut de matiere (a l'envers)
                if rev && p < src_start as f64 {
                    if looping {
                        p += span as f64;
                        if p < src_start as f64 {
                            let d = p - src_start as f64;
                            p = src_start as f64 + (d - (d / span as f64).floor() * span as f64);
                        }
                    } else {
                        st = state::IDLE;
                        ended_here = block_start + i as Frame;
                        break;
                    }
                }
                // --- fin de matiere
                if p >= src_end as f64 {
                    if looping {
                        // Une soustraction, puis une reduction en O(1) : jamais une
                        // boucle dont le nombre de tours depend d'une position.
                        p -= span as f64;
                        if p >= src_end as f64 {
                            let d = p - src_start as f64;
                            p = src_start as f64 + (d - (d / span as f64).floor() * span as f64);
                            if !(p >= src_start as f64 && p < src_end as f64) {
                                st = state::IDLE;
                                ended_here = block_start + i as Frame;
                                break;
                            }
                        }
                    } else {
                        st = state::IDLE;
                        ended_here = block_start + i as Frame;
                        break;
                    }
                }

                // --- lecture interpolee
                (l, r) = read(p);
            }
            if pon {
                let ge = envp_eval(ept, (p * inv_csr) as f32, &mut epi);
                l *= ge;
                r *= ge;
            }

            // --- croisement avec l'ancienne tete
            if ti < tl && !stretching {
                let (mut lo, mut ro) = (0.0f32, 0.0f32);
                if tp >= src_end as f64 && looping {
                    tp -= span as f64;
                }
                if tp < src_start as f64 && looping && rev {
                    tp += span as f64;
                }
                if tp >= src_start as f64 && tp < src_end as f64 {
                    (lo, ro) = read(tp);
                }
                if pon {
                    let mut tc = epi;
                    let gt = envp_eval(ept, (tp * inv_csr) as f32, &mut tc);
                    lo *= gt;
                    ro *= gt;
                }
                let w = ti as f32 / tl as f32;
                l = l * w + lo * (1.0 - w);
                r = r * w + ro * (1.0 - w);
                tp += dir;
                ti += 1;
            }

            // --- fondus
            let mut env: f32 = 1.0;
            if fil > 0 && fip < fil {
                env *= fip as f32 / fil as f32;
                fip += 1;
            }
            if st == state::STOPPING {
                if fol > 0 && fop < fol {
                    env *= 1.0 - fop as f32 / fol as f32;
                    fop += 1;
                } else {
                    st = state::IDLE;
                    ended_here = block_start + i as Frame;
                    break;
                }
            }
            // --- ADSR : l'etage avance AVANT la lecture, comme dans le JSFX
            if eon {
                if est == 0 {
                    ex += ainc;
                    if ex >= 1.0 {
                        elv = 1.0;
                        ex = 0.0;
                        est = 1;
                    } else {
                        elv = curve::shp(ex as f32, eka);
                    }
                } else if est == 1 {
                    ex += dinc;
                    if ex >= 1.0 {
                        elv = esus;
                        ex = 0.0;
                        est = 2;
                    } else {
                        elv = 1.0 - (1.0 - esus) * curve::shp(ex as f32, ekd);
                    }
                } else if est == 3 {
                    ex += rxi;
                    if ex >= 1.0 {
                        elv = 0.0;
                        st = state::IDLE;
                        ended_here = block_start + i as Frame;
                        break;
                    }
                    elv = rl0 * (1.0 - curve::shp(ex as f32, ekr));
                }
                env *= elv;
                if relk > 0.0 {
                    let left = if rev {
                        p - src_start as f64
                    } else {
                        src_end as f64 - 1.0 - p
                    };
                    let rk = left * relk;
                    if rk < 1.0 {
                        env *=
                            1.0 - curve::shp((1.0 - if rk > 0.0 { rk } else { 0.0 }) as f32, ekr);
                    }
                }
            }

            g += gstep;
            let mut gg = g * env;
            if gg < DENORMAL_FLOOR && gg > -DENORMAL_FLOOR {
                gg = 0.0;
            }

            let o = i as usize * nch;
            out[o] += l * gg * gl;
            if nch > 1 {
                out[o + 1] += r * gg * gr;
            }

            if !stretching {
                p += dir;
            }
            i += 1;
        }

        // --- l'etat remonte, une seule fois
        self.pos = p;
        self.fade_in_pos = fip;
        self.fade_out_pos = fop;
        self.env_st = est;
        self.env_x = ex;
        self.env_lv = elv;
        self.envp_i = epi;
        if tl > 0 {
            if ti >= tl {
                self.tail_len = 0;
                self.tail_i = 0;
            } else {
                self.tail_pos = tp;
                self.tail_i = ti;
            }
        }
        self.state = st;
        self.gain = self.gain_target;
        if ended_here >= 0 {
            self.clip = None;
            self.ended_at = ended_here;
        }
        // Finie : l'etireur retourne au vivier (le fil principal le tient
        // encore : ce n'est jamais une liberation).
        if self.state == state::IDLE {
            self.shifter = None;
        }

        // Coupure nette datee : la voix s'eteint exactement au frame demande.
        if self.stop_at < block_start + frames_i
            && self.state != state::IDLE
            && self.fade_out_len == 0
        {
            self.state = state::IDLE;
            self.clip = None;
            self.ended_at = self.stop_at;
        }
    }

    /// La matiere a disparu : la voix meurt ici (sinon son emplacement ne
    /// serait jamais rendu). Pas d'`ended_at` : un chemin d'erreur ne declenche
    /// pas d'enchainement.
    fn die(&mut self) {
        self.state = state::IDLE;
        self.clip = None;
        self.tail_len = 0;
    }
}

/// Hermite 4 points : contre le lineaire (audible sur les transitoires) et
/// contre un sinc 64 taps (hors budget sur la machine cible).
#[inline]
fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b = w + a;
    (((a * t) - b) * t + c) * t + x0
}

/// Une lecture a la position `p`. Les indices sont bornes au tampon : dans les
/// cas normaux c'est sans effet ; dans ceux ou l'ancien C++ lisait hors du
/// tampon (une position negative), on lit le bord au lieu de planter.
#[allow(clippy::too_many_arguments)]
#[inline]
fn read_at(
    data: &[f32],
    cnch: usize,
    frames: Frame,
    p: f64,
    unity: bool,
    looping: bool,
    src_start: Frame,
    src_end: Frame,
) -> (f32, f32) {
    let last = frames - 1;
    let i0 = p as Frame;
    let at = |i: Frame| i.clamp(0, last) as usize * cnch;
    // Un clip qui se remplit encore : au-dela du publie, le silence. (Un clip
    // complet ne passe jamais ici : le calcul reste celui de l'ancien C++.)
    let avail = (data.len() / cnch.max(1)) as Frame;
    let partial = avail < frames;
    if partial && i0.clamp(0, last) + if unity { 0 } else { 2 } >= avail {
        return (0.0, 0.0);
    }
    if unity {
        let s = at(i0);
        let l = data[s];
        return (l, if cnch > 1 { data[s + 1] } else { l });
    }
    let t = (p - i0 as f64) as f32;
    let mut im1 = i0 - 1;
    let mut i1 = i0 + 1;
    let mut i2 = i0 + 2;
    if im1 < src_start {
        im1 = if looping { src_end - 1 } else { src_start };
    }
    if i1 >= src_end {
        i1 = if looping { src_start } else { src_end - 1 };
    }
    if i2 >= src_end {
        i2 = if looping { src_start } else { src_end - 1 };
    }
    let (pm1, p0, p1, p2) = (at(im1), at(i0), at(i1), at(i2));
    let l = hermite(data[pm1], data[p0], data[p1], data[p2], t);
    let r = if cnch > 1 {
        hermite(data[pm1 + 1], data[p0 + 1], data[p1 + 1], data[p2 + 1], t)
    } else {
        l
    };
    (l, r)
}

/// L'enveloppe dessinee a l'instant source `t`. `cur` est le curseur du
/// segment : on n'avance en general que d'un cran ; les deux marches sont
/// bornees par le nombre de points.
#[inline]
fn envp_eval(e: &[EnvPt], t: f32, cur: &mut i32) -> f32 {
    let n = e.len() as i32;
    let mut i = *cur;
    if i < 0 || i >= n {
        i = 0;
    }
    if t < e[i as usize].t {
        while i > 0 && t < e[i as usize].t {
            i -= 1;
        }
    } else {
        while i + 1 < n && t >= e[(i + 1) as usize].t {
            i += 1;
        }
    }
    *cur = i;
    if t <= e[0].t {
        return e[0].g;
    }
    if i + 1 >= n {
        return e[(n - 1) as usize].g;
    }
    let (a, b) = (e[i as usize], e[(i + 1) as usize]);
    let dt = b.t - a.t;
    if dt <= 0.0 {
        return b.g;
    }
    let x = (t - a.t) / dt;
    a.g + (b.g - a.g) * curve::shp(x, a.k)
}
