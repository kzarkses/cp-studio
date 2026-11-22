//! Des pages en PDF, sans dependance : chaque page est une image en niveaux
//! de gris (la surface dessinee par le moteur), compressee en deflate.
//!
//! Le deflate est le notre : LZ77 (fenetre de 32 Kio, chaines de hachage)
//! et les codes de Huffman FIXES de la norme (RFC 1951, bloc de type 1). Une
//! page de partition est surtout blanche : les longues plages blanches
//! tiennent en quelques bits, et une page A4 a 300 DPI (8,7 Mo bruts) pese
//! quelques centaines de Kio.

use crate::{Color, Surface};

// ---------------------------------------------------------------------------
// Deflate (RFC 1951) et zlib (RFC 1950)
// ---------------------------------------------------------------------------

/// Les longueurs de copie : (debut de chaque code 257..285, bits en plus).
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Les distances : (debut de chaque code 0..29, bits en plus).
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const WINDOW: usize = 1 << 15;
const MAX_MATCH: usize = 258;
const MIN_MATCH: usize = 3;
const HASH_BITS: u32 = 15;
/// Les candidats essayes par position (assez pour une page de musique).
const CHAIN: usize = 24;

/// Les bits, du poids faible au poids fort (l'ordre de deflate).
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Un code de Huffman : ecrit du bit de poids fort au plus faible.
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0u32;
        for k in 0..len {
            r |= ((code >> k) & 1) << (len - 1 - k);
        }
        self.put(r, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Un symbole litteral ou longueur (0..=285), en code fixe.
fn lit(b: &mut Bits, v: u32) {
    match v {
        0..=143 => b.code(0x30 + v, 8),
        144..=255 => b.code(0x190 + v - 144, 9),
        256..=279 => b.code(v - 256, 7),
        _ => b.code(0xC0 + v - 280, 8),
    }
}

fn copy(b: &mut Bits, len: usize, dist: usize) {
    let li = LEN_BASE
        .iter()
        .rposition(|&x| x as usize <= len)
        .unwrap_or(0);
    lit(b, 257 + li as u32);
    if LEN_EXTRA[li] > 0 {
        b.put((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
    }
    let di = DIST_BASE
        .iter()
        .rposition(|&x| x as usize <= dist)
        .unwrap_or(0);
    b.code(di as u32, 5);
    if DIST_EXTRA[di] > 0 {
        b.put(
            (dist - DIST_BASE[di] as usize) as u32,
            DIST_EXTRA[di] as u32,
        );
    }
}

fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) << 16 | (d[i + 1] as u32) << 8 | d[i + 2] as u32;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Un flux deflate brut (un seul bloc a codes fixes).
pub fn deflate(d: &[u8]) -> Vec<u8> {
    let mut b = Bits {
        out: Vec::with_capacity(d.len() / 8 + 64),
        acc: 0,
        n: 0,
    };
    b.put(1, 1); // dernier bloc
    b.put(1, 2); // codes fixes
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; WINDOW];
    let insert = |head: &mut [usize], prev: &mut [usize], i: usize| {
        if i + MIN_MATCH <= d.len() {
            let h = hash3(d, i);
            prev[i & (WINDOW - 1)] = head[h];
            head[h] = i;
        }
    };
    let mut i = 0;
    while i < d.len() {
        let mut best = (0usize, 0usize);
        if i + MIN_MATCH <= d.len() {
            let max = (d.len() - i).min(MAX_MATCH);
            let mut cand = head[hash3(d, i)];
            let mut tries = 0;
            while cand != usize::MAX && i - cand < WINDOW && tries < CHAIN {
                if d[cand + best.0.min(max - 1)] == d[i + best.0.min(max - 1)] {
                    let mut l = 0;
                    while l < max && d[cand + l] == d[i + l] {
                        l += 1;
                    }
                    if l > best.0 {
                        best = (l, i - cand);
                        if l == max {
                            break;
                        }
                    }
                }
                let next = prev[cand & (WINDOW - 1)];
                // La chaine ne remonte que vers le passe.
                if next == usize::MAX || next >= cand {
                    break;
                }
                cand = next;
                tries += 1;
            }
        }
        if best.0 >= MIN_MATCH {
            copy(&mut b, best.0, best.1);
            for k in i..i + best.0 {
                insert(&mut head, &mut prev, k);
            }
            i += best.0;
        } else {
            lit(&mut b, d[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    lit(&mut b, 256);
    b.finish()
}

/// Un flux zlib (l'enveloppe que PDF appelle FlateDecode).
pub fn zlib(d: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    z.extend(deflate(d));
    z.extend(crate::png::adler32(d).to_be_bytes());
    z
}

// ---------------------------------------------------------------------------
// Le PDF
// ---------------------------------------------------------------------------

/// La taille A4, en points PDF (1/72 de pouce).
pub const A4_PT: (f32, f32) = (595.276, 841.89);

/// Les niveaux de gris d'une surface (posee sur du blanc).
pub fn gray(s: &Surface) -> Vec<u8> {
    s.px.iter()
        .map(|&p| {
            let c = Color::from_premul(p);
            let a = c.a as f32 / 255.0;
            let l = 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
            (l * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Une chaine PDF : en UTF-16 (hexadecimal), tout caractere passe.
fn text_string(s: &str) -> String {
    let mut o = String::from("<FEFF");
    for u in s.encode_utf16() {
        o += &format!("{u:04X}");
    }
    o.push('>');
    o
}

/// Une page prete : son image en gris, compressee. Une surface A4 a 300 DPI
/// pese 35 Mo : chaque page est compressee des qu'elle est dessinee, et sa
/// surface sert a la suivante.
pub struct Page {
    pub w: i32,
    pub h: i32,
    pub z: Vec<u8>,
}

impl Page {
    pub fn of(s: &Surface) -> Page {
        Page {
            w: s.w,
            h: s.h,
            z: zlib(&gray(s)),
        }
    }
}

/// Un PDF : une page par image, chacune etiree sur `size` (points).
pub fn write(pages: &[Page], size: (f32, f32), title: &str) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offs: Vec<usize> = Vec::new();
    let n = pages.len();
    // 1 le catalogue, 2 les pages, 3 les infos ; puis par page : la page,
    // son contenu, son image.
    let page_id = |k: usize| 4 + 3 * k;
    let obj = |out: &mut Vec<u8>, offs: &mut Vec<usize>, body: &[u8]| {
        offs.push(out.len());
        out.extend(format!("{} 0 obj\n", offs.len()).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    obj(&mut out, &mut offs, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|k| format!("{} 0 R", page_id(k))).collect();
    obj(
        &mut out,
        &mut offs,
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).as_bytes(),
    );
    obj(
        &mut out,
        &mut offs,
        format!(
            "<< /Title {} /Producer {} >>",
            text_string(title),
            text_string("CP Studio")
        )
        .as_bytes(),
    );
    let (w, h) = size;
    for (k, s) in pages.iter().enumerate() {
        let id = page_id(k);
        obj(
            &mut out,
            &mut offs,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                id + 2,
                id + 1
            )
            .as_bytes(),
        );
        let content = format!("q {w:.2} 0 0 {h:.2} 0 0 cm /Im0 Do Q");
        obj(
            &mut out,
            &mut offs,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            )
            .as_bytes(),
        );
        let z = &s.z;
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Interpolate true /Filter /FlateDecode /Length {} >>\nstream\n",
            s.w,
            s.h,
            z.len()
        )
        .into_bytes();
        body.extend(z);
        body.extend(b"\nendstream");
        obj(&mut out, &mut offs, &body);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offs.len() + 1).as_bytes());
    for o in &offs {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offs.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le decodeur d'un bloc a codes fixes (ce que `deflate` ecrit), pour
    /// verifier l'aller-retour.
    fn inflate_fixed(z: &[u8]) -> Vec<u8> {
        let mut pos = 0usize;
        let bit = |pos: &mut usize| -> u32 {
            let v = (z[*pos / 8] >> (*pos % 8)) & 1;
            *pos += 1;
            v as u32
        };
        let bits = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for k in 0..n {
                v |= bit(pos) << k;
            }
            v
        };
        let code = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for _ in 0..n {
                v = (v << 1) | bit(pos);
            }
            v
        };
        assert_eq!(bits(&mut pos, 1), 1);
        assert_eq!(bits(&mut pos, 2), 1);
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Le symbole : 7 bits, puis 8, puis 9.
            let mut v = code(&mut pos, 7);
            let sym = if v <= 0x17 {
                v + 256
            } else {
                v = (v << 1) | bit(&mut pos);
                if (0x30..=0xBF).contains(&v) {
                    v - 0x30
                } else if (0xC0..=0xC7).contains(&v) {
                    v - 0xC0 + 280
                } else {
                    v = (v << 1) | bit(&mut pos);
                    v - 0x190 + 144
                }
            };
            match sym {
                0..=255 => out.push(sym as u8),
                256 => break,
                _ => {
                    let li = (sym - 257) as usize;
                    let len = LEN_BASE[li] as usize + bits(&mut pos, LEN_EXTRA[li] as u32) as usize;
                    let di = code(&mut pos, 5) as usize;
                    let dist =
                        DIST_BASE[di] as usize + bits(&mut pos, DIST_EXTRA[di] as u32) as usize;
                    let from = out.len() - dist;
                    for k in 0..len {
                        out.push(out[from + k]);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn deflate_fait_l_aller_retour() {
        let mut d: Vec<u8> = Vec::new();
        // Du blanc, du texte, des motifs, du bruit.
        d.extend(std::iter::repeat_n(255u8, 70_000));
        d.extend(b"La tablature de Cedric, mesure 1, mesure 2, mesure 3.".repeat(40));
        let mut x = 12345u32;
        for _ in 0..50_000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            d.push((x >> 16) as u8);
        }
        d.extend(std::iter::repeat_n(0u8, 300));
        d.extend([1, 2, 3]);
        let z = deflate(&d);
        assert_eq!(inflate_fixed(&z), d);
        // Le blanc ne coute presque rien.
        let white = deflate(&vec![255u8; 1_000_000]);
        assert!(white.len() < 7_000, "{}", white.len());
        assert!(deflate(&[]).len() <= 2);
    }

    #[test]
    fn un_pdf_bien_forme() {
        let mut s = Surface::new(40, 20);
        for p in s.px.iter_mut() {
            *p = 0xFFFF_FFFF;
        }
        let b = write(&[Page::of(&s), Page::of(&s)], A4_PT, "Riff (2) é");
        // Les octets un a un (les flux sont binaires : pas de texte).
        let find = |pat: &[u8]| b.windows(pat.len()).rposition(|w| w == pat);
        assert!(b.starts_with(b"%PDF-1.4"));
        assert!(find(b"/Count 2").is_some());
        assert!(b.ends_with(b"%%EOF\n"));
        // La table des renvois pointe sur chaque objet.
        let at = find(b"startxref\n").unwrap() + 10;
        let end = at + b[at..].iter().position(|&c| c == b'\n').unwrap();
        let xref: usize = std::str::from_utf8(&b[at..end]).unwrap().parse().unwrap();
        assert!(b[xref..].starts_with(b"xref\n0 10\n"));
        let table = std::str::from_utf8(&b[xref..at - 10]).unwrap();
        let table = table.split("trailer").next().unwrap();
        for (k, line) in table.lines().skip(3).enumerate() {
            let o: usize = line[..10].parse().unwrap();
            assert!(
                b[o..].starts_with(format!("{} 0 obj\n", k + 1).as_bytes()),
                "{k}"
            );
        }
        assert_eq!(table.lines().count(), 3 + 9);
        assert!(find(b"<FEFF005200690066006600200028003200290020").is_some());
    }
}
