//! Reader for Codex rollout JSONL (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), lines shaped
//! `{timestamp, type, payload}`. Port of `CodexRolloutReader.php` (Inv §3.7 "Codex rollout JSONL").
//!
//! Messages come from `event_msg` `user_message`/`agent_message`. Newer rollouts only carry them
//! in `event_msg` `item_completed` (`UserMessage`/`AgentMessage`), used when there is no
//! `user_message` at all.

use std::ops::ControlFlow;
use std::path::Path;

use serde_json::Value;

use super::codex_source::classify;
use super::extract::{FileTouch, TranscriptExtract, Turn};
use super::jsonl::{for_each_value, for_each_value_until};
use super::php::{PHP_WS, string};
use crate::Result;
use crate::time::parse_ts;

const TOOL_CALL_TYPES: [&str; 3] = ["function_call", "custom_tool_call", "local_shell_call"];
/// Lines scanned from the start of a rollout looking for `session_meta`.
const META_SCAN_LINES: usize = 5;

/// Reads a rollout; `None` if the file does not exist.
pub fn read(path: &Path) -> Result<Option<TranscriptExtract>> {
    if !path.is_file() {
        return Ok(None);
    }

    let mut x = TranscriptExtract::default();
    let mut source = Value::Null;
    let mut events: Vec<Turn> = Vec::new();
    let mut items: Vec<Turn> = Vec::new();
    let mut patch = Touched::default();
    let mut change = Touched::default();

    for_each_value(path, |line| {
        let ms = line
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        if let Some(ms) = ms {
            x.first_at.get_or_insert(ms);
            x.last_at = Some(ms);
            x.activity_times.push(ms);
        }

        let kind = line.get("type").and_then(Value::as_str);
        let payload = line.get("payload").filter(|p| p.is_object());
        let field = |k: &str| payload.and_then(|p| p.get(k));
        let payload_type = field("type").and_then(Value::as_str);

        match (kind, payload_type) {
            (Some("session_meta"), _) => {
                x.cwd = string(field("cwd")).or(x.cwd.take());
                let branch = field("git").and_then(|g| g.get("branch"));
                x.branch = string(branch).or(x.branch.take());
                if let Some(s) = field("source").filter(|s| !s.is_null()) {
                    source = s.clone();
                }
                // The line is written after the session was created, which payload.timestamp has.
                let created = field("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_ts);
                if let Some(created) = created
                    && x.first_at.is_none_or(|first| created < first)
                {
                    x.first_at = Some(created);
                }
            }
            (Some("turn_context"), _) => {
                x.model = string(field("model")).or(x.model.take());
                if x.cwd.is_none() {
                    x.cwd = string(field("cwd"));
                }
            }
            (Some("event_msg"), Some("user_message")) => {
                add_prompt(&mut events, string(field("message")));
            }
            (Some("event_msg"), Some("agent_message")) => {
                add_answer(&mut events, string(field("message")));
            }
            (Some("event_msg"), Some("item_completed")) => {
                let Some(item) = field("item").filter(|i| i.is_object() || i.is_array()) else {
                    return;
                };
                match item.get("type").and_then(Value::as_str) {
                    Some("UserMessage") => add_prompt(&mut items, texts(item.get("content"))),
                    Some("AgentMessage") => add_answer(
                        &mut items,
                        texts(item.get("content")).or_else(|| string(item.get("text"))),
                    ),
                    Some("FileChange") => {
                        for file in change_paths(item.get("changes")) {
                            change.push(file, ms);
                        }
                    }
                    _ => {}
                }
            }
            (Some("response_item"), Some(t)) if TOOL_CALL_TYPES.contains(&t) => {
                x.tool_use_count += 1;
                if field("name").and_then(Value::as_str) == Some("apply_patch") {
                    for file in patch_paths(payload) {
                        patch.push(file, ms);
                    }
                }
            }
            _ => {}
        }
    })?;

    x.turns = if events.is_empty() { items } else { events };
    (x.origin, x.parent_session_id, x.unknown_origin) = classify(&source);
    let used = if change.files.is_empty() {
        patch
    } else {
        change
    };
    x.files_touched = used.files;
    x.file_touches = used.touches;

    Ok(Some(x))
}

/// `session_meta` within the first 5 lines declares `history_mode = "paginated"`: the content then
/// lives in the thread-history databases instead of the rollout.
pub fn is_paginated(path: &Path) -> bool {
    let mut scan = MetaScan::default();
    let _ = for_each_value_until(path, |line| scan.feed(&line));
    scan.paginated
}

/// Decides `is_paginated` line by line; `feed` breaks once the answer is known, so the reader
/// stops (the first `session_meta` decides, otherwise after `META_SCAN_LINES` valid lines).
#[derive(Default)]
struct MetaScan {
    consumed: usize,
    paginated: bool,
}

impl MetaScan {
    fn feed(&mut self, line: &Value) -> ControlFlow<()> {
        self.consumed += 1;
        if line.get("type").and_then(Value::as_str) == Some("session_meta") {
            self.paginated = line
                .get("payload")
                .and_then(|p| p.get("history_mode"))
                .and_then(Value::as_str)
                == Some("paginated");
            return ControlFlow::Break(());
        }
        if self.consumed >= META_SCAN_LINES {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// Unique files in first-touch order, plus every touch with its line time.
#[derive(Default)]
struct Touched {
    files: Vec<String>,
    touches: Vec<FileTouch>,
}

impl Touched {
    fn push(&mut self, path: String, at_ms: Option<i64>) {
        if !self.files.contains(&path) {
            self.files.push(path.clone());
        }
        self.touches.push(FileTouch { path, at_ms });
    }
}

/// Paths of the `*** (Add|Update|Delete) File: <path>` lines of an `apply_patch` call.
fn patch_paths(payload: Option<&Value>) -> Vec<String> {
    let field = |k: &str| payload.and_then(|p| p.get(k));
    let mut input = field("input").and_then(Value::as_str).map(str::to_string);

    if input.is_none()
        && let Some(arguments) = field("arguments").and_then(Value::as_str)
    {
        input = match serde_json::from_str::<Value>(arguments) {
            Ok(decoded @ (Value::Object(_) | Value::Array(_))) => decoded
                .get("input")
                .and_then(Value::as_str)
                .map(String::from),
            _ => Some(arguments.to_string()),
        };
    }

    let Some(input) = input else {
        return Vec::new();
    };
    input.split('\n').filter_map(patch_line_path).collect()
}

/// PCRE `^\*\*\* (?:Add|Update|Delete) File: (.+?)\s*$` on one line (`\s` is ASCII here): the
/// shortest non-empty prefix after which only whitespace remains.
fn patch_line_path(line: &str) -> Option<String> {
    let rest = ["Add", "Update", "Delete"]
        .iter()
        .find_map(|verb| line.strip_prefix(&format!("*** {verb} File: ")))?;
    let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r');
    for (i, c) in rest.char_indices() {
        let end = i + c.len_utf8();
        if rest[end..].chars().all(is_ws) {
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// `FileChange.changes` is a `{path: change}` map in the rollout and a `[{path, kind, diff}]`
/// list in the paginated history.
pub(super) fn change_paths(changes: Option<&Value>) -> Vec<String> {
    match changes {
        Some(Value::Array(list)) => list_paths(list.iter()),
        Some(Value::Object(map)) => {
            // PHP decodes an object keyed "0","1",… as a list, and turns integer-like keys into ints.
            if map.keys().enumerate().all(|(i, k)| *k == i.to_string()) && !map.is_empty() {
                return list_paths(map.values());
            }
            map.keys()
                .filter(|k| !k.is_empty() && !is_php_int_key(k))
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    }
}

/// `array_filter(array_map(str(c.path)))`: drops missing, empty and "0" values.
fn list_paths<'a>(changes: impl Iterator<Item = &'a Value>) -> Vec<String> {
    changes
        .filter_map(|c| string(c.get("path")))
        .filter(|p| p != "0")
        .collect()
}

/// Keys PHP stores as integers: canonical decimal i64 (no sign but `-`, no leading zeros).
fn is_php_int_key(key: &str) -> bool {
    let digits = key.strip_prefix('-').unwrap_or(key);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && key != "-0";
    canonical && key.parse::<i64>().is_ok()
}

/// Opens a turn with `text`, ignoring blank text.
fn add_prompt(turns: &mut Vec<Turn>, text: Option<String>) {
    if let Some(text) = text.map(|t| t.trim_matches(PHP_WS).to_string())
        && !text.is_empty()
    {
        turns.push(Turn {
            prompt: text,
            response: None,
        });
    }
}

/// Sets the answer of the last turn; an answer with no prompt before it (subagent) opens a turn
/// with an empty prompt, without inventing text.
fn add_answer(turns: &mut Vec<Turn>, text: Option<String>) {
    let Some(text) = text.map(|t| t.trim_matches(PHP_WS).to_string()) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    match turns.last_mut() {
        Some(last) => last.response = Some(text),
        None => turns.push(Turn {
            prompt: String::new(),
            response: Some(text),
        }),
    }
}

/// Joins the text blocks of a `content[]` (`{type: text|Text, text}`) with newlines.
fn texts(content: Option<&Value>) -> Option<String> {
    let blocks: Vec<&Value> = match content? {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => o.values().collect(),
        _ => return None,
    };
    let texts: Vec<&str> = blocks
        .into_iter()
        .filter(|b| {
            b.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.eq_ignore_ascii_case("text"))
        })
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionOrigin;
    use serde_json::json;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/rollout.jsonl")
    }

    fn t(v: &str) -> i64 {
        parse_ts(v).unwrap()
    }

    fn temp_rollout(lines: &[Value]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    fn meta(extra: Value) -> Value {
        let mut payload = json!({"id": "s", "cwd": "/tmp/x", "source": "vscode"});
        payload
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"timestamp": "2026-10-20T15:00:00.000Z", "type": "session_meta", "payload": payload})
    }

    fn item(ts: &str, item: Value) -> Value {
        json!({"timestamp": ts, "type": "event_msg", "payload": {"type": "item_completed", "item": item}})
    }

    fn turn(prompt: &str, response: Option<&str>) -> Turn {
        Turn {
            prompt: prompt.into(),
            response: response.map(Into::into),
        }
    }

    fn read_fixture() -> TranscriptExtract {
        read(&fixture()).unwrap().unwrap()
    }

    /// Codex: apply_patch extrai os paths
    #[test]
    fn apply_patch_extracts_paths() {
        let x = read_fixture();
        assert_eq!(
            x.files_touched,
            ["src/carrinho.py", "src/app.py", "src/legado.py"]
        );
        assert_eq!(x.tool_use_count, 3);
    }

    /// Codex: extrai metadados, prompts e respostas e ignora a linha truncada
    #[test]
    fn extracts_metadata_prompts_and_answers_and_skips_truncated_line() {
        let x = read_fixture();
        assert_eq!(x.cwd.as_deref(), Some("/home/fulano/Code/Loja Virtual"));
        assert_eq!(x.branch.as_deref(), Some("develop"));
        assert_eq!(x.model.as_deref(), Some("gpt-modelo-b"));
        assert_eq!(x.title, None);
        assert_eq!(x.first_at, Some(t("2026-10-20T15:00:00Z")));
        assert_eq!(x.last_at, Some(t("2026-10-20T15:10:06Z")));
        assert_eq!(
            x.turns,
            [
                turn(
                    "Adicione o carrinho de compras.",
                    Some("Carrinho criado e testes passando.")
                ),
                turn("Remova o arquivo legado.", Some("Arquivo legado removido.")),
            ]
        );
    }

    /// Codex: source exec é automation
    #[test]
    fn source_exec_is_automation() {
        assert_eq!(read_fixture().origin, SessionOrigin::Automation);
    }

    /// Codex: source de subagent no rollout preenche parentSessionId
    #[test]
    fn subagent_source_fills_parent_session_id() {
        let (_d, path) = temp_rollout(&[meta(json!({
            "id": "filho",
            "source": {"subagent": {"thread_spawn": {"parent_thread_id": "pai-1", "depth": 1}}},
        }))]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.origin, SessionOrigin::Subagent);
        assert_eq!(x.parent_session_id.as_deref(), Some("pai-1"));
        assert_eq!(x.branch, None);
    }

    /// Codex: usa os item_completed quando não há event_msg de mensagem
    #[test]
    fn uses_item_completed_when_there_are_no_message_events() {
        let (_d, path) = temp_rollout(&[
            meta(json!({})),
            item(
                "2026-10-20T15:00:01.000Z",
                json!({"type": "UserMessage", "id": "u1", "content": [{"type": "text", "text": "Corrija o bug."}]}),
            ),
            item(
                "2026-10-20T15:00:02.000Z",
                json!({"type": "FileChange", "id": "f1", "changes": {"/tmp/x/a.py": {"type": "update"}}}),
            ),
            item(
                "2026-10-20T15:00:03.000Z",
                json!({"type": "AgentMessage", "id": "a1", "phase": "final_answer", "content": [{"type": "Text", "text": "Bug corrigido."}]}),
            ),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.origin, SessionOrigin::Interactive);
        assert_eq!(x.turns, [turn("Corrija o bug.", Some("Bug corrigido."))]);
        assert_eq!(x.files_touched, ["/tmp/x/a.py"]);
    }

    /// Codex: devolve null sem caminho ou com arquivo inexistente
    #[test]
    fn returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("nada.jsonl")).unwrap().is_none());
    }

    /// Codex: firstAt usa o horário de criação do session_meta quando é anterior à linha
    #[test]
    fn first_at_uses_session_meta_creation_time_when_earlier() {
        let (_d, path) = temp_rollout(&[
            json!({"timestamp": "2026-10-20T15:00:30.000Z", "type": "session_meta", "payload": {
                "id": "s", "timestamp": "2026-10-20T15:00:00.000Z", "cwd": "/tmp/x", "source": "cli"}}),
            json!({"timestamp": "2026-10-20T15:01:00.000Z", "type": "event_msg",
                "payload": {"type": "user_message", "message": "Oi."}}),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.first_at, Some(t("2026-10-20T15:00:00Z")));
        assert_eq!(x.last_at, Some(t("2026-10-20T15:01:00Z")));
    }

    /// rollout de subagente sem prompt mantém a resposta, sem inventar prompt
    #[test]
    fn subagent_rollout_without_prompt_keeps_answer() {
        let (_d, path) = temp_rollout(&[
            meta(json!({"cwd": "/x", "source": {"subagent": {"other": "guardian"}}})),
            json!({"timestamp": "2026-10-20T15:00:01.000Z", "type": "event_msg",
                "payload": {"type": "agent_message", "message": "Revisei e está ok."}}),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.turns, [turn("", Some("Revisei e está ok."))]);
        assert_eq!(x.origin, SessionOrigin::Subagent);
    }

    /// rollout novo (item_completed) de subagente sem prompt mantém a resposta
    #[test]
    fn new_rollout_item_completed_without_prompt_keeps_answer() {
        let (_d, path) = temp_rollout(&[
            meta(json!({"cwd": "/x", "source": "cli"})),
            item(
                "2026-10-20T15:00:01.000Z",
                json!({"type": "AgentMessage", "content": [{"type": "Text", "text": "Feito."}]}),
            ),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.turns, [turn("", Some("Feito."))]);
    }

    /// Codex: a primeira atividade depois de um instante vem do timestamp das linhas
    #[test]
    fn first_activity_after_instant_comes_from_line_timestamps() {
        assert_eq!(
            read_fixture().first_at_after(t("2026-10-20T15:00:08Z")),
            Some(t("2026-10-20T15:10:00Z"))
        );
    }

    /// Codex: guarda o horário de cada arquivo do apply_patch (timestamp da linha)
    #[test]
    fn apply_patch_files_keep_their_line_time() {
        let x = read_fixture();
        assert_eq!(
            x.files_touched_between(Some(t("2026-10-20T15:10:00Z")), None),
            ["src/legado.py", "src/app.py"]
        );
        assert_eq!(
            x.files_touched_between(None, Some(t("2026-10-20T15:05:00Z"))),
            ["src/carrinho.py", "src/app.py"]
        );
    }

    /// Codex: FileChange usa o timestamp da linha
    #[test]
    fn file_change_uses_line_timestamp() {
        let change = |ts, id, file: &str| {
            item(
                ts,
                json!({"type": "FileChange", "id": id, "changes": {file: {"type": "update"}}}),
            )
        };
        let (_d, path) = temp_rollout(&[
            meta(json!({})),
            change("2026-10-20T15:00:02.000Z", "f1", "/tmp/x/a.py"),
            change("2026-10-20T16:00:00.000Z", "f2", "/tmp/x/b.py"),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(
            x.files_touched_between(Some(t("2026-10-20T15:30:00Z")), None),
            ["/tmp/x/b.py"]
        );
    }

    #[test]
    fn user_message_events_win_over_item_completed_and_turns_stay_aligned() {
        let msg = |ts, kind, text| json!({"timestamp": ts, "type": "event_msg", "payload": {"type": kind, "message": text}});
        let (_d, path) = temp_rollout(&[
            meta(json!({})),
            item(
                "2026-10-20T15:00:00.500Z",
                json!({"type": "UserMessage", "content": [{"type": "text", "text": "ignorado"}]}),
            ),
            msg("2026-10-20T15:00:01.000Z", "user_message", "  Um  \n"),
            msg("2026-10-20T15:00:02.000Z", "user_message", "Dois"),
            msg("2026-10-20T15:00:03.000Z", "agent_message", "R2"),
            msg("2026-10-20T15:00:04.000Z", "user_message", "   "),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.turns, [turn("Um", None), turn("Dois", Some("R2"))]);
        assert_eq!(
            x.fingerprint(),
            format!("{}:2:2", t("2026-10-20T15:00:04Z") / 1000)
        );
    }

    #[test]
    fn file_change_takes_precedence_over_apply_patch_and_list_changes_work() {
        let patch = json!({"timestamp": "2026-10-20T15:00:03.000Z", "type": "response_item",
            "payload": {"type": "custom_tool_call", "name": "apply_patch",
                "input": "*** Begin Patch\n*** Add File: p.py\n*** End Patch\n"}});
        let (_d, path) = temp_rollout(&[
            meta(json!({})),
            patch,
            item(
                "2026-10-20T15:00:04.000Z",
                json!({"type": "FileChange", "changes": [{"path": "/tmp/x/z.py", "kind": "add"}, {"kind": "x"}, {"path": "/tmp/x/a.py"}]}),
            ),
            item(
                "2026-10-20T15:00:05.000Z",
                json!({"type": "FileChange", "changes": {"/tmp/x/z.py": {}, "/tmp/x/b.py": {}, "7": {}}}),
            ),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(
            x.files_touched,
            ["/tmp/x/z.py", "/tmp/x/a.py", "/tmp/x/b.py"]
        );
        assert_eq!(x.file_touches.len(), 4);
        assert_eq!(x.tool_use_count, 1);
    }

    #[test]
    fn apply_patch_reads_arguments_json_or_raw_and_trims_paths() {
        let call = |payload: Value| json!({"timestamp": "2026-10-20T15:00:03.000Z", "type": "response_item", "payload": payload});
        let (_d, path) = temp_rollout(&[
            call(json!({"type": "function_call", "name": "apply_patch",
                "arguments": json!({"input": "*** Update File: a b.py  \r\n*** Delete File: c.py"}).to_string()})),
            call(json!({"type": "local_shell_call", "name": "apply_patch",
                "arguments": "*** Add File: raw.py\n"})),
            call(json!({"type": "function_call", "name": "apply_patch",
                "arguments": json!({"other": 1}).to_string()})),
            call(
                json!({"type": "function_call", "name": "exec_command", "input": "*** Add File: no.py"}),
            ),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.files_touched, ["a b.py", "c.py", "raw.py"]);
        assert_eq!(x.tool_use_count, 4);
    }

    #[test]
    fn turn_context_fills_cwd_only_when_missing_and_agent_text_fallback() {
        let (_d, path) = temp_rollout(&[
            json!({"timestamp": "2026-10-20T15:00:00.000Z", "type": "turn_context",
                "payload": {"cwd": "/from/context", "model": "m1"}}),
            meta(json!({"cwd": "/from/meta"})),
            json!({"timestamp": "2026-10-20T15:00:01.000Z", "type": "turn_context",
                "payload": {"cwd": "/other"}}),
            item(
                "2026-10-20T15:00:02.000Z",
                json!({"type": "AgentMessage", "text": "Só texto."}),
            ),
        ]);
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.cwd.as_deref(), Some("/from/meta"));
        assert_eq!(x.model.as_deref(), Some("m1"));
        assert_eq!(x.turns, [turn("", Some("Só texto."))]);
    }

    #[test]
    fn patch_line_path_matches_pcre_semantics() {
        assert_eq!(
            patch_line_path("*** Add File: x.py ").as_deref(),
            Some("x.py")
        );
        assert_eq!(patch_line_path("*** Move File: x.py"), None);
        assert_eq!(patch_line_path("*** Add File: "), None);
        assert_eq!(patch_line_path("  *** Add File: x"), None);
    }

    /// Feeds values until the scan breaks; returns (paginated, lines consumed).
    fn drive(lines: &[Value]) -> (bool, usize) {
        let mut scan = MetaScan::default();
        for line in lines {
            if scan.feed(line).is_break() {
                break;
            }
        }
        (scan.paginated, scan.consumed)
    }

    #[test]
    fn meta_scan_consumes_exactly_what_it_needs() {
        let noise = json!({"type": "turn_context", "payload": {}});
        let paginated = meta(json!({"history_mode": "paginated"}));

        // (a) session_meta on line 1 decides after 1 line.
        let mut a = vec![paginated.clone()];
        a.extend(vec![noise.clone(); 10]);
        assert_eq!(drive(&a), (true, 1));

        // (b) 5 non-meta lines, then a paginated session_meta on line 6: false, 5 consumed.
        let mut b = vec![noise.clone(); 5];
        b.push(paginated.clone());
        assert_eq!(drive(&b), (false, 5));

        // session_meta on line 5 still counts.
        let mut c = vec![noise.clone(); 4];
        c.push(paginated);
        assert_eq!(drive(&c), (true, 5));
    }

    #[test]
    fn invalid_lines_do_not_count_toward_the_scan_limit() {
        let noise =
            json!({"timestamp": "2026-10-20T15:00:00.000Z", "type": "turn_context", "payload": {}});
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut body = String::from("garbage\n{\"truncated\":\n\n");
        for _ in 0..4 {
            body.push_str(&format!("{noise}\nnot json\n"));
        }
        body.push_str(&format!("{}\n", meta(json!({"history_mode": "paginated"}))));
        std::fs::write(&path, body).unwrap();
        // Only 4 valid lines precede the session_meta, so it is still found.
        assert!(is_paginated(&path));
    }

    #[test]
    fn is_paginated_ignores_a_session_meta_after_five_valid_lines() {
        let noise =
            json!({"timestamp": "2026-10-20T15:00:00.000Z", "type": "turn_context", "payload": {}});
        let mut lines = vec![noise; 5];
        lines.push(meta(json!({"history_mode": "paginated"})));
        let (_d, p) = temp_rollout(&lines);
        assert!(!is_paginated(&p));
    }

    #[test]
    fn detects_paginated_history_in_first_five_lines() {
        let noise =
            json!({"timestamp": "2026-10-20T15:00:00.000Z", "type": "turn_context", "payload": {}});
        let paginated = meta(json!({"history_mode": "paginated"}));

        let (_d, p) = temp_rollout(std::slice::from_ref(&paginated));
        assert!(is_paginated(&p));
        let (_d, p) = temp_rollout(&[meta(json!({"history_mode": "legacy"}))]);
        assert!(!is_paginated(&p));
        let (_d, p) = temp_rollout(&[meta(json!({}))]);
        assert!(!is_paginated(&p));
        let four = vec![
            noise.clone(),
            noise.clone(),
            noise.clone(),
            noise.clone(),
            paginated.clone(),
        ];
        let (_d, p) = temp_rollout(&four);
        assert!(is_paginated(&p));
        let six = vec![noise.clone(); 5]
            .into_iter()
            .chain([paginated])
            .collect::<Vec<_>>();
        let (_d, p) = temp_rollout(&six);
        assert!(!is_paginated(&p));
        assert!(!is_paginated(Path::new("/nonexistent/rollout.jsonl")));
        // The first session_meta decides.
        let (_d, p) = temp_rollout(&[
            meta(json!({"history_mode": "legacy"})),
            meta(json!({"history_mode": "paginated"})),
        ]);
        assert!(!is_paginated(&p));
    }
}
