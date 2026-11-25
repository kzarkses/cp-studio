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

/// L'etat de la capture (pour la session : rien n'est garde).
#[derive(Default)]
pub(crate) struct Capture {
    pub armed: Vec<Cap>,
    /// Le passage en cours : le dernier instant ecrit, et les dernieres
    /// valeurs.
    pass: Option<f64>,
    last: Vec<f64>,
    envs: Vec<Env>,
    pub points: u32,
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
        let id = match self.fxc.win.as_ref().map(|w| w.hwnd) {
            Some(h) => {
                let (sx, sy) = cp_sys::ui::client_to_screen(h, x as i32, y as i32);
                cp_sys::ui::popup_tree(h, sx, sy, &e)
            }
            None => 0,
        };
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
    /// quand elle bouge (au plus 30 par seconde), le passage efface devant
    /// lui.
    pub(crate) fn fxc_capture_tick(&mut self) {
        if self.fxc.capture.armed.is_empty() {
            self.fxc_capture_end();
            return;
        }
        let playing = self.r.xc_play_state() & 1 != 0;
        if !playing {
            self.fxc_capture_end();
            return;
        }
        let (Some(t), Some(b)) = (self.fxc.tk, self.fxc.chain.find(FxKind::Bridge)) else {
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
        let cap = &mut self.fxc.capture;
        let fresh = cap.pass.is_none() || cap.envs.len() != armed.len();
        if fresh {
            cap.envs = armed
                .iter()
                .filter_map(|c| self.r.xc_fx_envelope(t, b, c.param(), true))
                .collect();
            cap.last = vec![f64::NAN; armed.len()];
        }
        let from = cap.pass.unwrap_or(now);
        if now < from {
            // Un saut en arriere (une boucle) : un nouveau passage.
            cap.pass = Some(now);
            return;
        }
        for (i, e) in cap.envs.clone().iter().enumerate() {
            let v = vals.get(i).copied().unwrap_or(0.0);
            self.r.xc_env_delete_range(*e, from + 1e-6, now + 1e-6);
            let last = cap.last.get(i).copied().unwrap_or(f64::NAN);
            if fresh || last.is_nan() || (v - last).abs() > 0.0005 {
                self.r.xc_env_insert(*e, now, v);
                cap.points += 1;
                if let Some(l) = cap.last.get_mut(i) {
                    *l = v;
                }
            }
        }
        cap.pass = Some(now);
    }

    /// La fin d'un passage : les points tries.
    fn fxc_capture_end(&mut self) {
        let cap = &mut self.fxc.capture;
        if cap.pass.take().is_some() {
            for e in std::mem::take(&mut cap.envs) {
                self.r.xc_env_sort(e);
            }
        }
    }
}
