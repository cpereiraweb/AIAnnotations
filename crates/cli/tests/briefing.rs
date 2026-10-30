//! `dev briefing` end to end, with a fake `claude` script (never the real binary).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const REPLY: &str = r#"{"result":"{\"paragraph\":\"Dia produtivo\",\"next_steps\":[{\"project\":\"proj\",\"step\":\"Seguir\"}]}","is_error":false}"#;

fn cli(tmp: &TempDir, claude: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env("AIANNOTATIONS_CLAUDE_BIN", claude)
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
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

/// One visible run started a minute ago, so it falls in the default 24 h window.
fn seed(tmp: &TempDir) {
    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    let started = chrono::Utc::now().timestamp_millis() - 60_000;
    conn.execute_batch(&format!(
        "INSERT INTO projects (key, name, path) VALUES ('k', 'Proj', '/p/Proj');
         INSERT INTO ai_sessions (project_id, tool, session_id, cwd, last_activity_at)
             VALUES (1, 'claude', 's1', '/p/Proj', 1);
         INSERT INTO session_runs (ai_session_id, started_at, summary) VALUES (1, {started}, 'Fez X');"
    ))
    .unwrap();
}

#[test]
fn briefing_generates_and_stores_the_day() {
    let tmp = TempDir::new().unwrap();
    seed(&tmp);
    let bin = fake_claude(&tmp);
    let out = cli(&tmp, &bin)
        .args([
            "dev",
            "briefing",
            "--lang",
            "pt-BR",
            "--tz",
            "America/Sao_Paulo",
        ])
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["outcome"], "Generated");
    assert_eq!(v["briefing"]["status"], "done");
    assert_eq!(v["briefing"]["body"], "Dia produtivo");
    assert_eq!(v["briefing"]["next_steps"][0]["projeto"], "Proj");

    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    let (status, body, steps): (String, String, String) = conn
        .query_row("SELECT status, body, next_steps FROM briefings", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((status.as_str(), body.as_str()), ("done", "Dia produtivo"));
    assert!(steps.contains("\"passo\":\"Seguir\""), "{steps}");

    // A second run without --force is a no-op.
    let out = cli(&tmp, &bin)
        .args(["dev", "briefing", "--tz", "America/Sao_Paulo"])
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["outcome"], "AlreadyDone");
}

#[test]
fn invalid_tz_is_a_cli_error() {
    let tmp = TempDir::new().unwrap();
    let bin = fake_claude(&tmp);
    cli(&tmp, &bin)
        .args(["dev", "briefing", "--tz", "Mars/Olympus"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown time zone"));
}
