//! Ce que FX Constellation lit et ecrit dans REAPER : les parametres des
//! effets d'une piste (normalises, bruts, affiches), leurs reglages nommes
//! (les liens de parametres `param.N.plink.*`, les etats d'un effet), le GUID
//! d'un effet, les donnees d'une piste (`P_EXT`) et du projet, les presets
//! d'un effet.
//!
//! Une piste se tient par un [`Tk`] : son pointeur, verifie par
//! `ValidatePtr2` a chaque appel (REAPER libere une piste supprimee ; un
//! pointeur perime ne doit jamais etre appele).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

/// Une piste, tenue par son pointeur (verifie a chaque appel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tk(usize);

/// Ce que `GetLastTouchedFX` rend : la piste (1 = la premiere, 0 = la
/// maitre), l'effet et le parametre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touched {
    pub track: i32,
    pub fx: i32,
    pub param: i32,
}

fn cstr(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

impl Reaper {
    fn xf<F: Copy>(&self, name: &CStr) -> Option<F> {
        // SAFETY: les appelants nomment une fonction du SDK avec sa signature.
        unsafe { self.api::<F>(name) }
    }

    /// Le pointeur de la piste, s'il est toujours une piste du projet.
    fn xp(&self, t: Tk) -> Option<P> {
        type Valid = unsafe extern "C" fn(P, P, *const c_char) -> bool;
        let v: Valid = self.xf(c"ValidatePtr2")?;
        let p = t.0 as P;
        if p.is_null() {
            return None;
        }
        // SAFETY: projet nul = courant ; ValidatePtr2 accepte tout pointeur.
        unsafe { v(null_mut(), p, c"MediaTrack*".as_ptr()) }.then_some(p)
    }

    /// La piste d'un GUID (« MASTER » : la piste maitre).
    pub fn xc_track(&self, guid: &str) -> Option<Tk> {
        self.fb_track_ptr(guid).map(|p| Tk(p as usize))
    }

    /// La piste choisie en premier (la maitre si elle seule l'est).
    pub fn xc_selected(&self) -> Option<Tk> {
        self.fx_selected_track().and_then(|g| self.xc_track(&g))
    }

    /// Le GUID de la piste (« MASTER » pour la maitre).
    pub fn xc_guid(&self, t: Tk) -> Option<String> {
        let p = self.xp(t)?;
        self.fb_guid_of(p)
    }

    /// Le nom de la piste et sa couleur.
    pub fn xc_track_name(&self, t: Tk) -> (String, Option<(u8, u8, u8)>) {
        let Some(g) = self.xc_guid(t) else {
            return (String::new(), None);
        };
        self.fx_track(&g)
            .map(|f| (f.name, f.color))
            .unwrap_or_default()
    }

    /// Le numero de la piste (1 = la premiere, 0 = la maitre).
    pub fn xc_track_number(&self, t: Tk) -> i32 {
        type Num = unsafe extern "C" fn(P, *const c_char) -> f64;
        match (self.xp(t), self.xf::<Num>(c"GetMediaTrackInfo_Value")) {
            (Some(p), Some(f)) => {
                // SAFETY: piste vivante.
                let n = unsafe { f(p, c"IP_TRACKNUMBER".as_ptr()) } as i32;
                if n < 0 { 0 } else { n }
            }
            _ => -1,
        }
    }

    /// La piste d'un numero (0 = la maitre).
    pub fn xc_track_at(&self, n: i32) -> Option<Tk> {
        self.fx_track_by_number(n).and_then(|g| self.xc_track(&g))
    }

    // -------------------------------------------------------------------
    // Les effets
    // -------------------------------------------------------------------

    pub fn xc_fx_count(&self, t: Tk) -> usize {
        type F = unsafe extern "C" fn(P) -> i32;
        match (self.xp(t), self.xf::<F>(c"TrackFX_GetCount")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p) }.max(0) as usize,
            _ => 0,
        }
    }

    /// Le nom montre (le nom donne par l'utilisateur s'il l'a renomme).
    pub fn xc_fx_name(&self, t: Tk, fx: usize) -> String {
        type F = unsafe extern "C" fn(P, i32, *mut c_char, i32) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetFXName")) else {
            return String::new();
        };
        let mut buf = [0u8; 512];
        // SAFETY: piste vivante ; tampon de 512 octets.
        if unsafe { f(p, fx as i32, buf.as_mut_ptr() as *mut c_char, 512) } {
            cstr(&buf)
        } else {
            String::new()
        }
    }

    /// Le GUID d'un effet, « {...} ».
    pub fn xc_fx_guid(&self, t: Tk, fx: usize) -> Option<String> {
        type G = unsafe extern "C" fn(P, i32) -> P;
        type S = unsafe extern "C" fn(P, *mut c_char);
        let p = self.xp(t)?;
        let g: G = self.xf(c"TrackFX_GetFXGUID")?;
        let s: S = self.xf(c"guidToString")?;
        // SAFETY: piste vivante.
        let id = unsafe { g(p, fx as i32) };
        if id.is_null() {
            return None;
        }
        let mut buf = [0u8; 64];
        // SAFETY: GUID de REAPER ; tampon de 64 octets (en demande 39).
        unsafe { s(id, buf.as_mut_ptr() as *mut c_char) };
        let out = cstr(&buf);
        (!out.is_empty()).then_some(out)
    }

    /// La place d'un effet par son GUID.
    pub fn xc_fx_index(&self, t: Tk, guid: &str) -> Option<usize> {
        (0..self.xc_fx_count(t)).find(|&i| self.xc_fx_guid(t, i).as_deref() == Some(guid))
    }

    pub fn xc_fx_enabled(&self, t: Tk, fx: usize) -> bool {
        type F = unsafe extern "C" fn(P, i32) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_GetEnabled")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32) },
            _ => false,
        }
    }

    pub fn xc_fx_set_enabled(&self, t: Tk, fx: usize, on: bool) {
        type F = unsafe extern "C" fn(P, i32, bool);
        if let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_SetEnabled")) {
            // SAFETY: piste vivante.
            unsafe { f(p, fx as i32, on) };
        }
    }

    pub fn xc_fx_offline(&self, t: Tk, fx: usize) -> bool {
        type F = unsafe extern "C" fn(P, i32) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_GetOffline")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32) },
            _ => false,
        }
    }

    /// Ajoute un effet a la place `at` (aucune : a la fin). Rend sa place.
    pub fn xc_fx_add(&self, t: Tk, name: &str, at: Option<usize>) -> Option<usize> {
        type F = unsafe extern "C" fn(P, *const c_char, bool, i32) -> i32;
        let p = self.xp(t)?;
        let f: F = self.xf(c"TrackFX_AddByName")?;
        let c = CString::new(name).ok()?;
        let inst = at.map_or(-1, |k| -1000 - k.min(4000) as i32);
        // SAFETY: piste vivante ; nom termine par zero.
        let k = unsafe { f(p, c.as_ptr(), false, inst) };
        (k >= 0).then_some(k as usize)
    }

    /// La place du premier effet de ce nom (sans en ajouter).
    pub fn xc_fx_find(&self, t: Tk, name: &str) -> Option<usize> {
        type F = unsafe extern "C" fn(P, *const c_char, bool, i32) -> i32;
        let p = self.xp(t)?;
        let f: F = self.xf(c"TrackFX_AddByName")?;
        let c = CString::new(name).ok()?;
        // SAFETY: piste vivante ; 0 = chercher seulement.
        let k = unsafe { f(p, c.as_ptr(), false, 0) };
        (k >= 0).then_some(k as usize)
    }

    pub fn xc_fx_delete(&self, t: Tk, fx: usize) -> bool {
        type F = unsafe extern "C" fn(P, i32) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_Delete")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32) },
            _ => false,
        }
    }

    /// Deplace l'effet `from` pour qu'il finisse a la place `to` (0..n-1).
    pub fn xc_fx_move(&self, t: Tk, from: usize, to: usize) {
        type F = unsafe extern "C" fn(P, i32, P, i32, bool);
        if from == to {
            return;
        }
        if let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_CopyToTrack")) {
            // SAFETY: piste vivante ; deplacement dans la meme chaine.
            unsafe { f(p, from as i32, p, to as i32, true) };
        }
    }

    /// Montre (ou cache) la fenetre flottante d'un effet.
    pub fn xc_fx_float(&self, t: Tk, fx: usize, show: bool) {
        type F = unsafe extern "C" fn(P, i32, i32);
        if let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_Show")) {
            // SAFETY: piste vivante ; 3 = flottante, 2 = la cacher.
            unsafe { f(p, fx as i32, if show { 3 } else { 2 }) };
        }
    }

    // -------------------------------------------------------------------
    // Les parametres
    // -------------------------------------------------------------------

    pub fn xc_param_count(&self, t: Tk, fx: usize) -> usize {
        type F = unsafe extern "C" fn(P, i32) -> i32;
        match (self.xp(t), self.xf::<F>(c"TrackFX_GetNumParams")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32) }.max(0) as usize,
            _ => 0,
        }
    }

    pub fn xc_param_name(&self, t: Tk, fx: usize, k: usize) -> String {
        type F = unsafe extern "C" fn(P, i32, i32, *mut c_char, i32) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetParamName")) else {
            return String::new();
        };
        let mut buf = [0u8; 256];
        // SAFETY: piste vivante ; tampon de 256 octets.
        if unsafe { f(p, fx as i32, k as i32, buf.as_mut_ptr() as *mut c_char, 256) } {
            cstr(&buf)
        } else {
            String::new()
        }
    }

    /// La valeur normalisee de REAPER (0..1).
    pub fn xc_get(&self, t: Tk, fx: usize, k: usize) -> f64 {
        type F = unsafe extern "C" fn(P, i32, i32) -> f64;
        match (self.xp(t), self.xf::<F>(c"TrackFX_GetParamNormalized")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32, k as i32) },
            _ => 0.0,
        }
    }

    /// Ecrit la valeur normalisee de REAPER (0..1).
    pub fn xc_set(&self, t: Tk, fx: usize, k: usize, v: f64) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32, f64) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_SetParamNormalized")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32, k as i32, v.clamp(0.0, 1.0)) },
            _ => false,
        }
    }

    /// La valeur brute, et ses bornes (un curseur JSFX : sa valeur ; ⚠ un
    /// curseur `:log` ou `:sqr` parle autrement, passer par le normalise).
    pub fn xc_get_raw(&self, t: Tk, fx: usize, k: usize) -> (f64, f64, f64) {
        type F = unsafe extern "C" fn(P, i32, i32, *mut f64, *mut f64) -> f64;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetParam")) else {
            return (0.0, 0.0, 1.0);
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        // SAFETY: piste vivante ; deux f64 a nous.
        let v = unsafe { f(p, fx as i32, k as i32, &mut lo, &mut hi) };
        (v, lo, hi)
    }

    /// Ecrit la valeur brute.
    pub fn xc_set_raw(&self, t: Tk, fx: usize, k: usize, v: f64) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32, f64) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_SetParam")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32, k as i32, v) },
            _ => false,
        }
    }

    /// La valeur telle que l'effet l'affiche.
    pub fn xc_format(&self, t: Tk, fx: usize, k: usize) -> String {
        type F = unsafe extern "C" fn(P, i32, i32, *mut c_char, i32) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetFormattedParamValue"))
        else {
            return String::new();
        };
        let mut buf = [0u8; 128];
        // SAFETY: piste vivante ; tampon de 128 octets.
        if unsafe { f(p, fx as i32, k as i32, buf.as_mut_ptr() as *mut c_char, 128) } {
            cstr(&buf)
        } else {
            String::new()
        }
    }

    /// Une valeur normalisee telle que l'effet l'afficherait.
    pub fn xc_format_at(&self, t: Tk, fx: usize, k: usize, v: f64) -> Option<String> {
        type F = unsafe extern "C" fn(P, i32, i32, f64, *mut c_char, i32) -> bool;
        let p = self.xp(t)?;
        let f: F = self.xf(c"TrackFX_FormatParamValueNormalized")?;
        let mut buf = [0u8; 128];
        // SAFETY: piste vivante ; tampon de 128 octets.
        unsafe {
            f(
                p,
                fx as i32,
                k as i32,
                v,
                buf.as_mut_ptr() as *mut c_char,
                128,
            )
        }
        .then(|| cstr(&buf))
        .filter(|s| !s.is_empty())
    }

    /// Le parametre n'a que deux etats (un interrupteur).
    pub fn xc_is_toggle(&self, t: Tk, fx: usize, k: usize) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32, *mut f64, *mut f64, *mut f64, *mut bool) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetParameterStepSizes"))
        else {
            return false;
        };
        let (mut a, mut b, mut c, mut tg) = (0.0, 0.0, 0.0, false);
        // SAFETY: piste vivante ; sorties a nous.
        let ok = unsafe { f(p, fx as i32, k as i32, &mut a, &mut b, &mut c, &mut tg) };
        ok && tg
    }

    /// Le nombre de pas d'un parametre : 0 continu, 2 un interrupteur, N un
    /// choix (d'apres le pas que REAPER donne et les bornes).
    pub fn xc_step_count(&self, t: Tk, fx: usize, k: usize) -> u32 {
        type F = unsafe extern "C" fn(P, i32, i32, *mut f64, *mut f64, *mut f64, *mut bool) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetParameterStepSizes"))
        else {
            return 0;
        };
        let (mut step, mut b, mut c, mut tg) = (0.0, 0.0, 0.0, false);
        // SAFETY: piste vivante ; sorties a nous.
        let ok = unsafe { f(p, fx as i32, k as i32, &mut step, &mut b, &mut c, &mut tg) };
        if !ok {
            return 0;
        }
        if tg {
            return 2;
        }
        if step <= 0.0 {
            return 0;
        }
        let (_, lo, hi) = self.xc_get_raw(t, fx, k);
        let n = ((hi - lo).abs() / step).round() + 1.0;
        if n.is_finite() && n >= 2.0 {
            n.min(1e6) as u32
        } else {
            0
        }
    }

    /// La fin d'un geste sur un parametre (l'automation en mode Touch).
    pub fn xc_end_edit(&self, t: Tk, fx: usize, k: usize) {
        type F = unsafe extern "C" fn(P, i32, i32) -> bool;
        if let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_EndParamEdit")) {
            // SAFETY: piste vivante.
            unsafe { f(p, fx as i32, k as i32) };
        }
    }

    // -------------------------------------------------------------------
    // Les reglages nommes (liens, etats)
    // -------------------------------------------------------------------

    /// Un reglage nomme d'un effet (`param.3.plink.active`, `fx_name`,
    /// `vst_chunk`…). Le tampon grandit jusqu'a 32 Mo pour un etat.
    pub fn xc_cfg(&self, t: Tk, fx: usize, key: &str) -> Option<String> {
        type F = unsafe extern "C" fn(P, i32, *const c_char, *mut c_char, i32) -> bool;
        let p = self.xp(t)?;
        let f: F = self.xf(c"TrackFX_GetNamedConfigParm")?;
        let k = CString::new(key).ok()?;
        let mut cap = 4096usize;
        loop {
            let mut buf = vec![0u8; cap];
            // SAFETY: piste vivante ; tampon de `cap` octets.
            let ok = unsafe {
                f(
                    p,
                    fx as i32,
                    k.as_ptr(),
                    buf.as_mut_ptr() as *mut c_char,
                    cap as i32,
                )
            };
            if !ok {
                return None;
            }
            let len = buf.iter().position(|&b| b == 0).unwrap_or(cap);
            if len + 1 < cap || cap >= 32 << 20 {
                buf.truncate(len);
                return Some(String::from_utf8_lossy(&buf).into_owned());
            }
            cap *= 8;
        }
    }

    pub fn xc_set_cfg(&self, t: Tk, fx: usize, key: &str, value: &str) -> bool {
        type F = unsafe extern "C" fn(P, i32, *const c_char, *const c_char) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_SetNamedConfigParm")) else {
            return false;
        };
        let (Ok(k), Ok(v)) = (CString::new(key), CString::new(value)) else {
            return false;
        };
        // SAFETY: piste vivante ; chaines terminees par zero.
        unsafe { f(p, fx as i32, k.as_ptr(), v.as_ptr()) }
    }

    // -------------------------------------------------------------------
    // Les presets d'un effet
    // -------------------------------------------------------------------

    /// (le preset choisi, le nombre de presets) ; -1 : aucun.
    pub fn xc_preset_index(&self, t: Tk, fx: usize) -> (i32, i32) {
        type F = unsafe extern "C" fn(P, i32, *mut i32) -> i32;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetPresetIndex")) else {
            return (-1, 0);
        };
        let mut n = 0;
        // SAFETY: piste vivante.
        let i = unsafe { f(p, fx as i32, &mut n) };
        (i, n)
    }

    /// Le nom du preset choisi (vide : aucun, ou modifie).
    pub fn xc_preset_name(&self, t: Tk, fx: usize) -> String {
        type F = unsafe extern "C" fn(P, i32, *mut c_char, i32) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"TrackFX_GetPreset")) else {
            return String::new();
        };
        let mut buf = [0u8; 512];
        // SAFETY: piste vivante ; tampon de 512 octets.
        unsafe { f(p, fx as i32, buf.as_mut_ptr() as *mut c_char, 512) };
        cstr(&buf)
    }

    pub fn xc_preset_set_index(&self, t: Tk, fx: usize, i: i32) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_SetPresetByIndex")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32, i) },
            _ => false,
        }
    }

    /// Le preset suivant (+1) ou precedent (-1).
    pub fn xc_preset_step(&self, t: Tk, fx: usize, d: i32) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32) -> bool;
        match (self.xp(t), self.xf::<F>(c"TrackFX_NavigatePresets")) {
            // SAFETY: piste vivante.
            (Some(p), Some(f)) => unsafe { f(p, fx as i32, d) },
            _ => false,
        }
    }

    // -------------------------------------------------------------------
    // Les donnees d'une piste et du projet
    // -------------------------------------------------------------------

    /// L'etat entier de la piste (`GetTrackStateChunk`), quelle que soit sa
    /// taille : le tampon grandit tant que REAPER le remplit.
    pub fn xc_chunk(&self, t: Tk) -> Option<String> {
        type F = unsafe extern "C" fn(P, *mut c_char, i32, bool) -> bool;
        let p = self.xp(t)?;
        let f: F = self.xf(c"GetTrackStateChunk")?;
        let mut cap: usize = 1 << 20;
        loop {
            let mut buf = vec![0u8; cap];
            // SAFETY: piste vivante ; tampon de `cap` octets ; false = l'etat
            // complet (pas celui, allege, de l'annulation).
            let ok = unsafe { f(p, buf.as_mut_ptr() as *mut c_char, cap as i32, false) };
            let n = buf.iter().position(|&b| b == 0).unwrap_or(cap);
            if ok && n + 1 < cap {
                return Some(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            if cap >= 1 << 28 {
                return None;
            }
            cap *= 4;
        }
    }

    /// Remplace l'etat entier de la piste (`SetTrackStateChunk`).
    pub fn xc_set_chunk(&self, t: Tk, chunk: &str) -> bool {
        type F = unsafe extern "C" fn(P, *const c_char, bool) -> bool;
        let (Some(p), Some(f), Ok(c)) = (
            self.xp(t),
            self.xf::<F>(c"SetTrackStateChunk"),
            CString::new(chunk.replace('\0', " ")),
        ) else {
            return false;
        };
        // SAFETY: piste vivante ; chaine terminee par zero ; false = pas un
        // etat d'annulation.
        unsafe { f(p, c.as_ptr(), false) }
    }

    /// Un GUID neuf, en texte (`{XXXXXXXX-...}`).
    pub fn xc_new_guid(&self) -> Option<String> {
        type Gen = unsafe extern "C" fn(*mut [u8; 16]);
        type ToStr = unsafe extern "C" fn(*const [u8; 16], *mut c_char);
        let g: Gen = self.xf(c"genGuid")?;
        let s: ToStr = self.xf(c"guidToString")?;
        let mut id = [0u8; 16];
        let mut buf = [0u8; 64];
        // SAFETY: un GUID de 16 octets a nous ; guidToString ecrit au plus
        // 39 octets dans un tampon de 64.
        unsafe {
            g(&mut id);
            s(&id, buf.as_mut_ptr() as *mut c_char);
        }
        let out = cstr(&buf);
        (out.len() == 38).then_some(out)
    }

    /// `P_EXT:<key>` de la piste. `GetSetMediaTrackInfo_String` ne recoit
    /// pas la taille du tampon : il en a 64 Ko (nos valeurs de piste restent
    /// courtes ; l'etat long de FX Constellation va dans le projet).
    pub fn xc_track_ext(&self, t: Tk, key: &str) -> Option<String> {
        type F = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let p = self.xp(t)?;
        let f: F = self.xf(c"GetSetMediaTrackInfo_String")?;
        let k = CString::new(format!("P_EXT:{key}")).ok()?;
        let mut buf = vec![0u8; 64 << 10];
        // SAFETY: piste vivante ; tampon de 64 Ko termine par zero.
        let ok = unsafe { f(p, k.as_ptr(), buf.as_mut_ptr() as *mut c_char, false) };
        if !ok {
            return None;
        }
        let s = cstr(&buf);
        (!s.is_empty()).then_some(s)
    }

    pub fn xc_set_track_ext(&self, t: Tk, key: &str, value: &str) -> bool {
        type F = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"GetSetMediaTrackInfo_String")) else {
            return false;
        };
        let (Ok(k), Ok(v)) = (
            CString::new(format!("P_EXT:{key}")),
            CString::new(value.replace('\0', " ")),
        ) else {
            return false;
        };
        let mut b = v.into_bytes_with_nul();
        // SAFETY: piste vivante ; en ecriture REAPER lit la chaine.
        unsafe { f(p, k.as_ptr(), b.as_mut_ptr() as *mut c_char, true) }
    }

    /// Une valeur du projet (`GetProjExtState`), jusqu'a 1 Mo.
    pub fn xc_proj_ext(&self, section: &str, key: &str) -> Option<String> {
        type F = unsafe extern "C" fn(P, *const c_char, *const c_char, *mut c_char, i32) -> i32;
        let f: F = self.xf(c"GetProjExtState")?;
        let (s, k) = (CString::new(section).ok()?, CString::new(key).ok()?);
        let mut cap = 16 << 10;
        loop {
            let mut buf = vec![0u8; cap];
            // SAFETY: projet nul = courant ; tampon de `cap` octets.
            let n = unsafe {
                f(
                    null_mut(),
                    s.as_ptr(),
                    k.as_ptr(),
                    buf.as_mut_ptr() as *mut c_char,
                    cap as i32,
                )
            };
            if n <= 0 {
                return None;
            }
            let len = buf.iter().position(|&b| b == 0).unwrap_or(cap);
            if len + 1 < cap || cap >= 1 << 20 {
                buf.truncate(len);
                return Some(String::from_utf8_lossy(&buf).into_owned());
            }
            cap *= 8;
        }
    }

    pub fn xc_set_proj_ext(&self, section: &str, key: &str, value: &str) {
        type F = unsafe extern "C" fn(P, *const c_char, *const c_char, *const c_char) -> i32;
        let Some(f) = self.xf::<F>(c"SetProjExtState") else {
            return;
        };
        let (Ok(s), Ok(k), Ok(v)) = (
            CString::new(section),
            CString::new(key),
            CString::new(value),
        ) else {
            return;
        };
        // SAFETY: projet nul = courant ; chaines terminees par zero.
        unsafe { f(null_mut(), s.as_ptr(), k.as_ptr(), v.as_ptr()) };
    }

    // -------------------------------------------------------------------
    // Le reste
    // -------------------------------------------------------------------

    /// Le dernier parametre touche a la souris dans une fenetre d'effet.
    pub fn xc_last_touched(&self) -> Option<Touched> {
        type F = unsafe extern "C" fn(*mut i32, *mut i32, *mut i32) -> bool;
        let f: F = self.xf(c"GetLastTouchedFX")?;
        let (mut tr, mut fx, mut pa) = (0, 0, 0);
        // SAFETY: sorties a nous.
        unsafe { f(&mut tr, &mut fx, &mut pa) }.then_some(Touched {
            track: tr,
            fx,
            param: pa,
        })
    }

    /// Le debut d'un pas d'annulation (le fermer par [`Reaper::xc_undo_end`]).
    pub fn xc_undo_begin(&self) {
        type B = unsafe extern "C" fn(P);
        if let Some(b) = self.xf::<B>(c"Undo_BeginBlock2") {
            // SAFETY: projet nul = courant.
            unsafe { b(null_mut()) };
        }
    }

    /// La fin d'un pas d'annulation : tout ce qui a change depuis le debut.
    pub fn xc_undo_end(&self, name: &str) {
        type E = unsafe extern "C" fn(P, *const c_char, i32);
        if let (Some(e), Ok(n)) = (self.xf::<E>(c"Undo_EndBlock2"), CString::new(name)) {
            // SAFETY: projet nul = courant ; -1 = tout ce qui a change.
            unsafe { e(null_mut(), n.as_ptr(), -1) };
        }
    }

    /// Un pas d'annulation autour de `f`.
    pub fn xc_undo<R>(&self, name: &str, f: impl FnOnce() -> R) -> R {
        type B = unsafe extern "C" fn(P);
        type E = unsafe extern "C" fn(P, *const c_char, i32);
        let b = self.xf::<B>(c"Undo_BeginBlock2");
        let e = self.xf::<E>(c"Undo_EndBlock2");
        if let Some(b) = b {
            // SAFETY: projet nul = courant.
            unsafe { b(null_mut()) };
        }
        let out = f();
        if let (Some(e), Ok(n)) = (e, CString::new(name)) {
            // SAFETY: idem ; -1 = tout ce qui a change.
            unsafe { e(null_mut(), n.as_ptr(), -1) };
        }
        out
    }

    // -------------------------------------------------------------------
    // Les pistes du projet, la piste cachee « CP MOD », ses envois
    // -------------------------------------------------------------------

    pub fn xc_track_count(&self) -> usize {
        type F = unsafe extern "C" fn(P) -> i32;
        match self.xf::<F>(c"CountTracks") {
            // SAFETY: projet nul = courant.
            Some(f) => unsafe { f(null_mut()) }.max(0) as usize,
            None => 0,
        }
    }

    /// La piste `i` (0 = la premiere ; la maitre n'en fait pas partie).
    pub fn xc_track_index(&self, i: usize) -> Option<Tk> {
        type F = unsafe extern "C" fn(P, i32) -> P;
        let f: F = self.xf(c"GetTrack")?;
        // SAFETY: projet nul = courant ; indice verifie par REAPER.
        let p = unsafe { f(null_mut(), i as i32) };
        (!p.is_null()).then_some(Tk(p as usize))
    }

    /// Le nom d'une piste (sans repli « Track N »).
    pub fn xc_track_raw_name(&self, t: Tk) -> String {
        type F = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let (Some(p), Some(f)) = (self.xp(t), self.xf::<F>(c"GetSetMediaTrackInfo_String")) else {
            return String::new();
        };
        let mut buf = vec![0u8; 1024];
        // SAFETY: piste vivante ; tampon de 1 Ko (un nom de piste est court).
        if unsafe {
            f(
                p,
                c"P_NAME".as_ptr(),
                buf.as_mut_ptr() as *mut c_char,
                false,
            )
        } {
            cstr(&buf)
        } else {
            String::new()
        }
    }

    pub fn xc_set_track_name(&self, t: Tk, name: &str) {
        type F = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        if let (Some(p), Some(f), Ok(v)) = (
            self.xp(t),
            self.xf::<F>(c"GetSetMediaTrackInfo_String"),
            CString::new(name),
        ) {
            let mut b = v.into_bytes_with_nul();
            // SAFETY: piste vivante ; en ecriture REAPER lit la chaine.
            unsafe { f(p, c"P_NAME".as_ptr(), b.as_mut_ptr() as *mut c_char, true) };
        }
    }

    /// Une valeur d'une piste (`GetMediaTrackInfo_Value`).
    pub fn xc_track_value(&self, t: Tk, key: &str) -> f64 {
        type F = unsafe extern "C" fn(P, *const c_char) -> f64;
        let (Some(p), Some(f), Ok(k)) = (
            self.xp(t),
            self.xf::<F>(c"GetMediaTrackInfo_Value"),
            CString::new(key),
        ) else {
            return 0.0;
        };
        // SAFETY: piste vivante ; cle terminee par zero.
        unsafe { f(p, k.as_ptr()) }
    }

    pub fn xc_set_track_value(&self, t: Tk, key: &str, v: f64) {
        type F = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        if let (Some(p), Some(f), Ok(k)) = (
            self.xp(t),
            self.xf::<F>(c"SetMediaTrackInfo_Value"),
            CString::new(key),
        ) {
            // SAFETY: piste vivante ; cle terminee par zero.
            unsafe { f(p, k.as_ptr(), v) };
        }
    }

    /// Une piste neuve a la fin du projet.
    pub fn xc_insert_track_end(&self) -> Option<Tk> {
        type Ins = unsafe extern "C" fn(i32, bool);
        let n = self.xc_track_count();
        let ins: Ins = self.xf(c"InsertTrackAtIndex")?;
        // SAFETY: indice = le nombre de pistes (a la fin).
        unsafe { ins(n as i32, false) };
        self.xc_track_index(n)
    }

    /// Les pistes que `src` envoie (ses envois).
    pub fn xc_send_dests(&self, src: Tk) -> Vec<Tk> {
        type Num = unsafe extern "C" fn(P, i32) -> i32;
        type Info = unsafe extern "C" fn(P, i32, i32, *const c_char) -> f64;
        let (Some(p), Some(num), Some(info)) = (
            self.xp(src),
            self.xf::<Num>(c"GetTrackNumSends"),
            self.xf::<Info>(c"GetTrackSendInfo_Value"),
        ) else {
            return Vec::new();
        };
        // SAFETY: piste vivante ; 0 = envois.
        let n = unsafe { num(p, 0) };
        (0..n)
            .map(|i| {
                // SAFETY: indice dans les bornes ; le pointeur rendu est
                // celui d'une piste (en double).
                let d = unsafe { info(p, 0, i, c"P_DESTTRACK".as_ptr()) };
                Tk(d as usize)
            })
            .collect()
    }

    /// Un envoi MIDI seul (sans audio) de `src` vers `dst`. Rend vrai s'il
    /// vient d'etre cree.
    pub fn xc_ensure_midi_send(&self, src: Tk, dst: Tk) -> bool {
        type Send = unsafe extern "C" fn(P, P) -> i32;
        type Set = unsafe extern "C" fn(P, i32, i32, *const c_char, f64) -> bool;
        if self.xc_send_dests(src).contains(&dst) {
            return false;
        }
        let (Some(a), Some(b), Some(send), Some(set)) = (
            self.xp(src),
            self.xp(dst),
            self.xf::<Send>(c"CreateTrackSend"),
            self.xf::<Set>(c"SetTrackSendInfo_Value"),
        ) else {
            return false;
        };
        // SAFETY: pistes vivantes.
        let i = unsafe { send(a, b) };
        if i < 0 {
            return false;
        }
        // SAFETY: l'envoi vient d'etre cree a l'indice i.
        unsafe {
            set(a, 0, i, c"I_SRCCHAN".as_ptr(), -1.0);
            set(a, 0, i, c"I_MIDIFLAGS".as_ptr(), 0.0);
        }
        true
    }

    /// Une boite de REAPER ; rend le bouton (1 OK, 2 Annuler, 6 Oui, 7 Non).
    pub fn xc_message(&self, title: &str, text: &str, kind: i32) -> i32 {
        type F = unsafe extern "C" fn(*const c_char, *const c_char, i32) -> i32;
        let (Some(f), Ok(t), Ok(m)) = (
            self.xf::<F>(c"ShowMessageBox"),
            CString::new(title),
            CString::new(text),
        ) else {
            return 0;
        };
        // SAFETY: chaines vivantes pendant l'appel (boite modale).
        unsafe { f(m.as_ptr(), t.as_ptr(), kind) }
    }

    /// Le parametre porte-t-il une enveloppe d'automation ?
    pub fn xc_has_envelope(&self, t: Tk, fx: usize, k: usize) -> bool {
        type F = unsafe extern "C" fn(P, i32, i32, bool) -> P;
        match (self.xp(t), self.xf::<F>(c"GetFXEnvelope")) {
            // SAFETY: piste vivante ; faux = ne pas creer.
            (Some(p), Some(f)) => !unsafe { f(p, fx as i32, k as i32, false) }.is_null(),
            _ => false,
        }
    }

    /// Le nom d'une piste vers son pointeur (la premiere de ce nom).
    pub fn xc_find_track_named(&self, name: &str) -> Option<Tk> {
        (0..self.xc_track_count())
            .filter_map(|i| self.xc_track_index(i))
            .find(|&t| self.xc_track_raw_name(t) == name)
    }

    /// La memoire partagee des JSFX (`gmem_*`) est-elle offerte aux
    /// extensions ? (Le SDK 7.78 ne la declare pas.)
    pub fn xc_has_gmem(&self) -> bool {
        !self.get_func(c"gmem_read").is_null() && !self.get_func(c"gmem_attach").is_null()
    }
}

/// Une enveloppe d'automation, tenue par son pointeur (verifie a chaque
/// appel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env(usize);

impl Reaper {
    fn xe(&self, e: Env) -> Option<P> {
        type Valid = unsafe extern "C" fn(P, P, *const c_char) -> bool;
        let v: Valid = self.xf(c"ValidatePtr2")?;
        let p = e.0 as P;
        if p.is_null() {
            return None;
        }
        // SAFETY: projet nul = courant ; ValidatePtr2 accepte tout pointeur.
        unsafe { v(null_mut(), p, c"TrackEnvelope*".as_ptr()) }.then_some(p)
    }

    /// L'enveloppe d'un parametre d'effet (`create` : la creer si besoin).
    pub fn xc_fx_envelope(&self, t: Tk, fx: usize, k: usize, create: bool) -> Option<Env> {
        type F = unsafe extern "C" fn(P, i32, i32, bool) -> P;
        let p = self.xp(t)?;
        let f: F = self.xf(c"GetFXEnvelope")?;
        // SAFETY: piste vivante.
        let e = unsafe { f(p, fx as i32, k as i32, create) };
        (!e.is_null()).then_some(Env(e as usize))
    }

    /// Un point (forme lineaire), sans trier : [`Reaper::xc_env_sort`] apres.
    pub fn xc_env_insert(&self, e: Env, time: f64, value: f64) -> bool {
        type F = unsafe extern "C" fn(P, f64, f64, i32, f64, bool, *mut bool) -> bool;
        let (Some(p), Some(f)) = (self.xe(e), self.xf::<F>(c"InsertEnvelopePoint")) else {
            return false;
        };
        let mut no_sort = true;
        // SAFETY: enveloppe vivante ; le drapeau est a nous.
        unsafe { f(p, time, value, 0, 0.0, false, &mut no_sort) }
    }

    /// Efface les points de `a` a `b` (secondes).
    pub fn xc_env_delete_range(&self, e: Env, a: f64, b: f64) {
        type F = unsafe extern "C" fn(P, f64, f64) -> bool;
        if let (Some(p), Some(f)) = (self.xe(e), self.xf::<F>(c"DeleteEnvelopePointRange")) {
            // SAFETY: enveloppe vivante.
            unsafe { f(p, a, b) };
        }
    }

    /// La valeur de l'enveloppe a cet instant (None si elle n'est plus).
    pub fn xc_env_eval(&self, e: Env, time: f64) -> Option<f64> {
        type F =
            unsafe extern "C" fn(P, f64, f64, i32, *mut f64, *mut f64, *mut f64, *mut f64) -> i32;
        let (p, f) = (self.xe(e)?, self.xf::<F>(c"Envelope_Evaluate")?);
        let mut v = 0.0;
        // SAFETY: enveloppe vivante ; la sortie est a nous, les autres nulles.
        unsafe {
            f(
                p,
                time,
                48_000.0,
                1,
                &mut v,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        Some(v)
    }

    pub fn xc_env_sort(&self, e: Env) {
        type F = unsafe extern "C" fn(P) -> bool;
        if let (Some(p), Some(f)) = (self.xe(e), self.xf::<F>(c"Envelope_SortPoints")) {
            // SAFETY: enveloppe vivante.
            unsafe { f(p) };
        }
    }

    /// L'etat du transport (`GetPlayState` : 1 lecture, 2 pause, 4 enreg.).
    pub fn xc_play_state(&self) -> i32 {
        type F = unsafe extern "C" fn() -> i32;
        match self.xf::<F>(c"GetPlayState") {
            // SAFETY: sans argument.
            Some(f) => unsafe { f() },
            None => 0,
        }
    }

    /// Ce qu'on entend (`GetPlayPosition`, en secondes).
    pub fn xc_play_position(&self) -> f64 {
        type F = unsafe extern "C" fn() -> f64;
        match self.xf::<F>(c"GetPlayPosition") {
            // SAFETY: sans argument.
            Some(f) => unsafe { f() },
            None => 0.0,
        }
    }
}

impl Reaper {
    /// Le nombre de points d'une enveloppe.
    pub fn xc_env_count(&self, e: Env) -> i32 {
        type F = unsafe extern "C" fn(P) -> i32;
        match (self.xe(e), self.xf::<F>(c"CountEnvelopePoints")) {
            // SAFETY: enveloppe vivante.
            (Some(p), Some(f)) => unsafe { f(p) },
            _ => 0,
        }
    }

    /// Un point d'annulation, l'etat ENTIER du projet tel qu'il est
    /// (`Undo_OnStateChange2` ne garde que les items).
    pub fn xc_undo_point(&self, name: &str) {
        type F = unsafe extern "C" fn(*const c_char, i32, i32);
        if let (Some(f), Ok(n)) = (self.xf::<F>(c"Undo_OnStateChangeEx"), CString::new(name)) {
            // SAFETY: chaine vivante ; -1 = tous les etats, -1 = pas une piste.
            unsafe { f(n.as_ptr(), -1, -1) };
        }
    }
}

impl Reaper {
    /// Ce que « Annuler » annulerait (vide : rien).
    pub fn xc_undo_label(&self) -> String {
        type F = unsafe extern "C" fn(P) -> *const c_char;
        let Some(f) = self.xf::<F>(c"Undo_CanUndo2") else {
            return String::new();
        };
        // SAFETY: projet nul = courant ; la chaine est a REAPER, copiee ici.
        let p = unsafe { f(null_mut()) };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: chaine terminee par zero.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
