//! CP Guitar Tab : de l'audio d'une guitare a une tablature que Cedric corrige
//! dans MuseScore.
//!
//! La chaine, de bout en bout, sans Windows ni REAPER (tout se teste ici) :
//! 1. [`resample`] : le son, mono, a 22 050 Hz (le seul taux que le modele
//!    connait) ;
//! 2. [`analyze`] : le reseau de Basic Pitch ([`model`]), fenetre par fenetre,
//!    sur plusieurs fils ;
//! 3. [`notes`] : les matrices deviennent des notes (debut, fin, hauteur,
//!    force de l'attaque, bends) ;
//! 4. [`fret`] : chaque note recoit sa corde et sa case, par un doigte calcule
//!    sur TOUTE la phrase (rester en position, limiter les deplacements) ;
//! 5. [`score`] : le rythme est cale sur le tempo du projet, et le tout
//!    s'ecrit en MusicXML (une portee standard et une portee de tablature).
//!
//! Le but n'est pas une partition parfaite : un premier jet juste en rythme et
//! en place, que Cedric relit au lieu de l'ecrire.
//!
//! Basic Pitch : Spotify, ICASSP 2022, licence Apache-2.0
//! (<https://github.com/spotify/basic-pitch>). Ses poids sont dans
//! `src/bp_weights.bin` (voir NOTICE).

pub mod analyze;
pub mod cache;
pub mod doc;
pub mod engrave;
pub mod fret;
pub mod model;
pub mod notes;
pub mod resample;
pub mod score;
pub mod tuner;

#[cfg(test)]
mod tests;

pub use analyze::{Posteriors, Progress, analyze};
pub use cache::Analysis;
pub use doc::{TNote, Tech};
pub use notes::{Note, NoteParams, make_notes};
