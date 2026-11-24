//! REAPER cote interface : l'ancrage dans les dockers, le clavier (crochet
//! « accelerator »), l'etat des actions a bascule, les reglages persistants
//! (ExtState), et le crochet souris du fil principal.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::OnceLock;

use crate::guard;
use crate::reaper::Reaper;
use crate::win::Hwnd;

// ---------------------------------------------------------------------------
// L'ancrage
// ---------------------------------------------------------------------------

impl Reaper {
    /// Place la fenetre (enfant, `WS_CHILD`) dans un docker. `ident` est la cle
    /// sous laquelle REAPER retient sa place.
    pub fn dock_add(&self, h: Hwnd, name: &str, ident: &str) -> bool {
        type F = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, bool);
        // SAFETY: void DockWindowAddEx(HWND, const char*, const char*, bool).
        let Some(f) = (unsafe { self.api::<F>(c"DockWindowAddEx") }) else {
            return false;
        };
        let (Ok(n), Ok(i)) = (CString::new(name), CString::new(ident)) else {
            return false;
        };
        // SAFETY: chaines vivantes pendant l'appel ; REAPER les recopie.
        unsafe { f(h.0 as *mut c_void, n.as_ptr(), i.as_ptr(), true) };
        true
    }

    /// Retire la fenetre de son docker. A FAIRE AVANT DE LA DETRUIRE : dans
    /// l'autre sens REAPER garde une reference vers une fenetre morte.
    pub fn dock_remove(&self, h: Hwnd) {
        type F = unsafe extern "C" fn(*mut c_void);
        // SAFETY: void DockWindowRemove(HWND).
        if let Some(f) = unsafe { self.api::<F>(c"DockWindowRemove") } {
            // SAFETY: REAPER ignore une fenetre qu'il ne connait pas.
            unsafe { f(h.0 as *mut c_void) };
        }
    }

    /// Montre l'onglet de la fenetre dans son docker.
    pub fn dock_activate(&self, h: Hwnd) {
        type F = unsafe extern "C" fn(*mut c_void);
        // SAFETY: void DockWindowActivate(HWND).
        if let Some(f) = unsafe { self.api::<F>(c"DockWindowActivate") } {
            // SAFETY: idem.
            unsafe { f(h.0 as *mut c_void) };
        }
    }

    /// Le docker qui contient la fenetre (`None` : flottante).
    pub fn dock_of(&self, h: Hwnd) -> Option<i32> {
        type F = unsafe extern "C" fn(*mut c_void, *mut bool) -> i32;
        // SAFETY: int DockIsChildOfDock(HWND, bool* isFloatingDockerOut).
        let f = unsafe { self.api::<F>(c"DockIsChildOfDock") }?;
        let mut floating = false;
        // SAFETY: ecrit un bool.
        let d = unsafe { f(h.0 as *mut c_void, &mut floating) };
        (d >= 0).then_some(d)
    }

    /// La place d'un docker : 0 en bas, 1 a gauche, 2 en haut, 3 a droite,
    /// 4 flottant ; -1 s'il n'existe pas.
    pub fn dock_position(&self, which: i32) -> i32 {
        type F = unsafe extern "C" fn(i32) -> i32;
        // SAFETY: int DockGetPosition(int whichDock).
        match unsafe { self.api::<F>(c"DockGetPosition") } {
            // SAFETY: un entier, rien d'autre.
            Some(f) => unsafe { f(which) },
            None => -1,
        }
    }

    /// Le docker ou ira `ident` a sa prochaine ouverture.
    pub fn dock_set_target(&self, ident: &str, which: i32) {
        type F = unsafe extern "C" fn(*const c_char, i32);
        // SAFETY: void Dock_UpdateDockID(const char* ident_str, int whichDock).
        let f = unsafe { self.api::<F>(c"Dock_UpdateDockID") };
        if let (Some(f), Ok(i)) = (f, CString::new(ident)) {
            // SAFETY: chaine vivante pendant l'appel.
            unsafe { f(i.as_ptr(), which) };
        }
    }

    // -----------------------------------------------------------------------
    // Les reglages persistants
    // -----------------------------------------------------------------------

    pub fn ext_get(&self, section: &str, key: &str) -> Option<String> {
        type F = unsafe extern "C" fn(*const c_char, *const c_char) -> *const c_char;
        // SAFETY: const char* GetExtState(const char* section, const char* key).
        let f = unsafe { self.api::<F>(c"GetExtState") }?;
        let (s, k) = (CString::new(section).ok()?, CString::new(key).ok()?);
        // SAFETY: chaines vivantes ; le resultat est tenu par REAPER.
        let p = unsafe { f(s.as_ptr(), k.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: chaine terminee par zero.
        let v = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        (!v.is_empty()).then_some(v)
    }

    /// `persist` : ecrit dans reaper-extstate.ini (survit au redemarrage).
    pub fn ext_set(&self, section: &str, key: &str, value: &str, persist: bool) {
        type F = unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, bool);
        // SAFETY: void SetExtState(const char*, const char*, const char*, bool).
        let Some(f) = (unsafe { self.api::<F>(c"SetExtState") }) else {
            return;
        };
        let (Ok(s), Ok(k), Ok(v)) = (
            CString::new(section),
            CString::new(key),
            CString::new(value.replace('\0', " ")),
        ) else {
            return;
        };
        // SAFETY: chaines vivantes pendant l'appel.
        unsafe { f(s.as_ptr(), k.as_ptr(), v.as_ptr(), persist) };
    }

    /// Une preference entiere de REAPER (une cle de reaper.ini), lue dans sa
    /// memoire par `get_config_var`.
    pub fn config_int(&self, name: &str) -> Option<i32> {
        let p = self.config_var(name, 4)?;
        // SAFETY: REAPER annonce une variable de 4 octets a cette adresse.
        Some(unsafe { *(p as *const i32) })
    }

    /// Change une preference entiere de REAPER. Elle sera enregistree dans
    /// reaper.ini avec les autres.
    pub fn set_config_int(&self, name: &str, v: i32) -> bool {
        let Some(p) = self.config_var(name, 4) else {
            return false;
        };
        // SAFETY: meme variable de 4 octets, ecrite depuis le fil principal.
        unsafe { *(p as *mut i32) = v };
        true
    }

    fn config_var(&self, name: &str, size: i32) -> Option<*mut c_void> {
        type F = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
        // SAFETY: void* get_config_var(const char* name, int* szOut).
        let f = unsafe { self.api::<F>(c"get_config_var") }?;
        let c = CString::new(name).ok()?;
        let mut sz = 0;
        // SAFETY: chaine vivante, taille ecrite par REAPER.
        let p = unsafe { f(c.as_ptr(), &mut sz) };
        (!p.is_null() && sz == size).then_some(p)
    }

    /// Redispose tous les dockers (apres un changement de preference).
    pub fn dock_refresh(&self) {
        type F = unsafe extern "C" fn();
        // SAFETY: void DockWindowRefresh().
        if let Some(f) = unsafe { self.api::<F>(c"DockWindowRefresh") } {
            // SAFETY: sans argument.
            unsafe { f() };
        }
    }

    /// Une action de REAPER par son nom (« _RS... », « _SWS_... ») ou son
    /// numero ; 0 si inconnue.
    pub fn named_command(&self, name: &str) -> i32 {
        type F = unsafe extern "C" fn(*const c_char) -> i32;
        // SAFETY: int NamedCommandLookup(const char* command_name).
        let f = unsafe { self.api::<F>(c"NamedCommandLookup") };
        let (Some(f), Ok(c)) = (f, CString::new(name)) else {
            return 0;
        };
        // SAFETY: chaine vivante.
        unsafe { f(c.as_ptr()) }
    }
}

// ---------------------------------------------------------------------------
// Le clavier : le crochet « accelerator »
// ---------------------------------------------------------------------------

/// `MSG` de Windows.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Msg {
    pub hwnd: isize,
    pub message: u32,
    pub wp: usize,
    pub lp: isize,
    pub time: u32,
    pub pt_x: i32,
    pub pt_y: i32,
    pub private: u32,
}

#[repr(C)]
struct AccelReg {
    translate: unsafe extern "C" fn(*mut Msg, *mut AccelReg) -> i32,
    is_local: bool,
    user: *mut c_void,
}

/// Ce que rend le crochet clavier (voir `accelerator_register_t`).
pub mod accel {
    /// Pas pour nous : REAPER continue (ses raccourcis).
    pub const NOT_OURS: i32 = 0;
    /// Mange : la touche a ete traitee.
    pub const EAT: i32 = 1;
    /// A livrer normalement a la fenetre (qui recevra aussi WM_CHAR).
    pub const PASS_TO_WINDOW: i32 = -1;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
}

/// Livre soi-meme une touche a sa fenetre (WM_CHAR compris) : REAPER ne rend
/// pas ses touches a une fenetre posee a la place de ses propres panneaux
/// (une fille directe de sa fenetre principale). L'appelant mange ensuite le
/// message pour REAPER.
pub fn deliver(m: &Msg) {
    // SAFETY: une copie complete d'un MSG que Windows a mis en file.
    unsafe {
        TranslateMessage(m);
        DispatchMessageW(m);
    }
}

static ACCEL_FN: OnceLock<fn(&mut Msg) -> i32> = OnceLock::new();

unsafe extern "C" fn accel_trampoline(msg: *mut Msg, _: *mut AccelReg) -> i32 {
    if msg.is_null() {
        return accel::NOT_OURS;
    }
    // SAFETY: REAPER passe le MSG qu'il traite, modifiable (on peut le
    // readresser a une autre fenetre).
    let m = unsafe { &mut *msg };
    ACCEL_FN
        .get()
        .and_then(|f| guard(|| f(m)))
        .unwrap_or(accel::NOT_OURS)
}

static ACCEL_REG: OnceLock<usize> = OnceLock::new();

impl Reaper {
    /// Le crochet clavier : `f` voit CHAQUE touche avant REAPER. Un seul par
    /// processus.
    pub fn register_accelerator(&self, f: fn(&mut Msg) -> i32) -> bool {
        if ACCEL_FN.set(f).is_err() {
            return false;
        }
        let reg = Box::leak(Box::new(AccelReg {
            translate: accel_trampoline,
            is_local: true,
            user: std::ptr::null_mut(),
        }));
        let _ = ACCEL_REG.set(reg as *mut AccelReg as usize);
        self.register_raw("accelerator", reg as *mut AccelReg as *mut c_void)
    }

    pub fn unregister_accelerator(&self) {
        if let Some(&p) = ACCEL_REG.get() {
            self.register_raw("-accelerator", p as *mut c_void);
        }
    }

    // -----------------------------------------------------------------------
    // L'etat des actions a bascule (la coche des menus et des barres)
    // -----------------------------------------------------------------------

    /// `f(cmd)` : 1 allume, 0 eteint, -1 pas a nous.
    pub fn register_toggle_state(&self, f: fn(i32) -> i32) -> bool {
        if TOGGLE_FN.set(f).is_err() {
            return false;
        }
        self.register_raw(
            "toggleaction",
            toggle_trampoline as *const () as *mut c_void,
        )
    }

    pub fn unregister_toggle_state(&self) {
        self.register_raw(
            "-toggleaction",
            toggle_trampoline as *const () as *mut c_void,
        );
    }
}

static TOGGLE_FN: OnceLock<fn(i32) -> i32> = OnceLock::new();

extern "C" fn toggle_trampoline(cmd: i32) -> i32 {
    TOGGLE_FN.get().and_then(|f| guard(|| f(cmd))).unwrap_or(-1)
}

// ---------------------------------------------------------------------------
// Le crochet souris du fil principal (« le dernier clic l'emporte »)
// ---------------------------------------------------------------------------

#[repr(C)]
struct MouseHookStruct {
    pt_x: i32,
    pt_y: i32,
    hwnd: *mut c_void,
    hit: u32,
    extra: usize,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        id: i32,
        f: unsafe extern "system" fn(i32, usize, isize) -> isize,
        m: *mut c_void,
        tid: u32,
    ) -> *mut c_void;
    fn UnhookWindowsHookEx(h: *mut c_void) -> i32;
    fn CallNextHookEx(h: *mut c_void, code: i32, wp: usize, lp: isize) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

/// (fenetre, message, x, y ecran) -> pris.
type MouseFn = fn(Hwnd, u32, i32, i32) -> bool;

static MOUSE_FN: OnceLock<MouseFn> = OnceLock::new();
static MOUSE_HOOK: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

unsafe extern "system" fn mouse_proc(code: i32, wp: usize, lp: isize) -> isize {
    if code >= 0 && lp != 0 {
        // SAFETY: pour WH_MOUSE, lParam pointe un MOUSEHOOKSTRUCT.
        let m = unsafe { &*(lp as *const MouseHookStruct) };
        if let Some(f) = MOUSE_FN.get() {
            // Vrai : le message est pris, la fenetre ne le recoit pas.
            if guard(|| f(Hwnd(m.hwnd as isize), wp as u32, m.pt_x, m.pt_y)).unwrap_or(false) {
                return 1;
            }
        }
    }
    // SAFETY: le premier argument est ignore depuis Windows NT.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
}

/// Voit chaque message souris du fil appelant (le fil principal de REAPER)
/// avant sa fenetre : (fenetre, message, point ecran). `f` rend vrai pour
/// prendre le message (la fenetre ne le recoit pas). Le code du crochet doit
/// vivre dans une DLL jamais dechargee : le chargeur.
pub fn install_mouse_hook(f: MouseFn) -> bool {
    let _ = MOUSE_FN.set(f);
    let mut g = MOUSE_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if *g != 0 {
        return true;
    }
    const WH_MOUSE: i32 = 7;
    // SAFETY: crochet du fil courant (module nul autorise pour un crochet de
    // fil), procedure dans cette DLL.
    let h = unsafe {
        SetWindowsHookExW(
            WH_MOUSE,
            mouse_proc,
            std::ptr::null_mut(),
            GetCurrentThreadId(),
        )
    };
    *g = h as usize;
    !h.is_null()
}

pub fn remove_mouse_hook() {
    let mut g = MOUSE_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if *g != 0 {
        // SAFETY: le crochet pose ci-dessus.
        unsafe { UnhookWindowsHookEx(*g as *mut c_void) };
        *g = 0;
    }
}
