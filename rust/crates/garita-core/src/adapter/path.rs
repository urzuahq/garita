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

fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/') || matches!(bytes, [drive, b':', b'/', ..] if drive.is_ascii_alphabetic())
}
