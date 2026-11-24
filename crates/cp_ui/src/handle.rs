//! Les onglets d'une vue ancree, a la REAPER : le nom des vues du docker
//! dans de petits onglets poses sur le coin du bas de la vue, la ou REAPER
//! met les onglets de ses dockers. Un clic sur un autre onglet montre cette
//! vue ; on tire l'onglet de la vue pour la deplacer d'un docker a l'autre
//! (le module dessine les zones de depot et la deplace). Ils ne prennent pas
//! de place : dessines par-dessus le pied de la vue (la ligne d'etat
//! s'ecarte d'eux).
//!
//! Ils se posent dans le coin LOIN du bord qui redimensionne le docker : en
//! bas a gauche, en bas a droite dans un docker de droite (Cedric,
//! 2026-11-24 : un lisere au bord haut touchait le bord qui redimensionne le
//! docker du bas). Le module en decide : une vue seule dans son docker, ou
//! un docker qui ne tient que des vues CP (REAPER y cache alors sa bande
//! d'onglets, que les notres remplacent).
//!
//! Chaque vue les dessine APRES son contenu (ils passent par-dessus) et les
//! teste AVANT lui (ils l'emportent sur ce qu'ils couvrent).

use cp_gfx::Canvas;
use cp_gfx::text::{Align, Font, TextCache};

use crate::Theme;

/// Un rectangle : (x, y, largeur, hauteur).
pub type R4 = (f32, f32, f32, f32);

/// Le coin des onglets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corner {
    #[default]
    BottomLeft,
    /// Dans un docker de droite (son bord gauche redimensionne).
    BottomRight,
}

/// La hauteur d'un onglet (px a 96 DPI).
pub const TAB_H: f32 = 20.0;

/// Un glisser commence sur l'onglet au-dela de ce pas (px a 96 DPI) : un
/// simple clic ne deplace rien.
pub const HANDLE_DRAG_PX: f32 = 4.0;

/// La largeur d'un onglet nomme `name` (une estimation du nom : la vue la
/// connait avant de dessiner, et le test de la souris aussi).
pub fn tab_w(name: &str, scale: f32) -> f32 {
    ((20.0 + 6.4 * name.chars().count() as f32) * scale).round()
}

/// Les onglets du docker d'une vue, tels que le module les lui donne.
#[derive(Clone, Debug, PartialEq)]
pub struct DockTabs {
    /// Montres (le module en decide).
    pub shown: bool,
    pub corner: Corner,
    /// Les vues du docker, dans l'ordre des onglets.
    pub names: Vec<String>,
    /// L'onglet de CETTE vue.
    pub active: usize,
    /// L'onglet sous la souris.
    pub hot: Option<usize>,
}

/// Ce qu'un appui sur les onglets demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Un autre onglet : montrer sa vue.
    Switch(usize),
    /// L'onglet de cette vue : un glisser peut commencer.
    Grab,
}

impl DockTabs {
    /// Les onglets d'une vue nommee `name`, seule (caches).
    pub fn new(name: &str) -> DockTabs {
        DockTabs {
            shown: false,
            corner: Corner::BottomLeft,
            names: vec![name.to_string()],
            active: 0,
            hot: None,
        }
    }

    /// Chaque onglet, dans une vue de taille `w` x `h` a l'echelle `s`.
    pub fn rects(&self, w: f32, h: f32, s: f32) -> Vec<R4> {
        let th = (TAB_H * s).round();
        let m = (4.0 * s).round();
        let gap = (2.0 * s).round();
        let ws: Vec<f32> = self.names.iter().map(|n| tab_w(n, s)).collect();
        let total = ws.iter().sum::<f32>() + gap * ws.len().saturating_sub(1) as f32;
        let mut x = match self.corner {
            Corner::BottomLeft => m,
            Corner::BottomRight => (w - m - total).max(0.0),
        };
        let y = (h - th).max(0.0);
        ws.iter()
            .map(|&tw| {
                let r = (x, y, tw, th);
                x += tw + gap;
                r
            })
            .collect()
    }

    /// Le rectangle de la rangee entiere.
    pub fn bounds(&self, w: f32, h: f32, s: f32) -> R4 {
        let rs = self.rects(w, h, s);
        match (rs.first(), rs.last()) {
            (Some(a), Some(b)) => (a.0, a.1, b.0 + b.2 - a.0, a.3),
            _ => (0.0, h, 0.0, 0.0),
        }
    }

    /// L'onglet sous le point (montres seulement).
    pub fn at(&self, w: f32, h: f32, s: f32, x: f32, y: f32) -> Option<usize> {
        if !self.shown {
            return None;
        }
        self.rects(w, h, s)
            .iter()
            .position(|r| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3)
    }

    /// La souris bouge : l'onglet survole. Vrai s'il a change.
    pub fn track(&mut self, w: f32, h: f32, s: f32, x: f32, y: f32) -> bool {
        let hot = self.at(w, h, s, x, y);
        let changed = hot != self.hot;
        self.hot = hot;
        changed
    }

    /// Un appui en (x, y) : changer de vue, ou saisir celle-ci.
    pub fn press(&self, w: f32, h: f32, s: f32, x: f32, y: f32) -> Option<Press> {
        let i = self.at(w, h, s, x, y)?;
        Some(if i == self.active {
            Press::Grab
        } else {
            Press::Switch(i)
        })
    }

    /// La place que la rangee prend sur la ligne du bas : (a gauche, a
    /// droite). La ligne d'etat commence ou finit au-dela.
    pub fn reserve(&self, w: f32, h: f32, s: f32) -> (f32, f32) {
        if !self.shown {
            return (0.0, 0.0);
        }
        let k = self.bounds(w, h, s).2 + (8.0 * s).round();
        match self.corner {
            Corner::BottomLeft => (k, 0.0),
            Corner::BottomRight => (0.0, k),
        }
    }

    /// Dessine la rangee ; `grabbed` : l'onglet de cette vue est tire.
    pub fn draw(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        w: f32,
        h: f32,
        grabbed: bool,
    ) {
        if !self.shown {
            return;
        }
        let rs = self.rects(w, h, th.scale);
        for (i, (r, name)) in rs.iter().zip(&self.names).enumerate() {
            let active = i == self.active;
            let hot = self.hot == Some(i) || (active && grabbed);
            draw_tab(cv, text, th, *r, name, active, hot);
        }
    }
}

/// Un onglet pendu sous la vue, comme ceux des dockers de REAPER : le haut
/// droit, les coins du bas arrondis. Celui de la vue montree est plus clair ;
/// survole, un trait d'accent en haut.
pub fn draw_tab(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    r: R4,
    name: &str,
    active: bool,
    hot: bool,
) {
    let (x, y, w, h) = r;
    let rad = th.s(4.0).round();
    let k = match (active, hot) {
        (_, true) => 0.15,
        (true, false) => 0.09,
        (false, false) => 0.04,
    };
    let bg = th.bar.mix(th.text, k);
    cv.fill_round_rect(x, y, w, h, rad, bg);
    cv.fill_rect(x, y, w, (h * 0.5).round(), bg);
    if hot {
        cv.fill_rect(x, y, w, th.s(2.0).round().max(1.0), th.accent);
    }
    let fg = if hot {
        th.text
    } else if active {
        th.text_dim
    } else {
        th.text_faint
    };
    text.draw_in(
        cv,
        Font::new(0, th.s(11.0) as u16, 600),
        x,
        y,
        w,
        h,
        name,
        fg,
        Align::Center,
    );
}

/// L'infobulle des onglets.
pub const HANDLE_TIP: &str = "Click: show this view \u{00b7} drag: move the view to another docker \u{00b7} right-click: the view's menu";

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(names: &[&str], active: usize) -> DockTabs {
        DockTabs {
            shown: true,
            corner: Corner::BottomLeft,
            names: names.iter().map(|s| s.to_string()).collect(),
            active,
            hot: None,
        }
    }

    #[test]
    fn une_rangee_d_onglets_dans_son_coin() {
        let t = tabs(&["Browser"], 0);
        let l = t.rects(400.0, 700.0, 1.0)[0];
        assert_eq!((l.0, l.1, l.3), (4.0, 680.0, 20.0));
        assert!(l.2 > 50.0 && l.2 < 80.0);
        let mut r = t.clone();
        r.corner = Corner::BottomRight;
        let rr = r.rects(400.0, 700.0, 1.0)[0];
        assert_eq!(rr.0 + rr.2, 396.0);
        assert_eq!(t.at(400.0, 700.0, 1.0, 10.0, 690.0), Some(0));
        assert_eq!(t.at(400.0, 700.0, 1.0, 10.0, 679.0), None);
        // Cachee : rien sous la souris, aucune place prise.
        let mut c = t.clone();
        c.shown = false;
        assert_eq!(c.at(400.0, 700.0, 1.0, 10.0, 690.0), None);
        assert_eq!(c.reserve(400.0, 700.0, 1.0), (0.0, 0.0));
        // A 150 % : tout suit l'echelle.
        let b = t.rects(600.0, 1050.0, 1.5)[0];
        assert_eq!((b.0, b.1, b.3), (6.0, 1020.0, 30.0));
    }

    #[test]
    fn deux_vues_dans_un_docker() {
        let t = tabs(&["Browser", "FX Browser"], 1);
        let rs = t.rects(450.0, 700.0, 1.0);
        assert_eq!(rs.len(), 2);
        assert!(rs[1].0 > rs[0].0 + rs[0].2);
        let (a, b) = (rs[0], rs[1]);
        // L'autre onglet : montrer sa vue ; le sien : saisir.
        assert_eq!(
            t.press(450.0, 700.0, 1.0, a.0 + 5.0, a.1 + 5.0),
            Some(Press::Switch(0))
        );
        assert_eq!(
            t.press(450.0, 700.0, 1.0, b.0 + 5.0, b.1 + 5.0),
            Some(Press::Grab)
        );
        let bd = t.bounds(450.0, 700.0, 1.0);
        assert_eq!((bd.0, bd.0 + bd.2), (a.0, b.0 + b.2));
        assert_eq!(t.reserve(450.0, 700.0, 1.0).0, bd.2 + 8.0);
        let mut m = t.clone();
        assert!(m.track(450.0, 700.0, 1.0, a.0 + 5.0, a.1 + 5.0));
        assert_eq!(m.hot, Some(0));
        assert!(!m.track(450.0, 700.0, 1.0, a.0 + 6.0, a.1 + 5.0));
    }
}
