//! Le dessin de la palette : ses deux lignes (les groupes, puis les icones du
//! groupe ouvert), ses cibles.

use cp_gfx::Canvas;
use cp_gfx::text::{Align, Font, TextCache};
use cp_ui::{IconCache, Theme};

use crate::{Glyph, Op, Palette};

/// Ce que vise la souris dans la palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PalHit {
    /// Le fond (un clic n'y fait rien).
    Frame,
    /// Un groupe (rang dans [`Palette::cats`]).
    Cat(u8),
    /// Une operation.
    Op(u32),
}

/// La place de la vue : sa taille, et le haut de son contenu (sous sa barre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub w: f32,
    pub h: f32,
    pub top: f32,
}

/// Les tailles (a 96 DPI) : un bouton, l'ecart, la place d'un trait, le
/// bord, l'ecart entre les deux lignes.
const BTN: f32 = 26.0;
const GAP: f32 = 2.0;
const SEP: f32 = 9.0;
const PAD: f32 = 4.0;
const ROW_GAP: f32 = 5.0;

/// Un cadre : x, y, w, h.
pub type Rect = (f32, f32, f32, f32);

/// Une cible de la palette : son cadre et ce qu'elle est.
pub type Target = (f32, f32, f32, f32, PalHit);

/// Une case de la palette : x, y, w, h, sa cible, son operation (w = 0 : un
/// trait entre deux groupes).
pub type Cell = (f32, f32, f32, f32, PalHit, Option<Op>);

/// Le cadre de la palette et la place de chaque bouton.
pub fn layout<P>(
    p: &Palette<P>,
    ops: &[Op],
    text: &mut TextCache,
    th: &Theme,
    area: Area,
) -> ((f32, f32, f32, f32), Vec<Cell>) {
    let s = |v: f32| th.s(v);
    let fc = Font::new(0, s(11.0) as u16, 600);
    let mut cells: Vec<Cell> = Vec::new();
    let row = |items: Vec<(u8, PalHit, Option<Op>, f32)>, y: f32, cells: &mut Vec<Cell>| -> f32 {
        let mut x = s(PAD);
        let mut last: Option<u8> = None;
        for (g, hit, op, w) in items {
            if last.is_some_and(|l| l != g) {
                cells.push((
                    x + s(SEP) * 0.5 - s(GAP),
                    y,
                    0.0,
                    s(BTN),
                    PalHit::Frame,
                    None,
                ));
                x += s(SEP);
            }
            last = Some(g);
            cells.push((x, y, w, s(BTN), hit, op));
            x += w + s(GAP);
        }
        x - s(GAP) + s(PAD)
    };
    let cats: Vec<(u8, PalHit, Option<Op>, f32)> = p
        .cats
        .iter()
        .enumerate()
        .map(|(k, c)| (c.group(), PalHit::Cat(k as u8), None, s(BTN)))
        .collect();
    let w1 = row(cats, s(PAD), &mut cells);
    let y2 = s(PAD + BTN + ROW_GAP);
    let items: Vec<(u8, PalHit, Option<Op>, f32)> = ops
        .iter()
        .filter(|o| o.cat == p.cat)
        .map(|o| {
            let w = match &o.glyph {
                Glyph::Chip(t) => (text.measure(fc, t).ceil() + s(12.0)).max(s(BTN)),
                Glyph::Label(t) => text.measure(fc, t).ceil() + s(4.0),
                Glyph::Icon { .. } => s(BTN),
            };
            let hit = if o.id == 0 {
                PalHit::Frame
            } else {
                PalHit::Op(o.id)
            };
            (o.group, hit, Some(o.clone()), w)
        })
        .collect();
    let w2 = row(items, y2, &mut cells);
    let (w, h) = (w1.max(w2), y2 + s(BTN + PAD));
    let mut x = p.x + s(4.0);
    let mut y = p.y + s(10.0);
    x = x.min(area.w - w - s(4.0)).max(s(2.0));
    if y + h > area.h - s(4.0) {
        y = (p.y - h - s(8.0)).max(area.top + s(2.0));
    }
    let cells = cells
        .into_iter()
        .map(|c| (c.0 + x, c.1 + y, c.2, c.3, c.4, c.5))
        .collect();
    ((x.round(), y.round(), w.round(), h.round()), cells)
}

/// Dessine la palette (par-dessus tout) ; rend son cadre et ses cibles,
/// le fond en premier.
#[allow(clippy::too_many_arguments)]
pub fn draw<P>(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    icons: &mut IconCache,
    th: &Theme,
    p: &Palette<P>,
    ops: &[Op],
    hover: Option<PalHit>,
    area: Area,
) -> (Rect, Vec<Target>) {
    let (r, cells) = layout(p, ops, text, th, area);
    let s = |v: f32| th.s(v);
    let (x, y, w, h) = r;
    let mut out = vec![(x, y, w, h, PalHit::Frame)];
    cv.fill_round_rect(
        x + s(2.0),
        y + s(4.0),
        w,
        h,
        s(5.0),
        cp_gfx::Color::BLACK.with_alpha(90),
    );
    cv.fill_round_rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, s(5.0), th.seam);
    cv.fill_round_rect(x, y, w, h, s(4.0), th.panel);
    // Le trait entre les groupes et leurs icones.
    let y_sep = y + s(PAD + BTN) + s(ROW_GAP) * 0.5;
    cv.fill_rect(x + s(3.0), y_sep.round(), w - s(6.0), 1.0, th.seam);
    let fc = Font::new(0, s(11.0) as u16, 600);
    let ft = Font::new(0, s(8.0) as u16, 700);
    for (cx, cy, cw, ch, hit, op) in cells {
        if cw <= 0.0 {
            // Un trait entre deux groupes.
            cv.fill_rect(cx.round(), cy + s(4.0), 1.0, ch - s(8.0), th.seam);
            continue;
        }
        let hov = hover == Some(hit);
        let (open, ok, on) = match (&hit, &op) {
            (PalHit::Cat(k), _) => {
                let c = p.cats[*k as usize];
                let any = ops.iter().any(|o| o.cat == c && p.ok.contains(&o.id));
                (p.cat == c, any, false)
            }
            (PalHit::Op(id), Some(o)) => {
                (false, p.ok.contains(id), o.setting == Some(true) || o.on)
            }
            _ => (false, true, false),
        };
        if let Some(Op {
            glyph: Glyph::Label(t),
            ..
        }) = &op
        {
            text.draw_in(cv, fc, cx, cy, cw, ch, t, th.text_faint, Align::Center);
            continue;
        }
        let bg = if on {
            Some(th.accent.with_alpha(80))
        } else if open {
            Some(th.accent.with_alpha(55))
        } else if hov && ok {
            Some(th.hover)
        } else {
            None
        };
        if let Some(c) = bg {
            cv.fill_round_rect(cx, cy, cw, ch, s(3.0), c);
        }
        let fg = if !ok {
            th.text_faint.fade(0.7)
        } else if on || open || hov {
            th.text
        } else {
            th.text_dim
        };
        out.push((cx, cy, cw, ch, hit));
        let glyph = match (&hit, &op) {
            (PalHit::Cat(k), _) => p.cats[*k as usize].glyph(),
            (_, Some(o)) => o.glyph.clone(),
            _ => continue,
        };
        match glyph {
            Glyph::Chip(t) => {
                text.draw_in(cv, fc, cx, cy, cw, ch, &t, fg, Align::Center);
            }
            Glyph::Icon { s: st, f, t } => {
                // Cuit une fois par taille et cale sur les pixels, comme les
                // icones (trace en direct, il etait flou et empate).
                let size = s(17.0).round();
                icons.draw_glyph(
                    cv,
                    &st,
                    &f,
                    1.75,
                    cx + (cw - size) * 0.5,
                    cy + (ch - size) * 0.5,
                    size as u16,
                    fg,
                );
                if !t.is_empty() {
                    text.draw_in(
                        cv,
                        ft,
                        cx,
                        cy - s(1.0),
                        cw - s(1.5),
                        s(10.0),
                        &t,
                        fg,
                        Align::Right,
                    );
                }
            }
            Glyph::Label(_) => {}
        }
    }
    (r, out)
}
