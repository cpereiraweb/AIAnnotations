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

/// v1 (Laravel app) hook binary: any command containing it is legacy. Legacy hooks are not
/// ours: the two apps run side by side, and they are only replaced when the user asks.
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
    /// Codex on Windows runs `command` through PowerShell, where a quoted path followed by
    /// arguments is a syntax error. When set, these go to `commandWindows`, which Codex uses
    /// there instead of `command`.
    pub start_windows: Option<String>,
    pub end_windows: Option<String>,
}

impl HookSpec {
    fn command(&self, event: HookEvent) -> &str {
        match event {
            HookEvent::Start => &self.start,
            HookEvent::End => &self.end,
        }
    }

    fn command_windows(&self, event: HookEvent) -> Option<&str> {
        match event {
            HookEvent::Start => self.start_windows.as_deref(),
            HookEvent::End => self.end_windows.as_deref(),
        }
    }

    /// The group we want under `event`.
    fn group(&self, event: HookEvent) -> Value {
        let timeout = if self.tool == Tool::Claude { 5 } else { 3 };
        let mut hook = json!({"type": "command", "command": self.command(event)});
        if let Some(windows) = self.command_windows(event) {
            hook["commandWindows"] = json!(windows);
        }
        hook["timeout"] = json!(timeout);
        let hooks = json!([hook]);
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

/// What an install does with the v1 (Laravel app) hooks it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legacy {
    /// Leave them untouched: ours goes beside them.
    Keep,
    /// Ours takes the place of the first one; the others are removed.
    Replace,
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
}

/// True for ours: an executable named `aiannotations`/`aiannotations.exe` (quoted or bare)
/// followed by ` hook `. v1 commands are [`is_legacy`], not ours.
pub fn is_ours(command: &str) -> bool {
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

/// True for a v1 (Laravel app) hook command.
pub fn is_legacy(command: &str) -> bool {
    command.contains(V1_MARKER)
}

/// Installs (or updates) our hooks. `None` is a file that does not exist yet.
pub fn install(text: Option<&str>, spec: &HookSpec, legacy: Legacy) -> Result<Merge, HookError> {
    let mut doc = Doc::parse(text)?;
    let (changed, absent_events) = merge_into(&mut doc, spec, legacy)?;
    Ok(doc.finish(changed, absent_events))
}

/// True when a v1 (Laravel app) hook is under one of our events.
pub fn has_legacy(text: Option<&str>) -> Result<bool, HookError> {
    let mut doc = Doc::parse(text)?;
    let Some(hooks) = doc.hooks_mut()? else {
        return Ok(false);
    };
    for event in EVENTS {
        if groups_of(hooks, event.key())?
            .iter()
            .any(|g| group_hooks(g).any(|h| command_matches(h, is_legacy)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes only our commands (v1 hooks stay). `created` is the install state: `None` means unknown, so an
/// emptied event key is removed; with state, it is removed only if we created it, otherwise
/// it goes back to `[]`.
pub fn uninstall(text: Option<&str>, created: Option<&[HookEvent]>) -> Result<Merge, HookError> {
    let mut doc = Doc::parse(text)?;
    let mut changed = false;
    if let Some(hooks) = doc.hooks_mut()? {
        let events: Vec<String> = hooks.keys().cloned().collect();
        for event in events {
            let groups = groups_of(hooks, &event)?;
            let (kept, touched) = strip(&groups, is_ours);
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
    Ok(if merge_into(&mut doc, spec, Legacy::Keep)?.0 {
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

/// Adds or updates our group under each event (see [`Legacy`] for the v1 hooks). Returns
/// whether anything changed and which event keys did not exist before.
fn merge_into(
    doc: &mut Doc,
    spec: &HookSpec,
    legacy: Legacy,
) -> Result<(bool, Vec<HookEvent>), HookError> {
    let replace = legacy == Legacy::Replace;
    // In a replace, v1 commands count as ours: the one we take over and the ones we drop.
    let mine: fn(&str) -> bool = if replace {
        |c| is_ours(c) || is_legacy(c)
    } else {
        is_ours
    };
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
        let groups = groups_of(hooks, event.key())?;
        let desired = spec.group(event);
        let has =
            |g: &&Value, pred: fn(&str) -> bool| group_hooks(g).any(|h| command_matches(h, pred));
        // Our current group wins over a v1 one to take over.
        let target = groups
            .iter()
            .position(|g| has(&g, is_ours))
            .or_else(|| groups.iter().position(|g| has(&g, mine)));
        let mut merged = Vec::with_capacity(groups.len() + 1);
        for (i, group) in groups.iter().enumerate() {
            if Some(i) == target {
                merged.push(upgrade(group, desired.clone(), mine));
            } else if replace {
                merged.extend(strip_group(group, is_legacy));
            } else {
                merged.push(group.clone());
            }
        }
        if target.is_none() {
            merged.push(desired);
        }
        if merged == groups {
            continue;
        }
        hooks.insert(event.key().into(), Value::Array(merged));
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

fn command_matches(hook: &Value, pred: fn(&str) -> bool) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(pred)
}

fn our_command(hook: &Value) -> Option<&str> {
    hook.get("command")
        .and_then(Value::as_str)
        .filter(|c| is_ours(c))
}

/// A group with only `mine` commands becomes the desired group (matcher included). In a shared
/// group only the first `mine` command is replaced, the other `mine` ones are dropped and the
/// rest is left untouched.
fn upgrade(group: &Value, desired: Value, mine: fn(&str) -> bool) -> Value {
    if group_hooks(group).all(|h| command_matches(h, mine)) {
        return desired;
    }
    let mut placed = false;
    let hooks: Vec<Value> = group_hooks(group)
        .filter_map(|h| {
            if !command_matches(h, mine) {
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

/// Groups without the `pred` commands (groups left empty are dropped) and whether anything
/// went.
fn strip(groups: &[Value], pred: fn(&str) -> bool) -> (Vec<Value>, bool) {
    let kept: Vec<Value> = groups.iter().filter_map(|g| strip_group(g, pred)).collect();
    let touched = kept.as_slice() != groups;
    (kept, touched)
}

/// The group without the `pred` commands; `None` when nothing else was left in it.
fn strip_group(group: &Value, pred: fn(&str) -> bool) -> Option<Value> {
    let Some(hooks) = group.get("hooks").and_then(Value::as_array) else {
        return Some(group.clone());
    };
    let others: Vec<Value> = hooks
        .iter()
        .filter(|h| !command_matches(h, pred))
        .cloned()
        .collect();
    if others.len() == hooks.len() {
        return Some(group.clone());
    }
    (!others.is_empty()).then(|| {
        let mut copy = group.clone();
        copy["hooks"] = Value::Array(others);
        copy
    })
}
