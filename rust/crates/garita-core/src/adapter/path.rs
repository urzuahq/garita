use std::path::{Component, Path, PathBuf};

/// Joins a tool's cwd-relative `path` onto `cwd` (repository-relative), giving a
/// repository-relative path with `/` separators and no `.` segments. Absolute paths are only
/// normalised.
pub fn rebase(cwd: &str, path: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = is_absolute(&path);
    let mut parts: Vec<&str> = Vec::new();
    let base = if absolute { "" } else { cwd };
    let base = base.replace('\\', "/");
    for segment in base.split('/').chain(path.split('/')) {
        match segment {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// `path` written relative to the repository `root` with `/` separators (`.` for the root
/// itself) when it lies inside it, and as its normalised absolute path when it does not. `.` and
/// `..` components are resolved lexically; a relative `path` is taken to be relative to `root`.
pub fn within_root(root: &Path, path: &Path) -> String {
    let root = normalise(root);
    let full = if path.is_absolute() {
        normalise(path)
    } else {
        normalise(&root.join(path))
    };
    match full.strip_prefix(&root) {
        Ok(rest) => {
            let parts: Vec<_> = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            if parts.is_empty() {
                ".".to_string()
            } else {
                parts.join("/")
            }
        }
        Err(_) => full.display().to_string(),
    }
}

fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/') || matches!(bytes, [drive, b':', b'/', ..] if drive.is_ascii_alphabetic())
}
