//! Ce que CP Guitar Tab demande a REAPER : l'item a transcrire, la carte de
//! tempo (temps -> noires, la mesure d'un instant), et le resultat pose dans
//! le projet (une piste et un item MIDI). Plus l'ouverture d'un document par
//! son application (la partition dans MuseScore).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

/// L'audio d'un item choisi.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemAudio {
    pub path: String,
    /// Position et longueur de l'item dans le projet (s).
    pub pos: f64,
    pub len: f64,
    /// Decalage dans le fichier (s) et vitesse de la prise.
    pub offs: f64,
    pub rate: f64,
    pub name: String,
    /// Le numero de sa piste (1 = la premiere), et son nom.
    pub track: i32,
    pub track_name: String,
}

/// Une note a poser en MIDI (temps du projet).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiNote {
    pub t0: f64,
    pub t1: f64,
    pub pitch: i32,
    pub vel: i32,
}

impl Reaper {
    /// Le premier item choisi, s'il joue un fichier audio.
    pub fn selected_item_audio(&self) -> Option<ItemAudio> {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type TakeSrc = unsafe extern "C" fn(P) -> P;
        type SrcName = unsafe extern "C" fn(P, *mut c_char, i32);
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        type TakeName = unsafe extern "C" fn(P) -> *const c_char;
        type ItemTrack = unsafe extern "C" fn(P) -> P;
        // SAFETY: signatures du SDK : GetSelectedMediaItem, GetActiveTake,
        // GetMediaItemTake_Source, GetMediaSourceFileName,
        // GetMediaItemInfo_Value, GetMediaItemTakeInfo_Value, GetTakeName,
        // GetMediaItem_Track, GetMediaTrackInfo_Value.
        let (si, at, ts, sn, gi, gt, tn, it, gtr) = unsafe {
            (
                self.api::<SelItem>(c"GetSelectedMediaItem")?,
                self.api::<ActTake>(c"GetActiveTake")?,
                self.api::<TakeSrc>(c"GetMediaItemTake_Source")?,
                self.api::<SrcName>(c"GetMediaSourceFileName")?,
                self.api::<GetVal>(c"GetMediaItemInfo_Value")?,
                self.api::<GetVal>(c"GetMediaItemTakeInfo_Value")?,
                self.api::<TakeName>(c"GetTakeName")?,
                self.api::<ItemTrack>(c"GetMediaItem_Track")?,
                self.api::<GetVal>(c"GetMediaTrackInfo_Value")?,
            )
        };
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls ; tampon de
        // 4096 octets annonce.
        unsafe {
            let item = si(null_mut(), 0);
            if item.is_null() {
                return None;
            }
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
            let track = if tr.is_null() {
                0
            } else {
                gtr(tr, c"IP_TRACKNUMBER".as_ptr()) as i32
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
                track_name: String::new(),
            })
        }
    }

    /// Un instant du projet en noires (la carte de tempo comprise).
    pub fn time_to_qn(&self, t: f64) -> f64 {
        type F = unsafe extern "C" fn(P, f64) -> f64;
        // SAFETY: double TimeMap2_timeToQN(ReaProject*, double).
        match unsafe { self.api::<F>(c"TimeMap2_timeToQN") } {
            // SAFETY: projet nul = courant.
            Some(f) => unsafe { f(null_mut(), t) },
            None => t * self.master_tempo() / 60.0,
        }
    }

    /// Les changements de chiffrage du projet : (noire, chiffrage), le
    /// chiffrage du debut compris. Les marqueurs de tempo seul (sans
    /// chiffrage) n'y sont pas.
    pub fn meter_changes(&self) -> Vec<(f64, (i32, i32))> {
        self.time_map().0
    }

    /// Le temps du projet : ses changements de chiffrage, et ses changements
    /// de tempo (noire, BPM ; le tempo du debut compris).
    #[allow(clippy::type_complexity)]
    pub fn time_map(&self) -> (Vec<(f64, (i32, i32))>, Vec<(f64, f64)>) {
        type Count = unsafe extern "C" fn(P) -> i32;
        type Get = unsafe extern "C" fn(
            P,
            i32,
            *mut f64,
            *mut i32,
            *mut f64,
            *mut f64,
            *mut i32,
            *mut i32,
            *mut bool,
        ) -> bool;
        let (_, sig0, bpm0) = self.measure_at(0.0);
        let mut out = vec![(0.0, sig0)];
        let mut tempos = vec![(0.0, bpm0)];
        // SAFETY: int CountTempoTimeSigMarkers(ReaProject*) ; bool
        // GetTempoTimeSigMarker(ReaProject*, int, double* timepos, int*
        // measurepos, double* beatpos, double* bpm, int* num, int* denom,
        // bool* linear).
        let fns = unsafe {
            (
                self.api::<Count>(c"CountTempoTimeSigMarkers"),
                self.api::<Get>(c"GetTempoTimeSigMarker"),
            )
        };
        let (Some(count), Some(get)) = fns else {
            return (out, tempos);
        };
        // SAFETY: projet nul = courant.
        let n = unsafe { count(null_mut()) }.clamp(0, 100_000);
        for i in 0..n {
            let (mut t, mut mp, mut bp, mut bpm) = (0f64, 0i32, 0f64, 0f64);
            let (mut num, mut den, mut lin) = (0i32, 0i32, false);
            // SAFETY: sorties a nous ; projet nul = courant.
            let ok = unsafe {
                get(
                    null_mut(),
                    i,
                    &mut t,
                    &mut mp,
                    &mut bp,
                    &mut bpm,
                    &mut num,
                    &mut den,
                    &mut lin,
                )
            };
            if ok && num > 0 && den > 0 {
                out.push((self.time_to_qn(t), (num, den)));
            }
            if ok && bpm > 1.0 {
                tempos.push((self.time_to_qn(t), bpm));
            }
        }
        (out, tempos)
    }

    /// La mesure qui contient l'instant `t` : (son debut en noires,
    /// signature, tempo).
    pub fn measure_at(&self, t: f64) -> (f64, (i32, i32), f64) {
        type ToBeats = unsafe extern "C" fn(P, f64, *mut i32, *mut i32, *mut f64, *mut i32) -> f64;
        type Info =
            unsafe extern "C" fn(P, i32, *mut f64, *mut f64, *mut i32, *mut i32, *mut f64) -> f64;
        // SAFETY: double TimeMap2_timeToBeats(ReaProject*, double, int*, int*,
        // double*, int*) ; double TimeMap_GetMeasureInfo(ReaProject*, int,
        // double*, double*, int*, int*, double*).
        let fns = unsafe {
            (
                self.api::<ToBeats>(c"TimeMap2_timeToBeats"),
                self.api::<Info>(c"TimeMap_GetMeasureInfo"),
            )
        };
        let (Some(tb), Some(mi)) = fns else {
            return (0.0, (4, 4), self.master_tempo());
        };
        let mut meas = 0i32;
        // SAFETY: sorties a nous ou nulles ; projet nul = courant.
        unsafe { tb(null_mut(), t, &mut meas, null_mut(), null_mut(), null_mut()) };
        let (mut qs, mut qe, mut n, mut d, mut bpm) = (0f64, 0f64, 4i32, 4i32, 120f64);
        // SAFETY: idem.
        unsafe { mi(null_mut(), meas, &mut qs, &mut qe, &mut n, &mut d, &mut bpm) };
        (qs, (n.max(1), d.max(1)), bpm)
    }

    /// Une piste neuve sous la piste `after` (numero 1..), nommee, avec un
    /// item MIDI de `t0` a `t1` qui porte les notes. Un pas d'annulation.
    pub fn insert_midi_track(
        &self,
        after: i32,
        name: &str,
        t0: f64,
        t1: f64,
        notes: &[MidiNote],
    ) -> Result<(), String> {
        type Ins = unsafe extern "C" fn(i32, bool);
        type GetTrack = unsafe extern "C" fn(P, i32) -> P;
        type SetStr = unsafe extern "C" fn(P, *const c_char, *mut c_char, bool) -> bool;
        type NewMidi = unsafe extern "C" fn(P, f64, f64, *const bool) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type Ppq = unsafe extern "C" fn(P, f64) -> f64;
        type InsNote =
            unsafe extern "C" fn(P, bool, bool, f64, f64, i32, i32, i32, *const bool) -> bool;
        type Sort = unsafe extern "C" fn(P) -> bool;
        type UndoBegin = unsafe extern "C" fn(P);
        type UndoEnd = unsafe extern "C" fn(P, *const c_char, i32);
        type Update = unsafe extern "C" fn();
        let miss = || "REAPER API missing".to_string();
        // SAFETY: signatures du SDK : InsertTrackAtIndex, GetTrack,
        // GetSetMediaTrackInfo_String, CreateNewMIDIItemInProj, GetActiveTake,
        // MIDI_GetPPQPosFromProjTime, MIDI_InsertNote, MIDI_Sort,
        // GetSetMediaItemTakeInfo_String, Undo_BeginBlock2, Undo_EndBlock2,
        // UpdateArrange.
        unsafe {
            let ins = self.api::<Ins>(c"InsertTrackAtIndex").ok_or_else(miss)?;
            let gtr = self.api::<GetTrack>(c"GetTrack").ok_or_else(miss)?;
            let trs = self
                .api::<SetStr>(c"GetSetMediaTrackInfo_String")
                .ok_or_else(miss)?;
            let nm = self
                .api::<NewMidi>(c"CreateNewMIDIItemInProj")
                .ok_or_else(miss)?;
            let at = self.api::<ActTake>(c"GetActiveTake").ok_or_else(miss)?;
            let ppq = self
                .api::<Ppq>(c"MIDI_GetPPQPosFromProjTime")
                .ok_or_else(miss)?;
            let ins_note = self.api::<InsNote>(c"MIDI_InsertNote").ok_or_else(miss)?;
            let sort = self.api::<Sort>(c"MIDI_Sort").ok_or_else(miss)?;
            let tks = self
                .api::<SetStr>(c"GetSetMediaItemTakeInfo_String")
                .ok_or_else(miss)?;
            let ub = self.api::<UndoBegin>(c"Undo_BeginBlock2");
            let ue = self.api::<UndoEnd>(c"Undo_EndBlock2");
            let upd = self.api::<Update>(c"UpdateArrange");
            if let Some(f) = ub {
                f(null_mut());
            }
            let idx = after.max(0); // InsertTrackAtIndex : 0 = en tete
            ins(idx, true);
            let tr = gtr(null_mut(), idx);
            if tr.is_null() {
                if let Some(f) = ue {
                    f(null_mut(), c"CP Guitar Tab".as_ptr(), -1);
                }
                return Err("the new track was not created".into());
            }
            let cname = CString::new(name.replace('\0', " ")).unwrap_or_default();
            let mut nbuf = cname.as_bytes_with_nul().to_vec();
            trs(
                tr,
                c"P_NAME".as_ptr(),
                nbuf.as_mut_ptr() as *mut c_char,
                true,
            );
            let item = nm(tr, t0, t1.max(t0 + 0.01), std::ptr::null());
            let take = if item.is_null() { null_mut() } else { at(item) };
            if !take.is_null() {
                let mut nbuf = cname.as_bytes_with_nul().to_vec();
                tks(
                    take,
                    c"P_NAME".as_ptr(),
                    nbuf.as_mut_ptr() as *mut c_char,
                    true,
                );
                let nosort = true;
                for n in notes {
                    let a = ppq(take, n.t0);
                    let b = ppq(take, n.t1);
                    ins_note(
                        take,
                        false,
                        false,
                        a,
                        b.max(a + 1.0),
                        0,
                        n.pitch.clamp(0, 127),
                        n.vel.clamp(1, 127),
                        &nosort,
                    );
                }
                sort(take);
            }
            if let Some(f) = ue {
                f(null_mut(), c"CP Guitar Tab: transcription".as_ptr(), -1);
            }
            if let Some(f) = upd {
                f();
            }
            if take.is_null() {
                return Err("the MIDI item was not created".into());
            }
        }
        Ok(())
    }
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: P,
        op: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> P;
}

/// Ouvre un document par son application (« open » de l'Explorateur), dans
/// son propre dossier. Vrai si Windows l'a lance.
pub fn open_document(path: &std::path::Path) -> bool {
    let file = crate::win::wide_os(path.as_os_str());
    let dir = path
        .parent()
        .map(|d| crate::win::wide_os(d.as_os_str()))
        .unwrap_or_else(|| vec![0]);
    let op = crate::win::wide("open");
    const SW_SHOWNORMAL: i32 = 1;
    // SAFETY: chaines terminees par zero, vivantes pendant l'appel.
    let r = unsafe {
        ShellExecuteW(
            null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            dir.as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    r as isize > 32
}
