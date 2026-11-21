//! Une panique ne traverse JAMAIS une frontiere `extern "C"` : elle y ferait
//! avorter le processus, donc REAPER entier, avec le projet ouvert. Chaque
//! fonction appelee par REAPER, par Windows ou par l'autre DLL passe par ici.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Execute `f`, rend `None` si elle a panique.
pub fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}
