//! Cote module : le trait [`Module`], la table [`ModuleApi`] et la macro
//! [`export_module!`].
//!
//! Chaque copie chargee du module a SES statics (ceux de ce fichier compris) :
//! un rechargement repart de zero, et seul l'etat range par `shutdown` passe a
//! la generation suivante.
//!
//! Regles pour un module, a cause du dechargement :
//! - ne jamais lancer de fil qui survive a `shutdown` (son code disparaitrait
//!   sous lui) ;
//! - ne jamais confier a REAPER ou a Windows un pointeur vers son propre code :
//!   les actions, le battement et les fenetres passent par le chargeur.

use std::ffi::{CStr, c_void};
use std::sync::{Mutex, OnceLock, TryLockError};

use cp_abi::{
    ABI, BUSY, ClipInfo, FAULT, FrameInfo, HostApi, LOAD_PENDING, LOAD_READY, LOG_ERROR, LOG_INFO,
    LOG_WARN, ModuleApi, OK, ViewSpec,
};

pub use crate::host::AudioLoad;

use crate::guard;
use crate::win::{Hwnd, WinMsg};

/// Ce qu'un module rechargeable implemente.
pub trait Module: Send + 'static {
    /// `state` = ce que la generation precedente a range (vide au premier
    /// chargement).
    fn init(host: HostRef, state: &[u8]) -> Self
    where
        Self: Sized;
    /// Ranger son etat (`HostRef::store_state`) avant d'etre decharge.
    fn shutdown(&mut self) {}
    fn tick(&mut self) {}
    fn on_action(&mut self, _cmd: i32) -> bool {
        false
    }
    /// `Some(resultat)` si le message est traite.
    fn wnd_proc(&mut self, _msg: &WinMsg) -> Option<isize> {
        None
    }
    /// L'etat d'une action a bascule (`None` : pas d'etat).
    fn toggle_state(&mut self, _cmd: i32) -> Option<bool> {
        None
    }
}

/// L'hote vu du module : des methodes sures autour de [`HostApi`].
#[derive(Clone, Copy)]
pub struct HostRef(&'static HostApi);

impl HostRef {
    pub fn log(&self, level: u32, msg: &str) {
        (self.0.log)(level, msg.as_ptr(), msg.len());
    }
    pub fn info(&self, msg: &str) {
        self.log(LOG_INFO, msg);
    }
    pub fn warn(&self, msg: &str) {
        self.log(LOG_WARN, msg);
    }
    pub fn error(&self, msg: &str) {
        self.log(LOG_ERROR, msg);
    }
    /// `GetFunc` de REAPER (nul si absente).
    pub fn get_func(&self, name: &CStr) -> *mut c_void {
        (self.0.get_func)(name.as_ptr())
    }
    pub fn register_action(&self, id: &str, name: &str) -> i32 {
        (self.0.register_action)(id.as_ptr(), id.len(), name.as_ptr(), name.len())
    }
    pub fn create_window(&self, kind: u32, title: &str, w: i32, h: i32) -> Option<Hwnd> {
        let r = (self.0.create_window)(kind, title.as_ptr(), title.len(), w, h);
        (r != 0).then_some(Hwnd(r))
    }
    pub fn store_state(&self, data: &[u8]) {
        (self.0.store_state)(data.as_ptr(), data.len());
    }
    pub fn generation(&self) -> u32 {
        (self.0.generation)()
    }

    /// L'API de REAPER, par la table du chargeur (sans `Register`).
    pub fn reaper(&self) -> crate::reaper::Reaper {
        crate::reaper::Reaper::from_get_func(self.0.get_func)
    }

    // --- le moteur audio (il vit dans le chargeur)

    /// Ouvre et decode un fichier sur un fil de travail. Rend un ticket.
    pub fn audio_load(&self, path: &str) -> u32 {
        (self.0.audio.load)(path.as_ptr(), path.len())
    }
    pub fn audio_poll(&self, ticket: u32) -> AudioLoad {
        let mut clip = 0u32;
        let mut err = [0u8; 256];
        let mut n = 0usize;
        match (self.0.audio.poll)(ticket, &mut clip, err.as_mut_ptr(), err.len(), &mut n) {
            LOAD_PENDING => AudioLoad::Pending,
            LOAD_READY => AudioLoad::Ready(clip),
            _ => AudioLoad::Failed(String::from_utf8_lossy(&err[..n.min(err.len())]).into_owned()),
        }
    }
    pub fn audio_info(&self, clip: u32) -> Option<ClipInfo> {
        let mut i = ClipInfo::default();
        ((self.0.audio.info)(clip, &mut i) == 1).then_some(i)
    }
    pub fn audio_release(&self, clip: u32) {
        (self.0.audio.release)(clip)
    }
    /// Min et max par pixel ; `ch = None` fond les canaux.
    pub fn audio_peaks(
        &self,
        clip: u32,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        min: &mut [f32],
        max: &mut [f32],
    ) -> usize {
        let n = min.len().min(max.len());
        let c = ch.map_or(-1, |c| c as i32);
        (self.0.audio.peaks)(clip, c, start, fpp, n, min.as_mut_ptr(), max.as_mut_ptr())
    }
    /// Rend la voix (0 = refuse).
    pub fn audio_play(
        &self,
        port: u32,
        clip: u32,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
    ) -> u32 {
        (self.0.audio.play)(port, clip, start, end, looped as i32, gain)
    }
    pub fn audio_stop(&self, voice: u32, fade_s: f64) {
        (self.0.audio.stop)(voice, fade_s)
    }
    /// (position en frames du clip, etat) ; `None` si la voix n'existe plus.
    pub fn audio_voice(&self, voice: u32) -> Option<(f64, i32)> {
        let mut p = 0.0;
        let st = (self.0.audio.voice)(voice, &mut p);
        (st >= 0).then_some((p, st))
    }
    pub fn audio_set(&self, voice: u32, param: u32, value: f64) -> bool {
        (self.0.audio.set)(voice, param, value) == 1
    }
    pub fn audio_srate(&self) -> f64 {
        (self.0.audio.srate)()
    }

    // --- les vues

    /// Une vue flottante ou ancree (`ident` : la cle du docker chez REAPER).
    #[allow(clippy::too_many_arguments)]
    pub fn create_view(
        &self,
        title: &str,
        ident: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        docked: bool,
    ) -> Option<Hwnd> {
        let spec = ViewSpec {
            title: title.as_ptr(),
            title_len: title.len(),
            ident: ident.as_ptr(),
            ident_len: ident.len(),
            x,
            y,
            w,
            h,
            docked: docked as i32,
        };
        let r = (self.0.create_view)(&spec);
        (r != 0).then_some(Hwnd(r))
    }
    /// Retire du docker puis detruit.
    pub fn destroy_window(&self, h: Hwnd) {
        (self.0.destroy_window)(h.0)
    }
    /// Un WM_CP_FRAME a chaque image de l'ecran, ou plus.
    pub fn frames(&self, h: Hwnd, on: bool) {
        (self.0.frames)(h.0, on as i32)
    }
    pub fn frame_info(&self) -> FrameInfo {
        let mut f = FrameInfo::default();
        (self.0.frame_info)(&mut f);
        f
    }
}

type Ctor = fn(HostRef, &[u8]) -> Box<dyn Module>;

static INSTANCE: Mutex<Option<Box<dyn Module>>> = Mutex::new(None);
static CTOR: OnceLock<Ctor> = OnceLock::new();
static HOST: OnceLock<HostRef> = OnceLock::new();

static API: ModuleApi = ModuleApi {
    abi: ABI,
    init: t_init,
    shutdown: t_shutdown,
    tick: t_tick,
    on_action: t_on_action,
    wnd_proc: t_wnd_proc,
    toggle_state: t_toggle_state,
};

fn ctor<T: Module>(h: HostRef, s: &[u8]) -> Box<dyn Module> {
    Box::new(T::init(h, s))
}

/// Corps de `cp_module_entry` (voir [`export_module!`]).
#[doc(hidden)]
pub fn entry<T: Module>(abi: u32) -> *const ModuleApi {
    if abi != ABI {
        return std::ptr::null();
    }
    let _ = CTOR.set(ctor::<T>);
    &API
}

/// Exporte `cp_module_entry` pour le type `$t: Module`.
#[macro_export]
macro_rules! export_module {
    ($t:ty) => {
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn cp_module_entry(abi: u32) -> *const $crate::abi::ModuleApi {
            $crate::module::entry::<$t>(abi)
        }
    };
}

/// Appelle le module sans jamais attendre : un appel reentrant rend BUSY au
/// lieu de se bloquer sur lui-meme.
fn with_instance(f: impl FnOnce(&mut dyn Module) -> i32) -> i32 {
    let mut g = match INSTANCE.try_lock() {
        Ok(g) => g,
        Err(TryLockError::WouldBlock) => return BUSY,
        Err(TryLockError::Poisoned(_)) => return FAULT,
    };
    let Some(m) = g.as_mut() else { return OK };
    guard(|| f(m.as_mut())).unwrap_or(FAULT)
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(h) = HOST.get() {
            h.error(&format!("panique dans le module : {info}"));
        }
    }));
}

extern "C" fn t_init(host: *const HostApi, state: *const u8, len: usize) -> i32 {
    if host.is_null() {
        return FAULT;
    }
    // SAFETY: le chargeur passe sa table, qui vit aussi longtemps que REAPER.
    let h = HostRef(unsafe { &*host });
    let _ = HOST.set(h);
    install_panic_hook();
    let st: &[u8] = if state.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: le chargeur passe une tranche valide pendant l'appel.
        unsafe { std::slice::from_raw_parts(state, len) }
    };
    let Some(make) = CTOR.get() else { return FAULT };
    let Some(m) = guard(|| make(h, st)) else {
        return FAULT;
    };
    match INSTANCE.try_lock() {
        Ok(mut g) => {
            *g = Some(m);
            OK
        }
        Err(_) => FAULT,
    }
}

extern "C" fn t_shutdown() -> i32 {
    let taken = match INSTANCE.try_lock() {
        Ok(mut g) => g.take(),
        Err(TryLockError::WouldBlock) => return BUSY,
        Err(TryLockError::Poisoned(_)) => return FAULT,
    };
    let Some(mut m) = taken else { return OK };
    match guard(move || {
        m.shutdown();
        drop(m);
    }) {
        Some(()) => OK,
        None => FAULT,
    }
}

extern "C" fn t_tick() -> i32 {
    with_instance(|m| {
        m.tick();
        OK
    })
}

extern "C" fn t_on_action(cmd: i32) -> i32 {
    with_instance(|m| m.on_action(cmd) as i32)
}

extern "C" fn t_wnd_proc(hwnd: isize, msg: u32, wp: usize, lp: isize, out: *mut isize) -> i32 {
    let m = WinMsg {
        hwnd: Hwnd(hwnd),
        msg,
        wp,
        lp,
    };
    with_instance(|x| match x.wnd_proc(&m) {
        Some(r) => {
            if !out.is_null() {
                // SAFETY: le chargeur passe l'adresse d'un isize a lui.
                unsafe { *out = r };
            }
            1
        }
        None => 0,
    })
}

extern "C" fn t_toggle_state(cmd: i32) -> i32 {
    match with_instance(|m| match m.toggle_state(cmd) {
        Some(true) => 1,
        Some(false) => 0,
        None => 2,
    }) {
        0 => 0,
        1 => 1,
        _ => -1,
    }
}

/// La touche d'un WM_CP_ACCEL (le chargeur passe son adresse dans `lp`).
pub fn key_event(m: &WinMsg) -> Option<cp_abi::KeyEvent> {
    if m.msg != cp_abi::WM_CP_ACCEL || m.lp == 0 {
        return None;
    }
    // SAFETY: pour WM_CP_ACCEL, le chargeur passe l'adresse d'un KeyEvent a
    // lui, valide pendant l'appel ; on le copie.
    Some(unsafe { *(m.lp as *const cp_abi::KeyEvent) })
}
