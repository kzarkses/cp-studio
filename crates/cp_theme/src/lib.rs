//! Le theme de REAPER, lu tel quel : pour que ce que CP Studio dessine ait la
//! finesse et les couleurs des icones natives de Cedric, on prend LES MEMES
//! images, a leur taille d'origine (jamais reduites : une reduction floute).
//!
//! - [`ThemeFiles`] : un theme (`.ReaperThemeZip`, ou `.ReaperTheme` et son
//!   dossier d'images), ses images par nom et par echelle (`150/`, `200/`),
//!   ses couleurs.
//! - [`Frames`] : une image d'etats cote a cote (normale, survol, appui ou
//!   allumee), la convention des boutons de REAPER.
//! - [`Nine`] : une image a bord rose (la convention de REAPER pour ce qui
//!   s'etire), rendue a n'importe quelle taille.
//! - [`png`], [`zip`], [`inflate`] : de quoi lire tout cela sans Windows.

pub mod inflate;
pub mod png;
pub mod zip;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cp_gfx::Surface;

use zip::Zip;

/// D'ou viennent les fichiers d'un theme.
enum Source {
    /// Un dossier d'images (theme decompresse, ou `Data/toolbar_icons`).
    Dir(PathBuf),
    /// Un zip, et le dossier des images dedans (« Reapertips_v1.91_Green/ »).
    Zip(Box<Zip>, String),
}

/// Un theme de REAPER ouvert.
pub struct ThemeFiles {
    src: Source,
    /// Les cles de la section `[color theme]` du fichier `.ReaperTheme`
    /// (couleurs en entier natif, et polices en hexadecimal).
    pub keys: HashMap<String, String>,
    /// Le chemin d'origine (pour savoir si le theme a change).
    pub path: PathBuf,
}

/// Lit un fichier ini (sections ignorees : les cles d'un theme sont uniques).
fn ini_keys(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('[') || l.starts_with(';') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect()
}

/// Les dossiers a essayer pour une echelle d'ecran, du meilleur au pire, avec
/// l'echelle des images qu'on y trouve.
fn scale_dirs(scale: f32) -> [(&'static str, f32); 3] {
    if scale >= 1.75 {
        [("200/", 2.0), ("150/", 1.5), ("", 1.0)]
    } else if scale >= 1.25 {
        [("150/", 1.5), ("200/", 2.0), ("", 1.0)]
    } else {
        [("", 1.0), ("150/", 1.5), ("200/", 2.0)]
    }
}

impl ThemeFiles {
    /// Un dossier d'images (comme `Data/toolbar_icons`), sans couleurs.
    pub fn dir(path: &Path) -> ThemeFiles {
        ThemeFiles {
            src: Source::Dir(path.to_path_buf()),
            keys: HashMap::new(),
            path: path.to_path_buf(),
        }
    }

    /// Ouvre le theme que REAPER indique (`GetLastColorThemeFile`) : un
    /// `.ReaperTheme` (et son dossier d'images, ou le zip du meme nom quand
    /// le fichier n'existe pas a part), ou un `.ReaperThemeZip`.
    pub fn open(path: &Path) -> Option<ThemeFiles> {
        let s = path.to_string_lossy();
        let zip_path = if s.to_ascii_lowercase().ends_with(".reaperthemezip") {
            Some(path.to_path_buf())
        } else if path.is_file() {
            None
        } else {
            Some(PathBuf::from(format!("{s}Zip")))
        };
        if let Some(zp) = zip_path {
            let z = Zip::parse(std::fs::read(&zp).ok()?)?;
            // Le .ReaperTheme est a la racine du zip.
            let ini = z
                .entries
                .iter()
                .find(|e| {
                    !e.name.contains('/') && e.name.to_ascii_lowercase().ends_with(".reapertheme")
                })
                .cloned()
                .and_then(|e| z.read_entry(&e))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let keys = ini_keys(&ini);
            let mut prefix = keys
                .get("ui_img")
                .map(|d| d.replace('\\', "/"))
                .unwrap_or_default();
            if !prefix.is_empty() && !prefix.ends_with('/') {
                prefix.push('/');
            }
            return Some(ThemeFiles {
                src: Source::Zip(Box::new(z), prefix),
                keys,
                path: path.to_path_buf(),
            });
        }
        let text = std::fs::read(path).ok()?;
        let keys = ini_keys(&String::from_utf8_lossy(&text));
        let base = path.parent()?.to_path_buf();
        let dir = match keys.get("ui_img") {
            Some(d) if !d.is_empty() => base.join(d),
            _ => base,
        };
        Some(ThemeFiles {
            src: Source::Dir(dir),
            keys,
            path: path.to_path_buf(),
        })
    }

    /// Les octets d'un fichier du dossier d'images (`rel` : « 200/x.png »).
    pub fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match &self.src {
            Source::Dir(d) => std::fs::read(d.join(rel)).ok(),
            Source::Zip(z, prefix) => z.read(&format!("{prefix}{rel}")),
        }
    }

    /// L'image `name` (« transport_play », sans « .png ») la plus juste pour
    /// une echelle d'ecran : (image, echelle de l'image).
    pub fn image(&self, name: &str, scale: f32) -> Option<(Surface, f32)> {
        let file = if name.to_ascii_lowercase().ends_with(".png") {
            name.to_string()
        } else {
            format!("{name}.png")
        };
        scale_dirs(scale).into_iter().find_map(|(dir, s)| {
            let b = self.read(&format!("{dir}{file}"))?;
            Some((png::decode(&b)?, s))
        })
    }

    /// Une couleur du fichier de theme (BGR natif de Windows), en (r, g, b).
    pub fn color(&self, key: &str) -> Option<(u8, u8, u8)> {
        let v: i64 = self.keys.get(&key.to_ascii_lowercase())?.parse().ok()?;
        if v < 0 {
            return None;
        }
        Some((v as u8, (v >> 8) as u8, (v >> 16) as u8))
    }

    /// Le nom de la police d'une cle de theme (`trans_font`...) : un LOGFONT
    /// ecrit en hexadecimal ; le nom commence a l'octet 28.
    pub fn font_face(&self, key: &str) -> Option<String> {
        let hex = self.keys.get(&key.to_ascii_lowercase())?;
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
            .collect();
        let name: Vec<u8> = bytes
            .get(28..)?
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        let s = String::from_utf8_lossy(&name).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// Reduit ou agrandit une image par moyenne de surface (pixels premultiplies :
/// la moyenne est juste). Ne sert que quand le theme n'a pas l'image a
/// l'echelle de l'ecran.
pub fn resample(src: &Surface, dw: i32, dh: i32) -> Surface {
    let (dw, dh) = (dw.max(1), dh.max(1));
    let (sw, sh) = (src.w, src.h);
    let mut out = Surface::new(dw, dh);
    if sw <= 0 || sh <= 0 {
        return out;
    }
    for y in 0..dh {
        let y0 = y as f32 / dh as f32 * sh as f32;
        let y1 = (y + 1) as f32 / dh as f32 * sh as f32;
        for x in 0..dw {
            let x0 = x as f32 / dw as f32 * sw as f32;
            let x1 = (x + 1) as f32 / dw as f32 * sw as f32;
            let mut acc = [0f32; 4];
            let mut wsum = 0f32;
            let mut yy = y0.floor() as i32;
            while (yy as f32) < y1 && yy < sh {
                let wy = ((yy + 1) as f32).min(y1) - (yy as f32).max(y0);
                let mut xx = x0.floor() as i32;
                while (xx as f32) < x1 && xx < sw {
                    let wx = ((xx + 1) as f32).min(x1) - (xx as f32).max(x0);
                    let wgt = wx * wy;
                    let p = src.get(xx, yy);
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += ((p >> (c * 8)) & 0xFF) as f32 * wgt;
                    }
                    wsum += wgt;
                    xx += 1;
                }
                yy += 1;
            }
            let mut px = 0u32;
            if wsum > 0.0 {
                for (c, a) in acc.iter().enumerate() {
                    px |= (((a / wsum).round() as u32).min(255)) << (c * 8);
                }
            }
            out.px[(y * dw + x) as usize] = px;
        }
    }
    out
}

/// Une partie d'image.
pub fn crop(src: &Surface, x: i32, y: i32, w: i32, h: i32) -> Surface {
    let mut out = Surface::new(w, h);
    for yy in 0..h.max(0) {
        for xx in 0..w.max(0) {
            let (sx, sy) = (x + xx, y + yy);
            if sx >= 0 && sy >= 0 && sx < src.w && sy < src.h {
                out.px[(yy * w + xx) as usize] = src.get(sx, sy);
            }
        }
    }
    out
}

/// Les etats d'un bouton de REAPER, cote a cote dans une image : 0 normal,
/// 1 survol, 2 appui (ou allume, pour une icone de barre d'outils).
#[derive(Clone, Debug, Default)]
pub struct Frames {
    pub f: Vec<Surface>,
}

impl Frames {
    /// `n` vignettes de meme largeur.
    pub fn split(img: &Surface, n: i32) -> Option<Frames> {
        if n <= 0 || img.w < n || img.h <= 0 {
            return None;
        }
        let fw = img.w / n;
        Some(Frames {
            f: (0..n).map(|i| crop(img, i * fw, 0, fw, img.h)).collect(),
        })
    }

    /// Une icone de barre d'outils : trois etats si l'image est trois fois
    /// plus large que haute (la convention), sinon une seule image.
    pub fn toolbar(img: &Surface) -> Frames {
        if img.h > 0 && img.w >= 3 * img.h - 2 && img.w <= 3 * img.h + 2 {
            if let Some(f) = Frames::split(img, 3) {
                return f;
            }
        }
        Frames {
            f: vec![img.clone()],
        }
    }

    /// Chaque vignette ramenee de l'echelle `from` a l'echelle `to`.
    pub fn rescaled(self, from: f32, to: f32) -> Frames {
        if (from - to).abs() < 0.01 || from <= 0.0 {
            return self;
        }
        let k = to / from;
        Frames {
            f: self
                .f
                .iter()
                .map(|s| {
                    resample(
                        s,
                        (s.w as f32 * k).round() as i32,
                        (s.h as f32 * k).round() as i32,
                    )
                })
                .collect(),
        }
    }

    pub fn w(&self) -> i32 {
        self.f.first().map_or(0, |s| s.w)
    }
    pub fn h(&self) -> i32 {
        self.f.first().map_or(0, |s| s.h)
    }

    /// La vignette `i`, ou la derniere qui existe.
    pub fn get(&self, i: usize) -> Option<&Surface> {
        self.f.get(i).or(self.f.last())
    }
}

/// Le rose de REAPER (premultiplie, opaque).
const PINK: u32 = 0xFFFF_00FF;

/// Une image qui s'etire : ses quatre bords gardent leur taille, le milieu
/// s'etend. La convention de REAPER (WDL) : un cadre d'un pixel rose dont les
/// traits, depuis le coin haut-gauche et le coin bas-droit, donnent les bords.
#[derive(Clone, Debug, Default)]
pub struct Nine {
    pub img: Surface,
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl Nine {
    /// Lit le cadre rose s'il y en a un ; sinon l'image entiere, avec des
    /// bords de `m` pixels.
    pub fn from_image(img: &Surface, m: i32) -> Nine {
        let (w, h) = (img.w, img.h);
        if w > 2 && h > 2 && img.get(0, 0) == PINK && img.get(w - 1, h - 1) == PINK {
            let mut x = 1;
            while x < w && img.get(x, 0) == PINK {
                x += 1;
            }
            let lt0 = x;
            let mut x = w - 2;
            while x > lt0 && img.get(x, h - 1) == PINK {
                x -= 1;
            }
            let rb0 = w - 1 - x;
            let mut y = 1;
            while y < h && img.get(0, y) == PINK {
                y += 1;
            }
            let lt1 = y;
            let mut y = h - 2;
            while y > lt1 && img.get(w - 1, y) == PINK {
                y -= 1;
            }
            let rb1 = h - 1 - y;
            return Nine {
                img: crop(img, 1, 1, w - 2, h - 2),
                l: (lt0 - 1).max(0),
                t: (lt1 - 1).max(0),
                r: (rb0 - 1).max(0),
                b: (rb1 - 1).max(0),
            };
        }
        let m = m.max(0).min(w / 2).min(h / 2);
        Nine {
            img: img.clone(),
            l: m,
            t: m,
            r: m,
            b: m,
        }
    }

    /// Les bords ramenes a l'echelle `to` depuis `from` (l'image aussi).
    pub fn rescaled(self, from: f32, to: f32) -> Nine {
        if (from - to).abs() < 0.01 || from <= 0.0 {
            return self;
        }
        let k = to / from;
        let s = |v: i32| (v as f32 * k).round() as i32;
        Nine {
            img: resample(&self.img, s(self.img.w), s(self.img.h)),
            l: s(self.l),
            t: s(self.t),
            r: s(self.r),
            b: s(self.b),
        }
    }

    /// L'image a la taille `w x h` : coins intacts, bords et milieu etires
    /// (au plus proche : un bord de theme est uni dans le sens de l'etirement).
    pub fn render(&self, w: i32, h: i32) -> Surface {
        let mut out = Surface::new(w, h);
        let (sw, sh) = (self.img.w, self.img.h);
        if w <= 0 || h <= 0 || sw <= 0 || sh <= 0 {
            return out;
        }
        let map = |d: i32, dn: i32, a: i32, b: i32, sn: i32| -> i32 {
            // Si la cible est plus petite que les deux bords : on les reduit.
            let (a, b) = if a + b > dn && a + b > 0 {
                let k = dn as f32 / (a + b) as f32;
                ((a as f32 * k) as i32, dn - (a as f32 * k) as i32)
            } else {
                (a, b)
            };
            if d < a {
                d
            } else if d >= dn - b {
                sn - (dn - d)
            } else {
                let mid_s = (sn - a - b).max(1);
                let mid_d = (dn - a - b).max(1);
                a + ((d - a) * mid_s / mid_d).min(mid_s - 1)
            }
        };
        for y in 0..h {
            let sy = map(y, h, self.t, self.b, sh).clamp(0, sh - 1);
            for x in 0..w {
                let sx = map(x, w, self.l, self.r, sw).clamp(0, sw - 1);
                out.px[(y * w + x) as usize] = self.img.get(sx, sy);
            }
        }
        out
    }
}
