//! La partition en MusicXML (l'export) : une partie, deux portees -- la
//! portee standard (clef de sol octaviee, les hauteurs REELLES) et la
//! tablature (6 lignes, l'accordage), ou chaque note porte sa corde et sa
//! case. Le rythme, les accords et les mesures viennent de [`engrave`] : la
//! fenetre de Guitar Tab dessine la meme partition.
//!
//! Les LIES : un marteau ou un tire ([`Tech`]) s'ecrit comme une liaison sur
//! la portee, et « H » ou « P » sur la tablature ; une glisse, un trait
//! (`<slide>`). Un bend s'ecrit sur la tablature (`<bend>`, et son
//! relachement), une glisse d'approche comme un « scoop » (d'en dessous) ou
//! un « plop » (d'au-dessus), ce que MuseScore lit comme ses glisses
//! d'entree.

use std::fmt::Write as _;

use crate::doc::{self, TNote, Tech};
use crate::engrave::engrave;
use crate::fret::Tuning;

/// Une note detectee, en noires du projet.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TabNote {
    pub qn0: f64,
    pub qn1: f64,
    pub pitch: i32,
    /// Jouee sans attaque (liee a la precedente).
    pub legato: bool,
    pub vel: u8,
    /// Arrivee en glissant depuis la note precedente (meme corde).
    pub slid: bool,
    /// La hauteur jouee au fil de la note (voir [`TNote::bend`]).
    pub bend: Vec<(f64, f32)>,
    /// La hauteur ondule (un vibrato).
    pub vibrato: bool,
}

#[derive(Clone, Debug)]
pub struct ScoreSpec {
    pub title: String,
    /// La barre de mesure ou commence la partition, en noires du projet.
    pub origin_qn: f64,
    /// (temps par mesure, valeur du temps).
    pub sig: (i32, i32),
    pub bpm: f64,
    /// Cases de la grille par noire.
    pub grid: u32,
    pub tuning: Tuning,
}

/// Ce que l'ecriture a fait.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub notes: usize,
    pub chords: usize,
    pub measures: usize,
    /// Notes qui ne se jouent pas (trop de notes ensemble) : absentes.
    pub dropped: usize,
    /// Lies ecrits (marteaux et tires).
    pub legato: usize,
}

const NAMES: [(&str, i32); 12] = [
    ("C", 0),
    ("C", 1),
    ("D", 0),
    ("D", 1),
    ("E", 0),
    ("F", 0),
    ("F", 1),
    ("G", 0),
    ("G", 1),
    ("A", 0),
    ("A", 1),
    ("B", 0),
];

/// Une hauteur nommee dans la tonalite (`fifths` : l'armure).
fn pitch_xml(p: i32, fifths: i32) -> String {
    let s = crate::key::spell(p, fifths);
    let step = crate::key::STEPS[s.step as usize];
    let oct = s.octave;
    if s.alter != 0 {
        format!(
            "<pitch><step>{step}</step><alter>{}</alter><octave>{oct}</octave></pitch>",
            s.alter
        )
    } else {
        format!("<pitch><step>{step}</step><octave>{oct}</octave></pitch>")
    }
}

/// Un nom d'accord en MusicXML (`<harmony>`).
fn harmony_xml(c: &crate::chord::Chord, fifths: i32) -> String {
    let step_alter = |pc: i32| {
        let s = crate::key::spell(pc + 60, fifths);
        (crate::key::STEPS[s.step as usize], s.alter)
    };
    let (kind, text) = c.kind();
    let (rs, ra) = step_alter(c.root);
    let mut x = format!("      <harmony><root><root-step>{rs}</root-step>");
    if ra != 0 {
        let _ = write!(x, "<root-alter>{ra}</root-alter>");
    }
    let _ = write!(x, "</root><kind text=\"{text}\">{kind}</kind>");
    if c.bass != c.root {
        let (bs, ba) = step_alter(c.bass);
        let _ = write!(x, "<bass><bass-step>{bs}</bass-step>");
        if ba != 0 {
            let _ = write!(x, "<bass-alter>{ba}</bass-alter>");
        }
        x += "</bass>";
    }
    x += "</harmony>\n";
    x
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Des notes detectees a la partition : le doigte ([`doc::finger`]), puis
/// l'ecriture. Rend (MusicXML, ce qui a ete fait).
pub fn musicxml(notes: &[TabNote], spec: &ScoreSpec) -> (String, Stats) {
    let (tab, dropped) = doc::finger(notes, spec.grid, spec.bpm, &spec.tuning);
    write_musicxml(&tab, spec, dropped)
}

/// Ecrit une tablature (deja doigtee) en MusicXML.
pub fn write_musicxml(notes: &[TNote], spec: &ScoreSpec, dropped: usize) -> (String, Stats) {
    let eng = engrave(notes, spec.origin_qn, spec.sig, spec.grid);
    let tuning = &spec.tuning;
    let stats = Stats {
        notes: eng.notes,
        chords: eng.chords,
        measures: eng.measures as usize,
        dropped,
        legato: eng.legato,
    };
    let g = eng.grid;
    let mlen = eng.mlen;
    let mut x = String::new();
    x += "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n";
    x += "<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n";
    x += "<score-partwise version=\"4.0\">\n";
    let _ = writeln!(
        x,
        "  <work><work-title>{}</work-title></work>",
        esc(&spec.title)
    );
    x += "  <identification><encoding><software>CP Studio - Guitar Tab</software></encoding></identification>\n";
    x += "  <part-list><score-part id=\"P1\"><part-name>Guitar</part-name></score-part></part-list>\n";
    x += "  <part id=\"P1\">\n";
    let nstr = tuning.strings.len();
    // Un passage P.M. ou let ring en cours (le mot n'est ecrit qu'a son
    // debut).
    let (mut pm_on, mut lr_on) = (false, false);
    for m in 0..eng.measures {
        let _ = writeln!(x, "    <measure number=\"{}\">", m + 1);
        if m == 0 {
            x += "      <attributes>\n";
            let _ = writeln!(x, "        <divisions>{g}</divisions>");
            let _ = writeln!(
                x,
                "        <key><fifths>{}</fifths><mode>{}</mode></key>",
                eng.key.fifths,
                if eng.key.minor { "minor" } else { "major" }
            );
            let _ = writeln!(
                x,
                "        <time><beats>{}</beats><beat-type>{}</beat-type></time>",
                spec.sig.0, spec.sig.1
            );
            x += "        <staves>2</staves>\n";
            x += "        <clef number=\"1\"><sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change></clef>\n";
            x += "        <clef number=\"2\"><sign>TAB</sign><line>5</line></clef>\n";
            x += "        <staff-details number=\"2\">\n";
            let _ = writeln!(x, "          <staff-lines>{nstr}</staff-lines>");
            for (i, &open) in tuning.strings.iter().enumerate() {
                let (step, alter) = NAMES[open.rem_euclid(12) as usize];
                let _ = write!(
                    x,
                    "          <staff-tuning line=\"{}\"><tuning-step>{step}</tuning-step>",
                    i + 1
                );
                if alter != 0 {
                    let _ = write!(x, "<tuning-alter>{alter}</tuning-alter>");
                }
                let _ = writeln!(
                    x,
                    "<tuning-octave>{}</tuning-octave></staff-tuning>",
                    open.div_euclid(12) - 1
                );
            }
            x += "        </staff-details>\n";
            x += "      </attributes>\n";
            let _ = writeln!(
                x,
                "      <direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>",
                spec.bpm.round(),
                spec.bpm
            );
        }
        // Portee 1 puis tablature ; dans chacune la voix 1, puis la 2 si
        // la mesure en a une (on revient au debut de la mesure entre deux).
        let two = eng.two_voices.contains(&m);
        let mut first_pass = true;
        for (staff, v) in [(1, 1u8), (1, 2), (2, 1), (2, 2)] {
            if v == 2 && !two {
                continue;
            }
            if !first_pass {
                let _ = writeln!(x, "      <backup><duration>{mlen}</duration></backup>");
            }
            first_pass = false;
            let voice = if staff == 1 { v as i32 } else { 4 + v as i32 };
            for sg in eng.signs.iter().filter(|s| s.measure == m && s.voice == v) {
                let ev = &eng.events[sg.ev];
                let d = sg.dur;
                // Le nom de l'accord, avant sa premiere note.
                if staff == 1 && v == 1 && sg.first {
                    if let Some((_, c)) = eng.harmony.iter().find(|h| h.0 == sg.start) {
                        x += &harmony_xml(c, eng.key.fifths);
                    }
                }
                // « P.M. » et « let ring » : un mot au debut de chaque
                // passage (sur la portee, voix 1).
                if staff == 1 && v == 1 && sg.first && !ev.notes.is_empty() {
                    for (bit, word, on) in [
                        (TNote::PALM, "P.M.", &mut pm_on),
                        (TNote::RING, "let ring", &mut lr_on),
                    ] {
                        let has = ev.notes.iter().any(|&i| notes[i].has(bit));
                        if has && !*on {
                            let _ = writeln!(
                                x,
                                "      <direction placement=\"below\"><direction-type><words>{word}</words></direction-type><staff>1</staff></direction>"
                            );
                        }
                        *on = has;
                    }
                }
                // Un triolet : trois dans le temps de deux, et son crochet.
                let tmod = if sg.tuplet == 3 {
                    "<time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>"
                } else {
                    ""
                };
                let mut tup = String::new();
                if sg.tup_start {
                    tup += "<tuplet type=\"start\" bracket=\"yes\"/>";
                }
                if sg.tup_stop {
                    tup += "<tuplet type=\"stop\"/>";
                }
                if ev.notes.is_empty() {
                    let _ = write!(
                        x,
                        "      <note><rest/><duration>{d}</duration><voice>{voice}</voice><type>{}</type>",
                        sg.ty
                    );
                    for _ in 0..sg.dots {
                        x += "<dot/>";
                    }
                    x += tmod;
                    let _ = write!(x, "<staff>{staff}</staff>");
                    if !tup.is_empty() {
                        let _ = write!(x, "<notations>{tup}</notations>");
                    }
                    x += "</note>\n";
                    continue;
                }
                let tag = |t: Tech| match t {
                    Tech::Hammer => "hammer-on",
                    Tech::Slide => "slide",
                    _ => "pull-off",
                };
                let lstop = (sg.first && ev.lig_in != Tech::Plain).then(|| tag(ev.lig_in));
                let lstart = (sg.last && ev.lig_out != Tech::Plain).then(|| tag(ev.lig_out));
                for (ci, &ni) in ev.notes.iter().enumerate() {
                    let n = &notes[ni];
                    x += "      <note>";
                    if ci > 0 {
                        x += "<chord/>";
                    }
                    x += &pitch_xml(n.pitch, eng.key.fifths);
                    let _ = write!(x, "<duration>{d}</duration>");
                    if sg.tie_in {
                        x += "<tie type=\"stop\"/>";
                    }
                    if sg.tie_out {
                        x += "<tie type=\"start\"/>";
                    }
                    let _ = write!(x, "<voice>{voice}</voice><type>{}</type>", sg.ty);
                    for _ in 0..sg.dots {
                        x += "<dot/>";
                    }
                    x += tmod;
                    if n.has(TNote::DEAD) {
                        x += "<notehead>x</notehead>";
                    } else if n.has(TNote::HARMONIC) {
                        x += "<notehead>diamond</notehead>";
                    }
                    let _ = write!(x, "<staff>{staff}</staff>");
                    let mut nots = String::new();
                    if ci == 0 {
                        nots += &tup;
                    }
                    if staff == 1 && sg.first && n.has(TNote::VIBRATO) {
                        nots += "<ornaments><wavy-line type=\"start\" number=\"1\"/><wavy-line type=\"stop\" number=\"1\"/></ornaments>";
                    }
                    if sg.tie_in {
                        nots += "<tied type=\"stop\"/>";
                    }
                    if sg.tie_out {
                        nots += "<tied type=\"start\"/>";
                    }
                    match lstop {
                        Some("slide") => nots += "<slide type=\"stop\" number=\"1\"/>",
                        Some(_) => nots += "<slur type=\"stop\" number=\"1\"/>",
                        None => {}
                    }
                    match lstart {
                        Some("slide") => {
                            nots += "<slide type=\"start\" number=\"1\" line-type=\"solid\"/>"
                        }
                        Some(_) => nots += "<slur type=\"start\" number=\"1\"/>",
                        None => {}
                    }
                    if staff == 1 && sg.first && !sg.tie_in {
                        match n.slide_in() {
                            0 => {}
                            k if k < 0 => nots += "<articulations><scoop/></articulations>",
                            _ => nots += "<articulations><plop/></articulations>",
                        }
                    }
                    if staff == 2 {
                        let mut tech = format!(
                            "<string>{}</string><fret>{}</fret>",
                            nstr - n.string,
                            n.fret
                        );
                        let mark = |k: &str| if k == "hammer-on" { "H" } else { "P" };
                        if let Some(k) = lstop.filter(|&k| k != "slide") {
                            let _ = write!(tech, "<{k} number=\"1\" type=\"stop\"/>");
                        }
                        if let Some(k) = lstart.filter(|&k| k != "slide") {
                            let _ =
                                write!(tech, "<{k} number=\"1\" type=\"start\">{}</{k}>", mark(k));
                        }
                        if n.has(TNote::HARMONIC) {
                            tech += "<harmonic><natural/></harmonic>";
                        }
                        if let Some(b) = n.bend_mark().filter(|_| sg.first) {
                            let _ =
                                write!(tech, "<bend><bend-alter>{}</bend-alter></bend>", b.amount);
                            if b.release.is_some() {
                                let _ = write!(
                                    tech,
                                    "<bend><bend-alter>{}</bend-alter><release/></bend>",
                                    -b.amount
                                );
                            }
                        }
                        let _ = write!(nots, "<technical>{tech}</technical>");
                    }
                    if !nots.is_empty() {
                        let _ = write!(x, "<notations>{nots}</notations>");
                    }
                    x += "</note>\n";
                }
            }
        }
        x += "    </measure>\n";
    }
    x += "  </part>\n</score-partwise>\n";
    (x, stats)
}
