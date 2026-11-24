//! Le dessin du navigateur d'effets.

use cp_fxlist::{Catalog, Src, Store, split_dev, split_name};
use cp_gfx::Canvas;
use cp_gfx::text::{Align, TextCache};
use cp_ui::{Icon, IconCache, Theme};

use crate::{Chain, GROUPS, Hit, Layout, R, SideEntry, View};

/// Le nom lisible d'un effet de la chaine : « VST3: Pro-Q 3 (FabFilter) » ->
/// « Pro-Q 3 ».
pub fn fx_label(name: &str) -> String {
    let (_, _, rest) = split_name(name);
    let (label, _) = split_dev(&rest);
    if label.is_empty() { rest } else { label }
}

/// L'icone d'une pastille fixe (les onglets montrent leur nom).
fn chip_icon(s: &Src) -> Option<Icon> {
    match s {
        Src::Fav => Some(Icon::Star),
        Src::Recent => Some(Icon::History),
        Src::Most => Some(Icon::TrendingUp),
        Src::Instr => Some(Icon::Piano),
        _ => None,
    }
}

impl View {
    /// Dessine toute la vue. `msg` : un message bref (une erreur).
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        cat: &Catalog,
        st: &Store,
        chain: &Chain,
        msg: Option<&str>,
        w: f32,
        h: f32,
    ) {
        self.size = (w, h);
        self.scale = th.scale;
        self.nfx = chain.fx.len();
        let l = self.layout(th, st, w, h);
        self.list_h = l.list.h;
        self.chip_geometry(text, th, &l);
        self.clamp_scrolls(&l);
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_top(cv, text, icons, th, st, &l);
        self.draw_chips(cv, text, icons, th, st, &l);
        if let Some(sd) = l.side {
            self.draw_side(cv, text, icons, th, &l, sd);
        }
        self.draw_list(cv, text, icons, th, cat, st, &l);
        if let Some(c) = l.chain {
            self.draw_chain(cv, text, icons, th, chain, &l, c);
        }
        for (seam, hot) in [
            (l.side_seam, self.hover == Hit::SideSeam),
            (l.chain_seam, self.hover == Hit::ChainSeam && l.chain_right),
        ] {
            if let Some(r) = seam {
                let c = if hot { th.accent.fade(0.6) } else { th.seam };
                cv.fill_rect(r.x, r.y, r.w, r.h, c);
            }
        }
        // Les onglets de la vue ancree, par-dessus son coin du bas.
        self.dock.draw(cv, text, th, w, h, self.grabbing_handle());
        if let Some(m) = msg {
            self.draw_msg(cv, text, th, &l, m);
        }
        self.draw_ghost(cv, text, th, cat, &l);
    }

    /// Les largeurs des pastilles (elles dependent du texte).
    fn chip_geometry(&mut self, text: &mut TextCache, th: &Theme, l: &Layout) {
        let s = l.s;
        let gap = (4.0 * s).round();
        let mut x = (6.0 * s).round();
        let icon_w = (28.0 * s).round();
        let f = th.font_small();
        self.chip_x.clear();
        for (i, c) in self.chips.iter().enumerate() {
            let w = if chip_icon(c).is_some() {
                icon_w
            } else {
                (text.measure(f, &c.name()) + (16.0 * s).round()).max(icon_w)
            };
            // Un trait entre les listes fixes et les onglets.
            if i == Src::CHIPS.len() {
                x += gap * 2.0;
            }
            self.chip_x.push((x, w));
            x += w + gap;
        }
        self.chip_add = (x, (24.0 * s).round());
    }

    fn button(&self, cv: &mut Canvas<'_>, th: &Theme, r: R, hot: bool, on: bool) {
        let bg = if on {
            th.sel
        } else if hot {
            th.hover.mix(th.text, 0.04)
        } else {
            th.bar
        };
        cv.fill_round_rect(r.x, r.y, r.w, r.h, (4.0 * th.scale).round(), bg);
    }

    fn draw_top(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        st: &Store,
        l: &Layout,
    ) {
        let s = l.s;
        let top = l.chips.y;
        cv.fill_rect(0.0, 0.0, l.w, top, th.bar);
        let r = l.search;
        let isz = (14.0 * s).round() as u16;
        let icon_box = r.h;
        self.search.draw_ex(
            cv,
            text,
            th,
            r.x,
            r.y,
            r.w,
            r.h,
            self.focus_search,
            "",
            icon_box - (6.0 * s).round(),
        );
        let ix = r.right() - icon_box + (icon_box - isz as f32) * 0.5;
        let iy = r.y + (r.h - isz as f32) * 0.5;
        if self.search.text.is_empty() {
            icons.draw(cv, Icon::Search, ix, iy, isz, th.text_faint, false);
        } else {
            let c = if self.hover == Hit::SearchClear {
                th.text
            } else {
                th.text_dim
            };
            icons.draw(cv, Icon::X, ix, iy, isz, c, false);
        }
        // Le de et son compte : un seul bouton en deux parties.
        let hot_d = self.hover == Hit::Dice;
        let hot_c = self.hover == Hit::Count;
        let joined = R::new(l.dice.x, l.dice.y, l.count.right() - l.dice.x, l.dice.h);
        self.button(cv, th, joined, false, false);
        if hot_d {
            self.button(cv, th, l.dice, true, false);
        }
        if hot_c {
            self.button(cv, th, l.count, true, false);
        }
        let dsz = (16.0 * s).round() as u16;
        icons.draw(
            cv,
            Icon::Dices,
            l.dice.x + (l.dice.w - dsz as f32) * 0.5,
            l.dice.y + (l.dice.h - dsz as f32) * 0.5,
            dsz,
            if hot_d { th.text } else { th.text_dim },
            false,
        );
        text.draw_in(
            cv,
            th.font_mono(),
            l.count.x,
            l.count.y,
            l.count.w - (3.0 * s).round(),
            l.count.h,
            &st.set.dice_count.to_string(),
            if hot_c { th.text } else { th.text_dim },
            Align::Center,
        );
        let hot_m = self.hover == Hit::Menu;
        if hot_m {
            self.button(cv, th, l.menu, true, false);
        }
        icons.draw(
            cv,
            Icon::Ellipsis,
            l.menu.x + (l.menu.w - isz as f32) * 0.5,
            l.menu.y + (l.menu.h - isz as f32) * 0.5,
            isz,
            if hot_m { th.text } else { th.text_dim },
            false,
        );
    }

    fn draw_chips(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        st: &Store,
        l: &Layout,
    ) {
        let s = l.s;
        let r = l.chips;
        cv.fill_rect(r.x, r.y, r.w, r.h, th.bar);
        cv.fill_rect(r.x, r.bottom() - 1.0, r.w, 1.0, th.seam);
        if !cv.push_clip(r.x, r.y, r.w, r.h) {
            cv.pop_clip();
            return;
        }
        let ch = (20.0 * s).round();
        let cy = r.y + ((r.h - ch) * 0.5).round();
        let rad = (ch * 0.5).round();
        let isz = (13.0 * s).round() as u16;
        let drop = self.drop_chip(l);
        let f = th.font_small();
        for (i, c) in self.chips.iter().enumerate() {
            let Some(&(px, pw)) = self.chip_x.get(i) else {
                continue;
            };
            let x = px - self.chip_scroll;
            if x > r.w || x + pw < 0.0 {
                continue;
            }
            if i == Src::CHIPS.len() {
                let sx = (x - (6.0 * s).round()).round();
                cv.fill_rect(
                    sx,
                    cy + (3.0 * s).round(),
                    1.0,
                    ch - (6.0 * s).round(),
                    th.seam,
                );
            }
            let on = *c == self.src;
            let hot = self.hover == Hit::Chip(i);
            let bg = if on {
                Some(th.sel)
            } else if hot {
                Some(th.hover.mix(th.text, 0.04))
            } else {
                None
            };
            if let Some(bg) = bg {
                cv.fill_round_rect(x, cy, pw, ch, rad, bg);
            }
            if drop == Some(i) {
                cv.stroke_round_rect(x, cy, pw, ch, rad, (1.5 * s).max(1.0), th.accent);
            }
            let fg = if on { th.text } else { th.text_dim };
            match chip_icon(c) {
                Some(ic) => {
                    let col = if *c == Src::Fav && on { th.star } else { fg };
                    icons.draw(
                        cv,
                        ic,
                        x + (pw - isz as f32) * 0.5,
                        cy + (ch - isz as f32) * 0.5,
                        isz,
                        col,
                        *c == Src::Fav && on,
                    );
                }
                None => {
                    let ruled =
                        matches!(c, Src::Tab(n) if st.tab(n).is_some_and(|t| t.rule.is_some()));
                    text.draw_in(cv, f, x, cy, pw, ch, &c.name(), fg, Align::Center);
                    if ruled {
                        // Un onglet a regle : un point d'accent.
                        let d = (2.0 * s).max(1.5);
                        cv.fill_circle(
                            x + pw - (5.0 * s).round(),
                            cy + (5.0 * s).round(),
                            d,
                            th.accent,
                        );
                    }
                }
            }
        }
        let (ax, aw) = self.chip_add;
        let x = ax - self.chip_scroll;
        if self.hover == Hit::ChipAdd {
            cv.fill_round_rect(x, cy, aw, ch, rad, th.hover.mix(th.text, 0.04));
        }
        icons.draw(
            cv,
            Icon::Plus,
            x + (aw - isz as f32) * 0.5,
            cy + (ch - isz as f32) * 0.5,
            isz,
            if self.hover == Hit::ChipAdd {
                th.text
            } else {
                th.text_faint
            },
            false,
        );
        cv.pop_clip();
    }

    fn draw_side(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        l: &Layout,
        sd: R,
    ) {
        let s = l.s;
        cv.fill_rect(sd.x, sd.y, sd.w, sd.h, th.bg_alt);
        if !cv.push_clip(sd.x, sd.y, sd.w, sd.h) {
            cv.pop_clip();
            return;
        }
        let rh = l.side_row;
        let pad = (8.0 * s).round();
        let isz = (11.0 * s).round() as u16;
        let first = (self.side_scroll / rh).floor().max(0.0) as usize;
        let fs = th.font_small();
        for (k, e) in self.side.iter().enumerate().skip(first) {
            let y = sd.y + k as f32 * rh - self.side_scroll;
            if y > sd.bottom() {
                break;
            }
            match e {
                SideEntry::Head(g) => {
                    let hot = self.hover == Hit::Group(*g);
                    let open = self
                        .side
                        .get(k + 1)
                        .is_some_and(|n| matches!(n, SideEntry::Item(..)));
                    let ic = if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    };
                    let c = if hot { th.text } else { th.text_faint };
                    icons.draw(
                        cv,
                        ic,
                        sd.x + (4.0 * s).round(),
                        y + (rh - isz as f32) * 0.5,
                        isz,
                        c,
                        false,
                    );
                    let name = GROUPS.get(*g).copied().unwrap_or("").to_uppercase();
                    text.draw_in(
                        cv,
                        fs,
                        sd.x + (4.0 * s).round() + isz as f32 + (4.0 * s).round(),
                        y,
                        sd.w - pad * 2.0,
                        rh,
                        &name,
                        c,
                        Align::Left,
                    );
                }
                SideEntry::Item(src, n) => {
                    let on = *src == self.src;
                    let hot = self.hover == Hit::Side(k);
                    if on {
                        cv.fill_rect(sd.x, y, sd.w, rh, th.sel);
                    } else if hot {
                        cv.fill_rect(sd.x, y, sd.w, rh, th.hover);
                    }
                    let count = n.to_string();
                    let cw = text.measure(fs, &count);
                    let x0 = sd.x + pad + (6.0 * s).round();
                    text.draw_in(
                        cv,
                        th.font(),
                        x0,
                        y,
                        (sd.right() - pad - cw - (6.0 * s).round() - x0).max(0.0),
                        rh,
                        &src.name(),
                        if on { th.text } else { th.text_dim },
                        Align::Left,
                    );
                    text.draw_in(
                        cv,
                        fs,
                        sd.right() - pad - cw,
                        y,
                        cw,
                        rh,
                        &count,
                        th.text_faint,
                        Align::Right,
                    );
                }
            }
        }
        cv.pop_clip();
    }

    fn draw_list(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        cat: &Catalog,
        st: &Store,
        l: &Layout,
    ) {
        let s = l.s;
        let r = l.list;
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if !cv.push_clip(r.x, r.y, r.w, r.h) {
            cv.pop_clip();
            return;
        }
        let rh = l.row;
        let star_w = (crate::geo::STAR * s).round();
        let ssz = (12.0 * s).round() as u16;
        let f = th.font();
        let fs = th.font_small();
        let fm = Font::mono_small(th);
        let tag_w = (44.0 * s).round();
        let pad = (8.0 * s).round();
        let first = (self.scroll / rh).floor().max(0.0) as usize;
        for (k, &i) in self.rows.iter().enumerate().skip(first) {
            let y = r.y + k as f32 * rh - self.scroll;
            if y > r.bottom() {
                break;
            }
            let Some(p) = cat.plugins.get(i) else {
                continue;
            };
            let sel = self.sel.contains(&i);
            let hot = self.hover == Hit::Row(k) || self.hover == Hit::Star(k);
            if sel {
                cv.fill_rect(r.x, y, r.w, rh, th.sel);
            } else if hot {
                cv.fill_rect(r.x, y, r.w, rh, th.hover);
            }
            if self.cursor == Some(k) && !self.focus_search {
                cv.stroke_rect(
                    r.x + 0.5,
                    y + 0.5,
                    r.w - 1.0,
                    rh - 1.0,
                    1.0,
                    th.accent.fade(0.5),
                );
            }
            let fav = st.is_fav(cat, i);
            if fav || self.hover == Hit::Star(k) {
                let c = if fav { th.star } else { th.text_faint };
                icons.draw(
                    cv,
                    Icon::Star,
                    r.x + (star_w - ssz as f32) * 0.5 + (2.0 * s).round(),
                    y + (rh - ssz as f32) * 0.5,
                    ssz,
                    c,
                    fav,
                );
            }
            let x0 = r.x + star_w + (2.0 * s).round();
            let tag_x = r.right() - pad - tag_w;
            let avail = (tag_x - x0 - pad).max(0.0);
            let fg = if sel {
                th.text
            } else {
                th.text.mix(th.bg, 0.05)
            };
            let lw = text.measure(f, &p.label).min(avail);
            text.draw_in(cv, f, x0, y, avail, rh, &p.label, fg, Align::Left);
            let dx = x0 + lw + (8.0 * s).round();
            if !p.dev.is_empty() && dx + (30.0 * s) < tag_x {
                text.draw_in(
                    cv,
                    fs,
                    dx,
                    y,
                    tag_x - dx - pad,
                    rh,
                    &p.dev,
                    th.text_faint,
                    Align::Left,
                );
            }
            text.draw_in(
                cv,
                fm,
                tag_x,
                y,
                tag_w,
                rh,
                &p.type_tag(),
                th.text_faint,
                Align::Right,
            );
        }
        // La barre de defilement : un trait fin a droite.
        let content = self.rows.len() as f32 * rh;
        if content > r.h {
            let bw = (3.0 * s).round().max(2.0);
            let th_h = (r.h * r.h / content).max((20.0 * s).round());
            let ty = r.y + (r.h - th_h) * (self.scroll / (content - r.h));
            cv.fill_round_rect(
                r.right() - bw - (2.0 * s).round(),
                ty,
                bw,
                th_h,
                bw * 0.5,
                th.text_faint.fade(0.5),
            );
        }
        cv.pop_clip();
    }

    fn draw_chain(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        chain: &Chain,
        l: &Layout,
        c: R,
    ) {
        let s = l.s;
        cv.fill_rect(c.x, c.y, c.w, c.h, th.bg_alt);
        if !cv.push_clip(c.x, c.y, c.w, c.h) {
            cv.pop_clip();
            return;
        }
        // L'en-tete : la piste.
        let hh = l.chain_head;
        cv.fill_rect(c.x, c.y, c.w, hh, th.bar);
        cv.fill_rect(c.x, c.y + hh - 1.0, c.w, 1.0, th.seam);
        let pad = (8.0 * s).round();
        let sw = (4.0 * s).round();
        let mut x = c.x + pad;
        if let Some(col) = chain
            .color
            .filter(|_| chain.track.is_some() && !chain.new_track)
        {
            cv.fill_round_rect(
                x,
                c.y + (6.0 * s).round(),
                sw,
                hh - (12.0 * s).round(),
                sw * 0.5,
                col,
            );
            x += sw + (6.0 * s).round();
        }
        let (name, col) = if chain.new_track {
            ("New track".to_string(), th.accent)
        } else {
            match &chain.track {
                Some(n) => (n.clone(), th.text),
                None => ("No track".to_string(), th.text_faint),
            }
        };
        let count = if chain.fx.is_empty() {
            String::new()
        } else {
            chain.fx.len().to_string()
        };
        let cw = text.measure(th.font_small(), &count);
        text.draw_in(
            cv,
            th.font_bold(),
            x,
            c.y,
            (c.right() - pad - cw - (6.0 * s).round() - x).max(0.0),
            hh,
            &name,
            col,
            Align::Left,
        );
        text.draw_in(
            cv,
            th.font_small(),
            c.right() - pad - cw,
            c.y,
            cw,
            hh,
            &count,
            th.text_faint,
            Align::Right,
        );
        // Les effets.
        let rh = l.chain_row;
        let y0 = c.y + hh;
        if !cv.push_clip(c.x, y0, c.w, (c.h - hh).max(0.0)) {
            cv.pop_clip();
            cv.pop_clip();
            return;
        }
        let isz = (rh - (10.0 * s).round()).max(8.0) as u16;
        let box_w = (rh - (6.0 * s).round()).max(8.0);
        let fm = Font::mono_small(th);
        let dragged = self.fx_dragged();
        for (i, fx) in chain.fx.iter().enumerate() {
            let y = y0 + i as f32 * rh - self.chain_scroll;
            if y > c.bottom() {
                break;
            }
            let hot =
                matches!(self.hover, Hit::Fx(k) | Hit::FxPower(k) | Hit::FxDelete(k) if k == i);
            if self.fx_sel == Some(i) {
                cv.fill_rect(c.x, y, c.w, rh, th.sel_dim);
            } else if hot {
                cv.fill_rect(c.x, y, c.w, rh, th.hover);
            }
            let dim = dragged == Some(i);
            let idx = format!("{}", i + 1);
            let iw = (20.0 * s).round();
            text.draw_in(
                cv,
                fm,
                c.x + pad * 0.5,
                y,
                iw,
                rh,
                &idx,
                th.text_faint,
                Align::Right,
            );
            let nx = c.x + pad * 0.5 + iw + (8.0 * s).round();
            let del_x = c.right() - (6.0 * s).round() - box_w;
            let pow_x = del_x - (4.0 * s).round() - box_w;
            let tw = (if hot || !fx.on {
                pow_x
            } else {
                c.right() - pad
            } - nx
                - (4.0 * s).round())
            .max(0.0);
            let fg = if !fx.on || dim {
                th.text_faint
            } else {
                th.text
            };
            let label = fx_label(&fx.name);
            text.draw_in(cv, th.font(), nx, y, tw, rh, &label, fg, Align::Left);
            if !fx.on {
                let lw = text.measure(th.font(), &label).min(tw);
                cv.fill_rect(nx, (y + rh * 0.5).round(), lw, 1.0, th.text_faint);
            }
            let icon_at = |bx: f32| (bx + (box_w - isz as f32) * 0.5, y + (rh - isz as f32) * 0.5);
            if hot || !fx.on {
                let (px, py) = icon_at(pow_x);
                let pc = if !fx.on {
                    th.wave_sel
                } else if self.hover == Hit::FxPower(i) {
                    th.text
                } else {
                    th.text_dim
                };
                icons.draw(cv, Icon::Power, px, py, isz, pc, false);
            }
            if hot {
                let (dx, dy) = icon_at(del_x);
                let dc = if self.hover == Hit::FxDelete(i) {
                    th.error
                } else {
                    th.text_dim
                };
                icons.draw(cv, Icon::Trash, dx, dy, isz, dc, false);
            }
        }
        if let Some(slot) = self.drop_slot(l) {
            let y = (y0 + slot as f32 * rh - self.chain_scroll).round();
            let lh = (2.0 * s).round().max(2.0);
            cv.fill_rect(
                c.x + (4.0 * s).round(),
                y - lh * 0.5,
                c.w - (8.0 * s).round(),
                lh,
                th.accent,
            );
        }
        cv.pop_clip();
        cv.pop_clip();
    }

    fn draw_msg(&self, cv: &mut Canvas<'_>, text: &mut TextCache, th: &Theme, l: &Layout, m: &str) {
        let s = l.s;
        let f = th.font_small();
        let tw = text.measure(f, m);
        let (bw, bh) = (tw + (20.0 * s).round(), (22.0 * s).round());
        let r = l.list;
        let x = (r.x + (r.w - bw) * 0.5).round().max(r.x);
        let y = (r.bottom() - bh - (10.0 * s).round()).round();
        cv.fill_round_rect(x, y, bw, bh, bh * 0.5, th.bar);
        cv.stroke_round_rect(x, y, bw, bh, bh * 0.5, 1.0, th.seam);
        text.draw_in(cv, f, x, y, bw, bh, m, th.text, Align::Center);
    }

    /// Les effets tenus suivent la souris.
    fn draw_ghost(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        cat: &Catalog,
        l: &Layout,
    ) {
        if !self.dragging_rows() {
            return;
        }
        let sel = self.selection();
        let label = match sel.as_slice() {
            [one] => format!(
                "+ {}",
                cat.plugins.get(*one).map_or("", |p| p.label.as_str())
            ),
            many => format!("+ {} effects", many.len()),
        };
        let s = l.s;
        let f = th.font_small();
        let tw = text.measure(f, &label).min((260.0 * s).round());
        let (bw, bh) = (tw + (14.0 * s).round(), (20.0 * s).round());
        let (mx, my) = self.mouse;
        let x = (mx + (12.0 * s).round()).min(l.w - bw).max(0.0);
        let y = (my + (10.0 * s).round()).min(l.h - bh).max(0.0);
        cv.fill_round_rect(x, y, bw, bh, (4.0 * s).round(), th.sel.fade(0.92));
        text.draw_in(
            cv,
            f,
            x + (7.0 * s).round(),
            y,
            tw,
            bh,
            &label,
            th.text,
            Align::Left,
        );
    }
}

/// La police des types et des numeros.
struct Font;

impl Font {
    fn mono_small(th: &Theme) -> cp_gfx::text::Font {
        cp_gfx::text::Font::new(1, th.s(11.0) as u16, 400)
    }
}
