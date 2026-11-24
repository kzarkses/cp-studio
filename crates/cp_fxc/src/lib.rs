//! FX Constellation, le modele : ce que la fenetre montre et ce que REAPER
//! doit recevoir, sans REAPER.
//!
//! - [`slots`] : les sources de modulation (l'espace d'adresses de l'ancienne
//!   version) et la carte des curseurs des JSFX internes ;
//! - [`model`] : une piste (ses parametres suivis, son pad, ses macros) ;
//! - [`links`] : les liens natifs voulus pour une piste (le plan) ;
//! - [`adopt`] : le modele refait depuis les liens d'un projet ;
//! - [`store`] : l'etat d'une piste en texte ;
//! - [`roll`] : ROLL, le randomiseur ;
//! - [`pad`] : le mode granulaire, la marche et les figures (pour
//!   l'affichage) ;
//! - [`preset`] : les presets et snapshots.

pub mod adopt;
pub mod links;
pub mod model;
pub mod pad;
pub mod preset;
pub mod roll;
pub mod slots;
pub mod store;

#[cfg(test)]
mod tests;
