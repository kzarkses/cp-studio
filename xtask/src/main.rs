//! `cargo xtask <commande>` :
//!
//! - `check`   : formatage, clippy sans avertissement, tous les tests.
//! - `module`  : reconstruit le module (REAPER le recharge seul) et dit combien
//!   de temps ca a pris.
//! - `install` : construit et installe le chargeur dans UserPlugins (REAPER
//!   doit etre ferme), et lui dit ou trouver le module de developpement.
//! - `reaper-test` : lance le VRAI REAPER avec l'auto-test du chargeur
//!   (`CP_STUDIO_SELFTEST=quit`, tout a gain 0), lit le verdict, et ferme
//!   REAPER s'il ne s'est pas ferme seul. Refuse si REAPER est deja ouvert.
//!   REAPER est lance depuis un dossier VIDE (jamais le depot comme dossier
//!   courant d'un outil tiers).
//!
//! Dossier de REAPER : `%APPDATA%\REAPER`, ou `CP_REAPER_RESOURCE` s'il est
//! defini (installation portable).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

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

fn reaper_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq reaper.exe", "/NH"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("reaper.exe")
        })
        .unwrap_or(false)
}

fn reaper_test() -> ExitCode {
    if reaper_running() {
        eprintln!(
            "REAPER est ouvert : l'auto-test lance son propre REAPER, fermer l'autre d'abord"
        );
        return ExitCode::from(2);
    }
    let exe = env::var_os("CP_REAPER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\REAPER (x64)\reaper.exe"));
    let dir = env::temp_dir().join("cp_selftest");
    let run = dir.join("run");
    let _ = fs::create_dir_all(&run);
    let result = dir.join("result.txt");
    let _ = fs::remove_file(&result);
    let t0 = Instant::now();
    let mut child = match Command::new(&exe)
        .current_dir(&run)
        .env("CP_STUDIO_SELFTEST", "quit")
        .args(["-new", "-nosplash", "-ignoreerrors"])
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} : {e}", exe.display());
            return ExitCode::FAILURE;
        }
    };
    // Les photos demandees par l'auto-test : snap.txt (flottante), snap2.txt
    // (ancree), snap3.txt (CP Transport a la place du transport de REAPER),
    // snap4.txt (le navigateur a la place de l'arrangement).
    let snaps = [
        ("snap.txt", ""),
        ("snap2.txt", "_docked"),
        ("snap3.txt", "_transport"),
        ("snap4.txt", "_band"),
    ];
    for (f, _) in snaps {
        let _ = fs::remove_file(dir.join(f));
    }
    let mut done = [false; 4];
    while !result.exists() && t0.elapsed() < Duration::from_secs(90) {
        for (i, (f, suffix)) in snaps.iter().enumerate() {
            let p = dir.join(f);
            if !done[i] && p.exists() {
                done[i] = true;
                let hwnd = fs::read_to_string(&p).unwrap_or_default();
                capture(&run, hwnd.trim(), &dir, suffix);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // Laisser REAPER se fermer seul ; sinon (une autre extension peut bloquer
    // la sortie), le fermer.
    let t1 = Instant::now();
    while t1.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Ok(None) = child.try_wait() {
        let _ = child.kill();
        let _ = child.wait();
        println!("(REAPER ne s'est pas ferme seul : arrete)");
    }
    match fs::read_to_string(&result) {
        Ok(text) => {
            print!("{text}");
            println!("({:.1} s)", t0.elapsed().as_secs_f64());
            if text.trim_end().ends_with("PASS") {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(_) => {
            eprintln!("pas de verdict en 90 s (voir cp_studio.log)");
            ExitCode::FAILURE
        }
    }
}

/// Photographie la fenetre du navigateur et la fenetre principale de REAPER
/// (et rien d'autre de l'ecran) dans `dir`.
fn capture(run: &Path, hwnd: &str, dir: &Path, suffix: &str) {
    let script = r#"param([string]$hwnd, [string]$out, [string]$suffix)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L, T, R, B; }
}
"@
[W]::SetProcessDPIAware() | Out-Null
function Shot([IntPtr]$h, [string]$file) {
  $r = New-Object W+RECT
  if (-not [W]::GetWindowRect($h, [ref]$r)) { return }
  $w = $r.R - $r.L; $hh = $r.B - $r.T
  if ($w -le 0 -or $hh -le 0) { return }
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($file)
  $g.Dispose(); $bmp.Dispose()
}
if ($hwnd -ne "0") { Shot ([IntPtr][Int64]$hwnd) (Join-Path $out ("browser" + $suffix.TrimStart("_") + ".png")) }
$p = Get-Process reaper -ErrorAction SilentlyContinue | Select-Object -First 1
if ($p) { Shot $p.MainWindowHandle (Join-Path $out ("reaper" + $suffix.TrimStart("_") + ".png")) }
"#;
    let ps1 = run.join("snap.ps1");
    if fs::write(&ps1, script).is_err() {
        return;
    }
    let st = Command::new("powershell")
        .current_dir(run)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&ps1)
        .arg(hwnd)
        .arg(dir)
        .arg(if suffix.is_empty() { "_" } else { suffix })
        .status();
    match st {
        Ok(s) if s.success() => println!("photos : {}", dir.display()),
        _ => println!("(photo impossible)"),
    }
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("check") => check(),
        Some("module") => module(),
        Some("install") => install(),
        Some("reaper-test") => reaper_test(),
        _ => {
            eprintln!("usage : cargo xtask <check | module | install | reaper-test>");
            ExitCode::FAILURE
        }
    }
}
