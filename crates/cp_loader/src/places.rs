//! Les places natives : une de nos vues A LA PLACE d'une vue de REAPER.
//!
//! LA METHODE (mesuree dans l'ancienne suite, le 2026-08-03 et le 2026-10-04,
//! puis revue dans le REAPER de Cedric le 2026-11-21) : on ne touche JAMAIS a
//! la geometrie de REAPER. On masque sa vue (ca tient), on LIT le rectangle
//! qu'il continue de calculer pour elle, et on y pose la notre. La vue
//! masquee est notre oracle de disposition : redimensionner la fenetre, tirer
//! un separateur, ouvrir un docker, tout se lit dans son rectangle.
//!
//! - L'ARRANGEMENT : la regle, les pistes, le panneau des pistes et ce qui vit
//!   dans leur bande (la barre d'outils principale, l'etiquette d'effets de
//!   la piste). Jamais les dockers, jamais le transport, jamais nos fenetres.
//!   REAPER les remontre pendant le glissement d'un separateur (62 fois en
//!   20 s, mesure) : le gardien les recache a chaque battement.
//! - LE TRANSPORT, ancre en haut de la fenetre principale. Masque, il ne se
//!   peint plus du tout (0 peinture/s, contre 30 recouvert) ; sa place et son
//!   rectangle sont gardes. REAPER ne le remontre jamais de lui-meme : s'il
//!   reapparait, c'est Cedric qui l'a rallume, et on lui CEDE la place (le
//!   recacher, c'est ce qu'il voyait « s'afficher une frame et se fermer »).
//!
//! Tout se rend : quand le module rend la place, quand il tombe (faute),
//! quand notre vue meurt, et quand REAPER s'arrete. C'est le chargeur qui le
//! fait, parce que lui ne tombe pas : un module en faute ne laisse jamais
//! REAPER sans arrangement ni transport.

use std::sync::Mutex;

use cp_abi::{
    PLACE_ARRANGE, PLACE_FREE, PLACE_HELD, PLACE_MISSING, PLACE_TRANSPORT, PLACE_YIELDED,
};
use cp_sys::reaper::Reaper;
use cp_sys::ui;
use cp_sys::win::{self, Hwnd, Rect};

use cp_abi::{LOG_INFO, LOG_WARN};

use crate::{lock, log};

/// Les classes de REAPER qui forment la bande de l'arrangement.
const ARRANGE_CLASSES: [&str; 5] = [
    "REAPERTrackListWindow",
    "REAPERTCPDisplay",
    "REAPERTimeDisplay",
    "REAPERRulerHeader",
    "Static",
];
const VIRT_HOST: &str = "REAPERVirtWndDlgHost";
const TRANSPORT_TITLE: &str = "Transport";
const OUR_CLASS: &str = "CPStudioWindow";

#[derive(Clone, Default)]
struct Place {
    /// Notre vue (0 : aucune).
    view: Hwnd,
    /// Posee et visible, la native masquee.
    shown: bool,
    /// Ce qu'on a masque (a remontrer en rendant).
    natives: Vec<Hwnd>,
    rect: Rect,
    /// Rendue d'elle-meme (transport rallume, place perdue).
    yielded: bool,
    /// Fois ou REAPER a remontre une vue masquee (la bande pendant un
    /// glissement de separateur).
    rehides: u64,
}

static PLACES: Mutex<[Place; 2]> = Mutex::new([
    Place {
        view: Hwnd(0),
        shown: false,
        natives: Vec::new(),
        rect: Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        yielded: false,
        rehides: 0,
    },
    Place {
        view: Hwnd(0),
        shown: false,
        natives: Vec::new(),
        rect: Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        yielded: false,
        rehides: 0,
    },
]);

fn index(place: u32) -> Option<usize> {
    match place {
        PLACE_ARRANGE => Some(0),
        PLACE_TRANSPORT => Some(1),
        _ => None,
    }
}

fn name(i: usize) -> &'static str {
    if i == 0 {
        "l'arrangement"
    } else {
        "le transport"
    }
}

/// Les vues natives de la place, filles directes de la fenetre principale.
fn find_natives(i: usize, main: Hwnd) -> Vec<Hwnd> {
    let kids = ui::children(main);
    if i == 0 {
        let band: Vec<Hwnd> = kids
            .into_iter()
            .filter(|&h| {
                let c = ui::class_name(h);
                if c == OUR_CLASS {
                    return false;
                }
                ARRANGE_CLASSES.contains(&c.as_str())
                    || (c == VIRT_HOST && ui::window_text(h) != TRANSPORT_TITLE)
            })
            .collect();
        // Sans la vue des pistes, ce n'est pas l'arrangement.
        if band
            .iter()
            .any(|&h| ui::class_name(h) == "REAPERTrackListWindow")
        {
            band
        } else {
            Vec::new()
        }
    } else {
        kids.into_iter()
            .filter(|&h| ui::class_name(h) == VIRT_HOST && ui::window_text(h) == TRANSPORT_TITLE)
            .take(1)
            .collect()
    }
}

/// Le rectangle de la place : l'union des rectangles que REAPER calcule pour
/// ses vues, masquees ou non.
fn oracle(natives: &[Hwnd], main: Hwnd) -> Option<Rect> {
    let mut u: Option<Rect> = None;
    for &h in natives {
        let r = ui::rect_in(h, main);
        if r.right < r.left || r.bottom < r.top {
            continue;
        }
        u = Some(match u {
            None => r,
            Some(a) => Rect {
                left: a.left.min(r.left),
                top: a.top.min(r.top),
                right: a.right.max(r.right),
                bottom: a.bottom.max(r.bottom),
            },
        });
    }
    u.filter(|r| r.right - r.left > 0 && r.bottom - r.top > 0)
}

fn put(view: Hwnd, r: Rect, show: Option<bool>) {
    ui::place_window(
        view,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        show,
    );
}

/// Prend la place (voir `HostApi::place_take`). `view` est une de nos vues.
pub fn take(r: &Reaper, place: u32, view: Hwnd, show: bool) -> bool {
    let Some(i) = index(place) else { return false };
    if view.0 == 0 {
        release(i, false);
        return true;
    }
    let main = r.main_hwnd();
    let natives = find_natives(i, main);
    let Some(rect) = oracle(&natives, main) else {
        log(
            LOG_WARN,
            &format!("place : {} introuvable, la vue n'est pas posee", name(i)),
        );
        return false;
    };
    if i == 1 && natives.iter().any(|&h| !ui::is_visible(h)) && !lock(&PLACES)[1].shown {
        // Le transport natif est ferme : il n'y a pas de place a prendre
        // (REAPER ne garde pas le rectangle d'un transport ferme).
        log(
            LOG_WARN,
            "place : le transport de REAPER est ferme, rien a remplacer",
        );
        return false;
    }
    // Une autre vue tenait la place : elle est garee (cachee), pas detruite.
    let previous = {
        let g = lock(&PLACES);
        (g[i].view != view && g[i].view.0 != 0).then_some(g[i].view)
    };
    if let Some(p) = previous {
        ui::show_window(p, false);
    }
    if !show {
        put(view, rect, Some(false));
        let mut g = lock(&PLACES);
        g[i].view = view;
        g[i].rect = rect;
        g[i].yielded = false;
        return true;
    }
    // La notre d'abord, au-dessus : la place n'est jamais vide a l'ecran.
    put(view, rect, Some(true));
    for &h in &natives {
        ui::show_window(h, false);
    }
    if i == 0 {
        ui::set_focus(view);
    }
    let mut g = lock(&PLACES);
    // Ce qui etait deja masque par nous reste a remontrer.
    let mut keep = std::mem::take(&mut g[i].natives);
    for h in natives {
        if !keep.contains(&h) {
            keep.push(h);
        }
    }
    g[i] = Place {
        view,
        shown: true,
        natives: keep,
        rect,
        yielded: false,
        rehides: g[i].rehides,
    };
    drop(g);
    log(
        LOG_INFO,
        &format!(
            "place : {} prise ({}x{} en {},{})",
            name(i),
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top
        ),
    );
    true
}

/// Rend la place a REAPER. `yielded` : d'elle-meme (pas a la demande).
fn release(i: usize, yielded: bool) {
    let p = std::mem::take(&mut lock(&PLACES)[i]);
    if p.view.0 != 0 {
        ui::show_window(p.view, false);
    }
    // Le transport rallume par Cedric est deja visible : rien a remontrer.
    if !(yielded && i == 1) {
        for &h in &p.natives {
            if win::is_window(h) {
                ui::show_window(h, true);
            }
        }
    }
    if i == 0 && p.shown {
        // Le clavier retourne a l'arrangement, comme avant.
        if let Some(tv) = p
            .natives
            .iter()
            .copied()
            .find(|&h| ui::class_name(h) == "REAPERTrackListWindow")
        {
            if ui::focus() == p.view || ui::focus().0 == 0 {
                ui::set_focus(tv);
            }
        }
    }
    let mut g = lock(&PLACES);
    g[i].yielded = yielded;
    // La vue reste connue (garee) si elle vit encore : le module peut la
    // reprendre. Une place rendue d'elle-meme l'oublie.
    if !yielded && win::is_window(p.view) {
        g[i].view = p.view;
    }
    g[i].rehides = p.rehides;
    drop(g);
    if p.shown {
        log(
            LOG_INFO,
            &format!(
                "place : {} rendue{}{}",
                name(i),
                if yielded { " d'elle-meme" } else { "" },
                if p.rehides > 0 {
                    format!(" ({} remises en place pendant qu'on la tenait)", p.rehides)
                } else {
                    String::new()
                }
            ),
        );
    }
}

/// L'etat d'une place et la vue qui l'occupe.
pub fn state(place: u32) -> (i32, isize) {
    let Some(i) = index(place) else {
        return (PLACE_MISSING, 0);
    };
    let g = lock(&PLACES);
    let p = &g[i];
    if p.shown {
        (PLACE_HELD, p.view.0)
    } else if p.yielded {
        (PLACE_YIELDED, 0)
    } else {
        (PLACE_FREE, p.view.0)
    }
}

/// Les mesures pour l'auto-test : (posee, rectangle de la vue native,
/// rectangle de notre vue, natives encore visibles, remises).
pub fn probe(r: &Reaper, place: u32) -> Option<(bool, Rect, Rect, usize, u64)> {
    let i = index(place)?;
    let main = r.main_hwnd();
    let natives = find_natives(i, main);
    let want = oracle(&natives, main)?;
    let g = lock(&PLACES);
    let p = &g[i];
    let ours = if p.view.0 != 0 {
        ui::rect_in(p.view, main)
    } else {
        Rect::default()
    };
    let visible = natives.iter().filter(|&&h| ui::is_visible(h)).count();
    Some((p.shown, want, ours, visible, p.rehides))
}

/// Le gardien, a chaque battement : la place suit son oracle, ce que REAPER
/// remontre est recache, et ce qui ne tient plus est rendu.
pub fn guard(r: &Reaper) {
    let main = r.main_hwnd();
    for i in 0..2 {
        let (view, shown) = {
            let g = lock(&PLACES);
            (g[i].view, g[i].shown)
        };
        if !shown {
            continue;
        }
        if !win::is_window(view) {
            release(i, false);
            continue;
        }
        let natives = find_natives(i, main);
        if i == 1 {
            // Le transport natif qui reapparait (ou qui a quitte sa place) :
            // c'est Cedric. On cede.
            if natives.is_empty() || natives.iter().any(|&h| ui::is_visible(h)) {
                log(
                    LOG_INFO,
                    "place : le transport de REAPER est revenu, CP Transport lui cede la place",
                );
                release(i, true);
                continue;
            }
        } else {
            let mut again = 0u64;
            for &h in &natives {
                if ui::is_visible(h) {
                    ui::show_window(h, false);
                    again += 1;
                }
            }
            if again > 0 {
                let mut g = lock(&PLACES);
                g[i].rehides += again;
                for h in &natives {
                    if !g[i].natives.contains(h) {
                        g[i].natives.push(*h);
                    }
                }
            }
        }
        let Some(rect) = oracle(&natives, main) else {
            log(LOG_WARN, &format!("place : {} a disparu", name(i)));
            release(i, true);
            continue;
        };
        let moved = {
            let mut g = lock(&PLACES);
            let m = g[i].rect != rect;
            g[i].rect = rect;
            m
        };
        if moved {
            put(view, rect, Some(true));
        } else if !ui::is_visible(view) {
            ui::show_window(view, true);
        }
    }
}

/// Notre vue meurt : sa place revient a REAPER.
pub fn forget(h: Hwnd) {
    for i in 0..2 {
        let held = lock(&PLACES)[i].view == h;
        if held {
            release(i, false);
            lock(&PLACES)[i] = Place::default();
        }
    }
}

/// Tout rendre (arret de REAPER, module en faute).
pub fn release_all() {
    for i in 0..2 {
        let shown = lock(&PLACES)[i].shown;
        if shown {
            release(i, false);
        }
    }
}

/// La vue est-elle posee a une place (fille de la fenetre principale) ?
pub fn is_placed(h: Hwnd) -> bool {
    lock(&PLACES).iter().any(|p| p.shown && p.view == h)
}
