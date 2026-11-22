//! DEFLATE (RFC 1951), en lecture seule : ce dont ont besoin un zip de theme
//! et un PNG. Decodage canonique bit a bit (a la maniere de `puff.c` de Mark
//! Adler) : court et sur ; les images d'un theme font quelques Ko.

/// Les bits d'un flux DEFLATE, poids faible d'abord.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl Bits<'_> {
    fn bits(&mut self, need: u32) -> Option<u32> {
        let mut val = self.buf;
        while self.cnt < need {
            let b = *self.d.get(self.pos)? as u32;
            self.pos += 1;
            val |= b << self.cnt;
            self.cnt += 8;
        }
        self.buf = if need >= 32 { 0 } else { val >> need };
        self.cnt -= need;
        Some(if need == 0 {
            0
        } else {
            val & ((1u32 << need) - 1)
        })
    }
}

/// Un code de Huffman canonique : combien de codes par longueur, et les
/// symboles ranges par code.
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huff {
    fn new(lengths: &[u8]) -> Option<Huff> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize & 15] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return None; // sur-souscrit
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let o = &mut offs[l as usize];
                symbol[*o as usize] = sym as u16;
                *o += 1;
            }
        }
        Some(Huff { count, symbol })
    }

    fn decode(&self, b: &mut Bits<'_>) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

const LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEXT: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn codes(b: &mut Bits<'_>, out: &mut Vec<u8>, lit: &Huff, dist: &Huff, max: usize) -> Option<()> {
    loop {
        let sym = lit.decode(b)? as usize;
        if sym < 256 {
            if out.len() >= max {
                return None;
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Some(());
        } else {
            let i = sym - 257;
            if i >= 29 {
                return None;
            }
            let len = LBASE[i] as usize + b.bits(LEXT[i] as u32)? as usize;
            let ds = dist.decode(b)? as usize;
            if ds >= 30 {
                return None;
            }
            let d = DBASE[ds] as usize + b.bits(DEXT[ds] as u32)? as usize;
            if d > out.len() || out.len() + len > max {
                return None;
            }
            let start = out.len() - d;
            for k in 0..len {
                let c = out[start + k];
                out.push(c);
            }
        }
    }
}

/// Decompresse un flux DEFLATE brut. `max` borne la taille rendue (un fichier
/// malforme ne remplit pas la memoire). `None` si le flux est invalide.
pub fn inflate(data: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut b = Bits {
        d: data,
        pos: 0,
        buf: 0,
        cnt: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = b.bits(1)?;
        match b.bits(2)? {
            0 => {
                // Bloc stocke : on repart a l'octet.
                b.buf = 0;
                b.cnt = 0;
                let p = b.pos;
                let hdr = data.get(p..p + 4)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
                if nlen != !(len as u16) || out.len() + len > max {
                    return None;
                }
                out.extend_from_slice(data.get(p + 4..p + 4 + len)?);
                b.pos = p + 4 + len;
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let lit = Huff::new(&l)?;
                let dist = Huff::new(&[5u8; 30])?;
                codes(&mut b, &mut out, &lit, &dist, max)?;
            }
            2 => {
                let nlen = b.bits(5)? as usize + 257;
                let ndist = b.bits(5)? as usize + 1;
                let ncode = b.bits(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return None;
                }
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl = [0u8; 19];
                for &o in &ORDER[..ncode] {
                    cl[o] = b.bits(3)? as u8;
                }
                let lencode = Huff::new(&cl)?;
                let mut lengths = vec![0u8; nlen + ndist];
                let mut i = 0;
                while i < nlen + ndist {
                    let sym = lencode.decode(&mut b)?;
                    let (v, n) = match sym {
                        0..=15 => (sym as u8, 1),
                        16 => {
                            if i == 0 {
                                return None;
                            }
                            (lengths[i - 1], 3 + b.bits(2)? as usize)
                        }
                        17 => (0, 3 + b.bits(3)? as usize),
                        _ => (0, 11 + b.bits(7)? as usize),
                    };
                    if i + n > nlen + ndist {
                        return None;
                    }
                    lengths[i..i + n].fill(v);
                    i += n;
                }
                if lengths[256] == 0 {
                    return None; // pas de fin de bloc
                }
                let lit = Huff::new(&lengths[..nlen])?;
                let dist = Huff::new(&lengths[nlen..])?;
                codes(&mut b, &mut out, &lit, &dist, max)?;
            }
            _ => return None,
        }
        if last == 1 {
            return Some(out);
        }
    }
}

/// Un flux zlib (RFC 1950) : l'en-tete de deux octets, puis DEFLATE.
pub fn zlib(data: &[u8], max: usize) -> Option<Vec<u8>> {
    let (&cmf, &flg) = (data.first()?, data.get(1)?);
    if cmf & 0x0F != 8 || ((cmf as u16) << 8 | flg as u16) % 31 != 0 || flg & 0x20 != 0 {
        return None;
    }
    inflate(&data[2..], max)
}
