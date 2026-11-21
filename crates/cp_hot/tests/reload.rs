//! Le rechargement a chaud, joue de bout en bout hors de REAPER.
//!
//! Un seul test, parce que l'hote s'installe une fois par processus. Il
//! construit trois variantes du module de test (etiquettes A, B) dans un dossier
//! de construction a lui, puis rejoue ce qui se passera dans REAPER :
//! chargement, rechargement avec passage de l'etat, panique rattrapee,
//! construction cassee qui laisse l'ancien en place, dechargement.

use std::ffi::{CStr, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cp_abi::{FAULT, OK};
use cp_hot::Hot;
use cp_sys::host::{self, Host};

struct TestHost {
    hot: Arc<Hot>,
    logs: Arc<Mutex<Vec<String>>>,
}

impl Host for TestHost {
    fn log(&self, _level: u32, msg: &str) {
        self.logs.lock().unwrap().push(msg.to_string());
    }
    fn get_func(&self, _name: &CStr) -> *mut c_void {
        std::ptr::null_mut()
    }
    fn register_action(&self, _id: &str, _name: &str) -> i32 {
        1000
    }
    fn create_window(&self, _kind: u32, _title: &str, _w: i32, _h: i32) -> isize {
        0
    }
    fn store_state(&self, data: &[u8]) {
        self.hot.store_state(data);
    }
    fn generation(&self) -> u32 {
        self.hot.generation()
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Construit le module de test avec une etiquette, dans un dossier a part (le
/// `cargo test` qui nous lance tient le verrou de `target/`).
fn build_fixture(target: &Path, tag: &str) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .current_dir(workspace_root())
        .args(["build", "-q", "-p", "cp_fixture", "--target-dir"])
        .arg(target)
        .env("CP_FIXTURE_TAG", tag)
        .status()
        .expect("cargo introuvable");
    assert!(st.success(), "construction de la variante {tag}");
    let out = target.join("debug").join("cp_fixture.dll");
    let dst = target.join(format!("fixture_{tag}.dll"));
    fs::copy(&out, &dst).unwrap();
    dst
}

fn copies(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("cp_module_"))
                .count()
        })
        .unwrap_or(0)
}

/// Deux sondages espaces de plus que STABLE : la construction est prise.
fn settle(
    hot: &Hot,
    h: &'static cp_abi::HostApi,
    t: &mut Instant,
) -> Option<Result<cp_hot::Report, String>> {
    assert!(
        hot.poll(*t, h, false).is_none(),
        "premier sondage : on attend la stabilite"
    );
    *t += cp_hot::STABLE + Duration::from_millis(50);
    hot.poll(*t, h, false)
}

#[test]
fn rechargement_de_bout_en_bout() {
    // Un dossier FIXE, reutilise d'un passage a l'autre (les constructions y
    // restent incrementales). On n'y efface que des fichiers nommes, jamais
    // recursivement.
    let root = std::env::temp_dir().join("cp_hot_test");
    let target = root.join("target");
    let shadow = root.join("shadow");
    let watch = root.join("watch");
    fs::create_dir_all(&watch).unwrap();
    let src = watch.join("cp_module.dll");
    let _ = fs::remove_file(&src);
    cp_hot::clean_shadow_dir(&shadow);
    let a = build_fixture(&target, "A");
    let b = build_fixture(&target, "B");

    let hot = Arc::new(Hot::new(src.clone(), shadow.clone()));
    let logs = Arc::new(Mutex::new(Vec::new()));
    let h = host::install(Box::new(TestHost {
        hot: hot.clone(),
        logs: logs.clone(),
    }));
    let has = |s: &str| logs.lock().unwrap().iter().any(|l| l == s);
    let tick = || hot.call(|api| (api.tick)());
    let mut t = Instant::now();

    // 1. Rien a charger : rien ne se passe.
    assert!(hot.poll(t, h, false).is_none());

    // 2. Premier chargement.
    fs::copy(&a, &src).unwrap();
    let r = settle(&hot, h, &mut t)
        .expect("chargement")
        .expect("A se charge");
    assert_eq!(r.generation, 1);
    assert!(r.init_ok);
    assert!(has("init A gen 1 n 0"));
    for _ in 0..3 {
        assert_eq!(tick(), Some(OK));
    }
    assert!(has("tick A 3"));
    assert_eq!(copies(&shadow), 1);

    // 3. Rechargement : B prend la suite du compte de A.
    fs::copy(&b, &src).unwrap();
    let r = settle(&hot, h, &mut t)
        .expect("rechargement")
        .expect("B se charge");
    assert_eq!(r.generation, 2);
    assert!(r.state_saved, "A a range son etat");
    assert!(has("shutdown A 3"));
    assert!(has("init B gen 2 n 3"));
    assert_eq!(tick(), Some(OK));
    assert!(has("tick B 4"));
    assert_eq!(copies(&shadow), 1, "la copie de A est effacee");

    // 4. Une panique dans le module : rattrapee, le module est mis de cote.
    assert_eq!(hot.call(|api| (api.on_action)(99)), Some(FAULT));
    assert!(hot.fault().is_some());
    assert!(
        logs.lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("panne voulue"))
    );
    assert_eq!(tick(), None, "un module en faute n'est plus appele");

    // 5. La construction suivante repart du dernier etat RANGE (celui de A,
    //    puisque B en faute n'a pas pu ranger le sien).
    fs::copy(&a, &src).unwrap();
    let r = settle(&hot, h, &mut t)
        .expect("rechargement")
        .expect("A se recharge");
    assert_eq!(r.generation, 3);
    assert!(!r.state_saved, "B en faute ne range rien");
    assert!(hot.fault().is_none());
    assert!(has("init A gen 3 n 3"));
    assert_eq!(tick(), Some(OK));
    assert!(has("tick A 4"));

    // 6. Une construction cassee : refusee, l'ancien module continue.
    fs::write(&src, b"pas une dll").unwrap();
    let r = settle(&hot, h, &mut t).expect("tentative");
    assert!(r.is_err(), "un fichier qui n'est pas une DLL est refuse");
    assert_eq!(hot.generation(), 3);
    assert_eq!(tick(), Some(OK));
    assert!(has("tick A 5"));
    assert!(
        hot.poll(t + Duration::from_secs(5), h, false).is_none(),
        "pas de nouvel essai sur le meme fichier"
    );
    assert_eq!(copies(&shadow), 1);

    // 7. Dechargement : A range son etat, sa copie disparait.
    hot.unload();
    assert!(has("shutdown A 5"));
    assert!(!hot.is_loaded());
    assert_eq!(copies(&shadow), 0);

    // 8. Le menage ne touche que nos copies.
    fs::write(shadow.join("cp_module_1_1.dll"), b"x").unwrap();
    fs::write(shadow.join("autre.txt"), b"x").unwrap();
    assert_eq!(cp_hot::clean_shadow_dir(&shadow), 1);
    assert!(shadow.join("autre.txt").exists());
    let _ = fs::remove_file(shadow.join("autre.txt"));
    let _ = fs::remove_file(&src);
}
