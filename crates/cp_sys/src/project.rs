//! Le projet de REAPER : son dossier, et poser un fichier (ou une section) a
//! la place du curseur d'edition.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;

use crate::reaper::Reaper;

type P = *mut c_void;

impl Reaper {
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
