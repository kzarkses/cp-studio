//! Le decodage des fichiers, sur un fil de travail.
//!
//! La source est OUVERTE par le fil principal (`cp_sys::audio::open_file`),
//! LUE ici, et rendue au fil principal pour etre DETRUITE : on n'appelle de
//! REAPER hors du fil principal que la lecture d'une source, ce que REAPER fait
//! lui-meme depuis ses fils.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use cp_engine::types::MAX_CHANS;
use cp_sys::pcm::CppSource;

use crate::peaks::Peaks;

/// Ce qu'on sait lire. `CppSource` en vrai ; une matiere synthetique en test.
pub trait Reader: Send {
    fn channels(&mut self) -> i32;
    fn sample_rate(&mut self) -> f64;
    fn length(&mut self) -> f64;
    fn read(&mut self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize;
}

impl Reader for CppSource {
    fn channels(&mut self) -> i32 {
        CppSource::channels(self)
    }
    fn sample_rate(&mut self) -> f64 {
        CppSource::sample_rate(self)
    }
    fn length(&mut self) -> f64 {
        CppSource::length(self)
    }
    fn read(&mut self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize {
        CppSource::read(self, time_s, srate, nch, out)
    }
}

pub struct Decoded {
    pub samples: Vec<f32>,
    pub nch: usize,
    pub srate: f64,
    pub peaks: Peaks,
}

/// Lit tout le fichier, a son propre taux, en au plus [`MAX_CHANS`] canaux.
///
/// Les morceaux font une seconde quand le taux est entier : chaque lecture
/// commence alors a un temps ENTIER, que `time_s * srate` retombe exactement
/// sur l'echantillon, quel que soit l'arrondi de la source.
pub fn decode(r: &mut dyn Reader, max_frames: usize) -> Result<Decoded, String> {
    let srate = r.sample_rate();
    if srate.is_nan() || srate < 1.0 {
        return Err("no audio in this file".into());
    }
    let nch = (r.channels().max(1) as usize).min(MAX_CHANS);
    let len = r.length();
    if len.is_nan() || len <= 0.0 {
        return Err("empty file".into());
    }
    let total = (len * srate).round() as usize;
    if total > max_frames {
        return Err(format!(
            "file too long for now ({:.0} s, the limit is {:.0} s)",
            len,
            max_frames as f64 / srate
        ));
    }
    let chunk = if srate.fract() == 0.0 {
        srate as usize
    } else {
        65536
    };
    let mut buf = vec![0.0f64; chunk * nch];
    let mut out: Vec<f32> = Vec::with_capacity(total * nch);
    let mut pos = 0usize;
    while pos < total {
        let want = chunk.min(total - pos);
        let got = r.read(pos as f64 / srate, srate, nch, &mut buf[..want * nch]);
        out.extend(buf[..got * nch].iter().map(|&v| v as f32));
        pos += got;
        if got < want {
            break;
        }
    }
    if out.is_empty() {
        return Err("the file returned no samples".into());
    }
    let peaks = Peaks::build(&out, nch);
    Ok(Decoded {
        samples: out,
        nch,
        srate,
        peaks,
    })
}

pub struct Job {
    pub ticket: u32,
    pub src: Box<dyn Reader>,
    pub max_frames: usize,
}

pub struct Done {
    pub ticket: u32,
    pub result: Result<Decoded, String>,
    /// La source revient au fil principal, qui la detruit.
    pub src: Box<dyn Reader>,
}

/// Le fil de travail. Il s'arrete quand le `Decoder` est lache.
pub struct Decoder {
    tx: Option<Sender<Job>>,
    rx: Receiver<Done>,
    thread: Option<JoinHandle<()>>,
}

impl Decoder {
    pub fn new() -> Decoder {
        let (tx, jobs) = channel::<Job>();
        let (done_tx, rx) = channel::<Done>();
        let thread = std::thread::Builder::new()
            .name("cp_decode".into())
            .spawn(move || {
                for mut job in jobs {
                    let result = cp_sys::guard(|| decode(job.src.as_mut(), job.max_frames))
                        .unwrap_or_else(|| Err("decoder panicked".into()));
                    let d = Done {
                        ticket: job.ticket,
                        result,
                        src: job.src,
                    };
                    if done_tx.send(d).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Decoder {
            tx: Some(tx),
            rx,
            thread,
        }
    }

    pub fn submit(&self, job: Job) -> bool {
        self.tx.as_ref().is_some_and(|t| t.send(job).is_ok())
    }

    pub fn try_done(&self) -> Option<Done> {
        self.rx.try_recv().ok()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Une source synthetique : la valeur d'un echantillon dit sa place, et la
    /// source arrondit `time_s * srate` comme elle veut (ici : vers le bas, le
    /// pire cas).
    pub struct Fake {
        pub frames: usize,
        pub nch: i32,
        pub srate: f64,
    }

    impl Reader for Fake {
        fn channels(&mut self) -> i32 {
            self.nch
        }
        fn sample_rate(&mut self) -> f64 {
            self.srate
        }
        fn length(&mut self) -> f64 {
            self.frames as f64 / self.srate
        }
        fn read(&mut self, time_s: f64, _srate: f64, nch: usize, out: &mut [f64]) -> usize {
            let start = (time_s * self.srate).floor() as usize;
            let want = out.len() / nch;
            let got = want.min(self.frames.saturating_sub(start));
            for i in 0..got {
                for c in 0..nch {
                    out[i * nch + c] = ((start + i) * 4 + c) as f64 / 1e7;
                }
            }
            got
        }
    }

    #[test]
    fn le_decodage_rend_chaque_echantillon_a_sa_place() {
        for &srate in &[44100.0, 48000.0, 96000.0] {
            let frames = (srate as usize) * 3 + 777;
            let mut f = Fake {
                frames,
                nch: 2,
                srate,
            };
            let d = decode(&mut f, 10_000_000).unwrap();
            assert_eq!(d.nch, 2);
            assert_eq!(d.samples.len(), frames * 2);
            for i in 0..frames {
                for c in 0..2 {
                    let want = ((i * 4 + c) as f64 / 1e7) as f32;
                    assert_eq!(d.samples[i * 2 + c], want, "srate {srate} frame {i}");
                }
            }
        }
    }

    #[test]
    fn trop_long_est_refuse_sans_lire() {
        let mut f = Fake {
            frames: 1000,
            nch: 1,
            srate: 48000.0,
        };
        assert!(decode(&mut f, 999).is_err());
    }

    #[test]
    fn le_fil_rend_la_source() {
        let d = Decoder::new();
        assert!(d.submit(Job {
            ticket: 7,
            src: Box::new(Fake {
                frames: 5000,
                nch: 1,
                srate: 48000.0
            }),
            max_frames: 1 << 20,
        }));
        let t0 = std::time::Instant::now();
        loop {
            if let Some(done) = d.try_done() {
                assert_eq!(done.ticket, 7);
                assert_eq!(done.result.unwrap().samples.len(), 5000);
                break;
            }
            assert!(t0.elapsed().as_secs() < 5);
            std::thread::yield_now();
        }
    }
}
