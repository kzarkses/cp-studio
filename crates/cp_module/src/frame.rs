//! L'image d'une grande vue (Guitar Tab, l'editeur).
//!
//! Trois regles, pour que REAPER garde la main :
//! - on ne dessine qu'au rythme de l'ecran : un geste de souris marque ce qui
//!   change, l'image suivante le dessine une fois (vingt mouvements entre deux
//!   images ne coutent qu'une image) ; le premier apres un repos part tout de
//!   suite ;
//! - on ne redessine que les zones qui changent (`damage`), et la tete de
//!   lecture, les curseurs et la ligne de la souris sont des traits poses
//!   par-dessus la scene (`Layers`) : la lecture ne redessine rien ;
//! - on n'envoie a l'ecran que ces zones.

use cp_gfx::layers::{Layers, Stroke};
use cp_gfx::{Canvas, IRect, Scratch};
use cp_sys::ui;
use cp_sys::win::Hwnd;

/// Les images de suite sans rien a montrer avant que l'horloge s'arrete
/// (pres d'une seconde : un geste qui reprend ne l'attend pas).
const IDLE_FRAMES: u32 = 60;

#[derive(Default)]
pub(crate) struct Frame {
    pub layers: Layers,
    present: Vec<u32>,
    /// L'horloge de l'ecran tourne pour cette vue.
    pub frames_on: bool,
    /// Images de suite sans rien de neuf.
    idle: u32,
}

impl Frame {
    /// La taille de la vue ; vrai si tout est a redessiner.
    pub fn resize(&mut self, w: i32, h: i32) -> bool {
        self.layers.resize(w, h)
    }

    /// Dessine les zones `damage` de la scene (`draw` la dessine entiere, le
    /// canevas coupe a la zone).
    pub fn paint(
        &mut self,
        damage: &[IRect],
        scratch: &mut Scratch,
        mut draw: impl FnMut(&mut Canvas<'_>),
    ) {
        let (w, h) = (self.layers.scene.w, self.layers.scene.h);
        let whole = IRect::new(0, 0, w, h);
        for r in damage {
            let r = r.intersect(&whole);
            if r.is_empty() {
                continue;
            }
            let mut cv = Canvas::new(&mut self.layers.scene, scratch);
            if cv.push_clip(r.x as f32, r.y as f32, r.w as f32, r.h as f32) {
                draw(&mut cv);
            }
            if r == whole {
                self.layers.drawn();
            }
        }
    }

    /// Pose les traits et envoie a l'ecran ce qui a change depuis l'image
    /// d'avant. Rend vrai si quelque chose est parti.
    pub fn show(&mut self, hwnd: Hwnd, damage: &[IRect], strokes: &[Stroke]) -> bool {
        let (w, h) = (self.layers.scene.w, self.layers.scene.h);
        if w <= 0 || h <= 0 {
            return false;
        }
        let rects = self.layers.compose(damage, strokes);
        if rects.is_empty() {
            self.idle = self.idle.saturating_add(1);
            return false;
        }
        self.idle = 0;
        let v: Vec<(i32, i32, i32, i32)> = rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect();
        ui::present(hwnd, &self.layers.out.px, w, h, &v, &mut self.present);
        true
    }

    /// WM_PAINT : l'image entiere, telle qu'elle est.
    pub fn repaint(&mut self, hwnd: Hwnd) -> bool {
        let (w, h) = (self.layers.out.w, self.layers.out.h);
        if w <= 0 || h <= 0 {
            return false;
        }
        ui::present(
            hwnd,
            &self.layers.out.px,
            w,
            h,
            &[(0, 0, w, h)],
            &mut self.present,
        );
        true
    }

    /// Assez d'images sans rien de neuf : l'horloge peut s'arreter.
    pub fn idle(&self) -> bool {
        self.idle >= IDLE_FRAMES
    }

    /// Quelque chose vient de changer : l'horloge repart (ou continue).
    pub fn wake(&mut self) {
        self.idle = 0;
    }
}
