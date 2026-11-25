//! Un champ de saisie d'une ligne : curseur, selection, mots (Ctrl), et le
//! texte qui defile quand il deborde. Le presse-papiers est a l'appelant
//! (`selected` / `insert`).

use cp_gfx::text::{Font, TextCache};
use cp_gfx::{Canvas, Color};

use crate::{Mods, Theme, vk};

#[derive(Clone, Debug, Default)]
pub struct TextField {
    pub text: String,
    /// Positions en OCTETS, toujours sur une frontiere de caractere.
    cur: usize,
    anchor: usize,
    scroll: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    None,
    Moved,
    Changed,
}

impl TextField {
    pub fn set(&mut self, s: &str) {
        self.text = s.to_string();
        self.cur = self.text.len();
        self.anchor = self.cur;
    }

    /// Le curseur et le debut de la selection (octets).
    pub fn caret(&self) -> (usize, usize) {
        (self.cur, self.anchor)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cur = 0;
        self.anchor = 0;
        self.scroll = 0.0;
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cur = self.text.len();
    }

    fn sel(&self) -> (usize, usize) {
        (self.cur.min(self.anchor), self.cur.max(self.anchor))
    }

    pub fn selected(&self) -> &str {
        let (a, b) = self.sel();
        &self.text[a..b]
    }

    fn prev(&self, i: usize) -> usize {
        self.text[..i]
            .char_indices()
            .next_back()
            .map_or(0, |(k, _)| k)
    }

    fn next(&self, i: usize) -> usize {
        self.text[i..]
            .chars()
            .next()
            .map_or(i, |c| i + c.len_utf8())
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && self.text[..i].ends_with(char::is_whitespace) {
            i = self.prev(i);
        }
        while i > 0 && !self.text[..i].ends_with(char::is_whitespace) {
            i = self.prev(i);
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        let n = self.text.len();
        while i < n && !self.text[i..].starts_with(char::is_whitespace) {
            i = self.next(i);
        }
        while i < n && self.text[i..].starts_with(char::is_whitespace) {
            i = self.next(i);
        }
        i
    }

    /// Remplace la selection par `s`.
    pub fn insert(&mut self, s: &str) -> Edit {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let (a, b) = self.sel();
        if s.is_empty() && a == b {
            return Edit::None;
        }
        self.text.replace_range(a..b, &s);
        self.cur = a + s.len();
        self.anchor = self.cur;
        Edit::Changed
    }

    pub fn char(&mut self, c: char) -> Edit {
        if c.is_control() {
            return Edit::None;
        }
        let mut b = [0u8; 4];
        self.insert(c.encode_utf8(&mut b))
    }

    pub fn key(&mut self, k: u32, m: Mods) -> Edit {
        let moved = |f: &mut TextField, to: usize, shift: bool| {
            f.cur = to;
            if !shift {
                f.anchor = to;
            }
            Edit::Moved
        };
        match k {
            vk::LEFT => {
                let (a, b) = self.sel();
                let to = if !m.shift && a != b {
                    a
                } else if m.ctrl {
                    self.word_left(self.cur)
                } else {
                    self.prev(self.cur)
                };
                moved(self, to, m.shift)
            }
            vk::RIGHT => {
                let (a, b) = self.sel();
                let to = if !m.shift && a != b {
                    b
                } else if m.ctrl {
                    self.word_right(self.cur)
                } else {
                    self.next(self.cur)
                };
                moved(self, to, m.shift)
            }
            vk::HOME => moved(self, 0, m.shift),
            vk::END => moved(self, self.text.len(), m.shift),
            vk::BACK => {
                let (a, b) = self.sel();
                if a == b {
                    if a == 0 {
                        return Edit::None;
                    }
                    self.anchor = if m.ctrl {
                        self.word_left(a)
                    } else {
                        self.prev(a)
                    };
                }
                self.insert("")
            }
            vk::DELETE => {
                let (a, b) = self.sel();
                if a == b {
                    if b >= self.text.len() {
                        return Edit::None;
                    }
                    self.anchor = if m.ctrl {
                        self.word_right(b)
                    } else {
                        self.next(b)
                    };
                }
                self.insert("")
            }
            0x41 if m.ctrl => {
                self.select_all();
                Edit::Moved
            }
            _ => Edit::None,
        }
    }

    /// Le caractere le plus proche de `x` (relatif au debut du texte visible).
    pub fn click(&mut self, t: &mut TextCache, f: Font, x: f32, shift: bool) {
        let target = x + self.scroll;
        let mut best = 0;
        let mut acc = 0.0;
        for (i, c) in self.text.char_indices() {
            let w = t.measure(f, c.encode_utf8(&mut [0u8; 4]));
            if acc + w * 0.5 > target {
                best = i;
                self.cur = best;
                if !shift {
                    self.anchor = best;
                }
                return;
            }
            acc += w;
            best = i + c.len_utf8();
        }
        self.cur = best;
        if !shift {
            self.anchor = best;
        }
    }

    /// Dessine le champ dans (x, y, w, h).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        cv: &mut Canvas<'_>,
        t: &mut TextCache,
        th: &Theme,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        focused: bool,
        placeholder: &str,
    ) {
        self.draw_ex(cv, t, th, x, y, w, h, focused, placeholder, 0.0);
    }

    /// Comme [`TextField::draw`], le texte s'arretant `right` px avant le
    /// bord droit (une icone y loge).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ex(
        &mut self,
        cv: &mut Canvas<'_>,
        t: &mut TextCache,
        th: &Theme,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        focused: bool,
        placeholder: &str,
        right: f32,
    ) {
        let f = th.font();
        let r = th.s(4.0);
        cv.fill_round_rect(x, y, w, h, r, th.field);
        if focused {
            cv.stroke_round_rect(x, y, w, h, r, th.s(1.0), th.accent.fade(0.7));
        }
        let pad = th.s(6.0);
        let (ix, iw) = (x + pad, (w - 2.0 * pad - right).max(0.0));
        let m = t.metrics(f);
        let base = y + (h - (m.ascent + m.descent)) * 0.5 + m.ascent;
        // Le curseur reste visible.
        let cx = t.measure(f, &self.text[..self.cur]);
        if cx - self.scroll > iw {
            self.scroll = cx - iw;
        } else if cx < self.scroll {
            self.scroll = cx;
        }
        if t.measure(f, &self.text) <= iw {
            self.scroll = 0.0;
        }
        if !cv.push_clip(ix, y, iw, h) {
            cv.pop_clip();
            return;
        }
        if self.text.is_empty() && !placeholder.is_empty() {
            t.draw(cv, f, ix, base, placeholder, th.text_faint);
        }
        let (a, b) = self.sel();
        if focused && a != b {
            let xa = t.measure(f, &self.text[..a]) - self.scroll;
            let xb = t.measure(f, &self.text[..b]) - self.scroll;
            cv.fill_rect(ix + xa, y + th.s(3.0), xb - xa, h - th.s(6.0), th.sel);
        }
        t.draw(cv, f, ix - self.scroll, base, &self.text, th.text);
        if focused {
            cv.fill_rect(
                (ix + cx - self.scroll).round(),
                y + th.s(4.0),
                th.s(1.0).max(1.0),
                h - th.s(8.0),
                Color { a: 230, ..th.text },
            );
        }
        cv.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saisir_effacer_et_naviguer_par_mots() {
        let mut f = TextField::default();
        for c in "kick drum é".chars() {
            f.char(c);
        }
        assert_eq!(f.text, "kick drum é");
        f.key(vk::BACK, Mods::default());
        assert_eq!(f.text, "kick drum ");
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        f.key(vk::BACK, ctrl);
        assert_eq!(f.text, "kick ");
        f.key(vk::HOME, Mods::default());
        f.key(
            vk::RIGHT,
            Mods {
                shift: true,
                ctrl: true,
                alt: false,
                win: false,
            },
        );
        assert_eq!(f.selected(), "kick ");
        f.insert("snare ");
        assert_eq!(f.text, "snare ");
    }
}
