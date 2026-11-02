//! The user's `settings.json` (app data dir), applied on top of `Settings::default()`.
//! Absent fields keep the default; the core never reads the clock, so the caller passes the
//! stamp used to name a file moved aside.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::installer::fsio::write_atomic;
use crate::model::Tool;
use crate::settings::Lang;
use crate::{Result, Settings};

/// What a launchable app is for: opening a project in an editor or running a CLI in a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub onboarding_done: bool,
    pub ai_provider: Option<Tool>,
    pub claude_model: Option<String>,
    pub claude_briefing_model: Option<String>,
    /// `Some("")` means the Codex default model.
    pub codex_model: Option<String>,
    pub codex_briefing_model: Option<String>,
    pub summarize_history: Option<bool>,
    pub autostart: Option<bool>,
    /// Interface and AI language; `None` follows the system.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_lang"
    )]
    pub lang: Option<Lang>,
}

/// An unknown value (hand-edited file) means "follow the system" instead of invalidating
/// the whole file.
fn lenient_lang<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Lang>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

impl UserSettings {
    /// Every `Some` field replaces the one in `base`; an empty Codex model becomes `None`.
    pub fn apply(&self, base: Settings) -> Settings {
        let codex = |v: &Option<String>, current: Option<String>| match v {
            Some(m) if m.is_empty() => None,
            Some(m) => Some(m.clone()),
            None => current,
        };
        Settings {
            ai_provider: self.ai_provider.unwrap_or(base.ai_provider),
            claude_model: self.claude_model.clone().unwrap_or(base.claude_model),
            claude_briefing_model: self
                .claude_briefing_model
                .clone()
                .unwrap_or(base.claude_briefing_model),
            codex_model: codex(&self.codex_model, base.codex_model),
            codex_briefing_model: codex(&self.codex_briefing_model, base.codex_briefing_model),
            summarize_history: self.summarize_history.unwrap_or(base.summarize_history),
            ..base
        }
    }

    /// The chosen language, else the system-detected one.
    pub fn effective_lang(&self, detected: Lang) -> Lang {
        self.lang.unwrap_or(detected)
    }

    /// Until the first-run wizard finishes, no AI call may happen.
    pub fn starts_paused(&self) -> bool {
        !self.onboarding_done
    }
}

/// Missing file gives the default without creating anything. Invalid JSON is moved to
/// `<name>.invalid-<stamp>` (same folder) and the default is returned.
pub fn load(path: &Path, stamp: &str) -> Result<UserSettings> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(UserSettings::default()),
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str(&text) {
        Ok(s) => Ok(s),
        Err(_) => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(format!(".invalid-{stamp}"));
            std::fs::rename(path, aside)?;
            Ok(UserSettings::default())
        }
    }
}

/// Pretty JSON (2 spaces), written atomically.
pub fn save(path: &Path, s: &UserSettings) -> Result<()> {
    let mut text = serde_json::to_string_pretty(s)?;
    text.push('\n');
    write_atomic(path, &text)?;
    Ok(())
}
