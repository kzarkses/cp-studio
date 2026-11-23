//! Nos tables virtuelles et nos structures, confrontees au vrai compilateur
//! C++ et au vrai SDK de REAPER (`cpp/pcm_abi.cpp`).
//!
//! Si un de ces tests casse, REAPER appellerait la mauvaise fonction de notre
//! source, ou lirait un champ au mauvais endroit : on le sait ici, pas dans
//! REAPER.

use std::ffi::c_void;
use std::mem::{offset_of, size_of};

use cp_oracle::{Dll, build_pcm_abi};
use cp_sys::audio::{AudioHookRegister, CriticalSection, PreviewRegister};
use cp_sys::pcm::{
    Block, CppSource, MidiEventT, PeakTransferHead, PortRender, PortSource, Transfer,
};

fn dll() -> Dll {
    let p = build_pcm_abi().unwrap_or_else(|e| panic!("{e}"));
    Dll::load(&p).unwrap()
}

#[test]
fn les_structures_ont_la_disposition_du_sdk() {
    let d = dll();
    // SAFETY: signature de cpp/pcm_abi.cpp.
    let layout: extern "C" fn(*mut i64, i32) -> i32 =
        unsafe { std::mem::transmute(d.sym("abi_layout").unwrap()) };
    let mut v = [0i64; 64];
    let n = layout(v.as_mut_ptr(), 64) as usize;
    let rust: Vec<i64> = vec![
        size_of::<Transfer>() as i64,
        offset_of!(Transfer, time_s) as i64,
        offset_of!(Transfer, samplerate) as i64,
        offset_of!(Transfer, nch) as i64,
        offset_of!(Transfer, length) as i64,
        offset_of!(Transfer, samples) as i64,
        offset_of!(Transfer, samples_out) as i64,
        offset_of!(Transfer, midi_events) as i64,
        offset_of!(Transfer, approximate_playback_latency) as i64,
        offset_of!(Transfer, absolute_time_s) as i64,
        offset_of!(Transfer, force_bpm) as i64,
        size_of::<PreviewRegister>() as i64,
        offset_of!(PreviewRegister, cs) as i64,
        offset_of!(PreviewRegister, src) as i64,
        offset_of!(PreviewRegister, m_out_chan) as i64,
        offset_of!(PreviewRegister, curpos) as i64,
        offset_of!(PreviewRegister, loop_) as i64,
        offset_of!(PreviewRegister, volume) as i64,
        offset_of!(PreviewRegister, peakvol) as i64,
        offset_of!(PreviewRegister, preview_track) as i64,
        size_of::<AudioHookRegister>() as i64,
        offset_of!(AudioHookRegister, on_audio_buffer) as i64,
        offset_of!(AudioHookRegister, userdata1) as i64,
        offset_of!(AudioHookRegister, userdata2) as i64,
        offset_of!(AudioHookRegister, input_nch) as i64,
        offset_of!(AudioHookRegister, output_nch) as i64,
        offset_of!(AudioHookRegister, get_buffer) as i64,
        offset_of!(PeakTransferHead, start_time) as i64,
        offset_of!(PeakTransferHead, peakrate) as i64,
        offset_of!(PeakTransferHead, numpeak_points) as i64,
        offset_of!(PeakTransferHead, nchpeaks) as i64,
        offset_of!(PeakTransferHead, peaks) as i64,
        offset_of!(PeakTransferHead, peaks_out) as i64,
        size_of::<CriticalSection>() as i64,
        size_of::<f64>() as i64,
        size_of::<MidiEventT>() as i64,
        offset_of!(MidiEventT, frame_offset) as i64,
        offset_of!(MidiEventT, size) as i64,
        offset_of!(MidiEventT, midi_message) as i64,
    ];
    assert_eq!(n, rust.len(), "nombre de mesures");
    assert_eq!(&v[..n], &rust[..]);
}

struct Ramp;

impl PortRender for Ramp {
    fn render(&self, b: &mut Block<'_>) {
        for i in 0..b.frames {
            for c in 0..b.nch {
                b.out[i * b.nch + c] =
                    b.time_s * 1000.0 + i as f64 + c as f64 * 0.5 + b.srate * 1e-6;
            }
        }
    }
    fn channels(&self) -> i32 {
        2
    }
    fn sample_rate(&self) -> f64 {
        44100.0
    }
}

#[test]
fn le_cpp_appelle_notre_source_par_ses_methodes() {
    let d = dll();
    // SAFETY: signature de cpp/pcm_abi.cpp.
    type Drive =
        extern "C" fn(*mut c_void, *mut f64, i32, i32, f64, f64, *mut f64, *mut u8, i32) -> i32;
    // SAFETY: voir ci-dessus.
    let drive: Drive = unsafe { std::mem::transmute(d.sym("abi_drive").unwrap()) };
    let src = PortSource::new(Box::new(Ramp));
    let (frames, nch) = (5usize, 3usize);
    let mut buf = vec![-1.0f64; frames * nch];
    let mut info = [f64::NAN; 32];
    let mut ty = [0u8; 32];
    let k = drive(
        src.as_ptr(),
        buf.as_mut_ptr(),
        frames as i32,
        nch as i32,
        1.25,
        96000.0,
        info.as_mut_ptr(),
        ty.as_mut_ptr(),
        32,
    ) as usize;
    assert_eq!(k, 20);
    let want = [
        1.0,     // IsAvailable
        2.0,     // GetNumChannels
        44100.0, // GetSampleRate
        1.0e9,   // GetLength
        -1.0,    // GetLengthBeats
        0.0,     // GetBitsPerSample
        -1.0,    // GetPreferredPosition
        1.0,     // Duplicate == null
        1.0,     // GetFileName == null
        0.0,     // SetFileName
        1.0,     // GetSource == null
        0.0,     // PropertiesWindow
        -1.0,    // LoadState
        0.0,     // PeaksBuild_Begin
        0.0,     // PeaksBuild_Run
        0.0,     // Extended
        0.0,     // GetPeakInfo -> peaks_out
        5.0,     // samples_out
        0.0,     // approximate_playback_latency
        7.0,     // absolute_time_s laisse tel quel
    ];
    assert_eq!(&info[..k], &want[..]);
    let end = ty.iter().position(|&b| b == 0).unwrap();
    assert_eq!(std::str::from_utf8(&ty[..end]).unwrap(), "CP_Port");
    for i in 0..frames {
        for c in 0..nch {
            let w = 1250.0 + i as f64 + c as f64 * 0.5 + 0.096;
            assert_eq!(buf[i * nch + c], w, "frame {i} canal {c}");
        }
    }
}

#[test]
fn nous_appelons_une_source_cpp_par_sa_table() {
    let d = dll();
    // SAFETY: signatures de cpp/pcm_abi.cpp.
    let make: extern "C" fn() -> *mut c_void =
        unsafe { std::mem::transmute(d.sym("abi_make_source").unwrap()) };
    // SAFETY: voir ci-dessus.
    let destroy: unsafe extern "C" fn(*mut c_void) =
        unsafe { std::mem::transmute(d.sym("abi_destroy_source").unwrap()) };
    // SAFETY: voir ci-dessus.
    let destroyed: extern "C" fn() -> i32 =
        unsafe { std::mem::transmute(d.sym("abi_destroyed").unwrap()) };

    let before = destroyed();
    // SAFETY: source neuve, liberee par abi_destroy_source.
    let mut s = unsafe { CppSource::from_raw(make(), Some(destroy)) }.unwrap();
    assert!(s.is_available());
    assert_eq!(s.type_name(), "PROBE");
    assert_eq!(s.channels(), 3);
    assert_eq!(s.sample_rate(), 44100.0);
    assert_eq!(s.length(), 2.5);
    assert_eq!(s.bits_per_sample(), 24);

    let mut out = vec![0.0f64; 4 * 3];
    let got = s.read(1.0, 48000.0, 3, &mut out);
    assert_eq!(got, 3, "samples_out rendu par la source");
    for i in 0..4 {
        for c in 0..3 {
            let w = 1.0 + i as f64 * 0.001 + c as f64 * 10.0 + 0.048;
            assert_eq!(out[i * 3 + c], w);
        }
    }

    // La methode virtuelle ajoutee par la classe derivee suit les 25 cases de
    // PCM_source : la table a donc exactement ce nombre de cases.
    let table = s.vtbl() as *const _ as *const usize;
    // SAFETY: la table de ProbeSource a 26 cases ; la 26e est Extra(), qui ne
    // prend que `this`.
    let extra: extern "C" fn(*mut c_void) -> i32 =
        unsafe { std::mem::transmute(*table.add(cp_sys::pcm::VTBL_SLOTS)) };
    assert_eq!(extra(s.as_ptr()), 77);

    drop(s);
    assert_eq!(destroyed(), before + 1);
}

/// Une source qui pose trois messages dans la liste MIDI du bloc.
struct Notes;

impl PortRender for Notes {
    fn render(&self, b: &mut Block<'_>) {
        b.out.fill(0.0);
        if let Some(m) = b.midi.as_mut() {
            assert!(m.add(3, &[0x90, 60, 100]));
            assert!(m.add(7, &[0x80, 60, 0]));
            // Hors du bloc : ramene a sa derniere frame.
            assert!(m.add(999, &[0xB0, 64, 127]));
            assert_eq!(m.size_bytes(), 3 * size_of::<MidiEventT>() as i32);
        }
    }
    fn sample_rate(&self) -> f64 {
        48000.0
    }
}

#[test]
fn notre_source_ecrit_dans_la_liste_midi_de_reaper() {
    let d = dll();
    type DriveMidi = extern "C" fn(*mut c_void, *mut f64, i32, i32, *mut i32, i32) -> i32;
    // SAFETY: signature de cpp/pcm_abi.cpp.
    let drive: DriveMidi = unsafe { std::mem::transmute(d.sym("abi_drive_midi").unwrap()) };
    let src = PortSource::new(Box::new(Notes));
    let mut buf = vec![0.0f64; 16 * 2];
    let mut out = [0i32; 32];
    let n = drive(src.as_ptr(), buf.as_mut_ptr(), 16, 2, out.as_mut_ptr(), 32);
    assert_eq!(n, 3);
    assert_eq!(
        &out[..15],
        &[3, 3, 0x90, 60, 100, 7, 3, 0x80, 60, 0, 15, 3, 0xB0, 64, 127]
    );
}
