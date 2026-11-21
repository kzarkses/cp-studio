//! REAPER : le point d'entree de l'extension, `Register` et `GetFunc`.
//!
//! D'apres `reaper_plugin.h` du SDK (C:\Users\Cedric\dev\reaper-sdk\sdk).
//! Seules les fonctions dont on a besoin sont declarees ; chacune est obtenue par
//! `GetFunc` et verifiee non nulle avant d'etre appelee.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::OnceLock;

use crate::guard;
use crate::win::Hwnd;

/// `REAPER_PLUGIN_VERSION` du SDK.
pub const PLUGIN_VERSION: i32 = 0x20E;

type RegisterFn = unsafe extern "C" fn(name: *const c_char, info: *mut c_void) -> i32;
type GetFuncFn = unsafe extern "C" fn(name: *const c_char) -> *mut c_void;

/// `reaper_plugin_info_t`.
#[repr(C)]
struct RawInfo {
    caller_version: i32,
    hwnd_main: *mut c_void,
    register: Option<RegisterFn>,
    get_func: Option<GetFuncFn>,
}

/// `custom_action_register_t`.
#[repr(C)]
struct CustomAction {
    section: i32,
    id: *const c_char,
    name: *const c_char,
    extra: *mut c_void,
}

/// Les deux fonctions que REAPER donne a l'extension. Copiable : ce ne sont que
/// des pointeurs vers REAPER, valides tant que REAPER tourne.
#[derive(Clone, Copy)]
pub struct Reaper {
    register: RegisterFn,
    get_func: GetFuncFn,
    main: isize,
}

/// Corps de `ReaperPluginEntry` (voir [`export_reaper_plugin!`]).
///
/// # Safety
/// `rec` est nul (dechargement) ou pointe sur le `reaper_plugin_info_t` que
/// REAPER passe a l'extension.
#[doc(hidden)]
pub unsafe fn plugin_entry(
    instance: isize,
    rec: *const c_void,
    load: fn(isize, Reaper) -> bool,
    unload: fn(),
) -> i32 {
    if rec.is_null() {
        let _ = guard(unload);
        return 0;
    }
    // SAFETY: garanti par l'appelant (REAPER).
    let raw = unsafe { &*(rec as *const RawInfo) };
    let (Some(register), Some(get_func)) = (raw.register, raw.get_func) else {
        return 0;
    };
    if raw.caller_version != PLUGIN_VERSION {
        return 0;
    }
    let r = Reaper {
        register,
        get_func,
        main: raw.hwnd_main as isize,
    };
    match guard(|| load(instance, r)) {
        Some(true) => 1,
        _ => 0,
    }
}

/// Exporte `ReaperPluginEntry`. `load(hinstance, reaper) -> bool` a l'ouverture,
/// `unload()` a la fermeture de REAPER.
#[macro_export]
macro_rules! export_reaper_plugin {
    ($load:path, $unload:path) => {
        // Le pointeur brut est la signature imposee par REAPER : c'est lui
        // qui appelle, jamais notre code.
        #[allow(unsafe_code, non_snake_case, clippy::not_unsafe_ptr_arg_deref)]
        #[unsafe(no_mangle)]
        pub extern "C" fn ReaperPluginEntry(
            instance: *mut ::core::ffi::c_void,
            rec: *const ::core::ffi::c_void,
        ) -> i32 {
            // SAFETY: c'est REAPER qui appelle, avec son reaper_plugin_info_t
            // (ou nul au dechargement).
            unsafe { $crate::reaper::plugin_entry(instance as isize, rec, $load, $unload) }
        }
    };
}

static TIMER: OnceLock<fn()> = OnceLock::new();
static COMMAND: OnceLock<fn(i32) -> bool> = OnceLock::new();

extern "C" fn timer_trampoline() {
    if let Some(f) = TIMER.get() {
        let _ = guard(f);
    }
}

extern "C" fn hook_trampoline(
    _section: *mut c_void,
    cmd: i32,
    _val: i32,
    _val2: i32,
    _relmode: i32,
    _hwnd: *mut c_void,
) -> bool {
    COMMAND
        .get()
        .and_then(|f| guard(|| f(cmd)))
        .unwrap_or(false)
}

fn fn_ptr(f: *const ()) -> *mut c_void {
    f as *mut c_void
}

impl Reaper {
    fn reg(&self, name: &str, info: *mut c_void) -> i32 {
        let Ok(c) = CString::new(name) else { return 0 };
        // SAFETY: `Register` de REAPER ; nom termine par zero, `info` du type
        // que ce nom attend (garanti par les appelants de ce module).
        unsafe { (self.register)(c.as_ptr(), info) }
    }

    pub fn main_hwnd(&self) -> Hwnd {
        Hwnd(self.main)
    }

    /// `GetFunc` : un pointeur vers une fonction de REAPER, ou nul. Le
    /// pointeur n'est pas dereference ici.
    pub fn get_func(&self, name: &CStr) -> *mut c_void {
        // SAFETY: `GetFunc` de REAPER, nom termine par zero.
        unsafe { (self.get_func)(name.as_ptr()) }
    }

    pub fn show_console_msg(&self, msg: &str) {
        let p = self.get_func(c"ShowConsoleMsg");
        if p.is_null() {
            return;
        }
        // SAFETY: signature de reaper_plugin_functions.h :
        // void ShowConsoleMsg(const char* msg).
        let f: unsafe extern "C" fn(*const c_char) = unsafe { std::mem::transmute(p) };
        if let Ok(c) = CString::new(msg.replace('\0', " ")) {
            // SAFETY: chaine valide pendant l'appel.
            unsafe { f(c.as_ptr()) }
        }
    }

    /// Le dossier de ressources de REAPER (celui qui contient UserPlugins).
    pub fn resource_path(&self) -> Option<PathBuf> {
        let p = self.get_func(c"GetResourcePath");
        if p.is_null() {
            return None;
        }
        // SAFETY: const char* GetResourcePath() ; chaine UTF-8 tenue par REAPER.
        let f: unsafe extern "C" fn() -> *const c_char = unsafe { std::mem::transmute(p) };
        // SAFETY: appel sans argument.
        let s = unsafe { f() };
        if s.is_null() {
            return None;
        }
        // SAFETY: chaine terminee par zero, tenue par REAPER.
        let s = unsafe { CStr::from_ptr(s) };
        Some(PathBuf::from(s.to_string_lossy().into_owned()))
    }

    /// Battement du fil principal (~30 Hz). Un seul par processus.
    pub fn register_timer(&self, f: fn()) -> bool {
        let _ = TIMER.set(f);
        self.reg("timer", fn_ptr(timer_trampoline as *const ())) == 1
    }

    pub fn unregister_timer(&self) {
        self.reg("-timer", fn_ptr(timer_trampoline as *const ()));
    }

    /// `hookcommand2` : `f(cmd)` rend vrai s'il a traite l'action.
    pub fn register_command_hook(&self, f: fn(i32) -> bool) -> bool {
        let _ = COMMAND.set(f);
        self.reg("hookcommand2", fn_ptr(hook_trampoline as *const ())) == 1
    }

    pub fn unregister_command_hook(&self) {
        self.reg("-hookcommand2", fn_ptr(hook_trampoline as *const ()));
    }

    /// Une action dans la liste principale. Rend son numero de commande (0 =
    /// echec). La structure est gardee pour la vie du processus.
    pub fn register_action(&self, id: &str, name: &str) -> i32 {
        let (Ok(id), Ok(name)) = (CString::new(id), CString::new(name)) else {
            return 0;
        };
        let ca = Box::leak(Box::new(CustomAction {
            section: 0,
            id: id.into_raw(),
            name: name.into_raw(),
            extra: null_mut(),
        }));
        self.reg("custom_action", ca as *mut CustomAction as *mut c_void)
    }
}

impl Reaper {
    /// La boite d'ouverture de fichier de REAPER (modale). `None` si annulee.
    pub fn ask_file_to_read(&self, title: &str, ext: &str) -> Option<String> {
        type F = unsafe extern "C" fn(*mut c_char, *const c_char, *const c_char) -> bool;
        let p = self.get_func(c"GetUserFileNameForRead");
        if p.is_null() {
            return None;
        }
        // SAFETY: bool GetUserFileNameForRead(char* filenameNeed4096,
        // const char* title, const char* defext).
        let f: F = unsafe { std::mem::transmute(p) };
        let (t, e) = (CString::new(title).ok()?, CString::new(ext).ok()?);
        let mut buf = vec![0u8; 4096];
        // SAFETY: tampon de 4096 octets, comme le demande l'API.
        if !unsafe { f(buf.as_mut_ptr() as *mut c_char, t.as_ptr(), e.as_ptr()) } {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }
}
