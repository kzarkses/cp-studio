//! Le pad hors du bridge : le mode granulaire (le seul vrai melange, entre
//! des points tires au hasard), le lissage du geste a la souris, et les
//! figures (pour dessiner leur trace). La marche et les figures elles-memes
//! tournent dans le @block du bridge.

use std::collections::BTreeMap;

use crate::model::{Grain, Pad};
use crate::roll::Rng;

/// Une grille de g × g grains ; chaque grain tient une valeur tiree pour
/// chaque parametre suivi, dans `gmin..gmax`.
pub fn init_grains(grid: u8, keys: &[String], pad: &Pad, rng: &mut Rng) -> Vec<Grain> {
    let g = grid.clamp(2, 4) as usize;
    let (lo, hi) = (pad.gmin.min(pad.gmax), pad.gmin.max(pad.gmax));
    let mut out = Vec::new();
    for j in 0..g {
        for i in 0..g {
            let values = keys
                .iter()
                .map(|k| (k.clone(), lo + rng.f() * (hi - lo)))
                .collect();
            out.push(Grain {
                x: (i as f64 + 0.5) / g as f64,
                y: (j as f64 + 0.5) / g as f64,
                values,
            });
        }
    }
    out
}

/// De nouvelles valeurs pour les grains (les places restent). Un parametre
/// suivi depuis est ajoute.
pub fn reroll_grains(grains: &mut [Grain], keys: &[String], pad: &Pad, rng: &mut Rng) {
    let (lo, hi) = (pad.gmin.min(pad.gmax), pad.gmin.max(pad.gmax));
    for g in grains.iter_mut() {
        g.values.clear();
        for k in keys {
            g.values.insert(k.clone(), lo + rng.f() * (hi - lo));
        }
    }
}

/// Les valeurs au point (x, y) : la moyenne des grains ponderee par
/// `max(0, 1 - distance / cellule)`. Un parametre qu'aucun grain proche ne
/// tient n'a pas de valeur.
pub fn granular_values(grains: &[Grain], grid: u8, x: f64, y: f64) -> BTreeMap<String, f64> {
    let cell = 1.0 / grid.clamp(2, 4) as f64;
    let mut sum: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for g in grains {
        let d = ((g.x - x).powi(2) + (g.y - y).powi(2)).sqrt();
        let w = (1.0 - d / cell).max(0.0);
        if w <= 0.0 {
            continue;
        }
        for (k, v) in &g.values {
            let e = sum.entry(k.clone()).or_insert((0.0, 0.0));
            e.0 += w * v;
            e.1 += w;
        }
    }
    sum.into_iter()
        .filter(|(_, (_, w))| *w > 0.0)
        .map(|(k, (s, w))| (k, (s / w).clamp(0.0, 1.0)))
        .collect()
}

/// Un pas du lissage : la position va vers la cible, vitesse bornee.
pub fn smooth_step(pos: (f64, f64), target: (f64, f64), pad: &Pad, dt: f64) -> (f64, f64) {
    if pad.smooth <= 0.0 {
        return target;
    }
    let (mut dx, mut dy) = (target.0 - pos.0, target.1 - pos.1);
    let d = (dx * dx + dy * dy).sqrt();
    if d < 0.001 {
        return target;
    }
    let lim = pad.max_speed * dt;
    if d > lim && lim > 0.0 {
        dx *= lim / d;
        dy *= lim / d;
    }
    // Le facteur est celui d'un pas de 1/30 s : rapporte au pas reel, le
    // rattrapage a la meme vitesse a 30, 60 ou 144 images par seconde.
    let k = pad.smooth.clamp(0.0, 1.0);
    let k = 1.0 - (1.0 - k).powf((dt * 30.0).max(0.0));
    (pos.0 + dx * k, pos.1 + dy * k)
}

/// Le point d'une figure a la phase `ph` (0..1), dans le carre unite (y vers
/// le haut), inscrit dans la zone.
pub fn figure_point(mode: u8, ph: f64, size: f64, zone: [f64; 4]) -> (f64, f64) {
    let a = ph.fract() * std::f64::consts::TAU;
    let h = size / 2.0;
    let k4 = (ph.fract() * 4.0).fract();
    let q = (ph.fract() * 4.0) as u32;
    let (x, y) = match mode {
        0 => (0.5 + h * a.cos(), 0.5 + h * a.sin()),
        1 => match q {
            0 => (0.5 - h + k4 * size, 0.5 - h),
            1 => (0.5 + h, 0.5 - h + k4 * size),
            2 => (0.5 + h - k4 * size, 0.5 + h),
            _ => (0.5 - h, 0.5 + h - k4 * size),
        },
        2 => {
            // Le triangle (un trace continu : l'ancien sautait deux fois).
            let k3 = (ph.fract() * 3.0).fract();
            let t = (ph.fract() * 3.0) as u32;
            let p = [(0.5, 0.5 + h), (0.5 - h, 0.5 - h), (0.5 + h, 0.5 - h)];
            let (a0, b0) = (p[t as usize % 3], p[(t as usize + 1) % 3]);
            (a0.0 + (b0.0 - a0.0) * k3, a0.1 + (b0.1 - a0.1) * k3)
        }
        3 => match q {
            0 => (0.5 - h + k4 * h, 0.5 + k4 * h),
            1 => (0.5 + k4 * h, 0.5 + h - k4 * h),
            2 => (0.5 + h - k4 * h, 0.5 - k4 * h),
            _ => (0.5 - k4 * h, 0.5 - h + k4 * h),
        },
        4 => match q {
            0 => (0.5 - h + k4 * size, 0.5 + h),
            1 => (0.5 + h - k4 * size, 0.5 + h - k4 * size),
            2 => (0.5 - h + k4 * size, 0.5 - h),
            _ => (0.5 + h - k4 * size, 0.5 - h + k4 * size),
        },
        _ => {
            let sc = 0.4 * size;
            let c = a.cos();
            let d = 1.0 + c * c;
            (0.5 + sc * a.sin() / d, 0.5 + sc * a.sin() * c / d)
        }
    };
    let (zx0, zx1, zy0, zy1) = (
        zone[0].min(zone[1]),
        zone[0].max(zone[1]),
        zone[2].min(zone[3]),
        zone[2].max(zone[3]),
    );
    (
        zx0 + x.clamp(0.0, 1.0) * (zx1 - zx0),
        zy0 + y.clamp(0.0, 1.0) * (zy1 - zy0),
    )
}

/// La valeur que le pad donne a un parametre (le chemin natif, sans
/// bornage) : `base + signe · (pos − ancre) · portee · G`.
pub fn pad_value(base: f64, span: f64, x: bool, y: bool, pad: &Pad) -> f64 {
    let dx = pad.x - pad.ax;
    let dy = pad.y - pad.ay;
    let d = match (x, y) {
        (true, true) => (dx + dy) / 2.0,
        (true, false) => dx,
        (false, true) => dy,
        _ => 0.0,
    };
    (base + d * span).clamp(0.0, 1.0)
}
