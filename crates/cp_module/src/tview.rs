//! La fenetre de CP Transport : posee a la place du transport de REAPER (la
//! place native que tient le chargeur), ses touches rendues a REAPER (sauf a
//! ses popovers, poses sur l'ecran : `crate::screenpop`), redessinee quand
//! l'etat de lecture change (a la cadence de l'ecran pendant la lecture).

use std::path::PathBuf;

use cp_abi::{PLACE_ARRANGE, PLACE_HELD, PLACE_TRANSPORT, PLACE_YIELDED, VIEW_PLACED};
use cp_gfx::text::TextCache;
use cp_gfx::{Canvas, Surface};
use cp_sys::font::GdiGlyphs;
use cp_sys::transport::{fmt, play};
use cp_sys::ui::{self, MenuEntry, MenuItem};
use cp_sys::win::{self, Hwnd, WinMsg};
use cp_theme::{Frames, ThemeFiles};
use cp_transport::{Act, Glyph, Item, RecMode, Skin, Status, ViewTab, fmt_bpm, initials};
use cp_ui::{Button, Ev, Theme};

use crate::items::{self, ItemCfg};
use crate::{App, Place, SECTION, hi16, lo16, mods, msg};

const TITLE: &str = "CP Transport";
const IDENT: &str = "cp_studio_transport";

/// Les actions de REAPER que pilote le transport (noms verifies par
/// l'auto-test dans le vrai REAPER).
pub(crate) mod cmd {
    pub const GO_START: i32 = 40042;
    pub const PLAY: i32 = 1007;
    pub const STOP: i32 = 1016;
    pub const PAUSE: i32 = 1008;
    pub const RECORD: i32 = 1013;
    pub const REPEAT: i32 = 1068;
    pub const METRONOME: i32 = 40364;
    pub const TAP: i32 = 1134;
    pub const PROJECT_SETTINGS: i32 = 40021;
    pub const SHOW_TRANSPORT: i32 = 40259;
    /// Les modes d'enregistrement (le menu du bouton Rec de REAPER).
    pub const REC_NORMAL: i32 = 40252;
    pub const REC_TIMESEL: i32 = 40076;
    pub const REC_ITEMS: i32 = 40253;
}

const M_RUN: u32 = 1;
const M_LEFT: u32 = 2;
const M_RIGHT: u32 = 3;
const M_REMOVE: u32 = 4;
const M_ADD: u32 = 5;
const M_CLOSE: u32 = 6;
const M_PLACE_ARRANGE: u32 = 10;
const M_PLACE_DOCK: u32 = 11;
const M_PLACE_FLOAT: u32 = 12;
const M_VIEW_CLOSE: u32 = 13;
const M_GTAB_SETTINGS: u32 = 14;
const M_TUNER_OFF: u32 = 20;
const M_TUNER_CLOCK: u32 = 21;
const M_TUNER_SLOT: u32 = 22;
const M_TUNER_A4: u32 = 23;
/// La clock externe : aucune, ou l'entree MIDI (100 + son rang).
const M_CLOCK_OFF: u32 = 99;
const M_CLOCK_DEV: u32 = 100;
/// Le retard du maitre (sa propre latence) : 80 + son rang dans `DELAYS`.
const M_CLOCK_DELAY: u32 = 80;
const DELAYS: [f64; 7] = [-20.0, -10.0, -5.0, 0.0, 5.0, 10.0, 20.0];
const M_REC_NORMAL: u32 = 30;
const M_REC_TIMESEL: u32 = 31;
const M_REC_ITEMS: u32 = 32;

/// La fenetre du transport.
pub(crate) struct TView {
    pub hwnd: Hwnd,
    surf: Surface,
    present: Vec<u32>,
    theme: Theme,
    tracking: bool,
    pub frames_on: bool,
}

/// Le transport et ses icones.
pub(crate) struct TPart {
    pub t: cp_transport::Transport,
    pub view: Option<TView>,
    cfg: Vec<ItemCfg>,
    /// Pour chaque icone montree : l'index dans `cfg` et la commande.
    shown: Vec<(usize, i32)>,
    /// Une session de la liste des actions est ouverte (« Add action »).
    pub prompt: bool,
    scripts_dir: PathBuf,
    data_file: Option<PathBuf>,
    kb: std::collections::HashMap<String, String>,
    /// Les icones deja lues, par nom et echelle.
    images: std::collections::HashMap<String, Option<Frames>>,
    /// Le theme de REAPER (ses images), et `Data/toolbar_icons`.
    theme: Option<ThemeFiles>,
    tools: ThemeFiles,
    /// L'habillage du transport, et ce dont il depend (theme, echelle,
    /// couleurs) : refait quand l'un change.
    pub skin: Option<Skin>,
    skin_key: Option<SkinKey>,
    skin_id: u32,
    since_theme: u32,
    /// Battements depuis la derniere relecture des icones.
    since_items: u32,
    /// L'infobulle, et la cible survolee depuis quand.
    tip: Option<ui::Tooltip>,
    hover: (cp_transport::Hit, std::time::Instant),
    /// L'accordeur.
    pub tu: crate::tuner::Tuner,
    /// La clock MIDI externe et ce qu'elle a fait au transport.
    pub clock: crate::clocksync::ClockSync,
    /// Les menus et les saisies du transport : un popover pose sur l'ecran
    /// (la barre est trop basse pour en tenir un).
    pub sp: crate::screenpop::ScreenPop<Act>,
    /// Son theme (celui de REAPER, comme les popovers des vues), pris a
    /// l'ouverture.
    pop_theme: Option<Theme>,
    /// La souris, dans la barre (une saisie s'ouvre la ou on a clique).
    mouse: (f32, f32),
}

impl TPart {
    pub fn new(r: &cp_sys::reaper::Reaper) -> TPart {
        let res = r.resource_path().unwrap_or_default();
        let kb = std::fs::read(res.join("reaper-kb.ini"))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        TPart {
            t: cp_transport::Transport::new(),
            view: None,
            cfg: Vec::new(),
            shown: Vec::new(),
            prompt: false,
            scripts_dir: res.join("Scripts"),
            data_file: Some(res.join("CP_Studio").join("transport_items.txt")),
            kb: items::kb_scripts(&kb),
            images: Default::default(),
            theme: None,
            tools: ThemeFiles::dir(&res.join("Data").join("toolbar_icons")),
            skin: None,
            skin_key: None,
            skin_id: 0,
            since_theme: 0,
            since_items: 0,
            tip: None,
            hover: (cp_transport::Hit::None, std::time::Instant::now()),
            tu: crate::tuner::Tuner::new(r),
            clock: crate::clocksync::ClockSync::load(*r),
            sp: Default::default(),
            pop_theme: None,
            mouse: (0.0, 0.0),
        }
    }
}

/// Les couleurs du theme que l'habillage lit (et qu'on surveille).
const SKIN_COLORS: [&str; 5] = [
    "col_main_bg2",
    "col_toolbar_text",
    "col_main_text2",
    "col_toolbar_text_on",
    "col_transport_editbk",
];

#[derive(Clone, Debug, PartialEq)]
struct SkinKey {
    path: Option<PathBuf>,
    scale: i32,
    colors: Vec<Option<(u8, u8, u8)>>,
}

impl App {
    // -----------------------------------------------------------------------
    // L'habillage : le theme de REAPER
    // -----------------------------------------------------------------------

    /// Ouvre le theme de REAPER s'il a change (partage par CP Transport et
    /// le navigateur).
    pub(crate) fn theme_files(&mut self, path: Option<&std::path::Path>) -> Option<&ThemeFiles> {
        let cur = self.tp.theme.as_ref().map(|t| t.path.as_path());
        if cur != path {
            self.tp.theme = path.and_then(ThemeFiles::open);
        }
        self.tp.theme.as_ref()
    }

    /// Relit le theme si REAPER en a change (fichier, couleurs) ou si
    /// l'echelle de l'ecran a change ; rend vrai s'il a ete refait.
    pub(crate) fn transport_skin(&mut self) -> bool {
        let scale = self.tp.view.as_ref().map_or(1.0, |v| v.theme.scale);
        let r = self.r;
        let key = SkinKey {
            path: r.theme_file(),
            scale: (scale * 100.0).round() as i32,
            colors: SKIN_COLORS.iter().map(|k| r.theme_color(k)).collect(),
        };
        if self.tp.skin_key.as_ref() == Some(&key) {
            return false;
        }
        self.theme_files(key.path.as_deref());
        self.tp.skin_id += 1;
        self.tp.images.clear();
        let face = self
            .tp
            .theme
            .as_ref()
            .and_then(|t| t.font_face("trans_font"))
            .unwrap_or_else(|| "Segoe UI".into());
        if face != self.face {
            // La police du transport du theme : famille 2 du texte.
            self.face = face;
            self.text = TextCache::new(Box::new(GdiGlyphs::new(&[
                "Segoe UI",
                "Consolas",
                &self.face,
                "Segoe UI Symbol",
            ])));
        }
        let id = self.tp.skin_id;
        self.tp.skin = self.tp.theme.as_ref().map(|t| {
            Skin::build(
                t,
                scale,
                &|k| r.theme_color(k).or_else(|| t.color(k)),
                2,
                id,
            )
        });
        let name = key
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "(aucun)".into(), |s| s.to_string_lossy().into_owned());
        let line = match &self.tp.skin {
            Some(sk) => format!(
                "CP Transport : habille du theme \u{ab} {name} \u{bb} a {} % (boutons {}, metronome {}, police {})",
                key.scale,
                if sk.play_off.is_some() {
                    "du theme"
                } else {
                    "vectoriels"
                },
                if sk.metro_off.is_some() {
                    "du theme"
                } else {
                    "vectoriel"
                },
                self.face
            ),
            None => {
                format!("CP Transport : theme \u{ab} {name} \u{bb} illisible, icones vectorielles")
            }
        };
        self.host.info(&line);
        self.tp.skin_key = Some(key);
        self.refresh_items();
        self.tp.t.mark();
        true
    }

    // -----------------------------------------------------------------------
    // Ouvrir, fermer
    // -----------------------------------------------------------------------

    /// Pose CP Transport a la place de celui de REAPER.
    pub(crate) fn transport_start(&mut self) {
        if self.tp.view.is_some() {
            return;
        }
        // REAPER peut avoir retenu « transport ferme » en quittant pendant que
        // le notre le masquait : on le rouvre pour en prendre la place.
        if self.r.toggle_state(cmd::SHOW_TRANSPORT) == 0 {
            self.r.main_on_command(cmd::SHOW_TRANSPORT);
        }
        let Some(hwnd) = self
            .host
            .create_view(TITLE, IDENT, 0, 0, 800, 36, VIEW_PLACED)
        else {
            self.host
                .error("CP Transport : la fenetre n'a pas pu etre creee");
            return;
        };
        if !self.host.place_take(PLACE_TRANSPORT, Some(hwnd), false) {
            self.host.destroy_window(hwnd);
            self.r.show_console_msg(
                "CP Transport: REAPER's transport must be docked at the top of the main window \
                 (right-click REAPER's transport > Dock transport > top of main window).\n",
            );
            self.r.ext_set(SECTION, "transport_on", "0", true);
            return;
        }
        self.transport_adopt(hwnd);
        self.host.place_take(PLACE_TRANSPORT, Some(hwnd), true);
        self.transport_render(true);
        self.r.ext_set(SECTION, "transport_on", "1", true);
    }

    pub(crate) fn transport_adopt(&mut self, hwnd: Hwnd) {
        // Pas « sans clavier » : un clic y revendique les touches, et le
        // transport les rend toutes a REAPER (0) sauf quand un de ses popovers
        // est ouvert. Ses raccourcis restent donc a REAPER (Espace apres un
        // clic sur Loop), et une saisie du tempo recoit ses chiffres. Ici, et
        // pas a la creation seule : un module recharge reprend la fenetre.
        self.host.set_keyless(hwnd, false);
        let scale = ui::dpi(hwnd) as f32 / 96.0;
        self.tp.view = Some(TView {
            hwnd,
            surf: Surface::default(),
            present: Vec::new(),
            theme: Theme::dark(scale),
            tracking: false,
            frames_on: false,
        });
        self.load_items();
        self.tp.skin_key = None;
        self.transport_skin();
        self.refresh_items();
        let st = self.read_status();
        self.tp.t.set_status(st);
        self.tp.t.views = self.view_tabs();
        self.tp.t.mark();
        self.transport_render(true);
    }

    /// `user` : Cedric l'a ferme (il ne reviendra pas au demarrage).
    pub(crate) fn transport_stop(&mut self, user: bool) {
        if let Some(v) = self.tp.view.take() {
            self.host.place_take(PLACE_TRANSPORT, None, false);
            self.host.destroy_window(v.hwnd);
        }
        if user {
            self.r.ext_set(SECTION, "transport_on", "0", true);
        }
    }

    // -----------------------------------------------------------------------
    // L'etat
    // -----------------------------------------------------------------------

    fn read_status(&self) -> Status {
        let r = &self.r;
        let ps = r.play_state();
        let pos = if ps & play::PLAYING != 0 || ps & play::RECORDING != 0 {
            r.play_position()
        } else {
            r.cursor_position()
        };
        let (n, d, bpm) = r.time_sig_at(pos);
        Status {
            play: ps,
            beats: r.format_time(pos, fmt::BEATS),
            time: r.format_time(pos, fmt::TIME),
            bpm,
            sig: (n, d),
            rate: r.play_rate(),
            repeat: r.repeat(),
            metronome: r.toggle_state(cmd::METRONOME) == 1,
            rec_mode: if r.toggle_state(cmd::REC_TIMESEL) == 1 {
                RecMode::TimeSel
            } else if r.toggle_state(cmd::REC_ITEMS) == 1 {
                RecMode::Items
            } else {
                RecMode::Normal
            },
        }
    }

    /// Les vues du selecteur : Arrange, puis chaque application.
    pub(crate) fn view_tabs(&self) -> Vec<ViewTab> {
        let (st, holder) = self.host.place_state(PLACE_ARRANGE);
        let bh = self.view.as_ref().map(|v| v.hwnd);
        let browser_in_band = st == PLACE_HELD && holder.is_some() && holder == bh;
        let browser_open_elsewhere = self
            .view
            .as_ref()
            .is_some_and(|v| v.place != Place::Arrange);
        vec![
            ViewTab {
                name: "Arrange".into(),
                active: st != PLACE_HELD,
                open: false,
            },
            ViewTab {
                name: "Browser".into(),
                active: browser_in_band,
                open: browser_open_elsewhere,
            },
            ViewTab {
                name: "Guitar Tab".into(),
                active: self.gtab_in_band(),
                open: self
                    .gt
                    .win
                    .as_ref()
                    .is_some_and(|w| w.place != Place::Arrange),
            },
            ViewTab {
                name: "Editor".into(),
                active: self.ed_in_band(),
                open: self
                    .ed
                    .win
                    .as_ref()
                    .is_some_and(|w| w.place != Place::Arrange),
            },
            ViewTab {
                name: "Session".into(),
                active: self.ses_in_band(),
                open: self
                    .ses
                    .win
                    .as_ref()
                    .is_some_and(|w| w.place != Place::Arrange),
            },
            ViewTab {
                name: "FX".into(),
                active: self.fxb_in_band(),
                open: self
                    .fxb
                    .win
                    .as_ref()
                    .is_some_and(|w| w.place != Place::Arrange),
            },
            ViewTab {
                name: "Constellation".into(),
                active: self.fxc_in_band(),
                open: self
                    .fxc
                    .win
                    .as_ref()
                    .is_some_and(|w| w.place != Place::Arrange),
            },
        ]
    }

    fn load_items(&mut self) {
        let mut cfg = self
            .tp
            .data_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| items::parse(&s))
            .unwrap_or_default();
        let mut seeded: Vec<String> = self
            .r
            .ext_get(SECTION, "transport_seeded")
            .unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let migrated = items::migrate(&mut cfg);
        if items::seed_tools(&mut cfg, &mut seeded) || migrated {
            self.tp.cfg = cfg;
            self.save_items();
        } else {
            self.tp.cfg = cfg;
        }
        self.r
            .ext_set(SECTION, "transport_seeded", &seeded.join("|"), true);
    }

    fn save_items(&self) {
        if let Some(p) = &self.tp.data_file {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, items::serialize(&self.tp.cfg));
        }
    }

    /// La commande d'une icone, si l'action existe (et, pour un script, si son
    /// fichier existe encore).
    fn resolve(&self, id: &str) -> i32 {
        if let Ok(n) = id.parse::<i32>() {
            return if self.r.command_text(n).is_empty() {
                0
            } else {
                n
            };
        }
        let c = self.r.named_command(id);
        if c == 0 {
            return 0;
        }
        if let Some(path) = self.tp.kb.get(id.trim_start_matches('_')) {
            let p = std::path::Path::new(path);
            let full = if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.tp.scripts_dir.join(p)
            };
            if !full.exists() {
                return 0;
            }
        }
        c
    }

    /// Une icone de barre d'outils, comme REAPER la trouve : dans le theme
    /// courant d'abord, puis dans `Data/toolbar_icons` ; a l'echelle de
    /// l'ecran (`150/`, `200/`), ses etats separes, jamais reduite a la main.
    fn image(&mut self, file: &str, scale: f32) -> Option<Frames> {
        let key = format!("{file}@{scale}");
        if let Some(v) = self.tp.images.get(&key) {
            return v.clone();
        }
        let found = self
            .tp
            .theme
            .as_ref()
            .and_then(|t| t.image(file, scale))
            .or_else(|| self.tp.tools.image(file, scale))
            .map(|(img, s)| Frames::toolbar(&img).rescaled(s, scale));
        self.tp.images.insert(key, found.clone());
        found
    }

    /// Ce que montre une icone : `png:`, `lucide:`, ou des initiales.
    fn glyph(&mut self, icon: &str, name: &str, scale: f32) -> Option<Glyph> {
        if let Some(f) = icon.strip_prefix("png:") {
            self.image(f, scale).map(Glyph::Frames)
        } else if let Some(n) = icon.strip_prefix("lucide:") {
            items::lucide(n).map(Glyph::Icon)
        } else if icon.is_empty() {
            Some(Glyph::Text(initials(name)))
        } else {
            None
        }
    }

    /// Les icones montrees : celles dont l'action existe, avec leur etat.
    pub(crate) fn refresh_items(&mut self) {
        let scale = self.tp.view.as_ref().map_or(1.0, |v| v.theme.scale);
        let mut shown = Vec::new();
        let mut out = Vec::new();
        let cfg = self.tp.cfg.clone();
        for (i, c) in cfg.iter().enumerate() {
            let cmd = self.resolve(&c.id);
            if cmd == 0 {
                continue;
            }
            let name = self.r.command_text(cmd);
            // L'icone choisie, sinon celle de secours d'un outil, sinon des
            // initiales.
            let glyph = self
                .glyph(&c.icon, &name, scale)
                .or_else(|| items::fallback(&c.id).and_then(|f| self.glyph(f, &name, scale)))
                .unwrap_or_else(|| Glyph::Text(initials(&name)));
            // Nos propres actions se lisent ici : demander leur etat a REAPER
            // repasserait par le module, occupe a ce battement (il rendrait
            // « pas d'etat »).
            let on = if c.id.starts_with("_CPSTUDIO_") {
                cp_sys::module::Module::toggle_state(self, cmd) == Some(true)
            } else {
                self.r.toggle_state(cmd) == 1
            };
            out.push(Item {
                id: c.id.clone(),
                name,
                glyph,
                on,
                open: false,
            });
            shown.push((i, cmd));
        }
        let changed = out.len() != self.tp.t.items.len()
            || out
                .iter()
                .zip(&self.tp.t.items)
                .any(|(a, b)| a.id != b.id || a.on != b.on);
        self.tp.shown = shown;
        self.tp.t.items = out;
        if changed {
            self.tp.t.mark();
        }
    }

    /// Le battement : l'etat de lecture, les icones, la liste des actions,
    /// et la place (Cedric a-t-il rallume le transport de REAPER ?).
    /// La clock externe : l'entree ecoutee (-1 : aucune), retenue.
    fn clock_set_device(&mut self, dev: i32) {
        // La synchro qui s'arrete (ou change d'entree) rend la vitesse de
        // lecture qu'elle avait prise.
        self.tp
            .clock
            .release_rate(&crate::clocksync::Reaper(self.r));
        let delay = self.tp.clock.delay_ms;
        self.tp.clock = crate::clocksync::ClockSync::on(dev);
        self.tp.clock.delay_ms = delay;
        self.host.clock_device(dev);
        self.r
            .ext_set(SECTION, "ext_clock_dev", &dev.to_string(), true);
        self.host
            .info(&format!("clock externe : entree MIDI {dev}"));
    }

    pub(crate) fn transport_tick(&mut self) {
        // La clock externe suit le maitre, meme CP Transport ferme.
        if self.tp.clock.dev >= 0 {
            let v = self.host.clock_read();
            let r = self.r;
            self.tp.clock.step(&crate::clocksync::Reaper(r), v);
        }
        let Some(hwnd) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return;
        };
        let (st, _) = self.host.place_state(PLACE_TRANSPORT);
        if st == PLACE_YIELDED || !win::is_window(hwnd) {
            self.host
                .info("CP Transport : le transport de REAPER est revenu, on lui laisse la place");
            if let Some(v) = self.tp.view.take() {
                self.host.destroy_window(v.hwnd);
            }
            self.r.ext_set(SECTION, "transport_on", "0", true);
            return;
        }
        if self.tp.prompt {
            let c = self.r.prompt_for_action(0);
            if c != 0 {
                self.tp.prompt = false;
                self.r.prompt_for_action(-1);
                if c > 0 {
                    let id = self.r.command_name(c).unwrap_or_else(|| c.to_string());
                    if !self.tp.cfg.iter().any(|x| x.id == id) {
                        self.tp.cfg.push(ItemCfg {
                            id,
                            icon: String::new(),
                        });
                        self.save_items();
                    }
                    self.refresh_items();
                }
            }
        }
        self.tp.since_theme += 1;
        if self.tp.since_theme >= 30 {
            self.tp.since_theme = 0;
            self.transport_skin();
        }
        self.tp.since_items += 1;
        if self.tp.since_items >= 10 {
            self.tp.since_items = 0;
            self.refresh_items();
        }
        self.transport_tip();
        // La souris partie ailleurs (une autre application) : le popover se
        // ferme.
        if self.tp.sp.is_open() {
            self.tp.sp.guard();
            if !self.tp.sp.is_open() {
                self.transport_pop_render();
            }
        }
        let tabs = self.view_tabs();
        if tabs != self.tp.t.views {
            self.tp.t.views = tabs;
            self.tp.t.mark();
        }
        let st = self.read_status();
        let playing = st.play & (play::PLAYING | play::RECORDING) != 0;
        self.tp.t.set_status(st);
        self.tuner_tick();
        if playing || self.tp.t.dragging() || self.tuner_ease() {
            self.transport_want_frames();
        } else if self.tp.t.take_dirty() {
            self.tp.t.mark();
            self.transport_render(false);
        }
    }

    /// L'infobulle : apres 0,6 s immobile sur une cible, sous elle.
    fn transport_tip(&mut self) {
        let Some(hwnd) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return;
        };
        let h = self.tp.t.hovered();
        if h != self.tp.hover.0 {
            self.tp.hover = (h, std::time::Instant::now());
            if let Some(t) = &mut self.tp.tip {
                t.hide();
            }
            return;
        }
        let shown = self.tp.tip.as_ref().is_some_and(|t| t.is_shown());
        if shown || self.tp.t.dragging() || self.tp.hover.1.elapsed().as_secs_f64() < 0.6 {
            return;
        }
        let (Some(text), Some((x, y, _, bh))) = (self.tp.t.label(h), self.tp.t.rect_of(h)) else {
            return;
        };
        if self.tp.tip.is_none() {
            self.tp.tip = ui::Tooltip::new(hwnd);
        }
        let (sx, sy) = ui::client_to_screen(hwnd, x as i32, (y + bh) as i32 + 6);
        if let Some(t) = &mut self.tp.tip {
            t.show(&text, sx, sy);
        }
    }

    fn transport_want_frames(&mut self) {
        if let Some(v) = &mut self.tp.view {
            if !v.frames_on {
                v.frames_on = true;
                self.host.frames(v.hwnd, true);
            }
        }
    }

    pub(crate) fn transport_on_frame(&mut self) {
        let st = self.read_status();
        let playing = st.play & (play::PLAYING | play::RECORDING) != 0;
        self.tp.t.set_status(st);
        let easing = self.tuner_ease();
        self.transport_render(false);
        if !playing && !self.tp.t.dragging() && !easing {
            if let Some(v) = &mut self.tp.view {
                if v.frames_on {
                    v.frames_on = false;
                    self.host.frames(v.hwnd, false);
                }
            }
        }
    }

    pub(crate) fn transport_render(&mut self, force: bool) {
        let Some(v) = self.tp.view.as_mut() else {
            return;
        };
        if !win::is_window(v.hwnd) {
            return;
        }
        let rc = ui::client_rect(v.hwnd);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return;
        }
        let scale = ui::dpi(v.hwnd) as f32 / 96.0;
        if (scale - v.theme.scale).abs() > 0.01 {
            v.theme = Theme::dark(scale);
            self.transport_skin();
            self.transport_render(true);
            return;
        }
        let resized = v.surf.resize(w, h);
        let dirty = self.tp.t.take_dirty();
        if !(dirty || force || resized) {
            return;
        }
        {
            let mut cv = Canvas::new(&mut v.surf, &mut self.scratch);
            self.tp.t.draw(
                &mut cv,
                &mut self.text,
                &mut self.icons,
                &v.theme,
                self.tp.skin.as_ref(),
                w as f32,
                h as f32,
            );
        }
        ui::present(v.hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
    }

    // -----------------------------------------------------------------------
    // Les gestes
    // -----------------------------------------------------------------------

    fn transport_act(&mut self, a: Act) {
        // Le geste que rejouera une question (menu, saisie).
        let top = self.tp.sp.ph.ask.begin(&a);
        self.transport_act_one(a);
        self.tp.sp.ph.ask.end(top);
        self.transport_pop_render();
    }

    fn transport_act_one(&mut self, a: Act) {
        let r = self.r;
        match a {
            Act::Start => r.main_on_command(cmd::GO_START),
            Act::Stop => r.main_on_command(cmd::STOP),
            Act::Play => r.main_on_command(cmd::PLAY),
            Act::Pause => r.main_on_command(cmd::PAUSE),
            Act::Record => r.main_on_command(cmd::RECORD),
            Act::Repeat => r.main_on_command(cmd::REPEAT),
            Act::Metronome => r.main_on_command(cmd::METRONOME),
            Act::Tap => r.main_on_command(cmd::TAP),
            Act::SetBpm { bpm, done } => r.set_bpm(bpm, done),
            Act::EditBpm => {
                let cur = fmt_bpm(self.tp.t.st.bpm);
                let (mx, my) = self.tp.mouse;
                if let Some(v) = self.transport_ask("Tempo (BPM)", &cur, mx, my) {
                    if let Ok(b) = v.trim().replace(',', ".").parse::<f64>() {
                        r.set_bpm(b, true);
                    }
                }
            }
            Act::EditSig => r.main_on_command(cmd::PROJECT_SETTINGS),
            Act::SetRate { rate, .. } => r.set_play_rate(rate),
            Act::Item(i) => {
                if let Some(&(_, c)) = self.tp.shown.get(i) {
                    // Nos propres actions se lancent ici : par REAPER, elles
                    // repasseraient par le module, occupe a ce message, et le
                    // chargeur les jetterait (BUSY). Guitar Tab ne partait pas.
                    if !cp_sys::module::Module::on_action(self, c) {
                        r.main_on_command(c);
                    }
                }
            }
            Act::ItemMenu { i, x, y } => self.item_menu(i, x, y),
            Act::AddItem => self.add_item(),
            Act::BarMenu { x, y } => self.bar_menu(x, y),
            Act::RecordMenu { x, y } => self.record_menu(x, y),
            // Les vues changent au battement, jamais pendant un message.
            Act::View(i) => self.queue(match i {
                0 => crate::WinOp::ViewArrange,
                1 => crate::WinOp::ViewBrowser,
                2 => crate::WinOp::GtabOpen,
                3 => crate::WinOp::EdOpen,
                4 => crate::WinOp::SesOpen,
                5 => crate::WinOp::FxbOpen,
                _ => crate::WinOp::FxcOpen,
            }),
            // La place d'une vue, depuis son icone : le popover de l'ecran.
            Act::ViewMenu { i, x, y } => match i {
                1 => self.browser_place_menu(x, y),
                2 => {
                    let e = self.gtab_place_entries();
                    let id = self.transport_menu(x, y, &e);
                    self.gtab_place_pick(id);
                }
                3 => {
                    let e = self.ed_place_entries();
                    let id = self.transport_menu(x, y, &e);
                    self.ed_place_pick(id);
                }
                4 => {
                    let e = self.ses_place_entries();
                    let id = self.transport_menu(x, y, &e);
                    self.ses_place_pick(id);
                }
                5 => {
                    let e = self.fxb_place_entries();
                    let id = self.transport_menu(x, y, &e);
                    self.fxb_place_pick(id);
                }
                6 => {
                    let e = self.fxc_place_entries();
                    let id = self.transport_menu(x, y, &e);
                    self.fxc_place_pick(id);
                }
                _ => {}
            },
        }
        let st = self.read_status();
        self.tp.t.set_status(st);
        self.tp.t.views = self.view_tabs();
        self.tp.t.mark();
        self.transport_render(false);
    }

    fn add_item(&mut self) {
        if !self.tp.prompt && self.r.prompt_for_action(1) >= 0 {
            self.tp.prompt = true;
        }
    }

    fn menu_at(&mut self, x: f32, y: f32, items: &[Option<MenuItem<'_>>]) -> u32 {
        let e = ui::flat_to_tree(items);
        self.transport_menu(x, y, &e)
    }

    /// Un menu sous le point (`x`, `y`) de la barre, en popover sur l'ecran :
    /// 0 tant que le choix n'est pas fait (le geste se rejouera avec lui).
    pub(crate) fn transport_menu(&mut self, x: f32, y: f32, e: &[MenuEntry]) -> u32 {
        let Some(h) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return 0;
        };
        self.tp.sp.menu(h, x, y, e)
    }

    /// Une valeur a saisir, en popover sur l'ecran.
    pub(crate) fn transport_ask(
        &mut self,
        title: &str,
        cur: &str,
        x: f32,
        y: f32,
    ) -> Option<String> {
        let h = self.tp.view.as_ref().map(|v| v.hwnd)?;
        self.tp.sp.text(h, title, cur, x, y)
    }

    /// Pose le popover a l'ecran (ou l'en retire).
    pub(crate) fn transport_pop_render(&mut self) {
        // Ferme, son image est deja retiree (`ScreenPop::close`).
        if !self.tp.sp.is_open() {
            self.tp.pop_theme = None;
            return;
        }
        if self.tp.pop_theme.is_none() {
            let sc = self.tp.view.as_ref().map_or(1.0, |v| v.theme.scale);
            self.tp.pop_theme = Some(self.reaper_theme(sc));
        }
        let Some(th) = self.tp.pop_theme.clone() else {
            return;
        };
        self.tp
            .sp
            .render(&th, &mut self.text, &mut self.icons, &mut self.scratch);
    }

    fn item_menu(&mut self, i: usize, x: f32, y: f32) {
        let Some(&(ci, _)) = self.tp.shown.get(i) else {
            return;
        };
        let name = self
            .tp
            .t
            .items
            .get(i)
            .map(|it| it.name.clone())
            .unwrap_or_default();
        let run = format!("Run: {name}");
        let n = self.tp.cfg.len();
        let is_gtab = self
            .tp
            .cfg
            .get(ci)
            .is_some_and(|c| c.id == "_CPSTUDIO_GUITARTAB");
        let mut items = vec![
            Some(MenuItem {
                id: M_RUN,
                text: &run,
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_LEFT,
                text: "Move left",
                checked: false,
                enabled: ci > 0,
            }),
            Some(MenuItem {
                id: M_RIGHT,
                text: "Move right",
                checked: false,
                enabled: ci + 1 < n,
            }),
            Some(MenuItem {
                id: M_REMOVE,
                text: "Remove from CP Transport",
                checked: false,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_ADD,
                text: "Add action...",
                checked: false,
                enabled: true,
            }),
        ];
        if is_gtab {
            items.insert(
                1,
                Some(MenuItem {
                    id: M_GTAB_SETTINGS,
                    text: "Guitar Tab settings...",
                    checked: false,
                    enabled: true,
                }),
            );
        }
        match self.menu_at(x, y, &items) {
            M_RUN => self.transport_act(Act::Item(i)),
            M_GTAB_SETTINGS => self.gtab_edit_settings(),
            M_LEFT if ci > 0 => {
                self.tp.cfg.swap(ci, ci - 1);
                self.save_items();
                self.refresh_items();
            }
            M_RIGHT if ci + 1 < n => {
                self.tp.cfg.swap(ci, ci + 1);
                self.save_items();
                self.refresh_items();
            }
            M_REMOVE => {
                self.tp.cfg.remove(ci);
                self.save_items();
                self.refresh_items();
            }
            M_ADD => self.add_item(),
            _ => {}
        }
    }

    /// Le mode d'enregistrement, comme le menu du bouton Rec de REAPER.
    fn record_menu(&mut self, x: f32, y: f32) {
        let m = self.tp.t.st.rec_mode;
        // Une action absente (une autre version de REAPER) reste grisee.
        let has = |c: i32| !self.r.command_text(c).is_empty();
        let items = [
            Some(MenuItem {
                id: M_REC_NORMAL,
                text: "Record mode: normal",
                checked: m == RecMode::Normal,
                enabled: has(cmd::REC_NORMAL),
            }),
            Some(MenuItem {
                id: M_REC_TIMESEL,
                text: "Record mode: time selection auto-punch",
                checked: m == RecMode::TimeSel,
                enabled: has(cmd::REC_TIMESEL),
            }),
            Some(MenuItem {
                id: M_REC_ITEMS,
                text: "Record mode: auto-punch selected items",
                checked: m == RecMode::Items,
                enabled: has(cmd::REC_ITEMS),
            }),
        ];
        let c = match self.menu_at(x, y, &items) {
            M_REC_NORMAL => cmd::REC_NORMAL,
            M_REC_TIMESEL => cmd::REC_TIMESEL,
            M_REC_ITEMS => cmd::REC_ITEMS,
            _ => return,
        };
        self.r.main_on_command(c);
    }

    fn bar_menu(&mut self, x: f32, y: f32) {
        use cp_transport::TunerMode;
        let tm = self.tp.tu.mode;
        // La clock MIDI externe : suivre le tempo et le depart d'une autre
        // machine (Ableton, une boite a rythmes) sur une entree MIDI.
        let ins = self.r.midi_inputs();
        let cur = self.tp.clock.dev;
        let mut clock = vec![MenuEntry::check(M_CLOCK_OFF, "Off", cur < 0)];
        for (i, n) in &ins {
            clock.push(MenuEntry::check(
                M_CLOCK_DEV + *i as u32,
                n.clone(),
                cur == *i,
            ));
        }
        // Le retard du maitre : ce qui reste a caler a l'oreille (la
        // latence de la carte est deja comptee).
        clock.push(MenuEntry::Sep);
        for (k, d) in DELAYS.iter().enumerate() {
            clock.push(MenuEntry::Item {
                id: M_CLOCK_DELAY + k as u32,
                text: format!("Delay {d:+.0} ms"),
                checked: (self.tp.clock.delay_ms - d).abs() < 1e-9,
                enabled: cur >= 0,
            });
        }
        let tuner = vec![
            MenuEntry::check(
                M_TUNER_CLOCK,
                "In the clock, when stopped",
                tm == TunerMode::Clock,
            ),
            MenuEntry::check(
                M_TUNER_SLOT,
                "Its own place, in the middle",
                tm == TunerMode::Slot,
            ),
            MenuEntry::check(M_TUNER_OFF, "Off", tm == TunerMode::Off),
            MenuEntry::Sep,
            MenuEntry::Item {
                id: M_TUNER_A4,
                text: format!("Reference: A4 = {} Hz…", self.tp.tu.a4),
                checked: false,
                enabled: tm != TunerMode::Off,
            },
        ];
        let e = vec![
            MenuEntry::item(M_ADD, "Add action…"),
            MenuEntry::Sep,
            MenuEntry::Sub("Tuner".into(), tuner),
            MenuEntry::Sub("External MIDI clock".into(), clock),
            MenuEntry::Sep,
            MenuEntry::item(M_CLOSE, "Close CP Transport (show REAPER's transport)"),
        ];
        let k = self.transport_menu(x, y, &e);
        if (M_CLOCK_DELAY..M_CLOCK_DELAY + DELAYS.len() as u32).contains(&k) {
            let d = DELAYS[(k - M_CLOCK_DELAY) as usize];
            self.tp.clock.delay_ms = d;
            self.r
                .ext_set(SECTION, "ext_clock_delay", &d.to_string(), true);
            return;
        }
        if k == M_CLOCK_OFF || k >= M_CLOCK_DEV {
            let dev = if k == M_CLOCK_OFF {
                -1
            } else {
                (k - M_CLOCK_DEV) as i32
            };
            self.clock_set_device(dev);
            return;
        }
        match k {
            M_ADD => self.add_item(),
            M_CLOSE => self.queue(crate::WinOp::TransportOff),
            M_TUNER_OFF => self.tuner_set_mode(TunerMode::Off),
            M_TUNER_CLOCK => self.tuner_set_mode(TunerMode::Clock),
            M_TUNER_SLOT => self.tuner_set_mode(TunerMode::Slot),
            M_TUNER_A4 => self.tuner_ask_a4(x, y),
            _ => {}
        }
    }

    fn browser_place_menu(&mut self, x: f32, y: f32) {
        let p = self.place;
        let open = self.view.is_some();
        let items = [
            Some(MenuItem {
                id: M_PLACE_ARRANGE,
                text: "Show in arrange view",
                checked: p == Place::Arrange,
                enabled: true,
            }),
            Some(MenuItem {
                id: M_PLACE_DOCK,
                text: "Dock in docker",
                checked: p == Place::Dock,
                enabled: true,
            }),
            Some(MenuItem {
                id: M_PLACE_FLOAT,
                text: "Floating window",
                checked: p == Place::Float,
                enabled: true,
            }),
            None,
            Some(MenuItem {
                id: M_VIEW_CLOSE,
                text: "Close Browser",
                checked: false,
                enabled: open,
            }),
        ];
        match self.menu_at(x, y, &items) {
            M_PLACE_ARRANGE => self.queue(crate::WinOp::Place(Place::Arrange)),
            M_PLACE_DOCK => self.queue(crate::WinOp::Place(Place::Dock)),
            M_PLACE_FLOAT => self.queue(crate::WinOp::Place(Place::Float)),
            M_VIEW_CLOSE => self.queue(crate::WinOp::Close),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Les messages de la fenetre
    // -----------------------------------------------------------------------

    /// Un evenement pour le popover ouvert ; sa reponse rejoue le geste.
    fn transport_pop_feed(&mut self, ev: Ev) {
        if let Some(Some(a)) = self.tp.sp.feed(&ev) {
            self.transport_act(a);
        }
        self.transport_pop_render();
    }

    pub(crate) fn transport_wnd(&mut self, m: &WinMsg) -> Option<isize> {
        let hwnd = self.tp.view.as_ref()?.hwnd;
        if m.hwnd != hwnd {
            return None;
        }
        let (x, y) = (lo16(m.lp), hi16(m.lp));
        let md = mods();
        let ev = match m.msg {
            cp_abi::WM_CP_FRAME => {
                self.transport_on_frame();
                return Some(0);
            }
            // Le clavier va a REAPER, sauf a un popover ouvert.
            cp_abi::WM_CP_ACCEL => {
                let Some(k) = cp_sys::module::key_event(m) else {
                    return Some(0);
                };
                let vk = k.wp as u32;
                let down = matches!(k.message, msg::KEYDOWN | msg::SYSKEYDOWN);
                let chr = matches!(k.message, msg::CHAR | msg::SYSCHAR);
                return Some(match self.tp.sp.key(vk, md, down, chr) {
                    Some(crate::ask::KeyDo::Feed) => {
                        if md.ctrl && vk == 0x56 {
                            if let (Some(t), Some(p)) =
                                (ui::clipboard_text(hwnd), self.tp.sp.ph.pop.as_mut())
                            {
                                p.paste(&t);
                            }
                            self.transport_pop_render();
                        } else {
                            self.transport_pop_feed(Ev::Key {
                                vk,
                                mods: md,
                                text: None,
                            });
                        }
                        1
                    }
                    Some(crate::ask::KeyDo::Code(c)) => c,
                    None => 0,
                });
            }
            msg::CHAR => {
                if let Some(c) = char::from_u32(m.wp as u32) {
                    if self.tp.sp.is_open() {
                        self.transport_pop_feed(Ev::Char(c));
                    }
                }
                return Some(0);
            }
            // La souris prise par un autre : le popover se ferme.
            msg::CAPTURECHANGED => {
                if self.tp.sp.is_open() && m.lp != hwnd.0 {
                    self.tp.sp.close();
                    self.transport_pop_render();
                }
                return Some(0);
            }
            msg::SIZE => {
                self.tp.t.mark();
                self.transport_render(true);
                return Some(0);
            }
            msg::PAINT => {
                drop(win::Paint::begin(hwnd));
                if let Some(v) = &mut self.tp.view {
                    let (w, h) = (v.surf.w, v.surf.h);
                    if w > 0 && h > 0 {
                        ui::present(hwnd, &v.surf.px, w, h, &[(0, 0, w, h)], &mut v.present);
                    } else {
                        self.transport_render(true);
                    }
                }
                return Some(0);
            }
            msg::ERASEBKGND => return Some(1),
            // Un clic n'active rien et ne prend pas le focus.
            msg::MOUSEACTIVATE => return Some(3), // MA_NOACTIVATE
            msg::MOUSEMOVE => {
                if let Some(v) = &mut self.tp.view {
                    if !v.tracking {
                        v.tracking = true;
                        ui::track_leave(hwnd);
                    }
                }
                Ev::Move { x, y, mods: md }
            }
            msg::MOUSELEAVE => {
                if let Some(v) = &mut self.tp.view {
                    v.tracking = false;
                }
                Ev::Leave
            }
            msg::LBUTTONDOWN | msg::LBUTTONDBLCLK | msg::RBUTTONDOWN | msg::RBUTTONDBLCLK => {
                let b = if matches!(m.msg, msg::LBUTTONDOWN | msg::LBUTTONDBLCLK) {
                    Button::Left
                } else {
                    Button::Right
                };
                if b == Button::Left {
                    ui::capture(hwnd);
                }
                Ev::Down {
                    x,
                    y,
                    b,
                    dbl: matches!(m.msg, msg::LBUTTONDBLCLK | msg::RBUTTONDBLCLK),
                    mods: md,
                }
            }
            msg::LBUTTONUP | msg::RBUTTONUP => {
                // Un popover ouvert garde la souris jusqu'au choix.
                if ui::has_capture(hwnd) && !self.tp.sp.is_open() {
                    ui::release_capture();
                }
                Ev::Up {
                    x,
                    y,
                    b: if m.msg == msg::LBUTTONUP {
                        Button::Left
                    } else {
                        Button::Right
                    },
                    mods: md,
                }
            }
            msg::MOUSEWHEEL => {
                let (cx, cy) = ui::screen_to_client(hwnd, x as i32, y as i32);
                let d = ((m.wp >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                Ev::Wheel {
                    x: cx as f32,
                    y: cy as f32,
                    dx: 0.0,
                    dy: d,
                    mods: md,
                }
            }
            msg::CLOSE => {
                self.queue(crate::WinOp::TransportOff);
                return Some(0);
            }
            _ => return None,
        };
        if let Ev::Move { x, y, .. } | Ev::Down { x, y, .. } | Ev::Up { x, y, .. } = ev {
            self.tp.mouse = (x, y);
        }
        // Un popover ouvert prend la souris (ses coordonnees passent sur
        // l'ecran) ; sa reponse rejoue le geste.
        if self.tp.sp.is_open() {
            self.transport_pop_feed(ev);
            return Some(0);
        }
        if matches!(ev, Ev::Down { .. } | Ev::Leave) {
            if let Some(t) = &mut self.tp.tip {
                t.hide();
            }
            // Le survol recommence a zero : pas de bulle juste apres un clic.
            self.tp.hover = (cp_transport::Hit::None, std::time::Instant::now());
        }
        let act = self.tp.t.event(&ev);
        if self.tp.t.dragging() {
            self.transport_want_frames();
        }
        match act {
            Some(a) => self.transport_act(a),
            None => self.transport_render(false),
        }
        Some(0)
    }
}

// ---------------------------------------------------------------------------
// L'auto-test des popovers du transport
// ---------------------------------------------------------------------------

impl App {
    /// « tpop_open » : le menu de la barre s'ouvre sur l'ecran, la souris
    /// tenue ; « tpop_check » : Echap le ferme et rend la souris, puis le
    /// tempo se saisit au clavier (133, remis ensuite).
    pub(crate) fn transport_selftest_pop(&mut self, step: &str) -> String {
        let Some(h) = self.tp.view.as_ref().map(|v| v.hwnd) else {
            return "tpop no_transport".into();
        };
        let key = |vk: u32| Ev::Key {
            vk,
            mods: cp_ui::Mods::default(),
            text: None,
        };
        match step {
            "tpop_open" => {
                self.tp.sp.close();
                self.transport_act(Act::BarMenu { x: 40.0, y: 18.0 });
                let img = self.tp.sp.image_window().map_or(0, |w| w.0);
                format!(
                    "tpop open={} capture={} shown={} img={img}",
                    self.tp.sp.is_open() as u8,
                    ui::has_capture(h) as u8,
                    (img != 0) as u8
                )
            }
            _ => {
                let still = self.tp.sp.is_open() as u8;
                self.transport_pop_feed(key(cp_ui::vk::ESCAPE));
                let closed = (!self.tp.sp.is_open() && !ui::has_capture(h)) as u8;
                let bpm0 = self.read_status().bpm;
                self.tp.mouse = (120.0, 18.0);
                self.transport_act(Act::EditBpm);
                let ask = self.tp.sp.is_open() as u8;
                for c in "133".chars() {
                    self.transport_pop_feed(Ev::Char(c));
                }
                self.transport_pop_feed(key(cp_ui::vk::RETURN));
                let bpm = self.read_status().bpm;
                let done = !self.tp.sp.is_open() as u8;
                self.r.set_bpm(bpm0, true);
                format!("tpop still={still} esc_closed={closed} ask={ask} bpm={bpm:.1} done={done}")
            }
        }
    }
}
