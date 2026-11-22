//! Le decodage des fichiers, sur un fil de travail, EN FLUX.
//!
//! La source est OUVERTE par le fil principal (`cp_sys::audio::open_file`),
//! qui lit sa forme (canaux, taux, longueur) et prepare un clip qui se
//! remplira ; elle est LUE ici, une seconde a la fois, dans l'ecrivain du clip ;
//! puis rendue au fil principal pour etre DETRUITE. On n'appelle de REAPER
//! hors du fil principal que la lecture d'une source, ce que REAPER fait
//! lui-meme depuis ses fils.
//!
//! L'ordre : TOUJOURS la demande la plus recente d'abord. En descendant une
//! liste au clavier, le fichier ou l'on s'arrete passe avant ceux qu'on a
//! survoles ; un clip dont plus personne ne veut s'arrete de se remplir.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use cp_engine::types::MAX_CHANS;
use cp_sys::grow::GrowWriter;
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

/// La forme d'un fichier, lue sur le fil principal avant le decodage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub nch: usize,
    pub srate: f64,
    pub frames: usize,
}

/// Lit la forme ; refuse ce qui n'est pas de l'audio.
pub fn shape(r: &mut dyn Reader) -> Result<Shape, String> {
    let srate = r.sample_rate();
    if srate.is_nan() || srate < 1.0 {
        return Err("no audio in this file".into());
    }
    let nch = (r.channels().max(1) as usize).min(MAX_CHANS);
    let len = r.length();
    if len.is_nan() || len <= 0.0 {
        return Err("empty file".into());
    }
    Ok(Shape {
        nch,
        srate,
        frames: (len * srate).round() as usize,
    })
}

/// Un decodage en cours.
pub struct Job {
    pub ticket: u32,
    pub src: Box<dyn Reader>,
    pub shape: Shape,
    pub w: GrowWriter,
}

struct Active {
    job: Job,
    pos: usize,
    buf: Vec<f64>,
    tmp: Vec<f32>,
    first_sent: bool,
}

/// Les morceaux font une seconde quand le taux est entier : chaque lecture
/// commence alors a un temps ENTIER, que `time_s * srate` retombe exactement
/// sur l'echantillon, quel que soit l'arrondi de la source.
pub(crate) fn chunk_frames(srate: f64) -> usize {
    if srate.fract() == 0.0 {
        srate as usize
    } else {
        65536
    }
}

/// Une vignette a calculer : le min et le max de `points` tranches du
/// fichier entier (canaux fondus). Basse priorite : seulement quand aucune
/// ecoute n'attend.
pub struct ThumbJob {
    pub path: String,
    pub src: Box<dyn Reader>,
    pub shape: Shape,
    pub points: usize,
}

struct ThumbActive {
    job: ThumbJob,
    pos: usize,
    acc: Vec<[f32; 2]>,
    buf: Vec<f64>,
}

impl ThumbActive {
    fn new(job: ThumbJob) -> ThumbActive {
        let n = job.points.max(1);
        ThumbActive {
            job,
            pos: 0,
            acc: vec![[f32::MAX, f32::MIN]; n],
            buf: Vec::new(),
        }
    }

    /// Un morceau. Rend vrai si c'est fini.
    fn step(&mut self) -> bool {
        let sh = self.job.shape;
        let chunk = chunk_frames(sh.srate);
        let want = chunk.min(sh.frames.saturating_sub(self.pos));
        if want == 0 {
            return true;
        }
        self.buf.resize(chunk * sh.nch, 0.0);
        let got = self.job.src.read(
            self.pos as f64 / sh.srate,
            sh.srate,
            sh.nch,
            &mut self.buf[..want * sh.nch],
        );
        let n = self.acc.len();
        for f in 0..got {
            let b = ((self.pos + f) * n / sh.frames.max(1)).min(n - 1);
            for c in 0..sh.nch {
                let v = self.buf[f * sh.nch + c] as f32;
                let e = &mut self.acc[b];
                e[0] = e[0].min(v);
                e[1] = e[1].max(v);
            }
        }
        self.pos += got;
        got < want || self.pos >= sh.frames
    }

    fn result(mut self) -> (String, Box<dyn Reader>, Vec<[f32; 2]>) {
        for e in &mut self.acc {
            if e[0] > e[1] {
                *e = [0.0, 0.0];
            }
        }
        (self.job.path, self.job.src, self.acc)
    }
}

/// Le travail confie au fil.
pub enum Work {
    Clip(Job),
    Thumb(ThumbJob),
}

/// Ce que le fil dit au fil principal.
pub enum Msg {
    /// Une vignette (vide si le fichier n'a rien rendu).
    Thumb {
        path: String,
        src: Box<dyn Reader>,
        data: Vec<[f32; 2]>,
    },
    /// Le debut est la : on peut jouer.
    First { ticket: u32 },
    /// Fini (ou abandonne) : la source revient pour etre detruite, avec les
    /// pics si le clip est complet.
    Done {
        ticket: u32,
        src: Box<dyn Reader>,
        peaks: Option<Peaks>,
        error: Option<String>,
    },
}

impl Active {
    /// Un morceau. Rend vrai si c'est fini.
    fn step(&mut self, out: &Sender<Msg>) -> bool {
        let sh = self.job.shape;
        if self.job.w.buf().is_abandoned() {
            return true;
        }
        let chunk = chunk_frames(sh.srate);
        let want = chunk.min(sh.frames - self.pos.min(sh.frames));
        if want == 0 {
            return true;
        }
        self.buf.resize(chunk * sh.nch, 0.0);
        let got = self.job.src.read(
            self.pos as f64 / sh.srate,
            sh.srate,
            sh.nch,
            &mut self.buf[..want * sh.nch],
        );
        self.tmp.clear();
        self.tmp
            .extend(self.buf[..got * sh.nch].iter().map(|&v| v as f32));
        self.job.w.push(&self.tmp);
        self.pos += got;
        if !self.first_sent && got > 0 {
            self.first_sent = true;
            let _ = out.send(Msg::First {
                ticket: self.job.ticket,
            });
        }
        got < want || self.pos >= sh.frames
    }
}

fn finish(a: Active, out: &Sender<Msg>) {
    let Active {
        job, first_sent, ..
    } = a;
    let abandoned = job.w.buf().is_abandoned();
    let empty = job.w.is_empty();
    let peaks = (!abandoned && !empty).then(|| Peaks::build(job.w.buf().filled(), job.shape.nch));
    let error = if empty && !abandoned {
        Some("the file returned no samples".to_string())
    } else {
        None
    };
    let ticket = job.ticket;
    let src = job.src;
    job.w.finish();
    if !first_sent && error.is_none() && !abandoned {
        let _ = out.send(Msg::First { ticket });
    }
    let _ = out.send(Msg::Done {
        ticket,
        src,
        peaks,
        error,
    });
}

fn worker(jobs: Receiver<Work>, out: Sender<Msg>) {
    let mut active: Vec<Active> = Vec::new();
    let mut thumbs: std::collections::VecDeque<ThumbActive> = std::collections::VecDeque::new();
    loop {
        // Recevoir : attendre s'il n'y a rien a faire, sinon juste regarder.
        let next = if active.is_empty() && thumbs.is_empty() {
            match jobs.recv() {
                Ok(j) => Some(j),
                Err(_) => return,
            }
        } else {
            match jobs.recv_timeout(Duration::ZERO) {
                Ok(j) => Some(j),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match next {
            Some(Work::Clip(j)) => {
                active.push(Active {
                    job: j,
                    pos: 0,
                    buf: Vec::new(),
                    tmp: Vec::new(),
                    first_sent: false,
                });
                continue; // d'abord tout recevoir : le plus recent passe devant
            }
            Some(Work::Thumb(t)) => {
                thumbs.push_back(ThumbActive::new(t));
                continue;
            }
            None => {}
        }
        // Les vignettes seulement quand aucune ecoute n'attend.
        if active.is_empty() {
            if let Some(mut t) = thumbs.pop_front() {
                let done = cp_sys::guard(|| t.step()).unwrap_or(true);
                if done {
                    let (path, src, data) = t.result();
                    let _ = out.send(Msg::Thumb { path, src, data });
                } else {
                    thumbs.push_front(t);
                }
            }
            continue;
        }
        // Le plus recent d'abord.
        let Some(mut a) = active.pop() else { continue };
        let done = cp_sys::guard(|| a.step(&out)).unwrap_or(true);
        if done {
            finish(a, &out);
        } else {
            active.push(a);
        }
    }
}

/// Le fil de travail. Il s'arrete quand le `Decoder` est lache.
pub struct Decoder {
    tx: Option<Sender<Work>>,
    rx: Receiver<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Decoder {
    pub fn new() -> Decoder {
        let (tx, jobs) = channel::<Work>();
        let (out, rx) = channel::<Msg>();
        let thread = std::thread::Builder::new()
            .name("cp_decode".into())
            .spawn(move || worker(jobs, out))
            .ok();
        Decoder {
            tx: Some(tx),
            rx,
            thread,
        }
    }

    /// Rend le travail s'il n'a pas pu etre confie.
    pub fn submit(&self, job: Job) -> Result<(), Box<Job>> {
        match &self.tx {
            Some(t) => t.send(Work::Clip(job)).map_err(|e| match e.0 {
                Work::Clip(j) => Box::new(j),
                Work::Thumb(_) => unreachable!("on a envoye un clip"),
            }),
            None => Err(Box::new(job)),
        }
    }

    pub fn submit_thumb(&self, job: ThumbJob) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|t| t.send(Work::Thumb(job)).is_ok())
    }

    pub fn try_msg(&self) -> Option<Msg> {
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
    use cp_sys::grow::GrowBuf;

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

    fn job(f: Fake, ticket: u32) -> (Job, std::sync::Arc<GrowBuf>) {
        let mut f = f;
        let sh = shape(&mut f).unwrap();
        let (buf, w) = GrowBuf::growing(sh.frames * sh.nch);
        (
            Job {
                ticket,
                src: Box::new(f),
                shape: sh,
                w,
            },
            buf,
        )
    }

    #[test]
    fn le_decodage_rend_chaque_echantillon_a_sa_place() {
        for &srate in &[44100.0, 48000.0, 96000.0] {
            let frames = (srate as usize) * 3 + 777;
            let (j, buf) = job(
                Fake {
                    frames,
                    nch: 2,
                    srate,
                },
                1,
            );
            let d = Decoder::new();
            assert!(d.submit(j).is_ok());
            let t0 = std::time::Instant::now();
            let (mut first, mut done) = (false, false);
            while !done {
                match d.try_msg() {
                    Some(Msg::First { .. }) => first = true,
                    Some(Msg::Done { peaks, error, .. }) => {
                        assert!(error.is_none() && peaks.is_some());
                        done = true;
                    }
                    Some(Msg::Thumb { .. }) => {}
                    None => std::thread::yield_now(),
                }
                assert!(t0.elapsed().as_secs() < 10);
            }
            assert!(first && buf.is_done());
            let got = buf.filled();
            assert_eq!(got.len(), frames * 2);
            for i in 0..frames {
                for c in 0..2 {
                    let want = ((i * 4 + c) as f64 / 1e7) as f32;
                    assert_eq!(got[i * 2 + c], want, "srate {srate} frame {i}");
                }
            }
        }
    }

    #[test]
    fn la_demande_la_plus_recente_passe_devant() {
        // Un long fichier, puis un court : le court est fini avant le long.
        let d = Decoder::new();
        let (jl, _bl) = job(
            Fake {
                frames: 48000 * 120,
                nch: 1,
                srate: 48000.0,
            },
            1,
        );
        let (js, _bs) = job(
            Fake {
                frames: 4800,
                nch: 1,
                srate: 48000.0,
            },
            2,
        );
        assert!(d.submit(jl).is_ok());
        assert!(d.submit(js).is_ok());
        let mut order = Vec::new();
        while order.len() < 2 {
            if let Some(Msg::Done { ticket, .. }) = d.try_msg() {
                order.push(ticket);
            }
        }
        assert_eq!(order, vec![2, 1]);
    }

    #[test]
    fn un_clip_abandonne_s_arrete() {
        let d = Decoder::new();
        let frames = 48000 * 400;
        let (j, b) = job(
            Fake {
                frames,
                nch: 1,
                srate: 48000.0,
            },
            1,
        );
        assert!(d.submit(j).is_ok());
        loop {
            if let Some(Msg::First { .. }) = d.try_msg() {
                break;
            }
        }
        b.abandon();
        let t0 = std::time::Instant::now();
        loop {
            if let Some(Msg::Done { peaks, .. }) = d.try_msg() {
                assert!(peaks.is_none());
                break;
            }
            assert!(t0.elapsed().as_secs() < 5, "l'abandon arrete le decodage");
        }
        assert!(b.filled().len() < frames);
    }
}
