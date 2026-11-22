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
use cp_sys::win::{self, Hwnd};

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
    /// Les cuissons en cours : (ticket, tempo, hauteur, frames source,
    /// clics ?), et quand elles ont commence.
    stretches: Vec<(u32, f64, f64, usize, bool)>,
    stretch_at: Option<Instant>,
    /// La piste de sortie (GUID), la voix, l'etape et son heure.
    route: Option<(String, cp_audio::VoiceH, u8, Instant)>,
    /// La position de la voix quand l'etirement temps reel commence.
    rt_pos0: f64,
    /// La prise de l'essai : son numero, son debut, sa fin, son heure.
    rec: Option<(u32, i64, i64, Instant)>,
    ui_at: Option<Instant>,
    keys_step: u8,
    wheels: u32,
    last_tick: Option<Instant>,
    max_gap_ms: f64,
    throttled0: u64,
    docked0: bool,
    /// CP Transport etait-il pose avant le test (on le rend tel quel).
    transport0: bool,
    /// La place du navigateur avant le test.
    place0: String,
    frames0: cp_abi::FrameInfo,
    snapped: bool,
}

struct TestFile {
    name: &'static str,
    path: PathBuf,
    want: Vec<f32>,
    nch: usize,
    srate: f64,
    ticket: u32,
    clip: Option<ClipH>,
    done: bool,
}

#[derive(PartialEq, Eq, Debug)]
enum Phase {
    Wait,
    Decoding,
    Playing,
    /// L'etireur de REAPER : durees exactes, clics a leur place, hauteur.
    Stretch,
    /// Une colonne qui joue DANS une piste, puis la piste effacee en jouant.
    Route,
    /// Une prise de l'entree de la carte : sa duree exacte.
    Record,
    /// La suite se passe hors du moteur audio : insertion, interface.
    Insert,
    OpenUi,
    Watch,
    Dock,
    Keys,
    /// CP Transport a la place du transport de REAPER.
    Transport,
    /// Le navigateur a la place de l'arrangement.
    Band,
    /// Guitar Tab sur un riff connu.
    GuitarTab,
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
        stretches: Vec::new(),
        stretch_at: None,
        route: None,
        rt_pos0: 0.0,
        rec: None,
        ui_at: None,
        keys_step: 0,
        wheels: 0,
        last_tick: None,
        max_gap_ms: 0.0,
        throttled0: 0,
        docked0: false,
        transport0: false,
        place0: String::new(),
        frames0: cp_abi::FrameInfo::default(),
        snapped: false,
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

/// Une cuisson dont le ticket est deja rendu : on garde son clip (marque par
/// ce bit) jusqu'a ce qu'il soit complet.
const STRETCH_CLIP: u32 = 1 << 31;

/// Des attaques : une salve de 10 ms (2 kHz, fenetre de Hann) tous les
/// 4800 echantillons, centree sur 2400 + k x 4800 ; stereo, `n` frames. (Un
/// clic d'un seul echantillon dans le silence n'est pas de la musique :
/// elastique le traite comme du bruit.)
fn clicks(n: usize) -> Vec<f32> {
    let mut v = vec![0f32; n * 2];
    for c in (2400..n).step_by(4800) {
        for j in 0..480usize {
            let k = c - 240 + j;
            if k >= n {
                break;
            }
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * j as f64 / 480.0).cos();
            let x = (std::f64::consts::TAU * 2000.0 * j as f64 / 48000.0).sin() * w * 0.8;
            v[k * 2] = x as f32;
            v[k * 2 + 1] = x as f32;
        }
    }
    v
}

impl SelfTest {
    /// Lance les cuissons : des clics a 0,75 et 1,5 (la duree), un La a 440
    /// Hz monte de trois demi-tons (la hauteur).
    fn start_stretch(&mut self, a: &mut AudioHost) {
        let sr = 48000.0;
        let n = 96000usize;
        if let Ok(c) = a.load_samples(clicks(n), 2, sr) {
            for tempo in [0.75, 1.5] {
                let t = a.stretch(c, 0, 0, tempo, 1.0);
                self.stretches.push((t, tempo, 1.0, n, true));
            }
        }
        let la: Vec<f32> = (0..n)
            .flat_map(|i| {
                let x = (std::f64::consts::TAU * 440.0 * i as f64 / sr).sin() as f32 * 0.5;
                [x, x]
            })
            .collect();
        if let Ok(c) = a.load_samples(la, 2, sr) {
            let shift = 2f64.powf(3.0 / 12.0);
            let t = a.stretch(c, 0, 0, 1.0, shift);
            self.stretches.push((t, 1.0, shift, n, false));
        }
        self.stretch_at = Some(Instant::now());
        self.stretch_probe();
    }

    /// Les essais de l'etireur de REAPER : une sinusoide de 2 s a 0,75, ses
    /// niveaux par quart de seconde, selon la facon de le nourrir.
    fn stretch_probe(&mut self) {
        use cp_audio::stretch::{Shifter, run_with};
        let Some(r) = crate::REAPER.get() else { return };
        let mut modes = Vec::new();
        for m in 0..32 {
            match r.pitch_mode_name(m) {
                Some(n) => modes.push(format!("{m}={n}")),
                None => break,
            }
        }
        crate::log(
            cp_abi::LOG_INFO,
            &format!("[selftest] modes d'etirement : {}", modes.join(", ")),
        );
        let sr = 48000.0;
        let n = 96000usize;
        let sine: Vec<f32> = (0..n)
            .flat_map(|i| {
                let x = (std::f64::consts::TAU * 440.0 * i as f64 / sr).sin() as f32 * 0.5;
                [x, x]
            })
            .collect();
        let Some(mut ps) = r.pitch_shift() else {
            return;
        };
        ps.setup(sr, 2, 0.75, 1.0);
        let out = run_with(&mut ps, &sine, 2, 48000, 1024, 8192);
        let lv: Vec<f64> = out
            .chunks(12000 * 2)
            .map(|c| (c.iter().map(|x| x * x).sum::<f64>() / c.len().max(1) as f64).sqrt())
            .collect();
        // 2 s a 0,75 : 2,67 s de son, dix quarts de seconde pleins.
        let full = lv.iter().take(10).all(|&v| (0.3..0.4).contains(&v));
        self.say(
            full,
            format!(
                "elastique x0.75 sur une sinusoide : {} frames, niveau par quart de seconde {}",
                out.len() / 2,
                lv.iter()
                    .map(|v| format!("{v:.2}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        );
    }

    /// Une piste muette pour les ecoutes (sans envoi au master : rien ne sort
    /// des enceintes), le port 3 joue dedans, a plein volume.
    fn start_route(&mut self, a: &mut AudioHost) {
        let Some(r) = crate::REAPER.get() else { return };
        // Un La d'une seconde, en boucle.
        let la: Vec<f32> = (0..48000)
            .flat_map(|i| {
                let x = (std::f64::consts::TAU * 440.0 * i as f64 / 48000.0).sin() as f32 * 0.5;
                [x, x]
            })
            .collect();
        let Ok(c) = a.load_samples(la, 2, 48000.0) else {
            self.say(false, "sortie dans une piste : clip refuse".into());
            return;
        };
        let Some(g) = r.insert_track(0, "CP selftest route") else {
            self.say(false, "sortie dans une piste : piste refusee".into());
            return;
        };
        // La clock externe ecoute l'entree MIDI 0 pendant cet essai : le
        // vrai objet MIDI de REAPER se lit dans le crochet audio.
        a.set_clock_device(0);
        // Rien ne doit sortir des enceintes de Cedric : le master du projet
        // de test se tait (la piste l'alimente, pour que REAPER la traite).
        r.set_master_mute(true);
        if !a.route_port(3, Some(&g)) || a.port_route(3).as_deref() != Some(g.as_str()) {
            self.say(
                false,
                "sortie dans une piste : PlayTrackPreview2Ex refuse".into(),
            );
            r.delete_track(&g);
            r.set_master_mute(false);
            return;
        }
        match a.play(3, c, 0, 0, true, 1.0) {
            // Le lancement ne doit pas ramener le port a la sortie.
            Some(v) if a.port_route(3).as_deref() == Some(g.as_str()) => {
                self.route = Some((g, v, 0, Instant::now()))
            }
            Some(v) => {
                a.stop(v, 0.0);
                self.say(
                    false,
                    "sortie dans une piste : le lancement a deroute le port".into(),
                );
                r.delete_track(&g);
                r.set_master_mute(false);
            }
            None => {
                self.say(false, "sortie dans une piste : voix refusee".into());
                r.delete_track(&g);
            }
        }
    }

    /// Rend vrai quand c'est fini.
    fn check_route(&mut self, a: &mut AudioHost) -> bool {
        let Some(r) = crate::REAPER.get() else {
            return true;
        };
        let Some((g, v, step, t)) = self.route.clone() else {
            return true;
        };
        if t.elapsed() < Duration::from_millis(700) {
            return false;
        }
        match step {
            0 => {
                // Le son est-il passe par nous ET par la piste ?
                let [l, rr] = a.port_levels(3);
                let tp = r.track_peak(&g);
                let blocks = a.stats().blocks;
                // Sans carte son ouverte, rien ne tourne : on le dit.
                let ok = (l > 0.01 && tp > 0.01) || blocks == 0;
                self.say(
                    ok,
                    format!(
                        "une colonne joue dans une piste : crete du port {l:.3}/{rr:.3}, crete de la piste {tp:.3}{}",
                        if blocks == 0 { " (aucun bloc audio : carte son fermee)" } else { "" }
                    ),
                );
                // Puis elastique EN TEMPS REEL sur la meme voix : tempo 0,8,
                // hauteur +4 demi-tons (1,26).
                a.set(v, cp_engine::Param::Tempo, 0.8);
                a.set(v, cp_engine::Param::Shift, 1.26);
                self.rt_pos0 = a.voice(v).map_or(0.0, |x| x.0);
                self.route = Some((g, v, 1, Instant::now()));
                false
            }
            1 => {
                let [l, rr] = a.port_levels(3);
                let tp = r.track_peak(&g);
                let (made, lent) = a.shifters_in_use();
                let st = a.stats();
                let blocks = st.blocks;
                self.say(
                    (l > 0.05 && tp > 0.05 && made >= 1 && lent >= 1 && st.busy_renders == 0)
                        || blocks == 0,
                    format!(
                        "elastique en temps reel (tempo 0,8, +4 demi-tons) : crete du port {l:.3}/{rr:.3}, de la piste {tp:.3}, {made} etireur(s) dont {lent} prete(s), {} rendu(s) refuse(s)",
                        st.busy_renders
                    ),
                );
                // Silence, puis la piste effacee EN JOUANT.
                a.set(v, cp_engine::Param::Gain, 0.0);
                r.delete_track(&g);
                self.route = Some((g, v, 2, Instant::now()));
                false
            }
            _ => {
                a.poll();
                let back = a.port_route(3).is_none();
                let alive = a.voice(v).is_some();
                self.say(
                    back && a.stats().routes_lost >= 1,
                    format!(
                        "la piste effacee en jouant : le port revient a la sortie materielle ({back}), {} piste(s) perdue(s), la voix vit encore ({alive}), REAPER tient",
                        a.stats().routes_lost
                    ),
                );
                a.stop(v, 0.0);
                a.route_port(3, None);
                r.set_master_mute(false);
                self.route = None;
                let ins = r.midi_inputs();
                let ck = a.clock_read();
                self.say(
                    ck.is_some() || a.stats().blocks == 0,
                    format!(
                        "clock externe : l'entree MIDI 0 lue dans le crochet ({} entree(s) MIDI ; {})",
                        ins.len(),
                        match ck {
                            Some((s, _)) => format!("{} impulsion(s), {} Start", s.ticks, s.starts),
                            None => "rien de publie".into(),
                        }
                    ),
                );
                a.set_clock_device(-1);
                true
            }
        }
    }

    /// Les cuissons finies : rend vrai quand il n'en reste plus.
    fn check_stretch(&mut self, a: &mut AudioHost) -> bool {
        let mut left = Vec::new();
        for (t, tempo, shift, n, is_clicks) in std::mem::take(&mut self.stretches) {
            // Le ticket est pret des la premiere seconde (l'ecoute en flux) :
            // on ne juge qu'un clip entierement cuit.
            let st = if t & STRETCH_CLIP != 0 {
                Load::Ready(ClipH(t & !STRETCH_CLIP))
            } else {
                a.ticket(t)
            };
            match st {
                Load::Pending => left.push((t, tempo, shift, n, is_clicks)),
                Load::Failed(e) => self.say(false, format!("etirement x{tempo} : {e}")),
                Load::Ready(c) if !a.clip_complete(c) => {
                    left.push((c.0 | STRETCH_CLIP, tempo, shift, n, is_clicks));
                }
                Load::Ready(c) => {
                    let want = (n as f64 / tempo).round() as usize;
                    let (frames, nch, _) = a.clip_info(c).unwrap_or((0, 0, 0.0));
                    let s = a.clip_samples(c).unwrap_or(&[]);
                    let mono = |k: usize| s.get(k * nch.max(1)).copied().unwrap_or(0.0);
                    if is_clicks {
                        // Chaque salve : son centre d'energie, a 10 ms pres de
                        // sa place etiree.
                        let mut worst = 0i64;
                        let mut offs: Vec<i64> = Vec::new();
                        for k in (2400..n).step_by(4800) {
                            let at = (k as f64 / tempo).round() as i64;
                            let (mut m0, mut m1) = (0f64, 0f64);
                            for j in (at - 480).max(0)..(at + 480).min(frames) {
                                let e = (mono(j as usize) as f64).powi(2);
                                m0 += e;
                                m1 += e * j as f64;
                            }
                            let c = if m0 > 0.0 {
                                (m1 / m0).round() as i64
                            } else {
                                at + 480
                            };
                            worst = worst.max((c - at).abs());
                            offs.push(c - at);
                        }
                        crate::log(
                            cp_abi::LOG_INFO,
                            &format!("[selftest] elastique x{tempo} : ecarts des salves {offs:?}"),
                        );
                        let ms = worst as f64 / 48.0;
                        let mut abs: Vec<i64> = offs.iter().map(|o| o.abs()).collect();
                        abs.sort_unstable();
                        let med = abs.get(abs.len() / 2).copied().unwrap_or(0) as f64 / 48.0;
                        // Elastique lisse un peu chaque attaque (comme sur un
                        // item de REAPER) : 5 ms au pire, 1 ms en mediane.
                        self.say(
                            frames as usize == want && ms <= 5.0 && med <= 1.0,
                            format!(
                                "elastique x{tempo} : {frames} frames (attendu {want}), attaques a {med:.2} ms de leur place en mediane, {ms:.2} ms au pire"
                            ),
                        );
                    } else {
                        // La hauteur : les passages par zero montants au
                        // milieu.
                        let (a0, b0) = (frames as usize / 4, frames as usize * 3 / 4);
                        let ups: Vec<usize> = (a0 + 1..b0)
                            .filter(|&k| mono(k - 1) < 0.0 && mono(k) >= 0.0)
                            .collect();
                        let hz = if ups.len() > 2 {
                            (ups.len() - 1) as f64 * 48000.0 / (ups[ups.len() - 1] - ups[0]) as f64
                        } else {
                            0.0
                        };
                        let want_hz = 440.0 * shift;
                        self.say(
                            frames as usize == want && (hz / want_hz - 1.0).abs() < 0.01,
                            format!(
                                "elastique +3 demi-tons : {frames} frames (attendu {want}), {hz:.1} Hz (attendu {want_hz:.1})"
                            ),
                        );
                    }
                    a.release(c);
                }
            }
        }
        self.stretches = left;
        if !self.stretches.is_empty()
            && self
                .stretch_at
                .is_some_and(|t| t.elapsed() > Duration::from_secs(20))
        {
            self.say(false, "etirement toujours en cours apres 20 s".into());
            self.stretches.clear();
        }
        self.stretches.is_empty()
    }

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
                        done: false,
                    });
                }
                self.phase = Phase::Decoding;
            }
            Phase::Decoding => {
                let mut pending = false;
                let mut results = Vec::new();
                for (i, f) in self.files.iter_mut().enumerate() {
                    if f.ticket != 0 {
                        match a.ticket(f.ticket) {
                            Load::Pending => {}
                            Load::Ready(c) => {
                                f.clip = Some(c);
                                f.ticket = 0;
                            }
                            Load::Failed(e) => {
                                f.ticket = 0;
                                f.done = true;
                                results.push((i, Err(e)));
                            }
                        }
                    }
                    // Le clip est jouable des sa premiere seconde ; on ne le
                    // compare qu'une fois ENTIEREMENT decode.
                    if let (Some(c), false) = (f.clip, f.done) {
                        if a.clip_complete(c) {
                            f.done = true;
                            results.push((i, Ok(c)));
                        }
                    }
                    if !f.done {
                        pending = true;
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
                    self.audio_report(a);
                    self.start_stretch(a);
                    self.phase = Phase::Stretch;
                }
            }
            Phase::Stretch => {
                if self.check_stretch(a) {
                    self.start_route(a);
                    self.phase = Phase::Route;
                }
            }
            Phase::Route => {
                if self.check_route(a) {
                    // Une prise d'une demi-seconde, dans 0,1 s.
                    let s0 = a.core().clock_now() + 4800;
                    let s1 = s0 + 24000;
                    match a.rec_start(0, 1, Some(s0)) {
                        Some(id) => {
                            a.rec_stop(id, Some(s1));
                            self.rec = Some((id, s0, s1, Instant::now()));
                        }
                        None => self.say(false, "prise : refusee".into()),
                    }
                    self.phase = Phase::Record;
                }
            }
            Phase::Record => {
                let Some((id, s0, s1, t)) = self.rec else {
                    self.phase = Phase::Insert;
                    return false;
                };
                a.poll();
                match a.rec_state(id) {
                    cp_audio::RecState::Ready(c) => {
                        let (frames, nch, _) = a.clip_info(c).unwrap_or((0, 0, 0.0));
                        let lat = crate::REAPER
                            .get()
                            .map_or((0, 0), cp_sys::audio::AudioHook::io_latency);
                        self.say(
                            frames == s1 - s0 && nch == 2,
                            format!(
                                "prise de l'entree 1/2 : {frames} frames (attendu {}), latence entree {} + sortie {} retiree",
                                s1 - s0,
                                lat.0,
                                lat.1
                            ),
                        );
                        a.release(c);
                        self.phase = Phase::Insert;
                    }
                    cp_audio::RecState::Failed(e) => {
                        let rb = a.rec_ring().map(|r| r.last_block());
                        let lat = crate::REAPER
                            .get()
                            .map_or((0, 0), cp_sys::audio::AudioHook::io_latency);
                        self.say(
                            false,
                            format!(
                                "prise : {e} (debut {s0}, fin {s1}, horloge {}, dernier bloc {rb:?}, latence {lat:?})",
                                a.core().clock_now()
                            ),
                        );
                        self.phase = Phase::Insert;
                    }
                    _ if t.elapsed() > Duration::from_secs(5) => {
                        // Sans carte son ouverte, rien n'arrive : on le dit.
                        let blocks = a.stats().blocks;
                        self.say(
                            blocks == 0,
                            format!("prise : toujours en cours apres 5 s ({blocks} blocs audio)"),
                        );
                        self.phase = Phase::Insert;
                    }
                    _ => {}
                }
            }
            Phase::Insert
            | Phase::OpenUi
            | Phase::Watch
            | Phase::Dock
            | Phase::Keys
            | Phase::Transport
            | Phase::Band
            | Phase::GuitarTab
            | Phase::Done => {}
        }
        false
    }

    /// La partie hors moteur audio. Rend vrai quand tout est fini.
    pub fn tick_ui(&mut self) -> bool {
        // Le battement de REAPER lui-meme : le plus grand trou entre deux
        // battements dit si son fil principal a ete gele.
        let now = Instant::now();
        if let Some(t) = self.last_tick {
            self.max_gap_ms = self
                .max_gap_ms
                .max(now.duration_since(t).as_secs_f64() * 1000.0);
        }
        self.last_tick = Some(now);
        let Some(r) = crate::REAPER.get() else {
            return true;
        };
        match self.phase {
            Phase::Insert => {
                if let Some(f) = self.files.first() {
                    let path = f.path.to_string_lossy().into_owned();
                    match r.insert_media(&path, Some((0.5, 1.0))) {
                        Ok(()) => match r.first_selected_item() {
                            Some((pos, len, off)) => {
                                let ok = (len - 0.5).abs() < 1e-9 && (off - 0.5).abs() < 1e-9;
                                self.say(
                                    ok,
                                    format!("insertion d'une section : item a {pos:.3} s, longueur {len:.6} s, decalage {off:.6} s"),
                                );
                            }
                            None => self.say(
                                false,
                                "insertion : aucun item choisi apres InsertMedia".into(),
                            ),
                        },
                        Err(e) => self.say(false, format!("insertion : {e}")),
                    }
                }
                let fx = r.installed_fx();
                self.say(
                    !fx.is_empty(),
                    format!(
                        "effets installes : {} (par exemple « {} »)",
                        fx.len(),
                        fx.first().map_or("", |f| f.0.as_str())
                    ),
                );
                let bpm = r.master_tempo();
                self.say(bpm > 0.0, format!("tempo du projet : {bpm} BPM"));
                self.phase = Phase::OpenUi;
            }
            Phase::OpenUi => {
                let cmd = r.named_command("_CPSTUDIO_BROWSER");
                let perf = r.named_command("_CPSTUDIO_PERF");
                if cmd == 0 {
                    self.say(false, "l'action du navigateur n'existe pas".into());
                    self.finish_report();
                    return true;
                }
                if crate::views_count() == 0 {
                    r.main_on_command(cmd);
                }
                if perf != 0 {
                    r.main_on_command(perf);
                }
                // L'ouverture se fait au battement suivant (jamais dans un
                // message) : on la verifie un peu plus tard.
                self.frames0 = cp_abi::FrameInfo::default();
                self.ui_at = Some(Instant::now());
                self.phase = Phase::Watch;
            }
            Phase::Watch => {
                let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
                if t > 1.5 && !self.snapped {
                    self.snapped = true;
                    self.say(
                        crate::views_count() > 0,
                        format!("le navigateur s'ouvre ({} fenetre)", crate::views_count()),
                    );
                    // Le signal pour la photo (cargo xtask reaper-test) : la
                    // fenetre du navigateur.
                    let h = crate::first_view().map_or(0, |h| h.0);
                    let _ = fs::write(dir().join("snap.txt"), format!("{h}\n"));
                }
                // La mesure commence APRES la photo (elle charge la machine) et
                // dure deux secondes.
                if t > 2.5 && self.frames0.qpc_freq == 0 {
                    self.frames0 = crate::views::frame_info();
                    self.frames0.qpc_freq = self.frames0.qpc_freq.max(1);
                    crate::views::reset_cost();
                    self.max_gap_ms = 0.0;
                    self.throttled0 = crate::views::throttled();
                }
                if t > 4.5 {
                    let f = crate::views::frame_info();
                    let n = f.count.saturating_sub(self.frames0.count);
                    let missed = f.missed.saturating_sub(self.frames0.missed);
                    let hz = if f.period_qpc > 0 {
                        f.qpc_freq as f64 / f.period_qpc as f64
                    } else {
                        0.0
                    };
                    self.say(
                        n > 0,
                        format!("images de l'ecran : {n} en 2 s, cadence mesuree {hz:.1} Hz"),
                    );
                    // Deux causes, deux lignes. Notre dessin doit tenir dans une
                    // periode de l'ecran : c'est NOTRE critere. Un fil principal
                    // de REAPER occupe ailleurs retarde tout ce qui vit sur lui
                    // (la souris comprise) : on le compte et on le dit, sans
                    // le confondre avec le notre.
                    let k = crate::views::cost();
                    let period_ms = if hz > 0.0 { 1000.0 / hz } else { 16.7 };
                    self.say(
                        k.max_draw_ms < period_ms,
                        format!(
                            "dessin du navigateur : {:.1} ms au pire, pour {:.1} ms par image ({} images)",
                            k.max_draw_ms, period_ms, k.frames
                        ),
                    );
                    self.say(
                        true,
                        format!(
                            "images retardees par le fil principal de REAPER : {missed} (retard max {:.1} ms)",
                            k.max_late_ms
                        ),
                    );
                    let docked = crate::first_view().is_some_and(|h| r.dock_of(h).is_some());
                    self.say(
                        self.max_gap_ms < 200.0,
                        format!(
                            "le battement de REAPER pendant l'animation ({}) : plus grand trou {:.0} ms ; images retenues pour lui laisser la main : {}",
                            if docked { "ancree" } else { "flottante" },
                            self.max_gap_ms,
                            crate::views::throttled() - self.throttled0
                        ),
                    );
                    // On remet l'affichage des mesures comme il etait.
                    let perf = r.named_command("_CPSTUDIO_PERF");
                    if perf != 0 {
                        r.main_on_command(perf);
                    }
                    // Puis l'ancrage : on bascule, on verifie, on rebascule
                    // (la preference de Cedric revient telle qu'elle etait).
                    let dock = r.named_command("_CPSTUDIO_BROWSER_DOCK");
                    self.docked0 = crate::first_view().is_some_and(|h| r.dock_of(h).is_some());
                    if dock != 0 && crate::first_view().is_some() {
                        r.main_on_command(dock);
                        self.ui_at = Some(Instant::now());
                        self.snapped = false;
                        self.phase = Phase::Dock;
                        return false;
                    }
                    self.finish_report();
                    return true;
                }
            }
            Phase::Dock => {
                let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
                if t > 1.0 && !self.snapped {
                    self.snapped = true;
                    let docked = crate::first_view().and_then(|h| r.dock_of(h));
                    self.say(
                        docked.is_some() != self.docked0,
                        if self.docked0 {
                            "le navigateur se desancre (flottant)".to_string()
                        } else {
                            format!("le navigateur s'ancre (docker {})", docked.unwrap_or(-1))
                        },
                    );
                    let h = crate::first_view().map_or(0, |h| h.0);
                    let _ = fs::write(dir().join("snap2.txt"), format!("{h}\n"));
                }
                if t > 2.5 {
                    // Rendre a Cedric sa fenetre comme elle etait.
                    let dock = r.named_command("_CPSTUDIO_BROWSER_DOCK");
                    if dock != 0 {
                        r.main_on_command(dock);
                    }
                    self.ui_at = Some(Instant::now());
                    self.keys_step = 9;
                    self.phase = Phase::Keys;
                }
            }
            Phase::Keys if self.keys_step == 9 => {
                let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
                if t > 0.5 {
                    let docked = crate::first_view().is_some_and(|h| r.dock_of(h).is_some());
                    self.say(
                        docked == self.docked0,
                        if self.docked0 {
                            "et revient au docker"
                        } else {
                            "et redevient flottante"
                        }
                        .into(),
                    );
                    // Ce que le docker garde autour de la vue (sa bande
                    // d'onglets) : a 0, la vue va jusqu'au bord.
                    if let Some(h) = crate::first_view().filter(|&h| r.dock_of(h).is_some()) {
                        let (v, p) = (
                            cp_sys::ui::window_rect(h),
                            cp_sys::ui::window_rect(cp_sys::ui::parent(h)),
                        );
                        crate::log(
                            cp_abi::LOG_INFO,
                            &format!(
                                "selftest : marges du docker autour du navigateur : gauche {}, droite {}, haut {}, bas {}",
                                v.left - p.left,
                                p.right - v.right,
                                v.top - p.top,
                                p.bottom - v.bottom
                            ),
                        );
                    }
                    // Puis le clavier, par le vrai chemin des touches.
                    if let Some(h) = crate::first_view() {
                        const WM_KEYDOWN: u32 = 0x100;
                        const WM_KEYUP: u32 = 0x101;
                        for vk in [0x24usize, 0x28, 0x28] {
                            // Debut, Bas, Bas : des dossiers, aucun son.
                            win::post_message(h, WM_KEYDOWN, vk, 1);
                            win::post_message(h, WM_KEYUP, vk, 1);
                        }
                    }
                    self.ui_at = Some(Instant::now());
                    self.keys_step = 0;
                    self.phase = Phase::Keys;
                }
            }
            Phase::Keys => {
                let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
                let state = |r: &cp_sys::reaper::Reaper| {
                    let c = r.named_command("_CPSTUDIO_BROWSER_STATE");
                    if c != 0 {
                        r.main_on_command(c);
                    }
                    r.ext_get("CP_Studio", "selftest_state").unwrap_or_default()
                };
                if self.keys_step == 0 && t > 0.5 {
                    let st = state(r);
                    self.say(
                        st.starts_with("sel=2 "),
                        format!("touches remises a la fenetre (Debut, Bas, Bas) : {st}"),
                    );
                    // La porte du focus vole : REAPER a le focus, le dernier
                    // clic etait chez nous. Une touche qui vise REAPER nous
                    // revient.
                    if let Some(h) = crate::first_view() {
                        crate::views::set_claim(h);
                        win::post_message(r.main_hwnd(), 0x100, 0x28, 1);
                        win::post_message(r.main_hwnd(), 0x101, 0x28, 1);
                    }
                    self.keys_step = 1;
                    self.ui_at = Some(Instant::now());
                } else if self.keys_step == 1 && t > 0.5 {
                    let st = state(r);
                    self.say(
                        st.starts_with("sel=3 "),
                        format!(
                            "touche adressee a REAPER, revenue au navigateur (dernier clic) : {st}"
                        ),
                    );
                    crate::views::set_claim(Hwnd(0));
                    // Le rechargement a chaud, navigateur ouvert : la fenetre
                    // reste, l'etat revient.
                    let before = crate::first_view();
                    let gen0 = crate::module_generation();
                    let ok = matches!(crate::reload(true), Some(Ok(_)));
                    let gen1 = crate::module_generation();
                    let after = crate::first_view();
                    self.say(
                        ok && gen1 == gen0 + 1 && before.is_some() && before == after,
                        format!("rechargement a chaud navigateur ouvert : generation {gen0} -> {gen1}, meme fenetre : {}", before == after),
                    );
                    self.keys_step = 2;
                    self.ui_at = Some(Instant::now());
                } else if self.keys_step == 2 && t > 0.5 {
                    let st = state(r);
                    self.say(
                        st.starts_with("sel=3 "),
                        format!("etat du navigateur apres le rechargement : {st}"),
                    );
                    // Le defilement a la molette : chaque cran redessine TOUTE
                    // la vue (la liste bouge). On mesure le dessin et le
                    // battement de REAPER.
                    crate::views::reset_cost();
                    self.max_gap_ms = 0.0;
                    self.throttled0 = crate::views::throttled();
                    self.keys_step = 3;
                    self.wheels = 0;
                    self.ui_at = Some(Instant::now());
                } else if self.keys_step == 3 {
                    if let Some(h) = crate::first_view() {
                        if self.wheels < 40 {
                            // Bas puis Haut, sur les racines (des dossiers :
                            // aucun son). La molette ne sert pas : REAPER
                            // l'envoie a la fenetre sous la VRAIE souris.
                            let vk = if self.wheels % 2 == 0 {
                                0x28usize
                            } else {
                                0x26
                            };
                            win::post_message(h, 0x100, vk, 1);
                            win::post_message(h, 0x101, vk, 1);
                            self.wheels += 1;
                        } else if t > 2.0 {
                            let k = crate::views::cost();
                            let docked = r.dock_of(h).is_some();
                            let rc = cp_sys::ui::client_rect(h);
                            self.say(
                                self.max_gap_ms < 200.0,
                                format!(
                                    "redessin complet ({}, {}x{} px, {} images) : {:.1} ms en moyenne, {:.1} au pire ; battement de REAPER {:.0} ms au pire ; images retenues {}",
                                    if docked { "ancree" } else { "flottante" },
                                    rc.right - rc.left,
                                    rc.bottom - rc.top,
                                    k.frames,
                                    k.sum_draw_ms / k.frames.max(1) as f64,
                                    k.max_draw_ms,
                                    self.max_gap_ms,
                                    crate::views::throttled() - self.throttled0
                                ),
                            );
                            self.keys_step = 0;
                            self.ui_at = Some(Instant::now());
                            self.phase = Phase::Transport;
                        }
                    }
                }
            }
            Phase::Transport => {
                if self.places_tick(r, true) {
                    self.keys_step = 0;
                    self.ui_at = Some(Instant::now());
                    self.phase = Phase::Band;
                }
            }
            Phase::Band => {
                if self.places_tick(r, false) {
                    self.keys_step = 0;
                    self.ui_at = Some(Instant::now());
                    self.phase = Phase::GuitarTab;
                }
            }
            Phase::GuitarTab => {
                if self.gtab_tick(r) {
                    self.finish_report();
                    return true;
                }
            }
            Phase::Done => return true,
            _ => {}
        }
        false
    }

    /// Guitar Tab de bout en bout : un riff de huit noires (dents de scie a
    /// 112 BPM, les hauteurs connues), pose dans le projet, transcrit par
    /// l'action ; on lit ce que le module a ecrit (ExtState) et le fichier.
    fn gtab_tick(&mut self, r: &cp_sys::reaper::Reaper) -> bool {
        const RIFF: [i32; 8] = [45, 52, 57, 52, 45, 52, 57, 60];
        let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
        match self.keys_step {
            0 => {
                let sr = 44100u32;
                let beat = 60.0 / 112.0;
                let n = (beat * RIFF.len() as f64 * sr as f64) as usize + sr as usize / 2;
                let mut x = vec![0f32; n];
                for (k, &p) in RIFF.iter().enumerate() {
                    let f = 440.0 * 2f64.powf((p - 69) as f64 / 12.0);
                    let a = (k as f64 * beat * sr as f64) as usize;
                    let len = (beat * 0.95 * sr as f64) as usize;
                    for i in 0..len {
                        let ph = (i as f64 * f / sr as f64).fract();
                        let env = (-(i as f64) / sr as f64 * 3.0).exp();
                        x[a + i] += ((2.0 * ph - 1.0) * 0.35 * env) as f32;
                    }
                }
                let path = dir().join("riff.wav");
                let _ = fs::write(&path, wav(&x, 1, sr, false));
                r.ext_set("CP_Studio", "selftest_gtab", "", false);
                r.ext_set("CP_Studio", "selftest_editor", "", false);
                // Le riff seul choisi : InsertMedia AJOUTE a la selection.
                let unsel = r.command_text(40289);
                if !(unsel.to_lowercase().contains("unselect")
                    && unsel.to_lowercase().contains("all items"))
                {
                    self.say(
                        false,
                        format!("40289 n'est pas « unselect all items » : {unsel:?}"),
                    );
                    return true;
                }
                r.main_on_command(40289);
                let p = path.to_string_lossy().into_owned();
                match r.insert_media(&p, None) {
                    Ok(()) => {
                        let c = r.named_command("_CPSTUDIO_GUITARTAB");
                        if c == 0 {
                            self.say(false, "l'action Guitar Tab n'existe pas".into());
                            return true;
                        }
                        r.main_on_command(c);
                    }
                    Err(e) => {
                        self.say(false, format!("Guitar Tab : riff non pose ({e})"));
                        return true;
                    }
                }
                self.keys_step = 1;
                self.ui_at = Some(Instant::now());
            }
            1 => {
                let st = r.ext_get("CP_Studio", "selftest_gtab").unwrap_or_default();
                if st.is_empty() {
                    if t > 40.0 {
                        self.say(false, "Guitar Tab : pas de resultat en 40 s".into());
                        return true;
                    }
                    return false;
                }
                let field = |k: &str| -> String {
                    st.split_whitespace()
                        .find_map(|w| w.strip_prefix(&format!("{k}=")).map(String::from))
                        .unwrap_or_default()
                };
                let pitches: Vec<i32> = field("pitches")
                    .split(',')
                    .filter_map(|v| v.parse().ok())
                    .collect();
                let file = st.split("file=").nth(1).unwrap_or("").to_string();
                let xml = fs::read_to_string(&file).unwrap_or_default();
                self.say(
                    pitches == RIFF,
                    format!("Guitar Tab : hauteurs du riff {pitches:?} (attendu {RIFF:?})"),
                );
                self.say(
                    field("midi") == "1" && xml.contains("<sign>TAB</sign>") && xml.contains("<fret>"),
                    format!(
                        "Guitar Tab : piste MIDI posee, partition ecrite ({} octets) en {t:.1} s ; {}",
                        xml.len(),
                        st.split(" file=").next().unwrap_or("")
                    ),
                );
                // Huit noires a 112 BPM sur la grille : chaque note dure une
                // noire (4 cases de double croche) dans la portee.
                let quarters = xml.matches("<type>quarter</type><staff>1</staff>").count();
                self.say(
                    quarters >= 6,
                    format!("Guitar Tab : rythme, {quarters} noires sur la portee (8 jouees)"),
                );
                // L'editeur, sur le MIDI du riff.
                let ed = r
                    .ext_get("CP_Studio", "selftest_editor")
                    .unwrap_or_default();
                self.say(
                    ed.contains("notes=8")
                        && ed.contains("transpose=1")
                        && ed.contains("canaux=1")
                        && ed.contains("annule=1"),
                    format!("Editeur : un demi-ton plus haut ecrit dans l'item, canaux gardes, annule ; {ed}"),
                );
                return true;
            }
            _ => return true,
        }
        false
    }

    /// Une etape des places natives ; vrai quand la phase est finie.
    fn places_tick(&mut self, r: &cp_sys::reaper::Reaper, transport: bool) -> bool {
        use cp_abi::{PLACE_ARRANGE, PLACE_HELD, PLACE_TRANSPORT};
        let t = self.ui_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
        let act = |name: &str| {
            let c = r.named_command(name);
            if c != 0 {
                r.main_on_command(c);
            }
            c != 0
        };
        let rect_eq = |a: win::Rect, b: win::Rect| {
            (a.left - b.left).abs() <= 1
                && (a.top - b.top).abs() <= 1
                && (a.right - b.right).abs() <= 1
                && (a.bottom - b.bottom).abs() <= 1
        };
        if transport {
            match self.keys_step {
                0 => {
                    // Les actions que pilote le transport, par leur nom.
                    let want = [
                        (40042, "start"),
                        (1007, "play"),
                        (1016, "stop"),
                        (1008, "pause"),
                        (1013, "record"),
                        (1068, "repeat"),
                        (40364, "metronome"),
                        (1134, "tap"),
                        (40021, "project settings"),
                        (40259, "transport"),
                        (40252, "record mode to normal"),
                        (40076, "time selection auto-punch"),
                        (40253, "selected item auto-punch"),
                    ];
                    let mut bad = Vec::new();
                    for (c, k) in want {
                        let n = r.command_text(c);
                        if !n.to_lowercase().contains(k) {
                            bad.push(format!("{c} = {n:?}"));
                        }
                    }
                    self.say(
                        bad.is_empty(),
                        if bad.is_empty() {
                            "les actions du transport portent les noms attendus".into()
                        } else {
                            format!("actions du transport inattendues : {}", bad.join(", "))
                        },
                    );
                    self.transport0 = crate::places::state(PLACE_TRANSPORT).0 == PLACE_HELD;
                    if !self.transport0 && !act("_CPSTUDIO_TRANSPORT") {
                        self.say(false, "l'action de CP Transport n'existe pas".into());
                        return true;
                    }
                    self.keys_step = 1;
                    self.ui_at = Some(Instant::now());
                }
                1 if t > 1.2 => {
                    match crate::places::probe(r, PLACE_TRANSPORT) {
                        Some((shown, want, ours, visible, _)) => self.say(
                            shown && rect_eq(want, ours) && visible == 0,
                            format!(
                                "CP Transport a la place du transport de REAPER : pose {shown}, rectangle {}x{} en {},{} (le notre {}x{} en {},{}), transport natif visible : {visible}",
                                want.right - want.left,
                                want.bottom - want.top,
                                want.left,
                                want.top,
                                ours.right - ours.left,
                                ours.bottom - ours.top,
                                ours.left,
                                ours.top
                            ),
                        ),
                        None => self.say(false, "le transport de REAPER est introuvable".into()),
                    }
                    let h = crate::view_titled("CP Transport").map_or(0, |h| h.0);
                    let _ = fs::write(dir().join("snap3.txt"), format!("{h}\n"));
                    self.keys_step = 2;
                    self.ui_at = Some(Instant::now());
                }
                2 if t > 1.5 => {
                    // Cedric rallume le transport de REAPER : on lui cede.
                    r.main_on_command(40259);
                    self.keys_step = 3;
                    self.ui_at = Some(Instant::now());
                }
                3 if t > 1.0 => {
                    let st = crate::places::state(PLACE_TRANSPORT).0;
                    let gone = crate::view_titled("CP Transport").is_none();
                    let native = crate::places::probe(r, PLACE_TRANSPORT).is_some_and(|p| p.3 == 1);
                    self.say(
                        st != PLACE_HELD && gone && native,
                        format!(
                            "transport de REAPER rallume : CP Transport lui cede la place (etat {st}, fenetre fermee {gone}, natif visible {native})"
                        ),
                    );
                    if self.transport0 {
                        act("_CPSTUDIO_TRANSPORT");
                    }
                    self.keys_step = 4;
                    self.ui_at = Some(Instant::now());
                }
                4 if t > 1.0 => return true,
                _ => {}
            }
            return false;
        }
        match self.keys_step {
            0 => {
                self.place0 = r.ext_get("CP_Studio", "browser_place").unwrap_or_else(|| {
                    if r.ext_get("CP_Studio", "browser_docked").as_deref() == Some("1") {
                        "dock".into()
                    } else {
                        "float".into()
                    }
                });
                act("_CPSTUDIO_BROWSER_IN_ARRANGE");
                self.keys_step = 1;
                self.ui_at = Some(Instant::now());
            }
            1 if t > 1.5 => {
                match crate::places::probe(r, PLACE_ARRANGE) {
                    Some((shown, want, ours, visible, _)) => self.say(
                        shown && rect_eq(want, ours) && visible == 0,
                        format!(
                            "le navigateur a la place de l'arrangement : pose {shown}, bande {}x{} en {},{} (le notre {}x{}), vues natives encore visibles : {visible}",
                            want.right - want.left,
                            want.bottom - want.top,
                            want.left,
                            want.top,
                            ours.right - ours.left,
                            ours.bottom - ours.top
                        ),
                    ),
                    None => self.say(false, "l'arrangement est introuvable".into()),
                }
                let h = crate::first_view().map_or(0, |h| h.0);
                let _ = fs::write(dir().join("snap4.txt"), format!("{h}\n"));
                if let Some(h) = crate::first_view() {
                    // Debut, Bas : les touches arrivent a la vue posee.
                    for vk in [0x24usize, 0x28] {
                        win::post_message(h, 0x100, vk, 1);
                        win::post_message(h, 0x101, vk, 1);
                    }
                }
                self.keys_step = 2;
                self.ui_at = Some(Instant::now());
            }
            2 if t > 1.5 => {
                let c = r.named_command("_CPSTUDIO_BROWSER_STATE");
                if c != 0 {
                    r.main_on_command(c);
                }
                let st = r.ext_get("CP_Studio", "selftest_state").unwrap_or_default();
                self.say(
                    st.starts_with("sel=1 ") && st.contains("band=1"),
                    format!("touches de la vue posee (Debut, Bas) : {st}"),
                );
                // Le rechargement a chaud : la vue reste a sa place.
                let before = crate::first_view();
                let ok = matches!(crate::reload(true), Some(Ok(_)));
                let held = crate::places::state(PLACE_ARRANGE);
                self.say(
                    ok && before.is_some_and(|h| held.0 == PLACE_HELD && held.1 == h.0),
                    "rechargement a chaud : le navigateur reste a la place de l'arrangement".into(),
                );
                act("_CPSTUDIO_VIEW_ARRANGE");
                self.keys_step = 3;
                self.ui_at = Some(Instant::now());
            }
            3 if t > 1.0 => {
                let p = crate::places::probe(r, PLACE_ARRANGE);
                let parked = crate::first_view().is_some_and(|h| !cp_sys::ui::is_visible(h));
                self.say(
                    p.is_some_and(|p| !p.0 && p.3 > 0) && parked,
                    format!(
                        "« Arrange » : l'arrangement revient ({} vues natives visibles), le navigateur est gare : {parked}",
                        p.map_or(0, |p| p.3)
                    ),
                );
                act("_CPSTUDIO_VIEW_BROWSER");
                self.keys_step = 4;
                self.ui_at = Some(Instant::now());
            }
            4 if t > 1.0 => {
                let held = crate::places::state(PLACE_ARRANGE).0 == PLACE_HELD;
                self.say(
                    held,
                    "« Browser » : le navigateur gare revient a sa place".into(),
                );
                // Rendre a Cedric sa place d'origine.
                match self.place0.as_str() {
                    "dock" => {
                        act("_CPSTUDIO_BROWSER_DOCKED");
                    }
                    "float" => {
                        act("_CPSTUDIO_BROWSER_FLOATING");
                    }
                    _ => {}
                }
                self.keys_step = 6;
                self.ui_at = Some(Instant::now());
            }
            6 if t > 1.5 => {
                let p = crate::places::probe(r, PLACE_ARRANGE);
                let place = r.ext_get("CP_Studio", "browser_place").unwrap_or_default();
                let back = place == self.place0;
                self.say(
                    back && (self.place0 == "arrange" || p.is_some_and(|p| !p.0 && p.3 > 0)),
                    format!("le navigateur retrouve sa place ({place}), l'arrangement est rendu"),
                );
                return true;
            }
            _ => {}
        }
        false
    }

    pub fn wants_audio(&self) -> bool {
        matches!(
            self.phase,
            Phase::Wait
                | Phase::Decoding
                | Phase::Playing
                | Phase::Stretch
                | Phase::Route
                | Phase::Record
        )
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
        // La vignette du premier fichier (basse priorite : apres les clips).
        if let Some(f) = self.files.first() {
            let _ = a.thumb(&f.path.to_string_lossy());
        }
        self.played_at = Some(Instant::now());
        self.phase = Phase::Playing;
    }

    fn audio_report(&mut self, a: &mut AudioHost) {
        if let Some(f) = self.files.first() {
            let p = f.path.to_string_lossy().into_owned();
            let peak = f.want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            match a.thumb(&p) {
                cp_audio::Thumb::Ready(d) => {
                    let hi = d.iter().fold(0.0f32, |m, e| m.max(e[1]).max(-e[0]));
                    self.say(
                        d.len() == cp_audio::THUMB_POINTS && (hi - peak).abs() < 1e-6,
                        format!(
                            "vignette : {} tranches, crete {hi} (fichier : {peak})",
                            d.len()
                        ),
                    );
                }
                other => self.say(false, format!("vignette : {other:?}")),
            }
        }
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
    }

    fn finish_report(&mut self) {
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
