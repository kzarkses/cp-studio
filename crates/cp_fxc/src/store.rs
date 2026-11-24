//! L'etat d'une piste, en texte : ce que le projet garde (ExtState de
//! projet, section `CPS_FXC`, cle = GUID de la piste).
//!
//! Une ligne par chose ; une ligne inconnue est ignoree (un format plus
//! recent se relit sans casser) :
//!
//! ```text
//! cps_fxc 1
//! pad nav=1 on=1 ws=2 ... x=0.5 y=0.5 ax=0.5 ay=0.5
//! macro 3 Cutoff sweep
//! preset Big one
//! snap Snapshot2
//! p {GUID}#3 sel=1 base=0.42 range=0.8 inv=0 x=1 y=0 src=1@0.5,211@-0.25u,103@0.1bx
//! fxmax {GUID} 4
//! fxonly {GUID}
//! grain 0.16 0.16 {GUID}#3=0.4;{GUID}#5=0.7
//! morph a {GUID}#3=0.4;{GUID}#5=0.7
//! ```

use std::collections::BTreeMap;

use crate::model::{Grain, Morph, Nav, PState, Pad, Src, Track};
use crate::slots::Pol;

const HEADER: &str = "cps_fxc 1";

fn f(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s
    }
}

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

/// Les sources d'un parametre : « slot@profondeur[u|b][x] » separes par des
/// virgules (le format de `FXC_ROUTES`).
pub fn encode_srcs(s: &[Src]) -> String {
    s.iter()
        .map(|s| {
            let mut t = format!("{}@{:.4}", s.slot, s.depth);
            if let Some(p) = s.pol {
                t.push(p.key());
            }
            if s.off {
                t.push('x');
            }
            t
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn decode_srcs(t: &str) -> Vec<Src> {
    let mut out = Vec::new();
    for part in t.split(',').filter(|p| !p.is_empty()) {
        let Some((slot, rest)) = part.split_once('@') else {
            continue;
        };
        let Ok(slot) = slot.trim().parse::<i32>() else {
            continue;
        };
        let num: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
            .collect();
        let flags = &rest[num.len()..];
        let Ok(depth) = num.parse::<f64>() else {
            continue;
        };
        if slot == 0 || out.iter().any(|s: &Src| s.slot == slot) {
            continue;
        }
        let mut s = Src::new(slot, depth);
        for c in flags.chars() {
            if let Some(p) = Pol::from_key(c) {
                s.pol = Some(p);
            } else if c == 'x' {
                s.off = true;
            }
        }
        out.push(s);
    }
    out.truncate(crate::model::MAX_SOURCES);
    out
}

fn encode_map(m: &BTreeMap<String, f64>) -> String {
    m.iter()
        .map(|(k, v)| format!("{k}={}", f(*v)))
        .collect::<Vec<_>>()
        .join(";")
}

fn decode_map(t: &str) -> BTreeMap<String, f64> {
    t.split(';')
        .filter_map(|kv| {
            let (k, v) = kv.rsplit_once('=')?;
            Some((k.to_string(), v.parse().ok()?))
        })
        .collect()
}

fn kv(rest: &str) -> BTreeMap<&str, &str> {
    rest.split_whitespace()
        .filter_map(|p| p.split_once('='))
        .collect()
}

pub fn encode_pad(p: &Pad) -> String {
    format!(
        "nav={} on={} ws={} wj={} wjump={} wsync={} fm={} fs={} fz={} zone={},{},{},{} gr={} gmin={} gmax={} slew={} smooth={} maxs={} sx={} sy={} gran={} grid={} x={} y={} ax={} ay={}",
        p.nav as u8,
        b(p.nav_on),
        f(p.walk_speed),
        f(p.walk_jitter),
        b(p.walk_jump),
        p.walk_sync,
        p.fig_mode,
        f(p.fig_speed),
        f(p.fig_size),
        f(p.zone[0]),
        f(p.zone[1]),
        f(p.zone[2]),
        f(p.zone[3]),
        f(p.gesture_range),
        f(p.gmin),
        f(p.gmax),
        f(p.link_slew),
        f(p.smooth),
        f(p.max_speed),
        p.src_x,
        p.src_y,
        b(p.granular),
        p.grid,
        f(p.x),
        f(p.y),
        f(p.ax),
        f(p.ay)
    )
}

/// Un pad relu : un champ absent prend sa valeur par defaut (jamais la
/// precedente).
pub fn decode_pad(rest: &str) -> Pad {
    let m = kv(rest);
    let d = Pad::default();
    let num = |k: &str, def: f64| m.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(def);
    let int = |k: &str, def: i64| m.get(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(def);
    let flag = |k: &str, def: bool| m.get(k).map_or(def, |v| *v == "1");
    let mut zone = d.zone;
    if let Some(z) = m.get("zone") {
        let v: Vec<f64> = z.split(',').filter_map(|x| x.parse().ok()).collect();
        if v.len() == 4 {
            zone = [v[0], v[1], v[2], v[3]];
        }
    }
    Pad {
        nav: Nav::from_i(int("nav", 0)),
        nav_on: flag("on", false),
        walk_speed: num("ws", d.walk_speed).clamp(0.1, 20.0),
        walk_jitter: num("wj", d.walk_jitter).clamp(0.0, 1.0),
        walk_jump: flag("wjump", false),
        walk_sync: int("wsync", 0).clamp(0, 5) as u8,
        fig_mode: int("fm", 0).clamp(0, 5) as u8,
        fig_speed: num("fs", d.fig_speed).clamp(0.01, 10.0),
        fig_size: num("fz", d.fig_size).clamp(0.1, 1.0),
        zone,
        gesture_range: num("gr", 1.0),
        gmin: num("gmin", 0.0).clamp(0.0, 1.0),
        gmax: num("gmax", 1.0).clamp(0.0, 1.0),
        link_slew: num("slew", 0.0).clamp(0.0, 2.0),
        smooth: num("smooth", 0.0).clamp(0.0, 1.0),
        max_speed: num("maxs", d.max_speed).clamp(0.1, 10.0),
        src_x: int("sx", 0) as i32,
        src_y: int("sy", 0) as i32,
        granular: flag("gran", false),
        grid: int("grid", 3).clamp(2, 4) as u8,
        x: num("x", 0.5).clamp(0.0, 1.0),
        y: num("y", 0.5).clamp(0.0, 1.0),
        ax: num("ax", 0.5).clamp(0.0, 1.0),
        ay: num("ay", 0.5).clamp(0.0, 1.0),
    }
}

pub fn encode_param(k: &str, p: &PState) -> String {
    let mut s = format!(
        "p {k} sel={} base={} range={} inv={} x={} y={}",
        b(p.sel),
        f(p.base),
        f(p.range),
        b(p.inv),
        b(p.x),
        b(p.y)
    );
    if !p.srcs.is_empty() {
        s.push_str(" src=");
        s.push_str(&encode_srcs(&p.srcs));
    }
    s
}

pub fn decode_param(rest: &str) -> Option<(String, PState)> {
    let (k, rest) = rest.split_once(' ')?;
    let m = kv(rest);
    let d = PState::default();
    let num = |key: &str, def: f64| {
        m.get(key)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(def)
    };
    let flag = |key: &str, def: bool| m.get(key).map_or(def, |v| *v == "1");
    Some((
        k.to_string(),
        PState {
            sel: flag("sel", false),
            base: num("base", d.base).clamp(0.0, 1.0),
            range: num("range", 1.0).clamp(0.0, 1.0),
            inv: flag("inv", false),
            x: flag("x", true),
            y: flag("y", true),
            srcs: m.get("src").map(|s| decode_srcs(s)).unwrap_or_default(),
        },
    ))
}

/// Ce qui merite d'etre garde d'un parametre (un parametre jamais touche
/// n'a pas de ligne).
fn worth(p: &PState) -> bool {
    p.sel || !p.srcs.is_empty() || p.range != 1.0 || p.inv || !(p.x && p.y)
}

pub fn encode(t: &Track) -> String {
    let mut out = vec![HEADER.to_string(), format!("pad {}", encode_pad(&t.pad))];
    for (i, n) in t.macro_names.iter().enumerate() {
        if !n.is_empty() {
            out.push(format!("macro {} {}", i + 1, n.replace('\n', " ")));
        }
    }
    if !t.preset.is_empty() {
        out.push(format!("preset {}", t.preset));
    }
    if !t.snapshot.is_empty() {
        out.push(format!("snap {}", t.snapshot));
    }
    for (k, p) in &t.params {
        if worth(p) {
            out.push(encode_param(k, p));
        }
    }
    for (g, n) in &t.fx_random_max {
        out.push(format!("fxmax {g} {n}"));
    }
    for (g, on) in &t.fx_tracked_only {
        if *on {
            out.push(format!("fxonly {g}"));
        }
    }
    for g in &t.grains {
        out.push(format!(
            "grain {} {} {}",
            f(g.x),
            f(g.y),
            encode_map(&g.values)
        ));
    }
    if let Some(m) = &t.morph_a {
        out.push(format!("morph a {}", encode_map(m)));
    }
    if let Some(m) = &t.morph_b {
        out.push(format!("morph b {}", encode_map(m)));
    }
    out.join("\n")
}

pub fn decode(s: &str) -> Option<Track> {
    let mut lines = s.lines();
    if lines.next()?.trim() != HEADER {
        return None;
    }
    let mut t = Track::default();
    for l in lines {
        let l = l.trim_end_matches('\r');
        let (what, rest) = l.split_once(' ').unwrap_or((l, ""));
        match what {
            "pad" => t.pad = decode_pad(rest),
            "macro" => {
                if let Some((n, name)) = rest.split_once(' ') {
                    if let Ok(n) = n.parse::<usize>() {
                        if (1..=8).contains(&n) {
                            t.macro_names[n - 1] = name.to_string();
                        }
                    }
                }
            }
            "preset" => t.preset = rest.to_string(),
            "snap" => t.snapshot = rest.to_string(),
            "p" => {
                if let Some((k, p)) = decode_param(rest) {
                    t.params.insert(k, p);
                }
            }
            "fxmax" => {
                if let Some((g, n)) = rest.split_once(' ') {
                    if let Ok(n) = n.trim().parse::<u8>() {
                        t.fx_random_max.insert(g.to_string(), n.clamp(1, 10));
                    }
                }
            }
            "fxonly" => {
                t.fx_tracked_only.insert(rest.trim().to_string(), true);
            }
            "grain" => {
                let mut it = rest.splitn(3, ' ');
                let (Some(x), Some(y)) = (it.next(), it.next()) else {
                    continue;
                };
                let (Ok(x), Ok(y)) = (x.parse(), y.parse()) else {
                    continue;
                };
                t.grains.push(Grain {
                    x,
                    y,
                    values: decode_map(it.next().unwrap_or("")),
                });
            }
            "morph" => {
                let (ab, m) = rest.split_once(' ').unwrap_or((rest, ""));
                let m: Morph = decode_map(m);
                match ab {
                    "a" => t.morph_a = Some(m),
                    "b" => t.morph_b = Some(m),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Some(t)
}
