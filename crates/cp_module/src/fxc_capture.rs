//! La capture de FX Constellation : les gestes deviennent de l'automation.
//!
//! On arme des sources (les axes du pad, les macros) ; pendant que REAPER
//! joue, leur valeur s'ecrit en points d'enveloppe sur le curseur du bridge
//! qui les porte (`x_pos`, `y_pos`, `macroN`). On enregistre la SOURCE,
//! jamais les parametres qu'elle mene : a la relecture, l'enveloppe mene le
//! bridge, et le bridge mene tout le reste par ses liens. Un passage rejoue
//! efface ce qu'il y avait (comme l'ecriture en Latch).

use cp_fxc::slots::{FxKind, bridge};
use cp_sys::fxc::Env;
use cp_sys::ui::MenuEntry;

use crate::App;

/// Ce qu'on peut capturer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cap {
    PadX,
    PadY,
    Macro(u8),
}

impl Cap {
    fn param(self) -> usize {
        match self {
            Cap::PadX => bridge::X_POS,
            Cap::PadY => bridge::Y_POS,
            Cap::Macro(n) => bridge::MACRO_IN + n as usize - 1,
        }
    }
}

/// Un saut en avant plus long (un clic sur la regle, une image figee) ne
/// s'efface pas : c'est un nouveau passage.
const GAP: f64 = 0.35;
/// Le temps d'une marche aux bords d'un passage.
const EDGE: f64 = 0.001;

/// L'etat de la capture (pour la session : rien n'est garde).
#[derive(Default)]
pub(crate) struct Capture {
    pub armed: Vec<Cap>,
    pass: Option<Pass>,
    pub points: u32,
}

/// Un passage : de son debut au dernier instant ecrit.
struct Pass {
    armed: Vec<Cap>,
    at: f64,
    lanes: Vec<Lane>,
}

/// Une source armee dans le passage.
struct Lane {
    /// Son enveloppe (une source sans enveloppe garde sa place : les
    /// rangs restent ceux des sources armees).
    env: Option<Env>,
    /// La derniere valeur ecrite et son instant.
    last: f64,
    last_t: f64,
    /// L'enveloppe avait des points avant le passage, et sa valeur
    /// d'origine au dernier instant (pour s'y raccorder a la fin).
    had: bool,
    orig: f64,
}

impl App {
    /// Le menu de la capture.
    pub(crate) fn fxc_capture_menu(&mut self, x: f32, y: f32) {
        let armed = self.fxc.capture.armed.clone();
        let is = |c: Cap| armed.contains(&c);
        let lx = self.fxc.plan.pad_link_x;
        let ly = self.fxc.plan.pad_link_y;
        let mut e = vec![
            MenuEntry::off("Capture to envelopes (while REAPER plays)"),
            MenuEntry::Item {
                id: 1,
                text: if lx {
                    "Pad X — follows a source".into()
                } else {
                    "Pad X".into()
                },
                checked: is(Cap::PadX),
                enabled: !lx,
            },
            MenuEntry::Item {
                id: 2,
                text: if ly {
                    "Pad Y — follows a source".into()
                } else {
                    "Pad Y".into()
                },
                checked: is(Cap::PadY),
                enabled: !ly,
            },
        ];
        e.push(MenuEntry::Sub(
            "Macros".into(),
            (1..=8)
                .map(|n| {
                    MenuEntry::check(
                        10 + n as u32,
                        self.fxc.t.macro_name(n as usize),
                        is(Cap::Macro(n)),
                    )
                })
                .collect(),
        ));
        e.push(MenuEntry::Sep);
        e.push(MenuEntry::Item {
            id: 3,
            text: "Disarm all".into(),
            checked: false,
            enabled: !armed.is_empty(),
        });
        // En popover (le choix rejoue le geste : `crate::ask`).
        let id = self.fxc_popup(x, y, &e);
        let toggle = |v: &mut Vec<Cap>, c: Cap| {
            if let Some(i) = v.iter().position(|x| *x == c) {
                v.remove(i);
            } else {
                v.push(c);
            }
        };
        let a = &mut self.fxc.capture.armed;
        match id {
            1 => toggle(a, Cap::PadX),
            2 => toggle(a, Cap::PadY),
            3 => a.clear(),
            i @ 11..=18 => toggle(a, Cap::Macro((i - 10) as u8)),
            _ => return,
        }
        a.sort();
        if !self.fxc.capture.armed.is_empty() {
            self.fxc_ensure_bridge();
        }
        self.fxc.view.mark();
    }

    /// Le battement : pendant que REAPER joue, un point par source armee
    /// quand elle bouge (au plus un par battement) ; le passage efface
    /// devant lui ce qu'il y avait, et se raccorde a l'enveloppe d'origine
    /// a ses deux bords.
    pub(crate) fn fxc_capture_tick(&mut self) {
        let playing = self.r.xc_play_state() & 1 != 0;
        let bridge = self.fxc.chain.find(FxKind::Bridge);
        let (Some(t), Some(b), true, false) = (
            self.fxc.tk,
            bridge,
            playing,
            self.fxc.capture.armed.is_empty(),
        ) else {
            self.fxc_capture_end();
            return;
        };
        let now = self.r.xc_play_position();
        let armed = self.fxc.capture.armed.clone();
        let vals: Vec<f64> = armed
            .iter()
            .map(|c| match c {
                Cap::PadX => self.fxc.t.pad.x,
                Cap::PadY => self.fxc.t.pad.y,
                Cap::Macro(n) => self.fxc.macros[*n as usize - 1],
            })
            .collect();
        // Un autre jeu de sources, un saut en arriere (une boucle) ou trop
        // loin en avant : le passage se ferme, un autre commence.
        let broken = self
            .fxc
            .capture
            .pass
            .as_ref()
            .is_some_and(|p| p.armed != armed || now < p.at - 1e-4 || now - p.at > GAP);
        if broken {
            self.fxc_capture_end();
        }
        if self.fxc.capture.pass.is_none() {
            let r = self.r;
            let mut lanes = Vec::with_capacity(armed.len());
            for (i, c) in armed.iter().enumerate() {
                let env = r.xc_fx_envelope(t, b, c.param(), true);
                let v = vals[i];
                let mut lane = Lane {
                    env,
                    last: v,
                    last_t: now,
                    had: false,
                    orig: v,
                };
                if let Some(e) = env {
                    lane.had = r.xc_env_count(e) > 0;
                    if lane.had {
                        // Ce qu'il y avait juste avant reste : une marche.
                        lane.orig = r.xc_env_eval(e, now).unwrap_or(v);
                        r.xc_env_delete_range(e, now - EDGE - 1e-6, now + 1e-6);
                        r.xc_env_insert(e, now - EDGE, lane.orig);
                        self.fxc.capture.points += 1;
                    }
                    r.xc_env_insert(e, now, v);
                    r.xc_env_sort(e);
                    self.fxc.capture.points += 1;
                }
                lanes.push(lane);
            }
            self.fxc.capture.pass = Some(Pass {
                armed,
                at: now,
                lanes,
            });
            return;
        }
        let r = self.r;
        let cap = &mut self.fxc.capture;
        let Some(pass) = cap.pass.as_mut() else {
            return;
        };
        let from = pass.at;
        if now <= from + 1e-6 {
            return;
        }
        for (i, lane) in pass.lanes.iter_mut().enumerate() {
            let Some(e) = lane.env else {
                continue;
            };
            if lane.had {
                if let Some(o) = r.xc_env_eval(e, now) {
                    lane.orig = o;
                }
            }
            r.xc_env_delete_range(e, from + 1e-6, now + 1e-6);
            let v = vals.get(i).copied().unwrap_or(lane.last);
            if (v - lane.last).abs() > 0.0005 {
                // Une valeur tenue reste plate jusqu'au battement d'avant
                // (sinon la droite vers le point suivant en ferait une rampe).
                if lane.last_t < from - 1e-6 {
                    r.xc_env_insert(e, from, lane.last);
                    cap.points += 1;
                }
                r.xc_env_insert(e, now, v);
                cap.points += 1;
                lane.last = v;
                lane.last_t = now;
            }
            r.xc_env_sort(e);
        }
        pass.at = now;
    }

    /// La fin d'un passage : la derniere valeur tenue jusqu'au bout, puis
    /// l'enveloppe d'origine reprend.
    pub(crate) fn fxc_capture_end(&mut self) {
        let Some(pass) = self.fxc.capture.pass.take() else {
            return;
        };
        let r = self.r;
        for lane in &pass.lanes {
            let Some(e) = lane.env else {
                continue;
            };
            if lane.last_t < pass.at - 1e-6 {
                r.xc_env_insert(e, pass.at, lane.last);
                self.fxc.capture.points += 1;
            }
            if lane.had {
                r.xc_env_delete_range(e, pass.at + 1e-6, pass.at + EDGE + 1e-6);
                r.xc_env_insert(e, pass.at + EDGE, lane.orig);
                self.fxc.capture.points += 1;
            }
            r.xc_env_sort(e);
        }
    }
}
