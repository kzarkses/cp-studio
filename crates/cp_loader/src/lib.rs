//! `reaper_cp_studio.dll` : la petite DLL FIXE.
//!
//! REAPER la charge au demarrage et ne la recharge jamais. Elle fait donc le
//! moins possible, et ce qui doit survivre a un rechargement du module :
//! - elle tient TOUS les enregistrements aupres de REAPER (battement, actions,
//!   classe de fenetre) et les transmet au module courant. Le module ne confie
//!   jamais a REAPER un pointeur vers son propre code, qui peut disparaitre ;
//! - elle possede les fenetres : elles restent ouvertes pendant un rechargement
//!   et le nouveau code les repeint ;
//! - elle surveille `cp_module.dll` et le recharge (voir cp_hot) ;
//! - elle porte le MOTEUR AUDIO (cp_audio) : un rechargement du module ne coupe
//!   jamais le son. Le module le pilote par `HostApi::audio`.
//!
//! Le module a charger : la premiere ligne de `reaper_cp_studio.module` a cote
//! de cette DLL (ecrit par `cargo xtask install`), sinon `cp_module.dll` a cote.
//! Journal : `<ressources REAPER>/cp_studio.log` (la session precedente dans
//! `cp_studio.prev.log`).

mod selftest;
pub(crate) mod views;

use std::ffi::{CStr, c_void};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use cp_abi::{
    ClipInfo, FAULT, FrameInfo, HostApi, KeyEvent, LOG_ERROR, LOG_INFO, LOG_WARN, WM_CP_FRAME,
};
use cp_audio::{AudioHost, ClipH, Load};
use cp_engine::VoiceH;
use cp_hot::{Hot, Report};
use cp_sys::host::{self, AudioLoad, Host};
use cp_sys::reaper::Reaper;
use cp_sys::win::{self, Hwnd, Paint, WinMsg, rgb};

const POLL_EVERY: Duration = Duration::from_millis(250);
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    Reload,
    Status,
    TestLoop,
    TestFile,
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
/// Le moteur audio. Fil principal seulement ; pris en `try_lock` par ce que le
/// module appelle, pour qu'une reentrance ne se bloque jamais sur elle-meme.
static AUDIO: Mutex<Option<AudioHost>> = Mutex::new(None);
static TEST: Mutex<TestState> = Mutex::new(TestState {
    loop_clip: None,
    loop_voice: None,
    file_ticket: None,
    file_voice: None,
});

static SELFTEST: Mutex<Option<selftest::SelfTest>> = Mutex::new(None);

/// Les sons des actions de test.
struct TestState {
    loop_clip: Option<ClipH>,
    loop_voice: Option<VoiceH>,
    file_ticket: Option<u32>,
    file_voice: Option<VoiceH>,
}

/// Le moteur, s'il est la et libre.
fn audio<T>(default: T, f: impl FnOnce(&mut AudioHost) -> T) -> T {
    match AUDIO.try_lock() {
        Ok(mut g) => g.as_mut().map_or(default, f),
        Err(_) => default,
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
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
    // `try_lock` : une panique pendant une ecriture du journal ne doit pas
    // attendre le journal qu'elle tient deja.
    if let Ok(mut g) = LOG.try_lock() {
        if let Some(f) = g.as_mut() {
            let _ = writeln!(f, "[{h:02}:{m:02}:{s:02}.{ms:03}] {tag:9} {msg}");
            let _ = f.flush();
        }
    }
}

/// Chaque panique du chargeur va au journal, avec sa pile : sans ca, elle ne
/// s'ecrit que sur une console que personne ne voit.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let th = std::thread::current();
        log(
            LOG_ERROR,
            &format!(
                "PANIQUE dans le chargeur (fil {}) : {info}\n{bt}",
                th.name().unwrap_or("?")
            ),
        );
    }));
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

    fn audio_load(&self, path: &str) -> u32 {
        audio(0, |a| a.load_file(path))
    }

    fn audio_poll(&self, ticket: u32) -> AudioLoad {
        match audio(Load::Failed("audio engine busy".into()), |a| {
            a.ticket(ticket)
        }) {
            Load::Pending => AudioLoad::Pending,
            Load::Ready(c) => AudioLoad::Ready(c.0),
            Load::Failed(e) => AudioLoad::Failed(e),
        }
    }

    fn audio_info(&self, clip: u32) -> Option<ClipInfo> {
        audio(None, |a| {
            a.clip_info(ClipH(clip)).map(|(frames, nch, srate)| {
                (
                    frames,
                    nch,
                    srate,
                    a.clip_decoded(ClipH(clip)).unwrap_or(frames),
                )
            })
        })
        .map(|(frames, nch, srate, decoded)| ClipInfo {
            frames,
            nch: nch as u32,
            srate,
            decoded,
        })
    }

    fn audio_release(&self, clip: u32) {
        audio((), |a| a.release(ClipH(clip)));
    }

    fn audio_peaks(
        &self,
        clip: u32,
        ch: Option<usize>,
        start: f64,
        fpp: f64,
        min: &mut [f32],
        max: &mut [f32],
    ) -> usize {
        audio(0, |a| a.peaks(ClipH(clip), ch, start, fpp, min, max))
    }

    fn audio_play(
        &self,
        port: u32,
        clip: u32,
        start: i64,
        end: i64,
        looped: bool,
        gain: f64,
    ) -> u32 {
        audio(None, |a| {
            a.play(port as usize, ClipH(clip), start, end, looped, gain)
        })
        .map_or(0, |v| v.0)
    }

    fn audio_stop(&self, voice: u32, fade_s: f64) {
        audio((), |a| a.stop(VoiceH(voice), fade_s));
    }

    fn audio_voice(&self, voice: u32) -> Option<(f64, i32)> {
        audio(None, |a| a.voice(VoiceH(voice)))
    }

    fn audio_set(&self, voice: u32, param: u32, value: f64) -> bool {
        let Some(p) = param_from(param) else {
            return false;
        };
        audio(false, |a| a.set(VoiceH(voice), p, value))
    }

    fn audio_srate(&self) -> f64 {
        audio(0.0, |a| a.core().srate())
    }

    fn audio_thumb(&self, path: &str, min: &mut [f32], max: &mut [f32]) -> i32 {
        match audio(cp_audio::Thumb::Failed, |a| a.thumb(path)) {
            cp_audio::Thumb::Pending => 0,
            cp_audio::Thumb::Failed => -1,
            cp_audio::Thumb::Ready(d) => {
                // Reechantillonne les tranches a la taille demandee.
                let n = min.len().min(max.len());
                for i in 0..n {
                    let a = i * d.len() / n.max(1);
                    let b = ((i + 1) * d.len() / n.max(1)).max(a + 1).min(d.len());
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    for e in &d[a..b] {
                        lo = lo.min(e[0]);
                        hi = hi.max(e[1]);
                    }
                    min[i] = lo;
                    max[i] = hi;
                }
                1
            }
        }
    }

    fn create_view(
        &self,
        title: &str,
        ident: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        docked: bool,
    ) -> isize {
        let Some(r) = REAPER.get() else { return 0 };
        // Aucun verrou tenu : Windows envoie des messages pendant la creation.
        let Some(hwnd) = win::create_view(title, x, y, w, h, r.main_hwnd(), docked) else {
            log(
                LOG_ERROR,
                &format!("creation de la vue refusee ({})", win::last_error()),
            );
            return 0;
        };
        lock(&WINDOWS).push(hwnd);
        if docked {
            r.dock_add(hwnd, title, ident);
            r.dock_activate(hwnd);
        } else {
            win::show(hwnd);
        }
        hwnd.0
    }

    fn destroy_window(&self, hwnd: isize) {
        let h = Hwnd(hwnd);
        if !lock(&WINDOWS).contains(&h) {
            return;
        }
        views::forget(h);
        // Retirer du docker AVANT de detruire : dans l'autre sens, REAPER
        // garde une reference vers une fenetre morte.
        if let Some(r) = REAPER.get() {
            r.dock_remove(h);
        }
        win::destroy_window(h);
    }

    fn frames(&self, hwnd: isize, on: bool) {
        if lock(&WINDOWS).contains(&Hwnd(hwnd)) {
            views::frames(Hwnd(hwnd), on);
        }
    }

    fn frame_info(&self) -> FrameInfo {
        views::frame_info()
    }
}

/// Une touche du crochet clavier, demandee au module. Rend le code du crochet.
fn ask_module_key(h: Hwnd, ev: &KeyEvent) -> i32 {
    let Some(hot) = HOT.get() else { return 0 };
    let mut out: isize = 0;
    let lp = ev as *const KeyEvent as isize;
    match hot.call(|api| (api.wnd_proc)(h.0, views::ACCEL_MSG, ev.message as usize, lp, &mut out)) {
        Some(1) => out as i32,
        _ => 0,
    }
}

/// La coche d'une action du module.
fn toggle_state(cmd: i32) -> i32 {
    let mine = lock(&ACTIONS)
        .by_cmd
        .iter()
        .any(|&(c, o)| c == cmd && o == Owner::Module);
    if !mine {
        return -1;
    }
    HOT.get()
        .and_then(|h| h.call(|api| (api.toggle_state)(cmd)))
        .unwrap_or(-1)
}

fn param_from(n: u32) -> Option<cp_engine::Param> {
    use cp_engine::Param::*;
    const ALL: [cp_engine::Param; 19] = [
        Rate, Gain, Pan, LoopStart, LoopEnd, FadeIn, FadeOut, Pos, Loop, Env, Attack, Decay,
        Sustain, Release, ACurve, DCurve, RCurve, Gate, EnvPts,
    ];
    ALL.iter().copied().find(|p| *p as u32 == n)
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

pub(crate) fn reload(force: bool) -> Option<Result<Report, String>> {
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
    audio((), |a| a.poll());
    test_file_poll();
    selftest_tick();
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
    if let Some(st) = audio(None, |a| Some(a.stats())) {
        s += &format!(
            "  audio: {} Hz, {} blocks of {}, hook {}, ports open {}\n",
            st.srate,
            st.blocks,
            st.last_block,
            if st.hook { "on" } else { "OFF" },
            st.ports_open
        );
        s += &format!(
            "  audio pulls: {} calls, over {} / under {}, rewinds {} / skips {}, busy renders {}, lost commands {}\n",
            st.calls,
            st.over_pulls,
            st.under_pulls,
            st.rewinds,
            st.skips,
            st.busy_renders,
            st.dropped_commands
        );
        s += &format!(
            "  audio load: {:.3} % of the audio thread, {} voices, {} clips ({:.1} MB), {} decoding\n",
            st.load_percent(),
            st.active_voices,
            st.clips,
            st.bytes as f64 / 1e6,
            st.decoding
        );
    }
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
        Some(Owner::TestLoop) => {
            test_loop_toggle();
            true
        }
        Some(Owner::TestFile) => {
            test_file_start();
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
// Les sons de test
// ---------------------------------------------------------------------------

pub(crate) fn module_generation() -> u32 {
    HOT.get().map_or(0, |h| h.generation())
}

pub(crate) fn views_count() -> usize {
    lock(&WINDOWS).len()
}

pub(crate) fn first_view() -> Option<Hwnd> {
    lock(&WINDOWS).first().copied()
}

fn selftest_tick() {
    let mut g = lock(&SELFTEST);
    let Some(st) = g.as_mut() else { return };
    let done = if st.wants_audio() {
        audio(false, |a| st.tick(a));
        false
    } else {
        // Hors du verrou du moteur : l'interface appelle le module, qui
        // appelle le moteur.
        let mut taken = g.take();
        drop(g);
        let done = taken.as_mut().is_some_and(|s| s.tick_ui());
        g = lock(&SELFTEST);
        *g = taken;
        done
    };
    let Some(st) = g.as_mut() else { return };
    if done {
        let quit = st.quit_after();
        *g = None;
        drop(g);
        if quit {
            log(LOG_INFO, "[selftest] fermeture de REAPER demandee");
            // DEPOSEE, pas appelee : REAPER se demonte pendant la commande, et
            // il ne doit pas le faire pendant que notre battement est sur la
            // pile (c'est ce que fait un clic dans le menu).
            if let Some(r) = REAPER.get() {
                win::post_message(r.main_hwnd(), win::WM_COMMAND, 40004, 0);
            }
        }
    }
}

/// Lance la boucle de test, ou l'arrete (fondu de 20 ms).
fn test_loop_toggle() {
    let mut t = lock(&TEST);
    let msg = audio("CP Studio: audio engine not available\n".to_string(), |a| {
        if let Some(v) = t.loop_voice.take() {
            a.stop(v, 0.02);
            return "CP Studio: test loop stopped\n".into();
        }
        let clip = match t.loop_clip.filter(|c| a.clip_info(*c).is_some()) {
            Some(c) => c,
            None => {
                let sr = a.core().srate();
                let (s, _) = cp_audio::testloop::synth(sr);
                match a.load_samples(s, 2, sr) {
                    Ok(c) => c,
                    Err(e) => return format!("CP Studio: test loop refused: {e}\n"),
                }
            }
        };
        t.loop_clip = Some(clip);
        match a.play(0, clip, 0, 0, true, 1.0) {
            Some(v) => {
                t.loop_voice = Some(v);
                format!(
                    "CP Studio: test loop playing ({} Hz). Run the action again to stop it.\n",
                    a.core().srate()
                )
            }
            None => "CP Studio: test loop refused by the engine\n".into(),
        }
    });
    drop(t);
    log(LOG_INFO, msg.trim_end());
    console(&msg);
}

/// Demande un fichier et le joue une fois, par le decodeur et le moteur.
fn test_file_start() {
    let Some(r) = REAPER.get() else { return };
    let Some(path) = r.ask_file_to_read("CP Studio: play an audio file", "wav") else {
        return;
    };
    let mut t = lock(&TEST);
    if let Some(v) = t.file_voice.take() {
        audio((), |a| a.stop(v, 0.01));
    }
    t.file_ticket = audio(None, |a| Some(a.load_file(&path)));
    log(LOG_INFO, &format!("fichier de test demande : {path}"));
}

fn test_file_poll() {
    let mut t = lock(&TEST);
    let Some(ticket) = t.file_ticket else { return };
    let state = audio(Load::Pending, |a| a.ticket(ticket));
    match state {
        Load::Pending => {}
        Load::Ready(c) => {
            t.file_ticket = None;
            let v = audio(None, |a| {
                let v = a.play(0, c, 0, 0, false, 1.0);
                // Le clip part du vivier des que la voix l'a fini.
                a.release(c);
                v
            });
            t.file_voice = v;
            console(if v.is_some() {
                "CP Studio: playing the file\n"
            } else {
                "CP Studio: the engine refused to play the file\n"
            });
        }
        Load::Failed(e) => {
            t.file_ticket = None;
            log(LOG_WARN, &format!("fichier de test refuse : {e}"));
            console(&format!("CP Studio: cannot play this file: {e}\n"));
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
        views::forget(m.hwnd);
    }
    let frame_vblank = if m.msg == WM_CP_FRAME {
        Some(views::frame_done(m.hwnd))
    } else {
        None
    };
    if let Some(hot) = HOT.get() {
        let sane = hot.fault().is_none();
        let mut out: isize = 0;
        let t0 = cp_sys::ui::qpc();
        let r = hot.call(|api| (api.wnd_proc)(m.hwnd.0, m.msg, m.wp, m.lp, &mut out));
        if let Some(vb) = frame_vblank {
            views::record(t0 - vb, cp_sys::ui::qpc() - t0);
        }
        match r {
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
    install_panic_hook();
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
    let srate = cp_sys::audio::device_srate(&r).unwrap_or(0.0);
    *lock(&AUDIO) = Some(AudioHost::new(Some(r), srate));
    log(
        LOG_INFO,
        &format!(
            "moteur audio pret ({} Hz annonces par le peripherique, crochet {})",
            srate,
            if audio(false, |a| a.stats().hook) {
                "pose"
            } else {
                "REFUSE"
            }
        ),
    );
    *lock(&SELFTEST) = selftest::requested();
    if lock(&SELFTEST).is_some() {
        log(LOG_INFO, "[selftest] demande par CP_STUDIO_SELFTEST");
    }
    let _ = HOT.set(Hot::new(src, shadow));
    let _ = HOST_API.set(host::install(Box::new(LoaderHost)));
    if let Err(e) = win::register_class(instance, wnd_proc) {
        log(LOG_ERROR, &e);
    }
    register_action(
        "CPSTUDIO_RELOAD",
        "CP Studio: Reload module now",
        Owner::Reload,
    );
    register_action("CPSTUDIO_STATUS", "CP Studio: Show status", Owner::Status);
    register_action(
        "CPSTUDIO_TEST_LOOP",
        "CP Studio: Play/stop the test loop",
        Owner::TestLoop,
    );
    register_action(
        "CPSTUDIO_TEST_FILE",
        "CP Studio: Play an audio file (engine test)",
        Owner::TestFile,
    );
    if !r.register_command_hook(on_command) {
        log(LOG_ERROR, "hookcommand2 refuse");
    }
    if !r.register_timer(on_timer) {
        log(LOG_ERROR, "timer refuse");
    }
    views::init_keys(|| lock(&WINDOWS).clone(), ask_module_key);
    if !r.register_accelerator(views::on_key) {
        log(LOG_ERROR, "crochet clavier refuse");
    }
    if !cp_sys::rui::install_mouse_hook(views::on_mouse) {
        log(LOG_ERROR, "crochet souris refuse");
    }
    if !r.register_toggle_state(toggle_state) {
        log(LOG_ERROR, "toggleaction refuse");
    }
    reload(true);
    true
}

fn on_unload() {
    // Le son d'abord : crochet retire, apercus arretes, sources liberees.
    if let Some(mut a) = lock(&AUDIO).take() {
        a.shutdown();
    }
    if let Some(r) = REAPER.get() {
        r.unregister_timer();
        r.unregister_command_hook();
        r.unregister_accelerator();
        r.unregister_toggle_state();
    }
    cp_sys::rui::remove_mouse_hook();
    views::stop_clock();
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
