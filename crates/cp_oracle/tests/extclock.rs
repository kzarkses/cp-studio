//! La clock externe portee (`cp_engine::extclock`) contre l'ancienne
//! (`reference/cp_native/src/core/cp_extclock.cpp`) : les memes messages,
//! dates au meme echantillon, doivent donner le meme etat AU BIT PRES apres
//! chaque bloc. Des scenarios (tempo stable et bruite, changement de tempo,
//! impulsions perdues, tempo divise par deux, Start / Stop / Continue / SPP,
//! doublons, changement de taux) puis 300 graines au hasard.

#![allow(clippy::missing_transmute_annotations)]

use std::ffi::c_void;

use cp_engine::extclock::{ExtClock, Snap, qn_at};
use cp_oracle::{Dll, build_extclock};

struct Old {
    p: *mut c_void,
    tick: extern "C" fn(*mut c_void, f64),
    start: extern "C" fn(*mut c_void, f64),
    cont: extern "C" fn(*mut c_void, f64),
    stop: extern "C" fn(*mut c_void, f64),
    spp: extern "C" fn(*mut c_void, i32),
    end_block: extern "C" fn(*mut c_void, f64),
    set_srate: extern "C" fn(*mut c_void, f64),
    reset: extern "C" fn(*mut c_void),
    live: extern "C" fn(*mut c_void, *mut f64),
    qn_at: extern "C" fn(*mut c_void, f64) -> f64,
    free: extern "C" fn(*mut c_void),
}

impl Old {
    fn new(d: &Dll) -> Old {
        macro_rules! f {
            ($n:literal) => {
                // SAFETY: signatures de cpp/extclock_shim.cpp.
                unsafe { std::mem::transmute(d.sym($n).unwrap()) }
            };
        }
        let new: extern "C" fn() -> *mut c_void = f!("ec_new");
        Old {
            p: new(),
            tick: f!("ec_tick"),
            start: f!("ec_start"),
            cont: f!("ec_cont"),
            stop: f!("ec_stop"),
            spp: f!("ec_spp"),
            end_block: f!("ec_end_block"),
            set_srate: f!("ec_set_srate"),
            reset: f!("ec_reset"),
            live: f!("ec_live"),
            qn_at: f!("ec_qn_at"),
            free: f!("ec_free"),
        }
    }

    fn snap(&self) -> [f64; 13] {
        let mut o = [0.0; 13];
        (self.live)(self.p, o.as_mut_ptr());
        o
    }
}

impl Drop for Old {
    fn drop(&mut self) {
        (self.free)(self.p);
    }
}

fn fields(s: &Snap) -> [f64; 13] {
    [
        s.bpm,
        s.bpm_err,
        s.bpm_short,
        s.jitter_ms,
        s.beats,
        s.t_last,
        s.pos_last,
        s.pos_next,
        s.spt,
        s.flags as f64,
        s.starts as f64,
        s.stops as f64,
        s.ticks as f64,
    ]
}

#[derive(Clone, Copy, Debug)]
enum Ev {
    Tick(f64),
    Start(f64),
    Cont(f64),
    Stop(f64),
    Spp(i32),
    End(f64),
    Srate(f64),
    Reset,
}

/// Joue les memes evenements dans les deux, et compare apres chaque bloc.
fn compare(d: &Dll, evs: &[Ev], what: &str) {
    let old = Old::new(d);
    let mut new = ExtClock::new();
    let mut blocks = 0;
    for (i, &e) in evs.iter().enumerate() {
        match e {
            Ev::Tick(t) => {
                (old.tick)(old.p, t);
                new.tick(t);
            }
            Ev::Start(t) => {
                (old.start)(old.p, t);
                new.start(t);
            }
            Ev::Cont(t) => {
                (old.cont)(old.p, t);
                new.cont(t);
            }
            Ev::Stop(t) => {
                (old.stop)(old.p, t);
                new.stop(t);
            }
            Ev::Spp(s) => {
                (old.spp)(old.p, s);
                new.spp(s);
            }
            Ev::Srate(sr) => {
                (old.set_srate)(old.p, sr);
                new.set_srate(sr);
            }
            Ev::Reset => {
                (old.reset)(old.p);
                new.reset();
            }
            Ev::End(now) => {
                (old.end_block)(old.p, now);
                new.end_block(now);
                blocks += 1;
                let (a, b) = (old.snap(), fields(new.live()));
                for k in 0..13 {
                    assert!(
                        a[k].to_bits() == b[k].to_bits(),
                        "{what} : evenement {i}, champ {k} : C++ {} / Rust {}",
                        a[k],
                        b[k]
                    );
                }
                // La position du maitre, a l'instant et un peu apres.
                for dt in [0.0, 333.0, 2000.5] {
                    let (qa, qb) = ((old.qn_at)(old.p, now + dt), qn_at(new.live(), now + dt));
                    assert_eq!(qa.to_bits(), qb.to_bits(), "{what} : qn_at, evenement {i}");
                }
                // La publication rend ce qui a ete publie.
                assert_eq!(new.read().as_ref().map(fields), Some(b));
            }
        }
    }
    assert!(blocks > 0);
}

/// Un generateur reproductible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Des impulsions a `bpm` de `t0` pendant `beats` noires, bruitees de
/// +-`jit` echantillons, avec un bloc de 512 entre deux.
fn pulses(
    evs: &mut Vec<Ev>,
    t0: f64,
    bpm: f64,
    beats: f64,
    jit: f64,
    r: &mut Rng,
    drop_every: usize,
) -> f64 {
    let spt = 48000.0 * 60.0 / (bpm * 24.0);
    let n = (beats * 24.0) as usize;
    let mut t = t0;
    let mut next_end = (t0 / 512.0).ceil() * 512.0;
    for k in 0..n {
        let tt = t + (r.next() * 2.0 - 1.0) * jit;
        while next_end < tt {
            evs.push(Ev::End(next_end));
            next_end += 512.0;
        }
        if drop_every == 0 || k % drop_every != drop_every - 1 {
            evs.push(Ev::Tick(tt.round()));
        }
        t += spt;
    }
    evs.push(Ev::End(next_end));
    t
}

#[test]
fn la_clock_externe_est_l_ancienne_au_bit_pres() {
    let p = build_extclock().unwrap_or_else(|e| panic!("{e}"));
    let d = Dll::load(&p).unwrap();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);

    // Un tempo stable, bruite d'une milliseconde, apres un Start.
    let mut e = vec![Ev::Start(0.0)];
    pulses(&mut e, 100.0, 120.0, 20.0, 48.0, &mut r, 0);
    compare(&d, &e, "120 BPM bruite");

    // 124 puis 128 : la fenetre longue repart.
    let mut e = vec![Ev::Start(0.0)];
    let t = pulses(&mut e, 100.0, 124.0, 20.0, 30.0, &mut r, 0);
    pulses(&mut e, t, 128.0, 20.0, 30.0, &mut r, 0);
    compare(&d, &e, "124 -> 128");

    // Une impulsion sur 7 perdue (remplie), puis 140 -> 70 (pas une perte).
    let mut e = vec![Ev::Start(0.0)];
    let t = pulses(&mut e, 100.0, 140.0, 12.0, 10.0, &mut r, 7);
    pulses(&mut e, t, 70.0, 12.0, 10.0, &mut r, 0);
    compare(&d, &e, "pertes, tempo divise");

    // Stop, SPP, Continue ; un doublon ; un silence ; un changement de taux.
    let mut e = vec![Ev::Start(0.0)];
    let t = pulses(&mut e, 100.0, 100.0, 8.0, 20.0, &mut r, 0);
    e.push(Ev::Stop(t));
    e.push(Ev::End(t + 512.0));
    e.push(Ev::Spp(33));
    e.push(Ev::Cont(t + 2000.0));
    e.push(Ev::Tick(t + 3000.0));
    e.push(Ev::Tick(t + 3001.0));
    let t = pulses(&mut e, t + 4200.0, 100.0, 8.0, 20.0, &mut r, 0);
    e.push(Ev::End(t + 96000.0));
    e.push(Ev::Srate(44100.0));
    let t = pulses(&mut e, t + 100000.0, 90.0, 8.0, 20.0, &mut r, 0);
    e.push(Ev::Reset);
    pulses(&mut e, t + 1000.0, 90.0, 4.0, 20.0, &mut r, 0);
    compare(&d, &e, "transport, doublon, taux");

    // 300 graines : des suites quelconques de tout.
    for seed in 1..=300u64 {
        let mut r = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let mut e = Vec::new();
        let mut t = 0.0;
        for _ in 0..12 {
            let x = r.next();
            if x < 0.15 {
                e.push(Ev::Start(t));
            } else if x < 0.22 {
                e.push(Ev::Stop(t));
            } else if x < 0.28 {
                e.push(Ev::Cont(t));
            } else if x < 0.33 {
                e.push(Ev::Spp((r.next() * 200.0) as i32));
            } else if x < 0.36 {
                e.push(Ev::Reset);
            } else if x < 0.38 {
                e.push(Ev::Srate(if r.next() < 0.5 { 44100.0 } else { 48000.0 }));
            }
            let bpm = 60.0 + r.next() * 140.0;
            let beats = 1.0 + r.next() * 6.0;
            let drop = [0, 0, 0, 5, 9][(r.next() * 5.0) as usize % 5];
            t = pulses(&mut e, t, bpm, beats, r.next() * 80.0, &mut r, drop);
            t += r.next() * 30000.0;
            e.push(Ev::End(t));
        }
        compare(&d, &e, &format!("graine {seed}"));
    }
}

#[test]
fn le_tempo_et_la_position_se_lisent() {
    // Sans l'oracle : 128 BPM bruite d'une ms, la pente a un centieme de
    // BPM pres, la position du maitre a la noire.
    let mut c = ExtClock::new();
    let mut r = Rng(7);
    let mut e = vec![Ev::Start(0.0)];
    pulses(&mut e, 1000.0, 128.0, 16.0, 48.0, &mut r, 0);
    let mut last = 0.0;
    for ev in e {
        match ev {
            Ev::Start(t) => c.start(t),
            Ev::Tick(t) => c.tick(t),
            Ev::End(t) => {
                c.end_block(t);
                last = t;
            }
            _ => {}
        }
    }
    let s = c.read().unwrap();
    assert!((s.bpm - 128.0).abs() < 0.01, "{}", s.bpm);
    // 16 noires depuis Start : le maitre est vers la seizieme.
    let q = qn_at(&s, last);
    assert!((q - 16.0).abs() < 0.1, "{q}");
}
