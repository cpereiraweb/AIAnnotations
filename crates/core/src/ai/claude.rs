//! `claude -p` client, isolated per spec §9.1 (port of Laravel `ClaudeCliClient`, Inv §4.1).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};

use super::{AiClient, AiError, ModelRole, binary, classify, json, process};

/// Calls the user's `claude` CLI with their subscription. The call leaves no trace:
/// it runs in `workdir` (outside any repo, so no project CLAUDE.md is read) with
/// `AIANNOTATIONS_INTERNAL=1`, no session persistence, no hooks, no tools, no MCP
/// servers and no user settings. The prompt goes through stdin only.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub bin: PathBuf,
    pub workdir: PathBuf,
    pub timeout: Duration,
    pub summary_model: String,
    pub briefing_model: String,
}

impl ClaudeCli {
    fn args(&self, model: &str) -> Vec<OsString> {
        [
            "-p",
            "--model",
            model,
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
        .map(OsString::from)
        .collect()
    }
}

impl AiClient for ClaudeCli {
    fn complete_json(&self, prompt: &str, role: ModelRole) -> Result<Map<String, Value>, AiError> {
        let model = match role {
            ModelRole::Summary => &self.summary_model,
            ModelRole::Briefing => &self.briefing_model,
        };
        std::fs::create_dir_all(&self.workdir)
            .map_err(|e| AiError::Failed(format!("cannot create the AI workdir: {e}")))?;
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
            &self.args(model),
            &self.workdir,
            &env,
            prompt,
            self.timeout,
        )
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                AiError::Unavailable(format!("claude not found at {}: {e}", self.bin.display()))
            }
            _ => AiError::Failed(format!("cannot run claude -p: {e}")),
        })?;

        if output.timed_out {
            return Err(AiError::Failed(format!(
                "claude -p timed out after {}s",
                self.timeout.as_secs()
            )));
        }

        let parsed: Option<Value> = serde_json::from_str(output.stdout.trim()).ok();
        let text = parsed
            .as_ref()
            .and_then(|v| v.get("result"))
            .and_then(Value::as_str);
        let is_error = match &parsed {
            Some(v) if v.is_object() => v.get("is_error").and_then(Value::as_bool) == Some(true),
            _ => true,
        };

        if output.status != Some(0) || is_error {
            let detail = format!(
                "{} {}",
                output.stderr.trim(),
                text.unwrap_or(&output.stdout).trim()
            );
            return Err(classify::failure(
                &detail,
                prompt,
                output.status,
                |d, exit| classify::claude_unavailable(d, &self.bin, exit),
                "claude -p",
            ));
        }

        let text =
            text.ok_or_else(|| AiError::Failed("claude -p output has no result field".into()))?;
        json::first_object(text)
            .ok_or_else(|| AiError::Failed("claude -p result has no JSON object".into()))
    }
}
