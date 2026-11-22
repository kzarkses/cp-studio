//! Le projet de REAPER : son dossier, et poser un fichier (ou une section) a
//! la place du curseur d'edition.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

impl Reaper {
    /// Le dossier ou REAPER enregistre pour le projet courant
    /// (`GetProjectPathEx`) : celui du projet, ou, sans projet enregistre,
    /// celui des preferences. C'est celui que montre la barre de titre.
    pub fn record_path(&self) -> Option<PathBuf> {
        type F = unsafe extern "C" fn(P, *mut c_char, i32);
        // SAFETY: void GetProjectPathEx(ReaProject* proj, char* buf, int sz).
        let f = unsafe { self.api::<F>(c"GetProjectPathEx") }?;
        let mut buf = vec![0u8; 4096];
        // SAFETY: projet courant (nul), tampon de 4096 octets annonce.
        unsafe { f(null_mut(), buf.as_mut_ptr() as *mut c_char, 4096) };
        // SAFETY: REAPER termine la chaine par zero dans le tampon.
        let s = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned();
        (!s.trim().is_empty()).then(|| PathBuf::from(s.trim()))
    }

    /// Le dossier du projet courant (`None` s'il n'est pas enregistre).
    pub fn project_dir(&self) -> Option<PathBuf> {
        type F = unsafe extern "C" fn(i32, *mut c_char, i32) -> P;
        // SAFETY: ReaProject* EnumProjects(int idx, char* projfnOutOptional, int sz).
        let f = unsafe { self.api::<F>(c"EnumProjects") }?;
        let mut buf = vec![0u8; 4096];
        // SAFETY: tampon de 4096 octets annonce.
        unsafe { f(-1, buf.as_mut_ptr() as *mut c_char, 4096) };
        // SAFETY: REAPER termine la chaine par zero dans le tampon.
        let s = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned();
        if s.is_empty() {
            return None;
        }
        PathBuf::from(s).parent().map(|p| p.to_path_buf())
    }

    /// Pose `path` sur la piste courante au curseur d'edition (`InsertMedia`,
    /// mode 0). Avec `section` (secondes dans le fichier), l'item est rogne a
    /// la section. Un seul pas d'annulation.
    pub fn insert_media(&self, path: &str, section: Option<(f64, f64)>) -> Result<(), String> {
        type Insert = unsafe extern "C" fn(*const c_char, i32) -> i32;
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        type UndoBegin = unsafe extern "C" fn(P);
        type UndoEnd = unsafe extern "C" fn(P, *const c_char, i32);
        type Void = unsafe extern "C" fn();
        type Refresh = unsafe extern "C" fn(i32);
        // SAFETY: signatures de reaper_plugin_functions.h :
        // int InsertMedia(const char* file, int mode) ;
        // MediaItem* GetSelectedMediaItem(ReaProject*, int) ;
        // MediaItem_Take* GetActiveTake(MediaItem*) ;
        // bool SetMediaItemTakeInfo_Value(MediaItem_Take*, const char*, double) ;
        // bool SetMediaItemInfo_Value(MediaItem*, const char*, double) ;
        // void Undo_BeginBlock2(ReaProject*) ; void Undo_EndBlock2(ReaProject*, const char*, int) ;
        // void UpdateArrange() ; void PreventUIRefresh(int).
        let (insert, sel_item, act_take, set_take, set_item, ub, ue, upd, pur) = unsafe {
            (
                self.api::<Insert>(c"InsertMedia"),
                self.api::<SelItem>(c"GetSelectedMediaItem"),
                self.api::<ActTake>(c"GetActiveTake"),
                self.api::<SetVal>(c"SetMediaItemTakeInfo_Value"),
                self.api::<SetVal>(c"SetMediaItemInfo_Value"),
                self.api::<UndoBegin>(c"Undo_BeginBlock2"),
                self.api::<UndoEnd>(c"Undo_EndBlock2"),
                self.api::<Void>(c"UpdateArrange"),
                self.api::<Refresh>(c"PreventUIRefresh"),
            )
        };
        let insert = insert.ok_or("InsertMedia is missing")?;
        let c = CString::new(path).map_err(|e| e.to_string())?;
        // SAFETY: toutes les fonctions viennent de REAPER (signatures ci-dessus),
        // appelees sur le fil principal ; les chaines vivent pendant les appels.
        unsafe {
            if let Some(f) = ub {
                f(null_mut());
            }
            if let Some(f) = pur {
                f(1);
            }
            let ok = insert(c.as_ptr(), 0) != 0;
            let mut res = if ok {
                Ok(())
            } else {
                Err("REAPER refused to insert the file".to_string())
            };
            if let (true, Some((a, b))) = (ok, section) {
                let item = sel_item.map_or(null_mut(), |f| f(null_mut(), 0));
                let take = if item.is_null() {
                    null_mut()
                } else {
                    act_take.map_or(null_mut(), |f| f(item))
                };
                match (item.is_null() || take.is_null(), set_take, set_item) {
                    (false, Some(st), Some(si)) => {
                        st(take, c"D_STARTOFFS".as_ptr(), a);
                        si(item, c"D_LENGTH".as_ptr(), (b - a).max(0.0));
                    }
                    _ => res = Err("inserted, but the section could not be applied".into()),
                }
            }
            if let Some(f) = pur {
                f(-1);
            }
            if let Some(f) = upd {
                f();
            }
            if let Some(f) = ue {
                f(null_mut(), c"CP Studio: insert media".as_ptr(), -1);
            }
            res
        }
    }

    /// (position, longueur, decalage de prise) du premier item choisi, en
    /// secondes (verification par l'auto-test).
    pub fn first_selected_item(&self) -> Option<(f64, f64, f64)> {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        // SAFETY: GetSelectedMediaItem, GetActiveTake, GetMediaItemInfo_Value,
        // GetMediaItemTakeInfo_Value (double f(ptr, const char*)).
        let (si, at, gi, gt) = unsafe {
            (
                self.api::<SelItem>(c"GetSelectedMediaItem")?,
                self.api::<ActTake>(c"GetActiveTake")?,
                self.api::<GetVal>(c"GetMediaItemInfo_Value")?,
                self.api::<GetVal>(c"GetMediaItemTakeInfo_Value")?,
            )
        };
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls.
        unsafe {
            let item = si(null_mut(), 0);
            if item.is_null() {
                return None;
            }
            let take = at(item);
            let off = if take.is_null() {
                0.0
            } else {
                gt(take, c"D_STARTOFFS".as_ptr())
            };
            Some((
                gi(item, c"D_POSITION".as_ptr()),
                gi(item, c"D_LENGTH".as_ptr()),
                off,
            ))
        }
    }
}

impl Reaper {
    /// Apres un glisser-deposer : rogne a la section [a, b) (secondes) les
    /// items choisis qui viennent d'etre poses depuis `path` (decalage nul).
    /// Rend le nombre d'items rognes.
    pub fn trim_dropped(&self, path: &str, a: f64, b: f64) -> usize {
        type Count = unsafe extern "C" fn(P) -> i32;
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type TakeSrc = unsafe extern "C" fn(P) -> P;
        type SrcName = unsafe extern "C" fn(P, *mut c_char, i32);
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        type UndoBegin = unsafe extern "C" fn(P);
        type UndoEnd = unsafe extern "C" fn(P, *const c_char, i32);
        type Void = unsafe extern "C" fn();
        // SAFETY: signatures de reaper_plugin_functions.h :
        // int CountSelectedMediaItems(ReaProject*) ; MediaItem* GetSelectedMediaItem(ReaProject*, int) ;
        // MediaItem_Take* GetActiveTake(MediaItem*) ; PCM_source* GetMediaItemTake_Source(MediaItem_Take*) ;
        // void GetMediaSourceFileName(PCM_source*, char*, int) ;
        // double GetMediaItemTakeInfo_Value(MediaItem_Take*, const char*) ;
        // bool Set...Info_Value(ptr, const char*, double) ; Undo_BeginBlock2 / Undo_EndBlock2 ; UpdateArrange.
        let fns = unsafe {
            (
                self.api::<Count>(c"CountSelectedMediaItems"),
                self.api::<SelItem>(c"GetSelectedMediaItem"),
                self.api::<ActTake>(c"GetActiveTake"),
                self.api::<TakeSrc>(c"GetMediaItemTake_Source"),
                self.api::<SrcName>(c"GetMediaSourceFileName"),
                self.api::<GetVal>(c"GetMediaItemTakeInfo_Value"),
                self.api::<SetVal>(c"SetMediaItemTakeInfo_Value"),
                self.api::<SetVal>(c"SetMediaItemInfo_Value"),
                self.api::<UndoBegin>(c"Undo_BeginBlock2"),
                self.api::<UndoEnd>(c"Undo_EndBlock2"),
                self.api::<Void>(c"UpdateArrange"),
            )
        };
        let (
            Some(count),
            Some(sel),
            Some(act),
            Some(tsrc),
            Some(name),
            Some(gt),
            Some(st),
            Some(si),
        ) = (fns.0, fns.1, fns.2, fns.3, fns.4, fns.5, fns.6, fns.7)
        else {
            return 0;
        };
        let want = path.to_lowercase().replace('/', "\\");
        let mut done = 0;
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls ; tampon de
        // 4096 octets annonce ; fil principal.
        unsafe {
            let n = count(null_mut());
            let mut hits = Vec::new();
            for i in 0..n {
                let item = sel(null_mut(), i);
                if item.is_null() {
                    continue;
                }
                let take = act(item);
                if take.is_null() {
                    continue;
                }
                let src = tsrc(take);
                if src.is_null() {
                    continue;
                }
                let mut buf = vec![0u8; 4096];
                name(src, buf.as_mut_ptr() as *mut c_char, 4096);
                let f = CStr::from_ptr(buf.as_ptr() as *const c_char)
                    .to_string_lossy()
                    .to_lowercase();
                if f.replace('/', "\\") == want && gt(take, c"D_STARTOFFS".as_ptr()).abs() < 1e-9 {
                    hits.push((item, take));
                }
            }
            if hits.is_empty() {
                return 0;
            }
            if let Some(f) = fns.8 {
                f(null_mut());
            }
            for (item, take) in hits {
                st(take, c"D_STARTOFFS".as_ptr(), a);
                si(item, c"D_LENGTH".as_ptr(), (b - a).max(0.0));
                done += 1;
            }
            if let Some(f) = fns.10 {
                f();
            }
            if let Some(f) = fns.9 {
                f(null_mut(), c"CP Studio: trim dropped section".as_ptr(), -1);
            }
        }
        done
    }
}

impl Reaper {
    /// Le tempo du projet (le premier marqueur de tempo).
    pub fn master_tempo(&self) -> f64 {
        type F = unsafe extern "C" fn() -> f64;
        // SAFETY: double Master_GetTempo().
        match unsafe { self.api::<F>(c"Master_GetTempo") } {
            // SAFETY: appel sans argument, fil principal.
            Some(f) => unsafe { f() },
            None => 0.0,
        }
    }

    /// Change la vitesse de lecture du premier item choisi (sa longueur suit).
    pub fn set_selected_item_rate(&self, rate: f64) {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        if rate.is_nan() || rate <= 0.0 || (rate - 1.0).abs() < 1e-12 {
            return;
        }
        // SAFETY: GetSelectedMediaItem, GetActiveTake, Get/SetMediaItemInfo_Value,
        // SetMediaItemTakeInfo_Value (signatures du SDK).
        let fns = unsafe {
            (
                self.api::<SelItem>(c"GetSelectedMediaItem"),
                self.api::<ActTake>(c"GetActiveTake"),
                self.api::<GetVal>(c"GetMediaItemInfo_Value"),
                self.api::<SetVal>(c"SetMediaItemInfo_Value"),
                self.api::<SetVal>(c"SetMediaItemTakeInfo_Value"),
            )
        };
        let (Some(si), Some(at), Some(gi), Some(sv), Some(st)) = fns else {
            return;
        };
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls.
        unsafe {
            let item = si(null_mut(), 0);
            if item.is_null() {
                return;
            }
            let take = at(item);
            if take.is_null() {
                return;
            }
            let len = gi(item, c"D_LENGTH".as_ptr());
            st(take, c"D_PLAYRATE".as_ptr(), rate);
            sv(item, c"D_LENGTH".as_ptr(), len / rate);
        }
    }

    /// Le fichier et le decalage de la prise active du premier item choisi.
    pub fn selected_take_file(&self) -> Option<(String, f64)> {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type TakeSrc = unsafe extern "C" fn(P) -> P;
        type SrcName = unsafe extern "C" fn(P, *mut c_char, i32);
        type GetVal = unsafe extern "C" fn(P, *const c_char) -> f64;
        // SAFETY: signatures du SDK (voir trim_dropped).
        let (si, at, ts, sn, gt) = unsafe {
            (
                self.api::<SelItem>(c"GetSelectedMediaItem")?,
                self.api::<ActTake>(c"GetActiveTake")?,
                self.api::<TakeSrc>(c"GetMediaItemTake_Source")?,
                self.api::<SrcName>(c"GetMediaSourceFileName")?,
                self.api::<GetVal>(c"GetMediaItemTakeInfo_Value")?,
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
            let f = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned();
            if f.is_empty() {
                return None;
            }
            Some((f, gt(take, c"D_STARTOFFS".as_ptr())))
        }
    }

    /// Remplace le fichier de la prise active du premier item choisi (la
    /// position et la longueur de l'item restent). `offset` : le decalage dans
    /// le nouveau fichier. Par SWS s'il est la (il garde les proprietes de la
    /// source), sinon par l'API de REAPER (l'ancienne source n'est pas
    /// detruite : l'historique d'annulation peut la tenir).
    pub fn swap_take_source(&self, path: &str, offset: f64) -> Result<(), String> {
        type SelItem = unsafe extern "C" fn(P, i32) -> P;
        type ActTake = unsafe extern "C" fn(P) -> P;
        type BrSet = unsafe extern "C" fn(P, *const c_char, bool, bool) -> bool;
        type Create = unsafe extern "C" fn(*const c_char) -> P;
        type SetSrc = unsafe extern "C" fn(P, P) -> bool;
        type SetVal = unsafe extern "C" fn(P, *const c_char, f64) -> bool;
        type Void = unsafe extern "C" fn();
        type Cmd = unsafe extern "C" fn(i32, i32);
        let c = CString::new(path).map_err(|e| e.to_string())?;
        // SAFETY: signatures du SDK et de SWS :
        // bool BR_SetTakeSourceFromFile2(MediaItem_Take*, const char*, bool, bool) ;
        // PCM_source* PCM_Source_CreateFromFile(const char*) ;
        // bool SetMediaItemTake_Source(MediaItem_Take*, PCM_source*).
        let (si, at, br, create, setsrc, st, upd, cmd) = unsafe {
            (
                self.api::<SelItem>(c"GetSelectedMediaItem"),
                self.api::<ActTake>(c"GetActiveTake"),
                self.api::<BrSet>(c"BR_SetTakeSourceFromFile2"),
                self.api::<Create>(c"PCM_Source_CreateFromFile"),
                self.api::<SetSrc>(c"SetMediaItemTake_Source"),
                self.api::<SetVal>(c"SetMediaItemTakeInfo_Value"),
                self.api::<Void>(c"UpdateArrange"),
                self.api::<Cmd>(c"Main_OnCommand"),
            )
        };
        let (Some(si), Some(at)) = (si, at) else {
            return Err("REAPER API missing".into());
        };
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls ; fil principal.
        unsafe {
            let item = si(null_mut(), 0);
            if item.is_null() {
                return Err("select an item in the arrange first".into());
            }
            let take = at(item);
            if take.is_null() {
                return Err("the selected item has no take".into());
            }
            let ok = match (br, create, setsrc) {
                (Some(br), _, _) => br(take, c.as_ptr(), false, true),
                (None, Some(cr), Some(ss)) => {
                    let src = cr(c.as_ptr());
                    !src.is_null() && ss(take, src)
                }
                _ => false,
            };
            if !ok {
                return Err("REAPER refused the new source".into());
            }
            if let Some(st) = st {
                st(take, c"D_STARTOFFS".as_ptr(), offset.max(0.0));
            }
            if let Some(f) = upd {
                f();
            }
            // Item: Build any missing peaks.
            if let Some(f) = cmd {
                f(40047, 0);
            }
        }
        Ok(())
    }

    /// Les effets installes : (nom affiche, identifiant).
    pub fn installed_fx(&self) -> Vec<(String, String)> {
        type F = unsafe extern "C" fn(i32, *mut *const c_char, *mut *const c_char) -> bool;
        // SAFETY: bool EnumInstalledFX(int index, const char** nameOut, const char** identOut).
        let Some(f) = (unsafe { self.api::<F>(c"EnumInstalledFX") }) else {
            return Vec::new();
        };
        let mut v = Vec::new();
        for i in 0..100_000 {
            let mut n: *const c_char = std::ptr::null();
            let mut id: *const c_char = std::ptr::null();
            // SAFETY: deux pointeurs de chaine ecrits par REAPER, tenus par lui.
            if !unsafe { f(i, &mut n, &mut id) } {
                break;
            }
            if n.is_null() {
                continue;
            }
            // SAFETY: chaines terminees par zero tenues par REAPER.
            let name = unsafe { CStr::from_ptr(n) }.to_string_lossy().into_owned();
            let ident = if id.is_null() {
                String::new()
            } else {
                // SAFETY: idem.
                unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned()
            };
            v.push((name, ident));
        }
        v
    }

    /// Ajoute un effet a la piste choisie (ou la derniere touchee) et l'ouvre.
    pub fn add_fx(&self, name: &str) -> Result<(), String> {
        type SelTrack = unsafe extern "C" fn(P, i32) -> P;
        type LastTrack = unsafe extern "C" fn() -> P;
        type Add = unsafe extern "C" fn(P, *const c_char, bool, i32) -> i32;
        type Show = unsafe extern "C" fn(P, i32, i32);
        let c = CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: MediaTrack* GetSelectedTrack(ReaProject*, int) ;
        // MediaTrack* GetLastTouchedTrack() ;
        // int TrackFX_AddByName(MediaTrack*, const char*, bool, int) ;
        // void TrackFX_Show(MediaTrack*, int, int).
        let (sel, last, add, show) = unsafe {
            (
                self.api::<SelTrack>(c"GetSelectedTrack"),
                self.api::<LastTrack>(c"GetLastTouchedTrack"),
                self.api::<Add>(c"TrackFX_AddByName"),
                self.api::<Show>(c"TrackFX_Show"),
            )
        };
        let add = add.ok_or("REAPER API missing")?;
        // SAFETY: pointeurs rendus par REAPER, verifies non nuls ; fil principal.
        unsafe {
            let mut tr = sel.map_or(null_mut(), |f| f(null_mut(), 0));
            if tr.is_null() {
                tr = last.map_or(null_mut(), |f| f());
            }
            if tr.is_null() {
                return Err("select a track first".into());
            }
            let idx = add(tr, c.as_ptr(), false, -1);
            if idx < 0 {
                return Err("REAPER could not add this effect".into());
            }
            if let Some(s) = show {
                s(tr, idx, 3);
            }
        }
        Ok(())
    }
}
