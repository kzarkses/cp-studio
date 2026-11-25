//! FX Constellation et REAPER : les JSFX installes, la chaine lue, les
//! valeurs, les liens ecrits (seulement ce qui differe), les effets internes
//! poses et retires (les liens recales par GUID autour), le moteur du pad.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use cp_fxc::adopt::{ReadLink, adopt};
use cp_fxc::links::{Chain, ChainFx, Ctx, LSrc, Link, plan};
use cp_fxc::model::{Nav, pkey, split_pkey};
use cp_fxc::slots::{FxKind, bank, bridge, fx_kind, keys, mix, notes, seq, sg, short_fx_name};
use cp_fxcview::{Bank, FxRow, Js, ParamRow, Slot};
use cp_sys::fxc::Tk;
use cp_sys::reaper::Reaper;

use crate::App;
use cp_sys::module::HostRef;

/// Les JSFX de FX Constellation, tels que CP Studio les garde.
const FILES: [(&str, &str); 7] = [
    (
        "FX Constellation Bridge.jsfx",
        include_str!("fxc_jsfx/FX Constellation Bridge.jsfx"),
    ),
    ("CP_Mod LFO.jsfx", include_str!("fxc_jsfx/CP_Mod LFO.jsfx")),
    (
        "CP_Mod MIDI.jsfx",
        include_str!("fxc_jsfx/CP_Mod MIDI.jsfx"),
    ),
    (
        "FX Constellation - Mod Mix.jsfx",
        include_str!("fxc_jsfx/FX Constellation - Mod Mix.jsfx"),
    ),
    (
        "FX Constellation - Notes.jsfx",
        include_str!("fxc_jsfx/FX Constellation - Notes.jsfx"),
    ),
    (
        "FX Constellation - Sequencer.jsfx",
        include_str!("fxc_jsfx/FX Constellation - Sequencer.jsfx"),
    ),
    (
        "FX Constellation - Sound Generator.jsfx",
        include_str!("fxc_jsfx/FX Constellation - Sound Generator.jsfx"),
    ),
];

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Ecrit les JSFX dans `Effects` s'ils manquent ou different (une fois par
/// session). REAPER les compile au chargement d'un projet.
pub(crate) fn install_jsfx(r: &Reaper, host: &HostRef) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(dir) = r.resource_path().map(|p| p.join("Effects")) else {
        return;
    };
    for (name, src) in FILES {
        let p = dir.join(name);
        let cur = std::fs::read(&p)
            .map(|b| String::from_utf8_lossy(&b).replace('\r', ""))
            .ok();
        if cur.as_deref() == Some(src) {
            continue;
        }
        match std::fs::write(&p, src) {
            Ok(()) => host.info(&format!("FX Constellation : {name} ecrit dans Effects")),
            Err(e) => host.error(&format!("FX Constellation : {name} non ecrit ({e})")),
        }
    }
}

/// Les curseurs courbes : `:log` (le brut est ln v) et `:sqr` (le brut est
/// signe·√|v|).
fn sg_shape(p: usize) -> u8 {
    match p {
        2 | 7 | 21 | 27 | 42 => 1,
        11 | 12 | 14 | 47 | 59 | 60 | 62 | 64 | 65 | 67 | 69 | 70 | 72 | 74 | 75 | 77 | 78 | 80
        | 81 | 83 | 84 => 2,
        _ => 0,
    }
}

fn unshape(shape: u8, v: f64) -> f64 {
    match shape {
        1 => v.exp(),
        2 => v * v.abs(),
        _ => v,
    }
}

fn parse(s: Option<String>) -> Option<f64> {
    s.and_then(|v| v.trim().parse::<f64>().ok())
}

/// Un lien tel qu'il est dans REAPER.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cur {
    active: bool,
    effect: i32,
    param: i32,
    msg: i32,
    msg2: i32,
    offset: f64,
    scale: f64,
    baseline: f64,
    mod_active: bool,
}

impl App {
    fn fxc_tk(&self) -> Option<Tk> {
        self.fxc.tk.filter(|&t| self.r.xc_guid(t).is_some())
    }

    // -----------------------------------------------------------------------
    // La chaine
    // -----------------------------------------------------------------------

    fn fxc_quick_sig(&self, t: Tk) -> String {
        let n = self.r.xc_fx_count(t);
        let mut s = format!("{n};");
        for i in 0..n {
            s.push_str(&self.r.xc_fx_guid(t, i).unwrap_or_default());
            s.push('|');
            s.push_str(&self.r.xc_fx_name(t, i));
            s.push(';');
        }
        s
    }

    /// Relit la chaine de la piste (effets, genres, parametres, pools).
    pub(crate) fn fxc_scan_chain(&mut self) {
        let Some(t) = self.fxc_tk() else {
            self.fxc.chain = Chain::default();
            self.fxc.rows.clear();
            return;
        };
        let r = self.r;
        let n = r.xc_fx_count(t);
        let before = self.fxc.chain.fx.len();
        let mut fx = Vec::with_capacity(n);
        for i in 0..n {
            let name = r.xc_fx_name(t, i);
            fx.push(ChainFx {
                guid: r.xc_fx_guid(t, i).unwrap_or_default(),
                kind: Some(fx_kind(&name)),
                nparams: r.xc_param_count(t, i),
                name,
            });
        }
        let mut detached: Vec<String> = r
            .xc_track_ext(t, keys::DETACHED)
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        // Le prochain effet ajoute va hors de ROLL (le « + » de DETACHED).
        if let Some(k) = self.fxc.next_detached {
            if n > k {
                let known: Vec<&str> = self.fxc.chain.fx.iter().map(|f| f.guid.as_str()).collect();
                for f in &fx {
                    if !known.contains(&f.guid.as_str()) && fx_kind(&f.name) == FxKind::User {
                        detached.push(f.guid.clone());
                    }
                }
                self.fxc.next_detached = None;
                let mut d = detached.clone();
                d.sort();
                d.dedup();
                r.xc_set_track_ext(t, keys::DETACHED, &d.join(" "));
            }
        }
        let _ = before;
        self.fxc.sig = self.fxc_quick_sig(t);
        self.fxc.chain = Chain { fx };
        // Les rangees : les effets de l'utilisateur.
        let mut rows = Vec::new();
        for (i, f) in self.fxc.chain.fx.clone().iter().enumerate() {
            if f.kind != Some(FxKind::User) {
                continue;
            }
            let key = (f.guid.clone(), f.nparams);
            if !self.fxc.names.contains_key(&key) {
                let mut v = Vec::with_capacity(f.nparams);
                for k in 0..f.nparams {
                    let name = r.xc_param_name(t, i, k);
                    let steps = if r.xc_is_toggle(t, i, k) { 2 } else { 0 };
                    v.push((name, steps));
                }
                self.fxc.names.insert(key.clone(), v);
            }
            let names = &self.fxc.names[&key];
            let tail = f.nparams.saturating_sub(3);
            let words = &self.fxc.hide_words;
            let show_hidden = self.fxc.show_hidden;
            let model = &self.fxc.t;
            let mut hidden = 0usize;
            let params = names
                .iter()
                .enumerate()
                .filter(|(k, (nm, _))| {
                    // Les parametres que REAPER ajoute a tout effet, et les
                    // vu-metres.
                    !(*k >= tail && matches!(nm.as_str(), "Bypass" | "Wet" | "Delta"))
                        && !nm.to_lowercase().contains("(meter)")
                })
                .filter(|(k, (nm, _))| {
                    // Un mot cache retire le parametre (jamais un suivi).
                    if show_hidden || !words.iter().any(|w| nm.contains(w.as_str())) {
                        return true;
                    }
                    let kept = model
                        .params
                        .get(&pkey(&f.guid, *k))
                        .is_some_and(|p| p.sel || !p.srcs.is_empty());
                    if !kept {
                        hidden += 1;
                    }
                    kept
                })
                .map(|(k, (nm, steps))| ParamRow {
                    key: pkey(&f.guid, k),
                    index: k,
                    name: nm.clone(),
                    live: 0.0,
                    text: String::new(),
                    steps: *steps,
                })
                .collect();
            rows.push(FxRow {
                guid: f.guid.clone(),
                name: short_fx_name(&f.name),
                enabled: r.xc_fx_enabled(t, i),
                detached: detached.contains(&f.guid),
                params,
                hidden,
            });
        }
        self.fxc.rows = rows;
        // Le modele oublie les effets partis.
        let alive: Vec<String> = self.fxc.chain.fx.iter().map(|f| f.guid.clone()).collect();
        self.fxc.t.prune(&|g| alive.iter().any(|a| a == g));
        self.fxc_read_values(true);
        self.fxc.view.mark();
    }

    // -----------------------------------------------------------------------
    // Les valeurs
    // -----------------------------------------------------------------------

    fn fxc_read_bank(&self, t: Tk, fx: usize, full: bool, prev: Option<&Bank>) -> Bank {
        let r = self.r;
        let np = r.xc_param_count(t, fx);
        let mut b = prev.cloned().unwrap_or_else(|| Bank {
            slots: vec![Slot::default(); 8],
            ext: np >= bank::EXT_COUNT,
        });
        b.ext = np >= bank::EXT_COUNT;
        for i in 1..=8 {
            let s = &mut b.slots[i - 1];
            s.out = r.xc_get(t, fx, bank::out(i));
            s.pho = r.xc_get(t, fx, bank::pho(i));
            if !full {
                continue;
            }
            s.on = r.xc_get_raw(t, fx, bank::on(i)).0 > 0.5;
            s.shape = r
                .xc_get_raw(t, fx, bank::shape(i))
                .0
                .round()
                .clamp(0.0, 9.0) as u8;
            s.rate_n = r.xc_get(t, fx, bank::rate(i));
            s.rate_hz = 0.01 * (2000f64).powf(s.rate_n);
            s.sync = r
                .xc_get_raw(t, fx, bank::sync(i))
                .0
                .round()
                .clamp(0.0, 10.0) as u8;
            s.phase = r.xc_get_raw(t, fx, bank::phase(i)).0;
            if np >= bank::EXT_COUNT {
                s.slew_n = r.xc_get(t, fx, bank::slew(i));
                s.slew_s = 2.0 * s.slew_n * s.slew_n;
                s.curve = r.xc_get_raw(t, fx, bank::curve(i)).0;
                s.mode = r.xc_get_raw(t, fx, bank::mode(i)).0.round().clamp(0.0, 2.0) as u8;
            }
            if np >= bank::UNI_COUNT {
                s.uni = r.xc_get_raw(t, fx, bank::uni(i)).0 > 0.5;
            }
            if np >= bank::SH_COUNT {
                s.sh = r.xc_get_raw(t, fx, bank::sh(i)).0.round().clamp(0.0, 8.0) as u8;
            }
            if np >= bank::ST_COUNT {
                let v = [0, 1, 2, 3].map(|k| r.xc_get_raw(t, fx, bank::steps(i, k)).0);
                s.steps = bank::unpack_steps(v);
                s.len = r.xc_get_raw(t, fx, bank::len(i)).0.round().clamp(1.0, 16.0) as u8;
            } else {
                s.len = 16;
            }
        }
        b
    }

    fn fxc_read_js(
        &self,
        t: Tk,
        fx: usize,
        shaped: bool,
        full: bool,
        prev: Option<&Js>,
        outs: &[usize],
    ) -> Js {
        let r = self.r;
        let np = r.xc_param_count(t, fx);
        let mut j = prev.cloned().unwrap_or_default();
        if j.raw.len() != np {
            j.raw = vec![0.0; np];
            j.norm = vec![0.0; np];
        }
        j.enabled = r.xc_fx_enabled(t, fx);
        let read = |p: usize, j: &mut Js| {
            let (v, _, _) = r.xc_get_raw(t, fx, p);
            j.raw[p] = if shaped { unshape(sg_shape(p), v) } else { v };
            j.norm[p] = r.xc_get(t, fx, p);
        };
        if full {
            for p in 0..np {
                read(p, &mut j);
            }
        } else {
            for &p in outs {
                if p < np {
                    read(p, &mut j);
                }
            }
        }
        j
    }

    /// Lit ce que la vue montre ; `full` : tout (sinon les sorties seules).
    pub(crate) fn fxc_read_values(&mut self, full: bool) {
        let Some(t) = self.fxc_tk() else { return };
        let r = self.r;
        let chain = self.fxc.chain.clone();
        // Les banques.
        self.fxc.bank = chain.find(FxKind::Bank).map(|fx| {
            let prev = self.fxc.bank.take();
            self.fxc_read_bank(t, fx, full || prev.is_none(), prev.as_ref())
        });
        if full || self.fxc.ticks % 30 == 0 {
            let gb = self.fxc_global_bank();
            self.fxc.gbank = gb.map(|(mt, fx)| {
                let prev = self.fxc.gbank.take();
                self.fxc_read_bank(mt, fx, true, prev.as_ref())
            });
        }
        self.fxc.sg = chain.find(FxKind::SoundGen).map(|fx| {
            let prev = self.fxc.sg.take();
            let f = full || prev.is_none();
            self.fxc_read_js(t, fx, true, f, prev.as_ref(), &[31, 52, 63, 68, 73])
        });
        self.fxc.seq = chain.find(FxKind::Seq).map(|fx| {
            let prev = self.fxc.seq.take();
            let f = full || prev.is_none();
            self.fxc_read_js(
                t,
                fx,
                false,
                f,
                prev.as_ref(),
                &[12, 13, 14, 15, 16, 17, 18, 219, 220, 221],
            )
        });
        // Les macros et la position du pad.
        if let Some(bfx) = chain.find(FxKind::Bridge) {
            for (n, m) in self.fxc.macros.iter_mut().enumerate() {
                *m = r.xc_get_raw(t, bfx, bridge::MACRO_IN + n).0;
            }
        }
        // Les sources, vives.
        let mut src = BTreeMap::new();
        if let Some(b) = &self.fxc.bank {
            for (i, s) in b.slots.iter().enumerate() {
                src.insert(i as i32 + 1, s.out);
            }
        }
        if let Some(b) = &self.fxc.gbank {
            for (i, s) in b.slots.iter().enumerate() {
                src.insert(101 + i as i32, s.out);
            }
        }
        if let Some(j) = &self.fxc.sg {
            for (i, p) in sg::ENV_OUT.iter().enumerate() {
                src.insert(201 + i as i32, j.r(*p));
            }
        }
        if let Some(j) = &self.fxc.seq {
            src.insert(221, j.r(seq::MOD_OUT[0]));
            src.insert(222, j.r(seq::MOD_OUT[1]));
        }
        if let Some(bfx) = chain.find(FxKind::Bridge) {
            for n in 0..8 {
                src.insert(
                    211 + n as i32,
                    r.xc_get_raw(t, bfx, bridge::MACRO_OUT + n).0,
                );
            }
            src.insert(-1, r.xc_get_raw(t, bfx, bridge::X_OUT).0);
            src.insert(-2, r.xc_get_raw(t, bfx, bridge::Y_OUT).0);
        }
        if let Some(nfx) = chain.find(FxKind::Notes) {
            for k in 0..4 {
                src.insert(231 + k as i32, r.xc_get_raw(t, nfx, k).0);
            }
        }
        self.fxc.src = src;
        // Les parametres : la valeur jouee.
        // Les rangees a l'ecran et les parametres suivis souvent ; les
        // autres une fois par seconde (une chaine peut en avoir des
        // milliers).
        let visible: std::collections::HashSet<&str> = self
            .fxc
            .view
            .items()
            .iter()
            .filter_map(|i| match &i.ctl {
                cp_fxcview::Ctl::Knob(k) => Some(k.as_str()),
                _ => None,
            })
            .collect();
        let text_tick = full || self.fxc.ticks % 10 == 0;
        let all = full || self.fxc.ticks % 30 == 0;
        let dests: std::collections::HashSet<(usize, usize)> = self
            .fxc
            .plan
            .links
            .iter()
            .map(|l| (l.fx, l.param))
            .collect();
        let mut rows = std::mem::take(&mut self.fxc.rows);
        for row in rows.iter_mut() {
            let Some(fx) = chain.index_of(&row.guid) else {
                continue;
            };
            row.enabled = r.xc_fx_enabled(t, fx);
            for p in row.params.iter_mut() {
                let tracked = self.fxc.t.params.get(&p.key).is_some_and(|s| s.sel);
                let shown = visible.contains(p.key.as_str());
                if !(all || tracked || (shown && self.fxc.ticks % 3 == 0)) {
                    continue;
                }
                let text_too = text_tick && (shown || full);
                let read = r.xc_get(t, fx, p.index);
                self.fxc.scan.entry(p.key.clone()).or_insert(read);
                p.live = self.fxc_live(&p.key, read, dests.contains(&(fx, p.index)));
                if text_too {
                    p.text = r
                        .xc_format_at(t, fx, p.index, p.live)
                        .unwrap_or_else(|| r.xc_format(t, fx, p.index));
                }
            }
        }
        self.fxc.rows = rows;
    }

    /// Relit un seul effet interne (le geste qui le regle tourne a la
    /// cadence de la souris : pas toute la chaine a chaque mouvement).
    pub(crate) fn fxc_refresh_js(&mut self, kind: FxKind) {
        let Some(t) = self.fxc_tk() else { return };
        let Some(fx) = self.fxc.chain.find(kind) else {
            return;
        };
        match kind {
            FxKind::SoundGen => {
                let prev = self.fxc.sg.take();
                let j = self.fxc_read_js(t, fx, true, true, prev.as_ref(), &[31, 52, 63, 68, 73]);
                for (i, p) in sg::ENV_OUT.iter().enumerate() {
                    self.fxc.src.insert(201 + i as i32, j.r(*p));
                }
                self.fxc.sg = Some(j);
            }
            FxKind::Seq => {
                let prev = self.fxc.seq.take();
                let j = self.fxc_read_js(
                    t,
                    fx,
                    false,
                    true,
                    prev.as_ref(),
                    &[12, 13, 14, 15, 16, 17, 18, 219, 220, 221],
                );
                self.fxc.src.insert(221, j.r(seq::MOD_OUT[0]));
                self.fxc.src.insert(222, j.r(seq::MOD_OUT[1]));
                self.fxc.seq = Some(j);
            }
            FxKind::Bank => {
                let prev = self.fxc.bank.take();
                let b = self.fxc_read_bank(t, fx, true, prev.as_ref());
                for (i, s) in b.slots.iter().enumerate() {
                    self.fxc.src.insert(i as i32 + 1, s.out);
                }
                self.fxc.bank = Some(b);
            }
            _ => {}
        }
        self.fxc.view.mark();
    }

    /// Le pad bouge-t-il sans la souris (marche, figure, axe mene, pad
    /// lisse qui rattrape) ?
    pub(crate) fn fxc_animating(&self) -> bool {
        let p = &self.fxc.t.pad;
        (p.nav_on && p.nav != Nav::Manual && !p.granular)
            || self.fxc.plan.pad_link_x
            || self.fxc.plan.pad_link_y
            || self.fxc.pad_target.is_some()
    }

    /// Une image de l'ecran : ce qui bouge seul (le pad en marche, lu sur le
    /// bridge ; le pad lisse) avance d'une image, et les boutons qu'il mene
    /// suivent. Rend vrai tant que ca bouge.
    pub(crate) fn fxc_animate(&mut self) -> bool {
        let now = std::time::Instant::now();
        let dt = self
            .fxc
            .last_frame
            .map_or(1.0 / 60.0, |t| (now - t).as_secs_f64())
            .clamp(0.001, 0.05);
        self.fxc.last_frame = Some(now);
        if !self.fxc_animating() {
            self.fxc.last_frame = None;
            return false;
        }
        let Some(t) = self.fxc_tk() else {
            return false;
        };
        let mut moved = false;
        // Le pad lisse rattrape la souris, au pas de l'image.
        if let Some(tg) = self.fxc.pad_target {
            let (lx, ly) = (self.fxc.plan.pad_link_x, self.fxc.plan.pad_link_y);
            let p = &self.fxc.t.pad;
            if (lx && ly) || p.granular || p.smooth <= 0.0 {
                self.fxc.pad_target = None;
            } else {
                let (x, y) = cp_fxc::pad::smooth_step((p.x, p.y), tg, p, dt);
                self.fxc_pad_to(x, y);
                let ex = if lx { 0.0 } else { (x - tg.0).abs() };
                let ey = if ly { 0.0 } else { (y - tg.1).abs() };
                if ex + ey < 0.001 {
                    self.fxc.pad_target = None;
                }
                moved = true;
            }
        }
        // La marche, la figure, un axe mene : la position que joue le
        // bridge (il la calcule dans le fil audio).
        let p = &self.fxc.t.pad;
        let runs = p.nav_on && p.nav != Nav::Manual && !p.granular;
        let (lx, ly) = (self.fxc.plan.pad_link_x, self.fxc.plan.pad_link_y);
        if runs || lx || ly {
            if let Some(b) = self.fxc.chain.find(FxKind::Bridge) {
                let x = self.r.xc_get_raw(t, b, bridge::X_OUT).0.clamp(0.0, 1.0);
                let y = self.r.xc_get_raw(t, b, bridge::Y_OUT).0.clamp(0.0, 1.0);
                let pad = &mut self.fxc.t.pad;
                if (x - pad.x).abs() > 1e-4 || (y - pad.y).abs() > 1e-4 {
                    if runs || lx {
                        pad.x = x;
                    }
                    if runs || ly {
                        pad.y = y;
                    }
                    self.fxc.src.insert(-1, x);
                    self.fxc.src.insert(-2, y);
                    moved = true;
                }
            }
        }
        if moved {
            self.fxc.anim_until = Some(now + std::time::Duration::from_millis(600));
            self.fxc_relive();
            self.fxc.view.mark();
        }
        // Tant que ca bouge (et un peu apres : le bridge n'avance qu'a chaque
        // bloc audio), ou que le pad rattrape la souris.
        self.fxc.pad_target.is_some() || self.fxc.anim_until.is_some_and(|t| now < t)
    }

    /// Les valeurs jouees des parametres a nous, recalculees depuis le
    /// modele (sans relire l'effet) : le pad vient de bouger.
    pub(crate) fn fxc_relive(&mut self) {
        let dests: std::collections::HashSet<(usize, usize)> = self
            .fxc
            .plan
            .links
            .iter()
            .map(|l| (l.fx, l.param))
            .collect();
        let mut rows = std::mem::take(&mut self.fxc.rows);
        for row in rows.iter_mut() {
            let Some(fx) = self.fxc.chain.index_of(&row.guid) else {
                continue;
            };
            for p in row.params.iter_mut() {
                let linked = dests.contains(&(fx, p.index));
                if linked && self.fxc.t.params.get(&p.key).is_some_and(|s| s.sel) {
                    p.live = self.fxc_live(&p.key, p.live, true);
                }
            }
        }
        self.fxc.rows = rows;
    }

    /// La banque globale relue seule.
    pub(crate) fn fxc_refresh_gbank(&mut self) {
        let gb = self.fxc_global_bank();
        self.fxc.gbank = gb.map(|(mt, fx)| {
            let prev = self.fxc.gbank.take();
            self.fxc_read_bank(mt, fx, true, prev.as_ref())
        });
        if let Some(b) = &self.fxc.gbank {
            for (i, s) in b.slots.iter().enumerate() {
                self.fxc.src.insert(101 + i as i32, s.out);
            }
        }
        self.fxc.view.mark();
    }

    /// La valeur qu'un parametre joue : un lien rend sa base a
    /// `GetParamNormalized`, la modulation se recalcule depuis ses sources.
    fn fxc_live(&self, key: &str, read: f64, has_link: bool) -> f64 {
        let Some(ps) = self.fxc.t.params.get(key) else {
            return read;
        };
        if !ps.sel {
            return read;
        }
        let pad = &self.fxc.t.pad;
        let g = pad.gesture_range;
        if !has_link {
            return read;
        }
        if ps.srcs.is_empty() {
            return cp_fxc::pad::pad_value(ps.base, ps.span(0, g), ps.x, ps.y, pad);
        }
        let mut sum = 0.0;
        for (i, s) in ps.srcs.iter().enumerate() {
            let Some(v) = self.fxc.src.get(&s.slot) else {
                continue;
            };
            let uni = match s.slot {
                1..=8 => self
                    .fxc
                    .bank
                    .as_ref()
                    .is_some_and(|b| b.slots[(s.slot - 1) as usize].uni),
                101..=108 => self
                    .fxc
                    .gbank
                    .as_ref()
                    .is_some_and(|b| b.slots[(s.slot - 101) as usize].uni),
                _ => false,
            };
            let c = s
                .pol
                .map_or(cp_fxc::slots::source_center(s.slot, uni), |p| p.center());
            sum += self.fxc_depth_live(ps.span_eff(i, g), s, g) * (v - c);
        }
        if self.fxc.plan.bus_of.contains_key(key) {
            if !self.fxc.plan.standby.contains(key)
                && !pad.granular
                && (ps.x || ps.y)
                && ps.srcs.len() < 4
            {
                let dx = pad.x - pad.ax;
                let dy = pad.y - pad.ay;
                let d = match (ps.x, ps.y) {
                    (true, true) => (dx + dy) / 2.0,
                    (true, false) => dx,
                    _ => dy,
                };
                sum += ps.span(0, g) * d;
            }
            sum = sum.clamp(-1.0, 1.0);
        }
        (ps.base + sum).clamp(0.0, 1.0)
    }

    /// La profondeur jouee d'un lien : la sienne, et ce que ses modulateurs
    /// y ajoutent (en profondeur).
    fn fxc_depth_live(&self, base: f64, s: &cp_fxc::model::Src, g: f64) -> f64 {
        if s.off {
            return 0.0;
        }
        let mut d = base;
        for dm in &s.dmods {
            let Some(v) = self.fxc.src.get(&dm.slot) else {
                continue;
            };
            let c = dm
                .pol
                .map_or(cp_fxc::slots::source_center(dm.slot, false), |p| p.center());
            let amt = if dm.off { 0.0 } else { dm.depth * g };
            d += self.fxc_depth_live(amt, dm, g) * (v - c);
        }
        d
    }

    // -----------------------------------------------------------------------
    // La banque globale (piste CP MOD)
    // -----------------------------------------------------------------------

    /// La piste CP MOD et sa banque, si elles existent.
    pub(crate) fn fxc_global_bank(&self) -> Option<(Tk, usize)> {
        let mt = self.r.xc_find_track_named(bank::MOD_TRACK)?;
        let n = self.r.xc_fx_count(mt);
        (0..n)
            .find(|&i| self.r.xc_fx_name(mt, i).contains(bank::MIDI_NAME))
            .map(|i| (mt, i))
    }

    /// Cree la piste CP MOD (cachee, sans envoi au master) et sa banque.
    pub(crate) fn fxc_ensure_global_bank(&mut self) -> Option<(Tk, usize)> {
        if let Some(g) = self.fxc_global_bank() {
            return Some(g);
        }
        let mt = match self.r.xc_find_track_named(bank::MOD_TRACK) {
            Some(t) => t,
            None => {
                let t = self.r.xc_insert_track_end()?;
                self.r.xc_set_track_name(t, bank::MOD_TRACK);
                self.r.xc_set_track_value(t, "B_SHOWINTCP", 0.0);
                self.r.xc_set_track_value(t, "B_SHOWINMIXER", 0.0);
                self.r.xc_set_track_value(t, "B_MAINSEND", 0.0);
                t
            }
        };
        let i = self.fxc_quiet_add(mt, bank::MIDI_ADD, None)?;
        self.fxc_count("global_bank");
        Some((mt, i))
    }

    /// La piste montree recoit la banque globale (un envoi MIDI seul) ; un
    /// envoi neuf fait re-emettre les valeurs.
    pub(crate) fn fxc_ensure_global_send(&mut self) {
        let Some(t) = self.fxc_tk() else { return };
        let Some((mt, fx)) = self.fxc_ensure_global_bank() else {
            return;
        };
        if mt == t {
            return;
        }
        if self.r.xc_ensure_midi_send(mt, t) {
            // Toute ecriture d'un curseur fait re-emettre les 8 valeurs.
            let v = self.r.xc_get_raw(mt, fx, bank::on(1)).0;
            self.r.xc_set_raw(mt, fx, bank::on(1), v);
        }
    }

    // -----------------------------------------------------------------------
    // Poser et retirer un effet interne
    // -----------------------------------------------------------------------

    /// Ajoute un effet sans que sa fenetre ne s'ouvre ; rend sa place.
    pub(crate) fn fxc_quiet_add(&mut self, t: Tk, name: &str, at: Option<usize>) -> Option<usize> {
        let old = self.r.config_int("fxfloat_focus");
        if let Some(v) = old {
            self.r.set_config_int("fxfloat_focus", v & !1);
        }
        let i = self.r.xc_fx_add(t, name, at);
        if let Some(v) = old {
            self.r.set_config_int("fxfloat_focus", v);
        }
        if let Some(i) = i {
            self.r.xc_fx_float(t, i, false);
        }
        i
    }

    /// Les liens de la piste, par GUID (avant de changer la chaine).
    pub(crate) fn fxc_snapshot_links(&self, t: Tk) -> Vec<(String, usize, String)> {
        let r = self.r;
        let n = r.xc_fx_count(t);
        let guids: Vec<String> = (0..n)
            .map(|i| r.xc_fx_guid(t, i).unwrap_or_default())
            .collect();
        let mut out = Vec::new();
        for (i, g) in guids.iter().enumerate() {
            for p in 0..r.xc_param_count(t, i) {
                if r.xc_cfg(t, i, &format!("param.{p}.plink.active"))
                    .as_deref()
                    != Some("1")
                {
                    continue;
                }
                let e = parse(r.xc_cfg(t, i, &format!("param.{p}.plink.effect"))).unwrap_or(-1.0)
                    as i64;
                if e >= 0 && (e as usize) < n {
                    out.push((g.clone(), p, guids[e as usize].clone()));
                }
            }
        }
        out
    }

    pub(crate) fn fxc_repair_links(&self, t: Tk, snap: &[(String, usize, String)]) {
        let r = self.r;
        for (g, p, src) in snap {
            let (Some(i), Some(s)) = (r.xc_fx_index(t, g), r.xc_fx_index(t, src)) else {
                continue;
            };
            let key = format!("param.{p}.plink.effect");
            let cur = parse(r.xc_cfg(t, i, &key)).unwrap_or(-1.0) as i64;
            if cur != s as i64 {
                r.xc_set_cfg(t, i, &key, &s.to_string());
            }
        }
    }

    /// Ajoute un effet interne a la place `at`, les liens recales.
    pub(crate) fn fxc_insert(&mut self, name: &str, at: Option<usize>) -> Option<usize> {
        let t = self.fxc_tk()?;
        let snap = self.fxc_snapshot_links(t);
        let i = self.fxc_quiet_add(t, name, at);
        self.fxc_repair_links(t, &snap);
        self.fxc_scan_chain();
        self.fxc.links_dirty = true;
        i
    }

    /// Retire un effet interne, sauf si quelque chose mourrait avec lui
    /// (une enveloppe, un reglage module, un lien dont il est la source).
    pub(crate) fn fxc_retire(&mut self, fx: usize) -> Result<(), String> {
        let t = self.fxc_tk().ok_or("no track")?;
        let r = self.r;
        let np = r.xc_param_count(t, fx);
        for p in 0..np {
            if r.xc_has_envelope(t, fx, p) {
                return Err("it has automation".into());
            }
            if r.xc_cfg(t, fx, &format!("param.{p}.plink.active"))
                .as_deref()
                == Some("1")
                && fx_kind(&r.xc_fx_name(t, fx)) != FxKind::Mix
            {
                return Err("one of its controls is modulated".into());
            }
        }
        let n = r.xc_fx_count(t);
        for i in (0..n).filter(|&i| i != fx) {
            for p in 0..r.xc_param_count(t, i) {
                if r.xc_cfg(t, i, &format!("param.{p}.plink.active"))
                    .as_deref()
                    == Some("1")
                    && parse(r.xc_cfg(t, i, &format!("param.{p}.plink.effect"))).map(|e| e as i64)
                        == Some(fx as i64)
                {
                    return Err("it drives a link".into());
                }
            }
        }
        let guid = r.xc_fx_guid(t, fx).unwrap_or_default();
        let snap: Vec<_> = self
            .fxc_snapshot_links(t)
            .into_iter()
            .filter(|(g, _, s)| *g != guid && *s != guid)
            .collect();
        if !r.xc_fx_delete(t, fx) || r.xc_fx_count(t) != n - 1 {
            return Err("REAPER did not remove it".into());
        }
        self.fxc_repair_links(t, &snap);
        self.fxc_scan_chain();
        self.fxc.links_dirty = true;
        Ok(())
    }

    /// La banque locale (posee au premier geste qui en a besoin).
    pub(crate) fn fxc_ensure_bank(&mut self) -> Option<usize> {
        if let Some(i) = self.fxc.chain.find(FxKind::Bank) {
            return Some(i);
        }
        let i = self.fxc_insert(bank::LFO_ADD, None)?;
        self.fxc_count("bank");
        self.fxc_read_values(true);
        Some(i)
    }

    /// Le bridge (le pad et les macros), pose a la fin de la chaine.
    pub(crate) fn fxc_ensure_bridge(&mut self) -> Option<usize> {
        if let Some(i) = self.fxc.chain.find(FxKind::Bridge) {
            // Un bridge d'avant sert tel qu'il est : le pad des 6 curseurs,
            // les macros des 22, le moteur des 31, la zone des 35. Le refaire
            // perdrait ses enveloppes et ses liens.
            return Some(i);
        }
        let t = self.fxc_tk()?;
        let at = self.r.xc_fx_count(t);
        let i = self.fxc_insert(bridge::ADD, Some(at))?;
        // Un bridge neuf : au centre, la zone entiere.
        self.r.xc_set_raw(t, i, bridge::X_POS, self.fxc.t.pad.x);
        self.r.xc_set_raw(t, i, bridge::Y_POS, self.fxc.t.pad.y);
        self.fxc.eng_cache.clear();
        self.fxc_count("bridge");
        Some(i)
    }

    // -----------------------------------------------------------------------
    // L'adoption
    // -----------------------------------------------------------------------

    /// Lit un lien.
    fn fxc_cur(&self, t: Tk, fx: usize, p: usize) -> Cur {
        let r = self.r;
        let k = |s: &str| format!("param.{p}.{s}");
        let active = r.xc_cfg(t, fx, &k("plink.active")).as_deref() == Some("1");
        if !active {
            return Cur {
                mod_active: r.xc_cfg(t, fx, &k("mod.active")).as_deref() == Some("1"),
                ..Cur::default()
            };
        }
        Cur {
            active,
            effect: parse(r.xc_cfg(t, fx, &k("plink.effect"))).unwrap_or(-1.0) as i32,
            param: parse(r.xc_cfg(t, fx, &k("plink.param"))).unwrap_or(-1.0) as i32,
            msg: parse(r.xc_cfg(t, fx, &k("plink.midi_msg"))).unwrap_or(-1.0) as i32,
            msg2: parse(r.xc_cfg(t, fx, &k("plink.midi_msg2"))).unwrap_or(-1.0) as i32,
            offset: parse(r.xc_cfg(t, fx, &k("plink.offset"))).unwrap_or(0.0),
            scale: parse(r.xc_cfg(t, fx, &k("plink.scale"))).unwrap_or(1.0),
            baseline: parse(r.xc_cfg(t, fx, &k("mod.baseline"))).unwrap_or(0.0),
            mod_active: r.xc_cfg(t, fx, &k("mod.active")).as_deref() == Some("1"),
        }
    }

    fn cur_src(c: &Cur) -> LSrc {
        if c.effect == -100 {
            // Seule la paire 14 bits d'un CC 16..23 vient de la banque
            // globale ; un autre lien MIDI (un bouton materiel) n'est pas une
            // de nos sources (255 : rien).
            let ours = c.msg == 176 && (128 + 16..=128 + 23).contains(&c.msg2);
            LSrc::Midi {
                cc: if ours { (c.msg2 - 128) as u8 } else { 255 },
            }
        } else {
            LSrc::Fx {
                fx: c.effect.max(0) as usize,
                param: c.param.max(0) as usize,
            }
        }
    }

    /// Refait le modele de la piste depuis ses liens et son bridge.
    pub(crate) fn fxc_adopt(&mut self) -> usize {
        let Some(t) = self.fxc_tk() else { return 0 };
        let r = self.r;
        let chain = self.fxc.chain.clone();
        let mut links = Vec::new();
        for (i, f) in chain.fx.iter().enumerate() {
            let k = f.kind.unwrap_or(FxKind::User);
            let range: Vec<usize> = match k {
                FxKind::User | FxKind::SoundGen | FxKind::Seq => (0..f.nparams).collect(),
                FxKind::Mix => (16..80)
                    .chain(mix::dm_in(1)..=mix::dm_in(mix::VOICES))
                    .collect(),
                FxKind::Bridge => [bridge::X_POS, bridge::Y_POS]
                    .into_iter()
                    .chain(bridge::MACRO_IN..bridge::MACRO_IN + 8)
                    .filter(|&p| p < f.nparams)
                    .collect(),
                FxKind::Bank => (1..=8)
                    .flat_map(|i| [bank::rate(i), bank::phase(i), bank::curve(i), bank::slew(i)])
                    .filter(|&p| p < f.nparams)
                    .collect(),
                _ => continue,
            };
            for p in range {
                let c = self.fxc_cur(t, i, p);
                if !c.active {
                    continue;
                }
                links.push(ReadLink {
                    fx: i,
                    param: p,
                    src: Self::cur_src(&c),
                    offset: c.offset,
                    scale: c.scale,
                    baseline: c.baseline,
                });
            }
        }
        let raw = |fx: usize, p: usize| r.xc_get_raw(t, fx, p).0;
        let uni = self
            .fxc
            .bank
            .as_ref()
            .map(|b| std::array::from_fn(|i| b.slots[i].uni))
            .unwrap_or([false; 8]);
        let guni = self
            .fxc
            .gbank
            .as_ref()
            .map(|b| std::array::from_fn(|i| b.slots[i].uni))
            .unwrap_or([false; 8]);
        let n = adopt(&mut self.fxc.t, &chain, &links, &raw, &uni, &guni);
        // Le pad : ce que le bridge tient.
        if let Some(bfx) = chain.find(FxKind::Bridge) {
            let np = r.xc_param_count(t, bfx);
            let pad = &mut self.fxc.t.pad;
            pad.x = r.xc_get_raw(t, bfx, bridge::X_OUT).0.clamp(0.0, 1.0);
            pad.y = r.xc_get_raw(t, bfx, bridge::Y_OUT).0.clamp(0.0, 1.0);
            if np >= 31 {
                let mode = r.xc_get_raw(t, bfx, bridge::ENG_MODE).0.round() as i64;
                if mode > 0 {
                    pad.nav = Nav::from_i(mode);
                    pad.nav_on = true;
                }
                pad.walk_speed = r.xc_get_raw(t, bfx, bridge::WALK_RATE).0.clamp(0.1, 20.0);
                pad.walk_jitter = r.xc_get_raw(t, bfx, bridge::WALK_JIT).0.clamp(0.0, 1.0);
                pad.walk_jump = r.xc_get_raw(t, bfx, bridge::WALK_JUMP).0 > 0.5;
                pad.walk_sync = r
                    .xc_get_raw(t, bfx, bridge::WALK_SYNC)
                    .0
                    .round()
                    .clamp(0.0, 5.0) as u8;
                pad.fig_mode = r
                    .xc_get_raw(t, bfx, bridge::FIG_MODE)
                    .0
                    .round()
                    .clamp(0.0, 5.0) as u8;
                pad.fig_speed = r.xc_get_raw(t, bfx, bridge::FIG_RATE).0.clamp(0.01, 10.0);
                pad.fig_size = r.xc_get_raw(t, bfx, bridge::FIG_SIZE).0.clamp(0.1, 1.0);
            }
            if np >= 35 {
                for (k, z) in pad.zone.iter_mut().enumerate() {
                    *z = r.xc_get_raw(t, bfx, bridge::ZONE + k).0.clamp(0.0, 1.0);
                }
            }
        }
        n
    }

    // -----------------------------------------------------------------------
    // Les liens
    // -----------------------------------------------------------------------

    fn fxc_write_link(&self, t: Tk, l: &Link) {
        let r = self.r;
        let k = |s: &str| format!("param.{}.{s}", l.param);
        match l.src {
            LSrc::Fx { fx, param } => {
                r.xc_set_cfg(t, l.fx, &k("plink.effect"), &fx.to_string());
                r.xc_set_cfg(t, l.fx, &k("plink.param"), &param.to_string());
            }
            LSrc::Midi { cc } => {
                r.xc_set_cfg(t, l.fx, &k("plink.effect"), "-100");
                r.xc_set_cfg(t, l.fx, &k("plink.midi_bus"), "0");
                r.xc_set_cfg(t, l.fx, &k("plink.midi_chan"), "1");
                r.xc_set_cfg(t, l.fx, &k("plink.midi_msg"), "176");
                r.xc_set_cfg(
                    t,
                    l.fx,
                    &k("plink.midi_msg2"),
                    &(128 + cc as i32).to_string(),
                );
            }
        }
        r.xc_set_cfg(t, l.fx, &k("plink.offset"), &format!("{:.9}", l.offset));
        r.xc_set_cfg(t, l.fx, &k("plink.scale"), &format!("{:.9}", l.scale));
        r.xc_set_cfg(t, l.fx, &k("mod.baseline"), &format!("{:.9}", l.baseline));
        r.xc_set_cfg(t, l.fx, &k("mod.active"), "1");
        r.xc_set_cfg(t, l.fx, &k("plink.active"), "1");
    }

    /// Relache un lien a nous : la valeur revient a sa base.
    fn fxc_release(&self, t: Tk, fx: usize, p: usize, reset: bool) {
        let r = self.r;
        let k = |s: &str| format!("param.{p}.{s}");
        let base = parse(r.xc_cfg(t, fx, &k("mod.baseline")));
        r.xc_set_cfg(t, fx, &k("plink.active"), "0");
        r.xc_set_cfg(t, fx, &k("plink.effect"), "-1");
        let lfo = r.xc_cfg(t, fx, &k("lfo.active")).as_deref() == Some("1");
        let acs = r.xc_cfg(t, fx, &k("acs.active")).as_deref() == Some("1");
        if !lfo && !acs {
            r.xc_set_cfg(t, fx, &k("mod.active"), "0");
        }
        if reset {
            if let Some(b) = base {
                r.xc_set(t, fx, p, b);
            }
        }
    }

    /// Le lien existant est-il a nous ?
    fn fxc_owned(&self, c: &Cur) -> bool {
        if !c.active {
            return false;
        }
        if c.effect == -100 {
            return c.msg == 176 && cp_fxc::links::owned(None, Some(c.msg2));
        }
        cp_fxc::links::owned(
            (c.effect >= 0).then(|| self.fxc.chain.kind(c.effect as usize)),
            None,
        )
    }

    /// Calcule le plan et l'ecrit (seulement ce qui differe).
    pub(crate) fn fxc_apply(&mut self) {
        let Some(t) = self.fxc_tk() else { return };
        let r = self.r;
        let chain = self.fxc.chain.clone();
        let mut ctx = Ctx::default();
        if let Some(b) = &self.fxc.bank {
            ctx.uni = std::array::from_fn(|i| b.slots[i].uni);
        }
        if let Some(b) = &self.fxc.gbank {
            ctx.guni = std::array::from_fn(|i| b.slots[i].uni);
        }
        // Les bus deja tenus (relus des liens).
        if let Some(m) = chain.find(FxKind::Mix) {
            for k in self.fxc.t.params.keys() {
                let Some((g, i)) = split_pkey(k) else {
                    continue;
                };
                let Some(fx) = chain.index_of(g) else {
                    continue;
                };
                let c = self.fxc_cur(t, fx, i);
                if c.active && c.effect == m as i32 && (c.param as usize) < mix::BUSES {
                    ctx.prev_bus.insert(k.clone(), c.param as usize + 1);
                }
            }
        }
        let p = plan(&self.fxc.t, &chain, &ctx);
        let _ = r;
        if p.wants_global {
            self.fxc_ensure_global_send();
        }
        let mut wanted = std::collections::BTreeSet::new();
        let mut p = p;
        let mut links = std::mem::take(&mut p.links);
        for l in links.iter_mut() {
            wanted.insert((l.fx, l.param));
            let gk = chain.fx.get(l.fx).map(|f| pkey(&f.guid, l.param));
            if let Some(k) = &gk {
                self.fxc.linked.insert(k.clone());
            }
            let c = self.fxc_cur(t, l.fx, l.param);
            let same_src = c.active
                && match l.src {
                    LSrc::Fx { fx, param } => c.effect == fx as i32 && c.param == param as i32,
                    LSrc::Midi { cc } => {
                        c.effect == -100 && (c.msg2 == 128 + cc as i32 || c.msg2 == cc as i32)
                    }
                };
            // La base d'un parametre, changee hors de FX Constellation (sa
            // fenetre, un autre script) depuis notre derniere ecriture, et
            // pas par le modele : le lien a raison, le modele la reprend.
            // Le lien porte-t-il la base du parametre (pas une entree de bus,
            // une voie, un axe du pad) ?
            let base_key = gk.as_ref().filter(|k| {
                self.fxc
                    .t
                    .params
                    .get(*k)
                    .is_some_and(|ps| (ps.base - l.baseline).abs() < 1e-9)
            });
            if let (Some(k), true) = (base_key, same_src && c.mod_active) {
                if let Some(&seen) = self.fxc.seen_base.get(k) {
                    if (c.baseline - seen).abs() > 1e-6 && (l.baseline - seen).abs() < 1e-9 {
                        self.fxc.t.param_mut(k).base = c.baseline.clamp(0.0, 1.0);
                        l.baseline = c.baseline;
                        self.fxc.dirty_at = Some(std::time::Instant::now());
                    }
                }
            }
            let same = same_src
                && c.mod_active
                && (c.offset - l.offset).abs() < 1e-6
                && (c.scale - l.scale).abs() < 1e-6
                && (c.baseline - l.baseline).abs() < 1e-6;
            if !same {
                self.fxc_write_link(t, l);
                self.fxc_count("link_write");
                // REAPER refuse un lien sur certains reglages : relu, et un
                // refus rend le parametre « non modulable » (plus suivi).
                if !self.fxc_cur(t, l.fx, l.param).active {
                    if let Some(k) = gk.as_ref().filter(|k| self.fxc.t.params.contains_key(*k)) {
                        self.fxc.unlinkable.insert(k.clone());
                        if let Some(ps) = self.fxc.t.params.get_mut(k) {
                            ps.sel = false;
                            ps.srcs.clear();
                        }
                        let name = r.xc_param_name(t, l.fx, l.param);
                        self.fxc_say(format!("{name}: REAPER refuses to modulate it"));
                        self.fxc.links_dirty = true;
                        self.fxc.dirty_at = Some(std::time::Instant::now());
                    }
                }
            }
            // La base telle que le lien la porte maintenant (ecrite ou vue).
            if let Some(k) = base_key {
                self.fxc.seen_base.insert(k.clone(), l.baseline);
            }
        }
        p.links = links;
        // Ce qui n'est plus voulu : les parametres du modele, et tout lien
        // que nous avons ecrit (le modele a pu les oublier : Init, un
        // preset).
        let mut keys: Vec<String> = self.fxc.t.params.keys().cloned().collect();
        keys.extend(self.fxc.linked.iter().cloned());
        keys.sort();
        keys.dedup();
        for k in &keys {
            let Some((g, i)) = split_pkey(k) else {
                continue;
            };
            let Some(fx) = chain.index_of(g) else {
                continue;
            };
            if wanted.contains(&(fx, i)) {
                continue;
            }
            let c = self.fxc_cur(t, fx, i);
            if self.fxc_owned(&c) {
                self.fxc_release(t, fx, i, true);
                self.fxc_count("link_release");
            }
            self.fxc.linked.remove(k);
            self.fxc.seen_base.remove(k);
        }
        if let Some(m) = chain.find(FxKind::Mix) {
            for pi in (16..80).chain(mix::dm_in(1)..=mix::dm_in(mix::VOICES)) {
                if wanted.contains(&(m, pi)) {
                    continue;
                }
                if r.xc_cfg(t, m, &format!("param.{pi}.plink.active"))
                    .as_deref()
                    == Some("1")
                {
                    self.fxc_release(t, m, pi, false);
                }
            }
        }
        if let Some(b) = chain.find(FxKind::Bridge) {
            for pi in [bridge::X_POS, bridge::Y_POS] {
                if wanted.contains(&(b, pi)) {
                    continue;
                }
                let c = self.fxc_cur(t, b, pi);
                if self.fxc_owned(&c) {
                    self.fxc_release(t, b, pi, false);
                }
            }
        }
        for w in &p.raws {
            let cur = r.xc_get_raw(t, w.fx, w.param).0;
            if (cur - w.value).abs() > 6e-5 {
                r.xc_set_raw(t, w.fx, w.param, w.value);
            }
        }
        // Le lissage du bridge.
        if let Some(b) = chain.find(FxKind::Bridge) {
            let want = (self.fxc.t.pad.link_slew / 2.0).clamp(0.0, 1.0);
            if (r.xc_get(t, b, bridge::SLEW) - want).abs() > 1e-4 {
                r.xc_set(t, b, bridge::SLEW, want);
            }
        }
        self.fxc.plan = p;
    }

    /// Ce qui pose ou retire un effet interne : jamais pendant un geste.
    pub(crate) fn fxc_restructure(&mut self) -> bool {
        let p = self.fxc.plan.clone();
        let guid = self.fxc.guid.clone();
        if p.mix_missing && !self.fxc.refused.contains_key(&format!("{guid}|mix")) {
            let chain = &self.fxc.chain;
            if let Some(i) = chain.find(FxKind::Mix) {
                // Trop court : refait.
                if let Err(e) = self.fxc_retire(i) {
                    self.fxc.refused.insert(format!("{guid}|mix"), e.clone());
                    self.fxc_say(format!("Extra sources stand by: {e}"));
                    return false;
                }
            }
            self.fxc_insert(mix::ADD, Some(0));
            self.fxc_count("mix");
            return true;
        }
        if p.mix_unused && !self.fxc.refused.contains_key(&format!("{guid}|mix_unused")) {
            if let Some(i) = self.fxc.chain.find(FxKind::Mix) {
                if let Err(e) = self.fxc_retire(i) {
                    self.fxc.refused.insert(format!("{guid}|mix_unused"), e);
                } else {
                    self.fxc_count("mix_retire");
                    return true;
                }
            }
        }
        if p.notes_missing && !self.fxc.refused.contains_key(&format!("{guid}|notes")) {
            let at = self.fxc.chain.find(FxKind::Seq).map_or(0, |i| i + 1);
            if let Some(i) = self.fxc_insert(notes::ADD, Some(at)) {
                if let (Some(t), Some(park)) = (
                    self.fxc_tk(),
                    self.fxc_tk()
                        .and_then(|t| self.r.xc_track_ext(t, notes::PARK)),
                ) {
                    for (k, v) in park.split(',').enumerate() {
                        if let Ok(v) = v.trim().parse::<f64>() {
                            self.r.xc_set_raw(t, i, 4 + k, v);
                        }
                    }
                    self.r.xc_set_track_ext(t, notes::PARK, "");
                }
            }
            self.fxc_count("notes");
            return true;
        }
        if p.notes_unused
            && !self
                .fxc
                .refused
                .contains_key(&format!("{guid}|notes_unused"))
        {
            if let (Some(t), Some(i)) = (self.fxc_tk(), self.fxc.chain.find(FxKind::Notes)) {
                let park: Vec<String> = (4..10)
                    .map(|k| format!("{:.6}", self.r.xc_get_raw(t, i, k).0))
                    .collect();
                match self.fxc_retire(i) {
                    Ok(()) => {
                        self.r.xc_set_track_ext(t, notes::PARK, &park.join(","));
                        return true;
                    }
                    Err(e) => {
                        self.fxc.refused.insert(format!("{guid}|notes_unused"), e);
                    }
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Le moteur du pad (dans le bridge)
    // -----------------------------------------------------------------------

    fn fxc_push_engine(&mut self) {
        let Some(t) = self.fxc_tk() else { return };
        let Some(bfx) = self.fxc.chain.find(FxKind::Bridge) else {
            return;
        };
        let np = self.r.xc_param_count(t, bfx);
        if np < bridge::ZONE {
            return;
        }
        let p = &self.fxc.t.pad;
        let run = p.nav_on && p.nav != Nav::Manual && !p.granular;
        let mode = if run { p.nav as u8 as f64 } else { 0.0 };
        let mask = if run {
            (!self.fxc.plan.pad_link_x) as u8 as f64
                + 2.0 * (!self.fxc.plan.pad_link_y) as u8 as f64
        } else {
            0.0
        };
        let z = p.zone_ordered();
        let want = [
            mode,
            mask,
            p.walk_speed.min(20.0),
            p.walk_jitter,
            p.walk_jump as u8 as f64,
            p.walk_sync as f64,
            p.fig_mode as f64,
            p.fig_speed.min(10.0),
            p.fig_size,
            z[0],
            z[1],
            z[2],
            z[3],
        ];
        // Un bridge sans zone (31 curseurs) : le moteur seulement.
        let want = &want[..if np >= bridge::COUNT { want.len() } else { 9 }];
        if self.fxc.eng_cache.len() != want.len() {
            self.fxc.eng_cache = (0..want.len())
                .map(|k| self.r.xc_get_raw(t, bfx, bridge::ENG_MODE + k).0)
                .collect();
        }
        for (k, v) in want.iter().enumerate() {
            if (self.fxc.eng_cache[k] - v).abs() > 1e-6 {
                self.r.xc_set_raw(t, bfx, bridge::ENG_MODE + k, *v);
                self.fxc.eng_cache[k] = *v;
            }
        }
    }

    /// Le battement : la chaine, les valeurs, le moteur, les liens.
    pub(crate) fn fxc_sync_tick(&mut self, down: bool) {
        let Some(t) = self.fxc_tk() else {
            if self.fxc.tk.is_some() {
                if self.fxc.dirty_at.is_some() {
                    self.fxc_save_now();
                }
                self.fxc.tk = None;
                self.fxc.view.mark();
            }
            return;
        };
        if self.fxc.ticks % 6 == 0 && !down {
            let s = self.fxc_quick_sig(t);
            if s != self.fxc.sig {
                self.fxc_scan_chain();
                self.fxc.links_dirty = true;
            }
        }
        let full = self.fxc.ticks % 15 == 0;
        // Le pad lisse rattrape la souris.
        let frames = self.fxc.win.as_ref().is_some_and(|w| w.frame.frames_on);
        if let Some(tg) = self.fxc.pad_target.filter(|_| !frames) {
            let (lx, ly) = (self.fxc.plan.pad_link_x, self.fxc.plan.pad_link_y);
            let p = &self.fxc.t.pad;
            if (lx && ly) || p.granular || p.smooth <= 0.0 {
                self.fxc.pad_target = None;
            } else {
                let (x, y) = cp_fxc::pad::smooth_step((p.x, p.y), tg, p, 1.0 / 30.0);
                self.fxc_pad_to(x, y);
                let ex = if lx { 0.0 } else { (x - tg.0).abs() };
                let ey = if ly { 0.0 } else { (y - tg.1).abs() };
                if ex + ey < 0.001 {
                    self.fxc.pad_target = None;
                }
                self.fxc.view.mark();
            }
        }
        self.fxc_read_values(full);
        self.fxc_keyboard_check_mode();
        self.fxc_push_engine();
        // Le pad suit le bridge quand il marche ou qu'un axe est mene.
        let p = &self.fxc.t.pad;
        let runs = p.nav_on && p.nav != Nav::Manual && !p.granular;
        let (lx, ly) = (self.fxc.plan.pad_link_x, self.fxc.plan.pad_link_y);
        if !down && (runs || lx || ly) {
            // Seuls les axes que le bridge mene (tous s'il marche) : un axe
            // libre que le pad lisse rattrape n'est pas ecrase.
            let x = if runs || lx {
                self.fxc.src.get(&-1).copied().unwrap_or(p.x)
            } else {
                p.x
            };
            let y = if runs || ly {
                self.fxc.src.get(&-2).copied().unwrap_or(p.y)
            } else {
                p.y
            };
            if (x - p.x).abs() > 0.0005 || (y - p.y).abs() > 0.0005 {
                self.fxc.t.pad.x = x;
                self.fxc.t.pad.y = y;
                self.fxc.anim_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(600));
            }
        }
        if self.fxc.links_dirty {
            self.fxc.links_dirty = false;
            self.fxc_apply();
            if !down && self.fxc_restructure() {
                self.fxc_apply();
            }
        } else if !down && self.fxc.ticks % 10 == 5 {
            // Ce que le plan demande a la chaine et qu'un geste a retarde.
            let p = &self.fxc.plan;
            if (p.mix_missing || p.mix_unused || p.notes_missing || p.notes_unused)
                && self.fxc_restructure()
            {
                self.fxc_apply();
            }
        }
        // La fenetre bouge avec ce qui joue.
        let anim = runs
            || self
                .fxc
                .bank
                .as_ref()
                .is_some_and(|b| b.slots.iter().any(|s| s.on))
            || self.fxc.seq.is_some();
        if anim && self.fxc.ticks % 2 == 0 {
            self.fxc.view.mark();
        }
    }

    // -----------------------------------------------------------------------
    // Le clavier de la fenetre (curseurs du Sound Gen, jamais du MIDI)
    // -----------------------------------------------------------------------

    pub(crate) fn fxc_keyboard(&mut self, note: Option<u8>) {
        let Some(t) = self.fxc_tk() else { return };
        let r = self.r;
        if let Some(fx) = self.fxc.chain.find(FxKind::SoundGen) {
            if r.xc_param_count(t, fx) >= 59 {
                match note {
                    Some(n) => {
                        r.xc_set(t, fx, 57, (100.0 - 1.0) / 126.0);
                        r.xc_set(t, fx, 55, n as f64 / 127.0);
                        r.xc_set(t, fx, 56, 1.0);
                    }
                    None => {
                        r.xc_set(t, fx, 56, 0.0);
                    }
                }
            }
        }
        if let Some(fx) = self.fxc.chain.find(FxKind::Notes) {
            match note {
                Some(n) => {
                    r.xc_set_raw(t, fx, notes::KVEL, 100.0);
                    r.xc_set_raw(t, fx, notes::KNOTE, n as f64);
                    r.xc_set_raw(t, fx, notes::KGATE, 1.0);
                }
                None => {
                    r.xc_set_raw(t, fx, notes::KGATE, 0.0);
                }
            }
        }
        self.fxc.kb_note = note;
        self.fxc.view.mark();
    }

    /// Une touche du clavier de la fenetre. En Triggered, on la tient (un
    /// glisser change la note sous la porte) ; en Continuous, un clic
    /// accroche la note, un clic sur la meme la decroche, relacher ne fait
    /// rien (comme l'ancienne version).
    pub(crate) fn fxc_key_gesture(&mut self, n: u8, on: bool) {
        let cont = self.fxc_sg_continuous();
        if !cont {
            self.fxc_keyboard(on.then_some(n));
            self.fxc.kb_cont = false;
            return;
        }
        if !on {
            return;
        }
        if self.fxc.kb_note == Some(n) {
            self.fxc_keyboard(None);
            return;
        }
        // Accrocher est le seul geste qui fait suivre le clavier au bourdon
        // (« Notes »).
        if let (Some(t), Some(fx)) = (self.fxc_tk(), self.fxc.chain.find(FxKind::SoundGen)) {
            if self.r.xc_param_count(t, fx) > sg::CONT_NOTE
                && self.r.xc_get_raw(t, fx, sg::CONT_NOTE).0 < 0.5
            {
                self.r.xc_set_raw(t, fx, sg::CONT_NOTE, 1.0);
            }
        }
        self.fxc_keyboard(Some(n));
        self.fxc.kb_cont = true;
    }

    /// Le Sound Gen est en mode Continuous (sans Sound Gen : non).
    fn fxc_sg_continuous(&self) -> bool {
        match (self.fxc_tk(), self.fxc.chain.find(FxKind::SoundGen)) {
            (Some(t), Some(fx)) => self.r.xc_get_raw(t, fx, 0).0 < 0.5,
            _ => false,
        }
    }

    /// Un changement de mode lache la touche tenue ou accrochee.
    pub(crate) fn fxc_keyboard_check_mode(&mut self) {
        if self.fxc.kb_note.is_none() {
            return;
        }
        let cont = self.fxc.sg.as_ref().is_some_and(|j| j.r(0) < 0.5);
        if cont != self.fxc.kb_cont {
            self.fxc_keyboard(None);
        }
    }

    pub(crate) fn fxc_keyboard_release(&mut self) {
        if self.fxc.kb_note.is_some() {
            self.fxc_keyboard(None);
        }
    }
}

/// Le nom d'un fichier de preset sur le disque.
pub(crate) fn preset_dir(r: &Reaper) -> Option<std::path::PathBuf> {
    r.resource_path()
        .map(|p| p.join("CP_Studio").join("FX Constellation"))
}
