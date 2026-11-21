//! `cp_module.dll` : le code de CP Studio, recharge a chaud par le chargeur.
//!
//! Pour l'instant, une seule chose : la fenetre de test du rechargement. Elle
//! montre la generation du module, l'age de la construction, et un compteur de
//! clics qui SURVIT au rechargement (range par `shutdown`, rendu a `init`).

use std::time::{SystemTime, UNIX_EPOCH};

use cp_sys::module::{HostRef, Module};
use cp_sys::win::{self, Hwnd, Paint, WinMsg, rgb};

/// Changer cette couleur et reconstruire : la bande a gauche de la fenetre
/// change sans fermer REAPER.
const ACCENT: u32 = rgb(80, 160, 255);

const BUILT: u64 = {
    let s = env!("CP_BUILD_UNIX").as_bytes();
    let mut i = 0;
    let mut v = 0u64;
    while i < s.len() {
        v = v * 10 + (s[i] - b'0') as u64;
        i += 1;
    }
    v
};

const STATE_VERSION: u8 = 1;

struct App {
    host: HostRef,
    clicks: u32,
    window: Option<Hwnd>,
    open_cmd: i32,
}

impl App {
    fn encode(&self) -> Vec<u8> {
        let mut v = vec![STATE_VERSION];
        v.extend_from_slice(&self.clicks.to_le_bytes());
        v.extend_from_slice(&(self.window.map_or(0, |h| h.0) as i64).to_le_bytes());
        v
    }

    fn decode(s: &[u8]) -> (u32, Option<Hwnd>) {
        if s.len() != 13 || s[0] != STATE_VERSION {
            return (0, None);
        }
        let clicks = u32::from_le_bytes(s[1..5].try_into().unwrap_or_default());
        let h = i64::from_le_bytes(s[5..13].try_into().unwrap_or_default()) as isize;
        (clicks, (h != 0).then_some(Hwnd(h)))
    }

    fn paint(&self, h: Hwnd) {
        let Some(p) = Paint::begin(h) else { return };
        let (w, hh) = p.size();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(BUILT);
        let ago = now.saturating_sub(BUILT);
        p.fill(0, 0, w, hh, rgb(30, 30, 34));
        p.fill(0, 0, 6, hh, ACCENT);
        p.text(24, 20, "CP Studio - hot reload", rgb(235, 235, 240));
        let lines = [
            format!("Module generation: {}", self.host.generation()),
            format!("Built {ago} s ago"),
            format!(
                "Clicks: {} (click anywhere; the count survives a reload)",
                self.clicks
            ),
        ];
        for (i, l) in lines.iter().enumerate() {
            p.text(24, 56 + 24 * i as i32, l, rgb(175, 175, 185));
        }
    }
}

impl Module for App {
    fn init(host: HostRef, state: &[u8]) -> Self {
        let (clicks, window) = App::decode(state);
        let window = window.filter(|h| win::is_window(*h));
        let open_cmd = host.register_action(
            "_CPSTUDIO_TEST_WINDOW",
            "CP Studio: Open hot-reload test window",
        );
        host.info(&format!(
            "module pret : generation {}, {clicks} clics, fenetre {}",
            host.generation(),
            if window.is_some() {
                "reprise"
            } else {
                "aucune"
            }
        ));
        App {
            host,
            clicks,
            window,
            open_cmd,
        }
    }

    fn shutdown(&mut self) {
        self.host.store_state(&self.encode());
    }

    fn on_action(&mut self, cmd: i32) -> bool {
        if cmd != self.open_cmd {
            return false;
        }
        match self.window.filter(|h| win::is_window(*h)) {
            Some(h) => win::show(h),
            None => {
                self.window = self
                    .host
                    .create_window(1, "CP Studio - hot reload", 560, 220)
            }
        }
        true
    }

    fn wnd_proc(&mut self, m: &WinMsg) -> Option<isize> {
        match m.msg {
            win::WM_PAINT => {
                self.paint(m.hwnd);
                Some(0)
            }
            win::WM_ERASEBKGND => Some(1),
            win::WM_LBUTTONDOWN => {
                self.clicks += 1;
                win::invalidate(m.hwnd);
                Some(0)
            }
            win::WM_SIZE => {
                win::invalidate(m.hwnd);
                None
            }
            _ => None,
        }
    }
}

cp_sys::export_module!(App);
