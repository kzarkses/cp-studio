//! Le mode sombre, la part SURE : les barres de titre (DWM, documente) et les
//! menus (le mode d'application de Windows). Rien dans le contenu des boites
//! (ni WM_CTLCOLOR, ni sous-classement) : c'est la que l'extension « Reaper
//! Dark Mode » faisait planter la boite de fichiers de Windows apres une mise
//! a jour (2026-11-21, plantage dans SHELL32.dll, mesure).

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use crate::guard;
use crate::win::{Hwnd, wide};

type Raw = *mut c_void;

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(h: Raw, attr: u32, value: *const c_void, size: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> Raw;
    fn GetProcAddress(m: Raw, name: *const u8) -> Raw;
    fn GetCurrentThreadId() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumThreadWindows(
        tid: u32,
        f: unsafe extern "system" fn(Raw, isize) -> i32,
        lp: isize,
    ) -> i32;
    fn SetWindowPos(h: Raw, after: Raw, x: i32, y: i32, w: i32, h2: i32, flags: u32) -> i32;
    fn GetWindowLongW(h: Raw, idx: i32) -> i32;
    fn SetWindowsHookExW(
        id: i32,
        f: unsafe extern "system" fn(i32, usize, isize) -> isize,
        m: Raw,
        tid: u32,
    ) -> Raw;
    fn UnhookWindowsHookEx(h: Raw) -> i32;
    fn CallNextHookEx(h: Raw, code: i32, wp: usize, lp: isize) -> isize;
}

/// La barre de titre d'une fenetre en sombre (ou en clair). Sans effet sur une
/// fenetre sans barre de titre.
pub fn set_title_bar(h: Hwnd, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_USE_IMMERSIVE_DARK_MODE_OLD: u32 = 19;
    const GWL_STYLE: i32 = -16;
    const WS_CAPTION: i32 = 0x00C0_0000;
    // SAFETY: lecture du style d'un handle (0 s'il est mort).
    let style = unsafe { GetWindowLongW(h.raw(), GWL_STYLE) };
    if style & WS_CAPTION != WS_CAPTION {
        return;
    }
    let v: i32 = dark as i32;
    // SAFETY: un BOOL de 4 octets ; DWM refuse proprement un handle mort.
    unsafe {
        if DwmSetWindowAttribute(
            h.raw(),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &v as *const i32 as *const c_void,
            4,
        ) < 0
        {
            DwmSetWindowAttribute(
                h.raw(),
                DWMWA_USE_IMMERSIVE_DARK_MODE_OLD,
                &v as *const i32 as *const c_void,
                4,
            );
        }
        // Redessiner le cadre tout de suite.
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOZORDER: u32 = 0x0004;
        const SWP_NOACTIVATE: u32 = 0x0010;
        const SWP_FRAMECHANGED: u32 = 0x0020;
        SetWindowPos(
            h.raw(),
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Le mode d'application (les menus et ce que Windows dessine lui-meme) :
/// `SetPreferredAppMode` (uxtheme, ordinal 135, Windows 10 1903 et apres),
/// puis `FlushMenuThemes` (136). Faux si ce Windows ne l'a pas.
pub fn set_app_mode(dark: bool) -> bool {
    let name = wide("uxtheme.dll");
    // SAFETY: uxtheme est une DLL du systeme, deja chargee par REAPER ; les
    // ordinaux 135 et 136 sont des fonctions (int) -> int et () -> void
    // depuis Windows 10 1903 (verifie par leur presence, pas par la version).
    unsafe {
        let m = LoadLibraryW(name.as_ptr());
        if m.is_null() {
            return false;
        }
        let set = GetProcAddress(m, 135usize as *const u8);
        let flush = GetProcAddress(m, 136usize as *const u8);
        if set.is_null() {
            return false;
        }
        let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set);
        set(if dark { 2 } else { 0 }); // ForceDark / Default
        if !flush.is_null() {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
    true
}

unsafe extern "system" fn collect(h: Raw, lp: isize) -> i32 {
    // SAFETY: `lp` est l'adresse du Vec passe par `thread_windows`.
    let v = unsafe { &mut *(lp as *mut Vec<Hwnd>) };
    v.push(Hwnd(h as isize));
    1
}

/// Les fenetres de premier niveau du fil courant.
pub fn thread_windows() -> Vec<Hwnd> {
    let mut v: Vec<Hwnd> = Vec::new();
    // SAFETY: le rappel ne vit que pendant l'appel et ecrit dans `v`.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            collect,
            &mut v as *mut Vec<Hwnd> as isize,
        )
    };
    v
}

static CBT_FN: OnceLock<fn(Hwnd)> = OnceLock::new();
static CBT_HOOK: Mutex<usize> = Mutex::new(0);

unsafe extern "system" fn cbt_proc(code: i32, wp: usize, lp: isize) -> isize {
    const HCBT_ACTIVATE: i32 = 5;
    if code == HCBT_ACTIVATE {
        if let Some(f) = CBT_FN.get() {
            let _ = guard(|| f(Hwnd(wp as isize)));
        }
    }
    // SAFETY: le premier argument est ignore depuis Windows NT.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
}

/// `f` voit chaque fenetre du fil courant au moment ou elle s'active (une
/// boite qui s'ouvre) : de quoi lui donner sa barre de titre. Le code doit
/// vivre dans une DLL jamais dechargee : le chargeur.
pub fn install_activate_hook(f: fn(Hwnd)) -> bool {
    let _ = CBT_FN.set(f);
    let mut g = CBT_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if *g != 0 {
        return true;
    }
    const WH_CBT: i32 = 5;
    // SAFETY: crochet du fil courant, procedure dans cette DLL.
    let h =
        unsafe { SetWindowsHookExW(WH_CBT, cbt_proc, std::ptr::null_mut(), GetCurrentThreadId()) };
    *g = h as usize;
    !h.is_null()
}

pub fn remove_activate_hook() {
    let mut g = CBT_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if *g != 0 {
        // SAFETY: le crochet pose par `install_activate_hook`.
        unsafe { UnhookWindowsHookEx(*g as Raw) };
        *g = 0;
    }
}
