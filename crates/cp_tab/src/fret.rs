//! Le doigte : chaque note recoit sa corde et sa case.
//!
//! La meme note se joue a plusieurs endroits du manche. On choisit sur TOUTE
//! la phrase, pas note par note (un Viterbi sur les accords successifs) :
//! - un accord tient sur des cordes distinctes, dans une main (ecart de cases
//!   borne, et penalise au-dela de trois) ;
//! - la main bouge le moins possible d'un accord au suivant, et d'autant moins
//!   que les notes s'enchainent vite (un silence laisse le temps de changer
//!   de position) ;
//! - a egalite, le bas du manche ; une corde a vide ne coute rien.
//!
//! Ce ne sera pas forcement le doigte du guitariste d'origine : c'est un doigte
//! jouable, que Cedric corrige vite.

/// Un accordage, de la corde la plus grave a la plus aigue (MIDI).
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub strings: Vec<i32>,
    pub frets: i32,
}

impl Tuning {
    /// Mi La Re Sol Si Mi (E2 A2 D3 G3 B3 E4), 22 cases.
    pub fn standard() -> Tuning {
        Tuning {
            strings: vec![40, 45, 50, 55, 59, 64],
            frets: 22,
        }
    }

    /// La note la plus grave et la plus aigue jouables.
    pub fn range(&self) -> (i32, i32) {
        let lo = *self.strings.iter().min().unwrap_or(&40);
        let hi = *self.strings.iter().max().unwrap_or(&64) + self.frets;
        (lo, hi)
    }
}

/// Une note placee : corde (0 = la plus grave) et case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub string: usize,
    pub fret: i32,
}

/// Ramene une hauteur dans l'etendue de l'instrument, par octaves.
pub fn fold(pitch: i32, t: &Tuning) -> i32 {
    let (lo, hi) = t.range();
    let mut p = pitch;
    while p < lo {
        p += 12;
    }
    while p > hi {
        p -= 12;
    }
    p
}

/// Toutes les facons de jouer un accord (cordes distinctes, main d'au plus
/// cinq cases), avec leur cout propre.
fn candidates(pitches: &[i32], t: &Tuning) -> Vec<(Vec<Spot>, f64)> {
    let mut out = Vec::new();
    let mut cur: Vec<Spot> = Vec::with_capacity(pitches.len());
    fn rec(
        i: usize,
        pitches: &[i32],
        t: &Tuning,
        cur: &mut Vec<Spot>,
        out: &mut Vec<(Vec<Spot>, f64)>,
    ) {
        if out.len() > 4000 {
            return;
        }
        if i == pitches.len() {
            let fretted: Vec<i32> = cur.iter().filter(|s| s.fret > 0).map(|s| s.fret).collect();
            let span = match (fretted.iter().min(), fretted.iter().max()) {
                (Some(a), Some(b)) => b - a,
                _ => 0,
            };
            if span > 4 {
                return;
            }
            let mut cost = 0.0;
            for s in cur.iter() {
                cost += s.fret as f64 * 0.04;
            }
            if span > 3 {
                cost += 1.5;
            }
            out.push((cur.clone(), cost));
            return;
        }
        for (si, &open) in t.strings.iter().enumerate() {
            let fret = pitches[i] - open;
            if fret < 0 || fret > t.frets || cur.iter().any(|s| s.string == si) {
                continue;
            }
            cur.push(Spot { string: si, fret });
            rec(i + 1, pitches, t, cur, out);
            cur.pop();
        }
    }
    rec(0, pitches, t, &mut cur, &mut out);
    out
}

/// La position de la main : la plus basse case appuyee (None : tout a vide).
fn hand(spots: &[Spot]) -> Option<i32> {
    spots.iter().filter(|s| s.fret > 0).map(|s| s.fret).min()
}

/// Place une suite d'accords (`groups[i]` : les hauteurs qui commencent
/// ensemble ; `gaps[i]` : le temps en secondes depuis l'accord precedent).
/// Rend, pour chaque accord, une place par hauteur (dans l'ordre donne), ou
/// `None` pour une hauteur qui ne se joue pas (trop de notes, hors manche).
pub fn assign(groups: &[Vec<i32>], gaps: &[f64], t: &Tuning) -> Vec<Vec<Option<Spot>>> {
    assign_bonded(groups, gaps, &[], t)
}

/// Comme [`assign`] ; `bonds[i]` : l'accord `i` (une note seule) glisse
/// depuis le precedent (une note seule) et reste sur sa corde.
pub fn assign_bonded(
    groups: &[Vec<i32>],
    gaps: &[f64],
    bonds: &[bool],
    t: &Tuning,
) -> Vec<Vec<Option<Spot>>> {
    let n = groups.len();
    // Les candidats de chaque accord ; un accord injouable perd ses notes
    // les plus hautes jusqu'a l'etre.
    let mut cands: Vec<Vec<(Vec<Spot>, f64)>> = Vec::with_capacity(n);
    let mut kept: Vec<Vec<usize>> = Vec::with_capacity(n);
    for g in groups {
        let mut idx: Vec<usize> = (0..g.len()).collect();
        idx.sort_by_key(|&i| g[i]);
        idx.truncate(t.strings.len());
        loop {
            let ps: Vec<i32> = idx.iter().map(|&i| g[i]).collect();
            let c = candidates(&ps, t);
            if !c.is_empty() || idx.is_empty() {
                cands.push(c);
                kept.push(idx);
                break;
            }
            idx.pop();
        }
    }
    // Viterbi, sur les seuls accords jouables (un accord injouable ajoute
    // son temps a l'ecart suivant).
    let live: Vec<usize> = (0..n).filter(|&i| !cands[i].is_empty()).collect();
    let mut gap_live = Vec::with_capacity(live.len());
    let mut acc = 0.0;
    let mut li = 0;
    for i in 0..n {
        acc += gaps.get(i).copied().unwrap_or(0.0).max(0.0);
        if li < live.len() && live[li] == i {
            gap_live.push(acc);
            acc = 0.0;
            li += 1;
        }
    }
    let mut cost: Vec<Vec<f64>> = Vec::with_capacity(live.len());
    let mut back: Vec<Vec<usize>> = Vec::with_capacity(live.len());
    for (k, &i) in live.iter().enumerate() {
        let mut ci = Vec::with_capacity(cands[i].len());
        let mut bi = Vec::with_capacity(cands[i].len());
        for (spots, own) in &cands[i] {
            if k == 0 {
                // Debut de phrase : on prefere commencer bas.
                ci.push(own + hand(spots).unwrap_or(0) as f64 * 0.05);
                bi.push(0);
                continue;
            }
            let ease = 1.0 / (1.0 + 2.0 * gap_live[k]);
            let prev = live[k - 1];
            let (mut best, mut arg) = (f64::MAX, 0);
            for (j, (pspots, _)) in cands[prev].iter().enumerate() {
                let mv = match (hand(pspots), hand(spots)) {
                    (Some(a), Some(b)) => (a - b).abs() as f64,
                    _ => 0.0,
                };
                // Une meme note rejouee reste a sa place.
                let same = spots.iter().any(|s| pspots.contains(s)) as i32 as f64;
                // Une glisse ne change pas de corde.
                let bond = bonds.get(i).copied().unwrap_or(false)
                    && prev + 1 == i
                    && spots.len() == 1
                    && pspots.len() == 1
                    && spots[0].string != pspots[0].string;
                let c = cost[k - 1][j] + mv * ease - same * 0.3 + if bond { 6.0 } else { 0.0 };
                if c < best {
                    best = c;
                    arg = j;
                }
            }
            ci.push(best + own);
            bi.push(arg);
        }
        cost.push(ci);
        back.push(bi);
    }
    // Le chemin, de la fin au debut.
    let mut pick = vec![usize::MAX; n];
    if let Some(last) = cost.last() {
        let mut j = 0;
        let mut best = f64::MAX;
        for (jj, &c) in last.iter().enumerate() {
            if c < best {
                best = c;
                j = jj;
            }
        }
        for k in (0..live.len()).rev() {
            pick[live[k]] = j;
            j = back[k][j];
        }
    }
    groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut out = vec![None; g.len()];
            if pick[i] != usize::MAX {
                let spots = &cands[i][pick[i]].0;
                for (k, &gi) in kept[i].iter().enumerate() {
                    out[gi] = spots.get(k).copied();
                }
            }
            out
        })
        .collect()
}
