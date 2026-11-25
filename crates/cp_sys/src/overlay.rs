//! Une image posee par-dessus l'ecran, transparente pixel par pixel, que la
//! souris traverse : les zones de depot d'un glisser d'onglet, le fantome de
//! l'onglet tire.
//!
//! Sa classe est une classe de Windows (« STATIC ») et son image passe par
//! `UpdateLayeredWindow` : Windows ne rappelle jamais notre code pour la
//! peindre. Un module recharge peut donc en tenir une sans rien confier a
//! Windows de son propre code. Les popovers du transport et les infobulles
//! de toutes les vues sont des images posees ainsi.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use crate::win::{Hwnd, wide};

type Raw = *mut c_void;

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Size {
    cx: i32,
    cy: i32,
}

#[repr(C)]
struct Blend {
    op: u8,
    flags: u8,
    alpha: u8,
    format: u8,
}

#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "CreateWindowExW"]
    fn OvCreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Raw,
        menu: Raw,
        inst: Raw,
        param: Raw,
    ) -> Raw;
    #[link_name = "DestroyWindow"]
    fn OvDestroyWindow(h: Raw) -> i32;
    #[link_name = "ShowWindow"]
    fn OvShowWindow(h: Raw, cmd: i32) -> i32;
    #[link_name = "IsWindowVisible"]
    fn OvIsWindowVisible(h: Raw) -> i32;
    #[link_name = "SetWindowPos"]
    fn OvSetWindowPos(h: Raw, after: Raw, x: i32, y: i32, w: i32, hh: i32, flags: u32) -> i32;
    #[link_name = "GetDC"]
    fn OvGetDC(h: Raw) -> Raw;
    #[link_name = "ReleaseDC"]
    fn OvReleaseDC(h: Raw, dc: Raw) -> i32;
    fn UpdateLayeredWindow(
        h: Raw,
        dst: Raw,
        pt_dst: *const Point,
        size: *const Size,
        src: Raw,
        pt_src: *const Point,
        key: u32,
        blend: *const Blend,
        flags: u32,
    ) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    #[link_name = "CreateCompatibleDC"]
    fn OvCreateCompatibleDC(dc: Raw) -> Raw;
    #[link_name = "DeleteDC"]
    fn OvDeleteDC(dc: Raw) -> i32;
    #[link_name = "CreateDIBSection"]
    fn OvCreateDIBSection(
        dc: Raw,
        bi: *const BitmapInfoHeader,
        usage: u32,
        bits: *mut *mut c_void,
        section: Raw,
        offset: u32,
    ) -> Raw;
    #[link_name = "SelectObject"]
    fn OvSelectObject(dc: Raw, obj: Raw) -> Raw;
    #[link_name = "DeleteObject"]
    fn OvDeleteObject(obj: Raw) -> i32;
}

/// Une image par-dessus l'ecran (voir le module).
pub struct Overlay {
    h: Hwnd,
    shown: bool,
}

impl Overlay {
    /// Une image vide, cachee, possedee par `owner` (elle reste au-dessus
    /// de lui et disparait avec lui).
    pub fn new(owner: Hwnd) -> Option<Overlay> {
        const WS_POPUP: u32 = 0x8000_0000;
        const WS_EX_LAYERED: u32 = 0x0008_0000;
        const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
        const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
        const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
        const WS_EX_TOPMOST: u32 = 0x0000_0008;
        let class = wide("STATIC");
        // SAFETY: chaines terminees par zero ; la classe est celle de
        // Windows ; un proprietaire mort donne une fenetre sans proprietaire.
        let h = unsafe {
            OvCreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                class.as_ptr(),
                null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                owner.0 as Raw,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        (!h.is_null()).then_some(Overlay {
            h: Hwnd(h as isize),
            shown: false,
        })
    }

    /// Pose l'image `px` (`w` x `h`, une ligne apres l'autre depuis le haut,
    /// `a << 24 | r << 16 | g << 8 | b` PREMULTIPLIE, le format de cp_gfx)
    /// avec son coin haut-gauche en (`x`, `y`) sur l'ecran, et la montre.
    pub fn show(&mut self, x: i32, y: i32, w: i32, h: i32, px: &[u32]) -> bool {
        const DIB_RGB_COLORS: u32 = 0;
        const AC_SRC_ALPHA: u8 = 1;
        const ULW_ALPHA: u32 = 2;
        const SW_SHOWNOACTIVATE: i32 = 4;
        if w <= 0 || h <= 0 || px.len() < (w as usize) * (h as usize) {
            return false;
        }
        let bi = BitmapInfoHeader {
            size: size_of::<BitmapInfoHeader>() as u32,
            width: w,
            height: -h,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
        };
        // SAFETY: les DC et la DIB sont crees et rendus ici, dans l'ordre ;
        // la DIB fait w * h pixels de 32 bits (on y copie autant) ; les
        // structures sont completes.
        unsafe {
            let screen = OvGetDC(null_mut());
            if screen.is_null() {
                return false;
            }
            let mem = OvCreateCompatibleDC(screen);
            let mut bits: *mut c_void = null_mut();
            let bmp = OvCreateDIBSection(screen, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let ok = if !mem.is_null() && !bmp.is_null() && !bits.is_null() {
                let n = (w as usize) * (h as usize);
                std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, n);
                let old = OvSelectObject(mem, bmp);
                let dst = Point { x, y };
                let size = Size { cx: w, cy: h };
                let src = Point { x: 0, y: 0 };
                let blend = Blend {
                    op: 0,
                    flags: 0,
                    alpha: 255,
                    format: AC_SRC_ALPHA,
                };
                let r = UpdateLayeredWindow(
                    self.h.0 as Raw,
                    screen,
                    &dst,
                    &size,
                    mem,
                    &src,
                    0,
                    &blend,
                    ULW_ALPHA,
                );
                OvSelectObject(mem, old);
                r != 0
            } else {
                false
            };
            if !bmp.is_null() {
                OvDeleteObject(bmp);
            }
            if !mem.is_null() {
                OvDeleteDC(mem);
            }
            OvReleaseDC(null_mut(), screen);
            if ok && (!self.shown || OvIsWindowVisible(self.h.0 as Raw) == 0) {
                OvShowWindow(self.h.0 as Raw, SW_SHOWNOACTIVATE);
                self.shown = true;
            }
            ok
        }
    }

    /// Deplace l'image (son coin haut-gauche en `x`, `y` sur l'ecran).
    pub fn move_to(&self, x: i32, y: i32) {
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOZORDER: u32 = 0x0004;
        const SWP_NOACTIVATE: u32 = 0x0010;
        // SAFETY: sans effet sur un handle mort.
        unsafe {
            OvSetWindowPos(
                self.h.0 as Raw,
                null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    pub fn hwnd(&self) -> Hwnd {
        self.h
    }

    /// Cache l'image (elle se remontre a la prochaine pose).
    pub fn hide(&mut self) {
        hide_window(self.h);
        self.shown = false;
    }

    /// L'image est-elle a l'ecran (lu sur la fenetre : cachee d'ailleurs,
    /// par [`hide_window`], elle le sait).
    pub fn is_shown(&self) -> bool {
        // SAFETY: rend 0 sur un handle mort.
        unsafe { OvIsWindowVisible(self.h.0 as Raw) != 0 }
    }
}

/// Cache une image par sa fenetre (sans la tenir).
pub fn hide_window(h: Hwnd) {
    const SW_HIDE: i32 = 0;
    // SAFETY: sans effet sur un handle mort.
    unsafe { OvShowWindow(h.0 as Raw, SW_HIDE) };
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // SAFETY: notre fenetre ; sans effet si elle est deja morte.
        unsafe { OvDestroyWindow(self.h.0 as Raw) };
    }
}
