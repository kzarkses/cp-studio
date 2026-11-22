//! Les dossiers : la boite « choisir un dossier » de Windows
//! (`IFileOpenDialog` en mode dossier, celle de l'Explorateur), et « montrer
//! dans l'Explorateur ».
//!
//! La boite est MODALE : elle fait tourner sa propre boucle de messages.
//! Le module est occupe pendant ce temps ; le chargeur reporte au battement
//! suivant les actions qui arrivent (voir `PENDING` dans cp_loader).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use crate::win::{Hwnd, wide, wide_os};

type Raw = *mut c_void;

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

/// CLSID_FileOpenDialog {DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}.
const CLSID_FILE_OPEN: Guid = Guid(
    0xDC1C_5A9C,
    0xE88A,
    0x4DDE,
    [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
);
/// IID_IFileOpenDialog {d57c7288-d4ad-4768-be02-9d969532d960}.
const IID_FILE_OPEN: Guid = Guid(
    0xD57C_7288,
    0xD4AD,
    0x4768,
    [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
);
/// IID_IShellItem {43826d1e-e718-42ee-bc55-a1e261c37bfe}.
const IID_SHELL_ITEM: Guid = Guid(
    0x4382_6D1E,
    0xE718,
    0x42EE,
    [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
);

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: Raw, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: Raw,
        ctx: u32,
        iid: *const Guid,
        out: *mut Raw,
    ) -> i32;
    fn CoTaskMemFree(p: Raw);
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHCreateItemFromParsingName(
        path: *const u16,
        bc: Raw,
        iid: *const Guid,
        out: *mut Raw,
    ) -> i32;
    fn ShellExecuteW(
        hwnd: Raw,
        op: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> Raw;
}

/// La methode `i` de la table virtuelle d'un objet COM.
///
/// # Safety
/// `obj` est un objet COM vivant dont la table a au moins `i + 1` entrees.
unsafe fn method(obj: Raw, i: usize) -> *const c_void {
    // SAFETY: un objet COM commence par un pointeur vers sa table.
    unsafe { *(*(obj as *const *const *const c_void)).add(i) }
}

/// Un objet COM rendu a sa destruction.
struct Com(Raw);

impl Drop for Com {
    fn drop(&mut self) {
        if !self.0.is_null() {
            type Release = unsafe extern "system" fn(Raw) -> u32;
            // SAFETY: IUnknown::Release est l'entree 2 de toute table COM.
            unsafe {
                let f: Release = std::mem::transmute::<*const c_void, Release>(method(self.0, 2));
                f(self.0);
            }
        }
    }
}

/// La boite « choisir un dossier » de Windows, ouverte sur `start`. `None`
/// si Cedric annule.
pub fn pick_folder(owner: Hwnd, start: Option<&Path>, title: &str) -> Option<PathBuf> {
    const COINIT_APARTMENTTHREADED: u32 = 2;
    // SAFETY: COM s'initialise par fil ; deja fait (S_FALSE) ou autre modele
    // (RPC_E_CHANGED_MODE) laissent COM utilisable.
    let hr = unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) };
    let out = pick(owner, start, title);
    if hr >= 0 {
        // SAFETY: rend l'initialisation reussie juste au-dessus.
        unsafe { CoUninitialize() };
    }
    out
}

fn pick(owner: Hwnd, start: Option<&Path>, title: &str) -> Option<PathBuf> {
    const CLSCTX_INPROC_SERVER: u32 = 1;
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    let mut d: Raw = null_mut();
    // SAFETY: GUID statiques, sortie ecrite par COM.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_FILE_OPEN,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_FILE_OPEN,
            &mut d,
        )
    };
    if hr < 0 || d.is_null() {
        return None;
    }
    let dlg = Com(d);
    // Les entrees de IFileDialog : Show 3, SetOptions 9, GetOptions 10,
    // SetFolder 12, SetTitle 17, GetResult 20.
    type GetOptions = unsafe extern "system" fn(Raw, *mut u32) -> i32;
    type SetOptions = unsafe extern "system" fn(Raw, u32) -> i32;
    type SetFolder = unsafe extern "system" fn(Raw, Raw) -> i32;
    type SetTitle = unsafe extern "system" fn(Raw, *const u16) -> i32;
    type Show = unsafe extern "system" fn(Raw, Raw) -> i32;
    type GetResult = unsafe extern "system" fn(Raw, *mut Raw) -> i32;
    type GetName = unsafe extern "system" fn(Raw, u32, *mut *mut u16) -> i32;
    let t = wide(title);
    // SAFETY: les entrees de la table de IFileOpenDialog, appelees avec des
    // arguments valides ; chaque objet rendu est relache (Com).
    unsafe {
        let mut opts = 0u32;
        let get: GetOptions = std::mem::transmute(method(dlg.0, 10));
        get(dlg.0, &mut opts);
        let set: SetOptions = std::mem::transmute(method(dlg.0, 9));
        set(dlg.0, opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
        let st: SetTitle = std::mem::transmute(method(dlg.0, 17));
        st(dlg.0, t.as_ptr());
        if let Some(s) = start.filter(|s| s.is_dir()) {
            let w = wide_os(s.as_os_str());
            let mut item: Raw = null_mut();
            if SHCreateItemFromParsingName(w.as_ptr(), null_mut(), &IID_SHELL_ITEM, &mut item) >= 0
                && !item.is_null()
            {
                let item = Com(item);
                let sf: SetFolder = std::mem::transmute(method(dlg.0, 12));
                sf(dlg.0, item.0);
            }
        }
        let show: Show = std::mem::transmute(method(dlg.0, 3));
        if show(dlg.0, owner.raw()) < 0 {
            return None; // annule (ou refuse)
        }
        let mut r: Raw = null_mut();
        let res: GetResult = std::mem::transmute(method(dlg.0, 20));
        if res(dlg.0, &mut r) < 0 || r.is_null() {
            return None;
        }
        let item = Com(r);
        // IShellItem::GetDisplayName (5), SIGDN_FILESYSPATH.
        const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
        let mut name: *mut u16 = null_mut();
        let gn: GetName = std::mem::transmute(method(item.0, 5));
        if gn(item.0, SIGDN_FILESYSPATH, &mut name) < 0 || name.is_null() {
            return None;
        }
        let mut n = 0;
        while *name.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, n));
        CoTaskMemFree(name as Raw);
        Some(PathBuf::from(s))
    }
}

/// Ouvre l'Explorateur sur `p`, la ligne choisie (fichier ou dossier).
pub fn show_in_explorer(p: &Path) -> bool {
    let params = wide(&format!("/select,\"{}\"", p.display()));
    let file = wide("explorer.exe");
    let op = wide("open");
    let dir = p
        .parent()
        .map(|d| wide_os(d.as_os_str()))
        .unwrap_or_else(|| vec![0]);
    const SW_SHOWNORMAL: i32 = 1;
    // SAFETY: chaines terminees par zero, vivantes pendant l'appel.
    let r = unsafe {
        ShellExecuteW(
            null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            dir.as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    r as isize > 32
}
