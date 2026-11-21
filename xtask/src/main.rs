//! `cargo xtask <commande>` :
//!
//! - `check`   : formatage, clippy sans avertissement, tous les tests.
//! - `module`  : reconstruit le module (REAPER le recharge seul) et dit combien
//!   de temps ca a pris.
//! - `install` : construit et installe le chargeur dans UserPlugins (REAPER
//!   doit etre ferme), et lui dit ou trouver le module de developpement.
//!
//! Dossier de REAPER : `%APPDATA%\REAPER`, ou `CP_REAPER_RESOURCE` s'il est
//! defini (installation portable).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn cargo(args: &[&str]) -> bool {
    let c = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    Command::new(c)
        .current_dir(root())
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

fn user_plugins() -> Option<PathBuf> {
    let base = env::var_os("CP_REAPER_RESOURCE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("APPDATA").map(|a| PathBuf::from(a).join("REAPER")))?;
    Some(base.join("UserPlugins"))
}

fn check() -> ExitCode {
    let ok = cargo(&["fmt", "--all", "--check"])
        && cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "-q",
            "--",
            "-D",
            "warnings",
        ])
        && cargo(&["test", "--workspace", "-q"]);
    if ok {
        println!("check : tout est vert");
        ExitCode::SUCCESS
    } else {
        eprintln!("check : ECHEC");
        ExitCode::FAILURE
    }
}

fn module() -> ExitCode {
    let t0 = Instant::now();
    if !cargo(&["build", "-q", "-p", "cp_module"]) {
        return ExitCode::FAILURE;
    }
    println!("module reconstruit en {:.1} s", t0.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

fn install() -> ExitCode {
    let Some(plugins) = user_plugins() else {
        eprintln!("dossier UserPlugins introuvable (APPDATA ou CP_REAPER_RESOURCE)");
        return ExitCode::FAILURE;
    };
    if !plugins.is_dir() {
        eprintln!("{} n'existe pas", plugins.display());
        return ExitCode::FAILURE;
    }
    if !cargo(&["build", "-q", "--release", "-p", "cp_loader"])
        || !cargo(&["build", "-q", "-p", "cp_module"])
    {
        return ExitCode::FAILURE;
    }
    let built = root()
        .join("target")
        .join("release")
        .join("reaper_cp_studio.dll");
    let dst = plugins.join("reaper_cp_studio.dll");
    let tmp = plugins.join("reaper_cp_studio.dll.new");
    if let Err(e) = fs::copy(&built, &tmp) {
        eprintln!("copie vers {} : {e}", tmp.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::rename(&tmp, &dst) {
        let _ = fs::remove_file(&tmp);
        eprintln!(
            "REAPER tourne et tient {} ({e}) : le fermer pour installer le chargeur",
            dst.display()
        );
        return ExitCode::from(2);
    }
    let module = root().join("target").join("debug").join("cp_module.dll");
    let cfg = plugins.join("reaper_cp_studio.module");
    if let Err(e) = fs::write(&cfg, format!("{}\n", module.display())) {
        eprintln!("ecriture de {} : {e}", cfg.display());
        return ExitCode::FAILURE;
    }
    println!("chargeur installe : {}", dst.display());
    println!("module de developpement : {}", module.display());
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("check") => check(),
        Some("module") => module(),
        Some("install") => install(),
        _ => {
            eprintln!("usage : cargo xtask <check | module | install>");
            ExitCode::FAILURE
        }
    }
}
