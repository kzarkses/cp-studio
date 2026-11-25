//! L'accordeur de CP Transport, cote module : quelle entree ecouter (la
//! piste armee, la choisie d'abord), la hauteur (`cp_tab::tuner`), le
//! lissage, et ce que montre le transport.
//!
//! L'entree vient du crochet audio du chargeur (ABI 7) : rien n'est pose sur
//! les pistes.

use std::collections::VecDeque;
use std::time::Instant;

use cp_sys::transport::play;
use cp_tab::tuner::{detect, nearest_string, note_name, note_of};
use cp_transport::{TunerMode, TunerView};

use crate::{App, SECTION};

pub(crate) struct Tuner {
    pub mode: TunerMode,
    /// Le La de reference (Hz).
    pub a4: f64,
    /// L'entree ecoutee (-1 : aucune).
    ch: (i32, i32),
    /// L'accordage de la piste ecoutee (de la corde grave a l'aigue).
    strings: Vec<i32>,
    buf: Vec<f32>,
    hist: VecDeque<f64>,
    last_ok: Option<Instant>,
    since_scan: u32,
    capturing: bool,
    /// La mesure a montrer : l'aiguille la rejoint image par image (une
    /// mesure par battement, l'aiguille sautait a 30 Hz).
    target: Option<TunerView>,
    last_ease: Option<Instant>,
}

impl Tuner {
    pub fn new(r: &cp_sys::reaper::Reaper) -> Tuner {
        Tuner {
            mode: TunerMode::from_key(&r.ext_get(SECTION, "tuner_mode").unwrap_or_default()),
            a4: r
                .ext_get(SECTION, "tuner_a4")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| (400.0..=480.0).contains(v))
                .unwrap_or(440.0),
            ch: (-1, -1),
            strings: cp_tabview::TUNINGS[0].1.to_vec(),
            buf: vec![0.0; 4096],
            hist: VecDeque::new(),
            last_ok: None,
            since_scan: 1000,
            capturing: false,
            target: None,
            last_ease: None,
        }
    }
}

impl App {
    /// L'entree de la piste armee (la choisie d'abord), et son accordage.
    fn tuner_scan(&mut self) {
        let tracks = self
            .r
            .snapshot_tracks(&["CPGT_role", "CPGT_tuning"], &[], &|_| false);
        let armed: Vec<_> = tracks
            .iter()
            .filter(|t| t.armed && t.input >= 0 && t.input & 4096 == 0)
            .collect();
        let pick = armed.iter().find(|t| t.selected).or(armed.first()).copied();
        let tu = &mut self.tp.tu;
        match pick {
            Some(t) => {
                let c0 = t.input & 1023;
                let c1 = if t.input & 1024 != 0 { c0 + 1 } else { -1 };
                if (c0, c1) != tu.ch {
                    tu.hist.clear();
                }
                tu.ch = (c0, c1);
                let k = if t.ext[0].as_deref() == Some("guitar") {
                    t.ext[1]
                        .as_deref()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0)
                } else {
                    0
                };
                tu.strings = cp_tabview::TUNINGS[k.min(cp_tabview::TUNINGS.len() - 1)]
                    .1
                    .to_vec();
            }
            None => tu.ch = (-1, -1),
        }
    }

    fn tuner_show(&mut self, v: Option<TunerView>) {
        self.tp.tu.target = v;
        self.tuner_ease();
    }

    /// Une image : l'aiguille rejoint la mesure (une autre note, ou plus de
    /// note : tout de suite). Rend vrai tant qu'elle bouge.
    pub(crate) fn tuner_ease(&mut self) -> bool {
        let now = Instant::now();
        let dt = self
            .tp
            .tu
            .last_ease
            .map_or(1.0 / 60.0, |t| (now - t).as_secs_f64())
            .clamp(0.001, 0.1);
        self.tp.tu.last_ease = Some(now);
        let target = self.tp.tu.target.clone();
        let cur = self.tp.t.tuner.clone();
        let next = match (&cur, target) {
            (_, None) => None,
            (Some(c), Some(t)) if c.note == t.note && c.octave == t.octave => {
                let k = 1.0 - (-dt / 0.07).exp();
                let cents = c.cents + (t.cents - c.cents) * k as f32;
                let cents = if (t.cents - cents).abs() < 0.05 {
                    t.cents
                } else {
                    cents
                };
                Some(TunerView { cents, ..t })
            }
            (_, Some(t)) => Some(t),
        };
        let moving = match (&next, &self.tp.tu.target) {
            (Some(n), Some(t)) => (n.cents - t.cents).abs() >= 0.05,
            _ => false,
        };
        if next != cur {
            self.tp.t.tuner = next;
            self.tp.t.mark();
        }
        moving
    }

    /// Le battement de l'accordeur.
    pub(crate) fn tuner_tick(&mut self) {
        let mode = self.tp.tu.mode;
        if self.tp.t.tuner_mode != mode {
            self.tp.t.tuner_mode = mode;
            self.tp.t.mark();
        }
        let playing = self.r.play_state() & (play::PLAYING | play::RECORDING) != 0;
        let idle = mode == TunerMode::Off || (mode == TunerMode::Clock && playing);
        if idle {
            if self.tp.tu.capturing {
                self.tp.tu.capturing = false;
                self.host.audio_input(-1, -1, &mut []);
            }
            self.tp.tu.hist.clear();
            self.tuner_show(None);
            return;
        }
        self.tp.tu.since_scan += 1;
        if self.tp.tu.since_scan >= 30 {
            self.tp.tu.since_scan = 0;
            self.tuner_scan();
        }
        let (c0, c1) = self.tp.tu.ch;
        if c0 < 0 {
            if self.tp.tu.capturing {
                self.tp.tu.capturing = false;
                self.host.audio_input(-1, -1, &mut []);
            }
            self.tuner_show(None);
            return;
        }
        self.tp.tu.capturing = true;
        let mut buf = std::mem::take(&mut self.tp.tu.buf);
        let n = self.host.audio_input(c0, c1, &mut buf);
        let sr = self.host.audio_srate();
        let heard = (n >= 2048 && sr > 0.0)
            .then(|| detect(&buf[..n], sr, 30.0, 1400.0))
            .flatten()
            .filter(|p| p.clarity >= 0.88);
        self.tp.tu.buf = buf;
        let tu = &mut self.tp.tu;
        match heard {
            Some(p) => {
                // Une autre note : on repart.
                if let Some(&last) = tu.hist.back() {
                    if (12.0 * (p.freq / last).log2()).abs() > 0.8 {
                        tu.hist.clear();
                    }
                }
                tu.hist.push_back(p.freq);
                while tu.hist.len() > 5 {
                    tu.hist.pop_front();
                }
                tu.last_ok = Some(Instant::now());
            }
            None => {
                let stale = tu.last_ok.is_none_or(|t| t.elapsed().as_secs_f64() > 1.5);
                if stale {
                    tu.hist.clear();
                    self.tuner_show(None);
                }
                return;
            }
        }
        // La mediane des dernieres mesures.
        let mut v: Vec<f64> = tu.hist.iter().copied().collect();
        v.sort_by(f64::total_cmp);
        let f = v[v.len() / 2];
        let (m, cents) = note_of(f, tu.a4);
        let (name, oct) = note_name(m);
        let mf = m as f64 + cents as f64 / 100.0;
        let string = nearest_string(mf, &tu.strings)
            .filter(|&i| tu.strings[i] == m)
            .map(|i| format!("string {}", tu.strings.len() - i))
            .unwrap_or_default();
        self.tuner_show(Some(TunerView {
            note: name.to_string(),
            octave: oct,
            cents,
            freq: f as f32,
            string,
        }));
    }

    /// Le reglage de l'accordeur (menu de la barre).
    pub(crate) fn tuner_set_mode(&mut self, m: TunerMode) {
        self.tp.tu.mode = m;
        self.r.ext_set(SECTION, "tuner_mode", m.key(), true);
        self.tuner_tick();
        self.tp.t.mark();
    }

    pub(crate) fn tuner_ask_a4(&mut self) {
        let cur = format!("{}", self.tp.tu.a4);
        if let Some(v) = self
            .r
            .user_inputs("Tuner reference", 1, "A4 (Hz)", &cur)
            .and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok())
            .filter(|v| (400.0..=480.0).contains(v))
        {
            self.tp.tu.a4 = v;
            self.r.ext_set(SECTION, "tuner_a4", &v.to_string(), true);
        }
    }
}
