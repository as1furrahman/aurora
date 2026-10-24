//! aurora-core must stay pure: no filesystem, process or network access in library code
//! (ADR-0006). All run-directory I/O belongs to aurora-fs; process launch to a later crate.

use std::path::Path;

fn scan(dir: &Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("src dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            scan(&path, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("source");
            for needle in ["std::fs", "std::process", "std::net", "std::env::set_var", "File::"] {
                if text.contains(needle) {
                    offenders.push(format!("{} uses {needle}", path.display()));
                }
            }
        }
    }
}

#[test]
fn core_library_performs_no_io() {
    let mut offenders = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut offenders);
    assert!(offenders.is_empty(), "{offenders:#?}");
}
