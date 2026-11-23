//! La palette du clic droit : des icones au curseur, par groupe, a la
//! place d'un long menu.
//!
//! Une ligne de groupes (choisir, editer, temps, velocite, accords, arpeges,
//! rythmes euclidiens, bends, plus) ; le groupe survole ouvre ses icones
//! dessous, sans clic. Une icone survolee montre son resultat en
//! pointilles sur les notes avant le clic ; une icone qui ne changerait rien
//! est eteinte. Un clic applique (un pas d'annulation) et ferme ; Maj+clic
//! garde la palette ouverte. La molette sur un rythme euclidien change ses
//! coups (Maj : ses cases). Le dernier groupe utilise s'ouvre d'office.
//!
//! Les icones sont des traces SVG (grille de 24, comme les icones Lucide),
//! dessines a chaque image : un trait, un remplissage, un petit texte.

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Path};
use cp_roll::{CHORDS, Note};
use cp_ui::Theme;
use cp_ui::svg::{Xf, path_data};

use crate::{Clip, Ed, HBox, Hit, Midi};

/// Un groupe de la palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cat {
    Select,
    Edit,
    Time,
    Vel,
    Chord,
    #[default]
    Arp,
    Euclid,
    Bend,
    More,
}

impl Cat {
    pub const ALL: [Cat; 9] = [
        Cat::Select,
        Cat::Edit,
        Cat::Time,
        Cat::Vel,
        Cat::Chord,
        Cat::Arp,
        Cat::Euclid,
        Cat::Bend,
        Cat::More,
    ];

    /// Un trait separe les familles.
    fn group(self) -> u8 {
        match self {
            Cat::Select => 0,
            Cat::Edit | Cat::Time | Cat::Vel => 1,
            Cat::Chord | Cat::Arp | Cat::Euclid => 2,
            Cat::Bend => 3,
            Cat::More => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Cat::Select => "Select",
            Cat::Edit => "Edit and length",
            Cat::Time => "Timing",
            Cat::Vel => "Velocity",
            Cat::Chord => "Chords",
            Cat::Arp => "Arpeggios",
            Cat::Euclid => "Euclidean rhythms",
            Cat::Bend => "Bends and slides",
            Cat::More => "More",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Cat::Select => icon(
                "M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z",
                "",
            ),
            Cat::Edit => icon(
                &format!(
                    "{} {} M8.12 8.12 12 12 M20 4 8.12 15.88 M14.8 14.8 20 20",
                    circ(6.0, 6.0, 3.0),
                    circ(6.0, 18.0, 3.0)
                ),
                "",
            ),
            Cat::Time => icon(&format!("M10 2h4 M12 14l3-3 {}", circ(12.0, 14.0, 8.0)), ""),
            Cat::Vel => icon("M3 3v16a2 2 0 0 0 2 2h16 M18 17V9 M13 17V5 M8 17v-3", ""),
            Cat::Chord => icon(
                "M11.6 17.5V3",
                &dots(&[(9.0, 17.5), (9.0, 12.4), (9.0, 7.3)], 2.6),
            ),
            Cat::Arp => icon(
                "",
                &dots(&[(4.0, 19.0), (9.3, 14.3), (14.7, 9.7), (20.0, 5.0)], 2.2),
            ),
            Cat::Euclid => euclid_glyph(3, 8),
            Cat::Bend => icon(BEND, ""),
            Cat::More => icon("", &dots(&[(5.0, 12.0), (12.0, 12.0), (19.0, 12.0)], 1.9)),
        }
    }
}

/// Un dessin de la palette.
#[derive(Clone, Debug, PartialEq)]
pub enum Glyph {
    /// Un trace (`s`), un remplissage (`f`) en donnees SVG sur une grille
    /// de 24, et un petit texte en haut a droite.
    Icon { s: String, f: String, t: String },
    /// Un mot sur le bouton.
    Chip(String),
    /// Un mot devant des pastilles (pas un bouton).
    Label(&'static str),
}

fn icon(s: &str, f: &str) -> Glyph {
    Glyph::Icon {
        s: s.into(),
        f: f.into(),
        t: String::new(),
    }
}

fn icon_t(s: &str, f: &str, t: &str) -> Glyph {
    Glyph::Icon {
        s: s.into(),
        f: f.into(),
        t: t.into(),
    }
}

/// Un cercle en donnees SVG.
fn circ(cx: f32, cy: f32, r: f32) -> String {
    format!(
        "M{} {}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0",
        cx - r,
        cy,
        2.0 * r,
        -2.0 * r
    )
}

fn dots(pts: &[(f32, f32)], r: f32) -> String {
    pts.iter()
        .map(|&(x, y)| circ(x, y, r))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Les coups d'un rythme euclidien (le premier sur le temps).
pub fn euclid_hits(pulses: usize, steps: usize) -> Vec<bool> {
    let steps = steps.max(1);
    let pulses = pulses.min(steps);
    let mut bucket = steps - pulses;
    (0..steps)
        .map(|_| {
            bucket += pulses;
            if bucket >= steps {
                bucket -= steps;
                true
            } else {
                false
            }
        })
        .collect()
}

/// Un rythme euclidien en icone : ses cases en rond, les coups pleins.
fn euclid_glyph(pulses: usize, steps: usize) -> Glyph {
    let hits = euclid_hits(pulses, steps);
    let n = hits.len();
    // Lisibles a 17 px : les coups pleins et gros, les cases vides en points.
    let (r_hit, r_off) = if n > 12 { (2.5, 0.8) } else { (3.0, 1.1) };
    let f: Vec<String> = hits
        .iter()
        .enumerate()
        .map(|(j, &h)| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * j as f32 / n as f32;
            circ(
                12.0 + 8.8 * a.cos(),
                12.0 + 8.8 * a.sin(),
                if h { r_hit } else { r_off },
            )
        })
        .collect();
    icon("", &f.join(" "))
}

/// Le bend des icones : une courbe qui monte, et sa fleche.
const BEND: &str = "M3 18h6c3.5 0 5-3 5.6-9 M12 11.2l2.6-3 2.6 3";

/// Une operation de la palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    /// Son numero (celui de [`Ed::transform`]) ; 0 : un mot, pas un bouton.
    pub id: u32,
    pub cat: Cat,
    /// Un trait separe les groupes d'un meme groupe de la palette.
    pub group: u8,
    pub label: String,
    pub glyph: Glyph,
    /// Un reglage (coche ou non) : il ne ferme pas la palette.
    pub setting: Option<bool>,
}

/// Les bends de la palette : (numero, demi-tons au sommet).
pub(crate) const BEND_OPS: [(u32, f32); 3] = [(100, 1.0), (101, 2.0), (102, 3.0)];

/// La palette ouverte.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    /// Ou le clic l'a ouverte.
    pub x: f32,
    pub y: f32,
    pub cat: Cat,
    /// Les operations qui changeraient quelque chose.
    pub ok: Vec<u32>,
    /// L'operation survolee et ce qu'elle donnerait.
    pub preview: Option<(u32, Midi)>,
    /// Le cadre dessine : x, y, w, h.
    pub rect: (f32, f32, f32, f32),
}

/// Les tailles (a 96 DPI) : un bouton, l'ecart, la place d'un trait, le
/// bord, l'ecart entre les deux lignes.
const BTN: f32 = 26.0;
const GAP: f32 = 2.0;
const SEP: f32 = 9.0;
const PAD: f32 = 4.0;
const ROW_GAP: f32 = 5.0;

/// Deux etats de notes donnent-ils la meme chose (notes, choix, bends) ?
pub(crate) fn same(a: &Midi, b: &Midi) -> bool {
    a.roll.notes == b.roll.notes && a.expr.carry(&a.roll.notes) == b.expr.carry(&b.roll.notes)
}

/// La courbe d'un bend de la palette sur une note de `len` noires a `bpm` :
/// (noires depuis son debut, demi-tons).
pub(crate) fn bend_curve(id: u32, len: f64, bpm: f64) -> Vec<(f64, f32)> {
    let len = len.max(1.0 / 64.0);
    let t0 = (len * 0.1).min(1.0 / 16.0);
    let rise = (len * 0.35).min(0.25);
    let up = |amt: f32, out: &mut Vec<(f64, f32)>| {
        out.push((0.0, 0.0));
        out.push((t0, 0.0));
        for k in 1..=6 {
            let u = k as f64 / 6.0;
            let v = 1.0 - (1.0 - u) * (1.0 - u);
            out.push((t0 + rise * u, amt * v as f32));
        }
    };
    let mut v = Vec::new();
    match id {
        100..=102 => {
            let amt = BEND_OPS[(id - 100) as usize].1;
            up(amt, &mut v);
        }
        // Monte d'un ton, tient, redescend.
        103 => {
            up(2.0, &mut v);
            let hold = (t0 + rise).max(len * 0.55);
            let down = rise.min((len * 0.95 - hold).max(1.0 / 64.0));
            for k in 1..=6 {
                let u = k as f64 / 6.0;
                v.push((hold + down * u, 2.0 * (1.0 - u * u) as f32));
            }
        }
        // Le vibrato : 5,5 Hz, un quart de ton de part et d'autre, apres un
        // huitieme de la note.
        104 => {
            let per = bpm.max(1.0) / 60.0 / 5.5;
            let q = per / 4.0;
            v.push((0.0, 0.0));
            let mut t = (len / 8.0).max(q);
            let mut k = 0;
            while t < len - 1e-6 {
                v.push((t, [0.25f32, 0.0, -0.25, 0.0][k % 4]));
                t += q;
                k += 1;
            }
        }
        // La glisse d'approche : un ton en dessous (au-dessus), un tic avant
        // la note, puis la note.
        105 | 106 => {
            let from = if id == 105 { -2.0 } else { 2.0 };
            let into = (len * 0.3).min(0.125);
            v.push((-1.0 / 960.0, from));
            for k in 1..=6 {
                let u = k as f64 / 6.0;
                v.push((into * u, from * (1.0 - u) as f32 * (1.0 - u) as f32));
            }
        }
        _ => {}
    }
    v
}

impl Ed {
    /// Les operations de la palette, dans l'ordre, groupe par groupe.
    pub fn palette_ops(&self) -> Vec<Op> {
        let mut v: Vec<Op> = Vec::new();
        let mut add = |cat: Cat, group: u8, id: u32, label: String, glyph: Glyph| {
            v.push(Op {
                id,
                cat,
                group,
                label,
                glyph,
                setting: None,
            });
        };
        use Cat::*;
        add(
            Select,
            0,
            1,
            "Select all".into(),
            icon(
                "M3 5h18v14H3z",
                "M6 8h5v2.5H6z M10 11h6v2.5h-6z M13 14.5h5V17h-5z",
            ),
        );
        add(
            Select,
            0,
            2,
            "Invert selection".into(),
            icon("M3 5h18v14H3z M12 5v14", "M3 5h9v14H3z"),
        );
        add(
            Select,
            0,
            3,
            "Select same pitch".into(),
            icon(
                "M2 7.5h20 M2 16.5h20",
                "M3 10.75h4.5v2.5H3z M9.75 10.75h4.5v2.5h-4.5z M16.5 10.75H21v2.5h-4.5z",
            ),
        );
        add(
            Edit,
            0,
            10,
            "Legato".into(),
            icon("", "M3 7h9v3H3z M12 11h6v3h-6z M18 15h3v3h-3z"),
        );
        add(
            Edit,
            0,
            13,
            "Glue".into(),
            icon("M12 6v3 M12 15v3", "M3 10.5h18v3H3z"),
        );
        add(
            Edit,
            0,
            14,
            "Split at edit cursor".into(),
            icon("M12 5v14", "M3 10.5h7.5v3H3z M13.5 10.5H21v3h-7.5z"),
        );
        add(
            Edit,
            0,
            11,
            "Reverse".into(),
            icon("M8 3 4 7l4 4 M4 7h16 M16 21l4-4-4-4 M20 17H4", ""),
        );
        add(
            Edit,
            0,
            12,
            "Mirror pitches".into(),
            icon(
                "M17 3l-5 5-5-5z M17 21l-5-5-5 5z M3 12h2 M8 12h2 M14 12h2 M19 12h2",
                "",
            ),
        );
        add(
            Edit,
            0,
            15,
            if self.scale_of().is_some() {
                "Snap to scale".into()
            } else {
                "Snap to scale (no scale set)".into()
            },
            icon(
                "M3 5h18v14H3z M9 13v6 M15 13v6",
                "M7.5 5h3v8h-3z M13.5 5h3v8h-3z",
            ),
        );
        add(
            Edit,
            1,
            50,
            "Double length".into(),
            Glyph::Chip("\u{00d7}2".into()),
        );
        add(
            Edit,
            1,
            51,
            "Halve length".into(),
            Glyph::Chip("\u{00f7}2".into()),
        );
        add(
            Edit,
            1,
            52,
            format!("Length = grid ({})", crate::grid_name(self.grid)),
            icon("M4 5v14 M10 5v14 M16 5v14", "M4 10.5h6v3H4z"),
        );
        let grid_dots = |pts: &[(f32, f32)]| icon("M5 4v16 M12 4v16 M19 4v16", &dots(pts, 2.1));
        add(
            Time,
            0,
            54,
            format!(
                "Quantize \u{00b7} {} % \u{00b7} swing {} %",
                (self.strength * 100.0).round(),
                (self.swing * 100.0).round()
            ),
            grid_dots(&[(5.0, 12.0), (12.0, 12.0), (19.0, 12.0)]),
        );
        add(
            Time,
            1,
            30,
            "Humanize light".into(),
            grid_dots(&[(5.9, 11.0), (11.2, 13.0), (19.7, 11.6)]),
        );
        add(
            Time,
            1,
            31,
            "Humanize medium".into(),
            grid_dots(&[(6.9, 10.0), (10.4, 14.2), (20.6, 11.0)]),
        );
        add(
            Time,
            1,
            32,
            "Humanize heavy".into(),
            grid_dots(&[(8.0, 8.6), (9.4, 15.6), (21.2, 10.2)]),
        );
        add(Time, 2, 0, String::new(), Glyph::Label("Q%"));
        for (k, p) in [100, 75, 50, 25].into_iter().enumerate() {
            add(
                Time,
                2,
                40 + k as u32,
                format!("Quantize strength {p} %"),
                Glyph::Chip(p.to_string()),
            );
        }
        add(Time, 3, 0, String::new(), Glyph::Label("Sw%"));
        for (k, p) in [0, 8, 16, 24].into_iter().enumerate() {
            add(
                Time,
                3,
                44 + k as u32,
                format!("Swing {p} %"),
                Glyph::Chip(p.to_string()),
            );
        }
        for (k, val) in [127, 100, 80, 64].into_iter().enumerate() {
            add(
                Vel,
                0,
                20 + k as u32,
                format!("Velocity {val}"),
                Glyph::Chip(val.to_string()),
            );
        }
        add(
            Vel,
            1,
            24,
            "Velocity ramp up".into(),
            icon(
                "",
                "M3 16h3v4H3z M8 12h3v8H8z M13 8h3v12h-3z M18 4h3v16h-3z",
            ),
        );
        add(
            Vel,
            1,
            25,
            "Velocity ramp down".into(),
            icon(
                "",
                "M3 4h3v16H3z M8 8h3v12H8z M13 12h3v8h-3z M18 16h3v4h-3z",
            ),
        );
        add(
            Vel,
            1,
            26,
            "Velocity compress".into(),
            icon(
                "M12 22v-6 M12 8V2 M4 12H2 M10 12H8 M16 12h-2 M22 12h-2 M15 19l-3-3-3 3 M15 5l-3 3-3-3",
                "",
            ),
        );
        add(
            Vel,
            1,
            27,
            "Velocity expand".into(),
            icon(
                "M12 22v-6 M12 8V2 M4 12H2 M10 12H8 M16 12h-2 M22 12h-2 M15 19l-3 3-3-3 M15 5l-3-3-3 3",
                "",
            ),
        );
        let short = ["Tri", "M", "m", "7", "M7", "m7", "sus4", "5", "8ve"];
        for (k, (name, _)) in CHORDS.iter().enumerate() {
            let label = if *name == "Triad" {
                if self.scale_of().is_some() {
                    "Chord: triad in the scale".to_string()
                } else {
                    "Chord: triad in the scale (major without a scale)".to_string()
                }
            } else {
                format!("Chord: {name}")
            };
            add(
                Chord,
                u8::from(k > 0),
                70 + k as u32,
                label,
                Glyph::Chip(short.get(k).copied().unwrap_or("?").into()),
            );
        }
        let g = crate::grid_name(if self.grid > 0.0 { self.grid } else { 0.25 });
        for (id, label, pts, r) in [
            (
                80u32,
                format!("Arpeggio up (one note per {g})"),
                vec![(4.0, 19.0), (9.3, 14.3), (14.7, 9.7), (20.0, 5.0)],
                2.2f32,
            ),
            (
                81,
                "Arpeggio down".into(),
                vec![(4.0, 5.0), (9.3, 9.7), (14.7, 14.3), (20.0, 19.0)],
                2.2,
            ),
            (
                82,
                "Arpeggio up-down".into(),
                vec![
                    (3.0, 18.0),
                    (7.5, 12.0),
                    (12.0, 6.0),
                    (16.5, 12.0),
                    (21.0, 18.0),
                ],
                2.1,
            ),
            (
                83,
                "Arpeggio random".into(),
                vec![(4.0, 12.0), (9.0, 18.5), (14.5, 5.5), (20.0, 14.0)],
                2.2,
            ),
            (
                84,
                "Arpeggio up, 2 octaves".into(),
                vec![
                    (3.0, 20.5),
                    (6.6, 17.5),
                    (10.2, 14.5),
                    (13.8, 11.5),
                    (17.4, 8.5),
                    (21.0, 5.5),
                ],
                1.7,
            ),
        ] {
            add(Arp, 0, id, label, icon("", &dots(&pts, r)));
        }
        for (k, &(p, n)) in self.euclid.iter().enumerate() {
            add(
                Euclid,
                0,
                90 + k as u32,
                format!("Euclid {p}/{n} \u{00b7} wheel: hits, Shift+wheel: steps"),
                euclid_glyph(p, n),
            );
        }
        let range = self.bend_range();
        for (id, amt, t, label) in [
            (100u32, 1.0f32, "\u{00bd}", "Bend \u{00bd} tone"),
            (101, 2.0, "full", "Bend full tone"),
            (102, 3.0, "1\u{00bd}", "Bend 1\u{00bd} tones"),
        ] {
            let label = if amt > range + 1e-3 {
                format!("{label} (the pitch bend range is \u{00b1}{range} st)")
            } else {
                label.to_string()
            };
            add(Bend, 0, id, label, icon_t(BEND, "", t));
        }
        add(
            Bend,
            0,
            103,
            "Bend and release".into(),
            icon("M2 18h4c2.5 0 3-9 6-9s3.5 9 6 9h4", ""),
        );
        add(
            Bend,
            1,
            104,
            "Vibrato".into(),
            icon("M2 12q2.5-6 5 0t5 0t5 0t5 0", ""),
        );
        add(
            Bend,
            1,
            105,
            "Slide in from below".into(),
            icon("M3 20 13 10", &circ(16.5, 7.5, 3.0)),
        );
        add(
            Bend,
            1,
            106,
            "Slide in from above".into(),
            icon("M3 4 13 14", &circ(16.5, 16.5, 3.0)),
        );
        add(
            Bend,
            2,
            107,
            "Remove bend".into(),
            icon(&format!("{BEND} M4 4l16 16"), ""),
        );
        add(
            More,
            0,
            61,
            "Step input from MIDI keyboard".into(),
            icon(
                "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M6 8h4 M14 8h.01 M18 8h.01 M2 12h20 M6 12v4 M10 12v4 M14 12v4 M18 12v4",
                "",
            ),
        );
        add(
            More,
            0,
            60,
            "Open in REAPER's MIDI editor".into(),
            icon(
                "M15 3h6v6 M10 14 21 3 M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
                "",
            ),
        );
        // Les reglages : coches.
        for o in &mut v {
            o.setting = match o.id {
                40..=43 => Some(
                    (self.strength - [1.0, 0.75, 0.5, 0.25][(o.id - 40) as usize]).abs() < 1e-6,
                ),
                44..=47 => {
                    Some((self.swing - [0.0, 0.08, 0.16, 0.24][(o.id - 44) as usize]).abs() < 1e-6)
                }
                61 => Some(self.step_input),
                _ => None,
            };
        }
        v
    }

    /// L'etendue des pitch bends (demi-tons) du canal des notes choisies.
    pub(crate) fn bend_range(&self) -> f32 {
        let Some(m) = self.midi() else { return 2.0 };
        let chan = m
            .roll
            .notes
            .iter()
            .find(|n| n.sel)
            .map_or(self.last_chan, |n| n.chan);
        m.expr.range(chan)
    }

    /// Ouvre la palette la ou l'on a clique (le dernier groupe utilise).
    pub fn open_palette(&mut self, x: f32, y: f32) {
        if !matches!(self.clip, Clip::Midi(_)) {
            return;
        }
        self.palette = Some(Palette {
            x,
            y,
            cat: self.pal_last,
            ..Palette::default()
        });
        self.pal_refresh();
        self.dirty = true;
    }

    pub fn close_palette(&mut self) {
        if self.palette.take().is_some() {
            self.dirty = true;
        }
    }

    /// Ce que ferait l'operation `id` sur les notes d'aujourd'hui (rien si
    /// elle ne change rien).
    pub(crate) fn pal_result(&self, id: u32) -> Option<Midi> {
        let m = self.midi()?;
        let mut t = m.clone();
        self.op_on(id, &mut t, self.gen_seed.wrapping_add(1));
        (!same(m, &t)).then_some(t)
    }

    /// Les operations qui changeraient quelque chose ; l'apercu se refait.
    pub(crate) fn pal_refresh(&mut self) {
        let range = self.bend_range();
        let ids: Vec<(u32, bool)> = self
            .palette_ops()
            .iter()
            .map(|o| {
                let ok = match o.id {
                    0 => false,
                    // Les reglages et les actions sans notes.
                    40..=47 | 60 | 61 => true,
                    100..=102 if BEND_OPS[(o.id - 100) as usize].1 > range + 1e-3 => false,
                    id => self.pal_result(id).is_some(),
                };
                (o.id, ok)
            })
            .collect();
        if let Some(p) = &mut self.palette {
            p.ok = ids.into_iter().filter(|x| x.1).map(|x| x.0).collect();
            p.preview = None;
        }
    }

    /// Survoler l'operation `id` : son resultat en apercu.
    pub(crate) fn pal_hover(&mut self, id: Option<u32>) {
        let cur = self
            .palette
            .as_ref()
            .and_then(|p| p.preview.as_ref().map(|x| x.0));
        if cur == id {
            return;
        }
        let pv = id
            .filter(|&i| !matches!(i, 0 | 40..=47 | 60 | 61))
            .filter(|i| self.palette.as_ref().is_some_and(|p| p.ok.contains(i)))
            .and_then(|i| self.pal_result(i).map(|m| (i, m)));
        if let Some(p) = &mut self.palette {
            p.preview = pv;
            self.dirty = true;
        }
    }

    /// Un clic dans la palette. Maj garde la palette ouverte.
    pub(crate) fn pal_click(&mut self, h: Hit, shift: bool) -> Vec<crate::Act> {
        let Some(cat) = self.palette.as_ref().map(|p| p.cat) else {
            return Vec::new();
        };
        match h {
            Hit::PalCat(k) => {
                if let Some(p) = &mut self.palette {
                    p.cat = Cat::ALL[k as usize % Cat::ALL.len()];
                    p.preview = None;
                }
                self.dirty = true;
                Vec::new()
            }
            Hit::PalOp(id) => {
                let ok = self.palette.as_ref().is_some_and(|p| p.ok.contains(&id));
                if !ok {
                    return Vec::new();
                }
                self.pal_last = cat;
                let setting = matches!(id, 40..=47 | 61);
                let out = if id == 54 {
                    self.quantize()
                } else {
                    self.transform(id)
                };
                if setting || shift {
                    self.pal_refresh();
                } else {
                    self.close_palette();
                }
                self.dirty = true;
                out
            }
            _ => Vec::new(),
        }
    }

    /// La molette sur un rythme euclidien : ses coups (Maj : ses cases).
    pub(crate) fn pal_wheel(&mut self, id: u32, up: bool, shift: bool) {
        let k = (id - 90) as usize;
        let Some(e) = self.euclid.get_mut(k) else {
            return;
        };
        let d: i64 = if up { 1 } else { -1 };
        if shift {
            e.1 = (e.1 as i64 + d).clamp(e.0.max(2) as i64, 32) as usize;
        } else {
            e.0 = (e.0 as i64 + d).clamp(1, e.1 as i64) as usize;
        }
        self.pal_refresh();
        if let Some(p) = &mut self.palette {
            p.preview = None;
        }
        self.pal_hover(Some(id));
    }

    /// Le cadre de la palette et la place de chaque bouton : (cadre,
    /// [(x, y, w, h, cible, operation)]).
    #[allow(clippy::type_complexity)]
    fn pal_layout(
        &self,
        text: &mut TextCache,
        th: &Theme,
    ) -> Option<(
        (f32, f32, f32, f32),
        Vec<(f32, f32, f32, f32, Hit, Option<Op>)>,
    )> {
        let p = self.palette.as_ref()?;
        let s = |v: f32| th.s(v);
        let fc = Font::new(0, s(11.0) as u16, 600);
        let mut cells: Vec<(f32, f32, f32, f32, Hit, Option<Op>)> = Vec::new();
        let row = |items: Vec<(u8, Hit, Option<Op>, f32)>,
                   y: f32,
                   cells: &mut Vec<(f32, f32, f32, f32, Hit, Option<Op>)>|
         -> f32 {
            let mut x = s(PAD);
            let mut last: Option<u8> = None;
            for (g, hit, op, w) in items {
                if last.is_some_and(|l| l != g) {
                    cells.push((
                        x + s(SEP) * 0.5 - s(GAP),
                        y,
                        0.0,
                        s(BTN),
                        Hit::Palette,
                        None,
                    ));
                    x += s(SEP);
                }
                last = Some(g);
                cells.push((x, y, w, s(BTN), hit, op));
                x += w + s(GAP);
            }
            x - s(GAP) + s(PAD)
        };
        let cats: Vec<(u8, Hit, Option<Op>, f32)> = Cat::ALL
            .iter()
            .enumerate()
            .map(|(k, c)| (c.group(), Hit::PalCat(k as u8), None, s(BTN)))
            .collect();
        let w1 = row(cats, s(PAD), &mut cells);
        let y2 = s(PAD + BTN + ROW_GAP);
        let ops: Vec<(u8, Hit, Option<Op>, f32)> = self
            .palette_ops()
            .into_iter()
            .filter(|o| o.cat == p.cat)
            .map(|o| {
                let w = match &o.glyph {
                    Glyph::Chip(t) => (text.measure(fc, t).ceil() + s(12.0)).max(s(BTN)),
                    Glyph::Label(t) => text.measure(fc, t).ceil() + s(4.0),
                    Glyph::Icon { .. } => s(BTN),
                };
                let hit = if o.id == 0 {
                    Hit::Palette
                } else {
                    Hit::PalOp(o.id)
                };
                (o.group, hit, Some(o), w)
            })
            .collect();
        let w2 = row(ops, y2, &mut cells);
        let (w, h) = (w1.max(w2), y2 + s(BTN + PAD));
        let l = self.lay;
        let mut x = p.x + s(4.0);
        let mut y = p.y + s(10.0);
        x = x.min(l.w - w - s(4.0)).max(s(2.0));
        if y + h > l.h - s(4.0) {
            y = (p.y - h - s(8.0)).max(l.bar_h + s(2.0));
        }
        let cells = cells
            .into_iter()
            .map(|c| (c.0 + x, c.1 + y, c.2, c.3, c.4, c.5))
            .collect();
        Some(((x.round(), y.round(), w.round(), h.round()), cells))
    }

    /// Dessine la palette, par-dessus tout, et retient ses cibles.
    pub(crate) fn draw_palette(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let Some((r, cells)) = self.pal_layout(text, th) else {
            return;
        };
        let s = |v: f32| th.s(v);
        let Some(p) = self.palette.as_ref() else {
            return;
        };
        let (x, y, w, h) = r;
        cv.fill_round_rect(
            x + s(2.0),
            y + s(4.0),
            w,
            h,
            s(5.0),
            cp_gfx::Color::BLACK.with_alpha(90),
        );
        cv.fill_round_rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, s(5.0), th.seam);
        cv.fill_round_rect(x, y, w, h, s(4.0), th.panel);
        // Le trait entre les groupes et leurs icones.
        let y_sep = y + s(PAD + BTN) + s(ROW_GAP) * 0.5;
        cv.fill_rect(x + s(3.0), y_sep.round(), w - s(6.0), 1.0, th.seam);
        bx.push((x, y, w, h, Hit::Palette));
        let fc = Font::new(0, s(11.0) as u16, 600);
        let ft = Font::new(0, s(8.0) as u16, 700);
        for (cx, cy, cw, ch, hit, op) in cells {
            if cw <= 0.0 {
                // Un trait entre deux groupes.
                cv.fill_rect(cx.round(), cy + s(4.0), 1.0, ch - s(8.0), th.seam);
                continue;
            }
            let hov = self.hover == hit;
            let (open, ok, on) = match (&hit, &op) {
                (Hit::PalCat(k), _) => {
                    let c = Cat::ALL[*k as usize];
                    let any = self
                        .palette_ops()
                        .iter()
                        .any(|o| o.cat == c && p.ok.contains(&o.id));
                    (p.cat == c, any, false)
                }
                (Hit::PalOp(id), Some(o)) => (false, p.ok.contains(id), o.setting == Some(true)),
                _ => (false, true, false),
            };
            if let Some(Op {
                glyph: Glyph::Label(t),
                ..
            }) = &op
            {
                text.draw_in(cv, fc, cx, cy, cw, ch, t, th.text_faint, Align::Center);
                continue;
            }
            let bg = if on {
                Some(th.accent.with_alpha(80))
            } else if open {
                Some(th.accent.with_alpha(55))
            } else if hov && ok {
                Some(th.hover)
            } else {
                None
            };
            if let Some(c) = bg {
                cv.fill_round_rect(cx, cy, cw, ch, s(3.0), c);
            }
            let fg = if !ok {
                th.text_faint.fade(0.7)
            } else if on || open || hov {
                th.text
            } else {
                th.text_dim
            };
            bx.push((cx, cy, cw, ch, hit));
            let glyph = match (&hit, &op) {
                (Hit::PalCat(k), _) => Cat::ALL[*k as usize].glyph(),
                (_, Some(o)) => o.glyph.clone(),
                _ => continue,
            };
            match glyph {
                Glyph::Chip(t) => {
                    text.draw_in(cv, fc, cx, cy, cw, ch, &t, fg, Align::Center);
                }
                Glyph::Icon { s: st, f, t } => {
                    let size = s(17.0);
                    let k = size / 24.0;
                    let xf = Xf {
                        k,
                        ox: (cx + (cw - size) * 0.5).round(),
                        oy: (cy + (ch - size) * 0.5).round(),
                        grid: None,
                    };
                    if !st.is_empty() {
                        let mut pth = Path::new();
                        path_data(&st, xf, &mut pth);
                        cv.stroke_path(&pth, (1.75 * k).max(1.0), fg);
                    }
                    if !f.is_empty() {
                        let mut pth = Path::new();
                        path_data(&f, xf, &mut pth);
                        cv.fill_path(&pth, fg);
                    }
                    if !t.is_empty() {
                        text.draw_in(
                            cv,
                            ft,
                            cx,
                            cy - s(1.0),
                            cw - s(1.5),
                            s(10.0),
                            &t,
                            fg,
                            Align::Right,
                        );
                    }
                }
                Glyph::Label(_) => {}
            }
        }
        if let Some(pp) = &mut self.palette {
            pp.rect = r;
        }
    }
}

/// Les poignees du bend d'une note : son pied (ou il part) et son sommet,
/// (noire, demi-tons) ; le pied manque quand la courbe part du sommet (une
/// glisse d'approche).
/// Le pied (s'il y en a un) et le sommet d'un bend.
pub(crate) type Handles<T> = (Option<(T, f32)>, (T, f32));

pub(crate) fn bend_handles(curve: &[(f64, f32)]) -> Option<Handles<f64>> {
    if curve.len() < 2 {
        return None;
    }
    // La derniere valeur est celle de la fin (rajoutee par `curve`).
    let body = &curve[..curve.len() - 1];
    let (pi, peak) = body
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.abs().total_cmp(&b.1.1.abs()))?;
    if peak.1.abs() < 0.05 {
        return None;
    }
    let foot = (0..pi)
        .rev()
        .find(|&k| body[k].1.abs() < peak.1.abs() * 0.1)
        .map(|k| body[k]);
    Some((foot, *peak))
}

/// La courbe d'une note (relative a son debut), pour la modifier : sans la
/// derniere valeur (celle de la fin, que [`cp_roll::expr::Expr::set_bend`]
/// remet).
pub(crate) fn rel_curve(n: &Note, curve: &[(f64, f32)]) -> Vec<(f64, f32)> {
    let k = curve.len().saturating_sub(1);
    curve[..k].iter().map(|&(q, v)| (q - n.qn0, v)).collect()
}
