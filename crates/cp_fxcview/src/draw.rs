//! Le dessin des elements.
//!
//! Les couleurs viennent toutes du theme de REAPER (`Theme::reaper`) : le
//! fond, l'accent, le texte. Les cartes, les creux et les bordures en sont
//! des melanges, jamais une couleur a nous.

use cp_fxc::slots::{Band, band_of, long_label};
use cp_gfx::path::Path;
use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, rgb};
use cp_ui::{Icon, IconCache, Theme};

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

/// Les teintes derivees du theme.
struct Pal {
    card: Color,
    card_line: Color,
    head: Color,
    well: Color,
    well_line: Color,
    sunk: Color,
    sunk_line: Color,
    hov: Color,
    /// Le texte pose sur l'accent.
    on_accent: Color,
}

fn pal(th: &Theme) -> Pal {
    Pal {
        card: th.panel,
        card_line: th.bg.mix(th.text, 0.13),
        head: th.panel.mix(th.text, 0.035),
        well: th.bg.mix(th.text, 0.035),
        well_line: th.bg.mix(th.text, 0.10),
        sunk: th.field,
        sunk_line: th.field.mix(th.text, 0.10),
        hov: th.text.fade(0.07),
        on_accent: th.accent.mix(rgb(0, 0, 0), 0.78),
    }
}

/// Une couleur plus opaque (le bord d'une teinte).
fn stronger(c: Color, k: f32) -> Color {
    Color {
        a: (c.a as f32 * k).min(255.0) as u8,
        ..c
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
    // Un polygone : l'arc exterieur, puis l'interieur a l'envers. Un seul
    // remplissage, sans les recouvrements d'un trait (et dix fois moins
    // cher).
    let n = ((b - a) * (r as f64).max(8.0) * 1.2)
        .ceil()
        .clamp(3.0, 96.0) as usize;
    let (ro, ri) = (r + w / 2.0, (r - w / 2.0).max(0.0));
    let mut p = Path::new();
    for i in 0..=n {
        let v = a + (b - a) * i as f64 / n as f64;
        let (x, y) = knob_pt(cx, cy, ro, v);
        if i == 0 {
            p.move_to(x, y);
        } else {
            p.line_to(x, y);
        }
    }
    for i in (0..=n).rev() {
        let v = a + (b - a) * i as f64 / n as f64;
        let (x, y) = knob_pt(cx, cy, ri, v);
        p.line_to(x, y);
    }
    p.close();
    cv.fill_path(&p, c);
}

/// Un rectangle arrondi : les bandes droites par le chemin rapide (des
/// lignes de pixels pleins), seuls les quatre coins passent par le
/// rasteriseur. Le chemin general tramait toute la surface d'une carte.
#[allow(clippy::too_many_arguments)]
fn rrect(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
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

/// Le contour d'un rectangle arrondi, `t` px vers l'interieur : les cotes
/// droits pleins, les coins au rasteriseur.
#[allow(clippy::too_many_arguments)]
fn rline(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: Color) {
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

/// Une icone centree dans un rectangle.
fn icon_in(cv: &mut Canvas<'_>, icons: &mut IconCache, th: &Theme, ic: Icon, r: R, c: Color) {
    let sz = th.s(16.0).min(r.h - 4.0).min(r.w - 2.0).max(8.0) as u16;
    icons.draw(
        cv,
        ic,
        (r.cx() - sz as f32 / 2.0).round(),
        (r.cy() - sz as f32 / 2.0).round(),
        sz,
        c,
        false,
    );
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
        let p = pal(th);
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
            // Hors de la zone a repeindre (et de la zone qui defile) : rien.
            let c = cv.clip();
            let m = th.s(16.0);
            if it.r.x - m > c.right() as f32
                || it.r.right() + m < c.x as f32
                || it.r.y - m > c.bottom() as f32
                || it.r.bottom() + m < c.y as f32
            {
                continue;
            }
            self.draw_item(cv, text, icons, th, &p, it);
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
            cv.fill_circle(x, y, th.s(11.0), c.fade(0.25));
            cv.fill_circle(x, y, th.s(5.5), c);
            let l = long_label(slot);
            let f = th.font_small();
            let w = text.measure(f, &l) + th.s(12.0);
            let (bx, by) = (x + th.s(12.0), y + th.s(8.0));
            rrect(cv, bx, by, w, th.s(20.0), th.s(5.0), p.card);
            rline(cv, bx, by, w, th.s(20.0), th.s(5.0), 1.0, c.fade(0.6));
            text.draw_in(cv, f, bx + th.s(6.0), by, w, th.s(20.0), &l, c, Align::Left);
        }
        let (w, h) = (cv.width() as f32, cv.height() as f32);
        self.dock.draw(cv, text, th, w, h, false);
    }

    /// Le cout de chaque sorte d'element (pour mesurer).
    #[doc(hidden)]
    pub fn draw_profile(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
    ) -> Vec<(String, f64, usize)> {
        let p = pal(th);
        let mut acc: std::collections::BTreeMap<String, (f64, usize)> = Default::default();
        for it in self.items() {
            let name = format!("{:?}", it.look);
            let name = name.split([' ', '(', '{']).next().unwrap_or("").to_string();
            let t0 = std::time::Instant::now();
            self.draw_item(cv, text, icons, th, &p, it);
            let e = acc.entry(name).or_default();
            e.0 += t0.elapsed().as_secs_f64() * 1000.0;
            e.1 += 1;
        }
        let mut v: Vec<(String, f64, usize)> =
            acc.into_iter().map(|(k, (t, n))| (k, t, n)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
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
        p: &Pal,
        it: &Item,
    ) {
        let r = it.r;
        let s = th.scale;
        let rad = th.s(6.0);
        let hov = self.hovered(it);
        match &it.look {
            Look::Clip(_) => {}
            Look::Panel(sh) => self.draw_panel(cv, th, p, it, *sh),
            Look::Text {
                s: t,
                font: f,
                color,
                align,
            } => {
                let c = if hov && matches!(it.ctl, Ctl::Sel(_) | Ctl::FxHead(_)) {
                    color.mix(th.text, 0.5)
                } else {
                    *color
                };
                text.draw_in(cv, font(th, *f), r.x, r.y, r.w, r.h, t, c, *align);
            }
            Look::Sep => cv.fill_rect(r.x, r.y, r.w, 1.0, p.card_line),
            Look::Button {
                label,
                icon,
                on,
                enabled,
                tone,
            } => {
                let hv = hov && *enabled;
                let (bg, line, fg) = match (tone, on) {
                    (Tone::Accent, _) => (
                        if hv {
                            th.accent.mix(th.text, 0.12)
                        } else {
                            th.accent
                        },
                        None,
                        p.on_accent,
                    ),
                    (Tone::Danger, _) => (
                        th.error.fade(if hv { 0.22 } else { 0.14 }),
                        Some(th.error.fade(0.5)),
                        th.error,
                    ),
                    (Tone::Amber, _) => (
                        amber().fade(if hv { 0.2 } else { 0.12 }),
                        Some(amber().fade(0.45)),
                        amber(),
                    ),
                    (_, true) => (
                        th.accent.fade(if hv { 0.24 } else { 0.16 }),
                        Some(th.accent.fade(0.42)),
                        th.accent,
                    ),
                    _ if label.is_empty() => (
                        if hv { p.hov } else { Color { a: 0, ..p.hov } },
                        None,
                        if hv {
                            th.text
                        } else {
                            th.text.mix(th.text_dim, 0.35)
                        },
                    ),
                    _ => (
                        if hv {
                            p.well.mix(th.text, 0.06)
                        } else {
                            p.well
                        },
                        Some(p.well_line),
                        th.text,
                    ),
                };
                let fg = if *enabled { fg } else { th.text_faint };
                if bg.a > 0 {
                    rrect(cv, r.x, r.y, r.w, r.h, rad, bg);
                }
                if let Some(l) = line {
                    rline(cv, r.x, r.y, r.w, r.h, rad, 1.0, l);
                }
                let isz = th.s(16.0).min(r.h - th.s(8.0)).max(8.0) as u16;
                let f = if *tone == Tone::Accent && r.h >= th.s(32.0) {
                    th.font_bold()
                } else {
                    th.font_small()
                };
                match icon {
                    Some(ic) if label.is_empty() => {
                        icons.draw(
                            cv,
                            *ic,
                            (r.cx() - isz as f32 / 2.0).round(),
                            (r.cy() - isz as f32 / 2.0).round(),
                            isz,
                            fg,
                            false,
                        );
                    }
                    Some(ic) => {
                        let lw = text.measure(f, label);
                        let total = isz as f32 + th.s(6.0) + lw;
                        let x0 = (r.cx() - total / 2.0).round();
                        icons.draw(
                            cv,
                            *ic,
                            x0,
                            (r.cy() - isz as f32 / 2.0).round(),
                            isz,
                            fg,
                            false,
                        );
                        text.draw_in(
                            cv,
                            f,
                            x0 + isz as f32 + th.s(6.0),
                            r.y,
                            lw + 2.0,
                            r.h,
                            label,
                            fg,
                            Align::Left,
                        );
                    }
                    None => {
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
            }
            Look::Seg {
                label,
                icon,
                sel,
                first,
                last,
            } => {
                let rr = th.s(7.0);
                // Le fond du groupe, part par part (les bouts arrondis).
                rrect(
                    cv,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    if *first || *last { rr } else { 0.0 },
                    p.sunk,
                );
                if !*first {
                    cv.fill_rect(r.x, r.y, r.w.min(rr), r.h, p.sunk);
                }
                if !*last {
                    cv.fill_rect(r.right() - rr.min(r.w), r.y, rr.min(r.w), r.h, p.sunk);
                }
                cv.fill_rect(r.x, r.y, r.w, 1.0, p.sunk_line);
                cv.fill_rect(r.x, r.bottom() - 1.0, r.w, 1.0, p.sunk_line.fade(0.5));
                if *first {
                    cv.fill_rect(r.x, r.y + rr, 1.0, r.h - 2.0 * rr, p.sunk_line.fade(0.6));
                }
                if *last {
                    cv.fill_rect(
                        r.right() - 1.0,
                        r.y + rr,
                        1.0,
                        r.h - 2.0 * rr,
                        p.sunk_line.fade(0.6),
                    );
                }
                let inner = r.inset(th.s(2.0));
                let icon_only = label.is_empty();
                let fg = if *sel {
                    if icon_only { p.on_accent } else { th.text }
                } else if hov {
                    th.text
                } else {
                    th.text_dim
                };
                if *sel {
                    let bg = if icon_only {
                        th.accent
                    } else {
                        p.card.mix(th.text, 0.12)
                    };
                    rrect(cv, inner.x, inner.y, inner.w, inner.h, th.s(5.0), bg);
                } else if hov {
                    rrect(cv, inner.x, inner.y, inner.w, inner.h, th.s(5.0), p.hov);
                }
                let f = th.font_small();
                let isz = th.s(15.0).min(r.h - th.s(10.0)).max(8.0) as u16;
                match icon {
                    Some(ic) if icon_only => {
                        icons.draw(
                            cv,
                            *ic,
                            (r.cx() - isz as f32 / 2.0).round(),
                            (r.cy() - isz as f32 / 2.0).round(),
                            isz,
                            fg,
                            false,
                        );
                    }
                    Some(ic) => {
                        let lw = text.measure(f, label);
                        let total = isz as f32 + th.s(6.0) + lw;
                        let x0 = (r.cx() - total / 2.0).round();
                        let ic_c = if *sel { th.accent } else { fg };
                        icons.draw(
                            cv,
                            *ic,
                            x0,
                            (r.cy() - isz as f32 / 2.0).round(),
                            isz,
                            ic_c,
                            false,
                        );
                        text.draw_in(
                            cv,
                            f,
                            x0 + isz as f32 + th.s(6.0),
                            r.y,
                            lw + 2.0,
                            r.h,
                            label,
                            fg,
                            Align::Left,
                        );
                    }
                    None => {
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
            }
            Look::Field {
                text: t,
                modified,
                muted,
            } => {
                rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
                rline(
                    cv,
                    r.x + 0.5,
                    r.y + 0.5,
                    r.w - 1.0,
                    r.h - 1.0,
                    rad,
                    1.0,
                    if hov {
                        p.sunk_line.mix(th.text, 0.2)
                    } else {
                        p.sunk_line
                    },
                );
                let mut x = r.x + th.s(10.0);
                if *modified {
                    cv.fill_circle(x + th.s(2.0), r.cy(), th.s(3.0), amber());
                    x += th.s(10.0);
                }
                let interactive = it.ctl != Ctl::None;
                text.draw_in(
                    cv,
                    th.font(),
                    x,
                    r.y,
                    r.right() - x - th.s(if interactive { 20.0 } else { 30.0 }),
                    r.h,
                    t,
                    if *muted { th.text_faint } else { th.text },
                    Align::Left,
                );
                if interactive {
                    let isz = th.s(12.0) as u16;
                    icons.draw(
                        cv,
                        Icon::ChevronDown,
                        (r.right() - th.s(18.0)).round(),
                        (r.cy() - isz as f32 / 2.0).round(),
                        isz,
                        th.text_dim,
                        false,
                    );
                }
            }
            Look::Chip {
                label,
                on,
                standby,
                amber: am,
                enabled,
            } => {
                let hv = hov && *enabled;
                let rr = th.s(5.0).min(r.h / 2.5);
                let (bg, line, fg) = if *am && *on {
                    (amber(), None, p.on_accent)
                } else if *on {
                    (th.accent, None, p.on_accent)
                } else if *standby {
                    (p.sunk, Some(th.accent.fade(0.6)), th.accent)
                } else {
                    (
                        if hv {
                            p.sunk.mix(th.text, 0.08)
                        } else {
                            p.sunk
                        },
                        Some(p.sunk_line),
                        if hv { th.text } else { th.text_dim },
                    )
                };
                let fg = if *enabled { fg } else { th.text_faint };
                rrect(cv, r.x, r.y, r.w, r.h, rr, bg);
                if let Some(l) = line {
                    rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, l);
                }
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
            Look::IconChip {
                icon,
                on,
                enabled,
                color,
            } => {
                let hv = hov && *enabled;
                let c = color.unwrap_or(th.accent);
                let rr = th.s(6.0).min(r.h / 2.5);
                let (bg, line, fg) = if *on {
                    (c.fade(0.18), c.fade(0.6), c.mix(th.text, 0.25))
                } else {
                    (
                        if hv {
                            p.sunk.mix(th.text, 0.08)
                        } else {
                            p.sunk
                        },
                        p.sunk_line,
                        if hv { th.text } else { th.text_dim },
                    )
                };
                let fg = if *enabled { fg } else { th.text_faint };
                rrect(cv, r.x, r.y, r.w, r.h, rr, bg);
                rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, line);
                icon_in(cv, icons, th, *icon, r, fg);
            }
            Look::Pill {
                color,
                label,
                driven,
            } => {
                let rr = r.h / 2.0;
                if *driven {
                    rrect(cv, r.x, r.y, r.w, r.h, rr, color.fade(0.16));
                    rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, color.fade(0.7));
                } else {
                    rrect(
                        cv,
                        r.x,
                        r.y,
                        r.w,
                        r.h,
                        rr,
                        if hov {
                            p.sunk.mix(th.text, 0.05)
                        } else {
                            p.sunk
                        },
                    );
                    rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, p.sunk_line);
                }
                text.draw_in(
                    cv,
                    th.font_small(),
                    r.x + th.s(24.0),
                    r.y,
                    r.w - th.s(28.0),
                    r.h,
                    label,
                    if *driven {
                        color.mix(th.text, 0.3)
                    } else {
                        th.text
                    },
                    Align::Left,
                );
            }
            Look::Scroll { pos, len, active } => {
                let c = if hov || *active {
                    th.text.fade(0.38)
                } else {
                    th.text.fade(0.2)
                };
                rrect(cv, r.x, r.y + pos, r.w, *len, r.w / 2.0, c);
            }
            Look::Search { focus, .. } => {
                rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
                let line = if *focus {
                    th.accent.fade(0.7)
                } else if hov {
                    p.sunk_line.mix(th.text, 0.2)
                } else {
                    p.sunk_line
                };
                rline(cv, r.x, r.y, r.w, r.h, rad, 1.0, line);
                let isz = th.s(14.0) as u16;
                icons.draw(
                    cv,
                    Icon::Search,
                    (r.x + th.s(9.0)).round(),
                    (r.cy() - isz as f32 / 2.0).round(),
                    isz,
                    th.text_dim,
                    false,
                );
                let mut tf = self.search.clone();
                let tx = r.x + th.s(26.0);
                let inner = R::new(tx, r.y + 1.0, r.right() - tx - 2.0, r.h - 2.0);
                // Le champ peint son propre fond : celui du creux.
                let mut th2 = th.clone();
                th2.field = p.sunk;
                th2.accent = Color { a: 0, ..th.accent };
                tf.draw_ex(
                    cv, text, &th2, inner.x, inner.y, inner.w, inner.h, *focus, "Filter", 0.0,
                );
            }
            Look::Badge { text: t, color } => {
                rrect(cv, r.x, r.y, r.w, r.h, r.h / 2.0, color.fade(0.16));
                text.draw_in(
                    cv,
                    th.font_small(),
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    t,
                    *color,
                    Align::Center,
                );
            }
            Look::Glyph { icon, color } => {
                let sz = th.s(14.0).min(r.h).min(r.w).max(8.0) as u16;
                let sz = if r.w > th.s(60.0) && r.h > th.s(60.0) {
                    th.s(22.0) as u16
                } else {
                    sz
                };
                icons.draw(
                    cv,
                    *icon,
                    (r.cx() - sz as f32 / 2.0).round(),
                    (r.cy() - sz as f32 / 2.0).round(),
                    sz,
                    *color,
                    false,
                );
            }
            Look::Wave { pts, color, phase } => {
                rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
                rline(cv, r.x, r.y, r.w, r.h, rad, 1.0, p.sunk_line);
                let ir = R::new(
                    r.x + th.s(8.0),
                    r.y + th.s(8.0),
                    r.w - th.s(16.0),
                    r.h - th.s(16.0),
                );
                cv.fill_rect(ir.x, ir.cy().round(), ir.w, 1.0, th.text.fade(0.07));
                cv.fill_rect(
                    ir.cx().round(),
                    r.y + th.s(4.0),
                    1.0,
                    r.h - th.s(8.0),
                    th.text.fade(0.05),
                );
                let mut path = Path::new();
                for (i, (t, v)) in pts.iter().enumerate() {
                    let (x, y) = (ir.x + ir.w * *t, ir.bottom() - ir.h * *v);
                    if i == 0 {
                        path.move_to(x, y);
                    } else {
                        path.line_to(x, y);
                    }
                }
                cv.stroke_path(&path, 2.0 * s, *color);
                if let Some((t, v)) = phase {
                    let x = ir.x + ir.w * *t;
                    cv.fill_rect(
                        x.round(),
                        r.y + th.s(4.0),
                        1.0,
                        r.h - th.s(8.0),
                        th.text.fade(0.22),
                    );
                    cv.fill_circle(
                        x,
                        ir.bottom() - ir.h * *v,
                        th.s(4.0),
                        color.mix(th.text, 0.4),
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
                dim,
                ..
            } => {
                let ks = size
                    .min(r.w)
                    .min(if label.is_empty() {
                        r.h
                    } else {
                        // Le nom et la valeur tiennent dans le rectangle.
                        r.h - th.s(25.0)
                    })
                    .max(th.s(12.0));
                let cx = r.cx();
                let cy = if label.is_empty() {
                    r.cy()
                } else {
                    r.y + ks / 2.0 + th.s(1.0)
                };
                let rr = ks / 2.0 - 1.0;
                let tw0 = (rr * 0.17).clamp(2.0 * s.min(1.5), 3.5 * s);
                let rv = rr - th.s(3.0) - tw0 / 2.0;
                // Les anneaux de modulation, dehors.
                let mut rm = rr - 0.5 * s;
                for g in rings {
                    let Ring {
                        lo,
                        hi,
                        color,
                        thin,
                        dim: rd,
                    } = g;
                    let c = if *rd { th.text_faint.fade(0.6) } else { *color };
                    arc(
                        cv,
                        cx,
                        cy,
                        rm.max(2.0),
                        *lo,
                        *hi,
                        if *thin { 1.5 * s } else { 2.0 * s },
                        c,
                    );
                    if *thin {
                        rm -= 2.0 * s;
                    }
                }
                // La piste, puis la valeur.
                arc(cv, cx, cy, rv, 0.0, 1.0, tw0, p.sunk.mix(th.text, 0.10));
                let base_c = if !*enabled {
                    th.text_faint
                } else if *dim {
                    th.text_dim.mix(p.sunk, 0.35)
                } else {
                    th.accent
                };
                if *bipolar {
                    arc(cv, cx, cy, rv, 0.5, *v, tw0, base_c);
                } else {
                    arc(cv, cx, cy, rv, 0.0, *v, tw0, base_c);
                }
                // Le corps et son aiguille (la base).
                let rb = (rv - tw0 / 2.0 - th.s(2.0)).max(2.0);
                cv.fill_circle(cx, cy, rb, p.card.mix(th.text, 0.05));
                cv.stroke_circle(cx, cy, rb, 1.0, p.card_line.mix(th.text, 0.06));
                let (px, py) = knob_pt(cx, cy, rb * 0.78, *v);
                let needle = if *enabled && !*dim {
                    th.text
                } else {
                    th.text_dim
                };
                cv.line(cx, cy, px, py, 2.0 * s, needle);
                // Le point : ce qui joue.
                if let Some(lv) = live {
                    if (lv - v).abs() > 0.004 || !rings.is_empty() {
                        let (lx, ly) = knob_pt(cx, cy, rv, *lv);
                        cv.fill_circle(lx, ly, (tw0 * 0.75).max(2.0), th.text);
                    }
                }
                if let Some(c) = hl {
                    cv.stroke_circle(cx, cy, rr + th.s(2.5), 1.5 * s, c.fade(0.9));
                }
                if hov && self.carry.is_some() {
                    cv.stroke_circle(cx, cy, rr + th.s(2.5), 2.0 * s, th.accent);
                }
                if !label.is_empty() {
                    let ty = cy + ks / 2.0 + th.s(1.0);
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        ty,
                        r.w,
                        th.s(12.0),
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
                        th.s(12.0),
                        value,
                        if *enabled { th.text } else { th.text_faint },
                        Align::Center,
                    );
                }
            }
            Look::Fader { v, label, enabled } => {
                let rr = (r.h / 2.0).min(rad);
                rrect(cv, r.x, r.y, r.w, r.h, rr, p.sunk);
                rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, p.sunk_line);
                let fw = (r.w * *v as f32).clamp(0.0, r.w);
                if fw > 1.0 {
                    rrect(
                        cv,
                        r.x,
                        r.y,
                        fw,
                        r.h,
                        rr,
                        if *enabled {
                            th.accent.fade(if hov { 0.7 } else { 0.55 })
                        } else {
                            th.text_faint.fade(0.3)
                        },
                    );
                }
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
                let rr = r.h / 2.0;
                rrect(cv, r.x, r.y, r.w, r.h, rr, p.sunk);
                let (a, b) = (r.x + r.w * *lo as f32, r.x + r.w * *hi as f32);
                rrect(cv, a, r.y, (b - a).max(1.0), r.h, rr, th.accent.fade(0.8));
                let hw = th.s(8.0);
                for x in [a, b] {
                    rrect(
                        cv,
                        x - hw / 2.0,
                        r.y - th.s(4.0),
                        hw,
                        r.h + th.s(8.0),
                        th.s(3.0),
                        th.text,
                    );
                }
            }
            Look::Pad(pl) => self.draw_pad(cv, th, p, r, pl),
            Look::Dot { color, hollow } => {
                let rr = r.w.min(r.h) / 2.0;
                if hov {
                    cv.fill_circle(r.cx(), r.cy(), rr * 2.1, color.fade(0.3));
                }
                cv.fill_circle(r.cx(), r.cy(), rr, rgb(0, 0, 0).fade(0.45));
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
                let rr = th.s(7.0);
                let (bg, line) = if *sel {
                    (color.fade(0.12), color.fade(0.75))
                } else if *hl || hov {
                    (p.well.mix(th.text, 0.04), p.well_line.mix(th.text, 0.15))
                } else {
                    (p.well, p.well_line)
                };
                rrect(cv, r.x, r.y, r.w, r.h, rr, p.well);
                rrect(cv, r.x, r.y, r.w, r.h, rr, bg);
                rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, line);
                let tx = r.x + th.s(21.0);
                text.draw_in(
                    cv,
                    th.font_small(),
                    tx,
                    r.y + th.s(2.0),
                    r.right() - tx - th.s(14.0),
                    th.s(20.0),
                    title,
                    if *on { th.text } else { th.text_dim },
                    Align::Left,
                );
                if *count > 0 {
                    let t = if *count > 9 {
                        "9+".to_string()
                    } else {
                        count.to_string()
                    };
                    text.draw_in(
                        cv,
                        th.font_small(),
                        r.x,
                        r.y + th.s(2.0),
                        r.w - th.s(6.0),
                        th.s(20.0),
                        &t,
                        color.mix(th.text, 0.3),
                        Align::Right,
                    );
                }
                let vr = R::new(
                    r.x + th.s(8.0),
                    r.y + th.s(24.0),
                    r.w - th.s(16.0),
                    r.h - th.s(30.0),
                );
                let c = if *on {
                    color.fade(0.9)
                } else {
                    th.text_faint.fade(0.45)
                };
                let slot = match it.ctl {
                    Ctl::Tile(sl) => sl,
                    _ => 0,
                };
                let mut path = Path::new();
                let mut first = true;
                let mut to = |x: f32, y: f32, path: &mut Path| {
                    let (px, py) = (vr.x + vr.w * x, vr.bottom() - vr.h * y);
                    if first {
                        path.move_to(px, py);
                        first = false;
                    } else {
                        path.line_to(px, py);
                    }
                };
                match (shape, band_of(slot)) {
                    (Some((sh, curve)), _) => {
                        if *on {
                            for i in 0..=48 {
                                let t = i as f64 / 48.0;
                                to(
                                    t as f32,
                                    shape_value(*sh, *curve, t * 2.0) as f32,
                                    &mut path,
                                );
                            }
                        } else {
                            to(0.0, 0.5, &mut path);
                            to(1.0, 0.5, &mut path);
                        }
                    }
                    (None, Some(Band::Env(_))) => {
                        for (x, y) in [(0.0, 0.0), (0.12, 1.0), (0.3, 0.6), (0.7, 0.6), (1.0, 0.0)]
                        {
                            to(x, y, &mut path);
                        }
                    }
                    (None, Some(Band::Seq(n))) => {
                        let hs: [f32; 8] = if n == 1 {
                            [0.7, 0.3, 0.9, 0.2, 0.6, 0.15, 0.8, 0.45]
                        } else {
                            [0.3, 0.8, 0.15, 0.6, 0.35, 0.95, 0.25, 0.55]
                        };
                        for (i, h) in hs.iter().enumerate() {
                            let x = vr.x + vr.w * (i as f32 + 0.5) / 8.0;
                            cv.fill_rect(
                                (x - s).round(),
                                vr.bottom() - vr.h * h,
                                (2.0 * s).max(1.0),
                                vr.h * h,
                                c,
                            );
                        }
                    }
                    _ => {
                        let pts: &[(f32, f32)] = match slot {
                            231 => &[
                                (0.0, 0.2),
                                (0.3, 0.2),
                                (0.3, 0.6),
                                (0.6, 0.6),
                                (0.6, 0.4),
                                (0.8, 0.4),
                                (0.8, 0.9),
                                (1.0, 0.9),
                            ],
                            232 => &[
                                (0.0, 0.0),
                                (0.0, 0.6),
                                (0.25, 0.6),
                                (0.25, 0.0),
                                (0.4, 0.0),
                                (0.4, 0.9),
                                (0.65, 0.9),
                                (0.65, 0.0),
                                (0.8, 0.0),
                                (0.8, 0.4),
                                (1.0, 0.4),
                            ],
                            233 => &[
                                (0.0, 0.5),
                                (0.15, 0.9),
                                (0.3, 0.2),
                                (0.45, 0.7),
                                (0.6, 0.35),
                                (0.75, 0.95),
                                (0.9, 0.15),
                                (1.0, 0.6),
                            ],
                            _ => &[
                                (0.0, 0.1),
                                (0.33, 0.1),
                                (0.33, 0.5),
                                (0.66, 0.5),
                                (0.66, 0.9),
                                (1.0, 0.9),
                            ],
                        };
                        for (x, y) in pts {
                            to(*x, *y, &mut path);
                        }
                    }
                }
                cv.stroke_path(&path, 1.5 * s, c);
                if let (Some((sh, curve)), Some(ph), true) = (shape, phase, *on) {
                    let t = ph.fract();
                    let v = shape_value(*sh, *curve, t);
                    cv.fill_circle(
                        vr.x + vr.w * (t * 0.5) as f32,
                        vr.bottom() - vr.h * v as f32,
                        2.2 * s,
                        th.text,
                    );
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
                rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
                rline(cv, r.x, r.y, r.w, r.h, rad, 1.0, p.sunk_line);
                let cw = r.w / (*n).max(1) as f32;
                for (k, v) in vals.iter().enumerate() {
                    let x = r.x + cw * k as f32;
                    let a = if (k as u8) < *len { 1.0 } else { 0.22 };
                    if *play == Some(k as u8) {
                        cv.fill_rect(x, r.y + 1.0, cw, r.h - 2.0, th.text.fade(0.12));
                    }
                    if k % 4 == 0 && k > 0 {
                        cv.fill_rect(x, r.y + 3.0, 1.0, r.h - 6.0, p.sunk_line);
                    }
                    let gx = (cw * 0.12).max(1.5);
                    if *bools {
                        let c = if *v > 0.5 {
                            color.fade(0.9 * a)
                        } else {
                            th.text.fade(0.06 * a)
                        };
                        rrect(
                            cv,
                            x + gx,
                            r.y + 3.0,
                            cw - 2.0 * gx,
                            r.h - 6.0,
                            th.s(3.0),
                            c,
                        );
                    } else {
                        let y0 = r.bottom() - 2.0 - (r.h - 4.0) * *origin as f32;
                        let y1 = r.bottom() - 2.0 - (r.h - 4.0) * *v as f32;
                        let (top, bot) = (y0.min(y1), y0.max(y1));
                        cv.fill_rect(
                            x + gx,
                            top,
                            cw - 2.0 * gx,
                            (bot - top).max(1.5),
                            color.fade(0.85 * a),
                        );
                    }
                }
                if *origin > 0.0 && !*bools {
                    let y0 = r.bottom() - 2.0 - (r.h - 4.0) * *origin as f32;
                    cv.fill_rect(r.x, y0, r.w, 1.0, th.text_faint.fade(0.6));
                }
            }
            Look::Curve { pts, color, level } => {
                let ir = r.inset(th.s(3.0));
                let mut path = Path::new();
                for (i, (t, v)) in pts.iter().enumerate() {
                    let (x, y) = (ir.x + ir.w * *t, ir.bottom() - ir.h * *v);
                    if i == 0 {
                        path.move_to(x, y);
                    } else {
                        path.line_to(x, y);
                    }
                }
                cv.stroke_path(&path, 1.75 * s, *color);
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
                    (true, false) => rgb(28, 28, 30),
                    (false, false) => {
                        if hov {
                            rgb(240, 240, 240)
                        } else {
                            rgb(222, 222, 224)
                        }
                    }
                };
                rrect(cv, r.x, r.y - th.s(4.0), r.w, r.h + th.s(4.0), th.s(3.0), c);
                if !black {
                    cv.fill_rect(r.x, r.y, r.w, 1.0, rgb(0, 0, 0).fade(0.25));
                }
            }
            Look::SrcLabel { dots, label } => {
                if hov {
                    rrect(
                        cv,
                        r.x - th.s(3.0),
                        r.y + 1.0,
                        r.w + th.s(3.0),
                        r.h - 2.0,
                        th.s(4.0),
                        p.hov,
                    );
                }
                let mut x = r.x;
                let dr = th.s(3.5);
                for (c, hollow) in dots {
                    if *hollow {
                        cv.stroke_circle(x + dr, r.cy(), dr, 1.2 * s, c.fade(0.6));
                    } else {
                        cv.fill_circle(x + dr, r.cy(), dr, *c);
                    }
                    x += dr * 2.0 + th.s(3.0);
                }
                if !label.is_empty() {
                    let c = dots
                        .first()
                        .map_or(th.text_faint, |d| d.0.mix(th.text, 0.3));
                    text.draw_in(
                        cv,
                        th.font_small(),
                        x + th.s(1.0),
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

    fn draw_panel(&self, cv: &mut Canvas<'_>, th: &Theme, p: &Pal, it: &Item, sh: Shade) {
        let r = it.r;
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let hov = self.hovered(it);
        let card_r = th.s(9.0);
        match sh {
            Shade::Zone => {
                cv.fill_rect(r.x, r.y, r.w, r.h, th.bg);
                if let Ctl::Seam(_) = it.ctl {
                    if hov {
                        cv.fill_rect(r.cx() - 0.5, r.y, 1.0, r.h, th.accent.fade(0.7));
                    }
                }
            }
            Shade::Bar => {
                cv.fill_rect(r.x, r.y, r.w, r.h, th.bar);
                cv.fill_rect(r.x, r.y, r.w, 1.0, rgb(255, 255, 255).fade(0.04));
                cv.fill_rect(r.x, r.bottom() - 1.0, r.w, 1.0, rgb(0, 0, 0).fade(0.35));
            }
            Shade::Card => {
                rrect(
                    cv,
                    r.x,
                    r.y + 1.0,
                    r.w,
                    r.h,
                    card_r,
                    rgb(0, 0, 0).fade(0.22),
                );
                rrect(cv, r.x, r.y, r.w, r.h, card_r, p.card);
                rline(cv, r.x, r.y, r.w, r.h, card_r, 1.0, p.card_line);
                if matches!(it.ctl, Ctl::FxHead(_)) && hov {
                    rrect(
                        cv,
                        r.x + 1.0,
                        r.y + 1.0,
                        r.w - 2.0,
                        r.h - 2.0,
                        card_r - 1.0,
                        p.hov,
                    );
                }
            }
            Shade::Head => {
                let c = if hov {
                    p.head.mix(th.text, 0.04)
                } else {
                    p.head
                };
                let rr = card_r - 1.0;
                rrect(cv, r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 1.0, rr, c);
                cv.fill_rect(r.x + 1.0, r.bottom() - rr, r.w - 2.0, rr, c);
                cv.fill_rect(r.x + 1.0, r.bottom() - 1.0, r.w - 2.0, 1.0, p.card_line);
            }
            Shade::Sunk => {
                rrect(cv, r.x, r.y, r.w, r.h, th.s(6.0), p.sunk);
                rline(cv, r.x, r.y, r.w, r.h, th.s(6.0), 1.0, p.sunk_line);
            }
            Shade::Well => {
                let rr = th.s(7.0);
                rrect(cv, r.x, r.y, r.w, r.h, rr, p.well);
                rline(
                    cv,
                    r.x + 0.5,
                    r.y + 0.5,
                    r.w - 1.0,
                    r.h - 1.0,
                    rr,
                    1.0,
                    if hov {
                        p.well_line.mix(th.text, 0.15)
                    } else {
                        p.well_line
                    },
                );
            }
            Shade::Tint(c) => {
                let rr = th.s(7.0);
                rrect(cv, r.x, r.y, r.w, r.h, rr, p.well);
                rrect(cv, r.x, r.y, r.w, r.h, rr, c);
                rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, stronger(c, 3.5));
            }
            Shade::Drop => {
                let rr = th.s(9.0);
                rrect(cv, r.x, r.y, r.w, r.h, rr, th.text.fade(0.015));
                // Des tirets sur le pourtour.
                let c = p.card_line.mix(th.text, 0.1);
                let (dash, gap) = (th.s(6.0), th.s(5.0));
                let mut x = r.x + rr;
                while x < r.right() - rr {
                    let w = dash.min(r.right() - rr - x);
                    cv.fill_rect(x, r.y, w, 1.0, c);
                    cv.fill_rect(x, r.bottom() - 1.0, w, 1.0, c);
                    x += dash + gap;
                }
                let mut y = r.y + rr;
                while y < r.bottom() - rr {
                    let h = dash.min(r.bottom() - rr - y);
                    cv.fill_rect(r.x, y, 1.0, h, c);
                    cv.fill_rect(r.right() - 1.0, y, 1.0, h, c);
                    y += dash + gap;
                }
            }
        }
    }

    fn draw_pad(&self, cv: &mut Canvas<'_>, th: &Theme, p: &Pal, r: R, pl: &PadLook) {
        // Tout reste dans le carre : la lueur du point ne deborde pas sur la
        // carte (la ou la comparaison des images ne repeint pas).
        cv.push_clip(r.x, r.y, r.w, r.h);
        self.draw_pad_in(cv, th, p, r, pl);
        cv.pop_clip();
    }

    fn draw_pad_in(&self, cv: &mut Canvas<'_>, th: &Theme, p: &Pal, r: R, pl: &PadLook) {
        let s = th.scale;
        let rr = th.s(8.0);
        rrect(cv, r.x, r.y, r.w, r.h, rr, p.sunk);
        let px = |x: f64| r.x + r.w * x as f32;
        let py = |y: f64| r.bottom() - r.h * y as f32;
        // La grille, fixe : elle ne bouge jamais avec le point.
        let g = if pl.grains.is_empty() {
            8
        } else {
            pl.grid.max(2)
        };
        for i in 1..g {
            let k = i as f32 / g as f32;
            let c = if i * 2 == g {
                th.text.fade(0.09)
            } else {
                th.text.fade(0.045)
            };
            cv.fill_rect((r.x + r.w * k).round(), r.y + 1.0, 1.0, r.h - 2.0, c);
            cv.fill_rect(r.x + 1.0, (r.y + r.h * k).round(), r.w - 2.0, 1.0, c);
        }
        for (gx, gy) in &pl.grains {
            cv.stroke_circle(
                px(*gx),
                py(*gy),
                r.w / g as f32 / 2.0,
                1.0,
                th.text_faint.fade(0.25),
            );
            cv.fill_circle(px(*gx), py(*gy), 1.5 * s, th.text_dim);
        }
        if let Some(z) = pl.zone {
            let (x0, x1, y0, y1) = (px(z[0]), px(z[1]), py(z[3]), py(z[2]));
            let veil = th.bg.fade(0.55);
            cv.fill_rect(r.x, r.y, r.w, y0 - r.y, veil);
            cv.fill_rect(r.x, y1, r.w, r.bottom() - y1, veil);
            cv.fill_rect(r.x, y0, x0 - r.x, y1 - y0, veil);
            cv.fill_rect(x1, y0, r.right() - x1, y1 - y0, veil);
            cv.stroke_rect(x0, y0, x1 - x0, y1 - y0, 1.0, th.accent.fade(0.7));
        }
        if let Some(f) = &pl.figure {
            let mut path = Path::new();
            for (i, (x, y)) in f.iter().enumerate() {
                if i == 0 {
                    path.move_to(px(*x), py(*y));
                } else {
                    path.line_to(px(*x), py(*y));
                }
            }
            cv.stroke_path(&path, 1.25 * s, th.accent.fade(0.35));
        }
        rline(cv, r.x, r.y, r.w, r.h, rr, 1.0, p.sunk_line);
        // Le sillage : de plus en plus net jusqu'au point.
        let n = pl.trail.len();
        for (i, w) in pl.trail.windows(2).enumerate() {
            let a = (i + 1) as f32 / n.max(1) as f32;
            cv.line(
                px(w[0].0),
                py(w[0].1),
                px(w[1].0),
                py(w[1].1),
                (1.0 + 1.5 * a) * s,
                th.accent.fade(0.08 + 0.5 * a),
            );
        }
        let (dx, dy) = (px(pl.x), py(pl.y));
        // Les reperes sur les bords (la ou il est), a la couleur de ce qui
        // mene l'axe.
        let cx_c = pl.drive_x.unwrap_or(th.accent);
        let cy_c = pl.drive_y.unwrap_or(th.accent);
        let tick = th.s(7.0);
        let tw0 = (2.0 * s).max(1.5);
        rrect(cv, dx - tw0 / 2.0, r.y, tw0, tick, tw0 / 2.0, cx_c);
        rrect(
            cv,
            dx - tw0 / 2.0,
            r.bottom() - tick,
            tw0,
            tick,
            tw0 / 2.0,
            cx_c,
        );
        rrect(cv, r.x, dy - tw0 / 2.0, tick, tw0, tw0 / 2.0, cy_c);
        rrect(
            cv,
            r.right() - tick,
            dy - tw0 / 2.0,
            tick,
            tw0,
            tw0 / 2.0,
            cy_c,
        );
        // L'ancre.
        let (ax, ay) = (px(pl.ax), py(pl.ay));
        let dot = (r.w * 0.024).clamp(th.s(5.0), th.s(8.0));
        if (ax - dx).abs() + (ay - dy).abs() > r.w * 0.01 {
            cv.stroke_circle(ax, ay, dot * 0.8, 1.5 * s, th.text.fade(0.35));
        }
        // Le point.
        let c = th.accent;
        cv.fill_circle(dx, dy, dot * 3.2, c.fade(if pl.runs { 0.16 } else { 0.11 }));
        cv.fill_circle(dx, dy, dot * 1.9, c.fade(0.18));
        cv.fill_circle(dx, dy, dot + 1.5 * s, rgb(0, 0, 0).fade(0.5));
        cv.fill_circle(dx, dy, dot, c);
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
