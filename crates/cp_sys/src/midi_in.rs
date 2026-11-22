//! L'entree MIDI d'un peripherique, lue dans le crochet audio : ce que la
//! clock externe ecoute (0xF8, Start, Continue, Stop, SPP).
//!
//! `GetMidiInput(idx)` (hors du SDK publie, par GetFunc) rend l'objet C++
//! `midi_Input` de REAPER ; `GetReadBuf()` donne la liste des evenements du
//! bloc en cours, dates en 1/1 024 000 de seconde depuis son debut. Les deux
//! classes s'appellent par leur table virtuelle, dans l'ordre de
//! `reaper_plugin.h` ; `cp_oracle/tests/midi_abi.rs` confronte cet ordre au
//! vrai compilateur. Fil audio seulement (c'est la que REAPER remplit les
//! tampons).

use std::ffi::c_void;

use crate::reaper::Reaper;

/// Un evenement de la liste (`MIDI_event_t`) : `size` octets de message, au
/// moins trois (plus pour un sysex).
#[repr(C)]
pub struct MidiEvent {
    /// En 1/1 024 000 s depuis le debut du bloc.
    pub frame_offset: i32,
    pub size: i32,
    pub msg: [u8; 4],
}

/// La table de `midi_Input` (toutes les cases : la disposition compte).
#[repr(C)]
#[allow(dead_code)]
struct InVtbl {
    dtor: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    start: unsafe extern "C" fn(*mut c_void),
    stop: unsafe extern "C" fn(*mut c_void),
    swap_bufs: unsafe extern "C" fn(*mut c_void, u32),
    run_pre: unsafe extern "C" fn(*mut c_void, i32),
    get_read_buf: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
}

/// La table de `MIDI_eventlist`.
#[repr(C)]
#[allow(dead_code)]
struct ListVtbl {
    add_item: unsafe extern "C" fn(*mut c_void, *mut MidiEvent),
    enum_items: unsafe extern "C" fn(*mut c_void, *mut i32) -> *mut MidiEvent,
    delete_item: unsafe extern "C" fn(*mut c_void, i32),
    get_size: unsafe extern "C" fn(*mut c_void) -> i32,
    empty: unsafe extern "C" fn(*mut c_void),
}

/// L'acces aux entrees MIDI (une fonction de REAPER, copiable dans le fil
/// audio).
#[derive(Clone, Copy)]
pub struct MidiInputs {
    get: Option<unsafe extern "C" fn(i32) -> *mut c_void>,
}

impl MidiInputs {
    pub fn new(r: &Reaper) -> MidiInputs {
        type Get = unsafe extern "C" fn(i32) -> *mut c_void;
        // SAFETY: midi_Input* GetMidiInput(int idx).
        let get = unsafe { r.api::<Get>(c"GetMidiInput") };
        MidiInputs { get }
    }

    /// Les tests : un objet `midi_Input` fabrique ailleurs, rendu par `get`.
    pub fn with(get: unsafe extern "C" fn(i32) -> *mut c_void) -> MidiInputs {
        MidiInputs { get: Some(get) }
    }

    /// Fil audio : chaque evenement du bloc en cours sur l'entree `dev`
    /// (decalage en 1/1 024 000 s, octets du message).
    pub fn for_each(&self, dev: i32, mut f: impl FnMut(i32, &[u8])) {
        let Some(get) = self.get else { return };
        // SAFETY: la fonction de REAPER (ou du test) rend un objet vivant le
        // temps du bloc, ou nul.
        let inp = unsafe { get(dev) };
        if inp.is_null() {
            return;
        }
        // SAFETY: un objet C++ polymorphe commence par sa table.
        let list = unsafe {
            let vt = &**(inp as *const *const InVtbl);
            (vt.get_read_buf)(inp)
        };
        if list.is_null() {
            return;
        }
        // SAFETY: idem ; la liste rend ses evenements jusqu'a nul, `bpos`
        // avance seul ; chaque evenement tient `size` octets de message.
        unsafe {
            let vt = &**(list as *const *const ListVtbl);
            let mut bpos = 0i32;
            for _ in 0..100_000 {
                let ev = (vt.enum_items)(list, &mut bpos);
                if ev.is_null() {
                    break;
                }
                let n = (*ev).size.clamp(0, 1 << 20) as usize;
                let msg = std::slice::from_raw_parts((*ev).msg.as_ptr(), n);
                f((*ev).frame_offset, msg);
            }
        }
    }
}
