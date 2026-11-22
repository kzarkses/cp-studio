//! Les lettres, par GDI : chaque caractere est demande une fois en niveaux de
//! gris (`GGO_GRAY8_BITMAP`, avec l'alignement sur la grille de la police :
//! net aux petites tailles), puis garde dans le cache de `cp_gfx`.
//!
//! Au-dela du plan de base (U+10000 et plus : la cle de sol U+1D11E, les
//! silences) `GetGlyphOutlineW` ne prend pas le caractere : on cherche le
//! glyphe dans la table `cmap` de la police (format 12), et on le demande par
//! son numero (`GGO_GLYPH_INDEX`).

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

use cp_gfx::text::{Font, Glyph, GlyphSource, Metrics};

use crate::win::wide;

type Raw = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct TextMetricW {
    height: i32,
    ascent: i32,
    descent: i32,
    internal_leading: i32,
    external_leading: i32,
    ave_char_width: i32,
    max_char_width: i32,
    weight: i32,
    overhang: i32,
    digitized_aspect_x: i32,
    digitized_aspect_y: i32,
    first_char: u16,
    last_char: u16,
    default_char: u16,
    break_char: u16,
    italic: u8,
    underlined: u8,
    struck_out: u8,
    pitch_and_family: u8,
    char_set: u8,
}

#[repr(C)]
#[derive(Default)]
struct GlyphMetrics {
    black_box_x: u32,
    black_box_y: u32,
    origin_x: i32,
    origin_y: i32,
    cell_inc_x: i16,
    cell_inc_y: i16,
}

#[repr(C)]
struct Fixed {
    fract: u16,
    value: i16,
}

#[repr(C)]
struct Mat2 {
    m11: Fixed,
    m12: Fixed,
    m21: Fixed,
    m22: Fixed,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: Raw) -> Raw;
    fn DeleteDC(dc: Raw) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn CreateFontW(
        h: i32,
        w: i32,
        esc: i32,
        orient: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out_prec: u32,
        clip_prec: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Raw;
    fn SelectObject(dc: Raw, obj: Raw) -> Raw;
    fn DeleteObject(obj: Raw) -> i32;
    fn GetTextMetricsW(dc: Raw, tm: *mut TextMetricW) -> i32;
    fn GetGlyphOutlineW(
        dc: Raw,
        ch: u32,
        format: u32,
        gm: *mut GlyphMetrics,
        n: u32,
        buf: *mut c_void,
        mat: *const Mat2,
    ) -> u32;
    fn GetFontData(dc: Raw, table: u32, offset: u32, buf: *mut c_void, n: u32) -> u32;
}

const GGO_GRAY8_BITMAP: u32 = 6;
const GGO_GLYPH_INDEX: u32 = 0x80;

/// Les groupes (premier, dernier, premier glyphe) de la sous-table de format
/// 12 d'une table `cmap` : la correspondance caractere -> glyphe au-dela du
/// plan de base.
fn cmap12(c: &[u8]) -> Vec<(u32, u32, u32)> {
    let u16_at = |p: usize| {
        c.get(p..p + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as u32)
    };
    let u32_at = |p: usize| {
        c.get(p..p + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let Some(n) = u16_at(2) else {
        return Vec::new();
    };
    for i in 0..n as usize {
        let Some(off) = u32_at(8 + 8 * i) else { break };
        let off = off as usize;
        if u16_at(off) != Some(12) {
            continue;
        }
        let Some(ng) = u32_at(off + 12) else { break };
        let mut out = Vec::with_capacity(ng.min(100_000) as usize);
        for k in 0..ng as usize {
            let p = off + 16 + 12 * k;
            match (u32_at(p), u32_at(p + 4), u32_at(p + 8)) {
                (Some(a), Some(b), Some(g)) => out.push((a, b, g)),
                _ => break,
            }
        }
        return out;
    }
    Vec::new()
}
const GDI_ERROR: u32 = 0xFFFF_FFFF;

/// La source de glyphes de GDI. Une par fenetre de dessin (ou partagee sur le
/// fil principal) : elle tient un DC memoire et ses polices.
pub struct GdiGlyphs {
    dc: Raw,
    families: Vec<Vec<u16>>,
    fonts: HashMap<Font, Raw>,
    /// 0..64 (GDI) vers 0..255, un peu releve : le gris de GDI est lineaire et
    /// une lettre claire sur fond sombre parait sinon trop maigre.
    ramp: [u8; 65],
    /// La table `cmap` (format 12) de chaque famille, lue au premier besoin.
    cmaps: HashMap<u16, Vec<(u32, u32, u32)>>,
}

impl GdiGlyphs {
    /// `families[i]` est la famille de `Font { family: i, .. }`.
    pub fn new(families: &[&str]) -> GdiGlyphs {
        // SAFETY: DC memoire compatible avec l'ecran, libere au Drop.
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        let mut ramp = [0u8; 65];
        for (i, r) in ramp.iter_mut().enumerate() {
            *r = ((i as f32 / 64.0).powf(0.8) * 255.0 + 0.5) as u8;
        }
        GdiGlyphs {
            dc,
            families: families.iter().map(|f| wide(f)).collect(),
            fonts: HashMap::new(),
            ramp,
            cmaps: HashMap::new(),
        }
    }

    /// Le glyphe d'un caractere hors du plan de base, dans la police choisie
    /// (0 : absent).
    fn glyph_index(&mut self, f: Font, code: u32) -> u32 {
        let dc = self.dc;
        let groups = self.cmaps.entry(f.family).or_insert_with(|| {
            // 'cmap', octets dans l'ordre du fichier.
            let tag = u32::from_le_bytes(*b"cmap");
            // SAFETY: premier appel sans tampon : la taille de la table.
            let n = unsafe { GetFontData(dc, tag, 0, null_mut(), 0) };
            if n == GDI_ERROR || n == 0 || n > 16 << 20 {
                return Vec::new();
            }
            let mut buf = vec![0u8; n as usize];
            // SAFETY: tampon de la taille annoncee.
            let r = unsafe { GetFontData(dc, tag, 0, buf.as_mut_ptr() as *mut c_void, n) };
            if r == GDI_ERROR {
                return Vec::new();
            }
            cmap12(&buf)
        });
        groups
            .iter()
            .find(|&&(a, b, _)| a <= code && code <= b)
            .map_or(0, |&(a, _, g)| g + code - a)
    }

    fn select(&mut self, f: Font) -> bool {
        if self.dc.is_null() {
            return false;
        }
        let fam = self
            .families
            .get(f.family as usize)
            .or(self.families.first())
            .cloned()
            .unwrap_or_else(|| wide("Segoe UI"));
        let h = *self.fonts.entry(f).or_insert_with(|| {
            // SAFETY: nom de famille termine par zero ; police liberee au Drop.
            unsafe {
                CreateFontW(
                    -(f.px as i32),
                    0,
                    0,
                    0,
                    f.weight as i32,
                    0,
                    0,
                    0,
                    1, // DEFAULT_CHARSET
                    4, // OUT_TT_PRECIS
                    0,
                    4, // ANTIALIASED_QUALITY
                    0,
                    fam.as_ptr(),
                )
            }
        });
        if h.is_null() {
            return false;
        }
        // SAFETY: DC et police vivants.
        unsafe { SelectObject(self.dc, h) };
        true
    }
}

impl GlyphSource for GdiGlyphs {
    fn metrics(&mut self, f: Font) -> Metrics {
        if !self.select(f) {
            let p = f.px as f32;
            return Metrics {
                ascent: p * 0.8,
                descent: p * 0.2,
                line: p * 1.2,
            };
        }
        let mut tm = TextMetricW::default();
        // SAFETY: ecrit un TEXTMETRICW.
        unsafe { GetTextMetricsW(self.dc, &mut tm) };
        Metrics {
            ascent: tm.ascent as f32,
            descent: tm.descent as f32,
            line: (tm.height + tm.external_leading) as f32,
        }
    }

    fn glyph(&mut self, f: Font, ch: char) -> Option<Glyph> {
        let mut code = ch as u32;
        if !self.select(f) {
            return None;
        }
        let mut format = GGO_GRAY8_BITMAP;
        if code > 0xFFFF {
            code = self.glyph_index(f, code);
            if code == 0 {
                return None;
            }
            format |= GGO_GLYPH_INDEX;
        }
        let mat = Mat2 {
            m11: Fixed { fract: 0, value: 1 },
            m12: Fixed { fract: 0, value: 0 },
            m21: Fixed { fract: 0, value: 0 },
            m22: Fixed { fract: 0, value: 1 },
        };
        let mut gm = GlyphMetrics::default();
        // SAFETY: premier appel sans tampon : rend la taille necessaire.
        let n = unsafe { GetGlyphOutlineW(self.dc, code, format, &mut gm, 0, null_mut(), &mat) };
        if n == GDI_ERROR {
            return None;
        }
        let advance = gm.cell_inc_x as f32;
        if n == 0 {
            return Some(Glyph {
                advance,
                ..Glyph::default()
            });
        }
        let mut buf = vec![0u8; n as usize];
        // SAFETY: tampon de la taille annoncee par le premier appel.
        let r = unsafe {
            GetGlyphOutlineW(
                self.dc,
                code,
                format,
                &mut gm,
                n,
                buf.as_mut_ptr() as *mut c_void,
                &mat,
            )
        };
        if r == GDI_ERROR {
            return None;
        }
        let (w, h) = (gm.black_box_x as i32, gm.black_box_y as i32);
        // Les lignes du tampon sont alignees sur 4 octets.
        let pitch = ((w + 3) & !3) as usize;
        let mut mask = vec![0u8; (w * h) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let v = buf.get(y * pitch + x).copied().unwrap_or(0).min(64);
                mask[y * w as usize + x] = self.ramp[v as usize];
            }
        }
        Some(Glyph {
            w,
            h,
            left: gm.origin_x,
            top: gm.origin_y,
            advance,
            mask,
        })
    }
}

impl Drop for GdiGlyphs {
    fn drop(&mut self) {
        // SAFETY: objets crees par nous ; le DC d'abord (il ne tient plus de
        // police que nous allons detruire une fois desselectionnee par sa mort).
        unsafe {
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
            for (_, h) in self.fonts.drain() {
                if !h.is_null() {
                    DeleteObject(h);
                }
            }
        }
    }
}

// SAFETY: le DC et les polices ne sont touches que par `&mut self`, donc par
// un fil a la fois ; GDI accepte qu'un DC memoire change de fil.
unsafe impl Send for GdiGlyphs {}
