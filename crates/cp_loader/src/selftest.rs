//! L'auto-test dans le VRAI REAPER, en silence.
//!
//! Il ne tourne que si la variable d'environnement `CP_STUDIO_SELFTEST` est
//! posee au lancement de REAPER (`1` : rester ouvert ; `quit` : fermer REAPER a
//! la fin). Cedric ne le declenche jamais par accident.
//!
//! Ce qu'il prouve, et que le faux REAPER ne peut pas prouver :
//! - REAPER decode nos fichiers par `PCM_Source_CreateFromFileEx` et notre
//!   lecture rend CHAQUE echantillon a sa place (WAV flottant 32 bits, et 16
//!   bits a un autre taux) ;
//! - l'apercu materiel est tire une fois par bloc, entre les deux passages du
//!   crochet : les compteurs « trop / pas assez lu » et « rembobine / saute »
//!   restent a zero ;
//! - la part du fil audio que le moteur coute.
//!
//! Tout joue a gain 0 : on n'entend rien. Le resultat va dans le journal et
//! dans `%TEMP%\cp_selftest\result.txt`.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cp_audio::{AudioHost, ClipH, Load};
use cp_engine::VoiceH;

pub struct SelfTest {
    quit: bool,
    t0: Option<Instant>,
    phase: Phase,
    lines: Vec<String>,
    ok: bool,
    files: Vec<TestFile>,
    voices: Vec<VoiceH>,
    loop_clip: Option<ClipH>,
    played_at: Option<Instant>,
}

struct TestFile {
    name: &'static str,
    path: PathBuf,
    want: Vec<f32>,
    nch: usize,
    srate: f64,
    ticket: u32,
    clip: Option<ClipH>,
}

#[derive(PartialEq, Eq, Debug)]
enum Phase {
    Wait,
    Decoding,
    Playing,
    Done,
}

/// `Some` si l'auto-test est demande.
pub fn requested() -> Option<SelfTest> {
    let v = std::env::var("CP_STUDIO_SELFTEST").ok()?;
    Some(SelfTest {
        quit: v.trim().eq_ignore_ascii_case("quit"),
        t0: None,
        phase: Phase::Wait,
        lines: Vec::new(),
        ok: true,
        files: Vec::new(),
        voices: Vec::new(),
        loop_clip: None,
        played_at: None,
    })
}

fn dir() -> PathBuf {
    std::env::temp_dir().join("cp_selftest")
}

/// Une matiere pseudo-aleatoire, exactement representable dans le format vise.
fn noise(frames: usize, nch: usize, seed: u32, bits16: bool) -> Vec<f32> {
    let mut x = seed;
    (0..frames * nch)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = (x >> 8) as f32 / (1u32 << 24) as f32 * 1.8 - 0.9;
            if bits16 {
                (v * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0
            } else {
                v
            }
        })
        .collect()
}

fn wav(samples: &[f32], nch: usize, srate: u32, bits16: bool) -> Vec<u8> {
    let (fmt, bps): (u16, u16) = if bits16 { (1, 16) } else { (3, 32) };
    let data_len = samples.len() * (bps as usize / 8);
    let mut b = Vec::with_capacity(44 + data_len);
    b.extend(b"RIFF");
    b.extend(((36 + data_len) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(fmt.to_le_bytes());
    b.extend((nch as u16).to_le_bytes());
    b.extend(srate.to_le_bytes());
    b.extend((srate * nch as u32 * bps as u32 / 8).to_le_bytes());
    b.extend((nch as u16 * bps / 8).to_le_bytes());
    b.extend(bps.to_le_bytes());
    b.extend(b"data");
    b.extend((data_len as u32).to_le_bytes());
    for &v in samples {
        if bits16 {
            b.extend(((v * 32768.0) as i16).to_le_bytes());
        } else {
            b.extend(v.to_le_bytes());
        }
    }
    b
}

impl SelfTest {
    fn say(&mut self, ok: bool, line: String) {
        if !ok {
            self.ok = false;
        }
        let l = format!("[selftest] {} {line}", if ok { "OK  " } else { "FAIL" });
        crate::log(cp_abi::LOG_INFO, &l);
        self.lines.push(l);
    }

    /// Appele a chaque battement. Rend vrai quand c'est fini.
    pub fn tick(&mut self, a: &mut AudioHost) -> bool {
        let t0 = *self.t0.get_or_insert_with(Instant::now);
        match self.phase {
            Phase::Wait => {
                if t0.elapsed() < Duration::from_secs(1) {
                    return false;
                }
                let _ = fs::create_dir_all(dir());
                let specs: [(&'static str, usize, usize, u32, bool, u32); 2] = [
                    ("float32_stereo_44100.wav", 2, 103_441, 44100, false, 7),
                    ("int16_mono_48000.wav", 1, 77_777, 48000, true, 11),
                ];
                for (name, nch, frames, srate, b16, seed) in specs {
                    let want = noise(frames, nch, seed, b16);
                    let path = dir().join(name);
                    if let Err(e) = fs::write(&path, wav(&want, nch, srate, b16)) {
                        self.say(false, format!("ecriture de {name} : {e}"));
                        continue;
                    }
                    let ticket = a.load_file(&path.to_string_lossy());
                    self.files.push(TestFile {
                        name,
                        path,
                        want,
                        nch,
                        srate: srate as f64,
                        ticket,
                        clip: None,
                    });
                }
                self.phase = Phase::Decoding;
            }
            Phase::Decoding => {
                let mut pending = false;
                let mut results = Vec::new();
                for (i, f) in self.files.iter_mut().enumerate() {
                    if f.clip.is_some() || f.ticket == 0 {
                        continue;
                    }
                    match a.ticket(f.ticket) {
                        Load::Pending => pending = true,
                        Load::Ready(c) => {
                            f.clip = Some(c);
                            f.ticket = 0;
                            results.push((i, Ok(c)));
                        }
                        Load::Failed(e) => {
                            f.ticket = 0;
                            results.push((i, Err(e)));
                        }
                    }
                }
                for (i, r) in results {
                    let (name, path) = (self.files[i].name, self.files[i].path.clone());
                    match r {
                        Err(e) => self.say(false, format!("{name} : {e} ({})", path.display())),
                        Ok(c) => {
                            let f = &self.files[i];
                            let verdict = compare(a, c, &f.want, f.nch, f.srate);
                            let ok = verdict.is_ok();
                            let line = verdict.unwrap_or_else(|e| e);
                            self.say(ok, format!("{name} : {line}"));
                        }
                    }
                }
                // Le delai ne compte que pour un decodage ENCORE en cours : une
                // boite modale (une erreur de script au demarrage) peut geler le
                // battement bien plus longtemps que le decodage n'a dure.
                if !pending {
                    self.start_playing(a);
                } else if t0.elapsed() > Duration::from_secs(30) {
                    self.say(false, "decodage toujours en cours apres 30 s".into());
                    self.start_playing(a);
                }
            }
            Phase::Playing => {
                if self
                    .played_at
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(4))
                {
                    self.finish(a);
                    return true;
                }
            }
            Phase::Done => return true,
        }
        false
    }

    fn start_playing(&mut self, a: &mut AudioHost) {
        let sr = a.core().srate();
        let (s, _) = cp_audio::testloop::synth(sr);
        match a.load_samples(s, 2, sr) {
            Ok(c) => {
                self.loop_clip = Some(c);
                if let Some(v) = a.play(0, c, 0, 0, true, 0.0) {
                    self.voices.push(v);
                }
            }
            Err(e) => self.say(false, format!("boucle de test : {e}")),
        }
        let clips: Vec<ClipH> = self.files.iter().filter_map(|f| f.clip).collect();
        for c in clips {
            if let Some(v) = a.play(1, c, 0, 0, true, 0.0) {
                self.voices.push(v);
            }
        }
        self.played_at = Some(Instant::now());
        self.phase = Phase::Playing;
    }

    fn finish(&mut self, a: &mut AudioHost) {
        let st = a.stats();
        self.say(
            st.hook,
            format!(
                "crochet audio : {}",
                if st.hook { "pose" } else { "REFUSE" }
            ),
        );
        self.say(
            st.calls > 50,
            format!(
                "apercus tires {} fois ({} blocs de {} frames a {} Hz)",
                st.calls, st.blocks, st.last_block, st.srate
            ),
        );
        self.say(
            st.over_pulls == 0 && st.under_pulls == 0,
            format!(
                "lu en trop {} / pas assez {}",
                st.over_pulls, st.under_pulls
            ),
        );
        self.say(
            st.rewinds == 0 && st.skips == 0,
            format!("demandes rembobinees {} / sautees {}", st.rewinds, st.skips),
        );
        self.say(
            st.busy_renders == 0 && st.dropped_commands == 0,
            format!(
                "rendus refuses {} / commandes perdues {}",
                st.busy_renders, st.dropped_commands
            ),
        );
        self.say(
            true,
            format!(
                "charge : {:.4} % du fil audio pour {} voix",
                st.load_percent(),
                st.active_voices
            ),
        );
        for v in self.voices.drain(..) {
            a.stop(v, 0.0);
        }
        if let Some(c) = self.loop_clip.take() {
            a.release(c);
        }
        for f in &self.files {
            if let Some(c) = f.clip {
                a.release(c);
            }
        }
        let verdict = if self.ok { "PASS" } else { "FAIL" };
        crate::log(cp_abi::LOG_INFO, &format!("[selftest] {verdict}"));
        let mut text = self.lines.join("\n");
        text.push_str(&format!("\n{verdict}\n"));
        let _ = fs::write(dir().join("result.txt"), text);
        self.phase = Phase::Done;
    }

    pub fn quit_after(&self) -> bool {
        self.quit
    }
}

fn compare(
    a: &AudioHost,
    c: ClipH,
    want: &[f32],
    nch: usize,
    srate: f64,
) -> Result<String, String> {
    let Some((frames, cn, sr)) = a.clip_info(c) else {
        return Err("clip absent".into());
    };
    let wf = want.len() / nch;
    if cn != nch || sr != srate || frames as usize != wf {
        return Err(format!(
            "forme {frames} frames x {cn} canaux a {sr} Hz, attendu {wf} x {nch} a {srate}"
        ));
    }
    let got = a.clip_samples(c).ok_or("matiere absente")?;
    if let Some(i) = (0..want.len()).find(|&i| got[i].to_bits() != want[i].to_bits()) {
        return Err(format!(
            "premier ecart a l'echantillon {i} (frame {}) : {} au lieu de {}",
            i / nch,
            got[i],
            want[i]
        ));
    }
    Ok(format!("{wf} frames x {nch} canaux, identiques au bit"))
}
