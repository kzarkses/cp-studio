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
//!   au Stop, REAPER s'arrete s'il etait parti par lui.
//!
//! Pas encore : la phase (rattraper un retard d'une fraction de temps par la
//! vitesse de lecture), qui demande la latence de la carte et un essai en
//! vrai avec une machine.

/// Ce que la synchro demande au transport (REAPER, ou une imitation).
pub(crate) trait Transport {
    fn bpm(&self) -> f64;
    fn set_bpm(&self, bpm: f64);
    fn playing(&self) -> bool;
    fn play(&self);
    fn stop(&self);
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
}

const PRESENT: i32 = 1;
const RUNNING: i32 = 2;
const WAITING: i32 = 16;

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
        ClockSync {
            dev,
            ..Default::default()
        }
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
        // Le tempo : celui du maitre, au centieme, une fois la fenetre
        // assez large pour que la pente soit sure.
        if present && bpm > 20.0 && beats >= 1.0 {
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
            if self.started_by_us && t.playing() {
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
}
