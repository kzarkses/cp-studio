//! La mise en partition d'une tablature : le rythme sur la grille, les
//! accords, les silences, les mesures, les signes de duree (une noire, une
//! croche pointee...) et les liaisons de prolongation. La fenetre de Guitar
//! Tab DESSINE ce resultat et l'export MusicXML l'ECRIT : les deux montrent
//! exactement la meme partition.
//!
//! - Le RYTHME : chaque debut et chaque fin tombent sur la grille (`grid`
//!   cases par noire ; 4 = la double croche).
//! - Les ACCORDS : les notes qui commencent sur la meme case ; l'accord dure
//!   jusqu'au suivant au plus. Une corde ne joue qu'une note : une note
//!   s'arrete a la suivante sur sa corde.
//! - DEUX VOIX : une note grave tenue sous des notes plus aigues qui
//!   commencent pendant qu'elle sonne (une basse sous une melodie) passe
//!   dans la voix 2, hampes en bas ; elle n'est plus raccourcie. La voix 2
//!   ne s'ecrit que dans les mesures ou elle joue.
//! - Les MESURES : une note qui deborde est coupee et liee ; une duree qui ne
//!   s'ecrit pas d'un seul signe (5 doubles croches) devient deux signes lies.
//! - Les TRIOLETS : chaque temps (une noire) choisit entre la grille droite
//!   et des triolets de croches, selon ce qui explique le mieux ses attaques
//!   jouees. Un temps en triolets se coupe a ses bords ; ses signes portent
//!   le 3 (crochet, `<time-modification>` du MusicXML).

use std::collections::{BTreeMap, BTreeSet};

use crate::doc::{TNote, Tech};

/// Un evenement de la voix : un accord, ou un silence (`notes` vide).
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// 1 (la voix du haut) ou 2 (la basse tenue).
    pub voice: u8,
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
    /// La voix de son evenement.
    pub voice: u8,
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
    /// 3 : un signe de triolet (trois dans le temps de deux) ; 0 sinon.
    pub tuplet: u8,
    /// Le premier et le dernier signe de son temps en triolets : le crochet
    /// et son 3 vont de l'un a l'autre.
    pub tup_start: bool,
    pub tup_stop: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Engraved {
    /// Cases par noire, et par mesure. Avec des triolets, la grille est
    /// assez fine pour les deux (la grille droite fois 3, ou moins).
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
    /// Les temps en triolets (rang de la noire depuis l'origine).
    pub triplets: BTreeSet<i64>,
    /// Les mesures a deux voix (hampes en haut pour la 1, en bas pour la 2).
    pub two_voices: BTreeSet<i64>,
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

/// Le temps (rang de la noire) d'un instant `t` (noires depuis l'origine) :
/// une attaque un peu avant le temps lui appartient (la moitie d'une case
/// de la grille droite).
fn beat_of(t: f64, g: i64) -> i64 {
    (t + 0.5 / g as f64).floor() as i64
}

/// Les temps joues en triolets : ceux dont les attaques s'expliquent bien
/// mieux par trois croches par noire que par la grille droite (`g` cases
/// par noire). Il en faut deux distinctes, dont une hors du temps.
pub fn triplet_beats(notes: &[TNote], origin_qn: f64, g: i64) -> BTreeSet<i64> {
    let g = g.max(1);
    let mut out = BTreeSet::new();
    // Une grille droite qui contient deja les triolets (g multiple de 3) :
    // rien a choisir.
    if g % 3 == 0 {
        return out;
    }
    let mut by: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for n in notes {
        let t = n.qn0 - origin_qn;
        if t < -0.5 / g as f64 {
            continue;
        }
        let b = beat_of(t, g);
        by.entry(b).or_default().push(t - b as f64);
    }
    for (b, mut ph) in by {
        ph.sort_by(f64::total_cmp);
        // Les notes d'un accord comptent une fois.
        ph.dedup_by(|a, b| (*a - *b).abs() < 0.04);
        if ph.len() < 2 {
            continue;
        }
        let err = |k: f64| -> f64 {
            ph.iter()
                .map(|&p| {
                    let d = p - (p * k).round() / k;
                    d * d
                })
                .sum()
        };
        let (es, et) = (err(g as f64), err(3.0));
        let off = ph.iter().any(|&p| {
            let r = (p * 3.0).round();
            (r == 1.0 || r == 2.0) && (p * 3.0 - r).abs() < 0.25
        });
        if off && et < es * 0.35 {
            out.insert(b);
        }
    }
    out
}

/// Met la tablature en partition. Les notes avant `origin_qn` sont ignorees.
pub fn engrave(notes: &[TNote], origin_qn: f64, sig: (i32, i32), grid: u32) -> Engraved {
    let g0 = grid.max(1) as i64;
    let trip = triplet_beats(notes, origin_qn, g0);
    // La grille fine : la droite, et les triolets s'il y en a.
    let g = if trip.is_empty() {
        g0
    } else {
        g0 * 3 / gcd(g0, 3)
    };
    // Un instant sur la grille de son temps : droite ou en triolets ; et
    // le pas de cette grille.
    let cell = |t: f64| -> (i64, i64) {
        let b = beat_of(t, g0);
        let k = if trip.contains(&b) { 3 } else { g0 };
        let c = ((t - b as f64) * k as f64).round() as i64;
        (b * g + c * (g / k), g / k)
    };
    // 1. Le rythme, sur la grille.
    let mut groups: BTreeMap<i64, Vec<(usize, i64)>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        let (s, step) = cell(n.qn0 - origin_qn);
        let e = cell(n.qn1 - origin_qn).0.max(s + step);
        if s < 0 {
            continue;
        }
        groups.entry(s).or_default().push((i, e));
    }
    let mut eng = Engraved {
        grid: g,
        mlen: (sig.0.max(1) as i64 * g * 4 / sig.1.max(1) as i64).max(1),
        origin_qn,
        sig,
        triplets: trip.clone(),
        ..Engraved::default()
    };
    // 2. Les deux voix, puis les evenements de chacune.
    let (v1, v2) = split_voices(notes, &groups, g);
    let (mut ev1, t1) = voice_events(notes, &v1, 1, &mut eng);
    let (mut ev2, t2) = voice_events(notes, &v2, 2, &mut eng);
    eng.legato += ligatures(notes, &mut ev1) + ligatures(notes, &mut ev2);
    // 3. Les mesures (chaque voix completee d'un silence).
    let mlen = eng.mlen;
    let t = t1.max(t2);
    let nmeas = ((t.max(1) + mlen - 1) / mlen).max(1);
    for (evs, tv, voice) in [(&mut ev1, t1, 1u8), (&mut ev2, t2, 2u8)] {
        if tv < nmeas * mlen {
            evs.push(Event {
                voice,
                start: tv,
                dur: nmeas * mlen - tv,
                notes: Vec::new(),
                lig_in: Tech::Plain,
                lig_out: Tech::Plain,
            });
        }
    }
    for e in ev2.iter().filter(|e| !e.notes.is_empty()) {
        let (a, b) = (e.start / mlen, (e.start + e.dur - 1) / mlen);
        eng.two_voices.extend(a..=b);
    }
    eng.events = ev1;
    eng.events.extend(ev2);
    eng.measures = nmeas;
    // 4. Les signes. Un temps en triolets coupe ce qui le traverse a ses
    // bords, et s'ecrit en croches (ou noires) de triolet.
    let table = notatable(g);
    let ttable: Vec<(i64, &'static str, u8)> =
        vec![(2 * g / 3, "quarter", 0), (g / 3, "eighth", 0)];
    for m in 0..nmeas {
        let (m0, m1) = (m * mlen, (m + 1) * mlen);
        for (ei, ev) in eng.events.iter().enumerate() {
            let (a, b) = (ev.start.max(m0), (ev.start + ev.dur).min(m1));
            if b <= a || (ev.voice == 2 && !eng.two_voices.contains(&m)) {
                continue;
            }
            // Les morceaux : coupes aux bords des temps en triolets.
            let mut cuts = vec![a, b];
            for &tb in trip.range(a.div_euclid(g)..=b.div_euclid(g)) {
                for c in [tb * g, (tb + 1) * g] {
                    if c > a && c < b {
                        cuts.push(c);
                    }
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut parts: Vec<(i64, &'static str, u8, u8)> = Vec::new();
            for w in cuts.windows(2) {
                let (ca, cb) = (w[0], w[1]);
                let in_trip = trip.contains(&ca.div_euclid(g)) && cb - ca < g;
                let (tab, k) = if in_trip { (&ttable, 3) } else { (&table, 0) };
                let mut sp = split_dur(cb - ca, tab);
                // Un temps en triolets coupe par une barre au milieu (7/8) :
                // la grille droite.
                let (sp, k) = if sp.iter().map(|x| x.0).sum::<i64>() == cb - ca {
                    (std::mem::take(&mut sp), k)
                } else {
                    (split_dur(cb - ca, &table), 0)
                };
                parts.extend(sp.into_iter().map(|(d, t, dots)| (d, t, dots, k)));
            }
            let n = parts.len();
            let mut at = a;
            for (pi, (d, ty, dots, tuplet)) in parts.into_iter().enumerate() {
                eng.signs.push(Sign {
                    voice: ev.voice,
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
                    tuplet,
                    tup_start: false,
                    tup_stop: false,
                });
                at += d;
            }
        }
    }
    // Le premier et le dernier signe de chaque temps en triolets.
    let mut by_beat: BTreeMap<(u8, i64), Vec<usize>> = BTreeMap::new();
    for (si, sg) in eng.signs.iter().enumerate() {
        if sg.tuplet == 3 {
            by_beat
                .entry((sg.voice, sg.start.div_euclid(g)))
                .or_default()
                .push(si);
        }
    }
    for v in by_beat.values() {
        if let (Some(&f), Some(&l)) = (v.first(), v.last()) {
            eng.signs[f].tup_start = true;
            eng.signs[l].tup_stop = true;
        }
    }
    eng
}

/// Les notes d'une voix par case de debut : (rang, fin en cases).
type Groups = BTreeMap<i64, Vec<(usize, i64)>>;

/// Les deux voix. Une note s'arrete au plus tard a la note suivante sur sa
/// corde. Une note tenue au moins une croche au-dela de l'attaque suivante
/// passe dans la voix 2 si tout ce qui commence pendant qu'elle sonne est
/// plus aigu, et ressemble a une melodie : deux attaques au moins, aux deux
/// tiers des notes seules (des accords plaques qui resonnent restent une
/// seule voix).
fn split_voices(notes: &[TNote], groups: &Groups, g: i64) -> (Groups, Groups) {
    let mut flat: Vec<(i64, usize, i64)> = groups
        .iter()
        .flat_map(|(&s, v)| v.iter().map(move |&(i, e)| (s, i, e)))
        .collect();
    for k in 0..flat.len() {
        let (s, i, e) = flat[k];
        let cut = flat[k + 1..]
            .iter()
            .find(|x| x.0 > s && notes[x.1].string == notes[i].string)
            .map(|x| x.0);
        if let Some(c) = cut.filter(|&c| c < e) {
            flat[k].2 = c;
        }
    }
    let starts: Vec<i64> = groups.keys().copied().collect();
    let mut v2: BTreeSet<usize> = BTreeSet::new();
    for (k, &s) in starts.iter().enumerate() {
        let Some(&next) = starts.get(k + 1) else {
            continue;
        };
        for &(_, i, e) in flat.iter().filter(|x| x.0 == s) {
            if e - next < (g / 2).max(1) {
                continue;
            }
            let later: Vec<&(i64, usize, i64)> =
                flat.iter().filter(|x| x.0 >= next && x.0 < e).collect();
            if later.is_empty() || !later.iter().all(|x| notes[x.1].pitch > notes[i].pitch) {
                continue;
            }
            let mut onsets: Vec<i64> = later.iter().map(|x| x.0).collect();
            onsets.dedup();
            let single = onsets
                .iter()
                .filter(|&&o| later.iter().filter(|x| x.0 == o).count() == 1)
                .count();
            if onsets.len() >= 2 && single * 3 >= onsets.len() * 2 {
                v2.insert(i);
            }
        }
    }
    let (mut a, mut b) = (Groups::new(), Groups::new());
    for (s, i, e) in flat {
        let to = if v2.contains(&i) { &mut b } else { &mut a };
        to.entry(s).or_default().push((i, e));
    }
    (a, b)
}

/// Les evenements d'une voix : ses accords (chacun dure jusqu'au suivant au
/// plus) et les silences entre eux ; et la case ou elle s'arrete.
fn voice_events(
    notes: &[TNote],
    groups: &Groups,
    voice: u8,
    eng: &mut Engraved,
) -> (Vec<Event>, i64) {
    let starts: Vec<i64> = groups.keys().copied().collect();
    let mut out = Vec::new();
    let mut t = 0i64;
    for (k, (&s, gr)) in groups.iter().enumerate() {
        if s > t {
            out.push(Event {
                voice,
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
        out.push(Event {
            voice,
            start: s,
            dur: end - s,
            notes: idx,
            lig_in: Tech::Plain,
            lig_out: Tech::Plain,
        });
        t = end;
    }
    (out, t)
}

/// Les liaisons H/P d'une voix : une note seule, liee, collee a la note
/// seule d'avant sur la meme corde. Rend leur nombre.
fn ligatures(notes: &[TNote], evs: &mut [Event]) -> usize {
    let mut k = 0;
    for i in 1..evs.len() {
        let (a, b) = (&evs[i - 1], &evs[i]);
        if a.notes.len() != 1 || b.notes.len() != 1 || a.start + a.dur != b.start {
            continue;
        }
        let (na, nb) = (&notes[a.notes[0]], &notes[b.notes[0]]);
        if nb.tech == Tech::Plain || na.string != nb.string || na.pitch == nb.pitch {
            continue;
        }
        let tech = nb.tech;
        evs[i - 1].lig_out = tech;
        evs[i].lig_in = tech;
        k += 1;
    }
    k
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}
