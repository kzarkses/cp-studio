//! FX Constellation dans REAPER : sa fenetre (arrangement, docker ou
//! flottante), la piste suivie, ce qu'on garde, la boucle.
//!
//! - Le modele (`cp_fxc`) dit ce que la piste veut ; `fxc_sync` le lit et
//!   l'ecrit dans REAPER (la chaine, les liens, le bridge) ; `fxc_act`
//!   traduit les gestes de la vue.
//! - L'etat d'une piste vit dans le projet (ExtState de projet `CPS_FXC`,
//!   cle = GUID de la piste). Une piste qui n'en a pas adopte ses liens.
//! - Les reglages de ROLL et de la fenetre sont des preferences
//!   (ExtState `CP_Studio`, cles `fxc_*`).

use std::collections::BTreeMap;
use std::time::Instant;

use cp_abi::{
    PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK, VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL, WM_CP_FRAME,
};
use cp_fxc::links::{Chain, Plan};
use cp_fxc::model::Track;
use cp_fxc::roll::{Rng, Settings};
use cp_fxcview::{Act, Bank, Ctl, Data, FxRow, Js, KeyUse, View};
use cp_gfx::IRect;
use cp_sys::fxc::Tk;
use cp_sys::module::key_event;
use cp_sys::ui;
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, Theme};

use crate::gtab::{LOOK, selftest};
use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

pub(crate) const TITLE: &str = "FX Constellation";
pub(crate) const IDENT: &str = "cp_studio_fxconstellation";
/// La section de projet de l'etat des pistes.
pub(crate) const PROJ: &str = "CPS_FXC";

pub(crate) struct CWin {
    pub hwnd: Hwnd,
    pub place: Place,
    frame: crate::frame::Frame,
    pub(crate) theme: Theme,
    theme_key: Vec<Option<(u8, u8, u8)>>,
    tracking: bool,
    closing: bool,
    tip: Option<ui::Tooltip>,
    hover: (Ctl, Instant),
}

/// Ce que FX Constellation tient.
pub(crate) struct CPart {
    pub view: View,
    pub win: Option<CWin>,
    pub place: Place,
    /// La piste montree, son GUID.
    pub(crate) tk: Option<Tk>,
    pub(crate) guid: String,
    pub(crate) locked: bool,
    /// Le modele de la piste montree.
    pub(crate) t: Track,
    pub(crate) dirty_at: Option<Instant>,
    pub(crate) chain: Chain,
    /// La signature de la chaine (noms et nombres de parametres).
    pub(crate) sig: String,
    pub(crate) rows: Vec<FxRow>,
    pub(crate) plan: Plan,
    pub(crate) bank: Option<Bank>,
    pub(crate) gbank: Option<Bank>,
    pub(crate) sg: Option<Js>,
    pub(crate) seq: Option<Js>,
    pub(crate) macros: [f64; 8],
    pub(crate) src: BTreeMap<i32, f64>,
    pub(crate) roll: Settings,
    pub(crate) rng: Rng,
    /// Les liens sont a refaire (la passe attend que la souris soit levee
    /// pour ce qui restructure la chaine).
    pub(crate) links_dirty: bool,
    pub(crate) flash: Option<(String, Instant)>,
    pub(crate) ticks: u32,
    /// Ce que le bridge a recu (pour n'ecrire que ce qui change).
    pub(crate) eng_cache: Vec<f64>,
    /// La valeur de chaque parametre au passage (le double-clic y revient).
    pub(crate) scan: BTreeMap<String, f64>,
    /// Les noms des parametres, par effet (GUID, nombre de parametres).
    pub(crate) names: BTreeMap<(String, usize), Vec<(String, u8)>>,
    pub(crate) kb_note: Option<u8>,
    pub(crate) morph: f64,
    /// Le prochain effet ajoute va hors de ROLL.
    pub(crate) next_detached: Option<usize>,
    /// Un refus de restructurer, retenu par piste (pas redemande).
    pub(crate) refused: BTreeMap<String, String>,
    /// Le preset de la piste, lu (ses snapshots).
    pub(crate) preset: Option<cp_fxc::preset::Preset>,
    pub(crate) preset_modified: bool,
    pub(crate) prefs_dirty: bool,
    /// L'auto-test : des compteurs.
    pub(crate) stats: BTreeMap<&'static str, u32>,
    /// Le point que le pad lisse rattrape.
    pub(crate) pad_target: Option<(f64, f64)>,
    /// Les liens que nous avons ecrits (par cle), pour les relacher meme
    /// quand le modele les a oublies.
    pub(crate) linked: std::collections::BTreeSet<String>,
    /// La base que chaque lien avait a notre derniere ecriture (une base
    /// changee hors de FX Constellation se reprend).
    pub(crate) seen_base: BTreeMap<String, f64>,
    /// L'etat ecrit en dernier dans la piste (une annulation le change).
    pub(crate) last_saved: String,
    /// La capture en enveloppes.
    pub(crate) capture: crate::fxc_capture::Capture,
    /// Les parametres sur lesquels REAPER refuse un lien (des reglages
    /// discrets de certains effets) : ni suivis, ni tires par ROLL.
    pub(crate) unlinkable: std::collections::BTreeSet<String>,
}

impl CPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> CPart {
        let place = Place::from_key(&r.ext_get(SECTION, "fxc_place").unwrap_or_default())
            .unwrap_or(Place::Float);
        let mut view = View::new();
        let roll = r
            .ext_get(SECTION, "fxc_roll")
            .and_then(|s| crate::fxc_act::decode_roll(&s))
            .unwrap_or_default();
        if let Some(s) = r.ext_get(SECTION, "fxc_view") {
            crate::fxc_act::decode_view(&s, &mut view);
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(7, |d| d.as_nanos() as u64);
        CPart {
            view,
            win: None,
            place,
            tk: None,
            guid: String::new(),
            locked: false,
            t: Track::default(),
            dirty_at: None,
            chain: Chain::default(),
            sig: String::new(),
            rows: Vec::new(),
            plan: Plan::default(),
            bank: None,
            gbank: None,
            sg: None,
            seq: None,
            macros: [0.0; 8],
            src: BTreeMap::new(),
            roll,
            rng: Rng::new(seed),
            links_dirty: false,
            flash: None,
            ticks: 0,
            eng_cache: Vec::new(),
            scan: BTreeMap::new(),
            names: BTreeMap::new(),
            kb_note: None,
            morph: 0.0,
            next_detached: None,
            refused: BTreeMap::new(),
            preset: None,
            preset_modified: false,
            prefs_dirty: false,
            stats: BTreeMap::new(),
            pad_target: None,
            linked: Default::default(),
            seen_base: BTreeMap::new(),
            last_saved: String::new(),
            capture: Default::default(),
            unlinkable: Default::default(),
        }
    }
}

impl App {
    pub(crate) fn fxc_keep(&self, key: &str, value: &str) {
        if !selftest() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    pub(crate) fn fxc_say(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.host.info(&format!("FX Constellation : {s}"));
        self.fxc.flash = Some((s, Instant::now()));
        self.fxc.view.mark();
    }

    pub(crate) fn fxc_count(&mut self, k: &'static str) {
        *self.fxc.stats.entry(k).or_insert(0) += 1;
    }

    /// Le modele a change : la vue, la sauvegarde, les liens.
    pub(crate) fn fxc_changed(&mut self) {
        self.fxc.dirty_at = Some(Instant::now());
        self.fxc.links_dirty = true;
        self.fxc.view.mark();
    }

    // -----------------------------------------------------------------------
    // La piste suivie et ce qu'on garde
    // -----------------------------------------------------------------------

    /// Ecrit l'etat de la piste montree dans la piste (`P_EXT:CPS_FXC`) :
    /// l'annulation de REAPER le ramene avec le reste. Un etat trop long
    /// (48 Ko) va dans le projet, sans annulation.
    pub(crate) fn fxc_save_now(&mut self) {
        self.fxc.dirty_at = None;
        let Some(t) = self.fxc.tk.filter(|&t| self.r.xc_guid(t).is_some()) else {
            return;
        };
        if self.fxc.guid.is_empty() {
            return;
        }
        let s = cp_fxc::store::encode(&self.fxc.t);
        if s == self.fxc.last_saved {
            return;
        }
        if s.len() < 48_000 {
            self.r.xc_set_track_ext(t, PROJ, &s);
            if self.r.xc_proj_ext(PROJ, &self.fxc.guid).is_some() {
                self.r.xc_set_proj_ext(PROJ, &self.fxc.guid, "");
            }
        } else {
            self.r.xc_set_track_ext(t, PROJ, "");
            self.r.xc_set_proj_ext(PROJ, &self.fxc.guid, &s);
        }
        self.fxc.last_saved = s;
    }

    /// L'etat garde de la piste : la piste d'abord, puis le projet.
    pub(crate) fn fxc_stored(&self, t: Tk) -> Option<String> {
        self.r
            .xc_track_ext(t, PROJ)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.r
                    .xc_proj_ext(PROJ, &self.fxc.guid)
                    .filter(|s| !s.is_empty())
            })
    }

    /// L'etat de la piste a change sans nous (une annulation, un autre
    /// projet) : le modele le reprend.
    pub(crate) fn fxc_reload_if_changed(&mut self) {
        let Some(t) = self.fxc.tk.filter(|&t| self.r.xc_guid(t).is_some()) else {
            return;
        };
        let Some(s) = self.fxc_stored(t) else { return };
        if s == self.fxc.last_saved {
            return;
        }
        if let Some(st) = cp_fxc::store::decode(&s) {
            self.fxc.t = st;
            self.fxc.last_saved = s;
            self.fxc.eng_cache.clear();
            self.fxc_read_preset();
            self.fxc.links_dirty = true;
            self.fxc.view.mark();
            self.fxc_count("reload");
        }
    }

    pub(crate) fn fxc_save_prefs(&mut self) {
        self.fxc.prefs_dirty = false;
        let r = crate::fxc_act::encode_roll(&self.fxc.roll);
        self.fxc_keep("fxc_roll", &r);
        let v = crate::fxc_act::encode_view(&self.fxc.view);
        self.fxc_keep("fxc_view", &v);
    }

    /// La piste a montrer : celle qu'on tient, sinon la piste choisie.
    fn fxc_follow(&mut self) {
        let cur = self.fxc.tk.filter(|&t| self.r.xc_guid(t).is_some());
        if self.fxc.locked && cur.is_some() {
            return;
        }
        if self.fxc.locked && cur.is_none() {
            self.fxc.locked = false;
        }
        let want = self.r.xc_selected();
        let want_guid = want.and_then(|t| self.r.xc_guid(t)).unwrap_or_default();
        if want_guid == self.fxc.guid && cur.is_some() {
            return;
        }
        self.fxc_arrive(want, want_guid);
    }

    /// Change de piste : garde l'ancienne, lit la nouvelle.
    pub(crate) fn fxc_arrive(&mut self, tk: Option<Tk>, guid: String) {
        if self.fxc.dirty_at.is_some() {
            self.fxc_save_now();
        }
        self.fxc_keyboard_release();
        self.fxc.tk = tk;
        self.fxc.guid = guid;
        self.fxc.sig.clear();
        self.fxc.eng_cache.clear();
        self.fxc.scan.clear();
        self.fxc.linked.clear();
        self.fxc.seen_base.clear();
        self.fxc.last_saved.clear();
        self.fxc.capture = Default::default();
        self.fxc.preset = None;
        self.fxc.preset_modified = false;
        self.fxc.view.carry = None;
        self.fxc.view.scroll[0] = 0.0;
        self.fxc.rows.clear();
        self.fxc.plan = Plan::default();
        let Some(t) = tk else {
            self.fxc.t = Track::default();
            self.fxc.view.mark();
            return;
        };
        self.fxc_scan_chain();
        let raw_state = self.fxc_stored(t);
        let stored = raw_state.as_deref().and_then(cp_fxc::store::decode);
        match stored {
            Some(st) => {
                self.fxc.t = st;
                self.fxc.last_saved = raw_state.unwrap_or_default();
            }
            None => {
                // Une piste jamais vue : son pad vient du bridge, ses
                // parametres de ses liens.
                self.fxc.t = Track::default();
                let n = self.fxc_adopt();
                if n > 0 {
                    self.host.info(&format!(
                        "FX Constellation : {n} parametre(s) adopte(s) de leurs liens"
                    ));
                    self.fxc.dirty_at = Some(Instant::now());
                }
            }
        }
        self.fxc_read_preset();
        let _ = t;
        self.fxc.links_dirty = true;
        self.fxc.view.mark();
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    pub(crate) fn fxc_toggle(&mut self) {
        if self.fxc.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd)) && !self.fxc_hidden_in_band()
        {
            self.queue(WinOp::FxcClose);
        } else {
            self.queue(WinOp::FxcOpen);
        }
    }

    fn fxc_hidden_in_band(&self) -> bool {
        self.fxc
            .win
            .as_ref()
            .is_some_and(|w| w.place == Place::Arrange)
            && !self.fxc_in_band()
    }

    pub(crate) fn fxc_in_band(&self) -> bool {
        let Some(w) = &self.fxc.win else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(w.hwnd)
    }

    pub(crate) fn fxc_open(&mut self) {
        crate::fxc_sync::install_jsfx(&self.r, &self.host);
        if let Some(w) = &self.fxc.win {
            let h = w.hwnd;
            match w.place {
                Place::Dock => self.r.dock_activate(h),
                Place::Float => win::show(h),
                Place::Arrange => {
                    if !self.fxc_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true) {
                        self.fxc_render(true);
                    }
                }
            }
            ui::set_focus(h);
            self.fxc_render(true);
            return;
        }
        let place = self.fxc.place;
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 1400, 800, mode) else {
            self.host
                .error("la fenetre de FX Constellation n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange && !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.fxc.place = Place::Float;
            self.fxc_open();
            return;
        }
        self.fxc_adopt_window(hwnd);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.fxc_render(true);
        }
        ui::set_focus(hwnd);
        self.fxc_keep("fxc_open", "1");
    }

    pub(crate) fn fxc_adopt_window(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.fxc.win = Some(CWin {
            hwnd,
            place: self.fxc.place,
            frame: crate::frame::Frame::default(),
            theme: self.reaper_theme(scale),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            closing: false,
            tip: None,
            hover: (Ctl::None, Instant::now()),
        });
        self.fxc_follow();
        self.fxc.view.mark();
        self.fxc_render(true);
    }

    pub(crate) fn fxc_shutdown(&mut self) {
        let h = self.fxc.win.as_ref().map_or(0, |w| w.hwnd.0);
        if let Some(w) = &self.fxc.win {
            self.host.frames(w.hwnd, false);
        }
        self.r.ext_set(SECTION, "fxc_hwnd", &h.to_string(), false);
        self.fxc_keyboard_release();
        if self.fxc.dirty_at.is_some() {
            self.fxc_save_now();
        }
        if self.fxc.prefs_dirty {
            self.fxc_save_prefs();
        }
    }

    pub(crate) fn fxc_close(&mut self, user: bool) {
        let in_band = self.fxc_in_band();
        self.fxc_keyboard_release();
        if let Some(w) = &mut self.fxc.win {
            w.closing = true;
            let h = w.hwnd;
            if in_band {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.fxc.win = None;
        if user {
            self.fxc_keep("fxc_open", "0");
        }
        if self.fxc.dirty_at.is_some() {
            self.fxc_save_now();
        }
        if self.fxc.prefs_dirty {
            self.fxc_save_prefs();
        }
    }

    pub(crate) fn fxc_set_place(&mut self, p: Place) {
        let reopen = self.fxc.win.as_ref().is_some_and(|w| w.place != p);
        self.fxc.place = p;
        self.fxc_keep("fxc_place", p.key());
        if reopen {
            self.fxc_close(false);
        }
        self.fxc_open();
    }

    pub(crate) fn fxc_place_menu_at(&mut self, owner: Hwnd, sx: i32, sy: i32) {
        let p = self.fxc.place;
        let open = self.fxc.win.is_some();
        let item = |id, text, checked, enabled| ui::MenuItem {
            id,
            text,
            checked,
            enabled,
        };
        let items = [
            Some(item(1, "Show in arrange view", p == Place::Arrange, true)),
            Some(item(2, "Dock in docker", p == Place::Dock, true)),
            Some(item(3, "Floating window", p == Place::Float, true)),
            None,
            Some(item(4, "Close FX Constellation", false, open)),
        ];
        match ui::popup_menu(owner, sx, sy, &items) {
            1 => self.queue(WinOp::FxcPlace(Place::Arrange)),
            2 => self.queue(WinOp::FxcPlace(Place::Dock)),
            3 => self.queue(WinOp::FxcPlace(Place::Float)),
            4 => self.queue(WinOp::FxcClose),
            _ => {}
        }
    }

    pub(crate) fn fxc_theme(&self) -> Theme {
        self.fxc
            .win
            .as_ref()
            .map_or_else(|| Theme::dark(1.0), |w| w.theme.clone())
    }

    fn fxc_want_frames(&mut self, on: bool) {
        if let Some(w) = &mut self.fxc.win {
            if on {
                w.frame.wake();
            }
            if w.frame.frames_on != on {
                w.frame.frames_on = on;
                self.host.frames(w.hwnd, on);
            }
        }
    }

    pub(crate) fn fxc_kick(&mut self) {
        let on = self.fxc.win.as_ref().is_some_and(|w| w.frame.frames_on);
        if !on {
            self.fxc_render(false);
        }
        self.fxc_want_frames(true);
    }

    /// Les donnees de l'image.
    fn fxc_layout(&mut self, w: f32, h: f32) {
        let th = self.fxc_theme();
        let name = self.fxc.tk.map(|t| self.r.xc_track_name(t).0);
        let msg = self.fxc.flash.as_ref().map(|f| f.0.clone());
        let mut view = std::mem::take(&mut self.fxc.view);
        let p = &self.fxc;
        let snap_label = match &p.preset {
            None => "snapshots".to_string(),
            Some(pr) => {
                if !p.t.snapshot.is_empty() && pr.snapshots.contains_key(&p.t.snapshot) {
                    p.t.snapshot.clone()
                } else if pr.snapshots.is_empty() {
                    "no snapshot".into()
                } else {
                    format!("{} snapshots", pr.snapshots.len())
                }
            }
        };
        let morph = (p.t.morph_a.is_some() && p.t.morph_b.is_some()).then_some(p.morph);
        let modulated =
            p.t.params
                .values()
                .filter(|s| s.sel && !s.srcs.is_empty())
                .count();
        let d = Data {
            track: name.as_deref(),
            locked: p.locked,
            t: &p.t,
            fx: &p.rows,
            plan: &p.plan,
            bridge: p.chain.find(cp_fxc::slots::FxKind::Bridge).is_some(),
            engine_runs: p.t.pad.nav_on && p.t.pad.nav != cp_fxc::model::Nav::Manual,
            bank: p.bank.as_ref(),
            gbank: p.gbank.as_ref(),
            sg: p.sg.as_ref(),
            seq: p.seq.as_ref(),
            macros: p.macros,
            preset: &p.t.preset,
            modified: p.preset_modified,
            snapshot_label: snap_label,
            has_preset: p.preset.is_some(),
            morph,
            roll: &p.roll,
            links: p.plan.links.len(),
            modulated,
            src: &p.src,
            msg: msg.as_deref(),
            kb_note: p.kb_note,
            guids: self.fxc_guids(),
            capture: self.fxc.capture.armed.len(),
        };
        view.layout(&d, &th, w, h);
        self.fxc.view = view;
    }

    pub(crate) fn fxc_render(&mut self, force: bool) {
        let Some(w) = self.fxc.win.as_mut() else {
            return;
        };
        if !win::is_window(w.hwnd) {
            return;
        }
        let rc = ui::client_rect(w.hwnd);
        let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
        if cw <= 0 || ch <= 0 {
            return;
        }
        let scale = ui::dpi(w.hwnd) as f32 / 96.0;
        if (scale - w.theme.scale).abs() > 0.01 {
            let th = self.reaper_theme(scale);
            let Some(w) = self.fxc.win.as_mut() else {
                return;
            };
            w.theme = th;
            self.fxc.view.mark();
        }
        let Some(w) = self.fxc.win.as_mut() else {
            return;
        };
        if w.frame.resize(cw, ch) || force {
            self.fxc.view.mark();
        }
        if !self.fxc.view.take_dirty() {
            w.frame.show(w.hwnd, &[], &[]);
            return;
        }
        self.fxc_layout(cw as f32, ch as f32);
        let Some(w) = self.fxc.win.as_mut() else {
            return;
        };
        let damage = vec![IRect::new(0, 0, cw, ch)];
        let view = &self.fxc.view;
        let (text, icons, th) = (&mut self.text, &mut self.icons, &w.theme);
        w.frame.paint(&damage, &mut self.scratch, |cv| {
            view.draw(cv, text, icons, th);
        });
        w.frame.show(w.hwnd, &damage, &[]);
    }

    pub(crate) fn fxc_feed(&mut self, ev: Ev) {
        let a = self.fxc.view.event(&ev);
        if let Some(a) = a {
            self.fxc_act(a);
        }
        self.fxc_kick();
    }

    fn fxc_tip(&mut self) {
        let h = self.fxc.view.hover.clone();
        let dragging = self.fxc.view.dragging();
        let Some(w) = self.fxc.win.as_mut() else {
            return;
        };
        if h != w.hover.0 {
            w.hover = (h, Instant::now());
            if let Some(t) = &mut w.tip {
                t.hide();
            }
            return;
        }
        let shown = w.tip.as_ref().is_some_and(|t| t.is_shown());
        if dragging && shown {
            if let Some(t) = &mut w.tip {
                t.hide();
            }
        }
        if shown || dragging || w.hover.1.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        let text = self.fxc.view.label(&h);
        if text.is_empty() {
            return;
        }
        let (sx, sy) = ui::cursor_pos();
        let Some(w) = self.fxc.win.as_mut() else {
            return;
        };
        if w.tip.is_none() {
            w.tip = ui::Tooltip::new(w.hwnd);
        }
        if let Some(t) = &mut w.tip {
            t.show(&text, sx + 12, sy + 20);
        }
    }

    pub(crate) fn fxc_tick(&mut self) {
        self.fxc.ticks = self.fxc.ticks.wrapping_add(1);
        if self
            .fxc
            .dirty_at
            .is_some_and(|t| t.elapsed().as_secs_f64() > 0.8)
            && !self.fxc.view.dragging()
        {
            self.fxc_save_now();
        }
        if self.fxc.prefs_dirty && self.fxc.ticks % 30 == 0 {
            self.fxc_save_prefs();
        }
        if self
            .fxc
            .flash
            .as_ref()
            .is_some_and(|f| f.1.elapsed().as_secs_f64() > 3.0)
        {
            self.fxc.flash = None;
            self.fxc.view.mark();
        }
        let Some(hwnd) = self.fxc.win.as_ref().map(|w| w.hwnd) else {
            return;
        };
        if !ui::is_shown(hwnd) {
            return;
        }
        let down = self.fxc.view.dragging();
        if self.fxc.ticks % 3 == 0 && !down {
            self.fxc_follow();
        }
        if self.fxc.ticks % 10 == 7 && !down && self.fxc.dirty_at.is_none() {
            self.fxc_reload_if_changed();
        }
        self.fxc_sync_tick(down);
        self.fxc_capture_tick();
        if self.fxc.ticks % 10 == 0 {
            let key: Vec<_> = LOOK.iter().map(|k| self.r.theme_color(k)).collect();
            if let Some(w) = &self.fxc.win {
                if w.theme_key != key {
                    let sc = w.theme.scale;
                    let th = self.reaper_theme(sc);
                    if let Some(w) = &mut self.fxc.win {
                        w.theme = th;
                        w.theme_key = key;
                    }
                    self.fxc.view.mark();
                }
            }
        }
        self.fxc_tip();
        self.fxc_render(false);
    }

    pub(crate) fn fxc_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.fxc.win.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        match m.msg {
            WM_CP_FRAME => {
                if self.fxc.view.dragging() {
                    self.fxc.view.mark();
                }
                self.fxc_render(false);
                let idle = self.fxc.win.as_ref().is_none_or(|w| w.frame.idle());
                if !self.fxc.view.dragging() && idle {
                    self.fxc_want_frames(false);
                }
                Some(0)
            }
            WM_CP_ACCEL => {
                let Some(k) = key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                let usage = self.fxc.view.key_use(vk, md);
                Some(match k.message {
                    msg::KEYDOWN | msg::SYSKEYDOWN => match usage {
                        KeyUse::Eat => {
                            self.fxc_feed(Ev::Key {
                                vk,
                                mods: md,
                                text: None,
                            });
                            1
                        }
                        KeyUse::NotMine => 0,
                    },
                    msg::KEYUP | msg::SYSKEYUP => match usage {
                        KeyUse::Eat => 1,
                        KeyUse::NotMine => 0,
                    },
                    _ => 0,
                })
            }
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001),
            msg::KEYDOWN | msg::SYSKEYDOWN => {
                self.fxc_feed(Ev::Key {
                    vk: m.wp as u32,
                    mods: md,
                    text: None,
                });
                Some(0)
            }
            msg::SIZE => {
                self.fxc.view.mark();
                self.fxc_render(true);
                Some(0)
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                let shown = self.fxc.win.as_mut().is_some_and(|w| w.frame.repaint(hwnd));
                if !shown {
                    self.fxc_render(true);
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::SETCURSOR if (m.lp & 0xFFFF) == 1 => {
                let (cx, cy) = Self::mouse_over(hwnd)?;
                let p = match self.fxc.view.hit(cx, cy) {
                    Ctl::DockHandle => cp_ui::Pointer::ItemMove,
                    Ctl::Seam(_) => cp_ui::Pointer::ResizeWE,
                    Ctl::TileDot(_) | Ctl::MacroDot(_) | Ctl::AxisDot(_) => {
                        cp_ui::Pointer::HandScroll
                    }
                    _ => return None,
                };
                ui::set_pointer(p);
                Some(1)
            }
            msg::MOUSEMOVE => {
                if let Some(w) = &mut self.fxc.win {
                    if !w.tracking {
                        w.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.fxc_feed(Ev::Move { x, y, mods: md });
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(w) = &mut self.fxc.win {
                    w.tracking = false;
                }
                if !self.fxc.view.dragging() {
                    self.fxc_feed(Ev::Leave);
                }
                Some(0)
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                ui::capture(hwnd);
                let b = match m.msg {
                    msg::LBUTTONDOWN | msg::LBUTTONDBLCLK => Button::Left,
                    _ => Button::Right,
                };
                if let Some(w) = &mut self.fxc.win {
                    if let Some(t) = &mut w.tip {
                        t.hide();
                    }
                }
                self.fxc_feed(Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                });
                Some(0)
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                let b = if m.msg == msg::LBUTTONUP {
                    Button::Left
                } else {
                    Button::Right
                };
                self.fxc_feed(Ev::Up { x, y, b, mods: md });
                Some(0)
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                let (cx, cy) = ui::screen_to_client(hwnd, x as i32, y as i32);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                let (dx, dy) = if m.msg == msg::MOUSEWHEEL {
                    (0.0, d)
                } else {
                    (d, 0.0)
                };
                self.fxc_feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
                Some(0)
            }
            msg::CLOSE => {
                self.queue(WinOp::FxcClose);
                Some(0)
            }
            msg::DESTROY => {
                let ours = self.fxc.win.as_ref().is_some_and(|w| w.closing);
                if !ours {
                    self.fxc.win = None;
                }
                None
            }
            _ => None,
        }
    }

    /// Les GUID des effets internes de la piste.
    pub(crate) fn fxc_guids(&self) -> cp_fxcview::Guids {
        use cp_fxc::slots::FxKind;
        let c = &self.fxc.chain;
        let g = |k| c.find(k).and_then(|i| c.fx.get(i)).map(|f| f.guid.clone());
        cp_fxcview::Guids {
            sg: g(FxKind::SoundGen),
            seq: g(FxKind::Seq),
            bank: g(FxKind::Bank),
            bridge: g(FxKind::Bridge),
        }
    }

    /// Pour l'auto-test et le journal : ce que montre la fenetre.
    pub(crate) fn fxc_state_line(&self) -> String {
        let p = &self.fxc;
        format!(
            "open={} track={} tracked={} links={} fx={} bank={} mix={} sg={} seq={} pad={:.2},{:.2} nav={} stats={:?}",
            p.win.is_some() as u8,
            if p.guid.is_empty() { "-" } else { &p.guid },
            p.t.tracked_count(),
            p.plan.links.len(),
            p.rows.len(),
            p.bank.is_some() as u8,
            p.chain.find(cp_fxc::slots::FxKind::Mix).is_some() as u8,
            p.sg.as_ref().is_some_and(|j| j.enabled) as u8,
            p.seq.is_some() as u8,
            p.t.pad.x,
            p.t.pad.y,
            p.t.pad.nav as u8,
            p.stats
        )
    }

    pub(crate) fn fxc_act_dock(&mut self, a: &Act) -> bool {
        let Some(h) = self.fxc.win.as_ref().map(|w| w.hwnd) else {
            return false;
        };
        match a {
            Act::DockGrab => self.dock_grab(h),
            Act::DockSwitch(i) => self.dock_switch(h, *i),
            Act::ViewMenu(x, y) => {
                let (sx, sy) = ui::client_to_screen(h, *x as i32, *y as i32);
                self.fxc_place_menu_at(h, sx, sy);
            }
            _ => return false,
        }
        true
    }
}
