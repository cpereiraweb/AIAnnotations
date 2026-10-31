//! Merges (and removes) our SessionStart/SessionEnd hooks in Claude's `settings.json` and
//! Codex's `hooks.json`. Port of the v1 `HookConfigMerger`, as pure text-to-text functions:
//! reading, backup and writing belong to the caller (see `installer::fsio`).
//!
//! Everything we do not own is preserved: key order, empty `{}`/`[]`, unicode, slashes, the
//! indentation and the trailing newline of the original.

use crate::model::Tool;
use serde::Serialize;
use serde_json::{Map, Value, json};
use similar::TextDiff;
use thiserror::Error;

/// v1 hook binary: any command containing it is ours (and legacy).
const V1_MARKER: &str = "bin/aiannotations-hook";
const CODEX_START_MATCHER: &str = "startup|resume|clear|compact|fork";
const EVENTS: [HookEvent; 2] = [HookEvent::Start, HookEvent::End];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    Start,
    End,
}

impl HookEvent {
    /// Key under `hooks` in the config file (also how v1's install state names the event).
    pub fn key(self) -> &'static str {
        match self {
            HookEvent::Start => "SessionStart",
            HookEvent::End => "SessionEnd",
        }
    }

    /// Inverse of [`HookEvent::key`].
    pub fn from_key(key: &str) -> Option<HookEvent> {
        EVENTS.into_iter().find(|e| e.key() == key)
    }
}

/// What to install: the tool and the full commands for each event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    pub tool: Tool,
    pub start: String,
    pub end: String,
}

impl HookSpec {
    fn command(&self, event: HookEvent) -> &str {
        match event {
            HookEvent::Start => &self.start,
            HookEvent::End => &self.end,
        }
    }

    /// The group we want under `event`.
    fn group(&self, event: HookEvent) -> Value {
        let timeout = if self.tool == Tool::Claude { 5 } else { 3 };
        let hooks =
            json!([{"type": "command", "command": self.command(event), "timeout": timeout}]);
        if self.tool == Tool::Codex && event == HookEvent::Start {
            json!({"matcher": CODEX_START_MATCHER, "hooks": hooks})
        } else {
            json!({"hooks": hooks})
        }
    }
}

/// Result of a merge: the new file contents and whether they differ from the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub contents: String,
    pub changed: bool,
    /// Event keys that did not exist before the merge: what the install "created".
    pub absent_events: Vec<HookEvent>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HookError {
    #[error("invalid hooks config: {0}")]
    InvalidJson(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookStatus {
    NotInstalled,
    Current,
    Outdated,
    Legacy,
}

/// True for a v1 command (`bin/aiannotations-hook`) or for ours: an executable named
/// `aiannotations`/`aiannotations.exe` (quoted or bare) followed by ` hook `.
pub fn is_ours(command: &str) -> bool {
    if command.contains(V1_MARKER) {
        return true;
    }
    let command = command.trim_start();
    let (exe, rest) = match command.chars().next() {
        Some('\'') => match split_single_quoted(&command[1..]) {
            Some(parts) => parts,
            None => return false,
        },
        Some('"') => match command[1..].split_once('"') {
            Some((exe, rest)) => (exe.to_string(), rest),
            None => return false,
        },
        _ => {
            let (exe, rest) =
                command.split_at(command.find(char::is_whitespace).unwrap_or(command.len()));
            (exe.to_string(), rest)
        }
    };
    let name = exe.as_str().rsplit(['/', '\\']).next().unwrap_or("");
    (name.eq_ignore_ascii_case("aiannotations") || name.eq_ignore_ascii_case("aiannotations.exe"))
        && rest.starts_with(" hook ")
}

/// Reads a POSIX single-quoted word (the opening quote already consumed), where a literal
/// `'` is written `'\''`. Returns the word and what follows the closing quote.
fn split_single_quoted(text: &str) -> Option<(String, &str)> {
    let mut word = String::new();
    let mut rest = text;
    loop {
        let (chunk, after) = rest.split_once('\'')?;
        word.push_str(chunk);
        match after.strip_prefix("\\''") {
            Some(next) => {
                word.push('\'');
                rest = next;
            }
            None => return Some((word, after)),
        }
    }
}

fn is_legacy(command: &str) -> bool {
    command.contains(V1_MARKER)
}

/// Installs (or updates) our hooks. `None` is a file that does not exist yet.
pub fn install(text: Option<&str>, spec: &HookSpec) -> Result<Merge, HookError> {
    let mut doc = Doc::parse(text)?;
    let (changed, absent_events) = merge_into(&mut doc, spec)?;
    Ok(doc.finish(changed, absent_events))
}

/// Removes only our commands. `created` is the install state: `None` means unknown, so an
/// emptied event key is removed; with state, it is removed only if we created it, otherwise
/// it goes back to `[]`.
pub fn uninstall(text: Option<&str>, created: Option<&[HookEvent]>) -> Result<Merge, HookError> {
    let mut doc = Doc::parse(text)?;
    let mut changed = false;
    if let Some(hooks) = doc.hooks_mut()? {
        let events: Vec<String> = hooks.keys().cloned().collect();
        for event in events {
            let groups = groups_of(hooks, &event)?;
            let (kept, touched) = strip_ours(&groups);
            if !touched {
                continue;
            }
            changed = true;
            let we_created = created.is_none_or(|c| c.iter().any(|e| e.key() == event));
            if kept.is_empty() && we_created {
                hooks.shift_remove(&event);
            } else {
                hooks.insert(event, Value::Array(kept));
            }
        }
    }
    Ok(doc.finish(changed, Vec::new()))
}

/// Where the file stands relative to `spec`.
pub fn status(text: Option<&str>, spec: &HookSpec) -> Result<HookStatus, HookError> {
    let mut doc = Doc::parse(text)?;
    let mut commands = Vec::new();
    if let Some(hooks) = doc.hooks_mut()? {
        for event in EVENTS {
            for group in groups_of(hooks, event.key())? {
                commands.extend(
                    group_hooks(&group)
                        .filter_map(|h| our_command(h))
                        .map(str::to_string),
                );
            }
        }
    }
    if commands.is_empty() {
        return Ok(HookStatus::NotInstalled);
    }
    if commands.iter().all(|c| is_legacy(c)) {
        return Ok(HookStatus::Legacy);
    }
    Ok(if merge_into(&mut doc, spec)?.0 {
        HookStatus::Outdated
    } else {
        HookStatus::Current
    })
}

/// Unified diff (3 lines of context) from `old` (`None` = new file) to `new`.
pub fn unified_diff(path_label: &str, old: Option<&str>, new: &str) -> String {
    TextDiff::from_lines(old.unwrap_or(""), new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path_label}"), &format!("b/{path_label}"))
        .to_string()
}

/// Parsed config plus the formatting of the original.
struct Doc<'a> {
    original: Option<&'a str>,
    config: Map<String, Value>,
    indent: String,
    trailing_newline: bool,
    crlf: bool,
    /// The file existed with content (blank or missing files count as `{}` but changed).
    existed: bool,
}

impl<'a> Doc<'a> {
    fn parse(text: Option<&'a str>) -> Result<Self, HookError> {
        let blank = text.is_none_or(|t| t.trim().is_empty());
        let config = if blank {
            Map::new()
        } else {
            match serde_json::from_str(text.unwrap_or_default()) {
                Ok(Value::Object(map)) => map,
                Ok(_) => return Err(HookError::InvalidJson("not a JSON object".into())),
                Err(e) => return Err(HookError::InvalidJson(e.to_string())),
            }
        };
        let source = if blank { "" } else { text.unwrap_or_default() };
        Ok(Doc {
            original: text,
            config,
            indent: detect_indent(source),
            trailing_newline: blank || source.ends_with('\n'),
            crlf: source.contains("\r\n"),
            existed: !blank,
        })
    }

    /// The `hooks` object (`null` counts as absent). Errors when it is something else.
    fn hooks_mut(&mut self) -> Result<Option<&mut Map<String, Value>>, HookError> {
        match self.config.get_mut("hooks") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(map)) => Ok(Some(map)),
            Some(_) => Err(HookError::InvalidJson(
                "\"hooks\" is not a JSON object".into(),
            )),
        }
    }

    fn finish(self, changed: bool, absent_events: Vec<HookEvent>) -> Merge {
        let contents = if changed {
            self.encode()
        } else {
            self.original.unwrap_or_default().to_string()
        };
        Merge {
            contents,
            changed,
            absent_events,
        }
    }

    fn encode(&self) -> String {
        let mut out = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(self.indent.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
        // Serializing a Map of Values to a Vec cannot fail.
        serde::Serialize::serialize(&self.config, &mut ser).expect("serialize JSON map");
        let mut text = String::from_utf8(out).expect("serde_json emits UTF-8");
        if self.trailing_newline {
            text.push('\n');
        }
        if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        }
    }
}

/// Leading whitespace of the first indented line; 2 spaces when there is none.
fn detect_indent(text: &str) -> String {
    text.lines()
        .find(|l| l.starts_with([' ', '\t']))
        .map(|l| l.chars().take_while(|c| matches!(c, ' ' | '\t')).collect())
        .unwrap_or_else(|| "  ".into())
}

/// Adds or updates our group under each event. Returns whether anything changed and which
/// event keys did not exist before.
fn merge_into(doc: &mut Doc, spec: &HookSpec) -> Result<(bool, Vec<HookEvent>), HookError> {
    let mut changed = !doc.existed;
    if matches!(doc.config.get("hooks"), None | Some(Value::Null)) {
        doc.config.insert("hooks".into(), Value::Object(Map::new()));
        changed = true;
    }
    let hooks = doc.hooks_mut()?.expect("hooks object was just ensured");
    let mut absent = Vec::new();
    for event in EVENTS {
        if !hooks.contains_key(event.key()) {
            absent.push(event);
        }
        let mut groups = groups_of(hooks, event.key())?;
        let desired = spec.group(event);
        match groups.iter().position(has_ours) {
            None => groups.push(desired),
            Some(i) => {
                let updated = upgrade(&groups[i], desired);
                if updated == groups[i] {
                    continue;
                }
                groups[i] = updated;
            }
        }
        hooks.insert(event.key().into(), Value::Array(groups));
        changed = true;
    }
    Ok((changed, absent))
}

/// The groups under `event`: a missing key or `null` is `[]`, anything else must be an array.
fn groups_of(hooks: &Map<String, Value>, event: &str) -> Result<Vec<Value>, HookError> {
    match hooks.get(event) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(groups)) => Ok(groups.clone()),
        Some(_) => Err(HookError::InvalidJson(format!(
            "hooks.{event} is not a list"
        ))),
    }
}

fn group_hooks(group: &Value) -> impl Iterator<Item = &Value> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn our_command(hook: &Value) -> Option<&str> {
    hook.get("command")
        .and_then(Value::as_str)
        .filter(|c| is_ours(c))
}

fn has_ours(group: &Value) -> bool {
    group_hooks(group).any(|h| our_command(h).is_some())
}

/// A group with only our command becomes the desired group (matcher included). In a shared
/// group only our (first) command is replaced and the others are left untouched.
fn upgrade(group: &Value, desired: Value) -> Value {
    if group_hooks(group).all(|h| our_command(h).is_some()) {
        return desired;
    }
    let mut placed = false;
    let hooks: Vec<Value> = group_hooks(group)
        .filter_map(|h| {
            if our_command(h).is_none() {
                Some(h.clone())
            } else if placed {
                None
            } else {
                placed = true;
                desired["hooks"][0].clone().into()
            }
        })
        .collect();
    let mut copy = group.clone();
    copy["hooks"] = Value::Array(hooks);
    copy
}

/// Groups without our commands (groups left empty are dropped) and whether anything went.
fn strip_ours(groups: &[Value]) -> (Vec<Value>, bool) {
    let mut kept = Vec::new();
    let mut touched = false;
    for group in groups {
        let Some(hooks) = group.get("hooks").and_then(Value::as_array) else {
            kept.push(group.clone());
            continue;
        };
        let others: Vec<Value> = hooks
            .iter()
            .filter(|h| our_command(h).is_none())
            .cloned()
            .collect();
        if others.len() == hooks.len() {
            kept.push(group.clone());
            continue;
        }
        touched = true;
        if !others.is_empty() {
            let mut copy = group.clone();
            copy["hooks"] = Value::Array(others);
            kept.push(copy);
        }
    }
    (kept, touched)
}
