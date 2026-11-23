//! La tablature editable : une liste de notes, chacune avec sa corde, sa case
//! et sa liaison. C'est elle que l'item MIDI garde dans le projet (une corde
//! par canal MIDI), elle que la fenetre de Guitar Tab dessine et modifie, et
//! elle que la partition ecrit.
//!
//! - [`finger`] : des notes detectees a la tablature (le doigte sur toute la
//!   phrase, les marteaux et les tires) ;
//! - [`ghosts`] : les notes que le reseau a PRESQUE entendues (sous le seuil),
//!   que Cedric garde d'un clic ;
//! - [`to_midi`] / [`from_midi`] : l'aller-retour avec l'item MIDI (les
//!   bends et les glisses y sont des pitch bends du canal de la corde,
//!   [`midi_events`]), et [`Sidecar`] : ce que le MIDI ne sait pas porter
//!   (liaisons, notes touchees par Cedric, notes qu'il a effacees).

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
    /// Glisse depuis la precedente, sur la meme corde, sans attaque.
    Slide,
}

/// Une note de la tablature.
#[derive(Clone, Debug, PartialEq, Default)]
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
    /// La hauteur jouee au fil de la note : (noires depuis son debut,
    /// demi-tons par rapport a elle), lineaire entre les points ; vide : la
    /// note juste. Une courbe qui part de plus bas (ou plus haut) et rejoint 0
    /// est une glisse d'approche ; ce qui monte ensuite est un bend. C'est
    /// elle que portent les pitch bends du MIDI.
    pub bend: Vec<(f64, f32)>,
    /// Les techniques de jeu : [`TNote::PALM`], [`TNote::DEAD`],
    /// [`TNote::HARMONIC`], [`TNote::RING`], [`TNote::VIBRATO`].
    pub marks: u8,
}

/// Une courbe de hauteur : (noires depuis le debut de la note, demi-tons).
pub type Curve = Vec<(f64, f32)>;

/// Le bend d'une note, pour l'ecrire : de combien (demi-tons, au demi
/// pres), quand il atteint sa hauteur, et quand il est relache.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendMark {
    pub amount: f32,
    pub peak: f64,
    pub release: Option<f64>,
}

impl TNote {
    /// Etouffee de la paume (« P.M. »).
    pub const PALM: u8 = 1;
    /// Note morte (« x ») : la corde frappee sans hauteur.
    pub const DEAD: u8 = 2;
    /// Harmonique naturelle (« <12> »).
    pub const HARMONIC: u8 = 4;
    /// Laisser sonner (« let ring »).
    pub const RING: u8 = 8;
    /// Vibrato (« ~ »).
    pub const VIBRATO: u8 = 16;

    pub fn has(&self, m: u8) -> bool {
        self.marks & m != 0
    }

    /// La glisse d'approche : d'ou elle part (demi-tons, negatif : d'en
    /// dessous), 0 : aucune.
    pub fn slide_in(&self) -> i8 {
        match self.bend.first() {
            Some(&(t, v)) if t < 1e-6 && v.abs() >= 0.75 => v.round().clamp(-24.0, 24.0) as i8,
            _ => 0,
        }
    }

    /// Les deux parties de la courbe : la glisse d'approche (jusqu'a son
    /// arrivee comprise) et le reste (le bend).
    fn curve_parts(&self) -> (Curve, Curve) {
        if self.slide_in() == 0 {
            return (Vec::new(), self.bend.clone());
        }
        match self.bend.iter().position(|p| p.0 > 1e-9 && p.1.abs() < 0.3) {
            Some(k) => (self.bend[..=k].to_vec(), self.bend[k + 1..].to_vec()),
            None => (self.bend.clone(), Vec::new()),
        }
    }

    fn set_curve(&mut self, head: Vec<(f64, f32)>, rest: Vec<(f64, f32)>) {
        let from = head.last().map_or(-1.0, |p| p.0);
        let mut v = head;
        v.extend(rest.into_iter().filter(|p| p.0 > from + 1e-9));
        if v.iter().all(|p| p.1.abs() < 0.05) {
            v.clear();
        }
        self.bend = v;
    }

    /// Pose une glisse d'approche de `k` demi-tons (negatif : d'en dessous ;
    /// 0 : aucune). Le bend reste.
    pub fn set_slide_in(&mut self, k: i8) {
        let (_, rest) = self.curve_parts();
        let len = (self.qn1 - self.qn0).max(1e-3);
        let head = if k == 0 {
            Vec::new()
        } else {
            vec![(0.0, k as f32), (SLIDE_QN.min(len * 0.3), 0.0)]
        };
        self.set_curve(head, rest);
    }

    /// Pose un bend de `semis` demi-tons (`None` : aucun), relache ou non :
    /// il monte sur le premier quart de la note, et redescend apres le
    /// milieu. La glisse d'approche reste.
    pub fn set_bend(&mut self, semis: Option<f32>, release: bool) {
        let (head, _) = self.curve_parts();
        let len = (self.qn1 - self.qn0).max(1e-3);
        let t0 = head.last().map_or(0.0, |p| p.0);
        let span = (len - t0).max(1e-3);
        let up = (span * 0.25).min(0.5);
        let rest = match semis {
            None => Vec::new(),
            Some(a) => {
                let mut v = vec![(t0, 0.0), (t0 + up, a)];
                if release {
                    let r0 = t0 + span * 0.6;
                    v.push((r0, a));
                    v.push(((r0 + up).min(t0 + span * 0.95), 0.0));
                }
                v
            }
        };
        self.set_curve(head, rest);
    }

    /// Le bend, s'il y en a un (apres la glisse d'approche).
    pub fn bend_mark(&self) -> Option<BendMark> {
        let from = if self.slide_in() != 0 {
            self.bend.iter().position(|p| p.1.abs() < 0.3)?
        } else {
            0
        };
        let rest = &self.bend[from..];
        let top = rest.iter().map(|p| p.1).fold(0f32, f32::max);
        if top < 0.4 {
            return None;
        }
        // Un vibrato autour de la note n'est pas un bend (un bend suivi d'un
        // vibrato l'est : sa courbe reste haute).
        if self.has(TNote::VIBRATO) {
            let mean = rest.iter().map(|p| p.1).sum::<f32>() / rest.len().max(1) as f32;
            if mean < 0.35 {
                return None;
            }
        }
        let pk = rest.iter().position(|p| p.1 >= top * 0.8)?;
        let release = rest[pk..].iter().find(|p| p.1 <= top * 0.35).map(|p| p.0);
        Some(BendMark {
            amount: (top * 2.0).round().max(1.0) / 2.0,
            peak: rest[pk].0,
            release,
        })
    }

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

/// Une note d'accord.
#[derive(Clone)]
struct Member {
    p: i32,
    q0: f64,
    q1: f64,
    legato: bool,
    vel: u8,
    slid: bool,
    bend: Vec<(f64, f32)>,
    vibrato: bool,
}

/// Comment les notes d'un accord se reconnaissent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Chords {
    /// Elles commencent sur la meme case de la grille (`grid` par noire).
    Grid(u32),
    /// Elles commencent a moins de tant de secondes de la premiere (le mode
    /// libre, sans grille : un accord gratte s'etale sur quelques ms).
    Within(f64),
}

/// Les cases par noire du mode libre : assez fines pour garder le temps
/// joue (une demi-milliseconde a 120 BPM).
pub const FREE_CELLS: u32 = 960;

/// Des notes detectees a la tablature. Les notes qui commencent sur la meme
/// case de la grille (`grid` par noire) forment un accord ; le doigte se
/// choisit sur toute la phrase ([`fret::assign`]) ; une note sans attaque
/// collee a la precedente sur la meme corde est un marteau ou un tire. Rend
/// (tablature, notes injouables laissees de cote).
pub fn finger(notes: &[TabNote], grid: u32, bpm: f64, tuning: &Tuning) -> (Vec<TNote>, usize) {
    finger_by(notes, Chords::Grid(grid), bpm, tuning)
}

/// [`finger`], les accords reconnus selon `by`.
pub fn finger_by(notes: &[TabNote], by: Chords, bpm: f64, tuning: &Tuning) -> (Vec<TNote>, usize) {
    let (g, tol_cells) = match by {
        Chords::Grid(grid) => (grid.max(1) as f64, 0i64),
        Chords::Within(sec) => {
            let g = FREE_CELLS as f64;
            (g, (sec.max(0.0) * bpm.max(1.0) / 60.0 * g).round() as i64)
        }
    };
    // La case de chaque note : la sienne sur la grille, ou celle de la
    // premiere note de son accord (mode libre).
    let mut cell = vec![0i64; notes.len()];
    match by {
        Chords::Grid(_) => {
            for (k, n) in notes.iter().enumerate() {
                cell[k] = (n.qn0 * g).round() as i64;
            }
        }
        Chords::Within(_) => {
            let mut order: Vec<usize> = (0..notes.len()).collect();
            order.sort_by(|&a, &b| notes[a].qn0.total_cmp(&notes[b].qn0));
            let mut start: Option<i64> = None;
            for k in order {
                let c = (notes[k].qn0 * g).round() as i64;
                let s = match start {
                    Some(s) if c - s <= tol_cells => s,
                    _ => c,
                };
                start = Some(s);
                cell[k] = s;
            }
        }
    }
    // Les accords.
    let mut groups: BTreeMap<i64, Vec<Member>> = BTreeMap::new();
    for (k, n) in notes.iter().enumerate() {
        let s = cell[k];
        let p = fret::fold(n.pitch, tuning);
        let gr = groups.entry(s).or_default();
        if let Some(x) = gr.iter_mut().find(|x| x.p == p) {
            x.q0 = x.q0.min(n.qn0);
            x.q1 = x.q1.max(n.qn1);
            x.legato &= n.legato;
            x.vel = x.vel.max(n.vel);
            x.slid &= n.slid;
            x.vibrato |= n.vibrato;
            if x.bend.is_empty() {
                x.bend = n.bend.clone();
            }
        } else {
            gr.push(Member {
                p,
                q0: n.qn0,
                q1: n.qn1,
                legato: n.legato,
                vel: n.vel,
                slid: n.slid,
                bend: n.bend.clone(),
                vibrato: n.vibrato,
            });
        }
    }
    let starts: Vec<i64> = groups.keys().copied().collect();
    let pitches: Vec<Vec<i32>> = groups
        .values()
        .map(|v| v.iter().map(|x| x.p).collect())
        .collect();
    // Une note seule glissee depuis une note seule : la meme corde.
    let firsts: Vec<bool> = groups.values().map(|v| v.len() == 1 && v[0].slid).collect();
    let bonds: Vec<bool> = (0..firsts.len())
        .map(|i| i > 0 && firsts[i] && pitches[i - 1].len() == 1)
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
    let spots = fret::assign_bonded(&pitches, &gaps, &bonds, tuning);
    let mut out: Vec<TNote> = Vec::new();
    let mut dropped = 0;
    // Pour les liaisons : la note seule de l'accord precedent (indice, fin en
    // cases), et son debut.
    let mut prev_single: Option<(usize, i64)> = None;
    for (i, (&s, gr)) in groups.iter().enumerate() {
        let next = starts.get(i + 1).copied().unwrap_or(i64::MAX);
        let first = out.len();
        for (k, m) in gr.iter().enumerate() {
            match spots[i].get(k).copied().flatten() {
                Some(sp) => out.push(TNote {
                    qn0: m.q0,
                    qn1: m.q1.max(m.q0 + 1e-6),
                    pitch: m.p,
                    string: sp.string,
                    fret: sp.fret,
                    vel: m.vel,
                    tech: Tech::Plain,
                    locked: false,
                    bend: m.bend.clone(),
                    marks: if m.vibrato { TNote::VIBRATO } else { 0 },
                }),
                None => dropped += 1,
            }
        }
        let placed = out.len() - first;
        if placed == 1 && gr.len() == 1 {
            let b = first;
            if let Some((a, a_end)) = prev_single {
                let (legato, slid) = (gr[0].legato, gr[0].slid);
                if (legato || slid)
                    && a_end >= s - tol_cells
                    && out[a].string == out[b].string
                    && out[a].pitch != out[b].pitch
                {
                    out[b].tech = if slid {
                        Tech::Slide
                    } else if out[b].pitch > out[a].pitch {
                        Tech::Hammer
                    } else {
                        Tech::Pull
                    };
                }
            }
            let e = ((gr[0].q1 * g).round() as i64).min(next).max(s + 1);
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
            bend: Vec::new(),
            marks: 0,
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

/// Un evenement de canal ecrit avec les notes : un pitch bend (demi-tons,
/// l'etendue est de [`BEND_RANGE`]) ou la pedale legato (CC 68 : la note
/// suivante ne se repince pas).
#[derive(Clone, Debug, PartialEq)]
pub struct MidiCc {
    pub qn: f64,
    pub chan: u8,
    /// Decalage en tics du MIDI (-1 : juste avant la note de ce moment).
    pub tick: i32,
    pub kind: CcKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CcKind {
    Bend(f32),
    Legato(bool),
}

/// L'etendue des pitch bends ecrits (RPN 0 au debut de l'item) : une glisse
/// d'une octave tient.
pub const BEND_RANGE: f32 = 12.0;

/// La duree de la rampe d'une glisse entre deux notes (noires), au plus.
pub const SLIDE_QN: f64 = 0.125;

fn slide_ramp(a: &TNote) -> f64 {
    SLIDE_QN.min((a.qn1 - a.qn0) * 0.5).max(1.0 / 64.0)
}

/// Les points d'une courbe, tous les 1/64 de noire entre ses points
/// (les pitch bends sont des marches).
fn dense(pts: &[(f64, f32)]) -> Vec<(f64, f32)> {
    let step = 1.0 / 64.0;
    let mut out: Vec<(f64, f32)> = Vec::new();
    for w in pts.windows(2) {
        let ((ta, va), (tb, vb)) = (w[0], w[1]);
        out.push((ta, va));
        if (vb - va).abs() > 0.02 {
            let mut t = ta + step;
            while t < tb - 1e-9 {
                let k = ((t - ta) / (tb - ta)) as f32;
                out.push((t, va + (vb - va) * k));
                t += step;
            }
        }
    }
    if let Some(&l) = pts.last() {
        out.push(l);
    }
    out
}

/// Les pitch bends et les pedales legato de la tablature : la courbe de
/// chaque note sur le canal de sa corde ; une glisse vers la note suivante
/// monte en fin de note ; le canal revient a 0 avant la note d'apres ; un
/// marteau, un tire et une glisse ne se repincent pas (pedale legato le temps
/// de la note).
pub fn midi_events(doc: &[TNote], tuning: &Tuning) -> Vec<MidiCc> {
    let n = tuning.strings.len();
    let chan_of = |t: &TNote| (n.saturating_sub(1).saturating_sub(t.string)).min(15) as u8;
    let mut by: BTreeMap<u8, Vec<&TNote>> = BTreeMap::new();
    for t in doc {
        by.entry(chan_of(t)).or_default().push(t);
    }
    let mut out = Vec::new();
    for (ch, mut v) in by {
        v.sort_by(|a, b| {
            a.qn0
                .partial_cmp(&b.qn0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut bent = false;
        for (k, a) in v.iter().enumerate() {
            let next = v.get(k + 1);
            let mut pts: Vec<(f64, f32)> = a.bend.clone();
            // La glisse vers la note suivante : la fin de celle-ci monte.
            if let Some(b) = next.filter(|b| b.tech == Tech::Slide && (b.qn0 - a.qn1).abs() < 0.02)
            {
                let len = a.qn1 - a.qn0;
                let r = slide_ramp(a);
                let at = (len - r).max(0.0);
                let from = value_at(&pts, at);
                pts.retain(|p| p.0 < at - 1e-9);
                pts.push((at, from));
                pts.push((len - 1.0 / 128.0, (b.pitch - a.pitch) as f32));
            }
            if !pts.is_empty() || bent {
                // Le canal part de la hauteur de la courbe (0 sans courbe).
                let first = pts
                    .first()
                    .map_or(0.0, |p| if p.0 < 1e-9 { p.1 } else { 0.0 });
                out.push(MidiCc {
                    qn: a.qn0,
                    chan: ch,
                    tick: -1,
                    kind: CcKind::Bend(first),
                });
                for (t, val) in dense(&pts).into_iter().filter(|p| p.0 > 1e-9) {
                    out.push(MidiCc {
                        qn: a.qn0 + t,
                        chan: ch,
                        tick: 0,
                        kind: CcKind::Bend(val.clamp(-BEND_RANGE, BEND_RANGE)),
                    });
                }
                bent = pts.iter().any(|p| p.1.abs() > 1e-3);
                if bent {
                    // Retour a 0 : avant la note suivante, ou apres la queue.
                    let at = next.map_or(a.qn1 + 0.5, |b| b.qn0.min(a.qn1 + 0.5));
                    if next.is_none_or(|b| at < b.qn0 - 1e-6) {
                        out.push(MidiCc {
                            qn: at,
                            chan: ch,
                            tick: 0,
                            kind: CcKind::Bend(0.0),
                        });
                        bent = false;
                    }
                }
            }
            if matches!(a.tech, Tech::Hammer | Tech::Pull | Tech::Slide) {
                out.push(MidiCc {
                    qn: a.qn0,
                    chan: ch,
                    tick: -1,
                    kind: CcKind::Legato(true),
                });
                out.push(MidiCc {
                    qn: a.qn0,
                    chan: ch,
                    tick: 1,
                    kind: CcKind::Legato(false),
                });
            }
        }
    }
    out.sort_by(|a, b| {
        (a.qn, a.tick)
            .partial_cmp(&(b.qn, b.tick))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// La valeur d'une courbe a `t` (lineaire, tenue aux bouts).
pub fn value_at(pts: &[(f64, f32)], t: f64) -> f32 {
    match pts.iter().position(|p| p.0 > t) {
        None => pts.last().map_or(0.0, |p| p.1),
        Some(0) => pts[0].1,
        Some(k) => {
            let ((ta, va), (tb, vb)) = (pts[k - 1], pts[k]);
            if tb - ta < 1e-12 {
                vb
            } else {
                va + (vb - va) * ((t - ta) / (tb - ta)) as f32
            }
        }
    }
}

/// Garde les points qui comptent d'une courbe lue (une rampe dense de pitch
/// bends redevient deux points).
pub fn simplify_curve(v: &[(f64, f32)], tol: f32) -> Vec<(f64, f32)> {
    if v.len() <= 2 {
        return v.to_vec();
    }
    let mut keep = vec![false; v.len()];
    keep[0] = true;
    keep[v.len() - 1] = true;
    let mut stack = vec![(0usize, v.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut worst, mut wi) = (0f32, a);
        for k in a + 1..b {
            let lin = value_at(&[v[a], v[b]], v[k].0);
            let e = (v[k].1 - lin).abs();
            if e > worst {
                worst = e;
                wi = k;
            }
        }
        if worst > tol {
            keep[wi] = true;
            stack.push((a, wi));
            stack.push((wi, b));
        }
    }
    v.iter().zip(&keep).filter(|x| *x.1).map(|x| *x.0).collect()
}

/// Ce que le MIDI ne porte pas : les liaisons, les notes touchees par Cedric,
/// et celles qu'il a effacees (un nouveau decodage ne les ramene pas).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sidecar {
    pub tech: HashMap<(i64, i32), Tech>,
    /// Les techniques de jeu ([`TNote::marks`]).
    pub marks: HashMap<(i64, i32), u8>,
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
            if t.marks != 0 {
                s.marks.insert(t.key(), t.marks);
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
            let c = match t {
                Tech::Hammer => 'H',
                Tech::Slide => 'G',
                _ => 'P',
            };
            v.push(format!("{c}:{q}:{p}"));
        }
        let mut l: Vec<_> = self.locked.iter().collect();
        l.sort();
        for &(q, p) in l {
            v.push(format!("L:{q}:{p}"));
        }
        let mut mk: Vec<_> = self.marks.iter().collect();
        mk.sort();
        for (&(q, p), &m) in mk {
            v.push(format!("K:{q}:{p}:{m}"));
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
                "G" => {
                    out.tech.insert((q, p), Tech::Slide);
                }
                "L" => {
                    out.locked.insert((q, p));
                }
                "K" => {
                    if let Some(m) = it.next().and_then(|m| m.parse::<u8>().ok()) {
                        out.marks.insert((q, p), m);
                    }
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
/// Cedric dans REAPER : elle est tenue. `bends` : les pitch bends de l'item
/// (noires, canal, demi-tons) ; chaque note prend ceux de son canal pendant
/// qu'elle sonne (sa courbe), moins la rampe d'une glisse vers la suivante.
pub fn from_midi(
    notes: &[MidiQn],
    bends: &[(f64, u8, f32)],
    tuning: &Tuning,
    side: &Sidecar,
) -> Vec<TNote> {
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
                bend: Vec::new(),
                marks: 0,
            };
            let k = t.key();
            t.tech = side.tech.get(&k).copied().unwrap_or_default();
            t.locked = side.locked.contains(&k);
            t.marks = side.marks.get(&k).copied().unwrap_or(0);
            // La courbe : les pitch bends du canal pendant la note (celui
            // pose juste avant son debut compris).
            let eps = 1.0 / 480.0;
            let mut pts: Vec<(f64, f32)> = bends
                .iter()
                .filter(|b| b.1 == m.chan && b.0 >= m.qn0 - eps && b.0 < m.qn1 - 1e-9)
                .map(|b| ((b.0 - m.qn0).max(0.0), b.2))
                .collect();
            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            if pts.iter().any(|p| p.1.abs() > 0.05) {
                t.bend = simplify_curve(&pts, 0.06);
            }
            (t, m.chan)
        })
        .collect::<Vec<(TNote, u8)>>()
        .into_iter()
        .map(|x| x.0)
        .collect();
    sort(&mut out);
    // La rampe d'une glisse appartient a la liaison, pas a la note d'avant.
    for k in 0..out.len() {
        if out[k].tech != Tech::Slide {
            continue;
        }
        let (q0, s) = (out[k].qn0, out[k].string);
        if let Some(a) = (0..k)
            .rev()
            .find(|&j| out[j].string == s && (out[j].qn1 - q0).abs() < 0.02)
        {
            let at = (out[a].qn1 - out[a].qn0) - slide_ramp(&out[a]);
            out[a].bend.retain(|p| p.0 < at - 1e-6);
            if out[a].bend.iter().all(|p| p.1.abs() <= 0.05) {
                out[a].bend.clear();
            }
        }
    }
    out
}

/// Le MIDI a ete modifie par Cedric (dans l'editeur, dans REAPER) : ce qui a
/// disparu de la tablature d'avant est EFFACE (ni un decodage ni un fantome
/// ne le ramene), ce qui est apparu est TENU (un decodage le respecte). Le
/// MIDI s'eloigne de l'audio, et c'est voulu.
pub fn reconcile(prev: &[TNote], now: &mut [TNote], suppressed: &mut HashSet<(i64, i32)>) {
    let before: HashSet<(i64, i32)> = prev.iter().map(TNote::key).collect();
    let after: HashSet<(i64, i32)> = now.iter().map(TNote::key).collect();
    for k in before.difference(&after) {
        suppressed.insert(*k);
    }
    for n in now.iter_mut() {
        if !before.contains(&n.key()) {
            n.locked = true;
            suppressed.remove(&n.key());
        }
    }
}

/// Retire les fantomes d'une note effacee (meme hauteur, a une case de
/// grille pres).
pub fn drop_suppressed(ghosts: &mut Vec<TNote>, suppressed: &HashSet<(i64, i32)>, grid: u32) {
    let cell = (960 / grid.max(1)) as i64;
    ghosts.retain(|g| {
        let (q, p) = g.key();
        !suppressed
            .iter()
            .any(|&(sq, sp)| sp == p && (sq - q).abs() <= cell)
    });
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

/// Ou se rangent les dates d'une tablature dans son fichier.
pub enum Clock<'a> {
    /// En mesures : en noires depuis le debut de l'item (`at_qn`).
    Qn(f64),
    /// En jeu libre : en secondes depuis le debut de l'item (noires du
    /// projet -> secondes). Un tempo pose plus tard ne les bouge pas : les
    /// notes restent sur l'audio.
    Secs(&'a dyn Fn(f64) -> f64),
}

/// La tablature d'une prise, gardee dans un fichier a cote de son analyse
/// (`<GUID de la prise>.cptab`) : les notes, les notes effacees et les
/// reglages de detection. Les positions sont RELATIVES au debut de l'item
/// (`at_qn`) : un item deplace garde sa tablature.
pub fn tab_to_text(
    doc: &[TNote],
    suppressed: &HashSet<(i64, i32)>,
    detect: &str,
    at_qn: f64,
) -> String {
    tab_to_text_on(doc, suppressed, detect, &Clock::Qn(at_qn))
}

/// [`tab_to_text`], les dates selon `clock` (le jeu libre les range en
/// secondes : la ligne « unit s » le dit).
pub fn tab_to_text_on(
    doc: &[TNote],
    suppressed: &HashSet<(i64, i32)>,
    detect: &str,
    clock: &Clock,
) -> String {
    let rel = |q: f64| match clock {
        Clock::Qn(at) => q - at,
        Clock::Secs(f) => f(q),
    };
    let mut s = String::from("CPTAB 1\n");
    if matches!(clock, Clock::Secs(_)) {
        s += "unit s\n";
    }
    s += &format!("detect {detect}\n");
    for t in doc {
        s += &format!(
            "n {:.6} {:.6} {} {} {} {} {} {}\n",
            rel(t.qn0),
            rel(t.qn1),
            t.pitch,
            t.string,
            t.fret,
            t.vel,
            match t.tech {
                Tech::Plain => 0,
                Tech::Hammer => 1,
                Tech::Pull => 2,
                Tech::Slide => 3,
            },
            t.locked as u8
        );
        if !t.bend.is_empty() {
            s += "b";
            for (q, v) in &t.bend {
                // Depuis le debut de la note, dans l'unite du fichier.
                let d = match clock {
                    Clock::Qn(_) => *q,
                    Clock::Secs(_) => rel(t.qn0 + q) - rel(t.qn0),
                };
                s += &format!(" {d:.5} {v:.3}");
            }
            s += "\n";
        }
        if t.marks != 0 {
            s += &format!("m {}\n", t.marks);
        }
    }
    let mut sup: Vec<_> = suppressed.iter().collect();
    sup.sort();
    for &(q, p) in sup {
        match clock {
            Clock::Qn(at) => {
                let base = (at * 960.0).round() as i64;
                s += &format!("s {} {p}\n", q - base);
            }
            Clock::Secs(_) => s += &format!("s {:.6} {p}\n", rel(q as f64 / 960.0)),
        }
    }
    s
}

/// Le contraire de [`tab_to_text`] : (notes, effacees, reglages).
pub type TabFile = (Vec<TNote>, HashSet<(i64, i32)>, String);

pub fn tab_from_text(s: &str, at_qn: f64) -> Option<TabFile> {
    tab_from_text_on(s, at_qn, None)
}

/// [`tab_from_text`] ; `from_secs` (secondes depuis le debut de l'item ->
/// noires du projet) relit un fichier du jeu libre (« unit s ») ; sans
/// lui, un tel fichier ne se relit pas.
pub fn tab_from_text_on(
    s: &str,
    at_qn: f64,
    from_secs: Option<&dyn Fn(f64) -> f64>,
) -> Option<TabFile> {
    let mut lines = s.lines().peekable();
    if lines.next()?.trim() != "CPTAB 1" {
        return None;
    }
    let secs = lines.peek().map(|l| l.trim()) == Some("unit s");
    if secs {
        lines.next();
    }
    let from = match (secs, from_secs) {
        (true, None) => return None,
        (true, f) => f,
        (false, _) => None,
    };
    let abs = |x: f64| from.map_or(x + at_qn, |f| f(x));
    let base = (at_qn * 960.0).round() as i64;
    let mut doc = Vec::new();
    // Le debut de la note d'avant, dans l'unite du fichier (sa courbe).
    let mut last_x = 0.0;
    let mut sup = HashSet::new();
    let mut detect = String::new();
    for l in lines {
        if let Some(d) = l.strip_prefix("detect ") {
            detect = d.to_string();
            continue;
        }
        let v: Vec<&str> = l.split_whitespace().collect();
        match v.first() {
            Some(&"n") if v.len() == 9 => {
                let f = |i: usize| v[i].parse::<f64>().ok();
                let (Some(a), Some(b), Some(p), Some(st), Some(fr), Some(ve), Some(te), Some(lo)) =
                    (f(1), f(2), f(3), f(4), f(5), f(6), f(7), f(8))
                else {
                    continue;
                };
                last_x = a;
                doc.push(TNote {
                    qn0: abs(a),
                    qn1: abs(b),
                    pitch: p as i32,
                    string: st.max(0.0) as usize,
                    fret: fr as i32,
                    vel: ve.clamp(1.0, 127.0) as u8,
                    tech: match te as i32 {
                        1 => Tech::Hammer,
                        2 => Tech::Pull,
                        3 => Tech::Slide,
                        _ => Tech::Plain,
                    },
                    locked: lo != 0.0,
                    bend: Vec::new(),
                    marks: 0,
                });
            }
            // La courbe de la note d'avant.
            Some(&"b") => {
                if let Some(last) = doc.last_mut() {
                    let v: Vec<f64> = v[1..].iter().filter_map(|x| x.parse().ok()).collect();
                    let q0 = last.qn0;
                    last.bend = v
                        .chunks_exact(2)
                        .map(|c| {
                            let d = if from.is_some() {
                                abs(last_x + c[0]) - q0
                            } else {
                                c[0]
                            };
                            (d, c[1] as f32)
                        })
                        .collect();
                }
            }
            Some(&"s") if v.len() == 3 => {
                let Ok(p) = v[2].parse::<i32>() else {
                    continue;
                };
                if from.is_some() {
                    if let Ok(x) = v[1].parse::<f64>() {
                        sup.insert(((abs(x) * 960.0).round() as i64, p));
                    }
                } else if let Ok(q) = v[1].parse::<i64>() {
                    sup.insert((q + base, p));
                }
            }
            // Les techniques de jeu de la note d'avant.
            Some(&"m") if v.len() == 2 => {
                if let (Some(last), Ok(m)) = (doc.last_mut(), v[1].parse::<u8>()) {
                    last.marks = m;
                }
            }
            _ => {}
        }
    }
    sort(&mut doc);
    Some((doc, sup, detect))
}
