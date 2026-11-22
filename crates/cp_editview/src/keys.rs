//! Les raccourcis de l'editeur MIDI de REAPER, repris tels quels.
//!
//! Deux sources, dans l'ordre ou REAPER les applique :
//! - les defauts de la section « MIDI Editor » (32060), que REAPER donne dans
//!   sa `KbdSectionInfo` ;
//! - les lignes `KEY` de `reaper-kb.ini` pour cette section : elles remplacent
//!   le defaut de la meme touche, et l'action 0 le desactive.
//!
//! Une touche est (drapeaux, code) comme REAPER l'ecrit : drapeaux 1 touche
//! virtuelle, 4 Maj, 8 Ctrl, 16 Alt, 32 Win ; code = la touche virtuelle,
//! plus 0x8000 pour une touche du pave de navigation (fleches, Origine...).
//!
//! Les actions personnalisees et les scripts sont gardes par leur NOM (lignes
//! `ACT` et `SCR`) : l'editeur reconnait les plus courants.

use std::collections::HashMap;

use cp_ui::Mods;

/// La section de l'editeur MIDI de REAPER.
pub const SECTION: u32 = 32060;

/// Ce qu'une touche fait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bind {
    /// Une action de l'editeur MIDI de REAPER (son numero).
    Cmd(u32),
    /// Une action personnalisee ou un script : son nom.
    Named(String),
}

/// Les drapeaux d'une touche, comme REAPER les ecrit.
pub fn flags_of(m: Mods) -> u32 {
    1 | if m.shift { 4 } else { 0 }
        | if m.ctrl { 8 } else { 0 }
        | if m.alt { 16 } else { 0 }
        | if m.win { 32 } else { 0 }
}

/// Le code d'une touche : `ext` est le drapeau « touche etendue » du
/// message (bit 24 de `lParam`).
pub fn key_of(vk: u32, ext: bool) -> u32 {
    if ext && is_nav(vk) { vk | 0x8000 } else { vk }
}

/// Les touches que le pave numerique double (REAPER les distingue).
fn is_nav(vk: u32) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E)
}

#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    map: HashMap<(u32, u32), Bind>,
}

impl KeyMap {
    /// `defaults` : (touche, action, drapeaux) de la section ; `ini` : le
    /// texte de `reaper-kb.ini`.
    pub fn build(defaults: &[(u32, u32, u32)], ini: &str) -> KeyMap {
        let mut map: HashMap<(u32, u32), Bind> = HashMap::new();
        for &(key, cmd, flags) in defaults {
            // 255 : la molette, pas une touche.
            if cmd != 0 && flags != 255 {
                map.insert((flags | 1, key), Bind::Cmd(cmd));
            }
        }
        // Les noms des actions personnalisees et des scripts.
        let mut names: HashMap<String, String> = HashMap::new();
        for line in ini.lines() {
            let w = words(line);
            match w.first().map(String::as_str) {
                Some("SCR") if w.len() >= 5 => {
                    names.insert(format!("_{}", w[3]), w[4].clone());
                }
                Some("ACT") if w.len() >= 5 => {
                    names.insert(format!("_{}", w[3]), w[4].clone());
                }
                _ => {}
            }
        }
        for line in ini.lines() {
            let line = line.split('#').next().unwrap_or("");
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.len() < 5 || w[0] != "KEY" || w[4].parse::<u32>().ok() != Some(SECTION) {
                continue;
            }
            let (Ok(flags), Ok(key)) = (w[1].parse::<u32>(), w[2].parse::<u32>()) else {
                continue;
            };
            // 255 : la molette, pas une touche.
            if flags == 255 {
                continue;
            }
            let k = (flags | 1, key);
            match w[3].parse::<u32>() {
                Ok(0) => {
                    map.remove(&k);
                }
                Ok(c) => {
                    map.insert(k, Bind::Cmd(c));
                }
                Err(_) => {
                    let id = w[3].to_string();
                    let name = names.get(&id).cloned().unwrap_or(id);
                    map.insert(k, Bind::Named(name));
                }
            }
        }
        KeyMap { map }
    }

    pub fn get(&self, vk: u32, ext: bool, m: Mods) -> Option<&Bind> {
        self.map.get(&(flags_of(m), key_of(vk, ext)))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Toutes les liaisons (pour le journal).
    pub fn iter(&self) -> impl Iterator<Item = (&(u32, u32), &Bind)> {
        self.map.iter()
    }
}

/// Les mots d'une ligne, les guillemets gardant leurs espaces.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Ce qu'un nom d'action personnalisee ou de script veut dire ici (les plus
/// courants chez Cedric), ou rien.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Duplicate,
    Split,
    DeleteUnderMouse,
    Fit,
    PlayFromItemStart,
}

pub fn named(name: &str) -> Option<Named> {
    let n = name.to_ascii_lowercase();
    if n.contains("duplicate") {
        Some(Named::Duplicate)
    } else if n.contains("delete note under mouse") {
        Some(Named::DeleteUnderMouse)
    } else if n.contains("split") && n.contains("note") && !n.contains("mousewheel") {
        Some(Named::Split)
    } else if n.contains("zoom to item") && n.contains("notes") || n.contains("zoom to content") {
        Some(Named::Fit)
    } else if n.contains("play from start of file") {
        Some(Named::PlayFromItemStart)
    } else {
        None
    }
}
