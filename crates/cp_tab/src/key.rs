//! La tonalite d'une tablature et l'orthographe de ses notes : l'armure
//! (combien de dieses ou de bemols), le nom de chaque note dans cette
//! tonalite (Fa diese en Sol majeur, Si bemol en Fa), et les alterations a
//! ecrire dans une mesure (une alteration vaut jusqu'a la barre).
//!
//! La tonalite se devine par les profils de Krumhansl-Kessler : la duree
//! jouee de chaque classe de hauteur, comparee aux 24 tonalites.

use crate::doc::TNote;

/// Les profils de Krumhansl-Kessler, de la tonique a la septieme majeure.
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// L'armure d'une tonalite majeure par sa tonique (Do = 0) : dieses > 0,
/// bemols < 0 (Fa diese plutot que Sol bemol : la guitare joue en dieses).
const FIFTHS: [i32; 12] = [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];

/// Le Do de chaque lettre (Do, Re, Mi, Fa, Sol, La, Si).
const NAT: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

/// Les noms des lettres, en anglais (comme le MusicXML).
pub const STEPS: [&str; 7] = ["C", "D", "E", "F", "G", "A", "B"];

/// Une tonalite : son armure et son mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Key {
    /// Dieses (> 0) ou bemols (< 0), de -6 a 6.
    pub fifths: i32,
    pub minor: bool,
}

impl Key {
    /// « E minor », « Bb major ».
    pub fn name(&self) -> String {
        let major_pc = (FIFTHS.iter().position(|&f| f == self.fifths).unwrap_or(0)) as i32;
        let pc = if self.minor {
            (major_pc + 9).rem_euclid(12)
        } else {
            major_pc
        };
        let s = spell(pc + 60, self.fifths);
        let acc = match s.alter {
            1 => "#",
            -1 => "b",
            _ => "",
        };
        format!(
            "{}{acc} {}",
            STEPS[s.step as usize],
            if self.minor { "minor" } else { "major" }
        )
    }
}

/// La tonalite la plus probable des notes (Do majeur s'il y en a trop peu).
pub fn detect(notes: &[TNote]) -> Key {
    detect_iter(notes)
}

/// [`detect`] sur des notes prises n'importe ou (les prises d'une guitare).
pub fn detect_iter<'a>(notes: impl IntoIterator<Item = &'a TNote>) -> Key {
    let mut h = [0f64; 12];
    let mut count = 0;
    for n in notes {
        h[n.pitch.rem_euclid(12) as usize] += (n.qn1 - n.qn0).clamp(0.1, 4.0);
        count += 1;
    }
    let distinct = h.iter().filter(|&&v| v > 0.0).count();
    if count < 6 || distinct < 4 {
        return Key::default();
    }
    let corr = |prof: &[f64; 12], tonic: usize| -> f64 {
        let mx = h.iter().sum::<f64>() / 12.0;
        let mp = prof.iter().sum::<f64>() / 12.0;
        let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
        for k in 0..12 {
            let x = h[(tonic + k) % 12] - mx;
            let y = prof[k] - mp;
            a += x * y;
            b += x * x;
            c += y * y;
        }
        a / (b * c).sqrt().max(1e-12)
    };
    let mut best = (f64::MIN, Key::default());
    for tonic in 0..12 {
        let maj = corr(&MAJOR, tonic);
        if maj > best.0 {
            best = (
                maj,
                Key {
                    fifths: FIFTHS[tonic],
                    minor: false,
                },
            );
        }
        let min = corr(&MINOR, tonic);
        if min > best.0 {
            best = (
                min,
                Key {
                    fifths: FIFTHS[(tonic + 3) % 12],
                    minor: true,
                },
            );
        }
    }
    best.1
}

/// L'alteration de chaque lettre (Do..Si) dans l'armure.
pub fn key_alters(fifths: i32) -> [i32; 7] {
    // Fa Do Sol Re La Mi Si, et l'inverse pour les bemols.
    const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
    const FLATS: [usize; 7] = [6, 2, 5, 1, 4, 0, 3];
    let mut a = [0; 7];
    if fifths > 0 {
        for &l in SHARPS.iter().take(fifths.min(7) as usize) {
            a[l] = 1;
        }
    } else {
        for &l in FLATS.iter().take((-fifths).min(7) as usize) {
            a[l] = -1;
        }
    }
    a
}

/// Les lettres alterees de l'armure, dans l'ordre ou elles s'ecrivent.
pub fn signature(fifths: i32) -> Vec<usize> {
    const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
    const FLATS: [usize; 7] = [6, 2, 5, 1, 4, 0, 3];
    if fifths > 0 {
        SHARPS[..fifths.min(7) as usize].to_vec()
    } else {
        FLATS[..(-fifths).min(7) as usize].to_vec()
    }
}

/// Une note ecrite : sa lettre (0 = Do .. 6 = Si), son alteration (-1, 0,
/// 1) et son octave (Do 4 = 60).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spelled {
    pub step: i32,
    pub alter: i32,
    pub octave: i32,
}

impl Spelled {
    /// La place sur la portee en lignes et interlignes (Mi 4 = 0, la ligne
    /// du bas en cle de sol).
    pub fn staff_pos(&self) -> i32 {
        self.octave * 7 + self.step - (4 * 7 + 2)
    }
}

/// Le nom d'une hauteur dans la tonalite : celui de l'armure s'il y en a un,
/// sinon la note naturelle, sinon un diese (armure en dieses ou sans) ou un
/// bemol (armure en bemols).
pub fn spell(pitch: i32, fifths: i32) -> Spelled {
    let pc = pitch.rem_euclid(12);
    let ka = key_alters(fifths);
    let mut cands: Vec<(i32, i32)> = Vec::new();
    for step in 0..7 {
        for alter in -1..=1 {
            if (NAT[step as usize] + alter).rem_euclid(12) == pc {
                cands.push((step, alter));
            }
        }
    }
    let pref = if fifths >= 0 { 1 } else { -1 };
    let (step, alter) = cands
        .iter()
        .find(|c| ka[c.0 as usize] == c.1)
        .or_else(|| cands.iter().find(|c| c.1 == 0))
        .or_else(|| cands.iter().find(|c| c.1 == pref))
        .copied()
        .unwrap_or((0, 0));
    let octave = (pitch - NAT[step as usize] - alter).div_euclid(12) - 1;
    Spelled {
        step,
        alter,
        octave,
    }
}

/// Les alterations a ecrire dans une mesure : celle d'une note qui change
/// la lettre a cette octave par rapport a l'armure ou a ce qui precede dans
/// la mesure (rien sinon). Remis a l'armure a chaque barre.
#[derive(Clone, Debug)]
pub struct Accidentals {
    key: [i32; 7],
    now: std::collections::HashMap<(i32, i32), i32>,
}

impl Accidentals {
    pub fn new(fifths: i32) -> Accidentals {
        Accidentals {
            key: key_alters(fifths),
            now: std::collections::HashMap::new(),
        }
    }

    /// Une nouvelle mesure.
    pub fn bar(&mut self) {
        self.now.clear();
    }

    /// L'alteration a ecrire devant `s` (Some(-1, 0 ou 1) : bemol, becarre,
    /// diese), et la note compte pour la suite de la mesure.
    pub fn take(&mut self, s: Spelled) -> Option<i32> {
        let k = (s.step, s.octave);
        let cur = self
            .now
            .get(&k)
            .copied()
            .unwrap_or(self.key[s.step as usize]);
        self.now.insert(k, s.alter);
        (cur != s.alter).then_some(s.alter)
    }

    /// Une note liee depuis la mesure d'avant : rien a ecrire, mais elle
    /// compte.
    pub fn hold(&mut self, s: Spelled) {
        self.now.insert((s.step, s.octave), s.alter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(p: i32, q: f64, len: f64) -> TNote {
        TNote {
            qn0: q,
            qn1: q + len,
            pitch: p,
            vel: 90,
            ..Default::default()
        }
    }

    /// Une gamme et un arpege de la tonalite, la tonique appuyee.
    fn tune(tonic: i32, minor: bool) -> Vec<TNote> {
        let scale: [i32; 7] = if minor {
            [0, 2, 3, 5, 7, 8, 10]
        } else {
            [0, 2, 4, 5, 7, 9, 11]
        };
        let mut v = Vec::new();
        let mut q = 0.0;
        for &d in &scale {
            v.push(n(52 + tonic + d, q, 0.5));
            q += 0.5;
        }
        for &d in &[0, if minor { 3 } else { 4 }, 7, 12, 7, 0] {
            v.push(n(52 + tonic + d, q, 1.0));
            q += 1.0;
        }
        v
    }

    #[test]
    fn la_tonalite_se_devine() {
        // Mi mineur (une armure d'un diese), La majeur (trois), Fa majeur
        // (un bemol), Re mineur (un bemol), Si bemol majeur (deux).
        assert_eq!(
            detect(&tune(0, true)),
            Key {
                fifths: 1,
                minor: true
            }
        );
        assert_eq!(
            detect(&tune(5, false)),
            Key {
                fifths: 3,
                minor: false
            }
        );
        assert_eq!(
            detect(&tune(1, false)),
            Key {
                fifths: -1,
                minor: false
            }
        );
        assert_eq!(
            detect(&tune(10, true)),
            Key {
                fifths: -1,
                minor: true
            }
        );
        assert_eq!(
            detect(&tune(6, false)),
            Key {
                fifths: -2,
                minor: false
            }
        );
        assert_eq!(
            Key {
                fifths: 1,
                minor: true
            }
            .name(),
            "E minor"
        );
        assert_eq!(
            Key {
                fifths: -2,
                minor: false
            }
            .name(),
            "Bb major"
        );
        assert_eq!(
            Key {
                fifths: 6,
                minor: false
            }
            .name(),
            "F# major"
        );
        // Trop peu pour juger : Do.
        assert_eq!(detect(&tune(0, true)[..3]), Key::default());
    }

    #[test]
    fn les_notes_s_ecrivent_dans_la_tonalite() {
        let sp = |p: i32, f: i32| {
            let s = spell(p, f);
            (STEPS[s.step as usize], s.alter, s.octave)
        };
        // Fa diese en Sol, Sol bemol... non : Fa diese aussi en Do (diese par
        // defaut), Si bemol en Fa, La diese en Mi (hors gamme, armure en
        // dieses), Mi diese en Fa diese majeur.
        assert_eq!(sp(66, 1), ("F", 1, 4));
        assert_eq!(sp(66, 0), ("F", 1, 4));
        assert_eq!(sp(70, -1), ("B", -1, 4));
        assert_eq!(sp(70, 4), ("A", 1, 4));
        assert_eq!(sp(65, 6), ("E", 1, 4));
        assert_eq!(sp(63, -3), ("E", -1, 4));
        // Do 4 = 60 : la ligne supplementaire sous la cle de sol.
        assert_eq!(spell(60, 0).staff_pos(), -2);
        assert_eq!(spell(64, 0).staff_pos(), 0);
    }

    #[test]
    fn une_alteration_vaut_jusqu_a_la_barre() {
        // En Sol (Fa diese a l'armure) : Fa diese rien, Fa naturel becarre,
        // Fa naturel encore rien, Fa diese le diese ; a la barre, Fa diese
        // redevient muet.
        let mut a = Accidentals::new(1);
        let fs = spell(66, 1);
        let f = Spelled { alter: 0, ..fs };
        assert_eq!(a.take(fs), None);
        assert_eq!(a.take(f), Some(0));
        assert_eq!(a.take(f), None);
        assert_eq!(a.take(fs), Some(1));
        a.bar();
        assert_eq!(a.take(fs), None);
        // Une autre octave a sa propre alteration.
        let mut a = Accidentals::new(0);
        assert_eq!(a.take(spell(61, 0)), Some(1));
        assert_eq!(a.take(spell(73, 0)), Some(1));
        assert_eq!(a.take(spell(61, 0)), None);
    }
}
