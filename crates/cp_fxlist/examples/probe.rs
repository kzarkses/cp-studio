//! Lit les fichiers d'un dossier de ressources de REAPER et dit ce que le
//! catalogue en tire : combien d'effets, et combien d'elements de chaque
//! dossier il retrouve. La liste de REAPER (`EnumInstalledFX`) n'existant pas
//! hors de REAPER, elle est reconstituee des fichiers .ini (sans ident).
//!
//! `cargo run -p cp_fxlist --example probe -- <dossier de ressources>`

use cp_fxlist::{Catalog, IniTexts, ini};

fn read(p: &std::path::Path) -> String {
    std::fs::read(p)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage : probe <dossier de ressources de REAPER>");
        return;
    };
    let d = std::path::Path::new(&dir);
    let vst = read(&d.join("reaper-vstplugins64.ini"));
    let clap = read(&d.join("reaper-clap-win64.ini"));
    let jsfx = read(&d.join("reaper-jsfx.ini"));
    let mut installed = Vec::new();
    for l in ini::parse_vst_ini(&vst) {
        let k = if l.file.ends_with(".vst3") {
            "VST3"
        } else {
            "VST"
        };
        let i = if l.instrument { "i" } else { "" };
        installed.push((format!("{k}{i}: {}", l.name), String::new()));
    }
    for l in ini::parse_clap_ini(&clap) {
        let i = if l.instrument { "i" } else { "" };
        installed.push((format!("CLAP{i}: {}", l.name), String::new()));
    }
    for (_, n) in ini::parse_jsfx_ini(&jsfx) {
        installed.push((n, String::new()));
    }
    let chains: Vec<String> = std::fs::read_dir(d.join("FXChains"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.to_lowercase().ends_with(".rfxchain"))
                .collect()
        })
        .unwrap_or_default();
    let texts = IniTexts {
        vst: vec![vst],
        clap: vec![clap],
        jsfx,
        folders: read(&d.join("reaper-fxfolders.ini")),
        tags: read(&d.join("reaper-fxtags.ini")),
    };
    let t = std::time::Instant::now();
    let c = Catalog::build(&installed, &texts, &chains);
    println!(
        "{} effets ({} instruments), {} chaines, en {:.1} ms",
        c.plugins.len(),
        c.plugins.iter().filter(|p| p.instrument).count(),
        chains.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "{} categories, {} developpeurs, {} sans developpeur, {} sans fichier",
        c.cats.len(),
        c.devs.len(),
        c.plugins.iter().filter(|p| p.dev.is_empty()).count(),
        c.plugins.iter().filter(|p| p.file.is_empty()).count()
    );
    let folders = ini::parse_folders(&texts.folders);
    for (f, r) in folders.iter().zip(&c.folders) {
        println!(
            "  {:<18} {:>4} / {:>4}",
            f.name,
            r.items.len(),
            f.items.len()
        );
        if std::env::args().any(|a| a == "-v") {
            let mut seen = std::collections::HashSet::new();
            for it in &f.items {
                let k = ini::norm_file(&it.value);
                let js = ini::norm_js(&it.value);
                let found = c.plugins.iter().any(|p| p.file == k || p.file == js);
                if !found {
                    println!("      absent : [{}] {}", it.kind, it.value);
                } else if !seen.insert(k.clone()) {
                    println!("      deux fois : {}", it.value);
                }
            }
        }
    }
}
