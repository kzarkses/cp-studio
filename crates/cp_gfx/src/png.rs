//! Une surface en PNG, sans dependance (deflate « stocke », sans compression).
//!
//! Pour REGARDER ce que le moteur dessine, hors de REAPER : les tests et les
//! outils ecrivent des PNG que Claude et Cedric peuvent ouvrir.

use crate::{Color, Surface};

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut c = Vec::with_capacity(4 + data.len());
    c.extend(kind);
    c.extend(data);
    out.extend(&c);
    out.extend(crc32(&c).to_be_bytes());
}

/// Les octets d'un PNG RGBA (alpha droit) de la surface.
pub fn encode(s: &Surface) -> Vec<u8> {
    let (w, h) = (s.w.max(0) as usize, s.h.max(0) as usize);
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0); // filtre : aucun
        for x in 0..w {
            let c = Color::from_premul(s.px[y * w + x]);
            raw.extend([c.r, c.g, c.b, c.a]);
        }
    }
    // zlib : en-tete, blocs stockes de 65535 octets au plus, adler32.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    if blocks.is_empty() {
        z.extend([1, 0, 0, 0xFF, 0xFF]);
    }
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        let n = b.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend(*b);
    }
    z.extend(adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8 bits, RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_et_adler_connus() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn un_png_a_la_bonne_signature() {
        let s = Surface::new(3, 2);
        let b = encode(&s);
        assert_eq!(&b[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&b[12..16], b"IHDR");
    }
}
