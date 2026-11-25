//! Le navigateur d'effets dans REAPER : sa fenetre (arrangement, docker ou
//! flottante), le catalogue, la chaine de la piste choisie, et ce qui
//! remplace la liste de REAPER.
//!
//! - Le catalogue se construit a la premiere ouverture (la liste de REAPER et
//!   ses fichiers : dossiers, categories, developpeurs).
//! - Ce que CP Studio garde (favoris ajoutes ou retires, recents, compteurs,
//!   onglets, reglages) vit dans `<ressources>/CP_Studio/fx_browser.txt`.
//! - Remplacer celui de REAPER (un reglage, oui par defaut) : « View: Show FX
//!   browser window » et « Insert virtual instrument on new track » passent
//!   par ici, et le bouton FX d'une piste sans effet ouvre ce navigateur.
//!   Il faut le chargeur de l'extension 2.
//!
//! Le modele est `cp_fxlist`, la vue `cp_fxview` ; ici, le lien a REAPER.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cp_abi::{
    ACTION_ADD_FX_TO, ACTION_FX_BUTTON, ADD_FX_MAX_TRACK, PLACE_ARRANGE, PLACE_HELD, VIEW_DOCK,
    VIEW_FLOAT, VIEW_PLACED, WM_CP_ACCEL, WM_CP_FRAME,
};
use cp_fxlist::{Catalog, IniTexts, Kind, PostInsert, PostParams, Rule, Src, Store, Tab, dice};
use cp_fxview::{Act, Chain, ChainFx, Hit, KeyUse, View};
use cp_gfx::{IRect, rgb};
use cp_sys::module::key_event;
use cp_sys::reaper::Reaper;
use cp_sys::ui::{self, MenuEntry, MenuItem};
use cp_ui::pop::Pop;

use crate::ask::Ask;
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_ui::{Button, Ev, Theme};

use crate::gtab::{LOOK, selftest};
use crate::{App, Place, SECTION, WinOp, hi16, lo16, mods, msg};

const TITLE: &str = "FX Browser";
const IDENT: &str = "cp_studio_fxbrowser";
/// « View: Show FX browser window ».
pub(crate) const REAPER_FX_BROWSER: i32 = 40271;
/// « Insert virtual instrument on new track... ».
pub(crate) const REAPER_INSERT_INSTRUMENT: i32 = 40701;

pub(crate) struct FWin {
    pub hwnd: Hwnd,
    pub place: Place,
    frame: crate::frame::Frame,
    theme: Theme,
    theme_key: Vec<Option<(u8, u8, u8)>>,
    tracking: bool,
    closing: bool,
    tip: Option<ui::Tooltip>,
    hover: (Hit, Instant),
}

pub(crate) struct FPart {
    /// Les questions (menus, noms) en popover : le geste a rejouer.
    pub ask: crate::ask::Asker<Act>,
    pub cat: Catalog,
    pub st: Store,
    pub view: View,
    pub win: Option<FWin>,
    pub place: Place,
    chain: Chain,
    /// La vraie place de chaque effet montre, et le nombre d'effets de la
    /// piste : les effets du systeme (FX Constellation, ses sources...) ne
    /// se montrent pas, chaque geste traduit sa rangee.
    chain_real: Vec<usize>,
    chain_total: usize,
    /// Le GUID de chaque effet montre (detacher, rattacher).
    chain_guids: Vec<String>,
    /// La piste montree (son GUID).
    track: Option<String>,
    /// Le prochain ajout cree une piste ; la piste choisie quand on l'a
    /// demande (une autre piste choisie l'annule).
    new_track: Option<Option<String>>,
    loaded: bool,
    file: Option<PathBuf>,
    dirty_at: Option<Instant>,
    flash: Option<(String, Instant)>,
    ticks: u32,
    /// Ce que le chargeur a pris pour nous (aucun : pas encore applique).
    stolen: Option<bool>,
    /// Les actions de REAPER prises depuis le chargement (l'auto-test).
    taken: u32,
    /// Les boites « Add FX to » de REAPER remplacees (l'auto-test).
    adds: u32,
}

impl FPart {
    pub fn new(r: &Reaper) -> FPart {
        let place = Place::from_key(&r.ext_get(SECTION, "fxb_place").unwrap_or_default())
            .unwrap_or(Place::Dock);
        let file = r
            .resource_path()
            .map(|p| p.join("CP_Studio").join("fx_browser.txt"));
        let st = file
            .as_deref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| Store::decode(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        FPart {
            cat: Catalog::default(),
            st,
            view: View::new(),
            win: None,
            place,
            chain: Chain::default(),
            chain_real: Vec::new(),
            chain_total: 0,
            chain_guids: Vec::new(),
            track: None,
            new_track: None,
            loaded: false,
            file,
            dirty_at: None,
            flash: None,
            ticks: 0,
            stolen: None,
            taken: 0,
            adds: 0,
            ask: Default::default(),
        }
    }
}

fn read_text(p: &Path) -> String {
    std::fs::read(p)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// Les chaines d'effets enregistrees : leurs chemins sous `FXChains`.
fn chains_in(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let p = e.path();
            if p.is_dir() {
                if out.len() < 5000 && rel.matches('/').count() < 6 {
                    stack.push((p, r));
                }
            } else if name.to_lowercase().ends_with(".rfxchain") {
                out.push(r);
            }
        }
    }
    out.sort_by_key(|s| s.to_lowercase());
    out
}

fn item(id: u32, text: &str, checked: bool, enabled: bool) -> MenuItem<'_> {
    MenuItem {
        id,
        text,
        checked,
        enabled,
    }
}

/// Les effets montres d'une chaine (jamais ceux du systeme : FX
/// Constellation, ses sources, le synthe de Guitar Tab, le kit) et la vraie
/// place de chacun. Toute liste d'effets a nous passe par la.
pub(crate) fn shown_fx(fx: &[(String, bool)]) -> (Vec<(String, bool)>, Vec<usize>) {
    fx.iter()
        .enumerate()
        .filter(|(_, f)| !cp_fxlist::is_internal(&f.0))
        .map(|(i, f)| (f.clone(), i))
        .unzip()
}

/// Un effet du coeur de CP Studio (FX Constellation, ses sources, le kit,
/// le synthe de Guitar Tab) : le de ne le tire pas et ne le retire pas.
pub(crate) fn is_core(name: &str) -> bool {
    use cp_fxc::slots::{FxKind, fx_kind};
    cp_fxlist::is_internal(name) || fx_kind(name) != FxKind::User
}

/// Le sous-menu de la post-insertion : ce que FX Constellation fait d'un
/// effet ajoute.
fn post_menu(p: &PostInsert) -> MenuEntry {
    let random = match p.params {
        PostParams::Random(n) => format!("At random ({n})"),
        _ => "At random".to_string(),
    };
    let picks: Vec<MenuEntry> = (1..=16u8)
        .map(|n| {
            MenuEntry::check(
                710 + n as u32,
                n.to_string(),
                p.params == PostParams::Random(n),
            )
        })
        .collect();
    MenuEntry::Sub(
        "After insert (FX Constellation)".into(),
        vec![
            MenuEntry::off("Track parameters"),
            MenuEntry::check(700, "None", p.params == PostParams::None),
            MenuEntry::check(701, "All", p.params == PostParams::All),
            MenuEntry::check(702, "All continuous", p.params == PostParams::Continuous),
            MenuEntry::Sub(random, picks),
            MenuEntry::Sep,
            MenuEntry::check(730, "Random X/Y axis", p.xy),
            MenuEntry::check(731, "Random range", p.range),
            MenuEntry::check(732, "Random base", p.base),
            MenuEntry::check(733, "Random invert", p.invert),
            MenuEntry::Sep,
            MenuEntry::check(734, "Insert bypassed", p.bypass),
        ],
    )
}

fn post_pick(p: &mut PostInsert, id: u32) {
    match id {
        700 => p.params = PostParams::None,
        701 => p.params = PostParams::All,
        702 => p.params = PostParams::Continuous,
        711..=726 => p.params = PostParams::Random((id - 710) as u8),
        730 => p.xy = !p.xy,
        731 => p.range = !p.range,
        732 => p.base = !p.base,
        733 => p.invert = !p.invert,
        734 => p.bypass = !p.bypass,
        _ => {}
    }
}

impl App {
    fn fxb_keep(&self, key: &str, value: &str) {
        if !selftest() {
            self.r.ext_set(SECTION, key, value, true);
        }
    }

    fn fxb_say(&mut self, s: impl Into<String>) {
        self.fxb.flash = Some((s.into(), Instant::now()));
        self.fxb.view.mark();
        self.fxb_kick();
    }

    // -----------------------------------------------------------------------
    // Le catalogue et ce qu'on garde
    // -----------------------------------------------------------------------

    /// Construit le catalogue (une fois ; « Reload » le refait).
    pub(crate) fn fxb_load(&mut self) {
        if self.fxb.loaded {
            return;
        }
        let t0 = Instant::now();
        let installed = self.r.fx_installed();
        let res = self.r.resource_path().unwrap_or_default();
        let ini = IniTexts {
            vst: [
                "reaper-vstplugins64.ini",
                "reaper-vst3plugins64.ini",
                "reaper-vstplugins.ini",
            ]
            .iter()
            .map(|f| read_text(&res.join(f)))
            .collect(),
            clap: ["reaper-clap-win64.ini", "reaper-clap-win32.ini"]
                .iter()
                .map(|f| read_text(&res.join(f)))
                .collect(),
            jsfx: read_text(&res.join("reaper-jsfx.ini")),
            folders: read_text(&res.join("reaper-fxfolders.ini")),
            tags: read_text(&res.join("reaper-fxtags.ini")),
        };
        let chains = chains_in(&res.join("FXChains"));
        self.fxb.cat = Catalog::build(&installed, &ini, &chains);
        self.fxb.loaded = true;
        let src = Src::from_key(&self.fxb.st.set.src).unwrap_or_default();
        self.fxb.view.src = src;
        self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
        let c = &self.fxb.cat;
        self.host.info(&format!(
            "navigateur d'effets : {} effets ({} instruments), {} chaines, {} dossiers, {} categories, {} developpeurs, {} favoris de REAPER, en {:.0} ms",
            c.plugins.len() - chains.len(),
            c.plugins.iter().filter(|p| p.instrument).count(),
            chains.len(),
            c.folders.len(),
            c.cats.len(),
            c.devs.len(),
            c.reaper_favs.len(),
            t0.elapsed().as_secs_f64() * 1000.0
        ));
        if !c.missing.is_empty() {
            let first: Vec<&str> = c.missing.iter().take(12).map(String::as_str).collect();
            self.host.info(&format!(
                "navigateur d'effets : {} elements de dossiers introuvables, dont : {}",
                c.missing.len(),
                first.join(" ; ")
            ));
        }
    }

    fn fxb_reload(&mut self) {
        self.fxb.loaded = false;
        self.fxb_load();
        self.fxb.view.mark();
    }

    pub(crate) fn fxb_save_soon(&mut self) {
        self.fxb.dirty_at = Some(Instant::now());
    }

    fn fxb_save_now(&mut self) {
        self.fxb.dirty_at = None;
        if selftest() {
            return;
        }
        let Some(f) = self.fxb.file.clone() else {
            return;
        };
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&f, self.fxb.st.encode()) {
            self.host.warn(&format!(
                "navigateur d'effets : {} non ecrit ({e})",
                f.display()
            ));
        }
    }

    // -----------------------------------------------------------------------
    // La chaine de la piste choisie
    // -----------------------------------------------------------------------

    /// La vraie place d'une rangee montree de la chaine.
    fn fxb_real(&self, shown: usize) -> Option<usize> {
        self.fxb.chain_real.get(shown).copied()
    }

    /// La vraie place d'un emplacement montre (0..=n : avant cette rangee) ;
    /// apres la derniere rangee, la fin de la chaine.
    fn fxb_real_slot(&self, slot: usize) -> usize {
        self.fxb
            .chain_real
            .get(slot)
            .copied()
            .unwrap_or(self.fxb.chain_total)
    }

    fn fxb_sync_chain(&mut self) {
        let sel = self.r.fx_selected_track();
        if let Some(asked) = &self.fxb.new_track {
            if *asked != sel {
                self.fxb.new_track = None;
            }
        }
        let new_track = self.fxb.new_track.is_some();
        let mut ch = Chain {
            new_track,
            ..Chain::default()
        };
        let (mut real, mut total, mut guids) = (Vec::new(), 0, Vec::new());
        if !new_track {
            if let Some(t) = sel.as_deref().and_then(|g| self.r.fx_track(g)) {
                ch.track = Some(t.name);
                ch.color = t.color.map(|(r, g, b)| rgb(r, g, b));
                total = t.fx.len();
                let (shown, places) = shown_fx(&t.fx);
                // Hors de ROLL dans FX Constellation (DETACHED) : par GUID.
                let g = sel.as_deref().unwrap_or_default();
                let tk = self.r.xc_track(g);
                let detached = self.fxc_detached_of(g);
                guids = places
                    .iter()
                    .map(|&k| tk.and_then(|t| self.r.xc_fx_guid(t, k)).unwrap_or_default())
                    .collect();
                ch.fx = shown
                    .into_iter()
                    .zip(&guids)
                    .map(|((name, on), id)| ChainFx {
                        name,
                        on,
                        detached: !id.is_empty() && detached.contains(id),
                    })
                    .collect();
                real = places;
            }
        }
        self.fxb.chain_real = real;
        self.fxb.chain_total = total;
        self.fxb.chain_guids = guids;
        self.fxb.track = sel;
        if ch != self.fxb.chain {
            if ch.fx.len() != self.fxb.chain.fx.len() || ch.track != self.fxb.chain.track {
                self.fxb.view.fx_sel = None;
            }
            self.fxb.chain = ch;
            self.fxb.view.mark();
        }
    }

    /// La piste qui recoit : celle montree, ou une piste neuve (le mode
    /// « piste neuve », ou `force_new`), nommee d'apres le premier effet.
    fn fxb_target(&mut self, first: usize, force_new: bool) -> Option<String> {
        if self.fxb.new_track.is_some() || force_new {
            let p = self.fxb.cat.plugins.get(first)?;
            let (label, inst) = (p.label.clone(), p.instrument);
            let after = self.fxb.track.clone();
            let g = self.r.fx_new_track(&label, after.as_deref(), inst);
            self.fxb.new_track = None;
            return g;
        }
        self.fxb.track.clone()
    }

    /// Ajoute des effets a une piste (aucune : celle montree), a la place
    /// `at`. Un instrument sans place va en tete.
    fn fxb_add(
        &mut self,
        plugins: &[usize],
        at: Option<usize>,
        to: Option<String>,
        force_new: bool,
    ) {
        self.fxb_add_to(plugins, at, to, force_new, true);
    }

    /// [`App::fxb_add`] : `post` = la post-insertion de FX Constellation
    /// s'applique. Rend les GUID des effets poses (le glisser commun les
    /// detache s'ils tombent dans DETACHED).
    pub(crate) fn fxb_add_to(
        &mut self,
        plugins: &[usize],
        at: Option<usize>,
        to: Option<String>,
        force_new: bool,
        post: bool,
    ) -> Vec<String> {
        let Some(&first) = plugins.first() else {
            return Vec::new();
        };
        self.fxb_load();
        let r = self.r;
        r.undo_begin();
        let target = match to {
            Some(g) if !force_new => Some(g),
            _ => self.fxb_target(first, force_new),
        };
        let Some(g) = target else {
            r.undo_end("CP FX Browser: add FX", -1);
            self.fxb_say("Select a track first");
            return Vec::new();
        };
        let mut last = None;
        let mut failed = Vec::new();
        // Les effets poses (pour la post-insertion de FX Constellation).
        let tk = r.xc_track(&g);
        let mut added: Vec<String> = Vec::new();
        for (k, &i) in plugins.iter().enumerate() {
            let Some(p) = self.fxb.cat.plugins.get(i) else {
                continue;
            };
            let pos = match at {
                Some(a) => Some(a + k),
                None if p.instrument && p.kind != Kind::Chain => Some(k),
                None => None,
            };
            let name = p.name.clone();
            let chain = p.kind == Kind::Chain;
            match r.fx_add(&g, &name, pos) {
                Ok(idx) => {
                    last = Some(idx);
                    self.fxb.st.used(&name);
                    if let (false, Some(t)) = (chain, tk) {
                        if let Some(fg) = r.xc_fx_guid(t, idx.max(0) as usize) {
                            added.push(fg);
                        }
                    }
                }
                Err(_) => failed.push(p.label.clone()),
            }
        }
        let pi = self.fxb.st.set.post;
        if post && !added.is_empty() && (pi.params != PostParams::None || pi.bypass) {
            self.fxc_post_insert(&g, &added, &pi);
        }
        r.undo_end("CP FX Browser: add FX", -1);
        if let (Some(idx), true) = (last, self.fxb.st.set.auto_open) {
            r.fx_open(&g, idx as usize, false);
        }
        if !failed.is_empty() {
            self.fxb_say(format!("Could not add {}", failed.join(", ")));
        }
        self.fxb_save_soon();
        self.fxb_sync_chain();
        if matches!(self.fxb.view.src, Src::Recent | Src::Most) {
            self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
        }
        added
    }

    /// Des effets laches hors de la fenetre : sur une vue de CP Studio qui
    /// les prend (le glisser commun : FX Constellation), une colonne de la
    /// Session, une piste de l'arrangement ou de la console, ou sous les
    /// pistes (une piste neuve).
    fn fxb_drop_out(&mut self, plugins: Vec<usize>) {
        let (sx, sy) = ui::cursor_pos();
        if self.carry_drop(sx, sy) {
            return;
        }
        if let Some(g) = self.ses_track_at_screen(sx, sy) {
            self.fxb_add(&plugins, None, Some(g), false);
            return;
        }
        let (g, info) = self.r.thing_at(sx, sy);
        let on_tracks = info.starts_with("tcp") || info.starts_with("mcp") || info == "arrange";
        match g {
            Some(g) if on_tracks => {
                self.r.fx_select_track(&g);
                self.fxb_sync_chain();
                self.fxb_add(&plugins, None, Some(g), false);
            }
            None if on_tracks || info.starts_with("spacer") => {
                self.fxb_add(&plugins, None, None, true);
            }
            _ => {}
        }
    }

    fn fxb_dice(&mut self) {
        self.fxb_load();
        let n = self.fxb.st.set.dice_count as usize;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let picks = dice(&self.fxb.cat, &self.fxb.view.rows, n, seed);
        if picks.is_empty() {
            self.fxb_say("Nothing to draw in this list");
            return;
        }
        let only_inst = picks.iter().all(|&i| self.fxb.cat.plugins[i].instrument);
        let replace = self.fxb.st.set.dice_replace && !only_inst && self.fxb.new_track.is_none();
        if replace {
            if let Some(g) = self.fxb.track.clone() {
                let r = self.r;
                r.undo_begin();
                // Les instruments restent : le de ne tire que des effets. Le
                // coeur aussi (FX Constellation, ses sources, le kit).
                // La vraie chaine (la chaine montree cache le systeme). Un
                // effet hors de ROLL (DETACHED) reste aussi.
                let fx = r.fx_track(&g).map(|t| t.fx).unwrap_or_default();
                let detached = self.fxc_detached_of(&g);
                let tk = r.xc_track(&g);
                for (i, (name, _)) in fx.iter().enumerate().rev() {
                    let inst = self
                        .fxb
                        .cat
                        .find(name)
                        .is_some_and(|k| self.fxb.cat.plugins[k].instrument);
                    let det = tk
                        .and_then(|t| r.xc_fx_guid(t, i))
                        .is_some_and(|id| detached.contains(&id));
                    if !inst && !det && !is_core(name) {
                        r.fx_remove(&g, i);
                    }
                }
                r.undo_end("CP FX Browser: clear chain for dice", -1);
            }
        }
        self.fxb.view.clear_selection();
        self.fxb_add(&picks, None, None, false);
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn fxb_act(&mut self, a: Act) {
        if let Act::Pop(_, out) = a {
            // La reponse a une question : le geste qui l'a posee se rejoue.
            if out == cp_ui::pop::Out::Closed {
                self.fxb.ask.cancel();
            } else if let Some(g) = self.fxb.ask.answer(&out) {
                self.fxb_act(g);
            }
            self.fxb.view.mark();
            return;
        }
        let top = self.fxb.ask.begin(&a);
        self.fxb_act_one(a);
        self.fxb.ask.end(top);
    }

    fn fxb_act_one(&mut self, a: Act) {
        let r = self.r;
        match a {
            Act::Pop(..) => {}
            Act::Add { plugins, at } => {
                let at = at.map(|a| self.fxb_real_slot(a));
                self.fxb_add(&plugins, at, None, false)
            }
            Act::DropOut { plugins } => self.fxb_drop_out(plugins),
            Act::ToggleFav(i) => {
                self.fxb.st.toggle_fav(&self.fxb.cat, i);
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                self.fxb_save_soon();
            }
            Act::RowMenu { x, y } => self.fxb_row_menu(x, y),
            Act::ChipMenu { chip, x, y } => self.fxb_chip_menu(chip, x, y),
            Act::AddTabMenu { x, y } => self.fxb_add_tab_menu(x, y),
            Act::Menu { x, y } => self.fxb_menu(x, y),
            Act::DiceMenu { x, y } => self.fxb_dice_menu(x, y),
            Act::Dice => self.fxb_dice(),
            Act::ToTab { tab, plugins } => {
                let names: Vec<String> = plugins
                    .iter()
                    .filter_map(|&i| self.fxb.cat.plugins.get(i).map(|p| p.name.clone()))
                    .collect();
                let k = self.fxb.st.add_to_tab(&tab, &names);
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                self.fxb_save_soon();
                self.fxb_say(format!("+{k} {tab}"));
            }
            // Une rangee de la chaine montree -> sa vraie place (les effets
            // du systeme ne se montrent pas).
            Act::FxOpen(i) => {
                if let (Some(g), Some(k)) = (self.fxb.track.clone(), self.fxb_real(i)) {
                    r.fx_open(&g, k, false);
                }
            }
            Act::FxToggle(i) => {
                if let (Some(g), Some(f), Some(k)) = (
                    self.fxb.track.clone(),
                    self.fxb.chain.fx.get(i),
                    self.fxb_real(i),
                ) {
                    r.undo_begin();
                    r.fx_set_enabled(&g, k, !f.on);
                    r.undo_end("CP FX Browser: bypass FX", -1);
                }
                self.fxb_sync_chain();
            }
            Act::FxDelete(i) => {
                if let (Some(g), Some(k)) = (self.fxb.track.clone(), self.fxb_real(i)) {
                    r.undo_begin();
                    r.fx_remove(&g, k);
                    r.undo_end("CP FX Browser: delete FX", -1);
                }
                self.fxb_sync_chain();
            }
            Act::FxMove { from, to, copy } => {
                let mut at = None;
                if let (Some(g), Some(k)) = (self.fxb.track.clone(), self.fxb_real(from)) {
                    let slot = self.fxb_real_slot(to);
                    r.undo_begin();
                    at = r.fx_move(&g, k, slot, copy);
                    r.undo_end(
                        if copy {
                            "CP FX Browser: copy FX"
                        } else {
                            "CP FX Browser: move FX"
                        },
                        -1,
                    );
                }
                self.fxb_sync_chain();
                // La nouvelle place -> sa rangee montree.
                self.fxb.view.fx_sel =
                    at.and_then(|a| self.fxb.chain_real.iter().position(|&k| k == a));
            }
            Act::FxMenu { fx, x, y } => self.fxb_fx_menu(fx, x, y),
            Act::Paste => {
                let h = self.fxb.win.as_ref().map_or(Hwnd(0), |w| w.hwnd);
                if let Some(s) = ui::clipboard_text(h) {
                    self.fxb.view.paste(&self.fxb.cat, &self.fxb.st, &s);
                }
            }
            Act::Changed => self.fxb_save_soon(),
            Act::DockGrab => {
                if let Some(h) = self.fxb.win.as_ref().map(|w| w.hwnd) {
                    self.dock_grab(h);
                }
            }
            Act::DockSwitch(i) => {
                if let Some(h) = self.fxb.win.as_ref().map(|w| w.hwnd) {
                    self.dock_switch(h, i);
                }
            }
            Act::ViewMenu { x, y } => self.fxb_place_menu(x, y),
        }
        self.fxb.view.mark();
    }

    /// Un menu plat, en popover (voir [`App::fxb_tree`]).
    fn fxb_popup(&mut self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let e = ui::flat_to_tree(items);
        self.fxb_tree(x, y, &e)
    }

    fn fxb_row_menu(&mut self, x: f32, y: f32) {
        let sel = self.fxb.view.selection();
        if sel.is_empty() {
            return;
        }
        let all_fav = sel.iter().all(|&i| self.fxb.st.is_fav(&self.fxb.cat, i));
        let tabs: Vec<String> = self.fxb.st.tabs.iter().map(|t| t.name.clone()).collect();
        let in_tab = match &self.fxb.view.src {
            Src::Tab(n) => self.fxb.st.tab(n).is_some_and(|t| {
                sel.iter()
                    .any(|&i| t.items.contains(&self.fxb.cat.plugins[i].name))
            }),
            _ => false,
        };
        let has_track = self.fxb.track.is_some();
        let add_to: Vec<String> = tabs.iter().map(|t| format!("Add to tab: {t}")).collect();
        let mut items = vec![
            Some(item(1, "Add to the track", false, has_track)),
            Some(item(2, "Add on a new track", false, true)),
            None,
            Some(item(3, "Favorite", all_fav, true)),
            None,
        ];
        for (k, t) in add_to.iter().enumerate() {
            items.push(Some(item(100 + k as u32, t, false, true)));
        }
        items.push(Some(item(4, "New tab with the selection...", false, true)));
        if in_tab {
            items.push(Some(item(5, "Remove from this tab", false, true)));
        }
        let names: Vec<String> = sel
            .iter()
            .map(|&i| self.fxb.cat.plugins[i].name.clone())
            .collect();
        match self.fxb_popup(x, y, &items) {
            1 => self.fxb_add(&sel, None, None, false),
            2 => self.fxb_add(&sel, None, None, true),
            3 => {
                for &i in &sel {
                    if self.fxb.st.is_fav(&self.fxb.cat, i) == all_fav {
                        self.fxb.st.toggle_fav(&self.fxb.cat, i);
                    }
                }
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                self.fxb_save_soon();
            }
            4 => {
                if let Some(name) = self.fxb_ask_name("New tab", "") {
                    let name = self.fxb.st.free_tab_name(&name);
                    self.fxb.st.tabs.push(Tab {
                        name: name.clone(),
                        items: names,
                        rule: None,
                    });
                    self.fxb_show_tab(&name);
                }
            }
            5 => {
                if let Src::Tab(n) = self.fxb.view.src.clone() {
                    if let Some(t) = self.fxb.st.tab_mut(&n) {
                        t.items.retain(|x| !names.contains(x));
                    }
                    self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                    self.fxb_save_soon();
                }
            }
            k @ 100.. => {
                if let Some(t) = tabs.get((k - 100) as usize) {
                    let n = self.fxb.st.add_to_tab(t, &names);
                    self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                    self.fxb_save_soon();
                    self.fxb_say(format!("+{n} {t}"));
                }
            }
            _ => {}
        }
    }

    /// Un nom demande, en popover (meme rejeu que les menus). `None` :
    /// pas encore, annule ou vide.
    fn fxb_ask_name(&mut self, title: &str, cur: &str) -> Option<String> {
        match self.fxb.ask.text() {
            Ask::Ready(v) => {
                let v = v.trim().replace(['\t', '\n', '\r'], " ");
                (!v.is_empty()).then_some(v)
            }
            Ask::Open => {
                let (x, y) = self.fxb.view.mouse;
                self.fxb.view.open_pop(Pop::ask(0, title, cur, "OK", x, y));
                self.fxb_kick();
                None
            }
            Ask::Skip => None,
        }
    }

    fn fxb_show_tab(&mut self, name: &str) {
        let (cat, st) = (&self.fxb.cat, &mut self.fxb.st);
        self.fxb.view.refresh(cat, st);
        self.fxb
            .view
            .set_source(cat, st, Src::Tab(name.to_string()));
        self.fxb_save_soon();
    }

    fn fxb_chip_menu(&mut self, chip: usize, x: f32, y: f32) {
        let Some(Src::Tab(name)) = self.fxb.view.chips.get(chip).cloned() else {
            return;
        };
        let Some(tab) = self.fxb.st.tab(&name).cloned() else {
            return;
        };
        let ruled = tab.rule.is_some();
        let items = [
            Some(item(1, "Rename...", false, true)),
            Some(item(
                2,
                if ruled {
                    "Edit the search..."
                } else {
                    "Add a search..."
                },
                false,
                true,
            )),
            Some(item(4, "Remove the search", false, ruled)),
            None,
            Some(item(5, "Move left", false, chip > Src::CHIPS.len())),
            Some(item(
                6,
                "Move right",
                false,
                chip + 1 < self.fxb.view.chips.len(),
            )),
            None,
            Some(item(3, "Delete the tab", false, true)),
        ];
        let k = self.fxb_popup(x, y, &items);
        let pos = self.fxb.st.tabs.iter().position(|t| t.name == name);
        match k {
            1 => {
                if let Some(n) = self.fxb_ask_name("Rename tab", &name) {
                    if n != name {
                        let n = self.fxb.st.free_tab_name(&n);
                        if let Some(t) = self.fxb.st.tab_mut(&name) {
                            t.name = n.clone();
                        }
                        // Les regles qui citaient l'onglet le suivent.
                        let (old, new) = (Src::Tab(name.clone()).key(), Src::Tab(n.clone()).key());
                        for t in &mut self.fxb.st.tabs {
                            if let Some(r) = &mut t.rule {
                                if r.src == old {
                                    r.src = new.clone();
                                }
                            }
                        }
                        if self.fxb.view.src == Src::Tab(name.clone()) {
                            self.fxb.view.src = Src::Tab(n.clone());
                        }
                        self.fxb_show_tab(&n);
                    }
                }
            }
            2 => {
                let cur = tab
                    .rule
                    .as_ref()
                    .map_or("", |r| r.query.as_str())
                    .to_string();
                if let Some(v) = self.fxb_ask_name("Tab search (every word, -word excludes)", &cur)
                {
                    let src = tab
                        .rule
                        .as_ref()
                        .map_or_else(|| "all".to_string(), |r| r.src.clone());
                    if let Some(t) = self.fxb.st.tab_mut(&name) {
                        t.rule = Some(Rule {
                            query: v.trim().to_string(),
                            src,
                        });
                    }
                    self.fxb_show_tab(&name);
                }
            }
            4 => {
                if let Some(t) = self.fxb.st.tab_mut(&name) {
                    t.rule = None;
                }
                self.fxb_show_tab(&name);
            }
            5 | 6 => {
                if let Some(p) = pos {
                    let q = if k == 5 {
                        p.checked_sub(1)
                    } else {
                        Some(p + 1)
                    };
                    if let Some(q) = q.filter(|&q| q < self.fxb.st.tabs.len()) {
                        self.fxb.st.tabs.swap(p, q);
                    }
                }
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                self.fxb_save_soon();
            }
            3 => {
                if let Some(p) = pos {
                    self.fxb.st.tabs.remove(p);
                }
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
                self.fxb.st.set.src = self.fxb.view.src.key();
                self.fxb_save_soon();
            }
            _ => {}
        }
    }

    fn fxb_add_tab_menu(&mut self, x: f32, y: f32) {
        let sel = self.fxb.view.selection();
        let q = self.fxb.view.search.text.trim().to_string();
        let src = self.fxb.view.src.clone();
        let can_rule = !q.is_empty() || src != Src::All;
        let items = [
            Some(item(1, "New tab", false, true)),
            Some(item(
                2,
                "New tab with the selection",
                false,
                !sel.is_empty(),
            )),
            Some(item(3, "New tab from this search", false, can_rule)),
        ];
        let k = self.fxb_popup(x, y, &items);
        if k == 0 {
            return;
        }
        let default = match k {
            3 if !q.is_empty() => q.clone(),
            3 => src.name(),
            _ => String::new(),
        };
        let Some(name) = self.fxb_ask_name("New tab", &default) else {
            return;
        };
        let name = self.fxb.st.free_tab_name(&name);
        let items = if k == 2 {
            sel.iter()
                .map(|&i| self.fxb.cat.plugins[i].name.clone())
                .collect()
        } else {
            Vec::new()
        };
        let rule = (k == 3).then(|| Rule {
            query: q.clone(),
            src: src.key(),
        });
        self.fxb.st.tabs.push(Tab {
            name: name.clone(),
            items,
            rule,
        });
        if k == 3 {
            self.fxb.view.search.clear();
        }
        self.fxb_show_tab(&name);
    }

    fn fxb_dice_menu(&mut self, x: f32, y: f32) {
        let mut e: Vec<MenuEntry> = (1..=12u32)
            .map(|n| {
                let l = if n == 1 {
                    "1 effect".to_string()
                } else {
                    format!("{n} effects")
                };
                MenuEntry::check(n, l, self.fxb.st.set.dice_count == n)
            })
            .collect();
        e.push(MenuEntry::Sep);
        e.push(MenuEntry::check(
            50,
            "Replace the chain (instruments and FX Constellation stay)",
            self.fxb.st.set.dice_replace,
        ));
        e.push(post_menu(&self.fxb.st.set.post));
        match self.fxb_tree(x, y, &e) {
            k @ 1..=12 => self.fxb.st.set.dice_count = k,
            50 => self.fxb.st.set.dice_replace = !self.fxb.st.set.dice_replace,
            id @ 700..=799 => post_pick(&mut self.fxb.st.set.post, id),
            _ => return,
        }
        self.fxb_save_soon();
    }

    /// Un menu en popover : 0 tant que le choix n'est pas fait (le geste
    /// s'arrete) ; le choix rejoue le geste et rend ici son numero
    /// (`crate::ask`).
    fn fxb_tree(&mut self, x: f32, y: f32, entries: &[MenuEntry]) -> u32 {
        match self.fxb.ask.pick(entries) {
            Ask::Ready(id) => id,
            Ask::Open => {
                let e = entries.iter().map(MenuEntry::to_pop).collect();
                self.fxb.view.open_pop(Pop::menu(0, e, x, y));
                self.fxb_kick();
                0
            }
            Ask::Skip => 0,
        }
    }

    fn fxb_menu(&mut self, x: f32, y: f32) {
        let s = self.fxb.st.set.clone();
        let p = self.fxb.place;
        let has_ext = self.host.ext().is_some_and(|e| e.version >= 2);
        let e = vec![
            MenuEntry::check(1, "Open the FX window on add", s.auto_open),
            MenuEntry::check(2, "Sort Z to A", s.sort_za),
            post_menu(&s.post),
            MenuEntry::Sep,
            MenuEntry::check(3, "Show sources (folders, categories...)", s.show_side),
            MenuEntry::check(4, "Show the track's FX chain", s.show_chain),
            MenuEntry::Sep,
            MenuEntry::check(
                5,
                if has_ext {
                    "Replace REAPER's FX browser"
                } else {
                    "Replace REAPER's FX browser (restart REAPER)"
                },
                s.steal,
            ),
            MenuEntry::item(6, "Reload the plugin list"),
            MenuEntry::Sep,
            MenuEntry::check(10, "Show in arrange view", p == Place::Arrange),
            MenuEntry::check(11, "Dock in docker", p == Place::Dock),
            MenuEntry::check(12, "Floating window", p == Place::Float),
            MenuEntry::item(13, "Close the FX Browser"),
        ];
        match self.fxb_tree(x, y, &e) {
            id @ 700..=799 => post_pick(&mut self.fxb.st.set.post, id),
            1 => self.fxb.st.set.auto_open = !s.auto_open,
            2 => {
                self.fxb.st.set.sort_za = !s.sort_za;
                self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
            }
            3 => self.fxb.st.set.show_side = !s.show_side,
            4 => self.fxb.st.set.show_chain = !s.show_chain,
            5 => {
                self.fxb.st.set.steal = !s.steal;
                self.fxb_steal_tick();
            }
            6 => self.fxb_reload(),
            10 => self.queue(WinOp::FxbPlace(Place::Arrange)),
            11 => self.queue(WinOp::FxbPlace(Place::Dock)),
            12 => self.queue(WinOp::FxbPlace(Place::Float)),
            13 => self.queue(WinOp::FxbClose),
            _ => return,
        }
        self.fxb.view.mark();
        self.fxb_save_soon();
    }

    fn fxb_fx_menu(&mut self, fx: usize, x: f32, y: f32) {
        let Some(f) = self.fxb.chain.fx.get(fx).cloned() else {
            return;
        };
        let id = self.fxb.chain_guids.get(fx).cloned().unwrap_or_default();
        let items = [
            Some(item(1, "Open", false, true)),
            Some(item(2, if f.on { "Bypass" } else { "Enable" }, false, true)),
            Some(item(3, "Delete", false, true)),
            None,
            Some(item(
                5,
                "Detached (ROLL and the dice leave it alone)",
                f.detached,
                !id.is_empty(),
            )),
            None,
            Some(item(4, "Show REAPER's FX chain", false, true)),
        ];
        let k = self.fxb_popup(x, y, &items);
        match k {
            1 => self.fxb_act(Act::FxOpen(fx)),
            2 => self.fxb_act(Act::FxToggle(fx)),
            3 => self.fxb_act(Act::FxDelete(fx)),
            5 => {
                if let Some(g) = self.fxb.track.clone() {
                    self.fxc_set_detached_on(&g, &id, !f.detached);
                }
                self.fxb_sync_chain();
            }
            4 => {
                if let (Some(g), Some(k)) = (self.fxb.track.clone(), self.fxb_real(fx)) {
                    self.r.fx_open(&g, k, true);
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Remplacer celui de REAPER
    // -----------------------------------------------------------------------

    /// Ce que le chargeur prend pour nous suit le reglage.
    fn fxb_steal_tick(&mut self) {
        let want = self.fxb.st.set.steal;
        if self.fxb.stolen == Some(want) {
            return;
        }
        let ok = self.host.steal_action(REAPER_FX_BROWSER, want)
            && self.host.steal_action(REAPER_INSERT_INSTRUMENT, want)
            && self.host.steal_fx_button(want);
        self.fxb.stolen = Some(want);
        if want && !ok {
            self.host.info(
                "navigateur d'effets : le chargeur ne sait pas encore prendre la place de celui de REAPER (redemarrer REAPER)",
            );
        }
    }

    /// Les actions que le chargeur nous passe. `None` : pas une des notres.
    pub(crate) fn fxb_on_action(&mut self, cmd: i32) -> Option<bool> {
        match cmd {
            REAPER_FX_BROWSER => {
                if !self.fxb.st.set.steal {
                    return Some(false);
                }
                self.fxb.taken += 1;
                self.fxb_toggle();
                Some(true)
            }
            REAPER_INSERT_INSTRUMENT => {
                if !self.fxb.st.set.steal {
                    return Some(false);
                }
                let sel = self.r.fx_selected_track();
                self.fxb.new_track = Some(sel);
                self.fxb_load();
                let (cat, st) = (&self.fxb.cat, &mut self.fxb.st);
                self.fxb.view.set_source(cat, st, Src::Instr);
                self.fxb_ready_to_type();
                self.queue(WinOp::FxbOpen);
                Some(true)
            }
            ACTION_FX_BUTTON => {
                if !self.fxb.st.set.steal {
                    return Some(false);
                }
                let (sx, sy) = ui::cursor_pos();
                let (g, _) = self.r.thing_at(sx, sy);
                let Some(g) = g else { return Some(false) };
                // Une piste qui a des effets : sa chaine, celle de REAPER.
                if self.r.fx_count(&g) > 0 {
                    return Some(false);
                }
                self.r.fx_select_track(&g);
                self.fxb.new_track = None;
                self.fxb_ready_to_type();
                self.queue(WinOp::FxbOpen);
                Some(true)
            }
            // La boite « Add FX to ... » de REAPER (le bouton « Add » d'une
            // chaine, un emplacement libre de la liste d'effets d'une
            // piste) : la notre, pour la meme piste.
            c if (ACTION_ADD_FX_TO - ADD_FX_MAX_TRACK..=ACTION_ADD_FX_TO).contains(&c) => {
                if !self.fxb.st.set.steal {
                    return Some(false);
                }
                let Some(g) = self.r.fx_track_by_number(ACTION_ADD_FX_TO - c) else {
                    return Some(false);
                };
                self.fxb.adds += 1;
                self.r.fx_select_track(&g);
                self.fxb.new_track = None;
                self.fxb_ready_to_type();
                self.queue(WinOp::FxbOpen);
                Some(true)
            }
            _ => None,
        }
    }

    /// La recherche prete a recevoir la frappe.
    fn fxb_ready_to_type(&mut self) {
        self.fxb.view.focus_search = true;
        self.fxb.view.search.select_all();
        self.fxb.view.mark();
    }

    /// Ouvre le navigateur pour une piste (la tranche de la Session).
    pub(crate) fn fxb_open_for(&mut self, guid: &str) {
        self.r.fx_select_track(guid);
        self.fxb.new_track = None;
        self.fxb_ready_to_type();
        self.queue(WinOp::FxbOpen);
    }

    // -----------------------------------------------------------------------
    // La fenetre
    // -----------------------------------------------------------------------

    pub(crate) fn fxb_toggle(&mut self) {
        if self.fxb.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd)) && !self.fxb_hidden_in_band()
        {
            self.queue(WinOp::FxbClose);
        } else {
            self.fxb_ready_to_type();
            self.queue(WinOp::FxbOpen);
        }
    }

    fn fxb_hidden_in_band(&self) -> bool {
        self.fxb
            .win
            .as_ref()
            .is_some_and(|w| w.place == Place::Arrange)
            && !self.fxb_in_band()
    }

    pub(crate) fn fxb_in_band(&self) -> bool {
        let Some(w) = &self.fxb.win else { return false };
        let (st, h) = self.host.place_state(PLACE_ARRANGE);
        st == PLACE_HELD && h == Some(w.hwnd)
    }

    pub(crate) fn fxb_open(&mut self) {
        self.fxb_load();
        if let Some(w) = &self.fxb.win {
            let h = w.hwnd;
            match w.place {
                Place::Dock => self.r.dock_activate(h),
                Place::Float => win::show(h),
                Place::Arrange => {
                    if !self.fxb_in_band() && self.host.place_take(PLACE_ARRANGE, Some(h), true) {
                        self.fxb_render(true);
                    }
                }
            }
            ui::set_focus(h);
            self.fxb_sync_chain();
            self.fxb_render(true);
            return;
        }
        let place = self.fxb.place;
        let mode = match place {
            Place::Arrange => VIEW_PLACED,
            Place::Dock => VIEW_DOCK,
            Place::Float => VIEW_FLOAT,
        };
        let Some(hwnd) = self.host.create_view(TITLE, IDENT, -1, -1, 560, 640, mode) else {
            self.host
                .error("la fenetre du navigateur d'effets n'a pas pu etre creee");
            return;
        };
        if place == Place::Arrange && !self.host.place_take(PLACE_ARRANGE, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.fxb.place = Place::Dock;
            self.fxb_open();
            return;
        }
        self.fxb_adopt(hwnd);
        if place == Place::Arrange {
            self.host.place_take(PLACE_ARRANGE, Some(hwnd), true);
            self.fxb_render(true);
        }
        ui::set_focus(hwnd);
        self.fxb_keep("fxb_open", "1");
    }

    pub(crate) fn fxb_adopt(&mut self, hwnd: Hwnd) {
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        let r = self.r;
        self.fxb.win = Some(FWin {
            hwnd,
            place: self.fxb.place,
            frame: crate::frame::Frame::default(),
            theme: self.reaper_theme(scale),
            theme_key: LOOK.iter().map(|k| r.theme_color(k)).collect(),
            tracking: false,
            closing: false,
            tip: None,
            hover: (Hit::None, Instant::now()),
        });
        self.fxb_load();
        self.fxb_sync_chain();
        self.fxb.view.mark();
        self.fxb_render(true);
    }

    pub(crate) fn fxb_shutdown(&mut self) {
        let h = self.fxb.win.as_ref().map_or(0, |w| w.hwnd.0);
        if let Some(w) = &self.fxb.win {
            self.host.frames(w.hwnd, false);
        }
        self.r.ext_set(SECTION, "fxb_hwnd", &h.to_string(), false);
        if self.fxb.dirty_at.is_some() {
            self.fxb_save_now();
        }
        // REAPER reprend ses actions tant qu'aucun module ne les tient.
        if self.fxb.stolen == Some(true) {
            self.host.steal_action(REAPER_FX_BROWSER, false);
            self.host.steal_action(REAPER_INSERT_INSTRUMENT, false);
            self.host.steal_fx_button(false);
        }
    }

    pub(crate) fn fxb_close(&mut self, user: bool) {
        let in_band = self.fxb_in_band();
        if let Some(w) = &mut self.fxb.win {
            w.closing = true;
            let h = w.hwnd;
            if in_band {
                self.host.place_take(PLACE_ARRANGE, None, false);
            }
            self.host.destroy_window(h);
        }
        self.fxb.win = None;
        self.fxb.new_track = None;
        if user {
            self.fxb_keep("fxb_open", "0");
        }
        if self.fxb.dirty_at.is_some() {
            self.fxb_save_now();
        }
    }

    pub(crate) fn fxb_set_place(&mut self, p: Place) {
        let reopen = self.fxb.win.as_ref().is_some_and(|w| w.place != p);
        self.fxb.place = p;
        self.fxb_keep("fxb_place", p.key());
        if reopen {
            self.fxb_close(false);
        }
        self.fxb_open();
    }

    /// Les choix de place de la fenetre (et la poignee du docker).
    fn fxb_place_entries(&self) -> Vec<MenuEntry> {
        let p = self.fxb.place;
        let open = self.fxb.win.is_some();
        let items = [
            Some(item(1, "Show in arrange view", p == Place::Arrange, true)),
            Some(item(2, "Dock in docker", p == Place::Dock, true)),
            Some(item(3, "Floating window", p == Place::Float, true)),
            None,
            Some(item(
                20,
                "Docker handle: auto",
                self.handle_mode() == 0,
                true,
            )),
            Some(item(
                21,
                "Docker handle: always",
                self.handle_mode() == 1,
                true,
            )),
            Some(item(
                22,
                "Docker handle: never",
                self.handle_mode() == 2,
                true,
            )),
            None,
            Some(item(4, "Close the FX Browser", false, open)),
        ];
        ui::flat_to_tree(&items)
    }

    fn fxb_place_pick(&mut self, id: u32) {
        match id {
            k @ 20..=22 => self.set_handle_mode((k - 20) as u8),
            1 => self.queue(WinOp::FxbPlace(Place::Arrange)),
            2 => self.queue(WinOp::FxbPlace(Place::Dock)),
            3 => self.queue(WinOp::FxbPlace(Place::Float)),
            4 => self.queue(WinOp::FxbClose),
            _ => {}
        }
    }

    /// La place de la fenetre, en popover (le choix rejoue le geste).
    fn fxb_place_menu(&mut self, x: f32, y: f32) {
        let e = self.fxb_place_entries();
        let id = self.fxb_tree(x, y, &e);
        self.fxb_place_pick(id);
    }

    /// Depuis le transport (son icone) : le popover vient avec la vue du
    /// transport ; d'ici la, le menu du systeme.
    pub(crate) fn fxb_place_menu_at(&mut self, owner: Hwnd, sx: i32, sy: i32) {
        let e = self.fxb_place_entries();
        let id = ui::popup_tree(owner, sx, sy, &e);
        self.fxb_place_pick(id);
    }

    fn fxb_theme(&self) -> Theme {
        self.fxb
            .win
            .as_ref()
            .map_or_else(|| Theme::dark(1.0), |w| w.theme.clone())
    }

    fn fxb_want_frames(&mut self, on: bool) {
        if let Some(w) = &mut self.fxb.win {
            if on {
                w.frame.wake();
            }
            if w.frame.frames_on != on {
                w.frame.frames_on = on;
                self.host.frames(w.hwnd, on);
            }
        }
    }

    fn fxb_kick(&mut self) {
        let on = self.fxb.win.as_ref().is_some_and(|w| w.frame.frames_on);
        if !on {
            self.fxb_render(false);
        }
        self.fxb_want_frames(true);
    }

    pub(crate) fn fxb_render(&mut self, force: bool) {
        let Some(w) = self.fxb.win.as_mut() else {
            return;
        };
        if !win::is_window(w.hwnd) {
            return;
        }
        let rc = ui::client_rect(w.hwnd);
        let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
        if cw <= 0 || ch <= 0 {
            return;
        }
        let scale = ui::dpi(w.hwnd) as f32 / 96.0;
        if (scale - w.theme.scale).abs() > 0.01 {
            let th = self.reaper_theme(scale);
            let Some(w) = self.fxb.win.as_mut() else {
                return;
            };
            w.theme = th;
            self.fxb.view.mark();
        }
        let Some(w) = self.fxb.win.as_mut() else {
            return;
        };
        if w.frame.resize(cw, ch) || force {
            self.fxb.view.mark();
        }
        if !self.fxb.view.take_dirty() {
            w.frame.show(w.hwnd, &[], &[]);
            return;
        }
        let damage = vec![IRect::new(0, 0, cw, ch)];
        let msg = self.fxb.flash.as_ref().map(|f| f.0.clone());
        let (view, cat, st, chain) = (
            &mut self.fxb.view,
            &self.fxb.cat,
            &self.fxb.st,
            &self.fxb.chain,
        );
        let (text, icons, th) = (&mut self.text, &mut self.icons, &w.theme);
        w.frame.paint(&damage, &mut self.scratch, |cv| {
            view.draw(
                cv,
                text,
                icons,
                th,
                cat,
                st,
                chain,
                msg.as_deref(),
                cw as f32,
                ch as f32,
            );
        });
        w.frame.show(w.hwnd, &damage, &[]);
    }

    fn fxb_feed(&mut self, ev: Ev) {
        let th = self.fxb_theme();
        let a = self.fxb.view.event(
            &self.fxb.cat,
            &mut self.fxb.st,
            &self.fxb.chain,
            &th,
            &mut self.text,
            &ev,
        );
        if let Some(a) = a {
            self.fxb_act(a);
        }
        self.fxb_kick();
    }

    fn fxb_tip(&mut self) {
        let h = self.fxb.view.hover;
        let dragging = self.fxb.view.dragging();
        let Some(w) = self.fxb.win.as_mut() else {
            return;
        };
        if h != w.hover.0 {
            w.hover = (h, Instant::now());
            if let Some(t) = &mut w.tip {
                t.hide();
            }
            return;
        }
        let shown = w.tip.as_ref().is_some_and(|t| t.is_shown());
        if dragging && shown {
            if let Some(t) = &mut w.tip {
                t.hide();
            }
        }
        if shown || dragging || w.hover.1.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        let text = self
            .fxb
            .view
            .label(&self.fxb.cat, &self.fxb.st, &self.fxb.chain, h);
        if text.is_empty() {
            return;
        }
        let (sx, sy) = ui::cursor_pos();
        let Some(w) = self.fxb.win.as_mut() else {
            return;
        };
        if w.tip.is_none() {
            w.tip = ui::Tooltip::new(w.hwnd);
        }
        if let Some(t) = &mut w.tip {
            t.show(&text, sx + 12, sy + 20);
        }
    }

    pub(crate) fn fxb_tick(&mut self) {
        self.fxb.ticks = self.fxb.ticks.wrapping_add(1);
        if self.fxb.ticks % 15 == 1 {
            self.fxb_steal_tick();
        }
        if self
            .fxb
            .dirty_at
            .is_some_and(|t| t.elapsed().as_secs_f64() > 0.8)
        {
            self.fxb_save_now();
        }
        if self
            .fxb
            .flash
            .as_ref()
            .is_some_and(|f| f.1.elapsed().as_secs_f64() > 2.5)
        {
            self.fxb.flash = None;
            self.fxb.view.mark();
        }
        let Some(hwnd) = self.fxb.win.as_ref().map(|w| w.hwnd) else {
            return;
        };
        if !ui::is_shown(hwnd) {
            return;
        }
        // La chaine de la piste choisie, cinq fois par seconde.
        if self.fxb.ticks % 6 == 0 && !self.fxb.view.dragging() {
            self.fxb_sync_chain();
        }
        if self.fxb.ticks % 10 == 0 {
            let key: Vec<_> = LOOK.iter().map(|k| self.r.theme_color(k)).collect();
            if let Some(w) = &self.fxb.win {
                if w.theme_key != key {
                    let sc = w.theme.scale;
                    let th = self.reaper_theme(sc);
                    if let Some(w) = &mut self.fxb.win {
                        w.theme = th;
                        w.theme_key = key;
                    }
                    self.fxb.view.mark();
                }
            }
        }
        self.fxb_tip();
        self.fxb_render(false);
    }

    pub(crate) fn fxb_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.fxb.win.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        match m.msg {
            WM_CP_FRAME => {
                if self.fxb.view.dragging() {
                    self.fxb.view.mark();
                }
                self.fxb_render(false);
                let idle = self.fxb.win.as_ref().is_none_or(|w| w.frame.idle());
                if !self.fxb.view.dragging() && idle {
                    self.fxb_want_frames(false);
                }
                Some(0)
            }
            WM_CP_ACCEL => {
                let Some(k) = key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                let usage = self.fxb.view.key_use(vk, md);
                Some(match k.message {
                    msg::KEYDOWN | msg::SYSKEYDOWN => match usage {
                        KeyUse::Eat => {
                            self.last_mods = md;
                            self.fxb_feed(Ev::Key {
                                vk,
                                mods: md,
                                text: None,
                            });
                            1
                        }
                        KeyUse::Text => -1,
                        KeyUse::NotMine => 0,
                    },
                    msg::CHAR | msg::SYSCHAR => {
                        let typing = match &self.fxb.view.pop {
                            Some(p) => p.wants_text(),
                            None => self.fxb.view.focus_search,
                        };
                        if typing { -1 } else { 0 }
                    }
                    msg::KEYUP | msg::SYSKEYUP => match usage {
                        KeyUse::Eat => 1,
                        KeyUse::Text => -1,
                        KeyUse::NotMine => 0,
                    },
                    _ => 0,
                })
            }
            msg::GETDLGCODE => Some(0x0004 | 0x0080 | 0x0001),
            msg::KEYDOWN | msg::SYSKEYDOWN => {
                self.fxb_feed(Ev::Key {
                    vk: m.wp as u32,
                    mods: md,
                    text: None,
                });
                Some(0)
            }
            msg::CHAR => {
                if let Some(c) = char::from_u32(m.wp as u32) {
                    self.fxb_feed(Ev::Char(c));
                }
                Some(0)
            }
            msg::SIZE => {
                self.fxb.view.mark();
                self.fxb_render(true);
                Some(0)
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                let shown = self.fxb.win.as_mut().is_some_and(|w| w.frame.repaint(hwnd));
                if !shown {
                    self.fxb_render(true);
                }
                Some(0)
            }
            msg::ERASEBKGND => Some(1),
            msg::SETCURSOR if (m.lp & 0xFFFF) == 1 => {
                let (cx, cy) = Self::mouse_over(hwnd)?;
                let th = self.fxb_theme();
                let p = match self.fxb.view.hit(&th, &self.fxb.st, cx, cy) {
                    Hit::DockHandle => cp_ui::Pointer::ItemMove,
                    Hit::SideSeam | Hit::ChainSeam => cp_ui::Pointer::ResizeWE,
                    Hit::Search => cp_ui::Pointer::IBeam,
                    _ => return None,
                };
                ui::set_pointer(p);
                Some(1)
            }
            msg::MOUSEMOVE => {
                if let Some(w) = &mut self.fxb.win {
                    if !w.tracking {
                        w.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                self.fxb_feed(Ev::Move { x, y, mods: md });
                // Le glisser commun : des effets tires hors de la fenetre
                // allument la vue qui les prendrait.
                if self.fxb.view.dragging_rows() {
                    let (sx, sy) = ui::client_to_screen(hwnd, x as i32, y as i32);
                    let plugins = self.fxb.view.selection();
                    self.carry_move(crate::carry::Payload::Plugins(plugins), sx, sy);
                } else {
                    self.carry_cancel();
                }
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(w) = &mut self.fxb.win {
                    w.tracking = false;
                }
                if !self.fxb.view.dragging() {
                    self.fxb_feed(Ev::Leave);
                }
                Some(0)
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                ui::set_focus(hwnd);
                ui::capture(hwnd);
                let b = match m.msg {
                    msg::LBUTTONDOWN | msg::LBUTTONDBLCLK => Button::Left,
                    _ => Button::Right,
                };
                self.last_mods = md;
                if let Some(w) = &mut self.fxb.win {
                    if let Some(t) = &mut w.tip {
                        t.hide();
                    }
                }
                self.fxb_feed(Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                });
                Some(0)
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                if ui::has_capture(hwnd) {
                    ui::release_capture();
                }
                let b = if m.msg == msg::LBUTTONUP {
                    Button::Left
                } else {
                    Button::Right
                };
                self.fxb_feed(Ev::Up { x, y, b, mods: md });
                // Lache ailleurs que sur une vue qui le prend : rien ne reste
                // allume.
                self.carry_cancel();
                Some(0)
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                let (cx, cy) = ui::screen_to_client(hwnd, x as i32, y as i32);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                let (dx, dy) = if m.msg == msg::MOUSEWHEEL {
                    (0.0, d)
                } else {
                    (d, 0.0)
                };
                self.fxb_feed(Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx,
                    dy,
                    mods: md,
                });
                Some(0)
            }
            msg::CLOSE => {
                self.queue(WinOp::FxbClose);
                Some(0)
            }
            msg::DESTROY => {
                let ours = self.fxb.win.as_ref().is_some_and(|w| w.closing);
                if !ours {
                    self.fxb.win = None;
                }
                None
            }
            _ => None,
        }
    }

    /// Pour l'auto-test et le journal : ce que montre le navigateur.
    pub(crate) fn fxb_state_line(&self) -> String {
        format!(
            "open={} shown={} taken={} adds={} src={} rows={} sel={} track={} fx={} steal={:?}",
            self.fxb.win.is_some() as u8,
            self.fxb.win.as_ref().is_some_and(|w| ui::is_shown(w.hwnd)) as u8,
            self.fxb.taken,
            self.fxb.adds,
            self.fxb.view.src.key(),
            self.fxb.view.rows.len(),
            self.fxb.view.selection().len(),
            self.fxb.chain.track.as_deref().unwrap_or("-"),
            self.fxb.chain.fx.len(),
            self.fxb.stolen
        )
    }
}

impl App {
    /// L'auto-test, par pas (le chargeur nomme le pas dans
    /// `selftest_fxb_step`) : « run » ajoute, deplace, contourne, copie et
    /// tire au de sur la piste choisie ; « state » dit ce que montre le
    /// navigateur ; « clean » le ferme.
    pub(crate) fn fxb_selftest(&mut self) {
        let r = self.r;
        let step = r.ext_get(SECTION, "selftest_fxb_step").unwrap_or_default();
        let out = match step.as_str() {
            "run" => self.fxb_selftest_run(),
            "state" => format!("state {}", self.fxb_state_line()),
            "clean" => {
                self.fxb_close(false);
                "cleaned".to_string()
            }
            s => format!("unknown step {s}"),
        };
        r.ext_set(SECTION, "selftest_fxb", &out, false);
    }

    fn fxb_selftest_run(&mut self) -> String {
        // Flottante : dans un docker, elle pourrait etre l'onglet cache
        // derriere le navigateur, et l'action la montrerait au lieu de la
        // fermer.
        self.fxb.place = Place::Float;
        self.fxb_open();
        self.fxb.st.set.auto_open = false;
        self.fxb_sync_chain();
        let labels = |a: &App| -> String {
            a.fxb
                .chain
                .fx
                .iter()
                .map(|f| cp_fxview::fx_label(&f.name))
                .collect::<Vec<_>>()
                .join("|")
        };
        let find = |a: &App, n: &str| {
            a.fxb
                .cat
                .plugins
                .iter()
                .position(|p| p.label == n && p.dev == "Cockos")
        };
        let (Some(eq), Some(comp)) = (find(self, "ReaEQ"), find(self, "ReaComp")) else {
            return format!(
                "report error=no_reaeq plugins={}",
                self.fxb.cat.plugins.len()
            );
        };
        self.fxb_add(&[eq], None, None, false);
        self.fxb_add(&[comp], Some(0), None, false);
        let order1 = labels(self);
        self.fxb_act(Act::FxMove {
            from: 0,
            to: 2,
            copy: false,
        });
        let order2 = labels(self);
        self.fxb_act(Act::FxToggle(0));
        let bypass = self.fxb.chain.fx.first().is_some_and(|f| !f.on);
        self.fxb_act(Act::FxMove {
            from: 0,
            to: 2,
            copy: true,
        });
        let copied = labels(self);
        // Le de : deux effets de Cockos, ajoutes ; puis un seul, a la place
        // de toute la chaine.
        let (cat, st) = (&self.fxb.cat, &mut self.fxb.st);
        self.fxb.view.set_source(cat, st, Src::Dev("Cockos".into()));
        self.fxb.st.set.dice_count = 2;
        self.fxb.st.set.dice_replace = false;
        let before = self.fxb.chain.fx.len();
        self.fxb_dice();
        let dice = self.fxb.chain.fx.len() as i64 - before as i64;
        // Le coeur (ici le bridge de FX Constellation) reste quand le de
        // remplace la chaine ; « replaced » compte les autres effets.
        let r = self.r;
        let g = self.fxb.track.clone().unwrap_or_default();
        let _ = r.fx_add(&g, cp_fxc::slots::bridge::ADD, None);
        self.fxb_sync_chain();
        self.fxb.st.set.dice_count = 1;
        self.fxb.st.set.dice_replace = true;
        self.fxb_dice();
        // Le bridge reste sur la piste, mais la chaine montree le cache.
        let real = r.fx_track(&g).map(|t| t.fx).unwrap_or_default();
        let core_kept = real.iter().filter(|f| is_core(&f.0)).count();
        let hidden = self.fxb.chain_total - self.fxb.chain.fx.len();
        let replaced = self.fxb.chain.fx.len();
        // Une rangee montree touche son vrai effet : contourner la premiere
        // (le bridge est avant elle dans la vraie chaine).
        let first_real = self.fxb_real(0);
        self.fxb_act(Act::FxToggle(0));
        let toggled = first_real.is_some_and(|k| {
            let now = r.fx_track(&g).map(|t| t.fx).unwrap_or_default();
            now.get(k).is_some_and(|f| !f.1 && !is_core(&f.0))
                && now.iter().filter(|f| is_core(&f.0)).all(|f| f.1)
        }) as u8;
        self.fxb_act(Act::FxToggle(0));
        // La post-insertion : deux parametres de ReaEQ suivis par FX
        // Constellation.
        self.fxb.st.set.post = PostInsert {
            params: PostParams::Random(2),
            ..PostInsert::default()
        };
        self.fxb_add(&[eq], None, None, false);
        self.fxb.st.set.post = PostInsert::default();
        let post = r
            .xc_track(&g)
            .and_then(|t| {
                let fg = r.xc_fx_guid(t, r.xc_fx_count(t).checked_sub(1)?)?;
                let model = if self.fxc.tk == Some(t) && self.fxc.guid == g {
                    self.fxc.t.clone()
                } else {
                    cp_fxc::store::decode(&self.fxc_stored_at(t, &g)?)?
                };
                Some(model.tracked().filter(|(k, _)| k.starts_with(&fg)).count())
            })
            .unwrap_or(0);
        // Un effet hors de ROLL (DETACHED) : la chaine le dit, le de qui
        // remplace la chaine le garde.
        self.fxb_sync_chain();
        let id0 = self.fxb.chain_guids.first().cloned().unwrap_or_default();
        self.fxc_set_detached_on(&g, &id0, true);
        self.fxb_sync_chain();
        let det_shown = self.fxb.chain.fx.first().is_some_and(|f| f.detached) as u8;
        self.fxb.st.set.dice_count = 1;
        self.fxb.st.set.dice_replace = true;
        self.fxb_dice();
        let det_kept = (!id0.is_empty() && self.fxb.chain_guids.contains(&id0)) as u8;
        // La recherche.
        let (cat, st) = (&self.fxb.cat, &mut self.fxb.st);
        self.fxb.view.set_source(cat, st, Src::All);
        for c in "reaeq cockos".chars() {
            self.fxb.view.char(cat, st, c);
        }
        let found = self.fxb.view.rows.contains(&eq);
        self.fxb.view.search.clear();
        self.fxb.view.refresh(&self.fxb.cat, &self.fxb.st);
        format!(
            "report plugins={} folders={} favs={} tabs={} order1={order1} order2={order2} bypass={} copy={copied} dice={dice} replaced={replaced} core_kept={core_kept} hidden={hidden} toggled={toggled} post={post} det_shown={det_shown} det_kept={det_kept} search={} stolen={}",
            self.fxb.cat.plugins.len(),
            self.fxb.cat.folders.len(),
            self.fxb.cat.reaper_favs.len(),
            self.fxb.st.tabs.len(),
            bypass as u8,
            found as u8,
            (self.fxb.stolen == Some(true)) as u8,
        )
    }
}

#[cfg(test)]
mod core_tests {
    fn desc(src: &str) -> String {
        src.lines()
            .find_map(|l| l.strip_prefix("desc:"))
            .unwrap_or("")
            .trim()
            .to_string()
    }

    /// Chaque JSFX que CP Studio pose est du coeur, sous tous ses noms : le de
    /// ne le tire pas, remplacer la chaine ne le retire pas.
    #[test]
    fn every_internal_jsfx_is_core() {
        let mut all: Vec<(&str, &str)> = crate::fxc_sync::FILES.to_vec();
        all.push(("", crate::gtab::SYNTH_SRC));
        for (file, src) in all {
            let d = desc(src);
            assert!(!d.is_empty(), "{file} sans desc");
            let mut names = vec![d.clone(), format!("JS: {d}")];
            if !file.is_empty() {
                names.push(file.to_string());
            }
            for n in names {
                assert!(super::is_core(&n), "{n}");
                assert!(cp_fxlist::is_internal(&n), "{n}");
            }
        }
        assert!(!super::is_core("VST3: kHs Delay (Kilohearts)"));
        assert!(!super::is_core("JS: CP_FC_Phaser"));
    }
}
