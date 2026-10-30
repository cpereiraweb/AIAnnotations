//! Locating the AI CLI binaries (port of Laravel `ClaudeBinary`, Inv §4.1).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Folders searched after the user's PATH.
fn system_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/usr/local/bin", "/usr/bin", "/bin"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
    }
    dirs
}

/// File names tried for `name` inside one folder.
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|ext| dir.join(format!("{name}.{ext}")))
            .collect()
    } else {
        vec![dir.join(name)]
    }
}

/// Absolute path to an existing, executable file (on Windows: existing file).
fn usable(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolves the binary: a non-blank override (used as is), then `home/.local/bin`,
/// then each `path_var` folder plus the system folders, else the bare `name`.
pub fn resolve(
    name: &str,
    override_bin: Option<&OsStr>,
    home: &Path,
    path_var: Option<&OsStr>,
) -> PathBuf {
    if let Some(o) = override_bin
        && !o.to_string_lossy().trim().is_empty()
    {
        return PathBuf::from(o);
    }
    let mut dirs = vec![home.join(".local").join("bin")];
    if let Some(p) = path_var {
        dirs.extend(std::env::split_paths(p));
    }
    dirs.extend(system_dirs());
    dirs.iter()
        .flat_map(|d| candidates(d, name))
        .find(|c| usable(c))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// PATH for the child process: the binary's folder, `current`, then the system
/// folders, without duplicates.
pub fn path_env(bin: &Path, current: Option<&OsStr>) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = bin.parent().filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(parent.to_path_buf());
    }
    if let Some(c) = current {
        dirs.extend(std::env::split_paths(c));
    }
    dirs.extend(system_dirs());
    let mut unique: Vec<PathBuf> = Vec::new();
    for d in dirs {
        if !d.as_os_str().is_empty() && !unique.contains(&d) {
            unique.push(d);
        }
    }
    std::env::join_paths(unique)
        .unwrap_or_else(|_| current.map(OsStr::to_os_string).unwrap_or_default())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make(dir: &Path, name: &str, mode: u32) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn override_is_used_as_is_even_if_missing() {
        let home = tempfile::tempdir().unwrap();
        let got = resolve("claude", Some(OsStr::new("/x/y/claude")), home.path(), None);
        assert_eq!(got, PathBuf::from("/x/y/claude"));
    }

    #[test]
    fn blank_override_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        let bin = make(&home.path().join(".local/bin"), "claude", 0o755);
        let got = resolve("claude", Some(OsStr::new("   ")), home.path(), None);
        assert_eq!(got, bin);
    }

    #[test]
    fn local_bin_wins_over_path() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let local = make(&home.path().join(".local/bin"), "claude", 0o755);
        make(other.path(), "claude", 0o755);
        let got = resolve("claude", None, home.path(), Some(other.path().as_os_str()));
        assert_eq!(got, local);
    }

    #[test]
    fn first_executable_in_path_wins_and_non_executable_is_skipped() {
        let home = tempfile::tempdir().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let c = tempfile::tempdir().unwrap();
        make(a.path(), "claude", 0o644);
        let wanted = make(b.path(), "claude", 0o755);
        make(c.path(), "claude", 0o755);
        let path = std::env::join_paths([a.path(), b.path(), c.path()]).unwrap();
        let got = resolve("claude", None, home.path(), Some(&path));
        assert_eq!(got, wanted);
    }

    #[test]
    fn falls_back_to_bare_name() {
        let home = tempfile::tempdir().unwrap();
        let got = resolve("aiannotations-no-such-bin", None, home.path(), None);
        assert_eq!(got, PathBuf::from("aiannotations-no-such-bin"));
    }

    #[test]
    fn path_env_starts_with_binary_folder() {
        let got = path_env(Path::new("/opt/x/bin/claude"), Some(OsStr::new("/a:/b")));
        let dirs: Vec<PathBuf> = std::env::split_paths(&got).collect();
        assert_eq!(dirs[0], PathBuf::from("/opt/x/bin"));
        assert_eq!(dirs[1], PathBuf::from("/a"));
        assert!(dirs.contains(&PathBuf::from("/usr/bin")));
    }

    #[test]
    fn path_env_does_not_duplicate() {
        let got = path_env(
            Path::new("/usr/bin/claude"),
            Some(OsStr::new("/usr/bin:/a")),
        );
        let dirs: Vec<PathBuf> = std::env::split_paths(&got).collect();
        assert_eq!(
            dirs.iter().filter(|d| **d == Path::new("/usr/bin")).count(),
            1
        );
    }
}
