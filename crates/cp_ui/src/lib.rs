//! Le toolkit de CP Studio : de quoi faire une interface avec le moteur de
//! dessin, sans Windows (testable et regardable hors de REAPER).
//!
//! - [`Theme`] : couleurs, tailles et polices, a l'echelle de l'ecran (DPI).
//! - [`Ev`] : les evenements d'entree, deja traduits de Windows.
//! - [`icons`] : Lucide en vecteur.
//! - [`TextField`] : un champ de saisie d'une ligne.
//! - [`scroll`] : la geometrie d'une barre de defilement.

// Le dessin prend naturellement x, y, w, h, rayon, epaisseur, couleur... : des
// parametres positionnels nombreux sont la forme juste ici.
#![allow(clippy::too_many_arguments)]

pub mod icons;
pub mod scroll;
pub mod svg;
pub mod textfield;

pub use icons::{Icon, IconCache};
pub use textfield::TextField;

use cp_gfx::text::Font;
use cp_gfx::{Color, hex, rgb};

/// Les boutons de la souris.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Les touches de modification, au moment de l'evenement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Un evenement d'entree, en pixels de la vue.
#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Move {
        x: f32,
        y: f32,
        mods: Mods,
    },
    Down {
        x: f32,
        y: f32,
        b: Button,
        dbl: bool,
        mods: Mods,
    },
    Up {
        x: f32,
        y: f32,
        b: Button,
        mods: Mods,
    },
    /// Crans de molette (1.0 = un cran), `dx` pour la molette horizontale.
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        mods: Mods,
    },
    /// Une touche (code virtuel Windows) et le texte qu'elle produirait.
    Key {
        vk: u32,
        mods: Mods,
        text: Option<String>,
    },
    /// Un caractere tape (saisie de texte).
    Char(char),
    Leave,
}

/// Les codes de touches dont le toolkit a besoin.
pub mod vk {
    pub const BACK: u32 = 0x08;
    pub const TAB: u32 = 0x09;
    pub const RETURN: u32 = 0x0D;
    pub const ESCAPE: u32 = 0x1B;
    pub const SPACE: u32 = 0x20;
    pub const PRIOR: u32 = 0x21;
    pub const NEXT: u32 = 0x22;
    pub const END: u32 = 0x23;
    pub const HOME: u32 = 0x24;
    pub const LEFT: u32 = 0x25;
    pub const UP: u32 = 0x26;
    pub const RIGHT: u32 = 0x27;
    pub const DOWN: u32 = 0x28;
    pub const DELETE: u32 = 0x2E;
    pub const F2: u32 = 0x71;
    pub const F5: u32 = 0x74;
}

/// Une couleur (r, g, b).
pub type Rgb = (u8, u8, u8);

/// Couleurs, tailles, polices. `scale` = DPI / 96.
#[derive(Clone, Debug)]
pub struct Theme {
    pub scale: f32,
    pub bg: Color,
    pub bg_alt: Color,
    pub panel: Color,
    pub bar: Color,
    pub seam: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_faint: Color,
    pub accent: Color,
    pub sel: Color,
    pub sel_dim: Color,
    pub hover: Color,
    pub wave: Color,
    pub wave_sel: Color,
    pub section: Color,
    pub playhead: Color,
    pub star: Color,
    pub field: Color,
    pub error: Color,
}

impl Theme {
    pub fn dark(scale: f32) -> Theme {
        Theme {
            scale: scale.max(0.5),
            bg: hex(0x1C1D21),
            bg_alt: hex(0x202126),
            panel: hex(0x24252B),
            bar: hex(0x2A2C32),
            seam: hex(0x111215),
            text: hex(0xDCDDE2),
            text_dim: hex(0x9A9CA4),
            text_faint: hex(0x6A6C74),
            accent: hex(0x4C9EFF),
            sel: hex(0x2F5583),
            sel_dim: hex(0x2B3646),
            hover: hex(0x2A2C33),
            wave: hex(0x5AA6E8),
            wave_sel: hex(0xF0C060),
            section: hex(0x3A3320),
            playhead: hex(0xFFFFFF),
            star: hex(0xF2C444),
            field: hex(0x16171A),
            error: hex(0xE06A5A),
        }
    }

    /// Les couleurs du theme de REAPER : `color` lit une cle du theme
    /// (`GetThemeColor`). Les listes prennent celles des listes de REAPER
    /// (`genlist_*`), les barres celle du transport, l'accent celui des
    /// boutons allumes ; ce que le theme ne dit pas garde la valeur sombre.
    pub fn reaper(scale: f32, color: &dyn Fn(&str) -> Option<Rgb>) -> Theme {
        let d = Theme::dark(scale);
        let c = |k: &str, def: Color| color(k).map_or(def, |(r, g, b)| rgb(r, g, b));
        let bg = c("genlist_bg", d.bg);
        let text = c("genlist_fg", d.text);
        let accent = c("col_toolbar_text_on", d.accent);
        let play = c("playcursor_color", d.wave_sel);
        Theme {
            scale: d.scale,
            bg,
            bg_alt: bg.mix(text, 0.025),
            panel: bg.mix(text, 0.07),
            bar: c("col_main_bg2", d.bar),
            seam: c("genlist_grid", d.seam),
            text,
            text_dim: c("col_main_text", d.text_dim),
            text_faint: c("col_main_text2", d.text_faint),
            accent,
            sel: c("genlist_selbg", d.sel),
            sel_dim: c("genlist_seliabg", d.sel_dim),
            hover: bg.mix(text, 0.06),
            wave: accent.mix(text, 0.25),
            wave_sel: play,
            section: bg.mix(play, 0.14),
            playhead: play,
            star: d.star,
            field: c("col_main_editbk", d.field),
            error: d.error,
        }
    }

    /// Une longueur de la maquette (a 96 DPI) a l'ecran.
    pub fn s(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    pub fn font(&self) -> Font {
        Font::new(0, self.s(13.0) as u16, 400)
    }
    pub fn font_bold(&self) -> Font {
        Font::new(0, self.s(13.0) as u16, 600)
    }
    pub fn font_small(&self) -> Font {
        Font::new(0, self.s(11.0) as u16, 400)
    }
    pub fn font_mono(&self) -> Font {
        Font::new(1, self.s(12.0) as u16, 400)
    }
}
