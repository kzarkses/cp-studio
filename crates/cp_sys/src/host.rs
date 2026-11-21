//! Cote chargeur : la table [`HostApi`] offerte au module.
//!
//! Le chargeur ecrit un `impl Host` en Rust sur ; ce module fabrique les
//! fonctions `extern "C"` qui le servent au module.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;

use cp_abi::{
    ABI, AudioApi, ClipInfo, FrameInfo, HostApi, LOAD_FAILED, LOAD_PENDING, LOAD_READY, ViewSpec,
};

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

    // --- le moteur audio (par defaut : absent)
    fn audio_load(&self, _path: &str) -> u32 {
        0
    }
    fn audio_poll(&self, _ticket: u32) -> AudioLoad {
        AudioLoad::Failed("no audio engine".into())
    }
    fn audio_info(&self, _clip: u32) -> Option<ClipInfo> {
        None
    }
    fn audio_release(&self, _clip: u32) {}
    #[allow(clippy::too_many_arguments)]
    fn audio_peaks(
        &self,
        _clip: u32,
        _ch: Option<usize>,
        _start: f64,
        _fpp: f64,
        _min: &mut [f32],
        _max: &mut [f32],
    ) -> usize {
        0
    }
    fn audio_play(
        &self,
        _port: u32,
        _clip: u32,
        _start: i64,
        _end: i64,
        _looped: bool,
        _gain: f64,
    ) -> u32 {
        0
    }
    fn audio_stop(&self, _voice: u32, _fade_s: f64) {}
    fn audio_voice(&self, _voice: u32) -> Option<(f64, i32)> {
        None
    }
    fn audio_set(&self, _voice: u32, _param: u32, _value: f64) -> bool {
        false
    }
    fn audio_srate(&self) -> f64 {
        0.0
    }
    /// 1 rendue, 0 en cours, -1 impossible.
    fn audio_thumb(&self, _path: &str, _min: &mut [f32], _max: &mut [f32]) -> i32 {
        -1
    }

    // --- les vues (ABI 3)
    #[allow(clippy::too_many_arguments)]
    fn create_view(
        &self,
        _title: &str,
        _ident: &str,
        _x: i32,
        _y: i32,
        _w: i32,
        _h: i32,
        _docked: bool,
    ) -> isize {
        0
    }
    fn destroy_window(&self, _hwnd: isize) {}
    fn frames(&self, _hwnd: isize, _on: bool) {}
    fn frame_info(&self) -> FrameInfo {
        FrameInfo::default()
    }
}

/// Ou en est un chargement de fichier.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioLoad {
    Pending,
    Ready(u32),
    Failed(String),
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
    audio: AudioApi {
        load: a_load,
        poll: a_poll,
        info: a_info,
        release: a_release,
        peaks: a_peaks,
        play: a_play,
        stop: a_stop,
        voice: a_voice,
        set: a_set,
        srate: a_srate,
        thumb: a_thumb,
    },
    create_view: v_create,
    destroy_window: v_destroy,
    frames: v_frames,
    frame_info: v_frame_info,
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

// ---------------------------------------------------------------------------
// Le moteur audio
// ---------------------------------------------------------------------------

fn with<T>(default: T, f: impl FnOnce(&dyn Host) -> T) -> T {
    match IMPL.get() {
        Some(h) => guard(|| f(h.as_ref())).unwrap_or(default),
        None => default,
    }
}

extern "C" fn a_load(path: *const u8, len: usize) -> u32 {
    // SAFETY: tranche valide pendant l'appel.
    let p = unsafe { bytes(path, len) };
    with(0, |h| h.audio_load(&String::from_utf8_lossy(p)))
}

extern "C" fn a_poll(
    ticket: u32,
    clip: *mut u32,
    err: *mut u8,
    cap: usize,
    err_len: *mut usize,
) -> i32 {
    match with(AudioLoad::Failed("panic".into()), |h| h.audio_poll(ticket)) {
        AudioLoad::Pending => LOAD_PENDING,
        AudioLoad::Ready(c) => {
            if !clip.is_null() {
                // SAFETY: le module passe l'adresse d'un u32 a lui.
                unsafe { *clip = c };
            }
            LOAD_READY
        }
        AudioLoad::Failed(e) => {
            let b = e.as_bytes();
            let n = b.len().min(cap);
            if !err.is_null() && n > 0 {
                // SAFETY: le module passe `cap` octets inscriptibles.
                unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), err, n) };
            }
            if !err_len.is_null() {
                // SAFETY: l'adresse d'un usize du module.
                unsafe { *err_len = n };
            }
            LOAD_FAILED
        }
    }
}

extern "C" fn a_info(clip: u32, out: *mut ClipInfo) -> i32 {
    match with(None, |h| h.audio_info(clip)) {
        Some(i) => {
            if !out.is_null() {
                // SAFETY: l'adresse d'un ClipInfo du module.
                unsafe { *out = i };
            }
            1
        }
        None => 0,
    }
}

extern "C" fn a_release(clip: u32) {
    with((), |h| h.audio_release(clip));
}

extern "C" fn a_peaks(
    clip: u32,
    ch: i32,
    start: f64,
    fpp: f64,
    n: usize,
    min: *mut f32,
    max: *mut f32,
) -> usize {
    if min.is_null() || max.is_null() || n == 0 {
        return 0;
    }
    // SAFETY: le module passe deux tableaux distincts de `n` f32, a lui,
    // pendant l'appel.
    let (lo, hi) = unsafe {
        (
            std::slice::from_raw_parts_mut(min, n),
            std::slice::from_raw_parts_mut(max, n),
        )
    };
    let c = usize::try_from(ch).ok();
    with(0, |h| h.audio_peaks(clip, c, start, fpp, lo, hi))
}

extern "C" fn a_play(port: u32, clip: u32, start: i64, end: i64, looped: i32, gain: f64) -> u32 {
    with(0, |h| {
        h.audio_play(port, clip, start, end, looped != 0, gain)
    })
}

extern "C" fn a_stop(voice: u32, fade_s: f64) {
    with((), |h| h.audio_stop(voice, fade_s));
}

extern "C" fn a_voice(voice: u32, pos: *mut f64) -> i32 {
    match with(None, |h| h.audio_voice(voice)) {
        Some((p, st)) => {
            if !pos.is_null() {
                // SAFETY: l'adresse d'un f64 du module.
                unsafe { *pos = p };
            }
            st
        }
        None => -1,
    }
}

extern "C" fn a_set(voice: u32, param: u32, value: f64) -> i32 {
    with(false, |h| h.audio_set(voice, param, value)) as i32
}

extern "C" fn a_thumb(path: *const u8, len: usize, min: *mut f32, max: *mut f32, n: usize) -> i32 {
    if min.is_null() || max.is_null() || n == 0 {
        return -1;
    }
    // SAFETY: le module passe un chemin valide et deux tableaux distincts de
    // `n` f32, a lui, pendant l'appel.
    let (p, lo, hi) = unsafe {
        (
            bytes(path, len),
            std::slice::from_raw_parts_mut(min, n),
            std::slice::from_raw_parts_mut(max, n),
        )
    };
    with(-1, |h| h.audio_thumb(&String::from_utf8_lossy(p), lo, hi))
}

extern "C" fn a_srate() -> f64 {
    with(0.0, |h| h.audio_srate())
}

// ---------------------------------------------------------------------------
// Les vues
// ---------------------------------------------------------------------------

extern "C" fn v_create(spec: *const ViewSpec) -> isize {
    if spec.is_null() {
        return 0;
    }
    // SAFETY: le module passe une ViewSpec valide pendant l'appel.
    let sp = unsafe { &*spec };
    // SAFETY: tranches decrites par la ViewSpec, valides pendant l'appel.
    let (t, i) = unsafe { (bytes(sp.title, sp.title_len), bytes(sp.ident, sp.ident_len)) };
    let (t, i) = (String::from_utf8_lossy(t), String::from_utf8_lossy(i));
    with(0, |h| {
        h.create_view(&t, &i, sp.x, sp.y, sp.w, sp.h, sp.docked != 0)
    })
}

extern "C" fn v_destroy(hwnd: isize) {
    with((), |h| h.destroy_window(hwnd));
}

extern "C" fn v_frames(hwnd: isize, on: i32) {
    with((), |h| h.frames(hwnd, on != 0));
}

extern "C" fn v_frame_info(out: *mut FrameInfo) {
    let fi = with(FrameInfo::default(), |h| h.frame_info());
    if !out.is_null() {
        // SAFETY: l'adresse d'un FrameInfo du module.
        unsafe { *out = fi };
    }
}
