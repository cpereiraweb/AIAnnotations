//! `CodexCli` and `client_for` against fake `#!/bin/sh` scripts (never the real binary).
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aiannotations_core::ai::codex::CodexCli;
use aiannotations_core::ai::{AiClient, AiError, ModelRole, client_for};
use aiannotations_core::model::Tool;
use aiannotations_core::{Paths, Settings};
use serde_json::json;

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-codex");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn client(bin: PathBuf, workdir: PathBuf, model: Option<&str>) -> CodexCli {
    CodexCli {
        bin,
        workdir,
        timeout: Duration::from_secs(20),
        summary_model: model.map(String::from),
        briefing_model: Some("gpt-big".into()),
    }
}

/// Shell snippet that writes `$1` into the file named after `--output-last-message`.
const WRITE_LAST: &str = r#"out=""
prev=""
for a in "$@"; do
  if [ "$prev" = "--output-last-message" ]; then out="$a"; fi
  prev="$a"
done"#;

type Run = (String, String, String, PathBuf, PathBuf, tempfile::TempDir);

fn run_isolation(model: Option<&str>) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let body = format!(
        r#"{WRITE_LAST}
for a in "$@"; do printf '[%s]' "$a"; done > "{o}/argv"
printf '%s' "$AIANNOTATIONS_INTERNAL" > "{o}/env"
printf '%s' "$out" > "{o}/outfile"
cat > "{o}/stdin"
printf '%s' '{{"summary":"ok"}}' > "$out""#,
        o = out.display()
    );
    let bin = script(dir.path(), &body);
    let workdir = dir.path().join("ai-work").join("nested");
    let c = client(bin, workdir.clone(), model);
    let got = c.complete_json("o prompt", ModelRole::Summary).unwrap();
    assert_eq!(serde_json::Value::Object(got), json!({"summary": "ok"}));
    let argv = std::fs::read_to_string(out.join("argv")).unwrap();
    let env = std::fs::read_to_string(out.join("env")).unwrap();
    let stdin = std::fs::read_to_string(out.join("stdin")).unwrap();
    let outfile = PathBuf::from(std::fs::read_to_string(out.join("outfile")).unwrap());
    (argv, env, stdin, workdir, outfile, dir)
}

fn expected_argv(workdir: &Path, model: Option<&str>, outfile: &Path) -> String {
    let mut args: Vec<String> = [
        "exec",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--color",
        "never",
        "-C",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(workdir.display().to_string());
    if let Some(m) = model {
        args.push("-m".into());
        args.push(m.into());
    }
    args.push("-c".into());
    args.push(r#"model_reasoning_effort="low""#.into());
    args.push("--output-last-message".into());
    args.push(outfile.display().to_string());
    args.push("-".into());
    args.iter().map(|a| format!("[{a}]")).collect()
}

#[test]
fn passes_isolation_args() {
    let (argv, env, stdin, workdir, outfile, _dir) = run_isolation(None);
    assert_eq!(argv, expected_argv(&workdir, None, &outfile));
    assert_eq!(env, "1");
    assert_eq!(stdin, "o prompt");
    assert_eq!(outfile.parent().unwrap(), workdir);
    assert!(
        !outfile.exists(),
        "the last-message file is deleted after reading"
    );
}

#[test]
fn passes_model_before_config_override() {
    let (argv, _, _, workdir, outfile, _dir) = run_isolation(Some("gpt-x"));
    assert_eq!(argv, expected_argv(&workdir, Some("gpt-x"), &outfile));
}

#[test]
fn briefing_role_uses_the_briefing_model() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let body = format!(
        r#"{WRITE_LAST}
for a in "$@"; do printf '[%s]' "$a"; done > "{}"
printf '%s' '{{"a":1}}' > "$out""#,
        argv.display()
    );
    let bin = script(dir.path(), &body);
    let c = client(bin, dir.path().join("w"), None);
    c.complete_json("p", ModelRole::Briefing).unwrap();
    assert!(
        std::fs::read_to_string(&argv)
            .unwrap()
            .contains("[-m][gpt-big][-c]")
    );
}

#[test]
fn exit_0_with_empty_last_message_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{WRITE_LAST}\n: > \"$out\"");
    let bin = script(dir.path(), &body);
    let work = dir.path().join("w");
    let err = client(bin, work.clone(), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)), "{err:?}");
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
}

#[test]
fn exit_0_with_missing_last_message_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "cat > /dev/null");
    let err = client(bin, dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)), "{err:?}");
}

#[test]
fn last_message_without_json_is_failed() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{WRITE_LAST}\nprintf 'no json here' > \"$out\"");
    let bin = script(dir.path(), &body);
    let err = client(bin, dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)), "{err:?}");
}

#[test]
fn usage_limit_in_stderr_is_rate_limited() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "echo \"ERROR: You've hit your usage limit. Try again later.\" >&2; exit 1",
    );
    let err = client(bin, dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::RateLimited(_)), "{err:?}");
}

#[test]
fn not_logged_in_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "echo 'Not logged in. Run codex login' >&2; exit 1",
    );
    let err = client(bin, dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Unavailable(_)), "{err:?}");
}

#[test]
fn other_failure_is_failed_and_cleans_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{WRITE_LAST}\nprintf 'partial' > \"$out\"; echo boom >&2; exit 2");
    let bin = script(dir.path(), &body);
    let work = dir.path().join("w");
    let err = client(bin, work.clone(), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Failed(_)), "{err:?}");
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
}

#[test]
fn timeout_is_failed_and_cleans_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{WRITE_LAST}\nprintf 'x' > \"$out\"; sleep 30");
    let bin = script(dir.path(), &body);
    let work = dir.path().join("w");
    let mut c = client(bin, work.clone(), None);
    c.timeout = Duration::from_millis(300);
    let err = c.complete_json("p", ModelRole::Summary).unwrap_err();
    assert!(matches!(err, AiError::Failed(_)), "{err:?}");
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
}

#[test]
fn missing_binary_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let err = client(dir.path().join("nope"), dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    assert!(matches!(err, AiError::Unavailable(_)), "{err:?}");
}

#[test]
fn client_for_picks_provider_and_bin_override() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("called");
    let body = format!(
        r#"{WRITE_LAST}
echo codex > "{}"
printf '%s' '{{"who":"codex"}}' > "$out""#,
        marker.display()
    );
    let bin = script(dir.path(), &body);
    let settings = Settings {
        ai_provider: Tool::Codex,
        ..Settings::default()
    };
    let home = dir.path().join("home");
    let paths = Paths::from_env(|_| None, &home, dir.path());
    let bin_for_env = bin.clone();
    let env = move |k: &str| match k {
        "AIANNOTATIONS_CODEX_BIN" => Some(bin_for_env.clone().into_os_string()),
        "AIANNOTATIONS_CLAUDE_BIN" => Some(OsString::from("/nonexistent/claude")),
        _ => None,
    };
    let c = client_for(&settings, &paths, env, &home);
    let got = c.complete_json("p", ModelRole::Summary).unwrap();
    assert_eq!(got["who"], "codex");
    assert!(marker.exists());
}

#[test]
fn client_for_claude_uses_claude_override() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-claude");
    std::fs::write(
        &bin,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"result\":\"{\\\"who\\\":\\\"claude\\\"}\",\"is_error\":false}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = Settings::default();
    let home = dir.path().join("home");
    let paths = Paths::from_env(|_| None, &home, dir.path());
    let bin_for_env = bin.clone();
    let env = move |k: &str| {
        (k == "AIANNOTATIONS_CLAUDE_BIN").then(|| bin_for_env.clone().into_os_string())
    };
    let c = client_for(&settings, &paths, env, &home);
    let got = c.complete_json("p", ModelRole::Summary).unwrap();
    assert_eq!(got["who"], "claude");
}

/// `codex exec` 0.166 copies the prompt into stderr (banner, `user`, prompt, `ERROR:`).
fn echoing_codex(dir: &Path, error_line: &str) -> PathBuf {
    let body = format!(
        r#"echo 'OpenAI Codex v0.166.0' >&2
echo '--------' >&2
echo 'user' >&2
cat >&2
echo >&2
echo "{error_line}" >&2
exit 1"#
    );
    script(dir, &body)
}

const TRANSCRIPT_PROMPT: &str = "Resuma esta sessão:\n\
     dev: the API said rate limit exceeded again\n\
     dev: then authentication failed and 401 Unauthorized\n\
     dev: sh: 1: foo: not found\n";

#[test]
fn echoed_prompt_does_not_drive_the_classification() {
    let dir = tempfile::tempdir().unwrap();
    let bin = echoing_codex(dir.path(), "ERROR: stream disconnected before completion");
    let err = client(bin, dir.path().join("w"), None)
        .complete_json(TRANSCRIPT_PROMPT, ModelRole::Summary)
        .unwrap_err();
    assert!(
        matches!(err, AiError::Failed(ref m) if m.contains("stream disconnected")),
        "{err:?}"
    );
}

#[test]
fn real_usage_limit_after_echoed_prompt_is_rate_limited() {
    let dir = tempfile::tempdir().unwrap();
    let bin = echoing_codex(
        dir.path(),
        "ERROR: You've hit your usage limit. Try again later.",
    );
    let err = client(bin, dir.path().join("w"), None)
        .complete_json(TRANSCRIPT_PROMPT, ModelRole::Summary)
        .unwrap_err();
    assert!(
        matches!(err, AiError::RateLimited(ref m) if m.contains("usage limit")),
        "{err:?}"
    );
}

#[test]
fn failed_message_keeps_the_end_of_a_long_detail() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "head -c 2000 /dev/zero | tr '\\0' 'é' >&2; echo >&2; echo 'ERROR: final cause' >&2; exit 1",
    );
    let err = client(bin, dir.path().join("w"), None)
        .complete_json("p", ModelRole::Summary)
        .unwrap_err();
    let AiError::Failed(m) = err else {
        panic!("{err:?}")
    };
    assert!(m.contains("ERROR: final cause"), "{m}");
    assert!(m.chars().count() < 600, "{}", m.chars().count());
}
