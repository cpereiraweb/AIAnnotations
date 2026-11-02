use aiannotations_core::installer::fsio;
use std::fs;
use std::path::{Path, PathBuf};

fn backups(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.contains(".bak-aiannotations-"))
        .collect();
    v.sort();
    v
}

fn file(dir: &Path) -> PathBuf {
    let p = dir.join("settings.json");
    fs::write(&p, "{}").unwrap();
    p
}

#[cfg(unix)]
#[test]
fn backup_keeps_restricted_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let p = file(dir.path());
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    let b = fsio::backup(&p, "20261031120000").unwrap();
    assert_eq!(
        fs::metadata(&b).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&b).unwrap(), "{}");
}

#[test]
fn same_second_backups_do_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let p = file(dir.path());
    let a = fsio::backup(&p, "20261031120000").unwrap();
    fs::write(&p, "{\"x\":1}").unwrap();
    let b = fsio::backup(&p, "20261031120000").unwrap();
    assert_ne!(a, b);
    assert!(b.to_string_lossy().ends_with("20261031120000-1"));
    assert_eq!(fs::read_to_string(&a).unwrap(), "{}");
    assert_eq!(fs::read_to_string(&b).unwrap(), "{\"x\":1}");
}

#[test]
fn keeps_only_three_most_recent_backups() {
    let dir = tempfile::tempdir().unwrap();
    let p = file(dir.path());
    for s in [
        "20261031120001",
        "20261031120002",
        "20261031120003",
        "20261031120004",
    ] {
        fsio::backup(&p, s).unwrap();
    }
    let left = backups(dir.path());
    assert_eq!(
        left,
        vec![
            "settings.json.bak-aiannotations-20261031120002",
            "settings.json.bak-aiannotations-20261031120003",
            "settings.json.bak-aiannotations-20261031120004",
        ]
    );
}

#[test]
fn suffixed_backups_count_in_suffix_order() {
    let dir = tempfile::tempdir().unwrap();
    let p = file(dir.path());
    // pre-existing backups: numeric order puts -10 above -2 and -9
    for suffix in ["", "-2", "-9", "-10"] {
        let name = format!("settings.json.bak-aiannotations-20261031120000{suffix}");
        fs::write(dir.path().join(name), "old").unwrap();
    }
    // unrelated files must survive
    fs::write(dir.path().join("settings.json.bak-aiannotations-x"), "k").unwrap();
    fs::write(
        dir.path()
            .join("other.json.bak-aiannotations-20261031120000"),
        "k",
    )
    .unwrap();
    fsio::backup(&p, "20261031120000").unwrap();
    let left: Vec<String> = backups(dir.path())
        .into_iter()
        .filter(|n| n.starts_with("settings.json.bak-aiannotations-2"))
        .collect();
    assert_eq!(
        left,
        vec![
            "settings.json.bak-aiannotations-20261031120000-10",
            "settings.json.bak-aiannotations-20261031120000-11",
            "settings.json.bak-aiannotations-20261031120000-9",
        ]
    );
    assert!(
        dir.path()
            .join("settings.json.bak-aiannotations-x")
            .exists()
    );
    assert!(
        dir.path()
            .join("other.json.bak-aiannotations-20261031120000")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn write_follows_symlink_and_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.json");
    fs::write(&real, "old").unwrap();
    let mid = dir.path().join("mid.json");
    let link = dir.path().join("link.json");
    std::os::unix::fs::symlink(&real, &mid).unwrap();
    std::os::unix::fs::symlink("mid.json", &link).unwrap();
    assert_eq!(fsio::target(&link), real);
    fsio::write_atomic(&link, "new").unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&real).unwrap(), "new");
    let b = fsio::backup(&link, "20261031120000").unwrap();
    assert_eq!(
        b,
        dir.path()
            .join("real.json.bak-aiannotations-20261031120000")
    );
}

#[test]
fn target_without_symlink_is_the_path() {
    let p = PathBuf::from("/nonexistent/a.json");
    assert_eq!(fsio::target(&p), p);
}

#[cfg(unix)]
#[test]
fn write_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let p = file(dir.path());
    fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
    fsio::write_atomic(&p, "new").unwrap();
    assert_eq!(
        fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_to_string(&p).unwrap(), "new");
    let leftovers = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 1);
}

#[test]
fn write_creates_missing_parent() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a/b/c.json");
    fsio::write_atomic(&p, "hi").unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), "hi");
}
