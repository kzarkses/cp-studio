//! Le dessin du navigateur.

use cp_gfx::text::{Align, TextCache};
use cp_gfx::{Canvas, Color, Pt, pt};
use cp_theme::Frames;
use cp_ui::{Icon, IconCache, Theme};

use crate::model::{Kind, Meta};
use crate::{Browser, Focus, Host, Row};

/// Les boutons du theme de REAPER pour la barre : ceux de son transport, a
/// leur taille (lecture, lecture allumee, boucle eteinte et allumee).
#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub play: Option<Frames>,
    pub play_on: Option<Frames>,
    pub repeat_off: Option<Frames>,
    pub repeat_on: Option<Frames>,
}

/// Ce dont le dessin a besoin.
pub struct Gfx<'a> {
    pub text: &'a mut TextCache,
    pub icons: &'a mut IconCache,
    pub th: &'a Theme,
    /// Les boutons natifs ; `None` : les icones vectorielles.
    pub skin: Option<&'a Skin>,
}

/// « 1:23.456 » ou « 4.250 s ».
pub fn fmt_time(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "-".into();
    }
    if s < 60.0 {
        format!("{s:.3} s")
    } else {
        let m = (s / 60.0).floor();
        format!("{}:{:06.3}", m as i64, s - m * 60.0)
    }
}

fn fmt_meta(m: &Meta) -> String {
    let mut v = Vec::new();
    if let Some(l) = m.length_s {
        v.push(if l < 60.0 {
            format!("{l:.1} s")
        } else {
            format!("{}:{:02}", (l / 60.0) as i64, (l % 60.0) as i64)
        });
    }
    if let Some(s) = m.srate {
        v.push(format!("{}k", (s as f64 / 1000.0)));
    }
    if let Some(c) = m.chans {
        v.push(if c == 1 {
            "mono".into()
        } else if c == 2 {
            "st".into()
        } else {
            format!("{c}ch")
        });
    }
    v.join("  ")
}

impl Browser {
    pub fn draw(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>, h: &mut dyn Host) {
        let th = g.th;
        let l = self.lay;
        cv.clear(th.bg);
        self.draw_bar(cv, g);
        // Le lisere de la vue ancree, par-dessus le haut de la barre.
        if self.dock_handle {
            let hot = self.grip_hover || matches!(self.grab, crate::Grab::Handle { .. });
            cp_ui::handle::draw_grip(cv, th, l.w, hot);
        }
        cv.fill_rect(0.0, l.bar_h, l.w, 1.0, th.seam);
        self.thumbs_pending.set(false);
        self.draw_list(cv, g, h);
        cv.fill_rect(0.0, l.wave_y - 1.0, l.w, 1.0, th.seam);
        self.draw_wave(cv, g, h);
        let rate = self
            .sync_rate(h)
            .zip(crate::v2::bpm_from_name(&crate::model::display_name(
                std::path::Path::new(&self.wave.path),
            )));
        self.draw_status(cv, g, rate);
    }

    /// Le bas seulement : la forme d'onde et la ligne d'etat (la tete de
    /// lecture qui avance ne redessine pas la liste).
    pub fn draw_wave_band(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>, h: &mut dyn Host) {
        let th = g.th;
        let l = self.lay;
        cv.fill_rect(0.0, l.wave_y - 1.0, l.w, 1.0, th.seam);
        self.draw_wave(cv, g, h);
        let rate = self
            .sync_rate(h)
            .zip(crate::v2::bpm_from_name(&crate::model::display_name(
                std::path::Path::new(&self.wave.path),
            )));
        self.draw_status(cv, g, rate);
    }

    fn draw_bar(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>) {
        let th = g.th;
        let l = self.lay;
        cv.fill_rect(0.0, 0.0, l.w, l.bar_h, th.bar);
        let fh = th.s(24.0);
        let fy = ((l.bar_h - fh) * 0.5).round();
        // Le champ de recherche, sa loupe a gauche.
        let ic = th.s(14.0);
        cv.fill_round_rect(l.search_x, fy, l.search_w, fh, th.s(4.0), th.field);
        g.icons.draw(
            cv,
            Icon::Search,
            l.search_x + th.s(7.0),
            fy + (fh - ic) * 0.5,
            ic as u16,
            th.text_faint,
            false,
        );
        let off = th.s(24.0);
        let placeholder = if self.db_loading() {
            "Search (reading databases…)"
        } else {
            "Search"
        };
        self.search.draw(
            cv,
            g.text,
            th,
            l.search_x + off,
            fy,
            (l.search_w - off).max(0.0),
            fh,
            self.focus == Focus::Search,
            placeholder,
        );
        if self.focus == Focus::Search {
            cv.stroke_round_rect(
                l.search_x,
                fy,
                l.search_w,
                fh,
                th.s(4.0),
                th.s(1.0),
                th.accent.fade(0.7),
            );
        }
        // Les boutons.
        let bi = th.s(16.0);
        let by = ((l.bar_h - l.btn) * 0.5).round();
        let button = |cv: &mut Canvas<'_>,
                      g: &mut Gfx<'_>,
                      x: f32,
                      icon: Icon,
                      on: bool,
                      filled: bool,
                      tint: Color| {
            if on {
                cv.fill_round_rect(x, by, l.btn, l.btn, g.th.s(4.0), g.th.sel_dim);
            }
            g.icons.draw(
                cv,
                icon,
                x + (l.btn - bi) * 0.5,
                by + (l.btn - bi) * 0.5,
                bi as u16,
                tint,
                filled,
            );
        };
        let view_tint = if self.samples_view {
            th.accent
        } else {
            th.text_dim
        };
        button(
            cv,
            g,
            l.view_x,
            Icon::AudioLines,
            self.samples_view,
            false,
            view_tint,
        );
        let sync_tint = if self.sync { th.accent } else { th.text_dim };
        button(cv, g, l.sync_x, Icon::Music, self.sync, false, sync_tint);
        let playing = self.voice != 0;
        // Les boutons du transport du theme, a leur taille ; a defaut, les
        // icones vectorielles.
        let frame =
            |f: &Option<Frames>, on: &Option<Frames>, lit: bool| -> Option<cp_gfx::Surface> {
                let f = if lit {
                    on.as_ref().or(f.as_ref())
                } else {
                    f.as_ref()
                };
                f.and_then(|f| f.get(0)).cloned()
            };
        let skin = g.skin;
        let loop_img = skin.and_then(|s| frame(&s.repeat_off, &s.repeat_on, self.looping));
        let play_img = skin.and_then(|s| frame(&s.play, &s.play_on, playing));
        match loop_img {
            Some(img) => cv.draw_image(
                &img,
                l.loop_x,
                ((l.bar_h - img.h as f32) * 0.5).floor(),
                1.0,
            ),
            None => {
                let loop_tint = if self.looping { th.accent } else { th.text_dim };
                button(
                    cv,
                    g,
                    l.loop_x,
                    Icon::Repeat,
                    self.looping,
                    false,
                    loop_tint,
                );
            }
        }
        match play_img {
            Some(img) => cv.draw_image(
                &img,
                l.play_x,
                ((l.bar_h - img.h as f32) * 0.5).floor(),
                1.0,
            ),
            None => {
                let (pi, pt_) = if playing {
                    (Icon::Stop, th.text)
                } else {
                    (Icon::Play, th.text_dim)
                };
                button(cv, g, l.play_x, pi, false, false, pt_);
            }
        }
        // Le volume : une piste, le remplissage, la poignee.
        let vy = (l.bar_h * 0.5).round();
        let vh = th.s(4.0);
        cv.fill_round_rect(l.vol_x, vy - vh * 0.5, l.vol_w, vh, vh * 0.5, th.field);
        let vx = l.vol_x + l.vol_w * self.volume;
        cv.fill_round_rect(
            l.vol_x,
            vy - vh * 0.5,
            vx - l.vol_x,
            vh,
            vh * 0.5,
            th.accent.fade(0.8),
        );
        cv.fill_circle(vx, vy, th.s(5.0), th.text);
        let fav = self
            .sel
            .and_then(|i| self.rows.get(i).copied())
            .and_then(|r| self.row_path(r))
            .is_some_and(|p| self.is_favorite(p));
        button(
            cv,
            g,
            l.star_x,
            Icon::Star,
            false,
            fav,
            if fav { th.star } else { th.text_dim },
        );
    }

    fn row_icon(&self, r: Row, th: &Theme) -> (Icon, Color, bool) {
        match r {
            Row::Hit(_) => (Icon::FileMusic, Self::file_color(th), false),
            Row::Node(id) => {
                let n = &self.tree.nodes[id];
                let folder = if n.expanded {
                    Icon::FolderOpen
                } else {
                    Icon::Folder
                };
                if Some(id) == self.tree.recordings {
                    return (folder, th.accent, false);
                }
                match n.kind {
                    Kind::Dir => (folder, Self::dir_color(th), false),
                    Kind::Drive => (Icon::HardDrive, Self::dir_color(th), false),
                    Kind::Db => (Icon::Database, Self::db_color(th), false),
                    Kind::File => (Icon::FileMusic, Self::file_color(th), false),
                    Kind::Favorites => (Icon::Star, th.star, true),
                    Kind::Section => (Icon::List, th.text_faint, false),
                    Kind::Collection(_) => (Icon::List, Self::dir_color(th), false),
                    Kind::FxRoot | Kind::FxGroup => (folder, th.text_dim, false),
                    Kind::Fx => (Icon::AudioLines, th.text_dim, false),
                    Kind::Add => (Icon::Plus, th.text_faint, false),
                }
            }
        }
    }

    /// Les teintes des icones, prises dans le theme : un dossier tire vers
    /// le jaune de la tete de lecture, une base et un fichier vers l'accent.
    fn dir_color(th: &Theme) -> Color {
        th.text_dim.mix(th.wave_sel, 0.3)
    }
    fn db_color(th: &Theme) -> Color {
        th.text_dim.mix(th.accent, 0.55)
    }
    fn file_color(th: &Theme) -> Color {
        th.text_faint.mix(th.accent, 0.35)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &self,
        cv: &mut Canvas<'_>,
        g: &mut Gfx<'_>,
        h: &mut dyn Host,
        i: usize,
        r: Row,
        y: f32,
        w: f32,
        sticky: bool,
    ) {
        let th = g.th;
        let l = self.lay;
        let rh = l.row_h;
        let sel = self.sel == Some(i);
        let bg = if sel {
            if self.focus == Focus::List {
                th.sel
            } else {
                th.sel_dim
            }
        } else if sticky {
            th.panel
        } else if self.hover == Some(i) {
            th.hover
        } else if i % 2 == 1 {
            th.bg_alt
        } else {
            th.bg
        };
        cv.fill_rect(0.0, y, w, rh, bg);
        // En vue « samples », le nom occupe le haut de la ligne, la forme
        // d'onde le bas : tout ce qui suit se place dans la bande du nom.
        let full_rh = rh;
        let rh = if self.samples_view {
            th.s(22.0).min(rh)
        } else {
            rh
        };
        let f = th.font();
        let s = |v: f32| th.s(v);
        let (depth, name, container, expanded, section) = match r {
            Row::Node(id) => {
                let n = &self.tree.nodes[id];
                (
                    n.depth as f32,
                    n.name.as_str(),
                    self.tree.is_container(id),
                    n.expanded,
                    n.kind == Kind::Section,
                )
            }
            Row::Hit(k) => (0.0, self.hits[k].name.as_str(), false, false, false),
        };
        if section {
            // Un titre de section : petit, en capitales, une ligne fine.
            let fs = th.font_small();
            let tw = g.text.measure(fs, &name.to_uppercase());
            g.text.draw_in(
                cv,
                fs,
                s(10.0),
                y,
                w - s(20.0),
                rh,
                &name.to_uppercase(),
                th.text_faint,
                Align::Left,
            );
            cv.fill_rect(
                s(16.0) + tw,
                y + (rh * 0.5).round(),
                (w - s(26.0) - tw).max(0.0),
                1.0,
                th.seam,
            );
            return;
        }
        let mut x = s(8.0) + depth * s(14.0);
        let ci = s(12.0);
        if container {
            let ch = if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            g.icons.draw(
                cv,
                ch,
                x,
                y + (rh - ci) * 0.5,
                ci as u16,
                th.text_faint,
                false,
            );
        }
        x += s(16.0);
        let (icon, tint, filled) = self.row_icon(r, th);
        let isz = s(15.0);
        match r {
            Row::Node(id) if matches!(self.tree.nodes[id].kind, Kind::Collection(_)) => {
                if let Kind::Collection(n) = self.tree.nodes[id].kind {
                    let c = cp_gfx::hex(crate::model::COLLECTION_COLORS[(n - 1) as usize]);
                    cv.fill_circle(x + isz * 0.5, y + rh * 0.5, s(5.0), c);
                }
            }
            _ => g
                .icons
                .draw(cv, icon, x, y + (rh - isz) * 0.5, isz as u16, tint, filled),
        }
        x += isz + s(6.0);
        // A droite : ce qu'on sait du fichier, ou le dossier d'un resultat.
        let fs = th.font_small();
        let right = match r {
            Row::Node(id) => {
                let n = &self.tree.nodes[id];
                if n.kind == Kind::File && n.meta == Meta::default() && n.mtime.is_some() {
                    // Un enregistrement : son age (« 3 min ago »).
                    n.mtime
                        .map(|t| crate::model::age(t, std::time::SystemTime::now()))
                        .unwrap_or_default()
                } else if n.kind == Kind::File {
                    fmt_meta(&n.meta)
                } else if Some(id) == self.tree.recordings && n.error.is_none() {
                    // Ou est-il : la fin du chemin.
                    let p = n.path.to_string_lossy();
                    let tail: String = p
                        .chars()
                        .rev()
                        .take(34)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if tail.len() < p.len() {
                        format!("…{tail}")
                    } else {
                        tail
                    }
                } else if let Some(e) = &n.error {
                    let _ = e;
                    "unavailable".into()
                } else {
                    String::new()
                }
            }
            Row::Hit(k) => fmt_meta(&self.hits[k].meta),
        };
        let mut rw = 0.0;
        if let Some(p) = self.row_path(r) {
            let mask = self.collection_mask(p);
            if mask != 0 {
                let mut cx = w - s(10.0);
                for n in (0..7).rev() {
                    if mask & (1 << n) != 0 {
                        let c = cp_gfx::hex(crate::model::COLLECTION_COLORS[n]);
                        cv.fill_circle(cx, y + rh * 0.5, s(3.5), c);
                        cx -= s(9.0);
                    }
                }
                rw = w - cx - s(4.0);
            }
        }
        if !right.is_empty() {
            let mw = g.text.measure(fs, &right) + s(10.0);
            g.text.draw_in(
                cv,
                fs,
                w - rw - mw,
                y,
                mw - s(8.0),
                rh,
                &right,
                th.text_faint,
                Align::Right,
            );
            rw += mw;
        }
        let avail = (w - x - rw - s(4.0)).max(0.0);
        let col = if sel {
            cp_gfx::rgb(255, 255, 255)
        } else {
            th.text
        };
        if let Row::Hit(k) = r {
            let nw = g.text.measure(f, name).min(avail);
            g.text
                .draw_in(cv, f, x, y, avail, rh, name, col, Align::Left);
            let dx = x + nw + s(8.0);
            if dx < x + avail - s(30.0) {
                g.text.draw_in(
                    cv,
                    fs,
                    dx,
                    y,
                    x + avail - dx,
                    rh,
                    &self.hits[k].dir,
                    th.text_faint,
                    Align::Left,
                );
            }
        } else {
            g.text
                .draw_in(cv, f, x, y, avail, rh, name, col, Align::Left);
        }
        // La vue « samples » : la forme d'onde du fichier sous son nom.
        if self.samples_view && self.row_is_file(r) && full_rh > rh {
            if let Some(p) = self.row_path(r) {
                let tx = x;
                let tw = (w - tx - s(10.0)).max(0.0) as usize;
                let ty = y + rh;
                let thh = full_rh - rh - s(4.0);
                if tw > 4 && thh > 2.0 {
                    let mut lo = vec![0.0f32; tw];
                    let mut hi = vec![0.0f32; tw];
                    match h.thumb(&p.to_string_lossy(), &mut lo, &mut hi) {
                        1 => {
                            let mid = ty + thh * 0.5;
                            let half = thh * 0.5;
                            let c = if sel { th.text } else { th.wave.fade(0.75) };
                            for k in 0..tw {
                                let a = (mid - hi[k].clamp(-1.0, 1.0) * half).floor();
                                let b = (mid - lo[k].clamp(-1.0, 1.0) * half).ceil();
                                cv.fill_rect(tx + k as f32, a, 1.0, (b - a).max(1.0), c);
                            }
                        }
                        0 => {
                            self.thumbs_pending.set(true);
                            cv.fill_rect(tx, ty + thh * 0.5, tw as f32, 1.0, th.seam);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw_list(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>, h: &mut dyn Host) {
        let th = g.th;
        let l = self.lay;
        if !cv.push_clip(0.0, l.list_y, l.w, l.list_h) {
            cv.pop_clip();
            return;
        }
        cv.fill_rect(0.0, l.list_y, l.w, l.list_h, th.bg);
        let w = if self.scroll.needed() {
            l.w - l.scroll_w - 2.0
        } else {
            l.w
        };
        let rh = l.row_h;
        let first = (self.scroll.pos / rh).floor() as usize;
        let n = (l.list_h / rh).ceil() as usize + 1;
        for i in first..(first + n).min(self.rows.len()) {
            let y = l.list_y + i as f32 * rh - self.scroll.pos;
            self.draw_row(cv, g, h, i, self.rows[i], y, w, false);
        }
        // Les en-tetes colles, empiles.
        let sticky = self.sticky();
        for (k, &id) in sticky.iter().enumerate() {
            let y = l.list_y + k as f32 * rh;
            if let Some(i) = self.rows.iter().position(|&r| r == Row::Node(id)) {
                self.draw_row(cv, g, h, i, Row::Node(id), y, w, true);
            }
        }
        if !sticky.is_empty() {
            let y = l.list_y + sticky.len() as f32 * rh;
            cv.fill_rect(0.0, y, w, 1.0, th.seam);
            cv.fill_rect(0.0, y + 1.0, w, th.s(3.0), cp_gfx::rgba(0, 0, 0, 40));
        }
        // Les etats vides.
        if self.rows.is_empty() {
            let msg = if !self.search.text.trim().is_empty() {
                if self.db_loading() {
                    "Reading databases…"
                } else {
                    "No match"
                }
            } else {
                "No shortcut: add folders in REAPER's Media Explorer"
            };
            g.text.draw_in(
                cv,
                th.font(),
                0.0,
                l.list_y + th.s(10.0),
                l.w,
                rh,
                msg,
                th.text_faint,
                Align::Center,
            );
        }
        // La barre de defilement.
        if self.scroll.needed() {
            let (t0, tl) = self.scroll.thumb(l.list_h, th.s(20.0));
            let x = l.w - l.scroll_w;
            let c = if self.scroll.grab.is_some() {
                th.text_dim
            } else {
                th.text_faint.fade(0.6)
            };
            cv.fill_round_rect(
                x + th.s(2.0),
                l.list_y + t0 + 2.0,
                l.scroll_w - th.s(4.0),
                tl - 4.0,
                th.s(3.0),
                c,
            );
        }
        cv.pop_clip();
    }

    fn draw_wave(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>, h: &mut dyn Host) {
        let th = g.th;
        let l = self.lay;
        if !cv.push_clip(0.0, l.wave_y, l.w, l.wave_h) {
            cv.pop_clip();
            return;
        }
        cv.fill_rect(0.0, l.wave_y, l.w, l.wave_h, th.panel);
        let wv = &self.wave;
        if !wv.has() {
            let msg = if let Some(e) = &self.error {
                format!("Cannot play this file: {e}")
            } else if self.pending.is_some() {
                "Loading…".to_string()
            } else {
                "Select a file to listen".to_string()
            };
            let col = if self.error.is_some() {
                th.error
            } else {
                th.text_faint
            };
            g.text.draw_in(
                cv,
                th.font(),
                th.s(10.0),
                l.wave_y,
                l.w - th.s(20.0),
                l.wave_h,
                &msg,
                col,
                Align::Center,
            );
            cv.pop_clip();
            return;
        }
        let w = l.w.floor() as usize;
        let nch = wv.info.nch.clamp(1, 2) as usize;
        let lane_h = l.wave_h / nch as f32;
        // La section, sous la forme d'onde.
        if let Some((a, b)) = wv.sel {
            let xa = wv.frame_to_x(a as f64).max(-1.0);
            let xb = wv.frame_to_x(b as f64).min(l.w + 1.0);
            let ww = (xb - xa).max(1.0);
            cv.fill_rect(xa, l.wave_y, ww, l.wave_h, th.section);
        }
        let mut lo = vec![0.0f32; w];
        let mut hi = vec![0.0f32; w];
        for ch in 0..nch {
            let y0 = l.wave_y + ch as f32 * lane_h;
            let mid = y0 + lane_h * 0.5;
            let half = lane_h * 0.5 - th.s(3.0);
            cv.fill_rect(0.0, mid.round(), l.w, 1.0, th.seam);
            let n = h.peaks(wv.clip, Some(ch), wv.start, wv.fpp, &mut lo, &mut hi);
            let n = n.min(w);
            let color = th.wave;
            if wv.fpp > 1.0 {
                for i in 0..n {
                    let ya = (mid - hi[i].clamp(-1.0, 1.0) * half).floor();
                    let yb = (mid - lo[i].clamp(-1.0, 1.0) * half).ceil();
                    cv.fill_rect(i as f32, ya, 1.0, (yb - ya).max(1.0), color);
                }
            } else {
                // Sous l'echantillon : la courbe, et les echantillons eux-memes.
                let pts: Vec<Pt> = (0..n)
                    .map(|i| pt(i as f32 + 0.5, mid - lo[i].clamp(-1.0, 1.0) * half))
                    .collect();
                cv.polyline(&pts, th.s(1.5), color);
                if wv.fpp < 0.25 {
                    let f0 = wv.start.ceil() as i64;
                    let f1 = (wv.start + w as f64 * wv.fpp).floor() as i64;
                    let mut one = [0.0f32; 1];
                    let mut one2 = [0.0f32; 1];
                    for f in f0..=f1.min(wv.info.frames - 1) {
                        let x = wv.frame_to_x(f as f64);
                        h.peaks(wv.clip, Some(ch), f as f64, 1.0 / 64.0, &mut one, &mut one2);
                        cv.fill_circle(
                            x,
                            mid - one[0].clamp(-1.0, 1.0) * half,
                            th.s(2.5),
                            th.wave_sel,
                        );
                    }
                }
            }
            if ch + 1 < nch {
                cv.fill_rect(0.0, (y0 + lane_h).round(), l.w, 1.0, th.seam.fade(0.6));
            }
        }
        // Les bords de la section.
        if let Some((a, b)) = wv.sel {
            for f in [a, b] {
                let x = wv.frame_to_x(f as f64).round();
                cv.fill_rect(x, l.wave_y, 1.0, l.wave_h, th.wave_sel);
            }
        }
        // La tete de lecture.
        if let Some(p) = wv.playhead {
            let x = wv.frame_to_x(p);
            if x >= -1.0 && x <= l.w + 1.0 {
                cv.fill_rect(
                    x.round(),
                    l.wave_y,
                    th.s(1.0).max(1.0),
                    l.wave_h,
                    th.playhead,
                );
            }
        }
        // L'echelle : ou on est dans le fichier.
        if wv.fpp * (l.w as f64) < wv.info.frames as f64 * 0.999 {
            let total = wv.info.frames as f64;
            let a = wv.start / total;
            let b = (wv.start + wv.fpp * l.w as f64) / total;
            let y = l.wave_y + l.wave_h - th.s(3.0);
            cv.fill_rect(0.0, y, l.w, th.s(3.0), th.seam);
            cv.fill_rect(
                (a * l.w as f64) as f32,
                y,
                (((b - a) * l.w as f64) as f32).max(2.0),
                th.s(3.0),
                th.text_faint,
            );
        }
        cv.pop_clip();
    }

    fn draw_status(&mut self, cv: &mut Canvas<'_>, g: &mut Gfx<'_>, sync: Option<(f64, f64)>) {
        let th = g.th;
        let l = self.lay;
        cv.fill_rect(0.0, l.status_y, l.w, l.status_h, th.bar);
        let f = th.font_small();
        let pad = th.s(8.0);
        let left = if let Some(e) = &self.error {
            e.clone()
        } else if !self.status.is_empty() {
            self.status.clone()
        } else if self.wave.has() {
            let i = self.wave.info;
            let len = i.frames as f64 / i.srate.max(1.0);
            let ch = match i.nch {
                1 => "mono".to_string(),
                2 => "stereo".to_string(),
                n => format!("{n} ch"),
            };
            let mut s = format!(
                "{}  ·  {} Hz  ·  {}  ·  {}",
                crate::model::display_name(std::path::Path::new(&self.wave.path)),
                i.srate,
                ch,
                fmt_time(len)
            );
            if let Some((r, bpm)) = sync {
                s += &format!("  ·  {bpm:.0} BPM  x{r:.3}");
            }
            if let Some((a, b)) = self.wave.sel {
                s += &format!(
                    "  ·  section {}  ({} samples)",
                    fmt_time((b - a) as f64 / i.srate.max(1.0)),
                    b - a
                );
            }
            s
        } else if let Some(p) = self
            .sel
            .and_then(|i| self.rows.get(i).copied())
            .and_then(|r| self.row_path(r))
        {
            p.to_string_lossy().into_owned()
        } else {
            String::new()
        };
        let col = if self.error.is_some() {
            th.error
        } else {
            th.text_dim
        };
        let right = format!("{:+.1} dB", self.gain_db().max(-99.0));
        let rw = g.text.measure(f, &right) + pad;
        g.text.draw_in(
            cv,
            f,
            l.w - rw,
            l.status_y,
            rw - pad,
            l.status_h,
            &right,
            th.text_faint,
            Align::Right,
        );
        g.text.draw_in(
            cv,
            f,
            pad,
            l.status_y,
            l.w - rw - 2.0 * pad,
            l.status_h,
            &left,
            col,
            Align::Left,
        );
    }
}
