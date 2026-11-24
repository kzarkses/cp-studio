//! Le dessin des elements.

use cp_fxc::slots::long_label;
use cp_gfx::path::Path;
use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, rgb};
use cp_ui::{IconCache, Theme};

use crate::build::{F, Item, Look, PadLook, Ring, Shade, Tone, amber, src_color};
use crate::{Ctl, R, View};

fn font(th: &Theme, f: F) -> Font {
    match f {
        F::Normal => th.font(),
        F::Bold => th.font_bold(),
        F::Small => th.font_small(),
        F::Mono => th.font_mono(),
    }
}

/// Le point d'un bouton tourne pour la valeur `v` (270 degres, l'ecart en
/// bas).
fn knob_pt(cx: f32, cy: f32, r: f32, v: f64) -> (f32, f32) {
    let a = (-135.0 + 270.0 * v.clamp(0.0, 1.0)) as f32 * std::f32::consts::PI / 180.0;
    (cx + r * a.sin(), cy - r * a.cos())
}

#[allow(clippy::too_many_arguments)]
fn arc(cv: &mut Canvas<'_>, cx: f32, cy: f32, r: f32, v0: f64, v1: f64, w: f32, c: Color) {
    let (a, b) = (v0.min(v1), v0.max(v1));
    if b - a < 1e-4 {
        return;
    }
    let n = ((b - a) * 48.0).ceil().max(2.0) as usize;
    let mut p = Path::new();
    for i in 0..=n {
        let v = a + (b - a) * i as f64 / n as f64;
        let (x, y) = knob_pt(cx, cy, r, v);
        if i == 0 {
            p.move_to(x, y);
        } else {
            p.line_to(x, y);
        }
    }
    cv.stroke_path(&p, w, c);
}

impl View {
    /// Dessine la derniere image construite.
    pub fn draw(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
    ) {
        let clips: Vec<(u8, R)> = self
            .items()
            .iter()
            .filter_map(|i| match i.look {
                Look::Clip(z) => Some((z, i.r)),
                _ => None,
            })
            .collect();
        let mut cur: Option<u8> = None;
        for it in self.items() {
            if it.clip != cur {
                if cur.is_some() {
                    cv.pop_clip();
                }
                cur = it.clip;
                if let Some(z) = cur {
                    let r = clips
                        .iter()
                        .find(|c| c.0 == z)
                        .map_or(R::default(), |c| c.1);
                    cv.push_clip(r.x, r.y, r.w, r.h);
                }
            }
            self.draw_item(cv, text, icons, th, it);
        }
        if cur.is_some() {
            cv.pop_clip();
        }
        // Par-dessus : la zone tiree, la pastille portee, les onglets.
        if let Some(z) = self.zone_preview {
            cv.fill_rect(z.x, z.y, z.w, z.h, th.text.fade(0.08));
            cv.stroke_rect(z.x, z.y, z.w, z.h, 1.0, th.text.fade(0.85));
        }
        if let Some((slot, x, y)) = self.carry {
            let c = src_color(th, slot);
            cv.fill_circle(x, y, th.s(10.0), c.fade(0.25));
            cv.fill_circle(x, y, th.s(5.0), c);
            let l = long_label(slot);
            let f = th.font_small();
            let w = text.measure(f, &l) + th.s(10.0);
            let (bx, by) = (x + th.s(12.0), y + th.s(8.0));
            cv.fill_round_rect(bx, by, w, th.s(18.0), th.s(3.0), th.panel);
            text.draw_in(cv, f, bx + th.s(5.0), by, w, th.s(18.0), &l, c, Align::Left);
        }
        let (w, h) = (cv.width() as f32, cv.height() as f32);
        self.dock.draw(cv, text, th, w, h, false);
    }

    fn hovered(&self, it: &Item) -> bool {
        it.ctl != Ctl::None && self.hover == it.ctl
    }

    fn draw_item(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        it: &Item,
    ) {
        let r = it.r;
        let s = th.scale;
        let rad = th.s(3.0);
        match &it.look {
            Look::Clip(_) => {}
            Look::Panel(sh) => {
                let c = match sh {
                    Shade::Bar => th.bar,
                    Shade::Zone => th.bg,
                    Shade::Card => th.panel,
                    Shade::Sunk => th.field,
                    Shade::Head => th.bar.mix(th.text, 0.04),
                    Shade::Tint(c) => {
                        cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, th.panel);
                        *c
                    }
                };
                if matches!(sh, Shade::Card | Shade::Sunk | Shade::Tint(_) | Shade::Head) {
                    cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, c);
                } else {
                    cv.fill_rect(r.x, r.y, r.w, r.h, c);
                }
                if let Ctl::Seam(_) = it.ctl {
                    if self.hovered(it) {
                        cv.fill_rect(r.cx() - 0.5, r.y, 1.0, r.h, th.accent.fade(0.7));
                    }
                }
            }
            Look::Text {
                s: t,
                font: f,
                color,
                align,
            } => {
                text.draw_in(cv, font(th, *f), r.x, r.y, r.w, r.h, t, *color, *align);
            }
            Look::Sep => cv.fill_rect(r.x, r.y, r.w, 1.0, th.seam),
            Look::Button {
                label,
                icon,
                on,
                enabled,
                tone,
            } => {
                let hov = self.hovered(it) && *enabled;
                let (bg, fg) = match (tone, on) {
                    (Tone::Accent, _) => (
                        if hov {
                            th.accent.mix(th.text, 0.15)
                        } else {
                            th.accent.mix(th.bg, 0.25)
                        },
                        th.bg,
                    ),
                    (_, true) => (th.accent.mix(th.panel, 0.55), th.text),
                    (Tone::Amber, _) => (th.panel, amber()),
                    (Tone::Danger, _) => (th.panel, th.error),
                    _ => (
                        if hov {
                            th.hover.mix(th.text, 0.06)
                        } else {
                            th.panel
                        },
                        th.text,
                    ),
                };
                let fg = if *enabled { fg } else { th.text_faint };
                cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, bg);
                let isz = th.s(14.0).min(r.h - 2.0).max(6.0) as u16;
                let f = th.font_small();
                if let Some(ic) = icon {
                    if label.is_empty() {
                        icons.draw(
                            cv,
                            *ic,
                            r.cx() - isz as f32 / 2.0,
                            r.cy() - isz as f32 / 2.0,
                            isz,
                            fg,
                            false,
                        );
                    } else {
                        let lw = text.measure(f, label);
                        let total = isz as f32 + th.s(4.0) + lw;
                        let x0 = r.cx() - total / 2.0;
                        icons.draw(cv, *ic, x0, r.cy() - isz as f32 / 2.0, isz, fg, false);
                        text.draw_in(
                            cv,
                            f,
                            x0 + isz as f32 + th.s(4.0),
                            r.y,
                            lw + 2.0,
                            r.h,
                            label,
                            fg,
                            Align::Left,
                        );
                    }
                } else {
                    text.draw_in(
                        cv,
                        f,
                        r.x + 2.0,
                        r.y,
                        r.w - 4.0,
                        r.h,
                        label,
                        fg,
                        Align::Center,
                    );
                }
            }
            Look::Seg { label, sel, .. } => {
                let hov = self.hovered(it);
                let bg = if *sel {
                    th.accent.mix(th.panel, 0.45)
                } else if hov {
                    th.hover.mix(th.text, 0.06)
                } else {
                    th.panel
                };
                cv.fill_rect(r.x, r.y, r.w, r.h, bg);
                cv.fill_rect(r.right() - 1.0, r.y + 3.0, 1.0, r.h - 6.0, th.seam);
                text.draw_in(
                    cv,
                    th.font_small(),
                    r.x + 2.0,
                    r.y,
                    r.w - 4.0,
                    r.h,
                    label,
                    if *sel { th.text } else { th.text_dim },
                    Align::Center,
                );
            }
            Look::Field {
                text: t,
                modified,
                muted,
            } => {
                let hov = self.hovered(it);
                cv.fill_round_rect(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    rad,
                    if hov {
                        th.field.mix(th.text, 0.05)
                    } else {
                        th.field
                    },
                );
                let mut x = r.x + th.s(6.0);
                if *modified {
                    cv.fill_circle(x + th.s(2.0), r.cy(), th.s(2.5), amber());
                    x += th.s(8.0);
                }
                text.draw_in(
                    cv,
                    th.font_small(),
                    x,
                    r.y,
                    r.right() - x - th.s(14.0),
                    r.h,
                    t,
                    if *muted { th.text_faint } else { th.text },
                    Align::Left,
                );
                let isz = th.s(10.0) as u16;
                icons.draw(
                    cv,
                    cp_ui::Icon::ChevronDown,
                    r.right() - th.s(13.0),
                    r.cy() - isz as f32 / 2.0,
                    isz,
                    th.text_dim,
                    false,
                );
            }
            Look::Chip {
                label,
                on,
                standby,
                amber: am,
                enabled,
            } => {
                let hov = self.hovered(it) && *enabled;
                let (bg, fg) = if *am && *on {
                    (amber(), th.bg)
                } else if *on {
                    (th.accent.mix(th.panel, 0.35), th.text)
                } else if *standby {
                    (th.field, th.accent)
                } else {
                    (
                        if hov {
                            th.hover.mix(th.text, 0.06)
                        } else {
                            th.field
                        },
                        th.text_dim,
                    )
                };
                let fg = if *enabled { fg } else { th.text_faint };
                cv.fill_round_rect(r.x, r.y, r.w, r.h, rad.min(r.h / 3.0), bg);
                if !label.is_empty() {
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x + 1.0,
                        r.y,
                        r.w - 2.0,
                        r.h,
                        label,
                        fg,
                        Align::Center,
                    );
                }
            }
            Look::Knob {
                v,
                rings,
                live,
                label,
                value,
                hl,
                size,
                enabled,
                bipolar,
                ..
            } => {
                let ks = size
                    .min(r.w)
                    .min(if label.is_empty() {
                        r.h
                    } else {
                        r.h - th.s(22.0)
                    })
                    .max(th.s(10.0));
                let cx = r.cx();
                let cy = if label.is_empty() {
                    r.cy()
                } else {
                    r.y + ks / 2.0 + th.s(1.0)
                };
                let rr = ks / 2.0 - 1.0;
                let tw0 = (rr * 0.16).max(2.0 * s.min(1.5));
                cv.fill_circle(cx, cy, rr, th.field);
                arc(
                    cv,
                    cx,
                    cy,
                    rr - tw0 / 2.0 - 1.0,
                    0.0,
                    1.0,
                    tw0,
                    th.seam.mix(th.text, 0.1),
                );
                let base_c = if *enabled { th.accent } else { th.text_faint };
                if *bipolar {
                    arc(cv, cx, cy, rr - tw0 / 2.0 - 1.0, 0.5, *v, tw0, base_c);
                } else {
                    arc(cv, cx, cy, rr - tw0 / 2.0 - 1.0, 0.0, *v, tw0, base_c);
                }
                let mut ri = rr - tw0 * 1.6 - 1.5;
                for g in rings {
                    let Ring {
                        lo,
                        hi,
                        color,
                        thin,
                        dim,
                    } = g;
                    let c = if *dim {
                        th.text_faint.fade(0.6)
                    } else {
                        color.fade(0.75)
                    };
                    let w = if *thin { 1.2 * s } else { tw0 };
                    arc(cv, cx, cy, ri.max(2.0), *lo, *hi, w, c);
                    if *thin {
                        ri -= 2.0 * s;
                    }
                }
                if let Some(c) = hl {
                    cv.stroke_circle(cx, cy, rr + th.s(2.0), 1.5 * s, c.fade(0.9));
                }
                // L'aiguille : la base ; le point : ce qui joue.
                let (px, py) = knob_pt(cx, cy, rr * 0.55, *v);
                cv.line(
                    cx,
                    cy,
                    px,
                    py,
                    1.5 * s,
                    if *enabled { th.text } else { th.text_faint },
                );
                if let Some(lv) = live {
                    if (lv - v).abs() > 0.004 || !rings.is_empty() {
                        let (lx, ly) = knob_pt(cx, cy, rr - tw0 / 2.0 - 1.0, *lv);
                        cv.fill_circle(lx, ly, (tw0 * 0.7).max(2.0), th.text);
                    }
                }
                if self.hovered(it) && self.carry.is_some() {
                    cv.stroke_circle(cx, cy, rr + th.s(2.0), 2.0 * s, th.accent);
                }
                if !label.is_empty() {
                    let ty = cy + ks / 2.0 + th.s(1.0);
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        ty,
                        r.w,
                        th.s(11.0),
                        label,
                        th.text_dim,
                        Align::Center,
                    );
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        ty + th.s(10.0),
                        r.w,
                        th.s(11.0),
                        value,
                        if *enabled { th.text } else { th.text_faint },
                        Align::Center,
                    );
                }
            }
            Look::Fader { v, label, enabled } => {
                cv.fill_round_rect(r.x, r.y, r.w, r.h, r.h.min(rad), th.field);
                let fw = (r.w * *v as f32).clamp(0.0, r.w);
                cv.fill_round_rect(
                    r.x,
                    r.y,
                    fw,
                    r.h,
                    r.h.min(rad),
                    if *enabled {
                        th.accent.fade(0.55)
                    } else {
                        th.text_faint.fade(0.3)
                    },
                );
                if !label.is_empty() && r.h >= th.s(10.0) {
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        r.y,
                        r.w,
                        r.h,
                        label,
                        th.text,
                        Align::Center,
                    );
                }
            }
            Look::Range { lo, hi } => {
                cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, th.field);
                let (a, b) = (r.x + r.w * *lo as f32, r.x + r.w * *hi as f32);
                cv.fill_rect(a, r.y, (b - a).max(1.0), r.h, th.accent.fade(0.5));
                for x in [a, b] {
                    cv.fill_rect(x - 1.5 * s, r.y - 2.0, 3.0 * s, r.h + 4.0, th.text);
                }
            }
            Look::Pad(p) => self.draw_pad(cv, th, r, p),
            Look::Dot { color, hollow } => {
                let rr = r.w.min(r.h) / 2.0;
                if self.hovered(it) {
                    cv.fill_circle(r.cx(), r.cy(), rr * 2.2, color.fade(0.35));
                }
                cv.fill_circle(r.cx(), r.cy(), rr, rgb(0, 0, 0).fade(0.55));
                if *hollow {
                    cv.stroke_circle(r.cx(), r.cy(), rr - 1.0, 1.5 * s, *color);
                } else {
                    cv.fill_circle(r.cx(), r.cy(), rr - 1.0, *color);
                }
            }
            Look::Tile {
                title,
                color,
                on,
                count,
                shape,
                phase,
                sel,
                hl,
            } => {
                let bg = if *sel {
                    th.panel.mix(*color, 0.18)
                } else if *hl || self.hovered(it) {
                    th.panel.mix(th.text, 0.05)
                } else {
                    th.panel
                };
                cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, bg);
                cv.fill_rect(
                    r.x,
                    r.y + 2.0,
                    2.0 * s,
                    r.h - 4.0,
                    color.fade(if *on { 1.0 } else { 0.35 }),
                );
                let tx = r.x + th.s(16.0);
                text.draw_in(
                    cv,
                    th.font_small(),
                    tx,
                    r.y,
                    r.w * 0.5,
                    r.h,
                    title,
                    if *on { th.text } else { th.text_faint },
                    Align::Left,
                );
                if *count > 0 {
                    let t = if *count > 8 {
                        "8+".to_string()
                    } else {
                        count.to_string()
                    };
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        r.y,
                        r.w - th.s(4.0),
                        r.h,
                        &t,
                        th.text_dim,
                        Align::Right,
                    );
                }
                if let (Some((sh, curve)), true) = (shape, *on) {
                    let cr = R::new(
                        r.x + r.w * 0.52,
                        r.y + th.s(5.0),
                        r.w * 0.32,
                        r.h - th.s(10.0),
                    );
                    let mut p = Path::new();
                    for i in 0..=32 {
                        let t = i as f64 / 32.0;
                        let v = shape_value(*sh, *curve, t);
                        let (x, y) = (cr.x + cr.w * t as f32, cr.bottom() - cr.h * v as f32);
                        if i == 0 {
                            p.move_to(x, y);
                        } else {
                            p.line_to(x, y);
                        }
                    }
                    cv.stroke_path(&p, 1.2 * s, color.fade(0.8));
                    if let Some(ph) = phase {
                        let v = shape_value(*sh, *curve, *ph);
                        cv.fill_circle(
                            cr.x + cr.w * *ph as f32,
                            cr.bottom() - cr.h * v as f32,
                            2.0 * s,
                            th.text,
                        );
                    }
                }
            }
            Look::Steps {
                n,
                vals,
                len,
                play,
                origin,
                color,
                bools,
            } => {
                cv.fill_round_rect(r.x, r.y, r.w, r.h, rad, th.field);
                let cw = r.w / (*n).max(1) as f32;
                for (k, v) in vals.iter().enumerate() {
                    let x = r.x + cw * k as f32;
                    let a = if (k as u8) < *len { 1.0 } else { 0.22 };
                    if *play == Some(k as u8) {
                        cv.fill_rect(x, r.y, cw, r.h, th.text.fade(0.14));
                    }
                    if *bools {
                        if *v > 0.5 {
                            cv.fill_rect(
                                x + 1.5,
                                r.y + 2.0,
                                cw - 3.0,
                                r.h - 4.0,
                                color.fade(0.85 * a),
                            );
                        } else {
                            cv.fill_rect(
                                x + 1.5,
                                r.y + 2.0,
                                cw - 3.0,
                                r.h - 4.0,
                                th.text.fade(0.06 * a),
                            );
                        }
                    } else {
                        let y0 = r.bottom() - r.h * *origin as f32;
                        let y1 = r.bottom() - r.h * *v as f32;
                        let (top, bot) = (y0.min(y1), y0.max(y1));
                        cv.fill_rect(
                            x + 1.5,
                            top,
                            cw - 3.0,
                            (bot - top).max(1.5),
                            color.fade(0.85 * a),
                        );
                    }
                }
                if *origin > 0.0 && !*bools {
                    let y0 = r.bottom() - r.h * *origin as f32;
                    cv.fill_rect(r.x, y0, r.w, 1.0, th.text_faint.fade(0.6));
                }
            }
            Look::Curve { pts, color, level } => {
                let ir = r.inset(th.s(3.0));
                let mut p = Path::new();
                for (i, (t, v)) in pts.iter().enumerate() {
                    let (x, y) = (ir.x + ir.w * *t, ir.bottom() - ir.h * *v);
                    if i == 0 {
                        p.move_to(x, y);
                    } else {
                        p.line_to(x, y);
                    }
                }
                cv.stroke_path(&p, 1.5 * s, *color);
                if let Some(l) = level {
                    if *l > 0.001 {
                        let y = ir.bottom() - ir.h * l.clamp(0.0, 1.0);
                        cv.fill_rect(ir.x, y, ir.w, 1.0, th.text.fade(0.7));
                    }
                }
            }
            Look::Key { black, lit } => {
                let c = match (black, lit) {
                    (_, true) => th.accent,
                    (true, false) => rgb(24, 24, 28),
                    (false, false) => rgb(222, 222, 226),
                };
                cv.fill_rect(r.x, r.y, r.w, r.h, c);
                if !black {
                    cv.fill_rect(r.right() - 1.0, r.y, 1.0, r.h, rgb(120, 120, 128));
                }
            }
            Look::SrcLabel { dots, label } => {
                let hov = self.hovered(it);
                if hov {
                    cv.fill_round_rect(r.x, r.y + r.h * 0.2, r.w, r.h * 0.6, rad, th.hover);
                }
                let mut x = r.x + th.s(4.0);
                let dr = if dots.len() > 1 { th.s(2.5) } else { th.s(3.5) };
                for (c, hollow) in dots {
                    if *hollow {
                        cv.stroke_circle(x + dr, r.cy(), dr, 1.2 * s, c.fade(0.6));
                    } else {
                        cv.fill_circle(x + dr, r.cy(), dr, *c);
                    }
                    x += dr * 2.0 + th.s(2.0);
                }
                if !label.is_empty() {
                    let c = dots.first().map_or(th.text_faint, |d| d.0);
                    text.draw_in(
                        cv,
                        th.font_small(),
                        x,
                        r.y,
                        r.right() - x,
                        r.h,
                        label,
                        c,
                        Align::Left,
                    );
                }
            }
        }
    }

    fn draw_pad(&self, cv: &mut Canvas<'_>, th: &Theme, r: R, p: &PadLook) {
        let s = th.scale;
        cv.fill_round_rect(r.x, r.y, r.w, r.h, th.s(4.0), th.field);
        let px = |x: f64| r.x + r.w * x as f32;
        let py = |y: f64| r.bottom() - r.h * y as f32;
        if !p.grains.is_empty() {
            let g = p.grid.max(2) as f32;
            for i in 1..p.grid {
                let k = i as f32 / g;
                cv.fill_rect(r.x + r.w * k, r.y, 1.0, r.h, th.seam.fade(0.35));
                cv.fill_rect(r.x, r.y + r.h * k, r.w, 1.0, th.seam.fade(0.35));
            }
            for (gx, gy) in &p.grains {
                cv.stroke_circle(
                    px(*gx),
                    py(*gy),
                    r.w / g / 2.0,
                    1.0,
                    th.text_faint.fade(0.25),
                );
                cv.fill_circle(px(*gx), py(*gy), 1.5 * s, th.text_dim);
            }
        }
        if let Some(z) = p.zone {
            let (x0, x1, y0, y1) = (px(z[0]), px(z[1]), py(z[3]), py(z[2]));
            let veil = th.bg.fade(0.55);
            cv.fill_rect(r.x, r.y, r.w, y0 - r.y, veil);
            cv.fill_rect(r.x, y1, r.w, r.bottom() - y1, veil);
            cv.fill_rect(r.x, y0, x0 - r.x, y1 - y0, veil);
            cv.fill_rect(x1, y0, r.right() - x1, y1 - y0, veil);
            cv.stroke_rect(x0, y0, x1 - x0, y1 - y0, 1.0, th.accent.fade(0.7));
        }
        if let Some(f) = &p.figure {
            let mut path = Path::new();
            for (i, (x, y)) in f.iter().enumerate() {
                if i == 0 {
                    path.move_to(px(*x), py(*y));
                } else {
                    path.line_to(px(*x), py(*y));
                }
            }
            cv.stroke_path(&path, 1.0, th.text_faint.fade(0.5));
        }
        let (dx, dy) = (px(p.x), py(p.y));
        let cx_c = p.drive_x.unwrap_or(th.seam.mix(th.text, 0.15));
        let cy_c = p.drive_y.unwrap_or(th.seam.mix(th.text, 0.15));
        cv.fill_rect(
            dx - 0.5,
            r.y,
            1.0,
            r.h,
            cx_c.fade(if p.drive_x.is_some() { 0.9 } else { 0.55 }),
        );
        cv.fill_rect(
            r.x,
            dy - 0.5,
            r.w,
            1.0,
            cy_c.fade(if p.drive_y.is_some() { 0.9 } else { 0.55 }),
        );
        let (ax, ay) = (px(p.ax), py(p.ay));
        let dot = (r.w * 0.025).max(4.0);
        if (ax - dx).abs() + (ay - dy).abs() > r.w * 0.01 {
            cv.stroke_circle(ax, ay, dot + 2.0, 1.0, th.accent.fade(0.4));
        }
        cv.fill_circle(dx, dy, dot * 4.0, th.accent.fade(0.12));
        cv.fill_circle(dx, dy, dot * 2.0, th.accent.fade(0.2));
        cv.fill_circle(dx, dy, dot, th.accent);
        cv.stroke_circle(dx, dy, dot, 1.0, th.text);
    }
}

/// La valeur d'une forme de slot a la phase `t` (pour la miniature).
pub fn shape_value(shape: u8, curve: f64, t: f64) -> f64 {
    let pf = t.fract();
    let v = match shape {
        0 => 0.5 + 0.5 * (std::f64::consts::TAU * pf).sin(),
        1 => {
            if pf < 0.5 {
                2.0 * pf
            } else {
                2.0 - 2.0 * pf
            }
        }
        2 => pf,
        3 => 1.0 - pf,
        4 => {
            if pf < 0.5 + curve * 0.45 {
                1.0
            } else {
                0.0
            }
        }
        5 | 6 => [0.2, 0.8, 0.45, 0.65][((pf * 4.0) as usize).min(3)],
        _ => [0.3, 0.7, 0.5, 0.9, 0.2, 0.6, 0.4, 0.8][((pf * 8.0) as usize).min(7)],
    };
    if shape == 4 {
        v
    } else {
        v.clamp(0.0, 1.0).powf(2f64.powf(curve * 3.0))
    }
}
