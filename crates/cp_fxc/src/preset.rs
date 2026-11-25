//! Les presets et les snapshots de FX Constellation.
//!
//! - Un **preset** dit tout ce qu'on entend sur la piste : la chaine (chaque
//!   effet par son nom d'origine, son nom donne, son etat entier, ses
//!   valeurs), le Sound Gen, le sequenceur, la banque de sources, le pad, les
//!   macros, ce que chaque parametre porte, les liens internes. Il vit dans
//!   un fichier a lui (`<ressources>/CP_Studio/FX Constellation/<nom>.fxc`).
//! - Un **snapshot** est le mapping et le pad, rangé DANS un preset : il ne
//!   touche que les effets qu'il connait (par leur nom).
//!
//! Ce qu'on garde d'un parametre est la BASE (l'ancre), jamais la valeur
//! jouee : sinon le decalage du pad se cuit et le son derive a chaque aller
//! retour.

use std::collections::BTreeMap;

use crate::model::{PState, Pad};
use crate::store::{decode_pad, decode_param, encode_pad, encode_param};

const HEADER: &str = "cps_fxc_preset 1";

/// Un effet de la chaine d'un preset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetFx {
    /// Le nom montre (`TrackFX_GetFXName`).
    pub name: String,
    /// Le nom d'origine (`fx_name`), celui que `TrackFX_AddByName` reconnait.
    pub ident: String,
    /// Le nom donne par l'utilisateur (vide : aucun).
    pub renamed: String,
    pub enabled: bool,
    pub detached: bool,
    /// Le preset de l'effet (`TrackFX_GetPreset`).
    pub preset: String,
    /// L'etat entier : (`vst_chunk` | `clap_chunk`, donnees).
    pub chunk: Option<(String, String)>,
    /// Toutes les valeurs, normalisees (le repli sans etat).
    pub raw: Vec<f64>,
    /// Ce que portent ses parametres (par indice), avec leur nom.
    pub params: BTreeMap<usize, (String, PState)>,
}

/// Un snapshot : le mapping et le pad.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub pad: Pad,
    pub macros: [f64; 8],
    pub macro_names: [String; 8],
    /// Les effets qu'il connait (par nom) et leur contournement.
    pub fx: Vec<(String, bool)>,
    /// « nom de l'effet||nom du parametre » → ce qu'il porte.
    pub params: BTreeMap<String, PState>,
    /// Les reglages de la banque locale (normalises), s'il y en avait une.
    pub bank: Option<Vec<f64>>,
    /// Les reglages internes menes par des sources (comme le preset).
    pub internal: BTreeMap<(String, usize), PState>,
}

/// Un preset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub pad: Pad,
    pub macros: [f64; 8],
    pub macro_names: [String; 8],
    pub fx: Vec<PresetFx>,
    /// Le Sound Gen : allume, et ses valeurs normalisees.
    pub sg: Option<(bool, Vec<f64>)>,
    pub seq: Option<Vec<f64>>,
    pub bank: Option<Vec<f64>>,
    /// Les liens internes (`FXC_ROUTES`).
    pub routes: String,
    /// Les reglages des effets internes menes par des sources : (« sg »,
    /// « seq », « bank », « bridge », indice) → ce qu'ils portent.
    pub internal: BTreeMap<(String, usize), PState>,
    pub snapshots: BTreeMap<String, Snapshot>,
}

fn list(v: &[f64]) -> String {
    v.iter()
        .map(|x| {
            let s = format!("{x:.6}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() || s == "-0" {
                "0".to_string()
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn unlist(s: &str) -> Vec<f64> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn encode_macros(out: &mut Vec<String>, pre: &str, v: &[f64; 8], names: &[String; 8]) {
    for i in 0..8 {
        out.push(format!(
            "{pre}macro {} {:.6}\t{}",
            i + 1,
            v[i],
            clean(&names[i])
        ));
    }
}

fn decode_macro(rest: &str, v: &mut [f64; 8], names: &mut [String; 8]) {
    let (head, name) = rest.split_once('\t').unwrap_or((rest, ""));
    let mut it = head.split_whitespace();
    let (Some(n), Some(x)) = (it.next(), it.next()) else {
        return;
    };
    if let (Ok(n), Ok(x)) = (n.parse::<usize>(), x.parse::<f64>()) {
        if (1..=8).contains(&n) {
            v[n - 1] = x.clamp(0.0, 1.0);
            names[n - 1] = name.to_string();
        }
    }
}

pub fn encode(p: &Preset) -> String {
    let mut out = vec![HEADER.to_string(), format!("pad {}", encode_pad(&p.pad))];
    encode_macros(&mut out, "", &p.macros, &p.macro_names);
    for (i, f) in p.fx.iter().enumerate() {
        out.push(format!(
            "fx {i} enabled={} detached={}",
            f.enabled as u8, f.detached as u8
        ));
        out.push(format!("fx.name {i}\t{}", clean(&f.name)));
        out.push(format!("fx.ident {i}\t{}", clean(&f.ident)));
        if !f.renamed.is_empty() {
            out.push(format!("fx.renamed {i}\t{}", clean(&f.renamed)));
        }
        if !f.preset.is_empty() {
            out.push(format!("fx.preset {i}\t{}", clean(&f.preset)));
        }
        if let Some((k, d)) = &f.chunk {
            out.push(format!("fx.chunk {i} {k}\t{}", clean(d)));
        }
        if !f.raw.is_empty() {
            out.push(format!("fx.raw {i}\t{}", list(&f.raw)));
        }
        for (pi, (name, ps)) in &f.params {
            let e = encode_param("_", ps);
            let rest = e.trim_start_matches("p _ ");
            out.push(format!("fx.p {i} {pi}\t{}\t{rest}", clean(name)));
        }
    }
    if let Some((on, v)) = &p.sg {
        out.push(format!("sg {}\t{}", *on as u8, list(v)));
    }
    if let Some(v) = &p.seq {
        out.push(format!("seq\t{}", list(v)));
    }
    if let Some(v) = &p.bank {
        out.push(format!("bank\t{}", list(v)));
    }
    if !p.routes.is_empty() {
        out.push(format!("routes\t{}", clean(&p.routes)));
    }
    for ((k, i), ps) in &p.internal {
        let e = encode_param("_", ps);
        let rest = e.trim_start_matches("p _ ");
        out.push(format!("int {k} {i}\t{rest}"));
    }
    for (name, s) in &p.snapshots {
        out.push(format!("snap\t{}", clean(name)));
        out.push(format!("snap.pad {}", encode_pad(&s.pad)));
        encode_macros(&mut out, "snap.", &s.macros, &s.macro_names);
        for (n, by) in &s.fx {
            out.push(format!("snap.fx {}\t{}", *by as u8, clean(n)));
        }
        for (k, ps) in &s.params {
            let e = encode_param("_", ps);
            let rest = e.trim_start_matches("p _ ");
            out.push(format!("snap.p\t{}\t{rest}", clean(k)));
        }
        if let Some(v) = &s.bank {
            out.push(format!("snap.bank\t{}", list(v)));
        }
        for ((k, i), ps) in &s.internal {
            let e = encode_param("_", ps);
            let rest = e.trim_start_matches("p _ ");
            out.push(format!("snap.int {k} {i}\t{rest}"));
        }
    }
    out.join("\n")
}

pub fn decode(s: &str) -> Option<Preset> {
    let mut lines = s.lines();
    if lines.next()?.trim() != HEADER {
        return None;
    }
    let mut p = Preset::default();
    let mut snap: Option<(String, Snapshot)> = None;
    let fx_at = |p: &mut Preset, i: usize| -> Option<usize> {
        if i > 4096 {
            return None;
        }
        while p.fx.len() <= i {
            p.fx.push(PresetFx::default());
        }
        Some(i)
    };
    for l in lines {
        let l = l.trim_end_matches('\r');
        let (head, tail) = l.split_once('\t').unwrap_or((l, ""));
        let mut w = head.split_whitespace();
        let what = w.next().unwrap_or("");
        let idx =
            |w: &mut std::str::SplitWhitespace<'_>| w.next().and_then(|x| x.parse::<usize>().ok());
        match what {
            "pad" => p.pad = decode_pad(head.trim_start_matches("pad").trim()),
            "macro" => decode_macro(
                l.trim_start_matches("macro "),
                &mut p.macros,
                &mut p.macro_names,
            ),
            "fx" => {
                if let Some(i) = idx(&mut w).and_then(|i| fx_at(&mut p, i)) {
                    for kv in w {
                        match kv {
                            "enabled=1" => p.fx[i].enabled = true,
                            "detached=1" => p.fx[i].detached = true,
                            _ => {}
                        }
                    }
                }
            }
            "fx.name" | "fx.ident" | "fx.renamed" | "fx.preset" | "fx.raw" => {
                if let Some(i) = idx(&mut w).and_then(|i| fx_at(&mut p, i)) {
                    let f = &mut p.fx[i];
                    match what {
                        "fx.name" => f.name = tail.to_string(),
                        "fx.ident" => f.ident = tail.to_string(),
                        "fx.renamed" => f.renamed = tail.to_string(),
                        "fx.preset" => f.preset = tail.to_string(),
                        _ => f.raw = unlist(tail),
                    }
                }
            }
            "fx.chunk" => {
                if let (Some(i), Some(k)) = (idx(&mut w), w.next()) {
                    if let Some(i) = fx_at(&mut p, i) {
                        p.fx[i].chunk = Some((k.to_string(), tail.to_string()));
                    }
                }
            }
            "fx.p" => {
                let (Some(i), Some(pi)) = (idx(&mut w), idx(&mut w)) else {
                    continue;
                };
                let Some(i) = fx_at(&mut p, i) else { continue };
                let (name, rest) = tail.split_once('\t').unwrap_or((tail, ""));
                if let Some((_, ps)) = decode_param(&format!("_ {rest}")) {
                    p.fx[i].params.insert(pi, (name.to_string(), ps));
                }
            }
            "sg" => {
                let on = w.next() == Some("1");
                p.sg = Some((on, unlist(tail)));
            }
            "seq" => p.seq = Some(unlist(tail)),
            "bank" => p.bank = Some(unlist(tail)),
            "routes" => p.routes = tail.to_string(),
            "int" | "snap.int" => {
                let (Some(k), Some(i)) = (w.next(), idx(&mut w)) else {
                    continue;
                };
                if let Some((_, ps)) = decode_param(&format!("_ {tail}")) {
                    let key = (k.to_string(), i);
                    if what == "int" {
                        p.internal.insert(key, ps);
                    } else if let Some((_, s)) = snap.as_mut() {
                        s.internal.insert(key, ps);
                    }
                }
            }
            "snap" => {
                if let Some((n, s)) = snap.take() {
                    p.snapshots.insert(n, s);
                }
                snap = Some((tail.to_string(), Snapshot::default()));
            }
            "snap.pad" | "snap.macro" | "snap.fx" | "snap.p" | "snap.bank" => {
                let Some((_, s)) = snap.as_mut() else {
                    continue;
                };
                match what {
                    "snap.pad" => s.pad = decode_pad(head.trim_start_matches("snap.pad").trim()),
                    "snap.macro" => decode_macro(
                        l.trim_start_matches("snap.macro "),
                        &mut s.macros,
                        &mut s.macro_names,
                    ),
                    "snap.fx" => s.fx.push((tail.to_string(), w.next() == Some("1"))),
                    "snap.p" => {
                        let (k, rest) = tail.split_once('\t').unwrap_or((tail, ""));
                        if let Some((_, ps)) = decode_param(&format!("_ {rest}")) {
                            s.params.insert(k.to_string(), ps);
                        }
                    }
                    _ => s.bank = Some(unlist(tail)),
                }
            }
            _ => {}
        }
    }
    if let Some((n, s)) = snap.take() {
        p.snapshots.insert(n, s);
    }
    Some(p)
}

/// Le prochain nom libre « Snapshot<N> » (calcule au clic : « + » ne doit
/// jamais ecraser un snapshot).
pub fn next_snapshot_name(p: &Preset) -> String {
    let n = p
        .snapshots
        .keys()
        .filter_map(|k| k.strip_prefix("Snapshot")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Snapshot{}", n + 1)
}

/// Un nom de fichier sur pour un nom de preset.
pub fn file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()'".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.fxc", s.trim())
}
