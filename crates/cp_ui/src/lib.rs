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

pub mod handle;
pub mod icons;
pub mod paint;
pub mod pop;
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
    /// La touche Windows (REAPER s'en sert dans ses gestes souris).
    pub win: bool,
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

/// Les curseurs de souris de REAPER, par ce qu'un geste ferait (les noms de
/// ses fichiers `Cursors\<nom>.cur`). Une vue dit lequel montrer selon ce
/// qui est sous la souris et les touches tenues, comme REAPER.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Pointer {
    #[default]
    Arrow,
    // L'arrangement
    ItemMove,
    ItemCopy,
    ItemSlide,
    TrimLeft,
    TrimRight,
    StretchLeft,
    StretchRight,
    DualEdge,
    FadeIn,
    FadeOut,
    ItemVol,
    IBeam,
    TimeSelEdge,
    Marquee,
    HandScroll,
    Delete,
    // L'editeur MIDI
    NoteDraw,
    NotePaint,
    NoteErase,
    NoteCopy,
    NoteEdgeLeft,
    NoteEdgeRight,
    NoteEdgeDual,
    NoteStretchLeft,
    NoteStretchRight,
    NoteMoveH,
    NoteMoveV,
    NoteVel,
    NoteChord,
    NoteArp,
    TouchSelect,
    /// Une piste qu'on glisse (son en-tete).
    TrackDrag,
    // Les poignees d'interface
    ResizeNS,
    ResizeWE,
}

/// Une edition « au survol » : sur ce qui est sous la souris, a la place de
/// la souris (les scripts que Cedric lie a Q, D, S, Z, A, E dans
/// l'arrangement : nvk, LKC, CP_DeleteUnderMouse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverEdit {
    TrimLeft,
    TrimRight,
    Split,
    Delete,
    FadeIn,
    FadeOut,
}

impl HoverEdit {
    /// Reconnait une action de REAPER a son nom (un script, une action
    /// personnalisee) : demain, des raccourcis propres a CP Studio porteront
    /// les memes mots.
    pub fn of_name(name: &str) -> Option<HoverEdit> {
        let n = name.to_ascii_lowercase();
        if n.contains("trim left") || (n.contains("trim") && n.contains("left edge")) {
            Some(HoverEdit::TrimLeft)
        } else if n.contains("trim right") || (n.contains("trim") && n.contains("right edge")) {
            Some(HoverEdit::TrimRight)
        } else if n.contains("fadein") || n.contains("fade in") {
            Some(HoverEdit::FadeIn)
        } else if n.contains("fadeout") || n.contains("fade out") {
            Some(HoverEdit::FadeOut)
        } else if n.contains("delete") && n.contains("mouse") && !n.contains("envelope") {
            Some(HoverEdit::Delete)
        } else if n.contains("split") && !n.contains("time selection") && !n.contains("razor") {
            Some(HoverEdit::Split)
        } else {
            None
        }
    }
}

/// La ligne verticale a la souris, avec les reglages de REAPER.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Guide {
    pub on: bool,
    /// Calee sur la grille quand le magnetisme est la.
    pub snap: bool,
    /// Maj (ou Ctrl) tenue : sans magnetisme.
    pub shift_free: bool,
    pub ctrl_free: bool,
    pub color: Option<Color>,
}

impl Guide {
    /// Les reglages de REAPER : `guidelines2` (Preferences > Appearance,
    /// « Display vertical line at mouse position »), releve dans reaper.exe
    /// le 2026-11-22 : 0x10 montree, 0x20 calee (le bouton du magnetisme),
    /// 0x40 Maj sans, 0x80 Ctrl sans.
    pub fn from_reaper(guidelines2: i32, color: Option<Color>) -> Guide {
        Guide {
            on: guidelines2 & 0x10 != 0,
            snap: guidelines2 & 0x20 != 0,
            shift_free: guidelines2 & 0x40 != 0,
            ctrl_free: guidelines2 & 0x80 != 0,
            color,
        }
    }

    /// Calee, avec ces touches tenues ?
    pub fn snaps(&self, m: Mods) -> bool {
        self.snap && !(self.shift_free && m.shift) && !(self.ctrl_free && m.ctrl)
    }
}

/// Les couleurs de l'arrangement et de l'editeur MIDI de REAPER, lues dans
/// le theme (ses cles et l'opacite de leurs modes de melange) : nos vues ont
/// les memes fonds, grilles, items, notes et curseurs que REAPER. Les valeurs
/// par defaut sont celles du theme de Cedric (Reapertips Green, 2026-11-22).
#[derive(Clone, Copy, Debug)]
pub struct ReaperLook {
    // L'arrangement
    pub arr_bg: Color,
    pub tr_bg: [Color; 2],
    pub tr_sel_bg: [Color; 2],
    pub tr_div: Color,
    pub grid_bar: Color,
    pub grid_beat: Color,
    pub grid_sub: Color,
    pub item_bg: Color,
    pub item_bg_sel: Color,
    pub item_border: Color,
    pub item_border_sel: Color,
    pub item_label: Color,
    pub item_label_sel: Color,
    pub peaks: Color,
    pub peaks_sel: Color,
    pub fade_line: Color,
    pub fade_zone: Color,
    pub timesel: Color,
    pub ruler_bg: Color,
    pub ruler_fg: Color,
    pub ruler_fg2: Color,
    // L'editeur MIDI
    pub m_bg: [Color; 2],
    pub m_outer: [Color; 2],
    pub m_oct: Color,
    pub m_grid_bar: Color,
    pub m_grid_beat: Color,
    pub m_grid_sub: Color,
    pub m_note_border: Color,
    pub m_note_sel: Color,
    pub m_note_mute: Color,
    pub m_note_text: Color,
    pub m_key: [Color; 2],
    pub m_key_sel: Color,
    pub m_left_bg: Color,
    pub m_ruler_bg: Color,
    pub m_ruler_fg: Color,
    pub m_edit_cursor: Color,
    pub m_selbg: Color,
    /// Le lasso de l'arrangement (clic droit glisse).
    pub marquee_fill: Color,
    pub marquee_outline: Color,
}

impl Default for ReaperLook {
    fn default() -> Self {
        let a = |c: u32, k: f32| hex(c).with_alpha((k * 255.0).round() as u8);
        ReaperLook {
            arr_bg: hex(0x424242),
            tr_bg: [hex(0x424242), hex(0x454545)],
            tr_sel_bg: [hex(0x525252), hex(0x4F4F4F)],
            tr_div: hex(0x333333),
            grid_bar: a(0x000000, 0.55),
            grid_beat: a(0x000000, 0.25),
            grid_sub: a(0x000000, 0.15),
            item_bg: hex(0x9E9E9E),
            item_bg_sel: hex(0x949494),
            item_border: a(0x000000, 0.8),
            item_border_sel: a(0xFFFFFF, 0.8),
            item_label: hex(0x1C1C1C),
            item_label_sel: hex(0x2C2C2C),
            peaks: hex(0x101010),
            peaks_sel: hex(0x1F1F1F),
            fade_line: hex(0x950018),
            fade_zone: a(0x3F0001, 0.3),
            timesel: a(0xFFFFFF, 0.3),
            ruler_bg: hex(0x333333),
            ruler_fg: hex(0x9A9A9A),
            ruler_fg2: hex(0x585858),
            m_bg: [hex(0x4C4C4C), hex(0x424242)],
            m_outer: [hex(0x333333), hex(0x252525)],
            m_oct: hex(0xA6A6A6),
            m_grid_bar: a(0xFFFFFF, 0.25),
            m_grid_beat: a(0x000000, 0.5),
            m_grid_sub: a(0x000000, 0.2),
            m_note_border: hex(0x101214),
            m_note_sel: hex(0xFFFFFF),
            m_note_mute: hex(0x353535),
            m_note_text: hex(0x1F1F1F),
            m_key: [hex(0xBABABA), hex(0x282828)],
            m_key_sel: hex(0x8C8C8C),
            m_left_bg: hex(0x323232),
            m_ruler_bg: hex(0x333333),
            m_ruler_fg: hex(0xC8C8C8),
            m_edit_cursor: hex(0x339887),
            m_selbg: a(0xFFFFFF, 0.06),
            marquee_fill: a(0x808080, 0.3),
            marquee_outline: hex(0xFFFFFF),
        }
    }
}

impl ReaperLook {
    /// Lu dans le theme de REAPER : `color` rend une cle de couleur, `value`
    /// la valeur brute d'une cle (un mode de melange : `mode = melange |
    /// ((alpha * 256 + 512) << 8)`). Une cle absente garde sa valeur par
    /// defaut.
    pub fn read(color: &dyn Fn(&str) -> Option<Rgb>, value: &dyn Fn(&str) -> Option<i32>) -> Self {
        let d = ReaperLook::default();
        let c = |k: &str, def: Color| color(k).map_or(def, |(r, g, b)| rgb(r, g, b));
        let alpha = |mode: &str, def: f32| {
            value(mode).map_or(def, |m| {
                ((((m >> 8) & 0x3FF) - 512) as f32 / 256.0).clamp(0.0, 1.0)
            })
        };
        let ca = |k: &str, mode: &str, def: Color| {
            let base = c(k, def);
            base.with_alpha((alpha(mode, def.a as f32 / 255.0) * 255.0).round() as u8)
        };
        ReaperLook {
            arr_bg: c("col_arrangebg", d.arr_bg),
            tr_bg: [c("col_tr1_bg", d.tr_bg[0]), c("col_tr2_bg", d.tr_bg[1])],
            tr_sel_bg: [
                c("selcol_tr1_bg", d.tr_sel_bg[0]),
                c("selcol_tr2_bg", d.tr_sel_bg[1]),
            ],
            tr_div: c("col_tr1_divline", d.tr_div),
            grid_bar: ca("col_gridlines2", "col_gridlines2dm", d.grid_bar),
            grid_beat: ca("col_gridlines3", "col_gridlines3dm", d.grid_beat),
            grid_sub: ca("col_gridlines", "col_gridlines1dm", d.grid_sub),
            item_bg: c("col_mi_bg", d.item_bg),
            item_bg_sel: c("col_tr1_itembgsel", d.item_bg_sel),
            item_border: d.item_border,
            item_border_sel: d.item_border_sel,
            item_label: c("col_mi_label", d.item_label),
            item_label_sel: c("col_mi_label_sel", d.item_label_sel),
            peaks: c("col_tr1_peaks", d.peaks),
            peaks_sel: c("col_tr1_ps2", d.peaks_sel),
            fade_line: c("col_mi_fades", d.fade_line),
            fade_zone: ca("fadezone_color", "fadezone_drawmode", d.fade_zone),
            timesel: ca("col_tl_bgsel", "timesel_drawmode", d.timesel),
            ruler_bg: c("col_tl_bg", d.ruler_bg),
            ruler_fg: c("col_tl_fg", d.ruler_fg),
            ruler_fg2: c("col_tl_fg2", d.ruler_fg2),
            m_bg: [c("midi_trackbg1", d.m_bg[0]), c("midi_trackbg2", d.m_bg[1])],
            m_outer: [
                c("midi_trackbg_outer1", d.m_outer[0]),
                c("midi_trackbg_outer2", d.m_outer[1]),
            ],
            m_oct: c("midioct", d.m_oct),
            m_grid_bar: ca("midi_grid2", "midi_griddm2", d.m_grid_bar),
            m_grid_beat: ca("midi_grid3", "midi_griddm3", d.m_grid_beat),
            m_grid_sub: ca("midi_grid1", "midi_griddm1", d.m_grid_sub),
            m_note_border: c("midi_notebg", d.m_note_border),
            m_note_sel: c("midi_notefg", d.m_note_sel),
            m_note_mute: c("midi_notemute", d.m_note_mute),
            m_note_text: c("midifont_col_dark_unsel", d.m_note_text),
            m_key: [c("midi_pkey1", d.m_key[0]), c("midi_pkey2", d.m_key[1])],
            m_key_sel: c("midi_pkey3", d.m_key_sel),
            m_left_bg: c("midi_leftbg", d.m_left_bg),
            m_ruler_bg: c("midi_rulerbg", d.m_ruler_bg),
            m_ruler_fg: c("midi_rulerfg", d.m_ruler_fg),
            m_edit_cursor: c("midi_editcurs", d.m_edit_cursor),
            m_selbg: ca("midi_selbg", "midi_selbg_drawmode", d.m_selbg),
            marquee_fill: ca("marquee_fill", "marquee_drawmode", d.marquee_fill),
            marquee_outline: c("marquee_outline", d.marquee_outline),
        }
    }
}

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
    /// Le curseur d'edition de l'arrangement (`col_cursor`).
    pub edit_cursor: Color,
    /// Les couleurs de l'arrangement et de l'editeur MIDI de REAPER.
    pub rp: ReaperLook,
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
            edit_cursor: hex(0xDCDDE2),
            rp: ReaperLook::default(),
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
            edit_cursor: c("col_cursor", d.edit_cursor),
            rp: ReaperLook::read(color, &|_| None),
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
