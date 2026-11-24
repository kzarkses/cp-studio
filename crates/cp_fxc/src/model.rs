//! Ce que FX Constellation sait d'une piste : ses parametres suivis (base,
//! portee, axes, inversion, sources), son pad, ses macros. Tout tient dans
//! la piste elle-meme (`P_EXT:CPS_FXC`, voir [`crate::store`]) : le modele
//! voyage avec la piste, l'annulation de REAPER le ramene.
//!
//! Un parametre se nomme par le GUID de son effet et son indice : deux
//! instances du meme effet ne se confondent plus (l'ancienne cle par nom les
//! melangeait).

use std::collections::BTreeMap;

use crate::slots::Pol;

/// Le nom d'un parametre dans le modele : « {GUID de l'effet}#indice ».
pub fn pkey(fx_guid: &str, param: usize) -> String {
    format!("{fx_guid}#{param}")
}

/// L'inverse de [`pkey`].
pub fn split_pkey(k: &str) -> Option<(&str, usize)> {
    let (g, i) = k.rsplit_once('#')?;
    Some((g, i.parse().ok()?))
}

/// Une source d'un parametre.
#[derive(Clone, Debug, PartialEq)]
pub struct Src {
    pub slot: i32,
    /// Profondeur signee (-1..1), relative a `gesture_range`. Pour la
    /// premiere source, la profondeur est la portee du parametre
    /// (`range`, signee par `inv`) : ce champ y est ignore.
    pub depth: f64,
    /// Polarite propre au lien (aucune : celle de la source).
    pub pol: Option<Pol>,
    /// Contournee : la profondeur reste, le lien joue 0.
    pub off: bool,
}

impl Src {
    pub fn new(slot: i32, depth: f64) -> Src {
        Src {
            slot,
            depth: depth.clamp(-1.0, 1.0),
            pol: None,
            off: false,
        }
    }
}

/// Un parametre suivi (ou qui a ete touche).
#[derive(Clone, Debug, PartialEq)]
pub struct PState {
    /// Suivi : le pad, ROLL et la modulation ne touchent que ceux-la.
    pub sel: bool,
    /// La base (normalise de REAPER) : l'ancre du pad, la valeur de repos.
    pub base: f64,
    /// La portee (0..1) : la course quand le pad va d'un bord a l'autre ;
    /// aussi la profondeur de la premiere source.
    pub range: f64,
    pub inv: bool,
    pub x: bool,
    pub y: bool,
    /// 0 a 4 sources ; vide = le pad.
    pub srcs: Vec<Src>,
}

impl Default for PState {
    fn default() -> PState {
        PState {
            sel: false,
            base: 0.5,
            range: 1.0,
            inv: false,
            x: true,
            y: true,
            srcs: Vec::new(),
        }
    }
}

/// Le nombre de sources d'un parametre, au plus.
pub const MAX_SOURCES: usize = 4;

impl PState {
    /// La profondeur signee de la source `i` (0 d'abord), multipliee par
    /// `gesture_range`.
    pub fn span(&self, i: usize, g: f64) -> f64 {
        if i == 0 {
            let s = if self.inv { -1.0 } else { 1.0 };
            self.range * s * g
        } else {
            self.srcs.get(i).map_or(0.0, |s| s.depth * g)
        }
    }

    /// Ce qui compile : 0 si la source est contournee.
    pub fn span_eff(&self, i: usize, g: f64) -> f64 {
        if self.srcs.get(i).is_some_and(|s| s.off) {
            0.0
        } else {
            self.span(i, g)
        }
    }

    /// Change la profondeur de la source `i` (la premiere : la portee et
    /// l'inversion).
    pub fn set_span(&mut self, i: usize, v: f64, g: f64) {
        let g = if g.abs() < 1e-9 { 1.0 } else { g };
        let v = (v / g).clamp(-1.0, 1.0);
        if i == 0 {
            self.range = v.abs();
            self.inv = v < 0.0;
        } else if let Some(s) = self.srcs.get_mut(i) {
            s.depth = v;
        }
    }

    pub fn index_of(&self, slot: i32) -> Option<usize> {
        self.srcs.iter().position(|s| s.slot == slot)
    }

    /// Ajoute une source (la premiere prend la portee ; les suivantes, la
    /// portee aussi : « une valeur qu'on entend deja »). Rend faux si elle y
    /// etait deja ou s'il y en a quatre.
    pub fn add_source(&mut self, slot: i32) -> Result<(), &'static str> {
        if self.index_of(slot).is_some() {
            return Ok(());
        }
        if self.srcs.len() >= MAX_SOURCES {
            return Err("four sources at most on one parameter");
        }
        let d = self.range * if self.inv { -1.0 } else { 1.0 };
        self.srcs.push(Src::new(slot, d));
        self.sel = true;
        Ok(())
    }

    /// Retire la source `i`. Retirer la premiere promeut la deuxieme, avec
    /// sa profondeur.
    pub fn remove_source(&mut self, i: usize) {
        if i >= self.srcs.len() {
            return;
        }
        if i == 0 && self.srcs.len() > 1 {
            let d = self.srcs[1].depth;
            self.range = d.abs();
            self.inv = d < 0.0;
        }
        self.srcs.remove(i);
    }

    /// Le parametre suit le pad (aucune source).
    pub fn on_pad(&self) -> bool {
        self.srcs.is_empty()
    }

    /// Le pad ecrit ce parametre (pastille allumee) : suivi, sans source,
    /// et l'axe est a lui.
    pub fn pad_drives(&self) -> bool {
        self.sel && self.srcs.is_empty() && (self.x || self.y)
    }
}

/// Le mouvement du pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Manual = 0,
    Walk = 1,
    Figures = 2,
}

impl Nav {
    pub fn from_i(i: i64) -> Nav {
        match i {
            1 => Nav::Walk,
            2 => Nav::Figures,
            _ => Nav::Manual,
        }
    }
}

pub const FIGURES: [&str; 6] = ["Circle", "Square", "Triangle", "Diamond", "Z", "Infinity"];
pub const WALK_SYNCS: [&str; 6] = ["Free", "1/16", "1/8", "1/4", "1/2", "1 bar"];

/// Le pad d'une piste (les `PAD_FIELDS` de l'ancienne version, et la
/// position).
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub nav: Nav,
    /// Walk ou Figures tourne (un geste de la souris l'arrete, le mode
    /// reste).
    pub nav_on: bool,
    pub walk_speed: f64,
    pub walk_jitter: f64,
    pub walk_jump: bool,
    pub walk_sync: u8,
    pub fig_mode: u8,
    pub fig_speed: f64,
    pub fig_size: f64,
    /// x0, x1, y0, y1 (0..1).
    pub zone: [f64; 4],
    pub gesture_range: f64,
    pub gmin: f64,
    pub gmax: f64,
    pub link_slew: f64,
    pub smooth: f64,
    pub max_speed: f64,
    /// La source qui mene l'axe X (0 : aucune).
    pub src_x: i32,
    pub src_y: i32,
    pub granular: bool,
    pub grid: u8,
    /// La position (0..1) et l'ancre du geste.
    pub x: f64,
    pub y: f64,
    pub ax: f64,
    pub ay: f64,
}

impl Default for Pad {
    fn default() -> Pad {
        Pad {
            nav: Nav::Manual,
            nav_on: false,
            walk_speed: 2.0,
            walk_jitter: 0.2,
            walk_jump: false,
            walk_sync: 0,
            fig_mode: 0,
            fig_speed: 1.0,
            fig_size: 0.5,
            zone: [0.0, 1.0, 0.0, 1.0],
            gesture_range: 1.0,
            gmin: 0.0,
            gmax: 1.0,
            link_slew: 0.0,
            smooth: 0.0,
            max_speed: 2.0,
            src_x: 0,
            src_y: 0,
            granular: false,
            grid: 3,
            x: 0.5,
            y: 0.5,
            ax: 0.5,
            ay: 0.5,
        }
    }
}

impl Pad {
    /// La zone, rangee (x0 <= x1, y0 <= y1) et bornee.
    pub fn zone_ordered(&self) -> [f64; 4] {
        let c = |v: f64| v.clamp(0.0, 1.0);
        let (a, b) = (c(self.zone[0]), c(self.zone[1]));
        let (p, q) = (c(self.zone[2]), c(self.zone[3]));
        [a.min(b), a.max(b), p.min(q), p.max(q)]
    }

    /// Une zone plus petite que le carre entier existe.
    pub fn has_zone(&self) -> bool {
        let z = self.zone_ordered();
        z[0] > 0.0005 || z[1] < 0.9995 || z[2] > 0.0005 || z[3] < 0.9995
    }

    pub fn clamp_to_zone(&self, x: f64, y: f64) -> (f64, f64) {
        let z = self.zone_ordered();
        (x.clamp(z[0], z[1]), y.clamp(z[2], z[3]))
    }
}

/// Une capture pour le fondu A → B : (GUID de l'effet, indice) → valeur.
pub type Morph = BTreeMap<String, f64>;

/// Un grain du mode granulaire.
#[derive(Clone, Debug, PartialEq)]
pub struct Grain {
    pub x: f64,
    pub y: f64,
    /// Parametre (cle du modele) → valeur.
    pub values: BTreeMap<String, f64>,
}

/// Ce que FX Constellation garde d'une piste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub params: BTreeMap<String, PState>,
    pub pad: Pad,
    pub macro_names: [String; 8],
    /// Le preset charge (vide : aucun).
    pub preset: String,
    pub snapshot: String,
    /// Le nombre de parametres de « Track at random » par effet (GUID).
    pub fx_random_max: BTreeMap<String, u8>,
    /// Les effets montres en entier (GUID → vrai) ; absent = tout montrer.
    pub fx_tracked_only: BTreeMap<String, bool>,
    pub grains: Vec<Grain>,
    pub morph_a: Option<Morph>,
    pub morph_b: Option<Morph>,
}

impl Track {
    pub fn param(&self, k: &str) -> Option<&PState> {
        self.params.get(k)
    }

    pub fn param_mut(&mut self, k: &str) -> &mut PState {
        self.params.entry(k.to_string()).or_default()
    }

    pub fn tracked(&self) -> impl Iterator<Item = (&String, &PState)> {
        self.params.iter().filter(|(_, p)| p.sel)
    }

    pub fn tracked_count(&self) -> usize {
        self.params.values().filter(|p| p.sel).count()
    }

    /// Les parametres menes par une source (pour les tuiles du rack).
    pub fn dest_count(&self, slot: i32) -> usize {
        let mut n = self
            .params
            .values()
            .filter(|p| p.sel && p.srcs.iter().any(|s| s.slot == slot))
            .count();
        if self.pad.src_x == slot {
            n += 1;
        }
        if self.pad.src_y == slot {
            n += 1;
        }
        n
    }

    pub fn macro_name(&self, n: usize) -> String {
        let s = &self.macro_names[n.clamp(1, 8) - 1];
        if s.is_empty() {
            format!("MACRO {n}")
        } else {
            s.clone()
        }
    }

    /// Oublie les parametres d'effets qui ne sont plus sur la piste.
    pub fn prune(&mut self, alive: &dyn Fn(&str) -> bool) {
        self.params
            .retain(|k, _| split_pkey(k).is_some_and(|(g, _)| alive(g)));
        self.fx_random_max.retain(|g, _| alive(g));
        self.fx_tracked_only.retain(|g, _| alive(g));
    }
}
