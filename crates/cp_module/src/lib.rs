//! `cp_module.dll` : le code de CP Studio, recharge a chaud par le chargeur.
//!
//! Il porte :
//! - le navigateur de fichiers (jalon 1), dans une vraie fenetre de REAPER :
//!   flottante, ancree dans un docker, ou A LA PLACE DE L'ARRANGEMENT (la
//!   place native que tient le chargeur) ;
//! - CP Transport (`tview.rs`), a la place du transport de REAPER, avec les
//!   icones de Cedric et le selecteur de vues (Arrange, Browser...).
//!
//! Pour chaque fenetre :
//! - il traduit les messages de Windows en evenements du toolkit ;
//! - il dessine a la cadence de l'ecran (WM_CP_FRAME) quand quelque chose a
//!   change, et ne fait rien sinon ;
//! - il repond au crochet clavier (WM_CP_ACCEL) : ce que le navigateur prend,
//!   ce qu'il laisse a REAPER, ce qui est du texte (le transport ne prend
//!   jamais le clavier) ;
//! - il garde, d'une generation a l'autre, les fenetres et leur etat.

use std::path::PathBuf;

use cp_abi::{
    PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK, VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL, WM_CP_FRAME,
};
use cp_browser::draw::Gfx;
use cp_browser::model::{self, Places, Shortcut};
use cp_browser::wave::ClipInfo;
use cp_browser::{Browser, Focus, Host as BrowserHost, KeyUse, Loaded, MenuEntry, Redraw};
use cp_gfx::text::{Align, TextCache};
use cp_gfx::{Canvas, Scratch, Surface};
use cp_sys::font::GdiGlyphs;
use cp_sys::module::{AudioLoad, HostRef, Module, key_event};
use cp_sys::reaper::Reaper;
use cp_sys::shell::DragResult;
use cp_sys::ui::{self, Cursor, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, IconCache, Mods, Theme};

mod clocksync;
mod editor;
mod frame;
mod gtab;
mod items;
mod native;
mod tuner;
mod tview;

pub(crate) const SECTION: &str = "CP_Studio";
const DOCK_IDENT: &str = "cp_studio_browser";
const TITLE: &str = "CP Browser";

const WIN_CLOSE: u32 = 1002;
const WIN_PERF: u32 = 1003;
const WIN_PLACE_ARRANGE: u32 = 1004;
const WIN_PLACE_DOCK: u32 = 1005;
const WIN_PLACE_FLOAT: u32 = 1006;

pub(crate) mod msg {
    pub const DESTROY: u32 = 0x0002;
    pub const SIZE: u32 = 0x0005;
    pub const PAINT: u32 = 0x000F;
    pub const CLOSE: u32 = 0x0010;
    pub const ERASEBKGND: u32 = 0x0014;
    pub const SETCURSOR: u32 = 0x0020;
    pub const MOUSEACTIVATE: u32 = 0x0021;
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

pub(crate) fn lo16(v: isize) -> f32 {
    (v & 0xFFFF) as u16 as i16 as f32
}
pub(crate) fn hi16(v: isize) -> f32 {
    ((v >> 16) & 0xFFFF) as u16 as i16 as f32
}

pub(crate) fn mods() -> Mods {
    let m = ui::mods();
    Mods {
        shift: m.shift,
        ctrl: m.ctrl,
        alt: m.alt,
        win: m.win,
    }
}

/// Ou vit le navigateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// A la place de l'arrangement (la vue choisie dans CP Transport).
    Arrange,
    Dock,
    Float,
}

impl Place {
    fn key(self) -> &'static str {
        match self {
            Place::Arrange => "arrange",
            Place::Dock => "dock",
            Place::Float => "float",
        }
    }
    fn from_key(s: &str) -> Option<Place> {
        match s {
            "arrange" => Some(Place::Arrange),
            "dock" => Some(Place::Dock),
            "float" => Some(Place::Float),
            _ => None,
        }
    }
}

/// Le passage choisi dans le navigateur quand son fichier est depose dans
/// Guitar Tab : (fichier, debut, fin en secondes du fichier). Guitar Tab le
/// prend en posant la prise.
pub(crate) static DROP_SECTION: std::sync::Mutex<Option<(String, f64, f64)>> =
    std::sync::Mutex::new(None);

/// Ce que le navigateur demande, branche sur le chargeur et REAPER.
struct Adapter<'a> {
    host: HostRef,
    r: Reaper,
    hwnd: Hwnd,
    /// La fenetre de Guitar Tab (un depot y arrive en WM_DROPFILES).
    gtab: Option<Hwnd>,
    place: Place,
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
                // Depose dans Guitar Tab : c'est elle qui pose la prise (et
                // garde le passage choisi).
                let in_gtab = self
                    .gtab
                    .is_some_and(|g| cp_sys::shell::pending(g, cp_sys::shell::WM_DROPFILES));
                if in_gtab {
                    if let (Some((a, b)), Ok(mut s)) = (section, DROP_SECTION.lock()) {
                        *s = Some((path.to_string(), a, b));
                    }
                } else if let Some((a, b)) = section {
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
            MenuEntry::new(
                WIN_PLACE_ARRANGE,
                "Show in arrange view",
                self.place == Place::Arrange,
            ),
            MenuEntry::new(WIN_PLACE_DOCK, "Dock in docker", self.place == Place::Dock),
            MenuEntry::new(
                WIN_PLACE_FLOAT,
                "Floating window",
                self.place == Place::Float,
            ),
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
        // Hors vitesse 1, l'interpolation de haute qualite (le sinc,
        // `Param::Interp` = 19) : rien ne se replie quand la hauteur monte.
        self.host.audio_set(voice, 19, 1.0);
    }
    fn insert_rate(&mut self, rate: f64) {
        self.r.set_selected_item_rate(rate);
    }
    fn stretch(&mut self, clip: u32, start: i64, end: i64, tempo: f64) -> u32 {
        // L'etireur de REAPER (elastique) : la hauteur reste.
        self.host.audio_stretch(clip, start, end, tempo, 1.0)
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
    fn pick_folder(&mut self, start: Option<&std::path::Path>) -> Option<PathBuf> {
        cp_sys::folders::pick_folder(self.hwnd, start, "Add a folder to CP Browser")
    }
    fn show_in_explorer(&mut self, p: &std::path::Path) {
        cp_sys::folders::show_in_explorer(p);
    }
    fn reaper_shortcuts(&mut self) -> Vec<model::Place> {
        let (sc, mediadb) = read_shortcuts(&self.r);
        Places::from_shortcuts(&sc, &mediadb, self.r.project_dir().as_deref())
    }
    fn thumb(&mut self, path: &str, min: &mut [f32], max: &mut [f32]) -> i32 {
        self.host.audio_thumb(path, min, max)
    }
}

/// Ce qu'on fait aux fenetres, au battement : JAMAIS pendant le traitement
/// d'un message d'une de nos fenetres (detruire une fenetre ancree depuis son
/// propre clic gelait REAPER).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WinOp {
    Open,
    Close,
    ToggleDock,
    Place(Place),
    ViewArrange,
    ViewBrowser,
    TransportOn,
    TransportOff,
    GtabOpen,
    GtabClose,
    GtabPlace(Place),
    EdOpen,
    EdClose,
    EdPlace(Place),
}

/// La fenetre du navigateur.
struct View {
    hwnd: Hwnd,
    place: Place,
    /// L'infobulle : ce qu'elle explique (et depuis quand la souris y est),
    /// et la bulle de Windows.
    tip_key: Option<(String, std::time::Instant)>,
    tip: Option<ui::Tooltip>,
    tip_shown: bool,
    mouse: (f32, f32),
    surf: Surface,
    present: Vec<u32>,
    /// Ce qui est a l'ecran (pour n'envoyer que ce qui change).
    shown: Vec<u32>,
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

pub(crate) struct App {
    host: HostRef,
    r: Reaper,
    cmd_browser: i32,
    cmd_perf: i32,
    cmd_dock: i32,
    cmd_state: i32,
    cmd_transport: i32,
    cmd_view_arrange: i32,
    cmd_view_browser: i32,
    cmd_place_arrange: i32,
    cmd_place_dock: i32,
    cmd_place_float: i32,
    cmd_gtab: i32,
    cmd_gtab_set: i32,
    cmd_view_gtab: i32,
    cmd_gtab_make: i32,
    cmd_editor: i32,
    /// L'editeur : la fenetre, l'item ouvert.
    ed: editor::EPart,
    /// Une transcription Guitar Tab en cours.
    /// Guitar Tab : la fenetre, la prise, la transcription.
    gt: gtab::GPart,
    browser: Browser,
    view: Option<View>,
    /// La place choisie pour le navigateur.
    place: Place,
    tp: tview::TPart,
    /// Le navigateur habille du theme : ses boutons, et ce dont ils dependent
    /// (fichier du theme, couleurs, echelle).
    bskin: Option<cp_browser::draw::Skin>,
    blook: Option<LookKey>,
    text: TextCache,
    /// La famille 2 du texte : la police du transport du theme.
    face: String,
    icons: IconCache,
    scratch: Scratch,
    action: Option<u32>,
    show_perf: bool,
    ops: Vec<WinOp>,
    /// Battements depuis le chargement : au demarrage de REAPER, on attend
    /// qu'il ait fini de se construire avant d'ouvrir (et d'ancrer).
    ticks: u32,
    /// Les raccourcis de la fenetre principale de REAPER (les editions au
    /// survol), lus une fois.
    main_keys: Option<cp_editview::keys::KeyMap>,
    /// Les touches de modification au battement d'avant (le curseur suit).
    last_mods: Mods,
}

fn read_shortcuts(r: &Reaper) -> (Vec<Shortcut>, PathBuf) {
    let res = r.resource_path().unwrap_or_default();
    let ini = std::fs::read(res.join("reaper.ini"))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    (model::read_shortcuts(&ini), res.join("MediaDB"))
}

/// Ce dont depend l'habillage du navigateur : le fichier du theme, ses
/// couleurs, l'echelle (x 100).
type LookKey = (Option<PathBuf>, Vec<Option<(u8, u8, u8)>>, i32);

/// Les couleurs du theme que le navigateur lit (et qu'on surveille).
const BROWSER_COLORS: [&str; 10] = [
    "genlist_bg",
    "genlist_fg",
    "genlist_grid",
    "genlist_selbg",
    "genlist_seliabg",
    "col_main_bg2",
    "col_main_text",
    "col_main_text2",
    "col_main_editbk",
    "col_toolbar_text_on",
];

impl App {
    /// Habille le navigateur du theme de REAPER s'il a change : ses couleurs
    /// (celles des listes de REAPER) et les boutons du transport du theme.
    pub(crate) fn browser_look(&mut self) {
        let Some(scale) = self.view.as_ref().map(|v| v.theme.scale) else {
            return;
        };
        let r = self.r;
        let key = (
            r.theme_file(),
            BROWSER_COLORS
                .iter()
                .map(|k| r.theme_color(k))
                .collect::<Vec<_>>(),
            (scale * 100.0).round() as i32,
        );
        if self.blook.as_ref() == Some(&key) {
            return;
        }
        let tb = |t: &cp_theme::ThemeFiles, name: &str| {
            let (img, s) = t.image(name, scale)?;
            Some(cp_theme::Frames::split(&img, 3)?.rescaled(s, scale))
        };
        self.bskin = self
            .theme_files(key.0.as_deref())
            .map(|t| cp_browser::draw::Skin {
                play: tb(t, "transport_play"),
                play_on: tb(t, "transport_play_on"),
                repeat_off: tb(t, "transport_repeat_off"),
                repeat_on: tb(t, "transport_repeat_on"),
            });
        self.browser.theme_btn_w = self
            .bskin
            .as_ref()
            .and_then(|s| s.play.as_ref().map(|f| f.w() as f32));
        if let Some(v) = self.view.as_mut() {
            v.theme = Theme::reaper(scale, &|k| r.theme_color(k));
            let (w, h) = (self.browser.lay.w, self.browser.lay.h);
            if w > 0.0 && h > 0.0 {
                self.browser.layout(w, h, scale);
            }
        }
        self.blook = Some(key);
        self.browser.mark();
        self.want_frames();
    }

    fn ext(&self, key: &str) -> Option<String> {
        self.r.ext_get(SECTION, key)
    }

    fn adapter_parts(&mut self) -> (Hwnd, Place) {
        self.view
            .as_ref()
            .map_or((Hwnd(0), self.place), |v| (v.hwnd, v.place))
    }

    /// Fait `f` avec le navigateur et son hote.
    fn with_browser<T>(&mut self, f: impl FnOnce(&mut Browser, &mut Adapter<'_>) -> T) -> T {
        let (hwnd, place) = self.adapter_parts();
        let mut ad = Adapter {
            host: self.host,
            r: self.r,
            hwnd,
            gtab: self.gt.win.as_ref().map(|w| w.hwnd),
            place,
            perf: self.show_perf,
            action: &mut self.action,
        };
        f(&mut self.browser, &mut ad)
    }

    // -----------------------------------------------------------------------
    // La fenetre du navigateur
    // -----------------------------------------------------------------------

    /// Le navigateur est-il pose a la place de l'arrangement, visible ?
    fn browser_in_band(&self) -> bool {
        let Some(v) = &self.view else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(v.hwnd)
    }

    /// Montre le navigateur a sa place (le cree s'il le faut).
    fn open_view(&mut self) {
        if let Some(v) = &self.view {
            match v.place {
                Place::Dock => self.r.dock_activate(v.hwnd),
                Place::Float => win::show(v.hwnd),
                Place::Arrange => {
                    let h = v.hwnd;
                    if !self.browser_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true)
                    {
                        self.render(true);
                        self.want_frames();
                    }
                }
            }
            return;
        }
        let place = self.place;
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
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, DOCK_IDENT, x, y, w, h, mode) else {
            self.host
                .error("la fenetre du navigateur n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange {
            // A la taille de la place, cachee ; dessinee ; puis montree a la
            // place de l'arrangement : jamais une image vide a l'ecran.
            if !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
                self.host.destroy_window(hwnd);
                self.host
                    .warn("pas d'arrangement a remplacer : le navigateur s'ancre");
                self.place = Place::Dock;
                self.r.ext_set(SECTION, "browser_place", "dock", true);
                self.open_view();
                return;
            }
            self.adopt(hwnd, place);
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.render(true);
        } else {
            self.adopt(hwnd, place);
        }
        self.r.ext_set(SECTION, "browser_open", "1", true);
    }

    fn adopt(&mut self, hwnd: Hwnd, place: Place) {
        // Des fichiers deposes depuis l'Explorateur : WM_DROPFILES.
        cp_sys::shell::accept_files(hwnd, true);
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        self.view = Some(View {
            hwnd,
            place,
            surf: Surface::default(),
            present: Vec::new(),
            shown: Vec::new(),
            theme: Theme::reaper(scale, &|k| self.r.theme_color(k)),
            tracking: false,
            frames_on: false,
            closing: false,
            perf: Perf::default(),
            tip_key: None,
            tip: None,
            tip_shown: false,
            mouse: (-1.0, -1.0),
        });
        self.blook = None;
        self.browser_look();
        self.render(true);
        // Ce qui a ete demande avant que la fenetre existe (les mesures, une
        // ecoute) reprend sa cadence.
        if self.show_perf || self.browser.animating() {
            self.want_frames();
        }
    }

    fn save_rect(&self) {
        if let Some(v) = &self.view {
            if v.place == Place::Float && win::is_window(v.hwnd) {
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
            if v.place == Place::Arrange {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.view = None;
        if user {
            self.r.ext_set(SECTION, "browser_open", "0", true);
        }
    }

    /// Change la place du navigateur (et l'y montre).
    pub(crate) fn set_place(&mut self, p: Place) {
        let reopen = self.view.as_ref().is_some_and(|v| v.place != p);
        self.place = p;
        self.r.ext_set(SECTION, "browser_place", p.key(), true);
        if reopen {
            self.close_view(false);
        }
        self.open_view();
    }

    /// « Arrange » dans le selecteur : l'arrangement revient, la vue qui
    /// l'occupait est garee (cachee, vivante).
    pub(crate) fn view_arrange(&mut self) {
        let (st, _) = self.host.place_state(PLACE_ARRANGE);
        if st == PLACE_HELD {
            self.host.place_take(PLACE_ARRANGE, None, false);
            if let Some(v) = &mut self.view {
                if v.frames_on {
                    v.frames_on = false;
                    self.host.frames(v.hwnd, false);
                }
            }
        }
    }

    /// « Browser » dans le selecteur : une destination, pas un interrupteur.
    pub(crate) fn view_browser(&mut self) {
        self.open_view();
    }

    fn toggle_dock(&mut self) {
        let p = if self.place == Place::Dock {
            Place::Float
        } else {
            Place::Dock
        };
        self.set_place(p);
    }

    /// Fait les operations de fenetre en attente (depuis le battement).
    fn do_ops(&mut self) {
        let ops = std::mem::take(&mut self.ops);
        for op in ops {
            match op {
                WinOp::Open => self.open_view(),
                WinOp::Close => self.close_view(true),
                WinOp::ToggleDock => self.toggle_dock(),
                WinOp::Place(p) => self.set_place(p),
                WinOp::ViewArrange => self.view_arrange(),
                WinOp::ViewBrowser => self.view_browser(),
                WinOp::TransportOn => self.transport_start(),
                WinOp::TransportOff => self.transport_stop(true),
                WinOp::GtabOpen => self.gtab_open(),
                WinOp::GtabClose => self.gtab_close(true),
                WinOp::GtabPlace(p) => self.gtab_set_place(p),
                WinOp::EdOpen => self.ed_open(),
                WinOp::EdClose => self.ed_close(true),
                WinOp::EdPlace(p) => self.ed_set_place(p),
            }
        }
        if self.tp.view.is_some() {
            self.tp.t.views = self.view_tabs();
            self.tp.t.mark();
            self.transport_render(false);
        }
    }

    pub(crate) fn queue(&mut self, op: WinOp) {
        if !self.ops.contains(&op) {
            self.ops.push(op);
        }
    }

    fn run_action(&mut self) {
        match self.action.take() {
            Some(WIN_CLOSE) => self.queue(WinOp::Close),
            Some(WIN_PLACE_ARRANGE) => self.queue(WinOp::Place(Place::Arrange)),
            Some(WIN_PLACE_DOCK) => self.queue(WinOp::Place(Place::Dock)),
            Some(WIN_PLACE_FLOAT) => self.queue(WinOp::Place(Place::Float)),
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
            if !v.frames_on && ui::is_shown(v.hwnd) {
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
            v.theme = Theme::reaper(scale, &|k| self.r.theme_color(k));
            self.browser_look();
            self.render(true);
            return;
        }
        let resized = v.surf.resize(w, h);
        if resized || force || (self.browser.lay.w as i32 != w) || (self.browser.lay.h as i32 != h)
        {
            self.browser.layout(w as f32, h as f32, scale);
        }
        // NE REDESSINER QUE CE QUI CHANGE : la tete de lecture et les mesures
        // ne touchent que le bas. Tout redessiner a chaque image d'une grande
        // vue ancree coutait plus d'une periode de l'ecran.
        let kind = if force || resized {
            self.browser.take_redraw();
            Redraw::Full
        } else {
            self.browser.take_redraw()
        };
        if kind == Redraw::None {
            return;
        }
        let (band_y, band_h) = self.browser.wave_band();
        let t0 = ui::qpc();
        let (hwnd, place) = (v.hwnd, v.place);
        {
            let View {
                surf, theme, perf, ..
            } = &mut *v;
            let mut cv = Canvas::new(surf, &mut self.scratch);
            let mut g = Gfx {
                text: &mut self.text,
                icons: &mut self.icons,
                th: theme,
                skin: self.bskin.as_ref(),
            };
            let mut ad = Adapter {
                host: self.host,
                r: self.r,
                hwnd,
                gtab: self.gt.win.as_ref().map(|w| w.hwnd),
                place,
                perf: self.show_perf,
                action: &mut self.action,
            };
            if kind == Redraw::Full {
                self.browser.draw(&mut cv, &mut g, &mut ad);
            } else {
                cv.push_clip(0.0, band_y, w as f32, band_h);
                self.browser.draw_wave_band(&mut cv, &mut g, &mut ad);
                cv.pop_clip();
            }
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
                // Dans la forme d'onde, en haut a droite : il est redessine
                // avec elle.
                let (bx, by) = (
                    w as f32 - tw - theme.s(6.0),
                    self.browser.lay.wave_y + theme.s(4.0),
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
        // N'envoyer a l'ecran que les lignes qui ont change : sur une grande
        // vue ancree, la copie vers l'ecran coutait plus que le dessin.
        let rects: Vec<(i32, i32, i32, i32)> =
            if force || resized || v.shown.len() != v.surf.px.len() {
                v.shown.clear();
                v.shown.extend_from_slice(&v.surf.px);
                vec![(0, 0, w, h)]
            } else {
                let y0 = if kind == Redraw::Full {
                    0
                } else {
                    (band_y.floor() as i32).clamp(0, h) as usize
                };
                cp_gfx::changed_bands(&v.surf.px, &mut v.shown, w as usize, y0, h as usize, 8)
                    .into_iter()
                    .map(|(a, b)| (0, a as i32, w, (b - a) as i32))
                    .collect()
            };
        if !rects.is_empty() {
            ui::present(v.hwnd, &v.surf.px, w, h, &rects, &mut v.present);
        }
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
            // Le compteur change a chaque image : on garde la cadence (le bas
            // seulement).
            self.browser.mark_wave();
        }
    }

    fn on_frame(&mut self) {
        self.with_browser(|b, h| b.tick(h));
        self.run_action();
        self.render(false);
        let hidden = self.view.as_ref().is_some_and(|v| !ui::is_shown(v.hwnd));
        let idle = (!self.browser.animating() && !self.show_perf) || hidden;
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
                    v.mouse = (x, y);
                }
                self.feed(Ev::Move { x, y, mods: md });
            }
            msg::MOUSELEAVE => {
                if let Some(v) = &mut self.view {
                    v.tracking = false;
                    v.mouse = (-1.0, -1.0);
                }
                self.browser_tip_hide();
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

    fn browser_wnd(&mut self, m: &WinMsg) -> Option<isize> {
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
            // Des fichiers deposes depuis l'Explorateur.
            cp_sys::shell::WM_DROPFILES => {
                let (files, _, _) = cp_sys::shell::take_drop(m.wp);
                let paths: Vec<std::path::PathBuf> =
                    files.into_iter().map(std::path::PathBuf::from).collect();
                self.with_browser(|b, h| b.drop_paths(h, &paths));
                self.browser.mark();
                self.want_frames();
                Some(0)
            }
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
                self.queue(WinOp::Close);
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

// ---------------------------------------------------------------------------
// L'etat entre deux generations
// ---------------------------------------------------------------------------

const STATE_TAG: &str = "cp_module 3";

/// Ce qu'une generation passe a la suivante.
#[derive(Debug, Default, PartialEq)]
struct Saved {
    /// La fenetre du navigateur et sa place.
    view: Option<(Hwnd, Place)>,
    place: Option<Place>,
    transport: Option<Hwnd>,
    perf: bool,
    browser: String,
}

fn encode(app: &App) -> Vec<u8> {
    let mut s = format!("{STATE_TAG}\n");
    if let Some(v) = &app.view {
        s += &format!("hwnd\t{}\nplace\t{}\n", v.hwnd.0, v.place.key());
    }
    s += &format!("want\t{}\n", app.place.key());
    if let Some(t) = &app.tp.view {
        s += &format!("transport\t{}\n", t.hwnd.0);
    }
    s += &format!("perf\t{}\n", app.show_perf as u8);
    s += "browser\n";
    s += &app.browser.save_state();
    s.into_bytes()
}

fn decode(st: &[u8]) -> Option<Saved> {
    let s = std::str::from_utf8(st).ok()?;
    let mut lines = s.split_inclusive('\n');
    if lines.next()?.trim_end() != STATE_TAG {
        return None;
    }
    let mut out = Saved::default();
    let (mut hwnd, mut vplace) = (0isize, Place::Dock);
    let mut in_browser = false;
    for l in lines {
        if in_browser {
            out.browser.push_str(l);
            continue;
        }
        let t = l.trim_end();
        if t == "browser" {
            in_browser = true;
        } else if let Some(v) = t.strip_prefix("hwnd\t") {
            hwnd = v.parse().unwrap_or(0);
        } else if let Some(v) = t.strip_prefix("place\t") {
            vplace = Place::from_key(v).unwrap_or(Place::Dock);
        } else if let Some(v) = t.strip_prefix("want\t") {
            out.place = Place::from_key(v);
        } else if let Some(v) = t.strip_prefix("transport\t") {
            out.transport = v.parse().ok().filter(|&h| h != 0).map(Hwnd);
        } else if let Some(v) = t.strip_prefix("perf\t") {
            out.perf = v == "1";
        }
    }
    out.view = (hwnd != 0).then_some((Hwnd(hwnd), vplace));
    Some(out)
}

impl App {
    /// La bande d'onglets des dockers. Quand un docker ne tient qu'une
    /// fenetre et qu'il est plus petit que `dock_mini_tab_size`
    /// (Preferences > Appearance, 200 px par defaut), REAPER remplace sa bande
    /// d'onglets par une poignee de 6 px. Cedric la veut toujours cachee : la
    /// taille passe a 4096 px a chaque chargement (REAPER ne l'enregistre pas).
    /// Mesure du 2026-11-22 : marge gauche du navigateur 15 px -> 0.
    fn docker_tabs(&self) {
        const SIZE: &str = "dock_mini_tab_size";
        const WANT: i32 = 4096;
        match self.r.config_int(SIZE) {
            Some(was) if was < WANT => {
                self.docker_margins();
                if self.r.set_config_int(SIZE, WANT) {
                    self.r.dock_refresh();
                    // Les dockers deja poses gardaient leur bande jusqu'au
                    // premier redimensionnement (le pixel que Cedric devait
                    // bouger au demarrage) : chacun refait sa mise en page.
                    self.docker_relayout();
                }
                self.host
                    .info(&format!("onglets des dockers : {SIZE} {was} -> {WANT}"));
            }
            Some(_) => {}
            None => self.host.warn("dock_mini_tab_size introuvable"),
        }
    }

    /// Chaque docker (ancre dans la fenetre principale ou flottant) recoit
    /// sa taille actuelle : il refait sa mise en page, la bande d'onglets
    /// avec.
    fn docker_relayout(&self) {
        // Un docker est la boite (#32770) qui tient les fenetres ancrees :
        // le parent de chaque fenetre que REAPER dit dans un docker.
        let mut hosts: Vec<Hwnd> = Vec::new();
        for h in self.docked_windows() {
            let p = ui::parent(h);
            if p.0 != 0 && !hosts.contains(&p) && ui::class_name(p) == "#32770" {
                hosts.push(p);
            }
        }
        for &h in &hosts {
            ui::resend_size(h);
        }
        self.host.info(&format!(
            "onglets des dockers : {} docker(s) remis en page",
            hosts.len()
        ));
        self.docker_margins();
    }

    /// Les fenetres dans un docker (de REAPER ou a nous), une fois chacune.
    fn docked_windows(&self) -> Vec<Hwnd> {
        let main = self.r.main_hwnd();
        let mut wins = ui::descendants(main);
        for t in ui::process_windows() {
            if t != main {
                wins.extend(ui::descendants(t));
            }
        }
        wins.retain(|&h| self.r.dock_of(h).is_some());
        wins.dedup();
        wins
    }

    /// Ce que chaque docker garde autour de nos vues ancrees (sa bande
    /// d'onglets) : a 0, la vue va jusqu'au bord.
    fn docker_margins(&self) {
        for h in self.docked_windows() {
            if ui::class_name(h) != "CPStudioWindow" {
                continue;
            }
            let p = ui::parent(h);
            let (v, r) = (ui::window_rect(h), ui::window_rect(p));
            self.host.info(&format!(
                "onglets des dockers : « {} » dans « {} » : marges gauche {}, droite {}, haut {}, bas {}",
                ui::window_text(h),
                ui::class_name(p),
                v.left - r.left,
                r.right - v.right,
                v.top - r.top,
                r.bottom - v.bottom
            ));
        }
    }
}

impl App {
    /// L'infobulle du navigateur : apres 0,6 s sur un bouton de la barre.
    fn browser_tip(&mut self) {
        let Some(v) = self.view.as_mut() else { return };
        let (x, y) = v.mouse;
        let now = if x < 0.0 {
            None
        } else {
            self.browser.tip_at(x, y)
        };
        match (&now, &v.tip_key) {
            (Some((t, _)), Some((k, _))) if t == k => {}
            _ => {
                v.tip_key = now
                    .as_ref()
                    .map(|(t, _)| (t.clone(), std::time::Instant::now()));
                if v.tip_shown {
                    if let Some(t) = &mut v.tip {
                        t.hide();
                    }
                    v.tip_shown = false;
                }
                return;
            }
        }
        let (Some((text, (rx, ry, _, rh))), Some((_, since))) = (now, &v.tip_key) else {
            return;
        };
        if v.tip_shown || since.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        if v.tip.is_none() {
            v.tip = ui::Tooltip::new(v.hwnd);
        }
        let (sx, sy) = ui::client_to_screen(v.hwnd, rx as i32, (ry + rh) as i32 + 6);
        if let Some(t) = &mut v.tip {
            t.show(&text.replace('\n', "\r\n"), sx, sy);
            v.tip_shown = true;
        }
    }

    fn browser_tip_hide(&mut self) {
        if let Some(v) = self.view.as_mut() {
            if v.tip_shown {
                if let Some(t) = &mut v.tip {
                    t.hide();
                }
            }
            v.tip_shown = false;
            v.tip_key = None;
        }
    }
}

impl Module for App {
    fn init(host: HostRef, state: &[u8]) -> Self {
        let r = host.reaper();
        let (_, mediadb) = read_shortcuts(&r);
        let places = Places {
            recordings: r.record_path(),
            folders: Places::parse_folders(
                &r.ext_get(SECTION, "browser_folders").unwrap_or_default(),
            ),
        };
        let favs: Vec<PathBuf> = r
            .ext_get(SECTION, "browser_favorites")
            .unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        let mut browser = Browser::new(places, &mediadb, favs);
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
        // La place du navigateur ; l'ancien reglage « ancre » s'y lit.
        let place = r
            .ext_get(SECTION, "browser_place")
            .and_then(|v| Place::from_key(&v))
            .unwrap_or(
                if r.ext_get(SECTION, "browser_docked").as_deref() == Some("1") {
                    Place::Dock
                } else {
                    Place::Float
                },
            );
        let cmd_browser =
            host.register_action("CPSTUDIO_BROWSER", "CP Studio: Browser (show/hide)");
        let cmd_perf = host.register_action(
            "CPSTUDIO_PERF",
            "CP Studio: Performance overlay (show/hide)",
        );
        let cmd_dock =
            host.register_action("CPSTUDIO_BROWSER_DOCK", "CP Studio: Browser - dock/undock");
        let cmd_state = host.register_action(
            "CPSTUDIO_BROWSER_STATE",
            "CP Studio: (test) Browser state to ExtState",
        );
        let cmd_transport = host.register_action(
            "CPSTUDIO_TRANSPORT",
            "CP Studio: Transport (replace REAPER's transport, on/off)",
        );
        let cmd_view_arrange =
            host.register_action("CPSTUDIO_VIEW_ARRANGE", "CP Studio: View - Arrange");
        let cmd_view_browser =
            host.register_action("CPSTUDIO_VIEW_BROWSER", "CP Studio: View - Browser");
        let cmd_place_arrange = host.register_action(
            "CPSTUDIO_BROWSER_IN_ARRANGE",
            "CP Studio: Browser - show in arrange view",
        );
        let cmd_place_dock = host.register_action(
            "CPSTUDIO_BROWSER_DOCKED",
            "CP Studio: Browser - dock in docker",
        );
        let cmd_place_float = host.register_action(
            "CPSTUDIO_BROWSER_FLOATING",
            "CP Studio: Browser - floating window",
        );
        let cmd_gtab = host.register_action(
            "CPSTUDIO_GUITARTAB",
            "CP Studio: Guitar Tab - open (transcribe the selected item)",
        );
        let cmd_view_gtab =
            host.register_action("CPSTUDIO_VIEW_GUITARTAB", "CP Studio: View - Guitar Tab");
        let cmd_gtab_set = host.register_action(
            "CPSTUDIO_GUITARTAB_SETTINGS",
            "CP Studio: Guitar Tab - settings",
        );
        let cmd_gtab_make = host.register_action(
            "CPSTUDIO_GUITARTAB_MAKE",
            "CP Studio: Guitar Tab - make the selected track a guitar track",
        );
        let cmd_editor = host.register_action(
            "CPSTUDIO_EDITOR",
            "CP Studio: Editor - open (the selected MIDI or audio item)",
        );
        let tp = tview::TPart::new(&r);
        // La clock externe retenue : le chargeur l'ecoute de nouveau.
        if tp.clock.dev >= 0 {
            host.clock_device(tp.clock.dev);
        }
        // Un module recharge repart sans saisie pas a pas.
        host.midi_listen(false);
        let mut app = App {
            host,
            r,
            cmd_browser,
            cmd_perf,
            cmd_dock,
            cmd_state,
            cmd_transport,
            cmd_view_arrange,
            cmd_view_browser,
            cmd_place_arrange,
            cmd_place_dock,
            cmd_place_float,
            cmd_gtab,
            cmd_gtab_set,
            cmd_view_gtab,
            cmd_gtab_make,
            cmd_editor,
            ed: editor::EPart::new(&r),
            gt: gtab::GPart::new(&r),
            browser,
            view: None,
            place,
            tp,
            bskin: None,
            blook: None,
            text: TextCache::new(Box::new(GdiGlyphs::new(&[
                "Segoe UI",
                "Consolas",
                "Segoe UI",
                "Segoe UI Symbol",
            ]))),
            face: String::new(),
            icons: IconCache::default(),
            scratch: Scratch::default(),
            action: None,
            ops: Vec::new(),
            ticks: 0,
            show_perf: false,
            main_keys: None,
            last_mods: Mods::default(),
        };
        // Les curseurs personnalises de REAPER (Cursors\<nom>.cur).
        if let Some(res) = app.r.resource_path() {
            ui::set_cursor_dir(&res);
        }
        match decode(state) {
            Some(sv) => {
                app.show_perf = sv.perf;
                app.browser.restore_state(&sv.browser);
                if let Some(p) = sv.place {
                    app.place = p;
                }
                if let Some((h, p)) = sv.view {
                    if win::is_window(h) {
                        app.adopt(h, p);
                        app.want_frames();
                    }
                }
                if let Some(h) = sv.transport {
                    if win::is_window(h) {
                        app.transport_adopt(h);
                    }
                }
            }
            None => {
                // Premier chargement de la session : rouvrir ce qui l'etait,
                // une fois REAPER construit (au battement).
                if app.ext("transport_on").as_deref() == Some("1") {
                    app.queue(WinOp::TransportOn);
                }
                if app.ext("browser_open").as_deref() == Some("1") {
                    app.queue(WinOp::Open);
                }
                if app.ext("gtab_open").as_deref() == Some("1") {
                    app.queue(WinOp::GtabOpen);
                }
                if app.ext("ed_open").as_deref() == Some("1") {
                    app.queue(WinOp::EdOpen);
                }
            }
        }
        // Guitar Tab : sa fenetre reprise apres un rechargement, et sa prise.
        let gh = app
            .ext("gtab_hwnd")
            .and_then(|v| v.parse::<isize>().ok())
            .map(Hwnd)
            .filter(|&h| h.0 != 0 && win::is_window(h));
        if let Some(h) = gh {
            app.gtab_adopt(h);
        }
        app.gt.pending = app.ext("gtab_take");
        // L'editeur : sa fenetre reprise, et son item.
        let eh = app
            .ext("ed_hwnd")
            .and_then(|v| v.parse::<isize>().ok())
            .map(Hwnd)
            .filter(|&h| h.0 != 0 && win::is_window(h));
        if let Some(h) = eh {
            app.ed.pending = app.ext("ed_item").filter(|g| !g.is_empty());
            app.ed_adopt(h);
        }
        host.info(&format!(
            "module pret : generation {}, navigateur {} ({}), transport {}",
            host.generation(),
            if app.view.is_some() {
                "ouvert"
            } else {
                "ferme"
            },
            app.place.key(),
            if app.tp.view.is_some() {
                "pose"
            } else {
                "absent"
            }
        ));
        app
    }

    fn shutdown(&mut self) {
        self.save_rect();
        self.host.store_state(&encode(self));
        let (hwnd, place) = self.adapter_parts();
        let mut ad = Adapter {
            host: self.host,
            r: self.r,
            hwnd,
            gtab: self.gt.win.as_ref().map(|w| w.hwnd),
            place,
            perf: self.show_perf,
            action: &mut self.action,
        };
        self.browser.release_all(&mut ad);
        if self.tp.prompt {
            self.r.prompt_for_action(-1);
        }
        self.gtab_shutdown();
        self.ed_shutdown();
        // Les fenetres restent au chargeur : le module suivant les reprend.
        // Elles ne recevront plus d'images d'ici la.
        if let Some(v) = &self.view {
            self.host.frames(v.hwnd, false);
        }
        if let Some(v) = &self.tp.view {
            self.host.frames(v.hwnd, false);
        }
    }

    fn tick(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        self.browser_tip();
        // Une demi-seconde de battements apres le chargement : REAPER a fini
        // de se construire (ses dockers compris).
        if !self.ops.is_empty() && self.ticks >= 15 {
            self.do_ops();
        }
        if self.ticks == 60 && crate::gtab::selftest() {
            self.docker_margins();
        }
        if self.ticks == 15 {
            self.docker_tabs();
        }
        self.reaper_look();
        self.pointer_tick();
        self.gtab_tick();
        self.ed_tick();
        self.transport_tick();
        if self.view.is_none() {
            return;
        }
        self.with_browser(|b, h| b.tick(h));
        // Un fichier enregistre apparait tout seul ; le dossier
        // d'enregistrement suit le projet ; le theme suit REAPER.
        self.browser.watch();
        if self.ticks % 30 == 0 {
            let rp = self.r.record_path();
            self.browser.set_recordings(rp.as_deref());
            self.browser_look();
        }
        let shown = self.view.as_ref().is_some_and(|v| ui::is_shown(v.hwnd));
        if !shown {
            return;
        }
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
                self.queue(WinOp::Close);
            } else {
                self.queue(WinOp::Open);
            }
            true
        } else if cmd == self.cmd_perf {
            self.show_perf = !self.show_perf;
            self.browser.mark();
            self.want_frames();
            true
        } else if cmd == self.cmd_state {
            // Pour l'auto-test : ce que le navigateur montre, lisible par
            // GetExtState (non persistant).
            let st = format!(
                "sel={} rows={} focus={} place={} band={}",
                self.browser.sel.map_or(-1, |s| s as i64),
                self.browser.rows.len(),
                if self.browser.focus == Focus::Search {
                    "search"
                } else {
                    "list"
                },
                self.place.key(),
                self.browser_in_band() as u8,
            );
            self.r.ext_set(SECTION, "selftest_state", &st, false);
            true
        } else if cmd == self.cmd_dock {
            self.queue(WinOp::ToggleDock);
            true
        } else if cmd == self.cmd_transport {
            self.queue(if self.tp.view.is_some() {
                WinOp::TransportOff
            } else {
                WinOp::TransportOn
            });
            true
        } else if cmd == self.cmd_view_arrange {
            self.queue(WinOp::ViewArrange);
            true
        } else if cmd == self.cmd_view_browser {
            self.queue(WinOp::ViewBrowser);
            true
        } else if cmd == self.cmd_place_arrange {
            self.queue(WinOp::Place(Place::Arrange));
            true
        } else if cmd == self.cmd_place_dock {
            self.queue(WinOp::Place(Place::Dock));
            true
        } else if cmd == self.cmd_place_float {
            self.queue(WinOp::Place(Place::Float));
            true
        } else if cmd == self.cmd_view_gtab {
            self.queue(WinOp::GtabOpen);
            true
        } else if cmd == self.cmd_gtab {
            self.gtab_start();
            true
        } else if cmd == self.cmd_gtab_set {
            self.gtab_edit_settings();
            true
        } else if cmd == self.cmd_gtab_make {
            self.gtab_make_selected();
            true
        } else if cmd == self.cmd_editor {
            self.ed_start();
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
            Some(self.place == Place::Dock)
        } else if cmd == self.cmd_transport {
            Some(self.tp.view.is_some())
        } else if cmd == self.cmd_view_browser {
            Some(self.browser_in_band())
        } else if cmd == self.cmd_view_arrange {
            Some(self.host.place_state(PLACE_ARRANGE).0 != PLACE_HELD)
        } else if cmd == self.cmd_place_arrange {
            Some(self.place == Place::Arrange)
        } else if cmd == self.cmd_place_dock {
            Some(self.place == Place::Dock)
        } else if cmd == self.cmd_place_float {
            Some(self.place == Place::Float)
        } else if cmd == self.cmd_gtab || cmd == self.cmd_view_gtab {
            Some(self.gt.win.is_some() || self.gtab_running())
        } else if cmd == self.cmd_editor {
            Some(self.ed.win.is_some())
        } else {
            None
        }
    }

    fn wnd_proc(&mut self, m: &WinMsg) -> Option<isize> {
        if let Some(r) = self.transport_wnd(m) {
            return Some(r);
        }
        if let Some(r) = self.gtab_wnd(m) {
            return Some(r);
        }
        if let Some(r) = self.ed_wnd(m) {
            return Some(r);
        }
        self.browser_wnd(m)
    }
}

cp_sys::export_module!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_etat_se_relit() {
        let bstate = "open\tC:\\x\nsel\tC:\\x\\a.wav\n";
        let s = format!(
            "{STATE_TAG}\nhwnd\t1234\nplace\tarrange\nwant\tarrange\ntransport\t99\nperf\t0\nbrowser\n{bstate}"
        );
        let sv = decode(s.as_bytes()).unwrap();
        assert_eq!(sv.view, Some((Hwnd(1234), Place::Arrange)));
        assert_eq!(sv.place, Some(Place::Arrange));
        assert_eq!(sv.transport, Some(Hwnd(99)));
        assert!(!sv.perf);
        assert_eq!(sv.browser, bstate);
        assert!(decode(b"autre chose\n").is_none());
    }
}
