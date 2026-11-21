//! `cp_module.dll` : le code de CP Studio, recharge a chaud par le chargeur.
//!
//! Il porte le navigateur de fichiers (jalon 1) dans une vraie fenetre de
//! REAPER, flottante ou ancree dans un docker :
//! - il traduit les messages de Windows en evenements du toolkit ;
//! - il dessine a la cadence de l'ecran (WM_CP_FRAME) quand quelque chose a
//!   change, et ne fait rien sinon ;
//! - il repond au crochet clavier (WM_CP_ACCEL) : ce que le navigateur prend,
//!   ce qu'il laisse a REAPER, ce qui est du texte ;
//! - il garde, d'une generation a l'autre, la fenetre et l'etat du navigateur.

use std::path::PathBuf;

use cp_abi::{WM_CP_ACCEL, WM_CP_FRAME};
use cp_browser::draw::Gfx;
use cp_browser::model::{self, Shortcut};
use cp_browser::wave::ClipInfo;
use cp_browser::{Browser, Focus, Host as BrowserHost, KeyUse, Loaded, MenuEntry};
use cp_gfx::text::{Align, TextCache};
use cp_gfx::{Canvas, Scratch, Surface};
use cp_sys::font::GdiGlyphs;
use cp_sys::module::{AudioLoad, HostRef, Module, key_event};
use cp_sys::reaper::Reaper;
use cp_sys::shell::DragResult;
use cp_sys::ui::{self, Cursor, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, IconCache, Mods, Theme};

const SECTION: &str = "CP_Studio";
const DOCK_IDENT: &str = "cp_studio_browser";
const TITLE: &str = "CP Browser";

const WIN_DOCK: u32 = 1001;
const WIN_CLOSE: u32 = 1002;
const WIN_PERF: u32 = 1003;

mod msg {
    pub const DESTROY: u32 = 0x0002;
    pub const SIZE: u32 = 0x0005;
    pub const PAINT: u32 = 0x000F;
    pub const CLOSE: u32 = 0x0010;
    pub const ERASEBKGND: u32 = 0x0014;
    pub const SETCURSOR: u32 = 0x0020;
    pub const GETDLGCODE: u32 = 0x0087;
    pub const KEYDOWN: u32 = 0x0100;
    pub const KEYUP: u32 = 0x0101;
    pub const CHAR: u32 = 0x0102;
    pub const SYSKEYDOWN: u32 = 0x0104;
    pub const SYSKEYUP: u32 = 0x0105;
    pub const SYSCHAR: u32 = 0x0106;
    pub const MOUSEMOVE: u32 = 0x0200;
    pub const LBUTTONDOWN: u32 = 0x0201;
    pub const LBUTTONUP: u32 = 0x0202;
    pub const LBUTTONDBLCLK: u32 = 0x0203;
    pub const RBUTTONDOWN: u32 = 0x0204;
    pub const RBUTTONUP: u32 = 0x0205;
    pub const RBUTTONDBLCLK: u32 = 0x0206;
    pub const MBUTTONDOWN: u32 = 0x0207;
    pub const MBUTTONUP: u32 = 0x0208;
    pub const MBUTTONDBLCLK: u32 = 0x0209;
    pub const MOUSEWHEEL: u32 = 0x020A;
    pub const MOUSEHWHEEL: u32 = 0x020E;
    pub const MOUSELEAVE: u32 = 0x02A3;
}

fn lo16(v: isize) -> f32 {
    (v & 0xFFFF) as u16 as i16 as f32
}
fn hi16(v: isize) -> f32 {
    ((v >> 16) & 0xFFFF) as u16 as i16 as f32
}

fn mods() -> Mods {
    let m = ui::mods();
    Mods {
        shift: m.shift,
        ctrl: m.ctrl,
        alt: m.alt,
    }
}

/// Ce que le navigateur demande, branche sur le chargeur et REAPER.
struct Adapter<'a> {
    host: HostRef,
    r: Reaper,
    hwnd: Hwnd,
    docked: bool,
    perf: bool,
    action: &'a mut Option<u32>,
}

impl BrowserHost for Adapter<'_> {
    fn load(&mut self, path: &str) -> u32 {
        self.host.audio_load(path)
    }
    fn poll(&mut self, ticket: u32) -> Loaded {
        match self.host.audio_poll(ticket) {
            AudioLoad::Pending => Loaded::Pending,
            AudioLoad::Ready(c) => Loaded::Ready(c),
            AudioLoad::Failed(e) => Loaded::Failed(e),
        }
    }
    fn info(&mut self, clip: u32) -> Option<ClipInfo> {
        self.host.audio_info(clip).map(|i| ClipInfo {
            frames: i.frames,
            nch: i.nch,
            srate: i.srate,
            decoded: i.decoded,
        })
    }
    fn release(&mut self, clip: u32) {
        self.host.audio_release(clip)
    }
    fn peaks(
        &mut self,
        clip: u32,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        min: &mut [f32],
        max: &mut [f32],
    ) -> usize {
        self.host.audio_peaks(clip, ch, start, fpp, min, max)
    }
    fn play(&mut self, clip: u32, start: i64, end: i64, looped: bool, gain: f64) -> u32 {
        self.host.audio_play(0, clip, start, end, looped, gain)
    }
    fn stop(&mut self, voice: u32, fade_s: f64) {
        self.host.audio_stop(voice, fade_s)
    }
    fn voice(&mut self, voice: u32) -> Option<(f64, i32)> {
        self.host.audio_voice(voice)
    }
    fn set_gain(&mut self, voice: u32, gain: f64) {
        self.host.audio_set(voice, 1, gain);
    }
    fn insert(&mut self, path: &str, section: Option<(f64, f64)>) -> Result<(), String> {
        self.r.insert_media(path, section)
    }
    fn drag_out(&mut self, path: &str, section: Option<(f64, f64)>) -> bool {
        // Le geste appartient desormais au shell : on lui laisse la souris.
        ui::release_capture();
        match cp_sys::shell::drag_file(self.hwnd, path) {
            DragResult::Dropped => {
                if let Some((a, b)) = section {
                    self.r.trim_dropped(path, a, b);
                }
                true
            }
            DragResult::Cancelled => true,
            DragResult::Failed => false,
        }
    }
    fn set_clipboard(&mut self, s: &str) {
        ui::set_clipboard_text(self.hwnd, s);
    }
    fn clipboard(&mut self) -> Option<String> {
        ui::clipboard_text(self.hwnd)
    }
    fn menu(&mut self, x: f32, y: f32, items: &[MenuEntry]) -> u32 {
        let (sx, sy) = ui::client_to_screen(self.hwnd, x as i32, y as i32);
        let v: Vec<Option<MenuItem<'_>>> = items
            .iter()
            .map(|e| {
                (e.id != 0).then_some(MenuItem {
                    id: e.id,
                    text: &e.text,
                    checked: e.checked,
                    enabled: true,
                })
            })
            .collect();
        ui::popup_menu(self.hwnd, sx, sy, &v)
    }
    fn save(&mut self, key: &str, value: &str) {
        self.r
            .ext_set(SECTION, &format!("browser_{key}"), value, true);
    }
    fn window_items(&mut self) -> Vec<MenuEntry> {
        vec![
            MenuEntry::new(WIN_DOCK, "Dock in Docker", self.docked),
            MenuEntry::new(WIN_PERF, "Performance overlay", self.perf),
            MenuEntry::new(WIN_CLOSE, "Close", false),
        ]
    }
    fn window_action(&mut self, id: u32) {
        *self.action = Some(id);
    }
    fn project_bpm(&mut self) -> f64 {
        self.r.master_tempo()
    }
    fn set_rate(&mut self, voice: u32, rate: f64) {
        self.host.audio_set(voice, 0, rate);
    }
    fn insert_rate(&mut self, rate: f64) {
        self.r.set_selected_item_rate(rate);
    }
    fn swap_begin(&mut self) -> Result<(String, f64), String> {
        self.r.selected_take_file().ok_or_else(|| {
            "select an item in the arrange first (hot swap replaces its file)".to_string()
        })
    }
    fn swap(&mut self, path: &str, offset: f64) -> Result<(), String> {
        self.r.swap_take_source(path, offset)
    }
    fn installed_fx(&mut self) -> Vec<String> {
        self.r.installed_fx().into_iter().map(|(n, _)| n).collect()
    }
    fn add_fx(&mut self, name: &str) -> Result<(), String> {
        self.r.add_fx(name)
    }
    fn thumb(&mut self, path: &str, min: &mut [f32], max: &mut [f32]) -> i32 {
        self.host.audio_thumb(path, min, max)
    }
}

/// La fenetre du navigateur.
struct View {
    hwnd: Hwnd,
    docked: bool,
    surf: Surface,
    present: Vec<u32>,
    theme: Theme,
    tracking: bool,
    frames_on: bool,
    /// Nous detruisons la fenetre nous-memes (pas une fermeture par REAPER).
    closing: bool,
    perf: Perf,
}

#[derive(Default)]
struct Perf {
    last_ms: f32,
    avg_ms: f32,
    worst_ms: f32,
    drawn: u64,
}

struct App {
    host: HostRef,
    r: Reaper,
    cmd_browser: i32,
    cmd_perf: i32,
    cmd_dock: i32,
    browser: Browser,
    view: Option<View>,
    text: TextCache,
    icons: IconCache,
    scratch: Scratch,
    action: Option<u32>,
    show_perf: bool,
}

fn read_shortcuts(r: &Reaper) -> (Vec<Shortcut>, PathBuf) {
    let res = r.resource_path().unwrap_or_default();
    let ini = std::fs::read(res.join("reaper.ini"))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    (model::read_shortcuts(&ini), res.join("MediaDB"))
}

impl App {
    fn ext(&self, key: &str) -> Option<String> {
        self.r.ext_get(SECTION, key)
    }

    fn adapter_parts(&mut self) -> (Hwnd, bool) {
        self.view
            .as_ref()
            .map_or((Hwnd(0), false), |v| (v.hwnd, v.docked))
    }

    /// Fait `f` avec le navigateur et son hote.
    fn with_browser<T>(&mut self, f: impl FnOnce(&mut Browser, &mut Adapter<'_>) -> T) -> T {
        let (hwnd, docked) = self.adapter_parts();
        let mut ad = Adapter {
            host: self.host,
            r: self.r,
            hwnd,
            docked,
            perf: self.show_perf,
            action: &mut self.action,
        };
        f(&mut self.browser, &mut ad)
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    fn open_view(&mut self) {
        if self.view.is_some() {
            if let Some(v) = &self.view {
                if v.docked {
                    self.r.dock_activate(v.hwnd);
                } else {
                    win::show(v.hwnd);
                }
            }
            return;
        }
        let docked = self.ext("browser_docked").as_deref() == Some("1");
        let rect: Vec<i32> = self
            .ext("browser_rect")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        let (x, y, w, h) = if rect.len() == 4 && rect[2] > 100 && rect[3] > 100 {
            (rect[0], rect[1], rect[2], rect[3])
        } else {
            (-1, -1, 460, 720)
        };
        let Some(hwnd) = self.host.create_view(TITLE, DOCK_IDENT, x, y, w, h, docked) else {
            self.host
                .error("la fenetre du navigateur n'a pas pu etre creee");
            return;
        };
        self.adopt(hwnd, docked);
        self.r.ext_set(SECTION, "browser_open", "1", true);
    }

    fn adopt(&mut self, hwnd: Hwnd, docked: bool) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        self.view = Some(View {
            hwnd,
            docked,
            surf: Surface::default(),
            present: Vec::new(),
            theme: Theme::dark(scale),
            tracking: false,
            frames_on: false,
            closing: false,
            perf: Perf::default(),
        });
        self.render(true);
    }

    fn save_rect(&self) {
        if let Some(v) = &self.view {
            if !v.docked && win::is_window(v.hwnd) {
                let r = ui::window_rect(v.hwnd);
                let s = format!(
                    "{} {} {} {}",
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top
                );
                self.r.ext_set(SECTION, "browser_rect", &s, true);
            }
        }
    }

    /// `user` : fermee par Cedric (elle ne se rouvrira pas au demarrage).
    fn close_view(&mut self, user: bool) {
        self.save_rect();
        self.with_browser(|b, h| b.stop(h));
        if let Some(v) = &mut self.view {
            v.closing = true;
            let h = v.hwnd;
            self.host.destroy_window(h);
        }
        self.view = None;
        if user {
            self.r.ext_set(SECTION, "browser_open", "0", true);
        }
    }

    fn toggle_dock(&mut self) {
        let Some(v) = &self.view else { return };
        let docked = !v.docked;
        self.close_view(false);
        self.r.ext_set(
            SECTION,
            "browser_docked",
            if docked { "1" } else { "0" },
            true,
        );
        self.open_view();
    }

    fn run_action(&mut self) {
        match self.action.take() {
            Some(WIN_DOCK) => self.toggle_dock(),
            Some(WIN_CLOSE) => self.close_view(true),
            Some(WIN_PERF) => {
                self.show_perf = !self.show_perf;
                self.browser.mark();
                self.want_frames();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Le dessin
    // -----------------------------------------------------------------------

    fn want_frames(&mut self) {
        if let Some(v) = &mut self.view {
            if !v.frames_on {
                v.frames_on = true;
                self.host.frames(v.hwnd, true);
            }
        }
    }

    /// Dessine et presente. `force` : meme si rien n'a change (taille,
    /// exposition).
    fn render(&mut self, force: bool) {
        let Some(v) = self.view.as_mut() else { return };
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
        if resized || force || (self.browser.lay.w as i32 != w) || (self.browser.lay.h as i32 != h)
        {
            self.browser.layout(w as f32, h as f32, scale);
        }
        let dirty = self.browser.take_dirty();
        if !dirty && !force && !resized {
            return;
        }
        let t0 = ui::qpc();
        let (hwnd, docked) = (v.hwnd, v.docked);
        {
            let View {
                surf, theme, perf, ..
            } = &mut *v;
            let mut cv = Canvas::new(surf, &mut self.scratch);
            let mut g = Gfx {
                text: &mut self.text,
                icons: &mut self.icons,
                th: theme,
            };
            let mut ad = Adapter {
                host: self.host,
                r: self.r,
                hwnd,
                docked,
                perf: self.show_perf,
                action: &mut self.action,
            };
            self.browser.draw(&mut cv, &mut g, &mut ad);
            if self.show_perf {
                let fi = self.host.frame_info();
                let hz = if fi.period_qpc > 0 {
                    fi.qpc_freq as f64 / fi.period_qpc as f64
                } else {
                    0.0
                };
                let p = &*perf;
                let line = format!(
                    "{:.1} ms  avg {:.1}  worst {:.1}  ·  {:.0} Hz  ·  missed {}  ·  {} glyphs",
                    p.last_ms,
                    p.avg_ms,
                    p.worst_ms,
                    hz,
                    fi.missed,
                    self.text.cached()
                );
                let f = theme.font_small();
                let tw = self.text.measure(f, &line) + theme.s(12.0);
                let bh = theme.s(18.0);
                let (bx, by) = (
                    w as f32 - tw - theme.s(6.0),
                    self.browser.lay.list_y + theme.s(4.0),
                );
                cv.fill_round_rect(bx, by, tw, bh, theme.s(4.0), cp_gfx::rgba(0, 0, 0, 200));
                self.text.draw_in(
                    &mut cv,
                    f,
                    bx,
                    by,
                    tw,
                    bh,
                    &line,
                    cp_gfx::hex(0x8CE08C),
                    Align::Center,
                );
            }
        }
        ui::present(v.hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
        let ms = ((ui::qpc() - t0) as f64 * 1000.0 / ui::qpc_freq() as f64) as f32;
        let p = &mut v.perf;
        p.last_ms = ms;
        p.avg_ms = if p.drawn == 0 {
            ms
        } else {
            p.avg_ms * 0.95 + ms * 0.05
        };
        p.worst_ms = p.worst_ms.max(ms) * 0.999;
        p.drawn += 1;
        if self.show_perf {
            // Le compteur change a chaque image : on garde la cadence.
            self.browser.mark();
        }
    }

    fn on_frame(&mut self) {
        self.with_browser(|b, h| b.tick(h));
        self.run_action();
        self.render(false);
        let idle = !self.browser.animating() && !self.show_perf;
        if let Some(v) = &mut self.view {
            if idle && v.frames_on {
                v.frames_on = false;
                self.host.frames(v.hwnd, false);
            }
        }
    }

    /// Un evenement pour le navigateur ; redessine a la prochaine image.
    fn feed(&mut self, ev: Ev) {
        let changed = self.with_browser(|b, h| b.event(h, &ev));
        self.run_action();
        if changed || self.browser.animating() {
            self.browser.mark();
            self.want_frames();
        }
    }

    // -----------------------------------------------------------------------
    // Le clavier
    // -----------------------------------------------------------------------

    /// Le crochet : 1 mangee, 0 a REAPER, -1 a livrer a la fenetre.
    fn accel(&mut self, m: &WinMsg) -> isize {
        let Some(k) = key_event(m) else { return 0 };
        let vk = k.wp as u32;
        match k.message {
            msg::KEYDOWN | msg::SYSKEYDOWN => match self.browser.key_use(vk, mods()) {
                KeyUse::Eat => {
                    self.feed(Ev::Key {
                        vk,
                        mods: mods(),
                        text: None,
                    });
                    1
                }
                KeyUse::Text => -1,
                KeyUse::NotMine => 0,
            },
            msg::CHAR | msg::SYSCHAR => {
                if self.browser.focus == Focus::Search {
                    -1
                } else {
                    0
                }
            }
            msg::KEYUP | msg::SYSKEYUP => {
                if self.browser.focus == Focus::Search {
                    -1
                } else if self.browser.key_use(vk, mods()) == KeyUse::Eat {
                    1
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    // -----------------------------------------------------------------------
    // Les messages de la fenetre
    // -----------------------------------------------------------------------

    fn mouse(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.view.as_ref()?.hwnd;
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        let down = |b: Button, dbl: bool| Ev::Down {
            x,
            y,
            b,
            dbl,
            mods: md,
        };
        match m.msg {
            msg::MOUSEMOVE => {
                if let Some(v) = &mut self.view {
                    if !v.tracking {
                        v.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.feed(Ev::Move { x, y, mods: md });
            }
            msg::MOUSELEAVE => {
                if let Some(v) = &mut self.view {
                    v.tracking = false;
                }
                self.feed(Ev::Leave);
            }
            msg::LBUTTONDOWN
            | msg::LBUTTONDBLCLK
            | msg::RBUTTONDOWN
            | msg::RBUTTONDBLCLK
            | msg::MBUTTONDOWN
            | msg::MBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                let (b, dbl) = match m.msg {
                    msg::LBUTTONDOWN => (Button::Left, false),
                    msg::LBUTTONDBLCLK => (Button::Left, true),
                    msg::RBUTTONDOWN => (Button::Right, false),
                    msg::RBUTTONDBLCLK => (Button::Right, true),
                    msg::MBUTTONDOWN => (Button::Middle, false),
                    _ => (Button::Middle, true),
                };
                if b != Button::Right {
                    ui::capture(hwnd);
                }
                self.feed(down(b, dbl));
            }
            msg::LBUTTONUP | msg::RBUTTONUP | msg::MBUTTONUP => {
                let b = match m.msg {
                    msg::LBUTTONUP => Button::Left,
                    msg::RBUTTONUP => Button::Right,
                    _ => Button::Middle,
                };
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                self.feed(Ev::Up { x, y, b, mods: md });
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                let (sx, sy) = (lo16(m.lp) as i32, hi16(m.lp) as i32);
                let (cx, cy) = ui::screen_to_client(hwnd, sx, sy);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                let (dx, dy) = if m.msg == msg::MOUSEWHEEL {
                    (0.0, d)
                } else {
                    (d, 0.0)
                };
                self.feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
            }
            _ => return None,
        }
        Some(0)
    }

    fn cursor(&self, hwnd: Hwnd) {
        let (sx, sy) = ui::cursor_pos();
        let (x, y) = ui::screen_to_client(hwnd, sx, sy);
        let l = &self.browser.lay;
        let (x, y) = (x as f32, y as f32);
        let c = if y < l.bar_h && x >= l.search_x && x < l.search_x + l.search_w {
            Cursor::IBeam
        } else if self.browser.over_split(y) {
            Cursor::SizeNS
        } else {
            Cursor::Arrow
        };
        ui::set_cursor(c);
    }
}

// ---------------------------------------------------------------------------
// L'etat entre deux generations
// ---------------------------------------------------------------------------

const STATE_TAG: &str = "cp_module 2";

fn encode(app: &App) -> Vec<u8> {
    let mut s = format!("{STATE_TAG}\n");
    if let Some(v) = &app.view {
        s += &format!("hwnd\t{}\ndocked\t{}\n", v.hwnd.0, v.docked as u8);
    }
    s += &format!("perf\t{}\n", app.show_perf as u8);
    s += "browser\n";
    s += &app.browser.save_state();
    s.into_bytes()
}

/// (la vue : sa fenetre et si elle est ancree), l'affichage des mesures, et
/// l'etat du navigateur.
type Saved = (Option<(Hwnd, bool)>, bool, String);

fn decode(st: &[u8]) -> Option<Saved> {
    let s = std::str::from_utf8(st).ok()?;
    let mut lines = s.split_inclusive('\n');
    if lines.next()?.trim_end() != STATE_TAG {
        return None;
    }
    let (mut hwnd, mut docked, mut perf) = (0isize, false, false);
    let mut rest = String::new();
    let mut in_browser = false;
    for l in lines {
        if in_browser {
            rest.push_str(l);
            continue;
        }
        let t = l.trim_end();
        if t == "browser" {
            in_browser = true;
        } else if let Some(v) = t.strip_prefix("hwnd\t") {
            hwnd = v.parse().unwrap_or(0);
        } else if let Some(v) = t.strip_prefix("docked\t") {
            docked = v == "1";
        } else if let Some(v) = t.strip_prefix("perf\t") {
            perf = v == "1";
        }
    }
    let view = (hwnd != 0).then_some((Hwnd(hwnd), docked));
    Some((view, perf, rest))
}

impl Module for App {
    fn init(host: HostRef, state: &[u8]) -> Self {
        let r = host.reaper();
        let (sc, mediadb) = read_shortcuts(&r);
        let project = r.project_dir();
        let favs: Vec<PathBuf> = r
            .ext_get(SECTION, "browser_favorites")
            .unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        let mut browser = Browser::new(&sc, &mediadb, project.as_deref(), favs);
        if let Some(v) = r
            .ext_get(SECTION, "browser_volume")
            .and_then(|v| v.parse().ok())
        {
            browser.volume = v;
        }
        browser.looping = r.ext_get(SECTION, "browser_loop").as_deref() == Some("1");
        browser.sync = r.ext_get(SECTION, "browser_sync").as_deref() == Some("1");
        browser.samples_view = r.ext_get(SECTION, "browser_samples").as_deref() == Some("1");
        if let Some(v) = r
            .ext_get(SECTION, "browser_wave_h")
            .and_then(|v| v.parse().ok())
        {
            browser.wave_h_pref = v;
        }
        browser.data_dir = r.resource_path().map(|p| p.join("CP_Studio"));
        browser.load_collections();
        let cmd_browser =
            host.register_action("CPSTUDIO_BROWSER", "CP Studio: Browser (show/hide)");
        let cmd_perf = host.register_action(
            "CPSTUDIO_PERF",
            "CP Studio: Performance overlay (show/hide)",
        );
        let cmd_dock =
            host.register_action("CPSTUDIO_BROWSER_DOCK", "CP Studio: Browser - dock/undock");
        let mut app = App {
            host,
            r,
            cmd_browser,
            cmd_perf,
            cmd_dock,
            browser,
            view: None,
            text: TextCache::new(Box::new(GdiGlyphs::new(&["Segoe UI", "Consolas"]))),
            icons: IconCache::default(),
            scratch: Scratch::default(),
            action: None,
            show_perf: false,
        };
        match decode(state) {
            Some((view, perf, bstate)) => {
                app.show_perf = perf;
                app.browser.restore_state(&bstate);
                if let Some((h, docked)) = view {
                    if win::is_window(h) {
                        app.adopt(h, docked);
                        app.want_frames();
                    }
                }
            }
            None => {
                // Premier chargement de la session : rouvrir si elle l'etait.
                if app.ext("browser_open").as_deref() == Some("1") {
                    app.open_view();
                }
            }
        }
        host.info(&format!(
            "module pret : generation {}, navigateur {}",
            host.generation(),
            if app.view.is_some() {
                "ouvert"
            } else {
                "ferme"
            }
        ));
        app
    }

    fn shutdown(&mut self) {
        self.save_rect();
        self.host.store_state(&encode(self));
        let (hwnd, docked) = self.adapter_parts();
        let mut ad = Adapter {
            host: self.host,
            r: self.r,
            hwnd,
            docked,
            perf: self.show_perf,
            action: &mut self.action,
        };
        self.browser.release_all(&mut ad);
        // La fenetre reste au chargeur : le module suivant la reprend. Elle
        // ne recevra plus d'images d'ici la.
        if let Some(v) = &self.view {
            self.host.frames(v.hwnd, false);
        }
    }

    fn tick(&mut self) {
        if self.view.is_none() {
            return;
        }
        self.with_browser(|b, h| b.tick(h));
        if self.browser.animating() {
            self.want_frames();
        } else if self.browser.take_dirty() {
            self.browser.mark();
            self.want_frames();
        }
    }

    fn on_action(&mut self, cmd: i32) -> bool {
        if cmd == self.cmd_browser {
            if self.view.is_some() {
                self.close_view(true);
            } else {
                self.open_view();
            }
            true
        } else if cmd == self.cmd_perf {
            self.show_perf = !self.show_perf;
            self.browser.mark();
            self.want_frames();
            true
        } else if cmd == self.cmd_dock {
            if self.view.is_some() {
                self.toggle_dock();
            } else {
                let docked = self.ext("browser_docked").as_deref() == Some("1");
                self.r.ext_set(
                    SECTION,
                    "browser_docked",
                    if docked { "0" } else { "1" },
                    true,
                );
                self.open_view();
            }
            true
        } else {
            false
        }
    }

    fn toggle_state(&mut self, cmd: i32) -> Option<bool> {
        if cmd == self.cmd_browser {
            Some(self.view.is_some())
        } else if cmd == self.cmd_perf {
            Some(self.show_perf)
        } else if cmd == self.cmd_dock {
            Some(self.view.as_ref().is_some_and(|v| v.docked))
        } else {
            None
        }
    }

    fn wnd_proc(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.view.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        match m.msg {
            WM_CP_FRAME => {
                self.on_frame();
                Some(0)
            }
            WM_CP_ACCEL => Some(self.accel(m)),
            msg::SIZE => {
                self.render(true);
                Some(0)
            }
            msg::PAINT => {
                // Valider la zone, puis tout presenter.
                drop(win::Paint::begin(hwnd));
                if let Some(v) = &mut self.view {
                    let (w, h) = (v.surf.w, v.surf.h);
                    if w > 0 && h > 0 {
                        ui::present(hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
                    } else {
                        self.render(true);
                    }
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::SETCURSOR => {
                if (m.lp & 0xFFFF) == 1 {
                    self.cursor(hwnd);
                    Some(1)
                } else {
                    None
                }
            }
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001), // toutes les touches
            msg::KEYDOWN | msg::SYSKEYDOWN => {
                let vk = m.wp as u32;
                self.feed(Ev::Key {
                    vk,
                    mods: mods(),
                    text: None,
                });
                Some(0)
            }
            msg::CHAR => {
                if let Some(c) = char::from_u32(m.wp as u32) {
                    self.feed(Ev::Char(c));
                }
                Some(0)
            }
            msg::CLOSE => {
                self.close_view(true);
                Some(0)
            }
            msg::DESTROY => {
                // Detruite par REAPER (sa fermeture, un docker ferme) : on
                // oublie la vue sans toucher au reglage « ouverte ».
                let ours = self.view.as_ref().is_some_and(|v| v.closing);
                if !ours {
                    self.save_rect();
                    self.with_browser(|b, h| b.stop(h));
                    self.view = None;
                }
                None
            }
            _ => self.mouse(m),
        }
    }
}

cp_sys::export_module!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_etat_se_relit() {
        let bstate = "open\tC:\\x\nsel\tC:\\x\\a.wav\n";
        let s = format!("{STATE_TAG}\nhwnd\t1234\ndocked\t1\nperf\t0\nbrowser\n{bstate}");
        let (v, perf, rest) = decode(s.as_bytes()).unwrap();
        assert_eq!(v, Some((Hwnd(1234), true)));
        assert!(!perf);
        assert_eq!(rest, bstate);
        assert!(decode(b"autre chose\n").is_none());
    }
}
