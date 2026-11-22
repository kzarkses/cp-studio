//! Le decodage PNG, sans Windows : tous les types de couleur, profondeurs 1
//! a 16 bits, transparence `tRNS`, entrelacement Adam7. Rend des pixels
//! BGRA PREMULTIPLIES (le format du moteur de dessin), comme WIC.
//!
//! Pourquoi pas WIC : les images d'un theme sont DANS un zip ; les decoder ici
//! garde toute la lecture du theme testable hors de Windows.

use cp_gfx::Surface;

use crate::inflate::zlib;

struct Header {
    w: usize,
    h: usize,
    depth: u8,
    color: u8,
    interlace: u8,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }
    /// Octets d'une ligne de `w` pixels (sans l'octet de filtre).
    fn stride(&self, w: usize) -> usize {
        (w * self.channels() * self.depth as usize).div_ceil(8)
    }
    /// L'ecart du filtre : octets d'un pixel complet, au moins 1.
    fn bpp(&self) -> usize {
        (self.channels() * self.depth as usize).div_ceil(8)
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Defiltre `h` lignes de `stride` octets lues dans `src` ; rend les lignes
/// et ce qui a ete consomme.
fn unfilter(src: &[u8], stride: usize, h: usize, bpp: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = vec![0u8; stride * h];
    let mut p = 0;
    for y in 0..h {
        let f = *src.get(p)?;
        let line = src.get(p + 1..p + 1 + stride)?;
        p += 1 + stride;
        let (prev, cur) = out.split_at_mut(y * stride);
        let up = if y > 0 {
            &prev[(y - 1) * stride..]
        } else {
            &[][..]
        };
        let cur = &mut cur[..stride];
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = up.get(i).copied().unwrap_or(0);
            let c = if i >= bpp {
                up.get(i - bpp).copied().unwrap_or(0)
            } else {
                0
            };
            let x = line[i];
            cur[i] = match f {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
        }
    }
    Some((out, p))
}

/// L'echantillon `i` (canal compris) d'une ligne, a sa profondeur d'origine.
fn sample(line: &[u8], i: usize, depth: u8) -> u16 {
    match depth {
        8 => line[i] as u16,
        16 => u16::from_be_bytes([line[2 * i], line[2 * i + 1]]),
        d => {
            let bit = i * d as usize;
            let byte = line[bit / 8];
            let shift = 8 - d as usize - (bit % 8);
            ((byte >> shift) & ((1u8 << d) - 1)) as u16
        }
    }
}

struct Palette {
    rgb: Vec<[u8; 3]>,
    alpha: Vec<u8>,
}

/// Les pixels (r, g, b, a) d'une ligne defiltree.
fn pixel(hd: &Header, pal: &Palette, trns: Option<&[u8]>, line: &[u8], x: usize) -> [u8; 4] {
    let d = hd.depth;
    // Un echantillon ramene sur 8 bits.
    let to8 = |v: u16| -> u8 {
        match d {
            16 => (v >> 8) as u8,
            8 => v as u8,
            _ => (v as u32 * 255 / ((1u32 << d) - 1)) as u8,
        }
    };
    // La couleur transparente de tRNS (types 0 et 2), a la profondeur d'origine.
    let key = |k: usize| -> Option<u16> {
        let t = trns?;
        Some(u16::from_be_bytes([*t.get(2 * k)?, *t.get(2 * k + 1)?]))
    };
    match hd.color {
        0 => {
            let g = sample(line, x, d);
            let a = if key(0) == Some(g) { 0 } else { 255 };
            let g = to8(g);
            [g, g, g, a]
        }
        2 => {
            let (r, g, b) = (
                sample(line, 3 * x, d),
                sample(line, 3 * x + 1, d),
                sample(line, 3 * x + 2, d),
            );
            let t = (key(0), key(1), key(2));
            let a = if t == (Some(r), Some(g), Some(b)) {
                0
            } else {
                255
            };
            [to8(r), to8(g), to8(b), a]
        }
        3 => {
            let i = sample(line, x, d) as usize;
            let c = pal.rgb.get(i).copied().unwrap_or([0, 0, 0]);
            let a = pal.alpha.get(i).copied().unwrap_or(255);
            [c[0], c[1], c[2], a]
        }
        4 => {
            let g = to8(sample(line, 2 * x, d));
            [g, g, g, to8(sample(line, 2 * x + 1, d))]
        }
        _ => [
            to8(sample(line, 4 * x, d)),
            to8(sample(line, 4 * x + 1, d)),
            to8(sample(line, 4 * x + 2, d)),
            to8(sample(line, 4 * x + 3, d)),
        ],
    }
}

/// (r, g, b, a) droit vers BGRA premultiplie.
fn premul([r, g, b, a]: [u8; 4]) -> u32 {
    let m = |c: u8| (c as u32 * a as u32 + 127) / 255;
    (a as u32) << 24 | m(r) << 16 | m(g) << 8 | m(b)
}

/// Decode un PNG. `None` s'il est invalide ou demesure (plus de 8192 px).
pub fn decode(bytes: &[u8]) -> Option<Surface> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut p = 8;
    let mut hd: Option<Header> = None;
    let mut pal = Palette {
        rgb: Vec::new(),
        alpha: Vec::new(),
    };
    let mut trns: Option<Vec<u8>> = None;
    let mut idat = Vec::new();
    while p + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[p..p + 4].try_into().ok()?) as usize;
        let kind = &bytes[p + 4..p + 8];
        let body = bytes.get(p + 8..p + 8 + len)?;
        match kind {
            b"IHDR" => {
                let w = u32::from_be_bytes(body.get(0..4)?.try_into().ok()?) as usize;
                let h = u32::from_be_bytes(body.get(4..8)?.try_into().ok()?) as usize;
                let (depth, color, interlace) = (*body.get(8)?, *body.get(9)?, *body.get(12)?);
                let ok_depth = match color {
                    0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(depth, 8 | 16),
                    _ => false,
                };
                if !ok_depth || w == 0 || h == 0 || w > 8192 || h > 8192 || interlace > 1 {
                    return None;
                }
                hd = Some(Header {
                    w,
                    h,
                    depth,
                    color,
                    interlace,
                });
            }
            b"PLTE" => {
                pal.rgb = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => {
                if hd.as_ref().is_some_and(|h| h.color == 3) {
                    pal.alpha = body.to_vec();
                } else {
                    trns = Some(body.to_vec());
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    let hd = hd?;
    let bpp = hd.bpp();
    // Taille exacte des donnees defiltrees, passes comprises.
    const PASSES: [(usize, usize, usize, usize); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let passes: Vec<(usize, usize, usize, usize)> = if hd.interlace == 1 {
        PASSES.to_vec()
    } else {
        vec![(0, 0, 1, 1)]
    };
    let mut total = 0;
    for &(x0, y0, dx, dy) in &passes {
        let pw = (hd.w + dx - 1 - x0) / dx;
        let ph = (hd.h + dy - 1 - y0) / dy;
        if pw > 0 && ph > 0 {
            total += (hd.stride(pw) + 1) * ph;
        }
    }
    let raw = zlib(&idat, total)?;
    let mut out = Surface::new(hd.w as i32, hd.h as i32);
    let mut at = 0;
    for &(x0, y0, dx, dy) in &passes {
        let pw = (hd.w + dx - 1 - x0) / dx;
        let ph = (hd.h + dy - 1 - y0) / dy;
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = hd.stride(pw);
        let (lines, used) = unfilter(raw.get(at..)?, stride, ph, bpp)?;
        at += used;
        for py in 0..ph {
            let line = &lines[py * stride..(py + 1) * stride];
            let y = y0 + py * dy;
            for px in 0..pw {
                let x = x0 + px * dx;
                out.px[y * hd.w + x] = premul(pixel(&hd, &pal, trns.as_deref(), line, px));
            }
        }
    }
    Some(out)
}
