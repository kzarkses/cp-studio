//! Le contrat binaire entre le chargeur (`reaper_cp_studio.dll`, charge une fois
//! par REAPER au demarrage) et le module (`cp_module.dll`, recharge a chaud).
//!
//! C'est la SEULE frontiere binaire du projet. Elle ne porte que des pointeurs de
//! fonction et des octets empruntes : aucune memoire allouee d'un cote n'est
//! liberee de l'autre (chaque DLL Rust embarque sa propre copie de std).
//!
//! Changer une de ces structures = monter `ABI`, recompiler les deux cotes, et
//! redemarrer REAPER pour installer le nouveau chargeur. Le chargeur refuse un
//! module d'un autre ABI et garde l'ancien : rien ne casse en silence.

use core::ffi::{c_char, c_void};

/// Version du contrat. Un entier : jamais de mineure (le piege du double de
/// l'ancien moteur, ou `1.10` valait `1.1`).
pub const ABI: u32 = 1;

/// Le symbole exporte par le module.
pub const ENTRY_SYMBOL: &[u8] = b"cp_module_entry\0";

/// Codes de retour des appels vers le module.
pub const OK: i32 = 0;
/// Le module a panique (la panique a ete rattrapee de son cote) : le chargeur
/// cesse de l'appeler jusqu'a la prochaine construction.
pub const FAULT: i32 = -1;
/// Le module etait deja occupe (appel reentrant) : l'appel n'a pas eu lieu.
pub const BUSY: i32 = -2;

/// Niveaux de journal.
pub const LOG_INFO: u32 = 0;
pub const LOG_WARN: u32 = 1;
pub const LOG_ERROR: u32 = 2;

/// `cp_module_entry(abi)` rend la table du module, ou nul si l'ABI differe.
pub type EntryFn = extern "C" fn(abi: u32) -> *const ModuleApi;

/// Ce que le chargeur offre au module. Vit aussi longtemps que REAPER.
#[repr(C)]
pub struct HostApi {
    pub abi: u32,
    pub log: extern "C" fn(level: u32, msg: *const u8, len: usize),
    /// `GetFunc` de REAPER, tel quel (nul si la fonction n'existe pas).
    pub get_func: extern "C" fn(name: *const c_char) -> *mut c_void,
    /// Enregistre une action (idempotent par identifiant : le meme numero de
    /// commande revient apres un rechargement).
    pub register_action:
        extern "C" fn(id: *const u8, id_len: usize, name: *const u8, name_len: usize) -> i32,
    /// Cree une fenetre APPARTENANT AU CHARGEUR : elle survit au rechargement,
    /// et ses messages sont transmis au module courant.
    pub create_window:
        extern "C" fn(kind: u32, title: *const u8, len: usize, w: i32, h: i32) -> isize,
    /// Le module y depose son etat pendant `shutdown` ; le suivant le recoit a
    /// `init`.
    pub store_state: extern "C" fn(data: *const u8, len: usize),
    /// Numero de chargement du module courant (1, 2, 3...).
    pub generation: extern "C" fn() -> u32,
}

/// Ce que le module offre au chargeur. Tous les appels se font sur le fil
/// principal de REAPER.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleApi {
    pub abi: u32,
    pub init: extern "C" fn(host: *const HostApi, state: *const u8, len: usize) -> i32,
    /// Le module range son etat (`store_state`) et lache tout ce qu'il tient.
    pub shutdown: extern "C" fn() -> i32,
    /// Battement du fil principal (~30 Hz).
    pub tick: extern "C" fn() -> i32,
    /// 1 = action traitee, 0 = pas a lui.
    pub on_action: extern "C" fn(cmd: i32) -> i32,
    /// 1 = message traite (resultat dans `out`), 0 = traitement par defaut.
    pub wnd_proc:
        extern "C" fn(hwnd: isize, msg: u32, wp: usize, lp: isize, out: *mut isize) -> i32,
}
