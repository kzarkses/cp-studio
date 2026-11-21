//! Les chemins : des contours faits de segments et de courbes de Bezier.
//!
//! Tout ce qui n'est pas un rectangle droit se dessine par un chemin : coins
//! arrondis, disques, anneaux, traits epais, polygones, icones. Les courbes sont
//! aplaties en segments a 0,1 px pres au moment du dessin.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

pub const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

impl Pt {
    fn lerp(self, o: Pt, t: f32) -> Pt {
        pt(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Pt),
    Line(Pt),
    Quad(Pt, Pt),
    Cubic(Pt, Pt, Pt),
    Close,
}

/// La constante du quart de cercle en Bezier cubique.
const KAPPA: f32 = 0.552_284_8;

#[derive(Clone, Debug, Default)]
pub struct Path {
    pub segs: Vec<Seg>,
}

impl Path {
    pub fn new() -> Path {
        Path::default()
    }

    pub fn clear(&mut self) {
        self.segs.clear();
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.segs.push(Seg::Move(pt(x, y)));
        self
    }
    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.segs.push(Seg::Line(pt(x, y)));
        self
    }
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        self.segs.push(Seg::Quad(pt(cx, cy), pt(x, y)));
        self
    }
    pub fn cubic_to(
        &mut self,
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    ) -> &mut Self {
        self.segs
            .push(Seg::Cubic(pt(c1x, c1y), pt(c2x, c2y), pt(x, y)));
        self
    }
    pub fn close(&mut self) -> &mut Self {
        self.segs.push(Seg::Close);
        self
    }

    /// Un rectangle, sens horaire (a l'ecran, y vers le bas).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        self.move_to(x, y)
            .line_to(x + w, y)
            .line_to(x + w, y + h)
            .line_to(x, y + h)
            .close()
    }

    /// Un rectangle aux coins arrondis de rayon `r` (borne a la demi-taille).
    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> &mut Self {
        let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
        if r <= 0.0 {
            return self.rect(x, y, w, h);
        }
        let k = r * KAPPA;
        let (x1, y1) = (x + w, y + h);
        self.move_to(x + r, y)
            .line_to(x1 - r, y)
            .cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r)
            .line_to(x1, y1 - r)
            .cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1)
            .line_to(x + r, y1)
            .cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r)
            .line_to(x, y + r)
            .cubic_to(x, y + r - k, x + r - k, y, x + r, y)
            .close()
    }

    /// Une ellipse, sens horaire.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) -> &mut Self {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.move_to(cx + rx, cy)
            .cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry)
            .cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy)
            .cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry)
            .cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy)
            .close()
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) -> &mut Self {
        self.ellipse(cx, cy, r, r)
    }

    /// La meme ellipse en sens INVERSE : ajoutee a un disque plus grand, elle le
    /// troue (anneau).
    pub fn ellipse_rev(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) -> &mut Self {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.move_to(cx + rx, cy)
            .cubic_to(cx + rx, cy - ky, cx + kx, cy - ry, cx, cy - ry)
            .cubic_to(cx - kx, cy - ry, cx - rx, cy - ky, cx - rx, cy)
            .cubic_to(cx - rx, cy + ky, cx - kx, cy + ry, cx, cy + ry)
            .cubic_to(cx + kx, cy + ry, cx + rx, cy + ky, cx + rx, cy)
            .close()
    }

    /// Les polylignes fermees du chemin, courbes aplaties (tolerance en px).
    pub fn flatten(&self, tol: f32, out: &mut Vec<Vec<Pt>>) {
        out.clear();
        let mut cur: Vec<Pt> = Vec::new();
        let mut last = pt(0.0, 0.0);
        let mut start = last;
        let flush = |cur: &mut Vec<Pt>, out: &mut Vec<Vec<Pt>>| {
            if cur.len() >= 2 {
                out.push(std::mem::take(cur));
            } else {
                cur.clear();
            }
        };
        for s in &self.segs {
            match *s {
                Seg::Move(p) => {
                    flush(&mut cur, out);
                    cur.push(p);
                    last = p;
                    start = p;
                }
                Seg::Line(p) => {
                    if cur.is_empty() {
                        cur.push(last);
                    }
                    cur.push(p);
                    last = p;
                }
                Seg::Quad(c, p) => {
                    if cur.is_empty() {
                        cur.push(last);
                    }
                    let dd = (last.x - 2.0 * c.x + p.x).hypot(last.y - 2.0 * c.y + p.y);
                    let n = (dd / (8.0 * tol)).sqrt().ceil().clamp(1.0, 256.0) as usize;
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let a = last.lerp(c, t);
                        let b = c.lerp(p, t);
                        cur.push(a.lerp(b, t));
                    }
                    last = p;
                }
                Seg::Cubic(c1, c2, p) => {
                    if cur.is_empty() {
                        cur.push(last);
                    }
                    let d1 = (last.x - 2.0 * c1.x + c2.x).hypot(last.y - 2.0 * c1.y + c2.y);
                    let d2 = (c1.x - 2.0 * c2.x + p.x).hypot(c1.y - 2.0 * c2.y + p.y);
                    let dd = d1.max(d2);
                    // Au moins 12 segments : un polygone inscrit perd (2 pi^2 / 3) / n^2
                    // de l aire du cercle, 0,3 % a 48 cotes.
                    let n = (0.75 * dd / tol).sqrt().ceil().clamp(12.0, 256.0) as usize;
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let mt = 1.0 - t;
                        let a = mt * mt * mt;
                        let b = 3.0 * mt * mt * t;
                        let c = 3.0 * mt * t * t;
                        let d = t * t * t;
                        cur.push(pt(
                            a * last.x + b * c1.x + c * c2.x + d * p.x,
                            a * last.y + b * c1.y + c * c2.y + d * p.y,
                        ));
                    }
                    last = p;
                }
                Seg::Close => {
                    if !cur.is_empty() {
                        cur.push(start);
                    }
                    flush(&mut cur, out);
                    last = start;
                }
            }
        }
        flush(&mut cur, out);
    }

    /// Le contour d'un trait d'epaisseur `w` le long des polylignes du chemin,
    /// bouts et jointures ronds. C'est une UNION de formes de meme sens
    /// (rectangles par segment, disques aux sommets) : le rasteriseur la
    /// remplit en saturant la couverture.
    pub fn stroke(&self, w: f32, out: &mut Path) {
        let mut polys = Vec::new();
        self.flatten(0.1, &mut polys);
        let r = (w * 0.5).max(0.01);
        for poly in &polys {
            for i in 0..poly.len() {
                let p = poly[i];
                out.circle(p.x, p.y, r);
                if i + 1 < poly.len() {
                    let q = poly[i + 1];
                    let (dx, dy) = (q.x - p.x, q.y - p.y);
                    let len = dx.hypot(dy);
                    if len < 1e-6 {
                        continue;
                    }
                    let (nx, ny) = (-dy / len * r, dx / len * r);
                    // Sens horaire quel que soit le sens du segment.
                    let (a, b, c, d) = (
                        pt(p.x + nx, p.y + ny),
                        pt(q.x + nx, q.y + ny),
                        pt(q.x - nx, q.y - ny),
                        pt(p.x - nx, p.y - ny),
                    );
                    let area = (b.x - a.x) * (d.y - a.y) - (b.y - a.y) * (d.x - a.x);
                    if area >= 0.0 {
                        out.move_to(a.x, a.y)
                            .line_to(b.x, b.y)
                            .line_to(c.x, c.y)
                            .line_to(d.x, d.y)
                            .close();
                    } else {
                        out.move_to(a.x, a.y)
                            .line_to(d.x, d.y)
                            .line_to(c.x, c.y)
                            .line_to(b.x, b.y)
                            .close();
                    }
                }
            }
        }
    }
}
