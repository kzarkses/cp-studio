//! Les pistes du projet par leur GUID, et un INSTANTANE des pistes marquees
//! (Guitar Tab) : leurs reglages, leurs items, leurs prises, lus d'un seul
//! passage. Jamais un pointeur garde : un rechargement du module ou une
//! piste effacee ne laisse rien de pendant.
//!
//! Les donnees privees vivent dans `P_EXT:<cle>` (piste, item, prise).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;
type GetStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;

/// Une prise d'un item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TakeSnap {
    pub guid: String,
    pub name: String,
    pub midi: bool,
    /// Le fichier de la source (vide pour une prise MIDI).
    pub path: String,
    pub offs: f64,
    pub rate: f64,
    /// L'empreinte des notes d'une prise MIDI (elle change a chaque
    /// modification).
    pub hash: Option<String>,
}

/// Un item d'une piste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemSnap {
    pub guid: String,
    pub pos: f64,
    pub len: f64,
    /// La lane fixe (pistes en lanes fixes), sinon 0.
    pub lane: i32,
    pub active: usize,
    pub takes: Vec<TakeSnap>,
    pub selected: bool,
    pub mute: bool,
    /// Les fondus (s) : l'automatique s'il y en a un, sinon le manuel.
    pub fade_in: f64,
    pub fade_out: f64,
    /// Les donnees privees demandees, dans l'ordre des cles.
    pub ext: Vec<Option<String>>,
}

impl ItemSnap {
    pub fn take(&self) -> Option<&TakeSnap> {
        self.takes.get(self.active)
    }
}

/// Une piste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackSnap {
    pub guid: String,
    /// 0 = la premiere piste.
    pub index: i32,
    pub name: String,
    pub depth: i32,
    pub armed: bool,
    pub input: i32,
    pub monitor: i32,
    pub mute: bool,
    pub solo: bool,
    pub vol: f64,
    pub pan: f64,
    pub selected: bool,
    /// 2 = lanes fixes.
    pub freemode: i32,
    pub lanes: i32,
    /// Pour chaque lane : 0 muette, 1 seule a jouer, 2 joue avec d'autres.
    pub lane_plays: Vec<i32>,
    /// Les donnees privees demandees, dans l'ordre des cles.
    pub ext: Vec<Option<String>>,
    pub items: Vec<ItemSnap>,
}

impl Reaper {
    fn tf<F: Copy>(&self, name: &CStr) -> Option<F> {
        // SAFETY: les appelants nomment une fonction du SDK avec sa signature.
        unsafe { self.api::<F>(name) }
    }

    /// Lit ou ecrit une chaine d'un objet (piste, item, prise) par la
    /// fonction `GetSetXxxInfo_String` donnee. Les valeurs lues font moins de
    /// 8 Ko (nos donnees longues vont dans des fichiers).
    fn obj_str(&self, f: GetStr, obj: P, key: &str, set: Option<&str>) -> Option<String> {
        let k = CString::new(key).ok()?;
        let mut buf = vec![0u8; 8192];
        if let Some(v) = set {
            let c = CString::new(v.replace('\0', " ")).ok()?;
            let b = c.as_bytes_with_nul();
            if b.len() > buf.len() {
                buf.resize(b.len(), 0);
            }
            buf[..b.len()].copy_from_slice(b);
        }
        // SAFETY: objet vivant (rendu par REAPER a ce battement) ; tampon de
        // 8 Ko pour une lecture, ou contenant la valeur a ecrire.
        let ok = unsafe {
            f(
                obj,
                k.as_ptr(),
                buf.as_mut_ptr() as *mut c_char,
                set.is_some(),
            )
        };
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

    fn tr_str(&self, tr: P, key: &str, set: Option<&str>) -> Option<String> {
        let f: GetStr = self.tf(c"GetSetMediaTrackInfo_String")?;
        self.obj_str(f, tr, key, set)
    }

    fn it_str(&self, it: P, key: &str, set: Option<&str>) -> Option<String> {
        let f: GetStr = self.tf(c"GetSetMediaItemInfo_String")?;
        self.obj_str(f, it, key, set)
    }

    fn tk_str(&self, tk: P, key: &str, set: Option<&str>) -> Option<String> {
        let f: GetStr = self.tf(c"GetSetMediaItemTakeInfo_String")?;
        self.obj_str(f, tk, key, set)
    }

    fn tracks_raw(&self) -> Vec<P> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let (Some(c), Some(g)) = (
            self.tf::<Count>(c"CountTracks"),
            self.tf::<Get>(c"GetTrack"),
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

    /// La piste d'un GUID.
    pub(crate) fn track_of(&self, guid: &str) -> Option<P> {
        self.tracks_raw()
            .into_iter()
            .find(|&t| self.tr_str(t, "GUID", None).as_deref() == Some(guid))
    }

    fn items_of(&self, tr: P) -> Vec<P> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let (Some(c), Some(g)) = (
            self.tf::<Count>(c"CountTrackMediaItems"),
            self.tf::<Get>(c"GetTrackMediaItem"),
        ) else {
            return Vec::new();
        };
        // SAFETY: piste vivante ; indices dans les bornes.
        unsafe {
            let n = c(tr);
            (0..n).map(|i| g(tr, i)).filter(|p| !p.is_null()).collect()
        }
    }

    /// L'item d'un GUID, cherche sur une piste d'abord (rapide), sinon dans
    /// tout le projet.
    fn find_item(&self, guid: &str, on: Option<P>) -> Option<P> {
        if let Some(tr) = on {
            if let Some(it) = self
                .items_of(tr)
                .into_iter()
                .find(|&it| self.it_str(it, "GUID", None).as_deref() == Some(guid))
            {
                return Some(it);
            }
        }
        self.tracks_raw().into_iter().find_map(|tr| {
            self.items_of(tr)
                .into_iter()
                .find(|&it| self.it_str(it, "GUID", None).as_deref() == Some(guid))
        })
    }

    fn take_snap(&self, tk: P) -> TakeSnap {
        type IsMidi = unsafe extern "C" fn(P, bool) -> bool;
        type Src = unsafe extern "C" fn(P) -> P;
        type SrcName = unsafe extern "C" fn(P, *mut c_char, i32);
        type Hash = unsafe extern "C" fn(P, bool, *mut c_char, i32) -> bool;
        let (im, ts, sn, gt) = (
            self.tf::<IsMidi>(c"TakeIsMIDI"),
            self.tf::<Src>(c"GetMediaItemTake_Source"),
            self.tf::<SrcName>(c"GetMediaSourceFileName"),
            self.tf::<GetVal>(c"GetMediaItemTakeInfo_Value"),
        );
        let midi = im.is_some_and(|f| {
            // SAFETY: prise vivante.
            unsafe { f(tk, false) }
        });
        let mut path = String::new();
        let mut hash = None;
        if midi {
            if let Some(h) = self.tf::<Hash>(c"MIDI_GetHash") {
                let mut buf = vec![0u8; 128];
                // SAFETY: prise vivante ; tampon de 128 octets annonce.
                unsafe {
                    if h(tk, true, buf.as_mut_ptr() as *mut c_char, 128) {
                        hash = Some(
                            CStr::from_ptr(buf.as_ptr() as *const c_char)
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
            }
        } else {
            if let (Some(ts), Some(sn)) = (ts, sn) {
                // SAFETY: prise vivante ; tampon de 4096 octets annonce.
                unsafe {
                    let src = ts(tk);
                    if !src.is_null() {
                        let mut buf = vec![0u8; 4096];
                        sn(src, buf.as_mut_ptr() as *mut c_char, 4096);
                        path = CStr::from_ptr(buf.as_ptr() as *const c_char)
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            }
        }
        // SAFETY: prise vivante.
        let val = |k: &CStr| gt.map_or(0.0, |f| unsafe { f(tk, k.as_ptr()) });
        let rate = val(c"D_PLAYRATE");
        TakeSnap {
            guid: self.tk_str(tk, "GUID", None).unwrap_or_default(),
            name: self.tk_str(tk, "P_NAME", None).unwrap_or_default(),
            midi,
            path,
            offs: val(c"D_STARTOFFS"),
            rate: if rate > 0.0 { rate } else { 1.0 },
            hash,
        }
    }

    fn item_snap(&self, it: P, keys: &[&str]) -> ItemSnap {
        type CountTakes = unsafe extern "C" fn(P) -> i32;
        type GetTake = unsafe extern "C" fn(P, i32) -> P;
        type Active = unsafe extern "C" fn(P) -> P;
        let gi = self.tf::<GetVal>(c"GetMediaItemInfo_Value");
        // SAFETY: item vivant.
        let val = |k: &CStr| gi.map_or(0.0, |f| unsafe { f(it, k.as_ptr()) });
        let mut takes = Vec::new();
        let mut active = 0;
        if let (Some(ct), Some(gt), Some(at)) = (
            self.tf::<CountTakes>(c"CountTakes"),
            self.tf::<GetTake>(c"GetTake"),
            self.tf::<Active>(c"GetActiveTake"),
        ) {
            // SAFETY: item vivant ; indices dans les bornes ; prises non nulles.
            unsafe {
                let a = at(it);
                for k in 0..ct(it) {
                    let tk = gt(it, k);
                    if tk.is_null() {
                        continue;
                    }
                    if tk == a {
                        active = takes.len();
                    }
                    takes.push(self.take_snap(tk));
                }
            }
        }
        ItemSnap {
            guid: self.it_str(it, "GUID", None).unwrap_or_default(),
            pos: val(c"D_POSITION"),
            len: val(c"D_LENGTH"),
            lane: val(c"I_FIXEDLANE") as i32,
            active,
            takes,
            selected: val(c"B_UISEL") != 0.0,
            mute: val(c"B_MUTE") != 0.0,
            fade_in: {
                let a = val(c"D_FADEINLEN_AUTO");
                if a > 0.0 { a } else { val(c"D_FADEINLEN") }
            },
            fade_out: {
                let a = val(c"D_FADEOUTLEN_AUTO");
                if a > 0.0 { a } else { val(c"D_FADEOUTLEN") }
            },
            ext: keys
                .iter()
                .map(|k| {
                    self.it_str(it, &format!("P_EXT:{k}"), None)
                        .filter(|s| !s.is_empty())
                })
                .collect(),
        }
    }

    fn track_snap(
        &self,
        tr: P,
        index: i32,
        tkeys: &[&str],
        ikeys: &[&str],
        items: bool,
    ) -> TrackSnap {
        let gv = self.tf::<GetVal>(c"GetMediaTrackInfo_Value");
        let val = |k: &str| {
            let c = CString::new(k).unwrap_or_default();
            // SAFETY: piste vivante, cle terminee par zero.
            gv.map_or(0.0, |f| unsafe { f(tr, c.as_ptr()) })
        };
        let freemode = val("I_FREEMODE") as i32;
        let lanes = if freemode == 2 {
            val("I_NUMFIXEDLANES") as i32
        } else {
            0
        };
        TrackSnap {
            guid: self.tr_str(tr, "GUID", None).unwrap_or_default(),
            index,
            name: self.tr_str(tr, "P_NAME", None).unwrap_or_default(),
            depth: val("I_FOLDERDEPTH") as i32,
            armed: val("I_RECARM") != 0.0,
            input: val("I_RECINPUT") as i32,
            monitor: val("I_RECMON") as i32,
            mute: val("B_MUTE") != 0.0,
            solo: val("I_SOLO") != 0.0,
            vol: val("D_VOL"),
            pan: val("D_PAN"),
            selected: val("I_SELECTED") != 0.0,
            freemode,
            lanes,
            lane_plays: (0..lanes)
                .map(|i| val(&format!("C_LANEPLAYS:{i}")) as i32)
                .collect(),
            ext: tkeys
                .iter()
                .map(|k| {
                    self.tr_str(tr, &format!("P_EXT:{k}"), None)
                        .filter(|s| !s.is_empty())
                })
                .collect(),
            items: if items {
                self.items_of(tr)
                    .into_iter()
                    .map(|it| self.item_snap(it, ikeys))
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    /// Toutes les pistes, avec leurs donnees privees `tkeys`. Les items (avec
    /// leurs donnees `ikeys`) seulement pour les pistes ou `want(ext)` dit
    /// oui.
    pub fn snapshot_tracks(
        &self,
        tkeys: &[&str],
        ikeys: &[&str],
        want: &dyn Fn(&[Option<String>]) -> bool,
    ) -> Vec<TrackSnap> {
        self.tracks_raw()
            .into_iter()
            .enumerate()
            .map(|(i, tr)| {
                let mut s = self.track_snap(tr, i as i32, tkeys, ikeys, false);
                if want(&s.ext) {
                    s.items = self
                        .items_of(tr)
                        .into_iter()
                        .map(|it| self.item_snap(it, ikeys))
                        .collect();
                }
                s
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Les pistes
    // -----------------------------------------------------------------------

    pub fn track_ext(&self, guid: &str, key: &str) -> Option<String> {
        let tr = self.track_of(guid)?;
        self.tr_str(tr, &format!("P_EXT:{key}"), None)
            .filter(|s| !s.is_empty())
    }

    pub fn set_track_ext(&self, guid: &str, key: &str, value: &str) -> bool {
        self.track_of(guid).is_some_and(|tr| {
            self.tr_str(tr, &format!("P_EXT:{key}"), Some(value))
                .is_some()
        })
    }

    pub fn track_value(&self, guid: &str, key: &str) -> Option<f64> {
        let tr = self.track_of(guid)?;
        let f: GetVal = self.tf(c"GetMediaTrackInfo_Value")?;
        let c = CString::new(key).ok()?;
        // SAFETY: piste vivante, cle terminee par zero.
        Some(unsafe { f(tr, c.as_ptr()) })
    }

    pub fn set_track_value(&self, guid: &str, key: &str, v: f64) -> bool {
        let (Some(tr), Some(f)) = (
            self.track_of(guid),
            self.tf::<SetVal>(c"SetMediaTrackInfo_Value"),
        ) else {
            return false;
        };
        let Ok(c) = CString::new(key) else {
            return false;
        };
        // SAFETY: piste vivante, cle terminee par zero.
        unsafe { f(tr, c.as_ptr(), v) }
    }

    pub fn set_track_name(&self, guid: &str, name: &str) -> bool {
        self.track_of(guid)
            .is_some_and(|tr| self.tr_str(tr, "P_NAME", Some(name)).is_some())
    }

    /// Une piste neuve a l'indice donne (0 = la premiere). Rend son GUID.
    pub fn insert_track(&self, index: i32, name: &str) -> Option<String> {
        type Ins = unsafe extern "C" fn(i32, bool);
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let ins: Ins = self.tf(c"InsertTrackAtIndex")?;
        let get: Get = self.tf(c"GetTrack")?;
        // SAFETY: fil principal ; la piste est verifiee non nulle.
        let tr = unsafe {
            ins(index.max(0), true);
            get(null_mut(), index.max(0))
        };
        if tr.is_null() {
            return None;
        }
        self.tr_str(tr, "P_NAME", Some(name));
        self.tr_str(tr, "GUID", None)
    }

    /// Choisit seulement cette piste (et la montre dans l'arrangement).
    pub fn select_only_track(&self, guid: &str) -> bool {
        type Sel = unsafe extern "C" fn(P);
        type Cmd = unsafe extern "C" fn(i32, i32);
        let (Some(tr), Some(s)) = (self.track_of(guid), self.tf::<Sel>(c"SetOnlyTrackSelected"))
        else {
            return false;
        };
        // SAFETY: piste vivante ; 40913 = Piste : faire defiler la piste
        // choisie jusqu'a la voir.
        unsafe {
            s(tr);
            if let Some(c) = self.tf::<Cmd>(c"Main_OnCommand") {
                c(40913, 0);
            }
        }
        true
    }

    pub fn delete_track(&self, guid: &str) -> bool {
        type Del = unsafe extern "C" fn(P);
        let (Some(tr), Some(f)) = (self.track_of(guid), self.tf::<Del>(c"DeleteTrack")) else {
            return false;
        };
        // SAFETY: piste vivante.
        unsafe { f(tr) };
        true
    }

    /// La crete de la piste depuis le dernier appel (0..1+, lineaire).
    pub fn track_peak(&self, guid: &str) -> f32 {
        type Peak = unsafe extern "C" fn(P, i32) -> f64;
        let (Some(tr), Some(f)) = (self.track_of(guid), self.tf::<Peak>(c"Track_GetPeakInfo"))
        else {
            return 0.0;
        };
        // SAFETY: piste vivante.
        unsafe { f(tr, 0).max(f(tr, 1)) as f32 }
    }

    /// Les entrees audio de la carte son : leurs noms.
    pub fn audio_inputs(&self) -> Vec<String> {
        type Num = unsafe extern "C" fn() -> i32;
        type Name = unsafe extern "C" fn(i32) -> *const c_char;
        let (Some(n), Some(nm)) = (
            self.tf::<Num>(c"GetNumAudioInputs"),
            self.tf::<Name>(c"GetInputChannelName"),
        ) else {
            return Vec::new();
        };
        // SAFETY: chaines tenues par REAPER, copiees aussitot.
        unsafe {
            (0..n())
                .map(|i| {
                    let p = nm(i);
                    if p.is_null() {
                        format!("Input {}", i + 1)
                    } else {
                        CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                })
                .collect()
        }
    }

    /// Un envoi de la piste vers le master (rend faux s'il n'a pas pu etre
    /// cree).
    pub fn send_to_master(&self, guid: &str) -> bool {
        type Master = unsafe extern "C" fn(P) -> P;
        type Send = unsafe extern "C" fn(P, P) -> i32;
        let (Some(tr), Some(m), Some(s)) = (
            self.track_of(guid),
            self.tf::<Master>(c"GetMasterTrack"),
            self.tf::<Send>(c"CreateTrackSend"),
        ) else {
            return false;
        };
        // SAFETY: pistes vivantes.
        unsafe {
            let mt = m(null_mut());
            !mt.is_null() && s(tr, mt) >= 0
        }
    }

    // -----------------------------------------------------------------------
    // Les effets d'une piste
    // -----------------------------------------------------------------------

    /// L'indice d'un effet de la piste dont le nom contient `part`.
    pub fn track_fx_find(&self, guid: &str, part: &str) -> Option<i32> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Name = unsafe extern "C" fn(P, i32, *mut c_char, i32) -> bool;
        let tr = self.track_of(guid)?;
        let c: Count = self.tf(c"TrackFX_GetCount")?;
        let nm: Name = self.tf(c"TrackFX_GetFXName")?;
        let low = part.to_lowercase();
        // SAFETY: piste vivante ; tampon de 512 octets annonce.
        unsafe {
            (0..c(tr)).find(|&i| {
                let mut buf = vec![0u8; 512];
                nm(tr, i, buf.as_mut_ptr() as *mut c_char, 512)
                    && CStr::from_ptr(buf.as_ptr() as *const c_char)
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&low)
            })
        }
    }

    /// Le nombre d'effets de la piste.
    pub fn track_fx_count(&self, guid: &str) -> i32 {
        type Count = unsafe extern "C" fn(P) -> i32;
        match (self.track_of(guid), self.tf::<Count>(c"TrackFX_GetCount")) {
            // SAFETY: piste vivante.
            (Some(tr), Some(c)) => unsafe { c(tr) },
            _ => 0,
        }
    }

    /// Ajoute un effet a la fin de la chaine, sans ouvrir sa fenetre : la
    /// preference « Auto-float newly created FX windows » (`fxfloat_focus`)
    /// ouvre la fenetre DANS l'appel ; on la coupe le temps de l'ajout.
    pub fn track_fx_add(&self, guid: &str, name: &str) -> Option<i32> {
        type Add = unsafe extern "C" fn(P, *const c_char, bool, i32) -> i32;
        let tr = self.track_of(guid)?;
        let add: Add = self.tf(c"TrackFX_AddByName")?;
        let c = CString::new(name).ok()?;
        let float = self.config_int("fxfloat_focus");
        if float.is_some_and(|v| v != 0) {
            self.set_config_int("fxfloat_focus", 0);
        }
        // SAFETY: piste vivante ; -1 = toujours ajouter.
        let i = unsafe { add(tr, c.as_ptr(), false, -1) };
        if let Some(v) = float.filter(|&v| v != 0) {
            self.set_config_int("fxfloat_focus", v);
        }
        (i >= 0).then_some(i)
    }

    /// La valeur BRUTE d'un parametre (pour un JSFX : la valeur du slider).
    pub fn track_fx_param(&self, guid: &str, fx: i32, param: i32) -> Option<f64> {
        type Get = unsafe extern "C" fn(P, i32, i32, *mut f64, *mut f64) -> f64;
        let tr = self.track_of(guid)?;
        let g: Get = self.tf(c"TrackFX_GetParam")?;
        let (mut lo, mut hi) = (0.0, 0.0);
        // SAFETY: piste vivante ; sorties a nous.
        Some(unsafe { g(tr, fx, param, &mut lo, &mut hi) })
    }

    pub fn set_track_fx_param(&self, guid: &str, fx: i32, param: i32, v: f64) -> bool {
        type Set = unsafe extern "C" fn(P, i32, i32, f64) -> bool;
        match (self.track_of(guid), self.tf::<Set>(c"TrackFX_SetParam")) {
            // SAFETY: piste vivante.
            (Some(tr), Some(s)) => unsafe { s(tr, fx, param, v) },
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Les items et les prises
    // -----------------------------------------------------------------------

    pub fn set_item_value(&self, guid: &str, key: &str, v: f64) -> bool {
        let (Some(it), Some(f)) = (
            self.find_item(guid, None),
            self.tf::<SetVal>(c"SetMediaItemInfo_Value"),
        ) else {
            return false;
        };
        let Ok(c) = CString::new(key) else {
            return false;
        };
        // SAFETY: item vivant.
        unsafe { f(it, c.as_ptr(), v) }
    }

    /// Efface un item.
    pub fn delete_item(&self, guid: &str) -> bool {
        type Track = unsafe extern "C" fn(P) -> P;
        type Del = unsafe extern "C" fn(P, P) -> bool;
        let (Some(it), Some(t), Some(d)) = (
            self.find_item(guid, None),
            self.tf::<Track>(c"GetMediaItem_Track"),
            self.tf::<Del>(c"DeleteTrackMediaItem"),
        ) else {
            return false;
        };
        // SAFETY: item et piste vivants.
        unsafe {
            let tr = t(it);
            !tr.is_null() && d(tr, it)
        }
    }

    /// Deplace un item sur une autre piste.
    pub fn move_item_to_track(&self, item: &str, track: &str) -> bool {
        type Mv = unsafe extern "C" fn(P, P) -> bool;
        let (Some(it), Some(tr), Some(m)) = (
            self.find_item(item, None),
            self.track_of(track),
            self.tf::<Mv>(c"MoveMediaItemToTrack"),
        ) else {
            return false;
        };
        // SAFETY: item et piste vivants.
        unsafe { m(it, tr) }
    }

    /// Deplace ces pistes (par GUID) juste avant la piste d'indice `before`
    /// (0 = la premiere ; le nombre de pistes = a la fin), comme un glisser
    /// dans les en-tetes de REAPER. Elles restent choisies.
    pub fn reorder_tracks(&self, guids: &[String], before: i32) -> bool {
        type Sel = unsafe extern "C" fn(P, bool);
        type Cmd = unsafe extern "C" fn(i32, i32);
        type Reorder = unsafe extern "C" fn(i32, i32) -> bool;
        let (Some(sel), Some(cmd), Some(re)) = (
            self.tf::<Sel>(c"SetTrackSelected"),
            self.tf::<Cmd>(c"Main_OnCommand"),
            self.tf::<Reorder>(c"ReorderSelectedTracks"),
        ) else {
            return false;
        };
        let tracks: Vec<P> = guids.iter().filter_map(|g| self.track_of(g)).collect();
        if tracks.is_empty() {
            return false;
        }
        // SAFETY: fil principal ; 40297 = Piste : deselectionner toutes les
        // pistes ; pistes vivantes ; bool ReorderSelectedTracks(int, int).
        unsafe {
            cmd(40297, 0);
            for &t in &tracks {
                sel(t, true);
            }
            re(before.max(0), 0)
        }
    }

    /// Coupe un item a `t` (s, temps du projet). Rend le GUID de la partie
    /// droite (l'item d'origine garde la gauche).
    pub fn split_item(&self, guid: &str, t: f64) -> Option<String> {
        type Split = unsafe extern "C" fn(P, f64) -> P;
        let it = self.find_item(guid, None)?;
        let f = self.tf::<Split>(c"SplitMediaItem")?;
        // SAFETY: MediaItem* SplitMediaItem(MediaItem*, double) ; item vivant.
        let right = unsafe { f(it, t) };
        if right.is_null() {
            return None;
        }
        self.it_str(right, "GUID", None)
    }

    /// Le GUID de la prise active d'un item.
    pub fn active_take_guid(&self, guid: &str) -> Option<String> {
        type Act = unsafe extern "C" fn(P) -> P;
        type TStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let it = self.find_item(guid, None)?;
        let at = self.tf::<Act>(c"GetActiveTake")?;
        let ts = self.tf::<TStr>(c"GetSetMediaItemTakeInfo_String")?;
        // SAFETY: item vivant ; prise verifiee non nulle ; tampon de 64 octets.
        unsafe {
            let tk = at(it);
            if tk.is_null() {
                return None;
            }
            let mut buf = [0u8; 64];
            if !ts(tk, c"GUID".as_ptr(), buf.as_mut_ptr() as *mut c_char, false) {
                return None;
            }
            let s = std::ffi::CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned();
            (!s.is_empty()).then_some(s)
        }
    }

    /// Le magnetisme de REAPER sur un temps (s) : sa grille, ses reglages ;
    /// le temps tel quel si le magnetisme est coupe.
    pub fn snap_time(&self, t: f64) -> f64 {
        type F = unsafe extern "C" fn(P, f64) -> f64;
        match self.tf::<F>(c"SnapToGrid") {
            // SAFETY: double SnapToGrid(ReaProject*, double) ; projet nul =
            // courant.
            Some(f) => unsafe { f(null_mut(), t) },
            None => t,
        }
    }

    /// Un fichier audio pose sur une piste a `pos` (s), en un item de toute
    /// sa longueur. Les cretes sont construites. Rend le GUID de l'item.
    pub fn insert_audio_item(&self, track: &str, path: &str, pos: f64) -> Option<String> {
        type Add = unsafe extern "C" fn(P) -> P;
        type AddTake = unsafe extern "C" fn(P) -> P;
        type Create = unsafe extern "C" fn(*const c_char) -> P;
        type SetSrc = unsafe extern "C" fn(P, P) -> bool;
        type Len = unsafe extern "C" fn(P, *mut bool) -> f64;
        type Peaks = unsafe extern "C" fn(P, i32) -> i32;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        type Upd = unsafe extern "C" fn(P) -> bool;
        let tr = self.track_of(track)?;
        let add: Add = self.tf(c"AddMediaItemToTrack")?;
        let add_take: AddTake = self.tf(c"AddTakeToMediaItem")?;
        let create: Create = self.tf(c"PCM_Source_CreateFromFile")?;
        let set_src: SetSrc = self.tf(c"SetMediaItemTake_Source")?;
        let len: Len = self.tf(c"GetMediaSourceLength")?;
        let peaks: Peaks = self.tf(c"PCM_Source_BuildPeaks")?;
        let set: SetVal = self.tf(c"SetMediaItemInfo_Value")?;
        let upd: Upd = self.tf(c"UpdateItemInProject")?;
        let c = CString::new(path).ok()?;
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // SAFETY: piste vivante ; chaque pointeur rendu par REAPER est verifie
        // non nul ; la source passe a la prise (REAPER la detruit avec elle).
        unsafe {
            let src = create(c.as_ptr());
            if src.is_null() {
                return None;
            }
            let mut qn = false;
            let l = len(src, &mut qn);
            let l = if qn { self.qn_to_time(l) } else { l };
            if l <= 0.0 {
                return None;
            }
            let it = add(tr);
            if it.is_null() {
                return None;
            }
            let tk = add_take(it);
            if tk.is_null() {
                return None;
            }
            set_src(tk, src);
            self.tk_str(tk, "P_NAME", Some(&name));
            set(it, c"D_POSITION".as_ptr(), pos.max(0.0));
            set(it, c"D_LENGTH".as_ptr(), l);
            // Les cretes : la vue les lit tout de suite.
            if peaks(src, 0) != 0 {
                let t0 = std::time::Instant::now();
                while peaks(src, 1) != 0 && t0.elapsed().as_secs_f64() < 10.0 {}
                peaks(src, 2);
            }
            upd(it);
            self.it_str(it, "GUID", None)
        }
    }

    /// Une copie d'un item sur une piste, a `pos` (s) : son etat entier (ses
    /// prises, ses donnees privees), de nouveaux GUID. Rend le GUID.
    pub fn copy_item(&self, item: &str, track: &str, pos: f64) -> Option<String> {
        type GetChunk = unsafe extern "C" fn(P, *mut c_char, i32, bool) -> bool;
        type SetChunk = unsafe extern "C" fn(P, *const c_char, bool) -> bool;
        type Add = unsafe extern "C" fn(P) -> P;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        let src = self.find_item(item, None)?;
        let tr = self.track_of(track)?;
        let get: GetChunk = self.tf(c"GetItemStateChunk")?;
        let put: SetChunk = self.tf(c"SetItemStateChunk")?;
        let add: Add = self.tf(c"AddMediaItemToTrack")?;
        let set: SetVal = self.tf(c"SetMediaItemInfo_Value")?;
        // SAFETY: tampon de la taille annoncee ; item et piste vivants.
        unsafe {
            let mut buf = vec![0u8; 1 << 20];
            if !get(
                src,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as i32,
                false,
            ) {
                return None;
            }
            let text = CStr::from_ptr(buf.as_ptr() as *const c_char).to_string_lossy();
            // Sans ses GUID, REAPER en donne de neufs a l'item et a ses prises.
            let chunk: String = text
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !(t.starts_with("IGUID ") || t.starts_with("GUID "))
                })
                .map(|l| format!("{l}\n"))
                .collect();
            let it = add(tr);
            if it.is_null() {
                return None;
            }
            let c = CString::new(chunk).ok()?;
            if !put(it, c.as_ptr(), false) {
                return None;
            }
            set(it, c"D_POSITION".as_ptr(), pos.max(0.0));
            self.it_str(it, "GUID", None)
        }
    }

    /// Un item MIDI vide sur une piste, de `t0` a `t1`. Rend son GUID.
    pub fn create_midi_item_on(&self, track: &str, name: &str, t0: f64, t1: f64) -> Option<String> {
        type NewMidi = unsafe extern "C" fn(P, f64, f64, *const bool) -> P;
        type Active = unsafe extern "C" fn(P) -> P;
        let tr = self.track_of(track)?;
        let nm: NewMidi = self.tf(c"CreateNewMIDIItemInProj")?;
        let at: Active = self.tf(c"GetActiveTake")?;
        // SAFETY: piste vivante ; l'item et sa prise sont verifies non nuls.
        unsafe {
            let it = nm(tr, t0, t1.max(t0 + 0.01), std::ptr::null());
            if it.is_null() {
                return None;
            }
            let tk = at(it);
            if !tk.is_null() {
                self.tk_str(tk, "P_NAME", Some(name));
            }
            self.it_str(it, "GUID", None)
        }
    }

    /// Rend active la prise `k` d'un item.
    pub fn set_active_take(&self, item: &str, k: usize) -> bool {
        type GetTake = unsafe extern "C" fn(P, i32) -> P;
        type SetAct = unsafe extern "C" fn(P);
        let (Some(it), Some(g), Some(s)) = (
            self.find_item(item, None),
            self.tf::<GetTake>(c"GetTake"),
            self.tf::<SetAct>(c"SetActiveTake"),
        ) else {
            return false;
        };
        // SAFETY: item vivant ; prise verifiee non nulle.
        unsafe {
            let tk = g(it, k as i32);
            if tk.is_null() {
                return false;
            }
            s(tk);
        }
        true
    }

    /// Choisit un seul item (desactive les autres) et rend la main.
    pub fn select_only_item(&self, guid: &str) -> bool {
        type Sel = unsafe extern "C" fn(P, bool);
        type Cmd = unsafe extern "C" fn(i32, i32);
        let (Some(it), Some(s), Some(c)) = (
            self.find_item(guid, None),
            self.tf::<Sel>(c"SetMediaItemSelected"),
            self.tf::<Cmd>(c"Main_OnCommand"),
        ) else {
            return false;
        };
        // SAFETY: fil principal ; 40289 = Item : enlever toute selection.
        unsafe {
            c(40289, 0);
            s(it, true);
        }
        true
    }

    /// Un bloc d'annulation (un seul pas pour plusieurs gestes).
    pub fn undo_begin(&self) {
        type F = unsafe extern "C" fn(P);
        if let Some(f) = self.tf::<F>(c"Undo_BeginBlock2") {
            // SAFETY: projet nul = courant.
            unsafe { f(null_mut()) };
        }
    }

    /// Ferme le bloc. `what` : 1 pistes, 4 items, -1 tout.
    pub fn undo_end(&self, label: &str, what: i32) {
        type F = unsafe extern "C" fn(P, *const c_char, i32);
        if let (Some(f), Ok(c)) = (self.tf::<F>(c"Undo_EndBlock2"), CString::new(label)) {
            // SAFETY: projet nul = courant ; chaine vivante.
            unsafe { f(null_mut(), c.as_ptr(), what) };
        }
    }

    /// Un pas d'annulation pour ce qui vient de changer (des valeurs posees
    /// pendant un geste, sans bloc) : REAPER retient l'etat present, annuler
    /// revient a celui d'avant le geste.
    pub fn undo_point(&self, label: &str) {
        type F = unsafe extern "C" fn(P, *const c_char);
        if let (Some(f), Ok(c)) = (self.tf::<F>(c"Undo_OnStateChange2"), CString::new(label)) {
            // SAFETY: projet nul = courant ; chaine vivante.
            unsafe { f(null_mut(), c.as_ptr()) };
        }
    }

    /// Redessine l'arrangement et la liste des pistes.
    pub fn refresh_arrange(&self) {
        type Upd = unsafe extern "C" fn();
        type Adj = unsafe extern "C" fn(bool);
        // SAFETY: fil principal.
        unsafe {
            if let Some(f) = self.tf::<Adj>(c"TrackList_AdjustWindows") {
                f(false);
            }
            if let Some(f) = self.tf::<Upd>(c"UpdateArrange") {
                f();
            }
        }
    }

    /// La prise d'un item par son GUID de prise : une donnee privee.
    pub fn take_ext(&self, item: &str, take: &str, key: &str) -> Option<String> {
        let it = self.find_item(item, None)?;
        let tk = self.take_of(it, take)?;
        self.tk_str(tk, &format!("P_EXT:{key}"), None)
            .filter(|s| !s.is_empty())
    }

    pub fn set_take_ext(&self, item: &str, take: &str, key: &str, value: &str) -> bool {
        let Some(it) = self.find_item(item, None) else {
            return false;
        };
        self.take_of(it, take).is_some_and(|tk| {
            self.tk_str(tk, &format!("P_EXT:{key}"), Some(value))
                .is_some()
        })
    }

    /// Une valeur de la prise active d'un item (`D_STARTOFFS`, `D_PITCH`...).
    pub fn set_active_take_value(&self, item: &str, key: &str, v: f64) -> bool {
        type Active = unsafe extern "C" fn(P) -> P;
        type SetTk = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        let (Some(it), Some(act), Some(f)) = (
            self.find_item(item, None),
            self.tf::<Active>(c"GetActiveTake"),
            self.tf::<SetTk>(c"SetMediaItemTakeInfo_Value"),
        ) else {
            return false;
        };
        let Ok(c) = CString::new(key) else {
            return false;
        };
        // SAFETY: item vivant ; prise active verifiee non nulle.
        unsafe {
            let tk = act(it);
            !tk.is_null() && f(tk, c.as_ptr(), v)
        }
    }

    fn take_of(&self, it: P, guid: &str) -> Option<P> {
        type CountTakes = unsafe extern "C" fn(P) -> i32;
        type GetTake = unsafe extern "C" fn(P, i32) -> P;
        let ct: CountTakes = self.tf(c"CountTakes")?;
        let gt: GetTake = self.tf(c"GetTake")?;
        // SAFETY: item vivant ; indices dans les bornes.
        unsafe {
            (0..ct(it))
                .map(|k| gt(it, k))
                .find(|&tk| !tk.is_null() && self.tk_str(tk, "GUID", None).as_deref() == Some(guid))
        }
    }
}
