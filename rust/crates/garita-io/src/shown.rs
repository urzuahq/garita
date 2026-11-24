//! How a path or a git diagnostic is named in a report: relative to the repository root when it
//! lies inside it.

use std::fs;
use std::path::{Path, PathBuf};

use garita_core::adapter::within_root;

/// `path` as the report names it. A path that is not lexically inside `root` is tried again with
/// the real location of its longest existing ancestor, so a symlink into the repository is
/// still shown relative; one that is really outside keeps the spelling it was given.
pub fn shown(root: &Path, path: &Path) -> String {
    let lexical = within_root(root, path);
    if !Path::new(&lexical).is_absolute() {
        return lexical;
    }
    let real = within_root(&real_root(root), &resolved(path));
    if Path::new(&real).is_absolute() {
        lexical
    } else {
        real
    }
}

fn real_root(root: &Path) -> PathBuf {
    fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// `path` with its longest existing ancestor made real; the rest is appended as spelled.
fn resolved(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = fs::canonicalize(cur) {
            return rest.iter().rev().fold(real, |p, c| p.join(c));
        }
        match (cur.file_name(), cur.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// `text` with the repository root, as held and as resolved, removed where a `/` follows it.
pub fn without_root(root: &Path, text: &str) -> String {
    let mut out = text.to_string();
    for form in [root.to_path_buf(), real_root(root)] {
        let prefix = format!("{}/", form.display());
        out = out.replace(&prefix, "");
    }
    out
}
