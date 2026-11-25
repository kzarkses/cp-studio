//! L'auto-test de FX Constellation dans le vrai REAPER (`cargo xtask
//! reaper-test`, phase Fxc). Une piste « CPC test » avec ReaEQ : suivre un
//! parametre (le bridge arrive, le lien du pad aussi), bouger le pad, tirer
//! une source (la banque arrive), une deuxieme (le Mod Mix arrive en tete,
//! les liens suivent), ROLL, un preset enregistre puis rappele, le Sound
//! Gen et le sequenceur poses puis retires.

use cp_fxc::model::pkey;
use cp_fxc::slots::{FxKind, bank, bridge, mix};
use cp_fxcview::{Act, Ctl};
use cp_sys::fxc::Tk;
use cp_ui::Mods;

use crate::{App, Place, SECTION};

const TRACK: &str = "CPC test";
const PRESET: &str = "CPC selftest";

impl App {
    pub(crate) fn fxc_selftest(&mut self) {
        let r = self.r;
        let step = r.ext_get(SECTION, "selftest_fxc_step").unwrap_or_default();
        let out = match step.as_str() {
            "run" => self.fxc_selftest_run(),
            "state" => format!("state {}", self.fxc_state_line()),
            "clean" => self.fxc_selftest_clean(),
            "cap_start" => {
                self.fxc.capture.armed = vec![crate::fxc_capture::Cap::PadX];
                self.fxc_ensure_bridge();
                self.fxc_pad_to(0.2, 0.5);
                // 1007 : lecture.
                r.main_on_command(1007);
                "cap started".to_string()
            }
            "cap_move" => {
                self.fxc_pad_to(0.8, 0.5);
                "cap moved".to_string()
            }
            "cap_check" => {
                // 1016 : arret.
                r.main_on_command(1016);
                self.fxc_capture_tick();
                let n = match (self.fxc.tk, self.fxc.chain.find(FxKind::Bridge)) {
                    (Some(t), Some(b)) => r
                        .xc_fx_envelope(t, b, bridge::X_POS, false)
                        .map_or(0, |e| r.xc_env_count(e)),
                    _ => -1,
                };
                self.fxc.capture.armed.clear();
                format!("cap points={n} written={}", self.fxc.capture.points)
            }
            "snap" => {
                // La vraie fenetre, pour la photo de l'auto-test : ouverte,
                // l'onglet des parametres, repeinte en entier.
                if self.fxc.win.is_none() {
                    self.fxc_open();
                }
                self.fxc.view.tab = 0;
                self.fxc.view.mark_full();
                self.fxc_render(true);
                let h = self.fxc.win.as_ref().map_or(0, |w| w.hwnd.0);
                format!("snap hwnd={h}")
            }
            s => format!("unknown step {s}"),
        };
        r.ext_set(SECTION, "selftest_fxc", &out, false);
    }

    fn fxc_selftest_clean(&mut self) -> String {
        if let Some(p) =
            crate::fxc_sync::preset_dir(&self.r).map(|d| d.join(cp_fxc::preset::file_name(PRESET)))
        {
            let _ = std::fs::remove_file(p);
        }
        self.fxc_close(false);
        let r = self.r;
        let mut n = 0;
        while let Some(t) = r.xc_find_track_named(TRACK) {
            r.xc_set_track_value(t, "I_SELECTED", 1.0);
            // 40005 : supprimer les pistes choisies.
            let others: Vec<Tk> = (0..r.xc_track_count())
                .filter_map(|i| r.xc_track_index(i))
                .filter(|&o| o != t)
                .collect();
            for o in others {
                r.xc_set_track_value(o, "I_SELECTED", 0.0);
            }
            r.main_on_command(40005);
            n += 1;
            if n > 4 {
                break;
            }
        }
        format!("cleaned {n}")
    }

    /// La source d'un lien : (effet, parametre), ou -100 pour le MIDI.
    fn fxc_test_link(&self, t: Tk, fx: usize, p: usize) -> Option<(i64, i64)> {
        let r = self.r;
        if r.xc_cfg(t, fx, &format!("param.{p}.plink.active"))
            .as_deref()
            != Some("1")
        {
            return None;
        }
        let e = r
            .xc_cfg(t, fx, &format!("param.{p}.plink.effect"))?
            .parse::<f64>()
            .ok()? as i64;
        let q = r
            .xc_cfg(t, fx, &format!("param.{p}.plink.param"))?
            .parse::<f64>()
            .ok()? as i64;
        Some((e, q))
    }

    fn fxc_test_kind(&self, t: Tk, i: i64) -> String {
        if i < 0 {
            return "-".into();
        }
        format!(
            "{:?}",
            cp_fxc::slots::fx_kind(&self.r.xc_fx_name(t, i as usize))
        )
    }

    fn fxc_selftest_run(&mut self) -> String {
        let r = self.r;
        crate::fxc_sync::install_jsfx(&r, &self.host);
        // La piste du test, choisie, avec ReaEQ.
        let Some(t) = r.xc_insert_track_end() else {
            return "run FAIL no track".into();
        };
        r.xc_set_track_name(t, TRACK);
        for i in 0..r.xc_track_count() {
            if let Some(o) = r.xc_track_index(i) {
                r.xc_set_track_value(o, "I_SELECTED", (o == t) as u8 as f64);
            }
        }
        if r.xc_fx_add(t, "ReaEQ", None).is_none() {
            return "run FAIL ReaEQ".into();
        }
        let eq_guid = r.xc_fx_guid(t, 0).unwrap_or_default();
        if r.xc_fx_add(t, "ReaComp", None).is_none() {
            return "run FAIL ReaComp".into();
        }
        let comp_guid = r.xc_fx_guid(t, 1).unwrap_or_default();
        self.fxc.place = Place::Float;
        self.fxc_open();
        let guid = r.xc_guid(t).unwrap_or_default();
        self.fxc_arrive(Some(t), guid);
        self.fxc.locked = true;
        let k0 = pkey(&eq_guid, 0);
        let k1 = pkey(&eq_guid, 1);
        let mut out = Vec::new();
        let mut ok = true;
        // 1. Suivre un parametre : le bridge arrive, le pad le mene.
        self.fxc_act(Act::Click(Ctl::Sel(k0.clone()), Mods::default()));
        self.fxc_sync_tick(false);
        let bfx = self.fxc.chain.find(FxKind::Bridge);
        let eq = self.fxc.chain.index_of(&eq_guid).unwrap_or(99);
        let l0 = self.fxc_test_link(t, eq, 0);
        let pad_ok = bfx.is_some()
            && l0.is_some_and(|(e, q)| {
                e == bfx.unwrap_or(99) as i64
                    && (bridge::X_OUT..=bridge::XY_MIX).contains(&(q as usize))
            });
        out.push(format!("pad_link={}", pad_ok as u8));
        ok &= pad_ok;
        // 1b. Une base changee hors de FX Constellation (la fenetre de
        //     l'effet, un autre script) : le modele la reprend, il ne
        //     l'ecrase pas.
        let eq = self.fxc.chain.index_of(&eq_guid).unwrap_or(99);
        r.xc_set_cfg(t, eq, "param.0.mod.baseline", "0.770000000");
        self.fxc_apply();
        let model_base = self.fxc.t.params.get(&k0).map_or(-1.0, |p| p.base);
        let link_base = r
            .xc_cfg(t, eq, "param.0.mod.baseline")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(-1.0);
        let pull_ok = (model_base - 0.77).abs() < 1e-6 && (link_base - 0.77).abs() < 1e-6;
        out.push(format!(
            "base_pull={} ({model_base:.3}/{link_base:.3})",
            pull_ok as u8
        ));
        ok &= pull_ok;
        // 2. Le pad bouge : le bridge recoit sa position.
        self.fxc_act(Act::PadPress(0.9, 0.2));
        self.fxc_act(Act::PadRelease);
        let xp = bfx.map_or(-1.0, |b| r.xc_get_raw(t, b, bridge::X_POS).0);
        let pad_moved = (xp - 0.9).abs() < 1e-6;
        out.push(format!("x_pos={xp:.3}"));
        ok &= pad_moved;
        // 3. Une source tiree : la banque arrive, le lien la lit.
        self.fxc_act(Act::Drop(2, Ctl::Knob(k1.clone())));
        self.fxc_sync_tick(false);
        let bk = self.fxc.chain.find(FxKind::Bank);
        let eq = self.fxc.chain.index_of(&eq_guid).unwrap_or(99);
        let l1 = self.fxc_test_link(t, eq, 1);
        let src_ok = bk.is_some() && l1 == Some((bk.unwrap_or(99) as i64, bank::out(2) as i64));
        let on2 = bk.is_some_and(|b| r.xc_get_raw(t, b, bank::on(2)).0 > 0.5);
        out.push(format!("lfo_link={} slot2_on={}", src_ok as u8, on2 as u8));
        ok &= src_ok && on2;
        // 4. Une deuxieme source : le Mod Mix arrive en tete, les liens
        //    suivent (le pad du parametre 0 lit toujours le bridge).
        self.fxc_act(Act::Drop(211, Ctl::Knob(k1.clone())));
        self.fxc_sync_tick(false);
        self.fxc_sync_tick(false);
        let mx = self.fxc.chain.find(FxKind::Mix);
        let eq = self.fxc.chain.index_of(&eq_guid).unwrap_or(99);
        let bfx = self.fxc.chain.find(FxKind::Bridge);
        let l1 = self.fxc_test_link(t, eq, 1);
        let l0 = self.fxc_test_link(t, eq, 0);
        let bus_ok = mx == Some(0) && l1.is_some_and(|(e, q)| e == 0 && (q as usize) < mix::BUSES);
        let in1 = mx.and_then(|m| {
            l1.and_then(|(_, b)| self.fxc_test_link(t, m, mix::input(b as usize + 1, 1)))
        });
        let in_ok = in1.is_some_and(|(e, _)| self.fxc_test_kind(t, e) == "Bank");
        let still = l0.is_some_and(|(e, _)| Some(e as usize) == bfx);
        out.push(format!(
            "mix={} bus_link={} bus_input={} pad_link_after={}",
            mx.map_or(-1, |m| m as i64),
            bus_ok as u8,
            in_ok as u8,
            still as u8
        ));
        ok &= bus_ok && in_ok && still;
        // 4a. Un lien sur un lien : l'LFO 4 module la profondeur de l'LFO 2
        //     sur le parametre 1 (une voie de profondeur du Mod Mix).
        if let Some(s) = self.fxc.t.param_mut(&k1).srcs.get_mut(0) {
            s.dmods.push(cp_fxc::model::Src::new(4, 0.3));
        }
        self.fxc_changed();
        self.fxc_sync_tick(false);
        let mx = self.fxc.chain.find(FxKind::Mix);
        let bk = self.fxc.chain.find(FxKind::Bank);
        let voice = mx.and_then(|m| self.fxc_test_link(t, m, mix::dm_in(1)));
        let tgt = mx.map_or(-1.0, |m| r.xc_get_raw(t, m, mix::dm_tgt(1)).0);
        let amt = mx.map_or(-1.0, |m| r.xc_get_raw(t, m, mix::dm_amt(1)).0);
        let dmod_ok = voice == bk.map(|b| (b as i64, bank::out(4) as i64))
            && (1.0..=64.0).contains(&tgt)
            && (amt - 0.3).abs() < 1e-3;
        out.push(format!(
            "dmod={} dm_tgt={tgt} dm_amt={amt:.3}",
            dmod_ok as u8
        ));
        ok &= dmod_ok;
        // 4b. Une source sur une macro (destination interne) ; une macro
        //     ne se mene pas elle-meme.
        self.fxc_act(Act::Drop(3, Ctl::Macro(1)));
        self.fxc_act(Act::Drop(211, Ctl::Macro(1)));
        self.fxc_sync_tick(false);
        self.fxc_sync_tick(false);
        let bfx = self.fxc.chain.find(FxKind::Bridge);
        let bk = self.fxc.chain.find(FxKind::Bank);
        let lm = bfx.and_then(|b| self.fxc_test_link(t, b, bridge::MACRO_IN));
        let macro_ok = lm == bk.map(|k| (k as i64, bank::out(3) as i64));
        let self_refused = self
            .fxc
            .t
            .params
            .get(&pkey(
                &r.xc_fx_guid(t, bfx.unwrap_or(0)).unwrap_or_default(),
                bridge::MACRO_IN,
            ))
            .is_some_and(|p| p.srcs.len() == 1);
        out.push(format!(
            "macro_link={} self_refused={}",
            macro_ok as u8, self_refused as u8
        ));
        ok &= macro_ok && self_refused;
        // 5. ROLL : rien ne casse, les parametres suivis restent bornes ; et
        //    une annulation ramene le modele d'avant (il vit dans la piste).
        self.fxc_save_now();
        let before_s = self.fxc_stored(t).unwrap_or_default();
        r.xc_undo_point("CPC test: before ROLL");
        let mut before_roll: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        before_roll.sort();
        self.fxc.roll.armed = [true, true, true, true, true, false, false, false];
        self.fxc_roll(None);
        self.fxc_sync_tick(false);
        let mut after_roll: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        after_roll.sort();
        let after_s = self.fxc_stored(t).unwrap_or_default();
        let label0 = r.xc_undo_label().replace(' ', "_");
        // 40029 : annuler.
        r.main_on_command(40029);
        let label1 = r.xc_undo_label().replace(' ', "_");
        let undone_s = self.fxc_stored(t).unwrap_or_default();
        out.push(format!(
            "undo_state={} roll_saved={} undo_before={label0} undo_after={label1}",
            (undone_s == before_s) as u8,
            (after_s != before_s) as u8,
        ));
        self.fxc_reload_if_changed();
        self.fxc_scan_chain();
        self.fxc_sync_tick(false);
        let mut undone: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        undone.sort();
        let undo_ok = undone == before_roll;
        out.push(format!(
            "undo={} (roll changed it: {})",
            undo_ok as u8,
            (after_roll != before_roll) as u8
        ));
        ok &= undo_ok;
        // Puis ROLL pour de bon.
        self.fxc_roll(None);
        self.fxc_sync_tick(false);
        let (eqg, cg) = (eq_guid.clone(), comp_guid.clone());
        let n = self
            .fxc
            .t
            .tracked()
            .filter(|(k, _)| k.starts_with(&eqg) || k.starts_with(&cg))
            .count();
        out.push(format!("roll_tracked={n}"));
        ok &= (1..=8).contains(&n);
        // 5b. ROLL de l'ordre (et du contournement) : la chaine change, les
        //     liens suivent chaque effet a sa nouvelle place.
        self.fxc.roll.armed = [false, false, false, false, false, true, true, false];
        let mut orders = Vec::new();
        for _ in 0..3 {
            self.fxc_roll(None);
            self.fxc_sync_tick(false);
            // Une seconde passe (ce que le battement suivant ferait).
            self.fxc.links_dirty = true;
            self.fxc_sync_tick(false);
            let eq_at = r.xc_fx_index(t, &eq_guid).map_or(-1, |i| i as i64);
            let comp_at = r.xc_fx_index(t, &comp_guid).map_or(-1, |i| i as i64);
            orders.push(format!("{eq_at}/{comp_at}"));
        }
        let bad: Vec<String> = self
            .fxc
            .plan
            .links
            .iter()
            .filter_map(|l| {
                let want = match l.src {
                    cp_fxc::links::LSrc::Fx { fx, param } => Some((fx as i64, param as i64)),
                    cp_fxc::links::LSrc::Midi { .. } => self.fxc_test_link(t, l.fx, l.param),
                };
                let got = self.fxc_test_link(t, l.fx, l.param);
                (got != want).then(|| {
                    format!(
                        "{}.{}[{}|{}|n={}]:{want:?}/{got:?}",
                        l.fx,
                        l.param,
                        r.xc_fx_name(t, l.fx),
                        r.xc_param_name(t, l.fx, l.param),
                        r.xc_param_count(t, l.fx)
                    )
                })
            })
            .collect();
        let links_ok = bad.is_empty();
        if !links_ok {
            out.push(format!(
                "order_bad={} chain={}",
                bad.join(";").replace(' ', ""),
                (0..r.xc_fx_count(t))
                    .map(|i| format!("{:?}", cp_fxc::slots::fx_kind(&r.xc_fx_name(t, i))))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        let both = r.xc_fx_index(t, &eq_guid).is_some() && r.xc_fx_index(t, &comp_guid).is_some();
        out.push(format!(
            "order={} order_links={} order_fx={}",
            orders.join(","),
            links_ok as u8,
            both as u8
        ));
        ok &= links_ok && both;
        for i in 0..r.xc_fx_count(t) {
            r.xc_fx_set_enabled(t, i, true);
        }
        // 6. Un preset : enregistre, la piste change, rappele.
        let before: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        self.fxc_act(Act::Click(Ctl::UntrackAll, Mods::default()));
        self.fxc_sync_tick(false);
        for k in &before {
            self.fxc.t.param_mut(k).sel = true;
        }
        self.fxc_changed();
        self.fxc_sync_tick(false);
        let mut names_before: Vec<String> = before
            .iter()
            .filter_map(|k| cp_fxc::model::split_pkey(k).map(|(_, i)| i.to_string()))
            .collect();
        self.fxc_save_preset(PRESET);
        self.fxc_act(Act::Click(Ctl::UntrackAll, Mods::default()));
        self.fxc_sync_tick(false);
        self.fxc_load_preset(PRESET);
        self.fxc_sync_tick(false);
        let mut names_after: Vec<String> = self
            .fxc
            .t
            .tracked()
            .filter_map(|(k, _)| cp_fxc::model::split_pkey(k).map(|(_, i)| i.to_string()))
            .collect();
        names_before.sort();
        names_after.sort();
        let preset_ok = names_before == names_after && !names_after.is_empty();
        out.push(format!(
            "preset={} ({}→{})",
            preset_ok as u8,
            names_before.join(","),
            names_after.join(",")
        ));
        ok &= preset_ok;
        // 7. Le Sound Gen : pose, puis retire (son reglage reste dans la
        //    piste).
        self.fxc_act(Act::Click(Ctl::SgOn, Mods::default()));
        self.fxc_sync_tick(false);
        let sg_on = self.fxc.chain.find(FxKind::SoundGen).is_some();
        self.fxc_act(Act::Click(Ctl::SgOn, Mods::default()));
        self.fxc_sync_tick(false);
        let sg_off = self.fxc.chain.find(FxKind::SoundGen).is_none();
        let parked = r.xc_track_ext(t, "CP_FXC_SG_PARK").is_some();
        out.push(format!(
            "sg_on={} sg_off={} sg_park={}",
            sg_on as u8, sg_off as u8, parked as u8
        ));
        ok &= sg_on && sg_off && parked;
        // 8. Le sequenceur : en tete, puis retire.
        self.fxc_act(Act::Click(Ctl::SeqAdd, Mods::default()));
        self.fxc_sync_tick(false);
        let seq_at = self.fxc.chain.find(FxKind::Seq);
        self.fxc_act(Act::Click(Ctl::SeqRemove, Mods::default()));
        self.fxc_sync_tick(false);
        let seq_gone = self.fxc.chain.find(FxKind::Seq).is_none();
        out.push(format!(
            "seq_at={} seq_removed={}",
            seq_at.map_or(-1, |i| i as i64),
            seq_gone as u8
        ));
        ok &= seq_at == Some(0) && seq_gone;
        // 8b. Init : la chaine de ROLL part, chaque lien a nous est relache
        //     (celui de la macro aussi, que le modele oublie).
        let bfx = self.fxc.chain.find(FxKind::Bridge);
        let macro_before = bfx.and_then(|b| self.fxc_test_link(t, b, bridge::MACRO_IN));
        self.fxc_init();
        self.fxc_sync_tick(false);
        let bfx = self.fxc.chain.find(FxKind::Bridge);
        let macro_after = bfx.and_then(|b| self.fxc_test_link(t, b, bridge::MACRO_IN));
        let users = (0..r.xc_fx_count(t))
            .filter(|&i| cp_fxc::slots::fx_kind(&r.xc_fx_name(t, i)) == FxKind::User)
            .count();
        let init_ok = macro_before.is_some() && macro_after.is_none() && users == 0;
        out.push(format!(
            "init={} (macro link {}→{}, user fx {users})",
            init_ok as u8,
            macro_before.is_some() as u8,
            macro_after.is_some() as u8
        ));
        ok &= init_ok;
        // 9. L'etat se relit : la piste revisitee garde son modele.
        self.fxc_changed();
        self.fxc_save_now();
        let stored = self.fxc_stored(t).and_then(|s| cp_fxc::store::decode(&s));
        let state_ok = stored.is_some_and(|s| s.tracked_count() == self.fxc.t.tracked_count());
        out.push(format!("state={}", state_ok as u8));
        ok &= state_ok;
        format!("run {} {}", if ok { "OK" } else { "FAIL" }, out.join(" "))
    }
}
