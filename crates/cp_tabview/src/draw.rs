//! Le dessin de l'arrangeur de guitares : la barre, la regle, les pistes
//! (leur en-tete a gauche, leurs couches qui defilent a droite), l'inspecteur
//! de l'item choisi et la ligne d'etat.
//!
//! La partition et la tablature d'un item se dessinent sur l'axe du temps
//! commun, dans les couches de sa piste. Les signes musicaux qu'on ne trace
//! pas soi-meme (cle de sol, silences, diese) viennent de Segoe UI Symbol
//! (famille 3 du texte).

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Path};
use cp_tab::doc::Tech;
use cp_tab::engrave::Engraved;
use cp_ui::{Icon, IconCache, Theme};

use crate::{Arr, HBox, Hit, Item, Lane, Lay, Listen, Sound, State, TUNINGS};

const CLEF: char = '\u{1D11E}';
const SHARP: char = '\u{266F}';

fn rest_char(ty: &str) -> char {
    match ty {
        "whole" => '\u{1D13B}',
        "half" => '\u{1D13C}',
        "quarter" => '\u{1D13D}',
        "eighth" => '\u{1D13E}',
        "16th" => '\u{1D13F}',
        _ => '\u{1D140}',
    }
}

fn beams_of(ty: &str) -> usize {
    match ty {
        "eighth" => 1,
        "16th" => 2,
        "32nd" => 3,
        _ => 0,
    }
}

/// La place d'une hauteur sur la portee (0 = la ligne du bas, Mi ; un pas =
/// une ligne ou un interligne), et son diese. La guitare s'ecrit une octave
/// au-dessus du son.
pub fn staff_pos(pitch: i32) -> (i32, bool) {
    const STEP: [i32; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
    const SHARP: [bool; 12] = [
        false, true, false, true, false, false, true, false, true, false, true, false,
    ];
    let w = pitch + 12;
    let pc = w.rem_euclid(12) as usize;
    let oct = w.div_euclid(12) - 1;
    (oct * 7 + STEP[pc] - (4 * 7 + 2), SHARP[pc])
}

/// Une tete de note : une ellipse inclinee.
fn head(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
    let mut p = Path::new();
    let a: f32 = -0.35;
    let (sa, ca) = a.sin_cos();
    for k in 0..=20 {
        let t = k as f32 / 20.0 * std::f32::consts::TAU;
        let (x, y) = (rx * t.cos(), ry * t.sin());
        let (px, py) = (cx + x * ca - y * sa, cy + x * sa + y * ca);
        if k == 0 {
            p.move_to(px, py);
        } else {
            p.line_to(px, py);
        }
    }
    p.close();
    p
}

/// Un trait pointille.
fn dashed(
    cv: &mut Canvas<'_>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    on: f32,
    off: f32,
    w: f32,
    c: Color,
) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.0 {
        return;
    }
    let mut t = 0.0;
    while t < len {
        let t1 = (t + on).min(len);
        cv.line(
            x0 + dx * t / len,
            y0 + dy * t / len,
            x0 + dx * t1 / len,
            y0 + dy * t1 / len,
            w,
            c,
        );
        t += on + off;
    }
}

fn dashed_rect(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, c: Color) {
    dashed(cv, x, y, x + w, y, 2.0, 1.6, 1.0, c);
    dashed(cv, x, y + h, x + w, y + h, 2.0, 1.6, 1.0, c);
    dashed(cv, x, y, x, y + h, 2.0, 1.6, 1.0, c);
    dashed(cv, x + w, y, x + w, y + h, 2.0, 1.6, 1.0, c);
}

/// Un arc (liaison) d'un point a l'autre, bombe de `bulge` (negatif : vers le
/// haut).
fn arc(cv: &mut Canvas<'_>, x0: f32, y0: f32, x1: f32, y1: f32, bulge: f32, w: f32, c: Color) {
    let mut p = Path::new();
    p.move_to(x0, y0)
        .quad_to((x0 + x1) * 0.5, (y0 + y1) * 0.5 + bulge * 2.0, x1, y1);
    cv.stroke_path(&p, w, c);
}

/// Une cible, rognee a la zone visible (rien si elle en sort).
fn push(bx: &mut Vec<HBox>, clip: (f32, f32, f32, f32), x: f32, y: f32, w: f32, h: f32, hit: Hit) {
    let x0 = x.max(clip.0);
    let y0 = y.max(clip.1);
    let x1 = (x + w).min(clip.0 + clip.2);
    let y1 = (y + h).min(clip.1 + clip.3);
    if x1 > x0 && y1 > y0 {
        bx.push((x0, y0, x1 - x0, y1 - y0, hit));
    }
}

/// La hauteur d'une couche (a 96 DPI).
pub fn lane_h(l: Lane) -> f32 {
    match l {
        Lane::Audio => 62.0,
        Lane::Midi => 46.0,
        Lane::Score => 118.0,
        Lane::Tab => 88.0,
    }
}

/// La hauteur que demande l'en-tete d'une piste (a 96 DPI).
const HEAD_MIN_H: f32 = 168.0;
const TRACK_SEP: f32 = 3.0;
const ADD_ROW_H: f32 = 40.0;

/// La hauteur d'une piste (a l'echelle).
pub fn track_h(tr: &crate::Track, s: f32) -> f32 {
    let lanes: f32 = Lane::ALL
        .iter()
        .filter(|&&l| tr.head.lanes.on(l))
        .map(|&l| lane_h(l) + 1.0)
        .sum();
    (lanes.max(HEAD_MIN_H) * s).round()
}

/// Le debut et la largeur des faders d'un en-tete.
pub fn fader_span(a: &Arr, _t: usize) -> (f32, f32) {
    let s = a.scale;
    let fx = (12.0 + 42.0 + 7.0) * s;
    (fx, (a.lay.head_w - fx - 52.0 * s).max(10.0))
}

/// La couleur d'une corde (MIDI).
fn string_color(k: usize) -> Color {
    const C: [u32; 6] = [0xa8c97f, 0xde9a86, 0xc99ad6, 0x8fb0de, 0x7fc6b4, 0xd9b45a];
    cp_gfx::hex(C[k.min(5)])
}

/// La geometrie de l'axe du temps.
#[derive(Clone, Copy)]
struct Geo {
    sx: f32,
    scroll: f64,
    ppq: f32,
    s: f32,
    /// La zone visible (cibles et dessins).
    clip: (f32, f32, f32, f32),
}

impl Geo {
    fn x(&self, q: f64) -> f32 {
        self.sx + ((q - self.scroll) as f32) * self.ppq
    }

    fn visible(&self, x: f32) -> bool {
        x > self.clip.0 - 40.0 * self.s && x < self.clip.0 + self.clip.2 + 40.0 * self.s
    }
}

impl Arr {
    fn layout(&mut self, w: f32, h: f32, scale: f32) {
        let s = |v: f32| (v * scale).round();
        let bar_h = s(36.0);
        let status_h = s(24.0);
        let panel_w = if w > s(980.0) { s(262.0) } else { 0.0 };
        let panel_x = w - panel_w;
        let top = bar_h + 2.0;
        let bottom = h - status_h - 2.0;
        let head_w = s(236.0);
        let label_x = head_w + 2.0;
        let sx = label_x + s(58.0);
        let right = if panel_w > 0.0 { panel_x - 2.0 } else { w };
        let content_h = self
            .tracks
            .iter()
            .map(|t| track_h(t, scale) + s(TRACK_SEP))
            .sum::<f32>()
            + s(ADD_ROW_H);
        self.lay = Lay {
            w,
            h,
            bar_h,
            status_h,
            panel_w,
            panel_x,
            top,
            bottom,
            head_w,
            ruler_h: s(26.0),
            label_x,
            sx,
            sw: (right - sx - s(10.0)).max(10.0),
            ppq: self.zoom * scale,
            content_h,
        };
        self.vscroll = self.vscroll.clamp(0.0, self.max_vscroll());
    }

    /// Dessine toute la fenetre (`w x h` px) et retient ou sont les cibles.
    pub fn draw(
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
        let l = self.lay;
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_bar(cv, text, icons, th, &mut bx);
        cv.fill_rect(0.0, l.bar_h, w, 2.0, th.seam);
        self.draw_tracks(cv, text, th, &mut bx);
        if l.panel_w > 0.0 {
            cv.fill_rect(l.panel_x - 2.0, l.top, 2.0, l.bottom - l.top, th.seam);
            self.draw_panel(cv, text, th, &mut bx);
        }
        self.draw_status(cv, text, th);
        self.boxes = bx;
    }

    fn button(
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
        let f = Font::new(0, th.s(12.0) as u16, if on { 700 } else { 400 });
        let bw = text.measure(f, label).ceil() + th.s(18.0);
        let bg = if self.hover == hit {
            th.hover.mix(th.text, 0.06)
        } else {
            th.field
        };
        cv.fill_round_rect(x, y, bw, h, th.s(3.0), bg);
        text.draw_in(
            cv,
            f,
            x,
            y,
            bw,
            h,
            label,
            if on { th.accent } else { th.text },
            Align::Center,
        );
        bx.push((x, y, bw, h, hit));
        bw
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
        let mut x = s(10.0);
        let isz = s(18.0) as u16;
        icons.draw(
            cv,
            Icon::Guitar,
            x,
            ((l.bar_h - isz as f32) * 0.5).round(),
            isz,
            th.accent,
            false,
        );
        x += isz as f32 + s(6.0);
        let fb = Font::new(0, s(13.0) as u16, 700);
        let f = Font::new(0, s(12.0) as u16, 400);
        x += text.draw(
            cv,
            fb,
            x,
            (l.bar_h * 0.5 + s(4.5)).round(),
            "Guitar Tab",
            th.text,
        ) + s(16.0);
        let by = ((l.bar_h - s(24.0)) * 0.5).floor();
        let bh = s(24.0);
        x += text.draw(
            cv,
            f,
            x,
            (l.bar_h * 0.5 + s(4.0)).round(),
            "Listen",
            th.text_dim,
        ) + s(6.0);
        for m in [Listen::Original, Listen::Tab, Listen::Both] {
            let label = match m {
                Listen::Original => "Original",
                Listen::Tab => "Tab",
                Listen::Both => "Both",
            };
            x += self.button(
                cv,
                text,
                th,
                bx,
                x,
                by,
                bh,
                label,
                self.listen == m,
                Hit::Listen(m),
            ) + s(1.0);
        }
        x += s(4.0);
        x += self.button(
            cv,
            text,
            th,
            bx,
            x,
            by,
            bh,
            "L | R",
            self.split && self.listen == Listen::Both,
            Hit::Split,
        ) + s(14.0);
        x += self.button(cv, text, th, bx, x, by, bh, "\u{2212}", false, Hit::ZoomOut) + s(1.0);
        x += self.button(cv, text, th, bx, x, by, bh, "+", false, Hit::ZoomIn) + s(8.0);
        let _ = self.button(
            cv,
            text,
            th,
            bx,
            x,
            by,
            bh,
            "Follow playback",
            self.follow,
            Hit::Follow,
        );
        // A droite : la piste neuve, et l'enregistrement en cours.
        let add = "+ Guitar track";
        let fa = Font::new(0, s(12.0) as u16, 700);
        let aw = text.measure(fa, add).ceil() + s(20.0);
        let ax = l.w - s(10.0) - aw;
        let hov = self.hover == Hit::AddTrack;
        cv.fill_round_rect(
            ax,
            by,
            aw,
            bh,
            s(3.0),
            if hov {
                th.accent.mix(th.text, 0.2)
            } else {
                th.accent
            },
        );
        text.draw_in(cv, fa, ax, by, aw, bh, add, th.bg, Align::Center);
        bx.push((ax, by, aw, bh, Hit::AddTrack));
        if let Some(r) = self.rec.first() {
            let name = self
                .tracks
                .get(r.track)
                .map(|t| t.head.name.clone())
                .unwrap_or_default();
            let lab = format!("\u{25cf} REC  {name}");
            let rw = text.measure(fa, &lab).ceil() + s(20.0);
            let rx = ax - s(10.0) - rw;
            cv.fill_round_rect(rx, by, rw, bh, s(3.0), th.field);
            text.draw_in(cv, fa, rx, by, rw, bh, &lab, th.error, Align::Center);
        }
    }

    // -----------------------------------------------------------------------
    // Les pistes
    // -----------------------------------------------------------------------

    fn draw_tracks(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let ry = l.top;
        let content_r = l.sx + l.sw;
        // --- la regle (et le coin des en-tetes)
        cv.fill_rect(0.0, ry, l.head_w, l.ruler_h, th.panel);
        let fh = Font::new(0, s(10.0) as u16, 700);
        let fs = Font::new(0, s(10.5) as u16, 400);
        text.draw(cv, fh, s(12.0), ry + s(16.0), "TRACKS", th.text_faint);
        let n = self.tracks.len();
        let cnt = match n {
            0 => "no guitar yet".to_string(),
            1 => "1 guitar".to_string(),
            _ => format!("{n} guitars"),
        };
        text.draw_in(
            cv,
            fs,
            0.0,
            ry,
            l.head_w - s(10.0),
            l.ruler_h,
            &cnt,
            th.text_faint,
            Align::Right,
        );
        cv.fill_rect(l.head_w, ry, 2.0, l.bottom - ry, th.seam);
        cv.fill_rect(l.label_x, ry, l.w, l.ruler_h, th.panel);
        let geo = Geo {
            sx: l.sx,
            scroll: self.scroll,
            ppq: l.ppq,
            s: th.scale,
            clip: (l.sx, ry, l.sw, l.bottom - ry),
        };
        let bq = self.bar_qn();
        let q0 = self.scroll;
        let q1 = self.scroll + self.view_qn();
        let first = (q0 / bq).floor().max(0.0) as i64;
        let last = (q1 / bq).ceil() as i64;
        let fb = Font::new(0, s(11.0) as u16, 700);
        let ft = Font::new(1, s(9.5) as u16, 400);
        if cv.push_clip(l.sx, ry, l.sw, l.ruler_h) {
            if let Some((a, b)) = self.range {
                let (xa, xb) = (geo.x(a), geo.x(b));
                cv.fill_rect(xa, ry, xb - xa, l.ruler_h, th.accent.with_alpha(40));
                cv.fill_rect(xa, ry + l.ruler_h - s(3.0), xb - xa, s(3.0), th.accent);
            }
            for m in first..=last {
                let q = m as f64 * bq;
                let x = geo.x(q).round();
                cv.fill_rect(x, ry + s(3.0), 1.0, l.ruler_h - s(3.0), th.text_faint);
                text.draw(
                    cv,
                    fb,
                    x + s(4.0),
                    ry + s(14.0),
                    &(m + 1).to_string(),
                    th.text,
                );
                let t = q * 60.0 / self.bpm.max(1.0);
                text.draw(
                    cv,
                    ft,
                    x + s(24.0),
                    ry + s(14.0),
                    &format!("{}:{:04.1}", (t / 60.0) as i64, t % 60.0),
                    th.text_faint,
                );
                for b in 1..self.sig.0.max(1) {
                    let bxx = geo.x(q + b as f64 * 4.0 / self.sig.1.max(1) as f64).round();
                    cv.fill_rect(
                        bxx,
                        ry + l.ruler_h - s(7.0),
                        1.0,
                        s(7.0),
                        th.text_faint.fade(0.6),
                    );
                }
            }
        }
        cv.pop_clip();
        push(bx, geo.clip, l.sx, ry, l.sw, l.ruler_h, Hit::Ruler);
        cv.fill_rect(0.0, ry + l.ruler_h, l.w - l.panel_w, 1.0, th.seam);

        // --- les pistes
        let top = ry + l.ruler_h + 1.0;
        let vis = (0.0, top, l.panel_x, (l.bottom - top).max(0.0));
        let mut y = top - self.vscroll;
        if cv.push_clip(
            0.0,
            top,
            if l.panel_w > 0.0 {
                l.panel_x - 2.0
            } else {
                l.w
            },
            vis.3,
        ) {
            for t in 0..self.tracks.len() {
                let h = track_h(&self.tracks[t], th.scale);
                if y + h >= top && y <= l.bottom {
                    self.draw_header(cv, text, th, bx, t, y, h, vis);
                    self.draw_lanes(cv, text, th, bx, t, y, h, geo, vis);
                }
                y += h;
                cv.fill_rect(
                    0.0,
                    y,
                    l.panel_x,
                    s(TRACK_SEP),
                    th.seam.mix(Color::BLACK, 0.4),
                );
                y += s(TRACK_SEP);
            }
            // La ligne d'ajout.
            let ah = s(ADD_ROW_H);
            let hov = self.hover == Hit::AddTrack;
            cv.fill_rect(0.0, y, l.panel_x, ah, if hov { th.hover } else { th.bg });
            dashed(cv, 0.0, y, content_r, y, s(4.0), s(3.0), 1.0, th.text_faint);
            let f = Font::new(0, s(12.0) as u16, 400);
            let fa = Font::new(0, s(13.0) as u16, 700);
            let w1 = text.draw(cv, fa, s(14.0), y + ah * 0.5 + s(4.5), "+", th.accent);
            let w2 = text.draw(
                cv,
                f,
                s(22.0) + w1,
                y + ah * 0.5 + s(4.0),
                "New guitar track",
                th.text,
            );
            text.draw(
                cv,
                f,
                s(30.0) + w1 + w2,
                y + ah * 0.5 + s(4.0),
                "· or right-click a track in REAPER \u{2192} \u{201c}Guitar Tab: make guitar track\u{201d}",
                th.text_faint,
            );
            push(bx, vis, 0.0, y, content_r, ah, Hit::AddTrack);
            // La tete de lecture, a travers toutes les pistes.
            if let Some(p) = self.playhead {
                let x = geo.x(p).round();
                if x >= l.sx && x <= content_r {
                    cv.fill_rect(x, top, s(1.5), (y - top).max(0.0), th.playhead);
                }
            }
            // Le passage choisi, a travers toutes les pistes.
            if let Some((a, b)) = self.range {
                let (xa, xb) = (geo.x(a).max(l.sx), geo.x(b).min(content_r));
                if xb > xa {
                    cv.fill_rect(
                        xa,
                        top,
                        xb - xa,
                        (y - top).max(0.0),
                        th.accent.with_alpha(16),
                    );
                }
            }
        }
        cv.pop_clip();
        // La tete de lecture dans la regle.
        if let Some(p) = self.playhead {
            let x = geo.x(p).round();
            if x >= l.sx && x <= content_r {
                cv.fill_rect(x, ry, s(1.5), l.ruler_h, th.playhead);
                let mut tri = Path::new();
                tri.move_to(x - s(5.0), ry + l.ruler_h - s(8.0))
                    .line_to(x + s(6.5), ry + l.ruler_h - s(8.0))
                    .line_to(x + s(0.75), ry + l.ruler_h - s(1.0))
                    .close();
                cv.fill_path(&tri, th.playhead);
            }
        }
        // La barre de defilement vertical.
        let view = (l.bottom - top).max(1.0);
        if l.content_h > view {
            let sbx = content_r + s(2.0);
            let sbw = s(6.0);
            let k = view / l.content_h;
            let hh = (view * k).max(s(24.0));
            let yy = top + (self.vscroll / l.content_h) * view;
            cv.fill_round_rect(sbx, top, sbw, view, s(3.0), th.field);
            cv.fill_round_rect(
                sbx,
                yy,
                sbw,
                hh.min(view),
                s(3.0),
                if self.hover == Hit::VScroll {
                    th.text_dim
                } else {
                    th.text_faint
                },
            );
            bx.push((sbx - s(2.0), top, sbw + s(4.0), view, Hit::VScroll));
        }
        if self.tracks.is_empty() {
            let f = Font::new(0, s(13.0) as u16, 400);
            let msg = "Add a guitar track to record and transcribe. Each recording gets its MIDI and its tab, on the same track.";
            text.draw_in(
                cv,
                f,
                l.sx,
                top + s(70.0),
                l.sw,
                s(40.0),
                msg,
                th.text_dim,
                Align::Center,
            );
        }
    }

    fn draw_header(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        t: usize,
        y: f32,
        h: f32,
        vis: (f32, f32, f32, f32),
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let hd = &self.tracks[t].head;
        let selected = self.sel.is_some_and(|x| x.t == t);
        let bg = if selected {
            th.panel.mix(th.accent, 0.07)
        } else {
            th.panel
        };
        cv.fill_rect(0.0, y, l.head_w, h, bg);
        push(bx, vis, 0.0, y, l.head_w, h, Hit::Header(t));
        if hd.armed {
            cv.fill_rect(0.0, y, s(3.0), h, th.error);
        }
        let x0 = s(12.0);
        let xr = l.head_w - s(10.0);
        // Le nom, l'accordage.
        let fnum = Font::new(1, s(10.5) as u16, 400);
        let fname = Font::new(0, s(13.0) as u16, 700);
        let fsm = Font::new(0, s(10.5) as u16, 400);
        let nw = text.draw(
            cv,
            fnum,
            x0,
            y + s(22.0),
            &hd.num.to_string(),
            th.text_faint,
        );
        let tun = format!("{} \u{25be}", TUNINGS[hd.tuning.min(TUNINGS.len() - 1)].0);
        let tw = text.measure(fsm, &tun).ceil() + s(10.0);
        let tx = xr - tw;
        let hov_t = self.hover == Hit::Tuning(t);
        if hov_t {
            cv.fill_round_rect(tx, y + s(8.0), tw, s(19.0), s(3.0), th.field);
        }
        text.draw_in(
            cv,
            fsm,
            tx,
            y + s(8.0),
            tw,
            s(19.0),
            &tun,
            th.text_faint,
            Align::Center,
        );
        push(bx, vis, tx, y + s(8.0), tw, s(19.0), Hit::Tuning(t));
        let namex = x0 + nw + s(7.0);
        text.draw_in(
            cv,
            fname,
            namex,
            y + s(8.0),
            (tx - namex - s(6.0)).max(0.0),
            s(19.0),
            &hd.name,
            th.text,
            Align::Left,
        );
        // Armer, entree, ecoute, mute, solo.
        let ry = y + s(34.0);
        let rh = s(20.0);
        let ar = s(8.0);
        let acx = x0 + ar;
        let acy = ry + rh * 0.5;
        if hd.armed {
            cv.fill_circle(acx, acy, ar + s(2.0), th.error.with_alpha(70));
            cv.fill_circle(acx, acy, ar, th.error);
        } else {
            cv.fill_circle(acx, acy, ar, th.text_faint);
            cv.fill_circle(acx, acy, ar - s(1.5), th.field);
            if self.hover == Hit::Arm(t) {
                cv.fill_circle(acx, acy, ar - s(4.0), th.error.fade(0.6));
            }
        }
        push(
            bx,
            vis,
            x0 - s(2.0),
            ry - s(2.0),
            ar * 2.0 + s(4.0),
            rh + s(4.0),
            Hit::Arm(t),
        );
        let f11 = Font::new(0, s(11.0) as u16, 400);
        let ix = x0 + ar * 2.0 + s(8.0);
        let inp = format!("{} \u{25be}", hd.input);
        let iw = text.measure(f11, &inp).ceil() + s(14.0);
        cv.fill_round_rect(
            ix,
            ry,
            iw,
            rh,
            s(3.0),
            if self.hover == Hit::Input(t) {
                th.hover
            } else {
                th.field
            },
        );
        text.draw_in(cv, f11, ix, ry, iw, rh, &inp, th.text, Align::Center);
        push(bx, vis, ix, ry, iw, rh, Hit::Input(t));
        // A droite : S, M, puis l'ecoute de l'entree.
        let bw = s(22.0);
        let fb = Font::new(0, s(10.5) as u16, 700);
        let sx = xr - bw;
        let mx = sx - bw - s(4.0);
        for (bxx, lab, on, hit, col) in [
            (sx, "S", hd.solo, Hit::Solo(t), th.playhead),
            (mx, "M", hd.mute, Hit::Mute(t), th.error),
        ] {
            let bgc = if on {
                col
            } else if self.hover == hit {
                th.hover
            } else {
                th.field
            };
            cv.fill_round_rect(bxx, ry, bw, rh, s(3.0), bgc);
            text.draw_in(
                cv,
                fb,
                bxx,
                ry,
                bw,
                rh,
                lab,
                if on { th.bg } else { th.text_dim },
                Align::Center,
            );
            push(bx, vis, bxx, ry, bw, rh, hit);
        }
        let mon = match hd.monitor {
            1 => "MON",
            2 => "AUTO",
            _ => "MON",
        };
        let mw = text.measure(fb, mon).ceil() + s(12.0);
        let monx = mx - s(6.0) - mw;
        if monx > ix + iw + s(4.0) {
            let on = hd.monitor != 0;
            cv.fill_round_rect(
                monx,
                ry,
                mw,
                rh,
                s(3.0),
                if self.hover == Hit::Monitor(t) {
                    th.hover
                } else {
                    th.field
                },
            );
            text.draw_in(
                cv,
                fb,
                monx,
                ry,
                mw,
                rh,
                mon,
                if on { th.playhead } else { th.text_faint },
                Align::Center,
            );
            push(bx, vis, monx, ry, mw, rh, Hit::Monitor(t));
        }
        // Les deux volumes, et le vumetre de la guitare.
        let (fx, fw) = fader_span(self, t);
        let rows: [(&str, f32, bool, f32); 2] = [
            ("Guitar", hd.vol_db, false, y + s(62.0)),
            ("Tab", hd.tab_vol_db, true, y + s(92.0)),
        ];
        for (lab, db, tab, fy) in rows {
            if tab && hd.tab_guid.is_none() {
                continue;
            }
            let fh = s(18.0);
            text.draw_in(cv, fsm, x0, fy, s(42.0), fh, lab, th.text_dim, Align::Left);
            let cy = fy + fh * 0.5;
            cv.fill_round_rect(fx, cy - s(2.0), fw, s(4.0), s(2.0), th.seam);
            let p = Arr::fader_pos(db);
            let kx = fx + p * fw;
            cv.fill_round_rect(
                fx,
                cy - s(2.0),
                kx - fx,
                s(4.0),
                s(2.0),
                th.accent.mix(th.text_dim, 0.45),
            );
            let hot = self.hover == Hit::Fader { t, tab };
            cv.fill_round_rect(
                kx - s(4.5),
                cy - s(7.0),
                s(9.0),
                s(14.0),
                s(2.0),
                if hot {
                    th.text
                } else {
                    th.text_dim.mix(th.text, 0.6)
                },
            );
            let v = if db <= -60.0 {
                "-inf".to_string()
            } else {
                format!("{db:+.1}")
            };
            text.draw_in(
                cv,
                Font::new(1, s(10.0) as u16, 400),
                fx + fw + s(4.0),
                fy,
                xr - fx - fw - s(4.0),
                fh,
                &v,
                th.text,
                Align::Right,
            );
            push(
                bx,
                vis,
                fx - s(6.0),
                fy,
                fw + s(12.0),
                fh,
                Hit::Fader { t, tab },
            );
            if !tab {
                // Le vumetre, sous le fader de la guitare.
                let my = fy + fh + s(2.0);
                cv.fill_rect(fx, my, fw, s(3.0), th.seam);
                let m = hd.meter.clamp(0.0, 1.0);
                if m > 0.0 {
                    let mc = if m > 0.89 {
                        th.error
                    } else if m > 0.5 {
                        th.playhead
                    } else {
                        th.accent
                    };
                    cv.fill_rect(fx, my, fw * m, s(3.0), mc);
                }
            }
        }
        // Le son de la tab.
        let sy = y + s(118.0);
        let sh = s(20.0);
        text.draw_in(
            cv,
            fsm,
            x0,
            sy,
            s(42.0),
            sh,
            "Sound",
            th.text_dim,
            Align::Left,
        );
        match &hd.synth {
            Some(sy0) => {
                let mut xx = fx;
                let segw = (xr - fx) / 4.0;
                for snd in Sound::ALL {
                    let on = sy0.sound == snd;
                    let hit = Hit::SoundPick { t, s: snd };
                    let bgc = if on {
                        th.bg
                    } else if self.hover == hit {
                        th.hover
                    } else {
                        th.field
                    };
                    cv.fill_rect(xx, sy, segw - 1.0, sh, bgc);
                    text.draw_in(
                        cv,
                        Font::new(0, s(10.5) as u16, if on { 700 } else { 400 }),
                        xx,
                        sy,
                        segw - 1.0,
                        sh,
                        snd.name(),
                        if on { th.accent } else { th.text_dim },
                        Align::Center,
                    );
                    push(bx, vis, xx, sy, segw, sh, hit);
                    xx += segw;
                }
            }
            None => {
                let msg = if hd.tab_guid.is_some() {
                    "your instrument"
                } else {
                    "no tab track yet"
                };
                text.draw_in(
                    cv,
                    fsm,
                    fx,
                    sy,
                    xr - fx,
                    sh,
                    msg,
                    th.text_faint,
                    Align::Left,
                );
            }
        }
        // Les couches.
        let ly = y + s(146.0);
        let mut lx = x0;
        let fl = Font::new(0, s(10.0) as u16, 400);
        for lane in Lane::ALL {
            let on = hd.lanes.on(lane);
            let lab = lane.name();
            let lw = text.measure(fl, lab).ceil() + s(10.0);
            let hit = Hit::LaneToggle { t, lane };
            let c = if on { th.accent } else { th.text_faint };
            if self.hover == hit {
                cv.fill_round_rect(lx, ly - s(1.0), lw, s(16.0), s(3.0), th.hover);
            }
            cv.stroke_rect(
                lx,
                ly - s(1.0),
                lw,
                s(16.0),
                1.0,
                if on {
                    th.accent.with_alpha(90)
                } else {
                    th.seam
                },
            );
            text.draw_in(cv, fl, lx, ly - s(1.0), lw, s(16.0), lab, c, Align::Center);
            push(bx, vis, lx, ly - s(1.0), lw, s(16.0), hit);
            lx += lw + s(4.0);
        }
    }

    fn draw_lanes(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        t: usize,
        y0: f32,
        h: f32,
        geo: Geo,
        vis: (f32, f32, f32, f32),
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let right = l.sx + l.sw;
        let lanes = self.tracks[t].head.lanes;
        let nstr = crate::tuning_of(self.tracks[t].head.tuning).strings.len();
        let paper = th.bg;
        let staff_c = th.text_dim.fade(0.75);
        let fs = Font::new(0, s(9.5) as u16, 700);
        let fm = Font::new(0, s(9.5) as u16, 400);
        let sel = self.sel;
        let hover = self.hover;
        let bq = self.bar_qn();
        let first = (self.scroll / bq).floor().max(0.0) as i64;
        let last = ((self.scroll + self.view_qn()) / bq).ceil() as i64;
        let content_clip = (l.sx, vis.1, l.sw, vis.3);
        // Le fond sous les couches (si l'en-tete est plus haut qu'elles).
        cv.fill_rect(l.label_x, y0, right - l.label_x + s(10.0), h, th.bg);
        let mut y = y0;
        let mut lane_ys: Vec<(Lane, f32, f32)> = Vec::new();
        for lane in Lane::ALL {
            if !lanes.on(lane) {
                continue;
            }
            let lh = s(lane_h(lane));
            lane_ys.push((lane, y, lh));
            y += lh + 1.0;
        }
        let rec: Vec<crate::Rec> = self.rec.iter().filter(|r| r.track == t).cloned().collect();
        let playhead = self.playhead;
        for &(lane, ly, lh) in &lane_ys {
            // Le fond et le nom de la couche.
            let bg = match lane {
                Lane::Audio | Lane::Midi => th.field,
                _ => paper,
            };
            cv.fill_rect(l.label_x, ly, right - l.label_x, lh, bg);
            if matches!(lane, Lane::Audio | Lane::Midi) {
                cv.fill_rect(
                    l.label_x,
                    ly,
                    l.sx - l.label_x,
                    lh,
                    th.panel.mix(th.bg, 0.5),
                );
            }
            text.draw(
                cv,
                fs,
                l.label_x + s(8.0),
                ly + s(13.0),
                &lane.name().to_uppercase(),
                th.text_dim,
            );
            let sub = match lane {
                Lane::Audio => self.tracks[t].head.input.clone(),
                Lane::Midi => format!("{nstr} ch"),
                _ => String::new(),
            };
            if !sub.is_empty() {
                text.draw(
                    cv,
                    fm,
                    l.label_x + s(8.0),
                    ly + s(26.0),
                    &sub,
                    th.text_faint,
                );
            }
            push(bx, content_clip, l.sx, ly, l.sw, lh, Hit::Lane { t, lane });
            // Les portees, sur toute la largeur, et les barres de mesure.
            match lane {
                Lane::Score => {
                    let gap = s(8.0);
                    let sy = ly + s(38.0);
                    for k in 0..5 {
                        cv.fill_rect(
                            l.label_x + s(22.0),
                            sy + k as f32 * gap,
                            right - l.label_x - s(22.0),
                            1.0,
                            staff_c,
                        );
                    }
                    let fc = Font::new(3, s(40.0) as u16, 400);
                    text.draw(
                        cv,
                        fc,
                        l.label_x + s(24.0),
                        sy + 4.0 * gap + s(8.0),
                        &CLEF.to_string(),
                        th.text,
                    );
                    text.draw(
                        cv,
                        Font::new(0, s(8.0) as u16, 700),
                        l.label_x + s(33.0),
                        sy + 4.0 * gap + s(19.0),
                        "8",
                        th.text,
                    );
                    if cv.push_clip(l.sx, ly, l.sw, lh) {
                        for m in first..=last {
                            let x = geo.x(m as f64 * bq).round();
                            cv.fill_rect(x, sy, 1.2, 4.0 * gap, staff_c);
                        }
                    }
                    cv.pop_clip();
                }
                Lane::Tab => {
                    let tg = s(11.0);
                    let ty = ly + s(18.0);
                    let names = [
                        "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
                    ];
                    let tun = crate::tuning_of(self.tracks[t].head.tuning);
                    for k in 0..nstr {
                        let yy = ty + k as f32 * tg;
                        cv.fill_rect(l.sx - s(6.0), yy, right - l.sx + s(6.0), 1.0, staff_c);
                        let open = tun.strings[nstr - 1 - k];
                        let nm = names[open.rem_euclid(12) as usize];
                        let nm = if k == 0 {
                            nm.to_lowercase()
                        } else {
                            nm.to_string()
                        };
                        text.draw_in(
                            cv,
                            fm,
                            l.sx - s(32.0),
                            yy - s(6.0),
                            s(22.0),
                            s(12.0),
                            &nm,
                            th.text_faint,
                            Align::Right,
                        );
                    }
                    if cv.push_clip(l.sx, ly, l.sw, lh) {
                        for m in first..=last {
                            let x = geo.x(m as f64 * bq).round();
                            cv.fill_rect(x, ty, 1.2, (nstr - 1) as f32 * tg, staff_c);
                        }
                    }
                    cv.pop_clip();
                }
                _ => {}
            }
        }
        // Les items, couche par couche.
        if cv.push_clip(
            l.sx,
            vis.1.max(y0),
            l.sw,
            (vis.1 + vis.3).min(y0 + h) - vis.1.max(y0),
        ) {
            for (i, it) in self.tracks[t].items.iter_mut().enumerate() {
                let (qa, qb) = (it.qn0, it.qn1);
                let (xa, xb) = (geo.x(qa), geo.x(qb));
                if xb < l.sx || xa > right {
                    continue;
                }
                let is_sel = sel.is_some_and(|x| x.t == t && x.i == Some(i));
                let sel_n = sel.filter(|x| x.t == t && x.i == Some(i)).and_then(|x| x.n);
                for &(lane, ly, lh) in &lane_ys {
                    match lane {
                        Lane::Audio => {
                            draw_audio_item(cv, text, th, it, geo, xa, xb, ly, lh, is_sel)
                        }
                        Lane::Midi => draw_midi_item(cv, th, it, geo, xa, xb, ly, lh, nstr),
                        _ => {}
                    }
                    push(
                        bx,
                        content_clip,
                        xa,
                        ly,
                        xb - xa,
                        lh,
                        Hit::Item { t, i, lane },
                    );
                    match lane {
                        Lane::Score => {
                            if it.state == Some(State::Ready) || !it.doc.is_empty() {
                                draw_score(
                                    cv,
                                    text,
                                    th,
                                    it,
                                    t,
                                    i,
                                    geo,
                                    ly + s(38.0),
                                    sel_n,
                                    hover,
                                    bx,
                                    content_clip,
                                );
                            } else {
                                draw_pending(cv, text, th, it, xa, xb, ly, lh);
                            }
                        }
                        Lane::Tab => {
                            let tg = s(11.0);
                            let ty = ly + s(18.0);
                            for k in 0..nstr {
                                push(
                                    bx,
                                    content_clip,
                                    xa,
                                    ty + k as f32 * tg - tg * 0.5,
                                    xb - xa,
                                    tg,
                                    Hit::TabLine {
                                        t,
                                        i,
                                        s: nstr - 1 - k,
                                    },
                                );
                            }
                            if it.state == Some(State::Ready) || !it.doc.is_empty() {
                                draw_tab(
                                    cv,
                                    text,
                                    th,
                                    it,
                                    t,
                                    i,
                                    geo,
                                    ty,
                                    tg,
                                    sel_n,
                                    hover,
                                    bx,
                                    content_clip,
                                );
                            } else if !lanes.on(Lane::Score) {
                                draw_pending(cv, text, th, it, xa, xb, ly, lh);
                            }
                        }
                        _ => {}
                    }
                }
                if is_sel {
                    cv.stroke_rect(
                        xa,
                        y0 + 1.0,
                        xb - xa,
                        (y - y0 - 2.0).max(0.0),
                        s(1.5),
                        th.accent,
                    );
                }
            }
            // L'enregistrement en cours : une prise rouge qui grandit.
            if let (Some(p), Some(&(_, ly, lh))) =
                (playhead, lane_ys.iter().find(|x| x.0 == Lane::Audio))
            {
                for r in &rec {
                    let (xa, xb) = (geo.x(r.qn0), geo.x(p));
                    if xb <= xa {
                        continue;
                    }
                    cv.fill_round_rect(
                        xa,
                        ly + s(3.0),
                        xb - xa,
                        lh - s(6.0),
                        s(2.0),
                        th.error.mix(th.bg, 0.65),
                    );
                    cv.fill_rect(xa, ly + s(3.0), xb - xa, s(14.0), th.error.mix(th.bg, 0.3));
                    text.draw(
                        cv,
                        Font::new(0, s(10.5) as u16, 600),
                        xa + s(6.0),
                        ly + s(14.0),
                        "\u{25cf} Recording",
                        th.text,
                    );
                    let mid = ly + s(17.0) + (lh - s(23.0)) * 0.5;
                    let half = (lh - s(25.0)) * 0.5;
                    for &(q, v) in &r.env {
                        let x = geo.x(q);
                        if x < xa || x > xb {
                            continue;
                        }
                        let hh = (v.clamp(0.0, 1.0) * half).max(0.5);
                        cv.fill_rect(x, mid - hh, 1.5, 2.0 * hh, th.error.mix(th.text, 0.45));
                    }
                }
                for r in &rec {
                    // Les autres couches : la tab viendra a l'arret.
                    for &(lane, ly2, lh2) in &lane_ys {
                        if matches!(lane, Lane::Score | Lane::Tab) {
                            let (xa, xb) = (geo.x(r.qn0), geo.x(p));
                            if xb > xa {
                                dashed_rect(
                                    cv,
                                    xa,
                                    ly2 + s(4.0),
                                    xb - xa,
                                    lh2 - s(8.0),
                                    th.text_faint,
                                );
                                text.draw(
                                    cv,
                                    Font::new(0, s(11.0) as u16, 400),
                                    xa + s(10.0),
                                    ly2 + lh2 * 0.5 + s(4.0),
                                    "The tab appears when you stop",
                                    th.text_dim,
                                );
                            }
                            break;
                        }
                    }
                }
            }
        }
        cv.pop_clip();
    }

    // -----------------------------------------------------------------------
    // L'inspecteur
    // -----------------------------------------------------------------------

    fn slider(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        y: f32,
        label: &str,
        t: f32,
        val: &str,
        hit: Hit,
        mark: Option<f32>,
    ) -> f32 {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let x0 = l.panel_x;
        let pad = s(12.0);
        let f = Font::new(0, s(12.0) as u16, 400);
        let fv = Font::new(1, s(11.0) as u16, 400);
        let tx0 = x0 + s(120.0);
        let tw = (l.w - tx0 - s(64.0)).max(10.0);
        let rh = s(22.0);
        text.draw_in(
            cv,
            f,
            x0 + pad,
            y,
            s(110.0),
            rh,
            label,
            th.text,
            Align::Left,
        );
        let cy = y + rh * 0.5;
        cv.fill_round_rect(tx0, cy - s(2.0), tw, s(4.0), s(2.0), th.seam);
        let kx = tx0 + t.clamp(0.0, 1.0) * tw;
        cv.fill_round_rect(
            tx0,
            cy - s(2.0),
            kx - tx0,
            s(4.0),
            s(2.0),
            th.accent.mix(th.text_dim, 0.45),
        );
        if let Some(m) = mark {
            let ox = tx0 + m * tw;
            cv.fill_rect(ox, cy - s(5.0), 1.0, s(10.0), th.text_faint);
        }
        let hot = self.hover == hit;
        cv.fill_circle(kx, cy, s(if hot { 6.5 } else { 5.5 }), th.text);
        cv.fill_round_rect(
            l.w - s(56.0),
            y + s(2.0),
            s(46.0),
            rh - s(4.0),
            s(3.0),
            th.field,
        );
        text.draw_in(
            cv,
            fv,
            l.w - s(56.0),
            y + s(2.0),
            s(42.0),
            rh - s(4.0),
            val,
            th.text,
            Align::Right,
        );
        bx.push((tx0 - s(6.0), y, tw + s(12.0), rh, hit));
        rh + s(2.0)
    }

    fn check(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        y: f32,
        label: &str,
        on: bool,
        hit: Hit,
    ) -> f32 {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let pad = s(12.0);
        let bxx = l.panel_x + pad;
        let bs = s(13.0);
        let by = y + s(4.0);
        cv.fill_round_rect(bxx, by, bs, bs, s(2.0), th.field);
        if on {
            text.draw_in(
                cv,
                Font::new(0, s(11.0) as u16, 700),
                bxx,
                by,
                bs,
                bs,
                "\u{2713}",
                th.accent,
                Align::Center,
            );
        }
        text.draw_in(
            cv,
            Font::new(0, s(12.0) as u16, 400),
            bxx + bs + s(7.0),
            y,
            l.panel_w - pad * 2.0 - bs,
            s(21.0),
            label,
            th.text,
            Align::Left,
        );
        bx.push((bxx, y, l.panel_w - pad * 2.0, s(21.0), hit));
        s(22.0)
    }

    fn pbutton(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        x: f32,
        y: f32,
        label: &str,
        hit: Hit,
        danger: bool,
    ) -> f32 {
        let s = |v: f32| th.s(v);
        let f = Font::new(0, s(12.0) as u16, 400);
        let w = text.measure(f, label).ceil() + s(18.0);
        cv.fill_round_rect(
            x,
            y,
            w,
            s(24.0),
            s(3.0),
            if self.hover == hit {
                th.hover
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
            s(24.0),
            label,
            if danger { th.error } else { th.text },
            Align::Center,
        );
        bx.push((x, y, w, s(24.0), hit));
        w
    }

    fn section(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme, y: f32, t: &str) {
        text.draw(
            cv,
            Font::new(0, th.s(9.5) as u16, 700),
            self.lay.panel_x + th.s(12.0),
            y + th.s(8.0),
            t,
            th.text_faint,
        );
    }

    fn draw_panel(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let x0 = l.panel_x;
        let w = l.panel_w;
        let pad = s(12.0);
        cv.fill_rect(x0, l.top, w, l.bottom - l.top, th.panel);
        if !cv.push_clip(x0, l.top, w, l.bottom - l.top) {
            cv.pop_clip();
            return;
        }
        let fh = Font::new(0, s(10.5) as u16, 700);
        let fs = Font::new(0, s(10.5) as u16, 400);
        let f = Font::new(0, s(12.0) as u16, 400);
        let mut y = l.top + s(12.0);
        let sel_item = self.sel_item();
        if let Some((t, i)) = sel_item {
            let (name, qa, qb, takes, active, has_synth) = {
                let it = &self.tracks[t].items[i];
                (
                    it.name.clone(),
                    it.qn0,
                    it.qn1,
                    it.takes.clone(),
                    it.active,
                    self.tracks[t].head.synth.is_some(),
                )
            };
            let _ = has_synth;
            text.draw(cv, fh, x0 + pad, y + s(9.0), "ITEM", th.text_dim);
            text.draw_in(
                cv,
                fs,
                x0 + pad + s(40.0),
                y,
                w - pad * 2.0 - s(40.0),
                s(12.0),
                &name,
                th.text_faint,
                Align::Left,
            );
            y += s(16.0);
            let bq = self.bar_qn();
            let (ba, bb) = (
                (qa / bq).floor() as i64 + 1,
                ((qb - 1e-6) / bq).floor() as i64 + 1,
            );
            text.draw(
                cv,
                fs,
                x0 + pad,
                y + s(9.0),
                &format!("Bars {ba}\u{2013}{bb}"),
                th.text_faint,
            );
            y += s(20.0);
            // Les prises.
            self.section(cv, text, th, y, "TAKES");
            y += s(16.0);
            for (k, tk) in takes.iter().enumerate() {
                let rh = s(21.0);
                let on = k == active;
                let hit = Hit::Take(k);
                cv.fill_round_rect(
                    x0 + pad,
                    y,
                    w - pad * 2.0,
                    rh - s(2.0),
                    s(3.0),
                    if self.hover == hit {
                        th.hover
                    } else {
                        th.field
                    },
                );
                if on {
                    cv.fill_rect(x0 + pad, y, s(2.0), rh - s(2.0), th.accent);
                }
                let lab = if tk.name.is_empty() {
                    format!("Take {}", k + 1)
                } else {
                    format!("{} · {}", k + 1, tk.name)
                };
                text.draw_in(
                    cv,
                    f,
                    x0 + pad + s(8.0),
                    y,
                    w - pad * 2.0 - s(80.0),
                    rh - s(2.0),
                    &lab,
                    if on { th.text } else { th.text_dim },
                    Align::Left,
                );
                text.draw_in(
                    cv,
                    fs,
                    x0 + pad,
                    y,
                    w - pad * 2.0 - s(8.0),
                    rh - s(2.0),
                    if tk.done { "transcribed" } else { "\u{2014}" },
                    th.text_faint,
                    Align::Right,
                );
                bx.push((x0 + pad, y, w - pad * 2.0, rh - s(2.0), hit));
                y += rh;
            }
            y += s(8.0);
            // La detection de cet item.
            let det = self.tracks[t].items[i].detect.clone();
            self.section(cv, text, th, y, "DETECTION \u{00b7} THIS ITEM");
            if self.range.is_some() {
                text.draw_in(
                    cv,
                    fs,
                    x0,
                    y,
                    w - pad,
                    s(14.0),
                    "applies to the passage",
                    th.accent,
                    Align::Right,
                );
            }
            y += s(18.0);
            y += self.slider(
                cv,
                text,
                th,
                bx,
                y,
                "Onset sensitivity",
                (det.onset - 0.05) / 0.9,
                &format!("{:.2}", det.onset),
                Hit::Slider(0),
                Some((0.5 - 0.05) / 0.9),
            );
            text.draw(
                cv,
                fs,
                x0 + pad,
                y + s(9.0),
                "Lower = more notes caught, and more noise.",
                th.text_faint,
            );
            y += s(16.0);
            y += self.slider(
                cv,
                text,
                th,
                bx,
                y,
                "Note sensitivity",
                (det.frame - 0.05) / 0.9,
                &format!("{:.2}", det.frame),
                Hit::Slider(1),
                None,
            );
            y += self.slider(
                cv,
                text,
                th,
                bx,
                y,
                "Shortest note",
                ((det.min_ms - 20.0) / 380.0) as f32,
                &format!("{:.0} ms", det.min_ms),
                Hit::Slider(2),
                None,
            );
            y += self.check(
                cv,
                text,
                th,
                bx,
                y,
                "Legato notes (no attack)",
                det.legato,
                Hit::Check(0),
            );
            y += self.check(
                cv,
                text,
                th,
                bx,
                y,
                "Show ghosts (just under the threshold)",
                det.ghosts,
                Hit::Check(1),
            );
            y += s(4.0);
            text.draw_in(
                cv,
                f,
                x0 + pad,
                y,
                s(80.0),
                s(24.0),
                "Grid",
                th.text,
                Align::Left,
            );
            let mut gx = l.w - pad;
            for (g, label) in [(8u32, "1/32"), (4, "1/16"), (2, "1/8")] {
                let gw = text.measure(f, label) + s(16.0);
                gx -= gw;
                let on = det.grid == g;
                cv.fill_rect(
                    gx,
                    y + s(1.0),
                    gw - 1.0,
                    s(22.0),
                    if on { th.bg } else { th.field },
                );
                text.draw_in(
                    cv,
                    f,
                    gx,
                    y + s(1.0),
                    gw - 1.0,
                    s(22.0),
                    label,
                    if on { th.accent } else { th.text_dim },
                    Align::Center,
                );
                bx.push((gx, y, gw, s(24.0), Hit::Grid(g)));
            }
            y += s(32.0);
            // Les comptes.
            let (nn, chords, gh, dropped) = {
                let it = &mut self.tracks[t].items[i];
                let c = it.engraved().chords;
                (it.doc.len(), c, it.ghosts.len(), it.dropped)
            };
            let stats = [
                ("Notes", nn.to_string()),
                ("Chords", chords.to_string()),
                (
                    "Ghosts",
                    if det.ghosts {
                        gh.to_string()
                    } else {
                        "off".into()
                    },
                ),
                ("Unplayable", dropped.to_string()),
            ];
            let bh = s(46.0);
            cv.fill_round_rect(x0 + pad, y, w - 2.0 * pad, bh, s(4.0), th.field);
            let cw = (w - 2.0 * pad) * 0.5;
            let fv = Font::new(1, s(11.0) as u16, 400);
            for (k, (key, v)) in stats.iter().enumerate() {
                let cx = x0 + pad + (k % 2) as f32 * cw + s(8.0);
                let cy = y + s(4.0) + (k / 2) as f32 * s(19.0);
                text.draw_in(
                    cv,
                    fs,
                    cx,
                    cy,
                    cw - s(16.0),
                    s(18.0),
                    key,
                    th.text_dim,
                    Align::Left,
                );
                text.draw_in(
                    cv,
                    fv,
                    cx,
                    cy,
                    cw - s(16.0),
                    s(18.0),
                    v,
                    th.text,
                    Align::Right,
                );
            }
            y += bh + s(10.0);
            let keep = if self.range.is_some() {
                "Keep ghosts in passage"
            } else {
                "Keep all ghosts"
            };
            let kw = self.pbutton(cv, text, th, bx, x0 + pad, y, keep, Hit::KeepGhosts, false);
            if x0 + pad + kw + s(6.0) + s(110.0) < l.w {
                self.pbutton(
                    cv,
                    text,
                    th,
                    bx,
                    x0 + pad + kw + s(6.0),
                    y,
                    "Transcribe again",
                    Hit::Retranscribe,
                    false,
                );
            }
            y += s(30.0);
            let dw = self.pbutton(
                cv,
                text,
                th,
                bx,
                x0 + pad,
                y,
                "Delete item",
                Hit::DeleteItem,
                true,
            );
            self.pbutton(
                cv,
                text,
                th,
                bx,
                x0 + pad + dw + s(6.0),
                y,
                "Export MusicXML",
                Hit::Export,
                false,
            );
            y += s(30.0);
            text.draw(
                cv,
                fs,
                x0 + pad,
                y + s(9.0),
                "Delete removes the take and its tab together.",
                th.text_faint,
            );
            y += s(14.0);
            text.draw(
                cv,
                fs,
                x0 + pad,
                y + s(9.0),
                "Double-click the MIDI to open the MIDI editor.",
                th.text_faint,
            );
            y += s(24.0);
        } else {
            text.draw(cv, fh, x0 + pad, y + s(9.0), "GUITAR TAB", th.text_dim);
            y += s(24.0);
            for line in [
                "Click an item to see its takes",
                "and its detection settings.",
                "",
                "Arm a track and record: the take",
                "is transcribed when you stop.",
                "",
                "Double-click the MIDI to open",
                "the MIDI editor.",
            ] {
                text.draw(cv, f, x0 + pad, y + s(10.0), line, th.text_dim);
                y += s(17.0);
            }
            y += s(10.0);
        }
        // Le son de la piste.
        if let Some(t) = self.sel_track() {
            if let Some(sy) = self.tracks[t].head.synth {
                cv.fill_rect(x0 + pad, y, w - 2.0 * pad, 1.0, th.seam);
                y += s(10.0);
                self.section(cv, text, th, y, "TRACK SOUND");
                text.draw_in(
                    cv,
                    fs,
                    x0,
                    y,
                    w - pad,
                    s(14.0),
                    &self.tracks[t].head.name,
                    th.text_faint,
                    Align::Right,
                );
                y += s(18.0);
                y += self.slider(
                    cv,
                    text,
                    th,
                    bx,
                    y,
                    "Tone",
                    sy.tone,
                    &format!("{:.0} %", sy.tone * 100.0),
                    Hit::SynthSlider(1),
                    None,
                );
                y += self.slider(
                    cv,
                    text,
                    th,
                    bx,
                    y,
                    "Sustain",
                    (sy.sustain - 0.1) / 5.9,
                    &format!("{:.1} s", sy.sustain),
                    Hit::SynthSlider(2),
                    None,
                );
                self.check(
                    cv,
                    text,
                    th,
                    bx,
                    y,
                    "One note per string",
                    sy.mono,
                    Hit::SynthMono,
                );
            }
        }
        cv.pop_clip();
    }

    fn draw_status(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let y = l.h - l.status_h;
        cv.fill_rect(0.0, y - 2.0, l.w, 2.0, th.seam);
        cv.fill_rect(0.0, y, l.w, l.status_h, th.bar);
        let f = Font::new(0, th.s(11.0) as u16, 400);
        let fm = Font::new(1, th.s(10.5) as u16, 400);
        let left = if !self.rec.is_empty() {
            "\u{25cf} Recording \u{00b7} the tab appears when you stop".to_string()
        } else {
            self.sel_text().unwrap_or_else(|| self.status.clone())
        };
        let lw = text.measure(f, &left).min(l.w * 0.45);
        text.draw_in(
            cv,
            f,
            th.s(10.0),
            y,
            lw,
            l.status_h,
            &left,
            if self.rec.is_empty() {
                th.text
            } else {
                th.error
            },
            Align::Left,
        );
        let keys = "R arm \u{00b7} Ctrl+R record \u{00b7} Del item \u{00b7} 0\u{2013}9 fret \u{00b7} \u{2191}\u{2193} string \u{00b7} H P";
        let kx = th.s(24.0) + lw;
        let right = match &self.busy {
            Some(b) => b.clone(),
            None => {
                let items: usize = self.tracks.iter().map(|t| t.items.len()).sum();
                format!(
                    "{} track{} \u{00b7} {} item{}",
                    self.tracks.len(),
                    if self.tracks.len() == 1 { "" } else { "s" },
                    items,
                    if items == 1 { "" } else { "s" }
                )
            }
        };
        let rw = text.measure(f, &right).ceil();
        if l.w - rw - th.s(30.0) - kx > th.s(80.0) {
            text.draw_in(
                cv,
                fm,
                kx,
                y,
                l.w - rw - th.s(30.0) - kx,
                l.status_h,
                keys,
                th.text_faint,
                Align::Left,
            );
        }
        text.draw_in(
            cv,
            f,
            l.w - th.s(10.0) - rw,
            y,
            rw,
            l.status_h,
            &right,
            if self.busy.is_some() {
                th.accent
            } else {
                th.text_dim
            },
            Align::Left,
        );
    }
}

/// L'item dans la couche audio : son cadre, son nom, sa forme d'onde (de
/// l'analyse), ou ce qui l'attend.
fn draw_audio_item(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &Item,
    geo: Geo,
    xa: f32,
    xb: f32,
    ly: f32,
    lh: f32,
    sel: bool,
) {
    let s = |v: f32| v * geo.s;
    let body = if sel {
        th.panel.mix(th.text, 0.16)
    } else {
        th.panel.mix(th.text, 0.1)
    };
    cv.fill_round_rect(
        xa + 1.0,
        ly + s(3.0),
        (xb - xa - 2.0).max(1.0),
        lh - s(6.0),
        s(2.0),
        body,
    );
    cv.fill_rect(
        xa + 1.0,
        ly + s(3.0),
        (xb - xa - 2.0).max(1.0),
        s(14.0),
        if sel {
            th.accent.with_alpha(70)
        } else {
            Color::BLACK.with_alpha(40)
        },
    );
    let f = Font::new(0, s(10.5) as u16, 500);
    let mut name = it.name.clone();
    if it.takes.len() > 1 {
        name = format!(
            "{name} \u{00b7} take {} of {}",
            it.active + 1,
            it.takes.len()
        );
    }
    if cv.push_clip(xa + 1.0, ly, (xb - xa - 2.0).max(0.0), lh) {
        text.draw(
            cv,
            f,
            xa.max(geo.clip.0) + s(6.0),
            ly + s(13.5),
            &name,
            th.text,
        );
        // La forme d'onde.
        let mid = ly + s(17.0) + (lh - s(23.0)) * 0.5;
        let half = (lh - s(25.0)) * 0.5;
        let n = it.frame_qn.len();
        if n > 0 && !it.env.is_empty() {
            let wave_c = th.text_dim.mix(th.text, 0.3).fade(0.85);
            let mut x = xa.max(geo.clip.0);
            let x_end = xb.min(geo.clip.0 + geo.clip.2);
            while x < x_end {
                let qa = geo.scroll + ((x - geo.sx) / geo.ppq.max(1e-3)) as f64;
                let qb = geo.scroll + ((x + 1.0 - geo.sx) / geo.ppq.max(1e-3)) as f64;
                let fa = it.frame_qn.partition_point(|&q| q < qa);
                let fb = it.frame_qn.partition_point(|&q| q < qb).max(fa + 1).min(n);
                if fa < n && fa < it.env.len() {
                    let e = it.env[fa..fb.min(it.env.len())]
                        .iter()
                        .fold(0f32, |m, &v| m.max(v));
                    let hh = (e * half).max(0.5);
                    cv.fill_rect(x, mid - hh, 1.0, 2.0 * hh, wave_c);
                }
                x += 1.0;
            }
        }
        if let Some(st) = &it.state {
            let msg = match st {
                State::Ready => String::new(),
                State::Waiting => "Waiting to be transcribed\u{2026}".into(),
                State::Busy(p) => format!("Transcribing\u{2026} {p} %"),
                State::Failed(e) => format!("Not transcribed: {e}"),
            };
            if !msg.is_empty() {
                let fm = Font::new(0, s(11.0) as u16, 400);
                text.draw(
                    cv,
                    fm,
                    xa.max(geo.clip.0) + s(8.0),
                    mid + s(4.0),
                    &msg,
                    th.text_dim,
                );
                if let State::Busy(p) = st {
                    let pw = (xb - xa - s(4.0)).max(0.0) * (*p as f32 / 100.0);
                    cv.fill_rect(xa + s(2.0), ly + lh - s(6.0), pw, s(2.0), th.accent);
                }
            }
        }
    }
    cv.pop_clip();
}

/// L'item dans la couche MIDI : un petit piano roll, une couleur par corde.
fn draw_midi_item(
    cv: &mut Canvas<'_>,
    th: &Theme,
    it: &Item,
    geo: Geo,
    xa: f32,
    xb: f32,
    ly: f32,
    lh: f32,
    nstr: usize,
) {
    let s = |v: f32| v * geo.s;
    cv.fill_round_rect(
        xa + 1.0,
        ly + s(3.0),
        (xb - xa - 2.0).max(1.0),
        lh - s(6.0),
        s(2.0),
        th.bg.mix(th.text, 0.05),
    );
    let (lo, hi) = (38.0f32, 82.0f32);
    for n in &it.doc {
        let x = geo.x(n.qn0);
        let w = (geo.x(n.qn1) - x - 1.0).max(1.5);
        if !geo.visible(x) && !geo.visible(x + w) {
            continue;
        }
        let y = ly + s(5.0) + (hi - n.pitch as f32) / (hi - lo) * (lh - s(13.0));
        cv.fill_rect(
            x,
            y,
            w,
            s(3.0),
            string_color(n.string.min(nstr.saturating_sub(1))),
        );
    }
    for g in &it.ghosts {
        let x = geo.x(g.qn0);
        let w = (geo.x(g.qn1) - x - 1.0).max(1.5);
        let y = ly + s(5.0) + (hi - g.pitch as f32) / (hi - lo) * (lh - s(13.0));
        dashed_rect(cv, x, y, w, s(3.0), th.text_faint);
    }
}

/// Un item pas encore transcrit, dans la partition ou la tab.
fn draw_pending(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &Item,
    xa: f32,
    xb: f32,
    ly: f32,
    lh: f32,
) {
    let s = th.scale;
    let msg = match &it.state {
        Some(State::Busy(p)) => format!("Transcribing\u{2026} {p} %"),
        Some(State::Failed(e)) => format!("Not transcribed: {e}"),
        _ => "Waiting to be transcribed\u{2026}".into(),
    };
    dashed_rect(
        cv,
        xa + 2.0,
        ly + 4.0 * s,
        (xb - xa - 4.0).max(0.0),
        lh - 8.0 * s,
        th.text_faint,
    );
    text.draw(
        cv,
        Font::new(0, (11.0 * s) as u16, 400),
        xa + 10.0 * s,
        ly + lh * 0.5 + 4.0 * s,
        &msg,
        th.text_dim,
    );
}

/// La partition d'un item (sa portee est deja tracee par la couche).
fn draw_score(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &mut Item,
    t: usize,
    i: usize,
    geo: Geo,
    score_y: f32,
    sel_n: Option<usize>,
    hover: Hit,
    bx: &mut Vec<HBox>,
    clip: (f32, f32, f32, f32),
) {
    let s = |v: f32| v * geo.s;
    it.engraved();
    let eng: Engraved = std::mem::take(&mut it.eng);
    let ink = th.text;
    let paper = th.bg;
    let staff_c = th.text_dim.fade(0.75);
    let gap = s(8.0);
    let pad = s(9.0);
    let hx = |cell: i64| geo.x(eng.qn(cell)) + pad;
    let (rx, ry) = (gap * 0.62, gap * 0.46);
    let y_of = |pos: i32| score_y + 4.0 * gap - pos as f32 * gap * 0.5;
    // Les groupes de ligature : signes brefs consecutifs d'un meme temps.
    let mut beam_group: Vec<Option<usize>> = vec![None; eng.signs.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (si, sg) in eng.signs.iter().enumerate() {
        let ev = &eng.events[sg.ev];
        if ev.notes.is_empty() || beams_of(sg.ty) == 0 {
            continue;
        }
        let beat = sg.start / eng.grid.max(1);
        let joins = si > 0
            && beam_group[si - 1].is_some()
            && eng.signs[si - 1].measure == sg.measure
            && eng.signs[si - 1].start / eng.grid.max(1) == beat;
        if joins {
            let g = beam_group[si - 1].unwrap_or(0);
            groups[g].push(si);
            beam_group[si] = Some(g);
        } else {
            groups.push(vec![si]);
            beam_group[si] = Some(groups.len() - 1);
        }
    }
    let positions = |si: usize| -> Vec<i32> {
        eng.events[eng.signs[si].ev]
            .notes
            .iter()
            .map(|&n| staff_pos(it.doc[n].pitch).0)
            .collect()
    };
    let mut stem: Vec<Option<(bool, f32)>> = vec![None; eng.signs.len()];
    for g in &groups {
        let all: Vec<i32> = g.iter().flat_map(|&si| positions(si)).collect();
        let up = (all.iter().sum::<i32>() as f32 / all.len().max(1) as f32) < 4.0;
        let end = if up {
            all.iter().map(|&p| y_of(p)).fold(f32::MAX, f32::min) - 3.4 * gap
        } else {
            all.iter().map(|&p| y_of(p)).fold(f32::MIN, f32::max) + 3.4 * gap
        };
        for &si in g {
            stem[si] = Some((up, end));
        }
    }
    let fm = Font::new(3, (gap * 4.0) as u16, 400);
    let fsharp = Font::new(3, (gap * 2.6) as u16, 400);
    for (si, sg) in eng.signs.iter().enumerate() {
        let x = hx(sg.start);
        if !geo.visible(x) {
            continue;
        }
        let ev = &eng.events[sg.ev];
        if ev.notes.is_empty() {
            let c = rest_char(sg.ty);
            let rw = text.measure(fm, &c.to_string());
            let base = if sg.ty == "whole" || sg.ty == "half" {
                y_of(6) + gap * 0.2
            } else {
                y_of(4) + gap * 1.2
            };
            text.draw(cv, fm, x - rw * 0.5, base, &c.to_string(), th.text_dim);
            continue;
        }
        let ps: Vec<(i32, bool, usize)> = ev
            .notes
            .iter()
            .map(|&n| {
                let (p, sh) = staff_pos(it.doc[n].pitch);
                (p, sh, n)
            })
            .collect();
        let (up, end) = stem[si].unwrap_or_else(|| {
            let up = (ps.iter().map(|p| p.0).sum::<i32>() as f32 / ps.len() as f32) < 4.0;
            let e = if up {
                y_of(ps.iter().map(|p| p.0).max().unwrap_or(0)) - 3.4 * gap
            } else {
                y_of(ps.iter().map(|p| p.0).min().unwrap_or(0)) + 3.4 * gap
            };
            (up, e)
        });
        let lo = ps.iter().map(|p| p.0).min().unwrap_or(0);
        let hi = ps.iter().map(|p| p.0).max().unwrap_or(0);
        let mut p = -2;
        while p >= lo {
            cv.fill_rect(x - rx * 1.7, y_of(p), rx * 3.4, 1.0, staff_c);
            p -= 2;
        }
        let mut p = 10;
        while p <= hi {
            cv.fill_rect(x - rx * 1.7, y_of(p), rx * 3.4, 1.0, staff_c);
            p += 2;
        }
        let hollow = sg.ty == "half" || sg.ty == "whole";
        let mut prev: Option<i32> = None;
        for &(pos, sharp, n) in &ps {
            let dx = if prev.is_some_and(|q| pos - q == 1) {
                if up { 2.0 * rx } else { -2.0 * rx }
            } else {
                0.0
            };
            prev = Some(pos);
            let y = y_of(pos);
            let c = if sel_n == Some(n) { th.accent } else { ink };
            let hp = head(x + dx, y, rx, ry);
            cv.fill_path(&hp, c);
            if hollow {
                cv.fill_path(&head(x + dx, y, rx * 0.62, ry * 0.42), paper);
            }
            if sharp && !sg.tie_in {
                let sw = text.measure(fsharp, &SHARP.to_string());
                text.draw(
                    cv,
                    fsharp,
                    x + dx - rx - s(3.0) - sw,
                    y + gap * 0.9,
                    &SHARP.to_string(),
                    ink,
                );
            }
            if sg.dots > 0 {
                let dy = if pos % 2 == 0 { -gap * 0.5 } else { 0.0 };
                cv.fill_circle(x + rx + s(4.0), y + dy, s(1.6), ink);
            }
            if sg.tie_out {
                if let Some(nx) = eng
                    .signs
                    .get(si + 1)
                    .filter(|t| t.ev == sg.ev)
                    .map(|t| hx(t.start))
                {
                    let b = if up { gap * 0.9 } else { -gap * 0.9 };
                    arc(
                        cv,
                        x + rx,
                        y + b * 0.6,
                        nx - rx,
                        y + b * 0.6,
                        b * 0.5,
                        1.1,
                        ink,
                    );
                }
            }
            push(
                bx,
                clip,
                x + dx - rx - 2.0,
                y - ry - 2.0,
                2.0 * rx + 4.0,
                2.0 * ry + 4.0,
                Hit::Note { t, i, n },
            );
        }
        if sg.ty != "whole" {
            let sx = if up { x + rx - 0.8 } else { x - rx + 0.2 };
            let from = if up { y_of(lo) } else { y_of(hi) };
            cv.line(sx, from, sx, end, s(1.15), ink);
            if beam_group[si].is_some_and(|g| groups[g].len() == 1) {
                for k in 0..beams_of(sg.ty) {
                    let yy = if up {
                        end + k as f32 * gap * 0.8
                    } else {
                        end - k as f32 * gap * 0.8
                    };
                    let mut fp = Path::new();
                    if up {
                        fp.move_to(sx, yy).quad_to(
                            sx + gap * 1.2,
                            yy + gap * 0.9,
                            sx + gap * 0.9,
                            yy + gap * 2.2,
                        );
                    } else {
                        fp.move_to(sx, yy).quad_to(
                            sx + gap * 1.2,
                            yy - gap * 0.9,
                            sx + gap * 0.9,
                            yy - gap * 2.2,
                        );
                    }
                    cv.stroke_path(&fp, s(1.4), ink);
                }
            }
        }
        if sg.first && ev.lig_in != Tech::Plain {
            if let Some(prev_sign) = eng.signs[..si].iter().rev().find(|t| t.ev + 1 == sg.ev) {
                let px = hx(prev_sign.start);
                let y = y_of(hi) - gap * 1.2;
                arc(cv, px, y, x, y, -gap * 0.7, 1.1, th.text_dim);
            }
        }
    }
    // Les ligatures.
    for g in &groups {
        if g.len() < 2 {
            continue;
        }
        let (up, end) = stem[g[0]].unwrap_or((true, 0.0));
        let xs: Vec<f32> = g
            .iter()
            .map(|&si| {
                let x = hx(eng.signs[si].start);
                if up { x + rx - 0.8 } else { x - rx + 0.2 }
            })
            .collect();
        if xs.iter().all(|&x| !geo.visible(x)) {
            continue;
        }
        let th_b = gap * 0.48;
        let by = if up { end } else { end - th_b };
        cv.fill_rect(xs[0], by, xs[xs.len() - 1] - xs[0] + s(1.15), th_b, ink);
        for k in 1..3 {
            let off = if up {
                k as f32 * gap * 0.8
            } else {
                -(k as f32) * gap * 0.8
            };
            for (j, &si) in g.iter().enumerate() {
                if beams_of(eng.signs[si].ty) <= k {
                    continue;
                }
                let next = g.get(j + 1).filter(|&&n| beams_of(eng.signs[n].ty) > k);
                let prev = j > 0 && beams_of(eng.signs[g[j - 1]].ty) > k;
                if next.is_some() {
                    cv.fill_rect(xs[j], by + off, xs[j + 1] - xs[j] + s(1.15), th_b, ink);
                } else if !prev {
                    let stub = gap * 1.1;
                    let x0 = if j + 1 < xs.len() {
                        xs[j]
                    } else {
                        xs[j] - stub
                    };
                    cv.fill_rect(x0, by + off, stub, th_b, ink);
                }
            }
        }
    }
    // Les fantomes : une tete creuse en pointilles.
    if it.detect.ghosts {
        let g = eng.grid.max(1) as f64;
        for (gi, gn) in it.ghosts.iter().enumerate() {
            let cell = ((gn.qn0 - eng.origin_qn) * g).round();
            let x = geo.x(eng.origin_qn + cell / g) + pad;
            if !geo.visible(x) {
                continue;
            }
            let c = if hover == (Hit::Ghost { t, i, g: gi }) {
                th.accent
            } else {
                th.text_faint
            };
            let (pos, _) = staff_pos(gn.pitch);
            let y = y_of(pos);
            let n = 14;
            for k in (0..n).step_by(2) {
                let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
                cv.line(
                    x + rx * a0.cos(),
                    y + ry * a0.sin(),
                    x + rx * a1.cos(),
                    y + ry * a1.sin(),
                    1.1,
                    c,
                );
            }
            push(
                bx,
                clip,
                x - rx - 2.0,
                y - ry - 2.0,
                2.0 * rx + 4.0,
                2.0 * ry + 4.0,
                Hit::Ghost { t, i, g: gi },
            );
        }
    }
    it.eng = eng;
}

/// La tablature d'un item (ses lignes sont deja tracees par la couche).
fn draw_tab(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &mut Item,
    t: usize,
    i: usize,
    geo: Geo,
    tab_y: f32,
    tab_gap: f32,
    sel_n: Option<usize>,
    hover: Hit,
    bx: &mut Vec<HBox>,
    clip: (f32, f32, f32, f32),
) {
    let s = |v: f32| v * geo.s;
    it.engraved();
    let eng: Engraved = std::mem::take(&mut it.eng);
    let nstr = it.detect.tuning().strings.len();
    let paper = th.bg;
    let pad = s(9.0);
    let hx = |cell: i64| geo.x(eng.qn(cell)) + pad;
    let fd = Font::new(0, s(11.0) as u16, 600);
    let fl = Font::new(0, s(8.5) as u16, 700);
    let line_y = |string: usize| tab_y + (nstr - 1 - string.min(nstr - 1)) as f32 * tab_gap;
    for (si, sg) in eng.signs.iter().enumerate() {
        if !sg.first {
            continue;
        }
        let x = hx(sg.start);
        if !geo.visible(x) {
            continue;
        }
        let ev = &eng.events[sg.ev];
        for &n in &ev.notes {
            let note = &it.doc[n];
            let y = line_y(note.string);
            let tx = note.fret.to_string();
            let tw = text.measure(fd, &tx).ceil() + s(4.0);
            let sel = sel_n == Some(n);
            if sel {
                cv.fill_round_rect(
                    x - tw * 0.5 - s(1.0),
                    y - s(6.5),
                    tw + s(2.0),
                    s(13.0),
                    s(2.0),
                    th.accent,
                );
            } else {
                cv.fill_rect(x - tw * 0.5, y - s(5.5), tw, s(11.0), paper);
            }
            text.draw_in(
                cv,
                fd,
                x - tw * 0.5,
                y - s(7.0),
                tw,
                s(14.0),
                &tx,
                if sel { th.bg } else { th.text },
                Align::Center,
            );
            push(
                bx,
                clip,
                x - tw * 0.5 - s(2.0),
                y - s(7.0),
                tw + s(4.0),
                s(14.0),
                Hit::Note { t, i, n },
            );
        }
        if ev.lig_in != Tech::Plain && ev.notes.len() == 1 {
            if let Some(prev_sign) = eng.signs[..si].iter().rev().find(|t| t.ev + 1 == sg.ev) {
                let px = hx(prev_sign.start);
                let y = line_y(it.doc[ev.notes[0]].string) - s(8.0);
                arc(cv, px, y, x, y, -s(4.0), 1.1, th.text_dim);
                let lab = if ev.lig_in == Tech::Hammer { "H" } else { "P" };
                text.draw_in(
                    cv,
                    fl,
                    (px + x) * 0.5 - s(6.0),
                    y - s(15.0),
                    s(12.0),
                    s(10.0),
                    lab,
                    th.text_dim,
                    Align::Center,
                );
            }
        }
    }
    if it.detect.ghosts {
        let g = eng.grid.max(1) as f64;
        for (gi, gn) in it.ghosts.iter().enumerate() {
            let cell = ((gn.qn0 - eng.origin_qn) * g).round();
            let x = geo.x(eng.origin_qn + cell / g) + pad;
            if !geo.visible(x) {
                continue;
            }
            let hov = hover == (Hit::Ghost { t, i, g: gi });
            let c = if hov { th.accent } else { th.text_faint };
            let y = line_y(gn.string);
            let tx = gn.fret.to_string();
            let tw = text.measure(fd, &tx).ceil() + s(4.0);
            cv.fill_rect(
                x - tw * 0.5 - s(1.0),
                y - s(6.5),
                tw + s(2.0),
                s(13.0),
                paper,
            );
            dashed_rect(
                cv,
                x - tw * 0.5 - s(1.0),
                y - s(6.5),
                tw + s(2.0),
                s(13.0),
                c,
            );
            text.draw_in(
                cv,
                fd,
                x - tw * 0.5,
                y - s(7.0),
                tw,
                s(14.0),
                &tx,
                c,
                Align::Center,
            );
            push(
                bx,
                clip,
                x - tw * 0.5 - s(2.0),
                y - s(7.0),
                tw + s(4.0),
                s(14.0),
                Hit::Ghost { t, i, g: gi },
            );
        }
    }
    it.eng = eng;
}
