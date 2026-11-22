use super::*;

const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/theme.zip");

fn zip() -> Zip {
    Zip::parse(FIXTURE.to_vec()).expect("le zip de test se lit")
}

fn big_text() -> Vec<u8> {
    let mut s = String::new();
    for i in 0..4000 {
        s += &format!(
            "ligne {i} : le transport de CP Studio {}\n",
            "x".repeat(i % 17)
        );
    }
    s.into_bytes()
}

#[test]
fn zip_et_deflate() {
    let z = zip();
    // Huffman dynamique (gros texte), fixe (petit), blocs stockes, et stocke.
    assert_eq!(z.read("big.txt").unwrap(), big_text());
    assert_eq!(z.read("tiny.txt").unwrap(), b"hello hello hello");
    assert_eq!(z.read("level0.txt").unwrap(), big_text()[..70000].to_vec());
    assert_eq!(
        z.read("stored.bin").unwrap(),
        (0..=255u8).collect::<Vec<_>>()
    );
    // Sans egard a la casse ni aux barres.
    assert!(z.read("IMG\\A.PNG").is_some());
    assert!(z.read("absent.png").is_none());
}

fn premul_rgba(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|c| {
            let a = c[3] as u32;
            let m = |v: u8| (v as u32 * a + 127) / 255;
            a << 24 | m(c[0]) << 16 | m(c[1]) << 8 | m(c[2])
        })
        .collect()
}

#[test]
fn png_tous_les_formats() {
    let z = zip();
    for (name, w, h) in [
        ("a", 5, 3),
        ("pal4", 7, 5),
        ("gray1", 9, 2),
        ("rgb16", 3, 3),
        ("ga8", 4, 4),
        ("inter", 11, 9),
        ("pink", 8, 6),
    ] {
        let s = png::decode(&z.read(&format!("Img/{name}.png")).unwrap())
            .unwrap_or_else(|| panic!("{name} ne se decode pas"));
        assert_eq!((s.w, s.h), (w, h), "{name}");
        let want = premul_rgba(&z.read(&format!("expect/{name}.rgba")).unwrap());
        assert_eq!(s.px, want, "{name} : pixels");
    }
}

#[test]
fn png_refuse_le_faux() {
    assert!(png::decode(b"pas un png").is_none());
    let mut b = zip().read("Img/a.png").unwrap();
    let n = b.len();
    b.truncate(n - 20); // IDAT coupe
    assert!(png::decode(&b).is_none());
}

#[test]
fn theme_zip_images_couleurs_police() {
    let dir = std::env::temp_dir().join("cp_theme_test");
    let _ = std::fs::create_dir_all(&dir);
    let zp = dir.join("Essai.ReaperThemeZip");
    std::fs::write(&zp, FIXTURE).unwrap();
    // REAPER retient « .ReaperTheme » meme quand seul le zip existe.
    let t = ThemeFiles::open(&dir.join("Essai.ReaperTheme")).expect("theme ouvert");
    assert_eq!(t.color("col_main_bg2"), Some((0x3e, 0x3e, 0x3e)));
    assert_eq!(t.font_face("trans_font").as_deref(), Some("Roboto"));
    // L'image a l'echelle de l'ecran : 200/ a 2, la racine a 1.
    let (a1, s1) = t.image("a", 1.0).unwrap();
    assert_eq!((a1.w, a1.h, s1), (5, 3, 1.0));
    let (a2, s2) = t.image("a", 2.0).unwrap();
    assert_eq!((a2.w, a2.h, s2), (10, 6, 2.0));
    // A 1,5 : pas de 150/, donc 200/ (le plus net), reechantillonne par
    // l'appelant.
    let (_, s15) = t.image("a", 1.5).unwrap();
    assert_eq!(s15, 2.0);
    // Pas de 200/ pour pal4 : la racine.
    assert_eq!(t.image("pal4", 2.0).unwrap().1, 1.0);
    assert!(t.image("absent", 1.0).is_none());
    // Le chemin direct du zip marche aussi.
    assert!(ThemeFiles::open(&zp).is_some());
}

#[test]
fn bord_rose_et_etirement() {
    let z = zip();
    let img = png::decode(&z.read("Img/pink.png").unwrap()).unwrap();
    let n = Nine::from_image(&img, 0);
    assert_eq!((n.img.w, n.img.h), (6, 4));
    assert_eq!((n.l, n.t, n.r, n.b), (2, 1, 3, 2));
    // A sa taille : l'image interieure telle quelle.
    assert_eq!(n.render(6, 4).px, n.img.px);
    // Etiree : les coins ne bougent pas.
    let big = n.render(20, 9);
    assert_eq!(big.get(0, 0), n.img.get(0, 0));
    assert_eq!(big.get(19, 8), n.img.get(5, 3));
    assert_eq!(big.get(1, 0), n.img.get(1, 0));
    assert_eq!(big.get(17, 7), n.img.get(3, 2));
    // Sans bord rose : l'image entiere, bords fixes.
    let plain = Nine::from_image(&n.img, 1);
    assert_eq!((plain.l, plain.r), (1, 1));
}

#[test]
fn vignettes_d_etats() {
    let mut s = Surface::new(90, 30);
    for y in 0..30 {
        for x in 0..90 {
            s.px[(y * 90 + x) as usize] = 0xFF00_0000 | (x / 30) as u32;
        }
    }
    let f = Frames::toolbar(&s);
    assert_eq!(f.f.len(), 3);
    assert_eq!((f.w(), f.h()), (30, 30));
    assert_eq!(f.get(2).unwrap().get(0, 0), 0xFF00_0002);
    assert_eq!(f.get(7).unwrap().get(0, 0), 0xFF00_0002); // au-dela : la derniere
    // Une image carree : un seul etat.
    assert_eq!(Frames::toolbar(&Surface::new(30, 30)).f.len(), 1);
    // A l'echelle 2 depuis 1 : deux fois plus grande.
    let g = f.rescaled(1.0, 2.0);
    assert_eq!((g.w(), g.h()), (60, 60));
}

/// Sur les fichiers de la machine (lance a la main : `--ignored`) : notre
/// decodeur contre celui de Windows (WIC) sur chaque icone de
/// `Data/toolbar_icons`, et chaque PNG du theme courant decode.
#[test]
#[ignore]
fn vrais_fichiers() {
    let res = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("REAPER");
    let dir = res.join("Data").join("toolbar_icons");
    let (mut same, mut diff, mut fail) = (0, 0, 0);
    let mut worst = 0u32;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| !x.eq_ignore_ascii_case("png")) {
            continue;
        }
        let ours = png::decode(&std::fs::read(&p).unwrap());
        let wic = cp_sys::image::load(&p);
        match (ours, wic) {
            (Some(a), Some(b)) if a.w == b.w && a.h == b.h => {
                // WIC arrondit la premultiplication a sa facon : 1 d'ecart permis.
                let d =
                    a.px.iter()
                        .zip(&b.px)
                        .map(|(x, y)| {
                            (0..4)
                                .map(|c| ((x >> (8 * c)) & 255).abs_diff((y >> (8 * c)) & 255))
                                .max()
                                .unwrap()
                        })
                        .max()
                        .unwrap_or(0);
                worst = worst.max(d);
                if d <= 1 {
                    same += 1;
                } else {
                    diff += 1;
                    println!("ecart {d} : {}", p.display());
                }
            }
            (None, Some(_)) => {
                fail += 1;
                println!("refuse : {}", p.display());
            }
            _ => {}
        }
    }
    println!(
        "toolbar_icons : {same} identiques, {diff} differents, {fail} refuses, pire ecart {worst}"
    );
    assert_eq!((diff, fail), (0, 0));

    let ini = std::fs::read_to_string(res.join("reaper.ini")).unwrap_or_default();
    let theme = ini
        .lines()
        .find_map(|l| l.strip_prefix("lastthemefn5="))
        .unwrap()
        .trim()
        .to_string();
    let t0 = std::time::Instant::now();
    let t = ThemeFiles::open(std::path::Path::new(&theme)).expect("theme courant");
    let Source::Zip(z, _) = &t.src else { return };
    let (mut ok, mut bad) = (0, 0);
    for e in &z.entries {
        if e.name.to_ascii_lowercase().ends_with(".png") {
            match z.read_entry(e).and_then(|b| png::decode(&b)) {
                Some(_) => ok += 1,
                None => {
                    bad += 1;
                    println!("theme, refuse : {}", e.name);
                }
            }
        }
    }
    println!(
        "theme : {ok} PNG decodes, {bad} refuses, en {:.0} ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(bad, 0);
}
