//! La fenetre de CP Transport : posee a la place du transport de REAPER (la
//! place native que tient le chargeur), sans clavier, redessinee quand l'etat
//! de lecture change (a la cadence de l'ecran pendant la lecture).

use std::path::PathBuf;

use cp_abi::{PLACE_ARRANGE, PLACE_HELD, PLACE_TRANSPORT, PLACE_YIELDED, VIEW_PLACED};
use cp_gfx::{Canvas, Surface};
use cp_sys::transport::{fmt, play};
use cp_sys::ui::{self, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_transport::{Act, Glyph, Item, Status, ViewTab, fit_icon, fmt_bpm, initials};
use cp_ui::{Button, Ev, Theme};

use crate::items::{self, ItemCfg};
use crate::{App, Place, SECTION, hi16, lo16, mods, msg};

const TITLE: &str = "CP Transport";
const IDENT: &str = "cp_studio_transport";

/// Les actions de REAPER que pilote le transport (noms verifies par
/// l'auto-test dans le vrai REAPER).
pub(crate) mod cmd {
    pub const GO_START: i32 = 40042;
    pub const PLAY: i32 = 1007;
    pub const STOP: i32 = 1016;
    pub const PAUSE: i32 = 1008;
    pub const RECORD: i32 = 1013;
    pub const REPEAT: i32 = 1068;
    pub const METRONOME: i32 = 40364;
    pub const TAP: i32 = 1134;
    pub const PROJECT_SETTINGS: i32 = 40021;
    pub const SHOW_TRANSPORT: i32 = 40259;
}

const M_RUN: u32 = 1;
const M_LEFT: u32 = 2;
const M_RIGHT: u32 = 3;
const M_REMOVE: u32 = 4;
const M_ADD: u32 = 5;
const M_CLOSE: u32 = 6;
const M_PLACE_ARRANGE: u32 = 10;
const M_PLACE_DOCK: u32 = 11;
const M_PLACE_FLOAT: u32 = 12;
const M_VIEW_CLOSE: u32 = 13;

/// La fenetre du transport.
pub(crate) struct TView {
    pub hwnd: Hwnd,
    surf: Surface,
    present: Vec<u32>,
    theme: Theme,
    tracking: bool,
    pub frames_on: bool,
}

/// Le transport et ses icones.
pub(crate) struct TPart {
    pub t: cp_transport::Transport,
    pub view: Option<TView>,
    cfg: Vec<ItemCfg>,
    /// Pour chaque icone montree : l'index dans `cfg` et la commande.
    shown: Vec<(usize, i32)>,
    /// Une session de la liste des actions est ouverte (« Add action »).
    pub prompt: bool,
    icons_dir: PathBuf,
    scripts_dir: PathBuf,
    data_file: Option<PathBuf>,
    kb: std::collections::HashMap<String, String>,
    images: std::collections::HashMap<String, Option<Surface>>,
    /// Battements depuis la derniere relecture des icones.
    since_items: u32,
    /// L'infobulle, et la cible survolee depuis quand.
    tip: Option<ui::Tooltip>,
    hover: (cp_transport::Hit, std::time::Instant),
}

impl TPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> TPart {
        let res = r.resource_path().unwrap_or_default();
        let kb = std::fs::read(res.join("reaper-kb.ini"))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        TPart {
            t: cp_transport::Transport::new(),
            view: None,
            cfg: Vec::new(),
            shown: Vec::new(),
            prompt: false,
            icons_dir: res.join("Data").join("toolbar_icons"),
            scripts_dir: res.join("Scripts"),
            data_file: Some(res.join("CP_Studio").join("transport_items.txt")),
            kb: items::kb_scripts(&kb),
            images: Default::default(),
            since_items: 0,
            tip: None,
            hover: (cp_transport::Hit::None, std::time::Instant::now()),
        }
    }
}

impl App {
    // -----------------------------------------------------------------------
    // Ouvrir, fermer
    // -----------------------------------------------------------------------

    /// Pose CP Transport a la place de celui de REAPER.
    pub(crate) fn transport_start(&mut self) {
        if self.tp.view.is_some() {
            return;
        }
        // REAPER peut avoir retenu « transport ferme » en quittant pendant que
        // le notre le masquait : on le rouvre pour en prendre la place.
        if self.r.toggle_state(cmd::SHOW_TRANSPORT) == 0 {
            self.r.main_on_command(cmd::SHOW_TRANSPORT);
        }
        let Some(hwnd) = self
            .host
            .create_view(TITLE, IDENT, 0, 0, 800, 36, VIEW_PLACED)
        else {
            self.host
                .error("CP Transport : la fenetre n'a pas pu etre creee");
            return;
        };
        self.host.set_keyless(hwnd, true);
        if !self.host.place_take(PLACE_TRANSPORT, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.r.show_console_msg(
                "CP Transport: REAPER's transport must be docked at the top of the main window \
                 (right-click REAPER's transport > Dock transport > top of main window).\n",
            );
            self.r.ext_set(SECTION, "transport_on", "0", true);
            return;
        }
        self.transport_adopt(hwnd);
        self.host.place_take(PLACE_TRANSPORT, Some(hwnd), true);
        self.transport_render(true);
        self.r.ext_set(SECTION, "transport_on", "1", true);
    }

    pub(crate) fn transport_adopt(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        self.tp.view = Some(TView {
            hwnd,
            surf: Surface::default(),
            present: Vec::new(),
            theme: Theme::dark(scale),
            tracking: false,
            frames_on: false,
        });
        self.load_items();
        self.refresh_items();
        let st = self.read_status();
        self.tp.t.set_status(st);
        self.tp.t.views = self.view_tabs();
        self.tp.t.mark();
        self.transport_render(true);
    }

    /// `user` : Cedric l'a ferme (il ne reviendra pas au demarrage).
    pub(crate) fn transport_stop(&mut self, user: bool) {
        if let Some(v) = self.tp.view.take() {
            self.host.place_take(PLACE_TRANSPORT, None, false);
            self.host.destroy_window(v.hwnd);
        }
        if user {
            self.r.ext_set(SECTION, "transport_on", "0", true);
        }
    }

    // -----------------------------------------------------------------------
    // L'etat
    // -----------------------------------------------------------------------

    fn read_status(&self) -> Status {
        let r = &self.r;
        let ps = r.play_state();
        let pos = if ps & play::PLAYING != 0 || ps & play::RECORDING != 0 {
            r.play_position()
        } else {
            r.cursor_position()
        };
        let (n, d, bpm) = r.time_sig_at(pos);
        Status {
            play: ps,
            beats: r.format_time(pos, fmt::BEATS),
            time: r.format_time(pos, fmt::TIME),
            bpm,
            sig: (n, d),
            rate: r.play_rate(),
            repeat: r.repeat(),
            metronome: r.toggle_state(cmd::METRONOME) == 1,
        }
    }

    /// Les vues du selecteur : Arrange, puis chaque application.
    pub(crate) fn view_tabs(&self) -> Vec<ViewTab> {
        let (st, holder) = self.host.place_state(PLACE_ARRANGE);
        let bh = self.view.as_ref().map(|v| v.hwnd);
        let browser_in_band = st == PLACE_HELD && holder.is_some() && holder == bh;
        let browser_open_elsewhere = self
            .view
            .as_ref()
            .is_some_and(|v| v.place != Place::Arrange);
        vec![
            ViewTab {
                name: "Arrange".into(),
                active: st != PLACE_HELD,
                open: false,
            },
            ViewTab {
                name: "Browser".into(),
                active: browser_in_band,
                open: browser_open_elsewhere,
            },
        ]
    }

    fn load_items(&mut self) {
        let mut cfg = self
            .tp
            .data_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| items::parse(&s))
            .unwrap_or_default();
        let mut seeded: Vec<String> = self
            .r
            .ext_get(SECTION, "transport_seeded")
            .unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if items::seed_tools(&mut cfg, &mut seeded) {
            self.tp.cfg = cfg;
            self.save_items();
        } else {
            self.tp.cfg = cfg;
        }
        self.r
            .ext_set(SECTION, "transport_seeded", &seeded.join("|"), true);
    }

    fn save_items(&self) {
        if let Some(p) = &self.tp.data_file {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, items::serialize(&self.tp.cfg));
        }
    }

    /// La commande d'une icone, si l'action existe (et, pour un script, si son
    /// fichier existe encore).
    fn resolve(&self, id: &str) -> i32 {
        if let Ok(n) = id.parse::<i32>() {
            return if self.r.command_text(n).is_empty() {
                0
            } else {
                n
            };
        }
        let c = self.r.named_command(id);
        if c == 0 {
            return 0;
        }
        if let Some(path) = self.tp.kb.get(id.trim_start_matches('_')) {
            let p = std::path::Path::new(path);
            let full = if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.tp.scripts_dir.join(p)
            };
            if !full.exists() {
                return 0;
            }
        }
        c
    }

    fn image(&mut self, file: &str, size: i32) -> Option<Surface> {
        let key = format!("{file}@{size}");
        if let Some(v) = self.tp.images.get(&key) {
            return v.clone();
        }
        let img = cp_sys::image::load(&self.tp.icons_dir.join(file)).map(|s| fit_icon(&s, size));
        self.tp.images.insert(key, img.clone());
        img
    }

    /// Les icones montrees : celles dont l'action existe, avec leur etat.
    pub(crate) fn refresh_items(&mut self) {
        let scale = self.tp.view.as_ref().map_or(1.0, |v| v.theme.scale);
        let isz = (24.0 * scale).round() as i32;
        let mut shown = Vec::new();
        let mut out = Vec::new();
        let cfg = self.tp.cfg.clone();
        for (i, c) in cfg.iter().enumerate() {
            let cmd = self.resolve(&c.id);
            if cmd == 0 {
                continue;
            }
            let name = self.r.command_text(cmd);
            let glyph = if let Some(f) = c.icon.strip_prefix("png:") {
                match self.image(f, isz) {
                    Some(img) => Glyph::Image(img),
                    None => Glyph::Text(initials(&name)),
                }
            } else if let Some(n) = c.icon.strip_prefix("lucide:") {
                items::lucide(n).map_or_else(|| Glyph::Text(initials(&name)), Glyph::Icon)
            } else {
                Glyph::Text(initials(&name))
            };
            // Nos propres actions se lisent ici : demander leur etat a REAPER
            // repasserait par le module, occupe a ce battement (il rendrait
            // « pas d'etat »).
            let on = if c.id.starts_with("_CPSTUDIO_") {
                cp_sys::module::Module::toggle_state(self, cmd) == Some(true)
            } else {
                self.r.toggle_state(cmd) == 1
            };
            out.push(Item {
                id: c.id.clone(),
                name,
                glyph,
                on,
                open: false,
            });
            shown.push((i, cmd));
        }
        let changed = out.len() != self.tp.t.items.len()
            || out
                .iter()
                .zip(&self.tp.t.items)
                .any(|(a, b)| a.id != b.id || a.on != b.on);
        self.tp.shown = shown;
        self.tp.t.items = out;
        if changed {
            self.tp.t.mark();
        }
    }

    /// Le battement : l'etat de lecture, les icones, la liste des actions,
    /// et la place (Cedric a-t-il rallume le transport de REAPER ?).
    pub(crate) fn transport_tick(&mut self) {
        let Some(hwnd) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return;
        };
        let (st, _) = self.host.place_state(PLACE_TRANSPORT);
        if st == PLACE_YIELDED || !win::is_window(hwnd) {
            self.host
                .info("CP Transport : le transport de REAPER est revenu, on lui laisse la place");
            if let Some(v) = self.tp.view.take() {
                self.host.destroy_window(v.hwnd);
            }
            self.r.ext_set(SECTION, "transport_on", "0", true);
            return;
        }
        if self.tp.prompt {
            let c = self.r.prompt_for_action(0);
            if c != 0 {
                self.tp.prompt = false;
                self.r.prompt_for_action(-1);
                if c > 0 {
                    let id = self.r.command_name(c).unwrap_or_else(|| c.to_string());
                    if !self.tp.cfg.iter().any(|x| x.id == id) {
                        self.tp.cfg.push(ItemCfg {
                            id,
                            icon: String::new(),
                        });
                        self.save_items();
                    }
                    self.refresh_items();
                }
            }
        }
        self.tp.since_items += 1;
        if self.tp.since_items >= 10 {
            self.tp.since_items = 0;
            self.refresh_items();
        }
        self.transport_tip();
        let tabs = self.view_tabs();
        if tabs != self.tp.t.views {
            self.tp.t.views = tabs;
            self.tp.t.mark();
        }
        let st = self.read_status();
        let playing = st.play & (play::PLAYING | play::RECORDING) != 0;
        self.tp.t.set_status(st);
        if playing || self.tp.t.dragging() {
            self.transport_want_frames();
        } else if self.tp.t.take_dirty() {
            self.tp.t.mark();
            self.transport_render(false);
        }
    }

    /// L'infobulle : apres 0,6 s immobile sur une cible, sous elle.
    fn transport_tip(&mut self) {
        let Some(hwnd) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return;
        };
        let h = self.tp.t.hovered();
        if h != self.tp.hover.0 {
            self.tp.hover = (h, std::time::Instant::now());
            if let Some(t) = &mut self.tp.tip {
                t.hide();
            }
            return;
        }
        let shown = self.tp.tip.as_ref().is_some_and(|t| t.is_shown());
        if shown || self.tp.t.dragging() || self.tp.hover.1.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        let (Some(text), Some((x, y, _, bh))) = (self.tp.t.label(h), self.tp.t.rect_of(h)) else {
            return;
        };
        if self.tp.tip.is_none() {
            self.tp.tip = ui::Tooltip::new(hwnd);
        }
        let (sx, sy) = ui::client_to_screen(hwnd, x as i32, (y + bh) as i32 + 6);
        if let Some(t) = &mut self.tp.tip {
            t.show(&text, sx, sy);
        }
    }

    fn transport_want_frames(&mut self) {
        if let Some(v) = &mut self.tp.view {
            if !v.frames_on {
                v.frames_on = true;
                self.host.frames(v.hwnd, true);
            }
        }
    }

    pub(crate) fn transport_on_frame(&mut self) {
        let st = self.read_status();
        let playing = st.play & (play::PLAYING | play::RECORDING) != 0;
        self.tp.t.set_status(st);
        self.transport_render(false);
        if !playing && !self.tp.t.dragging() {
            if let Some(v) = &mut self.tp.view {
                if v.frames_on {
                    v.frames_on = false;
                    self.host.frames(v.hwnd, false);
                }
            }
        }
    }

    pub(crate) fn transport_render(&mut self, force: bool) {
        let Some(v) = self.tp.view.as_mut() else {
            return;
        };
        if !win::is_window(v.hwnd) {
            return;
        }
        let rc = ui::client_rect(v.hwnd);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return;
        }
        let scale = ui::dpi(v.hwnd) as f32 / 96.0;
        if (scale - v.theme.scale).abs() > 0.01 {
            v.theme = Theme::dark(scale);
        }
        let resized = v.surf.resize(w, h);
        let dirty = self.tp.t.take_dirty();
        if !(dirty || force || resized) {
            return;
        }
        {
            let mut cv = Canvas::new(&mut v.surf, &mut self.scratch);
            self.tp.t.draw(
                &mut cv,
                &mut self.text,
                &mut self.icons,
                &v.theme,
                w as f32,
                h as f32,
            );
        }
        ui::present(v.hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn transport_act(&mut self, a: Act) {
        let r = self.r;
        match a {
            Act::Start => r.main_on_command(cmd::GO_START),
            Act::Stop => r.main_on_command(cmd::STOP),
            Act::Play => r.main_on_command(cmd::PLAY),
            Act::Pause => r.main_on_command(cmd::PAUSE),
            Act::Record => r.main_on_command(cmd::RECORD),
            Act::Repeat => r.main_on_command(cmd::REPEAT),
            Act::Metronome => r.main_on_command(cmd::METRONOME),
            Act::Tap => r.main_on_command(cmd::TAP),
            Act::SetBpm { bpm, done } => r.set_bpm(bpm, done),
            Act::EditBpm => {
                let cur = fmt_bpm(self.tp.t.st.bpm);
                if let Some(v) = r.user_inputs("CP Transport", 1, "Tempo (BPM)", &cur) {
                    if let Ok(b) = v.trim().replace(',', ".").parse::<f64>() {
                        r.set_bpm(b, true);
                    }
                }
            }
            Act::EditSig => r.main_on_command(cmd::PROJECT_SETTINGS),
            Act::SetRate { rate, .. } => r.set_play_rate(rate),
            Act::Item(i) => {
                if let Some(&(_, c)) = self.tp.shown.get(i) {
                    r.main_on_command(c);
                }
            }
            Act::ItemMenu { i, x, y } => self.item_menu(i, x, y),
            Act::AddItem => self.add_item(),
            Act::BarMenu { x, y } => self.bar_menu(x, y),
            // Les vues changent au battement, jamais pendant un message.
            Act::View(i) => self.queue(if i == 0 {
                crate::WinOp::ViewArrange
            } else {
                crate::WinOp::ViewBrowser
            }),
            Act::ViewMenu { i, x, y } => {
                if i == 1 {
                    self.browser_place_menu(x, y);
                }
            }
        }
        let st = self.read_status();
        self.tp.t.set_status(st);
        self.tp.t.views = self.view_tabs();
        self.tp.t.mark();
        self.transport_render(false);
    }

    fn add_item(&mut self) {
        if !self.tp.prompt && self.r.prompt_for_action(1) >= 0 {
            self.tp.prompt = true;
        }
    }

    fn menu_at(&self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let Some(v) = &self.tp.view else { return 0 };
        let (sx, sy) = ui::client_to_screen(v.hwnd, x as i32, y as i32);
        ui::popup_menu(v.hwnd, sx, sy, items)
    }

    fn item_menu(&mut self, i: usize, x: f32, y: f32) {
        let Some(&(ci, _)) = self.tp.shown.get(i) else {
            return;
        };
        let name = self
            .tp
            .t
            .items
            .get(i)
            .map(|it| it.name.clone())
            .unwrap_or_default();
        let run = format!("Run: {name}");
        let n = self.tp.cfg.len();
        let items = [
            Some(MenuItem {
                id: M_RUN,
                text: &run,
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_LEFT,
                text: "Move left",
                checked: false,
                enabled: ci > 0,
            }),
            Some(MenuItem {
                id: M_RIGHT,
                text: "Move right",
                checked: false,
                enabled: ci + 1 < n,
            }),
            Some(MenuItem {
                id: M_REMOVE,
                text: "Remove from CP Transport",
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_ADD,
                text: "Add action...",
                checked: false,
                enabled: true,
            }),
        ];
        match self.menu_at(x, y, &items) {
            M_RUN => self.transport_act(Act::Item(i)),
            M_LEFT if ci > 0 => {
                self.tp.cfg.swap(ci, ci - 1);
                self.save_items();
                self.refresh_items();
            }
            M_RIGHT if ci + 1 < n => {
                self.tp.cfg.swap(ci, ci + 1);
                self.save_items();
                self.refresh_items();
            }
            M_REMOVE => {
                self.tp.cfg.remove(ci);
                self.save_items();
                self.refresh_items();
            }
            M_ADD => self.add_item(),
            _ => {}
        }
    }

    fn bar_menu(&mut self, x: f32, y: f32) {
        let items = [
            Some(MenuItem {
                id: M_ADD,
                text: "Add action...",
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_CLOSE,
                text: "Close CP Transport (show REAPER's transport)",
                checked: false,
                enabled: true,
            }),
        ];
        match self.menu_at(x, y, &items) {
            M_ADD => self.add_item(),
            M_CLOSE => self.queue(crate::WinOp::TransportOff),
            _ => {}
        }
    }

    fn browser_place_menu(&mut self, x: f32, y: f32) {
        let p = self.place;
        let open = self.view.is_some();
        let items = [
            Some(MenuItem {
                id: M_PLACE_ARRANGE,
                text: "Show in arrange view",
                checked: p == Place::Arrange,
                enabled: true,
            }),
            Some(MenuItem {
                id: M_PLACE_DOCK,
                text: "Dock in docker",
                checked: p == Place::Dock,
                enabled: true,
            }),
            Some(MenuItem {
                id: M_PLACE_FLOAT,
                text: "Floating window",
                checked: p == Place::Float,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_VIEW_CLOSE,
                text: "Close Browser",
                checked: false,
                enabled: open,
            }),
        ];
        match self.menu_at(x, y, &items) {
            M_PLACE_ARRANGE => self.queue(crate::WinOp::Place(Place::Arrange)),
            M_PLACE_DOCK => self.queue(crate::WinOp::Place(Place::Dock)),
            M_PLACE_FLOAT => self.queue(crate::WinOp::Place(Place::Float)),
            M_VIEW_CLOSE => self.queue(crate::WinOp::Close),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Les messages de la fenetre
    // -----------------------------------------------------------------------

    pub(crate) fn transport_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.tp.view.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        let ev = match m.msg {
            cp_abi::WM_CP_FRAME => {
                self.transport_on_frame();
                return Some(0);
            }
            // Jamais le clavier : tout va a REAPER.
            cp_abi::WM_CP_ACCEL => return Some(0),
            msg::SIZE => {
                self.tp.t.mark();
                self.transport_render(true);
                return Some(0);
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                if let Some(v) = &mut self.tp.view {
                    let (w, h) = (v.surf.w, v.surf.h);
                    if w > 0 && h > 0 {
                        ui::present(hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
                    } else {
                        self.transport_render(true);
                    }
                }
                return Some(0);
            }
            msg::ERASEBKGND => return Some(1),
            // Un clic n'active rien et ne prend pas le focus.
            msg::MOUSEACTIVATE => return Some(3), // MA_NOACTIVATE
            msg::MOUSEMOVE => {
                if let Some(v) = &mut self.tp.view {
                    if !v.tracking {
                        v.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                Ev::Move { x, y, mods: md }
            }
            msg::MOUSELEAVE => {
                if let Some(v) = &mut self.tp.view {
                    v.tracking = false;
                }
                Ev::Leave
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                let b = if matches!(m.msg, msg::LBUTTONDOWN | msg::LBUTTONDBLCLK) {
                    Button::Left
                } else {
                    Button::Right
                };
                if b == Button::Left {
                    ui::capture(hwnd);
                }
                Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                }
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                Ev::Up {
                    x,
                    y,
                    b: if m.msg == msg::LBUTTONUP {
                        Button::Left
                    } else {
                        Button::Right
                    },
                    mods: md,
                }
            }
            msg::MOUSEWHEEL => {
                let (cx, cy) = ui::screen_to_client(hwnd, x as i32, y as i32);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx: 0.0,
                    dy: d,
                    mods: md,
                }
            }
            msg::CLOSE => {
                self.queue(crate::WinOp::TransportOff);
                return Some(0);
            }
            _ => return None,
        };
        if matches!(ev, Ev::Down { .. } | Ev::Leave) {
            if let Some(t) = &mut self.tp.tip {
                t.hide();
            }
            // Le survol recommence a zero : pas de bulle juste apres un clic.
            self.tp.hover = (cp_transport::Hit::None, std::time::Instant::now());
        }
        let act = self.tp.t.event(&ev);
        if self.tp.t.dragging() {
            self.transport_want_frames();
        }
        match act {
            Some(a) => self.transport_act(a),
            None => self.transport_render(false),
        }
        Some(0)
    }
}
