//! Ce que CP Studio garde du navigateur d'effets : les favoris ajoutes ou
//! retires (par rapport au dossier « Favorites » de REAPER), les recents, les
//! compteurs d'usage, les onglets et les reglages. Un fichier texte, une
//! ligne par chose ; les effets y sont nommes comme REAPER les nomme.

use crate::Catalog;

/// Une regle d'onglet : une recherche dans une liste, tenue a jour toute
/// seule.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Rule {
    pub query: String,
    /// La liste de depart ([`crate::Src::key`]).
    pub src: String,
}

/// Un onglet : des effets choisis a la main, et (ou) une regle.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Tab {
    pub name: String,
    pub items: Vec<String>,
    pub rule: Option<Rule>,
}

/// Les reglages du navigateur.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Combien d'effets tire le de (1 a 12).
    pub dice_count: u32,
    /// Le de remplace la chaine (sinon il ajoute).
    pub dice_replace: bool,
    /// Un effet ajoute ouvre sa fenetre.
    pub auto_open: bool,
    pub sort_za: bool,
    /// La barre de cote et la chaine (montrees, largeurs a 96 DPI).
    pub show_side: bool,
    pub side_w: f32,
    pub show_chain: bool,
    pub chain_w: f32,
    /// Le navigateur remplace celui de REAPER.
    pub steal: bool,
    /// La liste montree.
    pub src: String,
    /// Les groupes ouverts de la barre de cote (dossiers, categories,
    /// developpeurs, formats).
    pub open_groups: [bool; 4],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            dice_count: 3,
            dice_replace: false,
            auto_open: true,
            sort_za: false,
            show_side: true,
            side_w: 150.0,
            show_chain: true,
            chain_w: 220.0,
            steal: true,
            src: "all".into(),
            open_groups: [true, false, false, false],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Store {
    pub fav_add: Vec<String>,
    pub fav_del: Vec<String>,
    /// Le plus recent en tete.
    pub recent: Vec<String>,
    pub uses: Vec<(String, u32)>,
    pub tabs: Vec<Tab>,
    pub set: Settings,
}

const HEAD: &str = "cp_fx_browser 1";
pub const MAX_RECENT: usize = 30;

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

impl Store {
    /// L'effet `i` est-il favori ? Le dossier « Favorites » de REAPER, plus
    /// ce qu'on y a ajoute, moins ce qu'on en a retire.
    pub fn is_fav(&self, cat: &Catalog, i: usize) -> bool {
        let name = &cat.plugins[i].name;
        if self.fav_add.iter().any(|n| n == name) {
            return true;
        }
        cat.reaper_favs.contains(&i) && !self.fav_del.iter().any(|n| n == name)
    }

    pub fn toggle_fav(&mut self, cat: &Catalog, i: usize) {
        let name = cat.plugins[i].name.clone();
        if self.is_fav(cat, i) {
            if let Some(k) = self.fav_add.iter().position(|n| *n == name) {
                self.fav_add.remove(k);
            }
            if cat.reaper_favs.contains(&i) && !self.fav_del.contains(&name) {
                self.fav_del.push(name);
            }
        } else {
            if let Some(k) = self.fav_del.iter().position(|n| *n == name) {
                self.fav_del.remove(k);
            }
            if !cat.reaper_favs.contains(&i) && !self.fav_add.contains(&name) {
                self.fav_add.push(name);
            }
        }
    }

    /// Un effet vient d'etre ajoute : il passe en tete des recents, et son
    /// compteur monte.
    pub fn used(&mut self, name: &str) {
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(MAX_RECENT);
        match self.uses.iter_mut().find(|(n, _)| n == name) {
            Some((_, c)) => *c += 1,
            None => self.uses.push((name.to_string(), 1)),
        }
    }

    pub fn tab(&self, name: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.name == name)
    }

    pub fn tab_mut(&mut self, name: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.name == name)
    }

    /// Un nom d'onglet libre : « Tab », « Tab 2 »...
    pub fn free_tab_name(&self, base: &str) -> String {
        let base = if base.trim().is_empty() {
            "Tab"
        } else {
            base.trim()
        };
        if self.tab(base).is_none() {
            return base.to_string();
        }
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| self.tab(n).is_none())
            .unwrap_or_default()
    }

    /// Ajoute des effets a un onglet (sans doublon). Rend combien sont entres.
    pub fn add_to_tab(&mut self, tab: &str, names: &[String]) -> usize {
        let Some(t) = self.tab_mut(tab) else { return 0 };
        let mut k = 0;
        for n in names {
            if !t.items.contains(n) {
                t.items.push(n.clone());
                k += 1;
            }
        }
        k
    }

    pub fn encode(&self) -> String {
        let s = &self.set;
        let mut o = String::new();
        o.push_str(HEAD);
        o.push('\n');
        let mut set = |k: &str, v: String| {
            o.push_str(&format!("set {k} {v}\n"));
        };
        set("dice_count", s.dice_count.to_string());
        set("dice_replace", b(s.dice_replace).into());
        set("auto_open", b(s.auto_open).into());
        set("sort_za", b(s.sort_za).into());
        set("show_side", b(s.show_side).into());
        set("side_w", format!("{:.0}", s.side_w));
        set("show_chain", b(s.show_chain).into());
        set("chain_w", format!("{:.0}", s.chain_w));
        set("steal", b(s.steal).into());
        set("src", s.src.clone());
        set(
            "open_groups",
            s.open_groups.iter().map(|&g| b(g)).collect::<String>(),
        );
        for n in &self.fav_add {
            o.push_str(&format!("fav+ {n}\n"));
        }
        for n in &self.fav_del {
            o.push_str(&format!("fav- {n}\n"));
        }
        for n in &self.recent {
            o.push_str(&format!("recent {n}\n"));
        }
        for (n, c) in &self.uses {
            o.push_str(&format!("use {c} {n}\n"));
        }
        for t in &self.tabs {
            o.push_str(&format!("tab {}\n", t.name));
            if let Some(r) = &t.rule {
                o.push_str(&format!("rule {}\t{}\n", r.src, r.query));
            }
            for n in &t.items {
                o.push_str(&format!("item {n}\n"));
            }
        }
        o
    }

    /// Relit [`Store::encode`]. Un fichier d'un autre format : `None`.
    pub fn decode(text: &str) -> Option<Store> {
        let mut lines = text.lines();
        if lines.next()?.trim() != HEAD {
            return None;
        }
        let mut st = Store::default();
        for line in lines {
            let line = line.trim_end_matches('\r');
            let Some((k, v)) = line.split_once(' ') else {
                continue;
            };
            match k {
                "set" => {
                    let (name, val) = v.split_once(' ').unwrap_or((v, ""));
                    let s = &mut st.set;
                    let on = val == "1";
                    match name {
                        "dice_count" => s.dice_count = val.parse().unwrap_or(3).clamp(1, 12),
                        "dice_replace" => s.dice_replace = on,
                        "auto_open" => s.auto_open = on,
                        "sort_za" => s.sort_za = on,
                        "show_side" => s.show_side = on,
                        "side_w" => s.side_w = val.parse().unwrap_or(150.0),
                        "show_chain" => s.show_chain = on,
                        "chain_w" => s.chain_w = val.parse().unwrap_or(220.0),
                        "steal" => s.steal = on,
                        "src" => s.src = val.to_string(),
                        "open_groups" => {
                            for (g, c) in s.open_groups.iter_mut().zip(val.chars()) {
                                *g = c == '1';
                            }
                        }
                        _ => {}
                    }
                }
                "fav+" => st.fav_add.push(v.to_string()),
                "fav-" => st.fav_del.push(v.to_string()),
                "recent" => st.recent.push(v.to_string()),
                "use" => {
                    if let Some((c, n)) = v.split_once(' ') {
                        if let Ok(c) = c.parse() {
                            st.uses.push((n.to_string(), c));
                        }
                    }
                }
                "tab" => st.tabs.push(Tab {
                    name: v.to_string(),
                    ..Tab::default()
                }),
                "rule" => {
                    if let (Some(t), Some((src, q))) = (st.tabs.last_mut(), v.split_once('\t')) {
                        t.rule = Some(Rule {
                            query: q.to_string(),
                            src: src.to_string(),
                        });
                    }
                }
                "item" => {
                    if let Some(t) = st.tabs.last_mut() {
                        t.items.push(v.to_string());
                    }
                }
                _ => {}
            }
        }
        Some(st)
    }
}
