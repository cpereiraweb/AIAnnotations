//! Reader for Claude Code JSONL transcripts (`~/.claude/projects/<project>/<session>.jsonl`).
//! Port of `ClaudeTranscriptReader.php` (Inv §3.7 "Claude JSONL").

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::extract::{FileTouch, TranscriptExtract, Turn};
use super::jsonl::for_each_value;
use super::php::{PHP_WS, php_empty, string};
use crate::Result;
use crate::model::SessionOrigin;
use crate::time::parse_ts;

/// Tools whose `input.file_path` (or `notebook_path`) is a changed file.
const FILE_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
/// Known interactive entrypoints; `sdk*` is automation.
const INTERACTIVE_ENTRYPOINTS: [&str; 3] = ["cli", "claude-desktop", "claude-vscode"];
/// "User" messages that are really injected by the harness.
const NOISE_PREFIXES: [&str; 7] = [
    "<task-notification>",
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<system-reminder>",
];

/// `projects/*/<session_id>.jsonl` under `claude_dir`; first match in name order.
pub fn locate(claude_dir: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains('/') {
        return None;
    }
    let mut projects: Vec<PathBuf> = std::fs::read_dir(claude_dir.join("projects"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    projects.sort();
    projects
        .into_iter()
        .map(|p| p.join(format!("{session_id}.jsonl")))
        .find(|p| p.is_file())
}

/// Reads a transcript; `None` if the file does not exist. The session id is the file stem
/// (Claude names transcripts `<session>.jsonl`).
pub fn read(path: &Path) -> Result<Option<TranscriptExtract>> {
    if !path.is_file() {
        return Ok(None);
    }
    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut x = TranscriptExtract::default();
    let mut entrypoint: Option<String> = None;
    let mut line_session_id: Option<String> = None;
    let mut saw_sidechain_flag = false;
    let mut all_sidechain = true;
    let mut answers: Vec<Option<String>> = Vec::new();
    let mut prompts: Vec<String> = Vec::new();

    for_each_value(path, |line| {
        let kind = line.get("type").and_then(Value::as_str);

        if kind == Some("ai-title")
            && let Some(t) = line
                .get("aiTitle")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
        {
            x.title = Some(t.to_string());
            return;
        }

        let time = line
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        if let Some(ms) = time {
            x.first_at.get_or_insert(ms);
            x.last_at = Some(ms);
            x.activity_times.push(ms);
        }

        if x.cwd.is_none() {
            x.cwd = string(line.get("cwd"));
        }
        // "HEAD": outside git or detached, never a branch name.
        if let Some(b) = string(line.get("gitBranch")).filter(|b| b != "HEAD") {
            x.branch = Some(b);
        }
        if entrypoint.is_none() {
            entrypoint = string(line.get("entrypoint"));
        }
        if line_session_id.is_none() {
            line_session_id = string(line.get("sessionId"));
        }
        if let Some(flag) = line.get("isSidechain") {
            saw_sidechain_flag = true;
            all_sidechain = all_sidechain && flag == &Value::Bool(true);
        }

        let Some(message) = line.get("message").filter(|m| m.is_object()) else {
            return;
        };

        // isCompactSummary / isVisibleInTranscriptOnly: compaction summary, not a user prompt.
        if kind == Some("user")
            && ["isMeta", "isCompactSummary", "isVisibleInTranscriptOnly"]
                .iter()
                .all(|k| php_empty(line.get(*k)))
            && let Some(prompt) = prompt_text(message.get("content"))
        {
            prompts.push(prompt);
            answers.push(None);
        }

        if kind == Some("assistant") {
            if let Some(m) = string(message.get("model")).filter(|m| m != "<synthetic>") {
                x.model = Some(m);
            }
            let blocks: Vec<&Value> = match message.get("content") {
                Some(Value::Array(a)) => a.iter().collect(),
                Some(Value::Object(o)) => o.values().collect(),
                _ => Vec::new(),
            };
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let (Some(text), Some(last)) =
                            (string(block.get("text")), answers.last_mut())
                        {
                            *last = Some(text);
                        }
                    }
                    Some("tool_use") => {
                        x.tool_use_count += 1;
                        let name = block.get("name").and_then(Value::as_str);
                        if name.is_some_and(|n| FILE_TOOLS.contains(&n)) {
                            let input = block.get("input");
                            let field = |k| input.and_then(|i| i.get(k)).filter(|v| !v.is_null());
                            let file =
                                string(field("file_path").or_else(|| field("notebook_path")));
                            if let Some(path) = file {
                                if !x.files_touched.contains(&path) {
                                    x.files_touched.push(path.clone());
                                }
                                x.file_touches.push(FileTouch { path, at_ms: time });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    })?;

    x.turns = prompts
        .into_iter()
        .zip(answers)
        .map(|(prompt, response)| Turn { prompt, response })
        .collect();

    let sidechain = saw_sidechain_flag && all_sidechain;
    if let Some(parent) = subagent_parent(path) {
        x.origin = SessionOrigin::Subagent;
        x.parent_session_id = Some(parent);
    } else if sidechain {
        x.origin = SessionOrigin::Subagent;
        x.parent_session_id = line_session_id.filter(|id| *id != session_id);
    } else if let Some(ep) = entrypoint {
        if ep.starts_with("sdk") {
            x.origin = SessionOrigin::Automation;
        } else if !INTERACTIVE_ENTRYPOINTS.contains(&ep.as_str()) {
            x.unknown_origin = Some(ep);
        }
    }

    Ok(Some(x))
}

/// Parent session from a `/<parent>/subagents/` path segment (regex `#/([^/]+)/subagents/#`).
fn subagent_parent(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let parts: Vec<&str> = text.split('/').collect();
    (2..parts.len().saturating_sub(1))
        .find(|&i| parts[i] == "subagents" && !parts[i - 1].is_empty())
        .map(|i| parts[i - 1].to_string())
}

/// Text of a user prompt: a string, or the "text" blocks of an array joined by newlines.
/// Arrays with only tool_result (or images) are not prompts.
fn prompt_text(content: Option<&Value>) -> Option<String> {
    let joined;
    let text = match content? {
        Value::String(s) => s.as_str(),
        Value::Array(_) | Value::Object(_) => {
            let blocks: Vec<&Value> = match content? {
                Value::Array(a) => a.iter().collect(),
                Value::Object(o) => o.values().collect(),
                _ => unreachable!(),
            };
            let texts: Vec<&str> = blocks
                .into_iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            if texts.is_empty() {
                return None;
            }
            joined = texts.join("\n");
            joined.as_str()
        }
        _ => return None,
    };
    if text.trim_matches(PHP_WS).is_empty() {
        return None;
    }
    let lead = text.trim_start_matches(PHP_WS);
    if NOISE_PREFIXES.iter().any(|p| lead.starts_with(p)) {
        return None;
    }
    Some(text.trim_matches(PHP_WS).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const SESSION: &str = "11111111-aaaa-4bbb-8ccc-000000000001";
    const PROJ: &str = "/home/fulano/Code/Projeto Ação/Exemplo";

    fn write(dir: &Path, rel: &str, lines: &[Value]) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn user(extra: Value) -> Value {
        let mut base = json!({
            "type": "user", "timestamp": "2026-10-20T10:00:00.000Z", "cwd": "/tmp/x",
            "gitBranch": "main", "sessionId": "sessao-pai", "entrypoint": "cli",
            "isSidechain": false,
            "message": {"role": "user", "content": "Faça algo."},
        });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    fn base(extra: Value) -> Value {
        let mut b = json!({
            "isSidechain": false, "cwd": PROJ, "sessionId": SESSION,
            "entrypoint": "cli", "gitBranch": "main",
        });
        b.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        b
    }

    fn assistant(ts: &str, model: &str, content: Value, branch: &str) -> Value {
        base(
            json!({"type": "assistant", "timestamp": ts, "gitBranch": branch,
            "message": {"role": "assistant", "model": model, "content": content}}),
        )
    }

    fn user_msg(ts: &str, content: Value, branch: &str) -> Value {
        base(json!({"type": "user", "timestamp": ts, "gitBranch": branch,
            "message": {"role": "user", "content": content}}))
    }

    /// Mirrors tests/Fixtures/claude/session.jsonl, ending in a truncated line.
    fn fixture(dir: &Path) -> PathBuf {
        let p = |c: &str| path_of(c);
        fn path_of(c: &str) -> String {
            format!("{PROJ}/{c}")
        }
        let lines = vec![
            json!({"type": "permission-mode", "permissionMode": "default", "sessionId": SESSION}),
            user_msg(
                "2026-10-20T12:00:00.000Z",
                json!("Crie a página de contato com um formulário simples."),
                "main",
            ),
            base(
                json!({"type": "user", "isMeta": true, "timestamp": "2026-10-20T12:00:01.000Z",
                "message": {"role": "user", "content": [{"type": "text", "text": "Conteúdo injetado pelo harness, não é um prompt."}]}}),
            ),
            assistant(
                "2026-10-20T12:00:05.000Z",
                "claude-modelo-antigo",
                json!([
                    {"type": "thinking", "thinking": "pensando"},
                    {"type": "text", "text": "Vou criar o formulário."},
                    {"type": "tool_use", "name": "Write", "input": {"file_path": p("contato.html")}},
                ]),
                "main",
            ),
            user_msg(
                "2026-10-20T12:00:06.000Z",
                json!([{"type": "tool_result", "content": "Arquivo criado."}]),
                "main",
            ),
            assistant(
                "2026-10-20T12:00:08.000Z",
                "claude-modelo-antigo",
                json!([
                    {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
                ]),
                "main",
            ),
            user_msg(
                "2026-10-20T12:00:09.000Z",
                json!([{"type": "tool_result", "content": "contato.html"}]),
                "main",
            ),
            assistant(
                "2026-10-20T12:00:10.000Z",
                "claude-modelo-antigo",
                json!([
                    {"type": "text", "text": "Formulário criado em contato.html."},
                ]),
                "main",
            ),
            json!({"type": "ai-title", "aiTitle": "Título provisório", "sessionId": SESSION}),
            user_msg(
                "2026-10-20T12:05:00.000Z",
                json!([{"type": "text", "text": "Agora valide o e-mail no formulário."}]),
                "feat/contato",
            ),
            assistant(
                "2026-10-20T12:05:04.000Z",
                "claude-modelo-novo",
                json!([
                    {"type": "tool_use", "name": "Edit", "input": {"file_path": p("contato.html")}},
                    {"type": "tool_use", "name": "Edit", "input": {"file_path": p("valida.js")}},
                ]),
                "feat/contato",
            ),
            user_msg(
                "2026-10-20T12:05:05.000Z",
                json!([{"type": "tool_result", "content": "ok"}]),
                "feat/contato",
            ),
            assistant(
                "2026-10-20T12:05:30.000Z",
                "claude-modelo-novo",
                json!([
                    {"type": "text", "text": "Validação de e-mail adicionada."},
                ]),
                "feat/contato",
            ),
            json!({"type": "ai-title", "aiTitle": "Página de contato com validação", "sessionId": SESSION}),
            json!({"type": "last-prompt", "lastPrompt": "Agora valide o e-mail no formulário.", "sessionId": SESSION}),
        ];
        let path = write(dir, &format!("projeto/{SESSION}.jsonl"), &lines);
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str(r#"{"type":"assistant","message":{"role":"assistant","model":"claude-modelo-cortado","content":[{"type":"text","text":"Esta linha foi cor"#);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn origin_of(entrypoint: &str) -> TranscriptExtract {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "p/s1.jsonl",
            &[user(json!({"entrypoint": entrypoint, "sessionId": "s1"}))],
        );
        read(&path).unwrap().unwrap()
    }

    /// extrai prompts sem contar tool_result
    #[test]
    fn extracts_prompts_without_counting_tool_result() {
        let dir = tempfile::tempdir().unwrap();
        let x = read(&fixture(dir.path())).unwrap().unwrap();
        assert_eq!(
            x.turns,
            vec![
                Turn {
                    prompt: "Crie a página de contato com um formulário simples.".into(),
                    response: Some("Formulário criado em contato.html.".into()),
                },
                Turn {
                    prompt: "Agora valide o e-mail no formulário.".into(),
                    response: Some("Validação de e-mail adicionada.".into()),
                },
            ]
        );
    }

    /// Outside git Claude Code records `"gitBranch": "HEAD"` (seen on Windows in the home
    /// and `system32`); like a detached HEAD in `git::snapshot`, it is no branch.
    #[test]
    fn head_is_not_a_branch() {
        let dir = tempfile::tempdir().unwrap();
        let lines = [user_msg("2026-10-20T12:00:00.000Z", json!("Oi"), "HEAD")];
        let x = read(&write(dir.path(), "s.jsonl", &lines))
            .unwrap()
            .unwrap();
        assert_eq!(x.branch, None);
    }

    /// detecta arquivos tocados por Edit e Write
    #[test]
    fn detects_files_touched_by_edit_and_write() {
        let dir = tempfile::tempdir().unwrap();
        let x = read(&fixture(dir.path())).unwrap().unwrap();
        assert_eq!(
            x.files_touched,
            vec![format!("{PROJ}/contato.html"), format!("{PROJ}/valida.js")]
        );
        assert_eq!(x.tool_use_count, 4);
    }

    /// ignora linha JSONL truncada no final
    #[test]
    fn ignores_truncated_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture(dir.path());
        assert!(!std::fs::read_to_string(&path).unwrap().ends_with("}\n"));
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.model.as_deref(), Some("claude-modelo-novo"));
        assert_eq!(x.title.as_deref(), Some("Página de contato com validação"));
        assert_eq!(x.cwd.as_deref(), Some(PROJ));
        assert_eq!(x.branch.as_deref(), Some("feat/contato"));
        assert_eq!(x.first_at, parse_ts("2026-10-20T12:00:00Z"));
        assert_eq!(x.last_at, parse_ts("2026-10-20T12:05:30Z"));
        assert_eq!(x.origin, SessionOrigin::Interactive);
        assert_eq!(x.parent_session_id, None);
    }

    /// classifica sdk-cli como automation (sdk-cli, sdk-ts)
    #[test]
    fn classifies_sdk_entrypoints_as_automation() {
        for ep in ["sdk-cli", "sdk-ts"] {
            assert_eq!(origin_of(ep).origin, SessionOrigin::Automation, "{ep}");
        }
    }

    /// classifica claude-desktop como interactive
    #[test]
    fn classifies_claude_desktop_as_interactive() {
        let x = origin_of("claude-desktop");
        assert_eq!(x.origin, SessionOrigin::Interactive);
        assert_eq!(x.unknown_origin, None);
    }

    /// classifica isSidechain como subagent
    #[test]
    fn classifies_sidechain_as_subagent() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "p/agente-1.jsonl",
            &[
                user(json!({"isSidechain": true})),
                json!({"type": "assistant", "isSidechain": true, "sessionId": "sessao-pai",
                "timestamp": "2026-10-20T10:00:01.000Z",
                "message": {"role": "assistant", "model": "m", "content": [{"type": "text", "text": "Feito."}]}}),
            ],
        );
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.origin, SessionOrigin::Subagent);
        assert_eq!(x.parent_session_id.as_deref(), Some("sessao-pai"));
    }

    /// classifica transcrição em /subagents/ como subagent e usa a pasta como sessão-pai
    #[test]
    fn classifies_subagents_path_and_uses_folder_as_parent() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "projeto/sessao-pai-123/subagents/agent-a1.jsonl",
            &[user(json!({"sessionId": "outro"}))],
        );
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.origin, SessionOrigin::Subagent);
        assert_eq!(x.parent_session_id.as_deref(), Some("sessao-pai-123"));
    }

    /// localiza a transcrição pelo sessionId quando o caminho não é informado
    #[test]
    fn locates_transcript_by_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "projects/-home-fulano-proj/abc.jsonl",
            &[user(json!({"sessionId": "abc"}))],
        );
        let found = locate(dir.path(), "abc").unwrap();
        assert_eq!(found, path);
        assert_eq!(read(&found).unwrap().unwrap().turns[0].prompt, "Faça algo.");
        assert_eq!(locate(dir.path(), "nao-existe"), None);
        assert_eq!(locate(dir.path(), "../abc"), None);
        assert_eq!(locate(dir.path(), ""), None);
    }

    /// devolve null quando o arquivo não existe
    #[test]
    fn returns_none_when_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&dir.path().join("nada.jsonl")).unwrap(), None);
    }

    /// ignora resumos de compactação e linhas só da transcrição
    #[test]
    fn ignores_compact_summaries_and_transcript_only_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "p/sessao-pai.jsonl",
            &[
                user(json!({"message": {"role": "user", "content": "Pedido real."}})),
                user(json!({"isCompactSummary": true, "message": {"role": "user",
                "content": "This session is being continued from a previous conversation that ran out of context."}})),
                user(
                    json!({"isVisibleInTranscriptOnly": true, "message": {"role": "user",
                "content": [{"type": "text", "text": "Resumo visível só na transcrição."}]}}),
                ),
                user(json!({"message": {"role": "user", "content": "Segundo pedido real."}})),
            ],
        );
        let prompts: Vec<String> = read(&path)
            .unwrap()
            .unwrap()
            .turns
            .into_iter()
            .map(|t| t.prompt)
            .collect();
        assert_eq!(prompts, ["Pedido real.", "Segundo pedido real."]);
    }

    /// continua interactive e registra um warning só uma vez por valor
    #[test]
    fn unknown_entrypoint_stays_interactive_and_is_reported() {
        let x = origin_of("claude-novo-app");
        assert_eq!(x.origin, SessionOrigin::Interactive);
        assert_eq!(x.unknown_origin.as_deref(), Some("claude-novo-app"));
    }

    /// não registra warning para os entrypoints conhecidos nem para a ausência dele
    #[test]
    fn known_or_missing_entrypoints_are_not_reported() {
        for ep in [
            "cli",
            "claude-desktop",
            "claude-vscode",
            "sdk-cli",
            "sdk-ts",
            "sdk-py",
        ] {
            assert_eq!(origin_of(ep).unknown_origin, None, "{ep}");
        }
        let dir = tempfile::tempdir().unwrap();
        let mut line = user(json!({"sessionId": "s1"}));
        line.as_object_mut().unwrap().remove("entrypoint");
        let path = write(dir.path(), "p/s1.jsonl", &[line]);
        assert_eq!(read(&path).unwrap().unwrap().unknown_origin, None);
    }

    /// guarda o horário de cada evento para achar a primeira atividade depois de um instante
    #[test]
    fn keeps_event_times_for_first_activity_after() {
        let dir = tempfile::tempdir().unwrap();
        let x = read(&fixture(dir.path())).unwrap().unwrap();
        let after = parse_ts("2026-10-20T12:00:10Z").unwrap();
        assert_eq!(x.first_at_after(after), parse_ts("2026-10-20T12:05:00Z"));
    }

    /// guarda o horário de cada arquivo tocado (linha do tool_use)
    #[test]
    fn keeps_time_of_each_touched_file() {
        let dir = tempfile::tempdir().unwrap();
        let x = read(&fixture(dir.path())).unwrap().unwrap();
        let t = |s| parse_ts(s);
        assert_eq!(
            x.files_touched_between(t("2026-10-20T12:00:00Z"), t("2026-10-20T12:01:00Z")),
            vec![format!("{PROJ}/contato.html")]
        );
        assert_eq!(
            x.files_touched_between(t("2026-10-20T12:05:00Z"), None),
            vec![format!("{PROJ}/contato.html"), format!("{PROJ}/valida.js")]
        );
        assert!(
            x.files_touched_between(t("2026-10-20T12:05:30Z"), None)
                .is_empty()
        );
    }

    /// Turn count mirrors PHP: noise prefixes are dropped, replies attach to the last prompt only.
    #[test]
    fn turn_count_and_noise_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "p/s1.jsonl",
            &[
                user(json!({"message": {"role": "user", "content": "  <task-notification>x"}})),
                assistant(
                    "2026-10-20T10:00:01.000Z",
                    "<synthetic>",
                    json!([{"type": "text", "text": "órfã"}]),
                    "main",
                ),
                user(json!({"message": {"role": "user", "content": "  Primeiro  "}})),
                user(json!({"message": {"role": "user", "content": "Segundo"}})),
                assistant(
                    "2026-10-20T10:00:02.000Z",
                    "<synthetic>",
                    json!([{"type": "text", "text": "R"}]),
                    "main",
                ),
            ],
        );
        let x = read(&path).unwrap().unwrap();
        assert_eq!(x.model, None);
        assert_eq!(
            x.turns,
            vec![
                Turn {
                    prompt: "Primeiro".into(),
                    response: None
                },
                Turn {
                    prompt: "Segundo".into(),
                    response: Some("R".into())
                },
            ]
        );
        assert_eq!(
            x.fingerprint(),
            format!("{}:2:2", x.last_at.unwrap() / 1000)
        );
    }
}
