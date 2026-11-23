//! Le modele de notes de l'editeur MIDI de CP Studio, porte de `Roll.lua`
//! (l'ancien CP_Editor) : une liste de notes en noires du projet, une
//! selection, et les transformations que l'editeur et ses touches appellent.
//!
//! Pur : ni REAPER ni Windows. L'hote lit les notes d'une prise, les donne ici,
//! et reecrit la liste apres chaque geste (un pas d'annulation par geste).
//!
//! Une regle tenue partout : **le canal d'une note ne change jamais** sauf si
//! on le demande. L'ancien modele ecrivait toute note neuve sur le canal 1 ;
//! dupliquer, coller, coller bout a bout ou couper remettaient donc le canal a
//! 1, alors que Guitar Tab y range la corde. Les tests le tiennent.
//!
//! Les operations agissent sur la selection, ou sur toutes les notes quand
//! rien n'est choisi (sauf celles qui disent le contraire).
//!
//! L'expression d'une note (ses pitch bends, sa pedale legato) la suit :
//! chaque note sait de quelle note lue elle vient (`src`) et ou le debut de
//! celle-ci se trouve maintenant (`anchor`) ; voir [`expr`].

pub mod expr;

/// Les gammes : un nom et ses intervalles depuis la tonique.
pub const SCALES: [(&str, &[i32]); 13] = [
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Harmonic minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Melodic minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("Major pentatonic", &[0, 2, 4, 7, 9]),
    ("Minor pentatonic", &[0, 3, 5, 7, 10]),
    ("Blues", &[0, 3, 5, 6, 7, 10]),
];

/// Les noms des notes (dieses).
pub const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// « E3 » : le nom et l'octave (Do 4 = 60).
pub fn note_name(p: i32) -> String {
    format!(
        "{}{}",
        NAMES[p.rem_euclid(12) as usize],
        p.div_euclid(12) - 1
    )
}

/// Une gamme : sa tonique (0 = Do) et son rang dans [`SCALES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    pub root: i32,
    pub kind: usize,
}

impl Scale {
    pub fn intervals(&self) -> &'static [i32] {
        SCALES[self.kind.min(SCALES.len() - 1)].1
    }

    pub fn name(&self) -> String {
        format!(
            "{} {}",
            NAMES[self.root.rem_euclid(12) as usize],
            SCALES[self.kind.min(SCALES.len() - 1)].0.to_lowercase()
        )
    }

    pub fn contains(&self, p: i32) -> bool {
        let d = (p - self.root).rem_euclid(12);
        self.intervals().contains(&d)
    }

    /// La hauteur de la gamme la plus proche (vers le bas a egalite).
    pub fn snap(&self, p: i32) -> i32 {
        for d in 0..12 {
            if self.contains(p - d) {
                return p - d;
            }
            if self.contains(p + d) {
                return p + d;
            }
        }
        p
    }

    /// `steps` degres de la gamme au-dessus (ou au-dessous) de `p`.
    pub fn step(&self, p: i32, steps: i32) -> i32 {
        let mut q = self.snap(p);
        let dir = steps.signum();
        for _ in 0..steps.abs() {
            q += dir;
            while !self.contains(q) {
                q += dir;
            }
        }
        q
    }
}

/// Une note : en noires du projet. `id` reste le meme d'un geste a l'autre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub id: u64,
    pub qn0: f64,
    pub len: f64,
    pub pitch: i32,
    /// Le canal MIDI (0..15). Jamais remis a zero par une operation.
    pub chan: i32,
    pub vel: i32,
    pub sel: bool,
    pub mute: bool,
    /// La note lue dont celle-ci porte l'expression (elle-meme ; une copie,
    /// un morceau coupe : la note d'origine).
    pub src: u64,
    /// Ou le debut de la note `src` se trouve maintenant : un deplacement
    /// l'emmene, un rognage du debut le laisse (la courbe reste au temps ou
    /// elle sonne).
    pub anchor: f64,
}

impl Note {
    pub fn end(&self) -> f64 {
        self.qn0 + self.len
    }

    /// Le debut a `q`, l'expression emmenee avec lui (un deplacement, pas
    /// un rognage).
    pub fn place(&mut self, q: f64) {
        self.anchor += q - self.qn0;
        self.qn0 = q;
    }
}

/// La plus petite longueur d'une note (noires).
pub const MIN_LEN: f64 = 1.0 / 64.0;

const EPS: f64 = 1e-6;

/// Un generateur reproductible (Park-Miller) : un humanize relance avec la
/// meme graine refait exactement la meme chose.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng((seed % 2_147_483_646).max(1))
    }

    /// -1 .. 1
    fn next(&mut self) -> f64 {
        self.0 = (self.0 * 16807) % 2_147_483_647;
        self.0 as f64 / 2_147_483_647.0 * 2.0 - 1.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Roll {
    pub notes: Vec<Note>,
    next: u64,
    /// Le presse-papiers : des notes relatives a la premiere (qn0 = 0).
    clip: Vec<Note>,
    pub scale: Option<Scale>,
}

impl Roll {
    pub fn new() -> Roll {
        Roll::default()
    }

    /// Les notes d'une prise : (debut, longueur, hauteur, canal, velocite,
    /// choisie, muette).
    pub fn from_notes(it: impl IntoIterator<Item = (f64, f64, i32, i32, i32, bool, bool)>) -> Roll {
        let mut r = Roll::new();
        for (qn0, len, pitch, chan, vel, sel, mute) in it {
            let id = r.mint();
            r.notes.push(Note {
                id,
                qn0,
                len: len.max(MIN_LEN),
                pitch: pitch.clamp(0, 127),
                chan: chan.clamp(0, 15),
                vel: vel.clamp(1, 127),
                sel,
                mute,
                src: id,
                anchor: qn0,
            });
        }
        r.sort();
        r
    }

    /// Un numero neuf : une note, ou l'expression a part d'une note.
    pub fn mint(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    pub fn sort(&mut self) {
        self.notes.sort_by(|a, b| {
            a.qn0
                .total_cmp(&b.qn0)
                .then(a.pitch.cmp(&b.pitch))
                .then(a.id.cmp(&b.id))
        });
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    // -----------------------------------------------------------------------
    // La selection
    // -----------------------------------------------------------------------

    pub fn sel_count(&self) -> usize {
        self.notes.iter().filter(|n| n.sel).count()
    }

    pub fn has_sel(&self) -> bool {
        self.notes.iter().any(|n| n.sel)
    }

    pub fn selected(&self) -> Vec<u64> {
        self.notes.iter().filter(|n| n.sel).map(|n| n.id).collect()
    }

    pub fn clear_sel(&mut self) -> bool {
        let had = self.has_sel();
        for n in &mut self.notes {
            n.sel = false;
        }
        had
    }

    pub fn select_only(&mut self, id: u64) {
        for n in &mut self.notes {
            n.sel = n.id == id;
        }
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(n) = self.get_mut(id) {
            n.sel = !n.sel;
        }
    }

    pub fn add_sel(&mut self, id: u64) {
        if let Some(n) = self.get_mut(id) {
            n.sel = true;
        }
    }

    pub fn select_all(&mut self) {
        for n in &mut self.notes {
            n.sel = true;
        }
    }

    pub fn invert_sel(&mut self) {
        for n in &mut self.notes {
            n.sel = !n.sel;
        }
    }

    /// Les notes qui touchent le rectangle [q0, q1] x [p0, p1].
    pub fn select_box(&mut self, q0: f64, q1: f64, p0: i32, p1: i32, additive: bool) -> usize {
        let (qa, qb) = (q0.min(q1), q0.max(q1));
        let (pa, pb) = (p0.min(p1), p0.max(p1));
        let mut k = 0;
        for n in &mut self.notes {
            let hit = n.qn0 < qb && n.end() > qa && n.pitch >= pa && n.pitch <= pb;
            if hit {
                n.sel = true;
                k += 1;
            } else if !additive {
                n.sel = false;
            }
        }
        k
    }

    /// Toute une hauteur.
    pub fn select_row(&mut self, pitch: i32, additive: bool) -> usize {
        let mut k = 0;
        for n in &mut self.notes {
            if n.pitch == pitch {
                n.sel = true;
                k += 1;
            } else if !additive {
                n.sel = false;
            }
        }
        k
    }

    /// Les notes qui commencent dans [q0, q1).
    pub fn select_in(&mut self, q0: f64, q1: f64, additive: bool) -> usize {
        let mut k = 0;
        for n in &mut self.notes {
            if n.qn0 >= q0 - EPS && n.qn0 < q1 - EPS {
                n.sel = true;
                k += 1;
            } else if !additive {
                n.sel = false;
            }
        }
        k
    }

    /// Maj + clic : de la note choisie la plus proche jusqu'a celle-ci, dans
    /// le temps.
    pub fn select_range_to(&mut self, id: u64) {
        let Some(t) = self.get(id).map(|n| n.qn0) else {
            return;
        };
        let anchor = self
            .notes
            .iter()
            .filter(|n| n.sel)
            .map(|n| n.qn0)
            .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()));
        let (a, b) = match anchor {
            Some(a) => (a.min(t), a.max(t)),
            None => (t, t),
        };
        for n in &mut self.notes {
            if n.qn0 >= a - EPS && n.qn0 <= b + EPS {
                n.sel = true;
            }
        }
    }

    /// Les notes de la mesure de `qn` (mesures de `bar` noires depuis 0).
    pub fn select_measure(&mut self, qn: f64, bar: f64) -> usize {
        let b0 = (qn / bar).floor() * bar;
        self.select_in(b0, b0 + bar, false)
    }

    /// Cette note et toutes les suivantes.
    pub fn select_from(&mut self, qn: f64) -> usize {
        self.select_in(qn, f64::INFINITY, false)
    }

    /// Les notes de meme hauteur que la selection.
    pub fn select_same_pitch(&mut self) -> usize {
        let ps: Vec<i32> = self
            .notes
            .iter()
            .filter(|n| n.sel)
            .map(|n| n.pitch)
            .collect();
        let mut k = 0;
        for n in &mut self.notes {
            n.sel = ps.contains(&n.pitch);
            k += n.sel as usize;
        }
        k
    }

    /// Les cibles d'une operation : la selection, sinon tout.
    fn targets(&self) -> Vec<usize> {
        let any = self.has_sel();
        (0..self.notes.len())
            .filter(|&i| !any || self.notes[i].sel)
            .collect()
    }

    /// Le debut et la fin des cibles.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        let t = self.targets();
        if t.is_empty() {
            return None;
        }
        let a = t
            .iter()
            .map(|&i| self.notes[i].qn0)
            .fold(f64::INFINITY, f64::min);
        let b = t
            .iter()
            .map(|&i| self.notes[i].end())
            .fold(f64::NEG_INFINITY, f64::max);
        Some((a, b))
    }

    // -----------------------------------------------------------------------
    // Poser, effacer
    // -----------------------------------------------------------------------

    /// Une note neuve (non choisie) ; rend son identite.
    pub fn insert(&mut self, qn0: f64, len: f64, pitch: i32, chan: i32, vel: i32) -> u64 {
        let id = self.mint();
        let qn0 = qn0.max(0.0);
        self.notes.push(Note {
            id,
            qn0,
            len: len.max(MIN_LEN),
            pitch: pitch.clamp(0, 127),
            chan: chan.clamp(0, 15),
            vel: vel.clamp(1, 127),
            sel: false,
            mute: false,
            src: id,
            anchor: qn0,
        });
        self.sort();
        id
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let n = self.notes.len();
        self.notes.retain(|x| x.id != id);
        self.notes.len() != n
    }

    pub fn delete_sel(&mut self) -> usize {
        let n = self.notes.len();
        self.notes.retain(|x| !x.sel);
        n - self.notes.len()
    }

    // -----------------------------------------------------------------------
    // Deplacer, allonger
    // -----------------------------------------------------------------------

    /// Deplace la selection de `dq` noires et `dp` demi-tons. Avec une gamme,
    /// les hauteurs restent dedans.
    pub fn move_sel(&mut self, dq: f64, dp: i32) {
        let scale = self.scale;
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            n.place((n.qn0 + dq).max(0.0));
            let p = (n.pitch + dp).clamp(0, 127);
            n.pitch = match scale {
                Some(s) if dp != 0 => s.snap(p).clamp(0, 127),
                _ => p,
            };
        }
        self.sort();
    }

    /// Deplace la selection sans tenir compte de la gamme (REAPER : « ignoring
    /// scale/key »).
    pub fn move_sel_free(&mut self, dq: f64, dp: i32) {
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            n.place((n.qn0 + dq).max(0.0));
            n.pitch = (n.pitch + dp).clamp(0, 127);
        }
        self.sort();
    }

    /// Rend muettes les cibles (`Some(true)`), les rallume (`Some(false)`), ou
    /// bascule (`None`, sur l'etat de la premiere).
    pub fn mute_sel(&mut self, m: Option<bool>) -> usize {
        let t = self.targets();
        let Some(&first) = t.first() else { return 0 };
        let v = m.unwrap_or(!self.notes[first].mute);
        for &i in &t {
            self.notes[i].mute = v;
        }
        t.len()
    }

    /// Coupe les cibles sur chaque ligne de grille qu'elles enjambent.
    pub fn split_grid(&mut self, grid: f64) -> usize {
        if grid <= EPS {
            return 0;
        }
        let t = self.targets();
        let mut add = Vec::new();
        for &i in &t {
            let n = self.notes[i];
            let mut q = ((n.qn0 / grid).floor() + 1.0) * grid;
            let mut start = n.qn0;
            let mut first = true;
            while q < n.end() - EPS {
                if q > start + EPS {
                    if first {
                        self.notes[i].len = q - n.qn0;
                        first = false;
                    } else {
                        add.push(Note {
                            qn0: start,
                            len: q - start,
                            ..n
                        });
                    }
                    start = q;
                }
                q += grid;
            }
            if !first {
                add.push(Note {
                    qn0: start,
                    len: n.end() - start,
                    ..n
                });
            }
        }
        let k = add.len();
        for mut n in add {
            n.id = self.mint();
            self.notes.push(n);
        }
        self.sort();
        k
    }

    /// Les debuts des notes choisies, ecartes selon leur hauteur (REAPER :
    /// « stretch note positions (arpeggiate) ») : la note `id` avance de `dq`,
    /// la plus eloignee d'elle en hauteur reste, les autres suivent au prorata.
    pub fn arpeggiate(&mut self, id: u64, dq: f64) {
        let Some(g) = self.get(id).copied() else {
            return;
        };
        let sel: Vec<i32> = self
            .notes
            .iter()
            .filter(|n| n.sel)
            .map(|n| n.pitch)
            .collect();
        let (lo, hi) = (
            sel.iter().copied().min().unwrap_or(g.pitch),
            sel.iter().copied().max().unwrap_or(g.pitch),
        );
        // Le pivot : l'extremite opposee a la note saisie.
        let pivot = if g.pitch - lo >= hi - g.pitch { lo } else { hi };
        let span = (g.pitch - pivot) as f64;
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            let t = if span.abs() < 0.5 {
                if n.id == id { 1.0 } else { 0.0 }
            } else {
                (n.pitch - pivot) as f64 / span
            };
            n.place((n.qn0 + dq * t).max(0.0));
        }
        self.sort();
    }

    /// Allonge (ou raccourcit) la selection de `dlen` noires.
    pub fn resize_sel(&mut self, dlen: f64) {
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            n.len = (n.len + dlen).max(MIN_LEN);
        }
    }

    pub fn set_len(&mut self, len: f64) -> usize {
        let t = self.targets();
        for &i in &t {
            self.notes[i].len = len.max(MIN_LEN);
        }
        t.len()
    }

    /// Le debut de la selection bouge, la fin reste.
    pub fn trim_start_sel(&mut self, dq: f64) {
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            let end = n.end();
            let q = (n.qn0 + dq).clamp(0.0, end - MIN_LEN);
            n.qn0 = q;
            n.len = end - q;
        }
        self.sort();
    }

    /// Etire la selection autour de `anchor` (positions et longueurs).
    pub fn stretch_sel(&mut self, anchor: f64, k: f64) {
        let k = k.max(0.01);
        for n in self.notes.iter_mut().filter(|n| n.sel) {
            n.place((anchor + (n.qn0 - anchor) * k).max(0.0));
            n.len = (n.len * k).max(MIN_LEN);
        }
        self.sort();
    }

    /// Double ou divise les longueurs des cibles.
    pub fn scale_len(&mut self, k: f64) -> usize {
        let t = self.targets();
        for &i in &t {
            self.notes[i].len = (self.notes[i].len * k).max(MIN_LEN);
        }
        t.len()
    }

    pub fn transpose(&mut self, d: i32) -> usize {
        let t = self.targets();
        for &i in &t {
            self.notes[i].pitch = (self.notes[i].pitch + d).clamp(0, 127);
        }
        self.sort();
        t.len()
    }

    /// Des degres de la gamme (sans gamme : des demi-tons).
    pub fn transpose_scale(&mut self, steps: i32) -> usize {
        let Some(s) = self.scale else {
            return self.transpose(steps);
        };
        let t = self.targets();
        for &i in &t {
            self.notes[i].pitch = s.step(self.notes[i].pitch, steps).clamp(0, 127);
        }
        self.sort();
        t.len()
    }

    pub fn nudge(&mut self, dq: f64) -> usize {
        let t = self.targets();
        for &i in &t {
            let q = (self.notes[i].qn0 + dq).max(0.0);
            self.notes[i].place(q);
        }
        self.sort();
        t.len()
    }

    pub fn snap_to_scale(&mut self) -> usize {
        let Some(s) = self.scale else { return 0 };
        let t = self.targets();
        let mut k = 0;
        for &i in &t {
            let p = s.snap(self.notes[i].pitch);
            if p != self.notes[i].pitch {
                self.notes[i].pitch = p;
                k += 1;
            }
        }
        self.sort();
        k
    }

    // -----------------------------------------------------------------------
    // Le temps
    // -----------------------------------------------------------------------

    /// Cale les debuts sur la grille de `grid` noires, a `strength` (0..1).
    /// `swing` (0..1) retarde les pas impairs d'autant de demi-pas.
    pub fn quantize(&mut self, grid: f64, strength: f64, swing: f64) -> usize {
        if grid <= 0.0 {
            return 0;
        }
        let t = self.targets();
        let mut k = 0;
        for &i in &t {
            let s = self.notes[i].qn0;
            let step = (s / grid).round();
            let mut q = step * grid;
            if (step as i64).rem_euclid(2) == 1 {
                q += swing.clamp(0.0, 1.0) * grid * 0.5;
            }
            let ns = (s + (q - s) * strength.clamp(0.0, 1.0)).max(0.0);
            if (ns - s).abs() > EPS {
                self.notes[i].place(ns);
                k += 1;
            }
        }
        self.sort();
        k
    }

    /// Chaque cible dure jusqu'au debut suivant (toutes notes confondues).
    pub fn legato(&mut self) -> usize {
        let mut starts: Vec<f64> = self.notes.iter().map(|n| n.qn0).collect();
        starts.sort_by(f64::total_cmp);
        starts.dedup_by(|a, b| (*a - *b).abs() < EPS);
        let t = self.targets();
        let mut k = 0;
        for &i in &t {
            let s = self.notes[i].qn0;
            if let Some(&ns) = starts.iter().find(|&&x| x > s + EPS) {
                let nl = ns - s;
                if (nl - self.notes[i].len).abs() > EPS {
                    self.notes[i].len = nl.max(MIN_LEN);
                    k += 1;
                }
            }
        }
        k
    }

    /// Le temps des cibles a l'envers, dans leur propre etendue.
    pub fn reverse(&mut self) -> usize {
        let Some((a, b)) = self.bounds() else {
            return 0;
        };
        let t = self.targets();
        for &i in &t {
            let n = &mut self.notes[i];
            n.place(a + (b - n.end()));
        }
        self.sort();
        t.len()
    }

    /// Les hauteurs en miroir autour du milieu des cibles.
    pub fn invert_pitch(&mut self) -> usize {
        let t = self.targets();
        if t.is_empty() {
            return 0;
        }
        let lo = t.iter().map(|&i| self.notes[i].pitch).min().unwrap_or(0);
        let hi = t.iter().map(|&i| self.notes[i].pitch).max().unwrap_or(0);
        for &i in &t {
            self.notes[i].pitch = (hi + lo - self.notes[i].pitch).clamp(0, 127);
        }
        self.sort();
        t.len()
    }

    // -----------------------------------------------------------------------
    // La velocite
    // -----------------------------------------------------------------------

    pub fn vel_set(&mut self, v: i32) -> usize {
        let t = self.targets();
        for &i in &t {
            self.notes[i].vel = v.clamp(1, 127);
        }
        t.len()
    }

    /// Le meme ecart pour toutes : la dynamique de la phrase reste.
    pub fn vel_add(&mut self, d: i32) -> usize {
        let t = self.targets();
        for &i in &t {
            self.notes[i].vel = (self.notes[i].vel + d).clamp(1, 127);
        }
        t.len()
    }

    /// Comprime (k < 1) ou etend (k > 1) autour de `pivot`.
    pub fn vel_scale(&mut self, k: f64, pivot: i32) -> usize {
        let t = self.targets();
        for &i in &t {
            let v = pivot as f64 + (self.notes[i].vel - pivot) as f64 * k;
            self.notes[i].vel = (v.round() as i32).clamp(1, 127);
        }
        t.len()
    }

    /// De `v0` a `v1` dans l'ordre du temps.
    pub fn vel_ramp(&mut self, v0: i32, v1: i32) -> usize {
        let t = self.targets();
        if t.is_empty() {
            return 0;
        }
        // Du premier debut au dernier : la derniere note prend v1.
        let a = t
            .iter()
            .map(|&i| self.notes[i].qn0)
            .fold(f64::INFINITY, f64::min);
        let b = t
            .iter()
            .map(|&i| self.notes[i].qn0)
            .fold(f64::NEG_INFINITY, f64::max);
        let span = (b - a).max(EPS);
        for &i in &t {
            let x = (self.notes[i].qn0 - a) / span;
            let v = v0 as f64 + (v1 - v0) as f64 * x;
            self.notes[i].vel = (v.round() as i32).clamp(1, 127);
        }
        t.len()
    }

    /// Du jeu : le temps (noires) et la velocite, reproductibles par `seed`.
    pub fn humanize(&mut self, seed: u64, t_amt: f64, v_amt: i32) -> usize {
        let mut rng = Rng::new(seed);
        let t = self.targets();
        for &i in &t {
            let dt = rng.next() * t_amt;
            let dv = (rng.next() * v_amt as f64).round() as i32;
            let n = &mut self.notes[i];
            n.place((n.qn0 + dt).max(0.0));
            n.vel = (n.vel + dv).clamp(1, 127);
        }
        self.sort();
        t.len()
    }

    // -----------------------------------------------------------------------
    // Copier, dupliquer, coller bout a bout, couper
    // -----------------------------------------------------------------------

    /// De combien une duplication decale la copie : l'unite que la selection
    /// remplit deja (la mesure, sinon le temps, sinon le pas de grille), le
    /// tout arrondi au-dessus. Quatre doubles croches sur le temps 1 se
    /// repetent sur le temps 2 ; une mesure pleine, a la mesure suivante.
    pub fn dup_offset(&self, bar: f64, grid: f64) -> f64 {
        let Some((a, b)) = self.bounds() else {
            return 0.0;
        };
        let span = b - a;
        let ceil = |unit: f64| ((span / unit - EPS).ceil().max(1.0)) * unit;
        if bar > 0.0 && span >= bar - EPS {
            return ceil(bar);
        }
        if span >= 1.0 - EPS {
            return ceil(1.0);
        }
        if grid > 0.0 {
            return ceil(grid);
        }
        if span > 0.0 { span } else { 1.0 }
    }

    /// Copie les cibles a `dq` plus loin ; la copie devient la selection.
    pub fn duplicate(&mut self, dq: f64, dp: i32) -> usize {
        let t = self.targets();
        let src: Vec<Note> = t.iter().map(|&i| self.notes[i]).collect();
        for n in &mut self.notes {
            n.sel = false;
        }
        for s in &src {
            let id = self.mint();
            let mut n = Note {
                id,
                pitch: (s.pitch + dp).clamp(0, 127),
                sel: true,
                ..*s
            };
            n.place((s.qn0 + dq).max(0.0));
            self.notes.push(n);
        }
        self.sort();
        src.len()
    }

    /// Pose une copie des notes choisies a leur place (le copier-glisser :
    /// l'original part avec la souris, la copie reste).
    pub fn stamp_sel(&mut self) -> usize {
        let src: Vec<Note> = self.notes.iter().filter(|n| n.sel).copied().collect();
        for s in &src {
            let id = self.mint();
            self.notes.push(Note {
                id,
                sel: false,
                ..*s
            });
        }
        self.sort();
        src.len()
    }

    pub fn copy(&mut self) -> usize {
        let sel: Vec<Note> = self.notes.iter().filter(|n| n.sel).copied().collect();
        if sel.is_empty() {
            return 0;
        }
        let a = sel.iter().map(|n| n.qn0).fold(f64::INFINITY, f64::min);
        self.clip = sel
            .into_iter()
            .map(|mut n| {
                n.place(n.qn0 - a);
                n
            })
            .collect();
        self.clip.len()
    }

    pub fn cut(&mut self) -> usize {
        let k = self.copy();
        if k > 0 {
            self.delete_sel();
        }
        k
    }

    pub fn has_clip(&self) -> bool {
        !self.clip.is_empty()
    }

    /// Colle a `at` ; la copie devient la selection.
    pub fn paste(&mut self, at: f64) -> usize {
        let clip = self.clip.clone();
        for n in &mut self.notes {
            n.sel = false;
        }
        for c in &clip {
            let id = self.mint();
            let mut n = Note {
                id,
                sel: true,
                ..*c
            };
            n.place((at + c.qn0).max(0.0));
            self.notes.push(n);
        }
        self.sort();
        clip.len()
    }

    /// Colle bout a bout les notes choisies de meme hauteur et de meme canal
    /// qui se touchent ; la velocite est leur moyenne.
    pub fn glue(&mut self) -> usize {
        let mut sel: Vec<Note> = self.notes.iter().filter(|n| n.sel).copied().collect();
        if sel.len() < 2 {
            return 0;
        }
        sel.sort_by(|a, b| {
            a.chan
                .cmp(&b.chan)
                .then(a.pitch.cmp(&b.pitch))
                .then(a.qn0.total_cmp(&b.qn0))
        });
        let mut runs: Vec<(Note, f64, i32, i32)> = Vec::new(); // (premiere, fin, somme vel, nombre)
        for n in sel {
            match runs.last_mut() {
                Some((f, end, sv, k))
                    if f.pitch == n.pitch && f.chan == n.chan && n.qn0 <= *end + EPS =>
                {
                    *end = end.max(n.end());
                    *sv += n.vel;
                    *k += 1;
                }
                _ => {
                    let e = n.end();
                    runs.push((n, e, n.vel, 1));
                }
            }
        }
        self.delete_sel();
        for (f, end, sv, k) in &runs {
            let id = self.mint();
            self.notes.push(Note {
                id,
                len: end - f.qn0,
                vel: ((*sv as f64 / *k as f64).round() as i32).clamp(1, 127),
                sel: true,
                ..*f
            });
        }
        self.sort();
        runs.len()
    }

    /// Coupe en deux les cibles qui enjambent `at`.
    pub fn split_at(&mut self, at: f64) -> usize {
        let t = self.targets();
        let mut add = Vec::new();
        for &i in &t {
            let n = self.notes[i];
            if n.qn0 < at - EPS && n.end() > at + EPS {
                self.notes[i].len = at - n.qn0;
                add.push(Note {
                    qn0: at,
                    len: n.end() - at,
                    ..n
                });
            }
        }
        let k = add.len();
        for mut n in add {
            n.id = self.mint();
            self.notes.push(n);
        }
        self.sort();
        k
    }
}

/// Les accords que l'on pose sur une note : (nom, intervalles depuis
/// elle). « Triad » suit la gamme posee (deux degres, deux degres : un
/// accord mineur sur le second degre d'une majeure), sinon c'est l'accord
/// majeur.
pub const CHORDS: [(&str, &[i32]); 9] = [
    ("Triad", &[0, 4, 7]),
    ("Major", &[0, 4, 7]),
    ("Minor", &[0, 3, 7]),
    ("7", &[0, 4, 7, 10]),
    ("Maj7", &[0, 4, 7, 11]),
    ("Min7", &[0, 3, 7, 10]),
    ("Sus4", &[0, 5, 7]),
    ("Power", &[0, 7, 12]),
    ("Octave", &[0, 12]),
];

/// L'ordre d'un arpege.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
}

impl Roll {
    // -----------------------------------------------------------------------
    // Les generateurs (portes de Roll.lua) : ils gardent le canal
    // -----------------------------------------------------------------------

    /// Les hauteurs d'un accord pose sur `p` (la gamme : « Triad » monte de
    /// deux degres deux fois ; les autres sont calees dans la gamme).
    fn chord_on(&self, p: i32, k: usize) -> Vec<i32> {
        let (name, iv) = CHORDS[k.min(CHORDS.len() - 1)];
        if name == "Triad" {
            if let Some(s) = self.scale {
                let mut v = vec![p];
                let mut q = p;
                for _ in 0..2 {
                    let mut steps = 0;
                    while q < 127 && steps < 2 {
                        q += 1;
                        if s.contains(q) {
                            steps += 1;
                        }
                    }
                    v.push(q);
                }
                return v;
            }
        }
        iv.iter()
            .map(|&i| match self.scale {
                Some(s) if i != 0 && i != 12 => s.snap(p + i),
                _ => p + i,
            })
            .filter(|q| (0..=127).contains(q))
            .collect()
    }

    /// Chaque cible devient un accord (`CHORDS[k]`) : les notes au-dessus
    /// s'ajoutent, meme debut, meme longueur, meme canal, meme velocite.
    /// L'accord devient la selection.
    pub fn chordify(&mut self, k: usize) -> usize {
        let t = self.targets();
        let base: Vec<Note> = t.iter().map(|&i| self.notes[i]).collect();
        for n in &mut self.notes {
            n.sel = false;
        }
        let mut made = 0;
        for b in &base {
            for q in self.chord_on(b.pitch, k) {
                if let Some(x) = self
                    .notes
                    .iter_mut()
                    .find(|x| x.pitch == q && (x.qn0 - b.qn0).abs() < EPS && x.chan == b.chan)
                {
                    x.sel = true;
                    continue;
                }
                let id = self.mint();
                self.notes.push(Note {
                    id,
                    pitch: q,
                    sel: true,
                    src: id,
                    anchor: b.qn0,
                    ..*b
                });
                made += 1;
            }
        }
        self.sort();
        made
    }

    /// Les notes choisies (un accord) deviennent un arpege qui remplit leur
    /// etendue : une note tous les `rate` noires, tenue `gate` de son pas,
    /// sur `octaves` octaves, dans l'ordre `mode` (`seed` : le hasard
    /// reproductible). Le canal et la velocite sont ceux de l'accord (la
    /// premiere note, la moyenne). Rend le nombre de notes.
    pub fn arp(&mut self, rate: f64, mode: ArpMode, gate: f64, octaves: u32, seed: u64) -> usize {
        if rate <= EPS {
            return 0;
        }
        let sel: Vec<Note> = self.notes.iter().filter(|n| n.sel).copied().collect();
        let Some(first) = sel.first().copied() else {
            return 0;
        };
        let a = sel.iter().map(|n| n.qn0).fold(f64::INFINITY, f64::min);
        let b = sel
            .iter()
            .map(|n| n.end())
            .fold(f64::NEG_INFINITY, f64::max);
        if b <= a {
            return 0;
        }
        let vel = (sel.iter().map(|n| n.vel as f64).sum::<f64>() / sel.len() as f64).round() as i32;
        let mut ps: Vec<i32> = sel.iter().map(|n| n.pitch).collect();
        ps.sort_unstable();
        ps.dedup();
        let base = ps.clone();
        for o in 1..octaves.max(1) {
            ps.extend(base.iter().map(|p| p + 12 * o as i32).filter(|p| *p <= 127));
        }
        ps.sort_unstable();
        ps.dedup();
        let seq: Vec<i32> = match mode {
            ArpMode::Up | ArpMode::Random => ps.clone(),
            ArpMode::Down => ps.iter().rev().copied().collect(),
            ArpMode::UpDown => {
                let mut v = ps.clone();
                if ps.len() > 2 {
                    v.extend(ps[1..ps.len() - 1].iter().rev());
                }
                v
            }
        };
        self.delete_sel();
        let mut rng = Rng::new(seed.wrapping_mul(40503).wrapping_add(1));
        let mut t = a;
        let mut k = 0usize;
        let mut made = 0;
        while t < b - 1e-6 && made < 512 {
            let p = match mode {
                ArpMode::Random => {
                    let i =
                        (((rng.next() + 1.0) * 0.5 * seq.len() as f64) as usize).min(seq.len() - 1);
                    seq[i]
                }
                _ => seq[k % seq.len()],
            };
            let len = (rate * gate.clamp(0.05, 1.0)).min(b - t);
            if len > 1e-4 {
                let id = self.mint();
                self.notes.push(Note {
                    id,
                    qn0: t,
                    len: len.max(MIN_LEN),
                    pitch: p.clamp(0, 127),
                    vel: vel.clamp(1, 127),
                    sel: true,
                    mute: false,
                    src: id,
                    anchor: t,
                    ..first
                });
                made += 1;
            }
            t += rate;
            k += 1;
        }
        self.sort();
        made
    }

    /// Un rythme euclidien : `pulses` coups repartis au plus egal sur `steps`
    /// cases de `[a, b)`, a la hauteur `pitch` sur le canal `chan`, decale
    /// de `rotation` cases (le premier coup sur le temps). Les notes
    /// s'ajoutent et deviennent la selection.
    #[allow(clippy::too_many_arguments)]
    pub fn euclid(
        &mut self,
        a: f64,
        b: f64,
        pitch: i32,
        chan: i32,
        vel: i32,
        steps: usize,
        pulses: usize,
        rotation: usize,
    ) -> usize {
        if steps == 0 || b <= a {
            return 0;
        }
        let pulses = pulses.min(steps);
        let cell = (b - a) / steps as f64;
        for n in &mut self.notes {
            n.sel = false;
        }
        let mut bucket = steps - pulses;
        let mut made = 0;
        for i in 0..steps {
            bucket += pulses;
            if bucket >= steps {
                bucket -= steps;
                let si = (i + rotation) % steps;
                let q = a + si as f64 * cell;
                // Une note deja la, a la meme hauteur et au meme instant (la
                // note choisie qui a donne la hauteur) : la garder, pas la
                // doubler.
                let same = self
                    .notes
                    .iter()
                    .find(|n| n.pitch == pitch && n.chan == chan && (n.qn0 - q).abs() < 1e-9)
                    .map(|n| n.id);
                let id = match same {
                    Some(id) => id,
                    None => {
                        made += 1;
                        self.insert(q, cell * 0.9, pitch, chan, vel)
                    }
                };
                self.add_sel(id);
            }
        }
        made
    }
}

#[cfg(test)]
mod tests;
