//! Les icones : Lucide (lucide.dev, licence ISC, copyright Lucide Icons and
//! Contributors), tracees en vecteur par le moteur, a toute taille.
//!
//! Une icone est un trait rond d'epaisseur 2 dans un repere 24 x 24 (c'est le
//! dessin de Lucide, nos traits ont les memes bouts et jointures ronds). Elle
//! est rasterisee UNE fois par taille en masque de couverture, puis teintee a
//! chaque pose.
//!
//! Le masque est cuit en grand (6 a 16 fois) puis reduit par moyenne : la
//! couverture de chaque pixel est l'aire vraie du trait. Le rasteriseur, lui,
//! unit les morceaux d'un trait (un rectangle par segment, un disque par
//! sommet) en ADDITIONNANT leurs couvertures : la ou deux morceaux se
//! recouvrent sur un bord, le pixel comptait deux fois. A la taille d'une
//! icone, c'etaient des coins en pate, des anneaux remplis, des courbes
//! bosselees et trop grasses. Cuit en grand, l'erreur tombe sous le seizieme
//! de pixel.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use cp_gfx::path::Seg;
use cp_gfx::{Canvas, Color, Path, Pt, Scratch, Surface, pt, rgb};

use crate::svg::{Xf, hint, path_data, shapes};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Folder,
    FolderOpen,
    FolderSearch,
    File,
    FileMusic,
    Star,
    Search,
    Play,
    Stop,
    Pause,
    Repeat,
    Volume,
    VolumeX,
    ChevronRight,
    ChevronDown,
    X,
    Shuffle,
    Database,
    HardDrive,
    AudioLines,
    Music,
    Refresh,
    ArrowUp,
    House,
    List,
    SkipBack,
    Circle,
    Metronome,
    Plus,
    Guitar,
    Trash,
    Download,
    Headphones,
    Locate,
    Help,
    Magnet,
    Palette,
    Fold,
    Wand,
    Piano,
    Reverse,
    Normalize,
    Bars,
    /// Le jeu libre (sans mesure) : une onde.
    Free,
    /// La timeline de l'item seul : un item entre crochets.
    Isolate,
    /// La Session : une grille de cases.
    Grid,
    /// Une piste neuve.
    SquarePlus,
    /// L'overdub : un cercle et un plus.
    CirclePlus,
    /// Capturer une scene : un appareil photo.
    Camera,
    /// La panique : un eclair.
    Zap,
    /// Le de du navigateur d'effets.
    Dices,
    /// Les recents.
    History,
    /// Les plus utilises.
    TrendingUp,
    /// Allumer, contourner un effet.
    Power,
    /// Les reglages d'une vue (trois points).
    Ellipsis,
    /// Une chaine d'effets.
    Link,
    /// Un onglet, une etiquette.
    Tag,
    /// Le navigateur d'effets : une piece de puzzle.
    Puzzle,
    /// Tenir une piste (FX Constellation).
    Lock,
    LockOpen,
    /// Enregistrer (une disquette).
    Save,
    /// Les reglages (deux curseurs).
    Settings,
    /// Ouvrir la fenetre d'un effet.
    Maximize,
    /// Les axes X / Y.
    Move,
    /// Les portees.
    Ruler,
    /// Les bases.
    Gauge,
    ChevronLeft,
}

impl Icon {
    pub fn svg(self) -> &'static str {
        match self {
            Icon::Folder => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />"#
            }
            Icon::FolderOpen => {
                r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />"#
            }
            Icon::FolderSearch => {
                r#"<path d="M10.7 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v4.1" /><path d="m21 21-1.9-1.9" /><circle cx="17" cy="17" r="3" />"#
            }
            Icon::File => {
                r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" /><path d="M14 2v5a1 1 0 0 0 1 1h5" />"#
            }
            Icon::FileMusic => {
                r#"<path d="M11.65 22H18a2 2 0 0 0 2-2V8a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 14 2H6a2 2 0 0 0-2 2v10.35" /><path d="M14 2v5a1 1 0 0 0 1 1h5" /><path d="M8 20v-7l3 1.474" /><circle cx="6" cy="20" r="2" />"#
            }
            Icon::Star => {
                r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />"#
            }
            Icon::Search => r#"<path d="m21 21-4.34-4.34" /><circle cx="11" cy="11" r="8" />"#,
            Icon::Play => {
                r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z" />"#
            }
            Icon::Stop => r#"<rect width="18" height="18" x="3" y="3" rx="2" />"#,
            Icon::Pause => {
                r#"<rect x="14" y="3" width="5" height="18" rx="1" /><rect x="5" y="3" width="5" height="18" rx="1" />"#
            }
            Icon::Repeat => {
                r#"<path d="m17 2 4 4-4 4" /><path d="M3 11v-1a4 4 0 0 1 4-4h14" /><path d="m7 22-4-4 4-4" /><path d="M21 13v1a4 4 0 0 1-4 4H3" />"#
            }
            Icon::Volume => {
                r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z" /><path d="M16 9a5 5 0 0 1 0 6" /><path d="M19.364 18.364a9 9 0 0 0 0-12.728" />"#
            }
            Icon::VolumeX => {
                r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z" /><line x1="22" x2="16" y1="9" y2="15" /><line x1="16" x2="22" y1="9" y2="15" />"#
            }
            Icon::ChevronRight => r#"<path d="m9 18 6-6-6-6" />"#,
            Icon::ChevronDown => r#"<path d="m6 9 6 6 6-6" />"#,
            Icon::X => r#"<path d="M18 6 6 18" /><path d="m6 6 12 12" />"#,
            Icon::Shuffle => {
                r#"<path d="m18 14 4 4-4 4" /><path d="m18 2 4 4-4 4" /><path d="M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22" /><path d="M2 6h1.972a4 4 0 0 1 3.6 2.2" /><path d="M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45" />"#
            }
            Icon::Database => {
                r#"<ellipse cx="12" cy="5" rx="9" ry="3" /><path d="M3 5V19A9 3 0 0 0 21 19V5" /><path d="M3 12A9 3 0 0 0 21 12" />"#
            }
            Icon::HardDrive => {
                r#"<path d="M10 16h.01" /><path d="M2.212 11.577a2 2 0 0 0-.212.896V18a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-5.527a2 2 0 0 0-.212-.896L18.55 5.11A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" /><path d="M21.946 12.013H2.054" /><path d="M6 16h.01" />"#
            }
            Icon::AudioLines => {
                r#"<path d="M2 10v3" /><path d="M6 6v11" /><path d="M10 3v18" /><path d="M14 8v7" /><path d="M18 5v13" /><path d="M22 10v3" />"#
            }
            Icon::Music => {
                r#"<path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />"#
            }
            Icon::Refresh => {
                r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /><path d="M8 16H3v5" />"#
            }
            Icon::ArrowUp => r#"<path d="m5 12 7-7 7 7" /><path d="M12 19V5" />"#,
            Icon::House => {
                r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" /><path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />"#
            }
            Icon::List => {
                r#"<path d="M3 5h.01" /><path d="M3 12h.01" /><path d="M3 19h.01" /><path d="M8 5h13" /><path d="M8 12h13" /><path d="M8 19h13" />"#
            }
            Icon::SkipBack => {
                r#"<path d="M17.971 4.285A2 2 0 0 1 21 6v12a2 2 0 0 1-3.029 1.715l-9.997-5.998a2 2 0 0 1-.003-3.432z" /><path d="M3 20V4" />"#
            }
            Icon::Circle => r#"<circle cx="12" cy="12" r="10" />"#,
            Icon::Metronome => {
                r#"<path d="M12 11.4V9.1" /><path d="m12 17 6.59-6.59" /><path d="m15.05 5.7-.218-.691a3 3 0 0 0-5.663 0L4.418 19.695A1 1 0 0 0 5.37 21h13.253a1 1 0 0 0 .951-1.31L18.45 16.2" /><circle cx="20" cy="9" r="2" />"#
            }
            Icon::Plus => r#"<path d="M5 12h14" /><path d="M12 5v14" />"#,
            Icon::Guitar => {
                r#"<path d="m11.9 12.1 4.514-4.514" /><path d="M20.1 2.3a1 1 0 0 0-1.4 0l-1.114 1.114A2 2 0 0 0 17 4.828v1.344a2 2 0 0 1-.586 1.414A2 2 0 0 1 17.828 7h1.344a2 2 0 0 0 1.414-.586L21.7 5.3a1 1 0 0 0 0-1.4z" /><path d="m6 16 2 2" /><path d="M8.23 9.85A3 3 0 0 1 11 8a5 5 0 0 1 5 5 3 3 0 0 1-1.85 2.77l-.92.38A2 2 0 0 0 12 18a4 4 0 0 1-4 4 6 6 0 0 1-6-6 4 4 0 0 1 4-4 2 2 0 0 0 1.85-1.23z" />"#
            }
            Icon::Trash => {
                r#"<path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />"#
            }
            Icon::Download => {
                r#"<path d="M12 15V3" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" />"#
            }
            Icon::Headphones => {
                r#"<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />"#
            }
            Icon::Locate => {
                r#"<line x1="2" x2="5" y1="12" y2="12" /><line x1="19" x2="22" y1="12" y2="12" /><line x1="12" x2="12" y1="2" y2="5" /><line x1="12" x2="12" y1="19" y2="22" /><circle cx="12" cy="12" r="7" /><circle cx="12" cy="12" r="3" />"#
            }
            Icon::Help => {
                r#"<circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" /><path d="M12 17h.01" />"#
            }
            Icon::Magnet => {
                r#"<path d="m12 15 4 4" /><path d="M2.352 10.648a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.029-6.029a1 1 0 1 1 3 3l-6.029 6.029a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.365-6.367A1 1 0 0 0 8.716 4.282z" /><path d="m5 8 4 4" />"#
            }
            Icon::Palette => {
                r#"<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z" /><circle cx="13.5" cy="6.5" r=".5" /><circle cx="17.5" cy="10.5" r=".5" /><circle cx="6.5" cy="12.5" r=".5" /><circle cx="8.5" cy="7.5" r=".5" />"#
            }
            Icon::Fold => {
                r#"<path d="M12 22v-6" /><path d="M12 8V2" /><path d="M4 12H2" /><path d="M10 12H8" /><path d="M16 12h-2" /><path d="M22 12h-2" /><path d="m15 19-3-3-3 3" /><path d="m15 5-3 3-3-3" />"#
            }
            Icon::Wand => {
                r#"<path d="M15 4V2" /><path d="M15 16v-2" /><path d="M8 9h2" /><path d="M20 9h2" /><path d="M17.8 11.8 19 13" /><path d="M15 9h.01" /><path d="M17.8 6.2 19 5" /><path d="m3 21 9-9" /><path d="M12.2 6.2 11 5" />"#
            }
            Icon::Piano => {
                r#"<path d="M18.5 8c-1.4 0-2.6-.8-3.2-2A6.87 6.87 0 0 0 2 9v11a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-8.5C22 9.6 20.4 8 18.5 8" /><path d="M2 14h20" /><path d="M6 14v4" /><path d="M10 14v4" /><path d="M14 14v4" /><path d="M18 14v4" />"#
            }
            Icon::Reverse => {
                r#"<path d="M8 3 4 7l4 4" /><path d="M4 7h16" /><path d="m16 21 4-4-4-4" /><path d="M20 17H4" />"#
            }
            Icon::Normalize => {
                r#"<path d="M12 2v20" /><path d="m8 18 4 4 4-4" /><path d="m8 6 4-4 4 4" />"#
            }
            Icon::Bars => r#"<path d="M5 21v-6" /><path d="M12 21V3" /><path d="M19 21V9" />"#,
            Icon::Free => r#"<path d="M2 12c2.5-6 5-6 7.5 0s5 6 7.5 0c1.2-3 2.5-4.5 5-4.5" />"#,
            Icon::Grid => {
                r#"<rect width="7" height="7" x="3" y="3" rx="1" /><rect width="7" height="7" x="14" y="3" rx="1" /><rect width="7" height="7" x="14" y="14" rx="1" /><rect width="7" height="7" x="3" y="14" rx="1" />"#
            }
            Icon::SquarePlus => {
                r#"<rect width="18" height="18" x="3" y="3" rx="2" /><path d="M8 12h8" /><path d="M12 8v8" />"#
            }
            Icon::CirclePlus => {
                r#"<circle cx="12" cy="12" r="10" /><path d="M8 12h8" /><path d="M12 8v8" />"#
            }
            Icon::Camera => {
                r#"<path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z" /><circle cx="12" cy="13" r="3" />"#
            }
            Icon::Zap => {
                r#"<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />"#
            }
            Icon::Isolate => {
                r#"<path d="M8 3H5v18h3" /><path d="M16 3h3v18h-3" /><rect x="9" y="9" width="6" height="6" rx="1" />"#
            }
            Icon::Dices => {
                r#"<rect width="12" height="12" x="2" y="10" rx="2" ry="2" /><path d="m17.92 14 3.5-3.5a2.24 2.24 0 0 0 0-3l-5-4.92a2.24 2.24 0 0 0-3 0L10 6" /><circle cx="6" cy="18" r="0.9" /><circle cx="10" cy="14" r="0.9" /><circle cx="15" cy="6" r="0.9" /><circle cx="18" cy="9" r="0.9" />"#
            }
            Icon::History => {
                r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /><path d="M12 7v5l4 2" />"#
            }
            Icon::TrendingUp => {
                r#"<polyline points="22 7 13.5 15.5 8.5 10.5 2 17" /><polyline points="16 7 22 7 22 13" />"#
            }
            Icon::Power => r#"<path d="M12 2v10" /><path d="M18.4 6.6a9 9 0 1 1-12.77.04" />"#,
            Icon::Ellipsis => {
                r#"<circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" /><circle cx="5" cy="12" r="1.6" />"#
            }
            Icon::Link => {
                r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" /><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />"#
            }
            Icon::Puzzle => {
                r#"<path d="M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z" />"#
            }
            Icon::Tag => {
                r#"<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z" /><circle cx="7.5" cy="7.5" r="1.2" />"#
            }
            Icon::Lock => {
                r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />"#
            }
            Icon::LockOpen => {
                r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 9.9-1" />"#
            }
            Icon::Save => {
                r#"<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" /><path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7" /><path d="M7 3v4a1 1 0 0 0 1 1h7" />"#
            }
            Icon::Settings => {
                r#"<path d="M20 7h-9" /><path d="M14 17H5" /><circle cx="17" cy="17" r="3" /><circle cx="7" cy="7" r="3" />"#
            }
            Icon::Maximize => {
                r#"<path d="M15 3h6v6" /><path d="m21 3-7 7" /><path d="m3 21 7-7" /><path d="M9 21H3v-6" />"#
            }
            Icon::Move => {
                r#"<path d="M12 2v20" /><path d="m15 19-3 3-3-3" /><path d="m19 9 3 3-3 3" /><path d="M2 12h20" /><path d="m5 9-3 3 3 3" /><path d="m9 5 3-3 3 3" />"#
            }
            Icon::Ruler => {
                r#"<path d="M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z" /><path d="m14.5 12.5 2-2" /><path d="m11.5 9.5 2-2" /><path d="m8.5 6.5 2-2" /><path d="m17.5 15.5 2-2" />"#
            }
            Icon::Gauge => r#"<path d="m12 14 4-4" /><path d="M3.34 19a10 10 0 1 1 17.32 0" />"#,
            Icon::ChevronLeft => r#"<path d="m15 18-6-6 6-6" />"#,
        }
    }
}

/// Un masque de couverture carre : son cote en px et ses valeurs (0..255).
pub type MaskPx = (i32, Vec<u8>);

/// Les masques deja rasterises, par (icone, taille en px, plein), et les
/// glyphes donnes en SVG (la palette), par (empreinte, taille).
#[derive(Default)]
pub struct IconCache {
    masks: HashMap<(Icon, u16, bool), MaskPx>,
    glyphs: HashMap<(u64, u16), MaskPx>,
    scratch: Scratch,
}

/// Le chemin agrandi `k` fois.
fn scaled(p: &Path, k: f32) -> Path {
    let s = |q: Pt| pt(q.x * k, q.y * k);
    Path {
        segs: p
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg::Move(q) => Seg::Move(s(q)),
                Seg::Line(q) => Seg::Line(s(q)),
                Seg::Quad(c, q) => Seg::Quad(s(c), s(q)),
                Seg::Cubic(a, b, q) => Seg::Cubic(s(a), s(b), s(q)),
                Seg::Close => Seg::Close,
            })
            .collect(),
    }
}

/// Le masque d'un dessin de `n` x `n` px : `fill` rempli, `stroke` trace.
/// Les chemins sont en px exacts ; `sw24` est l'epaisseur du trait sur la
/// grille de 24.
fn render(stroke: &Path, fill: &Path, n: i32, sw24: f32, sc: &mut Scratch) -> MaskPx {
    let n = n.max(1);
    let k = n as f32 / 24.0;
    // En petit, une epaisseur fractionnaire (1,25 px a 15 px) grise deux
    // pixels : on trace en pixels ENTIERS, les ancres calees sur la grille.
    // Au-dela de 32 px, le trace exact est deja net.
    let (w, grid) = if n <= 32 {
        let w = (sw24 * k).round().max(1.0);
        (w, Some(if w as i32 % 2 == 1 { 0.5 } else { 0.0 }))
    } else {
        (sw24 * k, None)
    };
    // Environ 256 px de cote en grand : le cout reste borne a toute taille.
    let ss = (256 / n).clamp(6, 16);
    let big = n * ss;
    let prep = |p: &Path| match grid {
        Some(g) => scaled(&hint(p, g), ss as f32),
        None => scaled(p, ss as f32),
    };
    let mut s = Surface::new(big, big);
    {
        let mut cv = Canvas::new(&mut s, sc);
        if !fill.segs.is_empty() {
            cv.fill_path(&prep(fill), rgb(255, 255, 255));
        }
        if !stroke.segs.is_empty() {
            cv.stroke_path(&prep(stroke), w * ss as f32, rgb(255, 255, 255));
        }
    }
    let mut acc = vec![0u32; (n * n) as usize];
    for y in 0..big {
        let row = &s.px[(y * big) as usize..((y + 1) * big) as usize];
        let out = &mut acc[((y / ss) * n) as usize..((y / ss + 1) * n) as usize];
        for (x, &px) in row.iter().enumerate() {
            out[x / ss as usize] += px >> 24;
        }
    }
    let d = (ss * ss) as u32;
    (n, acc.iter().map(|&v| ((v + d / 2) / d) as u8).collect())
}

impl IconCache {
    /// Le masque de l'icone a `size` px (carre). `filled` : rempli au lieu de
    /// trace (une etoile de favori).
    pub fn mask(&mut self, icon: Icon, size: u16, filled: bool) -> &MaskPx {
        let sc = &mut self.scratch;
        self.masks.entry((icon, size, filled)).or_insert_with(|| {
            let n = size.max(1) as i32;
            let mut p = Path::new();
            let xf = Xf {
                k: n as f32 / 24.0,
                ox: 0.0,
                oy: 0.0,
            };
            shapes(icon.svg(), xf, &mut p);
            let empty = Path::new();
            render(&p, if filled { &p } else { &empty }, n, 2.0, sc)
        })
    }

    /// Pose l'icone, coin haut-gauche en (x, y), teinte `c`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        icon: Icon,
        x: f32,
        y: f32,
        size: u16,
        c: Color,
        filled: bool,
    ) {
        let (n, m) = self.mask(icon, size, filled);
        let n = *n;
        cv.draw_mask(m, n, n, x.round() as i32, y.round() as i32, c);
    }

    /// Le masque d'un glyphe donne en donnees SVG (`d`) sur une grille de
    /// 24 : `stroke` trace en `sw24`, `fill` rempli.
    pub fn glyph_mask(&mut self, stroke: &str, fill: &str, sw24: f32, size: u16) -> &MaskPx {
        let mut h = std::hash::DefaultHasher::new();
        (stroke, fill, sw24.to_bits()).hash(&mut h);
        let sc = &mut self.scratch;
        self.glyphs.entry((h.finish(), size)).or_insert_with(|| {
            let n = size.max(1) as i32;
            let xf = Xf {
                k: n as f32 / 24.0,
                ox: 0.0,
                oy: 0.0,
            };
            let (mut ps, mut pf) = (Path::new(), Path::new());
            path_data(stroke, xf, &mut ps);
            path_data(fill, xf, &mut pf);
            render(&ps, &pf, n, sw24, sc)
        })
    }

    /// Pose un glyphe SVG (voir [`IconCache::glyph_mask`]), coin haut-gauche
    /// en (x, y), teinte `c`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_glyph(
        &mut self,
        cv: &mut Canvas<'_>,
        stroke: &str,
        fill: &str,
        sw24: f32,
        x: f32,
        y: f32,
        size: u16,
        c: Color,
    ) {
        let (n, m) = self.glyph_mask(stroke, fill, sw24, size);
        let n = *n;
        cv.draw_mask(m, n, n, x.round() as i32, y.round() as i32, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_icone_se_dessine_dans_son_carre() {
        let mut c = IconCache::default();
        let all = [
            Icon::Folder,
            Icon::FolderOpen,
            Icon::FolderSearch,
            Icon::File,
            Icon::FileMusic,
            Icon::Star,
            Icon::Search,
            Icon::Play,
            Icon::Stop,
            Icon::Pause,
            Icon::Repeat,
            Icon::Volume,
            Icon::VolumeX,
            Icon::ChevronRight,
            Icon::ChevronDown,
            Icon::X,
            Icon::Shuffle,
            Icon::Database,
            Icon::HardDrive,
            Icon::AudioLines,
            Icon::Music,
            Icon::Refresh,
            Icon::ArrowUp,
            Icon::House,
            Icon::List,
            Icon::SkipBack,
            Icon::Circle,
            Icon::Metronome,
            Icon::Plus,
            Icon::Guitar,
            Icon::Trash,
            Icon::Download,
            Icon::Headphones,
            Icon::Locate,
            Icon::Help,
            Icon::Magnet,
            Icon::Palette,
            Icon::Fold,
            Icon::Wand,
            Icon::Piano,
            Icon::Reverse,
            Icon::Normalize,
            Icon::Bars,
            Icon::Free,
            Icon::Isolate,
            Icon::Grid,
            Icon::SquarePlus,
            Icon::CirclePlus,
            Icon::Camera,
            Icon::Zap,
            Icon::Dices,
            Icon::History,
            Icon::TrendingUp,
            Icon::Power,
            Icon::Ellipsis,
            Icon::Link,
            Icon::Tag,
            Icon::Puzzle,
            Icon::Lock,
            Icon::LockOpen,
            Icon::Save,
            Icon::Settings,
            Icon::Maximize,
            Icon::Move,
            Icon::Ruler,
            Icon::Gauge,
            Icon::ChevronLeft,
        ];
        for i in all {
            let (n, m) = c.mask(i, 24, false);
            let ink: u32 = m.iter().map(|&v| v as u32).sum();
            assert!(ink > 255 * 10, "{i:?} presque vide");
            assert!(ink < 255 * (*n * *n) as u32 / 2, "{i:?} trop pleine");
        }
    }

    /// Un anneau couvre son aire vraie (2 pi r w). Le rasteriseur additionne
    /// les couvertures des morceaux d'un trait qui se recouvrent : rendu
    /// directement, l'anneau de 48 px etait encre a 108 %.
    #[test]
    fn un_anneau_couvre_son_aire_vraie() {
        let mut c = IconCache::default();
        let (_, m) = c.mask(Icon::Circle, 48, false);
        let ink: f64 = m.iter().map(|&v| v as f64 / 255.0).sum();
        // Rayon 10 et trait 2 sur 24, a 48 px : 20 et 4.
        let want = std::f64::consts::TAU * 20.0 * 4.0;
        assert!(
            (ink - want).abs() / want < 0.02,
            "{ink:.1} px encres contre {want:.1}"
        );
    }

    /// Un carre cale a des bords nets a toute petite taille : sur sa
    /// colonne et sa ligne du milieu, deux pixels pleins (les deux bords) et
    /// rien d'autre. La largeur et le rayon exacts d'un rectangle laissaient
    /// un bord a cheval sur deux pixels gris.
    #[test]
    fn un_carre_cale_a_des_bords_nets() {
        let mut c = IconCache::default();
        for size in 12u16..=24 {
            let (n, m) = c.mask(Icon::Stop, size, false);
            let n = *n as usize;
            let mid = n / 2;
            for (sens, line) in [
                (
                    "colonne",
                    (0..n).map(|y| m[y * n + mid]).collect::<Vec<u8>>(),
                ),
                ("ligne", m[mid * n..(mid + 1) * n].to_vec()),
            ] {
                let full = line.iter().filter(|&&v| v >= 250).count();
                let grey = line.iter().filter(|&&v| v > 5 && v < 250).count();
                let w = if size >= 18 { 4 } else { 2 };
                assert!(
                    full == w && grey == 0,
                    "{size} px, {sens} du milieu : {line:?}"
                );
            }
        }
    }

    /// Un glyphe donne en SVG (la palette) passe par le meme rendu qu'une
    /// icone : meme dessin, meme masque.
    #[test]
    fn un_glyphe_svg_se_rend_comme_l_icone() {
        let mut c = IconCache::default();
        for size in [13u16, 16, 18, 40] {
            let a = c.mask(Icon::Plus, size, false).clone();
            let b = c.glyph_mask("M5 12h14M12 5v14", "", 2.0, size).clone();
            assert_eq!(a, b, "{size} px");
        }
    }

    /// Un trait horizontal cale : des pixels pleins (255), pas deux lignes
    /// a moitie grises.
    #[test]
    fn un_trait_cale_est_net() {
        let mut c = IconCache::default();
        for size in [12u16, 15, 16, 18, 20] {
            let (n, m) = c.mask(Icon::Plus, size, false);
            let n = *n as usize;
            // La barre horizontale du plus : la ligne la plus encree.
            let best = (0..n)
                .map(|y| m[y * n..(y + 1) * n].iter().map(|&v| v as u32).sum::<u32>())
                .max()
                .unwrap();
            let full = (0..n)
                .find(|&y| m[y * n..(y + 1) * n].iter().map(|&v| v as u32).sum::<u32>() == best)
                .map(|y| m[y * n..(y + 1) * n].iter().filter(|&&v| v == 255).count())
                .unwrap();
            assert!(
                full >= n / 2,
                "{size} px : {full} pixels pleins sur la barre"
            );
        }
    }
}
