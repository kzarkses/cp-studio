//! L'etirement CUIT (A5) : une region etiree une fois par l'etireur de
//! REAPER (elastique, la qualite d'un item), rangee comme un clip, puis
//! jouee a vitesse 1. La lecture ne coute rien de plus et la duree est
//! exacte a l'echantillon.
//!
//! L'etireur rend sa sortie en retard (son amorcage, 85 a 139 ms mesures
//! par l'ancien moteur). Ce retard se MESURE au lieu de se supposer : un
//! clic passe d'abord dans un etireur regle a l'identique, et l'ecart entre
//! ou il ressort et ou il devrait ressortir est retire de la vraie sortie.
//! Le tout tourne sur le fil de travail du decodage (un `Reader`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// La derniere cuisson, pour le journal : frames donnees, rendues, retard
/// mesure.
pub static LAST: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

use cp_engine::ClipData;
use cp_sys::pitch::PitchShift;

use crate::decode::Reader;

/// Ce que l'etirement demande a un etireur (celui de REAPER, ou une
/// imitation dans les tests).
pub trait Shifter: Send {
    /// Regle et remet a zero : `tempo` (1,5 = une fois et demie plus vite),
    /// `shift` (rapport de hauteur, 1 = inchangee).
    fn setup(&mut self, srate: f64, nch: usize, tempo: f64, shift: f64);
    fn feed(&mut self, input: &[f64], nch: usize);
    fn flush(&mut self);
    fn take(&mut self, out: &mut [f64], nch: usize) -> usize;
}

impl Shifter for PitchShift {
    fn setup(&mut self, srate: f64, nch: usize, tempo: f64, shift: f64) {
        self.set_quality(-1);
        self.set_srate(srate);
        self.set_nch(nch as i32);
        self.set_tempo(tempo);
        self.set_shift(shift);
        self.set_formant_shift(-1.0);
        self.reset();
    }
    fn feed(&mut self, input: &[f64], nch: usize) {
        PitchShift::feed(self, input, nch);
    }
    fn flush(&mut self) {
        PitchShift::flush(self);
    }
    fn take(&mut self, out: &mut [f64], nch: usize) -> usize {
        PitchShift::take(self, out, nch)
    }
}

/// L'etireur de REAPER en temps reel, prete a une voix
/// ([`cp_engine::shift`]) : la meme classe que pour la cuisson.
pub struct RtElastique(pub PitchShift);

impl cp_engine::shift::RtShift for RtElastique {
    fn setup(&mut self, srate: f64, nch: usize) {
        let p = &mut self.0;
        p.set_quality(-1);
        p.set_srate(srate);
        p.set_nch(nch as i32);
        p.set_tempo(1.0);
        p.set_shift(1.0);
        p.set_formant_shift(-1.0);
        p.reset();
    }
    fn set_tempo(&mut self, tempo: f64) {
        self.0.set_tempo(tempo);
    }
    fn set_shift(&mut self, shift: f64) {
        self.0.set_shift(shift);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn feed(&mut self, input: &[f64], nch: usize) {
        self.0.feed(input, nch);
    }
    fn flush(&mut self) {
        self.0.flush();
    }
    fn take(&mut self, out: &mut [f64], nch: usize) -> usize {
        self.0.take(out, nch)
    }
}

/// Fait passer `input` (entrelace) dans l'etireur, suivi de `pad` frames de
/// silence (la queue sort) ; rend toute la sortie.
fn run(sh: &mut dyn Shifter, input: &[f32], nch: usize, pad: usize) -> Vec<f64> {
    run_with(sh, input, nch, pad, 1024, 8192)
}

/// [`run`] avec la taille des morceaux donnes et demandes (les essais).
pub fn run_with(
    sh: &mut dyn Shifter,
    input: &[f32],
    nch: usize,
    pad: usize,
    chunk: usize,
    ask: usize,
) -> Vec<f64> {
    #[allow(non_snake_case)]
    let CHUNK = chunk.max(1);
    let mut out = Vec::with_capacity(input.len() * 2);
    let mut buf = vec![0f64; CHUNK * nch];
    let mut tmp = vec![0f64; ask.max(1) * nch];
    let mut drain = |sh: &mut dyn Shifter, out: &mut Vec<f64>| {
        for _ in 0..10_000 {
            let n = sh.take(&mut tmp, nch);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&tmp[..n * nch]);
        }
    };
    for c in input.chunks(CHUNK * nch) {
        for (d, &s) in buf.iter_mut().zip(c) {
            *d = s as f64;
        }
        sh.feed(&buf[..c.len()], nch);
        drain(sh, &mut out);
    }
    buf.iter_mut().for_each(|x| *x = 0.0);
    let mut left = pad;
    while left > 0 {
        let k = left.min(CHUNK);
        sh.feed(&buf[..k * nch], nch);
        drain(sh, &mut out);
        left -= k;
    }
    sh.flush();
    drain(sh, &mut out);
    out
}

/// Le retard de l'etireur ainsi regle, en frames de sortie : un clic place a
/// `at` doit ressortir a `at / tempo` ; on cherche ou il ressort vraiment.
pub fn latency(sh: &mut dyn Shifter, srate: f64, nch: usize, tempo: f64, shift: f64) -> usize {
    // Un taux absurde (un fichier mal lu) : pas de sonde possible.
    if srate.is_nan() || srate < 100.0 {
        return 0;
    }
    let at = (srate * 0.25) as usize;
    let n = (srate * 0.5) as usize;
    let mut probe = vec![0f32; n * nch];
    for c in 0..nch {
        probe[at * nch + c] = 1.0;
    }
    sh.setup(srate, nch, tempo, shift);
    let out = run(sh, &probe, nch, (srate * 0.5) as usize);
    // Le maximum d'energie (les canaux fondus).
    let peak = out
        .chunks(nch)
        .enumerate()
        .map(|(k, f)| (k, f.iter().map(|x| x * x).sum::<f64>()))
        .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a })
        .0;
    let want = (at as f64 / tempo).round() as usize;
    peak.saturating_sub(want)
}

/// Etire `input` (entrelace, `nch` canaux) : sa duree devient exactement
/// `round(frames / tempo)`, sa hauteur est multipliee par `shift`. Le retard
/// de l'etireur est mesure puis retire.
pub fn bake(
    sh: &mut dyn Shifter,
    input: &[f32],
    nch: usize,
    srate: f64,
    tempo: f64,
    shift: f64,
) -> Vec<f32> {
    let nch = nch.max(1);
    let frames = input.len() / nch;
    let want = (frames as f64 / tempo).round() as usize;
    if frames == 0 || want == 0 {
        return Vec::new();
    }
    let d = latency(sh, srate, nch, tempo, shift);
    sh.setup(srate, nch, tempo, shift);
    // Assez de silence derriere pour que la queue et le retard sortent.
    let pad = ((d as f64 * tempo) as usize).max((srate * 0.5) as usize) + 4096;
    let all = run(sh, input, nch, pad);
    LAST[0].store((frames + pad) as u64, Relaxed);
    LAST[1].store((all.len() / nch) as u64, Relaxed);
    LAST[2].store(d as u64, Relaxed);
    let mut out = vec![0f32; want * nch];
    let from = d * nch;
    for (o, &x) in out.iter_mut().zip(all.iter().skip(from)) {
        *o = x as f32;
    }
    out
}

/// Une region etiree, lue par le decodage comme un fichier : la cuisson se
/// fait au premier `read`, sur le fil de travail.
pub struct StretchReader {
    clip: Arc<ClipData>,
    start: usize,
    end: usize,
    tempo: f64,
    shift: f64,
    sh: Option<Box<dyn Shifter>>,
    baked: Option<Vec<f32>>,
}

impl StretchReader {
    /// La region `[start, end)` du clip (`end` = 0 : jusqu'au bout).
    pub fn new(
        clip: Arc<ClipData>,
        start: i64,
        end: i64,
        tempo: f64,
        shift: f64,
        sh: Box<dyn Shifter>,
    ) -> StretchReader {
        let frames = clip.frames().max(0) as usize;
        let e = if end <= 0 {
            frames
        } else {
            (end as usize).min(frames)
        };
        StretchReader {
            start: (start.max(0) as usize).min(e),
            end: e,
            clip,
            tempo: tempo.clamp(0.05, 20.0),
            shift: shift.clamp(0.05, 20.0),
            sh: Some(sh),
            baked: None,
        }
    }

    fn out_frames(&self) -> usize {
        ((self.end - self.start) as f64 / self.tempo).round() as usize
    }

    fn baked(&mut self) -> &[f32] {
        if self.baked.is_none() {
            let nch = self.clip.nch().max(1);
            let data = self.clip.samples();
            let (a, b) = (self.start * nch, (self.end * nch).min(data.len()));
            let v = match self.sh.take() {
                Some(mut sh) if a < b => bake(
                    sh.as_mut(),
                    &data[a..b],
                    nch,
                    self.clip.srate(),
                    self.tempo,
                    self.shift,
                ),
                _ => Vec::new(),
            };
            self.baked = Some(v);
        }
        self.baked.as_deref().unwrap_or(&[])
    }
}

impl Reader for StretchReader {
    fn channels(&mut self) -> i32 {
        self.clip.nch() as i32
    }
    fn sample_rate(&mut self) -> f64 {
        self.clip.srate()
    }
    fn length(&mut self) -> f64 {
        self.out_frames() as f64 / self.clip.srate().max(1.0)
    }
    fn read(&mut self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize {
        let src_nch = self.clip.nch().max(1);
        let total = self.out_frames();
        let first = (time_s * srate).round().max(0.0) as usize;
        let want = out.len() / nch.max(1);
        let b = self.baked();
        let mut k = 0;
        while k < want && first + k < total {
            let f = first + k;
            for c in 0..nch {
                let sc = c.min(src_nch - 1);
                out[k * nch + c] = b.get(f * src_nch + sc).copied().unwrap_or(0.0) as f64;
            }
            k += 1;
        }
        k
    }
}

/// Un etireur d'imitation : un retard fixe (en frames de sortie), puis la
/// matiere lue a `tempo` (interpolation lineaire). La hauteur change avec la
/// vitesse : il ne sert qu'aux tests (le vrai est celui de REAPER).
pub struct LinearShifter {
    pub delay: usize,
    tempo: f64,
    nch: usize,
    input: Vec<f64>,
    made: usize,
    flushed: bool,
}

impl LinearShifter {
    pub fn new(delay: usize) -> LinearShifter {
        LinearShifter {
            delay,
            tempo: 1.0,
            nch: 1,
            input: Vec::new(),
            made: 0,
            flushed: false,
        }
    }
}

impl Shifter for LinearShifter {
    fn setup(&mut self, _srate: f64, nch: usize, tempo: f64, _shift: f64) {
        self.tempo = tempo;
        self.nch = nch;
        self.input.clear();
        self.made = 0;
        self.flushed = false;
    }
    fn feed(&mut self, input: &[f64], _nch: usize) {
        self.input.extend_from_slice(input);
    }
    fn flush(&mut self) {
        self.flushed = true;
    }
    fn take(&mut self, out: &mut [f64], nch: usize) -> usize {
        let fed = self.input.len() / self.nch;
        let mut k = 0;
        while k < out.len() / nch {
            let o = self.made;
            if o < self.delay {
                for c in 0..nch {
                    out[k * nch + c] = 0.0;
                }
            } else {
                let x = (o - self.delay) as f64 * self.tempo;
                let i = x.floor() as usize;
                if i + 1 >= fed && !(self.flushed && i < fed) {
                    break;
                }
                let t = x - i as f64;
                for c in 0..nch {
                    let a = self.input[i * self.nch + c];
                    let b = self.input.get((i + 1) * self.nch + c).copied().unwrap_or(a);
                    out[k * nch + c] = a + (b - a) * t;
                }
            }
            self.made += 1;
            k += 1;
        }
        k
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn la_cuisson_dure_exactement_et_retire_le_retard() {
        // Des clics tous les 4800 echantillons, stereo, 2 s.
        let sr = 48000.0;
        let n = 96000;
        let mut input = vec![0f32; n * 2];
        for k in (2400..n).step_by(4800) {
            input[k * 2] = 1.0;
            input[k * 2 + 1] = -1.0;
        }
        for &(tempo, delay) in &[(0.75, 4321usize), (1.5, 777), (1.0, 0), (0.5, 6000)] {
            let mut sh = LinearShifter::new(delay);
            assert_eq!(latency(&mut sh, sr, 2, tempo, 1.0), delay, "tempo {tempo}");
            let out = bake(&mut sh, &input, 2, sr, tempo, 1.0);
            let want = (n as f64 / tempo).round() as usize;
            assert_eq!(out.len(), want * 2, "tempo {tempo}");
            // Chaque clic tombe a sa place etiree, a un echantillon pres.
            for k in (2400..n).step_by(4800) {
                let at = (k as f64 / tempo).round() as usize;
                let peak = (at.saturating_sub(2)..(at + 3).min(want))
                    .max_by(|&a, &b| out[a * 2].abs().total_cmp(&out[b * 2].abs()))
                    .unwrap();
                assert!(
                    (peak as i64 - at as i64).abs() <= 1 && out[peak * 2] > 0.4,
                    "tempo {tempo} clic {k} : {peak} au lieu de {at}"
                );
            }
        }
    }
}
