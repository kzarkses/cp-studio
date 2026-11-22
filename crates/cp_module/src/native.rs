//! Ce qui fait que les vues se comportent comme REAPER, partage par Guitar
//! Tab et l'editeur : les editions au survol liees dans la fenetre principale
//! (Q, D, S, Z, A, E chez Cedric), la ligne a la souris avec ses reglages, la
//! tete de lecture du theme, et les curseurs qui changent avec les touches.

use cp_editview::keys::{Bind, KeyMap};
use cp_sys::ui;
use cp_sys::win::Hwnd;
use cp_ui::{Guide, HoverEdit, Mods, Theme};

use crate::App;

/// Les battements entre deux relectures des reglages (une seconde).
const LOOK_EVERY: u32 = 30;

/// Les ecoutes a la souris de SWS/BR que Guitar Tab reprend : vrai avec le
/// solo de la piste sous la souris.
fn play_of_name(n: &str) -> Option<bool> {
    match n {
        "_BR_CONT_PLAY_MOUSE_SOLO_TRACK" => Some(true),
        "_BR_CONT_PLAY_MOUSE" => Some(false),
        _ => None,
    }
}

impl App {
    /// L'edition au survol que la fenetre principale de REAPER lie a cette
    /// touche (un script nvk, LKC, CP_DeleteUnderMouse : reconnu a son nom).
    pub(crate) fn main_hover(&mut self, vk: u32, ext: bool, md: Mods) -> Option<HoverEdit> {
        match self.main_keymap()?.get(vk, ext, md)? {
            Bind::Named(n) => HoverEdit::of_name(n),
            Bind::Cmd(_) => None,
        }
    }

    /// L'ecoute a la souris que la fenetre principale lie a cette touche
    /// (SWS/BR, « perform until shortcut released ») : vrai avec le solo de
    /// la piste sous la souris.
    pub(crate) fn main_play(&mut self, vk: u32, ext: bool, md: Mods) -> Option<bool> {
        match self.main_keymap()?.get(vk, ext, md)? {
            Bind::Named(n) => play_of_name(n),
            Bind::Cmd(_) => None,
        }
    }

    /// La table des touches de la fenetre principale (lue une fois).
    fn main_keymap(&mut self) -> Option<&KeyMap> {
        if self.main_keys.is_none() {
            let r = self.r;
            let defs = r.section_default_keys(0);
            let ini = r
                .resource_path()
                .and_then(|p| std::fs::read(p.join("reaper-kb.ini")).ok())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let km = KeyMap::build_for(0, &defs, &ini);
            let hover: Vec<String> = km
                .iter()
                .filter_map(|(&(f, k), b)| match b {
                    Bind::Named(n) => HoverEdit::of_name(n)
                        .map(|e| format!("{f}/{k} {e:?} {n}"))
                        .or_else(|| play_of_name(n).map(|s| format!("{f}/{k} Play(solo {s}) {n}"))),
                    _ => None,
                })
                .collect();
            self.host.info(&format!(
                "Editions au survol de la fenetre principale : {}",
                if hover.is_empty() {
                    "aucune".to_string()
                } else {
                    hover.join(" ; ")
                }
            ));
            self.main_keys = Some(km);
        }
        self.main_keys.as_ref()
    }

    /// La souris est sur cette fenetre (une edition au survol ne vise
    /// qu'elle ; ailleurs, la touche revient a REAPER).
    pub(crate) fn mouse_over(hwnd: Hwnd) -> Option<(f32, f32)> {
        // Un onglet cache du docker garde sa place : il ne vise rien.
        if !ui::is_shown(hwnd) {
            return None;
        }
        let (sx, sy) = ui::cursor_pos();
        let (x, y) = ui::screen_to_client(hwnd, sx, sy);
        let rc = ui::client_rect(hwnd);
        (x >= 0 && y >= 0 && x < rc.right - rc.left && y < rc.bottom - rc.top)
            .then_some((x as f32, y as f32))
    }

    /// Le theme de REAPER pour nos vues : ses couleurs, et la tete de lecture
    /// telle que REAPER la dessine (sa couleur a son opacite du theme, 2 px).
    pub(crate) fn reaper_theme(&self, scale: f32) -> Theme {
        let r = self.r;
        let mut th = Theme::reaper(scale, &|k| r.theme_color(k));
        th.rp = cp_ui::ReaperLook::read(&|k| r.theme_color(k), &|k| r.theme_value(k));
        if let Some(m) = r.theme_value("playcursor_drawmode") {
            let (a, _) = cp_sys::theme::drawmode_alpha(m);
            th.playhead = th.playhead.with_alpha((a * 255.0).round() as u8);
        }
        th
    }

    /// Les reglages de REAPER relus (une fois par seconde) : la ligne a la
    /// souris (Preferences > Appearance) et sa couleur du theme.
    pub(crate) fn reaper_look(&mut self) {
        if self.ticks % LOOK_EVERY != 0 {
            return;
        }
        let r = self.r;
        let g2 = r.config_int("guidelines2").unwrap_or(0);
        let color = r.theme_color("mouseitem_color").map(|(cr, cg, cb)| {
            let a = r
                .theme_value("mouseitem_mode")
                .map_or(0.5, |m| cp_sys::theme::drawmode_alpha(m).0);
            cp_gfx::rgba(cr, cg, cb, (a * 255.0).round() as u8)
        });
        // Le magnetisme de REAPER, a calculer au zoom de Guitar Tab : sa
        // division, son bouton (1157), son ecart minimum en pixels.
        let grid = cp_tabview::SnapGrid {
            on: r.toggle_state(1157) == 1,
            div_qn: r.project_grid().unwrap_or(0.0625) * 4.0,
            min_px: r
                .config_int("projgridsnapmin")
                .filter(|&v| v > 0)
                .unwrap_or(8) as f32,
        };
        if self.gt.arr.grid != Some(grid) {
            self.gt.arr.grid = Some(grid);
        }
        let g = Guide::from_reaper(g2, color);
        if self.gt.arr.guide != g {
            self.gt.arr.guide = g;
            if !g.on {
                self.gt.arr.guide_q = None;
            }
        }
        if self.ed.ed.guide != g {
            self.ed.ed.guide = g;
            if !g.on {
                self.ed.ed.guide_q = None;
            }
        }
    }

    /// Une touche de modification pressee ou relachee sans bouger la souris :
    /// le curseur change quand meme, comme dans REAPER (Ctrl sur une note :
    /// la copie).
    pub(crate) fn pointer_tick(&mut self) {
        let md = crate::mods();
        if md == self.last_mods {
            return;
        }
        self.last_mods = md;
        if let Some(w) = self.gt.win.as_ref().map(|w| w.hwnd) {
            if let Some((x, y)) = Self::mouse_over(w) {
                ui::set_pointer(self.gt.arr.pointer(x, y, md));
                return;
            }
        }
        if let Some(w) = self.ed.win.as_ref().map(|w| w.hwnd) {
            if let Some((x, y)) = Self::mouse_over(w) {
                ui::set_pointer(self.ed.ed.pointer(x, y, md));
            }
        }
    }
}
