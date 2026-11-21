//! Le navigateur de fichiers de CP Studio (jalon 1).
//!
//! Il ne connait ni Windows ni REAPER : il recoit des evenements ([`Ev`]),
//! demande ce qu'il lui faut a un [`Host`] (le moteur audio, l'insertion dans
//! le projet, la persistance), et se dessine dans un `Canvas`. Le module le
//! branche sur une vraie fenetre ; les tests le branchent sur un faux hote et
//! le dessinent en PNG.
//!
//! Ce qu'il fait :
//! - l'arbre des raccourcis de l'Explorateur de REAPER, les dossiers ouverts en
//!   ligne, les en-tetes de dossier qui restent colles en haut, empiles ;
//! - l'ecoute au RELACHEMENT du clic (un glisser reste muet), et des la
//!   selection au clavier ;
//! - la forme d'onde a tout zoom, une section choisie (jusqu'a 1 echantillon)
//!   qui s'ecoute seule ;
//! - la recherche (noms et bases MediaDB), les favoris, le clavier complet.

// Le dessin prend naturellement x, y, w, h, rayon, epaisseur, couleur... : des
// parametres positionnels nombreux sont la forme juste ici.
#![allow(clippy::too_many_arguments)]

pub mod draw;
pub mod model;
pub mod v2;
pub mod wave;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use cp_ui::scroll::Scroll;
use cp_ui::textfield::Edit;
use cp_ui::{Button, Ev, Mods, TextField, vk};

use model::{DbEntry, Kind, Meta, Shortcut, Tree};
use wave::{ClipInfo, Wave};

/// Un chargement en cours, vu du navigateur.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded {
    Pending,
    Ready(u32),
    Failed(String),
}

/// Ce que le navigateur demande au monde.
pub trait Host {
    fn load(&mut self, path: &str) -> u32;
    fn poll(&mut self, ticket: u32) -> Loaded;
    fn info(&mut self, clip: u32) -> Option<ClipInfo>;
    fn release(&mut self, clip: u32);
    #[allow(clippy::too_many_arguments)]
    fn peaks(
        &mut self,
        clip: u32,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        min: &mut [f32],
        max: &mut [f32],
    ) -> usize;
    /// Rend la voix (0 = refuse).
    fn play(&mut self, clip: u32, start: i64, end: i64, looped: bool, gain: f64) -> u32;
    fn stop(&mut self, voice: u32, fade_s: f64);
    /// (position en frames, etat) d'une voix vivante.
    fn voice(&mut self, voice: u32) -> Option<(f64, i32)>;
    fn set_gain(&mut self, voice: u32, gain: f64);
    /// Pose le fichier (ou sa section, en secondes) dans le projet.
    fn insert(&mut self, path: &str, section: Option<(f64, f64)>) -> Result<(), String>;
    /// Commence un glisser-deposer Windows (bloquant). Faux si non disponible.
    fn drag_out(&mut self, _path: &str, _section: Option<(f64, f64)>) -> bool {
        false
    }
    fn set_clipboard(&mut self, _s: &str) {}
    fn clipboard(&mut self) -> Option<String> {
        None
    }
    /// Un menu contextuel (x, y dans la vue) ; rend l'identifiant choisi.
    fn menu(&mut self, _x: f32, _y: f32, _items: &[MenuEntry]) -> u32 {
        0
    }
    /// Range un reglage durable.
    fn save(&mut self, _key: &str, _value: &str) {}
    /// Les lignes de menu de la FENETRE (ancrer, fermer...), identifiants
    /// >= 1000, ajoutees a chaque menu contextuel.
    fn window_items(&mut self) -> Vec<MenuEntry> {
        Vec::new()
    }
    fn window_action(&mut self, _id: u32) {}

    // --- V2 (par defaut : absent)
    /// Le tempo du projet.
    fn project_bpm(&mut self) -> f64 {
        0.0
    }
    fn set_rate(&mut self, _voice: u32, _rate: f64) {}
    /// Apres une insertion : la vitesse de l'item pose.
    fn insert_rate(&mut self, _rate: f64) {}
    /// Le fichier et le decalage de l'item choisi dans REAPER.
    fn swap_begin(&mut self) -> Result<(String, f64), String> {
        Err("hot swap is not available".into())
    }
    fn swap(&mut self, _path: &str, _offset: f64) -> Result<(), String> {
        Err("hot swap is not available".into())
    }
    fn installed_fx(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn add_fx(&mut self, _name: &str) -> Result<(), String> {
        Err("effects are not available".into())
    }
}

/// Une ligne de menu (`id` 0 = separateur).
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub id: u32,
    pub text: String,
    pub checked: bool,
}

impl MenuEntry {
    pub fn new(id: u32, text: &str, checked: bool) -> MenuEntry {
        MenuEntry {
            id,
            text: text.to_string(),
            checked,
        }
    }
    pub fn sep() -> MenuEntry {
        MenuEntry {
            id: 0,
            text: String::new(),
            checked: false,
        }
    }
}

/// Une ligne de la liste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Node(usize),
    Hit(usize),
}

/// Un resultat de recherche.
#[derive(Clone, Debug)]
pub struct Hit {
    pub path: PathBuf,
    pub name: String,
    pub dir: String,
    pub meta: Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    List,
    Search,
}

/// Ce que le module doit faire d'une touche vue par le crochet clavier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUse {
    /// Le navigateur la traite tout de suite (et REAPER ne la voit pas).
    Eat,
    /// A livrer normalement (saisie de texte : WM_KEYDOWN puis WM_CHAR).
    Text,
    /// Pas pour le navigateur : REAPER la traite (ses raccourcis).
    NotMine,
}

/// La geometrie de la vue, en pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    pub row_h: f32,
    pub list_y: f32,
    pub list_h: f32,
    pub wave_y: f32,
    pub wave_h: f32,
    pub status_y: f32,
    pub status_h: f32,
    pub scroll_w: f32,
    pub search_x: f32,
    pub search_w: f32,
    pub btn: f32,
    pub loop_x: f32,
    pub play_x: f32,
    pub vol_x: f32,
    pub vol_w: f32,
    pub star_x: f32,
    pub sync_x: f32,
}

/// Un appui de souris en cours sur la liste.
#[derive(Clone, Copy, Debug)]
struct Press {
    x: f32,
    y: f32,
    row: Option<usize>,
    dragging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    None,
    Scrollbar,
    Volume,
    Wave,
    WavePan,
    /// La couture entre la liste et la forme d'onde.
    Split,
}

/// Un clip garde en memoire (les derniers ecoutes).
#[derive(Clone, Debug)]
struct Cached {
    path: String,
    clip: u32,
    info: ClipInfo,
}

const CACHE_MAX: usize = 12;
/// Au plus 1,5 Go de fichiers gardes en memoire.
const CACHE_BYTES: i64 = 1_500_000_000;

pub struct Browser {
    pub tree: Tree,
    pub rows: Vec<Row>,
    pub hits: Vec<Hit>,
    pub search: TextField,
    pub focus: Focus,
    pub scroll: Scroll,
    pub sel: Option<usize>,
    pub hover: Option<usize>,
    pub wave: Wave,
    pub looping: bool,
    /// Position du curseur de volume, 0..1 (le gain en est deduit).
    pub volume: f32,
    pub status: String,
    pub error: Option<String>,
    pub lay: Layout,
    favorites: Vec<PathBuf>,
    cache: Vec<Cached>,
    /// (chemin, ticket, jouer a l'arrivee, depart, fin).
    pending: Option<(String, u32, bool)>,
    voice: u32,
    voice_path: String,
    press: Option<Press>,
    grab: Grab,
    db_job: Option<JoinHandle<Vec<DbEntry>>>,
    db: Option<Vec<DbEntry>>,
    db_wanted: bool,
    mediadb_dir: PathBuf,
    /// Le texte de la derniere recherche faite (pour ne pas la refaire).
    searched: String,
    dirty: bool,
    pub scale: f32,
    /// Le calage au tempo du projet.
    pub sync: bool,
    pub swap: Option<v2::Swap>,
    pub collections: v2::Collections,
    /// Ou ranger les collections.
    pub data_dir: Option<PathBuf>,
    seed: u64,
    /// La hauteur voulue de la forme d'onde (a 96 DPI).
    pub wave_h_pref: f32,
    /// La racine des effets demande sa liste au prochain evenement.
    fx_wanted: bool,
}

impl Browser {
    pub fn new(
        shortcuts: &[Shortcut],
        mediadb_dir: &Path,
        project: Option<&Path>,
        favorites: Vec<PathBuf>,
    ) -> Browser {
        let tree = Tree::from_shortcuts(shortcuts, mediadb_dir, project, &favorites);
        let mut b = Browser {
            tree,
            rows: Vec::new(),
            hits: Vec::new(),
            search: TextField::default(),
            focus: Focus::List,
            scroll: Scroll::default(),
            sel: None,
            hover: None,
            wave: Wave::default(),
            looping: false,
            volume: 0.794,
            status: String::new(),
            error: None,
            lay: Layout::default(),
            favorites,
            cache: Vec::new(),
            pending: None,
            voice: 0,
            voice_path: String::new(),
            press: None,
            grab: Grab::None,
            db_job: None,
            db: None,
            db_wanted: false,
            mediadb_dir: mediadb_dir.to_path_buf(),
            searched: String::new(),
            dirty: true,
            scale: 1.0,
            sync: false,
            swap: None,
            collections: v2::Collections::new(),
            data_dir: None,
            seed: v2::seed(),
            wave_h_pref: 120.0,
            fx_wanted: false,
        };
        b.rebuild();
        b
    }

    /// Le gain : une loi cubique, 0 dB a 0,794, +6 dB au bout.
    pub fn gain(&self) -> f64 {
        let v = self.volume.clamp(0.0, 1.0) as f64;
        v * v * v * 2.0
    }

    pub fn gain_db(&self) -> f64 {
        let g = self.gain();
        if g <= 1e-6 {
            f64::NEG_INFINITY
        } else {
            20.0 * g.log10()
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    /// Joue-t-on, charge-t-on : faut-il des images a la cadence de l'ecran ?
    pub fn animating(&self) -> bool {
        self.voice != 0
            || self.pending.is_some()
            || self.db_job.is_some()
            || self.grab != Grab::None
            || (self.wave.has() && !self.wave.info.complete())
    }

    // -----------------------------------------------------------------------
    // Les lignes
    // -----------------------------------------------------------------------

    fn searching(&self) -> bool {
        !self.search.text.trim().is_empty()
    }

    /// Recalcule les lignes visibles en gardant la ligne choisie.
    pub fn rebuild(&mut self) {
        let keep = self.sel.and_then(|i| self.rows.get(i).copied());
        if self.searching() {
            self.rows = (0..self.hits.len()).map(Row::Hit).collect();
        } else {
            let mut v = Vec::new();
            self.tree.rows(&mut v);
            self.rows = v.into_iter().map(Row::Node).collect();
        }
        self.sel = keep.and_then(|k| self.rows.iter().position(|&r| r == k));
        self.layout_scroll();
        self.dirty = true;
    }

    fn layout_scroll(&mut self) {
        let content = self.rows.len() as f32 * self.lay.row_h;
        self.scroll.set(content, self.lay.list_h);
    }

    pub fn row_path(&self, r: Row) -> Option<&Path> {
        match r {
            Row::Node(id) => {
                let n = &self.tree.nodes[id];
                (!n.path.as_os_str().is_empty()).then_some(n.path.as_path())
            }
            Row::Hit(i) => self.hits.get(i).map(|h| h.path.as_path()),
        }
    }

    pub fn row_is_file(&self, r: Row) -> bool {
        match r {
            Row::Node(id) => self.tree.nodes[id].kind == Kind::File,
            Row::Hit(_) => true,
        }
    }

    pub fn selected_path_pub(&self) -> Option<PathBuf> {
        self.selected_path()
    }

    pub fn select_row_pub(&mut self, i: usize, h: &mut dyn Host, play: bool) {
        self.select_row(i, h, play)
    }

    pub fn insert_selected_pub(&mut self, h: &mut dyn Host) {
        self.insert_selected(h)
    }

    fn selected_path(&self) -> Option<PathBuf> {
        let r = *self.rows.get(self.sel?)?;
        self.row_path(r).map(Path::to_path_buf)
    }

    // -----------------------------------------------------------------------
    // La geometrie
    // -----------------------------------------------------------------------

    pub fn layout(&mut self, w: f32, h: f32, scale: f32) {
        self.scale = scale;
        let s = |v: f32| (v * scale).round();
        let bar_h = s(34.0);
        let status_h = s(22.0);
        let row_h = s(22.0);
        let wave_h = s(self.wave_h_pref.clamp(40.0, 2000.0))
            .min((h - bar_h - status_h - s(80.0)).max(s(30.0)))
            .max(s(30.0).min(h * 0.3));
        let btn = s(26.0);
        let pad = s(6.0);
        let vol_w = s(70.0);
        let star_x = w - pad - btn;
        let vol_x = star_x - s(8.0) - vol_w;
        let play_x = vol_x - s(8.0) - btn;
        let loop_x = play_x - s(4.0) - btn;
        let sync_x = loop_x - s(4.0) - btn;
        let search_x = pad;
        let search_w = (sync_x - s(8.0) - search_x).max(s(40.0));
        let list_y = bar_h + 1.0;
        let wave_y = h - status_h - wave_h;
        self.lay = Layout {
            w,
            h,
            bar_h,
            row_h,
            list_y,
            list_h: (wave_y - 1.0 - list_y).max(0.0),
            wave_y,
            wave_h,
            status_y: h - status_h,
            status_h,
            scroll_w: s(10.0),
            search_x,
            search_w,
            btn,
            loop_x,
            play_x,
            vol_x,
            vol_w,
            star_x,
            sync_x,
        };
        self.wave.layout(w);
        self.layout_scroll();
        self.dirty = true;
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        let l = &self.lay;
        if y < l.list_y || y >= l.list_y + l.list_h {
            return None;
        }
        let i = ((y - l.list_y + self.scroll.pos) / l.row_h).floor() as usize;
        // Un en-tete colle couvre la ligne dessous : c'est lui qu'on touche.
        let sticky = self.sticky();
        let k = ((y - l.list_y) / l.row_h).floor() as usize;
        if k < sticky.len() {
            return self.rows.iter().position(|&r| r == Row::Node(sticky[k]));
        }
        (i < self.rows.len()).then_some(i)
    }

    /// Les en-tetes colles : les dossiers ouverts dont la ligne est passee
    /// au-dessus du haut de la liste, mais dont des enfants sont encore
    /// visibles. Point fixe : chaque en-tete pose couvre une ligne de plus.
    pub fn sticky(&self) -> Vec<usize> {
        if self.searching() || self.rows.is_empty() || self.lay.row_h <= 0.0 {
            return Vec::new();
        }
        let rh = self.lay.row_h;
        let mut n = 0usize;
        for _ in 0..16 {
            let y = self.scroll.pos + n as f32 * rh;
            let i = (y / rh).floor() as usize;
            let Some(&Row::Node(id)) = self.rows.get(i) else {
                break;
            };
            let mut anc = self.tree.ancestors(id);
            // Un dossier ouvert tout en haut est son propre en-tete.
            if self.tree.nodes[id].expanded
                && self.tree.is_container(id)
                && (i as f32) * rh < self.scroll.pos
            {
                anc.push(id);
            }
            // Seuls les ancetres dont la ligne est hors de vue.
            anc.retain(|&a| {
                self.rows
                    .iter()
                    .position(|&r| r == Row::Node(a))
                    .is_some_and(|ri| (ri as f32) * rh < self.scroll.pos + (n as f32) * rh)
            });
            if anc.len() == n {
                return anc;
            }
            n = anc.len();
        }
        Vec::new()
    }

    // -----------------------------------------------------------------------
    // L'ecoute
    // -----------------------------------------------------------------------

    fn cached(&mut self, path: &str) -> Option<Cached> {
        let i = self.cache.iter().position(|c| c.path == path)?;
        let c = self.cache.remove(i);
        self.cache.push(c.clone());
        Some(c)
    }

    fn remember(&mut self, h: &mut dyn Host, c: Cached) {
        self.cache.retain(|x| x.path != c.path);
        self.cache.push(c);
        let bytes = |v: &Vec<Cached>| {
            v.iter()
                .map(|c| c.info.frames * c.info.nch as i64 * 4)
                .sum::<i64>()
        };
        while self.cache.len() > CACHE_MAX
            || (self.cache.len() > 1 && bytes(&self.cache) > CACHE_BYTES)
        {
            let old = self.cache.remove(0);
            if old.clip != self.wave.clip {
                h.release(old.clip);
            }
        }
    }

    /// Arrete l'ecoute (fondu court).
    pub fn stop(&mut self, h: &mut dyn Host) {
        if self.voice != 0 {
            h.stop(self.voice, 0.01);
            self.voice = 0;
        }
        self.wave.playhead = None;
        self.dirty = true;
    }

    /// Montre (et joue si `play`) le fichier choisi.
    pub fn select_file(&mut self, h: &mut dyn Host, play: bool) {
        let Some(p) = self.selected_path() else {
            return;
        };
        let Some(&r) = self.sel.and_then(|i| self.rows.get(i)) else {
            return;
        };
        if !self.row_is_file(r) {
            return;
        }
        let ps = p.to_string_lossy().into_owned();
        self.error = None;
        if self.swap.is_some() && play {
            // En remplacement a chaud, on n'ecoute pas : le fichier va dans
            // l'item, et c'est le projet qui joue.
            self.swap_to_selected(h);
        }
        let play = play && self.swap.is_none();
        if let Some(c) = self.cached(&ps) {
            self.wave.set_clip(c.clip, c.info, &ps);
            self.wave.layout(self.lay.w);
            if play {
                self.play_wave(h, None);
            }
        } else {
            if let Some((old, t, _)) = self.pending.take() {
                // Un chargement abandonne : son clip sera rendu a l'arrivee.
                let _ = (old, t);
            }
            let t = h.load(&ps);
            self.pending = Some((ps, t, play));
            self.status = "Loading…".into();
        }
        self.dirty = true;
    }

    /// Joue le fichier montre : la section, ou a partir de `from` (frames), ou
    /// du debut.
    pub fn play_wave(&mut self, h: &mut dyn Host, from: Option<i64>) {
        if !self.wave.has() {
            return;
        }
        self.stop(h);
        let (a, b) = match (self.wave.sel, from) {
            (Some((a, b)), None) => (a, b),
            (Some((a, b)), Some(f)) if f >= a && f < b => (f, b),
            (_, Some(f)) => (f, 0),
            (None, None) => (0, 0),
        };
        self.voice = h.play(self.wave.clip, a, b, self.looping, self.gain());
        if self.voice != 0 {
            if let Some(r) = self.sync_rate(h) {
                h.set_rate(self.voice, r);
            }
        }
        self.voice_path = self.wave.path.clone();
        if self.voice == 0 {
            self.error = Some("the audio engine refused to play".into());
        }
        self.dirty = true;
    }

    pub fn toggle_play(&mut self, h: &mut dyn Host) {
        if self.voice != 0 {
            self.stop(h);
        } else if self.wave.has() {
            self.play_wave(h, None);
        } else {
            self.select_file(h, true);
        }
    }

    // -----------------------------------------------------------------------
    // Le battement : chargements, tete de lecture, base MediaDB
    // -----------------------------------------------------------------------

    pub fn tick(&mut self, h: &mut dyn Host) {
        if let Some((path, t, play)) = self.pending.clone() {
            match h.poll(t) {
                Loaded::Pending => {}
                Loaded::Ready(clip) => {
                    self.pending = None;
                    let info = h.info(clip).unwrap_or_default();
                    let still = self
                        .selected_path()
                        .is_some_and(|p| p.to_string_lossy() == path);
                    self.remember(
                        h,
                        Cached {
                            path: path.clone(),
                            clip,
                            info,
                        },
                    );
                    if still {
                        self.wave.set_clip(clip, info, &path);
                        self.wave.layout(self.lay.w);
                        self.status.clear();
                        if play {
                            self.play_wave(h, None);
                        }
                    }
                    self.dirty = true;
                }
                Loaded::Failed(e) => {
                    self.pending = None;
                    self.error = Some(e);
                    self.status.clear();
                    self.wave.clear();
                    self.dirty = true;
                }
            }
        }
        if self.voice != 0 {
            // Une voix vit tant que son handle est valide : elle peut etre
            // encore « au repos » juste apres le lancement (le fil audio ne l'a
            // pas encore prise), et le moteur rend le handle quand elle a fini.
            match h.voice(self.voice) {
                Some((pos, st)) => {
                    if st != 0 && self.voice_path == self.wave.path {
                        let ph = Some(pos);
                        if ph != self.wave.playhead {
                            self.wave.playhead = ph;
                            self.dirty = true;
                        }
                    }
                }
                None => {
                    self.voice = 0;
                    self.wave.playhead = None;
                    self.dirty = true;
                }
            }
        }
        // Un fichier qui se decode encore : sa forme d'onde se complete.
        if self.wave.has() && !self.wave.info.complete() {
            if let Some(i) = h.info(self.wave.clip) {
                if i.decoded != self.wave.info.decoded {
                    self.wave.info = i;
                    if let Some(c) = self.cache.iter_mut().find(|c| c.clip == self.wave.clip) {
                        c.info = i;
                    }
                    self.dirty = true;
                }
            } else {
                self.wave.info.decoded = self.wave.info.frames;
            }
        }
        if self.db_job.as_ref().is_some_and(|j| j.is_finished()) {
            if let Some(j) = self.db_job.take() {
                self.db = Some(j.join().unwrap_or_default());
                self.searched.clear();
                self.run_search();
            }
        }
    }

    // -----------------------------------------------------------------------
    // La recherche
    // -----------------------------------------------------------------------

    fn start_db(&mut self) {
        if self.db.is_some() || self.db_job.is_some() {
            return;
        }
        let files: Vec<PathBuf> = self
            .tree
            .roots
            .iter()
            .filter(|&&r| self.tree.nodes[r].kind == Kind::Db)
            .map(|&r| self.tree.nodes[r].path.clone())
            .collect();
        let _ = &self.mediadb_dir;
        self.db_job = std::thread::Builder::new()
            .name("cp_mediadb".into())
            .spawn(move || {
                let mut all = Vec::new();
                for f in files {
                    if let Ok(b) = std::fs::read(&f) {
                        all.extend(model::parse_mediadb(&String::from_utf8_lossy(&b)));
                    }
                }
                all
            })
            .ok();
    }

    /// Tous les mots, sans casse, dans le chemin.
    fn matches(tokens: &[String], path: &str) -> bool {
        let l = path.to_lowercase();
        tokens.iter().all(|t| l.contains(t.as_str()))
    }

    pub fn run_search(&mut self) {
        let q = self.search.text.trim().to_string();
        if q == self.searched {
            return;
        }
        self.searched = q.clone();
        self.hits.clear();
        if !q.is_empty() {
            if !self.db_wanted {
                self.db_wanted = true;
                self.start_db();
            }
            let tokens: Vec<String> = q
                .to_lowercase()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let mut seen = std::collections::HashSet::new();
            let mut push = |hits: &mut Vec<Hit>, p: &Path, meta: &Meta| {
                let s = p.to_string_lossy();
                if hits.len() >= 5000 || !Self::matches(&tokens, &s) {
                    return;
                }
                if !seen.insert(s.to_lowercase()) {
                    return;
                }
                hits.push(Hit {
                    path: p.to_path_buf(),
                    name: model::display_name(p),
                    dir: p
                        .parent()
                        .map(|d| d.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    meta: meta.clone(),
                });
            };
            for n in &self.tree.nodes {
                if n.kind == Kind::File {
                    push(&mut self.hits, &n.path, &n.meta);
                }
            }
            if let Some(db) = &self.db {
                for e in db {
                    push(&mut self.hits, Path::new(&e.path), &e.meta);
                }
            }
            self.hits.sort_by_cached_key(|h| h.name.to_lowercase());
        }
        self.sel = if self.hits.is_empty() { None } else { Some(0) };
        self.scroll.pos = 0.0;
        self.rebuild();
        if self.searching() && !self.hits.is_empty() {
            self.sel = Some(0);
        }
    }

    pub fn db_loading(&self) -> bool {
        self.db_job.is_some()
    }

    // -----------------------------------------------------------------------
    // L'arbre
    // -----------------------------------------------------------------------

    fn toggle(&mut self, id: usize) {
        if !self.tree.is_container(id) {
            return;
        }
        if id == self.tree.fx_root && self.tree.nodes[id].children.is_none() {
            self.fx_wanted = true;
        }
        self.tree.load(id);
        let n = &mut self.tree.nodes[id];
        n.expanded = !n.expanded;
        if let Some(e) = &n.error {
            self.error = Some(e.clone());
        }
        self.rebuild();
    }

    fn set_expanded(&mut self, id: usize, on: bool) {
        if self.tree.is_container(id) && self.tree.nodes[id].expanded != on {
            self.toggle(id);
        }
    }

    pub fn is_favorite(&self, p: &Path) -> bool {
        self.favorites.iter().any(|f| f == p)
    }

    pub fn toggle_favorite(&mut self, h: &mut dyn Host) {
        let Some(p) = self.selected_path() else {
            return;
        };
        if let Some(i) = self.favorites.iter().position(|f| *f == p) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(p);
        }
        self.tree.set_favorites(&self.favorites);
        h.save("favorites", &self.favorites_text());
        self.rebuild();
    }

    pub fn favorites_text(&self) -> String {
        self.favorites
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("|")
    }

    // -----------------------------------------------------------------------
    // La selection
    // -----------------------------------------------------------------------

    fn select_row(&mut self, i: usize, h: &mut dyn Host, play: bool) {
        if self.rows.is_empty() {
            return;
        }
        let i = i.min(self.rows.len() - 1);
        self.sel = Some(i);
        let rh = self.lay.row_h;
        let sticky = self.sticky().len() as f32 * rh;
        let y = i as f32 * rh;
        if y < self.scroll.pos + sticky {
            self.scroll.pos = (y - sticky).max(0.0);
        }
        self.scroll.reveal(y, rh);
        if self.row_is_file(self.rows[i]) {
            self.select_file(h, play);
        }
        self.dirty = true;
    }

    fn move_sel(&mut self, d: i32, h: &mut dyn Host) {
        let n = self.rows.len() as i32;
        if n == 0 {
            return;
        }
        let cur = self.sel.map_or(-1, |s| s as i32);
        let to = (cur + d).clamp(0, n - 1) as usize;
        self.stop(h);
        self.select_row(to, h, true);
    }

    fn insert_selected(&mut self, h: &mut dyn Host) {
        let Some(p) = self.selected_path() else {
            return;
        };
        let sec = if self.wave.path == p.to_string_lossy() {
            self.wave.sel_seconds()
        } else {
            None
        };
        let rate = if self.wave.path == p.to_string_lossy() {
            self.sync_rate(h)
        } else {
            None
        };
        match h.insert(&p.to_string_lossy(), sec) {
            Ok(()) => {
                if let Some(r) = rate {
                    h.insert_rate(r);
                }
                self.status = format!("Inserted {}", model::display_name(&p));
            }
            Err(e) => self.error = Some(e),
        }
        self.dirty = true;
    }

    // -----------------------------------------------------------------------
    // Le clavier
    // -----------------------------------------------------------------------

    /// Que faire de cette touche (vue par le crochet, avant REAPER) ?
    pub fn key_use(&self, k: u32, m: Mods) -> KeyUse {
        if self.focus == Focus::Search {
            return match k {
                vk::ESCAPE | vk::RETURN | vk::UP | vk::DOWN | vk::TAB | vk::PRIOR | vk::NEXT => {
                    KeyUse::Eat
                }
                0x43 | 0x56 | 0x58 | 0x41 if m.ctrl => KeyUse::Eat, // C V X A
                _ => KeyUse::Text,
            };
        }
        match k {
            vk::UP
            | vk::DOWN
            | vk::LEFT
            | vk::RIGHT
            | vk::RETURN
            | vk::SPACE
            | vk::ESCAPE
            | vk::HOME
            | vk::END
            | vk::PRIOR
            | vk::NEXT
            | vk::BACK
            | vk::F5 => KeyUse::Eat,
            0x46 if m.ctrl => KeyUse::Eat,                   // Ctrl+F
            0x43 if m.ctrl => KeyUse::Eat,                   // Ctrl+C : le chemin
            0x46 | 0x4C if !m.ctrl && !m.alt => KeyUse::Eat, // F favori, L boucle
            0x31..=0x37 if !m.ctrl && !m.alt => KeyUse::Eat, // 1-7 collections
            0x52 | 0x51 | 0x54 if !m.ctrl && !m.alt => KeyUse::Eat, // R hasard, Q echange, T tempo
            _ => KeyUse::NotMine,
        }
    }

    fn key(&mut self, h: &mut dyn Host, k: u32, m: Mods) -> bool {
        if self.focus == Focus::Search {
            match k {
                vk::ESCAPE => {
                    if self.search.text.is_empty() {
                        self.focus = Focus::List;
                    } else {
                        self.search.clear();
                        self.run_search();
                    }
                }
                vk::RETURN | vk::DOWN | vk::TAB => {
                    self.focus = Focus::List;
                    if !self.rows.is_empty() {
                        self.stop(h);
                        self.select_row(self.sel.unwrap_or(0), h, true);
                    }
                }
                0x43 if m.ctrl => h.set_clipboard(self.search.selected()),
                0x58 if m.ctrl => {
                    h.set_clipboard(self.search.selected());
                    self.search.insert("");
                    self.run_search();
                }
                0x56 if m.ctrl => {
                    if let Some(s) = h.clipboard() {
                        self.search.insert(&s);
                        self.run_search();
                    }
                }
                _ => {
                    if self.search.key(k, m) == Edit::Changed {
                        self.run_search();
                    }
                }
            }
            self.dirty = true;
            return true;
        }
        let page = ((self.lay.list_h / self.lay.row_h.max(1.0)).floor() as i32 - 1).max(1);
        match k {
            vk::UP => self.move_sel(-1, h),
            vk::DOWN => self.move_sel(1, h),
            vk::PRIOR => self.move_sel(-page, h),
            vk::NEXT => self.move_sel(page, h),
            vk::HOME => self.move_sel(-(self.rows.len() as i32), h),
            vk::END => self.move_sel(self.rows.len() as i32, h),
            vk::RIGHT => {
                if let Some(Row::Node(id)) = self.sel.and_then(|i| self.rows.get(i).copied()) {
                    if self.tree.is_container(id) {
                        if self.tree.nodes[id].expanded {
                            self.move_sel(1, h);
                        } else {
                            self.set_expanded(id, true);
                        }
                    }
                }
            }
            vk::LEFT | vk::BACK => {
                if let Some(Row::Node(id)) = self.sel.and_then(|i| self.rows.get(i).copied()) {
                    if self.tree.is_container(id) && self.tree.nodes[id].expanded {
                        self.set_expanded(id, false);
                    } else if let Some(p) = self.tree.nodes[id].parent {
                        if let Some(i) = self.rows.iter().position(|&r| r == Row::Node(p)) {
                            self.select_row(i, h, false);
                            if k == vk::BACK {
                                self.set_expanded(p, false);
                            }
                        }
                    }
                }
            }
            vk::RETURN if self.swap.is_some() => {
                self.swap = None;
                self.status = "Hot swap kept".into();
            }
            vk::ESCAPE if self.swap.is_some() => self.swap_cancel(h),
            vk::RETURN if self.add_selected_fx(h) => {}
            vk::RETURN => {
                if let Some(r) = self.sel.and_then(|i| self.rows.get(i).copied()) {
                    match r {
                        Row::Node(id) if self.tree.is_container(id) => self.toggle(id),
                        _ => self.insert_selected(h),
                    }
                }
            }
            vk::SPACE => self.toggle_play(h),
            vk::ESCAPE => self.stop(h),
            vk::F5 => self.refresh(),
            0x46 if m.ctrl => {
                self.focus = Focus::Search;
                self.search.select_all();
            }
            0x43 if m.ctrl => {
                if let Some(p) = self.selected_path() {
                    h.set_clipboard(&p.to_string_lossy());
                    self.status = "Path copied".into();
                }
            }
            0x46 => self.toggle_favorite(h),
            0x31..=0x37 => self.toggle_collection((k - 0x30) as u8),
            0x52 => self.random_pick(h, m.shift),
            0x51 => self.swap_toggle(h),
            0x54 => {
                self.sync = !self.sync;
                h.save("sync", if self.sync { "1" } else { "0" });
                if self.voice != 0 {
                    let r = self.sync_rate(h).unwrap_or(1.0);
                    h.set_rate(self.voice, r);
                }
            }
            0x4C => {
                self.looping = !self.looping;
                h.save("loop", if self.looping { "1" } else { "0" });
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// Relit les dossiers ouverts (F5).
    pub fn refresh(&mut self) {
        let expanded: Vec<PathBuf> = self
            .tree
            .nodes
            .iter()
            .filter(|n| n.expanded && !n.path.as_os_str().is_empty())
            .map(|n| n.path.clone())
            .collect();
        for n in &mut self.tree.nodes {
            if matches!(n.kind, Kind::Dir | Kind::Drive | Kind::Db) {
                n.children = None;
                n.expanded = false;
                n.error = None;
            }
        }
        for p in expanded {
            self.expand_path(&p);
        }
        self.rebuild();
    }

    /// Rouvre un dossier par son chemin (sans changer la selection).
    fn expand_path(&mut self, p: &Path) {
        let ids: Vec<usize> = self.tree.roots.clone();
        for r in ids {
            if self.tree.nodes[r].path == p {
                self.tree.load(r);
                self.tree.nodes[r].expanded = true;
                return;
            }
        }
        if let Some(id) = self.tree.reveal(p) {
            self.tree.load(id);
            self.tree.nodes[id].expanded = true;
        }
    }

    // -----------------------------------------------------------------------
    // La souris
    // -----------------------------------------------------------------------

    /// Sur la couture entre la liste et la forme d'onde (a 4 px pres).
    pub fn over_split(&self, y: f32) -> bool {
        (y - self.lay.wave_y).abs() <= 4.0 * self.scale
    }

    fn in_wave(&self, y: f32) -> bool {
        y >= self.lay.wave_y && y < self.lay.status_y
    }

    fn on_scrollbar(&self, x: f32, y: f32) -> bool {
        self.scroll.needed()
            && x >= self.lay.w - self.lay.scroll_w - 2.0
            && y >= self.lay.list_y
            && y < self.lay.list_y + self.lay.list_h
    }

    fn in_search(&self, x: f32, y: f32) -> bool {
        y < self.lay.bar_h && x >= self.lay.search_x && x < self.lay.search_x + self.lay.search_w
    }

    fn btn_hit(&self, x: f32, y: f32, bx: f32) -> bool {
        y < self.lay.bar_h && x >= bx && x < bx + self.lay.btn
    }

    fn set_volume_x(&mut self, x: f32, h: &mut dyn Host) {
        self.volume = ((x - self.lay.vol_x) / self.lay.vol_w).clamp(0.0, 1.0);
        if self.voice != 0 {
            h.set_gain(self.voice, self.gain());
        }
        self.dirty = true;
    }

    /// Un evenement. Rend vrai si la vue doit etre redessinee.
    pub fn event(&mut self, h: &mut dyn Host, ev: &Ev) -> bool {
        let r = self.event_inner(h, ev);
        if self.fx_wanted {
            self.fx_wanted = false;
            self.ensure_fx(h);
            self.rebuild();
            return true;
        }
        r
    }

    fn event_inner(&mut self, h: &mut dyn Host, ev: &Ev) -> bool {
        match *ev {
            Ev::Key { vk: k, mods, .. } => return self.key(h, k, mods),
            Ev::Char(c) => {
                if self.focus == Focus::Search && self.search.char(c) == Edit::Changed {
                    self.run_search();
                    self.dirty = true;
                }
            }
            Ev::Wheel { x, y, dx, dy, mods } => {
                if self.in_wave(y) && self.wave.has() {
                    if mods.shift || dx != 0.0 {
                        let d = if dx != 0.0 { dx } else { -dy };
                        self.wave.pan_px(d * 60.0 * self.scale);
                    } else {
                        self.wave.zoom(x, dy);
                    }
                } else {
                    self.scroll.pos -= dy * 3.0 * self.lay.row_h;
                    self.scroll.clamp();
                    self.hover = self.row_at(y);
                }
                self.dirty = true;
            }
            Ev::Move { x, y, .. } => {
                match self.grab {
                    Grab::Scrollbar => {
                        let track = self.lay.list_h;
                        self.scroll
                            .drag(y - self.lay.list_y, track, 20.0 * self.scale);
                    }
                    Grab::Volume => self.set_volume_x(x, h),
                    Grab::Wave => self.wave.drag_select(x),
                    Grab::Split => {
                        let bottom = self.lay.status_y;
                        self.wave_h_pref = ((bottom - y) / self.scale).clamp(40.0, 2000.0);
                        let (w, hh, sc) = (self.lay.w, self.lay.h, self.scale);
                        self.layout(w, hh, sc);
                    }
                    Grab::WavePan => {
                        if let Some((x0, s0)) = self.wave.pan {
                            self.wave.start = s0;
                            self.wave.pan_px(x0 - x);
                        }
                    }
                    Grab::None => {
                        if let Some(p) = &mut self.press {
                            if !p.dragging
                                && ((x - p.x).abs() > 6.0 * self.scale
                                    || (y - p.y).abs() > 6.0 * self.scale)
                            {
                                p.dragging = true;
                                let row = p.row;
                                self.press = None;
                                self.start_drag_out(h, row);
                            }
                        }
                        let hv = self.row_at(y);
                        if hv != self.hover {
                            self.hover = hv;
                            self.dirty = true;
                        }
                        return self.take_dirty();
                    }
                }
                self.dirty = true;
            }
            Ev::Leave => {
                if self.hover.is_some() {
                    self.hover = None;
                    self.dirty = true;
                }
            }
            Ev::Down { x, y, b, dbl, mods } => self.down(h, x, y, b, dbl, mods),
            Ev::Up { x, y, b, .. } => self.up(h, x, y, b),
        }
        self.take_dirty()
    }

    fn down(&mut self, h: &mut dyn Host, x: f32, y: f32, b: Button, dbl: bool, mods: Mods) {
        self.dirty = true;
        if self.in_search(x, y) && b == Button::Left {
            self.focus = Focus::Search;
            return;
        }
        if y < self.lay.bar_h {
            self.focus = Focus::List;
            if b == Button::Right {
                self.window_menu(h, x, y);
                return;
            }
            if b != Button::Left {
                return;
            }
            if self.btn_hit(x, y, self.lay.sync_x) {
                self.sync = !self.sync;
                h.save("sync", if self.sync { "1" } else { "0" });
                if self.voice != 0 {
                    let r = self.sync_rate(h).unwrap_or(1.0);
                    h.set_rate(self.voice, r);
                }
            } else if self.btn_hit(x, y, self.lay.loop_x) {
                self.looping = !self.looping;
                h.save("loop", if self.looping { "1" } else { "0" });
            } else if self.btn_hit(x, y, self.lay.play_x) {
                self.toggle_play(h);
            } else if self.btn_hit(x, y, self.lay.star_x) {
                self.toggle_favorite(h);
            } else if x >= self.lay.vol_x - 4.0 && x < self.lay.vol_x + self.lay.vol_w + 4.0 {
                self.grab = Grab::Volume;
                self.set_volume_x(x, h);
            }
            return;
        }
        self.focus = Focus::List;
        if b == Button::Left && self.over_split(y) {
            self.grab = Grab::Split;
            return;
        }
        if self.in_wave(y) {
            if !self.wave.has() {
                return;
            }
            match b {
                Button::Left if dbl => {
                    self.wave.sel = None;
                    self.wave.fit();
                }
                Button::Left => {
                    self.grab = Grab::Wave;
                    self.wave.begin_select(x);
                }
                Button::Middle => {
                    self.grab = Grab::WavePan;
                    self.wave.pan = Some((x, self.wave.start));
                }
                Button::Right => {
                    self.wave.sel = None;
                }
            }
            return;
        }
        if self.on_scrollbar(x, y) && b == Button::Left {
            let (t0, tl) = self.scroll.thumb(self.lay.list_h, 20.0 * self.scale);
            let ty = y - self.lay.list_y;
            if ty < t0 || ty > t0 + tl {
                // Page vers le clic.
                let dir = if ty < t0 { -1.0 } else { 1.0 };
                self.scroll.pos += dir * self.lay.list_h;
                self.scroll.clamp();
            }
            self.scroll.grab = Some((ty, self.scroll.pos));
            self.grab = Grab::Scrollbar;
            return;
        }
        let row = self.row_at(y);
        match b {
            Button::Left => {
                if let Some(i) = row {
                    let r = self.rows[i];
                    if let Row::Node(id) = r {
                        // Le chevron (ou un double-clic) ouvre et ferme.
                        let d = self.tree.nodes[id].depth as f32;
                        let chev_x1 = (8.0 + d * 14.0 + 16.0) * self.scale;
                        if self.tree.is_container(id) && (x < chev_x1 || dbl) {
                            self.sel = Some(i);
                            self.toggle(id);
                            return;
                        }
                    }
                    if dbl {
                        if let Row::Node(id) = r {
                            if self.tree.nodes[id].kind == Kind::Fx {
                                self.sel = Some(i);
                                self.add_selected_fx(h);
                                return;
                            }
                        }
                    }
                    if dbl && self.row_is_file(r) {
                        self.insert_selected(h);
                        return;
                    }
                    self.sel = Some(i);
                    if self.row_is_file(r) {
                        // On montre tout de suite, on joue au relachement.
                        self.stop(h);
                        self.select_file(h, false);
                    }
                    let _ = mods;
                    self.press = Some(Press {
                        x,
                        y,
                        row: Some(i),
                        dragging: false,
                    });
                }
            }
            Button::Right => {
                if let Some(i) = row {
                    self.sel = Some(i);
                    self.context_menu(h, x, y);
                } else {
                    self.window_menu(h, x, y);
                }
            }
            Button::Middle => {}
        }
    }

    fn window_menu(&mut self, h: &mut dyn Host, x: f32, y: f32) {
        let items = h.window_items();
        if items.is_empty() {
            return;
        }
        let id = h.menu(x, y, &items);
        if id >= 1000 {
            h.window_action(id);
        }
    }

    fn up(&mut self, h: &mut dyn Host, x: f32, y: f32, b: Button) {
        let _ = y;
        self.dirty = true;
        match std::mem::replace(&mut self.grab, Grab::None) {
            Grab::Scrollbar => {
                self.scroll.grab = None;
                return;
            }
            Grab::Volume => {
                h.save("volume", &format!("{:.4}", self.volume));
                return;
            }
            Grab::WavePan => {
                self.wave.pan = None;
                return;
            }
            Grab::Split => {
                h.save("wave_h", &format!("{:.0}", self.wave_h_pref));
                return;
            }
            Grab::Wave => {
                let click = self.wave.end_select();
                if b == Button::Left {
                    if click {
                        let f = self.wave.snap(x);
                        self.play_wave(h, Some(f));
                    } else {
                        self.play_wave(h, None);
                    }
                }
                return;
            }
            Grab::None => {}
        }
        if let Some(p) = self.press.take() {
            if !p.dragging && b == Button::Left {
                if let Some(i) = p.row {
                    let r = self.rows.get(i).copied();
                    match r {
                        Some(r) if self.row_is_file(r) => self.play_wave_or_load(h),
                        Some(Row::Node(id)) if self.tree.is_container(id) => self.toggle(id),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Le relachement d'un clic sur un fichier : on joue (des que charge).
    fn play_wave_or_load(&mut self, h: &mut dyn Host) {
        let want = self
            .selected_path()
            .map(|p| p.to_string_lossy().into_owned());
        if want.is_some() && want.as_deref() == Some(self.wave.path.as_str()) && self.wave.has() {
            self.play_wave(h, None);
        } else if let Some(p) = &mut self.pending {
            p.2 = true;
        } else {
            self.select_file(h, true);
        }
    }

    fn start_drag_out(&mut self, h: &mut dyn Host, row: Option<usize>) {
        let Some(i) = row else { return };
        let Some(r) = self.rows.get(i).copied() else {
            return;
        };
        if !self.row_is_file(r) {
            return;
        }
        let Some(p) = self.row_path(r).map(|p| p.to_string_lossy().into_owned()) else {
            return;
        };
        let sec = if self.wave.path == p {
            self.wave.sel_seconds()
        } else {
            None
        };
        self.stop(h);
        if !h.drag_out(&p, sec) {
            self.error = Some("this file cannot be dragged (press Enter to insert it)".into());
        }
        self.dirty = true;
    }

    fn context_menu(&mut self, h: &mut dyn Host, x: f32, y: f32) {
        let Some(r) = self.sel.and_then(|i| self.rows.get(i).copied()) else {
            return;
        };
        let path = self.row_path(r).map(Path::to_path_buf);
        let fav = path.as_ref().is_some_and(|p| self.is_favorite(p));
        let mut items = Vec::new();
        if self.row_is_file(r) {
            items.push(MenuEntry::new(1, "Insert into project\tEnter", false));
            items.push(MenuEntry::new(2, "Play\tSpace", false));
            items.push(MenuEntry::sep());
        }
        if path.is_some() {
            items.push(MenuEntry::new(3, "Favorite\tF", fav));
            items.push(MenuEntry::new(4, "Copy path\tCtrl+C", false));
        }
        items.push(MenuEntry::sep());
        items.push(MenuEntry::new(5, "Refresh\tF5", false));
        let win = h.window_items();
        if !win.is_empty() {
            items.push(MenuEntry::sep());
            items.extend(win);
        }
        match h.menu(x, y, &items) {
            id if id >= 1000 => h.window_action(id),
            1 => self.insert_selected(h),
            2 => {
                self.stop(h);
                self.select_file(h, true);
            }
            3 => self.toggle_favorite(h),
            4 => {
                if let Some(p) = path {
                    h.set_clipboard(&p.to_string_lossy());
                }
            }
            5 => self.refresh(),
            _ => {}
        }
        self.dirty = true;
    }

    // -----------------------------------------------------------------------
    // L'etat entre deux generations du module
    // -----------------------------------------------------------------------

    /// Ce qu'il faut pour retrouver la vue apres un rechargement.
    pub fn save_state(&self) -> String {
        let mut s = String::new();
        for n in &self.tree.nodes {
            if n.expanded && !n.path.as_os_str().is_empty() {
                s += &format!("open\t{}\n", n.path.to_string_lossy());
            }
        }
        if self.tree.nodes[self.tree.favorites].expanded {
            s += "open_favorites\t1\n";
        }
        if let Some(p) = self.selected_path() {
            s += &format!("sel\t{}\n", p.to_string_lossy());
        }
        s += &format!("scroll\t{}\n", self.scroll.pos);
        s += &format!("search\t{}\n", self.search.text);
        s
    }

    pub fn restore_state(&mut self, s: &str) {
        let mut sel = None;
        let mut scroll = 0.0;
        for line in s.lines() {
            let Some((k, v)) = line.split_once('\t') else {
                continue;
            };
            match k {
                "open" => self.expand_path(Path::new(v)),
                "open_favorites" => {
                    let f = self.tree.favorites;
                    self.tree.nodes[f].expanded = true;
                }
                "sel" => sel = Some(PathBuf::from(v)),
                "scroll" => scroll = v.parse().unwrap_or(0.0),
                "search" => self.search.set(v),
                _ => {}
            }
        }
        if self.searching() {
            self.run_search();
        }
        self.rebuild();
        if let Some(p) = sel {
            self.sel = self
                .rows
                .iter()
                .position(|&r| self.row_path(r) == Some(p.as_path()));
        }
        self.scroll.pos = scroll;
        self.scroll.clamp();
    }

    /// Lache tout ce qui est tenu dans le moteur (avant un rechargement ou a la
    /// fermeture).
    pub fn release_all(&mut self, h: &mut dyn Host) {
        self.stop(h);
        for c in self.cache.drain(..) {
            h.release(c.clip);
        }
        self.wave.clear();
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Le fil de la base finit seul (lecture de fichiers) : on l'attend
        // pour qu'aucun fil ne survive au module.
        if let Some(j) = self.db_job.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests;
