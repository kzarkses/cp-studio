//! Le transport habille par le theme de REAPER : les images memes du
//! transport et des barres d'outils natives, a leur taille d'origine (jamais
//! reduites : c'est ce qui donne la finesse des icones de REAPER), et les
//! couleurs du theme. Le module lit le theme (`cp_theme`) et remplit un
//! [`Skin`] ; sans lui, le transport garde ses icones vectorielles.
//!
//! Les etats suivent la convention de REAPER : vignette 0 normale, 1 survol,
//! 2 appui ; une icone de barre d'outils a bascule montre la 2 allumee.

use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Surface, hex, rgb};
use cp_theme::{Frames, Nine, ThemeFiles};
use cp_ui::{Icon, IconCache, Theme};

use crate::{Glyph, Hit, PAUSED, PLAYING, RECORDING, RecMode, Transport, fmt_bpm, rec_mode_mark};

/// Une couleur (r, g, b).
pub type Rgb = (u8, u8, u8);

/// Les images et couleurs du theme courant, a l'echelle de l'ecran.
#[derive(Clone, Debug, Default)]
pub struct Skin {
    /// Change a chaque nouveau theme (les images etirees en cache s'y fient).
    pub id: u32,
    /// `col_main_bg2` : le fond du transport.
    pub bg: Color,
    /// `col_toolbar_text` : le texte des boutons et des afficheurs.
    pub text: Color,
    /// `col_main_text2` : les etiquettes (« BPM », le temps en secondes).
    pub text_dim: Color,
    /// `col_toolbar_text_on` : ce qui est allume.
    pub on: Color,
    /// `col_transport_editbk` : le fond des afficheurs, faute d'image.
    pub field: Color,
    /// Les couleurs des boutons allumes du theme (lecture, pause,
    /// enregistrement), prises dans leurs images.
    pub play: Color,
    pub pause: Color,
    pub rec: Color,
    /// La famille de police des afficheurs (celle du transport du theme).
    pub face: u16,
    pub home: Option<Frames>,
    pub stop: Option<Frames>,
    pub play_off: Option<Frames>,
    pub play_on: Option<Frames>,
    pub pause_off: Option<Frames>,
    pub pause_on: Option<Frames>,
    pub rec_off: Option<Frames>,
    pub rec_on: Option<Frames>,
    /// Rec en auto-punch : sur la selection de temps (`loop`), sur les items.
    pub rec_loop_off: Option<Frames>,
    pub rec_loop_on: Option<Frames>,
    pub rec_item_off: Option<Frames>,
    pub rec_item_on: Option<Frames>,
    pub repeat_off: Option<Frames>,
    pub repeat_on: Option<Frames>,
    pub metro_off: Option<Frames>,
    pub metro_on: Option<Frames>,
    /// Le bouton vide des barres d'outils (`toolbar_blank`), ses 3 etats,
    /// pret a s'etirer (boutons de texte, onglets, fond des icones vectorielles).
    pub blank: Vec<Nine>,
    /// Le fond des afficheurs (`custom_transport_edit_bg`), a bord rose.
    pub edit: Option<Nine>,
}

impl Skin {
    /// L'habillage d'un theme a l'echelle `scale` de l'ecran. `color` lit une
    /// couleur du theme (`GetThemeColor` dans REAPER, le fichier sinon) ;
    /// `face` : la famille de police des afficheurs ; `id` : neuf a chaque
    /// theme.
    pub fn build(
        t: &ThemeFiles,
        scale: f32,
        color: &dyn Fn(&str) -> Option<Rgb>,
        face: u16,
        id: u32,
    ) -> Skin {
        let col = |k: &str, d: u32| color(k).map_or(hex(d), |(r, g, b)| rgb(r, g, b));
        // Un bouton du transport : trois vignettes de meme largeur.
        let tb = |name: &str| {
            let (img, s) = t.image(name, scale)?;
            Some(Frames::split(&img, 3)?.rescaled(s, scale))
        };
        // Une icone de barre d'outils (trois etats, ou une seule image).
        let tool = |name: &str| {
            let (img, s) = t.image(name, scale)?;
            Some(Frames::toolbar(&img).rescaled(s, scale))
        };
        let m = (5.0 * scale).round() as i32;
        let blank = tool("toolbar_blank")
            .map(|f| f.f.iter().map(|s| Nine::from_image(s, m)).collect())
            .unwrap_or_default();
        let edit = t
            .image("custom_transport_edit_bg", scale)
            .map(|(img, s)| Nine::from_image(&img, 0).rescaled(s, scale));
        let play_on = tb("transport_play_on");
        let pause_on = tb("transport_pause_on");
        let rec_on = tb("transport_record_on");
        let on = col("col_toolbar_text_on", 0x4C9EFF);
        Skin {
            id,
            bg: col("col_main_bg2", 0x333333),
            text: col("col_toolbar_text", 0xC8C8C8),
            text_dim: col("col_main_text2", 0x8A8A8A),
            on,
            field: col("col_transport_editbk", 0x262626),
            play: play_on.as_ref().and_then(face_color).unwrap_or(on),
            pause: pause_on
                .as_ref()
                .and_then(face_color)
                .unwrap_or(hex(0xF0B44C)),
            rec: rec_on
                .as_ref()
                .and_then(face_color)
                .unwrap_or(hex(0xE5534B)),
            face,
            home: tb("transport_home"),
            stop: tb("transport_stop"),
            play_off: tb("transport_play"),
            play_on,
            pause_off: tb("transport_pause"),
            pause_on,
            rec_off: tb("transport_record"),
            rec_on,
            rec_loop_off: tb("transport_record_loop"),
            rec_loop_on: tb("transport_record_loop_on"),
            rec_item_off: tb("transport_record_item"),
            rec_item_on: tb("transport_record_item_on"),
            repeat_off: tb("transport_repeat_off"),
            repeat_on: tb("transport_repeat_on"),
            metro_off: tool("toolbar_metro_off"),
            metro_on: tool("toolbar_metro_on"),
            blank,
            edit,
        }
    }
}

/// La couleur d'un bouton allume (le vert de lecture, le rouge
/// d'enregistrement) : un pixel a gauche, a mi-hauteur, hors du dessin.
pub fn face_color(f: &Frames) -> Option<Color> {
    let s = f.get(0)?;
    if s.w < 8 || s.h < 6 {
        return None;
    }
    let c = Color::from_premul(s.get(3, s.h / 2));
    (c.a > 200).then_some(c.with_alpha(255))
}

/// Que montre-t-on d'un bouton : l'etat de la souris sur lui.
fn idx(t: &Transport, hit: Hit) -> usize {
    if hit != Hit::None && t.pressed == hit {
        2
    } else if hit != Hit::None && t.hover == hit {
        1
    } else {
        0
    }
}

impl Transport {
    /// Une image etiree, gardee tant que la taille et le theme ne changent pas.
    fn stretched(&mut self, key: (u32, u8, i32, i32), n: &Nine) -> Surface {
        if let Some((_, s)) = self.stretch.iter().find(|(k, _)| *k == key) {
            return s.clone();
        }
        let s = n.render(key.2, key.3);
        if self.stretch.len() >= 24 {
            self.stretch.remove(0);
        }
        self.stretch.push((key, s.clone()));
        s
    }

    /// Le fond d'un bouton vide (texte, onglet) a la taille voulue.
    fn blank(
        &mut self,
        cv: &mut Canvas<'_>,
        sk: &Skin,
        state: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        if let Some(n) = sk.blank.get(state).or(sk.blank.last()) {
            let img = self.stretched((sk.id, state as u8, w as i32, h as i32), n);
            cv.draw_image(&img, x, y, 1.0);
        }
    }

    /// Un afficheur (temps, tempo, signature, vitesse).
    fn field(
        &mut self,
        cv: &mut Canvas<'_>,
        th: &Theme,
        sk: &Skin,
        hit: Hit,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        match &sk.edit {
            Some(n) => {
                let img = self.stretched((sk.id, 10, w as i32, h as i32), n);
                cv.draw_image(&img, x, y, 1.0);
            }
            None => cv.fill_round_rect(x, y, w, h, th.s(3.0), sk.field),
        }
        if hit != Hit::None && (self.hover == hit || self.pressed == hit) {
            cv.fill_round_rect(x, y, w, h, th.s(3.0), Color::WHITE.with_alpha(14));
        }
    }

    /// Un bouton du transport : l'image du theme, ou a defaut le bouton vide
    /// et l'icone vectorielle. Rend sa largeur.
    fn tbutton(
        &mut self,
        cv: &mut Canvas<'_>,
        icons: &mut IconCache,
        th: &Theme,
        sk: &Skin,
        hit: Hit,
        frames: Option<&Frames>,
        icon: Icon,
        lit: Option<Color>,
        x: f32,
        h: f32,
        bh: f32,
    ) -> f32 {
        let i = idx(self, hit);
        if let Some(img) = frames.and_then(|f| f.get(i)) {
            let y = ((h - img.h as f32) * 0.5).floor();
            cv.draw_image(img, x, y, 1.0);
            self.push(hit, x, y, img.w as f32, img.h as f32);
            return img.w as f32;
        }
        let by = ((h - bh) * 0.5).floor();
        self.blank(cv, sk, i, x, by, bh, bh);
        let isz = th.s(14.0) as u16;
        let c = lit.unwrap_or(sk.text);
        icons.draw(
            cv,
            icon,
            x + ((bh - isz as f32) * 0.5).round(),
            by + ((bh - isz as f32) * 0.5).round(),
            isz,
            c,
            lit.is_some() && icon != Icon::Repeat,
        );
        self.push(hit, x, by, bh, bh);
        bh
    }

    /// Toute la barre, habillee du theme.
    pub(crate) fn draw_skin(
        &mut self,
        cv: &mut Canvas<'_>,
        text: &mut TextCache,
        icons: &mut IconCache,
        th: &Theme,
        sk: &Skin,
        w: f32,
        h: f32,
    ) {
        cv.fill_rect(0.0, 0.0, w, h, sk.bg);
        let gap = th.s(2.0);
        let mut x = th.s(6.0);
        // La hauteur des afficheurs : celle des boutons du transport du theme.
        let bh = sk
            .play_off
            .as_ref()
            .or(sk.home.as_ref())
            .map_or(th.s(26.0), |f| f.h() as f32)
            .min(h);
        let by = ((h - bh) * 0.5).floor();

        // --- le transport
        let play = self.st.play;
        let playing = play & PLAYING != 0 && play & PAUSED == 0;
        let paused = play & PAUSED != 0;
        let rec = play & RECORDING != 0;
        let pick = |on: bool, a: &Option<Frames>, b: &Option<Frames>| -> Option<Frames> {
            if on {
                a.clone().or(b.clone())
            } else {
                b.clone()
            }
        };
        let buttons = [
            (Hit::Start, sk.home.clone(), Icon::SkipBack, None),
            (Hit::Stop, sk.stop.clone(), Icon::Stop, None),
            (
                Hit::Play,
                pick(playing, &sk.play_on, &sk.play_off),
                Icon::Play,
                playing.then_some(sk.play),
            ),
            (
                Hit::Pause,
                pick(paused, &sk.pause_on, &sk.pause_off),
                Icon::Pause,
                paused.then_some(sk.pause),
            ),
            (
                Hit::Record,
                // Le bouton du mode, comme celui de REAPER ; le bouton normal
                // si le theme n'en a pas (la marque vectorielle le dit).
                match self.st.rec_mode {
                    RecMode::TimeSel if sk.rec_loop_off.is_some() => {
                        pick(rec, &sk.rec_loop_on, &sk.rec_loop_off)
                    }
                    RecMode::Items if sk.rec_item_off.is_some() => {
                        pick(rec, &sk.rec_item_on, &sk.rec_item_off)
                    }
                    _ => pick(rec, &sk.rec_on, &sk.rec_off),
                },
                Icon::Circle,
                Some(if rec { sk.rec } else { sk.rec.fade(0.7) }),
            ),
            (
                Hit::Repeat,
                pick(self.st.repeat, &sk.repeat_on, &sk.repeat_off),
                Icon::Repeat,
                self.st.repeat.then_some(sk.on),
            ),
        ];
        let mode = self.st.rec_mode;
        let mode_img = match mode {
            RecMode::Normal => true,
            RecMode::TimeSel => sk.rec_loop_off.is_some(),
            RecMode::Items => sk.rec_item_off.is_some(),
        };
        for (hit, frames, icon, lit) in buttons {
            let bw = self.tbutton(cv, icons, th, sk, hit, frames.as_ref(), icon, lit, x, h, bh);
            if hit == Hit::Record && !mode_img {
                let c = if rec { sk.rec } else { sk.rec.fade(0.7) };
                rec_mode_mark(cv, th, mode, x, by, bw, bh, c);
            }
            x += bw + gap;
        }
        x += th.s(8.0);

        // --- le compteur
        let big = Font::new(sk.face, th.s(17.0) as u16, 700);
        let small = Font::new(sk.face, th.s(11.0) as u16, 400);
        let cw_beats = text.measure(big, "0000.00.00").ceil();
        let cw_time = text.measure(small, "00:00:00.000").ceil();
        let cw = cw_beats + cw_time + th.s(24.0);
        self.field(cv, th, sk, Hit::Counter, x, by, cw, bh);
        let ink = crate::TunerInk {
            text: sk.text,
            dim: sk.text_dim,
            good: sk.play,
            warn: sk.pause,
            bad: sk.rec,
            track: sk.text_dim.fade(0.35),
        };
        if self.tuner_in_clock() {
            crate::draw_tuner(
                cv,
                text,
                sk.face,
                th.scale,
                x,
                by,
                cw,
                bh,
                self.tuner.as_ref(),
                &ink,
            );
        } else {
            let beats_c = if rec {
                sk.rec
            } else if play & PLAYING != 0 {
                sk.play
            } else {
                sk.text
            };
            text.draw_in(
                cv,
                big,
                x + th.s(9.0),
                by,
                cw_beats,
                bh,
                &self.st.beats,
                beats_c,
                Align::Left,
            );
            text.draw_in(
                cv,
                small,
                x + cw - cw_time - th.s(9.0),
                by,
                cw_time,
                bh,
                &self.st.time,
                sk.text_dim,
                Align::Right,
            );
        }
        self.push(Hit::Counter, x, by, cw, bh);
        x += cw + th.s(8.0);

        // --- tempo, signature, tap, metronome, vitesse
        let mid = Font::new(sk.face, th.s(14.0) as u16, 700);
        let tiny = Font::new(sk.face, th.s(10.0) as u16, 400);
        let lw = text.measure(tiny, "BPM").ceil();
        let tw = text.measure(mid, "000.00").ceil() + lw + th.s(18.0);
        self.field(cv, th, sk, Hit::Bpm, x, by, tw, bh);
        let bpm = fmt_bpm(self.st.bpm);
        text.draw_in(
            cv,
            mid,
            x + th.s(6.0),
            by,
            tw - lw - th.s(13.0),
            bh,
            &bpm,
            sk.text,
            Align::Right,
        );
        text.draw_in(
            cv,
            tiny,
            x + tw - lw - th.s(6.0),
            by,
            lw,
            bh,
            "BPM",
            sk.text_dim,
            Align::Left,
        );
        self.push(Hit::Bpm, x, by, tw, bh);
        x += tw + gap;

        let sig = format!("{}/{}", self.st.sig.0, self.st.sig.1);
        let sw = text.measure(mid, "00/00").ceil() + th.s(14.0);
        self.field(cv, th, sk, Hit::Sig, x, by, sw, bh);
        text.draw_in(cv, mid, x, by, sw, bh, &sig, sk.text, Align::Center);
        self.push(Hit::Sig, x, by, sw, bh);
        x += sw + gap;

        let tf = Font::new(sk.face, th.s(12.0) as u16, 700);
        let tapw = text.measure(tf, "TAP").ceil() + th.s(16.0);
        let i = idx(self, Hit::Tap);
        self.blank(cv, sk, i, x, by, tapw, bh);
        text.draw_in(cv, tf, x, by, tapw, bh, "TAP", sk.text, Align::Center);
        self.push(Hit::Tap, x, by, tapw, bh);
        x += tapw + gap;

        let metro = if self.st.metronome {
            sk.metro_on.clone().or(sk.metro_off.clone())
        } else {
            sk.metro_off.clone()
        };
        let lit = self.st.metronome.then_some(sk.on);
        let mw = self.tbutton(
            cv,
            icons,
            th,
            sk,
            Hit::Metronome,
            metro.as_ref(),
            Icon::Metronome,
            lit,
            x,
            h,
            bh,
        );
        x += mw + gap;

        let rate = format!("{:.2}x", self.st.rate);
        let rw = text.measure(mid, "0.00x").ceil() + th.s(14.0);
        self.field(cv, th, sk, Hit::Rate, x, by, rw, bh);
        let rc = if (self.st.rate - 1.0).abs() > 1e-6 {
            sk.pause
        } else {
            sk.text
        };
        text.draw_in(cv, mid, x, by, rw, bh, &rate, rc, Align::Center);
        self.push(Hit::Rate, x, by, rw, bh);
        x += rw + th.s(14.0);

        // --- le selecteur de vues, a droite
        let vf = Font::new(0, th.s(13.0) as u16, 600);
        let widths: Vec<f32> = self
            .views
            .iter()
            .map(|v| text.measure(vf, &v.name).ceil() + th.s(22.0))
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
        let mut vx = (w - th.s(6.0) - total).max(x);
        let strip_end = vx - th.s(10.0);
        for (i, &vw) in widths.iter().enumerate() {
            let v = self.views[i].clone();
            let hit = Hit::View(i);
            let state = if v.active { 2 } else { idx(self, hit).min(1) };
            self.blank(cv, sk, state, vx, by, vw, bh);
            let c = if v.active { sk.on } else { sk.text };
            text.draw_in(cv, vf, vx, by, vw, bh, &v.name, c, Align::Center);
            if v.open && !v.active {
                // Sous l'onglet, hors du texte.
                let d = th.s(1.5).max(1.5);
                cv.fill_circle(vx + vw * 0.5, (by + bh + th.s(2.5)).min(h - d), d, sk.on);
            }
            self.push(hit, vx, by, vw, bh);
            vx += vw + gap;
        }

        // --- les icones, entre les deux : a leur taille, comme une barre
        // d'outils de REAPER
        let slot = sk.blank.first().map_or(th.s(30.0), |n| n.img.h as f32);
        for i in 0..self.items.len() {
            let it = self.items[i].clone();
            let hit = Hit::Item(i);
            let iw = match &it.glyph {
                Glyph::Frames(f) => f.w() as f32,
                _ => slot,
            };
            if x + iw > strip_end {
                break;
            }
            let state = if it.on || self.pressed == hit {
                2
            } else {
                idx(self, hit)
            };
            match &it.glyph {
                Glyph::Frames(f) => {
                    if let Some(img) = f.get(state) {
                        let y = ((h - img.h as f32) * 0.5).floor();
                        cv.draw_image(img, x, y, 1.0);
                    }
                }
                Glyph::Image(img) => {
                    let sy = ((h - slot) * 0.5).floor();
                    self.blank(cv, sk, state, x, sy, slot, slot);
                    cv.draw_image(
                        img,
                        x + ((slot - img.w as f32) * 0.5).round(),
                        ((h - img.h as f32) * 0.5).round(),
                        1.0,
                    );
                }
                Glyph::Icon(icon) => {
                    let sy = ((h - slot) * 0.5).floor();
                    self.blank(cv, sk, state, x, sy, slot, slot);
                    let isz = th.s(18.0) as u16;
                    let c = if it.on { sk.on } else { sk.text };
                    icons.draw(
                        cv,
                        *icon,
                        x + ((slot - isz as f32) * 0.5).round(),
                        ((h - isz as f32) * 0.5).round(),
                        isz,
                        c,
                        false,
                    );
                }
                Glyph::Text(t) => {
                    let sy = ((h - slot) * 0.5).floor();
                    self.blank(cv, sk, state, x, sy, slot, slot);
                    let c = if it.on { sk.on } else { sk.text };
                    text.draw_in(
                        cv,
                        Font::new(0, th.s(12.0) as u16, 700),
                        x,
                        sy,
                        slot,
                        slot,
                        t,
                        c,
                        Align::Center,
                    );
                }
            }
            if it.open {
                let d = th.s(2.0).max(1.5);
                cv.fill_circle(x + iw * 0.5, h - th.s(2.5), d, sk.on);
            }
            let sy = ((h - slot) * 0.5).floor();
            self.push(hit, x, sy, iw, slot);
            x += iw + gap;
        }
        if x + slot <= strip_end {
            let sy = ((h - slot) * 0.5).floor();
            if self.hover == Hit::Add || self.pressed == Hit::Add {
                let st = idx(self, Hit::Add);
                self.blank(cv, sk, st, x, sy, slot, slot);
            }
            let s = th.s(14.0) as u16;
            icons.draw(
                cv,
                Icon::Plus,
                x + ((slot - s as f32) * 0.5).round(),
                ((h - s as f32) * 0.5).round(),
                s,
                sk.text_dim,
                false,
            );
            self.push(Hit::Add, x, sy, slot, slot);
            x += slot + gap;
        }
        if self.tuner_mode == crate::TunerMode::Slot {
            let tw = crate::slot_width(th.scale);
            if strip_end - x >= tw + th.s(16.0) {
                let tx = (x + (strip_end - x - tw) * 0.5).round();
                self.field(cv, th, sk, Hit::Tuner, tx, by, tw, bh);
                crate::draw_tuner(
                    cv,
                    text,
                    sk.face,
                    th.scale,
                    tx,
                    by,
                    tw,
                    bh,
                    self.tuner.as_ref(),
                    &ink,
                );
                self.push(Hit::Tuner, tx, by, tw, bh);
            }
        }
    }
}
