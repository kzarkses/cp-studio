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
pub const ABI: u32 = 3;

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

/// Une image de l'ecran : envoye a chaque vue qui l'a demande (`frames`),
/// juste apres une composition de l'ecran. `wp` = numero de l'image.
pub const WM_CP_FRAME: u32 = 0x8000 + 0x100;
/// Une touche vue par le crochet clavier, AVANT REAPER. `lp` pointe un
/// [`KeyEvent`]. Le module rend dans `out` : 1 = mangee (traitee), 0 = pas a
/// lui (REAPER la traite : ses raccourcis), -1 = a livrer normalement a la
/// fenetre (WM_KEYDOWN puis WM_CHAR : la saisie de texte).
pub const WM_CP_ACCEL: u32 = 0x8000 + 0x101;

/// La touche telle que Windows l'a mise en file.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyEvent {
    pub message: u32,
    pub wp: usize,
    pub lp: isize,
    /// Vrai si elle visait une AUTRE fenetre (REAPER avait repris le focus) et
    /// qu'elle nous revient parce que le dernier clic etait chez nous.
    pub redirected: u32,
}

/// Une vue a creer.
#[repr(C)]
pub struct ViewSpec {
    pub title: *const u8,
    pub title_len: usize,
    /// La cle sous laquelle REAPER retient le docker de la vue.
    pub ident: *const u8,
    pub ident_len: usize,
    /// Position et taille si flottante (`x < 0` : place par defaut).
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 1 = dans un docker.
    pub docked: i32,
}

/// La cadence de l'ecran.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    /// Images de l'ecran comptees depuis le demarrage de l'horloge.
    pub count: u64,
    /// L'instant de la derniere composition (QueryPerformanceCounter).
    pub vblank_qpc: i64,
    /// La periode mesuree d'une image, en unites de compteur.
    pub period_qpc: i64,
    pub qpc_freq: i64,
    /// Images ou une vue n'avait pas fini la precedente (elle en a saute une).
    pub missed: u64,
}

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
    /// Une vue flottante ou ancree (ABI 3). Rend le HWND (0 = echec).
    pub create_view: extern "C" fn(spec: *const ViewSpec) -> isize,
    /// Retire du docker PUIS detruit.
    pub destroy_window: extern "C" fn(hwnd: isize),
    /// Demande (1) ou non (0) un WM_CP_FRAME a chaque image de l'ecran.
    pub frames: extern "C" fn(hwnd: isize, on: i32),
    pub frame_info: extern "C" fn(out: *mut FrameInfo),
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
    /// L'etat d'une action a bascule (coche des menus) : 1, 0, ou -1 si
    /// l'action n'a pas d'etat (ABI 3).
    pub toggle_state: extern "C" fn(cmd: i32) -> i32,
}
