//! Les essais du catalogue, sur des extraits des vrais fichiers de REAPER
//! (la machine de Cedric, 2026-11-24).

use super::*;

const VST: &str = "[vstcache]
reaeq.dll=005423FA4C03DD01,1919247729,ReaEQ (Cockos)
reasynth.dll=005423FA4C03DD01,1919251321,ReaSynth (Cockos)!!!VSTi
FabFilter_Pro_Q_3.vst3=8040160E5746DB01,756089518{72C4DB717A4D459AB97E51745D84B39D,Pro-Q 3 (FabFilter)
kHs_3_Band_EQ.vst3=804C5FDE5112DB01,843615827{D696B572003344BC0072C4B500A11C12,kHs 3-Band EQ
kHs_Haas.vst3=804C5FDE5112DB01,843615828{D696B572003344BC0072C4B500A11C13,kHs Haas
Serum.vst3=0023B4B49643D901,1035478619{56535458667358736572756D00000000,Serum (Xfer Records)!!!VSTi
Kontakt_7.vst3=AEBFAA459BE4D901,1219583956{5653544E694B376B6F6E74616B742037,Kontakt 7 (Native Instruments) (64 out)!!!VSTi
MTremolo.vst3=80390FF9527FD801,1326722673{4D656C646170726F4D4165374D416537,MTremolo (MeldaProduction)
";

const CLAP: &str = "[Surge XT Effects.clap]
_=006620844F7FD901295280434D98D901
org.surge-synth-team.surge-xt-fx=0|Surge XT Effects (Surge Synth Team)
[Surge XT.clap]
_=00E3FA30507FD901BF6220434D98D901
org.surge-synth-team.surge-xt=1|Surge XT (Surge Synth Team)
";

const JSFX: &str = "NAME -------------------------------------------------- \"JS: --------------------------------------------------\"
NAME midi/midi_arp \"JS: MIDI Arpeggiator\"
NAME \"Saike Tools/Amaranth.jsfx\" \"JS: Amaranth\"
REV \"MIDI Arpeggiator\" midi/midi_arp
";

const FOLDERS: &str = "[deleted_categories]
Bass=1

[developer]
kHs_3_Band_EQ.vst3=Kilohearts
[Folder0]
Item0=C:\\Program Files\\Common Files\\VST3\\Kilohearts\\kHs 3-Band EQ.vst3
Type0=3
Item1=midi/midi_arp
Type1=2
Item2=reaeq.dll
Type2=3
Item3=MIDI Trigger
Type3=1000
Item4=C:\\nowhere\\Gone.vst3
Type4=3
Nb=5
[Folder1]
Item0=C:\\Program Files\\Common Files\\VST3\\FabFilter Pro-Q 3.vst3
Type0=3
Nb=1
[Folders]
Id0=0
Id1=1
Name0=Favorites
Name1=Fabfilter
NbFolders=2
";

const TAGS: &str = "[developer]
Amaranth.jsfx=Saike
kHs_Haas.vst3=Kilohearts
[category]
reaeq.dll=EQ
FabFilter_Pro_Q_3.vst3=EQ
kHs_Haas.vst3=Spatial|Stereo
reasynth.dll=Synth
";

fn installed() -> Vec<(String, String)> {
    [
        ("VST: ReaEQ (Cockos)", "reaeq.dll"),
        ("VSTi: ReaSynth (Cockos)", "reasynth.dll"),
        ("VST3: Pro-Q 3 (FabFilter)", ""),
        ("VST3: kHs 3-Band EQ", ""),
        ("VST3: kHs Haas", ""),
        ("VST3i: Serum (Xfer Records)", ""),
        ("VST3i: Kontakt 7 (Native Instruments) (64 out)", ""),
        ("VST3: MTremolo (MeldaProduction)", ""),
        ("CLAP: Surge XT Effects (Surge Synth Team)", ""),
        ("CLAPi: Surge XT (Surge Synth Team)", ""),
        ("JS: MIDI Arpeggiator", "midi/midi_arp"),
        ("JS: Amaranth", ""),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

fn catalog() -> Catalog {
    let ini = IniTexts {
        vst: vec![VST.into()],
        clap: vec![CLAP.into()],
        jsfx: JSFX.into(),
        folders: FOLDERS.into(),
        tags: TAGS.into(),
    };
    Catalog::build(
        &installed(),
        &ini,
        &[
            "MIDI_Trigger.RfxChain".into(),
            "Eleven/High.RfxChain".into(),
        ],
    )
}

fn idx(c: &Catalog, name: &str) -> usize {
    c.find(name).unwrap_or_else(|| panic!("{name} absent"))
}

#[test]
fn les_noms_se_separent() {
    assert_eq!(split_name("VST3i: Serum (Xfer Records)").0, Kind::Vst3);
    assert!(split_name("VST3i: Serum (Xfer Records)").1);
    assert!(split_name("VSTi: ReaSynth (Cockos)").1);
    assert!(!split_name("VST: ReaEQ (Cockos)").1);
    assert_eq!(
        split_name("CLAPi: Surge XT (Surge Synth Team)").0,
        Kind::Clap
    );
    assert_eq!(split_name("JS: MIDI Arpeggiator").0, Kind::Js);
    assert_eq!(
        split_dev("Kontakt 7 (Native Instruments) (64 out)"),
        ("Kontakt 7 (64 out)".into(), "Native Instruments".into())
    );
    assert_eq!(
        split_dev("Helm (x86) (Matt Tytel)"),
        ("Helm (x86)".into(), "Matt Tytel".into())
    );
    assert_eq!(
        split_dev("Surge XT (Surge Synth Team) (2->6ch)"),
        ("Surge XT (2->6ch)".into(), "Surge Synth Team".into())
    );
    assert_eq!(split_dev("kHs Haas"), ("kHs Haas".into(), String::new()));
    assert_eq!(
        split_dev("Helm (Matt Tytel) (mono)"),
        ("Helm (mono)".into(), "Matt Tytel".into())
    );
}

#[test]
fn les_fichiers_se_comparent() {
    assert_eq!(
        ini::norm_file("C:\\Program Files\\Common Files\\VST3\\Kilohearts\\kHs 3-Band EQ.vst3"),
        ini::norm_file("kHs_3_Band_EQ.vst3")
    );
    assert_eq!(
        ini::norm_file("FabFilter_Pro_Q_3.vst3<756089518{72C4"),
        "fabfilter_pro_q_3.vst3"
    );
    assert_eq!(
        ini::norm_js("Saike Tools\\Amaranth.jsfx"),
        "saike tools/amaranth"
    );
    assert_eq!(ini::js_base("saike tools/amaranth"), "amaranth.jsfx");
}

#[test]
fn le_catalogue_reunit_liste_dossiers_et_etiquettes() {
    let c = catalog();
    // 12 effets + 2 chaines.
    assert_eq!(c.plugins.len(), 14);
    let haas = &c.plugins[idx(&c, "VST3: kHs Haas")];
    assert_eq!(haas.file, "khs_haas.vst3");
    assert_eq!(haas.dev, "Kilohearts");
    assert_eq!(haas.cats, vec!["Spatial".to_string(), "Stereo".to_string()]);
    // Le developpeur range dans reaper-fxfolders.ini.
    assert_eq!(c.plugins[idx(&c, "VST3: kHs 3-Band EQ")].dev, "Kilohearts");
    let serum = &c.plugins[idx(&c, "VST3i: Serum (Xfer Records)")];
    assert!(serum.instrument);
    assert_eq!(serum.type_tag(), "VST3i");
    assert_eq!(serum.label, "Serum");
    assert!(c.plugins[idx(&c, "CLAPi: Surge XT (Surge Synth Team)")].instrument);
    assert_eq!(
        c.plugins[idx(&c, "JS: Amaranth")].dev,
        "Saike",
        "un JS sans ident : son chemin vient de reaper-jsfx.ini"
    );
    // Le dossier « Favorites » : quatre trouves, le cinquieme n'existe plus.
    assert_eq!(c.folders.len(), 2);
    let fav: Vec<&str> = c
        .reaper_favs
        .iter()
        .map(|&i| c.plugins[i].name.as_str())
        .collect();
    assert_eq!(
        fav,
        vec![
            "VST3: kHs 3-Band EQ",
            "JS: MIDI Arpeggiator",
            "VST: ReaEQ (Cockos)",
            "MIDI_Trigger.RfxChain"
        ]
    );
    assert_eq!(
        c.folders[1].items,
        vec![idx(&c, "VST3: Pro-Q 3 (FabFilter)")]
    );
    // Les categories et developpeurs, par nom.
    let eq = c.cats.iter().find(|(n, _)| n == "EQ").unwrap();
    assert_eq!(eq.1.len(), 2);
    assert!(c.devs.iter().any(|(n, _)| n == "Kilohearts"));
    // Les noms d'avant (« nom (developpeur) ») se retrouvent.
    assert_eq!(c.find("kHs Haas (Kilohearts)"), c.find("VST3: kHs Haas"));
    assert_eq!(
        c.find("Pro-Q 3 (FabFilter)"),
        c.find("VST3: Pro-Q 3 (FabFilter)")
    );
}

#[test]
fn la_recherche_prend_tous_les_mots_et_exclut() {
    let c = catalog();
    let st = Store::default();
    let names = |q: &str| -> Vec<String> {
        list(&c, &st, &Src::All, q)
            .iter()
            .map(|&i| c.plugins[i].label.clone())
            .collect()
    };
    assert_eq!(names("eq"), vec!["kHs 3-Band EQ", "Pro-Q 3", "ReaEQ"]);
    assert_eq!(names("eq -kilohearts"), vec!["Pro-Q 3", "ReaEQ"]);
    assert_eq!(names("fabfilter eq"), vec!["Pro-Q 3"]);
    assert_eq!(names("stereo"), vec!["kHs Haas"]);
    assert!(names("synth").contains(&"ReaSynth".to_string()));
    // Les chaines ne sont pas dans « All ».
    assert!(!names("").iter().any(|n| n == "MIDI_Trigger"));
}

#[test]
fn les_favoris_partent_de_reaper() {
    let c = catalog();
    let mut st = Store::default();
    let eq = idx(&c, "VST: ReaEQ (Cockos)");
    let pro = idx(&c, "VST3: Pro-Q 3 (FabFilter)");
    assert!(st.is_fav(&c, eq));
    assert!(!st.is_fav(&c, pro));
    st.toggle_fav(&c, eq);
    st.toggle_fav(&c, pro);
    assert!(!st.is_fav(&c, eq) && st.is_fav(&c, pro));
    assert_eq!(st.fav_del, vec!["VST: ReaEQ (Cockos)".to_string()]);
    assert_eq!(st.fav_add, vec!["VST3: Pro-Q 3 (FabFilter)".to_string()]);
    // Retour a l'etat de REAPER : rien a garder.
    st.toggle_fav(&c, eq);
    st.toggle_fav(&c, pro);
    assert!(st.fav_add.is_empty() && st.fav_del.is_empty());
    let favs = list(&c, &st, &Src::Fav, "");
    assert_eq!(favs.len(), 4);
}

#[test]
fn recents_et_plus_utilises() {
    let c = catalog();
    let mut st = Store::default();
    for n in [
        "VST: ReaEQ (Cockos)",
        "VST3: kHs Haas",
        "VST: ReaEQ (Cockos)",
    ] {
        st.used(n);
    }
    let rec: Vec<usize> = list(&c, &st, &Src::Recent, "");
    assert_eq!(
        rec,
        vec![idx(&c, "VST: ReaEQ (Cockos)"), idx(&c, "VST3: kHs Haas")]
    );
    let most = list(&c, &st, &Src::Most, "");
    assert_eq!(most[0], idx(&c, "VST: ReaEQ (Cockos)"));
    for k in 0..40 {
        st.used(&format!("x{k}"));
    }
    assert_eq!(st.recent.len(), store::MAX_RECENT);
}

#[test]
fn un_onglet_a_regle_se_tient_a_jour() {
    let c = catalog();
    let mut st = Store::default();
    st.tabs.push(Tab {
        name: "Kilo".into(),
        items: vec!["kHs Haas (Kilohearts)".into(), "Pro-Q 3 (FabFilter)".into()],
        rule: Some(Rule {
            query: "eq -fabfilter".into(),
            src: "all".into(),
        }),
    });
    let v: Vec<String> = list(&c, &st, &Src::Tab("Kilo".into()), "")
        .iter()
        .map(|&i| c.plugins[i].label.clone())
        .collect();
    // La regle (EQ sans FabFilter) plus les effets choisis a la main.
    assert_eq!(v, vec!["kHs 3-Band EQ", "kHs Haas", "Pro-Q 3", "ReaEQ"]);
    // Une regle qui se cite elle-meme ne boucle pas.
    st.tabs[0].rule.as_mut().unwrap().src = "tab:Kilo".into();
    assert!(!list(&c, &st, &Src::Tab("Kilo".into()), "").is_empty());
}

#[test]
fn le_de_tire_sans_remise_et_sans_instrument() {
    let c = catalog();
    let st = Store::default();
    let all = list(&c, &st, &Src::All, "");
    for seed in 1..200u64 {
        let d = dice(&c, &all, 4, seed);
        assert_eq!(d.len(), 4);
        let mut u = d.clone();
        u.sort();
        u.dedup();
        assert_eq!(u.len(), 4, "sans remise");
        assert!(d.iter().all(|&i| !c.plugins[i].instrument));
    }
    // Une liste d'instruments : un seul.
    let inst = list(&c, &st, &Src::Instr, "");
    assert_eq!(dice(&c, &inst, 5, 7).len(), 1);
    // Plus demande que la liste n'en a : toute la liste.
    let eq = list(&c, &st, &Src::All, "eq");
    assert_eq!(dice(&c, &eq, 12, 3).len(), 3);
    // Deux graines differentes ne tirent pas toujours la meme chose.
    let a: Vec<_> = (1..20).map(|s| dice(&c, &all, 2, s)).collect();
    assert!(a.windows(2).any(|w| w[0] != w[1]));
}

#[test]
fn le_fichier_se_relit() {
    let mut st = Store::default();
    st.fav_add.push("VST3: Pro-Q 3 (FabFilter)".into());
    st.fav_del.push("VST: ReaEQ (Cockos)".into());
    st.used("JS: MIDI Arpeggiator");
    st.tabs.push(Tab {
        name: "Mes delais 2".into(),
        items: vec!["kHs Delay (Kilohearts)".into()],
        rule: Some(Rule {
            query: "delay -tape".into(),
            src: "cat:Delay".into(),
        }),
    });
    st.set.dice_count = 7;
    st.set.steal = false;
    st.set.open_groups = [false, true, false, true];
    st.set.src = "folder:Melda".into();
    let back = Store::decode(&st.encode()).unwrap();
    assert_eq!(back, st);
    assert!(Store::decode("autre chose").is_none());
}

#[test]
fn les_sources_ont_une_cle() {
    for s in [
        Src::All,
        Src::Fav,
        Src::Recent,
        Src::Most,
        Src::Instr,
        Src::Kind(Kind::Vst3),
        Src::Kind(Kind::Chain),
        Src::Folder("Melda".into()),
        Src::Cat("EQ".into()),
        Src::Dev("FabFilter".into()),
        Src::Tab("cool".into()),
    ] {
        assert_eq!(Src::from_key(&s.key()), Some(s));
    }
}
