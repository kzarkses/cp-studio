//! Cote chargeur : la table [`HostApi`] offerte au module.
//!
//! Le chargeur ecrit un `impl Host` en Rust sur ; ce module fabrique les
//! fonctions `extern "C"` qui le servent au module.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;

use cp_abi::{
    ABI, AudioApi, ClipInfo, EXT_NAME, EXT_VERSION, ExtApi, FrameInfo, HostApi, LOAD_FAILED,
    LOAD_PENDING, LOAD_READY, LaneClip, LaneInfo, LaneNote, ViewSpec,
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
    /// Un ticket de cuisson (0 : refuse).
    fn audio_stretch(&self, _clip: u32, _start: i64, _end: i64, _tempo: f64, _shift: f64) -> u32 {
        0
    }
    /// Un port dans une piste (rien : la sortie materielle).
    fn audio_route(&self, _port: u32, _guid: Option<&str>) -> bool {
        false
    }
    /// Les cretes d'un port depuis la derniere lecture.
    fn audio_levels(&self, _port: u32) -> Option<[f32; 2]> {
        None
    }
    /// La clock externe : l'entree ecoutee.
    fn clock_device(&self, _dev: i32) {}
    /// Les notes des entrees MIDI : ecouter.
    fn midi_listen(&self, _on: bool) {}
    /// Les notes arrivees.
    fn midi_notes(&self) -> Vec<u32> {
        Vec::new()
    }
    /// La clock externe : ses 14 valeurs.
    fn clock_read(&self) -> Option<[f64; 14]> {
        None
    }
    /// Le temps du moteur, en frames.
    fn audio_now(&self) -> i64 {
        0
    }
    /// Un lancement date (`at` : frame du moteur, `None` : tout de suite).
    #[allow(clippy::too_many_arguments)]
    fn audio_play_at(
        &self,
        _port: u32,
        _clip: u32,
        _start: i64,
        _end: i64,
        _looped: bool,
        _gain: f64,
        _at: Option<i64>,
    ) -> u32 {
        0
    }
    /// Une prise audio.
    fn rec_start(&self, _ch0: i32, _ch1: i32, _start: Option<i64>) -> u32 {
        0
    }
    fn rec_stop(&self, _id: u32, _at: Option<i64>) {}
    /// (etat, frames pris ou clip).
    fn rec_state(&self, _id: u32) -> (i32, i64) {
        (cp_abi::REC_UNKNOWN, 0)
    }
    /// Une prise MIDI.
    fn midi_rec_start(&self, _dev: i32, _start: Option<i64>) -> u32 {
        0
    }
    fn midi_rec_stop(&self, _id: u32, _at: Option<i64>) {}
    /// Ses messages si elle est finie et qu'ils tiennent dans `cap` ;
    /// `Err(n)` : n messages qui ne tiennent pas (rien n'est oublie) ;
    /// `Ok(None)` : elle court encore.
    fn midi_rec_take(
        &self,
        _id: u32,
        _cap: usize,
    ) -> Result<Option<Vec<cp_abi::MidiEvent>>, usize> {
        Ok(None)
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
        _mode: i32,
    ) -> isize {
        0
    }
    fn destroy_window(&self, _hwnd: isize) {}
    fn frames(&self, _hwnd: isize, _on: bool) {}
    fn frame_info(&self) -> FrameInfo {
        FrameInfo::default()
    }

    // --- les places natives (ABI 6)
    fn place_take(&self, _place: u32, _hwnd: isize, _show: bool) -> bool {
        false
    }
    /// (etat `PLACE_*`, vue qui l'occupe).
    fn place_state(&self, _place: u32) -> (i32, isize) {
        (cp_abi::PLACE_MISSING, 0)
    }
    fn set_keyless(&self, _hwnd: isize, _on: bool) {}

    // --- l'entree de la carte son (ABI 7)
    // --- l'extension (EXT_VERSION 1) : les lanes MIDI de la Session
    fn ports(&self) -> u32 {
        32
    }
    #[allow(clippy::too_many_arguments)]
    fn lane_play(
        &self,
        _port: u32,
        _clip: LaneClipRef<'_>,
        _from: f64,
        _fpb: f64,
        _at: Option<i64>,
    ) -> bool {
        false
    }
    fn lane_update(&self, _port: u32, _clip: LaneClipRef<'_>) -> bool {
        false
    }
    fn lane_stop(&self, _port: u32, _at: Option<i64>) -> bool {
        false
    }
    fn lane_rate(&self, _port: u32, _fpb: f64, _at: Option<i64>) -> bool {
        false
    }
    fn lane_msg(&self, _port: u32, _at: Option<i64>, _msg: &[u8]) -> bool {
        false
    }
    fn lane_query(&self, _port: u32) -> Option<LaneInfo> {
        None
    }
    fn stop_at(&self, _voice: u32, _at: i64, _fade_s: f64) -> bool {
        false
    }
    fn midi_sent(&self, _port: u32) -> u64 {
        0
    }
    fn lane_cancel(&self, _port: u32) -> bool {
        false
    }
    fn clip_save(&self, _clip: u32, _path: &str) -> bool {
        false
    }

    fn audio_input(&self, _ch0: i32, _ch1: i32, _out: &mut [f32]) -> usize {
        0
    }
}

/// Ou en est un chargement de fichier.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioLoad {
    Pending,
    Ready(u32),
    Failed(String),
}

/// Un clip MIDI tel que le module l'a passe, emprunte le temps de l'appel.
pub struct LaneClipRef<'a> {
    pub id: u64,
    pub notes: &'a [LaneNote],
    pub len: f64,
    pub loop_a: f64,
    pub loop_b: f64,
    pub looped: bool,
}

static IMPL: OnceLock<Box<dyn Host>> = OnceLock::new();

static EXT: ExtApi = ExtApi {
    version: EXT_VERSION,
    ports: e_ports,
    lane_play: e_lane_play,
    lane_update: e_lane_update,
    lane_stop: e_lane_stop,
    lane_rate: e_lane_rate,
    lane_msg: e_lane_msg,
    lane_query: e_lane_query,
    stop_at: e_stop_at,
    midi_sent: e_midi_sent,
    lane_cancel: e_lane_cancel,
    clip_save: e_clip_save,
};

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
        stretch: a_stretch,
        route: a_route,
        levels: a_levels,
        clock_device: a_clock_device,
        clock_read: a_clock_read,
        midi_listen: a_midi_listen,
        midi_notes: a_midi_notes,
        now: a_now,
        play_at: a_play_at,
        rec_start: a_rec_start,
        rec_stop: a_rec_stop,
        rec_state: a_rec_state,
        midi_rec_start: a_midi_rec_start,
        midi_rec_stop: a_midi_rec_stop,
        midi_rec_take: a_midi_rec_take,
    },
    create_view: v_create,
    destroy_window: v_destroy,
    frames: v_frames,
    frame_info: v_frame_info,
    place_take: p_take,
    place_state: p_state,
    set_keyless: p_keyless,
    audio_input: i_input,
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
    // L'extension n'est pas une fonction de REAPER : la table elle-meme.
    if n.to_bytes_with_nul() == EXT_NAME {
        return &EXT as *const ExtApi as *mut c_void;
    }
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

extern "C" fn a_stretch(clip: u32, start: i64, end: i64, tempo: f64, shift: f64) -> u32 {
    with(0, |h| h.audio_stretch(clip, start, end, tempo, shift))
}

extern "C" fn a_route(port: u32, guid: *const u8, len: usize) -> i32 {
    // SAFETY: le module passe un GUID valide pendant l'appel (ou len = 0).
    let g = (len > 0 && !guid.is_null()).then(|| unsafe { bytes(guid, len) });
    let g = g.map(|b| String::from_utf8_lossy(b).into_owned());
    with(false, |h| h.audio_route(port, g.as_deref())) as i32
}

extern "C" fn a_levels(port: u32, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    match with(None, |h| h.audio_levels(port)) {
        Some(v) => {
            // SAFETY: le module passe deux f32 a lui.
            unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, 2) };
            1
        }
        None => 0,
    }
}

extern "C" fn a_midi_listen(on: i32) {
    with((), |h| h.midi_listen(on != 0));
}

extern "C" fn a_midi_notes(out: *mut u32, cap: usize) -> usize {
    if out.is_null() || cap == 0 {
        return 0;
    }
    let v = with(Vec::new(), |h| h.midi_notes());
    let n = v.len().min(cap);
    // SAFETY: le module passe `cap` u32 a lui.
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, n) };
    n
}

fn when(v: i64) -> Option<i64> {
    (v >= 0).then_some(v)
}

extern "C" fn a_now() -> i64 {
    with(0, |h| h.audio_now())
}

extern "C" fn a_play_at(
    port: u32,
    clip: u32,
    start: i64,
    end: i64,
    looped: i32,
    gain: f64,
    at: i64,
) -> u32 {
    with(0, |h| {
        h.audio_play_at(port, clip, start, end, looped != 0, gain, when(at))
    })
}

extern "C" fn a_rec_start(ch0: i32, ch1: i32, start: i64) -> u32 {
    with(0, |h| h.rec_start(ch0, ch1, when(start)))
}

extern "C" fn a_rec_stop(id: u32, at: i64) {
    with((), |h| h.rec_stop(id, when(at)));
}

extern "C" fn a_rec_state(id: u32, out: *mut i64) -> i32 {
    let (st, v) = with((cp_abi::REC_UNKNOWN, 0), |h| h.rec_state(id));
    if !out.is_null() {
        // SAFETY: le module passe un i64 a lui.
        unsafe { *out = v };
    }
    st
}

extern "C" fn a_midi_rec_start(dev: i32, start: i64) -> u32 {
    with(0, |h| h.midi_rec_start(dev, when(start)))
}

extern "C" fn a_midi_rec_stop(id: u32, at: i64) {
    with((), |h| h.midi_rec_stop(id, when(at)));
}

extern "C" fn a_midi_rec_take(id: u32, out: *mut cp_abi::MidiEvent, cap: usize) -> i64 {
    let cap = if out.is_null() { 0 } else { cap };
    match with(Ok(None), |h| h.midi_rec_take(id, cap)) {
        Ok(None) => -1,
        Ok(Some(v)) => {
            let n = v.len().min(cap);
            if n > 0 {
                // SAFETY: le module passe `cap` messages a lui ; n <= cap.
                unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, n) };
            }
            n as i64
        }
        Err(n) => -(n as i64) - 2,
    }
}

extern "C" fn a_clock_device(dev: i32) {
    with((), |h| h.clock_device(dev));
}

extern "C" fn a_clock_read(out: *mut f64) -> i32 {
    if out.is_null() {
        return 0;
    }
    match with(None, |h| h.clock_read()) {
        Some(v) => {
            // SAFETY: le module passe 14 f64 a lui.
            unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, 14) };
            1
        }
        None => 0,
    }
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
        h.create_view(&t, &i, sp.x, sp.y, sp.w, sp.h, sp.docked)
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

// ---------------------------------------------------------------------------
// Les places natives
// ---------------------------------------------------------------------------

extern "C" fn p_take(place: u32, hwnd: isize, show: i32) -> i32 {
    with(false, |h| h.place_take(place, hwnd, show != 0)) as i32
}

extern "C" fn p_state(place: u32, hwnd: *mut isize) -> i32 {
    let (st, w) = with((cp_abi::PLACE_MISSING, 0), |h| h.place_state(place));
    if !hwnd.is_null() {
        // SAFETY: l'adresse d'un isize du module.
        unsafe { *hwnd = w };
    }
    st
}

extern "C" fn p_keyless(hwnd: isize, on: i32) {
    with((), |h| h.set_keyless(hwnd, on != 0));
}

// ---------------------------------------------------------------------------
// L'entree de la carte son
// ---------------------------------------------------------------------------

extern "C" fn i_input(ch0: i32, ch1: i32, out: *mut f32, n: usize) -> usize {
    let buf: &mut [f32] = if out.is_null() || n == 0 {
        &mut []
    } else {
        // SAFETY: `n` floats du module, a nous pendant l'appel.
        unsafe { std::slice::from_raw_parts_mut(out, n) }
    };
    with(0, |h| h.audio_input(ch0, ch1, buf))
}

// ---------------------------------------------------------------------------
// L'extension
// ---------------------------------------------------------------------------

/// # Safety
/// `c` est nul, ou pointe un `LaneClip` dont les notes sont lisibles pendant
/// l'appel.
unsafe fn lane_clip<'a>(c: *const LaneClip) -> Option<LaneClipRef<'a>> {
    if c.is_null() {
        return None;
    }
    // SAFETY: garanti par l'appelant.
    let c = unsafe { &*c };
    let notes: &[LaneNote] = if c.notes.is_null() || c.count == 0 {
        &[]
    } else {
        // SAFETY: garanti par l'appelant ; borne a 1 million de notes.
        unsafe { std::slice::from_raw_parts(c.notes, c.count.min(1 << 20)) }
    };
    Some(LaneClipRef {
        id: c.id,
        notes,
        len: c.len,
        loop_a: c.loop_a,
        loop_b: c.loop_b,
        looped: c.looped != 0,
    })
}

extern "C" fn e_ports() -> u32 {
    with(32, |h| h.ports())
}

extern "C" fn e_lane_play(port: u32, clip: *const LaneClip, from: f64, fpb: f64, at: i64) -> i32 {
    // SAFETY: le module passe un clip valide pendant l'appel.
    let Some(c) = (unsafe { lane_clip(clip) }) else {
        return 0;
    };
    with(false, |h| h.lane_play(port, c, from, fpb, when(at))) as i32
}

extern "C" fn e_lane_update(port: u32, clip: *const LaneClip) -> i32 {
    // SAFETY: comme e_lane_play.
    let Some(c) = (unsafe { lane_clip(clip) }) else {
        return 0;
    };
    with(false, |h| h.lane_update(port, c)) as i32
}

extern "C" fn e_lane_stop(port: u32, at: i64) -> i32 {
    with(false, |h| h.lane_stop(port, when(at))) as i32
}

extern "C" fn e_lane_rate(port: u32, fpb: f64, at: i64) -> i32 {
    with(false, |h| h.lane_rate(port, fpb, when(at))) as i32
}

extern "C" fn e_lane_msg(port: u32, at: i64, msg: *const u8, len: usize) -> i32 {
    // SAFETY: tranche valide pendant l'appel.
    let m = unsafe { bytes(msg, len.min(3)) };
    with(false, |h| h.lane_msg(port, when(at), m)) as i32
}

extern "C" fn e_lane_query(port: u32, out: *mut LaneInfo) -> i32 {
    if out.is_null() {
        return 0;
    }
    match with(None, |h| h.lane_query(port)) {
        Some(i) => {
            // SAFETY: le module passe une case valide.
            unsafe { *out = i };
            1
        }
        None => 0,
    }
}

extern "C" fn e_stop_at(voice: u32, at: i64, fade_s: f64) -> i32 {
    with(false, |h| h.stop_at(voice, at, fade_s)) as i32
}

extern "C" fn e_midi_sent(port: u32) -> u64 {
    with(0, |h| h.midi_sent(port))
}

extern "C" fn e_lane_cancel(port: u32) -> i32 {
    with(false, |h| h.lane_cancel(port)) as i32
}

extern "C" fn e_clip_save(clip: u32, path: *const u8, len: usize) -> i32 {
    // SAFETY: tranche valide pendant l'appel.
    let p = unsafe { bytes(path, len) };
    with(false, |h| h.clip_save(clip, &String::from_utf8_lossy(p))) as i32
}
