//! La geometrie d'une barre de defilement verticale.
//!
//! Le defilement est NET : aucune glisse (decision de Cedric, l'ancienne
//! suite avait retire sa glisse de 90 ms). La molette avance d'un nombre de
//! lignes par cran.

/// L'etat d'un defilement : contenu de `content` px dans une vue de `view` px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    pub pos: f32,
    pub content: f32,
    pub view: f32,
    /// Saisie du curseur : (y de la souris, pos) au debut du geste.
    pub grab: Option<(f32, f32)>,
}

impl Scroll {
    pub fn max(&self) -> f32 {
        (self.content - self.view).max(0.0)
    }

    pub fn clamp(&mut self) {
        self.pos = self.pos.clamp(0.0, self.max());
    }

    pub fn set(&mut self, content: f32, view: f32) {
        self.content = content;
        self.view = view;
        self.clamp();
    }

    /// Rend visible la bande [y, y + h) du contenu, au plus pres.
    pub fn reveal(&mut self, y: f32, h: f32) {
        if y < self.pos {
            self.pos = y;
        } else if y + h > self.pos + self.view {
            self.pos = y + h - self.view;
        }
        self.clamp();
    }

    pub fn needed(&self) -> bool {
        self.content > self.view + 0.5
    }

    /// Le curseur dans une piste de `track` px : (debut, longueur).
    pub fn thumb(&self, track: f32, min_len: f32) -> (f32, f32) {
        if !self.needed() || track <= 0.0 {
            return (0.0, track);
        }
        let len = (track * self.view / self.content).clamp(min_len.min(track), track);
        let room = track - len;
        let t = if self.max() > 0.0 {
            self.pos / self.max()
        } else {
            0.0
        };
        (room * t, len)
    }

    /// Un deplacement du curseur de `dy` px depuis la saisie.
    pub fn drag(&mut self, y: f32, track: f32, min_len: f32) {
        if let Some((y0, p0)) = self.grab {
            let (_, len) = self.thumb(track, min_len);
            let room = (track - len).max(1.0);
            self.pos = p0 + (y - y0) / room * self.max();
            self.clamp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_curseur_suit_la_position() {
        let mut s = Scroll::default();
        s.set(1000.0, 100.0);
        assert_eq!(s.thumb(100.0, 10.0), (0.0, 10.0));
        s.pos = s.max();
        let (a, l) = s.thumb(100.0, 10.0);
        assert!((a + l - 100.0).abs() < 1e-4);
        s.reveal(10.0, 20.0);
        assert_eq!(s.pos, 10.0);
        s.reveal(500.0, 20.0);
        assert_eq!(s.pos, 420.0);
    }
}
