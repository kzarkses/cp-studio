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
pub const ABI: u32 = 2;

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
    /// Le moteur audio (ABI 2). Il vit dans le chargeur : un rechargement du
    /// module ne coupe jamais le son.
    pub audio: AudioApi,
}

/// Etats d'un chargement de fichier.
pub const LOAD_PENDING: i32 = 0;
pub const LOAD_READY: i32 = 1;
pub const LOAD_FAILED: i32 = -1;

/// Ce qu'on sait d'un clip.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipInfo {
    pub frames: i64,
    pub nch: u32,
    pub srate: f64,
}

/// Le moteur audio vu du module. Les clips et les voix sont des entiers a
/// generation : 0 n'est jamais valide, et un handle perime est refuse.
#[repr(C)]
pub struct AudioApi {
    /// Ouvre et decode un fichier (chemin UTF-8, sans zero final), sur un fil
    /// de travail. Rend un ticket (> 0).
    pub load: extern "C" fn(path: *const u8, len: usize) -> u32,
    /// L'etat d'un ticket : LOAD_PENDING, LOAD_READY (`*clip` recoit le clip),
    /// ou LOAD_FAILED (le message, UTF-8, tronque a `cap` octets, et sa longueur
    /// dans `*err_len`). Un ticket fini est oublie apres avoir ete lu.
    pub poll: extern "C" fn(
        ticket: u32,
        clip: *mut u32,
        err: *mut u8,
        cap: usize,
        err_len: *mut usize,
    ) -> i32,
    /// 1 et `*out` rempli si le clip vit, 0 sinon.
    pub info: extern "C" fn(clip: u32, out: *mut ClipInfo) -> i32,
    /// Plus aucun lancement ; la matiere part quand plus rien ne la joue.
    pub release: extern "C" fn(clip: u32),
    /// Min et max par pixel (`n` pixels a partir de la frame `start`, `fpp`
    /// frames par pixel). `ch` < 0 fond les canaux. Rend le nombre de pixels
    /// qui tombent dans le clip.
    pub peaks: extern "C" fn(
        clip: u32,
        ch: i32,
        start: f64,
        fpp: f64,
        n: usize,
        min: *mut f32,
        max: *mut f32,
    ) -> usize,
    /// Joue `[start, end)` (frames du clip ; `end` = 0 : jusqu'au bout) sur un
    /// port. Rend la voix, 0 si refuse.
    pub play:
        extern "C" fn(port: u32, clip: u32, start: i64, end: i64, looped: i32, gain: f64) -> u32,
    /// Arrete et rend une voix, avec un fondu en secondes.
    pub stop: extern "C" fn(voice: u32, fade_s: f64),
    /// L'etat d'une voix vivante (et sa position en frames du clip), -1 si elle
    /// n'existe plus.
    pub voice: extern "C" fn(voice: u32, pos: *mut f64) -> i32,
    /// Un reglage de voix (numeros de `cp_engine::Param`). 1 si poste.
    pub set: extern "C" fn(voice: u32, param: u32, value: f64) -> i32,
    /// Le taux du peripherique audio.
    pub srate: extern "C" fn() -> f64,
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
