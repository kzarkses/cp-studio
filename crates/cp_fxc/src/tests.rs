use crate::adopt::{ReadLink, adopt};
use crate::links::{Chain, ChainFx, Ctx, LSrc, link_value, mod_window, plan};
use crate::model::{PState, Pad, Src, Track, pkey};
use crate::pad::{figure_point, granular_values, init_grains, pad_value};
use crate::preset::{Preset, PresetFx, Snapshot, next_snapshot_name};
use crate::roll::{Rng, Settings};
use crate::slots::{
    Band, FxKind, Pol, band_of, bank, bridge, fx_kind, mix, short_fx_name, slot_of,
};
use crate::{preset, roll, store};

fn fx(guid: &str, name: &str, n: usize) -> ChainFx {
    ChainFx {
        guid: guid.into(),
        name: name.into(),
        kind: Some(fx_kind(name)),
        nparams: n,
    }
}

/// Une chaine : un effet de l'utilisateur, le bridge, la banque.
fn chain() -> Chain {
    Chain {
        fx: vec![
            fx("{A}", "VST3: Pro-Q 3 (FabFilter)", 50),
            fx("{B}", "JS: FX Constellation Bridge", 35),
            fx("{L}", "JS: CP_Mod LFO", 147),
        ],
    }
}

fn chain_mix() -> Chain {
    let mut c = chain();
    c.fx.insert(0, fx("{M}", "JS: FX Constellation - Mod Mix", 240));
    c
}

#[test]
fn bands_follow_the_contract() {
    assert_eq!(band_of(0), Some(Band::Pad));
    assert_eq!(band_of(3), Some(Band::Lfo(3)));
    assert_eq!(band_of(9), None);
    assert_eq!(band_of(108), Some(Band::Global(8)));
    assert_eq!(band_of(109), None);
    assert_eq!(band_of(201), Some(Band::Env(1)));
    assert_eq!(band_of(205), None);
    assert_eq!(band_of(210), None);
    assert_eq!(band_of(218), Some(Band::Macro(8)));
    assert_eq!(band_of(222), Some(Band::Seq(2)));
    assert_eq!(band_of(223), None);
    assert_eq!(band_of(234), Some(Band::Note(4)));
    assert_eq!(band_of(235), None);
    assert_eq!(band_of(-1), Some(Band::PadX));
    for s in [
        0, 1, 8, 101, 108, 201, 204, 211, 218, 221, 222, 231, 234, -1, -2,
    ] {
        assert_eq!(slot_of(band_of(s).unwrap()), s);
    }
}

#[test]
fn steps_pack_like_the_bank() {
    // 8255455 = 31 partout (le defaut de la banque).
    let s = bank::unpack_steps([8255455.0; 4]);
    assert!(s.iter().all(|&v| v == 31));
    let mut t = [0u8; 16];
    for (i, v) in t.iter_mut().enumerate() {
        *v = (i * 4) as u8;
    }
    assert_eq!(bank::unpack_steps(bank::pack_steps(&t)), t);
    assert!(bank::pack_steps(&[63; 16]).iter().all(|&v| v == 16777215.0));
}

#[test]
fn fx_names_and_kinds() {
    assert_eq!(fx_kind("JS: FX Constellation Bridge"), FxKind::Bridge);
    assert_eq!(fx_kind("JS: CP_Mod MIDI"), FxKind::Bank);
    assert_eq!(fx_kind("JS: FX Constellation - Mod Mix"), FxKind::Mix);
    assert_eq!(
        fx_kind("VSTi: ReaSamplOmatic5000 (Cockos)"),
        FxKind::Protected
    );
    assert_eq!(fx_kind("VST3: Pro-Q 3 (FabFilter)"), FxKind::User);
    assert_eq!(short_fx_name("VST3: Pro-Q 3 (FabFilter)"), "Pro-Q 3");
    assert_eq!(
        short_fx_name("VST: A very long plugin name that goes on"),
        "A very long plugin nam..."
    );
}

fn track_with(k: &str, p: PState) -> Track {
    let mut t = Track::default();
    t.params.insert(k.into(), p);
    t
}

#[test]
fn pad_only_links_from_the_bridge_output() {
    let k = pkey("{A}", 3);
    let mut t = track_with(
        &k,
        PState {
            sel: true,
            base: 0.4,
            range: 0.5,
            inv: true,
            x: true,
            y: false,
            srcs: vec![],
        },
    );
    t.pad.ax = 0.3;
    let p = plan(&t, &chain(), &Ctx::default());
    assert_eq!(p.links.len(), 1);
    let l = &p.links[0];
    assert_eq!((l.fx, l.param), (0, 3));
    assert_eq!(
        l.src,
        LSrc::Fx {
            fx: 1,
            param: bridge::X_OUT
        }
    );
    assert!((l.offset + 0.3).abs() < 1e-12);
    assert!((l.scale + 0.5).abs() < 1e-12);
    // Ce que REAPER calcule = ce que le pad promet.
    t.pad.x = 0.9;
    let want = pad_value(0.4, -0.5, true, false, &t.pad);
    assert!((link_value(l, 0.9) - want).abs() < 1e-12);
    // Le pad en mode granulaire : plus de lien.
    t.pad.granular = true;
    assert!(plan(&t, &chain(), &Ctx::default()).links.is_empty());
}

#[test]
fn both_chips_use_the_mix_of_the_axes() {
    let k = pkey("{A}", 1);
    let mut t = track_with(
        &k,
        PState {
            sel: true,
            ..PState::default()
        },
    );
    t.pad.ax = 0.2;
    t.pad.ay = 0.6;
    let p = plan(&t, &chain(), &Ctx::default());
    let l = &p.links[0];
    assert_eq!(
        l.src,
        LSrc::Fx {
            fx: 1,
            param: bridge::XY_MIX
        }
    );
    assert!((l.offset + 0.4).abs() < 1e-12);
}

#[test]
fn a_single_source_links_directly() {
    let k = pkey("{A}", 7);
    let mut ps = PState {
        sel: true,
        base: 0.5,
        range: 0.6,
        x: false,
        y: false,
        ..PState::default()
    };
    ps.add_source(2).unwrap();
    let t = track_with(&k, ps);
    let p = plan(&t, &chain(), &Ctx::default());
    assert_eq!(p.links.len(), 1);
    let l = &p.links[0];
    assert_eq!(
        l.src,
        LSrc::Fx {
            fx: 2,
            param: bank::out(2)
        }
    );
    // Bipolaire : neutre 0.5.
    assert!((l.offset + 0.5).abs() < 1e-12);
    assert!((l.scale - 0.6).abs() < 1e-12);
    // Unipolaire : neutre 0.
    let mut ctx = Ctx::default();
    ctx.uni[1] = true;
    assert!(plan(&t, &chain(), &ctx).links[0].offset.abs() < 1e-12);
    // Une polarite propre au lien l'emporte.
    let mut t2 = t.clone();
    t2.param_mut(&k).srcs[0].pol = Some(Pol::Bi);
    assert!((plan(&t2, &chain(), &ctx).links[0].offset + 0.5).abs() < 1e-12);
    // Contournee : la profondeur joue 0.
    let mut t3 = t.clone();
    t3.param_mut(&k).srcs[0].off = true;
    assert_eq!(plan(&t3, &chain(), &Ctx::default()).links[0].scale, 0.0);
}

#[test]
fn a_global_source_is_a_midi_link() {
    let k = pkey("{A}", 0);
    let mut ps = PState {
        sel: true,
        x: false,
        y: false,
        ..PState::default()
    };
    ps.add_source(103).unwrap();
    let p = plan(&track_with(&k, ps), &chain(), &Ctx::default());
    assert_eq!(p.links[0].src, LSrc::Midi { cc: 18 });
    assert!(p.wants_global);
}

#[test]
fn two_sources_go_through_a_bus() {
    let k = pkey("{A}", 5);
    let mut ps = PState {
        sel: true,
        base: 0.3,
        range: 0.4,
        x: false,
        y: false,
        ..PState::default()
    };
    ps.add_source(1).unwrap();
    ps.add_source(211).unwrap();
    ps.srcs[1].depth = -0.25;
    let t = track_with(&k, ps.clone());
    // Sans Mod Mix : il manque, la premiere source joue seule.
    let p = plan(&t, &chain(), &Ctx::default());
    assert!(p.mix_missing);
    assert_eq!(p.links.len(), 1);
    // Avec : le parametre lit la sortie du bus 1.
    let c = chain_mix();
    let p = plan(&t, &c, &Ctx::default());
    assert!(!p.mix_missing && !p.mix_unused);
    assert_eq!(p.bus_of.get(&k), Some(&1));
    let main = p.links.iter().find(|l| l.fx == 1 && l.param == 5).unwrap();
    assert_eq!(
        main.src,
        LSrc::Fx {
            fx: 0,
            param: mix::out(1)
        }
    );
    assert_eq!((main.offset, main.scale, main.baseline), (-0.5, 2.0, 0.3));
    let raw = |param: usize| {
        p.raws
            .iter()
            .rev()
            .find(|r| r.fx == 0 && r.param == param)
            .map(|r| r.value)
    };
    assert!((raw(mix::amt(1, 1)).unwrap() - 0.4).abs() < 1e-12);
    assert!((raw(mix::amt(1, 2)).unwrap() + 0.25).abs() < 1e-12);
    assert_eq!(raw(mix::ctr(1, 1)), Some(0.5));
    assert_eq!(raw(mix::ctr(1, 2)), Some(0.0));
    assert_eq!(raw(mix::amt(1, 3)), Some(0.0));
    // Le bus deja tenu reste a lui.
    let mut ctx = Ctx::default();
    ctx.prev_bus.insert(k.clone(), 7);
    assert_eq!(plan(&t, &c, &ctx).bus_of.get(&k), Some(&7));
}

#[test]
fn the_pad_rides_with_a_source_on_a_bus() {
    let k = pkey("{A}", 2);
    let mut ps = PState {
        sel: true,
        base: 0.5,
        range: 0.5,
        x: true,
        y: false,
        ..PState::default()
    };
    ps.add_source(4).unwrap();
    let mut t = track_with(&k, ps);
    t.pad.ax = 0.25;
    let p = plan(&t, &chain_mix(), &Ctx::default());
    let input2 = p
        .links
        .iter()
        .find(|l| l.fx == 0 && l.param == mix::input(1, 2))
        .unwrap();
    assert_eq!(
        input2.src,
        LSrc::Fx {
            fx: 2,
            param: bridge::X_OUT
        }
    );
    let ctr = p
        .raws
        .iter()
        .find(|r| r.param == mix::ctr(1, 2))
        .unwrap()
        .value;
    assert_eq!(ctr, 0.25);
    // Sans Mod Mix : l'axe attend, la source joue.
    let p = plan(&t, &chain(), &Ctx::default());
    assert!(p.standby.contains(&k));
    assert!(!p.mix_missing);
}

#[test]
fn a_driven_pad_axis_stays_in_its_zone() {
    let mut t = Track::default();
    t.pad.src_x = 3;
    t.pad.zone = [0.8, 0.2, 0.0, 1.0];
    let p = plan(&t, &chain(), &Ctx::default());
    assert!(p.pad_link_x && !p.pad_link_y);
    let l = p.links.iter().find(|l| l.param == bridge::X_POS).unwrap();
    assert_eq!(l.fx, 1);
    assert!((l.scale - 0.6).abs() < 1e-12 && (l.baseline - 0.2).abs() < 1e-12);
}

#[test]
fn a_setting_of_an_internal_fx_is_a_destination() {
    // Une macro (l'entree du bridge) et un reglage de la banque, menes par
    // une source ; x_pos du bridge n'est jamais une destination de ce genre.
    let mut t = Track::default();
    let mut m = PState {
        sel: true,
        base: 0.2,
        range: 0.5,
        x: false,
        y: false,
        ..PState::default()
    };
    m.add_source(3).unwrap();
    t.params.insert(pkey("{B}", bridge::MACRO_IN), m.clone());
    t.params.insert(pkey("{L}", bank::rate(1)), m.clone());
    t.params.insert(pkey("{B}", bridge::X_POS), m);
    let p = plan(&t, &chain(), &Ctx::default());
    let to = |fx: usize, param: usize| p.links.iter().find(|l| l.fx == fx && l.param == param);
    let src = LSrc::Fx {
        fx: 2,
        param: bank::out(3),
    };
    assert_eq!(to(1, bridge::MACRO_IN).map(|l| l.src), Some(src));
    assert_eq!(to(2, bank::rate(1)).map(|l| l.src), Some(src));
    assert!(to(1, bridge::X_POS).is_none());
}

#[test]
fn the_mod_window_crosses_for_a_negative_depth() {
    assert_eq!(mod_window(0.5, 0.4, 0.5), (0.3, 0.7));
    assert_eq!(mod_window(0.5, 0.4, 0.0), (0.5, 0.9));
    let (lo, hi) = mod_window(0.5, -0.4, 0.0);
    assert!(lo > hi);
    assert_eq!(mod_window(0.9, 0.4, 0.0), (0.9, 1.0));
}

#[test]
fn removing_the_first_source_promotes_the_second() {
    let mut ps = PState {
        range: 0.8,
        ..PState::default()
    };
    ps.add_source(1).unwrap();
    ps.add_source(2).unwrap();
    ps.srcs[1].depth = -0.3;
    ps.remove_source(0);
    assert_eq!(ps.srcs.len(), 1);
    assert_eq!(ps.srcs[0].slot, 2);
    assert!((ps.range - 0.3).abs() < 1e-12 && ps.inv);
    for s in [3, 4, 5] {
        ps.add_source(s).unwrap();
    }
    assert!(ps.add_source(6).is_err());
}

#[test]
fn the_state_reads_back() {
    let mut t = Track {
        pad: Pad {
            nav: crate::model::Nav::Figures,
            nav_on: true,
            fig_mode: 5,
            zone: [0.1, 0.9, 0.2, 0.7],
            src_y: 103,
            x: 0.25,
            ay: 0.75,
            ..Pad::default()
        },
        ..Track::default()
    };
    t.macro_names[2] = "Cutoff sweep".into();
    t.preset = "Big one".into();
    let mut ps = PState {
        sel: true,
        base: 0.42,
        range: 0.8,
        inv: true,
        x: true,
        y: false,
        srcs: vec![],
    };
    ps.add_source(1).unwrap();
    ps.add_source(211).unwrap();
    ps.srcs[1].depth = -0.25;
    ps.srcs[1].pol = Some(Pol::Uni);
    ps.srcs[1].off = true;
    t.params.insert(pkey("{A}", 3), ps);
    t.params.insert(pkey("{A}", 4), PState::default());
    t.fx_random_max.insert("{A}".into(), 4);
    t.fx_tracked_only.insert("{A}".into(), true);
    t.morph_a = Some([(pkey("{A}", 3), 0.4)].into_iter().collect());
    let s = store::encode(&t);
    let back = store::decode(&s).unwrap();
    // Un parametre jamais touche n'a pas de ligne.
    t.params.remove(&pkey("{A}", 4));
    assert_eq!(back.params, t.params);
    assert_eq!(back.pad, t.pad);
    assert_eq!(back, t);
    assert!(store::decode("autre chose").is_none());
}

/// Ce que REAPER rendrait des liens d'un plan.
fn as_read(p: &crate::links::Plan) -> Vec<ReadLink> {
    p.links
        .iter()
        .map(|l| ReadLink {
            fx: l.fx,
            param: l.param,
            src: l.src,
            offset: l.offset,
            scale: l.scale,
            baseline: l.baseline,
        })
        .collect()
}

#[test]
fn a_track_adopts_its_links() {
    let c = chain_mix();
    let mut t = Track::default();
    t.pad.ax = 0.3;
    t.pad.ay = 0.6;
    t.params.insert(
        pkey("{A}", 1),
        PState {
            sel: true,
            base: 0.2,
            range: 0.7,
            inv: true,
            x: true,
            y: false,
            srcs: vec![],
        },
    );
    let mut one = PState {
        sel: true,
        base: 0.6,
        range: 0.5,
        x: false,
        y: false,
        ..PState::default()
    };
    one.add_source(104).unwrap();
    t.params.insert(pkey("{A}", 2), one);
    let mut two = PState {
        sel: true,
        base: 0.1,
        range: 0.9,
        x: false,
        y: true,
        ..PState::default()
    };
    two.add_source(3).unwrap();
    two.add_source(212).unwrap();
    two.srcs[1].depth = 0.4;
    two.srcs[1].pol = Some(Pol::Bi);
    t.params.insert(pkey("{A}", 9), two);
    let p = plan(&t, &c, &Ctx::default());
    let reads = as_read(&p);
    let raws = p.raws.clone();
    let raw = move |fx: usize, param: usize| {
        raws.iter()
            .rev()
            .find(|r| r.fx == fx && r.param == param)
            .map_or(0.0, |r| r.value)
    };
    let mut back = Track::default();
    let n = adopt(&mut back, &c, &reads, &raw, &[false; 8], &[false; 8]);
    assert_eq!(n, 3);
    assert_eq!((back.pad.ax, back.pad.ay), (0.3, 0.6));
    for k in [pkey("{A}", 1), pkey("{A}", 2), pkey("{A}", 9)] {
        let (a, b) = (&t.params[&k], &back.params[&k]);
        assert_eq!((a.sel, a.x, a.y, a.inv), (b.sel, b.x, b.y, b.inv), "{k}");
        assert!((a.base - b.base).abs() < 1e-9 && (a.range - b.range).abs() < 1e-9);
        let sa: Vec<(i32, Option<Pol>)> = a.srcs.iter().map(|s| (s.slot, s.pol)).collect();
        let sb: Vec<(i32, Option<Pol>)> = b.srcs.iter().map(|s| (s.slot, s.pol)).collect();
        assert_eq!(sa, sb, "{k}");
    }
    // Le plan du modele adopte est le meme.
    assert_eq!(plan(&back, &c, &Ctx::default()).links, p.links);
}

fn pool(n: usize) -> Vec<String> {
    (0..n).map(|i| pkey("{A}", i)).collect()
}

#[test]
fn roll_tracks_between_min_and_max() {
    let mut rng = Rng::new(7);
    let s = Settings::default();
    let p = pool(40);
    for _ in 0..50 {
        let mut t = Track::default();
        roll::select(&mut t, &p, &s, &mut rng);
        let n = t.tracked_count();
        assert!((3..=8).contains(&n), "{n}");
    }
    let mut t = Track::default();
    roll::select(&mut t, &pool(2), &s, &mut rng);
    assert_eq!(t.tracked_count(), 2);
}

#[test]
fn roll_bases_stay_in_the_span_around_its_center() {
    let mut rng = Rng::new(3);
    let s = Settings::default();
    let p = pool(20);
    let mut t = Track::default();
    for k in &p {
        t.param_mut(k).sel = true;
    }
    let out = roll::bases(&mut t, &p, &s, &mut rng);
    assert_eq!(out.len(), 20);
    assert!(out.iter().all(|(_, v)| (0.35..=0.65).contains(v)));
}

#[test]
fn roll_xy_is_exclusive_and_skips_sourced_params() {
    let mut rng = Rng::new(11);
    let p = pool(30);
    let mut t = Track::default();
    for k in &p {
        t.param_mut(k).sel = true;
    }
    t.param_mut(&p[0]).srcs.push(Src::new(1, 0.5));
    t.param_mut(&p[0]).x = true;
    t.param_mut(&p[0]).y = true;
    roll::xy(&mut t, &p, true, &mut rng);
    assert!(t.params[&p[0]].x && t.params[&p[0]].y);
    assert!(p[1..].iter().all(|k| t.params[k].x != t.params[k].y));
}

#[test]
fn roll_order_is_a_permutation() {
    let mut rng = Rng::new(5);
    let mut o = roll::order(9, &mut rng);
    o.sort();
    assert_eq!(o, (0..9).collect::<Vec<_>>());
}

#[test]
fn granular_blends_near_grains() {
    let keys = vec![pkey("{A}", 0)];
    let pad = Pad::default();
    let mut rng = Rng::new(1);
    let g = init_grains(3, &keys, &pad, &mut rng);
    assert_eq!(g.len(), 9);
    // Sur un grain : sa valeur.
    let v = granular_values(&g, 3, g[4].x, g[4].y);
    assert!((v[&keys[0]] - g[4].values[&keys[0]]).abs() < 1e-12);
    // Entre deux grains voisins : entre leurs valeurs.
    let (a, b) = (g[3].values[&keys[0]], g[4].values[&keys[0]]);
    let mid = granular_values(&g, 3, (g[3].x + g[4].x) / 2.0, g[4].y)[&keys[0]];
    assert!(mid >= a.min(b) - 1e-9 && mid <= a.max(b) + 1e-9);
}

#[test]
fn figures_are_continuous_and_inside_the_zone() {
    let zone = [0.2, 0.8, 0.1, 0.5];
    for mode in 0..6u8 {
        let mut last = figure_point(mode, 0.0, 0.8, zone);
        for i in 1..=400 {
            let p = figure_point(mode, i as f64 / 400.0, 0.8, zone);
            assert!(
                p.0 >= 0.2 - 1e-9 && p.0 <= 0.8 + 1e-9 && p.1 >= 0.1 - 1e-9 && p.1 <= 0.5 + 1e-9
            );
            let d = ((p.0 - last.0).powi(2) + (p.1 - last.1).powi(2)).sqrt();
            assert!(d < 0.05, "figure {mode} jumps at {i}: {d}");
            last = p;
        }
    }
}

#[test]
fn a_preset_reads_back() {
    let mut p = Preset {
        pad: Pad {
            fig_mode: 2,
            ..Pad::default()
        },
        ..Preset::default()
    };
    p.macros[1] = 0.25;
    p.macro_names[1] = "Wobble".into();
    let mut ps = PState {
        sel: true,
        base: 0.3,
        ..PState::default()
    };
    ps.add_source(2).unwrap();
    p.fx.push(PresetFx {
        name: "VST3: Pro-Q 3 (FabFilter)".into(),
        ident: "VST3: Pro-Q 3 (FabFilter)".into(),
        renamed: "My EQ".into(),
        enabled: true,
        detached: false,
        preset: String::new(),
        chunk: Some(("vst_chunk".into(), "AAAA==".into())),
        raw: vec![0.0, 0.5, 1.0],
        params: [(4usize, ("Gain".to_string(), ps.clone()))]
            .into_iter()
            .collect(),
    });
    p.sg = Some((true, vec![0.1, 0.2]));
    p.bank = Some(vec![1.0, 0.0]);
    p.routes = "v1;m:2=1@0.5000".into();
    p.internal.insert(("bridge".into(), 6), ps.clone());
    let s = Snapshot {
        internal: [(("bank".to_string(), 2usize), ps.clone())]
            .into_iter()
            .collect(),
        params: [("Pro-Q 3||Gain".to_string(), ps)].into_iter().collect(),
        fx: vec![("VST3: Pro-Q 3 (FabFilter)".into(), true)],
        ..Snapshot::default()
    };
    p.snapshots.insert("Snapshot1".into(), s.clone());
    p.snapshots.insert("Snapshot4".into(), s);
    let back = preset::decode(&preset::encode(&p)).unwrap();
    assert_eq!(back, p);
    assert_eq!(next_snapshot_name(&p), "Snapshot5");
    assert_eq!(preset::file_name("a/b: c"), "a_b_ c.fxc");
}
