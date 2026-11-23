//! Un clip de la Session, et sa forme ecrite (CPC1, celle de l'ancienne
//! Session : `CPC1|kind=..|cle=valeur|...`, les caracteres reserves en
//! `%XX`, une cle inconnue ignoree).

use std::fmt::Write as _;

/// Les couleurs des clips (1..8), celles de l'ancienne Session.
pub const COLORS: [(&str, (u8, u8, u8)); 8] = [
    ("Red", (0xE0, 0x5A, 0x4F)),
    ("Orange", (0xE8, 0x8E, 0x3C)),
    ("Yellow", (0xE3, 0xC2, 0x3D)),
    ("Green", (0x6C, 0xC0, 0x5A)),
    ("Teal", (0x3F, 0xB8, 0xA8)),
    ("Blue", (0x4C, 0x8E, 0xE6)),
    ("Violet", (0x95, 0x6C, 0xD8)),
    ("Pink", (0xDC, 0x6C, 0xB0)),
];

/// Une note, en noires depuis le debut du clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub start: f64,
    pub len: f64,
    pub pitch: u8,
    pub vel: u8,
    /// 0..15 (une note de Guitar Tab garde sa corde).
    pub chan: u8,
    /// Probabilite en % (100 : toujours).
    pub prob: u8,
}

/// Comment une case audio suit le tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TempoMode {
    /// La vitesse change, la hauteur avec (le defaut).
    #[default]
    Repitch,
    /// La duree change, pas la hauteur : la region est cuite par l'etireur.
    Stretch,
    /// Le fichier garde sa vitesse.
    None,
}

impl TempoMode {
    pub fn key(self) -> &'static str {
        match self {
            TempoMode::Repitch => "repitch",
            TempoMode::Stretch => "stretch",
            TempoMode::None => "none",
        }
    }
    pub fn of_key(s: &str) -> TempoMode {
        match s {
            "stretch" => TempoMode::Stretch,
            "none" => TempoMode::None,
            _ => TempoMode::Repitch,
        }
    }
}

/// Une case audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub path: String,
    /// La region, en secondes de la source (`len` = 0 : jusqu'au bout).
    pub offs: f64,
    pub len: f64,
    /// Le tempo declare de la source (sinon il se cherche).
    pub src_bpm: Option<f64>,
    pub tempo: TempoMode,
    pub gain: f64,
    /// Faux : la region rejoue une fois par passe (one-shot) ; vrai : la
    /// matiere boucle sur la region.
    pub looped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Midi(Vec<Note>),
    Audio(Audio),
}

/// Un clip : ce que porte une case.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// L'identite (une copie en recoit une neuve).
    pub id: u64,
    pub name: String,
    /// 1..8 (voir [`COLORS`]), `None` : la couleur de la piste.
    pub color: Option<u8>,
    /// La passe, en mesures (des fractions jusqu'au 1/16).
    pub bars: f64,
    /// La signature du clip ; `None` : celle du projet.
    pub sig: Option<(u32, u32)>,
    /// L'accolade, en noires ; `loop_b <= loop_a` : le clip entier.
    pub loop_a: f64,
    pub loop_b: f64,
    /// Verrou de phase : la case entre la ou en serait une boucle lancee au
    /// temps zero ; sinon elle part de son debut.
    pub phase_lock: bool,
    pub kind: Kind,
}

impl Clip {
    pub fn midi(id: u64, bars: f64, notes: Vec<Note>) -> Clip {
        Clip {
            id,
            name: String::new(),
            color: None,
            bars,
            sig: None,
            loop_a: 0.0,
            loop_b: 0.0,
            phase_lock: false,
            kind: Kind::Midi(notes),
        }
    }

    pub fn audio(id: u64, path: &str) -> Clip {
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Clip {
            id,
            name,
            color: None,
            bars: 0.0,
            sig: None,
            loop_a: 0.0,
            loop_b: 0.0,
            phase_lock: false,
            kind: Kind::Audio(Audio {
                path: path.to_string(),
                offs: 0.0,
                len: 0.0,
                src_bpm: None,
                tempo: TempoMode::Repitch,
                gain: 1.0,
                looped: true,
            }),
        }
    }

    pub fn is_midi(&self) -> bool {
        matches!(self.kind, Kind::Midi(_))
    }

    pub fn notes(&self) -> &[Note] {
        match &self.kind {
            Kind::Midi(n) => n,
            Kind::Audio(_) => &[],
        }
    }

    /// Les noires d'une mesure du clip (`qpb` : celles du projet).
    pub fn qpb(&self, qpb: f64) -> f64 {
        match self.sig {
            Some((n, d)) if n > 0 && d > 0 => n as f64 * 4.0 / d as f64,
            _ => qpb,
        }
    }

    /// La longueur de la passe, en noires.
    pub fn len_beats(&self, qpb: f64) -> f64 {
        let b = if self.bars > 0.0 { self.bars } else { 1.0 };
        b * self.qpb(qpb)
    }

    /// La zone qui boucle, en noires : l'accolade si elle tient, sinon la
    /// passe entiere.
    pub fn span(&self, qpb: f64) -> (f64, f64) {
        let len = self.len_beats(qpb);
        let a = self.loop_a.max(0.0);
        let b = self.loop_b.min(len);
        if b > a + 1e-9 { (a, b) } else { (0.0, len) }
    }

    /// Un clip qui n'a rien a jouer (MIDI sans note).
    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Midi(n) => n.is_empty(),
            Kind::Audio(a) => a.path.is_empty(),
        }
    }

    /// La couleur a dessiner, si le clip en a une.
    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        let c = self.color?;
        COLORS.get((c as usize).checked_sub(1)?).map(|c| c.1)
    }
}

// ---------------------------------------------------------------------------
// La forme ecrite
// ---------------------------------------------------------------------------

/// `%`, les separateurs et les fins de ligne deviennent `%XX`.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '%' | '|' | '=' | ';' | ',' | '\n' | '\r' | '\t' => {
                let _ = write!(o, "%{:02X}", ch as u32);
            }
            c => o.push(c),
        }
    }
    o
}

pub fn unesc(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(a), Some(c)) = (h(b[i + 1]), h(b[i + 2])) {
                out.push((a * 16 + c) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn num(v: f64) -> String {
    // Court et exact pour les valeurs usuelles (0.25, 4, 1.5).
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

impl Clip {
    /// La forme ecrite (une ligne, sans tabulation ni fin de ligne).
    pub fn to_cpc1(&self) -> String {
        let mut o = String::from("CPC1");
        let mut kv = |k: &str, v: &str| {
            o.push('|');
            o.push_str(k);
            o.push('=');
            o.push_str(v);
        };
        kv(
            "kind",
            match self.kind {
                Kind::Midi(_) => "midi",
                Kind::Audio(_) => "audio",
            },
        );
        kv("id", &self.id.to_string());
        if !self.name.is_empty() {
            kv("name", &esc(&self.name));
        }
        if let Some(c) = self.color {
            kv("color", &c.to_string());
        }
        kv("bars", &num(self.bars));
        if let Some((n, d)) = self.sig {
            kv("tsnum", &n.to_string());
            kv("tsden", &d.to_string());
        }
        if self.loop_b > self.loop_a {
            kv("loop_a", &num(self.loop_a));
            kv("loop_b", &num(self.loop_b));
        }
        if self.phase_lock {
            kv("launch", "sync");
        }
        match &self.kind {
            Kind::Midi(notes) => {
                let mut s = String::new();
                for (i, n) in notes.iter().enumerate() {
                    if i > 0 {
                        s.push(';');
                    }
                    let _ = write!(s, "{},{},{},{}", num(n.start), num(n.len), n.pitch, n.vel);
                    if n.chan != 0 || n.prob != 100 {
                        let _ = write!(s, ",{}", n.chan);
                    }
                    if n.prob != 100 {
                        let _ = write!(s, ",{}", n.prob);
                    }
                }
                kv("notes", &s);
            }
            Kind::Audio(a) => {
                kv("path", &esc(&a.path));
                if a.offs > 0.0 {
                    kv("offs", &num(a.offs));
                }
                if a.len > 0.0 {
                    kv("len", &num(a.len));
                }
                if let Some(b) = a.src_bpm {
                    kv("src_bpm", &num(b));
                }
                kv("tempo_mode", a.tempo.key());
                if (a.gain - 1.0).abs() > 1e-9 {
                    kv("gain", &num(a.gain));
                }
                kv("lmode", if a.looped { "loop" } else { "oneshot" });
            }
        }
        o
    }

    /// Relit une forme ecrite (celle-ci, ou celle de l'ancienne Session). Un
    /// clip sans identite en recoit une de `fresh`.
    pub fn from_cpc1(s: &str, fresh: &mut dyn FnMut() -> u64) -> Option<Clip> {
        let mut it = s.trim().split('|');
        if it.next()? != "CPC1" {
            return None;
        }
        let mut get = std::collections::HashMap::new();
        for kv in it {
            if let Some((k, v)) = kv.split_once('=') {
                get.insert(k.to_string(), v.to_string());
            }
        }
        let f = |k: &str| get.get(k).and_then(|v| v.parse::<f64>().ok());
        let kind = match get.get("kind").map(String::as_str) {
            Some("audio") => {
                let path = unesc(get.get("path").map_or("", |s| s.as_str()));
                Kind::Audio(Audio {
                    path,
                    offs: f("offs").unwrap_or(0.0).max(0.0),
                    len: f("len").unwrap_or(0.0).max(0.0),
                    src_bpm: f("src_bpm").filter(|b| *b > 1.0),
                    tempo: TempoMode::of_key(get.get("tempo_mode").map_or("", |s| s.as_str())),
                    gain: f("gain").unwrap_or(1.0),
                    // L'ancienne Session : one-shot par defaut.
                    looped: get.get("lmode").map(String::as_str) == Some("loop"),
                })
            }
            _ => {
                let probs: Vec<u8> = get
                    .get("probs")
                    .map(|p| p.split(';').map(|x| x.parse().unwrap_or(100)).collect())
                    .unwrap_or_default();
                let mut notes = Vec::new();
                for (i, n) in get
                    .get("notes")
                    .map_or("", |s| s.as_str())
                    .split(';')
                    .filter(|x| !x.is_empty())
                    .enumerate()
                {
                    let v: Vec<&str> = n.split(',').collect();
                    if v.len() < 4 {
                        continue;
                    }
                    let (Ok(st), Ok(ln), Ok(p), Ok(vl)) = (
                        v[0].parse::<f64>(),
                        v[1].parse::<f64>(),
                        v[2].parse::<f64>(),
                        v[3].parse::<f64>(),
                    ) else {
                        continue;
                    };
                    let chan = v.get(4).and_then(|x| x.parse::<u8>().ok()).unwrap_or(0);
                    let prob = v
                        .get(5)
                        .and_then(|x| x.parse::<u8>().ok())
                        .or_else(|| probs.get(i).copied())
                        .unwrap_or(100);
                    notes.push(Note {
                        start: st.max(0.0),
                        len: ln.max(1e-3),
                        pitch: (p as i32).clamp(0, 127) as u8,
                        vel: (vl as i32).clamp(1, 127) as u8,
                        chan: chan & 0x0F,
                        prob: prob.min(100),
                    });
                }
                Kind::Midi(notes)
            }
        };
        let id = get
            .get("id")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or_else(fresh);
        let sig = match (
            get.get("tsnum").and_then(|v| v.parse::<u32>().ok()),
            get.get("tsden").and_then(|v| v.parse::<u32>().ok()),
        ) {
            (Some(n), d) if n > 0 => Some((n, d.filter(|d| *d > 0).unwrap_or(4))),
            _ => None,
        };
        let bars = f("bars").filter(|b| *b > 0.0).unwrap_or(match kind {
            Kind::Midi(_) => 4.0,
            Kind::Audio(_) => 0.0,
        });
        Some(Clip {
            id,
            name: unesc(get.get("name").map_or("", |s| s.as_str())),
            color: get
                .get("color")
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|c| (1..=8).contains(c)),
            bars,
            sig,
            loop_a: f("loop_a").unwrap_or(0.0),
            loop_b: f("loop_b").unwrap_or(0.0),
            phase_lock: get.get("launch").map(String::as_str) == Some("sync"),
            kind,
        })
    }
}

// ---------------------------------------------------------------------------
// Le tempo d'une source
// ---------------------------------------------------------------------------

/// D'ou vient le tempo d'une source (montre sous la case).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoWhy {
    /// Declare dans la case.
    Set,
    /// Lu dans le fichier (`GetTempoMatchPlayRate`).
    File,
    /// Lu dans le nom.
    Name,
    /// Devine par la duree.
    Guess,
    /// Rien trouve : la source garde sa vitesse.
    Unknown,
}

impl TempoWhy {
    pub fn label(self) -> &'static str {
        match self {
            TempoWhy::Set => "set",
            TempoWhy::File => "file",
            TempoWhy::Name => "name",
            TempoWhy::Guess => "guess",
            TempoWhy::Unknown => "no tempo",
        }
    }
}

/// Un tempo ecrit dans un nom de fichier : « 120bpm », « 120 BPM »,
/// « _120_ », « -128- » (entre 60 et 200 pour un nombre nu).
pub fn tempo_from_name(name: &str) -> Option<f64> {
    let low = name.to_lowercase();
    let b = low.as_bytes();
    // « 120bpm », « 120 bpm », « 120_bpm »
    if let Some(i) = low.find("bpm") {
        let mut j = i;
        while j > 0 && matches!(b[j - 1], b' ' | b'_' | b'-') {
            j -= 1;
        }
        let mut k = j;
        while k > 0 && (b[k - 1].is_ascii_digit() || b[k - 1] == b'.') {
            k -= 1;
        }
        if k < j {
            if let Ok(v) = low[k..j].parse::<f64>() {
                if (40.0..=300.0).contains(&v) {
                    return Some(v);
                }
            }
        }
    }
    // Un nombre seul entre separateurs.
    for tok in low.split(|c: char| !c.is_ascii_digit() && c != '.') {
        if tok.len() >= 2 && tok.len() <= 3 {
            if let Ok(v) = tok.parse::<f64>() {
                if (60.0..=200.0).contains(&v) && v.fract() == 0.0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Un tempo devine par la duree seule : une boucle de 1, 2, 4, 8 ou 16
/// mesures (`qpb` noires chacune) dont le tempo tombe pres de `project`
/// (a deux demi-tons pres), et une seule.
pub fn tempo_from_length(seconds: f64, qpb: f64, project: f64) -> Option<f64> {
    if seconds.is_nan() || project.is_nan() || seconds <= 0.2 || project <= 1.0 {
        return None;
    }
    let semi2 = 2f64.powf(2.0 / 12.0);
    let mut found = None;
    for bars in [1.0, 2.0, 4.0, 8.0, 16.0] {
        let bpm = bars * qpb * 60.0 / seconds;
        if bpm >= project / semi2 && bpm <= project * semi2 {
            if found.is_some() {
                return None;
            }
            found = Some(bpm);
        }
    }
    found
}

/// La passe d'une case audio, en mesures : la plus courte longueur du menu
/// qui contient la region, a 2 % pres.
pub fn bars_for(region_beats: f64, qpb: f64) -> f64 {
    let bars = region_beats / qpb.max(1e-9);
    for l in LENGTHS {
        if bars <= l * 1.02 {
            return l;
        }
    }
    LENGTHS[LENGTHS.len() - 1]
}

/// Les longueurs du menu, en mesures.
pub const LENGTHS: [f64; 10] = [0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];

/// Le libelle d'une longueur (« 1/4 », « 2 »).
pub fn length_label(bars: f64) -> String {
    if bars >= 1.0 {
        num(bars)
    } else {
        format!("1/{}", (1.0 / bars).round() as i64)
    }
}
