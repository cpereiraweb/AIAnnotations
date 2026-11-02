//! The "open folder" argv and validation.

use std::ffi::OsString;
use std::path::Path;

use aiannotations_services::actions::{open_folder, open_folder_argv};

#[test]
fn open_folder_rejects_missing_or_relative() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    assert!(open_folder_argv(&missing).is_err());
    assert!(open_folder(&missing).is_err());
    assert!(open_folder_argv(Path::new("")).is_err());
    assert!(open_folder(Path::new("relative/dir")).is_err());
    // A file is not a folder.
    let file = dir.path().join("f.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(open_folder_argv(&file).is_err());
    assert!(open_folder(&file).is_err());
}

#[test]
fn open_folder_argv_per_os() {
    let dir = tempfile::tempdir().unwrap();
    let argv = open_folder_argv(dir.path()).unwrap();
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    // A temp dir path has no forward slash or trailing separator, so it passes through as is.
    assert_eq!(
        argv,
        vec![OsString::from(opener), dir.path().as_os_str().to_owned()]
    );
}

#[cfg(windows)]
#[test]
fn open_folder_argv_normalizes_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().to_string_lossy().into_owned();
    let expected = vec![OsString::from("explorer"), OsString::from(&native)];
    let slashed = format!("{}/", native.replace('\\', "/"));
    assert_eq!(open_folder_argv(Path::new(&slashed)).unwrap(), expected);
    let trailing = format!("{native}\\");
    assert_eq!(open_folder_argv(Path::new(&trailing)).unwrap(), expected);
    // Validation still comes first.
    assert!(open_folder_argv(&dir.path().join("gone/")).is_err());
}
