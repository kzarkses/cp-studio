//! La chaine d'effets d'une piste telle que REAPER la range : le bloc
//! `<FXCHAIN` d'un chunk de piste (`<MASTERFXLIST` dans le projet pour la
//! maitre). La lire, la remplacer, renouveler ses identifiants.
//!
//! Un preset garde ce bloc tel quel : chaque effet avec son etat entier
//! (`<JS` et ses donnees `<JS_SER` -- les patterns du sequenceur --, l'etat
//! d'un VST), son contournement, ses liens de parametres (`<PROGRAMENV`,
//! qui nomment l'effet source par sa PLACE), l'ordre. C'est le seul rappel
//! exact : refaire chaque effet par son nom perd l'ordre et les donnees des
//! JSFX.

/// Les blocs de chaine d'effets d'une piste (pas `<FXCHAIN_REC`, la chaine
/// d'entree).
const TAGS: [&str; 2] = ["<FXCHAIN", "<MASTERFXLIST"];

fn first_word(l: &str) -> &str {
    l.split_whitespace().next().unwrap_or("")
}

fn opens(l: &str) -> bool {
    l.trim_start().starts_with('<')
}

fn closes(l: &str) -> bool {
    l.trim() == ">"
}

/// Les lignes d'ouverture et de fermeture du bloc de chaine, enfant direct
/// de `<TRACK`.
fn block(lines: &[&str]) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut start = None;
    for (i, l) in lines.iter().enumerate() {
        if opens(l) {
            depth += 1;
            if depth == 2 && start.is_none() && TAGS.contains(&first_word(l)) {
                start = Some(i);
            }
        } else if closes(l) {
            if depth == 2 {
                if let Some(s) = start {
                    return Some((s, i));
                }
            }
            depth = depth.saturating_sub(1);
        }
    }
    None
}

/// Une ligne gardee : sans retrait ; la fenetre de chaine et les fenetres
/// flottantes ne s'ouvriront pas au rappel.
fn keep(l: &str) -> String {
    let l = l.trim_start();
    match first_word(l) {
        "SHOW" => "SHOW 0".to_string(),
        "FLOAT" => format!("FLOATPOS{}", &l["FLOAT".len()..]),
        _ => l.to_string(),
    }
}

/// Le contenu du bloc de chaine d'un chunk de piste (vide : aucun effet).
pub fn chain_lines(chunk: &str) -> Vec<String> {
    let lines: Vec<&str> = chunk.lines().collect();
    match block(&lines) {
        Some((s, e)) => lines[s + 1..e].iter().map(|l| keep(l)).collect(),
        None => Vec::new(),
    }
}

/// Le chunk de piste avec cette chaine a la place de la sienne. Une piste
/// sans effet n'a pas de bloc : il se pose avant la fin de `<TRACK`.
pub fn with_chain(chunk: &str, inner: &[String]) -> String {
    let lines: Vec<&str> = chunk.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + inner.len() + 2);
    match block(&lines) {
        Some((s, e)) => {
            out.extend(lines[..=s].iter().map(|l| l.to_string()));
            out.extend(inner.iter().cloned());
            out.extend(lines[e..].iter().map(|l| l.to_string()));
        }
        None => {
            let Some(end) = lines.iter().rposition(|l| closes(l)) else {
                return chunk.to_string();
            };
            if inner.is_empty() {
                return chunk.to_string();
            }
            out.extend(lines[..end].iter().map(|l| l.to_string()));
            out.push("<FXCHAIN".to_string());
            out.extend(inner.iter().cloned());
            out.push(">".to_string());
            out.extend(lines[end..].iter().map(|l| l.to_string()));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Les identifiants des effets (`FXID {...}`), dans l'ordre de la chaine.
pub fn fx_ids(inner: &[String]) -> Vec<String> {
    inner
        .iter()
        .filter(|l| first_word(l) == "FXID")
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// Donne a chaque effet un identifiant neuf (deux pistes ne partagent
/// jamais un effet) ; rend (ancien, neuf) pour recaler ce qui les nomme.
pub fn renew_ids(inner: &mut [String], mut fresh: impl FnMut() -> String) -> Vec<(String, String)> {
    let mut map = Vec::new();
    for l in inner.iter_mut() {
        if first_word(l) != "FXID" {
            continue;
        }
        let Some(old) = l.split_whitespace().nth(1).map(str::to_string) else {
            continue;
        };
        let new = fresh();
        *l = format!("FXID {new}");
        map.push((old, new));
    }
    map
}

/// Le texte avec chaque ancien identifiant remplace par le neuf.
pub fn remap(s: &str, map: &[(String, String)]) -> String {
    let mut s = s.to_string();
    for (old, new) in map {
        if !old.is_empty() && s.contains(old.as_str()) {
            s = s.replace(old.as_str(), new);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK: &str = "<TRACK {T}
NAME Lead
<FXCHAIN_REC
SHOW 0
BYPASS 0 0 0
<JS rec.jsfx \"\"
>
FXID {R}
>
<FXCHAIN
WNDRECT 1 2 3 4
SHOW 2
LASTSEL 1
DOCKED 0
BYPASS 0 0 0
<JS \"FX Constellation - Sequencer.jsfx\" \"\"
1 1 0 2
>
<JS_SER
AAAA
>
FLOATPOS 0 0 505 1392
FXID {A}
WAK 0 0
BYPASS 1 0 0
<VST \"VST3: Delay\" Delay.vst3 0 \"\" 1{X} \"\"
  ZZZZ
>
FLOAT 10 20 30 40
FXID {B}
<PROGRAMENV 1:1 0 \"Shift\"
PARAMBASE 0.5
PLINK -0.2 0:1 3 -0.5 \"\"
>
WAK 0 0
>
<ITEM
POSITION 0
<SOURCE WAVE
FILE \"a.wav\"
>
>
>
";

    #[test]
    fn the_chain_is_read_whole_and_nothing_else() {
        let c = chain_lines(TRACK);
        assert_eq!(c.first().map(String::as_str), Some("WNDRECT 1 2 3 4"));
        assert_eq!(c.last().map(String::as_str), Some("WAK 0 0"));
        assert!(
            c.contains(&"SHOW 0".to_string()),
            "no chain window at recall"
        );
        assert!(
            c.contains(&"FLOATPOS 10 20 30 40".to_string()),
            "no floating window"
        );
        assert!(c.contains(&"ZZZZ".to_string()));
        assert!(c.iter().any(|l| l.starts_with("PLINK")));
        assert!(
            !c.iter().any(|l| l.contains("rec.jsfx")),
            "not the input chain"
        );
        assert!(!c.iter().any(|l| l.contains("a.wav")), "not the items");
        assert_eq!(fx_ids(&c), vec!["{A}".to_string(), "{B}".to_string()]);
    }

    #[test]
    fn a_chain_replaces_the_other_and_keeps_the_track() {
        let new = vec![
            "BYPASS 0 0 0".to_string(),
            "<JS x.jsfx \"\"".into(),
            ">".into(),
            "FXID {N}".into(),
        ];
        let out = with_chain(TRACK, &new);
        assert_eq!(chain_lines(&out), new);
        assert!(out.contains("FILE \"a.wav\""), "items stay");
        assert!(out.contains("rec.jsfx"), "input chain stays");
        assert!(!out.contains("{A}"));
        assert_eq!(out.matches("<FXCHAIN\n").count(), 1);
    }

    #[test]
    fn a_track_without_effects_gets_a_chain() {
        let bare = "<TRACK {T}\nNAME x\n<ITEM\nPOSITION 0\n>\n>\n";
        let new = vec!["BYPASS 0 0 0".to_string(), "FXID {N}".into()];
        let out = with_chain(bare, &new);
        assert_eq!(chain_lines(&out), new);
        assert!(out.ends_with(">\n"));
        assert!(out.contains("POSITION 0"));
        assert_eq!(with_chain(bare, &[]), bare);
    }

    #[test]
    fn the_master_chain_reads_too() {
        let m = "<TRACK\n<MASTERFXLIST\nSHOW 1\nBYPASS 0 0 0\nFXID {M}\n>\n>\n";
        assert_eq!(fx_ids(&chain_lines(m)), vec!["{M}".to_string()]);
    }

    #[test]
    fn fresh_ids_follow_everything_that_names_them() {
        let mut c = chain_lines(TRACK);
        let mut n = 0;
        let map = renew_ids(&mut c, || {
            n += 1;
            format!("{{NEW{n}}}")
        });
        assert_eq!(fx_ids(&c), vec!["{NEW1}".to_string(), "{NEW2}".to_string()]);
        let state = "p {A}#0 sel=1\np {B}#3 sel=1\ndetached {B}";
        assert_eq!(
            remap(state, &map),
            "p {NEW1}#0 sel=1\np {NEW2}#3 sel=1\ndetached {NEW2}"
        );
    }
}
