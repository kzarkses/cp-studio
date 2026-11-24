//! Ce que le navigateur d'effets lit et ecrit dans REAPER : la liste des
//! effets installes, la chaine d'une piste (lire, ajouter a une place,
//! deplacer, copier, contourner, supprimer, ouvrir), la piste choisie, ce qui
//! est sous la souris, une piste neuve pour un instrument.
//!
//! Une piste se nomme par son GUID ; « MASTER » nomme la piste maitre.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

/// Le nom de la piste maitre pour le navigateur.
pub const MASTER: &str = "MASTER";

/// La piste que montre le navigateur.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FxTrack {
    pub guid: String,
    pub name: String,
    pub color: Option<(u8, u8, u8)>,
    /// Ses effets : (nom, allume).
    pub fx: Vec<(String, bool)>,
}

impl Reaper {
    fn fb<F: Copy>(&self, name: &CStr) -> Option<F> {
        // SAFETY: les appelants nomment une fonction du SDK avec sa signature.
        unsafe { self.api::<F>(name) }
    }

    fn fb_str(&self, tr: P, key: &str, set: Option<&str>) -> Option<String> {
        type F = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let f: F = self.fb(c"GetSetMediaTrackInfo_String")?;
        let k = CString::new(key).ok()?;
        let mut buf = vec![0u8; 1024];
        if let Some(v) = set {
            let v = CString::new(v).ok()?;
            let b = v.as_bytes_with_nul();
            buf[..b.len().min(1024)].copy_from_slice(&b[..b.len().min(1024)]);
        }
        // SAFETY: piste vivante ; tampon de 1 Ko termine par zero.
        let ok = unsafe { f(tr, k.as_ptr(), buf.as_mut_ptr() as *mut c_char, set.is_some()) };
        if !ok {
            return None;
        }
        // SAFETY: chaine terminee par zero dans le tampon.
        Some(
            unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn fb_master(&self) -> Option<P> {
        type F = unsafe extern "C" fn(P) -> P;
        let f: F = self.fb(c"GetMasterTrack")?;
        // SAFETY: projet nul = courant.
        let m = unsafe { f(null_mut()) };
        (!m.is_null()).then_some(m)
    }

    fn fb_track(&self, guid: &str) -> Option<P> {
        if guid == MASTER {
            return self.fb_master();
        }
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let c: Count = self.fb(c"CountTracks")?;
        let g: Get = self.fb(c"GetTrack")?;
        // SAFETY: projet courant ; indices dans les bornes.
        let n = unsafe { c(null_mut()) };
        (0..n)
            // SAFETY: idem.
            .map(|i| unsafe { g(null_mut(), i) })
            .filter(|p| !p.is_null())
            .find(|&t| self.fb_str(t, "GUID", None).as_deref() == Some(guid))
    }

    fn fb_guid(&self, tr: P) -> Option<String> {
        if self.fb_master() == Some(tr) {
            return Some(MASTER.into());
        }
        self.fb_str(tr, "GUID", None).filter(|g| !g.is_empty())
    }

    /// Les effets installes : (nom, ident) d'`EnumInstalledFX`.
    pub fn fx_installed(&self) -> Vec<(String, String)> {
        type F = unsafe extern "C" fn(i32, *mut *const c_char, *mut *const c_char) -> bool;
        let Some(f) = self.fb::<F>(c"EnumInstalledFX") else {
            return Vec::new();
        };
        let mut v = Vec::new();
        for i in 0..100_000 {
            let mut n: *const c_char = null_mut();
            let mut id: *const c_char = null_mut();
            // SAFETY: deux chaines tenues par REAPER.
            if !unsafe { f(i, &mut n, &mut id) } {
                break;
            }
            if n.is_null() {
                continue;
            }
            let s = |p: *const c_char| {
                if p.is_null() {
                    String::new()
                } else {
                    // SAFETY: chaine terminee par zero tenue par REAPER.
                    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
                }
            };
            v.push((s(n), s(id)));
        }
        v
    }

    /// La premiere piste choisie (la maitre si elle seule l'est).
    pub fn fx_selected_track(&self) -> Option<String> {
        type Sel = unsafe extern "C" fn(P, i32) -> P;
        type IsSel = unsafe extern "C" fn(P) -> bool;
        let s: Sel = self.fb(c"GetSelectedTrack")?;
        // SAFETY: projet courant.
        let tr = unsafe { s(null_mut(), 0) };
        if !tr.is_null() {
            return self.fb_guid(tr);
        }
        let m = self.fb_master()?;
        let is: IsSel = self.fb(c"IsTrackSelected")?;
        // SAFETY: piste maitre vivante.
        unsafe { is(m) }.then(|| MASTER.to_string())
    }

    /// La piste, son nom, sa couleur et ses effets.
    pub fn fx_track(&self, guid: &str) -> Option<FxTrack> {
        type Num = unsafe extern "C" fn(P, *const c_char) -> f64;
        type Color = unsafe extern "C" fn(P) -> i32;
        type Count = unsafe extern "C" fn(P) -> i32;
        type Name = unsafe extern "C" fn(P, i32, *mut c_char, i32) -> bool;
        type En = unsafe extern "C" fn(P, i32) -> bool;
        let tr = self.fb_track(guid)?;
        let mut name = if guid == MASTER {
            "MASTER".to_string()
        } else {
            self.fb_str(tr, "P_NAME", None).unwrap_or_default()
        };
        if name.is_empty() {
            if let Some(n) = self.fb::<Num>(c"GetMediaTrackInfo_Value") {
                // SAFETY: piste vivante.
                let k = unsafe { n(tr, c"IP_TRACKNUMBER".as_ptr()) };
                name = format!("Track {}", k as i32);
            }
        }
        let color = self.fb::<Color>(c"GetTrackColor").and_then(|f| {
            // SAFETY: piste vivante.
            let c = unsafe { f(tr) };
            (c & 0x0100_0000 != 0).then_some((
                (c & 0xFF) as u8,
                ((c >> 8) & 0xFF) as u8,
                ((c >> 16) & 0xFF) as u8,
            ))
        });
        let mut fx = Vec::new();
        if let (Some(c), Some(nm), Some(en)) = (
            self.fb::<Count>(c"TrackFX_GetCount"),
            self.fb::<Name>(c"TrackFX_GetFXName"),
            self.fb::<En>(c"TrackFX_GetEnabled"),
        ) {
            // SAFETY: piste vivante ; tampon de 512 octets.
            let n = unsafe { c(tr) };
            for i in 0..n {
                let mut buf = [0u8; 512];
                // SAFETY: idem.
                let ok = unsafe { nm(tr, i, buf.as_mut_ptr() as *mut c_char, 512) };
                let s = if ok {
                    // SAFETY: chaine terminee par zero.
                    unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                };
                // SAFETY: idem.
                fx.push((s, unsafe { en(tr, i) }));
            }
        }
        Some(FxTrack {
            guid: guid.to_string(),
            name,
            color,
            fx,
        })
    }

    /// Ajoute un effet (ou une chaine .RfxChain) a la place `at` (aucune : a
    /// la fin). Rend sa place.
    pub fn fx_add(&self, guid: &str, name: &str, at: Option<usize>) -> Result<i32, String> {
        type Add = unsafe extern "C" fn(P, *const c_char, bool, i32) -> i32;
        let tr = self.fb_track(guid).ok_or("track not found")?;
        let add: Add = self.fb(c"TrackFX_AddByName").ok_or("REAPER API missing")?;
        let c = CString::new(name).map_err(|e| e.to_string())?;
        let inst = at.map_or(-1, |p| -1000 - p.min(4000) as i32);
        // SAFETY: piste vivante ; nom termine par zero.
        let k = unsafe { add(tr, c.as_ptr(), false, inst) };
        if k < 0 {
            Err(format!("REAPER could not add {name}"))
        } else {
            Ok(k)
        }
    }

    /// Le nombre d'effets d'une piste.
    pub fn fx_count(&self, guid: &str) -> i32 {
        type Count = unsafe extern "C" fn(P) -> i32;
        match (self.fb_track(guid), self.fb::<Count>(c"TrackFX_GetCount")) {
            // SAFETY: piste vivante.
            (Some(tr), Some(c)) => unsafe { c(tr) },
            _ => 0,
        }
    }

    /// Deplace (ou copie) l'effet `from` a la place `to` (0..=n, avant
    /// l'effet `to`). Rend sa nouvelle place.
    pub fn fx_move(&self, guid: &str, from: usize, to: usize, copy: bool) -> Option<usize> {
        type Copy = unsafe extern "C" fn(P, i32, P, i32, bool);
        let tr = self.fb_track(guid)?;
        let f: Copy = self.fb(c"TrackFX_CopyToTrack")?;
        let dest = if !copy && to > from { to - 1 } else { to };
        // SAFETY: piste vivante ; indices dans la chaine.
        unsafe { f(tr, from as i32, tr, dest as i32, !copy) };
        Some(dest)
    }

    pub fn fx_set_enabled(&self, guid: &str, fx: usize, on: bool) {
        type En = unsafe extern "C" fn(P, i32, bool);
        if let (Some(tr), Some(f)) = (self.fb_track(guid), self.fb::<En>(c"TrackFX_SetEnabled")) {
            // SAFETY: piste vivante.
            unsafe { f(tr, fx as i32, on) };
        }
    }

    pub fn fx_remove(&self, guid: &str, fx: usize) -> bool {
        type Del = unsafe extern "C" fn(P, i32) -> bool;
        match (self.fb_track(guid), self.fb::<Del>(c"TrackFX_Delete")) {
            // SAFETY: piste vivante.
            (Some(tr), Some(f)) => unsafe { f(tr, fx as i32) },
            _ => false,
        }
    }

    /// Ouvre la fenetre de l'effet (flottante) ; `chain` : la chaine.
    pub fn fx_open(&self, guid: &str, fx: usize, chain: bool) {
        type Show = unsafe extern "C" fn(P, i32, i32);
        if let (Some(tr), Some(f)) = (self.fb_track(guid), self.fb::<Show>(c"TrackFX_Show")) {
            // SAFETY: piste vivante ; 3 = flottante, 1 = la chaine.
            unsafe { f(tr, fx as i32, if chain { 1 } else { 3 }) };
        }
    }

    /// Une piste neuve pour un instrument, apres la piste `after` (aucune : a
    /// la fin) : nommee, choisie, armee sur toutes les entrees MIDI, a
    /// l'ecoute, comme « Insert virtual instrument on new track ». Rend son
    /// GUID.
    pub fn fx_new_track(&self, name: &str, after: Option<&str>) -> Option<String> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Num = unsafe extern "C" fn(P, *const c_char) -> f64;
        type Ins = unsafe extern "C" fn(i32, bool);
        type Get = unsafe extern "C" fn(P, i32) -> P;
        type Set = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        type Only = unsafe extern "C" fn(P);
        let count: Count = self.fb(c"CountTracks")?;
        let ins: Ins = self.fb(c"InsertTrackAtIndex")?;
        let get: Get = self.fb(c"GetTrack")?;
        let set: Set = self.fb(c"SetMediaTrackInfo_Value")?;
        let num: Num = self.fb(c"GetMediaTrackInfo_Value")?;
        // SAFETY: projet courant ; fil principal.
        let n = unsafe { count(null_mut()) };
        let index = after
            .filter(|g| *g != MASTER)
            .and_then(|g| self.fb_track(g))
            // SAFETY: piste vivante ; IP_TRACKNUMBER est 1 pour la premiere.
            .map(|t| unsafe { num(t, c"IP_TRACKNUMBER".as_ptr()) } as i32)
            .unwrap_or(n)
            .clamp(0, n);
        // SAFETY: indice dans les bornes ; la piste est verifiee non nulle.
        let tr = unsafe {
            ins(index, true);
            get(null_mut(), index)
        };
        if tr.is_null() {
            return None;
        }
        self.fb_str(tr, "P_NAME", Some(name));
        // SAFETY: piste vivante. 4096 | 63 << 5 : toutes les entrees MIDI,
        // tous les canaux.
        unsafe {
            set(tr, c"I_RECINPUT".as_ptr(), (4096 | (63 << 5)) as f64);
            set(tr, c"I_RECMON".as_ptr(), 1.0);
            set(tr, c"I_RECARM".as_ptr(), 1.0);
            if let Some(o) = self.fb::<Only>(c"SetOnlyTrackSelected") {
                o(tr);
            }
        }
        self.fb_guid(tr)
    }

    /// Choisit seulement cette piste (la maitre comprise).
    pub fn fx_select_track(&self, guid: &str) {
        type Only = unsafe extern "C" fn(P);
        if let (Some(tr), Some(o)) = (self.fb_track(guid), self.fb::<Only>(c"SetOnlyTrackSelected")) {
            // SAFETY: piste vivante.
            unsafe { o(tr) };
        }
    }

    /// Ce qui est sous un point de l'ecran : la piste (son GUID) et ce que
    /// REAPER en dit (« tcp.fx », « arrange », « mcp »...).
    pub fn thing_at(&self, x: i32, y: i32) -> (Option<String>, String) {
        type F = unsafe extern "C" fn(i32, i32, *mut c_char, i32) -> P;
        let Some(f) = self.fb::<F>(c"GetThingFromPoint") else {
            return (None, String::new());
        };
        let mut buf = [0u8; 256];
        // SAFETY: tampon de 256 octets termine par zero par REAPER.
        let tr = unsafe { f(x, y, buf.as_mut_ptr() as *mut c_char, 256) };
        // SAFETY: idem.
        let info = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned();
        let g = if tr.is_null() { None } else { self.fb_guid(tr) };
        (g, info)
    }

    /// Le point (ecran) est-il sur la zone des pistes de l'arrangement, sous
    /// la derniere piste ? (un lacher y cree une piste)
    pub fn below_last_track(&self, x: i32, y: i32) -> bool {
        let (tr, info) = self.thing_at(x, y);
        tr.is_none() && (info == "arrange" || info.starts_with("tcp"))
    }
}
