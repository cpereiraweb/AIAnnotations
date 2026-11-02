//! AI integration foundation: error classification, the client trait and helpers.

pub mod binary;
pub mod classify;
pub mod claude;
pub mod codex;
pub mod json;
pub mod models;
pub mod process;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::model::Tool;
use crate::{Paths, Settings};

/// Failure of an AI call. `RateLimited` and `Unavailable` pause the whole batch;
/// `Failed` counts as an attempt for the item (Inv §4.1–4.2).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    #[error("AI rate limited: {0}")]
    RateLimited(String),
    #[error("AI unavailable: {0}")]
    Unavailable(String),
    #[error("AI call failed: {0}")]
    Failed(String),
}

/// Which configured model a call uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Summary,
    Briefing,
}

/// A CLI-backed AI that answers a prompt with one JSON object.
pub trait AiClient: Send + Sync {
    fn complete_json(&self, prompt: &str, role: ModelRole) -> Result<Map<String, Value>, AiError>;
}

/// Builds the client for `settings.ai_provider`. The binary comes from
/// `AIANNOTATIONS_CLAUDE_BIN` / `AIANNOTATIONS_CODEX_BIN` (read through `env`) or is
/// looked up with `PATH` (also read through `env`) and `home`.
pub fn client_for(
    settings: &Settings,
    paths: &Paths,
    env: impl Fn(&str) -> Option<OsString>,
    home: &Path,
) -> Box<dyn AiClient> {
    let path_var = env("PATH");
    let timeout = Duration::from_secs(u64::from(settings.ai_timeout_seconds));
    let workdir = paths.ai_workdir.clone();
    match settings.ai_provider {
        Tool::Claude => {
            let override_bin = env("AIANNOTATIONS_CLAUDE_BIN");
            Box::new(claude::ClaudeCli {
                bin: binary::resolve("claude", override_bin.as_deref(), home, path_var.as_deref()),
                workdir,
                timeout,
                summary_model: settings.claude_model.clone(),
                briefing_model: settings.claude_briefing_model.clone(),
            })
        }
        Tool::Codex => {
            let override_bin = env("AIANNOTATIONS_CODEX_BIN");
            Box::new(codex::CodexCli {
                bin: binary::resolve("codex", override_bin.as_deref(), home, path_var.as_deref()),
                workdir,
                timeout,
                summary_model: settings.codex_model.clone(),
                briefing_model: settings.codex_briefing_model.clone(),
            })
        }
    }
}
