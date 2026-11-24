//! La petite poignee de REAPER sous nos vues ancrees, masquee.
//!
//! Seule dans son docker, une vue garde une poignee de REAPER : un
//! WDLTabCtrl de 6 px, en bas dans un docker de cote, a gauche dans celui du
//! bas. Notre lisere fait le meme travail : la bande est cachee et la vue
//! prend sa place. Deux temps :
//! - quand REAPER place la vue (WM_WINDOWPOSCHANGING), elle s'etend de
//!   l'epaisseur de la bande, de son cote, jusqu'au bord du docker. Deja au
//!   bord, rien ne change : rejouer la mise en page ne l'etend pas deux fois ;
//! - a chaque battement, la bande est cachee, ou rendue a REAPER (plusieurs
//!   vues dans le docker : sa bande d'onglets pleine ; le lisere coupe ; un
//!   glisser en cours), et le docker est remis en page si la vue ne couvre
//!   pas ce qu'elle doit.
//!
//! Rien n'est retenu entre deux battements (ni entre deux chargements du
//! module) : une bande cachee se reconnait a ce que la vue la couvre.

use cp_sys::ui;
use cp_sys::win::{self, Hwnd, Rect, WinMsg};

use crate::App;

/// La petite poignee : 1 a 10 px dans un sens (la bande d'onglets pleine
/// en fait 15 et plus).
pub(crate) fn thin(r: &Rect) -> bool {
    (1..=10).contains(&(r.bottom - r.top).min(r.right - r.left))
}

pub(crate) fn covers(a: &Rect, b: &Rect) -> bool {
    a.left <= b.left && a.top <= b.top && a.right >= b.right && a.bottom >= b.bottom
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Les onglets de REAPER dans un docker (sa boite #32770).
fn tab_ctrls(host: Hwnd, view: Hwnd) -> Vec<Hwnd> {
    ui::children(host)
        .into_iter()
        .filter(|&c| c != view && ui::class_name(c) == "WDLTabCtrl")
        .collect()
}

/// La place `r` (coordonnees du docker) etendue sur la bande `g` (ecran),
/// du cote ou elle est par rapport a la vue `v` (ecran), jusqu'au bord du
/// docker `pc` au plus.
pub(crate) fn extend(mut r: Rect, g: &Rect, v: &Rect, pc: &Rect) -> Rect {
    let (gw, gh) = (g.right - g.left, g.bottom - g.top);
    let t = gw.min(gh);
    let (gx, gy) = (g.left + g.right, g.top + g.bottom);
    let (vx, vy) = (v.left + v.right, v.top + v.bottom);
    if gw < gh {
        if gx < vx {
            r.left = (r.left - t).max(pc.left);
        } else {
            r.right = (r.right + t).min(pc.right);
        }
    } else if gy > vy {
        r.bottom = (r.bottom + t).min(pc.bottom);
    } else {
        r.top = (r.top - t).max(pc.top);
    }
    r
}

impl App {
    /// Nos vues ouvertes.
    pub(crate) fn cp_views(&self) -> Vec<Hwnd> {
        [
            self.view.as_ref().map(|v| v.hwnd),
            self.ed.win.as_ref().map(|w| w.hwnd),
            self.gt.win.as_ref().map(|w| w.hwnd),
            self.ses.win.as_ref().map(|w| w.hwnd),
            self.fxb.win.as_ref().map(|w| w.hwnd),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// La bande de REAPER s'efface quand notre lisere la remplace (le
    /// reglage « jamais » la lui rend).
    fn grip_hide_wanted(&self) -> bool {
        self.handle_mode() != 2
    }

    /// REAPER place une de nos vues : elle couvre aussi sa petite poignee.
    pub(crate) fn grip_cover(&mut self, m: &WinMsg) {
        if m.msg != win::WM_WINDOWPOSCHANGING
            || !self.grip_hide_wanted()
            || !self.cp_views().contains(&m.hwnd)
        {
            return;
        }
        let Some(r) = win::pos_changing(m) else {
            return;
        };
        let Some(g) = crate::dock_grip(m.hwnd) else {
            return;
        };
        let host = ui::parent(m.hwnd);
        let out = extend(
            r,
            &ui::window_rect(g),
            &ui::window_rect(m.hwnd),
            &ui::client_rect(host),
        );
        if out != r {
            win::set_pos_changing(m, out);
            self.grip_say(m.hwnd, "couverte (place de REAPER etendue)");
        }
    }

    /// A chaque battement : la bande cachee ou rendue, et la mise en page
    /// rejouee si une vue ne couvre pas ce qu'elle doit (une fois par
    /// seconde au plus).
    pub(crate) fn grip_tick(&mut self) {
        let want = self.grip_hide_wanted() && self.grab.is_none();
        for v in self.cp_views() {
            if self.r.dock_of(v).is_none() {
                continue;
            }
            let host = ui::parent(v);
            if host.0 == 0 {
                continue;
            }
            let vr = ui::window_rect(v);
            let mut relayout = false;
            for c in tab_ctrls(host, v) {
                let cr = ui::window_rect(c);
                let shown = ui::is_visible(c);
                if want && thin(&cr) {
                    // La bande ne se cache que sous la vue : la mise en page
                    // la voit montree, et c'est sur elle que la vue s'etend.
                    if !covers(&vr, &cr) {
                        if shown {
                            // La vue s'etend elle-meme (REAPER ne replace
                            // pas une vue dont la place n'a pas change) ;
                            // ses mises en page suivantes passent par
                            // `grip_cover`.
                            let (x0, y0) = ui::screen_to_client(host, vr.left, vr.top);
                            let (x1, y1) = ui::screen_to_client(host, vr.right, vr.bottom);
                            let here = Rect {
                                left: x0,
                                top: y0,
                                right: x1,
                                bottom: y1,
                            };
                            let to = extend(here, &cr, &vr, &ui::client_rect(host));
                            if to != here {
                                ui::move_window(
                                    v,
                                    to.left,
                                    to.top,
                                    to.right - to.left,
                                    to.bottom - to.top,
                                );
                                self.grip_say(v, "couverte");
                            }
                        } else {
                            // Cachee sans etre couverte : montree d'abord,
                            // la vue s'etend au battement suivant.
                            ui::show_window(c, true);
                        }
                    } else if shown {
                        ui::show_window(c, false);
                        self.grip_say(v, "cachee");
                    }
                } else {
                    if !shown {
                        ui::show_window(c, true);
                        self.grip_say(v, "rendue a REAPER");
                    }
                    // Une vue qui deborde sur des onglets montres : la mise
                    // en page de REAPER la remet a sa place.
                    relayout |= self.grab.is_none() && overlaps(&vr, &cr);
                }
            }
            if relayout && self.ticks.saturating_sub(self.grip_relayout) >= 30 {
                self.grip_relayout = self.ticks;
                ui::resend_size(host);
                self.grip_say(v, "docker remis en page");
            }
        }
    }

    /// Le journal (les premieres fois seulement) : ce qui arrive a la bande
    /// et les marges de la vue dans son docker apres.
    fn grip_say(&mut self, v: Hwnd, what: &str) {
        if self.grip_logs >= 24 {
            return;
        }
        self.grip_logs += 1;
        let p = ui::parent(v);
        let (a, b) = (ui::window_rect(v), ui::window_rect(p));
        self.host.info(&format!(
            "bande de REAPER {what} : « {} », marges gauche {}, droite {}, haut {}, bas {}",
            ui::window_text(v),
            a.left - b.left,
            b.right - a.right,
            a.top - b.top,
            b.bottom - a.bottom
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Mesures du 2026-11-24 : le Browser a gauche (bande de 444 x 6 en
    /// bas), l'Editor en bas (bande de 6 de large a gauche).
    #[test]
    fn la_vue_s_etend_sur_la_bande_une_seule_fois() {
        // Docker de gauche : 449 x 900 ; la vue 0..444 x 0..894, la bande
        // 0..444 x 894..900 (ecran = docker ici).
        let pc = rc(0, 0, 449, 900);
        let g = rc(0, 894, 444, 900);
        let v = rc(0, 0, 444, 894);
        let out = extend(v, &g, &v, &pc);
        assert_eq!(out, rc(0, 0, 444, 900));
        // Rejouee sur une place deja etendue : rien de plus.
        assert_eq!(extend(out, &g, &out, &pc), out);
        // Docker du bas : la bande a gauche, la vue de 6 a 2000, 5 en haut
        // (le bord qui redimensionne reste).
        let pc = rc(0, 0, 2000, 250);
        let g = rc(0, 5, 6, 250);
        let v = rc(6, 5, 2000, 250);
        let out = extend(v, &g, &v, &pc);
        assert_eq!(out, rc(0, 5, 2000, 250));
        assert_eq!(extend(out, &g, &out, &pc), out);
        assert!(thin(&g) && !thin(&rc(0, 0, 444, 15)) && !thin(&rc(0, 0, 0, 0)));
        assert!(covers(&out, &g) && !overlaps(&rc(0, 0, 10, 10), &rc(10, 0, 20, 10)));
    }
}
