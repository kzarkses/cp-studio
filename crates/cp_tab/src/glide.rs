//! Les bends et les glisses, lus dans la hauteur fine du reseau (le CONTOUR :
//! un tiers de demi-ton par case, trame par trame).
//!
//! Le reseau decoupe une corde qui change de hauteur sans etre repincee en
//! plusieurs notes, et y voit souvent une attaque. Ce qui les distingue est
//! le CHEMIN de la hauteur fine entre elles :
//! - un BEND monte d'un trait continu, lentement (un ton en 70 ms ou plus),
//!   d'au plus un ton et demi, et souvent redescend (le relachement) : la
//!   note d'arrivee (et celle du retour) rejoignent la note pincee, qui
//!   porte la courbe ;
//! - une GLISSE passe par les cases intermediaires, vite : la note d'arrivee
//!   est glissee depuis la precedente (meme corde) ;
//! - une GLISSE D'APPROCHE part de quelques cases plus bas (ou plus haut)
//!   juste avant que la note s'installe : la note commence au pincement et
//!   porte sa glisse ;
//! - un saut d'un coup (un marteau, une note repincee) n'est ni l'un ni
//!   l'autre : rien ne change.
//!
//! Les seuils viennent de cordes synthetiques ecoutees par le vrai reseau
//! (`synth_tests.rs`) : un bend d'un ton en 150 ms y monte d'un tiers de
//! demi-ton par trame, une glisse de cinq cases en 80 ms d'un demi-ton.

use crate::model::{CONTOUR, MIDI_OFFSET};
use crate::notes::Note;

/// Le contour du reseau en octets (0..255), trames x 264.
#[derive(Clone, Copy, Debug)]
pub struct Contour<'a> {
    pub frames: usize,
    pub v: &'a [u8],
}

/// Une note et sa facon d'etre jouee.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaped {
    pub note: Note,
    /// Le bend : (trames depuis le debut de la note, demi-tons au-dessus
    /// d'elle), quelques points ; vide : aucun.
    pub bend: Vec<(usize, f32)>,
    /// Glisse d'approche : de combien de demi-tons (negatif : d'en dessous) ;
    /// 0 : aucune. `approach` : ses trames, du pincement a l'arrivee.
    pub slide_in: i8,
    pub approach: usize,
    /// Arrivee glissee depuis la note precedente (meme corde).
    pub slid: bool,
    /// La hauteur ondule autour d'elle-meme (un vibrato).
    pub vibrato: bool,
}

/// La case d'une hauteur juste. MESUREE, pas la formule de Basic Pitch : un
/// la 440 pur tombe sur la case 145 = 3 x (69 - 21) + 1.
pub fn bin_of(p: f32) -> f32 {
    3.0 * (p - MIDI_OFFSET as f32) + 1.0
}

pub fn pitch_of(b: f32) -> f32 {
    (b - 1.0) / 3.0 + MIDI_OFFSET as f32
}

/// Un pic plus faible ne compte pas.
const FLOOR: f32 = 0.12;
/// Une voix suivie bouge d'au plus 4 cases (1 demi-ton 1/3) par trame.
const STEP: i32 = 4;
/// Un bend monte d'au plus un tiers de demi-ton par trame (29 demi-tons
/// par seconde) ; plus vite, c'est une glisse.
const BEND_RATE: f32 = 0.34;
/// Et d'au plus un ton et demi.
const BEND_MAX: i32 = 3;
/// Une courbe plus petite n'est pas un bend (un vibrato, une note un peu
/// haute).
const BEND_MIN: f32 = 0.4;

impl Contour<'_> {
    fn at(&self, t: usize, b: i32) -> f32 {
        if t >= self.frames || b < 0 || b >= CONTOUR as i32 {
            return 0.0;
        }
        self.v
            .get(t * CONTOUR + b as usize)
            .map_or(0.0, |&x| x as f32 / 255.0)
    }

    /// Le pic le plus proche de la case `b` (a `step` cases pres ; `strong` :
    /// le plus fort), affine par une parabole ; `None` s'il est trop faible.
    fn peak_by(&self, t: usize, b: f32, step: i32, strong: bool) -> Option<f32> {
        let c = b.round() as i32;
        let mut best: Option<(i32, f32, i32)> = None;
        for k in c - step..=c + step {
            let v = self.at(t, k);
            if v < FLOOR || v < self.at(t, k - 1) || v < self.at(t, k + 1) {
                continue;
            }
            let d = (k - c).abs();
            let better = match best {
                None => true,
                Some((_, bv, bd)) if strong => v > bv || (v == bv && d < bd),
                Some((_, bv, bd)) => d < bd || (d == bd && v > bv),
            };
            if better {
                best = Some((k, v, d));
            }
        }
        let (k, v, _) = best?;
        let (l, r) = (self.at(t, k - 1), self.at(t, k + 1));
        let den = l - 2.0 * v + r;
        let off = if den < -1e-6 {
            (0.5 * (l - r) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        Some(k as f32 + off)
    }

    fn peak(&self, t: usize, b: f32, step: i32) -> Option<f32> {
        self.peak_by(t, b, step, false)
    }

    /// Suit une voix de la trame `from` vers `to` (exclue ; en arriere si
    /// `to < from`), partant de la case `b`. S'arrete a deux trames sans pic.
    /// `strong` : d'une trame a l'autre, le pic le plus fort (une note qui
    /// glisse vers une autre : l'energie passe a la nouvelle hauteur pendant
    /// que l'ancienne s'eteint) ; sinon le plus proche. Rend (trame, case).
    fn follow_by(&self, from: usize, to: usize, b: f32, strong: bool) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        let mut pos = b;
        let mut miss = 0;
        let mut t = from as i64;
        let dir: i64 = if to >= from { 1 } else { -1 };
        while t != to as i64 && t >= 0 && (t as usize) < self.frames {
            match self.peak_by(t as usize, pos, STEP, strong) {
                Some(p) => {
                    pos = p;
                    miss = 0;
                    out.push((t as usize, p));
                }
                None => {
                    miss += 1;
                    if miss >= 2 {
                        break;
                    }
                }
            }
            t += dir;
        }
        out
    }

    fn follow(&self, from: usize, to: usize, b: f32) -> Vec<(usize, f32)> {
        self.follow_by(from, to, b, false)
    }

    /// La trame ou la note s'entend le mieux a sa hauteur (ses premieres
    /// trames), d'ou on la suit.
    fn anchor(&self, n: &Note) -> Option<usize> {
        let c = bin_of(n.pitch as f32);
        let last = (n.start + 8).min(n.end);
        (n.start..last)
            .filter_map(|t| {
                let p = self.peak(t, c, 1)?;
                ((p - c).abs() <= 1.0).then(|| (t, self.at(t, p.round() as i32)))
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|x| x.0)
    }
}

/// Une voix en cours : la note qui porte la courbe, la hauteur ou elle est
/// arrivee, sa fin.
struct Open {
    idx: usize,
    tail: i32,
}

/// Lit les bends et les glisses d'une liste de notes (celles de
/// [`crate::make_notes`]). Les notes absorbees par un bend disparaissent.
pub fn shape(notes: &[Note], c: &Contour) -> Vec<Shaped> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (notes[i].start, notes[i].pitch));
    let mut out: Vec<Shaped> = Vec::with_capacity(notes.len());
    // La fin de la courbe de chaque note (une glisse qui en part n'en fait
    // pas partie).
    let mut stop: Vec<usize> = Vec::with_capacity(notes.len());
    // Pour chaque note sortie : sa hauteur de queue (ou un bend l'a menee).
    let mut open: Vec<Open> = Vec::new();
    // Les notes deja suivies jusqu'a une suivante (une seule suite chacune).
    let mut taken: Vec<bool> = Vec::new();
    for &ni in &order {
        let b = &notes[ni];
        // La note qui y mene : finie pres du debut de celle-ci, sa voix suivie
        // jusqu'a elle.
        let mut linked: Option<(usize, Link)> = None;
        for (oi, o) in open.iter().enumerate() {
            if taken[oi] {
                continue;
            }
            let a = &out[o.idx].note;
            if a.end + 8 < b.start || a.end > b.start + 10 || o.tail == b.pitch {
                continue;
            }
            if (b.pitch - o.tail).abs() > 12 {
                continue;
            }
            if let Some(l) = link(c, a, o.tail, b) {
                let better = match &linked {
                    None => true,
                    Some((_, lb)) => l.dur < lb.dur,
                };
                if better {
                    linked = Some((oi, l));
                }
            }
        }
        if let Some((oi, l)) = linked {
            let base = out[open[oi].idx].note.pitch;
            let d = b.pitch - open[oi].tail;
            let up_from_base = b.pitch - base;
            let slow = l.rate <= BEND_RATE;
            // Un bend : lent, et la corde reste entre sa note et un ton et
            // demi au-dessus (en montant, ou en revenant).
            if slow && (0..=BEND_MAX).contains(&up_from_base) && d != 0 {
                let idx = open[oi].idx;
                out[idx].note.end = out[idx].note.end.max(b.end);
                open[oi].tail = b.pitch;
                continue;
            }
            if l.through > 0 {
                taken[oi] = true;
                let idx = open[oi].idx;
                // La note d'avant sonne jusqu'a l'arrivee ; sa courbe s'arrete
                // ou la glisse part.
                stop[idx] = l.left.min(stop[idx]);
                out[idx].note.end = b.start.max(out[idx].note.start + 1);
                out.push(Shaped {
                    note: b.clone(),
                    bend: Vec::new(),
                    slide_in: 0,
                    approach: 0,
                    slid: true,
                    vibrato: false,
                });
                stop.push(usize::MAX);
                open.push(Open {
                    idx: out.len() - 1,
                    tail: b.pitch,
                });
                taken.push(false);
                continue;
            }
            taken[oi] = true;
        }
        out.push(Shaped {
            note: b.clone(),
            bend: Vec::new(),
            slide_in: 0,
            approach: 0,
            slid: false,
            vibrato: false,
        });
        stop.push(usize::MAX);
        open.push(Open {
            idx: out.len() - 1,
            tail: b.pitch,
        });
        taken.push(false);
    }
    // Les courbes et les approches.
    for (s, &st) in out.iter_mut().zip(&stop) {
        (s.bend, s.vibrato) = curve(c, &s.note, st);
        if !s.slid {
            approach(c, s);
        }
    }
    out
}

/// Le chemin d'une voix d'une note a la suivante.
struct Link {
    /// La trame ou la voix quitte la note d'avant.
    left: usize,
    /// Trames entre le depart et l'arrivee.
    dur: usize,
    /// Trames passees strictement entre les deux hauteurs.
    through: usize,
    /// Demi-tons par trame.
    rate: f32,
}

fn link(c: &Contour, a: &Note, a_tail: i32, b: &Note) -> Option<Link> {
    // On part de la fin de la note d'avant, a sa hauteur de queue.
    let from = a.end.saturating_sub(4).max(a.start);
    let start_bin = c
        .peak(from, bin_of(a_tail as f32), 2)
        .filter(|p| (p - bin_of(a_tail as f32)).abs() <= 1.5)?;
    let path = c.follow_by(from, (b.start + 5).min(c.frames), start_bin, true);
    let (pa, pb) = (a_tail as f32, b.pitch as f32);
    let up = pb > pa;
    let mut left: Option<usize> = None;
    let mut reached: Option<usize> = None;
    let mut through = 0;
    let mut last = pa;
    for &(t, bin) in &path {
        let p = pitch_of(bin);
        // Un pas en arriere de plus d'un demi-ton : ce n'est pas la meme voix
        // qui va de l'une a l'autre.
        if (up && p < last - 0.5) || (!up && p > last + 0.5) {
            return None;
        }
        last = if up { last.max(p) } else { last.min(p) };
        if left.is_none() && (p - pa).abs() > 0.4 {
            left = Some(t);
        }
        if (p - pb).abs() <= 0.4 {
            reached = Some(t);
            break;
        }
        let between = if up {
            p > pa + 0.4 && p < pb - 0.4
        } else {
            p < pa - 0.4 && p > pb + 0.4
        };
        if between {
            through += 1;
        }
    }
    let reached = reached?;
    // L'arrivee doit tomber sur la note suivante, pas avant son debut de
    // beaucoup.
    if reached + 6 < b.start {
        return None;
    }
    let left = left.unwrap_or(reached);
    let dur = reached.saturating_sub(left).max(1);
    Some(Link {
        left,
        dur,
        through,
        rate: (pb - pa).abs() / dur as f32,
    })
}

/// La courbe de hauteur d'une note par rapport a elle (demi-tons), en
/// quelques points ; vide si elle reste sous [`BEND_MIN`].
/// La courbe d'une note (vide : juste), et si elle ondule (un vibrato).
fn curve(c: &Contour, n: &Note, stop: usize) -> (Vec<(usize, f32)>, bool) {
    let none = (Vec::new(), false);
    let Some(t0) = c.anchor(n) else {
        return none;
    };
    let base = n.pitch as f32;
    let b0 = bin_of(base);
    let Some(start) = c.peak(t0, b0, 1) else {
        return none;
    };
    let end = n.end.min(c.frames).min(stop.max(t0 + 1));
    let mut pts: Vec<(usize, f32)> = c.follow(t0, end, start);
    // Avant l'ancre : rien (l'attaque).
    if pts.len() < 3 {
        return none;
    }
    let semis: Vec<(usize, f32)> = pts
        .drain(..)
        .map(|(t, b)| (t - n.start, ((pitch_of(b) - base) * 10.0).round() / 10.0))
        .collect();
    // Lisse (mediane de trois) : un pic isole n'est pas un bend.
    let mut sm = semis.clone();
    for k in 1..semis.len().saturating_sub(1) {
        let mut w = [semis[k - 1].1, semis[k].1, semis[k + 1].1];
        w.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sm[k].1 = w[1];
    }
    // La note est juste : le gros de la courbe est autour de 0. Le decalage
    // moyen des premieres trames est retire (une corde un peu haute).
    let head: Vec<f32> = sm.iter().take(4).map(|x| x.1).collect();
    let off = head.iter().sum::<f32>() / head.len().max(1) as f32;
    let off = if off.abs() < 0.3 { off } else { 0.0 };
    for x in &mut sm {
        x.1 = (x.1 - off).clamp(-1.0, BEND_MAX as f32 + 1.0);
    }
    let top = sm.iter().map(|x| x.1).fold(0f32, f32::max);
    // Le vibrato se lit sur la hauteur non lissee (la mediane de trois
    // aplatit ses crêtes).
    let vib = is_vibrato(&semis);
    if top < BEND_MIN && !vib {
        return none;
    }
    // Depart a 0 au debut de la note.
    let mut v = vec![(0usize, 0f32)];
    v.extend(sm.iter().copied().filter(|x| x.0 > 0));
    // Un vibrato garde ses ondulations (le synthe les rejoue).
    (simplify(&v, if vib { 0.06 } else { 0.12 }), vib)
}

/// La hauteur ondule-t-elle (un vibrato) ? Autour de sa moyenne glissante
/// (une periode), au moins deux allers-retours reguliers de plus de 0,15
/// demi-ton de part et d'autre,
/// a 3,3..11 Hz (4 a 13 trames du reseau, 86 par seconde, par demi-periode).
pub fn is_vibrato(semis: &[(usize, f32)]) -> bool {
    // L'attaque d'abord : trois trames.
    let x: Vec<f32> = semis.iter().skip(3).map(|p| p.1).collect();
    if x.len() < 20 {
        return false;
    }
    let half = 7usize;
    let r: Vec<f32> = (0..x.len())
        .map(|k| {
            let (a, b) = (k.saturating_sub(half), (k + half + 1).min(x.len()));
            x[k] - x[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();
    // Les passages d'un cote a l'autre, avec une marge (le bruit ne compte
    // pas).
    const H: f32 = 0.15;
    let mut side = 0i8;
    let mut cross: Vec<usize> = Vec::new();
    let (mut lo, mut hi) = (0f32, 0f32);
    for (k, &v) in r.iter().enumerate() {
        lo = lo.min(v);
        hi = hi.max(v);
        let s = if v > H {
            1
        } else if v < -H {
            -1
        } else {
            0
        };
        if s != 0 && s != side {
            if side != 0 {
                cross.push(k);
            }
            side = s;
        }
    }
    if cross.len() < 4 || hi - lo > 3.0 {
        return false;
    }
    let p = (cross[cross.len() - 1] - cross[0]) as f32 / (cross.len() - 1) as f32;
    // Une onde reguliere : chaque demi-periode proche de la moyenne (le
    // bruit passe d'un cote a l'autre au hasard).
    let regular = cross
        .windows(2)
        .all(|w| (0.5 * p..=1.6 * p).contains(&((w[1] - w[0]) as f32)));
    regular && (4.0..=13.0).contains(&p)
}

/// Garde les points qui comptent (Douglas-Peucker, `tol` demi-tons).
pub fn simplify(v: &[(usize, f32)], tol: f32) -> Vec<(usize, f32)> {
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
        let (ta, pa) = (v[a].0 as f32, v[a].1);
        let (tb, pb) = (v[b].0 as f32, v[b].1);
        let (mut worst, mut wi) = (0f32, a);
        for (k, &(t, p)) in v.iter().enumerate().take(b).skip(a + 1) {
            let lin = if tb > ta {
                pa + (pb - pa) * (t as f32 - ta) / (tb - ta)
            } else {
                pa
            };
            let e = (p - lin).abs();
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

/// La glisse d'approche : la voix, suivie en arriere depuis la note, vient
/// de plus bas (ou de plus haut) juste avant. La note commence alors au
/// pincement.
fn approach(c: &Contour, s: &mut Shaped) {
    let n = &s.note;
    let Some(t0) = c.anchor(n) else {
        return;
    };
    let b0 = bin_of(n.pitch as f32);
    let Some(start) = c.peak(t0, b0, 1) else {
        return;
    };
    let back = c.follow(t0, n.start.saturating_sub(10), start);
    let Some(&(te, be)) = back.last() else {
        return;
    };
    let depth = pitch_of(be) - n.pitch as f32;
    if depth.abs() < 1.5 {
        return;
    }
    // Le chemin doit aller vers la note, sans revenir en arriere.
    let mut prev = depth;
    for &(_, b) in back.iter().rev() {
        let d = pitch_of(b) - n.pitch as f32;
        if d.abs() > prev.abs() + 0.5 {
            return;
        }
        prev = d;
    }
    s.slide_in = depth.round().clamp(-12.0, 12.0) as i8;
    s.approach = t0.saturating_sub(te).max(1);
    if te < s.note.start {
        // Les courbes sont relatives au debut : elles glissent d'autant.
        let shift = s.note.start - te;
        for p in &mut s.bend {
            p.0 += shift;
        }
        s.note.start = te;
    }
}

#[cfg(test)]
mod vib_tests {
    use super::is_vibrato;

    /// Une hauteur au fil des trames (86 par seconde).
    fn series(f: impl Fn(f32) -> f32, frames: usize) -> Vec<(usize, f32)> {
        (0..frames).map(|k| (k, f(k as f32 / 86.13))).collect()
    }

    #[test]
    fn le_vibrato_se_reconnait() {
        let tau = std::f32::consts::TAU;
        // 6 Hz, un demi-ton crete a crete, une demi-seconde.
        assert!(is_vibrato(&series(|t| 0.25 * (tau * 6.0 * t).sin(), 45)));
        // Un vibrato large et lent (4 Hz) sur un bend d'un ton.
        assert!(is_vibrato(&series(
            |t| 2.0 + 0.4 * (tau * 4.0 * t).sin(),
            60
        )));
        // Pas un vibrato : un bend qui monte, une note juste un peu bruitee,
        // un tremblement trop rapide, une note trop courte.
        assert!(!is_vibrato(&series(|t| (t * 8.0).min(2.0), 60)));
        assert!(!is_vibrato(&series(
            |t| 0.07 * ((t * 977.0).sin() + (t * 613.0).cos()),
            60
        )));
        assert!(!is_vibrato(&series(|t| 0.3 * (tau * 25.0 * t).sin(), 60)));
        assert!(!is_vibrato(&series(|t| 0.3 * (tau * 6.0 * t).sin(), 15)));
    }
}
