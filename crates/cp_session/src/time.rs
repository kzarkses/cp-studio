//! Le temps de la Session : la grille de lancement (Q) et la ligne de temps
//! qui lie les frames du moteur aux noires.

/// La grille de lancement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Quant {
    /// Tout de suite.
    Off,
    Beat,
    #[default]
    Bar,
    Bar2,
    Bar4,
}

impl Quant {
    pub const ALL: [Quant; 5] = [
        Quant::Off,
        Quant::Beat,
        Quant::Bar,
        Quant::Bar2,
        Quant::Bar4,
    ];

    /// Sa longueur en noires (`qpb` : noires par mesure) ; 0 = sans grille.
    pub fn beats(self, qpb: f64) -> f64 {
        match self {
            Quant::Off => 0.0,
            Quant::Beat => 1.0,
            Quant::Bar => qpb,
            Quant::Bar2 => 2.0 * qpb,
            Quant::Bar4 => 4.0 * qpb,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Quant::Off => "Off",
            Quant::Beat => "Beat",
            Quant::Bar => "Bar",
            Quant::Bar2 => "2 bars",
            Quant::Bar4 => "4 bars",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Quant::Off => "off",
            Quant::Beat => "beat",
            Quant::Bar => "bar",
            Quant::Bar2 => "bar2",
            Quant::Bar4 => "bar4",
        }
    }

    pub fn of_key(s: &str) -> Quant {
        match s {
            "off" => Quant::Off,
            "beat" => Quant::Beat,
            "bar2" => Quant::Bar2,
            "bar4" => Quant::Bar4,
            _ => Quant::Bar,
        }
    }
}

/// D'ou vient l'horloge de la Session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClockMode {
    /// La sienne : elle part au premier lancement, transport arrete compris.
    #[default]
    Free,
    /// Le transport de REAPER (et donc une clock externe).
    Follow,
}

/// La ligne de temps : au frame `f0` du moteur, la noire `b0`, puis `fpb`
/// frames par noire. `running` faux : rien ne bat (Free : la Session se tait ;
/// Follow : le transport est arrete).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    pub f0: i64,
    pub b0: f64,
    pub fpb: f64,
    /// Noires par mesure (signature du projet).
    pub qpb: f64,
    pub running: bool,
}

impl Default for Timeline {
    fn default() -> Timeline {
        Timeline {
            f0: 0,
            b0: 0.0,
            fpb: 24000.0,
            qpb: 4.0,
            running: false,
        }
    }
}

/// Le delai d'un lancement « tout de suite » : de quoi que le moteur recoive
/// la commande avant le frame vise (plusieurs cases lancees ensemble partent
/// au meme echantillon).
pub const LATENCY_S: f64 = 0.02;

impl Timeline {
    /// La noire au frame `f`.
    pub fn beat(&self, f: i64) -> f64 {
        self.b0 + (f - self.f0) as f64 / self.fpb
    }

    /// Le frame de la noire `b`.
    pub fn frame(&self, b: f64) -> i64 {
        self.f0 + ((b - self.b0) * self.fpb).round() as i64
    }

    /// La frontiere visee par un lancement demande au frame `now` : la
    /// prochaine de la grille `q` noires. Un lancement tardif (dans la
    /// tolerance, 1/8 de Q et au plus 0,25 noire) compte pour la frontiere
    /// qui vient de passer. Rend (noire, frame).
    pub fn target(&self, now: i64, q: f64, lat: i64) -> (f64, i64) {
        let b = self.beat(now + lat);
        if q <= 0.0 {
            return (b, now + lat);
        }
        let k = (b / q).floor();
        let prev = k * q;
        let tol = (q / 8.0).min(0.25);
        let t = if b - prev <= tol { prev } else { prev + q };
        (t, self.frame(t))
    }

    /// Le frame de la prochaine frontiere de `q` noires strictement apres
    /// `now + lat` (un arret : sans tolerance).
    pub fn next_after(&self, now: i64, q: f64, lat: i64) -> i64 {
        let b = self.beat(now + lat);
        if q <= 0.0 {
            return now + lat;
        }
        let t = ((b / q).floor() + 1.0) * q;
        self.frame(t)
    }

    /// La prochaine frontiere de `q` noires a partir de la noire `b`
    /// (incluse si `b` y est deja, a un millionieme pres).
    pub fn next_boundary(b: f64, q: f64) -> f64 {
        if q <= 0.0 {
            return b;
        }
        let k = (b / q - 1e-6).ceil();
        k * q
    }
}
