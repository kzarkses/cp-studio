//! Le dessin de la fenetre de Guitar Tab : la barre, la partition (portee
//! et tablature) sur un axe du temps proportionnel, le son et les attaques
//! en dessous, la vue d'ensemble du morceau, le panneau de detection, la
//! ligne d'etat.
//!
//! Les signes musicaux qu'on ne trace pas soi-meme (cle de sol, silences,
//! diese) viennent de Segoe UI Symbol (famille 3 du texte).

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Path};
use cp_tab::doc::Tech;
use cp_ui::{Icon, IconCache, Theme};

use crate::{Hit, Lay, Mode, TUNINGS, TabView};

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

impl TabView {
    fn layout(&mut self, w: f32, h: f32, scale: f32) {
        let s = |v: f32| (v * scale).round();
        let bar_h = s(36.0);
        let sub_h = s(26.0);
        let status_h = s(24.0);
        let panel_w = if w > s(760.0) { s(290.0) } else { 0.0 };
        let panel_x = w - panel_w;
        let gutter = s(78.0);
        let top = bar_h + 2.0 + sub_h;
        let bottom = h - status_h - 2.0;
        let ruler_y = top + s(4.0);
        let mut y = ruler_y + s(24.0);
        let gap = s(8.0);
        let score_y = if self.mode != Mode::Tab {
            let sy = y + s(36.0);
            y = sy + 4.0 * gap + s(46.0);
            sy
        } else {
            -1000.0
        };
        let tab_gap = s(12.0);
        let tab_y = if self.mode != Mode::Score {
            let ty = y + s(16.0);
            y = ty + 5.0 * tab_gap + s(22.0);
            ty
        } else {
            -1000.0
        };
        let audio_y = y;
        let audio_h = s(54.0);
        y += audio_h + s(8.0);
        let onset_y = y;
        let onset_h = s(34.0);
        y += onset_h + s(12.0);
        self.lay = Lay {
            w,
            h,
            bar_h,
            sub_h,
            status_h,
            panel_w,
            sx: gutter,
            sw: (panel_x - 2.0 - gutter - s(12.0)).max(10.0),
            top,
            bottom,
            ppq: self.zoom * scale,
            ruler_y,
            score_y,
            tab_y,
            tab_gap,
            audio_y,
            audio_h,
            onset_y,
            onset_h,
            over_y: y,
            over_h: s(26.0),
            panel_x,
        };
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
        self.boxes.clear();
        let l = self.lay;
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_bar(cv, text, icons, th);
        cv.fill_rect(0.0, l.bar_h, w, 2.0, th.seam);
        self.draw_sub(cv, text, th);
        if self.take.is_some() {
            self.draw_staves(cv, text, th);
        } else {
            let f = Font::new(0, th.s(13.0) as u16, 400);
            let msg = self.busy.clone().unwrap_or_else(|| {
                "Select an audio item and press Transcribe, or record a take: it is transcribed when you stop.".into()
            });
            text.draw_in(
                cv,
                f,
                l.sx,
                l.top,
                l.sw,
                (l.bottom - l.top).max(0.0),
                &msg,
                th.text_dim,
                Align::Center,
            );
        }
        if l.panel_w > 0.0 {
            cv.fill_rect(l.panel_x - 2.0, l.bar_h + 2.0, 2.0, h, th.seam);
            self.draw_panel(cv, text, th);
        }
        self.draw_status(cv, text, th);
    }

    fn button(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        x: f32,
        label: &str,
        on: bool,
        hit: Hit,
    ) -> f32 {
        let l = self.lay;
        let f = Font::new(0, th.s(12.0) as u16, if on { 700 } else { 400 });
        let bw = text.measure(f, label).ceil() + th.s(18.0);
        let bh = th.s(24.0);
        let by = ((l.bar_h - bh) * 0.5).floor();
        let bg = if self.hover == hit {
            th.hover.mix(th.text, 0.06)
        } else {
            th.field
        };
        cv.fill_round_rect(x, by, bw, bh, th.s(3.0), bg);
        text.draw_in(
            cv,
            f,
            x,
            by,
            bw,
            bh,
            label,
            if on { th.accent } else { th.text },
            Align::Center,
        );
        self.push(x, by, bw, bh, hit);
        bw
    }

    fn draw_bar(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
    ) {
        let l = self.lay;
        cv.fill_rect(0.0, 0.0, l.w, l.bar_h, th.bar);
        let mut x = th.s(10.0);
        let isz = th.s(18.0) as u16;
        icons.draw(
            cv,
            Icon::Guitar,
            x,
            ((l.bar_h - isz as f32) * 0.5).round(),
            isz,
            th.accent,
            false,
        );
        x += isz as f32 + th.s(6.0);
        let fb = Font::new(0, th.s(13.0) as u16, 700);
        x += text.draw(
            cv,
            fb,
            x,
            (l.bar_h * 0.5 + th.s(4.5)).round(),
            "Guitar Tab",
            th.text,
        ) + th.s(12.0);
        // La prise.
        if let Some(t) = &self.take {
            let f = Font::new(0, th.s(12.0) as u16, 400);
            let fs = Font::new(0, th.s(11.0) as u16, 400);
            let name = t.name.clone();
            let info = format!(
                "{}:{:02} · track \u{201c}{}\u{201d}",
                (t.len_s / 60.0) as i64,
                (t.len_s % 60.0) as i64,
                t.track
            );
            let cw = text.measure(f, &name).min(th.s(260.0)) + text.measure(fs, &info) + th.s(24.0);
            let ch = th.s(24.0);
            let cy = ((l.bar_h - ch) * 0.5).floor();
            cv.fill_round_rect(x, cy, cw, ch, th.s(3.0), th.field);
            let nw = text.measure(f, &name).min(th.s(260.0));
            text.draw_in(
                cv,
                f,
                x + th.s(9.0),
                cy,
                nw,
                ch,
                &name,
                th.text,
                Align::Left,
            );
            text.draw_in(
                cv,
                fs,
                x + th.s(15.0) + nw,
                cy,
                cw,
                ch,
                &info,
                th.text_faint,
                Align::Left,
            );
            x += cw + th.s(8.0);
        }
        x += self.button(cv, text, th, x, "Transcribe", false, Hit::Transcribe) + th.s(14.0);
        for (m, label) in [
            (Mode::Both, "Score + Tab"),
            (Mode::Tab, "Tab"),
            (Mode::Score, "Score"),
        ] {
            x += self.button(cv, text, th, x, label, self.mode == m, Hit::Mode(m)) + th.s(2.0);
        }
        x += th.s(10.0);
        x += self.button(cv, text, th, x, "\u{2212}", false, Hit::ZoomOut) + th.s(2.0);
        x += self.button(cv, text, th, x, "+", false, Hit::ZoomIn) + th.s(10.0);
        let _ = self.button(cv, text, th, x, "Follow playback", self.follow, Hit::Follow);
        // A droite : le lien MIDI et l'export.
        let f = Font::new(0, th.s(12.0) as u16, 400);
        let ew = text.measure(f, "Export MusicXML") + th.s(18.0);
        let ex = l.w - th.s(10.0) - ew;
        let _ = self.button(cv, text, th, ex, "Export MusicXML", false, Hit::Export);
        let link = match self.take.as_ref().and_then(|t| t.midi.clone()) {
            Some(m) => format!("MIDI: {m}"),
            None => "MIDI: not linked".into(),
        };
        let lw = text.measure(f, &link);
        let lx = ex - th.s(16.0) - lw;
        if lx > x + th.s(140.0) {
            let linked = self.take.as_ref().is_some_and(|t| t.midi.is_some());
            cv.fill_circle(
                lx - th.s(8.0),
                l.bar_h * 0.5,
                th.s(3.5),
                if linked { th.accent } else { th.text_faint },
            );
            text.draw_in(cv, f, lx, 0.0, lw, l.bar_h, &link, th.text, Align::Left);
        }
    }

    fn draw_sub(&mut self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let y = l.bar_h + 2.0;
        let w = l.panel_x - if l.panel_w > 0.0 { 2.0 } else { 0.0 };
        cv.fill_rect(0.0, y, w, l.sub_h, th.panel);
        let f = Font::new(0, th.s(11.5) as u16, 400);
        let fb = Font::new(0, th.s(11.5) as u16, 600);
        let left = match &self.take {
            Some(t) => {
                let bq = self.bar_qn();
                let bar = |q: f64| t.first_bar + ((q - t.origin_qn) / bq).floor() as i32;
                let last = bar(t.qn1 - 1e-6).max(t.first_bar);
                let a = bar(self.scroll).clamp(t.first_bar, last);
                let b = bar(self.scroll + self.view_qn()).clamp(a, last);
                (
                    format!("Bars {a}\u{2013}{b}"),
                    format!(
                        " of {last} · {}/{} · {:.0} BPM (project)",
                        t.sig.0, t.sig.1, t.bpm
                    ),
                )
            }
            None => ("No take".into(), String::new()),
        };
        let x = th.s(12.0);
        let aw = text.draw(
            cv,
            fb,
            x,
            y + (l.sub_h * 0.5 + th.s(4.0)).round(),
            &left.0,
            th.text,
        );
        text.draw(
            cv,
            f,
            x + aw,
            y + (l.sub_h * 0.5 + th.s(4.0)).round(),
            &left.1,
            th.text_dim,
        );
        let right = if let Some(b) = &self.busy {
            b.clone()
        } else {
            let gv = self
                .ghosts
                .iter()
                .filter(|g| g.qn0 >= self.scroll && g.qn0 <= self.scroll + self.view_qn())
                .count();
            let mut s = String::new();
            if let Some((a, b)) = self.range {
                s += &format!("passage of {:.1} beats · ", b - a);
            }
            if self.sel.is_some() {
                s += "1 note selected · ";
            }
            if self.detect.ghosts {
                s += &format!("{gv} ghosts in view");
            }
            s.trim_end_matches(" · ").to_string()
        };
        text.draw_in(
            cv,
            f,
            w * 0.5,
            y,
            w * 0.5 - th.s(12.0),
            l.sub_h,
            &right,
            th.text_dim,
            Align::Right,
        );
    }

    fn draw_staves(&mut self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let eng = {
            self.engraved();
            std::mem::take(&mut self.eng)
        };
        let take = self.take.clone().unwrap_or_default();
        let tuning = self.detect.tuning();
        let nstr = tuning.strings.len();
        let paper = th.bg;
        let ink = th.text;
        let staff_c = th.text_dim.fade(0.75);
        let gap = s(8.0);
        let x_end = l.sx + l.sw;
        let area_h = (l.bottom - l.top).max(0.0);

        // --- les cibles de fond (les notes, posees apres, passent devant)
        self.push(l.sx, l.ruler_y, l.sw, s(22.0), Hit::Ruler);
        if self.mode != Mode::Score {
            for k in 0..nstr {
                let y = l.tab_y + k as f32 * l.tab_gap;
                self.push(
                    l.sx,
                    y - l.tab_gap * 0.5,
                    l.sw,
                    l.tab_gap,
                    Hit::TabLine(nstr - 1 - k),
                );
            }
        }
        self.push(
            l.sx,
            l.audio_y,
            l.sw,
            l.onset_y + l.onset_h - l.audio_y,
            Hit::Lanes,
        );
        self.push(l.sx, l.over_y, l.sw, l.over_h, Hit::Overview);

        // --- la marge : cle, TAB, etiquettes (ne defile pas)
        let fs = Font::new(0, s(9.5) as u16, 700);
        let fm = Font::new(0, s(10.0) as u16, 400);
        if self.mode != Mode::Tab {
            for i in 0..5 {
                let y = l.score_y + i as f32 * gap;
                cv.fill_rect(s(12.0), y, l.sx - s(12.0), 1.0, staff_c);
            }
            let fc = Font::new(3, s(44.0) as u16, 400);
            text.draw(
                cv,
                fc,
                s(14.0),
                l.score_y + 4.0 * gap + s(9.0),
                &CLEF.to_string(),
                ink,
            );
            text.draw(
                cv,
                Font::new(0, s(8.5) as u16, 700),
                s(24.0),
                l.score_y + 4.0 * gap + s(21.0),
                "8",
                ink,
            );
        }
        if self.mode != Mode::Score {
            for k in 0..nstr {
                let y = l.tab_y + k as f32 * l.tab_gap;
                cv.fill_rect(s(12.0), y, l.sx - s(12.0), 1.0, staff_c);
            }
            let ft = Font::new(0, s(12.0) as u16, 700);
            for (i, ch) in ["T", "A", "B"].iter().enumerate() {
                text.draw_in(
                    cv,
                    ft,
                    s(16.0),
                    l.tab_y + s(4.0) + i as f32 * s(16.0),
                    s(14.0),
                    s(16.0),
                    ch,
                    ink,
                    Align::Center,
                );
            }
            // L'accordage, corde par corde, a gauche des lignes.
            let names = [
                "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
            ];
            for (k, &open) in tuning.strings.iter().enumerate() {
                let y = l.tab_y + (nstr - 1 - k) as f32 * l.tab_gap;
                let n = names[open.rem_euclid(12) as usize];
                text.draw_in(
                    cv,
                    fm,
                    s(40.0),
                    y - s(6.0),
                    s(30.0),
                    s(12.0),
                    n,
                    th.text_faint,
                    Align::Right,
                );
            }
        }
        text.draw(cv, fs, s(12.0), l.audio_y + s(13.0), "AUDIO", th.text_dim);
        text.draw(cv, fs, s(12.0), l.onset_y + s(13.0), "ONSETS", th.text_dim);
        text.draw(
            cv,
            fm,
            s(12.0),
            l.onset_y + s(25.0),
            "network",
            th.text_faint,
        );
        text.draw(cv, fs, s(12.0), l.over_y + s(13.0), "SONG", th.text_dim);

        // --- ce qui defile
        if !cv.push_clip(l.sx, l.top, l.sw, area_h) {
            cv.pop_clip();
            self.eng = eng;
            return;
        }
        let q0 = self.scroll;
        let q1 = self.scroll + self.view_qn();
        // Le passage choisi.
        if let Some((a, b)) = self.range {
            let (xa, xb) = (self.x_of(a), self.x_of(b));
            cv.fill_rect(
                xa,
                l.ruler_y,
                xb - xa,
                l.onset_y + l.onset_h - l.ruler_y,
                th.accent.with_alpha(22),
            );
            cv.fill_rect(xa, l.ruler_y, xb - xa, s(3.0), th.accent);
        }
        // La regle et les barres de mesure.
        let bq = self.bar_qn();
        let fb = Font::new(0, s(11.0) as u16, 700);
        let ft = Font::new(1, s(9.5) as u16, 400);
        let first = ((q0 - take.origin_qn) / bq).floor().max(0.0) as i64;
        let last = ((q1 - take.origin_qn) / bq).ceil() as i64;
        let secs = |q: f64| (q - take.qn0) * 60.0 / take.bpm.max(1.0);
        for m in first..=last {
            let q = take.origin_qn + m as f64 * bq;
            let x = self.x_of(q).round();
            cv.fill_rect(x, l.ruler_y + s(2.0), 1.0, s(16.0), th.text_faint);
            text.draw(
                cv,
                fb,
                x + s(4.0),
                l.ruler_y + s(13.0),
                &(take.first_bar + m as i32).to_string(),
                ink,
            );
            let t = secs(q).max(0.0);
            text.draw(
                cv,
                ft,
                x + s(26.0),
                l.ruler_y + s(13.0),
                &format!("{}:{:04.1}", (t / 60.0) as i64, t % 60.0),
                th.text_faint,
            );
            for b in 1..take.sig.0.max(1) {
                let bx = self
                    .x_of(q + b as f64 * 4.0 / take.sig.1.max(1) as f64)
                    .round();
                cv.fill_rect(
                    bx,
                    l.ruler_y + s(12.0),
                    1.0,
                    s(6.0),
                    th.text_faint.fade(0.6),
                );
            }
            if self.mode != Mode::Tab {
                cv.fill_rect(x, l.score_y, 1.2, 4.0 * gap, staff_c);
            }
            if self.mode != Mode::Score {
                cv.fill_rect(x, l.tab_y, 1.2, (nstr - 1) as f32 * l.tab_gap, staff_c);
            }
        }
        cv.fill_rect(l.sx, l.ruler_y + s(21.0), l.sw, 1.0, th.seam);
        // Les lignes des portees.
        if self.mode != Mode::Tab {
            for i in 0..5 {
                cv.fill_rect(l.sx, l.score_y + i as f32 * gap, l.sw, 1.0, staff_c);
            }
        }
        if self.mode != Mode::Score {
            for k in 0..nstr {
                cv.fill_rect(l.sx, l.tab_y + k as f32 * l.tab_gap, l.sw, 1.0, staff_c);
            }
        }

        // --- la partition
        let pad = s(9.0);
        let hx = |me: &TabView, cell: i64| me.x_of(eng.qn(cell)) + pad;
        let (rx, ry) = (gap * 0.62, gap * 0.46);
        let y_of = |pos: i32| l.score_y + 4.0 * gap - pos as f32 * gap * 0.5;
        let visible = |x: f32| x > l.sx - s(40.0) && x < x_end + s(40.0);
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
        // Le sens des hampes et le bout des hampes, par groupe (ou signe seul).
        let positions = |si: usize| -> Vec<i32> {
            eng.events[eng.signs[si].ev]
                .notes
                .iter()
                .map(|&n| staff_pos(self.doc[n].pitch).0)
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
        if self.mode != Mode::Tab {
            let fm = Font::new(3, (gap * 4.0) as u16, 400);
            let fsharp = Font::new(3, (gap * 2.6) as u16, 400);
            for (si, sg) in eng.signs.iter().enumerate() {
                let x = hx(self, sg.start);
                if !visible(x) {
                    continue;
                }
                let ev = &eng.events[sg.ev];
                if ev.notes.is_empty() {
                    // Un silence, au milieu de la portee.
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
                        let (p, sh) = staff_pos(self.doc[n].pitch);
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
                // Lignes supplementaires.
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
                    let sel = self.sel == Some(n);
                    let c = if sel { th.accent } else { ink };
                    let hp = head(x + dx, y, rx, ry);
                    if hollow {
                        cv.fill_path(&hp, c);
                        cv.fill_path(&head(x + dx, y, rx * 0.62, ry * 0.42), paper);
                    } else {
                        cv.fill_path(&hp, c);
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
                        // Vers le signe suivant du meme evenement.
                        if let Some(nx) = eng
                            .signs
                            .get(si + 1)
                            .filter(|t| t.ev == sg.ev)
                            .map(|t| hx(self, t.start))
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
                    self.push(
                        x + dx - rx - 2.0,
                        y - ry - 2.0,
                        2.0 * rx + 4.0,
                        2.0 * ry + 4.0,
                        Hit::Note(n),
                    );
                }
                if sg.ty != "whole" {
                    let sx = if up { x + rx - 0.8 } else { x - rx + 0.2 };
                    let from = if up { y_of(lo) } else { y_of(hi) };
                    cv.line(sx, from, sx, end, s(1.15), ink);
                    if beam_group[si].is_some_and(|g| groups[g].len() == 1) {
                        // Un crochet par barre.
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
                // La liaison d'un marteau ou d'un tire : un arc depuis la
                // note d'avant.
                if sg.first && ev.lig_in != Tech::Plain {
                    if let Some(prev_sign) =
                        eng.signs[..si].iter().rev().find(|t| t.ev + 1 == sg.ev)
                    {
                        let px = hx(self, prev_sign.start);
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
                        let x = hx(self, eng.signs[si].start);
                        if up { x + rx - 0.8 } else { x - rx + 0.2 }
                    })
                    .collect();
                if xs.iter().all(|&x| !visible(x)) {
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
                        if let Some(_n) = next {
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
        }

        // --- la tablature
        if self.mode != Mode::Score {
            let fd = Font::new(0, s(11.5) as u16, 600);
            let fl = Font::new(0, s(8.5) as u16, 700);
            for (si, sg) in eng.signs.iter().enumerate() {
                if !sg.first {
                    continue;
                }
                let x = hx(self, sg.start);
                if !visible(x) {
                    continue;
                }
                let ev = &eng.events[sg.ev];
                for &n in &ev.notes {
                    let note = &self.doc[n];
                    let y = l.tab_y + (nstr - 1 - note.string.min(nstr - 1)) as f32 * l.tab_gap;
                    let t = note.fret.to_string();
                    let tw = text.measure(fd, &t).ceil() + s(4.0);
                    let sel = self.sel == Some(n);
                    if sel {
                        cv.fill_round_rect(
                            x - tw * 0.5 - s(1.0),
                            y - s(7.0),
                            tw + s(2.0),
                            s(14.0),
                            s(2.0),
                            th.accent,
                        );
                    } else {
                        cv.fill_rect(x - tw * 0.5, y - s(5.5), tw, s(11.0), paper);
                    }
                    let c = if sel {
                        th.bg
                    } else if note.locked {
                        th.text
                    } else {
                        ink
                    };
                    text.draw_in(
                        cv,
                        fd,
                        x - tw * 0.5,
                        y - s(7.0),
                        tw,
                        s(14.0),
                        &t,
                        c,
                        Align::Center,
                    );
                    self.push(
                        x - tw * 0.5 - s(2.0),
                        y - s(7.0),
                        tw + s(4.0),
                        s(14.0),
                        Hit::Note(n),
                    );
                }
                // « H » / « P » : un arc depuis la note d'avant.
                if ev.lig_in != Tech::Plain && ev.notes.len() == 1 {
                    if let Some(prev_sign) =
                        eng.signs[..si].iter().rev().find(|t| t.ev + 1 == sg.ev)
                    {
                        let px = hx(self, prev_sign.start);
                        let s0 = self.doc[ev.notes[0]].string;
                        let y = l.tab_y + (nstr - 1 - s0.min(nstr - 1)) as f32 * l.tab_gap - s(8.0);
                        arc(cv, px, y, x, y, -s(4.0), 1.1, th.text_dim);
                        let lab = if ev.lig_in == Tech::Hammer { "H" } else { "P" };
                        text.draw_in(
                            cv,
                            fl,
                            (px + x) * 0.5 - s(6.0),
                            y - s(16.0),
                            s(12.0),
                            s(10.0),
                            lab,
                            th.text_dim,
                            Align::Center,
                        );
                    }
                }
            }
        }

        // --- les notes fantomes : en pointilles, gardees d'un clic
        if self.detect.ghosts {
            let fd = Font::new(0, s(11.5) as u16, 600);
            let ghost_c = th.text_faint;
            let g = eng.grid.max(1) as f64;
            for gi in 0..self.ghosts.len() {
                let gn = self.ghosts[gi].clone();
                let cell = ((gn.qn0 - eng.origin_qn) * g).round();
                let x = self.x_of(eng.origin_qn + cell / g) + pad;
                if !visible(x) {
                    continue;
                }
                let hov = self.hover == Hit::Ghost(gi);
                let c = if hov { th.accent } else { ghost_c };
                if self.mode != Mode::Score {
                    let y = l.tab_y + (nstr - 1 - gn.string.min(nstr - 1)) as f32 * l.tab_gap;
                    let t = gn.fret.to_string();
                    let tw = text.measure(fd, &t).ceil() + s(4.0);
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
                        &t,
                        c,
                        Align::Center,
                    );
                    self.push(
                        x - tw * 0.5 - s(2.0),
                        y - s(7.0),
                        tw + s(4.0),
                        s(14.0),
                        Hit::Ghost(gi),
                    );
                }
                if self.mode != Mode::Tab {
                    let (pos, _) = staff_pos(gn.pitch);
                    let y = y_of(pos);
                    // Une tete creuse en pointilles.
                    let n = 14;
                    for k in 0..n {
                        if k % 2 == 1 {
                            continue;
                        }
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
                    self.push(
                        x - rx - 2.0,
                        y - ry - 2.0,
                        2.0 * rx + 4.0,
                        2.0 * ry + 4.0,
                        Hit::Ghost(gi),
                    );
                }
            }
        }

        // --- le son et les attaques
        cv.fill_rect(l.sx, l.audio_y, l.sw, l.audio_h, th.field);
        cv.fill_rect(l.sx, l.onset_y, l.sw, l.onset_h, th.field);
        if !take.frame_qn.is_empty() {
            let mid = l.audio_y + l.audio_h * 0.5;
            let half = l.audio_h * 0.5 - s(3.0);
            let base = l.onset_y + l.onset_h - s(3.0);
            let span = l.onset_h - s(6.0);
            let wave_c = th.text_dim.fade(0.85);
            let onset_c = th.accent.mix(th.text_dim, 0.4);
            let n = take.frame_qn.len();
            let col_w = 1.0f32;
            let mut prev_on: Option<(f32, f32)> = None;
            let mut x = l.sx;
            while x < x_end {
                let qa = self.qn_of(x);
                let qb = self.qn_of(x + col_w);
                let fa = take.frame_qn.partition_point(|&q| q < qa);
                let fb = take
                    .frame_qn
                    .partition_point(|&q| q < qb)
                    .max(fa + 1)
                    .min(n);
                if fa < n {
                    let e = take.env[fa..fb].iter().fold(0f32, |m, &v| m.max(v));
                    let o = take.onset[fa..fb].iter().fold(0f32, |m, &v| m.max(v));
                    let hh = (e * half).max(0.5);
                    cv.fill_rect(x, mid - hh, col_w, 2.0 * hh, wave_c);
                    let oy = base - o * span;
                    if let Some((px, py)) = prev_on {
                        cv.line(px, py, x, oy, 1.1, onset_c);
                    }
                    prev_on = Some((x, oy));
                }
                x += col_w;
            }
            // Les seuils : les notes gardees, et les fantomes.
            let ty = base - self.detect.onset * span;
            dashed(cv, l.sx, ty, x_end, ty, s(4.0), s(3.0), 1.0, th.text_dim);
            if self.detect.ghosts {
                let gy = base - self.detect.ghost_onset() * span;
                dashed(cv, l.sx, gy, x_end, gy, s(1.5), s(3.0), 1.0, th.text_faint);
            }
        }

        // --- la tete de lecture
        if let Some(p) = self.playhead {
            let x = self.x_of(p).round();
            if visible(x) {
                cv.fill_rect(
                    x,
                    l.ruler_y,
                    s(1.5),
                    l.onset_y + l.onset_h - l.ruler_y,
                    th.playhead,
                );
                let mut tri = Path::new();
                tri.move_to(x - s(5.0), l.ruler_y)
                    .line_to(x + s(6.5), l.ruler_y)
                    .line_to(x + s(0.75), l.ruler_y + s(6.0))
                    .close();
                cv.fill_path(&tri, th.playhead);
            }
        }
        cv.pop_clip();

        // --- la vue d'ensemble : toutes les mesures, la page visible
        let total_bars = (((take.qn1 - take.origin_qn) / bq).ceil() as usize).max(1);
        let bw = l.sw / total_bars as f32;
        let mut counts = vec![0usize; total_bars];
        for n in &self.doc {
            let m = ((n.qn0 - take.origin_qn) / bq).floor();
            if m >= 0.0 && (m as usize) < total_bars {
                counts[m as usize] += 1;
            }
        }
        let maxc = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
        for (m, &c) in counts.iter().enumerate() {
            let hh = s(4.0) + (c as f32 / maxc) * (l.over_h - s(6.0));
            let x = l.sx + m as f32 * bw;
            let q = take.origin_qn + m as f64 * bq;
            let inside = q + bq > q0 && q < q1;
            let col = if inside {
                th.accent.mix(th.text_dim, 0.35)
            } else {
                th.text_faint.fade(0.6)
            };
            cv.fill_round_rect(
                x + 0.5,
                l.over_y + l.over_h - hh,
                (bw - 1.0).max(1.0),
                hh,
                1.0,
                col,
            );
        }
        let span = (take.qn1 - take.origin_qn).max(1.0);
        let vx0 = l.sx + ((q0 - take.origin_qn) / span) as f32 * l.sw;
        let vx1 = l.sx + ((q1 - take.origin_qn) / span) as f32 * l.sw;
        cv.stroke_rect(
            vx0.max(l.sx),
            l.over_y - 1.0,
            (vx1.min(x_end) - vx0.max(l.sx)).max(2.0),
            l.over_h + 2.0,
            1.2,
            th.accent,
        );
        self.eng = eng;
    }

    fn draw_panel(&mut self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let x0 = l.panel_x;
        let w = l.panel_w;
        cv.fill_rect(
            x0,
            l.bar_h + 2.0,
            w,
            l.h - l.bar_h - 2.0 - l.status_h - 2.0,
            th.panel,
        );
        let fh = Font::new(0, s(10.5) as u16, 700);
        let f = Font::new(0, s(12.0) as u16, 400);
        let fs = Font::new(0, s(10.5) as u16, 400);
        let fv = Font::new(1, s(11.0) as u16, 400);
        let pad = s(12.0);
        let mut y = l.bar_h + 2.0 + s(12.0);
        text.draw(cv, fh, x0 + pad, y + s(9.0), "DETECTION", th.text_dim);
        let sub = if self.range.is_some() {
            "applies to the passage"
        } else {
            "applies live"
        };
        text.draw(cv, fs, x0 + pad + s(80.0), y + s(9.0), sub, th.text_faint);
        y += s(26.0);
        let section = |text: &mut TextCache, cv: &mut Canvas<'_>, y: f32, t: &str| {
            text.draw(
                cv,
                Font::new(0, s(9.5) as u16, 700),
                x0 + pad,
                y + s(8.0),
                t,
                th.text_faint,
            );
        };
        section(text, cv, y, "NOTES");
        y += s(18.0);
        let tx0 = x0 + s(120.0);
        let tw = (l.w - tx0 - s(64.0)).max(10.0);
        let sliders: [(u8, &str, f32, String); 3] = [
            (
                0,
                "Onset sensitivity",
                (self.detect.onset - 0.05) / 0.9,
                format!("{:.2}", self.detect.onset),
            ),
            (
                1,
                "Note sensitivity",
                (self.detect.frame - 0.05) / 0.9,
                format!("{:.2}", self.detect.frame),
            ),
            (
                2,
                "Shortest note",
                ((self.detect.min_ms - 20.0) / 380.0) as f32,
                format!("{:.0} ms", self.detect.min_ms),
            ),
        ];
        for (k, label, t, val) in sliders {
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
            if k == 0 {
                // Le seuil d'origine (0,50), pour s'y retrouver.
                let ox = tx0 + (0.5 - 0.05) / 0.9 * tw;
                cv.fill_rect(ox, cy - s(5.0), 1.0, s(10.0), th.text_faint);
            }
            let hot = self.hover == Hit::Slider(k);
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
                &val,
                th.text,
                Align::Right,
            );
            self.push(tx0 - s(6.0), y, tw + s(12.0), rh, Hit::Slider(k));
            y += rh + s(2.0);
            if k == 0 {
                text.draw(
                    cv,
                    fs,
                    x0 + pad,
                    y + s(9.0),
                    "Lower = more notes caught, and more noise.",
                    th.text_faint,
                );
                y += s(16.0);
            }
        }
        for (k, label, on) in [
            (0u8, "Legato notes (no attack)", self.detect.legato),
            (
                1u8,
                "Show ghosts (just under the threshold)",
                self.detect.ghosts,
            ),
        ] {
            let bx = x0 + pad;
            let bs = s(13.0);
            let by = y + s(4.0);
            cv.fill_round_rect(bx, by, bs, bs, s(2.0), th.field);
            if on {
                text.draw_in(
                    cv,
                    Font::new(0, s(11.0) as u16, 700),
                    bx,
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
                f,
                bx + bs + s(7.0),
                y,
                w - pad * 2.0 - bs,
                s(21.0),
                label,
                th.text,
                Align::Left,
            );
            self.push(bx, y, w - pad * 2.0, s(21.0), Hit::Check(k));
            y += s(22.0);
        }
        y += s(8.0);
        section(text, cv, y, "RHYTHM");
        y += s(18.0);
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
            let on = self.detect.grid == g;
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
            self.push(gx, y, gw, s(24.0), Hit::Grid(g));
        }
        y += s(32.0);
        section(text, cv, y, "FINGERING");
        y += s(18.0);
        text.draw_in(
            cv,
            f,
            x0 + pad,
            y,
            s(80.0),
            s(24.0),
            "Tuning",
            th.text,
            Align::Left,
        );
        let tname = format!(
            "{} \u{25be}",
            TUNINGS[self.detect.tuning.min(TUNINGS.len() - 1)].0
        );
        let tnw = text.measure(f, &tname) + s(18.0);
        let tnx = l.w - pad - tnw;
        cv.fill_round_rect(
            tnx,
            y + s(1.0),
            tnw,
            s(22.0),
            s(3.0),
            if self.hover == Hit::Tuning {
                th.hover
            } else {
                th.field
            },
        );
        text.draw_in(
            cv,
            f,
            tnx,
            y + s(1.0),
            tnw,
            s(22.0),
            &tname,
            th.text,
            Align::Center,
        );
        self.push(tnx, y, tnw, s(24.0), Hit::Tuning);
        y += s(34.0);
        // Les comptes.
        let chords = {
            self.engraved();
            self.eng.chords
        };
        let stats = [
            ("Notes", self.doc.len().to_string()),
            ("Chords", chords.to_string()),
            (
                "Ghosts",
                if self.detect.ghosts {
                    self.ghosts.len().to_string()
                } else {
                    "off".into()
                },
            ),
            ("Unplayable", self.dropped.to_string()),
        ];
        let bh = s(46.0);
        cv.fill_round_rect(x0 + pad, y, w - 2.0 * pad, bh, s(4.0), th.field);
        let cw = (w - 2.0 * pad) * 0.5;
        for (i, (k, v)) in stats.iter().enumerate() {
            let cx = x0 + pad + (i % 2) as f32 * cw + s(8.0);
            let cy = y + s(4.0) + (i / 2) as f32 * s(19.0);
            text.draw_in(
                cv,
                fs,
                cx,
                cy,
                cw - s(16.0),
                s(18.0),
                k,
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
        let kw = text.measure(f, keep) + s(18.0);
        let hov = self.hover == Hit::KeepGhosts;
        cv.fill_round_rect(
            x0 + pad,
            y,
            kw,
            s(24.0),
            s(3.0),
            if hov { th.hover } else { th.field },
        );
        text.draw_in(
            cv,
            f,
            x0 + pad,
            y,
            kw,
            s(24.0),
            keep,
            th.text,
            Align::Center,
        );
        self.push(x0 + pad, y, kw, s(24.0), Hit::KeepGhosts);
        let rt = "Transcribe again";
        let rw = text.measure(f, rt) + s(18.0);
        if x0 + pad + kw + s(6.0) + rw < l.w - pad {
            let rx = x0 + pad + kw + s(6.0);
            cv.fill_round_rect(
                rx,
                y,
                rw,
                s(24.0),
                s(3.0),
                if self.hover == Hit::Retranscribe {
                    th.hover
                } else {
                    th.field
                },
            );
            text.draw_in(cv, f, rx, y, rw, s(24.0), rt, th.text, Align::Center);
            self.push(rx, y, rw, s(24.0), Hit::Retranscribe);
        }
        y += s(36.0);
        cv.fill_rect(x0 + pad, y, w - 2.0 * pad, 1.0, th.seam);
        y += s(8.0);
        let note = if self.net_note.is_empty() {
            "The network runs once per take. These settings re-read its output.".to_string()
        } else {
            self.net_note.clone()
        };
        // Deux lignes au plus.
        let words: Vec<&str> = note.split(' ').collect();
        let mut line = String::new();
        let mut lines = Vec::new();
        for wd in words {
            let t = if line.is_empty() {
                wd.to_string()
            } else {
                format!("{line} {wd}")
            };
            if text.measure(fs, &t) > w - 2.0 * pad && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = wd.to_string();
            } else {
                line = t;
            }
        }
        lines.push(line);
        for (i, ln) in lines.iter().take(3).enumerate() {
            text.draw(
                cv,
                fs,
                x0 + pad,
                y + s(10.0) + i as f32 * s(14.0),
                ln,
                th.text_faint,
            );
        }
    }

    fn draw_status(&mut self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme) {
        let l = self.lay;
        let y = l.h - l.status_h;
        cv.fill_rect(0.0, y - 2.0, l.w, 2.0, th.seam);
        cv.fill_rect(0.0, y, l.w, l.status_h, th.bar);
        let f = Font::new(0, th.s(11.0) as u16, 400);
        let fm = Font::new(1, th.s(10.5) as u16, 400);
        let left = self.sel_text().unwrap_or_else(|| self.status.clone());
        let lw = text.measure(f, &left).min(l.w * 0.45);
        text.draw_in(
            cv,
            f,
            th.s(10.0),
            y,
            lw,
            l.status_h,
            &left,
            th.text,
            Align::Left,
        );
        let keys = "0\u{2013}9 fret · \u{2191}\u{2193} string · \u{2190}\u{2192} note · H P · Del · double-click: add";
        let kx = th.s(24.0) + lw;
        let right_w = th.s(170.0);
        if l.w - right_w - kx > th.s(80.0) {
            text.draw_in(
                cv,
                fm,
                kx,
                y,
                l.w - right_w - kx,
                l.status_h,
                keys,
                th.text_faint,
                Align::Left,
            );
        }
        let (dot, label) = match self.take.as_ref().map(|t| t.midi.is_some()) {
            Some(true) => (th.accent, "MIDI in sync"),
            Some(false) => (th.text_faint, "no MIDI item"),
            None => (th.text_faint, ""),
        };
        if !label.is_empty() {
            let tw = text.measure(f, label);
            let x = l.w - th.s(10.0) - tw;
            cv.fill_circle(x - th.s(8.0), y + l.status_h * 0.5, th.s(3.5), dot);
            text.draw_in(
                cv,
                f,
                x,
                y,
                tw,
                l.status_h,
                label,
                if dot == th.accent {
                    th.accent
                } else {
                    th.text_dim
                },
                Align::Left,
            );
        }
    }
}

/// Pour les tests et l'auto-test : la position d'une note sur l'ecran.
pub fn note_rect(v: &TabView, i: usize) -> Option<(f32, f32, f32, f32)> {
    v.boxes
        .iter()
        .rev()
        .find(|b| b.4 == Hit::Note(i))
        .map(|b| (b.0, b.1, b.2, b.3))
}

pub fn ghost_rect(v: &TabView, i: usize) -> Option<(f32, f32, f32, f32)> {
    v.boxes
        .iter()
        .rev()
        .find(|b| b.4 == Hit::Ghost(i))
        .map(|b| (b.0, b.1, b.2, b.3))
}
