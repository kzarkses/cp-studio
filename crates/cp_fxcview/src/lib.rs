//! La vue de FX Constellation, sans Windows.
//!
//! Une image se construit en deux temps : [`View::build`] pose tous les
//! elements (leur rectangle, leur controle, leur aspect) ; le dessin, le test
//! de la souris et les infobulles relisent la MEME liste. Un controle est
//! donc toujours la ou il est dessine.
//!
//! La vue ne change rien d'elle-meme au modele : chaque geste devient une
//! [`Act`] que le module applique (et qui revient a l'image suivante).
//!
//! ```text
//! ┌ barre de commande : preset ‹ › 💾 · snapshot ‹ › + · Lock · Single|Granular · Manual|Walk|Figures ┐
//! ├ gauche ─────┬ centre : Parameters | Sound Gen | Seq ─────────────┬ droite : sources ─┤
//! │ pad, pied,  │  pools ROLL / DETACHED, effets, parametres          │ Track|Global|Links│
//! │ navigation, │                                                     │ tuiles, slot      │
//! │ ROLL, macros│  clavier                                            │                   │
//! └ barre d'etat : comptes · Release all links · Stop all engines · PAD ← X Y ─────────────┘
//! ```

mod build;
mod damage;
mod draw;
pub mod panels;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use cp_fxc::links::Plan;
use cp_fxc::model::Track;
use cp_fxc::roll::Settings;
use cp_gfx::Color;
use cp_ui::handle::{DockTabs, HANDLE_DRAG_PX, Press};
use cp_ui::{Button, Ev, Icon, Mods, Theme, vk};

pub use build::{Item, Look, Ring, note_name, slot_type_name};

/// Un rectangle de la vue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> R {
        R { x, y, w, h }
    }
    pub fn has(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn inset(&self, d: f32) -> R {
        R::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Un parametre montre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamRow {
    /// La cle du modele (« {GUID}#indice »).
    pub key: String,
    pub index: usize,
    pub name: String,
    /// La valeur que l'effet joue (normalise), calculee par le module.
    pub live: f64,
    /// Sa valeur telle que l'effet l'affiche.
    pub text: String,
    /// 0 continu, 2 interrupteur, 3..10 crans.
    pub steps: u8,
}

/// Un effet montre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FxRow {
    pub guid: String,
    pub name: String,
    pub enabled: bool,
    pub detached: bool,
    pub params: Vec<ParamRow>,
    /// Les parametres que les mots caches retirent de la liste.
    pub hidden: usize,
}

/// Un slot d'une banque de sources.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub on: bool,
    pub shape: u8,
    pub rate_hz: f64,
    pub rate_n: f64,
    pub sync: u8,
    pub phase: f64,
    pub slew_s: f64,
    pub slew_n: f64,
    pub curve: f64,
    pub mode: u8,
    pub uni: bool,
    pub sh: u8,
    pub len: u8,
    pub steps: [u8; 16],
    /// La sortie (0..1) et la phase du cycle.
    pub out: f64,
    pub pho: f64,
}

/// Une banque (`CP_Mod LFO` de la piste, ou `CP_Mod MIDI` de CP MOD).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bank {
    pub slots: Vec<Slot>,
    /// La bande des reglages etendus (courbe, lissage, mode...).
    pub ext: bool,
}

/// Un JSFX interne montre dans un onglet (Sound Gen, sequenceur).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Js {
    /// Valeurs brutes, normalisees, et affichees, par curseur.
    pub raw: Vec<f64>,
    pub norm: Vec<f64>,
    pub enabled: bool,
}

impl Js {
    pub fn r(&self, p: usize) -> f64 {
        self.raw.get(p).copied().unwrap_or(0.0)
    }
    pub fn n(&self, p: usize) -> f64 {
        self.norm.get(p).copied().unwrap_or(0.0)
    }
    pub fn has(&self, p: usize) -> bool {
        p < self.raw.len()
    }
}

/// Ce que le module donne a chaque image.
pub struct Data<'a> {
    /// La piste montree (aucune : « No track selected. »).
    pub track: Option<&'a str>,
    pub locked: bool,
    pub t: &'a Track,
    pub fx: &'a [FxRow],
    pub plan: &'a Plan,
    /// Le bridge est sur la piste (le pad passe par des liens natifs).
    pub bridge: bool,
    /// Walk ou Figures tourne dans le bridge.
    pub engine_runs: bool,
    pub bank: Option<&'a Bank>,
    pub gbank: Option<&'a Bank>,
    pub sg: Option<&'a Js>,
    pub seq: Option<&'a Js>,
    pub macros: [f64; 8],
    pub preset: &'a str,
    pub modified: bool,
    pub snapshot_label: String,
    pub has_preset: bool,
    /// Le fondu A → B (aucun : pas de capture A et B).
    pub morph: Option<f64>,
    pub roll: &'a Settings,
    pub links: usize,
    pub modulated: usize,
    /// La valeur vive de chaque source (0..1).
    pub src: &'a BTreeMap<i32, f64>,
    pub msg: Option<&'a str>,
    /// La note que le clavier tient.
    pub kb_note: Option<u8>,
    /// Les effets internes de la piste.
    pub guids: Guids,
    /// Les sources armees pour la capture.
    pub capture: usize,
}

/// Les GUID des effets internes de la piste (les reglages modulables).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guids {
    pub sg: Option<String>,
    pub seq: Option<String>,
    pub bank: Option<String>,
    pub bridge: Option<String>,
}

/// Les parametres du sequenceur qu'une source peut moduler : le swing, la
/// transposition, les pas des voies a valeurs et leur bas, la mutation, le
/// de et le haut de chaque voie.
pub fn seq_modulable(p: usize) -> bool {
    if p == 4 || p == 9 {
        return true;
    }
    if !(20..218).contains(&p) {
        return false;
    }
    let l = (p - 20) / 22;
    let k = (p - 20) % 22;
    let bools = panels::LANES[l].bools;
    match k {
        0..=15 | 20 => !bools,
        18 | 19 | 21 => true,
        _ => false,
    }
}

/// Le parametre (cle du modele) qu'un bouton d'un effet interne tient : un
/// bouton du Sound Gen ou du sequenceur, un reglage du slot choisi de la
/// banque locale, une macro. Le PITCH d'un oscillateur module son `pm`.
pub fn internal_key(c: &Ctl, g: &Guids, sel_slot: i32) -> Option<String> {
    use cp_fxc::model::pkey;
    use cp_fxc::slots::{bank, bridge};
    match c {
        Ctl::Js(panels::SG, p) => {
            let p = panels::OSC
                .iter()
                .find(|o| o.freq == *p)
                .map_or(*p, |o| o.pm);
            Some(pkey(g.sg.as_ref()?, p))
        }
        Ctl::Js(panels::SEQ, p) => {
            if !seq_modulable(*p) {
                return None;
            }
            Some(pkey(g.seq.as_ref()?, *p))
        }
        Ctl::SlotKnob(k) => {
            if !(1..=8).contains(&sel_slot) {
                return None;
            }
            let i = sel_slot as usize;
            let p = match k {
                0 => bank::rate(i),
                1 => bank::phase(i),
                2 => bank::curve(i),
                _ => bank::slew(i),
            };
            Some(pkey(g.bank.as_ref()?, p))
        }
        Ctl::Macro(n) => Some(pkey(g.bridge.as_ref()?, bridge::MACRO_IN + *n as usize - 1)),
        _ => None,
    }
}

/// Une source peut-elle moduler ce reglage ? (Jamais ses propres reglages.)
pub fn self_source(c: &Ctl, slot: i32, sel_slot: i32) -> bool {
    match c {
        Ctl::SlotKnob(_) => slot == sel_slot,
        Ctl::Macro(n) => slot == 210 + *n as i32,
        Ctl::Js(panels::SEQ, p) => {
            let l = p.saturating_sub(20) / 22;
            (*p >= 20 && l == 7 && slot == 221) || (*p >= 20 && l == 8 && slot == 222)
        }
        _ => false,
    }
}

/// Les controles. Le module les interprete (les menus aussi).
#[derive(Clone, Debug, PartialEq)]
pub enum Ctl {
    None,
    DockHandle,
    // La barre de commande.
    PresetPrev,
    PresetNext,
    PresetField,
    PresetSave,
    SnapPrev,
    SnapNext,
    SnapField,
    SnapAdd,
    Lock,
    /// La capture : le menu de ce qu'on arme.
    Capture,
    Granular(bool),
    Nav(u8),
    Morph,
    Settings,
    Help,
    // La zone de gauche.
    Pad,
    ZoneClear,
    PadReset,
    /// Un reglage du pad (smooth, vitesse, marche, figure...).
    PadField(PadF),
    WalkJump,
    WalkSync,
    Figure(u8),
    GRange,
    GrainGrid,
    GrainRoll,
    Roll,
    RollAxis(u8),
    Macro(u8),
    MacroDot(u8),
    // Le centre.
    Tab(u8),
    /// La colonne d'un pool (vrai : DETACHED), la ou un en-tete se pose.
    Pool(bool),
    AddFx(bool),
    CollapseAll,
    UntrackAll,
    Values,
    FxHead(String),
    /// Les verbes d'un effet : 0 fenetre, 1 contourner, 2 X/Y, 3 portees,
    /// 4 bases, 5 menu.
    FxVerb(String, u8),
    Sel(String),
    Knob(String),
    /// Les pastilles d'une rangee : 0 X, 1 Y, 2 N.
    Chip(String, u8),
    Src(String),
    // Le rack.
    RackTab(u8),
    Map,
    Fold,
    Tile(i32),
    TileDot(i32),
    AddBank,
    SlotOn,
    SlotType,
    SlotShape,
    SlotPol(bool),
    SlotRetrig,
    SlotRnd,
    SlotSync,
    SlotMode,
    SlotSh,
    /// RATE, PHASE, CURVE, SLEW du slot choisi.
    SlotKnob(u8),
    Steps,
    StepLen,
    StepRnd,
    /// Une ligne de l'onglet Links : (parametre, rang de la source).
    LinkDepth(String, usize),
    LinkRemove(String, usize),
    /// Un lien sur un lien : (parametre, rang de la source, rang du
    /// modulateur de sa profondeur).
    DmodDepth(String, usize, usize),
    DmodRemove(String, usize, usize),
    // La barre d'etat.
    Pill(u8),
    AxisKnob(u8),
    AxisDot(u8),
    StopEngines,
    ReleaseLinks,
    // Les onglets des JSFX : 0 Sound Gen, 1 sequenceur.
    Js(u8, usize),
    JsMenu(u8, usize),
    JsToggle(u8, usize),
    JsSet(u8, usize, i32),
    SgOn,
    SgHold,
    SeqAdd,
    SeqRemove,
    SeqPattern(u8),
    SeqLane(u8),
    SeqLen(u8),
    SeqRail(u8),
    SeqRoll(u8),
    SeqRollAll,
    KbFold,
    Key(u8),
    Seam(u8),
    /// Le champ de recherche des parametres.
    Search,
    /// Les mots qui cachent des parametres : montrer, modifier.
    HideMenu,
    /// La barre de defilement d'une zone.
    ScrollBar(u8),
    /// Une forme de slot choisie d'un clic (0..7, `bank::SHAPES`).
    ShapePick(u8),
}

/// Les reglages du pad qu'un bouton tourne.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadF {
    Smooth,
    MaxSpeed,
    WalkSpeed,
    WalkJitter,
    FigSpeed,
    FigSize,
}

/// Ce que la vue demande au module.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Un clic sur un controle (bouton, pastille, puce).
    Click(Ctl, Mods),
    /// Un menu a ouvrir pour ce controle, a ce point de l'ecran (vue).
    Menu(Ctl, f32, f32),
    /// Une valeur 0..1 (bouton tourne, curseur).
    Value(Ctl, f64),
    /// Une profondeur signee (Alt + glisser sur un bouton module).
    Depth(Ctl, f64),
    /// La fin d'un geste sur ce controle.
    End(Ctl),
    /// Double-clic : la valeur d'origine.
    Reset(Ctl),
    /// Une pastille de source lachee sur un controle.
    Drop(i32, Ctl),
    PadPress(f64, f64),
    PadDrag(f64, f64),
    PadRelease,
    /// Une zone tiree au clic droit (x0, x1, y0, y1).
    Zone([f64; 4]),
    /// L'etendue « Range » du pad (bas, haut).
    Range(f64, f64),
    /// Un pas peint : (voie, pas, valeur 0..1).
    Step(Ctl, u8, f64),
    /// Une note du clavier, enfoncee ou relachee.
    Note(u8, bool),
    /// Tirer l'onglet de la vue (le deplacer de docker).
    DockGrab,
    DockSwitch(usize),
    ViewMenu(f32, f32),
    /// Un en-tete d'effet tire dans l'autre colonne : (GUID, hors de ROLL).
    MoveFx(String, bool),
    /// Rien a faire de plus que redessiner.
    Changed,
}

/// Une touche : a nous, du texte, ou a REAPER.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUse {
    Eat,
    /// Du texte (la recherche) : la fenetre le recoit en WM_CHAR.
    Text,
    NotMine,
}

/// Le geste en cours.
#[derive(Clone, Debug, PartialEq)]
enum Drag {
    /// Un bouton : sa valeur et sa profondeur au depart.
    Knob {
        ctl: Ctl,
        y0: f32,
        v0: f64,
        depth0: f64,
        alt: bool,
        fine: bool,
        moved: bool,
        bipolar: bool,
    },
    /// Un curseur horizontal (le fondu, la largeur).
    Fader {
        ctl: Ctl,
        r: R,
    },
    /// Le bas ou le haut de l'etendue « Range » (0 bas, 1 haut, 2 les deux).
    Range {
        r: R,
        which: u8,
        x0: f32,
        lo0: f64,
        hi0: f64,
    },
    Pad {
        r: R,
    },
    Zone {
        r: R,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
    /// Une pastille de source qu'on tire.
    Source {
        slot: i32,
        x0: f32,
        y0: f32,
        moved: bool,
    },
    Seam {
        which: u8,
        x0: f32,
        w0: f32,
    },
    /// Des pas peints (voie de sequenceur ou pas d'un slot STEP).
    Paint {
        ctl: Ctl,
        r: R,
        n: u8,
        last: Option<(u8, f64)>,
        /// Une voie a interrupteurs pose l'inverse du premier pas touche.
        fill: Option<f64>,
    },
    /// Un clic qui attend son relachement (boutons).
    Press {
        ctl: Ctl,
        mods: Mods,
    },
    Handle {
        x0: f32,
        y0: f32,
    },
    /// Un en-tete d'effet : un clic le replie, un glisser le change de pool.
    FxHead {
        guid: String,
        x0: f32,
        y0: f32,
        moved: bool,
    },
    Key(u8),
    /// Le curseur d'une barre de defilement : (zone, y de depart, defilement
    /// de depart, contenu par pixel de piste).
    Scroll {
        z: u8,
        y0: f32,
        s0: f32,
        k: f32,
    },
}

/// L'etat propre de la vue (rien de ce qu'elle montre du modele).
pub struct View {
    pub dock: DockTabs,
    pub scale: f32,
    dirty: bool,
    /// L'onglet du centre : 0 Parameters, 1 Sound Gen, 2 Seq.
    pub tab: u8,
    /// L'onglet du rack : 0 Track, 1 Global, 2 Links.
    pub rack_tab: u8,
    pub rack_fold: bool,
    /// Le mode Map (a la Bitwig) : tirer un bouton regle la profondeur de la
    /// source choisie. Jamais garde.
    pub map: bool,
    /// La source choisie dans le rack.
    pub sel_slot: i32,
    pub left_w: f32,
    pub right_w: f32,
    /// Les defilements : 0 la liste, 1 le rack, 2 la gauche, 3 Sound Gen,
    /// 4 sequenceur.
    pub scroll: [f32; 5],
    pub show_values: bool,
    pub kb_open: bool,
    /// La recherche des parametres, et si elle a le clavier.
    pub search: cp_ui::TextField,
    pub search_focus: bool,
    /// Les parametres caches par leur nom sont montres.
    pub show_hidden: bool,
    /// Les effets replies (GUID).
    pub folded: std::collections::BTreeSet<String>,
    /// Ce que la souris survole.
    pub hover: Ctl,
    pub mouse: (f32, f32),
    drag: Option<Drag>,
    /// La derniere liste construite (le test de la souris la relit).
    items: Vec<Item>,
    size: (f32, f32),
    /// La source qu'on tire (dessinee par-dessus tout) et son point.
    pub carry: Option<(i32, f32, f32)>,
    pub zone_preview: Option<R>,
    /// La hauteur de contenu de chaque zone qui defile (pour borner).
    content_h: [f32; 5],
    /// Les actions en plus de celle que rend un evenement (les pas sautes
    /// d'une peinture, la fin d'un geste interrompu) : le module les vide.
    pending: Vec<Act>,
    /// L'image d'avant (pour ne repeindre que ce qui change).
    pub(crate) prev: Vec<Item>,
    pub(crate) prev_hover: Ctl,
    drawn_hover: Ctl,
    pub(crate) prev_overlay: bool,
    drawn_overlay: bool,
    pub(crate) prev_dock_hot: Option<usize>,
    drawn_dock_hot: Option<usize>,
    /// La prochaine image est repeinte en entier (taille, theme).
    full: bool,
    pub(crate) full_now: bool,
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

impl View {
    pub fn new() -> View {
        View {
            dock: DockTabs::new("FX Constellation"),
            scale: 1.0,
            dirty: true,
            tab: 0,
            rack_tab: 0,
            rack_fold: false,
            map: false,
            sel_slot: 1,
            left_w: 300.0,
            right_w: 280.0,
            scroll: [0.0; 5],
            show_values: true,
            kb_open: false,
            search: cp_ui::TextField::default(),
            search_focus: false,
            show_hidden: false,
            folded: Default::default(),
            hover: Ctl::None,
            mouse: (-1.0, -1.0),
            drag: None,
            items: Vec::new(),
            size: (0.0, 0.0),
            carry: None,
            zone_preview: None,
            content_h: [0.0; 5],
            pending: Vec::new(),
            prev: Vec::new(),
            prev_hover: Ctl::None,
            drawn_hover: Ctl::None,
            prev_overlay: false,
            drawn_overlay: false,
            prev_dock_hot: None,
            drawn_dock_hot: None,
            full: true,
            full_now: true,
        }
    }

    /// La prochaine image se repeint en entier (la taille, le theme ont
    /// change).
    pub fn mark_full(&mut self) {
        self.full = true;
        self.dirty = true;
    }

    /// Les actions en attente (a jouer apres celle de l'evenement).
    pub fn take_pending(&mut self) -> Vec<Act> {
        std::mem::take(&mut self.pending)
    }

    /// Le geste en cours s'arrete sans relachement (la fenetre a perdu la
    /// souris) : il se termine comme un relachement, rien ne reste coince
    /// (une note tenue, HOLD, le pad).
    pub fn cancel(&mut self) {
        let Some(d) = self.drag.take() else {
            return;
        };
        self.zone_preview = None;
        self.carry = None;
        let a = match d {
            Drag::Knob {
                ctl, moved: true, ..
            } => Some(Act::End(ctl)),
            Drag::Fader { ctl, .. } | Drag::Paint { ctl, .. } => Some(Act::End(ctl)),
            Drag::Range { .. } => Some(Act::End(Ctl::GRange)),
            Drag::Pad { .. } => Some(Act::PadRelease),
            Drag::Key(n) => Some(Act::Note(n, false)),
            Drag::Press {
                ctl: Ctl::SgHold, ..
            } => Some(Act::Value(Ctl::SgHold, 0.0)),
            Drag::Seam { .. } => Some(Act::Changed),
            _ => None,
        };
        self.pending.extend(a);
        self.mark();
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

    /// L'en-tete d'effet qu'on tire (son GUID), pour le dessiner.
    pub fn moving_fx(&self) -> Option<&str> {
        match &self.drag {
            Some(Drag::FxHead {
                guid, moved: true, ..
            }) => Some(guid),
            _ => None,
        }
    }

    /// Une pastille de source est tiree (le module garde les images).
    pub fn carrying(&self) -> bool {
        self.carry.is_some()
    }

    /// Le geste en cours touche-t-il ce controle ?
    pub fn gesture_on(&self, c: &Ctl) -> bool {
        match &self.drag {
            Some(Drag::Knob { ctl, .. }) | Some(Drag::Fader { ctl, .. }) => ctl == c,
            Some(Drag::Pad { .. }) => *c == Ctl::Pad,
            _ => false,
        }
    }

    /// Construit les elements de l'image (et les garde pour la souris).
    pub fn layout(&mut self, d: &Data, th: &Theme, w: f32, h: f32) {
        if (th.scale - self.scale).abs() > 1e-3 || (w, h) != self.size {
            self.full = true;
        }
        self.scale = th.scale;
        self.size = (w, h);
        let (items, content) = build::build(self, d, th, w, h);
        self.prev = std::mem::replace(&mut self.items, items);
        self.content_h = content;
        for z in 0..5 {
            self.clamp_scroll(z);
        }
        self.prev_hover = std::mem::replace(&mut self.drawn_hover, self.hover.clone());
        let ov = self.overlay();
        self.prev_overlay = std::mem::replace(&mut self.drawn_overlay, ov);
        self.prev_dock_hot = std::mem::replace(&mut self.drawn_dock_hot, self.dock.hot);
        self.full_now = std::mem::take(&mut self.full);
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    fn clamp_scroll(&mut self, z: usize) {
        let view_h = self.zone_h(z);
        let max = (self.content_h[z] - view_h).max(0.0);
        self.scroll[z] = self.scroll[z].clamp(0.0, max);
    }

    fn zone_h(&self, z: usize) -> f32 {
        self.items
            .iter()
            .find(|i| matches!(i.look, Look::Clip(k) if k as usize == z))
            .map_or(0.0, |i| i.r.h)
    }

    /// Le controle sous le point (le dernier pose gagne).
    pub fn hit(&self, x: f32, y: f32) -> Ctl {
        if self
            .dock
            .at(self.size.0, self.size.1, self.scale, x, y)
            .is_some()
        {
            return Ctl::DockHandle;
        }
        self.item_at(x, y).map_or(Ctl::None, |i| i.ctl.clone())
    }

    fn item_at(&self, x: f32, y: f32) -> Option<&Item> {
        // Une zone qui defile coupe ce qui depasse : un element hors de sa
        // zone visible ne se touche pas.
        let clips: Vec<(u8, R)> = self
            .items
            .iter()
            .filter_map(|i| match i.look {
                Look::Clip(z) => Some((z, i.r)),
                _ => None,
            })
            .collect();
        self.items.iter().rev().find(|i| {
            i.ctl != Ctl::None
                && i.r.has(x, y)
                && i.clip
                    .is_none_or(|z| clips.iter().any(|(k, r)| *k == z && r.has(x, y)))
        })
    }

    /// La zone qui defile sous le point.
    fn scroll_zone_at(&self, x: f32, y: f32) -> Option<u8> {
        self.items.iter().rev().find_map(|i| match i.look {
            Look::Clip(z) if i.r.has(x, y) => Some(z),
            _ => None,
        })
    }

    /// Le texte d'aide d'un controle (les infobulles).
    pub fn label(&self, c: &Ctl) -> String {
        self.items
            .iter()
            .rev()
            .find(|i| &i.ctl == c)
            .map(|i| i.tip.clone())
            .unwrap_or_default()
    }

    pub fn key_use(&self, vk_code: u32, m: Mods) -> KeyUse {
        if self.search_focus {
            return match vk_code {
                vk::ESCAPE | vk::RETURN | vk::TAB => KeyUse::Eat,
                _ if m.ctrl || m.alt || m.win => {
                    if matches!(vk_code, vk::LEFT | vk::RIGHT | vk::BACK | vk::DELETE | 0x41) {
                        KeyUse::Eat
                    } else {
                        KeyUse::NotMine
                    }
                }
                vk::LEFT | vk::RIGHT | vk::HOME | vk::END | vk::BACK | vk::DELETE => KeyUse::Eat,
                _ => KeyUse::Text,
            };
        }
        if vk_code == vk::ESCAPE && (self.map || self.carry.is_some()) {
            KeyUse::Eat
        } else {
            KeyUse::NotMine
        }
    }

    /// Un caractere tape : la recherche, si elle a le clavier.
    pub fn char(&mut self, c: char) -> Option<Act> {
        if !self.search_focus || c.is_control() {
            return None;
        }
        if self.search.char(c) == cp_ui::textfield::Edit::Changed {
            self.scroll[0] = 0.0;
        }
        self.mark();
        Some(Act::Changed)
    }

    /// Echap : la recherche, la pastille tiree, puis le mode Map.
    pub fn key(&mut self, vk_code: u32, m: Mods) -> Option<Act> {
        if self.search_focus {
            match vk_code {
                vk::ESCAPE => {
                    if self.search.text.is_empty() {
                        self.search_focus = false;
                    }
                    self.search.clear();
                }
                vk::RETURN | vk::TAB => self.search_focus = false,
                k => {
                    if self.search.key(k, m) == cp_ui::textfield::Edit::Changed {
                        self.scroll[0] = 0.0;
                    }
                }
            }
            self.mark();
            return Some(Act::Changed);
        }
        if vk_code != vk::ESCAPE {
            return None;
        }
        if self.carry.take().is_some() {
            self.drag = None;
        } else if self.map {
            self.map = false;
        } else {
            return None;
        }
        self.mark();
        Some(Act::Changed)
    }

    fn look_of(&self, c: &Ctl) -> Option<&Look> {
        self.items
            .iter()
            .rev()
            .find(|i| &i.ctl == c)
            .map(|i| &i.look)
    }

    /// Un evenement de la souris ou du clavier.
    pub fn event(&mut self, ev: &Ev) -> Option<Act> {
        let (w, h) = self.size;
        match *ev {
            Ev::Move { x, y, mods } => {
                self.mouse = (x, y);
                if self.dock.track(w, h, self.scale, x, y) {
                    self.mark();
                }
                if self.drag.is_some() {
                    return self.drag_move(x, y, mods);
                }
                let c = self.hit(x, y);
                if c != self.hover {
                    self.hover = c;
                    self.mark();
                }
                None
            }
            Ev::Leave => {
                self.dock.hot = None;
                self.mouse = (-1.0, -1.0);
                if self.drag.is_none() && self.hover != Ctl::None {
                    self.hover = Ctl::None;
                    self.mark();
                }
                None
            }
            Ev::Down { x, y, b, dbl, mods } => {
                self.mouse = (x, y);
                self.down(x, y, b, dbl, mods)
            }
            Ev::Up { x, y, b, mods } => self.up(x, y, b, mods),
            Ev::Wheel { x, y, dy, mods, .. } => self.wheel(x, y, dy, mods),
            Ev::Key { vk, mods, .. } => self.key(vk, mods),
            Ev::Char(c) => self.char(c),
        }
    }

    fn down(&mut self, x: f32, y: f32, b: Button, dbl: bool, mods: Mods) -> Option<Act> {
        let (w, h) = self.size;
        if self.drag.is_some() && self.carry.is_none() {
            if b != Button::Left {
                return None;
            }
            self.cancel();
        }
        let c = self.hit(x, y);
        if self.search_focus && c != Ctl::Search {
            self.search_focus = false;
            self.mark();
        }
        if c == Ctl::DockHandle {
            if b == Button::Right {
                return Some(Act::ViewMenu(x, y));
            }
            if let Some(Press::Switch(i)) = self.dock.press(w, h, self.scale, x, y) {
                return Some(Act::DockSwitch(i));
            }
            self.drag = Some(Drag::Handle { x0: x, y0: y });
            return None;
        }
        // Une pastille qu'on porte (clic, deplacement, clic) se pose ici.
        if let Some((slot, _, _)) = self.carry {
            if b == Button::Left {
                self.carry = None;
                self.drag = None;
                self.mark();
                return (c != Ctl::None).then_some(Act::Drop(slot, c));
            }
        }
        if b == Button::Right {
            return match c {
                Ctl::Pad => {
                    let r = self.rect_of(&Ctl::Pad)?;
                    self.drag = Some(Drag::Zone {
                        r,
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    });
                    None
                }
                Ctl::None => None,
                c => Some(Act::Menu(c, x, y)),
            };
        }
        if b != Button::Left {
            return None;
        }
        let look = self.look_of(&c).cloned();
        match (&c, look) {
            (Ctl::None, _) => None,
            (Ctl::Seam(k), _) => {
                if dbl {
                    if *k == 0 {
                        self.left_w = 300.0;
                    } else {
                        self.right_w = 280.0;
                    }
                    self.mark();
                    return Some(Act::Changed);
                }
                let w0 = if *k == 0 { self.left_w } else { self.right_w };
                self.drag = Some(Drag::Seam {
                    which: *k,
                    x0: x,
                    w0,
                });
                None
            }
            (Ctl::Search, _) => {
                self.search_focus = true;
                if dbl {
                    self.search.select_all();
                } else {
                    let t = self.search.text.clone();
                    self.search.set(&t);
                }
                self.mark();
                Some(Act::Changed)
            }
            (Ctl::ScrollBar(z), Some(Look::Scroll { pos, len })) => {
                let r = self.rect_of(&c)?;
                let z = *z;
                let view_h = self.zone_h(z as usize).max(1.0);
                let content = self.content_h[z as usize];
                let room = (r.h - len).max(1.0);
                let k = (content - view_h).max(0.0) / room;
                // Hors du curseur : il saute la ou l'on clique.
                if y < r.y + pos || y > r.y + pos + len {
                    let t = ((y - r.y - len / 2.0) / room).clamp(0.0, 1.0);
                    self.scroll[z as usize] = t * (content - view_h).max(0.0);
                    self.clamp_scroll(z as usize);
                }
                self.drag = Some(Drag::Scroll {
                    z,
                    y0: y,
                    s0: self.scroll[z as usize],
                    k,
                });
                self.mark();
                Some(Act::Changed)
            }
            (Ctl::Pad, _) => {
                let r = self.rect_of(&Ctl::Pad)?;
                self.drag = Some(Drag::Pad { r });
                let (px, py) = pad_point(r, x, y);
                Some(Act::PadPress(px, py))
            }
            (c @ (Ctl::TileDot(_) | Ctl::MacroDot(_) | Ctl::AxisDot(_)), _) => {
                let slot = match c {
                    Ctl::TileDot(s) => *s,
                    Ctl::MacroDot(n) => 210 + *n as i32,
                    Ctl::AxisDot(0) => -1,
                    _ => -2,
                };
                self.drag = Some(Drag::Source {
                    slot,
                    x0: x,
                    y0: y,
                    moved: false,
                });
                None
            }
            (
                _,
                Some(Look::Knob {
                    v,
                    depth,
                    bipolar,
                    enabled,
                    ..
                }),
            ) => {
                if !enabled {
                    return None;
                }
                if dbl && !mods.alt && !mods.shift {
                    return Some(Act::Reset(c));
                }
                self.drag = Some(Drag::Knob {
                    ctl: c,
                    y0: y,
                    v0: v,
                    depth0: depth,
                    alt: mods.alt,
                    fine: mods.shift,
                    moved: false,
                    bipolar,
                });
                None
            }
            (_, Some(Look::Fader { enabled: true, .. })) => {
                let r = self.rect_of(&c)?;
                if dbl {
                    return Some(Act::Reset(c));
                }
                self.drag = Some(Drag::Fader { ctl: c.clone(), r });
                Some(Act::Value(c, fader_value(r, x)))
            }
            (Ctl::GRange, Some(Look::Range { lo, hi })) => {
                let r = self.rect_of(&c)?;
                let v = fader_value(r, x);
                let which = if (v - lo).abs() < 0.04 {
                    0
                } else if (v - hi).abs() < 0.04 {
                    1
                } else if v > lo && v < hi {
                    2
                } else if v < lo {
                    0
                } else {
                    1
                };
                self.drag = Some(Drag::Range {
                    r,
                    which,
                    x0: x,
                    lo0: lo,
                    hi0: hi,
                });
                None
            }
            (Ctl::Steps | Ctl::SeqLane(_), Some(Look::Steps { n, vals, bools, .. })) => {
                let r = self.rect_of(&c)?;
                let fill = bools.then(|| {
                    let k = step_at(r, n, x) as usize;
                    if vals.get(k).copied().unwrap_or(0.0) > 0.5 {
                        0.0
                    } else {
                        1.0
                    }
                });
                self.drag = Some(Drag::Paint {
                    ctl: c.clone(),
                    r,
                    n,
                    last: None,
                    fill,
                });
                self.paint(x, y)
            }
            (Ctl::Key(n), _) => {
                self.drag = Some(Drag::Key(*n));
                Some(Act::Note(*n, true))
            }
            (Ctl::SgHold, _) => {
                self.drag = Some(Drag::Press {
                    ctl: c.clone(),
                    mods,
                });
                Some(Act::Value(c, 1.0))
            }
            (Ctl::FxHead(g), _) => {
                self.drag = Some(Drag::FxHead {
                    guid: g.clone(),
                    x0: x,
                    y0: y,
                    moved: false,
                });
                None
            }
            _ => {
                self.drag = Some(Drag::Press { ctl: c, mods });
                None
            }
        }
    }

    fn rect_of(&self, c: &Ctl) -> Option<R> {
        self.items.iter().rev().find(|i| &i.ctl == c).map(|i| i.r)
    }

    fn paint(&mut self, x: f32, y: f32) -> Option<Act> {
        let Some(Drag::Paint {
            ctl,
            r,
            n,
            last,
            fill,
        }) = self.drag.clone()
        else {
            return None;
        };
        let k = step_at(r, n, x);
        let v = fill.unwrap_or((1.0 - ((y - r.y) / r.h.max(1.0)).clamp(0.0, 1.0)) as f64);
        if last == Some((k, v)) {
            return None;
        }
        // Une souris rapide saute des pas : ceux d'entre deux prennent la
        // valeur de la droite qui relie les deux points.
        if let Some((k0, v0)) = last {
            let (a, b) = (k0.min(k), k0.max(k));
            for j in a + 1..b {
                let t = (j as f64 - k0 as f64) / (k as f64 - k0 as f64);
                self.pending
                    .push(Act::Step(ctl.clone(), j, v0 + (v - v0) * t));
            }
        }
        self.drag = Some(Drag::Paint {
            ctl: ctl.clone(),
            r,
            n,
            last: Some((k, v)),
            fill,
        });
        Some(Act::Step(ctl, k, v))
    }

    fn drag_move(&mut self, x: f32, y: f32, mods: Mods) -> Option<Act> {
        let d = self.drag.clone()?;
        match d {
            Drag::Knob {
                ctl,
                y0,
                v0,
                depth0,
                alt,
                fine,
                moved,
                bipolar,
            } => {
                let dy = y0 - y;
                if !moved && dy.abs() < 2.0 {
                    return None;
                }
                let k = if mods.shift || fine { 0.0004 } else { 0.004 } / self.scale.max(0.5);
                self.drag = Some(Drag::Knob {
                    ctl: ctl.clone(),
                    y0,
                    v0,
                    depth0,
                    alt,
                    fine,
                    moved: true,
                    bipolar,
                });
                self.mark();
                if alt || self.map {
                    let lo = -1.0;
                    Some(Act::Depth(
                        ctl,
                        (depth0 + dy as f64 * k as f64).clamp(lo, 1.0),
                    ))
                } else {
                    Some(Act::Value(ctl, (v0 + dy as f64 * k as f64).clamp(0.0, 1.0)))
                }
            }
            Drag::Fader { ctl, r } => Some(Act::Value(ctl, fader_value(r, x))),
            Drag::Range {
                r,
                which,
                x0,
                lo0,
                hi0,
            } => {
                let v = fader_value(r, x);
                let (lo, hi) = match which {
                    0 => (v.min(hi0), hi0),
                    1 => (lo0, v.max(lo0)),
                    _ => {
                        let dv = ((x - x0) / r.w.max(1.0)) as f64;
                        let span = hi0 - lo0;
                        let lo = (lo0 + dv).clamp(0.0, 1.0 - span);
                        (lo, lo + span)
                    }
                };
                Some(Act::Range(lo, hi))
            }
            Drag::Pad { r } => {
                let (px, py) = pad_point(r, x, y);
                Some(Act::PadDrag(px, py))
            }
            Drag::Zone { r, x0, y0, .. } => {
                self.drag = Some(Drag::Zone {
                    r,
                    x0,
                    y0,
                    x1: x,
                    y1: y,
                });
                let (a, b) = (x0.min(x).max(r.x), x0.max(x).min(r.right()));
                let (c, e) = (y0.min(y).max(r.y), y0.max(y).min(r.bottom()));
                self.zone_preview = Some(R::new(a, c, b - a, e - c));
                self.mark();
                Some(Act::Changed)
            }
            Drag::Source {
                slot,
                x0,
                y0,
                moved,
            } => {
                if !moved && (x - x0).abs() + (y - y0).abs() < 4.0 {
                    return None;
                }
                self.drag = Some(Drag::Source {
                    slot,
                    x0,
                    y0,
                    moved: true,
                });
                self.carry = Some((slot, x, y));
                self.hover = self.hit(x, y);
                self.mark();
                Some(Act::Changed)
            }
            Drag::Seam { which, x0, w0 } => {
                let dx = x - x0;
                let min = 160.0 * self.scale;
                let max = (self.size.0 * 0.45).max(min);
                let s = self.scale.max(0.1);
                if which == 0 {
                    self.left_w = (w0 + dx / s).clamp(min / s, max / s);
                } else {
                    self.right_w = (w0 - dx / s).clamp(min / s, max / s);
                }
                self.mark();
                Some(Act::Changed)
            }
            Drag::Paint { .. } => self.paint(x, y),
            Drag::Handle { x0, y0 } => {
                if (x - x0).abs() + (y - y0).abs() > HANDLE_DRAG_PX * self.scale {
                    self.drag = None;
                    return Some(Act::DockGrab);
                }
                None
            }
            Drag::Key(n) => {
                if let Ctl::Key(m) = self.hit(x, y) {
                    if m != n {
                        self.drag = Some(Drag::Key(m));
                        return Some(Act::Note(m, true));
                    }
                }
                None
            }
            Drag::FxHead {
                guid,
                x0,
                y0,
                moved,
            } => {
                if !moved && (x - x0).abs() + (y - y0).abs() < 5.0 * self.scale {
                    return None;
                }
                self.drag = Some(Drag::FxHead {
                    guid,
                    x0,
                    y0,
                    moved: true,
                });
                self.mark();
                Some(Act::Changed)
            }
            Drag::Press { .. } => None,
            Drag::Scroll { z, y0, s0, k } => {
                self.scroll[z as usize] = s0 + (y - y0) * k;
                self.clamp_scroll(z as usize);
                self.mark();
                Some(Act::Changed)
            }
        }
    }

    fn up(&mut self, x: f32, y: f32, b: Button, mods: Mods) -> Option<Act> {
        let d = self.drag.take();
        self.zone_preview = None;
        match d {
            Some(Drag::Knob {
                ctl, moved, alt, ..
            }) => {
                if moved {
                    Some(Act::End(ctl))
                } else if alt || mods.alt || mods.shift {
                    Some(Act::Click(ctl, mods))
                } else {
                    None
                }
            }
            Some(Drag::Fader { ctl, .. }) => Some(Act::End(ctl)),
            Some(Drag::Range { .. }) => Some(Act::End(Ctl::GRange)),
            Some(Drag::Pad { .. }) => Some(Act::PadRelease),
            Some(Drag::Zone { r, x0, y0, .. }) if b == Button::Right => {
                let f = |v: f32, a: f32, l: f32| ((v - a) / l.max(1.0)).clamp(0.0, 1.0) as f64;
                let (xa, xb) = (f(x0.min(x), r.x, r.w), f(x0.max(x), r.x, r.w));
                // y vers le haut.
                let (ya, yb) = (1.0 - f(y0.max(y), r.y, r.h), 1.0 - f(y0.min(y), r.y, r.h));
                self.mark();
                Some(Act::Zone([xa, xb, ya, yb]))
            }
            Some(Drag::Source { slot, moved, .. }) => {
                if !moved {
                    // Un clic sur une pastille : la source se choisit.
                    self.carry = None;
                    return Some(Act::Click(
                        match slot {
                            -1 => Ctl::AxisDot(0),
                            -2 => Ctl::AxisDot(1),
                            s if s > 210 && s < 219 => Ctl::MacroDot((s - 210) as u8),
                            s => Ctl::Tile(s),
                        },
                        mods,
                    ));
                }
                self.carry = None;
                self.mark();
                let c = self.hit(x, y);
                (c != Ctl::None).then_some(Act::Drop(slot, c))
            }
            Some(Drag::Seam { .. }) => Some(Act::Changed),
            Some(Drag::FxHead { guid, moved, .. }) => {
                if !moved {
                    return Some(Act::Click(Ctl::FxHead(guid), mods));
                }
                self.mark();
                // La colonne sous la souris.
                self.items
                    .iter()
                    .rev()
                    .find_map(|i| match i.ctl {
                        Ctl::Pool(d) if i.r.has(x, y) => Some(d),
                        _ => None,
                    })
                    .map(|d| Act::MoveFx(guid, d))
            }
            Some(Drag::Paint { ctl, .. }) => Some(Act::End(ctl)),
            Some(Drag::Key(n)) => Some(Act::Note(n, false)),
            Some(Drag::Press { ctl, mods }) => {
                if ctl == Ctl::SgHold {
                    return Some(Act::Value(Ctl::SgHold, 0.0));
                }
                (self.hit(x, y) == ctl).then_some(Act::Click(ctl, mods))
            }
            _ => None,
        }
    }

    fn wheel(&mut self, x: f32, y: f32, dy: f32, mods: Mods) -> Option<Act> {
        let c = self.hit(x, y);
        if let Some(Look::Knob {
            v,
            depth,
            enabled: true,
            ..
        }) = self.look_of(&c).cloned()
        {
            let k: f64 = if mods.ctrl { 0.005 } else { 0.02 };
            if mods.alt || self.map {
                return Some(Act::Depth(c, (depth + dy as f64 * k).clamp(-1.0, 1.0)));
            }
            return Some(Act::Value(c, (v + dy as f64 * k).clamp(0.0, 1.0)));
        }
        let z = self.scroll_zone_at(x, y)? as usize;
        self.scroll[z] -= dy * 48.0 * self.scale;
        self.clamp_scroll(z);
        self.mark();
        Some(Act::Changed)
    }
}

/// Le pas sous l'abscisse.
fn step_at(r: R, n: u8, x: f32) -> u8 {
    let cw = r.w / n.max(1) as f32;
    (((x - r.x) / cw.max(0.1)).floor() as i32).clamp(0, n.max(1) as i32 - 1) as u8
}

/// Une longueur de 1 a 16 pas sur un curseur : chaque longueur a la meme
/// largeur.
pub fn len_of(v: f64) -> f64 {
    ((v * 16.0).floor() + 1.0).clamp(1.0, 16.0)
}

/// Le point du pad (y vers le haut).
fn pad_point(r: R, x: f32, y: f32) -> (f64, f64) {
    (
        ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) as f64,
        (1.0 - (y - r.y) / r.h.max(1.0)).clamp(0.0, 1.0) as f64,
    )
}

fn fader_value(r: R, x: f32) -> f64 {
    ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) as f64
}

/// Une couleur 8 bits en couleur de dessin.
pub fn col(c: (u8, u8, u8)) -> Color {
    cp_gfx::rgb(c.0, c.1, c.2)
}

/// L'icone d'un verbe d'effet.
pub fn verb_icon(k: u8) -> Icon {
    match k {
        0 => Icon::Maximize,
        1 => Icon::Power,
        2 => Icon::Move,
        3 => Icon::Ruler,
        4 => Icon::Gauge,
        _ => Icon::Ellipsis,
    }
}
