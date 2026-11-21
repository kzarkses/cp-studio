//! `reaper_cp_studio.dll` : la petite DLL FIXE.
//!
//! REAPER la charge au demarrage et ne la recharge jamais. Elle fait donc le
//! moins possible, et ce qui doit survivre a un rechargement du module :
//! - elle tient TOUS les enregistrements aupres de REAPER (battement, actions,
//!   classe de fenetre) et les transmet au module courant. Le module ne confie
//!   jamais a REAPER un pointeur vers son propre code, qui peut disparaitre ;
//! - elle possede les fenetres : elles restent ouvertes pendant un rechargement
//!   et le nouveau code les repeint ;
//! - elle surveille `cp_module.dll` et le recharge (voir cp_hot).
//!
//! Le module a charger : la premiere ligne de `reaper_cp_studio.module` a cote
//! de cette DLL (ecrit par `cargo xtask install`), sinon `cp_module.dll` a cote.
//! Journal : `<ressources REAPER>/cp_studio.log` (la session precedente dans
//! `cp_studio.prev.log`).

use std::ffi::{CStr, c_void};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use cp_abi::{FAULT, HostApi, LOG_ERROR, LOG_INFO, LOG_WARN};
use cp_hot::{Hot, Report};
use cp_sys::host::{self, Host};
use cp_sys::reaper::Reaper;
use cp_sys::win::{self, Hwnd, Paint, WinMsg, rgb};

const POLL_EVERY: Duration = Duration::from_millis(250);
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    Reload,
    Status,
    Module,
}

struct Actions {
    by_cmd: Vec<(i32, Owner)>,
    by_id: Vec<(String, i32)>,
}

static REAPER: OnceLock<Reaper> = OnceLock::new();
static HOT: OnceLock<Hot> = OnceLock::new();
static HOST_API: OnceLock<&'static HostApi> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG: Mutex<Option<File>> = Mutex::new(None);
static ACTIONS: Mutex<Actions> = Mutex::new(Actions {
    by_cmd: Vec::new(),
    by_id: Vec::new(),
});
static WINDOWS: Mutex<Vec<Hwnd>> = Mutex::new(Vec::new());
static LAST_POLL: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_RELOAD: Mutex<Option<String>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

fn open_log(r: &Reaper) {
    let Some(dir) = r.resource_path() else { return };
    let path = dir.join("cp_studio.log");
    let _ = fs::rename(&path, dir.join("cp_studio.prev.log"));
    if let Ok(f) = File::create(&path) {
        *lock(&LOG) = Some(f);
        let _ = LOG_PATH.set(path);
    }
}

fn log(level: u32, msg: &str) {
    let (h, m, s, ms) = win::local_time();
    let tag = match level {
        LOG_WARN => "attention",
        LOG_ERROR => "ERREUR",
        _ => "info",
    };
    if let Some(f) = lock(&LOG).as_mut() {
        let _ = writeln!(f, "[{h:02}:{m:02}:{s:02}.{ms:03}] {tag:9} {msg}");
        let _ = f.flush();
    }
}

fn console(msg: &str) {
    if let Some(r) = REAPER.get() {
        r.show_console_msg(msg);
    }
}

// ---------------------------------------------------------------------------
// Ce que le module peut demander
// ---------------------------------------------------------------------------

struct LoaderHost;

impl Host for LoaderHost {
    fn log(&self, level: u32, msg: &str) {
        log(level, &format!("[module] {msg}"));
    }

    fn get_func(&self, name: &CStr) -> *mut c_void {
        REAPER
            .get()
            .map_or(std::ptr::null_mut(), |r| r.get_func(name))
    }

    fn register_action(&self, id: &str, name: &str) -> i32 {
        register_action(id, name, Owner::Module)
    }

    fn create_window(&self, _kind: u32, title: &str, w: i32, h: i32) -> isize {
        let Some(r) = REAPER.get() else { return 0 };
        // Aucun verrou tenu ici : Windows envoie des messages pendant la
        // creation, et ils remontent jusqu'au module.
        match win::create_window(title, w, h, r.main_hwnd()) {
            Some(hwnd) => {
                lock(&WINDOWS).push(hwnd);
                hwnd.0
            }
            None => {
                log(
                    LOG_ERROR,
                    &format!("CreateWindowExW a echoue ({})", win::last_error()),
                );
                0
            }
        }
    }

    fn store_state(&self, data: &[u8]) {
        if let Some(h) = HOT.get() {
            h.store_state(data);
        }
    }

    fn generation(&self) -> u32 {
        HOT.get().map_or(0, |h| h.generation())
    }
}

/// Idempotent par identifiant : apres un rechargement, le module retrouve le
/// meme numero de commande (REAPER garde l'action, et les raccourcis de
/// Cedric dessus).
fn register_action(id: &str, name: &str, owner: Owner) -> i32 {
    {
        let mut a = lock(&ACTIONS);
        if let Some(&(_, cmd)) = a.by_id.iter().find(|(i, _)| i == id) {
            if let Some(e) = a.by_cmd.iter_mut().find(|(c, _)| *c == cmd) {
                e.1 = owner;
            }
            return cmd;
        }
    }
    let Some(r) = REAPER.get() else { return 0 };
    let cmd = r.register_action(id, name);
    if cmd == 0 {
        log(LOG_ERROR, &format!("action refusee par REAPER : {id}"));
        return 0;
    }
    let mut a = lock(&ACTIONS);
    a.by_id.push((id.to_string(), cmd));
    a.by_cmd.push((cmd, owner));
    cmd
}

// ---------------------------------------------------------------------------
// Rechargement
// ---------------------------------------------------------------------------

fn invalidate_all() {
    let ws = lock(&WINDOWS).clone();
    for h in ws {
        win::invalidate(h);
    }
}

fn reload(force: bool) -> Option<Result<Report, String>> {
    let (hot, api) = (HOT.get()?, HOST_API.get()?);
    let r = hot.poll(Instant::now(), api, force)?;
    match &r {
        Ok(rep) => {
            let (h, m, s, _) = win::local_time();
            let line = format!(
                "generation {} a {h:02}:{m:02}:{s:02}, {} ms, etat {}{}",
                rep.generation,
                rep.elapsed.as_millis(),
                if rep.state_saved {
                    "transmis"
                } else {
                    "repris du dernier rangement"
                },
                if rep.init_ok { "" } else { ", INIT EN ECHEC" },
            );
            log(LOG_INFO, &format!("module charge : {line}"));
            *lock(&LAST_RELOAD) = Some(line);
            invalidate_all();
        }
        Err(e) => log(
            LOG_ERROR,
            &format!("rechargement refuse, l'ancien module reste : {e}"),
        ),
    }
    Some(r)
}

fn check_fault(before: bool, what: &str) {
    if let Some(hot) = HOT.get() {
        if before && hot.fault().is_some() {
            log(
                LOG_ERROR,
                &format!("module mis de cote ({what}) jusqu'a la prochaine construction"),
            );
            invalidate_all();
        }
    }
}

fn on_timer() {
    let now = Instant::now();
    let due = {
        let mut lp = lock(&LAST_POLL);
        let due = lp.is_none_or(|t| now.duration_since(t) >= POLL_EVERY);
        if due {
            *lp = Some(now);
        }
        due
    };
    if due {
        reload(false);
    }
    if let Some(hot) = HOT.get() {
        let sane = hot.fault().is_none();
        hot.call(|api| (api.tick)());
        check_fault(sane, "battement");
    }
}

fn status_text() -> String {
    let mut s = String::from("CP Studio status\n");
    s += &format!("  loader: {VERSION} (ABI {})\n", cp_abi::ABI);
    if let Some(hot) = HOT.get() {
        let src = hot.source();
        s += &format!(
            "  module source: {} ({})\n",
            src.display(),
            if src.exists() { "found" } else { "MISSING" }
        );
        s += &format!("  generation: {}\n", hot.generation());
        let st = match hot.fault() {
            Some(f) => format!("faulted: {f}"),
            None if hot.is_loaded() => "running".into(),
            None => "not loaded".into(),
        };
        s += &format!("  state: {st}\n");
    }
    if let Some(l) = lock(&LAST_RELOAD).as_ref() {
        s += &format!("  last reload: {l}\n");
    }
    s += &format!("  windows: {}\n", lock(&WINDOWS).len());
    if let Some(p) = LOG_PATH.get() {
        s += &format!("  log: {}\n", p.display());
    }
    s
}

fn on_command(cmd: i32) -> bool {
    let owner = lock(&ACTIONS)
        .by_cmd
        .iter()
        .find(|(c, _)| *c == cmd)
        .map(|e| e.1);
    match owner {
        None => false,
        Some(Owner::Reload) => {
            let msg = match reload(true) {
                Some(Ok(r)) => {
                    format!("CP Studio: module reloaded (generation {})\n", r.generation)
                }
                Some(Err(e)) => format!("CP Studio: reload refused, previous module kept: {e}\n"),
                None => "CP Studio: reload deferred (module busy)\n".into(),
            };
            console(&msg);
            true
        }
        Some(Owner::Status) => {
            console(&status_text());
            true
        }
        Some(Owner::Module) => {
            let hot = HOT.get();
            let sane = hot.is_some_and(|h| h.fault().is_none());
            match hot.and_then(|h| h.call(|api| (api.on_action)(cmd))) {
                Some(FAULT) => check_fault(sane, "action"),
                Some(_) => {}
                None => log(
                    LOG_WARN,
                    "action du module ignoree : module absent ou en faute",
                ),
            }
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Fenetres
// ---------------------------------------------------------------------------

fn paint_placeholder(h: Hwnd) {
    let Some(p) = Paint::begin(h) else { return };
    let (w, hh) = p.size();
    p.fill(0, 0, w, hh, rgb(28, 28, 32));
    p.text(24, 20, "CP Studio", rgb(230, 230, 235));
    let line = match HOT.get() {
        Some(hot) => match hot.fault() {
            Some(f) => format!("Module stopped: {f}. Waiting for the next build."),
            None if hot.is_loaded() => "Module busy...".into(),
            None => "Module not loaded. Waiting for a build.".into(),
        },
        None => "Loader not initialised.".into(),
    };
    p.text(24, 48, &line, rgb(200, 160, 120));
}

fn wnd_proc(m: &WinMsg) -> isize {
    if m.msg == win::WM_NCDESTROY {
        lock(&WINDOWS).retain(|h| *h != m.hwnd);
    }
    if let Some(hot) = HOT.get() {
        let sane = hot.fault().is_none();
        let mut out: isize = 0;
        match hot.call(|api| (api.wnd_proc)(m.hwnd.0, m.msg, m.wp, m.lp, &mut out)) {
            Some(1) => return out,
            Some(FAULT) => check_fault(sane, "fenetre"),
            _ => {}
        }
    }
    match m.msg {
        win::WM_PAINT => {
            paint_placeholder(m.hwnd);
            0
        }
        win::WM_ERASEBKGND => 1,
        _ => win::def_window_proc(m),
    }
}

// ---------------------------------------------------------------------------
// Entree et sortie
// ---------------------------------------------------------------------------

fn module_source(plugins: &Path) -> PathBuf {
    let cfg = plugins.join("reaper_cp_studio.module");
    if let Ok(text) = fs::read_to_string(&cfg) {
        if let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
            return PathBuf::from(line);
        }
    }
    plugins.join("cp_module.dll")
}

fn on_load(instance: isize, r: Reaper) -> bool {
    let _ = REAPER.set(r);
    open_log(&r);
    log(
        LOG_INFO,
        &format!("chargeur {VERSION} demarre (ABI {})", cp_abi::ABI),
    );
    let Some(dll) = win::module_path(instance) else {
        log(LOG_ERROR, "chemin du chargeur introuvable");
        return false;
    };
    let plugins = dll.parent().map(Path::to_path_buf).unwrap_or_default();
    let src = module_source(&plugins);
    let shadow = std::env::temp_dir().join("cp_studio_hot");
    let cleaned = cp_hot::clean_shadow_dir(&shadow);
    log(
        LOG_INFO,
        &format!(
            "module surveille : {} ({cleaned} copies anciennes effacees)",
            src.display()
        ),
    );
    let _ = HOT.set(Hot::new(src, shadow));
    let _ = HOST_API.set(host::install(Box::new(LoaderHost)));
    if let Err(e) = win::register_class(instance, wnd_proc) {
        log(LOG_ERROR, &e);
    }
    register_action(
        "_CPSTUDIO_RELOAD",
        "CP Studio: Reload module now",
        Owner::Reload,
    );
    register_action("_CPSTUDIO_STATUS", "CP Studio: Show status", Owner::Status);
    if !r.register_command_hook(on_command) {
        log(LOG_ERROR, "hookcommand2 refuse");
    }
    if !r.register_timer(on_timer) {
        log(LOG_ERROR, "timer refuse");
    }
    reload(true);
    true
}

fn on_unload() {
    if let Some(r) = REAPER.get() {
        r.unregister_timer();
        r.unregister_command_hook();
    }
    if let Some(hot) = HOT.get() {
        hot.unload();
    }
    let ws = lock(&WINDOWS).clone();
    for h in ws {
        win::destroy_window(h);
    }
    log(LOG_INFO, "chargeur arrete");
}

cp_sys::export_reaper_plugin!(on_load, on_unload);
