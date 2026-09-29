//! Windows path handling that behaves the same on every OS (so it's testable anywhere):
//! backslash separators, case-insensitive comparison, `.`/`..` resolution.

/// Normalizes a Windows path: trims quotes/whitespace, `/` → `\`, resolves `.` and `..`,
/// drops trailing separators (except for a drive root like `C:\`).
pub fn normalize(path: &str) -> String {
    let raw = path.trim().trim_matches('"').replace('/', "\\");
    let (prefix, rest) = split_root(&raw);
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("\\");
    if prefix.is_empty() {
        joined
    } else if joined.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}{joined}")
    }
}

/// Splits off `C:\` or `\\server\share\` so `..` can't climb above it.
fn split_root(path: &str) -> (&str, &str) {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return (&path[..3], &path[3..]);
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return (&path[..2], &path[2..]);
    }
    if let Some(unc) = path.strip_prefix("\\\\") {
        // \\server\share\ is the root; everything after it is the path.
        let mut separators = 0;
        for (i, c) in unc.char_indices() {
            if c == '\\' {
                separators += 1;
                if separators == 2 {
                    let end = 2 + i + 1;
                    return (&path[..end], &path[end..]);
                }
            }
        }
        return (path, "");
    }
    ("", path)
}

/// Case-insensitive equality of two normalized paths.
pub fn eq(a: &str, b: &str) -> bool {
    normalize(a).eq_ignore_ascii_case_unicode(&normalize(b))
}

/// The last component (`C:\Games\x.exe` → `x.exe`).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// File name without its extension.
pub fn file_stem(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// The parent folder of a normalized path, if it has one.
pub fn parent(path: &str) -> Option<&str> {
    let index = path.rfind('\\')?;
    if index == 0 {
        return None;
    }
    // Keep the separator for a drive root ("C:\x" -> "C:\").
    if index == 2 && path.as_bytes().get(1) == Some(&b':') {
        return Some(&path[..3]);
    }
    Some(&path[..index])
}

/// If `path` is inside `dir`, the part after `dir\` (both already normalized; case-insensitive).
pub fn relative_to<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    let dir = dir.trim_end_matches('\\');
    if path.len() <= dir.len() + 1 {
        return None;
    }
    let (head, tail) = path.split_at(dir.len());
    if !head.eq_ignore_ascii_case_unicode(dir) || !tail.starts_with('\\') {
        return None;
    }
    Some(&tail[1..])
}

/// Unicode-aware case-insensitive comparison (Windows file names are case-insensitive).
pub trait EqIgnoreCase {
    fn eq_ignore_ascii_case_unicode(&self, other: &str) -> bool;
}

impl EqIgnoreCase for str {
    fn eq_ignore_ascii_case_unicode(&self, other: &str) -> bool {
        self.eq_ignore_ascii_case(other) || self.to_lowercase() == other.to_lowercase()
    }
}

/// A case-insensitive key for maps keyed by path or name.
pub fn key(text: &str) -> String {
    text.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(
            normalize(r#" "C:/Games/Foo/../Bar/./x.exe" "#),
            r"C:\Games\Bar\x.exe"
        );
        assert_eq!(normalize(r"C:\"), r"C:\");
        assert_eq!(normalize(r"C:\Games\"), r"C:\Games");
        assert_eq!(normalize(r"C:\..\..\x"), r"C:\x");
        assert_eq!(normalize(r"\\nas\games\..\x\g.exe"), r"\\nas\games\x\g.exe");
    }

    #[test]
    fn compares_case_insensitively() {
        assert!(eq(r"c:\games\ELDEN RING", r"C:\Games\elden ring\"));
        assert!(!eq(r"C:\Games\A", r"C:\Games\B"));
    }

    #[test]
    fn components() {
        assert_eq!(file_name(r"C:\a\b\Game.exe"), "Game.exe");
        assert_eq!(file_stem(r"C:\a\b\Game.v2.exe"), "Game.v2");
        assert_eq!(parent(r"C:\a\b\Game.exe"), Some(r"C:\a\b"));
        assert_eq!(parent(r"C:\Game.exe"), Some(r"C:\"));
        assert_eq!(parent("Game.exe"), None);
    }

    #[test]
    fn relative() {
        assert_eq!(
            relative_to(r"D:\Steam\common\Game\bin\g.exe", r"d:\steam\common"),
            Some(r"Game\bin\g.exe")
        );
        assert_eq!(
            relative_to(r"D:\Steam\commonX\g.exe", r"D:\Steam\common"),
            None
        );
        assert_eq!(relative_to(r"D:\Steam\common", r"D:\Steam\common"), None);
    }
}
