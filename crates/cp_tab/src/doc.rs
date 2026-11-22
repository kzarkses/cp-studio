//! La tablature editable : une liste de notes, chacune avec sa corde, sa case
//! et sa liaison. C'est elle que l'item MIDI garde dans le projet (une corde
//! par canal MIDI), elle que la fenetre de Guitar Tab dessine et modifie, et
//! elle que la partition ecrit.
//!
//! - [`finger`] : des notes detectees a la tablature (le doigte sur toute la
//!   phrase, les marteaux et les tires) ;
//! - [`ghosts`] : les notes que le reseau a PRESQUE entendues (sous le seuil),
//!   que Cedric garde d'un clic ;
//! - [`to_midi`] / [`from_midi`] : l'aller-retour avec l'item MIDI, et
//!   [`Sidecar`] : ce que le MIDI ne sait pas porter (liaisons, notes
//!   touchees par Cedric, notes qu'il a effacees).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::fret::{self, Tuning};
use crate::score::TabNote;

/// La liaison d'une note avec la precedente, sur la meme corde.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Tech {
    #[default]
    Plain,
    /// Marteau (« H ») : plus haute, sans attaque.
    Hammer,
    /// Tire (« P ») : plus basse, sans attaque.
    Pull,
}

/// Une note de la tablature.
#[derive(Clone, Debug, PartialEq)]
pub struct TNote {
    /// Debut et fin, en noires du projet (le temps reel joue, pas la grille).
    pub qn0: f64,
    pub qn1: f64,
    /// MIDI, la hauteur jouee.
    pub pitch: i32,
    /// La corde : 0 = la plus grave (rang dans l'accordage).
    pub string: usize,
    pub fret: i32,
    pub vel: u8,
    pub tech: Tech,
    /// Touchee par Cedric (deplacee, changee, ajoutee, gardee) : un nouveau
    /// decodage la respecte.
    pub locked: bool,
}

impl TNote {
    /// La cle d'une note pour la retrouver d'une lecture a l'autre : son debut
    /// au 1/960 de noire et sa hauteur.
    pub fn key(&self) -> (i64, i32) {
        ((self.qn0 * 960.0).round() as i64, self.pitch)
    }
}

/// Trie par debut, puis hauteur.
pub fn sort(notes: &mut [TNote]) {
    notes.sort_by(|a, b| {
        a.qn0
            .partial_cmp(&b.qn0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.pitch.cmp(&b.pitch))
    });
}

/// Une note d'accord : (hauteur repliee, debut, fin, sans attaque, velocite).
type Member = (i32, f64, f64, bool, u8);

/// Des notes detectees a la tablature. Les notes qui commencent sur la meme
/// case de la grille (`grid` par noire) forment un accord ; le doigte se
/// choisit sur toute la phrase ([`fret::assign`]) ; une note sans attaque
/// collee a la precedente sur la meme corde est un marteau ou un tire. Rend
/// (tablature, notes injouables laissees de cote).
pub fn finger(notes: &[TabNote], grid: u32, bpm: f64, tuning: &Tuning) -> (Vec<TNote>, usize) {
    let g = grid.max(1) as f64;
    // Les accords : (hauteur repliee, debut, fin, sans attaque, velocite).
    let mut groups: BTreeMap<i64, Vec<Member>> = BTreeMap::new();
    for n in notes {
        let s = (n.qn0 * g).round() as i64;
        let p = fret::fold(n.pitch, tuning);
        let gr = groups.entry(s).or_default();
        if let Some(x) = gr.iter_mut().find(|x| x.0 == p) {
            x.1 = x.1.min(n.qn0);
            x.2 = x.2.max(n.qn1);
            x.3 &= n.legato;
            x.4 = x.4.max(n.vel);
        } else {
            gr.push((p, n.qn0, n.qn1, n.legato, n.vel));
        }
    }
    let starts: Vec<i64> = groups.keys().copied().collect();
    let pitches: Vec<Vec<i32>> = groups
        .values()
        .map(|v| v.iter().map(|x| x.0).collect())
        .collect();
    let sec_per_cell = 60.0 / bpm.max(1.0) / g;
    let gaps: Vec<f64> = starts
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            if i == 0 {
                0.0
            } else {
                (s - starts[i - 1]) as f64 * sec_per_cell
            }
        })
        .collect();
    let spots = fret::assign(&pitches, &gaps, tuning);
    let mut out: Vec<TNote> = Vec::new();
    let mut dropped = 0;
    // Pour les liaisons : la note seule de l'accord precedent (indice, fin en
    // cases), et son debut.
    let mut prev_single: Option<(usize, i64)> = None;
    for (i, (&s, gr)) in groups.iter().enumerate() {
        let next = starts.get(i + 1).copied().unwrap_or(i64::MAX);
        let first = out.len();
        for (k, &(p, q0, q1, _, vel)) in gr.iter().enumerate() {
            match spots[i].get(k).copied().flatten() {
                Some(sp) => out.push(TNote {
                    qn0: q0,
                    qn1: q1.max(q0 + 1e-6),
                    pitch: p,
                    string: sp.string,
                    fret: sp.fret,
                    vel,
                    tech: Tech::Plain,
                    locked: false,
                }),
                None => dropped += 1,
            }
        }
        let placed = out.len() - first;
        if placed == 1 && gr.len() == 1 {
            let b = first;
            if let Some((a, a_end)) = prev_single {
                let legato = gr[0].3;
                if legato
                    && a_end == s
                    && out[a].string == out[b].string
                    && out[a].pitch != out[b].pitch
                {
                    out[b].tech = if out[b].pitch > out[a].pitch {
                        Tech::Hammer
                    } else {
                        Tech::Pull
                    };
                }
            }
            let e = ((gr[0].2 * g).round() as i64).min(next).max(s + 1);
            prev_single = Some((b, e));
        } else {
            prev_single = None;
        }
    }
    sort(&mut out);
    (out, dropped)
}

/// Les notes que le reseau a presque entendues : celles d'un decodage plus
/// sensible (`candidates`) qui ne sont pas deja dans la tablature (meme
/// hauteur, a une case de grille pres, ou une note qui sonne encore). Chacune
/// est placee pres de la note qui la precede.
pub fn ghosts(candidates: &[TabNote], doc: &[TNote], grid: u32, tuning: &Tuning) -> Vec<TNote> {
    let g = grid.max(1) as f64;
    let mut out: Vec<TNote> = Vec::new();
    for c in candidates {
        let p = fret::fold(c.pitch, tuning);
        let s = (c.qn0 * g).round();
        let known = doc.iter().chain(out.iter()).any(|d| {
            d.pitch == p
                && (((d.qn0 * g).round() - s).abs() <= 1.0
                    || (d.qn0 < c.qn0 && c.qn0 < d.qn1 - 0.5 / g))
        });
        if known {
            continue;
        }
        // La main est la ou etait la note d'avant.
        let reference = doc
            .iter()
            .filter(|d| d.qn0 <= c.qn0)
            .max_by(|a, b| {
                a.qn0
                    .partial_cmp(&b.qn0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(3, |d| d.fret.max(1));
        let best = tuning
            .strings
            .iter()
            .enumerate()
            .filter_map(|(si, &open)| {
                let f = p - open;
                (0..=tuning.frets).contains(&f).then_some((si, f))
            })
            .min_by_key(|&(_, f)| if f == 0 { 0 } else { (f - reference).abs() + 1 });
        let Some((string, fret)) = best else {
            continue;
        };
        out.push(TNote {
            qn0: c.qn0,
            qn1: c.qn1.max(c.qn0 + 1e-6),
            pitch: p,
            string,
            fret,
            vel: c.vel,
            tech: Tech::Plain,
            locked: false,
        });
    }
    sort(&mut out);
    out
}

/// Une note de l'item MIDI, en noires du projet.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiQn {
    pub qn0: f64,
    pub qn1: f64,
    pub pitch: i32,
    /// 0 = la corde la plus aigue (canal MIDI 1).
    pub chan: u8,
    pub vel: u8,
}

/// La tablature en notes MIDI : la corde dans le canal (canal 1 = la plus
/// aigue, la convention des micros guitare MIDI et de Guitar Pro).
pub fn to_midi(doc: &[TNote], tuning: &Tuning) -> Vec<MidiQn> {
    let n = tuning.strings.len();
    doc.iter()
        .map(|t| MidiQn {
            qn0: t.qn0,
            qn1: t.qn1,
            pitch: t.pitch,
            chan: (n.saturating_sub(1).saturating_sub(t.string)).min(15) as u8,
            vel: t.vel.max(1),
        })
        .collect()
}

/// Ce que le MIDI ne porte pas : les liaisons, les notes touchees par Cedric,
/// et celles qu'il a effacees (un nouveau decodage ne les ramene pas).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sidecar {
    pub tech: HashMap<(i64, i32), Tech>,
    pub locked: HashSet<(i64, i32)>,
    pub suppressed: HashSet<(i64, i32)>,
}

impl Sidecar {
    /// Ce qu'il faut retenir de la tablature (les effacees s'ajoutent a part).
    pub fn of(doc: &[TNote], suppressed: &HashSet<(i64, i32)>) -> Sidecar {
        let mut s = Sidecar {
            suppressed: suppressed.clone(),
            ..Sidecar::default()
        };
        for t in doc {
            if t.tech != Tech::Plain {
                s.tech.insert(t.key(), t.tech);
            }
            if t.locked {
                s.locked.insert(t.key());
            }
        }
        s
    }

    /// Une ligne : « H:debut:hauteur|P:...|L:...|S:... » (debut au 1/960 de
    /// noire).
    pub fn to_text(&self) -> String {
        let mut v: Vec<String> = Vec::new();
        let mut tech: Vec<_> = self.tech.iter().collect();
        tech.sort();
        for (&(q, p), &t) in tech {
            let c = if t == Tech::Hammer { 'H' } else { 'P' };
            v.push(format!("{c}:{q}:{p}"));
        }
        let mut l: Vec<_> = self.locked.iter().collect();
        l.sort();
        for &(q, p) in l {
            v.push(format!("L:{q}:{p}"));
        }
        let mut s: Vec<_> = self.suppressed.iter().collect();
        s.sort();
        for &(q, p) in s {
            v.push(format!("S:{q}:{p}"));
        }
        v.join("|")
    }

    pub fn from_text(s: &str) -> Sidecar {
        let mut out = Sidecar::default();
        for e in s.split('|') {
            let mut it = e.split(':');
            let (Some(c), Some(q), Some(p)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let (Ok(q), Ok(p)) = (q.parse::<i64>(), p.parse::<i32>()) else {
                continue;
            };
            match c {
                "H" => {
                    out.tech.insert((q, p), Tech::Hammer);
                }
                "P" => {
                    out.tech.insert((q, p), Tech::Pull);
                }
                "L" => {
                    out.locked.insert((q, p));
                }
                "S" => {
                    out.suppressed.insert((q, p));
                }
                _ => {}
            }
        }
        out
    }
}

/// L'item MIDI relu : la corde vient du canal ; une note dont la case sort du
/// manche (ou d'un canal hors des cordes) est replacee sur la corde jouable la
/// plus proche. Une note que le pense-bete ne connait pas a ete ecrite par
/// Cedric dans REAPER : elle est tenue.
pub fn from_midi(notes: &[MidiQn], tuning: &Tuning, side: &Sidecar) -> Vec<TNote> {
    let n = tuning.strings.len();
    let mut out: Vec<TNote> = notes
        .iter()
        .map(|m| {
            let mut string = n.saturating_sub(1).saturating_sub(m.chan as usize);
            let ok = |s: usize| {
                tuning
                    .strings
                    .get(s)
                    .is_some_and(|&o| (0..=tuning.frets).contains(&(m.pitch - o)))
            };
            if (m.chan as usize) >= n || !ok(string) {
                if let Some(s) = (0..n)
                    .filter(|&s| ok(s))
                    .min_by_key(|&s| (s as i64 - string as i64).abs())
                {
                    string = s;
                }
            }
            let fret = m.pitch - tuning.strings.get(string).copied().unwrap_or(40);
            let mut t = TNote {
                qn0: m.qn0,
                qn1: m.qn1,
                pitch: m.pitch,
                string,
                fret,
                vel: m.vel,
                tech: Tech::Plain,
                locked: false,
            };
            let k = t.key();
            t.tech = side.tech.get(&k).copied().unwrap_or_default();
            t.locked = side.locked.contains(&k);
            t
        })
        .collect();
    sort(&mut out);
    out
}

/// Un nouveau decodage applique a une tablature deja corrigee : les notes
/// tenues restent ; les notes detectees qui les recouvrent (meme corde ou
/// meme hauteur, au meme moment) et celles que Cedric a effacees ne
/// reviennent pas. `range` : seulement ce passage (en noires), le reste ne
/// bouge pas.
pub fn merge(
    old: &[TNote],
    fresh: Vec<TNote>,
    suppressed: &HashSet<(i64, i32)>,
    range: Option<(f64, f64)>,
) -> Vec<TNote> {
    let inside = |t: &TNote| range.is_none_or(|(a, b)| t.qn0 >= a && t.qn0 < b);
    let mut out: Vec<TNote> = old
        .iter()
        .filter(|t| t.locked || !inside(t))
        .cloned()
        .collect();
    for f in fresh {
        if !inside(&f) || suppressed.contains(&f.key()) {
            continue;
        }
        let clash = out.iter().any(|o| {
            inside(o)
                && o.qn0 < f.qn1
                && f.qn0 < o.qn1
                && (o.string == f.string || o.pitch == f.pitch)
        });
        if !clash {
            out.push(f);
        }
    }
    sort(&mut out);
    out
}
