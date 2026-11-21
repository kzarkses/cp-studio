//! Windows : la classe de fenetre du chargeur, la peinture GDI, l'horloge locale.
//!
//! Les declarations sont ecrites a la main (une vingtaine de fonctions stables
//! depuis trente ans) : aucune dependance, et une compilation instantanee.
//!
//! La peinture GDI est provisoire : elle sert a voir le rechargement a chaud
//! avant que le moteur de dessin soit porte.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::OnceLock;

type Raw = *mut c_void;
type WndProcFn = unsafe extern "system" fn(Raw, u32, usize, isize) -> isize;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: i32 = 0x8000_0000_u32 as i32;
const CS_VREDRAW: u32 = 0x0001;
const CS_HREDRAW: u32 = 0x0002;
const IDC_ARROW: usize = 32512;
const TRANSPARENT: i32 = 1;
const DEFAULT_GUI_FONT: i32 = 17;
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
const SW_SHOWNORMAL: i32 = 1;

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    wnd_proc: Option<WndProcFn>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Raw,
    icon: Raw,
    cursor: Raw,
    background: Raw,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: Raw,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
struct PaintStruct {
    hdc: Raw,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(wc: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Raw,
        menu: Raw,
        instance: Raw,
        param: Raw,
    ) -> Raw;
    fn DefWindowProcW(h: Raw, msg: u32, wp: usize, lp: isize) -> isize;
    fn DestroyWindow(h: Raw) -> i32;
    fn IsWindow(h: Raw) -> i32;
    fn ShowWindow(h: Raw, cmd: i32) -> i32;
    fn SetForegroundWindow(h: Raw) -> i32;
    fn InvalidateRect(h: Raw, r: *const Rect, erase: i32) -> i32;
    fn BeginPaint(h: Raw, ps: *mut PaintStruct) -> Raw;
    fn EndPaint(h: Raw, ps: *const PaintStruct) -> i32;
    fn GetClientRect(h: Raw, r: *mut Rect) -> i32;
    fn FillRect(dc: Raw, r: *const Rect, brush: Raw) -> i32;
    fn LoadCursorW(instance: Raw, name: *const u16) -> Raw;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Raw;
    fn DeleteObject(obj: Raw) -> i32;
    fn SetBkMode(dc: Raw, mode: i32) -> i32;
    fn SetTextColor(dc: Raw, color: u32) -> u32;
    fn TextOutW(dc: Raw, x: i32, y: i32, s: *const u16, n: i32) -> i32;
    fn SelectObject(dc: Raw, obj: Raw) -> Raw;
    fn GetStockObject(i: i32) -> Raw;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleFileNameW(module: Raw, buf: *mut u16, n: u32) -> u32;
    fn GetLocalTime(st: *mut SystemTime);
    fn GetLastError() -> u32;
}

/// Une fenetre. Un entier, donc `Send` : c'est un nom, pas une propriete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hwnd(pub isize);

impl Hwnd {
    fn raw(self) -> Raw {
        self.0 as Raw
    }
}

/// Un message de fenetre, tel que Windows le donne.
#[derive(Clone, Copy, Debug)]
pub struct WinMsg {
    pub hwnd: Hwnd,
    pub msg: u32,
    pub wp: usize,
    pub lp: isize,
}

impl WinMsg {
    /// Coordonnees souris d'un message WM_*BUTTON* / WM_MOUSEMOVE.
    pub fn mouse_xy(&self) -> (i32, i32) {
        let x = (self.lp & 0xFFFF) as i16 as i32;
        let y = ((self.lp >> 16) & 0xFFFF) as i16 as i32;
        (x, y)
    }
}

/// Une chaine terminee par un zero, en UTF-16.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

pub fn last_error() -> u32 {
    // SAFETY: aucune precondition.
    unsafe { GetLastError() }
}

// ---------------------------------------------------------------------------
// La classe de fenetre du chargeur
// ---------------------------------------------------------------------------

static HANDLER: OnceLock<fn(&WinMsg) -> isize> = OnceLock::new();
static CLASS: OnceLock<(isize, Vec<u16>)> = OnceLock::new();

unsafe extern "system" fn trampoline(h: Raw, msg: u32, wp: usize, lp: isize) -> isize {
    let m = WinMsg {
        hwnd: Hwnd(h as isize),
        msg,
        wp,
        lp,
    };
    if let Some(f) = HANDLER.get() {
        if let Some(r) = crate::guard(|| f(&m)) {
            return r;
        }
    }
    // SAFETY: les arguments viennent de Windows et lui sont rendus tels quels.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Enregistre LA classe de fenetre, une fois. `handler` recoit tous les messages
/// et rend le resultat (il appelle [`def_window_proc`] pour le comportement par
/// defaut). Le code de la procedure vit dans la DLL qui appelle ceci : seul le
/// chargeur, qui n'est jamais decharge, doit le faire.
pub fn register_class(instance: isize, handler: fn(&WinMsg) -> isize) -> Result<(), String> {
    if HANDLER.set(handler).is_err() {
        return Ok(());
    }
    let (inst, name) = CLASS.get_or_init(|| (instance, wide("CPStudioWindow")));
    // SAFETY: curseur systeme, aucune ressource a liberer.
    let cursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW as *const u16) };
    let wc = WndClassExW {
        cb_size: size_of::<WndClassExW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        wnd_proc: Some(trampoline),
        cls_extra: 0,
        wnd_extra: 0,
        instance: *inst as Raw,
        icon: null_mut(),
        cursor,
        background: null_mut(),
        menu_name: std::ptr::null(),
        class_name: name.as_ptr(),
        icon_small: null_mut(),
    };
    // SAFETY: `wc` est complet ; le nom de classe vit dans un static.
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 && last_error() != ERROR_CLASS_ALREADY_EXISTS {
        return Err(format!("RegisterClassExW a echoue ({})", last_error()));
    }
    Ok(())
}

/// Cree une fenetre de la classe du chargeur, possedee par `owner`.
pub fn create_window(title: &str, w: i32, h: i32, owner: Hwnd) -> Option<Hwnd> {
    let (inst, name) = CLASS.get()?;
    let t = wide(title);
    // SAFETY: classe enregistree, chaines terminees par zero et vivantes
    // pendant l'appel. Windows envoie des messages pendant la creation : ils
    // passent par `trampoline`, qui ne suppose rien.
    let h = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            t.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            w,
            h,
            owner.raw(),
            null_mut(),
            *inst as Raw,
            null_mut(),
        )
    };
    (!h.is_null()).then_some(Hwnd(h as isize))
}

pub fn def_window_proc(m: &WinMsg) -> isize {
    // SAFETY: le message vient de Windows (ou de lui-meme, recopie).
    unsafe { DefWindowProcW(m.hwnd.raw(), m.msg, m.wp, m.lp) }
}

pub fn destroy_window(h: Hwnd) {
    // SAFETY: sans effet sur une fenetre deja detruite.
    unsafe {
        DestroyWindow(h.raw());
    }
}

pub fn is_window(h: Hwnd) -> bool {
    // SAFETY: aucune precondition.
    unsafe { IsWindow(h.raw()) != 0 }
}

pub fn show(h: Hwnd) {
    // SAFETY: sans effet sur une fenetre invalide.
    unsafe {
        ShowWindow(h.raw(), SW_SHOWNORMAL);
        SetForegroundWindow(h.raw());
    }
}

/// Demande un nouveau WM_PAINT.
pub fn invalidate(h: Hwnd) {
    // SAFETY: rectangle nul = toute la fenetre ; sans effet si invalide.
    unsafe {
        InvalidateRect(h.raw(), std::ptr::null(), 0);
    }
}

pub fn client_size(h: Hwnd) -> (i32, i32) {
    let mut r = Rect::default();
    // SAFETY: `r` est un RECT valide en ecriture.
    unsafe {
        GetClientRect(h.raw(), &mut r);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Le chemin d'une DLL chargee (`None` = l'executable).
pub fn module_path(instance: isize) -> Option<PathBuf> {
    let mut buf = vec![0u16; 2048];
    // SAFETY: le tampon fait la taille annoncee.
    let n = unsafe { GetModuleFileNameW(instance as Raw, buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize >= buf.len() {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])))
}

/// L'heure locale : (heure, minute, seconde, milliseconde).
pub fn local_time() -> (u16, u16, u16, u16) {
    let mut st = SystemTime::default();
    // SAFETY: `st` est valide en ecriture.
    unsafe { GetLocalTime(&mut st) };
    (st.hour, st.minute, st.second, st.millis)
}

// ---------------------------------------------------------------------------
// Peinture GDI (provisoire)
// ---------------------------------------------------------------------------

/// Une passe de peinture : `BeginPaint` a la creation, `EndPaint` a la fin.
pub struct Paint {
    hwnd: Hwnd,
    ps: PaintStruct,
    old_font: Raw,
}

impl Paint {
    /// A appeler en reponse a WM_PAINT seulement.
    pub fn begin(hwnd: Hwnd) -> Option<Paint> {
        // SAFETY: PaintStruct ne contient que des entiers et des pointeurs :
        // des zeros sont une valeur valide, que BeginPaint remplit.
        let mut ps: PaintStruct = unsafe { std::mem::zeroed() };
        // SAFETY: `ps` est valide en ecriture.
        let dc = unsafe { BeginPaint(hwnd.raw(), &mut ps) };
        if dc.is_null() {
            return None;
        }
        // SAFETY: contexte de peinture valide jusqu'a EndPaint.
        let old_font = unsafe {
            SetBkMode(dc, TRANSPARENT);
            SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT))
        };
        Some(Paint { hwnd, ps, old_font })
    }

    pub fn size(&self) -> (i32, i32) {
        client_size(self.hwnd)
    }

    pub fn fill(&self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let r = Rect {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        // SAFETY: pinceau cree et detruit ici ; contexte valide.
        unsafe {
            let brush = CreateSolidBrush(color);
            FillRect(self.ps.hdc, &r, brush);
            DeleteObject(brush);
        }
    }

    pub fn text(&self, x: i32, y: i32, s: &str, color: u32) {
        let w: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: `w` vit pendant l'appel, longueur exacte.
        unsafe {
            SetTextColor(self.ps.hdc, color);
            TextOutW(self.ps.hdc, x, y, w.as_ptr(), w.len() as i32);
        }
    }
}

impl Drop for Paint {
    fn drop(&mut self) {
        // SAFETY: rend la police d'origine puis ferme la passe ouverte par
        // `begin`, avec la meme structure.
        unsafe {
            SelectObject(self.ps.hdc, self.old_font);
            EndPaint(self.hwnd.raw(), &self.ps);
        }
    }
}
