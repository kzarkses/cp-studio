//! La palette du clic droit de l'editeur : celle de [`cp_palette`] (commune
//! avec Guitar Tab), ses bends en courbes de pitch bend, et le reste de
//! l'editeur (la saisie pas a pas, l'editeur de REAPER). L'apercu de
//! l'operation survolee se dessine en pointilles sur les notes.

use cp_gfx::Canvas;
use cp_gfx::text::TextCache;
use cp_palette::{Area, BEND, BEND_OPS, Ctx, Host, PalHit, circ, icon, icon_t};
use cp_roll::Note;
use cp_ui::Theme;

pub use cp_palette::{Cat, Glyph, Op, bend_curve, euclid_hits};

use crate::{Clip, Ed, HBox, Hit, Midi};

/// La palette ouverte de l'editeur : son apercu, ce que donneraient les notes.
pub type Palette = cp_palette::Palette<Midi>;

/// Deux etats de notes donnent-ils la meme chose (notes, choix, bends) ?
pub(crate) fn same(a: &Midi, b: &Midi) -> bool {
    a.roll.notes == b.roll.notes && a.expr.carry(&a.roll.notes) == b.expr.carry(&b.roll.notes)
}

/// Ce que vise la souris dans la palette, pour la palette.
fn pal_hit(h: Hit) -> Option<PalHit> {
    match h {
        Hit::Palette => Some(PalHit::Frame),
        Hit::PalCat(k) => Some(PalHit::Cat(k)),
        Hit::PalOp(id) => Some(PalHit::Op(id)),
        _ => None,
    }
}

impl Ed {
    /// Ce que les operations communes savent de l'editeur.
    pub(crate) fn pal_ctx(&self) -> Ctx {
        Ctx {
            host: Host::Editor,
            grid: self.grid,
            origin: self.origin(),
            grid_name: crate::grid_name(if self.grid > 0.0 { self.grid } else { 0.25 }).into(),
            strength: self.strength,
            swing: self.swing,
            euclid: self.euclid,
            scale: self.scale_of().is_some(),
            cursor: self.playhead.unwrap_or(0.0),
            range: self.range,
            bar: self.bar_qn(),
            pitch: 36,
            chan: self.last_chan,
            vel: self.last_vel,
        }
    }

    /// Les operations de la palette, dans l'ordre, groupe par groupe.
    pub fn palette_ops(&self) -> Vec<Op> {
        let mut c = self.pal_ctx();
        // « Length = grid » nomme la grille de l'editeur, meme sans grille.
        c.grid_name = crate::grid_name(self.grid).into();
        let mut v = cp_palette::common_ops(&c);
        let mut add = |cat: Cat, group: u8, id: u32, label: String, glyph: Glyph| {
            v.push(Op::new(cat, group, id, label, glyph));
        };
        use Cat::*;
        let range = self.bend_range();
        for (id, amt, t, label) in [
            (100u32, 1.0f32, "\u{00bd}", "Bend \u{00bd} tone"),
            (101, 2.0, "full", "Bend full tone"),
            (102, 3.0, "1\u{00bd}", "Bend 1\u{00bd} tones"),
        ] {
            let label = if amt > range + 1e-3 {
                format!("{label} (the pitch bend range is \u{00b1}{range} st)")
            } else {
                label.to_string()
            };
            add(Bend, 0, id, label, icon_t(BEND, "", t));
        }
        add(
            Bend,
            0,
            103,
            "Bend and release".into(),
            icon("M2 18h4c2.5 0 3-9 6-9s3.5 9 6 9h4", ""),
        );
        add(
            Bend,
            1,
            104,
            "Vibrato".into(),
            icon("M2 12q2.5-6 5 0t5 0t5 0t5 0", ""),
        );
        add(
            Bend,
            1,
            105,
            "Slide in from below".into(),
            icon("M3 20 13 10", &circ(16.5, 7.5, 3.0)),
        );
        add(
            Bend,
            1,
            106,
            "Slide in from above".into(),
            icon("M3 4 13 14", &circ(16.5, 16.5, 3.0)),
        );
        add(
            Bend,
            2,
            107,
            "Remove bend".into(),
            icon(&format!("{BEND} M4 4l16 16"), ""),
        );
        add(
            More,
            0,
            61,
            "Step input from MIDI keyboard".into(),
            icon(
                "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M6 8h4 M14 8h.01 M18 8h.01 M2 12h20 M6 12v4 M10 12v4 M14 12v4 M18 12v4",
                "",
            ),
        );
        add(
            More,
            0,
            60,
            "Open in REAPER's MIDI editor".into(),
            icon(
                "M15 3h6v6 M10 14 21 3 M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
                "",
            ),
        );
        if let Some(o) = v.iter_mut().find(|o| o.id == 61) {
            o.setting = Some(self.step_input);
        }
        v
    }

    /// L'etendue des pitch bends (demi-tons) du canal des notes choisies.
    pub(crate) fn bend_range(&self) -> f32 {
        let Some(m) = self.midi() else { return 2.0 };
        let chan = m
            .roll
            .notes
            .iter()
            .find(|n| n.sel)
            .map_or(self.last_chan, |n| n.chan);
        m.expr.range(chan)
    }

    /// Ouvre la palette la ou l'on a clique (le dernier groupe utilise).
    pub fn open_palette(&mut self, x: f32, y: f32) {
        if !matches!(self.clip, Clip::Midi(_)) {
            return;
        }
        self.palette = Some(Palette::new(x, y, &Cat::EDITOR, self.pal_last));
        self.pal_refresh();
        self.dirty = true;
    }

    pub fn close_palette(&mut self) {
        if self.palette.take().is_some() {
            self.dirty = true;
        }
    }

    /// Ce que ferait l'operation `id` sur les notes d'aujourd'hui (rien si
    /// elle ne change rien).
    pub(crate) fn pal_result(&self, id: u32) -> Option<Midi> {
        let m = self.midi()?;
        let mut t = m.clone();
        self.op_on(id, &mut t, self.gen_seed.wrapping_add(1));
        (!same(m, &t)).then_some(t)
    }

    /// Les operations qui changeraient quelque chose ; l'apercu se refait.
    pub(crate) fn pal_refresh(&mut self) {
        let range = self.bend_range();
        let ids: Vec<(u32, bool)> = self
            .palette_ops()
            .iter()
            .map(|o| {
                let ok = match o.id {
                    0 => false,
                    // Les reglages et les actions sans notes.
                    40..=47 | 60 | 61 => true,
                    100..=102 if BEND_OPS[(o.id - 100) as usize].1 > range + 1e-3 => false,
                    id => self.pal_result(id).is_some(),
                };
                (o.id, ok)
            })
            .collect();
        if let Some(p) = &mut self.palette {
            p.ok = ids.into_iter().filter(|x| x.1).map(|x| x.0).collect();
            p.preview = None;
        }
    }

    /// Survoler l'operation `id` : son resultat en apercu.
    pub(crate) fn pal_hover(&mut self, id: Option<u32>) {
        let cur = self
            .palette
            .as_ref()
            .and_then(|p| p.preview.as_ref().map(|x| x.0));
        if cur == id {
            return;
        }
        let pv = id
            .filter(|&i| !matches!(i, 0 | 40..=47 | 60 | 61))
            .filter(|i| self.palette.as_ref().is_some_and(|p| p.ok.contains(i)))
            .and_then(|i| self.pal_result(i).map(|m| (i, m)));
        if let Some(p) = &mut self.palette {
            p.preview = pv;
            self.dirty = true;
        }
    }

    /// Un clic dans la palette. Maj garde la palette ouverte.
    pub(crate) fn pal_click(&mut self, h: Hit, shift: bool) -> Vec<crate::Act> {
        let Some(cat) = self.palette.as_ref().map(|p| p.cat) else {
            return Vec::new();
        };
        match h {
            Hit::PalCat(k) => {
                if let Some(p) = &mut self.palette {
                    if let Some(c) = p.cat_at(k) {
                        p.cat = c;
                    }
                    p.preview = None;
                }
                self.dirty = true;
                Vec::new()
            }
            Hit::PalOp(id) => {
                let ok = self.palette.as_ref().is_some_and(|p| p.ok.contains(&id));
                if !ok {
                    return Vec::new();
                }
                self.pal_last = cat;
                let setting = matches!(id, 40..=47 | 61);
                let out = if id == 54 {
                    self.quantize()
                } else {
                    self.transform(id)
                };
                if setting || shift {
                    self.pal_refresh();
                } else {
                    self.close_palette();
                }
                self.dirty = true;
                out
            }
            _ => Vec::new(),
        }
    }

    /// La molette sur un rythme euclidien : ses coups (Maj : ses cases).
    pub(crate) fn pal_wheel(&mut self, id: u32, up: bool, shift: bool) {
        let k = (id - 90) as usize;
        let Some(e) = self.euclid.get_mut(k) else {
            return;
        };
        let d: i64 = if up { 1 } else { -1 };
        if shift {
            e.1 = (e.1 as i64 + d).clamp(e.0.max(2) as i64, 32) as usize;
        } else {
            e.0 = (e.0 as i64 + d).clamp(1, e.1 as i64) as usize;
        }
        self.pal_refresh();
        if let Some(p) = &mut self.palette {
            p.preview = None;
        }
        self.pal_hover(Some(id));
    }

    /// Dessine la palette, par-dessus tout, et retient ses cibles.
    pub(crate) fn draw_palette(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        let Some(p) = self.palette.as_ref() else {
            return;
        };
        let ops = self.palette_ops();
        let l = self.lay;
        let area = Area {
            w: l.w,
            h: l.h,
            top: l.bar_h,
        };
        let (r, cells) = cp_palette::draw(cv, text, th, p, &ops, pal_hit(self.hover), area);
        for (x, y, w, h, ph) in cells {
            let hit = match ph {
                PalHit::Frame => Hit::Palette,
                PalHit::Cat(k) => Hit::PalCat(k),
                PalHit::Op(id) => Hit::PalOp(id),
            };
            bx.push((x, y, w, h, hit));
        }
        if let Some(pp) = &mut self.palette {
            pp.rect = r;
        }
    }
}

/// Le pied (s'il y en a un) et le sommet d'un bend.
pub(crate) type Handles<T> = (Option<(T, f32)>, (T, f32));

/// Les poignees du bend d'une note : son pied (ou il part) et son sommet,
/// (noire, demi-tons) ; le pied manque quand la courbe part du sommet (une
/// glisse d'approche).
pub(crate) fn bend_handles(curve: &[(f64, f32)]) -> Option<Handles<f64>> {
    if curve.len() < 2 {
        return None;
    }
    // La derniere valeur est celle de la fin (rajoutee par `curve`).
    let body = &curve[..curve.len() - 1];
    let (pi, peak) = body
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.abs().total_cmp(&b.1.1.abs()))?;
    if peak.1.abs() < 0.05 {
        return None;
    }
    let foot = (0..pi)
        .rev()
        .find(|&k| body[k].1.abs() < peak.1.abs() * 0.1)
        .map(|k| body[k]);
    Some((foot, *peak))
}

/// La courbe d'une note (relative a son debut), pour la modifier : sans la
/// derniere valeur (celle de la fin, que [`cp_roll::expr::Expr::set_bend`]
/// remet).
pub(crate) fn rel_curve(n: &Note, curve: &[(f64, f32)]) -> Vec<(f64, f32)> {
    let k = curve.len().saturating_sub(1);
    curve[..k].iter().map(|&(q, v)| (q - n.qn0, v)).collect()
}
