//! Un popover hors de sa vue, pose sur l'ecran : le transport est une barre
//! de 36 px, un menu n'y tient pas.
//!
//! - L'image : une fenetre du systeme que Windows ne rappelle jamais
//!   (`cp_sys::overlay`, transparente pixel par pixel, que la souris
//!   traverse). Un module recharge peut la tenir sans rien confier a Windows
//!   de son propre code, et le chargeur n'a rien a savoir.
//! - La souris : la vue qui l'ouvre la tient (capture) le temps du choix,
//!   comme un menu de Windows ; ses coordonnees passent dans l'espace du
//!   popover (la zone de travail de l'ecran). Un clic hors du popover le
//!   ferme ; la souris perdue (une autre application) aussi.
//! - Le clavier : la vue le recoit du crochet du chargeur (elle ne doit pas
//!   etre « sans clavier ») et le donne au popover ([`PopHost::key`]).
//!
//! Les questions se rejouent comme dans les vues ([`crate::ask`]).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Color, Scratch, Surface};
use cp_sys::overlay::Overlay;
use cp_sys::ui::{self, MenuEntry};
use cp_sys::win::Hwnd;
use cp_ui::{Ev, IconCache, Theme};

use crate::ask::{KeyDo, PopHost};

pub(crate) struct ScreenPop<A> {
    pub ph: PopHost<A>,
    /// La vue qui l'a ouvert (elle tient la souris).
    owner: Hwnd,
    /// La zone de travail de l'ecran ou il s'ouvre : (gauche, haut, largeur,
    /// hauteur), l'espace du popover.
    area: (i32, i32, i32, i32),
    ov: Option<Overlay>,
    surf: Surface,
    /// Ce qui est a l'ecran : (marque du popover, rectangle) ; rien ne se
    /// repeint tant que ca ne change pas.
    shown: Option<(u32, (i32, i32, i32, i32))>,
}

impl<A> Default for ScreenPop<A> {
    fn default() -> Self {
        ScreenPop {
            ph: PopHost::default(),
            owner: Hwnd(0),
            area: (0, 0, 1920, 1080),
            ov: None,
            surf: Surface::new(0, 0),
            shown: None,
        }
    }
}

impl<A: Clone> ScreenPop<A> {
    pub fn is_open(&self) -> bool {
        self.ph.pop.is_some()
    }

    /// Un point de la vue `owner` dans l'espace du popover.
    fn local(&self, owner: Hwnd, x: f32, y: f32) -> (f32, f32) {
        let (sx, sy) = ui::client_to_screen(owner, x as i32, y as i32);
        ((sx - self.area.0) as f32, (sy - self.area.1) as f32)
    }

    /// L'ecran du point (`x`, `y`) de la vue devient l'espace du popover.
    fn aim(&mut self, owner: Hwnd, x: f32, y: f32) -> (f32, f32) {
        let (sx, sy) = ui::client_to_screen(owner, x as i32, y as i32);
        self.area = ui::work_area(sx, sy);
        self.owner = owner;
        self.local(owner, x, y)
    }

    fn opened(&mut self) {
        if self.ph.pop.is_some() {
            ui::capture(self.owner);
            ui::set_pointer(cp_ui::Pointer::Arrow);
            self.shown = None;
        }
    }

    /// Un menu sous le point (`x`, `y`) de la vue : le choix du rejeu, ou
    /// l'ouvrir (0).
    pub fn menu(&mut self, owner: Hwnd, x: f32, y: f32, e: &[MenuEntry]) -> u32 {
        let (lx, ly) = self.aim(owner, x, y);
        let id = self.ph.menu(lx, ly, e);
        self.opened();
        id
    }

    /// Une valeur a saisir : celle du rejeu, ou ouvrir la saisie.
    pub fn text(&mut self, owner: Hwnd, title: &str, cur: &str, x: f32, y: f32) -> Option<String> {
        let (lx, ly) = self.aim(owner, x, y);
        let t = self.ph.text(title, cur, lx, ly);
        self.opened();
        t
    }

    /// Un evenement de la vue qui le tient (ses coordonnees) : `None` s'il
    /// n'y a pas de popover ; sinon le geste a rejouer s'il y en a un.
    pub fn feed(&mut self, ev: &Ev) -> Option<Option<A>> {
        self.ph.pop.as_ref()?;
        let o = self.owner;
        let ev = match *ev {
            Ev::Move { x, y, mods } => {
                let (x, y) = self.local(o, x, y);
                Ev::Move { x, y, mods }
            }
            Ev::Down { x, y, b, dbl, mods } => {
                let (x, y) = self.local(o, x, y);
                Ev::Down { x, y, b, dbl, mods }
            }
            Ev::Up { x, y, b, mods } => {
                let (x, y) = self.local(o, x, y);
                Ev::Up { x, y, b, mods }
            }
            Ev::Wheel { x, y, dx, dy, mods } => {
                let (x, y) = self.local(o, x, y);
                Ev::Wheel { x, y, dx, dy, mods }
            }
            // La souris est tenue : sortir de la vue ne veut rien dire.
            Ev::Leave => return Some(None),
            ref other => other.clone(),
        };
        let r = self.ph.feed(&ev);
        if self.ph.pop.is_none() {
            self.closed();
        }
        r
    }

    /// La fenetre de l'image, si elle est a l'ecran (l'auto-test la
    /// photographie).
    pub fn image_window(&self) -> Option<Hwnd> {
        self.ov
            .as_ref()
            .map(|o| o.hwnd())
            .filter(|&h| ui::is_shown(h))
    }

    /// Une touche du crochet (voir [`PopHost::key`]).
    pub fn key(&self, vk: u32, m: cp_ui::Mods, down: bool, chr: bool) -> Option<KeyDo> {
        self.ph.key(vk, m, down, chr)
    }

    /// Ferme sans reponse (la souris perdue, la vue fermee).
    pub fn close(&mut self) {
        if self.ph.pop.take().is_some() {
            self.ph.ask.cancel();
        }
        self.closed();
    }

    fn closed(&mut self) {
        if self.owner.0 != 0 && ui::has_capture(self.owner) {
            ui::release_capture();
        }
        self.ov = None;
        self.shown = None;
    }

    /// Le garde du battement : la souris partie ailleurs (une autre
    /// application au premier plan, une autre fenetre qui la prend) ferme.
    pub fn guard(&mut self) {
        if self.is_open() && (!ui::has_capture(self.owner) || !ui::foreground_is_ours()) {
            self.close();
        }
    }

    /// Pose l'image a l'ecran (rien si rien n'a change).
    pub fn render(
        &mut self,
        th: &Theme,
        text: &mut TextCache,
        icons: &mut IconCache,
        scratch: &mut Scratch,
    ) {
        let (ax, ay) = (self.area.0, self.area.1);
        if self.ph.pop.is_none() {
            self.ov = None;
            return;
        }
        let Some(key) = self.paint(th, text, icons, scratch) else {
            return;
        };
        let (bx, by, bw, bh) = key.1;
        if self.ov.is_none() {
            self.ov = Overlay::new(self.owner);
        }
        if let Some(ov) = &mut self.ov {
            if ov.show(ax + bx, ay + by, bw, bh, &self.surf.px) {
                self.shown = Some(key);
            }
        }
    }

    /// Dessine le popover dans sa propre image (son rectangle et son ombre,
    /// le reste transparent) ; rend (marque, rectangle dans l'espace du
    /// popover), rien s'il n'y a rien a repeindre.
    fn paint(
        &mut self,
        th: &Theme,
        text: &mut TextCache,
        icons: &mut IconCache,
        scratch: &mut Scratch,
    ) -> Option<(u32, (i32, i32, i32, i32))> {
        let (_, _, aw, ah) = self.area;
        let p = self.ph.pop.as_mut()?;
        p.layout(th, text, aw as f32, ah as f32);
        // Le rectangle du popover et de son ombre.
        let m = th.s(16.0);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for r in p.rects() {
            x0 = x0.min(r.x - m);
            y0 = y0.min(r.y - m);
            x1 = x1.max(r.right() + m);
            y1 = y1.max(r.bottom() + m);
        }
        if x0 > x1 {
            return None;
        }
        let bx = (x0.floor() as i32).clamp(0, aw);
        let by = (y0.floor() as i32).clamp(0, ah);
        let bw = (x1.ceil() as i32).clamp(0, aw) - bx;
        let bh = (y1.ceil() as i32).clamp(0, ah) - by;
        if bw <= 0 || bh <= 0 {
            return None;
        }
        let key = (p.stamp(), (bx, by, bw, bh));
        if self.shown == Some(key) && self.ov.is_some() {
            return None;
        }
        self.surf.resize(bw, bh);
        let mut cv = Canvas::new(&mut self.surf, scratch);
        cv.clear(Color::TRANSPARENT);
        cv.push_origin(-bx as f32, -by as f32);
        p.draw(&mut cv, th, text, icons);
        cv.pop_origin();
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_gfx::png;
    use cp_sys::font::GdiGlyphs;
    use cp_ui::pop::{Entry, Pop};
    use cp_ui::{Button, Mods};

    fn alpha(s: &Surface, x: i32, y: i32) -> u32 {
        s.px[(y * s.w + x) as usize] >> 24
    }

    /// Le menu de la barre du transport, un sous-menu ouvert, dessine dans
    /// sa propre image : transparent hors du popover, plein dedans ; et une
    /// infobulle. Planches : %TEMP%/cp_screenpop_*.png.
    #[test]
    fn a_screen_popover_paints_only_itself() {
        let mut text = TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas"])));
        let mut icons = IconCache::default();
        let mut scratch = Scratch::default();
        let th = Theme::dark(1.0);
        let mut sp: ScreenPop<u8> = ScreenPop {
            area: (0, 0, 1600, 900),
            ..Default::default()
        };
        let tuner = vec![
            Entry::check(21, "In the clock, when stopped", true),
            Entry::check(22, "Its own place, in the middle", false),
            Entry::check(20, "Off", false),
            Entry::Sep,
            Entry::item(23, "Reference: A4 = 440 Hz…"),
        ];
        let e = vec![
            Entry::item(5, "Add action…"),
            Entry::Sep,
            Entry::sub("Tuner", tuner),
            Entry::sub("External MIDI clock", vec![Entry::check(99, "Off", true)]),
            Entry::Sep,
            Entry::item(6, "Close CP Transport (show REAPER's transport)"),
        ];
        sp.ph.pop = Some(Pop::menu(0, e, 40.0, 30.0));
        let key = sp.paint(&th, &mut text, &mut icons, &mut scratch);
        assert!(key.is_some());
        // Le sous-menu « Tuner » : la souris sur sa ligne l'ouvre.
        let row = sp.ph.pop.as_ref().map(|p| p.rects()[0]).unwrap();
        let (mx, my) = (row.x + 40.0, row.y + 70.0);
        let _ = sp.ph.pop.as_mut().unwrap().event(&Ev::Move {
            x: mx,
            y: my,
            mods: Mods::default(),
        });
        let _ = sp.ph.pop.as_mut().unwrap().event(&Ev::Down {
            x: mx,
            y: my,
            b: Button::Left,
            dbl: false,
            mods: Mods::default(),
        });
        let (_, (bx, by, bw, bh)) = sp.paint(&th, &mut text, &mut icons, &mut scratch).unwrap();
        let n = sp.ph.pop.as_ref().map_or(0, |p| p.rects().len());
        std::fs::write(
            std::env::temp_dir().join("cp_screenpop_menu.png"),
            png::encode(&sp.surf),
        )
        .unwrap();
        assert!(bw > 0 && bh > 0 && bx >= 0 && by >= 0);
        // Les coins de l'image : transparents ; le milieu du menu : plein.
        assert_eq!(alpha(&sp.surf, 0, 0), 0);
        assert_eq!(alpha(&sp.surf, bw - 1, bh - 1), 0);
        let r0 = sp.ph.pop.as_ref().map(|p| p.rects()[0]).unwrap();
        let (cx, cy) = (
            (r0.x + r0.w / 2.0) as i32 - bx,
            (r0.y + r0.h / 2.0) as i32 - by,
        );
        assert_eq!(alpha(&sp.surf, cx, cy), 255);
        assert!(n >= 1);

        // L'infobulle.
        let s = concat!("Rate\n", "LFO 1 +30 %\n", "Alt+drag: depth");
        let (w, h) = cp_ui::pop::tip_size(&th, &mut text, s);
        let mut surf = Surface::new(w as i32 + 24, h as i32 + 24);
        {
            let mut cv = Canvas::new(&mut surf, &mut scratch);
            cv.clear(Color::TRANSPARENT);
            cp_ui::pop::draw_tip(&mut cv, &th, &mut text, s, 12.0, 12.0);
        }
        std::fs::write(
            std::env::temp_dir().join("cp_screenpop_tip.png"),
            png::encode(&surf),
        )
        .unwrap();
        assert_eq!(alpha(&surf, 0, 0), 0);
        assert_eq!(alpha(&surf, 12 + w as i32 / 2, 12 + h as i32 / 2), 255);
        assert!(h > 3.0 * 17.0, "trois lignes");
    }
}
