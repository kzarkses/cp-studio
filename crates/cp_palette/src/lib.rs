//! La palette du clic droit, commune a l'editeur et a Guitar Tab : des
//! icones au curseur, par groupe, a la place d'un long menu.
//!
//! Une ligne de groupes (choisir, editer, temps, velocite, accords, arpeges,
//! rythmes euclidiens, bends, techniques, plus) ; le groupe survole ouvre ses
//! icones dessous, sans clic. Une icone survolee montre son resultat avant le
//! clic (chaque vue le dessine) ; une icone qui ne changerait rien est
//! eteinte. Un clic applique (un pas d'annulation) et ferme ; Maj+clic garde
//! la palette ouverte. La molette sur un rythme euclidien change ses coups
//! (Maj : ses cases). Le dernier groupe utilise s'ouvre d'office.
//!
//! Les operations sur les notes ([`apply`]) sont celles de [`cp_roll`] : la
//! meme operation fait la meme chose dans les deux vues. Guitar Tab y passe
//! ses notes (la corde dans le canal) et redonne ensuite une corde aux notes
//! neuves.
//!
//! Les icones sont des traces SVG (grille de 24, comme les icones Lucide),
//! dessines a chaque image : un trait, un remplissage, un petit texte.

mod draw;

pub use draw::{Area, PalHit, draw, layout};

use cp_roll::{ArpMode, CHORDS, Roll};

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
    /// Les techniques de jeu de Guitar Tab (P.M., note morte, harmonique,
    /// laisser sonner).
    Tech,
    More,
}

impl Cat {
    /// Les groupes de l'editeur, dans l'ordre.
    pub const EDITOR: [Cat; 9] = [
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

    /// Ceux de Guitar Tab : les memes, et ses techniques.
    pub const TAB: [Cat; 10] = [
        Cat::Select,
        Cat::Edit,
        Cat::Time,
        Cat::Vel,
        Cat::Chord,
        Cat::Arp,
        Cat::Euclid,
        Cat::Bend,
        Cat::Tech,
        Cat::More,
    ];

    /// Un trait separe les familles.
    pub fn group(self) -> u8 {
        match self {
            Cat::Select => 0,
            Cat::Edit | Cat::Time | Cat::Vel => 1,
            Cat::Chord | Cat::Arp | Cat::Euclid => 2,
            Cat::Bend | Cat::Tech => 3,
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
            Cat::Tech => "Playing techniques",
            Cat::More => "More",
        }
    }

    pub fn glyph(self) -> Glyph {
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
            // Une main : la facon de jouer.
            Cat::Tech => icon(
                "M18 11V6a2 2 0 0 0-4 0v0 M14 10V4a2 2 0 0 0-4 0v2 M10 10.5V6a2 2 0 0 0-4 0v8 M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15",
                "",
            ),
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

pub fn icon(s: &str, f: &str) -> Glyph {
    Glyph::Icon {
        s: s.into(),
        f: f.into(),
        t: String::new(),
    }
}

pub fn icon_t(s: &str, f: &str, t: &str) -> Glyph {
    Glyph::Icon {
        s: s.into(),
        f: f.into(),
        t: t.into(),
    }
}

/// Un cercle en donnees SVG.
pub fn circ(cx: f32, cy: f32, r: f32) -> String {
    format!(
        "M{} {}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0",
        cx - r,
        cy,
        2.0 * r,
        -2.0 * r
    )
}

pub fn dots(pts: &[(f32, f32)], r: f32) -> String {
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
pub fn euclid_glyph(pulses: usize, steps: usize) -> Glyph {
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
pub const BEND: &str = "M3 18h6c3.5 0 5-3 5.6-9 M12 11.2l2.6-3 2.6 3";

/// Une operation de la palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    /// Son numero ; 0 : un mot, pas un bouton.
    pub id: u32,
    pub cat: Cat,
    /// Un trait separe les groupes d'un meme groupe de la palette.
    pub group: u8,
    pub label: String,
    pub glyph: Glyph,
    /// Un reglage (coche ou non) : il ne ferme pas la palette.
    pub setting: Option<bool>,
    /// Les notes l'ont deja (une technique, un bend) : le bouton est allume,
    /// un clic l'enleve.
    pub on: bool,
}

impl Op {
    pub fn new(cat: Cat, group: u8, id: u32, label: impl Into<String>, glyph: Glyph) -> Op {
        Op {
            id,
            cat,
            group,
            label: label.into(),
            glyph,
            setting: None,
            on: false,
        }
    }
}

/// Les bends de la palette : (numero, demi-tons au sommet).
pub const BEND_OPS: [(u32, f32); 3] = [(100, 1.0), (101, 2.0), (102, 3.0)];

/// La palette ouverte. `P` : ce que la vue garde pour l'apercu de
/// l'operation survolee.
#[derive(Clone, Debug)]
pub struct Palette<P> {
    /// Ou le clic l'a ouverte.
    pub x: f32,
    pub y: f32,
    /// Ses groupes ([`Cat::EDITOR`], [`Cat::TAB`]).
    pub cats: &'static [Cat],
    pub cat: Cat,
    /// Les operations qui changeraient quelque chose.
    pub ok: Vec<u32>,
    /// L'operation survolee et ce qu'elle donnerait.
    pub preview: Option<(u32, P)>,
    /// Le cadre dessine : x, y, w, h.
    pub rect: (f32, f32, f32, f32),
}

impl<P> Palette<P> {
    pub fn new(x: f32, y: f32, cats: &'static [Cat], cat: Cat) -> Palette<P> {
        let cat = if cats.contains(&cat) {
            cat
        } else {
            cats.first().copied().unwrap_or_default()
        };
        Palette {
            x,
            y,
            cats,
            cat,
            ok: Vec::new(),
            preview: None,
            rect: (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Le groupe de rang `k`.
    pub fn cat_at(&self, k: u8) -> Option<Cat> {
        self.cats.get(k as usize).copied()
    }
}

/// La courbe d'un bend de la palette sur une note de `len` noires a `bpm` :
/// (noires depuis son debut, demi-tons).
pub fn bend_curve(id: u32, len: f64, bpm: f64) -> Vec<(f64, f32)> {
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

/// La vue qui ouvre la palette (le nom de ses pas d'annulation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Editor,
    Tab,
}

/// Ce que les operations communes savent de la vue.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub host: Host,
    /// La grille (noires) ; 0 : aucune (le jeu libre de Guitar Tab).
    pub grid: f64,
    /// Son nom (« 1/16 »).
    pub grid_name: String,
    pub strength: f64,
    pub swing: f64,
    /// Les rythmes euclidiens : (coups, cases).
    pub euclid: [(usize, usize); 6],
    /// Une gamme est posee.
    pub scale: bool,
    /// Le curseur d'edition, la plage de la regle, la longueur d'une mesure
    /// au curseur (noires).
    pub cursor: f64,
    pub range: Option<(f64, f64)>,
    pub bar: f64,
    /// La hauteur, le canal et la velocite d'une note neuve (sans note
    /// choisie).
    pub pitch: i32,
    pub chan: i32,
    pub vel: i32,
}

/// Les rythmes euclidiens de la palette, au depart : (coups, cases).
pub const EUCLID: [(usize, usize); 6] = [(3, 8), (5, 8), (5, 16), (7, 16), (9, 16), (11, 16)];

impl Default for Ctx {
    fn default() -> Ctx {
        Ctx {
            host: Host::Editor,
            grid: 0.25,
            grid_name: "1/16".into(),
            strength: 1.0,
            swing: 0.0,
            euclid: EUCLID,
            scale: false,
            cursor: 0.0,
            range: None,
            bar: 4.0,
            pitch: 36,
            chan: 0,
            vel: 100,
        }
    }
}

/// Le nom d'un pas d'annulation, selon la vue.
macro_rules! lab {
    ($h:expr, $s:literal) => {
        match $h {
            Host::Editor => concat!("CP Editor: ", $s),
            Host::Tab => concat!("CP Guitar Tab: ", $s),
        }
    };
}

/// Les operations communes : choisir, editer, temps, velocite, accords,
/// arpeges, rythmes euclidiens (chaque vue ajoute ses bends, ses
/// techniques et le reste).
pub fn common_ops(c: &Ctx) -> Vec<Op> {
    use Cat::*;
    // Le jeu libre de Guitar Tab : pas de grille (l'editeur garde ses mots).
    let free = c.grid <= 0.0 && c.host == Host::Tab;
    let mut v: Vec<Op> = Vec::new();
    let mut add = |cat: Cat, group: u8, id: u32, label: String, glyph: Glyph| {
        v.push(Op::new(cat, group, id, label, glyph));
    };
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
        if c.scale {
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
        if !free {
            format!("Length = grid ({})", c.grid_name)
        } else {
            "Length = grid (no grid in free time)".into()
        },
        icon("M4 5v14 M10 5v14 M16 5v14", "M4 10.5h6v3H4z"),
    );
    let grid_dots = |pts: &[(f32, f32)]| icon("M5 4v16 M12 4v16 M19 4v16", &dots(pts, 2.1));
    add(
        Time,
        0,
        54,
        if !free {
            format!(
                "Quantize \u{00b7} {} % \u{00b7} swing {} %",
                (c.strength * 100.0).round(),
                (c.swing * 100.0).round()
            )
        } else {
            "Quantize (no grid in free time)".into()
        },
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
            if c.scale {
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
    let g = if c.grid > 0.0 {
        c.grid_name.clone()
    } else {
        "1/16".into()
    };
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
    for (k, &(p, n)) in c.euclid.iter().enumerate() {
        add(
            Euclid,
            0,
            90 + k as u32,
            format!("Euclid {p}/{n} \u{00b7} wheel: hits, Shift+wheel: steps"),
            euclid_glyph(p, n),
        );
    }
    // Les reglages : coches.
    for o in &mut v {
        o.setting = match o.id {
            40..=43 => {
                Some((c.strength - [1.0, 0.75, 0.5, 0.25][(o.id - 40) as usize]).abs() < 1e-6)
            }
            44..=47 => Some((c.swing - [0.0, 0.08, 0.16, 0.24][(o.id - 44) as usize]).abs() < 1e-6),
            _ => None,
        };
    }
    v
}

/// Les reglages communs (ils ne touchent pas aux notes).
pub fn is_setting(id: u32) -> bool {
    (40..=47).contains(&id)
}

/// Pose un reglage commun (force, swing). Vrai si `id` en est un.
pub fn set_setting(id: u32, strength: &mut f64, swing: &mut f64) -> bool {
    match id {
        40..=43 => *strength = [1.0, 0.75, 0.5, 0.25][(id - 40) as usize],
        44..=47 => *swing = [0.0, 0.08, 0.16, 0.24][(id - 44) as usize],
        _ => return false,
    }
    true
}

/// Une operation commune sur des notes (les vraies, ou une copie pour
/// l'apercu) : (combien de notes, le nom du pas d'annulation ; vide : rien
/// a ecrire). Sans note choisie, elle vise toutes les notes (comme REAPER).
pub fn apply(id: u32, r: &mut Roll, c: &Ctx, seed: u64) -> (usize, &'static str) {
    let g = c.grid;
    let h = c.host;
    // Sans grille (le jeu libre) : ni quantification, ni longueur de grille.
    if g <= 0.0 && matches!(id, 52 | 54) {
        return (0, "");
    }
    if id == 54 {
        return (r.quantize(g, c.strength, c.swing), lab!(h, "quantize"));
    }
    // Le rythme euclidien : sur la plage de la regle, sinon l'etendue des
    // notes choisies, sinon la mesure du curseur ; a la hauteur de la
    // premiere note choisie (sinon `c.pitch`), sur son canal.
    let eu_span = c
        .range
        .filter(|(a, b)| b - a > 1e-6)
        .or_else(|| r.bounds().filter(|_| r.has_sel()));
    let bar = c.bar.max(1e-6);
    let eu_span = eu_span.unwrap_or_else(|| {
        let a = (c.cursor / bar).floor() * bar;
        (a, a + bar)
    });
    let eu_note = r.notes.iter().find(|n| n.sel).copied();
    let (eu_p, eu_c, eu_v) = eu_note.map_or((c.pitch, c.chan, c.vel), |n| (n.pitch, n.chan, n.vel));
    let g_arp = if g > 0.0 { g } else { 0.25 };
    let g_hum = if g > 0.0 { g } else { 0.25 };
    match id {
        70..=78 => (r.chordify((id - 70) as usize), lab!(h, "chord")),
        80..=84 => {
            let (mode, oct) = match id {
                80 => (ArpMode::Up, 1),
                81 => (ArpMode::Down, 1),
                82 => (ArpMode::UpDown, 1),
                83 => (ArpMode::Random, 1),
                _ => (ArpMode::Up, 2),
            };
            (r.arp(g_arp, mode, 0.9, oct, seed), lab!(h, "arpeggio"))
        }
        90..=95 => {
            let (p, n) = c.euclid[(id - 90) as usize];
            let (a, b) = eu_span;
            (r.euclid(a, b, eu_p, eu_c, eu_v, n, p, 0), lab!(h, "euclid"))
        }
        1 => {
            r.select_all();
            (0, "")
        }
        2 => {
            r.invert_sel();
            (0, "")
        }
        3 => {
            r.select_same_pitch();
            (0, "")
        }
        10 => (r.legato(), lab!(h, "legato")),
        11 => (r.reverse(), lab!(h, "reverse")),
        12 => (r.invert_pitch(), lab!(h, "mirror")),
        13 => (r.glue(), lab!(h, "glue")),
        14 => (r.split_at(c.cursor), lab!(h, "split")),
        15 => (r.snap_to_scale(), lab!(h, "snap to scale")),
        20 => (r.vel_set(127), lab!(h, "velocity")),
        21 => (r.vel_set(100), lab!(h, "velocity")),
        22 => (r.vel_set(80), lab!(h, "velocity")),
        23 => (r.vel_set(64), lab!(h, "velocity")),
        24 => (r.vel_ramp(40, 120), lab!(h, "velocity ramp")),
        25 => (r.vel_ramp(120, 40), lab!(h, "velocity ramp")),
        26 => (r.vel_scale(0.5, 80), lab!(h, "velocity")),
        27 => (r.vel_scale(1.5, 80), lab!(h, "velocity")),
        30 => (r.humanize(1, g_hum * 0.05, 4), lab!(h, "humanize")),
        31 => (r.humanize(2, g_hum * 0.12, 10), lab!(h, "humanize")),
        32 => (r.humanize(3, g_hum * 0.25, 20), lab!(h, "humanize")),
        50 => (r.scale_len(2.0), lab!(h, "length")),
        51 => (r.scale_len(0.5), lab!(h, "length")),
        52 => (r.set_len(g), lab!(h, "length")),
        _ => (0, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_ids_ne_se_doublent_pas() {
        let ops = common_ops(&Ctx::default());
        let mut ids: Vec<u32> = ops.iter().map(|o| o.id).filter(|&i| i != 0).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn sans_grille_ni_quantification_ni_longueur() {
        let mut r = Roll::from_notes([(0.1, 0.3, 60, 0, 100, true, false)]);
        let c = Ctx {
            grid: 0.0,
            ..Ctx::default()
        };
        assert_eq!(apply(54, &mut r, &c, 1).0, 0);
        assert_eq!(apply(52, &mut r, &c, 1).0, 0);
        assert!((r.notes[0].qn0 - 0.1).abs() < 1e-9);
        // L'accord, lui, se pose.
        assert_eq!(apply(71, &mut r, &c, 1), (2, "CP Editor: chord"));
        let c = Ctx {
            host: Host::Tab,
            ..c
        };
        assert_eq!(apply(10, &mut r, &c, 1).1, "CP Guitar Tab: legato");
    }

    #[test]
    fn le_groupe_d_ouverture_est_un_des_siens() {
        let p: Palette<()> = Palette::new(0.0, 0.0, &Cat::EDITOR, Cat::Tech);
        assert_eq!(p.cat, Cat::Select);
        let p: Palette<()> = Palette::new(0.0, 0.0, &Cat::TAB, Cat::Tech);
        assert_eq!(p.cat, Cat::Tech);
        assert_eq!(p.cat_at(8), Some(Cat::Tech));
    }
}
