//! Les elements d'une image : ou est chaque controle, et de quoi il a l'air.
//!
//! La grammaire (2026-11-25, la maquette validee par Cedric) : des cartes
//! posees sur le fond, une marge et une bordure fine ; des icones plutot que
//! des mots, l'explication dans l'infobulle ; des choix segmentes ; le pad
//! sur une grille fixe.

use cp_fxc::links::mod_window;
use cp_fxc::model::{FIGURES, Nav, PState, WALK_SYNCS};
use cp_fxc::roll::AXES;
use cp_fxc::slots::{Band, band_of, bank, color, long_label, short_label, source_center};
use cp_gfx::text::Align;
use cp_gfx::{Color, rgb};
use cp_ui::{Icon, Theme};

use crate::{Ctl, Data, PadF, R, View, col};

/// Un arc de modulation sur un bouton : de `lo` a `hi` (0..1, croises pour
/// une profondeur negative).
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub lo: f64,
    pub hi: f64,
    pub color: Color,
    /// Un arc fin (une source parmi plusieurs).
    pub thin: bool,
    pub dim: bool,
}

/// Les polices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F {
    Normal,
    Bold,
    Small,
    Mono,
}

/// Les fonds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shade {
    /// Les barres du haut et du bas.
    Bar,
    /// Le fond de la fenetre.
    Zone,
    /// Une carte posee sur le fond : coins ronds, bordure fine.
    Card,
    /// Un creux (un champ, un affichage).
    Sunk,
    /// L'en-tete d'une carte d'effet.
    Head,
    /// Une case dans une carte (un parametre).
    Well,
    /// La ou l'on depose un en-tete d'effet (tirets).
    Drop,
    /// Une teinte donnee (une case suivie, menee par la source survolee).
    Tint(Color),
}

/// Le ton d'un bouton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Accent,
    Amber,
    Danger,
}

/// L'aspect d'un element.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    Panel(Shade),
    /// Une zone qui defile (son rectangle visible ; rien a dessiner).
    Clip(u8),
    Text {
        s: String,
        font: F,
        color: Color,
        align: Align,
    },
    Button {
        label: String,
        icon: Option<Icon>,
        on: bool,
        enabled: bool,
        tone: Tone,
    },
    /// Une part d'un choix segmente.
    Seg {
        label: String,
        icon: Option<Icon>,
        sel: bool,
        first: bool,
        last: bool,
    },
    /// Le champ d'un preset ou d'un snapshot.
    Field {
        text: String,
        modified: bool,
        muted: bool,
    },
    Knob {
        /// La valeur que le geste change (la base), 0..1.
        v: f64,
        /// La profondeur que Alt + glisser change (signee).
        depth: f64,
        bipolar: bool,
        enabled: bool,
        rings: Vec<Ring>,
        /// La valeur jouee (le point sur l'anneau).
        live: Option<f64>,
        label: String,
        value: String,
        /// Pulse dans cette couleur (la source survolee la mene).
        hl: Option<Color>,
        size: f32,
        /// Un parametre que FX Constellation ne suit pas : estompe.
        dim: bool,
    },
    Fader {
        v: f64,
        label: String,
        enabled: bool,
    },
    Range {
        lo: f64,
        hi: f64,
    },
    Pad(PadLook),
    /// Une pastille de source.
    Dot {
        color: Color,
        hollow: bool,
    },
    Tile {
        title: String,
        color: Color,
        on: bool,
        count: usize,
        shape: Option<(u8, f64)>,
        phase: Option<f64>,
        sel: bool,
        hl: bool,
    },
    Steps {
        n: u8,
        vals: Vec<f64>,
        len: u8,
        play: Option<u8>,
        origin: f64,
        color: Color,
        bools: bool,
    },
    Chip {
        label: String,
        on: bool,
        standby: bool,
        amber: bool,
        enabled: bool,
    },
    /// Une courbe (forme d'onde, enveloppe) : des points 0..1.
    Curve {
        pts: Vec<(f32, f32)>,
        color: Color,
        level: Option<f32>,
    },
    Key {
        black: bool,
        lit: bool,
    },
    Sep,
    /// La pastille X / Y / N d'une rangee sans texte (une source : son nom).
    SrcLabel {
        dots: Vec<(Color, bool)>,
        label: String,
    },
    /// Un interrupteur a icone (une forme, un axe de ROLL).
    IconChip {
        icon: Icon,
        on: bool,
        enabled: bool,
        /// La couleur allumee (sinon l'accent).
        color: Option<Color>,
    },
    /// Un axe du pad : sa valeur, ce qui le mene.
    Pill {
        color: Color,
        label: String,
        driven: bool,
    },
    /// Le curseur d'une barre de defilement (debut, longueur dans la piste).
    Scroll {
        pos: f32,
        len: f32,
    },
    /// Le champ de recherche (le texte vit dans la vue).
    Search {
        focus: bool,
    },
    /// Un petit compte dans une pastille.
    Badge {
        text: String,
        color: Color,
    },
    /// Une icone posee (decor).
    Glyph {
        icon: Icon,
        color: Color,
    },
    /// La forme d'un LFO en grand, et ou il en est.
    Wave {
        pts: Vec<(f32, f32)>,
        color: Color,
        phase: Option<(f32, f32)>,
    },
}

/// Ce que le pad dessine.
#[derive(Clone, Debug, PartialEq)]
pub struct PadLook {
    pub x: f64,
    pub y: f64,
    pub ax: f64,
    pub ay: f64,
    pub zone: Option<[f64; 4]>,
    pub grains: Vec<(f64, f64)>,
    pub grid: u8,
    pub drive_x: Option<Color>,
    pub drive_y: Option<Color>,
    pub figure: Option<Vec<(f64, f64)>>,
    /// Le pad bouge seul (marche, figure) : son sillage se dessine.
    pub runs: bool,
    /// Les dernieres positions (de la plus ancienne a la plus recente).
    pub trail: Vec<(f64, f64)>,
}

/// Un element de l'image.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub r: R,
    pub ctl: Ctl,
    pub look: Look,
    pub tip: String,
    /// La zone qui defile qui le coupe.
    pub clip: Option<u8>,
}

/// Le constructeur : pose les elements, une zone a la fois.
pub(crate) struct B<'a> {
    pub items: Vec<Item>,
    pub th: &'a Theme,
    pub clip: Option<u8>,
    /// Le decalage vertical de la zone qui defile.
    pub dy: f32,
}

impl B<'_> {
    pub fn s(&self, v: f32) -> f32 {
        self.th.s(v)
    }
    pub fn push(&mut self, r: R, ctl: Ctl, look: Look, tip: &str) {
        let r = R::new(r.x, r.y - self.dy, r.w, r.h);
        self.items.push(Item {
            r,
            ctl,
            look,
            tip: tip.to_string(),
            clip: self.clip,
        });
    }
    pub fn panel(&mut self, r: R, sh: Shade) {
        self.push(r, Ctl::None, Look::Panel(sh), "");
    }
    /// Une carte dont la hauteur se saura apres : son indice, pour
    /// [`B::close`].
    pub fn open_card(&mut self, x: f32, y: f32, w: f32) -> usize {
        self.panel(R::new(x, y, w, 0.0), Shade::Card);
        self.items.len() - 1
    }
    /// La carte ouverte s'arrete a `bottom`.
    pub fn close(&mut self, i: usize, bottom: f32) {
        if let Some(it) = self.items.get_mut(i) {
            it.r.h = (bottom - self.dy - it.r.y).max(0.0);
        }
    }
    pub fn text(&mut self, r: R, s: &str, font: F, color: Color, align: Align) {
        self.push(
            r,
            Ctl::None,
            Look::Text {
                s: s.to_string(),
                font,
                color,
                align,
            },
            "",
        );
    }
    pub fn button(&mut self, r: R, ctl: Ctl, label: &str, icon: Option<Icon>, on: bool, tip: &str) {
        self.push(
            r,
            ctl,
            Look::Button {
                label: label.to_string(),
                icon,
                on,
                enabled: true,
                tone: Tone::Normal,
            },
            tip,
        );
    }
    /// Un bouton d'icone seule.
    pub fn icon_btn(&mut self, r: R, ctl: Ctl, icon: Icon, on: bool, tip: &str) {
        self.button(r, ctl, "", Some(icon), on, tip);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn button_ex(
        &mut self,
        r: R,
        ctl: Ctl,
        label: &str,
        icon: Option<Icon>,
        on: bool,
        enabled: bool,
        tone: Tone,
        tip: &str,
    ) {
        self.push(
            r,
            ctl,
            Look::Button {
                label: label.to_string(),
                icon,
                on,
                enabled,
                tone,
            },
            tip,
        );
    }
    pub fn chip(&mut self, r: R, ctl: Ctl, label: &str, on: bool, enabled: bool, tip: &str) {
        self.push(
            r,
            ctl,
            Look::Chip {
                label: label.to_string(),
                on,
                standby: false,
                amber: false,
                enabled,
            },
            tip,
        );
    }
    pub fn icon_chip(&mut self, r: R, ctl: Ctl, icon: Icon, on: bool, tip: &str) {
        self.push(
            r,
            ctl,
            Look::IconChip {
                icon,
                on,
                enabled: true,
                color: None,
            },
            tip,
        );
    }
    pub fn glyph(&mut self, r: R, icon: Icon, color: Color) {
        self.push(r, Ctl::None, Look::Glyph { icon, color }, "");
    }
    /// Un choix segmente : une part par etiquette.
    pub fn seg(
        &mut self,
        r: R,
        labels: &[&str],
        sel: usize,
        ctl: impl Fn(usize) -> Ctl,
        tips: &[&str],
    ) {
        let parts: Vec<(Option<Icon>, &str)> = labels.iter().map(|l| (None, *l)).collect();
        self.seg_ic(r, &parts, sel, ctl, tips);
    }
    /// Un choix segmente a icones (et etiquettes, vides si l'icone suffit).
    pub fn seg_ic(
        &mut self,
        r: R,
        parts: &[(Option<Icon>, &str)],
        sel: usize,
        ctl: impl Fn(usize) -> Ctl,
        tips: &[&str],
    ) {
        let n = parts.len().max(1) as f32;
        let pw = (r.w / n).floor();
        for (i, (ic, l)) in parts.iter().enumerate() {
            let x = r.x + pw * i as f32;
            let w = if i + 1 == parts.len() {
                r.right() - x
            } else {
                pw
            };
            self.push(
                R::new(x, r.y, w, r.h),
                ctl(i),
                Look::Seg {
                    label: l.to_string(),
                    icon: *ic,
                    sel: i == sel,
                    first: i == 0,
                    last: i + 1 == parts.len(),
                },
                tips.get(i).copied().unwrap_or(""),
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn knob(
        &mut self,
        r: R,
        ctl: Ctl,
        v: f64,
        label: &str,
        value: &str,
        enabled: bool,
        bipolar: bool,
        tip: &str,
    ) {
        self.push(
            r,
            ctl,
            Look::Knob {
                v,
                depth: 0.0,
                bipolar,
                enabled,
                rings: Vec::new(),
                live: None,
                label: label.to_string(),
                value: value.to_string(),
                hl: None,
                size: r.w.min(r.h),
                dim: false,
            },
            tip,
        );
    }
    /// Une etiquette de section (petites capitales estompees).
    pub fn label(&mut self, r: R, s: &str) {
        let c = self.th.text_dim;
        self.text(r, s, F::Small, c, Align::Left);
    }
}

/// Une largeur de texte estimee (les mises en page n'ont pas les polices).
pub(crate) fn tw(th: &Theme, s: &str) -> f32 {
    (s.chars().count() as f32 * 6.6 + 14.0) * th.scale
}

/// Les couleurs de la vue, d'apres le theme.
pub(crate) fn amber() -> Color {
    rgb(0xd9, 0x99, 0x4a)
}

fn accent8(th: &Theme) -> (u8, u8, u8) {
    let c = th.accent;
    (c.r, c.g, c.b)
}

/// La couleur d'une source dans ce theme.
pub(crate) fn src_color(th: &Theme, slot: i32) -> Color {
    col(color(slot, accent8(th)))
}

/// La marge des cartes, l'ecart entre elles.
fn pad_of(b: &B) -> f32 {
    b.s(10.0)
}
fn gap_of(b: &B) -> f32 {
    b.s(8.0)
}

pub(crate) fn build(v: &View, d: &Data, th: &Theme, w: f32, h: f32) -> (Vec<Item>, [f32; 5]) {
    let mut b = B {
        items: Vec::with_capacity(512),
        th,
        clip: None,
        dy: 0.0,
    };
    let mut content = [0.0f32; 5];
    b.panel(R::new(0.0, 0.0, w, h), Shade::Zone);
    // Etroite, la barre de commande passe sur deux rangees.
    let row_h = b.s(40.0);
    let two = w < b.s(1240.0);
    let bar_h = if two { 2.0 * row_h } else { row_h };
    let info_h = b.s(28.0);
    command_bar(&mut b, v, d, w, row_h, two);
    info_bar(&mut b, v, d, w, h - info_h, info_h);
    let Some(_) = d.track else {
        let r = R::new(0.0, bar_h, w, h - bar_h - info_h);
        b.text(
            R::new(r.x, r.cy() - b.s(14.0), r.w, b.s(20.0)),
            "No track selected",
            F::Bold,
            th.text_dim,
            Align::Center,
        );
        return (b.items, content);
    };
    let gap = gap_of(&b);
    let body = R::new(0.0, bar_h, w, (h - bar_h - info_h).max(0.0));
    let lw = b.s(v.left_w).min((w * 0.4).max(b.s(160.0)));
    let rw = if v.rack_fold {
        b.s(28.0)
    } else {
        b.s(v.right_w).min((w * 0.4).max(b.s(160.0)))
    };
    let left = R::new(gap, body.y + gap, lw - gap, body.h - 2.0 * gap);
    let right = R::new(w - rw, body.y + gap, rw - gap, body.h - 2.0 * gap);
    let center = R::new(
        left.right() + gap,
        body.y + gap,
        (right.x - left.right() - 2.0 * gap).max(b.s(80.0)),
        body.h - 2.0 * gap,
    );
    // Les coutures (glisser = largeur, double-clic = celle d'origine).
    b.push(
        R::new(left.right(), body.y, gap, body.h),
        Ctl::Seam(0),
        Look::Panel(Shade::Zone),
        "",
    );
    if !v.rack_fold {
        b.push(
            R::new(center.right(), body.y, gap, body.h),
            Ctl::Seam(1),
            Look::Panel(Shade::Zone),
            "",
        );
    }
    content[2] = left_zone(&mut b, v, d, left);
    content[0] = center_zone(&mut b, v, d, center, &mut content);
    content[1] = rack(&mut b, v, d, right);
    dress_internal(&mut b.items, v, d, th);
    scroll_bars(&mut b, v, &content);
    (b.items, content)
}

/// Une barre de defilement par zone qui deborde (posee par-dessus).
fn scroll_bars(b: &mut B, v: &View, content: &[f32; 5]) {
    let clips: Vec<(u8, R)> = b
        .items
        .iter()
        .filter_map(|i| match i.look {
            Look::Clip(z) => Some((z, i.r)),
            _ => None,
        })
        .collect();
    let bw = b.s(6.0);
    for (z, r) in clips {
        let c = content[z as usize];
        if c <= r.h + 1.0 || r.h < b.s(30.0) {
            continue;
        }
        let len = (r.h * r.h / c).clamp(b.s(24.0).min(r.h), r.h);
        let max = (c - r.h).max(1.0);
        let pos = (r.h - len) * (v.scroll[z as usize] / max).clamp(0.0, 1.0);
        b.push(
            R::new(r.right() - bw - b.s(1.0), r.y, bw, r.h),
            Ctl::ScrollBar(z),
            Look::Scroll { pos, len },
            "",
        );
    }
}

// ---------------------------------------------------------------------------
// La barre de commande
// ---------------------------------------------------------------------------

fn command_bar(b: &mut B, v: &View, d: &Data, w: f32, bh: f32, two: bool) {
    let th = b.th;
    b.panel(
        R::new(0.0, 0.0, w, if two { 2.0 * bh } else { bh }),
        Shade::Bar,
    );
    let ch = b.s(28.0);
    let sq = ch;
    let mut y = (bh - ch) / 2.0;
    let mut x = b.s(10.0);
    let narrow = w < b.s(1000.0);
    if !narrow {
        let t = "FX Constellation";
        b.text(
            R::new(x, 0.0, tw(th, t), bh),
            t,
            F::Bold,
            th.text,
            Align::Left,
        );
        x += tw(th, t) + b.s(4.0);
    }
    // La piste suivie, et le verrou.
    let name = d.track.map_or("—".to_string(), |t| {
        if t.is_empty() {
            "—".into()
        } else {
            t.to_string()
        }
    });
    let fw = (tw(th, &name) + sq).clamp(b.s(90.0), b.s(220.0));
    b.push(
        R::new(x, y, fw, ch),
        Ctl::None,
        Look::Field {
            text: name.clone(),
            modified: false,
            muted: d.track.is_none(),
        },
        "",
    );
    b.icon_btn(
        R::new(
            x + fw - sq + b.s(2.0),
            y + b.s(2.0),
            sq - b.s(4.0),
            ch - b.s(4.0),
        ),
        Ctl::Lock,
        if d.locked { Icon::Lock } else { Icon::LockOpen },
        d.locked,
        "Lock: stay on this track whatever REAPER selects",
    );
    x += fw + b.s(12.0);
    // Le preset.
    b.icon_btn(
        R::new(x, y, sq, ch),
        Ctl::PresetPrev,
        Icon::ChevronLeft,
        false,
        "Previous preset",
    );
    x += sq + b.s(2.0);
    let pw = b.s(170.0);
    b.push(
        R::new(x, y, pw, ch),
        Ctl::PresetField,
        Look::Field {
            text: if d.preset.is_empty() {
                "No preset".into()
            } else {
                d.preset.to_string()
            },
            modified: d.modified,
            muted: d.preset.is_empty(),
        },
        &if d.preset.is_empty() {
            "Preset: none\nClick: save, load, init, rename…".to_string()
        } else {
            format!(
                "Preset: {}{}\nClick: save, load, init, rename…",
                d.preset,
                if d.modified { " (modified)" } else { "" }
            )
        },
    );
    x += pw + b.s(2.0);
    b.icon_btn(
        R::new(x, y, sq, ch),
        Ctl::PresetNext,
        Icon::ChevronRight,
        false,
        "Next preset",
    );
    x += sq + b.s(2.0);
    b.icon_btn(
        R::new(x, y, sq, ch),
        Ctl::PresetSave,
        Icon::Save,
        false,
        "Save the preset",
    );
    x += sq + b.s(12.0);
    // Les snapshots du preset.
    b.glyph(R::new(x, y, b.s(18.0), ch), Icon::Camera, th.text_dim);
    x += b.s(22.0);
    b.icon_btn(
        R::new(x, y, sq, ch),
        Ctl::SnapPrev,
        Icon::ChevronLeft,
        false,
        "Previous snapshot",
    );
    x += sq + b.s(2.0);
    let sw = b.s(120.0);
    b.push(
        R::new(x, y, sw, ch),
        Ctl::SnapField,
        Look::Field {
            text: d.snapshot_label.clone(),
            modified: false,
            muted: !d.has_preset,
        },
        "Snapshots: the mapping and the pad, inside the preset",
    );
    x += sw + b.s(2.0);
    b.icon_btn(
        R::new(x, y, sq, ch),
        Ctl::SnapNext,
        Icon::ChevronRight,
        false,
        "Next snapshot",
    );
    x += sq + b.s(2.0);
    b.button_ex(
        R::new(x, y, sq, ch),
        Ctl::SnapAdd,
        "",
        Some(Icon::Plus),
        false,
        d.has_preset,
        Tone::Normal,
        "Capture a new snapshot",
    );
    x += sq + b.s(12.0);
    if let Some(m) = d.morph {
        let mw = b.s(110.0);
        b.push(
            R::new(x, y + b.s(8.0), mw, ch - b.s(16.0)),
            Ctl::Morph,
            Look::Fader {
                v: m,
                label: String::new(),
                enabled: true,
            },
            "Morph between the A and B captures",
        );
    }
    // A droite : les modes du pad, la capture, les reglages, l'aide.
    if two {
        y += bh;
    }
    let mut rx = w - b.s(10.0);
    rx -= sq;
    b.icon_btn(R::new(rx, y, sq, ch), Ctl::Help, Icon::Help, false, "Help");
    rx -= sq + b.s(2.0);
    b.icon_btn(
        R::new(rx, y, sq, ch),
        Ctl::Settings,
        Icon::Settings,
        false,
        "Settings",
    );
    rx -= b.s(12.0);
    let cw = if d.capture > 0 { b.s(46.0) } else { sq };
    rx -= cw;
    b.button_ex(
        R::new(rx, y, cw, ch),
        Ctl::Capture,
        &if d.capture > 0 {
            d.capture.to_string()
        } else {
            String::new()
        },
        Some(Icon::Circle),
        false,
        true,
        if d.capture > 0 {
            Tone::Danger
        } else {
            Tone::Normal
        },
        "Capture to envelopes: the armed pad axes and macros are written while REAPER plays",
    );
    rx -= b.s(12.0);
    let segw = b.s(36.0);
    rx -= 3.0 * segw;
    b.seg_ic(
        R::new(rx, y, 3.0 * segw, ch),
        &[
            (Some(Icon::Hand), ""),
            (Some(Icon::Footprints), ""),
            (Some(Icon::Spline), ""),
        ],
        d.t.pad.nav as usize,
        |i| Ctl::Nav(i as u8),
        &[
            "Manual: the mouse moves the pad",
            "Walk: the pad wanders on its own (click again to restart it)",
            "Figures: the pad draws a figure (click again to restart it)",
        ],
    );
    rx -= b.s(8.0) + 2.0 * segw;
    b.seg_ic(
        R::new(rx, y, 2.0 * segw, ch),
        &[(Some(Icon::Crosshair), ""), (Some(Icon::Grid), "")],
        d.t.pad.granular as usize,
        |i| Ctl::Granular(i == 1),
        &[
            "Single: the pad moves each param from its base",
            "Granular: the pad blends random grain points",
        ],
    );
    let _ = v;
}

// ---------------------------------------------------------------------------
// La barre du bas
// ---------------------------------------------------------------------------

fn info_bar(b: &mut B, v: &View, d: &Data, w: f32, y: f32, ih: f32) {
    let th = b.th;
    b.panel(R::new(0.0, y, w, ih), Shade::Bar);
    let (rw0, _) = v.dock.reserve(w, y + ih, th.scale);
    let mut x = b.s(10.0) + rw0;
    let isz = b.s(14.0);
    if let Some(m) = d.msg {
        b.text(
            R::new(x, y, (w * 0.5).max(b.s(200.0)), ih),
            m,
            F::Small,
            th.accent,
            Align::Left,
        );
    } else {
        for (ic, n) in [
            (Icon::ListChecks, d.t.tracked_count()),
            (Icon::Link, d.links),
            (Icon::Activity, d.modulated),
        ] {
            b.glyph(R::new(x, y, isz, ih), ic, th.text_dim);
            x += isz + b.s(5.0);
            let t = n.to_string();
            b.text(
                R::new(x, y, tw(th, &t), ih),
                &t,
                F::Small,
                th.text_dim,
                Align::Left,
            );
            x += tw(th, &t) + b.s(6.0);
        }
    }
    let bs = ih - b.s(4.0);
    let mut rx = w - b.s(8.0) - bs;
    b.icon_btn(
        R::new(rx, y + b.s(2.0), bs, bs),
        Ctl::StopEngines,
        Icon::Stop,
        false,
        "Stop Walk and Figures on every track (links stay)",
    );
    rx -= bs + b.s(2.0);
    b.icon_btn(
        R::new(rx, y + b.s(2.0), bs, bs),
        Ctl::ReleaseLinks,
        Icon::Unlink,
        false,
        "Release every FX Constellation link of this track (values stay)",
    );
}

// ---------------------------------------------------------------------------
// La colonne de gauche : le pad, son mouvement, ROLL, les macros
// ---------------------------------------------------------------------------

fn left_zone(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    b.push(r, Ctl::None, Look::Clip(2), "");
    b.clip = Some(2);
    b.dy = v.scroll[2];
    let cp = pad_of(b);
    let gap = gap_of(b);
    let x = r.x;
    let w = r.w;
    let iw = w - 2.0 * cp;
    let ix = x + cp;
    let mut y = r.y;
    let p = &d.t.pad;
    // Le pad : un carre, et ses deux axes.
    let side = iw.min((r.h * 0.52).max(b.s(120.0))).max(b.s(80.0));
    let card = b.open_card(x, y, w);
    y += cp;
    let pr = R::new(ix + (iw - side) / 2.0, y, side, side);
    let grains = if p.granular {
        d.t.grains.iter().map(|g| (g.x, g.y)).collect()
    } else {
        Vec::new()
    };
    let figure = (p.nav == Nav::Figures).then(|| {
        (0..=96)
            .map(|i| {
                cp_fxc::pad::figure_point(p.fig_mode, i as f64 / 96.0, p.fig_size, p.zone_ordered())
            })
            .collect()
    });
    b.push(
        pr,
        Ctl::Pad,
        Look::Pad(PadLook {
            x: p.x,
            y: p.y,
            ax: p.ax,
            ay: p.ay,
            zone: p.has_zone().then(|| p.zone_ordered()),
            grains,
            grid: p.grid,
            drive_x: d.plan.pad_link_x.then(|| src_color(th, p.src_x)),
            drive_y: d.plan.pad_link_y.then(|| src_color(th, p.src_y)),
            figure,
            runs: d.engine_runs,
            trail: v.trail.iter().copied().collect(),
        }),
        "Drag: move the pad · right-drag: draw a zone · right-click: clear it",
    );
    y += side + b.s(8.0);
    let rh = b.s(26.0);
    let mut px = ix;
    for axis in [0u8, 1u8] {
        let slot = if axis == 0 { p.src_x } else { p.src_y };
        let name = if axis == 0 { "X" } else { "Y" };
        let pv = if axis == 0 { p.x } else { p.y };
        let pw = b.s(92.0);
        let label = if slot != 0 {
            format!("{name}  {}", short_label(slot))
        } else {
            format!("{name}  {pv:.2}")
        };
        b.push(
            R::new(px, y, pw, rh),
            Ctl::Pill(axis),
            Look::Pill {
                color: src_color(
                    th,
                    if slot != 0 {
                        slot
                    } else if axis == 0 {
                        -1
                    } else {
                        -2
                    },
                ),
                label,
                driven: slot != 0,
            },
            &if slot != 0 {
                format!(
                    "Pad {name} is driven by {}\nClick: free it",
                    long_label(slot)
                )
            } else {
                format!("Pad {name}: {pv:.2}\nDrop a source here to drive it")
            },
        );
        let ds = b.s(12.0);
        b.push(
            R::new(px + b.s(7.0), y + (rh - ds) / 2.0, ds, ds),
            Ctl::AxisDot(axis),
            Look::Dot {
                color: src_color(th, if axis == 0 { -1 } else { -2 }),
                hollow: false,
            },
            &format!("Pad {name}: drag onto a knob to modulate it"),
        );
        px += pw + b.s(6.0);
    }
    let sq = rh;
    b.icon_btn(
        R::new(ix + iw - sq, y, sq, sq),
        Ctl::PadReset,
        Icon::Locate,
        false,
        "Pad and anchor back to the center",
    );
    if p.has_zone() {
        b.icon_btn(
            R::new(ix + iw - 2.0 * sq - b.s(2.0), y, sq, sq),
            Ctl::ZoneClear,
            Icon::Scan,
            true,
            "Clear the zone (right-drag on the pad draws a new one)",
        );
    }
    y += rh + cp;
    b.close(card, y);
    y += gap;
    // Le mouvement du pad.
    let card = b.open_card(x, y, w);
    y += cp;
    let hh = b.s(24.0);
    let (nic, nname) = match p.nav {
        Nav::Manual => (Icon::Hand, "MANUAL"),
        Nav::Walk => (Icon::Footprints, "WALK"),
        Nav::Figures => (Icon::Spline, "FIGURES"),
    };
    b.glyph(R::new(ix, y, b.s(16.0), hh), nic, th.accent);
    b.label(R::new(ix + b.s(22.0), y, b.s(90.0), hh), nname);
    let kh = b.s(58.0);
    let kw = (iw / 4.0).floor();
    let knob =
        |b: &mut B, i: usize, y: f32, f: PadF, v: f64, label: &str, val: String, tip: &str| {
            b.knob(
                R::new(ix + kw * i as f32, y, kw, kh),
                Ctl::PadField(f),
                v,
                label,
                &val,
                true,
                false,
                tip,
            );
        };
    match p.nav {
        Nav::Manual => {
            y += hh + b.s(4.0);
            knob(
                b,
                0,
                y,
                PadF::Smooth,
                p.smooth,
                "SMOOTH",
                format!("{:.2}", p.smooth),
                "Smooth: the pad chases the mouse",
            );
            knob(
                b,
                1,
                y,
                PadF::MaxSpeed,
                (p.max_speed - 0.1) / 9.9,
                "SPEED",
                format!("{:.1}", p.max_speed),
                "Top speed of the smoothed pad",
            );
            y += kh;
        }
        Nav::Walk => {
            let sync_w = b.s(64.0);
            b.chip(
                R::new(ix + iw - sync_w, y + b.s(1.0), sync_w, hh - b.s(2.0)),
                Ctl::WalkSync,
                WALK_SYNCS[p.walk_sync as usize % 6],
                p.walk_sync > 0,
                true,
                "Walk timing: free (Hz) or synced to the tempo",
            );
            b.icon_chip(
                R::new(ix + iw - sync_w - b.s(4.0) - hh, y, hh, hh),
                Ctl::WalkJump,
                Icon::Zap,
                p.walk_jump,
                "Jump: teleport to each new point",
            );
            y += hh + b.s(4.0);
            if p.walk_sync == 0 {
                knob(
                    b,
                    0,
                    y,
                    PadF::WalkSpeed,
                    (p.walk_speed - 0.1) / 9.9,
                    "SPEED",
                    format!("{:.1} Hz", p.walk_speed),
                    "Walk speed",
                );
                knob(
                    b,
                    1,
                    y,
                    PadF::WalkJitter,
                    p.walk_jitter,
                    "JITTER",
                    format!("{:.2}", p.walk_jitter),
                    "Walk jitter: uneven steps",
                );
                y += kh;
            }
        }
        Nav::Figures => {
            y += hh + b.s(4.0);
            let cell = b.s(26.0);
            for (i, name) in FIGURES.iter().enumerate() {
                b.chip(
                    R::new(ix + (cell + b.s(4.0)) * i as f32, y, cell, cell),
                    Ctl::Figure(i as u8),
                    &name[..1],
                    p.fig_mode == i as u8,
                    true,
                    name,
                );
            }
            y += cell + b.s(6.0);
            knob(
                b,
                0,
                y,
                PadF::FigSpeed,
                (p.fig_speed - 0.01) / 9.99,
                "SPEED",
                format!("{:.2} Hz", p.fig_speed),
                "Figure speed",
            );
            knob(
                b,
                1,
                y,
                PadF::FigSize,
                (p.fig_size - 0.1) / 0.9,
                "SIZE",
                format!("{:.2}", p.fig_size),
                "Figure size",
            );
            y += kh;
        }
    }
    let rrh = b.s(22.0);
    b.glyph(
        R::new(ix, y, b.s(14.0), rrh),
        Icon::MoveHorizontal,
        th.text_dim,
    );
    b.push(
        R::new(
            ix + b.s(24.0),
            y + b.s(7.0),
            iw - b.s(28.0),
            rrh - b.s(14.0),
        ),
        Ctl::GRange,
        Look::Range {
            lo: p.gmin,
            hi: p.gmax,
        },
        "Range: how far the values may go",
    );
    y += rrh;
    if p.granular {
        y += b.s(6.0);
        b.chip(
            R::new(ix, y, b.s(60.0), rrh + b.s(4.0)),
            Ctl::GrainGrid,
            &format!("{0}x{0}", p.grid),
            true,
            true,
            "Grain grid",
        );
        b.icon_btn(
            R::new(ix + b.s(64.0), y, rrh + b.s(4.0), rrh + b.s(4.0)),
            Ctl::GrainRoll,
            Icon::Dices,
            false,
            "New values for every grain",
        );
        y += rrh + b.s(4.0);
    }
    y += cp;
    b.close(card, y);
    y += gap;
    // ROLL.
    let card = b.open_card(x, y, w);
    y += cp;
    let rollh = b.s(34.0);
    b.button_ex(
        R::new(ix, y, iw, rollh),
        Ctl::Roll,
        "ROLL",
        Some(Icon::Dices),
        true,
        true,
        Tone::Accent,
        &format!(
            "Roll every armed axis ({} of 8 armed)",
            d.roll.armed_count()
        ),
    );
    y += rollh + b.s(6.0);
    let ag = b.s(5.0);
    let aw = ((iw - 3.0 * ag) / 4.0).floor();
    let ah = b.s(26.0);
    for (i, a) in AXES.iter().enumerate() {
        let ic = match i {
            0 => Icon::ListChecks,
            1 => Icon::Gauge,
            2 => Icon::MoveHorizontal,
            3 => Icon::Move,
            4 => Icon::FlipVertical,
            5 => Icon::Power,
            6 => Icon::ArrowDownUp,
            _ => Icon::Activity,
        };
        b.icon_chip(
            R::new(
                ix + (aw + ag) * (i % 4) as f32,
                y + (ah + ag) * (i / 4) as f32,
                aw,
                ah,
            ),
            Ctl::RollAxis(i as u8),
            ic,
            d.roll.armed[i],
            &format!("{} — click to arm, right-click to roll it now", a.tip()),
        );
    }
    y += 2.0 * ah + ag + cp;
    b.close(card, y);
    y += gap;
    // Les macros.
    let card = b.open_card(x, y, w);
    y += cp;
    let mw = (iw / 4.0).floor();
    let mh = b.s(60.0);
    for n in 1..=8u8 {
        let i = (n - 1) as usize;
        let mx = ix + mw * (i % 4) as f32;
        let my = y + mh * (i / 4) as f32;
        let slot = 210 + n as i32;
        let c = src_color(th, slot);
        let hl =
            (v.hover == Ctl::TileDot(slot) || v.carry.is_some_and(|c| c.0 == slot)).then_some(c);
        b.push(
            R::new(mx, my, mw, mh - b.s(4.0)),
            Ctl::Macro(n),
            Look::Knob {
                v: d.macros[i],
                depth: 0.0,
                bipolar: false,
                enabled: d.bridge,
                rings: Vec::new(),
                live: None,
                label: d.t.macro_name(n as usize),
                value: format!("{:.0} %", d.macros[i] * 100.0),
                hl,
                size: b.s(32.0),
                dim: false,
            },
            &format!(
                "{}  {:.0} %\nRight-click: rename · drag the dot onto a knob to modulate it",
                d.t.macro_name(n as usize),
                d.macros[i] * 100.0
            ),
        );
        let ds = b.s(9.0);
        b.push(
            R::new(mx + mw - ds - b.s(8.0), my + b.s(1.0), ds, ds),
            Ctl::MacroDot(n),
            Look::Dot {
                color: c,
                hollow: false,
            },
            "Drag onto a knob to modulate it",
        );
    }
    y += 2.0 * mh + cp - b.s(4.0);
    b.close(card, y);
    b.clip = None;
    b.dy = 0.0;
    y - r.y
}

// ---------------------------------------------------------------------------
// Le centre : les onglets, les parametres
// ---------------------------------------------------------------------------

fn center_zone(b: &mut B, v: &View, d: &Data, r: R, content: &mut [f32; 5]) -> f32 {
    let th = b.th;
    let tab_h = b.s(30.0);
    let tabs_w = b.s(330.0).min(r.w);
    b.seg_ic(
        R::new(r.x, r.y, tabs_w, tab_h),
        &[
            (Some(Icon::SlidersVertical), "Params"),
            (Some(Icon::AudioLines), "Sound Gen"),
            (Some(Icon::Grid), "Seq"),
        ],
        v.tab as usize,
        |i| Ctl::Tab(i as u8),
        &[
            "The tracked params of the chain",
            "The Sound Generator of this track",
            "The sequencer of this track",
        ],
    );
    if v.tab == 0 {
        // A droite : chercher, cacher, les valeurs, replier, tout retirer.
        let sq = tab_h;
        let mut x = r.right() - sq;
        b.icon_btn(
            R::new(x, r.y, sq, sq),
            Ctl::UntrackAll,
            Icon::ListX,
            false,
            "Untrack every param",
        );
        x -= sq + b.s(2.0);
        b.icon_btn(
            R::new(x, r.y, sq, sq),
            Ctl::CollapseAll,
            Icon::Fold,
            false,
            "Fold or unfold every FX",
        );
        x -= sq + b.s(2.0);
        b.icon_btn(
            R::new(x, r.y, sq, sq),
            Ctl::Values,
            Icon::Eye,
            v.show_values,
            "Show the values",
        );
        x -= b.s(10.0);
        let hidden: usize = d.fx.iter().map(|f| f.hidden).sum();
        let hw = if hidden > 0 { b.s(64.0) } else { sq };
        x -= hw;
        b.button(
            R::new(x, r.y, hw, sq),
            Ctl::HideMenu,
            &if hidden > 0 {
                hidden.to_string()
            } else {
                String::new()
            },
            Some(Icon::Funnel),
            v.show_hidden,
            &format!(
                "{hidden} params hidden by their name (MIDI, CC…)\nClick: show them, edit the words"
            ),
        );
        x -= b.s(6.0);
        let sw = b.s(210.0).min((x - r.x - tabs_w - b.s(10.0)).max(0.0));
        if sw > b.s(60.0) {
            b.push(
                R::new(x - sw, r.y, sw, sq),
                Ctl::Search,
                Look::Search {
                    focus: v.search_focus,
                },
                "Filter the params by name (Esc clears)",
            );
        }
    }
    // Le clavier, en bas.
    let kb_bar = b.s(30.0);
    let kb_keys = if v.kb_open { b.s(64.0) } else { 0.0 };
    let kb = R::new(r.x, r.bottom() - kb_bar - kb_keys, r.w, kb_bar + kb_keys);
    keyboard(b, v, d, kb);
    let body = R::new(
        r.x,
        r.y + tab_h + b.s(8.0),
        r.w,
        (kb.y - r.y - tab_h - b.s(16.0)).max(0.0),
    );
    let _ = th;
    match v.tab {
        1 => {
            content[3] = crate::panels::sound_gen(b, v, d, body);
            0.0
        }
        2 => {
            content[4] = crate::panels::sequencer(b, v, d, body);
            0.0
        }
        _ => params_tab(b, v, d, body),
    }
}

fn keyboard(b: &mut B, v: &View, d: &Data, r: R) {
    let th = b.th;
    b.panel(r, Shade::Card);
    let bh = b.s(30.0);
    b.push(
        R::new(r.x + b.s(4.0), r.y + b.s(3.0), b.s(54.0), bh - b.s(6.0)),
        Ctl::KbFold,
        Look::Button {
            label: String::new(),
            icon: Some(Icon::Piano),
            on: v.kb_open,
            enabled: true,
            tone: Tone::Normal,
        },
        "Play the Sound Gen with the mouse",
    );
    b.glyph(
        R::new(r.x + b.s(40.0), r.y, b.s(12.0), bh),
        if v.kb_open {
            Icon::ChevronDown
        } else {
            Icon::ChevronUp
        },
        th.text_dim,
    );
    if let Some(n) = d.kb_note {
        b.text(
            R::new(r.x + b.s(66.0), r.y, b.s(120.0), bh),
            &note_name(n),
            F::Bold,
            th.accent,
            Align::Left,
        );
    }
    if !v.kb_open {
        return;
    }
    // Trois octaves depuis C3, et le do du haut.
    let inner = R::new(
        r.x + b.s(6.0),
        r.y + bh,
        r.w - b.s(12.0),
        r.h - bh - b.s(6.0),
    );
    let ky = inner.y;
    let kh = inner.h;
    let whites: Vec<u8> = (48u8..=84).filter(|n| !is_black(*n)).collect();
    let ww = (inner.w / whites.len() as f32).floor();
    for (i, n) in whites.iter().enumerate() {
        b.push(
            R::new(inner.x + ww * i as f32, ky, ww - 1.0, kh),
            Ctl::Key(*n),
            Look::Key {
                black: false,
                lit: d.kb_note == Some(*n),
            },
            "",
        );
    }
    for n in 48u8..=84 {
        if !is_black(n) {
            continue;
        }
        let below = whites.iter().position(|w| *w == n - 1).unwrap_or(0);
        let bx = inner.x + ww * (below + 1) as f32 - ww * 0.3;
        b.push(
            R::new(bx, ky, ww * 0.6, kh * 0.6),
            Ctl::Key(n),
            Look::Key {
                black: true,
                lit: d.kb_note == Some(n),
            },
            "",
        );
    }
}

fn params_tab(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    let list = r;
    b.push(list, Ctl::None, Look::Clip(0), "");
    b.clip = Some(0);
    b.dy = v.scroll[0];
    let gap = gap_of(b);
    // DETACHED vide : une colonne etroite ; sinon un tiers.
    let any_detached = d.fx.iter().any(|f| f.detached);
    let dw = if any_detached {
        ((list.w - gap) * 0.34)
            .max(b.s(230.0))
            .min((list.w - gap) * 0.45)
            .floor()
    } else {
        b.s(170.0).min(list.w * 0.3)
    };
    // La place de la barre de defilement, a droite de chaque colonne.
    let sb = b.s(10.0);
    let lw = list.w - gap - dw - sb;
    let pools = [
        (false, R::new(list.x, list.y, lw, list.h)),
        (true, R::new(list.x + lw + gap, list.y, dw, list.h)),
    ];
    let mut tallest = 0.0f32;
    let bh = b.s(24.0);
    for (detached, pr) in pools {
        // La colonne entiere recoit un en-tete tire (les effets, poses
        // ensuite, l'emportent pour la souris).
        let moving = v
            .moving_fx()
            .is_some_and(|g| d.fx.iter().any(|f| f.guid == g && f.detached != detached));
        b.push(
            R::new(pr.x, pr.y + b.dy, pr.w, pr.h.max(b.s(40.0))),
            Ctl::Pool(detached),
            Look::Panel(if moving {
                Shade::Tint(th.accent.fade(0.12))
            } else {
                Shade::Zone
            }),
            "",
        );
        let mut y = pr.y;
        let n = d.fx.iter().filter(|f| f.detached == detached).count();
        let (label, tip) = if detached {
            (
                "DETACHED",
                "FX that ROLL leaves alone: nothing is re-rolled and they keep their place in the chain",
            )
        } else {
            (
                "ROLL",
                "FX that ROLL re-rolls: tracked params, bases, ranges, X/Y, invert, sources, bypass and chain order",
            )
        };
        b.push(
            R::new(pr.x + b.s(2.0), y, b.s(80.0), bh),
            Ctl::None,
            Look::Text {
                s: label.into(),
                font: F::Small,
                color: th.text_dim,
                align: Align::Left,
            },
            tip,
        );
        let nt = n.to_string();
        b.text(
            R::new(pr.x + b.s(2.0) + tw(th, label), y, b.s(40.0), bh),
            &nt,
            F::Small,
            th.text_faint,
            Align::Left,
        );
        b.icon_btn(
            R::new(pr.right() - bh, y, bh, bh),
            Ctl::AddFx(detached),
            Icon::Plus,
            false,
            if detached {
                "Add an FX out of ROLL"
            } else {
                "Add an FX to ROLL"
            },
        );
        y += bh + b.s(6.0);
        if n == 0 {
            let dh = (pr.h - (y - pr.y) - b.s(4.0))
                .max(b.s(80.0))
                .min(b.s(240.0));
            b.panel(R::new(pr.x, y, pr.w, dh), Shade::Drop);
            b.glyph(
                R::new(pr.x, y, pr.w, dh),
                if detached { Icon::Download } else { Icon::Plus },
                th.text_faint,
            );
            y += dh;
        }
        for f in d.fx.iter().filter(|f| f.detached == detached) {
            y = fx_group(b, v, d, f, R::new(pr.x, y, pr.w, 0.0), list);
            y += gap;
        }
        tallest = tallest.max(y - pr.y);
    }
    b.clip = None;
    b.dy = 0.0;
    tallest
}

/// Les parametres montres d'un effet : le filtre « seulement les suivis »
/// et la recherche.
fn shown_rows<'a>(v: &View, d: &Data, f: &'a crate::FxRow) -> Vec<&'a crate::ParamRow> {
    let only = d.t.fx_tracked_only.get(&f.guid).copied().unwrap_or(false);
    let q = v.search.text.trim().to_lowercase();
    f.params
        .iter()
        .filter(|p| !only || d.t.params.get(&p.key).is_some_and(|s| s.sel))
        .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
        .collect()
}

fn fx_group(b: &mut B, v: &View, d: &Data, f: &crate::FxRow, r: R, list: R) -> f32 {
    let th = b.th;
    let hh = b.s(36.0);
    let cp = b.s(8.0);
    let folded = v.folded.contains(&f.guid);
    let rows = shown_rows(v, d, f);
    let cols = if r.w >= b.s(2.0 * 250.0) { 2 } else { 1 };
    let cg = b.s(6.0);
    let cell_h = b.s(50.0);
    let n_rows = rows.len().div_ceil(cols);
    let body_h = if folded {
        0.0
    } else if rows.is_empty() {
        cp + b.s(22.0) + cp
    } else {
        cp + (cell_h + cg) * n_rows as f32 - cg + cp
    };
    let mut y = r.y;
    b.panel(R::new(r.x, y, r.w, hh + body_h), Shade::Card);
    b.push(
        R::new(r.x, y, r.w, hh),
        Ctl::FxHead(f.guid.clone()),
        Look::Panel(if folded { Shade::Card } else { Shade::Head }),
        &format!(
            "{}\nClick: fold · drag to the other column: in or out of ROLL",
            f.name
        ),
    );
    let tracked = f
        .params
        .iter()
        .filter(|p| d.t.params.get(&p.key).is_some_and(|s| s.sel))
        .count();
    let mut x = r.x + b.s(8.0);
    b.glyph(
        R::new(x, y, b.s(14.0), hh),
        if folded {
            Icon::ChevronRight
        } else {
            Icon::ChevronDown
        },
        th.text_dim,
    );
    x += b.s(20.0);
    let ic = b.s(26.0);
    let name_min = b.s(90.0);
    let fits = |n: f32| r.w - (x - r.x) - n * (ic + b.s(2.0)) - b.s(12.0) >= name_min + b.s(60.0);
    let verbs: &[u8] = if fits(6.0) {
        &[0, 1, 2, 3, 4, 5]
    } else if fits(3.0) {
        &[0, 1, 5]
    } else {
        &[5]
    };
    let verbs_w = verbs.len() as f32 * (ic + b.s(2.0));
    let avail = (r.w - (x - r.x) - verbs_w - b.s(12.0)).max(b.s(40.0));
    let badge = format!("{tracked} / {}", f.params.len());
    let bw = tw(th, &badge) - b.s(4.0);
    let hid = if f.hidden > 0 && avail > name_min + bw + b.s(70.0) {
        b.s(16.0) + tw(th, &f.hidden.to_string())
    } else {
        0.0
    };
    let name_w = (avail - bw - hid - b.s(12.0))
        .min(tw(th, &f.name) + b.s(6.0))
        .max(b.s(30.0));
    b.push(
        R::new(x, y, name_w, hh),
        Ctl::FxHead(f.guid.clone()),
        Look::Text {
            s: f.name.clone(),
            font: F::Bold,
            color: if f.enabled { th.text } else { th.text_faint },
            align: Align::Left,
        },
        &format!(
            "{}\nClick: fold · drag to the other column: in or out of ROLL",
            f.name
        ),
    );
    x += name_w + b.s(6.0);
    b.push(
        R::new(x, y + (hh - b.s(18.0)) / 2.0, bw, b.s(18.0)),
        Ctl::None,
        Look::Badge {
            text: badge,
            color: if tracked > 0 {
                th.accent
            } else {
                th.text_faint
            },
        },
        "",
    );
    x += bw + b.s(8.0);
    if hid > 0.0 {
        b.glyph(R::new(x, y, b.s(12.0), hh), Icon::Funnel, th.text_faint);
        let t = f.hidden.to_string();
        b.text(
            R::new(x + b.s(15.0), y, tw(th, &t), hh),
            &t,
            F::Small,
            th.text_faint,
            Align::Left,
        );
    }
    let tips = [
        "Open the plugin window",
        "Bypass this FX",
        "Roll X / Y on the tracked params",
        "Roll ranges on the tracked params",
        "Roll bases on the tracked params",
        "Everything this FX can do",
    ];
    let nv = verbs.len();
    for (j, &k) in verbs.iter().enumerate() {
        let ix = r.right() - b.s(6.0) - (ic + b.s(2.0)) * (nv - j) as f32 + b.s(2.0);
        b.push(
            R::new(ix, y + (hh - ic) / 2.0, ic, ic),
            Ctl::FxVerb(f.guid.clone(), k),
            Look::Button {
                label: String::new(),
                icon: Some(crate::verb_icon(k)),
                on: k == 1 && f.enabled,
                enabled: true,
                tone: if k == 1 && !f.enabled {
                    Tone::Amber
                } else {
                    Tone::Normal
                },
            },
            tips[k as usize],
        );
    }
    y += hh;
    if folded {
        return y;
    }
    y += cp;
    let iw = r.w - 2.0 * cp;
    let cw = ((iw - cg * (cols - 1) as f32) / cols as f32).floor();
    for (i, p) in rows.iter().enumerate() {
        let row = i / cols;
        let c = i % cols;
        let rr = R::new(
            r.x + cp + (cw + cg) * c as f32,
            y + (cell_h + cg) * row as f32,
            cw,
            cell_h,
        );
        // Seules les cases visibles se posent.
        let vis_y = rr.y - b.dy;
        if vis_y + rr.h < list.y || vis_y > list.bottom() {
            continue;
        }
        param_row(b, v, d, p, rr);
    }
    if rows.is_empty() {
        b.text(
            R::new(r.x + cp, y, iw, b.s(22.0)),
            if v.search.text.trim().is_empty() {
                "—"
            } else {
                "no match"
            },
            F::Small,
            th.text_faint,
            Align::Left,
        );
        return y + b.s(22.0) + cp;
    }
    y + (cell_h + cg) * n_rows as f32 - cg + cp
}

fn param_row(b: &mut B, v: &View, d: &Data, p: &crate::ParamRow, r: R) {
    let th = b.th;
    let ps = d.t.params.get(&p.key).cloned().unwrap_or_default();
    let standby = d.plan.standby.contains(&p.key);
    // La teinte : suivie, ou menee par la source survolee.
    let hover_slot = match &v.hover {
        Ctl::Tile(s) | Ctl::TileDot(s) => Some(*s),
        _ => v.carry.map(|c| c.0),
    };
    let driven_by_hover = hover_slot.is_some_and(|s| ps.sel && ps.srcs.iter().any(|x| x.slot == s));
    let shade = if driven_by_hover {
        Shade::Tint(src_color(th, hover_slot.unwrap_or(0)).fade(0.22))
    } else if ps.sel {
        Shade::Tint(th.accent.fade(0.10))
    } else {
        Shade::Well
    };
    b.panel(r, shade);
    let k = p.key.clone();
    let ks = b.s(38.0);
    let kx = r.x + b.s(6.0);
    let rings = rings_of(d, th, &ps, standby);
    let hl = hover_slot
        .filter(|s| ps.srcs.iter().any(|x| x.slot == *s))
        .map(|s| src_color(th, s));
    let base = if ps.sel || !ps.srcs.is_empty() {
        ps.base
    } else {
        p.live
    };
    let tip = param_tip(d, p, &ps, standby);
    b.push(
        R::new(kx, r.cy() - ks / 2.0, ks, ks),
        Ctl::Knob(k.clone()),
        Look::Knob {
            v: base,
            depth: knob_depth(v, d, &ps),
            bipolar: false,
            enabled: true,
            rings,
            live: Some(p.live),
            label: String::new(),
            value: String::new(),
            hl,
            size: ks,
            dim: !ps.sel && ps.srcs.is_empty(),
        },
        &tip,
    );
    // Les axes du pad, a droite : X, Y, inverser.
    let aw = b.s(22.0);
    let ah = b.s(20.0);
    let axes = r.w >= b.s(220.0);
    let ax0 = if axes {
        r.right() - b.s(6.0) - 3.0 * aw - 2.0 * b.s(2.0)
    } else {
        r.right()
    };
    let pad_writes =
        ps.pad_drives() || (!ps.srcs.is_empty() && !standby && d.plan.bus_of.contains_key(&p.key));
    for i in (0..3u8).filter(|_| axes) {
        let on = match i {
            0 => ps.x,
            1 => ps.y,
            _ => ps.inv,
        };
        let live = ps.sel || on;
        let rr = R::new(ax0 + (aw + b.s(2.0)) * i as f32, r.cy() - ah / 2.0, aw, ah);
        let tip = match i {
            0 => "X: the pad's X axis moves this param",
            1 => "Y: the pad's Y axis moves this param",
            _ => "Invert",
        };
        if i == 2 {
            b.push(
                rr,
                Ctl::Chip(k.clone(), i),
                Look::IconChip {
                    icon: Icon::FlipVertical,
                    on,
                    enabled: live,
                    color: Some(amber()),
                },
                tip,
            );
        } else {
            b.push(
                rr,
                Ctl::Chip(k.clone(), i),
                Look::Chip {
                    label: if i == 0 { "X".into() } else { "Y".into() },
                    on: on && ps.sel && pad_writes,
                    standby: on && ps.sel && !pad_writes,
                    amber: false,
                    enabled: live,
                },
                tip,
            );
        }
    }
    // Le nom (un clic : suivi), la valeur, les sources.
    let nx = kx + ks + b.s(8.0);
    let nw = (ax0 - nx - b.s(6.0)).max(b.s(20.0));
    let line = b.s(18.0);
    let top = r.y + (r.h - 2.0 * line) / 2.0;
    b.push(
        R::new(nx, top, nw, line),
        Ctl::Sel(k.clone()),
        Look::Text {
            s: p.name.clone(),
            font: F::Normal,
            color: if ps.sel {
                th.text
            } else {
                th.text.mix(th.text_dim, 0.4)
            },
            align: Align::Left,
        },
        if ps.sel {
            "Tracked: click to untrack"
        } else {
            "Click to track this param"
        },
    );
    let mut x = nx;
    if v.show_values {
        let t = match p.steps {
            2 => {
                if p.live >= 0.5 {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            _ => p.text.clone(),
        };
        let vw = (tw(th, &t) - b.s(8.0)).min(nw * 0.6).max(b.s(20.0));
        b.text(
            R::new(x, top + line, vw, line),
            &t,
            F::Small,
            if ps.sel {
                th.accent.mix(th.text, 0.55)
            } else {
                th.text_dim
            },
            Align::Left,
        );
        x += vw + b.s(6.0);
    }
    // Les sources : leurs pastilles (un clic : leur menu).
    let dots: Vec<(Color, bool)> = ps
        .srcs
        .iter()
        .map(|s| (src_color(th, s.slot), s.off))
        .collect();
    let label = if ps.srcs.is_empty() {
        String::new()
    } else if ps.srcs.len() == 1 {
        short_label(ps.srcs[0].slot)
    } else {
        String::new()
    };
    let sw = (nx + nw - x).max(b.s(16.0));
    b.push(
        R::new(x, top + line, sw, line),
        Ctl::Src(k),
        Look::SrcLabel { dots, label },
        "Sources: click for the source menu",
    );
}

fn is_black(n: u8) -> bool {
    matches!(n % 12, 1 | 3 | 6 | 8 | 10)
}

/// Le nom d'une note (60 = C4).
pub fn note_name(n: u8) -> String {
    const N: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", N[(n % 12) as usize], (n / 12) as i32 - 1)
}

/// La profondeur que tire Alt + glisser : la premiere source, ou en mode Map
/// celle de la source choisie dans le rack (0 si elle n'y est pas encore).
fn knob_depth(v: &View, d: &Data, ps: &PState) -> f64 {
    let g = d.t.pad.gesture_range;
    if v.map {
        ps.index_of(v.sel_slot).map_or(0.0, |i| ps.span(i, g))
    } else {
        ps.span(0, g)
    }
}

/// Les boutons des effets internes que des sources menent : leurs anneaux,
/// leur profondeur (Alt + glisser), et ce qu'ils jouent.
fn dress_internal(items: &mut [Item], v: &View, d: &Data, th: &Theme) {
    for it in items.iter_mut() {
        let Some(key) = crate::internal_key(&it.ctl, &d.guids, v.sel_slot) else {
            continue;
        };
        let Some(ps) = d.t.params.get(&key) else {
            continue;
        };
        if ps.srcs.is_empty() || !ps.sel {
            continue;
        }
        if let Look::Knob {
            depth,
            rings,
            live,
            hl,
            ..
        } = &mut it.look
        {
            *rings = rings_of(d, th, ps, false);
            *depth = knob_depth(v, d, ps);
            let g = d.t.pad.gesture_range;
            let mut sum = 0.0;
            for (i, s) in ps.srcs.iter().enumerate() {
                if let Some(sv) = d.src.get(&s.slot) {
                    sum += ps.span_eff(i, g) * (sv - link_center(d, s.slot, s.pol));
                }
            }
            *live = Some((ps.base + sum).clamp(0.0, 1.0));
            let hover = match &v.hover {
                Ctl::Tile(s) | Ctl::TileDot(s) => Some(*s),
                _ => v.carry.map(|c| c.0),
            };
            *hl = hover
                .filter(|s| ps.srcs.iter().any(|x| x.slot == *s))
                .map(|s| src_color(th, s));
            it.tip = format!(
                "{}\n{}\nAlt+drag: depth · Alt+click: release · Shift+click: bypass",
                it.tip,
                ps.srcs
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("{} {:+.0} %", long_label(s.slot), ps.span(i, g) * 100.0))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
}

/// Le neutre d'un lien (sa polarite, sinon celle de sa source).
pub(crate) fn link_center(d: &Data, slot: i32, pol: Option<cp_fxc::slots::Pol>) -> f64 {
    if let Some(p) = pol {
        return p.center();
    }
    let uni = match band_of(slot) {
        Some(Band::Lfo(n)) => d
            .bank
            .is_some_and(|b| b.slots.get(n as usize - 1).is_some_and(|s| s.uni)),
        Some(Band::Global(n)) => d
            .gbank
            .is_some_and(|b| b.slots.get(n as usize - 1).is_some_and(|s| s.uni)),
        _ => false,
    };
    source_center(slot, uni)
}

/// Les anneaux d'un parametre : la fenetre de chaque source, ou celle du
/// pad.
pub(crate) fn rings_of(d: &Data, th: &Theme, ps: &PState, standby: bool) -> Vec<Ring> {
    let g = d.t.pad.gesture_range;
    let mut out = Vec::new();
    if ps.srcs.is_empty() {
        if ps.sel && (ps.x || ps.y) {
            let c = match (ps.x, ps.y) {
                (true, true) => (d.t.pad.ax + d.t.pad.ay) / 2.0,
                (true, false) => d.t.pad.ax,
                _ => d.t.pad.ay,
            };
            let (lo, hi) = mod_window(ps.base, ps.span(0, g), c);
            out.push(Ring {
                lo,
                hi,
                color: th.accent,
                thin: false,
                dim: false,
            });
        }
        return out;
    }
    let many = ps.srcs.len() > 1;
    for (i, s) in ps.srcs.iter().enumerate() {
        let c = link_center(d, s.slot, s.pol);
        let (lo, hi) = mod_window(ps.base, ps.span(i, g), c);
        out.push(Ring {
            lo,
            hi,
            color: src_color(th, s.slot),
            thin: many,
            dim: s.off,
        });
    }
    let _ = standby;
    out
}

fn param_tip(d: &Data, p: &crate::ParamRow, ps: &PState, standby: bool) -> String {
    let mut t = p.name.clone();
    if ps.srcs.is_empty() {
        if ps.sel {
            t.push_str("\n(pad)");
        } else {
            t.push_str("\n(not tracked)");
        }
    } else {
        for (i, s) in ps.srcs.iter().enumerate() {
            let dep = ps.span(i, d.t.pad.gesture_range) * 100.0;
            t.push_str(&format!(
                "\n{}  {:+.0} %{}",
                long_label(s.slot),
                dep,
                if s.off { " (bypassed)" } else { "" }
            ));
        }
        if standby {
            t.push_str("\npad axis standing by - no room left on a sum bus");
        }
    }
    if ps.inv {
        t.push_str("\n[INVERTED]");
    }
    t.push_str("\nDrag: base · Alt+drag: depth · Shift: fine\nAlt+click: release the sources · Shift+click: bypass them");
    t
}

fn links_tab(b: &mut B, _v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    let mut y = r.y;
    let lh = b.s(22.0);
    let names: std::collections::BTreeMap<&str, (&str, &str)> =
        d.fx.iter()
            .flat_map(|f| {
                f.params
                    .iter()
                    .map(move |p| (p.key.as_str(), (p.name.as_str(), f.name.as_str())))
            })
            .collect();
    let mut n = 0;
    for (k, ps) in
        d.t.params
            .iter()
            .filter(|(_, p)| p.sel && !p.srcs.is_empty())
    {
        let (pn, fxn) = names.get(k.as_str()).copied().unwrap_or(("?", "?"));
        b.text(
            R::new(r.x, y, r.w, lh),
            &format!("{pn} · {fxn}"),
            F::Bold,
            th.text,
            Align::Left,
        );
        y += lh;
        for (i, s) in ps.srcs.iter().enumerate() {
            let c = src_color(th, s.slot);
            let ds = b.s(9.0);
            b.push(
                R::new(r.x + b.s(4.0), y + (lh - ds) / 2.0, ds, ds),
                Ctl::None,
                Look::Dot {
                    color: c,
                    hollow: s.off,
                },
                "",
            );
            b.text(
                R::new(r.x + b.s(18.0), y, r.w * 0.33, lh),
                &long_label(s.slot),
                F::Small,
                th.text_dim,
                Align::Left,
            );
            let fx = r.x + b.s(18.0) + r.w * 0.33;
            let fw = r.w - (fx - r.x) - lh - b.s(4.0);
            let dep = ps.span(i, 1.0);
            b.push(
                R::new(fx, y + b.s(5.0), fw, lh - b.s(10.0)),
                Ctl::LinkDepth(k.clone(), i),
                Look::Fader {
                    v: (dep + 1.0) / 2.0,
                    label: format!("{:+.0} %", dep * 100.0),
                    enabled: true,
                },
                "Depth (double-click: zero)",
            );
            b.button(
                R::new(r.right() - lh, y, lh, lh),
                Ctl::LinkRemove(k.clone(), i),
                "",
                Some(Icon::X),
                false,
                "Remove this link",
            );
            y += lh;
            // Ce qui module la profondeur de ce lien.
            for (j, dm) in s.dmods.iter().enumerate() {
                let c = src_color(th, dm.slot);
                let ds = b.s(7.0);
                let ix = r.x + b.s(22.0);
                b.push(
                    R::new(ix, y + (lh - ds) / 2.0, ds, ds),
                    Ctl::None,
                    Look::Dot {
                        color: c,
                        hollow: dm.off,
                    },
                    "",
                );
                b.text(
                    R::new(ix + b.s(12.0), y, r.w * 0.33 - b.s(16.0), lh),
                    &format!("↳ {}", long_label(dm.slot)),
                    F::Small,
                    th.text_faint,
                    Align::Left,
                );
                b.push(
                    R::new(fx, y + b.s(6.0), fw, lh - b.s(12.0)),
                    Ctl::DmodDepth(k.clone(), i, j),
                    Look::Fader {
                        v: (dm.depth + 1.0) / 2.0,
                        label: format!("{:+.0} %", dm.depth * 100.0),
                        enabled: true,
                    },
                    "How much this source moves the depth above (double-click: zero)",
                );
                b.button(
                    R::new(r.right() - lh, y, lh, lh),
                    Ctl::DmodRemove(k.clone(), i, j),
                    "",
                    Some(Icon::X),
                    false,
                    "Remove this link on the link",
                );
                y += lh;
            }
        }
        y += b.s(6.0);
        n += 1;
    }
    if n == 0 {
        b.text(
            R::new(r.x, y, r.w, lh),
            "no link on this track",
            F::Small,
            th.text_faint,
            Align::Left,
        );
        y += lh;
    }
    y - r.y
}

// ---------------------------------------------------------------------------
// Le rack des sources
// ---------------------------------------------------------------------------

fn rack(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    if v.rack_fold {
        b.button(
            R::new(r.x, r.y, r.w, r.h),
            Ctl::Fold,
            "",
            Some(Icon::ChevronLeft),
            false,
            "Show the sources",
        );
        return 0.0;
    }
    let bh = b.s(30.0);
    let sq = bh;
    b.seg_ic(
        R::new(r.x, r.y, r.w - 2.0 * sq - b.s(10.0), bh),
        &[
            (Some(Icon::List), "Track"),
            (Some(Icon::Globe), "Global"),
            (Some(Icon::Link), "Links"),
        ],
        v.rack_tab as usize,
        |i| Ctl::RackTab(i as u8),
        &[
            "Sources of this track",
            "The global bank: a hidden CP MOD track, reaches any track by 14-bit CC",
            "Every link of this track",
        ],
    );
    b.icon_btn(
        R::new(r.right() - 2.0 * sq - b.s(2.0), r.y, sq, sq),
        Ctl::Map,
        Icon::Magnet,
        v.map,
        "Map: dragging any knob sets the selected source's depth (Esc leaves)",
    );
    b.icon_btn(
        R::new(r.right() - sq, r.y, sq, sq),
        Ctl::Fold,
        Icon::ChevronRight,
        false,
        "Fold the sources",
    );
    let body = R::new(
        r.x,
        r.y + bh + b.s(8.0),
        r.w,
        (r.h - bh - b.s(8.0)).max(0.0),
    );
    b.push(body, Ctl::None, Look::Clip(1), "");
    b.clip = Some(1);
    b.dy = v.scroll[1];
    let h = match v.rack_tab {
        2 => {
            let card = b.open_card(body.x, body.y, body.w);
            let cp = pad_of(b);
            let end = links_tab(
                b,
                v,
                d,
                R::new(body.x + cp, body.y + cp, body.w - 2.0 * cp, 0.0),
            );
            let bottom = body.y + cp + end + cp;
            b.close(card, bottom);
            bottom - body.y
        }
        t => sources_tab(b, v, d, body, t == 1),
    };
    b.clip = None;
    b.dy = 0.0;
    let _ = th;
    h
}

/// Un slot eteint, quand sa banque n'est pas encore posee (les tuiles sont
/// la quand meme : le premier usage pose la banque).
fn idle_slot() -> crate::Slot {
    crate::Slot {
        rate_hz: 1.0,
        rate_n: 0.3,
        len: 16,
        ..Default::default()
    }
}

fn sources_tab(b: &mut B, v: &View, d: &Data, r: R, global: bool) -> f32 {
    let th = b.th;
    let cp = pad_of(b);
    let gap = gap_of(b);
    let mut y = r.y;
    let iw = r.w - 2.0 * cp;
    let ix = r.x + cp;
    let tg = b.s(5.0);
    let cols = (((iw + tg) / (b.s(70.0) + tg)).floor() as usize).clamp(2, 4);
    let tw0 = ((iw - tg * (cols - 1) as f32) / cols as f32).floor();
    let th0 = b.s(46.0);
    let tile = |b: &mut B,
                i: usize,
                y0: f32,
                slot: i32,
                title: String,
                on: bool,
                shape: Option<(u8, f64)>,
                phase: Option<f64>,
                tip: &str| {
        let x = ix + (tw0 + tg) * (i % cols) as f32;
        let yy = y0 + (th0 + tg) * (i / cols) as f32;
        let c = src_color(b.th, slot);
        b.push(
            R::new(x, yy, tw0, th0),
            Ctl::Tile(slot),
            Look::Tile {
                title,
                color: c,
                on,
                count: d.t.dest_count(slot),
                shape,
                phase,
                sel: v.sel_slot == slot,
                hl: v.hover == Ctl::Tile(slot) || v.hover == Ctl::TileDot(slot),
            },
            tip,
        );
        let ds = b.s(10.0);
        b.push(
            R::new(x + b.s(6.0), yy + b.s(7.0), ds, ds),
            Ctl::TileDot(slot),
            Look::Dot {
                color: c,
                hollow: !on,
            },
            "Drag onto a knob to modulate it",
        );
    };
    let card = b.open_card(r.x, y, r.w);
    y += cp;
    let mut i = 0;
    if !global {
        // Les sources de la piste : enveloppes, sequenceur, notes.
        let sg_on = d.sg.is_some_and(|j| j.enabled);
        for n in 1..=4 {
            tile(
                b,
                i,
                y,
                200 + n,
                format!("ENV {n}"),
                sg_on,
                None,
                None,
                if sg_on {
                    "A Sound Gen envelope · drag the dot onto a knob"
                } else {
                    "The Sound Gen is off on this track"
                },
            );
            i += 1;
        }
        for n in 1..=2 {
            tile(
                b,
                i,
                y,
                220 + n,
                if n == 1 {
                    "SEQ A".into()
                } else {
                    "SEQ B".into()
                },
                d.seq.is_some(),
                None,
                None,
                if d.seq.is_some() {
                    "A sequencer modulation lane · drag the dot onto a knob"
                } else {
                    "No sequencer on this track"
                },
            );
            i += 1;
        }
        for (n, name) in ["NOTE", "VEL", "RAND", "OCT"].iter().enumerate() {
            tile(
                b,
                i,
                y,
                231 + n as i32,
                name.to_string(),
                true,
                None,
                None,
                "A note source: held from one note to the next",
            );
            i += 1;
        }
        // Les LFO commencent une rangee.
        i = i.div_ceil(cols) * cols;
    }
    let bank = if global { d.gbank } else { d.bank };
    let base = if global { 100 } else { 0 };
    let idle = idle_slot();
    for k in 0..8usize {
        let s = bank.and_then(|bk| bk.slots.get(k)).unwrap_or(&idle);
        let slot = base + k as i32 + 1;
        let ty = slot_type_name(s.shape);
        tile(
            b,
            i,
            y,
            slot,
            format!("{ty} {}", k + 1),
            s.on,
            Some((s.shape, s.curve)),
            s.on.then_some(s.pho),
            if s.on {
                "Click: edit · drag the dot onto a knob"
            } else {
                "Off · click: edit · drag the dot onto a knob to start it"
            },
        );
        i += 1;
    }
    y += (th0 + tg) * i.div_ceil(cols) as f32 - tg + cp;
    b.close(card, y);
    y += gap;
    // L'enveloppe choisie (ENV 1..4) : son editeur.
    if !global {
        if let (Some(Band::Env(n)), Some(j)) = (band_of(v.sel_slot), d.sg.filter(|j| j.enabled)) {
            let card = b.open_card(r.x, y, r.w);
            y = crate::panels::env_editor(b, j, n, R::new(ix, y + cp, iw, 0.0)) + cp;
            b.close(card, y);
            y += gap;
        }
    }
    // Le slot choisi.
    let sel = v.sel_slot - base;
    if (1..=8).contains(&sel) {
        let s = bank
            .and_then(|bk| bk.slots.get(sel as usize - 1))
            .unwrap_or(&idle);
        let ext = bank.is_none_or(|bk| bk.ext);
        let card = b.open_card(r.x, y, r.w);
        y = slot_editor(b, v, d, s, ext, R::new(ix, y + cp, iw, 0.0), base + sel) + cp;
        b.close(card, y);
        y += gap;
    }
    let _ = th;
    y - r.y
}

/// Le nom d'un slot d'apres sa forme.
pub fn slot_type_name(shape: u8) -> &'static str {
    match shape {
        0..=4 => "LFO",
        5 | 6 => "S&H",
        _ => "STEP",
    }
}

/// L'icone d'une forme de slot.
pub fn shape_icon(k: u8) -> Icon {
    match k {
        0 => Icon::ShapeSine,
        1 => Icon::ShapeTriangle,
        2 => Icon::ShapeSawUp,
        3 => Icon::ShapeSawDown,
        4 => Icon::ShapeSquare,
        5 => Icon::ShapeRandom,
        6 => Icon::ShapeHold,
        _ => Icon::ShapeSteps,
    }
}

fn slot_editor(b: &mut B, v: &View, d: &Data, s: &crate::Slot, ext: bool, r: R, slot: i32) -> f32 {
    let th = b.th;
    let bh = b.s(26.0);
    let gap = b.s(6.0);
    let c = src_color(th, slot);
    let mut y = r.y;
    // L'en-tete : la couleur, le nom, l'interrupteur.
    let ds = b.s(9.0);
    b.push(
        R::new(r.x, y + (bh - ds) / 2.0, ds, ds),
        Ctl::None,
        Look::Dot {
            color: c,
            hollow: !s.on,
        },
        "",
    );
    b.text(
        R::new(r.x + b.s(16.0), y, r.w - bh - b.s(20.0), bh),
        &long_label(slot),
        F::Bold,
        th.text,
        Align::Left,
    );
    b.icon_chip(
        R::new(r.right() - bh, y, bh, bh),
        Ctl::SlotOn,
        Icon::Power,
        s.on,
        "Turn the slot on or off",
    );
    y += bh + gap;
    // La forme en grand, et ou elle en est.
    let wh = b.s(64.0);
    let n = 96;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            (
                t as f32,
                crate::draw::shape_value(s.shape, s.curve, t * 2.0) as f32,
            )
        })
        .collect();
    let phase = s.on.then(|| {
        let t = s.pho.fract();
        (
            (t * 0.5) as f32,
            crate::draw::shape_value(s.shape, s.curve, t) as f32,
        )
    });
    b.push(
        R::new(r.x, y, r.w, wh),
        Ctl::None,
        Look::Wave {
            pts,
            color: c,
            phase,
        },
        "",
    );
    y += wh + gap;
    // Les formes.
    let sg = b.s(4.0);
    let sw = ((r.w - 7.0 * sg) / 8.0).floor();
    for k in 0..8u8 {
        b.push(
            R::new(r.x + (sw + sg) * k as f32, y, sw, bh),
            Ctl::ShapePick(k),
            Look::IconChip {
                icon: shape_icon(k),
                on: s.shape == k,
                enabled: true,
                color: Some(c),
            },
            bank::SHAPES[k as usize],
        );
    }
    y += bh + gap;
    // Polarite, tempo, cycle, relance, hasard.
    let pw = b.s(60.0);
    b.seg(
        R::new(r.x, y, pw, bh),
        &["±", "+"],
        s.uni as usize,
        |i| Ctl::SlotPol(i == 1),
        &[
            "Bipolar: swings around the base",
            "Unipolar: only above the base",
        ],
    );
    let mut x = r.x + pw + gap;
    let syw = b.s(70.0);
    b.push(
        R::new(x, y, syw, bh),
        Ctl::SlotSync,
        Look::Button {
            label: bank::SYNCS[(s.sync as usize).min(10)].to_string(),
            icon: Some(if s.sync > 0 { Icon::Music } else { Icon::Clock }),
            on: s.sync > 0,
            enabled: true,
            tone: Tone::Normal,
        },
        "Free rate or synced to the tempo",
    );
    x += syw + gap;
    b.push(
        R::new(x, y, bh, bh),
        Ctl::SlotMode,
        Look::Button {
            label: String::new(),
            icon: Some(match s.mode {
                1 => Icon::ArrowRightToLine,
                2 => Icon::Refresh,
                _ => Icon::Repeat,
            }),
            on: s.mode > 0,
            enabled: ext,
            tone: Tone::Normal,
        },
        &format!(
            "{} · click: loop, one-shot, or loop restarted with the transport",
            bank::MODES[(s.mode as usize).min(2)]
        ),
    );
    let rx = r.right() - bh;
    b.icon_btn(
        R::new(rx, y, bh, bh),
        Ctl::SlotRnd,
        Icon::Dices,
        false,
        "Random settings for this slot",
    );
    b.button_ex(
        R::new(rx - bh - b.s(2.0), y, bh, bh),
        Ctl::SlotRetrig,
        "",
        Some(Icon::Refresh),
        false,
        ext,
        Tone::Normal,
        "Restart the cycle now",
    );
    y += bh + gap;
    if (5..=6).contains(&s.shape) {
        let src = if s.sh == 0 {
            "Noise".to_string()
        } else {
            format!("LFO {}", s.sh)
        };
        b.chip(
            R::new(r.x, y, r.w, bh),
            Ctl::SlotSh,
            &format!("Sample: {src}"),
            true,
            true,
            "What S&H samples",
        );
        y += bh + gap;
    }
    if s.shape == 7 {
        let sh = b.s(56.0);
        let lb = b.s(8.0);
        b.push(
            R::new(r.x, y, r.w, lb),
            Ctl::StepLen,
            Look::Fader {
                v: (s.len as f64 - 1.0) / 15.0,
                label: String::new(),
                enabled: true,
            },
            &format!("Length: {} steps", s.len),
        );
        y += lb + b.s(2.0);
        b.push(
            R::new(r.x, y, r.w, sh),
            Ctl::Steps,
            Look::Steps {
                n: 16,
                vals: s.steps.iter().map(|&v| v as f64 / 63.0).collect(),
                len: s.len,
                play: s.on.then(|| ((s.pho * s.len.max(1) as f64) as u8).min(15)),
                origin: 0.0,
                color: c,
                bools: false,
            },
            "Paint the steps",
        );
        y += sh + gap;
        b.icon_btn(
            R::new(r.x, y, bh, bh),
            Ctl::StepRnd,
            Icon::Dices,
            false,
            "New values for the steps that play",
        );
        y += bh + gap;
    }
    // Les quatre boutons.
    let kw = (r.w / 4.0).floor();
    let kh = b.s(58.0);
    let rate_label = if s.sync > 0 {
        bank::SYNCS[(s.sync as usize).min(10)].to_string()
    } else {
        format!("{:.2} Hz", s.rate_hz)
    };
    let knobs: [(u8, f64, &str, String, bool, bool); 4] = [
        (0, s.rate_n, "RATE", rate_label, s.sync == 0, false),
        (1, s.phase, "PHASE", format!("{:.2}", s.phase), true, false),
        (
            2,
            (s.curve + 1.0) / 2.0,
            if s.shape == 4 { "WIDTH" } else { "CURVE" },
            format!("{:+.2}", s.curve),
            ext,
            true,
        ),
        (
            3,
            s.slew_n,
            "SLEW",
            if s.slew_s < 0.001 {
                "Off".into()
            } else {
                format!("{:.0} ms", s.slew_s * 1000.0)
            },
            ext,
            false,
        ),
    ];
    for (i, (k, val, label, txt, en, bip)) in knobs.into_iter().enumerate() {
        b.knob(
            R::new(r.x + kw * i as f32, y, kw, kh),
            Ctl::SlotKnob(k),
            val,
            label,
            &txt,
            en,
            bip,
            "Drop a source here to modulate it",
        );
    }
    y += kh + gap;
    // Ce que ce slot mene, et de combien.
    let names: std::collections::BTreeMap<&str, &str> =
        d.fx.iter()
            .flat_map(|f| f.params.iter().map(|p| (p.key.as_str(), p.name.as_str())))
            .collect();
    let lh = b.s(24.0);
    for (k, ps) in d.t.params.iter().filter(|(_, p)| p.sel) {
        let Some(i) = ps.index_of(slot) else {
            continue;
        };
        let name = names.get(k.as_str()).copied().unwrap_or("?");
        b.glyph(R::new(r.x, y, b.s(14.0), lh), Icon::Link, c);
        let fw = (r.w * 0.42).floor();
        b.text(
            R::new(r.x + b.s(20.0), y, r.w - fw - b.s(26.0), lh),
            name,
            F::Small,
            th.text,
            Align::Left,
        );
        let dep = ps.span(i, 1.0);
        b.push(
            R::new(r.right() - fw, y + b.s(6.0), fw, lh - b.s(12.0)),
            Ctl::LinkDepth(k.clone(), i),
            Look::Fader {
                v: (dep + 1.0) / 2.0,
                label: format!("{:+.0} %", dep * 100.0),
                enabled: true,
            },
            "Depth (double-click: zero)",
        );
        y += lh;
    }
    let _ = v;
    y
}
