//! Le theme de REAPER vu d'une extension : le fichier du theme courant, et
//! ses couleurs telles que REAPER les applique (reglages du theme compris).
//! La lecture des images est dans `cp_theme` (sans Windows).

use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

use crate::reaper::Reaper;

impl Reaper {
    /// Le fichier du theme courant (`GetLastColorThemeFile`) : un
    /// `.ReaperTheme`, parfois sans fichier a part (le zip du meme nom).
    pub fn theme_file(&self) -> Option<PathBuf> {
        type F = unsafe extern "C" fn() -> *const c_char;
        // SAFETY: const char* GetLastColorThemeFile().
        let f = unsafe { self.api::<F>(c"GetLastColorThemeFile") }?;
        // SAFETY: fil principal ; chaine tenue par REAPER.
        let p = unsafe { f() };
        if p.is_null() {
            return None;
        }
        // SAFETY: chaine terminee par zero.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        (!s.is_empty()).then(|| PathBuf::from(s))
    }

    /// Une couleur du theme (« col_main_bg2 »...) en (r, g, b) ; `None` si
    /// la cle n'existe pas.
    pub fn theme_color(&self, key: &str) -> Option<(u8, u8, u8)> {
        type F = unsafe extern "C" fn(*const c_char, i32) -> i32;
        // SAFETY: int GetThemeColor(const char* ini_key, int flagsOptional).
        let f = unsafe { self.api::<F>(c"GetThemeColor") }?;
        let k = CString::new(key).ok()?;
        // SAFETY: fil principal ; cle terminee par zero.
        let v = unsafe { f(k.as_ptr(), 0) };
        if v < 0 {
            return None;
        }
        // Couleur native de Windows : 0x00BBGGRR.
        Some((v as u8, (v >> 8) as u8, (v >> 16) as u8))
    }
}
