//! La prise MIDI (A8) : ce qui est joue sur les entrees MIDI, date a
//! l'echantillon du moteur, et place la ou le musicien l'a entendu.
//!
//! Le crochet audio (passage « pre ») pose chaque message court des entrees
//! (notes, controleurs, pitch bend, pression, programme) dans un anneau, date
//! au frame du moteur (debut du bloc + son decalage), et publie jusqu'ou il
//! est alle. Le fil principal vide l'anneau dans les prises en cours. Sans
//! verrou : le fil audio n'ecrit que des atomiques.
//!
//! LA LATENCE. Le musicien joue ce qu'il ENTEND : le frame F du moteur sort
//! des enceintes une latence de sortie plus tard. Un message qui arrive au
//! frame G a donc ete joue sur G - sortie : la prise retire cet ecart. (Le
//! MIDI n'a pas de latence d'entree annoncee par la carte.)
//!
//! LES NOTES COUPEES. Une note tenue a la fin de la prise y est relachee ; un
//! relachement dont l'appui precede la prise est oublie.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, AtomicUsize, Ordering::*};

/// L'anneau : 16 384 messages (le fil principal le vide a chaque battement).
const MN: usize = 1 << 14;

/// Un message d'une prise : son frame depuis le debut de la prise, son
/// entree, ses octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiEv {
    pub frame: i64,
    pub dev: u8,
    pub len: u8,
    pub msg: [u8; 3],
}

/// Ce que le crochet ecrit et que le fil principal lit.
pub struct MidiRing {
    /// Deux cases par message : son frame, puis (octets, longueur, entree).
    ev: Box<[AtomicU64]>,
    w: AtomicUsize,
    /// Le frame du moteur jusqu'ou le crochet a lu (fin du dernier bloc).
    progress: AtomicI64,
    /// Les entrees ecoutees (leur nombre ; 0 : rien).
    on: AtomicI32,
}

/// La longueur d'un message court d'apres son statut (0 : on l'ignore).
pub fn short_len(status: u8) -> usize {
    match status & 0xF0 {
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3,
        0xC0 | 0xD0 => 2,
        _ => 0,
    }
}

impl MidiRing {
    pub fn new() -> Arc<MidiRing> {
        Arc::new(MidiRing {
            ev: (0..MN * 2).map(|_| AtomicU64::new(0)).collect(),
            w: AtomicUsize::new(0),
            progress: AtomicI64::new(i64::MIN),
            on: AtomicI32::new(0),
        })
    }

    pub fn inputs(&self) -> i32 {
        self.on.load(Relaxed)
    }
    pub fn set_inputs(&self, n: i32) {
        self.on.store(n.max(0), Relaxed);
    }

    /// Fil audio : un message, au frame `frame` du moteur.
    pub fn push(&self, frame: i64, dev: u8, msg: &[u8]) {
        let Some(&st) = msg.first() else { return };
        let n = short_len(st);
        if n == 0 || msg.len() < n {
            return;
        }
        let mut data = [0u8; 3];
        data[..n].copy_from_slice(&msg[..n]);
        let packed = data[0] as u64
            | (data[1] as u64) << 8
            | (data[2] as u64) << 16
            | (n as u64) << 24
            | (dev as u64) << 32;
        let w = self.w.load(Relaxed);
        let i = (w % MN) * 2;
        self.ev[i].store(frame as u64, Relaxed);
        self.ev[i + 1].store(packed, Relaxed);
        self.w.store(w + 1, Release);
    }

    /// Fil audio : le bloc est lu jusqu'au frame `end` (exclu).
    pub fn advance(&self, end: i64) {
        self.progress.store(end, Release);
    }

    pub fn progress(&self) -> i64 {
        self.progress.load(Acquire)
    }

    pub fn written(&self) -> usize {
        self.w.load(Acquire)
    }

    /// Le message `i` (compte depuis le depart), s'il n'est pas ecrase :
    /// (frame du moteur, entree, longueur, octets).
    fn read(&self, i: usize) -> Option<(i64, u8, u8, [u8; 3])> {
        let k = (i % MN) * 2;
        let frame = self.ev[k].load(Relaxed) as i64;
        let p = self.ev[k + 1].load(Relaxed);
        // Ecrase pendant la lecture : l'ecrivain a fait le tour.
        if self.w.load(Acquire).saturating_sub(i) > MN {
            return None;
        }
        Some((
            frame,
            (p >> 32) as u8,
            ((p >> 24) & 0xFF) as u8,
            [p as u8, (p >> 8) as u8, (p >> 16) as u8],
        ))
    }
}

struct Active {
    id: u32,
    /// Le debut et la fin de la prise (frames du moteur, temps ENTENDU).
    start: i64,
    stop: Option<i64>,
    /// La latence retiree a chaque message.
    lat: i64,
    /// L'entree retenue (-1 : toutes).
    dev: i32,
    events: Vec<MidiEv>,
    /// Les notes tenues : (entree, canal, note).
    held: Vec<(u8, u8, u8)>,
}

/// Les prises en cours, nourries par l'anneau.
pub struct MidiRecorder {
    ring: Arc<MidiRing>,
    read: usize,
    next: u32,
    takes: Vec<Active>,
    /// Les messages perdus (l'anneau a fait le tour avant d'etre vide).
    pub lost: usize,
}

impl MidiRecorder {
    pub fn new(ring: Arc<MidiRing>) -> MidiRecorder {
        let read = ring.written();
        MidiRecorder {
            ring,
            read,
            next: 1,
            takes: Vec::new(),
            lost: 0,
        }
    }

    pub fn ring(&self) -> &Arc<MidiRing> {
        &self.ring
    }

    /// Une prise qui commence au frame `start` (temps entendu), sur
    /// l'entree `dev` (-1 : toutes), en retirant `lat` frames.
    pub fn start(&mut self, start: i64, lat: i64, dev: i32) -> u32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.takes.push(Active {
            id,
            start,
            stop: None,
            lat,
            dev,
            events: Vec::new(),
            held: Vec::new(),
        });
        id
    }

    pub fn stop(&mut self, id: u32, at: i64) {
        if let Some(t) = self.takes.iter_mut().find(|t| t.id == id) {
            t.stop = Some(at.max(t.start));
        }
    }

    pub fn active(&self) -> usize {
        self.takes.len()
    }

    /// Les messages deja pris d'une prise en cours.
    pub fn count(&self, id: u32) -> Option<usize> {
        self.takes
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.events.len())
    }

    /// Vide l'anneau dans les prises ; rend les prises finies (tout ce qui
    /// pouvait tomber avant leur fin est arrive).
    pub fn drain(&mut self) -> Vec<(u32, Vec<MidiEv>)> {
        let w = self.ring.written();
        if w.saturating_sub(self.read) > MN {
            self.lost += w - self.read - MN;
            self.read = w - MN;
        }
        while self.read < w {
            let Some((g, dev, len, msg)) = self.ring.read(self.read) else {
                self.lost += 1;
                self.read += 1;
                continue;
            };
            self.read += 1;
            for t in &mut self.takes {
                let at = g - t.lat;
                if at < t.start || t.stop.is_some_and(|s| at >= s) {
                    continue;
                }
                if t.dev >= 0 && t.dev != dev as i32 {
                    continue;
                }
                let st = msg[0] & 0xF0;
                let key = (dev, msg[0] & 0x0F, msg[1]);
                let on = st == 0x90 && msg[2] > 0;
                let off = st == 0x80 || (st == 0x90 && msg[2] == 0);
                if off {
                    // Un relachement sans appui dans la prise : oublie.
                    match t.held.iter().position(|h| *h == key) {
                        Some(i) => {
                            t.held.swap_remove(i);
                        }
                        None => continue,
                    }
                } else if on {
                    t.held.push(key);
                }
                t.events.push(MidiEv {
                    frame: at - t.start,
                    dev,
                    len,
                    msg,
                });
            }
        }
        let done_to = self.ring.progress();
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.takes.len() {
            let t = &self.takes[i];
            let finished = t
                .stop
                .is_some_and(|s| done_to != i64::MIN && done_to - t.lat >= s);
            if finished {
                let mut t = self.takes.swap_remove(i);
                let end = t.stop.unwrap_or(t.start) - t.start;
                // Les notes encore tenues : relachees a la fin.
                for (dev, ch, note) in t.held.drain(..) {
                    t.events.push(MidiEv {
                        frame: end,
                        dev,
                        len: 3,
                        msg: [0x80 | ch, note, 0],
                    });
                }
                t.events.sort_by_key(|e| e.frame);
                out.push((t.id, t.events));
            } else {
                i += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_prise_tombe_la_ou_elle_a_ete_entendue() {
        let ring = MidiRing::new();
        let mut r = MidiRecorder::new(ring.clone());
        // La prise de 1000 a 5000 (temps entendu), latence de sortie 200.
        let id = r.start(1000, 200, -1);
        r.stop(id, 5000);
        // Avant la prise (entendu a 900) : rien ; une note entendue a 1500,
        // relachee a 2500 ; un relachement d'une note d'avant : oublie ; un
        // controleur ; une note tenue jusqu'a la fin ; apres la fin : rien.
        ring.push(1100, 0, &[0x90, 60, 100]);
        ring.push(1700, 0, &[0x90, 64, 90]);
        ring.push(2000, 0, &[0x80, 60, 0]);
        ring.push(2700, 0, &[0x80, 64, 0]);
        ring.push(3000, 1, &[0xB0, 64, 127]);
        ring.push(4000, 1, &[0x91, 67, 80]);
        ring.push(5300, 0, &[0x90, 72, 80]);
        ring.push(3100, 0, &[0xF8]);
        ring.advance(5100);
        assert!(r.drain().is_empty(), "la fin n'est pas encore passee");
        ring.advance(5300);
        let done = r.drain();
        assert_eq!(done.len(), 1);
        let ev: Vec<(i64, [u8; 3])> = done[0].1.iter().map(|e| (e.frame, e.msg)).collect();
        assert_eq!(
            ev,
            vec![
                (500, [0x90, 64, 90]),
                (1500, [0x80, 64, 0]),
                (1800, [0xB0, 64, 127]),
                (2800, [0x91, 67, 80]),
                (4000, [0x81, 67, 0]),
            ]
        );
        assert_eq!(r.active(), 0);
    }

    #[test]
    fn une_entree_choisie_et_deux_prises_en_meme_temps() {
        let ring = MidiRing::new();
        let mut r = MidiRecorder::new(ring.clone());
        let a = r.start(0, 0, 1);
        let b = r.start(500, 0, -1);
        ring.push(100, 0, &[0x90, 60, 100]);
        ring.push(600, 1, &[0x90, 62, 100]);
        ring.push(700, 1, &[0x80, 62, 0]);
        r.stop(a, 1000);
        r.stop(b, 1000);
        ring.advance(1000);
        let mut done = r.drain();
        done.sort_by_key(|d| d.0);
        assert_eq!(done[0].1.len(), 2, "l'entree 1 seulement");
        assert_eq!(done[1].1.len(), 2, "toutes les entrees, apres 500");
        assert_eq!(done[1].1[0].frame, 100);
    }

    #[test]
    fn l_anneau_qui_deborde_compte_ses_pertes() {
        let ring = MidiRing::new();
        let mut r = MidiRecorder::new(ring.clone());
        let id = r.start(0, 0, -1);
        for i in 0..(MN + 100) {
            ring.push(i as i64, 0, &[0xB0, 1, (i % 128) as u8]);
        }
        r.stop(id, (MN + 100) as i64);
        ring.advance((MN + 100) as i64);
        let done = r.drain();
        assert_eq!(r.lost, 100);
        assert_eq!(done[0].1.len(), MN);
        assert_eq!(done[0].1[0].frame, 100, "les plus vieux sont perdus");
    }
}
