//! Le glisser-deposer de fichiers, comme depuis l'Explorateur.
//!
//! L'objet de donnees est fabrique par le shell de Windows a partir des
//! chemins (`SHCreateDataObject`), et le geste est mene par le shell
//! (`SHDoDragDrop`, source de glisser par defaut, image du fichier sous la
//! souris). Aucun objet COM n'est ecrit ici : Windows ne garde aucun pointeur
//! vers notre code, et le module peut etre recharge ensuite sans risque.

use std::ffi::c_void;
use std::ptr::null_mut;

use crate::win::{Hwnd, wide};

type Raw = *mut c_void;

#[repr(C)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

/// IID_IDataObject {0000010e-0000-0000-C000-000000000046}.
const IID_IDATAOBJECT: Guid = Guid {
    d1: 0x0000_010E,
    d2: 0,
    d3: 0,
    d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHParseDisplayName(
        name: *const u16,
        bc: Raw,
        pidl: *mut Raw,
        sfgao_in: u32,
        sfgao_out: *mut u32,
    ) -> i32;
    fn SHCreateDataObject(
        folder: Raw,
        cidl: u32,
        apidl: *const Raw,
        inner: Raw,
        riid: *const Guid,
        out: *mut Raw,
    ) -> i32;
    fn SHDoDragDrop(hwnd: Raw, data: Raw, src: Raw, ok_effects: u32, effect: *mut u32) -> i32;
    fn ILClone(pidl: Raw) -> Raw;
    fn ILRemoveLastID(pidl: Raw) -> i32;
    fn ILFindLastID(pidl: Raw) -> Raw;
    fn ILFree(pidl: Raw);
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn OleInitialize(reserved: Raw) -> i32;
}

const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_LINK: u32 = 4;
const DRAGDROP_S_DROP: i32 = 0x0004_0100;

/// Ce qu'a donne le geste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragResult {
    Dropped,
    Cancelled,
    Failed,
}

/// Glisse un fichier hors de la fenetre. BLOQUANT : rend la main quand le
/// bouton est lache (Windows fait tourner sa propre boucle de messages).
pub fn drag_file(owner: Hwnd, path: &str) -> DragResult {
    let w = wide(path);
    // SAFETY: appels du shell de Windows avec des arguments valides ; chaque
    // PIDL alloue est libere, l'objet de donnees est relache (Release = case
    // 2 de la table d'IUnknown).
    unsafe {
        // Rend S_FALSE si OLE est deja pret sur ce fil : sans effet.
        OleInitialize(null_mut());
        let mut pidl: Raw = null_mut();
        if SHParseDisplayName(w.as_ptr(), null_mut(), &mut pidl, 0, null_mut()) < 0
            || pidl.is_null()
        {
            return DragResult::Failed;
        }
        let folder = ILClone(pidl);
        if folder.is_null() {
            ILFree(pidl);
            return DragResult::Failed;
        }
        ILRemoveLastID(folder);
        let child = ILFindLastID(pidl);
        let mut data: Raw = null_mut();
        let hr = SHCreateDataObject(folder, 1, &child, null_mut(), &IID_IDATAOBJECT, &mut data);
        let res = if hr >= 0 && !data.is_null() {
            let mut effect = 0u32;
            let r = SHDoDragDrop(
                owner.0 as Raw,
                data,
                null_mut(),
                DROPEFFECT_COPY | DROPEFFECT_LINK,
                &mut effect,
            );
            let vt = *(data as *const *const usize);
            let release =
                std::mem::transmute::<usize, unsafe extern "system" fn(Raw) -> u32>(*vt.add(2));
            release(data);
            // REAPER (et d'autres) rendent parfois un effet nul sur un vrai
            // depot : c'est le code de retour qui fait foi.
            if r == DRAGDROP_S_DROP {
                DragResult::Dropped
            } else {
                DragResult::Cancelled
            }
        } else {
            DragResult::Failed
        };
        ILFree(folder);
        ILFree(pidl);
        res
    }
}
