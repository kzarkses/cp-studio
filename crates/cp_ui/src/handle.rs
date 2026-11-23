//! La poignee d'une vue ancree : des points, l'icone et le nom de la vue, a
//! gauche de sa barre du haut. On la saisit pour deplacer la vue d'un
//! docker a l'autre (le module passe le glisser a REAPER). Elle se montre
//! quand REAPER cache sa propre bande d'onglets (une seule vue dans le
//! docker) ; le module en decide.

use cp_gfx::Canvas;
use cp_gfx::text::{Align, Font, TextCache};

use crate::{Icon, IconCache, Theme};

/// La largeur de la poignee d'une vue nommee `name`, a l'echelle `scale`
/// (une estimation : la vue la connait avant de dessiner).
pub fn handle_width(name: &str, scale: f32) -> f32 {
    ((42.0 + 7.4 * name.chars().count() as f32) * scale).round()
}

/// Un glisser commence sur la poignee au-dela de ce pas (px a 96 DPI) : un
/// simple clic ne deplace rien.
pub const HANDLE_DRAG_PX: f32 = 4.0;

/// Dessine la poignee dans `(x, y, w, h)` ; `hot` : survolee ou saisie.
pub fn draw_handle(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    icons: &mut IconCache,
    th: &Theme,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    icon: Icon,
    name: &str,
    hot: bool,
) {
    let s = |v: f32| th.s(v);
    let bg = if hot {
        th.hover
    } else {
        th.bar.mix(th.text, 0.05)
    };
    cv.fill_rect(x, y, w, h, bg);
    cv.fill_rect(x + w - 1.0, y, 1.0, h, th.seam);
    // Les points : deux colonnes de trois.
    let dot = if hot { th.text_dim } else { th.text_faint };
    let (gx, gy) = (x + s(7.0), y + h * 0.5 - s(4.0));
    for c in 0..2 {
        for r in 0..3 {
            cv.fill_circle(gx + c as f32 * s(4.0), gy + r as f32 * s(4.0), s(1.15), dot);
        }
    }
    let isz = s(14.0);
    icons.draw(
        cv,
        icon,
        (x + s(17.0)).round(),
        (y + (h - isz) * 0.5).round(),
        isz as u16,
        if hot { th.text } else { th.text_dim },
        false,
    );
    let tx = x + s(36.0);
    if w > s(40.0) {
        text.draw_in(
            cv,
            Font::new(0, s(12.0) as u16, 600),
            tx,
            y,
            (x + w - s(4.0) - tx).max(0.0),
            h,
            name,
            if hot { th.text } else { th.text_dim },
            Align::Left,
        );
    }
}

/// L'infobulle de la poignee.
pub const HANDLE_TIP: &str =
    "Drag: move the view to another docker \u{00b7} right-click: the view's menu";
