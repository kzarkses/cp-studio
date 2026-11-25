//! Des formes rapides : un rectangle arrondi et son contour dont les bandes
//! droites passent par le chemin plein (des lignes de pixels), seuls les
//! quatre coins par le rasteriseur. Le chemin general tramait toute la
//! surface d'une carte (26 ms par image dans FX Constellation, 2026-11-25).

use cp_gfx::{Canvas, Color};

/// Un rectangle arrondi plein.
pub fn rrect(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if w <= 0.0 || h <= 0.0 || c.a == 0 {
        return;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 1.0 {
        cv.fill_rect(x, y, w, h, c);
        return;
    }
    if w * h < 1200.0 {
        cv.fill_round_rect(x, y, w, h, r, c);
        return;
    }
    let (x0, y0, x1, y1) = (x.round(), y.round(), (x + w).round(), (y + h).round());
    let k = r.ceil();
    if 2.0 * k > x1 - x0 || 2.0 * k > y1 - y0 {
        // Les bandes et les coins se recouvriraient (une teinte translucide
        // compterait deux fois).
        cv.fill_round_rect(x0, y0, x1 - x0, y1 - y0, r, c);
        return;
    }
    cv.fill_rect(x0, y0 + k, x1 - x0, (y1 - y0 - 2.0 * k).max(0.0), c);
    cv.fill_rect(x0 + k, y0, (x1 - x0 - 2.0 * k).max(0.0), k, c);
    cv.fill_rect(x0 + k, y1 - k, (x1 - x0 - 2.0 * k).max(0.0), k, c);
    for (qx, qy) in [(x0, y0), (x1 - k, y0), (x0, y1 - k), (x1 - k, y1 - k)] {
        if cv.push_clip(qx, qy, k, k) {
            cv.fill_round_rect(x0, y0, x1 - x0, y1 - y0, r, c);
        }
        cv.pop_clip();
    }
}

/// Le contour d'un rectangle arrondi, `t` px vers l'interieur.
pub fn rline(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: Color) {
    if w <= 0.0 || h <= 0.0 || c.a == 0 {
        return;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (x.round(), y.round(), (x + w).round(), (y + h).round());
    if r < 1.0 {
        cv.stroke_rect(x0, y0, x1 - x0, y1 - y0, t, c);
        return;
    }
    if w * h < 2500.0 {
        cv.stroke_round_rect(x0, y0, x1 - x0, y1 - y0, r, t, c);
        return;
    }
    let k = r.ceil().max(t);
    if 2.0 * k > x1 - x0 || 2.0 * k > y1 - y0 {
        cv.stroke_round_rect(x0, y0, x1 - x0, y1 - y0, r, t, c);
        return;
    }
    let (iw, ih) = ((x1 - x0 - 2.0 * k).max(0.0), (y1 - y0 - 2.0 * k).max(0.0));
    cv.fill_rect(x0 + k, y0, iw, t, c);
    cv.fill_rect(x0 + k, y1 - t, iw, t, c);
    cv.fill_rect(x0, y0 + k, t, ih, c);
    cv.fill_rect(x1 - t, y0 + k, t, ih, c);
    for (qx, qy) in [(x0, y0), (x1 - k, y0), (x0, y1 - k), (x1 - k, y1 - k)] {
        if cv.push_clip(qx, qy, k, k) {
            cv.stroke_round_rect(x0, y0, x1 - x0, y1 - y0, r, t, c);
        }
        cv.pop_clip();
    }
}
