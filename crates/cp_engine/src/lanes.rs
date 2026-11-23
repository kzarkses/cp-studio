//! Les lanes MIDI : un clip de notes par port, joue DANS le fil audio et date
//! a l'echantillon (la Session).
//!
//! Le moteur ne connait que l'echantillon : un clip est lance a un frame,
//! avec un nombre de frames par noire (`fpb`). Le fil principal tient la
//! carte de tempo ; quand le tempo change, il reposte `fpb` et la lane se
//! re-ancre la ou elle est (les notes en cours gardent leur fin en noires).
//!
//! Pourquoi dans le fil audio : une boucle qui tourne ne doit dependre d'aucun
//! battement du fil principal (une boite modale l'arrete, une note se
//! perdrait). Le fil principal ne fait que lancer, arreter, remplacer les
//! notes.
//!
//! Les memes regles que les voix : rien n'est alloue ni libere ici. Un clip
//! voyage dans la commande qui le lance (`Arc`), et le fil principal en garde
//! toujours une reference ([`crate::Ctl::lanes_gc`]) : la lane qui lache un
//! clip n'est jamais la derniere a le tenir.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering::*};

use crate::types::Frame;

/// Une note d'un clip, en noires depuis le debut du clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MNote {
    pub start: f64,
    pub len: f64,
    /// 0..15.
    pub chan: u8,
    pub pitch: u8,
    pub vel: u8,
}

/// Un clip MIDI tel que la lane le joue. Les notes sont triees par debut.
#[derive(Clone, Debug, Default)]
pub struct MidiClip {
    /// L'identite du clip (la case qui le porte) : une edition remplace les
    /// notes d'un clip qui joue sans le relancer.
    pub id: u64,
    pub notes: Vec<MNote>,
    /// La longueur du clip, en noires.
    pub len: f64,
    /// L'accolade : `[loop_a, loop_b)` ; `loop_b <= loop_a` = le clip entier.
    pub loop_a: f64,
    pub loop_b: f64,
    /// Faux : le clip joue une fois, jusqu'a `len`, puis la lane se tait.
    pub looped: bool,
}

impl MidiClip {
    /// Trie les notes (la lane cherche par dichotomie) et borne ce qui doit
    /// l'etre. A appeler avant d'envoyer un clip.
    pub fn normalized(mut self) -> MidiClip {
        self.notes
            .retain(|n| n.start.is_finite() && n.len.is_finite() && n.len > 0.0);
        for n in &mut self.notes {
            n.chan &= 0x0F;
            n.pitch &= 0x7F;
            n.vel = n.vel.clamp(1, 127);
            if n.start < 0.0 {
                n.start = 0.0;
            }
        }
        self.notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        if !(self.len.is_finite() && self.len > 0.0) {
            self.len = self
                .notes
                .iter()
                .map(|n| n.start + n.len)
                .fold(4.0, f64::max);
        }
        self
    }

    /// La zone qui boucle : l'accolade si elle tient dans le clip, sinon le
    /// clip entier.
    pub fn span(&self) -> (f64, f64) {
        let a = self.loop_a.max(0.0);
        let b = self.loop_b.min(self.len);
        if b > a + 1e-9 {
            (a, b)
        } else {
            (0.0, self.len)
        }
    }
}

/// Ce qu'on demande a la lane d'un port.
#[derive(Clone, Debug)]
pub enum LaneOp {
    /// Joue `clip` a partir de la position `from` (noires du clip), `fpb`
    /// frames par noire. Remplace ce qui jouait, a ce frame exact.
    Play {
        clip: Arc<MidiClip>,
        from: f64,
        fpb: f64,
    },
    /// Se tait (les notes tenues relachees a ce frame).
    Stop,
    /// Un autre tempo, a partir de ce frame.
    Rate { fpb: f64 },
    /// Un message court, envoye tel quel (une note jouee a la main, un CC).
    Msg { len: u8, msg: [u8; 3] },
}

/// Ce que le fil principal lit d'une lane.
pub struct LanePub {
    /// 0 : rien ; 1 : un clip attend son frame ; 2 : il joue.
    pub state: AtomicI32,
    /// Le clip qui joue (ou attend).
    pub clip: AtomicU64,
    /// La position dans le clip a la fin du dernier bloc (bits d'un f64).
    pub pos: AtomicU64,
    /// Les noires jouees depuis le lancement (bits d'un f64).
    pub beats: AtomicU64,
    /// Le frame de la fin du dernier bloc.
    pub frame: AtomicI64,
    /// Le frame du lancement du clip qui joue.
    pub started: AtomicI64,
    /// Messages perdus (le bloc en avait trop, ou la file etait pleine).
    pub dropped: AtomicU32,
}

impl LanePub {
    pub(crate) fn new() -> LanePub {
        LanePub {
            state: AtomicI32::new(0),
            clip: AtomicU64::new(0),
            pos: AtomicU64::new(0f64.to_bits()),
            beats: AtomicU64::new(0f64.to_bits()),
            frame: AtomicI64::new(0),
            started: AtomicI64::new(-1),
            dropped: AtomicU32::new(0),
        }
    }
}

/// Ce qu'une lane publie, lu par le fil principal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSnap {
    pub state: i32,
    pub clip: u64,
    pub pos: f64,
    pub beats: f64,
    pub frame: Frame,
    pub started: Frame,
    pub dropped: u32,
}

impl LanePub {
    pub fn snap(&self) -> LaneSnap {
        LaneSnap {
            state: self.state.load(Acquire),
            clip: self.clip.load(Acquire),
            pos: f64::from_bits(self.pos.load(Acquire)),
            beats: f64::from_bits(self.beats.load(Acquire)),
            frame: self.frame.load(Acquire),
            started: self.started.load(Acquire),
            dropped: self.dropped.load(Relaxed),
        }
    }
}

/// Ou vont les messages : la liste MIDI du bloc que REAPER tire.
pub trait MidiSink {
    /// `offset` : le frame dans le bloc. Faux si la liste refuse.
    fn push(&mut self, offset: i32, msg: &[u8]) -> bool;
}

/// Notes tenues au plus par lane.
const MAX_ACTIVE: usize = 128;
/// Commandes datees en attente par lane.
const MAX_PENDING: usize = 32;
/// Messages au plus par bloc (un bloc de 512 frames).
const MAX_EVENTS: usize = 512;

#[derive(Clone, Copy)]
struct Active {
    chan: u8,
    pitch: u8,
    /// La fin, en noires depuis le lancement.
    u_off: f64,
    /// Le debut de la note dans le clip (pour la reconnaitre apres une
    /// edition).
    start: f64,
}

struct Pending {
    at: Frame,
    op: LaneOp,
}

#[derive(Clone, Copy)]
struct Ev {
    off: i32,
    /// 0 : une fin de note, 1 : un message, 2 : un debut de note (a frame
    /// egal, les fins d'abord).
    kind: u8,
    len: u8,
    msg: [u8; 3],
}

pub(crate) struct Lane {
    clip: Option<Arc<MidiClip>>,
    /// La position du lancement dans le clip.
    from: f64,
    fpb: f64,
    /// L'ancre : au frame `anchor_f`, `anchor_u` noires depuis le lancement.
    anchor_f: Frame,
    anchor_u: f64,
    started: Frame,
    /// Le clip a fini (une fois, sans boucle).
    done: bool,
    active: [Active; MAX_ACTIVE],
    nactive: usize,
    pending: Vec<Pending>,
    evs: Vec<Ev>,
}

impl Lane {
    pub(crate) fn new() -> Lane {
        Lane {
            clip: None,
            from: 0.0,
            fpb: 24000.0,
            anchor_f: 0,
            anchor_u: 0.0,
            started: -1,
            done: false,
            active: [Active {
                chan: 0,
                pitch: 0,
                u_off: 0.0,
                start: 0.0,
            }; MAX_ACTIVE],
            nactive: 0,
            // Reserves une fois, ici, sur le fil principal.
            pending: Vec::with_capacity(MAX_PENDING),
            evs: Vec::with_capacity(MAX_EVENTS),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.clip = None;
        self.nactive = 0;
        self.pending.clear();
        self.evs.clear();
        self.done = false;
        self.started = -1;
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.clip.is_none() && self.nactive == 0 && self.pending.is_empty()
    }

    /// Une commande datee (fil audio, au drainage). Faux si la file est
    /// pleine.
    pub(crate) fn post(&mut self, at: Frame, op: LaneOp) -> bool {
        if self.pending.len() >= MAX_PENDING {
            return false;
        }
        // Inseree a sa place : la file reste triee par frame, et deux
        // commandes au meme frame gardent leur ordre d'arrivee.
        let i = self.pending.partition_point(|p| p.at <= at);
        self.pending.insert(i, Pending { at, op });
        true
    }

    /// Les notes d'un clip qui joue, remplacees sans relance (une edition).
    /// Une note tenue qui n'existe plus est relachee au debut du bloc.
    pub(crate) fn update(&mut self, clip: Arc<MidiClip>) {
        let Some(cur) = &self.clip else { return };
        if cur.id != clip.id {
            return;
        }
        let mut k = 0;
        while k < self.nactive {
            let a = self.active[k];
            let kept = clip.notes.iter().any(|n| {
                n.chan == a.chan && n.pitch == a.pitch && (n.start - a.start).abs() < 1e-6
            });
            if kept {
                k += 1;
            } else {
                push_ev(&mut self.evs, 0, 0, 3, [0x80 | a.chan, a.pitch, 0]);
                self.active[k] = self.active[self.nactive - 1];
                self.nactive -= 1;
            }
        }
        self.clip = Some(clip);
    }

    /// Les noires depuis le lancement, au frame `f`.
    fn u_at(&self, f: Frame) -> f64 {
        self.anchor_u + (f - self.anchor_f) as f64 / self.fpb
    }

    fn release_all(&mut self, off: i32) {
        for k in 0..self.nactive {
            let a = self.active[k];
            push_ev(&mut self.evs, off, 0, 3, [0x80 | a.chan, a.pitch, 0]);
        }
        self.nactive = 0;
    }

    /// Le bloc `[f0, f0 + n)` : les commandes a leur frame, les notes, puis
    /// les messages vers `sink` dans l'ordre du temps.
    pub(crate) fn render(
        &mut self,
        f0: Frame,
        n: usize,
        sink: Option<&mut dyn MidiSink>,
        p: &LanePub,
    ) {
        let f1 = f0 + n as Frame;
        let mut t = f0;
        loop {
            let next = self.pending.first().map(|q| q.at.max(t));
            let seg_end = match next {
                Some(at) if at < f1 => at,
                _ => f1,
            };
            if seg_end > t {
                self.play_segment(f0, t, seg_end);
            }
            t = seg_end;
            match next {
                Some(at) if at < f1 => {
                    let q = self.pending.remove(0);
                    self.apply(f0, at, q.op);
                }
                _ => break,
            }
        }

        // --- vers REAPER, dans l'ordre du temps (fins avant debuts)
        let evs = &mut self.evs;
        // Tri par insertion : le bloc en porte peu, et c'est stable.
        for i in 1..evs.len() {
            let e = evs[i];
            let mut j = i;
            while j > 0 && (evs[j - 1].off, evs[j - 1].kind) > (e.off, e.kind) {
                evs[j] = evs[j - 1];
                j -= 1;
            }
            evs[j] = e;
        }
        let mut lost = 0u32;
        if let Some(s) = sink {
            for e in evs.iter() {
                let off = e.off.clamp(0, n.max(1) as i32 - 1);
                if !s.push(off, &e.msg[..e.len as usize]) {
                    lost += 1;
                }
            }
        }
        evs.clear();

        // Un clip joue une fois, fini et sans note tenue : la lane se tait.
        if self.done && self.nactive == 0 && self.pending.is_empty() {
            self.clip = None;
            self.started = -1;
        }

        // --- publication
        let (state, id, pos, u) = match &self.clip {
            Some(c) => {
                let u = self.u_at(f1).max(0.0);
                let st = if self.started >= 0 && self.started < f1 {
                    2
                } else {
                    1
                };
                (st, c.id, clip_pos(c, self.from, u), u)
            }
            None => (0, 0, 0.0, 0.0),
        };
        p.clip.store(id, Release);
        p.pos.store(pos.to_bits(), Release);
        p.beats.store(u.to_bits(), Release);
        p.frame.store(f1, Release);
        p.started.store(self.started, Release);
        p.state.store(state, Release);
        if lost > 0 {
            p.dropped.fetch_add(lost, Relaxed);
        }
    }

    fn apply(&mut self, f0: Frame, at: Frame, op: LaneOp) {
        let off = (at - f0) as i32;
        match op {
            LaneOp::Play { clip, from, fpb } => {
                self.release_all(off);
                let fpb = if fpb.is_finite() && fpb > 1.0 {
                    fpb
                } else {
                    self.fpb
                };
                let (_, lb) = clip.span();
                let end = if clip.looped { lb } else { clip.len };
                self.from = if from.is_finite() && from >= 0.0 && from < end {
                    from
                } else {
                    clip.span().0.min(end)
                };
                self.clip = Some(clip);
                self.fpb = fpb;
                self.anchor_f = at;
                self.anchor_u = 0.0;
                self.started = at;
                self.done = false;
            }
            LaneOp::Stop => {
                self.release_all(off);
                self.clip = None;
                self.started = -1;
            }
            LaneOp::Rate { fpb } => {
                if fpb.is_finite() && fpb > 1.0 && self.clip.is_some() {
                    // Re-ancre la ou l'on est : rien ne saute.
                    let u = self.u_at(at);
                    self.anchor_f = at;
                    self.anchor_u = u;
                    self.fpb = fpb;
                } else if fpb.is_finite() && fpb > 1.0 {
                    self.fpb = fpb;
                }
            }
            LaneOp::Msg { len, msg } => {
                let len = len.clamp(1, 3);
                push_ev(&mut self.evs, off, 1, len, msg);
            }
        }
    }

    /// Les notes entre les frames `[a, b)` du bloc qui commence en `f0`.
    fn play_segment(&mut self, f0: Frame, a: Frame, b: Frame) {
        let Some(clip) = self.clip.clone() else {
            return;
        };
        if a < self.started {
            // Le lancement est plus loin (dans ce bloc ou apres) : rien avant.
            if b <= self.started {
                return;
            }
            return self.play_segment(f0, self.started, b);
        }
        let u0 = self.u_at(a);
        let u1 = self.u_at(b);
        if u1 <= u0 {
            return;
        }
        let fpb = self.fpb;
        let frame_of = |u: f64| -> i32 {
            let f = self.anchor_f as f64 + (u - self.anchor_u) * fpb;
            ((f.round() as Frame) - f0) as i32
        };

        // --- les fins de notes qui tombent ici
        let mut k = 0;
        while k < self.nactive {
            let ac = self.active[k];
            if ac.u_off < u1 {
                let off = frame_of(ac.u_off.max(u0));
                push_ev(&mut self.evs, off, 0, 3, [0x80 | ac.chan, ac.pitch, 0]);
                self.active[k] = self.active[self.nactive - 1];
                self.nactive -= 1;
            } else {
                k += 1;
            }
        }
        if self.done {
            return;
        }

        // --- les debuts : passe par passe dans la zone jouee
        let (la, lb) = clip.span();
        let looped = clip.looped && lb > la + 1e-9;
        let first_end = if looped { lb } else { clip.len };
        let mut seg_u = 0.0; // noires depuis le lancement au debut du morceau
        let mut seg_p = self.from; // position du clip au debut du morceau
        let mut seg_end = first_end;
        // Saute les passes deja jouees.
        let span = lb - la;
        let first_len = (first_end - self.from).max(0.0);
        if u0 >= first_len {
            if !looped {
                // Fini : plus rien ne commence.
                self.done = true;
                return;
            }
            let passes = ((u0 - first_len) / span).floor();
            seg_u = first_len + passes * span;
            seg_p = la;
            seg_end = lb;
        }
        let mut guard = 0;
        while seg_u < u1 && guard < 4096 {
            guard += 1;
            let seg_len = seg_end - seg_p;
            if seg_len <= 1e-12 {
                break;
            }
            // La part du morceau dans [u0, u1).
            let lo = u0.max(seg_u);
            let hi = u1.min(seg_u + seg_len);
            if hi > lo {
                let p_lo = seg_p + (lo - seg_u);
                let p_hi = seg_p + (hi - seg_u);
                let i0 = clip.notes.partition_point(|nt| nt.start < p_lo - 1e-9);
                for nt in &clip.notes[i0..] {
                    if nt.start >= p_hi - 1e-9 {
                        break;
                    }
                    // Une note qui commence avant le debut d'une passe
                    // n'appartient pas a cette passe.
                    if nt.start < seg_p - 1e-9 {
                        continue;
                    }
                    let u_on = seg_u + (nt.start - seg_p);
                    let end = (nt.start + nt.len).min(seg_end);
                    let u_off = u_on + (end - nt.start).max(1e-6);
                    let off = frame_of(u_on);
                    // La meme hauteur deja tenue : relachee d'abord.
                    if let Some(j) = (0..self.nactive).find(|&j| {
                        self.active[j].chan == nt.chan && self.active[j].pitch == nt.pitch
                    }) {
                        push_ev(&mut self.evs, off, 0, 3, [0x80 | nt.chan, nt.pitch, 0]);
                        self.active[j] = self.active[self.nactive - 1];
                        self.nactive -= 1;
                    }
                    if self.nactive >= MAX_ACTIVE {
                        continue;
                    }
                    push_ev(&mut self.evs, off, 2, 3, [0x90 | nt.chan, nt.pitch, nt.vel]);
                    self.active[self.nactive] = Active {
                        chan: nt.chan,
                        pitch: nt.pitch,
                        u_off,
                        start: nt.start,
                    };
                    self.nactive += 1;
                    // Une note plus courte que le bloc finit dans le bloc.
                    if u_off < u1 {
                        let k = self.nactive - 1;
                        let offf = frame_of(u_off);
                        push_ev(&mut self.evs, offf, 0, 3, [0x80 | nt.chan, nt.pitch, 0]);
                        self.active[k] = self.active[self.nactive - 1];
                        self.nactive -= 1;
                    }
                }
            }
            seg_u += seg_len;
            if !looped {
                if seg_u <= u1 {
                    self.done = true;
                }
                break;
            }
            seg_p = la;
            seg_end = lb;
        }
    }
}

fn push_ev(evs: &mut Vec<Ev>, off: i32, kind: u8, len: u8, msg: [u8; 3]) {
    // La capacite est reservee : au-dela, le message se perd (compte par
    // l'appelant comme une perte), on n'alloue jamais ici.
    if evs.len() < evs.capacity() {
        evs.push(Ev {
            off,
            kind,
            len,
            msg,
        });
    }
}

/// La position dans le clip apres `u` noires jouees depuis `from`.
pub fn clip_pos(c: &MidiClip, from: f64, u: f64) -> f64 {
    let (la, lb) = c.span();
    let looped = c.looped && lb > la + 1e-9;
    let first_end = if looped { lb } else { c.len };
    let first_len = (first_end - from).max(0.0);
    if u < first_len {
        return from + u;
    }
    if !looped {
        return c.len;
    }
    let span = lb - la;
    la + (u - first_len).rem_euclid(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rec(Vec<(Frame, Vec<u8>)>, Frame);
    impl MidiSink for Rec {
        fn push(&mut self, offset: i32, msg: &[u8]) -> bool {
            self.0.push((self.1 + offset as Frame, msg.to_vec()));
            true
        }
    }

    fn clip(notes: &[(f64, f64, u8)], len: f64) -> Arc<MidiClip> {
        Arc::new(
            MidiClip {
                id: 7,
                notes: notes
                    .iter()
                    .map(|&(s, l, p)| MNote {
                        start: s,
                        len: l,
                        chan: 0,
                        pitch: p,
                        vel: 100,
                    })
                    .collect(),
                len,
                loop_a: 0.0,
                loop_b: 0.0,
                looped: true,
            }
            .normalized(),
        )
    }

    /// Tire `blocks` blocs de `bs` frames et rend les messages dates.
    fn run(l: &mut Lane, from: Frame, blocks: usize, bs: usize) -> Vec<(Frame, Vec<u8>)> {
        let p = LanePub::new();
        let mut r = Rec(Vec::new(), 0);
        for b in 0..blocks {
            let f0 = from + (b * bs) as Frame;
            r.1 = f0;
            l.render(f0, bs, Some(&mut r), &p);
        }
        r.0
    }

    #[test]
    fn une_boucle_rend_ses_notes_a_l_echantillon() {
        // Deux notes sur une mesure de 4 noires, 1000 frames par noire.
        let mut l = Lane::new();
        let c = clip(&[(0.0, 1.0, 60), (2.0, 0.5, 64)], 4.0);
        l.post(
            100,
            LaneOp::Play {
                clip: c,
                from: 0.0,
                fpb: 1000.0,
            },
        );
        let ev = run(&mut l, 0, 100, 97); // blocs de 97 frames : rien d'aligne
        let ons: Vec<Frame> = ev.iter().filter(|e| e.1[0] == 0x90).map(|e| e.0).collect();
        let offs: Vec<Frame> = ev.iter().filter(|e| e.1[0] == 0x80).map(|e| e.0).collect();
        assert_eq!(&ons[..4], &[100, 2100, 4100, 6100]);
        assert_eq!(&offs[..4], &[1100, 2600, 5100, 6600]);
    }

    #[test]
    fn dix_minutes_sans_une_note_decalee() {
        // 120 BPM a 48 kHz : 24000 frames par noire ; 10 min = 1200 noires.
        let mut l = Lane::new();
        let c = clip(&[(0.0, 0.25, 36), (1.5, 0.25, 38)], 4.0);
        l.post(
            0,
            LaneOp::Play {
                clip: c,
                from: 0.0,
                fpb: 24000.0,
            },
        );
        let p = LanePub::new();
        let mut r = Rec(Vec::new(), 0);
        let bs = 128;
        let total = 600 * 48000;
        let mut f = 0;
        while f < total {
            r.1 = f;
            l.render(f, bs, Some(&mut r), &p);
            // Garde seulement les debuts, pour ne pas grossir.
            r.0.retain(|e| e.1[0] == 0x90);
            if let Some(e) = r.0.last() {
                let beat = e.0 as f64 / 24000.0;
                let expect = if e.1[1] == 36 { 0.0 } else { 1.5 };
                assert_eq!(beat.rem_euclid(4.0), expect, "frame {}", e.0);
                assert_eq!(e.0 % 12000, 0);
            }
            r.0.clear();
            f += bs as Frame;
        }
        assert_eq!(l.u_at(total).round(), 1200.0);
    }

    #[test]
    fn l_accolade_et_le_point_de_depart() {
        // Clip de 8 noires, accolade [4, 6) : la premiere passe part de 0
        // et va jusqu'a 6, puis boucle sur [4, 6).
        let mut l = Lane::new();
        let mut m = (*clip(&[(1.0, 0.5, 60), (4.0, 0.5, 62), (5.0, 0.5, 64)], 8.0)).clone();
        m.loop_a = 4.0;
        m.loop_b = 6.0;
        l.post(
            0,
            LaneOp::Play {
                clip: Arc::new(m),
                from: 0.0,
                fpb: 100.0,
            },
        );
        let ev = run(&mut l, 0, 20, 64);
        let ons: Vec<(Frame, u8)> = ev
            .iter()
            .filter(|e| e.1[0] == 0x90)
            .map(|e| (e.0, e.1[1]))
            .collect();
        assert_eq!(
            &ons[..6],
            &[
                (100, 60),
                (400, 62),
                (500, 64),
                (600, 62),
                (700, 64),
                (800, 62)
            ]
        );
    }

    #[test]
    fn un_clip_sans_boucle_se_tait_a_sa_fin() {
        let mut l = Lane::new();
        let mut m = (*clip(&[(0.0, 3.0, 60), (3.5, 2.0, 62)], 4.0)).clone();
        m.looped = false;
        l.post(
            0,
            LaneOp::Play {
                clip: Arc::new(m),
                from: 0.0,
                fpb: 100.0,
            },
        );
        let ev = run(&mut l, 0, 20, 50);
        let ons = ev.iter().filter(|e| e.1[0] == 0x90).count();
        assert_eq!(ons, 2);
        // La seconde note est coupee a la fin du clip (4 noires).
        let last_off = ev.iter().filter(|e| e.1[0] == 0x80).map(|e| e.0).max();
        assert_eq!(last_off, Some(400));
    }

    #[test]
    fn un_lancement_date_remplace_l_autre_au_frame_exact() {
        let mut l = Lane::new();
        let a = clip(&[(0.0, 4.0, 60)], 4.0);
        let mut bm = (*clip(&[(0.0, 1.0, 72)], 4.0)).clone();
        bm.id = 9;
        l.post(
            0,
            LaneOp::Play {
                clip: a,
                from: 0.0,
                fpb: 100.0,
            },
        );
        l.post(
            250,
            LaneOp::Play {
                clip: Arc::new(bm),
                from: 0.0,
                fpb: 100.0,
            },
        );
        let ev = run(&mut l, 0, 10, 64);
        assert_eq!(ev[0], (0, vec![0x90, 60, 100]));
        // La note tenue relachee au frame du lancement, AVANT la nouvelle.
        assert_eq!(ev[1], (250, vec![0x80, 60, 0]));
        assert_eq!(ev[2], (250, vec![0x90, 72, 100]));
    }

    #[test]
    fn un_arret_relache_tout_et_publie() {
        let mut l = Lane::new();
        l.post(
            0,
            LaneOp::Play {
                clip: clip(&[(0.0, 4.0, 60), (0.0, 4.0, 64)], 4.0),
                from: 0.0,
                fpb: 100.0,
            },
        );
        l.post(150, LaneOp::Stop);
        let p = LanePub::new();
        let mut r = Rec(Vec::new(), 0);
        l.render(0, 100, Some(&mut r), &p);
        assert_eq!(p.snap().state, 2);
        r.1 = 100;
        l.render(100, 100, Some(&mut r), &p);
        let offs: Vec<Frame> = r.0.iter().filter(|e| e.1[0] == 0x80).map(|e| e.0).collect();
        assert_eq!(offs, vec![150, 150]);
        assert_eq!(p.snap().state, 0);
        assert!(l.is_idle());
    }

    #[test]
    fn un_tempo_change_sans_saut() {
        // A 100 frames par noire, puis 200 a partir du frame 150 : la note
        // de la noire 2 tombe a 150 + 0,5 * 200 = 250.
        let mut l = Lane::new();
        l.post(
            0,
            LaneOp::Play {
                clip: clip(&[(0.0, 0.1, 60), (2.0, 0.1, 62)], 4.0),
                from: 0.0,
                fpb: 100.0,
            },
        );
        l.post(150, LaneOp::Rate { fpb: 200.0 });
        let ev = run(&mut l, 0, 10, 64);
        let on62 = ev.iter().find(|e| e.1 == vec![0x90, 62, 100]).map(|e| e.0);
        assert_eq!(on62, Some(250));
    }

    #[test]
    fn une_edition_relache_seulement_la_note_disparue() {
        let mut l = Lane::new();
        let c = clip(&[(0.0, 4.0, 60), (0.0, 4.0, 64)], 4.0);
        l.post(
            0,
            LaneOp::Play {
                clip: c.clone(),
                from: 0.0,
                fpb: 100.0,
            },
        );
        let p = LanePub::new();
        let mut r = Rec(Vec::new(), 0);
        l.render(0, 64, Some(&mut r), &p);
        let mut m = (*c).clone();
        m.notes.retain(|n| n.pitch != 64);
        l.update(Arc::new(m));
        r.0.clear();
        r.1 = 64;
        l.render(64, 64, Some(&mut r), &p);
        assert_eq!(r.0, vec![(64, vec![0x80, 64, 0])]);
    }

    #[test]
    fn un_lancement_au_milieu_d_un_bloc_et_une_position_de_depart() {
        let mut l = Lane::new();
        l.post(
            30,
            LaneOp::Play {
                clip: clip(&[(1.0, 0.5, 60), (3.0, 0.5, 62)], 4.0),
                from: 2.5,
                fpb: 10.0,
            },
        );
        let ev = run(&mut l, 0, 4, 32);
        let ons: Vec<(Frame, u8)> = ev
            .iter()
            .filter(|e| e.1[0] == 0x90)
            .map(|e| (e.0, e.1[1]))
            .collect();
        // 62 a la noire 3 (30 + 0,5 * 10 = 35), puis la boucle : 60 a 30 +
        // (1,5 + 1) * 10 = 55, 62 a 75.
        assert_eq!(&ons[..3], &[(35, 62), (55, 60), (75, 62)]);
    }

    #[test]
    fn un_message_date_part_a_son_frame() {
        let mut l = Lane::new();
        l.post(
            70,
            LaneOp::Msg {
                len: 3,
                msg: [0xB0, 64, 127],
            },
        );
        let ev = run(&mut l, 0, 2, 64);
        assert_eq!(ev, vec![(70, vec![0xB0, 64, 127])]);
    }
}
