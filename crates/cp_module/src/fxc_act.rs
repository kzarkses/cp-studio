//! FX Constellation : ce que font les gestes de la vue, les menus, ROLL,
//! les presets et snapshots, Init.

use std::collections::BTreeMap;

use cp_fxc::model::{MAX_SOURCES, Nav, PState, Pad, Track, pkey, split_pkey};
use cp_fxc::preset::{Preset, PresetFx, Snapshot, file_name, next_snapshot_name};
use cp_fxc::roll::{self, AXES, Axis, Settings};
use cp_fxc::slots::{Band, FxKind, band_of, bank, bridge, keys, long_label, sg};
use cp_fxcview::panels::{
    self, ARP_ORDERS, DIRS, ENV_SYNCS, KEYS, LANES, PAT_WHEN, RATES, ROOTS, SCALES, SEQ, SG,
    SPEEDS, WAVES,
};
use cp_fxcview::{Act, Ctl, PadF, View};
use cp_sys::fxc::Tk;
use cp_sys::ui::{self, MenuEntry};
use cp_ui::Mods;

use crate::App;

// ---------------------------------------------------------------------------
// Les preferences
// ---------------------------------------------------------------------------

pub(crate) fn encode_roll(s: &Settings) -> String {
    let armed: String = s.armed.iter().map(|&a| if a { '1' } else { '0' }).collect();
    format!(
        "armed={armed} min={} max={} by={} mo={} ex={} rmin={} rmax={} int={} bmin={} bmax={} lfo={} lsh={} lr={} ls={} lp={}",
        s.random_min,
        s.random_max,
        s.bypass_pct,
        s.mod_prob,
        s.exclusive_xy as u8,
        s.range_min,
        s.range_max,
        s.intensity,
        s.base_min,
        s.base_max,
        s.lfo_slots,
        s.lfo_shape as u8,
        s.lfo_rate as u8,
        s.lfo_sync as u8,
        s.lfo_phase as u8
    )
}

pub(crate) fn decode_roll(t: &str) -> Option<Settings> {
    let m: BTreeMap<&str, &str> = t
        .split_whitespace()
        .filter_map(|p| p.split_once('='))
        .collect();
    let d = Settings::default();
    let f = |k: &str, def: f64| m.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(def);
    let b = |k: &str, def: bool| m.get(k).map_or(def, |v| *v == "1");
    let mut armed = d.armed;
    if let Some(a) = m.get("armed") {
        for (i, c) in a.chars().take(8).enumerate() {
            armed[i] = c == '1';
        }
    }
    Some(Settings {
        armed,
        random_min: f("min", 3.0).clamp(1.0, 300.0) as usize,
        random_max: f("max", 8.0).clamp(1.0, 300.0) as usize,
        bypass_pct: f("by", d.bypass_pct).clamp(0.0, 1.0),
        mod_prob: f("mo", 0.0).clamp(0.0, 1.0),
        exclusive_xy: b("ex", true),
        range_min: f("rmin", 0.0).clamp(0.0, 1.0),
        range_max: f("rmax", 1.0).clamp(0.0, 1.0),
        intensity: f("int", d.intensity).clamp(0.0, 1.0),
        base_min: f("bmin", 0.0).clamp(0.0, 1.0),
        base_max: f("bmax", 1.0).clamp(0.0, 1.0),
        lfo_slots: f("lfo", 8.0).clamp(1.0, 8.0) as usize,
        lfo_shape: b("lsh", true),
        lfo_rate: b("lr", true),
        lfo_sync: b("ls", false),
        lfo_phase: b("lp", false),
    })
}

pub(crate) fn encode_view(v: &View) -> String {
    format!(
        "tab={} rack={} fold={} vals={} kb={} lw={} rw={}",
        v.tab,
        v.rack_tab,
        v.rack_fold as u8,
        v.show_values as u8,
        v.kb_open as u8,
        v.left_w,
        v.right_w
    )
}

pub(crate) fn decode_view(t: &str, v: &mut View) {
    for (k, val) in t.split_whitespace().filter_map(|p| p.split_once('=')) {
        let n = val.parse::<f32>().unwrap_or(0.0);
        match k {
            "tab" => v.tab = (n as u8).min(2),
            "rack" => v.rack_tab = (n as u8).min(2),
            "fold" => v.rack_fold = n > 0.5,
            "vals" => v.show_values = n > 0.5,
            "kb" => v.kb_open = n > 0.5,
            "lw" if n > 100.0 => v.left_w = n,
            "rw" if n > 100.0 => v.right_w = n,
            _ => {}
        }
    }
}

fn parse(s: Option<String>) -> Option<f64> {
    s.and_then(|v| v.trim().parse::<f64>().ok())
}

/// Les etendues proposees (portees, bases).
const SPANS: [(f64, f64); 6] = [
    (0.0, 1.0),
    (0.25, 0.75),
    (0.0, 0.5),
    (0.5, 1.0),
    (0.1, 0.4),
    (0.4, 0.9),
];

/// Un choix de menu : les identifiants de chaque famille.
const M_SRC: u32 = 1000;
const M_REMOVE: u32 = 2000;
const M_BYPASS: u32 = 3000;
/// « Modulate a depth » : 5000 + rang·1000 + (source + 500).
const M_DMOD: u32 = 5000;
const M_DCLEAR: u32 = 9900;

impl App {
    fn fxc_t(&self) -> Option<Tk> {
        self.fxc.tk.filter(|&t| self.r.xc_guid(t).is_some())
    }

    fn fxc_hwnd(&self) -> cp_sys::win::Hwnd {
        self.fxc
            .win
            .as_ref()
            .map_or(cp_sys::win::Hwnd(0), |w| w.hwnd)
    }

    /// Un menu a la souris.
    fn fxc_popup(&self, x: f32, y: f32, entries: &[MenuEntry]) -> u32 {
        let h = self.fxc_hwnd();
        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
        ui::popup_tree(h, sx, sy, entries)
    }

    /// (piste, effet, parametre) d'une cle du modele.
    fn fxc_loc(&self, key: &str) -> Option<(Tk, usize, usize)> {
        let t = self.fxc_t()?;
        let (g, p) = split_pkey(key)?;
        Some((t, self.fxc.chain.index_of(g)?, p))
    }

    /// Le lien de ce parametre est-il voulu (la base va dans le lien) ?
    fn fxc_linked(&self, key: &str) -> bool {
        let Some((_, fx, p)) = self.fxc_loc(key) else {
            return false;
        };
        self.fxc
            .plan
            .links
            .iter()
            .any(|l| l.fx == fx && l.param == p)
    }

    /// La base d'un parametre : le modele, puis le lien ou l'effet.
    pub(crate) fn fxc_set_base(&mut self, key: &str, v: f64) {
        let v = v.clamp(0.0, 1.0);
        let linked = self.fxc_linked(key);
        self.fxc.t.param_mut(key).base = v;
        if let Some((t, fx, p)) = self.fxc_loc(key) {
            if linked {
                self.r.xc_set_cfg(
                    t,
                    fx,
                    &format!("param.{p}.mod.baseline"),
                    &format!("{v:.9}"),
                );
            } else {
                self.r.xc_set(t, fx, p, v);
            }
        }
        self.fxc_changed();
    }

    /// Le parametre qu'un bouton d'un effet interne tient.
    fn fxc_ikey(&self, c: &Ctl) -> Option<String> {
        cp_fxcview::internal_key(c, &self.fxc_guids(), self.fxc.view.sel_slot)
    }

    /// Des sources menent-elles ce parametre ?
    fn fxc_modulated(&self, key: &str) -> bool {
        self.fxc
            .t
            .params
            .get(key)
            .is_some_and(|p| p.sel && !p.srcs.is_empty())
    }

    /// La valeur qu'un parametre a dans REAPER (sa base s'il est lie).
    fn fxc_read_norm(&self, key: &str) -> f64 {
        self.fxc_loc(key)
            .map_or(0.5, |(t, fx, p)| self.r.xc_get(t, fx, p))
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    pub(crate) fn fxc_act(&mut self, a: Act) {
        if self.fxc_act_dock(&a) {
            return;
        }
        match a {
            Act::Changed => self.fxc.prefs_dirty = true,
            Act::Click(c, m) => self.fxc_click(c, m),
            Act::Menu(c, x, y) => self.fxc_menu(c, x, y),
            Act::Value(c, v) => self.fxc_value(c, v),
            Act::Depth(c, d) => self.fxc_depth(c, d),
            Act::End(c) => {
                if let Ctl::Knob(k) = &c {
                    if let Some((t, fx, p)) = self.fxc_loc(k) {
                        self.r.xc_end_edit(t, fx, p);
                    }
                }
                self.fxc.dirty_at = Some(std::time::Instant::now());
            }
            Act::Reset(c) => self.fxc_reset(c),
            Act::Drop(slot, c) => self.fxc_drop(slot, c),
            Act::PadPress(x, y) => {
                self.fxc.t.pad.nav_on = false;
                self.fxc_pad_aim(x, y);
            }
            Act::PadDrag(x, y) => self.fxc_pad_aim(x, y),
            Act::PadRelease => self.fxc.dirty_at = Some(std::time::Instant::now()),
            Act::Zone(z) => {
                if (z[1] - z[0]).abs() > 0.03 && (z[3] - z[2]).abs() > 0.03 {
                    self.fxc.t.pad.zone = z;
                    let (x, y) = self
                        .fxc
                        .t
                        .pad
                        .clamp_to_zone(self.fxc.t.pad.x, self.fxc.t.pad.y);
                    self.fxc_pad_to(x, y);
                } else {
                    self.fxc.t.pad.zone = [0.0, 1.0, 0.0, 1.0];
                }
                self.fxc_changed();
            }
            Act::Range(lo, hi) => {
                self.fxc.t.pad.gmin = lo;
                self.fxc.t.pad.gmax = hi;
                self.fxc_changed();
            }
            Act::Step(c, k, v) => self.fxc_step(c, k, v),
            Act::Note(n, on) => {
                if on {
                    self.fxc_keyboard(Some(n));
                } else {
                    self.fxc_keyboard(None);
                }
            }
            Act::MoveFx(g, det) => self.fxc_set_detached(&g, det),
            Act::DockGrab | Act::DockSwitch(_) | Act::ViewMenu(..) => {}
        }
        self.fxc.view.mark();
    }

    /// La souris vise ce point : le pad y va (lisse : il la rattrape).
    fn fxc_pad_aim(&mut self, x: f64, y: f64) {
        if self.fxc.t.pad.smooth > 0.0 && !self.fxc.t.pad.granular {
            self.fxc.pad_target = Some(self.fxc.t.pad.clamp_to_zone(x, y));
        } else {
            self.fxc.pad_target = None;
            self.fxc_pad_to(x, y);
        }
    }

    /// Le pad a ce point (les axes menes par une source n'obeissent pas).
    pub(crate) fn fxc_pad_to(&mut self, x: f64, y: f64) {
        let (x, y) = self.fxc.t.pad.clamp_to_zone(x, y);
        let (lx, ly) = (self.fxc.plan.pad_link_x, self.fxc.plan.pad_link_y);
        if lx && ly {
            self.fxc_say("Pad X and Y follow modulators: click the X / Y pills below to free them");
            return;
        }
        if !lx {
            self.fxc.t.pad.x = x;
        }
        if !ly {
            self.fxc.t.pad.y = y;
        }
        if self.fxc.t.pad.granular {
            let vals = cp_fxc::pad::granular_values(
                &self.fxc.t.grains,
                self.fxc.t.pad.grid,
                self.fxc.t.pad.x,
                self.fxc.t.pad.y,
            );
            for (k, v) in vals {
                if self.fxc.t.params.get(&k).is_some_and(|p| p.sel) {
                    self.fxc_set_base(&k, v);
                }
            }
            return;
        }
        let Some(t) = self.fxc_t() else { return };
        if let Some(b) = self.fxc.chain.find(FxKind::Bridge) {
            if !lx {
                self.r.xc_set_raw(t, b, bridge::X_POS, self.fxc.t.pad.x);
            }
            if !ly {
                self.r.xc_set_raw(t, b, bridge::Y_POS, self.fxc.t.pad.y);
            }
        } else if self.fxc.t.tracked_count() > 0 {
            self.fxc_ensure_bridge();
            self.fxc.links_dirty = true;
        }
    }

    fn fxc_click(&mut self, c: Ctl, m: Mods) {
        if m.alt || m.shift {
            if let Some(k) = self.fxc_ikey(&c) {
                self.fxc_click(Ctl::Knob(k), m);
                return;
            }
        }
        match c {
            Ctl::PresetPrev | Ctl::PresetNext => {
                let list = self.fxc_preset_list();
                if list.is_empty() {
                    self.fxc_say("no preset yet");
                    return;
                }
                let cur = list.iter().position(|n| *n == self.fxc.t.preset);
                let n = list.len() as i64;
                let i = match (cur, c == Ctl::PresetNext) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i as i64 + 1) % n,
                    (Some(i), false) => (i as i64 - 1 + n) % n,
                };
                let name = list[i as usize].clone();
                self.fxc_load_preset(&name);
            }
            Ctl::PresetSave => {
                let name = if self.fxc.t.preset.is_empty() {
                    self.fxc_ask("Save the FX Constellation preset", "Name", "")
                } else {
                    Some(self.fxc.t.preset.clone())
                };
                if let Some(n) = name {
                    self.fxc_save_preset(&n);
                }
            }
            Ctl::SnapPrev | Ctl::SnapNext => {
                let Some(p) = &self.fxc.preset else {
                    self.fxc_say("A snapshot lives inside a preset");
                    return;
                };
                let list: Vec<String> = p.snapshots.keys().cloned().collect();
                if list.is_empty() {
                    return;
                }
                let cur = list.iter().position(|n| *n == self.fxc.t.snapshot);
                let n = list.len() as i64;
                let i = match (cur, c == Ctl::SnapNext) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i as i64 + 1) % n,
                    (Some(i), false) => (i as i64 - 1 + n) % n,
                };
                let name = list[i as usize].clone();
                self.fxc_recall_snapshot(&name);
            }
            Ctl::SnapAdd => {
                if let Some(p) = &self.fxc.preset {
                    let n = next_snapshot_name(p);
                    self.fxc_capture_snapshot(&n);
                }
            }
            Ctl::Lock => {
                self.fxc.locked = !self.fxc.locked;
            }
            Ctl::Granular(on) => {
                self.fxc.t.pad.granular = on;
                if on && self.fxc.t.grains.is_empty() {
                    self.fxc_init_grains();
                }
                self.fxc_changed();
            }
            Ctl::Nav(i) => {
                let pad = &mut self.fxc.t.pad;
                pad.nav = Nav::from_i(i as i64);
                pad.nav_on = pad.nav != Nav::Manual;
                if pad.nav_on {
                    self.fxc_ensure_bridge();
                }
                self.fxc_changed();
            }
            Ctl::Settings => self.fxc_settings_menu(),
            Ctl::Help => {
                self.r.xc_message(
                    "FX Constellation",
                    "Track params (the square at the left of a row), then move the pad: each tracked \
                     param moves from its base by its range, on the X and/or Y axis of its chips.\n\n\
                     Knob: drag = base, Alt+drag = depth, Shift = fine, double-click = back to the \
                     value it had, Alt+click = release its sources, Shift+click = bypass them.\n\n\
                     Sources (right): drag a dot onto a knob to modulate it. Up to four sources per \
                     param, summed by the Mod Mix bus.\n\nROLL rolls every armed axis \
                     (P B R XY N BY OR MO); right-click an axis to roll it alone.\n\n\
                     Right-drag on the pad draws a zone.",
                    0,
                );
            }
            Ctl::ZoneClear => {
                self.fxc.t.pad.zone = [0.0, 1.0, 0.0, 1.0];
                self.fxc_changed();
            }
            Ctl::PadReset => {
                let p = &mut self.fxc.t.pad;
                p.x = 0.5;
                p.y = 0.5;
                p.ax = 0.5;
                p.ay = 0.5;
                self.fxc_pad_to(0.5, 0.5);
                self.fxc_changed();
            }
            Ctl::WalkJump => {
                self.fxc.t.pad.walk_jump = !self.fxc.t.pad.walk_jump;
                self.fxc_changed();
            }
            Ctl::Figure(i) => {
                self.fxc.t.pad.fig_mode = i;
                self.fxc.t.pad.nav_on = true;
                self.fxc_changed();
            }
            Ctl::GrainRoll => {
                let keys: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
                let pad = self.fxc.t.pad.clone();
                cp_fxc::pad::reroll_grains(&mut self.fxc.t.grains, &keys, &pad, &mut self.fxc.rng);
                self.fxc_changed();
            }
            Ctl::Roll => self.fxc_roll(None),
            Ctl::RollAxis(i) => {
                let i = i as usize;
                self.fxc.roll.armed[i] = !self.fxc.roll.armed[i];
                self.fxc.prefs_dirty = true;
            }
            Ctl::MacroDot(n) => self.fxc.view.sel_slot = 210 + n as i32,
            Ctl::Tab(i) => {
                self.fxc.view.tab = i;
                self.fxc.prefs_dirty = true;
            }
            Ctl::AddFx(det) => {
                if det {
                    self.fxc.next_detached = Some(self.fxc.chain.fx.len());
                }
                if !self.fxc.guid.is_empty() {
                    let g = self.fxc.guid.clone();
                    self.fxb_open_for(&g);
                }
            }
            Ctl::CollapseAll => {
                let all: Vec<String> = self.fxc.rows.iter().map(|r| r.guid.clone()).collect();
                if all.iter().all(|g| self.fxc.view.folded.contains(g)) {
                    self.fxc.view.folded.clear();
                } else {
                    self.fxc.view.folded.extend(all);
                }
            }
            Ctl::UntrackAll => {
                for p in self.fxc.t.params.values_mut() {
                    p.sel = false;
                }
                self.fxc_changed();
            }
            Ctl::Values => {
                self.fxc.view.show_values = !self.fxc.view.show_values;
                self.fxc.prefs_dirty = true;
            }
            Ctl::FxHead(g) => {
                if !self.fxc.view.folded.remove(&g) {
                    self.fxc.view.folded.insert(g);
                }
            }
            Ctl::FxVerb(g, k) => self.fxc_fx_verb(&g, k),
            Ctl::Sel(k) => {
                let on = !self.fxc.t.params.get(&k).is_some_and(|p| p.sel);
                let read = self.fxc_read_norm(&k);
                let ps = self.fxc.t.param_mut(&k);
                ps.sel = on;
                if on && ps.srcs.is_empty() {
                    ps.base = read;
                    if self.fxc.roll.exclusive_xy && ps.x && ps.y {
                        ps.y = false;
                    }
                }
                if on && !self.fxc.t.pad.granular {
                    self.fxc_ensure_bridge();
                }
                self.fxc_count("track");
                self.fxc_changed();
            }
            Ctl::Knob(k) => {
                let ps = self.fxc.t.param_mut(&k);
                if m.alt && !ps.srcs.is_empty() {
                    ps.srcs.clear();
                    self.fxc_changed();
                } else if m.shift && !ps.srcs.is_empty() {
                    let all = ps.srcs.iter().all(|s| s.off);
                    for s in ps.srcs.iter_mut() {
                        s.off = !all;
                    }
                    self.fxc_changed();
                }
            }
            Ctl::Chip(k, i) => {
                let ex = self.fxc.roll.exclusive_xy;
                let ps = self.fxc.t.param_mut(&k);
                match i {
                    0 => {
                        ps.x = !ps.x;
                        if ex && ps.x {
                            ps.y = false;
                        }
                    }
                    1 => {
                        ps.y = !ps.y;
                        if ex && ps.y {
                            ps.x = false;
                        }
                    }
                    _ => ps.inv = !ps.inv,
                }
                self.fxc_changed();
            }
            Ctl::Src(k) => {
                let (x, y) = self.fxc.view.mouse;
                self.fxc_source_menu(&k, x, y);
            }
            Ctl::RackTab(i) => {
                self.fxc.view.rack_tab = i;
                self.fxc.view.sel_slot = if i == 1 { 101 } else { 1 };
                self.fxc.prefs_dirty = true;
            }
            Ctl::Map => self.fxc.view.map = !self.fxc.view.map,
            Ctl::Fold => {
                self.fxc.view.rack_fold = !self.fxc.view.rack_fold;
                self.fxc.view.map = false;
                self.fxc.prefs_dirty = true;
            }
            Ctl::Tile(s) => {
                self.fxc.view.sel_slot = s;
                match band_of(s) {
                    Some(Band::Env(1)) => self.fxc.view.tab = 1,
                    Some(Band::Seq(_)) => self.fxc.view.tab = 2,
                    _ => {}
                }
            }
            Ctl::AddBank => {
                if self.fxc.view.rack_tab == 1 {
                    self.fxc_ensure_global_send();
                } else {
                    self.fxc_ensure_bank();
                }
                self.fxc_read_values(true);
            }
            Ctl::SlotOn => self.fxc_slot_write(SlotW::On(None)),
            Ctl::SlotPol(uni) => {
                self.fxc_slot_write(SlotW::Raw(SlotField::Uni, uni as u8 as f64));
                self.fxc_changed();
            }
            Ctl::SlotRetrig => self.fxc_slot_write(SlotW::Raw(SlotField::Trig, 1.0)),
            Ctl::SlotRnd => {
                let rr = roll::roll_slot(&self.fxc.roll, &mut self.fxc.rng);
                self.fxc_slot_apply_roll(self.fxc.view.sel_slot, &rr);
            }
            Ctl::StepRnd => {
                if let Some((s, _)) = self.fxc_sel_slot_state() {
                    let mut st = s.steps;
                    for v in st.iter_mut().take(s.len as usize) {
                        *v = self.fxc.rng.int(0, 63) as u8;
                    }
                    self.fxc_slot_write(SlotW::Steps(st));
                }
            }
            Ctl::LinkRemove(k, i) => {
                self.fxc.t.param_mut(&k).remove_source(i);
                self.fxc_changed();
            }
            Ctl::DmodRemove(k, i, j) => {
                if let Some(s) = self.fxc.t.param_mut(&k).srcs.get_mut(i) {
                    if j < s.dmods.len() {
                        s.dmods.remove(j);
                    }
                }
                self.fxc_changed();
            }
            Ctl::Pill(axis) => {
                if axis == 0 {
                    self.fxc.t.pad.src_x = 0;
                } else {
                    self.fxc.t.pad.src_y = 0;
                }
                self.fxc_changed();
            }
            Ctl::StopEngines => {
                let mut n = 0;
                for i in 0..self.r.xc_track_count() {
                    let Some(t) = self.r.xc_track_index(i) else {
                        continue;
                    };
                    for fx in 0..self.r.xc_fx_count(t) {
                        if self.r.xc_fx_name(t, fx).contains(bridge::NAME)
                            && self.r.xc_param_count(t, fx) >= 31
                            && self.r.xc_get_raw(t, fx, bridge::ENG_MODE).0 > 0.5
                        {
                            self.r.xc_set_raw(t, fx, bridge::ENG_MODE, 0.0);
                            n += 1;
                        }
                    }
                }
                self.fxc.t.pad.nav_on = false;
                self.fxc.eng_cache.clear();
                self.fxc_say(if n == 0 {
                    "no engine running".to_string()
                } else {
                    format!("{n} engine(s) stopped")
                });
            }
            Ctl::ReleaseLinks => {
                let n = self.fxc.plan.links.len();
                for p in self.fxc.t.params.values_mut() {
                    p.sel = false;
                    p.srcs.clear();
                }
                self.fxc.t.pad.src_x = 0;
                self.fxc.t.pad.src_y = 0;
                self.fxc_changed();
                self.fxc_say(if n == 0 {
                    "no link to release".to_string()
                } else {
                    format!("{n} link(s) released")
                });
            }
            Ctl::JsToggle(panel, p) => {
                if let Some((t, fx)) = self.fxc_js(panel) {
                    let v = self.r.xc_get_raw(t, fx, p).0;
                    self.r.xc_set_raw(t, fx, p, if v > 0.5 { 0.0 } else { 1.0 });
                    self.fxc_read_values(true);
                }
            }
            Ctl::JsSet(panel, p, i) => {
                if let Some((t, fx)) = self.fxc_js(panel) {
                    self.r.xc_set_raw(t, fx, p, i as f64);
                    self.fxc_read_values(true);
                }
            }
            Ctl::SgOn => self.fxc_sg_toggle(),
            Ctl::SeqAdd => self.fxc_seq_add(),
            Ctl::SeqRemove => self.fxc_seq_remove(),
            Ctl::SeqPattern(k) => self.fxc_seq_request(1, k as u32),
            Ctl::SeqRoll(l) => self.fxc_seq_roll(1 << l),
            Ctl::SeqRollAll => self.fxc_seq_roll(511),
            Ctl::KbFold => {
                self.fxc.view.kb_open = !self.fxc.view.kb_open;
                self.fxc.prefs_dirty = true;
            }
            // Les choix a plusieurs valeurs s'ouvrent en menu au clic.
            c @ (Ctl::WalkSync
            | Ctl::GrainGrid
            | Ctl::SlotType
            | Ctl::SlotShape
            | Ctl::SlotSync
            | Ctl::SlotMode
            | Ctl::SlotSh
            | Ctl::JsMenu(..)
            | Ctl::PresetField
            | Ctl::SnapField) => {
                let (x, y) = self.fxc.view.mouse;
                self.fxc_menu(c, x, y);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Valeurs, profondeurs, retours
    // -----------------------------------------------------------------------

    fn fxc_value(&mut self, c: Ctl, v: f64) {
        if let Some(k) = self.fxc_ikey(&c) {
            let own = match &c {
                Ctl::Js(_, p) => k.ends_with(&format!("#{p}")),
                _ => true,
            };
            if own && self.fxc_modulated(&k) {
                self.fxc_set_base(&k, v);
                return;
            }
        }
        match c {
            Ctl::Knob(k) => self.fxc_set_base(&k, v),
            Ctl::PadField(f) => {
                let p = &mut self.fxc.t.pad;
                match f {
                    PadF::Smooth => p.smooth = v,
                    PadF::MaxSpeed => p.max_speed = 0.1 + v * 9.9,
                    PadF::WalkSpeed => p.walk_speed = 0.1 + v * 9.9,
                    PadF::WalkJitter => p.walk_jitter = v,
                    PadF::FigSpeed => p.fig_speed = 0.01 + v * 9.99,
                    PadF::FigSize => p.fig_size = 0.1 + v * 0.9,
                }
                self.fxc_changed();
            }
            Ctl::Morph => {
                self.fxc.morph = v;
                self.fxc_apply_morph(v);
            }
            Ctl::Macro(n) => {
                if let (Some(t), Some(b)) = (self.fxc_t(), self.fxc.chain.find(FxKind::Bridge)) {
                    self.r
                        .xc_set_raw(t, b, bridge::MACRO_IN + n as usize - 1, v);
                    self.fxc.macros[n as usize - 1] = v;
                }
            }
            Ctl::AxisKnob(a) => {
                let p = &mut self.fxc.t.pad;
                if a == 0 {
                    p.x = v;
                    p.ax = v;
                } else {
                    p.y = v;
                    p.ay = v;
                }
                let (x, y) = (p.x, p.y);
                self.fxc_pad_to(x, y);
                self.fxc_changed();
            }
            Ctl::SlotKnob(k) => {
                let f = match k {
                    0 => SlotW::Norm(SlotField::Rate, v),
                    1 => SlotW::Raw(SlotField::Phase, v),
                    2 => SlotW::Raw(SlotField::Curve, v * 2.0 - 1.0),
                    _ => SlotW::Norm(SlotField::Slew, v),
                };
                self.fxc_slot_write(f);
            }
            Ctl::StepLen => {
                self.fxc_slot_write(SlotW::Raw(SlotField::Len, (1.0 + v * 15.0).round()));
            }
            Ctl::LinkDepth(k, i) => {
                let g = self.fxc.t.pad.gesture_range;
                self.fxc.t.param_mut(&k).set_span(i, v * 2.0 - 1.0, g);
                self.fxc_changed();
            }
            Ctl::DmodDepth(k, i, j) => {
                if let Some(d) = self
                    .fxc
                    .t
                    .param_mut(&k)
                    .srcs
                    .get_mut(i)
                    .and_then(|s| s.dmods.get_mut(j))
                {
                    d.depth = (v * 2.0 - 1.0).clamp(-1.0, 1.0);
                }
                self.fxc_changed();
            }
            Ctl::Js(panel, p) => self.fxc_js_value(panel, p, v),
            Ctl::SeqLen(l) => {
                if let Some((t, fx)) = self.fxc_js(SEQ) {
                    let len = (1.0 + v * 15.0).round();
                    self.r
                        .xc_set_raw(t, fx, panels::lane_base(l as usize) + 16, len);
                    self.fxc_read_values(true);
                }
            }
            Ctl::SgHold => {
                if let Some((t, fx)) = self.fxc_js(SG) {
                    self.r.xc_set(t, fx, 16, v);
                }
            }
            _ => {}
        }
    }

    fn fxc_depth(&mut self, c: Ctl, d: f64) {
        let k = match c {
            Ctl::Knob(k) => k,
            other => match self.fxc_ikey(&other) {
                Some(k) => k,
                None => return,
            },
        };
        let g = self.fxc.t.pad.gesture_range;
        if self.fxc.view.map {
            // Map : la profondeur de la source choisie dans le rack.
            let slot = self.fxc.view.sel_slot;
            if slot == 0 {
                return;
            }
            let ps = self.fxc.t.param_mut(&k);
            let i = match ps.index_of(slot) {
                Some(i) => i,
                None => {
                    if d.abs() < 0.005 {
                        return;
                    }
                    if ps.add_source(slot).is_err() {
                        return;
                    }
                    ps.srcs.len() - 1
                }
            };
            ps.set_span(i, d, g);
            self.fxc_arm(slot);
            self.fxc_changed();
            return;
        }
        let ps = self.fxc.t.param_mut(&k);
        if ps.srcs.len() > 1 {
            // Plusieurs sources : toutes ensemble, en proportion.
            let old = ps.span(0, g);
            let ratio = if old.abs() < 1e-6 { 1.0 } else { d / old };
            for i in 1..ps.srcs.len() {
                ps.srcs[i].depth = (ps.srcs[i].depth * ratio).clamp(-1.0, 1.0);
            }
        }
        ps.set_span(0, d, g);
        self.fxc_changed();
    }

    fn fxc_reset(&mut self, c: Ctl) {
        match c {
            Ctl::Knob(k) => {
                if let Some(v) = self.fxc.scan.get(&k).copied() {
                    self.fxc_set_base(&k, v);
                }
            }
            Ctl::LinkDepth(k, i) => {
                let g = self.fxc.t.pad.gesture_range;
                self.fxc.t.param_mut(&k).set_span(i, 0.0, g);
                self.fxc_changed();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Les sources
    // -----------------------------------------------------------------------

    /// Rend une source disponible (la banque posee, le slot allume, la
    /// piste CP MOD reliee).
    fn fxc_arm(&mut self, slot: i32) {
        match band_of(slot) {
            Some(Band::Lfo(n)) => {
                if self.fxc_ensure_bank().is_some() {
                    let on = self
                        .fxc
                        .bank
                        .as_ref()
                        .is_some_and(|b| b.slots[n as usize - 1].on);
                    if !on {
                        self.fxc_slot_write_at(slot, SlotW::On(Some(true)));
                    }
                }
            }
            Some(Band::Global(n)) => {
                self.fxc_ensure_global_send();
                let on = self
                    .fxc
                    .gbank
                    .as_ref()
                    .is_some_and(|b| b.slots[n as usize - 1].on);
                if !on {
                    self.fxc_slot_write_at(slot, SlotW::On(Some(true)));
                }
            }
            Some(Band::Macro(_)) | Some(Band::PadX) | Some(Band::PadY) => {
                self.fxc_ensure_bridge();
            }
            Some(Band::Env(_)) if self.fxc.chain.find(FxKind::SoundGen).is_none() => {
                self.fxc_say("no Sound Gen on this track");
            }
            Some(Band::Seq(_)) if self.fxc.chain.find(FxKind::Seq).is_none() => {
                self.fxc_say("no sequencer on this track");
            }
            _ => {}
        }
    }

    fn fxc_drop(&mut self, slot: i32, c: Ctl) {
        match c {
            Ctl::Knob(k) | Ctl::Src(k) | Ctl::Sel(k) => {
                if slot < 0 {
                    // Un axe du pad : le parametre suit le pad sur cet axe.
                    let ps = self.fxc.t.param_mut(&k);
                    let pad_only = ps.sel && ps.srcs.is_empty();
                    ps.srcs.clear();
                    ps.sel = true;
                    if !pad_only {
                        ps.x = false;
                        ps.y = false;
                    }
                    if slot == -1 {
                        ps.x = true;
                    } else {
                        ps.y = true;
                    }
                    self.fxc_ensure_bridge();
                } else {
                    let read = self.fxc_read_norm(&k);
                    let ps = self.fxc.t.param_mut(&k);
                    if !ps.sel && ps.srcs.is_empty() {
                        ps.base = read;
                        ps.x = false;
                        ps.y = false;
                    }
                    if let Err(e) = ps.add_source(slot) {
                        self.fxc_say(e);
                        return;
                    }
                    self.fxc_arm(slot);
                }
                self.fxc_count("drop");
                self.fxc_changed();
            }
            Ctl::Pill(axis) => {
                if slot <= 0 {
                    self.fxc_say("a pad axis cannot drive the pad");
                    return;
                }
                if axis == 0 {
                    self.fxc.t.pad.src_x = slot;
                } else {
                    self.fxc.t.pad.src_y = slot;
                }
                self.fxc_arm(slot);
                self.fxc_ensure_bridge();
                self.fxc_changed();
            }
            c @ (Ctl::Macro(_) | Ctl::SlotKnob(_) | Ctl::Js(..)) => {
                if cp_fxcview::self_source(&c, slot, self.fxc.view.sel_slot) {
                    self.fxc_say("a source cannot drive its own settings");
                    return;
                }
                if let Ctl::Macro(_) = c {
                    self.fxc_ensure_bridge();
                }
                let Some(k) = self.fxc_ikey(&c) else {
                    self.fxc_say(match c {
                        Ctl::SlotKnob(_) => {
                            "a global source's settings follow only the global bank"
                        }
                        Ctl::Js(SEQ, _) => "this sequencer setting cannot be modulated",
                        _ => "nothing to modulate here",
                    });
                    return;
                };
                let read = self.fxc_read_norm(&k);
                let ps = self.fxc.t.param_mut(&k);
                if !ps.sel && ps.srcs.is_empty() {
                    ps.base = read;
                    ps.x = false;
                    ps.y = false;
                    // Un reglage de source module : une portee moderee.
                    ps.range = 0.5;
                }
                if let Err(e) = ps.add_source(slot) {
                    self.fxc_say(e);
                    return;
                }
                self.fxc_arm(slot);
                self.fxc_count("drop_internal");
                self.fxc_changed();
            }
            _ => {}
        }
    }

    fn fxc_source_menu(&mut self, key: &str, x: f32, y: f32) {
        let ps = self.fxc.t.params.get(key).cloned().unwrap_or_default();
        let g = self.fxc.t.pad.gesture_range;
        let mut e = Vec::new();
        let name = self
            .fxc
            .rows
            .iter()
            .flat_map(|r| r.params.iter())
            .find(|p| p.key == key)
            .map_or("Parameter".to_string(), |p| p.name.clone());
        e.push(MenuEntry::off(name));
        if !ps.srcs.is_empty() {
            for (i, s) in ps.srcs.iter().enumerate() {
                e.push(MenuEntry::check(
                    M_BYPASS + i as u32,
                    format!("{}   {:+.0} %", long_label(s.slot), ps.span(i, g) * 100.0),
                    !s.off,
                ));
            }
            let mut rm: Vec<MenuEntry> = ps
                .srcs
                .iter()
                .enumerate()
                .map(|(i, s)| MenuEntry::item(M_REMOVE + i as u32, long_label(s.slot)))
                .collect();
            rm.push(MenuEntry::Sep);
            rm.push(MenuEntry::item(M_REMOVE + 99, "All sources"));
            e.push(MenuEntry::Sub("Remove source".into(), rm));
            // Moduler la profondeur d'un lien par une autre source.
            let cands: Vec<(i32, String)> = (1..=8)
                .map(|n| (n, format!("CP LFO {n}")))
                .chain((101..=108).map(|n| (n, format!("Global LFO {}", n - 100))))
                .chain((211..=218).map(|n| (n, self.fxc.t.macro_name((n - 210) as usize))))
                .chain((201..=204).map(|n| (n, long_label(n))))
                .chain((221..=222).map(|n| (n, long_label(n))))
                .chain((231..=234).map(|n| (n, long_label(n))))
                .chain([(-1, "Pad X".to_string()), (-2, "Pad Y".to_string())])
                .collect();
            let dm: Vec<MenuEntry> = ps
                .srcs
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    MenuEntry::Sub(
                        format!("Depth of {}", long_label(s.slot)),
                        cands
                            .iter()
                            .map(|(c, l)| {
                                MenuEntry::check(
                                    M_DMOD + i as u32 * 1000 + (c + 500) as u32,
                                    l.clone(),
                                    s.dmods.iter().any(|d| d.slot == *c),
                                )
                            })
                            .collect(),
                    )
                })
                .collect();
            e.push(MenuEntry::Sub("Modulate a depth".into(), dm));
            if ps.srcs.iter().any(|s| !s.dmods.is_empty()) {
                e.push(MenuEntry::item(M_DCLEAR, "Clear the depth modulations"));
            }
            e.push(MenuEntry::Sep);
        }
        let pad_only = ps.srcs.is_empty() && ps.sel;
        e.push(MenuEntry::check(
            10,
            "Follow Pad X",
            pad_only && ps.x && !ps.y,
        ));
        e.push(MenuEntry::check(
            11,
            "Follow Pad Y",
            pad_only && ps.y && !ps.x,
        ));
        e.push(MenuEntry::check(
            12,
            "Follow Pad XY",
            pad_only && ps.x && ps.y,
        ));
        let has = |s: i32| ps.srcs.iter().any(|x| x.slot == s);
        let family = |name: &str, slots: Vec<(i32, String)>| {
            MenuEntry::Sub(
                name.into(),
                slots
                    .into_iter()
                    .map(|(s, l)| MenuEntry::check(M_SRC + (s + 500) as u32, l, has(s)))
                    .collect(),
            )
        };
        e.push(family(
            "CP LFO",
            (1..=8).map(|n| (n, format!("LFO {n}"))).collect(),
        ));
        e.push(family(
            "Global LFO",
            (101..=108)
                .map(|n| (n, format!("Global {}", n - 100)))
                .collect(),
        ));
        e.push(family(
            "Macro",
            (211..=218)
                .map(|n| (n, self.fxc.t.macro_name((n - 210) as usize)))
                .collect(),
        ));
        e.push(family(
            "Note Envelope",
            (201..=204).map(|n| (n, long_label(n))).collect(),
        ));
        e.push(family(
            "Sequencer",
            (221..=222).map(|n| (n, long_label(n))).collect(),
        ));
        e.push(family(
            "Note",
            (231..=234).map(|n| (n, long_label(n))).collect(),
        ));
        let id = self.fxc_popup(x, y, &e);
        let ps = self.fxc.t.param_mut(key);
        match id {
            0 => return,
            10..=12 => {
                ps.srcs.clear();
                ps.sel = true;
                ps.x = id != 11;
                ps.y = id != 10;
                self.fxc_ensure_bridge();
            }
            i if (M_BYPASS..M_BYPASS + MAX_SOURCES as u32).contains(&i) => {
                if let Some(s) = ps.srcs.get_mut((i - M_BYPASS) as usize) {
                    s.off = !s.off;
                }
            }
            i if i == M_REMOVE + 99 => ps.srcs.clear(),
            i if (M_REMOVE..M_REMOVE + MAX_SOURCES as u32).contains(&i) => {
                ps.remove_source((i - M_REMOVE) as usize)
            }
            M_DCLEAR => {
                for s in ps.srcs.iter_mut() {
                    s.dmods.clear();
                }
            }
            i if (M_DMOD..M_DMOD + 4000).contains(&i) => {
                let r = (i - M_DMOD) / 1000;
                let slot = ((i - M_DMOD) % 1000) as i32 - 500;
                if let Some(s) = ps.srcs.get_mut(r as usize) {
                    match s.dmods.iter().position(|d| d.slot == slot) {
                        Some(j) => {
                            s.dmods.remove(j);
                        }
                        None if s.dmods.len() < MAX_SOURCES => {
                            s.dmods.push(cp_fxc::model::Src::new(slot, 0.25));
                            self.fxc_arm(slot);
                            self.fxc_count("dmod");
                        }
                        None => self.fxc_say("four sources at most on one depth"),
                    }
                }
            }
            i if (M_SRC..M_SRC + 1000).contains(&i) => {
                let slot = i as i32 - M_SRC as i32 - 500;
                match ps.index_of(slot) {
                    Some(k) => ps.remove_source(k),
                    None => {
                        let read = self.fxc_read_norm(key);
                        let ps = self.fxc.t.param_mut(key);
                        if !ps.sel && ps.srcs.is_empty() {
                            ps.base = read;
                            ps.x = false;
                            ps.y = false;
                        }
                        if let Err(e) = ps.add_source(slot) {
                            self.fxc_say(e);
                            return;
                        }
                        self.fxc_arm(slot);
                    }
                }
            }
            _ => {}
        }
        self.fxc_changed();
    }

    // -----------------------------------------------------------------------
    // Les menus
    // -----------------------------------------------------------------------

    fn fxc_menu(&mut self, c: Ctl, x: f32, y: f32) {
        match c {
            Ctl::Knob(k) | Ctl::Src(k) | Ctl::Sel(k) | Ctl::Chip(k, _) => {
                self.fxc_source_menu(&k, x, y)
            }
            Ctl::RollAxis(i) => {
                if let Some(a) = AXES.get(i as usize) {
                    self.fxc_roll(Some(*a));
                }
            }
            c @ (Ctl::Js(..) | Ctl::SlotKnob(_))
                if self.fxc_ikey(&c).is_some_and(|k| self.fxc_modulated(&k)) =>
            {
                if let Some(k) = self.fxc_ikey(&c) {
                    self.fxc_source_menu(&k, x, y);
                }
            }
            Ctl::Macro(n)
                if self
                    .fxc_ikey(&Ctl::Macro(n))
                    .is_some_and(|k| self.fxc_modulated(&k)) =>
            {
                if let Some(k) = self.fxc_ikey(&Ctl::Macro(n)) {
                    self.fxc_source_menu(&k, x, y);
                }
            }
            Ctl::Macro(n) | Ctl::MacroDot(n) => {
                let cur = self.fxc.t.macro_names[n as usize - 1].clone();
                if let Some(s) = self.fxc_ask("Rename the macro", "Name", &cur) {
                    self.fxc.t.macro_names[n as usize - 1] = s;
                    self.fxc_changed();
                }
            }
            Ctl::FxHead(g) | Ctl::FxVerb(g, _) => self.fxc_fx_menu(&g, x, y),
            Ctl::PresetField => self.fxc_preset_menu(x, y),
            Ctl::SnapField => self.fxc_snap_menu(x, y),
            Ctl::WalkSync => {
                let cur = self.fxc.t.pad.walk_sync;
                let e: Vec<MenuEntry> = cp_fxc::model::WALK_SYNCS
                    .iter()
                    .enumerate()
                    .map(|(i, l)| MenuEntry::check(i as u32 + 1, *l, cur == i as u8))
                    .collect();
                let id = self.fxc_popup(x, y, &e);
                if id > 0 {
                    self.fxc.t.pad.walk_sync = (id - 1) as u8;
                    self.fxc_changed();
                }
            }
            Ctl::GrainGrid => {
                let cur = self.fxc.t.pad.grid;
                let e: Vec<MenuEntry> = (2..=4)
                    .map(|g| MenuEntry::check(g, format!("{g}x{g}"), cur as u32 == g))
                    .collect();
                let id = self.fxc_popup(x, y, &e);
                if id > 0 {
                    self.fxc.t.pad.grid = id as u8;
                    self.fxc_init_grains();
                    self.fxc_changed();
                }
            }
            Ctl::SlotType | Ctl::SlotShape | Ctl::SlotSync | Ctl::SlotMode | Ctl::SlotSh => {
                self.fxc_slot_menu(c, x, y)
            }
            Ctl::Tile(s) if (231..=234).contains(&s) => self.fxc_notes_menu(s, x, y),
            Ctl::Tile(s) => {
                self.fxc.view.sel_slot = s;
                if let Some(Band::Lfo(_)) | Some(Band::Global(_)) = band_of(s) {
                    self.fxc_slot_menu(Ctl::SlotType, x, y);
                }
            }
            Ctl::JsMenu(panel, p) => self.fxc_js_menu(panel, p, x, y),
            Ctl::SeqLane(l) => self.fxc_lane_menu(l, x, y),
            Ctl::SeqPattern(k) => {
                let e = vec![
                    MenuEntry::off(format!("Pattern {}", k + 1)),
                    MenuEntry::item(1, "Copy to the current pattern"),
                    MenuEntry::item(2, "Clear"),
                ];
                match self.fxc_popup(x, y, &e) {
                    1 => {
                        let cur = self.fxc.seq.as_ref().map_or(0, |j| j.r(219) as u32);
                        self.fxc_seq_request(2, (k as u32) * 16 + cur);
                    }
                    2 => self.fxc_seq_request(3, k as u32),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn fxc_fx_menu(&mut self, g: &str, x: f32, y: f32) {
        let Some(t) = self.fxc_t() else { return };
        let Some(fx) = self.fxc.chain.index_of(g) else {
            return;
        };
        let row = self.fxc.rows.iter().find(|r| r.guid == g).cloned();
        let det = row.as_ref().is_some_and(|r| r.detached);
        let only = self.fxc.t.fx_tracked_only.get(g).copied().unwrap_or(false);
        let max = self.fxc.t.fx_random_max.get(g).copied().unwrap_or(3);
        let e = vec![
            MenuEntry::off(row.as_ref().map_or("FX".to_string(), |r| r.name.clone())),
            MenuEntry::check(1, "Keep out of ROLL (detached)", det),
            MenuEntry::check(2, "Show tracked only", only),
            MenuEntry::Sep,
            MenuEntry::item(3, "Track all"),
            MenuEntry::item(4, "Track continuous only"),
            MenuEntry::item(5, "Track none"),
            MenuEntry::item(6, "Track at random"),
            MenuEntry::Sub(
                "Random count".into(),
                (1..=10)
                    .map(|n| MenuEntry::check(100 + n, n.to_string(), max as u32 == n))
                    .collect(),
            ),
            MenuEntry::Sep,
            MenuEntry::item(7, "Roll X / Y here"),
            MenuEntry::item(8, "Roll ranges here"),
            MenuEntry::item(9, "Roll bases here"),
            MenuEntry::Sep,
            MenuEntry::item(10, "Open the plugin window"),
        ];
        let id = self.fxc_popup(x, y, &e);
        let keys: Vec<String> = row
            .as_ref()
            .map(|r| r.params.iter().map(|p| p.key.clone()).collect())
            .unwrap_or_default();
        match id {
            1 => self.fxc_set_detached(g, !det),
            2 => {
                self.fxc.t.fx_tracked_only.insert(g.to_string(), !only);
            }
            3..=5 => {
                let rows: Vec<(String, u8)> = row
                    .as_ref()
                    .map(|r| r.params.iter().map(|p| (p.key.clone(), p.steps)).collect())
                    .unwrap_or_default();
                for (k, steps) in rows {
                    let on = match id {
                        3 => true,
                        4 => steps == 0,
                        _ => false,
                    };
                    let read = self.fxc_read_norm(&k);
                    let ps = self.fxc.t.param_mut(&k);
                    if on && !ps.sel && ps.srcs.is_empty() {
                        ps.base = read;
                    }
                    ps.sel = on;
                }
                if id != 5 {
                    self.fxc_ensure_bridge();
                }
            }
            6 => {
                roll::select_fx(&mut self.fxc.t, &keys, max as usize, &mut self.fxc.rng);
                for k in &keys {
                    if self
                        .fxc
                        .t
                        .params
                        .get(k)
                        .is_some_and(|p| p.sel && p.srcs.is_empty())
                    {
                        let v = self.fxc_read_norm(k);
                        self.fxc.t.param_mut(k).base = v;
                    }
                }
                self.fxc_ensure_bridge();
            }
            i @ 101..=110 => {
                self.fxc
                    .t
                    .fx_random_max
                    .insert(g.to_string(), (i - 100) as u8);
            }
            7 => roll::xy(
                &mut self.fxc.t,
                &keys,
                self.fxc.roll.exclusive_xy,
                &mut self.fxc.rng,
            ),
            8 => {
                let s = self.fxc.roll.clone();
                roll::ranges(&mut self.fxc.t, &keys, &s, &mut self.fxc.rng);
            }
            9 => {
                let out = roll::bases_uniform(&mut self.fxc.t, &keys, &mut self.fxc.rng);
                for (k, v) in out {
                    self.fxc_set_base(&k, v);
                }
            }
            10 => self.r.xc_fx_float(t, fx, true),
            _ => return,
        }
        self.fxc_changed();
    }

    /// Un effet sort de ROLL (DETACHED) ou y revient. Le sortir relache ce
    /// que ROLL y avait pose ; les valeurs restent.
    pub(crate) fn fxc_set_detached(&mut self, g: &str, det: bool) {
        let Some(t) = self.fxc_t() else { return };
        let mut d: Vec<String> = self
            .r
            .xc_track_ext(t, keys::DETACHED)
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if det {
            d.push(g.to_string());
            let keys: Vec<String> = self
                .fxc
                .rows
                .iter()
                .find(|r| r.guid == g)
                .map(|r| r.params.iter().map(|p| p.key.clone()).collect())
                .unwrap_or_default();
            for k in &keys {
                if let Some(p) = self.fxc.t.params.get_mut(k) {
                    p.sel = false;
                    p.srcs.clear();
                }
            }
        } else {
            d.retain(|x| x != g);
        }
        d.sort();
        d.dedup();
        self.r.xc_set_track_ext(t, keys::DETACHED, &d.join(" "));
        self.fxc_scan_chain();
        self.fxc_changed();
    }

    fn fxc_fx_verb(&mut self, g: &str, k: u8) {
        let Some(t) = self.fxc_t() else { return };
        let Some(fx) = self.fxc.chain.index_of(g) else {
            return;
        };
        let keys: Vec<String> = self
            .fxc
            .rows
            .iter()
            .find(|r| r.guid == g)
            .map(|r| r.params.iter().map(|p| p.key.clone()).collect())
            .unwrap_or_default();
        match k {
            0 => self.r.xc_fx_float(t, fx, true),
            1 => {
                let on = self.r.xc_fx_enabled(t, fx);
                self.r.xc_fx_set_enabled(t, fx, !on);
                self.fxc_read_values(true);
            }
            2 => {
                roll::xy(
                    &mut self.fxc.t,
                    &keys,
                    self.fxc.roll.exclusive_xy,
                    &mut self.fxc.rng,
                );
                self.fxc_changed();
            }
            3 => {
                let s = self.fxc.roll.clone();
                roll::ranges(&mut self.fxc.t, &keys, &s, &mut self.fxc.rng);
                self.fxc_changed();
            }
            4 => {
                let out = roll::bases_uniform(&mut self.fxc.t, &keys, &mut self.fxc.rng);
                for (k, v) in out {
                    self.fxc_set_base(&k, v);
                }
            }
            _ => {
                let (x, y) = self.fxc.view.mouse;
                self.fxc_fx_menu(g, x, y);
            }
        }
    }

    fn fxc_settings_menu(&mut self) {
        let s = self.fxc.roll.clone();
        let pct = |id0: u32, cur: f64| -> Vec<MenuEntry> {
            [0, 10, 20, 30, 50, 75, 100]
                .iter()
                .map(|p| {
                    MenuEntry::check(
                        id0 + *p as u32,
                        format!("{p} %"),
                        (cur * 100.0).round() as i32 == *p,
                    )
                })
                .collect()
        };
        let mut e = vec![
            MenuEntry::off("ROLL"),
            MenuEntry::Sub(
                format!("Params tracked: {} to {}", s.random_min, s.random_max),
                [(1, 3), (2, 6), (3, 8), (4, 12), (6, 20), (10, 40)]
                    .iter()
                    .map(|(a, b)| {
                        MenuEntry::check(
                            100 + (*a * 100 + *b) as u32,
                            format!("{a} to {b}"),
                            s.random_min == *a && s.random_max == *b,
                        )
                    })
                    .collect(),
            ),
            MenuEntry::Sub(
                format!("Bypass {:.0} %", s.bypass_pct * 100.0),
                pct(10_000, s.bypass_pct),
            ),
            MenuEntry::Sub(
                format!("Modulated {:.0} %", s.mod_prob * 100.0),
                pct(20_000, s.mod_prob),
            ),
            MenuEntry::Sub(
                format!("Base intensity {:.0} %", s.intensity * 100.0),
                pct(30_000, s.intensity),
            ),
            MenuEntry::check(5, "Exclusive X / Y", s.exclusive_xy),
            MenuEntry::Sub(
                format!("Range span {:.2} – {:.2}", s.range_min, s.range_max),
                SPANS
                    .iter()
                    .enumerate()
                    .map(|(i, (a, b))| {
                        MenuEntry::check(
                            50 + i as u32,
                            format!("{a:.2} – {b:.2}"),
                            (s.range_min - a).abs() < 1e-6 && (s.range_max - b).abs() < 1e-6,
                        )
                    })
                    .collect(),
            ),
            MenuEntry::Sub(
                format!("Base span {:.2} – {:.2}", s.base_min, s.base_max),
                SPANS
                    .iter()
                    .enumerate()
                    .map(|(i, (a, b))| {
                        MenuEntry::check(
                            60 + i as u32,
                            format!("{a:.2} – {b:.2}"),
                            (s.base_min - a).abs() < 1e-6 && (s.base_max - b).abs() < 1e-6,
                        )
                    })
                    .collect(),
            ),
            MenuEntry::Sub(
                format!("Modulators: {} slot(s)", s.lfo_slots),
                (1..=8)
                    .map(|n| MenuEntry::check(70 + n, n.to_string(), s.lfo_slots == n as usize))
                    .chain([
                        MenuEntry::Sep,
                        MenuEntry::check(80, "Randomize the shape", s.lfo_shape),
                        MenuEntry::check(81, "Randomize the rate (free Hz)", s.lfo_rate),
                        MenuEntry::check(82, "Randomize the tempo sync", s.lfo_sync),
                        MenuEntry::check(83, "Randomize the phase", s.lfo_phase),
                    ])
                    .collect(),
            ),
            MenuEntry::Sep,
            MenuEntry::off("Pad"),
        ];
        let slew = self.fxc.t.pad.link_slew;
        e.push(MenuEntry::Sub(
            format!("Slew {:.0} ms", slew * 1000.0),
            [0, 20, 50, 100, 250, 500, 1000]
                .iter()
                .map(|ms| {
                    MenuEntry::check(
                        40_000 + *ms as u32,
                        format!("{ms} ms"),
                        (slew * 1000.0).round() as i32 == *ms,
                    )
                })
                .collect(),
        ));
        let id = self.fxc_popup(self.fxc.view.mouse.0, self.fxc.view.mouse.1, &e);
        let r = &mut self.fxc.roll;
        match id {
            5 => r.exclusive_xy = !r.exclusive_xy,
            i @ 50..=59 => {
                if let Some((a, b)) = SPANS.get((i - 50) as usize) {
                    r.range_min = *a;
                    r.range_max = *b;
                }
            }
            i @ 60..=69 => {
                if let Some((a, b)) = SPANS.get((i - 60) as usize) {
                    r.base_min = *a;
                    r.base_max = *b;
                }
            }
            i @ 71..=78 => r.lfo_slots = (i - 70) as usize,
            80 => r.lfo_shape = !r.lfo_shape,
            81 => r.lfo_rate = !r.lfo_rate,
            82 => r.lfo_sync = !r.lfo_sync,
            83 => r.lfo_phase = !r.lfo_phase,
            i @ 100..=9_999 => {
                let v = i - 100;
                r.random_min = (v / 100) as usize;
                r.random_max = (v % 100) as usize;
            }
            i @ 10_000..=10_100 => r.bypass_pct = (i - 10_000) as f64 / 100.0,
            i @ 20_000..=20_100 => r.mod_prob = (i - 20_000) as f64 / 100.0,
            i @ 30_000..=30_100 => r.intensity = (i - 30_000) as f64 / 100.0,
            i @ 40_000..=41_000 => {
                self.fxc.t.pad.link_slew = (i - 40_000) as f64 / 1000.0;
                self.fxc_changed();
            }
            _ => return,
        }
        self.fxc.prefs_dirty = true;
    }

    fn fxc_ask(&self, title: &str, caption: &str, cur: &str) -> Option<String> {
        self.r
            .user_inputs(title, 1, caption, cur)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    // -----------------------------------------------------------------------
    // ROLL
    // -----------------------------------------------------------------------

    /// Les parametres et les effets du pool ROLL.
    fn fxc_pool(&self) -> (Vec<String>, Vec<String>) {
        let rows: Vec<_> = self.fxc.rows.iter().filter(|r| !r.detached).collect();
        let keys = rows
            .iter()
            .flat_map(|r| r.params.iter().map(|p| p.key.clone()))
            .collect();
        let fx = rows.iter().map(|r| r.guid.clone()).collect();
        (keys, fx)
    }

    /// Tous les axes armes (ou un seul).
    pub(crate) fn fxc_roll(&mut self, only: Option<Axis>) {
        let Some(t) = self.fxc_t() else { return };
        let s = self.fxc.roll.clone();
        let on = |a: Axis| only.map_or(s.is_armed(a), |o| o == a);
        let (pool, fxs) = self.fxc_pool();
        self.r.xc_undo_begin();
        if on(Axis::Params) {
            roll::select(&mut self.fxc.t, &pool, &s, &mut self.fxc.rng);
            for k in &pool {
                if self
                    .fxc
                    .t
                    .params
                    .get(k)
                    .is_some_and(|p| p.sel && p.srcs.is_empty())
                {
                    let v = self.fxc_read_norm(k);
                    self.fxc.t.param_mut(k).base = v;
                }
            }
        }
        if on(Axis::Mods) && s.mod_prob > 0.0 {
            if self.fxc_ensure_bank().is_some() {
                for i in 1..=s.lfo_slots.clamp(1, 8) {
                    let rr = roll::roll_slot(&s, &mut self.fxc.rng);
                    self.fxc_slot_apply_roll(i as i32, &rr);
                }
            }
            roll::mods(&mut self.fxc.t, &pool, &s, &mut self.fxc.rng);
        }
        if on(Axis::Xy) {
            roll::xy(&mut self.fxc.t, &pool, s.exclusive_xy, &mut self.fxc.rng);
        }
        if on(Axis::Invert) {
            roll::invert(&mut self.fxc.t, &pool, &mut self.fxc.rng);
        }
        if on(Axis::Bases) {
            let out = roll::bases(&mut self.fxc.t, &pool, &s, &mut self.fxc.rng);
            for (k, v) in out {
                self.fxc_set_base(&k, v);
            }
        }
        if on(Axis::Ranges) {
            roll::ranges(&mut self.fxc.t, &pool, &s, &mut self.fxc.rng);
        }
        if on(Axis::Bypass) {
            let en = roll::bypass(fxs.len(), s.bypass_pct, &mut self.fxc.rng);
            for (g, e) in fxs.iter().zip(en) {
                if let Some(fx) = self.fxc.chain.index_of(g) {
                    self.r.xc_fx_set_enabled(t, fx, e);
                }
            }
        }
        if on(Axis::Order) && fxs.len() > 1 {
            self.fxc_shuffle(&fxs);
        }
        if self.fxc.t.tracked_count() > 0 && !self.fxc.t.pad.granular {
            self.fxc_ensure_bridge();
        }
        self.fxc_count("roll");
        self.fxc_scan_chain();
        self.fxc_changed();
        // Les liens dans le meme pas d'annulation.
        self.fxc_apply();
        self.r.xc_undo_end("FX Constellation: ROLL");
    }

    /// Un nouvel ordre pour les effets du pool (les autres ne bougent pas).
    fn fxc_shuffle(&mut self, fxs: &[String]) {
        let Some(t) = self.fxc_t() else { return };
        let r = self.r;
        let mut slots: Vec<usize> = fxs.iter().filter_map(|g| r.xc_fx_index(t, g)).collect();
        slots.sort();
        let perm = roll::order(fxs.len(), &mut self.fxc.rng);
        let order: Vec<&String> = perm.iter().map(|&i| &fxs[i]).collect();
        let snap = self.fxc_snapshot_for_move(t);
        for (k, g) in order.iter().enumerate() {
            let dest = slots[k];
            let Some(cur) = r.xc_fx_index(t, g) else {
                continue;
            };
            if cur > dest {
                // Echange : cur vient a dest, celui de dest va a cur.
                r.xc_fx_move(t, cur, dest);
                r.xc_fx_move(t, dest + 1, cur + 1);
            }
        }
        self.fxc_repair_after_move(t, &snap);
    }

    // -----------------------------------------------------------------------
    // La banque de sources
    // -----------------------------------------------------------------------

    /// La banque d'un slot : (piste, effet, numero 1..8).
    fn fxc_bank_of(&mut self, slot: i32) -> Option<(Tk, usize, usize)> {
        match band_of(slot) {
            Some(Band::Lfo(n)) => {
                let t = self.fxc_t()?;
                let fx = self.fxc.chain.find(FxKind::Bank)?;
                Some((t, fx, n as usize))
            }
            Some(Band::Global(n)) => {
                let (mt, fx) = self.fxc_global_bank()?;
                Some((mt, fx, n as usize))
            }
            _ => None,
        }
    }

    fn fxc_sel_slot_state(&self) -> Option<(cp_fxcview::Slot, bool)> {
        let s = self.fxc.view.sel_slot;
        match band_of(s) {
            Some(Band::Lfo(n)) => self
                .fxc
                .bank
                .as_ref()
                .map(|b| (b.slots[n as usize - 1].clone(), false)),
            Some(Band::Global(n)) => self
                .fxc
                .gbank
                .as_ref()
                .map(|b| (b.slots[n as usize - 1].clone(), true)),
            _ => None,
        }
    }

    fn fxc_slot_write(&mut self, w: SlotW) {
        let s = self.fxc.view.sel_slot;
        self.fxc_slot_write_at(s, w);
    }

    fn fxc_slot_write_at(&mut self, slot: i32, w: SlotW) {
        let Some((t, fx, i)) = self.fxc_bank_of(slot) else {
            return;
        };
        let r = self.r;
        let field = |f: SlotField| match f {
            SlotField::Shape => bank::shape(i),
            SlotField::Rate => bank::rate(i),
            SlotField::Sync => bank::sync(i),
            SlotField::Phase => bank::phase(i),
            SlotField::Slew => bank::slew(i),
            SlotField::Curve => bank::curve(i),
            SlotField::Mode => bank::mode(i),
            SlotField::Trig => bank::trig(i),
            SlotField::Uni => bank::uni(i),
            SlotField::Sh => bank::sh(i),
            SlotField::Len => bank::len(i),
        };
        match w {
            SlotW::On(v) => {
                let cur = r.xc_get_raw(t, fx, bank::on(i)).0 > 0.5;
                let on = v.unwrap_or(!cur);
                r.xc_set_raw(t, fx, bank::on(i), on as u8 as f64);
            }
            SlotW::Raw(f, v) => {
                r.xc_set_raw(t, fx, field(f), v);
            }
            SlotW::Norm(f, v) => {
                r.xc_set(t, fx, field(f), v);
            }
            SlotW::Steps(st) => {
                let v = bank::pack_steps(&st);
                for (k, x) in v.iter().enumerate() {
                    r.xc_set_raw(t, fx, bank::steps(i, k), *x);
                }
            }
        }
        self.fxc_read_values(true);
        self.fxc.view.mark();
    }

    fn fxc_slot_apply_roll(&mut self, slot: i32, rr: &roll::SlotRoll) {
        self.fxc_slot_write_at(slot, SlotW::On(Some(true)));
        if let Some(sh) = rr.shape {
            self.fxc_slot_write_at(slot, SlotW::Raw(SlotField::Shape, sh as f64));
        }
        if let Some(hz) = rr.rate {
            let n = ((hz / 0.01).ln() / 2000f64.ln()).clamp(0.0, 1.0);
            self.fxc_slot_write_at(slot, SlotW::Norm(SlotField::Rate, n));
        }
        if let Some(sy) = rr.sync {
            self.fxc_slot_write_at(slot, SlotW::Raw(SlotField::Sync, sy as f64));
        }
        if let Some(ph) = rr.phase {
            self.fxc_slot_write_at(slot, SlotW::Raw(SlotField::Phase, ph));
        }
    }

    fn fxc_slot_menu(&mut self, c: Ctl, x: f32, y: f32) {
        let Some((s, _)) = self.fxc_sel_slot_state() else {
            return;
        };
        let e: Vec<MenuEntry> = match c {
            Ctl::SlotType => vec![
                MenuEntry::check(1, "LFO", s.shape <= 4),
                MenuEntry::check(2, "S&H", s.shape == 5 || s.shape == 6),
                MenuEntry::check(3, "STEP", s.shape == 7),
            ],
            Ctl::SlotShape => (0..5)
                .map(|i| MenuEntry::check(10 + i, bank::SHAPES[i as usize], s.shape as u32 == i))
                .collect(),
            Ctl::SlotSync => (0..11)
                .map(|i| MenuEntry::check(20 + i, bank::SYNCS[i as usize], s.sync as u32 == i))
                .collect(),
            Ctl::SlotMode => (0..3)
                .map(|i| MenuEntry::check(40 + i, bank::MODES[i as usize], s.mode as u32 == i))
                .collect(),
            _ => {
                let mut v = vec![MenuEntry::check(50, "Noise", s.sh == 0)];
                for k in 1..=8 {
                    v.push(MenuEntry::check(
                        50 + k,
                        format!("LFO {k}"),
                        s.sh as u32 == k,
                    ));
                }
                v
            }
        };
        match self.fxc_popup(x, y, &e) {
            1 => self.fxc_slot_write(SlotW::Raw(SlotField::Shape, 0.0)),
            2 => self.fxc_slot_write(SlotW::Raw(SlotField::Shape, 6.0)),
            3 => self.fxc_slot_write(SlotW::Raw(SlotField::Shape, 7.0)),
            i @ 10..=14 => self.fxc_slot_write(SlotW::Raw(SlotField::Shape, (i - 10) as f64)),
            i @ 20..=30 => self.fxc_slot_write(SlotW::Raw(SlotField::Sync, (i - 20) as f64)),
            i @ 40..=42 => self.fxc_slot_write(SlotW::Raw(SlotField::Mode, (i - 40) as f64)),
            i @ 50..=58 => self.fxc_slot_write(SlotW::Raw(SlotField::Sh, (i - 50) as f64)),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Les pas peints
    // -----------------------------------------------------------------------

    fn fxc_step(&mut self, c: Ctl, k: u8, v: f64) {
        match c {
            Ctl::Steps => {
                if let Some((s, _)) = self.fxc_sel_slot_state() {
                    let mut st = s.steps;
                    st[k as usize % 16] = (v * 63.0).round().clamp(0.0, 63.0) as u8;
                    self.fxc_slot_write(SlotW::Steps(st));
                }
            }
            Ctl::SeqLane(l) => {
                let Some((t, fx)) = self.fxc_js(SEQ) else {
                    return;
                };
                let lane = &LANES[l as usize % 9];
                let mut raw = lane.min + v * (lane.max - lane.min);
                if lane.bools {
                    raw = if v > 0.5 { 1.0 } else { 0.0 };
                } else if lane.int {
                    raw = raw.round();
                }
                self.r
                    .xc_set_raw(t, fx, panels::lane_base(l as usize) + k as usize, raw);
                if let Some(j) = self.fxc.seq.as_mut() {
                    let p = panels::lane_base(l as usize) + k as usize;
                    if p < j.raw.len() {
                        j.raw[p] = raw;
                    }
                }
                self.fxc.view.mark();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Le Sound Gen et le sequenceur
    // -----------------------------------------------------------------------

    fn fxc_js(&self, panel: u8) -> Option<(Tk, usize)> {
        let t = self.fxc_t()?;
        let k = if panel == SG {
            FxKind::SoundGen
        } else {
            FxKind::Seq
        };
        Some((t, self.fxc.chain.find(k)?))
    }

    fn fxc_js_value(&mut self, panel: u8, p: usize, v: f64) {
        let Some((t, fx)) = self.fxc_js(panel) else {
            return;
        };
        let r = self.r;
        if panel == SG {
            // PITCH d'un oscillateur libre : par demi-tons.
            if let Some(o) = panels::OSC.iter().find(|o| o.freq == p) {
                let hz = 20.0 * 1000f64.powf(v);
                let fine = r.xc_get_raw(t, fx, o.fine).0;
                let m = (panels::midi_of(hz) - fine / 100.0).round();
                let hz2 = 440.0 * 2f64.powf((m + fine / 100.0 - 69.0) / 12.0);
                let n = ((hz2.clamp(20.0, 20000.0) / 20.0).ln() / 1000f64.ln()).clamp(0.0, 1.0);
                r.xc_set(t, fx, p, n);
                self.fxc_read_values(true);
                return;
            }
            // FINE d'un oscillateur libre : les cents, la frequence suit.
            if let Some(o) = panels::OSC.iter().find(|o| o.fine == p) {
                let fine_new = -50.0 + v * 100.0;
                let fine_old = r.xc_get_raw(t, fx, o.fine).0;
                let hz = unshape_log(r.xc_get_raw(t, fx, o.freq).0);
                let m = (panels::midi_of(hz) - fine_old / 100.0).round();
                let hz2 = 440.0 * 2f64.powf((m + fine_new / 100.0 - 69.0) / 12.0);
                r.xc_set_raw(t, fx, o.fine, fine_new);
                let n = ((hz2.clamp(20.0, 20000.0) / 20.0).ln() / 1000f64.ln()).clamp(0.0, 1.0);
                r.xc_set(t, fx, o.freq, n);
                self.fxc_read_values(true);
                return;
            }
        }
        r.xc_set(t, fx, p, v);
        let full = panel == SEQ;
        if let Some(j) = if panel == SG {
            self.fxc.sg.as_mut()
        } else {
            self.fxc.seq.as_mut()
        } {
            if p < j.norm.len() {
                j.norm[p] = v;
            }
        }
        self.fxc_read_values(full);
    }

    fn fxc_js_menu(&mut self, panel: u8, p: usize, x: f32, y: f32) {
        let Some((t, fx)) = self.fxc_js(panel) else {
            return;
        };
        let cur = self.r.xc_get_raw(t, fx, p).0.round() as i64;
        let opts: Vec<(i64, String)> = if panel == SG {
            if panels::OSC.iter().any(|o| o.wave == p) {
                WAVES
                    .iter()
                    .enumerate()
                    .map(|(i, w)| (i as i64, w.to_string()))
                    .collect()
            } else if let Some(o) = panels::OSC.iter().find(|o| o.scale == p) {
                // Les gammes, puis les toniques (sur un autre curseur).
                let root = self.r.xc_get_raw(t, fx, o.root).0.round() as i64;
                let mut e: Vec<MenuEntry> = SCALES
                    .iter()
                    .enumerate()
                    .map(|(i, s)| MenuEntry::check(1 + i as u32, *s, cur == i as i64))
                    .collect();
                e.push(MenuEntry::Sub(
                    format!("Root  {}", ROOTS[root.clamp(0, 11) as usize]),
                    ROOTS
                        .iter()
                        .enumerate()
                        .map(|(i, rt)| MenuEntry::check(100 + i as u32, *rt, root == i as i64))
                        .collect(),
                ));
                match self.fxc_popup(x, y, &e) {
                    i @ 1..=17 => {
                        self.r.xc_set_raw(t, fx, p, (i - 1) as f64);
                    }
                    i @ 100..=111 => {
                        self.r.xc_set_raw(t, fx, o.root, (i - 100) as f64);
                    }
                    _ => {}
                };
                self.fxc_read_values(true);
                return;
            } else if [76, 79, 82, 85].contains(&p) {
                ENV_SYNCS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect()
            } else {
                return;
            }
        } else {
            match p {
                3 => RATES
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                5 => DIRS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                7 => {
                    let root = self.r.xc_get_raw(t, fx, 6).0.round() as i64;
                    let mut e: Vec<MenuEntry> = SCALES[1..]
                        .iter()
                        .enumerate()
                        .map(|(i, s)| MenuEntry::check(1 + i as u32, *s, cur == i as i64))
                        .collect();
                    e.push(MenuEntry::Sub(
                        format!("Root  {}", ROOTS[root.clamp(0, 11) as usize]),
                        ROOTS
                            .iter()
                            .enumerate()
                            .map(|(i, rt)| MenuEntry::check(100 + i as u32, *rt, root == i as i64))
                            .collect(),
                    ));
                    match self.fxc_popup(x, y, &e) {
                        i @ 1..=16 => {
                            self.r.xc_set_raw(t, fx, p, (i - 1) as f64);
                        }
                        i @ 100..=111 => {
                            self.r.xc_set_raw(t, fx, 6, (i - 100) as f64);
                        }
                        _ => {}
                    };
                    self.fxc_read_values(true);
                    return;
                }
                8 => (0..=8).map(|i| (i, format!("Oct {i}"))).collect(),
                10 => KEYS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                11 => (1..=16).map(|i| (i, format!("Ch {i}"))).collect(),
                223 => std::iter::once((0, "Off".to_string()))
                    .chain((1..=10).map(|i| (i, format!("C{}", i - 2))))
                    .collect(),
                224 => PAT_WHEN
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                227 => ARP_ORDERS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                228 => (1..=4).map(|i| (i, format!("{i} oct"))).collect(),
                p if p >= 20 && (p - 20) % 22 == 17 => SPEEDS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as i64, s.to_string()))
                    .collect(),
                _ => return,
            }
        };
        let e: Vec<MenuEntry> = opts
            .iter()
            .map(|(v, l)| MenuEntry::check((*v + 1000) as u32, l.clone(), *v == cur))
            .collect();
        let id = self.fxc_popup(x, y, &e);
        if id >= 1 {
            self.r.xc_set_raw(t, fx, p, id as f64 - 1000.0);
            self.fxc_read_values(true);
        }
    }

    /// Une source de note : l'etendue des notes (NOTE) et le glisse.
    fn fxc_notes_menu(&mut self, slot: i32, x: f32, y: f32) {
        let Some(t) = self.fxc_t() else { return };
        let Some(fx) = self.fxc.chain.find(FxKind::Notes) else {
            self.fxc_say("the note sources arrive with their first link");
            return;
        };
        let k = (slot - 231) as usize;
        let lo = self.r.xc_get_raw(t, fx, 4).0.round() as i64;
        let hi = self.r.xc_get_raw(t, fx, 5).0.round() as i64;
        let gl = self.r.xc_get_raw(t, fx, 6 + k).0.round() as i64;
        let ranges = [
            (24, 96, "C1 - C7"),
            (36, 84, "C2 - C6"),
            (48, 72, "C3 - C5"),
            (12, 108, "C0 - C8"),
            (0, 127, "Full 0 - 127"),
        ];
        let glides = [
            (0, "Off"),
            (10, "10 ms"),
            (30, "30 ms"),
            (100, "100 ms"),
            (300, "300 ms"),
            (1000, "1 s"),
        ];
        let e = vec![
            MenuEntry::off(long_label(slot)),
            MenuEntry::Sub(
                "Range".into(),
                ranges
                    .iter()
                    .enumerate()
                    .map(|(i, (a, b, l))| MenuEntry::check(10 + i as u32, *l, lo == *a && hi == *b))
                    .collect(),
            ),
            MenuEntry::Sub(
                "Glide".into(),
                glides
                    .iter()
                    .enumerate()
                    .map(|(i, (ms, l))| MenuEntry::check(20 + i as u32, *l, gl == *ms))
                    .collect(),
            ),
        ];
        match self.fxc_popup(x, y, &e) {
            i @ 10..=14 => {
                let (a, b, _) = ranges[(i - 10) as usize];
                self.r.xc_set_raw(t, fx, 4, a as f64);
                self.r.xc_set_raw(t, fx, 5, b as f64);
            }
            i @ 20..=25 => {
                self.r
                    .xc_set_raw(t, fx, 6 + k, glides[(i - 20) as usize].0 as f64);
            }
            _ => {}
        }
    }

    /// Une voie du sequenceur : la rouler, la remettre, la decaler.
    fn fxc_lane_menu(&mut self, l: u8, x: f32, y: f32) {
        let Some((t, fx)) = self.fxc_js(SEQ) else {
            return;
        };
        let lane = &LANES[l as usize % 9];
        let base = panels::lane_base(l as usize);
        let len = self
            .r
            .xc_get_raw(t, fx, base + 16)
            .0
            .round()
            .clamp(1.0, 16.0) as usize;
        let mut e = vec![
            MenuEntry::off(lane.name),
            MenuEntry::item(1, "Roll lane"),
            MenuEntry::item(2, "Reset lane"),
            MenuEntry::item(3, "Shift left"),
            MenuEntry::item(4, "Shift right"),
        ];
        if lane.bools {
            e.push(MenuEntry::Sep);
            e.push(MenuEntry::item(5, "All on"));
            e.push(MenuEntry::item(6, "All off"));
            e.push(MenuEntry::item(7, "Invert"));
        }
        let id = self.fxc_popup(x, y, &e);
        let read = |a: &App, k: usize| a.r.xc_get_raw(t, fx, base + k).0;
        let mut v: Vec<f64> = (0..16).map(|k| read(self, k)).collect();
        let default = match l {
            0 => 1.0,
            3 => 100.0,
            4 => 50.0,
            5 => 100.0,
            7 | 8 => 0.5,
            _ => 0.0,
        };
        match id {
            1 => {
                self.fxc_seq_roll(1 << l);
                return;
            }
            2 => v.iter_mut().for_each(|x| *x = default),
            3 => v[..len].rotate_left(1),
            4 => v[..len].rotate_right(1),
            5 => v.iter_mut().for_each(|x| *x = 1.0),
            6 => v.iter_mut().for_each(|x| *x = 0.0),
            7 => v
                .iter_mut()
                .for_each(|x| *x = if *x > 0.5 { 0.0 } else { 1.0 }),
            _ => return,
        }
        for (k, x) in v.iter().enumerate() {
            self.r.xc_set_raw(t, fx, base + k, *x);
        }
        self.fxc_read_values(true);
    }

    /// Allume ou eteint le Sound Gen. Eteint, il quitte la chaine et son
    /// reglage reste dans la piste (`P_EXT:CP_FXC_SG_PARK`).
    fn fxc_sg_toggle(&mut self) {
        let Some(t) = self.fxc_t() else { return };
        match self.fxc.chain.find(FxKind::SoundGen) {
            Some(fx) => {
                let park: Vec<String> = (0..self.r.xc_param_count(t, fx))
                    .map(|p| format!("{p}={:.9}", self.r.xc_get(t, fx, p)))
                    .collect();
                for p in [31, 52, 63, 68, 73] {
                    self.r.xc_set(t, fx, p, 0.0);
                }
                match self.fxc_retire(fx) {
                    Ok(()) => {
                        self.r
                            .xc_set_track_ext(t, "CP_FXC_SG_PARK", &park.join(";"));
                        self.fxc_count("sg_off");
                    }
                    Err(e) => {
                        self.r.xc_fx_set_enabled(t, fx, false);
                        self.fxc_say(format!("Sound Gen bypassed but kept: {e}"));
                    }
                }
            }
            None => {
                let at = self
                    .fxc
                    .chain
                    .fx
                    .iter()
                    .rposition(|f| matches!(f.kind, Some(FxKind::Seq) | Some(FxKind::Notes)));
                let at = at.map_or(0, |i| i + 1);
                if let Some(fx) = self.fxc_insert(sg::ADD, Some(at)) {
                    if let Some(park) = self.r.xc_track_ext(t, "CP_FXC_SG_PARK") {
                        for kv in park.split(';') {
                            if let Some((p, v)) = kv.split_once('=') {
                                if let (Ok(p), Ok(v)) = (p.parse::<usize>(), v.parse::<f64>()) {
                                    if ![16, 31, 52, 55, 56, 57, 63, 68, 73].contains(&p) && p < 145
                                    {
                                        self.r.xc_set(t, fx, p, v);
                                    }
                                }
                            }
                        }
                        self.r.xc_set_track_ext(t, "CP_FXC_SG_PARK", "");
                    }
                    self.fxc_count("sg_on");
                }
            }
        }
        self.fxc_read_values(true);
        self.fxc.links_dirty = true;
    }

    fn fxc_seq_add(&mut self) {
        let Some(t) = self.fxc_t() else { return };
        if self.fxc.chain.find(FxKind::Seq).is_some() {
            return;
        }
        if let Some(fx) = self.fxc_insert(cp_fxc::slots::seq::ADD, Some(0)) {
            if let Some(park) = self.r.xc_track_ext(t, cp_fxc::slots::seq::PARK) {
                for kv in park.split(';') {
                    if let Some((p, v)) = kv.split_once('=') {
                        if let (Ok(p), Ok(v)) = (p.parse::<usize>(), v.parse::<f64>()) {
                            if p != 1 {
                                self.r.xc_set_raw(t, fx, p, v);
                            }
                        }
                    }
                }
                self.r.xc_set_track_ext(t, cp_fxc::slots::seq::PARK, "");
            }
            self.r.xc_set_raw(t, fx, 1, 0.0);
            // Le Sound Gen doit entendre les notes.
            if let Some(sgfx) = self.fxc.chain.find(FxKind::SoundGen) {
                let trig = self.r.xc_get_raw(t, sgfx, 0).0 > 0.5;
                let midi = self.r.xc_get_raw(t, sgfx, 15).0 > 0.5;
                if !(trig && midi) {
                    self.r.xc_set_raw(t, sgfx, 0, 1.0);
                    self.r.xc_set_raw(t, sgfx, 15, 1.0);
                    self.fxc_say("Sound Gen set to Triggered + MIDI so it plays the sequence");
                }
            }
            self.fxc_count("seq_add");
        }
        self.fxc_read_values(true);
    }

    fn fxc_seq_remove(&mut self) {
        let Some(t) = self.fxc_t() else { return };
        let Some(fx) = self.fxc.chain.find(FxKind::Seq) else {
            return;
        };
        let skip = [
            12usize, 13, 14, 15, 16, 17, 18, 19, 218, 219, 220, 221, 222, 225, 226,
        ];
        let park: Vec<String> = (0..self.r.xc_param_count(t, fx))
            .filter(|p| !skip.contains(p))
            .map(|p| format!("{p}={:.9}", self.r.xc_get_raw(t, fx, p).0))
            .collect();
        match self.fxc_retire(fx) {
            Ok(()) => {
                self.r
                    .xc_set_track_ext(t, cp_fxc::slots::seq::PARK, &park.join(";"));
                self.fxc_say("sequencer removed; its settings stay with the track");
                self.fxc_count("seq_remove");
            }
            Err(e) => self.fxc_say(format!("the sequencer stays: {e}")),
        }
        self.fxc_read_values(true);
    }

    /// Une requete de pattern : `nonce·4096 + op·256 + arg`.
    fn fxc_seq_request(&mut self, op: u32, arg: u32) {
        let Some((t, fx)) = self.fxc_js(SEQ) else {
            return;
        };
        let cur = self.r.xc_get_raw(t, fx, 222).0 as u64;
        let nonce = ((cur / 4096 + 1) % 1024).max(1);
        let v = nonce * 4096 + op as u64 * 256 + (arg as u64 & 255);
        self.r.xc_set_raw(t, fx, 222, v as f64);
    }

    /// Une requete de des : `nonce·512 + masque des voies`.
    fn fxc_seq_roll(&mut self, mask: u32) {
        let Some((t, fx)) = self.fxc_js(SEQ) else {
            return;
        };
        let cur = self.r.xc_get_raw(t, fx, 19).0 as u64;
        let nonce = ((cur / 512 + 1) % 1024).max(1);
        self.r
            .xc_set_raw(t, fx, 19, (nonce * 512 + (mask as u64 & 511)) as f64);
        self.fxc_count("seq_roll");
    }

    // -----------------------------------------------------------------------
    // Le granulaire, le fondu
    // -----------------------------------------------------------------------

    fn fxc_init_grains(&mut self) {
        let keys: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        let pad = self.fxc.t.pad.clone();
        self.fxc.t.grains = cp_fxc::pad::init_grains(pad.grid, &keys, &pad, &mut self.fxc.rng);
    }

    fn fxc_capture_morph(&self) -> BTreeMap<String, f64> {
        self.fxc
            .t
            .tracked()
            .map(|(k, p)| (k.clone(), p.base))
            .collect()
    }

    fn fxc_apply_morph(&mut self, v: f64) {
        let (Some(a), Some(b)) = (self.fxc.t.morph_a.clone(), self.fxc.t.morph_b.clone()) else {
            return;
        };
        for (k, va) in a {
            if let Some(vb) = b.get(&k) {
                self.fxc_set_base(&k, va * (1.0 - v) + vb * v);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Les presets et les snapshots
    // -----------------------------------------------------------------------

    pub(crate) fn fxc_preset_list(&self) -> Vec<String> {
        let Some(dir) = crate::fxc_sync::preset_dir(&self.r) else {
            return Vec::new();
        };
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let n = e.file_name().to_string_lossy().into_owned();
                        n.strip_suffix(".fxc").map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    fn fxc_preset_path(&self, name: &str) -> Option<std::path::PathBuf> {
        crate::fxc_sync::preset_dir(&self.r).map(|d| d.join(file_name(name)))
    }

    pub(crate) fn fxc_read_preset(&mut self) {
        self.fxc.preset = if self.fxc.t.preset.is_empty() {
            None
        } else {
            self.fxc_preset_path(&self.fxc.t.preset.clone())
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| cp_fxc::preset::decode(&String::from_utf8_lossy(&b)))
        };
    }

    /// Ce que la piste fait entendre, en preset.
    fn fxc_capture(&self) -> Option<Preset> {
        let t = self.fxc_t()?;
        let r = self.r;
        let mut p = Preset {
            pad: self.fxc.t.pad.clone(),
            macros: self.fxc.macros,
            macro_names: self.fxc.t.macro_names.clone(),
            ..Preset::default()
        };
        for (i, f) in self.fxc.chain.fx.iter().enumerate() {
            if f.kind != Some(FxKind::User) {
                continue;
            }
            let chunk = ["vst_chunk", "clap_chunk"].iter().find_map(|k| {
                r.xc_cfg(t, i, k)
                    .filter(|s| !s.is_empty())
                    .map(|d| (k.to_string(), d))
            });
            let names = self.fxc.names.get(&(f.guid.clone(), f.nparams));
            let params = (0..f.nparams)
                .filter_map(|k| {
                    let ps = self.fxc.t.params.get(&pkey(&f.guid, k))?;
                    let nm = names
                        .and_then(|n| n.get(k))
                        .map_or(String::new(), |n| n.0.clone());
                    Some((k, (nm, ps.clone())))
                })
                .collect();
            p.fx.push(PresetFx {
                name: f.name.clone(),
                ident: r.xc_cfg(t, i, "fx_name").unwrap_or_else(|| f.name.clone()),
                renamed: r.xc_cfg(t, i, "renamed_name").unwrap_or_default(),
                enabled: r.xc_fx_enabled(t, i),
                detached: self.fxc.rows.iter().any(|x| x.guid == f.guid && x.detached),
                preset: r.xc_preset_name(t, i),
                chunk,
                raw: (0..f.nparams).map(|k| r.xc_get(t, i, k)).collect(),
                params,
            });
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::SoundGen) {
            p.sg = Some((
                r.xc_fx_enabled(t, fx),
                (0..r.xc_param_count(t, fx))
                    .map(|k| r.xc_get(t, fx, k))
                    .collect(),
            ));
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::Seq) {
            p.seq = Some(
                (0..r.xc_param_count(t, fx))
                    .map(|k| r.xc_get_raw(t, fx, k).0)
                    .collect(),
            );
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::Bank) {
            p.bank = Some(
                (0..r.xc_param_count(t, fx))
                    .map(|k| r.xc_get(t, fx, k))
                    .collect(),
            );
        }
        p.routes = r.xc_track_ext(t, keys::ROUTES).unwrap_or_default();
        p.internal = self.fxc_internal_capture();
        Some(p)
    }

    pub(crate) fn fxc_save_preset(&mut self, name: &str) {
        let Some(mut p) = self.fxc_capture() else {
            return;
        };
        // Une capture ne dit rien des snapshots : ceux du preset restent.
        if let Some(old) = self
            .fxc_preset_path(name)
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| cp_fxc::preset::decode(&String::from_utf8_lossy(&b)))
        {
            p.snapshots = old.snapshots;
        }
        let Some(path) = self.fxc_preset_path(name) else {
            return;
        };
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(&path, cp_fxc::preset::encode(&p)) {
            Ok(()) => {
                self.fxc.t.preset = name.to_string();
                self.fxc.preset = Some(p);
                self.fxc.preset_modified = false;
                self.fxc_count("preset_save");
                self.fxc_say(format!("Preset '{name}' saved"));
                self.fxc_changed();
            }
            Err(e) => self.fxc_say(format!("Preset not saved: {e}")),
        }
    }

    /// Rappelle un preset : la chaine de ROLL refaite, chaque effet par son
    /// nom d'origine et son etat entier, le pad, les macros, le mapping.
    pub(crate) fn fxc_load_preset(&mut self, name: &str) {
        let Some(p) = self
            .fxc_preset_path(name)
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| cp_fxc::preset::decode(&String::from_utf8_lossy(&b)))
        else {
            self.fxc_say(format!("Preset '{name}' not found"));
            return;
        };
        let Some(t) = self.fxc_t() else { return };
        let r = self.r;
        r.xc_undo_begin();
        let mute = r.xc_track_value(t, "B_MUTE");
        r.xc_set_track_value(t, "B_MUTE", 1.0);
        // Les effets de l'utilisateur partent (les internes restent).
        for i in (0..r.xc_fx_count(t)).rev() {
            if fx_kind_at(&r, t, i) == FxKind::User {
                r.xc_fx_delete(t, i);
            }
        }
        let mut missing = Vec::new();
        let mut model: BTreeMap<String, PState> = BTreeMap::new();
        let mut detached = Vec::new();
        for f in &p.fx {
            let at = r.xc_fx_count(t);
            let idx = self
                .fxc_quiet_add(t, &f.ident, Some(at))
                .or_else(|| self.fxc_quiet_add(t, &f.name, Some(at)));
            let Some(i) = idx else {
                missing.push(f.name.clone());
                continue;
            };
            if !f.renamed.is_empty() {
                r.xc_set_cfg(t, i, "renamed_name", &f.renamed);
            }
            r.xc_fx_set_enabled(t, i, f.enabled);
            let mut stated = false;
            if let Some((k, d)) = &f.chunk {
                stated = r.xc_set_cfg(t, i, k, d);
            }
            if !stated && r.xc_param_count(t, i) == f.raw.len() {
                for (k, v) in f.raw.iter().enumerate() {
                    r.xc_set(t, i, k, *v);
                }
            }
            let g = r.xc_fx_guid(t, i).unwrap_or_default();
            if f.detached {
                detached.push(g.clone());
            }
            for (k, (_, ps)) in &f.params {
                model.insert(pkey(&g, *k), ps.clone());
            }
        }
        detached.sort();
        r.xc_set_track_ext(t, keys::DETACHED, &detached.join(" "));
        self.fxc_scan_chain();
        // Le Sound Gen, le sequenceur, la banque.
        if let Some((on, vals)) = &p.sg {
            let fx = match self.fxc.chain.find(FxKind::SoundGen) {
                Some(fx) => Some(fx),
                None => {
                    let at = self
                        .fxc
                        .chain
                        .fx
                        .iter()
                        .rposition(|f| matches!(f.kind, Some(FxKind::Seq) | Some(FxKind::Notes)))
                        .map_or(0, |i| i + 1);
                    self.fxc_insert(sg::ADD, Some(at))
                }
            };
            if let Some(fx) = fx {
                for (k, v) in vals.iter().enumerate() {
                    if ![16, 31, 52, 55, 56, 57, 63, 68, 73].contains(&k) && k < 145 {
                        r.xc_set(t, fx, k, *v);
                    }
                }
                r.xc_fx_set_enabled(t, fx, *on);
            }
        }
        if let Some(vals) = &p.seq {
            if self.fxc.chain.find(FxKind::Seq).is_none() {
                self.fxc_insert(cp_fxc::slots::seq::ADD, Some(0));
            }
            if let Some(fx) = self.fxc.chain.find(FxKind::Seq) {
                for (k, v) in vals.iter().enumerate() {
                    if !(12..=19).contains(&k) && !(218..=226).contains(&k) {
                        r.xc_set_raw(t, fx, k, *v);
                    }
                }
                r.xc_set_raw(t, fx, 1, 0.0);
            }
        }
        if let Some(vals) = &p.bank {
            if let Some(fx) = self.fxc_ensure_bank() {
                for (k, v) in vals.iter().enumerate() {
                    if !(40..56).contains(&k) && k < 144 {
                        r.xc_set(t, fx, k, *v);
                    }
                }
            }
        }
        r.xc_set_track_ext(t, keys::ROUTES, &p.routes);
        // Le pad, les macros, le mapping (les reglages internes aussi).
        self.fxc.t.params = model;
        self.fxc_internal_restore(&p.internal);
        self.fxc.t.pad = p.pad.clone();
        self.fxc.t.macro_names = p.macro_names.clone();
        self.fxc.t.preset = name.to_string();
        self.fxc.t.snapshot.clear();
        if let Some(b) = self.fxc.chain.find(FxKind::Bridge).or_else(|| {
            (self.fxc.t.tracked_count() > 0)
                .then(|| self.fxc_ensure_bridge())
                .flatten()
        }) {
            for (n, v) in p.macros.iter().enumerate() {
                r.xc_set_raw(t, b, bridge::MACRO_IN + n, *v);
            }
            r.xc_set_raw(t, b, bridge::X_POS, p.pad.x);
            r.xc_set_raw(t, b, bridge::Y_POS, p.pad.y);
        }
        r.xc_set_track_value(t, "B_MUTE", mute);
        self.fxc.preset = Some(p);
        self.fxc.preset_modified = false;
        self.fxc.eng_cache.clear();
        self.fxc_scan_chain();
        self.fxc_count("preset_load");
        self.fxc_apply();
        r.xc_undo_end(&format!("Load FX Constellation preset: {name}"));
        if missing.is_empty() {
            self.fxc_say(format!("Preset '{name}'"));
        } else {
            self.fxc_say(format!(
                "Preset '{name}': not installed here -- {}",
                missing.join(", ")
            ));
        }
        self.fxc_changed();
    }

    fn fxc_capture_snapshot(&mut self, name: &str) {
        let Some(t) = self.fxc_t() else { return };
        let mut s = Snapshot {
            pad: self.fxc.t.pad.clone(),
            macros: self.fxc.macros,
            macro_names: self.fxc.t.macro_names.clone(),
            ..Snapshot::default()
        };
        for (i, f) in self.fxc.chain.fx.iter().enumerate() {
            if f.kind != Some(FxKind::User) {
                continue;
            }
            s.fx.push((f.name.clone(), !self.r.xc_fx_enabled(t, i)));
            let names = self.fxc.names.get(&(f.guid.clone(), f.nparams));
            for k in 0..f.nparams {
                if let Some(ps) = self.fxc.t.params.get(&pkey(&f.guid, k)) {
                    if ps.sel || !ps.srcs.is_empty() {
                        let nm = names
                            .and_then(|n| n.get(k))
                            .map_or(String::new(), |n| n.0.clone());
                        s.params.insert(format!("{}||{nm}", f.name), ps.clone());
                    }
                }
            }
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::Bank) {
            s.bank = Some(
                (0..self.r.xc_param_count(t, fx))
                    .map(|k| self.r.xc_get(t, fx, k))
                    .collect(),
            );
        }
        s.internal = self.fxc_internal_capture();
        let name = name.to_string();
        if let Some(p) = self.fxc.preset.as_mut() {
            p.snapshots.insert(name.clone(), s);
            let pr = p.clone();
            if let Some(path) = self.fxc_preset_path(&self.fxc.t.preset.clone()) {
                let _ = std::fs::write(path, cp_fxc::preset::encode(&pr));
            }
            self.fxc.t.snapshot = name.clone();
            self.fxc_count("snap_save");
            self.fxc_say(format!("Snapshot '{name}' captured"));
            self.fxc_changed();
        }
    }

    /// Un snapshot ne touche que les effets qu'il connait.
    fn fxc_recall_snapshot(&mut self, name: &str) {
        let Some(s) = self
            .fxc
            .preset
            .as_ref()
            .and_then(|p| p.snapshots.get(name))
            .cloned()
        else {
            return;
        };
        let Some(t) = self.fxc_t() else { return };
        let r = self.r;
        let known: Vec<String> = s.fx.iter().map(|f| f.0.clone()).collect();
        let mut missing = Vec::new();
        for (n, by) in &s.fx {
            match self.fxc.chain.fx.iter().position(|f| &f.name == n) {
                Some(i) => r.xc_fx_set_enabled(t, i, !by),
                None => missing.push(n.clone()),
            }
        }
        for f in self.fxc.chain.fx.clone() {
            if f.kind != Some(FxKind::User) || !known.contains(&f.name) {
                continue;
            }
            let names = self.fxc.names.get(&(f.guid.clone(), f.nparams)).cloned();
            for k in 0..f.nparams {
                let nm = names
                    .as_ref()
                    .and_then(|n| n.get(k))
                    .map_or(String::new(), |n| n.0.clone());
                let key = pkey(&f.guid, k);
                match s.params.get(&format!("{}||{nm}", f.name)) {
                    Some(ps) => {
                        self.fxc.t.params.insert(key.clone(), ps.clone());
                        if ps.srcs.is_empty() && !ps.sel {
                            continue;
                        }
                    }
                    None => {
                        if let Some(p) = self.fxc.t.params.get_mut(&key) {
                            p.sel = false;
                            p.srcs.clear();
                        }
                    }
                }
            }
        }
        self.fxc.t.pad = s.pad.clone();
        self.fxc.t.macro_names = s.macro_names.clone();
        self.fxc_internal_restore(&s.internal);
        if let Some(b) = self.fxc.chain.find(FxKind::Bridge) {
            for (n, v) in s.macros.iter().enumerate() {
                r.xc_set_raw(t, b, bridge::MACRO_IN + n, *v);
            }
            r.xc_set_raw(t, b, bridge::X_POS, s.pad.x);
            r.xc_set_raw(t, b, bridge::Y_POS, s.pad.y);
        }
        if let (Some(vals), Some(fx)) = (&s.bank, self.fxc.chain.find(FxKind::Bank)) {
            for (k, v) in vals.iter().enumerate() {
                if !(40..56).contains(&k) && k < 144 {
                    r.xc_set(t, fx, k, *v);
                }
            }
        }
        self.fxc.t.snapshot = name.to_string();
        self.fxc.eng_cache.clear();
        self.fxc_count("snap_load");
        if !missing.is_empty() {
            self.fxc_say(format!(
                "Snapshot '{name}': missing -- {}",
                missing.join(", ")
            ));
        }
        self.fxc_changed();
    }

    fn fxc_preset_menu(&mut self, x: f32, y: f32) {
        let list = self.fxc_preset_list();
        let cur = self.fxc.t.preset.clone();
        let mut e = vec![
            MenuEntry::off(if cur.is_empty() {
                "no preset".to_string()
            } else {
                cur.clone()
            }),
            MenuEntry::item(1, "Init — start from an empty constellation…"),
            MenuEntry::Sep,
            MenuEntry::item(
                2,
                if cur.is_empty() {
                    "Save…".to_string()
                } else {
                    format!("Save '{cur}'")
                },
            ),
            MenuEntry::item(3, "Save as…"),
            MenuEntry::Sep,
        ];
        let items: Vec<MenuEntry> = list
            .iter()
            .enumerate()
            .map(|(i, n)| MenuEntry::check(100 + i as u32, n.clone(), *n == cur))
            .collect();
        if items.len() <= 24 {
            e.extend(items);
        } else {
            e.push(MenuEntry::Sub("Load".into(), items));
        }
        e.push(MenuEntry::Sep);
        e.push(MenuEntry::Item {
            id: 4,
            text: "Rename…".into(),
            checked: false,
            enabled: !cur.is_empty(),
        });
        e.push(MenuEntry::Item {
            id: 5,
            text: "Delete…".into(),
            checked: false,
            enabled: !cur.is_empty(),
        });
        match self.fxc_popup(x, y, &e) {
            1 => self.fxc_init(),
            2 => {
                let n = if cur.is_empty() {
                    self.fxc_ask("Save the FX Constellation preset", "Name", "")
                } else {
                    Some(cur)
                };
                if let Some(n) = n {
                    self.fxc_save_preset(&n);
                }
            }
            3 => {
                if let Some(n) = self.fxc_ask("Save the preset as", "Name", &cur) {
                    self.fxc_save_preset(&n);
                }
            }
            4 => {
                if let Some(n) = self.fxc_ask("Rename the preset", "Name", &cur) {
                    if let (Some(a), Some(b)) =
                        (self.fxc_preset_path(&cur), self.fxc_preset_path(&n))
                    {
                        if std::fs::rename(a, b).is_ok() {
                            self.fxc.t.preset = n;
                            self.fxc_changed();
                        }
                    }
                }
            }
            5 => {
                if self.r.xc_message(
                    "FX Constellation",
                    &format!("Delete the preset '{cur}'?"),
                    4,
                ) == 6
                {
                    if let Some(a) = self.fxc_preset_path(&cur) {
                        let _ = std::fs::remove_file(a);
                    }
                    self.fxc.t.preset.clear();
                    self.fxc.preset = None;
                    self.fxc_changed();
                }
            }
            i if i >= 100 => {
                if let Some(n) = list.get((i - 100) as usize).cloned() {
                    self.fxc_load_preset(&n);
                }
            }
            _ => {}
        }
    }

    fn fxc_snap_menu(&mut self, x: f32, y: f32) {
        let Some(p) = self.fxc.preset.clone() else {
            let e = vec![
                MenuEntry::off("A snapshot lives inside a preset"),
                MenuEntry::item(1, "Save a new preset…"),
            ];
            if self.fxc_popup(x, y, &e) == 1 {
                if let Some(n) = self.fxc_ask("Save the FX Constellation preset", "Name", "") {
                    self.fxc_save_preset(&n);
                }
            }
            return;
        };
        let cur = self.fxc.t.snapshot.clone();
        let next = next_snapshot_name(&p);
        let names: Vec<String> = p.snapshots.keys().cloned().collect();
        let mut e = vec![
            MenuEntry::off(format!("Snapshots of {}", self.fxc.t.preset)),
            MenuEntry::item(1, format!("Capture as '{next}'")),
            MenuEntry::item(2, "Capture as…"),
            MenuEntry::Item {
                id: 3,
                text: format!("Overwrite '{cur}'"),
                checked: false,
                enabled: !cur.is_empty(),
            },
            MenuEntry::Sep,
        ];
        for (i, n) in names.iter().enumerate() {
            e.push(MenuEntry::check(100 + i as u32, n.clone(), *n == cur));
        }
        e.push(MenuEntry::Sep);
        e.push(MenuEntry::Item {
            id: 4,
            text: format!("Delete '{cur}'"),
            checked: false,
            enabled: !cur.is_empty(),
        });
        e.push(MenuEntry::Sep);
        e.push(MenuEntry::item(5, "Set A = now"));
        e.push(MenuEntry::item(6, "Set B = now"));
        e.push(MenuEntry::item(7, "Clear A / B"));
        match self.fxc_popup(x, y, &e) {
            1 => self.fxc_capture_snapshot(&next),
            2 => {
                if let Some(n) = self.fxc_ask("Capture a snapshot", "Name", &next) {
                    self.fxc_capture_snapshot(&n);
                }
            }
            3 => self.fxc_capture_snapshot(&cur),
            4 => {
                if let Some(pr) = self.fxc.preset.as_mut() {
                    pr.snapshots.remove(&cur);
                    let pr = pr.clone();
                    if let Some(path) = self.fxc_preset_path(&self.fxc.t.preset.clone()) {
                        let _ = std::fs::write(path, cp_fxc::preset::encode(&pr));
                    }
                }
                self.fxc.t.snapshot.clear();
                self.fxc_changed();
            }
            5 => {
                self.fxc.t.morph_a = Some(self.fxc_capture_morph());
                self.fxc_changed();
            }
            6 => {
                self.fxc.t.morph_b = Some(self.fxc_capture_morph());
                self.fxc_changed();
            }
            7 => {
                self.fxc.t.morph_a = None;
                self.fxc.t.morph_b = None;
                self.fxc_changed();
            }
            i if i >= 100 => {
                if let Some(n) = names.get((i - 100) as usize).cloned() {
                    self.fxc_recall_snapshot(&n);
                }
            }
            _ => {}
        }
    }

    /// Les reglages des effets internes menes par des sources, par genre
    /// d'effet (les GUID changent d'une piste ou d'un rappel a l'autre).
    fn fxc_internal_capture(&self) -> BTreeMap<(String, usize), PState> {
        let g = self.fxc_guids();
        let kinds = [
            ("sg", g.sg),
            ("seq", g.seq),
            ("bank", g.bank),
            ("bridge", g.bridge),
        ];
        let mut out = BTreeMap::new();
        for (k, ps) in &self.fxc.t.params {
            if !ps.sel || ps.srcs.is_empty() {
                continue;
            }
            let Some((fg, i)) = split_pkey(k) else {
                continue;
            };
            if let Some((name, _)) = kinds.iter().find(|(_, x)| x.as_deref() == Some(fg)) {
                out.insert((name.to_string(), i), ps.clone());
            }
        }
        out
    }

    fn fxc_internal_restore(&mut self, int: &BTreeMap<(String, usize), PState>) {
        // Ce que les effets internes portaient s'en va, puis ce que le preset
        // dit.
        let g = self.fxc_guids();
        let ours: Vec<String> = [&g.sg, &g.seq, &g.bank, &g.bridge]
            .iter()
            .filter_map(|x| (*x).clone())
            .collect();
        self.fxc
            .t
            .params
            .retain(|k, _| split_pkey(k).is_none_or(|(fg, _)| !ours.iter().any(|o| o == fg)));
        for ((kind, i), ps) in int {
            if kind == "bridge" && self.fxc.chain.find(FxKind::Bridge).is_none() {
                self.fxc_ensure_bridge();
            }
            if kind == "bank" && self.fxc.chain.find(FxKind::Bank).is_none() {
                self.fxc_ensure_bank();
            }
            let g = self.fxc_guids();
            let fg = match kind.as_str() {
                "sg" => g.sg,
                "seq" => g.seq,
                "bank" => g.bank,
                _ => g.bridge,
            };
            if let Some(fg) = fg {
                self.fxc.t.params.insert(pkey(&fg, *i), ps.clone());
            }
        }
    }

    /// Init : la chaine de ROLL vide, les liens relaches, le pad et les
    /// sources a leur naissance. Les effets DETACHED restent.
    pub(crate) fn fxc_init(&mut self) {
        if !crate::gtab::selftest()
            && self.r.xc_message(
                "FX Constellation",
                "Init clears this track's ROLL chain, releases every FX Constellation link, untracks all params and resets the modulators, the macros and the pad. DETACHED FX stay where they are. Undo restores everything.",
                1,
            ) != 1
        {
            return;
        }
        let Some(t) = self.fxc_t() else { return };
        let r = self.r;
        r.xc_undo_begin();
        self.fxc.t.params.clear();
        self.fxc.t.pad = Pad::default();
        self.fxc.t.macro_names = Default::default();
        self.fxc.t.preset.clear();
        self.fxc.t.snapshot.clear();
        self.fxc.t.morph_a = None;
        self.fxc.t.morph_b = None;
        self.fxc.t.grains.clear();
        self.fxc_apply();
        let detached: Vec<String> = self
            .fxc
            .rows
            .iter()
            .filter(|x| x.detached)
            .map(|x| x.guid.clone())
            .collect();
        for i in (0..r.xc_fx_count(t)).rev() {
            let g = r.xc_fx_guid(t, i).unwrap_or_default();
            if fx_kind_at(&r, t, i) == FxKind::User && !detached.contains(&g) {
                r.xc_fx_delete(t, i);
            }
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::Bank) {
            for i in 1..=8 {
                r.xc_set_raw(t, fx, bank::on(i), (i == 1) as u8 as f64);
                r.xc_set_raw(t, fx, bank::shape(i), 0.0);
                r.xc_set_raw(t, fx, bank::sync(i), 0.0);
                r.xc_set_raw(t, fx, bank::phase(i), 0.0);
                r.xc_set(
                    t,
                    fx,
                    bank::rate(i),
                    ((1.0f64 / 0.01).ln() / 2000f64.ln()).clamp(0.0, 1.0),
                );
            }
        }
        if let Some(b) = self.fxc.chain.find(FxKind::Bridge) {
            for n in 0..8 {
                r.xc_set_raw(t, b, bridge::MACRO_IN + n, 0.0);
            }
            r.xc_set_raw(t, b, bridge::X_POS, 0.5);
            r.xc_set_raw(t, b, bridge::Y_POS, 0.5);
        }
        self.fxc.preset = None;
        self.fxc.eng_cache.clear();
        self.fxc_scan_chain();
        self.fxc_count("init");
        self.fxc_changed();
        self.fxc_apply();
        r.xc_undo_end("FX Constellation: Init");
    }

    /// Les liens de la piste avant de deplacer des effets (par GUID).
    fn fxc_snapshot_for_move(&self, t: Tk) -> Vec<(String, usize, String)> {
        let r = self.r;
        let n = r.xc_fx_count(t);
        let guids: Vec<String> = (0..n)
            .map(|i| r.xc_fx_guid(t, i).unwrap_or_default())
            .collect();
        let mut out = Vec::new();
        for (i, g) in guids.iter().enumerate() {
            for p in 0..r.xc_param_count(t, i) {
                if r.xc_cfg(t, i, &format!("param.{p}.plink.active"))
                    .as_deref()
                    != Some("1")
                {
                    continue;
                }
                let e = parse(r.xc_cfg(t, i, &format!("param.{p}.plink.effect"))).unwrap_or(-1.0)
                    as i64;
                if e >= 0 && (e as usize) < n {
                    out.push((g.clone(), p, guids[e as usize].clone()));
                }
            }
        }
        out
    }

    fn fxc_repair_after_move(&self, t: Tk, snap: &[(String, usize, String)]) {
        let r = self.r;
        for (g, p, src) in snap {
            let (Some(i), Some(s)) = (r.xc_fx_index(t, g), r.xc_fx_index(t, src)) else {
                continue;
            };
            let key = format!("param.{p}.plink.effect");
            if parse(r.xc_cfg(t, i, &key)).map(|e| e as i64) != Some(s as i64) {
                r.xc_set_cfg(t, i, &key, &s.to_string());
            }
        }
    }
}

fn fx_kind_at(r: &cp_sys::reaper::Reaper, t: Tk, i: usize) -> FxKind {
    cp_fxc::slots::fx_kind(&r.xc_fx_name(t, i))
}

fn unshape_log(v: f64) -> f64 {
    v.exp()
}

/// Un champ d'un slot de banque.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SlotField {
    Shape,
    Rate,
    Sync,
    Phase,
    Slew,
    Curve,
    Mode,
    Trig,
    Uni,
    Sh,
    Len,
}

/// Une ecriture dans un slot.
#[derive(Clone, Debug)]
pub(crate) enum SlotW {
    On(Option<bool>),
    Raw(SlotField, f64),
    Norm(SlotField, f64),
    Steps([u8; 16]),
}

#[allow(dead_code)]
fn _unused(_: Track) {}
