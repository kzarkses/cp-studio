//! L'entree MIDI de REAPER (`midi_Input`, `MIDI_eventlist`) lue par nos
//! tables virtuelles (`cp_sys::midi_in`), confrontee a une imitation
//! compilee contre le vrai SDK (`cpp/midi_abi.cpp`).

use std::ffi::c_void;

use cp_oracle::{Dll, build_midi_abi};
use cp_sys::midi_in::MidiInputs;

#[test]
fn les_evenements_du_bloc_se_lisent() {
    let p = build_midi_abi().unwrap_or_else(|e| panic!("{e}"));
    let d = Dll::load(&p).unwrap();
    // SAFETY: signatures de cpp/midi_abi.cpp.
    let fill: extern "C" fn() = unsafe { std::mem::transmute(d.sym("mi_fill").unwrap()) };
    // SAFETY: idem.
    let get: unsafe extern "C" fn(i32) -> *mut c_void =
        unsafe { std::mem::transmute(d.sym("mi_get").unwrap()) };
    fill();
    let m = MidiInputs::with(get);
    let mut got: Vec<(i32, Vec<u8>)> = Vec::new();
    m.for_each(2, |off, msg| got.push((off, msg.to_vec())));
    assert_eq!(
        got,
        vec![
            (0, vec![0xF8, 0, 0]),
            (21333, vec![0xF8, 0, 0]),
            (512000, vec![0xFA, 0, 0]),
            (600000, vec![0xF2, 0x10, 0x02]),
            (700000, vec![0x90, 60, 100]),
        ]
    );
    // Une entree qui n'existe pas : rien.
    let mut none = 0;
    m.for_each(1, |_, _| none += 1);
    assert_eq!(none, 0);
}
