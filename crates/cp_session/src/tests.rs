//! La Session sur un faux moteur : chaque commande est notee avec son frame.

use std::collections::HashMap;

use super::*;
use crate::clip::{TempoMode, bars_for, tempo_from_length, tempo_from_name};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    LanePlay {
        port: u32,
        id: u64,
        from: f64,
        at: i64,
    },
    LaneUpdate {
        port: u32,
        notes: usize,
    },
    LaneStop {
        port: u32,
        at: i64,
    },
    LaneCancel {
        port: u32,
    },
    LaneRate {
        port: u32,
        fpb: f64,
    },
    VoicePlay {
        port: u32,
        v: u32,
        start: i64,
        looped: bool,
        rate: f64,
        at: i64,
    },
    VoiceStop {
        v: u32,
        at: Option<i64>,
    },
    VoiceKeep {
        v: u32,
    },
    MidiRecStart {
        id: u32,
        at: i64,
    },
    MidiRecStop {
        id: u32,
        at: i64,
    },
    AudioRecStart {
        id: u32,
        at: i64,
    },
    AudioRecStop {
        id: u32,
        at: i64,
    },
}

struct Fake {
    now: i64,
    calls: Vec<Call>,
    lanes: bool,
    next_v: u32,
    next_rec: u32,
    srcs: HashMap<String, AudioSrc>,
    loading: Vec<String>,
    takes: HashMap<u32, Vec<(i64, [u8; 3])>>,
    rec_stop: HashMap<u32, i64>,
    baked: bool,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            now: 0,
            calls: Vec::new(),
            lanes: true,
            next_v: 1,
            next_rec: 1,
            srcs: HashMap::new(),
            loading: Vec::new(),
            takes: HashMap::new(),
            rec_stop: HashMap::new(),
            baked: false,
        }
    }
    fn take(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.calls)
    }
}

impl Engine for Fake {
    fn now(&self) -> i64 {
        self.now
    }
    fn srate(&self) -> f64 {
        48000.0
    }
    fn has_lanes(&self) -> bool {
        self.lanes
    }
    fn ports(&self) -> u32 {
        128
    }
    fn route(&mut self, _port: u32, _track: &str) -> bool {
        true
    }
    fn lane_play(&mut self, port: u32, clip: &LaneClip, from: f64, _fpb: f64, at: i64) -> bool {
        self.calls.push(Call::LanePlay {
            port,
            id: clip.id,
            from,
            at,
        });
        true
    }
    fn lane_update(&mut self, port: u32, clip: &LaneClip) -> bool {
        self.calls.push(Call::LaneUpdate {
            port,
            notes: clip.notes.len(),
        });
        true
    }
    fn lane_stop(&mut self, port: u32, at: i64) -> bool {
        self.calls.push(Call::LaneStop { port, at });
        true
    }
    fn lane_cancel(&mut self, port: u32) {
        self.calls.push(Call::LaneCancel { port });
    }
    fn lane_rate(&mut self, port: u32, fpb: f64, _at: i64) {
        self.calls.push(Call::LaneRate { port, fpb });
    }
    fn audio_src(&mut self, path: &str) -> Option<Result<AudioSrc, String>> {
        if self.loading.iter().any(|p| p == path) {
            return None;
        }
        Some(
            self.srcs
                .get(path)
                .cloned()
                .ok_or_else(|| "missing".to_string()),
        )
    }
    fn voice_play(
        &mut self,
        port: u32,
        _src: &AudioSrc,
        start: i64,
        _end: i64,
        looped: bool,
        _gain: f64,
        rate: f64,
        at: i64,
    ) -> Option<u32> {
        let v = self.next_v;
        self.next_v += 1;
        self.calls.push(Call::VoicePlay {
            port,
            v,
            start,
            looped,
            rate,
            at,
        });
        Some(v)
    }
    fn voice_stop(&mut self, voice: u32, at: Option<i64>, _fade_s: f64) {
        self.calls.push(Call::VoiceStop { v: voice, at });
    }
    fn voice_keep(&mut self, voice: u32) {
        self.calls.push(Call::VoiceKeep { v: voice });
    }
    fn voice_rate(&mut self, _voice: u32, _rate: f64) {}
    fn midi_rec_start(&mut self, _dev: i32, at: i64) -> u32 {
        let id = self.next_rec;
        self.next_rec += 1;
        self.calls.push(Call::MidiRecStart { id, at });
        id
    }
    fn midi_rec_stop(&mut self, id: u32, at: i64) {
        self.rec_stop.insert(id, at);
        self.calls.push(Call::MidiRecStop { id, at });
    }
    fn midi_rec_take(&mut self, id: u32) -> Option<Vec<(i64, [u8; 3])>> {
        let end = *self.rec_stop.get(&id)?;
        if self.now < end {
            return None;
        }
        Some(self.takes.remove(&id).unwrap_or_default())
    }
    fn audio_rec_start(&mut self, _ch0: i32, _ch1: i32, at: i64) -> u32 {
        let id = self.next_rec;
        self.next_rec += 1;
        self.calls.push(Call::AudioRecStart { id, at });
        id
    }
    fn audio_rec_stop(&mut self, id: u32, at: i64) {
        self.rec_stop.insert(id, at);
        self.calls.push(Call::AudioRecStop { id, at });
    }
    fn audio_rec_poll(&mut self, id: u32) -> RecPoll {
        match self.rec_stop.get(&id) {
            Some(&end) if self.now >= end => RecPoll::Ready(AudioSrc {
                clip: 900 + id,
                frames: 96000,
                srate: 48000.0,
                file_bpm: None,
            }),
            _ => RecPoll::Recording,
        }
    }
    fn stretched(&mut self, src: &AudioSrc, start: i64, end: i64, tempo: f64) -> Option<AudioSrc> {
        self.baked.then(|| AudioSrc {
            clip: src.clip + 100,
            frames: ((end - start) as f64 / tempo) as i64,
            srate: src.srate,
            file_bpm: None,
        })
    }
    fn save_take(&mut self, clip: u32, name: &str) -> Option<String> {
        let p = format!("C:/rec/{name}.wav");
        self.srcs.insert(
            p.clone(),
            AudioSrc {
                clip,
                frames: 96000,
                srate: 48000.0,
                file_bpm: None,
            },
        );
        Some(p)
    }
}

/// 120 BPM a 48 kHz : 24 000 frames par noire, 4 noires par mesure.
const FPB: f64 = 24000.0;
const BAR: i64 = 96000;

fn track(g: &str, input: Input) -> TrackInfo {
    TrackInfo {
        guid: g.into(),
        name: g.into(),
        color: None,
        armed: false,
        input,
        instrument: false,
        sound: None,
    }
}

fn notes(v: &[(f64, f64, u8)]) -> Vec<Note> {
    v.iter()
        .map(|&(s, l, p)| Note {
            start: s,
            len: l,
            pitch: p,
            vel: 100,
            chan: 0,
            prob: 100,
        })
        .collect()
}

fn setup() -> (Session, Fake) {
    let mut s = Session::new();
    let mut e = Fake::new();
    s.set_tracks(
        &mut e,
        vec![track("A", Input::Midi(-1)), track("B", Input::Audio(0, 1))],
    );
    s.sync_clock(&mut e, 0, 0.0, FPB, 4.0, false);
    (s, e)
}

fn lat() -> i64 {
    (time::LATENCY_S * 48000.0) as i64
}

#[test]
fn chaque_colonne_a_son_port_a_partir_de_1() {
    let (s, _) = setup();
    assert_eq!(s.cols[0].port, 1);
    assert_eq!(s.cols[1].port, 2);
}

#[test]
fn le_premier_lancement_part_tout_de_suite_puis_les_autres_a_la_mesure() {
    let (mut s, mut e) = setup();
    let id = s.fresh_id();
    s.put(0, 0, Clip::midi(id, 1.0, notes(&[(0.0, 1.0, 60)])));
    let id2 = s.fresh_id();
    s.put(0, 1, Clip::midi(id2, 1.0, notes(&[(0.0, 1.0, 62)])));
    // Rien ne joue : l'horloge libre part maintenant (+ la latence).
    e.now = 1000;
    s.click(&mut e, 0, 0);
    assert_eq!(
        e.take(),
        vec![Call::LanePlay {
            port: 1,
            id,
            from: 0.0,
            at: 1000 + lat()
        }]
    );
    let t0 = 1000 + lat();
    e.now = t0 + 10;
    s.tick(&mut e);
    assert_eq!(s.state(0, 0), CellState::Playing);
    // A mi-mesure, une autre case : la mesure suivante.
    e.now = t0 + BAR / 2;
    s.click(&mut e, 0, 1);
    let calls = e.take();
    assert!(calls.contains(&Call::LanePlay {
        port: 1,
        id: id2,
        from: 0.0,
        at: t0 + BAR
    }));
    assert_eq!(s.state(0, 1), CellState::Queued);
    assert_eq!(s.state(0, 0), CellState::Playing);
    e.now = t0 + BAR + 1;
    s.tick(&mut e);
    assert_eq!(s.state(0, 1), CellState::Playing);
    assert_eq!(s.state(0, 0), CellState::Stopped);
}

#[test]
fn un_lancement_tardif_compte_pour_la_frontiere_passee() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + 1;
    s.tick(&mut e);
    // 0,1 noire apres la mesure (sous la tolerance de 0,25) : la mesure qui
    // vient de passer.
    e.now = t0 + BAR + (0.1 * FPB) as i64 - lat();
    e.take();
    s.click(&mut e, 0, 1);
    assert!(e.take().contains(&Call::LanePlay {
        port: 1,
        id: b,
        from: 0.0,
        at: t0 + BAR
    }));
}

#[test]
fn recliquer_annule_puis_arrete_a_la_frontiere() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + BAR / 2;
    s.tick(&mut e);
    s.click(&mut e, 0, 1);
    assert_eq!(s.state(0, 1), CellState::Queued);
    e.take();
    s.click(&mut e, 0, 1);
    assert_eq!(e.take(), vec![Call::LaneCancel { port: 1 }]);
    assert_eq!(s.state(0, 1), CellState::Stopped);
    // Recliquer la case qui joue : elle s'arrete a la mesure.
    s.click(&mut e, 0, 0);
    assert_eq!(
        e.take(),
        vec![Call::LaneStop {
            port: 1,
            at: t0 + BAR
        }]
    );
    assert_eq!(s.state(0, 0), CellState::Stopping);
    e.now = t0 + BAR;
    s.tick(&mut e);
    assert_eq!(s.state(0, 0), CellState::Stopped);
    // Plus rien ne joue : l'horloge libre s'arrete.
    assert!(!s.tl.running);
}

#[test]
fn une_scene_lance_ses_cases_et_arrete_les_autres_sans_toggle() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(1, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.cols[1].track.input = Input::Midi(-1);
    s.launch_scene(&mut e, 0, false);
    let t0 = lat();
    e.now = t0 + 1;
    s.tick(&mut e);
    assert_eq!(s.state(0, 0), CellState::Playing);
    e.take();
    // Relancer la meme scene : la case qui joue n'est PAS arretee.
    s.launch_scene(&mut e, 0, false);
    assert!(
        !e.take()
            .iter()
            .any(|c| matches!(c, Call::LaneStop { port: 1, .. }))
    );
    assert_eq!(s.state(0, 0), CellState::Playing);
    // La scene 1 : la colonne A (sans clip la) s'arrete, B part.
    s.launch_scene(&mut e, 1, false);
    let c = e.take();
    assert!(c.contains(&Call::LaneStop {
        port: 1,
        at: t0 + BAR
    }));
    assert!(
        c.iter()
            .any(|c| matches!(c, Call::LanePlay { port: 2, .. }))
    );
}

#[test]
fn verrou_de_phase() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let mut c = Clip::midi(s.fresh_id(), 2.0, notes(&[(0.0, 1.0, 62)]));
    c.phase_lock = true;
    let b = c.id;
    s.put(1, 0, c);
    s.cols[1].track.input = Input::Midi(-1);
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + BAR + BAR / 2;
    s.tick(&mut e);
    e.take();
    s.q = Quant::Bar;
    s.click(&mut e, 1, 0);
    // Lancee a la mesure 3 (noire 8) : une boucle de 2 mesures lancee au
    // temps 0 y serait a 0.
    assert!(e.take().contains(&Call::LanePlay {
        port: 2,
        id: b,
        from: 0.0,
        at: t0 + 2 * BAR
    }));
    // Une mesure plus tard, elle serait a la noire 4.
    let mut c = Clip::midi(s.fresh_id(), 2.0, notes(&[(0.0, 1.0, 64)]));
    c.phase_lock = true;
    let d = c.id;
    s.put(1, 1, c);
    e.now = t0 + 2 * BAR + BAR / 2;
    s.tick(&mut e);
    e.take();
    s.click(&mut e, 1, 1);
    assert!(e.take().contains(&Call::LanePlay {
        port: 2,
        id: d,
        from: 4.0,
        at: t0 + 3 * BAR
    }));
}

#[test]
fn une_prise_midi_part_a_la_frontiere_dure_rec_mesures_et_tourne() {
    let (mut s, mut e) = setup();
    s.rec_bars = 2;
    // Une colonne qui joue deja : la prise attend la frontiere de 2 mesures.
    let a = s.fresh_id();
    s.put(1, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.cols[1].track.input = Input::Midi(-1);
    s.click(&mut e, 1, 0);
    let t0 = lat();
    e.now = t0 + BAR / 2;
    s.tick(&mut e);
    e.take();
    assert!(s.record(&mut e, 0, 3));
    let start = t0 + 2 * BAR;
    let end = start + 2 * BAR;
    assert_eq!(
        e.take(),
        vec![
            Call::MidiRecStart { id: 1, at: start },
            Call::MidiRecStop { id: 1, at: end }
        ]
    );
    e.now = start + 1;
    s.tick(&mut e);
    assert_eq!(s.state(0, 3), CellState::Recording);
    // Deux notes jouees.
    e.takes.insert(
        1,
        vec![
            (0, [0x90, 60, 100]),
            ((FPB / 2.0) as i64, [0x80, 60, 0]),
            ((5.0 * FPB) as i64, [0x90, 64, 90]),
            // Le canal 16 : un apercu, jamais pris.
            ((5.0 * FPB) as i64, [0x9F, 70, 90]),
        ],
    );
    e.now = end + 10;
    s.tick(&mut e);
    let c = s.clip(0, 3).unwrap();
    assert_eq!(c.bars, 2.0);
    let n = c.notes();
    assert_eq!(n.len(), 2);
    assert_eq!((n[0].start, n[0].len, n[0].pitch), (0.0, 0.5, 60));
    // Encore tenue a la fin : elle finit avec la prise.
    assert_eq!((n[1].start, n[1].len), (5.0, 3.0));
    // Le clip tourne, en phase avec la fin de la prise.
    assert!(
        e.take()
            .iter()
            .any(|c| matches!(c, Call::LanePlay { port: 1, at, .. } if *at == end))
    );
}

#[test]
fn une_prise_dans_une_session_silencieuse_part_tout_de_suite() {
    let (mut s, mut e) = setup();
    s.rec_bars = 4;
    e.now = 500;
    assert!(s.record(&mut e, 0, 0));
    assert_eq!(
        e.take()[0],
        Call::MidiRecStart {
            id: 1,
            at: 500 + lat()
        }
    );
    // Recliquee avant la fin : elle finit maintenant, arrondie a la mesure.
    e.now = 500 + lat() + BAR + BAR / 3;
    s.stop_rec(&mut e, 0);
    assert_eq!(s.cols[0].run.rec.as_ref().unwrap().beats, 8.0);
}

#[test]
fn une_prise_audio_devient_une_case_qui_boucle() {
    let (mut s, mut e) = setup();
    s.rec_bars = 1;
    e.now = 0;
    assert!(s.record(&mut e, 1, 0));
    let c = e.take();
    assert_eq!(c[0], Call::AudioRecStart { id: 1, at: lat() });
    e.now = lat() + BAR + 5;
    s.tick(&mut e);
    let clip = s.clip(1, 0).unwrap();
    assert_eq!(clip.bars, 1.0);
    let Kind::Audio(a) = &clip.kind else { panic!() };
    assert!(a.looped);
    assert_eq!(a.src_bpm, Some(120.0));
    assert!(e.take().iter().any(|c| matches!(
        c,
        Call::VoicePlay {
            port: 2,
            looped: true,
            ..
        }
    )));
}

#[test]
fn une_case_audio_prend_sa_passe_et_son_taux() {
    let (mut s, mut e) = setup();
    // 2,05 s a 120 BPM = une mesure de 2 s a 2,5 % pres : pas ajustee ;
    // 2,03 s nommee 120bpm : ajustee a la mesure exacte.
    e.srcs.insert(
        "C:/x/loop 120bpm.wav".into(),
        AudioSrc {
            clip: 7,
            frames: (2.03 * 48000.0) as i64,
            srate: 48000.0,
            file_bpm: None,
        },
    );
    let id = s.fresh_id();
    s.put(1, 0, Clip::audio(id, "C:/x/loop 120bpm.wav"));
    s.click(&mut e, 1, 0);
    let c = e.take();
    let Call::VoicePlay { rate, looped, .. } = c[0] else {
        panic!("{c:?}")
    };
    assert!(looped);
    assert!((rate - 2.03 / 2.0).abs() < 1e-4, "{rate}");
    assert_eq!(s.clip(1, 0).unwrap().bars, 1.0);
}

#[test]
fn un_one_shot_rejoue_a_chaque_passe() {
    let (mut s, mut e) = setup();
    e.srcs.insert(
        "C:/x/hit.wav".into(),
        AudioSrc {
            clip: 7,
            frames: 4800,
            srate: 48000.0,
            file_bpm: None,
        },
    );
    let id = s.fresh_id();
    let mut c = Clip::audio(id, "C:/x/hit.wav");
    c.bars = 1.0;
    if let Kind::Audio(a) = &mut c.kind {
        a.looped = false;
        a.tempo = TempoMode::None;
    }
    s.put(1, 0, c);
    s.click(&mut e, 1, 0);
    let t0 = lat();
    e.take();
    for k in 0..3 {
        e.now = t0 + k * BAR + BAR - 1000;
        s.tick(&mut e);
    }
    let plays: Vec<i64> = e
        .take()
        .iter()
        .filter_map(|c| match c {
            Call::VoicePlay { at, .. } => Some(*at),
            _ => None,
        })
        .collect();
    assert_eq!(plays, vec![t0 + BAR, t0 + 2 * BAR, t0 + 3 * BAR]);
}

#[test]
fn une_case_audio_qui_se_charge_attend_puis_part() {
    let (mut s, mut e) = setup();
    e.loading.push("C:/x/a.wav".into());
    e.srcs.insert(
        "C:/x/a.wav".into(),
        AudioSrc {
            clip: 3,
            frames: 96000,
            srate: 48000.0,
            file_bpm: Some(120.0),
        },
    );
    let id = s.fresh_id();
    s.put(1, 0, Clip::audio(id, "C:/x/a.wav"));
    s.click(&mut e, 1, 0);
    assert_eq!(s.state(1, 0), CellState::Waiting);
    e.loading.clear();
    e.now = 3000;
    s.tick(&mut e);
    assert!(e.take().iter().any(|c| matches!(c, Call::VoicePlay { .. })));
}

#[test]
fn motion_marche_et_revient() {
    let (mut s, mut e) = setup();
    for k in 0..2 {
        let id = s.fresh_id();
        s.put(
            0,
            k,
            Clip::midi(id, 1.0, notes(&[(0.0, 1.0, 60 + k as u8)])),
        );
    }
    s.cols[0].motion = Motion::MarchWrap;
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + 1;
    s.tick(&mut e);
    e.take();
    // A une noire de la fin de la passe : la case suivante, datee a la fin.
    e.now = t0 + BAR - (FPB as i64) + 10;
    s.tick(&mut e);
    let c = e.take();
    assert!(
        c.iter()
            .any(|c| matches!(c, Call::LanePlay { at, .. } if *at == t0 + BAR))
    );
    e.now = t0 + BAR + 1;
    s.tick(&mut e);
    assert_eq!(s.state(0, 1), CellState::Playing);
    // Et revient en haut.
    e.now = t0 + 2 * BAR - (FPB as i64) + 10;
    s.tick(&mut e);
    e.now = t0 + 2 * BAR + 1;
    s.tick(&mut e);
    assert_eq!(s.state(0, 0), CellState::Playing);
}

#[test]
fn l_overdub_fond_les_notes_dans_la_boucle() {
    let (mut s, mut e) = setup();
    let id = s.fresh_id();
    s.put(0, 0, Clip::midi(id, 1.0, notes(&[(0.0, 0.5, 60)])));
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + BAR + 100;
    s.tick(&mut e);
    e.take();
    assert!(s.overdub(&mut e, 0));
    let c = e.take();
    let Call::MidiRecStart { id: rid, at: rs } = c[0] else {
        panic!("{c:?}")
    };
    // La premiere prise finit a la fin de la passe.
    assert_eq!(
        c[1],
        Call::MidiRecStop {
            id: rid,
            at: t0 + 2 * BAR
        }
    );
    // Une note jouee une noire et demie apres le debut de la prise.
    let f = (t0 + BAR + (1.5 * FPB) as i64) - rs;
    e.takes
        .insert(rid, vec![(f, [0x90, 67, 80]), (f + 12000, [0x80, 67, 0])]);
    e.now = t0 + 2 * BAR - 1000;
    s.tick(&mut e);
    e.now = t0 + 2 * BAR + 10;
    s.tick(&mut e);
    let n = s.clip(0, 0).unwrap().notes().to_vec();
    assert_eq!(n.len(), 2);
    assert!((n[1].start - 1.5).abs() < 1e-3, "{n:?}");
    assert!(
        e.take()
            .iter()
            .any(|c| matches!(c, Call::LaneUpdate { notes: 2, .. }))
    );
}

#[test]
fn deplacer_echanger_copier_effacer_et_rendre() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.move_cell(&mut e, (0, 0), (0, 1), false);
    assert_eq!(s.clip(0, 1).unwrap().id, a);
    assert_eq!(s.clip(0, 0).unwrap().id, b);
    s.move_cell(&mut e, (0, 1), (1, 4), true);
    let c = s.clip(1, 4).unwrap();
    assert_ne!(c.id, a);
    assert_eq!(c.notes().len(), 1);
    s.erase(&mut e, 1, 4);
    assert!(s.clip(1, 4).is_none());
    assert!(s.undo_erase());
    assert!(s.clip(1, 4).is_some());
}

#[test]
fn capturer_une_scene_ne_relance_rien_et_n_ecrase_rien() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(1, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.cols[1].track.input = Input::Midi(-1);
    s.click(&mut e, 0, 0);
    s.click(&mut e, 1, 1);
    e.now = lat() + 5;
    s.tick(&mut e);
    e.take();
    s.sel = (0, 0);
    let row = s.capture_scene().unwrap();
    assert_eq!(row, 2);
    assert!(s.clip(0, 2).is_some() && s.clip(1, 2).is_some());
    assert!(e.take().is_empty());
}

#[test]
fn le_mode_follow_attend_le_transport() {
    let (mut s, mut e) = setup();
    s.clock = ClockMode::Follow;
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.click(&mut e, 0, 0);
    assert_eq!(s.state(0, 0), CellState::Waiting);
    assert!(e.take().is_empty());
    // Le transport part a la noire 2.5 du projet : la mesure suivante (4).
    e.now = 10_000;
    s.sync_clock(&mut e, 10_000, 2.5, FPB, 4.0, true);
    assert!(e.take().contains(&Call::LanePlay {
        port: 1,
        id: a,
        from: 0.0,
        at: 10_000 + (1.5 * FPB) as i64
    }));
}

#[test]
fn sans_lanes_le_midi_le_dit() {
    let (mut s, mut e) = setup();
    e.lanes = false;
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.click(&mut e, 0, 0);
    assert!(e.take().is_empty());
    assert!(s.flash.as_ref().unwrap().0.contains("restart"));
}

#[test]
fn enregistrer_et_relire() {
    let (mut s, mut e) = setup();
    let mut c = Clip::midi(s.fresh_id(), 2.0, notes(&[(0.0, 0.5, 60), (1.5, 0.25, 64)]));
    c.name = "Riff | 1".into();
    c.color = Some(3);
    c.loop_a = 2.0;
    c.loop_b = 6.0;
    c.phase_lock = true;
    c.sig = Some((6, 8));
    let mut n = c.notes().to_vec();
    n[1].chan = 3;
    n[1].prob = 50;
    c.kind = Kind::Midi(n);
    s.put(0, 2, c.clone());
    let mut au = Clip::audio(s.fresh_id(), "C:/a|b.wav");
    if let Kind::Audio(a) = &mut au.kind {
        a.offs = 0.5;
        a.len = 1.25;
        a.src_bpm = Some(98.5);
        a.tempo = TempoMode::Stretch;
        a.gain = 0.5;
        a.looped = false;
    }
    au.bars = 0.25;
    s.put(1, 9, au.clone());
    s.cols[0].motion = Motion::Random;
    s.q = Quant::Beat;
    s.rec_bars = 4;
    s.clock = ClockMode::Follow;
    let saved: HashMap<&str, String> = s.save().into_iter().collect();
    let mut t = Session::new();
    t.load(&|k| saved.get(k).cloned());
    t.set_tracks(
        &mut e,
        vec![track("A", Input::Midi(-1)), track("B", Input::Audio(0, 1))],
    );
    t.apply_pending_motion();
    assert_eq!(t.clip(0, 2), Some(&c));
    assert_eq!(t.clip(1, 9), Some(&au));
    assert_eq!(t.cols[0].motion, Motion::Random);
    assert_eq!(
        (t.q, t.rec_bars, t.clock),
        (Quant::Beat, 4, ClockMode::Follow)
    );
    assert!(t.scenes >= 11);
    assert!(t.next_id > au.id);
}

#[test]
fn reprendre_l_ancienne_session() {
    let mut s = Session::new();
    let old = "0:0:CPC1|kind=midi|name=Bass|bars=2|notes=0,1,36,100;2,1,38,90|probs=100;50\n\
               1:3:CPC1|kind=audio|path=C%3A/x/loop.wav|lmode=loop|src_bpm=120\n\
               5:0:CPC1|kind=midi|notes=0,1,60,100\n";
    let n = s.import_old(old, &|t| match t {
        0 => Some("GA".into()),
        1 => Some("GB".into()),
        _ => None,
    });
    assert_eq!(n, 2);
    let c = &s.cells[&("GA".to_string(), 0)];
    assert_eq!(c.name, "Bass");
    assert_eq!(c.notes()[1].prob, 50);
    let Kind::Audio(a) = &s.cells[&("GB".to_string(), 3)].kind else {
        panic!()
    };
    assert_eq!(a.path, "C:/x/loop.wav");
    assert!(a.looped);
}

#[test]
fn une_piste_disparue_garde_ses_clips() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(1, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.set_tracks(&mut e, vec![track("A", Input::None)]);
    assert_eq!(s.cols.len(), 2);
    assert!(s.cols[1].held);
    assert_eq!(s.cols[1].track.guid, "B");
    // Revenue : la colonne reprend sa place, et ses clips.
    s.set_tracks(
        &mut e,
        vec![track("B", Input::None), track("A", Input::None)],
    );
    assert!(!s.cols[0].held);
    assert!(s.clip(0, 0).is_some());
}

#[test]
fn les_tempos_d_un_nom_et_d_une_duree() {
    assert_eq!(tempo_from_name("Drums 128bpm.wav"), Some(128.0));
    assert_eq!(tempo_from_name("loop_95_BPM_Am.wav"), Some(95.0));
    assert_eq!(tempo_from_name("kit_124_hat.wav"), Some(124.0));
    assert_eq!(tempo_from_name("kick 01.wav"), None);
    // 4 s a 120 BPM = 2 mesures, seul candidat pres de 118.
    assert_eq!(tempo_from_length(4.0, 4.0, 118.0), Some(120.0));
    assert_eq!(bars_for(3.9, 4.0), 1.0);
    assert_eq!(bars_for(4.2, 4.0), 2.0);
    assert_eq!(bars_for(1.0, 4.0), 0.25);
}

#[test]
fn arrange_rec_note_ce_qui_joue_et_le_rend_a_l_arret() {
    let (mut s, mut e) = setup();
    s.clock = ClockMode::Follow;
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.arr_rec = true;
    // Le transport part a la noire 0.
    s.sync_clock(&mut e, 0, 0.0, FPB, 4.0, true);
    s.click(&mut e, 0, 0);
    e.now = lat() + 10;
    s.tick(&mut e);
    // La scene 1, demandee a mi-mesure : a la mesure 2 (noire 4).
    e.now = BAR / 2;
    s.click(&mut e, 0, 1);
    e.now = BAR + 10;
    s.tick(&mut e);
    // Le transport s'arrete a la noire 10 ; REAPER rend son curseur au
    // depart (noire 0).
    e.now = (10.0 * FPB) as i64;
    let n = e.now;
    s.sync_clock(&mut e, n, 0.0, FPB, 4.0, false);
    assert!(!s.arr_rec);
    let log = s.take_arrange();
    assert_eq!(log.len(), 2);
    assert_eq!((log[0].start_b, log[0].end_b), (0.0, Some(4.0)));
    assert_eq!((log[1].start_b, log[1].end_b), (4.0, Some(10.0)));
    // Deplie : 6 noires d'un clip d'une mesure = deux passes (la seconde
    // coupee).
    let n = unfold(&log[1].clip, log[1].from, log[1].span, 6.0);
    assert_eq!(
        n.iter().map(|n| n.start).collect::<Vec<_>>(),
        vec![0.0, 4.0]
    );
}

#[test]
fn deplier_une_accolade_et_un_depart() {
    let mut c = Clip::midi(
        1,
        2.0,
        notes(&[(1.0, 0.5, 60), (4.0, 0.5, 62), (5.0, 3.0, 64)]),
    );
    c.loop_a = 4.0;
    c.loop_b = 6.0;
    let n = unfold(&c, 0.0, c.span(4.0), 9.0);
    // Premiere passe 0..6 : 60 a 1, 62 a 4, 64 a 5 (coupee a 6) ; puis la
    // zone 4..6 : 62 a 6, 64 a 7 (coupee), 62 a 8.
    let v: Vec<(f64, u8, f64)> = n.iter().map(|n| (n.start, n.pitch, n.len)).collect();
    assert_eq!(
        v,
        vec![
            (1.0, 60, 0.5),
            (4.0, 62, 0.5),
            (5.0, 64, 1.0),
            (6.0, 62, 0.5),
            (7.0, 64, 1.0),
            (8.0, 62, 0.5)
        ]
    );
}

#[test]
fn une_case_etiree_change_de_hauteur_puis_prend_sa_version_cuite() {
    let (mut s, mut e) = setup();
    e.srcs.insert(
        "C:/x/loop 100bpm.wav".into(),
        AudioSrc {
            clip: 7,
            frames: (4.8 * 48000.0) as i64,
            srate: 48000.0,
            file_bpm: None,
        },
    );
    let id = s.fresh_id();
    let mut c = Clip::audio(id, "C:/x/loop 100bpm.wav");
    if let Kind::Audio(a) = &mut c.kind {
        a.tempo = TempoMode::Stretch;
    }
    s.put(1, 0, c);
    s.click(&mut e, 1, 0);
    let c = e.take();
    // Pas encore cuite : le taux 1,2 (120 / 100).
    assert!(matches!(c[0], Call::VoicePlay { rate, .. } if (rate - 1.2).abs() < 1e-9));
    e.baked = true;
    e.now = lat() + 100;
    s.tick(&mut e);
    let c = e.take();
    // La version cuite, a taux 1, a la fin de la passe (2 mesures).
    assert!(c.iter().any(|c| matches!(c, Call::VoicePlay { rate, at, .. } if *rate == 1.0 && *at == lat() + 2 * BAR)), "{c:?}");
}

#[test]
fn un_saut_du_transport_recale_ce_qui_joue() {
    let (mut s, mut e) = setup();
    s.clock = ClockMode::Follow;
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.sync_clock(&mut e, 0, 0.0, FPB, 4.0, true);
    s.click(&mut e, 0, 0);
    e.now = lat() + 10;
    s.tick(&mut e);
    e.take();
    // Une noire plus tard, le transport saute a la noire 13 (une mesure de
    // 4 : position 1 dans le clip).
    e.now = FPB as i64;
    let n = e.now;
    s.sync_clock(&mut e, n, 13.0 - time::LATENCY_S * 2.0, FPB, 4.0, true);
    let c = e.take();
    let Some(Call::LanePlay { from, .. }) = c.iter().find(|c| matches!(c, Call::LanePlay { .. }))
    else {
        panic!("{c:?}")
    };
    assert!((from - 1.0).abs() < 1e-6, "{from}");
}

#[test]
fn une_case_audio_d_une_piste_a_instrument_joue_dans_sa_piste_de_son() {
    let (mut s, mut e) = setup();
    let mut t = track("B", Input::None);
    t.instrument = true;
    t.sound = Some("Bs".into());
    s.set_tracks(&mut e, vec![track("A", Input::Midi(-1)), t]);
    assert_eq!(s.cols[1].port, 2);
    assert_eq!(s.audio_port(1), 3);
    e.srcs.insert(
        "C:/x/a.wav".into(),
        AudioSrc {
            clip: 3,
            frames: 96000,
            srate: 48000.0,
            file_bpm: Some(120.0),
        },
    );
    let id = s.fresh_id();
    s.put(1, 0, Clip::audio(id, "C:/x/a.wav"));
    s.click(&mut e, 1, 0);
    assert!(
        e.take()
            .iter()
            .any(|c| matches!(c, Call::VoicePlay { port: 3, .. }))
    );
}

#[test]
fn relecture_un_tempo_change_recale_les_fins_de_passe() {
    // Motion datee apres un changement de tempo : a la fin REELLE de la
    // passe (120 -> 240 BPM a mi-mesure : la mesure finit 1 s plus tot...
    // en frames : 2 noires a 24000 puis 2 a 12000).
    let (mut s, mut e) = setup();
    for k in 0..2 {
        let id = s.fresh_id();
        s.put(
            0,
            k,
            Clip::midi(id, 1.0, notes(&[(0.0, 1.0, 60 + k as u8)])),
        );
    }
    s.cols[0].motion = Motion::MarchWrap;
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + 2 * FPB as i64;
    s.tick(&mut e);
    let n = e.now;
    s.sync_clock(&mut e, n, 0.0, 12000.0, 4.0, false);
    e.take();
    e.now = t0 + 2 * FPB as i64 + 12000 + 100;
    s.tick(&mut e);
    let end = t0 + 2 * FPB as i64 + 2 * 12000;
    let c = e.take();
    assert!(
        c.iter()
            .any(|c| matches!(c, Call::LanePlay { at, .. } if (*at - end).abs() <= 1)),
        "{c:?}"
    );
}

#[test]
fn relecture_un_clip_qui_attend_prend_le_nouveau_tempo() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.click(&mut e, 0, 0);
    e.now = lat() + BAR / 2;
    s.tick(&mut e);
    s.click(&mut e, 0, 1);
    let at = s.cols[0].run.next.as_ref().unwrap().start_f;
    e.take();
    // Un petit changement : la lane qui attend recoit le tempo a son frame.
    let n = e.now;
    s.sync_clock(&mut e, n, 0.0, 23990.0, 4.0, false);
    assert!(e.take().contains(&Call::LaneRate {
        port: 1,
        fpb: 23990.0
    }));
    let _ = at;
}

#[test]
fn relecture_un_overdub_ne_passe_pas_dans_le_clip_suivant() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 0.5, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 0.5, 72)])));
    s.click(&mut e, 0, 0);
    let t0 = lat();
    e.now = t0 + BAR + 100;
    s.tick(&mut e);
    e.take();
    s.overdub(&mut e, 0);
    let c = e.take();
    let Call::MidiRecStart { id: rid, at: rs } = c[0] else {
        panic!("{c:?}")
    };
    // La scene 1 est lancee a la mesure 3 : la prise (qui finissait la)
    // est la derniere ; aucune autre ne s'enchaine.
    let releve = t0 + 2 * BAR;
    assert_eq!(
        c[1],
        Call::MidiRecStop {
            id: rid,
            at: releve
        }
    );
    e.now = t0 + BAR + BAR / 2;
    s.click(&mut e, 0, 1);
    e.take();
    let f = t0 + BAR + (1.5 * FPB) as i64 - rs;
    e.takes
        .insert(rid, vec![(f, [0x90, 67, 80]), (f + 6000, [0x80, 67, 0])]);
    e.now = releve + 10;
    s.tick(&mut e);
    e.now = releve + 20;
    s.tick(&mut e);
    // La note rejoint le clip A ; B est intact ; l'overdub est fini.
    assert_eq!(s.clip(0, 0).unwrap().notes().len(), 2);
    assert_eq!(s.clip(0, 1).unwrap().notes().len(), 1);
    assert!(s.cols[0].run.dub.is_none());
}

#[test]
fn relecture_le_transport_s_arrete_pendant_une_prise() {
    let (mut s, mut e) = setup();
    s.clock = ClockMode::Follow;
    s.sync_clock(&mut e, 0, 0.0, FPB, 4.0, true);
    s.record(&mut e, 0, 0);
    e.now = BAR / 2;
    s.tick(&mut e);
    let n = e.now;
    s.sync_clock(&mut e, n, 2.0, FPB, 4.0, false);
    e.now = 3 * BAR;
    s.tick(&mut e);
    // Rien n'est garde, rien ne joue.
    assert!(s.clip(0, 0).is_none());
    assert!(s.cols[0].run.rec.is_none() && s.cols[0].run.play.is_none());
}

#[test]
fn relecture_deplacer_dans_sa_colonne_suit_ce_qui_joue() {
    let (mut s, mut e) = setup();
    let x = s.fresh_id();
    s.put(0, 2, Clip::midi(x, 1.0, notes(&[(0.0, 1.0, 60)])));
    let y = s.fresh_id();
    s.put(0, 0, Clip::midi(y, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.click(&mut e, 0, 2);
    e.now = lat() + 10;
    s.tick(&mut e);
    // Y glisse sur la case 2 (qui joue X) : X part en 0 et joue toujours.
    s.move_cell(&mut e, (0, 0), (0, 2), false);
    assert_eq!(s.clip(0, 0).unwrap().id, x);
    assert_eq!(s.state(0, 0), CellState::Playing);
    assert_eq!(s.state(0, 2), CellState::Stopped);
}

#[test]
fn relecture_un_one_shot_continue_pendant_qu_une_case_attend() {
    let (mut s, mut e) = setup();
    e.srcs.insert(
        "C:/x/hit.wav".into(),
        AudioSrc {
            clip: 7,
            frames: 4800,
            srate: 48000.0,
            file_bpm: None,
        },
    );
    let id = s.fresh_id();
    let mut c = Clip::audio(id, "C:/x/hit.wav");
    c.bars = 1.0;
    if let Kind::Audio(a) = &mut c.kind {
        a.looped = false;
        a.tempo = TempoMode::None;
    }
    s.put(1, 0, c);
    let mut c2 = Clip::audio(s.fresh_id(), "C:/x/hit.wav");
    c2.bars = 1.0;
    s.put(1, 1, c2);
    s.q = Quant::Bar4;
    s.click(&mut e, 1, 0);
    let t0 = lat();
    e.now = t0 + BAR / 2;
    s.tick(&mut e);
    s.click(&mut e, 1, 1);
    e.take();
    for k in 0..3 {
        e.now = t0 + k * BAR + BAR - 1000;
        s.tick(&mut e);
    }
    let all = e.take();
    let plays = all
        .iter()
        .filter(|c| matches!(c, Call::VoicePlay { .. }))
        .count();
    // Trois passes de la case 0 avant sa releve (a 4 mesures).
    assert_eq!(plays, 3);
}

#[test]
fn relecture_editer_un_clip_qui_attend() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    let b = s.fresh_id();
    s.put(0, 1, Clip::midi(b, 1.0, notes(&[(0.0, 1.0, 62)])));
    s.click(&mut e, 0, 0);
    e.now = lat() + BAR / 2;
    s.tick(&mut e);
    s.click(&mut e, 0, 1);
    let at = s.cols[0].run.next.as_ref().unwrap().start_f;
    e.take();
    if let Some(c) = s.clip_mut(0, 1) {
        c.kind = Kind::Midi(notes(&[(0.0, 1.0, 62), (2.0, 1.0, 65)]));
    }
    s.clip_changed(&mut e, 0, 1);
    let c = e.take();
    assert!(c.contains(&Call::LaneCancel { port: 1 }));
    assert!(
        c.iter()
            .any(|x| matches!(x, Call::LanePlay { at: f, .. } if *f == at))
    );
}

#[test]
fn le_tempo_change_et_les_lanes_suivent() {
    let (mut s, mut e) = setup();
    let a = s.fresh_id();
    s.put(0, 0, Clip::midi(a, 1.0, notes(&[(0.0, 1.0, 60)])));
    s.click(&mut e, 0, 0);
    e.now = lat() + 10;
    s.tick(&mut e);
    e.take();
    let n = e.now;
    s.sync_clock(&mut e, n, 0.0, 20000.0, 4.0, false);
    assert!(e.take().contains(&Call::LaneRate {
        port: 1,
        fpb: 20000.0
    }));
    assert!((s.bpm - 144.0).abs() < 1e-9);
}
