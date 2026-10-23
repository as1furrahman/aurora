//! Apply the install overlays to every SKILL.md under a directory and report the result.
//! Usage: cargo run -p aurora-skills --example overlay_corpus -- <dir>

use std::path::Path;

use aurora_skills::overlay::strip_frontmatter_keys;

fn visit(dir: &Path, total: &mut (usize, usize, usize)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && !path.ends_with(".git") {
            visit(&path, total);
        } else if path.file_name().is_some_and(|n| n == "SKILL.md") {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            total.0 += 1;
            match strip_frontmatter_keys(&text, &["allowed-tools", "hooks"]) {
                Ok((_, removed)) if !removed.is_empty() => {
                    total.1 += 1;
                    println!("stripped {:?}: {}", removed, path.display());
                }
                Ok(_) => {}
                Err(e) => {
                    total.2 += 1;
                    println!("REFUSED {}: {e}", path.display());
                }
            }
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("usage: overlay_corpus <dir>");
    let mut total = (0, 0, 0);
    visit(Path::new(&root), &mut total);
    println!("{} SKILL.md files, {} changed, {} refused", total.0, total.1, total.2);
}
