//! La mise en partition d'une tablature : le rythme sur la grille, les
//! accords, les silences, les mesures, les signes de duree (une noire, une
//! croche pointee...) et les liaisons de prolongation. La fenetre de Guitar
//! Tab DESSINE ce resultat et l'export MusicXML l'ECRIT : les deux montrent
//! exactement la meme partition.
//!
//! - Le RYTHME : chaque debut et chaque fin tombent sur la grille (`grid`
//!   cases par noire ; 4 = la double croche).
//! - Les ACCORDS : les notes qui commencent sur la meme case. Une seule voix :
//!   l'accord dure jusqu'au suivant au plus.
//! - Les MESURES : une note qui deborde est coupee et liee ; une duree qui ne
//!   s'ecrit pas d'un seul signe (5 doubles croches) devient deux signes lies.

use std::collections::BTreeMap;

use crate::doc::{TNote, Tech};

/// Un evenement de la voix : un accord, ou un silence (`notes` vide).
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// En cases depuis l'origine.
    pub start: i64,
    pub dur: i64,
    /// Les notes (rangs dans la tablature), de la plus grave a la plus aigue.
    pub notes: Vec<usize>,
    /// La liaison qui arrive sur cet accord (une note seule), et celle qui
    /// en part.
    pub lig_in: Tech,
    pub lig_out: Tech,
}

/// Un signe de duree : une partie d'evenement dans une mesure.
#[derive(Clone, Debug, PartialEq)]
pub struct Sign {
    pub measure: i64,
    pub start: i64,
    pub dur: i64,
    /// « whole », « half », « quarter », « eighth », « 16th », « 32nd ».
    pub ty: &'static str,
    pub dots: u8,
    /// L'evenement (rang dans `events`).
    pub ev: usize,
    /// Lie au signe d'avant / d'apres (la meme note prolongee).
    pub tie_in: bool,
    pub tie_out: bool,
    /// La premiere (ou la derniere) partie de l'evenement : c'est elle qui
    /// porte la liaison d'arrivee (ou de depart).
    pub first: bool,
    pub last: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Engraved {
    /// Cases par noire, et par mesure.
    pub grid: i64,
    pub mlen: i64,
    pub measures: i64,
    /// La premiere barre de mesure, en noires du projet.
    pub origin_qn: f64,
    pub sig: (i32, i32),
    pub events: Vec<Event>,
    /// Dans l'ordre : mesure par mesure, evenement par evenement.
    pub signs: Vec<Sign>,
    /// Les notes (hors silences) et les accords ecrits, les liaisons H/P.
    pub notes: usize,
    pub chords: usize,
    pub legato: usize,
}

impl Engraved {
    /// La position en noires du projet d'une case.
    pub fn qn(&self, cell: i64) -> f64 {
        self.origin_qn + cell as f64 / self.grid.max(1) as f64
    }
}

/// Les durees qui s'ecrivent d'un seul signe, de la plus longue a la plus
/// courte : (cases, type, points).
pub fn notatable(grid: i64) -> Vec<(i64, &'static str, u8)> {
    let mut v = Vec::new();
    let base: [(i64, i64, &str); 6] = [
        (4, 1, "whole"),
        (2, 1, "half"),
        (1, 1, "quarter"),
        (1, 2, "eighth"),
        (1, 4, "16th"),
        (1, 8, "32nd"),
    ];
    for (num, den, name) in base {
        if (grid * num) % den != 0 {
            continue;
        }
        let d = grid * num / den;
        if d <= 0 {
            continue;
        }
        if (d * 3) % 2 == 0 {
            v.push((d * 3 / 2, name, 1u8));
        }
        v.push((d, name, 0u8));
    }
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    v.dedup_by_key(|x| x.0);
    v
}

/// Une duree en signes lies (le plus long d'abord).
pub fn split_dur(d: i64, table: &[(i64, &'static str, u8)]) -> Vec<(i64, &'static str, u8)> {
    let mut out = Vec::new();
    let mut left = d;
    while left > 0 {
        let Some(&p) = table.iter().find(|x| x.0 <= left) else {
            break;
        };
        out.push(p);
        left -= p.0;
    }
    out
}

/// Met la tablature en partition. Les notes avant `origin_qn` sont ignorees.
pub fn engrave(notes: &[TNote], origin_qn: f64, sig: (i32, i32), grid: u32) -> Engraved {
    let g = grid.max(1) as i64;
    let gf = g as f64;
    // 1. Le rythme, sur la grille.
    let mut groups: BTreeMap<i64, Vec<(usize, i64)>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        let s = ((n.qn0 - origin_qn) * gf).round() as i64;
        let e = (((n.qn1 - origin_qn) * gf).round() as i64).max(s + 1);
        if s < 0 {
            continue;
        }
        groups.entry(s).or_default().push((i, e));
    }
    let starts: Vec<i64> = groups.keys().copied().collect();
    let mut eng = Engraved {
        grid: g,
        mlen: (sig.0.max(1) as i64 * g * 4 / sig.1.max(1) as i64).max(1),
        origin_qn,
        sig,
        ..Engraved::default()
    };
    // 2. Les evenements de la voix.
    let mut t = 0i64;
    for (k, (&s, gr)) in groups.iter().enumerate() {
        if s > t {
            eng.events.push(Event {
                start: t,
                dur: s - t,
                notes: Vec::new(),
                lig_in: Tech::Plain,
                lig_out: Tech::Plain,
            });
        }
        let next = starts.get(k + 1).copied().unwrap_or(i64::MAX);
        let end = gr
            .iter()
            .map(|x| x.1)
            .max()
            .unwrap_or(s + 1)
            .min(next)
            .max(s + 1);
        let mut idx: Vec<usize> = gr.iter().map(|x| x.0).collect();
        idx.sort_by_key(|&i| notes[i].pitch);
        eng.notes += idx.len();
        eng.chords += 1;
        eng.events.push(Event {
            start: s,
            dur: end - s,
            notes: idx,
            lig_in: Tech::Plain,
            lig_out: Tech::Plain,
        });
        t = end;
    }
    // Les liaisons H/P : une note seule, liee, collee a la note seule
    // d'avant sur la meme corde.
    for i in 1..eng.events.len() {
        let (a, b) = (&eng.events[i - 1], &eng.events[i]);
        if a.notes.len() != 1 || b.notes.len() != 1 || a.start + a.dur != b.start {
            continue;
        }
        let (na, nb) = (&notes[a.notes[0]], &notes[b.notes[0]]);
        if nb.tech == Tech::Plain || na.string != nb.string || na.pitch == nb.pitch {
            continue;
        }
        let tech = nb.tech;
        eng.events[i - 1].lig_out = tech;
        eng.events[i].lig_in = tech;
        eng.legato += 1;
    }
    // 3. Les mesures (la derniere completee d'un silence).
    let mlen = eng.mlen;
    let nmeas = ((t.max(1) + mlen - 1) / mlen).max(1);
    if t < nmeas * mlen {
        eng.events.push(Event {
            start: t,
            dur: nmeas * mlen - t,
            notes: Vec::new(),
            lig_in: Tech::Plain,
            lig_out: Tech::Plain,
        });
    }
    eng.measures = nmeas;
    // 4. Les signes.
    let table = notatable(g);
    for m in 0..nmeas {
        let (m0, m1) = (m * mlen, (m + 1) * mlen);
        for (ei, ev) in eng.events.iter().enumerate() {
            let (a, b) = (ev.start.max(m0), (ev.start + ev.dur).min(m1));
            if b <= a {
                continue;
            }
            let parts = split_dur(b - a, &table);
            let n = parts.len();
            let mut at = a;
            for (pi, (d, ty, dots)) in parts.into_iter().enumerate() {
                eng.signs.push(Sign {
                    measure: m,
                    start: at,
                    dur: d,
                    ty,
                    dots,
                    ev: ei,
                    tie_in: pi > 0 || a > ev.start,
                    tie_out: pi + 1 < n || b < ev.start + ev.dur,
                    first: pi == 0 && a == ev.start,
                    last: pi + 1 == n && b == ev.start + ev.dur,
                });
                at += d;
            }
        }
    }
    eng
}
