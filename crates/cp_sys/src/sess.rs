//! Ce que la Session lit et ecrit dans REAPER : les pistes d'un seul
//! passage (colonnes et tranches), l'etat du projet (`ProjExtState`), les
//! effets d'une piste, les entrees et sorties MIDI.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;
type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
type GetStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;

/// Une piste telle que la Session la montre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessTrack {
    pub guid: String,
    /// 0 = la premiere piste.
    pub index: i32,
    pub name: String,
    pub color: Option<(u8, u8, u8)>,
    pub depth: i32,
    pub armed: bool,
    /// `I_RECINPUT` tel quel.
    pub input: i32,
    /// `I_RECMON` : 0 non, 1 oui, 2 auto.
    pub monitor: i32,
    pub mute: bool,
    pub solo: bool,
    pub vol: f64,
    pub pan: f64,
    /// `I_AUTOMODE`.
    pub auto: i32,
    /// `I_MIDIHWOUT` tel quel (< 0 : aucune).
    pub midi_out: i32,
    /// Montree dans la console (`B_SHOWINMIXER`).
    pub show: bool,
    /// Elle porte un instrument.
    pub instrument: bool,
    pub selected: bool,
    /// Ses effets : (nom court, allume).
    pub fx: Vec<(String, bool)>,
    /// Les cretes (gauche, droite).
    pub peak: [f32; 2],
    /// La marque `P_EXT:CP` (une piste de la suite).
    pub mark: Option<String>,
}

impl Reaper {
    fn sf<F: Copy>(&self, name: &CStr) -> Option<F> {
        // SAFETY: les appelants nomment une fonction du SDK avec sa signature.
        unsafe { self.api::<F>(name) }
    }

    fn s_tracks(&self) -> Vec<P> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let (Some(c), Some(g)) = (
            self.sf::<Count>(c"CountTracks"),
            self.sf::<Get>(c"GetTrack"),
        ) else {
            return Vec::new();
        };
        // SAFETY: projet nul = courant ; indices dans les bornes.
        unsafe {
            let n = c(null_mut());
            (0..n)
                .map(|i| g(null_mut(), i))
                .filter(|p| !p.is_null())
                .collect()
        }
    }

    fn s_str(&self, tr: P, key: &str) -> Option<String> {
        let f: GetStr = self.sf(c"GetSetMediaTrackInfo_String")?;
        let k = CString::new(key).ok()?;
        let mut buf = vec![0u8; 1024];
        // SAFETY: piste vivante ; tampon de 1 Ko, termine par zero par REAPER.
        let ok = unsafe { f(tr, k.as_ptr(), buf.as_mut_ptr() as *mut c_char, false) };
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

    fn s_track_of(&self, guid: &str) -> Option<P> {
        self.s_tracks()
            .into_iter()
            .find(|&t| self.s_str(t, "GUID").as_deref() == Some(guid))
    }

    /// Les pistes du projet, d'un seul passage. `fx` : avec leurs effets
    /// (les tranches de mixage).
    pub fn session_tracks(&self, fx: bool) -> Vec<SessTrack> {
        type Peak = unsafe extern "C" fn(P, i32) -> f64;
        type Instr = unsafe extern "C" fn(P) -> i32;
        type FxCount = unsafe extern "C" fn(P) -> i32;
        type FxName = unsafe extern "C" fn(P, i32, *mut c_char, i32) -> bool;
        type FxEn = unsafe extern "C" fn(P, i32) -> bool;
        let gv = self.sf::<GetVal>(c"GetMediaTrackInfo_Value");
        let peak = self.sf::<Peak>(c"Track_GetPeakInfo");
        let instr = self.sf::<Instr>(c"TrackFX_GetInstrument");
        let fc = self.sf::<FxCount>(c"TrackFX_GetCount");
        let fname = self.sf::<FxName>(c"TrackFX_GetFXName");
        let fen = self.sf::<FxEn>(c"TrackFX_GetEnabled");
        self.s_tracks()
            .into_iter()
            .enumerate()
            .map(|(i, tr)| {
                let val = |k: &CStr| {
                    // SAFETY: piste vivante, cle terminee par zero.
                    gv.map_or(0.0, |f| unsafe { f(tr, k.as_ptr()) })
                };
                let col = val(c"I_CUSTOMCOLOR") as i32;
                let color = (col & 0x0100_0000 != 0).then_some((
                    (col & 0xFF) as u8,
                    ((col >> 8) & 0xFF) as u8,
                    ((col >> 16) & 0xFF) as u8,
                ));
                let mut fxs = Vec::new();
                if fx {
                    if let (Some(c), Some(nm), Some(en)) = (fc, fname, fen) {
                        // SAFETY: piste vivante ; tampon de 256 octets.
                        unsafe {
                            for k in 0..c(tr).clamp(0, 64) {
                                let mut buf = [0 as c_char; 256];
                                let name = if nm(tr, k, buf.as_mut_ptr(), 256) {
                                    CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
                                } else {
                                    String::new()
                                };
                                fxs.push((short_fx(&name), en(tr, k)));
                            }
                        }
                    }
                }
                SessTrack {
                    guid: self.s_str(tr, "GUID").unwrap_or_default(),
                    index: i as i32,
                    name: self.s_str(tr, "P_NAME").unwrap_or_default(),
                    color,
                    depth: val(c"I_FOLDERDEPTH") as i32,
                    armed: val(c"I_RECARM") != 0.0,
                    input: val(c"I_RECINPUT") as i32,
                    monitor: val(c"I_RECMON") as i32,
                    mute: val(c"B_MUTE") != 0.0,
                    solo: val(c"I_SOLO") != 0.0,
                    vol: val(c"D_VOL"),
                    pan: val(c"D_PAN"),
                    auto: val(c"I_AUTOMODE") as i32,
                    midi_out: val(c"I_MIDIHWOUT") as i32,
                    show: val(c"B_SHOWINMIXER") != 0.0,
                    // SAFETY: piste vivante.
                    instrument: instr.is_some_and(|f| unsafe { f(tr) } >= 0),
                    selected: val(c"I_SELECTED") != 0.0,
                    fx: fxs,
                    // SAFETY: piste vivante ; canaux 0 et 1.
                    peak: peak.map_or([0.0; 2], |f| unsafe { [f(tr, 0) as f32, f(tr, 1) as f32] }),
                    mark: self.s_str(tr, "P_EXT:CP").filter(|s| !s.is_empty()),
                }
            })
            .collect()
    }

    /// Les cretes seules (GUID, gauche, droite), piste par piste : ce que les
    /// vumetres lisent a chaque image.
    pub fn session_peaks(&self) -> Vec<(String, [f32; 2])> {
        type Peak = unsafe extern "C" fn(P, i32) -> f64;
        let Some(peak) = self.sf::<Peak>(c"Track_GetPeakInfo") else {
            return Vec::new();
        };
        self.s_tracks()
            .into_iter()
            .map(|tr| {
                (
                    self.s_str(tr, "GUID").unwrap_or_default(),
                    // SAFETY: piste vivante (prise a l'instant) ; canaux 0 et 1.
                    unsafe { [peak(tr, 0) as f32, peak(tr, 1) as f32] },
                )
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // L'etat du projet
    // -----------------------------------------------------------------------

    /// Une valeur de `ProjExtState` (le projet courant), sans limite de
    /// taille.
    pub fn proj_ext_get(&self, section: &str, key: &str) -> Option<String> {
        type F = unsafe extern "C" fn(P, *const c_char, *const c_char, *mut c_char, i32) -> i32;
        let f: F = self.sf(c"GetProjExtState")?;
        let (s, k) = (CString::new(section).ok()?, CString::new(key).ok()?);
        let mut cap = 1 << 16;
        loop {
            let mut buf = vec![0u8; cap];
            // SAFETY: projet nul = courant ; tampon de `cap` octets.
            unsafe {
                f(
                    null_mut(),
                    s.as_ptr(),
                    k.as_ptr(),
                    buf.as_mut_ptr() as *mut c_char,
                    cap as i32,
                )
            };
            let n = buf.iter().position(|&b| b == 0).unwrap_or(cap);
            // Rempli jusqu'au bout : la valeur etait peut-etre plus longue.
            if n + 1 >= cap && cap < (1 << 26) {
                cap *= 4;
                continue;
            }
            let v = String::from_utf8_lossy(&buf[..n]).into_owned();
            return (!v.is_empty()).then_some(v);
        }
    }

    pub fn proj_ext_set(&self, section: &str, key: &str, value: &str) {
        type F = unsafe extern "C" fn(P, *const c_char, *const c_char, *const c_char) -> i32;
        let Some(f) = self.sf::<F>(c"SetProjExtState") else {
            return;
        };
        let (Ok(s), Ok(k), Ok(v)) = (
            CString::new(section),
            CString::new(key),
            CString::new(value.replace('\0', " ")),
        ) else {
            return;
        };
        // SAFETY: projet nul = courant ; chaines vivantes.
        unsafe { f(null_mut(), s.as_ptr(), k.as_ptr(), v.as_ptr()) };
    }

    /// Le projet courant : son adresse (l'onglet) et son fichier. Un projet
    /// ouvert par-dessus garde l'adresse : le fichier le dit.
    pub fn project_id(&self) -> (usize, String) {
        type F = unsafe extern "C" fn(i32, *mut c_char, i32) -> P;
        let Some(f) = self.sf::<F>(c"EnumProjects") else {
            return (0, String::new());
        };
        let mut buf = [0 as c_char; 1024];
        // SAFETY: -1 = le projet courant ; tampon de 1 Ko.
        let p = unsafe { f(-1, buf.as_mut_ptr(), 1024) };
        // SAFETY: chaine terminee par zero.
        let name = unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        (p as usize, name)
    }

    pub fn mark_dirty(&self) {
        type F = unsafe extern "C" fn(P);
        if let Some(f) = self.sf::<F>(c"MarkProjectDirty") {
            // SAFETY: projet nul = courant.
            unsafe { f(null_mut()) };
        }
    }

    // -----------------------------------------------------------------------
    // Les effets d'une piste
    // -----------------------------------------------------------------------

    /// Ouvre la fenetre d'un effet (flottante).
    pub fn track_fx_show(&self, guid: &str, fx: i32) {
        type F = unsafe extern "C" fn(P, i32, i32);
        if let (Some(tr), Some(f)) = (self.s_track_of(guid), self.sf::<F>(c"TrackFX_Show")) {
            // SAFETY: piste vivante ; 3 = fenetre flottante.
            unsafe { f(tr, fx, 3) };
        }
    }

    /// La chaine d'effets de la piste.
    pub fn track_fx_chain(&self, guid: &str) {
        type F = unsafe extern "C" fn(P, i32, i32);
        if let (Some(tr), Some(f)) = (self.s_track_of(guid), self.sf::<F>(c"TrackFX_Show")) {
            // SAFETY: piste vivante ; 1 = la chaine.
            unsafe { f(tr, 0, 1) };
        }
    }

    pub fn track_fx_enable(&self, guid: &str, fx: i32, on: bool) {
        type F = unsafe extern "C" fn(P, i32, bool);
        if let (Some(tr), Some(f)) = (self.s_track_of(guid), self.sf::<F>(c"TrackFX_SetEnabled")) {
            // SAFETY: piste vivante.
            unsafe { f(tr, fx, on) };
        }
    }

    pub fn track_fx_delete(&self, guid: &str, fx: i32) -> bool {
        type F = unsafe extern "C" fn(P, i32) -> bool;
        match (self.s_track_of(guid), self.sf::<F>(c"TrackFX_Delete")) {
            // SAFETY: piste vivante.
            (Some(tr), Some(f)) => unsafe { f(tr, fx) },
            _ => false,
        }
    }

    /// Les sorties MIDI : (rang, nom) de celles qui existent.
    pub fn midi_outputs(&self) -> Vec<(i32, String)> {
        type Num = unsafe extern "C" fn() -> i32;
        type Name = unsafe extern "C" fn(i32, *mut c_char, i32) -> bool;
        let (Some(n), Some(nm)) = (
            self.sf::<Num>(c"GetNumMIDIOutputs"),
            self.sf::<Name>(c"GetMIDIOutputName"),
        ) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // SAFETY: tampon de 256 octets, termine par zero par REAPER.
        unsafe {
            for i in 0..n().clamp(0, 256) {
                let mut buf = [0 as c_char; 256];
                if nm(i, buf.as_mut_ptr(), 256) {
                    out.push((
                        i,
                        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned(),
                    ));
                }
            }
        }
        out
    }

    /// Le taux qui cale un fichier sur le tempo du projet, s'il declare son
    /// tempo (`GetTempoMatchPlayRate`) : rend le tempo de la source.
    pub fn file_tempo(&self, path: &str) -> Option<f64> {
        type Create = unsafe extern "C" fn(*const c_char) -> P;
        type Destroy = unsafe extern "C" fn(P);
        type Match = unsafe extern "C" fn(P, f64, f64, f64, *mut f64, *mut f64) -> bool;
        let c = self.sf::<Create>(c"PCM_Source_CreateFromFile")?;
        let d = self.sf::<Destroy>(c"PCM_Source_Destroy")?;
        let m = self.sf::<Match>(c"GetTempoMatchPlayRate")?;
        let p = CString::new(path).ok()?;
        // SAFETY: chemin termine par zero ; la source creee est detruite ici.
        unsafe {
            let src = c(p.as_ptr());
            if src.is_null() {
                return None;
            }
            let (mut rate, mut len) = (0f64, 0f64);
            let ok = m(src, 1.0, 0.0, 1.0, &mut rate, &mut len);
            d(src);
            if !ok || rate.is_nan() || rate <= 0.0 {
                return None;
            }
            // Le taux qui l'amene au tempo du projet : son tempo = projet / taux.
            Some(self.master_tempo() / rate)
        }
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetDlgItem(h: isize, id: i32) -> isize;
}

impl Reaper {
    /// La piste sous la souris (dans l'arrangement ou sa liste de pistes).
    pub fn track_under_mouse(&self) -> Option<String> {
        type F = unsafe extern "C" fn(i32, i32, *mut i32) -> P;
        let f = self.sf::<F>(c"GetTrackFromPoint")?;
        let (x, y) = crate::ui::cursor_pos();
        let mut info = 0i32;
        // SAFETY: coordonnees d'ecran ; `info` vit pendant l'appel.
        let tr = unsafe { f(x, y, &mut info) };
        if tr.is_null() {
            return None;
        }
        self.s_str(tr, "GUID").filter(|g| !g.is_empty())
    }

    /// L'instant sous la souris dans l'arrangement (s), si elle y est.
    pub fn position_under_mouse(&self) -> Option<f64> {
        type F = unsafe extern "C" fn(P, bool, i32, i32, *mut f64, *mut f64);
        let f = self.sf::<F>(c"GetSet_ArrangeView2")?;
        // La vue des pistes : la fille 1000 de la fenetre principale.
        // SAFETY: fenetre principale vivante ; GetDlgItem rend 0 sinon.
        let tv = unsafe { GetDlgItem(self.main_hwnd().0, 1000) };
        if tv == 0 {
            return None;
        }
        let tvh = crate::win::Hwnd(tv);
        let (sx, sy) = crate::ui::cursor_pos();
        let (cx, cy) = crate::ui::screen_to_client(tvh, sx, sy);
        let rc = crate::ui::client_rect(tvh);
        if cx < 0 || cy < 0 || cx >= rc.right - rc.left || cy >= rc.bottom - rc.top {
            return None;
        }
        let (mut a, mut b) = (0f64, 0f64);
        // SAFETY: lecture de la vue ; `a`, `b` vivent pendant l'appel.
        unsafe { f(null_mut(), false, cx, cx + 1, &mut a, &mut b) };
        Some(a.max(0.0))
    }
}

/// Le nom court d'un effet : « VST3: ReaComp (Cockos) » -> « ReaComp ».
pub fn short_fx(name: &str) -> String {
    let s = name.split_once(": ").map_or(name, |(_, b)| b);
    let s = match s.rfind(" (") {
        Some(i) if s.ends_with(')') && i > 0 => &s[..i],
        _ => s,
    };
    s.trim().to_string()
}

/// Le libelle court d'une entree (`I_RECINPUT`).
pub fn input_label(v: i32, audio: &[String], midi: &[(i32, String)]) -> String {
    if v < 0 {
        return String::new();
    }
    if v & 4096 != 0 {
        let dev = (v >> 5) & 63;
        let ch = v & 31;
        let d = match dev {
            63 => "All".to_string(),
            62 => "VKB".to_string(),
            k => midi
                .iter()
                .find(|m| m.0 == k)
                .map_or(format!("{}", k + 1), |m| m.1.clone()),
        };
        return if ch == 0 {
            format!("MIDI {d}")
        } else {
            format!("MIDI {d} ch{ch}")
        };
    }
    let c = v & 1023;
    let _ = audio;
    if c >= 512 {
        return format!("Loopback {}", c - 511);
    }
    if v & 2048 != 0 {
        format!("In {}+", c + 1)
    } else if v & 1024 != 0 {
        format!("In {}/{}", c + 1, c + 2)
    } else {
        format!("In {}", c + 1)
    }
}

/// Le libelle court d'une sortie MIDI materielle (`I_MIDIHWOUT`).
pub fn midi_out_label(v: i32, outs: &[(i32, String)]) -> String {
    if v < 0 {
        return String::new();
    }
    let dev = (v >> 5) & 31;
    let ch = v & 31;
    let d = outs
        .iter()
        .find(|o| o.0 == dev)
        .map_or(format!("Out {}", dev + 1), |o| o.1.clone());
    if ch == 0 { d } else { format!("{d} ch{ch}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_libelles() {
        assert_eq!(short_fx("VST3: ReaComp (Cockos)"), "ReaComp");
        assert_eq!(short_fx("JS: CP Sampler"), "CP Sampler");
        assert_eq!(input_label(-1, &[], &[]), "");
        assert_eq!(input_label(1024, &[], &[]), "In 1/2");
        assert_eq!(input_label(2, &[], &[]), "In 3");
        assert_eq!(input_label(4096 | (63 << 5), &[], &[]), "MIDI All");
        assert_eq!(
            input_label(4096 | (1 << 5) | 3, &[], &[(1, "Hydra".into())]),
            "MIDI Hydra ch3"
        );
        assert_eq!(midi_out_label(2 << 5, &[(2, "Synth".into())]), "Synth");
        assert_eq!(midi_out_label(-1, &[]), "");
    }
}
