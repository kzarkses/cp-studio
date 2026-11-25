//! L'infobulle de nos vues, dans le style des popovers : une image posee sur
//! l'ecran (`cp_sys::overlay`, que la souris traverse et que Windows ne
//! rappelle jamais), plus le controle de Windows (sa boite jaune ou grise,
//! sa police a lui).

use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Color, Scratch, Surface};
use cp_sys::overlay::Overlay;
use cp_sys::ui;
use cp_sys::win::Hwnd;
use cp_ui::Theme;

pub(crate) struct Tip {
    owner: Hwnd,
    ov: Option<Overlay>,
    surf: Surface,
    /// Ce qui est a l'ecran (le meme texte au meme endroit ne se repeint
    /// pas).
    shown: Option<(String, i32, i32)>,
}

impl Tip {
    /// Une infobulle pour la vue `owner` (rien n'est cree avant la premiere
    /// pose).
    pub fn new(owner: Hwnd) -> Option<Tip> {
        Some(Tip {
            owner,
            ov: None,
            surf: Surface::new(0, 0),
            shown: None,
        })
    }

    /// Montre `s` (une ligne par saut de ligne), son coin haut-gauche au
    /// point d'ecran (`sx`, `sy`) ; elle reste dans l'ecran (au-dessus du
    /// point si elle deborde en bas).
    pub fn show(
        &mut self,
        s: &str,
        sx: i32,
        sy: i32,
        th: &Theme,
        text: &mut TextCache,
        scratch: &mut Scratch,
    ) {
        if s.trim().is_empty() {
            self.hide();
            return;
        }
        if self
            .shown
            .as_ref()
            .is_some_and(|(t, x, y)| t == s && *x == sx && *y == sy)
            && self.ov.as_ref().is_some_and(|o| o.is_shown())
        {
            return;
        }
        let (w, h) = cp_ui::pop::tip_size(th, text, s);
        let m = th.s(12.0).ceil();
        let (iw, ih) = ((w + 2.0 * m) as i32, (h + 2.0 * m) as i32);
        self.surf.resize(iw, ih);
        {
            let mut cv = Canvas::new(&mut self.surf, scratch);
            cv.clear(Color::TRANSPARENT);
            cp_ui::pop::draw_tip(&mut cv, th, text, s, m, m);
        }
        // Dans la zone de travail de l'ecran du point.
        let (ax, ay, aw, ah) = ui::work_area(sx, sy);
        let mut x = sx;
        let mut y = sy;
        if x + w as i32 > ax + aw {
            x = (ax + aw - w as i32).max(ax);
        }
        if y + h as i32 > ay + ah {
            y = (sy - h as i32 - (28.0 * th.scale) as i32).max(ay);
        }
        if self.ov.is_none() {
            self.ov = Overlay::new(self.owner);
        }
        if let Some(ov) = &mut self.ov {
            if ov.show(x - m as i32, y - m as i32, iw, ih, &self.surf.px) {
                self.shown = Some((s.to_string(), sx, sy));
            }
        }
    }

    pub fn hide(&mut self) {
        if let Some(ov) = &mut self.ov {
            ov.hide();
        }
        self.shown = None;
    }

    pub fn is_shown(&self) -> bool {
        self.ov.as_ref().is_some_and(|o| o.is_shown())
    }
}
