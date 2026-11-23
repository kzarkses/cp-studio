//! Les notes choisies de Guitar Tab : un lasso (clic droit glisse dans la
//! partition ou la tablature), Ctrl + clic, Maj + clic ; glissees ensemble
//! (clic gauche), effacees ensemble ; et la palette du clic droit, celle de
//! l'editeur ([`cp_palette`]) avec les bends et les techniques de la tab.
//!
//! Les operations de la palette sont celles de [`cp_roll`] : les notes d'un
//! item y passent avec leur corde dans le canal, et reviennent ; une note qui
//! change de hauteur, ou qui nait (un accord, un arpege, un rythme
//! euclidien), prend un doigte jouable autour des notes restees en place.

use std::collections::HashSet;

use cp_gfx::Canvas;
use cp_gfx::text::TextCache;
use cp_palette::{Area, Cat, Ctx, Glyph, Host, Op, PalHit, circ, dots, icon, icon_t};
use cp_roll::Roll;
use cp_tab::doc::{self, TNote};
use cp_tab::fret::{self, Tuning};
use cp_ui::Theme;

use crate::{Act, Arr, HBox, Hit, Item, Mark, Pick, Sel};

/// La cle d'une note (son debut au 1/960 de noire, sa hauteur).
pub type NoteKey = (i64, i32);

/// L'apercu de la palette : la tablature de chaque item vise, telle que
/// l'operation survolee la laisserait.
pub type TabPreview = Vec<(usize, usize, Vec<TNote>)>;

/// La palette ouverte de Guitar Tab.
pub type TabPalette = cp_palette::Palette<TabPreview>;

/// Les marques de la tab dans la palette : (numero, marque).
pub const MARK_OPS: [(u32, Mark); 14] = [
    (100, Mark::Half),
    (101, Mark::Full),
    (102, Mark::OneHalf),
    (103, Mark::Release),
    (104, Mark::Vibrato),
    (105, Mark::InBelow),
    (106, Mark::InAbove),
    (107, Mark::Plain),
    (108, Mark::Legato),
    (109, Mark::Slide),
    (110, Mark::Palm),
    (111, Mark::Dead),
    (112, Mark::Harmonic),
    (113, Mark::Ring),
];

fn mark_of(id: u32) -> Option<Mark> {
    MARK_OPS.iter().find(|x| x.0 == id).map(|x| x.1)
}

/// Le nom d'une grille (noires) : « 1/16 », « 1/8T ».
pub fn grid_name(g: f64) -> String {
    if g <= 0.0 {
        return "grid".into();
    }
    let d = 4.0 / g;
    if (d - d.round()).abs() < 1e-6 && (d.round() as i64).count_ones() == 1 {
        return format!("1/{}", d.round() as i64);
    }
    let t = d * 2.0 / 3.0;
    if (t - t.round()).abs() < 1e-6 {
        return format!("1/{}T", t.round() as i64);
    }
    "grid".into()
}

impl Item {
    /// La note `n` est-elle choisie ?
    pub fn is_chosen(&self, n: usize) -> bool {
        self.doc
            .get(n)
            .is_some_and(|x| self.sel_notes.contains(&x.key()))
    }

    /// Les rangs des notes choisies.
    pub fn chosen(&self) -> Vec<usize> {
        if self.sel_notes.is_empty() {
            return Vec::new();
        }
        (0..self.doc.len()).filter(|&n| self.is_chosen(n)).collect()
    }

    /// Oublie les cles qui ne sont plus celles d'une note.
    pub fn prune_chosen(&mut self) {
        if self.sel_notes.is_empty() {
            return;
        }
        let keys: HashSet<NoteKey> = self.doc.iter().map(TNote::key).collect();
        self.sel_notes.retain(|k| keys.contains(k));
    }
}

/// Les notes d'un item dans un [`Roll`] : la note `k` y porte l'id `k + 1`,
/// sa corde dans le canal ; les choisies le sont.
fn roll_of(it: &Item) -> Roll {
    let mut r = Roll::from_notes(it.doc.iter().map(|n| {
        (
            n.qn0,
            n.qn1 - n.qn0,
            n.pitch,
            n.string as i32,
            n.vel as i32,
            it.sel_notes.contains(&n.key()),
            false,
        )
    }));
    r.scale = it.key.map(|k| cp_roll::Scale {
        root: k.tonic(),
        kind: if k.minor { 2 } else { 1 },
    });
    r
}

/// Une corde jouable pour `pitch`, libre dans `taken` : celle dont la case
/// est la plus proche de `fret` (la main), puis la plus proche de `near`.
fn free_string(pitch: i32, near: usize, fret: i32, taken: &[usize], t: &Tuning) -> Option<usize> {
    (0..t.strings.len())
        .filter(|s| !taken.contains(s))
        .filter(|&s| (0..=t.frets).contains(&(pitch - t.strings[s])))
        .min_by_key(|&s| {
            let f = pitch - t.strings[s];
            ((f - fret).abs(), (s as i64 - near as i64).abs())
        })
}

/// La tablature d'un item apres une operation sur son [`Roll`] : (notes,
/// cles choisies, cles effacees). Une note gardee garde sa corde, ses
/// liaisons et ses techniques ; une note nee ou changee de hauteur prend un
/// doigte sur sa phrase ([`fret::assign`]), en evitant les cordes des notes
/// qui sonnent avec elle ; une note injouable tombe.
fn doc_of_roll(it: &Item, r: &Roll, bpm: f64) -> (Vec<TNote>, HashSet<NoteKey>, Vec<NoteKey>) {
    let t = it.detect.tuning();
    let nstr = t.strings.len();
    let mut out: Vec<(TNote, bool, bool)> = Vec::new(); // (note, a placer, choisie)
    let mut seen = vec![false; it.doc.len()];
    for n in &r.notes {
        let k = n.id as usize;
        let old = (1..=it.doc.len()).contains(&k).then(|| &it.doc[k - 1]);
        let mut x = match old {
            Some(o) => {
                seen[k - 1] = true;
                o.clone()
            }
            None => TNote {
                string: (n.chan.max(0) as usize).min(nstr.saturating_sub(1)),
                ..TNote::default()
            },
        };
        let len = n.len.max(cp_roll::MIN_LEN);
        let changed = old.is_none_or(|o| {
            (o.qn0 - n.qn0).abs() > 1e-9
                || ((o.qn1 - o.qn0) - len).abs() > 1e-9
                || o.pitch != n.pitch
                || o.vel as i32 != n.vel
        });
        let repitch = old.is_none_or(|o| o.pitch != n.pitch);
        x.qn0 = n.qn0;
        x.qn1 = n.qn0 + len;
        x.pitch = n.pitch;
        x.vel = n.vel.clamp(1, 127) as u8;
        if changed {
            x.locked = true;
            // La courbe ne depasse pas la note.
            x.bend.retain(|p| p.0 < len - 1e-9);
        }
        if repitch {
            // Une autre hauteur : ni liaison ni courbe d'avant.
            x.tech = cp_tab::doc::Tech::Plain;
            x.bend.clear();
            if old.is_some() {
                x.string = (n.chan.max(0) as usize).min(nstr.saturating_sub(1));
            }
        }
        out.push((x, repitch, n.sel));
    }
    // Le doigte des notes a placer : leur phrase, accord par accord.
    let cell = |q: f64| (q * doc::FREE_CELLS as f64).round() as i64;
    let mut starts: Vec<i64> = out.iter().filter(|e| e.1).map(|e| cell(e.0.qn0)).collect();
    starts.sort_unstable();
    starts.dedup();
    if !starts.is_empty() {
        let groups: Vec<Vec<usize>> = starts
            .iter()
            .map(|&s| {
                (0..out.len())
                    .filter(|&j| out[j].1 && cell(out[j].0.qn0) == s)
                    .collect()
            })
            .collect();
        let pitches: Vec<Vec<i32>> = groups
            .iter()
            .map(|g| g.iter().map(|&j| out[j].0.pitch).collect())
            .collect();
        let gaps: Vec<f64> = starts
            .iter()
            .enumerate()
            .map(|(k, &s)| {
                if k == 0 {
                    1.0
                } else {
                    (s - starts[k - 1]) as f64 / doc::FREE_CELLS as f64 * 60.0 / bpm.max(1.0)
                }
            })
            .collect();
        let spots = fret::assign(&pitches, &gaps, &t);
        for (g, sp) in groups.iter().zip(spots) {
            // Les cordes deja prises a ce moment : les notes gardees.
            let s0 = cell(out[g[0]].0.qn0);
            let mut taken: Vec<usize> = out
                .iter()
                .filter(|e| !e.1 && cell(e.0.qn0) == s0)
                .map(|e| e.0.string)
                .collect();
            for (&j, spot) in g.iter().zip(sp) {
                let p = out[j].0.pitch;
                let want = spot.map_or(out[j].0.string, |s| s.string);
                let wf = spot.map_or_else(|| p - t.strings[want.min(nstr - 1)], |s| s.fret);
                match free_string(p, want, wf, &taken, &t) {
                    Some(s) => {
                        out[j].0.string = s;
                        out[j].0.fret = p - t.strings[s];
                        out[j].1 = false;
                        taken.push(s);
                    }
                    // Injouable ici : elle tombe.
                    None => out[j].0.fret = i32::MIN,
                }
            }
        }
    }
    let mut notes: Vec<TNote> = Vec::with_capacity(out.len());
    let mut chosen: HashSet<NoteKey> = HashSet::new();
    for (x, _, sel) in out {
        if x.fret == i32::MIN {
            continue;
        }
        if sel {
            chosen.insert(x.key());
        }
        notes.push(x);
    }
    doc::sort(&mut notes);
    let kept: HashSet<NoteKey> = notes.iter().map(TNote::key).collect();
    let gone: Vec<NoteKey> = it
        .doc
        .iter()
        .enumerate()
        .filter(|(k, n)| !seen[*k] && !kept.contains(&n.key()))
        .map(|(_, n)| n.key())
        .collect();
    (notes, chosen, gone)
}

impl Arr {
    // -----------------------------------------------------------------------
    // Le choix des notes
    // -----------------------------------------------------------------------

    /// Toutes les notes choisies : (piste, item, rang).
    pub fn chosen_notes(&self) -> Vec<(usize, usize, usize)> {
        let mut v = Vec::new();
        for (t, tr) in self.tracks.iter().enumerate() {
            for (i, it) in tr.items.iter().enumerate() {
                for n in it.chosen() {
                    v.push((t, i, n));
                }
            }
        }
        v
    }

    /// Plus aucune note choisie. Vrai si quelque chose a change.
    pub fn clear_chosen(&mut self) -> bool {
        let mut any = false;
        for tr in &mut self.tracks {
            for it in &mut tr.items {
                any |= !it.sel_notes.is_empty();
                it.sel_notes.clear();
            }
        }
        if any {
            self.dirty = true;
        }
        any
    }

    /// Choisit une note selon `pick` (seule, en plus, en inversant).
    pub fn choose(&mut self, t: usize, i: usize, n: usize, pick: Pick) {
        if pick == Pick::Replace {
            self.clear_chosen();
        }
        let Some(it) = self.item_mut(t, i) else {
            return;
        };
        let Some(k) = it.doc.get(n).map(TNote::key) else {
            return;
        };
        if pick == Pick::Toggle && it.sel_notes.contains(&k) {
            it.sel_notes.remove(&k);
        } else {
            it.sel_notes.insert(k);
        }
        self.dirty = true;
    }

    /// La note `n` devient la note retenue et la seule choisie.
    pub fn focus_note(&mut self, t: usize, i: usize, n: Option<usize>) {
        self.select(Some(Sel { t, i: Some(i), n }));
        self.clear_chosen();
        if let Some(n) = n {
            self.choose(t, i, n, Pick::Add);
        }
    }

    /// Le lasso commence ou finit-il dans une partition ou une tablature ?
    pub(crate) fn in_notes_lane(&self, x: f32, y: f32) -> bool {
        self.boxes.iter().rev().any(|b| {
            x >= b.0
                && x < b.0 + b.2
                && y >= b.1
                && y < b.1 + b.3
                && matches!(
                    b.4,
                    Hit::Item {
                        lane: crate::Lane::Score | crate::Lane::Tab,
                        ..
                    } | Hit::Lane {
                        lane: crate::Lane::Score | crate::Lane::Tab,
                        ..
                    } | Hit::Note { .. }
                        | Hit::TabLine { .. }
                )
        })
    }

    /// Les notes que touche un lasso (partition ou tablature).
    pub fn note_hits(&self, r: (f32, f32, f32, f32)) -> Vec<(usize, usize, usize)> {
        let (xa, ya, xb, yb) = (r.0.min(r.2), r.1.min(r.3), r.0.max(r.2), r.1.max(r.3));
        let mut out: Vec<(usize, usize, usize)> = Vec::new();
        for b in &self.boxes {
            let Hit::Note { t, i, n } = b.4 else {
                continue;
            };
            if b.0 >= xb || b.0 + b.2 <= xa || b.1 >= yb || b.1 + b.3 <= ya {
                continue;
            }
            if !out.contains(&(t, i, n)) {
                out.push((t, i, n));
            }
        }
        out
    }

    /// Le lasso des notes commence : la selection d'avant est retenue.
    pub(crate) fn lasso_begin(&mut self) {
        self.lasso_base = self
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(t, tr)| {
                tr.items
                    .iter()
                    .enumerate()
                    .map(move |(i, it)| (t, i, it.sel_notes.clone()))
            })
            .collect();
    }

    /// Le lasso des notes bouge : la selection suit, tout de suite.
    pub(crate) fn lasso_update(&mut self, r: (f32, f32, f32, f32), pick: Pick) {
        let hits = self.note_hits(r);
        let base = std::mem::take(&mut self.lasso_base);
        for (t, i, was) in &base {
            let Some(it) = self.item_mut(*t, *i) else {
                continue;
            };
            let hit: HashSet<NoteKey> = hits
                .iter()
                .filter(|h| h.0 == *t && h.1 == *i)
                .filter_map(|h| it.doc.get(h.2).map(TNote::key))
                .collect();
            it.sel_notes = match pick {
                Pick::Replace => hit,
                Pick::Add => was.union(&hit).copied().collect(),
                Pick::Toggle => was.symmetric_difference(&hit).copied().collect(),
            };
        }
        self.lasso_base = base;
        self.dirty = true;
    }

    /// Efface les notes choisies (un nouveau decodage ne les ramene pas).
    pub fn delete_chosen(&mut self) -> Option<Act> {
        let mut items = Vec::new();
        for (t, tr) in self.tracks.iter_mut().enumerate() {
            for (i, it) in tr.items.iter_mut().enumerate() {
                if it.sel_notes.is_empty() {
                    continue;
                }
                let sel = std::mem::take(&mut it.sel_notes);
                let before = it.doc.len();
                let mut gone = Vec::new();
                it.doc.retain(|n| {
                    let k = n.key();
                    if sel.contains(&k) {
                        gone.push(k);
                        false
                    } else {
                        true
                    }
                });
                if it.doc.len() != before {
                    it.suppressed.extend(gone);
                    it.touch();
                    items.push((t, i));
                }
            }
        }
        if items.is_empty() {
            return None;
        }
        if let Some(s) = self.sel {
            self.select(Some(Sel { n: None, ..s }));
        }
        self.dirty = true;
        Some(Act::EditedItems {
            items,
            label: "CP Guitar Tab: delete notes",
        })
    }

    // -----------------------------------------------------------------------
    // Les notes glissees ensemble
    // -----------------------------------------------------------------------

    /// Un glisser de notes commence : la note saisie et les choisies.
    pub(crate) fn notes_drag_begin(&mut self) {
        self.moving = self
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(t, tr)| {
                tr.items.iter().enumerate().filter_map(move |(i, it)| {
                    let c = it.chosen();
                    (!c.is_empty()).then(|| (t, i, it.doc.clone(), c))
                })
            })
            .collect();
    }

    /// Les notes glissees : `dq` noires (calees par la note saisie), `ds`
    /// cordes (une note qui ne se joue pas sur sa nouvelle corde reste sur la
    /// sienne). Chaque note reste dans son item.
    pub(crate) fn notes_drag_to(&mut self, dq: f64, ds: i32) -> bool {
        // Le pas, borne pour que toutes restent dans leur item.
        let mut d = dq;
        for (t, i, before, idx) in &self.moving {
            let Some(it) = self.item(*t, *i) else {
                continue;
            };
            let lo = idx
                .iter()
                .map(|&k| before[k].qn0)
                .fold(f64::INFINITY, f64::min);
            let hi = idx
                .iter()
                .map(|&k| before[k].qn0)
                .fold(f64::NEG_INFINITY, f64::max);
            d = d.max(it.qn0 - lo).min(it.qn1 - 1e-6 - hi);
        }
        let moving = std::mem::take(&mut self.moving);
        let mut changed = false;
        for (t, i, before, idx) in &moving {
            let Some(it) = self.item_mut(*t, *i) else {
                continue;
            };
            let tu = it.detect.tuning();
            let mut doc = before.clone();
            let mut keys = HashSet::new();
            for &k in idx {
                let n = &mut doc[k];
                if d.abs() > 1e-9 || ds != 0 {
                    n.locked = true;
                }
                n.qn0 += d;
                n.qn1 += d;
                if ds != 0 {
                    let s = n.string as i32 + ds;
                    if s >= 0 && (s as usize) < tu.strings.len() {
                        let f = n.pitch - tu.strings[s as usize];
                        if (0..=tu.frets).contains(&f) {
                            n.string = s as usize;
                            n.fret = f;
                        }
                    }
                }
                keys.insert(n.key());
            }
            changed |= doc != it.doc;
            it.doc = doc;
            it.sel_notes = keys;
            it.touch();
        }
        self.moving = moving;
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// Le glisser de notes fini : les tablatures se retrient, la note saisie
    /// reste la note retenue. Rend les items a ecrire.
    pub(crate) fn notes_drag_end(&mut self, grab: (usize, usize, usize)) -> Vec<(usize, usize)> {
        let moving = std::mem::take(&mut self.moving);
        let mut items = Vec::new();
        let mut focus = None;
        for (t, i, before, _) in &moving {
            let Some(it) = self.item_mut(*t, *i) else {
                continue;
            };
            let gk = ((*t, *i) == (grab.0, grab.1))
                .then(|| it.doc.get(grab.2).map(TNote::key))
                .flatten();
            let moved = it.doc != *before;
            doc::sort(&mut it.doc);
            it.touch();
            if let Some(k) = gk {
                focus = it.doc.iter().position(|x| x.key() == k);
            }
            if moved {
                items.push((*t, *i));
            }
        }
        self.select(Some(Sel {
            t: grab.0,
            i: Some(grab.1),
            n: focus,
        }));
        items
    }

    // -----------------------------------------------------------------------
    // La palette
    // -----------------------------------------------------------------------

    /// La grille des operations (noires) : celle de REAPER, sinon celle de
    /// l'item ; aucune en jeu libre.
    fn pal_grid(&self, t: usize, i: usize) -> f64 {
        let Some(it) = self.item(t, i) else {
            return 0.25;
        };
        if it.free {
            return 0.0;
        }
        match self.grid {
            Some(g) if g.on => g.div_qn.max(1.0 / 64.0),
            _ => 1.0 / it.detect.grid.max(1) as f64,
        }
    }

    /// Ce que les operations communes savent de l'item `(t, i)`.
    fn pal_ctx(&self, t: usize, i: usize) -> Ctx {
        let it = self.item(t, i);
        let g = self.pal_grid(t, i);
        let cursor = self.edit_cursor.or(self.playhead).unwrap_or(0.0);
        let nstr = it.map_or(6, |x| x.detect.tuning().strings.len());
        Ctx {
            host: Host::Tab,
            grid: g,
            grid_name: grid_name(g),
            strength: self.strength,
            swing: self.swing,
            euclid: self.euclid,
            scale: it.is_some_and(|x| x.key.is_some()),
            cursor,
            range: self.range,
            bar: it.map_or(4.0, Item::bar_qn),
            // Sans note choisie : un Mi grave sur la corde de La.
            pitch: 52,
            chan: (nstr.min(6) as i32 - 5).max(0),
            vel: 96,
        }
    }

    /// Les operations de la palette de Guitar Tab, dans l'ordre.
    pub fn palette_ops(&self) -> Vec<Op> {
        let (t, i) = self.pal_items.first().copied().unwrap_or((0, 0));
        let mut v = cp_palette::common_ops(&self.pal_ctx(t, i));
        let chosen = self.pal_targets();
        let on = |m: Mark| {
            !chosen.is_empty() && chosen.iter().all(|&(t, i, n)| self.mark_on(t, i, n, m))
        };
        let rising = chosen
            .first()
            .and_then(|&(t, i, n)| {
                let j = self.prev_on_string(t, i, n)?;
                let it = self.item(t, i)?;
                Some(it.doc.get(n)?.pitch > it.doc.get(j)?.pitch)
            })
            .unwrap_or(true);
        use cp_palette::BEND;
        let ops: Vec<(Cat, u8, u32, Glyph)> = vec![
            (Cat::Bend, 0, 100, icon_t(BEND, "", "\u{00bd}")),
            (Cat::Bend, 0, 101, icon_t(BEND, "", "full")),
            (Cat::Bend, 0, 102, icon_t(BEND, "", "1\u{00bd}")),
            (
                Cat::Bend,
                0,
                103,
                icon("M2 18h4c2.5 0 3-9 6-9s3.5 9 6 9h4", ""),
            ),
            (Cat::Bend, 1, 104, icon("M2 12q2.5-6 5 0t5 0t5 0t5 0", "")),
            (
                Cat::Bend,
                2,
                105,
                icon("M3 20 13 10", &circ(16.5, 7.5, 3.0)),
            ),
            (
                Cat::Bend,
                2,
                106,
                icon("M3 4 13 14", &circ(16.5, 16.5, 3.0)),
            ),
            (
                Cat::Bend,
                3,
                108,
                icon_t(
                    "M6 13q6-7 12 0",
                    &dots(&[(6.0, 17.0), (18.0, 17.0)], 2.4),
                    if rising { "H" } else { "P" },
                ),
            ),
            (
                Cat::Bend,
                3,
                109,
                if rising {
                    icon(
                        "M8.5 14.5 15.5 9.5",
                        &dots(&[(5.0, 17.0), (19.0, 7.0)], 2.4),
                    )
                } else {
                    icon(
                        "M8.5 9.5 15.5 14.5",
                        &dots(&[(5.0, 7.0), (19.0, 17.0)], 2.4),
                    )
                },
            ),
            (Cat::Bend, 4, 107, icon(&format!("{BEND} M4 4l16 16"), "")),
            (Cat::Tech, 0, 110, Glyph::Chip("P.M.".into())),
            (
                Cat::Tech,
                0,
                111,
                icon("M5 5.5h14v13H5z M9 9l6 6 M15 9l-6 6", ""),
            ),
            (Cat::Tech, 0, 112, icon("M12 4l8 8-8 8-8-8z", "")),
            (Cat::Tech, 0, 113, Glyph::Chip("let ring".into())),
        ];
        for (cat, g, id, glyph) in ops {
            let Some(m) = mark_of(id) else { continue };
            let mut o = Op::new(cat, g, id, m.label(), glyph);
            o.on = m != Mark::Plain && on(m);
            v.push(o);
        }
        v.push(Op::new(
            Cat::More,
            0,
            60,
            "Open in the editor",
            icon(
                "M15 3h6v6 M10 14 21 3 M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
                "",
            ),
        ));
        v
    }

    /// La marque `m` se pose-t-elle (ou s'enleve-t-elle) sur cette note ?
    /// `on` : toutes les notes visees l'ont (le clic l'enleve a toutes) ;
    /// sinon elle se pose sur celles qui ne l'ont pas. « Note simple »
    /// touche celles qui ne le sont pas.
    fn mark_goes(&self, t: usize, i: usize, n: usize, m: Mark, on: bool) -> bool {
        let cur = self.mark_on(t, i, n, m);
        if m == Mark::Plain {
            return !cur;
        }
        cur == on && self.mark_ok(t, i, n, m)
    }

    /// Les notes que vise la palette : les choisies des items vises.
    fn pal_targets(&self) -> Vec<(usize, usize, usize)> {
        self.pal_items
            .iter()
            .flat_map(|&(t, i)| {
                self.item(t, i)
                    .map(|it| it.chosen())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |n| (t, i, n))
            })
            .collect()
    }

    /// Ouvre la palette en `(x, y)` sur l'item `(t, i)` (ses notes, ou celles
    /// de tous les items qui en ont de choisies).
    pub fn open_palette(&mut self, x: f32, y: f32, t: usize, i: usize) {
        let mut items: Vec<(usize, usize)> = Vec::new();
        for (tt, tr) in self.tracks.iter().enumerate() {
            for (ii, it) in tr.items.iter().enumerate() {
                if !it.sel_notes.is_empty() {
                    items.push((tt, ii));
                }
            }
        }
        if !items.contains(&(t, i)) {
            if self.item(t, i).is_some_and(|it| it.sel_notes.is_empty()) && !items.is_empty() {
                // Un clic droit sur un autre item : celui-la seul.
                items.clear();
            }
            items.insert(0, (t, i));
        } else {
            items.retain(|x| *x != (t, i));
            items.insert(0, (t, i));
        }
        self.pal_items = items;
        self.palette = Some(TabPalette::new(x, y, &Cat::TAB, self.pal_last));
        self.pal_refresh();
        self.dirty = true;
    }

    pub fn close_palette(&mut self) {
        if self.palette.take().is_some() {
            self.dirty = true;
        }
    }

    /// Ce que ferait l'operation `id` : la tablature de chaque item qu'elle
    /// change (rien si elle ne change rien).
    fn pal_result(&self, id: u32, seed: u64) -> Option<TabPreview> {
        let mut out: TabPreview = Vec::new();
        if let Some(m) = mark_of(id) {
            let targets = self.pal_targets();
            let on =
                !targets.is_empty() && targets.iter().all(|&(t, i, n)| self.mark_on(t, i, n, m));
            // Un essai sur une copie des items vises.
            for &(t, i) in &self.pal_items {
                let mut probe = Arr::new();
                let Some(it) = self.item(t, i) else { continue };
                let mut tr = crate::Track::default();
                tr.items.push(it.clone());
                probe.tracks.push(tr);
                let mut any = false;
                for n in it.chosen() {
                    if probe.mark_goes(0, 0, n, m, on) {
                        any |= probe.apply_mark(0, 0, n, m).is_some();
                    }
                }
                let d = probe.tracks[0].items[0].doc.clone();
                if any && d != it.doc {
                    out.push((t, i, d));
                }
            }
            return (!out.is_empty()).then_some(out);
        }
        if matches!(id, 0 | 40..=47 | 60) {
            return None;
        }
        for &(t, i) in &self.pal_items {
            let Some(it) = self.item(t, i) else { continue };
            let mut r = roll_of(it);
            let before = r.notes.clone();
            cp_palette::apply(id, &mut r, &self.pal_ctx(t, i), seed);
            if r.notes == before {
                continue;
            }
            let (d, _, _) = doc_of_roll(it, &r, it.bpm);
            out.push((t, i, d));
        }
        (!out.is_empty()).then_some(out)
    }

    /// L'operation changerait-elle quelque chose ? (sans refaire le doigte)
    fn pal_changes(&self, id: u32) -> bool {
        match id {
            0 => false,
            40..=47 | 60 => true,
            _ if mark_of(id).is_some() => {
                let targets = self.pal_targets();
                let m = mark_of(id).unwrap_or(Mark::Plain);
                if m == Mark::Plain {
                    return targets
                        .iter()
                        .any(|&(t, i, n)| !self.mark_on(t, i, n, Mark::Plain));
                }
                targets.iter().any(|&(t, i, n)| self.mark_ok(t, i, n, m))
            }
            _ => self.pal_items.iter().any(|&(t, i)| {
                self.item(t, i).is_some_and(|it| {
                    let mut r = roll_of(it);
                    let before = r.notes.clone();
                    cp_palette::apply(id, &mut r, &self.pal_ctx(t, i), self.gen_seed + 1);
                    r.notes != before
                })
            }),
        }
    }

    /// Les operations qui changeraient quelque chose ; l'apercu se refait.
    pub(crate) fn pal_refresh(&mut self) {
        let ids: Vec<u32> = self
            .palette_ops()
            .iter()
            .map(|o| o.id)
            .filter(|&id| self.pal_changes(id))
            .collect();
        if let Some(p) = &mut self.palette {
            p.ok = ids;
            p.preview = None;
        }
        self.dirty = true;
    }

    /// Survoler l'operation `id` : la tablature qu'elle donnerait.
    pub(crate) fn pal_hover(&mut self, id: Option<u32>) {
        let cur = self
            .palette
            .as_ref()
            .and_then(|p| p.preview.as_ref().map(|x| x.0));
        if cur == id {
            return;
        }
        let pv = id
            .filter(|&i| !matches!(i, 0 | 40..=47 | 60))
            .filter(|i| self.palette.as_ref().is_some_and(|p| p.ok.contains(i)))
            .and_then(|i| self.pal_result(i, self.gen_seed + 1).map(|m| (i, m)));
        if let Some(p) = &mut self.palette {
            p.preview = pv;
        }
        self.dirty = true;
    }

    /// Un clic dans la palette. Maj la garde ouverte.
    pub(crate) fn pal_click(&mut self, h: Hit, shift: bool) -> Option<Act> {
        let cat = self.palette.as_ref()?.cat;
        match h {
            Hit::PalCat(k) => {
                if let Some(p) = &mut self.palette {
                    if let Some(c) = p.cat_at(k) {
                        p.cat = c;
                    }
                    p.preview = None;
                }
                self.dirty = true;
                None
            }
            Hit::PalOp(id) => {
                if !self.palette.as_ref().is_some_and(|p| p.ok.contains(&id)) {
                    return None;
                }
                self.pal_last = cat;
                let setting = cp_palette::is_setting(id);
                let out = self.pal_apply(id);
                if setting || shift {
                    self.pal_refresh();
                } else {
                    self.close_palette();
                }
                self.dirty = true;
                out
            }
            _ => None,
        }
    }

    /// Applique l'operation `id` aux notes visees.
    fn pal_apply(&mut self, id: u32) -> Option<Act> {
        if cp_palette::set_setting(id, &mut self.strength, &mut self.swing) {
            return None;
        }
        if id == 60 {
            let (t, i) = *self.pal_items.first()?;
            return Some(Act::OpenMidi { t, i });
        }
        if let Some(m) = mark_of(id) {
            let targets = self.pal_targets();
            let on =
                !targets.is_empty() && targets.iter().all(|&(t, i, n)| self.mark_on(t, i, n, m));
            let mut items: Vec<(usize, usize)> = Vec::new();
            // Les rangs ne bougent pas : une marque ne deplace pas de note.
            for (t, i, n) in targets {
                if self.mark_goes(t, i, n, m, on)
                    && self.apply_mark(t, i, n, m).is_some()
                    && !items.contains(&(t, i))
                {
                    items.push((t, i));
                }
            }
            return (!items.is_empty()).then_some(Act::EditedItems {
                items,
                label: "CP Guitar Tab: playing technique",
            });
        }
        self.gen_seed = self.gen_seed.wrapping_add(1);
        let seed = self.gen_seed;
        let mut items: Vec<(usize, usize)> = Vec::new();
        let mut label = "";
        let targets = self.pal_items.clone();
        for (t, i) in targets {
            let ctx = self.pal_ctx(t, i);
            let Some(it) = self.item_mut(t, i) else {
                continue;
            };
            let mut r = roll_of(it);
            let before = r.notes.clone();
            let (k, l) = cp_palette::apply(id, &mut r, &ctx, seed);
            if r.notes == before {
                continue;
            }
            let (d, chosen, gone) = doc_of_roll(it, &r, it.bpm);
            let writes = k > 0 && !l.is_empty();
            if writes {
                it.suppressed.extend(gone);
                it.doc = d;
                it.touch();
                items.push((t, i));
                label = l;
            }
            it.sel_notes = chosen;
        }
        self.dirty = true;
        (!items.is_empty()).then_some(Act::EditedItems { items, label })
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
        self.pal_hover(Some(id));
    }

    /// La palette survolee : un groupe s'ouvre (si l'on y va de cote), une
    /// operation montre son resultat.
    pub(crate) fn pal_move(&mut self, h: Hit, dx: f32, dy: f32) {
        match h {
            Hit::PalCat(k) if dx.abs() >= dy.abs() => {
                if let Some(p) = &mut self.palette {
                    let c = p.cat_at(k).unwrap_or(p.cat);
                    if p.cat != c {
                        p.cat = c;
                        p.preview = None;
                        self.dirty = true;
                    }
                }
            }
            Hit::PalOp(id) => self.pal_hover(Some(id)),
            Hit::PalFrame | Hit::PalCat(_) => {}
            _ => self.pal_hover(None),
        }
    }

    /// Dessine la palette par-dessus tout et retient ses cibles.
    pub(crate) fn draw_tab_palette(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        th: &Theme,
        bx: &mut Vec<HBox>,
    ) {
        if self.palette.is_none() {
            return;
        }
        let ops = self.palette_ops();
        let l = self.lay;
        let area = Area {
            w: l.w,
            h: l.h,
            top: l.bar_h,
        };
        let hover = match self.hover {
            Hit::PalFrame => Some(PalHit::Frame),
            Hit::PalCat(k) => Some(PalHit::Cat(k)),
            Hit::PalOp(id) => Some(PalHit::Op(id)),
            _ => None,
        };
        let Some(p) = self.palette.as_ref() else {
            return;
        };
        let (r, cells) = cp_palette::draw(cv, text, th, p, &ops, hover, area);
        for (x, y, w, h, ph) in cells {
            bx.push((
                x,
                y,
                w,
                h,
                match ph {
                    PalHit::Frame => Hit::PalFrame,
                    PalHit::Cat(k) => Hit::PalCat(k),
                    PalHit::Op(id) => Hit::PalOp(id),
                },
            ));
        }
        if let Some(pp) = &mut self.palette {
            pp.rect = r;
        }
    }

    /// L'apercu de la palette : les tablatures qu'elle donnerait prennent la
    /// place des vraies le temps du dessin (`swap` deux fois : aller, retour).
    pub(crate) fn swap_preview(&mut self) {
        let Some(pv) = self.palette.as_mut().and_then(|p| p.preview.as_mut()) else {
            return;
        };
        let mut pv = std::mem::take(&mut pv.1);
        for (t, i, d) in &mut pv {
            if let Some(it) = self.tracks.get_mut(*t).and_then(|tr| tr.items.get_mut(*i)) {
                std::mem::swap(&mut it.doc, d);
                it.touch();
            }
        }
        if let Some(x) = self.palette.as_mut().and_then(|p| p.preview.as_mut()) {
            x.1 = pv;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(qn0: f64, len: f64, pitch: i32, string: usize, fret: i32) -> TNote {
        TNote {
            qn0,
            qn1: qn0 + len,
            pitch,
            string,
            fret,
            vel: 96,
            ..TNote::default()
        }
    }

    #[test]
    fn un_accord_pose_par_la_palette_prend_des_cordes_distinctes() {
        // Un La (corde de La a vide), choisi.
        let mut it = Item {
            doc: vec![note(0.0, 1.0, 45, 1, 0)],
            ..Item::default()
        };
        it.sel_notes.insert(it.doc[0].key());
        let mut r = roll_of(&it);
        let c = Ctx {
            host: Host::Tab,
            ..Ctx::default()
        };
        // L'accord majeur : La, Do#, Mi.
        let (k, _) = cp_palette::apply(71, &mut r, &c, 1);
        assert_eq!(k, 2);
        let (d, chosen, gone) = doc_of_roll(&it, &r, 120.0);
        assert_eq!(d.len(), 3);
        assert!(gone.is_empty());
        assert_eq!(chosen.len(), 3);
        let t = it.detect.tuning();
        let mut strings: Vec<usize> = d.iter().map(|n| n.string).collect();
        strings.sort_unstable();
        strings.dedup();
        assert_eq!(strings.len(), 3, "{d:?}");
        // La note d'origine n'a pas bouge ; chaque case est juste.
        assert!(
            d.iter()
                .any(|n| n.pitch == 45 && n.string == 1 && n.fret == 0)
        );
        for n in &d {
            assert_eq!(n.pitch, t.strings[n.string] + n.fret);
        }
    }

    #[test]
    fn la_grille_a_un_nom() {
        assert_eq!(grid_name(0.25), "1/16");
        assert_eq!(grid_name(1.0), "1/4");
        assert_eq!(grid_name(1.0 / 6.0), "1/16T");
    }
}
