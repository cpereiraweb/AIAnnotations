use aiannotations_services::launch::workspaces::{strip_jsonc, workspace_for};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn ws_json(paths: &[&Path]) -> String {
    let folders: Vec<_> = paths
        .iter()
        .map(|p| serde_json::json!({ "path": p.to_string_lossy() }))
        .collect();
    serde_json::json!({ "folders": folders }).to_string()
}

fn proj(root: &TempDir, name: &str) -> PathBuf {
    let d = root.path().join("projects").join(name);
    fs::create_dir_all(&d).unwrap();
    fs::canonicalize(d).unwrap()
}

fn wsdir(root: &TempDir) -> PathBuf {
    let d = root.path().join("ws");
    fs::create_dir_all(&d).unwrap();
    fs::canonicalize(d).unwrap()
}

fn put(dir: &Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

#[test]
fn single_folder_wins() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    let beta = proj(&root, "beta");
    put(&dir, "a-multi.code-workspace", &ws_json(&[&p, &beta]));
    put(&dir, "z-single.code-workspace", &ws_json(&[&p]));
    assert_eq!(
        workspace_for(&dir, &p),
        Some(dir.join("z-single.code-workspace"))
    );
}

#[test]
fn basename_wins_among_multi() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    put(&dir, "a.code-workspace", &ws_json(&[&p]));
    put(&dir, "alpha.code-workspace", &ws_json(&[&p]));
    put(&dir, "b.code-workspace", &ws_json(&[&p]));
    assert_eq!(
        workspace_for(&dir, &p),
        Some(dir.join("alpha.code-workspace"))
    );
}

#[test]
fn alphabetical_fallback() {
    let root = TempDir::new().unwrap();
    let (dir, q) = (wsdir(&root), proj(&root, "gamma"));
    put(&dir, "b.code-workspace", &ws_json(&[&q]));
    put(&dir, "a.code-workspace", &ws_json(&[&q]));
    assert_eq!(workspace_for(&dir, &q), Some(dir.join("a.code-workspace")));
}

#[test]
fn multi_folder_matches_any_folder() {
    let root = TempDir::new().unwrap();
    let dir = wsdir(&root);
    let (a, b) = (proj(&root, "hermes"), proj(&root, "hermes-alfred"));
    put(&dir, "Hermes.code-workspace", &ws_json(&[&a, &b]));
    assert_eq!(
        workspace_for(&dir, &a),
        Some(dir.join("Hermes.code-workspace"))
    );
    assert_eq!(
        workspace_for(&dir, &b),
        Some(dir.join("Hermes.code-workspace"))
    );
}

#[test]
fn no_match_is_none() {
    let root = TempDir::new().unwrap();
    let dir = wsdir(&root);
    put(&dir, "x.code-workspace", &ws_json(&[&proj(&root, "outro")]));
    assert_eq!(workspace_for(&dir, &proj(&root, "alpha")), None);
}

#[test]
fn relative_folder_resolved_from_file_dir() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    // ws/../projects/alpha, relative to the workspace file's directory
    put(
        &dir,
        "rel.code-workspace",
        r#"{"folders":[{"path":"../projects/alpha"}]}"#,
    );
    assert_eq!(
        workspace_for(&dir, &p),
        Some(dir.join("rel.code-workspace"))
    );
}

#[test]
fn jsonc_comments_and_trailing_commas() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    let path = serde_json::Value::String(p.to_string_lossy().into_owned());
    let text = format!(
        "{{\n // comment\n \"folders\": [\n /* block */\n {{ \"path\": {path}, }}, // end\n ],\n \"settings\": {{ \"url\": \"http://x//y\", }},\n}}"
    );
    put(&dir, "j.code-workspace", &text);
    assert_eq!(workspace_for(&dir, &p), Some(dir.join("j.code-workspace")));
}

#[test]
fn strip_jsonc_keeps_strings() {
    let out = strip_jsonc(r#"{"a": "x//y /* z */", /* c */ "b": [1,2,],} // t"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["a"], "x//y /* z */");
    assert_eq!(v["b"], serde_json::json!([1, 2]));
}

#[test]
fn invalid_file_ignored() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    put(&dir, "bad.code-workspace", "{ not json");
    put(&dir, "nofolders.code-workspace", r#"{"settings": {}}"#);
    put(&dir, "ok.code-workspace", &ws_json(&[&p]));
    assert_eq!(workspace_for(&dir, &p), Some(dir.join("ok.code-workspace")));
}

#[test]
fn missing_dir_is_none() {
    let root = TempDir::new().unwrap();
    let p = proj(&root, "alpha");
    assert_eq!(workspace_for(&root.path().join("nope"), &p), None);
}

#[cfg(unix)]
#[test]
fn symlink_outside_dir_ignored() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    let outside = root.path().join("outside.code-workspace");
    fs::write(&outside, ws_json(&[&p])).unwrap();
    let beta = proj(&root, "beta");
    put(&dir, "z-multi.code-workspace", &ws_json(&[&p, &beta]));
    std::os::unix::fs::symlink(&outside, dir.join("alpha.code-workspace")).unwrap();
    assert_eq!(
        workspace_for(&dir, &p),
        Some(dir.join("z-multi.code-workspace"))
    );
}

#[cfg(unix)]
#[test]
fn matches_by_realpath() {
    let root = TempDir::new().unwrap();
    let (dir, p) = (wsdir(&root), proj(&root, "alpha"));
    let link = root.path().join("projects").join("link");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    put(&dir, "x.code-workspace", &ws_json(&[&link]));
    // project passed through the symlink matches too
    assert_eq!(
        workspace_for(&dir, &link),
        Some(dir.join("x.code-workspace"))
    );
    assert_eq!(workspace_for(&dir, &p), Some(dir.join("x.code-workspace")));
}
