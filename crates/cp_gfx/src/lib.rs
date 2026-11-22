//! Le moteur de dessin de CP Studio.
//!
//! Il ecrit des pixels dans une [`Surface`] (premultipliee, BGRA en memoire :
//! l'ordre d'une DIB de Windows), sans rien savoir de Windows : il se teste et
//! se regarde hors de REAPER (`png`).
//!
//! - Tout ce qui n'est pas un rectangle droit est un [`Path`], rempli avec une
//!   couverture EXACTE (aire vraie du pixel sous le bord, `raster`).
//! - Le decoupage est au pixel, pour TOUTES les primitives, par une pile.
//! - Le texte passe par un atlas de glyphes ([`text`]) que la plateforme
//!   remplit (GDI dans `cp_sys`).

// Le dessin prend naturellement x, y, w, h, rayon, epaisseur, couleur... : des
// parametres positionnels nombreux sont la forme juste ici.
#![allow(clippy::too_many_arguments)]

pub mod color;
pub mod layers;
pub mod path;
pub mod pdf;
pub mod png;
pub mod raster;
pub mod text;

pub use color::{Color, hex, rgb, rgba};
pub use path::{Path, Pt, pt};
pub use raster::Raster;

use color::{over, scale};

/// Un rectangle entier (pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn intersect(&self, o: &IRect) -> IRect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        IRect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        IRect::new(x0, y0, x1 - x0, y1 - y0)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// Une couverture (0..255) et sa place : le coin haut-gauche en pixels.
/// Une forme qui revient cent fois (une tete de note) se rasterise une fois
/// et se pose ensuite au pixel pres ([`Canvas::draw_mask`]).
#[derive(Clone, Debug, Default)]
pub struct Mask {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

impl Mask {
    /// La couverture d'un chemin rempli, a ses propres coordonnees.
    pub fn of_path(p: &Path) -> Mask {
        let mut polys = Vec::new();
        p.flatten(0.02, &mut polys);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for q in polys.iter().flatten() {
            x0 = x0.min(q.x);
            y0 = y0.min(q.y);
            x1 = x1.max(q.x);
            y1 = y1.max(q.y);
        }
        if x0 > x1 {
            return Mask::default();
        }
        let (bx, by) = (x0.floor() as i32, y0.floor() as i32);
        let (w, h) = (x1.ceil() as i32 - bx + 1, y1.ceil() as i32 - by + 1);
        let mut s = Surface::new(w, h);
        let mut k = Scratch::default();
        {
            let mut cv = Canvas::new(&mut s, &mut k);
            cv.push_origin(-bx as f32, -by as f32);
            cv.fill_path(p, Color::WHITE);
        }
        Mask {
            x: bx,
            y: by,
            w,
            h,
            px: s.px.iter().map(|&v| (v >> 24) as u8).collect(),
        }
    }
}

/// Des pixels premultiplies, `w x h`, sans marge.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Surface {
        let (w, h) = (w.max(0), h.max(0));
        Surface {
            w,
            h,
            px: vec![0; (w * h) as usize],
        }
    }

    /// Change la taille ; le contenu est perdu si elle change.
    pub fn resize(&mut self, w: i32, h: i32) -> bool {
        let (w, h) = (w.max(0), h.max(0));
        if w == self.w && h == self.h {
            return false;
        }
        self.w = w;
        self.h = h;
        self.px.clear();
        self.px.resize((w * h) as usize, 0);
        true
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        self.px[(y * self.w + x) as usize]
    }
}

/// Une image a dessiner (icone, vignette), premultipliee.
pub type Image = Surface;

/// Comment remplir.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// De `c0` en (x0, y0) a `c1` en (x1, y1).
    Linear {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        c0: Color,
        c1: Color,
    },
    /// De `c0` au centre a `c1` au rayon `r`.
    Radial {
        cx: f32,
        cy: f32,
        r: f32,
        c0: Color,
        c1: Color,
    },
}

impl From<Color> for Paint {
    fn from(c: Color) -> Paint {
        Paint::Solid(c)
    }
}

impl Paint {
    /// La couleur premultipliee au CENTRE du pixel (x, y) (coordonnees
    /// surface, origine comprise).
    fn at(&self, x: i32, y: i32, ox: f32, oy: f32) -> u32 {
        let (px, py) = (x as f32 + 0.5 - ox, y as f32 + 0.5 - oy);
        match *self {
            Paint::Solid(c) => c.premul(),
            Paint::Linear {
                x0,
                y0,
                x1,
                y1,
                c0,
                c1,
            } => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 0.0 {
                    ((px - x0) * dx + (py - y0) * dy) / l2
                } else {
                    0.0
                };
                c0.mix(c1, t).premul()
            }
            Paint::Radial { cx, cy, r, c0, c1 } => {
                let d = (px - cx).hypot(py - cy);
                c0.mix(c1, if r > 0.0 { d / r } else { 1.0 }).premul()
            }
        }
    }
}

/// Le materiel de travail du dessin, garde d'une image a l'autre.
#[derive(Default)]
pub struct Scratch {
    raster: Raster,
    polys: Vec<Vec<Pt>>,
    path: Path,
}

/// Dessiner dans une surface : decoupage et origine en piles.
pub struct Canvas<'a> {
    s: &'a mut Surface,
    k: &'a mut Scratch,
    clip: IRect,
    clips: Vec<IRect>,
    ox: f32,
    oy: f32,
    origins: Vec<(f32, f32)>,
}

impl<'a> Canvas<'a> {
    pub fn new(s: &'a mut Surface, k: &'a mut Scratch) -> Canvas<'a> {
        let clip = IRect::new(0, 0, s.w, s.h);
        Canvas {
            s,
            k,
            clip,
            clips: Vec::new(),
            ox: 0.0,
            oy: 0.0,
            origins: Vec::new(),
        }
    }

    pub fn width(&self) -> i32 {
        self.s.w
    }
    pub fn height(&self) -> i32 {
        self.s.h
    }

    // --- piles -------------------------------------------------------------

    /// Restreint le decoupage a `r` (coordonnees courantes). Rend faux si plus
    /// rien n'est visible (l'appelant peut sauter tout un panneau).
    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.clips.push(self.clip);
        let x0 = (x + self.ox).floor() as i32;
        let y0 = (y + self.oy).floor() as i32;
        let x1 = (x + w + self.ox).ceil() as i32;
        let y1 = (y + h + self.oy).ceil() as i32;
        self.clip = self.clip.intersect(&IRect::new(x0, y0, x1 - x0, y1 - y0));
        !self.clip.is_empty()
    }

    pub fn pop_clip(&mut self) {
        if let Some(c) = self.clips.pop() {
            self.clip = c;
        }
    }

    /// Le decoupage courant, en coordonnees de surface.
    pub fn clip(&self) -> IRect {
        self.clip
    }

    /// Le nombre de decoupages poses (chaque `push_clip` doit avoir son
    /// `pop_clip`).
    pub fn clip_depth(&self) -> usize {
        self.clips.len()
    }

    /// Revient aux decoupages d'avant un dessin (`clip_depth`, `clip`) ; rend
    /// faux s'ils n'y etaient plus : un `pop_clip` de trop defait celui de la
    /// zone a redessiner, et la suite se dessine sur toute la fenetre.
    pub fn restore_clips(&mut self, depth: usize, clip: IRect) -> bool {
        let ok = self.clips.len() == depth && self.clip == clip;
        self.clips.truncate(depth);
        self.clip = clip;
        ok
    }

    pub fn push_origin(&mut self, dx: f32, dy: f32) {
        self.origins.push((self.ox, self.oy));
        self.ox += dx;
        self.oy += dy;
    }

    pub fn pop_origin(&mut self) {
        if let Some((x, y)) = self.origins.pop() {
            self.ox = x;
            self.oy = y;
        }
    }

    // --- rectangles --------------------------------------------------------

    /// Tout le decoupage, remplace (pas compose) par `c`.
    pub fn clear(&mut self, c: Color) {
        let p = c.premul();
        let cl = self.clip;
        for y in cl.y..cl.bottom() {
            let row = (y * self.s.w) as usize;
            self.s.px[row + cl.x as usize..row + cl.right() as usize].fill(p);
        }
    }

    /// Un rectangle aux bords eventuellement fractionnaires : la couverture de
    /// chaque pixel de bord est son aire exacte.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, paint: impl Into<Paint>) {
        let paint = paint.into();
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (x0, y0) = (x + self.ox, y + self.oy);
        let (x1, y1) = (x0 + w, y0 + h);
        let ix0 = (x0.floor() as i32).max(self.clip.x);
        let iy0 = (y0.floor() as i32).max(self.clip.y);
        let ix1 = (x1.ceil() as i32).min(self.clip.right());
        let iy1 = (y1.ceil() as i32).min(self.clip.bottom());
        if ix0 >= ix1 || iy0 >= iy1 {
            return;
        }
        // Le cas courant d'une interface : un rectangle sur des pixels entiers.
        // Opaque, il se REMPLIT ligne par ligne ; translucide, il se compose
        // sans calcul de couverture. (Le chemin general coutait 6 ms par image
        // sur une grande vue ancree, pour des fonds de lignes.)
        let whole =
            x0.fract() == 0.0 && y0.fract() == 0.0 && x1.fract() == 0.0 && y1.fract() == 0.0;
        if let (true, Paint::Solid(c)) = (whole, paint) {
            let src = c.premul();
            for py in iy0..iy1 {
                let row = (py * self.s.w) as usize;
                let span = &mut self.s.px[row + ix0 as usize..row + ix1 as usize];
                if c.a == 255 {
                    span.fill(src);
                } else {
                    for d in span {
                        *d = over(*d, src, 255);
                    }
                }
            }
            return;
        }
        let cov1 = |a: f32, b: f32, i: i32| -> f32 {
            let lo = a.max(i as f32);
            let hi = b.min(i as f32 + 1.0);
            (hi - lo).clamp(0.0, 1.0)
        };
        let solid = match paint {
            Paint::Solid(c) => Some(c.premul()),
            _ => None,
        };
        for py in iy0..iy1 {
            let cy = cov1(y0, y1, py);
            let row = (py * self.s.w) as usize;
            for px in ix0..ix1 {
                let cx = cov1(x0, x1, px);
                let cov = (cx * cy * 255.0 + 0.5) as u32;
                if cov == 0 {
                    continue;
                }
                let src = solid.unwrap_or_else(|| paint.at(px, py, self.ox, self.oy));
                let d = &mut self.s.px[row + px as usize];
                *d = over(*d, src, cov);
            }
        }
    }

    /// Un cadre de `t` px a l'interieur du rectangle.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, c: Color) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h - t, w, t, c);
        self.fill_rect(x, y + t, t, h - 2.0 * t, c);
        self.fill_rect(x + w - t, y + t, t, h - 2.0 * t, c);
    }

    // --- chemins -----------------------------------------------------------

    /// Remplit un chemin (coordonnees courantes).
    pub fn fill_path(&mut self, p: &Path, paint: impl Into<Paint>) {
        let paint = paint.into();
        p.flatten(0.02, &mut self.k.polys);
        let (ox, oy) = (self.ox, self.oy);
        // La boite des points, coupee au decoupage.
        let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for poly in &self.k.polys {
            for q in poly {
                bx0 = bx0.min(q.x + ox);
                by0 = by0.min(q.y + oy);
                bx1 = bx1.max(q.x + ox);
                by1 = by1.max(q.y + oy);
            }
        }
        if bx0 > bx1 {
            return;
        }
        let bb = IRect::new(
            bx0.floor() as i32,
            by0.floor() as i32,
            (bx1.ceil() - bx0.floor()) as i32 + 1,
            (by1.ceil() - by0.floor()) as i32 + 1,
        )
        .intersect(&self.clip);
        if bb.is_empty() {
            return;
        }
        let r = &mut self.k.raster;
        r.reset(bb.w as usize, bb.h as usize);
        let (dx, dy) = (ox - bb.x as f32, oy - bb.y as f32);
        for poly in &self.k.polys {
            for s in poly.windows(2) {
                r.line(pt(s[0].x + dx, s[0].y + dy), pt(s[1].x + dx, s[1].y + dy));
            }
        }
        let surf = &mut *self.s;
        let solid = match paint {
            Paint::Solid(c) => Some(c.premul()),
            _ => None,
        };
        r.sweep(|y, x0, covs| {
            let sy = bb.y + y as i32;
            let row = (sy * surf.w) as usize;
            for (i, &c) in covs.iter().enumerate() {
                let sx = bb.x + (x0 + i) as i32;
                let src = solid.unwrap_or_else(|| paint.at(sx, sy, ox, oy));
                let d = &mut surf.px[row + sx as usize];
                *d = over(*d, src, c as u32);
            }
        });
    }

    /// Le trait d'un chemin, d'epaisseur `w`, bouts et jointures ronds.
    pub fn stroke_path(&mut self, p: &Path, w: f32, paint: impl Into<Paint>) {
        let mut st = std::mem::take(&mut self.k.path);
        st.clear();
        p.stroke(w, &mut st);
        self.fill_path(&st, paint);
        self.k.path = st;
    }

    pub fn fill_round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        paint: impl Into<Paint>,
    ) {
        let mut p = std::mem::take(&mut self.k.path);
        p.clear();
        p.round_rect(x, y, w, h, r);
        self.fill_path(&p, paint);
        self.k.path = p;
    }

    /// Le contour d'un rectangle arrondi, de `t` px vers l'interieur.
    pub fn stroke_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: Color) {
        let mut p = std::mem::take(&mut self.k.path);
        p.clear();
        p.round_rect(x, y, w, h, r);
        let mut inner = Path::new();
        inner.round_rect(x + t, y + t, w - 2.0 * t, h - 2.0 * t, (r - t).max(0.0));
        // Le trou : le meme contour parcouru a l'envers.
        let mut polys = Vec::new();
        inner.flatten(0.1, &mut polys);
        for poly in &polys {
            for (i, q) in poly.iter().rev().enumerate() {
                if i == 0 {
                    p.move_to(q.x, q.y);
                } else {
                    p.line_to(q.x, q.y);
                }
            }
            p.close();
        }
        self.fill_path(&p, c);
        self.k.path = p;
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, paint: impl Into<Paint>) {
        let mut p = std::mem::take(&mut self.k.path);
        p.clear();
        p.circle(cx, cy, r);
        self.fill_path(&p, paint);
        self.k.path = p;
    }

    /// Un anneau : le cercle de rayon `r`, epaisseur `t` vers l'interieur.
    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, t: f32, c: Color) {
        let mut p = std::mem::take(&mut self.k.path);
        p.clear();
        p.circle(cx, cy, r).ellipse_rev(cx, cy, r - t, r - t);
        self.fill_path(&p, c);
        self.k.path = p;
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        // Hors du decoupage : rien a construire (un trait coute un chemin).
        let m = w * 0.5 + 1.0;
        let (ox, oy) = (self.ox, self.oy);
        let bx0 = (x0.min(x1) - m + ox).floor() as i32;
        let by0 = (y0.min(y1) - m + oy).floor() as i32;
        let bx1 = (x0.max(x1) + m + ox).ceil() as i32;
        let by1 = (y0.max(y1) + m + oy).ceil() as i32;
        if self
            .clip
            .intersect(&IRect::new(bx0, by0, bx1 - bx0, by1 - by0))
            .is_empty()
        {
            return;
        }
        // Droit (une hampe, un trait de grille) : un rectangle, sans chemin.
        if x0 == x1 {
            self.fill_rect(x0 - w * 0.5, y0.min(y1), w, (y1 - y0).abs().max(w), c);
            return;
        }
        if y0 == y1 {
            self.fill_rect(x0.min(x1), y0 - w * 0.5, (x1 - x0).abs().max(w), w, c);
            return;
        }
        let mut p = Path::new();
        p.move_to(x0, y0).line_to(x1, y1);
        self.stroke_path(&p, w, c);
    }

    /// Un pointille horizontal d'un pixel, de `x0` a `x1` sur la ligne `y` :
    /// `on` pixels pleins, `off` vides, la phase partant de `x0`. Seule la
    /// partie dans le decoupage est parcourue (un item de dix mille pixels ne
    /// coute que ce qu'on en voit).
    pub fn dash_h(&mut self, x0: f32, x1: f32, y: f32, on: f32, off: f32, c: Color) {
        let period = on + off;
        if period <= 0.0 || x1 <= x0 {
            return;
        }
        let cl = self.clip;
        let (lo, hi) = (cl.x as f32 - self.ox, cl.right() as f32 - self.ox);
        let a = x0.max(lo);
        let b = x1.min(hi);
        if a >= b {
            return;
        }
        let y = y.floor();
        let mut k = ((a - x0) / period).floor().max(0.0);
        loop {
            let s = x0 + k * period;
            if s >= b {
                break;
            }
            let e = (s + on).min(x1);
            if e > a {
                self.fill_rect(s.max(a), y, e - s.max(a), 1.0, c);
            }
            k += 1.0;
        }
    }

    /// Un pointille vertical d'un pixel (voir [`Canvas::dash_h`]).
    pub fn dash_v(&mut self, x: f32, y0: f32, y1: f32, on: f32, off: f32, c: Color) {
        let period = on + off;
        if period <= 0.0 || y1 <= y0 {
            return;
        }
        let cl = self.clip;
        let (lo, hi) = (cl.y as f32 - self.oy, cl.bottom() as f32 - self.oy);
        let xl = x.floor();
        if xl + self.ox >= cl.right() as f32 || xl + 1.0 + self.ox <= cl.x as f32 {
            return;
        }
        let a = y0.max(lo);
        let b = y1.min(hi);
        if a >= b {
            return;
        }
        let mut k = ((a - y0) / period).floor().max(0.0);
        loop {
            let s = y0 + k * period;
            if s >= b {
                break;
            }
            let e = (s + on).min(y1);
            if e > a {
                self.fill_rect(xl, s.max(a), 1.0, e - s.max(a), c);
            }
            k += 1.0;
        }
    }

    /// Un cadre pointille d'un pixel.
    pub fn dash_rect(&mut self, x: f32, y: f32, w: f32, h: f32, on: f32, off: f32, c: Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (x, y) = (x.floor(), y.floor());
        let (w, h) = (w.round().max(1.0), h.round().max(1.0));
        self.dash_h(x, x + w, y, on, off, c);
        self.dash_h(x, x + w, y + h - 1.0, on, off, c);
        self.dash_v(x, y + 1.0, y + h - 1.0, on, off, c);
        self.dash_v(x + w - 1.0, y + 1.0, y + h - 1.0, on, off, c);
    }

    pub fn polyline(&mut self, pts: &[Pt], w: f32, c: Color) {
        if pts.len() < 2 {
            return;
        }
        let mut p = Path::new();
        p.move_to(pts[0].x, pts[0].y);
        for q in &pts[1..] {
            p.line_to(q.x, q.y);
        }
        self.stroke_path(&p, w, c);
    }

    // --- images et masques -------------------------------------------------

    /// Une image a sa taille, coin haut-gauche en (x, y) arrondi au pixel,
    /// opacite `alpha` (0..1).
    pub fn draw_image(&mut self, img: &Image, x: f32, y: f32, alpha: f32) {
        let k = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        if k == 0 {
            return;
        }
        let dx = (x + self.ox).round() as i32;
        let dy = (y + self.oy).round() as i32;
        let r = IRect::new(dx, dy, img.w, img.h).intersect(&self.clip);
        for py in r.y..r.bottom() {
            let srow = ((py - dy) * img.w) as usize;
            let drow = (py * self.s.w) as usize;
            for px in r.x..r.right() {
                let src = img.px[srow + (px - dx) as usize];
                let d = &mut self.s.px[drow + px as usize];
                *d = over(*d, src, k);
            }
        }
    }

    /// Un masque de couverture (glyphe, icone pre-cuite) teinte d'une couleur.
    pub fn draw_mask(&mut self, mask: &[u8], mw: i32, mh: i32, x: i32, y: i32, c: Color) {
        let src = c.premul();
        let dx = x + self.ox.round() as i32;
        let dy = y + self.oy.round() as i32;
        let r = IRect::new(dx, dy, mw, mh).intersect(&self.clip);
        for py in r.y..r.bottom() {
            let mrow = ((py - dy) * mw) as usize;
            let drow = (py * self.s.w) as usize;
            for px in r.x..r.right() {
                let m = mask[mrow + (px - dx) as usize] as u32;
                if m != 0 {
                    let d = &mut self.s.px[drow + px as usize];
                    *d = over(*d, src, m);
                }
            }
        }
    }

    /// Assombrit (ou eclaircit) tout le decoupage : un voile.
    pub fn veil(&mut self, c: Color) {
        let r = self.clip;
        let src = c.premul();
        for py in r.y..r.bottom() {
            let row = (py * self.s.w) as usize;
            for px in r.x..r.right() {
                let d = &mut self.s.px[row + px as usize];
                *d = over(*d, src, 255);
            }
        }
    }

    /// Pour les rares cas ou il faut multiplier ce qui est deja la.
    pub fn fade_region(&mut self, k: f32) {
        let k = (k.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        let r = self.clip;
        for py in r.y..r.bottom() {
            let row = (py * self.s.w) as usize;
            for px in r.x..r.right() {
                let d = &mut self.s.px[row + px as usize];
                *d = scale(*d, k);
            }
        }
    }

    /// L'origine courante (pour le texte, qui se pose au pixel).
    pub fn origin(&self) -> (f32, f32) {
        (self.ox, self.oy)
    }
}

#[cfg(test)]
mod tests;

/// Les bandes de lignes ou `now` differe de `shown` (ce qui est a l'ecran),
/// entre les lignes `y0` et `y1` ; `shown` est mis a jour au passage. Deux
/// bandes separees de moins de `gap` lignes n'en font qu'une. C'est ce qui
/// fait qu'on n'envoie a l'ecran que ce qui a change : la copie vers l'ecran
/// coute plus que le dessin sur une grande fenetre.
pub fn changed_bands(
    now: &[u32],
    shown: &mut [u32],
    w: usize,
    y0: usize,
    y1: usize,
    gap: usize,
) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for y in y0..y1 {
        let r = y * w..(y + 1) * w;
        if now[r.clone()] != shown[r.clone()] {
            shown[r.clone()].copy_from_slice(&now[r]);
            start.get_or_insert(y);
        } else if let Some(s) = start.take() {
            match out.last_mut() {
                Some(last) if s - last.1 < gap => last.1 = y,
                _ => out.push((s, y)),
            }
        }
    }
    if let Some(s) = start {
        match out.last_mut() {
            Some(last) if s - last.1 < gap => last.1 = y1,
            _ => out.push((s, y1)),
        }
    }
    out
}
