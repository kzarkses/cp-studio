//! Les classes audio de REAPER : `PCM_source`, dans les deux sens.
//!
//! - [`PortSource`] : NOTRE source, que REAPER appelle comme un objet C++. Un
//!   aperçu la tire bloc par bloc dans le fil audio ; elle demande a un
//!   [`PortRender`] (le moteur) de remplir le bloc.
//! - [`CppSource`] : une source de REAPER (un fichier ouvert par
//!   `PCM_Source_CreateFromFileEx`), que NOUS appelons comme un objet C++.
//!
//! Un objet C++ a heritage simple et fonctions virtuelles, compile par MSVC x64,
//! commence par un pointeur vers sa table virtuelle : un tableau de pointeurs de
//! fonctions, dans l'ordre de DECLARATION des methodes virtuelles de la classe
//! (`reaper_plugin.h`). Le destructeur virtuel n'y occupe qu'UNE case (le
//! « scalar deleting destructor », qui prend un drapeau). Sur x64, `this` est
//! simplement le premier argument, et la convention d'appel est celle de C.
//! Aucune surcharge dans `PCM_source` : MSVC regroupe les surcharges, ce qui
//! changerait l'ordre.
//!
//! Tout cela est VERIFIE contre le vrai compilateur : le test
//! `cp_oracle/tests/pcm_abi.rs` compile un programme C++ contre le SDK qui
//! appelle notre source par ses methodes C++, et qui nous tend une source C++
//! que nous appelons par cette table.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;

use crate::guard;

/// `ReaSample` : `double` (REASAMPLE_SIZE 8, le defaut du SDK).
pub type ReaSample = f64;

/// `PCM_source_transfer_t`.
#[repr(C)]
#[derive(Debug)]
pub struct Transfer {
    pub time_s: f64,
    pub samplerate: f64,
    pub nch: i32,
    pub length: i32,
    pub samples: *mut ReaSample,
    pub samples_out: i32,
    pub midi_events: *mut c_void,
    pub approximate_playback_latency: f64,
    pub absolute_time_s: f64,
    pub force_bpm: f64,
}

impl Transfer {
    pub fn new(time_s: f64, samplerate: f64, nch: i32, length: i32, samples: *mut f64) -> Self {
        Transfer {
            time_s,
            samplerate,
            nch,
            length,
            samples,
            samples_out: 0,
            midi_events: null_mut(),
            approximate_playback_latency: 0.0,
            absolute_time_s: 0.0,
            force_bpm: 0.0,
        }
    }
}

/// Le debut de `PCM_source_peaktransfer_t` : on n'y ecrit que `peaks_out`.
#[repr(C)]
pub struct PeakTransferHead {
    pub start_time: f64,
    pub peakrate: f64,
    pub numpeak_points: i32,
    pub nchpeaks: i32,
    pub peaks: *mut ReaSample,
    pub peaks_out: i32,
}

/// La table virtuelle de `class PCM_source`, case par case.
#[repr(C)]
pub struct Vtbl {
    /// `virtual ~PCM_source()` (scalar deleting destructor : drapeau &1 = liberer).
    pub dtor: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    pub duplicate: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub is_available: unsafe extern "C" fn(*mut c_void) -> bool,
    pub set_available: unsafe extern "C" fn(*mut c_void, bool),
    pub get_type: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    pub get_file_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    pub set_file_name: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    pub get_source: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub set_source: unsafe extern "C" fn(*mut c_void, *mut c_void),
    pub get_num_channels: unsafe extern "C" fn(*mut c_void) -> i32,
    pub get_sample_rate: unsafe extern "C" fn(*mut c_void) -> f64,
    pub get_length: unsafe extern "C" fn(*mut c_void) -> f64,
    pub get_length_beats: unsafe extern "C" fn(*mut c_void) -> f64,
    pub get_bits_per_sample: unsafe extern "C" fn(*mut c_void) -> i32,
    pub get_preferred_position: unsafe extern "C" fn(*mut c_void) -> f64,
    pub properties_window: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32,
    pub get_samples: unsafe extern "C" fn(*mut c_void, *mut Transfer),
    pub get_peak_info: unsafe extern "C" fn(*mut c_void, *mut PeakTransferHead),
    pub save_state: unsafe extern "C" fn(*mut c_void, *mut c_void),
    pub load_state: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void) -> i32,
    pub peaks_clear: unsafe extern "C" fn(*mut c_void, bool),
    pub peaks_build_begin: unsafe extern "C" fn(*mut c_void) -> i32,
    pub peaks_build_run: unsafe extern "C" fn(*mut c_void) -> i32,
    pub peaks_build_finish: unsafe extern "C" fn(*mut c_void),
    pub extended:
        unsafe extern "C" fn(*mut c_void, i32, *mut c_void, *mut c_void, *mut c_void) -> i32,
}

/// Le nombre de cases de la table (verifie par le programme C++).
pub const VTBL_SLOTS: usize = 25;
const _: () = assert!(std::mem::size_of::<Vtbl>() == VTBL_SLOTS * 8);

// ---------------------------------------------------------------------------
// Notre source, tiree par REAPER
// ---------------------------------------------------------------------------

/// Un bloc a remplir, vu sans pointeur brut.
pub struct Block<'a> {
    /// Le temps DEMANDE par l'hote (pas un compteur).
    pub time_s: f64,
    pub srate: f64,
    pub nch: usize,
    pub frames: usize,
    /// `frames * nch` echantillons entrelaces, a ecraser.
    pub out: &'a mut [ReaSample],
}

/// Ce qui remplit le bloc. Appele dans le FIL AUDIO : n'alloue pas, ne bloque
/// pas, ne panique pas (une panique est rattrapee et rend du silence).
pub trait PortRender: Send + Sync {
    fn render(&self, block: &mut Block<'_>);
    fn channels(&self) -> i32 {
        2
    }
    fn sample_rate(&self) -> f64;
}

#[repr(C)]
struct PortRaw {
    vtbl: *const Vtbl,
    render: Box<dyn PortRender>,
}

/// Une `PCM_source` dont le rendu est un [`PortRender`]. L'adresse est stable
/// (boite) : c'est elle que REAPER garde. La liberer pendant que REAPER la tire
/// serait fatal : son proprietaire (l'aperçu) l'arrete d'abord.
pub struct PortSource {
    raw: Box<PortRaw>,
}

// SAFETY: `PortRaw` ne contient qu'un pointeur vers une table statique et un
// `Box<dyn PortRender>` lui-meme `Send + Sync`.
unsafe impl Send for PortSource {}
// SAFETY: idem ; les appels de REAPER ne prennent que `&` sur le rendu.
unsafe impl Sync for PortSource {}

impl PortSource {
    pub fn new(render: Box<dyn PortRender>) -> PortSource {
        PortSource {
            raw: Box::new(PortRaw {
                vtbl: &PORT_VTBL,
                render,
            }),
        }
    }

    /// Le `PCM_source*` a donner a REAPER.
    pub fn as_ptr(&self) -> *mut c_void {
        &*self.raw as *const PortRaw as *mut c_void
    }

    /// Tire un bloc PAR LA TABLE VIRTUELLE, exactement comme REAPER (tests, faux
    /// REAPER). Rend `samples_out`.
    pub fn pull(&self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize {
        if nch == 0 {
            return 0;
        }
        let frames = out.len() / nch;
        let mut t = Transfer::new(time_s, srate, nch as i32, frames as i32, out.as_mut_ptr());
        // SAFETY: notre objet, vivant ; la table est lue dans l'objet comme le
        // ferait REAPER ; le tampon tient `frames * nch` echantillons.
        unsafe {
            let vt = &**(self.as_ptr() as *const *const Vtbl);
            (vt.get_samples)(self.as_ptr(), &mut t);
        }
        t.samples_out.max(0) as usize
    }
}

/// # Safety
/// `this` est un `PortRaw` vivant (REAPER n'appelle que la source qu'on lui a
/// donnee, et on ne la libere qu'apres l'avoir arretee).
unsafe fn port<'a>(this: *mut c_void) -> &'a PortRaw {
    // SAFETY: voir la fonction.
    unsafe { &*(this as *const PortRaw) }
}

static PORT_TYPE: &CStr = c"CP_Port";

unsafe extern "C" fn port_dtor(this: *mut c_void, _flags: u32) -> *mut c_void {
    // REAPER ne detruit jamais la source d'un aperçu ; si quelqu'un le faisait,
    // on ne libere rien : la memoire est a nous.
    this
}
unsafe extern "C" fn port_duplicate(_: *mut c_void) -> *mut c_void {
    null_mut()
}
unsafe extern "C" fn port_true(_: *mut c_void) -> bool {
    true
}
unsafe extern "C" fn port_set_available(_: *mut c_void, _: bool) {}
unsafe extern "C" fn port_get_type(_: *mut c_void) -> *const c_char {
    PORT_TYPE.as_ptr()
}
unsafe extern "C" fn port_null_str(_: *mut c_void) -> *const c_char {
    std::ptr::null()
}
unsafe extern "C" fn port_set_file_name(_: *mut c_void, _: *const c_char) -> bool {
    false
}
unsafe extern "C" fn port_null_src(_: *mut c_void) -> *mut c_void {
    null_mut()
}
unsafe extern "C" fn port_set_source(_: *mut c_void, _: *mut c_void) {}
unsafe extern "C" fn port_channels(this: *mut c_void) -> i32 {
    // SAFETY: appele par REAPER sur notre source.
    let p = unsafe { port(this) };
    guard(|| p.render.channels()).unwrap_or(2)
}
unsafe extern "C" fn port_srate(this: *mut c_void) -> f64 {
    // SAFETY: appele par REAPER sur notre source.
    let p = unsafe { port(this) };
    guard(|| p.render.sample_rate()).unwrap_or(48000.0)
}
/// Un aperçu permanent n'a pas de fin : une duree enorme (trente ans), pas
/// l'infini (le SDK ne dit pas ce que REAPER en ferait).
unsafe extern "C" fn port_length(_: *mut c_void) -> f64 {
    1.0e9
}
unsafe extern "C" fn port_minus_one(_: *mut c_void) -> f64 {
    -1.0
}
unsafe extern "C" fn port_zero(_: *mut c_void) -> i32 {
    0
}
unsafe extern "C" fn port_properties(_: *mut c_void, _: *mut c_void) -> i32 {
    0
}
unsafe extern "C" fn port_get_samples(this: *mut c_void, block: *mut Transfer) {
    if block.is_null() {
        return;
    }
    // SAFETY: REAPER passe un bloc valide le temps de l'appel.
    let b = unsafe { &mut *block };
    if b.samples.is_null() || b.length <= 0 {
        return;
    }
    // Un bloc sans canaux : son tampon ne tient rien, on n'y ecrit rien.
    if b.nch <= 0 {
        b.samples_out = 0;
        return;
    }
    let nch = b.nch as usize;
    let frames = b.length as usize;
    // SAFETY: l'appelant alloue `length * nch` echantillons (contrat du SDK).
    let out = unsafe { std::slice::from_raw_parts_mut(b.samples, frames * nch) };
    // SAFETY: appele par REAPER sur notre source.
    let p = unsafe { port(this) };
    let mut blk = Block {
        time_s: b.time_s,
        srate: b.samplerate,
        nch,
        frames,
        out,
    };
    if guard(|| p.render.render(&mut blk)).is_none() {
        blk.out.fill(0.0);
    }
    b.samples_out = b.length;
    // Rien n'est mis en tampon entre la demande et la sortie.
    b.approximate_playback_latency = 0.0;
}
unsafe extern "C" fn port_get_peak_info(_: *mut c_void, block: *mut PeakTransferHead) {
    if !block.is_null() {
        // SAFETY: REAPER passe un bloc valide le temps de l'appel.
        unsafe { (*block).peaks_out = 0 };
    }
}
unsafe extern "C" fn port_save_state(_: *mut c_void, _: *mut c_void) {}
unsafe extern "C" fn port_load_state(_: *mut c_void, _: *const c_char, _: *mut c_void) -> i32 {
    -1
}
unsafe extern "C" fn port_peaks_clear(_: *mut c_void, _: bool) {}
unsafe extern "C" fn port_void(_: *mut c_void) {}
unsafe extern "C" fn port_extended(
    _: *mut c_void,
    _: i32,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
) -> i32 {
    0
}

static PORT_VTBL: Vtbl = Vtbl {
    dtor: port_dtor,
    duplicate: port_duplicate,
    is_available: port_true,
    set_available: port_set_available,
    get_type: port_get_type,
    get_file_name: port_null_str,
    set_file_name: port_set_file_name,
    get_source: port_null_src,
    set_source: port_set_source,
    get_num_channels: port_channels,
    get_sample_rate: port_srate,
    get_length: port_length,
    get_length_beats: port_minus_one,
    get_bits_per_sample: port_zero,
    get_preferred_position: port_minus_one,
    properties_window: port_properties,
    get_samples: port_get_samples,
    get_peak_info: port_get_peak_info,
    save_state: port_save_state,
    load_state: port_load_state,
    peaks_clear: port_peaks_clear,
    peaks_build_begin: port_zero,
    peaks_build_run: port_zero,
    peaks_build_finish: port_void,
    extended: port_extended,
};

// ---------------------------------------------------------------------------
// Une source C++ de REAPER, appelee par nous
// ---------------------------------------------------------------------------

/// Comment liberer la source a la fin.
type DestroyFn = unsafe extern "C" fn(*mut c_void);

/// Une `PCM_source` C++ (un fichier ouvert par REAPER). Elle n'est utilisee que
/// par un fil a la fois (`&mut self` partout) ; REAPER lui-meme lit ses sources
/// depuis ses fils de travail.
pub struct CppSource {
    ptr: *mut c_void,
    destroy: Option<DestroyFn>,
}

// SAFETY: une source n'est jamais touchee par deux fils a la fois (`&mut`
// exige partout), et REAPER lit ses sources depuis des fils de travail.
unsafe impl Send for CppSource {}

impl CppSource {
    /// Enveloppe une source existante. `destroy` est appelee au `Drop`.
    ///
    /// # Safety
    /// `ptr` est un `PCM_source*` vivant, possede par l'appelant, et `destroy`
    /// (s'il y en a une) sait le liberer.
    pub unsafe fn from_raw(ptr: *mut c_void, destroy: Option<DestroyFn>) -> Option<CppSource> {
        if ptr.is_null() {
            return None;
        }
        Some(CppSource { ptr, destroy })
    }

    fn vt(&self) -> &Vtbl {
        // SAFETY: un objet C++ polymorphe commence par le pointeur de sa table
        // (contrat de `from_raw`).
        unsafe { &**(self.ptr as *const *const Vtbl) }
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }

    pub fn is_available(&mut self) -> bool {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().is_available)(self.ptr) }
    }

    pub fn type_name(&mut self) -> String {
        // SAFETY: methode virtuelle de l'objet vivant.
        let p = unsafe { (self.vt().get_type)(self.ptr) };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: chaine C tenue par la source.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    pub fn channels(&mut self) -> i32 {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().get_num_channels)(self.ptr) }
    }

    pub fn sample_rate(&mut self) -> f64 {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().get_sample_rate)(self.ptr) }
    }

    pub fn length(&mut self) -> f64 {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().get_length)(self.ptr) }
    }

    pub fn bits_per_sample(&mut self) -> i32 {
        // SAFETY: methode virtuelle de l'objet vivant.
        unsafe { (self.vt().get_bits_per_sample)(self.ptr) }
    }

    /// Lit `out.len() / nch` frames a partir de `time_s`, au taux `srate`.
    /// Rend le nombre de frames rendus (moins a la fin du fichier).
    pub fn read(&mut self, time_s: f64, srate: f64, nch: usize, out: &mut [f64]) -> usize {
        if nch == 0 {
            return 0;
        }
        let frames = out.len() / nch;
        if frames == 0 {
            return 0;
        }
        let mut t = Transfer::new(time_s, srate, nch as i32, frames as i32, out.as_mut_ptr());
        // SAFETY: le tampon tient `frames * nch` echantillons pendant l'appel.
        unsafe { (self.vt().get_samples)(self.ptr, &mut t) };
        t.samples_out.clamp(0, frames as i32) as usize
    }

    /// Appel direct d'une case de la table, pour le test d'ABI.
    pub fn vtbl(&self) -> &Vtbl {
        self.vt()
    }
}

impl Drop for CppSource {
    fn drop(&mut self) {
        if let Some(d) = self.destroy {
            // SAFETY: contrat de `from_raw` : `destroy` libere cette source.
            unsafe { d(self.ptr) }
        }
    }
}
