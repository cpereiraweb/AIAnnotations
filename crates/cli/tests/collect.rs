//! `dev collect` / `dev backfill` output contract (Inv §3.5).
use assert_cmd::Command;
use tempfile::TempDir;

/// A data dir, a Claude dir holding one transcript last modified in 2020 and a Codex home whose
/// `threads` lacks `updated_at_ms` (one isolated error).
fn setup() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("claude/projects/-tmp-x");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("s1.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"user\",\"timestamp\":\"2020-01-01T10:00:00Z\",\"cwd\":\"/tmp/x\",\"message\":{\"role\":\"user\",\"content\":\"Oi\"}}\n",
    )
    .unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_577_872_800);
    std::fs::File::options()
        .write(true)
        .open(&transcript)
        .unwrap()
        .set_modified(old)
        .unwrap();
    // Any SQLite file with an old `threads` schema; the core's `db::open` is a handy writer.
    let codex = aiannotations_core::db::open(&tmp.path().join("codex/state_5.sqlite")).unwrap();
    codex
        .execute_batch(
            "CREATE TABLE threads (id TEXT, rollout_path TEXT, cwd TEXT, updated_at INTEGER);",
        )
        .unwrap();
    tmp
}

fn cli(tmp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

/// `seen=N closed_idle=N errors=N warnings=N`, then one `error: …` line per error and one
/// `warning: …` line per warning.
#[test]
fn dev_backfill_prints_counts_then_errors() {
    let tmp = setup();
    let out = cli(&tmp).args(["dev", "backfill"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[0], "seen=1 closed_idle=0 errors=1 warnings=0",
        "{stdout}"
    );
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(
        lines[1].starts_with("error: Codex discovery failed: "),
        "{stdout}"
    );
}

/// The 2020 transcript is outside the 2-day window of `collect`.
#[test]
fn dev_collect_uses_the_window() {
    let tmp = setup();
    let out = cli(&tmp).args(["dev", "collect"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.starts_with("seen=0 closed_idle=0 errors=1 warnings=0\n"),
        "{stdout}"
    );
}

/// Recorder warnings (here an unknown Claude entrypoint) are counted apart and printed after the
/// errors as `warning: …` lines.
#[test]
fn dev_backfill_prints_warnings_after_errors() {
    let tmp = setup();
    std::fs::write(
        tmp.path().join("claude/projects/-tmp-x/s1.jsonl"),
        "{\"type\":\"user\",\"timestamp\":\"2020-01-01T10:00:00Z\",\"cwd\":\"/tmp/x\",\"entrypoint\":\"claude-novo-app\",\"message\":{\"role\":\"user\",\"content\":\"Oi\"}}\n",
    )
    .unwrap();
    let out = cli(&tmp).args(["dev", "backfill"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[0], "seen=1 closed_idle=0 errors=1 warnings=1",
        "{stdout}"
    );
    assert_eq!(lines.len(), 3, "{stdout}");
    assert!(
        lines[1].starts_with("error: Codex discovery failed: "),
        "{stdout}"
    );
    assert!(
        lines[2].starts_with("warning: unknown claude session origin \"claude-novo-app\""),
        "{stdout}"
    );
}
