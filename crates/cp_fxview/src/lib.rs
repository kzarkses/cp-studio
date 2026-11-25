//! La vue du navigateur d'effets : la recherche, les pastilles (listes fixes
//! et onglets), la barre de cote (dossiers, categories, developpeurs et
//! formats de REAPER), la liste, le de, et la chaine de la piste choisie.
//!
//! Ce crate ne connait ni Windows ni REAPER : il lit le [`Catalog`] et le
//! [`Store`], la [`Chain`] lue par le module, dessine, et traduit chaque
//! geste en une [`Act`] que le module execute. Aucune phrase a l'ecran : ce
//! qui s'explique vient au survol ([`View::label`]).

#![allow(clippy::too_many_arguments)]

mod draw;
#[cfg(test)]
mod tests;

pub use draw::fx_label;

use std::collections::BTreeSet;

use cp_fxlist::{Catalog, Src, Store, list};
use cp_gfx::Color;
use cp_gfx::text::TextCache;
use cp_ui::pop::{Out, Pop};
use cp_ui::textfield::Edit;
use cp_ui::{Button, Ev, Mods, TextField, Theme, vk};

/// Un effet de la chaine de la piste.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChainFx {
    /// Le nom de REAPER (« VST3: Pro-Q 3 (FabFilter) »).
    pub name: String,
    pub on: bool,
    /// Hors de ROLL dans FX Constellation (DETACHED) : rien ne le tire au
    /// hasard, ni ROLL ni le de.
    pub detached: bool,
}

/// La chaine que montre le panneau de droite.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Chain {
    /// Le nom de la piste (aucune : rien n'est choisi).
    pub track: Option<String>,
    pub color: Option<Color>,
    /// Le prochain ajout cree une piste (Ctrl+Y de REAPER).
    pub new_track: bool,
    pub fx: Vec<ChainFx>,
}

/// Les groupes de la barre de cote.
pub const GROUPS: [&str; 4] = ["Folders", "Categories", "Developers", "Formats"];

/// Ce qui est sous la souris.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hit {
    #[default]
    None,
    DockHandle,
    Search,
    SearchClear,
    Dice,
    Count,
    Menu,
    /// Une pastille (son rang dans [`View::chips`]).
    Chip(usize),
    ChipAdd,
    /// L'en-tete d'un groupe de la barre de cote.
    Group(usize),
    /// Une entree de la barre de cote (son rang dans [`View::side`]).
    Side(usize),
    SideSeam,
    /// Une rangee de la liste (son rang dans [`View::rows`]).
    Row(usize),
    Star(usize),
    ChainHead,
    Fx(usize),
    FxPower(usize),
    FxDelete(usize),
    /// Sous la derniere rangee de la chaine.
    ChainEmpty,
    ChainSeam,
}

/// Ce que le module doit faire.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Ce qu'un popover rend : (son contexte, le resultat).
    Pop(u32, Out),
    /// Ajouter des effets (rangs du catalogue) a la chaine, a la place `at`
    /// (aucune : la place par defaut).
    Add {
        plugins: Vec<usize>,
        at: Option<usize>,
    },
    /// Des effets lâches hors de la fenetre : sur une piste, une colonne de
    /// la Session, ou dans le vide (une piste neuve).
    DropOut {
        plugins: Vec<usize>,
    },
    ToggleFav(usize),
    /// Le menu de la selection.
    RowMenu {
        x: f32,
        y: f32,
    },
    /// Le menu d'une pastille (un onglet).
    ChipMenu {
        chip: usize,
        x: f32,
        y: f32,
    },
    /// Le « + » des onglets.
    AddTabMenu {
        x: f32,
        y: f32,
    },
    Menu {
        x: f32,
        y: f32,
    },
    DiceMenu {
        x: f32,
        y: f32,
    },
    Dice,
    /// Des effets glisses sur un onglet.
    ToTab {
        tab: String,
        plugins: Vec<usize>,
    },
    FxOpen(usize),
    FxToggle(usize),
    FxDelete(usize),
    FxMove {
        from: usize,
        to: usize,
        copy: bool,
    },
    FxMenu {
        fx: usize,
        x: f32,
        y: f32,
    },
    /// Le presse-papiers vers la recherche.
    Paste,
    /// Un reglage a change (liste, largeurs, groupes, compte du de) : a
    /// garder.
    Changed,
    DockGrab,
    /// Un autre onglet du docker est clique : montrer sa vue.
    DockSwitch(usize),
    ViewMenu {
        x: f32,
        y: f32,
    },
}

/// Ce que le module fait d'une touche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUse {
    /// La vue la prend (le module la lui donne et la retire a REAPER).
    Eat,
    /// Du texte : la fenetre la recoit (WM_KEYDOWN puis WM_CHAR).
    Text,
    /// A REAPER.
    NotMine,
}

/// Un rectangle de la vue.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> R {
        R { x, y, w, h }
    }
    pub fn has(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Les mesures de la maquette (a 96 DPI).
pub mod geo {
    pub const TOP: f32 = 34.0;
    pub const CHIPS: f32 = 26.0;
    pub const ROW: f32 = 22.0;
    pub const SIDE_ROW: f32 = 20.0;
    pub const CHAIN_HEAD: f32 = 26.0;
    pub const CHAIN_ROW: f32 = 24.0;
    pub const SEAM: f32 = 4.0;
    pub const BTN: f32 = 26.0;
    pub const PAD: f32 = 6.0;
    pub const STAR: f32 = 20.0;
    /// En dessous, la barre de cote se cache.
    pub const SIDE_FROM: f32 = 420.0;
    pub const SIDE_MIN: f32 = 90.0;
    pub const CHAIN_MIN: f32 = 150.0;
    /// La liste garde au moins cette largeur quand la chaine est a droite.
    pub const LIST_MIN: f32 = 240.0;
}

/// La geometrie d'une image.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub s: f32,
    pub w: f32,
    pub h: f32,
    pub search: R,
    pub dice: R,
    pub count: R,
    pub menu: R,
    pub chips: R,
    pub side: Option<R>,
    pub side_seam: Option<R>,
    pub list: R,
    pub chain: Option<R>,
    pub chain_seam: Option<R>,
    /// La chaine a droite (sinon : sous la liste).
    pub chain_right: bool,
    pub row: f32,
    pub side_row: f32,
    pub chain_head: f32,
    pub chain_row: f32,
}

/// Une entree de la barre de cote.
#[derive(Clone, Debug, PartialEq)]
pub enum SideEntry {
    Head(usize),
    Item(Src, usize),
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    /// Des rangees pressees : un glisser commence au-dela de 4 px.
    /// `reduce` : un clic sur une rangee deja choisie (la selection se
    /// reduit a elle au relachement, si rien n'a bouge).
    Rows {
        x0: f32,
        y0: f32,
        moved: bool,
        reduce: Option<usize>,
    },
    Fx {
        from: usize,
        y0: f32,
        moved: bool,
    },
    SideSeam {
        x0: f32,
        w0: f32,
    },
    ChainSeam {
        x0: f32,
        w0: f32,
    },
    Handle {
        x0: f32,
        y0: f32,
    },
}

/// L'etat de la vue.
pub struct View {
    /// Un popover ouvert (menu, saisie d'un nom, confirmation) : il prend
    /// la souris et le clavier, il se dessine par-dessus.
    pub pop: Option<Pop>,
    pub search: TextField,
    /// La frappe va a la recherche (sinon, Espace va a REAPER).
    pub focus_search: bool,
    pub src: Src,
    /// La liste montree : des rangs du catalogue.
    pub rows: Vec<usize>,
    pub side: Vec<SideEntry>,
    pub chips: Vec<Src>,
    sel: BTreeSet<usize>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    pub scroll: f32,
    pub side_scroll: f32,
    pub chain_scroll: f32,
    pub chip_scroll: f32,
    /// La rangee choisie de la chaine.
    pub fx_sel: Option<usize>,
    pub hover: Hit,
    pub mouse: (f32, f32),
    drag: Option<Drag>,
    pub size: (f32, f32),
    pub scale: f32,
    pub ctrl: bool,
    dirty: bool,
    /// Les onglets du docker de la vue ancree (le module les pose).
    pub dock: cp_ui::handle::DockTabs,
    /// Les pastilles a l'ecran : (x, largeur) dans la rangee, sans le
    /// defilement ; puis le « + ».
    chip_x: Vec<(f32, f32)>,
    chip_add: (f32, f32),
    /// Le nombre d'effets de la chaine (la geometrie, entre deux images).
    nfx: usize,
    /// La hauteur de la liste a la derniere image (les pages du clavier).
    list_h: f32,
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

impl View {
    pub fn new() -> View {
        View {
            pop: None,
            search: TextField::default(),
            focus_search: true,
            src: Src::All,
            rows: Vec::new(),
            side: Vec::new(),
            chips: Src::CHIPS.to_vec(),
            sel: BTreeSet::new(),
            anchor: None,
            cursor: None,
            scroll: 0.0,
            side_scroll: 0.0,
            chain_scroll: 0.0,
            chip_scroll: 0.0,
            fx_sel: None,
            hover: Hit::None,
            mouse: (-1.0, -1.0),
            drag: None,
            size: (0.0, 0.0),
            scale: 1.0,
            ctrl: false,
            dirty: true,
            dock: cp_ui::handle::DockTabs::new("FX Browser"),
            chip_x: Vec::new(),
            chip_add: (0.0, 0.0),
            nfx: 0,
            list_h: 0.0,
        }
    }

    /// Ouvre un popover (il remplace celui qui l'etait) ; le geste en cours
    /// s'arrete.
    pub fn open_pop(&mut self, p: Pop) {
        self.drag = None;
        self.pop = Some(p);
        self.mark();
    }

    /// Un evenement pour le popover ouvert : ce qu'il rend devient une
    /// [`Act::Pop`] ; ferme, il se retire.
    fn pop_event(&mut self, ev: &Ev) -> Option<Act> {
        let p = self.pop.as_mut()?;
        let before = p.stamp();
        let out = p.event(ev);
        let ctx = p.ctx;
        if p.stamp() != before {
            self.mark();
        }
        let o = out?;
        if o.closes() {
            self.pop = None;
        }
        self.mark();
        Some(Act::Pop(ctx, o))
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Des effets sont tenus (le curseur de copie hors de la fenetre).
    pub fn dragging_rows(&self) -> bool {
        matches!(self.drag, Some(Drag::Rows { moved: true, .. }))
    }

    pub fn on_tab(&self, x: f32, y: f32) -> bool {
        let (w, h) = self.size;
        self.dock.at(w, h, self.scale, x, y).is_some()
    }

    /// Les onglets de la vue ancree, au pied de la vue : la rangee.
    pub fn tab_rect(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.size;
        self.dock.bounds(w, h, self.scale)
    }

    pub(crate) fn grabbing_handle(&self) -> bool {
        matches!(self.drag, Some(Drag::Handle { .. }))
    }

    /// Les effets choisis, dans l'ordre de la liste.
    pub fn selection(&self) -> Vec<usize> {
        self.rows
            .iter()
            .copied()
            .filter(|i| self.sel.contains(i))
            .collect()
    }

    pub fn is_selected(&self, plugin: usize) -> bool {
        self.sel.contains(&plugin)
    }

    pub fn clear_selection(&mut self) {
        self.sel.clear();
        self.anchor = None;
        self.mark();
    }

    /// Choisit un seul effet (et le montre).
    pub fn select_only(&mut self, plugin: usize) {
        self.sel.clear();
        self.sel.insert(plugin);
        if let Some(r) = self.rows.iter().position(|&i| i == plugin) {
            self.anchor = Some(r);
            self.cursor = Some(r);
            self.reveal(r);
        }
        self.mark();
    }

    /// Relit la liste (la source, la recherche, le catalogue ou les
    /// onglets ont change). La selection garde ce qui reste visible.
    pub fn refresh(&mut self, cat: &Catalog, st: &Store) {
        self.chips = Src::CHIPS
            .iter()
            .cloned()
            .chain(st.tabs.iter().map(|t| Src::Tab(t.name.clone())))
            .collect();
        // Une liste disparue (onglet supprime, dossier renomme) : tout.
        let exists = match &self.src {
            Src::Tab(n) => st.tab(n).is_some(),
            Src::Folder(n) => cat.folders.iter().any(|f| &f.name == n),
            Src::Cat(n) => cat.cats.iter().any(|(c, _)| c == n),
            Src::Dev(n) => cat.devs.iter().any(|(c, _)| c == n),
            Src::Kind(k) => cat.kinds.iter().any(|(c, _)| c == k),
            _ => true,
        };
        if !exists {
            self.src = Src::All;
        }
        self.rows = list(cat, st, &self.src, &self.search.text);
        let shown: BTreeSet<usize> = self.rows.iter().copied().collect();
        self.sel.retain(|i| shown.contains(i));
        if self.cursor.is_some_and(|c| c >= self.rows.len()) {
            self.cursor = None;
        }
        if self.anchor.is_some_and(|c| c >= self.rows.len()) {
            self.anchor = None;
        }
        self.side = side_entries(cat, st);
        self.mark();
    }

    /// Montre une autre liste (la recherche reste).
    pub fn set_source(&mut self, cat: &Catalog, st: &mut Store, src: Src) {
        if self.src != src {
            self.src = src;
            self.scroll = 0.0;
            self.cursor = None;
            self.anchor = None;
        }
        st.set.src = self.src.key();
        self.refresh(cat, st);
    }

    pub fn layout(&self, th: &Theme, st: &Store, w: f32, h: f32) -> Layout {
        let s = th.scale;
        let k = |v: f32| (v * s).round();
        let pad = k(geo::PAD);
        let btn = k(geo::BTN);
        let top = k(geo::TOP);
        let by = ((top - btn) * 0.5).round();
        let menu = R::new(w - pad - btn, by, btn, btn);
        let count = R::new(menu.x - k(4.0) - k(22.0), by, k(22.0), btn);
        let dice = R::new(count.x - btn, by, btn, btn);
        let fh = k(24.0);
        let search = R::new(
            pad,
            ((top - fh) * 0.5).round(),
            (dice.x - k(6.0) - pad).max(k(40.0)),
            fh,
        );
        let chips = R::new(0.0, top, w, k(geo::CHIPS));
        let body_y = chips.bottom();
        let body_h = (h - body_y).max(0.0);
        let seam = k(geo::SEAM);
        let (side_w, chain_w) = self.live_widths(st);
        let (side, side_seam, x0) = if st.set.show_side && w >= k(geo::SIDE_FROM) {
            let sw = k(side_w).clamp(k(geo::SIDE_MIN), (w * 0.45).max(k(geo::SIDE_MIN)));
            (
                Some(R::new(0.0, body_y, sw, body_h)),
                Some(R::new(sw, body_y, seam, body_h)),
                sw + seam,
            )
        } else {
            (None, None, 0.0)
        };
        let avail = (w - x0).max(0.0);
        let (list, chain, chain_seam, chain_right) = if st.set.show_chain {
            let cw = k(chain_w).clamp(k(geo::CHAIN_MIN), (avail * 0.6).max(k(geo::CHAIN_MIN)));
            if avail - cw - seam >= k(geo::LIST_MIN) {
                (
                    R::new(x0, body_y, avail - cw - seam, body_h),
                    Some(R::new(w - cw, body_y, cw, body_h)),
                    Some(R::new(w - cw - seam, body_y, seam, body_h)),
                    true,
                )
            } else {
                let ch = (body_h * 0.4)
                    .clamp(k(110.0).min(body_h * 0.6), body_h * 0.6)
                    .round();
                (
                    R::new(x0, body_y, avail, (body_h - ch - seam).max(0.0)),
                    Some(R::new(x0, h - ch, avail, ch)),
                    Some(R::new(x0, h - ch - seam, avail, seam)),
                    false,
                )
            }
        } else {
            (R::new(x0, body_y, avail, body_h), None, None, false)
        };
        Layout {
            s,
            w,
            h,
            search,
            dice,
            count,
            menu,
            chips,
            side,
            side_seam,
            list,
            chain,
            chain_seam,
            chain_right,
            row: k(geo::ROW),
            side_row: k(geo::SIDE_ROW),
            chain_head: k(geo::CHAIN_HEAD),
            chain_row: k(geo::CHAIN_ROW),
        }
    }

    /// La rangee de la chaine ou se poserait un lacher en `y` (0..=n).
    fn fx_slot(&self, l: &Layout, y: f32) -> usize {
        let Some(c) = l.chain else { return 0 };
        let y0 = c.y + l.chain_head - self.chain_scroll;
        let k = ((y - y0 + l.chain_row * 0.5) / l.chain_row).floor();
        (k.max(0.0) as usize).min(self.nfx)
    }

    /// Ce qui est sous le point (x, y).
    pub fn hit(&self, th: &Theme, st: &Store, x: f32, y: f32) -> Hit {
        let (w, h) = self.size;
        let l = self.layout(th, st, w, h);
        // L'onglet passe avant ce qu'il couvre (il est dessine par-dessus).
        if self.on_tab(x, y) {
            return Hit::DockHandle;
        }
        if y < l.chips.y {
            if l.search.has(x, y) {
                let clear = !self.search.text.is_empty() && x >= l.search.right() - l.search.h;
                return if clear { Hit::SearchClear } else { Hit::Search };
            }
            if l.dice.has(x, y) {
                return Hit::Dice;
            }
            if l.count.has(x, y) {
                return Hit::Count;
            }
            if l.menu.has(x, y) {
                return Hit::Menu;
            }
            return Hit::None;
        }
        if l.chips.has(x, y) {
            let cx = x + self.chip_scroll;
            for (i, &(px, pw)) in self.chip_x.iter().enumerate() {
                if cx >= px && cx < px + pw {
                    return Hit::Chip(i);
                }
            }
            if cx >= self.chip_add.0 && cx < self.chip_add.0 + self.chip_add.1 {
                return Hit::ChipAdd;
            }
            return Hit::None;
        }
        if l.side_seam.is_some_and(|r| r.has(x, y)) {
            return Hit::SideSeam;
        }
        if let Some(r) = l.chain_seam.filter(|r| r.has(x, y)) {
            let _ = r;
            return if l.chain_right {
                Hit::ChainSeam
            } else {
                Hit::None
            };
        }
        if let Some(sd) = l.side.filter(|r| r.has(x, y)) {
            let k = ((y - sd.y + self.side_scroll) / l.side_row).floor();
            if k >= 0.0 {
                return match self.side.get(k as usize) {
                    Some(SideEntry::Head(g)) => Hit::Group(*g),
                    Some(SideEntry::Item(..)) => Hit::Side(k as usize),
                    None => Hit::None,
                };
            }
            return Hit::None;
        }
        if let Some(c) = l.chain.filter(|r| r.has(x, y)) {
            if y < c.y + l.chain_head {
                return Hit::ChainHead;
            }
            let k = ((y - c.y - l.chain_head + self.chain_scroll) / l.chain_row).floor();
            if k < 0.0 || k as usize >= self.nfx {
                return Hit::ChainEmpty;
            }
            let i = k as usize;
            let isz = (l.chain_row - (6.0 * l.s).round()).max(8.0);
            let del_x = c.right() - (6.0 * l.s).round() - isz;
            let pow_x = del_x - (4.0 * l.s).round() - isz;
            if x >= del_x {
                return Hit::FxDelete(i);
            }
            if x >= pow_x {
                return Hit::FxPower(i);
            }
            return Hit::Fx(i);
        }
        if l.list.has(x, y) {
            let k = ((y - l.list.y + self.scroll) / l.row).floor();
            if k < 0.0 || k as usize >= self.rows.len() {
                return Hit::None;
            }
            let r = k as usize;
            if x < l.list.x + (geo::STAR * l.s).round() {
                return Hit::Star(r);
            }
            return Hit::Row(r);
        }
        Hit::None
    }

    fn reveal(&mut self, r: usize) {
        let (w, h) = self.size;
        if h <= 0.0 {
            return;
        }
        let row = (geo::ROW * self.scale).round();
        let top = r as f32 * row;
        // La hauteur de la liste n'est connue qu'avec le theme : celle de la
        // derniere image.
        let view = self.list_h.max(row);
        let _ = w;
        if top < self.scroll {
            self.scroll = top;
        } else if top + row > self.scroll + view {
            self.scroll = top + row - view;
        }
    }

    fn clamp_scrolls(&mut self, l: &Layout) {
        let max = (self.rows.len() as f32 * l.row - l.list.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
        if let Some(sd) = l.side {
            let max = (self.side.len() as f32 * l.side_row - sd.h).max(0.0);
            self.side_scroll = self.side_scroll.clamp(0.0, max);
        }
        if let Some(c) = l.chain {
            let max = (self.nfx as f32 * l.chain_row - (c.h - l.chain_head)).max(0.0);
            self.chain_scroll = self.chain_scroll.clamp(0.0, max);
        }
        let end = self.chip_add.0 + self.chip_add.1 + (6.0 * l.s).round();
        self.chip_scroll = self.chip_scroll.clamp(0.0, (end - l.w).max(0.0));
    }

    /// Ce qu'explique l'infobulle.
    pub fn label(&self, cat: &Catalog, st: &Store, chain: &Chain, hit: Hit) -> String {
        match hit {
            Hit::None => String::new(),
            Hit::DockHandle => cp_ui::handle::HANDLE_TIP.into(),
            Hit::Search => {
                "Search: every word must match \u{00b7} -word excludes \u{00b7} Tab: next list"
                    .into()
            }
            Hit::SearchClear => "Clear the search (Esc)".into(),
            Hit::Dice => format!(
                "Dice: {} random effects from this list, {} the chain \u{00b7} wheel: how many \u{00b7} right-click: options",
                st.set.dice_count,
                if st.set.dice_replace {
                    "replacing"
                } else {
                    "added to"
                }
            ),
            Hit::Count => "How many effects the dice draws (wheel)".into(),
            Hit::Menu => "Options".into(),
            Hit::Chip(i) => match self.chips.get(i) {
                Some(Src::Tab(n)) => {
                    let rule = st.tab(n).and_then(|t| t.rule.as_ref());
                    match rule {
                        Some(r) => format!(
                            "{n}: \u{00ab}{}\u{00bb} in {} \u{00b7} right-click: rename, edit, delete",
                            r.query,
                            Src::from_key(&r.src).unwrap_or_default().name()
                        ),
                        None => format!(
                            "{n} \u{00b7} drop effects here to add them \u{00b7} right-click: rename, delete"
                        ),
                    }
                }
                Some(s) => s.name(),
                None => String::new(),
            },
            Hit::ChipAdd => "New tab".into(),
            Hit::Group(g) => GROUPS.get(g).map_or(String::new(), |s| {
                if g == 0 {
                    format!("{s} (REAPER's FX folders)")
                } else {
                    (*s).to_string()
                }
            }),
            Hit::Side(k) => match self.side.get(k) {
                Some(SideEntry::Item(s, n)) => format!("{} ({n})", s.name()),
                _ => String::new(),
            },
            Hit::SideSeam | Hit::ChainSeam => "Drag: resize".into(),
            Hit::Row(r) => match self.rows.get(r).and_then(|&i| cat.plugins.get(i)) {
                Some(p) => {
                    let mut s = p.name.clone();
                    if !p.cats.is_empty() {
                        s.push_str(&format!(" \u{00b7} {}", p.cats.join(", ")));
                    }
                    s.push_str(
                        "\nDouble-click or Enter: add \u{00b7} drag: onto the chain, a track, a tab \u{00b7} right-click: more",
                    );
                    s
                }
                None => String::new(),
            },
            Hit::Star(r) => match self.rows.get(r) {
                Some(&i) if st.is_fav(cat, i) => "Remove from favorites".into(),
                Some(_) => "Add to favorites".into(),
                None => String::new(),
            },
            Hit::ChainHead => {
                if chain.new_track {
                    "The next effect goes on a new track (Esc: back to the selected track)".into()
                } else if chain.track.is_some() {
                    "The selected track's FX chain".into()
                } else {
                    "Select a track".into()
                }
            }
            Hit::Fx(i) => match chain.fx.get(i) {
                Some(f) => format!(
                    "{}{}\nDouble-click: open \u{00b7} drag: move (Ctrl: copy) \u{00b7} Shift+click: bypass \u{00b7} Alt+click: delete",
                    f.name,
                    if f.detached {
                        "\nDetached: FX Constellation's ROLL and the dice leave it alone"
                    } else {
                        ""
                    }
                ),
                None => String::new(),
            },
            Hit::FxPower(i) => match chain.fx.get(i) {
                Some(f) if f.on => "Bypass".into(),
                Some(_) => "Enable".into(),
                None => String::new(),
            },
            Hit::FxDelete(_) => "Delete".into(),
            Hit::ChainEmpty => String::new(),
        }
    }

    /// Un geste. `text` : les mesures du texte (le clic dans la recherche).
    pub fn event(
        &mut self,
        cat: &Catalog,
        st: &mut Store,
        chain: &Chain,
        th: &Theme,
        text: &mut TextCache,
        ev: &Ev,
    ) -> Option<Act> {
        if self.pop.is_some() {
            match *ev {
                // Le presse-papiers d'une saisie passe par `key`.
                Ev::Key { vk, mods, .. } => return self.key(cat, st, vk, mods),
                Ev::Move { x, y, .. } => self.mouse = (x, y),
                _ => {}
            }
            return self.pop_event(ev);
        }
        self.nfx = chain.fx.len();
        let (w, h) = self.size;
        let l = self.layout(th, st, w, h);
        match *ev {
            Ev::Move { x, y, mods } => {
                self.mouse = (x, y);
                self.ctrl = mods.ctrl;
                let hit = self.hit(th, st, x, y);
                if hit != self.hover {
                    self.hover = hit;
                    self.mark();
                }
                let (w, h) = self.size;
                if self.dock.track(w, h, self.scale, x, y) {
                    self.mark();
                }
                self.drag_move(&l, x, y)
            }
            Ev::Leave => {
                if self.hover != Hit::None || self.dock.hot.is_some() {
                    self.hover = Hit::None;
                    self.dock.hot = None;
                    self.mark();
                }
                None
            }
            Ev::Down {
                x,
                y,
                b: Button::Left,
                dbl,
                mods,
            } => self.down(cat, st, &l, th, text, x, y, dbl, mods),
            Ev::Down {
                x,
                y,
                b: Button::Right,
                ..
            } => self.right_down(cat, st, th, x, y),
            Ev::Up {
                x,
                y,
                b: Button::Left,
                mods,
            } => self.up(cat, st, &l, x, y, mods),
            Ev::Wheel { x, y, dx, dy, .. } => self.wheel(st, &l, x, y, dx, dy),
            Ev::Key { vk, mods, .. } => self.key(cat, st, vk, mods),
            Ev::Char(c) => self.char(cat, st, c),
            _ => None,
        }
    }

    fn drag_move(&mut self, l: &Layout, x: f32, y: f32) -> Option<Act> {
        let s = l.s;
        let step = (4.0 * s).round();
        match self.drag.as_mut()? {
            Drag::Rows { x0, y0, moved, .. } => {
                if !*moved && ((x - *x0).abs() > step || (y - *y0).abs() > step) {
                    *moved = true;
                }
                self.mark();
            }
            Drag::Fx { y0, moved, .. } => {
                if !*moved && (y - *y0).abs() > step {
                    *moved = true;
                }
                self.mark();
            }
            Drag::SideSeam { .. } | Drag::ChainSeam { .. } => {
                self.mark();
            }
            Drag::Handle { x0, y0 } => {
                let d = cp_ui::handle::HANDLE_DRAG_PX * s;
                if (x - *x0).abs() > d || (y - *y0).abs() > d {
                    self.drag = None;
                    return Some(Act::DockGrab);
                }
            }
        }
        None
    }

    fn down(
        &mut self,
        cat: &Catalog,
        st: &mut Store,
        l: &Layout,
        th: &Theme,
        text: &mut TextCache,
        x: f32,
        y: f32,
        dbl: bool,
        mods: Mods,
    ) -> Option<Act> {
        let hit = self.hit(th, st, x, y);
        self.hover = hit;
        self.mark();
        if hit != Hit::Search && hit != Hit::SearchClear {
            // Un clic dans la liste rend Espace a REAPER ; la frappe d'une
            // lettre revient a la recherche.
            self.focus_search = matches!(hit, Hit::Chip(_) | Hit::ChipAdd | Hit::Dice);
        }
        match hit {
            Hit::DockHandle => {
                let (w, h) = self.size;
                if let Some(cp_ui::handle::Press::Switch(i)) =
                    self.dock.press(w, h, self.scale, x, y)
                {
                    return Some(Act::DockSwitch(i));
                }
                self.drag = Some(Drag::Handle { x0: x, y0: y });
                None
            }
            Hit::Search => {
                self.focus_search = true;
                let pad = (6.0 * l.s).round();
                if dbl {
                    self.search.select_all();
                } else {
                    self.search
                        .click(text, th.font(), x - l.search.x - pad, mods.shift);
                }
                None
            }
            Hit::SearchClear => {
                self.focus_search = true;
                self.search.clear();
                self.refresh(cat, st);
                None
            }
            Hit::Dice => Some(Act::Dice),
            Hit::Count => Some(Act::DiceMenu { x, y }),
            Hit::Menu => Some(Act::Menu { x, y }),
            Hit::Chip(i) => {
                let src = self.chips.get(i)?.clone();
                self.set_source(cat, st, src);
                Some(Act::Changed)
            }
            Hit::ChipAdd => Some(Act::AddTabMenu { x, y }),
            Hit::Group(g) => {
                if let Some(o) = st.set.open_groups.get_mut(g) {
                    *o = !*o;
                }
                self.side = side_entries(cat, st);
                Some(Act::Changed)
            }
            Hit::Side(k) => {
                let Some(SideEntry::Item(src, _)) = self.side.get(k).cloned() else {
                    return None;
                };
                self.set_source(cat, st, src);
                Some(Act::Changed)
            }
            Hit::SideSeam => {
                self.drag = Some(Drag::SideSeam {
                    x0: x,
                    w0: st.set.side_w,
                });
                None
            }
            Hit::ChainSeam => {
                self.drag = Some(Drag::ChainSeam {
                    x0: x,
                    w0: st.set.chain_w,
                });
                None
            }
            Hit::Star(r) => {
                let i = *self.rows.get(r)?;
                Some(Act::ToggleFav(i))
            }
            Hit::Row(r) => {
                let i = *self.rows.get(r)?;
                if dbl {
                    if !self.sel.contains(&i) {
                        self.sel.clear();
                        self.sel.insert(i);
                    }
                    self.drag = None;
                    return Some(Act::Add {
                        plugins: self.selection(),
                        at: None,
                    });
                }
                let mut reduce = None;
                if mods.shift {
                    let a = self.anchor.unwrap_or(r);
                    let (lo, hi) = (a.min(r), a.max(r));
                    if !mods.ctrl {
                        self.sel.clear();
                    }
                    for k in lo..=hi {
                        if let Some(&p) = self.rows.get(k) {
                            self.sel.insert(p);
                        }
                    }
                } else if mods.ctrl {
                    if !self.sel.remove(&i) {
                        self.sel.insert(i);
                    }
                    self.anchor = Some(r);
                } else if self.sel.contains(&i) {
                    reduce = Some(r);
                } else {
                    self.sel.clear();
                    self.sel.insert(i);
                    self.anchor = Some(r);
                }
                self.cursor = Some(r);
                if self.sel.contains(&i) {
                    self.drag = Some(Drag::Rows {
                        x0: x,
                        y0: y,
                        moved: false,
                        reduce,
                    });
                }
                None
            }
            Hit::Fx(i) => {
                if mods.alt {
                    return Some(Act::FxDelete(i));
                }
                if mods.shift {
                    return Some(Act::FxToggle(i));
                }
                if dbl {
                    return Some(Act::FxOpen(i));
                }
                self.fx_sel = Some(i);
                self.drag = Some(Drag::Fx {
                    from: i,
                    y0: y,
                    moved: false,
                });
                None
            }
            Hit::FxPower(i) => Some(Act::FxToggle(i)),
            Hit::FxDelete(i) => Some(Act::FxDelete(i)),
            Hit::ChainHead | Hit::ChainEmpty | Hit::None => {
                if hit == Hit::ChainEmpty {
                    self.fx_sel = None;
                }
                None
            }
        }
    }

    fn right_down(
        &mut self,
        cat: &Catalog,
        st: &mut Store,
        th: &Theme,
        x: f32,
        y: f32,
    ) -> Option<Act> {
        let hit = self.hit(th, st, x, y);
        let _ = cat;
        match hit {
            Hit::DockHandle => Some(Act::ViewMenu { x, y }),
            Hit::Row(r) | Hit::Star(r) => {
                let i = *self.rows.get(r)?;
                if !self.sel.contains(&i) {
                    self.sel.clear();
                    self.sel.insert(i);
                    self.anchor = Some(r);
                    self.cursor = Some(r);
                }
                self.mark();
                Some(Act::RowMenu { x, y })
            }
            Hit::Chip(i) => match self.chips.get(i) {
                Some(Src::Tab(_)) => Some(Act::ChipMenu { chip: i, x, y }),
                _ => None,
            },
            Hit::ChipAdd => Some(Act::AddTabMenu { x, y }),
            Hit::Dice | Hit::Count => Some(Act::DiceMenu { x, y }),
            Hit::Menu => Some(Act::Menu { x, y }),
            Hit::Fx(i) | Hit::FxPower(i) | Hit::FxDelete(i) => {
                self.fx_sel = Some(i);
                self.mark();
                Some(Act::FxMenu { fx: i, x, y })
            }
            _ => None,
        }
    }

    fn up(
        &mut self,
        cat: &Catalog,
        st: &mut Store,
        l: &Layout,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> Option<Act> {
        let d = self.drag.take()?;
        self.mark();
        let (w, h) = self.size;
        let outside = x < 0.0 || y < 0.0 || x >= w || y >= h;
        match d {
            Drag::Rows {
                moved: false,
                reduce,
                ..
            } => {
                if let Some(r) = reduce {
                    if let Some(&i) = self.rows.get(r) {
                        self.sel.clear();
                        self.sel.insert(i);
                        self.anchor = Some(r);
                    }
                }
                None
            }
            Drag::Rows { moved: true, .. } => {
                let plugins = self.selection();
                if plugins.is_empty() {
                    return None;
                }
                if outside {
                    return Some(Act::DropOut { plugins });
                }
                if l.chain.is_some_and(|c| c.has(x, y)) {
                    return Some(Act::Add {
                        plugins,
                        at: Some(self.fx_slot(l, y)),
                    });
                }
                if l.chips.has(x, y) {
                    let cx = x + self.chip_scroll;
                    for (i, &(px, pw)) in self.chip_x.iter().enumerate() {
                        if cx >= px && cx < px + pw {
                            if let Some(Src::Tab(n)) = self.chips.get(i) {
                                return Some(Act::ToTab {
                                    tab: n.clone(),
                                    plugins,
                                });
                            }
                        }
                    }
                }
                None
            }
            Drag::Fx { moved: false, .. } => None,
            Drag::Fx { from, .. } => {
                if outside || !l.chain.is_some_and(|c| c.has(x, y)) {
                    return None;
                }
                let slot = self.fx_slot(l, y);
                let copy = mods.ctrl;
                // Lacher a sa propre place : rien.
                if !copy && (slot == from || slot == from + 1) {
                    return None;
                }
                Some(Act::FxMove {
                    from,
                    to: slot,
                    copy,
                })
            }
            Drag::SideSeam { x0, w0 } => {
                st.set.side_w = (w0 + (x - x0) / l.s).clamp(geo::SIDE_MIN, 400.0).round();
                let _ = cat;
                Some(Act::Changed)
            }
            Drag::ChainSeam { x0, w0 } => {
                st.set.chain_w = (w0 - (x - x0) / l.s).clamp(geo::CHAIN_MIN, 600.0).round();
                Some(Act::Changed)
            }
            Drag::Handle { .. } => None,
        }
    }

    /// La largeur d'une couture pendant son glisser (le dessin la suit).
    pub(crate) fn live_widths(&self, st: &Store) -> (f32, f32) {
        let (mx, _) = self.mouse;
        match self.drag {
            Some(Drag::SideSeam { x0, w0 }) => (
                (w0 + (mx - x0) / self.scale).clamp(geo::SIDE_MIN, 400.0),
                st.set.chain_w,
            ),
            Some(Drag::ChainSeam { x0, w0 }) => (
                st.set.side_w,
                (w0 - (mx - x0) / self.scale).clamp(geo::CHAIN_MIN, 600.0),
            ),
            _ => (st.set.side_w, st.set.chain_w),
        }
    }

    /// Le glisser d'effets en cours, au-dessus de la chaine : la place du
    /// lacher.
    pub(crate) fn drop_slot(&self, l: &Layout) -> Option<usize> {
        let (x, y) = self.mouse;
        let over = l.chain.is_some_and(|c| c.has(x, y));
        match self.drag {
            Some(Drag::Rows { moved: true, .. }) | Some(Drag::Fx { moved: true, .. }) if over => {
                Some(self.fx_slot(l, y))
            }
            _ => None,
        }
    }

    /// Le glisser d'effets au-dessus d'une pastille d'onglet.
    pub(crate) fn drop_chip(&self, l: &Layout) -> Option<usize> {
        if !self.dragging_rows() {
            return None;
        }
        let (x, y) = self.mouse;
        if !l.chips.has(x, y) {
            return None;
        }
        let cx = x + self.chip_scroll;
        self.chip_x
            .iter()
            .position(|&(px, pw)| cx >= px && cx < px + pw)
            .filter(|&i| matches!(self.chips.get(i), Some(Src::Tab(_))))
    }

    pub(crate) fn fx_dragged(&self) -> Option<usize> {
        match self.drag {
            Some(Drag::Fx {
                from, moved: true, ..
            }) => Some(from),
            _ => None,
        }
    }

    fn wheel(
        &mut self,
        st: &mut Store,
        l: &Layout,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Option<Act> {
        self.mark();
        if l.dice.has(x, y) || l.count.has(x, y) {
            let n = (st.set.dice_count as i32 + dy.signum() as i32).clamp(1, 12);
            if n as u32 != st.set.dice_count {
                st.set.dice_count = n as u32;
                return Some(Act::Changed);
            }
            return None;
        }
        if l.chips.has(x, y) {
            let d = if dx != 0.0 { dx } else { -dy };
            self.chip_scroll += d * (40.0 * l.s).round();
        } else if l.side.is_some_and(|r| r.has(x, y)) {
            self.side_scroll -= dy * 3.0 * l.side_row;
        } else if l.chain.is_some_and(|r| r.has(x, y)) {
            self.chain_scroll -= dy * 2.0 * l.chain_row;
        } else if l.list.has(x, y) {
            self.scroll -= dy * 3.0 * l.row;
        }
        self.clamp_scrolls(l);
        None
    }

    /// Ce que le module fait de la touche `vk`.
    pub fn key_use(&self, vk: u32, m: Mods) -> KeyUse {
        if let Some(p) = &self.pop {
            return match p.key_route(vk, m) {
                cp_ui::pop::KeyRoute::Eat => KeyUse::Eat,
                cp_ui::pop::KeyRoute::Text => KeyUse::Text,
                cp_ui::pop::KeyRoute::Pass => KeyUse::NotMine,
            };
        }
        match vk {
            vk::UP | vk::DOWN | vk::PRIOR | vk::NEXT | vk::RETURN | vk::ESCAPE | vk::TAB => {
                KeyUse::Eat
            }
            0x46 if m.ctrl => KeyUse::Eat, // Ctrl+F
            0x56 if m.ctrl => {
                if self.focus_search {
                    KeyUse::Eat
                } else {
                    KeyUse::NotMine
                }
            }
            _ if m.ctrl || m.alt || m.win => {
                if self.focus_search
                    && matches!(vk, vk::LEFT | vk::RIGHT | vk::BACK | vk::DELETE | 0x41)
                {
                    KeyUse::Text
                } else {
                    KeyUse::NotMine
                }
            }
            vk::SPACE => {
                if self.focus_search {
                    KeyUse::Text
                } else {
                    KeyUse::NotMine
                }
            }
            vk::BACK => KeyUse::Text,
            vk::LEFT | vk::RIGHT | vk::HOME | vk::END | vk::DELETE => {
                if self.focus_search {
                    KeyUse::Text
                } else {
                    KeyUse::NotMine
                }
            }
            // Lettres, chiffres, ponctuation : la recherche.
            0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF => KeyUse::Text,
            _ => KeyUse::NotMine,
        }
    }

    fn move_cursor(&mut self, to: usize, shift: bool) {
        if self.rows.is_empty() {
            return;
        }
        let to = to.min(self.rows.len() - 1);
        self.cursor = Some(to);
        if shift {
            let a = self.anchor.unwrap_or(to);
            self.sel.clear();
            for k in a.min(to)..=a.max(to) {
                self.sel.insert(self.rows[k]);
            }
        } else {
            self.sel.clear();
            self.sel.insert(self.rows[to]);
            self.anchor = Some(to);
        }
        self.reveal(to);
        self.mark();
    }

    pub fn key(&mut self, cat: &Catalog, st: &mut Store, k: u32, m: Mods) -> Option<Act> {
        if let Some(p) = &self.pop {
            if p.wants_text() && m.ctrl && !m.alt && k == 0x56 {
                return Some(Act::Paste);
            }
            return self.pop_event(&Ev::Key {
                vk: k,
                mods: m,
                text: None,
            });
        }
        let page = ((self.list_h / (geo::ROW * self.scale).round()).floor() as usize).max(1);
        let cur = self.cursor;
        match k {
            vk::DOWN => {
                let to = cur.map_or(0, |c| c + 1);
                self.move_cursor(to, m.shift);
                None
            }
            vk::UP => {
                let to = cur.map_or(0, |c| c.saturating_sub(1));
                self.move_cursor(to, m.shift);
                None
            }
            vk::NEXT => {
                let to = cur.map_or(0, |c| c + page);
                self.move_cursor(to, m.shift);
                None
            }
            vk::PRIOR => {
                let to = cur.map_or(0, |c| c.saturating_sub(page));
                self.move_cursor(to, m.shift);
                None
            }
            vk::RETURN => {
                if self.sel.is_empty() {
                    if let Some(&first) = self.rows.first() {
                        self.sel.insert(first);
                    }
                }
                let plugins = self.selection();
                (!plugins.is_empty()).then_some(Act::Add { plugins, at: None })
            }
            vk::ESCAPE => {
                if !self.search.text.is_empty() {
                    self.search.clear();
                    self.refresh(cat, st);
                } else {
                    self.clear_selection();
                }
                None
            }
            vk::TAB => {
                let n = self.chips.len();
                if n == 0 {
                    return None;
                }
                let i = self.chips.iter().position(|c| *c == self.src);
                let next = match (i, m.shift) {
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                    (None, false) => 0,
                    (None, true) => n - 1,
                };
                let src = self.chips[next].clone();
                self.set_source(cat, st, src);
                Some(Act::Changed)
            }
            0x46 if m.ctrl => {
                self.focus_search = true;
                self.search.select_all();
                self.mark();
                None
            }
            0x56 if m.ctrl => Some(Act::Paste),
            _ => {
                if !self.focus_search && k != vk::BACK {
                    return None;
                }
                self.focus_search = true;
                match self.search.key(k, m) {
                    Edit::Changed => self.refresh(cat, st),
                    Edit::Moved => self.mark(),
                    Edit::None => {}
                }
                None
            }
        }
    }

    pub fn char(&mut self, cat: &Catalog, st: &mut Store, c: char) -> Option<Act> {
        if self.pop.is_some() {
            return self.pop_event(&Ev::Char(c));
        }
        if c.is_control() {
            return None;
        }
        self.focus_search = true;
        if self.search.char(c) == Edit::Changed {
            self.scroll = 0.0;
            self.refresh(cat, st);
        }
        None
    }

    /// Colle du texte dans la recherche.
    pub fn paste(&mut self, cat: &Catalog, st: &Store, s: &str) {
        if let Some(p) = self.pop.as_mut() {
            if p.wants_text() {
                p.paste(s);
                self.mark();
            }
            return;
        }
        let one_line: String = s.lines().next().unwrap_or("").to_string();
        self.focus_search = true;
        if self.search.insert(&one_line) == Edit::Changed {
            self.refresh(cat, st);
        }
    }
}

/// Les entrees de la barre de cote : les groupes, et ce qu'ont les groupes
/// ouverts.
pub fn side_entries(cat: &Catalog, st: &Store) -> Vec<SideEntry> {
    let mut v = Vec::new();
    for g in 0..GROUPS.len() {
        v.push(SideEntry::Head(g));
        if !st.set.open_groups.get(g).copied().unwrap_or(false) {
            continue;
        }
        match g {
            0 => {
                for f in &cat.folders {
                    v.push(SideEntry::Item(Src::Folder(f.name.clone()), f.items.len()));
                }
            }
            1 => {
                for (n, items) in &cat.cats {
                    v.push(SideEntry::Item(Src::Cat(n.clone()), items.len()));
                }
            }
            2 => {
                for (n, items) in &cat.devs {
                    v.push(SideEntry::Item(Src::Dev(n.clone()), items.len()));
                }
            }
            _ => {
                for (k, items) in &cat.kinds {
                    v.push(SideEntry::Item(Src::Kind(*k), items.len()));
                }
            }
        }
    }
    v
}
