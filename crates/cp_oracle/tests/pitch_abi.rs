//! L'etireur de REAPER (`IReaperPitchShift`) appele par notre table
//! virtuelle (`cp_sys::pitch`), confronte a une imitation compilee par le
//! vrai compilateur C++ contre le vrai SDK (`cpp/pitch_abi.cpp`). Si l'ordre
//! des cases etait faux, REAPER executerait une autre methode que celle
//! qu'on croit appeler.

use std::ffi::c_void;

use cp_oracle::{Dll, build_pitch_abi};
use cp_sys::pitch::PitchShift;

#[test]
fn chaque_appel_tombe_sur_sa_methode() {
    let p = build_pitch_abi().unwrap_or_else(|e| panic!("{e}"));
    let d = Dll::load(&p).unwrap();
    // SAFETY: signatures de cpp/pitch_abi.cpp.
    let new: extern "C" fn() -> *mut c_void =
        unsafe { std::mem::transmute(d.sym("ps_fake_new").unwrap()) };
    // SAFETY: idem.
    let log: extern "C" fn(*mut f64, i32) -> i32 =
        unsafe { std::mem::transmute(d.sym("ps_fake_log").unwrap()) };
    {
        // SAFETY: un objet neuf, a nous.
        let mut ps = unsafe { PitchShift::from_raw(new()) }.unwrap();
        ps.set_srate(48000.0);
        ps.set_nch(2);
        ps.set_shift(1.5);
        ps.set_formant_shift(-1.0);
        ps.set_tempo(0.75);
        ps.reset();
        ps.feed(&[0.25, -0.25, 0.5, -0.5, 0.75, -0.75], 2);
        ps.flush();
        assert!(ps.is_reset());
        let mut out = [0.0; 8];
        assert_eq!(ps.take(&mut out, 2), 3);
        assert_eq!(&out[..6], &[0.25, -0.25, 0.5, -0.5, 0.75, -0.75]);
        ps.set_quality(-1);
        assert_eq!(ps.extended(0x100), 42);
    }
    let mut v = [0.0; 256];
    let n = log(v.as_mut_ptr(), 256) as usize;
    let got: Vec<(i32, f64)> = v[..n].chunks(2).map(|c| (c[0] as i32, c[1])).collect();
    assert_eq!(
        got,
        vec![
            (1, 48000.0),
            (2, 2.0),
            (3, 1.5),
            (4, -1.0),
            (5, 0.75),
            (6, 0.0),
            (7, 3.0),
            (8, 3.0),
            (9, 0.0),
            (10, 0.0),
            (11, 4.0),
            (12, -1.0),
            (13, 256.0),
            (100, 0.0),
        ]
    );
}
