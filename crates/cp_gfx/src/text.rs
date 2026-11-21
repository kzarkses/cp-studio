//! Le texte : un cache de glyphes, rempli par la plateforme.
//!
//! Le moteur ne sait pas dessiner une lettre : il demande a une
//! [`GlyphSource`] (GDI dans `cp_sys`, une fausse source dans les tests) le
//! masque de couverture de chaque caractere, une fois par police et par
//! taille, puis le pose au pixel entier (net).

use std::collections::HashMap;

use crate::{Canvas, Color};

/// Une police : famille (index dans la liste de la source), taille en pixels,
/// graisse (400 normal, 600 demi-gras, 700 gras).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Font {
    pub family: u16,
    pub px: u16,
    pub weight: u16,
}

impl Font {
    pub const fn new(family: u16, px: u16, weight: u16) -> Font {
        Font { family, px, weight }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// Du haut de la ligne a la ligne de base.
    pub ascent: f32,
    /// De la ligne de base au bas de la ligne.
    pub descent: f32,
    pub line: f32,
}

/// Un caractere rasterise.
#[derive(Clone, Debug, Default)]
pub struct Glyph {
    pub w: i32,
    pub h: i32,
    /// Du point de plume au bord gauche du masque.
    pub left: i32,
    /// De la ligne de base au HAUT du masque (vers le haut, positif).
    pub top: i32,
    pub advance: f32,
    pub mask: Vec<u8>,
}

pub trait GlyphSource: Send {
    fn metrics(&mut self, f: Font) -> Metrics;
    fn glyph(&mut self, f: Font, ch: char) -> Option<Glyph>;
}

/// Ou poser un texte dans sa boite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct TextCache {
    src: Box<dyn GlyphSource>,
    glyphs: HashMap<(Font, char), Option<Glyph>>,
    metrics: HashMap<Font, Metrics>,
}

impl TextCache {
    pub fn new(src: Box<dyn GlyphSource>) -> TextCache {
        TextCache {
            src,
            glyphs: HashMap::new(),
            metrics: HashMap::new(),
        }
    }

    pub fn metrics(&mut self, f: Font) -> Metrics {
        let src = &mut self.src;
        *self.metrics.entry(f).or_insert_with(|| src.metrics(f))
    }

    fn glyph(&mut self, f: Font, ch: char) -> Option<&Glyph> {
        let src = &mut self.src;
        self.glyphs
            .entry((f, ch))
            .or_insert_with(|| src.glyph(f, ch))
            .as_ref()
    }

    fn advance(&mut self, f: Font, ch: char) -> f32 {
        self.glyph(f, ch).map_or(0.0, |g| g.advance)
    }

    /// La largeur du texte, en pixels.
    pub fn measure(&mut self, f: Font, s: &str) -> f32 {
        s.chars().map(|c| self.advance(f, c)).sum()
    }

    /// Le plus long debut de `s` qui tient dans `max_w`, avec « … » s'il a
    /// fallu couper.
    pub fn fit(&mut self, f: Font, s: &str, max_w: f32) -> String {
        if self.measure(f, s) <= max_w {
            return s.to_string();
        }
        let ell = self.advance(f, '…');
        let mut w = 0.0;
        let mut out = String::new();
        for c in s.chars() {
            let a = self.advance(f, c);
            if w + a + ell > max_w {
                break;
            }
            w += a;
            out.push(c);
        }
        out.push('…');
        out
    }

    /// Pose `s`, plume en (x, ligne de base y). Rend l'avance totale.
    pub fn draw(&mut self, cv: &mut Canvas<'_>, f: Font, x: f32, y: f32, s: &str, c: Color) -> f32 {
        let (ox, oy) = cv.origin();
        let base = (y + oy).round() as i32;
        let mut pen = x + ox;
        for ch in s.chars() {
            let Some(g) = self.glyph(f, ch) else { continue };
            let gx = pen.round() as i32 + g.left;
            let gy = base - g.top;
            if g.w > 0 && g.h > 0 {
                // draw_mask ajoute l'origine : on la retire ici.
                cv.draw_mask(
                    &g.mask,
                    g.w,
                    g.h,
                    gx - ox.round() as i32,
                    gy - oy.round() as i32,
                    c,
                );
            }
            pen += g.advance;
        }
        pen - (x + ox)
    }

    /// Pose `s` dans la boite, centre verticalement, coupe avec « … » si trop
    /// long.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_in(
        &mut self,
        cv: &mut Canvas<'_>,
        f: Font,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        s: &str,
        c: Color,
        align: Align,
    ) {
        let t = self.fit(f, s, w);
        let tw = self.measure(f, &t);
        let m = self.metrics(f);
        let tx = match align {
            Align::Left => x,
            Align::Center => x + (w - tw) * 0.5,
            Align::Right => x + w - tw,
        };
        let base = y + (h - (m.ascent + m.descent)) * 0.5 + m.ascent;
        self.draw(cv, f, tx, base, &t, c);
    }

    /// Nombre de glyphes en cache (diagnostic).
    pub fn cached(&self) -> usize {
        self.glyphs.len()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Une source de test : chaque caractere est un pave plein de
    /// `px/2 x px*0.7`, avance `px/2 + 1`.
    pub struct Boxes;

    impl GlyphSource for Boxes {
        fn metrics(&mut self, f: Font) -> Metrics {
            let p = f.px as f32;
            Metrics {
                ascent: p * 0.8,
                descent: p * 0.2,
                line: p * 1.2,
            }
        }
        fn glyph(&mut self, f: Font, ch: char) -> Option<Glyph> {
            let w = (f.px / 2) as i32;
            let h = (f.px as f32 * 0.7) as i32;
            if ch == ' ' {
                return Some(Glyph {
                    advance: w as f32 + 1.0,
                    ..Glyph::default()
                });
            }
            Some(Glyph {
                w,
                h,
                left: 0,
                top: h,
                advance: w as f32 + 1.0,
                mask: vec![255; (w * h) as usize],
            })
        }
    }

    #[test]
    fn mesurer_et_couper() {
        let mut t = TextCache::new(Box::new(Boxes));
        let f = Font::new(0, 12, 400);
        assert_eq!(t.measure(f, "abc"), 21.0);
        let s = t.fit(f, "abcdefgh", 30.0);
        assert!(s.ends_with('…'));
        assert!(t.measure(f, &s) <= 30.0);
    }
}
