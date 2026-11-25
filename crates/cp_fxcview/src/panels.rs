//! Les onglets des JSFX internes : le Sound Gen et le sequenceur. Chaque
//! controle nomme le curseur du JSFX qu'il tient (`Ctl::Js(onglet, p)`) ; le
//! module ecrit la valeur (normalisee pour une forme `:log` / `:sqr`).

use cp_gfx::text::Align;

use crate::build::{B, F, Look, Shade, note_name, src_color, tw};
use crate::{Ctl, Data, Js, R, View};

pub const SG: u8 = 0;
pub const SEQ: u8 = 1;

pub const WAVES: [&str; 6] = ["Sine", "Triangle", "Square", "Saw", "Noise", "Click"];
pub const SCALES: [&str; 17] = [
    "Off",
    "Chromatic",
    "Major",
    "Minor",
    "Dorian",
    "Phrygian",
    "Lydian",
    "Mixolydian",
    "Locrian",
    "Harmonic Minor",
    "Melodic Minor",
    "Major Pentatonic",
    "Minor Pentatonic",
    "Blues",
    "Whole Tone",
    "Fifths",
    "Octaves",
];
pub const ROOTS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
pub const ENV_SYNCS: [&str; 8] = [
    "Free", "1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars", "1/32",
];

/// Les curseurs d'un oscillateur du Sound Gen.
pub struct Osc {
    pub on: usize,
    pub wave: usize,
    pub freq: usize,
    pub width: usize,
    pub vol: usize,
    pub color: usize,
    pub pan: usize,
    pub pm: usize,
    pub fine: usize,
    pub scale: usize,
    pub root: usize,
    /// (relatif, demi-tons, cents) pour OSC 2 et 3.
    pub rel: Option<(usize, usize, usize)>,
}

pub const OSC: [Osc; 3] = [
    Osc {
        on: 17,
        wave: 1,
        freq: 2,
        width: 3,
        vol: 18,
        color: 5,
        pan: 38,
        pm: 86,
        fine: 89,
        scale: 92,
        root: 93,
        rel: None,
    },
    Osc {
        on: 19,
        wave: 20,
        freq: 21,
        width: 22,
        vol: 23,
        color: 24,
        pan: 39,
        pm: 87,
        fine: 90,
        scale: 94,
        root: 95,
        rel: Some((32, 33, 34)),
    },
    Osc {
        on: 25,
        wave: 26,
        freq: 27,
        width: 28,
        vol: 29,
        color: 30,
        pan: 40,
        pm: 88,
        fine: 91,
        scale: 96,
        root: 97,
        rel: Some((35, 36, 37)),
    },
];

/// La note d'une frequence (69 = A4).
pub fn midi_of(hz: f64) -> f64 {
    69.0 + 12.0 * (hz.max(1.0) / 440.0).log2()
}

fn card(b: &mut B, r: R, title: &str, caption: &str) -> f32 {
    let th = b.th;
    b.panel(r, Shade::Card);
    let hh = b.s(20.0);
    b.text(
        R::new(r.x + b.s(6.0), r.y, r.w, hh),
        title,
        F::Bold,
        th.text,
        Align::Left,
    );
    if !caption.is_empty() {
        b.text(
            R::new(r.x, r.y, r.w - b.s(6.0), hh),
            caption,
            F::Small,
            th.text_dim,
            Align::Right,
        );
    }
    r.y + hh
}

/// Une rangee de boutons tournes sur des curseurs du JSFX.
#[allow(clippy::type_complexity)]
fn knobs(
    b: &mut B,
    panel: u8,
    j: &Js,
    x: f32,
    y: f32,
    w: f32,
    list: &[(usize, &str, String, bool, bool)],
) -> f32 {
    let n = list.len().max(1) as f32;
    let kw = (w / n.max(4.0)).floor();
    let kh = b.s(60.0);
    for (i, (p, label, val, en, bip)) in list.iter().enumerate() {
        b.knob(
            R::new(x + kw * i as f32, y, kw, kh),
            Ctl::Js(panel, *p),
            j.n(*p),
            label,
            val,
            *en && j.has(*p),
            *bip,
            "Drop a source here to modulate it",
        );
    }
    y + kh
}

fn wave_curve(wave: u8, color: f64) -> Vec<(f32, f32)> {
    (0..=128)
        .map(|i| {
            let t = i as f32 / 128.0;
            let ph = (t * 2.0).fract();
            let v = match wave {
                0 => (ph * std::f32::consts::TAU).cos(),
                1 => {
                    let tri = 4.0 * ph - 1.0;
                    if tri > 1.0 { 2.0 - tri } else { tri }
                }
                2 => {
                    if ph < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                3 => 1.0 - 2.0 * ph,
                4 => {
                    // Un bruit stable (le meme a chaque image).
                    let h = ((i as u32).wrapping_mul(2_654_435_761) >> 16) as f32 / 65535.0;
                    (h * 2.0 - 1.0) * (1.0 - color as f32 * 0.6)
                }
                _ => {
                    if t < 0.05 {
                        1.0 - t / 0.05
                    } else {
                        0.0
                    }
                }
            };
            (t, 0.5 + 0.45 * v)
        })
        .collect()
}

/// Le Sound Gen.
pub(crate) fn sound_gen(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    let bh = b.s(22.0);
    b.push(r, Ctl::None, Look::Clip(3), "");
    b.clip = Some(3);
    b.dy = v.scroll[3];
    let mut y = r.y;
    let on = d.sg.is_some_and(|j| j.enabled);
    b.chip(
        R::new(r.x, y, b.s(64.0), bh),
        Ctl::SgOn,
        if on { "● ON" } else { "○ OFF" },
        on,
        true,
        "Turn the Sound Gen on or off (off, it leaves the chain and keeps its patch)",
    );
    let Some(j) = d.sg.filter(|j| j.enabled) else {
        b.clip = None;
        b.dy = 0.0;
        return bh;
    };
    let trig = j.r(0) > 0.5;
    b.seg(
        R::new(r.x + b.s(70.0), y, b.s(180.0), bh),
        &["Continuous", "Triggered"],
        trig as usize,
        |i| Ctl::JsSet(SG, 0, i as i32),
        &[
            "Continuous: a drone (or a rhythmic gate)",
            "Triggered: notes (MIDI or the keyboard)",
        ],
    );
    let n_on = OSC.iter().filter(|o| j.r(o.on) > 0.5).count();
    b.text(
        R::new(r.x, y, r.w, bh),
        &format!("{n_on} osc"),
        F::Small,
        th.text_dim,
        Align::Right,
    );
    y += bh + b.s(6.0);
    let gap = b.s(6.0);
    let card_min = b.s(200.0);
    let cols = if r.w >= 3.0 * card_min + 2.0 * gap {
        3
    } else if r.w >= 2.0 * card_min + gap {
        2
    } else {
        1
    };
    let cw = ((r.w - gap * (cols - 1) as f32) / cols as f32).floor();
    let card_h = b.s(190.0);
    type Card = Box<dyn Fn(&mut B, R)>;
    let mut cards: Vec<Card> = Vec::new();
    for (i, o) in OSC.iter().enumerate() {
        let j = j.clone();
        cards.push(Box::new(move |b: &mut B, r: R| osc_card(b, &j, i, o, r)));
    }
    {
        let j = j.clone();
        cards.push(Box::new(move |b: &mut B, r: R| env_card(b, &j, r)));
    }
    {
        let j = j.clone();
        cards.push(Box::new(move |b: &mut B, r: R| filter_card(b, &j, r)));
    }
    {
        let j = j.clone();
        cards.push(Box::new(move |b: &mut B, r: R| voice_card(b, &j, r)));
    }
    for (i, f) in cards.iter().enumerate() {
        let (c, row) = if cols == 3 {
            (i % 3, i / 3)
        } else if cols == 2 {
            (if i < 3 { 0 } else { 1 }, i % 3)
        } else {
            (0, i)
        };
        let cr = R::new(
            r.x + (cw + gap) * c as f32,
            y + (card_h + gap) * row as f32,
            cw,
            card_h,
        );
        f(b, cr);
    }
    let rows = if cols == 1 {
        6
    } else {
        2 + (cols == 2) as usize
    };
    y += (card_h + gap) * rows as f32;
    b.clip = None;
    b.dy = 0.0;
    y - r.y
}

fn osc_card(b: &mut B, j: &Js, i: usize, o: &Osc, r: R) {
    let th = b.th;
    let bh = b.s(20.0);
    b.panel(r, Shade::Card);
    let on = j.r(o.on) > 0.5;
    let mut y = r.y + b.s(4.0);
    let x = r.x + b.s(4.0);
    let w = r.w - b.s(8.0);
    b.chip(
        R::new(x, y, bh, bh),
        Ctl::JsToggle(SG, o.on),
        &format!("{}", i + 1),
        on,
        true,
        "Turn this oscillator on or off",
    );
    let wave = j.r(o.wave).round().clamp(0.0, 5.0) as u8;
    b.chip(
        R::new(x + bh + b.s(4.0), y, b.s(80.0), bh),
        Ctl::JsMenu(SG, o.wave),
        WAVES[wave as usize],
        true,
        true,
        "Waveform",
    );
    let sc = j.r(o.scale).round().clamp(0.0, 16.0) as usize;
    let scl = if sc == 0 {
        "SCALE".to_string()
    } else if sc == 1 {
        "Chromatic".into()
    } else {
        format!(
            "{} {}",
            ROOTS[j.r(o.root).round().clamp(0.0, 11.0) as usize],
            SCALES[sc]
        )
    };
    let sx = x + bh + b.s(88.0);
    b.chip(
        R::new(sx, y, (w - (sx - x)).max(b.s(30.0)), bh),
        Ctl::JsMenu(SG, o.scale),
        &scl,
        sc > 0,
        wave < 4,
        "Snap this oscillator to a scale",
    );
    y += bh + b.s(4.0);
    let gh = b.s(40.0);
    b.panel(R::new(x, y, w, gh), Shade::Sunk);
    b.push(
        R::new(x, y, w, gh),
        Ctl::None,
        Look::Curve {
            pts: wave_curve(wave, j.r(o.color)),
            color: if on { th.accent } else { th.text_faint },
            level: None,
        },
        "",
    );
    y += gh + b.s(2.0);
    let hz = j.r(o.freq);
    let readout = match wave {
        4 => format!("noise · color {:.2}", j.r(o.color)),
        5 => "click".into(),
        _ => {
            let m = midi_of(hz);
            let n = m.round();
            let ct = (m - n) * 100.0;
            format!(
                "{}  ·  {:.1} Hz{}",
                note_name(n.clamp(0.0, 127.0) as u8),
                hz,
                if ct.abs() >= 1.0 {
                    format!("  {ct:+.0} ct")
                } else {
                    String::new()
                }
            )
        }
    };
    b.text(
        R::new(x, y, w, b.s(15.0)),
        &readout,
        F::Small,
        th.text_dim,
        Align::Center,
    );
    y += b.s(16.0);
    let rel = o.rel.filter(|(rp, _, _)| j.r(*rp) > 0.5);
    let mut list: Vec<(usize, &str, String, bool, bool)> = Vec::new();
    match (rel, wave) {
        (_, 4) => list.push((o.color, "COLOR", format!("{:.2}", j.r(o.color)), on, false)),
        (_, 5) => list.push((o.freq, "PITCH", "—".into(), false, false)),
        (Some((_, semi, _)), _) => {
            list.push((semi, "PITCH", format!("{:+.0} st", j.r(semi)), on, true))
        }
        (None, _) => {
            let m = midi_of(hz).round().clamp(0.0, 127.0) as u8;
            list.push((o.freq, "PITCH", note_name(m), on, false))
        }
    }
    match rel {
        Some((_, _, cents)) => {
            list.push((cents, "FINE", format!("{:+.1} ct", j.r(cents)), on, true))
        }
        None => list.push((
            o.fine,
            "FINE",
            format!("{:+.1} ct", j.r(o.fine)),
            on && wave < 4,
            true,
        )),
    }
    list.push((o.vol, "VOL", format!("{:.2}", j.r(o.vol)), on, false));
    list.push((
        o.pan,
        "PAN",
        format!("{:+.0}", j.r(o.pan) * 100.0),
        on,
        true,
    ));
    if i == 0 {
        list.push((132, "FM", format!("{:.2}", j.r(132)), on, false));
    }
    y = knobs(b, SG, j, x, y, w, &list);
    let ch = b.s(16.0);
    if let Some((rp, _, _)) = o.rel {
        b.chip(
            R::new(x, y, b.s(48.0), ch),
            Ctl::JsToggle(SG, rp),
            if j.r(rp) > 0.5 { "REL" } else { "FREE" },
            j.r(rp) > 0.5,
            true,
            "REL: tuned from OSC 1 · FREE: its own pitch",
        );
    }
    b.push(
        R::new(x + b.s(52.0), y + b.s(2.0), w - b.s(52.0), ch - b.s(4.0)),
        Ctl::Js(SG, o.width),
        Look::Fader {
            v: j.n(o.width),
            label: format!("WIDTH {:.0} ct", j.r(o.width)),
            enabled: on,
        },
        "Stereo width (cents between left and right)",
    );
}

/// Les curseurs d'une enveloppe du Sound Gen : (DLY, A, H, D, S, R, SYNC).
pub fn env_params(n: u8) -> [usize; 7] {
    match n {
        2 => [77, 59, 78, 60, 61, 62, 79],
        3 => [80, 64, 81, 65, 66, 67, 82],
        4 => [83, 69, 84, 70, 71, 72, 85],
        _ => [74, 11, 75, 12, 13, 14, 76],
    }
}

fn env_curve(j: &Js) -> Vec<(f32, f32)> {
    env_curve_of(j, env_params(1))
}

fn env_curve_of(j: &Js, e: [usize; 7]) -> Vec<(f32, f32)> {
    let sh = |t: f64, max: f64| (t / max).clamp(0.0, 1.0).sqrt() as f32;
    let (dl, a, h, dc, s, rl) = (
        sh(j.r(e[0]), 2.0) * 0.12,
        sh(j.r(e[1]), 2.0) * 0.22 + 0.01,
        sh(j.r(e[2]), 2.0) * 0.12,
        sh(j.r(e[3]), 2.0) * 0.22 + 0.01,
        j.r(e[4]) as f32,
        sh(j.r(e[5]), 5.0) * 0.20 + 0.01,
    );
    let mut x = 0.0;
    let mut pts = vec![(0.0, 0.0)];
    x += dl;
    pts.push((x, 0.0));
    x += a;
    pts.push((x, 1.0));
    x += h;
    pts.push((x, 1.0));
    x += dc;
    pts.push((x, s));
    x += 0.12;
    pts.push((x, s));
    x += rl;
    pts.push((x, 0.0));
    let k = 1.0 / x.max(0.01);
    pts.into_iter().map(|(a, b)| (a * k, b)).collect()
}

fn env_card(b: &mut B, j: &Js, r: R) {
    let th = b.th;
    let bh = b.s(18.0);
    let trig = j.r(0) > 0.5;
    let mut y = card(b, r, "ENVELOPE", "");
    let x = r.x + b.s(4.0);
    let w = r.w - b.s(8.0);
    let gh = b.s(44.0);
    b.panel(R::new(x, y, w, gh), Shade::Sunk);
    let pts = if trig {
        env_curve(j)
    } else {
        // La porte rythmique, quatre cycles.
        let duty = j.r(8) as f32;
        (0..=128)
            .map(|i| {
                let t = i as f32 / 128.0;
                let ph = (t * 4.0).fract();
                (
                    t,
                    if j.r(6) > 0.5 {
                        if ph < duty { 0.95 } else { 0.05 }
                    } else {
                        0.95
                    },
                )
            })
            .collect()
    };
    b.push(
        R::new(x, y, w, gh),
        Ctl::None,
        Look::Curve {
            pts,
            color: th.accent,
            level: Some(j.r(31) as f32),
        },
        "",
    );
    y += gh + b.s(4.0);
    if trig {
        let sy = j.r(76).round().clamp(0.0, 7.0) as usize;
        let t = |p: usize| {
            if sy > 0 {
                let max = if p == 14 { 5.0 } else { 2.0 };
                format!(
                    "{}x{}",
                    (j.r(p) * 16.0 / max).round().clamp(0.0, 16.0),
                    ENV_SYNCS[sy]
                )
            } else {
                format!("{:.3} s", j.r(p))
            }
        };
        let list = [
            (74, "DLY", t(74), true, false),
            (11, "A", t(11), true, false),
            (75, "H", t(75), true, false),
            (12, "D", t(12), true, false),
            (13, "S", format!("{:.2}", j.r(13)), true, false),
            (14, "R", t(14), true, false),
        ];
        y = knobs(b, SG, j, x, y, w, &list);
        let cw = ((w - b.s(8.0)) / 4.0).floor();
        b.chip(
            R::new(x, y, cw, bh),
            Ctl::JsToggle(SG, 10),
            "ADSR",
            j.r(10) > 0.5,
            true,
            "ADSR on, or a plain gate",
        );
        b.seg(
            R::new(x + cw + b.s(4.0), y, cw * 2.0, bh),
            &["Manual", "MIDI"],
            (j.r(15) > 0.5) as usize,
            |i| Ctl::JsSet(SG, 15, i as i32),
            &[
                "Manual: the HOLD button and the keyboard",
                "MIDI: notes from the track",
            ],
        );
        b.chip(
            R::new(x + 3.0 * cw + b.s(8.0), y, cw, bh),
            Ctl::JsMenu(SG, 76),
            ENV_SYNCS[sy],
            sy > 0,
            true,
            "Envelope timing: seconds or tempo divisions",
        );
        // HOLD : une note tant qu'on appuie (Manual seulement).
        let manual = j.r(15) < 0.5;
        b.push(
            R::new(x, y + bh + b.s(4.0), w, bh),
            Ctl::SgHold,
            Look::Button {
                label: "HOLD".into(),
                icon: None,
                on: false,
                enabled: manual,
                tone: crate::build::Tone::Normal,
            },
            "Press and hold: plays the Sound Gen (Manual trigger)",
        );
    } else {
        let list = [
            (7, "RATE", format!("{:.2} Hz", j.r(7)), j.r(6) > 0.5, false),
            (
                8,
                "DUTY",
                format!("{:.0} %", j.r(8) * 100.0),
                j.r(6) > 0.5,
                false,
            ),
            (9, "CURVE", format!("{:.2}", j.r(9)), j.r(6) > 0.5, false),
        ];
        y = knobs(b, SG, j, x, y, w, &list);
        let cw = ((w - b.s(4.0)) / 2.0).floor();
        b.chip(
            R::new(x, y, cw, bh),
            Ctl::JsToggle(SG, 6),
            "RHYTHMIC",
            j.r(6) > 0.5,
            true,
            "A rhythmic gate",
        );
        b.chip(
            R::new(x + cw + b.s(4.0), y, cw, bh),
            Ctl::JsToggle(SG, 58),
            "NOTES",
            j.r(58) > 0.5,
            true,
            "Follow the keyboard's latched note",
        );
    }
}

/// L'editeur d'une enveloppe (ENV 1..4) dans le rack : sa courbe, ses six
/// boutons, sa synchronisation. Rend le bas.
pub(crate) fn env_editor(b: &mut B, j: &Js, n: u8, r: R) -> f32 {
    let th = b.th;
    let e = env_params(n);
    let mut y = r.y;
    b.push(R::new(r.x, y, r.w, 1.0), Ctl::None, Look::Sep, "");
    y += b.s(4.0);
    b.text(
        R::new(r.x, y, r.w, b.s(20.0)),
        &if n == 1 {
            "ENV 1 (amp)".to_string()
        } else {
            format!("ENV {n}")
        },
        F::Bold,
        src_color(th, 200 + n as i32),
        Align::Left,
    );
    y += b.s(20.0);
    let gh = b.s(40.0);
    b.panel(R::new(r.x, y, r.w, gh), Shade::Sunk);
    let out = [31usize, 63, 68, 73][(n as usize - 1).min(3)];
    b.push(
        R::new(r.x, y, r.w, gh),
        Ctl::None,
        Look::Curve {
            pts: env_curve_of(j, e),
            color: src_color(th, 200 + n as i32),
            level: Some(j.r(out) as f32),
        },
        "",
    );
    y += gh + b.s(4.0);
    let sy = j.r(e[6]).round().clamp(0.0, 7.0) as usize;
    let t = |p: usize, max: f64| {
        if sy > 0 {
            format!(
                "{}x{}",
                (j.r(p) * 16.0 / max).round().clamp(0.0, 16.0),
                ENV_SYNCS[sy]
            )
        } else {
            format!("{:.3} s", j.r(p))
        }
    };
    let list = [
        (e[0], "DLY", t(e[0], 2.0), true, false),
        (e[1], "A", t(e[1], 2.0), true, false),
        (e[2], "H", t(e[2], 2.0), true, false),
        (e[3], "D", t(e[3], 2.0), true, false),
        (e[4], "S", format!("{:.2}", j.r(e[4])), true, false),
        (e[5], "R", t(e[5], 5.0), true, false),
    ];
    y = knobs(b, SG, j, r.x, y, r.w, &list) + b.s(6.0);
    b.chip(
        R::new(r.x, y, b.s(90.0), b.s(18.0)),
        Ctl::JsMenu(SG, e[6]),
        &format!("SYNC {}", ENV_SYNCS[sy]),
        sy > 0,
        true,
        "Envelope timing: seconds or tempo divisions",
    );
    y + b.s(24.0)
}

fn filter_card(b: &mut B, j: &Js, r: R) {
    let th = b.th;
    let ty = j.r(41).round().clamp(0.0, 4.0) as usize;
    let mut y = card(b, r, "FILTER", &format!("{:.0} Hz", j.r(42)));
    let x = r.x + b.s(4.0);
    let w = r.w - b.s(8.0);
    let gh = b.s(44.0);
    b.panel(R::new(x, y, w, gh), Shade::Sunk);
    let cut = ((j.r(42).max(20.0) / 20.0).ln() / 1000f64.ln()) as f32;
    let res = j.r(43) as f32;
    let pts: Vec<(f32, f32)> = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.0;
            let dx = (t - cut) * 10.0;
            let v = match ty {
                1 => {
                    if dx < 0.0 {
                        0.75 + res * 0.2 * (-(dx * dx)).exp()
                    } else {
                        (0.75 - dx * 0.25).max(0.05) + res * 0.2 * (-(dx * dx)).exp()
                    }
                }
                3 => {
                    if dx > 0.0 {
                        0.75 + res * 0.2 * (-(dx * dx)).exp()
                    } else {
                        (0.75 + dx * 0.25).max(0.05) + res * 0.2 * (-(dx * dx)).exp()
                    }
                }
                2 => (0.75 - dx.abs() * 0.25).max(0.05) + res * 0.2 * (-(dx * dx)).exp(),
                4 => (0.75 - 0.6 * (-(dx * dx) * 2.0).exp()).max(0.05),
                _ => 0.75,
            };
            (t, v.min(0.98))
        })
        .collect();
    b.push(
        R::new(x, y, w, gh),
        Ctl::None,
        Look::Curve {
            pts,
            color: if ty > 0 { th.accent } else { th.text_faint },
            level: None,
        },
        "",
    );
    y += gh + b.s(4.0);
    let on = ty > 0;
    let list = [
        (42, "CUT", format!("{:.0} Hz", j.r(42)), on, false),
        (43, "RES", format!("{:.2}", j.r(43)), on, false),
        (44, "ENV", format!("{:+.0} %", j.r(44) * 100.0), on, true),
        (45, "KEY", format!("{:.0} %", j.r(45) * 100.0), on, false),
        (46, "VEL", format!("{:+.0} %", j.r(46) * 100.0), on, true),
    ];
    y = knobs(b, SG, j, x, y, w, &list);
    b.seg(
        R::new(x, y, w, b.s(18.0)),
        &["Off", "LP", "BP", "HP", "Notch"],
        ty,
        |i| Ctl::JsSet(SG, 41, i as i32),
        &["No filter", "Low pass", "Band pass", "High pass", "Notch"],
    );
}

fn voice_card(b: &mut B, j: &Js, r: R) {
    let th = b.th;
    let peak = j.r(52);
    let db = if peak > 1e-6 {
        format!("{:.1} dB", 20.0 * peak.log10())
    } else {
        "-inf".into()
    };
    let mut y = card(b, r, "VOICE", &db);
    let x = r.x + b.s(4.0);
    let w = r.w - b.s(8.0);
    let gh = b.s(44.0);
    b.panel(R::new(x, y, w, gh), Shade::Sunk);
    let amt = j.r(48) as f32;
    let pts: Vec<(f32, f32)> = (0..=64)
        .map(|i| {
            let v = i as f32 / 64.0;
            (v, (1.0 - amt * (1.0 - v * v)) * 0.9)
        })
        .collect();
    b.push(
        R::new(x, y, w, gh),
        Ctl::None,
        Look::Curve {
            pts,
            color: th.accent,
            level: Some(peak.min(1.0) as f32),
        },
        "",
    );
    y += gh + b.s(4.0);
    let trig = j.r(0) > 0.5;
    let list = [
        (4, "AMP", format!("{:.2}", j.r(4)), true, false),
        (54, "VOICES", format!("{:.0}", j.r(54)), trig, false),
        (
            47,
            "GLIDE",
            format!("{:.0} ms", j.r(47) * 1000.0),
            trig,
            false,
        ),
        (48, "VEL", format!("{:.0} %", j.r(48) * 100.0), trig, false),
    ];
    y = knobs(b, SG, j, x, y, w, &list);
    let cw = ((w - b.s(12.0)) / 4.0).floor();
    for (i, (p, l, tip)) in [
        (53, "LEGATO", "Mono: a new note glides without a new attack"),
        (49, "RETRIG", "Reset the phase at each note"),
        (50, "AA", "Anti-aliased square and saw"),
        (
            51,
            "AUTO",
            "Auto gain: keeps the sum of the oscillators below 1",
        ),
    ]
    .iter()
    .enumerate()
    {
        b.chip(
            R::new(x + (cw + b.s(4.0)) * i as f32, y, cw, b.s(18.0)),
            Ctl::JsToggle(SG, *p),
            l,
            j.r(*p) > 0.5,
            true,
            tip,
        );
    }
}

// ---------------------------------------------------------------------------
// Le sequenceur
// ---------------------------------------------------------------------------

pub const RATES: [&str; 9] = [
    "1/32", "1/16T", "1/16", "1/8T", "1/8", "1/4T", "1/4", "1/2", "1 bar",
];
pub const DIRS: [&str; 4] = ["Forward", "Backward", "Ping-Pong", "Random"];
pub const KEYS: [&str; 3] = ["Thru", "Transpose", "Arp"];
pub const SPEEDS: [&str; 9] = [
    "1/4x", "1/3x", "1/2x", "2/3x", "1x", "3/2x", "2x", "3x", "4x",
];
pub const ARP_ORDERS: [&str; 6] = ["Up", "Down", "Up/Down", "Down/Up", "As Played", "Random"];
pub const PAT_WHEN: [&str; 4] = ["Now", "Next Beat", "Next Bar", "Pattern End"];

/// Une voie du sequenceur : nom, bornes des pas, origine des barres, voie
/// a deux etats.
pub struct Lane {
    pub name: &'static str,
    pub min: f64,
    pub max: f64,
    pub origin: f64,
    pub bools: bool,
    pub int: bool,
}

pub const LANES: [Lane; 9] = [
    Lane {
        name: "TRIG",
        min: 0.0,
        max: 1.0,
        origin: 0.0,
        bools: true,
        int: true,
    },
    Lane {
        name: "PITCH",
        min: -7.0,
        max: 14.0,
        origin: 0.0,
        bools: false,
        int: true,
    },
    Lane {
        name: "OCT",
        min: -2.0,
        max: 2.0,
        origin: 0.0,
        bools: false,
        int: true,
    },
    Lane {
        name: "VEL",
        min: 1.0,
        max: 127.0,
        origin: 1.0,
        bools: false,
        int: true,
    },
    Lane {
        name: "GATE",
        min: 1.0,
        max: 100.0,
        origin: 1.0,
        bools: false,
        int: true,
    },
    Lane {
        name: "CHANCE",
        min: 0.0,
        max: 100.0,
        origin: 0.0,
        bools: false,
        int: true,
    },
    Lane {
        name: "SLIDE",
        min: 0.0,
        max: 1.0,
        origin: 0.0,
        bools: true,
        int: true,
    },
    Lane {
        name: "MOD A",
        min: 0.0,
        max: 1.0,
        origin: 0.5,
        bools: false,
        int: false,
    },
    Lane {
        name: "MOD B",
        min: 0.0,
        max: 1.0,
        origin: 0.5,
        bools: false,
        int: false,
    },
];

/// Le premier curseur (indice) d'une voie.
pub fn lane_base(l: usize) -> usize {
    20 + 22 * l
}

/// La place de la tete de lecture de chaque voie.
pub fn lane_pos(j: &Js, l: usize) -> u8 {
    let (v, k) = if l < 5 {
        (j.r(13) as u64, l)
    } else {
        (j.r(14) as u64, l - 5)
    };
    ((v >> (4 * k)) & 15) as u8
}

pub(crate) fn sequencer(b: &mut B, v: &View, d: &Data, r: R) -> f32 {
    let th = b.th;
    let bh = b.s(22.0);
    b.push(r, Ctl::None, Look::Clip(4), "");
    b.clip = Some(4);
    b.dy = v.scroll[4];
    let mut y = r.y;
    let Some(j) = d.seq else {
        b.button(
            R::new(r.x, y, b.s(150.0), bh),
            Ctl::SeqAdd,
            "ADD SEQUENCER",
            None,
            false,
            "It sits at the top of the chain and plays whatever instrument follows it",
        );
        b.clip = None;
        b.dy = 0.0;
        return bh;
    };
    let on = j.r(0) > 0.5;
    let run = j.r(1) > 0.5;
    let clk = j.r(2).round().clamp(0.0, 2.0) as usize;
    let mut x = r.x;
    b.chip(
        R::new(x, y, b.s(56.0), bh),
        Ctl::JsToggle(SEQ, 0),
        if on { "ON" } else { "OFF" },
        on,
        true,
        "On, or paused",
    );
    x += b.s(60.0);
    b.chip(
        R::new(x, y, b.s(56.0), bh),
        Ctl::JsToggle(SEQ, 1),
        if run { "STOP" } else { "PLAY" },
        run,
        on && clk != 1,
        "Run its own clock while the transport is stopped",
    );
    x += b.s(60.0);
    b.seg(
        R::new(x, y, b.s(210.0), bh),
        &["Auto", "Transport", "Free"],
        clk,
        |i| Ctl::JsSet(SEQ, 2, i as i32),
        &[
            "Auto: the transport when REAPER plays, its own clock when stopped",
            "Transport: only while REAPER plays",
            "Free: its own clock only",
        ],
    );
    x += b.s(214.0);
    b.button(
        R::new(x, y, b.s(70.0), bh),
        Ctl::SeqRemove,
        "REMOVE",
        None,
        false,
        "Take the sequencer off the track. Its settings stay with the track and come back with ADD SEQUENCER.",
    );
    let st = j.r(15) as u32;
    let cap = if !on {
        "Off".to_string()
    } else if st & 2 != 0 {
        "Following the transport".into()
    } else if st & 1 != 0 {
        "Running free".into()
    } else {
        "Stopped".into()
    };
    let note = j.r(18);
    b.text(
        R::new(r.x, y, r.w, bh),
        &if note >= 0.0 && st & 4 != 0 {
            format!("{cap}  {}", note_name(note as u8))
        } else {
            cap
        },
        F::Small,
        th.text_dim,
        Align::Right,
    );
    y += bh + b.s(6.0);
    // Les reglages globaux.
    let kw = b.s(56.0);
    let kh = b.s(60.0);
    b.knob(
        R::new(r.x, y, kw, kh),
        Ctl::Js(SEQ, 4),
        j.n(4),
        "SWING",
        &format!("{:.0} %", j.r(4)),
        true,
        false,
        "Swing of the odd steps",
    );
    b.knob(
        R::new(r.x + kw, y, kw, kh),
        Ctl::Js(SEQ, 9),
        j.n(9),
        "TRANSP",
        &format!("{:+.0} st", j.r(9)),
        true,
        true,
        "Transpose",
    );
    let cx = r.x + 2.0 * kw + b.s(6.0);
    let cw = b.s(96.0);
    let ch = b.s(22.0);
    let sc = j.r(7).round().clamp(0.0, 15.0) as usize;
    let scale = if sc == 0 {
        "Chromatic".to_string()
    } else {
        format!(
            "{} {}",
            ROOTS[j.r(6).round().clamp(0.0, 11.0) as usize],
            SCALES[sc + 1]
        )
    };
    let chips: [(Ctl, String, bool, &str); 6] = [
        (
            Ctl::JsMenu(SEQ, 3),
            format!("Step {}", RATES[(j.r(3).round() as usize).min(8)]),
            false,
            "Step length",
        ),
        (
            Ctl::JsMenu(SEQ, 5),
            DIRS[(j.r(5).round() as usize).min(3)].to_string(),
            false,
            "Direction",
        ),
        (Ctl::JsMenu(SEQ, 7), scale, false, "Scale and root"),
        (
            Ctl::JsMenu(SEQ, 8),
            format!("Oct {}", j.r(8).round()),
            false,
            "Octave of the tonic",
        ),
        (
            Ctl::JsMenu(SEQ, 11),
            format!("Ch {}", j.r(11).round()),
            false,
            "Output MIDI channel",
        ),
        (
            Ctl::JsMenu(SEQ, 10),
            format!(
                "Keys {}",
                KEYS[(j.r(10).round() as usize).min(2)].to_lowercase()
            ),
            j.r(10) > 0.5,
            "Incoming notes: thru, transpose, or arpeggiate",
        ),
    ];
    for (i, (c, l, lit, tip)) in chips.into_iter().enumerate() {
        b.chip(
            R::new(
                cx + (cw + b.s(4.0)) * (i % 3) as f32,
                y + (ch + b.s(4.0)) * (i / 3) as f32,
                cw,
                ch,
            ),
            c,
            &l,
            lit,
            true,
            tip,
        );
    }
    b.button_ex(
        R::new(
            cx + 3.0 * (cw + b.s(4.0)),
            y,
            b.s(80.0),
            2.0 * ch + b.s(4.0),
        ),
        Ctl::SeqRollAll,
        "ROLL ALL",
        Some(cp_ui::Icon::Dices),
        true,
        true,
        crate::build::Tone::Accent,
        "Roll every lane at once, each by its own DICE amount",
    );
    y += kh.max(2.0 * ch + b.s(4.0)) + b.s(6.0);
    // Les patterns.
    if j.has(226) {
        b.text(
            R::new(r.x, y, b.s(64.0), bh),
            "PATTERN",
            F::Small,
            th.text_dim,
            Align::Left,
        );
        let cur = j.r(219).round() as i64;
        let mask = j.r(220) as u64;
        let next = j.r(221).round() as i64;
        let pw = b.s(24.0);
        for k in 0..16u8 {
            let def = mask & (1 << k) != 0;
            b.push(
                R::new(r.x + b.s(66.0) + (pw + b.s(2.0)) * k as f32, y, pw, bh),
                Ctl::SeqPattern(k),
                Look::Chip {
                    label: format!("{}", k + 1),
                    on: cur == k as i64,
                    standby: next == k as i64,
                    amber: false,
                    enabled: true,
                },
                if def {
                    "Click: play this pattern · right-click: copy, paste, clear"
                } else {
                    "An empty pattern: born a copy of the current one"
                },
            );
            let _ = def;
        }
        let px = r.x + b.s(66.0) + (pw + b.s(2.0)) * 16.0 + b.s(6.0);
        let pn = j.r(223).round() as i64;
        b.chip(
            R::new(px, y, b.s(72.0), bh),
            Ctl::JsMenu(SEQ, 223),
            &if pn <= 0 {
                "Notes off".to_string()
            } else {
                format!("Notes C{}", pn - 2)
            },
            pn > 0,
            true,
            "The MIDI notes that switch the patterns",
        );
        b.chip(
            R::new(px + b.s(76.0), y, b.s(80.0), bh),
            Ctl::JsMenu(SEQ, 224),
            PAT_WHEN[(j.r(224).round() as usize).min(3)],
            false,
            true,
            "When a new pattern starts",
        );
        y += bh + b.s(6.0);
    }
    if j.r(10).round() as i64 == 2 && j.has(229) {
        b.text(
            R::new(r.x, y, b.s(64.0), bh),
            "ARP",
            F::Small,
            th.text_dim,
            Align::Left,
        );
        b.chip(
            R::new(r.x + b.s(66.0), y, b.s(80.0), bh),
            Ctl::JsMenu(SEQ, 227),
            ARP_ORDERS[(j.r(227).round() as usize).min(5)],
            true,
            true,
            "Arp order",
        );
        b.chip(
            R::new(r.x + b.s(150.0), y, b.s(56.0), bh),
            Ctl::JsMenu(SEQ, 228),
            &format!("{} oct", j.r(228).round()),
            true,
            true,
            "Arp octaves",
        );
        b.chip(
            R::new(r.x + b.s(210.0), y, b.s(60.0), bh),
            Ctl::JsToggle(SEQ, 229),
            "LATCH",
            j.r(229) > 0.5,
            true,
            "Latch: the chord stays after the keys are released",
        );
        y += bh + b.s(6.0);
    }
    // Les neuf voies.
    let running = st & 1 != 0;
    let lw = b.s(70.0);
    let rw = b.s(232.0);
    let rh = b.s(46.0);
    let sw = (r.w - lw - rw - b.s(8.0)).max(b.s(160.0));
    for (l, lane) in LANES.iter().enumerate() {
        let base = lane_base(l);
        let ly = y;
        b.panel(R::new(r.x, ly, r.w, rh), Shade::Card);
        let name = if l == 1 && j.r(10).round() as i64 == 2 {
            "TONE"
        } else {
            lane.name
        };
        b.text(
            R::new(r.x + b.s(4.0), ly, lw, b.s(18.0)),
            name,
            F::Bold,
            th.text,
            Align::Left,
        );
        if l >= 7 {
            let slot = if l == 7 { 221 } else { 222 };
            let ds = b.s(10.0);
            b.push(
                R::new(r.x + lw - ds - b.s(4.0), ly + b.s(4.0), ds, ds),
                Ctl::TileDot(slot),
                Look::Dot {
                    color: src_color(th, slot),
                    hollow: false,
                },
                "Drag onto a knob: this lane modulates it",
            );
        }
        b.chip(
            R::new(
                r.x + b.s(4.0),
                ly + rh - b.s(20.0),
                lw - b.s(8.0),
                b.s(16.0),
            ),
            Ctl::JsMenu(SEQ, base + 17),
            SPEEDS[(j.r(base + 17).round() as usize).min(8)],
            false,
            true,
            "Lane speed (polyrhythm)",
        );
        let sx = r.x + lw;
        let len = j.r(base + 16).round().clamp(1.0, 16.0) as u8;
        b.push(
            R::new(sx, ly + b.s(2.0), sw, b.s(5.0)),
            Ctl::SeqLen(l as u8),
            Look::Fader {
                v: (len as f64 - 1.0) / 15.0,
                label: String::new(),
                enabled: true,
            },
            &format!("Length: {len} steps"),
        );
        let vals: Vec<f64> = (0..16)
            .map(|k| ((j.r(base + k) - lane.min) / (lane.max - lane.min)).clamp(0.0, 1.0))
            .collect();
        b.push(
            R::new(sx, ly + b.s(9.0), sw, rh - b.s(12.0)),
            Ctl::SeqLane(l as u8),
            Look::Steps {
                n: 16,
                vals,
                len,
                play: running.then(|| lane_pos(j, l)),
                origin: (lane.origin - lane.min) / (lane.max - lane.min),
                color: if l >= 7 {
                    src_color(th, if l == 7 { 221 } else { 222 })
                } else {
                    th.accent
                },
                bools: lane.bools,
            },
            "Paint the steps",
        );
        let kx = sx + sw + b.s(4.0);
        let kk = b.s(44.0);
        let mut list: Vec<(usize, &str, String, bool, bool)> = vec![
            (
                base + 18,
                "MUT",
                format!("{:.0} %", j.r(base + 18)),
                true,
                false,
            ),
            (
                base + 19,
                "DICE",
                format!("{:.0} %", j.r(base + 19)),
                true,
                false,
            ),
        ];
        if lane.bools {
            list.push((
                base + 21,
                "DENS",
                format!("{:.0} %", j.r(base + 21) * 100.0),
                true,
                false,
            ));
        } else {
            list.push((
                base + 20,
                "LO",
                format!("{:.4}", j.r(base + 20))
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string(),
                true,
                false,
            ));
            list.push((
                base + 21,
                "HI",
                format!("{:.4}", j.r(base + 21))
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string(),
                true,
                false,
            ));
        }
        for (i, (p, label, val, en, bip)) in list.into_iter().enumerate() {
            b.knob(
                R::new(kx + kk * i as f32, ly, kk, rh),
                Ctl::Js(SEQ, p),
                j.n(p),
                label,
                &val,
                en,
                bip,
                "",
            );
        }
        b.button(
            R::new(
                r.right() - b.s(40.0),
                ly + (rh - b.s(20.0)) / 2.0,
                b.s(36.0),
                b.s(20.0),
            ),
            Ctl::SeqRoll(l as u8),
            "ROLL",
            None,
            false,
            "Roll this lane by its DICE amount",
        );
        y += rh + b.s(3.0);
    }
    let _ = tw;
    b.clip = None;
    b.dy = 0.0;
    y - r.y
}
