//! `ClaudeCli` against fake `#!/bin/sh` scripts (never the real binary).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aiannotations_core::ai::claude::ClaudeCli;
use aiannotations_core::ai::{AiClient, AiError, ModelRole};
use serde_json::json;

/// Writes an executable script into `dir`.
fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-claude");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn client(bin: PathBuf, workdir: PathBuf) -> ClaudeCli {
    ClaudeCli {
        bin,
        workdir,
        timeout: Duration::from_secs(20),
        summary_model: "haiku".into(),
        briefing_model: "sonnet".into(),
    }
}

const OK_JSON: &str = r#"{"result":"{\"summary\":\"ok\"}","is_error":false}"#;

#[test]
fn passes_isolation_args_env_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let body = format!(
        r#"for a in "$@"; do printf '[%s]' "$a"; done > "{o}/argv"
printf '%s' "$AIANNOTATIONS_INTERNAL" > "{o}/env"
pwd > "{o}/pwd"
cat > "{o}/stdin"
printf '%s' '{OK_JSON}'"#,
        o = out.display()
    );
    let bin = script(dir.path(), &body);
    let workdir = dir.path().join("ai-work").join("nested");
    let c = client(bin, workdir.clone());

    let got = c.complete_json("o prompt", ModelRole::Summary).unwrap();
    assert_eq!(serde_json::Value::Object(got), json!({"summary": "ok"}));

    let argv = std::fs::read_to_string(out.join("argv")).unwrap();
    let expected = [
        "-p",
        "--model",
        "haiku",
        "--output-format",
        "json",
        "--no-session-persistence",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--disable-slash-commands",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        r#"{"mcpServers":{}}"#,
        "--setting-sources",
        "",
    ]
    .iter()
    .map(|a| format!("[{a}]"))
    .collect::<String>();
    assert_eq!(argv, expected);
    assert_eq!(std::fs::read_to_string(out.join("env")).unwrap(), "1");
    let pwd = std::fs::read_to_string(out.join("pwd")).unwrap();
    assert_eq!(
        Path::new(pwd.trim()).canonicalize().unwrap(),
        workdir.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(out.join("stdin")).unwrap(),
        "o prompt"
    );
}

#[test]
fn briefing_role_uses_the_briefing_model() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let body = format!(
        r#"printf '%s ' "$@" > "{}"
printf '%s' '{OK_JSON}'"#,
        argv.display()
    );
    let c = client(script(dir.path(), &body), dir.path().join("w"));
    c.complete_json("p", ModelRole::Briefing).unwrap();
    assert!(
        std::fs::read_to_string(&argv)
            .unwrap()
            .contains("--model sonnet ")
    );
}

#[test]
fn is_error_true_with_exit_0_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"printf '%s' '{"result":"something broke","is_error":true}'"#,
    );
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(ref m) if m.contains("something broke")));
}

#[test]
fn rate_limit_in_result_is_rate_limited() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"printf '%s' '{"result":"Claude AI usage limit reached","is_error":true}'
exit 1"#,
    );
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::RateLimited(_)));
}

#[test]
fn timeout_kills_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    // No `exec`: `sleep` is a grandchild holding the pipes, so the whole group must die.
    let bin = script(dir.path(), "sleep 30");
    let mut c = client(bin, dir.path().join("w"));
    c.timeout = Duration::from_secs(1);
    let started = Instant::now();
    let err = c.complete_json("p", ModelRole::Summary).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(matches!(err, AiError::Failed(ref m) if m.contains("timed out")));
}

#[test]
fn large_output_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!(
        r#"head -c 2097152 /dev/zero | tr '\0' 'x' >&2
printf '%s' '{OK_JSON}'"#
    );
    let bin = script(dir.path(), &body);
    let got = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap();
    assert_eq!(got["summary"], "ok");
}

#[test]
fn large_stdin_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!(
        r#"printf '%s' '{OK_JSON}'
exec cat > /dev/null"#
    );
    let bin = script(dir.path(), &body);
    let prompt = "y".repeat(2 * 1024 * 1024);
    let got = client(bin, dir.path().join("w"))
        .complete_json(&prompt, ModelRole::Summary)
        .unwrap();
    assert_eq!(got["summary"], "ok");
}

#[test]
fn missing_binary_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path().join("nope/claude"), dir.path().join("w"));
    let err = c.complete_json("p", ModelRole::Summary).unwrap_err();
    assert!(matches!(err, AiError::Unavailable(_)));
}

#[test]
fn exit_127_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "exit 127");
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Unavailable(_)));
}

#[test]
fn result_without_json_object_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"printf '%s' '{"result":"just prose","is_error":false}'"#,
    );
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)));
}

#[test]
fn stdout_without_result_field_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), r#"printf '%s' '{"other":1}'"#);
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)));
}

#[test]
fn rate_limit_is_found_even_after_long_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"head -c 1000 /dev/zero | tr '\0' 'x' >&2
printf '%s' '{"result":"Claude AI usage limit reached","is_error":true}'
exit 1"#,
    );
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::RateLimited(_)));
}

#[test]
fn failed_message_keeps_the_end_of_a_long_detail() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"head -c 2000 /dev/zero | tr '\0' 'x' >&2
printf '%s' '{"result":"final cause","is_error":true}'
exit 1"#,
    );
    let err = client(bin, dir.path().join("w"))
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    let AiError::Failed(m) = err else {
        panic!("{err:?}")
    };
    assert!(m.ends_with("final cause"), "{m}");
    assert!(m.chars().count() < 600, "{}", m.chars().count());
}
