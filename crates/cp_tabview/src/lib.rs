//! La fenetre de CP Guitar Tab, sans Windows ni REAPER : un ARRANGEUR de
//! guitares. Des pistes hautes, chacune avec ses couches alignees sur les
//! memes mesures : l'audio enregistre, le MIDI transcrit, la partition et la
//! tablature. Chaque item garde sa propre tablature (une liste de notes, deux
//! dessins), sa detection reglable en direct et ses notes FANTOMES.
//!
//! Le module tient le projet (les pistes REAPER, les analyses, les items
//! MIDI), remplit [`Arr::tracks`] et execute les [`Act`]. Ici : l'etat de la
//! fenetre, le dessin (`draw.rs`), les corrections (`item.rs`) et les gestes.
//!
//! L'axe du temps est en NOIRES DU PROJET, proportionnel : les couches de
//! toutes les pistes s'alignent a la verticale.

#![allow(clippy::too_many_arguments)]

pub mod draw;
mod item;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::time::Instant;

use cp_tab::doc::{TNote, Tech};
use cp_tab::engrave::Engraved;
use cp_tab::fret::Tuning;
use cp_ui::{Button, Ev, Mods, vk};

/// Les accordages proposes (de la corde grave a l'aigue).
/// L'aide : les touches et les gestes (l'infobulle du « ? »).
pub const HELP: &str = "Keys\n\
R  arm   \u{00b7}   Ctrl+R  record   \u{00b7}   Space  play   \u{00b7}   Ctrl+Z  undo\n\
Del  note or item   \u{00b7}   0\u{2013}9  fret   \u{00b7}   \u{2191}\u{2193}  string   \u{00b7}   \u{2190}\u{2192}  note\n\
H  hammer-on   \u{00b7}   P  pull-off\n\
\n\
Mouse\n\
Wheel  scroll   \u{00b7}   Alt+wheel  move in time\n\
Shift+wheel  zoom   \u{00b7}   Ctrl+wheel  lane height\n\
Middle drag  move the view (on the ruler, up/down zooms)\n\
Drag a take  move it (Shift: no snap, Ctrl: copy)   \u{00b7}   drop audio files  new takes\n\
Double-click the MIDI  editor   \u{00b7}   double-click a string  add a note";

pub const TUNINGS: [(&str, [i32; 6]); 5] = [
    ("Standard E", [40, 45, 50, 55, 59, 64]),
    ("Drop D", [38, 45, 50, 55, 59, 64]),
    ("Eb Standard", [39, 44, 49, 54, 58, 63]),
    ("DADGAD", [38, 45, 50, 55, 57, 62]),
    ("Open G", [38, 43, 50, 55, 59, 62]),
];

pub fn tuning_of(k: usize) -> Tuning {
    Tuning {
        strings: TUNINGS[k.min(TUNINGS.len() - 1)].1.to_vec(),
        frets: 22,
    }
}

/// Les reglages de la detection d'un item.
#[derive(Clone, Debug, PartialEq)]
pub struct Detect {
    /// Le seuil d'une attaque, 0..1 : plus bas, plus de notes (et de bruit).
    pub onset: f32,
    /// Le seuil d'une note qui sonne.
    pub frame: f32,
    /// La plus courte note gardee.
    pub min_ms: f64,
    /// Cases de la grille par noire : 2, 4 ou 8.
    pub grid: u32,
    /// Les notes sans attaque (marteaux, tires).
    pub legato: bool,
    /// Montrer les notes fantomes.
    pub ghosts: bool,
    /// Rang dans [`TUNINGS`] (celui de la piste).
    pub tuning: usize,
}

impl Default for Detect {
    fn default() -> Self {
        Detect {
            onset: 0.5,
            frame: 0.3,
            min_ms: 127.7,
            grid: 4,
            legato: true,
            ghosts: true,
            tuning: 0,
        }
    }
}

impl Detect {
    pub fn tuning(&self) -> Tuning {
        tuning_of(self.tuning)
    }

    /// Le seuil d'une note fantome : sous celui des notes gardees.
    pub fn ghost_onset(&self) -> f32 {
        (self.onset * 0.6).max(0.08)
    }

    pub fn to_text(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.onset,
            self.frame,
            self.min_ms,
            self.grid,
            self.legato as u8,
            self.ghosts as u8,
            self.tuning
        )
    }

    pub fn from_text(s: &str) -> Detect {
        let d = Detect::default();
        let v: Vec<&str> = s.split_whitespace().collect();
        let f = |i: usize| v.get(i).and_then(|x| x.parse::<f64>().ok());
        Detect {
            onset: f(0)
                .filter(|x| (0.05..=0.95).contains(x))
                .map_or(d.onset, |x| x as f32),
            frame: f(1)
                .filter(|x| (0.05..=0.95).contains(x))
                .map_or(d.frame, |x| x as f32),
            min_ms: f(2)
                .filter(|x| (20.0..=1000.0).contains(x))
                .unwrap_or(d.min_ms),
            grid: f(3)
                .map(|x| x as u32)
                .filter(|g| [2, 4, 8].contains(g))
                .unwrap_or(d.grid),
            legato: f(4).map_or(d.legato, |x| x != 0.0),
            ghosts: f(5).map_or(d.ghosts, |x| x != 0.0),
            tuning: f(6)
                .map(|x| x as usize)
                .filter(|&t| t < TUNINGS.len())
                .unwrap_or(d.tuning),
        }
    }
}

/// Le son de la tablature (le synthe de Guitar Tab sur la piste tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Guitar,
    Piano,
    Sine,
    Bass,
}

impl Sound {
    pub const ALL: [Sound; 4] = [Sound::Guitar, Sound::Piano, Sound::Sine, Sound::Bass];

    pub fn name(self) -> &'static str {
        match self {
            Sound::Guitar => "Guitar",
            Sound::Piano => "Piano",
            Sound::Sine => "Sine",
            Sound::Bass => "Bass",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Sound {
        Sound::ALL[i.min(3)]
    }
}

/// Le synthe de la piste tab, s'il y est (sinon : l'instrument de Cedric).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Synth {
    pub sound: Sound,
    /// 0..1.
    pub tone: f32,
    /// Secondes.
    pub sustain: f32,
    /// Une note par corde : une note neuve coupe la precedente.
    pub mono: bool,
}

/// Les couches d'une piste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Audio,
    Midi,
    Score,
    Tab,
}

impl Lane {
    pub const ALL: [Lane; 4] = [Lane::Audio, Lane::Midi, Lane::Score, Lane::Tab];

    pub fn name(self) -> &'static str {
        match self {
            Lane::Audio => "Audio",
            Lane::Midi => "MIDI",
            Lane::Score => "Score",
            Lane::Tab => "Tab",
        }
    }
}

/// Les couches montrees : audio, MIDI, partition, tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lanes(pub [bool; 4]);

impl Default for Lanes {
    /// Decide par Cedric (2026-11-22) : audio, partition et tab ; le MIDI dit
    /// la meme chose que la tab.
    fn default() -> Self {
        Lanes([true, false, true, true])
    }
}

impl Lanes {
    pub fn on(&self, l: Lane) -> bool {
        self.0[l as usize]
    }

    pub fn to_text(&self) -> String {
        self.0.iter().map(|&b| if b { '1' } else { '0' }).collect()
    }

    pub fn from_text(s: &str) -> Lanes {
        let mut l = Lanes::default();
        if s.len() == 4 {
            for (i, c) in s.chars().enumerate() {
                l.0[i] = c == '1';
            }
        }
        if !l.0.iter().any(|&b| b) {
            l.0[Lane::Tab as usize] = true;
        }
        l
    }
}

/// L'en-tete d'une piste de guitare : ses deux pistes REAPER.
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    /// Le GUID de la piste audio (le dossier).
    pub guid: String,
    pub tab_guid: Option<String>,
    /// Le numero de la piste dans REAPER (1 = la premiere).
    pub num: i32,
    pub name: String,
    pub armed: bool,
    /// « In 1 », « In 1/2 », « MIDI », « None ».
    pub input: String,
    /// 0 eteint, 1 allume, 2 automatique.
    pub monitor: i32,
    pub mute: bool,
    pub solo: bool,
    pub vol_db: f32,
    pub tab_vol_db: f32,
    /// La crete de l'entree ou de la lecture, 0..1.
    pub meter: f32,
    pub synth: Option<Synth>,
    pub tuning: usize,
    pub lanes: Lanes,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            guid: String::new(),
            tab_guid: None,
            num: 1,
            name: "Guitar".into(),
            armed: false,
            input: "In 1".into(),
            monitor: 0,
            mute: false,
            solo: false,
            vol_db: 0.0,
            tab_vol_db: 0.0,
            meter: 0.0,
            synth: None,
            tuning: 0,
            lanes: Lanes::default(),
        }
    }
}

/// Ou en est la transcription d'un item.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Transcrit : sa tablature est la.
    Ready,
    /// Pas transcrit : on transcrit quand Cedric le demande.
    Idle,
    /// En file d'attente.
    Waiting,
    /// Le reseau ecoute (pourcentage).
    Busy(i32),
    /// Impossible (le message dit pourquoi).
    Failed(String),
}

/// Une prise d'un item (dans l'inspecteur).
#[derive(Clone, Debug, PartialEq)]
pub struct TakeInfo {
    pub name: String,
    /// Sa tablature existe deja (son analyse est en cache).
    pub done: bool,
}

/// Un item audio d'une piste, et sa tablature.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// Le GUID de l'item audio.
    pub guid: String,
    /// Le GUID de la prise active (la cle de sa tablature).
    pub take: String,
    /// La cle de son analyse (le son joue : fichier, decalage, longueur,
    /// vitesse) : une copie de l'item la retrouve.
    pub akey: String,
    pub name: String,
    /// L'item, en noires du projet.
    pub qn0: f64,
    pub qn1: f64,
    /// La barre de mesure ou commence sa partition.
    pub origin_qn: f64,
    pub sig: (i32, i32),
    pub bpm: f64,
    pub takes: Vec<TakeInfo>,
    pub active: usize,
    pub state: Option<State>,
    /// L'item MIDI lie existe.
    pub midi: bool,
    /// Pour chaque trame de l'analyse : sa position en noires, la crete du
    /// son, l'attaque la plus forte.
    pub frame_qn: Vec<f64>,
    pub env: Vec<f32>,
    pub onset: Vec<f32>,
    /// Les fondus (noires) : leur longueur depuis le debut, avant la fin.
    pub fade_in: f64,
    pub fade_out: f64,
    /// La forme d'onde de REAPER sur tout l'item : (min, max) par colonne,
    /// colonnes egales de `qn0` a `qn1`.
    pub peaks: Vec<(f32, f32)>,
    pub doc: Vec<TNote>,
    pub ghosts: Vec<TNote>,
    /// Les notes effacees par Cedric (un nouveau decodage ne les ramene pas).
    pub suppressed: HashSet<(i64, i32)>,
    pub detect: Detect,
    /// Notes injouables laissees de cote par le doigte.
    pub dropped: usize,
    pub(crate) eng: Engraved,
    /// La partition est a jour (faux : a refaire au prochain dessin).
    pub(crate) eng_ok: bool,
}

/// Une piste de guitare : son en-tete et ses items (dans l'ordre du temps).
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub head: Head,
    pub items: Vec<Item>,
}

/// Ce qu'on entend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    Original,
    Tab,
    Both,
}

impl Listen {
    pub fn key(self) -> &'static str {
        match self {
            Listen::Original => "original",
            Listen::Tab => "tab",
            Listen::Both => "both",
        }
    }

    pub fn from_key(s: &str) -> Listen {
        match s {
            "original" => Listen::Original,
            "tab" => Listen::Tab,
            _ => Listen::Both,
        }
    }
}

/// Une prise en train de s'enregistrer, sur une piste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rec {
    pub track: usize,
    pub qn0: f64,
    /// La crete de la piste, battement par battement.
    pub env: Vec<(f64, f32)>,
}

/// Ce qui est choisi : une piste, un item, une note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub t: usize,
    pub i: Option<usize>,
    pub n: Option<usize>,
}

/// Ce que vise la souris.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Ruler,
    /// Une note de la tablature ou de la partition.
    Note {
        t: usize,
        i: usize,
        n: usize,
    },
    Ghost {
        t: usize,
        i: usize,
        g: usize,
    },
    /// Une ligne de la tablature (corde `s`) dans un item.
    TabLine {
        t: usize,
        i: usize,
        s: usize,
    },
    /// Un item dans une couche.
    Item {
        t: usize,
        i: usize,
        lane: Lane,
    },
    /// Montrer ou cacher une couche d'une guitare (les boutons de l'en-tete).
    LaneToggle {
        t: usize,
        lane: Lane,
    },
    /// Le bord d'une prise (le rognage, comme dans l'arrangement).
    ItemEdge {
        t: usize,
        i: usize,
        left: bool,
    },
    /// La poignee d'un fondu (en haut, a la fin du fondu).
    ItemFade {
        t: usize,
        i: usize,
        fade_in: bool,
    },
    /// Une couche, hors des items.
    Lane {
        t: usize,
        lane: Lane,
    },
    Header(usize),
    Arm(usize),
    Input(usize),
    Monitor(usize),
    Mute(usize),
    Solo(usize),
    Fader {
        t: usize,
        tab: bool,
    },
    SoundPick {
        t: usize,
        s: Sound,
    },
    Tuning(usize),
    // La barre
    Listen(Listen),
    Split,
    Follow,
    /// Transcrire les prises neuves d'elles-memes.
    Auto,
    AddTrack,
    /// L'aide (une infobulle : les touches et les gestes).
    Help,
    /// Transcrire cet item (il ne l'est pas encore).
    Transcribe {
        t: usize,
        i: usize,
    },
    // L'inspecteur
    Take(usize),
    Slider(u8),
    Check(u8),
    Grid(u32),
    KeepGhosts,
    Retranscribe,
    DeleteItem,
    Export,
    SynthSlider(u8),
    SynthMono,
    VScroll,
    HScroll,
}

/// Ce que le module doit faire. `t` : la piste, `i` : l'item (rangs dans
/// [`Arr::tracks`], valables jusqu'au prochain rafraichissement).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Deplacer le curseur d'edition de REAPER (noires).
    Seek(f64),
    Play,
    Record,
    Undo,
    /// La tablature d'un item a ete corrigee : l'ecrire dans son item MIDI.
    Edited {
        t: usize,
        i: usize,
    },
    /// Un reglage a change : redecoder l'item (seulement ce passage).
    Detect {
        t: usize,
        i: usize,
        range: Option<(f64, f64)>,
    },
    /// L'accordage de la piste a change : refaire les doigtes.
    Retune {
        t: usize,
    },
    Transcribe {
        t: usize,
        i: usize,
    },
    DeleteItem {
        t: usize,
        i: usize,
    },
    OpenMidi {
        t: usize,
        i: usize,
    },
    SetTake {
        t: usize,
        i: usize,
        k: usize,
    },
    Arm {
        t: usize,
    },
    Monitor {
        t: usize,
    },
    Mute {
        t: usize,
    },
    Solo {
        t: usize,
    },
    InputMenu {
        t: usize,
        x: f32,
        y: f32,
    },
    Volume {
        t: usize,
        tab: bool,
        db: f32,
    },
    Sound {
        t: usize,
        s: Sound,
    },
    /// Un reglage du synthe : 1 tone, 2 sustain, 3 une note par corde.
    Synth {
        t: usize,
        p: u8,
        v: f32,
    },
    /// Les couches montrees d'une piste ont change (a retenir).
    Lanes {
        t: usize,
    },
    /// Ce qu'on entend a change ([`Arr::listen`], [`Arr::split`]).
    Listen,
    AddTrack,
    /// Clic droit sur un en-tete.
    TrackMenu {
        t: usize,
        x: f32,
        y: f32,
    },
    /// Clic droit sur la barre : la place de la fenetre.
    ViewMenu {
        x: f32,
        y: f32,
    },
    Export {
        t: usize,
        i: usize,
    },
    /// Une prise audio glissee : son nouveau debut (noires) et sa guitare.
    MoveItem {
        t: usize,
        i: usize,
        qn0: f64,
        to: usize,
    },
    /// ... copiee (Ctrl, comme dans l'arrangement).
    CopyItem {
        t: usize,
        i: usize,
        qn0: f64,
        to: usize,
    },
    /// La transcription automatique a ete allumee ou coupee ([`Arr::auto`]).
    Auto,
    /// Une prise rognee : ses nouveaux bords (noires) ; son MIDI suit.
    TrimItem {
        t: usize,
        i: usize,
        qn0: f64,
        qn1: f64,
    },
    /// Un fondu : sa longueur (noires).
    FadeItem {
        t: usize,
        i: usize,
        fade_in: bool,
        len: f64,
    },
    /// Une guitare glissee par son en-tete a la place de la guitare `to`.
    MoveTrack {
        t: usize,
        to: usize,
    },
    /// Retranscrire en repartant de zero (Maj) : les notes retouchees et
    /// effacees sont oubliees.
    TranscribeFresh {
        t: usize,
        i: usize,
    },
    /// Un simple clic sur une prise : le curseur a `q`, et l'item de cette
    /// couche (le MIDI, ou l'audio) choisi dans REAPER et montre dans
    /// l'editeur.
    ClickItem {
        t: usize,
        i: usize,
        lane: Lane,
        q: f64,
    },
    /// Une prise coupee en deux (avec son MIDI).
    SplitItem {
        t: usize,
        i: usize,
        qn: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Une note deplacee : son debut d'origine et la position de la souris.
    Note {
        t: usize,
        i: usize,
        n: usize,
        qn0: f64,
        x0: f32,
        moved: bool,
    },
    /// Un passage choisi sur la regle.
    Range {
        q0: f64,
    },
    Slider(u8),
    SynthSlider(u8),
    Fader {
        t: usize,
        tab: bool,
    },
    VScroll {
        y0: f32,
        v0: f32,
    },
    /// La barre de defilement horizontal : la souris et le defilement au
    /// debut du geste.
    HScroll {
        x0: f32,
        s0: f64,
    },
    /// Une prise audio qu'on glisse : dans le temps, et vers une autre
    /// guitare. `x0` : ou le geste a commence ; rien ne bouge avant 4 px.
    Item {
        t: usize,
        i: usize,
        /// La couche saisie : un simple clic montre son item (audio ou MIDI)
        /// dans l'editeur.
        lane: Lane,
        x0: f32,
        /// La noire sous la souris a l'appui.
        qp: f64,
        moved: bool,
        /// Ou elle irait : son debut, sa guitare ; copiee (Ctrl).
        qn0: f64,
        to: usize,
        copy: bool,
    },
    /// Une guitare qu'on glisse par son en-tete (comme les pistes de
    /// REAPER) : ou elle irait.
    TrackMove {
        t: usize,
        y0: f32,
        to: usize,
        moved: bool,
    },
    /// Le bord d'une prise qu'on tire : `q0` le bord d'origine, `q` ou il
    /// irait (cale par le magnetisme de REAPER, Maj : sans).
    Trim {
        t: usize,
        i: usize,
        left: bool,
        qp: f64,
        q0: f64,
        q: f64,
        moved: bool,
    },
    /// La poignee d'un fondu qu'on tire : sa longueur (noires).
    Fade {
        t: usize,
        i: usize,
        fade_in: bool,
        len: f64,
    },
    /// Le bouton du milieu (comme dans REAPER) : la vue suit la main. Sur la
    /// regle, un geste vertical zoome autour de la noire saisie.
    Hand {
        y0: f32,
        v0: f32,
        z0: f32,
        /// La noire saisie : elle reste sous la souris.
        q: f64,
        ruler: bool,
        zooming: bool,
    },
}

/// La selection par cles : la piste, l'item, la note.
type SelKey = (String, Option<String>, Option<(i64, i32)>);

/// Une cible : un rectangle et ce qu'il est.
pub type HBox = (f32, f32, f32, f32, Hit);

/// Les zones de la fenetre (calculees au dessin).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lay {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    /// La barre de defilement horizontal, sous les pistes.
    pub hs_h: f32,
    pub panel_w: f32,
    pub panel_x: f32,
    /// Le haut et le bas de la zone des pistes.
    pub top: f32,
    pub bottom: f32,
    pub head_w: f32,
    pub ruler_h: f32,
    /// La colonne des noms de couche, puis le contenu qui defile.
    pub label_x: f32,
    pub sx: f32,
    pub sw: f32,
    /// Pixels par noire.
    pub ppq: f32,
    /// La hauteur de toutes les pistes.
    pub content_h: f32,
}

pub struct Arr {
    pub tracks: Vec<Track>,
    pub listen: Listen,
    /// En « Both » : l'original a gauche, la tab a droite.
    pub split: bool,
    /// La mesure du projet (barres a partir de la noire 0).
    pub sig: (i32, i32),
    pub bpm: f64,
    /// Pixels par noire a 96 DPI.
    pub zoom: f32,
    /// La noire au bord gauche.
    pub scroll: f64,
    /// Le defilement vertical des pistes (px).
    pub vscroll: f32,
    /// La hauteur des couches audio et MIDI (Ctrl + molette).
    pub vzoom: f32,
    pub follow: bool,
    /// La tete de lecture (en lecture) ; sinon le curseur d'edition.
    pub playhead: Option<f64>,
    pub playing: bool,
    /// Le curseur d'edition de REAPER (montre aussi pendant la lecture).
    pub edit_cursor: Option<f64>,
    /// La ligne verticale a la souris, comme REAPER (ses reglages : montree,
    /// calee sur la grille) ; `guide_q` : ou elle est.
    pub guide: Guide,
    pub guide_q: Option<f64>,
    pub rec: Vec<Rec>,
    /// Transcrire les prises neuves d'elles-memes (sinon : sur demande).
    pub auto: bool,
    /// Le magnetisme de REAPER (noires -> noires), pose par le module ; sans
    /// lui, la double croche.
    pub snapper: Option<Box<dyn Fn(f64) -> f64 + Send>>,
    /// La grille de REAPER, calculee au zoom de CETTE vue (prioritaire sur
    /// `snapper`) : SnapToGrid prend celle de l'arrangement de REAPER, a SON
    /// zoom (cache derriere Guitar Tab, il calait toutes les deux mesures).
    pub grid: Option<SnapGrid>,
    pub sel: Option<Sel>,
    pub range: Option<(f64, f64)>,
    /// Ce qui se passe (« Transcribing 45 % »).
    pub busy: Option<String>,
    pub status: String,
    pub lay: Lay,
    pub scale: f32,
    pub(crate) boxes: Vec<HBox>,
    pub(crate) hover: Hit,
    drag: Option<Drag>,
    /// Le premier chiffre d'une case tapee (pour « 12 »), et quand.
    digit: Option<(i32, Instant)>,
    /// La selection retenue par cles (GUID de l'item, cle de la note) pour
    /// la retrouver apres un rafraichissement des pistes.
    sel_key: Option<SelKey>,
    dirty: bool,
    /// Les zones a redessiner quand tout ne l'est pas (un survol).
    damage: Vec<cp_gfx::IRect>,
    /// La souris : sa place et les touches tenues (les editions au survol,
    /// le curseur).
    mouse: (f32, f32, Mods),
    /// L'item qui a le focus dans la prise choisie : son audio ou son MIDI
    /// (celui que REAPER a choisi), montre comme un item choisi de REAPER.
    pub focus_lane: Lane,
    /// Le bas des pistes a l'ecran (les traits s'y arretent).
    pub(crate) tracks_bottom: f32,
}

/// Le magnetisme de REAPER : sa division, s'il est allume, et l'ecart
/// minimum entre deux lignes de magnetisme (`projgridsnapmin`, en px) au-dela
/// duquel REAPER prend une grille deux fois plus large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapGrid {
    pub on: bool,
    /// La division, en noires (la grille de REAPER en rondes x 4).
    pub div_qn: f64,
    pub min_px: f32,
}

impl SnapGrid {
    /// Le pas effectif a `ppq` pixels par noire.
    pub fn step(&self, ppq: f32) -> f64 {
        let mut g = self.div_qn.max(1.0 / 64.0);
        let min = self.min_px.max(1.0) as f64;
        while g * (ppq as f64) < min && g < 4096.0 {
            g *= 2.0;
        }
        g
    }
}

pub use cp_ui::Guide;

impl Default for Arr {
    fn default() -> Self {
        Arr::new()
    }
}

impl Arr {
    pub fn new() -> Arr {
        Arr {
            tracks: Vec::new(),
            listen: Listen::Both,
            split: false,
            sig: (4, 4),
            bpm: 120.0,
            zoom: 56.0,
            scroll: 0.0,
            vscroll: 0.0,
            vzoom: 1.0,
            follow: true,
            playhead: None,
            playing: false,
            rec: Vec::new(),
            auto: false,
            snapper: None,
            grid: None,
            sel: None,
            range: None,
            busy: None,
            status: String::new(),
            lay: Lay::default(),
            scale: 1.0,
            boxes: Vec::new(),
            hover: Hit::None,
            drag: None,
            digit: None,
            sel_key: None,
            dirty: true,
            edit_cursor: None,
            guide: Guide::default(),
            guide_q: None,
            damage: Vec::new(),
            tracks_bottom: 0.0,
            mouse: (0.0, 0.0, Mods::default()),
            focus_lane: Lane::Audio,
        }
    }

    /// Tout est a redessiner.
    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        self.damage.clear();
        std::mem::take(&mut self.dirty)
    }

    /// Quelque chose est a redessiner (tout, ou des zones).
    pub fn needs_draw(&self) -> bool {
        self.dirty || !self.damage.is_empty()
    }

    /// Les zones de la scene a redessiner : toute la fenetre, ou celles d'un
    /// survol ; vide : rien.
    /// `w x h` : la taille de la fenetre (pas celle du dernier dessin, qui
    /// n'existe pas encore apres une ouverture ou un rechargement : la vue
    /// restait noire).
    pub fn take_damage(&mut self, w: i32, h: i32) -> Vec<cp_gfx::IRect> {
        if std::mem::take(&mut self.dirty) {
            self.damage.clear();
            return vec![cp_gfx::IRect::new(0, 0, w, h)];
        }
        let v = std::mem::take(&mut self.damage);
        let v = cp_gfx::layers::merge(v);
        if v.len() > 4 {
            let u = v.iter().fold(cp_gfx::IRect::default(), |a, r| a.union(r));
            return vec![u];
        }
        v
    }

    /// Le vumetre d'une piste : seule sa barre se redessine.
    pub fn set_meter(&mut self, t: usize, m: f32) {
        let Some(tr) = self.tracks.get_mut(t) else {
            return;
        };
        if (tr.head.meter - m).abs() <= 0.004 {
            return;
        }
        tr.head.meter = m;
        // Il est sous le fader de la guitare (hors de l'ecran : rien).
        if let Some(r) = self.rect_of(Hit::Fader { t, tab: false }) {
            self.damage.push(cp_gfx::IRect::new(
                r.0.floor() as i32,
                (r.1 + r.3).floor() as i32 - 1,
                r.2.ceil() as i32 + 1,
                (6.0 * self.scale).ceil() as i32 + 2,
            ));
        }
    }

    /// La cible survolee change : seules ses deux places se redessinent.
    fn set_hover(&mut self, h: Hit) {
        if h == self.hover {
            return;
        }
        let pad = (4.0 * self.scale).ceil();
        for r in [self.rect_of(self.hover), self.rect_of(h)]
            .into_iter()
            .flatten()
        {
            self.damage.push(cp_gfx::IRect::new(
                (r.0 - pad).floor() as i32,
                (r.1 - pad).floor() as i32,
                (r.2 + 2.0 * pad).ceil() as i32 + 1,
                (r.3 + 2.0 * pad).ceil() as i32 + 1,
            ));
        }
        if self.hover != Hit::None && self.rect_of(self.hover).is_none() {
            self.dirty = true;
        }
        self.hover = h;
    }

    /// Les traits qui bougent seuls par-dessus la scene : le curseur
    /// d'edition, la tete de lecture, la ligne a la souris (un pixel, comme
    /// REAPER).
    pub fn strokes(&self, th: &cp_ui::Theme) -> Vec<cp_gfx::layers::Stroke> {
        use cp_gfx::layers::Stroke;
        let l = self.lay;
        let top = l.top as i32;
        let bot = self.tracks_bottom.min(l.bottom - l.hs_h).max(l.top) as i32;
        let (x0, x1) = (l.sx, l.sx + l.sw);
        let mut out = Vec::new();
        let mut line = |q: f64, c: cp_gfx::Color| {
            let x = self.x_of(q).round();
            if x >= x0 && x < x1 && bot > top {
                out.push(Stroke {
                    r: cp_gfx::IRect::new(x as i32, top, 1, bot - top),
                    c,
                });
            }
        };
        if let (true, Some(q)) = (self.guide.on, self.guide_q) {
            line(q, self.guide.color.unwrap_or(th.text_faint));
        }
        if let Some(q) = self.edit_cursor {
            line(q, th.edit_cursor);
        }
        if let (true, Some(q)) = (self.playing, self.playhead) {
            line(q, th.playhead);
        }
        out
    }

    /// Les noires d'une mesure.
    pub fn bar_qn(&self) -> f64 {
        self.sig.0.max(1) as f64 * 4.0 / self.sig.1.max(1) as f64
    }

    pub fn x_of(&self, qn: f64) -> f32 {
        self.lay.sx + ((qn - self.scroll) as f32) * self.lay.ppq
    }

    pub fn qn_of(&self, x: f32) -> f64 {
        self.scroll + ((x - self.lay.sx) / self.lay.ppq.max(1e-3)) as f64
    }

    /// Les noires visibles.
    pub fn view_qn(&self) -> f64 {
        (self.lay.sw / self.lay.ppq.max(1e-3)) as f64
    }

    /// La fin du contenu (noires) : le dernier item, la tete de lecture,
    /// plus une demi-vue pour respirer.
    pub fn end_qn(&self) -> f64 {
        let mut e: f64 = 0.0;
        for t in &self.tracks {
            for it in &t.items {
                e = e.max(it.qn1);
            }
        }
        if let Some(p) = self.playhead {
            e = e.max(p);
        }
        let v = self.view_qn();
        (e + v * 0.5).max(v).max(self.scroll + v)
    }

    /// Zoome a `z`, la noire `q` restant a l'abscisse `x`.
    fn zoom_at(&mut self, z: f32, q: f64, x: f32) {
        self.zoom = z.clamp(8.0, 600.0);
        self.lay.ppq = self.zoom * self.scale;
        self.scroll = (q - ((x - self.lay.sx) / self.lay.ppq) as f64).max(0.0);
    }

    pub fn item(&self, t: usize, i: usize) -> Option<&Item> {
        self.tracks.get(t)?.items.get(i)
    }

    pub fn item_mut(&mut self, t: usize, i: usize) -> Option<&mut Item> {
        self.tracks.get_mut(t)?.items.get_mut(i)
    }

    /// L'item choisi.
    pub fn sel_item(&self) -> Option<(usize, usize)> {
        let s = self.sel?;
        Some((s.t, s.i?))
    }

    /// La piste choisie (sinon la premiere).
    pub fn sel_track(&self) -> Option<usize> {
        self.sel
            .map(|s| s.t)
            .filter(|&t| t < self.tracks.len())
            .or((!self.tracks.is_empty()).then_some(0))
    }

    /// Choisit (et retient par cles, pour un rafraichissement).
    pub fn select(&mut self, s: Option<Sel>) {
        if s != self.sel {
            self.sel = s;
            self.digit = None;
            self.dirty = true;
        }
        self.sel_key = s.map(|s| {
            let tg = self
                .tracks
                .get(s.t)
                .map(|t| t.head.guid.clone())
                .unwrap_or_default();
            let it = s.i.and_then(|i| self.item(s.t, i));
            (
                tg,
                it.map(|x| x.guid.clone()),
                s.n.and_then(|n| it?.doc.get(n)).map(TNote::key),
            )
        });
        // La note choisie reste a l'ecran.
        if let Some(q) = s
            .and_then(|s| self.item(s.t, s.i?)?.doc.get(s.n?))
            .map(|n| n.qn0)
        {
            let v = self.view_qn();
            if v > 0.0 && (q < self.scroll || q > self.scroll + v * 0.9) {
                self.scroll = q - v * 0.2;
            }
        }
    }

    /// Apres un rafraichissement des pistes par le module : retrouve la
    /// selection par ses cles.
    pub fn reselect(&mut self) {
        let Some((tg, ig, nk)) = self.sel_key.clone() else {
            self.sel = None;
            return;
        };
        let Some(t) = self.tracks.iter().position(|x| x.head.guid == tg) else {
            self.sel = None;
            return;
        };
        let i = ig.and_then(|g| self.tracks[t].items.iter().position(|x| x.guid == g));
        let n = match (i, nk) {
            (Some(i), Some(k)) => self.tracks[t].items[i]
                .doc
                .iter()
                .position(|x| x.key() == k),
            _ => None,
        };
        self.sel = Some(Sel { t, i, n });
        self.dirty = true;
    }

    /// Le battement : la tete de lecture suit, la page tourne.
    pub fn set_playhead(&mut self, qn: Option<f64>, playing: bool) {
        if qn != self.playhead || playing != self.playing {
            // La tete est un trait par-dessus la scene : elle ne la salit que
            // si une prise s'enregistre (elle grandit avec elle) ou si la fin
            // du contenu recule.
            let end = self.end_qn();
            self.playhead = qn;
            self.playing = playing;
            if !self.rec.is_empty() || (self.end_qn() - end).abs() > 1e-9 {
                self.dirty = true;
            }
        }
        if let (true, true, Some(p)) = (self.follow, playing, qn) {
            let v = self.view_qn();
            if v > 0.0 && (p < self.scroll || p > self.scroll + v * 0.85) {
                self.scroll = p - v * 0.1;
                self.dirty = true;
            }
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let Some(b) = self
            .boxes
            .iter()
            .rev()
            .find(|b| x >= b.0 && x < b.0 + b.2 && y >= b.1 && y < b.1 + b.3)
        else {
            return Hit::None;
        };
        // Une prise, comme dans l'arrangement : ses bords se rognent, ses
        // coins du haut tirent les fondus.
        if let Hit::Item { t, i, lane } = b.4 {
            if matches!(lane, Lane::Audio | Lane::Midi) {
                if let Some(it) = self.item(t, i) {
                    let (xa, xb) = (self.x_of(it.qn0), self.x_of(it.qn1));
                    let e = (5.0 * self.scale).min((xb - xa) / 4.0).max(1.0);
                    if lane == Lane::Audio && y < b.1 + 9.0 * self.scale {
                        let mid = (xa + xb) * 0.5;
                        let fi = self.x_of(it.qn0 + it.fade_in);
                        let fo = self.x_of(it.qn1 - it.fade_out);
                        if (x - fi).abs() <= e && x < mid {
                            return Hit::ItemFade {
                                t,
                                i,
                                fade_in: true,
                            };
                        }
                        if (x - fo).abs() <= e && x >= mid {
                            return Hit::ItemFade {
                                t,
                                i,
                                fade_in: false,
                            };
                        }
                    }
                    if x - xa < e {
                        return Hit::ItemEdge { t, i, left: true };
                    }
                    if xb - x <= e {
                        return Hit::ItemEdge { t, i, left: false };
                    }
                }
            }
        }
        b.4
    }

    /// La prise sous la souris (ou qu'elle soit dans ses couches).
    fn item_under(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        match self.hit(x, y) {
            Hit::Item { t, i, .. }
            | Hit::ItemEdge { t, i, .. }
            | Hit::ItemFade { t, i, .. }
            | Hit::Note { t, i, .. }
            | Hit::Ghost { t, i, .. }
            | Hit::TabLine { t, i, .. }
            | Hit::Transcribe { t, i } => Some((t, i)),
            _ => None,
        }
    }

    /// Une edition au survol (les touches de Cedric) : sur la prise sous la
    /// souris, a la place de la souris calee par le magnetisme de REAPER ;
    /// sur une note, Supprimer l'efface.
    pub fn hover_edit(&mut self, e: cp_ui::HoverEdit) -> Option<Act> {
        use cp_ui::HoverEdit as H;
        let (x, y, m) = self.mouse;
        if e == H::Delete {
            if let Hit::Note { t, i, n } = self.hit(x, y) {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: Some(n),
                }));
                return self.delete_note();
            }
        }
        let (t, i) = self.item_under(x, y)?;
        let it = self.item(t, i)?;
        let (q0, q1) = (it.qn0, it.qn1);
        let raw = self.qn_of(x);
        let q = if m.shift { raw } else { self.snap_item(raw) };
        let inside = q > q0 + 1e-6 && q < q1 - 1e-6;
        match e {
            H::Delete => Some(Act::DeleteItem { t, i }),
            H::TrimLeft if inside => Some(Act::TrimItem {
                t,
                i,
                qn0: q,
                qn1: q1,
            }),
            H::TrimRight if inside => Some(Act::TrimItem {
                t,
                i,
                qn0: q0,
                qn1: q,
            }),
            H::Split if inside => Some(Act::SplitItem { t, i, qn: q }),
            H::FadeIn if inside => Some(Act::FadeItem {
                t,
                i,
                fade_in: true,
                len: q - q0,
            }),
            H::FadeOut if inside => Some(Act::FadeItem {
                t,
                i,
                fade_in: false,
                len: q1 - q,
            }),
            _ => None,
        }
    }

    /// Le curseur de REAPER a montrer. Au repos, la fleche (Cedric,
    /// 2026-11-22 : « normal, et deplacer lorsque je deplace ») ; les bords
    /// et les poignees de fondu montrent leur geste ; Ctrl sur une prise, la
    /// copie. Pendant un geste, le sien : deplacer des que la prise bouge.
    pub fn pointer(&self, x: f32, y: f32, m: Mods) -> cp_ui::Pointer {
        use cp_ui::Pointer as P;
        match self.drag {
            Some(Drag::Item { moved: false, .. }) => return P::Arrow,
            Some(Drag::Item { copy, .. }) => {
                return if copy { P::ItemCopy } else { P::ItemMove };
            }
            Some(Drag::Trim { left, .. }) => {
                return if left { P::TrimLeft } else { P::TrimRight };
            }
            Some(Drag::Fade { fade_in, .. }) => {
                return if fade_in { P::FadeIn } else { P::FadeOut };
            }
            Some(Drag::Hand { .. }) => return P::HandScroll,
            Some(Drag::TrackMove { moved: true, .. }) => return P::TrackDrag,
            Some(Drag::Range { .. }) => return P::TimeSelEdge,
            Some(_) => return P::Arrow,
            None => {}
        }
        match self.hit(x, y) {
            Hit::Item {
                lane: Lane::Audio | Lane::Midi,
                ..
            } => {
                if m.ctrl {
                    P::ItemCopy
                } else {
                    P::Arrow
                }
            }
            Hit::ItemEdge { left, .. } => {
                if left {
                    P::TrimLeft
                } else {
                    P::TrimRight
                }
            }
            Hit::ItemFade { fade_in, .. } => {
                if fade_in {
                    P::FadeIn
                } else {
                    P::FadeOut
                }
            }
            _ => P::Arrow,
        }
    }

    /// La guitare qu'on glisse et ou elle irait (le dessin le montre).
    pub(crate) fn track_move(&self) -> Option<(usize, usize)> {
        match self.drag {
            Some(Drag::TrackMove {
                t, to, moved: true, ..
            }) => Some((t, to)),
            _ => None,
        }
    }

    /// Le bord qu'on tire et ou il irait (le dessin le montre).
    pub(crate) fn trim_preview(&self) -> Option<(usize, usize, bool, f64)> {
        match self.drag {
            Some(Drag::Trim {
                t,
                i,
                left,
                q,
                moved: true,
                ..
            }) => Some((t, i, left, q)),
            _ => None,
        }
    }

    /// Le fondu qu'on tire : sa longueur (le dessin la montre).
    pub(crate) fn fade_preview(&self) -> Option<(usize, usize, bool, f64)> {
        match self.drag {
            Some(Drag::Fade { t, i, fade_in, len }) => Some((t, i, fade_in, len)),
            _ => None,
        }
    }

    /// Ou est une cible a l'ecran (tests, auto-test).
    pub fn rect_of(&self, h: Hit) -> Option<(f32, f32, f32, f32)> {
        self.boxes
            .iter()
            .rev()
            .find(|b| b.4 == h)
            .map(|b| (b.0, b.1, b.2, b.3))
    }

    // -----------------------------------------------------------------------
    // Les corrections : sur la note choisie de l'item choisi
    // -----------------------------------------------------------------------

    fn edited(&mut self, t: usize, i: usize) -> Option<Act> {
        if let Some(it) = self.item_mut(t, i) {
            it.touch();
        }
        self.dirty = true;
        Some(Act::Edited { t, i })
    }

    fn with_note<R>(
        &mut self,
        f: impl FnOnce(&mut Item, usize) -> Option<R>,
    ) -> Option<(usize, usize, R)> {
        let s = self.sel?;
        let (i, n) = (s.i?, s.n?);
        let r = f(self.item_mut(s.t, i)?, n)?;
        Some((s.t, i, r))
    }

    pub fn set_fret(&mut self, fret: i32) -> Option<Act> {
        let (t, i, _) = self.with_note(|it, n| it.set_fret(n, fret))?;
        self.edited(t, i)
    }

    pub fn change_string(&mut self, dir: i32) -> Option<Act> {
        let (t, i, _) = self.with_note(|it, n| it.change_string(n, dir))?;
        self.edited(t, i)
    }

    pub fn set_tech(&mut self, tech: Tech) -> Option<Act> {
        let (t, i, _) = self.with_note(|it, n| it.set_tech(n, tech))?;
        self.edited(t, i)
    }

    pub fn delete_note(&mut self) -> Option<Act> {
        let (t, i, next) = self.with_note(|it, n| it.delete_note(n))?;
        let s = Some(Sel {
            t,
            i: Some(i),
            n: next,
        });
        self.select(s);
        self.edited(t, i)
    }

    pub fn add_note(
        &mut self,
        t: usize,
        i: usize,
        qn: f64,
        string: usize,
        fret: i32,
    ) -> Option<Act> {
        let n = self.item_mut(t, i)?.add_note(qn, string, fret)?;
        self.select(Some(Sel {
            t,
            i: Some(i),
            n: Some(n),
        }));
        self.edited(t, i)
    }

    pub fn keep_ghost(&mut self, t: usize, i: usize, g: usize) -> Option<Act> {
        let n = self.item_mut(t, i)?.keep_ghost(g)?;
        self.select(Some(Sel {
            t,
            i: Some(i),
            n: Some(n),
        }));
        self.edited(t, i)
    }

    pub fn keep_ghosts(&mut self) -> Option<Act> {
        let (t, i) = self.sel_item()?;
        let r = self.range;
        if self.item_mut(t, i)?.keep_ghosts(r) == 0 {
            return None;
        }
        self.edited(t, i)
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn slider_value(&self, k: u8, x: f32, synth: bool) -> f32 {
        let l = self.lay;
        let x0 = l.panel_x + draw::SLIDER_X * self.scale;
        let w = (l.w - x0 - draw::SLIDER_R * self.scale).max(10.0);
        let _ = (k, synth);
        ((x - x0) / w).clamp(0.0, 1.0)
    }

    fn set_slider(&mut self, k: u8, x: f32) -> Option<Act> {
        let tv = self.slider_value(k, x, false);
        let (t, i) = self.sel_item()?;
        let range = self.range;
        let it = self.item_mut(t, i)?;
        // Les seuils de 0,05 a 0,95 au centieme ; la note la plus courte de
        // 20 a 400 ms.
        let v = ((0.05 + tv * 0.9) * 100.0).round() / 100.0;
        match k {
            0 => it.detect.onset = v,
            1 => it.detect.frame = v,
            _ => it.detect.min_ms = (20.0 + tv as f64 * 380.0).round(),
        }
        self.dirty = true;
        Some(Act::Detect { t, i, range })
    }

    fn set_synth_slider(&mut self, k: u8, x: f32) -> Option<Act> {
        let tv = self.slider_value(k, x, true);
        let t = self.sel_track()?;
        let sy = self.tracks.get_mut(t)?.head.synth.as_mut()?;
        let v = match k {
            1 => {
                sy.tone = (tv * 100.0).round() / 100.0;
                sy.tone
            }
            _ => {
                sy.sustain = (0.1 + tv * 5.9).clamp(0.1, 6.0);
                sy.sustain = (sy.sustain * 10.0).round() / 10.0;
                sy.sustain
            }
        };
        self.dirty = true;
        Some(Act::Synth { t, p: k, v })
    }

    /// La position d'un fader vers des dB (-60 a +12, le bord gauche = -inf).
    pub fn fader_db(t: f32) -> f32 {
        if t <= 0.005 {
            -150.0
        } else {
            -60.0 + 72.0 * t.clamp(0.0, 1.0).powf(0.75)
        }
    }

    pub fn fader_pos(db: f32) -> f32 {
        if db <= -60.0 {
            0.0
        } else {
            ((db + 60.0) / 72.0).clamp(0.0, 1.0).powf(1.0 / 0.75)
        }
    }

    fn set_fader(&mut self, t: usize, tab: bool, x: f32) -> Option<Act> {
        let (fx, fw) = draw::fader_span(self, t);
        let db = Arr::fader_db((x - fx) / fw.max(1.0));
        let db = (db * 10.0).round() / 10.0;
        let h = &mut self.tracks.get_mut(t)?.head;
        if tab {
            h.tab_vol_db = db;
        } else {
            h.vol_db = db;
        }
        self.dirty = true;
        Some(Act::Volume { t, tab, db })
    }

    /// Un geste de l'utilisateur sur la vue : elle ne suit plus la lecture.
    fn unfollow(&mut self) {
        if self.playing {
            self.follow = false;
        }
        self.dirty = true;
    }

    fn max_vscroll(&self) -> f32 {
        let l = self.lay;
        (l.content_h - (l.bottom - l.hs_h - l.top - l.ruler_h)).max(0.0)
    }

    /// Un evenement de la fenetre ; rend ce que le module doit faire.
    pub fn event(&mut self, ev: &Ev) -> Option<Act> {
        match *ev {
            Ev::Move { x, y, mods } => {
                self.mouse = (x, y, mods);
                match self.drag {
                    Some(Drag::Note {
                        t,
                        i,
                        n,
                        qn0,
                        x0,
                        moved,
                    }) => {
                        if !moved && (x - x0).abs() < 4.0 * self.scale {
                            return None;
                        }
                        self.drag = Some(Drag::Note {
                            t,
                            i,
                            n,
                            qn0,
                            x0,
                            moved: true,
                        });
                        let dq = ((x - x0) / self.lay.ppq.max(1e-3)) as f64;
                        if let Some(it) = self.item_mut(t, i) {
                            let q = it.snap(qn0 + dq);
                            if it.move_note(n, q) {
                                self.dirty = true;
                            }
                        }
                        return None;
                    }
                    Some(Drag::Range { q0 }) => {
                        let q = self.cursor_q(x, mods);
                        self.range = Some((q0.min(q), q0.max(q)));
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::Item {
                        t,
                        i,
                        lane,
                        x0,
                        qp,
                        moved,
                        ..
                    }) => {
                        if !moved && (x - x0).abs() < 4.0 * self.scale {
                            return None;
                        }
                        let it = self.item(t, i)?;
                        // Comme dans l'arrangement : le debut de la prise se
                        // cale selon le magnetisme de REAPER ; Maj : sans ;
                        // Ctrl : une copie.
                        let raw = it.qn0 + (self.qn_of(x) - qp);
                        let m = match ev {
                            Ev::Move { mods, .. } => *mods,
                            _ => Mods::default(),
                        };
                        let q = if m.shift { raw } else { self.snap_item(raw) }.max(0.0);
                        let to = self.track_at(y).unwrap_or(t);
                        self.drag = Some(Drag::Item {
                            t,
                            i,
                            lane,
                            x0,
                            qp,
                            moved: true,
                            qn0: q,
                            to,
                            copy: m.ctrl,
                        });
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::Trim {
                        t,
                        i,
                        left,
                        qp,
                        q0,
                        moved,
                        ..
                    }) => {
                        let (lo, hi) = {
                            let it = self.item(t, i)?;
                            (it.qn0, it.qn1)
                        };
                        let raw = q0 + (self.qn_of(x) - qp);
                        let q = if mods.shift { raw } else { self.snap_item(raw) };
                        let min = 1.0 / 64.0;
                        let q = if left {
                            q.clamp(0.0, hi - min)
                        } else {
                            q.max(lo + min)
                        };
                        let moved = moved || (q - q0).abs() > 1e-9;
                        self.drag = Some(Drag::Trim {
                            t,
                            i,
                            left,
                            qp,
                            q0,
                            q,
                            moved,
                        });
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::Fade { t, i, fade_in, .. }) => {
                        let (lo, hi) = {
                            let it = self.item(t, i)?;
                            (it.qn0, it.qn1)
                        };
                        let raw = self.qn_of(x);
                        let q = if mods.shift { raw } else { self.snap_item(raw) };
                        let len = if fade_in { q - lo } else { hi - q }.clamp(0.0, hi - lo);
                        self.drag = Some(Drag::Fade { t, i, fade_in, len });
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::TrackMove { t, y0, moved, .. }) => {
                        let moved = moved || (y - y0).abs() > 4.0 * self.scale;
                        if !moved {
                            return None;
                        }
                        let last = self.tracks.len().saturating_sub(1);
                        let to =
                            self.track_at(y)
                                .unwrap_or(if y < self.lay.top + self.lay.ruler_h {
                                    0
                                } else {
                                    last
                                });
                        self.drag = Some(Drag::TrackMove { t, y0, to, moved });
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::Slider(k)) => return self.set_slider(k, x),
                    Some(Drag::SynthSlider(k)) => return self.set_synth_slider(k, x),
                    Some(Drag::Fader { t, tab }) => return self.set_fader(t, tab, x),
                    Some(Drag::VScroll { y0, v0 }) => {
                        let l = self.lay;
                        let view = (l.bottom - l.hs_h - l.top - l.ruler_h).max(1.0);
                        let k = l.content_h / view;
                        self.vscroll = (v0 + (y - y0) * k).clamp(0.0, self.max_vscroll());
                        self.dirty = true;
                        return None;
                    }
                    Some(Drag::HScroll { x0, s0 }) => {
                        let end = self.end_qn();
                        let k = end / self.lay.sw.max(1.0) as f64;
                        self.scroll = (s0 + (x - x0) as f64 * k).clamp(0.0, end);
                        self.unfollow();
                        return None;
                    }
                    Some(Drag::Hand {
                        y0,
                        v0,
                        z0,
                        q,
                        ruler,
                        zooming,
                    }) => {
                        let dy = y - y0;
                        if ruler {
                            // Une zone morte : un geste horizontal ne zoome pas.
                            let zooming = zooming || dy.abs() > 6.0 * self.scale;
                            let z = if zooming {
                                z0 * (-dy / (120.0 * self.scale)).exp2()
                            } else {
                                z0
                            };
                            self.zoom_at(z, q, x);
                            self.drag = Some(Drag::Hand {
                                y0,
                                v0,
                                z0,
                                q,
                                ruler,
                                zooming,
                            });
                        } else {
                            self.zoom_at(self.zoom, q, x);
                            self.vscroll = (v0 - dy).clamp(0.0, self.max_vscroll());
                        }
                        self.unfollow();
                        return None;
                    }
                    None => {}
                }
                let h = self.hit(x, y);
                self.set_hover(h);
                self.guide_at(x, y);
                None
            }
            Ev::Leave => {
                self.set_hover(Hit::None);
                self.guide_q = None;
                None
            }
            Ev::Down {
                x,
                y,
                b: Button::Middle,
                ..
            } => {
                let l = self.lay;
                if x >= l.sx && x < l.sx + l.sw && y >= l.top && y < l.bottom {
                    self.drag = Some(Drag::Hand {
                        y0: y,
                        v0: self.vscroll,
                        z0: self.zoom,
                        q: self.qn_of(x),
                        ruler: y < l.top + l.ruler_h,
                        zooming: false,
                    });
                }
                None
            }
            Ev::Down { x, y, b, dbl, mods } => self.down(x, y, b, dbl, mods),
            Ev::Up { .. } => {
                let d = self.drag.take();
                self.dirty = true;
                match d {
                    Some(Drag::Note {
                        moved: true,
                        t,
                        i,
                        n,
                        ..
                    }) => {
                        let it = self.item_mut(t, i)?;
                        let n2 = it.resort(n);
                        self.select(Some(Sel {
                            t,
                            i: Some(i),
                            n: n2,
                        }));
                        self.edited(t, i)
                    }
                    Some(Drag::Range { .. }) => {
                        // Un simple clic sur la regle : pas de passage.
                        if self.range.is_some_and(|(a, b)| b - a < 1e-6) {
                            self.range = None;
                        }
                        None
                    }
                    Some(Drag::Item {
                        t,
                        i,
                        lane,
                        x0,
                        moved,
                        qn0,
                        to,
                        copy,
                        ..
                    }) => {
                        if !moved {
                            // Un simple clic, comme dans l'arrangement : le
                            // curseur, et l'item choisi (son MIDI ou son audio,
                            // selon la couche) dans REAPER et l'editeur.
                            return Some(Act::ClickItem {
                                t,
                                i,
                                lane,
                                q: self.cursor_q(x0, self.mouse.2),
                            });
                        }
                        if copy {
                            return Some(Act::CopyItem { t, i, qn0, to });
                        }
                        let it = self.item(t, i)?;
                        if (it.qn0 - qn0).abs() < 1e-9 && to == t {
                            return None;
                        }
                        Some(Act::MoveItem { t, i, qn0, to })
                    }
                    Some(Drag::Trim {
                        t,
                        i,
                        left,
                        q,
                        moved: true,
                        ..
                    }) => {
                        let it = self.item(t, i)?;
                        let (a, b) = if left { (q, it.qn1) } else { (it.qn0, q) };
                        Some(Act::TrimItem {
                            t,
                            i,
                            qn0: a,
                            qn1: b,
                        })
                    }
                    Some(Drag::Trim { qp, .. }) => Some(Act::Seek(qp)),
                    Some(Drag::TrackMove {
                        t, to, moved: true, ..
                    }) if to != t => Some(Act::MoveTrack { t, to }),
                    Some(Drag::Fade { t, i, fade_in, len }) => {
                        Some(Act::FadeItem { t, i, fade_in, len })
                    }
                    _ => None,
                }
            }
            Ev::Wheel {
                dy, dx, mods, x, y, ..
            } => {
                // Les gestes de REAPER chez Cedric : molette = defilement
                // vertical, Alt = dans le temps, Maj = zoom horizontal, Ctrl =
                // hauteur des couches ; molette horizontale = dans le temps,
                // Alt (ou Ctrl, a l'envers) = zoom.
                let l = self.lay;
                let _ = y;
                if l.panel_w > 0.0 && x >= l.panel_x {
                    return None;
                }
                let x = x.clamp(l.sx, l.sx + l.sw);
                if dx != 0.0 {
                    if mods.alt || mods.ctrl {
                        let k = if mods.ctrl { -dx } else { dx };
                        let q = self.qn_of(x);
                        self.zoom_at(self.zoom * 1.25f32.powf(k), q, x);
                    } else {
                        self.scroll = (self.scroll + dx as f64 * self.view_qn() * 0.15).max(0.0);
                    }
                } else if mods.shift {
                    let q = self.qn_of(x);
                    self.zoom_at(self.zoom * 1.25f32.powf(dy), q, x);
                } else if mods.alt {
                    // Comme REAPER chez Cedric (Alt+molette = « View: Scroll
                    // horizontally », 988) : vers le haut, la vue avance.
                    self.scroll = (self.scroll + dy as f64 * self.view_qn() * 0.15).max(0.0);
                } else if mods.ctrl {
                    self.vzoom = (self.vzoom * 1.15f32.powf(dy)).clamp(0.6, 3.0);
                    self.dirty = true;
                    return None;
                } else {
                    self.vscroll =
                        (self.vscroll - dy * 60.0 * self.scale).clamp(0.0, self.max_vscroll());
                    self.dirty = true;
                    return None;
                }
                self.unfollow();
                None
            }
            Ev::Key { vk: k, mods, .. } => self.key(k, mods),
            Ev::Char(_) => None,
        }
    }

    /// Le debut d'une prise glissee, cale par le magnetisme de REAPER.
    fn snap_item(&self, q: f64) -> f64 {
        if let Some(g) = self.grid {
            if !g.on {
                return q;
            }
            let step = g.step(self.lay.ppq.max(1e-3));
            return (q / step).round() * step;
        }
        match &self.snapper {
            Some(f) => f(q),
            None => (q * 4.0).round() / 4.0,
        }
    }

    /// La ligne a la souris : sur le contenu (regle comprise), la ou un clic
    /// poserait le curseur.
    fn guide_at(&mut self, x: f32, y: f32) {
        let l = self.lay;
        let over = x >= l.sx && x < l.sx + l.sw && y >= l.top && y < l.bottom - l.hs_h;
        self.guide_q = (self.guide.on && over).then(|| self.cursor_q(x, self.mouse.2));
    }

    /// Ou un clic pose le curseur d'edition : la ou la ligne a la souris le
    /// montre (Cedric, 2026-11-22 : « le clic doit suivre le snap montre par
    /// la ligne »). Calee par le magnetisme de REAPER selon les reglages de
    /// la ligne (Maj ou Ctrl la liberent) ; sans ligne, cale sauf Maj.
    pub fn cursor_q(&self, x: f32, m: Mods) -> f64 {
        let q = self.qn_of(x);
        let snaps = if self.guide.on {
            self.guide.snaps(m)
        } else {
            !m.shift
        };
        if snaps { self.snap_item(q) } else { q }.max(0.0)
    }

    fn down(&mut self, x: f32, y: f32, b: Button, dbl: bool, mods: Mods) -> Option<Act> {
        let h = self.hit(x, y);
        self.dirty = true;
        if b == Button::Right {
            return match h {
                Hit::Header(t)
                | Hit::Arm(t)
                | Hit::Input(t)
                | Hit::Monitor(t)
                | Hit::Mute(t)
                | Hit::Solo(t)
                | Hit::Tuning(t) => Some(Act::TrackMenu { t, x, y }),
                _ if y < self.lay.bar_h => Some(Act::ViewMenu { x, y }),
                _ => None,
            };
        }
        if b != Button::Left {
            return None;
        }
        match h {
            Hit::Note { t, i, n } => {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: Some(n),
                }));
                let qn0 = self.item(t, i)?.doc.get(n)?.qn0;
                self.drag = Some(Drag::Note {
                    t,
                    i,
                    n,
                    qn0,
                    x0: x,
                    moved: false,
                });
                None
            }
            Hit::Ghost { t, i, g } => self.keep_ghost(t, i, g),
            Hit::TabLine { t, i, s } => {
                if dbl {
                    let q = self.qn_of(x);
                    self.add_note(t, i, q, s, 0)
                } else {
                    self.select(Some(Sel {
                        t,
                        i: Some(i),
                        n: None,
                    }));
                    Some(Act::Seek(self.cursor_q(x, mods)))
                }
            }
            Hit::ItemEdge { t, i, left } => {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: None,
                }));
                let it = self.item(t, i)?;
                let q0 = if left { it.qn0 } else { it.qn1 };
                self.drag = Some(Drag::Trim {
                    t,
                    i,
                    left,
                    qp: self.qn_of(x),
                    q0,
                    q: q0,
                    moved: false,
                });
                None
            }
            Hit::ItemFade { t, i, fade_in } => {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: None,
                }));
                let it = self.item(t, i)?;
                let len = if fade_in { it.fade_in } else { it.fade_out };
                self.drag = Some(Drag::Fade { t, i, fade_in, len });
                None
            }
            Hit::Item { t, i, lane } => {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: None,
                }));
                if dbl && lane == Lane::Midi {
                    return Some(Act::OpenMidi { t, i });
                }
                // La prise (son audio, son MIDI) se glisse ; un simple clic
                // place le curseur. La couche cliquee prend le focus.
                if matches!(lane, Lane::Audio | Lane::Midi) {
                    self.focus_lane = lane;
                    let q = self.item(t, i)?.qn0;
                    self.drag = Some(Drag::Item {
                        t,
                        i,
                        lane,
                        x0: x,
                        qp: self.qn_of(x),
                        moved: false,
                        qn0: q,
                        to: t,
                        copy: false,
                    });
                    return None;
                }
                Some(Act::Seek(self.cursor_q(x, mods)))
            }
            Hit::Lane { t, .. } => {
                self.select(Some(Sel {
                    t,
                    i: None,
                    n: None,
                }));
                Some(Act::Seek(self.cursor_q(x, mods)))
            }
            Hit::Ruler => {
                let q = self.cursor_q(x, mods);
                self.range = None;
                self.drag = Some(Drag::Range { q0: q });
                Some(Act::Seek(q))
            }
            Hit::Header(t) => {
                self.select(Some(Sel {
                    t,
                    i: None,
                    n: None,
                }));
                // L'en-tete se glisse : la guitare change de place.
                self.drag = Some(Drag::TrackMove {
                    t,
                    y0: y,
                    to: t,
                    moved: false,
                });
                None
            }
            Hit::LaneToggle { t, lane } => self.toggle_lane(t, lane),
            Hit::Arm(t) => Some(Act::Arm { t }),
            Hit::Input(t) => Some(Act::InputMenu { t, x, y }),
            Hit::Monitor(t) => Some(Act::Monitor { t }),
            Hit::Mute(t) => Some(Act::Mute { t }),
            Hit::Solo(t) => Some(Act::Solo { t }),
            Hit::Fader { t, tab } => {
                if dbl {
                    let hd = &mut self.tracks.get_mut(t)?.head;
                    if tab {
                        hd.tab_vol_db = 0.0;
                    } else {
                        hd.vol_db = 0.0;
                    }
                    return Some(Act::Volume { t, tab, db: 0.0 });
                }
                self.drag = Some(Drag::Fader { t, tab });
                self.set_fader(t, tab, x)
            }
            Hit::SoundPick { t, s } => {
                let sy = self.tracks.get_mut(t)?.head.synth.as_mut();
                if let Some(sy) = sy {
                    sy.sound = s;
                }
                Some(Act::Sound { t, s })
            }
            Hit::Tuning(t) => {
                let hd = &mut self.tracks.get_mut(t)?.head;
                hd.tuning = (hd.tuning + 1) % TUNINGS.len();
                let k = hd.tuning;
                for it in &mut self.tracks[t].items {
                    it.detect.tuning = k;
                }
                Some(Act::Retune { t })
            }
            Hit::Listen(l) => {
                self.listen = l;
                Some(Act::Listen)
            }
            Hit::Split => {
                self.split = !self.split;
                Some(Act::Listen)
            }
            Hit::Follow => {
                self.follow = !self.follow;
                None
            }
            Hit::Auto => {
                self.auto = !self.auto;
                Some(Act::Auto)
            }
            Hit::Transcribe { t, i } => {
                self.select(Some(Sel {
                    t,
                    i: Some(i),
                    n: None,
                }));
                Some(Act::Transcribe { t, i })
            }
            Hit::AddTrack => Some(Act::AddTrack),
            Hit::Take(k) => {
                let (t, i) = self.sel_item()?;
                Some(Act::SetTake { t, i, k })
            }
            Hit::Slider(k) => {
                self.drag = Some(Drag::Slider(k));
                self.set_slider(k, x)
            }
            Hit::SynthSlider(k) => {
                self.drag = Some(Drag::SynthSlider(k));
                self.set_synth_slider(k, x)
            }
            Hit::SynthMono => {
                let t = self.sel_track()?;
                let sy = self.tracks.get_mut(t)?.head.synth.as_mut()?;
                sy.mono = !sy.mono;
                let v = if sy.mono { 1.0 } else { 0.0 };
                Some(Act::Synth { t, p: 3, v })
            }
            Hit::Check(k) => {
                let (t, i) = self.sel_item()?;
                let range = self.range;
                let it = self.item_mut(t, i)?;
                if k == 0 {
                    it.detect.legato = !it.detect.legato;
                    Some(Act::Detect { t, i, range })
                } else {
                    it.detect.ghosts = !it.detect.ghosts;
                    Some(Act::Detect { t, i, range: None })
                }
            }
            Hit::Grid(g) => {
                let (t, i) = self.sel_item()?;
                let it = self.item_mut(t, i)?;
                if it.detect.grid != g {
                    it.detect.grid = g;
                    it.touch();
                    Some(Act::Detect { t, i, range: None })
                } else {
                    None
                }
            }
            Hit::KeepGhosts => self.keep_ghosts(),
            Hit::Retranscribe => {
                let (t, i) = self.sel_item()?;
                if mods.shift && self.item(t, i)?.state == Some(State::Ready) {
                    return Some(Act::TranscribeFresh { t, i });
                }
                Some(Act::Transcribe { t, i })
            }
            Hit::DeleteItem => {
                let (t, i) = self.sel_item()?;
                Some(Act::DeleteItem { t, i })
            }
            Hit::Export => {
                let (t, i) = self.sel_item()?;
                Some(Act::Export { t, i })
            }
            Hit::VScroll => {
                self.drag = Some(Drag::VScroll {
                    y0: y,
                    v0: self.vscroll,
                });
                None
            }
            Hit::HScroll => {
                // Hors du curseur : le curseur vient sous la souris.
                let (tx, tw) = self.hthumb();
                if x < tx || x > tx + tw {
                    let end = self.end_qn();
                    let k = end / self.lay.sw.max(1.0) as f64;
                    self.scroll = ((x - self.lay.sx - tw * 0.5) as f64 * k).clamp(0.0, end);
                    self.unfollow();
                }
                self.drag = Some(Drag::HScroll {
                    x0: x,
                    s0: self.scroll,
                });
                None
            }
            Hit::Help => None,
            Hit::None => {
                self.select(None);
                None
            }
        }
    }

    /// La guitare sous l'ordonnee `y` (zone des pistes).
    pub fn track_at(&self, y: f32) -> Option<usize> {
        let l = self.lay;
        let mut yy = l.top + l.ruler_h + 1.0 - self.vscroll;
        for t in 0..self.tracks.len() {
            let h = draw::track_h(&self.tracks[t], self.scale, self.vzoom)
                + (draw::TRACK_SEP * self.scale).round();
            if y >= yy && y < yy + h {
                return Some(t);
            }
            yy += h;
        }
        None
    }

    /// Le geste en cours sur une prise : (piste, item, debut vise, guitare,
    /// copie).
    pub(crate) fn item_drag(&self) -> Option<(usize, usize, f64, usize, bool)> {
        match self.drag {
            Some(Drag::Item {
                t,
                i,
                moved: true,
                qn0,
                to,
                copy,
                ..
            }) => Some((t, i, qn0, to, copy)),
            _ => None,
        }
    }

    /// Le curseur de la barre horizontale : (x, largeur).
    pub fn hthumb(&self) -> (f32, f32) {
        let l = self.lay;
        let end = self.end_qn().max(1e-6);
        let tw = ((self.view_qn() / end) as f32 * l.sw).clamp((24.0 * self.scale).min(l.sw), l.sw);
        let room = (l.sw - tw).max(0.0);
        let maxs = (end - self.view_qn()).max(1e-6);
        let t = (self.scroll / maxs).clamp(0.0, 1.0) as f32;
        (l.sx + room * t, tw)
    }

    /// Montre ou cache une couche d'une piste (le menu de la piste). Une
    /// piste garde au moins une couche.
    pub fn toggle_lane(&mut self, t: usize, lane: Lane) -> Option<Act> {
        let l = &mut self.tracks.get_mut(t)?.head.lanes;
        l.0[lane as usize] = !l.0[lane as usize];
        if !l.0.iter().any(|&b| b) {
            l.0[lane as usize] = true;
            return None;
        }
        self.dirty = true;
        Some(Act::Lanes { t })
    }

    /// La cible sous la souris (pour l'infobulle).
    pub fn hovered(&self) -> Hit {
        self.hover
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Le texte de l'infobulle d'une cible : l'ecran reste sans phrases,
    /// l'explication vient au survol.
    pub fn label(&self, h: Hit) -> Option<String> {
        let db = |v: f32| {
            if v <= -60.0 {
                "-inf dB".to_string()
            } else {
                format!("{v:+.1} dB")
            }
        };
        Some(match h {
            Hit::Note { t, i, n } => {
                let it = self.item(t, i)?;
                let nt = it.doc.get(n)?;
                let bar_qn = self.bar_qn();
                let bar = (nt.qn0 / bar_qn).floor();
                let beat = nt.qn0 - bar * bar_qn;
                let names = [
                    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                ];
                let nstr = it.detect.tuning().strings.len();
                format!(
                    "{}{} \u{00b7} string {}, fret {}{}\nBar {} \u{00b7} beat {:.2}",
                    names[nt.pitch.rem_euclid(12) as usize],
                    nt.pitch.div_euclid(12) - 1,
                    nstr - nt.string,
                    nt.fret,
                    match nt.tech {
                        Tech::Hammer => " \u{00b7} hammer-on",
                        Tech::Pull => " \u{00b7} pull-off",
                        Tech::Plain => "",
                    },
                    bar as i64 + 1,
                    beat + 1.0,
                )
            }
            Hit::Ghost { .. } => "Ghost note, just under the threshold\nClick: keep it".into(),
            Hit::TabLine { .. } => "Double-click: add a note".into(),
            Hit::Item { t, i, lane } => {
                let it = self.item(t, i)?;
                match lane {
                    Lane::Midi => format!(
                        "{}\nDrag: move (Shift: no snap, Ctrl: copy) \u{00b7} double-click: editor",
                        it.name
                    ),
                    Lane::Audio => format!("{}\nDrag: move (Shift: no snap, Ctrl: copy)", it.name),
                    _ => it.name.clone(),
                }
            }
            Hit::Arm(_) => "Arm for recording (R)".into(),
            Hit::Input(_) => "Input".into(),
            Hit::Monitor(_) => "Monitor the input".into(),
            Hit::Mute(_) => "Mute".into(),
            Hit::Solo(_) => "Solo".into(),
            Hit::Fader { t, tab } => {
                let hd = &self.tracks.get(t)?.head;
                if tab {
                    format!("Tab {}\nDouble-click: 0 dB", db(hd.tab_vol_db))
                } else {
                    format!("Guitar {}\nDouble-click: 0 dB", db(hd.vol_db))
                }
            }
            Hit::SoundPick { s, .. } => format!("Tab sound: {}", s.name()),
            Hit::Tuning(_) => "Tuning (click: next)".into(),
            Hit::Header(_) => {
                "Drag: move the guitar \u{00b7} right-click: lanes, rename, delete".into()
            }
            Hit::LaneToggle { lane, .. } => format!(
                "{} lane",
                match lane {
                    Lane::Audio => "Audio",
                    Lane::Midi => "MIDI",
                    Lane::Score => "Score",
                    Lane::Tab => "Tab",
                }
            ),
            Hit::Listen(Listen::Original) => "Hear the recording".into(),
            Hit::Listen(Listen::Tab) => "Hear the tab".into(),
            Hit::Listen(Listen::Both) => "Hear both".into(),
            Hit::Split => "Original left, tab right".into(),
            Hit::Follow => "Follow playback".into(),
            Hit::Auto => format!(
                "Transcribe new takes by themselves ({})",
                if self.auto {
                    "on"
                } else {
                    "off: click a take's wand"
                }
            ),
            Hit::Transcribe { .. } => "Transcribe this take".into(),
            Hit::AddTrack => "New guitar track".into(),
            Hit::Help => HELP.into(),
            Hit::Take(k) => format!("Take {}", k + 1),
            Hit::Slider(0) => "Onset sensitivity\nLower: more notes caught, and more noise".into(),
            Hit::Slider(1) => "Note sensitivity\nLower: softer notes kept".into(),
            Hit::Slider(_) => "Shortest note kept".into(),
            Hit::Check(0) => "Legato: notes without a new attack".into(),
            Hit::Check(_) => "Ghosts: notes just under the threshold".into(),
            Hit::Grid(_) => "Rhythm grid".into(),
            Hit::KeepGhosts => "Keep the ghost notes".into(),
            Hit::Retranscribe => {
                let (t, i) = self.sel_item()?;
                if self.item(t, i)?.state == Some(State::Ready) {
                    "Transcribe again (your edited notes stay)\nShift: start over".into()
                } else {
                    "Transcribe".into()
                }
            }
            Hit::DeleteItem => "Delete the take and its tab (Del)".into(),
            Hit::Export => "Export MusicXML".into(),
            Hit::SynthSlider(1) => "Tone".into(),
            Hit::SynthSlider(_) => "Sustain".into(),
            Hit::SynthMono => "One note per string".into(),
            Hit::Ruler => {
                "Click: play from here \u{00b7} drag: passage\nMiddle drag: move, up/down: zoom"
                    .into()
            }
            _ => return None,
        })
    }

    fn key(&mut self, k: u32, m: Mods) -> Option<Act> {
        self.dirty = true;
        match k {
            vk::SPACE => Some(Act::Play),
            0x5A if m.ctrl => Some(Act::Undo),   // Ctrl+Z
            0x52 if m.ctrl => Some(Act::Record), // Ctrl+R
            0x52 => {
                let t = self.sel_track()?;
                Some(Act::Arm { t })
            }
            vk::ESCAPE => {
                self.select(None);
                self.range = None;
                None
            }
            vk::LEFT | vk::RIGHT => {
                let s = self.sel?;
                let i = s.i?;
                let it = self.item(s.t, i)?;
                if it.doc.is_empty() {
                    return None;
                }
                let n = it.doc.len();
                let next = match (s.n, k == vk::RIGHT) {
                    (None, _) => {
                        let sc = self.scroll;
                        it.doc.iter().position(|t| t.qn0 >= sc).unwrap_or(0)
                    }
                    (Some(j), true) => (j + 1).min(n - 1),
                    (Some(j), false) => j.saturating_sub(1),
                };
                self.select(Some(Sel {
                    t: s.t,
                    i: Some(i),
                    n: Some(next),
                }));
                None
            }
            vk::UP => self.change_string(1),
            vk::DOWN => self.change_string(-1),
            vk::DELETE | vk::BACK => {
                let s = self.sel?;
                match (s.i, s.n) {
                    (Some(_), Some(_)) => self.delete_note(),
                    (Some(i), None) => Some(Act::DeleteItem { t: s.t, i }),
                    _ => None,
                }
            }
            0x48 if !m.ctrl => self.set_tech(Tech::Hammer), // H
            0x50 if !m.ctrl => self.set_tech(Tech::Pull),   // P
            0x30..=0x39 | 0x60..=0x69 => {
                let d = if k >= 0x60 { k - 0x60 } else { k - 0x30 } as i32;
                // Deux chiffres a moins de 0,8 s : une case a deux chiffres.
                let fret = match self.digit {
                    Some((a, t)) if t.elapsed().as_secs_f64() < 0.8 && a * 10 + d <= 24 => {
                        a * 10 + d
                    }
                    _ => d,
                };
                self.digit = Some((fret, Instant::now()));
                self.set_fret(fret)
            }
            _ => None,
        }
    }

    /// Les touches que la fenetre traite (les autres vont a REAPER).
    pub fn wants_key(&self, k: u32, m: Mods) -> bool {
        let note = self.sel.is_some_and(|s| s.n.is_some());
        let item = self.sel.is_some_and(|s| s.i.is_some());
        match k {
            vk::SPACE | vk::ESCAPE => true,
            0x5A | 0x52 if m.ctrl => true,
            0x52 => !m.ctrl && !m.alt,
            vk::LEFT | vk::RIGHT => item,
            vk::UP | vk::DOWN => note,
            vk::DELETE | vk::BACK => item,
            0x48 | 0x50 => note && !m.ctrl,
            0x30..=0x39 | 0x60..=0x69 => note,
            _ => false,
        }
    }

    /// Ce que dit la ligne d'etat de la note choisie.
    pub fn sel_text(&self) -> Option<String> {
        let s = self.sel?;
        let it = self.item(s.t, s.i?)?;
        let n = it.doc.get(s.n?)?;
        let bar_qn = self.bar_qn();
        let bar = (n.qn0 / bar_qn).floor();
        let beat = n.qn0 - bar * bar_qn;
        let names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let nstr = it.detect.tuning().strings.len();
        Some(format!(
            "Bar {} · {:.1} · {}{} · string {}, fret {}{}",
            bar as i64 + 1,
            beat + 1.0,
            names[n.pitch.rem_euclid(12) as usize],
            n.pitch.div_euclid(12) - 1,
            nstr - n.string,
            n.fret,
            match n.tech {
                Tech::Hammer => " · hammer-on",
                Tech::Pull => " · pull-off",
                Tech::Plain => "",
            }
        ))
    }
}
