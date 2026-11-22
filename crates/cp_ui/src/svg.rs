//! Le sous-ensemble de SVG des icones Lucide : `path` (M L H V C S Q T A Z,
//! absolus et relatifs), `circle`, `ellipse`, `rect` (coins `rx`), `line`,
//! `polyline`. Tout devient un [`Path`] dans le repere 24 x 24 de l'icone, mis
//! a l'echelle `k` et deplace en (ox, oy).

use cp_gfx::Path;

struct Tok<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Tok<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
        {
            self.i += 1;
        }
    }
    fn cmd(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }
    fn at_number(&mut self) -> bool {
        self.skip();
        self.s
            .get(self.i)
            .is_some_and(|&c| c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.')
    }
    fn num(&mut self) -> f32 {
        self.skip();
        let st = self.i;
        let s = self.s;
        if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < s.len() {
            let c = s[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else if (c == b'e' || c == b'E') && self.i > st {
                self.i += 1;
                if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        std::str::from_utf8(&s[st..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0)
    }
    /// Un drapeau d'arc : un seul chiffre, parfois colle au suivant (« 0 0 1 2-2 »
    /// ou « 001 »).
    fn flag(&mut self) -> bool {
        self.skip();
        let c = self.s.get(self.i).copied().unwrap_or(b'0');
        self.i += 1;
        c == b'1'
    }
}

/// Transforme les coordonnees de l'icone.
#[derive(Clone, Copy)]
pub struct Xf {
    pub k: f32,
    pub ox: f32,
    pub oy: f32,
    /// Le calage sur les pixels (l'« hinting » d'une police) : chaque point
    /// tombe sur la grille `n + grid` (0,5 pour un trait d'epaisseur impaire,
    /// 0 pour une paire), et le trait couvre des pixels entiers au lieu d'en
    /// griser deux. `None` : les coordonnees exactes.
    pub grid: Option<f32>,
}

impl Xf {
    fn snap(&self, v: f32) -> f32 {
        match self.grid {
            Some(g) => (v - g).round() + g,
            None => v,
        }
    }
    fn x(&self, v: f32) -> f32 {
        self.snap(self.ox + v * self.k)
    }
    fn y(&self, v: f32) -> f32 {
        self.snap(self.oy + v * self.k)
    }
}

/// Un arc elliptique SVG (parametrage par extremites) en cubiques.
#[allow(clippy::too_many_arguments)]
fn arc(
    out: &mut Path,
    xf: Xf,
    x0: f32,
    y0: f32,
    rx: f32,
    ry: f32,
    phi_deg: f32,
    large: bool,
    sweep: bool,
    x1: f32,
    y1: f32,
) {
    if (x0 - x1).abs() < 1e-6 && (y0 - y1).abs() < 1e-6 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 {
        out.line_to(xf.x(x1), xf.y(y1));
        return;
    }
    let phi = phi_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (x0 - x1) / 2.0;
    let dy = (y0 - y1) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut co = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x0 + x1) / 2.0;
    let cy = sp * cxp + cp * cyp + (y0 + y1) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let l = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / l).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let th1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dth = ang(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    let tau = std::f32::consts::TAU;
    if !sweep && dth > 0.0 {
        dth -= tau;
    } else if sweep && dth < 0.0 {
        dth += tau;
    }
    let n = (dth.abs() / (std::f32::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let d = dth / n as f32;
    let t = 4.0 / 3.0 * (d / 4.0).tan();
    let mut a = th1;
    for _ in 0..n {
        let (s1, c1) = a.sin_cos();
        let (s2, c2) = (a + d).sin_cos();
        let p = |c: f32, s: f32| {
            (
                cx + rx * c * cp - ry * s * sp,
                cy + rx * c * sp + ry * s * cp,
            )
        };
        let dp = |c: f32, s: f32| (-rx * s * cp - ry * c * sp, -rx * s * sp + ry * c * cp);
        let (p1x, p1y) = p(c1, s1);
        let (d1x, d1y) = dp(c1, s1);
        let (p2x, p2y) = p(c2, s2);
        let (d2x, d2y) = dp(c2, s2);
        out.cubic_to(
            xf.x(p1x + t * d1x),
            xf.y(p1y + t * d1y),
            xf.x(p2x - t * d2x),
            xf.y(p2y - t * d2y),
            xf.x(p2x),
            xf.y(p2y),
        );
        a += d;
    }
}

/// Les donnees `d` d'un `<path>`.
pub fn path_data(d: &str, xf: Xf, out: &mut Path) {
    let mut t = Tok {
        s: d.as_bytes(),
        i: 0,
    };
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    // Le dernier point de controle (pour S et T).
    let (mut lcx, mut lcy) = (0.0f32, 0.0f32);
    let mut last = b' ';
    let mut cmd = b' ';
    loop {
        if let Some(c) = t.cmd() {
            cmd = c;
        } else if !t.at_number() {
            break;
        } else if cmd == b'M' {
            cmd = b'L';
        } else if cmd == b'm' {
            cmd = b'l';
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                x = bx + t.num();
                y = by + t.num();
                sx = x;
                sy = y;
                out.move_to(xf.x(x), xf.y(y));
            }
            b'L' => {
                x = bx + t.num();
                y = by + t.num();
                out.line_to(xf.x(x), xf.y(y));
            }
            b'H' => {
                x = bx + t.num();
                out.line_to(xf.x(x), xf.y(y));
            }
            b'V' => {
                y = (if rel { y } else { 0.0 }) + t.num();
                out.line_to(xf.x(x), xf.y(y));
            }
            b'C' => {
                let (c1x, c1y) = (bx + t.num(), by + t.num());
                let (c2x, c2y) = (bx + t.num(), by + t.num());
                x = bx + t.num();
                y = by + t.num();
                out.cubic_to(xf.x(c1x), xf.y(c1y), xf.x(c2x), xf.y(c2y), xf.x(x), xf.y(y));
                lcx = c2x;
                lcy = c2y;
            }
            b'S' => {
                let (c1x, c1y) = if matches!(last, b'C' | b'S') {
                    (2.0 * x - lcx, 2.0 * y - lcy)
                } else {
                    (x, y)
                };
                let (c2x, c2y) = (bx + t.num(), by + t.num());
                x = bx + t.num();
                y = by + t.num();
                out.cubic_to(xf.x(c1x), xf.y(c1y), xf.x(c2x), xf.y(c2y), xf.x(x), xf.y(y));
                lcx = c2x;
                lcy = c2y;
            }
            b'Q' => {
                let (cx, cy) = (bx + t.num(), by + t.num());
                x = bx + t.num();
                y = by + t.num();
                out.quad_to(xf.x(cx), xf.y(cy), xf.x(x), xf.y(y));
                lcx = cx;
                lcy = cy;
            }
            b'T' => {
                let (cx, cy) = if matches!(last, b'Q' | b'T') {
                    (2.0 * x - lcx, 2.0 * y - lcy)
                } else {
                    (x, y)
                };
                x = bx + t.num();
                y = by + t.num();
                out.quad_to(xf.x(cx), xf.y(cy), xf.x(x), xf.y(y));
                lcx = cx;
                lcy = cy;
            }
            b'A' => {
                let rx = t.num();
                let ry = t.num();
                let rot = t.num();
                let large = t.flag();
                let sweep = t.flag();
                let (nx, ny) = (bx + t.num(), by + t.num());
                arc(out, xf, x, y, rx, ry, rot, large, sweep, nx, ny);
                x = nx;
                y = ny;
            }
            b'Z' => {
                out.close();
                x = sx;
                y = sy;
            }
            _ => break,
        }
        last = cmd.to_ascii_uppercase();
        if last == b'Z' && !t.at_number() {
            continue;
        }
    }
}

fn attr(el: &str, name: &str) -> Option<f32> {
    let key = format!(" {name}=\"");
    let i = el.find(&key)? + key.len();
    let j = el[i..].find('"')? + i;
    el[i..j].trim().parse().ok()
}

fn attr_str<'a>(el: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let i = el.find(&key)? + key.len();
    let j = el[i..].find('"')? + i;
    Some(&el[i..j])
}

/// Toutes les formes d'un fragment SVG (les elements internes d'une icone).
pub fn shapes(svg: &str, xf: Xf, out: &mut Path) {
    for el in svg.split('<').skip(1) {
        let el = format!(" {}", el.trim());
        let tag = el.split_whitespace().next().unwrap_or("");
        match tag {
            "path" => {
                if let Some(d) = attr_str(&el, "d") {
                    path_data(d, xf, out);
                }
            }
            "circle" => {
                let (cx, cy, r) = (
                    attr(&el, "cx").unwrap_or(0.0),
                    attr(&el, "cy").unwrap_or(0.0),
                    attr(&el, "r").unwrap_or(0.0),
                );
                out.circle(xf.x(cx), xf.y(cy), r * xf.k);
            }
            "ellipse" => {
                let (cx, cy, rx, ry) = (
                    attr(&el, "cx").unwrap_or(0.0),
                    attr(&el, "cy").unwrap_or(0.0),
                    attr(&el, "rx").unwrap_or(0.0),
                    attr(&el, "ry").unwrap_or(0.0),
                );
                out.ellipse(xf.x(cx), xf.y(cy), rx * xf.k, ry * xf.k);
            }
            "rect" => {
                let (x, y, w, h) = (
                    attr(&el, "x").unwrap_or(0.0),
                    attr(&el, "y").unwrap_or(0.0),
                    attr(&el, "width").unwrap_or(0.0),
                    attr(&el, "height").unwrap_or(0.0),
                );
                let r = attr(&el, "rx").unwrap_or(0.0);
                out.round_rect(xf.x(x), xf.y(y), w * xf.k, h * xf.k, r * xf.k);
            }
            "line" => {
                let (x1, y1, x2, y2) = (
                    attr(&el, "x1").unwrap_or(0.0),
                    attr(&el, "y1").unwrap_or(0.0),
                    attr(&el, "x2").unwrap_or(0.0),
                    attr(&el, "y2").unwrap_or(0.0),
                );
                out.move_to(xf.x(x1), xf.y(y1)).line_to(xf.x(x2), xf.y(y2));
            }
            "polyline" | "polygon" => {
                if let Some(p) = attr_str(&el, "points") {
                    let v: Vec<f32> = p
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    for (i, c) in v.chunks(2).enumerate() {
                        if c.len() == 2 {
                            if i == 0 {
                                out.move_to(xf.x(c[0]), xf.y(c[1]));
                            } else {
                                out.line_to(xf.x(c[0]), xf.y(c[1]));
                            }
                        }
                    }
                    if tag == "polygon" {
                        out.close();
                    }
                }
            }
            _ => {}
        }
    }
}
