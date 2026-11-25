//! Les popovers de CP Studio, dessines dans la vue, jamais un menu ou une
//! boite de Windows (regle de Cedric, 2026-11-25) : un menu (sous-menus,
//! coches, gestes), la saisie d'un nom, une confirmation, et le panneau de
//! reglages (des sections a gauche ; interrupteurs, choix segmentes,
//! curseurs, puces, compteurs, listes a cocher).
//!
//! Le popover ne sait rien de l'application. L'appelant le construit, lui
//! donne la taille de la vue et les mesures du texte ([`Pop::layout`]), le
//! dessine en dernier ([`Pop::draw`]), lui passe les evenements d'abord
//! ([`Pop::event`]) et recoit un [`Out`] qu'il applique. Rien ne bloque : un
//! menu de Windows tient le fil jusqu'au choix (et le module ne peut pas
//! s'appeler lui-meme pendant ce temps) ; celui-ci rend la main tout de
//! suite, le choix revient comme un geste.
//!
//! Tant qu'il est ouvert, il prend la souris et le clavier de la vue (un
//! clic dehors le ferme, comme un menu).

use std::collections::BTreeMap;

use cp_gfx::text::{Align, TextCache};
use cp_gfx::{Canvas, Color};

use crate::paint::{rline, rrect};
use crate::{Button, Ev, Icon, IconCache, TextField, Theme, vk};

/// Un rectangle de la vue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h * 0.5
    }
    fn shift(&self, dy: f32) -> R {
        R::new(self.x, self.y + dy, self.w, self.h)
    }
}

// ---------------------------------------------------------------------------
// Ce que l'appelant construit
// ---------------------------------------------------------------------------

/// Une ligne de menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item(Item),
    Sep,
    /// Un titre de groupe (discret, pas cliquable).
    Head(String),
    Sub {
        text: String,
        icon: Option<Icon>,
        entries: Vec<Entry>,
    },
}

/// Un choix de menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u32,
    pub text: String,
    pub icon: Option<Icon>,
    /// Une coche a droite.
    pub checked: bool,
    pub enabled: bool,
    /// Un geste qui detruit (supprimer) : rouge au survol.
    pub danger: bool,
    /// Le geste ou la touche qui fait la meme chose (a droite, discret).
    pub hint: String,
}

impl Entry {
    pub fn item(id: u32, text: impl Into<String>) -> Entry {
        Entry::Item(Item {
            id,
            text: text.into(),
            icon: None,
            checked: false,
            enabled: true,
            danger: false,
            hint: String::new(),
        })
    }
    pub fn check(id: u32, text: impl Into<String>, on: bool) -> Entry {
        let mut e = Entry::item(id, text);
        if let Entry::Item(i) = &mut e {
            i.checked = on;
        }
        e
    }
    pub fn head(text: impl Into<String>) -> Entry {
        Entry::Head(text.into())
    }
    pub fn sub(text: impl Into<String>, entries: Vec<Entry>) -> Entry {
        Entry::Sub {
            text: text.into(),
            icon: None,
            entries,
        }
    }
    /// Avec une icone devant.
    pub fn icon(mut self, ic: Icon) -> Entry {
        match &mut self {
            Entry::Item(i) => i.icon = Some(ic),
            Entry::Sub { icon, .. } => *icon = Some(ic),
            _ => {}
        }
        self
    }
    pub fn danger(mut self) -> Entry {
        if let Entry::Item(i) = &mut self {
            i.danger = true;
        }
        self
    }
    pub fn enabled(mut self, on: bool) -> Entry {
        if let Entry::Item(i) = &mut self {
            i.enabled = on;
        }
        self
    }
    pub fn hint(mut self, h: impl Into<String>) -> Entry {
        if let Entry::Item(i) = &mut self {
            i.hint = h.into();
        }
        self
    }
}

/// Une ligne du panneau de reglages.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// Un titre de groupe.
    Head(String),
    Toggle {
        id: u32,
        label: String,
        on: bool,
    },
    /// Un choix parmi peu (boutons segmentes).
    Seg {
        id: u32,
        label: String,
        opts: Vec<String>,
        cur: usize,
    },
    /// Un curseur, `v` en 0..1 ; `text` : la valeur montree.
    Slider {
        id: u32,
        label: String,
        v: f64,
        text: String,
    },
    /// Une etendue (deux poignees, 0..1).
    Range {
        id: u32,
        label: String,
        lo: f64,
        hi: f64,
        text: String,
    },
    /// Un compteur (- valeur +).
    Step {
        id: u32,
        label: String,
        text: String,
    },
    /// Des puces allumees ou non.
    Chips {
        id: u32,
        label: String,
        chips: Vec<(String, bool)>,
    },
    /// Une liste a cocher : (texte, coche, a droite discret). `add` : le
    /// texte d'invite d'une saisie pour ajouter (vide : pas de saisie).
    Checks {
        id: u32,
        items: Vec<(String, bool, String)>,
        add: String,
        removable: bool,
    },
}

/// Une section du panneau (un onglet du rail de gauche).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub label: String,
    pub icon: Icon,
    pub rows: Vec<Row>,
}

/// Le panneau de reglages.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub title: String,
    pub icon: Icon,
    pub sections: Vec<Section>,
}

/// Ou va une touche quand un popover est ouvert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    /// Au popover (la vue la prend, REAPER ne la voit pas).
    Eat,
    /// Du texte pour une saisie : la fenetre la recoit (WM_KEYDOWN puis
    /// WM_CHAR).
    Text,
    /// A REAPER (ses raccourcis restent actifs, comme sous un menu).
    Pass,
}

/// Ce que le popover rend a l'appelant.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Un choix de menu (le popover se ferme).
    Pick(u32),
    /// Le nom saisi (Entree ou le bouton ; le popover se ferme).
    Text(String),
    /// La confirmation (le popover se ferme).
    Yes,
    /// Ferme sans rien (Echap, un clic dehors, Annuler).
    Closed,
    // Le panneau (il reste ouvert ; l'appelant applique et le remet a jour
    // par `set_panel`).
    Toggle(u32),
    Seg(u32, usize),
    /// Un curseur bouge (0..1) ; `done` au relachement.
    Slide {
        id: u32,
        v: f64,
        done: bool,
    },
    Range {
        id: u32,
        lo: f64,
        hi: f64,
        done: bool,
    },
    /// Un pas du compteur (-1 ou +1).
    Step(u32, i32),
    Chip(u32, usize),
    Check(u32, usize),
    Remove(u32, usize),
    Add(u32, String),
}

impl Out {
    /// Le popover se ferme apres ce resultat.
    pub fn closes(&self) -> bool {
        matches!(self, Out::Pick(_) | Out::Text(_) | Out::Yes | Out::Closed)
    }
}

// ---------------------------------------------------------------------------
// L'etat
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    None,
    Item(usize, usize),
    Ok,
    Cancel,
    Field,
    Close,
    Rail(usize),
    Toggle(u32),
    Seg(u32, usize),
    Track(u32),
    Minus(u32),
    Plus(u32),
    Chip(u32, usize),
    Check(u32, usize),
    Remove(u32, usize),
    AddField(u32),
    AddBtn(u32),
}

#[derive(Clone, Debug)]
struct Level {
    entries: Vec<Entry>,
    r: R,
    rows: Vec<R>,
    hot: Option<usize>,
    /// Le choix du niveau d'avant qui a ouvert celui-ci.
    from: Option<usize>,
}

#[derive(Clone, Debug)]
struct PanelState {
    p: Panel,
    cur: usize,
    scroll: f32,
    content_h: f32,
    adds: BTreeMap<u32, TextField>,
    focus: Option<u32>,
    content: R,
}

#[derive(Clone, Debug)]
enum Kind {
    Menu(Vec<Level>),
    Ask {
        title: String,
        field: TextField,
        ok: String,
    },
    Confirm {
        title: String,
        ok: String,
        danger: bool,
    },
    Panel(Box<PanelState>),
}

/// Un geste de curseur en cours.
#[derive(Clone, Copy, Debug)]
struct Slide {
    id: u32,
    /// 0 : un curseur simple ; 1 : la poignee basse ; 2 : la haute.
    which: u8,
    track: R,
    other: f64,
}

/// Un popover ouvert.
#[derive(Clone, Debug)]
pub struct Pop {
    /// A quoi sert ce popover, pour l'appelant (rendu tel quel).
    pub ctx: u32,
    kind: Kind,
    at: (f32, f32),
    size: (f32, f32),
    /// Les corps dessines (un menu et ses sous-menus) : un clic hors de tous
    /// ferme.
    bodies: Vec<R>,
    hits: Vec<(R, Hit)>,
    hover: Hit,
    slide: Option<Slide>,
    /// Un bouton presse dans le popover depuis son ouverture (un menu ne
    /// choisit qu'au relachement d'un clic a lui, ou au bout d'un glisser).
    armed: bool,
    moved: bool,
    laid: bool,
    stamp: u32,
}

impl Pop {
    fn new(ctx: u32, kind: Kind, x: f32, y: f32) -> Pop {
        Pop {
            ctx,
            kind,
            at: (x, y),
            size: (0.0, 0.0),
            bodies: Vec::new(),
            hits: Vec::new(),
            hover: Hit::None,
            slide: None,
            armed: false,
            moved: false,
            laid: false,
            stamp: 1,
        }
    }

    /// Un menu, son coin haut-gauche en (x, y) (il se recale dans la vue).
    pub fn menu(ctx: u32, entries: Vec<Entry>, x: f32, y: f32) -> Pop {
        let lv = Level {
            entries,
            r: R::default(),
            rows: Vec::new(),
            hot: None,
            from: None,
        };
        Pop::new(ctx, Kind::Menu(vec![lv]), x, y)
    }

    /// La saisie d'un nom (le texte de depart tout choisi).
    pub fn ask(ctx: u32, title: &str, value: &str, ok: &str, x: f32, y: f32) -> Pop {
        let mut field = TextField::default();
        field.set(value);
        field.select_all();
        Pop::new(
            ctx,
            Kind::Ask {
                title: title.into(),
                field,
                ok: ok.into(),
            },
            x,
            y,
        )
    }

    /// Une confirmation (`danger` : le bouton est rouge).
    pub fn confirm(ctx: u32, title: &str, ok: &str, danger: bool, x: f32, y: f32) -> Pop {
        Pop::new(
            ctx,
            Kind::Confirm {
                title: title.into(),
                ok: ok.into(),
                danger,
            },
            x,
            y,
        )
    }

    /// Le panneau de reglages ; (x, y) : sous le bouton qui l'ouvre (le
    /// panneau s'aligne a sa droite).
    pub fn panel(ctx: u32, p: Panel, x: f32, y: f32) -> Pop {
        Pop::new(
            ctx,
            Kind::Panel(Box::new(PanelState {
                p,
                cur: 0,
                scroll: 0.0,
                content_h: 0.0,
                adds: BTreeMap::new(),
                focus: None,
                content: R::default(),
            })),
            x,
            y,
        )
    }

    /// Remplace le contenu du panneau (apres un reglage applique), en
    /// gardant la section, le defilement et les saisies.
    pub fn set_panel(&mut self, p: Panel) {
        if let Kind::Panel(st) = &mut self.kind {
            if st.p != p {
                st.cur = st.cur.min(p.sections.len().saturating_sub(1));
                st.p = p;
                self.stamp += 1;
            }
        }
    }

    /// La section montree du panneau.
    pub fn section(&self) -> Option<usize> {
        match &self.kind {
            Kind::Panel(st) => Some(st.cur),
            _ => None,
        }
    }

    pub fn set_section(&mut self, i: usize) {
        if let Kind::Panel(st) = &mut self.kind {
            st.cur = i.min(st.p.sections.len().saturating_sub(1));
            st.scroll = 0.0;
            self.stamp += 1;
        }
    }

    pub fn is_panel(&self) -> bool {
        matches!(self.kind, Kind::Panel(_))
    }

    /// Change a chaque fois que l'image du popover change.
    pub fn stamp(&self) -> u32 {
        self.stamp
    }

    /// Les rectangles qu'il couvre (la vue les repeint quand il change).
    pub fn rects(&self) -> &[R] {
        &self.bodies
    }

    /// Une saisie a le clavier (la vue rend les touches de texte au
    /// popover, pas a REAPER).
    pub fn wants_text(&self) -> bool {
        match &self.kind {
            Kind::Ask { .. } => true,
            Kind::Panel(st) => st.focus.is_some(),
            _ => false,
        }
    }

    /// Ou va une touche (la meme regle pour toutes les vues).
    pub fn key_route(&self, k: u32, m: crate::Mods) -> KeyRoute {
        let text = self.wants_text();
        match k {
            vk::ESCAPE | vk::RETURN | vk::TAB | vk::UP | vk::DOWN | vk::LEFT | vk::RIGHT => {
                KeyRoute::Eat
            }
            vk::SPACE if !text => KeyRoute::Eat,
            _ if !text => KeyRoute::Pass,
            // AltGr (Ctrl + Alt) : un caractere (@, #, [, ]...).
            _ if m.ctrl && m.alt && !m.win => KeyRoute::Text,
            // Les raccourcis d'une saisie : a nous, jamais a REAPER.
            0x41 | 0x43 | 0x56 | 0x58 | 0x5A if m.ctrl && !m.alt => KeyRoute::Eat,
            _ if m.ctrl || m.alt || m.win => KeyRoute::Pass,
            vk::HOME | vk::END | vk::BACK | vk::DELETE => KeyRoute::Eat,
            _ => KeyRoute::Text,
        }
    }

    /// Colle du texte dans la saisie qui a le clavier.
    pub fn paste(&mut self, s: &str) {
        let one = s.lines().next().unwrap_or("");
        let done = self.field_mut().map(|f| f.insert(one)).is_some();
        if done {
            self.stamp += 1;
        }
    }

    /// Le texte choisi de la saisie (Ctrl+C).
    pub fn selected_text(&self) -> String {
        match &self.kind {
            Kind::Ask { field, .. } => field.selected().to_string(),
            Kind::Panel(st) => st
                .focus
                .and_then(|id| st.adds.get(&id))
                .map(|f| f.selected().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn field_mut(&mut self) -> Option<&mut TextField> {
        match &mut self.kind {
            Kind::Ask { field, .. } => Some(field),
            Kind::Panel(st) => {
                let id = st.focus?;
                Some(st.adds.entry(id).or_default())
            }
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // La geometrie
    // -----------------------------------------------------------------------

    /// Calcule la geometrie pour une vue de `w` x `h` (a chaque image, avant
    /// le dessin ; les evenements la relisent).
    pub fn layout(&mut self, th: &Theme, text: &mut TextCache, w: f32, h: f32) {
        self.size = (w, h);
        self.bodies.clear();
        self.hits.clear();
        let m = th.s(6.0);
        match &mut self.kind {
            Kind::Menu(levels) => {
                for li in 0..levels.len() {
                    let (bw, bh, rows) = menu_size(th, text, &levels[li].entries);
                    let (x, y) = if li == 0 {
                        place(bw, bh, self.at.0, self.at.1, w, h, m)
                    } else {
                        let parent = &levels[li - 1];
                        let row = levels[li]
                            .from
                            .and_then(|k| parent.rows.get(k))
                            .copied()
                            .unwrap_or(parent.r);
                        let pad = th.s(5.0);
                        let right = parent.r.right() - th.s(3.0);
                        let x = if right + bw <= w - m {
                            right
                        } else {
                            (parent.r.x - bw + th.s(3.0)).max(m)
                        };
                        place(bw, bh, x, row.y - pad, w, h, m)
                    };
                    let lv = &mut levels[li];
                    lv.r = R::new(x, y, bw, bh);
                    lv.rows = rows.iter().map(|r| r.shift(y).with_x(x)).collect();
                    self.bodies.push(lv.r);
                    for (k, r) in lv.rows.iter().enumerate() {
                        let pickable = match &lv.entries[k] {
                            Entry::Item(i) => i.enabled,
                            Entry::Sub { .. } => true,
                            _ => false,
                        };
                        if pickable {
                            self.hits.push((*r, Hit::Item(li, k)));
                        }
                    }
                }
            }
            k @ (Kind::Ask { .. } | Kind::Confirm { .. }) => {
                let ask = matches!(k, Kind::Ask { .. });
                let bw = th.s(300.0).min(w - 2.0 * m);
                let bh = if ask { th.s(112.0) } else { th.s(92.0) };
                let (x, y) = place(bw, bh, self.at.0, self.at.1, w, h, m);
                let r = R::new(x, y, bw, bh);
                self.bodies.push(r);
                let p = th.s(14.0);
                let bh2 = th.s(28.0);
                let by = r.bottom() - p - bh2;
                let okw = th.s(84.0);
                let ok = R::new(r.right() - p - okw, by, okw, bh2);
                let cancel = R::new(ok.x - th.s(8.0) - okw, by, okw, bh2);
                self.hits.push((ok, Hit::Ok));
                self.hits.push((cancel, Hit::Cancel));
                if ask {
                    let f = R::new(x + p, y + p + th.s(20.0), bw - 2.0 * p, th.s(30.0));
                    self.hits.push((f, Hit::Field));
                }
            }
            Kind::Panel(st) => {
                let bw = th.s(660.0).min(w - 2.0 * m).max(th.s(240.0).min(w));
                let bh = th.s(600.0).min(h - 2.0 * m).max(th.s(160.0).min(h));
                let x = (self.at.0 + th.s(20.0) - bw).clamp(m, (w - bw - m).max(m));
                let y = self.at.1.clamp(m, (h - bh - m).max(m));
                let r = R::new(x, y, bw, bh);
                self.bodies.push(r);
                let head = th.s(44.0);
                let cx = R::new(
                    r.right() - th.s(36.0),
                    y + th.s(8.0),
                    th.s(28.0),
                    th.s(28.0),
                );
                self.hits.push((cx, Hit::Close));
                let compact = bw < th.s(480.0);
                let rail_w = if compact { th.s(48.0) } else { th.s(168.0) };
                let ih = th.s(34.0);
                for i in 0..st.p.sections.len() {
                    let rr = R::new(
                        x + th.s(8.0),
                        y + head + th.s(10.0) + i as f32 * (ih + th.s(2.0)),
                        rail_w - th.s(16.0),
                        ih,
                    );
                    self.hits.push((rr, Hit::Rail(i)));
                }
                let content = R::new(
                    x + rail_w + th.s(20.0),
                    y + head + th.s(14.0),
                    bw - rail_w - th.s(40.0),
                    bh - head - th.s(24.0),
                );
                st.content = content;
                let rows =
                    st.p.sections
                        .get(st.cur)
                        .map(|s| s.rows.clone())
                        .unwrap_or_default();
                let mut yy = 0.0;
                let mut hits = Vec::new();
                for row in &rows {
                    let rh = row_h(th, row);
                    let rr = R::new(content.x, content.y + yy - st.scroll, content.w, rh);
                    row_hits(th, text, row, rr, &mut hits);
                    yy += rh
                        + if matches!(row, Row::Head(_)) {
                            0.0
                        } else {
                            th.s(2.0)
                        };
                }
                st.content_h = yy;
                st.scroll = st.scroll.clamp(0.0, (yy - content.h).max(0.0));
                // Ce qui sort de la zone qui defile ne se touche pas.
                for (hr, hh) in hits {
                    if hr.y >= content.y - 1.0 && hr.bottom() <= content.bottom() + 1.0 {
                        self.hits.push((hr, hh));
                    }
                }
            }
        }
        self.laid = true;
    }

    fn hit(&self, x: f32, y: f32) -> Hit {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.has(x, y))
            .map_or(Hit::None, |(_, h)| *h)
    }

    fn inside(&self, x: f32, y: f32) -> bool {
        self.bodies.iter().any(|r| r.has(x, y))
    }

    // -----------------------------------------------------------------------
    // Les evenements
    // -----------------------------------------------------------------------

    /// Un evenement de la vue. Tant que le popover est ouvert, il les prend
    /// tous ; `Some` : ce que l'appelant doit appliquer (un [`Out`] qui
    /// `closes()` : il le retire).
    pub fn event(&mut self, ev: &Ev) -> Option<Out> {
        match ev {
            Ev::Move { x, y, .. } => self.mouse_move(*x, *y),
            Ev::Down { x, y, b, .. } => self.mouse_down(*x, *y, *b),
            Ev::Up { x, y, .. } => self.mouse_up(*x, *y),
            Ev::Wheel { x, y, dy, .. } => {
                if let Kind::Panel(st) = &mut self.kind {
                    if st.content.has(*x, *y) {
                        let max = (st.content_h - st.content.h).max(0.0);
                        st.scroll = (st.scroll - dy * 48.0).clamp(0.0, max);
                        self.stamp += 1;
                    }
                }
                None
            }
            Ev::Key { vk, mods, .. } => self.key(*vk, *mods),
            Ev::Char(c) => {
                let changed = !c.is_control()
                    && self
                        .field_mut()
                        .is_some_and(|f| f.char(*c) != crate::textfield::Edit::None);
                if changed {
                    self.stamp += 1;
                }
                None
            }
            Ev::Leave => {
                if self.hover != Hit::None && self.slide.is_none() {
                    self.hover = Hit::None;
                    self.stamp += 1;
                }
                None
            }
        }
    }

    fn mouse_move(&mut self, x: f32, y: f32) -> Option<Out> {
        if !self.laid {
            return None;
        }
        if let Some(s) = self.slide {
            let v = ((x - s.track.x) / s.track.w.max(1.0)).clamp(0.0, 1.0) as f64;
            self.stamp += 1;
            return Some(slide_out(s, v, false));
        }
        let dx = x - self.at.0;
        let dy = y - self.at.1;
        if dx * dx + dy * dy > 36.0 {
            self.moved = true;
        }
        let h = self.hit(x, y);
        if h != self.hover {
            self.hover = h;
            self.stamp += 1;
        }
        if let (Kind::Menu(levels), Hit::Item(li, k)) = (&mut self.kind, h) {
            // Le survol ouvre un sous-menu, ferme les niveaux plus profonds.
            if levels[li].hot != Some(k) {
                levels[li].hot = Some(k);
                levels.truncate(li + 1);
                if let Entry::Sub { entries, .. } = &levels[li].entries[k] {
                    let e = entries.clone();
                    levels.push(Level {
                        entries: e,
                        r: R::default(),
                        rows: Vec::new(),
                        hot: None,
                        from: Some(k),
                    });
                    // La geometrie du nouveau niveau vient au prochain
                    // `layout` (avant le dessin).
                    self.laid = false;
                }
                self.stamp += 1;
            }
        }
        None
    }

    fn mouse_down(&mut self, x: f32, y: f32, b: Button) -> Option<Out> {
        if !self.inside(x, y) {
            return Some(Out::Closed);
        }
        self.armed = true;
        if b != Button::Left && !matches!(self.kind, Kind::Menu(_)) {
            return None;
        }
        let h = self.hit(x, y);
        match (&mut self.kind, h) {
            (Kind::Ask { .. }, Hit::Field) => None,
            (Kind::Panel(st), _) => {
                st.focus = match h {
                    Hit::AddField(id) => Some(id),
                    _ => None,
                };
                self.stamp += 1;
                match h {
                    Hit::Close => Some(Out::Closed),
                    Hit::Rail(i) => {
                        st.cur = i;
                        st.scroll = 0.0;
                        None
                    }
                    Hit::Toggle(id) => Some(Out::Toggle(id)),
                    Hit::Seg(id, i) => Some(Out::Seg(id, i)),
                    Hit::Minus(id) => Some(Out::Step(id, -1)),
                    Hit::Plus(id) => Some(Out::Step(id, 1)),
                    Hit::Chip(id, i) => Some(Out::Chip(id, i)),
                    Hit::Check(id, i) => Some(Out::Check(id, i)),
                    Hit::Remove(id, i) => Some(Out::Remove(id, i)),
                    Hit::AddBtn(id) => {
                        let t = st
                            .adds
                            .get_mut(&id)
                            .map(|f| {
                                let t = f.text.trim().to_string();
                                f.clear();
                                t
                            })
                            .unwrap_or_default();
                        (!t.is_empty()).then_some(Out::Add(id, t))
                    }
                    Hit::Track(id) => {
                        let row = find_row(&st.p, st.cur, id)?;
                        let track = self
                            .hits
                            .iter()
                            .find(|(_, hh)| *hh == Hit::Track(id))
                            .map(|(r, _)| *r)?;
                        let v = ((x - track.x) / track.w.max(1.0)).clamp(0.0, 1.0) as f64;
                        let s = match row {
                            Row::Range { lo, hi, .. } => {
                                // La poignee la plus proche.
                                if (v - lo).abs() <= (v - hi).abs() {
                                    Slide {
                                        id,
                                        which: 1,
                                        track,
                                        other: *hi,
                                    }
                                } else {
                                    Slide {
                                        id,
                                        which: 2,
                                        track,
                                        other: *lo,
                                    }
                                }
                            }
                            _ => Slide {
                                id,
                                which: 0,
                                track,
                                other: 0.0,
                            },
                        };
                        self.slide = Some(s);
                        Some(slide_out(s, v, false))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn mouse_up(&mut self, x: f32, y: f32) -> Option<Out> {
        if let Some(s) = self.slide.take() {
            let v = ((x - s.track.x) / s.track.w.max(1.0)).clamp(0.0, 1.0) as f64;
            self.stamp += 1;
            return Some(slide_out(s, v, true));
        }
        let h = self.hit(x, y);
        match &mut self.kind {
            Kind::Menu(levels) => {
                // Le relachement du clic qui a ouvert le menu ne choisit
                // rien, sauf au bout d'un glisser.
                if !self.armed && !self.moved {
                    return None;
                }
                let Hit::Item(li, k) = h else { return None };
                match levels.get(li).and_then(|l| l.entries.get(k)) {
                    Some(Entry::Item(i)) if i.enabled => Some(Out::Pick(i.id)),
                    _ => None,
                }
            }
            Kind::Ask { field, .. } => match h {
                Hit::Ok => {
                    let t = field.text.trim().to_string();
                    (!t.is_empty()).then_some(Out::Text(t))
                }
                Hit::Cancel => Some(Out::Closed),
                _ => None,
            },
            Kind::Confirm { .. } => match h {
                Hit::Ok => Some(Out::Yes),
                Hit::Cancel => Some(Out::Closed),
                _ => None,
            },
            Kind::Panel(_) => None,
        }
    }

    fn key(&mut self, k: u32, mods: crate::Mods) -> Option<Out> {
        if k == vk::ESCAPE {
            // Un sous-menu se ferme d'abord.
            if let Kind::Menu(levels) = &mut self.kind {
                if levels.len() > 1 {
                    levels.pop();
                    self.stamp += 1;
                    return None;
                }
            }
            if let Kind::Panel(st) = &mut self.kind {
                if st.focus.take().is_some() {
                    self.stamp += 1;
                    return None;
                }
            }
            return Some(Out::Closed);
        }
        self.stamp += 1;
        match &mut self.kind {
            Kind::Menu(levels) => {
                let li = levels.len() - 1;
                let pickable: Vec<usize> = levels[li]
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| match e {
                        Entry::Item(i) => i.enabled,
                        Entry::Sub { .. } => true,
                        _ => false,
                    })
                    .map(|(i, _)| i)
                    .collect();
                if pickable.is_empty() {
                    return None;
                }
                let pos = levels[li]
                    .hot
                    .and_then(|h| pickable.iter().position(|&p| p == h));
                match k {
                    vk::DOWN => {
                        let n = pos.map_or(0, |p| (p + 1) % pickable.len());
                        levels[li].hot = Some(pickable[n]);
                    }
                    vk::UP => {
                        let n = pos.map_or(pickable.len() - 1, |p| {
                            (p + pickable.len() - 1) % pickable.len()
                        });
                        levels[li].hot = Some(pickable[n]);
                    }
                    vk::LEFT if li > 0 => {
                        levels.pop();
                    }
                    vk::RIGHT | vk::RETURN | vk::SPACE => {
                        let hot = levels[li].hot?;
                        match levels[li].entries[hot].clone() {
                            Entry::Item(i) if k != vk::RIGHT => return Some(Out::Pick(i.id)),
                            Entry::Sub { entries, .. } => {
                                let first = entries.iter().position(|e| {
                                    matches!(e, Entry::Item(i) if i.enabled)
                                        || matches!(e, Entry::Sub { .. })
                                });
                                levels.push(Level {
                                    entries,
                                    r: R::default(),
                                    rows: Vec::new(),
                                    hot: first,
                                    from: Some(hot),
                                });
                                self.laid = false;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                None
            }
            Kind::Ask { field, .. } => {
                if k == vk::RETURN {
                    let t = field.text.trim().to_string();
                    return (!t.is_empty()).then_some(Out::Text(t));
                }
                field.key(k, mods);
                None
            }
            Kind::Confirm { .. } => (k == vk::RETURN).then_some(Out::Yes),
            Kind::Panel(st) => {
                let id = st.focus?;
                let f = st.adds.entry(id).or_default();
                if k == vk::RETURN {
                    let t = f.text.trim().to_string();
                    f.clear();
                    return (!t.is_empty()).then_some(Out::Add(id, t));
                }
                f.key(k, mods);
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // Le dessin
    // -----------------------------------------------------------------------

    /// Dessine le popover (en dernier, par-dessus la vue ; dans la decoupe
    /// courante du canevas). [`Pop::layout`] d'abord.
    pub fn draw(
        &self,
        cv: &mut Canvas<'_>,
        th: &Theme,
        text: &mut TextCache,
        icons: &mut IconCache,
    ) {
        if !self.laid {
            return;
        }
        let p = Pal::of(th);
        for b in &self.bodies {
            shadow(cv, th, *b);
        }
        match &self.kind {
            Kind::Menu(levels) => {
                for (li, lv) in levels.iter().enumerate() {
                    body(cv, th, &p, lv.r);
                    // Une colonne d'icones : chaque texte s'aligne dessus.
                    let col = lv.entries.iter().any(|e| {
                        matches!(
                            e,
                            Entry::Item(Item { icon: Some(_), .. })
                                | Entry::Sub { icon: Some(_), .. }
                        )
                    });
                    for (k, (e, r)) in lv.entries.iter().zip(&lv.rows).enumerate() {
                        let hot = lv.hot == Some(k) || self.hover == Hit::Item(li, k);
                        draw_entry(cv, th, text, icons, &p, e, *r, hot, col);
                    }
                }
            }
            Kind::Ask { title, field, ok } => {
                let r = self.bodies[0];
                body(cv, th, &p, r);
                let pad = th.s(14.0);
                text.draw_in(
                    cv,
                    th.font_small(),
                    r.x + pad,
                    r.y + pad - th.s(2.0),
                    r.w - 2.0 * pad,
                    th.s(18.0),
                    title,
                    p.dim,
                    Align::Left,
                );
                if let Some((fr, _)) = self.hits.iter().find(|(_, h)| *h == Hit::Field) {
                    draw_field(cv, th, text, &p, field, *fr, true, "");
                }
                self.buttons(cv, th, text, &p, ok, false);
            }
            Kind::Confirm { title, ok, danger } => {
                let r = self.bodies[0];
                body(cv, th, &p, r);
                let pad = th.s(14.0);
                text.draw_in(
                    cv,
                    th.font_bold(),
                    r.x + pad,
                    r.y + pad,
                    r.w - 2.0 * pad,
                    th.s(22.0),
                    title,
                    p.text,
                    Align::Left,
                );
                self.buttons(cv, th, text, &p, ok, *danger);
            }
            Kind::Panel(st) => self.draw_panel(cv, th, text, icons, &p, st),
        }
    }

    fn buttons(
        &self,
        cv: &mut Canvas<'_>,
        th: &Theme,
        text: &mut TextCache,
        p: &Pal,
        ok: &str,
        danger: bool,
    ) {
        for (r, h) in &self.hits {
            let (label, bg, fg) = match h {
                Hit::Ok => (
                    ok,
                    if danger { th.error } else { p.accent },
                    if danger { p.on_danger } else { p.on_accent },
                ),
                Hit::Cancel => ("Cancel", p.raised, p.text),
                _ => continue,
            };
            let bg = if self.hover == *h {
                bg.mix(p.text, 0.12)
            } else {
                bg
            };
            rrect(cv, r.x, r.y, r.w, r.h, th.s(7.0), bg);
            if *h == Hit::Cancel {
                rline(cv, r.x, r.y, r.w, r.h, th.s(7.0), 1.0, p.line);
            }
            let f = if *h == Hit::Ok {
                th.font_bold()
            } else {
                th.font()
            };
            text.draw_in(cv, f, r.x, r.y, r.w, r.h, label, fg, Align::Center);
        }
    }

    fn draw_panel(
        &self,
        cv: &mut Canvas<'_>,
        th: &Theme,
        text: &mut TextCache,
        icons: &mut IconCache,
        p: &Pal,
        st: &PanelState,
    ) {
        let r = self.bodies[0];
        let rad = th.s(12.0);
        body(cv, th, p, r);
        let head = th.s(44.0);
        // L'en-tete : une bande un peu plus claire.
        if cv.push_clip(r.x, r.y, r.w, head) {
            rrect(cv, r.x + 1.0, r.y + 1.0, r.w - 2.0, head + rad, rad, p.head);
        }
        cv.pop_clip();
        cv.fill_rect(r.x + 1.0, r.y + head, r.w - 2.0, 1.0, p.line);
        let isz = th.s(16.0) as u16;
        icons.draw(
            cv,
            st.p.icon,
            r.x + th.s(16.0),
            r.y + (head - isz as f32) * 0.5,
            isz,
            p.accent,
            false,
        );
        text.draw_in(
            cv,
            th.font_bold(),
            r.x + th.s(42.0),
            r.y,
            r.w - th.s(90.0),
            head,
            &st.p.title,
            p.text,
            Align::Left,
        );
        if let Some((cr, _)) = self.hits.iter().find(|(_, h)| *h == Hit::Close) {
            if self.hover == Hit::Close {
                rrect(cv, cr.x, cr.y, cr.w, cr.h, th.s(6.0), p.hover);
            }
            icon_in(cv, icons, th, Icon::X, *cr, p.dim, th.s(14.0));
        }
        // Le rail.
        let rails: Vec<(R, usize)> = self
            .hits
            .iter()
            .filter_map(|(rr, h)| match h {
                Hit::Rail(i) => Some((*rr, *i)),
                _ => None,
            })
            .collect();
        let rail_w = rails.first().map_or(0.0, |(rr, _)| rr.w + th.s(16.0));
        cv.fill_rect(
            r.x + 1.0,
            r.y + head + 1.0,
            rail_w - 1.0,
            r.h - head - 2.0,
            p.rail,
        );
        cv.fill_rect(
            r.x + rail_w,
            r.y + head + 1.0,
            1.0,
            r.h - head - 2.0,
            p.line,
        );
        let compact = rail_w < th.s(100.0);
        for (rr, i) in rails {
            let Some(s) = st.p.sections.get(i) else {
                continue;
            };
            let on = i == st.cur;
            if on {
                rrect(cv, rr.x, rr.y, rr.w, rr.h, th.s(8.0), p.raised);
            } else if self.hover == Hit::Rail(i) {
                rrect(cv, rr.x, rr.y, rr.w, rr.h, th.s(8.0), p.hover);
            }
            let ic = if compact {
                R::new(rr.x, rr.y, rr.w, rr.h)
            } else {
                R::new(rr.x + th.s(4.0), rr.y, th.s(24.0), rr.h)
            };
            icon_in(
                cv,
                icons,
                th,
                s.icon,
                ic,
                if on { p.accent } else { p.dim },
                th.s(16.0),
            );
            if !compact {
                text.draw_in(
                    cv,
                    th.font(),
                    rr.x + th.s(34.0),
                    rr.y,
                    rr.w - th.s(38.0),
                    rr.h,
                    &s.label,
                    if on { p.text } else { p.dim },
                    Align::Left,
                );
            }
        }
        // Le contenu.
        let c = st.content;
        if !cv.push_clip(
            c.x - th.s(4.0),
            c.y - th.s(4.0),
            c.w + th.s(8.0),
            c.h + th.s(8.0),
        ) {
            cv.pop_clip();
            return;
        }
        let rows =
            st.p.sections
                .get(st.cur)
                .map(|s| &s.rows[..])
                .unwrap_or(&[]);
        let mut yy = 0.0;
        for row in rows {
            let rh = row_h(th, row);
            let rr = R::new(c.x, c.y + yy - st.scroll, c.w, rh);
            if rr.bottom() >= c.y - th.s(4.0) && rr.y <= c.bottom() + th.s(4.0) {
                self.draw_row(cv, th, text, icons, p, st, row, rr);
            }
            yy += rh
                + if matches!(row, Row::Head(_)) {
                    0.0
                } else {
                    th.s(2.0)
                };
        }
        cv.pop_clip();
        // La barre de defilement.
        if st.content_h > c.h + 1.0 {
            let k = c.h / st.content_h;
            let th_h = (c.h * k).max(th.s(24.0));
            let ty = c.y + (c.h - th_h) * (st.scroll / (st.content_h - c.h).max(1.0));
            rrect(
                cv,
                r.right() - th.s(8.0),
                ty,
                th.s(4.0),
                th_h,
                th.s(2.0),
                p.line,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &self,
        cv: &mut Canvas<'_>,
        th: &Theme,
        text: &mut TextCache,
        icons: &mut IconCache,
        p: &Pal,
        st: &PanelState,
        row: &Row,
        r: R,
    ) {
        let f = th.font();
        let label = |cv: &mut Canvas<'_>, text: &mut TextCache, s: &str| {
            text.draw_in(cv, f, r.x, r.y, r.w * 0.5, r.h, s, p.text, Align::Left);
        };
        match row {
            Row::Head(s) => {
                text.draw_in(
                    cv,
                    th.font_small(),
                    r.x,
                    r.y + th.s(6.0),
                    r.w,
                    r.h - th.s(6.0),
                    &s.to_uppercase(),
                    p.faint,
                    Align::Left,
                );
            }
            Row::Toggle { id, label: l, on } => {
                label(cv, text, l);
                if let Some(sr) = self.hit_rect(Hit::Toggle(*id)) {
                    switch(cv, th, p, sr, *on, self.hover == Hit::Toggle(*id));
                }
            }
            Row::Seg {
                id,
                label: l,
                opts,
                cur,
            } => {
                label(cv, text, l);
                let segs: Vec<(R, usize)> = self
                    .hits
                    .iter()
                    .filter_map(|(sr, h)| match h {
                        Hit::Seg(i, k) if i == id => Some((*sr, *k)),
                        _ => None,
                    })
                    .collect();
                if let (Some(a), Some(b)) = (segs.first(), segs.last()) {
                    let pad = th.s(2.0);
                    let box_r = R::new(
                        a.0.x - pad,
                        a.0.y - pad,
                        b.0.right() - a.0.x + 2.0 * pad,
                        a.0.h + 2.0 * pad,
                    );
                    rrect(cv, box_r.x, box_r.y, box_r.w, box_r.h, th.s(8.0), p.sunk);
                    rline(
                        cv,
                        box_r.x,
                        box_r.y,
                        box_r.w,
                        box_r.h,
                        th.s(8.0),
                        1.0,
                        p.line,
                    );
                }
                for (sr, k) in segs {
                    let on = k == *cur;
                    if on {
                        rrect(cv, sr.x, sr.y, sr.w, sr.h, th.s(6.0), p.raised);
                    } else if self.hover == Hit::Seg(*id, k) {
                        rrect(cv, sr.x, sr.y, sr.w, sr.h, th.s(6.0), p.hover);
                    }
                    let s = opts.get(k).map(String::as_str).unwrap_or("");
                    text.draw_in(
                        cv,
                        f,
                        sr.x,
                        sr.y,
                        sr.w,
                        sr.h,
                        s,
                        if on { p.text } else { p.dim },
                        Align::Center,
                    );
                }
            }
            Row::Slider {
                id,
                label: l,
                v,
                text: t,
            } => {
                label(cv, text, l);
                if let Some(tr) = self.hit_rect(Hit::Track(*id)) {
                    track(cv, th, p, tr, 0.0, *v, &[*v]);
                    value(cv, th, text, p, tr, t);
                }
            }
            Row::Range {
                id,
                label: l,
                lo,
                hi,
                text: t,
            } => {
                label(cv, text, l);
                if let Some(tr) = self.hit_rect(Hit::Track(*id)) {
                    track(cv, th, p, tr, *lo, *hi, &[*lo, *hi]);
                    value(cv, th, text, p, tr, t);
                }
            }
            Row::Step {
                id,
                label: l,
                text: t,
            } => {
                label(cv, text, l);
                if let (Some(a), Some(b)) = (
                    self.hit_rect(Hit::Minus(*id)),
                    self.hit_rect(Hit::Plus(*id)),
                ) {
                    let box_r = R::new(a.x, a.y, b.right() - a.x, a.h);
                    rrect(cv, box_r.x, box_r.y, box_r.w, box_r.h, th.s(7.0), p.sunk);
                    rline(
                        cv,
                        box_r.x,
                        box_r.y,
                        box_r.w,
                        box_r.h,
                        th.s(7.0),
                        1.0,
                        p.line,
                    );
                    for (rr, ic, h) in [
                        (a, Icon::Minus, Hit::Minus(*id)),
                        (b, Icon::Plus, Hit::Plus(*id)),
                    ] {
                        let c = if self.hover == h { p.text } else { p.dim };
                        icon_in(cv, icons, th, ic, rr, c, th.s(13.0));
                    }
                    text.draw_in(
                        cv,
                        th.font_mono(),
                        a.right(),
                        a.y,
                        b.x - a.right(),
                        a.h,
                        t,
                        p.text,
                        Align::Center,
                    );
                }
            }
            Row::Chips {
                id,
                label: l,
                chips,
            } => {
                label(cv, text, l);
                for (k, (s, on)) in chips.iter().enumerate() {
                    if let Some(cr) = self.hit_rect(Hit::Chip(*id, k)) {
                        let hov = self.hover == Hit::Chip(*id, k);
                        let (bg, line, fg) = if *on {
                            (p.accent.fade(0.10), p.accent.fade(0.45), p.accent)
                        } else {
                            (
                                if hov { p.hover } else { p.sunk },
                                p.line,
                                if hov { p.text } else { p.dim },
                            )
                        };
                        rrect(cv, cr.x, cr.y, cr.w, cr.h, th.s(7.0), bg);
                        rline(cv, cr.x, cr.y, cr.w, cr.h, th.s(7.0), 1.0, line);
                        text.draw_in(
                            cv,
                            th.font_small(),
                            cr.x,
                            cr.y,
                            cr.w,
                            cr.h,
                            s,
                            fg,
                            Align::Center,
                        );
                    }
                }
            }
            Row::Checks {
                id,
                items,
                add,
                removable,
            } => {
                rrect(cv, r.x, r.y, r.w, r.h, th.s(9.0), p.list);
                rline(cv, r.x, r.y, r.w, r.h, th.s(9.0), 1.0, p.line);
                let ih = th.s(32.0);
                for (k, (s, on, right)) in items.iter().enumerate() {
                    let yy = r.y + k as f32 * ih;
                    if k > 0 {
                        cv.fill_rect(r.x + 1.0, yy, r.w - 2.0, 1.0, p.line.fade(0.6));
                    }
                    let row_hot = matches!(self.hover, Hit::Check(i, kk) | Hit::Remove(i, kk) if i == *id && kk == k);
                    if let Some(cb) = self.hit_rect(Hit::Check(*id, k)) {
                        let bx = R::new(
                            cb.x + th.s(10.0),
                            yy + (ih - th.s(16.0)) * 0.5,
                            th.s(16.0),
                            th.s(16.0),
                        );
                        checkbox(cv, th, icons, p, bx, *on, row_hot);
                    }
                    text.draw_in(
                        cv,
                        f,
                        r.x + th.s(36.0),
                        yy,
                        r.w - th.s(120.0),
                        ih,
                        s,
                        if *on { p.text } else { p.dim },
                        Align::Left,
                    );
                    let rx = r.right() - th.s(if *removable { 40.0 } else { 12.0 });
                    text.draw_in(
                        cv,
                        th.font_mono(),
                        rx - th.s(70.0),
                        yy,
                        th.s(70.0),
                        ih,
                        right,
                        p.faint,
                        Align::Right,
                    );
                    if *removable && row_hot {
                        if let Some(xr) = self.hit_rect(Hit::Remove(*id, k)) {
                            if self.hover == Hit::Remove(*id, k) {
                                rrect(cv, xr.x, xr.y, xr.w, xr.h, th.s(5.0), p.hover);
                            }
                            icon_in(cv, icons, th, Icon::X, xr, p.dim, th.s(12.0));
                        }
                    }
                }
                if !add.is_empty() {
                    let yy = r.y + items.len() as f32 * ih;
                    cv.fill_rect(r.x + 1.0, yy, r.w - 2.0, 1.0, p.line.fade(0.6));
                    icon_in(
                        cv,
                        icons,
                        th,
                        Icon::Plus,
                        R::new(r.x + th.s(8.0), yy, th.s(20.0), ih),
                        p.faint,
                        th.s(13.0),
                    );
                    if let Some(fr) = self.hit_rect(Hit::AddField(*id)) {
                        let fld = st.adds.get(id).cloned().unwrap_or_default();
                        let focused = st.focus == Some(*id);
                        draw_field_bare(cv, th, text, p, &fld, fr, focused, add);
                    }
                    if let Some(br) = self.hit_rect(Hit::AddBtn(*id)) {
                        let hov = self.hover == Hit::AddBtn(*id);
                        rrect(
                            cv,
                            br.x,
                            br.y,
                            br.w,
                            br.h,
                            th.s(6.0),
                            p.accent.fade(if hov { 0.2 } else { 0.1 }),
                        );
                        rline(
                            cv,
                            br.x,
                            br.y,
                            br.w,
                            br.h,
                            th.s(6.0),
                            1.0,
                            p.accent.fade(0.45),
                        );
                        text.draw_in(
                            cv,
                            th.font_small(),
                            br.x,
                            br.y,
                            br.w,
                            br.h,
                            "Add",
                            p.accent,
                            Align::Center,
                        );
                    }
                }
            }
        }
    }

    fn hit_rect(&self, h: Hit) -> Option<R> {
        self.hits.iter().find(|(_, hh)| *hh == h).map(|(r, _)| *r)
    }
}

fn slide_out(s: Slide, v: f64, done: bool) -> Out {
    match s.which {
        1 => Out::Range {
            id: s.id,
            lo: v.min(s.other),
            hi: s.other,
            done,
        },
        2 => Out::Range {
            id: s.id,
            lo: s.other,
            hi: v.max(s.other),
            done,
        },
        _ => Out::Slide { id: s.id, v, done },
    }
}

fn find_row(p: &Panel, cur: usize, id: u32) -> Option<&Row> {
    p.sections.get(cur)?.rows.iter().find(|r| match r {
        Row::Slider { id: i, .. } | Row::Range { id: i, .. } => *i == id,
        _ => false,
    })
}

impl R {
    fn with_x(&self, x: f32) -> R {
        R::new(self.x + x, self.y, self.w, self.h)
    }
}

/// Le coin d'un corps de `w` x `h` voulu en (x, y), recale dans la vue.
fn place(w: f32, h: f32, x: f32, y: f32, vw: f32, vh: f32, m: f32) -> (f32, f32) {
    let x = x.min(vw - w - m).max(m);
    let y = if y + h > vh - m {
        (vh - h - m).max(m)
    } else {
        y.max(m)
    };
    (x.round(), y.round())
}

/// La taille d'un menu et ses rangees (relatives a son coin).
fn menu_size(th: &Theme, text: &mut TextCache, entries: &[Entry]) -> (f32, f32, Vec<R>) {
    let pad = th.s(5.0);
    let f = th.font();
    let icons = entries.iter().any(|e| {
        matches!(
            e,
            Entry::Item(Item { icon: Some(_), .. }) | Entry::Sub { icon: Some(_), .. }
        )
    });
    let mut wmax: f32 = 0.0;
    for e in entries {
        let w = match e {
            Entry::Item(i) => {
                let hint = if i.hint.is_empty() {
                    0.0
                } else {
                    text.measure(th.font_small(), &i.hint) + th.s(18.0)
                };
                text.measure(f, &i.text) + hint + th.s(26.0)
            }
            Entry::Sub { text: t, .. } => text.measure(f, t) + th.s(30.0),
            Entry::Head(t) => text.measure(th.font_small(), &t.to_uppercase()) + th.s(10.0),
            Entry::Sep => 0.0,
        };
        wmax = wmax.max(w);
    }
    let w = (wmax + th.s(20.0) + if icons { th.s(25.0) } else { 0.0 } + 2.0 * pad)
        .clamp(th.s(150.0), th.s(440.0))
        .round();
    let mut rows = Vec::with_capacity(entries.len());
    let mut y = pad;
    for e in entries {
        let h = match e {
            Entry::Sep => th.s(9.0),
            Entry::Head(_) => th.s(24.0),
            _ => th.s(28.0),
        };
        rows.push(R::new(pad, y, w - 2.0 * pad, h));
        y += h;
    }
    (w, (y + pad).round(), rows)
}

fn row_h(th: &Theme, row: &Row) -> f32 {
    match row {
        Row::Head(_) => th.s(30.0),
        Row::Checks { items, add, .. } => {
            th.s(32.0) * (items.len() + usize::from(!add.is_empty())) as f32
        }
        _ => th.s(34.0),
    }
}

/// Les zones qui se touchent d'une rangee du panneau.
fn row_hits(th: &Theme, text: &mut TextCache, row: &Row, r: R, out: &mut Vec<(R, Hit)>) {
    let cy = r.cy();
    match row {
        Row::Head(_) => {}
        Row::Toggle { id, .. } => {
            let (w, h) = (th.s(30.0), th.s(18.0));
            out.push((R::new(r.right() - w, cy - h * 0.5, w, h), Hit::Toggle(*id)));
        }
        Row::Seg { id, opts, .. } => {
            let h = th.s(24.0);
            let ws: Vec<f32> = opts
                .iter()
                .map(|o| {
                    (text.measure(th.font(), o) + th.s(18.0))
                        .max(th.s(30.0))
                        .round()
                })
                .collect();
            let total: f32 =
                ws.iter().sum::<f32>() + th.s(2.0) * (ws.len().saturating_sub(1)) as f32;
            let mut x = r.right() - th.s(2.0) - total;
            for (k, w) in ws.iter().enumerate() {
                out.push((R::new(x, cy - h * 0.5, *w, h), Hit::Seg(*id, k)));
                x += w + th.s(2.0);
            }
        }
        Row::Slider { id, .. } | Row::Range { id, .. } => {
            let tw = th.s(190.0).min(r.w * 0.42);
            let vw = th.s(72.0);
            out.push((
                R::new(
                    r.right() - vw - th.s(10.0) - tw,
                    cy - th.s(9.0),
                    tw,
                    th.s(18.0),
                ),
                Hit::Track(*id),
            ));
        }
        Row::Step { id, .. } => {
            let b = th.s(26.0);
            let vw = th.s(44.0);
            let x0 = r.right() - (2.0 * b + vw);
            out.push((R::new(x0, cy - b * 0.5, b, b), Hit::Minus(*id)));
            out.push((R::new(x0 + b + vw, cy - b * 0.5, b, b), Hit::Plus(*id)));
        }
        Row::Chips { id, chips, .. } => {
            let h = th.s(24.0);
            let ws: Vec<f32> = chips
                .iter()
                .map(|(s, _)| (text.measure(th.font_small(), s) + th.s(18.0)).round())
                .collect();
            let total: f32 =
                ws.iter().sum::<f32>() + th.s(6.0) * (ws.len().saturating_sub(1)) as f32;
            let mut x = r.right() - total;
            for (k, w) in ws.iter().enumerate() {
                out.push((R::new(x, cy - h * 0.5, *w, h), Hit::Chip(*id, k)));
                x += w + th.s(6.0);
            }
        }
        Row::Checks {
            id,
            items,
            add,
            removable,
        } => {
            let ih = th.s(32.0);
            for k in 0..items.len() {
                let y = r.y + k as f32 * ih;
                let right = if *removable { th.s(34.0) } else { 0.0 };
                out.push((R::new(r.x, y, r.w - right, ih), Hit::Check(*id, k)));
                if *removable {
                    let s = th.s(22.0);
                    out.push((
                        R::new(r.right() - th.s(8.0) - s, y + (ih - s) * 0.5, s, s),
                        Hit::Remove(*id, k),
                    ));
                }
            }
            if !add.is_empty() {
                let y = r.y + items.len() as f32 * ih;
                let bw = th.s(48.0);
                let h = th.s(24.0);
                out.push((
                    R::new(
                        r.x + th.s(30.0),
                        y + (ih - h) * 0.5,
                        r.w - th.s(30.0) - bw - th.s(14.0),
                        h,
                    ),
                    Hit::AddField(*id),
                ));
                out.push((
                    R::new(r.right() - th.s(8.0) - bw, y + (ih - h) * 0.5, bw, h),
                    Hit::AddBtn(*id),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Les pieces dessinees
// ---------------------------------------------------------------------------

/// Les couleurs d'un popover, tirees du theme (celui de REAPER).
struct Pal {
    bg: Color,
    head: Color,
    rail: Color,
    line: Color,
    hover: Color,
    raised: Color,
    sunk: Color,
    list: Color,
    text: Color,
    dim: Color,
    faint: Color,
    accent: Color,
    on_accent: Color,
    on_danger: Color,
    danger: Color,
}

fn lum(c: Color) -> f32 {
    (0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32) / 255.0
}

impl Pal {
    fn of(th: &Theme) -> Pal {
        let base = th.panel;
        let t = th.text;
        Pal {
            bg: base.mix(t, 0.035),
            head: base.mix(t, 0.07),
            rail: base.mix(t, 0.015),
            line: base.mix(t, 0.16),
            hover: base.mix(t, 0.10),
            raised: base.mix(t, 0.14),
            sunk: th.field,
            list: th.field.mix(t, 0.03),
            text: t,
            dim: th.text_dim,
            faint: th.text_faint,
            accent: th.accent,
            on_accent: if lum(th.accent) > 0.55 {
                th.accent.mix(cp_gfx::rgb(0, 0, 0), 0.8)
            } else {
                cp_gfx::rgb(255, 255, 255)
            },
            on_danger: cp_gfx::rgb(255, 255, 255),
            danger: th.error,
        }
    }
}

fn shadow(cv: &mut Canvas<'_>, th: &Theme, r: R) {
    // Des anneaux de plus en plus pales : une ombre sans tramer la surface.
    let rad = th.s(12.0);
    for (k, a) in [(1.0, 60u8), (2.0, 36), (4.0, 20), (7.0, 10)] {
        let d = th.s(k);
        rline(
            cv,
            r.x - d,
            r.y - d + th.s(k * 0.6),
            r.w + 2.0 * d,
            r.h + 2.0 * d,
            rad + d,
            th.s(1.0).max(1.0) + d * 0.5,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a,
            },
        );
    }
}

fn body(cv: &mut Canvas<'_>, th: &Theme, p: &Pal, r: R) {
    let rad = th.s(10.0);
    rrect(cv, r.x, r.y, r.w, r.h, rad, p.bg);
    rline(cv, r.x, r.y, r.w, r.h, rad, 1.0, p.line);
}

fn icon_in(
    cv: &mut Canvas<'_>,
    icons: &mut IconCache,
    th: &Theme,
    ic: Icon,
    r: R,
    c: Color,
    size: f32,
) {
    let sz = size.min(r.h).min(r.w).max(th.s(8.0)) as u16;
    icons.draw(
        cv,
        ic,
        (r.x + (r.w - sz as f32) * 0.5).round(),
        (r.y + (r.h - sz as f32) * 0.5).round(),
        sz,
        c,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_entry(
    cv: &mut Canvas<'_>,
    th: &Theme,
    text: &mut TextCache,
    icons: &mut IconCache,
    p: &Pal,
    e: &Entry,
    r: R,
    hot: bool,
    col: bool,
) {
    match e {
        Entry::Sep => cv.fill_rect(
            r.x + th.s(6.0),
            r.cy().round(),
            r.w - th.s(12.0),
            1.0,
            p.line,
        ),
        Entry::Head(t) => text.draw_in(
            cv,
            th.font_small(),
            r.x + th.s(10.0),
            r.y,
            r.w - th.s(20.0),
            r.h,
            &t.to_uppercase(),
            p.faint,
            Align::Left,
        ),
        Entry::Item(i) => {
            let danger_hot = hot && i.danger && i.enabled;
            if hot && i.enabled {
                rrect(
                    cv,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    th.s(7.0),
                    if danger_hot {
                        p.danger.fade(0.16)
                    } else {
                        p.hover
                    },
                );
            }
            let fg = if !i.enabled {
                p.faint
            } else if danger_hot {
                p.danger
            } else {
                p.text
            };
            let mut x = r.x + th.s(10.0);
            if let Some(ic) = i.icon {
                icon_in(
                    cv,
                    icons,
                    th,
                    ic,
                    R::new(x, r.y, th.s(16.0), r.h),
                    if danger_hot {
                        p.danger
                    } else if hot {
                        p.text
                    } else {
                        p.dim
                    },
                    th.s(15.0),
                );
            }
            if col {
                x += th.s(25.0);
            }
            let right = r.right() - th.s(10.0);
            let mut tw = right - x;
            if i.checked {
                icon_in(
                    cv,
                    icons,
                    th,
                    Icon::Check,
                    R::new(right - th.s(15.0), r.y, th.s(15.0), r.h),
                    p.accent,
                    th.s(15.0),
                );
                tw -= th.s(22.0);
            }
            if !i.hint.is_empty() {
                let hw = text.measure(th.font_small(), &i.hint);
                let hx = right - if i.checked { th.s(22.0) } else { 0.0 } - hw;
                text.draw_in(
                    cv,
                    th.font_small(),
                    hx,
                    r.y,
                    hw,
                    r.h,
                    &i.hint,
                    p.faint,
                    Align::Right,
                );
                tw = hx - th.s(10.0) - x;
            }
            text.draw_in(
                cv,
                th.font(),
                x,
                r.y,
                tw.max(0.0),
                r.h,
                &i.text,
                fg,
                Align::Left,
            );
        }
        Entry::Sub { text: t, icon, .. } => {
            if hot {
                rrect(cv, r.x, r.y, r.w, r.h, th.s(7.0), p.hover);
            }
            let mut x = r.x + th.s(10.0);
            if let Some(ic) = icon {
                icon_in(
                    cv,
                    icons,
                    th,
                    *ic,
                    R::new(x, r.y, th.s(16.0), r.h),
                    if hot { p.text } else { p.dim },
                    th.s(15.0),
                );
            }
            if col {
                x += th.s(25.0);
            }
            let right = r.right() - th.s(8.0);
            icon_in(
                cv,
                icons,
                th,
                Icon::ChevronRight,
                R::new(right - th.s(13.0), r.y, th.s(13.0), r.h),
                p.dim,
                th.s(13.0),
            );
            text.draw_in(
                cv,
                th.font(),
                x,
                r.y,
                right - th.s(20.0) - x,
                r.h,
                t,
                p.text,
                Align::Left,
            );
        }
    }
}

fn switch(cv: &mut Canvas<'_>, th: &Theme, p: &Pal, r: R, on: bool, hot: bool) {
    let rad = r.h * 0.5;
    if on {
        rrect(
            cv,
            r.x,
            r.y,
            r.w,
            r.h,
            rad,
            if hot {
                p.accent.mix(p.text, 0.15)
            } else {
                p.accent
            },
        );
    } else {
        rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
        rline(
            cv,
            r.x,
            r.y,
            r.w,
            r.h,
            rad,
            1.0,
            if hot { p.dim } else { p.line },
        );
    }
    let k = r.h - th.s(6.0);
    let kx = if on {
        r.right() - th.s(3.0) - k
    } else {
        r.x + th.s(3.0)
    };
    cv.fill_circle(
        kx + k * 0.5,
        r.cy(),
        k * 0.5,
        if on { p.on_accent } else { p.dim },
    );
}

fn track(cv: &mut Canvas<'_>, th: &Theme, p: &Pal, r: R, a: f64, b: f64, thumbs: &[f64]) {
    let ty = r.cy() - th.s(1.5);
    let th3 = th.s(3.0).max(2.0);
    rrect(cv, r.x, ty, r.w, th3, th3 * 0.5, p.line);
    let (xa, xb) = (r.x + r.w * a as f32, r.x + r.w * b as f32);
    if xb > xa {
        rrect(cv, xa, ty, xb - xa, th3, th3 * 0.5, p.accent);
    }
    for t in thumbs {
        let x = r.x + r.w * *t as f32;
        cv.fill_circle(x, r.cy(), th.s(6.5), cp_gfx::rgb(0, 0, 0).fade(0.35));
        cv.fill_circle(x, r.cy(), th.s(6.0), p.text);
    }
}

fn value(cv: &mut Canvas<'_>, th: &Theme, text: &mut TextCache, p: &Pal, tr: R, t: &str) {
    text.draw_in(
        cv,
        th.font_mono(),
        tr.right() + th.s(10.0),
        tr.y - th.s(4.0),
        th.s(72.0),
        tr.h + th.s(8.0),
        t,
        p.text,
        Align::Right,
    );
}

fn checkbox(
    cv: &mut Canvas<'_>,
    th: &Theme,
    icons: &mut IconCache,
    p: &Pal,
    r: R,
    on: bool,
    hot: bool,
) {
    let rad = th.s(4.0);
    if on {
        rrect(cv, r.x, r.y, r.w, r.h, rad, p.accent);
        icon_in(cv, icons, th, Icon::Check, r, p.on_accent, th.s(12.0));
    } else {
        rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
        rline(
            cv,
            r.x,
            r.y,
            r.w,
            r.h,
            rad,
            1.0,
            if hot { p.dim } else { p.line.mix(p.text, 0.1) },
        );
    }
}

/// Une saisie dessinee (lecture seule de son etat : le curseur reste
/// visible en faisant defiler le texte).
#[allow(clippy::too_many_arguments)]
fn draw_field(
    cv: &mut Canvas<'_>,
    th: &Theme,
    text: &mut TextCache,
    p: &Pal,
    f: &TextField,
    r: R,
    focused: bool,
    placeholder: &str,
) {
    let rad = th.s(7.0);
    rrect(cv, r.x, r.y, r.w, r.h, rad, p.sunk);
    rline(
        cv,
        r.x,
        r.y,
        r.w,
        r.h,
        rad,
        1.0,
        if focused { p.accent } else { p.line },
    );
    draw_field_bare(
        cv,
        th,
        text,
        p,
        f,
        R::new(r.x + th.s(4.0), r.y, r.w - th.s(8.0), r.h),
        focused,
        placeholder,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_field_bare(
    cv: &mut Canvas<'_>,
    th: &Theme,
    text: &mut TextCache,
    p: &Pal,
    f: &TextField,
    r: R,
    focused: bool,
    placeholder: &str,
) {
    let font = th.font();
    let pad = th.s(6.0);
    let (ix, iw) = (r.x + pad, (r.w - 2.0 * pad).max(0.0));
    let m = text.metrics(font);
    let base = r.y + (r.h - (m.ascent + m.descent)) * 0.5 + m.ascent;
    let (cur, anchor) = f.caret();
    let cx = text.measure(font, &f.text[..cur.min(f.text.len())]);
    let scroll = (cx - iw).max(0.0);
    if !cv.push_clip(ix, r.y, iw, r.h) {
        cv.pop_clip();
        return;
    }
    if f.text.is_empty() {
        text.draw(cv, font, ix, base, placeholder, p.faint);
    }
    let (a, b) = (cur.min(anchor), cur.max(anchor).min(f.text.len()));
    if focused && a != b {
        let xa = text.measure(font, &f.text[..a]) - scroll;
        let xb = text.measure(font, &f.text[..b]) - scroll;
        cv.fill_rect(
            ix + xa,
            r.y + th.s(5.0),
            xb - xa,
            r.h - th.s(10.0),
            p.accent.fade(0.35),
        );
    }
    text.draw(cv, font, ix - scroll, base, &f.text, p.text);
    if focused {
        cv.fill_rect(
            (ix + cx - scroll).round(),
            r.y + th.s(6.0),
            th.s(1.0).max(1.0),
            r.h - th.s(12.0),
            p.text,
        );
    }
    cv.pop_clip();
}

#[cfg(test)]
mod tests;
