//! Les icones : Lucide (lucide.dev, licence ISC, copyright Lucide Icons and
//! Contributors), tracees en vecteur par le moteur, a toute taille.
//!
//! Une icone est un trait rond d'epaisseur 2 dans un repere 24 x 24 (c'est le
//! dessin de Lucide, nos traits ont les memes bouts et jointures ronds). Elle
//! est rasterisee UNE fois par taille en masque de couverture, puis teintee a
//! chaque pose.

use std::collections::HashMap;

use cp_gfx::{Canvas, Color, Path, Scratch, Surface, rgb};

use crate::svg::{Xf, shapes};

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
        }
    }
}

/// Les masques deja rasterises, par (icone, taille en px, plein).
#[derive(Default)]
pub struct IconCache {
    masks: HashMap<(Icon, u16, bool), (i32, Vec<u8>)>,
    scratch: Scratch,
}

impl IconCache {
    /// Le masque de l'icone a `size` px (carre). `filled` : rempli au lieu de
    /// trace (une etoile de favori).
    pub fn mask(&mut self, icon: Icon, size: u16, filled: bool) -> &(i32, Vec<u8>) {
        let sc = &mut self.scratch;
        self.masks.entry((icon, size, filled)).or_insert_with(|| {
            let n = size.max(1) as i32;
            let mut s = Surface::new(n, n);
            let k = n as f32 / 24.0;
            // Lucide trace en 2 sur 24. En petit, une epaisseur fractionnaire
            // (1,25 px a 15 px) grise deux pixels : c'etait le flou. On trace
            // en pixels ENTIERS, les points cales sur la grille ; au-dela de
            // 32 px, le trace exact est deja net.
            let (w, grid) = if n <= 32 {
                let w = (2.0 * k).round().max(1.0);
                (w, Some(if w as i32 % 2 == 1 { 0.5 } else { 0.0 }))
            } else {
                (2.0 * k, None)
            };
            let mut p = Path::new();
            shapes(
                icon.svg(),
                Xf {
                    k,
                    ox: 0.0,
                    oy: 0.0,
                    grid,
                },
                &mut p,
            );
            {
                let mut cv = Canvas::new(&mut s, sc);
                if filled {
                    cv.fill_path(&p, rgb(255, 255, 255));
                }
                cv.stroke_path(&p, w, rgb(255, 255, 255));
            }
            let mask = s.px.iter().map(|&px| (px >> 24) as u8).collect();
            (n, mask)
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
        ];
        for i in all {
            let (n, m) = c.mask(i, 24, false);
            let ink: u32 = m.iter().map(|&v| v as u32).sum();
            assert!(ink > 255 * 10, "{i:?} presque vide");
            assert!(ink < 255 * (*n * *n) as u32 / 2, "{i:?} trop pleine");
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
