//! Les icones de CP Transport : une liste d'actions, rangee dans
//! `<ressources REAPER>/CP_Studio/transport_items.txt`, une par ligne :
//! `identifiant<TAB>icone`. L'identifiant est celui que REAPER garde stable
//! (« _RS... » pour un script, « _CPSTUDIO_... » pour les notres, un numero
//! pour une action native). L'icone : `png:<fichier>` (cherche dans le theme
//! courant, puis dans `Data/toolbar_icons`, comme REAPER le fait pour ses
//! barres d'outils), `lucide:<Nom>`, ou rien (des initiales).
//!
//! Les outils de CP Studio s'y ajoutent d'eux-memes, UNE fois : si Cedric en
//! retire un, il ne revient pas (la liste des deja-ajoutes est gardee a cote).

use std::collections::HashMap;

use cp_ui::Icon;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCfg {
    pub id: String,
    pub icon: String,
}

/// Les outils de CP Studio a poser dans la barre : (action, icone, icone de
/// secours si le theme et `Data/toolbar_icons` n'ont pas la premiere). Les
/// icones natives d'abord : elles ont la finesse et les couleurs du theme.
pub const TOOLS: [(&str, &str, &str); 5] = [
    (
        "_CPSTUDIO_GUITARTAB",
        "png:toolbar_misc_guitar.png",
        "lucide:Guitar",
    ),
    ("_CPSTUDIO_EDITOR", "lucide:Piano", "lucide:Piano"),
    ("_CPSTUDIO_SESSION", "lucide:Grid", "lucide:Grid"),
    ("_CPSTUDIO_FXBROWSER", "lucide:Puzzle", "lucide:Puzzle"),
    ("_CPSTUDIO_FXCONSTELLATION", "lucide:Move", "lucide:Move"),
];

/// L'icone de secours d'un outil de CP Studio.
pub fn fallback(id: &str) -> Option<&'static str> {
    TOOLS.iter().find(|t| t.0 == id).map(|t| t.2)
}

/// Les outils poses avec leur ancienne icone par defaut (vectorielle)
/// prennent l'icone native. Rend vrai si la liste a change.
pub fn migrate(items: &mut [ItemCfg]) -> bool {
    let mut changed = false;
    for c in items.iter_mut() {
        if let Some(t) = TOOLS.iter().find(|t| t.0 == c.id) {
            if c.icon == t.2 {
                c.icon = t.1.to_string();
                changed = true;
            }
        }
    }
    changed
}

pub fn parse(text: &str) -> Vec<ItemCfg> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut it = l.splitn(2, '\t');
            ItemCfg {
                id: it.next().unwrap_or("").trim().to_string(),
                icon: it.next().unwrap_or("").trim().to_string(),
            }
        })
        .filter(|c| !c.id.is_empty())
        .collect()
}

pub fn serialize(items: &[ItemCfg]) -> String {
    let mut s = String::from("# CP Transport: one action per line (id<TAB>icon)\n");
    for c in items {
        s += &format!("{}\t{}\n", c.id, c.icon);
    }
    s
}

/// Ajoute les outils pas encore poses. Rend vrai si la liste a change.
pub fn seed_tools(items: &mut Vec<ItemCfg>, seeded: &mut Vec<String>) -> bool {
    let mut changed = false;
    for (id, icon, _) in TOOLS {
        if seeded.iter().any(|s| s == id) {
            continue;
        }
        seeded.push(id.to_string());
        if !items.iter().any(|c| c.id == id) {
            items.push(ItemCfg {
                id: id.to_string(),
                icon: icon.to_string(),
            });
            changed = true;
        }
    }
    changed
}

/// Les scripts connus de REAPER (reaper-kb.ini, lignes « SCR ») : leur
/// identifiant (sans « _ ») et leur chemin tel qu'ecrit (relatif au dossier
/// Scripts, ou absolu). Le chemin n'a de guillemets que s'il contient un
/// espace.
pub fn kb_scripts(kb: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for l in kb.lines() {
        let Some(rest) = l.strip_prefix("SCR ") else {
            continue;
        };
        // SCR <drapeaux> <section> <id> <nom> <chemin>
        let mut parts = rest.splitn(3, ' ');
        let (Some(_), Some(_), Some(tail)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some((id, after)) = tail.split_once(' ') else {
            continue;
        };
        let after = after.trim_start();
        // Le nom : entre guillemets, ou un mot.
        let path = if let Some(stripped) = after.strip_prefix('"') {
            match stripped.find('"') {
                Some(e) => stripped[e + 1..].trim(),
                None => continue,
            }
        } else {
            match after.split_once(' ') {
                Some((_, p)) => p.trim(),
                None => continue,
            }
        };
        let path = path.trim_matches('"');
        if !path.is_empty() {
            out.insert(id.to_string(), path.to_string());
        }
    }
    out
}

pub fn lucide(name: &str) -> Option<Icon> {
    Some(match name {
        "Guitar" => Icon::Guitar,
        "Piano" => Icon::Piano,
        "Grid" => Icon::Grid,
        "Puzzle" => Icon::Puzzle,
        "Move" => Icon::Move,
        "Play" => Icon::Play,
        "Stop" => Icon::Stop,
        "Repeat" => Icon::Repeat,
        "Music" => Icon::Music,
        "AudioLines" => Icon::AudioLines,
        "Folder" => Icon::Folder,
        "FolderSearch" => Icon::FolderSearch,
        "Star" => Icon::Star,
        "Shuffle" => Icon::Shuffle,
        "Database" => Icon::Database,
        "List" => Icon::List,
        "House" => Icon::House,
        "Metronome" => Icon::Metronome,
        "Search" => Icon::Search,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_liste_se_relit() {
        let v = vec![
            ItemCfg {
                id: "_RSabc".into(),
                icon: "png:x y.png".into(),
            },
            ItemCfg {
                id: "40364".into(),
                icon: String::new(),
            },
        ];
        assert_eq!(parse(&serialize(&v)), v);
        assert!(parse("# rien\n\n").is_empty());
    }

    #[test]
    fn un_outil_retire_ne_revient_pas() {
        let mut items = Vec::new();
        let mut seeded = Vec::new();
        assert!(seed_tools(&mut items, &mut seeded));
        assert_eq!(items.len(), TOOLS.len());
        items.clear();
        assert!(!seed_tools(&mut items, &mut seeded));
        assert!(items.is_empty());
    }

    #[test]
    fn l_ancienne_icone_vectorielle_devient_native() {
        let mut v = vec![
            ItemCfg {
                id: "_CPSTUDIO_GUITARTAB".into(),
                icon: "lucide:Guitar".into(),
            },
            ItemCfg {
                id: "_RSabc".into(),
                icon: "lucide:Guitar".into(),
            },
        ];
        assert!(migrate(&mut v));
        assert_eq!(v[0].icon, "png:toolbar_misc_guitar.png");
        assert_eq!(v[1].icon, "lucide:Guitar"); // pas un outil : intact
        assert!(!migrate(&mut v));
        assert_eq!(fallback("_CPSTUDIO_GUITARTAB"), Some("lucide:Guitar"));
    }

    #[test]
    fn les_scripts_de_reaper_kb_avec_et_sans_guillemets() {
        let kb = "SCR 4 0 RSdbf6 \"Custom: BirdBird_Global Sampler.lua\" \"BirdBird ReaScript Testing/Global Sampler/BirdBird_Global Sampler.lua\"\n\
                  SCR 4 0 RS83 \"Custom: CP_ViewArrange.lua\" CP_Scripts/CP_Tools/CP_ViewArrange.lua\n\
                  KEY 1 65 0 0\n";
        let m = kb_scripts(kb);
        assert_eq!(
            m.get("RSdbf6").map(String::as_str),
            Some("BirdBird ReaScript Testing/Global Sampler/BirdBird_Global Sampler.lua")
        );
        assert_eq!(
            m.get("RS83").map(String::as_str),
            Some("CP_Scripts/CP_Tools/CP_ViewArrange.lua")
        );
        assert_eq!(m.len(), 2);
    }
}
