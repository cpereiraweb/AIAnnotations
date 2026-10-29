//! Parity with Laravel `tests/Feature/Sessions/IngestTest.php` cases 25 and 26 (Inv §3.3).
use aiannotations_core::sessions::ingest;
use aiannotations_core::{Paths, Settings, db};
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;

/// Hermetic: the Claude and Codex dirs are temp siblings of `data_dir`, never the user's.
fn cli(data_dir: &Path) -> Command {
    let base = data_dir.parent().unwrap();
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", data_dir)
        .env("CLAUDE_CONFIG_DIR", base.join("claude"))
        .env("CODEX_HOME", base.join("codex"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

fn spool_names(data_dir: &Path, sub: &str) -> usize {
    std::fs::read_dir(data_dir.join("spool").join(sub))
        .map(|rd| rd.count())
        .unwrap_or(0)
}

/// o comando aiannotations:ingest processa a fila e imprime a contagem
#[test]
fn dev_ingest_processes_queue_and_prints_counts() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let payload =
        json!({"session_id":"cmd","transcript_path":null,"cwd":tmp.path(),"source":"startup"});
    cli(&data)
        .args(["hook", "claude", "start"])
        .write_stdin(payload.to_string())
        .assert()
        .success();
    // A second, invalid file shows up in the counts.
    std::fs::write(data.join("spool/0-claude-end-1.json"), "{").unwrap();

    cli(&data)
        .args(["dev", "ingest"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "processed=1 failed=1 deferred=0\n",
        ))
        .stdout(predicates::str::contains("0-claude-end-1.json"));

    assert_eq!(spool_names(&data, "done"), 1);
    assert_eq!(spool_names(&data, "failed"), 1);
    let conn = db::open(&data.join("aiannotations.sqlite")).unwrap();
    let sid: String = conn
        .query_row("SELECT session_id FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sid, "cmd");
}

/// `dev` stays out of the help.
#[test]
fn dev_subcommand_is_hidden_from_help() {
    Command::cargo_bin("aiannotations")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"(?m)^\s+dev\b").unwrap().not());
}

/// o envelope gravado pelo hook real é ingerido
#[test]
fn envelope_written_by_real_hook_is_ingested() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let payload = json!({"session_id":"hk","transcript_path":null,"cwd":tmp.path(),"hook_event_name":"SessionStart","source":"startup"});
    cli(&data)
        .args(["hook", "claude", "start"])
        .write_stdin(payload.to_string())
        .assert()
        .code(0)
        .stdout("")
        .stderr("");

    let paths = Paths::from_env(
        |k| (k == "AIANNOTATIONS_DATA_DIR").then(|| data.clone().into()),
        tmp.path(),
        tmp.path(),
    );
    let mut conn = db::open(&paths.db_path).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let report = ingest::run(
        &mut conn,
        &paths,
        &Settings::default(),
        &Default::default(),
        now,
    )
    .unwrap();

    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (1, 0, 0)
    );
    let (sid, started): (String, i64) = conn
        .query_row(
            "SELECT s.session_id, r.started_at FROM ai_sessions s JOIN session_runs r ON r.ai_session_id = s.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sid, "hk");
    assert!((now - started).abs() < 60_000, "{started} vs {now}");
}
