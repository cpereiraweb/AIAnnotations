//! Safe writes of user config files: symlink-aware atomic write and rotating backups.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const BACKUP_INFIX: &str = ".bak-aiannotations-";
const KEEP_BACKUPS: usize = 3;
const MAX_LINK_HOPS: usize = 40;

/// Follows symlinks (also chained, also broken ones) to the real file path.
/// Without a symlink, returns `path` itself.
pub fn target(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        let is_link = fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink());
        if !is_link {
            break;
        }
        let Ok(link) = fs::read_link(&current) else {
            break;
        };
        current = if link.is_absolute() {
            link
        } else {
            current.parent().unwrap_or(Path::new("")).join(link)
        };
    }
    current
}

/// Writes `contents` to the real file behind `path` through a temp file in the same
/// folder plus `rename`. Keeps the existing file's permissions and creates missing folders.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let real = target(path);
    let dir = real
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = real
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mode_source = fs::metadata(&real).ok();
    let mut attempt = 0u32;
    let (tmp_path, mut tmp) = loop {
        let tmp_path = dir.join(format!(".{name}.tmp-{}-{attempt}", std::process::id()));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        match opts.open(&tmp_path) {
            Ok(f) => break (tmp_path, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        tmp.write_all(contents.as_bytes())?;
        tmp.sync_all()?;
        match &mode_source {
            Some(meta) => fs::set_permissions(&tmp_path, meta.permissions())?,
            None => set_mode(&tmp_path, 0o644)?,
        }
        fs::rename(&tmp_path, &real)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

/// Copies the real file to `<target>.bak-aiannotations-<stamp>` (or `-1`, `-2`... when the
/// name is taken), created 0600 and then given the original's permissions, and prunes
/// older backups of that file beyond the 3 most recent.
pub fn backup(path: &Path, stamp: &str) -> io::Result<PathBuf> {
    let real = target(path);
    let contents = fs::read(&real)?;
    let permissions = fs::metadata(&real)?.permissions();
    let base = format!("{}{BACKUP_INFIX}{stamp}", real.display());

    let mut n = next_suffix(&real, stamp);
    let (candidate, mut file) = loop {
        let candidate = PathBuf::from(if n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        });
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        match opts.open(&candidate) {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    file.write_all(&contents)?;
    drop(file);
    fs::set_permissions(&candidate, permissions)?;

    prune(&real)?;
    Ok(candidate)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Existing backups of `real` as `(stamp, suffix, path)`; no suffix counts as 0.
/// Only names matching exactly `<file>.bak-aiannotations-<digits>[-<digits>]` qualify.
fn existing_backups(real: &Path) -> io::Result<Vec<(String, u64, PathBuf)>> {
    let dir = real
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let prefix = format!(
        "{}{BACKUP_INFIX}",
        real.file_name().unwrap_or_default().to_string_lossy()
    );
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(rest) = file_name
            .to_string_lossy()
            .strip_prefix(&prefix)
            .map(str::to_owned)
        else {
            continue;
        };
        let (stamp, n) = match rest.split_once('-') {
            None => (rest.as_str(), Some(0)),
            Some((stamp, n)) => (
                stamp,
                all_digits(n).then(|| n.parse::<u64>().ok()).flatten(),
            ),
        };
        if let (true, Some(n)) = (all_digits(stamp), n) {
            found.push((stamp.to_owned(), n, entry.path()));
        }
    }
    Ok(found)
}

/// First suffix after the highest one in use for this stamp, so that pruned names
/// are not reused within the same second.
fn next_suffix(real: &Path, stamp: &str) -> u64 {
    existing_backups(real)
        .unwrap_or_default()
        .iter()
        .filter(|(s, _, _)| s == stamp)
        .map(|(_, n, _)| n + 1)
        .max()
        .unwrap_or(0)
}

fn prune(real: &Path) -> io::Result<()> {
    let mut found = existing_backups(real)?;
    found.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let excess = found.len().saturating_sub(KEEP_BACKUPS);
    for (_, _, path) in found.into_iter().take(excess) {
        fs::remove_file(path)?;
    }
    Ok(())
}
