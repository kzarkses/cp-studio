//! Refaire le modele d'une piste depuis les liens que ses effets portent.
//!
//! Les fichiers de l'ancienne version sont perdus ; les liens natifs des
//! projets sont le seul endroit qui dit quels parametres suivent le pad (et
//! sur quel axe, avec quelle portee, autour de quelle base) et lesquels
//! suivent une source. A la premiere visite d'une piste, le module les lit et
//! appelle [`adopt`] : rien n'est relache, le son ne change pas.

use std::collections::BTreeMap;

use crate::links::{Chain, LSrc};
use crate::model::{PState, Src, Track, pkey};
use crate::slots::{FxKind, Pol, bank, bridge, mix, sg, source_center};

/// Un lien lu sur (`fx`, `param`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReadLink {
    pub fx: usize,
    pub param: usize,
    pub src: LSrc,
    pub offset: f64,
    pub scale: f64,
    pub baseline: f64,
}

/// Ce que vise la source d'un lien, en numero de source (0 = le pad :
/// `pad_param` dit alors `x_out` / `y_out` / `xy_mix`).
pub fn slot_of_source(chain: &Chain, src: LSrc) -> Option<(i32, Option<usize>)> {
    match src {
        LSrc::Midi { cc } => {
            let cc = if cc >= 128 { cc - 128 } else { cc };
            (16..=23)
                .contains(&cc)
                .then_some((101 + (cc as i32 - 16), None))
        }
        LSrc::Fx { fx, param } => match chain.kind(fx) {
            FxKind::Bank => (40..48)
                .contains(&param)
                .then_some((param as i32 - 39, None)),
            FxKind::Bridge => match param {
                bridge::X_OUT | bridge::Y_OUT | bridge::XY_MIX => Some((0, Some(param))),
                p if (bridge::MACRO_OUT..bridge::MACRO_OUT + 8).contains(&p) => {
                    Some((211 + (p - bridge::MACRO_OUT) as i32, None))
                }
                _ => None,
            },
            FxKind::SoundGen => sg::ENV_OUT
                .iter()
                .position(|&p| p == param)
                .map(|i| (201 + i as i32, None)),
            FxKind::Seq => crate::slots::seq::MOD_OUT
                .iter()
                .position(|&p| p == param)
                .map(|i| (221 + i as i32, None)),
            FxKind::Notes => (param < 4).then_some((231 + param as i32, None)),
            _ => None,
        },
    }
}

/// La polarite d'un lien d'apres son neutre, si elle differe de celle de la
/// source.
fn pol_of(center: f64, slot: i32, uni: bool) -> Option<Pol> {
    let own = source_center(slot, uni);
    if (center - own).abs() < 0.01 {
        None
    } else if center.abs() < 0.01 {
        Some(Pol::Uni)
    } else if (center - 0.5).abs() < 0.01 {
        Some(Pol::Bi)
    } else {
        None
    }
}

/// La source au bout d'un chemin de modulateurs de profondeur.
fn src_at<'a>(s: &'a mut Src, path: &[usize]) -> Option<&'a mut Src> {
    match path.split_first() {
        None => Some(s),
        Some((j, rest)) => src_at(s.dmods.get_mut(*j)?, rest),
    }
}

/// Le modele d'une piste depuis ses liens. `raw(fx, param)` lit une valeur
/// brute (les profondeurs et neutres du bus). Rend le nombre de parametres
/// adoptes.
pub fn adopt(
    t: &mut Track,
    chain: &Chain,
    links: &[ReadLink],
    raw: &dyn Fn(usize, usize) -> f64,
    uni: &[bool; 8],
    guni: &[bool; 8],
) -> usize {
    let g = if t.pad.gesture_range.abs() < 1e-9 {
        1.0
    } else {
        t.pad.gesture_range
    };
    let uni_of = |slot: i32| match slot {
        1..=8 => uni[(slot - 1) as usize],
        101..=108 => guni[(slot - 101) as usize],
        _ => false,
    };
    let mix_fx = chain.find(FxKind::Mix);
    // Les entrees des bus : (bus, entree) → (source, pastille du pad).
    let mut inputs: BTreeMap<(usize, usize), (i32, Option<usize>)> = BTreeMap::new();
    if let Some(m) = mix_fx {
        for l in links.iter().filter(|l| l.fx == m) {
            if (16..80).contains(&l.param) {
                let e = l.param - 16;
                if let Some(s) = slot_of_source(chain, l.src) {
                    inputs.insert((e / 4 + 1, e % 4 + 1), s);
                }
            }
        }
    }
    let mut anchors: Vec<(usize, f64)> = Vec::new();
    let mut n = 0;
    // Les entrees des bus : (bus, entree) → (cle du parametre, rang de la
    // source), pour y rattacher les voies de profondeur.
    let mut on_input: BTreeMap<(usize, usize), (String, usize)> = BTreeMap::new();
    for l in links {
        let kind = chain.kind(l.fx);
        // Les destinations : les effets de l'utilisateur, le Sound Gen, et
        // les reglages des effets internes (banque, macros du bridge,
        // sequenceur).
        let dest = match kind {
            FxKind::User | FxKind::SoundGen | FxKind::Seq => true,
            FxKind::Bank => (1..=8).any(|i| {
                [bank::rate(i), bank::phase(i), bank::curve(i), bank::slew(i)].contains(&l.param)
            }),
            FxKind::Bridge => (bridge::MACRO_IN..bridge::MACRO_IN + 8).contains(&l.param),
            _ => false,
        };
        if !dest {
            continue;
        }
        let Some(fxg) = chain.fx.get(l.fx).map(|f| f.guid.clone()) else {
            continue;
        };
        let mut ps = PState {
            sel: true,
            base: l.baseline.clamp(0.0, 1.0),
            range: (l.scale.abs() / g).min(1.0),
            inv: l.scale < 0.0,
            x: false,
            y: false,
            srcs: Vec::new(),
        };
        let on_bus = mix_fx.is_some_and(
            |m| matches!(l.src, LSrc::Fx { fx, param } if fx == m && param < mix::BUSES),
        );
        if on_bus {
            let LSrc::Fx { param: bout, .. } = l.src else {
                continue;
            };
            let b = bout + 1;
            let m = mix_fx.unwrap_or(0);
            let mut first = true;
            for k in 1..=mix::INPUTS {
                let Some(&(slot, pad_p)) = inputs.get(&(b, k)) else {
                    continue;
                };
                let amt = raw(m, mix::amt(b, k));
                let ctr = raw(m, mix::ctr(b, k));
                if slot == 0 {
                    // Le pad qui roule avec la source : son neutre est
                    // l'ancre.
                    let p = pad_p.unwrap_or(bridge::XY_MIX);
                    ps.x = p != bridge::Y_OUT;
                    ps.y = p != bridge::X_OUT;
                    anchors.push((p, ctr));
                    continue;
                }
                let mut s = Src::new(slot, amt / g);
                s.pol = pol_of(ctr, slot, uni_of(slot));
                if first {
                    ps.range = (amt.abs() / g).min(1.0);
                    ps.inv = amt < 0.0;
                    first = false;
                }
                on_input.insert((b, k), (pkey(&fxg, l.param), ps.srcs.len()));
                ps.srcs.push(s);
            }
            if ps.srcs.is_empty() && !(ps.x || ps.y) {
                continue;
            }
        } else {
            let Some((slot, pad_p)) = slot_of_source(chain, l.src) else {
                continue;
            };
            if slot == 0 {
                let p = pad_p.unwrap_or(bridge::XY_MIX);
                ps.x = p != bridge::Y_OUT;
                ps.y = p != bridge::X_OUT;
                anchors.push((p, -l.offset));
            } else {
                let mut s = Src::new(slot, ps.range * if ps.inv { -1.0 } else { 1.0 });
                s.pol = pol_of(-l.offset, slot, uni_of(slot));
                ps.srcs.push(s);
            }
        }
        t.params.insert(pkey(&fxg, l.param), ps);
        n += 1;
    }
    // Les voies de profondeur : de la plus haute a la plus basse (ce qui
    // module une voie a un numero plus bas qu'elle : sa cible est deja
    // placee).
    if let Some(m) = mix_fx {
        // Ou est la source que chaque voie module : (cle, rang, chemin).
        let mut place: BTreeMap<usize, (String, usize, Vec<usize>)> = BTreeMap::new();
        for d in (1..=mix::VOICES).rev() {
            let tgt = raw(m, mix::dm_tgt(d)).round() as i64;
            if tgt <= 0 {
                continue;
            }
            let Some(l) = links.iter().find(|l| l.fx == m && l.param == mix::dm_in(d)) else {
                continue;
            };
            let Some((slot, None)) = slot_of_source(chain, l.src) else {
                continue;
            };
            let amt = raw(m, mix::dm_amt(d));
            let ctr = raw(m, mix::dm_ctr(d));
            let parent = if tgt <= (mix::BUSES * mix::INPUTS) as i64 {
                let e = (tgt - 1) as usize;
                on_input
                    .get(&(e / mix::INPUTS + 1, e % mix::INPUTS + 1))
                    .map(|(k, r)| (k.clone(), *r, Vec::new()))
            } else {
                place
                    .get(&((tgt as usize) - mix::BUSES * mix::INPUTS))
                    .cloned()
            };
            let Some((key, rank, path)) = parent else {
                continue;
            };
            let Some(ps) = t.params.get_mut(&key) else {
                continue;
            };
            let Some(src) = ps.srcs.get_mut(rank).and_then(|s| src_at(s, &path)) else {
                continue;
            };
            let mut dm = Src::new(slot, amt / g);
            dm.pol = pol_of(ctr, slot, uni_of(slot));
            let mut p2 = path.clone();
            p2.push(src.dmods.len());
            src.dmods.push(dm);
            place.insert(d, (key, rank, p2));
        }
    }
    // L'ancre du pad : celle des liens du pad (X d'abord, puis Y, puis XY).
    for (p, a) in &anchors {
        match *p {
            bridge::X_OUT => t.pad.ax = a.clamp(0.0, 1.0),
            bridge::Y_OUT => t.pad.ay = a.clamp(0.0, 1.0),
            _ => {}
        }
    }
    if !anchors.iter().any(|(p, _)| *p != bridge::XY_MIX) {
        if let Some((_, a)) = anchors.first() {
            t.pad.ax = a.clamp(0.0, 1.0);
            t.pad.ay = a.clamp(0.0, 1.0);
        }
    }
    // Les axes du pad menes par une source.
    if let Some(bfx) = chain.find(FxKind::Bridge) {
        for l in links.iter().filter(|l| l.fx == bfx) {
            if let Some((slot, None)) = slot_of_source(chain, l.src) {
                match l.param {
                    bridge::X_POS => t.pad.src_x = slot,
                    bridge::Y_POS => t.pad.src_y = slot,
                    _ => {}
                }
            }
        }
    }
    let _ = bank::COUNT;
    n
}
