//! Les images de disque (PNG, JPG, BMP...) par Windows Imaging Component :
//! aucune dependance, et le decodeur que Windows tient a jour.
//!
//! Le resultat est une `cp_gfx::Surface` en BGRA PREMULTIPLIE, le format du
//! moteur de dessin (WIC convertit lui-meme : `32bppPBGRA`).

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use cp_gfx::Surface;

use crate::win::wide_os;

type Raw = *mut c_void;

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

const CLSID_WIC_FACTORY: Guid = Guid(
    0xcacaf262,
    0x9370,
    0x4615,
    [0xa1, 0x3b, 0x9f, 0x55, 0x39, 0xda, 0x4c, 0x0a],
);
const IID_WIC_FACTORY: Guid = Guid(
    0xec5ec8a9,
    0xc395,
    0x4314,
    [0x9c, 0x77, 0x54, 0xd7, 0xa9, 0x35, 0xff, 0x70],
);
const PBGRA32: Guid = Guid(
    0x6fddc324,
    0x4e03,
    0x4bfe,
    [0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9, 0x10],
);

const CLSCTX_INPROC_SERVER: u32 = 1;
const COINIT_APARTMENTTHREADED: u32 = 2;
const GENERIC_READ: u32 = 0x8000_0000;

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: Raw, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: Raw,
        ctx: u32,
        iid: *const Guid,
        out: *mut Raw,
    ) -> i32;
}

/// La methode `i` de la table virtuelle d'un objet COM.
///
/// # Safety
/// `obj` est un objet COM vivant dont la table a au moins `i + 1` entrees.
unsafe fn method(obj: Raw, i: usize) -> *const c_void {
    // SAFETY: un objet COM commence par un pointeur vers sa table.
    unsafe { *(*(obj as *const *const *const c_void)).add(i) }
}

/// Un objet COM rendu a sa destruction.
struct Com(Raw);

impl Drop for Com {
    fn drop(&mut self) {
        if !self.0.is_null() {
            type Release = unsafe extern "system" fn(Raw) -> u32;
            // SAFETY: IUnknown::Release est l'entree 2 de toute table COM.
            unsafe {
                let f: Release = std::mem::transmute::<*const c_void, Release>(method(self.0, 2));
                f(self.0);
            }
        }
    }
}

/// Decode un fichier image. `None` si Windows ne sait pas le lire.
pub fn load(path: &Path) -> Option<Surface> {
    // SAFETY: COM s'initialise par fil ; S_FALSE (deja fait) ou un autre
    // modele deja pose (RPC_E_CHANGED_MODE) laissent COM utilisable.
    let hr = unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) };
    let out = decode(path);
    if hr >= 0 {
        // SAFETY: rend l'initialisation reussie juste au-dessus.
        unsafe { CoUninitialize() };
    }
    out
}

fn decode(path: &Path) -> Option<Surface> {
    let name = wide_os(path.as_os_str());
    let mut f: Raw = null_mut();
    // SAFETY: GUID statiques, sortie ecrite par COM.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_WIC_FACTORY,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_WIC_FACTORY,
            &mut f,
        )
    };
    if hr < 0 || f.is_null() {
        return None;
    }
    let factory = Com(f);

    // IWICImagingFactory::CreateDecoderFromFilename (3)
    type FromFile =
        unsafe extern "system" fn(Raw, *const u16, *const Guid, u32, u32, *mut Raw) -> i32;
    let mut d: Raw = null_mut();
    // SAFETY: entree 3 de IWICImagingFactory ; chemin termine par zero.
    let hr = unsafe {
        let m: FromFile = std::mem::transmute::<*const c_void, FromFile>(method(factory.0, 3));
        m(
            factory.0,
            name.as_ptr(),
            std::ptr::null(),
            GENERIC_READ,
            0,
            &mut d,
        )
    };
    if hr < 0 || d.is_null() {
        return None;
    }
    let decoder = Com(d);

    // IWICBitmapDecoder::GetFrame (13)
    type GetFrame = unsafe extern "system" fn(Raw, u32, *mut Raw) -> i32;
    let mut fr: Raw = null_mut();
    // SAFETY: entree 13 de IWICBitmapDecoder.
    let hr = unsafe {
        let m: GetFrame = std::mem::transmute::<*const c_void, GetFrame>(method(decoder.0, 13));
        m(decoder.0, 0, &mut fr)
    };
    if hr < 0 || fr.is_null() {
        return None;
    }
    let frame = Com(fr);

    // IWICImagingFactory::CreateFormatConverter (10)
    type NewConv = unsafe extern "system" fn(Raw, *mut Raw) -> i32;
    let mut c: Raw = null_mut();
    // SAFETY: entree 10 de IWICImagingFactory.
    let hr = unsafe {
        let m: NewConv = std::mem::transmute::<*const c_void, NewConv>(method(factory.0, 10));
        m(factory.0, &mut c)
    };
    if hr < 0 || c.is_null() {
        return None;
    }
    let conv = Com(c);

    // IWICFormatConverter::Initialize (8)
    type Init = unsafe extern "system" fn(Raw, Raw, *const Guid, u32, Raw, f64, u32) -> i32;
    // SAFETY: entree 8 de IWICFormatConverter ; source = la trame.
    let hr = unsafe {
        let m: Init = std::mem::transmute::<*const c_void, Init>(method(conv.0, 8));
        m(conv.0, frame.0, &PBGRA32, 0, null_mut(), 0.0, 0)
    };
    if hr < 0 {
        return None;
    }

    // IWICBitmapSource::GetSize (3) et CopyPixels (7)
    type GetSize = unsafe extern "system" fn(Raw, *mut u32, *mut u32) -> i32;
    let (mut w, mut h) = (0u32, 0u32);
    // SAFETY: entree 3 de IWICBitmapSource.
    let hr = unsafe {
        let m: GetSize = std::mem::transmute::<*const c_void, GetSize>(method(conv.0, 3));
        m(conv.0, &mut w, &mut h)
    };
    if hr < 0 || w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let mut px = vec![0u32; (w * h) as usize];
    type Copy = unsafe extern "system" fn(Raw, *const c_void, u32, u32, *mut u8) -> i32;
    // SAFETY: entree 7 de IWICBitmapSource ; le tampon fait w*h*4 octets.
    let hr = unsafe {
        let m: Copy = std::mem::transmute::<*const c_void, Copy>(method(conv.0, 7));
        m(
            conv.0,
            std::ptr::null(),
            w * 4,
            w * h * 4,
            px.as_mut_ptr() as *mut u8,
        )
    };
    if hr < 0 {
        return None;
    }
    Some(Surface {
        w: w as i32,
        h: h as i32,
        px,
    })
}
