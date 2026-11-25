//! Le catalogue des effets, tel que le navigateur d'effets le montre.
//!
//! - La liste vient de REAPER (`EnumInstalledFX`) : le nom qu'elle donne est
//!   celui que `TrackFX_AddByName` reconnait.
//! - Les dossiers, categories et developpeurs viennent des fichiers de REAPER
//!   ([`ini`]) : ce que l'utilisateur a range dans le navigateur de REAPER se
//!   retrouve ici, sans rien refaire.
//! - Les favoris partent du dossier « Favorites » de REAPER ; ce que CP Studio
//!   y ajoute ou en retire est garde a part ([`Store`]), avec les recents, les
//!   compteurs d'usage et les onglets.
//!
//! Pur : ni Windows ni REAPER. Le module lit les fichiers et la liste, et
//! passe le texte.

pub mod ini;
mod store;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

pub use store::{PostInsert, PostParams, Rule, Settings, Store, Tab};

use ini::{Folder, Tags, js_base, norm_file, norm_js};

/// Le format d'un effet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Vst3,
    Vst,
    Clap,
    Js,
    Lv2,
    Au,
    Dx,
    /// Une chaine d'effets enregistree (.RfxChain).
    Chain,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Vst3,
        Kind::Vst,
        Kind::Clap,
        Kind::Js,
        Kind::Lv2,
        Kind::Au,
        Kind::Dx,
        Kind::Chain,
        Kind::Other,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Kind::Vst3 => "VST3",
            Kind::Vst => "VST",
            Kind::Clap => "CLAP",
            Kind::Js => "JS",
            Kind::Lv2 => "LV2",
            Kind::Au => "AU",
            Kind::Dx => "DX",
            Kind::Chain => "Chain",
            Kind::Other => "FX",
        }
    }

    /// Le nom de la liste du format (la barre de cote).
    pub fn label(self) -> &'static str {
        match self {
            Kind::Chain => "FX chains",
            k => k.tag(),
        }
    }

    pub fn from_tag(s: &str) -> Option<Kind> {
        Kind::ALL
            .into_iter()
            .find(|k| k.tag().eq_ignore_ascii_case(s))
    }

    /// Le prefixe d'un nom de REAPER : « VST3i » -> (VST3, instrument).
    pub fn from_prefix(p: &str) -> (Kind, bool) {
        // Aucun format connu ne finit par « I » : un « i » final dit
        // l'instrument (VSTi, VST3i, CLAPi, AUi, DXi, LV2i).
        let up = p.trim().to_ascii_uppercase();
        let (base, inst) = match up.strip_suffix('I') {
            Some(b) if b.len() >= 2 => (b, true),
            _ => (up.as_str(), false),
        };
        let k = match base {
            "VST3" => Kind::Vst3,
            "VST" | "VST2" => Kind::Vst,
            "CLAP" => Kind::Clap,
            "JS" => Kind::Js,
            "LV2" => Kind::Lv2,
            "AU" => Kind::Au,
            "DX" => Kind::Dx,
            _ => return (Kind::Other, false),
        };
        (k, inst)
    }
}

/// Un effet (ou une chaine d'effets).
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// Le nom de REAPER, tel quel : « VST3: Pro-Q 3 (FabFilter) ». Pour une
    /// chaine : son chemin sous `FXChains`.
    pub name: String,
    /// Ce qu'on lit : « Pro-Q 3 ».
    pub label: String,
    pub dev: String,
    pub kind: Kind,
    pub instrument: bool,
    /// La cle de son fichier ([`norm_file`] ; pour un JS, [`norm_js`] de son
    /// chemin). Vide si REAPER ne l'a pas dit.
    pub file: String,
    pub cats: Vec<String>,
    /// Ce que la recherche lit (minuscules).
    hay: String,
}

impl Plugin {
    /// Le type montre : « VST3 », « VST3i ».
    pub fn type_tag(&self) -> String {
        if self.instrument && self.kind != Kind::Chain {
            format!("{}i", self.kind.tag())
        } else {
            self.kind.tag().to_string()
        }
    }

    /// Ce que la recherche lit : nom, developpeur, type, categories.
    pub fn haystack(&self) -> &str {
        &self.hay
    }

    fn index(&mut self) {
        let mut h = format!("{} {} {}", self.label, self.dev, self.type_tag());
        for c in &self.cats {
            h.push(' ');
            h.push_str(c);
        }
        if self.instrument {
            h.push_str(" instrument synth");
        }
        self.hay = h.to_lowercase();
    }
}

/// Les parentheses de fin qui disent les entrees et sorties, pas le
/// developpeur : « (64 out) », « (mono) », « (2->6ch) », « (x86) ».
fn is_io_group(g: &str) -> bool {
    let g = g.trim().to_ascii_lowercase();
    if matches!(
        g.as_str(),
        "mono" | "stereo" | "x86" | "x64" | "32 bit" | "64 bit"
    ) {
        return true;
    }
    let digits_then = |rest: &str| -> bool {
        let d = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        d.len() < rest.len() && matches!(d.trim(), "out" | "ch" | "in" | "outs" | "channels")
    };
    if digits_then(&g) {
        return true;
    }
    // « 2->6ch », « 4 in 8 out »
    if g.contains("->") && g.ends_with("ch") {
        return true;
    }
    if let Some((a, b)) = g.split_once(" in ") {
        return a.trim().chars().all(|c| c.is_ascii_digit()) && digits_then(b);
    }
    false
}

/// Separe un nom sans prefixe en (nom lisible, developpeur) : « Kontakt 7
/// (Native Instruments) (64 out) » -> (« Kontakt 7 (64 out) », « Native
/// Instruments »). Le developpeur est la derniere parenthese de fin qui ne
/// parle pas d'entrees ou de sorties.
pub fn split_dev(rest: &str) -> (String, String) {
    let s = rest.trim();
    // Les groupes de fin : (debut, fin) en octets.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut end = s.len();
    loop {
        let t = s[..end].trim_end();
        if !t.ends_with(')') {
            break;
        }
        let close = t.len() - 1;
        let mut depth = 0i32;
        let mut open = None;
        for (i, c) in t.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(o) = open else { break };
        if o == 0 {
            break;
        }
        groups.push((o, close + 1));
        end = o;
    }
    for &(o, c) in &groups {
        let inner = &s[o + 1..c - 1];
        if !is_io_group(inner) && !inner.trim().is_empty() {
            let label = format!("{} {}", s[..o].trim_end(), s[c..].trim_start());
            return (label.trim().to_string(), inner.trim().to_string());
        }
    }
    (s.to_string(), String::new())
}

/// Separe un nom de REAPER : « VST3i: Serum (Xfer Records) » -> (VST3,
/// instrument, « Serum (Xfer Records) »).
pub fn split_name(full: &str) -> (Kind, bool, String) {
    if let Some((p, rest)) = full.split_once(": ") {
        let (k, inst) = Kind::from_prefix(p);
        if k != Kind::Other {
            return (k, inst, rest.trim().to_string());
        }
    }
    (Kind::Other, false, full.trim().to_string())
}

/// Les fichiers de REAPER, en texte (un fichier absent : vide).
#[derive(Clone, Debug, Default)]
pub struct IniTexts {
    pub vst: Vec<String>,
    pub clap: Vec<String>,
    pub jsfx: String,
    pub folders: String,
    pub tags: String,
}

/// Un dossier de REAPER, resolu : les effets qu'il contient.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderIdx {
    pub name: String,
    pub items: Vec<usize>,
}

/// Le catalogue : les effets et leurs rangements.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub plugins: Vec<Plugin>,
    pub folders: Vec<FolderIdx>,
    /// (categorie, effets), par nom.
    pub cats: Vec<(String, Vec<usize>)>,
    /// (developpeur, effets), par nom.
    pub devs: Vec<(String, Vec<usize>)>,
    /// (format, effets) des formats presents.
    pub kinds: Vec<(Kind, Vec<usize>)>,
    /// Le dossier « Favorites » de REAPER.
    pub reaper_favs: Vec<usize>,
    /// Les elements des dossiers que le catalogue n'a pas trouves :
    /// « dossier : element » (le journal).
    pub missing: Vec<String>,
    by_name: HashMap<String, usize>,
    by_label: HashMap<String, usize>,
}

impl Catalog {
    /// Construit le catalogue. `installed` : (nom, ident) de
    /// `EnumInstalledFX` ; `chains` : les chaines (chemins sous `FXChains`).
    pub fn build(installed: &[(String, String)], ini: &IniTexts, chains: &[String]) -> Catalog {
        // Les noms des fichiers, par nom sans prefixe.
        let mut vst_by_name: HashMap<String, (String, bool)> = HashMap::new();
        // Le nom d'un fichier (un dossier peut nommer un VST2 que REAPER cache
        // derriere son VST3 : le meme nom le retrouve).
        let mut name_of_file: HashMap<String, String> = HashMap::new();
        for t in &ini.vst {
            for l in ini::parse_vst_ini(t) {
                name_of_file
                    .entry(l.file.clone())
                    .or_insert_with(|| l.name.to_lowercase());
                // Le meme nom en VST2 et en VST3 : deux fichiers. Le nom sans
                // son developpeur aussi (REAPER ajoute parfois le
                // developpeur de reaper-fxtags.ini au nom qu'il montre).
                let v = if l.file.ends_with(".vst3") { '3' } else { '2' };
                let (label, _) = split_dev(&l.name);
                for key in [l.name.to_lowercase(), label.to_lowercase()] {
                    vst_by_name
                        .entry(format!("{v}|{key}"))
                        .or_insert((l.file.clone(), l.instrument));
                }
            }
        }
        let mut clap_by_name: HashMap<String, (String, bool)> = HashMap::new();
        for t in &ini.clap {
            for l in ini::parse_clap_ini(t) {
                name_of_file
                    .entry(l.file.clone())
                    .or_insert_with(|| l.name.to_lowercase());
                clap_by_name
                    .entry(l.name.to_lowercase())
                    .or_insert((l.file, l.instrument));
            }
        }
        let mut js_by_name: HashMap<String, String> = HashMap::new();
        let mut js_name_of: HashMap<String, String> = HashMap::new();
        for (p, n) in ini::parse_jsfx_ini(&ini.jsfx) {
            js_name_of.entry(p.clone()).or_insert_with(|| n.clone());
            js_by_name.entry(n.to_lowercase()).or_insert(p);
        }
        let mut tags = Tags::default();
        tags.read(&ini.tags);
        tags.read(&ini.folders);

        let mut plugins = Vec::new();
        let mut seen: HashMap<String, ()> = HashMap::new();
        for (name, ident) in installed {
            if name.trim().is_empty() || seen.insert(name.clone(), ()).is_some() {
                continue;
            }
            let (kind, inst, rest) = split_name(name);
            let (mut label, mut dev) = if kind == Kind::Js {
                (rest.clone(), String::new())
            } else {
                split_dev(&rest)
            };
            let id = ident.trim();
            let mut instrument = inst;
            let file = match kind {
                Kind::Js => {
                    if !id.is_empty() && !id.contains(": ") {
                        norm_js(id)
                    } else {
                        js_by_name
                            .get(&name.to_lowercase())
                            .cloned()
                            .unwrap_or_default()
                    }
                }
                Kind::Vst | Kind::Vst3 | Kind::Clap => {
                    let low = id.to_ascii_lowercase();
                    let from_id = [".dll", ".vst3", ".vst", ".clap", ".so", ".dylib"]
                        .iter()
                        .any(|e| low.split('<').next().unwrap_or("").ends_with(e));
                    let low_rest = rest.to_lowercase();
                    let low_label = label.to_lowercase();
                    let (first, second) = if kind == Kind::Vst3 {
                        ('3', '2')
                    } else {
                        ('2', '3')
                    };
                    let hit = match kind {
                        Kind::Clap => clap_by_name.get(&low_rest),
                        _ => [&low_rest, &low_label]
                            .iter()
                            .flat_map(|k| [format!("{first}|{k}"), format!("{second}|{k}")])
                            .find_map(|k| vst_by_name.get(&k)),
                    };
                    if let Some((_, i)) = hit {
                        instrument |= *i;
                    }
                    if from_id {
                        norm_file(id)
                    } else {
                        hit.map(|h| h.0.clone()).unwrap_or_default()
                    }
                }
                _ => String::new(),
            };
            let tag_key = if kind == Kind::Js {
                js_base(&file)
            } else {
                file.clone()
            };
            if dev.is_empty() {
                if let Some(d) = tags.dev.get(&tag_key) {
                    dev = d.clone();
                }
            }
            if label.is_empty() {
                label = rest.clone();
            }
            let cats = tags.cat.get(&tag_key).cloned().unwrap_or_default();
            let mut p = Plugin {
                name: name.clone(),
                label,
                dev,
                kind,
                instrument,
                file,
                cats,
                hay: String::new(),
            };
            p.index();
            plugins.push(p);
        }
        for c in chains {
            let rel = c.replace('\\', "/");
            let stem = rel.rsplit('/').next().unwrap_or(&rel);
            let stem = stem
                .strip_suffix(".RfxChain")
                .or_else(|| stem.strip_suffix(".rfxchain"))
                .unwrap_or(stem);
            let folder = rel.rsplit_once('/').map_or("", |(a, _)| a);
            let mut p = Plugin {
                name: c.clone(),
                label: stem.to_string(),
                dev: folder.to_string(),
                kind: Kind::Chain,
                instrument: false,
                file: norm_file(stem),
                cats: Vec::new(),
                hay: String::new(),
            };
            p.index();
            plugins.push(p);
        }

        let mut cat = Catalog {
            plugins,
            ..Catalog::default()
        };
        cat.reindex();
        let mut missing = Vec::new();
        cat.folders = ini::parse_folders(&ini.folders)
            .iter()
            .map(|f| cat.resolve(f, &name_of_file, &js_name_of, &mut missing))
            .collect();
        cat.missing = missing;
        cat.reaper_favs = cat
            .folders
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("Favorites"))
            .map(|f| f.items.clone())
            .unwrap_or_default();
        cat
    }

    fn reindex(&mut self) {
        let mut cats: HashMap<String, Vec<usize>> = HashMap::new();
        let mut devs: HashMap<String, Vec<usize>> = HashMap::new();
        let mut kinds: HashMap<Kind, Vec<usize>> = HashMap::new();
        self.by_name.clear();
        self.by_label.clear();
        for (i, p) in self.plugins.iter().enumerate() {
            self.by_name.insert(p.name.to_lowercase(), i);
            let (_, _, rest) = split_name(&p.name);
            self.by_label.entry(rest.to_lowercase()).or_insert(i);
            if !p.dev.is_empty() {
                self.by_label
                    .entry(format!("{} ({})", p.label, p.dev).to_lowercase())
                    .or_insert(i);
            }
            self.by_label.entry(p.label.to_lowercase()).or_insert(i);
            for c in &p.cats {
                cats.entry(c.clone()).or_default().push(i);
            }
            if !p.dev.is_empty() && p.kind != Kind::Chain {
                devs.entry(p.dev.clone()).or_default().push(i);
            }
            kinds.entry(p.kind).or_default().push(i);
        }
        let mut c: Vec<_> = cats.into_iter().collect();
        c.sort_by_key(|(n, _)| n.to_lowercase());
        let mut d: Vec<_> = devs.into_iter().collect();
        d.sort_by_key(|(n, _)| n.to_lowercase());
        let mut k: Vec<_> = kinds.into_iter().collect();
        k.sort_by_key(|(k, _)| *k);
        self.cats = c;
        self.devs = d;
        self.kinds = k;
    }

    /// Les effets d'un dossier de REAPER.
    fn resolve(
        &self,
        f: &Folder,
        name_of_file: &HashMap<String, String>,
        js_name_of: &HashMap<String, String>,
        missing: &mut Vec<String>,
    ) -> FolderIdx {
        let by_rest = |n: &str| {
            self.plugins
                .iter()
                .position(|q| q.kind != Kind::Chain && split_name(&q.name).2.to_lowercase() == n)
        };
        let mut items = Vec::new();
        for it in &f.items {
            let found = match it.kind {
                2 => {
                    let p = norm_js(&it.value);
                    self.plugins
                        .iter()
                        .position(|q| q.kind == Kind::Js && q.file == p)
                        .or_else(|| {
                            let b = js_base(&it.value);
                            self.plugins
                                .iter()
                                .position(|q| q.kind == Kind::Js && js_base(&q.file) == b)
                        })
                        .or_else(|| js_name_of.get(&p).and_then(|n| self.find(n)))
                }
                1000 => {
                    let v = it.value.trim();
                    let n = norm_file(
                        v.strip_suffix(".RfxChain")
                            .or_else(|| v.strip_suffix(".rfxchain"))
                            .unwrap_or(v),
                    );
                    self.plugins
                        .iter()
                        .position(|q| q.kind == Kind::Chain && q.file == n)
                }
                _ => {
                    let k = norm_file(&it.value);
                    self.plugins
                        .iter()
                        .position(|q| q.kind != Kind::Js && q.kind != Kind::Chain && q.file == k)
                        .or_else(|| {
                            // Un format inconnu : un chemin de JS, peut-etre.
                            let p = norm_js(&it.value);
                            self.plugins
                                .iter()
                                .position(|q| q.kind == Kind::Js && q.file == p)
                        })
                        .or_else(|| name_of_file.get(&k).and_then(|n| by_rest(n)))
                }
            };
            match found {
                Some(i) => {
                    if !items.contains(&i) {
                        items.push(i);
                    }
                }
                None => missing.push(format!("{} : {}", f.name, it.value)),
            }
        }
        FolderIdx {
            name: f.name.clone(),
            items,
        }
    }

    /// L'effet d'un nom : le nom de REAPER, ou « nom (developpeur) », ou le
    /// nom seul (les onglets d'avant gardaient « kHs Delay (Kilohearts) »).
    pub fn find(&self, name: &str) -> Option<usize> {
        let n = name.trim().to_lowercase();
        self.by_name
            .get(&n)
            .or_else(|| self.by_label.get(&n))
            .copied()
    }

    /// Le dossier, la categorie ou le developpeur d'un nom.
    pub fn folder(&self, name: &str) -> Option<usize> {
        self.folders.iter().position(|f| f.name == name)
    }
}

/// Une liste : ce que montre une pastille ou une entree de la barre de cote.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Src {
    #[default]
    All,
    Fav,
    Recent,
    Most,
    Instr,
    Kind(Kind),
    Folder(String),
    Cat(String),
    Dev(String),
    Tab(String),
}

impl Src {
    /// Les pastilles fixes, avant les onglets.
    pub const CHIPS: [Src; 5] = [Src::All, Src::Fav, Src::Recent, Src::Most, Src::Instr];

    pub fn key(&self) -> String {
        match self {
            Src::All => "all".into(),
            Src::Fav => "fav".into(),
            Src::Recent => "recent".into(),
            Src::Most => "most".into(),
            Src::Instr => "instr".into(),
            Src::Kind(k) => format!("kind:{}", k.tag()),
            Src::Folder(n) => format!("folder:{n}"),
            Src::Cat(n) => format!("cat:{n}"),
            Src::Dev(n) => format!("dev:{n}"),
            Src::Tab(n) => format!("tab:{n}"),
        }
    }

    pub fn from_key(s: &str) -> Option<Src> {
        Some(match s {
            "all" => Src::All,
            "fav" => Src::Fav,
            "recent" => Src::Recent,
            "most" => Src::Most,
            "instr" => Src::Instr,
            _ => {
                let (a, b) = s.split_once(':')?;
                match a {
                    "kind" => Src::Kind(Kind::from_tag(b)?),
                    "folder" => Src::Folder(b.to_string()),
                    "cat" => Src::Cat(b.to_string()),
                    "dev" => Src::Dev(b.to_string()),
                    "tab" => Src::Tab(b.to_string()),
                    _ => return None,
                }
            }
        })
    }

    /// Ce que la pastille ou l'entree affiche.
    pub fn name(&self) -> String {
        match self {
            Src::All => "All".into(),
            Src::Fav => "Favorites".into(),
            Src::Recent => "Recent".into(),
            Src::Most => "Most used".into(),
            Src::Instr => "Instruments".into(),
            Src::Kind(k) => k.label().into(),
            Src::Folder(n) | Src::Cat(n) | Src::Dev(n) | Src::Tab(n) => n.clone(),
        }
    }

    /// L'ordre de la liste est-il le sien (recents, plus utilises) ?
    pub fn keeps_order(&self) -> bool {
        matches!(self, Src::Recent | Src::Most)
    }
}

/// La recherche : tous les mots (ET), sans casse ; `-mot` exclut.
pub fn matches(p: &Plugin, query: &str) -> bool {
    for w in query.split_whitespace() {
        let w = w.to_lowercase();
        if let Some(neg) = w.strip_prefix('-') {
            if !neg.is_empty() && p.hay.contains(neg) {
                return false;
            }
        } else if !p.hay.contains(&w) {
            return false;
        }
    }
    true
}

/// Les effets d'une liste, avant la recherche (dans l'ordre de la liste pour
/// les recents et les plus utilises).
pub fn source_items(cat: &Catalog, st: &Store, src: &Src) -> Vec<usize> {
    let not_chain = |i: &usize| cat.plugins[*i].kind != Kind::Chain;
    match src {
        Src::All => (0..cat.plugins.len()).filter(not_chain).collect(),
        Src::Fav => (0..cat.plugins.len())
            .filter(|&i| st.is_fav(cat, i))
            .collect(),
        Src::Recent => st.recent.iter().filter_map(|n| cat.find(n)).collect(),
        Src::Most => {
            let mut v: Vec<(usize, u32)> = st
                .uses
                .iter()
                .filter_map(|(n, c)| cat.find(n).map(|i| (i, *c)))
                .collect();
            v.sort_by_key(|a| std::cmp::Reverse(a.1));
            v.into_iter().map(|(i, _)| i).collect()
        }
        Src::Instr => (0..cat.plugins.len())
            .filter(|&i| cat.plugins[i].instrument)
            .collect(),
        Src::Kind(k) => cat
            .kinds
            .iter()
            .find(|(q, _)| q == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        Src::Folder(n) => cat
            .folders
            .iter()
            .find(|f| &f.name == n)
            .map(|f| f.items.clone())
            .unwrap_or_default(),
        Src::Cat(n) => cat
            .cats
            .iter()
            .find(|(q, _)| q == n)
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        Src::Dev(n) => cat
            .devs
            .iter()
            .find(|(q, _)| q == n)
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        Src::Tab(n) => {
            let Some(t) = st.tabs.iter().find(|t| &t.name == n) else {
                return Vec::new();
            };
            let mut v: Vec<usize> = Vec::new();
            if let Some(r) = &t.rule {
                let base = Src::from_key(&r.src).unwrap_or_default();
                // Une regle qui se cite elle-meme ne boucle pas.
                let base = if base == *src { Src::All } else { base };
                for i in source_items(cat, st, &base) {
                    if matches(&cat.plugins[i], &r.query) {
                        v.push(i);
                    }
                }
            }
            for n in &t.items {
                if let Some(i) = cat.find(n) {
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
            v
        }
    }
}

/// Ce que montre la liste : la source, la recherche, l'ordre.
pub fn list(cat: &Catalog, st: &Store, src: &Src, query: &str) -> Vec<usize> {
    let mut v: Vec<usize> = source_items(cat, st, src)
        .into_iter()
        .filter(|&i| matches(&cat.plugins[i], query))
        .collect();
    if !src.keeps_order() {
        v.sort_by_cached_key(|&i| cat.plugins[i].label.to_lowercase());
        if st.set.sort_za {
            v.reverse();
        }
    }
    v
}

/// Les effets du coeur de CP Studio (FX Constellation, ses sources, le synthe
/// de Guitar Tab) : le de ne les tire jamais, et remplacer la chaine les
/// garde. Par nom, sans casse (« JS: FX Constellation Bridge », son fichier,
/// ou le nom montre).
pub fn is_internal(name: &str) -> bool {
    const CORE: [&str; 5] = [
        "fx constellation bridge",
        "fx constellation - ",
        "cp_mod lfo",
        "cp_mod midi",
        "cp guitar tab synth",
    ];
    let l = name.to_lowercase();
    CORE.iter().any(|c| l.contains(c))
}

/// Le de : `n` effets differents, tires dans `visible`. Jamais un instrument
/// ni une chaine dans une chaine d'effets ; si la liste ne montre que des
/// instruments, un seul instrument.
pub fn dice(cat: &Catalog, visible: &[usize], n: usize, seed: u64) -> Vec<usize> {
    let free = |i: usize| {
        let p = &cat.plugins[i];
        !is_internal(&p.name) && !is_internal(&p.label)
    };
    let fx: Vec<usize> = visible
        .iter()
        .copied()
        .filter(|&i| free(i) && !cat.plugins[i].instrument && cat.plugins[i].kind != Kind::Chain)
        .collect();
    let (mut pool, n) = if fx.is_empty() {
        let inst: Vec<usize> = visible
            .iter()
            .copied()
            .filter(|&i| free(i) && cat.plugins[i].instrument)
            .collect();
        (inst, 1)
    } else {
        (fx, n)
    };
    let mut x = seed | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let n = n.min(pool.len());
    for k in 0..n {
        let j = k + (next() % (pool.len() - k) as u64) as usize;
        pool.swap(k, j);
    }
    pool.truncate(n);
    pool
}
