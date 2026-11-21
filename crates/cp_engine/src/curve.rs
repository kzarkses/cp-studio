//! La loi de courbe d'un segment, ecrite UNE fois (ancien `cp_curve.h`).
//!
//!   valeur = v0 + (v1 - v0) * shp(x, k),   shp(x, k) = x / (x + k (1 - x))
//!   k      = 8^(-c) si le segment monte, 8^(c) s'il descend ; c dans -1..1
//!
//! `k` se calcule a la validation d'un lot de points, jamais par echantillon.

#[inline]
pub fn shp(x: f32, k: f32) -> f32 {
    if k == 1.0 { x } else { x / (x + k * (1.0 - x)) }
}

pub fn factor(c: f32, rising: bool) -> f32 {
    let c = c.clamp(-1.0, 1.0);
    if c == 0.0 {
        return 1.0;
    }
    if rising {
        8f64.powf(-(c as f64)) as f32
    } else {
        8f64.powf(c as f64) as f32
    }
}
