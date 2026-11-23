//! Le dessin de l'editeur : la barre (des icones et des valeurs, pas une
//! phrase), la regle, le clavier, les rangs, les notes, la velocite ; ou le
//! clip audio (onde, fondus, gain, bords).

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Path, hex};
use cp_roll::{Note, note_name};
use cp_ui::{Icon, IconCache, Theme};

use crate::{Clip, Colors, Ed, HBox, Hit, Lay, Part, Rows, grid_name};

const BAR_H: f32 = 36.0;
const RULER_H: f32 = 24.0;
const KEYS_W: f32 = 60.0;
const VEL_H: f32 = 84.0;
const HS_H: f32 = 12.0;
const VS_W: f32 = 10.0;

/// Les couleurs par canal : les six premieres sont celles des cordes de
/// Guitar Tab (canal = corde).
const CHAN_C: [u32; 16] = [
    0xa8c97f, 0xde9a86, 0xc99ad6, 0x8fb0de, 0x7fc6b4, 0xd9b45a, 0xe58a8a, 0x8ad0e5, 0xc4d47a,
    0xe0a0c8, 0x9aa8f0, 0xe5b07a, 0x7ad4a0, 0xb8a0e0, 0xd8c890, 0xa0c0c0,
];

/// Par hauteur : les octaves partagent la couleur ; la clarte varie aussi
/// (pas la teinte seule).
const PITCH_C: [u32; 12] = [
    0xe5807a, 0xe59a72, 0xe2b56a, 0xd4cc6a, 0xa9cf7c, 0x7fcf9a, 0x6ccdbc, 0x72bde0, 0x8ca4ea,
    0xa894ea, 0xc690e2, 0xe28cc4,
];

const BLACK: [i32; 5] = [1, 3, 6, 8, 10];

fn vel_color(v: i32) -> Color {
    // Froid -> chaud, de plus en plus clair.
    let t = ((v - 40) as f32 / 87.0).clamp(0.0, 1.0);
    let a = hex(0x5d8fd6);
    let b = hex(0xf0a050);
    a.mix(b, t)
}

impl Ed {
    pub(crate) fn note_color(&self, n: &Note) -> Color {
        match self.colors {
            Colors::Channel => hex(CHAN_C[n.chan.clamp(0, 15) as usize]),
            Colors::Pitch => hex(PITCH_C[n.pitch.rem_euclid(12) as usize]),
            Colors::Velocity => vel_color(n.vel),
        }
    }

    fn layout(&mut self, w: f32, h: f32, s: f32) {
        let px = |v: f32| (v * s).round();
        let bar_h = px(BAR_H);
        let top = bar_h + 2.0;
        let ruler_h = px(RULER_H);
        let sx = px(KEYS_W) + 1.0;
        let sw = (w - sx - px(VS_W)).max(10.0);
        let hs_h = px(HS_H);
        let hs_y = h - hs_h;
        let midi = matches!(self.clip, Clip::Midi(_));
        let vel_h = if midi && self.vel_lane {
            px(VEL_H)
        } else {
            0.0
        };
        let vel_y = hs_y - vel_h;
        let area0 = top + ruler_h + 1.0;
        let area1 = if vel_h > 0.0 { vel_y - 3.0 } else { hs_y };
        // Les rangs ne changent jamais pendant un geste (le mode Notes en
        // depend) : la note glissee reste sous la souris.
        if !self.dragging() || self.rows.is_empty() {
            self.rows = self.compute_rows();
        }
        let _ = midi;
        let gy = area0;
        let gh = (area1 - gy).max(10.0);
        let n = self.rows.len().max(1) as f32;
        let rh = (self.row_h * s).max(3.0);
        self.lay = Lay {
            w,
            h,
            bar_h,
            top,
            ruler_h,
            sx,
            sw,
            gy,
            gh,
            vel_y,
            vel_h,
            hs_y,
            hs_h,
            ppq: self.zoom * s,
            rh,
            content_h: rh * n,
        };
        let maxv = (self.lay.content_h - gh).max(0.0);
        self.vscroll = self.vscroll.clamp(0.0, maxv);
        if !self.dragging() {
            self.apply_camera();
        }
    }

    /// Dessine toute la fenetre et retient les cibles de la barre.
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        w: f32,
        h: f32,
    ) {
        // Chaque decoupage pose est retire : sinon la suite de l'image
        // sortirait de la zone a redessiner (voir `Canvas::restore_clips`).
        let (depth, clip) = (cv.clip_depth(), cv.clip());
        self.draw_scene(cv, text, icons, th, w, h);
        let ok = cv.restore_clips(depth, clip);
        if cfg!(test) {
            assert!(ok, "decoupages desequilibres pendant le dessin");
        }
    }

    fn draw_scene(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        w: f32,
        h: f32,
    ) {
        self.scale = th.scale;
        self.layout(w, h, th.scale);
        let mut bx: Vec<HBox> = Vec::new();
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_bar(cv, text, icons, th, &mut bx);
        cv.fill_rect(0.0, self.lay.bar_h, w, 2.0, th.seam);
        self.draw_ruler(cv, text, th);
        match self.clip {
            Clip::Midi(_) => {
                self.draw_rows(cv, text, th);
                self.draw_notes(cv, text, th);
                self.draw_keys(cv, text, icons, th);
                if self.lay.vel_h > 0.0 {
                    self.draw_vel(cv, icons, th);
                }
            }
            Clip::Audio(_) => self.draw_audio(cv, th),
            Clip::None => {
                let s = th.scale;
                let isz = (48.0 * s) as u16;
                icons.draw(
                    cv,
                    Icon::Piano,
                    ((w - isz as f32) * 0.5).round(),
                    ((h - isz as f32) * 0.45).round(),
                    isz,
                    th.text_faint.fade(0.6),
                    false,
                );
            }
        }
        // La tete de lecture et les curseurs sont des traits, poses par-dessus
        // la scene (`strokes`).
        self.draw_scrollbars(cv, th);
        // La palette du clic droit, par-dessus tout (ses cibles d'abord).
        self.draw_palette(cv, text, th, &mut bx);
        self.boxes = bx;
    }

    // -----------------------------------------------------------------------
    // La barre
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn icon_btn(
        &self,
        cv: &mut Canvas<'_>,
        icons: &mut IconCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        x: f32,
        y: f32,
        h: f32,
        icon: Icon,
        on: bool,
        hit: Hit,
    ) -> f32 {
        let hov = self.hover == hit;
        cv.fill_round_rect(
            x,
            y,
            h,
            h,
            th.s(3.0),
            if hov {
                th.hover.mix(th.text, 0.06)
            } else {
                th.field
            },
        );
        let isz = th.s(15.0) as u16;
        let o = ((h - isz as f32) * 0.5).round();
        icons.draw(
            cv,
            icon,
            x + o,
            y + o,
            isz,
            if on { th.accent } else { th.text_dim },
            false,
        );
        bx.push((x, y, h, h, hit));
        h
    }

    #[allow(clippy::too_many_arguments)]
    fn chip(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        x: f32,
        y: f32,
        h: f32,
        label: &str,
        on: bool,
        hit: Hit,
    ) -> f32 {
        let f = Font::new(0, th.s(12.0) as u16, 500);
        let w = text.measure(f, label).ceil() + th.s(18.0);
        let hov = self.hover == hit;
        cv.fill_round_rect(
            x,
            y,
            w,
            h,
            th.s(3.0),
            if hov {
                th.hover.mix(th.text, 0.06)
            } else {
                th.field
            },
        );
        text.draw_in(
            cv,
            f,
            x,
            y,
            w,
            h,
            label,
            if on { th.accent } else { th.text },
            Align::Center,
        );
        bx.push((x, y, w, h, hit));
        w
    }

    fn draw_bar(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        cv.fill_rect(0.0, 0.0, l.w, l.bar_h, th.bar);
        let by = ((l.bar_h - s(24.0)) * 0.5).floor();
        let bh = s(24.0);
        let mut x = s(10.0);
        let (icon, name) = match &self.clip {
            Clip::Midi(m) => (Icon::Piano, m.name.as_str()),
            Clip::Audio(a) => (Icon::AudioLines, a.name.as_str()),
            Clip::None => (Icon::Piano, ""),
        };
        let isz = s(17.0) as u16;
        icons.draw(
            cv,
            icon,
            x,
            ((l.bar_h - isz as f32) * 0.5).round(),
            isz,
            th.accent,
            false,
        );
        x += isz as f32 + s(8.0);
        if !name.is_empty() {
            let f = Font::new(0, s(13.0) as u16, 700);
            let nw = text.measure(f, name).ceil().min(s(220.0));
            text.draw_in(cv, f, x, 0.0, nw, l.bar_h, name, th.text, Align::Left);
            x += nw + s(14.0);
            cv.fill_rect(x - s(7.0), by + s(3.0), 1.0, bh - s(6.0), th.seam);
        }
        match &self.clip {
            Clip::Midi(m) => {
                x += self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Magnet,
                    self.snap,
                    Hit::Snap,
                ) + s(2.0);
                x += self.chip(
                    cv,
                    text,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    grid_name(self.grid),
                    false,
                    Hit::GridMenu,
                ) + s(10.0);
                let sc = m
                    .roll
                    .scale
                    .map_or("\u{266f} \u{266d}".to_string(), |sc| sc.name());
                x += self.chip(
                    cv,
                    text,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    &sc,
                    m.roll.scale.is_some(),
                    Hit::ScaleMenu,
                ) + s(10.0);
                // La couleur : la palette et trois nuances du mode.
                let cw = s(46.0);
                let hov = self.hover == Hit::Color;
                cv.fill_round_rect(
                    x,
                    by,
                    cw,
                    bh,
                    s(3.0),
                    if hov {
                        th.hover.mix(th.text, 0.06)
                    } else {
                        th.field
                    },
                );
                let pi = s(15.0) as u16;
                icons.draw(
                    cv,
                    Icon::Palette,
                    x + s(5.0),
                    by + ((bh - pi as f32) * 0.5).round(),
                    pi,
                    th.text_dim,
                    false,
                );
                let sw: [Color; 3] = match self.colors {
                    Colors::Channel => [hex(CHAN_C[0]), hex(CHAN_C[2]), hex(CHAN_C[5])],
                    Colors::Pitch => [hex(PITCH_C[0]), hex(PITCH_C[4]), hex(PITCH_C[8])],
                    Colors::Velocity => [vel_color(50), vel_color(90), vel_color(125)],
                };
                for (k, c) in sw.iter().enumerate() {
                    cv.fill_round_rect(
                        x + s(24.0) + k as f32 * s(6.0),
                        by + s(6.0),
                        s(4.0),
                        bh - s(12.0),
                        s(1.5),
                        *c,
                    );
                }
                bx.push((x, by, cw, bh, Hit::Color));
                x += cw + s(2.0);
                x += self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Fold,
                    self.rows_mode == Rows::Notes,
                    Hit::Rows,
                ) + s(10.0);
                x += self.chip(cv, text, th, bx, x, by, bh, "Q", false, Hit::Quantize) + s(2.0);
                x += self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Wand,
                    false,
                    Hit::Transform,
                ) + s(2.0);
                let _ = self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Bars,
                    self.vel_lane,
                    Hit::VelToggle,
                );
            }
            Clip::Audio(a) => {
                x += self.chip(
                    cv,
                    text,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    &if a.gain_db <= -60.0 {
                        "-inf dB".to_string()
                    } else {
                        format!("{:+.1} dB", a.gain_db)
                    },
                    false,
                    Hit::GainChip,
                ) + s(2.0);
                x += self.chip(
                    cv,
                    text,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    &format!("{:+.0} st", a.pitch),
                    a.pitch.abs() > 1e-6,
                    Hit::PitchChip,
                ) + s(2.0);
                x += self.chip(
                    cv,
                    text,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    &format!("{:.2}\u{00d7}", a.rate),
                    (a.rate - 1.0).abs() > 1e-6,
                    Hit::RateChip,
                ) + s(10.0);
                x += self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Reverse,
                    a.reverse,
                    Hit::Reverse,
                ) + s(2.0);
                let _ = self.icon_btn(
                    cv,
                    icons,
                    th,
                    bx,
                    x,
                    by,
                    bh,
                    Icon::Normalize,
                    false,
                    Hit::Normalize,
                );
            }
            Clip::None => {}
        }
        // A droite : entendre les notes, suivre la lecture, l'aide.
        let mut rx = l.w - s(10.0) - bh;
        self.icon_btn(cv, icons, th, bx, rx, by, bh, Icon::Help, false, Hit::Help);
        rx -= s(4.0) + bh;
        self.icon_btn(
            cv,
            icons,
            th,
            bx,
            rx,
            by,
            bh,
            Icon::Locate,
            self.follow,
            Hit::Follow,
        );
        if matches!(self.clip, Clip::Midi(_)) {
            rx -= s(4.0) + bh;
            self.icon_btn(
                cv,
                icons,
                th,
                bx,
                rx,
                by,
                bh,
                Icon::Headphones,
                self.hear,
                Hit::Hear,
            );
        }
        if !self.status.is_empty() {
            let f = Font::new(0, s(11.0) as u16, 400);
            let mw = rx - s(12.0) - (x + s(40.0));
            if mw > s(40.0) {
                text.draw_in(
                    cv,
                    f,
                    x + s(40.0),
                    0.0,
                    mw,
                    l.bar_h,
                    &self.status,
                    th.text_dim,
                    Align::Right,
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // La regle
    // -----------------------------------------------------------------------

    fn draw_ruler(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let ry = l.top;
        let rp = &th.rp;
        cv.fill_rect(0.0, ry, l.w, l.ruler_h, rp.m_ruler_bg);
        cv.fill_rect(0.0, ry + l.ruler_h, l.w, 1.0, th.seam);
        if !cv.push_clip(l.sx, ry, l.sw, l.ruler_h) {
            cv.pop_clip();
            return;
        }
        // L'item : une bande plus claire.
        if let Some((a, b)) = self.span() {
            let (xa, xb) = (self.x_of(a), self.x_of(b));
            cv.fill_rect(xa, ry, xb - xa, l.ruler_h, th.text.with_alpha(14));
        }
        if let Some((a, b)) = self.range {
            let (xa, xb) = (self.x_of(a).round(), self.x_of(b).round());
            cv.fill_rect(xa, ry, xb - xa, l.ruler_h, rp.m_selbg.with_alpha(60));
        }
        let bq = self.bar_qn();
        let first = (self.scroll / bq).floor().max(0.0) as i64;
        let last = ((self.scroll + self.view_qn()) / bq).ceil() as i64;
        let bar_px = bq as f32 * l.ppq;
        let step = [1i64, 2, 4, 8, 16, 32, 64]
            .into_iter()
            .find(|&k| bar_px * k as f32 >= s(30.0))
            .unwrap_or(128);
        let fb = Font::new(0, s(11.0) as u16, 700);
        for m in first..=last {
            let q = m as f64 * bq;
            let x = self.x_of(q).round();
            let major = m % step == 0;
            cv.fill_rect(
                x,
                ry + if major { s(3.0) } else { l.ruler_h - s(8.0) },
                1.0,
                if major { l.ruler_h - s(3.0) } else { s(8.0) },
                rp.m_ruler_fg.fade(0.6),
            );
            if major {
                text.draw(
                    cv,
                    fb,
                    x + s(4.0),
                    ry + s(14.0),
                    &(m + 1).to_string(),
                    rp.m_ruler_fg,
                );
            }
            if step == 1 && bar_px > s(60.0) {
                for b in 1..self.sig.0.max(1) {
                    let bxx = self
                        .x_of(q + b as f64 * 4.0 / self.sig.1.max(1) as f64)
                        .round();
                    cv.fill_rect(
                        bxx,
                        ry + l.ruler_h - s(6.0),
                        1.0,
                        s(6.0),
                        rp.m_ruler_fg.fade(0.35),
                    );
                }
            }
        }
        cv.pop_clip();
    }

    // -----------------------------------------------------------------------
    // Les rangs, la grille
    // -----------------------------------------------------------------------

    fn draw_rows(&self, cv: &mut Canvas<'_>, _text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let scale = self.scale_of();
        let notes_mode = self.rows_mode == Rows::Notes && !self.rows.is_empty();
        let right = l.sx + l.sw;
        if !cv.push_clip(l.sx, l.gy, l.sw, l.gh) {
            cv.pop_clip();
            return;
        }
        let rp = &th.rp;
        let span = self
            .span()
            .map(|(a, b)| (self.x_of(a).round(), self.x_of(b).round()));
        for (i, &p) in self.rows.iter().enumerate() {
            let y = self.row_y(i);
            let rh = self.row_h(i);
            if y + rh < l.gy || y > l.gy + l.gh {
                continue;
            }
            let pc = p.rem_euclid(12);
            let in_scale = scale.is_none_or(|sc| sc.contains(p));
            // Les rangs de REAPER : touches blanches et noires (avec une
            // gamme, ses notes sont les blanches) ; hors de l'item, les fonds
            // exterieurs du theme.
            let k = if scale.is_some() {
                usize::from(!in_scale)
            } else {
                usize::from(BLACK.contains(&pc))
            };
            cv.fill_rect(l.sx, y, l.sw, rh, rp.m_bg[k]);
            if let Some((xa, xb)) = span {
                if xa > l.sx {
                    cv.fill_rect(l.sx, y, xa - l.sx, rh, rp.m_outer[k]);
                }
                if xb < right {
                    cv.fill_rect(xb, y, right - xb, rh, rp.m_outer[k]);
                }
            }
            // Un trait d'un pixel : l'octave (entre Si et Do), entre deux
            // touches blanches voisines (Mi, Fa), et un saut de hauteur en
            // mode Notes.
            let jump = notes_mode && i > 0 && self.rows[i - 1] - p > 1;
            if jump {
                cv.fill_rect(l.sx, y, l.sw, 1.0, th.seam);
            } else if !notes_mode && pc == 11 {
                cv.fill_rect(l.sx, y + rh, l.sw, 1.0, rp.m_oct.fade(0.55));
            } else if notes_mode || pc == 4 {
                cv.fill_rect(l.sx, y, l.sw, 1.0, rp.m_bg[1]);
            }
        }
        // Les lignes de temps : mesures, temps, pas de grille.
        let bq = self.bar_qn();
        let beat = 4.0 / self.sig.1.max(1) as f64;
        let g = if self.grid > 0.0 { self.grid } else { beat };
        let q0 = self.scroll;
        let q1 = self.scroll + self.view_qn();
        let gpx = g as f32 * l.ppq;
        let step = if gpx >= s(7.0) {
            g
        } else if beat as f32 * l.ppq >= s(7.0) {
            beat
        } else {
            bq
        };
        let mut k = (q0 / step).floor() as i64;
        loop {
            let q = k as f64 * step;
            if q > q1 {
                break;
            }
            let x = self.x_of(q).round();
            let on_bar = ((q / bq).round() * bq - q).abs() < 1e-6;
            let on_beat = ((q / beat).round() * beat - q).abs() < 1e-6;
            let c = if on_bar {
                rp.m_grid_bar
            } else if on_beat {
                rp.m_grid_beat
            } else {
                rp.m_grid_sub
            };
            cv.fill_rect(x, l.gy, 1.0, l.gh, c);
            k += 1;
        }
        cv.pop_clip();
    }

    fn draw_notes(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let Some(m) = self.midi() else { return };
        if !cv.push_clip(l.sx, l.gy, l.sw, l.gh) {
            cv.pop_clip();
            return;
        }
        let (q0, q1) = (self.scroll, self.scroll + self.view_qn());
        let fs = (l.rh * 0.62).clamp(8.0 * th.scale, 13.0 * th.scale);
        let f = Font::new(0, fs as u16, 700);
        let hover_id = match self.hover {
            Hit::Note { id, .. } => Some(id),
            _ => None,
        };
        // L'apercu de la palette : ce que l'operation survolee donnerait.
        let pv = self
            .palette
            .as_ref()
            .and_then(|p| p.preview.as_ref())
            .map(|x| &x.1);
        // Les choisies par-dessus.
        for pass in [false, true] {
            for n in m.roll.notes.iter().filter(|n| n.sel == pass) {
                if n.end() < q0 || n.qn0 > q1 {
                    continue;
                }
                let Some((x, y, w, h)) = self.note_rect(n) else {
                    continue;
                };
                if y + h < l.gy || y > l.gy + l.gh {
                    continue;
                }
                let mut c = self.note_color(n);
                if n.mute {
                    c = th.rp.m_note_mute;
                }
                let mut border = if n.sel {
                    th.rp.m_note_sel
                } else {
                    th.rp.m_note_border
                };
                // Pendant un apercu, les notes d'aujourd'hui s'effacent.
                if pv.is_some() {
                    c = c.with_alpha(55);
                    border = border.with_alpha(55);
                }
                cv.fill_rect(x, y, w, h, c);
                cv.stroke_rect(x, y, w, h, 1.0, border);
                // Le bend de la note (Guitar Tab : bends et glisses) : sa
                // hauteur au fil du temps, depuis le milieu de son rang.
                let curve = m.expr.curve(n);
                if curve.len() >= 2 && pv.is_none() {
                    let yc = y + h * 0.5;
                    let pt = |(q, st): (f64, f32)| (self.x_of(q), yc - st * l.rh);
                    let line = if n.sel {
                        th.rp.m_note_sel
                    } else {
                        th.rp.m_note_border
                    };
                    // Le bord sous tout le trait, puis le trait : sans
                    // pointilles aux jointures.
                    for (wd, col) in [(s(3.0), line), (s(1.5), c)] {
                        for w2 in curve.windows(2) {
                            let (xa, ya) = pt(w2[0]);
                            let (xb, yb) = pt(w2[1]);
                            cv.line(xa, ya, xb, yb, wd, col);
                        }
                    }
                }
                // Les poignees au survol : 30 % de chaque cote.
                if hover_id == Some(n.id) && w >= 5.0 * th.scale {
                    let hw = (w * 0.3).clamp(1.5 * th.scale, 6.0 * th.scale);
                    let hc = Color::WHITE.with_alpha(110);
                    cv.fill_rect(x, y, hw, h, hc);
                    cv.fill_rect(x + w - hw, y, hw, h, hc);
                }
                // Le nom dans la note, quand il tient.
                if w >= s(24.0) && h >= s(11.0) {
                    text.draw_in(
                        cv,
                        f,
                        x + s(4.0),
                        y,
                        w - s(6.0),
                        h,
                        &note_name(n.pitch),
                        th.rp.m_note_text,
                        Align::Left,
                    );
                }
            }
        }
        // L'apercu : les notes qu'on aurait, en pointilles, et leurs bends.
        if let Some(pm) = pv {
            for n in &pm.roll.notes {
                if n.end() < q0 || n.qn0 > q1 {
                    continue;
                }
                let Some((x, y, w, h)) = self.note_rect(n) else {
                    continue;
                };
                cv.fill_rect(x, y, w, h, self.note_color(n).with_alpha(150));
                cv.dash_rect(
                    x,
                    y,
                    w,
                    h,
                    3.0,
                    2.0,
                    if n.sel { th.accent } else { th.text_dim },
                );
                let curve = pm.expr.curve(n);
                if curve.len() >= 2 {
                    let yc = y + h * 0.5;
                    for w2 in curve.windows(2) {
                        let (xa, ya) = (self.x_of(w2[0].0), yc - w2[0].1 * l.rh);
                        let (xb, yb) = (self.x_of(w2[1].0), yc - w2[1].1 * l.rh);
                        cv.line(xa, ya, xb, yb, s(1.6), th.accent);
                    }
                }
            }
        } else {
            self.draw_bend_handles(cv, text, th);
        }
        // Le lasso.
        if let Some((x0, y0, x1, y1)) = self.marquee() {
            let (xa, ya) = (x0.min(x1), y0.min(y1));
            let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
            cv.fill_rect(xa, ya, w, h, th.accent.with_alpha(30));
            cv.stroke_rect(xa, ya, w, h, 1.0, th.accent);
        }
        // Les notes et accords peints.
        if let Some(cells) = self.chord_cells() {
            let g = if self.grid > 0.0 { self.grid } else { 0.25 };
            for &(q, p) in cells {
                if let Some(i) = self.row_of(p) {
                    let (xa, xb) = (self.x_of(q), self.x_of(q + g));
                    let y = self.row_y(i);
                    cv.fill_rect(
                        xa,
                        y,
                        (xb - xa - 1.0).max(1.0),
                        l.rh,
                        th.accent.with_alpha(110),
                    );
                }
            }
        }
        // La ligne peinte.
        if let Some((qa, pa, qb, pb)) = self.paint() {
            if let (Some(ia), Some(ib)) = (self.row_of(pa), self.row_of(pb)) {
                let ya = self.row_y(ia) + l.rh * 0.5;
                let yb = self.row_y(ib) + l.rh * 0.5;
                cv.line(self.x_of(qa), ya, self.x_of(qb), yb, s(2.0), th.accent);
            }
        }
        cv.pop_clip();
    }

    fn draw_keys(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let kw = l.sx - 1.0;
        cv.fill_rect(kw, l.gy, 1.0, l.gh, th.seam);
        if !cv.push_clip(0.0, l.gy, kw, l.gh) {
            cv.pop_clip();
            return;
        }
        let notes_mode = self.rows_mode == Rows::Notes;
        let scale = self.scale_of();
        let sel_pitches: Vec<i32> = self
            .midi()
            .map(|m| {
                m.roll
                    .notes
                    .iter()
                    .filter(|n| n.sel)
                    .map(|n| n.pitch)
                    .collect()
            })
            .unwrap_or_default();
        let fs = (l.rh * 0.6).clamp(8.0 * th.scale, 12.0 * th.scale);
        let f = Font::new(0, fs as u16, 600);
        for (i, &p) in self.rows.iter().enumerate() {
            let y = self.row_y(i);
            let rh = self.row_h(i);
            if y + rh < l.gy || y > l.gy + l.gh {
                continue;
            }
            let pc = p.rem_euclid(12);
            let black = BLACK.contains(&pc);
            let hov = self.hover == Hit::Key(p);
            let rp = &th.rp;
            let (bg, ink) = if black {
                (rp.m_key[1], rp.m_key[0])
            } else {
                (rp.m_key[0], rp.m_key[1])
            };
            let bg = if notes_mode && black {
                rp.m_key[1].mix(rp.m_key[0], 0.25)
            } else {
                bg
            };
            let bg = if hov || sel_pitches.contains(&p) {
                rp.m_key_sel
            } else {
                bg
            };
            cv.fill_rect(0.0, y, kw, rh, bg);
            cv.fill_rect(0.0, y, kw, 1.0, Color::BLACK.with_alpha(60));
            // Hors gamme : une marque a gauche.
            if scale.is_some_and(|sc| !sc.contains(p)) {
                cv.fill_rect(0.0, y, s(3.0), l.rh, th.text_faint.fade(0.7));
            }
            if sel_pitches.contains(&p) {
                cv.fill_rect(kw - s(3.0), y, s(3.0), l.rh, th.accent);
            }
            let named = notes_mode || pc == 0 || (l.rh >= s(16.0)) || sel_pitches.contains(&p);
            if named && l.rh >= s(8.0) {
                text.draw_in(
                    cv,
                    f,
                    0.0,
                    y,
                    kw - s(7.0),
                    l.rh,
                    &note_name(p),
                    ink,
                    Align::Right,
                );
            }
        }
        cv.pop_clip();
        let _ = icons;
    }

    /// Les poignees du bend de la note retenue et des notes choisies : le
    /// pied (quand il part) et le sommet (de combien), sa valeur au survol.
    fn draw_bend_handles(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let s = |v: f32| th.s(v);
        let Some(m) = self.midi() else { return };
        let dragged = match &self.drag {
            Some(crate::Drag::Bend { id, peak, .. }) => Some((*id, *peak)),
            _ => None,
        };
        let mut seen: Vec<u64> = Vec::new();
        let cands = self
            .bend_note
            .into_iter()
            .chain(m.roll.notes.iter().filter(|n| n.sel).take(64).map(|n| n.id));
        for id in cands {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let Some(n) = m.roll.get(id) else { continue };
            let Some((foot, peak)) = self.bend_points(n) else {
                continue;
            };
            for (pt, top) in [(foot, false), (Some(peak), true)] {
                let Some((hx, hy)) = pt else { continue };
                let hit = if top {
                    Hit::BendPeak(id)
                } else {
                    Hit::BendFoot(id)
                };
                let hot = self.hover == hit || dragged == Some((id, top));
                let r = s(4.5);
                cv.fill_circle(hx, hy, r + s(1.5), th.bg.with_alpha(200));
                cv.fill_circle(hx, hy, r, if hot { th.accent } else { th.bg });
                let mut p = Path::new();
                p.circle(hx, hy, r);
                cv.stroke_path(&p, s(1.6), th.accent);
                if top && hot {
                    let v = crate::palette::bend_handles(&m.expr.curve(n)).map_or(0.0, |x| x.1.1);
                    let lab = crate::bend_label(v);
                    let f = Font::new(0, s(10.0) as u16, 700);
                    let tw = text.measure(f, &lab).ceil() + s(8.0);
                    let (bx, by) = (hx + s(8.0), hy - s(18.0));
                    cv.fill_round_rect(bx, by, tw, s(14.0), s(3.0), th.panel);
                    text.draw_in(cv, f, bx, by, tw, s(14.0), &lab, th.text, Align::Center);
                }
            }
        }
    }

    fn draw_vel(&self, cv: &mut Canvas<'_>, icons: &mut IconCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let Some(m) = self.midi() else { return };
        cv.fill_rect(0.0, l.vel_y - 3.0, l.w, 3.0, th.seam);
        cv.fill_rect(0.0, l.vel_y, l.sx - 1.0, l.vel_h, th.panel);
        let isz = s(14.0) as u16;
        icons.draw(
            cv,
            Icon::Bars,
            ((l.sx - isz as f32) * 0.5).round(),
            (l.vel_y + (l.vel_h - isz as f32) * 0.5).round(),
            isz,
            th.text_faint,
            false,
        );
        cv.fill_rect(l.sx, l.vel_y, l.sw, l.vel_h, th.bg.mix(Color::BLACK, 0.12));
        if !cv.push_clip(l.sx, l.vel_y, l.sw, l.vel_h) {
            cv.pop_clip();
            return;
        }
        let bottom = l.vel_y + l.vel_h - s(4.0);
        let span = l.vel_h - s(10.0);
        // Les reperes 64 et 127.
        for v in [64, 127] {
            let y = bottom - v as f32 / 127.0 * span;
            cv.fill_rect(l.sx, y.round(), l.sw, 1.0, th.seam.mix(th.bg, 0.3));
        }
        let (q0, q1) = (self.scroll, self.scroll + self.view_qn());
        for pass in [false, true] {
            for n in m.roll.notes.iter().filter(|n| n.sel == pass) {
                if n.qn0 < q0 - 1.0 || n.qn0 > q1 {
                    continue;
                }
                let x = self.x_of(n.qn0);
                let h = n.vel as f32 / 127.0 * span;
                let w = if n.sel { s(4.0) } else { s(2.5) };
                let c = self.note_color(n);
                cv.fill_rect(x, bottom - h, w, h, if n.sel { c } else { c.fade(0.75) });
                cv.fill_rect(x - s(1.5), bottom - h - s(1.0), w + s(3.0), s(2.0), c);
            }
        }
        // L'apercu de la palette : les velocites qu'on aurait.
        if let Some((_, pm)) = self.palette.as_ref().and_then(|p| p.preview.as_ref()) {
            for n in &pm.roll.notes {
                if n.qn0 < q0 - 1.0 || n.qn0 > q1 {
                    continue;
                }
                let x = self.x_of(n.qn0);
                let h = n.vel as f32 / 127.0 * span;
                cv.dash_rect(
                    x - s(1.5),
                    bottom - h - s(1.0),
                    s(7.0),
                    h + s(1.0),
                    2.0,
                    2.0,
                    th.accent,
                );
            }
        }
        cv.pop_clip();
    }

    // -----------------------------------------------------------------------
    // Le clip audio
    // -----------------------------------------------------------------------

    fn draw_audio(&self, cv: &mut Canvas<'_>, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let Some(a) = self.audio() else { return };
        cv.fill_rect(0.0, l.gy, l.sx - 1.0, l.gh, th.panel);
        if !cv.push_clip(l.sx, l.gy, l.sw, l.gh) {
            cv.pop_clip();
            return;
        }
        let (xa, xb) = (self.x_of(a.qn0), self.x_of(a.qn1));
        cv.fill_rect(l.sx, l.gy, l.sw, l.gh, th.bg.mix(Color::BLACK, 0.2));
        cv.fill_rect(xa, l.gy, xb - xa, l.gh, th.bg.mix(th.text, 0.04));
        // Les lignes de mesure.
        let bq = self.bar_qn();
        let mut k = (self.scroll / bq).floor() as i64;
        loop {
            let q = k as f64 * bq;
            if q > self.scroll + self.view_qn() {
                break;
            }
            cv.fill_rect(
                self.x_of(q).round(),
                l.gy,
                1.0,
                l.gh,
                th.seam.mix(th.bg, 0.2),
            );
            k += 1;
        }
        // L'onde, a la hauteur du gain.
        let mid = l.gy + l.gh * 0.5;
        let half = l.gh * 0.46;
        let g = if a.gain_db <= -60.0 {
            0.0
        } else {
            10f32.powf(a.gain_db / 20.0)
        };
        let wave = th.text_dim.mix(th.accent, 0.35);
        if a.pk.2 > 0 && !a.peaks.is_empty() {
            let x0 = self.x_of(a.pk.0);
            let x1 = self.x_of(a.pk.1);
            let n = a.peaks.len();
            let cw = (x1 - x0) / n as f32;
            for (k, &(lo, hi)) in a.peaks.iter().enumerate() {
                let x = x0 + k as f32 * cw;
                // Le fondu, sur l'onde aussi.
                let q = a.pk.0 + (a.pk.1 - a.pk.0) * (k as f64 + 0.5) / n as f64;
                let mut fg = 1.0f32;
                if a.fade_in > 0.0 && q < a.qn0 + a.fade_in {
                    fg *= ((q - a.qn0) / a.fade_in).clamp(0.0, 1.0) as f32;
                }
                if a.fade_out > 0.0 && q > a.qn1 - a.fade_out {
                    fg *= ((a.qn1 - q) / a.fade_out).clamp(0.0, 1.0) as f32;
                }
                let top = mid - (hi * g * fg).clamp(-1.0, 1.0) * half;
                let bot = mid - (lo * g * fg).clamp(-1.0, 1.0) * half;
                cv.fill_rect(
                    x,
                    top.min(bot),
                    cw.max(1.0),
                    (bot - top).abs().max(1.0),
                    wave,
                );
            }
        }
        cv.fill_rect(xa, mid, xb - xa, 1.0, th.seam);
        // Les fondus : une courbe et leur poignee.
        let fi = self.x_of(a.qn0 + a.fade_in);
        let fo = self.x_of(a.qn1 - a.fade_out);
        let fc = th.text.with_alpha(160);
        if a.fade_in > 0.0 {
            cv.line(xa, l.gy + l.gh, fi, l.gy, s(1.5), fc);
        }
        if a.fade_out > 0.0 {
            cv.line(fo, l.gy, xb, l.gy + l.gh, s(1.5), fc);
        }
        for (x, hit) in [(fi, Hit::FadeIn), (fo, Hit::FadeOut)] {
            let hot = self.hover == hit || self.audio_drag_hit() == Some(hit);
            let sz = s(if hot { 10.0 } else { 8.0 });
            cv.fill_rect(
                x - sz * 0.5,
                l.gy + s(3.0),
                sz,
                sz,
                if hot { th.accent } else { th.text_dim },
            );
        }
        // La ligne du gain.
        let gy = self.gain_y(a.gain_db);
        let hot = self.hover == Hit::Gain || self.audio_drag_hit() == Some(Hit::Gain);
        cv.fill_rect(
            xa,
            gy.round(),
            xb - xa,
            if hot { s(2.0) } else { 1.0 },
            if hot { th.accent } else { th.accent.fade(0.6) },
        );
        // Les bords.
        for (x, hit) in [(xa, Hit::EdgeL), (xb, Hit::EdgeR)] {
            let hot = self.hover == hit || self.audio_drag_hit() == Some(hit);
            cv.fill_rect(
                x - if hot { s(1.5) } else { 0.5 },
                l.gy,
                if hot { s(3.0) } else { 1.0 },
                l.gh,
                if hot { th.accent } else { th.text_dim },
            );
        }
        cv.pop_clip();
    }

    // -----------------------------------------------------------------------
    // La tete de lecture, les barres de defilement
    // -----------------------------------------------------------------------

    fn draw_scrollbars(&self, cv: &mut Canvas<'_>, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        cv.fill_rect(0.0, l.hs_y, l.w, l.hs_h, th.bg);
        if matches!(self.clip, Clip::None) {
            return;
        }
        let (tx, tw) = self.hthumb();
        let hh = s(6.0);
        let ty = l.hs_y + ((l.hs_h - hh) * 0.5).round();
        cv.fill_round_rect(l.sx, ty, l.sw, hh, s(3.0), th.field);
        let hot = self.hover == Hit::HScroll || self.hscroll_hot();
        cv.fill_round_rect(
            tx,
            ty,
            tw,
            hh,
            s(3.0),
            if hot { th.text_dim } else { th.text_faint },
        );
        // La verticale, quand les rangs debordent.
        if matches!(self.clip, Clip::Midi(_)) && l.content_h > l.gh + 0.5 {
            let x = l.sx + l.sw + s(2.0);
            let w = s(6.0);
            let k = l.gh / l.content_h;
            let h = (l.gh * k).max(s(20.0)).min(l.gh);
            let room = l.gh - h;
            let maxv = (l.content_h - l.gh).max(1.0);
            let y = l.gy + room * (self.vscroll / maxv);
            cv.fill_round_rect(x, l.gy, w, l.gh, s(3.0), th.field);
            cv.fill_round_rect(
                x,
                y,
                w,
                h,
                s(3.0),
                if self.hover == Hit::VScroll {
                    th.text_dim
                } else {
                    th.text_faint
                },
            );
        }
        let _ = Part::Body;
    }
}
