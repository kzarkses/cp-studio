//! Le transport de REAPER vu d'une extension : l'etat de lecture, la position,
//! le tempo, la signature, la vitesse, l'etat des actions, et les petites
//! boites de REAPER (choisir une action, saisir une valeur).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

/// Les bits de `GetPlayState`.
pub mod play {
    pub const PLAYING: i32 = 1;
    pub const PAUSED: i32 = 2;
    pub const RECORDING: i32 = 4;
}

/// Les modes de `format_timestr_pos`.
pub mod fmt {
    pub const TIME: i32 = 0;
    pub const BEATS: i32 = 2;
}

impl Reaper {
    fn call0<T: Default>(&self, name: &CStr) -> T {
        type F<T> = unsafe extern "C" fn() -> T;
        // SAFETY: les appelants nomment une fonction sans argument qui rend T.
        match unsafe { self.api::<F<T>>(name) } {
            // SAFETY: appel sans argument, fil principal.
            Some(f) => unsafe { f() },
            None => T::default(),
        }
    }

    /// Les bits de [`play`] (0 = arrete).
    pub fn play_state(&self) -> i32 {
        self.call0::<i32>(c"GetPlayState")
    }

    /// Ce qu'on ENTEND (la latence de sortie deduite), comme le transport de
    /// REAPER.
    pub fn play_position(&self) -> f64 {
        self.call0::<f64>(c"GetPlayPosition")
    }

    pub fn cursor_position(&self) -> f64 {
        self.call0::<f64>(c"GetCursorPosition")
    }

    /// Une position mise en forme par REAPER (`fmt::BEATS` : « 12.3.45 »).
    pub fn format_time(&self, pos: f64, mode: i32) -> String {
        type F = unsafe extern "C" fn(f64, *mut c_char, i32, i32);
        // SAFETY: void format_timestr_pos(double, char*, int, int).
        let Some(f) = (unsafe { self.api::<F>(c"format_timestr_pos") }) else {
            return String::new();
        };
        let mut buf = [0u8; 64];
        // SAFETY: tampon de 64 octets annonce comme tel.
        unsafe { f(pos, buf.as_mut_ptr() as *mut c_char, 64, mode) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(0);
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }

    /// (numerateur, denominateur, tempo) en `pos`.
    pub fn time_sig_at(&self, pos: f64) -> (i32, i32, f64) {
        type F = unsafe extern "C" fn(P, f64, *mut i32, *mut i32, *mut f64);
        // SAFETY: void TimeMap_GetTimeSigAtTime(ReaProject*, double, int*,
        // int*, double*).
        let Some(f) = (unsafe { self.api::<F>(c"TimeMap_GetTimeSigAtTime") }) else {
            return (4, 4, self.master_tempo());
        };
        let (mut n, mut d, mut t) = (4, 4, 120.0);
        // SAFETY: trois sorties a nous ; projet nul = le projet courant.
        unsafe { f(null_mut(), pos, &mut n, &mut d, &mut t) };
        (n, d, t)
    }

    /// Le tempo du projet (`undo` : un point d'annulation, a la fin d'un
    /// geste seulement).
    pub fn set_bpm(&self, bpm: f64, undo: bool) {
        type F = unsafe extern "C" fn(P, f64, bool);
        // SAFETY: void SetCurrentBPM(ReaProject*, double, bool wantUndo).
        if let Some(f) = unsafe { self.api::<F>(c"SetCurrentBPM") } {
            // SAFETY: projet nul = courant.
            unsafe { f(null_mut(), bpm.clamp(1.0, 960.0), undo) };
        }
    }

    pub fn play_rate(&self) -> f64 {
        type F = unsafe extern "C" fn(P) -> f64;
        // SAFETY: double Master_GetPlayRate(ReaProject*).
        match unsafe { self.api::<F>(c"Master_GetPlayRate") } {
            // SAFETY: projet nul = courant.
            Some(f) => unsafe { f(null_mut()) },
            None => 1.0,
        }
    }

    pub fn set_play_rate(&self, r: f64) {
        type F = unsafe extern "C" fn(f64);
        // SAFETY: void CSurf_OnPlayRateChange(double playrate).
        if let Some(f) = unsafe { self.api::<F>(c"CSurf_OnPlayRateChange") } {
            // SAFETY: fil principal.
            unsafe { f(r.clamp(0.25, 4.0)) };
        }
    }

    /// L'etat d'une action a bascule de la section principale : 1, 0, ou -1.
    pub fn toggle_state(&self, cmd: i32) -> i32 {
        type F = unsafe extern "C" fn(i32) -> i32;
        // SAFETY: int GetToggleCommandState(int command_id).
        match unsafe { self.api::<F>(c"GetToggleCommandState") } {
            // SAFETY: fil principal.
            Some(f) => unsafe { f(cmd) },
            None => -1,
        }
    }

    /// La boucle de lecture (« repeat ») est-elle allumee ?
    pub fn repeat(&self) -> bool {
        type F = unsafe extern "C" fn(i32) -> i32;
        // SAFETY: int GetSetRepeat(int val) ; -1 = lire.
        match unsafe { self.api::<F>(c"GetSetRepeat") } {
            // SAFETY: fil principal.
            Some(f) => (unsafe { f(-1) }) > 0,
            None => false,
        }
    }

    /// Le nom d'une action de la section principale.
    pub fn command_text(&self, cmd: i32) -> String {
        type F = unsafe extern "C" fn(i32, P) -> *const c_char;
        // SAFETY: const char* kbd_getTextFromCmd(int, KbdSectionInfo*).
        let Some(f) = (unsafe { self.api::<F>(c"kbd_getTextFromCmd") }) else {
            return String::new();
        };
        // SAFETY: section nulle = principale ; chaine tenue par REAPER.
        let p = unsafe { f(cmd, null_mut()) };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: chaine terminee par zero.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    /// Le nom stable d'une action (« _RS... », « _SWS_... ») ; `None` pour une
    /// action native (son numero est stable).
    pub fn command_name(&self, cmd: i32) -> Option<String> {
        type F = unsafe extern "C" fn(i32) -> *const c_char;
        // SAFETY: const char* ReverseNamedCommandLookup(int).
        let f = unsafe { self.api::<F>(c"ReverseNamedCommandLookup") }?;
        // SAFETY: fil principal ; chaine tenue par REAPER.
        let p = unsafe { f(cmd) };
        if p.is_null() {
            return None;
        }
        // SAFETY: chaine terminee par zero.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        (!s.is_empty()).then(|| format!("_{s}"))
    }

    /// La liste des actions de REAPER, pour en choisir une : `PromptForAction`.
    /// `mode` 1 ouvre la session, 0 la lit (l'action choisie, 0 si rien encore,
    /// -1 si la liste est fermee), -1 la termine.
    pub fn prompt_for_action(&self, mode: i32) -> i32 {
        type F = unsafe extern "C" fn(i32, i32, i32) -> i32;
        // SAFETY: int PromptForAction(int session_mode, int init_id,
        // int section_id).
        match unsafe { self.api::<F>(c"PromptForAction") } {
            // SAFETY: fil principal ; section 0 = principale.
            Some(f) => unsafe { f(mode, 0, 0) },
            None => -1,
        }
    }

    /// La petite boite de saisie de REAPER (modale) : `captions` et `values`
    /// separes par des virgules. `None` si annulee.
    pub fn user_inputs(&self, title: &str, n: i32, captions: &str, values: &str) -> Option<String> {
        type F = unsafe extern "C" fn(*const c_char, i32, *const c_char, *mut c_char, i32) -> bool;
        // SAFETY: bool GetUserInputs(const char*, int, const char*, char*, int).
        let f = unsafe { self.api::<F>(c"GetUserInputs") }?;
        let (t, c) = (CString::new(title).ok()?, CString::new(captions).ok()?);
        let mut buf = vec![0u8; 1024];
        let v = values.as_bytes();
        let k = v.len().min(1023);
        buf[..k].copy_from_slice(&v[..k]);
        // SAFETY: tampon de 1024 octets annonce comme tel.
        let ok = unsafe {
            f(
                t.as_ptr(),
                n,
                c.as_ptr(),
                buf.as_mut_ptr() as *mut c_char,
                1024,
            )
        };
        if !ok {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }
}
