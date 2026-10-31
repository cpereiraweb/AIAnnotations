//! `install-state.json` (v1 format): which event keys our install created in each hooks
//! file, so an uninstall removes only those (an `"SessionEnd": []` that was already there
//! comes back as it was). A missing file, or one we cannot parse, is an empty state.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Result;
use crate::installer::fsio::write_atomic;
use crate::installer::hooks::HookEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallState {
    pub version: u32,
    pub files: BTreeMap<String, FileState>,
}

impl Default for InstallState {
    fn default() -> Self {
        InstallState {
            version: 1,
            files: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileState {
    #[serde(with = "event_keys")]
    pub created_events: Vec<HookEvent>,
}

/// On disk the events are v1's keys (`SessionStart`/`SessionEnd`); unknown ones are dropped.
mod event_keys {
    use super::*;

    pub fn serialize<S: Serializer>(
        events: &[HookEvent],
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(events.iter().map(|e| e.key()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Vec<HookEvent>, D::Error> {
        let keys = Vec::<serde_json::Value>::deserialize(d)?;
        Ok(keys
            .iter()
            .filter_map(|k| k.as_str().and_then(HookEvent::from_key))
            .collect())
    }
}

fn key_of(file: &Path) -> String {
    file.to_string_lossy().into_owned()
}

impl InstallState {
    /// Adds `events` to what is known for `file`, without duplicates. An empty list creates
    /// no entry: it would invent state for an install that predates the state file.
    pub fn record_created(&mut self, file: &Path, events: &[HookEvent]) {
        if events.is_empty() {
            return;
        }
        let entry = self.files.entry(key_of(file)).or_default();
        for e in events {
            if !entry.created_events.contains(e) {
                entry.created_events.push(*e);
            }
        }
    }

    /// Events the install created in `file`, or `None` when the state does not know it.
    pub fn created(&self, file: &Path) -> Option<&[HookEvent]> {
        self.files
            .get(&key_of(file))
            .map(|f| f.created_events.as_slice())
    }
}

/// Missing or unparsable file gives an empty state (like v1: the uninstall then falls back
/// to its old behaviour).
pub fn load(path: &Path) -> Result<InstallState> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(InstallState::default()),
        Err(e) => Err(e.into()),
    }
}

/// Pretty JSON (2 spaces) and a trailing newline, written atomically.
pub fn save(path: &Path, s: &InstallState) -> Result<()> {
    let mut text = serde_json::to_string_pretty(s)?;
    text.push('\n');
    write_atomic(path, &text)?;
    Ok(())
}
