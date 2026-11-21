//! cp_engine contre l'ancien moteur C++, echantillon par echantillon.
//!
//! Un JUMEAU envoie chaque commande aux deux moteurs, rend chaque bloc des deux
//! cotes, et exige des sorties IDENTIQUES AU BIT, la meme date de bloc, les
//! memes etats publies (position, etat, instant de depart) et les memes
//! comptes de voix. La moindre difference arrete le test et dit ou.
//!
//! Des scenarios ecrits pour chaque comportement, puis un fuzz : des centaines
//! de suites de commandes tirees au hasard (graines fixes, donc rejouables).

use std::collections::BTreeSet;
use std::sync::Arc;

use cp_engine::{At, ClipData, Ctl, Param, PlayMode, Pool, VoiceH, VoiceOp};
use cp_oracle::{K_NOW, NULL_VOICE, Oracle, cmd};

const NCH: usize = 2;

#[derive(Clone, Copy, Debug)]
enum Op {
    Play {
        clip: usize,
        rate: f64,
        gain: f64,
        looped: bool,
    },
    Stop {
        fade: f64,
    },
    Set {
        param: Param,
        value: f64,
    },
    Queue {
        next: usize,
        xfade: f64,
    },
    EnvPt {
        idx: i32,
        t: f64,
        g: f64,
        c: f32,
    },
}

struct Twin {
    ctl: Ctl,
    pool: Pool,
    ora: Oracle,
    voices: Vec<Option<(VoiceH, u32)>>,
    clips: Vec<(i32, i32)>,
    ports: BTreeSet<usize>,
    step: usize,
    trace: Vec<String>,
}

fn oracle_dll() -> std::path::PathBuf {
    cp_oracle::build().unwrap_or_else(|e| panic!("{e}"))
}

impl Twin {
    fn new(srate: f64, nvoices: usize) -> Twin {
        Twin {
            ctl: Ctl::new(srate),
            pool: Pool::new(),
            ora: Oracle::load(&oracle_dll(), srate).unwrap(),
            voices: vec![None; nvoices],
            clips: Vec::new(),
            ports: BTreeSet::new(),
            step: 0,
            trace: Vec::new(),
        }
    }

    fn note(&mut self, s: String) {
        self.step += 1;
        if self.trace.len() > 40 {
            self.trace.remove(0);
        }
        self.trace.push(format!("{:4} {s}", self.step));
    }

    fn fail(&self, what: String) -> ! {
        panic!(
            "DIVERGENCE : {what}\nderniers pas :\n{}",
            self.trace.join("\n")
        );
    }

    fn clip(&mut self, data: Vec<f32>, nch: usize, srate: f64) -> usize {
        let c = self.ora.clip(&data, nch, srate);
        let r = self.pool.add(data, nch, srate).unwrap();
        assert!(c >= 0);
        self.clips.push((r, c));
        self.clips.len() - 1
    }

    fn rust_clip(&self, cid: usize) -> Arc<ClipData> {
        self.pool.get(self.clips[cid].0).unwrap().clone()
    }

    fn alloc(&mut self, v: usize, port: usize) {
        self.note(format!("alloc v{v} port {port}"));
        let r = self.ctl.voice_alloc(port);
        let c = self.ora.alloc(port);
        match (r, c) {
            (Some(rh), ch) if ch != NULL_VOICE => {
                self.voices[v] = Some((rh, ch));
                self.ports.insert(port);
            }
            (None, NULL_VOICE) => self.voices[v] = None,
            _ => self.fail(format!("alloc : rust {r:?}, c++ {c:#x}")),
        }
    }

    fn release(&mut self, v: usize) {
        self.note(format!("release v{v}"));
        if let Some((r, c)) = self.voices[v] {
            self.ctl.voice_release(r);
            self.ora.release(c);
        }
    }

    fn send(&mut self, v: usize, at: At, op: Op) {
        self.note(format!("v{v} {at:?} {op:?}"));
        let Some((rh, ch)) = self.voices[v] else {
            return;
        };
        let cat = match at {
            At::Now => K_NOW,
            At::Frame(f) => f,
        };
        let (rop, ty, a, b, u0, u1) = match op {
            Op::Play {
                clip,
                rate,
                gain,
                looped,
            } => (
                VoiceOp::Play {
                    clip: self.rust_clip(clip),
                    rate,
                    gain,
                    mode: if looped {
                        PlayMode::Loop
                    } else {
                        PlayMode::Once
                    },
                },
                cmd::PLAY,
                rate,
                gain,
                self.clips[clip].1 as u32,
                looped as u32,
            ),
            Op::Stop { fade } => (VoiceOp::Stop { fade_s: fade }, cmd::STOP, fade, 0.0, 0, 0),
            Op::Set { param, value } => (
                VoiceOp::Set { param, value },
                cmd::SET,
                value,
                0.0,
                param as u32,
                0,
            ),
            Op::Queue { next, xfade } => {
                let Some((nr, nc)) = self.voices[next] else {
                    return;
                };
                (
                    VoiceOp::Queue {
                        next: nr,
                        xfade_s: xfade,
                    },
                    cmd::QUEUE,
                    xfade,
                    0.0,
                    nc,
                    0,
                )
            }
            Op::EnvPt { idx, t, g, c } => (
                VoiceOp::EnvPt {
                    idx,
                    t,
                    g,
                    curve: c,
                },
                cmd::ENV_PT,
                t,
                g,
                idx as u32,
                c.to_bits(),
            ),
        };
        let r_ok = self.ctl.send(rh, at, rop);
        let port = self.ora.voice_port(ch);
        let c_ok = port >= 0 && self.ora.post(port, ty, ch, cat, a, b, u0, u1);
        if r_ok != c_ok {
            self.fail(format!("envoi accepte : rust {r_ok}, c++ {c_ok}"));
        }
    }

    fn port_gain(&mut self, port: usize, g: f64) {
        self.note(format!("port {port} gain {g}"));
        self.ctl.port_gain(port, g);
        self.ora.post(
            port as i32,
            cmd::PORT_GAIN,
            0,
            K_NOW,
            g,
            0.0,
            0,
            port as u32,
        );
        self.ports.insert(port);
    }

    fn panic(&mut self) {
        self.note("panic".into());
        self.ctl.panic();
        self.ora.panic();
    }

    fn port_reset(&mut self, port: usize) {
        self.note(format!("port_reset {port}"));
        self.ctl.port_reset(port);
        self.ora.port_reset(port);
    }

    fn render(&mut self, frames: usize, tick: bool) {
        self.note(format!(
            "render {frames}{}",
            if tick { "" } else { " (sans tick)" }
        ));
        let ports: Vec<usize> = self.ports.iter().copied().collect();
        for p in ports {
            let mut ro = vec![0.0f32; frames * NCH];
            let mut co = vec![0.0f32; frames * NCH];
            let rb = self
                .ctl
                .core()
                .render_port(p, &mut ro, frames, NCH)
                .unwrap_or(-1);
            let cb = self.ora.render(p, &mut co, frames, NCH);
            if rb != cb {
                self.fail(format!("port {p} : debut de bloc rust {rb}, c++ {cb}"));
            }
            for i in 0..ro.len() {
                if ro[i].to_bits() != co[i].to_bits() {
                    self.fail(format!(
                        "port {p}, frame {} canal {} (bloc a {rb}) : rust {:e}, c++ {:e}",
                        i / NCH,
                        i % NCH,
                        ro[i],
                        co[i]
                    ));
                }
            }
            let (rl, cl) = (
                self.ctl.core().port_list_size(p).unwrap() as i32,
                self.ora.port_list_size(p),
            );
            if rl != cl {
                self.fail(format!(
                    "port {p} : {rl} voix en liste cote rust, {cl} cote c++"
                ));
            }
        }
        if tick {
            self.ctl.core().tick(frames as i32);
            self.ora.tick(frames as i32);
        }
        let (rc, cc) = (self.ctl.core().clock_now(), self.ora.clock());
        if rc != cc {
            self.fail(format!("horloge rust {rc}, c++ {cc}"));
        }
        let (ro, co) = (self.ctl.core().owned_voices() as i32, self.ora.owned());
        if ro != co {
            self.fail(format!("voix possedees rust {ro}, c++ {co}"));
        }
        for v in 0..self.voices.len() {
            let Some((rh, ch)) = self.voices[v] else {
                continue;
            };
            let rq = self.ctl.core().voice_query(rh);
            let cq = self.ora.query(ch);
            let same = match (rq, cq) {
                (Some((rp, rs)), Some((cp, cs))) => rp.to_bits() == cp.to_bits() && rs == cs,
                (None, None) => true,
                _ => false,
            };
            if !same {
                self.fail(format!("v{v} publie : rust {rq:?}, c++ {cq:?}"));
            }
            let (rs, cs) = (
                self.ctl.core().voice_started_at(rh),
                self.ora.started_at(ch),
            );
            if rs != cs {
                self.fail(format!("v{v} depart : rust {rs}, c++ {cs}"));
            }
        }
    }

    fn clock(&self) -> i64 {
        self.ctl.core().clock_now()
    }
}

// ---------------------------------------------------------------------------
// Les matieres de test : du bruit deterministe plus une sinusoide, pour que
// chaque interpolation se voie.
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
}

fn signal(frames: usize, nch: usize, seed: u64, freq: f64, srate: f64) -> Vec<f32> {
    let mut r = Rng(seed | 1);
    let mut v = Vec::with_capacity(frames * nch);
    for i in 0..frames {
        for ch in 0..nch {
            let s = (2.0 * std::f64::consts::PI * freq * (ch + 1) as f64 * i as f64 / srate).sin()
                * 0.5;
            v.push((s + (r.unit() - 0.5) * 0.3) as f32);
        }
    }
    v
}

fn standard_clips(t: &mut Twin) {
    t.clip(signal(4000, 1, 11, 440.0, 48000.0), 1, 48000.0); // 0 : mono 48 k
    t.clip(signal(6000, 2, 22, 330.0, 44100.0), 2, 44100.0); // 1 : stereo 44,1 k
    t.clip(signal(100, 1, 33, 1000.0, 48000.0), 1, 48000.0); // 2 : tres court
    t.clip(signal(20000, 2, 44, 110.0, 48000.0), 2, 48000.0); // 3 : stereo long
}

fn play(clip: usize, rate: f64, gain: f64, looped: bool) -> Op {
    Op::Play {
        clip,
        rate,
        gain,
        looped,
    }
}

fn set(param: Param, value: f64) -> Op {
    Op::Set { param, value }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[test]
fn lecture_simple_et_rendez_vous_dans_le_bloc() {
    let mut t = Twin::new(48000.0, 4);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.send(0, At::Frame(100), play(0, 1.0, 1.0, false));
    for _ in 0..40 {
        t.render(128, true);
    }
}

#[test]
fn taux_boucle_balance_gain_et_taux_du_materiau() {
    let mut t = Twin::new(48000.0, 4);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.alloc(1, 1);
    t.send(0, At::Now, set(Param::LoopStart, 500.0));
    t.send(0, At::Now, set(Param::LoopEnd, 1700.0));
    t.send(0, At::Now, set(Param::Pan, -0.4));
    t.send(0, At::Now, play(0, 1.5, 0.8, true));
    t.send(1, At::Now, play(1, 1.0, 1.0, true)); // 44,1 k lu a 48 k : interpole
    for i in 0..60 {
        if i == 20 {
            t.send(0, At::Now, set(Param::Gain, 0.2));
            t.send(1, At::Now, set(Param::Rate, 0.75));
        }
        if i == 30 {
            t.port_gain(1, 0.5);
        }
        t.render(256, true);
    }
}

#[test]
fn fondus_et_arrets() {
    let mut t = Twin::new(48000.0, 4);
    standard_clips(&mut t);
    for v in 0..3 {
        t.alloc(v, 0);
        t.send(v, At::Now, set(Param::FadeIn, 0.01));
        t.send(v, At::Now, set(Param::FadeOut, 0.02));
    }
    t.send(0, At::Now, play(3, 1.0, 1.0, true));
    t.send(1, At::Frame(300), play(3, 1.0, 1.0, true));
    t.send(2, At::Frame(1000), play(0, 1.0, 1.0, true));
    for i in 0..80 {
        let now = t.clock();
        match i {
            10 => t.send(0, At::Frame(now + 2000), Op::Stop { fade: 0.02 }), // date, avec fondu
            15 => t.send(1, At::Now, Op::Stop { fade: 0.01 }), // maintenant, en douceur
            20 => t.send(2, At::Frame(now + 77), Op::Stop { fade: 0.0 }), // coupure nette datee
            _ => {}
        }
        t.render(200, true);
    }
}

#[test]
fn adsr_courbes_et_relache() {
    let mut t = Twin::new(48000.0, 4);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.alloc(1, 0);
    for (v, looped) in [(0usize, true), (1, false)] {
        t.send(v, At::Now, set(Param::Env, 1.0));
        t.send(v, At::Now, set(Param::Attack, 0.01));
        t.send(v, At::Now, set(Param::Decay, 0.02));
        t.send(v, At::Now, set(Param::Sustain, 0.4));
        t.send(v, At::Now, set(Param::Release, 0.03));
        t.send(v, At::Now, set(Param::ACurve, 0.6));
        t.send(v, At::Now, set(Param::DCurve, -0.4));
        t.send(v, At::Now, set(Param::RCurve, 0.9));
        t.send(
            v,
            At::Now,
            play(if looped { 3 } else { 1 }, 1.0, 1.0, looped),
        );
    }
    for i in 0..70 {
        if i == 25 {
            t.send(0, At::Now, set(Param::Gate, 0.0));
        }
        t.render(160, true);
    }
}

#[test]
fn enveloppe_a_points_lot_refuse_et_valide() {
    let mut t = Twin::new(48000.0, 2);
    standard_clips(&mut t);
    t.alloc(0, 0);
    let pts = [
        (0.0, 0.0, 0.5),
        (0.01, 1.0, -0.7),
        (0.03, 0.3, 0.0),
        (0.05, 1.2, 1.0),
    ];
    for (i, &(tt, g, c)) in pts.iter().enumerate() {
        t.send(
            0,
            At::Now,
            Op::EnvPt {
                idx: i as i32,
                t: tt,
                g,
                c,
            },
        );
    }
    t.send(0, At::Now, set(Param::EnvPts, 5.0)); // lot incomplet : refuse
    for (i, &(tt, g, c)) in pts.iter().enumerate() {
        t.send(
            0,
            At::Now,
            Op::EnvPt {
                idx: i as i32,
                t: tt,
                g,
                c,
            },
        );
    }
    t.send(0, At::Now, set(Param::EnvPts, 4.0));
    t.send(0, At::Now, play(3, 1.25, 1.0, true));
    for _ in 0..50 {
        t.render(128, true);
    }
}

#[test]
fn sauts_de_tete_et_relances() {
    let mut t = Twin::new(48000.0, 2);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.send(0, At::Now, play(3, 1.0, 1.0, true));
    for i in 0..60 {
        match i {
            8 => t.send(0, At::Now, set(Param::Pos, 7777.25)), // saut : croisement
            16 => t.send(0, At::Now, play(3, 1.0, 1.0, true)), // meme clip : croisement
            24 => t.send(0, At::Now, play(1, 0.9, 1.0, true)), // autre clip : fondu force
            32 => t.send(0, At::Now, set(Param::Pos, 1_000_000.0)), // loin devant : reduit
            _ => {}
        }
        t.render(100, true);
    }
}

#[test]
fn enchainement_exact_a_l_echantillon() {
    let mut t = Twin::new(48000.0, 3);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.alloc(1, 0);
    t.alloc(2, 1);
    t.send(0, At::Now, play(2, 1.0, 1.0, false)); // 100 frames puis fin
    t.send(1, At::Frame(1_000_000), play(0, 1.0, 1.0, false));
    t.send(
        0,
        At::Now,
        Op::Queue {
            next: 1,
            xfade: 0.0005,
        },
    );
    t.send(
        2,
        At::Now,
        Op::Queue {
            next: 1,
            xfade: 0.0,
        },
    ); // autre port : sans effet
    for _ in 0..40 {
        t.render(64, true);
    }
}

#[test]
fn liberation_reallocation_panique_et_remise_a_zero() {
    let mut t = Twin::new(48000.0, 6);
    standard_clips(&mut t);
    for v in 0..4 {
        t.alloc(v, v % 2);
        t.send(v, At::Now, play(3, 1.0, 0.7, true));
    }
    for i in 0..60 {
        match i {
            5 => t.release(1),
            6 => t.send(1, At::Now, set(Param::Gain, 3.0)), // handle perime : refuse
            8 => t.alloc(4, 1),
            9 => t.send(4, At::Now, play(0, 1.0, 1.0, true)),
            20 => t.panic(),
            30 => t.port_reset(0),
            31 => t.alloc(5, 0),
            32 => t.send(5, At::Now, play(1, 1.0, 1.0, false)),
            _ => {}
        }
        t.render(128, true);
    }
}

#[test]
fn horloge_sans_crochet_audio() {
    let mut t = Twin::new(48000.0, 2);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.alloc(1, 1);
    t.send(0, At::Frame(500), play(0, 1.0, 1.0, true));
    t.send(1, At::Frame(700), play(3, 1.0, 1.0, true));
    for _ in 0..30 {
        t.render(256, false); // un port fait office d'horloge
    }
}

#[test]
fn changement_de_taux_de_l_appareil() {
    let mut t = Twin::new(48000.0, 1);
    standard_clips(&mut t);
    t.alloc(0, 0);
    t.send(0, At::Now, play(3, 1.0, 1.0, true));
    for i in 0..30 {
        if i == 10 {
            t.ctl.set_srate(44100.0);
            t.ora.set_srate(44100.0);
        }
        t.render(128, true);
    }
}

// ---------------------------------------------------------------------------
// Le fuzz : des suites tirees au hasard, sur trois ports
// ---------------------------------------------------------------------------

fn fuzz_one(seed: u64) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut t = Twin::new(r.pick(&[48000.0, 44100.0, 96000.0]), 10);
    standard_clips(&mut t);
    let params = [
        Param::Rate,
        Param::Gain,
        Param::Pan,
        Param::LoopStart,
        Param::LoopEnd,
        Param::FadeIn,
        Param::FadeOut,
        Param::Pos,
        Param::Loop,
        Param::Env,
        Param::Attack,
        Param::Decay,
        Param::Sustain,
        Param::Release,
        Param::ACurve,
        Param::DCurve,
        Param::RCurve,
        Param::Gate,
    ];
    for _ in 0..260 {
        let v = r.below(10);
        let now = t.clock();
        let at = if r.below(3) == 0 {
            At::Now
        } else {
            At::Frame(now + r.below(1500) as i64 - 200)
        };
        match r.below(100) {
            0..=7 => t.alloc(v, r.below(3)),
            8..=11 => t.release(v),
            12..=31 => {
                let op = play(
                    r.below(4),
                    r.pick(&[1.0, 0.5, 1.5, 2.0, 0.7, 1.000_000_001]),
                    r.pick(&[1.0, 0.5, 0.0, 1.3]),
                    r.below(2) == 0,
                );
                t.send(v, at, op);
            }
            32..=39 => {
                let fade = r.pick(&[0.0, 0.001, 0.01, 0.05]);
                t.send(v, at, Op::Stop { fade });
            }
            40..=56 => {
                let p = r.pick(&params);
                let x = match p {
                    Param::Rate => r.pick(&[0.5, 1.0, 1.5, 2.0, -1.0, 0.999]),
                    Param::Gain => r.pick(&[0.0, 0.5, 1.0, 2.0]),
                    Param::Pan => r.pick(&[-1.0, -0.5, 0.0, 0.3, 1.0, 2.0]),
                    Param::LoopStart => r.pick(&[0.0, 10.0, 100.0, 1000.0, 5000.0]),
                    Param::LoopEnd => r.pick(&[0.0, 50.0, 500.0, 3000.0, 100000.0]),
                    Param::FadeIn | Param::FadeOut => r.pick(&[0.0, 0.001, 0.01, 0.1]),
                    Param::Pos => r.pick(&[0.0, 10.5, 1000.0, 3999.9, 100000.0]),
                    Param::Loop | Param::Env | Param::Gate => r.pick(&[0.0, 1.0]),
                    Param::Attack | Param::Decay | Param::Release => {
                        r.pick(&[0.0, 0.001, 0.01, 0.05])
                    }
                    Param::Sustain => r.pick(&[0.0, 0.3, 1.0]),
                    _ => r.pick(&[-1.0, -0.5, 0.0, 0.5, 1.0]),
                };
                t.send(v, at, set(p, x));
            }
            57..=59 => {
                let next = r.below(10);
                let xfade = r.pick(&[0.0, 0.001, 0.01]);
                t.send(v, at, Op::Queue { next, xfade });
            }
            60..=62 => {
                let n = 1 + r.below(6);
                let mut tt = 0.0;
                for i in 0..n {
                    tt += r.unit() * 0.02;
                    let g = r.unit() * 1.5;
                    let c = (r.unit() * 2.0 - 1.0) as f32;
                    t.send(
                        v,
                        At::Now,
                        Op::EnvPt {
                            idx: i as i32,
                            t: tt,
                            g,
                            c,
                        },
                    );
                }
                let claimed = if r.below(5) == 0 { n + 1 } else { n };
                t.send(v, At::Now, set(Param::EnvPts, claimed as f64));
            }
            63..=64 => t.port_gain(r.below(3), r.pick(&[0.0, 0.5, 1.0, 1.5])),
            65 => t.panic(),
            66 => t.port_reset(r.below(3)),
            _ => {
                let frames = r.pick(&[64, 128, 256, 300, 512, 1, 37]);
                t.render(frames, r.below(30) != 0);
            }
        }
    }
}

#[test]
fn fuzz_trois_cents_graines() {
    for seed in 0..300 {
        fuzz_one(seed);
    }
}
