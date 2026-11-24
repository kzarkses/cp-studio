//! Les fichiers de REAPER sur ses effets, lus tels quels :
//! - `reaper-vstplugins64.ini` : fichier = date,identifiant,nom (`!!!VSTi` :
//!   un instrument) ;
//! - `reaper-clap-win64.ini` : une section par fichier, `id=0|nom` (1 : un
//!   instrument) ;
//! - `reaper-jsfx.ini` : `NAME chemin "JS: description"` ;
//! - `reaper-fxfolders.ini` : les dossiers de l'utilisateur (`[Folders]`, puis
//!   `[FolderN]` : `ItemK` et `TypeK`, 3 = VST, 2 = JS, 1000 = chaine) ;
//! - `reaper-fxtags.ini` (et les sections du meme nom de `reaper-fxfolders.ini`) :
//!   `[developer]` et `[category]`, par fichier.
//!
//! Les fichiers d'effets s'y ecrivent de plusieurs facons (« kHs 3-Band
//! EQ.vst3 » dans un dossier, « kHs_3_Band_EQ.vst3 » dans la liste) : tout se
//! compare par [`norm_file`].

use std::collections::HashMap;

/// La cle d'un fichier d'effet : son nom seul (sans chemin, sans ce qui suit
/// `<`), en minuscules, chaque signe qui n'est ni lettre, ni chiffre, ni point
/// devenu `_`. « C:\...\kHs 3-Band EQ.vst3 » et « kHs_3_Band_EQ.vst3 » donnent
/// tous deux « khs_3_band_eq.vst3 ».
pub fn norm_file(s: &str) -> String {
    let s = s.trim().trim_matches('"');
    let s = s.split('<').next().unwrap_or(s);
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    base.chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// La cle d'un effet JS : son chemin sous `Effects`, en minuscules, avec des
/// `/`, sans `.jsfx`.
pub fn norm_js(s: &str) -> String {
    let s = s.trim().trim_matches('"').replace('\\', "/").to_lowercase();
    s.strip_suffix(".jsfx").unwrap_or(&s).to_string()
}

/// Le nom seul d'un effet JS (la cle de ses etiquettes) : « saike tools/x.jsfx »
/// -> « x.jsfx ».
pub fn js_base(path: &str) -> String {
    let p = norm_js(path);
    let b = p.rsplit('/').next().unwrap_or(&p);
    norm_file(&format!("{b}.jsfx"))
}

/// Une ligne de la liste des VST.
#[derive(Clone, Debug, PartialEq)]
pub struct VstLine {
    pub file: String,
    pub name: String,
    pub instrument: bool,
}

pub fn parse_vst_ini(text: &str) -> Vec<VstLine> {
    let mut v = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') || line.starts_with(';') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let mut parts = val.splitn(3, ',');
        let (Some(_), Some(_), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (name, instrument) = match name.strip_suffix("!!!VSTi") {
            Some(n) => (n, true),
            None => (name, false),
        };
        let name = name.trim();
        if name.is_empty() || name.starts_with('<') {
            continue;
        }
        v.push(VstLine {
            file: norm_file(key),
            name: name.to_string(),
            instrument,
        });
    }
    v
}

/// Une entree de la liste des CLAP.
#[derive(Clone, Debug, PartialEq)]
pub struct ClapLine {
    pub file: String,
    pub id: String,
    pub name: String,
    pub instrument: bool,
}

pub fn parse_clap_ini(text: &str) -> Vec<ClapLine> {
    let mut v = Vec::new();
    let mut file = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(sec) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            file = norm_file(sec);
            continue;
        }
        let Some((id, val)) = line.split_once('=') else {
            continue;
        };
        if id == "_" {
            continue;
        }
        let Some((flag, name)) = val.split_once('|') else {
            continue;
        };
        v.push(ClapLine {
            file: file.clone(),
            id: id.to_string(),
            name: name.trim().to_string(),
            instrument: flag.trim().parse::<i32>().is_ok_and(|f| f & 1 != 0),
        });
    }
    v
}

/// Les effets JS : (chemin normalise, nom « JS: ... »).
pub fn parse_jsfx_ini(text: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("NAME ") else {
            continue;
        };
        let rest = rest.trim();
        let (path, tail) = if let Some(r) = rest.strip_prefix('"') {
            match r.split_once('"') {
                Some((p, t)) => (p, t),
                None => continue,
            }
        } else {
            match rest.split_once(' ') {
                Some((p, t)) => (p, t),
                None => continue,
            }
        };
        let name = tail.trim().trim_matches('"').trim();
        if name.is_empty() || path.is_empty() || path.starts_with('-') {
            continue;
        }
        v.push((norm_js(path), name.to_string()));
    }
    v
}

/// Un element d'un dossier de REAPER.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderItem {
    /// 3 : un VST (son fichier) ; 2 : un JS (son chemin) ; 1000 : une chaine
    /// d'effets (son nom) ; d'autres valeurs : d'autres formats.
    pub kind: i32,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub name: String,
    pub items: Vec<FolderItem>,
}

/// Les sections d'un fichier .ini : nom -> (cle, valeur) dans l'ordre.
fn sections(text: &str) -> HashMap<String, Vec<(String, String)>> {
    let mut m: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut cur = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(sec) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cur = sec.to_string();
            m.entry(cur.clone()).or_default();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            m.entry(cur.clone())
                .or_default()
                .push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    m
}

/// Les dossiers de l'utilisateur, dans l'ordre de REAPER.
pub fn parse_folders(text: &str) -> Vec<Folder> {
    let secs = sections(text);
    let Some(top) = secs.get("Folders") else {
        return Vec::new();
    };
    let get = |k: &str| top.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str());
    let n = get("NbFolders")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..n {
        let Some(name) = get(&format!("Name{i}")) else {
            continue;
        };
        let id = get(&format!("Id{i}")).unwrap_or("");
        let Some(sec) = secs.get(&format!("Folder{id}")) else {
            out.push(Folder {
                name: name.to_string(),
                items: Vec::new(),
            });
            continue;
        };
        let fget = |k: &str| sec.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str());
        let nb = fget("Nb")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut items = Vec::new();
        for k in 0..nb {
            let Some(value) = fget(&format!("Item{k}")) else {
                continue;
            };
            let kind = fget(&format!("Type{k}"))
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0);
            items.push(FolderItem {
                kind,
                value: value.to_string(),
            });
        }
        out.push(Folder {
            name: name.to_string(),
            items,
        });
    }
    out
}

/// Les developpeurs et les categories, par fichier ([`norm_file`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tags {
    pub dev: HashMap<String, String>,
    pub cat: HashMap<String, Vec<String>>,
}

impl Tags {
    /// Ajoute ce que dit un fichier (les cles deja vues sont remplacees : le
    /// dernier lu l'emporte).
    pub fn read(&mut self, text: &str) {
        let secs = sections(text);
        if let Some(d) = secs.get("developer") {
            for (k, v) in d {
                if !v.is_empty() {
                    self.dev.insert(norm_file(k), v.clone());
                }
            }
        }
        if let Some(c) = secs.get("category") {
            for (k, v) in c {
                let cats: Vec<String> = v
                    .split('|')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if !cats.is_empty() {
                    self.cat.insert(norm_file(k), cats);
                }
            }
        }
    }
}
