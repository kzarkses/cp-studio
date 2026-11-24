//! Le mode sombre de CP Studio : les barres de titre et les menus de REAPER
//! en sombre, rien d'autre (voir `cp_sys::dark` : la part sure). Eteint par
//! defaut ; l'action « CP Studio: Dark title bars and menus » l'allume, et le
//! reglage se garde (ExtState CP_Studio/dark).
//!
//! Il vit dans le chargeur : son crochet (une boite qui s'ouvre recoit sa
//! barre sombre) doit vivre dans du code jamais decharge.

use std::sync::atomic::{AtomicBool, Ordering};

use cp_abi::LOG_INFO;
use cp_sys::dark;
use cp_sys::reaper::Reaper;
use cp_sys::win::Hwnd;

use crate::log;

static ON: AtomicBool = AtomicBool::new(false);

pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn on_activate(h: Hwnd) {
    if is_on() {
        dark::set_title_bar(h, true);
    }
    // La boite « Add FX to ... » de REAPER : le navigateur d'effets la
    // remplace (s'il le veut).
    crate::add_fx_window(h);
}

/// Allume ou eteint, tout de suite, sur toutes les fenetres de REAPER.
pub fn set(r: &Reaper, on: bool, persist: bool) {
    ON.store(on, Ordering::Relaxed);
    let menus = dark::set_app_mode(on);
    // Le crochet reste pose meme eteint : il sert aussi au navigateur
    // d'effets (la boite « Add FX to ... »).
    dark::install_activate_hook(on_activate);
    for h in dark::thread_windows() {
        dark::set_title_bar(h, on);
    }
    if persist {
        r.ext_set("CP_Studio", "dark", if on { "1" } else { "0" }, true);
    }
    log(
        LOG_INFO,
        &format!(
            "mode sombre {} (barres de titre ; menus : {})",
            if on { "allume" } else { "eteint" },
            if menus { "oui" } else { "non disponible" }
        ),
    );
}

/// Au demarrage : le reglage garde.
pub fn restore(r: &Reaper) {
    dark::install_activate_hook(on_activate);
    if r.ext_get("CP_Studio", "dark").as_deref() == Some("1") {
        set(r, true, false);
    }
}

/// A l'arret : le crochet part avec nous.
pub fn shutdown() {
    dark::remove_activate_hook();
}
