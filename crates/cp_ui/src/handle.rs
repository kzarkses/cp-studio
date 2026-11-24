//! Le lisere d'une vue ancree : le bord haut de la vue, qu'on saisit pour
//! deplacer la vue d'un docker a l'autre (le module passe le glisser a
//! REAPER). Il ne prend aucune place : au repos, une courte barre au milieu
//! du bord ; au survol, un trait de la couleur d'accent sur toute la
//! largeur. Il se montre quand REAPER cache sa propre bande d'onglets (une
//! seule vue dans le docker) ; le module en decide.
//!
//! Chaque vue le dessine APRES sa barre du haut (il passe par-dessus) et le
//! teste AVANT ses boutons (il l'emporte sur eux dans sa bande).

use cp_gfx::Canvas;

use crate::Theme;

/// La hauteur de la bande qu'on saisit (px a 96 DPI) : le haut de la barre
/// de la vue, au-dessus de ses boutons.
pub const GRIP_PX: f32 = 4.0;

/// Un glisser commence sur le lisere au-dela de ce pas (px a 96 DPI) : un
/// simple clic ne deplace rien.
pub const HANDLE_DRAG_PX: f32 = 4.0;

/// La hauteur de la bande du lisere a l'echelle `scale`.
pub fn grip_h(scale: f32) -> f32 {
    (GRIP_PX * scale).round().max(3.0)
}

/// Le point (de la vue) est-il sur le lisere ?
pub fn over_grip(y: f32, scale: f32) -> bool {
    y >= 0.0 && y < grip_h(scale)
}

/// Dessine le lisere au bord haut d'une vue de largeur `w` ; `hot` :
/// survole ou saisi.
pub fn draw_grip(cv: &mut Canvas<'_>, th: &Theme, w: f32, hot: bool) {
    let s = |v: f32| th.s(v).round().max(1.0);
    if hot {
        cv.fill_rect(0.0, 0.0, w, s(2.0), th.accent);
    } else {
        let (pw, ph) = (s(28.0), s(3.0));
        let x = ((w - pw) * 0.5).round();
        cv.fill_round_rect(x, 0.0, pw, ph, ph * 0.5, th.bar.mix(th.text, 0.2));
    }
}

/// L'infobulle du lisere.
pub const HANDLE_TIP: &str =
    "Drag: move the view to another docker \u{00b7} right-click: the view's menu";
