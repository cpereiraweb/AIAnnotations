use aiannotations_core::installer::hooks::HookEvent;
use aiannotations_core::installer::state::{InstallState, load, save};
use std::path::Path;

#[test]
fn load_missing_is_empty_v1() {
    let dir = tempfile::tempdir().unwrap();
    let s = load(&dir.path().join("install-state.json")).unwrap();
    assert_eq!(s.version, 1);
    assert!(s.files.is_empty());
}

#[test]
fn state_roundtrip_v1_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/install-state.json");
    let mut s = load(&path).unwrap();
    s.record_created(
        Path::new("/h/.claude/settings.json"),
        &[HookEvent::Start, HookEvent::End],
    );
    save(&path, &s).unwrap();

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        raw,
        serde_json::json!({
            "version": 1,
            "files": {"/h/.claude/settings.json": {"created_events": ["SessionStart", "SessionEnd"]}}
        })
    );
    assert!(std::fs::read_to_string(&path).unwrap().ends_with('\n'));

    let back = load(&path).unwrap();
    assert_eq!(back, s);
    assert_eq!(
        back.created(Path::new("/h/.claude/settings.json")),
        Some(&[HookEvent::Start, HookEvent::End][..])
    );
    assert_eq!(back.created(Path::new("/other")), None);
}

#[test]
fn loads_a_v1_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("install-state.json");
    std::fs::write(
        &path,
        r#"{"version":1,"files":{"/x/hooks.json":{"created_events":["SessionEnd","Bogus"]}}}"#,
    )
    .unwrap();
    let s = load(&path).unwrap();
    assert_eq!(
        s.created(Path::new("/x/hooks.json")),
        Some(&[HookEvent::End][..])
    );
}

#[test]
fn record_created_unions_without_duplicates() {
    let mut s = InstallState::default();
    let f = Path::new("/f");
    s.record_created(f, &[HookEvent::Start]);
    s.record_created(f, &[HookEvent::End, HookEvent::Start]);
    assert_eq!(s.created(f), Some(&[HookEvent::Start, HookEvent::End][..]));
}

#[test]
fn record_created_empty_does_not_invent_state() {
    let mut s = InstallState::default();
    s.record_created(Path::new("/f"), &[]);
    assert!(s.files.is_empty());
    assert_eq!(s.created(Path::new("/f")), None);
}

#[test]
fn default_is_version_1() {
    assert_eq!(InstallState::default().version, 1);
}

#[test]
fn forget_drops_the_file_entry() {
    let mut s = InstallState::default();
    s.record_created(Path::new("/f"), &[HookEvent::Start]);
    s.record_created(Path::new("/g"), &[HookEvent::End]);
    s.forget(Path::new("/f"));
    assert_eq!(s.created(Path::new("/f")), None);
    assert_eq!(s.created(Path::new("/g")), Some(&[HookEvent::End][..]));
    s.forget(Path::new("/nope"));
}
