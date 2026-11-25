//! Les elements d'une image : ou est chaque controle, et de quoi il a l'air.

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
    Bar,
    Zone,
    Card,
    Sunk,
    Head,
    /// Une teinte donnee (une rangee menee par la source survolee).
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
    /// Un choix segmente : une part par etiquette.
    pub fn seg(
        &mut self,
        r: R,
        labels: &[&str],
        sel: usize,
        ctl: impl Fn(usize) -> Ctl,
        tips: &[&str],
    ) {
        let n = labels.len().max(1) as f32;
        let pw = (r.w / n).floor();
        for (i, l) in labels.iter().enumerate() {
            let x = r.x + pw * i as f32;
            let w = if i + 1 == labels.len() {
                r.right() - x
            } else {
                pw
            };
            self.push(
                R::new(x, r.y, w, r.h),
                ctl(i),
                Look::Seg {
                    label: l.to_string(),
                    sel: i == sel,
                    first: i == 0,
                    last: i + 1 == labels.len(),
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
            },
            tip,
        );
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
    let row_h = b.s(32.0);
    let two = w < b.s(1240.0);
    let bar_h = if two { 2.0 * row_h } else { row_h };
    let info_h = b.s(24.0);
    command_bar(&mut b, v, d, w, row_h, two);
    info_bar(&mut b, v, d, w, h - info_h, info_h);
    let Some(_) = d.track else {
        let r = R::new(0.0, bar_h, w, h - bar_h - info_h);
        b.text(
            R::new(r.x, r.cy() - b.s(14.0), r.w, b.s(20.0)),
            "No track selected.",
            F::Bold,
            th.text,
            Align::Center,
        );
        return (b.items, content);
    };
    let gap = b.s(6.0);
    let body = R::new(0.0, bar_h, w, (h - bar_h - info_h).max(0.0));
    let lw = b.s(v.left_w).min((w * 0.4).max(b.s(160.0)));
    let rw = if v.rack_fold {
        b.s(18.0)
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
    (b.items, content)
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
    let s = th.scale;
    let ch = bh - 2.0 * b.s(5.0);
    let mut y = b.s(5.0);
    let mut x = b.s(8.0);
    let narrow = w < b.s(1000.0);
    let title = if narrow { "" } else { "FX Constellation" };
    b.text(
        R::new(x, 0.0, tw(th, title), bh),
        title,
        F::Bold,
        th.text,
        Align::Left,
    );
    x += tw(th, title);
    let tn = d.track.map_or("· no track".to_string(), |t| {
        if t.is_empty() {
            "· —".into()
        } else {
            format!("· {t}")
        }
    });
    let tnw = tw(th, &tn).min(b.s(180.0));
    b.text(
        R::new(x, 0.0, tnw, bh),
        &tn,
        F::Normal,
        th.text_dim,
        Align::Left,
    );
    x += tnw + b.s(8.0);
    // Le preset.
    let sq = ch;
    b.button(
        R::new(x, y, sq, ch),
        Ctl::PresetPrev,
        "",
        Some(Icon::ChevronLeft),
        false,
        "Previous preset",
    );
    x += sq;
    let pw = b.s(150.0);
    b.push(
        R::new(x, y, pw, ch),
        Ctl::PresetField,
        Look::Field {
            text: if d.preset.is_empty() {
                "no preset".into()
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
    x += pw;
    b.button(
        R::new(x, y, sq, ch),
        Ctl::PresetNext,
        "",
        Some(Icon::ChevronRight),
        false,
        "Next preset",
    );
    x += sq + b.s(2.0);
    b.button(
        R::new(x, y, sq, ch),
        Ctl::PresetSave,
        "",
        Some(Icon::Save),
        false,
        "Save the preset",
    );
    x += sq + b.s(10.0);
    // Le snapshot.
    b.button(
        R::new(x, y, sq, ch),
        Ctl::SnapPrev,
        "",
        Some(Icon::ChevronLeft),
        false,
        "Previous snapshot",
    );
    x += sq;
    let sw = b.s(112.0);
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
    x += sw;
    b.button(
        R::new(x, y, sq, ch),
        Ctl::SnapNext,
        "",
        Some(Icon::ChevronRight),
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
    x += sq + b.s(10.0);
    let lw = b.s(64.0);
    b.button_ex(
        R::new(x, y, lw, ch),
        Ctl::Lock,
        if d.locked { "Locked" } else { "Lock" },
        Some(if d.locked { Icon::Lock } else { Icon::LockOpen }),
        d.locked,
        true,
        Tone::Normal,
        "Lock: stay on this track whatever REAPER selects",
    );
    x += lw + b.s(4.0);
    let cw = b.s(78.0);
    b.button_ex(
        R::new(x, y, cw, ch),
        Ctl::Capture,
        &if d.capture > 0 {
            format!("Capture {}", d.capture)
        } else {
            "Capture".to_string()
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
    x += cw + b.s(10.0);
    if two {
        x = b.s(8.0);
        y += bh;
    }
    let gw = b.s(140.0);
    let gran = d.t.pad.granular;
    b.seg(
        R::new(x, y, gw, ch),
        &["Single", "Granular"],
        gran as usize,
        |i| Ctl::Granular(i == 1),
        &[
            "Single: the pad moves each param from its base",
            "Granular: the pad blends random grain points",
        ],
    );
    x += gw + b.s(8.0);
    let nw = b.s(190.0);
    b.seg(
        R::new(x, y, nw, ch),
        &["Manual", "Walk", "Figures"],
        d.t.pad.nav as usize,
        |i| Ctl::Nav(i as u8),
        &[
            "Manual: the mouse moves the pad",
            "Walk: the pad wanders on its own (click again to restart it)",
            "Figures: the pad draws a figure (click again to restart it)",
        ],
    );
    x += nw + b.s(8.0);
    if let Some(m) = d.morph {
        let mw = b.s(110.0);
        b.push(
            R::new(x, y, mw, ch),
            Ctl::Morph,
            Look::Fader {
                v: m,
                label: "A → B".into(),
                enabled: true,
            },
            "Morph between the A and B captures",
        );
    }
    // A droite : l'aide et les reglages.
    let rx = w - b.s(8.0) - 2.0 * sq - b.s(2.0);
    b.button(
        R::new(rx, y, sq, ch),
        Ctl::Settings,
        "",
        Some(Icon::Settings),
        false,
        "Settings",
    );
    b.button(
        R::new(rx + sq + b.s(2.0), y, sq, ch),
        Ctl::Help,
        "",
        Some(Icon::Help),
        false,
        "Help",
    );
    let _ = (v, s);
}

// ---------------------------------------------------------------------------
// La barre d'etat
// ---------------------------------------------------------------------------

fn info_bar(b: &mut B, v: &View, d: &Data, w: f32, y: f32, ih: f32) {
    let th = b.th;
    b.panel(R::new(0.0, y, w, ih), Shade::Bar);
    let (rw0, _) = v.dock.reserve(w, y + ih, th.scale);
    let x0 = b.s(8.0) + rw0;
    let msg = d.msg.map(|s| s.to_string()).unwrap_or_else(|| {
        format!(
            "{} tracked · {} links · {} modulated",
            d.t.tracked_count(),
            d.links,
            d.modulated
        )
    });
    let col_msg = if d.msg.is_some() {
        th.accent
    } else {
        th.text_dim
    };
    b.text(
        R::new(x0, y, b.s(320.0), ih),
        &msg,
        F::Small,
        col_msg,
        Align::Left,
    );
    let mut x = w - b.s(8.0);
    let ch = ih - b.s(6.0);
    let cy = y + b.s(3.0);
    // Les axes du pad : pastille de reception, cadran, et sa pastille de
    // source.
    for axis in [1u8, 0u8] {
        let slot = if axis == 0 {
            d.t.pad.src_x
        } else {
            d.t.pad.src_y
        };
        let name = if axis == 0 { "X" } else { "Y" };
        let pw = b.s(70.0);
        x -= pw;
        let (label, c) = if slot != 0 {
            (format!("{name} {}", short_label(slot)), src_color(th, slot))
        } else {
            (format!("{name} free"), th.text_dim)
        };
        b.push(
            R::new(x, cy, pw, ch),
            Ctl::Pill(axis),
            Look::Chip {
                label,
                on: slot != 0,
                standby: false,
                amber: false,
                enabled: true,
            },
            &if slot != 0 {
                format!(
                    "Pad {name} is driven by {}\nClick: free it",
                    long_label(slot)
                )
            } else {
                format!("Drop a source here to drive pad {name}")
            },
        );
        let _ = c;
        x -= b.s(4.0);
        let ks = ih - b.s(2.0);
        x -= ks;
        let pv = if axis == 0 { d.t.pad.x } else { d.t.pad.y };
        b.push(
            R::new(x, y + b.s(1.0), ks, ks),
            Ctl::AxisKnob(axis),
            Look::Knob {
                v: pv,
                depth: 0.0,
                bipolar: false,
                enabled: slot == 0,
                rings: Vec::new(),
                live: None,
                label: String::new(),
                value: String::new(),
                hl: None,
                size: ks,
            },
            &format!("Pad {name}: {pv:.2}\nDrag its dot onto a knob to modulate it"),
        );
        let ds = b.s(9.0);
        b.push(
            R::new(x + ks - ds * 0.6, y + ih - ds - b.s(1.0), ds, ds),
            Ctl::AxisDot(axis),
            Look::Dot {
                color: src_color(th, if axis == 0 { -1 } else { -2 }),
                hollow: false,
            },
            &format!("Pad {name}: drag onto a knob to modulate it"),
        );
        x -= b.s(10.0);
    }
    let pl = "PAD ←";
    let plw = tw(th, pl);
    x -= plw;
    b.text(
        R::new(x, y, plw, ih),
        pl,
        F::Small,
        th.text_faint,
        Align::Right,
    );
    x -= b.s(10.0);
    let eng = if d.t.pad.granular {
        "engine: script (granular)"
    } else if d.bridge {
        "engine: native links"
    } else {
        "engine: no bridge"
    };
    let ew = tw(th, eng);
    x -= ew;
    b.text(
        R::new(x, y, ew, ih),
        eng,
        F::Small,
        if d.bridge && !d.t.pad.granular {
            th.accent
        } else {
            th.text_faint
        },
        Align::Right,
    );
    for (ctl, l, tip) in [
        (
            Ctl::StopEngines,
            "Stop all engines",
            "Stop Walk and Figures on every track (links stay)",
        ),
        (
            Ctl::ReleaseLinks,
            "Release all links",
            "Release every FX Constellation link of this track (values stay)",
        ),
    ] {
        let bw = tw(th, l);
        x -= bw + b.s(6.0);
        b.button(R::new(x, cy, bw, ch), ctl, l, None, false, tip);
    }
}

// ---------------------------------------------------------------------------
// La zone de gauche : pad, pied, navigation, ROLL, macros
// ---------------------------------------------------------------------------

fn left_zone(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    b.panel(r, Shade::Zone);
    b.push(r, Ctl::None, Look::Clip(2), "");
    b.clip = Some(2);
    b.dy = v.scroll[2];
    let pad_in = b.s(2.0);
    let x = r.x + pad_in;
    let w = r.w - 2.0 * pad_in;
    let mut y = r.y;
    let p = &d.t.pad;
    // Le pad : un carre.
    let side = w.min((r.h * 0.5).max(b.s(80.0))).max(b.s(80.0));
    let pr = R::new(x + (w - side) / 2.0, y, side, side);
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
        }),
        "Drag: move the pad · right-drag: draw a zone · right-click: clear it",
    );
    y += side + b.s(4.0);
    // Le pied.
    let rh = b.s(20.0);
    let foot = format!("{:.2}, {:.2}", p.x, p.y);
    b.text(
        R::new(x, y, b.s(80.0), rh),
        &foot,
        F::Small,
        th.text_dim,
        Align::Left,
    );
    let anc = format!("anchor {:.2}, {:.2}", p.ax, p.ay);
    b.text(
        R::new(x + b.s(78.0), y, b.s(110.0), rh),
        &anc,
        F::Small,
        th.text_faint,
        Align::Left,
    );
    let rbw = b.s(48.0);
    b.button(
        R::new(x + w - rbw, y, rbw, rh),
        Ctl::PadReset,
        "Reset",
        None,
        false,
        "Pad and anchor back to the center",
    );
    if p.has_zone() {
        b.button(
            R::new(x + w - rbw - b.s(52.0), y, b.s(48.0), rh),
            Ctl::ZoneClear,
            "Zone",
            Some(Icon::X),
            false,
            "Clear the zone. Right-drag on the pad draws a new one, a plain right-click clears it.",
        );
    }
    y += rh + b.s(6.0);
    b.push(R::new(x, y, w, 1.0), Ctl::None, Look::Sep, "");
    y += b.s(6.0);
    // La navigation.
    let kh = b.s(52.0);
    let kw = (w / 4.0).floor();
    let knob =
        |b: &mut B, i: usize, y: f32, f: PadF, v: f64, label: &str, val: String, tip: &str| {
            b.knob(
                R::new(x + kw * i as f32, y, kw, kh),
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
            b.chip(
                R::new(x, y, b.s(56.0), rh),
                Ctl::WalkJump,
                "Jump",
                p.walk_jump,
                true,
                "Jump: teleport to each new point",
            );
            b.chip(
                R::new(x + b.s(60.0), y, b.s(80.0), rh),
                Ctl::WalkSync,
                WALK_SYNCS[p.walk_sync as usize % 6],
                p.walk_sync > 0,
                true,
                "Walk timing: free (Hz) or synced to the tempo",
            );
            y += rh + b.s(4.0);
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
            let cell = b.s(28.0);
            for (i, name) in FIGURES.iter().enumerate() {
                b.chip(
                    R::new(x + (cell + b.s(3.0)) * i as f32, y, cell, cell),
                    Ctl::Figure(i as u8),
                    &name[..1],
                    p.fig_mode == i as u8,
                    true,
                    name,
                );
            }
            y += cell + b.s(4.0);
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
    y += b.s(4.0);
    b.text(
        R::new(x, y, b.s(48.0), rh),
        "Range",
        F::Small,
        th.text_dim,
        Align::Left,
    );
    b.push(
        R::new(x + b.s(50.0), y + b.s(4.0), w - b.s(50.0), rh - b.s(8.0)),
        Ctl::GRange,
        Look::Range {
            lo: p.gmin,
            hi: p.gmax,
        },
        "Range: granular values are drawn between these bounds",
    );
    y += rh + b.s(4.0);
    if p.granular {
        b.chip(
            R::new(x, y, b.s(60.0), rh),
            Ctl::GrainGrid,
            &format!("{0}x{0}", p.grid),
            true,
            true,
            "Grain grid",
        );
        b.button(
            R::new(x + b.s(64.0), y, w - b.s(64.0), rh),
            Ctl::GrainRoll,
            "Randomize grains",
            Some(Icon::Dices),
            false,
            "New values for every grain",
        );
        y += rh + b.s(4.0);
    }
    b.push(R::new(x, y, w, 1.0), Ctl::None, Look::Sep, "");
    y += b.s(6.0);
    // ROLL.
    let rollh = b.s(30.0);
    b.button_ex(
        R::new(x, y, w, rollh),
        Ctl::Roll,
        "ROLL",
        Some(Icon::Dices),
        true,
        true,
        Tone::Accent,
        "Roll every armed axis",
    );
    y += rollh + b.s(4.0);
    let cw = ((w - 7.0 * b.s(2.0)) / 8.0).floor();
    for (i, a) in AXES.iter().enumerate() {
        b.chip(
            R::new(x + (cw + b.s(2.0)) * i as f32, y, cw, rh),
            Ctl::RollAxis(i as u8),
            a.chip(),
            d.roll.armed[i],
            true,
            &format!("{} — click to arm, right-click to roll it now", a.tip()),
        );
    }
    y += rh + b.s(2.0);
    b.text(
        R::new(x, y, w, b.s(16.0)),
        &format!("{} of 8 axes armed", d.roll.armed_count()),
        F::Small,
        th.text_faint,
        Align::Left,
    );
    y += b.s(18.0);
    b.push(R::new(x, y, w, 1.0), Ctl::None, Look::Sep, "");
    y += b.s(6.0);
    // Les macros.
    b.text(
        R::new(x, y, w, b.s(16.0)),
        "MACROS",
        F::Small,
        th.text_dim,
        Align::Left,
    );
    y += b.s(18.0);
    let mw = (w / 4.0).floor();
    let mh = b.s(56.0);
    for n in 1..=8u8 {
        let i = (n - 1) as usize;
        let mx = x + mw * (i % 4) as f32;
        let my = y + mh * (i / 4) as f32;
        let slot = 210 + n as i32;
        let key = format!("m:{n}");
        let mut rings = Vec::new();
        let _ = key;
        let c = src_color(th, slot);
        let hl =
            (v.hover == Ctl::TileDot(slot) || v.carry.is_some_and(|c| c.0 == slot)).then_some(c);
        rings.clear();
        b.push(
            R::new(mx, my, mw, mh - b.s(4.0)),
            Ctl::Macro(n),
            Look::Knob {
                v: d.macros[i],
                depth: 0.0,
                bipolar: false,
                enabled: d.bridge,
                rings,
                live: None,
                label: d.t.macro_name(n as usize),
                value: format!("{:.0} %", d.macros[i] * 100.0),
                hl,
                size: mh - b.s(18.0),
            },
            &format!(
                "{}  {:.0} %\nRight-click: rename · drag the dot onto a knob to modulate it",
                d.t.macro_name(n as usize),
                d.macros[i] * 100.0
            ),
        );
        let ds = b.s(9.0);
        b.push(
            R::new(mx + mw - ds - b.s(6.0), my + b.s(2.0), ds, ds),
            Ctl::MacroDot(n),
            Look::Dot {
                color: c,
                hollow: false,
            },
            "Drag onto a knob to modulate it",
        );
    }
    y += 2.0 * mh;
    if !d.bridge {
        b.text(
            R::new(x, y, w, b.s(16.0)),
            "the macros live on the bridge",
            F::Small,
            th.text_faint,
            Align::Left,
        );
        y += b.s(18.0);
    }
    b.clip = None;
    b.dy = 0.0;
    y - r.y
}

// ---------------------------------------------------------------------------
// Le centre : les onglets, les parametres
// ---------------------------------------------------------------------------

fn center_zone(b: &mut B, v: &View, d: &Data, r: R, content: &mut [f32; 5]) -> f32 {
    let th = b.th;
    let tab_h = b.s(26.0);
    let tabs = ["Parameters", "Sound Gen", "Seq"];
    let mut x = r.x;
    for (i, t) in tabs.iter().enumerate() {
        let tw0 = tw(th, t) + b.s(8.0);
        b.push(
            R::new(x, r.y, tw0, tab_h),
            Ctl::Tab(i as u8),
            Look::Seg {
                label: t.to_string(),
                sel: v.tab == i as u8,
                first: i == 0,
                last: i == 2,
            },
            "",
        );
        x += tw0;
    }
    // Le clavier, en bas.
    let kb_bar = b.s(22.0);
    let kb_keys = if v.kb_open { b.s(56.0) } else { 0.0 };
    let kb = R::new(r.x, r.bottom() - kb_bar - kb_keys, r.w, kb_bar + kb_keys);
    keyboard(b, v, d, kb);
    let body = R::new(
        r.x,
        r.y + tab_h + b.s(4.0),
        r.w,
        (kb.y - r.y - tab_h - b.s(8.0)).max(0.0),
    );
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
    let bh = b.s(22.0);
    b.button(
        R::new(r.x, r.y, b.s(110.0), bh),
        Ctl::KbFold,
        "KEYBOARD",
        Some(if v.kb_open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        }),
        v.kb_open,
        "Play the Sound Gen with the mouse",
    );
    if let Some(n) = d.kb_note {
        b.text(
            R::new(r.x + b.s(116.0), r.y, b.s(120.0), bh),
            &note_name(n),
            F::Small,
            th.accent,
            Align::Left,
        );
    }
    if !v.kb_open {
        return;
    }
    // Trois octaves depuis C3, et le do du haut.
    let ky = r.y + bh;
    let kh = r.h - bh;
    let whites: Vec<u8> = (48u8..=84).filter(|n| !is_black(*n)).collect();
    let ww = (r.w / whites.len() as f32).floor();
    for (i, n) in whites.iter().enumerate() {
        b.push(
            R::new(r.x + ww * i as f32, ky, ww - 1.0, kh),
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
        let bx = r.x + ww * (below + 1) as f32 - ww * 0.3;
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

fn params_tab(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    let bh = b.s(22.0);
    // La barre d'outils.
    let mut x = r.x;
    for (ctl, l, on, tip) in [
        (
            Ctl::CollapseAll,
            "Collapse",
            false,
            "Fold or unfold every FX",
        ),
        (Ctl::UntrackAll, "Untrack all", false, "Untrack every param"),
        (Ctl::Values, "Values", v.show_values, "Show the values"),
    ] {
        let w0 = tw(th, l);
        b.button(R::new(x, r.y, w0, bh), ctl, l, None, on, tip);
        x += w0 + b.s(4.0);
    }
    let list = R::new(
        r.x,
        r.y + bh + b.s(4.0),
        r.w,
        (r.h - bh - b.s(4.0)).max(0.0),
    );
    b.panel(list, Shade::Zone);
    b.push(list, Ctl::None, Look::Clip(0), "");
    b.clip = Some(0);
    b.dy = v.scroll[0];
    let gap = b.s(8.0);
    let lw = ((list.w - gap) * 0.62).floor();
    let pools = [
        (false, R::new(list.x, list.y, lw, list.h)),
        (
            true,
            R::new(list.x + lw + gap, list.y, list.w - lw - gap, list.h),
        ),
    ];
    let mut tallest = 0.0f32;
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
            R::new(pr.x, y, pr.w - bh, bh),
            Ctl::None,
            Look::Text {
                s: format!("{label}  {n} FX"),
                font: F::Bold,
                color: if detached { th.text_dim } else { th.text },
                align: Align::Left,
            },
            tip,
        );
        b.button(
            R::new(pr.right() - bh, y, bh, bh),
            Ctl::AddFx(detached),
            "",
            Some(Icon::Plus),
            false,
            if detached {
                "Add an FX out of ROLL"
            } else {
                "Add an FX to ROLL"
            },
        );
        y += bh + b.s(4.0);
        if n == 0 {
            b.text(
                R::new(pr.x, y, pr.w, bh),
                if detached {
                    "Drag an FX header here"
                } else {
                    "No FX in the roll"
                },
                F::Small,
                th.text_faint,
                Align::Left,
            );
            y += bh;
        }
        for f in d.fx.iter().filter(|f| f.detached == detached) {
            y = fx_group(b, v, d, f, R::new(pr.x, y, pr.w, 0.0), list);
            y += b.s(6.0);
        }
        tallest = tallest.max(y - pr.y);
    }
    b.clip = None;
    b.dy = 0.0;
    tallest
}

fn fx_group(b: &mut B, v: &View, d: &Data, f: &crate::FxRow, r: R, list: R) -> f32 {
    let th = b.th;
    let hh = b.s(22.0);
    let mut y = r.y;
    let folded = v.folded.contains(&f.guid);
    b.push(
        R::new(r.x, y, r.w, hh),
        Ctl::FxHead(f.guid.clone()),
        Look::Panel(Shade::Head),
        "",
    );
    let tracked = f
        .params
        .iter()
        .filter(|p| d.t.params.get(&p.key).is_some_and(|s| s.sel))
        .count();
    let ic = b.s(20.0);
    let verbs = 6.0;
    let name_w = (r.w - verbs * ic - b.s(70.0)).max(b.s(40.0));
    b.push(
        R::new(r.x, y, b.s(18.0) + name_w, hh),
        Ctl::FxHead(f.guid.clone()),
        Look::Text {
            s: format!("{} {}", if folded { "▶" } else { "▼" }, f.name),
            font: F::Bold,
            color: if f.enabled { th.text } else { th.text_faint },
            align: Align::Left,
        },
        &format!(
            "{}\nClick: fold · drag to the other column: in or out of ROLL",
            f.name
        ),
    );
    b.text(
        R::new(r.x + b.s(18.0) + name_w, y, b.s(60.0), hh),
        &format!("{tracked} / {}", f.params.len()),
        F::Small,
        th.text_faint,
        Align::Left,
    );
    let tips = [
        "Open the plugin window",
        "Bypass this FX",
        "Roll X / Y on the tracked params",
        "Roll ranges on the tracked params",
        "Roll bases on the tracked params",
        "Everything this FX can do",
    ];
    for k in 0..6u8 {
        let ix = r.right() - ic * (6 - k) as f32;
        b.push(
            R::new(ix, y + (hh - ic) / 2.0, ic, ic),
            Ctl::FxVerb(f.guid.clone(), k),
            Look::Button {
                label: String::new(),
                icon: Some(crate::verb_icon(k)),
                on: false,
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
    y += hh + b.s(2.0);
    if folded {
        return y;
    }
    let only = d.t.fx_tracked_only.get(&f.guid).copied().unwrap_or(false);
    let rows: Vec<&crate::ParamRow> = f
        .params
        .iter()
        .filter(|p| !only || d.t.params.get(&p.key).is_some_and(|s| s.sel))
        .collect();
    let row_h = b.s(38.0);
    let two = r.w >= b.s(2.0 * 12.0 * 22.0);
    let cols = if two { 2 } else { 1 };
    let cw = ((r.w - b.s(4.0) * (cols - 1) as f32) / cols as f32).floor();
    let n_rows = rows.len().div_ceil(cols);
    for (i, p) in rows.iter().enumerate() {
        let row = i / cols;
        let c = i % cols;
        let rr = R::new(
            r.x + (cw + b.s(4.0)) * c as f32,
            y + row_h * row as f32,
            cw,
            row_h - b.s(2.0),
        );
        // Seules les rangees visibles se posent.
        let vis_y = rr.y - b.dy;
        if vis_y + rr.h < list.y || vis_y > list.bottom() {
            continue;
        }
        param_row(b, v, d, p, rr);
    }
    y += row_h * n_rows as f32;
    if rows.is_empty() {
        b.text(
            R::new(r.x, y, r.w, b.s(18.0)),
            "nothing tracked — the … menu shows every param",
            F::Small,
            th.text_faint,
            Align::Left,
        );
        y += b.s(18.0);
    }
    y
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
        Shade::Tint(src_color(th, hover_slot.unwrap_or(0)).fade(0.26))
    } else if ps.sel {
        Shade::Tint(th.accent.fade(0.10))
    } else {
        Shade::Card
    };
    b.panel(r, shade);
    let k = p.key.clone();
    let sel_s = b.s(10.0);
    b.push(
        R::new(r.x + b.s(3.0), r.cy() - sel_s / 2.0, sel_s, sel_s),
        Ctl::Sel(k.clone()),
        Look::Chip {
            label: String::new(),
            on: ps.sel,
            standby: false,
            amber: false,
            enabled: true,
        },
        if ps.sel {
            "Tracked: click to untrack"
        } else {
            "Click to track this param"
        },
    );
    let ks = b.s(32.0);
    let kx = r.x + b.s(16.0);
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
        },
        &tip,
    );
    let src_w = b.s(38.0);
    let chip_w = b.s(20.0);
    let val_w = if v.show_values { b.s(52.0) } else { 0.0 };
    let name_x = kx + ks + b.s(6.0);
    let name_w = (r.right() - src_w - 3.0 * chip_w - val_w - name_x - b.s(4.0)).max(b.s(20.0));
    b.text(
        R::new(name_x, r.y, name_w, r.h),
        &p.name,
        F::Normal,
        th.text,
        Align::Left,
    );
    let mut x = name_x + name_w;
    if v.show_values {
        let t = match p.steps {
            2 => {
                if p.live >= 0.5 {
                    "ON".to_string()
                } else {
                    "OFF".to_string()
                }
            }
            _ => p.text.clone(),
        };
        b.text(
            R::new(x, r.y, val_w, r.h),
            &t,
            F::Small,
            th.text_dim,
            Align::Right,
        );
        x += val_w;
    }
    for (i, l) in ["X", "Y", "N"].iter().enumerate() {
        let on = match i {
            0 => ps.x,
            1 => ps.y,
            _ => ps.inv,
        };
        let pad_writes = ps.pad_drives()
            || (!ps.srcs.is_empty() && !standby && d.plan.bus_of.contains_key(&p.key));
        let cs = b.s(18.0);
        b.push(
            R::new(x + b.s(1.0), r.cy() - cs / 2.0, cs, cs),
            Ctl::Chip(k.clone(), i as u8),
            Look::Chip {
                label: l.to_string(),
                on: on && (i == 2 || (ps.sel && pad_writes)),
                standby: on && i < 2 && ps.sel && !pad_writes,
                amber: i == 2 && on,
                enabled: true,
            },
            match i {
                0 => "X: the pad's X axis moves this param",
                1 => "Y: the pad's Y axis moves this param",
                _ => "Invert",
            },
        );
        x += chip_w;
    }
    // La colonne des sources.
    let (dots, label) = if ps.srcs.is_empty() {
        let l = match (ps.x, ps.y) {
            (true, true) => "XY",
            (true, false) => "X",
            (false, true) => "Y",
            _ => "–",
        };
        (Vec::new(), l.to_string())
    } else if ps.srcs.len() == 1 {
        let s = &ps.srcs[0];
        (vec![(src_color(th, s.slot), s.off)], short_label(s.slot))
    } else {
        (
            ps.srcs
                .iter()
                .map(|s| (src_color(th, s.slot), s.off))
                .collect(),
            String::new(),
        )
    };
    b.push(
        R::new(x + b.s(2.0), r.y, src_w - b.s(2.0), r.h),
        Ctl::Src(k),
        Look::SrcLabel { dots, label },
        "Sources: click for the source menu",
    );
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
    b.panel(r, Shade::Zone);
    let bh = b.s(22.0);
    let fw = bh;
    let mw = b.s(40.0);
    b.seg(
        R::new(r.x, r.y, r.w - fw - mw - b.s(4.0), bh),
        &["Track", "Global", "Links"],
        v.rack_tab as usize,
        |i| Ctl::RackTab(i as u8),
        &[
            "Sources of this track",
            "The global bank: a hidden CP MOD track, reaches any track by 14-bit CC",
            "Every link of this track",
        ],
    );
    b.button(
        R::new(r.right() - fw - mw - b.s(2.0), r.y, mw, bh),
        Ctl::Map,
        "Map",
        None,
        v.map,
        "Map: dragging any knob sets the selected source's depth (Esc leaves)",
    );
    b.button(
        R::new(r.right() - fw, r.y, fw, bh),
        Ctl::Fold,
        "",
        Some(Icon::ChevronRight),
        false,
        "Fold the sources",
    );
    let body = R::new(
        r.x,
        r.y + bh + b.s(4.0),
        r.w,
        (r.h - bh - b.s(4.0)).max(0.0),
    );
    b.push(body, Ctl::None, Look::Clip(1), "");
    b.clip = Some(1);
    b.dy = v.scroll[1];
    let h = match v.rack_tab {
        2 => links_tab(b, v, d, body),
        t => sources_tab(b, v, d, body, t == 1),
    };
    b.clip = None;
    b.dy = 0.0;
    let _ = th;
    h
}

fn sources_tab(b: &mut B, v: &View, d: &Data, r: R, global: bool) -> f32 {
    let th = b.th;
    let mut y = r.y;
    let cols = ((r.w / b.s(118.0)).floor() as usize).clamp(1, 4);
    let tw0 = ((r.w - b.s(3.0) * (cols - 1) as f32) / cols as f32).floor();
    let th0 = b.s(26.0);
    let tile = |b: &mut B,
                i: usize,
                y0: f32,
                slot: i32,
                title: String,
                on: bool,
                shape: Option<(u8, f64)>,
                phase: Option<f64>,
                tip: &str| {
        let x = r.x + (tw0 + b.s(3.0)) * (i % cols) as f32;
        let yy = y0 + (th0 + b.s(3.0)) * (i / cols) as f32;
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
        if on {
            let ds = b.s(10.0);
            b.push(
                R::new(x + b.s(3.0), yy + (th0 - ds) / 2.0, ds, ds),
                Ctl::TileDot(slot),
                Look::Dot {
                    color: c,
                    hollow: false,
                },
                "Drag onto a knob to modulate it",
            );
        }
    };
    if !global {
        // Les sources de la piste : enveloppes, sequenceur, notes.
        let mut i = 0;
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
                    "A Sound Gen envelope"
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
                    "A sequencer modulation lane"
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
        y += (th0 + b.s(3.0)) * i.div_ceil(cols) as f32 + b.s(6.0);
    }
    // L'enveloppe choisie (ENV 1..4) : son editeur, ici.
    if !global {
        if let (Some(Band::Env(n)), Some(j)) = (band_of(v.sel_slot), d.sg.filter(|j| j.enabled)) {
            y = crate::panels::env_editor(b, j, n, R::new(r.x, y, r.w, 0.0)) + b.s(6.0);
        }
    }
    let bank = if global { d.gbank } else { d.bank };
    let base = if global { 100 } else { 0 };
    match bank {
        None => {
            let bh = b.s(24.0);
            b.button(
                R::new(r.x, y, r.w, bh),
                Ctl::AddBank,
                if global {
                    "Add the global bank"
                } else {
                    "Add the source bank"
                },
                Some(Icon::Plus),
                false,
                if global {
                    "Global bank: hidden CP MOD track, modulates ANY track through 14-bit CC"
                } else {
                    "The CP_Mod bank: eight modulators on this track"
                },
            );
            y += bh + b.s(6.0);
        }
        Some(bk) => {
            for (i, s) in bk.slots.iter().enumerate().take(8) {
                let slot = base + i as i32 + 1;
                let ty = slot_type_name(s.shape);
                tile(
                    b,
                    i,
                    y,
                    slot,
                    format!("{ty} {}", i + 1),
                    s.on,
                    Some((s.shape, s.curve)),
                    s.on.then_some(s.pho),
                    "Click: edit · drag the dot onto a knob",
                );
            }
            y += (th0 + b.s(3.0)) * 8usize.div_ceil(cols) as f32 + b.s(8.0);
            // Le slot choisi.
            let sel = v.sel_slot - base;
            if (1..=8).contains(&sel) {
                if let Some(s) = bk.slots.get(sel as usize - 1) {
                    y = slot_editor(b, v, d, s, bk.ext, R::new(r.x, y, r.w, 0.0), base + sel);
                }
            }
        }
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

fn slot_editor(b: &mut B, v: &View, d: &Data, s: &crate::Slot, ext: bool, r: R, slot: i32) -> f32 {
    let th = b.th;
    let bh = b.s(22.0);
    let mut y = r.y;
    let gap = b.s(3.0);
    b.push(R::new(r.x, y, r.w, 1.0), Ctl::None, Look::Sep, "");
    y += b.s(4.0);
    b.text(
        R::new(r.x, y, r.w, bh),
        &long_label(slot),
        F::Bold,
        src_color(th, slot),
        Align::Left,
    );
    y += bh;
    let w3 = ((r.w - 2.0 * gap) / 3.0).floor();
    b.chip(
        R::new(r.x, y, w3, bh),
        Ctl::SlotOn,
        if s.on { "● ON" } else { "○ OFF" },
        s.on,
        true,
        "Turn the slot on or off",
    );
    b.chip(
        R::new(r.x + w3 + gap, y, w3, bh),
        Ctl::SlotType,
        slot_type_name(s.shape),
        true,
        true,
        "Type: LFO, S&H or STEP",
    );
    if s.shape <= 4 {
        b.chip(
            R::new(r.x + 2.0 * (w3 + gap), y, w3, bh),
            Ctl::SlotShape,
            bank::SHAPES[s.shape as usize],
            true,
            true,
            "Waveform",
        );
    }
    y += bh + gap;
    let half = ((r.w * 0.5) - gap).floor();
    b.seg(
        R::new(r.x, y, half, bh),
        &["Bipolar", "Unipolar"],
        s.uni as usize,
        |i| Ctl::SlotPol(i == 1),
        &[
            "Bipolar: swings around the base",
            "Unipolar: only above the base",
        ],
    );
    let q = ((r.w - half - 2.0 * gap) / 2.0).floor();
    b.button_ex(
        R::new(r.x + half + gap, y, q, bh),
        Ctl::SlotRetrig,
        "Retrig",
        None,
        false,
        ext,
        Tone::Normal,
        "Restart the cycle now",
    );
    b.button(
        R::new(r.x + half + 2.0 * gap + q, y, q, bh),
        Ctl::SlotRnd,
        "Rnd",
        Some(Icon::Dices),
        false,
        "Random settings for this slot",
    );
    y += bh + gap;
    b.chip(
        R::new(r.x, y, half, bh),
        Ctl::SlotSync,
        bank::SYNCS[(s.sync as usize).min(10)],
        s.sync > 0,
        true,
        "Free rate or synced to the tempo",
    );
    b.chip(
        R::new(r.x + half + gap, y, r.w - half - gap, bh),
        Ctl::SlotMode,
        bank::MODES[(s.mode as usize).min(2)],
        s.mode > 0,
        ext,
        "Loop, one-shot, or loop restarted with the transport",
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
                color: src_color(th, slot),
                bools: false,
            },
            "Paint the steps",
        );
        y += sh + gap;
        b.button(
            R::new(r.x, y, b.s(90.0), bh),
            Ctl::StepRnd,
            "Randomize",
            Some(Icon::Dices),
            false,
            "New values for the steps that play",
        );
        y += bh + gap;
    }
    // Les quatre boutons.
    let kw = (r.w / 4.0).floor();
    let kh = b.s(54.0);
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
    let _ = (v, d);
    y
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
