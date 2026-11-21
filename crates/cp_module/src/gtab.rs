//! CP Guitar Tab dans REAPER : l'item choisi devient une tablature.
//!
//! 1. L'action lit l'item choisi (son fichier, sa section, sa vitesse) et
//!    ouvre le fichier par REAPER, sur le fil principal.
//! 2. Un fil de travail lit l'audio, le met au format du modele (mono,
//!    22 050 Hz), passe le reseau de Basic Pitch et decode les notes
//!    (`cp_tab`). Rien ne bloque REAPER ; la progression se lit au battement,
//!    et l'icone de CP Transport reste allumee pendant le calcul.
//! 3. Au battement qui suit la fin : une piste MIDI sous l'item (pour ecouter
//!    la transcription contre l'original), la partition en MusicXML (portee
//!    et tablature, le rythme cale sur le tempo du projet) dans le dossier du
//!    projet, ouverte par son application (MuseScore chez Cedric).
//!
//! Le fil ne survit jamais au module : un rechargement l'annule et l'attend.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Instant;

use cp_sys::pcm::CppSource;
use cp_sys::tab::{ItemAudio, MidiNote};
use cp_tab::fret::Tuning;
use cp_tab::score::{ScoreSpec, TabNote, musicxml};
use cp_tab::{Note, NoteParams, Progress, analyze, make_notes};

use crate::{App, SECTION};

/// Ce que rend le fil : la source (detruite sur le fil principal) et les notes.
struct Out {
    src: Option<CppSource>,
    result: Result<Vec<Note>, String>,
    secs: f64,
}

pub(crate) struct Job {
    handle: Option<JoinHandle<Out>>,
    progress: Arc<Progress>,
    item: ItemAudio,
    started: Instant,
    last_pct: i32,
}

fn work(mut src: CppSource, item: ItemAudio, progress: Arc<Progress>) -> Out {
    let sr = src.sample_rate();
    let nch = src.channels().max(1) as usize;
    if sr <= 0.0 {
        return Out {
            src: Some(src),
            result: Err("the file has no sample rate".into()),
            secs: 0.0,
        };
    }
    // La section jouee par l'item, en secondes du fichier.
    let want = (item.len * item.rate * sr).max(0.0) as usize;
    let block = 65536usize;
    let mut buf = vec![0f64; block * nch];
    let mut mono: Vec<f32> = Vec::with_capacity(want);
    let mut at = 0usize;
    while at < want {
        if progress.cancel.load(Ordering::Relaxed) {
            return Out {
                src: Some(src),
                result: Err("cancelled".into()),
                secs: 0.0,
            };
        }
        let n = block.min(want - at);
        let t = item.offs + at as f64 / sr;
        let got = src.read(t, sr, nch, &mut buf[..n * nch]);
        for f in 0..got {
            let mut s = 0f64;
            for c in 0..nch {
                s += buf[f * nch + c];
            }
            mono.push((s / nch as f64) as f32);
        }
        if got < n {
            break;
        }
        at += n;
    }
    let secs = mono.len() as f64 / sr;
    let audio = cp_tab::resample::resample(&mono, sr, cp_tab::model::SR as f64);
    drop(mono);
    let Some(post) = analyze(&audio, 0, &progress) else {
        return Out {
            src: Some(src),
            result: Err("cancelled".into()),
            secs,
        };
    };
    let notes = make_notes(&post, &NoteParams::default());
    Out {
        src: Some(src),
        result: Ok(notes),
        secs,
    }
}

/// Un nom de fichier sans caracteres interdits.
fn clean_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let t = t.trim().trim_end_matches('.').to_string();
    if t.is_empty() { "Guitar Tab".into() } else { t }
}

/// Un chemin libre : « nom.musicxml », sinon « nom (2).musicxml »...
fn free_path(dir: &std::path::Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.musicxml"));
    if !first.exists() {
        return first;
    }
    for i in 2..1000 {
        let p = dir.join(format!("{stem} ({i}).musicxml"));
        if !p.exists() {
            return p;
        }
    }
    first
}

impl App {
    pub(crate) fn gtab_running(&self) -> bool {
        self.gtab.is_some()
    }

    /// L'action : lancer (ou annuler) la transcription de l'item choisi.
    pub(crate) fn gtab_start(&mut self) {
        if let Some(j) = &self.gtab {
            j.progress.cancel.store(true, Ordering::Relaxed);
            self.r
                .show_console_msg("CP Guitar Tab: cancelling the current transcription.\n");
            return;
        }
        let Some(item) = self.r.selected_item_audio() else {
            self.r.show_console_msg(
                "CP Guitar Tab: select an audio item first (a guitar recording), then run Guitar Tab again.\n",
            );
            return;
        };
        let Some(src) = cp_sys::audio::open_file(&self.r, &item.path) else {
            self.r.show_console_msg(&format!(
                "CP Guitar Tab: REAPER cannot open {}\n",
                item.path
            ));
            return;
        };
        let progress = Arc::new(Progress::default());
        let p2 = progress.clone();
        let it2 = item.clone();
        let handle = std::thread::Builder::new()
            .name("cp_guitar_tab".into())
            .spawn(move || work(src, it2, p2))
            .ok();
        if handle.is_none() {
            self.host
                .error("CP Guitar Tab : le fil de travail n'a pas demarre");
            return;
        }
        self.host.info(&format!(
            "Guitar Tab : transcription de \u{ab} {} \u{bb} ({:.1} s)",
            item.name, item.len
        ));
        self.gtab = Some(Job {
            handle,
            progress,
            item,
            started: Instant::now(),
            last_pct: -1,
        });
    }

    /// Le battement : la progression, puis le resultat.
    pub(crate) fn gtab_tick(&mut self) {
        let Some(j) = &mut self.gtab else { return };
        let (done, total) = (
            j.progress.done.load(Ordering::Relaxed),
            j.progress.total.load(Ordering::Relaxed),
        );
        if let Some(pct) = (done * 100).checked_div(total).map(|p| p as i32) {
            if pct / 25 != j.last_pct / 25 {
                j.last_pct = pct;
                self.host.info(&format!("Guitar Tab : {pct} %"));
            }
        }
        if !j.handle.as_ref().is_some_and(|h| h.is_finished()) {
            return;
        }
        let Some(job) = self.gtab.take() else { return };
        let elapsed = job.started.elapsed().as_secs_f64();
        let out = job.handle.and_then(|h| h.join().ok());
        let Some(out) = out else {
            self.r
                .show_console_msg("CP Guitar Tab: the analysis stopped unexpectedly.\n");
            return;
        };
        drop(out.src);
        match out.result {
            Ok(notes) => self.gtab_finish(&job.item, notes, out.secs, elapsed),
            Err(e) => {
                self.r.show_console_msg(&format!("CP Guitar Tab: {e}.\n"));
            }
        }
    }

    /// Un rechargement : annuler et attendre (aucun fil ne survit au module).
    pub(crate) fn gtab_cancel(&mut self) {
        if let Some(j) = self.gtab.take() {
            j.progress.cancel.store(true, Ordering::Relaxed);
            if let Some(h) = j.handle {
                let _ = h.join();
            }
        }
    }

    fn gtab_finish(&mut self, item: &ItemAudio, notes: Vec<Note>, secs: f64, elapsed: f64) {
        let r = self.r;
        // Le temps du fichier -> le temps du projet.
        let end = item.pos + item.len;
        let mut placed: Vec<(f64, f64, &Note)> = notes
            .iter()
            .map(|n| {
                (
                    item.pos + n.t0() / item.rate,
                    item.pos + n.t1() / item.rate,
                    n,
                )
            })
            .filter(|(a, _, _)| *a < end)
            .map(|(a, b, n)| (a, b.min(end), n))
            .collect();
        placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Le nom de la prise, sans l'extension du fichier.
        let mut name = item.name.clone();
        for ext in [
            ".wav", ".flac", ".mp3", ".aif", ".aiff", ".ogg", ".opus", ".wv",
        ] {
            if name.to_lowercase().ends_with(ext) {
                name.truncate(name.len() - ext.len());
                break;
            }
        }
        if name.trim().is_empty() {
            name = "Guitar".to_string();
        }
        // 1. La piste MIDI, sous l'item.
        let midi: Vec<MidiNote> = placed
            .iter()
            .map(|&(a, b, n)| MidiNote {
                t0: a,
                t1: b,
                pitch: n.pitch,
                vel: (40.0 + n.amp.clamp(0.0, 1.0) * 87.0) as i32,
            })
            .collect();
        let midi_res = r.insert_midi_track(
            item.track,
            &format!("Guitar Tab: {name}"),
            item.pos,
            end,
            &midi,
        );
        // 2. La partition.
        let (origin_qn, sig, bpm) = r.measure_at(item.pos);
        let tab: Vec<TabNote> = placed
            .iter()
            .map(|&(a, b, n)| TabNote {
                qn0: r.time_to_qn(a),
                qn1: r.time_to_qn(b),
                pitch: n.pitch,
            })
            .collect();
        let spec = ScoreSpec {
            title: name.clone(),
            origin_qn,
            sig,
            bpm,
            grid: 4,
            tuning: Tuning::standard(),
        };
        let (xml, st) = musicxml(&tab, &spec);
        // Jamais MuseScore ni le dossier de Cedric pendant l'auto-test.
        let selftest = std::env::var("CP_STUDIO_SELFTEST").is_ok();
        let dir = if selftest {
            std::env::temp_dir().join("cp_selftest")
        } else {
            r.project_dir()
                .or_else(|| r.resource_path().map(|p| p.join("CP_Studio")))
                .unwrap_or_default()
        }
        .join("Guitar Tab");
        let _ = std::fs::create_dir_all(&dir);
        let path = free_path(&dir, &clean_name(&name));
        let written = std::fs::write(&path, xml.as_bytes()).is_ok();
        let opened = written && !selftest && cp_sys::tab::open_document(&path);
        let pitches: Vec<String> = placed
            .iter()
            .take(16)
            .map(|&(_, _, n)| n.pitch.to_string())
            .collect();
        let line = format!(
            "notes={} chords={} measures={} dropped={} midi={} pitches={} file={}",
            st.notes,
            st.chords,
            st.measures,
            st.dropped,
            midi_res.is_ok() as u8,
            pitches.join(","),
            path.display()
        );
        self.r.ext_set(SECTION, "selftest_gtab", &line, false);
        self.host.info(&format!(
            "Guitar Tab : {} notes en {:.1} s pour {:.1} s d'audio ; {line}",
            placed.len(),
            elapsed,
            secs
        ));
        let mut msg = format!(
            "CP Guitar Tab: {} notes ({} chords, {} measures) from \"{name}\" in {:.1} s.\n",
            st.notes, st.chords, st.measures, elapsed
        );
        if st.dropped > 0 {
            msg += &format!(
                "  {} notes could not be fingered and were left out.\n",
                st.dropped
            );
        }
        match &midi_res {
            Ok(()) => {
                msg += "  MIDI: a new track \"Guitar Tab\" under the item, to compare by ear.\n"
            }
            Err(e) => msg += &format!("  MIDI: {e}\n"),
        }
        if written {
            msg += &format!("  Score: {}\n", path.display());
            if !opened && !selftest {
                msg += "  (open it in MuseScore: no application is associated with .musicxml)\n";
            }
        } else {
            msg += &format!("  Score: could not write {}\n", path.display());
        }
        self.r.show_console_msg(&msg);
    }
}
