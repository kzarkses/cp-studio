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

pub(crate) const CLEF: char = '\u{1D11E}';
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

thread_local! {
    /// Les formes deja rasterisees (tetes de note, fantomes), par sorte et
    /// taille : une partition vue de loin en pose des centaines par image.
    static SPRITES: std::cell::RefCell<std::collections::HashMap<(u8, u32, u32), cp_gfx::Mask>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Une forme centree en (cx, cy), posee au pixel pres ; `build` la trace
/// autour de (0, 0) la premiere fois.
fn sprite_at(
    cv: &mut Canvas<'_>,
    key: (u8, f32, f32),
    cx: f32,
    cy: f32,
    c: Color,
    build: impl FnOnce() -> Path,
) {
    SPRITES.with(|h| {
        let mut h = h.borrow_mut();
        let m = h
            .entry((key.0, key.1.to_bits(), key.2.to_bits()))
            .or_insert_with(|| cp_gfx::Mask::of_path(&build()));
        cv.draw_mask(
            &m.px,
            m.w,
            m.h,
            cx.round() as i32 + m.x,
            cy.round() as i32 + m.y,
            c,
        );
    });
}

/// Une tete de note centree en (cx, cy).
fn head_at(cv: &mut Canvas<'_>, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
    sprite_at(cv, (0, rx, ry), cx, cy, c, || head(0.0, 0.0, rx, ry));
}

/// Une tete creuse en pointilles (un fantome).
fn ghost_head_at(cv: &mut Canvas<'_>, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
    sprite_at(cv, (1, rx, ry), cx, cy, c, || {
        let n = 14;
        let mut dashes = Path::new();
        for k in (0..n).step_by(2) {
            let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
            dashes
                .move_to(rx * a0.cos(), ry * a0.sin())
                .line_to(rx * a1.cos(), ry * a1.sin());
        }
        let mut out = Path::new();
        dashes.stroke(1.1, &mut out);
        out
    });
}

/// Un cadre pointille d'un pixel, parcouru seulement dans le decoupage.
fn dashed_rect(cv: &mut Canvas<'_>, x: f32, y: f32, w: f32, h: f32, c: Color) {
    cv.dash_rect(x, y, w, h, 2.0, 2.0, c);
}

/// Un arc (liaison) d'un point a l'autre, bombe de `bulge` (negatif : vers le
/// haut).
fn arc(cv: &mut Canvas<'_>, x0: f32, y0: f32, x1: f32, y1: f32, bulge: f32, w: f32, c: Color) {
    let mut p = Path::new();
    p.move_to(x0, y0)
        .quad_to((x0 + x1) * 0.5, (y0 + y1) * 0.5 + bulge * 2.0, x1, y1);
    cv.stroke_path(&p, w, c);
}

impl Arr {
    /// La palette d'une note : ses marques en icones, groupees, celle de la
    /// note allumee ; une marque qui ne se pose pas est eteinte.
    fn draw_palette(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        p: crate::Palette,
    ) {
        use crate::Mark;
        let s = |v: f32| th.s(v);
        let z = s(26.0).round();
        let wide = |m: Mark| m.semis().is_some();
        let gap = s(7.0);
        let pad = s(4.0);
        let mut w = pad * 2.0;
        let mut last_g = None;
        for m in Mark::ALL {
            if last_g.is_some_and(|g| g != m.group()) {
                w += gap;
            }
            last_g = Some(m.group());
            w += if wide(m) { (z * 1.35).round() } else { z };
        }
        let h = z + pad * 2.0;
        let l = self.lay;
        let x = (p.x + s(6.0)).min(l.w - w - s(4.0)).max(0.0).round();
        let y = if p.y + s(14.0) + h < l.bottom {
            p.y + s(14.0)
        } else {
            (p.y - s(14.0) - h).max(l.top)
        }
        .round();
        cv.fill_rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, th.seam);
        cv.fill_rect(x, y, w, h, th.panel);
        // La note d'avant : monte-t-on vers celle-ci (« H ») ?
        let rising = self
            .prev_on_string(p.t, p.i, p.n)
            .and_then(|j| {
                let it = self.item(p.t, p.i)?;
                Some(it.doc.get(p.n)?.pitch > it.doc.get(j)?.pitch)
            })
            .unwrap_or(true);
        let mut bxx = x + pad;
        let mut last_g = None;
        for m in Mark::ALL {
            if last_g.is_some_and(|g| g != m.group()) {
                cv.fill_rect((bxx + gap * 0.5 - 0.5).round(), y + pad, 1.0, z, th.seam);
                bxx += gap;
            }
            last_g = Some(m.group());
            let bw = if wide(m) { (z * 1.35).round() } else { z };
            let on = self.mark_on(p.t, p.i, p.n, m);
            let ok = self.mark_ok(p.t, p.i, p.n, m);
            let hov = self.hover == Hit::Mark(m);
            if on {
                cv.fill_round_rect(bxx, y + pad, bw, z, s(3.0), th.accent.with_alpha(70));
            } else if hov && ok {
                cv.fill_round_rect(bxx, y + pad, bw, z, s(3.0), th.hover);
            }
            let c = if !ok {
                th.text_faint
            } else if on || hov {
                th.text
            } else {
                th.text_dim
            };
            mark_icon(cv, text, th, m, bxx, y + pad, bw, z, c, rising);
            if ok {
                push(bx, (x, y, w, h), bxx, y + pad, bw, z, Hit::Mark(m));
            }
            bxx += bw;
        }
    }
}

/// L'icone d'une marque, dessinee comme sur la tablature.
fn mark_icon(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    m: crate::Mark,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    c: Color,
    rising: bool,
) {
    use crate::Mark;
    let s = |v: f32| th.s(v);
    let (cx, cy) = (x + w * 0.5, y + h * 0.5);
    let lw = s(1.3);
    let dot = |cv: &mut Canvas<'_>, px: f32, py: f32| cv.fill_circle(px, py, s(2.4), c);
    let fl = Font::new(0, s(9.0) as u16, 700);
    match m {
        Mark::Plain => {
            let r = s(5.0);
            cv.line(cx - r, cy - r, cx + r, cy + r, lw, c);
            cv.line(cx - r, cy + r, cx + r, cy - r, lw, c);
        }
        Mark::Legato => {
            dot(cv, cx - s(6.0), cy + s(4.0));
            dot(cv, cx + s(6.0), cy + s(4.0));
            arc(cv, cx - s(6.0), cy, cx + s(6.0), cy, -s(3.5), lw, c);
            let lab = if rising { "H" } else { "P" };
            text.draw_in(
                cv,
                fl,
                cx - s(6.0),
                y + s(1.0),
                s(12.0),
                s(9.0),
                lab,
                c,
                Align::Center,
            );
        }
        Mark::Slide => {
            let d = if rising { s(4.0) } else { -s(4.0) };
            dot(cv, cx - s(7.0), cy + d);
            dot(cv, cx + s(7.0), cy - d);
            cv.line(cx - s(4.0), cy + d * 0.7, cx + s(4.0), cy - d * 0.7, lw, c);
        }
        Mark::InBelow | Mark::InAbove => {
            let d = if m == Mark::InBelow { s(5.0) } else { -s(5.0) };
            dot(cv, cx + s(5.0), cy);
            cv.line(cx - s(7.0), cy + d, cx + s(1.5), cy, lw, c);
        }
        Mark::Release => {
            let (x0, x1, top, bot) = (cx - s(7.0), cx + s(7.0), cy - s(6.0), cy + s(6.0));
            let mut p = Path::new();
            p.move_to(x0, bot)
                .quad_to(x0, top, cx, top)
                .quad_to(x1, top, x1, bot - s(3.0));
            cv.stroke_path(&p, lw, c);
            arrow_head(cv, x1, bot, s(3.5), false, c);
        }
        _ => {
            // Un bend : la fleche qui monte, et sa valeur.
            let (x0, x1) = (x + s(5.0), x + s(13.0));
            let (top, bot) = (cy - s(6.5), cy + s(6.5));
            let mut p = Path::new();
            p.move_to(x0, bot).quad_to(x1, bot, x1, top + s(3.0));
            cv.stroke_path(&p, lw, c);
            arrow_head(cv, x1, top, s(3.5), true, c);
            let lab = crate::bend_label(m.semis().unwrap_or(2.0));
            text.draw_in(
                cv,
                fl,
                x1 + s(1.0),
                y,
                x + w - x1 - s(2.0),
                h,
                &lab,
                c,
                Align::Center,
            );
        }
    }
}

/// Une pointe de fleche pleine, sa pointe en (x, y), vers le haut ou le bas.
fn arrow_head(cv: &mut Canvas<'_>, x: f32, y: f32, h: f32, up: bool, c: Color) {
    let d = if up { h } else { -h };
    let mut p = Path::new();
    p.move_to(x, y)
        .line_to(x - h * 0.6, y + d)
        .line_to(x + h * 0.6, y + d)
        .close();
    cv.fill_path(&p, c);
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

/// La hauteur d'une couche, le zoom vertical compris (audio et MIDI
/// seulement : la partition et la tab ont leur hauteur propre).
pub fn lane_hz(l: Lane, vz: f32) -> f32 {
    match l {
        Lane::Audio | Lane::Midi => (lane_h(l) * vz).round(),
        _ => lane_h(l),
    }
}

/// La hauteur que demande l'en-tete d'une piste (a 96 DPI).
const HEAD_MIN_H: f32 = 142.0;
pub(crate) const TRACK_SEP: f32 = 1.0;
/// Les curseurs de l'inspecteur : ou commence la glissiere, et la place
/// laissee a droite pour la valeur (a 96 DPI).
pub(crate) const SLIDER_X: f32 = 80.0;
pub(crate) const SLIDER_R: f32 = 62.0;

/// La hauteur d'une piste (a l'echelle).
pub fn track_h(tr: &crate::Track, s: f32, vz: f32) -> f32 {
    let lanes: f32 = Lane::ALL
        .iter()
        .filter(|&&l| tr.head.lanes.on(l))
        .map(|&l| lane_hz(l, vz) + 1.0)
        .sum();
    (lanes.max(HEAD_MIN_H) * s).round()
}

/// Le debut et la largeur des faders d'un en-tete.
pub fn fader_span(a: &Arr, _t: usize) -> (f32, f32) {
    let s = a.scale;
    let fx = (12.0 + 22.0) * s;
    (fx, (a.lay.head_w - fx - 14.0 * s).max(10.0))
}

/// La couleur d'une corde (MIDI).
fn string_color(k: usize) -> Color {
    const C: [u32; 6] = [0xa8c97f, 0xde9a86, 0xc99ad6, 0x8fb0de, 0x7fc6b4, 0xd9b45a];
    cp_gfx::hex(C[k.min(5)])
}

/// La geometrie de l'axe du temps.
#[derive(Clone, Copy)]
pub(crate) struct Geo {
    pub(crate) sx: f32,
    pub(crate) scroll: f64,
    pub(crate) ppq: f32,
    pub(crate) s: f32,
    /// La zone visible (cibles et dessins).
    pub(crate) clip: (f32, f32, f32, f32),
    /// Une note est posee a `pad` de sa date (elle ne touche pas la barre de
    /// mesure).
    pub(crate) pad: f32,
    /// Le trait fin (lignes supplementaires, liaisons, bends) : un pixel a
    /// l'ecran, plus sur une page imprimee.
    pub(crate) lw: f32,
    /// Ce qui se dessine encore hors de la zone visible (une note a cheval
    /// sur le bord). Rien sur une page : la mesure voisine n'y est pas.
    pub(crate) margin: f32,
    /// Sur une page, chaque mesure a sa largeur : l'axe passe par ces
    /// points (noires, x). Vide a l'ecran : l'axe est droit.
    pub(crate) warp: Warp,
}

/// Un axe par morceaux : `n` points (noires croissantes, x croissants),
/// droit entre deux points et prolonge aux bouts.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Warp {
    pub(crate) n: usize,
    pub(crate) q: [f64; 8],
    pub(crate) x: [f32; 8],
}

impl Warp {
    /// Les points (8 au plus ; les suivants sont ignores).
    pub(crate) fn new(pts: &[(f64, f32)]) -> Warp {
        let mut w = Warp::default();
        for &(q, x) in pts.iter().take(8) {
            w.q[w.n] = q;
            w.x[w.n] = x;
            w.n += 1;
        }
        w
    }

    fn x(&self, q: f64) -> Option<f32> {
        if self.n < 2 {
            return None;
        }
        // Le segment qui contient q (le premier ou le dernier au-dela).
        let k = (1..self.n - 1)
            .find(|&k| q < self.q[k])
            .unwrap_or(self.n - 1);
        let (q0, q1) = (self.q[k - 1], self.q[k]);
        let (x0, x1) = (self.x[k - 1], self.x[k]);
        Some(x0 + ((q - q0) / (q1 - q0).max(1e-9)) as f32 * (x1 - x0))
    }
}

impl Geo {
    /// L'axe de la fenetre.
    fn screen(sx: f32, scroll: f64, ppq: f32, s: f32, clip: (f32, f32, f32, f32)) -> Geo {
        Geo {
            sx,
            scroll,
            ppq,
            s,
            clip,
            pad: 9.0 * s,
            lw: 1.0,
            margin: 40.0 * s,
            warp: Warp::default(),
        }
    }

    pub(crate) fn x(&self, q: f64) -> f32 {
        self.warp
            .x(q)
            .unwrap_or(self.sx + ((q - self.scroll) as f32) * self.ppq)
    }

    fn visible(&self, x: f32) -> bool {
        x > self.clip.0 - self.margin && x < self.clip.0 + self.clip.2 + self.margin
    }
}

impl Arr {
    fn layout(&mut self, w: f32, h: f32, scale: f32) {
        let s = |v: f32| (v * scale).round();
        let bar_h = s(36.0);
        let hs_h = s(12.0);
        let panel_w = if w > s(980.0) { s(240.0) } else { 0.0 };
        let panel_x = w - panel_w;
        let top = bar_h + 2.0;
        let bottom = h;
        let head_w = s(210.0);
        let label_x = head_w + 2.0;
        let sx = label_x + s(40.0);
        let right = if panel_w > 0.0 { panel_x - 2.0 } else { w };
        let vz = self.vzoom;
        let content_h = self
            .tracks
            .iter()
            .map(|t| track_h(t, scale, vz) + s(TRACK_SEP))
            .sum::<f32>();
        self.lay = Lay {
            w,
            h,
            bar_h,
            hs_h,
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
        let l = self.lay;
        cv.fill_rect(0.0, 0.0, w, h, th.bg);
        self.draw_bar(cv, text, icons, th, &mut bx);
        cv.fill_rect(0.0, l.bar_h, w, 1.0, th.seam);
        self.draw_tracks(cv, text, icons, th, &mut bx);
        if l.panel_w > 0.0 {
            cv.fill_rect(l.panel_x - 1.0, l.top, 1.0, l.bottom - l.top, th.seam);
            self.draw_panel(cv, text, icons, th, &mut bx);
        }
        // La palette d'une note, par-dessus tout.
        if let Some(p) = self.palette {
            self.draw_palette(cv, text, th, &mut bx, p);
        }
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

    /// Un bouton icone, carre. `fill` : plein de la couleur d'accent.
    fn icon_button(
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
        fill: bool,
    ) -> f32 {
        let hov = self.hover == hit;
        let bg = match (fill, hov) {
            (true, true) => th.accent.mix(th.text, 0.2),
            (true, false) => th.accent,
            (false, true) => th.hover.mix(th.text, 0.06),
            (false, false) => th.field,
        };
        cv.fill_round_rect(x, y, h, h, th.s(3.0), bg);
        let isz = th.s(15.0) as u16;
        let c = if fill {
            th.bg
        } else if on {
            th.accent
        } else {
            th.text_dim
        };
        let o = ((h - isz as f32) * 0.5).round();
        icons.draw(cv, icon, x + o, y + o, isz, c, false);
        bx.push((x, y, h, h, hit));
        h
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
        x += isz as f32 + s(16.0);
        let by = ((l.bar_h - s(24.0)) * 0.5).floor();
        let bh = s(24.0);
        // L'ecoute : un casque, puis les trois choix.
        let hsz = s(14.0) as u16;
        icons.draw(
            cv,
            Icon::Headphones,
            x,
            ((l.bar_h - hsz as f32) * 0.5).round(),
            hsz,
            th.text_faint,
            false,
        );
        x += hsz as f32 + s(7.0);
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
        ) + s(10.0);
        x += self.icon_button(
            cv,
            icons,
            th,
            bx,
            x,
            by,
            bh,
            Icon::Locate,
            self.follow,
            Hit::Follow,
            false,
        ) + s(4.0);
        // La transcription automatique des prises neuves (coupee : sur
        // demande, la baguette de chaque prise).
        x += self.icon_button(
            cv,
            icons,
            th,
            bx,
            x,
            by,
            bh,
            Icon::Wand,
            self.auto,
            Hit::Auto,
            false,
        );
        // A droite : l'aide, la piste neuve, l'enregistrement en cours.
        let mut rx = l.w - s(10.0) - bh;
        self.icon_button(
            cv,
            icons,
            th,
            bx,
            rx,
            by,
            bh,
            Icon::Help,
            false,
            Hit::Help,
            false,
        );
        rx -= s(6.0) + bh;
        self.icon_button(
            cv,
            icons,
            th,
            bx,
            rx,
            by,
            bh,
            Icon::Plus,
            false,
            Hit::AddTrack,
            true,
        );
        let fa = Font::new(0, s(12.0) as u16, 700);
        if let Some(r) = self.rec.first() {
            let name = self
                .tracks
                .get(r.track)
                .map(|t| t.head.name.clone())
                .unwrap_or_default();
            let lab = format!("\u{25cf}  {name}");
            let rw = text.measure(fa, &lab).ceil() + s(20.0);
            rx -= s(10.0) + rw;
            cv.fill_round_rect(rx, by, rw, bh, s(3.0), th.field);
            text.draw_in(cv, fa, rx, by, rw, bh, &lab, th.error, Align::Center);
        }
        // Ce qui se passe : une transcription, un message du module.
        let msg = self.busy.clone().unwrap_or_else(|| self.status.clone());
        let mx = x + s(16.0);
        let mw = rx - s(12.0) - mx;
        if !msg.is_empty() && mw > s(40.0) {
            text.draw_in(
                cv,
                Font::new(0, s(11.0) as u16, 400),
                mx,
                0.0,
                mw,
                l.bar_h,
                &msg,
                if self.busy.is_some() {
                    th.accent
                } else {
                    th.text_dim
                },
                Align::Right,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Les pistes
    // -----------------------------------------------------------------------

    fn draw_tracks(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let l = self.lay;
        let s = |v: f32| th.s(v);
        let ry = l.top;
        let content_r = l.sx + l.sw;
        // La fin de la zone des pistes : la barre horizontale est dessous.
        let tb = l.bottom - l.hs_h;
        // --- la regle (et le coin des en-tetes, vide)
        cv.fill_rect(0.0, ry, l.head_w, l.ruler_h, th.panel);
        cv.fill_rect(l.head_w, ry, 1.0, l.bottom - ry, th.seam);
        cv.fill_rect(l.label_x, ry, l.w, l.ruler_h, th.rp.ruler_bg);
        let geo = Geo::screen(
            l.sx,
            self.scroll,
            l.ppq,
            th.scale,
            (l.sx, ry, l.sw, tb - ry),
        );
        let bq = self.bar_qn();
        let q0 = self.scroll;
        let q1 = self.scroll + self.view_qn();
        let first = (q0 / bq).floor().max(0.0) as i64;
        let last = (q1 / bq).ceil() as i64;
        let fb = Font::new(0, s(11.0) as u16, 700);
        let ft = Font::new(1, s(9.5) as u16, 400);
        // Les mesures numerotees : une sur deux, quatre... quand elles serrent.
        let bar_px = bq as f32 * l.ppq;
        let step = [1i64, 2, 4, 8, 16, 32]
            .into_iter()
            .find(|&k| bar_px * k as f32 >= s(34.0))
            .unwrap_or(64);
        if cv.push_clip(l.sx, ry, l.sw, l.ruler_h) {
            if let Some((a, b)) = self.range {
                let (xa, xb) = (geo.x(a), geo.x(b));
                cv.fill_rect(xa, ry, xb - xa, l.ruler_h, th.accent.with_alpha(40));
                cv.fill_rect(xa, ry + l.ruler_h - s(3.0), xb - xa, s(3.0), th.accent);
            }
            for m in first..=last {
                let q = m as f64 * bq;
                let x = geo.x(q).round();
                let major = m % step == 0;
                cv.fill_rect(
                    x,
                    ry + if major { s(3.0) } else { l.ruler_h - s(9.0) },
                    1.0,
                    if major { l.ruler_h - s(3.0) } else { s(9.0) },
                    th.text_faint,
                );
                if !major {
                    continue;
                }
                let nw = text.draw(
                    cv,
                    fb,
                    x + s(4.0),
                    ry + s(14.0),
                    &(m + 1).to_string(),
                    th.text,
                );
                // Le temps, quand il y a la place.
                if bar_px * step as f32 > s(96.0) {
                    let t = q * 60.0 / self.bpm.max(1.0);
                    text.draw(
                        cv,
                        ft,
                        x + s(10.0) + nw,
                        ry + s(14.0),
                        &format!("{}:{:04.1}", (t / 60.0) as i64, t % 60.0),
                        th.text_faint,
                    );
                }
                if step == 1 {
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
        }
        cv.pop_clip();
        push(bx, geo.clip, l.sx, ry, l.sw, l.ruler_h, Hit::Ruler);
        cv.fill_rect(0.0, ry + l.ruler_h, l.w - l.panel_w, 1.0, th.seam);

        // --- les pistes
        let top = ry + l.ruler_h + 1.0;
        let vis = (0.0, top, l.panel_x, (tb - top).max(0.0));
        let mut y = top - self.vscroll;
        let vz = self.vzoom;
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
            let mut tops: Vec<(f32, f32)> = Vec::new();
            for t in 0..self.tracks.len() {
                let h = track_h(&self.tracks[t], th.scale, vz);
                tops.push((y, y + h));
                if y + h >= top && y <= tb {
                    self.draw_header(cv, text, icons, th, bx, t, y, h, vis);
                    self.draw_lanes(cv, text, icons, th, bx, t, y, h, geo, vis);
                }
                y += h;
                cv.fill_rect(0.0, y, l.panel_x, s(TRACK_SEP), th.rp.tr_div);
                y += s(TRACK_SEP);
            }
            // La guitare qu'on glisse : la ligne ou elle tombera.
            if let Some((mt, to)) = self.track_move() {
                if let Some(&(a, b)) = tops.get(to) {
                    let yy = if to < mt { a } else { b };
                    cv.fill_rect(0.0, (yy - 1.0).round(), l.panel_x, 2.0, th.accent);
                }
            }
            // Le vide sous les pistes : celui de l'arrangement.
            if y < tb {
                cv.fill_rect(l.label_x, y, content_r - l.label_x, tb - y, th.rp.arr_bg);
            }
            // La tete de lecture et les curseurs sont des traits, poses
            // par-dessus la scene (`strokes`).
            self.tracks_bottom = y;
            // Le passage choisi, a travers toutes les pistes (la selection de
            // temps du theme ; son mode « incrustation » rendu en voile clair).
            if let Some((a, b)) = self.range {
                let (xa, xb) = (geo.x(a).max(l.sx).round(), geo.x(b).min(content_r).round());
                if xb > xa {
                    let ts = th.rp.timesel;
                    cv.fill_rect(
                        xa,
                        top,
                        xb - xa,
                        (y - top).max(0.0),
                        ts.with_alpha(ts.a / 3),
                    );
                }
            }
            // Le lasso (les couleurs du theme de REAPER).
            if let Some((xa, ya, xb, yb)) = self.marquee_rect() {
                let (xa, ya) = (xa.round(), ya.round());
                let (w, h) = ((xb.round() - xa).max(1.0), (yb.round() - ya).max(1.0));
                cv.fill_rect(xa, ya, w, h, th.rp.marquee_fill);
                cv.stroke_rect(
                    xa + 0.5,
                    ya + 0.5,
                    w - 1.0,
                    h - 1.0,
                    1.0,
                    th.rp.marquee_outline,
                );
            }
            // Une prise qu'on glisse (elle se dessine a sa nouvelle place) :
            // la ligne ou tombe son debut, a travers les pistes.
            if let Some((_, _, q, _, _)) = self.item_drag() {
                let x = geo.x(q).round();
                if x >= l.sx && x <= content_r {
                    cv.fill_rect(x, top, 1.0, (y - top).max(0.0), th.accent);
                }
            }
        }
        cv.pop_clip();
        // La barre de defilement vertical.
        let view = (tb - top).max(1.0);
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
        // La barre de defilement horizontal, sous les pistes.
        let hy = tb;
        cv.fill_rect(0.0, hy, l.panel_x.min(l.w), l.hs_h, th.bg);
        if !self.tracks.is_empty() {
            let (tx, tw) = self.hthumb();
            let th_h = s(6.0);
            let ty = hy + ((l.hs_h - th_h) * 0.5).round();
            cv.fill_round_rect(l.sx, ty, l.sw, th_h, s(3.0), th.field);
            let hot = self.hover == Hit::HScroll
                || matches!(self.drag, Some(crate::Drag::HScroll { .. }));
            cv.fill_round_rect(
                tx,
                ty,
                tw,
                th_h,
                s(3.0),
                if hot { th.text_dim } else { th.text_faint },
            );
            bx.push((l.sx, hy, l.sw, l.hs_h, Hit::HScroll));
        }
        // Rien encore : un seul geste possible, au milieu.
        if self.tracks.is_empty() {
            let bs = s(56.0);
            let cx = l.sx + (l.sw - bs) * 0.5;
            let cy = top + ((tb - top) - bs) * 0.4;
            let hov = self.hover == Hit::AddTrack;
            cv.fill_round_rect(
                cx,
                cy,
                bs,
                bs,
                s(8.0),
                if hov { th.hover } else { th.field },
            );
            let gs = s(26.0) as u16;
            icons.draw(
                cv,
                Icon::Guitar,
                cx + (bs - gs as f32) * 0.5,
                cy + (bs - gs as f32) * 0.5 - s(2.0),
                gs,
                th.text_dim,
                false,
            );
            let ps = s(14.0) as u16;
            cv.fill_circle(cx + bs - s(12.0), cy + bs - s(12.0), s(9.0), th.accent);
            icons.draw(
                cv,
                Icon::Plus,
                cx + bs - s(12.0) - ps as f32 * 0.5,
                cy + bs - s(12.0) - ps as f32 * 0.5,
                ps,
                th.bg,
                false,
            );
            bx.push((cx, cy, bs, bs, Hit::AddTrack));
        }
    }

    fn draw_header(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
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
        let fname = Font::new(0, s(13.0) as u16, 700);
        let fsm = Font::new(0, s(10.5) as u16, 400);
        let nw = -s(7.0);
        let tun = TUNINGS[hd.tuning.min(TUNINGS.len() - 1)].0.to_string();
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
        // Les deux volumes (une icone pour chacun), et le vumetre de la
        // guitare. La valeur vient au survol.
        let (fx, fw) = fader_span(self, t);
        let rows: [(Icon, f32, bool, f32); 2] = [
            (Icon::AudioLines, hd.vol_db, false, y + s(64.0)),
            (Icon::Music, hd.tab_vol_db, true, y + s(88.0)),
        ];
        for (icon, db, tab, fy) in rows {
            if tab && hd.tab_guid.is_none() {
                continue;
            }
            let fh = s(18.0);
            let hot = self.hover == Hit::Fader { t, tab };
            let isz = s(13.0) as u16;
            icons.draw(
                cv,
                icon,
                x0,
                (fy + (fh - isz as f32) * 0.5).round(),
                isz,
                if hot { th.text_dim } else { th.text_faint },
                false,
            );
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
            // Le zero : un trait.
            let zx = (fx + Arr::fader_pos(0.0) * fw).round();
            cv.fill_rect(zx, cy - s(5.0), 1.0, s(10.0), th.text_faint.fade(0.7));
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
                let my = fy + fh + s(1.0);
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
        // Les couches montrees : audio, MIDI, partition, tab (un bouton
        // chacune ; il en reste toujours une).
        let by = y + s(116.0);
        let bw = s(24.0);
        let bh = s(20.0);
        let mut bxx = s(12.0);
        for (lane, icon) in [
            (Lane::Audio, Icon::AudioLines),
            (Lane::Midi, Icon::Piano),
            (Lane::Score, Icon::Music),
            (Lane::Tab, Icon::Guitar),
        ] {
            let on = hd.lanes.on(lane);
            let hit = Hit::LaneToggle { t, lane };
            let hov = self.hover == hit;
            cv.fill_round_rect(
                bxx,
                by,
                bw,
                bh,
                s(3.0),
                if on {
                    th.accent.with_alpha(if hov { 70 } else { 45 })
                } else if hov {
                    th.hover
                } else {
                    th.field
                },
            );
            let isz = s(13.0) as u16;
            icons.draw(
                cv,
                icon,
                (bxx + (bw - isz as f32) * 0.5).round(),
                (by + (bh - isz as f32) * 0.5).round(),
                isz,
                if on { th.accent } else { th.text_faint },
                false,
            );
            push(bx, vis, bxx, by, bw, bh, hit);
            bxx += bw + s(4.0);
        }
    }

    fn draw_lanes(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
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
        let staff_c = th.text_dim.fade(0.75);
        let vz = self.vzoom;
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
            let lh = s(lane_hz(lane, vz));
            lane_ys.push((lane, y, lh));
            y += lh + 1.0;
        }
        let rec: Vec<crate::Rec> = self.rec.iter().filter(|r| r.track == t).cloned().collect();
        let playhead = self.playhead;
        // Le fond des pistes de REAPER (une sur deux, la choisie plus claire).
        let k = t % 2;
        let track_bg = if self.sel.is_some_and(|x| x.t == t) {
            th.rp.tr_sel_bg[k]
        } else {
            th.rp.tr_bg[k]
        };
        for &(lane, ly, lh) in &lane_ys {
            // Le fond et le nom de la couche.
            cv.fill_rect(l.label_x, ly, right - l.label_x, lh, track_bg);
            // La grille de l'arrangement, sous les items : mesures et temps.
            // Le decoupage pose seulement pour l'audio et le MIDI, et retire
            // seulement pour eux : retire pour chaque couche, il defaisait
            // celui de la zone a redessiner, et la suite de l'image se
            // dessinait par-dessus toute la fenetre (la selection de temps
            // voilee une fois de plus a chaque survol, 2026-11-22).
            if matches!(lane, Lane::Audio | Lane::Midi) {
                if cv.push_clip(l.sx, ly, l.sw, lh) {
                    let beat = 4.0 / self.sig.1.max(1) as f64;
                    let beat_px = beat as f32 * l.ppq;
                    // La grille de REAPER (sa division, elargie sous 8 px comme
                    // lui) : mesures, temps, et ses lignes entre les temps.
                    let step = self.grid.map(|g| g.step(l.ppq.max(1e-3)));
                    for m in first..=last {
                        let q = m as f64 * bq;
                        cv.fill_rect(geo.x(q).round(), ly, 1.0, lh, th.rp.grid_bar);
                        if let Some(st) = step.filter(|&st| st < bq) {
                            let n = (bq / st).round() as i64;
                            for k in 1..n {
                                let qq = q + k as f64 * st;
                                let on_beat = ((qq / beat).round() * beat - qq).abs() < 1e-6;
                                let c = if on_beat {
                                    th.rp.grid_beat
                                } else {
                                    th.rp.grid_sub
                                };
                                cv.fill_rect(geo.x(qq).round(), ly, 1.0, lh, c);
                            }
                        } else if step.is_none() && beat_px >= s(12.0) {
                            for b in 1..self.sig.0.max(1) {
                                let xb = geo.x(q + b as f64 * beat).round();
                                cv.fill_rect(xb, ly, 1.0, lh, th.rp.grid_beat);
                            }
                        }
                    }
                }
                cv.pop_clip();
            }
            if matches!(lane, Lane::Audio | Lane::Midi) {
                cv.fill_rect(
                    l.label_x,
                    ly,
                    l.sx - l.label_x,
                    lh,
                    th.panel.mix(th.bg, 0.5),
                );
            }
            // L'audio et le MIDI : une icone, pas de mot.
            let icon = match lane {
                Lane::Audio => Some(Icon::AudioLines),
                Lane::Midi => Some(Icon::Piano),
                _ => None,
            };
            if let Some(ic) = icon {
                let isz = s(13.0) as u16;
                icons.draw(
                    cv,
                    ic,
                    (l.label_x + (l.sx - l.label_x - isz as f32) * 0.5).round(),
                    (ly + (lh - isz as f32) * 0.5).round(),
                    isz,
                    th.text_faint,
                    false,
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
                            l.label_x + s(4.0),
                            sy + k as f32 * gap,
                            right - l.label_x - s(4.0),
                            1.0,
                            staff_c,
                        );
                    }
                    let fc = Font::new(3, s(40.0) as u16, 400);
                    text.draw(
                        cv,
                        fc,
                        l.label_x + s(6.0),
                        sy + 4.0 * gap + s(8.0),
                        &CLEF.to_string(),
                        th.text,
                    );
                    text.draw(
                        cv,
                        Font::new(0, s(8.0) as u16, 700),
                        l.label_x + s(15.0),
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
                            Font::new(0, s(9.5) as u16, 400),
                            l.label_x + s(4.0),
                            yy - s(6.0),
                            l.sx - l.label_x - s(14.0),
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
            // La prise qu'on glisse : elle quitte sa place (sauf une copie) et
            // se dessine, entiere, la ou elle irait.
            let drag = self.item_drag();
            let lay = ItemLay {
                lanes: &lane_ys,
                shown: lanes,
                nstr,
                clip: content_clip,
                y0,
                y1: y,
                right,
            };
            let fade = self.fade_preview();
            let trim = self.trim_preview();
            // La selection de REAPER, et celle qu'aurait le lasso en cours.
            let lasso = self
                .marquee_rect()
                .map(|r| self.marquee_hits(r))
                .unwrap_or_default();
            let picks: Vec<(bool, bool)> = (0..self.tracks[t].items.len())
                .map(|i| self.shown_pick(t, i, &lasso))
                .collect();
            for (i, it) in self.tracks[t].items.iter_mut().enumerate() {
                if drag.is_some_and(|d| d.0 == t && d.1 == i && !d.4) {
                    continue;
                }
                let is_sel = sel.is_some_and(|x| x.t == t && x.i == Some(i));
                let sel_n = sel.filter(|x| x.t == t && x.i == Some(i)).and_then(|x| x.n);
                // Le fondu qu'on tire se dessine a sa longueur du moment.
                let kept = (it.fade_in, it.fade_out);
                if let Some((ft, fi, fin, len)) = fade {
                    if (ft, fi) == (t, i) {
                        if fin {
                            it.fade_in = len;
                        } else {
                            it.fade_out = len;
                        }
                    }
                }
                draw_item(
                    cv, text, icons, th, bx, it, t, i, geo, &lay, is_sel, sel_n, hover, picks[i],
                );
                (it.fade_in, it.fade_out) = kept;
                // Le bord qu'on tire : ce qui part est voile, ce qui vient est
                // cadre, et une ligne marque le bord.
                if let Some((tt, ti, left, q)) = trim {
                    if (tt, ti) == (t, i) {
                        let (xa, xb) = (geo.x(it.qn0).round(), geo.x(it.qn1).round());
                        let xq = geo.x(q).round();
                        let (h0, h1) = (lay.y0, lay.y1);
                        let (a, b) = if left { (xa, xq) } else { (xq, xb) };
                        let shrink = if left { xq > xa } else { xq < xb };
                        if shrink {
                            cv.fill_rect(a, h0, b - a, h1 - h0, Color::BLACK.with_alpha(110));
                        } else {
                            let (a, b) = (a.min(b), a.max(b));
                            cv.dash_rect(a, h0 + 1.0, b - a, h1 - h0 - 2.0, 2.0, 2.0, th.accent);
                        }
                        cv.fill_rect(xq, h0, 1.0, h1 - h0, th.accent);
                    }
                }
            }
            if let Some((dt, di, q, to, _)) = drag {
                if to == t {
                    // La prise qu'on glisse est choisie (la couche saisie),
                    // comme dans l'arrangement.
                    let fl = self.focus_lane;
                    let grab = (fl == Lane::Audio, fl == Lane::Midi);
                    if let Some(it) = self.tracks.get_mut(dt).and_then(|x| x.items.get_mut(di)) {
                        let moved = Geo {
                            scroll: geo.scroll - (q - it.qn0),
                            ..geo
                        };
                        let mut none: Vec<HBox> = Vec::new();
                        draw_item(
                            cv,
                            text,
                            icons,
                            th,
                            &mut none,
                            it,
                            dt,
                            di,
                            moved,
                            &lay,
                            true,
                            None,
                            Hit::None,
                            grab,
                        );
                    }
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
                        "\u{25cf}",
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
        let tx0 = x0 + s(SLIDER_X);
        let tw = (l.w - tx0 - s(SLIDER_R)).max(10.0);
        let rh = s(22.0);
        text.draw_in(
            cv,
            f,
            x0 + pad,
            y,
            s(SLIDER_X - 16.0),
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

    /// Une case a cocher et son mot, dans `x .. x + w`.
    fn check_at(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        on: bool,
        hit: Hit,
    ) {
        let s = |v: f32| th.s(v);
        let bs = s(13.0);
        let by = y + s(4.0);
        cv.fill_round_rect(
            x,
            by,
            bs,
            bs,
            s(2.0),
            if self.hover == hit {
                th.hover
            } else {
                th.field
            },
        );
        if on {
            text.draw_in(
                cv,
                Font::new(0, s(11.0) as u16, 700),
                x,
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
            x + bs + s(7.0),
            y,
            (w - bs - s(7.0)).max(0.0),
            s(21.0),
            label,
            th.text,
            Align::Left,
        );
        bx.push((x, y, w, s(21.0), hit));
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

    /// Un trait de separation dans l'inspecteur ; `tag` : un mot a droite.
    fn rule(
        &self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        y: f32,
        tag: Option<&str>,
    ) {
        let l = self.lay;
        let pad = th.s(12.0);
        cv.fill_rect(l.panel_x + pad, y, l.panel_w - 2.0 * pad, 1.0, th.seam);
        if let Some(t) = tag {
            let f = Font::new(0, th.s(10.0) as u16, 700);
            let tw = text.measure(f, t).ceil() + th.s(10.0);
            let tx = l.w - pad - tw;
            cv.fill_round_rect(tx, y - th.s(7.0), tw, th.s(14.0), th.s(3.0), th.panel);
            text.draw_in(
                cv,
                f,
                tx,
                y - th.s(7.0),
                tw,
                th.s(14.0),
                t,
                th.accent,
                Align::Center,
            );
        }
    }

    fn draw_panel(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
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
        let fh = Font::new(0, s(12.5) as u16, 700);
        let f = Font::new(0, s(12.0) as u16, 400);
        let mut y = l.top + s(10.0);
        if let Some((t, i)) = self.sel_item() {
            let (name, takes, active) = {
                let it = &self.tracks[t].items[i];
                (it.name.clone(), it.takes.clone(), it.active)
            };
            // Le nom, et les gestes de l'item en icones.
            let bs = s(24.0);
            let mut ix = x0 + w - pad - bs;
            for (icon, hit) in [
                (Icon::Trash, Hit::DeleteItem),
                (Icon::Download, Hit::Export),
                (Icon::File, Hit::ExportPdf),
                (Icon::Refresh, Hit::Retranscribe),
            ] {
                self.icon_button(cv, icons, th, bx, ix, y, bs, icon, false, hit, false);
                ix -= bs + s(4.0);
            }
            text.draw_in(
                cv,
                fh,
                x0 + pad,
                y,
                (ix - x0 - pad).max(0.0),
                bs,
                &name,
                th.text,
                Align::Left,
            );
            y += bs + s(10.0);
            // Les prises, quand il y en a plusieurs.
            if takes.len() > 1 {
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
                        format!("{}", k + 1)
                    } else {
                        format!("{}  {}", k + 1, tk.name)
                    };
                    text.draw_in(
                        cv,
                        f,
                        x0 + pad + s(8.0),
                        y,
                        w - pad * 2.0 - s(28.0),
                        rh - s(2.0),
                        &lab,
                        if on { th.text } else { th.text_dim },
                        Align::Left,
                    );
                    // Transcrite : un point plein.
                    let cx = x0 + w - pad - s(10.0);
                    let cy = y + (rh - s(2.0)) * 0.5;
                    if tk.done {
                        cv.fill_circle(cx, cy, s(3.0), th.accent.fade(0.8));
                    } else {
                        cv.fill_circle(cx, cy, s(3.0), th.text_faint.fade(0.5));
                    }
                    bx.push((x0 + pad, y, w - pad * 2.0, rh - s(2.0), hit));
                    y += rh;
                }
                y += s(6.0);
            }
            // La detection de cet item (ou du passage choisi).
            let det = self.tracks[t].items[i].detect.clone();
            y += s(6.0);
            self.rule(cv, text, th, y, self.range.is_some().then_some("Passage"));
            y += s(10.0);
            y += self.slider(
                cv,
                text,
                th,
                bx,
                y,
                "Onsets",
                (det.onset - 0.05) / 0.9,
                &format!("{:.2}", det.onset),
                Hit::Slider(0),
                Some((0.5 - 0.05) / 0.9),
            );
            y += self.slider(
                cv,
                text,
                th,
                bx,
                y,
                "Notes",
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
                "Shortest",
                ((det.min_ms - 20.0) / 380.0) as f32,
                &format!("{:.0} ms", det.min_ms),
                Hit::Slider(2),
                None,
            );
            y += s(2.0);
            // Legato et fantomes cote a cote.
            let half = (w - 2.0 * pad) * 0.5;
            self.check_at(
                cv,
                text,
                th,
                bx,
                x0 + pad,
                y,
                half,
                "Legato",
                det.legato,
                Hit::Check(0),
            );
            self.check_at(
                cv,
                text,
                th,
                bx,
                x0 + pad + half,
                y,
                half,
                "Ghosts",
                det.ghosts,
                Hit::Check(1),
            );
            y += s(26.0);
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
            // Ce qui reste a trancher : les fantomes, les notes injouables.
            let (gh, dropped) = {
                let it = &self.tracks[t].items[i];
                (it.ghosts.len(), it.dropped)
            };
            if det.ghosts && gh > 0 {
                let lab = format!("Keep {gh} ghost{}", if gh == 1 { "" } else { "s" });
                self.pbutton(cv, text, th, bx, x0 + pad, y, &lab, Hit::KeepGhosts, false);
                y += s(30.0);
            }
            if dropped > 0 {
                text.draw(
                    cv,
                    Font::new(0, s(11.0) as u16, 400),
                    x0 + pad,
                    y + s(10.0),
                    &format!("\u{26a0} {dropped} unplayable"),
                    th.error,
                );
                y += s(20.0);
            }
            y += s(6.0);
        }
        // Le son de la tab de la piste choisie.
        if let Some(t) = self.sel_track() {
            if let Some(sy) = self.tracks[t].head.synth {
                if y > l.top + s(12.0) {
                    self.rule(cv, text, th, y, None);
                    y += s(10.0);
                }
                // Les quatre sons.
                let sh = s(22.0);
                let segw = (w - 2.0 * pad) / 4.0;
                let mut xx = x0 + pad;
                for snd in Sound::ALL {
                    let on = sy.sound == snd;
                    let hit = Hit::SoundPick { t, s: snd };
                    let bgc = if on {
                        th.bg
                    } else if self.hover == hit {
                        th.hover
                    } else {
                        th.field
                    };
                    cv.fill_rect(xx, y, segw - 1.0, sh, bgc);
                    text.draw_in(
                        cv,
                        Font::new(0, s(11.0) as u16, if on { 700 } else { 400 }),
                        xx,
                        y,
                        segw - 1.0,
                        sh,
                        snd.name(),
                        if on { th.accent } else { th.text_dim },
                        Align::Center,
                    );
                    bx.push((xx, y, segw, sh, hit));
                    xx += segw;
                }
                y += sh + s(8.0);
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
                y += s(2.0);
                self.check_at(
                    cv,
                    text,
                    th,
                    bx,
                    x0 + pad,
                    y,
                    w - 2.0 * pad,
                    "Per string",
                    sy.mono,
                    Hit::SynthMono,
                );
            }
        }
        cv.pop_clip();
    }
}

/// L'item dans la couche audio : son cadre, son nom, sa forme d'onde (de
/// l'analyse), ou ce qui l'attend.
/// Ou se dessinent les couches d'une piste (pour un item).
struct ItemLay<'a> {
    lanes: &'a [(Lane, f32, f32)],
    shown: crate::Lanes,
    nstr: usize,
    clip: (f32, f32, f32, f32),
    /// Le haut et le bas des couches, le bord droit du contenu.
    y0: f32,
    y1: f32,
    right: f32,
}

/// Un item dans toutes les couches de sa piste : l'audio, le MIDI, la
/// partition, la tab, et leurs cibles.
fn draw_item(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    icons: &mut IconCache,
    th: &Theme,
    bx: &mut Vec<HBox>,
    it: &mut Item,
    t: usize,
    i: usize,
    geo: Geo,
    lay: &ItemLay<'_>,
    is_sel: bool,
    sel_n: Option<usize>,
    hover: Hit,
    pick: (bool, bool),
) {
    let s = |v: f32| th.s(v);
    let (xa, xb) = (geo.x(it.qn0), geo.x(it.qn1));
    if xb < geo.sx || xa > lay.right {
        return;
    }
    let content_clip = lay.clip;
    let nstr = lay.nstr;
    let shown = it.state == Some(State::Ready) || !it.doc.is_empty();
    for &(lane, ly, lh) in lay.lanes {
        match lane {
            // L'item qui a le focus (l'audio ou le MIDI) : choisi, comme
            // dans REAPER.
            Lane::Audio => draw_audio_item(cv, text, th, it, geo, xa, xb, ly, lh, pick.0),
            Lane::Midi => draw_midi_item(cv, th, it, geo, xa, xb, ly, lh, nstr, pick.1),
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
                if shown {
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
                    draw_pending(
                        cv,
                        text,
                        icons,
                        th,
                        bx,
                        it,
                        t,
                        i,
                        xa,
                        xb,
                        ly,
                        lh,
                        hover,
                        content_clip,
                    );
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
                if shown {
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
                } else if !lay.shown.on(Lane::Score) {
                    draw_pending(
                        cv,
                        text,
                        icons,
                        th,
                        bx,
                        it,
                        t,
                        i,
                        xa,
                        xb,
                        ly,
                        lh,
                        hover,
                        content_clip,
                    );
                }
            }
            _ => {}
        }
    }
    if is_sel {
        cv.stroke_rect(
            xa.round(),
            lay.y0 + 1.0,
            (xb - xa).round(),
            (lay.y1 - lay.y0 - 2.0).max(0.0),
            1.0,
            th.accent,
        );
    }
}

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
    // Un item de REAPER : son fond, son bord d'un pixel (blanc choisi), son
    // nom et sa forme d'onde aux couleurs du theme.
    let rp = &th.rp;
    let (bg, border, label, peak) = if sel {
        (
            rp.item_bg_sel,
            rp.item_border_sel,
            rp.item_label_sel,
            rp.peaks_sel,
        )
    } else {
        (rp.item_bg, rp.item_border, rp.item_label, rp.peaks)
    };
    let (x0, x1) = (xa.round(), xb.round().max(xa.round() + 1.0));
    let (y0, y1) = ((ly + s(1.0)).round(), (ly + lh - s(1.0)).round());
    cv.fill_rect(x0, y0, x1 - x0, y1 - y0, bg);
    cv.stroke_rect(x0, y0, x1 - x0, y1 - y0, 1.0, border);
    let f = Font::new(0, s(10.5) as u16, 500);
    let mut name = it.name.clone();
    if it.takes.len() > 1 {
        name = format!("{name}  {}/{}", it.active + 1, it.takes.len());
    }
    if cv.push_clip(xa + 1.0, ly, (xb - xa - 2.0).max(0.0), lh) {
        text.draw(
            cv,
            f,
            xa.max(geo.clip.0) + s(6.0),
            ly + s(13.5),
            &name,
            label,
        );
        // La forme d'onde : celle de REAPER, sinon l'enveloppe de l'analyse.
        let mid = ly + s(17.0) + (lh - s(23.0)) * 0.5;
        let half = (lh - s(25.0)) * 0.5;
        let n = it.frame_qn.len();
        let np = it.peaks.len();
        if np > 0 && it.qn1 > it.qn0 {
            let wave_c = peak;
            let mut x = xa.max(geo.clip.0).floor();
            let x_end = xb.min(geo.clip.0 + geo.clip.2);
            let k = np as f64 / (it.qn1 - it.qn0);
            while x < x_end {
                let qa = geo.scroll + ((x - geo.sx) / geo.ppq.max(1e-3)) as f64;
                let qb = geo.scroll + ((x + 1.0 - geo.sx) / geo.ppq.max(1e-3)) as f64;
                let a = (((qa - it.qn0) * k).floor().max(0.0) as usize).min(np - 1);
                let b = (((qb - it.qn0) * k).ceil().max(0.0) as usize).clamp(a + 1, np);
                let (lo, hi) = it.peaks[a..b]
                    .iter()
                    .fold((0f32, 0f32), |(l, h), p| (l.min(p.0), h.max(p.1)));
                let top = mid - hi.clamp(-1.0, 1.0) * half;
                let bot = mid - lo.clamp(-1.0, 1.0) * half;
                cv.fill_rect(x, top.min(bot), 1.0, (bot - top).abs().max(1.0), wave_c);
                x += 1.0;
            }
        } else if n > 0 && !it.env.is_empty() {
            let wave_c = peak.with_alpha(170);
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
        // Les fondus : la zone qui se tait (au-dessus de la courbe) et la
        // courbe, d'un pixel, comme REAPER.
        let (top, bot) = (ly + s(3.0), ly + lh - s(3.0));
        for (fin, len) in [(true, it.fade_in), (false, it.fade_out)] {
            if len <= 1e-9 {
                continue;
            }
            let (x0, x1) = if fin {
                (xa, geo.x(it.qn0 + len))
            } else {
                (geo.x(it.qn1 - len), xb)
            };
            if x1 - x0 < 1.0 {
                continue;
            }
            let mut zone = Path::new();
            if fin {
                zone.move_to(x0, top)
                    .line_to(x1, top)
                    .line_to(x0, bot)
                    .close();
            } else {
                zone.move_to(x0, top)
                    .line_to(x1, top)
                    .line_to(x1, bot)
                    .close();
            }
            cv.fill_path(&zone, rp.fade_zone);
            if fin {
                cv.line(x0, bot, x1, top, 1.0, rp.fade_line);
            } else {
                cv.line(x0, top, x1, bot, 1.0, rp.fade_line);
            }
        }
        if let Some(st) = &it.state {
            let msg = match st {
                State::Ready | State::Idle => String::new(),
                State::Waiting => "\u{2026}".into(),
                State::Busy(p) => format!("{p} %"),
                State::Failed(e) => format!("\u{26a0} {e}"),
            };
            if !msg.is_empty() {
                let fm = Font::new(0, s(11.0) as u16, 400);
                text.draw(
                    cv,
                    fm,
                    xa.max(geo.clip.0) + s(8.0),
                    mid + s(4.0),
                    &msg,
                    label,
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
    sel: bool,
) {
    let s = |v: f32| v * geo.s;
    let rp = &th.rp;
    let (bg, border) = if sel {
        (rp.item_bg_sel, rp.item_border_sel)
    } else {
        (rp.item_bg, rp.item_border)
    };
    let (x0, x1) = (xa.round(), xb.round().max(xa.round() + 1.0));
    let (y0, y1) = ((ly + s(1.0)).round(), (ly + lh - s(1.0)).round());
    cv.fill_rect(x0, y0, x1 - x0, y1 - y0, bg);
    cv.stroke_rect(x0, y0, x1 - x0, y1 - y0, 1.0, border);
    let (lo, hi) = (38.0f32, 82.0f32);
    for n in &it.doc {
        let x = geo.x(n.qn0);
        let w = (geo.x(n.qn1) - x - 1.0).max(1.5);
        if !geo.visible(x) && !geo.visible(x + w) {
            continue;
        }
        let y = ly + s(5.0) + (hi - n.pitch as f32) / (hi - lo) * (lh - s(13.0));
        cv.fill_rect(
            x.round(),
            y.round(),
            w.round().max(1.0),
            s(3.0),
            string_color(n.string.min(nstr.saturating_sub(1))).mix(Color::BLACK, 0.35),
        );
    }
    for g in &it.ghosts {
        let x = geo.x(g.qn0);
        let w = (geo.x(g.qn1) - x - 1.0).max(1.5);
        let y = ly + s(5.0) + (hi - g.pitch as f32) / (hi - lo) * (lh - s(13.0));
        dashed_rect(cv, x, y, w, s(3.0), rp.peaks.with_alpha(140));
    }
}

/// Un item pas encore transcrit, dans la partition ou la tab : la baguette
/// le transcrit (rien ne se transcrit sans qu'on le demande).
fn draw_pending(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    icons: &mut IconCache,
    th: &Theme,
    bx: &mut Vec<HBox>,
    it: &Item,
    t: usize,
    i: usize,
    xa: f32,
    xb: f32,
    ly: f32,
    lh: f32,
    hover: Hit,
    clip: (f32, f32, f32, f32),
) {
    let s = th.scale;
    dashed_rect(
        cv,
        xa + 2.0,
        ly + 4.0 * s,
        (xb - xa - 4.0).max(0.0),
        lh - 8.0 * s,
        th.text_faint,
    );
    if matches!(it.state, None | Some(State::Idle)) {
        let bs = (26.0 * s).round();
        let x = (xa.max(clip.0) + 10.0 * s).round();
        let y = (ly + (lh - bs) * 0.5).round();
        if x + bs <= xb {
            let hit = Hit::Transcribe { t, i };
            let hov = hover == hit;
            cv.fill_round_rect(
                x,
                y,
                bs,
                bs,
                3.0 * s,
                if hov {
                    th.accent.with_alpha(60)
                } else {
                    th.field
                },
            );
            let isz = (16.0 * s) as u16;
            let o = ((bs - isz as f32) * 0.5).round();
            icons.draw(cv, Icon::Wand, x + o, y + o, isz, th.accent, false);
            push(bx, clip, x, y, bs, bs, hit);
        }
        return;
    }
    let msg = match &it.state {
        Some(State::Busy(p)) => format!("{p} %"),
        Some(State::Failed(e)) => format!("\u{26a0} {e}"),
        _ => "\u{2026}".into(),
    };
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
pub(crate) fn draw_score(
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
    let pad = geo.pad;
    let lw = geo.lw;
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
            cv.fill_rect(x - rx * 1.7, y_of(p), rx * 3.4, lw, staff_c);
            p -= 2;
        }
        let mut p = 10;
        while p <= hi {
            cv.fill_rect(x - rx * 1.7, y_of(p), rx * 3.4, lw, staff_c);
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
            head_at(cv, x + dx, y, rx, ry, c);
            if hollow {
                head_at(cv, x + dx, y, rx * 0.62, ry * 0.42, paper);
            }
            // La glisse d'approche : un trait qui monte (ou descend) vers la
            // tete, avant son diese.
            let k = it.doc[n].slide_in();
            if k != 0 && sg.first && !sg.tie_in {
                let sw = if sharp {
                    text.measure(fsharp, &SHARP.to_string()) + s(2.0)
                } else {
                    0.0
                };
                let x1 = x + dx - rx - s(3.0) - sw;
                let d = if k < 0 { gap } else { -gap };
                cv.line(x1 - s(10.0), y + d, x1, y, s(1.1), th.text_dim);
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
                        lw * 1.1,
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
                let prev_n = eng.events[prev_sign.ev].notes.first().copied();
                match (ev.lig_in, prev_n, ev.notes.first()) {
                    // Une glisse : un trait d'une tete a l'autre.
                    (Tech::Slide, Some(pn), Some(&cn)) => {
                        let ya = y_of(staff_pos(it.doc[pn].pitch).0);
                        let yb = y_of(staff_pos(it.doc[cn].pitch).0);
                        cv.line(
                            px + rx + s(2.0),
                            ya,
                            x - rx - s(2.0),
                            yb,
                            s(1.1),
                            th.text_dim,
                        );
                    }
                    _ => {
                        let y = y_of(hi) - gap * 1.2;
                        arc(cv, px, y, x, y, -gap * 0.7, lw * 1.1, th.text_dim);
                    }
                }
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
        // Visible si une de ses notes l'est (pas sa hampe : une hampe vers
        // le bas passe a gauche de sa note, et la ligature d'une mesure
        // voisine depassait au bord d'une page).
        if g.iter().all(|&si| !geo.visible(hx(eng.signs[si].start))) {
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
            ghost_head_at(cv, x, y, rx, ry, c);
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
pub(crate) fn draw_tab(
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
    let pad = geo.pad;
    let lw = geo.lw;
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
            let mark = th.text_dim;
            // La glisse d'approche : « /7 » (d'en dessous) ou « \7 ».
            let k = note.slide_in();
            if k != 0 {
                let x1 = x - tw * 0.5 - s(1.5);
                let d = if k < 0 { s(4.0) } else { -s(4.0) };
                cv.line(x1 - s(8.0), y + d, x1, y - d * 0.4, lw, mark);
            }
            // Le bend : une fleche qui monte au-dessus de la tablature, sa
            // valeur (« ½ », « full »), et le relachement qui redescend.
            if let Some(bm) = note.bend_mark() {
                let x0 = x + tw * 0.5 + s(1.0);
                let top = tab_y - s(9.0);
                let xp = (geo.x(note.qn0 + bm.peak) + pad).max(x0 + s(9.0));
                let mut p = Path::new();
                p.move_to(x0, y).quad_to(xp, y, xp, top + s(3.5));
                cv.stroke_path(&p, lw, mark);
                arrow_head(cv, xp, top, s(3.5), true, mark);
                text.draw_in(
                    cv,
                    fl,
                    xp - s(14.0),
                    top - s(11.0),
                    s(28.0),
                    s(10.0),
                    &crate::bend_label(bm.amount),
                    mark,
                    Align::Center,
                );
                if let Some(rq) = bm.release {
                    let xr = (geo.x(note.qn0 + rq) + pad).max(xp + s(9.0));
                    let mut p = Path::new();
                    p.move_to(xp + s(1.0), top + s(1.0))
                        .quad_to(xr, top + s(1.0), xr, y - s(9.0));
                    cv.stroke_path(&p, lw, mark);
                    arrow_head(cv, xr, y - s(5.5), s(3.5), false, mark);
                }
            }
        }
        if ev.lig_in != Tech::Plain && ev.notes.len() == 1 {
            if let Some(prev_sign) = eng.signs[..si].iter().rev().find(|t| t.ev + 1 == sg.ev) {
                let px = hx(prev_sign.start);
                let cur = &it.doc[ev.notes[0]];
                let prev_n = eng.events[prev_sign.ev].notes.first().copied();
                match (ev.lig_in, prev_n) {
                    // Une glisse : un trait d'une case a l'autre, sur la corde.
                    (Tech::Slide, Some(pn)) => {
                        let pv = &it.doc[pn];
                        let pw = text.measure(fd, &pv.fret.to_string()).ceil() + s(4.0);
                        let cw = text.measure(fd, &cur.fret.to_string()).ceil() + s(4.0);
                        let y = line_y(cur.string);
                        let d = if cur.pitch > pv.pitch {
                            s(3.0)
                        } else {
                            -s(3.0)
                        };
                        // Entre les deux cases ; rien si elles se touchent
                        // (le trait barrait la case d'arrivee).
                        let (a, b) = (px + pw * 0.5 + s(1.5), x - cw * 0.5 - s(1.5));
                        if b > a + s(3.0) {
                            cv.line(a, y + d, b, y - d, lw, th.text_dim);
                        }
                    }
                    _ => {
                        let y = line_y(cur.string) - s(8.0);
                        arc(cv, px, y, x, y, -s(4.0), lw * 1.1, th.text_dim);
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
