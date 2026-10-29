//! `dev prune` / `dev reclassify` / `dev stats` output contract.
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli(tmp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

#[test]
fn prune_removes_old_done_files_and_reports_counts() {
    let tmp = TempDir::new().unwrap();
    let done = tmp.path().join("data/spool/done");
    std::fs::create_dir_all(&done).unwrap();
    let old = done.join("old.json");
    let fresh = done.join("fresh.json");
    std::fs::write(&old, "{}").unwrap();
    std::fs::write(&fresh, "{}").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();

    cli(&tmp)
        .args(["dev", "prune"])
        .assert()
        .success()
        .stdout("done_removed=1 failed_removed=0\n");
    assert!(!old.exists() && fresh.exists());
}

#[test]
fn reclassify_on_an_empty_database_reports_zero() {
    let tmp = TempDir::new().unwrap();
    cli(&tmp)
        .args(["dev", "reclassify"])
        .assert()
        .success()
        .stdout("reclassified=0\n");
}

#[test]
fn stats_prints_json() {
    let tmp = TempDir::new().unwrap();
    let out = cli(&tmp).args(["dev", "stats"]).assert().success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["projects"], 0);
    assert_eq!(v["runs"], 0);
    assert_eq!(v["visible_sessions"], 0);
    assert!(v["sessions_by_tool"].is_object());
    cli(&tmp)
        .args(["dev", "stats"])
        .assert()
        .stdout(predicate::str::contains("runs_by_summary_status"));
}
