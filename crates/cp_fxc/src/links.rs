//! Les liens voulus d'une piste : ce que le modele demande a REAPER, calcule
//! sans REAPER. Le module lit la chaine, appelle [`plan`], puis n'ecrit que
//! ce qui differe (et relache les liens a nous qui ne sont plus voulus).
//!
//! Un lien natif (`param.N.plink.*`) donne au parametre :
//! `valeur = clamp01(baseline + (source + offset) * scale)`.
//!
//! - Une source seule : `offset = -neutre`, `scale = profondeur`,
//!   `baseline = base`.
//! - Le pad seul : la source est la sortie du bridge (`x_out`, `y_out` ou
//!   `xy_mix` selon les pastilles), `offset = -ancre`, `scale = portee`.
//! - Plusieurs sources (ou le pad avec une source) : un bus du Mod Mix fait
//!   la somme ; le parametre lit sa sortie (`offset -0.5`, `scale 2`) ; chaque
//!   entree du bus copie une source (lien `offset 0`, `scale 1`) et le bus
//!   tient sa profondeur et son neutre.
//! - Un axe du pad mene par une source : le lien va sur `x_pos` / `y_pos`,
//!   inscrit dans la zone (`scale = largeur`, `baseline = bord bas`).

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Track, split_pkey};
use crate::slots::{Band, FxKind, band_of, bank, bridge, mix, notes, seq, sg, source_center};

/// Un effet de la chaine, tel que le module l'a lu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainFx {
    pub guid: String,
    pub name: String,
    pub kind: Option<FxKind>,
    pub nparams: usize,
}

/// La chaine d'une piste (l'indice = la place dans la chaine).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chain {
    pub fx: Vec<ChainFx>,
}

impl Chain {
    pub fn kind(&self, i: usize) -> FxKind {
        self.fx.get(i).and_then(|f| f.kind).unwrap_or(FxKind::User)
    }
    pub fn find(&self, k: FxKind) -> Option<usize> {
        (0..self.fx.len()).find(|&i| self.kind(i) == k)
    }
    pub fn index_of(&self, guid: &str) -> Option<usize> {
        self.fx.iter().position(|f| f.guid == guid)
    }
    fn np(&self, i: usize) -> usize {
        self.fx.get(i).map_or(0, |f| f.nparams)
    }
}

/// Ce que le modele ne sait pas de lui-meme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ctx {
    /// Les slots unipolaires de la banque locale (curseur `lfoI_uni`).
    pub uni: [bool; 8],
    /// Ceux de la banque globale (CP MOD).
    pub guni: [bool; 8],
    /// Les bus deja tenus par un parametre (relu des liens : « le lien est
    /// la verite »).
    pub prev_bus: BTreeMap<String, usize>,
}

/// La source d'un lien.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LSrc {
    /// Un parametre d'un effet de la meme piste.
    Fx { fx: usize, param: usize },
    /// Un CC MIDI 14 bits (la banque globale).
    Midi { cc: u8 },
}

/// Un lien voulu sur (`fx`, `param`).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub fx: usize,
    pub param: usize,
    pub src: LSrc,
    pub offset: f64,
    pub scale: f64,
    pub baseline: f64,
}

/// Une valeur brute a ecrire (les profondeurs et neutres du bus).
#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub fx: usize,
    pub param: usize,
    pub value: f64,
}

/// Le plan d'une piste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub links: Vec<Link>,
    pub raws: Vec<Raw>,
    /// Le bus de chaque parametre qui en a un.
    pub bus_of: BTreeMap<String, usize>,
    /// Un bus est voulu et le Mod Mix manque (ou est trop court).
    pub mix_missing: bool,
    /// Le Mod Mix est la et rien ne le demande.
    pub mix_unused: bool,
    /// Plus de bus libre : des parametres jouent leur premiere source seule.
    pub mix_full: bool,
    pub notes_missing: bool,
    pub notes_unused: bool,
    /// Une source globale est voulue : la piste doit recevoir CP MOD.
    pub wants_global: bool,
    /// Un lien local est voulu et la banque manque.
    pub bank_missing: bool,
    /// Les axes du pad menes par une source (le fait, pas l'intention).
    pub pad_link_x: bool,
    pub pad_link_y: bool,
    /// Les parametres dont l'axe du pad attend (pas de place sur un bus).
    pub standby: BTreeSet<String>,
}

/// Ou lire une source, et son neutre.
struct Resolved {
    src: LSrc,
    center: f64,
}

fn resolve(slot: i32, chain: &Chain, ctx: &Ctx, plan: &mut Plan) -> Option<Resolved> {
    let b = band_of(slot)?;
    let center = |uni: bool| source_center(slot, uni);
    match b {
        Band::Lfo(n) => match chain.find(FxKind::Bank) {
            Some(fx) => Some(Resolved {
                src: LSrc::Fx {
                    fx,
                    param: bank::out(n as usize),
                },
                center: center(ctx.uni[(n - 1) as usize]),
            }),
            None => {
                plan.bank_missing = true;
                None
            }
        },
        Band::Global(n) => {
            plan.wants_global = true;
            Some(Resolved {
                src: LSrc::Midi {
                    cc: bank::cc(n as usize),
                },
                center: center(ctx.guni[(n - 1) as usize]),
            })
        }
        Band::Env(n) => {
            let fx = chain.find(FxKind::SoundGen)?;
            if n > 1 && chain.np(fx) < sg::EXTRA_COUNT {
                return None;
            }
            Some(Resolved {
                src: LSrc::Fx {
                    fx,
                    param: sg::ENV_OUT[(n - 1) as usize],
                },
                center: 0.0,
            })
        }
        Band::Macro(n) => {
            let fx = chain.find(FxKind::Bridge)?;
            if chain.np(fx) < 22 {
                return None;
            }
            Some(Resolved {
                src: LSrc::Fx {
                    fx,
                    param: bridge::MACRO_OUT + (n - 1) as usize,
                },
                center: 0.0,
            })
        }
        Band::Seq(n) => {
            let fx = chain.find(FxKind::Seq)?;
            let p = seq::MOD_OUT[(n - 1) as usize];
            (chain.np(fx) > p).then_some(Resolved {
                src: LSrc::Fx { fx, param: p },
                center: center(false),
            })
        }
        Band::Note(n) => match chain.find(FxKind::Notes) {
            Some(fx) => Some(Resolved {
                src: LSrc::Fx {
                    fx,
                    param: (n - 1) as usize,
                },
                center: center(false),
            }),
            None => {
                plan.notes_missing = true;
                None
            }
        },
        Band::PadX | Band::PadY => {
            let fx = chain.find(FxKind::Bridge)?;
            let p = if b == Band::PadX {
                bridge::X_OUT
            } else {
                bridge::Y_OUT
            };
            Some(Resolved {
                src: LSrc::Fx { fx, param: p },
                center: 0.5,
            })
        }
        Band::Pad => None,
    }
}

/// Le neutre d'un lien : sa polarite propre, sinon celle de la source.
fn link_center(r: &Resolved, pol: Option<crate::slots::Pol>) -> f64 {
    pol.map_or(r.center, |p| p.center())
}

/// La source du bridge et l'ancre pour des pastilles X / Y.
fn pad_source(x: bool, y: bool, ax: f64, ay: f64) -> Option<(usize, f64)> {
    match (x, y) {
        (true, true) => Some((bridge::XY_MIX, (ax + ay) / 2.0)),
        (true, false) => Some((bridge::X_OUT, ax)),
        (false, true) => Some((bridge::Y_OUT, ay)),
        _ => None,
    }
}

/// Le plan de la piste `t` sur la chaine `chain`.
pub fn plan(t: &Track, chain: &Chain, ctx: &Ctx) -> Plan {
    let mut p = Plan::default();
    let g = t.pad.gesture_range;
    let bridge_fx = chain
        .find(FxKind::Bridge)
        .filter(|&i| chain.np(i) >= bridge::COUNT);
    let mix_fx = chain.find(FxKind::Mix);
    let mix_ok = mix_fx.filter(|&i| chain.np(i) >= mix::COUNT);
    let want_pad = !t.pad.granular && bridge_fx.is_some();

    // Les parametres dont l'effet est sur la piste, dans l'ordre de la
    // chaine (un plan stable d'une passe a l'autre).
    let mut keys: Vec<(usize, usize, &String)> = t
        .params
        .iter()
        .filter(|(_, ps)| ps.sel)
        .filter_map(|(k, _)| {
            let (gd, i) = split_pkey(k)?;
            let fx = chain.index_of(gd)?;
            matches!(chain.kind(fx), FxKind::User | FxKind::SoundGen).then_some((fx, i, k))
        })
        .collect();
    keys.sort();

    // Qui veut un bus : plusieurs sources, ou le pad en plus d'une source.
    let pad_rides = |ps: &crate::model::PState| {
        want_pad && !ps.srcs.is_empty() && ps.srcs.len() < 4 && (ps.x || ps.y)
    };
    let mut want_bus: Vec<&String> = Vec::new();
    let mut riders: Vec<&String> = Vec::new();
    for (_, _, k) in &keys {
        let ps = &t.params[*k];
        if ps.srcs.len() >= 2 {
            want_bus.push(k);
        } else if pad_rides(ps) {
            riders.push(k);
        }
    }
    // Les bus : celui qu'un parametre tient deja reste a lui ; puis les
    // sources multiples ; puis le pad qui roule avec une source.
    let mut taken = [false; mix::BUSES + 1];
    let mut assign: BTreeMap<String, usize> = BTreeMap::new();
    let claimers: Vec<&String> = want_bus.iter().chain(riders.iter()).copied().collect();
    if mix_ok.is_some() {
        for k in &claimers {
            if let Some(&b) = ctx.prev_bus.get(*k) {
                if (1..=mix::BUSES).contains(&b) && !taken[b] {
                    taken[b] = true;
                    assign.insert((*k).clone(), b);
                }
            }
        }
    }
    for (i, k) in claimers.iter().enumerate() {
        if assign.contains_key(*k) {
            continue;
        }
        let rider = i >= want_bus.len();
        if mix_ok.is_none() {
            if !rider {
                p.mix_missing = true;
            }
            continue;
        }
        match (1..=mix::BUSES).find(|&b| !taken[b]) {
            Some(b) => {
                taken[b] = true;
                assign.insert((*k).clone(), b);
            }
            None => {
                if !rider {
                    p.mix_full = true;
                }
            }
        }
    }
    if mix_fx.is_some() && mix_ok.is_none() && !claimers.is_empty() {
        p.mix_missing = true;
    }

    for &(fx, param, k) in &keys {
        let ps = &t.params[k];
        if ps.srcs.is_empty() {
            // Le pad seul.
            if !want_pad {
                continue;
            }
            let Some(bfx) = bridge_fx else { continue };
            if let Some((bp, anchor)) = pad_source(ps.x, ps.y, t.pad.ax, t.pad.ay) {
                p.links.push(Link {
                    fx,
                    param,
                    src: LSrc::Fx { fx: bfx, param: bp },
                    offset: -anchor,
                    scale: ps.span(0, g),
                    baseline: ps.base,
                });
            }
            continue;
        }
        if let (Some(&b), Some(mfx)) = (assign.get(k), mix_ok) {
            // Le bus.
            p.bus_of.insert(k.clone(), b);
            p.links.push(Link {
                fx,
                param,
                src: LSrc::Fx {
                    fx: mfx,
                    param: mix::out(b),
                },
                offset: -0.5,
                scale: 2.0,
                baseline: ps.base,
            });
            let mut used = 0;
            for (i, s) in ps.srcs.iter().enumerate() {
                let kk = i + 1;
                match resolve(s.slot, chain, ctx, &mut p) {
                    Some(r) => {
                        p.links.push(Link {
                            fx: mfx,
                            param: mix::input(b, kk),
                            src: r.src,
                            offset: 0.0,
                            scale: 1.0,
                            baseline: 0.0,
                        });
                        p.raws.push(Raw {
                            fx: mfx,
                            param: mix::amt(b, kk),
                            value: ps.span_eff(i, g).clamp(-1.0, 1.0),
                        });
                        p.raws.push(Raw {
                            fx: mfx,
                            param: mix::ctr(b, kk),
                            value: link_center(&r, s.pol),
                        });
                    }
                    None => p.raws.push(Raw {
                        fx: mfx,
                        param: mix::amt(b, kk),
                        value: 0.0,
                    }),
                }
                used = kk;
            }
            if pad_rides(ps) {
                if let Some(bfx) = bridge_fx {
                    if let Some((bp, anchor)) = pad_source(ps.x, ps.y, t.pad.ax, t.pad.ay) {
                        let kk = used + 1;
                        p.links.push(Link {
                            fx: mfx,
                            param: mix::input(b, kk),
                            src: LSrc::Fx { fx: bfx, param: bp },
                            offset: 0.0,
                            scale: 1.0,
                            baseline: 0.0,
                        });
                        p.raws.push(Raw {
                            fx: mfx,
                            param: mix::amt(b, kk),
                            value: ps.span(0, g).clamp(-1.0, 1.0),
                        });
                        p.raws.push(Raw {
                            fx: mfx,
                            param: mix::ctr(b, kk),
                            value: anchor,
                        });
                        used = kk;
                    }
                }
            }
            for kk in used + 1..=mix::INPUTS {
                p.raws.push(Raw {
                    fx: mfx,
                    param: mix::amt(b, kk),
                    value: 0.0,
                });
            }
            continue;
        }
        // Une source seule (ou la premiere, faute de bus).
        if pad_rides(ps) {
            p.standby.insert(k.clone());
        }
        let s = &ps.srcs[0];
        if let Some(r) = resolve(s.slot, chain, ctx, &mut p) {
            p.links.push(Link {
                fx,
                param,
                src: r.src,
                offset: -link_center(&r, s.pol),
                scale: ps.span_eff(0, g),
                baseline: ps.base,
            });
        }
    }

    // Les bus que plus personne ne tient : a zero.
    if let Some(mfx) = mix_ok {
        for (b, &held) in taken.iter().enumerate().skip(1) {
            if !held {
                for kk in 1..=mix::INPUTS {
                    p.raws.push(Raw {
                        fx: mfx,
                        param: mix::amt(b, kk),
                        value: 0.0,
                    });
                }
            }
        }
    }
    p.mix_unused = mix_fx.is_some() && assign.is_empty() && !p.mix_missing;

    // Les axes du pad menes par une source.
    if let Some(bfx) = bridge_fx {
        let z = t.pad.zone_ordered();
        for (axis, slot) in [(0usize, t.pad.src_x), (1usize, t.pad.src_y)] {
            if slot == 0 || matches!(band_of(slot), Some(Band::PadX | Band::PadY) | None) {
                continue;
            }
            if let Some(r) = resolve(slot, chain, ctx, &mut p) {
                let (lo, hi) = if axis == 0 {
                    (z[0], z[1])
                } else {
                    (z[2], z[3])
                };
                p.links.push(Link {
                    fx: bfx,
                    param: if axis == 0 {
                        bridge::X_POS
                    } else {
                        bridge::Y_POS
                    },
                    src: r.src,
                    offset: 0.0,
                    scale: hi - lo,
                    baseline: lo,
                });
                if axis == 0 {
                    p.pad_link_x = true;
                } else {
                    p.pad_link_y = true;
                }
            }
        }
    }

    let notes_wanted = t
        .params
        .values()
        .filter(|ps| ps.sel)
        .flat_map(|ps| ps.srcs.iter())
        .any(|s| matches!(band_of(s.slot), Some(Band::Note(_))))
        || [t.pad.src_x, t.pad.src_y]
            .iter()
            .any(|&s| matches!(band_of(s), Some(Band::Note(_))));
    p.notes_unused = chain.find(FxKind::Notes).is_some() && !notes_wanted;
    if chain.find(FxKind::Notes).is_none() && notes_wanted {
        p.notes_missing = true;
    }
    let _ = notes::COUNT;
    p
}

/// La valeur qu'un lien donne a une source `v` (ce que REAPER calcule).
pub fn link_value(l: &Link, v: f64) -> f64 {
    (l.baseline + (v + l.offset) * l.scale).clamp(0.0, 1.0)
}

/// La fenetre que la modulation parcourt (pour l'anneau du bouton) :
/// (bas, haut), chacun borne a 0..1 ; une profondeur negative les croise.
pub fn mod_window(base: f64, depth: f64, center: f64) -> (f64, f64) {
    let lo = (base - center * depth).clamp(0.0, 1.0);
    let hi = (base + (1.0 - center) * depth).clamp(0.0, 1.0);
    (lo, hi)
}

/// Le genre d'un lien existant : a nous (on peut le relacher) ou a
/// l'utilisateur (jamais touche). `src_kind` est le genre de l'effet
/// source ; `msg2` le second octet d'un lien MIDI.
pub fn owned(src_kind: Option<FxKind>, midi_msg2: Option<i32>) -> bool {
    if let Some(m) = midi_msg2 {
        let cc = if m >= 128 { m - 128 } else { m };
        return (16..=23).contains(&cc);
    }
    matches!(
        src_kind,
        Some(
            FxKind::Bridge
                | FxKind::Bank
                | FxKind::Mix
                | FxKind::Notes
                | FxKind::Seq
                | FxKind::SoundGen
        )
    )
}
