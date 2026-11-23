//! L'arbre : le dossier ou REAPER enregistre (en tete, le plus recent
//! d'abord), les favoris, les collections, les dossiers de Cedric, les effets.
//!
//! Un arbre en arene : chaque noeud connait ses enfants (charges a la premiere
//! ouverture) et son parent. Les LIGNES visibles sont recalculees quand
//! l'arbre change (ouverture, fermeture, recherche), pas a chaque image.
//!
//! Les raccourcis de l'Explorateur de medias de REAPER ne sont plus montres
//! d'office (Cedric, 2026-11-22 : « il faut nettoyer tout ce qui s'affiche ») :
//! ils s'importent d'un clic parmi ses dossiers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Un titre de section (« FOLDERS »).
    Section,
    /// La racine des favoris.
    Favorites,
    Dir,
    /// Un lecteur (C:\).
    Drive,
    /// Une base MediaDB (`.ReaperFileList`).
    Db,
    File,
    /// Une collection coloree (1 a 7) : les fichiers marques de sa couleur.
    Collection(u8),
    /// La racine des effets installes, leurs familles, un effet.
    FxRoot,
    FxGroup,
    Fx,
    /// La ligne « Add folder… » sous les dossiers de Cedric.
    Add,
}

/// Les couleurs des collections 1 a 7.
pub const COLLECTION_COLORS: [u32; 7] = [
    0xE05252, 0xE8913A, 0xE8C93A, 0x5DBB63, 0x3AB7C9, 0x4C8DFF, 0xA970E8,
];
pub const COLLECTION_NAMES: [&str; 7] =
    ["Red", "Orange", "Yellow", "Green", "Cyan", "Blue", "Purple"];

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
    /// Les enfants du plus recent au plus ancien (le dossier d'enregistrement).
    pub by_date: bool,
    /// La date de modification (fichiers d'un dossier trie par date).
    pub mtime: Option<SystemTime>,
    /// Un dossier ajoute par Cedric (on peut le retirer, le deplacer).
    pub user: bool,
    /// La date du dossier a sa derniere lecture : s'il change, on relit.
    pub seen: Option<SystemTime>,
}

impl Node {
    fn new(
        kind: Kind,
        name: impl Into<String>,
        path: PathBuf,
        parent: Option<usize>,
        depth: u16,
    ) -> Node {
        Node {
            kind,
            name: name.into(),
            path,
            parent,
            depth,
            children: None,
            expanded: false,
            meta: Meta::default(),
            error: None,
            by_date: false,
            mtime: None,
            user: false,
            seen: None,
        }
    }

    /// Un noeud sans enfants a charger (fichier, effet, titre).
    fn leaf(mut self) -> Node {
        self.children = Some(Vec::new());
        self
    }
}

/// Les extensions qu'on montre (ce que REAPER lit).
pub const AUDIO_EXT: [&str; 14] = [
    "wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "opus", "wv", "w64", "rf64", "caf", "m4a",
    "bwf",
];

/// `target` sans son debut `base`, sans tenir compte de la casse (les
/// chemins de Windows).
pub fn strip_prefix_ci(target: &Path, base: &Path) -> Option<PathBuf> {
    let mut t = target.components();
    for b in base.components() {
        let c = t.next()?;
        if !c
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
        {
            return None;
        }
    }
    Some(t.as_path().to_path_buf())
}

pub fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

pub fn is_db(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ReaperFileList"))
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

/// Un dossier de Cedric (ou une base), et le nom qu'il lui donne.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub path: PathBuf,
    pub title: Option<String>,
}

/// Ce que montre l'arbre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Places {
    /// Le dossier ou REAPER enregistre (celui du projet, ou celui par defaut
    /// sans projet).
    pub recordings: Option<PathBuf>,
    /// Les dossiers de Cedric, dans son ordre.
    pub folders: Vec<Place>,
}

impl Places {
    /// Les raccourcis de l'Explorateur de REAPER comme dossiers : dossiers,
    /// lecteurs et bases ; ni les titres de section, ni les `<...>` (sauf le
    /// dossier du projet, s'il y en a un).
    pub fn from_shortcuts(sc: &[Shortcut], mediadb: &Path, project: Option<&Path>) -> Vec<Place> {
        let mut out = Vec::new();
        for s in sc {
            let target = s.target.trim();
            let title = s.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
            if title.is_some_and(|t| t.starts_with("---")) {
                continue;
            }
            let path = if target.eq_ignore_ascii_case("<Project Directory>") {
                match project {
                    Some(p) => p.to_path_buf(),
                    None => continue,
                }
            } else if target.starts_with('<') {
                continue;
            } else if is_db(Path::new(target)) && !Path::new(target).is_absolute() {
                mediadb.join(target)
            } else {
                PathBuf::from(target.replace('/', "\\"))
            };
            if !out.iter().any(|p: &Place| p.path == path) {
                out.push(Place {
                    path,
                    title: title.map(str::to_string),
                });
            }
        }
        out
    }

    /// Les dossiers en une ligne : `chemin*titre|chemin|...` (ni `|` ni `*`
    /// ne peuvent etre dans un chemin Windows).
    pub fn folders_text(folders: &[Place]) -> String {
        folders
            .iter()
            .map(|p| {
                let path = p.path.to_string_lossy();
                match &p.title {
                    Some(t) => format!("{path}*{}", t.replace(['|', '*'], "/")),
                    None => path.into_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    pub fn parse_folders(s: &str) -> Vec<Place> {
        s.split('|')
            .filter(|e| !e.trim().is_empty())
            .map(|e| match e.split_once('*') {
                Some((p, t)) => Place {
                    path: PathBuf::from(p),
                    title: Some(t.to_string()).filter(|t| !t.is_empty()),
                },
                None => Place {
                    path: PathBuf::from(e),
                    title: None,
                },
            })
            .collect()
    }
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

/// L'age d'un fichier, court : « just now », « 4 min ago », « 3 h ago »,
/// « 2 days ago », « 5 weeks ago ».
pub fn age(t: SystemTime, now: SystemTime) -> String {
    let s = now.duration_since(t).map_or(0, |d| d.as_secs());
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        172_800..=1_209_599 => format!("{} days ago", s / 86_400),
        1_209_600..=7_775_999 => format!("{} weeks ago", s / 604_800),
        _ => format!("{} months ago", s / 2_592_000),
    }
}

/// La date de modification d'un chemin.
pub fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[derive(Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
    pub favorites: usize,
    pub collections: [usize; 7],
    pub fx_root: usize,
    /// Le dossier d'enregistrement (en tete), s'il est connu.
    pub recordings: Option<usize>,
    /// Le titre « FOLDERS » et la ligne « Add folder… ».
    pub folders_title: usize,
    pub add_row: usize,
}

impl Tree {
    fn add(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn place_node(p: &Place) -> Node {
        let kind = if is_db(&p.path) {
            Kind::Db
        } else if p.path.parent().is_none() {
            Kind::Drive
        } else {
            Kind::Dir
        };
        let name = p.title.clone().unwrap_or_else(|| display_name(&p.path));
        let mut n = Node::new(kind, name, p.path.clone(), None, 0);
        n.user = true;
        n
    }

    /// L'arbre : le dossier d'enregistrement ouvert en tete, les favoris, les
    /// collections, les dossiers de Cedric, les effets.
    pub fn build(places: &Places, favs: &[PathBuf]) -> Tree {
        let mut t = Tree::default();
        if let Some(rec) = &places.recordings {
            let mut n = Node::new(Kind::Dir, "Recordings", rec.clone(), None, 0);
            n.by_date = true;
            let id = t.add(n);
            t.recordings = Some(id);
            t.roots.push(id);
            t.load(id);
            t.nodes[id].expanded = true;
        }
        let fav = t.add(Node::new(Kind::Favorites, "Favorites", PathBuf::new(), None, 0).leaf());
        t.favorites = fav;
        t.roots.push(fav);
        t.set_favorites(favs);
        for i in 0..7u8 {
            let id = t.add(
                Node::new(
                    Kind::Collection(i + 1),
                    COLLECTION_NAMES[i as usize],
                    PathBuf::new(),
                    None,
                    0,
                )
                .leaf(),
            );
            t.collections[i as usize] = id;
            t.roots.push(id);
        }
        let title = t.add(Node::new(Kind::Section, "Folders", PathBuf::new(), None, 0).leaf());
        t.folders_title = title;
        t.roots.push(title);
        for p in &places.folders {
            let id = t.add(Tree::place_node(p));
            t.roots.push(id);
        }
        let add = t.add(Node::new(Kind::Add, "Add folder…", PathBuf::new(), None, 0).leaf());
        t.add_row = add;
        t.roots.push(add);
        let fx = t.add(Node::new(Kind::FxRoot, "Effects", PathBuf::new(), None, 0));
        t.fx_root = fx;
        t.roots.push(fx);
        t
    }

    /// Les dossiers de Cedric, dans l'ordre de l'arbre.
    pub fn user_places(&self) -> Vec<Place> {
        self.roots
            .iter()
            .filter(|&&r| self.nodes[r].user)
            .map(|&r| {
                let n = &self.nodes[r];
                let auto = display_name(&n.path);
                Place {
                    path: n.path.clone(),
                    title: (n.name != auto).then(|| n.name.clone()),
                }
            })
            .collect()
    }

    /// Ajoute un dossier de Cedric (a la fin) ; rend son noeud, ou celui qui
    /// existait deja.
    pub fn add_place(&mut self, p: &Place) -> usize {
        if let Some(&r) = self
            .roots
            .iter()
            .find(|&&r| self.nodes[r].user && self.nodes[r].path == p.path)
        {
            return r;
        }
        let id = self.add(Tree::place_node(p));
        let at = self
            .roots
            .iter()
            .position(|&r| r == self.add_row)
            .unwrap_or(self.roots.len());
        self.roots.insert(at, id);
        id
    }

    /// Retire un dossier de Cedric de l'arbre.
    pub fn remove_place(&mut self, id: usize) {
        if self.nodes[id].user {
            self.roots.retain(|&r| r != id);
        }
    }

    /// Deplace un dossier de Cedric d'un rang parmi les siens (-1 : plus haut).
    pub fn move_place(&mut self, id: usize, dir: i32) {
        let users: Vec<usize> = self
            .roots
            .iter()
            .copied()
            .filter(|&r| self.nodes[r].user)
            .collect();
        let Some(k) = users.iter().position(|&r| r == id) else {
            return;
        };
        let j = k as i32 + dir;
        if j < 0 || j >= users.len() as i32 {
            return;
        }
        let other = users[j as usize];
        let (a, b) = (
            self.roots.iter().position(|&r| r == id),
            self.roots.iter().position(|&r| r == other),
        );
        if let (Some(a), Some(b)) = (a, b) {
            self.roots.swap(a, b);
        }
    }

    /// Change le dossier d'enregistrement (un autre projet) : relu s'il change.
    pub fn set_recordings(&mut self, p: Option<&Path>) -> bool {
        let cur = self.recordings.map(|id| self.nodes[id].path.clone());
        if cur.as_deref() == p {
            return false;
        }
        match (self.recordings, p) {
            (Some(id), Some(p)) => {
                let n = &mut self.nodes[id];
                n.path = p.to_path_buf();
                n.children = None;
                n.error = None;
                n.seen = None;
                if n.expanded {
                    self.load(id);
                }
            }
            (Some(id), None) => {
                self.roots.retain(|&r| r != id);
                self.recordings = None;
            }
            (None, Some(p)) => {
                let mut n = Node::new(Kind::Dir, "Recordings", p.to_path_buf(), None, 0);
                n.by_date = true;
                let id = self.add(n);
                self.recordings = Some(id);
                self.roots.insert(0, id);
                self.load(id);
                self.nodes[id].expanded = true;
            }
            (None, None) => {}
        }
        true
    }

    /// Les fichiers d'une collection (1 a 7).
    pub fn set_collection(&mut self, n: u8, paths: &[PathBuf]) {
        let root = self.collections[(n - 1) as usize];
        let mut kids = Vec::new();
        for p in paths {
            let kind = if is_audio(p) { Kind::File } else { Kind::Dir };
            kids.push(self.add(Node::new(kind, display_name(p), p.clone(), Some(root), 1)));
        }
        self.nodes[root].children = Some(kids);
    }

    /// Les effets installes, ranges par famille (« VST3 », « JS »...).
    pub fn set_fx(&mut self, names: &[String]) {
        let root = self.fx_root;
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for n in names {
            let fam = n
                .split_once(':')
                .map_or("Other", |(a, _)| a.trim())
                .to_string();
            match groups.iter_mut().find(|(g, _)| *g == fam) {
                Some((_, v)) => v.push(n.clone()),
                None => groups.push((fam, vec![n.clone()])),
            }
        }
        groups.sort_by_key(|(g, _)| g.to_lowercase());
        let mut kids = Vec::new();
        for (g, mut v) in groups {
            v.sort_by_key(|x| x.to_lowercase());
            let gid = self.add(Node::new(
                Kind::FxGroup,
                format!("{g} ({})", v.len()),
                PathBuf::new(),
                Some(root),
                1,
            ));
            let mut leaves = Vec::new();
            for n in v {
                let short = n
                    .split_once(':')
                    .map_or(n.as_str(), |(_, b)| b.trim())
                    .to_string();
                // Le nom complet sert a TrackFX_AddByName.
                leaves.push(
                    self.add(Node::new(Kind::Fx, short, PathBuf::from(n), Some(gid), 2).leaf()),
                );
            }
            self.nodes[gid].children = Some(leaves);
            kids.push(gid);
        }
        self.nodes[root].children = Some(kids);
    }

    /// Remplace les enfants des favoris.
    pub fn set_favorites(&mut self, favs: &[PathBuf]) {
        let fav = self.favorites;
        let mut kids = Vec::new();
        for p in favs {
            let kind = if is_audio(p) { Kind::File } else { Kind::Dir };
            kids.push(self.add(Node::new(kind, display_name(p), p.clone(), Some(fav), 1)));
        }
        self.nodes[fav].children = Some(kids);
    }

    /// Charge les enfants d'un dossier ou d'une base (une fois).
    pub fn load(&mut self, id: usize) {
        // La racine des effets est remplie par le navigateur (la liste vient
        // de REAPER), jamais par le disque.
        if self.nodes[id].children.is_some() || self.nodes[id].kind == Kind::FxRoot {
            return;
        }
        let (kind, path, depth, by_date) = {
            let n = &self.nodes[id];
            (n.kind.clone(), n.path.clone(), n.depth, n.by_date)
        };
        let mut kids = Vec::new();
        match kind {
            Kind::Dir | Kind::Drive => {
                self.nodes[id].seen = modified(&path);
                match std::fs::read_dir(&path) {
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
                            let mtime = if by_date {
                                e.metadata().and_then(|m| m.modified()).ok()
                            } else {
                                None
                            };
                            if is_dir {
                                dirs.push((name, p, mtime));
                            } else if is_audio(&p) {
                                files.push((name, p, mtime));
                            }
                        }
                        if by_date {
                            // Le plus recent d'abord : un enregistrement neuf
                            // arrive en haut.
                            files.sort_by_key(|f| std::cmp::Reverse(f.2));
                            dirs.sort_by_key(|d| std::cmp::Reverse(d.2));
                        } else {
                            dirs.sort_by_key(|a| a.0.to_lowercase());
                            files.sort_by_key(|a| a.0.to_lowercase());
                        }
                        let mut push = |t: &mut Tree, kind: Kind, (name, p, mtime): (String, PathBuf, Option<SystemTime>)| {
                            let leaf = kind == Kind::File;
                            let mut n = Node::new(kind, name, p, Some(id), depth + 1);
                            n.mtime = mtime;
                            if leaf {
                                n.children = Some(Vec::new());
                            }
                            kids.push(t.add(n));
                        };
                        if by_date {
                            for f in files {
                                push(self, Kind::File, f);
                            }
                            for d in dirs {
                                push(self, Kind::Dir, d);
                            }
                        } else {
                            for d in dirs {
                                push(self, Kind::Dir, d);
                            }
                            for f in files {
                                push(self, Kind::File, f);
                            }
                        }
                    }
                    Err(e) => self.nodes[id].error = Some(e.to_string()),
                }
            }
            Kind::Db => match std::fs::read(&path) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut entries = parse_mediadb(&text);
                    entries.sort_by_cached_key(|e| display_name(Path::new(&e.path)).to_lowercase());
                    for e in entries {
                        let p = PathBuf::from(&e.path);
                        let mut n =
                            Node::new(Kind::File, display_name(&p), p, Some(id), depth + 1).leaf();
                        n.meta = e.meta;
                        kids.push(self.add(n));
                    }
                }
                Err(e) => self.nodes[id].error = Some(e.to_string()),
            },
            _ => {}
        }
        self.nodes[id].children = Some(kids);
    }

    /// Les dossiers ouverts que le disque a changes depuis leur lecture
    /// (un fichier ajoute, retire, renomme).
    pub fn stale(&self, limit: usize) -> Vec<usize> {
        // Seuls les dossiers visibles (ouverts depuis une racine) : l'arene
        // garde les anciens noeuds d'un dossier relu, qu'on ne regarde plus.
        let mut open = Vec::new();
        let mut stack: Vec<usize> = self.roots.clone();
        while let Some(id) = stack.pop() {
            let n = &self.nodes[id];
            if !n.expanded {
                continue;
            }
            if let Some(k) = &n.children {
                if matches!(n.kind, Kind::Dir | Kind::Drive) {
                    open.push(id);
                }
                stack.extend(k.iter().copied());
            }
            if open.len() >= limit {
                break;
            }
        }
        open.into_iter()
            .filter(|&id| {
                let n = &self.nodes[id];
                n.seen.is_some() && modified(&n.path) != n.seen
            })
            .collect()
    }

    pub fn is_container(&self, id: usize) -> bool {
        matches!(
            self.nodes[id].kind,
            Kind::Dir
                | Kind::Drive
                | Kind::Db
                | Kind::Favorites
                | Kind::Collection(_)
                | Kind::FxRoot
                | Kind::FxGroup
        )
    }

    /// Les lignes visibles : les racines, et sous chaque noeud ouvert ses
    /// enfants.
    pub fn rows(&self, out: &mut Vec<usize>) {
        out.clear();
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            // Une collection vide, des favoris vides : rien a montrer.
            if matches!(self.nodes[id].kind, Kind::Collection(_) | Kind::Favorites)
                && self.nodes[id]
                    .children
                    .as_ref()
                    .is_none_or(|k| k.is_empty())
            {
                continue;
            }
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
            // Windows ignore la casse : « c:\Samples » est « C:\samples ».
            let Some(rel) = strip_prefix_ci(target, &base) else {
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
    fn les_raccourcis_de_reaper_deviennent_des_dossiers() {
        let ini = "[other]\nShortcut0=nope\n[reaper_explorer]\nNbShortcuts=4\nShortcut0=<Project Directory>\nShortcut1=00.ReaperFileList\nShortcutT1=DB: Medias\nShortcut2=D:\\Audio\nShortcut3=09.ReaperFileList\nShortcutT3=---------- DATABASES\nShortcut4=hors compte\n";
        let s = read_shortcuts(ini);
        assert_eq!(s.len(), 4);
        assert_eq!(s[1].title.as_deref(), Some("DB: Medias"));
        let p = Places::from_shortcuts(&s, Path::new("C:\\R\\MediaDB"), Some(Path::new("C:\\P")));
        // Le titre de section ne devient pas un dossier.
        assert_eq!(
            p.iter().map(|x| x.path.clone()).collect::<Vec<_>>(),
            vec![
                PathBuf::from("C:\\P"),
                PathBuf::from("C:\\R\\MediaDB\\00.ReaperFileList"),
                PathBuf::from("D:\\Audio"),
            ]
        );
        // Aller-retour du texte range.
        let txt = Places::folders_text(&p);
        assert_eq!(Places::parse_folders(&txt), p);
        // L'arbre : titre, les trois, la ligne d'ajout ; la base est une base.
        let t = Tree::build(
            &Places {
                recordings: None,
                folders: p,
            },
            &[],
        );
        let mut rows = Vec::new();
        t.rows(&mut rows);
        let kinds: Vec<Kind> = rows.iter().map(|&r| t.nodes[r].kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Kind::Section,
                Kind::Dir,
                Kind::Db,
                Kind::Dir,
                Kind::Add,
                Kind::FxRoot
            ]
        );
        assert_eq!(t.nodes[rows[2]].name, "DB: Medias");
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
        let places = Places {
            recordings: None,
            folders: vec![Place {
                path: root.clone(),
                title: None,
            }],
        };
        let mut t = Tree::build(&places, &[]);
        let r = *t.roots.iter().find(|&&r| t.nodes[r].user).unwrap();
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
        // Le titre, le dossier et ses 4 enfants, la ligne d'ajout, les effets
        // (favoris et collections vides ne se montrent pas).
        assert_eq!(rows.len(), 1 + 1 + 4 + 1 + 1);
        let found = t.reveal(&root.join("Zed"));
        assert!(found.is_some());
    }

    #[test]
    fn le_dossier_d_enregistrement_le_plus_recent_d_abord_et_relu() {
        let root = std::env::temp_dir().join("cp_browser_rec_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("old.wav"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(root.join("new.wav"), b"x").unwrap();
        let places = Places {
            recordings: Some(root.clone()),
            folders: Vec::new(),
        };
        let mut t = Tree::build(&places, &[]);
        let rec = t.recordings.unwrap();
        assert!(t.nodes[rec].expanded, "ouvert d'office");
        let names = |t: &Tree| -> Vec<String> {
            t.nodes[rec]
                .children
                .clone()
                .unwrap()
                .iter()
                .map(|&c| t.nodes[c].name.clone())
                .collect()
        };
        assert_eq!(names(&t), vec!["new.wav", "old.wav"]);
        assert!(t.stale(64).is_empty());
        // Un enregistrement arrive : le dossier change, on le relit.
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(root.join("take 3.wav"), b"x").unwrap();
        assert_eq!(t.stale(64), vec![rec]);
        t.nodes[rec].children = None;
        t.load(rec);
        assert_eq!(names(&t)[0], "take 3.wav");
        assert!(t.stale(64).is_empty());
        // Un autre projet : un autre dossier.
        assert!(t.set_recordings(Some(&std::env::temp_dir())));
        assert!(!t.set_recordings(Some(&std::env::temp_dir())));
    }

    #[test]
    fn ajouter_deplacer_retirer_un_dossier() {
        let mut t = Tree::build(&Places::default(), &[]);
        let a = t.add_place(&Place {
            path: PathBuf::from("C:\\A"),
            title: None,
        });
        let b = t.add_place(&Place {
            path: PathBuf::from("D:\\B"),
            title: Some("Bee".into()),
        });
        // Deux fois le meme : un seul.
        assert_eq!(
            t.add_place(&Place {
                path: PathBuf::from("C:\\A"),
                title: None
            }),
            a
        );
        let order = |t: &Tree| {
            t.user_places()
                .iter()
                .map(|p| p.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            order(&t),
            vec![PathBuf::from("C:\\A"), PathBuf::from("D:\\B")]
        );
        t.move_place(b, -1);
        assert_eq!(
            order(&t),
            vec![PathBuf::from("D:\\B"), PathBuf::from("C:\\A")]
        );
        assert_eq!(t.user_places()[0].title.as_deref(), Some("Bee"));
        t.remove_place(b);
        assert_eq!(order(&t), vec![PathBuf::from("C:\\A")]);
        // La ligne d'ajout reste derniere des dossiers.
        let pos = |id: usize| t.roots.iter().position(|&r| r == id).unwrap();
        assert!(pos(a) < pos(t.add_row));
        let _ = age(SystemTime::now(), SystemTime::now());
    }
}
