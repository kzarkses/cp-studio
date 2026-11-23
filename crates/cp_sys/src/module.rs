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
    ABI, BUSY, ClipInfo, EXT_NAME, ExtApi, FAULT, FrameInfo, HostApi, LOAD_PENDING, LOAD_READY,
    LOG_ERROR, LOG_INFO, LOG_WARN, LaneClip, LaneInfo, LaneNote, ModuleApi, OK, ViewSpec,
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

    // --- l'extension du chargeur (absente d'un chargeur plus ancien)

    /// La table d'extension, si le chargeur l'a.
    pub fn ext(&self) -> Option<&'static ExtApi> {
        let p = (self.0.get_func)(EXT_NAME.as_ptr() as *const core::ffi::c_char);
        if p.is_null() {
            return None;
        }
        // SAFETY: le chargeur rend sa table statique pour ce nom (et un
        // chargeur qui ne la connait pas rend nul, REAPER n'ayant pas de
        // fonction de ce nom).
        let e = unsafe { &*(p as *const ExtApi) };
        (e.version >= 1).then_some(e)
    }

    /// Les ports du moteur (32 avec un chargeur sans extension).
    pub fn ports(&self) -> u32 {
        self.ext().map_or(32, |e| (e.ports)())
    }

    /// Joue un clip MIDI sur la lane d'un port. Faux sans extension.
    #[allow(clippy::too_many_arguments)]
    pub fn lane_play(
        &self,
        port: u32,
        id: u64,
        notes: &[LaneNote],
        len: f64,
        brace: (f64, f64),
        looped: bool,
        from: f64,
        fpb: f64,
        at: Option<i64>,
    ) -> bool {
        let Some(e) = self.ext() else { return false };
        let c = LaneClip {
            id,
            notes: notes.as_ptr(),
            count: notes.len(),
            len,
            loop_a: brace.0,
            loop_b: brace.1,
            looped: looped as i32,
            pad: 0,
        };
        (e.lane_play)(port, &c, from, fpb, at.unwrap_or(-1)) == 1
    }

    /// Les notes du clip qui joue, remplacees sans relance.
    pub fn lane_update(
        &self,
        port: u32,
        id: u64,
        notes: &[LaneNote],
        len: f64,
        brace: (f64, f64),
        looped: bool,
    ) -> bool {
        let Some(e) = self.ext() else { return false };
        let c = LaneClip {
            id,
            notes: notes.as_ptr(),
            count: notes.len(),
            len,
            loop_a: brace.0,
            loop_b: brace.1,
            looped: looped as i32,
            pad: 0,
        };
        (e.lane_update)(port, &c) == 1
    }

    pub fn lane_stop(&self, port: u32, at: Option<i64>) -> bool {
        self.ext()
            .is_some_and(|e| (e.lane_stop)(port, at.unwrap_or(-1)) == 1)
    }

    pub fn lane_rate(&self, port: u32, fpb: f64, at: Option<i64>) -> bool {
        self.ext()
            .is_some_and(|e| (e.lane_rate)(port, fpb, at.unwrap_or(-1)) == 1)
    }

    pub fn lane_msg(&self, port: u32, at: Option<i64>, msg: &[u8]) -> bool {
        self.ext().is_some_and(|e| {
            (e.lane_msg)(port, at.unwrap_or(-1), msg.as_ptr(), msg.len().min(3)) == 1
        })
    }

    pub fn lane_query(&self, port: u32) -> Option<LaneInfo> {
        let e = self.ext()?;
        let mut i = LaneInfo::default();
        ((e.lane_query)(port, &mut i) == 1).then_some(i)
    }

    /// Arrete une voix au frame `at` ; sans extension, tout de suite.
    pub fn audio_stop_at(&self, voice: u32, at: i64, fade_s: f64) {
        match self.ext() {
            Some(e) => {
                (e.stop_at)(voice, at, fade_s);
            }
            None => self.audio_stop(voice, fade_s),
        }
    }

    pub fn midi_sent(&self, port: u32) -> u64 {
        self.ext().map_or(0, |e| (e.midi_sent)(port))
    }

    /// Retire les lancements et arrets de la lane qui attendent encore.
    pub fn lane_cancel(&self, port: u32) -> bool {
        self.ext().is_some_and(|e| (e.lane_cancel)(port) == 1)
    }

    /// Ecrit un clip du moteur en WAV. Faux sans extension ou si l'ecriture
    /// echoue.
    pub fn clip_save(&self, clip: u32, path: &str) -> bool {
        self.ext()
            .is_some_and(|e| (e.clip_save)(clip, path.as_ptr(), path.len()) == 1)
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
    /// La region `[start, end)` d'un clip etiree par l'etireur de REAPER
    /// (`tempo`, rapport de hauteur `shift`), cuite sur un fil de travail :
    /// un ticket, lu par [`Self::audio_poll`] ; 0 si refuse.
    pub fn audio_stretch(&self, clip: u32, start: i64, end: i64, tempo: f64, shift: f64) -> u32 {
        (self.0.audio.stretch)(clip, start, end, tempo, shift)
    }
    /// Fait jouer un port dans une piste (son GUID), ou (rien) sur la
    /// sortie materielle 1/2.
    pub fn audio_route(&self, port: u32, track: Option<&str>) -> bool {
        let g = track.unwrap_or("");
        (self.0.audio.route)(port, g.as_ptr(), g.len()) == 1
    }
    /// Les cretes d'un port (gauche, droite) depuis la derniere lecture.
    pub fn audio_levels(&self, port: u32) -> [f32; 2] {
        let mut v = [0f32; 2];
        (self.0.audio.levels)(port, v.as_mut_ptr());
        v
    }
    /// Le temps du moteur, en frames.
    pub fn audio_now(&self) -> i64 {
        (self.0.audio.now)()
    }
    /// Un lancement au frame `at` du moteur (`None` : tout de suite).
    #[allow(clippy::too_many_arguments)]
    pub fn audio_play_at(
        &self,
        port: u32,
        clip: u32,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
        at: Option<i64>,
    ) -> u32 {
        (self.0.audio.play_at)(
            port,
            clip,
            start,
            end,
            looped as i32,
            gain,
            at.unwrap_or(-1),
        )
    }
    /// Une prise audio des entrees `ch0`, `ch1` (-1 : mono).
    pub fn rec_start(&self, ch0: i32, ch1: i32, start: Option<i64>) -> u32 {
        (self.0.audio.rec_start)(ch0, ch1, start.unwrap_or(-1))
    }
    pub fn rec_stop(&self, id: u32, at: Option<i64>) {
        (self.0.audio.rec_stop)(id, at.unwrap_or(-1))
    }
    /// (etat `cp_abi::REC_*`, frames pris ou clip).
    pub fn rec_state(&self, id: u32) -> (i32, i64) {
        let mut v = 0i64;
        let st = (self.0.audio.rec_state)(id, &mut v);
        (st, v)
    }
    /// Une prise MIDI de l'entree `dev` (-1 : toutes).
    pub fn midi_rec_start(&self, dev: i32, start: Option<i64>) -> u32 {
        (self.0.audio.midi_rec_start)(dev, start.unwrap_or(-1))
    }
    pub fn midi_rec_stop(&self, id: u32, at: Option<i64>) {
        (self.0.audio.midi_rec_stop)(id, at.unwrap_or(-1))
    }
    /// Les messages d'une prise MIDI finie (`None` tant qu'elle court).
    pub fn midi_rec_take(&self, id: u32) -> Option<Vec<cp_abi::MidiEvent>> {
        let mut cap = 256usize;
        loop {
            let mut v = vec![cp_abi::MidiEvent::default(); cap];
            let n = (self.0.audio.midi_rec_take)(id, v.as_mut_ptr(), cap);
            if n == -1 {
                return None;
            }
            if n >= 0 {
                v.truncate(n as usize);
                return Some(v);
            }
            cap = (-(n + 2)) as usize;
        }
    }
    /// Ecouter (ou plus) les notes des entrees MIDI.
    pub fn midi_listen(&self, on: bool) {
        (self.0.audio.midi_listen)(on as i32)
    }
    /// Les notes arrivees depuis le dernier appel (statut, note, velocite,
    /// entree en octets).
    pub fn midi_notes(&self) -> Vec<u32> {
        let mut v = vec![0u32; 1024];
        let n = (self.0.audio.midi_notes)(v.as_mut_ptr(), v.len());
        v.truncate(n);
        v
    }
    /// La clock MIDI externe : ecouter l'entree `dev` (-1 : aucune).
    pub fn clock_device(&self, dev: i32) {
        (self.0.audio.clock_device)(dev)
    }
    /// La clock MIDI externe : ses 14 valeurs (voir `cp_abi::AudioApi`).
    pub fn clock_read(&self) -> Option<[f64; 14]> {
        let mut v = [0f64; 14];
        ((self.0.audio.clock_read)(v.as_mut_ptr()) == 1).then_some(v)
    }
    /// La vignette d'un fichier : 1 rendue, 0 en cours, -1 impossible.
    pub fn audio_thumb(&self, path: &str, min: &mut [f32], max: &mut [f32]) -> i32 {
        let n = min.len().min(max.len());
        (self.0.audio.thumb)(
            path.as_ptr(),
            path.len(),
            min.as_mut_ptr(),
            max.as_mut_ptr(),
            n,
        )
    }

    // --- les vues

    /// Une vue flottante, ancree ou a placer (`mode` : `cp_abi::VIEW_*` ;
    /// `ident` : la cle du docker chez REAPER).
    #[allow(clippy::too_many_arguments)]
    pub fn create_view(
        &self,
        title: &str,
        ident: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        mode: i32,
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
            docked: mode,
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

    // --- les places natives (ABI 6)

    /// Pose `h` a la place native (`show` faux : a la taille, cachee), ou la
    /// rend a REAPER (`h` = None).
    pub fn place_take(&self, place: u32, h: Option<Hwnd>, show: bool) -> bool {
        (self.0.place_take)(place, h.map_or(0, |h| h.0), show as i32) != 0
    }
    /// (etat `cp_abi::PLACE_*`, la vue qui l'occupe).
    pub fn place_state(&self, place: u32) -> (i32, Option<Hwnd>) {
        let mut w = 0isize;
        let st = (self.0.place_state)(place, &mut w);
        (st, (w != 0).then_some(Hwnd(w)))
    }
    /// Une vue qui ne prend jamais le clavier.
    pub fn set_keyless(&self, h: Hwnd, on: bool) {
        (self.0.set_keyless)(h.0, on as i32)
    }

    // --- l'entree de la carte son (ABI 7)

    /// Capture l'entree `ch0` (et `ch1` : la moyenne des deux), et rend ses
    /// derniers echantillons dans `out`. `ch0` < 0 arrete la capture.
    pub fn audio_input(&self, ch0: i32, ch1: i32, out: &mut [f32]) -> usize {
        (self.0.audio_input)(ch0, ch1, out.as_mut_ptr(), out.len())
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
