//! Le moteur audio de CP Studio.
//!
//! Porte de l'ancien coeur C++ (`reference/cp_native/src/core`, instantane du
//! 2026-10-25), et compare a lui echantillon par echantillon par `cp_oracle`.
//!
//! LA REGLE DE CE CRATE, heritee mot pour mot de l'ancien `src/core` : aucun appel
//! a REAPER, aucune entree-sortie, et RIEN dans le fil audio qui alloue, libere
//! ou attende. C'est ce qui permet de le verifier hors de REAPER.
//!
//! Qui possede quoi :
//! - le FIL PRINCIPAL tient un [`Ctl`] : il reserve des voix, poste des
//!   commandes, lit l'etat publie. Il ne touche jamais une voix.
//! - le FIL AUDIO appelle [`Core::render_port`] et [`Core::tick`]. Chaque port
//!   possede SES voix ; deux ports peuvent etre rendus en meme temps sur deux
//!   fils, ils ne partagent rien.
//! - un clip voyage DANS la commande qui le lance (`Arc`) : une voix ne peut pas
//!   perdre sa matiere en plein son, et le vivier ne libere un clip que lorsque
//!   plus personne ne le tient. La memoire n'est donc jamais rendue dans le fil
//!   audio (une voix qui lache son clip n'est jamais la derniere a le tenir).
//!
//! Pas encore porte : les lanes MIDI (`cp_lanes`) et la clock externe
//! (`cp_extclock`).

pub mod curve;
mod engine;
pub mod pool;
pub mod types;
mod voice;

pub use engine::{Core, Ctl, VoiceOp};
pub use pool::{ClipData, Pool, PoolError};
pub use types::{At, Frame, Param, PlayMode, VoiceH};
