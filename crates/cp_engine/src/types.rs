//! Les bornes et les types du moteur (ancien `cp_types.h`).

/// Le moteur ne connait que l'echantillon absolu : jamais le beat, jamais la
/// seconde. La carte de tempo reste sur le fil principal, ou elle est exacte.
pub type Frame = i64;

/// Une colonne = un port.
pub const MAX_PORTS: usize = 32;
/// Voix par port. Borne aussi ce que le fil audio parcourt a chaque bloc.
pub const MAX_PORT_VOICES: usize = 64;
/// Clips resident en RAM.
pub const MAX_CLIPS: usize = 512;
/// Mono ou stereo.
pub const MAX_CHANS: usize = 2;
/// Commandes en attente par port. 256 drainees par bloc a 64 echantillons =
/// 192 000 commandes/s : trois ordres de grandeur au-dessus d'une interface.
pub const CMD_RING_CAP: usize = 256;
/// Points d'une enveloppe dessinee (le meme nombre que l'editeur).
pub const MAX_ENV_PTS: usize = 64;
/// 64 s a 96 kHz : des boucles de 16 mesures, pas des stems (decide le
/// 2026-07-31).
pub const MAX_CLIP_FRAMES: Frame = 64 * 96000;

/// Etats d'une voix (les valeurs de l'ancien `VoiceState`).
pub mod state {
    pub const IDLE: i32 = 0;
    /// A un rendez-vous dans le futur.
    pub const SCHEDULED: i32 = 1;
    pub const PLAYING: i32 = 2;
    /// En fondu de sortie.
    pub const STOPPING: i32 = 3;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    Once = 0,
    Loop = 1,
}

/// Quand une commande prend effet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// Au debut du bloc ou la commande est appliquee.
    Now,
    /// A cet echantillon absolu.
    Frame(Frame),
}

/// Une voix, telle que le fil principal la designe. 16 bits d'index, 16 bits de
/// generation : un handle perime est detecte, jamais reutilise en silence.
/// L'index = port * 64 + emplacement dans le port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceH(pub u32);

impl VoiceH {
    pub(crate) fn new(port: usize, local: usize, generation: u16) -> VoiceH {
        let index = (port * MAX_PORT_VOICES + local) as u32;
        VoiceH((generation as u32) << 16 | (index & 0xFFFF))
    }
    pub fn port(self) -> usize {
        (self.0 & 0xFFFF) as usize / MAX_PORT_VOICES
    }
    pub(crate) fn local(self) -> usize {
        (self.0 & 0xFFFF) as usize % MAX_PORT_VOICES
    }
    pub fn generation(self) -> u16 {
        (self.0 >> 16) as u16
    }
}

/// Le mot de propriete d'un emplacement de voix : bits 0..15 = generation
/// (jamais 0), bit 16 = possede. Ecrit par le fil principal ; le fil audio n'y
/// EFFACE que le bit possede, quand il a fini d'eteindre la voix.
pub(crate) const CLAIM_OWNED: u32 = 1 << 16;

pub(crate) fn claim_gen(c: u32) -> u16 {
    (c & 0xFFFF) as u16
}

pub(crate) fn claim_owned(c: u32) -> bool {
    c & CLAIM_OWNED != 0
}

pub(crate) fn next_gen(g: u16) -> u16 {
    match g.wrapping_add(1) {
        0 => 1,
        n => n,
    }
}

/// Les reglages d'une voix (ancien `ParamId`, memes numeros).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Param {
    Rate = 0,
    Gain,
    Pan,
    /// En frames source.
    LoopStart,
    LoopEnd,
    /// En secondes.
    FadeIn,
    FadeOut,
    /// Deplacement de la tete, en frames source.
    Pos,
    /// 0 = une fois, sinon boucle. Change a la volee.
    Loop,
    /// 0 = aucune enveloppe, 1 = ADSR.
    Env,
    Attack,
    Decay,
    /// Niveau 0..1.
    Sustain,
    Release,
    /// -1..1, 0 = droite.
    ACurve,
    DCurve,
    RCurve,
    /// 0 = relacher maintenant.
    Gate,
    /// Valide les points recus : la valeur = leur nombre, 0 l'eteint.
    EnvPts,
    /// L'interpolation hors vitesse 1 : 0 = Hermite (le defaut, celui de
    /// l'ancien moteur), 1 = sinc (haute qualite, voir [`crate::sinc`]).
    Interp,
    /// 1 = a l'envers : la tete recule. Pose avant le depart, elle part de
    /// la fin de la region.
    Reverse,
}
