//! CP Transport : le transport de CP Studio, pose A LA PLACE de celui de
//! REAPER (la place native « transport », tenue par le chargeur).
//!
//! Une seule barre, de gauche a droite :
//! - le transport : debut, stop, lecture, pause, enregistrement, boucle ;
//! - le compteur (mesures.temps et temps), le tempo (glisser, molette,
//!   double-clic pour le taper), la signature, Tap, le metronome, la vitesse ;
//! - les ICONES : les actions de Cedric et celles de CP Studio, dans l'ordre
//!   qu'il choisit (clic droit : ajouter, retirer, deplacer) ;
//! - le SELECTEUR DE VUES : Arrange et chaque application de CP Studio. Une
//!   vue est une destination, pas un interrupteur : cliquer deux fois ne fait
//!   rien de plus que la premiere fois. Clic droit : sa place (dans
//!   l'arrangement, ancree, flottante).
//!
//! Ce crate ne connait ni Windows ni REAPER : il recoit l'etat de lecture
//! ([`Status`]), dessine, et traduit chaque geste en une [`Act`] que le module
//! execute. Tout se teste hors de REAPER (`tests.rs`, planches PNG).
//!
//! La fenetre ne prend jamais le clavier (le chargeur la declare « sans
//! clavier ») : apres un clic sur Loop, Espace va toujours a REAPER.

#![allow(clippy::too_many_arguments)]

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Surface, hex};
use cp_theme::Frames;
use cp_ui::{Button, Ev, Icon, IconCache, Mods, Theme};

mod skin;
#[cfg(test)]
mod tests;
pub mod tuner;

pub use skin::{Skin, face_color};
pub use tuner::{TunerInk, TunerMode, TunerView, draw_tuner, slot_width};

/// Les bits de lecture (ceux de `GetPlayState`).
pub const PLAYING: i32 = 1;
pub const PAUSED: i32 = 2;
pub const RECORDING: i32 = 4;

/// Le mode d'enregistrement de REAPER (le menu du bouton Rec).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecMode {
    #[default]
    Normal,
    /// Auto-punch sur la selection de temps.
    TimeSel,
    /// Auto-punch sur les items choisis.
    Items,
}

/// Ce qu'on lit de REAPER a chaque battement (ou chaque image en lecture).
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub play: i32,
    /// « 12.3.45 »
    pub beats: String,
    /// « 0:23.456 »
    pub time: String,
    pub bpm: f64,
    pub sig: (i32, i32),
    pub rate: f64,
    pub repeat: bool,
    pub metronome: bool,
    pub rec_mode: RecMode,
}

impl Default for Status {
    fn default() -> Status {
        Status {
            play: 0,
            beats: "1.1.00".into(),
            time: "0:00.000".into(),
            bpm: 120.0,
            sig: (4, 4),
            rate: 1.0,
            repeat: false,
            metronome: false,
            rec_mode: RecMode::Normal,
        }
    }
}

/// Ce qu'une icone montre.
#[derive(Clone, Debug)]
pub enum Glyph {
    /// Une icone de barre d'outils de REAPER et ses etats (normale, survol,
    /// allumee), a sa taille d'origine : dessinee telle quelle.
    Frames(Frames),
    /// Une image (une icone de barre d'outils de REAPER), deja a la taille.
    Image(Surface),
    Icon(Icon),
    /// Des initiales, faute d'icone.
    Text(String),
}

/// Une icone de la barre : une action.
#[derive(Clone, Debug)]
pub struct Item {
    /// L'identifiant de l'action, tel qu'on le range (« _RS... », « 40364 »,
    /// « _CPSTUDIO_BROWSER »).
    pub id: String,
    /// Son nom (menus).
    pub name: String,
    pub glyph: Glyph,
    /// Une action a bascule allumee.
    pub on: bool,
    /// Sa fenetre est ouverte (un point sous l'icone).
    pub open: bool,
}

/// Une vue du selecteur.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewTab {
    pub name: String,
    /// A l'ecran a la place de l'arrangement (ou l'arrangement lui-meme).
    pub active: bool,
    /// Ouverte ailleurs (ancree ou flottante) : un point.
    pub open: bool,
}

/// Ce que vise la souris.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    None,
    Start,
    Stop,
    Play,
    Pause,
    Record,
    Repeat,
    Counter,
    Bpm,
    Sig,
    Tap,
    Metronome,
    Rate,
    Item(usize),
    Add,
    View(usize),
    /// L'accordeur, dans son emplacement.
    Tuner,
}

/// Ce que le module doit faire.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Start,
    Stop,
    Play,
    Pause,
    Record,
    Repeat,
    Metronome,
    Tap,
    /// Le tempo pendant un geste (`done` : le geste est fini, l'annulation
    /// peut le retenir).
    SetBpm {
        bpm: f64,
        done: bool,
    },
    /// Taper le tempo (la petite boite de REAPER).
    EditBpm,
    EditSig,
    SetRate {
        rate: f64,
        done: bool,
    },
    Item(usize),
    ItemMenu {
        i: usize,
        x: f32,
        y: f32,
    },
    /// Ajouter une action (la liste des actions de REAPER).
    AddItem,
    /// Clic droit sur Rec : le mode d'enregistrement.
    RecordMenu {
        x: f32,
        y: f32,
    },
    /// Clic droit ailleurs sur la barre.
    BarMenu {
        x: f32,
        y: f32,
    },
    View(usize),
    ViewMenu {
        i: usize,
        x: f32,
        y: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    what: Hit,
    y0: f32,
    v0: f64,
    moved: bool,
}

#[derive(Clone, Copy, Debug)]
struct Box_ {
    hit: Hit,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Les couleurs propres au transport.
const GREEN: u32 = 0x5BD07A;
const RED: u32 = 0xE5534B;
const AMBER: u32 = 0xF0B44C;

pub struct Transport {
    pub st: Status,
    pub items: Vec<Item>,
    pub views: Vec<ViewTab>,
    /// Ce que l'accordeur entend (aucune note : `None`).
    pub tuner: Option<TunerView>,
    pub tuner_mode: TunerMode,
    boxes: Vec<Box_>,
    hover: Hit,
    pressed: Hit,
    drag: Option<Drag>,
    dirty: bool,
    /// Les images du theme deja etirees (voir `skin.rs`).
    stretch: Vec<((u32, u8, i32, i32), Surface)>,
}

impl Default for Transport {
    fn default() -> Self {
        Transport::new()
    }
}

/// La marque du mode d'enregistrement sous le bouton Rec, faute d'image du
/// theme : deux crochets (la selection de temps) ou un cadre (les items).
pub(crate) fn rec_mode_mark(
    cv: &mut Canvas<'_>,
    th: &Theme,
    m: RecMode,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    c: Color,
) {
    let s = |v: f32| th.s(v);
    let (mw, mh) = (s(12.0).round(), s(3.0).round());
    let mx = (x + (w - mw) * 0.5).round();
    let my = (y + h - s(4.0) - mh).round();
    match m {
        RecMode::Normal => {}
        RecMode::TimeSel => {
            cv.fill_rect(mx, my, 1.0, mh, c);
            cv.fill_rect(mx + mw - 1.0, my, 1.0, mh, c);
            cv.fill_rect(mx, my + mh - 1.0, mw, 1.0, c);
        }
        RecMode::Items => {
            cv.stroke_rect(mx + 0.5, my + 0.5, mw - 1.0, mh - 1.0, 1.0, c);
        }
    }
}

/// « 120 », « 120.5 », « 97.25 ».
pub fn fmt_bpm(b: f64) -> String {
    let c = (b * 100.0).round() / 100.0;
    let s = format!("{c:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Les initiales d'un nom d'action, pour une icone sans image :
/// « Custom: BirdBird_Global Sampler.lua » → « GS ».
pub fn initials(name: &str) -> String {
    let mut n = name;
    if let Some(i) = n.find(": ") {
        n = &n[i + 2..];
    }
    let n = n
        .trim_end_matches(".lua")
        .trim_end_matches(".eel")
        .trim_end_matches(".py");
    // Le prefixe d'auteur (« BirdBird_ ») ne dit rien de l'action.
    let n = match n.rfind('_') {
        Some(i) if i + 1 < n.len() => &n[i + 1..],
        _ => n,
    };
    let words: Vec<&str> = n
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = match words.len() {
        0 => "?".into(),
        1 => words[0].chars().take(2).collect(),
        _ => words
            .iter()
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect(),
    };
    let mut s = s.to_uppercase();
    if s.chars().count() > 2 {
        s = s.chars().take(2).collect();
    }
    s
}

/// Une icone de barre d'outils de REAPER a `size` px : la premiere des trois
/// vignettes si l'image en porte trois cote a cote (normale, survol, appui),
/// reduite par moyenne de surface (pixels premultiplies : la moyenne est
/// juste).
pub fn fit_icon(src: &Surface, size: i32) -> Surface {
    let size = size.max(1);
    let sw = if src.w == src.h * 3 { src.h } else { src.w };
    let sh = src.h;
    if sw <= 0 || sh <= 0 {
        return Surface::new(size, size);
    }
    let k = (size as f32 / sw.max(sh) as f32).min(1.0);
    let (dw, dh) = (
        ((sw as f32 * k).round() as i32).max(1),
        ((sh as f32 * k).round() as i32).max(1),
    );
    let mut out = Surface::new(size, size);
    let (ox, oy) = ((size - dw) / 2, (size - dh) / 2);
    for y in 0..dh {
        let y0 = y as f32 / dh as f32 * sh as f32;
        let y1 = (y + 1) as f32 / dh as f32 * sh as f32;
        for x in 0..dw {
            let x0 = x as f32 / dw as f32 * sw as f32;
            let x1 = (x + 1) as f32 / dw as f32 * sw as f32;
            let mut acc = [0f32; 4];
            let mut wsum = 0f32;
            let mut yy = y0.floor() as i32;
            while (yy as f32) < y1 && yy < sh {
                let wy = ((yy + 1) as f32).min(y1) - (yy as f32).max(y0);
                let mut xx = x0.floor() as i32;
                while (xx as f32) < x1 && xx < sw {
                    let wx = ((xx + 1) as f32).min(x1) - (xx as f32).max(x0);
                    let wgt = wx * wy;
                    let p = src.get(xx, yy);
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += ((p >> (c * 8)) & 0xFF) as f32 * wgt;
                    }
                    wsum += wgt;
                    xx += 1;
                }
                yy += 1;
            }
            let mut px = 0u32;
            if wsum > 0.0 {
                for (c, a) in acc.iter().enumerate() {
                    px |= (((a / wsum).round() as u32).min(255)) << (c * 8);
                }
            }
            out.px[((oy + y) * size + ox + x) as usize] = px;
        }
    }
    out
}

impl Transport {
    pub fn new() -> Transport {
        Transport {
            st: Status::default(),
            items: Vec::new(),
            views: Vec::new(),
            tuner: None,
            tuner_mode: TunerMode::Clock,
            boxes: Vec::new(),
            hover: Hit::None,
            pressed: Hit::None,
            drag: None,
            dirty: true,
            stretch: Vec::new(),
        }
    }

    /// Un nouvel etat de lecture ; vrai s'il change ce qui est a l'ecran.
    pub fn set_status(&mut self, st: Status) -> bool {
        if st != self.st {
            self.st = st;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    /// A redessiner ? (et l'oublie).
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Un geste en cours (glisser le tempo) : la cadence de l'ecran.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// La cible sous la souris.
    pub fn hovered(&self) -> Hit {
        self.hover
    }

    /// Ce que dit l'infobulle d'une cible.
    pub fn label(&self, h: Hit) -> Option<String> {
        Some(match h {
            Hit::None | Hit::Counter => return None,
            Hit::Start => "Go to start".into(),
            Hit::Stop => "Stop".into(),
            Hit::Play => "Play".into(),
            Hit::Pause => "Pause".into(),
            Hit::Record => match self.st.rec_mode {
                RecMode::Normal => "Record  (right-click: record mode)".into(),
                RecMode::TimeSel => {
                    "Record: time selection auto-punch  (right-click: record mode)".into()
                }
                RecMode::Items => {
                    "Record: auto-punch selected items  (right-click: record mode)".into()
                }
            },
            Hit::Repeat => "Repeat (loop playback)".into(),
            Hit::Bpm => "Tempo: drag or wheel (Shift: fine), double-click to type".into(),
            Hit::Sig => "Time signature (project settings)".into(),
            Hit::Tap => "Tap tempo".into(),
            Hit::Metronome => "Metronome".into(),
            Hit::Rate => "Play rate: drag or wheel, double-click for 1.00".into(),
            Hit::Add => "Add an action to CP Transport".into(),
            Hit::Tuner => {
                "Tuner: listens to the armed track's input (right-click: where it lives)".into()
            }
            Hit::Item(i) => {
                let it = self.items.get(i)?;
                format!("{}  (right-click: move, remove)", it.name)
            }
            Hit::View(i) => {
                let v = self.views.get(i)?;
                if i == 0 {
                    format!("{}: REAPER's tracks", v.name)
                } else {
                    format!("{}  (right-click: where it lives)", v.name)
                }
            }
        })
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        self.boxes
            .iter()
            .rev()
            .find(|b| x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h)
            .map_or(Hit::None, |b| b.hit)
    }

    /// Le rectangle d'une cible (apres un dessin), pour un menu ou un test.
    pub fn rect_of(&self, h: Hit) -> Option<(f32, f32, f32, f32)> {
        self.boxes
            .iter()
            .find(|b| b.hit == h)
            .map(|b| (b.x, b.y, b.w, b.h))
    }

    /// Un geste. Rend l'action a executer, s'il y en a une.
    pub fn event(&mut self, ev: &Ev) -> Option<Act> {
        match *ev {
            Ev::Move { x, y, mods } => {
                if let Some(d) = self.drag.as_mut() {
                    let dy = d.y0 - y;
                    if dy.abs() >= 1.0 || d.moved {
                        d.moved = true;
                        let d = *d;
                        self.dirty = true;
                        return Some(self.drag_value(d, dy, mods, false));
                    }
                    return None;
                }
                let h = self.hit(x, y);
                if h != self.hover {
                    self.hover = h;
                    self.dirty = true;
                }
                None
            }
            Ev::Leave => {
                if self.hover != Hit::None {
                    self.hover = Hit::None;
                    self.dirty = true;
                }
                None
            }
            Ev::Down { x, y, b, dbl, .. } => {
                let h = self.hit(x, y);
                self.dirty = true;
                match b {
                    Button::Right => match h {
                        Hit::Item(i) => Some(Act::ItemMenu { i, x, y }),
                        Hit::View(i) => Some(Act::ViewMenu { i, x, y }),
                        Hit::Record => Some(Act::RecordMenu { x, y }),
                        _ => Some(Act::BarMenu { x, y }),
                    },
                    Button::Middle => None,
                    Button::Left => {
                        self.pressed = h;
                        match h {
                            // Le transport part a l'APPUI, comme une touche.
                            Hit::Start => Some(Act::Start),
                            Hit::Stop => Some(Act::Stop),
                            Hit::Play => Some(Act::Play),
                            Hit::Pause => Some(Act::Pause),
                            Hit::Record => Some(Act::Record),
                            Hit::Repeat => Some(Act::Repeat),
                            Hit::Tap => Some(Act::Tap),
                            Hit::Metronome => Some(Act::Metronome),
                            Hit::Bpm if dbl => {
                                self.drag = None;
                                Some(Act::EditBpm)
                            }
                            Hit::Rate if dbl => {
                                self.drag = None;
                                Some(Act::SetRate {
                                    rate: 1.0,
                                    done: true,
                                })
                            }
                            Hit::Bpm => {
                                self.drag = Some(Drag {
                                    what: Hit::Bpm,
                                    y0: y,
                                    v0: self.st.bpm,
                                    moved: false,
                                });
                                None
                            }
                            Hit::Rate => {
                                self.drag = Some(Drag {
                                    what: Hit::Rate,
                                    y0: y,
                                    v0: self.st.rate,
                                    moved: false,
                                });
                                None
                            }
                            _ => None,
                        }
                    }
                }
            }
            Ev::Up { x, y, b, mods } => {
                let pressed = std::mem::replace(&mut self.pressed, Hit::None);
                self.dirty = true;
                if b != Button::Left {
                    return None;
                }
                if let Some(d) = self.drag.take() {
                    return d.moved.then(|| self.drag_value(d, d.y0 - y, mods, true));
                }
                let h = self.hit(x, y);
                if h != pressed {
                    return None;
                }
                // Les icones et les vues partent au RELACHEMENT, sur la meme
                // cible : un geste commence ailleurs ne lance rien.
                match h {
                    Hit::Item(i) => Some(Act::Item(i)),
                    Hit::Add => Some(Act::AddItem),
                    Hit::View(i) => Some(Act::View(i)),
                    Hit::Sig => Some(Act::EditSig),
                    _ => None,
                }
            }
            Ev::Wheel { x, y, dy, mods, .. } => {
                let notches = dy.round();
                if notches == 0.0 {
                    return None;
                }
                match self.hit(x, y) {
                    Hit::Bpm => {
                        let step = if mods.shift { 0.1 } else { 1.0 };
                        Some(Act::SetBpm {
                            bpm: (self.st.bpm + notches as f64 * step).clamp(1.0, 960.0),
                            done: true,
                        })
                    }
                    Hit::Rate => {
                        let step = if mods.shift { 0.001 } else { 0.01 };
                        Some(Act::SetRate {
                            rate: (self.st.rate + notches as f64 * step).clamp(0.25, 4.0),
                            done: true,
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn drag_value(&self, d: Drag, dy: f32, mods: Mods, done: bool) -> Act {
        match d.what {
            Hit::Rate => {
                let k = if mods.shift { 0.0005 } else { 0.005 };
                let r = (d.v0 + dy as f64 * k).clamp(0.25, 4.0);
                Act::SetRate {
                    rate: (r * 1000.0).round() / 1000.0,
                    done,
                }
            }
            _ => {
                let k = if mods.shift { 0.01 } else { 0.25 };
                let b = (d.v0 + dy as f64 * k).clamp(1.0, 960.0);
                Act::SetBpm {
                    bpm: (b * 100.0).round() / 100.0,
                    done,
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Le dessin
    // -----------------------------------------------------------------------

    fn push(&mut self, hit: Hit, x: f32, y: f32, w: f32, h: f32) {
        self.boxes.push(Box_ { hit, x, y, w, h });
    }

    fn bg_for(&self, h: Hit, th: &Theme) -> Option<Color> {
        if self.pressed == h && h != Hit::None {
            Some(th.sel_dim)
        } else if self.hover == h && h != Hit::None {
            Some(th.hover)
        } else {
            None
        }
    }

    /// Dessine toute la barre (`w x h` px) et retient ou sont les cibles.
    /// Avec un [`Skin`], les images et couleurs du theme de REAPER ; sans,
    /// les icones vectorielles de CP Studio.
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        skin: Option<&Skin>,
        w: f32,
        h: f32,
    ) {
        self.boxes.clear();
        if let Some(sk) = skin {
            self.draw_skin(cv, text, icons, th, sk, w, h);
            return;
        }
        cv.fill_rect(0.0, 0.0, w, h, th.bar);
        cv.fill_rect(0.0, h - 1.0, w, 1.0, th.seam);
        let pad = th.s(4.0);
        let bh = (h - 2.0 * pad).max(8.0);
        let by = ((h - bh) * 0.5).floor();
        let isz = th.s(16.0) as u16;
        let r = th.s(4.0);
        let mut x = th.s(6.0);

        // --- le transport
        let play = self.st.play;
        let buttons: [(Hit, Icon, Color, bool); 6] = [
            (Hit::Start, Icon::SkipBack, th.text_dim, false),
            (Hit::Stop, Icon::Stop, th.text_dim, false),
            (
                Hit::Play,
                Icon::Play,
                if play & PLAYING != 0 && play & PAUSED == 0 {
                    hex(GREEN)
                } else {
                    th.text_dim
                },
                play & PLAYING != 0 && play & PAUSED == 0,
            ),
            (
                Hit::Pause,
                Icon::Pause,
                if play & PAUSED != 0 {
                    hex(AMBER)
                } else {
                    th.text_dim
                },
                false,
            ),
            (
                Hit::Record,
                Icon::Circle,
                if play & RECORDING != 0 {
                    hex(RED)
                } else {
                    hex(RED).with_alpha(170)
                },
                play & RECORDING != 0,
            ),
            (
                Hit::Repeat,
                Icon::Repeat,
                if self.st.repeat {
                    th.accent
                } else {
                    th.text_dim
                },
                false,
            ),
        ];
        let bw = bh;
        for (hit, icon, color, filled) in buttons {
            if let Some(c) = self.bg_for(hit, th) {
                cv.fill_round_rect(x, by, bw, bh, r, c);
            }
            if hit == Hit::Repeat && self.st.repeat {
                cv.fill_round_rect(x, by, bw, bh, r, th.accent.with_alpha(40));
            }
            let ix = x + ((bw - isz as f32) * 0.5).round();
            let iy = by + ((bh - isz as f32) * 0.5).round();
            icons.draw(cv, icon, ix, iy, isz, color, filled);
            if hit == Hit::Record {
                rec_mode_mark(cv, th, self.st.rec_mode, x, by, bw, bh, color);
            }
            self.push(hit, x, by, bw, bh);
            x += bw + th.s(2.0);
        }
        x += th.s(8.0);

        // --- le compteur
        let big = Font::new(1, th.s(17.0) as u16, 600);
        let small = Font::new(1, th.s(11.0) as u16, 400);
        let cw_beats = text.measure(big, "0000.00.00").ceil();
        let cw_time = text.measure(small, "00:00:00.000").ceil();
        let cw = cw_beats + cw_time + th.s(22.0);
        cv.fill_round_rect(x, by, cw, bh, r, th.field);
        let ink = TunerInk {
            text: th.text,
            dim: th.text_dim,
            good: hex(GREEN),
            warn: hex(AMBER),
            bad: hex(RED),
            track: th.seam,
        };
        if self.tuner_in_clock() {
            draw_tuner(
                cv,
                text,
                0,
                th.scale,
                x,
                by,
                cw,
                bh,
                self.tuner.as_ref(),
                &ink,
            );
        } else {
            let beats_c = if play & RECORDING != 0 {
                hex(RED)
            } else if play & PLAYING != 0 {
                hex(GREEN)
            } else {
                th.text
            };
            text.draw_in(
                cv,
                big,
                x + th.s(8.0),
                by,
                cw_beats,
                bh,
                &self.st.beats,
                beats_c,
                Align::Left,
            );
            text.draw_in(
                cv,
                small,
                x + cw - cw_time - th.s(8.0),
                by,
                cw_time,
                bh,
                &self.st.time,
                th.text_dim,
                Align::Right,
            );
        }
        self.push(Hit::Counter, x, by, cw, bh);
        x += cw + th.s(8.0);

        // --- tempo, signature, tap, metronome, vitesse
        let mid = Font::new(1, th.s(14.0) as u16, 600);
        let tiny = th.font_small();
        let tw = text.measure(mid, "000.00").ceil() + text.measure(tiny, "BPM").ceil() + th.s(16.0);
        let bg = self.bg_for(Hit::Bpm, th).unwrap_or(th.field);
        cv.fill_round_rect(x, by, tw, bh, r, bg);
        let bpm = fmt_bpm(self.st.bpm);
        let lw = text.measure(tiny, "BPM").ceil();
        text.draw_in(
            cv,
            mid,
            x + th.s(6.0),
            by,
            tw - lw - th.s(12.0),
            bh,
            &bpm,
            th.text,
            Align::Right,
        );
        text.draw_in(
            cv,
            tiny,
            x + tw - lw - th.s(5.0),
            by,
            lw,
            bh,
            "BPM",
            th.text_faint,
            Align::Left,
        );
        self.push(Hit::Bpm, x, by, tw, bh);
        x += tw + th.s(4.0);

        let sig = format!("{}/{}", self.st.sig.0, self.st.sig.1);
        let sw = text.measure(mid, "00/00").ceil() + th.s(12.0);
        let bg = self.bg_for(Hit::Sig, th).unwrap_or(th.field);
        cv.fill_round_rect(x, by, sw, bh, r, bg);
        text.draw_in(cv, mid, x, by, sw, bh, &sig, th.text, Align::Center);
        self.push(Hit::Sig, x, by, sw, bh);
        x += sw + th.s(4.0);

        let tapw = text.measure(th.font_bold(), "TAP").ceil() + th.s(14.0);
        if let Some(c) = self.bg_for(Hit::Tap, th) {
            cv.fill_round_rect(x, by, tapw, bh, r, c);
        }
        cv.stroke_round_rect(x + 0.5, by + 0.5, tapw - 1.0, bh - 1.0, r, 1.0, th.seam);
        text.draw_in(
            cv,
            th.font_bold(),
            x,
            by,
            tapw,
            bh,
            "TAP",
            th.text_dim,
            Align::Center,
        );
        self.push(Hit::Tap, x, by, tapw, bh);
        x += tapw + th.s(4.0);

        if let Some(c) = self.bg_for(Hit::Metronome, th) {
            cv.fill_round_rect(x, by, bw, bh, r, c);
        }
        if self.st.metronome {
            cv.fill_round_rect(x, by, bw, bh, r, th.accent.with_alpha(40));
        }
        let mc = if self.st.metronome {
            th.accent
        } else {
            th.text_dim
        };
        icons.draw(
            cv,
            Icon::Metronome,
            x + ((bw - isz as f32) * 0.5).round(),
            by + ((bh - isz as f32) * 0.5).round(),
            isz,
            mc,
            false,
        );
        self.push(Hit::Metronome, x, by, bw, bh);
        x += bw + th.s(4.0);

        let rate = format!("{:.2}x", self.st.rate);
        let rw = text.measure(mid, "0.00x").ceil() + th.s(12.0);
        let bg = self.bg_for(Hit::Rate, th).unwrap_or(th.field);
        cv.fill_round_rect(x, by, rw, bh, r, bg);
        let rc = if (self.st.rate - 1.0).abs() > 1e-6 {
            hex(AMBER)
        } else {
            th.text_dim
        };
        text.draw_in(cv, mid, x, by, rw, bh, &rate, rc, Align::Center);
        self.push(Hit::Rate, x, by, rw, bh);
        x += rw + th.s(14.0);

        // --- le selecteur de vues, a droite
        let vf = th.font_bold();
        let mut widths = Vec::with_capacity(self.views.len());
        for v in &self.views {
            widths.push(text.measure(vf, &v.name).ceil() + th.s(22.0));
        }
        let total: f32 =
            widths.iter().sum::<f32>() + th.s(2.0) * widths.len().saturating_sub(1) as f32;
        let mut vx = (w - th.s(6.0) - total).max(x);
        let strip_end = vx - th.s(10.0);
        if !self.views.is_empty() {
            cv.fill_round_rect(
                vx - th.s(2.0),
                by - th.s(1.0),
                total + th.s(4.0),
                bh + th.s(2.0),
                r + th.s(1.0),
                th.field,
            );
        }
        for i in 0..self.views.len() {
            let vw = widths[i];
            let v = self.views[i].clone();
            let hit = Hit::View(i);
            if v.active {
                cv.fill_round_rect(vx, by, vw, bh, r, th.sel);
            } else if let Some(c) = self.bg_for(hit, th) {
                cv.fill_round_rect(vx, by, vw, bh, r, c);
            }
            let c = if v.active { th.text } else { th.text_dim };
            text.draw_in(cv, vf, vx, by, vw, bh, &v.name, c, Align::Center);
            if v.open && !v.active {
                let d = th.s(2.0).max(1.5);
                cv.fill_circle(vx + vw * 0.5, by + bh - th.s(4.0), d, th.accent);
            }
            self.push(hit, vx, by, vw, bh);
            vx += vw + th.s(2.0);
        }

        // --- les icones, entre les deux
        let isz2 = th.s(20.0) as u16;
        for i in 0..self.items.len() {
            if x + bw > strip_end {
                break;
            }
            let hit = Hit::Item(i);
            if let Some(c) = self.bg_for(hit, th) {
                cv.fill_round_rect(x, by, bw, bh, r, c);
            }
            let it = &self.items[i];
            if it.on {
                cv.fill_round_rect(x, by, bw, bh, r, th.accent.with_alpha(50));
            }
            let ix = x + ((bw - isz2 as f32) * 0.5).round();
            let iy = by + ((bh - isz2 as f32) * 0.5).round();
            match &it.glyph {
                Glyph::Frames(f) => {
                    if let Some(img) = f.get(if it.on { 2 } else { 0 }) {
                        let ox = x + ((bw - img.w as f32) * 0.5).round();
                        let oy = by + ((bh - img.h as f32) * 0.5).round();
                        cv.draw_image(img, ox, oy, 1.0);
                    }
                }
                Glyph::Image(img) => {
                    let ox = x + ((bw - img.w as f32) * 0.5).round();
                    let oy = by + ((bh - img.h as f32) * 0.5).round();
                    cv.draw_image(img, ox, oy, 1.0);
                }
                Glyph::Icon(icon) => {
                    let c = if it.on { th.accent } else { th.text_dim };
                    icons.draw(cv, *icon, ix, iy, isz2, c, false);
                }
                Glyph::Text(t) => {
                    cv.stroke_round_rect(
                        x + th.s(3.0) + 0.5,
                        by + th.s(3.0) + 0.5,
                        bw - th.s(6.0) - 1.0,
                        bh - th.s(6.0) - 1.0,
                        r,
                        1.0,
                        th.text_faint,
                    );
                    text.draw_in(
                        cv,
                        th.font_bold(),
                        x,
                        by,
                        bw,
                        bh,
                        t,
                        th.text_dim,
                        Align::Center,
                    );
                }
            }
            if it.open {
                let d = th.s(2.0).max(1.5);
                cv.fill_circle(x + bw * 0.5, by + bh - th.s(2.0), d, th.accent);
            }
            self.push(hit, x, by, bw, bh);
            x += bw + th.s(2.0);
        }
        if x + bw <= strip_end {
            if let Some(c) = self.bg_for(Hit::Add, th) {
                cv.fill_round_rect(x, by, bw, bh, r, c);
            }
            let s = th.s(14.0) as u16;
            icons.draw(
                cv,
                Icon::Plus,
                x + ((bw - s as f32) * 0.5).round(),
                by + ((bh - s as f32) * 0.5).round(),
                s,
                th.text_faint,
                false,
            );
            self.push(Hit::Add, x, by, bw, bh);
            x += bw + th.s(2.0);
        }
        if self.tuner_mode == TunerMode::Slot {
            let tw = slot_width(th.scale);
            if strip_end - x >= tw + th.s(16.0) {
                let tx = (x + (strip_end - x - tw) * 0.5).round();
                cv.fill_round_rect(tx, by, tw, bh, r, th.field);
                draw_tuner(
                    cv,
                    text,
                    0,
                    th.scale,
                    tx,
                    by,
                    tw,
                    bh,
                    self.tuner.as_ref(),
                    &ink,
                );
                self.push(Hit::Tuner, tx, by, tw, bh);
            }
        }
    }

    /// L'accordeur a la place du compteur : a l'arret, une note entendue.
    pub fn tuner_in_clock(&self) -> bool {
        self.tuner_mode == TunerMode::Clock
            && self.tuner.is_some()
            && self.st.play & (PLAYING | RECORDING) == 0
    }
}
