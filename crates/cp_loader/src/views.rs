//! Les vues : la cadence de l'ecran, et le clavier des fenetres ancrees.
//!
//! LA CADENCE. Un fil attend chaque composition de l'ecran (`DwmFlush`) et
//! depose un WM_CP_FRAME a chaque vue qui en veut. Une vue qui n'a pas fini
//! l'image precedente n'en recoit pas de nouvelle : la file ne s'emballe
//! jamais, et l'image sautee est COMPTEE (`missed`). Le dessin se fait donc
//! juste apres une composition, et l'image part a la suivante : une image de
//! latence au plus.
//!
//! LE CLAVIER (les « portes » de l'ancienne suite, ici en un seul endroit) :
//! - chaque touche passe d'abord par le crochet « accelerator » de REAPER ;
//!   si elle vise une de nos vues (ou un de ses enfants), le module decide :
//!   la manger, la laisser a REAPER (ses raccourcis), ou la recevoir
//!   normalement (texte) ;
//! - REAPER reprend le focus juste apres un clic dans une vue ancree : une
//!   touche qui vise alors REAPER nous revient quand meme si le DERNIER CLIC
//!   etait chez nous (crochet souris du fil principal) et que le focus n'est pas
//!   dans un champ de texte.

use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;

use cp_abi::{FrameInfo, KeyEvent, WM_CP_ACCEL, WM_CP_FRAME};
use cp_sys::rui::{Msg, accel};
use cp_sys::ui;
use cp_sys::win::{self, Hwnd};

use crate::lock;

// ---------------------------------------------------------------------------
// La cadence
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Clock {
    want: Vec<Hwnd>,
    /// Les vues qui n'ont pas fini leur image, et l'instant de la
    /// composition qui l'a declenchee.
    pending: Vec<(Hwnd, i64)>,
    info: FrameInfo,
    stop: bool,
}

static CLOCK: Mutex<Clock> = Mutex::new(Clock {
    want: Vec::new(),
    pending: Vec::new(),
    info: FrameInfo {
        count: 0,
        vblank_qpc: 0,
        period_qpc: 0,
        qpc_freq: 0,
        missed: 0,
    },
    stop: false,
});
static WAKE: Condvar = Condvar::new();

/// Ce que coute chaque image, mesure par le chargeur : le RETARD (de la
/// composition de l'ecran au traitement du message par la fenetre : le fil
/// principal etait-il occupe ?) et le DESSIN (le temps passe dans le module).
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCost {
    pub frames: u64,
    pub max_late_ms: f64,
    pub max_draw_ms: f64,
    pub late_over_period: u64,
}

static COST: Mutex<FrameCost> = Mutex::new(FrameCost {
    frames: 0,
    max_late_ms: 0.0,
    max_draw_ms: 0.0,
    late_over_period: 0,
});

pub fn cost() -> FrameCost {
    *lock(&COST)
}

pub fn reset_cost() {
    *lock(&COST) = FrameCost::default();
}

/// Une image traitee : `late` de la composition au debut du traitement,
/// `draw` le temps du module (en compteur).
pub fn record(late_qpc: i64, draw_qpc: i64) {
    let (freq, period) = {
        let c = lock(&CLOCK);
        (c.info.qpc_freq.max(1), c.info.period_qpc)
    };
    let ms = |q: i64| q as f64 * 1000.0 / freq as f64;
    let mut k = lock(&COST);
    k.frames += 1;
    k.max_late_ms = k.max_late_ms.max(ms(late_qpc));
    k.max_draw_ms = k.max_draw_ms.max(ms(draw_qpc));
    if period > 0 && late_qpc + draw_qpc > period {
        k.late_over_period += 1;
    }
}
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn clock_thread() {
    let freq = ui::qpc_freq();
    let mut last = 0i64;
    loop {
        {
            let mut c = lock(&CLOCK);
            while !c.stop && c.want.is_empty() {
                c = WAKE.wait(c).unwrap_or_else(|e| e.into_inner());
            }
            if c.stop {
                return;
            }
        }
        let t_before = ui::qpc();
        if !ui::dwm_flush() {
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        let mut now = ui::qpc();
        // DwmFlush peut rendre tout de suite (rien a composer) : on ne
        // cadence jamais plus vite que 4 ms.
        if (now - t_before) * 1000 < freq * 2 && last != 0 && (now - last) * 1000 < freq * 4 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            now = ui::qpc();
        }
        let mut c = lock(&CLOCK);
        if c.stop {
            return;
        }
        if last != 0 {
            let d = now - last;
            let p = c.info.period_qpc;
            // Une moyenne glissante, qui ignore les trous (une image sautee
            // par DWM lui-meme).
            if p == 0 {
                c.info.period_qpc = d;
            } else if d < p * 3 / 2 {
                c.info.period_qpc = (p * 15 + d) / 16;
            }
        }
        last = now;
        c.info.count += 1;
        c.info.vblank_qpc = now;
        c.info.qpc_freq = freq;
        let count = c.info.count;
        let want = c.want.clone();
        for h in want {
            if c.pending.iter().any(|p| p.0 == h) {
                c.info.missed += 1;
                continue;
            }
            if win::post_message(h, WM_CP_FRAME, count as usize, 0) {
                c.pending.push((h, now));
            }
        }
    }
}

pub fn frames(h: Hwnd, on: bool) {
    {
        let mut c = lock(&CLOCK);
        c.want.retain(|&x| x != h);
        c.pending.retain(|p| p.0 != h);
        if on {
            c.want.push(h);
        }
        c.stop = false;
    }
    let mut t = lock(&THREAD);
    if on && t.is_none() {
        *t = std::thread::Builder::new()
            .name("cp_frame_clock".into())
            .spawn(clock_thread)
            .ok();
    }
    WAKE.notify_all();
}

/// L'image a ete traitee : la vue peut en recevoir une autre. Rend l'instant
/// de la composition qui l'a declenchee.
pub fn frame_done(h: Hwnd) -> i64 {
    let mut c = lock(&CLOCK);
    let posted = c
        .pending
        .iter()
        .find(|p| p.0 == h)
        .map_or(c.info.vblank_qpc, |p| p.1);
    c.pending.retain(|p| p.0 != h);
    posted
}

pub fn frame_info() -> FrameInfo {
    lock(&CLOCK).info
}

pub fn forget(h: Hwnd) {
    let mut c = lock(&CLOCK);
    c.want.retain(|&x| x != h);
    c.pending.retain(|p| p.0 != h);
}

pub fn stop_clock() {
    lock(&CLOCK).stop = true;
    WAKE.notify_all();
    if let Some(t) = lock(&THREAD).take() {
        let _ = t.join();
    }
}

// ---------------------------------------------------------------------------
// Le clavier
// ---------------------------------------------------------------------------

/// La vue du dernier clic (0 = le dernier clic etait ailleurs).
static CLAIM: Mutex<isize> = Mutex::new(0);
/// Les vues, pour savoir a qui appartient une fenetre.
static VIEWS: OnceLock<fn() -> Vec<Hwnd>> = OnceLock::new();
/// Appel du module : (vue, touche) -> code du crochet.
static ASK: OnceLock<fn(Hwnd, &KeyEvent) -> i32> = OnceLock::new();

pub fn init_keys(views: fn() -> Vec<Hwnd>, ask: fn(Hwnd, &KeyEvent) -> i32) {
    let _ = VIEWS.set(views);
    let _ = ASK.set(ask);
}

/// La vue qui contient `h` (elle-meme ou un de ses enfants).
fn owner_view(h: Hwnd) -> Option<Hwnd> {
    if h.0 == 0 {
        return None;
    }
    let views = VIEWS.get()?();
    views.into_iter().find(|&v| v == h || ui::is_child(v, h))
}

/// Pose la revendication du clavier (l'auto-test, qui ne clique pas).
pub fn set_claim(h: Hwnd) {
    *lock(&CLAIM) = h.0;
}

/// Crochet souris : le dernier clic decide a qui va le clavier.
pub fn on_mouse(h: Hwnd, msg: u32) {
    const DOWNS: [u32; 6] = [0x201, 0x204, 0x207, 0xA1, 0xA4, 0xA7];
    if DOWNS.contains(&msg) {
        *lock(&CLAIM) = owner_view(h).map_or(0, |v| v.0);
    }
}

fn is_text_field(h: Hwnd) -> bool {
    let c = ui::class_name(h).to_ascii_lowercase();
    c == "edit" || c.starts_with("richedit") || c == "combobox"
}

/// Le crochet « accelerator ».
pub fn on_key(m: &mut Msg) -> i32 {
    const KEYS: [u32; 6] = [0x100, 0x101, 0x102, 0x104, 0x105, 0x106];
    if !KEYS.contains(&m.message) {
        return accel::NOT_OURS;
    }
    let (target, redirected) = match owner_view(Hwnd(m.hwnd)) {
        Some(v) => (v, false),
        None => {
            let claim = Hwnd(*lock(&CLAIM));
            let ok = claim.0 != 0
                && win::is_window(claim)
                && ui::is_shown(claim)
                && !is_text_field(Hwnd(m.hwnd));
            if !ok {
                return accel::NOT_OURS;
            }
            (claim, true)
        }
    };
    let Some(ask) = ASK.get() else {
        return accel::NOT_OURS;
    };
    let ev = KeyEvent {
        message: m.message,
        wp: m.wp,
        lp: m.lp,
        redirected: redirected as u32,
    };
    let r = ask(target, &ev);
    if r == accel::PASS_TO_WINDOW && redirected {
        // Readressee : Windows la traduira (WM_CHAR) et la livrera a la vue.
        m.hwnd = target.0;
    }
    r
}

/// Le message que le module recoit pour une touche du crochet.
pub const ACCEL_MSG: u32 = WM_CP_ACCEL;
