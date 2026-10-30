use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Distinct from the Laravel app's `~/.local/share/aiannotations/` so both can coexist.
pub const APP_IDENTIFIER: &str = "io.github.cpereiraweb.aiannotations";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub spool_dir: PathBuf,
    pub db_path: PathBuf,
    pub ai_workdir: PathBuf,
    pub claude_dir: PathBuf,
    pub codex_home: PathBuf,
}

/// Non-empty value of an environment variable; empty counts as absent.
fn non_empty(get: &impl Fn(&str) -> Option<OsString>, key: &str) -> Option<PathBuf> {
    get(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `fs::canonicalize` without the `\\?\` verbatim prefix Windows adds: git, the hooks'
/// `cwd` and every stored project path use the plain form, and verbatim paths
/// are not understood by git or by plain string comparison.
pub fn real_path(path: &Path) -> std::io::Result<PathBuf> {
    let real = std::fs::canonicalize(path)?;
    Ok(PathBuf::from(plain_path(&real.to_string_lossy())))
}

/// `path` without the Windows verbatim prefix, for paths that are not canonicalized: the
/// Codex desktop app writes session cwds as `\\?\C:\…`. Unchanged elsewhere.
pub fn plain_path(path: &str) -> String {
    if cfg!(windows)
        && let Some(plain) = strip_verbatim(path)
    {
        return plain;
    }
    path.to_string()
}

/// `\\?\C:\x` becomes `C:\x` and `\\?\UNC\srv\share` becomes `\\srv\share`; `None` when
/// the path is not in verbatim form.
fn strip_verbatim(path: &str) -> Option<String> {
    let rest = path.strip_prefix(r"\\?\")?;
    match rest.strip_prefix(r"UNC\") {
        Some(unc) => Some(format!(r"\\{unc}")),
        None => Some(rest.to_string()),
    }
}

impl Paths {
    pub fn from_env(
        get: impl Fn(&str) -> Option<OsString>,
        home: &Path,
        data_base: &Path,
    ) -> Paths {
        let data_dir = non_empty(&get, "AIANNOTATIONS_DATA_DIR")
            .unwrap_or_else(|| data_base.join(APP_IDENTIFIER));
        Paths {
            spool_dir: data_dir.join("spool"),
            db_path: data_dir.join("aiannotations.sqlite"),
            ai_workdir: data_dir.join("ai-workdir"),
            claude_dir: non_empty(&get, "CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|| home.join(".claude")),
            codex_home: non_empty(&get, "CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            data_dir,
        }
    }

    pub fn resolve() -> Paths {
        let get = |k: &str| std::env::var_os(k);
        let home = home_dir(get);
        let data_base = dirs::data_dir().unwrap_or_else(|| home.join(".local").join("share"));
        Paths::from_env(get, &home, &data_base)
    }
}

/// Port of Laravel `app/Support/Home.php`: env var, then the OS home, then the temp dir.
pub fn home_dir(get: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    non_empty(&get, var)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).map(OsString::from)
    }

    #[test]
    fn default_data_dir_is_not_laravel_dir() {
        let p = Paths::from_env(env(&[]), Path::new("/h"), Path::new("/d"));
        assert_eq!(
            p.data_dir,
            PathBuf::from("/d/io.github.cpereiraweb.aiannotations")
        );
        assert_eq!(p.spool_dir, p.data_dir.join("spool"));
        assert_eq!(p.db_path, p.data_dir.join("aiannotations.sqlite"));
        assert_eq!(p.ai_workdir, p.data_dir.join("ai-workdir"));
        assert_eq!(p.claude_dir, PathBuf::from("/h/.claude"));
        assert_eq!(p.codex_home, PathBuf::from("/h/.codex"));
    }

    #[test]
    fn env_overrides_win() {
        let get = env(&[
            ("AIANNOTATIONS_DATA_DIR", "/x"),
            ("CLAUDE_CONFIG_DIR", "/c"),
            ("CODEX_HOME", "/k"),
        ]);
        let p = Paths::from_env(get, Path::new("/h"), Path::new("/d"));
        assert_eq!(p.data_dir, PathBuf::from("/x"));
        assert_eq!(p.claude_dir, PathBuf::from("/c"));
        assert_eq!(p.codex_home, PathBuf::from("/k"));
    }

    #[test]
    fn empty_env_counts_as_absent() {
        let p = Paths::from_env(env(&[("CODEX_HOME", "")]), Path::new("/h"), Path::new("/d"));
        assert_eq!(p.codex_home, PathBuf::from("/h/.codex"));
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(strip_verbatim(r"\\?\C:\a\b").as_deref(), Some(r"C:\a\b"));
        assert_eq!(
            strip_verbatim(r"\\?\UNC\srv\share\a").as_deref(),
            Some(r"\\srv\share\a")
        );
        assert_eq!(strip_verbatim(r"C:\a\b"), None);
        assert_eq!(strip_verbatim("/a/b"), None);
    }

    #[test]
    fn real_path_resolves_an_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let real = real_path(dir.path()).unwrap();
        assert!(real.is_dir());
        assert!(!real.to_string_lossy().starts_with(r"\\?\"));
        assert!(real_path(&dir.path().join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn home_dir_fallbacks() {
        assert_eq!(home_dir(env(&[("HOME", "/a")])), PathBuf::from("/a"));
        let expected = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        assert_eq!(home_dir(env(&[("HOME", "")])), expected);
    }
}
