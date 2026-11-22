//! L'oracle : l'ancien moteur C++ (`reference/cp_native`), compile en DLL et
//! pilote depuis Rust, pour que `cp_engine` soit compare a lui echantillon par
//! echantillon.
//!
//! [`build`] le compile avec MSVC dans un dossier temporaire HORS du depot (un
//! outil tiers ne tourne jamais avec un dossier de valeur comme dossier
//! courant), et seulement si une source a change. [`Oracle`] l'enveloppe.
//!
//! Exception ecrite : ce crate contient du code `unsafe` (il appelle une DLL
//! C++). Il ne sert qu'aux tests et n'est jamais livre.

use std::ffi::c_void;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// `kNow` de l'ancien moteur.
pub const K_NOW: i64 = i64::MIN;
/// `kNullVoice`.
pub const NULL_VOICE: u32 = 0xFFFF_FFFF;

/// Les numeros de `CmdType` (cp_types.h).
pub mod cmd {
    pub const PLAY: u32 = 1;
    pub const STOP: u32 = 2;
    pub const SET: u32 = 3;
    pub const QUEUE: u32 = 4;
    pub const PORT_GAIN: u32 = 5;
    pub const ENV_PT: u32 = 9;
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn newest(paths: &[PathBuf]) -> Option<SystemTime> {
    paths
        .iter()
        .filter_map(|p| fs::metadata(p).ok()?.modified().ok())
        .max()
}

/// Compile l'oracle si besoin et rend le chemin de la DLL. Une seule
/// compilation par processus, meme si les tests tournent en parallele.
pub fn build() -> Result<PathBuf, String> {
    static ONCE: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(build_oracle).clone()
}

fn build_oracle() -> Result<PathBuf, String> {
    let refdir = workspace_root().join("reference");
    let core = refdir.join("cp_native").join("src").join("core");
    let oracle_dir = refdir.join("oracle");
    // cp_lanes.cpp est incomplet (voir oracle/lanes_stub.cpp).
    let units: Vec<PathBuf> = ["cp_engine.cpp", "cp_voice.cpp", "cp_pool.cpp"]
        .iter()
        .map(|f| core.join(f))
        .chain(
            ["oracle.cpp", "lanes_stub.cpp"]
                .iter()
                .map(|f| oracle_dir.join(f)),
        )
        .collect();
    let mut deps = units.clone();
    if let Ok(rd) = fs::read_dir(&core) {
        deps.extend(rd.flatten().map(|e| e.path()));
    }
    build_dll("cp_oracle", &units, &deps, &[])
}

/// Le SDK de REAPER : `$REAPER_SDK`, sinon `../reaper-sdk/sdk` a cote du depot.
pub fn reaper_sdk() -> Result<PathBuf, String> {
    let p = match std::env::var_os("REAPER_SDK") {
        Some(v) => PathBuf::from(v),
        None => workspace_root().join("..").join("reaper-sdk").join("sdk"),
    };
    if p.join("reaper_plugin.h").exists() {
        Ok(p)
    } else {
        Err(format!(
            "SDK de REAPER introuvable ({}) : cloner github.com/justinfrankel/reaper-sdk a cote du depot, ou poser REAPER_SDK",
            p.display()
        ))
    }
}

/// Le programme C++ compile contre le SDK qui verifie nos tables virtuelles et
/// nos structures (`cpp/pcm_abi.cpp`, voir `tests/pcm_abi.rs`).
pub fn build_pcm_abi() -> Result<PathBuf, String> {
    static ONCE: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let sdk = reaper_sdk()?;
        let unit = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("cpp")
            .join("pcm_abi.cpp");
        let deps = [unit.clone(), sdk.join("reaper_plugin.h")];
        build_dll("cp_pcm_abi", std::slice::from_ref(&unit), &deps, &[sdk])
    })
    .clone()
}

/// L'etireur de REAPER imite en C++ contre le SDK (`cpp/pitch_abi.cpp`,
/// voir `tests/pitch_abi.rs`).
pub fn build_pitch_abi() -> Result<PathBuf, String> {
    static ONCE: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let sdk = reaper_sdk()?;
        let unit = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("cpp")
            .join("pitch_abi.cpp");
        let deps = [unit.clone(), sdk.join("reaper_plugin.h")];
        build_dll("cp_pitch_abi", std::slice::from_ref(&unit), &deps, &[sdk])
    })
    .clone()
}

/// Compile une DLL C++ avec MSVC dans `%TEMP%\cp_oracle_build`, seulement si
/// une dependance a change.
fn build_dll(
    name: &str,
    units: &[PathBuf],
    deps: &[PathBuf],
    includes: &[PathBuf],
) -> Result<PathBuf, String> {
    let out = std::env::temp_dir().join("cp_oracle_build");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let dll = out.join(format!("{name}.dll"));
    let built = fs::metadata(&dll).ok().and_then(|m| m.modified().ok());
    if let (Some(b), Some(n)) = (built, newest(deps)) {
        if b >= n {
            return Ok(dll);
        }
    }

    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    let vs = Command::new(&vswhere)
        .current_dir(&out)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .map_err(|e| {
            format!("vswhere introuvable ({e}) : Visual Studio C++ est necessaire a l'oracle")
        })?;
    let vs = String::from_utf8_lossy(&vs.stdout).trim().to_string();
    if vs.is_empty() {
        return Err("Visual Studio C++ introuvable".into());
    }
    let vcvars = Path::new(&vs).join(r"VC\Auxiliary\Build\vcvars64.bat");

    // Les memes reglages que l'ancien binaire : /O2, /fp:precise (le defaut),
    // SSE2 (le defaut x64), runtime lie.
    let files: Vec<String> = units
        .iter()
        .map(|p| format!("\"{}\"", p.display()))
        .collect();
    let incs: Vec<String> = includes
        .iter()
        .map(|p| format!("/I\"{}\"", p.display()))
        .collect();
    let objdir = out.join(format!("{name}_obj"));
    fs::create_dir_all(&objdir).map_err(|e| e.to_string())?;
    let log = format!("{name}.log");
    let script = format!(
        "@echo off\r\ncall \"{}\" >nul 2>&1\r\ncl /nologo /LD /O2 /EHsc /std:c++17 /MT /W3 /fp:precise {} /Fe\"{}\" /Fo\"{}\\\\\" {} > {log} 2>&1\r\nexit /b %errorlevel%\r\n",
        vcvars.display(),
        incs.join(" "),
        dll.display(),
        objdir.display(),
        files.join(" ")
    );
    let cmd_file = out.join(format!("build_{name}.cmd"));
    fs::write(&cmd_file, script).map_err(|e| e.to_string())?;
    let st = Command::new("cmd")
        .current_dir(&out)
        .arg("/c")
        .arg(&cmd_file)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        let log = fs::read_to_string(out.join(&log)).unwrap_or_default();
        return Err(format!(
            "compilation de {name} en echec :
{log}"
        ));
    }
    Ok(dll)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn FreeLibrary(h: *mut c_void) -> i32;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

type E = *mut c_void;

#[allow(clippy::type_complexity)]
struct Fns {
    new: extern "C" fn(f64) -> E,
    free: extern "C" fn(E),
    set_srate: extern "C" fn(E, f64),
    clip: extern "C" fn(E, *const f32, i64, i32, f64) -> i32,
    alloc: extern "C" fn(E, i32) -> u32,
    release: extern "C" fn(E, u32),
    voice_port: extern "C" fn(E, u32) -> i32,
    post: extern "C" fn(E, i32, u32, u32, i64, f64, f64, u32, u32) -> i32,
    render: extern "C" fn(E, i32, *mut f32, i32, i32) -> i64,
    tick: extern "C" fn(E, i32),
    query: extern "C" fn(E, u32, *mut f64, *mut i32) -> i32,
    started_at: extern "C" fn(E, u32) -> i64,
    panic: extern "C" fn(E),
    port_reset: extern "C" fn(E, i32),
    clock: extern "C" fn(E) -> i64,
    owned: extern "C" fn(E) -> i32,
    port_list_size: extern "C" fn(E, i32) -> i32,
}

/// Un ancien moteur, vivant.
pub struct Oracle {
    lib: *mut c_void,
    f: Fns,
    e: E,
}

macro_rules! sym {
    ($lib:expr, $name:literal) => {{
        // SAFETY: handle valide ; nom termine par zero.
        let p = unsafe { GetProcAddress($lib, concat!($name, "\0").as_ptr()) };
        if p.is_null() {
            return Err(format!("symbole absent : {}", $name));
        }
        // SAFETY: la signature est celle d'oracle.cpp (meme ordre, memes types).
        unsafe { std::mem::transmute::<*mut c_void, _>(p) }
    }};
}

impl Oracle {
    // `sym!` transmute vers le type du champ de `Fns` qu'il remplit : le
    // compilateur le deduit et le verifie, l'ecrire deux fois n'ajoute rien.
    #[allow(clippy::missing_transmute_annotations)]
    pub fn load(dll: &Path, srate: f64) -> Result<Oracle, String> {
        let w: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: chemin termine par zero.
        let lib = unsafe { LoadLibraryW(w.as_ptr()) };
        if lib.is_null() {
            return Err(format!("LoadLibrary {}", dll.display()));
        }
        let f = Fns {
            new: sym!(lib, "ora_new"),
            free: sym!(lib, "ora_free"),
            set_srate: sym!(lib, "ora_set_srate"),
            clip: sym!(lib, "ora_clip"),
            alloc: sym!(lib, "ora_alloc"),
            release: sym!(lib, "ora_release"),
            voice_port: sym!(lib, "ora_voice_port"),
            post: sym!(lib, "ora_post"),
            render: sym!(lib, "ora_render"),
            tick: sym!(lib, "ora_tick"),
            query: sym!(lib, "ora_query"),
            started_at: sym!(lib, "ora_started_at"),
            panic: sym!(lib, "ora_panic"),
            port_reset: sym!(lib, "ora_port_reset"),
            clock: sym!(lib, "ora_clock"),
            owned: sym!(lib, "ora_owned"),
            port_list_size: sym!(lib, "ora_port_list_size"),
        };
        let e = (f.new)(srate);
        Ok(Oracle { lib, f, e })
    }

    pub fn set_srate(&mut self, sr: f64) {
        (self.f.set_srate)(self.e, sr)
    }
    pub fn clip(&mut self, data: &[f32], nch: usize, srate: f64) -> i32 {
        (self.f.clip)(
            self.e,
            data.as_ptr(),
            (data.len() / nch) as i64,
            nch as i32,
            srate,
        )
    }
    pub fn alloc(&mut self, port: usize) -> u32 {
        (self.f.alloc)(self.e, port as i32)
    }
    pub fn release(&mut self, h: u32) {
        (self.f.release)(self.e, h)
    }
    pub fn voice_port(&self, h: u32) -> i32 {
        (self.f.voice_port)(self.e, h)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &mut self,
        port: i32,
        ty: u32,
        voice: u32,
        at: i64,
        a: f64,
        b: f64,
        u0: u32,
        u1: u32,
    ) -> bool {
        (self.f.post)(self.e, port, ty, voice, at, a, b, u0, u1) != 0
    }
    pub fn render(&mut self, port: usize, out: &mut [f32], frames: usize, nch: usize) -> i64 {
        assert!(out.len() >= frames * nch);
        (self.f.render)(
            self.e,
            port as i32,
            out.as_mut_ptr(),
            frames as i32,
            nch as i32,
        )
    }
    pub fn tick(&mut self, frames: i32) {
        (self.f.tick)(self.e, frames)
    }
    pub fn query(&self, h: u32) -> Option<(f64, i32)> {
        let (mut p, mut s) = (0.0, 0);
        ((self.f.query)(self.e, h, &mut p, &mut s) != 0).then_some((p, s))
    }
    pub fn started_at(&self, h: u32) -> i64 {
        (self.f.started_at)(self.e, h)
    }
    pub fn panic(&mut self) {
        (self.f.panic)(self.e)
    }
    pub fn port_reset(&mut self, port: usize) {
        (self.f.port_reset)(self.e, port as i32)
    }
    pub fn clock(&self) -> i64 {
        (self.f.clock)(self.e)
    }
    pub fn owned(&self) -> i32 {
        (self.f.owned)(self.e)
    }
    pub fn port_list_size(&self, port: usize) -> i32 {
        (self.f.port_list_size)(self.e, port as i32)
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        (self.f.free)(self.e);
        // SAFETY: plus aucun pointeur de fonction de la DLL n'est appele.
        unsafe { FreeLibrary(self.lib) };
    }
}

/// Une DLL chargee, pour les tests qui appellent leurs propres symboles.
pub struct Dll(*mut c_void);

impl Dll {
    pub fn load(path: &Path) -> Result<Dll, String> {
        let w: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: chemin termine par zero.
        let h = unsafe { LoadLibraryW(w.as_ptr()) };
        if h.is_null() {
            return Err(format!("LoadLibrary {}", path.display()));
        }
        Ok(Dll(h))
    }

    /// L'adresse d'un symbole exporte (non nulle).
    pub fn sym(&self, name: &str) -> Result<*mut c_void, String> {
        let c = format!("{name}\0");
        // SAFETY: handle valide, nom termine par zero.
        let p = unsafe { GetProcAddress(self.0, c.as_ptr()) };
        if p.is_null() {
            Err(format!("symbole absent : {name}"))
        } else {
            Ok(p)
        }
    }
}
