//! CP Session : le lanceur de clips de CP Studio, sans REAPER (testable).
//!
//! - [`clip`] : un clip (MIDI ou audio) et sa forme ecrite (CPC1, celle de
//!   l'ancienne Session) ;
//! - [`time`] : la grille de lancement et la ligne de temps (frames du
//!   moteur <-> noires) ;
//! - [`session`] : la grille et ce qui la fait jouer, au travers d'un
//!   [`Engine`] (le chargeur dans REAPER, un faux dans les essais).
//!
//! Plan et etat : `ROADMAP_Session.md`.

pub mod clip;
pub mod session;
#[cfg(test)]
mod tests;
pub mod time;

pub use clip::{Audio, COLORS, Clip, Kind, Note, TempoMode, TempoWhy};
pub use session::{
    AudioSrc, CellState, ColRun, Column, Engine, Input, LaneClip, Motion, Play, Played, RecPoll,
    Session, Take, TrackInfo, unfold,
};
pub use time::{ClockMode, Quant, Timeline};

/// La position dans un clip apres `u` noires jouees depuis `from` : la
/// premiere passe va de `from` a la fin de la zone qui boucle, puis la zone
/// tourne (la meme regle que la lane du moteur).
pub fn clip_pos(span: (f64, f64), _len: f64, from: f64, u: f64) -> f64 {
    let (a, b) = span;
    let first = (b - from).max(0.0);
    if u < first {
        return from + u;
    }
    let s = (b - a).max(1e-9);
    a + (u - first).rem_euclid(s)
}

// ---------------------------------------------------------------------------
// L'enregistrement dans le projet
// ---------------------------------------------------------------------------

/// La section des donnees de la Session dans le projet (`ProjExtState`).
pub const SECTION: &str = "CP_Session2";

impl Session {
    /// Ce qui s'enregistre dans le projet : (cle, valeur).
    pub fn save(&self) -> Vec<(&'static str, String)> {
        let mut keys: Vec<&(String, usize)> = self.cells.keys().collect();
        keys.sort();
        let mut grid = String::new();
        for k in keys {
            let c = &self.cells[k];
            grid.push_str(&format!("{}\t{}\t{}\n", k.0, k.1, c.to_cpc1()));
        }
        let mut motion = String::new();
        for c in &self.cols {
            if c.motion != Motion::Stay {
                motion.push_str(&format!("{}={};", c.track.guid, c.motion.key()));
            }
        }
        let cfg = format!(
            "v=1|q={}|rec={}|clock={}|scenes={}|next={}",
            self.q.key(),
            self.rec_bars,
            match self.clock {
                ClockMode::Free => "free",
                ClockMode::Follow => "follow",
            },
            self.scenes,
            self.next_id
        );
        vec![("grid", grid), ("cfg", cfg), ("motion", motion)]
    }

    /// Relit ce que `save` a ecrit. Ce qui joue est oublie (un projet qu'on
    /// ouvre ne joue rien).
    pub fn load(&mut self, get: &dyn Fn(&str) -> Option<String>) {
        self.cells.clear();
        self.erased = None;
        if let Some(cfg) = get("cfg") {
            for kv in cfg.split('|') {
                let Some((k, v)) = kv.split_once('=') else {
                    continue;
                };
                match k {
                    "q" => self.q = Quant::of_key(v),
                    "rec" => self.rec_bars = v.parse().unwrap_or(1).clamp(1, 64),
                    "clock" => {
                        self.clock = if v == "follow" {
                            ClockMode::Follow
                        } else {
                            ClockMode::Free
                        }
                    }
                    "scenes" => self.scenes = v.parse().unwrap_or(session::MIN_SCENES),
                    "next" => self.next_id = v.parse().unwrap_or(self.next_id).max(self.next_id),
                    _ => {}
                }
            }
        }
        let mut max_id = 0;
        if let Some(grid) = get("grid") {
            for line in grid.lines() {
                let mut it = line.splitn(3, '\t');
                let (Some(g), Some(s), Some(c)) = (it.next(), it.next(), it.next()) else {
                    continue;
                };
                let Ok(s) = s.parse::<usize>() else { continue };
                let mut next = self.next_id;
                let clip = clip::Clip::from_cpc1(c, &mut || {
                    next += 1;
                    next - 1
                });
                self.next_id = self.next_id.max(next);
                if let Some(cl) = clip {
                    max_id = max_id.max(cl.id);
                    self.cells.insert((g.to_string(), s), cl);
                }
            }
        }
        self.next_id = self.next_id.max(max_id + 1);
        if let Some(m) = get("motion") {
            for kv in m.split(';') {
                let Some((g, v)) = kv.split_once('=') else {
                    continue;
                };
                if let Some(c) = self.cols.iter_mut().find(|c| c.track.guid == g) {
                    c.motion = Motion::of_key(v.parse().unwrap_or(0));
                }
            }
        }
        self.pending_motion = get("motion");
        self.fit_scenes();
    }

    /// La reprise de l'ancienne Session (CP_Session/grid + CP_Loop/dest<n>) :
    /// `grid` = ses lignes `t:s:CPC1...`, `dest(t)` = le GUID de la piste de
    /// la colonne t. Rend le nombre de clips repris.
    pub fn import_old(&mut self, grid: &str, dest: &dyn Fn(usize) -> Option<String>) -> usize {
        let mut n = 0;
        for line in grid.lines() {
            let mut it = line.splitn(3, ':');
            let (Some(t), Some(s), Some(c)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let (Ok(t), Ok(s)) = (t.trim().parse::<usize>(), s.trim().parse::<usize>()) else {
                continue;
            };
            let Some(g) = dest(t) else { continue };
            let mut next = self.next_id;
            let clip = clip::Clip::from_cpc1(c, &mut || {
                next += 1;
                next - 1
            });
            self.next_id = self.next_id.max(next);
            if let Some(mut cl) = clip {
                if self.cells.values().any(|x| x.id == cl.id) {
                    cl.id = self.fresh_id();
                }
                self.next_id = self.next_id.max(cl.id + 1);
                self.cells.insert((g, s), cl);
                n += 1;
            }
        }
        if n > 0 {
            self.dirty += 1;
            self.fit_scenes();
        }
        n
    }

    /// Les suites de lecture lues avant que les colonnes n'existent :
    /// appliquees quand les pistes arrivent.
    pub fn apply_pending_motion(&mut self) {
        let Some(m) = self.pending_motion.take() else {
            return;
        };
        for kv in m.split(';') {
            let Some((g, v)) = kv.split_once('=') else {
                continue;
            };
            if let Some(c) = self.cols.iter_mut().find(|c| c.track.guid == g) {
                c.motion = Motion::of_key(v.parse().unwrap_or(0));
            }
        }
    }
}
