//! La forme d'onde du fichier choisi : le zoom (jusque sous l'echantillon), la
//! section choisie, la tete de lecture.
//!
//! Tout est en FRAMES du fichier (des f64) : la vue commence a `start` et
//! montre `fpp` frames par pixel. Une section est [a, b) en frames entiers :
//! c'est ce qui sera joue, a l'echantillon pres, jusqu'a 1 frame.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipInfo {
    pub frames: i64,
    pub nch: u32,
    pub srate: f64,
    /// Frames deja decodes (le fichier se decode pendant qu'on l'ecoute).
    pub decoded: i64,
}

impl ClipInfo {
    pub fn complete(&self) -> bool {
        self.decoded >= self.frames
    }
}

#[derive(Clone, Debug, Default)]
pub struct Wave {
    pub clip: u32,
    pub info: ClipInfo,
    pub path: String,
    /// Premiere frame visible (fractionnaire).
    pub start: f64,
    /// Frames par pixel ; 0 = pas encore ajuste a la largeur.
    pub fpp: f64,
    pub width: f32,
    /// La section choisie, [a, b) en frames entiers.
    pub sel: Option<(i64, i64)>,
    /// Le geste en cours : la frame ou il a commence.
    pub drag_from: Option<f64>,
    pub dragged: bool,
    /// La tete de lecture (frames), si une voix joue ce fichier.
    pub playhead: Option<f64>,
    /// Le geste de defilement (molette du milieu) : (x, start).
    pub pan: Option<(f32, f64)>,
}

impl Wave {
    pub fn has(&self) -> bool {
        self.clip != 0 && self.info.frames > 0
    }

    pub fn set_clip(&mut self, clip: u32, info: ClipInfo, path: &str) {
        let same = self.path == path;
        self.clip = clip;
        self.info = info;
        self.path = path.to_string();
        if !same {
            self.sel = None;
            self.fpp = 0.0;
            self.start = 0.0;
        }
        self.playhead = None;
    }

    pub fn clear(&mut self) {
        *self = Wave {
            width: self.width,
            ..Wave::default()
        };
    }

    /// Ajuste la vue a la largeur (tout le fichier visible).
    pub fn fit(&mut self) {
        if self.width > 0.0 && self.info.frames > 0 {
            self.fpp = self.info.frames as f64 / self.width as f64;
            self.start = 0.0;
        }
    }

    pub fn layout(&mut self, width: f32) {
        let refit = self.fpp <= 0.0 || (self.width > 0.0 && self.is_fitted());
        self.width = width.max(1.0);
        if refit {
            self.fit();
        }
        self.clamp();
    }

    fn is_fitted(&self) -> bool {
        self.start <= 0.0 && (self.fpp * self.width as f64 - self.info.frames as f64).abs() < 1.0
    }

    /// La plus petite echelle : 1/64 de frame par pixel (64 px par
    /// echantillon) ; la plus grande : tout le fichier.
    fn min_fpp(&self) -> f64 {
        1.0 / 64.0
    }

    fn max_fpp(&self) -> f64 {
        (self.info.frames as f64 / self.width.max(1.0) as f64).max(self.min_fpp())
    }

    pub fn clamp(&mut self) {
        if !self.has() {
            return;
        }
        self.fpp = self.fpp.clamp(self.min_fpp(), self.max_fpp());
        let span = self.fpp * self.width as f64;
        let max_start = (self.info.frames as f64 - span).max(0.0);
        self.start = self.start.clamp(0.0, max_start);
    }

    pub fn x_to_frame(&self, x: f32) -> f64 {
        self.start + x as f64 * self.fpp
    }

    pub fn frame_to_x(&self, f: f64) -> f32 {
        ((f - self.start) / self.fpp) as f32
    }

    /// Zoom de `notches` crans autour du pixel `x` (positif = plus pres).
    pub fn zoom(&mut self, x: f32, notches: f32) {
        if !self.has() {
            return;
        }
        let anchor = self.x_to_frame(x);
        self.fpp *= 0.8f64.powf(notches as f64);
        self.clamp();
        self.start = anchor - x as f64 * self.fpp;
        self.clamp();
    }

    pub fn pan_px(&mut self, dx: f32) {
        self.start += dx as f64 * self.fpp;
        self.clamp();
    }

    /// Une frame entiere sous le pixel : au zoom extreme, la frame la plus
    /// proche (les points dessines SONT les echantillons).
    pub fn snap(&self, x: f32) -> i64 {
        let f = self.x_to_frame(x);
        let f = if self.fpp < 1.0 { f.round() } else { f.floor() };
        (f as i64).clamp(0, self.info.frames)
    }

    pub fn begin_select(&mut self, x: f32) {
        self.drag_from = Some(self.snap(x) as f64);
        self.dragged = false;
    }

    /// Le geste avance : la section va du point de depart au point courant.
    pub fn drag_select(&mut self, x: f32) {
        let Some(a) = self.drag_from else { return };
        let b = self.snap(x) as f64;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (lo, hi) = (lo as i64, hi as i64);
        if hi > lo {
            self.dragged = true;
            self.sel = Some((lo, hi));
        }
    }

    /// Fin du geste ; rend vrai si c'etait un simple clic (pas de section
    /// tiree).
    pub fn end_select(&mut self) -> bool {
        let click = !self.dragged;
        self.drag_from = None;
        self.dragged = false;
        click
    }

    /// La section en secondes.
    pub fn sel_seconds(&self) -> Option<(f64, f64)> {
        let (a, b) = self.sel?;
        let sr = self.info.srate.max(1.0);
        Some((a as f64 / sr, b as f64 / sr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave() -> Wave {
        let mut w = Wave::default();
        w.set_clip(
            7,
            ClipInfo {
                frames: 48000,
                nch: 2,
                srate: 48000.0,
                decoded: 48000,
            },
            "a.wav",
        );
        w.layout(480.0);
        w
    }

    #[test]
    fn le_zoom_garde_la_frame_sous_la_souris() {
        let mut w = wave();
        assert_eq!(w.fpp, 100.0);
        let f0 = w.x_to_frame(120.0);
        w.zoom(120.0, 3.0);
        assert!((w.x_to_frame(120.0) - f0).abs() < 1e-6);
        // Jusque sous l'echantillon.
        w.zoom(120.0, 60.0);
        assert!((w.fpp - 1.0 / 64.0).abs() < 1e-12);
        w.zoom(120.0, -200.0);
        assert_eq!(w.fpp, 100.0);
        assert_eq!(w.start, 0.0);
    }

    #[test]
    fn une_section_minuscule_tombe_sur_les_echantillons() {
        let mut w = wave();
        // 64 px par echantillon.
        w.fpp = 1.0 / 64.0;
        w.start = 1000.0;
        w.begin_select(10.0); // frame 1000,16 -> 1000
        w.drag_select(70.0); // frame 1001,09 -> 1001
        assert!(!w.end_select());
        assert_eq!(w.sel, Some((1000, 1001)), "une section d'UN echantillon");
        let (a, b) = w.sel_seconds().unwrap();
        assert!((b - a - 1.0 / 48000.0).abs() < 1e-12);
    }
}
