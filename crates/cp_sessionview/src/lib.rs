//! La vue de la Session : la grille des cases, les en-tetes, les scenes, la
//! ligne d'arret, la barre d'outils et les tranches de mixage.
//!
//! Ce crate ne connait ni Windows ni REAPER : il lit la [`Session`] (le
//! modele) et les tranches ([`Strip`], lues de REAPER par le module), dessine,
//! et traduit chaque geste en une [`Act`] que le module execute. Tout se
//! teste hors de REAPER (`tests.rs`, planches PNG).
//!
//! Aucune phrase a l'ecran : ce qui s'explique vient au survol
//! ([`View::label`]).

#![allow(clippy::too_many_arguments)]

mod draw;
#[cfg(test)]
mod tests;

use cp_session::{CellState, Session};
use cp_ui::pop::{Out, Pop};
use cp_ui::{Button, Ev, Mods, Theme};

/// Les boutons de la barre d'outils.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Clock,
    Quant,
    Rec,
    Overdub,
    StopAll,
    Panic,
    Capture,
    ArrangeRec,
    BackToArrange,
    Help,
}

/// Les parties d'une tranche de mixage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Input,
    MidiOut,
    Monitor,
    Fx(usize),
    FxAdd,
    Fader,
    Pan,
    Mute,
    Solo,
    Auto,
}

/// Ce qui est sous la souris.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hit {
    #[default]
    None,
    Tool(Tool),
    Header(usize),
    Arm(usize),
    /// Une case : `strip` = sa bande gauche (lancer, arreter, enregistrer).
    Cell {
        col: usize,
        scene: usize,
        strip: bool,
    },
    Scene(usize),
    StopAll,
    StopCol(usize),
    /// La colonne fantome : une piste neuve.
    Ghost,
    /// La couture entre la grille et les tranches.
    Seam,
    Strip(usize, Part),
    /// La barre de defilement des colonnes.
    HScroll,
    /// Le lisere de la vue ancree (la deplacer d'un docker a l'autre).
    DockHandle,
}

/// Une tranche de mixage, lue de REAPER par le module.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Strip {
    /// Le volume (gain lineaire, 1 = 0 dB).
    pub vol: f64,
    /// -1..1.
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    /// Le mode d'automation (0 Trim/Read, 1 Read, 2 Touch, 3 Write, 4 Latch,
    /// 5 Latch Preview).
    pub auto: i32,
    /// L'entree, en court (« In 1/2 », « MIDI All »).
    pub input: String,
    /// La sortie MIDI materielle, en court (vide : aucune).
    pub midi_out: String,
    /// L'ecoute de l'entree : 0 non, 1 oui, 2 auto.
    pub monitor: i32,
    /// Les cretes (gauche, droite), 0..1.
    pub meter: [f32; 2],
    /// Les effets : (nom, allume).
    pub fx: Vec<(String, bool)>,
}

/// Ce que le module doit faire.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Ce qu'un popover rend : (son contexte, le resultat).
    Pop(u32, Out),
    /// Lancer, annuler, ou arreter a la frontiere (le geste de la case).
    Click {
        col: usize,
        scene: usize,
    },
    Record {
        col: usize,
        scene: usize,
    },
    StopRec(usize),
    /// Ouvrir la case dans l'editeur (une case vide recoit un clip neuf).
    Open {
        col: usize,
        scene: usize,
    },
    Erase {
        col: usize,
        scene: usize,
    },
    Move {
        from: (usize, usize),
        to: (usize, usize),
        copy: bool,
    },
    /// Une case glissee hors de la fenetre (vers l'arrangement).
    DropOut {
        col: usize,
        scene: usize,
        copy: bool,
    },
    /// Une case glissee sur la colonne fantome : une piste neuve.
    DropGhost {
        col: usize,
        scene: usize,
    },
    CellMenu {
        col: usize,
        scene: usize,
        x: f32,
        y: f32,
    },
    HeaderMenu {
        col: usize,
        x: f32,
        y: f32,
    },
    SelectTrack(usize),
    Rename(usize),
    MoveTrack {
        from: usize,
        to: usize,
    },
    Arm {
        col: usize,
        add: bool,
    },
    Scene {
        scene: usize,
        keep: bool,
    },
    StopCol(usize),
    StopAll,
    Panic,
    Capture,
    ToggleClock,
    QuantMenu {
        x: f32,
        y: f32,
    },
    RecMenu {
        x: f32,
        y: f32,
    },
    Overdub(usize),
    ArrangeRec,
    BackToArrange,
    Help {
        x: f32,
        y: f32,
    },
    NewTrack,
    Select {
        col: usize,
        scene: usize,
    },
    UndoErase,
    /// Un message bref (la case n'a pas pu faire ce qu'on demandait).
    Hint(String),
    // --- les tranches
    SetVol {
        col: usize,
        vol: f64,
        done: bool,
    },
    SetPan {
        col: usize,
        pan: f64,
        done: bool,
    },
    Mute(usize),
    Solo {
        col: usize,
        exclusive: bool,
    },
    AutoMenu {
        col: usize,
        x: f32,
        y: f32,
    },
    InputMenu {
        col: usize,
        x: f32,
        y: f32,
    },
    MidiOutMenu {
        col: usize,
        x: f32,
        y: f32,
    },
    Monitor(usize),
    OpenFx {
        col: usize,
        fx: usize,
    },
    FxMenu {
        col: usize,
        fx: usize,
        x: f32,
        y: f32,
    },
    AddFx(usize),
    /// L'onglet de la vue est tire : le module la deplace.
    DockGrab,
    /// Un autre onglet du docker est clique : montrer sa vue.
    DockSwitch(usize),
    /// Le menu de la vue (ou elle vit), sous la souris.
    ViewMenu {
        x: f32,
        y: f32,
    },
}

/// Les mesures de la maquette (a 96 DPI).
pub mod geo {
    pub const TOOLBAR: f32 = 28.0;
    pub const HEADER: f32 = 22.0;
    pub const GUTTER: f32 = 26.0;
    pub const COL_W: f32 = 104.0;
    pub const GAP: f32 = 3.0;
    pub const CELL_H: f32 = 30.0;
    pub const STRIP_W: f32 = 18.0;
    pub const STOP_H: f32 = 18.0;
    pub const SCROLL_H: f32 = 10.0;
    pub const SEAM: f32 = 5.0;
    pub const STATUS: f32 = 20.0;
    /// La hauteur des tranches par defaut.
    pub const MIXER: f32 = 190.0;
    pub const MIXER_MIN: f32 = 0.0;
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Une case pressee : un glisser commence au-dela de 4 px.
    Cell {
        col: usize,
        scene: usize,
        x0: f32,
        y0: f32,
        copy: bool,
        moved: bool,
    },
    Header {
        col: usize,
        x0: f32,
        moved: bool,
    },
    Seam {
        y0: f32,
        h0: f32,
    },
    Fader {
        col: usize,
        y0: f32,
        v0: f64,
    },
    Pan {
        col: usize,
        x0: f32,
        v0: f64,
    },
    HScroll {
        x0: f32,
        s0: usize,
    },
    /// Le lisere, appuye : au-dela de quelques pixels, le glisser passe a
    /// REAPER.
    Handle {
        x0: f32,
        y0: f32,
    },
}

/// L'etat de la vue (ce que le modele ne garde pas).
pub struct View {
    /// Un popover ouvert (menu, saisie d'un nom) : il prend la souris et le
    /// clavier, il se dessine par-dessus.
    pub pop: Option<Pop>,
    /// La premiere colonne montree.
    pub scroll_col: usize,
    /// La premiere scene montree.
    pub scroll_row: usize,
    pub cell_h: f32,
    /// La hauteur des tranches.
    pub mixer_h: f32,
    pub hover: Hit,
    /// La souris, pour le fantome d'un glisser.
    pub mouse: (f32, f32),
    drag: Option<Drag>,
    /// La taille de la derniere image.
    pub size: (f32, f32),
    pub scale: f32,
    /// La touche Ctrl tenue (le curseur de copie).
    pub ctrl: bool,
    /// Ce qui a change depuis la derniere image.
    dirty: bool,
    /// Le nombre d'effets de chaque colonne (la geometrie des tranches).
    fx_counts: Vec<usize>,
    /// Les onglets du docker de la vue ancree (le module les pose).
    pub dock: cp_ui::handle::DockTabs,
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

/// La geometrie d'une image.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub s: f32,
    pub w: f32,
    pub h: f32,
    pub grid_y: f32,
    pub grid_h: f32,
    pub cell_h: f32,
    pub col_w: f32,
    pub gap: f32,
    pub gutter: f32,
    pub header_y: f32,
    pub stop_y: f32,
    pub scroll_y: f32,
    pub seam_y: f32,
    pub mixer_y: f32,
    pub mixer_h: f32,
    pub status_y: f32,
    /// Colonnes et scenes visibles.
    pub cols_vis: usize,
    pub rows_vis: usize,
}

impl View {
    pub fn new() -> View {
        View {
            pop: None,
            scroll_col: 0,
            scroll_row: 0,
            cell_h: geo::CELL_H,
            mixer_h: geo::MIXER,
            hover: Hit::None,
            mouse: (0.0, 0.0),
            drag: None,
            size: (0.0, 0.0),
            scale: 1.0,
            ctrl: false,
            dirty: true,
            fx_counts: Vec::new(),
            dock: cp_ui::handle::DockTabs::new("Session"),
        }
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Le point est-il sur l'onglet de la vue ancree ? (le curseur)
    pub fn on_tab(&self, x: f32, y: f32) -> bool {
        let (w, h) = self.size;
        self.dock.at(w, h, self.scale, x, y).is_some()
    }

    /// Les onglets de la vue ancree, au pied de la vue (sur la ligne
    /// d'etat) : la rangee.
    pub fn tab_rect(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.size;
        self.dock.bounds(w, h, self.scale)
    }

    /// L'onglet est saisi.
    pub(crate) fn grabbing_handle(&self) -> bool {
        matches!(self.drag, Some(Drag::Handle { .. }))
    }

    /// Un glisser de case en cours : (colonne, scene, copie).
    pub fn cell_drag(&self) -> Option<(usize, usize, bool)> {
        match self.drag {
            Some(Drag::Cell {
                col,
                scene,
                copy,
                moved: true,
                ..
            }) => Some((col, scene, copy || self.ctrl)),
            _ => None,
        }
    }

    pub fn layout(&self, th: &Theme, w: f32, h: f32) -> Layout {
        let s = th.scale;
        let k = |v: f32| (v * s).round();
        let header_y = k(geo::TOOLBAR);
        let grid_y = header_y + k(geo::HEADER);
        let status_y = h - k(geo::STATUS);
        let mixer_h = k(self.mixer_h).clamp(0.0, (status_y - grid_y - k(80.0)).max(0.0));
        let mixer_y = status_y - mixer_h;
        let seam_y = mixer_y - k(geo::SEAM);
        let scroll_y = seam_y - k(geo::SCROLL_H);
        let stop_y = scroll_y - k(geo::STOP_H);
        let cell_h = k(self.cell_h);
        let grid_h = (stop_y - grid_y).max(0.0);
        let col_w = k(geo::COL_W);
        let gap = k(geo::GAP);
        let gutter = k(geo::GUTTER);
        let cols_vis = (((w - gutter) / (col_w + gap)).floor() as usize).max(1);
        let rows_vis = ((grid_h / cell_h).floor() as usize).max(1);
        Layout {
            s,
            w,
            h,
            grid_y,
            grid_h,
            cell_h,
            col_w,
            gap,
            gutter,
            header_y,
            stop_y,
            scroll_y,
            seam_y,
            mixer_y,
            mixer_h,
            status_y,
            cols_vis,
            rows_vis,
        }
    }

    /// Le x d'une colonne (son rang dans la Session).
    pub fn col_x(&self, l: &Layout, col: usize) -> Option<f32> {
        if col < self.scroll_col {
            return None;
        }
        let k = col - self.scroll_col;
        (k < l.cols_vis + 1).then_some(l.gutter + k as f32 * (l.col_w + l.gap))
    }

    pub fn row_y(&self, l: &Layout, scene: usize) -> Option<f32> {
        if scene < self.scroll_row {
            return None;
        }
        let k = scene - self.scroll_row;
        let y = l.grid_y + k as f32 * l.cell_h;
        (y < l.stop_y).then_some(y)
    }

    /// Les boutons de la barre d'outils : (bouton, x, largeur).
    pub fn tools(&self, l: &Layout) -> Vec<(Tool, f32, f32)> {
        let s = l.s;
        let b = (24.0 * s).round();
        let wide = (56.0 * s).round();
        let mut x = (6.0 * s).round();
        let mut v = Vec::new();
        for (t, w) in [
            (Tool::Clock, wide),
            (Tool::Quant, wide),
            (Tool::Rec, wide),
            (Tool::Overdub, b),
            (Tool::StopAll, b),
            (Tool::Panic, b),
            (Tool::Capture, b),
            (Tool::ArrangeRec, b),
            (Tool::BackToArrange, b),
        ] {
            v.push((t, x, w));
            x += w + (4.0 * s).round();
        }
        v.push((Tool::Help, l.w - b - (6.0 * s).round(), b));
        v
    }

    /// Ce qui est sous le point (x, y).
    pub fn hit(&self, ses: &Session, th: &Theme, x: f32, y: f32) -> Hit {
        let (w, h) = self.size;
        let l = self.layout(th, w, h);
        // L'onglet passe avant ce qu'il couvre (il est dessine par-dessus).
        if self.on_tab(x, y) {
            return Hit::DockHandle;
        }
        if y < l.header_y {
            let by = (2.0 * l.s).round();
            for (t, tx, tw) in self.tools(&l) {
                if x >= tx && x < tx + tw && y >= by {
                    return Hit::Tool(t);
                }
            }
            return Hit::None;
        }
        if y >= l.status_y {
            return Hit::None;
        }
        if y >= l.seam_y && y < l.mixer_y {
            return Hit::Seam;
        }
        if y >= l.scroll_y && y < l.seam_y {
            return Hit::HScroll;
        }
        let ncols = ses.cols.len();
        // La colonne sous x (ou la fantome, juste apres la derniere).
        let col = if x >= l.gutter {
            let k = ((x - l.gutter) / (l.col_w + l.gap)).floor() as usize;
            let cx = l.gutter + k as f32 * (l.col_w + l.gap);
            if x - cx > l.col_w {
                None
            } else {
                Some(self.scroll_col + k)
            }
        } else {
            None
        };
        if y >= l.mixer_y {
            let Some(c) = col.filter(|&c| c < ncols) else {
                return Hit::None;
            };
            let cx = self.col_x(&l, c).unwrap_or(0.0);
            return self.strip_hit(&l, ses, c, x - cx, y - l.mixer_y);
        }
        if y < l.grid_y {
            return match col {
                Some(c) if c < ncols => {
                    let cx = self.col_x(&l, c).unwrap_or(0.0);
                    let r = (7.0 * l.s).round();
                    if x > cx + l.col_w - 2.0 * r - (4.0 * l.s) {
                        Hit::Arm(c)
                    } else {
                        Hit::Header(c)
                    }
                }
                Some(c) if c == ncols => Hit::Ghost,
                _ => Hit::None,
            };
        }
        if y >= l.stop_y {
            return match col {
                None => Hit::StopAll,
                Some(c) if c < ncols => Hit::StopCol(c),
                _ => Hit::None,
            };
        }
        let scene = self.scroll_row + ((y - l.grid_y) / l.cell_h).floor() as usize;
        match col {
            None => Hit::Scene(scene),
            Some(c) if c < ncols => {
                let cx = self.col_x(&l, c).unwrap_or(0.0);
                Hit::Cell {
                    col: c,
                    scene,
                    strip: x - cx < (geo::STRIP_W * l.s).round(),
                }
            }
            Some(c) if c == ncols => Hit::Ghost,
            _ => Hit::None,
        }
    }

    /// Les rangees d'une tranche : (partie, y, hauteur), relatives a la
    /// tranche.
    pub fn strip_rows(&self, l: &Layout, nfx: usize) -> Vec<(Part, f32, f32)> {
        let s = l.s;
        let rh = (16.0 * s).round();
        let mut y = (4.0 * s).round();
        let mut v = vec![
            (Part::Input, y, rh),
            (Part::MidiOut, y + rh + 1.0, rh),
            (Part::Monitor, y + 2.0 * (rh + 1.0), rh),
        ];
        y += 3.0 * (rh + 1.0) + (3.0 * s).round();
        let fader_min = (60.0 * s).round();
        let room = (l.mixer_h - y - fader_min - (24.0 * s).round()).max(0.0);
        let fx_rows = ((room / (rh + 1.0)).floor() as usize).min(nfx + 1);
        for k in 0..fx_rows {
            let p = if k < nfx { Part::Fx(k) } else { Part::FxAdd };
            v.push((p, y, rh));
            y += rh + 1.0;
        }
        y += (3.0 * s).round();
        let fh = (l.mixer_h - y - (22.0 * s).round()).max(0.0);
        v.push((Part::Fader, y, fh));
        v.push((Part::Pan, y + fh + (3.0 * s).round(), (14.0 * s).round()));
        v
    }

    fn strip_hit(&self, l: &Layout, ses: &Session, col: usize, x: f32, y: f32) -> Hit {
        let _ = ses;
        let s = l.s;
        let nfx = self.fx_counts.get(col).copied().unwrap_or(0);
        for (p, py, ph) in self.strip_rows(l, nfx) {
            if y >= py && y < py + ph {
                if p == Part::Fader {
                    // A droite du fader : M, S, et le mode d'automation.
                    let bx = l.col_w - (22.0 * s).round();
                    if x >= bx {
                        let bh = (16.0 * s).round();
                        let k = ((y - py) / (bh + 2.0)).floor() as usize;
                        return match k {
                            0 => Hit::Strip(col, Part::Mute),
                            1 => Hit::Strip(col, Part::Solo),
                            2 => Hit::Strip(col, Part::Auto),
                            _ => Hit::Strip(col, Part::Fader),
                        };
                    }
                }
                return Hit::Strip(col, p);
            }
        }
        Hit::None
    }

    /// Les noms a montrer au survol (pas de phrase a l'ecran).
    pub fn label(&self, ses: &Session, h: Hit) -> Option<String> {
        Some(match h {
            Hit::None | Hit::HScroll => return None,
            Hit::DockHandle => cp_ui::handle::HANDLE_TIP.into(),
            Hit::Tool(t) => match t {
                Tool::Clock => {
                    "Clock: Free (the Session's own) / Follow (REAPER's transport)".into()
                }
                Tool::Quant => format!("Launch quantization: {}", ses.q.label()),
                Tool::Rec => format!(
                    "Take length: {} bar(s). A take waits for this boundary and lasts this long",
                    ses.rec_bars
                ),
                Tool::Overdub => "Overdub the selected column (MIDI)".into(),
                Tool::StopAll => "Stop every clip at the boundary".into(),
                Tool::Panic => "Panic: stop everything now".into(),
                Tool::Capture => {
                    "Capture the playing clips into the first empty scene (Ctrl+Shift+I)".into()
                }
                Tool::ArrangeRec => {
                    "Arrange Rec: write what the Session plays into the arrangement".into()
                }
                Tool::BackToArrange => {
                    "Back to Arrangement: give the tracks back to their items".into()
                }
                Tool::Help => "Keys and gestures".into(),
            },
            Hit::Header(c) => {
                let col = ses.cols.get(c)?;
                if col.held {
                    "This column's track is gone: right-click to give its clips a track".into()
                } else {
                    let m = col.motion;
                    if m == cp_session::Motion::Stay {
                        col.track.name.clone()
                    } else {
                        format!("{} · {}", col.track.name, m.label())
                    }
                }
            }
            Hit::Arm(_) => "Arm (Ctrl: keep the others armed)".into(),
            Hit::Cell { col, scene, strip } => {
                let st = ses.state(col, scene);
                let clip = ses.clip(col, scene);
                if strip {
                    match st {
                        CellState::Empty => {
                            let armed = ses.cols.get(col).is_some_and(|c| c.track.armed);
                            if armed {
                                format!("Record {} bar(s)", ses.rec_bars)
                            } else {
                                "Arm this track to record here".into()
                            }
                        }
                        CellState::Recording | CellState::RecQueued => "Finish the take".into(),
                        CellState::Playing | CellState::Overdub => "Stop at the boundary".into(),
                        CellState::Queued | CellState::Waiting => "Cancel the launch".into(),
                        _ => "Launch".into(),
                    }
                } else {
                    match clip {
                        Some(c) => {
                            let name = if c.name.is_empty() { "Clip" } else { &c.name };
                            format!(
                                "{name} · {} bar(s) — click: edit, drag: move (Ctrl: copy), Alt+click: erase",
                                cp_session::clip::length_label(c.bars)
                            )
                        }
                        None => "Click: a new MIDI clip".into(),
                    }
                }
            }
            Hit::Scene(s) => format!("Launch scene {} (Ctrl: keep the other columns)", s + 1),
            Hit::StopAll => "Stop every clip".into(),
            Hit::StopCol(_) => "Stop this column".into(),
            Hit::Ghost => "A new track".into(),
            Hit::Seam => "Drag: mixer height · double-click: default".into(),
            Hit::Strip(_, p) => match p {
                Part::Input => "Input".into(),
                Part::MidiOut => "MIDI hardware output".into(),
                Part::Monitor => "Input monitoring: off, on, auto".into(),
                Part::Fx(_) => "Click: open · Shift: bypass · Alt: remove".into(),
                Part::FxAdd => "Add an effect".into(),
                Part::Fader => "Volume (double-click: 0 dB, Shift: fine)".into(),
                Part::Pan => "Pan (double-click: center)".into(),
                Part::Mute => "Mute".into(),
                Part::Solo => "Solo (Ctrl: exclusive)".into(),
                Part::Auto => "Automation mode (click: Read/Latch, right-click: all)".into(),
            },
        })
    }

    /// Le nombre d'effets de chaque colonne (pour la geometrie des
    /// tranches), pose par le module avec les tranches.
    pub fn set_strips(&mut self, strips: &[Strip]) {
        self.fx_counts = strips.iter().map(|s| s.fx.len()).collect();
    }

    /// Fait voir la case choisie.
    pub fn reveal(&mut self, th: &Theme, col: usize, scene: usize) {
        let l = self.layout(th, self.size.0, self.size.1);
        if col < self.scroll_col {
            self.scroll_col = col;
        } else if col >= self.scroll_col + l.cols_vis {
            self.scroll_col = col + 1 - l.cols_vis;
        }
        if scene < self.scroll_row {
            self.scroll_row = scene;
        } else if scene >= self.scroll_row + l.rows_vis {
            self.scroll_row = scene + 1 - l.rows_vis;
        }
        self.dirty = true;
    }

    fn clamp_scroll(&mut self, ses: &Session, l: &Layout) {
        let cols = ses.cols.len() + 1;
        self.scroll_col = self.scroll_col.min(cols.saturating_sub(l.cols_vis));
        self.scroll_row = self.scroll_row.min(ses.scenes.saturating_sub(l.rows_vis));
    }

    /// Un evenement. `strips` : les tranches (le fader part de leur valeur).
    /// Ouvre un popover (il remplace celui qui l'etait) ; le geste en cours
    /// s'arrete.
    pub fn open_pop(&mut self, p: Pop) {
        self.drag = None;
        self.pop = Some(p);
        self.dirty = true;
    }

    /// Un evenement pour le popover ouvert : ce qu'il rend devient une
    /// [`Act::Pop`] ; ferme, il se retire.
    pub fn pop_event(&mut self, ev: &Ev) -> Option<Act> {
        let p = self.pop.as_mut()?;
        if let Ev::Move { x, y, .. } = *ev {
            self.mouse = (x, y);
        }
        let before = p.stamp();
        let out = p.event(ev);
        let ctx = p.ctx;
        if p.stamp() != before {
            self.dirty = true;
        }
        let o = out?;
        if o.closes() {
            self.pop = None;
        }
        self.dirty = true;
        Some(Act::Pop(ctx, o))
    }

    /// Un caractere tape (une saisie de popover).
    pub fn char(&mut self, c: char) -> Option<Act> {
        self.pop.as_ref()?;
        self.pop_event(&Ev::Char(c))
    }

    /// Colle du texte dans la saisie d'un popover.
    pub fn paste(&mut self, s: &str) {
        if let Some(p) = self.pop.as_mut() {
            if p.wants_text() {
                p.paste(s);
                self.dirty = true;
            }
        }
    }

    pub fn event(&mut self, ses: &Session, strips: &[Strip], th: &Theme, ev: &Ev) -> Option<Act> {
        if self.pop.is_some() {
            return self.pop_event(ev);
        }
        let (w, h) = self.size;
        let l = self.layout(th, w, h);
        match ev {
            Ev::Move { x, y, mods } => {
                self.mouse = (*x, *y);
                self.ctrl = mods.ctrl;
                let hv = self.hit(ses, th, *x, *y);
                if hv != self.hover {
                    self.hover = hv;
                    self.dirty = true;
                }
                if self.dock.track(w, h, self.scale, *x, *y) {
                    self.dirty = true;
                }
                self.drag_move(ses, strips, &l, *x, *y, *mods)
            }
            Ev::Down { x, y, b, dbl, mods } => {
                self.down(ses, strips, th, &l, *x, *y, *b, *dbl, *mods)
            }
            Ev::Up { x, y, b, mods } => self.up(ses, th, &l, *x, *y, *b, *mods),
            Ev::Wheel { dy, dx, mods, .. } => {
                let n = if *dy > 0.0 { -1i32 } else { 1 };
                if mods.ctrl {
                    self.cell_h = (self.cell_h + 2.0 * -n as f32).clamp(18.0, 72.0);
                } else if mods.shift || mods.alt {
                    self.scroll_row = (self.scroll_row as i32 + n).max(0) as usize;
                } else if *dx != 0.0 || *dy != 0.0 {
                    let n = if *dx != 0.0 {
                        if *dx > 0.0 { 1 } else { -1 }
                    } else {
                        n
                    };
                    self.scroll_col = (self.scroll_col as i32 + n).max(0) as usize;
                }
                self.clamp_scroll(ses, &l);
                self.dirty = true;
                None
            }
            Ev::Key { vk, mods, .. } => self.key(ses, th, *vk, *mods),
            Ev::Leave => {
                if self.hover != Hit::None || self.dock.hot.is_some() {
                    self.hover = Hit::None;
                    self.dock.hot = None;
                    self.dirty = true;
                }
                None
            }
            Ev::Char(_) => None,
        }
    }

    fn down(
        &mut self,
        ses: &Session,
        strips: &[Strip],
        th: &Theme,
        l: &Layout,
        x: f32,
        y: f32,
        b: Button,
        dbl: bool,
        mods: Mods,
    ) -> Option<Act> {
        let hit = self.hit(ses, th, x, y);
        self.dirty = true;
        if b == Button::Right {
            return match hit {
                Hit::Cell { col, scene, .. } => Some(Act::CellMenu { col, scene, x, y }),
                Hit::Header(col) => Some(Act::HeaderMenu { col, x, y }),
                Hit::Strip(col, Part::Auto) => Some(Act::AutoMenu { col, x, y }),
                Hit::Strip(col, Part::Fx(fx)) => Some(Act::FxMenu { col, fx, x, y }),
                Hit::Strip(col, Part::Pan) => Some(Act::SetPan {
                    col,
                    pan: 0.0,
                    done: true,
                }),
                Hit::Tool(Tool::Quant) => Some(Act::QuantMenu { x, y }),
                Hit::Tool(Tool::Rec) => Some(Act::RecMenu { x, y }),
                Hit::DockHandle => Some(Act::ViewMenu { x, y }),
                _ => None,
            };
        }
        if b != Button::Left {
            return None;
        }
        match hit {
            Hit::DockHandle => {
                let (w, h) = self.size;
                if let Some(cp_ui::handle::Press::Switch(i)) =
                    self.dock.press(w, h, self.scale, x, y)
                {
                    return Some(Act::DockSwitch(i));
                }
                self.drag = Some(Drag::Handle { x0: x, y0: y });
                None
            }
            Hit::Tool(t) => Some(match t {
                Tool::Clock => Act::ToggleClock,
                Tool::Quant => Act::QuantMenu { x, y: l.header_y },
                Tool::Rec => Act::RecMenu { x, y: l.header_y },
                Tool::Overdub => Act::Overdub(ses.sel.0),
                Tool::StopAll => Act::StopAll,
                Tool::Panic => Act::Panic,
                Tool::Capture => Act::Capture,
                Tool::ArrangeRec => Act::ArrangeRec,
                Tool::BackToArrange => Act::BackToArrange,
                Tool::Help => Act::Help { x, y: l.header_y },
            }),
            Hit::Cell { col, scene, strip } => {
                if strip {
                    // Le second appui d'un double-clic n'annule pas le
                    // premier.
                    if dbl {
                        return None;
                    }
                    let st = ses.state(col, scene);
                    return Some(match st {
                        CellState::Recording | CellState::RecQueued => Act::StopRec(col),
                        CellState::Empty => {
                            if ses.cols.get(col).is_some_and(|c| c.track.armed) {
                                Act::Record { col, scene }
                            } else {
                                Act::Hint("Arm this track to record".into())
                            }
                        }
                        _ => Act::Click { col, scene },
                    });
                }
                if mods.alt {
                    return Some(Act::Erase { col, scene });
                }
                if ses.clip(col, scene).is_none() {
                    return Some(Act::Open { col, scene });
                }
                self.drag = Some(Drag::Cell {
                    col,
                    scene,
                    x0: x,
                    y0: y,
                    copy: mods.ctrl,
                    moved: false,
                });
                Some(Act::Select { col, scene })
            }
            Hit::Header(col) => {
                if dbl {
                    self.drag = None;
                    return Some(Act::Rename(col));
                }
                if ses.cols.get(col).is_some_and(|c| c.held) {
                    return Some(Act::HeaderMenu { col, x, y });
                }
                self.drag = Some(Drag::Header {
                    col,
                    x0: x,
                    moved: false,
                });
                Some(Act::SelectTrack(col))
            }
            Hit::Arm(col) => Some(Act::Arm {
                col,
                add: mods.ctrl,
            }),
            Hit::Scene(scene) => Some(Act::Scene {
                scene,
                keep: mods.ctrl,
            }),
            Hit::StopAll => Some(Act::StopAll),
            Hit::StopCol(c) => Some(Act::StopCol(c)),
            Hit::Ghost => Some(Act::NewTrack),
            Hit::Seam => {
                if dbl {
                    self.mixer_h = geo::MIXER;
                    return None;
                }
                self.drag = Some(Drag::Seam {
                    y0: y,
                    h0: self.mixer_h,
                });
                None
            }
            Hit::HScroll => {
                self.drag = Some(Drag::HScroll {
                    x0: x,
                    s0: self.scroll_col,
                });
                None
            }
            Hit::Strip(col, p) => {
                let st = strips.get(col).cloned().unwrap_or_default();
                match p {
                    Part::Input => Some(Act::InputMenu { col, x, y }),
                    Part::MidiOut => Some(Act::MidiOutMenu { col, x, y }),
                    Part::Monitor => Some(Act::Monitor(col)),
                    Part::Fx(fx) => Some(if mods.alt || mods.shift {
                        Act::FxMenu { col, fx, x, y }
                    } else {
                        Act::OpenFx { col, fx }
                    }),
                    Part::FxAdd => Some(Act::AddFx(col)),
                    Part::Fader => {
                        if dbl {
                            return Some(Act::SetVol {
                                col,
                                vol: 1.0,
                                done: true,
                            });
                        }
                        self.drag = Some(Drag::Fader {
                            col,
                            y0: y,
                            v0: st.vol,
                        });
                        None
                    }
                    Part::Pan => {
                        if dbl {
                            return Some(Act::SetPan {
                                col,
                                pan: 0.0,
                                done: true,
                            });
                        }
                        self.drag = Some(Drag::Pan {
                            col,
                            x0: x,
                            v0: st.pan,
                        });
                        None
                    }
                    Part::Mute => Some(Act::Mute(col)),
                    Part::Solo => Some(Act::Solo {
                        col,
                        exclusive: mods.ctrl,
                    }),
                    Part::Auto => Some(Act::AutoMenu { col, x, y }),
                }
            }
            Hit::None => None,
        }
    }

    fn drag_move(
        &mut self,
        ses: &Session,
        _strips: &[Strip],
        l: &Layout,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> Option<Act> {
        let thr = 4.0 * l.s;
        match self.drag.as_mut()? {
            Drag::Cell { x0, y0, moved, .. } => {
                if !*moved && ((x - *x0).abs() > thr || (y - *y0).abs() > thr) {
                    *moved = true;
                }
                self.dirty = true;
                None
            }
            Drag::Header { x0, moved, .. } => {
                if (x - *x0).abs() > thr {
                    *moved = true;
                }
                self.dirty = true;
                None
            }
            Drag::Seam { y0, h0 } => {
                self.mixer_h = (*h0 + (*y0 - y) / l.s).max(geo::MIXER_MIN);
                self.dirty = true;
                None
            }
            Drag::Fader { col, y0, v0 } => {
                let fine = if mods.shift { 0.1 } else { 1.0 };
                let db0 = db_of(*v0);
                let db = (db0 + (*y0 - y) as f64 * 0.25 * fine).clamp(-150.0, 12.0);
                let vol = vol_of(db);
                let col = *col;
                *y0 = y;
                *v0 = vol;
                Some(Act::SetVol {
                    col,
                    vol,
                    done: false,
                })
            }
            Drag::Pan { col, x0, v0 } => {
                let fine = if mods.shift { 0.1 } else { 1.0 };
                let pan = (*v0 + (x - *x0) as f64 / (80.0 * l.s) as f64 * fine).clamp(-1.0, 1.0);
                let col = *col;
                *x0 = x;
                *v0 = pan;
                Some(Act::SetPan {
                    col,
                    pan,
                    done: false,
                })
            }
            Drag::Handle { x0, y0 } => {
                let far = cp_ui::handle::HANDLE_DRAG_PX * l.s;
                if (x - *x0).abs() > far || (y - *y0).abs() > far {
                    self.drag = None;
                    self.dirty = true;
                    return Some(Act::DockGrab);
                }
                None
            }
            Drag::HScroll { x0, s0 } => {
                let per = l.col_w + l.gap;
                let n = ((x - *x0) / per).round() as i32;
                self.scroll_col = (*s0 as i32 + n).max(0) as usize;
                self.clamp_scroll(ses, l);
                self.dirty = true;
                None
            }
        }
    }

    fn up(
        &mut self,
        ses: &Session,
        th: &Theme,
        l: &Layout,
        x: f32,
        y: f32,
        b: Button,
        mods: Mods,
    ) -> Option<Act> {
        if b != Button::Left {
            return None;
        }
        let d = self.drag.take()?;
        self.dirty = true;
        match d {
            Drag::Cell {
                col,
                scene,
                copy,
                moved,
                ..
            } => {
                let copy = copy || mods.ctrl;
                if !moved {
                    return Some(Act::Open { col, scene });
                }
                if x < 0.0 || y < 0.0 || x >= l.w || y >= l.h {
                    return Some(Act::DropOut { col, scene, copy });
                }
                match self.hit(ses, th, x, y) {
                    Hit::Cell {
                        col: c2, scene: s2, ..
                    } => Some(Act::Move {
                        from: (col, scene),
                        to: (c2, s2),
                        copy,
                    }),
                    Hit::Ghost => Some(Act::DropGhost { col, scene }),
                    _ => None,
                }
            }
            Drag::Header { col, moved, .. } => {
                if !moved {
                    return None;
                }
                let to = match self.hit(ses, th, x, l.header_y + 1.0) {
                    Hit::Header(c) | Hit::Arm(c) => c,
                    Hit::Ghost => ses.cols.len().saturating_sub(1),
                    _ => return None,
                };
                (to != col).then_some(Act::MoveTrack { from: col, to })
            }
            Drag::Fader { col, v0, .. } => Some(Act::SetVol {
                col,
                vol: v0,
                done: true,
            }),
            Drag::Pan { col, v0, .. } => Some(Act::SetPan {
                col,
                pan: v0,
                done: true,
            }),
            Drag::Seam { .. } | Drag::HScroll { .. } | Drag::Handle { .. } => None,
        }
    }

    /// Le clavier de l'ancienne Session. `None` : pas a elle (REAPER garde
    /// ses raccourcis, Espace compris).
    pub fn key(&mut self, ses: &Session, th: &Theme, vk: u32, m: Mods) -> Option<Act> {
        if self.pop.is_some() {
            return self.pop_event(&Ev::Key {
                vk,
                mods: m,
                text: None,
            });
        }
        use cp_ui::vk::*;
        let (c, s) = ses.sel;
        let ncols = ses.cols.len().max(1);
        let nsc = ses.scenes.max(1);
        let sel = |col: usize, scene: usize| Some(Act::Select { col, scene });
        let act = match vk {
            LEFT if !m.ctrl => sel((c + ncols - 1) % ncols, s),
            RIGHT if !m.ctrl => sel((c + 1) % ncols, s),
            UP if !m.ctrl => sel(c, (s + nsc - 1) % nsc),
            DOWN if !m.ctrl => sel(c, (s + 1) % nsc),
            PRIOR => sel(c, 0),
            NEXT => {
                let last = (0..ses.scenes)
                    .rev()
                    .find(|&k| ses.clip(c, k).is_some())
                    .unwrap_or(0);
                sel(c, last)
            }
            RETURN if m.shift || m.ctrl => Some(Act::Scene {
                scene: s,
                keep: m.ctrl,
            }),
            RETURN => {
                if ses.clip(c, s).is_some() {
                    Some(Act::Click { col: c, scene: s })
                } else {
                    Some(Act::StopCol(c))
                }
            }
            DELETE => Some(Act::Erase { col: c, scene: s }),
            0x5A if m.ctrl && !m.shift => Some(Act::UndoErase),
            0x49 if m.ctrl && m.shift => Some(Act::Capture),
            _ => None,
        };
        if let Some(Act::Select { col, scene }) = act {
            self.reveal(th, col, scene);
        }
        act
    }
}

/// Le volume d'un gain lineaire, en dB.
pub fn db_of(vol: f64) -> f64 {
    if vol <= 1e-8 {
        -150.0
    } else {
        20.0 * vol.log10()
    }
}

pub fn vol_of(db: f64) -> f64 {
    if db <= -149.0 {
        0.0
    } else {
        10f64.powf(db / 20.0)
    }
}

pub use draw::Ink;
