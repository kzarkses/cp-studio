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

#[link(name = "shell32")]
unsafe extern "system" {
    fn DragAcceptFiles(hwnd: Raw, accept: i32);
    fn DragQueryFileW(drop: Raw, i: u32, buf: *mut u16, n: u32) -> u32;
    fn DragQueryPoint(drop: Raw, pt: *mut [i32; 2]) -> i32;
    fn DragFinish(drop: Raw);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn PeekMessageW(msg: *mut [usize; 6], hwnd: Raw, min: u32, max: u32, remove: u32) -> i32;
}

/// `WM_DROPFILES`.
pub const WM_DROPFILES: u32 = 0x0233;

/// La fenetre accepte (ou plus) des fichiers deposes : ils arrivent en
/// `WM_DROPFILES` (Windows tient le geste ; aucun objet COM a nous).
pub fn accept_files(owner: Hwnd, on: bool) {
    // SAFETY: fenetre vivante (ou appel sans effet).
    unsafe { DragAcceptFiles(owner.0 as Raw, on as i32) };
}

/// Les fichiers d'un `WM_DROPFILES` (son `wParam`) et le point du depot
/// (pixels de la fenetre). Libere le depot.
pub fn take_drop(hdrop: usize) -> (Vec<String>, i32, i32) {
    let h = hdrop as Raw;
    let mut out = Vec::new();
    let mut pt = [0i32; 2];
    // SAFETY: `hdrop` vient du message ; tampons de la taille annoncee ;
    // DragFinish une seule fois.
    unsafe {
        let n = DragQueryFileW(h, 0xFFFF_FFFF, null_mut(), 0);
        for i in 0..n {
            let len = DragQueryFileW(h, i, null_mut(), 0) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
            out.push(String::from_utf16_lossy(&buf[..got.min(len)]));
        }
        DragQueryPoint(h, &mut pt);
        DragFinish(h);
    }
    (out, pt[0], pt[1])
}

/// Un message `msg` attend-il dans la file de la fenetre (sans le retirer) ?
pub fn pending(owner: Hwnd, msg: u32) -> bool {
    let mut m = [0usize; 6];
    // SAFETY: tampon de la taille d'un MSG (6 mots sur 64 bits, avec le
    // point et le remplissage) ; PM_NOREMOVE.
    unsafe { PeekMessageW(&mut m, owner.0 as Raw, msg, msg, 0) != 0 }
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
