//! Le reseau de Basic Pitch, une fenetre a la fois.
//!
//! Basic Pitch (Spotify, ICASSP 2022, licence Apache-2.0) : un CQT a 3 cases
//! par demi-ton, empile sur huit harmoniques, puis six convolutions. Il rend
//! trois matrices par fenetre de 2 s : NOTE (88 hauteurs, la note sonne),
//! ONSET (88, la note COMMENCE) et CONTOUR (264 cases d'un tiers de demi-ton,
//! la hauteur fine : c'est la que se lisent les bends).
//!
//! CE FICHIER REJOUE LE GRAPHE ONNX DU MODELE, A LA LETTRE (le portage C++ du
//! 2026-11-20, verifie a 2e-5 contre onnxruntime, refait en Rust). Les poids
//! sont dans `bp_weights.bin` (tools/gen_weights.py) ; le CQT, son filtre et
//! ses longueurs sont CALCULES ici, avec les formules de nnAudio (CQT2010v2)
//! et de scipy.signal.firwin2 que le modele a vues a l'entrainement. Un CQT a
//! peine different, et le reseau voit autre chose que ce qu'il a appris : les
//! tests confrontent la sortie entiere a celle d'onnxruntime.
//!
//! L'ordre des operations :
//!   1. CQT2010v2 : l'octave du haut par 36 noyaux complexes de 256 points, pas
//!      de 256 ; puis huit fois : demi-bande (firwin2, 256 points, pas de 2,
//!      zeros autour), memes noyaux, pas divise par deux. Chaque octave est
//!      reflechie de 128 points avant ses noyaux. 9 x 36 = 324 cases, on garde
//!      les 309 du haut, multipliees par la racine de leur longueur ;
//!   2. log : 10 log10(m^2 + 1e-10), ramene a 0..1 sur la FENETRE entiere,
//!      puis la normalisation par lot ;
//!   3. empilement : huit copies decalees de -36, 0, 36, 57, 72, 84, 93, 101
//!      cases (harmoniques 0.5, 1, 2 .. 7), les 264 premieres ;
//!   4. six convolutions, deux branches.

use std::sync::OnceLock;

pub const SR: usize = 22050;
pub const HOP: usize = 256;
pub const WIN_SAMPLES: usize = 43844;
pub const WIN_FRAMES: usize = 172;
pub const NOTES: usize = 88;
pub const MIDI_OFFSET: i32 = 21;
pub const CONTOUR: usize = 264;
const CQT_BINS: usize = 309;
const OCTAVES: usize = 9;
const BINS_PER_OCT: usize = 36;
const TAPS: usize = 256;
const HARM: usize = 8;

/// Les poids du reseau (normalisation par lot deja fondue).
struct Weights {
    bn_mul: f32,
    bn_add: f32,
    c1w: Vec<f32>,
    c1b: Vec<f32>,
    c2w: Vec<f32>,
    c2b: Vec<f32>,
    n1w: Vec<f32>,
    n1b: Vec<f32>,
    n2w: Vec<f32>,
    n2b: Vec<f32>,
    o1w: Vec<f32>,
    o1b: Vec<f32>,
    o2w: Vec<f32>,
    o2b: Vec<f32>,
}

const WEIGHTS: &[u8] = include_bytes!("bp_weights.bin");

fn weights() -> &'static Weights {
    static W: OnceLock<Weights> = OnceLock::new();
    W.get_or_init(|| {
        assert_eq!(&WEIGHTS[..4], b"CPBW", "bp_weights.bin : signature");
        let mut at = 8;
        let mut take = |n: usize| -> Vec<f32> {
            let v = WEIGHTS[at..at + n * 4]
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            at += n * 4;
            v
        };
        let bn = take(2);
        let w = Weights {
            bn_mul: bn[0],
            bn_add: bn[1],
            c1w: take(8 * 8 * 3 * 39),
            c1b: take(8),
            c2w: take(8 * 5 * 5),
            c2b: take(1),
            n1w: take(32 * 7 * 7),
            n1b: take(32),
            n2w: take(32 * 7 * 3),
            n2b: take(1),
            o1w: take(32 * 8 * 5 * 5),
            o1b: take(32),
            o2w: take(33 * 3 * 3),
            o2b: take(1),
        };
        assert_eq!(at, WEIGHTS.len(), "bp_weights.bin : taille");
        w
    })
}

/// Les noyaux du CQT, le filtre de demi-bande, la normalisation des cases.
struct Kernels {
    re: Vec<[f32; TAPS]>,
    im: Vec<[f32; TAPS]>,
    lowpass: [f32; TAPS],
    sqrtlen: [f32; CQT_BINS],
}

const PI: f64 = std::f64::consts::PI;

/// Le Q du CQT : un filtre par 1/36 d'octave.
fn q_factor() -> f64 {
    1.0 / (2f64.powf(1.0 / BINS_PER_OCT as f64) - 1.0)
}

/// scipy.signal.firwin2(256, [0, 0.5/1.001, 0.5*1.001, 1], [1, 1, 0, 0]) :
/// echantillonnage en frequence sur 257 points, phase lineaire, irfft sur 512,
/// les 256 premiers points fenetres par une Hamming SYMETRIQUE.
fn make_lowpass() -> [f32; TAPS] {
    const NFREQS: usize = 257;
    let nfft = 2 * (NFREQS - 1);
    let (band, trans) = (0.5f64, 0.001f64);
    let (f1, f2) = (band / (1.0 + trans), band * (1.0 + trans));
    let mut re = [0f64; NFREQS];
    let mut im = [0f64; NFREQS];
    for k in 0..NFREQS {
        let x = k as f64 / (NFREQS - 1) as f64;
        let g = if x <= f1 {
            1.0
        } else if x >= f2 {
            0.0
        } else {
            (f2 - x) / (f2 - f1)
        };
        let ph = -((TAPS - 1) as f64) / 2.0 * PI * x;
        re[k] = g * ph.cos();
        im[k] = g * ph.sin();
    }
    let mut out = [0f32; TAPS];
    for (n, o) in out.iter_mut().enumerate() {
        // irfft : la partie imaginaire du continu et de Nyquist est ignoree.
        let mut s = re[0] + re[NFREQS - 1] * if n & 1 == 1 { -1.0 } else { 1.0 };
        for k in 1..NFREQS - 1 {
            let a = 2.0 * PI * k as f64 * n as f64 / nfft as f64;
            s += 2.0 * (re[k] * a.cos() - im[k] * a.sin());
        }
        s /= nfft as f64;
        let w = 0.54 - 0.46 * (2.0 * PI * n as f64 / (TAPS - 1) as f64).cos();
        *o = (s * w) as f32;
    }
    out
}

fn kernels() -> &'static Kernels {
    static K: OnceLock<Kernels> = OnceLock::new();
    K.get_or_init(|| {
        let q = q_factor();
        let fs = SR as f64;
        // CQT2010v2.build : la case du haut de l'octave du haut, puis son bas.
        let remainder = CQT_BINS % BINS_PER_OCT; // 21
        let fmin_t0 = 27.5 * 2f64.powi(OCTAVES as i32 - 1);
        let fmax_t = fmin_t0 * 2f64.powf((remainder as f64 - 1.0) / BINS_PER_OCT as f64);
        let fmin_t = fmax_t / 2f64.powf(1.0 - 1.0 / BINS_PER_OCT as f64);
        let mut re = vec![[0f32; TAPS]; BINS_PER_OCT];
        let mut im = vec![[0f32; TAPS]; BINS_PER_OCT];
        for b in 0..BINS_PER_OCT {
            let freq = fmin_t * 2f64.powf(b as f64 / BINS_PER_OCT as f64);
            let l = (q * fs / freq).ceil();
            let ll = l as i32;
            let start = (TAPS as f64 / 2.0 - l / 2.0).ceil() as i32 - (ll % 2);
            let m0 = (-l / 2.0).floor() as i32; // np.r_[-l//2 : l//2]
            // Hann PERIODIQUE, puis normalisation L1 du complexe.
            let mut wsum = 0.0;
            for n in 0..ll {
                wsum += 0.5 - 0.5 * (2.0 * PI * n as f64 / ll as f64).cos();
            }
            let norm = wsum / l;
            for n in 0..ll {
                let w = 0.5 - 0.5 * (2.0 * PI * n as f64 / ll as f64).cos();
                let a = (m0 + n) as f64 * 2.0 * PI * freq / fs;
                let at = start + n;
                if at < 0 || at >= TAPS as i32 {
                    continue;
                }
                re[b][at as usize] = (w * a.cos() / l / norm) as f32;
                im[b][at as usize] = (w * a.sin() / l / norm) as f32;
            }
        }
        let mut sqrtlen = [0f32; CQT_BINS];
        for (b, s) in sqrtlen.iter_mut().enumerate() {
            let freq = 27.5 * 2f64.powf(b as f64 / BINS_PER_OCT as f64);
            *s = ((q * fs / freq).ceil() as f32).sqrt();
        }
        Kernels {
            re,
            im,
            lowpass: make_lowpass(),
            sqrtlen,
        }
    })
}

/// Le miroir de numpy / torch : x[1], x[2] .. sans repeter le bord.
fn reflect_pad(x: &[f32], pad: usize, out: &mut [f32]) {
    let l = x.len();
    for i in 0..pad {
        out[i] = x[pad - i];
    }
    out[pad..pad + l].copy_from_slice(x);
    for j in 0..pad {
        out[pad + l + j] = x[l - 2 - j];
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Convolution 2D, disposition [canal][temps][frequence], pas de 1 en temps.
/// `ph` et `pwl` : les marges de debut (temps, frequence) du graphe ONNX ; la
/// marge de fin decoule des tailles.
#[allow(clippy::too_many_arguments)]
fn conv(
    input: &[f32],
    cin: usize,
    h: usize,
    win: usize,
    w: &[f32],
    b: &[f32],
    cout: usize,
    kh: usize,
    kw: usize,
    sw: usize,
    ph: i32,
    pwl: i32,
    out: &mut [f32],
    wout: usize,
) {
    let (plane_in, plane_out) = (h * win, h * wout);
    let (h_i, win_i, wout_i) = (h as i32, win as i32, wout as i32);
    for o in 0..cout {
        let op = &mut out[o * plane_out..(o + 1) * plane_out];
        op.fill(b[o]);
        for c in 0..cin {
            let ip = &input[c * plane_in..(c + 1) * plane_in];
            for dh in 0..kh as i32 {
                let t0 = (ph - dh).max(0);
                let t1 = (h_i - 1 + ph - dh).min(h_i - 1);
                for dw in 0..kw as i32 {
                    let wv = w[((o * cin + c) * kh + dh as usize) * kw + dw as usize];
                    // colonnes de sortie dont la source tombe dans [0, win)
                    let off = dw - pwl;
                    let mut f0 = 0i32;
                    while f0 < wout_i && f0 * sw as i32 + off < 0 {
                        f0 += 1;
                    }
                    let mut f1 = wout_i - 1;
                    while f1 >= f0 && f1 * sw as i32 + off >= win_i {
                        f1 -= 1;
                    }
                    if f1 < f0 {
                        continue;
                    }
                    for t in t0..=t1 {
                        let irow = ((t + dh - ph) * win_i + off) as isize;
                        let orow = (t * wout_i) as usize;
                        for f in f0..=f1 {
                            let src = (irow + (f as isize) * sw as isize) as usize;
                            op[orow + f as usize] += wv * ip[src];
                        }
                    }
                }
            }
        }
    }
}

fn relu(x: &mut [f32]) {
    for v in x {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
}

/// Le brouillon d'un fil (environ 8 Mo) : un par fil, reutilise de fenetre en
/// fenetre.
pub struct Work {
    sig: [Vec<f32>; 2],
    pad: Vec<f32>,
    cqt: Vec<f32>,
    hs: Vec<f32>,
    c1: Vec<f32>,
    n1: Vec<f32>,
    o1: Vec<f32>,
}

impl Default for Work {
    fn default() -> Self {
        Work::new()
    }
}

impl Work {
    pub fn new() -> Work {
        let n = WIN_SAMPLES + 2 * TAPS;
        Work {
            sig: [vec![0.0; n], vec![0.0; n]],
            pad: vec![0.0; n],
            cqt: vec![0.0; WIN_FRAMES * CQT_BINS],
            hs: vec![0.0; HARM * WIN_FRAMES * CONTOUR],
            c1: vec![0.0; 8 * WIN_FRAMES * CONTOUR],
            n1: vec![0.0; 32 * WIN_FRAMES * NOTES],
            o1: vec![0.0; 33 * WIN_FRAMES * NOTES],
        }
    }
}

/// Une fenetre de `WIN_SAMPLES` echantillons (22 050 Hz, mono) ->
/// note[WIN_FRAMES][NOTES], onset[WIN_FRAMES][NOTES],
/// contour[WIN_FRAMES][CONTOUR], toutes dans 0..1.
pub fn run_window(
    w: &mut Work,
    input: &[f32],
    note: &mut [f32],
    onset: &mut [f32],
    contour: &mut [f32],
) {
    let k = kernels();
    let wt = weights();
    // --- 1. CQT
    let mut len = WIN_SAMPLES;
    let (mut cur, mut nxt) = (0usize, 1usize);
    for (i, &v) in input.iter().take(WIN_SAMPLES).enumerate() {
        w.sig[cur][i] = if v.is_finite() && v.abs() < 1e30 {
            v
        } else {
            0.0
        };
    }
    let half = TAPS / 2;
    for d in 0..OCTAVES {
        let hop = HOP >> d;
        {
            let Work { sig, pad, .. } = &mut *w;
            reflect_pad(&sig[cur][..len], half, pad);
        }
        let base = (OCTAVES - 1 - d) as i32 * BINS_PER_OCT as i32
            - (OCTAVES * BINS_PER_OCT - CQT_BINS) as i32;
        for t in 0..WIN_FRAMES {
            let x = &w.pad[t * hop..t * hop + TAPS];
            for b in 0..BINS_PER_OCT {
                let bin = base + b as i32;
                if bin < 0 {
                    continue;
                }
                let (kr, ki) = (&k.re[b], &k.im[b]);
                let (mut sr, mut si) = (0f32, 0f32);
                for n in 0..TAPS {
                    sr += kr[n] * x[n];
                    si += ki[n] * x[n];
                }
                let s = k.sqrtlen[bin as usize];
                sr *= s;
                si *= s;
                w.cqt[t * CQT_BINS + bin as usize] = (sr * sr + si * si).sqrt();
            }
        }
        if d == OCTAVES - 1 {
            break;
        }
        // Demi-bande : zeros de 127 points autour, filtre de 256, pas de 2.
        let ln = (len + 2 * 127 - TAPS) / 2 + 1;
        {
            let [a, b] = &mut w.sig;
            let (src, dst) = if cur == 0 { (&*a, b) } else { (&*b, a) };
            for (i, o) in dst.iter_mut().take(ln).enumerate() {
                let j0 = 2 * i as i32 - 127;
                let mut s = 0f32;
                let n0 = if j0 < 0 { -j0 } else { 0 };
                let n1 = if j0 + TAPS as i32 > len as i32 {
                    len as i32 - j0
                } else {
                    TAPS as i32
                };
                for n in n0..n1 {
                    s += k.lowpass[n as usize] * src[(j0 + n) as usize];
                }
                *o = s;
            }
        }
        std::mem::swap(&mut cur, &mut nxt);
        len = ln;
    }

    // --- 2. log normalise sur la fenetre, puis normalisation par lot
    let (mut lo, mut hi) = (1e30f32, -1e30f32);
    for v in w.cqt.iter_mut() {
        let m = *v;
        let l = (m * m + 1e-10f32).ln() * std::f32::consts::LOG10_E * 10.0;
        *v = l;
        if l < lo {
            lo = l;
        }
    }
    for v in w.cqt.iter_mut() {
        let x = *v - lo;
        *v = x;
        if x > hi {
            hi = x;
        }
    }
    for v in w.cqt.iter_mut() {
        let x = if hi == 0.0 { 0.0 } else { *v / hi };
        *v = x * wt.bn_mul + wt.bn_add;
    }

    // --- 3. empilement harmonique
    const SHIFT: [i32; HARM] = [-36, 0, 36, 57, 72, 84, 93, 101];
    for (c, &s) in SHIFT.iter().enumerate() {
        for t in 0..WIN_FRAMES {
            let row = &mut w.hs[(c * WIN_FRAMES + t) * CONTOUR..(c * WIN_FRAMES + t + 1) * CONTOUR];
            let src = &w.cqt[t * CQT_BINS..(t + 1) * CQT_BINS];
            for (f, r) in row.iter_mut().enumerate() {
                let j = f as i32 + s;
                *r = if j >= 0 && (j as usize) < CQT_BINS {
                    src[j as usize]
                } else {
                    0.0
                };
            }
        }
    }

    // --- 4. le reseau
    let Work { hs, c1, n1, o1, .. } = &mut *w;
    // Contour : 3 x 39 (8 -> 8), ReLU, 5 x 5 (8 -> 1), sigmoide.
    conv(
        hs, HARM, WIN_FRAMES, CONTOUR, &wt.c1w, &wt.c1b, 8, 3, 39, 1, 1, 19, c1, CONTOUR,
    );
    relu(c1);
    conv(
        c1, 8, WIN_FRAMES, CONTOUR, &wt.c2w, &wt.c2b, 1, 5, 5, 1, 2, 2, contour, CONTOUR,
    );
    for v in contour.iter_mut().take(WIN_FRAMES * CONTOUR) {
        *v = sigmoid(*v);
    }
    // Notes : 7 x 7 pas 3 (1 -> 32), ReLU, 7 x 3 (32 -> 1), sigmoide.
    conv(
        contour, 1, WIN_FRAMES, CONTOUR, &wt.n1w, &wt.n1b, 32, 7, 7, 3, 3, 2, n1, NOTES,
    );
    relu(n1);
    let plane = WIN_FRAMES * NOTES;
    conv(
        n1,
        32,
        WIN_FRAMES,
        NOTES,
        &wt.n2w,
        &wt.n2b,
        1,
        7,
        3,
        1,
        3,
        1,
        &mut o1[..plane],
        NOTES,
    );
    for i in 0..plane {
        let v = sigmoid(o1[i]);
        o1[i] = v;
        note[i] = v;
    }
    // Onsets : 5 x 5 pas 3 sur l'empilement (8 -> 32), ReLU, concatene
    // derriere la note, 3 x 3 (33 -> 1), sigmoide.
    conv(
        hs,
        HARM,
        WIN_FRAMES,
        CONTOUR,
        &wt.o1w,
        &wt.o1b,
        32,
        5,
        5,
        3,
        2,
        1,
        &mut o1[plane..],
        NOTES,
    );
    relu(&mut o1[plane..]);
    conv(
        o1, 33, WIN_FRAMES, NOTES, &wt.o2w, &wt.o2b, 1, 3, 3, 1, 1, 1, onset, NOTES,
    );
    for v in onset.iter_mut().take(plane) {
        *v = sigmoid(*v);
    }
}
