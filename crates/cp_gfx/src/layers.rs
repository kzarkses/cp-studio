//! Les deux couches d'une vue.
//!
//! LA SCENE se redessine quand elle change, et seulement dans les zones qui
//! ont change (`damage`). LES TRAITS (tete de lecture, curseur d'edition,
//! ligne de la souris) bougent seuls par-dessus : une image ou seuls eux
//! bougent ne redessine rien, elle recopie de la scene les colonnes qu'ils
//! quittent et pose les nouvelles. C'est ce qui rend la lecture gratuite : une
//! tete de lecture qui avance coute deux colonnes, pas une fenetre.

use crate::color::over;
use crate::{Color, IRect, Surface};

/// Un trait : un rectangle plein (une ligne d'un pixel, une marque).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub r: IRect,
    pub c: Color,
}

#[derive(Default)]
pub struct Layers {
    /// La scene, sans les traits.
    pub scene: Surface,
    /// Ce qui part a l'ecran : la scene et les traits.
    pub out: Surface,
    /// Les traits poses sur `out` a la derniere image.
    shown: Vec<Stroke>,
    /// La scene n'a jamais ete dessinee a cette taille.
    fresh: bool,
}

impl Layers {
    /// Rend vrai si la taille a change (tout est a redessiner).
    pub fn resize(&mut self, w: i32, h: i32) -> bool {
        let a = self.scene.resize(w, h);
        let b = self.out.resize(w, h);
        if a || b {
            self.shown.clear();
            self.fresh = true;
        }
        a || b || self.fresh
    }

    /// La scene vient d'etre dessinee en entier.
    pub fn drawn(&mut self) {
        self.fresh = false;
    }

    /// Compose une image : les zones de scene redessinees (`damage`), les
    /// traits d'avant effaces, les traits d'aujourd'hui poses. Rend les zones
    /// a envoyer a l'ecran (vide : rien n'a change).
    pub fn compose(&mut self, damage: &[IRect], strokes: &[Stroke]) -> Vec<IRect> {
        let (w, h) = (self.scene.w, self.scene.h);
        let whole = IRect::new(0, 0, w, h);
        if damage.is_empty() && strokes == self.shown.as_slice() {
            return Vec::new();
        }
        let mut rects: Vec<IRect> = damage.iter().map(|r| r.intersect(&whole)).collect();
        // Un trait qui ne bouge pas, hors de toute zone redessinee, reste tel
        // quel a l'ecran.
        for s in self.shown.iter().filter(|s| !strokes.contains(s)) {
            rects.push(s.r.intersect(&whole));
        }
        for s in strokes.iter().filter(|s| !self.shown.contains(s)) {
            rects.push(s.r.intersect(&whole));
        }
        rects.retain(|r| !r.is_empty());
        let rects = merge(rects);
        let ws = w as usize;
        for r in &rects {
            for y in r.y..r.bottom() {
                let a = y as usize * ws + r.x as usize;
                let b = a + r.w as usize;
                self.out.px[a..b].copy_from_slice(&self.scene.px[a..b]);
            }
        }
        // Les traits, la ou l'image change (un trait reste ailleurs est deja
        // a l'ecran, pose une fois).
        for s in strokes {
            let src = s.c.premul();
            for r in &rects {
                let q = s.r.intersect(r).intersect(&whole);
                if q.is_empty() {
                    continue;
                }
                for y in q.y..q.bottom() {
                    let row = y as usize * ws;
                    for x in q.x..q.right() {
                        let d = &mut self.out.px[row + x as usize];
                        *d = over(*d, src, 255);
                    }
                }
            }
        }
        self.shown = strokes.to_vec();
        rects
    }
}

/// Fond les zones qui se touchent ou se recouvrent (deux colonnes voisines
/// d'une tete de lecture n'en font qu'une), sans fondre deux zones lointaines
/// en un grand rectangle.
pub fn merge(mut v: Vec<IRect>) -> Vec<IRect> {
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for i in 0..v.len() {
            for j in i + 1..v.len() {
                let (a, b) = (v[i], v[j]);
                let u = a.union(&b);
                let area = |r: &IRect| r.w as i64 * r.h as i64;
                // Fondre ne coute pas plus d'un quart de pixels en plus.
                if area(&u) * 4 <= (area(&a) + area(&b)) * 5 + 64 * 64 {
                    v[i] = u;
                    v.swap_remove(j);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rgb;

    fn fill(s: &mut Surface, c: u32) {
        s.px.fill(c);
    }

    #[test]
    fn une_tete_qui_avance_ne_recopie_que_ses_colonnes() {
        let mut l = Layers::default();
        assert!(l.resize(100, 50));
        fill(&mut l.scene, 0xff10_1010);
        let full = l.compose(&[IRect::new(0, 0, 100, 50)], &[]);
        l.drawn();
        assert_eq!(full, vec![IRect::new(0, 0, 100, 50)]);
        let red = rgb(255, 0, 0);
        let s1 = Stroke {
            r: IRect::new(10, 0, 1, 50),
            c: red,
        };
        let r1 = l.compose(&[], &[s1]);
        assert_eq!(r1, vec![IRect::new(10, 0, 1, 50)]);
        assert_eq!(l.out.px[10], red.premul());
        // Rien ne bouge : rien a envoyer.
        assert!(l.compose(&[], &[s1]).is_empty());
        // Elle avance d'un pixel : l'ancienne colonne et la nouvelle, fondues.
        let s2 = Stroke {
            r: IRect::new(11, 0, 1, 50),
            c: red,
        };
        let r2 = l.compose(&[], &[s2]);
        assert_eq!(r2, vec![IRect::new(10, 0, 2, 50)]);
        assert_eq!(l.out.px[10], 0xff10_1010);
        assert_eq!(l.out.px[11], red.premul());
        // Une zone de scene redessinee sous un trait immobile : le trait y
        // est repose.
        fill(&mut l.scene, 0xff20_2020);
        let r3 = l.compose(&[IRect::new(0, 0, 30, 10)], &[s2]);
        assert_eq!(r3, vec![IRect::new(0, 0, 30, 10)]);
        assert_eq!(l.out.px[11], red.premul());
        assert_eq!(l.out.px[12], 0xff20_2020);
        // Hors de la zone, l'ecran n'a pas change.
        assert_eq!(l.out.px[20 * 100 + 12], 0xff10_1010);
    }

    #[test]
    fn deux_zones_lointaines_restent_deux() {
        let v = merge(vec![IRect::new(0, 0, 1, 500), IRect::new(900, 0, 1, 500)]);
        assert_eq!(v.len(), 2);
        let v = merge(vec![IRect::new(0, 0, 1, 500), IRect::new(3, 0, 1, 500)]);
        assert_eq!(v, vec![IRect::new(0, 0, 4, 500)]);
    }
}
