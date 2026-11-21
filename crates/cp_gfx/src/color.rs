//! Les couleurs.
//!
//! L'API parle en couleurs DROITES (`Color`, alpha separe). Les pixels sont
//! stockes PREMULTIPLIES, en `u32` = `a << 24 | r << 16 | g << 8 | b` : en
//! memoire (petit-boutiste) c'est B, G, R, A, l'ordre d'une DIB 32 bits de
//! Windows. La composition « source sur destination » d'un premultiplie est une
//! addition : `d = s + d * (1 - as)`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

/// `0xRRGGBB`.
pub const fn hex(v: u32) -> Color {
    rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[inline]
pub(crate) fn div255(x: u32) -> u32 {
    // Division exacte par 255, arrondie (x <= 255 * 255 + 127).
    (x + 128 + ((x + 128) >> 8)) >> 8
}

impl Color {
    pub const TRANSPARENT: Color = rgba(0, 0, 0, 0);
    pub const BLACK: Color = rgb(0, 0, 0);
    pub const WHITE: Color = rgb(255, 255, 255);

    pub const fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }

    /// La couleur, alpha multiplie par `k` (0..1).
    pub fn fade(self, k: f32) -> Color {
        Color {
            a: (self.a as f32 * k.clamp(0.0, 1.0) + 0.5) as u8,
            ..self
        }
    }

    /// Melange droit : `t` = 0 donne `self`, 1 donne `o`.
    pub fn mix(self, o: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8;
        Color {
            r: m(self.r, o.r),
            g: m(self.g, o.g),
            b: m(self.b, o.b),
            a: m(self.a, o.a),
        }
    }

    /// Le pixel premultiplie.
    pub fn premul(self) -> u32 {
        let a = self.a as u32;
        let r = div255(self.r as u32 * a);
        let g = div255(self.g as u32 * a);
        let b = div255(self.b as u32 * a);
        a << 24 | r << 16 | g << 8 | b
    }

    /// D'un pixel premultiplie vers une couleur droite.
    pub fn from_premul(p: u32) -> Color {
        let a = p >> 24;
        if a == 0 {
            return Color::TRANSPARENT;
        }
        let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
        Color {
            r: un((p >> 16) & 255),
            g: un((p >> 8) & 255),
            b: un(p & 255),
            a: a as u8,
        }
    }
}

/// `src` (premultiplie) pondere par `cov` (0..255), compose sur `dst`.
#[inline]
pub(crate) fn over(dst: u32, src: u32, cov: u32) -> u32 {
    if cov == 0 {
        return dst;
    }
    let s = if cov >= 255 { src } else { scale(src, cov) };
    let sa = s >> 24;
    if sa == 255 {
        return s;
    }
    if sa == 0 && s == 0 {
        return dst;
    }
    let inv = 255 - sa;
    // Deux canaux a la fois : (r, b) puis (a, g), 8 bits de marge chacun.
    let rb = (dst & 0x00FF_00FF) * inv + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
    let ag = ((dst >> 8) & 0x00FF_00FF) * inv + 0x0080_0080;
    let ag = (ag + ((ag >> 8) & 0x00FF_00FF)) & 0xFF00_FF00;
    s.wrapping_add(rb | ag)
}

/// Un pixel premultiplie multiplie par `k` / 255.
#[inline]
pub(crate) fn scale(p: u32, k: u32) -> u32 {
    let rb = (p & 0x00FF_00FF) * k + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
    let ag = ((p >> 8) & 0x00FF_00FF) * k + 0x0080_0080;
    let ag = (ag + ((ag >> 8) & 0x00FF_00FF)) & 0xFF00_FF00;
    rb | ag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div255_est_exacte() {
        for x in 0..=255 * 255 {
            assert_eq!(div255(x), (x as f64 / 255.0).round() as u32, "{x}");
        }
    }

    #[test]
    fn la_composition_est_celle_du_calcul_reel() {
        // Contre le calcul flottant, a un niveau pres.
        let mut seed = 1u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 24
        };
        for _ in 0..20000 {
            let s = Color {
                r: next() as u8,
                g: next() as u8,
                b: next() as u8,
                a: next() as u8,
            };
            let d = Color {
                r: next() as u8,
                g: next() as u8,
                b: next() as u8,
                a: next() as u8,
            };
            let cov = next();
            let got = over(d.premul(), s.premul(), cov);
            let sp = s.premul();
            for sh in [0, 8, 16, 24] {
                let sc = ((sp >> sh) & 255) as f64 * cov as f64 / 255.0;
                let sa = (sp >> 24) as f64 * cov as f64 / 255.0;
                let dc = ((d.premul() >> sh) & 255) as f64;
                let want = sc + dc * (1.0 - sa / 255.0);
                let g = ((got >> sh) & 255) as f64;
                assert!((g - want).abs() <= 1.5, "canal {sh}: {g} contre {want}");
            }
        }
    }

    #[test]
    fn un_opaque_plein_remplace() {
        let c = rgb(10, 200, 30).premul();
        assert_eq!(over(0x1234_5678, c, 255), c);
        assert_eq!(over(0x1234_5678, c, 0), 0x1234_5678);
    }
}
