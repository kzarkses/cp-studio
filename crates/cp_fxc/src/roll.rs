//! ROLL, le randomiseur : huit axes, chacun armé ou non, joués dans un ordre
//! fixe (P → MO → XY → N → B → R, puis le contournement et l'ordre de la
//! chaine). Ces fonctions changent le modele ; le module ecrit ensuite les
//! bases, les contournements et l'ordre dans REAPER.
//!
//! Aucun axe ne touche un effet DETACHED (le module ne passe que les
//! parametres et les effets du pool ROLL).

use crate::model::{Track, split_pkey};

/// Un tirage pseudo-aleatoire (xorshift64*), reproductible pour les tests.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0..1.
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// lo..=hi.
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as i64
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.int(0, i as i64) as usize;
            v.swap(i, j);
        }
    }
}

/// Les axes de ROLL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Params,
    Bases,
    Ranges,
    Xy,
    Invert,
    Bypass,
    Order,
    Mods,
}

/// L'ordre des pastilles a l'ecran.
pub const AXES: [Axis; 8] = [
    Axis::Params,
    Axis::Bases,
    Axis::Ranges,
    Axis::Xy,
    Axis::Invert,
    Axis::Bypass,
    Axis::Order,
    Axis::Mods,
];

impl Axis {
    pub fn chip(self) -> &'static str {
        match self {
            Axis::Params => "P",
            Axis::Bases => "B",
            Axis::Ranges => "R",
            Axis::Xy => "XY",
            Axis::Invert => "N",
            Axis::Bypass => "BY",
            Axis::Order => "OR",
            Axis::Mods => "MO",
        }
    }
    pub fn tip(self) -> &'static str {
        match self {
            Axis::Params => "Params: track a random set of params",
            Axis::Bases => "Bases: new base values",
            Axis::Ranges => "Ranges: new ranges",
            Axis::Xy => "X / Y: new pad axes",
            Axis::Invert => "Invert: flip at random",
            Axis::Bypass => "Bypass: bypass FX at random",
            Axis::Order => "Order: shuffle the chain",
            Axis::Mods => "Modulators: random CP LFO sources",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Axis::Params => "params",
            Axis::Bases => "bases",
            Axis::Ranges => "ranges",
            Axis::Xy => "xy",
            Axis::Invert => "invert",
            Axis::Bypass => "bypass",
            Axis::Order => "order",
            Axis::Mods => "mods",
        }
    }
    pub fn from_key(k: &str) -> Option<Axis> {
        AXES.iter().copied().find(|a| a.key() == k)
    }
}

/// Les reglages de ROLL (des preferences, pour toute la machine).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Les axes armes (dans l'ordre de [`AXES`]).
    pub armed: [bool; 8],
    pub random_min: usize,
    pub random_max: usize,
    pub bypass_pct: f64,
    pub mod_prob: f64,
    pub exclusive_xy: bool,
    pub range_min: f64,
    pub range_max: f64,
    pub intensity: f64,
    pub base_min: f64,
    pub base_max: f64,
    /// Les slots de la banque que MO tire (1..8).
    pub lfo_slots: usize,
    pub lfo_shape: bool,
    pub lfo_rate: bool,
    pub lfo_sync: bool,
    pub lfo_phase: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            armed: [false, true, true, true, true, true, true, false],
            random_min: 3,
            random_max: 8,
            bypass_pct: 0.3,
            mod_prob: 0.0,
            exclusive_xy: true,
            range_min: 0.0,
            range_max: 1.0,
            intensity: 0.3,
            base_min: 0.0,
            base_max: 1.0,
            lfo_slots: 8,
            lfo_shape: true,
            lfo_rate: true,
            lfo_sync: false,
            lfo_phase: false,
        }
    }
}

impl Settings {
    pub fn is_armed(&self, a: Axis) -> bool {
        AXES.iter()
            .position(|&x| x == a)
            .is_some_and(|i| self.armed[i])
    }
    pub fn armed_count(&self) -> usize {
        self.armed.iter().filter(|&&a| a).count()
    }
}

/// P : de nouveaux parametres suivis. `pool` = tous les parametres des
/// effets du pool ROLL (cles du modele).
pub fn select(t: &mut Track, pool: &[String], s: &Settings, rng: &mut Rng) {
    for k in pool {
        t.param_mut(k).sel = false;
    }
    if pool.is_empty() {
        return;
    }
    let hi = s.random_max.min(pool.len()).max(1);
    let lo = s.random_min.min(hi).max(1);
    let n = rng.int(lo as i64, hi as i64) as usize;
    let mut v: Vec<&String> = pool.iter().collect();
    rng.shuffle(&mut v);
    for k in v.into_iter().take(n) {
        t.param_mut(k).sel = true;
    }
}

/// Un parametre suivi qui appartient au pool.
fn tracked<'a>(t: &'a Track, pool: &'a [String]) -> impl Iterator<Item = &'a String> + 'a {
    pool.iter()
        .filter(move |k| t.params.get(*k).is_some_and(|p| p.sel))
}

/// XY : de nouveaux axes pour les parametres suivis sans source.
pub fn xy(t: &mut Track, pool: &[String], exclusive: bool, rng: &mut Rng) {
    let keys: Vec<String> = tracked(t, pool).cloned().collect();
    for k in keys {
        let p = t.param_mut(&k);
        if !p.srcs.is_empty() {
            continue;
        }
        if exclusive {
            let x = rng.chance(0.5);
            p.x = x;
            p.y = !x;
        } else {
            match rng.int(0, 2) {
                0 => (p.x, p.y) = (true, false),
                1 => (p.x, p.y) = (false, true),
                _ => (p.x, p.y) = (true, true),
            }
        }
    }
}

/// N : l'inversion tiree a pile ou face.
pub fn invert(t: &mut Track, pool: &[String], rng: &mut Rng) {
    let keys: Vec<String> = tracked(t, pool).cloned().collect();
    for k in keys {
        t.param_mut(&k).inv = rng.chance(0.5);
    }
}

/// B : de nouvelles bases, autour du centre de l'etendue. Rend les bases
/// changees (cle, valeur) pour que le module les ecrive.
pub fn bases(t: &mut Track, pool: &[String], s: &Settings, rng: &mut Rng) -> Vec<(String, f64)> {
    let (lo, hi) = (s.base_min.min(s.base_max), s.base_min.max(s.base_max));
    let c = (lo + hi) / 2.0;
    let h = (hi - lo) / 2.0;
    let keys: Vec<String> = tracked(t, pool).cloned().collect();
    let mut out = Vec::new();
    for k in keys {
        let v = (c + (rng.f() * 2.0 - 1.0) * h * s.intensity).clamp(lo, hi);
        t.param_mut(&k).base = v;
        out.push((k, v));
    }
    out
}

/// Les bases d'un seul effet, tirees sur toute la course (le verbe de
/// l'en-tete d'un effet).
pub fn bases_uniform(t: &mut Track, keys: &[String], rng: &mut Rng) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for k in keys {
        if t.params.get(k).is_some_and(|p| p.sel) {
            let v = rng.f();
            t.param_mut(k).base = v;
            out.push((k.clone(), v));
        }
    }
    out
}

/// R : de nouvelles portees.
pub fn ranges(t: &mut Track, pool: &[String], s: &Settings, rng: &mut Rng) {
    let (lo, hi) = (s.range_min.min(s.range_max), s.range_min.max(s.range_max));
    let keys: Vec<String> = tracked(t, pool).cloned().collect();
    for k in keys {
        t.param_mut(&k).range = lo + rng.f() * (hi - lo);
    }
}

/// MO : une source de la banque locale pour certains parametres suivis ;
/// les autres reviennent au pad. Rend vrai si la banque est voulue.
pub fn mods(t: &mut Track, pool: &[String], s: &Settings, rng: &mut Rng) -> bool {
    if s.mod_prob <= 0.0 {
        return false;
    }
    let n = s.lfo_slots.clamp(1, 8) as i64;
    let keys: Vec<String> = tracked(t, pool).cloned().collect();
    let mut any = false;
    for k in keys {
        let p = t.param_mut(&k);
        if rng.chance(s.mod_prob) {
            let slot = rng.int(1, n) as i32;
            let d = p.range * if p.inv { -1.0 } else { 1.0 };
            p.srcs = vec![crate::model::Src::new(slot, d)];
            any = true;
        } else {
            p.srcs.clear();
        }
    }
    any
}

/// Le reglage tire d'un slot de la banque (MO).
#[derive(Clone, Debug, PartialEq)]
pub struct SlotRoll {
    pub shape: Option<u8>,
    /// Hz (libre).
    pub rate: Option<f64>,
    pub sync: Option<u8>,
    pub phase: Option<f64>,
}

pub fn roll_slot(s: &Settings, rng: &mut Rng) -> SlotRoll {
    let shape = s.lfo_shape.then(|| rng.int(0, 4) as u8);
    let free_rate = |rng: &mut Rng| (rng.f() * (10f64.ln() - 0.05f64.ln()) + 0.05f64.ln()).exp();
    let (rate, sync) = match (s.lfo_rate, s.lfo_sync) {
        (true, true) => {
            if rng.chance(0.5) {
                (Some(free_rate(rng)), Some(0))
            } else {
                (None, Some(rng.int(1, 6) as u8))
            }
        }
        (true, false) => (Some(free_rate(rng)), Some(0)),
        (false, true) => (None, Some(rng.int(0, 6) as u8)),
        _ => (None, None),
    };
    SlotRoll {
        shape,
        rate,
        sync,
        phase: s.lfo_phase.then(|| rng.f()),
    }
}

/// BY : quels effets du pool restent allumes.
pub fn bypass(n: usize, pct: f64, rng: &mut Rng) -> Vec<bool> {
    (0..n).map(|_| !rng.chance(pct)).collect()
}

/// OR : un nouvel ordre pour les places mobiles (une permutation de
/// 0..n).
pub fn order(n: usize, rng: &mut Rng) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    rng.shuffle(&mut v);
    v
}

/// « Track at random » d'un effet : 1..max parametres.
pub fn select_fx(t: &mut Track, keys: &[String], max: usize, rng: &mut Rng) {
    for k in keys {
        t.param_mut(k).sel = false;
    }
    if keys.is_empty() {
        return;
    }
    let n = rng.int(1, max.clamp(1, keys.len()) as i64) as usize;
    let mut v: Vec<&String> = keys.iter().collect();
    rng.shuffle(&mut v);
    for k in v.into_iter().take(n) {
        t.param_mut(k).sel = true;
    }
}

/// Les cles d'un effet parmi une liste.
pub fn keys_of_fx(keys: &[String], fx_guid: &str) -> Vec<String> {
    keys.iter()
        .filter(|k| split_pkey(k).is_some_and(|(g, _)| g == fx_guid))
        .cloned()
        .collect()
}
