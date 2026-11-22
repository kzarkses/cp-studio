//! La fenetre de l'editeur de CP Studio : un item MIDI (piano roll) ou un
//! clip audio (forme d'onde, rognage, fondus, gain, hauteur, vitesse).
//!
//! Sans Windows et sans REAPER : le module lui donne l'item, la vue rend des
//! [`Act`] (ecrire les notes en un pas d'annulation, deplacer le curseur,
//! faire entendre une note...). Tout se dessine dans un `Canvas` et se teste
//! en PNG.
//!
//! - Le « feeling » de REAPER : la table de modificateurs souris de Cedric
//!   (relevee dans son REAPER, ROADMAP_Editeur.md §2), ses raccourcis de
//!   l'editeur MIDI ([`keys`] : les defauts de REAPER et son reaper-kb.ini,
//!   rendus en actions par [`Ed::command`]) et sa molette.
//! - La vue verticale est une CAMERA : tout le clavier defile, la hauteur des
//!   rangs ne change que sur demande (Ctrl + molette). Elle se cadre sur les
//!   notes a l'ouverture d'un item et sur demande, jamais pendant un geste ;
//!   un geste qui sort de la vue la fait defiler (au bord).
//! - « Notes » ne garde que les rangs joues ; la liste est figee pendant un
//!   geste.
//! - Pas de phrase a l'ecran : les explications sont des infobulles
//!   ([`Ed::label`]).

mod draw;
pub mod keys;
#[cfg(test)]
mod tests;

pub use cp_roll;

use cp_roll::{MIN_LEN, Note, Roll, Scale, note_name};
use cp_ui::{Button, Ev, Mods, vk};

/// Les rangs montres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
    /// Tout le clavier, qui defile (cadre sur les notes a l'ouverture).
    Fit,
    /// Seulement les rangs joues.
    Notes,
}

/// La couleur des notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colors {
    /// Par canal (une guitare de Guitar Tab : par corde).
    Channel,
    /// Par hauteur : les octaves partagent la couleur.
    Pitch,
    Velocity,
}

impl Colors {
    pub fn key(self) -> &'static str {
        match self {
            Colors::Channel => "channel",
            Colors::Pitch => "pitch",
            Colors::Velocity => "velocity",
        }
    }

    pub fn from_key(s: &str) -> Colors {
        match s {
            "pitch" => Colors::Pitch,
            "velocity" => Colors::Velocity,
            _ => Colors::Channel,
        }
    }

    pub fn next(self) -> Colors {
        match self {
            Colors::Channel => Colors::Pitch,
            Colors::Pitch => Colors::Velocity,
            Colors::Velocity => Colors::Channel,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Colors::Channel => "by channel",
            Colors::Pitch => "by pitch",
            Colors::Velocity => "by velocity",
        }
    }
}

/// Les pas de grille (noires).
pub const GRIDS: [(&str, f64); 9] = [
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/4", 1.0),
    ("1/8", 0.5),
    ("1/16", 0.25),
    ("1/32", 0.125),
    ("1/64", 0.0625),
    ("1/8T", 1.0 / 3.0),
    ("1/16T", 1.0 / 6.0),
];

pub fn grid_name(g: f64) -> &'static str {
    GRIDS
        .iter()
        .find(|x| (x.1 - g).abs() < 1e-9)
        .map_or("grid", |x| x.0)
}

/// La grille d'une action « Grid: Set to ... » de l'editeur MIDI (noires).
fn grid_of(cmd: u32) -> Option<f64> {
    Some(match cmd {
        40204 | 41015 => 4.0,
        40203 | 41014 => 2.0,
        40201 | 41013 => 1.0,
        40197 | 41012 => 0.5,
        40192 | 41011 => 0.25,
        40190 | 41010 => 0.125,
        41020 | 41009 => 0.0625,
        41019 | 41008 => 0.03125,
        40205 | 41016 => 8.0,
        40207 | 41017 => 16.0,
        40202 => 4.0 / 3.0,
        40199 => 2.0 / 3.0,
        40193 => 1.0 / 3.0,
        40191 => 1.0 / 6.0,
        40189 => 1.0 / 12.0,
        _ => return None,
    })
}

/// Allonge les notes choisies (REAPER : rien sans selection).
fn lengthen(r: &mut Roll, d: f64) -> usize {
    let k = r.sel_count();
    if k > 0 {
        r.resize_sel(d);
    }
    k
}

/// Un item MIDI ouvert.
#[derive(Clone, Debug, Default)]
pub struct Midi {
    pub roll: Roll,
    pub name: String,
    /// Le GUID de l'item.
    pub item: String,
    /// L'item dans le projet (noires).
    pub qn0: f64,
    pub qn1: f64,
}

/// Un clip audio ouvert. Les durees sont en noires du projet.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub name: String,
    pub item: String,
    pub qn0: f64,
    pub qn1: f64,
    pub gain_db: f32,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Demi-tons, et vitesse de lecture.
    pub pitch: f64,
    pub rate: f64,
    pub reverse: bool,
    /// La forme d'onde : (min, max) par colonne sur `pk.0 .. pk.1` noires.
    pub peaks: Vec<(f32, f32)>,
    pub pk: (f64, f64, usize),
}

#[derive(Clone, Debug, Default)]
pub enum Clip {
    #[default]
    None,
    Midi(Midi),
    Audio(Audio),
}

/// La partie d'une note sous la souris.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Body,
    Left,
    Right,
}

/// Une cible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Ruler,
    /// Un rang du clavier.
    Key(i32),
    Note {
        id: u64,
        part: Part,
    },
    /// La grille vide.
    Grid,
    /// La barre de velocite d'une note.
    Vel(u64),
    VelLane,
    VScroll,
    HScroll,
    // La barre
    Snap,
    GridMenu,
    ScaleMenu,
    Color,
    Rows,
    Quantize,
    Transform,
    VelToggle,
    Follow,
    /// Entendre les notes qu'on touche.
    Hear,
    Help,
    // Le clip audio
    Wave,
    EdgeL,
    EdgeR,
    FadeIn,
    FadeOut,
    Gain,
    GainChip,
    PitchChip,
    RateChip,
    Reverse,
    Normalize,
}

/// Ce que le module doit faire.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Ecrire les notes dans l'item, en un pas d'annulation.
    Commit(&'static str),
    Seek(f64),
    Play,
    Undo,
    Redo,
    /// Faire entendre une note (tenue jusqu'a `AuditionOff` si `hold`).
    Audition {
        pitch: i32,
        chan: i32,
        vel: i32,
        hold: bool,
    },
    AuditionOff,
    GridMenu {
        x: f32,
        y: f32,
    },
    ScaleMenu {
        x: f32,
        y: f32,
    },
    TransformMenu {
        x: f32,
        y: f32,
    },
    /// Clic droit sur la barre : la place de la fenetre.
    ViewMenu {
        x: f32,
        y: f32,
    },
    /// Ouvrir l'item dans l'editeur MIDI de REAPER.
    OpenReaper,
    /// Clic droit : le menu des notes, sous la souris.
    ContextMenu {
        x: f32,
        y: f32,
    },
    /// Une action de la fenetre principale de REAPER (le transport).
    Main(u32),
    /// Fermer l'editeur (« File: Close window »).
    Close,
    /// Le clip audio : poser ses valeurs (pendant le geste, sans annulation),
    /// puis un pas d'annulation a la fin.
    AudioLive,
    AudioCommit(&'static str),
    AudioReverse,
    AudioNormalize,
}

/// Une cible de la barre : rectangle et ce qu'elle est.
pub type HBox = (f32, f32, f32, f32, Hit);

/// Les zones (calculees au dessin).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lay {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    /// Le haut de la regle.
    pub top: f32,
    pub ruler_h: f32,
    /// Le contenu qui defile : son bord gauche et sa largeur.
    pub sx: f32,
    pub sw: f32,
    /// La zone des rangs (ou de l'onde) : son haut et sa hauteur.
    pub gy: f32,
    pub gh: f32,
    /// La couche de velocite.
    pub vel_y: f32,
    pub vel_h: f32,
    /// La barre horizontale.
    pub hs_y: f32,
    pub hs_h: f32,
    /// Pixels par noire, hauteur d'un rang, hauteur de tous les rangs.
    pub ppq: f32,
    pub rh: f32,
    pub content_h: f32,
}

#[derive(Clone, Debug)]
enum Drag {
    /// Un appui pas encore decide (clic ou glisser) : sur une note (`id`) ou
    /// sur la grille vide.
    Press {
        x0: f32,
        y0: f32,
        id: Option<u64>,
        mods: Mods,
        right: bool,
    },
    Move {
        x0: f32,
        y0: f32,
        /// La noire sous la souris a l'appui (le geste se compte en noires :
        /// le defilement au bord ne le fausse pas).
        qp: f64,
        base: Vec<Note>,
        /// La note saisie (l'arpege s'ecarte autour d'elle).
        id: u64,
        /// Le debut de la note saisie, et le rang de reference de la souris.
        anchor_q: f64,
        anchor_p: i32,
        /// La hauteur de la note saisie (celle qu'on entend).
        pitch0: i32,
        snap: bool,
        /// 0 libre, 1 temps seul, 2 hauteur seule, 3 a decider.
        axis: u8,
        copy: bool,
        /// Les hauteurs hors de la gamme (REAPER : « ignoring scale/key »).
        free: bool,
        /// Ecarter les debuts selon la hauteur (« arpeggiate »).
        arp: bool,
        heard: i32,
    },
    /// Peindre des notes et des accords : une case par pas de grille, et
    /// chaque hauteur traversee dans un meme pas fait un accord.
    Chords {
        cells: Vec<(f64, i32)>,
        /// La derniere case : (pas, hauteur).
        at: Option<(f64, i32)>,
    },
    Edge {
        qp: f64,
        base: Vec<Note>,
        id: u64,
        left: bool,
        snap: bool,
        solo: bool,
        stretch: bool,
    },
    Insert {
        id: u64,
        q0: f64,
        heard: i32,
    },
    Erase {
        any: bool,
    },
    /// Le lasso : son coin de depart dans le contenu (noire, et ordonnee
    /// dans les rangs), son coin courant a l'ecran.
    Marquee {
        q0: f64,
        cy0: f32,
        x1: f32,
        y1: f32,
        add: bool,
    },
    Paint {
        q0: f64,
        p0: i32,
        q1: f64,
        p1: i32,
    },
    Vel {
        base: Vec<Note>,
        id: u64,
    },
    Range {
        q0: f64,
    },
    Hand {
        y0: f32,
        v0: f32,
        z0: f32,
        q: f64,
        ruler: bool,
        zooming: bool,
    },
    HScroll {
        x0: f32,
        s0: f64,
    },
    VScroll {
        y0: f32,
        v0: f32,
    },
    /// Le clip audio : la poignee, la souris et la valeur au depart.
    Audio {
        what: Hit,
        x0: f32,
        y0: f32,
        v0: f64,
    },
}

/// L'aide : les gestes (l'infobulle du « ? »). Les touches sont celles de
/// l'editeur MIDI de REAPER.
pub const HELP: &str = "Notes, as in REAPER\n\
Click: select   \u{00b7}   Shift: range   \u{00b7}   Ctrl: toggle   \u{00b7}   Alt: erase\n\
Shift+Alt: the measure   \u{00b7}   Ctrl+Alt: and the later notes\n\
Shift+Win: double length   \u{00b7}   Ctrl+Win: halve\n\
Drag: move   \u{00b7}   Shift: no snap   \u{00b7}   Ctrl: copy   \u{00b7}   Shift+Ctrl: one axis\n\
Shift+Ctrl+Alt drag: pitch out of the scale   \u{00b7}   Ctrl+Win drag: arpeggiate\n\
Edges: length   \u{00b7}   Ctrl: this note only   \u{00b7}   Alt: stretch\n\
\n\
Empty grid\n\
Click or drag: a new note   \u{00b7}   Ctrl: cursor, drag copies the selection\n\
Alt: erase   \u{00b7}   Ctrl+Alt drag: a line   \u{00b7}   Shift+Ctrl+Alt drag: notes and chords\n\
Right drag: lasso   \u{00b7}   right click: menu\n\
\n\
Keys: your REAPER MIDI editor shortcuts\n\
\n\
View\n\
Wheel: scroll   \u{00b7}   Alt+wheel: time   \u{00b7}   Shift+wheel: zoom   \u{00b7}   Ctrl+wheel: row height\n\
Middle drag: move the view (on the ruler, up/down zooms)";

pub struct Ed {
    pub clip: Clip,
    pub sig: (i32, i32),
    pub bpm: f64,
    /// Pixels par noire a 96 DPI.
    pub zoom: f32,
    /// La noire au bord gauche.
    pub scroll: f64,
    pub vscroll: f32,
    pub rows_mode: Rows,
    pub colors: Colors,
    pub snap: bool,
    pub grid: f64,
    /// La quantification : force et swing (0..1).
    pub strength: f64,
    pub swing: f64,
    pub vel_lane: bool,
    pub follow: bool,
    pub playhead: Option<f64>,
    pub playing: bool,
    /// Le curseur d'edition de REAPER (montre aussi pendant la lecture).
    pub edit_cursor: Option<f64>,
    /// La ligne verticale a la souris (les reglages de REAPER) et sa place.
    pub guide: cp_ui::Guide,
    pub guide_q: Option<f64>,
    /// Le passage choisi sur la regle (noires).
    pub range: Option<(f64, f64)>,
    pub status: String,
    pub lay: Lay,
    pub scale: f32,
    /// Entendre la note qu'on touche (par la piste de l'item).
    pub hear: bool,
    /// La hauteur d'un rang (px a 96 DPI) : elle ne change que sur demande
    /// (Ctrl + molette, un cadrage).
    pub row_h: f32,
    /// Le cadrage vertical a faire des que la hauteur de la vue est connue.
    want_fit: bool,
    /// ... ou seulement ramener les notes a l'ecran si aucune n'y est.
    want_reveal: bool,
    /// Les rangs montres, du haut vers le bas.
    pub(crate) rows: Vec<i32>,
    pub(crate) boxes: Vec<HBox>,
    pub(crate) hover: Hit,
    drag: Option<Drag>,
    /// La derniere position de la souris (le defilement au bord, les actions
    /// « sous la souris »).
    mouse: (f32, f32, Mods),
    last_len: f64,
    last_vel: i32,
    last_chan: i32,
    dirty: bool,
    /// Les zones a redessiner quand tout ne l'est pas (un survol).
    damage: Vec<cp_gfx::IRect>,
}

/// La hauteur d'un rang par defaut, et ses bornes (px a 96 DPI).
pub const ROW_H: f32 = 14.0;
const ROW_MIN: f32 = 5.0;
const ROW_MAX: f32 = 40.0;

impl Default for Ed {
    fn default() -> Self {
        Ed::new()
    }
}

const EPS: f64 = 1e-9;

impl Ed {
    pub fn new() -> Ed {
        Ed {
            clip: Clip::None,
            sig: (4, 4),
            bpm: 120.0,
            zoom: 80.0,
            scroll: 0.0,
            vscroll: 0.0,
            rows_mode: Rows::Fit,
            colors: Colors::Channel,
            snap: true,
            grid: 0.25,
            strength: 1.0,
            swing: 0.0,
            vel_lane: true,
            follow: true,
            playhead: None,
            playing: false,
            range: None,
            status: String::new(),
            lay: Lay::default(),
            scale: 1.0,
            hear: true,
            row_h: ROW_H,
            want_fit: true,
            want_reveal: false,
            rows: Vec::new(),
            boxes: Vec::new(),
            hover: Hit::None,
            drag: None,
            mouse: (0.0, 0.0, Mods::default()),
            last_len: 0.25,
            last_vel: 100,
            last_chan: 0,
            dirty: true,
            edit_cursor: None,
            guide: cp_ui::Guide::default(),
            guide_q: None,
            damage: Vec::new(),
        }
    }

    /// Tout est a redessiner.
    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        self.damage.clear();
        std::mem::take(&mut self.dirty)
    }

    /// Quelque chose est a redessiner (tout, ou des zones).
    pub fn needs_draw(&self) -> bool {
        self.dirty || !self.damage.is_empty()
    }

    /// Les zones de la scene a redessiner : toute la vue, ou celles d'un
    /// survol ; vide : rien.
    pub fn take_damage(&mut self) -> Vec<cp_gfx::IRect> {
        if std::mem::take(&mut self.dirty) {
            self.damage.clear();
            let l = self.lay;
            return vec![cp_gfx::IRect::new(0, 0, l.w.ceil() as i32, l.h.ceil() as i32)];
        }
        let v = cp_gfx::layers::merge(std::mem::take(&mut self.damage));
        if v.len() > 4 {
            return vec![v.iter().fold(cp_gfx::IRect::default(), |a, r| a.union(r))];
        }
        v
    }

    /// La cible survolee change : seules ses deux places se redessinent.
    fn set_hover(&mut self, h: Hit) {
        if h == self.hover {
            return;
        }
        let pad = (3.0 * self.scale).ceil();
        let old = self.hover;
        for r in [self.rect_of(old), self.rect_of(h)].into_iter().flatten() {
            self.damage.push(cp_gfx::IRect::new(
                (r.0 - pad).floor() as i32,
                (r.1 - pad).floor() as i32,
                (r.2 + 2.0 * pad).ceil() as i32 + 1,
                (r.3 + 2.0 * pad).ceil() as i32 + 1,
            ));
        }
        if (old != Hit::None && self.rect_of(old).is_none())
            || (h != Hit::None && self.rect_of(h).is_none())
        {
            self.dirty = true;
        }
        self.hover = h;
    }

    /// Les traits qui bougent seuls par-dessus la scene : la ligne a la
    /// souris, le curseur d'edition, la tete de lecture (un pixel, comme
    /// REAPER), de la regle au bas de la grille.
    pub fn strokes(&self, th: &cp_ui::Theme) -> Vec<cp_gfx::layers::Stroke> {
        use cp_gfx::layers::Stroke;
        let l = self.lay;
        let (top, bot) = (l.top as i32, l.hs_y as i32);
        let mut out = Vec::new();
        let mut line = |q: f64, c: cp_gfx::Color| {
            let x = self.x_of(q).round();
            if x >= l.sx && x < l.sx + l.sw && bot > top {
                out.push(Stroke {
                    r: cp_gfx::IRect::new(x as i32, top, 1, bot - top),
                    c,
                });
            }
        };
        if let (true, Some(q)) = (self.guide.on, self.guide_q) {
            line(q, self.guide.color.unwrap_or(th.text_faint));
        }
        if let Some(q) = self.edit_cursor {
            line(q, th.rp.m_edit_cursor);
        }
        if let (true, Some(q)) = (self.playing, self.playhead) {
            line(q, th.playhead);
        }
        out
    }

    /// Une edition au survol (les touches de l'arrangement de Cedric : Q, D,
    /// S, Z, A, E) : sur la note sous la souris, a la souris calee sur la
    /// grille (Maj : sans) ; dans un clip audio, ses bords et ses fondus.
    pub fn hover_edit(&mut self, e: cp_ui::HoverEdit) -> Vec<Act> {
        use cp_ui::HoverEdit as H;
        self.dirty = true;
        let (mx, my, m) = self.mouse;
        let q = self.snap_near(self.qn_of(mx), !m.shift);
        if let Some(a) = self.audio_mut() {
            let (q0, q1) = (a.qn0, a.qn1);
            if q <= q0 + 1e-6 || q >= q1 - 1e-6 {
                return Vec::new();
            }
            let label = match e {
                H::TrimLeft => {
                    a.qn0 = q;
                    a.fade_in = a.fade_in.min(q1 - q);
                    "CP Editor: trim clip start"
                }
                H::TrimRight => {
                    a.qn1 = q;
                    a.fade_out = a.fade_out.min(q - q0);
                    "CP Editor: trim clip end"
                }
                H::FadeIn => {
                    a.fade_in = q - q0;
                    "CP Editor: fade in"
                }
                H::FadeOut => {
                    a.fade_out = q1 - q;
                    "CP Editor: fade out"
                }
                H::Split | H::Delete => return Vec::new(),
            };
            return vec![Act::AudioLive, Act::AudioCommit(label)];
        }
        match e {
            H::Delete => self.named_cmd(keys::Named::DeleteUnderMouse),
            H::Split => self.named_cmd(keys::Named::Split),
            H::TrimLeft | H::TrimRight => {
                let Some((id, _)) = self.note_at(mx, my) else {
                    return Vec::new();
                };
                let left = e == H::TrimLeft;
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                // La note sous la souris ; choisie, toutes les choisies que la
                // souris traverse.
                let sel = md.roll.get(id).is_some_and(|n| n.sel);
                let mut k = 0;
                for n in md.roll.notes.iter_mut() {
                    let on = if sel { n.sel } else { n.id == id };
                    let end = n.end();
                    if !on || q <= n.qn0 + 1e-6 || q >= end - 1e-6 {
                        continue;
                    }
                    if left {
                        n.qn0 = q;
                        n.len = end - q;
                    } else {
                        n.len = q - n.qn0;
                    }
                    k += 1;
                }
                self.commit_if(
                    k,
                    if left {
                        "CP Editor: trim note start"
                    } else {
                        "CP Editor: trim note end"
                    },
                )
            }
            H::FadeIn | H::FadeOut => Vec::new(),
        }
    }

    /// Le curseur de REAPER a montrer : celui du geste en cours, sinon celui
    /// que ferait un geste ici avec ces touches (Ctrl sur une note : la
    /// copie ; Alt : la gomme ; un bord : la longueur...), comme l'editeur MIDI
    /// de REAPER et la table souris de Cedric.
    pub fn pointer(&self, x: f32, y: f32, m: Mods) -> cp_ui::Pointer {
        use cp_ui::Pointer as P;
        if let Some(d) = &self.drag {
            match d {
                Drag::Hand { .. } => return P::HandScroll,
                Drag::Marquee { .. } => return P::Marquee,
                Drag::Move {
                    copy, axis, arp, ..
                } => {
                    return if *arp {
                        P::NoteArp
                    } else if *copy {
                        P::NoteCopy
                    } else {
                        match axis {
                            1 => P::NoteMoveH,
                            2 => P::NoteMoveV,
                            _ => P::Arrow,
                        }
                    };
                }
                Drag::Edge { left, stretch, .. } => {
                    return match (*left, *stretch) {
                        (true, false) => P::NoteEdgeLeft,
                        (false, false) => P::NoteEdgeRight,
                        (true, true) => P::NoteStretchLeft,
                        (false, true) => P::NoteStretchRight,
                    };
                }
                Drag::Insert { .. } => return P::NoteDraw,
                Drag::Paint { .. } => return P::NotePaint,
                Drag::Chords { .. } => return P::NoteChord,
                Drag::Erase { .. } => return P::NoteErase,
                Drag::Vel { .. } => return P::NoteVel,
                Drag::Range { .. } => return P::TimeSelEdge,
                Drag::HScroll { .. } | Drag::VScroll { .. } => return P::Arrow,
                Drag::Audio { what, .. } => return Self::audio_pointer(*what),
                Drag::Press { .. } => {}
            }
        }
        match self.hit(x, y) {
            Hit::Note { part, .. } => match part {
                Part::Left if m.alt => P::NoteStretchLeft,
                Part::Right if m.alt => P::NoteStretchRight,
                Part::Left => P::NoteEdgeLeft,
                Part::Right => P::NoteEdgeRight,
                Part::Body => match (m.shift, m.ctrl, m.alt, m.win) {
                    (true, true, true, false) => P::NoteMoveV,
                    (_, true, false, true) => P::NoteArp,
                    (false, true, false, false) => P::NoteCopy,
                    (_, false, true, false) => P::NoteErase,
                    _ => P::Arrow,
                },
            },
            Hit::Grid => match (m.shift, m.ctrl, m.alt) {
                (true, true, true) => P::NoteChord,
                (false, true, true) => P::NotePaint,
                (_, false, true) => P::NoteErase,
                (false, true, false) => P::NoteCopy,
                _ => P::NoteDraw,
            },
            Hit::Vel(_) | Hit::VelLane => P::NoteVel,
            h @ (Hit::Wave
            | Hit::EdgeL
            | Hit::EdgeR
            | Hit::FadeIn
            | Hit::FadeOut
            | Hit::Gain) => Self::audio_pointer(h),
            _ => P::Arrow,
        }
    }

    fn audio_pointer(h: Hit) -> cp_ui::Pointer {
        use cp_ui::Pointer as P;
        match h {
            Hit::EdgeL => P::TrimLeft,
            Hit::EdgeR => P::TrimRight,
            Hit::FadeIn => P::FadeIn,
            Hit::FadeOut => P::FadeOut,
            Hit::Gain => P::ItemVol,
            Hit::Wave => P::IBeam,
            _ => P::Arrow,
        }
    }

    /// La ligne a la souris : sur la grille et la regle, calee sur la grille
    /// si le magnetisme est la (la ou un clic poserait le curseur).
    fn guide_at(&mut self, x: f32, y: f32) {
        let l = self.lay;
        let over = x >= l.sx && x < l.sx + l.sw && y >= l.top && y < l.hs_y;
        self.guide_q = if self.guide.on && over && !matches!(self.clip, Clip::None) {
            let q = self.qn_of(x);
            Some(self.snap_near(q, self.guide.snaps(self.mouse.2)))
        } else {
            None
        };
    }

    /// Une position calee sur la ligne de grille la plus proche (le curseur
    /// d'edition, la ligne a la souris : comme REAPER).
    pub fn snap_near(&self, q: f64, snap: bool) -> f64 {
        if snap && self.snap && self.grid > 0.0 {
            (q / self.grid).round() * self.grid
        } else {
            q
        }
    }

    pub fn midi(&self) -> Option<&Midi> {
        match &self.clip {
            Clip::Midi(m) => Some(m),
            _ => None,
        }
    }

    pub fn midi_mut(&mut self) -> Option<&mut Midi> {
        match &mut self.clip {
            Clip::Midi(m) => Some(m),
            _ => None,
        }
    }

    pub fn audio(&self) -> Option<&Audio> {
        match &self.clip {
            Clip::Audio(a) => Some(a),
            _ => None,
        }
    }

    pub fn audio_mut(&mut self) -> Option<&mut Audio> {
        match &mut self.clip {
            Clip::Audio(a) => Some(a),
            _ => None,
        }
    }

    /// L'item ouvert (son GUID).
    pub fn item(&self) -> Option<&str> {
        match &self.clip {
            Clip::Midi(m) => Some(&m.item),
            Clip::Audio(a) => Some(&a.item),
            Clip::None => None,
        }
    }

    /// Ouvre un item MIDI. `fresh` : un autre item que celui d'avant (la vue
    /// se recadre ; sinon elle reste ou elle est).
    pub fn open_midi(&mut self, m: Midi, fresh: bool) {
        if let Some(n) = m.roll.notes.first() {
            self.last_chan = n.chan;
        }
        self.clip = Clip::Midi(m);
        if fresh {
            self.fit();
        } else {
            // Le meme item relu (un geste ecrit, une annulation, une
            // modification ailleurs) : la camera ne bouge pas, sauf si plus
            // aucune note n'est a l'ecran.
            self.want_reveal = true;
        }
        self.drag = None;
        self.dirty = true;
    }

    pub fn open_audio(&mut self, a: Audio, fresh: bool) {
        self.clip = Clip::Audio(a);
        if fresh {
            self.fit();
        }
        self.drag = None;
        self.dirty = true;
    }

    pub fn close(&mut self) {
        self.clip = Clip::None;
        self.drag = None;
        self.dirty = true;
    }

    /// Un geste est-il en cours (le module ne recharge pas l'item pendant).
    pub fn busy(&self) -> bool {
        self.drag.is_some()
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn hovered(&self) -> Hit {
        self.hover
    }

    /// Les noires d'une mesure.
    pub fn bar_qn(&self) -> f64 {
        self.sig.0.max(1) as f64 * 4.0 / self.sig.1.max(1) as f64
    }

    /// L'item dans le projet (noires).
    pub fn span(&self) -> Option<(f64, f64)> {
        match &self.clip {
            Clip::Midi(m) => Some((m.qn0, m.qn1)),
            Clip::Audio(a) => Some((a.qn0, a.qn1)),
            Clip::None => None,
        }
    }

    /// Le cadrage : tout l'item dans la largeur ; en hauteur, les notes au
    /// milieu (fait au prochain dessin, quand la hauteur de la vue est
    /// connue). Seulement a l'ouverture d'un item et sur demande.
    pub fn fit(&mut self) {
        self.want_fit = true;
        if let Some((a, b)) = self.span() {
            let w = if self.lay.sw > 10.0 {
                self.lay.sw
            } else {
                1000.0 * self.scale
            };
            let len = (b - a).max(1.0);
            self.zoom = ((w / self.scale.max(0.5)) / (len as f32 * 1.06)).clamp(6.0, 800.0);
            self.lay.ppq = self.zoom * self.scale;
            self.scroll = (a - len * 0.03).max(0.0);
        }
        self.dirty = true;
    }

    // -----------------------------------------------------------------------
    // La geometrie
    // -----------------------------------------------------------------------

    pub fn x_of(&self, qn: f64) -> f32 {
        self.lay.sx + ((qn - self.scroll) as f32) * self.lay.ppq
    }

    pub fn qn_of(&self, x: f32) -> f64 {
        self.scroll + ((x - self.lay.sx) / self.lay.ppq.max(1e-3)) as f64
    }

    pub fn view_qn(&self) -> f64 {
        (self.lay.sw / self.lay.ppq.max(1e-3)) as f64
    }

    /// Les rangs : du plus aigu au plus grave. Tout le clavier, ou (mode
    /// Notes) les hauteurs jouees. Ne depend des notes qu'en mode Notes, et
    /// la vue ne les recalcule pas pendant un geste.
    pub(crate) fn compute_rows(&self) -> Vec<i32> {
        let Some(m) = self.midi() else {
            return Vec::new();
        };
        if self.rows_mode == Rows::Notes && !m.roll.notes.is_empty() {
            let mut used: Vec<i32> = m.roll.notes.iter().map(|n| n.pitch).collect();
            used.sort_unstable();
            used.dedup();
            used.reverse();
            return used;
        }
        (0..=127).rev().collect()
    }

    /// Les hauteurs extremes des notes (ou de la selection), ou rien.
    fn note_span(&self, sel_only: bool) -> Option<(i32, i32)> {
        let m = self.midi()?;
        let it = m.roll.notes.iter().filter(|n| !sel_only || n.sel);
        let (lo, hi) = it.fold((i32::MAX, i32::MIN), |(a, b), n| {
            (a.min(n.pitch), b.max(n.pitch))
        });
        (lo <= hi).then_some((lo, hi))
    }

    /// Le cadrage vertical, la hauteur de la vue etant connue : les rangs de
    /// `lo` a `hi` (et deux de marge) remplissent la vue, dans les bornes de
    /// hauteur ; l'ensemble est centre.
    pub(crate) fn frame_rows(&mut self, lo: i32, hi: i32) {
        let s = self.scale.max(0.5);
        let gh = self.lay.gh;
        if gh <= 10.0 || self.rows.is_empty() {
            return;
        }
        let (a, b) = (
            self.row_of(hi + 2).or(self.row_of(hi)),
            self.row_of(lo - 2).or(self.row_of(lo)),
        );
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            _ => (0, self.rows.len() - 1),
        };
        let n = (b - a + 1).max(12) as f32;
        let rh = (gh / n).clamp(ROW_MIN * s, 22.0 * s);
        self.row_h = rh / s;
        self.lay.rh = rh;
        self.lay.content_h = self.rows.len() as f32 * rh;
        let mid = (a + b + 1) as f32 * 0.5 * rh;
        self.vscroll = (mid - gh * 0.5).clamp(0.0, self.max_vscroll());
        self.dirty = true;
    }

    /// Apres le calcul de la vue : le cadrage demande, ou les notes ramenees
    /// a l'ecran si aucune n'y est (sans changer la hauteur des rangs).
    pub(crate) fn apply_camera(&mut self) {
        if self.lay.gh <= 10.0 || self.midi().is_none() {
            return;
        }
        if std::mem::take(&mut self.want_fit) {
            self.want_reveal = false;
            let (lo, hi) = self.note_span(false).unwrap_or((54, 66));
            self.frame_rows(lo, hi);
            return;
        }
        if std::mem::take(&mut self.want_reveal) {
            let Some(m) = self.midi() else { return };
            let any = m.roll.notes.iter().any(|n| {
                self.row_of(n.pitch).is_some_and(|i| {
                    let y = self.row_y(i);
                    y + self.lay.rh > self.lay.gy && y < self.lay.gy + self.lay.gh
                })
            });
            if !any && !m.roll.notes.is_empty() {
                self.reveal(None);
            }
        }
    }

    /// Fait defiler (sans zoomer) pour montrer les notes (ou la selection).
    fn reveal(&mut self, sel: Option<bool>) {
        let Some((lo, hi)) = self.note_span(sel.unwrap_or(false)) else {
            return;
        };
        let (Some(a), Some(b)) = (self.row_of(hi), self.row_of(lo)) else {
            return;
        };
        let rh = self.lay.rh.max(1.0);
        let (ya, yb) = (a.min(b) as f32 * rh, (a.max(b) + 1) as f32 * rh);
        let gh = self.lay.gh;
        let v = self.vscroll;
        let nv = if yb - ya > gh {
            (ya + yb) * 0.5 - gh * 0.5
        } else if ya < v {
            ya - rh
        } else if yb > v + gh {
            yb - gh + rh
        } else {
            v
        };
        self.vscroll = nv.clamp(0.0, self.max_vscroll());
        self.dirty = true;
    }

    /// Le haut d'un rang, au pixel pres (des bords entiers : des traits nets
    /// d'un pixel, et le remplissage rapide).
    pub(crate) fn row_y(&self, idx: usize) -> f32 {
        (self.lay.gy + idx as f32 * self.lay.rh - self.vscroll).round()
    }

    /// La hauteur a l'ecran du rang `idx` (jusqu'au haut du suivant).
    pub(crate) fn row_h(&self, idx: usize) -> f32 {
        self.row_y(idx + 1) - self.row_y(idx)
    }

    pub(crate) fn row_of(&self, pitch: i32) -> Option<usize> {
        self.rows.iter().position(|&p| p == pitch)
    }

    /// Le rang sous `y` (borne aux rangs montres).
    pub(crate) fn idx_at(&self, y: f32) -> usize {
        let k = ((y - self.lay.gy + self.vscroll) / self.lay.rh.max(1.0)).floor();
        (k.max(0.0) as usize).min(self.rows.len().saturating_sub(1))
    }

    pub(crate) fn pitch_at(&self, y: f32) -> i32 {
        self.rows.get(self.idx_at(y)).copied().unwrap_or(60)
    }

    /// Le rectangle d'une note : x, y, largeur, hauteur.
    pub(crate) fn note_rect(&self, n: &Note) -> Option<(f32, f32, f32, f32)> {
        let idx = self.row_of(n.pitch)?;
        let x0 = self.x_of(n.qn0).round();
        let x1 = self.x_of(n.end()).round().max(x0 + 2.0);
        Some((x0, self.row_y(idx), x1 - x0, self.row_h(idx)))
    }

    fn max_vscroll(&self) -> f32 {
        (self.lay.content_h - self.lay.gh).max(0.0)
    }

    /// La fin du contenu (noires), pour la barre horizontale.
    pub fn end_qn(&self) -> f64 {
        let v = self.view_qn();
        let e = self
            .span()
            .map_or(0.0, |s| s.1)
            .max(self.playhead.unwrap_or(0.0));
        (e + v * 0.5).max(v).max(self.scroll + v)
    }

    pub fn hthumb(&self) -> (f32, f32) {
        let l = self.lay;
        let end = self.end_qn().max(1e-6);
        let tw = ((self.view_qn() / end) as f32 * l.sw).clamp((24.0 * self.scale).min(l.sw), l.sw);
        let room = (l.sw - tw).max(0.0);
        let maxs = (end - self.view_qn()).max(1e-6);
        let t = (self.scroll / maxs).clamp(0.0, 1.0) as f32;
        (l.sx + room * t, tw)
    }

    fn zoom_at(&mut self, z: f32, q: f64, x: f32) {
        self.zoom = z.clamp(4.0, 1600.0);
        self.lay.ppq = self.zoom * self.scale;
        self.scroll = (q - ((x - self.lay.sx) / self.lay.ppq) as f64).max(0.0);
        self.dirty = true;
    }

    fn unfollow(&mut self) {
        if self.playing {
            self.follow = false;
        }
        self.dirty = true;
    }

    /// Cale `q` sur la grille (au plus pres).
    fn snap_q(&self, q: f64, snap: bool) -> f64 {
        if snap && self.snap && self.grid > 0.0 {
            (q / self.grid).round() * self.grid
        } else {
            q
        }
    }

    /// La case sous `q` (une note posee tombe dans la case du pointeur).
    fn snap_floor(&self, q: f64, snap: bool) -> f64 {
        if snap && self.snap && self.grid > 0.0 {
            (q / self.grid + 1e-4).floor() * self.grid
        } else {
            q
        }
    }

    /// Le battement : la tete de lecture suit, la page tourne.
    pub fn set_playhead(&mut self, qn: Option<f64>, playing: bool) {
        if qn != self.playhead || playing != self.playing {
            // Un trait par-dessus la scene : il ne la salit que si la fin du
            // contenu (la barre horizontale) bouge.
            let end = self.end_qn();
            self.playhead = qn;
            self.playing = playing;
            if (self.end_qn() - end).abs() > 1e-9 {
                self.dirty = true;
            }
        }
        if let (true, true, Some(p)) = (self.follow, playing, qn) {
            let v = self.view_qn();
            if v > 0.0 && (p < self.scroll || p > self.scroll + v * 0.85) {
                self.scroll = (p - v * 0.1).max(0.0);
                self.dirty = true;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Les cibles
    // -----------------------------------------------------------------------

    pub fn rect_of(&self, h: Hit) -> Option<(f32, f32, f32, f32)> {
        if let Some(b) = self.boxes.iter().rev().find(|b| b.4 == h) {
            return Some((b.0, b.1, b.2, b.3));
        }
        if let Hit::Note { id, .. } = h {
            let n = self.midi()?.roll.get(id)?;
            return self.note_rect(n);
        }
        None
    }

    /// La note sous la souris (la plus haute dans l'ordre du dessin).
    fn note_at(&self, x: f32, y: f32) -> Option<(u64, Part)> {
        let m = self.midi()?;
        for n in m.roll.notes.iter().rev() {
            let Some((nx, ny, nw, nh)) = self.note_rect(n) else {
                continue;
            };
            if x >= nx && x < nx + nw && y >= ny && y < ny + nh {
                let hw = (nw * 0.3).clamp(1.5 * self.scale, 6.0 * self.scale);
                let part = if nw < 5.0 * self.scale {
                    Part::Body
                } else if x < nx + hw {
                    Part::Left
                } else if x >= nx + nw - hw {
                    Part::Right
                } else {
                    Part::Body
                };
                return Some((n.id, part));
            }
        }
        None
    }

    /// La barre de velocite la plus proche (les choisies d'abord).
    fn vel_at(&self, x: f32) -> Option<u64> {
        let m = self.midi()?;
        let tol = 6.0 * self.scale;
        m.roll
            .notes
            .iter()
            .map(|n| (n, (self.x_of(n.qn0) - x).abs()))
            .filter(|(_, d)| *d <= tol)
            .min_by(|a, b| (!a.0.sel).cmp(&!b.0.sel).then(a.1.total_cmp(&b.1)))
            .map(|(n, _)| n.id)
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if let Some(b) = self
            .boxes
            .iter()
            .rev()
            .find(|b| x >= b.0 && x < b.0 + b.2 && y >= b.1 && y < b.1 + b.3)
        {
            return b.4;
        }
        let l = self.lay;
        if y < l.top {
            return Hit::None;
        }
        if y >= l.hs_y && y < l.hs_y + l.hs_h && x >= l.sx && x < l.sx + l.sw {
            return Hit::HScroll;
        }
        if y < l.top + l.ruler_h {
            return if x >= l.sx { Hit::Ruler } else { Hit::None };
        }
        match &self.clip {
            Clip::Midi(_) => {
                let gb = l.gy + l.gh;
                if y >= l.gy && y < gb {
                    if x >= l.sx + l.sw {
                        return if self.max_vscroll() > 0.0 {
                            Hit::VScroll
                        } else {
                            Hit::None
                        };
                    }
                    if x < l.sx {
                        return Hit::Key(self.pitch_at(y));
                    }
                    return match self.note_at(x, y) {
                        Some((id, part)) => Hit::Note { id, part },
                        None => Hit::Grid,
                    };
                }
                if l.vel_h > 0.0 && y >= l.vel_y && y < l.vel_y + l.vel_h && x >= l.sx {
                    return match self.vel_at(x) {
                        Some(id) => Hit::Vel(id),
                        None => Hit::VelLane,
                    };
                }
                Hit::None
            }
            Clip::Audio(a) => {
                if y < l.gy || y >= l.gy + l.gh || x < l.sx {
                    return Hit::None;
                }
                let (xa, xb) = (self.x_of(a.qn0), self.x_of(a.qn1));
                let hs = 7.0 * self.scale;
                let fi = self.x_of(a.qn0 + a.fade_in);
                let fo = self.x_of(a.qn1 - a.fade_out);
                if y < l.gy + 16.0 * self.scale {
                    if (x - fi).abs() <= hs {
                        return Hit::FadeIn;
                    }
                    if (x - fo).abs() <= hs {
                        return Hit::FadeOut;
                    }
                }
                if (x - xa).abs() <= hs * 0.8 {
                    return Hit::EdgeL;
                }
                if (x - xb).abs() <= hs * 0.8 {
                    return Hit::EdgeR;
                }
                if x > xa && x < xb {
                    let gy = self.gain_y(a.gain_db);
                    if (y - gy).abs() <= 5.0 * self.scale {
                        return Hit::Gain;
                    }
                    return Hit::Wave;
                }
                Hit::None
            }
            Clip::None => Hit::None,
        }
    }

    // -----------------------------------------------------------------------
    // Le clip audio : le gain sur la hauteur
    // -----------------------------------------------------------------------

    /// La place d'un gain sur la hauteur de l'onde (0 dB aux trois quarts).
    pub(crate) fn gain_y(&self, db: f32) -> f32 {
        let l = self.lay;
        let pos = if db <= -60.0 {
            0.0
        } else {
            ((db + 60.0) / 72.0).clamp(0.0, 1.0).powf(1.0 / 0.75)
        };
        l.gy + l.gh * (1.0 - pos)
    }

    fn gain_of_y(&self, y: f32) -> f32 {
        let l = self.lay;
        let t = (1.0 - (y - l.gy) / l.gh.max(1.0)).clamp(0.0, 1.0);
        if t <= 0.005 {
            -150.0
        } else {
            ((-60.0 + 72.0 * t.powf(0.75)) * 10.0).round() / 10.0
        }
    }

    /// Les crêtes que la vue voudrait : (debut, fin, colonnes), ou rien si
    /// celles qu'elle a suffisent.
    pub fn peaks_wanted(&self) -> Option<(f64, f64, usize)> {
        let a = self.audio()?;
        let q0 = a.qn0.max(self.scroll);
        let q1 = a.qn1.min(self.scroll + self.view_qn());
        if q1 <= q0 {
            return None;
        }
        let cols = ((q1 - q0) as f32 * self.lay.ppq).ceil().max(1.0) as usize;
        let same = (a.pk.0 - q0).abs() < 1e-6 && (a.pk.1 - q1).abs() < 1e-6 && a.pk.2 == cols;
        (!same).then_some((q0, q1, cols))
    }

    pub fn set_peaks(&mut self, q0: f64, q1: f64, peaks: Vec<(f32, f32)>) {
        if let Some(a) = self.audio_mut() {
            a.pk = (q0, q1, peaks.len());
            a.peaks = peaks;
        }
        self.dirty = true;
    }

    // -----------------------------------------------------------------------
    // Les evenements
    // -----------------------------------------------------------------------

    pub fn event(&mut self, ev: &Ev) -> Vec<Act> {
        match *ev {
            Ev::Move { x, y, mods } => self.on_move(x, y, mods),
            Ev::Leave => {
                self.set_hover(Hit::None);
                self.guide_q = None;
                Vec::new()
            }
            Ev::Down {
                x,
                y,
                b: Button::Middle,
                ..
            } => {
                let l = self.lay;
                if x >= l.sx && y >= l.top && y < l.hs_y {
                    self.drag = Some(Drag::Hand {
                        y0: y,
                        v0: self.vscroll,
                        z0: self.zoom,
                        q: self.qn_of(x),
                        ruler: y < l.top + l.ruler_h,
                        zooming: false,
                    });
                }
                Vec::new()
            }
            Ev::Down { x, y, b, dbl, mods } => self.on_down(x, y, b, dbl, mods),
            Ev::Up { x, y, b, mods } => self.on_up(x, y, b, mods),
            Ev::Wheel {
                x, y, dx, dy, mods, ..
            } => self.on_wheel(x, y, dx, dy, mods),
            Ev::Key { vk: k, mods, .. } => self.key(k, mods),
            Ev::Char(_) => Vec::new(),
        }
    }

    fn on_wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32, mods: Mods) -> Vec<Act> {
        let l = self.lay;
        let _ = y;
        let x = x.clamp(l.sx, l.sx + l.sw);
        if dx != 0.0 {
            if mods.alt || mods.ctrl {
                let k = if mods.ctrl { -dx } else { dx };
                let q = self.qn_of(x);
                self.zoom_at(self.zoom * 1.25f32.powf(k), q, x);
            } else {
                self.scroll = (self.scroll + dx as f64 * self.view_qn() * 0.15).max(0.0);
            }
        } else if mods.shift {
            let q = self.qn_of(x);
            self.zoom_at(self.zoom * 1.25f32.powf(dy), q, x);
        } else if mods.alt {
            self.scroll = (self.scroll - dy as f64 * self.view_qn() * 0.15).max(0.0);
        } else if mods.ctrl {
            // La hauteur des rangs, autour du rang pointe.
            if self.midi().is_some() {
                let before = (y - l.gy + self.vscroll) / l.rh.max(1.0);
                let rh =
                    (l.rh * 1.15f32.powf(dy)).clamp(ROW_MIN * self.scale, ROW_MAX * self.scale);
                self.row_h = rh / self.scale;
                self.lay.rh = rh;
                self.lay.content_h = self.rows.len() as f32 * rh;
                self.vscroll = (before * rh - (y - l.gy)).clamp(0.0, self.max_vscroll());
                self.dirty = true;
            }
            return Vec::new();
        } else {
            self.vscroll = (self.vscroll - dy * 3.0 * l.rh.max(6.0)).clamp(0.0, self.max_vscroll());
            self.dirty = true;
            return Vec::new();
        }
        self.unfollow();
        Vec::new()
    }

    fn on_down(&mut self, x: f32, y: f32, b: Button, dbl: bool, m: Mods) -> Vec<Act> {
        let h = self.hit(x, y);
        self.dirty = true;
        if b == Button::Right {
            // Clic droit : le menu ; glisser : le lasso (comme REAPER).
            return match h {
                Hit::Note { id, .. } => {
                    self.drag = Some(Drag::Press {
                        x0: x,
                        y0: y,
                        id: Some(id),
                        mods: m,
                        right: true,
                    });
                    Vec::new()
                }
                Hit::Grid => {
                    self.drag = Some(Drag::Press {
                        x0: x,
                        y0: y,
                        id: None,
                        mods: m,
                        right: true,
                    });
                    Vec::new()
                }
                Hit::Key(p) => self.audition(p, false),
                _ if y < self.lay.bar_h => vec![Act::ViewMenu { x, y }],
                _ => Vec::new(),
            };
        }
        if b != Button::Left {
            return Vec::new();
        }
        match h {
            Hit::Note { id, part } => self.down_note(x, y, id, part, m),
            Hit::Grid => self.down_grid(x, y, m),
            Hit::Key(p) => {
                if let Some(md) = self.midi_mut() {
                    md.roll.select_row(p, m.ctrl);
                }
                self.audition(p, true)
            }
            Hit::Vel(id) => {
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                if !md.roll.get(id).is_some_and(|n| n.sel) {
                    md.roll.select_only(id);
                }
                let base = md.roll.notes.clone();
                self.drag = Some(Drag::Vel { base, id });
                self.vel_drag(y);
                Vec::new()
            }
            Hit::Ruler => {
                let q = self.qn_of(x);
                if m.alt {
                    self.range = None;
                    return Vec::new();
                }
                if m.shift {
                    if let (Some((a, b)), Some(md)) = (self.range, self.midi_mut()) {
                        md.roll.select_in(a, b, false);
                    }
                    return Vec::new();
                }
                // Sur la regle, c'est Ctrl qui ignore le magnetisme (REAPER) ;
                // la ligne de grille la plus proche.
                let q = self.snap_near(q, !m.ctrl).max(0.0);
                self.range = None;
                self.drag = Some(Drag::Range { q0: q });
                vec![Act::Seek(q)]
            }
            Hit::VScroll => {
                self.drag = Some(Drag::VScroll {
                    y0: y,
                    v0: self.vscroll,
                });
                Vec::new()
            }
            Hit::HScroll => {
                let (tx, tw) = self.hthumb();
                if x < tx || x > tx + tw {
                    let end = self.end_qn();
                    let k = end / self.lay.sw.max(1.0) as f64;
                    self.scroll = ((x - self.lay.sx - tw * 0.5) as f64 * k).clamp(0.0, end);
                    self.unfollow();
                }
                self.drag = Some(Drag::HScroll {
                    x0: x,
                    s0: self.scroll,
                });
                Vec::new()
            }
            Hit::Snap => {
                self.snap = !self.snap;
                Vec::new()
            }
            Hit::GridMenu => vec![Act::GridMenu { x, y }],
            Hit::ScaleMenu => vec![Act::ScaleMenu { x, y }],
            Hit::Transform => vec![Act::TransformMenu { x, y }],
            Hit::Color => {
                self.colors = self.colors.next();
                Vec::new()
            }
            Hit::Rows => {
                self.rows_mode = match self.rows_mode {
                    Rows::Fit => Rows::Notes,
                    Rows::Notes => Rows::Fit,
                };
                // Un changement de vue demande : on recadre.
                self.want_fit = true;
                Vec::new()
            }
            Hit::Hear => {
                self.hear = !self.hear;
                if self.hear {
                    Vec::new()
                } else {
                    vec![Act::AuditionOff]
                }
            }
            Hit::Quantize => self.quantize(),
            Hit::VelToggle => {
                self.vel_lane = !self.vel_lane;
                Vec::new()
            }
            Hit::Follow => {
                self.follow = !self.follow;
                Vec::new()
            }
            Hit::Help | Hit::None | Hit::VelLane => Vec::new(),
            // Le clip audio.
            Hit::Wave => vec![Act::Seek(self.snap_q(self.qn_of(x), !m.shift))],
            Hit::EdgeL | Hit::EdgeR | Hit::FadeIn | Hit::FadeOut | Hit::Gain => {
                let Some(a) = self.audio() else {
                    return Vec::new();
                };
                if dbl && h == Hit::Gain {
                    if let Some(a) = self.audio_mut() {
                        a.gain_db = 0.0;
                    }
                    return vec![Act::AudioLive, Act::AudioCommit("CP Editor: gain")];
                }
                let v0 = match h {
                    Hit::EdgeL => a.qn0,
                    Hit::EdgeR => a.qn1,
                    Hit::FadeIn => a.fade_in,
                    Hit::FadeOut => a.fade_out,
                    _ => a.gain_db as f64,
                };
                self.drag = Some(Drag::Audio {
                    what: h,
                    x0: x,
                    y0: y,
                    v0,
                });
                Vec::new()
            }
            Hit::GainChip | Hit::PitchChip | Hit::RateChip => {
                let Some(a) = self.audio_mut() else {
                    return Vec::new();
                };
                if dbl {
                    match h {
                        Hit::GainChip => a.gain_db = 0.0,
                        Hit::PitchChip => a.pitch = 0.0,
                        _ => a.rate = 1.0,
                    }
                    return vec![Act::AudioLive, Act::AudioCommit("CP Editor: reset")];
                }
                let v0 = match h {
                    Hit::GainChip => a.gain_db as f64,
                    Hit::PitchChip => a.pitch,
                    _ => a.rate,
                };
                self.drag = Some(Drag::Audio {
                    what: h,
                    x0: x,
                    y0: y,
                    v0,
                });
                Vec::new()
            }
            Hit::Reverse => {
                if let Some(a) = self.audio_mut() {
                    a.reverse = !a.reverse;
                }
                vec![Act::AudioReverse]
            }
            Hit::Normalize => vec![Act::AudioNormalize],
        }
    }

    fn audition(&self, pitch: i32, hold: bool) -> Vec<Act> {
        if !self.hear {
            return Vec::new();
        }
        let (chan, vel) = self
            .midi()
            .and_then(|m| m.roll.notes.iter().find(|n| n.pitch == pitch))
            .map_or((self.last_chan, self.last_vel), |n| (n.chan, n.vel));
        vec![Act::Audition {
            pitch,
            chan,
            vel,
            hold,
        }]
    }

    fn down_note(&mut self, x: f32, y: f32, id: u64, part: Part, m: Mods) -> Vec<Act> {
        let qp = self.qn_of(x);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let Some(n) = md.roll.get(id).copied() else {
            return Vec::new();
        };
        // Les bords : la longueur (Ctrl : cette note seule ; Alt : etirer).
        if part != Part::Body {
            if !n.sel {
                md.roll.select_only(id);
            }
            let base = md.roll.notes.clone();
            self.drag = Some(Drag::Edge {
                qp,
                base,
                id,
                left: part == Part::Left,
                snap: !m.shift,
                solo: m.ctrl,
                stretch: m.alt,
            });
            return Vec::new();
        }
        // Alt seul : effacer (un clic efface la note, glisser efface ce qu'on
        // traverse).
        if m.alt && !m.shift && !m.ctrl && !m.win {
            md.roll.delete(id);
            self.drag = Some(Drag::Erase { any: true });
            return Vec::new();
        }
        // Sans modificateur, une note pas choisie l'est tout de suite : la
        // glisser deplace elle seule.
        if m == Mods::default() && !n.sel {
            md.roll.select_only(id);
        }
        self.last_vel = n.vel;
        self.last_chan = n.chan;
        self.last_len = n.len;
        // Le clic et le glisser font deux choses differentes : on attend.
        self.drag = Some(Drag::Press {
            x0: x,
            y0: y,
            id: Some(id),
            mods: m,
            right: false,
        });
        self.audition(n.pitch, true)
    }

    fn down_grid(&mut self, x: f32, y: f32, m: Mods) -> Vec<Act> {
        let q = self.qn_of(x);
        let p = self.pitch_at(y);
        let (s, c, a) = (m.shift, m.ctrl, m.alt);
        // Ctrl+Alt : une ligne de notes.
        if c && a && !s {
            let q0 = self.snap_floor(q, true);
            self.drag = Some(Drag::Paint {
                q0,
                p0: p,
                q1: q0,
                p1: p,
            });
            return Vec::new();
        }
        // Alt seul : tout deselectionner ; glisser efface.
        if a && !s && !c {
            if let Some(md) = self.midi_mut() {
                md.roll.clear_sel();
            }
            self.drag = Some(Drag::Erase { any: false });
            return Vec::new();
        }
        // Ctrl, Maj+Alt, Maj+Ctrl+Alt : le clic et le glisser different.
        if c || a {
            self.drag = Some(Drag::Press {
                x0: x,
                y0: y,
                id: None,
                mods: m,
                right: false,
            });
            return Vec::new();
        }
        // Une note neuve dans la case du pointeur (Maj : sans magnetisme) ;
        // glisser regle sa longueur, et sa hauteur tant qu'elle est neuve.
        let Some(id) = self.insert_note(q, p, !s) else {
            return Vec::new();
        };
        let q0 = self
            .midi()
            .and_then(|md| md.roll.get(id))
            .map_or(q, |n| n.qn0);
        self.drag = Some(Drag::Insert { id, q0, heard: p });
        self.audition(p, true)
    }

    /// Une note neuve, choisie seule : rend son identifiant.
    fn insert_note(&mut self, q: f64, p: i32, snap: bool) -> Option<u64> {
        let q0 = self.snap_floor(q, snap).max(0.0);
        let len = if self.snap {
            self.grid.max(self.last_len.min(self.grid * 4.0))
        } else {
            self.last_len
        };
        let (chan, vel) = (self.last_chan, self.last_vel);
        let md = self.midi_mut()?;
        let id = md.roll.insert(q0, len, p, chan, vel);
        md.roll.select_only(id);
        Some(id)
    }

    /// Le geste decide sur une note : selon les modificateurs de l'appui.
    #[allow(clippy::too_many_arguments)]
    fn start_note_drag(
        &mut self,
        x0: f32,
        y0: f32,
        id: u64,
        m: Mods,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> Vec<Act> {
        // (magnetisme, axe, copie, hors gamme, arpege)
        let g = match (m.shift, m.ctrl, m.alt, m.win) {
            (false, false, false, _) => (true, 0, false, false, false),
            (true, false, false, _) => (false, 0, false, false, false),
            (false, true, false, false) => (true, 0, true, false, false),
            (true, true, false, false) => (true, 3, false, false, false),
            (true, true, true, false) => (true, 2, false, true, false),
            (false, true, false, true) => (true, 0, false, false, true),
            (true, true, false, true) => (false, 0, false, false, true),
            // Maj+Alt, Ctrl+Alt : des clics, rien a glisser.
            _ => {
                self.drag = None;
                return vec![Act::AuditionOff];
            }
        };
        let (snap, axis, copy, free, arp) = g;
        let qp = self.qn_of(x0);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let Some(n) = md.roll.get(id).copied() else {
            return Vec::new();
        };
        if !n.sel {
            md.roll.add_sel(id);
        }
        if copy {
            md.roll.stamp_sel();
        }
        let base = md.roll.notes.clone();
        self.drag = Some(Drag::Move {
            x0,
            y0,
            qp,
            base,
            id,
            anchor_q: n.qn0,
            anchor_p: n.pitch,
            pitch0: n.pitch,
            snap,
            axis,
            copy,
            free,
            arp,
            heard: n.pitch,
        });
        self.on_move(x, y, mods)
    }

    /// Le geste decide sur la grille vide.
    #[allow(clippy::too_many_arguments)]
    fn start_grid_drag(
        &mut self,
        x0: f32,
        y0: f32,
        m: Mods,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> Vec<Act> {
        let (s, c, a) = (m.shift, m.ctrl, m.alt);
        if s && c && a {
            // Des notes et des accords.
            self.drag = Some(Drag::Chords {
                cells: Vec::new(),
                at: None,
            });
            self.chords_add(x0, y0);
            return self.on_move(x, y, mods);
        }
        if a {
            // Maj+Alt : effacer.
            if let Some(md) = self.midi_mut() {
                md.roll.clear_sel();
            }
            self.drag = Some(Drag::Erase { any: false });
            return self.on_move(x, y, mods);
        }
        // Ctrl : la selection copiee, menee par la souris ; l'ecart entre
        // l'appui et la premiere note reste, son debut se cale.
        let qp = self.qn_of(x0);
        let p_at = self.pitch_at(y0);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let Some(f) = md
            .roll
            .notes
            .iter()
            .filter(|n| n.sel)
            .min_by(|a, b| a.qn0.total_cmp(&b.qn0))
            .copied()
        else {
            self.drag = None;
            return Vec::new();
        };
        md.roll.stamp_sel();
        let base = md.roll.notes.clone();
        self.drag = Some(Drag::Move {
            x0,
            y0,
            qp,
            base,
            id: f.id,
            anchor_q: f.qn0,
            anchor_p: p_at,
            pitch0: f.pitch,
            snap: !s,
            axis: 0,
            copy: true,
            free: false,
            arp: false,
            heard: f.pitch,
        });
        self.on_move(x, y, mods)
    }

    /// Peindre des notes et des accords : la case du pointeur, et dans un
    /// meme pas toutes les hauteurs traversees.
    fn chords_add(&mut self, x: f32, y: f32) {
        let q = self.snap_floor(self.qn_of(x), true).max(0.0);
        let p = self.pitch_at(y);
        let notes_mode = self.rows_mode == Rows::Notes;
        let shown = self.rows.clone();
        let Some(Drag::Chords { cells, at }) = &mut self.drag else {
            return;
        };
        let from = at.filter(|c| (c.0 - q).abs() < 1e-9).map(|c| c.1);
        let span: Vec<i32> = match from {
            Some(f) if f <= p => (f..=p).collect(),
            Some(f) => (p..=f).collect(),
            None => vec![p],
        };
        for pp in span {
            if notes_mode && !shown.contains(&pp) {
                continue;
            }
            if !cells.iter().any(|c| (c.0 - q).abs() < 1e-9 && c.1 == pp) {
                cells.push((q, pp));
            }
        }
        *at = Some((q, p));
    }

    fn on_move(&mut self, x: f32, y: f32, mods: Mods) -> Vec<Act> {
        self.mouse = (x, y, mods);
        self.guide_at(x, y);
        let Some(d) = self.drag.clone() else {
            let h = self.hit(x, y);
            self.set_hover(h);
            return Vec::new();
        };
        self.dirty = true;
        match d {
            Drag::Press {
                x0,
                y0,
                id,
                mods: m,
                right,
            } => {
                if (x - x0).abs() < 4.0 * self.scale && (y - y0).abs() < 4.0 * self.scale {
                    return Vec::new();
                }
                if right {
                    self.drag = Some(Drag::Marquee {
                        q0: self.qn_of(x0),
                        cy0: y0 - self.lay.gy + self.vscroll,
                        x1: x,
                        y1: y,
                        add: m.ctrl,
                    });
                    return Vec::new();
                }
                match id {
                    Some(id) => self.start_note_drag(x0, y0, id, m, x, y, mods),
                    None => self.start_grid_drag(x0, y0, m, x, y, mods),
                }
            }
            Drag::Move {
                x0,
                y0,
                qp,
                base,
                id,
                anchor_q,
                anchor_p,
                pitch0,
                snap,
                axis,
                copy,
                free,
                arp,
                heard,
            } => {
                let mut axis = axis;
                if axis == 3 {
                    let (dx, dy) = ((x - x0).abs(), (y - y0).abs());
                    if dx.max(dy) > 6.0 * self.scale {
                        axis = if dx >= dy { 1 } else { 2 };
                    }
                }
                // En noires (pas en pixels) : le defilement au bord garde la
                // note sous la souris.
                let raw = anchor_q + (self.qn_of(x) - qp);
                let mut dq = self.snap_q(raw, snap) - anchor_q;
                let mut dp = self.pitch_at(y) - anchor_p;
                match axis {
                    1 => dp = 0,
                    2 => dq = 0.0,
                    3 => {
                        dp = 0;
                        dq = 0.0;
                    }
                    _ => {}
                }
                if arp {
                    dp = 0;
                }
                let now = (pitch0 + dp).clamp(0, 127);
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                md.roll.notes = base.clone();
                if arp {
                    md.roll.arpeggiate(id, dq);
                } else if free {
                    md.roll.move_sel_free(dq, dp);
                } else {
                    md.roll.move_sel(dq, dp);
                }
                self.drag = Some(Drag::Move {
                    x0,
                    y0,
                    qp,
                    base,
                    id,
                    anchor_q,
                    anchor_p,
                    pitch0,
                    snap,
                    axis,
                    copy,
                    free,
                    arp,
                    heard: now,
                });
                if now != heard {
                    return self.audition(now, true);
                }
                Vec::new()
            }
            Drag::Edge {
                qp,
                base,
                id,
                left,
                snap,
                solo,
                stretch,
            } => {
                let dq_raw = self.qn_of(x) - qp;
                let Some(n) = base.iter().find(|n| n.id == id).copied() else {
                    return Vec::new();
                };
                let sel: Vec<&Note> = base.iter().filter(|b| b.sel).collect();
                let g0 = sel.iter().map(|b| b.qn0).fold(f64::INFINITY, f64::min);
                let g1 = sel
                    .iter()
                    .map(|b| b.end())
                    .fold(f64::NEG_INFINITY, f64::max);
                let target_end = self.snap_q(n.end() + dq_raw, snap);
                let target_start = self.snap_q(n.qn0 + dq_raw, snap);
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                md.roll.notes = base.clone();
                if solo {
                    if let Some(nn) = md.roll.get_mut(id) {
                        if left {
                            let end = nn.end();
                            nn.qn0 = target_start.clamp(0.0, end - MIN_LEN);
                            nn.len = end - nn.qn0;
                        } else {
                            nn.len = (target_end - nn.qn0).max(MIN_LEN);
                        }
                    }
                    md.roll.sort();
                } else if stretch {
                    if left {
                        let span = (g1 - n.qn0).max(EPS);
                        let k = (g1 - target_start) / span;
                        md.roll.stretch_sel(g1, k.max(0.05));
                    } else {
                        let span = (n.end() - g0).max(EPS);
                        let k = (target_end - g0) / span;
                        md.roll.stretch_sel(g0, k.max(0.05));
                    }
                } else if left {
                    md.roll.trim_start_sel(target_start - n.qn0);
                } else {
                    md.roll.resize_sel(target_end - n.end());
                }
                Vec::new()
            }
            Drag::Insert { id, q0, heard } => {
                let q = self.qn_of(x);
                let end = if self.snap && !mods.shift && self.grid > 0.0 {
                    ((q / self.grid).ceil() * self.grid).max(q0 + self.grid)
                } else {
                    q.max(q0 + MIN_LEN)
                };
                let p = self.pitch_at(y);
                let keep = self.last_len;
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                if let Some(n) = md.roll.get_mut(id) {
                    n.len = (end - q0).max(MIN_LEN);
                    n.pitch = p;
                }
                md.roll.sort();
                let len = md.roll.get(id).map_or(keep, |n| n.len);
                self.last_len = len;
                self.drag = Some(Drag::Insert { id, q0, heard: p });
                if p != heard {
                    return self.audition(p, true);
                }
                Vec::new()
            }
            Drag::Erase { any } => {
                let mut any = any;
                if let Some((id, _)) = self.note_at(x, y) {
                    if let Some(md) = self.midi_mut() {
                        any |= md.roll.delete(id);
                    }
                }
                self.drag = Some(Drag::Erase { any });
                Vec::new()
            }
            Drag::Marquee { q0, cy0, add, .. } => {
                self.drag = Some(Drag::Marquee {
                    q0,
                    cy0,
                    x1: x,
                    y1: y,
                    add,
                });
                Vec::new()
            }
            Drag::Paint { q0, p0, .. } => {
                let q1 = self.snap_floor(self.qn_of(x), !mods.shift);
                let p1 = self.pitch_at(y);
                self.drag = Some(Drag::Paint { q0, p0, q1, p1 });
                Vec::new()
            }
            Drag::Chords { .. } => {
                self.chords_add(x, y);
                Vec::new()
            }
            Drag::Vel { .. } => {
                self.vel_drag(y);
                Vec::new()
            }
            Drag::Range { q0 } => {
                let q = self.snap_q(self.qn_of(x), !mods.ctrl).max(0.0);
                self.range = Some((q0.min(q), q0.max(q)));
                Vec::new()
            }
            Drag::Hand {
                y0,
                v0,
                z0,
                q,
                ruler,
                zooming,
            } => {
                let dy = y - y0;
                if ruler {
                    let zooming = zooming || dy.abs() > 6.0 * self.scale;
                    let z = if zooming {
                        z0 * (-dy / (120.0 * self.scale)).exp2()
                    } else {
                        z0
                    };
                    self.zoom_at(z, q, x);
                    self.drag = Some(Drag::Hand {
                        y0,
                        v0,
                        z0,
                        q,
                        ruler,
                        zooming,
                    });
                } else {
                    self.zoom_at(self.zoom, q, x);
                    self.vscroll = (v0 - dy).clamp(0.0, self.max_vscroll());
                }
                self.unfollow();
                Vec::new()
            }
            Drag::HScroll { x0, s0 } => {
                let end = self.end_qn();
                let k = end / self.lay.sw.max(1.0) as f64;
                self.scroll = (s0 + (x - x0) as f64 * k).clamp(0.0, end);
                self.unfollow();
                Vec::new()
            }
            Drag::VScroll { y0, v0 } => {
                let k = self.lay.content_h / self.lay.gh.max(1.0);
                self.vscroll = (v0 + (y - y0) * k).clamp(0.0, self.max_vscroll());
                Vec::new()
            }
            Drag::Audio { what, x0, y0, v0 } => self.audio_drag(what, x - x0, y, y0, v0, mods),
        }
    }

    /// Le battement : pendant un geste, la souris hors de la grille fait
    /// defiler la vue (comme REAPER), et le geste suit. Rien d'autre ne
    /// bouge la camera pendant un geste.
    pub fn tick(&mut self) -> Vec<Act> {
        let vertical = matches!(
            self.drag,
            Some(
                Drag::Move { .. }
                    | Drag::Insert { .. }
                    | Drag::Marquee { .. }
                    | Drag::Paint { .. }
                    | Drag::Chords { .. }
                    | Drag::Erase { .. }
            )
        );
        let horizontal =
            vertical || matches!(self.drag, Some(Drag::Edge { .. } | Drag::Range { .. }));
        if !horizontal || self.midi().is_none() {
            return Vec::new();
        }
        let (x, y, m) = self.mouse;
        let l = self.lay;
        let s = self.scale.max(0.5);
        let speed = |d: f32, max: f32| d.signum() * (d.abs() * 0.35).clamp(2.0 * s, max * s);
        let mut moved = false;
        if vertical {
            let d = if y < l.gy {
                y - l.gy
            } else if y > l.gy + l.gh {
                y - (l.gy + l.gh)
            } else {
                0.0
            };
            if d != 0.0 {
                let v = (self.vscroll + speed(d, 30.0)).clamp(0.0, self.max_vscroll());
                if (v - self.vscroll).abs() > 0.01 {
                    self.vscroll = v;
                    moved = true;
                }
            }
        }
        let d = if x < l.sx {
            x - l.sx
        } else if x > l.sx + l.sw {
            x - (l.sx + l.sw)
        } else {
            0.0
        };
        if d != 0.0 {
            let dq = (speed(d, 40.0) / l.ppq.max(1e-3)) as f64;
            let ns = (self.scroll + dq).max(0.0);
            if (ns - self.scroll).abs() > 1e-9 {
                self.scroll = ns;
                moved = true;
            }
        }
        if !moved {
            return Vec::new();
        }
        self.dirty = true;
        self.on_move(x, y, m)
    }
    /// La velocite sous la souris ; un groupe choisi garde ses ecarts.
    fn vel_drag(&mut self, y: f32) {
        let l = self.lay;
        let v = (((l.vel_y + l.vel_h - 4.0 * self.scale - y)
            / (l.vel_h - 10.0 * self.scale).max(1.0))
            * 127.0)
            .round()
            .clamp(1.0, 127.0) as i32;
        let Some(Drag::Vel { base, id, .. }) = self.drag.clone() else {
            return;
        };
        let Some(md) = self.midi_mut() else { return };
        let Some(b) = base.iter().find(|n| n.id == id) else {
            return;
        };
        let d = v - b.vel;
        md.roll.notes = base.clone();
        for n in md.roll.notes.iter_mut().filter(|n| n.sel) {
            n.vel = (n.vel + d).clamp(1, 127);
        }
        self.last_vel = v;
    }

    fn audio_drag(&mut self, what: Hit, dx: f32, y: f32, y0: f32, v0: f64, m: Mods) -> Vec<Act> {
        let dq = (dx / self.lay.ppq.max(1e-3)) as f64;
        let snap = !m.shift;
        let gain = self.gain_of_y(y);
        let ppq = self.lay.ppq;
        let sq_l = self.snap_q(v0 + dq, snap);
        let Some(a) = self.audio_mut() else {
            return Vec::new();
        };
        match what {
            Hit::EdgeL => a.qn0 = sq_l.clamp(0.0, a.qn1 - MIN_LEN),
            Hit::EdgeR => a.qn1 = sq_l.max(a.qn0 + MIN_LEN),
            Hit::FadeIn => a.fade_in = (v0 + dq).clamp(0.0, a.qn1 - a.qn0),
            Hit::FadeOut => a.fade_out = (v0 - dq).clamp(0.0, a.qn1 - a.qn0),
            Hit::Gain => a.gain_db = gain,
            Hit::GainChip => {
                a.gain_db = ((v0 as f32 + dx * 0.1 - (y - y0) * 0.1) * 10.0).round() / 10.0;
                a.gain_db = a.gain_db.clamp(-60.0, 24.0);
            }
            Hit::PitchChip => a.pitch = (v0 + (dx / 10.0).round() as f64).clamp(-48.0, 48.0),
            Hit::RateChip => {
                a.rate = ((v0 + dx as f64 * 0.005) * 100.0).round() / 100.0;
                a.rate = a.rate.clamp(0.1, 4.0);
            }
            _ => {}
        }
        let _ = ppq;
        vec![Act::AudioLive]
    }

    fn on_up(&mut self, x: f32, y: f32, b: Button, _mods: Mods) -> Vec<Act> {
        self.mouse.0 = x;
        self.mouse.1 = y;
        let Some(d) = self.drag.take() else {
            return Vec::new();
        };
        self.dirty = true;
        let _ = b;
        match d {
            // Un clic sur une note, selon les modificateurs (REAPER).
            Drag::Press {
                id: Some(id),
                mods: m,
                right,
                x0,
                ..
            } => {
                let bar = self.bar_qn();
                let q_mouse = self.qn_of(x0).max(0.0);
                let q_snap = self.snap_near(q_mouse, true).max(0.0);
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                let Some(n) = md.roll.get(id).copied() else {
                    return vec![Act::AuditionOff];
                };
                if right {
                    if !n.sel {
                        md.roll.select_only(id);
                    }
                    return vec![Act::AuditionOff, Act::ContextMenu { x, y }];
                }
                let mut out = vec![Act::AuditionOff];
                match (m.shift, m.ctrl, m.alt, m.win) {
                    // Choisir la note ; le curseur la ou l'on clique (cale sur
                    // la grille), pas au debut de la note.
                    (false, false, false, false) => {
                        md.roll.select_only(id);
                        out.push(Act::Seek(q_snap));
                    }
                    (true, false, false, false) => md.roll.select_range_to(id),
                    (false, true, false, false) => md.roll.toggle(id),
                    // ... le curseur sous la souris, sans magnetisme.
                    (true, true, false, false) => {
                        md.roll.select_only(id);
                        out.push(Act::Seek(q_mouse));
                    }
                    (true, false, true, false) => {
                        md.roll.select_measure(n.qn0, bar);
                    }
                    (false, true, true, false) => {
                        md.roll.select_from(n.qn0);
                    }
                    // Doubler (Maj+Win) ou diviser (Ctrl+Win) la longueur.
                    (s, c, false, true) if s != c => {
                        if !n.sel {
                            md.roll.select_only(id);
                        }
                        let k = if s { 2.0 } else { 0.5 };
                        if md.roll.scale_len(k) > 0 {
                            out.push(Act::Commit(if s {
                                "CP Editor: double length"
                            } else {
                                "CP Editor: halve length"
                            }));
                        }
                    }
                    _ => {}
                }
                out
            }
            // Un clic sur la grille vide.
            Drag::Press {
                id: None,
                mods: m,
                right,
                x0,
                y0,
            } => {
                if right {
                    return vec![Act::ContextMenu { x, y }];
                }
                let q = self.qn_of(x0);
                let p = self.pitch_at(y0);
                if m.ctrl && !m.alt {
                    // Tout deselectionner, le curseur sous la souris (sans
                    // magnetisme).
                    if let Some(md) = self.midi_mut() {
                        md.roll.clear_sel();
                    }
                    return vec![Act::Seek(q.max(0.0))];
                }
                if m.alt {
                    // Maj+Alt : une note sans magnetisme ; Maj+Ctrl+Alt : une
                    // note.
                    if self.insert_note(q, p, m.ctrl).is_some() {
                        let mut v = self.audition(p, false);
                        v.push(Act::Commit("CP Editor: insert note"));
                        return v;
                    }
                }
                Vec::new()
            }
            Drag::Move { base, copy, .. } => {
                let same = self.midi().is_some_and(|m| m.roll.notes == base);
                let mut v = vec![Act::AuditionOff];
                if !same || copy {
                    v.push(Act::Commit(if copy {
                        "CP Editor: copy notes"
                    } else {
                        "CP Editor: move notes"
                    }));
                }
                v
            }
            Drag::Edge { base, .. } => {
                if self.midi().is_some_and(|m| m.roll.notes == base) {
                    Vec::new()
                } else {
                    vec![Act::Commit("CP Editor: note length")]
                }
            }
            Drag::Insert { .. } => vec![Act::AuditionOff, Act::Commit("CP Editor: insert note")],
            Drag::Erase { any } => {
                if any {
                    vec![Act::Commit("CP Editor: erase notes")]
                } else {
                    Vec::new()
                }
            }
            Drag::Marquee { q0, cy0, add, .. } => {
                let qb = self.qn_of(x);
                let pa = self.pitch_at(self.lay.gy + cy0 - self.vscroll);
                let pb = self.pitch_at(y);
                if let Some(md) = self.midi_mut() {
                    md.roll.select_box(q0, qb, pa, pb, add);
                }
                Vec::new()
            }
            Drag::Paint { q0, p0, q1, p1 } => {
                let g = if self.grid > 0.0 { self.grid } else { 0.25 };
                let (chan, vel) = (self.last_chan, self.last_vel);
                let (a, b, pa, pb) = if q1 >= q0 {
                    (q0, q1, p0, p1)
                } else {
                    (q1, q0, p1, p0)
                };
                let steps = ((b - a) / g).round() as i64;
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                md.roll.clear_sel();
                for k in 0..=steps {
                    let t = if steps == 0 {
                        0.0
                    } else {
                        k as f64 / steps as f64
                    };
                    let p = (pa as f64 + (pb - pa) as f64 * t).round() as i32;
                    let id = md.roll.insert(a + k as f64 * g, g, p, chan, vel);
                    md.roll.add_sel(id);
                }
                vec![Act::Commit("CP Editor: paint notes")]
            }
            Drag::Chords { cells, .. } => {
                let g = if self.grid > 0.0 { self.grid } else { 0.25 };
                let (chan, vel) = (self.last_chan, self.last_vel);
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                if cells.is_empty() {
                    return Vec::new();
                }
                md.roll.clear_sel();
                for (q, p) in cells {
                    let id = md.roll.insert(q, g, p, chan, vel);
                    md.roll.add_sel(id);
                }
                vec![Act::Commit("CP Editor: paint chords")]
            }
            Drag::Vel { base, .. } => {
                if self.midi().is_some_and(|m| m.roll.notes == base) {
                    Vec::new()
                } else {
                    vec![Act::Commit("CP Editor: velocity")]
                }
            }
            Drag::Range { .. } => {
                if self.range.is_some_and(|(a, b)| b - a < 1e-6) {
                    self.range = None;
                }
                Vec::new()
            }
            Drag::Audio { what, .. } => vec![Act::AudioCommit(match what {
                Hit::EdgeL | Hit::EdgeR => "CP Editor: trim",
                Hit::FadeIn | Hit::FadeOut => "CP Editor: fade",
                Hit::PitchChip => "CP Editor: pitch",
                Hit::RateChip => "CP Editor: rate",
                _ => "CP Editor: gain",
            })],
            Drag::Hand { .. } | Drag::HScroll { .. } | Drag::VScroll { .. } => Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Les touches et les transformations
    // -----------------------------------------------------------------------

    fn commit_if(&self, k: usize, label: &'static str) -> Vec<Act> {
        if k > 0 {
            vec![Act::Commit(label)]
        } else {
            Vec::new()
        }
    }

    pub fn quantize(&mut self) -> Vec<Act> {
        let (g, s, w) = (self.grid, self.strength, self.swing);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let k = md.roll.quantize(g, s, w);
        self.commit_if(k, "CP Editor: quantize")
    }

    /// Le menu des transformations : (identifiant, texte, coche).
    pub fn transform_menu(&self) -> Vec<Option<(u32, String, bool)>> {
        let s = |id: u32, t: &str| Some((id, t.to_string(), false));
        let mut v = vec![
            s(1, "Select all"),
            s(2, "Invert selection"),
            s(3, "Select same pitch"),
            None,
            s(10, "Legato"),
            s(11, "Reverse"),
            s(12, "Mirror pitches"),
            s(13, "Glue"),
            s(14, "Split at cursor"),
            s(15, "Snap to scale"),
            None,
            s(20, "Velocity 127"),
            s(21, "Velocity 100"),
            s(22, "Velocity 80"),
            s(23, "Velocity 64"),
            s(24, "Velocity ramp up"),
            s(25, "Velocity ramp down"),
            s(26, "Velocity compress"),
            s(27, "Velocity expand"),
            None,
            s(30, "Humanize light"),
            s(31, "Humanize medium"),
            s(32, "Humanize heavy"),
            None,
            s(50, "Double length"),
            s(51, "Halve length"),
            s(52, "Length = grid"),
            None,
        ];
        for (k, p) in [100, 75, 50, 25].into_iter().enumerate() {
            v.push(Some((
                40 + k as u32,
                format!("Quantize strength {p} %"),
                (self.strength - p as f64 / 100.0).abs() < 1e-6,
            )));
        }
        for (k, p) in [0, 8, 16, 24].into_iter().enumerate() {
            v.push(Some((
                44 + k as u32,
                format!("Swing {p} %"),
                (self.swing - p as f64 / 100.0).abs() < 1e-6,
            )));
        }
        v.push(None);
        v.push(s(60, "Open in REAPER's MIDI editor"));
        v
    }

    pub fn transform(&mut self, id: u32) -> Vec<Act> {
        let cursor = self.playhead.unwrap_or(0.0);
        let (g, bar) = (self.grid, self.bar_qn());
        let _ = bar;
        match id {
            40..=43 => {
                self.strength = [1.0, 0.75, 0.5, 0.25][(id - 40) as usize];
                return Vec::new();
            }
            44..=47 => {
                self.swing = [0.0, 0.08, 0.16, 0.24][(id - 44) as usize];
                return Vec::new();
            }
            60 => return vec![Act::OpenReaper],
            _ => {}
        }
        self.dirty = true;
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let r = &mut md.roll;
        let (k, label): (usize, &'static str) = match id {
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
            10 => (r.legato(), "CP Editor: legato"),
            11 => (r.reverse(), "CP Editor: reverse"),
            12 => (r.invert_pitch(), "CP Editor: mirror"),
            13 => (r.glue(), "CP Editor: glue"),
            14 => (r.split_at(cursor), "CP Editor: split"),
            15 => (r.snap_to_scale(), "CP Editor: snap to scale"),
            20 => (r.vel_set(127), "CP Editor: velocity"),
            21 => (r.vel_set(100), "CP Editor: velocity"),
            22 => (r.vel_set(80), "CP Editor: velocity"),
            23 => (r.vel_set(64), "CP Editor: velocity"),
            24 => (r.vel_ramp(40, 120), "CP Editor: velocity ramp"),
            25 => (r.vel_ramp(120, 40), "CP Editor: velocity ramp"),
            26 => (r.vel_scale(0.5, 80), "CP Editor: velocity"),
            27 => (r.vel_scale(1.5, 80), "CP Editor: velocity"),
            30 => (r.humanize(1, g * 0.05, 4), "CP Editor: humanize"),
            31 => (r.humanize(2, g * 0.12, 10), "CP Editor: humanize"),
            32 => (r.humanize(3, g * 0.25, 20), "CP Editor: humanize"),
            50 => (r.scale_len(2.0), "CP Editor: length"),
            51 => (r.scale_len(0.5), "CP Editor: length"),
            52 => (r.set_len(g), "CP Editor: length"),
            _ => (0, ""),
        };
        self.commit_if(k, label)
    }

    pub fn set_grid(&mut self, g: f64) {
        self.grid = g;
        self.dirty = true;
    }

    pub fn set_scale(&mut self, s: Option<Scale>) {
        if let Some(md) = self.midi_mut() {
            md.roll.scale = s;
        }
        self.dirty = true;
    }

    pub fn scale_of(&self) -> Option<Scale> {
        self.midi().and_then(|m| m.roll.scale)
    }

    /// Aller de note en note (Alt + fleches), et l'entendre.
    fn walk(&mut self, k: u32, extend: bool) -> Vec<Act> {
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let notes = &md.roll.notes;
        if notes.is_empty() {
            return Vec::new();
        }
        let cur = notes.iter().position(|n| n.sel).unwrap_or(0);
        let c = notes[cur];
        let next = match k {
            vk::RIGHT => (cur + 1).min(notes.len() - 1),
            vk::LEFT => cur.saturating_sub(1),
            // Haut / bas : la note la plus proche au-dessus / au-dessous,
            // au meme moment d'abord.
            _ => {
                let up = k == vk::UP;
                notes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| {
                        if up {
                            n.pitch > c.pitch
                        } else {
                            n.pitch < c.pitch
                        }
                    })
                    .min_by(|a, b| {
                        (a.1.qn0 - c.qn0)
                            .abs()
                            .total_cmp(&(b.1.qn0 - c.qn0).abs())
                            .then(
                                (a.1.pitch - c.pitch)
                                    .abs()
                                    .cmp(&(b.1.pitch - c.pitch).abs()),
                            )
                    })
                    .map_or(cur, |(i, _)| i)
            }
        };
        let id = notes[next].id;
        let p = notes[next].pitch;
        let q = notes[next].qn0;
        if extend {
            md.roll.add_sel(id);
        } else {
            md.roll.select_only(id);
        }
        // La note reste a l'ecran.
        let v = self.view_qn();
        if q < self.scroll || q > self.scroll + v * 0.9 {
            self.scroll = (q - v * 0.2).max(0.0);
        }
        self.dirty = true;
        self.audition(p, false)
    }

    fn key(&mut self, k: u32, m: Mods) -> Vec<Act> {
        self.dirty = true;
        match k {
            vk::SPACE => return vec![Act::Play],
            0x5A if m.ctrl && m.shift => return vec![Act::Redo],
            0x5A if m.ctrl => return vec![Act::Undo],
            0x59 if m.ctrl => return vec![Act::Redo],
            vk::HOME => {
                self.fit();
                return Vec::new();
            }
            0x46 if !m.ctrl => {
                self.follow = !self.follow;
                return Vec::new();
            }
            _ => {}
        }
        if self.midi().is_none() {
            return Vec::new();
        }
        if m.alt && matches!(k, vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN) {
            return self.walk(k, m.shift);
        }
        let (g, bar) = (self.grid, self.bar_qn());
        let cursor = self.playhead.unwrap_or(0.0);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let r = &mut md.roll;
        let (n, label): (usize, &'static str) = match k {
            vk::ESCAPE => {
                r.clear_sel();
                self.range = None;
                return Vec::new();
            }
            vk::DELETE | vk::BACK => (r.delete_sel(), "CP Editor: delete notes"),
            0x41 if m.ctrl => {
                r.select_all();
                return Vec::new();
            }
            0x43 if m.ctrl => {
                r.copy();
                return Vec::new();
            }
            0x58 if m.ctrl => (r.cut(), "CP Editor: cut"),
            0x56 if m.ctrl => (r.paste(cursor), "CP Editor: paste"),
            0x44 if m.ctrl => {
                let d = r.dup_offset(bar, g);
                (r.duplicate(d, 0), "CP Editor: duplicate")
            }
            0x51 if !m.ctrl => return self.quantize(),
            vk::UP | vk::DOWN => {
                let dir = if k == vk::UP { 1 } else { -1 };
                let n = if m.ctrl {
                    r.transpose_scale(dir)
                } else if m.shift {
                    r.transpose(12 * dir)
                } else {
                    r.transpose(dir)
                };
                let p = r.notes.iter().find(|x| x.sel).map(|x| x.pitch);
                let mut v = self.commit_if(n, "CP Editor: transpose");
                if let Some(p) = p {
                    v.extend(self.audition(p, false));
                }
                return v;
            }
            vk::LEFT | vk::RIGHT => {
                let dir = if k == vk::RIGHT { 1.0 } else { -1.0 };
                (r.nudge(g * dir), "CP Editor: nudge")
            }
            _ => return Vec::new(),
        };
        self.commit_if(n, label)
    }

    /// Les touches que la fenetre garde (les autres vont a REAPER).
    pub fn wants_key(&self, k: u32, m: Mods) -> bool {
        let midi = self.midi().is_some();
        match k {
            vk::SPACE | vk::HOME => true,
            0x5A | 0x59 if m.ctrl => true,
            0x46 if !m.ctrl => true,
            vk::ESCAPE | vk::DELETE | vk::BACK => midi,
            0x41 | 0x43 | 0x58 | 0x56 | 0x44 if m.ctrl => midi,
            0x51 if !m.ctrl => midi,
            vk::UP | vk::DOWN | vk::LEFT | vk::RIGHT => midi,
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Les actions de l'editeur MIDI de REAPER (ses raccourcis, tels quels)
    // -----------------------------------------------------------------------

    /// Une action de la section « MIDI Editor » de REAPER (son numero), ou
    /// rien si l'editeur ne la connait pas.
    pub fn command(&mut self, cmd: u32) -> Option<Vec<Act>> {
        self.dirty = true;
        // Le transport et la vue, sans item aussi.
        let main = |c: u32| Some(vec![Act::Main(c)]);
        match cmd {
            40016 => return main(40044), // Transport : lecture/arret
            1140 => return main(1007),   // lecture
            1142 => return main(1016),   // arret
            40017 => return main(40073), // lecture/pause
            1141 => return main(1008),   // pause
            1139 => return main(1068),   // boucle
            1005 => return main(40026),  // enregistrer le projet
            40420 => return main(40605), // la liste des actions
            40013 => return Some(vec![Act::Undo]),
            40014 => return Some(vec![Act::Redo]),
            2 => return Some(vec![Act::Close]),
            40041 => {
                self.hear = !self.hear;
                return Some(if self.hear {
                    Vec::new()
                } else {
                    vec![Act::AuditionOff]
                });
            }
            40750 => {
                self.follow = !self.follow;
                return Some(Vec::new());
            }
            1014 => {
                self.snap = !self.snap;
                return Some(Vec::new());
            }
            1012 | 1011 => {
                let x = self.lay.sx + self.lay.sw * 0.5;
                let q = self.qn_of(x);
                let k = if cmd == 1012 { 1.25 } else { 0.8 };
                self.zoom_at(self.zoom * k, q, x);
                return Some(Vec::new());
            }
            40111 | 40112 => {
                let l = self.lay;
                let y = l.gy + l.gh * 0.5;
                let before = (y - l.gy + self.vscroll) / l.rh.max(1.0);
                let k = if cmd == 40111 { 1.15 } else { 1.0 / 1.15 };
                let rh = (l.rh * k).clamp(ROW_MIN * self.scale, ROW_MAX * self.scale);
                self.row_h = rh / self.scale;
                self.lay.rh = rh;
                self.lay.content_h = self.rows.len() as f32 * rh;
                self.vscroll = (before * rh - (y - l.gy)).clamp(0.0, self.max_vscroll());
                return Some(Vec::new());
            }
            40138 | 40139 => {
                let d = self.lay.rh * 4.0 * if cmd == 40138 { -1.0 } else { 1.0 };
                self.vscroll = (self.vscroll + d).clamp(0.0, self.max_vscroll());
                return Some(Vec::new());
            }
            40140 | 40141 => {
                let d = self.view_qn() * 0.2 * if cmd == 40140 { -1.0 } else { 1.0 };
                self.scroll = (self.scroll + d).max(0.0);
                return Some(Vec::new());
            }
            40466 => {
                self.fit();
                return Some(Vec::new());
            }
            _ => {}
        }
        if let Some(g) = grid_of(cmd) {
            self.set_grid(g);
            return Some(Vec::new());
        }
        let (g, bar) = (self.grid.max(1.0 / 64.0), self.bar_qn());
        let cursor = self.playhead.unwrap_or(0.0);
        let span = self.span();
        let px = 1.0 / self.lay.ppq.max(1e-3) as f64;
        let (mx, my) = (self.mouse.0, self.mouse.1);
        let mouse_q = self.qn_of(mx);
        let mouse_p = self.pitch_at(my);
        // La vue et le curseur.
        match cmd {
            40036 | 40037 => {
                let (a, b) = span?;
                let q = if cmd == 40036 { a } else { b };
                let v = self.view_qn();
                if q < self.scroll || q > self.scroll + v {
                    self.scroll = (q - v * 0.05).max(0.0);
                }
                return Some(vec![Act::Seek(q)]);
            }
            40151 | 40150 => {
                let v = self.view_qn();
                self.scroll = (cursor - v * 0.3).max(0.0);
                return Some(Vec::new());
            }
            40047 | 40048 => {
                let q = (cursor / g).round() * g + if cmd == 40048 { g } else { -g };
                return Some(vec![Act::Seek(q.max(0.0))]);
            }
            40682 | 40683 => {
                let q = (cursor / bar).floor() * bar + if cmd == 40682 { bar } else { -bar };
                return Some(vec![Act::Seek(q.max(0.0))]);
            }
            40684 => return Some(vec![Act::Seek((cursor / bar).floor() * bar)]),
            40185 | 40186 => {
                let d = if cmd == 40186 { px } else { -px };
                return Some(vec![Act::Seek((cursor + d).max(0.0))]);
            }
            40451 => return Some(vec![Act::Seek(((cursor / bar).floor() + 1.0) * bar)]),
            40725 => {
                self.fit_selection();
                return Some(Vec::new());
            }
            _ => {}
        }
        self.midi()?;
        if matches!(cmd, 40413 | 40414) {
            let k = if cmd == 40413 { vk::RIGHT } else { vk::LEFT };
            return Some(self.walk(k, false));
        }
        if matches!(cmd, 40440 | 40639 | 40872 | 40873) {
            let m = self.midi()?;
            let sel = m.roll.notes.iter().filter(|n| n.sel);
            let q = if matches!(cmd, 40440 | 40872) {
                sel.map(|n| n.qn0).fold(f64::INFINITY, f64::min)
            } else {
                sel.map(|n| n.end()).fold(f64::NEG_INFINITY, f64::max)
            };
            return Some(if q.is_finite() {
                vec![Act::Seek(q)]
            } else {
                Vec::new()
            });
        }
        let (strength, swing) = (self.strength, self.swing);
        let last = (self.last_chan, self.last_vel, self.last_len);
        let range = self.range;
        let (grid_raw, snap_on) = (self.grid, self.snap);
        let md = self.midi_mut()?;
        let r = &mut md.roll;
        let (n, label): (usize, &'static str) = match cmd {
            40667 | 40002 => (r.delete_sel(), "CP Editor: delete notes"),
            40006 | 40003 => {
                r.select_all();
                (0, "")
            }
            40214 => {
                r.clear_sel();
                (0, "")
            }
            40501 => {
                r.invert_sel();
                (0, "")
            }
            41140 | 41723 => {
                r.select_measure(cursor, bar);
                (0, "")
            }
            40746 | 40877 | 40875 | 40876 => {
                if let Some((a, b)) = range {
                    r.select_in(a, b, false);
                }
                (0, "")
            }
            40010 | 40733 | 40731 => {
                r.copy();
                (0, "")
            }
            40012 | 40734 | 40732 => (r.cut(), "CP Editor: cut"),
            40011 | 40429 | 40889 | 40890 => (r.paste(cursor), "CP Editor: paste"),
            40882 | 40883 | 40886 | 40887 | 40888 => {
                let d = r.dup_offset(bar, g);
                (r.duplicate(d, 0), "CP Editor: duplicate")
            }
            40884 => (r.duplicate(0.0, 12), "CP Editor: duplicate"),
            40885 => (r.duplicate(0.0, -12), "CP Editor: duplicate"),
            // Avec une gamme, REAPER reste dans la gamme ; « ignoring
            // scale/key » non.
            40177 => (r.transpose_scale(1), "CP Editor: transpose"),
            40178 => (r.transpose_scale(-1), "CP Editor: transpose"),
            41026 => (r.transpose(1), "CP Editor: transpose"),
            41027 => (r.transpose(-1), "CP Editor: transpose"),
            40179 => (r.transpose(12), "CP Editor: transpose"),
            40180 => (r.transpose(-12), "CP Editor: transpose"),
            40183 => (r.nudge(-g), "CP Editor: nudge"),
            40184 => (r.nudge(g), "CP Editor: nudge"),
            40181 => (r.nudge(-px), "CP Editor: nudge"),
            40182 => (r.nudge(px), "CP Editor: nudge"),
            40446 => (lengthen(r, g), "CP Editor: length"),
            40447 => (lengthen(r, -g), "CP Editor: length"),
            40444 => (lengthen(r, px), "CP Editor: length"),
            40445 => (lengthen(r, -px), "CP Editor: length"),
            40773 => (r.scale_len(2.0), "CP Editor: length"),
            40774 => (r.scale_len(0.5), "CP Editor: length"),
            40633 => (r.set_len(grid_raw.max(MIN_LEN)), "CP Editor: length"),
            40462 => (r.vel_add(1), "CP Editor: velocity"),
            40463 => (r.vel_add(10), "CP Editor: velocity"),
            40464 => (r.vel_add(-1), "CP Editor: velocity"),
            40465 => (r.vel_add(-10), "CP Editor: velocity"),
            40055 => (r.mute_sel(None), "CP Editor: mute"),
            40057 => (r.mute_sel(Some(true)), "CP Editor: mute"),
            40058 => (r.mute_sel(Some(false)), "CP Editor: unmute"),
            40046 => (r.split_at(cursor), "CP Editor: split"),
            40052 => (r.split_at(mouse_q), "CP Editor: split"),
            40641 => (r.split_grid(g), "CP Editor: split"),
            40456 => (r.glue(), "CP Editor: join notes"),
            40765 | 40766 => (r.legato(), "CP Editor: legato"),
            40905..=40908 => (r.invert_pitch(), "CP Editor: invert"),
            40019 | 40902 | 40904 => (r.reverse(), "CP Editor: reverse"),
            40469 | 40729 | 40728 | 40406 | 40727 | 41768 | 40009 => {
                (r.quantize(g, strength, swing), "CP Editor: quantize")
            }
            41769..=41773 => {
                let q = [1.0 / 16.0, 0.125, 0.25, 0.5, 1.0][(cmd - 41769) as usize];
                (r.quantize(q, strength, swing), "CP Editor: quantize")
            }
            40457 => (r.humanize(2, g * 0.12, 10), "CP Editor: humanize"),
            40790 | 40791 => {
                let mut k = 0;
                for n in r.notes.iter_mut().filter(|n| n.sel) {
                    let end = n.end();
                    if cmd == 40790 && cursor < end - MIN_LEN {
                        n.qn0 = cursor.max(0.0);
                        n.len = end - n.qn0;
                        k += 1;
                    } else if cmd == 40791 && cursor > n.qn0 + MIN_LEN {
                        n.len = cursor - n.qn0;
                        k += 1;
                    }
                }
                r.sort();
                (k, "CP Editor: trim")
            }
            40051 | 1000 | 40001 => {
                let (q, p) = if cmd == 40001 {
                    (mouse_q, mouse_p)
                } else {
                    let p = r.notes.iter().find(|n| n.sel).map_or(60, |n| n.pitch);
                    (cursor, p)
                };
                let q0 = if snap_on { (q / g).floor() * g } else { q };
                let id = r.insert(q0.max(0.0), last.2.max(MIN_LEN), p, last.0, last.1);
                r.select_only(id);
                (1, "CP Editor: insert note")
            }
            40754 => {
                let Some((a, b)) = range else {
                    return Some(Vec::new());
                };
                let (s0, s1) = r
                    .notes
                    .iter()
                    .filter(|n| n.sel)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(x, y), n| {
                        (x.min(n.qn0), y.max(n.end()))
                    });
                if s1 > s0 {
                    let k = (b - a) / (s1 - s0);
                    r.nudge(a - s0);
                    r.stretch_sel(a, k);
                    (1, "CP Editor: fit to time selection")
                } else {
                    (0, "")
                }
            }
            _ => return None,
        };
        let audit = matches!(cmd, 40177 | 40178 | 41026 | 41027 | 40179 | 40180);
        let p = r.notes.iter().find(|x| x.sel).map(|x| x.pitch);
        let mut v = self.commit_if(n, label);
        if audit {
            // La transposition s'entend, et la selection reste a l'ecran.
            self.reveal(Some(true));
            if let Some(p) = p {
                v.extend(self.audition(p, false));
            }
        }
        Some(v)
    }

    /// Ce que veut dire une action personnalisee ou un script (par son nom).
    pub fn named_cmd(&mut self, n: keys::Named) -> Vec<Act> {
        self.dirty = true;
        let (g, bar) = (self.grid.max(1.0 / 64.0), self.bar_qn());
        let (mx, my) = (self.mouse.0, self.mouse.1);
        match n {
            keys::Named::Fit => {
                self.fit();
                Vec::new()
            }
            keys::Named::PlayFromItemStart => match self.span() {
                Some((a, _)) => vec![Act::Seek(a), Act::Main(1007)],
                None => Vec::new(),
            },
            keys::Named::Duplicate => {
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                let d = md.roll.dup_offset(bar, g);
                let k = md.roll.duplicate(d, 0);
                self.commit_if(k, "CP Editor: duplicate")
            }
            keys::Named::DeleteUnderMouse => {
                let Some((id, _)) = self.note_at(mx, my) else {
                    return Vec::new();
                };
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                let k = md.roll.delete(id) as usize;
                self.commit_if(k, "CP Editor: delete note")
            }
            keys::Named::Split => {
                // La note sous la souris, a la souris ; sinon les choisies au
                // curseur.
                let q = self.qn_of(mx);
                let under = self.note_at(mx, my).map(|x| x.0);
                let cursor = self.playhead.unwrap_or(0.0);
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                let k = match under {
                    Some(id) => {
                        md.roll.select_only(id);
                        md.roll.split_at(q)
                    }
                    None => md.roll.split_at(cursor),
                };
                self.commit_if(k, "CP Editor: split")
            }
        }
    }

    /// Cadre la selection (REAPER : « zoom to selected notes »).
    fn fit_selection(&mut self) {
        let Some(m) = self.midi() else { return };
        let sel: Vec<&Note> = m.roll.notes.iter().filter(|n| n.sel).collect();
        if sel.is_empty() {
            self.fit();
            return;
        }
        let a = sel.iter().map(|n| n.qn0).fold(f64::INFINITY, f64::min);
        let b = sel
            .iter()
            .map(|n| n.end())
            .fold(f64::NEG_INFINITY, f64::max);
        let (lo, hi) = self.note_span(true).unwrap_or((60, 60));
        let w = self.lay.sw.max(100.0);
        let len = (b - a).max(0.25);
        self.zoom = ((w / self.scale.max(0.5)) / (len as f32 * 1.1)).clamp(4.0, 1600.0);
        self.lay.ppq = self.zoom * self.scale;
        self.scroll = (a - len * 0.05).max(0.0);
        self.frame_rows(lo, hi);
    }

    // -----------------------------------------------------------------------
    // Les infobulles
    // -----------------------------------------------------------------------

    pub fn label(&self, h: Hit) -> Option<String> {
        Some(match h {
            Hit::Note { id, .. } => {
                let n = self.midi()?.roll.get(id)?;
                format!(
                    "{} \u{00b7} channel {} \u{00b7} velocity {}",
                    note_name(n.pitch),
                    n.chan + 1,
                    n.vel
                )
            }
            Hit::Key(p) => {
                let out = self
                    .scale_of()
                    .is_some_and(|s| !s.contains(p));
                format!(
                    "{}{}\nClick: select the row \u{00b7} right click: hear it",
                    note_name(p),
                    if out { " \u{00b7} out of scale" } else { "" }
                )
            }
            Hit::Vel(id) => {
                let n = self.midi()?.roll.get(id)?;
                format!("Velocity {}", n.vel)
            }
            Hit::Hear => format!("Hear the notes ({})", if self.hear { "on" } else { "off" }),
            Hit::Snap => format!("Snap to grid ({})", if self.snap { "on" } else { "off" }),
            Hit::GridMenu => "Grid".into(),
            Hit::ScaleMenu => "Key and scale (out-of-scale rows are tinted)".into(),
            Hit::Color => format!(
                "Colour: {} (click: {})",
                self.colors.name(),
                self.colors.next().name()
            ),
            Hit::Rows => match self.rows_mode {
                Rows::Fit => "All the rows (click: only the rows that play)".into(),
                Rows::Notes => "Only the rows that play (click: all the rows)".into(),
            },
            Hit::Quantize => format!(
                "Quantize (Q) \u{00b7} {} % \u{00b7} swing {} %",
                (self.strength * 100.0).round(),
                (self.swing * 100.0).round()
            ),
            Hit::Transform => "Transform".into(),
            Hit::VelToggle => "Velocity lane".into(),
            Hit::Follow => "Follow playback (F)".into(),
            Hit::Help => HELP.into(),
            Hit::Ruler => "Click: edit cursor \u{00b7} drag: time selection\nShift+click: select the notes in it \u{00b7} Alt+click: clear".into(),
            Hit::Grid => "Click: new note \u{00b7} drag: its length".into(),
            Hit::EdgeL | Hit::EdgeR => "Trim (Shift: no snap)".into(),
            Hit::FadeIn => "Fade in".into(),
            Hit::FadeOut => "Fade out".into(),
            Hit::Gain => {
                let a = self.audio()?;
                format!("Gain {:+.1} dB \u{00b7} double-click: 0 dB", a.gain_db)
            }
            Hit::GainChip => "Gain \u{00b7} drag \u{00b7} double-click: 0 dB".into(),
            Hit::PitchChip => "Pitch (semitones) \u{00b7} drag \u{00b7} double-click: 0".into(),
            Hit::RateChip => "Rate \u{00b7} drag \u{00b7} double-click: 1.0".into(),
            Hit::Reverse => "Reverse".into(),
            Hit::Normalize => "Normalize".into(),
            _ => return None,
        })
    }

    /// Le marquee en cours (pour le dessin) : x0, y0, x1, y1.
    pub(crate) fn marquee(&self) -> Option<(f32, f32, f32, f32)> {
        match self.drag {
            Some(Drag::Marquee {
                q0, cy0, x1, y1, ..
            }) => Some((self.x_of(q0), self.lay.gy + cy0 - self.vscroll, x1, y1)),
            _ => None,
        }
    }

    /// Les cases peintes en cours (notes et accords).
    pub(crate) fn chord_cells(&self) -> Option<&[(f64, i32)]> {
        match &self.drag {
            Some(Drag::Chords { cells, .. }) => Some(cells),
            _ => None,
        }
    }

    /// La ligne peinte en cours : (debut, hauteur) -> (fin, hauteur).
    pub(crate) fn paint(&self) -> Option<(f64, i32, f64, i32)> {
        match self.drag {
            Some(Drag::Paint { q0, p0, q1, p1 }) => Some((q0, p0, q1, p1)),
            _ => None,
        }
    }

    pub(crate) fn hscroll_hot(&self) -> bool {
        matches!(self.drag, Some(Drag::HScroll { .. }))
    }

    pub(crate) fn audio_drag_hit(&self) -> Option<Hit> {
        match self.drag {
            Some(Drag::Audio { what, .. }) => Some(what),
            _ => None,
        }
    }
}
