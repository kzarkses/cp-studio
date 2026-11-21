//! Le VRAI chargeur (`reaper_cp_studio.dll`), charge par un faux REAPER.
//!
//! Ce test joue le role de REAPER avec les structures exactes du SDK
//! (`reaper_plugin_info_t`, `custom_action_register_t`, `hookcommand2`,
//! `timer`) : il charge la DLL, verifie ce qu'elle enregistre, fait battre son
//! horloge, declenche ses actions, provoque un rechargement du module par une
//! nouvelle construction, puis la decharge comme REAPER a la fermeture.
//!
//! Il joue aussi le FIL AUDIO de REAPER : a chaque bloc, le crochet (passage
//! « pre »), puis chaque apercu lance tire sa source par la table virtuelle de
//! `PCM_source`, puis le crochet (passage « post »).
//!
//! Il vit dans cp_sys parce que jouer REAPER est du FFI par nature : c'est le
//! seul crate autorise a ecrire `unsafe`.

use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use cp_sys::audio::{AudioHookRegister, PreviewRegister};
use cp_sys::pcm::{Transfer, Vtbl};

#[repr(C)]
struct RawInfo {
    caller_version: i32,
    hwnd_main: *mut c_void,
    register: unsafe extern "C" fn(*const c_char, *mut c_void) -> i32,
    get_func: unsafe extern "C" fn(*const c_char) -> *mut c_void,
}

#[repr(C)]
struct CustomAction {
    section: i32,
    id: *const c_char,
    name: *const c_char,
    extra: *mut c_void,
}

struct Fake {
    timer: Option<usize>,
    hook: Option<usize>,
    actions: Vec<(String, String, i32)>,
    console: String,
    next_cmd: i32,
    audio_hook: Option<usize>,
    previews: Vec<usize>,
    audio_time: f64,
}

static FAKE: Mutex<Fake> = Mutex::new(Fake {
    timer: None,
    hook: None,
    actions: Vec::new(),
    console: String::new(),
    next_cmd: 40000,
    audio_hook: None,
    previews: Vec::new(),
    audio_time: 0.0,
});

const SRATE: f64 = 44100.0;
static RESOURCE: Mutex<Option<CString>> = Mutex::new(None);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn FreeLibrary(h: *mut c_void) -> i32;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

/// # Safety
/// `p` : chaine terminee par zero.
unsafe fn cstr(p: *const c_char) -> String {
    // SAFETY: garanti par l'appelant.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

unsafe extern "C" fn fake_register(name: *const c_char, info: *mut c_void) -> i32 {
    // SAFETY: le chargeur passe un nom termine par zero.
    let n = unsafe { cstr(name) };
    let mut f = FAKE.lock().unwrap();
    match n.as_str() {
        "timer" => f.timer = Some(info as usize),
        "-timer" => f.timer = None,
        "hookcommand2" => f.hook = Some(info as usize),
        "-hookcommand2" => f.hook = None,
        "custom_action" => {
            // SAFETY: pour "custom_action", REAPER recoit un
            // custom_action_register_t (c'est ce que le test verifie).
            let ca = unsafe { &*(info as *const CustomAction) };
            assert_eq!(ca.section, 0);
            // SAFETY: chaines tenues par le chargeur pour la vie du processus.
            let (id, nm) = unsafe { (cstr(ca.id), cstr(ca.name)) };
            f.next_cmd += 1;
            let c = f.next_cmd;
            f.actions.push((id, nm, c));
            return c;
        }
        _ => return 0,
    }
    1
}

extern "C" fn fake_resource_path() -> *const c_char {
    RESOURCE
        .lock()
        .unwrap()
        .as_ref()
        .map_or(null(), |c| c.as_ptr())
}

extern "C" fn fake_console(msg: *const c_char) {
    // SAFETY: ShowConsoleMsg recoit une chaine terminee par zero.
    let m = unsafe { cstr(msg) };
    FAKE.lock().unwrap().console.push_str(&m);
}

extern "C" fn fake_reg_hook(add: bool, reg: *mut AudioHookRegister) -> i32 {
    let mut f = FAKE.lock().unwrap();
    if add {
        f.audio_hook = Some(reg as usize);
    } else if f.audio_hook == Some(reg as usize) {
        f.audio_hook = None;
    }
    1
}

extern "C" fn fake_play_preview(p: *mut PreviewRegister, flags: i32, align: f64) -> i32 {
    // Sans tampon, tout de suite : c'est ce que le moteur demande.
    assert_eq!((flags, align), (0, -1.0));
    let mut f = FAKE.lock().unwrap();
    assert!(
        !f.previews.contains(&(p as usize)),
        "apercu lance deux fois"
    );
    f.previews.push(p as usize);
    1
}

extern "C" fn fake_stop_preview(p: *mut PreviewRegister) -> i32 {
    FAKE.lock().unwrap().previews.retain(|&x| x != p as usize);
    1
}

extern "C" fn fake_device_info(attr: *const c_char, out: *mut c_char, n: i32) -> bool {
    // SAFETY: attribut termine par zero.
    if unsafe { cstr(attr) } != "SRATE" || n < 8 {
        return false;
    }
    let v = b"44100\0";
    // SAFETY: `n` octets inscriptibles, n >= 8.
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out as *mut u8, v.len()) };
    true
}

unsafe extern "C" fn fake_get_func(name: *const c_char) -> *mut c_void {
    // SAFETY: nom termine par zero.
    match unsafe { cstr(name) }.as_str() {
        "GetResourcePath" => fake_resource_path as *const () as *mut c_void,
        "ShowConsoleMsg" => fake_console as *const () as *mut c_void,
        "Audio_RegHardwareHook" => fake_reg_hook as *const () as *mut c_void,
        "PlayPreviewEx" => fake_play_preview as *const () as *mut c_void,
        "StopPreview" => fake_stop_preview as *const () as *mut c_void,
        "GetAudioDeviceInfo" => fake_device_info as *const () as *mut c_void,
        _ => null_mut(),
    }
}

/// Un bloc du fil audio de REAPER. Rend ce que les apercus ont produit
/// (stereo entrelace, sommes).
fn audio_block(frames: usize) -> Vec<f64> {
    let (hook, previews, t) = {
        let f = FAKE.lock().unwrap();
        (
            f.audio_hook.expect("crochet audio pose"),
            f.previews.clone(),
            f.audio_time,
        )
    };
    let reg = hook as *mut AudioHookRegister;
    // SAFETY: la structure enregistree par le chargeur, vivante.
    let cb = unsafe { (*reg).on_audio_buffer }.expect("OnAudioBuffer");
    // SAFETY: signature du SDK ; appele comme REAPER, passage « pre ».
    unsafe { cb(false, frames as i32, SRATE, reg) };
    let mut mix = vec![0.0f64; frames * 2];
    for p in previews {
        // SAFETY: le registre lance par PlayPreviewEx, vivant tant qu'il n'est
        // pas arrete.
        let src = unsafe { (*(p as *mut PreviewRegister)).src };
        let mut buf = vec![f64::NAN; frames * 2];
        let mut tr = Transfer::new(t, SRATE, 2, frames as i32, buf.as_mut_ptr());
        // SAFETY: `src` est un PCM_source : sa table est en tete de l'objet.
        unsafe {
            let vt = &**(src as *const *const Vtbl);
            (vt.get_samples)(src, &mut tr);
        }
        assert_eq!(tr.samples_out, frames as i32);
        for (m, b) in mix.iter_mut().zip(&buf) {
            assert!(b.is_finite());
            *m += b;
        }
    }
    // SAFETY: passage « post ».
    unsafe { cb(true, frames as i32, SRATE, reg) };
    FAKE.lock().unwrap().audio_time = t + frames as f64 / SRATE;
    mix
}

type Entry = extern "C" fn(*mut c_void, *const c_void) -> i32;
type Hook = extern "C" fn(*mut c_void, i32, i32, i32, i32, *mut c_void) -> bool;

fn tick() {
    let t = FAKE.lock().unwrap().timer.expect("timer enregistre");
    // SAFETY: adresse enregistree par le chargeur sous "timer" : void(*)().
    let f: extern "C" fn() = unsafe { std::mem::transmute(t) };
    f();
}

fn run_action(cmd: i32) -> bool {
    let h = FAKE.lock().unwrap().hook.expect("hookcommand2 enregistre");
    // SAFETY: adresse enregistree sous "hookcommand2", signature du SDK.
    let f: Hook = unsafe { std::mem::transmute(h) };
    f(null_mut(), cmd, 0, 0, 0, null_mut())
}

fn cmd_of(id: &str) -> i32 {
    FAKE.lock()
        .unwrap()
        .actions
        .iter()
        .find(|a| a.0 == id)
        .unwrap_or_else(|| panic!("action {id} non enregistree"))
        .2
}

fn take_console() -> String {
    std::mem::take(&mut FAKE.lock().unwrap().console)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn read_log(res: &Path) -> String {
    fs::read_to_string(res.join("cp_studio.log")).unwrap_or_default()
}

#[test]
fn le_vrai_chargeur_dans_un_faux_reaper() {
    // Dossier FIXE ; on n'y efface que des fichiers nommes.
    let root = std::env::temp_dir().join("cp_fake_reaper");
    let target = root.join("target");
    let res = root.join("resource");
    let plugins = res.join("UserPlugins");
    let watch = root.join("watch");
    fs::create_dir_all(&plugins).unwrap();
    fs::create_dir_all(&watch).unwrap();
    for f in [
        plugins.join("reaper_cp_studio.dll"),
        watch.join("cp_module.dll"),
        res.join("cp_studio.log"),
    ] {
        let _ = fs::remove_file(f);
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .current_dir(workspace_root())
        .args([
            "build",
            "-q",
            "-p",
            "cp_loader",
            "-p",
            "cp_module",
            "--target-dir",
        ])
        .arg(&target)
        .status()
        .unwrap();
    assert!(st.success(), "construction du chargeur et du module");
    let loader = plugins.join("reaper_cp_studio.dll");
    let module = watch.join("cp_module.dll");
    fs::copy(target.join("debug").join("reaper_cp_studio.dll"), &loader).unwrap();
    fs::copy(target.join("debug").join("cp_module.dll"), &module).unwrap();
    fs::write(
        plugins.join("reaper_cp_studio.module"),
        format!("{}\n", module.display()),
    )
    .unwrap();
    *RESOURCE.lock().unwrap() = Some(CString::new(res.to_string_lossy().as_bytes()).unwrap());

    // SAFETY: chemin termine par zero.
    let h = unsafe { LoadLibraryW(wide(&loader).as_ptr()) };
    assert!(!h.is_null(), "LoadLibrary du chargeur");
    // SAFETY: handle valide, nom termine par zero.
    let sym = unsafe { GetProcAddress(h, c"ReaperPluginEntry".as_ptr() as *const u8) };
    assert!(!sym.is_null(), "ReaperPluginEntry exporte");
    // SAFETY: signature de REAPER_PLUGIN_ENTRYPOINT.
    let entry: Entry = unsafe { std::mem::transmute(sym) };

    // Un REAPER d'une autre version : refuse proprement.
    let mut info = RawInfo {
        caller_version: 0x20D,
        hwnd_main: null_mut(),
        register: fake_register,
        get_func: fake_get_func,
    };
    assert_eq!(entry(h, &info as *const RawInfo as *const c_void), 0);
    assert!(FAKE.lock().unwrap().timer.is_none(), "rien d'enregistre");

    // Le bon : il se charge, enregistre tout, charge le module.
    info.caller_version = 0x20E;
    assert_eq!(entry(h, &info as *const RawInfo as *const c_void), 1);
    {
        let f = FAKE.lock().unwrap();
        assert!(f.timer.is_some() && f.hook.is_some());
        let ids: Vec<&str> = f.actions.iter().map(|a| a.0.as_str()).collect();
        assert!(ids.contains(&"_CPSTUDIO_RELOAD"));
        assert!(ids.contains(&"_CPSTUDIO_STATUS"));
        assert!(
            ids.contains(&"_CPSTUDIO_BROWSER"),
            "le module a enregistre son action"
        );
    }
    let log = read_log(&res);
    assert!(
        log.contains("module charge : generation 1"),
        "journal :\n{log}"
    );

    // Une action inconnue n'est pas a nous.
    assert!(!run_action(12345));

    // Le battement appelle le module sans rien casser.
    for _ in 0..5 {
        tick();
    }

    // Le son : la boucle de test, tiree comme REAPER la tire.
    assert!(
        FAKE.lock().unwrap().audio_hook.is_some(),
        "crochet audio pose"
    );
    for _ in 0..4 {
        assert!(audio_block(512).iter().all(|&x| x == 0.0));
    }
    assert!(run_action(cmd_of("_CPSTUDIO_TEST_LOOP")));
    assert!(take_console().contains("test loop playing (44100 Hz)"));
    assert_eq!(
        FAKE.lock().unwrap().previews.len(),
        1,
        "un apercu permanent"
    );
    let mut heard = Vec::new();
    let sizes = [512usize, 256, 441, 1024, 64];
    for k in 0..1000 {
        heard.extend(audio_block(sizes[k % sizes.len()]));
    }
    let peak = heard.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    assert!((peak - 0.5).abs() < 1e-3, "crete {peak}");
    // Deux mesures a 120 BPM = 4 s : ce qu'on entend se repete exactement.
    let lp = 4 * 44100 * 2;
    assert!(heard.len() > lp + 44100 * 2);
    let first = heard.iter().position(|&x| x != 0.0).unwrap() & !1;
    for i in first..heard.len() - lp {
        assert_eq!(heard[i], heard[i + lp], "echantillon {i}");
    }
    // L'arret : un fondu de 20 ms, puis le silence ; l'apercu continue.
    assert!(run_action(cmd_of("_CPSTUDIO_TEST_LOOP")));
    assert!(take_console().contains("test loop stopped"));
    let tail: Vec<f64> = (0..10).flat_map(|_| audio_block(512)).collect();
    assert!(
        tail[..2 * 400].iter().any(|&x| x != 0.0),
        "fondu, pas coupure"
    );
    assert!(
        tail[2 * 1000..].iter().all(|&x| x == 0.0),
        "silence apres le fondu"
    );
    assert_eq!(FAKE.lock().unwrap().previews.len(), 1, "jamais arrete");

    // L'etat.
    assert!(run_action(cmd_of("_CPSTUDIO_STATUS")));
    let c = take_console();
    assert!(
        c.contains("state: running") && c.contains("generation: 1"),
        "{c}"
    );
    assert!(
        c.contains("audio: 44100 Hz") && c.contains("hook on"),
        "{c}"
    );
    assert!(
        c.contains("over 0 / under 0, rewinds 0 / skips 0, busy renders 0, lost commands 0"),
        "{c}"
    );

    // Une nouvelle construction : rechargee par le battement seul.
    fs::copy(target.join("debug").join("cp_module.dll"), &module).unwrap();
    fs::File::options()
        .write(true)
        .open(&module)
        .unwrap()
        .set_modified(SystemTime::now())
        .unwrap();
    let t0 = Instant::now();
    while !read_log(&res).contains("module charge : generation 2") {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "rechargement trop lent"
        );
        tick();
        std::thread::sleep(Duration::from_millis(40));
    }
    let delay = t0.elapsed();
    assert!(delay < Duration::from_millis(1500), "pris en {delay:?}");
    assert!(read_log(&res).contains("module pret : generation 2"));
    let cmd_after = cmd_of("_CPSTUDIO_BROWSER");
    assert_eq!(
        FAKE.lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| a.0 == "_CPSTUDIO_BROWSER")
            .count(),
        1,
        "l'action du module n'est enregistree qu'une fois ({cmd_after})"
    );

    // Le rechargement force par l'action.
    assert!(run_action(cmd_of("_CPSTUDIO_RELOAD")));
    assert!(take_console().contains("module reloaded (generation 3)"));

    // La fermeture de REAPER.
    assert_eq!(entry(h, null()), 0);
    {
        let f = FAKE.lock().unwrap();
        assert!(
            f.timer.is_none() && f.hook.is_none(),
            "tout est desenregistre"
        );
        assert!(f.audio_hook.is_none(), "crochet audio retire");
        assert!(f.previews.is_empty(), "apercus arretes");
    }
    assert!(read_log(&res).contains("chargeur arrete"));
    // SAFETY: plus aucun pointeur vers le chargeur n'est tenu.
    unsafe { FreeLibrary(h) };
    eprintln!("rechargement pris en {delay:?}");
}
