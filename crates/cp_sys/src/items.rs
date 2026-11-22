//! Les items du projet par leur GUID (jamais un pointeur garde : un item
//! efface ou un rechargement du module ne laisse rien de pendant) : l'audio
//! d'un item, ses donnees privees (`P_EXT`), et l'item MIDI de Guitar Tab
//! (le creer, lire et reecrire ses notes, son empreinte).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;
use crate::tab::ItemAudio;

type P = *mut c_void;

/// Une note d'un item MIDI, en noires du projet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QnNote {
    pub qn0: f64,
    pub qn1: f64,
    pub pitch: i32,
    pub chan: i32,
    pub vel: i32,
}

type GetStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;

impl Reaper {
    fn f<F: Copy>(&self, name: &CStr) -> Option<F> {
        // SAFETY: les appelants nomment une fonction du SDK avec sa signature.
        unsafe { self.api::<F>(name) }
    }

    /// Lit (ou ecrit) une chaine d'un item : « GUID », « P_EXT:cle »...
    fn item_str(&self, item: P, key: &str, set: Option<&str>) -> Option<String> {
        let gs: GetStr = self.f(c"GetSetMediaItemInfo_String")?;
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
        // SAFETY: item vivant (vient de REAPER, a ce battement), tampon assez
        // grand pour une lecture (8 Ko) ou contenant la valeur a ecrire.
        let ok = unsafe {
            gs(
                item,
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

    /// L'item d'un GUID (parcourt le projet ; quelques milliers d'items se
    /// parcourent en bien moins d'une milliseconde).
    fn item_of(&self, guid: &str) -> Option<P> {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(P, i32) -> P;
        let count: Count = self.f(c"CountMediaItems")?;
        let get: Get = self.f(c"GetMediaItem")?;
        // SAFETY: projet nul = courant.
        let n = unsafe { count(null_mut()) };
        for i in 0..n {
            // SAFETY: indice dans les bornes rendues par REAPER.
            let it = unsafe { get(null_mut(), i) };
            if !it.is_null() && self.item_str(it, "GUID", None).as_deref() == Some(guid) {
                return Some(it);
            }
        }
        None
    }

    pub fn item_exists(&self, guid: &str) -> bool {
        self.item_of(guid).is_some()
    }

    /// Le GUID du premier item choisi.
    pub fn selected_item_guid(&self) -> Option<String> {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        let si: SelItem = self.f(c"GetSelectedMediaItem")?;
        // SAFETY: projet nul = courant.
        let it = unsafe { si(null_mut(), 0) };
        (!it.is_null())
            .then(|| self.item_str(it, "GUID", None))
            .flatten()
    }

    /// Une donnee privee d'un item (`P_EXT:<cle>`), lue ou ecrite.
    pub fn item_ext(&self, guid: &str, key: &str) -> Option<String> {
        let it = self.item_of(guid)?;
        self.item_str(it, &format!("P_EXT:{key}"), None)
            .filter(|s| !s.is_empty())
    }

    pub fn set_item_ext(&self, guid: &str, key: &str, value: &str) -> bool {
        let Some(it) = self.item_of(guid) else {
            return false;
        };
        self.item_str(it, &format!("P_EXT:{key}"), Some(value))
            .is_some()
    }

    /// L'audio d'un item par son GUID (relu : il a pu bouger).
    pub fn item_audio(&self, guid: &str) -> Option<ItemAudio> {
        type ActTake = unsafe extern "C" fn(P) -> P;
        type TakeSrc = unsafe extern "C" fn(P) -> P;
        type SrcName = unsafe extern "C" fn(P, *mut c_char, i32);
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        type TakeName = unsafe extern "C" fn(P) -> *const c_char;
        type ItemTrack = unsafe extern "C" fn(P) -> P;
        type TrStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        let item = self.item_of(guid)?;
        let at: ActTake = self.f(c"GetActiveTake")?;
        let ts: TakeSrc = self.f(c"GetMediaItemTake_Source")?;
        let sn: SrcName = self.f(c"GetMediaSourceFileName")?;
        let gi: GetVal = self.f(c"GetMediaItemInfo_Value")?;
        let gt: GetVal = self.f(c"GetMediaItemTakeInfo_Value")?;
        let tn: TakeName = self.f(c"GetTakeName")?;
        let it: ItemTrack = self.f(c"GetMediaItem_Track")?;
        let gtr: GetVal = self.f(c"GetMediaTrackInfo_Value")?;
        let trs: TrStr = self.f(c"GetSetMediaTrackInfo_String")?;
        // SAFETY: pointeurs rendus par REAPER a l'instant, verifies non nuls ;
        // tampons de 4096 octets annonces.
        unsafe {
            let take = at(item);
            if take.is_null() {
                return None;
            }
            let src = ts(take);
            if src.is_null() {
                return None;
            }
            let mut buf = vec![0u8; 4096];
            sn(src, buf.as_mut_ptr() as *mut c_char, 4096);
            let path = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned();
            if path.is_empty() {
                return None;
            }
            let np = tn(take);
            let name = if np.is_null() {
                String::new()
            } else {
                CStr::from_ptr(np).to_string_lossy().into_owned()
            };
            let tr = it(item);
            let (track, track_name) = if tr.is_null() {
                (0, String::new())
            } else {
                let mut nb = vec![0u8; 512];
                trs(
                    tr,
                    c"P_NAME".as_ptr(),
                    nb.as_mut_ptr() as *mut c_char,
                    false,
                );
                (
                    gtr(tr, c"IP_TRACKNUMBER".as_ptr()) as i32,
                    CStr::from_ptr(nb.as_ptr() as *const c_char)
                        .to_string_lossy()
                        .into_owned(),
                )
            };
            let rate = gt(take, c"D_PLAYRATE".as_ptr());
            Some(ItemAudio {
                path,
                pos: gi(item, c"D_POSITION".as_ptr()),
                len: gi(item, c"D_LENGTH".as_ptr()),
                offs: gt(take, c"D_STARTOFFS".as_ptr()),
                rate: if rate > 0.0 { rate } else { 1.0 },
                name,
                track,
                track_name,
            })
        }
    }

    /// La prise MIDI active d'un item.
    fn midi_take(&self, guid: &str) -> Option<P> {
        type ActTake = unsafe extern "C" fn(P) -> P;
        type IsMidi = unsafe extern "C" fn(P, bool) -> bool;
        let item = self.item_of(guid)?;
        let at: ActTake = self.f(c"GetActiveTake")?;
        let im: IsMidi = self.f(c"TakeIsMIDI")?;
        // SAFETY: item vivant ; prise verifiee non nulle.
        unsafe {
            let t = at(item);
            (!t.is_null() && im(t, false)).then_some(t)
        }
    }

    /// Une piste neuve sous la piste `after` (1..), nommee, avec un item MIDI
    /// vide de `t0` a `t1`. Rend le GUID de l'item.
    pub fn create_midi_item(&self, after: i32, name: &str, t0: f64, t1: f64) -> Option<String> {
        type Ins = unsafe extern "C" fn(i32, bool);
        type GetTrack = unsafe extern "C" fn(P, i32) -> P;
        type NewMidi = unsafe extern "C" fn(P, f64, f64, *const bool) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        let ins: Ins = self.f(c"InsertTrackAtIndex")?;
        let gtr: GetTrack = self.f(c"GetTrack")?;
        let trs: GetStr = self.f(c"GetSetMediaTrackInfo_String")?;
        let tks: GetStr = self.f(c"GetSetMediaItemTakeInfo_String")?;
        let nm: NewMidi = self.f(c"CreateNewMIDIItemInProj")?;
        let at: ActTake = self.f(c"GetActiveTake")?;
        let cname = CString::new(name.replace('\0', " ")).ok()?;
        // SAFETY: appels du SDK sur le fil principal ; la piste et l'item sont
        // verifies non nuls ; tampons termines par zero.
        unsafe {
            let idx = after.max(0);
            ins(idx, true);
            let tr = gtr(null_mut(), idx);
            if tr.is_null() {
                return None;
            }
            let mut nb = cname.as_bytes_with_nul().to_vec();
            trs(tr, c"P_NAME".as_ptr(), nb.as_mut_ptr() as *mut c_char, true);
            let item = nm(tr, t0, t1.max(t0 + 0.01), std::ptr::null());
            if item.is_null() {
                return None;
            }
            let take = at(item);
            if !take.is_null() {
                let mut nb = cname.as_bytes_with_nul().to_vec();
                tks(
                    take,
                    c"P_NAME".as_ptr(),
                    nb.as_mut_ptr() as *mut c_char,
                    true,
                );
            }
            self.item_str(item, "GUID", None)
        }
    }

    /// Les notes d'un item MIDI, en noires du projet.
    pub fn midi_notes(&self, guid: &str) -> Option<Vec<QnNote>> {
        type Count = unsafe extern "C" fn(P, *mut i32, *mut i32, *mut i32) -> i32;
        type GetNote = unsafe extern "C" fn(
            P,
            i32,
            *mut bool,
            *mut bool,
            *mut f64,
            *mut f64,
            *mut i32,
            *mut i32,
            *mut i32,
        ) -> bool;
        type ToQn = unsafe extern "C" fn(P, f64) -> f64;
        let take = self.midi_take(guid)?;
        let count: Count = self.f(c"MIDI_CountEvts")?;
        let get: GetNote = self.f(c"MIDI_GetNote")?;
        let qn: ToQn = self.f(c"MIDI_GetProjQNFromPPQPos")?;
        let mut out = Vec::new();
        // SAFETY: prise MIDI vivante ; sorties a nous.
        unsafe {
            let mut n = 0i32;
            count(take, &mut n, null_mut(), null_mut());
            for i in 0..n {
                let (mut s, mut m, mut a, mut b, mut c, mut p, mut v) =
                    (false, false, 0f64, 0f64, 0i32, 0i32, 0i32);
                if get(
                    take, i, &mut s, &mut m, &mut a, &mut b, &mut c, &mut p, &mut v,
                ) && !m
                {
                    out.push(QnNote {
                        qn0: qn(take, a),
                        qn1: qn(take, b),
                        pitch: p,
                        chan: c,
                        vel: v,
                    });
                }
            }
        }
        Some(out)
    }

    /// Remplace toutes les notes d'un item MIDI. Un pas d'annulation.
    pub fn set_midi_notes(&self, guid: &str, notes: &[QnNote], undo: &str) -> bool {
        type Count = unsafe extern "C" fn(P, *mut i32, *mut i32, *mut i32) -> i32;
        type Del = unsafe extern "C" fn(P, i32) -> bool;
        type FromQn = unsafe extern "C" fn(P, f64) -> f64;
        type InsNote =
            unsafe extern "C" fn(P, bool, bool, f64, f64, i32, i32, i32, *const bool) -> bool;
        type Sort = unsafe extern "C" fn(P) -> bool;
        type UndoBegin = unsafe extern "C" fn(P);
        type UndoEnd = unsafe extern "C" fn(P, *const c_char, i32);
        type Update = unsafe extern "C" fn();
        let Some(take) = self.midi_take(guid) else {
            return false;
        };
        let (Some(count), Some(del), Some(ppq), Some(ins), Some(sort)) = (
            self.f::<Count>(c"MIDI_CountEvts"),
            self.f::<Del>(c"MIDI_DeleteNote"),
            self.f::<FromQn>(c"MIDI_GetPPQPosFromProjQN"),
            self.f::<InsNote>(c"MIDI_InsertNote"),
            self.f::<Sort>(c"MIDI_Sort"),
        ) else {
            return false;
        };
        let ub = self.f::<UndoBegin>(c"Undo_BeginBlock2");
        let ue = self.f::<UndoEnd>(c"Undo_EndBlock2");
        let upd = self.f::<Update>(c"UpdateArrange");
        let label = CString::new(undo).unwrap_or_default();
        // SAFETY: prise MIDI vivante ; appels du SDK sur le fil principal.
        unsafe {
            if let Some(f) = ub {
                f(null_mut());
            }
            let mut n = 0i32;
            count(take, &mut n, null_mut(), null_mut());
            for i in (0..n).rev() {
                del(take, i);
            }
            let nosort = true;
            for nt in notes {
                let a = ppq(take, nt.qn0);
                let b = ppq(take, nt.qn1).max(a + 1.0);
                ins(
                    take,
                    false,
                    false,
                    a,
                    b,
                    nt.chan.clamp(0, 15),
                    nt.pitch.clamp(0, 127),
                    nt.vel.clamp(1, 127),
                    &nosort,
                );
            }
            sort(take);
            if let Some(f) = ue {
                // 4 = UNDO_STATE_ITEMS.
                f(null_mut(), label.as_ptr(), 4);
            }
            if let Some(f) = upd {
                f();
            }
        }
        true
    }

    /// L'empreinte des notes d'un item MIDI : elle change a chaque
    /// modification (dans REAPER ou par nous).
    pub fn midi_hash(&self, guid: &str) -> Option<String> {
        type Hash = unsafe extern "C" fn(P, bool, *mut c_char, i32) -> bool;
        let take = self.midi_take(guid)?;
        let h: Hash = self.f(c"MIDI_GetHash")?;
        let mut buf = vec![0u8; 128];
        // SAFETY: prise vivante, tampon de 128 octets annonce.
        unsafe {
            if !h(take, true, buf.as_mut_ptr() as *mut c_char, 128) {
                return None;
            }
            Some(
                CStr::from_ptr(buf.as_ptr() as *const c_char)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    /// Le nom de la piste d'un item.
    pub fn item_track_name(&self, guid: &str) -> Option<String> {
        type ItemTrack = unsafe extern "C" fn(P) -> P;
        let item = self.item_of(guid)?;
        let it: ItemTrack = self.f(c"GetMediaItem_Track")?;
        let trs: GetStr = self.f(c"GetSetMediaTrackInfo_String")?;
        // SAFETY: item vivant ; tampon de 512 octets.
        unsafe {
            let tr = it(item);
            if tr.is_null() {
                return None;
            }
            let mut nb = vec![0u8; 512];
            trs(
                tr,
                c"P_NAME".as_ptr(),
                nb.as_mut_ptr() as *mut c_char,
                false,
            );
            Some(
                CStr::from_ptr(nb.as_ptr() as *const c_char)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    /// Une position en noires vers le temps du projet.
    pub fn qn_to_time(&self, qn: f64) -> f64 {
        type F = unsafe extern "C" fn(P, f64) -> f64;
        match self.f::<F>(c"TimeMap2_QNToTime") {
            // SAFETY: projet nul = courant.
            Some(f) => unsafe { f(null_mut(), qn) },
            None => qn * 60.0 / self.master_tempo(),
        }
    }

    /// Le curseur d'edition (la lecture le suit si elle tourne).
    pub fn set_edit_cursor(&self, t: f64) {
        type F = unsafe extern "C" fn(f64, bool, bool);
        if let Some(f) = self.f::<F>(c"SetEditCurPos") {
            // SAFETY: fil principal.
            unsafe { f(t.max(0.0), false, true) };
        }
    }
}
