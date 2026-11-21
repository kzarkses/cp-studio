//! Le rechargement a chaud du module.
//!
//! Pourquoi il existe : REAPER ne charge une extension qu'au demarrage et ne la
//! recharge jamais. Sans ceci, chaque retouche couterait un redemarrage de REAPER
//! (1 a 3 minutes). C'etait LA raison de l'ancienne separation C++ / Lua.
//!
//! Le principe :
//! - le chargeur (fixe) surveille `cp_module.dll`, le fichier que cargo ecrit ;
//! - une construction neuve et STABLE (taille et date inchangees depuis
//!   [`STABLE`]) est copiee sous un nom unique, et c'est la copie qu'on charge :
//!   Windows ne verrouille qu'elle, cargo peut ecraser l'original ;
//! - la nouvelle copie est chargee et verifiee AVANT de toucher a l'ancienne :
//!   une construction cassee laisse l'ancien module en place ;
//! - l'ancien range son etat (`shutdown`), est libere, sa copie effacee ; le
//!   nouveau recoit l'etat (`init`).
//!
//! Deux interdits, verifies ici et pas laisses a la discipline :
//! - jamais de rechargement pendant un appel du module (`depth`) : son code
//!   disparaitrait sous ses pieds ;
//! - jamais de verrou tenu pendant un appel du module : il rappelle l'hote, qui
//!   doit pouvoir repondre (`state`, `generation` ont leurs propres verrous).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use cp_abi::{FAULT, HostApi, ModuleApi, OK};
use cp_sys::dylib::LoadedModule;

/// Une construction est prise quand elle n'a pas bouge depuis ce delai.
pub const STABLE: Duration = Duration::from_millis(300);

const SHADOW_PREFIX: &str = "cp_module_";

type Stamp = (Option<SystemTime>, u64);

fn stamp_of(p: &Path) -> Option<Stamp> {
    let m = fs::metadata(p).ok()?;
    Some((m.modified().ok(), m.len()))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ce qu'un rechargement reussi a fait.
#[derive(Debug, Clone)]
pub struct Report {
    pub generation: u32,
    pub shadow: PathBuf,
    pub elapsed: Duration,
    /// L'ancien module a-t-il range son etat ? (faux s'il etait en faute ou
    /// absent : le nouveau recoit alors le dernier etat range)
    pub state_saved: bool,
    pub init_ok: bool,
}

struct Inner {
    loaded: Option<LoadedModule>,
    loaded_stamp: Option<Stamp>,
    rejected: Option<Stamp>,
    candidate: Option<(Stamp, Instant)>,
}

pub struct Hot {
    src: PathBuf,
    shadow_dir: PathBuf,
    inner: Mutex<Inner>,
    current: Mutex<Option<ModuleApi>>,
    depth: AtomicU32,
    state: Mutex<Vec<u8>>,
    generation: AtomicU32,
    fault: Mutex<Option<String>>,
}

impl Hot {
    /// `src` : le fichier que cargo ecrit. `shadow_dir` : ou vivent les copies.
    pub fn new(src: PathBuf, shadow_dir: PathBuf) -> Hot {
        Hot {
            src,
            shadow_dir,
            inner: Mutex::new(Inner {
                loaded: None,
                loaded_stamp: None,
                rejected: None,
                candidate: None,
            }),
            current: Mutex::new(None),
            depth: AtomicU32::new(0),
            state: Mutex::new(Vec::new()),
            generation: AtomicU32::new(0),
            fault: Mutex::new(None),
        }
    }

    pub fn source(&self) -> &Path {
        &self.src
    }

    /// Appele par l'hote quand le module range son etat.
    pub fn store_state(&self, data: &[u8]) {
        let mut s = lock(&self.state);
        s.clear();
        s.extend_from_slice(data);
    }

    pub fn generation(&self) -> u32 {
        self.generation.load(SeqCst)
    }

    pub fn fault(&self) -> Option<String> {
        lock(&self.fault).clone()
    }

    pub fn is_loaded(&self) -> bool {
        lock(&self.current).is_some()
    }

    fn set_fault(&self, why: &str) {
        let mut f = lock(&self.fault);
        if f.is_none() {
            *f = Some(why.to_string());
        }
    }

    /// Appelle le module courant. `None` : pas de module, ou module en faute
    /// (on ne l'appelle plus jusqu'a la prochaine construction).
    pub fn call(&self, f: impl FnOnce(&ModuleApi) -> i32) -> Option<i32> {
        if lock(&self.fault).is_some() {
            return None;
        }
        let api = (*lock(&self.current))?;
        self.depth.fetch_add(1, SeqCst);
        let r = f(&api);
        self.depth.fetch_sub(1, SeqCst);
        if r == FAULT {
            self.set_fault("le module a panique (detail dans le journal)");
        }
        Some(r)
    }

    /// Verifie la source et recharge si une construction neuve est stable.
    /// `force` recharge meme si rien n'a change. `None` = rien a faire.
    pub fn poll(
        &self,
        now: Instant,
        host: &'static HostApi,
        force: bool,
    ) -> Option<Result<Report, String>> {
        if self.depth.load(SeqCst) > 0 {
            return None;
        }
        let mut inner = self.inner.try_lock().ok()?;
        let stamp = stamp_of(&self.src);
        if force {
            if stamp.is_none() {
                return Some(Err(format!("module introuvable : {}", self.src.display())));
            }
        } else {
            let s = stamp?;
            if inner.loaded_stamp == Some(s) || inner.rejected == Some(s) {
                inner.candidate = None;
                return None;
            }
            match inner.candidate {
                Some((c, since)) if c == s => {
                    if now.saturating_duration_since(since) < STABLE {
                        return None;
                    }
                }
                _ => {
                    inner.candidate = Some((s, now));
                    return None;
                }
            }
        }
        inner.candidate = None;
        let r = self.swap(&mut inner, host);
        match &r {
            Ok(_) => {
                inner.loaded_stamp = stamp;
                inner.rejected = None;
            }
            Err(_) => inner.rejected = stamp,
        }
        Some(r)
    }

    fn swap(&self, inner: &mut Inner, host: &'static HostApi) -> Result<Report, String> {
        let t0 = Instant::now();
        let next = self.generation() + 1;
        fs::create_dir_all(&self.shadow_dir)
            .map_err(|e| format!("dossier des copies {} : {e}", self.shadow_dir.display()))?;
        let shadow = self
            .shadow_dir
            .join(format!("{SHADOW_PREFIX}{}_{next}.dll", std::process::id()));
        fs::copy(&self.src, &shadow)
            .map_err(|e| format!("copie de {} : {e}", self.src.display()))?;
        let new = match LoadedModule::load(&shadow) {
            Ok(m) => m,
            Err(e) => {
                let _ = fs::remove_file(&shadow);
                return Err(e);
            }
        };

        // L'ancien range son etat, puis disparait. Plus aucun appel ne le
        // trouve des cette ligne.
        let mut state_saved = false;
        if let Some(old) = inner.loaded.take() {
            *lock(&self.current) = None;
            if lock(&self.fault).is_none() {
                self.depth.fetch_add(1, SeqCst);
                state_saved = old.shutdown() == OK;
                self.depth.fetch_sub(1, SeqCst);
            }
            let old_path = old.path().to_path_buf();
            drop(old);
            let _ = fs::remove_file(old_path);
        }

        self.generation.store(next, SeqCst);
        *lock(&self.fault) = None;
        let state = lock(&self.state).clone();
        self.depth.fetch_add(1, SeqCst);
        let init_ok = new.init(host, &state) == OK;
        self.depth.fetch_sub(1, SeqCst);
        if !init_ok {
            self.set_fault("init du module a echoue (detail dans le journal)");
        }
        *lock(&self.current) = Some(new.api());
        inner.loaded = Some(new);
        Ok(Report {
            generation: next,
            shadow,
            elapsed: t0.elapsed(),
            state_saved,
            init_ok,
        })
    }

    /// Decharge le module (fermeture de REAPER). Il range son etat s'il est sain.
    pub fn unload(&self) {
        if self.depth.load(SeqCst) > 0 {
            return;
        }
        let mut inner = lock(&self.inner);
        if let Some(old) = inner.loaded.take() {
            *lock(&self.current) = None;
            if lock(&self.fault).is_none() {
                self.depth.fetch_add(1, SeqCst);
                let _ = old.shutdown();
                self.depth.fetch_sub(1, SeqCst);
            }
            let p = old.path().to_path_buf();
            drop(old);
            let _ = fs::remove_file(p);
        }
        inner.loaded_stamp = None;
    }
}

/// Efface les copies laissees par une session precedente. Ne touche QUE les
/// fichiers `cp_module_*.dll` de ce dossier, jamais un sous-dossier, jamais
/// recursivement. Une copie encore chargee (autre REAPER ouvert) est verrouillee
/// par Windows et reste en place.
pub fn clean_shadow_dir(dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let is_copy = name.starts_with(SHADOW_PREFIX) && name.ends_with(".dll");
        if is_copy && e.file_type().is_ok_and(|t| t.is_file()) && fs::remove_file(e.path()).is_ok()
        {
            n += 1;
        }
    }
    n
}
