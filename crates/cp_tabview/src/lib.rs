//! La fenetre de CP Guitar Tab, sans Windows ni REAPER : la partition et la
//! tablature d'une prise, liees (une seule liste de notes, deux dessins),
//! la detection reglable en direct, et les notes FANTOMES (ce que le reseau a
//! presque entendu, gardees d'un clic).
//!
//! Le module tient la prise (l'analyse, l'item MIDI) et execute les [`Act`] :
//! ecrire le MIDI apres une correction, redecoder quand un reglage change,
//! deplacer le curseur de REAPER, exporter. Ici : l'etat de la fenetre, le
//! dessin (`draw.rs`) et les gestes.
//!
//! L'axe du temps est en NOIRES DU PROJET, proportionnel : la partition, la
//! tablature, le son et les attaques s'alignent a la verticale.

#![allow(clippy::too_many_arguments)]

pub mod draw;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::time::Instant;

use cp_tab::doc::{TNote, Tech};
use cp_tab::engrave::{Engraved, engrave};
use cp_tab::fret::Tuning;
use cp_ui::{Button, Ev, Mods, vk};

/// Les accordages proposes (de la corde grave a l'aigue).
pub const TUNINGS: [(&str, [i32; 6]); 5] = [
    ("Standard E", [40, 45, 50, 55, 59, 64]),
    ("Drop D", [38, 45, 50, 55, 59, 64]),
    ("Eb Standard", [39, 44, 49, 54, 58, 63]),
    ("DADGAD", [38, 45, 50, 55, 57, 62]),
    ("Open G", [38, 43, 50, 55, 59, 62]),
];

/// Les reglages de la detection (ExtState `gtab_detect`).
#[derive(Clone, Debug, PartialEq)]
pub struct Detect {
    /// Le seuil d'une attaque, 0..1 : plus bas, plus de notes (et de bruit).
    pub onset: f32,
    /// Le seuil d'une note qui sonne.
    pub frame: f32,
    /// La plus courte note gardee.
    pub min_ms: f64,
    /// Cases de la grille par noire : 2, 4 ou 8.
    pub grid: u32,
    /// Les notes sans attaque (marteaux, tires).
    pub legato: bool,
    /// Montrer les notes fantomes.
    pub ghosts: bool,
    /// Rang dans [`TUNINGS`].
    pub tuning: usize,
}

impl Default for Detect {
    fn default() -> Self {
        Detect {
            onset: 0.5,
            frame: 0.3,
            min_ms: 127.7,
            grid: 4,
            legato: true,
            ghosts: true,
            tuning: 0,
        }
    }
}

impl Detect {
    pub fn tuning(&self) -> Tuning {
        Tuning {
            strings: TUNINGS[self.tuning.min(TUNINGS.len() - 1)].1.to_vec(),
            frets: 22,
        }
    }

    /// Le seuil d'une note fantome : sous celui des notes gardees.
    pub fn ghost_onset(&self) -> f32 {
        (self.onset * 0.6).max(0.08)
    }

    pub fn to_text(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.onset,
            self.frame,
            self.min_ms,
            self.grid,
            self.legato as u8,
            self.ghosts as u8,
            self.tuning
        )
    }

    pub fn from_text(s: &str) -> Detect {
        let d = Detect::default();
        let v: Vec<&str> = s.split_whitespace().collect();
        let f = |i: usize| v.get(i).and_then(|x| x.parse::<f64>().ok());
        Detect {
            onset: f(0)
                .filter(|x| (0.05..=0.95).contains(x))
                .map_or(d.onset, |x| x as f32),
            frame: f(1)
                .filter(|x| (0.05..=0.95).contains(x))
                .map_or(d.frame, |x| x as f32),
            min_ms: f(2)
                .filter(|x| (20.0..=1000.0).contains(x))
                .unwrap_or(d.min_ms),
            grid: f(3)
                .map(|x| x as u32)
                .filter(|g| [2, 4, 8].contains(g))
                .unwrap_or(d.grid),
            legato: f(4).map_or(d.legato, |x| x != 0.0),
            ghosts: f(5).map_or(d.ghosts, |x| x != 0.0),
            tuning: f(6)
                .map(|x| x as usize)
                .filter(|&t| t < TUNINGS.len())
                .unwrap_or(d.tuning),
        }
    }
}

/// Ce que la fenetre montre : la partition et la tablature, l'une ou l'autre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Both,
    Tab,
    Score,
}

/// La prise : ce que le module sait d'elle.
#[derive(Clone, Debug, Default)]
pub struct Take {
    /// « 02-210831_1211.wav »
    pub name: String,
    /// La piste de l'item audio.
    pub track: String,
    pub len_s: f64,
    /// L'item, en noires du projet.
    pub qn0: f64,
    pub qn1: f64,
    /// La barre de mesure ou commence la partition, et son numero (1 = la
    /// premiere mesure du projet).
    pub origin_qn: f64,
    pub first_bar: i32,
    pub sig: (i32, i32),
    pub bpm: f64,
    /// La piste MIDI liee, si elle existe.
    pub midi: Option<String>,
    /// Pour chaque trame de l'analyse : sa position en noires, la crete du
    /// son, l'attaque la plus forte.
    pub frame_qn: Vec<f64>,
    pub env: Vec<f32>,
    pub onset: Vec<f32>,
}

/// Ce que vise la souris.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Note(usize),
    Ghost(usize),
    /// Une ligne de la tablature (corde) a une position.
    TabLine(usize),
    Ruler,
    Lanes,
    Overview,
    Transcribe,
    Mode(Mode),
    ZoomOut,
    ZoomIn,
    Follow,
    Export,
    Slider(u8),
    Check(u8),
    Grid(u32),
    Tuning,
    KeepGhosts,
    Retranscribe,
}

/// Ce que le module doit faire.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Deplacer le curseur d'edition de REAPER (noires).
    Seek(f64),
    /// Lecture / arret (Espace).
    Play,
    /// L'annulation de REAPER (Ctrl+Z).
    Undo,
    /// La tablature a ete corrigee : l'ecrire dans l'item MIDI.
    Edited,
    /// Un reglage a change : redecoder (seulement ce passage, s'il y en a un).
    Detect {
        range: Option<(f64, f64)>,
    },
    /// L'accordage a change : refaire le doigte.
    Retune,
    /// Relancer le reseau sur la prise (ou sur l'item choisi).
    Transcribe,
    Export,
    /// Clic droit sur la barre : la place de la fenetre.
    ViewMenu {
        x: f32,
        y: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Une note deplacee : le debut d'origine et la position de la souris.
    Note {
        i: usize,
        qn0: f64,
        x0: f32,
        moved: bool,
    },
    /// Un passage choisi sur la regle.
    Range {
        q0: f64,
    },
    Slider(u8),
    Overview,
}

/// Les zones de la fenetre (calculees au dessin).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lay {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    pub sub_h: f32,
    pub status_h: f32,
    pub panel_w: f32,
    /// La zone de la partition : x de debut (apres la cle), largeur, y.
    pub sx: f32,
    pub sw: f32,
    pub top: f32,
    pub bottom: f32,
    /// Pixels par noire.
    pub ppq: f32,
    pub ruler_y: f32,
    pub score_y: f32,
    pub tab_y: f32,
    pub tab_gap: f32,
    pub audio_y: f32,
    pub audio_h: f32,
    pub onset_y: f32,
    pub onset_h: f32,
    pub over_y: f32,
    pub over_h: f32,
    pub panel_x: f32,
}

pub struct TabView {
    pub take: Option<Take>,
    pub doc: Vec<TNote>,
    pub ghosts: Vec<TNote>,
    /// Les notes effacees par Cedric (un nouveau decodage ne les ramene pas).
    pub suppressed: HashSet<(i64, i32)>,
    pub detect: Detect,
    pub mode: Mode,
    /// Pixels par noire a 96 DPI.
    pub zoom: f32,
    /// La noire au bord gauche de la partition.
    pub scroll: f64,
    pub follow: bool,
    pub playhead: Option<f64>,
    pub playing: bool,
    pub sel: Option<usize>,
    pub range: Option<(f64, f64)>,
    /// Ce qui se passe (« Transcribing 45 % », « Recording… »).
    pub busy: Option<String>,
    pub status: String,
    /// Notes injouables laissees de cote par le doigte.
    pub dropped: usize,
    /// Ce que dit le pied du panneau (le reseau, son temps).
    pub net_note: String,
    pub lay: Lay,
    pub scale: f32,
    eng: Engraved,
    eng_dirty: bool,
    boxes: Vec<(f32, f32, f32, f32, Hit)>,
    hover: Hit,
    drag: Option<Drag>,
    /// Le premier chiffre d'une case tapee (pour « 12 »), et quand.
    digit: Option<(i32, Instant)>,
    dirty: bool,
}

impl Default for TabView {
    fn default() -> Self {
        TabView::new()
    }
}

impl TabView {
    pub fn new() -> TabView {
        TabView {
            take: None,
            doc: Vec::new(),
            ghosts: Vec::new(),
            suppressed: HashSet::new(),
            detect: Detect::default(),
            mode: Mode::Both,
            zoom: 72.0,
            scroll: 0.0,
            follow: true,
            playhead: None,
            playing: false,
            sel: None,
            range: None,
            busy: None,
            status: String::new(),
            dropped: 0,
            net_note: String::new(),
            lay: Lay::default(),
            scale: 1.0,
            eng: Engraved::default(),
            eng_dirty: true,
            boxes: Vec::new(),
            hover: Hit::None,
            drag: None,
            digit: None,
            dirty: true,
        }
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Une nouvelle tablature (decodage, relecture du MIDI) : la partition
    /// est a refaire.
    pub fn set_doc(&mut self, doc: Vec<TNote>) {
        let keep = self.sel.and_then(|i| self.doc.get(i)).map(TNote::key);
        self.doc = doc;
        self.sel = keep.and_then(|k| self.doc.iter().position(|t| t.key() == k));
        self.eng_dirty = true;
        self.dirty = true;
    }

    pub fn set_take(&mut self, t: Take) {
        let fresh = self
            .take
            .as_ref()
            .is_none_or(|o| o.name != t.name || o.qn0 != t.qn0);
        if fresh {
            self.scroll = t.origin_qn;
            self.sel = None;
            self.range = None;
        }
        self.take = Some(t);
        self.eng_dirty = true;
        self.dirty = true;
    }

    /// La partition (refaite si la tablature a change).
    pub fn engraved(&mut self) -> &Engraved {
        if self.eng_dirty {
            let (origin, sig) = self
                .take
                .as_ref()
                .map_or((0.0, (4, 4)), |t| (t.origin_qn, t.sig));
            self.eng = engrave(&self.doc, origin, sig, self.detect.grid);
            self.eng_dirty = false;
        }
        &self.eng
    }

    /// Les noires d'une mesure.
    pub fn bar_qn(&self) -> f64 {
        let (n, d) = self.take.as_ref().map_or((4, 4), |t| t.sig);
        n.max(1) as f64 * 4.0 / d.max(1) as f64
    }

    pub fn x_of(&self, qn: f64) -> f32 {
        self.lay.sx + ((qn - self.scroll) as f32) * self.lay.ppq
    }

    pub fn qn_of(&self, x: f32) -> f64 {
        self.scroll + ((x - self.lay.sx) / self.lay.ppq.max(1e-3)) as f64
    }

    /// La noire visible a droite.
    pub fn view_qn(&self) -> f64 {
        (self.lay.sw / self.lay.ppq.max(1e-3)) as f64
    }

    /// Une position calee sur la grille.
    fn snap(&self, qn: f64) -> f64 {
        let g = self.detect.grid.max(1) as f64;
        let o = self.take.as_ref().map_or(0.0, |t| t.origin_qn);
        o + ((qn - o) * g).round() / g
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
                self.scroll = p - v * 0.1;
                self.dirty = true;
            }
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        self.boxes
            .iter()
            .rev()
            .find(|b| x >= b.0 && x < b.0 + b.2 && y >= b.1 && y < b.1 + b.3)
            .map_or(Hit::None, |b| b.4)
    }

    fn push(&mut self, x: f32, y: f32, w: f32, h: f32, hit: Hit) {
        self.boxes.push((x, y, w, h, hit));
    }

    // -----------------------------------------------------------------------
    // Les corrections
    // -----------------------------------------------------------------------

    fn edited(&mut self) -> Option<Act> {
        self.eng_dirty = true;
        self.dirty = true;
        Some(Act::Edited)
    }

    /// Change la case de la note choisie (meme corde, autre hauteur).
    pub fn set_fret(&mut self, fret: i32) -> Option<Act> {
        let t = self.detect.tuning();
        let i = self.sel?;
        let n = self.doc.get_mut(i)?;
        let open = *t.strings.get(n.string)?;
        if !(0..=t.frets).contains(&fret) {
            return None;
        }
        n.fret = fret;
        n.pitch = open + fret;
        n.locked = true;
        self.edited()
    }

    /// La meme note sur la corde voisine (`dir` +1 : plus aigue).
    pub fn change_string(&mut self, dir: i32) -> Option<Act> {
        let t = self.detect.tuning();
        let i = self.sel?;
        let n = self.doc.get(i)?;
        let mut s = n.string as i32 + dir;
        while s >= 0 && (s as usize) < t.strings.len() {
            let f = n.pitch - t.strings[s as usize];
            if (0..=t.frets).contains(&f) {
                let n = &mut self.doc[i];
                n.string = s as usize;
                n.fret = f;
                n.locked = true;
                return self.edited();
            }
            s += dir;
        }
        None
    }

    pub fn set_tech(&mut self, tech: Tech) -> Option<Act> {
        let i = self.sel?;
        let n = self.doc.get_mut(i)?;
        n.tech = if n.tech == tech { Tech::Plain } else { tech };
        n.locked = true;
        self.edited()
    }

    pub fn delete_selected(&mut self) -> Option<Act> {
        let i = self.sel?;
        if i >= self.doc.len() {
            return None;
        }
        let n = self.doc.remove(i);
        self.suppressed.insert(n.key());
        self.sel = if self.doc.is_empty() {
            None
        } else {
            Some(i.min(self.doc.len() - 1))
        };
        self.edited()
    }

    /// Une note neuve, tenue, sur une corde, a une position (calee).
    pub fn add_note(&mut self, qn: f64, string: usize, fret: i32) -> Option<Act> {
        let t = self.detect.tuning();
        let open = *t.strings.get(string)?;
        let q0 = self.snap(qn);
        let n = TNote {
            qn0: q0,
            qn1: q0 + 2.0 / self.detect.grid.max(1) as f64,
            pitch: open + fret,
            string,
            fret,
            vel: 96,
            tech: Tech::Plain,
            locked: true,
        };
        let k = n.key();
        self.doc.push(n);
        cp_tab::doc::sort(&mut self.doc);
        self.sel = self.doc.iter().position(|x| x.key() == k);
        self.edited()
    }

    /// Garde une note fantome : elle entre dans la tablature, tenue.
    pub fn keep_ghost(&mut self, gi: usize) -> Option<Act> {
        if gi >= self.ghosts.len() {
            return None;
        }
        let mut n = self.ghosts.remove(gi);
        n.locked = true;
        let k = n.key();
        self.suppressed.remove(&k);
        self.doc.push(n);
        cp_tab::doc::sort(&mut self.doc);
        self.sel = self.doc.iter().position(|x| x.key() == k);
        self.edited()
    }

    /// Garde toutes les notes fantomes du passage choisi (ou toutes).
    pub fn keep_ghosts(&mut self) -> Option<Act> {
        let r = self.range;
        let inside = |t: &TNote| r.is_none_or(|(a, b)| t.qn0 >= a && t.qn0 < b);
        let (keep, rest): (Vec<TNote>, Vec<TNote>) = std::mem::take(&mut self.ghosts)
            .into_iter()
            .partition(inside);
        self.ghosts = rest;
        if keep.is_empty() {
            return None;
        }
        for mut n in keep {
            n.locked = true;
            self.suppressed.remove(&n.key());
            self.doc.push(n);
        }
        cp_tab::doc::sort(&mut self.doc);
        self.edited()
    }

    /// Deplace la note choisie de `cells` cases de grille.
    fn move_note(&mut self, i: usize, qn0: f64) -> bool {
        let Some(n) = self.doc.get_mut(i) else {
            return false;
        };
        let len = n.qn1 - n.qn0;
        if (n.qn0 - qn0).abs() < 1e-9 {
            return false;
        }
        n.qn0 = qn0;
        n.qn1 = qn0 + len;
        n.locked = true;
        self.eng_dirty = true;
        true
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn select(&mut self, i: Option<usize>) {
        if i != self.sel {
            self.sel = i;
            self.digit = None;
            self.dirty = true;
        }
        if let Some(n) = i.and_then(|i| self.doc.get(i)) {
            // La note choisie reste a l'ecran.
            let v = self.view_qn();
            if n.qn0 < self.scroll || n.qn0 > self.scroll + v * 0.9 {
                self.scroll = n.qn0 - v * 0.2;
            }
        }
    }

    fn set_slider(&mut self, k: u8, x: f32) -> Option<Act> {
        let l = self.lay;
        let x0 = l.panel_x + 120.0 * self.scale;
        let w = (l.w - x0 - 64.0 * self.scale).max(10.0);
        let t = ((x - x0) / w).clamp(0.0, 1.0);
        // Les seuils de 0,05 a 0,95 au centieme ; la note la plus courte de
        // 20 a 400 ms.
        let v = ((0.05 + t * 0.9) * 100.0).round() / 100.0;
        match k {
            0 => self.detect.onset = v,
            1 => self.detect.frame = v,
            _ => self.detect.min_ms = (20.0 + t as f64 * 380.0).round(),
        }
        self.dirty = true;
        Some(Act::Detect { range: self.range })
    }

    /// Un evenement de la fenetre ; rend ce que le module doit faire.
    pub fn event(&mut self, ev: &Ev) -> Option<Act> {
        match *ev {
            Ev::Move { x, y, .. } => {
                match self.drag {
                    Some(Drag::Note { i, qn0, x0, moved }) => {
                        if !moved && (x - x0).abs() < 4.0 * self.scale {
                            return None;
                        }
                        let q = self.snap(qn0 + ((x - x0) / self.lay.ppq.max(1e-3)) as f64);
                        self.drag = Some(Drag::Note {
                            i,
                            qn0,
                            x0,
                            moved: true,
                        });
                        if self.move_note(i, q) {
                            self.dirty = true;
                        }
                        return None;
                    }
                    Some(Drag::Range { q0 }) => {
                        let q = self.snap(self.qn_of(x));
                        self.range = Some((q0.min(q), q0.max(q)));
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::Slider(k)) => return self.set_slider(k, x),
                    Some(Drag::Overview) => {
                        self.overview_to(x);
                        return None;
                    }
                    None => {}
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
            Ev::Down { x, y, b, dbl, mods } => self.down(x, y, b, dbl, mods),
            Ev::Up { .. } => {
                let d = self.drag.take();
                self.dirty = true;
                match d {
                    Some(Drag::Note { moved: true, i, .. }) => {
                        let k = self.doc.get(i).map(TNote::key);
                        cp_tab::doc::sort(&mut self.doc);
                        self.sel = k.and_then(|k| self.doc.iter().position(|t| t.key() == k));
                        self.edited()
                    }
                    Some(Drag::Range { .. }) => {
                        // Un simple clic sur la regle : pas de passage.
                        if self.range.is_some_and(|(a, b)| b - a < 1e-6) {
                            self.range = None;
                        }
                        None
                    }
                    _ => None,
                }
            }
            Ev::Wheel {
                dy, dx, mods, x, ..
            } => {
                if mods.ctrl {
                    let q = self.qn_of(x);
                    let k = if dy > 0.0 { 1.25 } else { 0.8 };
                    self.zoom = (self.zoom * k).clamp(16.0, 400.0);
                    self.lay.ppq = self.zoom * self.scale;
                    // La noire sous la souris reste sous la souris.
                    self.scroll = q - ((x - self.lay.sx) / self.lay.ppq) as f64;
                } else {
                    let d = if dx != 0.0 { dx } else { -dy };
                    self.scroll += d as f64 * self.view_qn() * 0.15;
                }
                self.follow_off();
                self.dirty = true;
                None
            }
            Ev::Key { vk: k, mods, .. } => self.key(k, mods),
            Ev::Char(_) => None,
        }
    }

    fn follow_off(&mut self) {
        // Faire defiler a la main pendant la lecture : on cesse de suivre.
        if self.playing {
            self.follow = false;
        }
    }

    fn overview_to(&mut self, x: f32) {
        let Some(t) = &self.take else { return };
        let l = self.lay;
        let span = (t.qn1 - t.origin_qn).max(1.0);
        let q = t.origin_qn + ((x - l.sx) / l.sw.max(1.0)) as f64 * span;
        self.scroll = q - self.view_qn() * 0.5;
        self.dirty = true;
    }

    fn down(&mut self, x: f32, y: f32, b: Button, dbl: bool, _mods: Mods) -> Option<Act> {
        let h = self.hit(x, y);
        self.dirty = true;
        if b == Button::Right {
            if y < self.lay.bar_h {
                return Some(Act::ViewMenu { x, y });
            }
            return None;
        }
        if b != Button::Left {
            return None;
        }
        match h {
            Hit::Note(i) => {
                self.select(Some(i));
                let qn0 = self.doc[i].qn0;
                self.drag = Some(Drag::Note {
                    i,
                    qn0,
                    x0: x,
                    moved: false,
                });
                None
            }
            Hit::Ghost(gi) => self.keep_ghost(gi),
            Hit::TabLine(s) => {
                if dbl {
                    self.add_note(self.qn_of(x), s, 0)
                } else {
                    self.select(None);
                    Some(Act::Seek(self.snap(self.qn_of(x))))
                }
            }
            Hit::Ruler => {
                let q = self.snap(self.qn_of(x));
                self.range = None;
                self.drag = Some(Drag::Range { q0: q });
                Some(Act::Seek(q))
            }
            Hit::Lanes => {
                self.select(None);
                Some(Act::Seek(self.qn_of(x)))
            }
            Hit::Overview => {
                self.drag = Some(Drag::Overview);
                self.overview_to(x);
                None
            }
            Hit::Transcribe | Hit::Retranscribe => Some(Act::Transcribe),
            Hit::Mode(m) => {
                self.mode = m;
                None
            }
            Hit::ZoomOut => {
                self.zoom = (self.zoom * 0.8).clamp(16.0, 400.0);
                None
            }
            Hit::ZoomIn => {
                self.zoom = (self.zoom * 1.25).clamp(16.0, 400.0);
                None
            }
            Hit::Follow => {
                self.follow = !self.follow;
                None
            }
            Hit::Export => Some(Act::Export),
            Hit::Slider(k) => {
                self.drag = Some(Drag::Slider(k));
                self.set_slider(k, x)
            }
            Hit::Check(k) => {
                if k == 0 {
                    self.detect.legato = !self.detect.legato;
                    Some(Act::Detect { range: self.range })
                } else {
                    self.detect.ghosts = !self.detect.ghosts;
                    Some(Act::Detect { range: None })
                }
            }
            Hit::Grid(g) => {
                if self.detect.grid != g {
                    self.detect.grid = g;
                    self.eng_dirty = true;
                    Some(Act::Detect { range: None })
                } else {
                    None
                }
            }
            Hit::Tuning => {
                self.detect.tuning = (self.detect.tuning + 1) % TUNINGS.len();
                Some(Act::Retune)
            }
            Hit::KeepGhosts => self.keep_ghosts(),
            Hit::None => {
                self.select(None);
                None
            }
        }
    }

    fn key(&mut self, k: u32, m: Mods) -> Option<Act> {
        self.dirty = true;
        match k {
            vk::SPACE => Some(Act::Play),
            0x5A if m.ctrl => Some(Act::Undo), // Ctrl+Z
            vk::ESCAPE => {
                self.select(None);
                self.range = None;
                None
            }
            vk::LEFT | vk::RIGHT => {
                if self.doc.is_empty() {
                    return None;
                }
                let n = self.doc.len();
                let i = match (self.sel, k == vk::RIGHT) {
                    (None, _) => {
                        // La premiere note visible.
                        let s = self.scroll;
                        self.doc.iter().position(|t| t.qn0 >= s).unwrap_or(0)
                    }
                    (Some(i), true) => (i + 1).min(n - 1),
                    (Some(i), false) => i.saturating_sub(1),
                };
                self.select(Some(i));
                None
            }
            vk::UP => self.change_string(1),
            vk::DOWN => self.change_string(-1),
            vk::DELETE | vk::BACK => self.delete_selected(),
            0x48 if !m.ctrl => self.set_tech(Tech::Hammer), // H
            0x50 if !m.ctrl => self.set_tech(Tech::Pull),   // P
            0x30..=0x39 | 0x60..=0x69 => {
                let d = if k >= 0x60 { k - 0x60 } else { k - 0x30 } as i32;
                // Deux chiffres a moins de 0,8 s : une case a deux chiffres.
                let fret = match self.digit {
                    Some((a, t)) if t.elapsed().as_secs_f64() < 0.8 && a * 10 + d <= 24 => {
                        a * 10 + d
                    }
                    _ => d,
                };
                self.digit = Some((fret, Instant::now()));
                self.set_fret(fret)
            }
            _ => None,
        }
    }

    /// Les touches que la fenetre traite (les autres vont a REAPER).
    pub fn wants_key(&self, k: u32, m: Mods) -> bool {
        matches!(
            k,
            vk::SPACE
                | vk::ESCAPE
                | vk::LEFT
                | vk::RIGHT
                | vk::UP
                | vk::DOWN
                | vk::DELETE
                | vk::BACK
                | 0x30..=0x39
                | 0x60..=0x69
        ) || (matches!(k, 0x48 | 0x50) && !m.ctrl)
            || (k == 0x5A && m.ctrl)
    }

    /// Ce que dit la ligne d'etat de la note choisie.
    pub fn sel_text(&self) -> Option<String> {
        let n = self.doc.get(self.sel?)?;
        let t = self.take.as_ref()?;
        let bar_qn = self.bar_qn();
        let rel = n.qn0 - t.origin_qn;
        let bar = (rel / bar_qn).floor();
        let beat = rel - bar * bar_qn;
        let names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let nstr = self.detect.tuning().strings.len();
        Some(format!(
            "Bar {} · {:.1} · {}{} · string {}, fret {}{}",
            t.first_bar + bar as i32,
            beat + 1.0,
            names[n.pitch.rem_euclid(12) as usize],
            n.pitch.div_euclid(12) - 1,
            nstr - n.string,
            n.fret,
            match n.tech {
                Tech::Hammer => " · hammer-on",
                Tech::Pull => " · pull-off",
                Tech::Plain => "",
            }
        ))
    }
}
