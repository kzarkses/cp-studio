//! Le nom d'un accord joue : sa fondamentale, son type (« m7 », « sus4 »,
//! « 5 » pour un power chord) et sa basse quand ce n'est pas la
//! fondamentale (« C/E »). Les noms se nomment dans la tonalite (Bb en Fa
//! majeur, A# en Si majeur).

use crate::key::{STEPS, spell};

/// Les types reconnus : (intervalles depuis la fondamentale, suffixe ecrit,
/// `kind` du MusicXML). Du plus courant au plus rare : a egalite, le
/// premier gagne.
const KINDS: [(&[i32], &str, &str); 16] = [
    (&[0, 4, 7], "", "major"),
    (&[0, 3, 7], "m", "minor"),
    (&[0, 7], "5", "power"),
    (&[0, 4, 7, 10], "7", "dominant"),
    (&[0, 3, 7, 10], "m7", "minor-seventh"),
    (&[0, 4, 7, 11], "maj7", "major-seventh"),
    (&[0, 2, 7], "sus2", "suspended-second"),
    (&[0, 5, 7], "sus4", "suspended-fourth"),
    (&[0, 5, 7, 10], "7sus4", "dominant"),
    (&[0, 4, 7, 9], "6", "major-sixth"),
    (&[0, 3, 7, 9], "m6", "minor-sixth"),
    (&[0, 4, 7, 2], "add9", "major"),
    (&[0, 4, 7, 10, 2], "9", "dominant-ninth"),
    (&[0, 3, 7, 10, 2], "m9", "minor-ninth"),
    (&[0, 3, 6], "dim", "diminished"),
    (&[0, 4, 8], "aug", "augmented"),
];

/// Un accord nomme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    /// Classes de hauteur (Do = 0).
    pub root: i32,
    pub bass: i32,
    /// Rang dans les types.
    kind: usize,
}

impl Chord {
    /// « Am7 », « G/B », « E5 », dans la tonalite `fifths`.
    pub fn text(&self, fifths: i32) -> String {
        let mut s = format!("{}{}", pc_name(self.root, fifths), KINDS[self.kind].1);
        if self.bass != self.root {
            s += "/";
            s += &pc_name(self.bass, fifths);
        }
        s
    }

    /// Le `kind` du MusicXML, et le suffixe ecrit.
    pub fn kind(&self) -> (&'static str, &'static str) {
        (KINDS[self.kind].2, KINDS[self.kind].1)
    }
}

/// « C », « F# », « Bb ».
pub fn pc_name(pc: i32, fifths: i32) -> String {
    let s = spell(pc.rem_euclid(12) + 60, fifths);
    let acc = match s.alter {
        1 => "#",
        -1 => "b",
        _ => "",
    };
    format!("{}{acc}", STEPS[s.step as usize])
}

/// Le nom des hauteurs qui sonnent ensemble, ou rien : une note seule, deux
/// notes qui ne font pas une quinte, ou un ensemble qu'aucun type
/// n'explique sans trop de notes etrangeres.
pub fn name(pitches: &[i32]) -> Option<Chord> {
    let bass_p = *pitches.iter().min()?;
    let mut pcs: Vec<i32> = pitches.iter().map(|p| p.rem_euclid(12)).collect();
    pcs.sort_unstable();
    pcs.dedup();
    if pcs.len() < 2 {
        return None;
    }
    let bass = bass_p.rem_euclid(12);
    let mut best: Option<(i32, Chord)> = None;
    for &root in &pcs {
        let rel: Vec<i32> = pcs.iter().map(|&p| (p - root).rem_euclid(12)).collect();
        for (k, (iv, _, _)) in KINDS.iter().enumerate() {
            // Toutes les notes du type sonnent (la quinte peut manquer d'un
            // accord de quatre sons ou plus), aucune etrangere.
            let missing: Vec<i32> = iv.iter().copied().filter(|i| !rel.contains(i)).collect();
            let extra = rel.iter().filter(|r| !iv.contains(r)).count();
            let ok_missing = missing.is_empty() || (iv.len() >= 4 && missing == [7]);
            if !ok_missing || extra > 0 {
                continue;
            }
            // Les deux notes d'un power chord ne nomment qu'une quinte (pas
            // une quarte : sa basse est la fondamentale).
            if iv.len() == 2 && root != bass {
                continue;
            }
            let score = iv.len() as i32 * 10 - k as i32 + if root == bass { 5 } else { 0 }
                - missing.len() as i32 * 8;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((
                    score,
                    Chord {
                        root,
                        bass,
                        kind: k,
                    },
                ));
            }
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(p: &[i32], f: i32) -> Option<String> {
        name(p).map(|c| c.text(f))
    }

    #[test]
    fn les_accords_se_nomment() {
        // Mi majeur ouvert, La mineur ouvert, Sol (basse Si), un power chord
        // de Mi, Re7, Cmaj7, Asus4, un Bb en Fa majeur.
        assert_eq!(t(&[40, 47, 52, 56, 59, 64], 1).as_deref(), Some("E"));
        assert_eq!(t(&[45, 52, 57, 60, 64], 0).as_deref(), Some("Am"));
        assert_eq!(t(&[47, 50, 55, 59, 67], 1).as_deref(), Some("G/B"));
        assert_eq!(t(&[40, 47, 52], 0).as_deref(), Some("E5"));
        assert_eq!(t(&[50, 57, 60, 66], 1).as_deref(), Some("D7"));
        assert_eq!(t(&[48, 52, 55, 59, 64], 0).as_deref(), Some("Cmaj7"));
        assert_eq!(t(&[45, 52, 57, 62, 64], 0).as_deref(), Some("Asus4"));
        assert_eq!(t(&[46, 53, 58, 62, 65], -1).as_deref(), Some("Bb"));
        assert_eq!(t(&[45, 52, 55, 60, 64], 0).as_deref(), Some("Am7"));
        // Rien : une note, une tierce seule, un amas.
        assert_eq!(t(&[52], 0), None);
        assert_eq!(t(&[52, 56], 0), None);
        assert_eq!(t(&[60, 61, 62, 63], 0), None);
    }
}
