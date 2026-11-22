//! La fenetre de l'editeur de CP Studio : un item MIDI (piano roll) ou un
//! clip audio (forme d'onde, rognage, fondus, gain, hauteur, vitesse).
//!
//! Sans Windows et sans REAPER : le module lui donne l'item, la vue rend des
//! [`Act`] (ecrire les notes en un pas d'annulation, deplacer le curseur,
//! faire entendre une note...). Tout se dessine dans un `Canvas` et se teste
//! en PNG.
//!
//! - Le « feeling » de REAPER : la table de modificateurs souris de Cedric
//!   (reprise de l'ancien CP_Editor) et sa molette (verticale, Alt = temps,
//!   Maj = zoom, Ctrl = hauteur des rangs ; bouton du milieu = la main).
//! - Centre sur les notes : « Fit » cadre les notes (2 rangs de marge, le
//!   reste du clavier replie en deux bandes) ; « Notes » ne garde que les
//!   rangs joues.
//! - Pas de phrase a l'ecran : les explications sont des infobulles
//!   ([`Ed::label`]).

mod draw;
#[cfg(test)]
mod tests;

pub use cp_roll;

use cp_roll::{MIN_LEN, Note, Roll, Scale, note_name};
use cp_ui::{Button, Ev, Mods, vk};

/// Les rangs montres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
    /// La vue cadre les notes ; le reste du clavier se replie.
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
    FoldTop,
    FoldBottom,
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
    pub fold_top: bool,
    pub fold_bot: bool,
    pub fold_h: f32,
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
    /// Un appui pas encore decide (clic ou glisser).
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
        base: Vec<Note>,
        /// Le debut et le rang de la note saisie.
        anchor_q: f64,
        anchor_p: i32,
        snap: bool,
        /// 0 libre, 1 temps seul, 2 hauteur seule, 3 a decider.
        axis: u8,
        copy: bool,
        heard: i32,
    },
    Edge {
        x0: f32,
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
    Marquee {
        x0: f32,
        y0: f32,
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

/// L'aide : les touches et les gestes (l'infobulle du « ? »).
pub const HELP: &str = "Notes\n\
Click: select   \u{00b7}   Ctrl: toggle   \u{00b7}   Shift: range   \u{00b7}   Alt: erase\n\
Drag: move   \u{00b7}   Shift: no snap   \u{00b7}   Ctrl: copy   \u{00b7}   Shift+Ctrl: one axis\n\
Edges: length   \u{00b7}   Ctrl: this note only   \u{00b7}   Alt: stretch the group\n\
Empty grid: click = new note, drag = its length   \u{00b7}   Alt drag: erase\n\
Ctrl+Alt drag: paint a line   \u{00b7}   right drag: lasso   \u{00b7}   right click: delete\n\
\n\
Keys\n\
\u{2191}\u{2193} semitone   \u{00b7}   Shift: octave   \u{00b7}   Ctrl: scale step   \u{00b7}   \u{2190}\u{2192} nudge\n\
Alt+arrows: next note (heard)   \u{00b7}   Q quantize   \u{00b7}   Ctrl+D duplicate\n\
Ctrl+C / X / V   \u{00b7}   Del   \u{00b7}   Ctrl+A   \u{00b7}   Home: fit   \u{00b7}   F: follow\n\
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
    /// Le passage choisi sur la regle (noires).
    pub range: Option<(f64, f64)>,
    pub status: String,
    pub lay: Lay,
    pub scale: f32,
    /// Les rangs replies qu'on a rouverts : (au-dessus, au-dessous).
    extra: (i32, i32),
    /// La hauteur des rangs choisie (Ctrl + molette) ; sinon ils remplissent.
    row_h: Option<f32>,
    /// Les rangs montres, du haut vers le bas.
    pub(crate) rows: Vec<i32>,
    pub(crate) boxes: Vec<HBox>,
    pub(crate) hover: Hit,
    drag: Option<Drag>,
    last_len: f64,
    last_vel: i32,
    last_chan: i32,
    dirty: bool,
}

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
            extra: (0, 0),
            row_h: None,
            rows: Vec::new(),
            boxes: Vec::new(),
            hover: Hit::None,
            drag: None,
            last_len: 0.25,
            last_vel: 100,
            last_chan: 0,
            dirty: true,
        }
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
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

    /// Le cadrage : tout l'item dans la largeur, les rangs remis a la vue.
    pub fn fit(&mut self) {
        self.extra = (0, 0);
        self.row_h = None;
        self.vscroll = 0.0;
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

    /// Les rangs : du plus aigu au plus grave.
    pub(crate) fn compute_rows(&self) -> Vec<i32> {
        let Some(m) = self.midi() else {
            return Vec::new();
        };
        let mut used: Vec<i32> = m.roll.notes.iter().map(|n| n.pitch).collect();
        used.sort_unstable();
        used.dedup();
        if self.rows_mode == Rows::Notes && !used.is_empty() {
            used.reverse();
            return used;
        }
        let (lo, hi) = match (used.first(), used.last()) {
            (Some(&a), Some(&b)) => (a - 2, b + 2),
            _ => (48, 72),
        };
        let (mut lo, mut hi) = (lo - self.extra.1, hi + self.extra.0);
        // Au moins une octave.
        while hi - lo < 12 {
            hi += 1;
            if hi - lo < 12 {
                lo -= 1;
            }
        }
        let (lo, hi) = (lo.max(0), hi.min(127));
        (lo..=hi).rev().collect()
    }

    pub(crate) fn row_y(&self, idx: usize) -> f32 {
        self.lay.gy + idx as f32 * self.lay.rh - self.vscroll
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
        let x0 = self.x_of(n.qn0);
        let x1 = self.x_of(n.end()).max(x0 + 2.0);
        Some((x0, self.row_y(idx), x1 - x0, self.lay.rh))
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
            self.playhead = qn;
            self.playing = playing;
            self.dirty = true;
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
                if l.fold_top && y >= l.gy - l.fold_h && y < l.gy {
                    return Hit::FoldTop;
                }
                let gb = l.gy + l.gh;
                if l.fold_bot && y >= gb && y < gb + l.fold_h {
                    return Hit::FoldBottom;
                }
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
                if self.hover != Hit::None {
                    self.hover = Hit::None;
                    self.dirty = true;
                }
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
                let rh = (l.rh * 1.15f32.powf(dy)).clamp(4.0 * self.scale, 72.0 * self.scale);
                self.row_h = Some(rh / self.scale);
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
                    self.drag = Some(Drag::Marquee {
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                        add: m.ctrl,
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
                // Sur la regle, c'est Ctrl qui ignore le magnetisme (REAPER).
                let q = self.snap_q(q, !m.ctrl).max(0.0);
                self.range = None;
                self.drag = Some(Drag::Range { q0: q });
                vec![Act::Seek(q)]
            }
            Hit::FoldTop => {
                self.extra.0 += 12;
                Vec::new()
            }
            Hit::FoldBottom => {
                self.extra.1 += 12;
                Vec::new()
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
                self.row_h = None;
                self.vscroll = 0.0;
                Vec::new()
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
        let bar = self.bar_qn();
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
                x0: x,
                base,
                id,
                left: part == Part::Left,
                snap: !m.shift,
                solo: m.ctrl,
                stretch: m.alt,
            });
            return Vec::new();
        }
        if m.alt && m.shift {
            md.roll.select_measure(n.qn0, bar);
            return Vec::new();
        }
        if m.alt && m.ctrl {
            md.roll.select_from(n.qn0);
            return Vec::new();
        }
        if m.alt {
            md.roll.delete(id);
            self.drag = Some(Drag::Erase { any: true });
            return Vec::new();
        }
        if !m.ctrl && !m.shift && !n.sel {
            md.roll.select_only(id);
        }
        self.last_vel = n.vel;
        self.last_chan = n.chan;
        self.last_len = n.len;
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
        if m.ctrl && m.alt {
            let q0 = self.snap_floor(q, !m.shift);
            self.drag = Some(Drag::Paint {
                q0,
                p0: p,
                q1: q0,
                p1: p,
            });
            return Vec::new();
        }
        if m.alt {
            if let Some(md) = self.midi_mut() {
                md.roll.clear_sel();
            }
            self.drag = Some(Drag::Erase { any: false });
            return Vec::new();
        }
        if m.ctrl {
            if let Some(md) = self.midi_mut() {
                md.roll.clear_sel();
            }
            return vec![Act::Seek(q.max(0.0))];
        }
        // Une note neuve dans la case du pointeur ; glisser regle sa longueur
        // (et sa hauteur, tant qu'elle est neuve).
        let q0 = self.snap_floor(q, !m.shift).max(0.0);
        let len = if self.snap {
            self.grid.max(self.last_len.min(self.grid * 4.0))
        } else {
            self.last_len
        };
        let (chan, vel) = (self.last_chan, self.last_vel);
        let Some(md) = self.midi_mut() else {
            return Vec::new();
        };
        let id = md.roll.insert(q0, len, p, chan, vel);
        md.roll.select_only(id);
        self.drag = Some(Drag::Insert { id, q0, heard: p });
        self.audition(p, true)
    }

    fn on_move(&mut self, x: f32, y: f32, mods: Mods) -> Vec<Act> {
        let Some(d) = self.drag.clone() else {
            let h = self.hit(x, y);
            if h != self.hover {
                self.hover = h;
                self.dirty = true;
            }
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
                        x0,
                        y0,
                        x1: x,
                        y1: y,
                        add: m.ctrl,
                    });
                    return Vec::new();
                }
                let Some(id) = id else { return Vec::new() };
                let copy = m.ctrl && !m.shift;
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
                    base,
                    anchor_q: n.qn0,
                    anchor_p: n.pitch,
                    snap: !m.shift || (m.shift && m.ctrl),
                    axis: if m.shift && m.ctrl { 3 } else { 0 },
                    copy,
                    heard: n.pitch,
                });
                self.on_move(x, y, mods)
            }
            Drag::Move {
                x0,
                y0,
                base,
                anchor_q,
                anchor_p,
                snap,
                axis,
                copy,
                heard,
            } => {
                let mut axis = axis;
                if axis == 3 {
                    let (dx, dy) = ((x - x0).abs(), (y - y0).abs());
                    if dx.max(dy) > 6.0 * self.scale {
                        axis = if dx >= dy { 1 } else { 2 };
                    }
                }
                let raw = anchor_q + ((x - x0) / self.lay.ppq.max(1e-3)) as f64;
                let mut dq = self.snap_q(raw, snap) - anchor_q;
                let mut dp = self.pitch_at(y) - anchor_p;
                if self.rows_mode == Rows::Notes {
                    // Les rangs ne sont pas contigus : on vise un rang joue.
                    dp = self.pitch_at(y) - anchor_p;
                }
                match axis {
                    1 => dp = 0,
                    2 => dq = 0.0,
                    3 => {
                        dp = 0;
                        dq = 0.0;
                    }
                    _ => {}
                }
                let now = anchor_p + dp;
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                md.roll.notes = base.clone();
                md.roll.move_sel(dq, dp);
                self.drag = Some(Drag::Move {
                    x0,
                    y0,
                    base,
                    anchor_q,
                    anchor_p,
                    snap,
                    axis,
                    copy,
                    heard: now,
                });
                if now != heard {
                    return self.audition(now, true);
                }
                Vec::new()
            }
            Drag::Edge {
                x0,
                base,
                id,
                left,
                snap,
                solo,
                stretch,
            } => {
                let dq_raw = ((x - x0) / self.lay.ppq.max(1e-3)) as f64;
                let Some(n) = base.iter().find(|n| n.id == id).copied() else {
                    return Vec::new();
                };
                let sel: Vec<&Note> = base.iter().filter(|b| b.sel).collect();
                let g0 = sel.iter().map(|b| b.qn0).fold(f64::INFINITY, f64::min);
                let g1 = sel
                    .iter()
                    .map(|b| b.end())
                    .fold(f64::NEG_INFINITY, f64::max);
                let grid_ok = snap;
                let target_end = self.snap_q(n.end() + dq_raw, grid_ok);
                let target_start = self.snap_q(n.qn0 + dq_raw, grid_ok);
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
            Drag::Marquee { x0, y0, add, .. } => {
                self.drag = Some(Drag::Marquee {
                    x0,
                    y0,
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
        let Some(d) = self.drag.take() else {
            return Vec::new();
        };
        self.dirty = true;
        let _ = b;
        match d {
            Drag::Press {
                id: Some(id),
                mods: m,
                right,
                ..
            } => {
                let Some(md) = self.midi_mut() else {
                    return Vec::new();
                };
                if right {
                    md.roll.delete(id);
                    return vec![Act::Commit("CP Editor: delete note")];
                }
                if m.ctrl {
                    md.roll.toggle(id);
                } else if m.shift {
                    md.roll.select_range_to(id);
                } else {
                    md.roll.select_only(id);
                }
                vec![Act::AuditionOff]
            }
            Drag::Press { .. } => Vec::new(),
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
            Drag::Marquee { x0, y0, add, .. } => {
                let (qa, qb) = (self.qn_of(x0), self.qn_of(x));
                let (pa, pb) = (self.pitch_at(y0), self.pitch_at(y));
                if let Some(md) = self.midi_mut() {
                    md.roll.select_box(qa, qb, pa, pb, add);
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
                let _ = y;
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
            Hit::FoldTop => "Rows above \u{2014} click to open".into(),
            Hit::FoldBottom => "Rows below \u{2014} click to open".into(),
            Hit::Snap => format!("Snap to grid ({})", if self.snap { "on" } else { "off" }),
            Hit::GridMenu => "Grid".into(),
            Hit::ScaleMenu => "Key and scale (out-of-scale rows are tinted)".into(),
            Hit::Color => format!(
                "Colour: {} (click: {})",
                self.colors.name(),
                self.colors.next().name()
            ),
            Hit::Rows => match self.rows_mode {
                Rows::Fit => "Rows: fit to the notes (click: only the rows that play)".into(),
                Rows::Notes => "Rows: only the rows that play (click: fit)".into(),
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
            Some(Drag::Marquee { x0, y0, x1, y1, .. }) => Some((x0, y0, x1, y1)),
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

    pub(crate) fn row_h_fixed(&self) -> Option<f32> {
        self.row_h
    }

    pub(crate) fn audio_drag_hit(&self) -> Option<Hit> {
        match self.drag {
            Some(Drag::Audio { what, .. }) => Some(what),
            _ => None,
        }
    }
}
