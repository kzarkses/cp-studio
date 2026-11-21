//! Charger et decharger le module rechargeable.
//!
//! Le chargeur ne charge jamais le fichier que le compilateur ecrit : il en
//! charge une COPIE (voir cp_hot). Windows ne verrouille que la copie, et la
//! construction suivante peut ecraser l'original.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use cp_abi::{ABI, ENTRY_SYMBOL, EntryFn, HostApi, ModuleApi};

use crate::win::{last_error, wide_os};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn FreeLibrary(h: *mut c_void) -> i32;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

/// Une DLL chargee. La liberer (`drop`) rend son code invalide : personne ne
/// doit plus tenir un de ses pointeurs de fonction.
struct Library(isize);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: handle rendu par LoadLibraryW, libere une seule fois.
        unsafe {
            FreeLibrary(self.0 as *mut c_void);
        }
    }
}

/// Un module charge et verifie : bon point d'entree, bon ABI.
pub struct LoadedModule {
    api: ModuleApi,
    path: PathBuf,
    // Declare en dernier : libere apres tout le reste.
    _lib: Library,
}

impl LoadedModule {
    pub fn load(path: &Path) -> Result<LoadedModule, String> {
        let w = wide_os(path.as_os_str());
        // SAFETY: chemin termine par zero. Le DllMain d'une DLL Rust ne fait
        // rien de dangereux.
        let h = unsafe { LoadLibraryW(w.as_ptr()) };
        if h.is_null() {
            return Err(format!(
                "LoadLibrary a echoue (erreur {}) : {}",
                last_error(),
                path.display()
            ));
        }
        let lib = Library(h as isize);
        // SAFETY: handle valide, nom termine par zero.
        let sym = unsafe { GetProcAddress(h, ENTRY_SYMBOL.as_ptr()) };
        if sym.is_null() {
            return Err("pas de point d'entree cp_module_entry".into());
        }
        // SAFETY: le symbole est defini par `export_module!`, avec cette
        // signature exacte.
        let entry: EntryFn = unsafe { std::mem::transmute::<*mut c_void, EntryFn>(sym) };
        let p = entry(ABI);
        if p.is_null() {
            return Err(format!(
                "module construit pour un autre ABI (chargeur : {ABI})"
            ));
        }
        // SAFETY: `p` pointe sur le static ModuleApi du module, vivant tant que
        // la DLL est chargee ; on en copie les champs.
        let api = unsafe { *p };
        if api.abi != ABI {
            return Err(format!("ABI {} au lieu de {ABI}", api.abi));
        }
        Ok(LoadedModule {
            api,
            path: path.to_path_buf(),
            _lib: lib,
        })
    }

    /// La table du module. Valide tant que `self` vit.
    pub fn api(&self) -> ModuleApi {
        self.api
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init(&self, host: &'static HostApi, state: &[u8]) -> i32 {
        (self.api.init)(host, state.as_ptr(), state.len())
    }

    pub fn shutdown(&self) -> i32 {
        (self.api.shutdown)()
    }
}
