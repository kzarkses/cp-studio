//! Ce que l'analyse garde d'une prise : les matrices des notes et des
//! attaques (en octets : 0..255 suffit pour des seuils au centieme), et
//! l'enveloppe du son, trame par trame. Le reseau ne tourne qu'une fois ; les
//! reglages de detection relisent ceci en quelques millisecondes, et les notes
//! fantomes en viennent. Le tout tient dans un fichier a cote du projet.
//!
//! Format « CPGA » v2, petit-boutiste : magique, version (u32), trames (u32),
//! puis note (trames x 88 octets), onset (idem), enveloppe (trames octets),
//! et la hauteur fine (contour, trames x 264 octets : les bends et les
//! glisses). La v1 n'avait pas le contour : elle se relit, sans glisses.

use crate::analyze::Posteriors;
use crate::model::{CONTOUR, HOP, NOTES};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub frames: usize,
    pub note: Vec<u8>,
    pub onset: Vec<u8>,
    /// La crete du son (mono, 22 050 Hz) autour de chaque trame, 0..255.
    pub env: Vec<u8>,
    /// La hauteur fine (trames x 264) ; vide pour une analyse d'avant le
    /// 2026-11-22 (ni bends ni glisses).
    pub contour: Vec<u8>,
}

fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Analysis {
    /// Depuis les matrices du reseau et le son qu'il a ecoute.
    pub fn new(p: &Posteriors, audio: &[f32]) -> Analysis {
        let mut env = vec![0u8; p.frames];
        for (f, e) in env.iter_mut().enumerate() {
            let a = (f * HOP).saturating_sub(HOP / 2);
            let b = (f * HOP + HOP / 2).min(audio.len());
            let peak = audio
                .get(a..b)
                .map_or(0.0, |s| s.iter().fold(0f32, |m, &x| m.max(x.abs())));
            *e = q(peak);
        }
        Analysis {
            frames: p.frames,
            note: p.note.iter().map(|&v| q(v)).collect(),
            onset: p.onset.iter().map(|&v| q(v)).collect(),
            env,
            contour: if p.contour.len() == p.frames * CONTOUR {
                p.contour.iter().map(|&v| q(v)).collect()
            } else {
                Vec::new()
            },
        }
    }

    /// La hauteur fine est la (les bends et les glisses se lisent).
    pub fn has_contour(&self) -> bool {
        self.frames > 0 && self.contour.len() == self.frames * CONTOUR
    }

    /// Le contour pour la lecture des glisses.
    pub fn glide_contour(&self) -> crate::glide::Contour<'_> {
        crate::glide::Contour {
            frames: if self.has_contour() { self.frames } else { 0 },
            v: &self.contour,
        }
    }

    /// `lead` trames de silence au debut (un item tire avant son fichier).
    pub fn pad_front(&self, lead: usize) -> Analysis {
        let mut note = vec![0u8; lead * NOTES];
        note.extend_from_slice(&self.note);
        let mut onset = vec![0u8; lead * NOTES];
        onset.extend_from_slice(&self.onset);
        let mut env = vec![0u8; lead];
        env.extend_from_slice(&self.env);
        let contour = if self.has_contour() {
            let mut c = vec![0u8; lead * CONTOUR];
            c.extend_from_slice(&self.contour);
            c
        } else {
            Vec::new()
        };
        Analysis {
            frames: lead + self.frames,
            note,
            onset,
            env,
            contour,
        }
    }

    /// Les trames `f0 .. f0 + n` (bornees au fichier) : l'analyse d'un item
    /// tiree de celle de tout son fichier. Couper, rogner ou deplacer un item
    /// ne relance donc jamais le reseau.
    pub fn slice(&self, f0: usize, n: usize) -> Analysis {
        let a = f0.min(self.frames);
        let b = (a + n).min(self.frames);
        Analysis {
            frames: b - a,
            note: self.note[a * NOTES..b * NOTES].to_vec(),
            onset: self.onset[a * NOTES..b * NOTES].to_vec(),
            env: self.env[a..b].to_vec(),
            contour: if self.has_contour() {
                self.contour[a * CONTOUR..b * CONTOUR].to_vec()
            } else {
                Vec::new()
            },
        }
    }

    /// Les matrices pour le decodage (sans contour : pas de bends).
    pub fn posteriors(&self) -> Posteriors {
        Posteriors {
            frames: self.frames,
            note: self.note.iter().map(|&v| v as f32 / 255.0).collect(),
            onset: self.onset.iter().map(|&v| v as f32 / 255.0).collect(),
            contour: Vec::new(),
        }
    }

    /// L'attaque la plus forte de chaque trame (toutes hauteurs), 0..1 : la
    /// courbe « ONSETS » de la fenetre.
    pub fn onset_curve(&self) -> Vec<f32> {
        (0..self.frames)
            .map(|f| {
                self.onset[f * NOTES..(f + 1) * NOTES]
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(0) as f32
                    / 255.0
            })
            .collect()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v =
            Vec::with_capacity(12 + self.note.len() * 2 + self.env.len() + self.contour.len());
        v.extend_from_slice(b"CPGA");
        let ver: u32 = if self.has_contour() { 2 } else { 1 };
        v.extend_from_slice(&ver.to_le_bytes());
        v.extend_from_slice(&(self.frames as u32).to_le_bytes());
        v.extend_from_slice(&self.note);
        v.extend_from_slice(&self.onset);
        v.extend_from_slice(&self.env);
        if ver == 2 {
            v.extend_from_slice(&self.contour);
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Analysis> {
        let ver = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
        if b.get(..4)? != b"CPGA" || !(1..=2).contains(&ver) {
            return None;
        }
        let frames = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?) as usize;
        let n = frames.checked_mul(NOTES)?;
        let mut at = 12;
        let note = b.get(at..at + n)?.to_vec();
        at += n;
        let onset = b.get(at..at + n)?.to_vec();
        at += n;
        let env = b.get(at..at + frames)?.to_vec();
        at += frames;
        let contour = if ver == 2 {
            b.get(at..at + frames.checked_mul(CONTOUR)?)?.to_vec()
        } else {
            Vec::new()
        };
        Some(Analysis {
            frames,
            note,
            onset,
            env,
            contour,
        })
    }
}
