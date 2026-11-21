//! Des notes a la partition : le rythme cale sur le tempo, les accords, les
//! mesures, le doigte, et le MusicXML que MuseScore ouvre.
//!
//! - Le RYTHME : chaque debut et chaque fin tombent sur une grille (`grid`
//!   cases par noire ; 4 = la double croche). Les positions arrivent deja en
//!   noires du projet : REAPER a converti le temps par SA carte de tempo, donc
//!   un morceau joue au clic tombe juste.
//! - Les ACCORDS : les notes qui commencent sur la meme case. Une seule voix :
//!   l'accord dure jusqu'au suivant au plus (une basse tenue sous une melodie
//!   est raccourcie ; c'est un premier jet).
//! - Les MESURES : une note qui deborde est coupee et liee ; une duree qui ne
//!   s'ecrit pas d'un seul signe (5 doubles croches) devient deux signes lies.
//! - Le MusicXML : une partie, deux portees -- la portee standard (clef de sol
//!   octaviee, les hauteurs REELLES) et la tablature (6 lignes, l'accordage),
//!   ou chaque note porte sa corde et sa case.
//! - Les LIES : une note sans attaque (`legato`, l'astuce melodia de Basic
//!   Pitch) qui suit la precedente sans silence, sur la meme corde, est un
//!   marteau (plus haute) ou un tire (plus basse) : une liaison, et « H » ou
//!   « P » sur la tablature.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::fret::{self, Spot, Tuning};

/// Une note a ecrire, en noires du projet.
#[derive(Clone, Debug, PartialEq)]
pub struct TabNote {
    pub qn0: f64,
    pub qn1: f64,
    pub pitch: i32,
    /// Jouee sans attaque (liee a la precedente).
    pub legato: bool,
}

#[derive(Clone, Debug)]
pub struct ScoreSpec {
    pub title: String,
    /// La barre de mesure ou commence la partition, en noires du projet.
    pub origin_qn: f64,
    /// (temps par mesure, valeur du temps).
    pub sig: (i32, i32),
    pub bpm: f64,
    /// Cases de la grille par noire.
    pub grid: u32,
    pub tuning: Tuning,
}

/// Ce que l'ecriture a fait.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub notes: usize,
    pub chords: usize,
    pub measures: usize,
    /// Notes qui ne se jouent pas (trop de notes ensemble) : absentes.
    pub dropped: usize,
    /// Lies ecrits (marteaux et tires).
    pub legato: usize,
}

/// Un evenement de la voix : un accord ou un silence.
#[derive(Clone, Debug)]
struct Ev {
    start: i64,
    dur: i64,
    /// (hauteur, place) ; vide = silence.
    notes: Vec<(i32, Spot)>,
    legato: bool,
    /// Le lie qui part de cette note, et celui qui y arrive
    /// (« hammer-on » ou « pull-off »).
    lig_start: Option<&'static str>,
    lig_stop: Option<&'static str>,
}

const NAMES: [(&str, i32); 12] = [
    ("C", 0),
    ("C", 1),
    ("D", 0),
    ("D", 1),
    ("E", 0),
    ("F", 0),
    ("F", 1),
    ("G", 0),
    ("G", 1),
    ("A", 0),
    ("A", 1),
    ("B", 0),
];

fn pitch_xml(p: i32) -> String {
    let (step, alter) = NAMES[p.rem_euclid(12) as usize];
    let oct = p.div_euclid(12) - 1;
    if alter != 0 {
        format!("<pitch><step>{step}</step><alter>{alter}</alter><octave>{oct}</octave></pitch>")
    } else {
        format!("<pitch><step>{step}</step><octave>{oct}</octave></pitch>")
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Les durees qui s'ecrivent d'un seul signe, de la plus longue a la plus
/// courte : (cases, type, points).
fn notatable(grid: i64) -> Vec<(i64, &'static str, u8)> {
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
fn split_dur(d: i64, table: &[(i64, &'static str, u8)]) -> Vec<(i64, &'static str, u8)> {
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

/// Ecrit la partition. Rend (MusicXML, ce qui a ete fait).
pub fn musicxml(notes: &[TabNote], spec: &ScoreSpec) -> (String, Stats) {
    let g = spec.grid.max(1) as i64;
    let tuning = &spec.tuning;
    let mut stats = Stats::default();
    // 1. Le rythme, sur la grille.
    let mut groups: BTreeMap<i64, Vec<(i32, i64, bool)>> = BTreeMap::new();
    for n in notes {
        let s = ((n.qn0 - spec.origin_qn) * g as f64).round() as i64;
        let e = (((n.qn1 - spec.origin_qn) * g as f64).round() as i64).max(s + 1);
        if s < 0 {
            continue;
        }
        let p = fret::fold(n.pitch, tuning);
        let gr = groups.entry(s).or_default();
        if let Some(x) = gr.iter_mut().find(|x| x.0 == p) {
            x.1 = x.1.max(e);
            x.2 &= n.legato;
        } else {
            gr.push((p, e, n.legato));
        }
    }
    // 2. Le doigte, sur toute la phrase.
    let starts: Vec<i64> = groups.keys().copied().collect();
    let pitches: Vec<Vec<i32>> = groups
        .values()
        .map(|v| v.iter().map(|x| x.0).collect())
        .collect();
    let sec_per_cell = 60.0 / spec.bpm.max(1.0) / g as f64;
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
    // 3. Les evenements de la voix.
    let mut evs: Vec<Ev> = Vec::new();
    let mut t = 0i64;
    for (i, (&s, gr)) in groups.iter().enumerate() {
        if s > t {
            evs.push(Ev {
                start: t,
                dur: s - t,
                notes: Vec::new(),
                legato: false,
                lig_start: None,
                lig_stop: None,
            });
        }
        let next = starts.get(i + 1).copied().unwrap_or(i64::MAX);
        let end = gr
            .iter()
            .map(|x| x.1)
            .max()
            .unwrap_or(s + 1)
            .min(next)
            .max(s + 1);
        let mut chord: Vec<(i32, Spot)> = Vec::new();
        for (k, &(p, _, _)) in gr.iter().enumerate() {
            match spots[i].get(k).copied().flatten() {
                Some(sp) => chord.push((p, sp)),
                None => stats.dropped += 1,
            }
        }
        chord.sort_by_key(|x| x.0);
        let legato = gr.len() == 1 && gr[0].2;
        if !chord.is_empty() {
            stats.notes += chord.len();
            stats.chords += 1;
        }
        evs.push(Ev {
            start: s,
            dur: end - s,
            notes: chord,
            legato,
            lig_start: None,
            lig_stop: None,
        });
        t = end;
    }
    // Les lies : une note seule, sans attaque, collee a la note seule qui la
    // precede sur la meme corde.
    for i in 1..evs.len() {
        let (a, b) = (&evs[i - 1], &evs[i]);
        if !b.legato || a.notes.len() != 1 || b.notes.len() != 1 {
            continue;
        }
        if a.start + a.dur != b.start {
            continue;
        }
        let ((pa, sa), (pb, sb)) = (a.notes[0], b.notes[0]);
        if sa.string != sb.string || pa == pb {
            continue;
        }
        let kind = if pb > pa { "hammer-on" } else { "pull-off" };
        evs[i - 1].lig_start = Some(kind);
        evs[i].lig_stop = Some(kind);
        stats.legato += 1;
    }
    // 4. Les mesures (la derniere completee d'un silence).
    let mlen = (spec.sig.0.max(1) as i64 * g * 4 / spec.sig.1.max(1) as i64).max(1);
    let total = t.max(1);
    let nmeas = ((total + mlen - 1) / mlen).max(1);
    if t < nmeas * mlen {
        evs.push(Ev {
            start: t,
            dur: nmeas * mlen - t,
            notes: Vec::new(),
            legato: false,
            lig_start: None,
            lig_stop: None,
        });
    }
    stats.measures = nmeas as usize;
    let table = notatable(g);

    // 5. Le MusicXML.
    let mut x = String::new();
    x += "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n";
    x += "<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n";
    x += "<score-partwise version=\"4.0\">\n";
    let _ = writeln!(
        x,
        "  <work><work-title>{}</work-title></work>",
        esc(&spec.title)
    );
    x += "  <identification><encoding><software>CP Studio - Guitar Tab</software></encoding></identification>\n";
    x += "  <part-list><score-part id=\"P1\"><part-name>Guitar</part-name></score-part></part-list>\n";
    x += "  <part id=\"P1\">\n";
    let nstr = tuning.strings.len();
    for m in 0..nmeas {
        let (m0, m1) = (m * mlen, (m + 1) * mlen);
        let _ = writeln!(x, "    <measure number=\"{}\">", m + 1);
        if m == 0 {
            x += "      <attributes>\n";
            let _ = writeln!(x, "        <divisions>{g}</divisions>");
            x += "        <key><fifths>0</fifths></key>\n";
            let _ = writeln!(
                x,
                "        <time><beats>{}</beats><beat-type>{}</beat-type></time>",
                spec.sig.0, spec.sig.1
            );
            x += "        <staves>2</staves>\n";
            x += "        <clef number=\"1\"><sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change></clef>\n";
            x += "        <clef number=\"2\"><sign>TAB</sign><line>5</line></clef>\n";
            x += "        <staff-details number=\"2\">\n";
            let _ = writeln!(x, "          <staff-lines>{nstr}</staff-lines>");
            for (i, &open) in tuning.strings.iter().enumerate() {
                let (step, alter) = NAMES[open.rem_euclid(12) as usize];
                let _ = write!(
                    x,
                    "          <staff-tuning line=\"{}\"><tuning-step>{step}</tuning-step>",
                    i + 1
                );
                if alter != 0 {
                    let _ = write!(x, "<tuning-alter>{alter}</tuning-alter>");
                }
                let _ = writeln!(
                    x,
                    "<tuning-octave>{}</tuning-octave></staff-tuning>",
                    open.div_euclid(12) - 1
                );
            }
            x += "        </staff-details>\n";
            x += "      </attributes>\n";
            let _ = writeln!(
                x,
                "      <direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>",
                spec.bpm.round(),
                spec.bpm
            );
        }
        for staff in [1, 2] {
            if staff == 2 {
                let _ = writeln!(x, "      <backup><duration>{mlen}</duration></backup>");
            }
            let voice = if staff == 1 { 1 } else { 5 };
            for ev in &evs {
                let (a, b) = (ev.start.max(m0), (ev.start + ev.dur).min(m1));
                if b <= a {
                    continue;
                }
                let parts = split_dur(b - a, &table);
                let n = parts.len();
                for (pi, (d, ty, dots)) in parts.into_iter().enumerate() {
                    // Lie a ce qui precede (dans la note ou depuis la mesure
                    // precedente), et a ce qui suit.
                    let tie_stop = pi > 0 || a > ev.start;
                    let tie_start = pi + 1 < n || b < ev.start + ev.dur;
                    if ev.notes.is_empty() {
                        let _ = write!(
                            x,
                            "      <note><rest/><duration>{d}</duration><voice>{voice}</voice><type>{ty}</type>"
                        );
                        for _ in 0..dots {
                            x += "<dot/>";
                        }
                        let _ = writeln!(x, "<staff>{staff}</staff></note>");
                        continue;
                    }
                    for (ci, &(p, sp)) in ev.notes.iter().enumerate() {
                        x += "      <note>";
                        if ci > 0 {
                            x += "<chord/>";
                        }
                        x += &pitch_xml(p);
                        let _ = write!(x, "<duration>{d}</duration>");
                        if tie_stop {
                            x += "<tie type=\"stop\"/>";
                        }
                        if tie_start {
                            x += "<tie type=\"start\"/>";
                        }
                        let _ = write!(x, "<voice>{voice}</voice><type>{ty}</type>");
                        for _ in 0..dots {
                            x += "<dot/>";
                        }
                        let _ = write!(x, "<staff>{staff}</staff>");
                        let mut nots = String::new();
                        if tie_stop {
                            nots += "<tied type=\"stop\"/>";
                        }
                        if tie_start {
                            nots += "<tied type=\"start\"/>";
                        }
                        // Le lie arrive sur la premiere partie de la note et
                        // part de la derniere.
                        let lstop = ev.lig_stop.filter(|_| pi == 0 && a == ev.start);
                        let lstart = ev
                            .lig_start
                            .filter(|_| pi + 1 == n && b == ev.start + ev.dur);
                        if lstop.is_some() {
                            nots += "<slur type=\"stop\" number=\"1\"/>";
                        }
                        if lstart.is_some() {
                            nots += "<slur type=\"start\" number=\"1\"/>";
                        }
                        if staff == 2 {
                            let mut tech = format!(
                                "<string>{}</string><fret>{}</fret>",
                                nstr - sp.string,
                                sp.fret
                            );
                            let mark = |k: &str| if k == "hammer-on" { "H" } else { "P" };
                            if let Some(k) = lstop {
                                let _ = write!(tech, "<{k} number=\"1\" type=\"stop\"/>");
                            }
                            if let Some(k) = lstart {
                                let _ = write!(
                                    tech,
                                    "<{k} number=\"1\" type=\"start\">{}</{k}>",
                                    mark(k)
                                );
                            }
                            let _ = write!(nots, "<technical>{tech}</technical>");
                        }
                        if !nots.is_empty() {
                            let _ = write!(x, "<notations>{nots}</notations>");
                        }
                        x += "</note>\n";
                    }
                }
            }
        }
        x += "    </measure>\n";
    }
    x += "  </part>\n</score-partwise>\n";
    (x, stats)
}
