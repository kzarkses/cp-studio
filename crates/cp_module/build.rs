// L'heure de construction, pour que la fenetre de test montre QUELLE
// construction tourne. Sans directive rerun-if, cargo relance ce script a
// chaque modification d'un fichier du crate.
fn main() {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=CP_BUILD_UNIX={t}");
}
