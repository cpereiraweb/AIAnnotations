//! `dev summarize` end to end, with a fake `claude` script (never the real binary).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const REPLY: &str = r#"{"result":"{\"summary\":\"Fez X\",\"next_step\":null,\"unfinished\":false,\"kind\":\"code\"}","is_error":false}"#;

fn cli(tmp: &TempDir, claude: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env("AIANNOTATIONS_CLAUDE_BIN", claude)
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

/// One closed pending run (id 1) over a two-line Claude transcript.
fn seed(tmp: &TempDir) -> std::path::PathBuf {
    let transcript = tmp.path().join("t.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","sessionId":"s1","message":{"role":"user","content":"Oi"},"timestamp":"2026-10-20T12:00:00.000Z"}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"s1","message":{"role":"assistant","content":[{"type":"text","text":"Olá"}]},"timestamp":"2026-10-20T12:00:05.000Z"}"#,
            "\n"
        ),
    )
    .unwrap();
    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO projects (key, name, path) VALUES ('k', 'Proj', '/p/Proj');
         INSERT INTO ai_sessions (project_id, tool, session_id, cwd, transcript_path, last_activity_at)
             VALUES (1, 'claude', 's1', '/p/Proj', '{}', 1);
         INSERT INTO session_runs (ai_session_id, started_at, ended_at) VALUES (1, 1000, 9000);",
        transcript.display()
    ))
    .unwrap();
    transcript
}

fn fake_claude(tmp: &TempDir) -> std::path::PathBuf {
    let bin = tmp.path().join("fake-claude");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'EOF'\n{REPLY}\nEOF\n"),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[test]
fn summarize_turns_a_pending_run_into_done() {
    let tmp = TempDir::new().unwrap();
    seed(&tmp);
    let bin = fake_claude(&tmp);
    let out = cli(&tmp, &bin)
        .args(["dev", "summarize", "--limit", "3", "--lang", "en"])
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(report["summarized"], 1);
    assert_eq!(report["was_paused"], false);

    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    let (status, summary, kind): (String, String, String) = conn
        .query_row(
            "SELECT summary_status, summary, kind FROM session_runs WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (status.as_str(), summary.as_str(), kind.as_str()),
        ("done", "Fez X", "codigo")
    );
}

#[test]
fn summarize_retry_failed_requeues_first() {
    let tmp = TempDir::new().unwrap();
    seed(&tmp);
    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    conn.execute(
        "UPDATE session_runs SET summary_status = 'failed', summary_attempts = 5",
        [],
    )
    .unwrap();
    drop(conn);
    let bin = fake_claude(&tmp);
    let out = cli(&tmp, &bin)
        .args(["dev", "summarize", "--retry-failed", "--provider", "claude"])
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(report["retried"], 1);
    assert_eq!(report["summarized"], 1);
}
