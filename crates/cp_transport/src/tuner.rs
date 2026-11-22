//! L'accordeur de CP Transport : la note, une aiguille sur ±50 centiemes,
//! l'ecart et la frequence. Il prend la place du compteur quand le
//! transport est a l'arret et qu'une note tenue arrive sur une piste armee
//! (mode « Clock »), ou a un emplacement a lui au centre de la barre (mode
//! « Slot »). Regle au clic droit sur la barre.

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color};

/// Ou vit l'accordeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunerMode {
    Off,
    /// A la place du compteur, a l'arret, quand une note arrive.
    Clock,
    /// Un emplacement a lui, au centre de la barre, toujours la.
    Slot,
}

impl TunerMode {
    pub fn key(self) -> &'static str {
        match self {
            TunerMode::Off => "off",
            TunerMode::Clock => "clock",
            TunerMode::Slot => "slot",
        }
    }

    pub fn from_key(s: &str) -> TunerMode {
        match s {
            "off" => TunerMode::Off,
            "slot" => TunerMode::Slot,
            _ => TunerMode::Clock,
        }
    }
}

/// Ce que l'accordeur montre.
#[derive(Clone, Debug, PartialEq)]
pub struct TunerView {
    /// « A », « C# ».
    pub note: String,
    pub octave: i32,
    /// L'ecart a la note, en centiemes (-50..50).
    pub cents: f32,
    pub freq: f32,
    /// « string 5 » (la corde a vide la plus proche), ou vide.
    pub string: String,
}

/// Les couleurs de l'accordeur (celles du theme).
#[derive(Clone, Copy, Debug)]
pub struct TunerInk {
    pub text: Color,
    pub dim: Color,
    /// Juste (a 3 centiemes pres).
    pub good: Color,
    /// Presque.
    pub warn: Color,
    pub bad: Color,
    pub track: Color,
}

/// La largeur que l'accordeur demande (mode « Slot »).
pub fn slot_width(s: f32) -> f32 {
    (300.0 * s).round()
}

/// Dessine l'accordeur dans `x, y, w, h` (son fond est deja pose). Sans note
/// (`None`) : l'invitation a jouer.
#[allow(clippy::too_many_arguments)]
pub fn draw_tuner(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    face: u16,
    s: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tv: Option<&TunerView>,
    ink: &TunerInk,
) {
    let pad = (8.0 * s).round();
    let big = Font::new(face, (18.0 * s) as u16, 700);
    let sub = Font::new(face, (10.0 * s) as u16, 500);
    let small = Font::new(face, (10.0 * s) as u16, 400);
    let mono = Font::new(1, (11.0 * s) as u16, 500);
    let note_w = (46.0 * s).round();
    let right_w = (62.0 * s).round();
    let bx0 = x + pad + note_w;
    let bx1 = x + w - pad - right_w;
    let cy = (y + h * 0.5).round();
    // La note.
    match tv {
        Some(t) => {
            let nw = text.measure(big, &t.note).ceil();
            text.draw(cv, big, x + pad, cy + 6.5 * s, &t.note, ink.text);
            text.draw(
                cv,
                sub,
                x + pad + nw + 1.0,
                cy + 9.0 * s,
                &t.octave.to_string(),
                ink.dim,
            );
        }
        None => {
            text.draw(cv, big, x + pad, cy + 6.5 * s, "\u{2014}", ink.dim);
        }
    }
    // L'echelle : -50 .. +50 centiemes, la zone juste au milieu.
    if bx1 > bx0 + 20.0 * s {
        let bw = bx1 - bx0;
        let xc = |c: f32| bx0 + (c.clamp(-50.0, 50.0) + 50.0) / 100.0 * bw;
        cv.fill_round_rect(bx0, cy - 1.5 * s, bw, 3.0 * s, 1.5 * s, ink.track);
        let z0 = xc(-3.0);
        cv.fill_round_rect(
            z0,
            cy - 4.0 * s,
            xc(3.0) - z0,
            8.0 * s,
            2.0 * s,
            ink.good.with_alpha(60),
        );
        for c in [-50.0f32, -25.0, 25.0, 50.0] {
            cv.fill_rect(xc(c).round(), cy - 4.0 * s, 1.0, 8.0 * s, ink.dim.fade(0.7));
        }
        cv.fill_rect(xc(0.0).round(), cy - 8.0 * s, 1.0, 16.0 * s, ink.dim);
        // Sans note : le tiret et l'echelle suffisent.
        if let Some(t) = tv {
            let a = t.cents.abs();
            let c = if a <= 3.0 {
                ink.good
            } else if a <= 15.0 {
                ink.warn
            } else {
                ink.bad
            };
            let nx = xc(t.cents);
            let nh = (h - 8.0 * s).max(8.0);
            cv.fill_round_rect(nx - 1.5 * s, cy - nh * 0.5, 3.0 * s, nh, 1.5 * s, c);
            // A droite : l'ecart, puis la frequence (ou la corde).
            let sign = if t.cents < -0.05 {
                "\u{2212}"
            } else if t.cents > 0.05 {
                "+"
            } else {
                ""
            };
            let ct = format!("{sign}{:.0} \u{a2}", t.cents.abs());
            text.draw_in(
                cv,
                mono,
                bx1 + pad * 0.5,
                y,
                right_w,
                h * 0.55,
                &ct,
                c,
                Align::Left,
            );
            let sub_t = if t.string.is_empty() {
                format!("{:.1} Hz", t.freq)
            } else {
                t.string.clone()
            };
            text.draw_in(
                cv,
                small,
                bx1 + pad * 0.5,
                y + h * 0.45,
                right_w,
                h * 0.55,
                &sub_t,
                ink.dim,
                Align::Left,
            );
        }
    }
}
