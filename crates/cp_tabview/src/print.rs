//! La tablature mise en pages : l'export PDF d'un item.
//!
//! Des pages A4 a 300 DPI, noir sur blanc : un en-tete (le titre, le tempo,
//! l'accord des cordes), puis des systemes de quelques mesures, la partition
//! au-dessus de la tab (les couches que la piste montre). Les notes sont
//! dessinees par le code de la fenetre (`draw_score`, `draw_tab`) : la page
//! montre exactement ce que la vue montre, bends et glisses compris.
//!
//! L'espacement suit le temps dans chaque mesure, comme dans la vue. Chaque
//! mesure demande sa place (ses notes les plus serrees ne se touchent pas,
//! une mesure d'accords reste etroite, une mesure de doubles croches
//! s'elargit) ; un systeme prend autant de mesures qu'il en tient, puis
//! s'etire sur la largeur de la page.

use cp_gfx::pdf::{self, Page};
use cp_gfx::text::{Align, Font, TextCache};
use cp_gfx::{Canvas, Color, Scratch, Surface, hex};
use cp_tab::engrave::Engraved;
use cp_ui::Theme;

use crate::draw::{self, Geo, Warp};
use crate::{HBox, Hit, Item, Lane, Lanes, Track};

pub const DPI: f32 = 300.0;
/// A4 a 300 DPI.
pub const PAGE_W: i32 = 2480;
pub const PAGE_H: i32 = 3508;
/// L'echelle des tailles de la vue (ecrites a 96 DPI) : une note a la meme
/// taille sur la page que sur un ecran a 96 DPI.
const S: f32 = DPI / 96.0;
/// Les mesures d'un systeme, au plus.
const MAX_BARS: i64 = 6;

fn s(v: f32) -> f32 {
    v * S
}

fn mm(v: f32) -> f32 {
    v / 25.4 * DPI
}

const MARGIN_X: f32 = 15.0;
const MARGIN_TOP: f32 = 14.0;
const MARGIN_BOTTOM: f32 = 16.0;
/// La place entre deux notes voisines, et la mesure la plus etroite.
const NOTE_GAP: f32 = 18.0;
const MIN_BAR: f32 = 125.0;
/// L'en-tete de chaque systeme (la cle, TAB), et celui du premier (la
/// mesure en plus).
const HEAD: f32 = 34.0;
const HEAD_SIG: f32 = 18.0;
const GAP: f32 = 8.0;
const TAB_GAP: f32 = 11.0;

/// Un systeme : les mesures `m0..m1` sur une ligne, chacune sur sa
/// largeur (apres l'en-tete du systeme).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct System {
    pub(crate) m0: i64,
    pub(crate) m1: i64,
    pub(crate) widths: Vec<f32>,
}

impl System {
    pub(crate) fn width(&self) -> f32 {
        self.widths.iter().sum()
    }
}

/// La largeur des mesures : celle du premier systeme (la mesure en plus dans
/// son en-tete), puis celle des suivants.
pub(crate) fn avail() -> (f32, f32) {
    let cw = PAGE_W as f32 - 2.0 * mm(MARGIN_X);
    (cw - s(HEAD + HEAD_SIG), cw - s(HEAD))
}

/// La place que demande chaque mesure, en pixels par noire : ses notes les
/// plus serrees ne se touchent pas, et elle n'est jamais plus etroite que
/// `MIN_BAR`.
pub(crate) fn need_ppq(eng: &Engraved) -> Vec<f32> {
    let g = eng.grid.max(1);
    let ml = eng.mlen.max(1);
    let bq = ml as f32 / g as f32;
    let n = eng.measures.max(0) as usize;
    let mut starts: Vec<Vec<i64>> = vec![Vec::new(); n];
    for sg in &eng.signs {
        if let Some(v) = starts.get_mut(sg.measure.max(0) as usize) {
            v.push(sg.start);
        }
    }
    starts
        .into_iter()
        .enumerate()
        .map(|(m, mut v)| {
            v.sort_unstable();
            v.dedup();
            let end = (m as i64 + 1) * ml;
            let mut gap = ml;
            for w in v.windows(2) {
                gap = gap.min(w[1] - w[0]);
            }
            if let Some(&l) = v.last() {
                gap = gap.min(end - l);
            }
            let gq = gap.max(1) as f32 / g as f32;
            (s(NOTE_GAP) / gq).max(s(MIN_BAR) / bq)
        })
        .collect()
}

/// Les systemes : chacun prend les mesures qu'il tient (`first_w` pour le
/// premier, `w` ensuite), puis les etire a la largeur. Le dernier reste a
/// sa largeur naturelle s'il est court.
pub(crate) fn systems(need: &[f32], bq: f64, first_w: f32, w: f32) -> Vec<System> {
    let n = need.len();
    let nat = |m: usize| need[m] * bq as f32;
    let mut out: Vec<System> = Vec::new();
    let mut m = 0usize;
    while m < n {
        let avail = if out.is_empty() { first_w } else { w };
        let mut sum = nat(m);
        let mut k = 1usize;
        while m + k < n && (k as i64) < MAX_BARS && sum + nat(m + k) <= avail {
            sum += nat(m + k);
            k += 1;
        }
        // Le dernier systeme, s'il est court, garde presque sa largeur
        // naturelle (un peu d'air en plus).
        let short = m + k >= n && sum * 1.2 < avail * 0.75;
        let f = if short { 1.2 } else { avail / sum.max(1e-3) };
        out.push(System {
            m0: m as i64,
            m1: (m + k) as i64,
            widths: (m..m + k).map(|j| nat(j) * f).collect(),
        });
        m += k;
    }
    out
}

/// Les couleurs d'une page : noir sur blanc.
fn paper_theme() -> Theme {
    let mut th = Theme::dark(S);
    th.bg = hex(0xFFFFFF);
    th.bg_alt = th.bg;
    th.panel = th.bg;
    th.text = hex(0x000000);
    th.text_dim = hex(0x141414);
    th.text_faint = hex(0x6E6E6E);
    th.accent = th.text;
    th
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// Le titre : le nom de l'item, sans l'extension de son fichier.
fn title_of(name: &str) -> String {
    let t = name.trim();
    let low = t.to_ascii_lowercase();
    for ext in [
        ".wav", ".mp3", ".flac", ".aif", ".aiff", ".ogg", ".m4a", ".wv",
    ] {
        if low.ends_with(ext) {
            return t[..t.len() - ext.len()].trim().to_string();
        }
    }
    t.to_string()
}

/// Les hauteurs d'un systeme, depuis son haut : la portee, la tab, le bas.
#[derive(Clone, Copy, Debug)]
struct Rows {
    staff: Option<f32>,
    tab: Option<f32>,
    h: f32,
}

fn rows(score: bool, tab: bool, nstr: usize) -> Rows {
    let mut y = s(30.0);
    let staff = score.then_some(y);
    if score {
        y += 4.0 * s(GAP) + s(34.0);
    }
    let tab_top = tab.then(|| {
        if score {
            y += s(20.0);
        }
        y
    });
    if tab {
        y += (nstr.max(1) - 1) as f32 * s(TAB_GAP) + s(14.0);
    }
    Rows {
        staff,
        tab: tab_top,
        h: y,
    }
}

/// Une guitare entiere en une partition : ses items transcrits, bout a
/// bout (un morceau fait de plusieurs prises). Leurs notes sont en noires du
/// projet : elles se rejoignent telles quelles, un trou entre deux items
/// devient des silences. L'origine, la mesure, la grille, le tempo et
/// l'accord sont ceux du premier ; le nom, celui de la piste.
pub fn whole_track(tr: &Track) -> Option<Item> {
    let mut items: Vec<&Item> = tr.items.iter().filter(|it| !it.doc.is_empty()).collect();
    items.sort_by(|a, b| a.qn0.total_cmp(&b.qn0));
    let first = *items.first()?;
    let last = *items.last()?;
    let mut doc: Vec<cp_tab::doc::TNote> =
        items.iter().flat_map(|it| it.doc.iter().cloned()).collect();
    cp_tab::doc::sort(&mut doc);
    let mut it = Item {
        guid: first.guid.clone(),
        take: first.take.clone(),
        name: tr.head.name.clone(),
        qn0: first.qn0,
        qn1: last.qn1,
        origin_qn: first.origin_qn,
        sig: first.sig,
        bpm: first.bpm,
        detect: first.detect.clone(),
        dropped: items.iter().map(|it| it.dropped).sum(),
        ..Item::default()
    };
    it.set_doc(doc);
    Some(it)
}

/// Ce que la mise en pages a produit.
pub struct Printed {
    pub pdf: Vec<u8>,
    pub pages: usize,
    pub systems: usize,
    pub measures: i64,
}

/// Les pages d'un item, en PDF. `lanes` : les couches que sa piste montre
/// (la partition, la tab ; les deux si elle n'en montre aucune).
pub fn pdf(item: &Item, lanes: Lanes, text: &mut TextCache) -> Printed {
    let mut pages: Vec<Page> = Vec::new();
    let title = title_of(&item.name);
    let (n, sys) = render(item, lanes, text, &mut |surf| pages.push(Page::of(surf)));
    Printed {
        pdf: pdf::write(&pages, pdf::A4_PT, &title),
        pages: pages.len(),
        systems: sys,
        measures: n,
    }
}

/// Dessine les pages une a une ; `page` recoit chacune des qu'elle est
/// finie. Rend (mesures, systemes).
pub(crate) fn render(
    item: &Item,
    lanes: Lanes,
    text: &mut TextCache,
    page: &mut dyn FnMut(&Surface),
) -> (i64, usize) {
    let th = paper_theme();
    let mut it = item.clone();
    // Les fantomes sont des propositions : pas sur papier.
    it.detect.ghosts = false;
    let eng = it.engraved().clone();
    let tun = it.detect.tuning();
    let nstr = tun.strings.len();
    let (mut score, mut tab) = (lanes.on(Lane::Score), lanes.on(Lane::Tab));
    if !score && !tab {
        (score, tab) = (true, true);
    }
    let r = rows(score, tab, nstr);
    let bq = eng.mlen.max(1) as f64 / eng.grid.max(1) as f64;
    let x0 = mm(MARGIN_X);
    let cw = PAGE_W as f32 - 2.0 * x0;
    let need = need_ppq(&eng);
    let (first_w, w) = avail();
    let sys = systems(&need, bq, first_w, w);
    let bottom = PAGE_H as f32 - mm(MARGIN_BOTTOM);

    // Combien de pages : les systemes empiles, la premiere sous son en-tete.
    let head_h = s(64.0);
    let sys_gap = s(14.0);
    let mut breaks: Vec<usize> = vec![0];
    let mut y = mm(MARGIN_TOP) + head_h;
    for k in 0..sys.len() {
        if y + r.h > bottom && k > *breaks.last().unwrap_or(&0) {
            breaks.push(k);
            y = mm(MARGIN_TOP);
        }
        y += r.h + sys_gap;
    }
    let npages = breaks.len();

    let mut surf = Surface::new(PAGE_W, PAGE_H);
    let mut scratch = Scratch::default();
    for (p, &k0) in breaks.iter().enumerate() {
        let k1 = breaks.get(p + 1).copied().unwrap_or(sys.len());
        let mut cv = Canvas::new(&mut surf, &mut scratch);
        cv.clear(th.bg);
        let mut y = mm(MARGIN_TOP);
        if p == 0 {
            draw_header(&mut cv, text, &th, &it, &eng, x0, y, cw);
            y += head_h;
        }
        for (k, sy) in sys.iter().enumerate().take(k1).skip(k0) {
            let last = k + 1 == sys.len();
            draw_system(
                &mut cv,
                text,
                &th,
                &mut it,
                &eng,
                sy,
                k == 0,
                last,
                x0,
                y,
                r,
                nstr,
            );
            y += r.h + sys_gap;
        }
        if npages > 1 {
            text.draw_in(
                &mut cv,
                Font::new(0, s(8.5) as u16, 400),
                0.0,
                PAGE_H as f32 - mm(10.0),
                PAGE_W as f32,
                s(12.0),
                &format!("{} / {npages}", p + 1),
                th.text_faint,
                Align::Center,
            );
        }
        drop(cv);
        page(&surf);
    }
    (eng.measures, sys.len())
}

/// Le titre, le tempo, l'accord.
fn draw_header(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &Item,
    eng: &Engraved,
    x0: f32,
    y: f32,
    cw: f32,
) {
    let ink = th.text;
    text.draw_in(
        cv,
        Font::new(0, s(21.0) as u16, 700),
        x0,
        y,
        cw,
        s(30.0),
        &title_of(&it.name),
        ink,
        Align::Center,
    );
    let yi = y + s(38.0);
    let fi = Font::new(0, s(10.0) as u16, 400);
    // Le tempo : une noire, puis sa valeur.
    let fq = Font::new(3, s(15.0) as u16, 400);
    let note = '\u{2669}'.to_string();
    let nw = text.measure(fq, &note);
    text.draw_in(
        cv,
        fq,
        x0,
        yi - s(3.0),
        nw,
        s(18.0),
        &note,
        ink,
        Align::Left,
    );
    if it.bpm > 0.0 {
        let bpm = if (it.bpm - it.bpm.round()).abs() < 0.05 {
            format!("{}", it.bpm.round() as i64)
        } else {
            format!("{:.1}", it.bpm)
        };
        text.draw_in(
            cv,
            fi,
            x0 + nw + s(3.0),
            yi,
            s(120.0),
            s(14.0),
            &format!("= {bpm}"),
            ink,
            Align::Left,
        );
    }
    // L'accord, de la plus grave a la plus aigue.
    let tun = it.detect.tuning();
    let names: Vec<&str> = tun
        .strings
        .iter()
        .map(|&p| NAMES[p.rem_euclid(12) as usize])
        .collect();
    text.draw_in(
        cv,
        fi,
        x0,
        yi,
        cw,
        s(14.0),
        &format!("Tuning: {}", names.join(" ")),
        ink,
        Align::Right,
    );
    let _ = eng;
}

/// Un systeme : ses portees, ses barres, puis les notes (le dessin de la
/// vue, sur l'axe du systeme).
#[allow(clippy::too_many_arguments)]
fn draw_system(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    th: &Theme,
    it: &mut Item,
    eng: &Engraved,
    sy: &System,
    first: bool,
    last: bool,
    x0: f32,
    y: f32,
    r: Rows,
    nstr: usize,
) {
    let ink = th.text;
    let line = s(0.5);
    let bar_w = s(0.6);
    let head = s(HEAD) + if first { s(HEAD_SIG) } else { 0.0 };
    let sx = x0 + head;
    let width = sy.width();
    let xr = sx + width;
    let bq = eng.mlen.max(1) as f64 / eng.grid.max(1) as f64;
    let span = ((sy.m1 - sy.m0) as f64 * bq).max(1e-6);
    let pad = s(6.0);
    // L'axe passe par chaque barre de mesure.
    let mut pts = vec![(eng.origin_qn + sy.m0 as f64 * bq, sx)];
    let mut x = sx;
    for (j, w) in sy.widths.iter().enumerate() {
        x += w;
        pts.push((eng.origin_qn + (sy.m0 + j as i64 + 1) as f64 * bq, x));
    }
    let geo = Geo {
        sx,
        scroll: eng.origin_qn + sy.m0 as f64 * bq,
        ppq: (width as f64 / span) as f32,
        s: S,
        // Seules les notes du systeme : celles de la mesure voisine sont
        // sur la ligne d'avant ou d'apres.
        clip: (sx + pad - 0.5, 0.0, width, PAGE_H as f32),
        pad,
        lw: s(0.65),
        margin: 0.0,
        warp: Warp::new(&pts),
    };
    let gap = s(GAP);
    let tg = s(TAB_GAP);
    // Les staves : (haut, bas).
    let staff = r.staff.map(|o| (y + o, y + o + 4.0 * gap));
    let tabs = r
        .tab
        .map(|o| (y + o, y + o + (nstr.max(1) - 1) as f32 * tg));
    let (sig_n, sig_d) = eng.sig;
    if let Some((top, _)) = staff {
        for k in 0..5 {
            cv.fill_rect(x0, top + k as f32 * gap - line * 0.5, xr - x0, line, ink);
        }
        // La cle de sol, l'octave en dessous (la guitare sonne une octave
        // plus bas qu'elle ne s'ecrit).
        text.draw(
            cv,
            Font::new(3, s(40.0) as u16, 400),
            x0 + s(3.0),
            top + 4.0 * gap + s(8.0),
            &draw::CLEF.to_string(),
            ink,
        );
        text.draw(
            cv,
            Font::new(0, s(8.0) as u16, 700),
            x0 + s(12.0),
            top + 4.0 * gap + s(19.0),
            "8",
            ink,
        );
        if first {
            time_sig(
                cv,
                text,
                ink,
                x0 + s(HEAD) - s(2.0),
                top,
                2.0 * gap,
                sig_n,
                sig_d,
            );
        }
    }
    if let Some((top, _)) = tabs {
        for k in 0..nstr {
            cv.fill_rect(x0, top + k as f32 * tg - line * 0.5, xr - x0, line, ink);
        }
        let ft = Font::new(0, s(11.0) as u16, 700);
        let h = (nstr.max(2) - 1) as f32 * tg;
        for (k, c) in ["T", "A", "B"].iter().enumerate() {
            let cy = top + h * (0.2 + 0.3 * k as f32);
            cv.fill_rect(x0 + s(5.0), cy - s(5.5), s(12.0), s(11.0), th.bg);
            text.draw_in(
                cv,
                ft,
                x0 + s(3.0),
                cy - s(7.0),
                s(16.0),
                s(14.0),
                c,
                ink,
                Align::Center,
            );
        }
        if first && staff.is_none() {
            time_sig(
                cv,
                text,
                ink,
                x0 + s(HEAD) - s(2.0),
                top,
                h * 0.5,
                sig_n,
                sig_d,
            );
        }
    }
    // Le trait qui ouvre le systeme, d'une portee a l'autre.
    let top = staff.or(tabs).map_or(y, |v| v.0);
    let bot = tabs.or(staff).map_or(y, |v| v.1);
    cv.fill_rect(x0, top, bar_w, bot - top, ink);
    // Les barres de mesure ; la double barre a la fin du morceau.
    for m in sy.m0 + 1..=sy.m1 {
        let x = geo.x(eng.origin_qn + m as f64 * bq).min(xr);
        for (a, b) in [staff, tabs].into_iter().flatten() {
            if m == sy.m1 && last {
                cv.fill_rect(xr - s(2.4), a, s(2.4), b - a, ink);
                cv.fill_rect(xr - s(5.4), a, bar_w, b - a, ink);
            } else {
                cv.fill_rect(x - bar_w, a, bar_w, b - a, ink);
            }
        }
    }
    // Le numero de la premiere mesure (des la deuxieme ligne).
    if !first {
        text.draw_in(
            cv,
            Font::new(0, s(8.0) as u16, 400),
            x0,
            top - s(19.0),
            s(40.0),
            s(11.0),
            &(sy.m0 + 1).to_string(),
            th.text_faint,
            Align::Left,
        );
    }
    // Les notes, rognees au systeme (une glisse ou un bend qui en sort est
    // coupe au bord, comme sur une partition).
    let mut none: Vec<HBox> = Vec::new();
    let clip = (sx - s(14.0), 0.0, xr - sx + s(14.5), PAGE_H as f32);
    if cv.push_clip(clip.0, clip.1, clip.2, clip.3) {
        if let Some((top, _)) = staff {
            draw::draw_score(
                cv,
                text,
                th,
                it,
                0,
                0,
                geo,
                top,
                None,
                Hit::None,
                &mut none,
                clip,
            );
        }
        if let Some((top, _)) = tabs {
            draw::draw_tab(
                cv,
                text,
                th,
                it,
                0,
                0,
                geo,
                top,
                tg,
                None,
                Hit::None,
                &mut none,
                clip,
            );
        }
    }
    cv.pop_clip();
}

/// La mesure (4 sur 4) : deux chiffres l'un sur l'autre, chacun sur `half`
/// de hauteur.
#[allow(clippy::too_many_arguments)]
fn time_sig(
    cv: &mut Canvas<'_>,
    text: &mut TextCache,
    ink: Color,
    x: f32,
    top: f32,
    half: f32,
    n: i32,
    d: i32,
) {
    let f = Font::new(0, (half * 1.35) as u16, 700);
    let w = s(16.0);
    for (k, v) in [n, d].into_iter().enumerate() {
        text.draw_in(
            cv,
            f,
            x,
            top + k as f32 * half - half * 0.12,
            w,
            half,
            &v.to_string(),
            ink,
            Align::Center,
        );
    }
}
