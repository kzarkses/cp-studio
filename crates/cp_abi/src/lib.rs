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
pub const ABI: u32 = 11;

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
    /// [`VIEW_FLOAT`], [`VIEW_DOCK`] ou [`VIEW_PLACED`].
    pub docked: i32,
}

/// Une fenetre a elle, flottante.
pub const VIEW_FLOAT: i32 = 0;
/// Dans un docker de REAPER.
pub const VIEW_DOCK: i32 = 1;
/// Fille cachee de la fenetre principale, pour prendre une place native
/// ([`HostApi::place_take`]). ABI 6.
pub const VIEW_PLACED: i32 = 2;

/// Les places natives de REAPER qu'une de nos vues peut prendre (ABI 6) :
/// la vue native est MASQUEE (REAPER continue de calculer son rectangle, qui
/// sert d'oracle de disposition) et la notre est posee dans ce rectangle.
/// La bande de l'arrangement : regle, pistes, panneau des pistes.
pub const PLACE_ARRANGE: u32 = 1;
/// Le transport de REAPER, ancre en haut de la fenetre principale.
pub const PLACE_TRANSPORT: u32 = 2;

/// L'etat d'une place (`place_state`).
/// Personne ne la tient : la vue native est la.
pub const PLACE_FREE: i32 = 0;
/// Une de nos vues l'occupe.
pub const PLACE_HELD: i32 = 1;
/// Rendue a REAPER d'elle-meme : le transport natif a ete rallume (par
/// Cedric), ou la place a disparu. La vue qui l'occupait est cachee.
pub const PLACE_YIELDED: i32 = 2;
/// Introuvable (pas de transport en haut de la fenetre, pas d'arrangement).
pub const PLACE_MISSING: i32 = -1;

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
    /// Pose la vue `hwnd` (creee en [`VIEW_PLACED`]) a la place native
    /// `place`. `show` = 0 : seulement la mettre a la taille de la place, cachee
    /// (pour la dessiner avant de la montrer) ; 1 : masquer la vue native et
    /// montrer la notre. `hwnd` = 0 : rendre la place a REAPER (la vue qui
    /// l'occupait est cachee, pas detruite). 1 = fait, 0 = place introuvable.
    /// Le chargeur tient ensuite la place a chaque battement, et la rend de
    /// lui-meme si le module tombe ou si REAPER s'arrete. ABI 6.
    pub place_take: extern "C" fn(place: u32, hwnd: isize, show: i32) -> i32,
    /// L'etat d'une place (`PLACE_*`) et la vue qui l'occupe (0 = aucune).
    pub place_state: extern "C" fn(place: u32, hwnd: *mut isize) -> i32,
    /// Une fenetre qui ne prend pas le clavier : un clic n'y pose ni focus ni
    /// revendication, et REAPER garde ses raccourcis (un transport). ABI 6.
    pub set_keyless: extern "C" fn(hwnd: isize, on: i32),
    /// L'entree de la carte son (l'accordeur) : capture l'entree `ch0` (et
    /// `ch1` >= 0 : la moyenne des deux), et rend ses `n` derniers
    /// echantillons dans `out` (au taux de `audio.srate`). `ch0` < 0 arrete la
    /// capture. Rend combien d'echantillons sont ecrits. ABI 7.
    pub audio_input: extern "C" fn(ch0: i32, ch1: i32, out: *mut f32, n: usize) -> usize,
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
    /// Frames deja decodes (= `frames` quand le clip est complet). ABI 4.
    pub decoded: i64,
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
    /// La vignette d'un fichier (min et max de `n` tranches, canaux fondus),
    /// demandee au premier appel et calculee en basse priorite. Rend 1 (rendue
    /// dans `min`/`max`), 0 (en cours) ou -1 (impossible). ABI 5.
    pub thumb:
        extern "C" fn(path: *const u8, len: usize, min: *mut f32, max: *mut f32, n: usize) -> i32,
    /// La region `[start, end)` d'un clip (`end` = 0 : jusqu'au bout),
    /// etiree par l'etireur de REAPER : `tempo` (1,5 = une fois et demie plus
    /// vite), `shift` (rapport de hauteur). Cuite sur un fil de travail, a la
    /// duree exacte ; rend un ticket (comme `load`, lu par `poll`), 0 si
    /// refuse. ABI 8.
    pub stretch: extern "C" fn(clip: u32, start: i64, end: i64, tempo: f64, shift: f64) -> u32,
    /// Fait jouer un port DANS une piste (GUID UTF-8 ; `len` = 0 : la sortie
    /// materielle 1/2). 1 si fait. ABI 9.
    pub route: extern "C" fn(port: u32, guid: *const u8, len: usize) -> i32,
    /// Les cretes d'un port (gauche, droite) depuis la derniere lecture,
    /// ecrites dans `out[0..2]` ; 1 si le port existe. ABI 9.
    pub levels: extern "C" fn(port: u32, out: *mut f32) -> i32,
    /// La clock MIDI externe : l'entree MIDI ecoutee (son rang dans les
    /// entrees de REAPER ; -1 : aucune). ABI 10.
    pub clock_device: extern "C" fn(dev: i32),
    /// L'etat de la clock externe dans `out[0..14]` : bpm, son ecart-type,
    /// le bpm de la derniere noire, la gigue (ms), la fenetre (noires),
    /// l'instant ajuste de la derniere impulsion, sa position, la position
    /// suivante, les echantillons par impulsion, les drapeaux, les Start,
    /// les Stop, les impulsions, puis la position du maitre MAINTENANT en
    /// noires. 1 si publie. ABI 10.
    pub clock_read: extern "C" fn(out: *mut f64) -> i32,
    /// Ecouter (1) ou plus (0) les notes des entrees MIDI. ABI 11.
    pub midi_listen: extern "C" fn(on: i32),
    /// Les notes arrivees depuis le dernier appel, au plus `cap` : (statut,
    /// note, velocite, entree) en octets, du bas vers le haut. Rend leur
    /// nombre. ABI 11.
    pub midi_notes: extern "C" fn(out: *mut u32, cap: usize) -> usize,
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
