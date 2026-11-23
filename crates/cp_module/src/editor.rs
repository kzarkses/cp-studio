//! L'editeur de CP Studio dans REAPER : un item MIDI (piano roll) ou un clip
//! audio, dans sa propre fenetre (a la place de l'arrangement, dans un
//! docker, ou flottante). Une app a part de Guitar Tab.
//!
//! - L'item ouvert suit la selection de REAPER (fenetre ouverte).
//! - Chaque geste s'ecrit dans l'item en UN pas d'annulation ; Ctrl+Z est
//!   celui de REAPER.
//! - Une note modifiee ailleurs (l'editeur de REAPER, Guitar Tab) est relue.
//! - Tenir une note la fait entendre par sa piste (un apercu de piste).

use std::time::Instant;

use cp_abi::{
    PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK, VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL, WM_CP_FRAME,
};
use cp_editview::cp_roll::expr::{self, Expr};
use cp_editview::cp_roll::{NAMES, Roll, SCALES, Scale};
use cp_editview::keys::{self, Bind, KeyMap};
use cp_editview::{Act, Audio, Clip, Colors, Ed, GRIDS, Hit, Midi, Rows};
use cp_sys::audio::Audition;
use cp_sys::items::{EdEv, EdNote, ItemInfo};
use cp_sys::module::key_event;
use cp_sys::transport::play;
use cp_sys::ui::{self, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, Theme};

use crate::gtab::{LOOK, selftest};
use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

const TITLE: &str = "Editor";
const IDENT: &str = "cp_studio_editor";

/// La fenetre.
pub(crate) struct EWin {
    pub hwnd: Hwnd,
    pub place: Place,
    frame: crate::frame::Frame,
    theme: Theme,
    theme_key: Vec<Option<(u8, u8, u8)>>,
    tracking: bool,
    closing: bool,
    tip: Option<ui::Tooltip>,
    hover: (Hit, Instant),
    tip_text: String,
}

pub(crate) struct EPart {
    pub ed: Ed,
    pub win: Option<EWin>,
    pub place: Place,
    /// L'empreinte des notes telles qu'on les a lues ou ecrites.
    hash: Option<String>,
    /// L'item tel qu'on l'a lu (un changement fait ailleurs se voit).
    info: Option<ItemInfo>,
    last_sel: Option<String>,
    since_sync: u32,
    /// Le compteur de changements du projet a la derniere relecture.
    state_seen: i32,
    aud: Option<Audition>,
    /// Une note entendue d'un coup (pas tenue) s'arrete a cette heure.
    aud_off: Option<Instant>,
    /// L'item a rouvrir (apres un rechargement du module).
    pub pending: Option<String>,
    /// Les reglages gardes, tels qu'enregistres.
    saved: String,
    /// Les raccourcis de l'editeur MIDI de REAPER (lus a l'ouverture).
    keys: Option<KeyMap>,
    /// REAPER a donne les raccourcis par defaut de son editeur MIDI.
    keys_defaults: bool,
    /// La touche dont on a pris l'appui (on prend aussi son relachement).
    key_down: Option<u32>,
    /// Les actions de REAPER qu'on ne reprend pas encore, deja signalees.
    missing: std::collections::HashSet<String>,
}

impl EPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> EPart {
        let mut ed = Ed::new();
        let get = |k: &str| r.ext_get(SECTION, k).unwrap_or_default();
        ed.colors = Colors::from_key(&get("ed_colors"));
        ed.rows_mode = if get("ed_rows") == "notes" {
            Rows::Notes
        } else {
            Rows::Fit
        };
        if let Ok(g) = get("ed_grid").parse::<f64>() {
            if g > 0.0 {
                ed.grid = g;
            }
        }
        ed.snap = get("ed_snap") != "0";
        ed.vel_lane = get("ed_vel") != "0";
        ed.hear = get("ed_hear") != "0";
        ed.isolate = get("ed_isolate") == "1";
        EPart {
            ed,
            win: None,
            place: Place::from_key(&get("ed_place")).unwrap_or(Place::Dock),
            hash: None,
            info: None,
            last_sel: None,
            since_sync: 1000,
            state_seen: i32::MIN,
            aud: None,
            aud_off: None,
            pending: None,
            saved: String::new(),
            keys: None,
            keys_defaults: false,
            key_down: None,
            missing: std::collections::HashSet::new(),
        }
    }
}

/// Les reglages de la vue, en une ligne (pour savoir s'ils ont change).
fn prefs(ed: &Ed) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        ed.colors.key(),
        if ed.rows_mode == Rows::Notes {
            "notes"
        } else {
            "fit"
        },
        ed.grid,
        ed.snap as u8,
        ed.vel_lane as u8,
        ed.hear as u8,
        ed.isolate as u8
    )
}

fn db_of(vol: f64) -> f32 {
    if vol <= 0.000001 {
        -150.0
    } else {
        (20.0 * vol.log10()) as f32
    }
}

/// La cle d'une case de la Session ouverte dans l'editeur (a la place du
/// GUID d'un item).
fn cell_key(guid: &str, scene: usize) -> String {
    format!("cell:{guid}:{scene}")
}

fn parse_cell(item: &str) -> Option<(String, usize)> {
    let rest = item.strip_prefix("cell:")?;
    let (g, s) = rest.rsplit_once(':')?;
    Some((g.to_string(), s.parse().ok()?))
}

fn vol_of(db: f32) -> f64 {
    if db <= -60.0 {
        0.0
    } else {
        10f64.powf(db as f64 / 20.0)
    }
}

impl App {
    fn ed_keep(&self, key: &str, value: &str) {
        if !selftest() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    // -----------------------------------------------------------------------
    // Ouvrir un item
    // -----------------------------------------------------------------------

    /// L'action et l'icone : ouvrir l'editeur sur l'item choisi dans REAPER.
    pub(crate) fn ed_start(&mut self) {
        if let Some(g) = self.r.selected_item_guid() {
            self.ed.pending = Some(g);
        }
        self.queue(WinOp::EdOpen);
    }

    /// Ouvre l'editeur sur un item (Guitar Tab : le double-clic sur le MIDI).
    pub(crate) fn ed_open_item(&mut self, guid: &str) {
        self.ed.pending = Some(guid.to_string());
        self.queue(WinOp::EdOpen);
    }

    /// Ouvre le clip MIDI d'une case de la Session (sans item : ses notes
    /// vivent dans le projet). Ce qui s'y ecrit revient a la case, et
    /// s'entend tout de suite si elle joue.
    pub(crate) fn ed_open_cell(&mut self, guid: &str, scene: usize, clip: &cp_session::Clip) {
        let r = self.r;
        let qpb = self.ses.ses.qpb();
        let len = clip.len_beats(qpb);
        let roll = Roll::from_notes(clip.notes().iter().map(|n| {
            (
                n.start,
                n.len.max(1e-3),
                n.pitch as i32,
                n.chan as i32,
                n.vel as i32,
                false,
                false,
            )
        }));
        let (_, sig, bpm) = r.measure_at(r.cursor_position());
        self.ed.ed.sig = sig;
        self.ed.ed.bpm = bpm;
        let expr = Expr::new(&roll.notes, &[]);
        let name = if clip.name.is_empty() {
            format!("Clip {}", scene + 1)
        } else {
            clip.name.clone()
        };
        self.ed.ed.open_midi(
            Midi {
                roll,
                name,
                item: cell_key(guid, scene),
                qn0: 0.0,
                qn1: len,
                expr,
            },
            true,
        );
        self.ed.info = None;
        self.ed.hash = None;
        self.ed.pending = None;
        self.ed.last_sel = r.selected_item_guid();
        self.ses.editing = Some((guid.to_string(), scene));
        self.ed.ed.mark();
        self.queue(WinOp::EdOpen);
    }

    /// Lit un item dans la vue. `fresh` : un autre item (la vue se recadre).
    fn ed_load(&mut self, guid: &str, fresh: bool) -> bool {
        let r = self.r;
        self.ses.editing = None;
        let Some(info) = r.item_info(guid) else {
            return false;
        };
        let (_, sig, bpm) = r.measure_at(info.pos);
        self.ed.ed.sig = sig;
        self.ed.ed.bpm = bpm;
        let qn0 = r.time_to_qn(info.pos);
        let qn1 = r.time_to_qn(info.pos + info.len);
        if info.midi {
            let Some(notes) = r.ed_notes(guid) else {
                return false;
            };
            let mut roll = Roll::from_notes(notes.iter().map(|n| {
                (
                    n.qn0,
                    (n.qn1 - n.qn0).max(1e-3),
                    n.pitch,
                    n.chan,
                    n.vel,
                    n.sel,
                    n.mute,
                )
            }));
            // La gamme et le presse-papiers restent d'une lecture a l'autre.
            if let Some(m) = self.ed.ed.midi() {
                if m.item == guid {
                    roll.scale = m.roll.scale;
                }
            }
            // Les pitch bends et les pedales legato : ils suivent les notes.
            let evs: Vec<expr::Ev> = r
                .ed_events(guid)
                .unwrap_or_default()
                .into_iter()
                .map(|e| expr::Ev {
                    qn: e.qn,
                    chan: e.chan,
                    status: e.status,
                    d1: e.d1,
                    d2: e.d2,
                    sel: e.sel,
                    mute: e.mute,
                })
                .collect();
            let expr = Expr::new(&roll.notes, &evs);
            self.ed.ed.open_midi(
                Midi {
                    roll,
                    name: info.name.clone(),
                    item: guid.to_string(),
                    qn0,
                    qn1,
                    expr,
                },
                fresh,
            );
            self.ed.hash = r.midi_hash(guid);
        } else {
            let fade = |s: f64, from: f64| r.time_to_qn(from + s) - r.time_to_qn(from);
            let same_span = self.ed.ed.audio().is_some_and(|a| {
                a.item == guid && (a.qn0 - qn0).abs() < 1e-9 && (a.qn1 - qn1).abs() < 1e-9
            }) && self.ed.info.as_ref().is_some_and(|i| {
                (i.offs - info.offs).abs() < 1e-9 && (i.rate - info.rate).abs() < 1e-9
            });
            let (peaks, pk) = match (same_span, self.ed.ed.audio()) {
                (true, Some(a)) => (a.peaks.clone(), a.pk),
                _ => (Vec::new(), (0.0, 0.0, 0)),
            };
            let reverse = self
                .ed
                .ed
                .audio()
                .filter(|a| a.item == guid)
                .is_some_and(|a| a.reverse);
            self.ed.ed.open_audio(
                Audio {
                    name: info.name.clone(),
                    item: guid.to_string(),
                    qn0,
                    qn1,
                    gain_db: db_of(info.vol),
                    fade_in: fade(info.fade_in, info.pos),
                    fade_out: fade(info.fade_out, info.pos + info.len - info.fade_out),
                    pitch: info.pitch,
                    rate: info.rate,
                    reverse,
                    peaks,
                    pk,
                },
                fresh,
            );
            self.ed.hash = None;
        }
        self.ed.info = Some(info);
        self.ed.ed.mark();
        true
    }

    /// Les notes de la vue, ecrites dans l'item (un pas d'annulation).
    fn ed_commit(&mut self, label: &str) {
        let r = self.r;
        let Some(m) = self.ed.ed.midi() else { return };
        if let Some((g, scene)) = parse_cell(&m.item) {
            let notes: Vec<cp_session::Note> = m
                .roll
                .notes
                .iter()
                .map(|n| cp_session::Note {
                    start: n.qn0.max(0.0),
                    len: (n.end() - n.qn0).max(1e-3),
                    pitch: n.pitch.clamp(0, 127) as u8,
                    vel: n.vel.clamp(1, 127) as u8,
                    chan: n.chan.clamp(0, 15) as u8,
                    prob: 100,
                })
                .collect();
            self.ses_cell_notes(&g, scene, notes);
            return;
        }
        let notes: Vec<EdNote> = m
            .roll
            .notes
            .iter()
            .map(|n| EdNote {
                qn0: n.qn0,
                qn1: n.end(),
                pitch: n.pitch,
                chan: n.chan,
                vel: n.vel,
                sel: n.sel,
                mute: n.mute,
            })
            .collect();
        let g = m.item.clone();
        // Les notes appartiennent a leur item (sa propre timeline) : s'il a
        // bouge dans REAPER depuis la lecture, elles bougent avec lui.
        let moved = r
            .item_info(&g)
            .map(|i| r.time_to_qn(i.pos) - m.qn0)
            .filter(|d| d.abs() > 1e-9);
        let notes: Vec<EdNote> = match moved {
            Some(d) => notes
                .into_iter()
                .map(|n| EdNote {
                    qn0: n.qn0 + d,
                    qn1: n.qn1 + d,
                    ..n
                })
                .collect(),
            None => notes,
        };
        // Les pitch bends et les pedales legato suivent leurs notes ; sans
        // eux a la lecture, on ne touche a aucun CC.
        let evs: Option<Vec<EdEv>> = (!m.expr.is_empty()).then(|| {
            let d = moved.unwrap_or(0.0);
            m.expr
                .carry(&m.roll.notes)
                .into_iter()
                .map(|e| EdEv {
                    qn: e.qn + d,
                    chan: e.chan,
                    status: e.status,
                    d1: e.d1,
                    d2: e.d2,
                    sel: e.sel,
                    mute: e.mute,
                })
                .collect()
        });
        if r.set_ed_notes(&g, &notes, evs.as_deref(), label) {
            if moved.is_some() {
                self.ed.pending = Some(g.clone());
            }
            self.ed.hash = r.midi_hash(&g);
        } else {
            self.ed.ed.status = "The item is gone".into();
        }
    }

    /// Le clip audio : ses valeurs posees dans l'item (pendant un geste).
    fn ed_audio_apply(&mut self) {
        let r = self.r;
        let (Some(a), Some(info)) = (self.ed.ed.audio().cloned(), self.ed.info.clone()) else {
            return;
        };
        let g = a.item.as_str();
        let pos = r.qn_to_time(a.qn0);
        let end = r.qn_to_time(a.qn1);
        if (pos - info.pos).abs() > 1e-9 || (end - (info.pos + info.len)).abs() > 1e-9 {
            // Rogner le debut decale la source d'autant : le son reste ou il
            // sonne.
            r.set_item_value(g, "D_POSITION", pos);
            r.set_item_value(g, "D_LENGTH", (end - pos).max(0.001));
            let offs = info.offs + (pos - info.pos) * info.rate;
            r.set_active_take_value(g, "D_STARTOFFS", offs);
        }
        r.set_item_value(g, "D_VOL", vol_of(a.gain_db));
        let fi = r.qn_to_time(a.qn0 + a.fade_in) - pos;
        let fo = end - r.qn_to_time(a.qn1 - a.fade_out);
        r.set_item_value(g, "D_FADEINLEN", fi.max(0.0));
        r.set_item_value(g, "D_FADEOUTLEN", fo.max(0.0));
        r.set_active_take_value(g, "D_PITCH", a.pitch);
        r.set_active_take_value(g, "D_PLAYRATE", a.rate);
        r.refresh_arrange();
    }

    fn ed_audition(&mut self, pitch: i32, chan: i32, vel: i32, hold: bool) {
        let r = self.r;
        let Some(track) = self
            .ed
            .info
            .as_ref()
            .map(|i| i.track.clone())
            .or_else(|| self.ses.editing.as_ref().map(|e| e.0.clone()))
        else {
            return;
        };
        if self.ed.aud.is_none() {
            self.ed.aud = Some(Audition::new(&r));
        }
        if let Some(a) = &mut self.ed.aud {
            a.start(&r, &track, pitch, chan, vel);
        }
        self.ed.aud_off = (!hold).then(|| Instant::now() + std::time::Duration::from_millis(300));
    }

    fn ed_audition_off(&mut self) {
        if let Some(a) = &mut self.ed.aud {
            a.stop();
        }
        self.ed.aud_off = None;
    }

    fn ed_act(&mut self, acts: Vec<Act>) {
        let r = self.r;
        for a in acts {
            match a {
                Act::Commit(label) => self.ed_commit(label),
                Act::Seek(q) => r.set_edit_cursor(r.qn_to_time(q)),
                Act::StepListen(on) => self.host.midi_listen(on),
                Act::DockGrab => {
                    if let Some(h) = self.ed.win.as_ref().map(|w| w.hwnd) {
                        self.dock_grab(h);
                    }
                }
                Act::Play => r.main_on_command(40044),
                Act::Undo | Act::Redo => {
                    r.main_on_command(if a == Act::Undo { 40029 } else { 40030 });
                    if let Some(g) = self.ed.ed.item().map(str::to_string) {
                        if parse_cell(&g).is_none() {
                            self.ed_load(&g, false);
                        }
                    }
                }
                Act::Audition {
                    pitch,
                    chan,
                    vel,
                    hold,
                } => self.ed_audition(pitch, chan, vel, hold),
                Act::AuditionOff => self.ed_audition_off(),
                Act::GridMenu { x, y } => self.ed_grid_menu(x, y),
                Act::ScaleMenu { x, y } => self.ed_scale_menu(x, y),
                Act::ViewMenu { x, y } => {
                    if let Some(h) = self.ed.win.as_ref().map(|w| w.hwnd) {
                        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
                        self.ed_place_menu_at(h, sx, sy);
                    }
                }
                Act::Main(c) => r.main_on_command(c as i32),
                Act::Close => self.queue(WinOp::EdClose),
                Act::OpenReaper => {
                    if let Some(g) = self.ed.ed.item().map(str::to_string) {
                        if r.select_only_item(&g) {
                            r.main_on_command(40153);
                        }
                    }
                }
                Act::AudioLive => self.ed_audio_apply(),
                Act::AudioCommit(label) => {
                    self.ed_audio_apply();
                    r.undo_point(label);
                    if let Some(g) = self.ed.ed.item().map(str::to_string) {
                        self.ed_load(&g, false);
                    }
                }
                Act::AudioReverse | Act::AudioNormalize => {
                    if let Some(g) = self.ed.ed.item().map(str::to_string) {
                        if r.select_only_item(&g) {
                            // Inverser la prise ; normaliser l'item.
                            r.main_on_command(if a == Act::AudioReverse { 41051 } else { 40108 });
                        }
                        if let Some(au) = self.ed.ed.audio_mut() {
                            au.pk = (0.0, 0.0, 0);
                        }
                        self.ed_load(&g, false);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Les menus
    // -----------------------------------------------------------------------

    fn ed_menu(&self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let Some(h) = self.ed.win.as_ref().map(|w| w.hwnd) else {
            return 0;
        };
        let (sx, sy) = ui::client_to_screen(h, x as i32, y as i32);
        ui::popup_menu(h, sx, sy, items)
    }

    fn ed_grid_menu(&mut self, x: f32, y: f32) {
        let g = self.ed.ed.grid;
        let items: Vec<Option<MenuItem<'_>>> = GRIDS
            .iter()
            .enumerate()
            .map(|(k, (name, v))| {
                Some(MenuItem {
                    id: k as u32 + 1,
                    text: name,
                    checked: (v - g).abs() < 1e-9,
                    enabled: true,
                })
            })
            .collect();
        let k = self.ed_menu(x, y, &items);
        if k > 0 {
            self.ed.ed.set_grid(GRIDS[(k - 1) as usize].1);
        }
    }

    fn ed_scale_menu(&mut self, x: f32, y: f32) {
        let cur = self.ed.ed.scale_of();
        let root = cur.map_or(0, |s| s.root);
        let mut items: Vec<Option<MenuItem<'_>>> = NAMES
            .iter()
            .enumerate()
            .map(|(k, n)| {
                Some(MenuItem {
                    id: 100 + k as u32,
                    text: n,
                    checked: cur.is_some() && root == k as i32,
                    enabled: true,
                })
            })
            .collect();
        items.push(None);
        items.push(Some(MenuItem {
            id: 1,
            text: "No scale",
            checked: cur.is_none(),
            enabled: true,
        }));
        for (k, (name, _)) in SCALES.iter().enumerate().skip(1) {
            items.push(Some(MenuItem {
                id: 200 + k as u32,
                text: name,
                checked: cur.is_some_and(|s| s.kind == k),
                enabled: true,
            }));
        }
        match self.ed_menu(x, y, &items) {
            1 => self.ed.ed.set_scale(None),
            k @ 100..=111 => {
                let kind = cur.map_or(2, |s| s.kind);
                self.ed.ed.set_scale(Some(Scale {
                    root: (k - 100) as i32,
                    kind,
                }));
            }
            k @ 201..=299 => self.ed.ed.set_scale(Some(Scale {
                root,
                kind: (k - 200) as usize,
            })),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Le battement
    // -----------------------------------------------------------------------

    pub(crate) fn ed_tick(&mut self) {
        if let Some(t) = self.ed.aud_off {
            if Instant::now() >= t {
                self.ed_audition_off();
            }
        }
        if self.ed.win.is_none() {
            return;
        }
        // La saisie pas a pas : les notes jouees sur les claviers MIDI.
        if self.ed.ed.step_input {
            let ev = self.host.midi_notes();
            if !ev.is_empty() {
                let acts = self.ed.ed.step_notes(&ev);
                self.ed_act(acts);
            }
        }
        let r = self.r;
        // L'item a ouvrir (action, Guitar Tab, rechargement).
        if let Some(g) = self.ed.pending.take() {
            let fresh = self.ed.ed.item() != Some(g.as_str());
            if !self.ed_load(&g, fresh) {
                self.ed.ed.close();
            }
            self.ed.last_sel = r.selected_item_guid();
        }
        let ps = r.play_state();
        let playing = ps & (play::PLAYING | play::RECORDING) != 0;
        self.ed.since_sync += 1;
        // La selection de REAPER, a chaque battement (un clic dans
        // l'arrangement s'ouvre ici tout de suite ; c'etait un tiers de
        // seconde).
        if !self.ed.ed.busy() {
            let sel = r.selected_item_guid();
            if sel != self.ed.last_sel {
                self.ed.last_sel = sel.clone();
                if let Some(g) = sel {
                    if self.ed.ed.item() != Some(g.as_str()) && r.item_info(&g).is_some() {
                        self.ed_load(&g, true);
                    }
                }
            }
        }
        // L'item ouvert, modifie ailleurs (deplace, rogne, ses notes, efface) :
        // des que le compteur du projet bouge.
        let cell = self.ed.ed.item().is_some_and(|g| parse_cell(g).is_some());
        let sc = r.state_count();
        if sc != self.ed.state_seen && !self.ed.ed.busy() && !cell {
            self.ed.state_seen = sc;
            if let Some(g) = self.ed.ed.item().map(str::to_string) {
                match r.item_info(&g) {
                    None => self.ed.ed.close(),
                    Some(info) => {
                        // Une note bouge avec son item : la place de l'item
                        // compte autant que ses notes.
                        let changed = match &self.ed.ed.clip {
                            Clip::Midi(_) => {
                                r.midi_hash(&g) != self.ed.hash
                                    || self.ed.info.as_ref() != Some(&info)
                            }
                            _ => self.ed.info.as_ref() != Some(&info),
                        };
                        if changed {
                            self.ed_load(&g, false);
                        }
                    }
                }
            }
        }
        if self.ed.since_sync >= 10 && !self.ed.ed.busy() {
            self.ed.since_sync = 0;
            // L'item ouvert, modifie ailleurs (ou efface) : le filet.
            if let Some(g) = self.ed.ed.item().filter(|_| !cell).map(str::to_string) {
                match r.item_info(&g) {
                    None => self.ed.ed.close(),
                    Some(info) => {
                        let changed = match &self.ed.ed.clip {
                            Clip::Midi(_) => r.midi_hash(&g) != self.ed.hash,
                            _ => self.ed.info.as_ref() != Some(&info),
                        };
                        if changed {
                            self.ed_load(&g, false);
                        }
                    }
                }
            }
            // Le theme de REAPER.
            let key: Vec<_> = LOOK.iter().map(|k| r.theme_color(k)).collect();
            if let Some(w) = &mut self.ed.win {
                if w.theme_key != key {
                    w.theme_key = key;
                    let sc = w.theme.scale;
                    let th = self.reaper_theme(sc);
                    if let Some(w) = &mut self.ed.win {
                        w.theme = th;
                    }
                    self.ed.ed.mark();
                }
            }
            // Les reglages, gardes quand ils changent.
            let p = prefs(&self.ed.ed);
            if p != self.ed.saved {
                self.ed.saved = p;
                let e = &self.ed.ed;
                let (c, rw, g, s, v) = (
                    e.colors.key().to_string(),
                    if e.rows_mode == Rows::Notes {
                        "notes"
                    } else {
                        "fit"
                    },
                    e.grid.to_string(),
                    if e.snap { "1" } else { "0" },
                    if e.vel_lane { "1" } else { "0" },
                );
                self.ed_keep("ed_colors", &c);
                self.ed_keep("ed_rows", rw);
                self.ed_keep("ed_grid", &g);
                self.ed_keep("ed_snap", s);
                self.ed_keep("ed_vel", v);
                let h = if self.ed.ed.hear { "1" } else { "0" };
                self.ed_keep("ed_hear", h);
                let i = if self.ed.ed.isolate { "1" } else { "0" };
                self.ed_keep("ed_isolate", i);
            }
        }
        // Un geste qui sort de la vue : elle defile au bord, le geste suit.
        if self.ed.ed.dragging() {
            let acts = self.ed.ed.tick();
            if !acts.is_empty() {
                self.ed_act(acts);
            }
        }
        // La forme d'onde du clip, a la resolution de la vue.
        if let (Some((q0, q1, cols)), Some(g)) = (
            self.ed.ed.peaks_wanted(),
            self.ed.ed.item().map(str::to_string),
        ) {
            let (t0, t1) = (r.qn_to_time(q0), r.qn_to_time(q1));
            let pk = r
                .take_peaks(&g, t0, t1, cols)
                .unwrap_or_else(|| vec![(0.0, 0.0); cols]);
            self.ed.ed.set_peaks(q0, q1, pk);
        }
        // Le curseur d'edition (montre aussi pendant la lecture) et la tete.
        // Une case : la tete est celle de sa lane.
        if let Some((g, scene)) = self.ses.editing.clone().filter(|_| cell) {
            let pos = self.ses_cell_pos(&g, scene);
            self.ed.ed.edit_cursor = None;
            self.ed.ed.set_playhead(pos, pos.is_some());
            if pos.is_some() {
                self.ed_want_frames(true);
            }
        } else {
            let cur = r.time_to_qn(r.cursor_position());
            self.ed.ed.edit_cursor = Some(cur);
            if !playing {
                self.ed.ed.set_playhead(Some(cur), false);
            }
        }
        self.ed_tip();
        let shown = self.ed.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd));
        if !shown {
            return;
        }
        if playing {
            self.ed_want_frames(true);
        } else if !self.ed.win.as_ref().is_some_and(|w| w.frame.frames_on) {
            // Horloge arretee : ce qui a change part d'ici ; rien de neuf ne
            // coute rien.
            self.ed_render(false);
        }
    }

    /// L'infobulle : apres 0,6 s immobile sur une cible, sous elle.
    fn ed_tip(&mut self) {
        let h = self.ed.ed.hovered();
        let dragging = self.ed.ed.dragging();
        let Some(w) = self.ed.win.as_mut() else {
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
        let Some(text) = self.ed.ed.label(h) else {
            return;
        };
        // Sous la cible de la barre ; ailleurs, sous la souris.
        let (x, y) = match self.ed.ed.rect_of(h) {
            Some((x, y, bw, bh)) if h == Hit::Help => {
                ((x + bw - 460.0 * w.theme.scale).max(0.0), y + bh)
            }
            Some((x, y, _, bh)) => (x, y + bh),
            None => {
                let (sx, sy) = ui::cursor_pos();
                let (cx, cy) = ui::screen_to_client(w.hwnd, sx, sy);
                (cx as f32 + 12.0, cy as f32 + 18.0)
            }
        };
        if w.tip.is_none() {
            w.tip = ui::Tooltip::new(w.hwnd);
        }
        let (sx, sy) = ui::client_to_screen(w.hwnd, x as i32, y as i32 + 6);
        if let Some(t) = &mut w.tip {
            t.show(&text.replace('\n', "\r\n"), sx, sy);
        }
        w.tip_text = text;
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    pub(crate) fn ed_in_band(&self) -> bool {
        let Some(w) = &self.ed.win else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(w.hwnd)
    }

    pub(crate) fn ed_open(&mut self) {
        if let Some(w) = &self.ed.win {
            match w.place {
                Place::Dock => self.r.dock_activate(w.hwnd),
                Place::Float => win::show(w.hwnd),
                Place::Arrange => {
                    let h = w.hwnd;
                    if !self.ed_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true) {
                        self.ed_render(true);
                    }
                }
            }
            return;
        }
        let place = self.ed.place;
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 1200, 520, mode) else {
            self.host
                .error("la fenetre de l'editeur n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange && !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.ed.place = Place::Dock;
            self.ed_open();
            return;
        }
        self.ed_adopt(hwnd);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.ed_render(true);
        }
        self.ed_keep("ed_open", "1");
    }

    /// Reprend la fenetre (a sa creation, ou apres un rechargement).
    pub(crate) fn ed_adopt(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.ed.win = Some(EWin {
            hwnd,
            place: self.ed.place,
            frame: crate::frame::Frame::default(),
            theme: self.reaper_theme(scale),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            closing: false,
            tip: None,
            hover: (Hit::None, Instant::now()),
            tip_text: String::new(),
        });
        self.ed.since_sync = 1000;
        self.ed_keymap();
        if self.ed.pending.is_none() && self.ed.ed.item().is_none() {
            self.ed.pending = r.selected_item_guid();
        }
        self.ed.ed.mark();
        self.ed_render(true);
    }

    /// Avant un rechargement : la fenetre reste au chargeur, l'item est retenu.
    pub(crate) fn ed_shutdown(&mut self) {
        self.ed_audition_off();
        self.ed.aud = None;
        let h = self.ed.win.as_ref().map_or(0, |w| w.hwnd.0);
        if let Some(w) = &self.ed.win {
            self.host.frames(w.hwnd, false);
        }
        self.r.ext_set(SECTION, "ed_hwnd", &h.to_string(), false);
        let g = self.ed.ed.item().unwrap_or("").to_string();
        self.r.ext_set(SECTION, "ed_item", &g, false);
    }

    pub(crate) fn ed_close(&mut self, user: bool) {
        self.ed_audition_off();
        if self.ed.ed.step_input {
            self.ed.ed.step_input = false;
            self.host.midi_listen(false);
        }
        let in_band = self.ed_in_band();
        if let Some(w) = &mut self.ed.win {
            w.closing = true;
            let h = w.hwnd;
            if in_band {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.ed.win = None;
        if user {
            self.ed_keep("ed_open", "0");
        }
    }

    pub(crate) fn ed_set_place(&mut self, p: Place) {
        let reopen = self.ed.win.as_ref().is_some_and(|w| w.place != p);
        self.ed.place = p;
        self.ed_keep("ed_place", p.key());
        if reopen {
            let keep = self.ed.ed.item().map(str::to_string);
            self.ed_close(false);
            self.ed.pending = keep;
        }
        self.ed_open();
    }

    pub(crate) fn ed_place_menu_at(&mut self, owner: Hwnd, sx: i32, sy: i32) {
        let p = self.ed.place;
        let open = self.ed.win.is_some();
        let items = [
            Some(MenuItem {
                id: 1,
                text: "Show in arrange view",
                checked: p == Place::Arrange,
                enabled: true,
            }),
            Some(MenuItem {
                id: 2,
                text: "Dock in docker",
                checked: p == Place::Dock,
                enabled: true,
            }),
            Some(MenuItem {
                id: 3,
                text: "Floating window",
                checked: p == Place::Float,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: 20,
                text: "Docker handle: auto",
                checked: self.handle_mode() == 0,
                enabled: true,
            }),
            Some(MenuItem {
                id: 21,
                text: "Docker handle: always",
                checked: self.handle_mode() == 1,
                enabled: true,
            }),
            Some(MenuItem {
                id: 22,
                text: "Docker handle: never",
                checked: self.handle_mode() == 2,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: 4,
                text: "Close the editor",
                checked: false,
                enabled: open,
            }),
        ];
        match ui::popup_menu(owner, sx, sy, &items) {
            k @ 20..=22 => self.set_handle_mode((k - 20) as u8),
            1 => self.queue(WinOp::EdPlace(Place::Arrange)),
            2 => self.queue(WinOp::EdPlace(Place::Dock)),
            3 => self.queue(WinOp::EdPlace(Place::Float)),
            4 => self.queue(WinOp::EdClose),
            _ => {}
        }
    }

    fn ed_want_frames(&mut self, on: bool) {
        if let Some(w) = &mut self.ed.win {
            if on {
                w.frame.wake();
            }
            if w.frame.frames_on != on {
                w.frame.frames_on = on;
                self.host.frames(w.hwnd, on);
            }
        }
    }

    /// Un geste vient de changer la vue : au repos, l'image part tout de
    /// suite et l'horloge demarre ; sinon la prochaine image la prend.
    fn ed_kick(&mut self) {
        let on = self.ed.win.as_ref().is_some_and(|w| w.frame.frames_on);
        if !on {
            self.ed_render(false);
        }
        self.ed_want_frames(true);
    }

    pub(crate) fn ed_render(&mut self, force: bool) {
        let Some(w) = self.ed.win.as_mut() else {
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
            self.ed.ed.mark();
            let Some(w) = self.ed.win.as_mut() else {
                return;
            };
            w.theme = th;
        }
        let Some(w) = self.ed.win.as_mut() else {
            return;
        };
        if w.frame.resize(cw, ch) || force {
            self.ed.ed.mark();
        }
        let damage = self.ed.ed.take_damage(cw, ch);
        let ed = &mut self.ed.ed;
        let (text, icons, th) = (&mut self.text, &mut self.icons, &w.theme);
        w.frame.paint(&damage, &mut self.scratch, |cv| {
            ed.draw(cv, text, icons, th, cw as f32, ch as f32);
        });
        let strokes = self.ed.ed.strokes(&w.theme);
        w.frame.show(w.hwnd, &damage, &strokes);
    }

    /// Les raccourcis de l'editeur MIDI de REAPER : les defauts de REAPER et
    /// reaper-kb.ini, lus une fois. Une copie est ecrite dans
    /// %TEMP%/cp_editor_keys.txt (pour verifier ce qui est repris).
    fn ed_keymap(&mut self) {
        if self.ed.keys.is_some() {
            return;
        }
        let r = self.r;
        let defs = r.section_default_keys(keys::SECTION as i32);
        let ini = r
            .resource_path()
            .and_then(|p| std::fs::read(p.join("reaper-kb.ini")).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let km = KeyMap::build(&defs, &ini);
        let mut lines: Vec<String> = km
            .iter()
            .map(|(&(f, k), b)| match b {
                Bind::Cmd(c) => format!(
                    "{f:3} {k:6} {c:6} {}",
                    r.section_command_text(keys::SECTION as i32, *c as i32)
                ),
                Bind::Named(n) => format!("{f:3} {k:6}        {n}"),
            })
            .collect();
        lines.sort();
        let _ = std::fs::write(
            std::env::temp_dir().join("cp_editor_keys.txt"),
            lines.join("\r\n"),
        );
        self.host.info(&format!(
            "Editeur : {} raccourcis de l'editeur MIDI de REAPER ({} par defaut)",
            km.len(),
            defs.len()
        ));
        self.ed.keys_defaults = !defs.is_empty();
        self.ed.keys = Some(km);
    }

    /// Une touche : l'action que REAPER y lie dans son editeur MIDI. Rend
    /// vrai si l'editeur la prend ; une touche libre va a REAPER (comme son
    /// editeur MIDI la passe a la fenetre principale).
    fn ed_key(&mut self, vk: u32, ext: bool, md: cp_ui::Mods) -> bool {
        self.ed_keymap();
        // D'abord les editions au survol de la fenetre principale de Cedric
        // (Q, D, S, Z : sur la note sous la souris ; A, E : les fondus d'un
        // clip) : elles passent avant les touches de l'editeur MIDI de REAPER
        // (Q y quantifiait). La souris ailleurs : la touche suit son cours.
        let hwnd = self.ed.win.as_ref().map(|w| w.hwnd);
        if let Some(h) = hwnd {
            if Self::mouse_over(h).is_some() {
                if let Some(e) = self.main_hover(vk, ext, md) {
                    let acts = self.ed.ed.hover_edit(e);
                    self.ed_act(acts);
                    self.ed_kick();
                    return true;
                }
            }
        }
        let km = self.ed.keys.as_ref().filter(|k| !k.is_empty());
        let Some(km) = km else {
            // Pas de table (REAPER ne l'a pas donnee) : les touches de base.
            if !self.ed.ed.wants_key(vk, md) {
                return false;
            }
            self.ed_feed(Ev::Key {
                vk,
                mods: md,
                text: None,
            });
            return true;
        };
        let Some(bind) = km.get(vk, ext, md).cloned() else {
            // Sans les defauts de REAPER (seulement reaper-kb.ini), une touche
            // libre irait a la fenetre principale (Suppr y efface des items) :
            // les touches de base restent a l'editeur.
            if !self.ed.keys_defaults && self.ed.ed.wants_key(vk, md) {
                self.ed_feed(Ev::Key {
                    vk,
                    mods: md,
                    text: None,
                });
                return true;
            }
            return false;
        };
        let acts = match &bind {
            Bind::Cmd(c) => self.ed.ed.command(*c),
            Bind::Named(n) => keys::named(n).map(|x| self.ed.ed.named_cmd(x)),
        };
        match acts {
            Some(a) => {
                self.ed_act(a);
                self.ed_render(false);
                true
            }
            // « Close window if not docked, otherwise pass to main window ».
            None if bind == Bind::Cmd(40477) => false,
            None => {
                let what = match &bind {
                    Bind::Cmd(c) => format!(
                        "{c} {}",
                        self.r.section_command_text(keys::SECTION as i32, *c as i32)
                    ),
                    Bind::Named(n) => n.clone(),
                };
                if self.ed.missing.insert(what.clone()) {
                    self.host.info(&format!(
                        "Editeur : action REAPER pas encore reprise : {what}"
                    ));
                }
                // Liee dans l'editeur MIDI : REAPER ne la passerait pas a sa
                // fenetre principale, nous non plus.
                true
            }
        }
    }

    fn ed_feed(&mut self, ev: Ev) {
        let acts = self.ed.ed.event(&ev);
        self.ed_act(acts);
        self.ed_kick();
    }

    /// Les messages de la fenetre.
    pub(crate) fn ed_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.ed.win.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        match m.msg {
            WM_CP_FRAME => {
                let r = self.r;
                let ps = r.play_state();
                let mut playing = ps & (play::PLAYING | play::RECORDING) != 0;
                let cell = self
                    .ed
                    .ed
                    .item()
                    .and_then(parse_cell)
                    .filter(|c| self.ses.editing.as_ref() == Some(c));
                if let Some((g, scene)) = cell {
                    let pos = self.ses_cell_pos(&g, scene);
                    playing = pos.is_some();
                    self.ed.ed.set_playhead(pos, playing);
                } else {
                    let pos = if playing {
                        r.play_position()
                    } else {
                        r.cursor_position()
                    };
                    self.ed.ed.set_playhead(Some(r.time_to_qn(pos)), playing);
                    self.ed.ed.edit_cursor = Some(r.time_to_qn(r.cursor_position()));
                }
                self.ed_render(false);
                let idle = self.ed.win.as_ref().is_none_or(|w| w.frame.idle());
                if !playing && idle && !self.ed.ed.dragging() {
                    self.ed_want_frames(false);
                }
                Some(0)
            }
            WM_CP_ACCEL => {
                let Some(k) = key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                let ext = (k.lp >> 24) & 1 == 1;
                Some(match k.message {
                    msg::KEYDOWN | msg::SYSKEYDOWN => {
                        let took = self.ed_key(vk, ext, md);
                        self.ed.key_down = took.then_some(vk);
                        took as isize
                    }
                    msg::KEYUP | msg::SYSKEYUP if self.ed.key_down == Some(vk) => {
                        self.ed.key_down = None;
                        1
                    }
                    _ => 0,
                })
            }
            msg::SIZE => {
                self.ed.ed.mark();
                self.ed_render(true);
                Some(0)
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                let shown = self.ed.win.as_mut().is_some_and(|w| w.frame.repaint(hwnd));
                if !shown {
                    self.ed_render(true);
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001),
            // Le curseur de REAPER pour ce que ferait un geste ici.
            msg::SETCURSOR if (m.lp & 0xFFFF) == 1 => {
                let (cx, cy) = Self::mouse_over(hwnd).unwrap_or((x, y));
                ui::set_pointer(self.ed.ed.pointer(cx, cy, md));
                Some(1)
            }
            msg::MOUSEMOVE => {
                if let Some(w) = &mut self.ed.win {
                    if !w.tracking {
                        w.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.ed_feed(Ev::Move { x, y, mods: md });
                // Pendant un geste (souris capturee) : le curseur du geste.
                if ui::has_capture(hwnd) {
                    ui::set_pointer(self.ed.ed.pointer(x, y, md));
                }
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(w) = &mut self.ed.win {
                    w.tracking = false;
                }
                self.ed_feed(Ev::Leave);
                Some(0)
            }
            msg::LBUTTONDOWN
            | msg::LBUTTONDBLCLK
            | msg::RBUTTONDOWN
            | msg::RBUTTONDBLCLK
            | msg::MBUTTONDOWN
            | msg::MBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                ui::capture(hwnd);
                let b = match m.msg {
                    msg::LBUTTONDOWN | msg::LBUTTONDBLCLK => Button::Left,
                    msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => Button::Right,
                    _ => Button::Middle,
                };
                self.ed_feed(Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                });
                Some(0)
            }
            msg::LBUTTONUP | msg::RBUTTONUP | msg::MBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                let b = match m.msg {
                    msg::LBUTTONUP => Button::Left,
                    msg::RBUTTONUP => Button::Right,
                    _ => Button::Middle,
                };
                self.ed_feed(Ev::Up { x, y, b, mods: md });
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
                self.ed_feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
                Some(0)
            }
            msg::CLOSE => {
                self.queue(WinOp::EdClose);
                Some(0)
            }
            msg::DESTROY => {
                let ours = self.ed.win.as_ref().is_some_and(|w| w.closing);
                if !ours {
                    self.ed.win = None;
                }
                None
            }
            _ => None,
        }
    }

    /// L'auto-test : ouvrir l'item MIDI du riff, transposer d'un demi-ton,
    /// verifier dans REAPER, annuler. Rend la ligne du rapport.
    pub(crate) fn ed_selftest(&mut self, item: &str) -> String {
        let r = self.r;
        if !self.ed_load(item, true) {
            return "item illisible".into();
        }
        let before: Vec<i32> = r
            .ed_notes(item)
            .unwrap_or_default()
            .iter()
            .map(|n| n.pitch)
            .collect();
        let chans: Vec<i32> = r
            .ed_notes(item)
            .unwrap_or_default()
            .iter()
            .map(|n| n.chan)
            .collect();
        if let Some(m) = self.ed.ed.midi_mut() {
            m.roll.select_all();
        }
        let acts = self.ed.ed.event(&Ev::Key {
            vk: cp_ui::vk::UP,
            mods: cp_ui::Mods::default(),
            text: None,
        });
        self.ed_act(
            acts.into_iter()
                .filter(|a| matches!(a, Act::Commit(_)))
                .collect(),
        );
        let after: Vec<i32> = r
            .ed_notes(item)
            .unwrap_or_default()
            .iter()
            .map(|n| n.pitch)
            .collect();
        let chans_after: Vec<i32> = r
            .ed_notes(item)
            .unwrap_or_default()
            .iter()
            .map(|n| n.chan)
            .collect();
        r.main_on_command(40029);
        let undone: Vec<i32> = r
            .ed_notes(item)
            .unwrap_or_default()
            .iter()
            .map(|n| n.pitch)
            .collect();
        let up = !before.is_empty()
            && before.len() == after.len()
            && before.iter().zip(&after).all(|(a, b)| b - a == 1);
        let bend = self.ed_selftest_bend(item);
        format!(
            "notes={} transpose={} canaux={} annule={} bend={}",
            before.len(),
            up as u8,
            (chans == chans_after) as u8,
            (undone == before) as u8,
            bend
        )
    }

    /// L'auto-test d'un bend : un ton pose par la palette sur la premiere
    /// note, relu dans REAPER (ses pitch bends rattaches a la note), annule.
    /// Rend le sommet relu, en demi-tons (2 attendu), et « annule » ou non.
    fn ed_selftest_bend(&mut self, item: &str) -> String {
        let r = self.r;
        if !self.ed_load(item, true) {
            return "illisible".into();
        }
        let Some(first) = self
            .ed
            .ed
            .midi()
            .and_then(|m| m.roll.notes.first().copied())
        else {
            return "sans note".into();
        };
        if let Some(m) = self.ed.ed.midi_mut() {
            m.roll.select_only(first.id);
        }
        let acts = self.ed.ed.transform(101);
        self.ed_act(
            acts.into_iter()
                .filter(|a| matches!(a, Act::Commit(_)))
                .collect(),
        );
        let top = |r: cp_sys::reaper::Reaper| -> f32 {
            let notes = r.ed_notes(item).unwrap_or_default();
            let roll = Roll::from_notes(notes.iter().map(|n| {
                (
                    n.qn0,
                    (n.qn1 - n.qn0).max(1e-3),
                    n.pitch,
                    n.chan,
                    n.vel,
                    n.sel,
                    n.mute,
                )
            }));
            let evs: Vec<expr::Ev> = r
                .ed_events(item)
                .unwrap_or_default()
                .into_iter()
                .map(|e| expr::Ev {
                    qn: e.qn,
                    chan: e.chan,
                    status: e.status,
                    d1: e.d1,
                    d2: e.d2,
                    sel: e.sel,
                    mute: e.mute,
                })
                .collect();
            let x = Expr::new(&roll.notes, &evs);
            roll.notes
                .iter()
                .find(|n| (n.qn0 - first.qn0).abs() < 1e-6 && n.pitch == first.pitch)
                .map_or(0.0, |n| {
                    x.curve(n).iter().map(|p| p.1).fold(0.0f32, f32::max)
                })
        };
        let written = top(r);
        r.main_on_command(40029);
        let after_undo = top(r);
        format!(
            "{written:.2}{}",
            if after_undo.abs() < 0.01 {
                ",annule"
            } else {
                ""
            }
        )
    }
}
