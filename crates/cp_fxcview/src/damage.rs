//! Ce qui a change d'une image a l'autre : les rectangles a repeindre.
//!
//! La vue est reconstruite a chaque image (c'est bon marche) puis comparee,
//! element par element, a celle d'avant. Seuls les elements qui ont change,
//! et ceux que le survol allume ou eteint, sont repeints : le pad qui bouge
//! ne coute que le pad. Une structure qui change (un element de plus ou de
//! moins), un calque par-dessus (la pastille portee, la zone tiree), ou trop
//! de surface : toute l'image.

use crate::build::{Item, Look};
use crate::{Ctl, R, View};

/// Au-dela de cette part de la fenetre, repeindre tout coute moins cher que
/// de decouper.
const FULL_SHARE: f32 = 0.45;
/// Au-dela de ce nombre de rectangles, on les reunit.
const MAX_RECTS: usize = 10;

impl View {
    /// Les rectangles a repeindre depuis la derniere image (`None` : tout).
    pub fn damage(&self) -> Option<Vec<R>> {
        let (w, h) = self.size;
        if self.full_now || self.prev.is_empty() || self.prev.len() != self.items.len() {
            return None;
        }
        if self.prev_overlay || self.overlay() || self.prev_dock_hot != self.dock.hot {
            return None;
        }
        // Un popover qui s'ouvre, se ferme ou change : toute l'image. Ouvert
        // et immobile, il se redessine par-dessus chaque zone repeinte.
        if self.pop_changed {
            return None;
        }
        let clips: Vec<(u8, R)> = self
            .items
            .iter()
            .filter_map(|i| match i.look {
                Look::Clip(z) => Some((z, i.r)),
                _ => None,
            })
            .collect();
        // Ce qui deborde d'un element : la lueur d'une pastille survolee,
        // l'anneau d'un bouton allume.
        let pad = self.scale.max(0.5) * 9.0;
        let vis = |it: &Item| -> Option<R> {
            let r = R::new(
                it.r.x - pad,
                it.r.y - pad,
                it.r.w + 2.0 * pad,
                it.r.h + 2.0 * pad,
            );
            let r = match it.clip {
                Some(z) => {
                    let c = clips.iter().find(|c| c.0 == z).map(|c| c.1)?;
                    intersect(r, c)?
                }
                None => r,
            };
            intersect(r, R::new(0.0, 0.0, w, h))
        };
        let hover_moved = self.prev_hover != self.hover;
        let lit = |c: &Ctl| *c != Ctl::None && (*c == self.prev_hover || *c == self.hover);
        let mut rects: Vec<R> = Vec::new();
        for (a, b) in self.prev.iter().zip(&self.items) {
            if a != b || (hover_moved && (lit(&a.ctl) || lit(&b.ctl))) {
                rects.extend(vis(a));
                rects.extend(vis(b));
            }
        }
        let area = (w * h).max(1.0);
        if rects.iter().map(|r| r.w * r.h).sum::<f32>() > FULL_SHARE * area {
            return None;
        }
        let rects = merge(rects);
        if rects.len() > MAX_RECTS {
            let bb = rects.iter().skip(1).fold(rects[0], |a, b| union(a, *b));
            if bb.w * bb.h > FULL_SHARE * area {
                return None;
            }
            return Some(vec![bb]);
        }
        Some(rects)
    }

    /// Un calque par-dessus les elements (la pastille portee, la zone tiree) :
    /// il bouge sans que les elements le sachent.
    pub(crate) fn overlay(&self) -> bool {
        self.carry.is_some() || self.zone_preview.is_some()
    }
}

fn intersect(a: R, b: R) -> Option<R> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 > x0 && y1 > y0).then(|| R::new(x0, y0, x1 - x0, y1 - y0))
}

fn union(a: R, b: R) -> R {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    R::new(x0, y0, x1 - x0, y1 - y0)
}

/// Reunit les rectangles qui se touchent (jusqu'a ce qu'aucun ne touche un
/// autre).
fn merge(mut rs: Vec<R>) -> Vec<R> {
    loop {
        let mut merged = false;
        let mut out: Vec<R> = Vec::with_capacity(rs.len());
        for r in rs {
            if let Some(o) = out.iter_mut().find(|o| touch(**o, r)) {
                *o = union(*o, r);
                merged = true;
            } else {
                out.push(r);
            }
        }
        rs = out;
        if !merged {
            return rs;
        }
    }
}

fn touch(a: R, b: R) -> bool {
    a.x <= b.x + b.w && b.x <= a.x + a.w && a.y <= b.y + b.h && b.y <= a.y + a.h
}
