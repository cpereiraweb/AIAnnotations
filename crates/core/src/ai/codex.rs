//! `codex exec` client, isolated like `ClaudeCli` (spec §9.1, port of Laravel `CodexCliClient`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Map, Value};

use super::{AiClient, AiError, ModelRole, binary, classify, json, process};

/// Longest error detail kept in messages.
const DETAIL_CHARS: usize = 500;

/// Distinguishes the last-message files of concurrent calls in one process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Calls the user's `codex` CLI with their login. It runs read-only and ephemeral in
/// `workdir` (outside any repo) with `AIANNOTATIONS_INTERNAL=1`, ignoring user config and
/// rules. The prompt goes through stdin (the final `-`); the answer is read from the
/// `--output-last-message` file, which is deleted afterwards. A `None` model leaves the
/// choice to codex.
#[derive(Debug, Clone)]
pub struct CodexCli {
    pub bin: PathBuf,
    pub workdir: PathBuf,
    pub timeout: Duration,
    pub summary_model: Option<String>,
    pub briefing_model: Option<String>,
}

impl CodexCli {
    fn args(&self, model: Option<&str>, out_file: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = [
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
        .map(OsString::from)
        .collect();
        args.push(self.workdir.clone().into_os_string());
        if let Some(m) = model {
            args.push("-m".into());
            args.push(m.into());
        }
        args.push("-c".into());
        args.push(r#"model_reasoning_effort="low""#.into());
        args.push("--output-last-message".into());
        args.push(out_file.as_os_str().to_owned());
        args.push("-".into());
        args
    }

    fn run(&self, prompt: &str, model: Option<&str>, out_file: &Path) -> Result<String, AiError> {
        let env = [
            (
                OsString::from("AIANNOTATIONS_INTERNAL"),
                OsString::from("1"),
            ),
            (
                OsString::from("PATH"),
                binary::path_env(&self.bin, std::env::var_os("PATH").as_deref()),
            ),
        ];
        let output = process::run(
            &self.bin,
            &self.args(model, out_file),
            &self.workdir,
            &env,
            prompt,
            self.timeout,
        )
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                AiError::Unavailable(format!("codex not found at {}: {e}", self.bin.display()))
            }
            _ => AiError::Failed(format!("cannot run codex exec: {e}")),
        })?;

        if output.timed_out {
            return Err(AiError::Failed(format!(
                "codex exec timed out after {}s",
                self.timeout.as_secs()
            )));
        }

        if output.status != Some(0) {
            let detail = format!("{} {}", output.stderr.trim(), output.stdout.trim());
            let detail = detail.trim().to_string();
            let exit = output
                .status
                .map_or_else(|| "?".to_string(), |c| c.to_string());
            if classify::is_rate_limited(&detail) {
                return Err(AiError::RateLimited(truncate(&detail)));
            }
            if classify::codex_unavailable(&detail, &self.bin, output.status) {
                return Err(AiError::Unavailable(format!(
                    "exit {exit}: {}",
                    truncate(&detail)
                )));
            }
            return Err(AiError::Failed(format!(
                "codex exec failed (exit {exit}): {}",
                truncate(&detail)
            )));
        }

        std::fs::read_to_string(out_file)
            .ok()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| AiError::Failed("codex exec wrote no last message".into()))
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(DETAIL_CHARS).collect()
}

impl AiClient for CodexCli {
    fn complete_json(&self, prompt: &str, role: ModelRole) -> Result<Map<String, Value>, AiError> {
        let model = match role {
            ModelRole::Summary => &self.summary_model,
            ModelRole::Briefing => &self.briefing_model,
        }
        .as_deref()
        .filter(|m| !m.trim().is_empty());
        std::fs::create_dir_all(&self.workdir)
            .map_err(|e| AiError::Failed(format!("cannot create the AI workdir: {e}")))?;
        let out_file = self.workdir.join(format!(
            "last-message-{}-{}.txt",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        let result = self.run(prompt, model, &out_file);
        // A missing file is fine (codex may not have written it).
        let _ = std::fs::remove_file(&out_file);

        json::first_object(&result?)
            .ok_or_else(|| AiError::Failed("codex exec result has no JSON object".into()))
    }
}
