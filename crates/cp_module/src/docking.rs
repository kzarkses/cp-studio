//! Le docking a nous, par-dessus les dockers de REAPER.
//!
//! REAPER garde ses dockers : leur place, leur taille, et le docker de
//! chaque fenetre (retenu sous sa cle). Nous prenons ce qui se voit :
//! - les onglets : un docker qui ne tient que des vues CP perd la bande
//!   d'onglets de REAPER (grip.rs la cache, la vue prend sa place) ; chaque
//!   vue dessine la rangee des vues du docker (cp_ui::handle::DockTabs) ; un
//!   clic sur un autre onglet montre sa vue (DockWindowActivate) ;
//! - le glisser : tirer l'onglet de la vue montre des zones de depot (une
//!   image par-dessus la fenetre de REAPER, cp_sys::overlay) ; au lacher, la
//!   vue va dans le docker de la zone (Dock_UpdateDockID, DockWindowRemove,
//!   DockWindowAddEx : mesure du 2026-11-23).
//!
//! Un docker ou une fenetre de REAPER cotoie nos vues garde la bande de
//! REAPER : quand sa fenetre est montree, nous n'avons rien a l'ecran pour y
//! dessiner nos onglets.
//!
//! Rien n'est retenu entre deux battements : les groupes se relisent dans
//! les fenetres (les vues d'un docker sont les enfants de sa boite, une seule
//! montree ; mesure du 2026-11-24).

use cp_gfx::{Canvas, Surface};
use cp_sys::overlay::Overlay;
use cp_sys::ui;
use cp_sys::win::{Hwnd, Rect, WinMsg};
use cp_ui::handle::{Corner, DockTabs};

use crate::App;

/// Une de nos vues : sa fenetre, le nom de son onglet, et sa cle de docker
/// (la meme que l'IDENT de son module : REAPER y retient son docker).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewRef {
    pub hwnd: Hwnd,
    pub name: &'static str,
    pub ident: &'static str,
}

/// Les vues qui partagent un docker.
#[derive(Clone, Debug)]
pub(crate) struct Group {
    /// Nos vues, dans l'ordre des onglets.
    pub views: Vec<Hwnd>,
    /// Rien d'autre dans le docker (pas de fenetre de REAPER).
    pub cp_only: bool,
}

/// Une zone de depot : son rectangle dans l'image, et son docker.
#[derive(Clone, Copy, Debug)]
struct Zone {
    r: Rect,
    dock: i32,
}

/// Un onglet tire.
pub(crate) struct DockDrag {
    view: Hwnd,
    name: &'static str,
    ident: &'static str,
    from: Option<i32>,
    zones: Vec<Zone>,
    hot: Option<usize>,
    /// Le coin haut-gauche de l'image (ecran) et sa taille.
    origin: (i32, i32),
    size: (i32, i32),
    scale: f32,
    accent: cp_gfx::Color,
    overlay: Option<Overlay>,
    ghost: Option<Overlay>,
    t0: std::time::Instant,
}

/// Le docker a prendre pour chaque bord (0 en bas, 1 a gauche, 2 en haut,
/// 3 a droite) : celui qui tient deja des fenetres, sinon le premier qui a
/// cette place.
fn pick_dock(positions: &[(i32, i32)], used: &[i32], pos: i32) -> Option<i32> {
    let mut of_pos = positions.iter().filter(|p| p.1 == pos).map(|p| p.0);
    let all: Vec<i32> = of_pos.by_ref().collect();
    all.iter()
        .copied()
        .find(|d| used.contains(d))
        .or_else(|| all.first().copied())
}

/// Les zones de depot dans une image de `w` x `h` a l'echelle `s` : une
/// bande par bord ; `docks` : le docker de chaque bord (None : pas de zone).
fn zone_rects(w: i32, h: i32, s: f32, docks: [Option<i32>; 4]) -> Vec<Zone> {
    let side = ((w as f32 * 0.16).min(260.0 * s)).round() as i32;
    let band = ((h as f32 * 0.2).min(200.0 * s)).round() as i32;
    let rc = |l, t, r, b| Rect {
        left: l,
        top: t,
        right: r,
        bottom: b,
    };
    // Dans l'ordre des places de REAPER : bas, gauche, haut, droite.
    let rects = [
        rc(side, h - band, w - side, h),
        rc(0, 0, side, h),
        rc(side, 0, w - side, band),
        rc(w - side, 0, w, h),
    ];
    rects
        .iter()
        .zip(docks)
        .filter_map(|(r, d)| d.map(|dock| Zone { r: *r, dock }))
        .collect()
}

/// L'image des zones (pixels premultiplies), `hot` eclairee.
fn paint_zones(
    w: i32,
    h: i32,
    s: f32,
    zones: &[Zone],
    hot: Option<usize>,
    accent: cp_gfx::Color,
) -> Vec<u32> {
    let mut px = vec![0u32; (w.max(0) * h.max(0)) as usize];
    let border = (2.0 * s).round().max(1.0) as i32;
    let dash = (8.0 * s).round().max(2.0) as i32;
    let gap = (6.0 * s).round().max(2.0) as i32;
    let line = accent.with_alpha(190).premul();
    for (i, z) in zones.iter().enumerate() {
        let fill = accent
            .with_alpha(if hot == Some(i) { 72 } else { 16 })
            .premul();
        let (l, t, r, b) = (
            z.r.left.max(0),
            z.r.top.max(0),
            z.r.right.min(w),
            z.r.bottom.min(h),
        );
        let m = (4.0 * s).round() as i32;
        let (l, t, r, b) = (l + m, t + m, r - m, b - m);
        for y in t..b {
            let row = (y * w) as usize;
            for x in l..r {
                let edge = x < l + border || x >= r - border || y < t + border || y >= b - border;
                let along = if x < l + border || x >= r - border {
                    y
                } else {
                    x
                };
                let on = edge && (along % (dash + gap)) < dash;
                px[row + x as usize] = if on { line } else { fill };
            }
        }
    }
    px
}

impl App {
    /// Nos vues ouvertes, dans l'ordre des onglets.
    pub(crate) fn view_refs(&self) -> Vec<ViewRef> {
        let v = |h: Option<Hwnd>, name, ident| h.map(|hwnd| ViewRef { hwnd, name, ident });
        [
            v(
                self.view.as_ref().map(|v| v.hwnd),
                "Browser",
                "cp_studio_browser",
            ),
            v(
                self.ed.win.as_ref().map(|w| w.hwnd),
                "Editor",
                "cp_studio_editor",
            ),
            v(
                self.gt.win.as_ref().map(|w| w.hwnd),
                "Guitar Tab",
                "cp_studio_guitartab",
            ),
            v(
                self.ses.win.as_ref().map(|w| w.hwnd),
                "Session",
                "cp_studio_session",
            ),
            v(
                self.fxb.win.as_ref().map(|w| w.hwnd),
                "FX Browser",
                "cp_studio_fxbrowser",
            ),
            v(
                self.fxc.win.as_ref().map(|w| w.hwnd),
                "FX Constellation",
                "cp_studio_fxconstellation",
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Les vues du docker de `view` (None : elle n'est pas ancree).
    pub(crate) fn dock_group(&self, view: Hwnd) -> Option<Group> {
        self.r.dock_of(view)?;
        let host = ui::parent(view);
        if host.0 == 0 || ui::class_name(host) != "#32770" {
            return None;
        }
        let kids: Vec<Hwnd> = ui::children(host)
            .into_iter()
            .filter(|&c| ui::class_name(c) != "WDLTabCtrl")
            .collect();
        let refs = self.view_refs();
        let views: Vec<Hwnd> = refs
            .iter()
            .map(|r| r.hwnd)
            .filter(|h| kids.contains(h))
            .collect();
        let cp_only = kids.iter().all(|k| refs.iter().any(|r| r.hwnd == *k));
        Some(Group { views, cp_only })
    }

    /// Le coin des onglets d'une vue : loin du bord qui redimensionne son
    /// docker (en bas a droite dans un docker de droite).
    fn tab_corner(&self, view: Hwnd) -> Corner {
        match self.r.dock_of(view).map(|d| self.r.dock_position(d)) {
            Some(3) => Corner::BottomRight,
            _ => Corner::BottomLeft,
        }
    }

    /// Les onglets que la vue doit montrer (sans le survol).
    fn tabs_for(&self, v: &ViewRef, mode: u8) -> DockTabs {
        let mut t = DockTabs::new(v.name);
        if mode == 2 {
            return t;
        }
        let Some(g) = self.dock_group(v.hwnd) else {
            return t;
        };
        t.corner = self.tab_corner(v.hwnd);
        if g.cp_only {
            let refs = self.view_refs();
            t.names = g
                .views
                .iter()
                .filter_map(|h| refs.iter().find(|r| r.hwnd == *h))
                .map(|r| r.name.to_string())
                .collect();
            t.active = g.views.iter().position(|h| *h == v.hwnd).unwrap_or(0);
            t.shown = true;
        } else if mode == 1 {
            // « always » : son propre onglet, meme a cote d'une fenetre de
            // REAPER (dont la bande reste).
            t.shown = true;
        }
        t
    }

    /// Les onglets de chaque vue suivent leur docker.
    pub(crate) fn dock_tabs_tick(&mut self) {
        fn apply(slot: &mut DockTabs, mut n: DockTabs) -> bool {
            n.hot = if slot.names == n.names {
                slot.hot
            } else {
                None
            };
            let changed = *slot != n;
            if changed {
                *slot = n;
            }
            changed
        }
        let mode = self.handle_mode();
        for v in self.view_refs() {
            let t = self.tabs_for(&v, mode);
            match v.name {
                "Browser" => {
                    if apply(&mut self.browser.dock, t) {
                        self.browser.mark();
                        self.want_frames();
                    }
                }
                "Editor" => {
                    if apply(&mut self.ed.ed.dock, t) {
                        self.ed.ed.mark();
                        self.ed_render(true);
                    }
                }
                "Guitar Tab" => {
                    if apply(&mut self.gt.arr.dock, t) {
                        self.gt.arr.mark();
                        self.gtab_render(true);
                    }
                }
                "Session" => {
                    if apply(&mut self.ses.view.dock, t) {
                        self.ses.view.mark();
                        self.ses_render(true);
                    }
                }
                "FX Constellation" => {
                    if apply(&mut self.fxc.view.dock, t) {
                        self.fxc.view.mark();
                        self.fxc_render(true);
                    }
                }
                _ => {
                    if apply(&mut self.fxb.view.dock, t) {
                        self.fxb.view.mark();
                        self.fxb_render(true);
                    }
                }
            }
        }
    }

    /// Un autre onglet du docker de `view` est clique : montrer sa vue.
    pub(crate) fn dock_switch(&mut self, view: Hwnd, i: usize) {
        let Some(target) = self.dock_group(view).and_then(|g| g.views.get(i).copied()) else {
            return;
        };
        self.r.dock_activate(target);
        self.grip_tick();
        self.dock_tabs_tick();
    }

    /// L'onglet de `view` est tire : les zones de depot paraissent, la vue
    /// suivra au lacher.
    pub(crate) fn dock_grab(&mut self, view: Hwnd) {
        self.dock_drag_end(None);
        let Some(v) = self.view_refs().into_iter().find(|r| r.hwnd == view) else {
            return;
        };
        let main = self.r.main_hwnd();
        let cr = ui::client_rect(main);
        let (w, h) = (cr.right - cr.left, cr.bottom - cr.top);
        let origin = ui::client_to_screen(main, 0, 0);
        let s = ui::dpi(main) as f32 / 96.0;
        // Les dockers de chaque bord : ceux qui tiennent deja des fenetres
        // d'abord.
        let positions: Vec<(i32, i32)> = (0..16)
            .map(|d| (d, self.r.dock_position(d)))
            .filter(|p| (0..=3).contains(&p.1))
            .collect();
        let mut used: Vec<i32> = self
            .docked_windows()
            .into_iter()
            .filter_map(|h| self.r.dock_of(h))
            .collect();
        used.sort_unstable();
        used.dedup();
        let from = self.r.dock_of(view);
        let docks =
            [0, 1, 2, 3].map(|p| pick_dock(&positions, &used, p).filter(|d| Some(*d) != from));
        let zones = zone_rects(w, h, s, docks);
        let th = self.reaper_theme(s);
        // Le fantome : l'onglet tire, sous la souris.
        let tw = cp_ui::handle::tab_w(v.name, s) as i32;
        let tth = (cp_ui::handle::TAB_H * s).round() as i32;
        let mut surf = Surface::new(tw, tth);
        {
            let mut cv = Canvas::new(&mut surf, &mut self.scratch);
            cp_ui::handle::draw_tab(
                &mut cv,
                &mut self.text,
                &th,
                (0.0, 0.0, tw as f32, tth as f32),
                v.name,
                true,
                true,
            );
        }
        let (cx, cy) = ui::cursor_pos();
        let mut ghost = Overlay::new(main);
        if let Some(g) = &mut ghost {
            g.show(cx + 14, cy + 10, tw, tth, &surf.px);
        }
        let mut overlay = Overlay::new(main);
        if let Some(o) = &mut overlay {
            o.show(
                origin.0,
                origin.1,
                w,
                h,
                &paint_zones(w, h, s, &zones, None, th.accent),
            );
        }
        ui::capture(view);
        if crate::gtab::selftest() {
            self.r.ext_set(
                crate::SECTION,
                "selftest_drag",
                &format!("zones={} from={from:?}", zones.len()),
                false,
            );
        }
        self.host.info(&format!(
            "onglet : « {} » tire ({} zone(s) de depot, docker {:?})",
            v.name,
            zones.len(),
            from
        ));
        self.ddrag = Some(DockDrag {
            view,
            name: v.name,
            ident: v.ident,
            from,
            zones,
            hot: None,
            origin,
            size: (w, h),
            scale: s,
            accent: th.accent,
            overlay,
            ghost,
            t0: std::time::Instant::now(),
        });
        self.dock_drag_move(cx, cy);
    }

    /// La souris bouge pendant le glisser (en `cx`, `cy` sur l'ecran) : le
    /// fantome la suit, la zone sous elle s'eclaire.
    fn dock_drag_move(&mut self, cx: i32, cy: i32) {
        let Some(d) = &mut self.ddrag else { return };
        if let Some(g) = &d.ghost {
            g.move_to(cx + 14, cy + 10);
        }
        let (x, y) = (cx - d.origin.0, cy - d.origin.1);
        let hot = d
            .zones
            .iter()
            .position(|z| x >= z.r.left && x < z.r.right && y >= z.r.top && y < z.r.bottom);
        if hot != d.hot {
            d.hot = hot;
            let px = paint_zones(d.size.0, d.size.1, d.scale, &d.zones, hot, d.accent);
            if let Some(o) = &mut d.overlay {
                o.show(d.origin.0, d.origin.1, d.size.0, d.size.1, &px);
            }
        }
        ui::set_pointer(cp_ui::Pointer::ItemMove);
    }

    /// Fin du glisser : `drop` en (`x`, `y`) sur l'ecran, la vue va dans le
    /// docker de la zone sous ce point (s'il y en a une) ; sinon rien ne
    /// bouge.
    fn dock_drag_end(&mut self, drop: Option<(i32, i32)>) {
        if let Some((x, y)) = drop {
            self.dock_drag_move(x, y);
        }
        let Some(d) = self.ddrag.take() else { return };
        if ui::capture_window() == d.view {
            ui::release_capture();
        }
        let target = if drop.is_some() {
            d.hot.and_then(|i| d.zones.get(i)).map(|z| z.dock)
        } else {
            None
        };
        drop_overlays(d.overlay, d.ghost);
        match target {
            Some(t) if Some(t) != d.from => {
                let title = ui::window_text(d.view);
                self.r.dock_set_target(d.ident, t);
                self.r.dock_remove(d.view);
                self.r.dock_add(d.view, &title, d.ident);
                self.r.dock_activate(d.view);
                self.host.info(&format!(
                    "onglet : « {} » va du docker {:?} au docker {t} (place {})",
                    d.name,
                    d.from,
                    self.r.dock_position(t)
                ));
            }
            _ => self
                .host
                .info(&format!("onglet : « {} » lache sans depot", d.name)),
        }
        self.grip_tick();
        self.dock_tabs_tick();
    }

    /// Les messages de la vue tiree, pendant le glisser (la souris est a
    /// elle) : rien ne va a la vue.
    pub(crate) fn dock_drag_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        const WM_KEYDOWN: u32 = 0x0100;
        const WM_MOUSEMOVE: u32 = 0x0200;
        const WM_LBUTTONUP: u32 = 0x0202;
        const WM_RBUTTONDOWN: u32 = 0x0204;
        const WM_CAPTURECHANGED: u32 = 0x0215;
        const VK_ESCAPE: usize = 0x1B;
        let view = self.ddrag.as_ref()?.view;
        if m.hwnd != view {
            return None;
        }
        // Le point du message (ses coordonnees dans la vue, signees), pas le
        // curseur : l'auto-test glisse par messages, sans vraie souris.
        let (mx, my) = m.mouse_xy();
        let at = ui::client_to_screen(view, mx, my);
        match m.msg {
            WM_MOUSEMOVE => self.dock_drag_move(at.0, at.1),
            WM_LBUTTONUP => self.dock_drag_end(Some(at)),
            WM_RBUTTONDOWN => self.dock_drag_end(None),
            WM_KEYDOWN if m.wp == VK_ESCAPE => self.dock_drag_end(None),
            WM_CAPTURECHANGED if m.lp != view.0 => self.dock_drag_end(None),
            _ => return None,
        }
        Some(0)
    }

    /// A chaque battement : un lacher que la vue n'a pas recu (le bouton
    /// n'est plus tenu) finit le glisser la ou est la souris. L'auto-test
    /// n'a pas de vraie souris : il lache par message (10 s au plus).
    pub(crate) fn dock_drag_tick(&mut self) {
        let Some(d) = &self.ddrag else { return };
        let stale = d.t0.elapsed().as_secs() >= 10;
        if crate::gtab::selftest() {
            if stale {
                self.dock_drag_end(None);
            }
        } else if !ui::mouse_held() {
            let (x, y) = ui::cursor_pos();
            self.dock_drag_end(Some((x, y)));
        }
    }

    /// Un glisser d'onglet est en cours.
    pub(crate) fn dock_dragging(&self) -> bool {
        self.ddrag.is_some()
    }
}

fn drop_overlays(a: Option<Overlay>, b: Option<Overlay>) {
    drop(a);
    drop(b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_bord_prend_le_docker_qui_tient_deja_des_fenetres() {
        // 0 et 1 en bas, 6 et 7 a gauche, 2 a droite ; 7 tient le Browser.
        let pos = [(0, 0), (1, 0), (2, 3), (6, 1), (7, 1)];
        assert_eq!(pick_dock(&pos, &[1, 7], 1), Some(7));
        assert_eq!(pick_dock(&pos, &[1, 7], 0), Some(1));
        assert_eq!(pick_dock(&pos, &[], 0), Some(0));
        assert_eq!(pick_dock(&pos, &[], 2), None);
        // Pas de docker en haut : pas de zone en haut.
        let z = zone_rects(2000, 1000, 1.0, [Some(1), Some(7), None, Some(2)]);
        assert_eq!(z.len(), 3);
        assert_eq!((z[1].r.left, z[1].r.right), (0, 260));
        assert_eq!(z[0].r.bottom, 1000);
        // L'image : la zone eclairee est plus opaque que les autres.
        let px = paint_zones(2000, 1000, 1.0, &z, Some(1), cp_gfx::Color::WHITE);
        let a = |x: i32, y: i32| px[(y * 2000 + x) as usize] >> 24;
        assert!(a(100, 500) > a(1900, 500) && a(1900, 500) > 0);
        assert_eq!(a(1000, 500), 0);
    }
}
