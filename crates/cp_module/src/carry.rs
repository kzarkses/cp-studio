//! Le glisser commun : ce qu'une vue de CP Studio tire hors de sa fenetre,
//! vers une autre vue, ou qu'elle soit (ancree, flottante, dans un autre
//! docker).
//!
//! - La vue source le tient a jour a chaque mouvement ([`App::carry_move`]) :
//!   elle garde la souris pendant son geste, elle voit donc la souris hors
//!   de sa fenetre.
//! - La vue sous la souris (la fenetre la plus profonde sous le point, pas
//!   un rectangle : une fenetre posee par-dessus l'emporte) allume la place
//!   du lacher.
//! - Au lacher ([`App::carry_drop`]), c'est elle qui recoit ; sinon la vue
//!   source garde son geste d'avant (une piste, la Session, une piste
//!   neuve).
//!
//! Ce qu'on porte aujourd'hui : des effets du navigateur, vers FX
//! Constellation. Une nouvelle charge (un effet d'une chaine, une source de
//! modulation, un clip) ou une nouvelle vue d'arrivee s'ajoutent ici.

use cp_sys::ui;

use crate::App;

/// Ce qu'on porte.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Payload {
    /// Des effets du catalogue du navigateur (leurs rangs).
    Plugins(Vec<usize>),
}

/// Une vue qui peut recevoir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Fxc,
}

/// Le glisser en cours.
#[derive(Clone, Debug)]
pub(crate) struct Carry {
    pub payload: Payload,
    /// La vue allumee.
    pub over: Option<Target>,
}

impl App {
    /// La vue de CP Studio sous ce point de l'ecran qui prendrait cette
    /// charge, et le point dans sa fenetre.
    fn carry_target_at(&self, p: &Payload, sx: i32, sy: i32) -> Option<(Target, f32, f32)> {
        let h = ui::window_at(sx, sy);
        if let Some(w) = self.fxc.win.as_ref().map(|w| w.hwnd) {
            if ui::is_within(w, h) && ui::is_shown(w) && self.fxc_takes(p) {
                let (x, y) = ui::screen_to_client(w, sx, sy);
                return Some((Target::Fxc, x as f32, y as f32));
            }
        }
        None
    }

    /// La charge a bouge (point de l'ecran) : la vue dessous s'allume, celle
    /// d'avant s'eteint.
    pub(crate) fn carry_move(&mut self, payload: Payload, sx: i32, sy: i32) {
        let at = self.carry_target_at(&payload, sx, sy);
        let was = self.carry.as_ref().and_then(|c| c.over);
        if let Some(t) = was.filter(|&t| Some(t) != at.map(|a| a.0)) {
            self.carry_leave(t);
        }
        if let Some((t, x, y)) = at {
            match t {
                Target::Fxc => self.fxc_carry_over(x, y),
            }
        }
        self.carry = Some(Carry {
            payload,
            over: at.map(|a| a.0),
        });
    }

    fn carry_leave(&mut self, t: Target) {
        match t {
            Target::Fxc => self.fxc_carry_leave(),
        }
    }

    /// Le lacher (point de l'ecran) : vrai si une vue l'a recu.
    pub(crate) fn carry_drop(&mut self, sx: i32, sy: i32) -> bool {
        let Some(c) = self.carry.take() else {
            return false;
        };
        if let Some(t) = c.over {
            self.carry_leave(t);
        }
        let Some((t, x, y)) = self.carry_target_at(&c.payload, sx, sy) else {
            return false;
        };
        match t {
            Target::Fxc => self.fxc_carry_drop(c.payload, x, y),
        }
    }

    /// Le geste s'arrete sans lacher sur une vue : tout s'eteint.
    pub(crate) fn carry_cancel(&mut self) {
        if let Some(c) = self.carry.take() {
            if let Some(t) = c.over {
                self.carry_leave(t);
            }
        }
    }
}
