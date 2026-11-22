//! L'etireur de REAPER (`IReaperPitchShift`, elastique) appele par nous :
//! la vitesse sans changer la hauteur, et la hauteur sans changer la duree,
//! avec la qualite d'un item de REAPER.
//!
//! C'est une classe C++ : on l'appelle par sa table virtuelle, dans l'ordre
//! des methodes de `reaper_plugin.h` (MSVC x64 : `this` en premier argument,
//! le destructeur virtuel occupe une case, le « scalar deleting
//! destructor »). `cp_oracle/tests/pitch_abi.rs` confronte cet ordre au vrai
//! compilateur.

use std::ffi::{CStr, c_char, c_void};

use crate::reaper::Reaper;

/// La version de l'API que l'on demande (`REAPER_PITCHSHIFT_API_VER`).
pub const API_VER: i32 = 0x14;

/// La table virtuelle d'`IReaperPitchShift`, dans l'ordre du SDK.
#[repr(C)]
pub struct PsVtbl {
    /// `virtual ~IReaperPitchShift()` (drapeau &1 : liberer).
    pub dtor: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    pub set_srate: unsafe extern "C" fn(*mut c_void, f64),
    pub set_nch: unsafe extern "C" fn(*mut c_void, i32),
    pub set_shift: unsafe extern "C" fn(*mut c_void, f64),
    pub set_formant_shift: unsafe extern "C" fn(*mut c_void, f64),
    pub set_tempo: unsafe extern "C" fn(*mut c_void, f64),
    pub reset: unsafe extern "C" fn(*mut c_void),
    pub get_buffer: unsafe extern "C" fn(*mut c_void, i32) -> *mut f64,
    pub buffer_done: unsafe extern "C" fn(*mut c_void, i32),
    pub flush_samples: unsafe extern "C" fn(*mut c_void),
    pub is_reset: unsafe extern "C" fn(*mut c_void) -> bool,
    pub get_samples: unsafe extern "C" fn(*mut c_void, i32, *mut f64) -> i32,
    pub set_quality: unsafe extern "C" fn(*mut c_void, i32),
    pub extended:
        unsafe extern "C" fn(*mut c_void, i32, *mut c_void, *mut c_void, *mut c_void) -> i32,
}

/// Un etireur de REAPER, possede (libere au `Drop`). Utilise par un seul
/// fil a la fois.
pub struct PitchShift {
    ptr: *mut c_void,
}

// SAFETY: un etireur n'est touche que par un fil a la fois (`&mut self`
// partout) ; REAPER les fait tourner dans ses propres fils.
unsafe impl Send for PitchShift {}

impl PitchShift {
    /// Enveloppe un objet existant (possede : libere au `Drop`).
    ///
    /// # Safety
    /// `ptr` est un `IReaperPitchShift*` vivant que personne d'autre ne
    /// liberera.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Option<PitchShift> {
        (!ptr.is_null()).then_some(PitchShift { ptr })
    }

    fn vt(&self) -> &PsVtbl {
        // SAFETY: un objet C++ polymorphe commence par sa table (contrat de
        // `from_raw`).
        unsafe { &**(self.ptr as *const *const PsVtbl) }
    }

    pub fn set_srate(&mut self, sr: f64) {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().set_srate)(self.ptr, sr) }
    }

    pub fn set_nch(&mut self, n: i32) {
        // SAFETY: idem.
        unsafe { (self.vt().set_nch)(self.ptr, n) }
    }

    /// La hauteur, en rapport (2 = une octave au-dessus).
    pub fn set_shift(&mut self, s: f64) {
        // SAFETY: idem.
        unsafe { (self.vt().set_shift)(self.ptr, s) }
    }

    pub fn set_formant_shift(&mut self, s: f64) {
        // SAFETY: idem.
        unsafe { (self.vt().set_formant_shift)(self.ptr, s) }
    }

    /// La vitesse (1,5 = la matiere passe une fois et demie plus vite, la
    /// sortie dure 1/1,5).
    pub fn set_tempo(&mut self, t: f64) {
        // SAFETY: idem.
        unsafe { (self.vt().set_tempo)(self.ptr, t) }
    }

    pub fn reset(&mut self) {
        // SAFETY: idem.
        unsafe { (self.vt().reset)(self.ptr) }
    }

    /// Donne `input` (entrelace, `nch` canaux deja annonces) a l'etireur.
    pub fn feed(&mut self, input: &[f64], nch: usize) {
        let frames = input.len() / nch.max(1);
        if frames == 0 {
            return;
        }
        // SAFETY: l'objet rend un tampon d'au moins `frames * nch` doubles,
        // valable jusqu'a `BufferDone`.
        unsafe {
            let b = (self.vt().get_buffer)(self.ptr, frames as i32);
            if b.is_null() {
                return;
            }
            std::ptr::copy_nonoverlapping(input.as_ptr(), b, frames * nch);
            (self.vt().buffer_done)(self.ptr, frames as i32);
        }
    }

    /// Tout ce qui est donne doit sortir (la fin de la matiere).
    pub fn flush(&mut self) {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().flush_samples)(self.ptr) }
    }

    pub fn is_reset(&mut self) -> bool {
        // SAFETY: idem.
        unsafe { (self.vt().is_reset)(self.ptr) }
    }

    /// Prend ce qui est pret (au plus `out.len() / nch` frames) ; rend le
    /// nombre de frames.
    pub fn take(&mut self, out: &mut [f64], nch: usize) -> usize {
        let want = out.len() / nch.max(1);
        if want == 0 {
            return 0;
        }
        // SAFETY: le tampon tient `want * nch` doubles.
        let n = unsafe { (self.vt().get_samples)(self.ptr, want as i32, out.as_mut_ptr()) };
        n.clamp(0, want as i32) as usize
    }

    /// `(mode << 16) + sous-mode`, ou -1 : le reglage du projet.
    pub fn set_quality(&mut self, q: i32) {
        // SAFETY: idem.
        unsafe { (self.vt().set_quality)(self.ptr, q) }
    }

    pub fn extended(&mut self, call: i32) -> i32 {
        // SAFETY: idem ; aucun parametre pointeur passe.
        unsafe {
            (self.vt().extended)(
                self.ptr,
                call,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for PitchShift {
    fn drop(&mut self) {
        // SAFETY: destructeur virtuel, drapeau 1 : l'objet se libere avec
        // l'allocateur de celui qui l'a cree (REAPER).
        unsafe {
            (self.vt().dtor)(self.ptr, 1);
        }
    }
}

impl Reaper {
    /// Un etireur neuf (elastique, le reglage du projet), ou rien.
    pub fn pitch_shift(&self) -> Option<PitchShift> {
        type F = unsafe extern "C" fn(i32) -> *mut c_void;
        // SAFETY: fonction du SDK, nommee avec sa signature.
        let f = unsafe { self.api::<F>(c"ReaperGetPitchShiftAPI") }?;
        // SAFETY: fonction du SDK, version demandee.
        let p = unsafe { f(API_VER) };
        // SAFETY: un objet neuf, a nous.
        unsafe { PitchShift::from_raw(p) }
    }

    /// Le nom d'un mode d'etirement (« elastique 3.3.3 Pro »...), ou rien
    /// au-dela du dernier.
    pub fn pitch_mode_name(&self, mode: i32) -> Option<String> {
        type F = unsafe extern "C" fn(i32, *mut *const c_char) -> bool;
        // SAFETY: idem.
        let f = unsafe { self.api::<F>(c"EnumPitchShiftModes") }?;
        let mut s: *const c_char = std::ptr::null();
        // SAFETY: ecrit un pointeur vers une chaine tenue par REAPER.
        let ok = unsafe { f(mode, &mut s) };
        if !ok {
            return None;
        }
        if s.is_null() {
            return Some(String::new());
        }
        // SAFETY: chaine C tenue par REAPER.
        Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
    }
}
