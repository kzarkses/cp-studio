//! Les reglages de FX Constellation : un popover a sections
//! ([`cp_ui::pop::Panel`]) a la place des menus de Windows (maquette
//! validee par Cedric le 2026-11-25). Chaque reglage s'applique tout de
//! suite ; le panneau se reconstruit de l'etat.
//!
//! - Pad : le slew des sorties du pad.
//! - ROLL : ce qu'il tire (combien de parametres, portees, bases, bypass,
//!   modulation, X/Y exclusif, les modulateurs).
//! - Parameters : les mots qui cachent un parametre (une liste a cocher, un
//!   mot a ajouter ; plus un champ de texte a virgules), montrer les caches,
//!   les valeurs.
//! - After insert : la post-insertion du navigateur d'effets.
//! - Window : la place, rester sur la piste, suivre la maitre.

use cp_fxc::roll::Settings;
use cp_fxcview::Ctl;
use cp_fxlist::PostParams;
use cp_ui::Icon;
use cp_ui::pop::{Out, Panel, Pop, Row, Section};

use crate::{App, Place, WinOp};

/// Les popovers de FX Constellation : une question (rejouee), l'aide, les
/// reglages.
pub(crate) const POP_ASK: u32 = 1;
pub(crate) const POP_SETTINGS: u32 = 2;
pub(crate) const POP_HELP: u32 = 3;

/// Les sections, dans l'ordre du rail (Pad, ROLL, Parameters...).
pub(crate) const SECTION_ROLL: usize = 1;
pub(crate) const SECTION_PARAMS: usize = 2;

const SLEW: u32 = 10;
const R_COUNT: u32 = 20;
const R_RANGE: u32 = 21;
const R_BASE: u32 = 22;
const R_INT: u32 = 23;
const R_BYP: u32 = 24;
const R_MOD: u32 = 25;
const R_EXCL: u32 = 26;
const R_SLOTS: u32 = 27;
const R_RAND: u32 = 28;
const HIDE: u32 = 30;
const SHOW_HIDDEN: u32 = 31;
const SHOW_VALUES: u32 = 32;
const P_PARAMS: u32 = 40;
const P_N: u32 = 41;
const P_CHIPS: u32 = 42;
const P_BYPASS: u32 = 43;
const W_PLACE: u32 = 50;
const W_LOCK: u32 = 51;
const W_MASTER: u32 = 52;

/// ROLL tire de 1 a 40 parametres.
const MAX_TRACKED: f64 = 40.0;

fn pct(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

fn span(a: f64, b: f64) -> String {
    format!("{a:.2} – {b:.2}")
}

fn toggle(id: u32, label: &str, on: bool) -> Row {
    Row::Toggle {
        id,
        label: label.into(),
        on,
    }
}

fn slider(id: u32, label: &str, v: f64, text: String) -> Row {
    Row::Slider {
        id,
        label: label.into(),
        v,
        text,
    }
}

fn range(id: u32, label: &str, lo: f64, hi: f64, text: String) -> Row {
    Row::Range {
        id,
        label: label.into(),
        lo,
        hi,
        text,
    }
}

fn count_of(n: f64) -> usize {
    (1.0 + (n * (MAX_TRACKED - 1.0)).round()) as usize
}

fn norm_of(n: usize) -> f64 {
    (n.max(1) as f64 - 1.0) / (MAX_TRACKED - 1.0)
}

fn roll_rows(s: &Settings) -> Vec<Row> {
    vec![
        Row::Head("What ROLL draws".into()),
        range(
            R_COUNT,
            "Parameters tracked",
            norm_of(s.random_min),
            norm_of(s.random_max),
            format!("{} – {}", s.random_min, s.random_max),
        ),
        range(
            R_RANGE,
            "Range span",
            s.range_min,
            s.range_max,
            span(s.range_min, s.range_max),
        ),
        range(
            R_BASE,
            "Base span",
            s.base_min,
            s.base_max,
            span(s.base_min, s.base_max),
        ),
        slider(R_INT, "Base intensity", s.intensity, pct(s.intensity)),
        slider(R_BYP, "Bypass", s.bypass_pct, pct(s.bypass_pct)),
        slider(R_MOD, "Modulated", s.mod_prob, pct(s.mod_prob)),
        toggle(R_EXCL, "Exclusive X / Y", s.exclusive_xy),
        Row::Head("Modulators".into()),
        Row::Seg {
            id: R_SLOTS,
            label: "Slots".into(),
            opts: (1..=8).map(|n| n.to_string()).collect(),
            cur: s.lfo_slots.clamp(1, 8) - 1,
        },
        Row::Chips {
            id: R_RAND,
            label: "Randomize".into(),
            chips: vec![
                ("Shape".into(), s.lfo_shape),
                ("Rate".into(), s.lfo_rate),
                ("Sync".into(), s.lfo_sync),
                ("Phase".into(), s.lfo_phase),
            ],
        },
    ]
}

impl App {
    /// Ouvre les reglages sur une section, sous le bouton `under` (ou a la
    /// souris).
    pub(crate) fn fxc_settings_open(&mut self, section: usize, under: &Ctl) {
        let (x, y) = self
            .fxc
            .view
            .items()
            .iter()
            .rev()
            .find(|i| &i.ctl == under)
            .map(|i| (i.r.right(), i.r.bottom() + 6.0))
            .unwrap_or(self.fxc.view.mouse);
        let mut p = Pop::panel(POP_SETTINGS, self.fxc_settings_panel(), x, y);
        p.set_section(section);
        self.fxc.settings_section = section;
        self.fxc.view.open_pop(p);
        self.fxc_kick();
    }

    /// Le nombre de parametres des effets de la piste dont le nom contient
    /// `w` (ce que ce mot cache).
    fn fxc_hidden_by(&self, w: &str) -> usize {
        self.fxc
            .chain
            .fx
            .iter()
            .filter(|f| f.kind == Some(cp_fxc::slots::FxKind::User))
            .filter_map(|f| self.fxc.names.get(&(f.guid.clone(), f.nparams)))
            .map(|names| names.iter().filter(|(n, _)| n.contains(w)).count())
            .sum()
    }

    /// Le panneau, tire de l'etat.
    pub(crate) fn fxc_settings_panel(&self) -> Panel {
        let post = self.fxb.st.set.post;
        let (pi, n) = match post.params {
            PostParams::None => (0, 4),
            PostParams::All => (1, 4),
            PostParams::Continuous => (2, 4),
            PostParams::Random(n) => (3, n),
        };
        let mut insert = vec![
            Row::Head("An FX added from the FX Browser".into()),
            Row::Seg {
                id: P_PARAMS,
                label: "Track parameters".into(),
                opts: ["None", "All", "Continuous", "Random"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                cur: pi,
            },
        ];
        if pi == 3 {
            insert.push(Row::Step {
                id: P_N,
                label: "How many".into(),
                text: n.to_string(),
            });
        }
        insert.push(Row::Chips {
            id: P_CHIPS,
            label: "Then at random".into(),
            chips: vec![
                ("X/Y".into(), post.xy),
                ("Range".into(), post.range),
                ("Base".into(), post.base),
                ("Invert".into(), post.invert),
            ],
        });
        insert.push(toggle(P_BYPASS, "Insert bypassed", post.bypass));
        let slew = self.fxc.t.pad.link_slew;
        let words = self
            .fxc
            .hide_list
            .iter()
            .map(|(w, on)| (w.clone(), *on, self.fxc_hidden_by(w).to_string()))
            .collect();
        Panel {
            title: "Settings".into(),
            icon: Icon::Settings,
            sections: vec![
                Section {
                    label: "Pad".into(),
                    icon: Icon::Crosshair,
                    rows: vec![
                        Row::Head("Motion".into()),
                        slider(
                            SLEW,
                            "Slew",
                            slew.clamp(0.0, 1.0),
                            format!("{:.0} ms", slew * 1000.0),
                        ),
                    ],
                },
                Section {
                    label: "ROLL".into(),
                    icon: Icon::Dices,
                    rows: roll_rows(&self.fxc.roll),
                },
                Section {
                    label: "Parameters".into(),
                    icon: Icon::ListX,
                    rows: vec![
                        Row::Head("Hidden parameters".into()),
                        Row::Checks {
                            id: HIDE,
                            items: words,
                            add: "Add a word".into(),
                            removable: true,
                        },
                        toggle(SHOW_HIDDEN, "Show hidden parameters", self.fxc.show_hidden),
                        toggle(SHOW_VALUES, "Show values", self.fxc.view.show_values),
                    ],
                },
                Section {
                    label: "After insert".into(),
                    icon: Icon::Download,
                    rows: insert,
                },
                Section {
                    label: "Window".into(),
                    icon: Icon::Maximize,
                    rows: vec![
                        Row::Head("Window".into()),
                        Row::Seg {
                            id: W_PLACE,
                            label: "Place".into(),
                            opts: ["Arrange", "Dock", "Float"]
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                            cur: match self.fxc.place {
                                Place::Arrange => 0,
                                Place::Dock => 1,
                                Place::Float => 2,
                            },
                        },
                        toggle(W_LOCK, "Stay on this track", self.fxc.locked),
                        toggle(W_MASTER, "Follow the master", self.fxc.follow_master),
                    ],
                },
            ],
        }
    }

    /// Les mots caches ont change : ecrits, et la liste des parametres
    /// relue.
    fn fxc_hide_changed(&mut self) {
        self.fxc.hide_words = self
            .fxc
            .hide_list
            .iter()
            .filter(|w| w.1)
            .map(|w| w.0.clone())
            .collect();
        let t = crate::fxc::hide_text(&self.fxc.hide_list);
        self.fxc_keep("fxc_hide", &t);
        self.fxc_scan_chain();
    }

    /// Un reglage du panneau : applique, puis le panneau se refait.
    pub(crate) fn fxc_settings_out(&mut self, out: Out) {
        if let Some(i) = self.fxc.view.pop.as_ref().and_then(|p| p.section()) {
            self.fxc.settings_section = i;
        }
        let r = &mut self.fxc.roll;
        let mut roll = true;
        match out {
            Out::Closed => return,
            Out::Slide { id: SLEW, v, .. } => {
                self.fxc.t.pad.link_slew = (v * 1000.0).round() / 1000.0;
                self.fxc_changed();
                roll = false;
            }
            Out::Range {
                id: R_COUNT,
                lo,
                hi,
                ..
            } => {
                r.random_min = count_of(lo);
                r.random_max = count_of(hi).max(r.random_min);
            }
            Out::Range {
                id: R_RANGE,
                lo,
                hi,
                ..
            } => {
                r.range_min = (lo * 100.0).round() / 100.0;
                r.range_max = (hi * 100.0).round() / 100.0;
            }
            Out::Range {
                id: R_BASE, lo, hi, ..
            } => {
                r.base_min = (lo * 100.0).round() / 100.0;
                r.base_max = (hi * 100.0).round() / 100.0;
            }
            Out::Slide { id: R_INT, v, .. } => r.intensity = (v * 100.0).round() / 100.0,
            Out::Slide { id: R_BYP, v, .. } => r.bypass_pct = (v * 100.0).round() / 100.0,
            Out::Slide { id: R_MOD, v, .. } => r.mod_prob = (v * 100.0).round() / 100.0,
            Out::Toggle(R_EXCL) => r.exclusive_xy = !r.exclusive_xy,
            Out::Seg(R_SLOTS, k) => r.lfo_slots = k + 1,
            Out::Chip(R_RAND, k) => match k {
                0 => r.lfo_shape = !r.lfo_shape,
                1 => r.lfo_rate = !r.lfo_rate,
                2 => r.lfo_sync = !r.lfo_sync,
                _ => r.lfo_phase = !r.lfo_phase,
            },
            other => {
                roll = false;
                self.fxc_settings_other(other);
            }
        }
        if roll {
            self.fxc.prefs_dirty = true;
        }
        let p = self.fxc_settings_panel();
        if let Some(pop) = self.fxc.view.pop.as_mut() {
            pop.set_panel(p);
        }
        self.fxc.view.mark();
        self.fxc_kick();
    }

    fn fxc_settings_other(&mut self, out: Out) {
        let post = &mut self.fxb.st.set.post;
        match out {
            Out::Check(HIDE, k) => {
                if let Some(w) = self.fxc.hide_list.get_mut(k) {
                    w.1 = !w.1;
                }
                self.fxc_hide_changed();
            }
            Out::Remove(HIDE, k) => {
                if k < self.fxc.hide_list.len() {
                    self.fxc.hide_list.remove(k);
                }
                self.fxc_hide_changed();
            }
            Out::Add(HIDE, w) => {
                let w = w.replace([',', '!'], " ").trim().to_string();
                if !w.is_empty() {
                    match self.fxc.hide_list.iter_mut().find(|x| x.0 == w) {
                        Some(x) => x.1 = true,
                        None => self.fxc.hide_list.push((w, true)),
                    }
                    self.fxc_hide_changed();
                }
            }
            Out::Toggle(SHOW_HIDDEN) => {
                self.fxc.show_hidden = !self.fxc.show_hidden;
                self.fxc.view.show_hidden = self.fxc.show_hidden;
                self.fxc_keep(
                    "fxc_show_hidden",
                    if self.fxc.show_hidden { "1" } else { "0" },
                );
                self.fxc_scan_chain();
            }
            Out::Toggle(SHOW_VALUES) => {
                self.fxc.view.show_values = !self.fxc.view.show_values;
                self.fxc.prefs_dirty = true;
            }
            Out::Seg(P_PARAMS, k) => {
                let n = match post.params {
                    PostParams::Random(n) => n,
                    _ => 4,
                };
                post.params = match k {
                    0 => PostParams::None,
                    1 => PostParams::All,
                    2 => PostParams::Continuous,
                    _ => PostParams::Random(n),
                };
                self.fxb_save_soon();
            }
            Out::Step(P_N, d) => {
                if let PostParams::Random(n) = post.params {
                    post.params = PostParams::Random((n as i32 + d).clamp(1, 16) as u8);
                }
                self.fxb_save_soon();
            }
            Out::Chip(P_CHIPS, k) => {
                match k {
                    0 => post.xy = !post.xy,
                    1 => post.range = !post.range,
                    2 => post.base = !post.base,
                    _ => post.invert = !post.invert,
                }
                self.fxb_save_soon();
            }
            Out::Toggle(P_BYPASS) => {
                post.bypass = !post.bypass;
                self.fxb_save_soon();
            }
            Out::Seg(W_PLACE, k) => {
                let p = match k {
                    0 => Place::Arrange,
                    1 => Place::Dock,
                    _ => Place::Float,
                };
                if p != self.fxc.place {
                    self.queue(WinOp::FxcPlace(p));
                }
            }
            Out::Toggle(W_LOCK) => self.fxc.locked = !self.fxc.locked,
            Out::Toggle(W_MASTER) => {
                self.fxc.follow_master = !self.fxc.follow_master;
                self.fxc_keep(
                    "fxc_follow_master",
                    if self.fxc.follow_master { "1" } else { "0" },
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tracked_count_goes_both_ways() {
        for n in [1usize, 3, 8, 20, 40] {
            assert_eq!(count_of(norm_of(n)), n);
        }
        assert_eq!(count_of(0.0), 1);
        assert_eq!(count_of(1.0), 40);
    }

    #[test]
    fn a_word_off_stays_in_the_list_and_hides_nothing() {
        let l = crate::fxc::hide_list("MIDI, CC,!mix,,!");
        assert_eq!(
            l,
            vec![
                ("MIDI".to_string(), true),
                ("CC".to_string(), true),
                ("mix".to_string(), false)
            ]
        );
        assert_eq!(
            crate::fxc::hide_words("MIDI,!mix"),
            vec!["MIDI".to_string()]
        );
        assert_eq!(crate::fxc::hide_text(&l), "MIDI,CC,!mix");
    }
}
