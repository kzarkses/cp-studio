//! Les sources de modulation et les effets internes de FX Constellation :
//! le numero d'une source (l'espace d'adresses de l'ancienne version, garde
//! tel quel parce que les projets l'ont sur disque), ce qu'elle lit, son
//! neutre, sa couleur, son nom ; et la carte des curseurs de chaque JSFX.
//!
//! Les numeros (`P_EXT:FXC_ROUTES`, les presets) :
//! - 0 : le pad (selon les pastilles X / Y du parametre) ;
//! - 1..8 : la banque locale `CP_Mod LFO` de la piste ;
//! - 101..108 : la banque globale `CP_Mod MIDI` de la piste cachee « CP MOD »
//!   (elle arrive par un CC MIDI 14 bits) ;
//! - 201..204 : les enveloppes du Sound Gen ;
//! - 211..218 : les macros du bridge ;
//! - 221..222 : MOD A / MOD B du sequenceur ;
//! - 231..234 : les sources de note (NOTE, VEL, RAND, OCT) ;
//! - -1 / -2 : les axes X / Y du pad (une pastille).

/// Ce que vise une source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Band {
    Pad,
    /// La banque locale, slot 1..8.
    Lfo(u8),
    /// La banque globale, slot 1..8.
    Global(u8),
    /// Les enveloppes du Sound Gen, 1..4 (1 = celle de l'amplitude).
    Env(u8),
    Macro(u8),
    /// MOD A (1) / MOD B (2) du sequenceur.
    Seq(u8),
    /// NOTE (1), VEL (2), RAND (3), OCT (4).
    Note(u8),
    PadX,
    PadY,
}

/// Le decodeur unique. L'ordre des tests est le contrat (le plus haut
/// d'abord) ; un numero hors bande ne vise rien.
pub fn band_of(slot: i32) -> Option<Band> {
    let n = |base: i32, max: i32| {
        let k = slot - base;
        (1..=max).contains(&k).then_some(k as u8)
    };
    match slot {
        -1 => Some(Band::PadX),
        -2 => Some(Band::PadY),
        0 => Some(Band::Pad),
        s if s > 230 => n(230, 4).map(Band::Note),
        s if s > 220 => n(220, 2).map(Band::Seq),
        s if s > 210 => n(210, 8).map(Band::Macro),
        s if s > 200 => n(200, 4).map(Band::Env),
        s if s > 100 => n(100, 8).map(Band::Global),
        s if s > 0 => n(0, 8).map(Band::Lfo),
        _ => None,
    }
}

/// Le numero d'une bande.
pub fn slot_of(b: Band) -> i32 {
    match b {
        Band::Pad => 0,
        Band::Lfo(n) => n as i32,
        Band::Global(n) => 100 + n as i32,
        Band::Env(n) => 200 + n as i32,
        Band::Macro(n) => 210 + n as i32,
        Band::Seq(n) => 220 + n as i32,
        Band::Note(n) => 230 + n as i32,
        Band::PadX => -1,
        Band::PadY => -2,
    }
}

/// Le nom court d'une source (la colonne des sources d'une rangee).
pub fn short_label(slot: i32) -> String {
    match band_of(slot) {
        Some(Band::Pad) => "XY".into(),
        Some(Band::Lfo(n)) => format!("L{n}"),
        Some(Band::Global(n)) => format!("G{n}"),
        Some(Band::Env(n)) => format!("E{n}"),
        Some(Band::Macro(n)) => format!("M{n}"),
        Some(Band::Seq(n)) => format!("S{n}"),
        Some(Band::Note(n)) => ["NT", "VL", "RN", "OC"][(n - 1) as usize].into(),
        Some(Band::PadX) => "PX".into(),
        Some(Band::PadY) => "PY".into(),
        None => "?".into(),
    }
}

/// Le nom long d'une source.
pub fn long_label(slot: i32) -> String {
    match band_of(slot) {
        Some(Band::Pad) => "Pad".into(),
        Some(Band::Lfo(n)) => format!("CP LFO {n}"),
        Some(Band::Global(n)) => format!("Global LFO {n}"),
        Some(Band::Env(1)) => "ENV 1 (amp)".into(),
        Some(Band::Env(n)) => format!("ENV {n}"),
        Some(Band::Macro(n)) => format!("Macro {n}"),
        Some(Band::Seq(1)) => "Seq Mod A".into(),
        Some(Band::Seq(_)) => "Seq Mod B".into(),
        Some(Band::Note(n)) => {
            ["Note", "Velocity", "Random per note", "Note in octave"][(n - 1) as usize].into()
        }
        Some(Band::PadX) => "Pad X".into(),
        Some(Band::PadY) => "Pad Y".into(),
        None => format!("source {slot}"),
    }
}

/// Une couleur 8 bits.
pub type Rgb = (u8, u8, u8);

const PALETTE: [Rgb; 8] = [
    (0x8a, 0x94, 0xdd),
    (0xd9, 0x7f, 0xa8),
    (0x7f, 0xc4, 0xd9),
    (0xc9, 0xd9, 0x7f),
    (0xd9, 0xa3, 0x7f),
    (0x8b, 0xd3, 0xa3),
    (0xe0, 0x8b, 0x7a),
    (0xba, 0xa3, 0xd9),
];
const ENV_PALETTE: [Rgb; 4] = [
    (0xe8, 0xbd, 0x9c),
    (0xe9, 0xd3, 0x92),
    (0xe0, 0xaa, 0x8c),
    (0xf2, 0xc9, 0xb4),
];
const SEQ_PALETTE: [Rgb; 2] = [(0x63, 0xce, 0xbd), (0x7a, 0xb1, 0xe0)];
const NOTE_PALETTE: [Rgb; 4] = [
    (0xe8, 0x8e, 0xce),
    (0xf4, 0x80, 0x9d),
    (0xca, 0x98, 0xf4),
    (0xa7, 0x8d, 0xec),
];

fn scale(c: Rgb, k: f32, add: f32) -> Rgb {
    let f = |v: u8| ((v as f32 / 255.0 * k + add).clamp(0.0, 1.0) * 255.0).round() as u8;
    (f(c.0), f(c.1), f(c.2))
}

/// La couleur d'une source : la meme partout (pastille, anneau, fil, teinte
/// de la rangee). `accent` sert aux axes du pad.
pub fn color(slot: i32, accent: Rgb) -> Rgb {
    match band_of(slot) {
        Some(Band::Lfo(n)) => PALETTE[(n - 1) as usize],
        Some(Band::Global(n)) => scale(PALETTE[(n - 1) as usize], 0.68, 0.0),
        Some(Band::Macro(n)) => scale(PALETTE[(n - 1) as usize], 0.82, 0.16),
        Some(Band::Env(n)) => ENV_PALETTE[(n - 1) as usize],
        Some(Band::Seq(n)) => SEQ_PALETTE[(n - 1) as usize],
        Some(Band::Note(n)) => NOTE_PALETTE[(n - 1) as usize],
        Some(Band::PadX) | Some(Band::Pad) => accent,
        Some(Band::PadY) => scale(accent, 0.8, 0.2),
        None => (153, 153, 153),
    }
}

/// La polarite d'un lien : son neutre (la valeur de la source qui ne bouge
/// rien).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pol {
    /// Unipolaire : seulement au-dessus de la base (neutre 0).
    Uni,
    /// Bipolaire : autour de la base (neutre 0.5).
    Bi,
}

impl Pol {
    pub fn center(self) -> f64 {
        match self {
            Pol::Uni => 0.0,
            Pol::Bi => 0.5,
        }
    }
    pub fn key(self) -> char {
        match self {
            Pol::Uni => 'u',
            Pol::Bi => 'b',
        }
    }
    pub fn from_key(c: char) -> Option<Pol> {
        match c {
            'u' => Some(Pol::Uni),
            'b' => Some(Pol::Bi),
            _ => None,
        }
    }
}

/// Le neutre propre d'une source (`sourceCenter`) : `uni` dit si le slot de
/// banque est unipolaire (son curseur `lfoI_uni`).
pub fn source_center(slot: i32, uni: bool) -> f64 {
    match band_of(slot) {
        Some(Band::Lfo(_)) | Some(Band::Global(_)) => {
            if uni {
                0.0
            } else {
                0.5
            }
        }
        Some(Band::Seq(_)) => 0.5,
        Some(Band::Note(3)) => 0.5,
        Some(Band::PadX) | Some(Band::PadY) => 0.5,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Les JSFX : noms et cartes de curseurs (indices de parametre, 0 d'abord)
// ---------------------------------------------------------------------------

/// « FX Constellation Bridge » : le pad (le moteur tourne dans son @block)
/// et les 8 macros.
pub mod bridge {
    pub const NAME: &str = "FX Constellation Bridge";
    pub const ADD: &str = "JS:FX Constellation Bridge.jsfx";
    pub const X_POS: usize = 0;
    pub const Y_POS: usize = 1;
    pub const SLEW: usize = 2;
    pub const X_OUT: usize = 3;
    pub const Y_OUT: usize = 4;
    pub const XY_MIX: usize = 5;
    /// La valeur d'une macro (l'entree, aussi destination d'un lien).
    pub const MACRO_IN: usize = 6;
    /// La sortie lissee d'une macro (la source des slots 211..218).
    pub const MACRO_OUT: usize = 14;
    pub const ENG_MODE: usize = 22;
    pub const ENG_MASK: usize = 23;
    pub const WALK_RATE: usize = 24;
    pub const WALK_JIT: usize = 25;
    pub const WALK_JUMP: usize = 26;
    pub const WALK_SYNC: usize = 27;
    pub const FIG_MODE: usize = 28;
    pub const FIG_RATE: usize = 29;
    pub const FIG_SIZE: usize = 30;
    pub const ZONE: usize = 31;
    pub const COUNT: usize = 35;
}

/// `CP_Mod LFO` (sur la piste) et `CP_Mod MIDI` (sur « CP MOD ») : 8 slots.
pub mod bank {
    pub const LFO_NAME: &str = "CP_Mod LFO";
    pub const MIDI_NAME: &str = "CP_Mod MIDI";
    pub const LFO_ADD: &str = "JS:CP_Mod LFO.jsfx";
    pub const MIDI_ADD: &str = "JS:CP_Mod MIDI.jsfx";
    /// Le nom exact de la piste cachee qui porte la banque globale.
    pub const MOD_TRACK: &str = "CP MOD";
    pub fn on(i: usize) -> usize {
        (i - 1) * 5
    }
    pub fn shape(i: usize) -> usize {
        (i - 1) * 5 + 1
    }
    /// `:log`, 0.01..20 Hz : passer par le normalise.
    pub fn rate(i: usize) -> usize {
        (i - 1) * 5 + 2
    }
    pub fn sync(i: usize) -> usize {
        (i - 1) * 5 + 3
    }
    pub fn phase(i: usize) -> usize {
        (i - 1) * 5 + 4
    }
    /// LA sortie qu'un lien lit.
    pub fn out(i: usize) -> usize {
        40 + i - 1
    }
    /// La phase du cycle (affichage).
    pub fn pho(i: usize) -> usize {
        48 + i - 1
    }
    /// `:sqr`, 0..2 s : passer par le normalise.
    pub fn slew(i: usize) -> usize {
        56 + (i - 1) * 4
    }
    pub fn curve(i: usize) -> usize {
        57 + (i - 1) * 4
    }
    pub fn mode(i: usize) -> usize {
        58 + (i - 1) * 4
    }
    pub fn trig(i: usize) -> usize {
        59 + (i - 1) * 4
    }
    pub fn uni(i: usize) -> usize {
        88 + i - 1
    }
    pub fn sh(i: usize) -> usize {
        96 + i - 1
    }
    /// Les pas 4k+1..4k+4, 6 bits chacun.
    pub fn steps(i: usize, k: usize) -> usize {
        104 + (i - 1) * 4 + k
    }
    pub fn len(i: usize) -> usize {
        136 + i - 1
    }
    pub const SHAPED_MARK: usize = 144;
    pub const FAST_CH: usize = 145;
    pub const FAST_MASK: usize = 146;
    pub const EXT_COUNT: usize = 88;
    pub const UNI_COUNT: usize = 96;
    pub const SH_COUNT: usize = 104;
    pub const ST_COUNT: usize = 136;
    pub const COUNT: usize = 147;
    pub const SHAPES: [&str; 8] = [
        "Sine", "Triangle", "Saw Up", "Saw Down", "Square", "Random", "S&H", "Steps",
    ];
    pub const SYNCS: [&str; 11] = [
        "Free", "1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars", "4 bars", "8 bars", "16 bars",
        "32 bars",
    ];
    pub const MODES: [&str; 3] = ["Loop", "One-shot", "Loop resync"];
    /// Le CC MIDI (le MSB) du slot global i ; le LSB est a +32.
    pub fn cc(i: usize) -> u8 {
        (16 + i - 1) as u8
    }
    /// Les 16 pas d'un slot depuis ses 4 curseurs (0..63 chacun).
    pub fn unpack_steps(v: [f64; 4]) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (k, w) in v.iter().enumerate() {
            let w = w.max(0.0).round() as u64;
            for j in 0..4 {
                out[k * 4 + j] = ((w >> (6 * j)) & 63) as u8;
            }
        }
        out
    }
    pub fn pack_steps(s: &[u8; 16]) -> [f64; 4] {
        let mut out = [0.0; 4];
        for (k, o) in out.iter_mut().enumerate() {
            let mut w = 0u64;
            for j in 0..4 {
                w |= ((s[k * 4 + j] & 63) as u64) << (6 * j);
            }
            *o = w as f64;
        }
        out
    }
}

/// « FX Constellation - Mod Mix » : 16 bus de 4 entrees, 8 voies de
/// profondeur. Toujours en tete de chaine.
pub mod mix {
    pub const NAME: &str = "FX Constellation - Mod Mix";
    pub const ADD: &str = "JS:FX Constellation - Mod Mix.jsfx";
    pub const BUSES: usize = 16;
    pub const INPUTS: usize = 4;
    pub const VOICES: usize = 8;
    pub fn out(b: usize) -> usize {
        b - 1
    }
    pub fn input(b: usize, k: usize) -> usize {
        16 + (b - 1) * 4 + k - 1
    }
    pub fn amt(b: usize, k: usize) -> usize {
        80 + (b - 1) * 4 + k - 1
    }
    pub fn ctr(b: usize, k: usize) -> usize {
        144 + (b - 1) * 4 + k - 1
    }
    pub fn dm_tgt(d: usize) -> usize {
        208 + d - 1
    }
    pub fn dm_in(d: usize) -> usize {
        216 + d - 1
    }
    pub fn dm_amt(d: usize) -> usize {
        224 + d - 1
    }
    pub fn dm_ctr(d: usize) -> usize {
        232 + d - 1
    }
    pub const COUNT: usize = 240;
}

/// « FX Constellation - Notes » : les sources de note.
pub mod notes {
    pub const NAME: &str = "FX Constellation - Notes";
    pub const ADD: &str = "JS:FX Constellation - Notes.jsfx";
    pub const LO: usize = 4;
    pub const HI: usize = 5;
    pub const GLIDE: usize = 6;
    pub const KNOTE: usize = 10;
    pub const KGATE: usize = 11;
    pub const KVEL: usize = 12;
    pub const COUNT: usize = 13;
    pub const PARK: &str = "CP_FXC_NOTES_PARK";
}

/// « FX Constellation - Sequencer ».
pub mod seq {
    pub const NAME: &str = "FX Constellation - Sequencer";
    pub const ADD: &str = "JS:FX Constellation - Sequencer.jsfx";
    /// MOD A, MOD B : les sorties lues par les liens.
    pub const MOD_OUT: [usize; 2] = [16, 17];
    pub const PARK: &str = "CP_FXC_SEQ_PARK";
}

/// « FX Constellation - Sound Generator ».
pub mod sg {
    pub const NAME: &str = "FX Constellation - Sound Generator";
    pub const ADD: &str = "JS:FX Constellation - Sound Generator.jsfx";
    /// Les sorties des enveloppes 1..4 (une table, pas un calcul).
    pub const ENV_OUT: [usize; 4] = [31, 63, 68, 73];
    pub const EXTRA_COUNT: usize = 74;
    /// « Continuous Pitch » : le bourdon suit la note accrochee au clavier.
    pub const CONT_NOTE: usize = 58;
}

/// Les cles de piste (`P_EXT`) et de projet de l'ancienne version, reprises.
pub mod keys {
    /// Le modele des liens internes (`ModGraph`).
    pub const ROUTES: &str = "FXC_ROUTES";
    /// Le canal rapide de la piste : « ch|GUID ».
    pub const FAST_CH: &str = "FXC_FASTCH";
    /// Les effets hors de ROLL (GUID separes par des espaces, tries).
    pub const DETACHED: &str = "CP_FXC_detached";
    /// L'etat CP Studio de la piste (le nouveau format).
    pub const STATE: &str = "CPS_FXC";
    /// Section de projet (ExtState de projet) de l'ancienne version.
    pub const PROJ: &str = "CP_FXConstellation";
}

/// Le genre d'un effet de la chaine pour FX Constellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxKind {
    Bridge,
    /// `CP_Mod LFO` (ou `CP_Mod MIDI` sur la piste CP MOD).
    Bank,
    Mix,
    Notes,
    Seq,
    SoundGen,
    /// Un effet protege (jamais liste ni deplace) : RS5K, le kit.
    Protected,
    /// Un effet de l'utilisateur.
    User,
}

/// Le genre d'un effet d'apres son nom (`TrackFX_GetFXName`).
pub fn fx_kind(name: &str) -> FxKind {
    let l = name.to_lowercase();
    if l.contains("fx constellation bridge") {
        FxKind::Bridge
    } else if l.contains("cp_mod") {
        FxKind::Bank
    } else if l.contains("fx constellation - mod mix") {
        FxKind::Mix
    } else if l.contains("fx constellation - notes") {
        FxKind::Notes
    } else if l.contains("fx constellation - sequencer") {
        FxKind::Seq
    } else if l.contains("sound generator") {
        FxKind::SoundGen
    } else if [
        "reasamplomatic",
        "cp_kit_choke",
        "cp_kitsampler",
        "cp kit sampler",
    ]
    .iter()
    .any(|k| l.contains(k))
    {
        FxKind::Protected
    } else {
        FxKind::User
    }
}

/// Le nom court d'un effet : sans « VST3: », sans parentheses, 25 signes.
pub fn short_fx_name(full: &str) -> String {
    let s = match full.split_once(": ") {
        Some((pre, rest)) if pre.len() <= 6 && !pre.contains(' ') => rest,
        _ => full,
    };
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.trim().to_string();
    if t.chars().count() > 25 {
        let mut c: String = t.chars().take(22).collect();
        c.push_str("...");
        c
    } else if t.is_empty() {
        full.to_string()
    } else {
        t
    }
}
