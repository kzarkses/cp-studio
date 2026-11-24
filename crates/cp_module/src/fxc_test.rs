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
        // 5. ROLL : rien ne casse, les parametres suivis restent bornes.
        self.fxc.roll.armed = [true, true, true, true, true, false, false, false];
        self.fxc_roll(None);
        self.fxc_sync_tick(false);
        let n = self.fxc.t.tracked_count();
        out.push(format!("roll_tracked={n}"));
        ok &= (1..=8).contains(&n);
        // 6. Un preset : enregistre, la piste change, rappele.
        let before: Vec<String> = self.fxc.t.tracked().map(|(k, _)| k.clone()).collect();
        self.fxc_act(Act::Click(Ctl::UntrackAll, Mods::default()));
        self.fxc_sync_tick(false);
        for k in &before {
            self.fxc.t.param_mut(k).sel = true;
        }
        self.fxc_changed();
        self.fxc_sync_tick(false);
        let names_before: Vec<String> = before
            .iter()
            .filter_map(|k| cp_fxc::model::split_pkey(k).map(|(_, i)| i.to_string()))
            .collect();
        self.fxc_save_preset(PRESET);
        self.fxc_act(Act::Click(Ctl::UntrackAll, Mods::default()));
        self.fxc_sync_tick(false);
        self.fxc_load_preset(PRESET);
        self.fxc_sync_tick(false);
        let names_after: Vec<String> = self
            .fxc
            .t
            .tracked()
            .filter_map(|(k, _)| cp_fxc::model::split_pkey(k).map(|(_, i)| i.to_string()))
            .collect();
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
        // 9. L'etat se relit : la piste revisitee garde son modele.
        self.fxc_save_now();
        let stored = r
            .xc_proj_ext(crate::fxc::PROJ, &self.fxc.guid)
            .and_then(|s| cp_fxc::store::decode(&s));
        let state_ok = stored.is_some_and(|s| s.tracked_count() == self.fxc.t.tracked_count());
        out.push(format!("state={}", state_ok as u8));
        ok &= state_ok;
        format!("run {} {}", if ok { "OK" } else { "FAIL" }, out.join(" "))
    }
}
