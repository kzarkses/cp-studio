//! La synchro a une clock MIDI externe (A7) : CP Studio suit le TEMPO et le
//! DEPART d'une autre machine (Ableton sur un autre PC, une boite a
//! rythmes). La synchro externe de REAPER suit la position, jamais le tempo
//! du projet : tout ce qui se cale sur le tempo restait a l'ancien.
//!
//! Le calcul (la droite sur seize noires, la position du maitre) est dans
//! le chargeur (`cp_engine::extclock`, nourri dans le crochet audio) ; ici,
//! la politique, a chaque battement :
//! - le tempo du projet prend celui du maitre, arrondi au centieme, des que
//!   la fenetre couvre une noire (et pas a chaque frisson de la pente) ;
//! - au Start (ou Continue) du maitre, REAPER part a sa premiere impulsion ;
//!   au Stop, REAPER s'arrete s'il etait parti par lui ;
//! - le tempo du projet ne change QUE a l'arret : en jouant, REAPER garde la
//!   position en secondes et la position en noires saute (mesure dans le
//!   vrai REAPER, la sonde du tempo : -0,2 noire et 73 ms d'arret pour
//!   +1 %) ; alors qu'un changement de VITESSE de lecture est lisse (1,01 ->
//!   1,0092 s par seconde, sans saut) ;
//! - en jouant, la PHASE : une boucle a verrouillage (PLL) sur la vitesse de
//!   lecture. La vitesse vaut le rapport des tempos (le maitre peut changer
//!   de tempo en jouant), plus une correction qui rattrape l'ecart en deux
//!   secondes, bornee a 2 %. L'ecart se mesure a la mesure pres (on se cale
//!   sur la mesure du maitre, pas sur son numero) ; au-dela d'une
//!   demi-noire, on saute.
//!
//! L'ecart compare le bloc que REAPER calcule a la position du maitre PLUS
//! la latence de sortie : ce qu'on calcule maintenant s'entend une latence
//! plus tard, au moment ou le maitre y est. Un retard du maitre lui-meme
//! (sa propre latence) se regle par `delay_ms`.

/// Ce que la synchro demande au transport (REAPER, ou une imitation).
pub(crate) trait Transport {
    fn bpm(&self) -> f64;
    fn set_bpm(&self, bpm: f64);
    fn playing(&self) -> bool;
    fn play(&self);
    fn stop(&self);
    /// La vitesse de lecture (1 = normale).
    fn rate(&self) -> f64;
    fn set_rate(&self, r: f64);
    /// La noire du bloc que REAPER calcule.
    fn qn_now(&self) -> f64;
    /// Sauter a la noire `q`, en jouant.
    fn seek_qn(&self, q: f64);
    /// Les noires d'une mesure, la ou l'on joue.
    fn bar_qn(&self) -> f64;
    /// La latence de sortie, en secondes.
    fn out_latency_s(&self) -> f64;
}

/// Le transport de REAPER.
pub(crate) struct Reaper(pub cp_sys::reaper::Reaper);

impl Transport for Reaper {
    fn bpm(&self) -> f64 {
        self.0.master_tempo()
    }
    fn set_bpm(&self, bpm: f64) {
        self.0.set_bpm(bpm, false);
    }
    fn playing(&self) -> bool {
        self.0.play_state() & cp_sys::transport::play::PLAYING != 0
    }
    fn play(&self) {
        self.0.main_on_command(crate::tview::cmd::PLAY);
    }
    fn stop(&self) {
        self.0.main_on_command(crate::tview::cmd::STOP);
    }
    fn rate(&self) -> f64 {
        self.0.play_rate()
    }
    fn set_rate(&self, r: f64) {
        self.0.set_play_rate(r);
    }
    fn qn_now(&self) -> f64 {
        self.0.time_to_qn(self.0.play_position2())
    }
    fn seek_qn(&self, q: f64) {
        self.0.set_edit_cursor_ex(self.0.qn_to_time(q), true);
    }
    fn bar_qn(&self) -> f64 {
        let (n, d, _) = self.0.time_sig_at(self.0.play_position2());
        (n.max(1) as f64 * 4.0 / d.max(1) as f64).max(0.25)
    }
    fn out_latency_s(&self) -> f64 {
        let (_, out) = cp_sys::audio::AudioHook::io_latency(&self.0);
        match cp_sys::audio::device_srate(&self.0) {
            Some(sr) if sr > 1.0 => out as f64 / sr,
            _ => 0.0,
        }
    }
}

const PRESENT: i32 = 1;
const RUNNING: i32 = 2;
const WAITING: i32 = 16;

/// La PLL : l'ecart se rattrape en `PLL_S` secondes, la correction est
/// bornee a `PLL_MAX` de la vitesse ; au-dela de `JUMP_QN`, on saute ;
/// apres un depart ou un saut, `SETTLE` battements sans mesure (REAPER
/// remplit ses tampons).
const PLL_S: f64 = 2.0;
const PLL_MAX: f64 = 0.02;
const JUMP_QN: f64 = 0.5;
const SETTLE: u32 = 12;

#[derive(Clone, Debug)]
pub(crate) struct ClockSync {
    /// L'entree MIDI ecoutee (-1 : aucune).
    pub dev: i32,
    /// Le dernier tempo pose (pour ne pas le reposer a chaque battement).
    pub bpm_set: f64,
    /// Les Start et Stop deja vus (des compteurs du chargeur).
    starts: u32,
    stops: u32,
    /// Un Start vu, REAPER part a la premiere impulsion.
    pending_start: bool,
    /// REAPER est parti par le maitre (son Stop l'arrete).
    pub started_by_us: bool,
    /// Le premier instantane fixe les compteurs (un Start d'avant n'en est
    /// pas un).
    primed: bool,
    /// Le retard du maitre (ms) : ce qu'il faut ajouter a la latence.
    pub delay_ms: f64,
    /// La PLL : la vitesse d'avant (rendue a l'arret), la derniere posee, le
    /// decalage en mesures entiere entre le maitre et REAPER, l'ecart lisse,
    /// les battements a laisser passer.
    rate0: Option<f64>,
    rate_set: f64,
    offset: Option<f64>,
    err: f64,
    settle: u32,
    was_playing: bool,
    /// L'ecart mesure au dernier battement (noires ; NaN : aucun), et les
    /// sauts faits.
    pub last_err: f64,
    pub jumps: u32,
}

impl Default for ClockSync {
    fn default() -> ClockSync {
        ClockSync {
            dev: -1,
            bpm_set: 0.0,
            starts: 0,
            stops: 0,
            pending_start: false,
            started_by_us: false,
            primed: false,
            delay_ms: 0.0,
            rate0: None,
            rate_set: 1.0,
            offset: None,
            err: 0.0,
            settle: 0,
            was_playing: false,
            last_err: f64::NAN,
            jumps: 0,
        }
    }
}

impl ClockSync {
    /// Une synchro neuve sur l'entree `dev`.
    pub fn on(dev: i32) -> ClockSync {
        ClockSync {
            dev,
            ..Default::default()
        }
    }

    /// L'entree retenue (ExtState), ou aucune.
    pub fn load(r: cp_sys::reaper::Reaper) -> ClockSync {
        let dev = r
            .ext_get(crate::SECTION, "ext_clock_dev")
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1);
        let delay_ms = r
            .ext_get(crate::SECTION, "ext_clock_delay")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        ClockSync {
            dev,
            delay_ms,
            ..Default::default()
        }
    }

    /// Rendre la vitesse d'avant la synchro.
    fn release_rate(&mut self, t: &impl Transport) {
        if let Some(r0) = self.rate0.take() {
            t.set_rate(r0);
        }
        self.rate_set = 1.0;
        self.offset = None;
        self.err = 0.0;
        self.last_err = f64::NAN;
    }

    /// Un battement : `v` = les 14 valeurs de la clock (voir
    /// `cp_abi::AudioApi::clock_read`), ou rien si elle n'a rien publie.
    pub fn step(&mut self, t: &impl Transport, v: Option<[f64; 14]>) {
        let Some(v) = v else { return };
        let (bpm, beats, flags) = (v[0], v[4], v[9] as i32);
        let (starts, stops) = (v[10] as u32, v[11] as u32);
        if !self.primed {
            self.primed = true;
            self.starts = starts;
            self.stops = stops;
        }
        let present = flags & PRESENT != 0;
        let running = flags & RUNNING != 0;
        let waiting = flags & WAITING != 0;
        let playing = t.playing();
        let sure = present && bpm > 20.0 && beats >= 1.0;
        // Le tempo : celui du maitre, au centieme, une fois la fenetre
        // assez large pour que la pente soit sure ; a l'arret seulement.
        if sure && !playing {
            let target = (bpm * 100.0).round() / 100.0;
            if (target - self.bpm_set).abs() >= 0.02 && (t.bpm() - target).abs() >= 0.02 {
                t.set_bpm(target);
                self.bpm_set = target;
            }
        }
        // Le Stop du maitre arrete ce qu'il a lance.
        if stops != self.stops {
            self.stops = stops;
            self.pending_start = false;
            if self.started_by_us && playing {
                t.stop();
            }
            self.started_by_us = false;
        }
        // Le Start : REAPER part a la premiere impulsion qui suit.
        if starts != self.starts {
            self.starts = starts;
            self.pending_start = true;
        }
        if self.pending_start && running && !waiting {
            self.pending_start = false;
            if !t.playing() {
                t.play();
                self.started_by_us = true;
            }
        }
        // La phase : les deux jouent, la pente est sure.
        let playing = t.playing();
        if playing && !self.was_playing {
            self.settle = SETTLE;
        }
        self.was_playing = playing;
        if !(playing && running && !waiting && sure) {
            self.release_rate(t);
            return;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return;
        }
        let proj = t.bpm().max(1.0);
        let lead = t.out_latency_s() + self.delay_ms / 1000.0;
        let mq = v[13] + lead * bpm / 60.0;
        let rq = t.qn_now();
        let bar = t.bar_qn().max(0.25);
        let off = *self
            .offset
            .get_or_insert_with(|| ((rq - mq) / bar).round() * bar);
        let e = mq + off - rq;
        self.last_err = e;
        if self.rate0.is_none() {
            self.rate0 = Some(t.rate());
        }
        let base = bpm / proj;
        if e.abs() > JUMP_QN {
            // Trop loin : un saut, a la vitesse du maitre.
            t.seek_qn(mq + off);
            t.set_rate(base);
            self.rate_set = base;
            self.err = 0.0;
            self.settle = SETTLE;
            self.jumps += 1;
            return;
        }
        self.err += 0.3 * (e - self.err);
        let corr = (self.err / PLL_S * 60.0 / proj).clamp(-PLL_MAX * base, PLL_MAX * base);
        let r = base + corr;
        if (r - self.rate_set).abs() > 1e-4 {
            t.set_rate(r);
            self.rate_set = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        bpm: Cell<f64>,
        playing: Cell<bool>,
        log: RefCell<Vec<String>>,
        rate: Cell<f64>,
        qn: Cell<f64>,
        seeks: Cell<u32>,
    }

    impl Transport for Fake {
        fn bpm(&self) -> f64 {
            self.bpm.get()
        }
        fn set_bpm(&self, bpm: f64) {
            self.bpm.set(bpm);
            self.log.borrow_mut().push(format!("bpm {bpm}"));
        }
        fn playing(&self) -> bool {
            self.playing.get()
        }
        fn play(&self) {
            self.playing.set(true);
            self.log.borrow_mut().push("play".into());
        }
        fn stop(&self) {
            self.playing.set(false);
            self.log.borrow_mut().push("stop".into());
        }
        fn rate(&self) -> f64 {
            self.rate.get()
        }
        fn set_rate(&self, r: f64) {
            self.rate.set(r);
        }
        fn qn_now(&self) -> f64 {
            self.qn.get()
        }
        fn seek_qn(&self, q: f64) {
            self.qn.set(q);
            self.seeks.set(self.seeks.get() + 1);
        }
        fn bar_qn(&self) -> f64 {
            4.0
        }
        fn out_latency_s(&self) -> f64 {
            0.0036
        }
    }

    fn snap(bpm: f64, beats: f64, flags: i32, starts: u32, stops: u32) -> Option<[f64; 14]> {
        let mut v = [0.0; 14];
        v[0] = bpm;
        v[4] = beats;
        v[9] = flags as f64;
        v[10] = starts as f64;
        v[11] = stops as f64;
        Some(v)
    }

    #[test]
    fn la_synchro_suit_le_tempo_le_depart_et_l_arret() {
        let t = Fake::default();
        t.bpm.set(120.0);
        t.rate.set(1.0);
        let mut c = ClockSync {
            dev: 0,
            ..Default::default()
        };
        // Le maitre est la, a 127,994 BPM, la fenetre trop courte : rien.
        c.step(&t, snap(127.994, 0.5, PRESENT, 0, 0));
        assert!(t.log.borrow().is_empty());
        // Une noire : le tempo pris, au centieme ; pas repose ensuite pour un
        // frisson de la pente.
        c.step(&t, snap(127.994, 1.5, PRESENT, 0, 0));
        c.step(&t, snap(128.003, 3.0, PRESENT, 0, 0));
        assert_eq!(*t.log.borrow(), vec!["bpm 127.99".to_string()]);
        // Start : on attend la premiere impulsion, puis REAPER part.
        c.step(&t, snap(128.0, 4.0, PRESENT | RUNNING | WAITING, 1, 0));
        assert!(!t.playing.get());
        c.step(&t, snap(128.0, 4.0, PRESENT | RUNNING, 1, 0));
        assert!(t.playing.get() && c.started_by_us);
        // Stop : REAPER s'arrete.
        c.step(&t, snap(128.0, 4.0, PRESENT, 1, 1));
        assert!(!t.playing.get());
        // Un Start alors que Cedric joue deja : on ne le relance pas, et le
        // Stop ne l'arrete pas.
        t.playing.set(true);
        c.step(&t, snap(128.0, 4.0, PRESENT | RUNNING, 2, 1));
        c.step(&t, snap(128.0, 4.0, PRESENT, 2, 2));
        assert!(t.playing.get());
        assert_eq!(
            *t.log.borrow(),
            vec!["bpm 127.99".to_string(), "play".into(), "stop".into()]
        );
    }

    #[test]
    fn un_start_d_avant_n_en_est_pas_un() {
        // Le maitre jouait deja quand on a commence a ecouter : ses
        // compteurs ne declenchent rien.
        let t = Fake::default();
        let mut c = ClockSync {
            dev: 0,
            ..Default::default()
        };
        c.step(&t, snap(100.0, 4.0, PRESENT | RUNNING, 3, 2));
        assert!(!t.playing.get());
    }

    /// Le maitre et REAPER, simules a la frame : le maitre avance a son
    /// tempo, REAPER a sa vitesse fois le tempo du projet ; les positions
    /// lues le sont au bloc pres (512 frames), comme dans REAPER.
    struct Sim {
        t: Fake,
        c: ClockSync,
        master_qn: f64,
        reaper_qn: f64,
        now_s: f64,
    }

    impl Sim {
        fn new(proj_bpm: f64, reaper_qn: f64) -> Sim {
            let t = Fake::default();
            t.bpm.set(proj_bpm);
            t.rate.set(1.0);
            t.playing.set(true);
            Sim {
                t,
                c: ClockSync {
                    dev: 0,
                    primed: true,
                    ..Default::default()
                },
                master_qn: 0.0,
                reaper_qn,
                now_s: 0.0,
            }
        }

        /// `secs` secondes au battement de 60 Hz ; rend l'ecart vrai le plus
        /// grand (en ms, au tempo du maitre) sur la derniere seconde.
        fn run(&mut self, secs: f64, master_bpm: impl Fn(f64) -> f64) -> f64 {
            let dt = 1.0 / 60.0;
            let block = 512.0 / 48000.0;
            let mut worst = 0.0f64;
            let t_end = self.now_s + secs;
            while self.now_s < t_end {
                let mb = master_bpm(self.now_s);
                // Une seance de 1/60 s : les deux avancent.
                if self.t.seeks.get() > 0 {
                    self.reaper_qn = self.t.qn.get();
                    self.t.seeks.set(0);
                    self.c.jumps += 0;
                }
                self.master_qn += mb / 60.0 * dt;
                self.reaper_qn += self.t.rate.get() * self.t.bpm.get() / 60.0 * dt;
                self.now_s += dt;
                // Ce que chacun lit : au bloc pres.
                let q = |x: f64, bpm: f64| {
                    let s = x * 60.0 / bpm;
                    (s / block).floor() * block * bpm / 60.0
                };
                self.t
                    .qn
                    .set(q(self.reaper_qn, self.t.bpm.get() * self.t.rate.get()));
                let mut v = [0.0; 14];
                v[0] = mb;
                v[4] = 16.0;
                v[9] = (PRESENT | RUNNING) as f64;
                v[13] = q(self.master_qn, mb);
                let seeks_before = self.c.jumps;
                self.c.step(&self.t, Some(v));
                if self.c.jumps != seeks_before {
                    // Le saut : REAPER est ou on l'a mis.
                    self.reaper_qn = self.t.qn.get();
                }
                if t_end - self.now_s < 1.0 {
                    // L'ecart vrai : ce que REAPER fait entendre (une latence
                    // plus tard) contre le maitre, a la mesure pres.
                    let heard =
                        self.reaper_qn - 0.0036 * self.t.bpm.get() * self.t.rate.get() / 60.0;
                    let off = self.c.offset.unwrap_or(0.0);
                    let e = (self.master_qn + off - heard) * 60.0 / mb * 1000.0;
                    worst = worst.max(e.abs());
                }
            }
            worst
        }
    }

    #[test]
    fn la_phase_se_cale_sur_la_mesure_du_maitre() {
        // REAPER part de 16,7 noires (son curseur), le maitre de 0, a
        // 128,4 BPM contre 128 au projet : un saut a la mesure (16), puis la
        // vitesse rattrape le reste.
        let mut s = Sim::new(128.0, 16.7);
        let worst = s.run(6.0, |_| 128.4);
        eprintln!(
            "phase : {worst:.2} ms au pire apres 5 s, vitesse {:.5}",
            s.t.rate.get()
        );
        assert_eq!(s.c.jumps, 1, "un seul saut");
        assert_eq!(s.c.offset, Some(16.0));
        assert!(worst < 2.0, "ecart {worst:.2} ms apres 5 s");
        assert!(
            (s.t.rate.get() - 128.4 / 128.0).abs() < 0.002,
            "vitesse {}",
            s.t.rate.get()
        );
        // Jamais de tempo pose en jouant.
        assert!(
            s.t.log.borrow().iter().all(|l| !l.starts_with("bpm")),
            "{:?}",
            s.t.log.borrow()
        );
    }

    #[test]
    fn le_maitre_accelere_en_jouant() {
        // 128 -> 132 BPM en dix secondes : la vitesse suit, la phase tient,
        // le tempo du projet ne bouge pas ; a l'arret, la vitesse revient et
        // le tempo prend celui du maitre.
        let mut s = Sim::new(128.0, 0.0);
        s.run(3.0, |_| 128.0);
        let worst = s.run(10.0, |t| 128.0 + (t - 3.0).clamp(0.0, 10.0) * 0.4);
        eprintln!("rampe 128 -> 132 : {worst:.2} ms au pire sur la derniere seconde");
        assert!(worst < 5.0, "ecart {worst:.2} ms pendant la rampe");
        assert!((s.t.rate.get() - 132.0 / 128.0).abs() < 0.003);
        assert_eq!(s.c.jumps, 0);
        assert_eq!(s.t.bpm.get(), 128.0, "le tempo du projet n'a pas bouge");
        s.t.playing.set(false);
        let mut v = [0.0; 14];
        v[0] = 132.0;
        v[4] = 16.0;
        v[9] = PRESENT as f64;
        s.c.step(&s.t, Some(v));
        assert_eq!(s.t.rate.get(), 1.0, "la vitesse rendue");
        assert_eq!(s.t.bpm.get(), 132.0, "le tempo du maitre, a l'arret");
    }
}
