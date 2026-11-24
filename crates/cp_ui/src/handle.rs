//! L'onglet d'une vue ancree, a la REAPER : le nom de la vue dans un petit
//! onglet pose sur son coin du bas, la ou REAPER met les onglets de ses
//! dockers. On le tire pour deplacer la vue d'un docker a l'autre (le module
//! passe le glisser a REAPER). Il ne prend pas de place : il est dessine
//! par-dessus le pied de la vue (la ligne d'etat s'ecarte de lui).
//!
//! Il se pose dans le coin LOIN du bord qui redimensionne le docker : en bas
//! a gauche, en bas a droite dans un docker de droite (Cedric, 2026-11-24 :
//! un lisere au bord haut touchait le bord qui redimensionne le docker du
//! bas). Il se montre quand REAPER cache sa propre bande d'onglets (une seule
//! vue dans le docker) ; le module en decide.
//!
//! Chaque vue le dessine APRES son contenu (il passe par-dessus) et le teste
//! AVANT lui (il l'emporte sur ce qu'il couvre).

use cp_gfx::Canvas;
use cp_gfx::text::{Align, Font, TextCache};

use crate::Theme;

/// Le coin de l'onglet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corner {
    #[default]
    BottomLeft,
    /// Dans un docker de droite (son bord gauche redimensionne).
    BottomRight,
}

/// La hauteur de l'onglet (px a 96 DPI).
pub const TAB_H: f32 = 20.0;

/// Un glisser commence sur l'onglet au-dela de ce pas (px a 96 DPI) : un
/// simple clic ne deplace rien.
pub const HANDLE_DRAG_PX: f32 = 4.0;

/// L'onglet de la vue `name`, de taille `w` x `h`, a l'echelle `scale` :
/// (x, y, largeur, hauteur). La largeur est une estimation du nom (la vue
/// la connait avant de dessiner, et le test de la souris aussi).
pub fn tab_rect(name: &str, corner: Corner, w: f32, h: f32, scale: f32) -> (f32, f32, f32, f32) {
    let tw = ((20.0 + 6.4 * name.chars().count() as f32) * scale).round();
    let th = (TAB_H * scale).round();
    let m = (4.0 * scale).round();
    let x = match corner {
        Corner::BottomLeft => m,
        Corner::BottomRight => (w - m - tw).max(0.0),
    };
    (x, (h - th).max(0.0), tw, th)
}

/// Le point est-il sur l'onglet `r` ?
pub fn over(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// La place que l'onglet prend sur la ligne du bas, de son cote : la ligne
/// d'etat commence (a gauche) ou finit (a droite) au-dela.
pub fn reserve(r: (f32, f32, f32, f32), scale: f32) -> f32 {
    r.2 + (8.0 * scale).round()
}

/// Dessine l'onglet `r` de la vue `name` ; `hot` : survole ou saisi. Un
/// onglet pendu sous la vue, comme ceux des dockers de REAPER : le haut
/// droit, les coins du bas arrondis ; survole, un trait d'accent en haut.
pub fn draw_tab(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    r: (f32, f32, f32, f32),
    name: &str,
    hot: bool,
) {
    let (x, y, w, h) = r;
    let rad = th.s(4.0).round();
    let bg = th.bar.mix(th.text, if hot { 0.13 } else { 0.07 });
    cv.fill_round_rect(x, y, w, h, rad, bg);
    cv.fill_rect(x, y, w, (h * 0.5).round(), bg);
    if hot {
        cv.fill_rect(x, y, w, th.s(2.0).round().max(1.0), th.accent);
    }
    text.draw_in(
        cv,
        Font::new(0, th.s(11.0) as u16, 600),
        x,
        y,
        w,
        h,
        name,
        if hot { th.text } else { th.text_dim },
        Align::Center,
    );
}

/// L'infobulle de l'onglet.
pub const HANDLE_TIP: &str =
    "Drag: move the view to another docker \u{00b7} right-click: the view's menu";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_onglet_se_pose_dans_son_coin() {
        let l = tab_rect("Browser", Corner::BottomLeft, 400.0, 700.0, 1.0);
        assert_eq!((l.0, l.1, l.3), (4.0, 680.0, 20.0));
        assert!(l.2 > 50.0 && l.2 < 80.0);
        let r = tab_rect("Browser", Corner::BottomRight, 400.0, 700.0, 1.0);
        assert_eq!(r.0 + r.2, 396.0);
        assert!(over(l, 10.0, 690.0) && !over(l, 10.0, 679.0) && !over(r, 10.0, 690.0));
        // A 150 % : tout suit l'echelle.
        let b = tab_rect("Browser", Corner::BottomLeft, 600.0, 1050.0, 1.5);
        assert_eq!((b.0, b.1, b.3), (6.0, 1020.0, 30.0));
    }
}
