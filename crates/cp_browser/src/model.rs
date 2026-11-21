//! L'arbre : les raccourcis de l'Explorateur de REAPER, les dossiers ouverts en
//! ligne, les bases MediaDB, les favoris.
//!
//! Un arbre en arene : chaque noeud connait ses enfants (charges a la premiere
//! ouverture) et son parent. Les LIGNES visibles sont recalculees quand
//! l'arbre change (ouverture, fermeture, recherche), pas a chaque image.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Un titre de section (« ---------- DATABASES »).
    Section,
    /// La racine des favoris.
    Favorites,
    Dir,
    /// Un lecteur (C:\).
    Drive,
    /// Une base MediaDB (`.ReaperFileList`).
    Db,
    File,
}

/// Ce que la base sait d'un fichier (lignes DATA).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub length_s: Option<f64>,
    pub srate: Option<u32>,
    pub chans: Option<u16>,
    pub bits: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub name: String,
    pub path: PathBuf,
    pub parent: Option<usize>,
    pub depth: u16,
    pub children: Option<Vec<usize>>,
    pub expanded: bool,
    pub meta: Meta,
    /// Le contenu n'a pas pu etre lu (dossier absent, base illisible).
    pub error: Option<String>,
}

/// Les extensions qu'on montre (ce que REAPER lit).
pub const AUDIO_EXT: [&str; 14] = [
    "wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "opus", "wv", "w64", "rf64", "caf", "m4a",
    "bwf",
];

pub fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// Un raccourci de `[reaper_explorer]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub target: String,
    pub title: Option<String>,
}

/// Lit `NbShortcuts`, `ShortcutN` et `ShortcutTN` de la section
/// `[reaper_explorer]` de reaper.ini.
pub fn read_shortcuts(ini: &str) -> Vec<Shortcut> {
    let mut in_sec = false;
    let mut kv: HashMap<String, String> = HashMap::new();
    for line in ini.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_sec = l.eq_ignore_ascii_case("[reaper_explorer]");
            continue;
        }
        if !in_sec {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            kv.insert(k.trim().to_ascii_lowercase(), v.to_string());
        }
    }
    let n: usize = kv
        .get("nbshortcuts")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    (0..n)
        .filter_map(|i| {
            let target = kv.get(&format!("shortcut{i}"))?.clone();
            let title = kv
                .get(&format!("shortcutt{i}"))
                .cloned()
                .filter(|t| !t.trim().is_empty());
            Some(Shortcut { target, title })
        })
        .collect()
}

/// Une entree de base MediaDB.
#[derive(Clone, Debug, PartialEq)]
pub struct DbEntry {
    pub path: String,
    pub meta: Meta,
}

fn parse_len(v: &str) -> Option<f64> {
    // h:m:s.ms, m:s.ms ou s.ms
    let mut t = 0.0;
    for part in v.split(':') {
        t = t * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(t)
}

/// Les lignes `FILE "<chemin>" ...` et leurs `DATA` (longueur, taux, canaux,
/// bits).
pub fn parse_mediadb(text: &str) -> Vec<DbEntry> {
    let mut out: Vec<DbEntry> = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix("FILE ") {
            let rest = rest.trim_start();
            let path = if let Some(r) = rest.strip_prefix('"') {
                r.split('"').next().unwrap_or("").to_string()
            } else {
                rest.split_whitespace().next().unwrap_or("").to_string()
            };
            if !path.is_empty() {
                out.push(DbEntry {
                    path,
                    meta: Meta::default(),
                });
            }
        } else if let Some(rest) = l.strip_prefix("DATA ") {
            let Some(e) = out.last_mut() else { continue };
            // Des paires k:v, eventuellement entre guillemets.
            let mut toks = Vec::new();
            let mut cur = String::new();
            let mut q = false;
            for c in rest.chars() {
                match c {
                    '"' => q = !q,
                    ' ' if !q => {
                        if !cur.is_empty() {
                            toks.push(std::mem::take(&mut cur));
                        }
                    }
                    _ => cur.push(c),
                }
            }
            if !cur.is_empty() {
                toks.push(cur);
            }
            for t in toks {
                let Some((k, v)) = t.split_once(':') else {
                    continue;
                };
                match k {
                    "l" => e.meta.length_s = parse_len(v),
                    "s" => e.meta.srate = v.trim().parse().ok(),
                    "n" => e.meta.chans = v.trim().parse().ok(),
                    "i" => e.meta.bits = v.trim().parse().ok(),
                    _ => {}
                }
            }
        }
    }
    out
}

/// Le nom affiche d'un chemin : son dernier composant, ou le chemin entier
/// pour une racine (« D:\ »).
pub fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

#[derive(Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
    pub favorites: usize,
}

impl Tree {
    fn add(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    /// L'arbre des raccourcis. `mediadb` = le dossier des bases, `project` =
    /// le dossier du projet (pour « <Project Directory> »).
    pub fn from_shortcuts(
        sc: &[Shortcut],
        mediadb: &Path,
        project: Option<&Path>,
        favs: &[PathBuf],
    ) -> Tree {
        let mut t = Tree::default();
        let fav = t.add(Node {
            kind: Kind::Favorites,
            name: "Favorites".into(),
            path: PathBuf::new(),
            parent: None,
            depth: 0,
            children: Some(Vec::new()),
            expanded: false,
            meta: Meta::default(),
            error: None,
        });
        t.favorites = fav;
        t.roots.push(fav);
        t.set_favorites(favs);
        for s in sc {
            let target = s.target.trim();
            let title = s.title.as_deref().map(str::trim);
            // Un titre « ---- X » est un separateur : on le montre comme tel.
            if let Some(ti) = title {
                if ti.starts_with("---") {
                    let label = ti.trim_start_matches('-').trim().to_string();
                    let id = t.add(Node {
                        kind: Kind::Section,
                        name: label,
                        path: PathBuf::new(),
                        parent: None,
                        depth: 0,
                        children: Some(Vec::new()),
                        expanded: false,
                        meta: Meta::default(),
                        error: None,
                    });
                    t.roots.push(id);
                    continue;
                }
            }
            let (kind, path, name) = if target.eq_ignore_ascii_case("<Project Directory>") {
                match project {
                    Some(p) => (Kind::Dir, p.to_path_buf(), "Project directory".to_string()),
                    None => continue,
                }
            } else if target.starts_with('<') {
                continue; // <Track Templates> et autres : pas des medias
            } else if target.to_ascii_lowercase().ends_with(".reaperfilelist") {
                let p = if Path::new(target).is_absolute() {
                    PathBuf::from(target)
                } else {
                    mediadb.join(target)
                };
                let name = display_name(&p);
                (Kind::Db, p, name)
            } else {
                let p = PathBuf::from(target.replace('/', "\\"));
                let is_drive = p.parent().is_none();
                let name = display_name(&p);
                (if is_drive { Kind::Drive } else { Kind::Dir }, p, name)
            };
            let name = title.map(str::to_string).unwrap_or(name);
            let id = t.add(Node {
                kind,
                name,
                path,
                parent: None,
                depth: 0,
                children: None,
                expanded: false,
                meta: Meta::default(),
                error: None,
            });
            t.roots.push(id);
        }
        t
    }

    /// Remplace les enfants des favoris.
    pub fn set_favorites(&mut self, favs: &[PathBuf]) {
        let fav = self.favorites;
        let mut kids = Vec::new();
        for p in favs {
            let kind = if is_audio(p) { Kind::File } else { Kind::Dir };
            let id = self.add(Node {
                kind,
                name: display_name(p),
                path: p.clone(),
                parent: Some(fav),
                depth: 1,
                children: None,
                expanded: false,
                meta: Meta::default(),
                error: None,
            });
            kids.push(id);
        }
        self.nodes[fav].children = Some(kids);
    }

    /// Charge les enfants d'un dossier ou d'une base (une fois).
    pub fn load(&mut self, id: usize) {
        if self.nodes[id].children.is_some() {
            return;
        }
        let (kind, path, depth) = {
            let n = &self.nodes[id];
            (n.kind.clone(), n.path.clone(), n.depth)
        };
        let mut kids = Vec::new();
        match kind {
            Kind::Dir | Kind::Drive => match std::fs::read_dir(&path) {
                Ok(rd) => {
                    let mut dirs = Vec::new();
                    let mut files = Vec::new();
                    for e in rd.flatten() {
                        let p = e.path();
                        let ft = e.file_type();
                        let is_dir = ft.as_ref().is_ok_and(|t| t.is_dir())
                            || (ft.as_ref().is_ok_and(|t| t.is_symlink()) && p.is_dir());
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.starts_with('$')
                            || name.eq_ignore_ascii_case("System Volume Information")
                        {
                            continue;
                        }
                        if is_dir {
                            dirs.push((name, p));
                        } else if is_audio(&p) {
                            files.push((name, p));
                        }
                    }
                    let key = |s: &String| s.to_lowercase();
                    dirs.sort_by_key(|a| key(&a.0));
                    files.sort_by_key(|a| key(&a.0));
                    for (name, p) in dirs {
                        kids.push(self.add(Node {
                            kind: Kind::Dir,
                            name,
                            path: p,
                            parent: Some(id),
                            depth: depth + 1,
                            children: None,
                            expanded: false,
                            meta: Meta::default(),
                            error: None,
                        }));
                    }
                    for (name, p) in files {
                        kids.push(self.add(Node {
                            kind: Kind::File,
                            name,
                            path: p,
                            parent: Some(id),
                            depth: depth + 1,
                            children: Some(Vec::new()),
                            expanded: false,
                            meta: Meta::default(),
                            error: None,
                        }));
                    }
                }
                Err(e) => self.nodes[id].error = Some(e.to_string()),
            },
            Kind::Db => match std::fs::read(&path) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut entries = parse_mediadb(&text);
                    entries.sort_by_cached_key(|e| display_name(Path::new(&e.path)).to_lowercase());
                    for e in entries {
                        let p = PathBuf::from(&e.path);
                        kids.push(self.add(Node {
                            kind: Kind::File,
                            name: display_name(&p),
                            path: p,
                            parent: Some(id),
                            depth: depth + 1,
                            children: Some(Vec::new()),
                            expanded: false,
                            meta: e.meta,
                            error: None,
                        }));
                    }
                }
                Err(e) => self.nodes[id].error = Some(e.to_string()),
            },
            _ => {}
        }
        self.nodes[id].children = Some(kids);
    }

    pub fn is_container(&self, id: usize) -> bool {
        matches!(
            self.nodes[id].kind,
            Kind::Dir | Kind::Drive | Kind::Db | Kind::Favorites
        )
    }

    /// Les lignes visibles : les racines, et sous chaque noeud ouvert ses
    /// enfants.
    pub fn rows(&self, out: &mut Vec<usize>) {
        out.clear();
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            let n = &self.nodes[id];
            if n.expanded {
                if let Some(k) = &n.children {
                    stack.extend(k.iter().rev().copied());
                }
            }
        }
    }

    /// Ouvre tous les ancetres d'un chemin deja present dans l'arbre, en
    /// chargeant ce qu'il faut. Rend le noeud du chemin s'il est trouve.
    pub fn reveal(&mut self, target: &Path) -> Option<usize> {
        let roots = self.roots.clone();
        for r in roots {
            if !matches!(self.nodes[r].kind, Kind::Dir | Kind::Drive) {
                continue;
            }
            let base = self.nodes[r].path.clone();
            let Ok(rel) = target.strip_prefix(&base) else {
                continue;
            };
            let mut cur = r;
            let mut ok = true;
            for comp in rel.components() {
                self.load(cur);
                self.nodes[cur].expanded = true;
                let want = comp.as_os_str().to_string_lossy().to_lowercase();
                let next = self.nodes[cur].children.as_ref().and_then(|k| {
                    k.iter()
                        .copied()
                        .find(|&c| self.nodes[c].name.to_lowercase() == want)
                });
                match next {
                    Some(n) => cur = n,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                return Some(cur);
            }
        }
        None
    }

    /// Les ancetres d'un noeud, du plus haut au plus proche.
    pub fn ancestors(&self, id: usize) -> Vec<usize> {
        let mut v = Vec::new();
        let mut p = self.nodes[id].parent;
        while let Some(x) = p {
            v.push(x);
            p = self.nodes[x].parent;
        }
        v.reverse();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_raccourcis_de_reaper_ini() {
        let ini = "[other]\nShortcut0=nope\n[reaper_explorer]\nNbShortcuts=4\nShortcut0=<Project Directory>\nShortcut1=00.ReaperFileList\nShortcutT1=DB: Medias\nShortcut2=D:\\Audio\nShortcut3=09.ReaperFileList\nShortcutT3=---------- DATABASES\nShortcut4=hors compte\n";
        let s = read_shortcuts(ini);
        assert_eq!(s.len(), 4);
        assert_eq!(s[1].title.as_deref(), Some("DB: Medias"));
        let t = Tree::from_shortcuts(
            &s,
            Path::new("C:\\R\\MediaDB"),
            Some(Path::new("C:\\P")),
            &[],
        );
        let kinds: Vec<Kind> = t.roots.iter().map(|&r| t.nodes[r].kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Kind::Favorites,
                Kind::Dir,
                Kind::Db,
                Kind::Dir,
                Kind::Section
            ]
        );
        assert_eq!(
            t.nodes[t.roots[2]].path,
            PathBuf::from("C:\\R\\MediaDB\\00.ReaperFileList")
        );
        assert_eq!(t.nodes[t.roots[4]].name, "DATABASES");
    }

    #[test]
    fn une_base_mediadb() {
        let txt = "PATH \"C:\\x\"\r\nFILE \"C:\\x\\a b.wav\" 123 0 1 0\r\nDATA y:2024 s:48000 n:2 l:0:41.492\r\nDATA i:24 v:0.08\r\nFILE \"C:\\x\\c.flac\" 1 0 1 0\r\n";
        let e = parse_mediadb(txt);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "C:\\x\\a b.wav");
        assert_eq!(e[0].meta.srate, Some(48000));
        assert_eq!(e[0].meta.chans, Some(2));
        assert_eq!(e[0].meta.bits, Some(24));
        assert!((e[0].meta.length_s.unwrap() - 41.492).abs() < 1e-9);
        assert_eq!(e[1].meta, Meta::default());
    }

    #[test]
    fn un_dossier_s_ouvre_dossiers_d_abord_puis_l_audio() {
        let root = std::env::temp_dir().join("cp_browser_model_test");
        let _ = std::fs::create_dir_all(root.join("Zed"));
        let _ = std::fs::create_dir_all(root.join("alpha"));
        for f in ["b.wav", "A.flac", "notes.txt"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let sc = vec![Shortcut {
            target: root.to_string_lossy().into_owned(),
            title: None,
        }];
        let mut t = Tree::from_shortcuts(&sc, Path::new("."), None, &[]);
        let r = t.roots[1];
        t.load(r);
        t.nodes[r].expanded = true;
        let names: Vec<String> = t.nodes[r]
            .children
            .clone()
            .unwrap()
            .iter()
            .map(|&c| t.nodes[c].name.clone())
            .collect();
        assert_eq!(names, vec!["alpha", "Zed", "A.flac", "b.wav"]);
        let mut rows = Vec::new();
        t.rows(&mut rows);
        assert_eq!(rows.len(), 2 + 4);
        let found = t.reveal(&root.join("Zed"));
        assert!(found.is_some());
    }
}
