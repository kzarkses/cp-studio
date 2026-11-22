//! Le fil audio de REAPER : le crochet de bloc, les aperçus, les fichiers.
//!
//! - [`AudioHook`] : `Audio_RegHardwareHook`. REAPER l'appelle deux fois par
//!   bloc materiel, avant (`is_post = false`) et apres (`is_post = true`) son
//!   propre traitement. L'horloge du moteur avance au passage « post ».
//! - [`Preview`] : un `preview_register_t` permanent qui tire une
//!   [`PortSource`]. Vers une sortie materielle (`PlayPreviewEx`) : le rendu se
//!   fait dans le fil audio, au bloc materiel, entre les deux passages du
//!   crochet. (Un aperçu de PISTE passerait par la piste, donc par le rendu
//!   anticipe de REAPER, en avance et par gros blocs.)
//! - [`open_file`] : un fichier ouvert par REAPER (tous les formats qu'il lit).

use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::guard;
use crate::pcm::{CppSource, PortSource};
use crate::reaper::Reaper;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn InitializeCriticalSection(cs: *mut CriticalSection);
    fn DeleteCriticalSection(cs: *mut CriticalSection);
}

/// `CRITICAL_SECTION` x64 : 40 octets, aligne sur 8.
#[repr(C)]
pub struct CriticalSection([u64; 5]);

/// `preview_register_t` (Windows).
#[repr(C)]
pub struct PreviewRegister {
    pub cs: CriticalSection,
    pub src: *mut c_void,
    /// &1024 = mono ; les 10 bits bas = premiere sortie. -1 = vers `preview_track`.
    pub m_out_chan: i32,
    pub curpos: f64,
    pub loop_: bool,
    pub volume: f64,
    pub peakvol: [f64; 2],
    pub preview_track: *mut c_void,
}

/// `audio_hook_register_t`.
#[repr(C)]
pub struct AudioHookRegister {
    pub on_audio_buffer: Option<unsafe extern "C" fn(bool, i32, f64, *mut AudioHookRegister)>,
    pub userdata1: *mut c_void,
    pub userdata2: *mut c_void,
    pub input_nch: i32,
    pub output_nch: i32,
    pub get_buffer: Option<unsafe extern "C" fn(bool, i32) -> *mut f64>,
}

impl Reaper {
    /// Une fonction de l'API, typee.
    ///
    /// # Safety
    /// `F` est un pointeur de fonction `extern "C"` dont la signature est celle
    /// que `reaper_plugin_functions.h` donne pour `name`.
    pub unsafe fn api<F: Copy>(&self, name: &std::ffi::CStr) -> Option<F> {
        assert!(std::mem::size_of::<F>() == std::mem::size_of::<*mut c_void>());
        let p = self.get_func(name);
        if p.is_null() {
            return None;
        }
        // SAFETY: meme taille (verifie), et la signature est garantie par
        // l'appelant.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) })
    }
}

// ---------------------------------------------------------------------------
// Le crochet audio
// ---------------------------------------------------------------------------

type HookFn = fn(is_post: bool, frames: i32, srate: f64);
type RegHookFn = unsafe extern "C" fn(bool, *mut AudioHookRegister) -> i32;

static HOOK_FN: OnceLock<HookFn> = OnceLock::new();
/// Les entrees de la carte (`GetNumAudioInputs`), posees par le fil principal.
/// REAPER annonce 0 entree au crochet (`input_nch`, mesure sur une Fireface en
/// ASIO le 2026-10-10) : c'est cette borne qui compte.
static HW_INPUTS: AtomicI32 = AtomicI32::new(0);
/// L'adresse de la structure enregistree (vit toute la vie du processus).
static HOOK_REG: OnceLock<usize> = OnceLock::new();

unsafe extern "C" fn hook_trampoline(
    is_post: bool,
    len: i32,
    srate: f64,
    _: *mut AudioHookRegister,
) {
    if let Some(f) = HOOK_FN.get() {
        let _ = guard(|| f(is_post, len, srate));
    }
}

/// Le crochet de bloc. Un seul par processus.
pub struct AudioHook;

impl AudioHook {
    /// Enregistre `f`. Rend faux si REAPER refuse (ou si deja fait avec une
    /// autre fonction).
    pub fn register(r: &Reaper, f: HookFn) -> bool {
        if HOOK_FN.set(f).is_err() {
            return false;
        }
        // SAFETY: int Audio_RegHardwareHook(bool isAdd, audio_hook_register_t* reg).
        let Some(reg_fn) = (unsafe { r.api::<RegHookFn>(c"Audio_RegHardwareHook") }) else {
            return false;
        };
        let reg = Box::leak(Box::new(AudioHookRegister {
            on_audio_buffer: Some(hook_trampoline),
            userdata1: null_mut(),
            userdata2: null_mut(),
            input_nch: 0,
            output_nch: 0,
            get_buffer: None,
        }));
        let _ = HOOK_REG.set(reg as *mut AudioHookRegister as usize);
        // SAFETY: la structure vit jusqu'a la fin du processus (fuite voulue).
        unsafe { reg_fn(true, reg) > 0 }
    }

    /// PENDANT LE CROCHET SEULEMENT : l'entree `ch` de la carte son, `frames`
    /// echantillons (`None` hors des entrees). La tranche ne vaut que pour cet
    /// appel du crochet.
    pub fn input(ch: i32, frames: i32) -> Option<&'static [f64]> {
        let &addr = HOOK_REG.get()?;
        // SAFETY: la structure enregistree vit toute la vie du processus.
        let reg = unsafe { &*(addr as *const AudioHookRegister) };
        let n = reg.input_nch.max(HW_INPUTS.load(Ordering::Relaxed));
        if ch < 0 || ch >= n || frames <= 0 {
            return None;
        }
        let gb = reg.get_buffer?;
        // SAFETY: double* GetBuffer(bool isOutput, int idx), valable pendant
        // le crochet ; REAPER l'a rempli avant de nous appeler.
        let p = unsafe { gb(false, ch) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `frames` doubles du tampon d'entree de ce bloc.
        Some(unsafe { std::slice::from_raw_parts(p, frames as usize) })
    }

    /// Les latences de la carte (entree, sortie), en echantillons.
    pub fn io_latency(r: &Reaper) -> (i32, i32) {
        type F = unsafe extern "C" fn(*mut i32, *mut i32);
        // SAFETY: void GetInputOutputLatency(int*, int*).
        let Some(f) = (unsafe { r.api::<F>(c"GetInputOutputLatency") }) else {
            return (0, 0);
        };
        let (mut i, mut o) = (0i32, 0i32);
        // SAFETY: ecrit deux entiers a nous.
        unsafe { f(&mut i, &mut o) };
        (i.max(0), o.max(0))
    }

    /// Relit le nombre d'entrees de la carte (fil principal).
    pub fn refresh_inputs(r: &Reaper) -> i32 {
        type Num = unsafe extern "C" fn() -> i32;
        // SAFETY: int GetNumAudioInputs().
        let n = unsafe { r.api::<Num>(c"GetNumAudioInputs") }
            // SAFETY: sans argument, sur le fil principal.
            .map(|f| unsafe { f() })
            .unwrap_or(0)
            .max(0);
        HW_INPUTS.store(n, Ordering::Relaxed);
        n
    }

    /// Ce que REAPER a pose dans la structure du crochet : entrees et sorties
    /// annoncees, et si `GetBuffer` est la (pour le journal).
    pub fn announced() -> Option<(i32, i32, bool)> {
        let &addr = HOOK_REG.get()?;
        let reg = addr as *const AudioHookRegister;
        // SAFETY: la structure enregistree vit toute la vie du processus ;
        // lecture volatile, REAPER l'ecrit depuis le fil audio.
        unsafe {
            Some((
                std::ptr::read_volatile(&raw const (*reg).input_nch),
                std::ptr::read_volatile(&raw const (*reg).output_nch),
                std::ptr::read_volatile(&raw const (*reg).get_buffer).is_some(),
            ))
        }
    }

    /// Retire le crochet (a la fermeture de REAPER).
    pub fn unregister(r: &Reaper) {
        let Some(&addr) = HOOK_REG.get() else { return };
        // SAFETY: meme signature que ci-dessus.
        if let Some(reg_fn) = unsafe { r.api::<RegHookFn>(c"Audio_RegHardwareHook") } {
            // SAFETY: la structure enregistree, toujours vivante.
            unsafe { reg_fn(false, addr as *mut AudioHookRegister) };
        }
    }
}

// ---------------------------------------------------------------------------
// L'aperçu permanent
// ---------------------------------------------------------------------------

type PlayPreviewExFn = unsafe extern "C" fn(*mut PreviewRegister, i32, f64) -> i32;
type StopPreviewFn = unsafe extern "C" fn(*mut PreviewRegister) -> i32;

/// Un aperçu qui tire une [`PortSource`] sans jamais s'arreter.
pub struct Preview {
    reg: Box<PreviewRegister>,
    source: PortSource,
    playing: bool,
    play: Option<PlayPreviewExFn>,
    stop: Option<StopPreviewFn>,
    play_tr: Option<PlayTrackFn>,
    stop_tr: Option<StopTrackFn>,
    /// La piste ou l'apercu joue (son GUID), ou rien : la sortie materielle.
    route: Option<String>,
}

// SAFETY: le registre n'est touche que par le fil principal (play/stop/drop) ;
// REAPER le lit sous sa section critique.
unsafe impl Send for Preview {}

impl Preview {
    pub fn new(r: &Reaper, source: PortSource) -> Preview {
        let mut reg = Box::new(PreviewRegister {
            cs: CriticalSection([0; 5]),
            src: source.as_ptr(),
            m_out_chan: 0,
            curpos: 0.0,
            loop_: false,
            volume: 1.0,
            peakvol: [0.0; 2],
            preview_track: null_mut(),
        });
        // SAFETY: section critique dans une boite a adresse stable, detruite au Drop.
        unsafe { InitializeCriticalSection(&mut reg.cs) };
        Preview {
            reg,
            source,
            playing: false,
            // SAFETY: signatures de reaper_plugin_functions.h :
            // int PlayPreviewEx(preview_register_t*, int bufflags, double measure_align),
            // int StopPreview(preview_register_t*).
            play: unsafe { r.api::<PlayPreviewExFn>(c"PlayPreviewEx") },
            // SAFETY: voir ci-dessus.
            stop: unsafe { r.api::<StopPreviewFn>(c"StopPreview") },
            // SAFETY: int PlayTrackPreview2Ex(ReaProject*, preview_register_t*,
            // int flags, double measure_align) ; int StopTrackPreview2(
            // ReaProject*, preview_register_t*).
            play_tr: unsafe { r.api::<PlayTrackFn>(c"PlayTrackPreview2Ex") },
            // SAFETY: voir ci-dessus.
            stop_tr: unsafe { r.api::<StopTrackFn>(c"StopTrackPreview2") },
            route: None,
        }
    }

    /// Fait jouer l'apercu DANS une piste (son GUID) : par ses effets, son
    /// volume, ses envois, comme un item. Le master n'est pas une
    /// destination valable (REAPER repond « succes » et ne joue rien) ; une
    /// piste qui porte un instrument ignore l'audio qu'on y verse.
    pub fn play_track(&mut self, r: &Reaper, guid: &str) -> bool {
        if self.playing && self.route.as_deref() == Some(guid) {
            return true;
        }
        self.stop();
        let (Some(play), Some(tr)) = (self.play_tr, r.track_of(guid)) else {
            return false;
        };
        self.reg.preview_track = tr;
        self.reg.m_out_chan = -1;
        // SAFETY: registre et source vivants tant que `self` vit ; `stop`
        // (et `Drop`) arretent l'apercu avant de les liberer.
        self.playing = unsafe { play(null_mut(), &mut *self.reg, 0, -1.0) } != 0;
        if self.playing {
            self.route = Some(guid.to_string());
        }
        self.playing
    }

    /// La piste ou joue l'apercu.
    pub fn routed(&self) -> Option<&str> {
        self.route.as_deref()
    }

    /// La piste de l'apercu existe-t-elle toujours (a la meme adresse) ? Vrai
    /// sans piste.
    pub fn track_alive(&self, r: &Reaper) -> bool {
        if self.route.is_none() {
            return true;
        }
        type Valid =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *const std::ffi::c_char) -> bool;
        // SAFETY: bool ValidatePtr2(ReaProject*, void*, const char*) ; un
        // pointeur quelconque est permis, c'est son role.
        match unsafe { r.api::<Valid>(c"ValidatePtr2") } {
            // SAFETY: projet nul = courant ; tout pointeur est permis.
            Some(f) => unsafe { f(null_mut(), self.reg.preview_track, c"MediaTrack*".as_ptr()) },
            None => true,
        }
    }

    /// Lance l'aperçu vers la sortie materielle `first_out` (0 = sorties 1/2),
    /// sans tampon, immediatement.
    pub fn play_hardware(&mut self, first_out: i32) -> bool {
        if self.playing && self.route.is_none() {
            return true;
        }
        self.stop();
        let Some(play) = self.play else { return false };
        self.reg.m_out_chan = first_out & 1023;
        self.reg.preview_track = null_mut();
        // SAFETY: registre et source vivants tant que `self` vit ; `Drop`
        // arrete l'aperçu avant de les liberer.
        self.playing = unsafe { play(&mut *self.reg, 0, -1.0) } != 0;
        self.playing
    }

    pub fn stop(&mut self) {
        if !self.playing {
            return;
        }
        if self.route.is_some() {
            if let Some(stop) = self.stop_tr {
                // SAFETY: le registre lance par `play_track`.
                unsafe { stop(null_mut(), &mut *self.reg) };
            }
        } else if let Some(stop) = self.stop {
            // SAFETY: le registre lance par `play_hardware`.
            unsafe { stop(&mut *self.reg) };
        }
        self.playing = false;
        self.route = None;
        self.reg.preview_track = null_mut();
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn source(&self) -> &PortSource {
        &self.source
    }

    /// Le `preview_register_t*` tel que REAPER le voit (tests).
    pub fn register_ptr(&mut self) -> *mut PreviewRegister {
        &mut *self.reg
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: initialisee dans `new`, plus utilisee par REAPER (arretee).
        unsafe { DeleteCriticalSection(&mut self.reg.cs) };
    }
}

// ---------------------------------------------------------------------------
// Entendre une note par sa piste
// ---------------------------------------------------------------------------

type PlayTrackFn = unsafe extern "C" fn(*mut c_void, *mut PreviewRegister, i32, f64) -> i32;
type StopTrackFn = unsafe extern "C" fn(*mut c_void, *mut PreviewRegister) -> i32;

/// Faire entendre une note dans l'editeur : un petit fichier MIDI (une note
/// longue) joue en apercu de PISTE, donc par ses effets et son instrument,
/// comme le fait l'editeur MIDI de REAPER. Elle sonne jusqu'a [`stop`].
///
/// [`stop`]: Audition::stop
pub struct Audition {
    reg: Box<PreviewRegister>,
    src: Option<CppSource>,
    playing: bool,
    play: Option<PlayTrackFn>,
    stop: Option<StopTrackFn>,
}

// SAFETY: le registre n'est touche que par le fil principal ; REAPER le lit
// sous sa section critique.
unsafe impl Send for Audition {}

/// Une note de `secs` secondes a 120 BPM, format 0.
fn one_note_smf(pitch: i32, chan: i32, vel: i32, secs: f64) -> Vec<u8> {
    let ticks = (secs * 2.0 * 960.0) as u32;
    let mut vlq = vec![(ticks & 0x7F) as u8];
    let mut t = ticks >> 7;
    while t > 0 {
        vlq.insert(0, ((t & 0x7F) as u8) | 0x80);
        t >>= 7;
    }
    let c = (chan.clamp(0, 15)) as u8;
    let p = pitch.clamp(0, 127) as u8;
    let v = vel.clamp(1, 127) as u8;
    let mut trk = vec![
        0x00,
        0xFF,
        0x51,
        0x03,
        0x07,
        0xA1,
        0x20,
        0x00,
        0x90 | c,
        p,
        v,
    ];
    trk.extend_from_slice(&vlq);
    trk.extend_from_slice(&[0x80 | c, p, 0x00, 0x00, 0xFF, 0x2F, 0x00]);
    let mut f = b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x03\xC0MTrk".to_vec();
    f.extend_from_slice(&(trk.len() as u32).to_be_bytes());
    f.extend_from_slice(&trk);
    f
}

impl Audition {
    pub fn new(r: &Reaper) -> Audition {
        let mut reg = Box::new(PreviewRegister {
            cs: CriticalSection([0; 5]),
            src: null_mut(),
            m_out_chan: -1,
            curpos: 0.0,
            loop_: false,
            volume: 1.0,
            peakvol: [0.0; 2],
            preview_track: null_mut(),
        });
        // SAFETY: section critique dans une boite a adresse stable, detruite au Drop.
        unsafe { InitializeCriticalSection(&mut reg.cs) };
        Audition {
            reg,
            src: None,
            playing: false,
            // SAFETY: int PlayTrackPreview2Ex(ReaProject*, preview_register_t*,
            // int flags, double measure_align) ; int StopTrackPreview2(ReaProject*,
            // preview_register_t*).
            play: unsafe { r.api::<PlayTrackFn>(c"PlayTrackPreview2Ex") },
            // SAFETY: voir ci-dessus.
            stop: unsafe { r.api::<StopTrackFn>(c"StopTrackPreview2") },
        }
    }

    /// Fait sonner `pitch` sur la piste `track` (GUID), canal et velocite
    /// donnes. Rend faux si REAPER refuse.
    pub fn start(&mut self, r: &Reaper, track: &str, pitch: i32, chan: i32, vel: i32) -> bool {
        self.stop();
        let (Some(play), Some(tr)) = (self.play, r.track_of(track)) else {
            return false;
        };
        type CreateFn = unsafe extern "C" fn(*const std::ffi::c_char, bool) -> *mut c_void;
        type DestroyFn = unsafe extern "C" fn(*mut c_void);
        // SAFETY: signatures de reaper_plugin_functions.h.
        let (Some(create), Some(destroy)) = (
            unsafe { r.api::<CreateFn>(c"PCM_Source_CreateFromFileEx") },
            unsafe { r.api::<DestroyFn>(c"PCM_Source_Destroy") },
        ) else {
            return false;
        };
        let path = std::env::temp_dir().join("cp_studio_audition.mid");
        if std::fs::write(&path, one_note_smf(pitch, chan, vel, 30.0)).is_err() {
            return false;
        }
        let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) else {
            return false;
        };
        // SAFETY: chaine valide pendant l'appel ; faux = importer le MIDI.
        let p = unsafe { create(c.as_ptr(), false) };
        // SAFETY: source neuve, a nous, detruite par PCM_Source_Destroy.
        let Some(src) = (unsafe { CppSource::from_raw(p, Some(destroy)) }) else {
            return false;
        };
        self.reg.src = src.as_ptr();
        self.reg.preview_track = tr;
        self.reg.m_out_chan = -1;
        self.reg.curpos = 0.0;
        self.src = Some(src);
        // SAFETY: registre et source vivants tant que `self` vit ; `stop`
        // arrete l'apercu avant de liberer la source.
        self.playing = unsafe { play(null_mut(), &mut *self.reg, 0, -1.0) } != 0;
        self.playing
    }

    pub fn stop(&mut self) {
        if self.playing {
            if let Some(stop) = self.stop {
                // SAFETY: le registre lance par `start`.
                unsafe { stop(null_mut(), &mut *self.reg) };
            }
            self.playing = false;
        }
        self.reg.src = null_mut();
        self.src = None;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }
}

impl Drop for Audition {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: initialisee dans `new`, plus utilisee par REAPER (arretee).
        unsafe { DeleteCriticalSection(&mut self.reg.cs) };
    }
}

// ---------------------------------------------------------------------------
// Les fichiers
// ---------------------------------------------------------------------------

type CreateFn = unsafe extern "C" fn(*const std::ffi::c_char, bool) -> *mut c_void;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

/// Ouvre un fichier audio par REAPER (`PCM_Source_CreateFromFileEx`, sans
/// import MIDI). A appeler depuis le fil principal. La source peut ensuite etre
/// lue depuis un fil de travail ; elle est detruite par `PCM_Source_Destroy`.
pub fn open_file(r: &Reaper, path: &str) -> Option<CppSource> {
    // SAFETY: PCM_source* PCM_Source_CreateFromFileEx(const char*, bool).
    let create = unsafe { r.api::<CreateFn>(c"PCM_Source_CreateFromFileEx") }?;
    // SAFETY: void PCM_Source_Destroy(PCM_source*).
    let destroy = unsafe { r.api::<DestroyFn>(c"PCM_Source_Destroy") }?;
    let c = CString::new(path).ok()?;
    // SAFETY: chaine valide pendant l'appel.
    let p = unsafe { create(c.as_ptr(), true) };
    // SAFETY: source neuve, a nous, liberee par PCM_Source_Destroy.
    unsafe { CppSource::from_raw(p, Some(destroy)) }
}

/// Le taux du peripherique audio ouvert (`GetAudioDeviceInfo("SRATE")`).
pub fn device_srate(r: &Reaper) -> Option<f64> {
    type F = unsafe extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_char, i32) -> bool;
    // SAFETY: bool GetAudioDeviceInfo(const char* attribute, char* descOut, int descOut_sz).
    let f = unsafe { r.api::<F>(c"GetAudioDeviceInfo") }?;
    let mut buf = [0u8; 64];
    // SAFETY: tampon de 64 octets, taille passee.
    let ok = unsafe {
        f(
            c"SRATE".as_ptr(),
            buf.as_mut_ptr() as *mut std::ffi::c_char,
            64,
        )
    };
    if !ok {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end]).ok()?.trim().parse().ok()
}
