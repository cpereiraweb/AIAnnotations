//! `dev ai-check` with a fake `claude` script (never the real binary).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn fake_claude(tmp: &TempDir, script: &str) -> std::path::PathBuf {
    let bin = tmp.path().join("fake-claude");
    std::fs::write(&bin, format!("#!/bin/sh\ncat >/dev/null\n{script}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn cli(tmp: &TempDir, claude: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("AIANNOTATIONS_CLAUDE_BIN", claude)
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

#[test]
fn ai_check_prints_the_binary_and_the_reply() {
    let tmp = TempDir::new().unwrap();
    let bin = fake_claude(
        &tmp,
        r#"echo '{"result":"{\"ok\":true}","is_error":false}'"#,
    );
    cli(&tmp, &bin)
        .args(["dev", "ai-check"])
        .assert()
        .success()
        .stdout(contains(format!("binary: {}", bin.display())))
        .stdout(contains(r#"ok: {"ok":true}"#));
}

#[test]
fn ai_check_shows_the_ai_error_and_fails() {
    let tmp = TempDir::new().unwrap();
    let bin = fake_claude(&tmp, "echo 'boom: not logged in' >&2; exit 1");
    cli(&tmp, &bin)
        .args(["dev", "ai-check"])
        .assert()
        .code(1)
        .stderr(contains("boom: not logged in"));
}
