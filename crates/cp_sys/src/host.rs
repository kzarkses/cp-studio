//! Cote chargeur : la table [`HostApi`] offerte au module.
//!
//! Le chargeur ecrit un `impl Host` en Rust sur ; ce module fabrique les
//! fonctions `extern "C"` qui le servent au module.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;

use cp_abi::{ABI, HostApi};

use crate::guard;

/// Ce que le chargeur sait faire pour le module. Tout est appele sur le fil
/// principal, parfois en REENTRANCE (pendant un appel du chargeur vers le
/// module) : une implementation ne tient jamais un verrou pendant qu'elle
/// rappelle le module.
pub trait Host: Send + Sync {
    fn log(&self, level: u32, msg: &str);
    fn get_func(&self, name: &CStr) -> *mut c_void;
    fn register_action(&self, id: &str, name: &str) -> i32;
    fn create_window(&self, kind: u32, title: &str, w: i32, h: i32) -> isize;
    fn store_state(&self, data: &[u8]);
    fn generation(&self) -> u32;
}

static IMPL: OnceLock<Box<dyn Host>> = OnceLock::new();

static API: HostApi = HostApi {
    abi: ABI,
    log: t_log,
    get_func: t_get_func,
    register_action: t_register_action,
    create_window: t_create_window,
    store_state: t_store_state,
    generation: t_generation,
};

/// Installe l'implementation (une seule par processus ; un second appel est
/// ignore) et rend la table a donner au module.
pub fn install(h: Box<dyn Host>) -> &'static HostApi {
    let _ = IMPL.set(h);
    &API
}

/// # Safety
/// `p` est nul, ou pointe sur `n` octets lisibles pendant `'a`.
unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: garanti par l'appelant.
        unsafe { std::slice::from_raw_parts(p, n) }
    }
}

extern "C" fn t_log(level: u32, msg: *const u8, len: usize) {
    // SAFETY: le module passe une tranche valide pendant l'appel.
    let m = unsafe { bytes(msg, len) };
    let _ = guard(|| {
        if let Some(h) = IMPL.get() {
            h.log(level, &String::from_utf8_lossy(m));
        }
    });
}

extern "C" fn t_get_func(name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return null_mut();
    }
    // SAFETY: le module passe une chaine terminee par zero.
    let n = unsafe { CStr::from_ptr(name) };
    guard(|| IMPL.get().map(|h| h.get_func(n)))
        .flatten()
        .unwrap_or(null_mut())
}

extern "C" fn t_register_action(
    id: *const u8,
    id_len: usize,
    name: *const u8,
    name_len: usize,
) -> i32 {
    // SAFETY: tranches valides pendant l'appel.
    let (i, n) = unsafe { (bytes(id, id_len), bytes(name, name_len)) };
    guard(|| {
        IMPL.get().map_or(0, |h| {
            h.register_action(&String::from_utf8_lossy(i), &String::from_utf8_lossy(n))
        })
    })
    .unwrap_or(0)
}

extern "C" fn t_create_window(kind: u32, title: *const u8, len: usize, w: i32, h: i32) -> isize {
    // SAFETY: tranche valide pendant l'appel.
    let t = unsafe { bytes(title, len) };
    guard(|| {
        IMPL.get().map_or(0, |x| {
            x.create_window(kind, &String::from_utf8_lossy(t), w, h)
        })
    })
    .unwrap_or(0)
}

extern "C" fn t_store_state(data: *const u8, len: usize) {
    // SAFETY: tranche valide pendant l'appel (l'hote la recopie).
    let d = unsafe { bytes(data, len) };
    let _ = guard(|| {
        if let Some(h) = IMPL.get() {
            h.store_state(d);
        }
    });
}

extern "C" fn t_generation() -> u32 {
    guard(|| IMPL.get().map_or(0, |h| h.generation())).unwrap_or(0)
}
