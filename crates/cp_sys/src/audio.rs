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
        }
    }

    /// Lance l'aperçu vers la sortie materielle `first_out` (0 = sorties 1/2),
    /// sans tampon, immediatement.
    pub fn play_hardware(&mut self, first_out: i32) -> bool {
        if self.playing {
            return true;
        }
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
        if let Some(stop) = self.stop {
            // SAFETY: le registre lance par `play_hardware`.
            unsafe { stop(&mut *self.reg) };
        }
        self.playing = false;
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
