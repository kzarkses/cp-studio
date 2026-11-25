//! Ce dont une interface a besoin de Windows : afficher une image, la souris,
//! le clavier, les curseurs, les menus, le presse-papiers, l'heure exacte.
//!
//! Tout est sur : les fonctions prennent des `Hwnd` et des tranches, et ne
//! supposent rien d'un handle (Windows echoue proprement sur un handle mort).

use std::ffi::c_void;
use std::ptr::null_mut;

use crate::win::{Hwnd, Rect, wide};

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
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
struct TrackMouseEvent {
    size: u32,
    flags: u32,
    hwnd: Raw,
    hover_time: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetDC(h: Raw) -> Raw;
    fn ReleaseDC(h: Raw, dc: Raw) -> i32;
    fn GetClientRect(h: Raw, r: *mut Rect) -> i32;
    fn GetWindowRect(h: Raw, r: *mut Rect) -> i32;
    fn SetWindowPos(h: Raw, after: Raw, x: i32, y: i32, w: i32, h2: i32, flags: u32) -> i32;
    fn ScreenToClient(h: Raw, p: *mut Point) -> i32;
    fn ClientToScreen(h: Raw, p: *mut Point) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn WindowFromPoint(p: Point) -> Raw;
    fn SetCapture(h: Raw) -> Raw;
    fn ReleaseCapture() -> i32;
    fn GetCapture() -> Raw;
    fn SetCursor(c: Raw) -> Raw;
    fn LoadCursorW(inst: Raw, name: *const u16) -> Raw;
    fn LoadCursorFromFileW(path: *const u16) -> Raw;
    fn TrackMouseEvent(t: *mut TrackMouseEvent) -> i32;
    fn GetKeyState(vk: i32) -> i16;
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn GetKeyboardState(state: *mut u8) -> i32;
    fn ToUnicode(vk: u32, scan: u32, state: *const u8, buf: *mut u16, n: i32, flags: u32) -> i32;
    fn SetFocus(h: Raw) -> Raw;
    fn GetFocus() -> Raw;
    fn GetParent(h: Raw) -> Raw;
    fn GetAncestor(h: Raw, flags: u32) -> Raw;
    fn IsChild(parent: Raw, h: Raw) -> i32;
    fn IsWindowVisible(h: Raw) -> i32;
    fn IsIconic(h: Raw) -> i32;
    fn OpenClipboard(h: Raw) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(fmt: u32, mem: Raw) -> Raw;
    fn GetClipboardData(fmt: u32) -> Raw;
    fn CloseClipboard() -> i32;
    fn GetDpiForWindow(h: Raw) -> u32;
    fn SetWindowTextW(h: Raw, s: *const u16) -> i32;
    fn GetClassNameW(h: Raw, buf: *mut u16, n: i32) -> i32;
    fn GetDoubleClickTime() -> u32;
    fn GetWindow(h: Raw, cmd: u32) -> Raw;
    fn GetWindowTextW(h: Raw, buf: *mut u16, n: i32) -> i32;
    fn MapWindowPoints(from: Raw, to: Raw, pts: *mut Point, n: u32) -> i32;
    fn ShowWindow(h: Raw, cmd: i32) -> i32;
    fn UpdateWindow(h: Raw) -> i32;
    fn MonitorFromPoint(p: Point, flags: u32) -> Raw;
    fn GetMonitorInfoW(m: Raw, info: *mut MonitorInfo) -> i32;
    fn GetForegroundWindow() -> Raw;
    fn GetWindowThreadProcessId(h: Raw, pid: *mut u32) -> u32;
}

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn SetDIBitsToDevice(
        dc: Raw,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        xs: i32,
        ys: i32,
        start: u32,
        lines: u32,
        bits: *const c_void,
        bmi: *const BitmapInfoHeader,
        usage: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(c: *mut i64) -> i32;
    fn QueryPerformanceFrequency(f: *mut i64) -> i32;
    fn GlobalAlloc(flags: u32, n: usize) -> Raw;
    fn GlobalLock(m: Raw) -> Raw;
    fn GlobalUnlock(m: Raw) -> i32;
    fn GlobalFree(m: Raw) -> Raw;
    fn GetCurrentThreadId() -> u32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmFlush() -> i32;
}

// ---------------------------------------------------------------------------
// L'image
// ---------------------------------------------------------------------------

/// Copie des rectangles de `px` (`w x h`, BGRA, haut en premier) dans la zone
/// client. Chaque rectangle est recopie dans `scratch` (contigu) puis envoye :
/// aucune ambiguite d'origine des DIB, et seul ce qui a change part.
pub fn present(
    hwnd: Hwnd,
    px: &[u32],
    w: i32,
    h: i32,
    rects: &[(i32, i32, i32, i32)],
    scratch: &mut Vec<u32>,
) {
    if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
        return;
    }
    // SAFETY: GetDC accepte un handle mort (rend nul).
    let dc = unsafe { GetDC(hwnd.raw()) };
    if dc.is_null() {
        return;
    }
    for &(rx, ry, rw, rh) in rects {
        let x0 = rx.max(0);
        let y0 = ry.max(0);
        let x1 = (rx + rw).min(w);
        let y1 = (ry + rh).min(h);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        // Des lignes entieres sont deja contigues : pas de copie.
        let data: &[u32] = if x0 == 0 && x1 == w {
            &px[(y0 * w) as usize..(y1 * w) as usize]
        } else {
            scratch.clear();
            for y in y0..y1 {
                let row = (y * w) as usize;
                scratch.extend_from_slice(&px[row + x0 as usize..row + x1 as usize]);
            }
            scratch
        };
        let bmi = BitmapInfoHeader {
            size: size_of::<BitmapInfoHeader>() as u32,
            width: cw,
            height: -ch, // haut en premier
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
        };
        // SAFETY: `data` tient cw * ch pixels de 32 bits, decrits par `bmi`.
        unsafe {
            SetDIBitsToDevice(
                dc,
                x0,
                y0,
                cw as u32,
                ch as u32,
                0,
                0,
                0,
                ch as u32,
                data.as_ptr() as *const c_void,
                &bmi,
                0,
            );
        }
    }
    // SAFETY: le DC obtenu ci-dessus.
    unsafe { ReleaseDC(hwnd.raw(), dc) };
}

// ---------------------------------------------------------------------------
// Le temps
// ---------------------------------------------------------------------------

pub fn qpc() -> i64 {
    let mut c = 0i64;
    // SAFETY: ecrit un i64.
    unsafe { QueryPerformanceCounter(&mut c) };
    c
}

pub fn qpc_freq() -> i64 {
    let mut f = 0i64;
    // SAFETY: ecrit un i64.
    unsafe { QueryPerformanceFrequency(&mut f) };
    f.max(1)
}

/// Attend la prochaine composition de l'ecran (DWM). Faux si DWM refuse.
pub fn dwm_flush() -> bool {
    // SAFETY: aucune precondition.
    unsafe { DwmFlush() >= 0 }
}

pub fn double_click_ms() -> u32 {
    // SAFETY: aucune precondition.
    unsafe { GetDoubleClickTime() }
}

// ---------------------------------------------------------------------------
// Les fenetres
// ---------------------------------------------------------------------------

pub fn client_rect(h: Hwnd) -> Rect {
    let mut r = Rect::default();
    // SAFETY: ecrit un RECT.
    unsafe { GetClientRect(h.raw(), &mut r) };
    r
}

pub fn window_rect(h: Hwnd) -> Rect {
    let mut r = Rect::default();
    // SAFETY: ecrit un RECT.
    unsafe { GetWindowRect(h.raw(), &mut r) };
    r
}

pub fn move_window(h: Hwnd, x: i32, y: i32, w: i32, hh: i32) {
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    // SAFETY: sans effet sur un handle mort.
    unsafe {
        SetWindowPos(
            h.raw(),
            null_mut(),
            x,
            y,
            w,
            hh,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

pub fn set_title(h: Hwnd, s: &str) {
    let w = wide(s);
    // SAFETY: chaine terminee par zero.
    unsafe { SetWindowTextW(h.raw(), w.as_ptr()) };
}

pub fn screen_to_client(h: Hwnd, x: i32, y: i32) -> (i32, i32) {
    let mut p = Point { x, y };
    // SAFETY: ecrit un POINT.
    unsafe { ScreenToClient(h.raw(), &mut p) };
    (p.x, p.y)
}

pub fn client_to_screen(h: Hwnd, x: i32, y: i32) -> (i32, i32) {
    let mut p = Point { x, y };
    // SAFETY: ecrit un POINT.
    unsafe { ClientToScreen(h.raw(), &mut p) };
    (p.x, p.y)
}

/// La fenetre (la plus profonde) sous ce point de l'ecran.
pub fn window_at(x: i32, y: i32) -> Hwnd {
    // SAFETY: un POINT par valeur ; aucune autre precondition.
    Hwnd(unsafe { WindowFromPoint(Point { x, y }) } as isize)
}

/// `h` est `win` ou l'une de ses filles.
pub fn is_within(win: Hwnd, h: Hwnd) -> bool {
    h.0 != 0 && (h == win || is_child(win, h))
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = Point::default();
    // SAFETY: ecrit un POINT.
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

/// Les filles DIRECTES de `parent`, dans l'ordre de la pile (du dessus au
/// dessous).
pub fn children(parent: Hwnd) -> Vec<Hwnd> {
    const GW_HWNDNEXT: u32 = 2;
    const GW_CHILD: u32 = 5;
    let mut out = Vec::new();
    // SAFETY: GetWindow accepte tout handle et rend nul a la fin.
    let mut h = unsafe { GetWindow(parent.raw(), GW_CHILD) };
    while !h.is_null() && out.len() < 4096 {
        out.push(Hwnd(h as isize));
        // SAFETY: idem.
        h = unsafe { GetWindow(h, GW_HWNDNEXT) };
    }
    out
}

/// Le titre d'une fenetre.
pub fn window_text(h: Hwnd) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: tampon de 256 caracteres.
    let n = unsafe { GetWindowTextW(h.raw(), buf.as_mut_ptr(), 256) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Le rectangle de `h` dans les coordonnees client de `parent`.
pub fn rect_in(h: Hwnd, parent: Hwnd) -> Rect {
    let r = window_rect(h);
    let mut p = [
        Point {
            x: r.left,
            y: r.top,
        },
        Point {
            x: r.right,
            y: r.bottom,
        },
    ];
    // SAFETY: deux points en lecture-ecriture.
    unsafe { MapWindowPoints(null_mut(), parent.raw(), p.as_mut_ptr(), 2) };
    Rect {
        left: p[0].x,
        top: p[0].y,
        right: p[1].x,
        bottom: p[1].y,
    }
}

/// Le drapeau de visibilite de la fenetre elle-meme (pas de ses ancetres).
pub fn is_visible(h: Hwnd) -> bool {
    // SAFETY: aucune precondition.
    unsafe { IsWindowVisible(h.raw()) != 0 }
}

/// Cache ou montre sans activer.
pub fn show_window(h: Hwnd, show: bool) {
    const SW_HIDE: i32 = 0;
    const SW_SHOWNA: i32 = 8;
    // SAFETY: sans effet sur un handle mort.
    unsafe { ShowWindow(h.raw(), if show { SW_SHOWNA } else { SW_HIDE }) };
}

/// Pose une fenetre fille en haut de la pile de ses soeurs, a `x, y, w, h`
/// (coordonnees de son parent), en la montrant (`Some(true)`), la cachant
/// (`Some(false)`) ou sans toucher a sa visibilite (`None`). Jamais activee.
pub fn place_window(h: Hwnd, x: i32, y: i32, w: i32, hh: i32, show: Option<bool>) {
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const SWP_HIDEWINDOW: u32 = 0x0080;
    let flags = SWP_NOACTIVATE
        | match show {
            Some(true) => SWP_SHOWWINDOW,
            Some(false) => SWP_HIDEWINDOW,
            None => 0,
        };
    // SAFETY: HWND_TOP = nul ; sans effet sur un handle mort.
    unsafe { SetWindowPos(h.raw(), null_mut(), x, y, w, hh, flags) };
}

/// Peint tout de suite ce qui est a peindre (WM_PAINT synchrone).
pub fn update_window(h: Hwnd) {
    // SAFETY: sans effet sur un handle mort.
    unsafe { UpdateWindow(h.raw()) };
}

/// Visible pour de vrai : ni cachee (onglet de docker inactif, ancetre cache),
/// ni dans une fenetre principale reduite.
pub fn is_shown(h: Hwnd) -> bool {
    const GA_ROOT: u32 = 2;
    // SAFETY: aucune precondition.
    unsafe {
        if IsWindowVisible(h.raw()) == 0 {
            return false;
        }
        let root = GetAncestor(h.raw(), GA_ROOT);
        root.is_null() || IsIconic(root) == 0
    }
}

pub fn parent(h: Hwnd) -> Hwnd {
    // SAFETY: aucune precondition.
    Hwnd(unsafe { GetParent(h.raw()) } as isize)
}

pub fn is_child(parent: Hwnd, h: Hwnd) -> bool {
    // SAFETY: aucune precondition.
    unsafe { IsChild(parent.raw(), h.raw()) != 0 }
}

pub fn focus() -> Hwnd {
    // SAFETY: aucune precondition.
    Hwnd(unsafe { GetFocus() } as isize)
}

pub fn set_focus(h: Hwnd) {
    // SAFETY: sans effet sur un handle mort.
    unsafe { SetFocus(h.raw()) };
}

pub fn class_name(h: Hwnd) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: tampon de 128 caracteres.
    let n = unsafe { GetClassNameW(h.raw(), buf.as_mut_ptr(), 128) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Points par pouce de l'ecran de la fenetre (96 = 100 %).
pub fn dpi(h: Hwnd) -> u32 {
    // SAFETY: rend 0 sur un handle invalide.
    let d = unsafe { GetDpiForWindow(h.raw()) };
    if d == 0 { 96 } else { d }
}

// ---------------------------------------------------------------------------
// La souris
// ---------------------------------------------------------------------------

/// La zone de travail (sans la barre des taches) de l'ecran sous le point
/// d'ecran (`x`, `y`) : (gauche, haut, largeur, hauteur).
pub fn work_area(x: i32, y: i32) -> (i32, i32, i32, i32) {
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    let mut mi = MonitorInfo {
        size: size_of::<MonitorInfo>() as u32,
        monitor: Rect::default(),
        work: Rect::default(),
        flags: 0,
    };
    // SAFETY: structure complete, sa taille posee ; un moniteur existe
    // toujours (le plus proche).
    let ok = unsafe {
        let m = MonitorFromPoint(Point { x, y }, MONITOR_DEFAULTTONEAREST);
        !m.is_null() && GetMonitorInfoW(m, &mut mi) != 0
    };
    if !ok {
        return (0, 0, 1920, 1080);
    }
    let r = mi.work;
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

/// La fenetre au premier plan est-elle de ce fil (REAPER et ses fenetres,
/// pas une autre application) ?
pub fn foreground_is_ours() -> bool {
    // SAFETY: aucune precondition ; un handle nul rend 0.
    unsafe {
        let f = GetForegroundWindow();
        !f.is_null() && GetWindowThreadProcessId(f, null_mut()) == GetCurrentThreadId()
    }
}

pub fn capture(h: Hwnd) {
    // SAFETY: aucune precondition.
    unsafe { SetCapture(h.raw()) };
}

pub fn release_capture() {
    // SAFETY: aucune precondition.
    unsafe { ReleaseCapture() };
}

/// La fenetre qui tient la souris (de ce fil), 0 si aucune.
pub fn capture_window() -> Hwnd {
    // SAFETY: aucune precondition.
    Hwnd(unsafe { GetCapture() } as isize)
}

pub fn has_capture(h: Hwnd) -> bool {
    // SAFETY: aucune precondition.
    unsafe { GetCapture() as isize == h.0 }
}

/// Demande un WM_MOUSELEAVE quand la souris quitte la fenetre.
pub fn track_leave(h: Hwnd) {
    const TME_LEAVE: u32 = 2;
    let mut t = TrackMouseEvent {
        size: size_of::<TrackMouseEvent>() as u32,
        flags: TME_LEAVE,
        hwnd: h.raw(),
        hover_time: 0,
    };
    // SAFETY: structure complete.
    unsafe { TrackMouseEvent(&mut t) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Hand,
    IBeam,
    SizeWE,
    SizeNS,
    SizeAll,
    No,
    Wait,
    Cross,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Raw;
}

/// Le nom REAPER d'un curseur, et ou le prendre : une ressource de
/// reaper.exe (son numero) ou un curseur de Windows (IDC_*). Releve dans
/// reaper.exe le 2026-11-22 (les appels de son chargeur).
fn pointer_src(p: cp_ui::Pointer) -> (&'static str, Src) {
    use Src::{Idc, Res};
    use cp_ui::Pointer as P;
    const ARROW: u16 = 32512;
    const SIZEWE: u16 = 32644;
    const SIZENS: u16 = 32645;
    match p {
        P::Arrow => ("arrow", Idc(ARROW)),
        P::ItemMove => ("arrange_move", Res(187)),
        P::ItemCopy => ("arrange_dd_copy", Res(182)),
        P::ItemSlide => ("arrange_slide", Res(465)),
        P::TrimLeft => ("arrange_leftresize", Res(417)),
        P::TrimRight => ("arrange_rightresize", Res(418)),
        P::StretchLeft => ("arrange_leftstretch", Res(430)),
        P::StretchRight => ("arrange_rightstretch", Res(431)),
        P::DualEdge => ("arrange_dualedge", Res(450)),
        P::FadeIn => ("arrange_fadein", Res(105)),
        P::FadeOut => ("arrange_fadeout", Res(184)),
        P::ItemVol => ("arrange_itemvol", Idc(SIZENS)),
        P::IBeam => ("arrange_ibeam", Res(220)),
        P::TimeSelEdge => ("arrange_timesel", Idc(SIZEWE)),
        P::Marquee => ("arrange_marquee", Res(488)),
        P::HandScroll => ("arrange_handscroll", Res(429)),
        P::Delete => ("delete", Res(464)),
        P::NoteDraw => ("midi_draw", Res(185)),
        P::NotePaint => ("midi_paint", Res(473)),
        P::NoteErase => ("midi_erase", Res(472)),
        P::NoteCopy => ("midi_copy", Res(182)),
        P::NoteEdgeLeft => ("midi_noteedge_l", Res(417)),
        P::NoteEdgeRight => ("midi_noteedge_r", Res(418)),
        P::NoteEdgeDual => ("midi_noteedge_dual", Res(450)),
        P::NoteStretchLeft => ("midi_stretch_l", Res(430)),
        P::NoteStretchRight => ("midi_stretch_r", Res(431)),
        P::NoteMoveH => ("midi_move_horz", Idc(SIZEWE)),
        P::NoteMoveV => ("midi_move_vert", Idc(SIZENS)),
        P::NoteVel => ("midi_vel", Idc(SIZENS)),
        P::NoteChord => ("midi_chord", Res(503)),
        P::NoteArp => ("midi_arp", Res(502)),
        P::TouchSelect => ("midi_touch_select", Res(515)),
        P::TrackDrag => ("vertical_drag", Res(613)),
        P::ResizeNS => ("tcp_resize", Idc(SIZENS)),
        P::ResizeWE => ("tcppane_resize", Idc(SIZEWE)),
    }
}

#[derive(Clone, Copy)]
enum Src {
    Res(u16),
    Idc(u16),
}

thread_local! {
    /// Les curseurs charges (partages, jamais detruits) et le dossier
    /// `Cursors` de REAPER.
    static POINTERS: std::cell::RefCell<(Option<std::path::PathBuf>, std::collections::HashMap<cp_ui::Pointer, usize>)> =
        std::cell::RefCell::new((None, std::collections::HashMap::new()));
}

/// Le dossier de ressources de REAPER : ses curseurs personnalises sont dans
/// `Cursors`.
pub fn set_cursor_dir(resource: &std::path::Path) {
    POINTERS.with(|p| {
        let mut p = p.borrow_mut();
        let dir = resource.join("Cursors");
        if p.0.as_deref() != Some(dir.as_path()) {
            p.0 = Some(dir);
            p.1.clear();
        }
    });
}

/// Montre le curseur de REAPER pour ce geste, comme REAPER le charge : le
/// fichier `Cursors\<nom>.cur` s'il existe (les curseurs de Cedric), sinon
/// celui de reaper.exe, sinon celui de Windows.
pub fn set_pointer(ptr: cp_ui::Pointer) {
    let h = POINTERS.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(&h) = p.1.get(&ptr) {
            return h as Raw;
        }
        let (name, src) = pointer_src(ptr);
        let mut h: Raw = null_mut();
        if let Some(dir) = &p.0 {
            let f = dir.join(format!("{name}.cur"));
            if f.is_file() {
                let w = crate::win::wide(&f.to_string_lossy());
                // SAFETY: chemin UTF-16 termine par zero.
                h = unsafe { LoadCursorFromFileW(w.as_ptr()) };
            }
        }
        if h.is_null() {
            h = match src {
                // SAFETY: le module de reaper.exe (le processus) ; un numero de
                // ressource passe en MAKEINTRESOURCE.
                Src::Res(id) => unsafe {
                    LoadCursorW(GetModuleHandleW(null_mut()), id as usize as *const u16)
                },
                Src::Idc(_) => null_mut(),
            };
        }
        if h.is_null() {
            let idc = match src {
                Src::Idc(i) => i,
                Src::Res(_) => 32512,
            };
            // SAFETY: curseur systeme partage.
            h = unsafe { LoadCursorW(null_mut(), idc as usize as *const u16) };
        }
        p.1.insert(ptr, h as usize);
        h
    });
    // SAFETY: un curseur charge ci-dessus (partage, jamais detruit).
    unsafe { SetCursor(h) };
}

pub fn set_cursor(c: Cursor) {
    let id: usize = match c {
        Cursor::Arrow => 32512,
        Cursor::IBeam => 32513,
        Cursor::Wait => 32514,
        Cursor::Cross => 32515,
        Cursor::SizeWE => 32644,
        Cursor::SizeNS => 32645,
        Cursor::SizeAll => 32646,
        Cursor::No => 32648,
        Cursor::Hand => 32649,
    };
    // SAFETY: curseur systeme partage, rien a liberer.
    unsafe { SetCursor(LoadCursorW(null_mut(), id as *const u16)) };
}

// ---------------------------------------------------------------------------
// Le clavier
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
}

pub fn mods() -> Mods {
    // SAFETY: aucune precondition.
    let down = |vk: i32| unsafe { GetKeyState(vk) } < 0;
    Mods {
        shift: down(0x10),
        ctrl: down(0x11),
        alt: down(0x12),
        win: down(0x5B) || down(0x5C),
    }
}

/// La touche `vk` est tenue, ou qu'aille le clavier (une ecoute « tant que
/// la touche est tenue » se termine meme si son relachement va ailleurs).
pub fn key_held(vk: u32) -> bool {
    // SAFETY: aucune precondition.
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

/// Un bouton de la souris est tenu, ou qu'elle soit (un geste en cours dans
/// REAPER : on n'ecrit rien sous ses doigts).
pub fn mouse_held() -> bool {
    // SAFETY: aucune precondition.
    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) } < 0;
    down(0x01) || down(0x02) || down(0x04)
}

/// Le texte que produirait la touche `vk` (clavier courant, AZERTY compris),
/// sans toucher a l'etat des touches mortes.
pub fn key_text(vk: u32, scan: u32) -> Option<String> {
    let mut state = [0u8; 256];
    let mut buf = [0u16; 8];
    // SAFETY: tampons de la taille annoncee ; drapeau 4 = ne pas modifier
    // l'etat du clavier (Windows 10 1607+).
    let n = unsafe {
        GetKeyboardState(state.as_mut_ptr());
        ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 8, 4)
    };
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
}

// ---------------------------------------------------------------------------
// Les menus
// ---------------------------------------------------------------------------

/// Une ligne de menu : `None` = separateur. Les menus sont des popovers
/// maison ([`cp_ui::pop`]) : jamais un menu de Windows (regle de Cedric,
/// 2026-11-25 ; il n'y a plus de quoi en ouvrir un).
pub struct MenuItem<'a> {
    pub id: u32,
    pub text: &'a str,
    pub checked: bool,
    pub enabled: bool,
}

/// Un menu plat en lignes de menu en arbre (pour un popover maison).
pub fn flat_to_tree(items: &[Option<MenuItem<'_>>]) -> Vec<MenuEntry> {
    items
        .iter()
        .map(|i| match i {
            None => MenuEntry::Sep,
            Some(i) => MenuEntry::Item {
                id: i.id,
                text: i.text.to_string(),
                checked: i.checked,
                enabled: i.enabled,
            },
        })
        .collect()
}

/// Une ligne d'un menu en arbre.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item {
        id: u32,
        text: String,
        checked: bool,
        enabled: bool,
    },
    Sep,
    /// Un sous-menu.
    Sub(String, Vec<MenuEntry>),
}

impl MenuEntry {
    /// Le meme menu pour un popover maison ([`cp_ui::pop`]) : jamais un menu
    /// de Windows (regle de Cedric, 2026-11-25).
    pub fn to_pop(&self) -> cp_ui::pop::Entry {
        use cp_ui::pop::Entry;
        match self {
            MenuEntry::Item {
                id,
                text,
                checked,
                enabled,
            } => Entry::check(*id, text.clone(), *checked).enabled(*enabled),
            MenuEntry::Sep => Entry::Sep,
            MenuEntry::Sub(t, v) => Entry::sub(t.clone(), v.iter().map(|e| e.to_pop()).collect()),
        }
    }

    /// Le texte du choix `id` (dans les sous-menus aussi).
    pub fn text_of(entries: &[MenuEntry], id: u32) -> Option<String> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item { id: i, text, .. } if *i == id => Some(text.clone()),
            MenuEntry::Sub(_, v) => MenuEntry::text_of(v, id),
            _ => None,
        })
    }

    pub fn item(id: u32, text: impl Into<String>) -> MenuEntry {
        MenuEntry::Item {
            id,
            text: text.into(),
            checked: false,
            enabled: true,
        }
    }
    pub fn check(id: u32, text: impl Into<String>, checked: bool) -> MenuEntry {
        MenuEntry::Item {
            id,
            text: text.into(),
            checked,
            enabled: true,
        }
    }
    pub fn off(text: impl Into<String>) -> MenuEntry {
        MenuEntry::Item {
            id: 0,
            text: text.into(),
            checked: false,
            enabled: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Le presse-papiers
// ---------------------------------------------------------------------------

const CF_UNICODETEXT: u32 = 13;

pub fn set_clipboard_text(owner: Hwnd, s: &str) -> bool {
    const GMEM_MOVEABLE: u32 = 2;
    let w = wide(s);
    let bytes = w.len() * 2;
    // SAFETY: memoire globale allouee, remplie, puis confiee au presse-papiers
    // (qui la possede alors) ; liberee nous-memes en cas d'echec.
    unsafe {
        if OpenClipboard(owner.raw()) == 0 {
            return false;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(mem) as *mut u16;
        if p.is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
        GlobalUnlock(mem);
        let ok = !SetClipboardData(CF_UNICODETEXT, mem).is_null();
        if !ok {
            GlobalFree(mem);
        }
        CloseClipboard();
        ok
    }
}

pub fn clipboard_text(owner: Hwnd) -> Option<String> {
    // SAFETY: le presse-papiers est ouvert le temps de copier la chaine.
    unsafe {
        if OpenClipboard(owner.raw()) == 0 {
            return None;
        }
        let mem = GetClipboardData(CF_UNICODETEXT);
        let mut out = None;
        if !mem.is_null() {
            let p = GlobalLock(mem) as *const u16;
            if !p.is_null() {
                let mut n = 0usize;
                while *p.add(n) != 0 && n < 1 << 20 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(mem);
            }
        }
        CloseClipboard();
        out
    }
}

// ---------------------------------------------------------------------------
// Le retour de balayage de l'ecran
// ---------------------------------------------------------------------------

#[repr(C)]
struct OpenAdapterFromHdc {
    hdc: Raw,
    adapter: u32,
    luid_low: u32,
    luid_high: i32,
    source: u32,
}

#[repr(C)]
struct WaitVBlank {
    adapter: u32,
    device: u32,
    source: u32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn D3DKMTOpenAdapterFromHdc(p: *mut OpenAdapterFromHdc) -> i32;
    fn D3DKMTWaitForVerticalBlankEvent(p: *const WaitVBlank) -> i32;
    fn D3DKMTCloseAdapter(p: *const u32) -> i32;
}

/// Attendre le VRAI retour de balayage de l'ecran principal (par le pilote
/// graphique). Contrairement a `DwmFlush`, il arrive a chaque image meme quand
/// rien ne change a l'ecran : une horloge qui en depend ne s'arrete jamais.
pub struct VBlank {
    adapter: u32,
    source: u32,
}

impl VBlank {
    pub fn open() -> Option<VBlank> {
        // SAFETY: le DC de l'ecran, rendu apres l'ouverture ; structure
        // complete, ecrite par le systeme.
        unsafe {
            let dc = GetDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let mut o = OpenAdapterFromHdc {
                hdc: dc,
                adapter: 0,
                luid_low: 0,
                luid_high: 0,
                source: 0,
            };
            let st = D3DKMTOpenAdapterFromHdc(&mut o);
            ReleaseDC(null_mut(), dc);
            (st == 0).then_some(VBlank {
                adapter: o.adapter,
                source: o.source,
            })
        }
    }

    /// Bloque jusqu'au prochain retour de balayage. Faux en cas d'echec
    /// (changement de mode d'ecran : rouvrir).
    pub fn wait(&self) -> bool {
        let w = WaitVBlank {
            adapter: self.adapter,
            device: 0,
            source: self.source,
        };
        // SAFETY: l'adaptateur ouvert par `open`.
        unsafe { D3DKMTWaitForVerticalBlankEvent(&w) == 0 }
    }
}

impl Drop for VBlank {
    fn drop(&mut self) {
        // SAFETY: l'adaptateur ouvert par `open`, ferme une fois.
        unsafe { D3DKMTCloseAdapter(&self.adapter) };
    }
}

// ---------------------------------------------------------------------------
// Les fenetres du processus, et leur mise en page
// ---------------------------------------------------------------------------

#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "GetDesktopWindow"]
    fn GetDesktopWindow3() -> Raw;
    #[link_name = "GetWindowThreadProcessId"]
    fn GetWindowThreadProcessId3(h: Raw, pid: *mut u32) -> u32;
    #[link_name = "IsZoomed"]
    fn IsZoomed3(h: Raw) -> i32;
    #[link_name = "SendMessageW"]
    fn SendMessageW2(h: Raw, msg: u32, wp: usize, lp: isize) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetCurrentProcessId"]
    fn GetCurrentProcessId3() -> u32;
}

/// Toutes les descendantes de `h`, en largeur (bornees a 20 000).
pub fn descendants(h: Hwnd) -> Vec<Hwnd> {
    let mut out = children(h);
    let mut k = 0;
    while k < out.len() && out.len() < 20_000 {
        let c = children(out[k]);
        out.extend(c);
        k += 1;
    }
    out
}

/// Les fenetres de premier niveau de ce processus (les dockers flottants
/// compris).
pub fn process_windows() -> Vec<Hwnd> {
    // SAFETY: sans argument.
    let me = unsafe { GetCurrentProcessId3() };
    // SAFETY: sans argument.
    let desk = Hwnd(unsafe { GetDesktopWindow3() } as isize);
    children(desk)
        .into_iter()
        .filter(|h| {
            let mut pid = 0u32;
            // SAFETY: ecrit un u32 ; un handle mort rend 0.
            unsafe { GetWindowThreadProcessId3(h.raw(), &mut pid) };
            pid == me
        })
        .collect()
}

/// Rejoue a `h` sa taille actuelle (WM_SIZE) : il refait sa mise en page
/// comme si on venait de le redimensionner.
pub fn resend_size(h: Hwnd) {
    const WM_SIZE: u32 = 0x0005;
    let r = client_rect(h);
    let (w, hh) = ((r.right - r.left).max(0), (r.bottom - r.top).max(0));
    // SAFETY: rend 0 sur un handle mort.
    let kind = if unsafe { IsZoomed3(h.raw()) } != 0 {
        2
    } else {
        0
    };
    let lp = (((hh as isize) & 0xFFFF) << 16) | ((w as isize) & 0xFFFF);
    // SAFETY: message standard, sans pointeur ; fil principal.
    unsafe { SendMessageW2(h.raw(), WM_SIZE, kind, lp) };
}
