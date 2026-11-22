//! La seule porte vers REAPER et Windows.
//!
//! Tout le code `unsafe` du projet vit dans ce crate. Les autres crates le
//! refusent a la compilation (`unsafe_code = "deny"` dans le Cargo.toml racine) :
//! ils ne voient que des fonctions sures.
//!
//! - [`reaper`] : le point d'entree de l'extension, `Register`, `GetFunc`.
//! - [`win`] : fenetres, peinture GDI, horloge locale.
//! - [`dylib`] : charger / decharger le module rechargeable.
//! - [`host`] : cote chargeur, la table offerte au module.
//! - [`module`] : cote module, la table offerte au chargeur et la macro
//!   [`export_module!`].
//! - [`rt`] : les primitives temps reel (l'anneau sans verrou du fil audio).
//! - [`pcm`] : `PCM_source` dans les deux sens (notre source tiree par REAPER,
//!   une source de REAPER lue par nous).
//! - [`audio`] : le crochet de bloc, l'apercu permanent, l'ouverture de fichiers.
//! - [`ui`] : afficher une image, souris, clavier, curseurs, menus,
//!   presse-papiers, l'heure exacte et la cadence de l'ecran.
//! - [`font`] : les lettres par GDI, pour le cache de `cp_gfx`.
//! - [`rui`] : REAPER cote interface (dockers, crochet clavier, actions a
//!   bascule, ExtState, crochet souris).

pub use cp_abi as abi;

pub mod audio;
pub mod dark;
pub mod dylib;
pub mod folders;
pub mod font;
pub mod grow;
pub mod guard;
pub mod host;
pub mod image;
pub mod items;
pub mod module;
pub mod pcm;
pub mod project;
pub mod reaper;
pub mod rt;
pub mod rui;
pub mod shell;
pub mod tab;
pub mod theme;
pub mod tracks;
pub mod transport;
pub mod ui;
pub mod win;

pub use guard::guard;
