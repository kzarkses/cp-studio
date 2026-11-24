//! Le dessin de la Session.

use cp_gfx::text::{Align, TextCache};
use cp_gfx::{Canvas, Color, hex, rgb};
use cp_session::{CellState, ClockMode, Kind, Session};
use cp_ui::{Icon, IconCache, Theme};

use crate::{Hit, Layout, Part, Strip, Tool, View, db_of};

/// Les couleurs d'etat de la Session.
pub struct Ink;

impl Ink {
    pub const PLAY: u32 = 0x5BD07A;
    pub const REC: u32 = 0xE5534B;
    pub const QUEUE: u32 = 0xF0B44C;
    pub const DUB: u32 = 0xE58A4B;
}

fn tint(base: Color, c: Color, k: f32) -> Color {
    base.mix(c, k)
}

impl View {
    /// Dessine toute la vue. `now` : le frame du moteur (la barre de passe) ;
    /// `t` : des secondes qui avancent (les clignotements) ; `msg` : le
    /// message bref de la barre d'etat.
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        strips: &[Strip],
        now: i64,
        t: f64,
        msg: Option<&str>,
        w: f32,
        h: f32,
    ) {
        self.size = (w, h);
        self.scale = th.scale;
        self.set_strips(strips);
        let l = self.layout(th, w, h);
        self.clamp_scroll(ses, &l);
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_toolbar(cv, text, icons, th, ses, &l, t);
        self.draw_headers(cv, text, th, ses, &l);
        self.draw_grid(cv, text, icons, th, ses, &l, now, t);
        self.draw_stops(cv, icons, th, ses, &l);
        self.draw_scroll(cv, th, ses, &l);
        cv.fill_rect(0.0, l.seam_y, w, l.mixer_y - l.seam_y, th.seam);
        if l.mixer_h > 0.0 {
            self.draw_mixer(cv, text, icons, th, ses, strips, &l);
        }
        self.draw_status(cv, text, th, ses, &l, msg);
        // L'onglet de la vue ancree, par-dessus le pied de la ligne d'etat.
        if self.dock_handle {
            let hot = self.hover == Hit::DockHandle || self.grabbing_handle();
            cp_ui::handle::draw_tab(cv, text, th, self.tab_rect(), "Session", hot);
        }
        self.draw_ghost(cv, text, th, ses, &l);
    }

    fn draw_toolbar(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
        t: f64,
    ) {
        let s = l.s;
        cv.fill_rect(0.0, 0.0, l.w, l.header_y, th.bar);
        cv.fill_rect(0.0, l.header_y - 1.0, l.w, 1.0, th.seam);
        let by = (3.0 * s).round();
        let bh = l.header_y - 2.0 * by;
        let r = (4.0 * s).round();
        let isz = (14.0 * s).round() as u16;
        let font = th.font_small();
        let any_rec = ses.cols.iter().any(|c| c.run.rec.is_some());
        let any_dub = ses.cols.iter().any(|c| c.run.dub.is_some());
        for (tool, x, bw) in self.tools(l) {
            let hov = self.hover == Hit::Tool(tool);
            let on = match tool {
                Tool::Clock => ses.clock == ClockMode::Follow,
                Tool::Overdub => any_dub,
                Tool::ArrangeRec => ses.arr_rec,
                _ => false,
            };
            let bg = if on {
                th.sel
            } else if hov {
                th.hover
            } else {
                th.bar
            };
            cv.fill_round_rect(x, by, bw, bh, r, bg);
            let ic = |c: Color| c;
            let fg = if on { th.text } else { th.text_dim };
            match tool {
                Tool::Clock => {
                    let label = match ses.clock {
                        ClockMode::Free => "Free",
                        ClockMode::Follow => "Follow",
                    };
                    text.draw_in(cv, font, x, by, bw, bh, label, fg, Align::Center);
                }
                Tool::Quant => {
                    text.draw_in(
                        cv,
                        font,
                        x,
                        by,
                        bw,
                        bh,
                        &format!("Q {}", ses.q.label()),
                        fg,
                        Align::Center,
                    );
                }
                Tool::Rec => {
                    text.draw_in(
                        cv,
                        font,
                        x,
                        by,
                        bw,
                        bh,
                        &format!("Rec {}", ses.rec_bars),
                        fg,
                        Align::Center,
                    );
                }
                _ => {
                    let (icon, c) = match tool {
                        Tool::Overdub => {
                            (Icon::CirclePlus, if any_dub { hex(Ink::DUB) } else { fg })
                        }
                        Tool::StopAll => (Icon::Stop, fg),
                        Tool::Panic => (Icon::Zap, fg),
                        Tool::Capture => (Icon::Camera, fg),
                        Tool::ArrangeRec => (
                            Icon::Circle,
                            if any_rec && (t * 2.0).fract() < 0.5 {
                                hex(Ink::REC)
                            } else {
                                fg
                            },
                        ),
                        Tool::BackToArrange => (Icon::Reverse, fg),
                        _ => (Icon::Help, fg),
                    };
                    let ix = x + (bw - isz as f32) * 0.5;
                    let iy = by + (bh - isz as f32) * 0.5;
                    icons.draw(cv, icon, ix, iy, isz, ic(c), false);
                }
            }
        }
        // Le tempo, a droite des boutons.
        let tx = self
            .tools(l)
            .iter()
            .find(|t| t.0 == Tool::BackToArrange)
            .map_or(0.0, |t| t.1 + t.2 + (10.0 * s).round());
        let bpm = format!("{:.1} BPM", ses.bpm);
        text.draw_in(
            cv,
            th.font_small(),
            tx,
            by,
            (90.0 * s).round(),
            bh,
            &bpm,
            th.text_faint,
            Align::Left,
        );
    }

    fn draw_headers(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
    ) {
        let s = l.s;
        let y = l.header_y;
        let hh = l.grid_y - l.header_y;
        cv.fill_rect(0.0, y, l.w, hh, th.bg_alt);
        for (i, c) in ses.cols.iter().enumerate() {
            let Some(x) = self.col_x(l, i) else { continue };
            if x > l.w {
                break;
            }
            let sel = ses.sel.0 == i;
            let hov = self.hover == Hit::Header(i);
            let bg = if sel {
                th.sel_dim
            } else if hov {
                th.hover
            } else {
                th.panel
            };
            cv.fill_rect(x, y + 1.0, l.col_w, hh - 2.0, bg);
            // La couleur de la piste, un filet en haut.
            if let Some((r, g, b)) = c.track.color {
                cv.fill_rect(x, y + 1.0, l.col_w, (3.0 * s).round(), rgb(r, g, b));
            }
            let name = if c.held { "—" } else { c.track.name.as_str() };
            let pad = (5.0 * s).round();
            let r = (5.0 * s).round();
            let tag = c.motion.tag();
            let tag_w = if tag.is_empty() || l.col_w < 42.0 * s {
                0.0
            } else {
                text.measure(th.font_small(), tag) + pad
            };
            let room = l.col_w - 2.0 * pad - 2.0 * r - pad - tag_w;
            let fg = if c.held { th.text_faint } else { th.text };
            text.draw_in(
                cv,
                th.font_small(),
                x + pad,
                y + 2.0,
                room,
                hh - 2.0,
                name,
                fg,
                Align::Left,
            );
            if tag_w > 0.0 {
                text.draw_in(
                    cv,
                    th.font_small(),
                    x + pad + room,
                    y + 2.0,
                    tag_w,
                    hh - 2.0,
                    tag,
                    th.text_dim,
                    Align::Left,
                );
            }
            // L'armement.
            if !c.held {
                let cx = x + l.col_w - pad - r;
                let cy = y + hh * 0.5 + 1.0;
                if c.track.armed {
                    cv.fill_circle(cx, cy, r, hex(Ink::REC));
                } else {
                    let hv = self.hover == Hit::Arm(i);
                    cv.stroke_circle(
                        cx,
                        cy,
                        r - 0.5,
                        1.0,
                        if hv { th.text } else { th.text_faint },
                    );
                }
            }
        }
    }

    fn draw_grid(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
        now: i64,
        t: f64,
    ) {
        let s = l.s;
        cv.push_clip(0.0, l.grid_y, l.w, l.grid_h);
        let font = th.font_small();
        for k in 0..=l.rows_vis {
            let scene = self.scroll_row + k;
            if scene >= ses.scenes {
                break;
            }
            let y = l.grid_y + k as f32 * l.cell_h;
            // La gouttiere : le triangle de la scene et son numero.
            let hov = self.hover == Hit::Scene(scene);
            let any = (0..ses.cols.len()).any(|c| ses.clip(c, scene).is_some());
            let fg = if hov {
                th.text
            } else if any {
                th.text_dim
            } else {
                th.text_faint
            };
            let isz = (12.0 * s).round() as u16;
            icons.draw(
                cv,
                Icon::Play,
                (4.0 * s).round(),
                y + (l.cell_h - isz as f32) * 0.5,
                isz,
                fg,
                any,
            );
            text.draw_in(
                cv,
                font,
                (16.0 * s).round(),
                y,
                l.gutter - (17.0 * s).round(),
                l.cell_h,
                &format!("{}", scene + 1),
                th.text_faint,
                Align::Left,
            );
            for (i, _) in ses.cols.iter().enumerate() {
                let Some(x) = self.col_x(l, i) else { continue };
                if x > l.w {
                    break;
                }
                self.draw_cell(
                    cv,
                    text,
                    icons,
                    th,
                    ses,
                    l,
                    i,
                    scene,
                    x,
                    y + 1.0,
                    l.col_w,
                    l.cell_h - 2.0,
                    now,
                    t,
                );
            }
        }
        cv.pop_clip();
    }

    fn draw_cell(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
        col: usize,
        scene: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        now: i64,
        t: f64,
    ) {
        let s = l.s;
        let st = ses.state(col, scene);
        let clip = ses.clip(col, scene);
        let tc = ses
            .cols
            .get(col)
            .and_then(|c| c.track.color)
            .map(|(r, g, b)| rgb(r, g, b));
        let filled = clip.is_some();
        let ground = if filled { th.panel } else { th.bg_alt };
        let mut bg = ground;
        if let Some(c) = clip
            .and_then(|c| c.rgb())
            .map(|(r, g, b)| rgb(r, g, b))
            .or(if filled { tc } else { None })
        {
            bg = tint(bg, c, 0.30);
        }
        let blink = (t * 1.59 * 2.0).fract() < 0.5;
        bg = match st {
            CellState::Playing => tint(bg, hex(Ink::PLAY), 0.34),
            CellState::Overdub => tint(bg, hex(Ink::DUB), 0.40),
            CellState::Recording => tint(bg, hex(Ink::REC), 0.42),
            CellState::Queued | CellState::Waiting | CellState::RecQueued | CellState::Stopping => {
                tint(bg, hex(Ink::QUEUE), 0.18)
            }
            _ => bg,
        };
        let sel = ses.sel == (col, scene);
        let hov_cell =
            matches!(self.hover, Hit::Cell { col: c, scene: sc, .. } if c == col && sc == scene);
        if hov_cell {
            bg = bg.mix(th.text, 0.05);
        }
        let r = (3.0 * s).round();
        cv.fill_round_rect(x, y, w, h, r, bg);
        if sel {
            cv.stroke_round_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r, 1.0, th.text_dim);
        }
        // Un lancement qui attend : un contour qui clignote.
        if matches!(
            st,
            CellState::Queued | CellState::Waiting | CellState::RecQueued | CellState::Stopping
        ) && blink
        {
            let c = if st == CellState::RecQueued {
                hex(Ink::REC)
            } else {
                hex(Ink::QUEUE)
            };
            cv.stroke_round_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r, 1.0, c);
        }
        // La bande gauche : l'icone du geste.
        let sw = (crate::geo::STRIP_W * s).round();
        let hov_strip = matches!(self.hover, Hit::Cell { col: c, scene: sc, strip: true } if c == col && sc == scene);
        if hov_strip {
            cv.fill_rect(x, y, sw, h, bg.mix(th.text, 0.10));
        }
        let isz = (11.0 * s).round() as u16;
        let ix = x + (sw - isz as f32) * 0.5;
        let iy = y + (h - isz as f32) * 0.5;
        let armed = ses.cols.get(col).is_some_and(|c| c.track.armed);
        match st {
            CellState::Empty => {
                if armed {
                    cv.stroke_circle(
                        ix + isz as f32 * 0.5,
                        iy + isz as f32 * 0.5,
                        isz as f32 * 0.4,
                        1.0,
                        hex(Ink::REC).mix(bg, 0.4),
                    );
                }
            }
            CellState::Recording => {
                let a = 0.5 + 0.5 * (t * 5.0).sin() as f32;
                cv.fill_circle(
                    ix + isz as f32 * 0.5,
                    iy + isz as f32 * 0.5,
                    isz as f32 * 0.42,
                    hex(Ink::REC).mix(bg, 0.5 - 0.5 * a),
                );
            }
            CellState::RecQueued => {
                let a = 0.5 + 0.5 * (t * 2.0).sin() as f32;
                cv.fill_circle(
                    ix + isz as f32 * 0.5,
                    iy + isz as f32 * 0.5,
                    isz as f32 * 0.38,
                    hex(Ink::REC).mix(bg, 0.7 - 0.4 * a),
                );
            }
            CellState::Playing | CellState::Overdub | CellState::Stopping => {
                icons.draw(cv, Icon::Play, ix, iy, isz, hex(Ink::PLAY), true);
            }
            CellState::Queued | CellState::Waiting => {
                icons.draw(cv, Icon::Play, ix, iy, isz, hex(Ink::QUEUE), blink);
            }
            CellState::Stopped => {
                icons.draw(cv, Icon::Play, ix, iy, isz, th.text_dim, false);
            }
        }
        let Some(c) = clip else { return };
        // Le nom et la ligne du dessous.
        let tx = x + sw;
        let tw = w - sw - (4.0 * s).round();
        let name = if c.name.is_empty() {
            match &c.kind {
                Kind::Midi(_) => "MIDI".to_string(),
                Kind::Audio(a) => std::path::Path::new(&a.path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }
        } else {
            c.name.clone()
        };
        let two = h >= 26.0 * s;
        let f = th.font_small();
        if two {
            let lh = h * 0.5;
            text.draw_in(cv, f, tx, y + 1.0, tw, lh, &name, th.text, Align::Left);
            let sub = match &c.kind {
                Kind::Midi(n) => {
                    format!("{} · {}", cp_session::clip::length_label(c.bars), n.len())
                }
                Kind::Audio(a) => {
                    let b = a.src_bpm.map(|b| format!("{b:.0}")).unwrap_or_default();
                    format!("{} · {b}", cp_session::clip::length_label(c.bars))
                }
            };
            text.draw_in(
                cv,
                f,
                tx,
                y + lh - 2.0,
                tw,
                lh,
                &sub,
                th.text_faint,
                Align::Left,
            );
        } else {
            text.draw_in(cv, f, tx, y, tw, h, &name, th.text, Align::Left);
        }
        // Les passes jouees, a droite.
        let run = &ses.cols[col].run;
        if matches!(st, CellState::Playing | CellState::Overdub) && run.passes > 0 && w > 70.0 * s {
            let p = format!("x{}", run.passes + 1);
            text.draw_in(
                cv,
                f,
                x,
                y + 1.0,
                w - (4.0 * s).round(),
                h * 0.5,
                &p,
                th.text_dim,
                Align::Right,
            );
        }
        // La barre de passe.
        if matches!(
            st,
            CellState::Playing | CellState::Overdub | CellState::Stopping
        ) {
            if let Some((k, _)) = ses.progress(col, now) {
                let bh = (2.0 * s).round().max(1.0);
                let by = y + h - bh - 1.0;
                cv.fill_rect(tx, by, tw, bh, bg.mix(th.text, 0.12));
                cv.fill_rect(tx, by, (tw * k as f32).round(), bh, hex(Ink::PLAY));
            }
        }
        if st == CellState::Recording {
            if let Some(tk) = &run.rec {
                let k = ((now - tk.start_f) as f64 / (tk.end_f - tk.start_f).max(1) as f64)
                    .clamp(0.0, 1.0);
                let bh = (2.0 * s).round().max(1.0);
                cv.fill_rect(
                    tx,
                    y + h - bh - 1.0,
                    (tw * k as f32).round(),
                    bh,
                    hex(Ink::REC),
                );
            }
        }
    }

    fn draw_stops(
        &self,
        cv: &mut Canvas<'_>,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
    ) {
        let s = l.s;
        let y = l.stop_y;
        let hh = l.scroll_y - y;
        cv.fill_rect(0.0, y, l.w, hh, th.bg_alt);
        let isz = (10.0 * s).round() as u16;
        let iy = y + (hh - isz as f32) * 0.5;
        let hv = self.hover == Hit::StopAll;
        icons.draw(
            cv,
            Icon::Stop,
            (8.0 * s).round(),
            iy,
            isz,
            if hv { th.text } else { th.text_dim },
            true,
        );
        for (i, col) in ses.cols.iter().enumerate() {
            let Some(x) = self.col_x(l, i) else { continue };
            if x > l.w {
                break;
            }
            let playing = col.run.play.is_some();
            let hv = self.hover == Hit::StopCol(i);
            let c = if hv {
                th.text
            } else if playing {
                th.text_dim
            } else {
                th.text_faint
            };
            icons.draw(
                cv,
                Icon::Stop,
                x + (l.col_w - isz as f32) * 0.5,
                iy,
                isz,
                c,
                playing,
            );
        }
    }

    fn draw_scroll(&self, cv: &mut Canvas<'_>, th: &Theme, ses: &Session, l: &Layout) {
        let cols = ses.cols.len() + 1;
        let y = l.scroll_y;
        let hh = l.seam_y - y;
        cv.fill_rect(0.0, y, l.w, hh, th.bg);
        if cols <= l.cols_vis {
            return;
        }
        let track = l.w - l.gutter;
        let k = l.cols_vis as f32 / cols as f32;
        let tw = (track * k).max(20.0 * l.s);
        let tx = l.gutter + (track - tw) * (self.scroll_col as f32 / (cols - l.cols_vis) as f32);
        let r = (hh * 0.5 - 1.0).max(1.0);
        cv.fill_round_rect(tx, y + 2.0, tw, hh - 4.0, r, th.text_faint);
    }

    fn draw_mixer(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        ses: &Session,
        strips: &[Strip],
        l: &Layout,
    ) {
        let s = l.s;
        cv.push_clip(0.0, l.mixer_y, l.w, l.mixer_h);
        cv.fill_rect(0.0, l.mixer_y, l.w, l.mixer_h, th.bg_alt);
        let f = th.font_small();
        for (i, c) in ses.cols.iter().enumerate() {
            let Some(x) = self.col_x(l, i) else { continue };
            if x > l.w {
                break;
            }
            if c.held {
                continue;
            }
            let st = strips.get(i).cloned().unwrap_or_default();
            cv.fill_rect(x, l.mixer_y, l.col_w, l.mixer_h, th.panel);
            let pad = (4.0 * s).round();
            for (p, py, ph) in self.strip_rows(l, st.fx.len()) {
                let y = l.mixer_y + py;
                let hv = self.hover == Hit::Strip(i, p);
                match p {
                    Part::Input | Part::MidiOut | Part::Monitor | Part::Fx(_) | Part::FxAdd => {
                        let (label, on, dim) = match p {
                            Part::Input => (
                                if st.input.is_empty() {
                                    "No input".into()
                                } else {
                                    st.input.clone()
                                },
                                false,
                                st.input.is_empty(),
                            ),
                            Part::MidiOut => (
                                if st.midi_out.is_empty() {
                                    "No MIDI out".into()
                                } else {
                                    st.midi_out.clone()
                                },
                                false,
                                st.midi_out.is_empty(),
                            ),
                            Part::Monitor => (
                                match st.monitor {
                                    1 => "Monitor".into(),
                                    2 => "Monitor auto".into(),
                                    _ => "Monitor off".into(),
                                },
                                st.monitor != 0,
                                st.monitor == 0,
                            ),
                            Part::Fx(k) => {
                                let (n, en) = st.fx.get(k).cloned().unwrap_or_default();
                                (n, false, !en)
                            }
                            _ => ("+".into(), false, true),
                        };
                        let bg = if on {
                            th.sel_dim
                        } else if hv {
                            th.hover
                        } else {
                            th.bg_alt
                        };
                        cv.fill_rect(x + pad, y, l.col_w - 2.0 * pad, ph, bg);
                        let fg = if dim { th.text_faint } else { th.text_dim };
                        text.draw_in(
                            cv,
                            f,
                            x + pad + 3.0,
                            y,
                            l.col_w - 2.0 * pad - 6.0,
                            ph,
                            &label,
                            fg,
                            Align::Left,
                        );
                    }
                    Part::Fader => {
                        self.draw_fader(cv, text, th, &st, x, y, l.col_w, ph, l, i);
                    }
                    Part::Pan => {
                        let pw = l.col_w - 2.0 * pad;
                        cv.fill_rect(x + pad, y + ph * 0.5 - 1.0, pw, 2.0, th.seam);
                        let cx = x + pad + pw * 0.5;
                        let px = cx + (st.pan as f32) * pw * 0.5;
                        let (a, b) = if px < cx { (px, cx) } else { (cx, px) };
                        cv.fill_rect(a, y + ph * 0.5 - 1.0, (b - a).max(1.0), 2.0, th.accent);
                        cv.fill_rect(
                            px - 1.0,
                            y + 1.0,
                            2.0,
                            ph - 2.0,
                            if hv { th.text } else { th.text_dim },
                        );
                    }
                    _ => {}
                }
            }
            let _ = icons;
        }
        cv.pop_clip();
    }

    fn draw_fader(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        st: &Strip,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        l: &Layout,
        col: usize,
    ) {
        let s = l.s;
        let pad = (4.0 * s).round();
        let bw = (22.0 * s).round();
        let fw = w - 2.0 * pad - bw;
        // La glissiere et le vumetre.
        let gx = x + pad + fw * 0.5 - (2.0 * s).round();
        let top = y + (4.0 * s).round();
        let bot = y + h - (12.0 * s).round();
        let span = (bot - top).max(1.0);
        let pos = |db: f64| -> f32 {
            // 0 dB a 3/4 de la course ; -60 dB en bas.
            let k = if db >= 0.0 {
                0.75 + 0.25 * (db / 12.0).min(1.0)
            } else {
                0.75 * (1.0 + db / 60.0).max(0.0)
            };
            bot - span * k as f32
        };
        let mw = (3.0 * s).round();
        for (k, &m) in st.meter.iter().enumerate() {
            let mx = gx + (8.0 * s).round() + k as f32 * (mw + 1.0);
            cv.fill_rect(mx, top, mw, span, th.seam);
            let db = if m > 1e-6 {
                20.0 * (m as f64).log10()
            } else {
                -150.0
            };
            let my = pos(db);
            let c = if db > -0.1 {
                hex(Ink::REC)
            } else if db > -6.0 {
                hex(Ink::QUEUE)
            } else {
                hex(Ink::PLAY)
            };
            if my < bot {
                cv.fill_rect(mx, my, mw, bot - my, c);
            }
        }
        cv.fill_rect(gx, top, (3.0 * s).round(), span, th.seam);
        let db = db_of(st.vol);
        let ky = pos(db);
        let hv = self.hover == Hit::Strip(col, Part::Fader);
        let kw = (16.0 * s).round();
        let kh = (8.0 * s).round();
        cv.fill_round_rect(
            gx - kw * 0.5 + 1.5,
            ky - kh * 0.5,
            kw,
            kh,
            (2.0 * s).round(),
            if hv { th.text } else { th.text_dim },
        );
        let label = if db <= -149.0 {
            "-inf".to_string()
        } else {
            format!("{db:+.1}")
        };
        text.draw_in(
            cv,
            th.font_small(),
            x + pad,
            bot + 1.0,
            fw,
            (11.0 * s).round(),
            &label,
            th.text_faint,
            Align::Center,
        );
        // M, S, le mode d'automation.
        let bx = x + w - bw - pad + (2.0 * s).round();
        let bh = (16.0 * s).round();
        let auto = ["T", "R", "Tc", "W", "L", "LP"];
        for (k, (lab, on, c, part)) in [
            ("M", st.mute, hex(Ink::REC), Part::Mute),
            ("S", st.solo, hex(Ink::QUEUE), Part::Solo),
            (
                auto.get(st.auto.clamp(0, 5) as usize)
                    .copied()
                    .unwrap_or("R"),
                st.auto >= 2,
                th.accent,
                Part::Auto,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let by = y + k as f32 * (bh + 2.0);
            let hv = self.hover == Hit::Strip(col, part);
            let bg = if on {
                c.mix(th.panel, 0.3)
            } else if hv {
                th.hover
            } else {
                th.bg_alt
            };
            cv.fill_round_rect(
                bx,
                by,
                bw - 2.0 * (2.0 * s).round(),
                bh,
                (2.0 * s).round(),
                bg,
            );
            text.draw_in(
                cv,
                th.font_small(),
                bx,
                by,
                bw - 2.0 * (2.0 * s).round(),
                bh,
                lab,
                if on { th.text } else { th.text_dim },
                Align::Center,
            );
        }
    }

    fn draw_status(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
        msg: Option<&str>,
    ) {
        let s = l.s;
        let y = l.status_y;
        let hh = l.h - y;
        cv.fill_rect(0.0, y, l.w, hh, th.bar);
        cv.fill_rect(0.0, y, l.w, 1.0, th.seam);
        let pad = (8.0 * s).round();
        // L'onglet de la vue ancree tient un bout de la ligne.
        let (rl, rr) = if self.dock_handle {
            let k = cp_ui::handle::reserve(self.tab_rect(), s);
            match self.dock_corner {
                cp_ui::handle::Corner::BottomLeft => (k, 0.0),
                cp_ui::handle::Corner::BottomRight => (0.0, k),
            }
        } else {
            (0.0, 0.0)
        };
        let line = match msg {
            Some(m) => m.to_string(),
            None => {
                let playing = ses.cols.iter().filter(|c| c.run.play.is_some()).count();
                if playing > 0 {
                    format!("{playing} playing")
                } else {
                    String::new()
                }
            }
        };
        text.draw_in(
            cv,
            th.font_small(),
            pad + rl,
            y,
            l.w - 2.0 * pad - rl - rr,
            hh,
            &line,
            th.text_dim,
            Align::Left,
        );
    }

    /// La colonne fantome (une piste neuve) et le fantome d'un glisser.
    fn draw_ghost(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        ses: &Session,
        l: &Layout,
    ) {
        let s = l.s;
        if let Some(x) = self.col_x(l, ses.cols.len()) {
            if x < l.w {
                let hv = self.hover == Hit::Ghost;
                let c = if hv { th.text_dim } else { th.text_faint };
                let y = l.header_y + 2.0;
                let hh = l.stop_y - y - 2.0;
                cv.dash_rect(
                    x + 0.5,
                    y + 0.5,
                    l.col_w - 1.0,
                    hh - 1.0,
                    3.0 * s,
                    3.0 * s,
                    c,
                );
                text.draw_in(
                    cv,
                    th.font(),
                    x,
                    l.header_y,
                    l.col_w,
                    l.grid_y - l.header_y,
                    "+",
                    c,
                    Align::Center,
                );
            }
        }
        if let Some((col, scene, copy)) = self.cell_drag() {
            let Some(clip) = ses.clip(col, scene) else {
                return;
            };
            let (mx, my) = self.mouse;
            let w = l.col_w;
            let h = l.cell_h - 2.0;
            let x = mx - w * 0.3;
            let y = my - h * 0.5;
            let bg = th.panel.mix(th.text, 0.1);
            cv.fill_round_rect(x, y, w, h, (3.0 * s).round(), bg);
            cv.stroke_round_rect(
                x + 0.5,
                y + 0.5,
                w - 1.0,
                h - 1.0,
                (3.0 * s).round(),
                1.0,
                th.text_dim,
            );
            let name = if copy {
                format!("+ {}", clip.name)
            } else {
                clip.name.clone()
            };
            text.draw_in(
                cv,
                th.font_small(),
                x + (6.0 * s).round(),
                y,
                w - (8.0 * s).round(),
                h,
                &name,
                th.text,
                Align::Left,
            );
        }
    }
}
