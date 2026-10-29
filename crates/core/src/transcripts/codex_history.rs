//! Codex sessions in paginated mode, read straight from the Codex SQLite databases
//! (Inv §3.7 "Codex paginado"): `state_5.sqlite` (`threads`) and `thread_history_1.sqlite`
//! (`thread_items` and, for desktop voice conversations, `thread_realtime_items`).
//! Both are opened read-only. Absent database or unknown thread: `None`; a database that exists
//! but cannot be read (locked, unexpected schema): `Error::TranscriptUnreadable`.
//!
//! `thread_items` and `thread_realtime_items` share the `rollout_ordinal` sequence, so their
//! messages are interleaved by it. Voice segments whose text is already in `thread_items`
//! (delegation, promoted item) are ignored. Port of `CodexThreadHistoryReader.php`.

use std::path::Path;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use super::codex_rollout::change_paths;
use super::codex_source::classify;
use super::extract::{FileTouch, TranscriptExtract, Turn};
use super::php::PHP_WS;
use crate::{Error, Result};

pub const CODEX_STATE_DB: &str = "state_5.sqlite";
pub const CODEX_HISTORY_DB: &str = "thread_history_1.sqlite";

/// Columns of `threads` the reader depends on; any missing one means an unexpected schema.
const THREAD_COLUMNS: [&str; 8] = [
    "source",
    "cwd",
    "title",
    "git_branch",
    "model",
    "created_at_ms",
    "updated_at_ms",
    "id",
];

#[derive(Debug)]
struct Message {
    ord: i64,
    is_user: bool,
    text: String,
}

fn unreadable(e: rusqlite::Error) -> Error {
    Error::TranscriptUnreadable(format!("Codex database unreadable: {e}"))
}

pub fn read(codex_home: &Path, thread_id: &str) -> Result<Option<TranscriptExtract>> {
    let state_path = codex_home.join(CODEX_STATE_DB);
    let history_path = codex_home.join(CODEX_HISTORY_DB);
    if !state_path.is_file() || !history_path.is_file() {
        return Ok(None);
    }
    read_inner(&state_path, &history_path, thread_id).map_err(unreadable_or_same)
}

/// Database failures are transient ("not a transcript" would be wrong): report them as unreadable.
fn unreadable_or_same(e: Error) -> Error {
    match e {
        Error::Db(e) => unreadable(e),
        other => other,
    }
}

fn read_inner(
    state_path: &Path,
    history_path: &Path,
    id: &str,
) -> Result<Option<TranscriptExtract>> {
    let thread = {
        let state = connect(state_path)?;
        match fetch_thread(&state, id)? {
            Some(thread) => thread,
            None => return Ok(None),
        }
    };

    let history = connect(history_path)?;
    let has_realtime = has_realtime_table(&history)?;
    let (messages, files, touches) = fetch_items(&history, id)?;
    let mut all = fetch_realtime(&history, id, &messages, has_realtime)?;
    let mut merged = messages;
    merged.append(&mut all);

    let (origin, parent, unknown) = classify(&thread.source);
    Ok(Some(TranscriptExtract {
        cwd: thread.cwd,
        branch: thread.branch,
        model: thread.model,
        title: thread.title,
        first_at: thread.created_at_ms,
        last_at: thread.updated_at_ms,
        origin,
        parent_session_id: parent,
        turns: turns(merged),
        files_touched: files,
        tool_use_count: count_tools(&history, id)?,
        activity_times: activity_times(&history, id, has_realtime)?,
        file_touches: touches,
        unknown_origin: unknown,
    }))
}

/// Opens a Codex database strictly read-only (URI `file:…?mode=ro`), `busy_timeout` 5 s.
pub(crate) fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        read_only_uri(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

fn read_only_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro");
    uri
}

struct Thread {
    source: Value,
    cwd: Option<String>,
    branch: Option<String>,
    model: Option<String>,
    title: Option<String>,
    created_at_ms: Option<i64>,
    updated_at_ms: Option<i64>,
}

/// Text value, non-empty (PHP `is_string && !== ''`).
fn text_of(v: ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Text(t) => {
            Some(String::from_utf8_lossy(t).into_owned()).filter(|s| !s.is_empty())
        }
        _ => None,
    }
}

/// PHP `is_numeric` + `(int)` on a value coming from SQLite.
fn numeric_of(v: ValueRef<'_>) -> Option<i64> {
    match v {
        ValueRef::Integer(i) => Some(i),
        ValueRef::Real(f) => Some(f as i64),
        ValueRef::Text(t) => {
            let s = String::from_utf8_lossy(t);
            let s = s.trim();
            s.parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().map(|f| f as i64))
        }
        _ => None,
    }
}

fn fetch_thread(conn: &Connection, id: &str) -> Result<Option<Thread>> {
    let mut stmt = conn.prepare("SELECT * FROM threads WHERE id = ?1 LIMIT 1")?;
    let columns: Vec<String> = stmt.column_names().map(str::to_string).collect();
    if let Some(missing) = THREAD_COLUMNS
        .iter()
        .find(|c| !columns.iter().any(|x| x == *c))
    {
        return Err(Error::TranscriptUnreadable(format!(
            "Codex database has an unexpected schema: threads.{missing} is missing"
        )));
    }
    let col = |name: &str| columns.iter().position(|c| c == name).unwrap_or(0);

    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let get = |name: &str| row.get_ref(col(name));
    let source = match get("source")? {
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        _ => Value::Null,
    };
    Ok(Some(Thread {
        source,
        cwd: text_of(get("cwd")?),
        branch: text_of(get("git_branch")?),
        model: text_of(get("model")?),
        title: text_of(get("title")?),
        created_at_ms: numeric_of(get("created_at_ms")?),
        updated_at_ms: numeric_of(get("updated_at_ms")?),
    }))
}

type Items = (Vec<Message>, Vec<String>, Vec<FileTouch>);

/// Messages (in order), changed files and every change with its time, from `thread_items`.
fn fetch_items(conn: &Connection, id: &str) -> Result<Items> {
    let mut stmt = conn.prepare(
        "SELECT rollout_ordinal, created_at_ms, item_type, item_json FROM thread_items \
         WHERE thread_id = ?1 AND item_type IN ('userMessage', 'agentMessage', 'fileChange') \
         ORDER BY rollout_ordinal",
    )?;
    let mut rows = stmt.query(params![id])?;

    let mut messages = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut touches = Vec::new();

    while let Some(row) = rows.next()? {
        let Some(item) = decode_json(row.get_ref(3)?) else {
            continue;
        };
        let ord = numeric_of(row.get_ref(0)?).unwrap_or(0);
        let kind = text_of(row.get_ref(2)?).unwrap_or_default();

        match kind.as_str() {
            "userMessage" => {
                let blocks: Vec<&Value> = match item.get("content") {
                    Some(Value::Array(a)) => a.iter().collect(),
                    Some(Value::Object(o)) => o.values().collect(),
                    _ => Vec::new(),
                };
                let texts: Vec<&str> = blocks
                    .into_iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect();
                let text = unwrap_delegation(texts.join("\n").trim_matches(PHP_WS));
                if !text.is_empty() {
                    messages.push(Message {
                        ord,
                        is_user: true,
                        text,
                    });
                }
            }
            "agentMessage" => {
                let text = item
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim_matches(PHP_WS);
                if !text.is_empty() {
                    messages.push(Message {
                        ord,
                        is_user: false,
                        text: text.to_string(),
                    });
                }
            }
            _ => {
                let at_ms = numeric_of(row.get_ref(1)?);
                for path in change_paths(item.get("changes")) {
                    if !files.contains(&path) {
                        files.push(path.clone());
                    }
                    touches.push(FileTouch { path, at_ms });
                }
            }
        }
    }
    Ok((messages, files, touches))
}

fn decode_json(v: ValueRef<'_>) -> Option<Value> {
    let ValueRef::Text(t) = v else { return None };
    match serde_json::from_slice::<Value>(t) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => Some(v),
        _ => None,
    }
}

fn has_realtime_table(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'thread_realtime_items')",
        [],
        |r| r.get(0),
    )?)
}

/// Transcribed voice segments, minus those repeating text already in `thread_items`.
/// Databases from older Codex versions lack the table.
fn fetch_realtime(
    conn: &Connection,
    id: &str,
    known: &[Message],
    has_table: bool,
) -> Result<Vec<Message>> {
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT rollout_ordinal, item_json FROM thread_realtime_items \
         WHERE thread_id = ?1 AND item_type = 'transcript_segment' ORDER BY rollout_ordinal",
    )?;
    let mut rows = stmt.query(params![id])?;
    let mut segments = Vec::new();

    while let Some(row) = rows.next()? {
        let Some(item) = decode_json(row.get_ref(1)?) else {
            continue;
        };
        let is_user = match item.get("role").and_then(Value::as_str) {
            Some("user") => true,
            Some("assistant") => false,
            _ => continue,
        };
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_matches(PHP_WS);
        if text.is_empty() || known.iter().any(|m| m.text == text) {
            continue;
        }
        segments.push(Message {
            ord: numeric_of(row.get_ref(0)?).unwrap_or(0),
            is_user,
            text: text.to_string(),
        });
    }
    Ok(segments)
}

/// Aligned turns: each prompt opens a turn; the last answer after it is the response. An answer
/// with no prompt before it (subagent) opens a turn with an empty prompt.
fn turns(mut messages: Vec<Message>) -> Vec<Turn> {
    messages.sort_by_key(|m| m.ord); // stable, like PHP 8 usort
    let mut turns: Vec<Turn> = Vec::new();
    for m in messages {
        match (m.is_user, turns.last_mut()) {
            (true, _) => turns.push(Turn {
                prompt: m.text,
                response: None,
            }),
            (false, Some(last)) => last.response = Some(m.text),
            (false, None) => turns.push(Turn {
                prompt: String::new(),
                response: Some(m.text),
            }),
        }
    }
    turns
}

/// Delegated prompt from the voice conversation: only `<input>` matters (the rest is the recent
/// transcript, which already comes from the segments).
fn unwrap_delegation(text: &str) -> String {
    if text.starts_with("<realtime_delegation>")
        && let Some(start) = text.find("<input>")
    {
        let rest = &text[start + "<input>".len()..];
        if let Some(end) = rest.find("</input>") {
            return rest[..end].trim_matches(PHP_WS).to_string();
        }
    }
    text.to_string()
}

/// `created_at_ms` of every item of the thread (text, tools and voice), ascending.
fn activity_times(conn: &Connection, id: &str, has_realtime: bool) -> Result<Vec<i64>> {
    let mut sql = String::from("SELECT created_at_ms FROM thread_items WHERE thread_id = ?1");
    if has_realtime {
        sql.push_str(
            " UNION ALL SELECT created_at_ms FROM thread_realtime_items WHERE thread_id = ?1",
        );
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT created_at_ms FROM ({sql}) ORDER BY created_at_ms"
    ))?;
    let mut rows = stmt.query(params![id])?;
    let mut times = Vec::new();
    while let Some(row) = rows.next()? {
        times.push(numeric_of(row.get_ref(0)?).unwrap_or(0));
    }
    Ok(times)
}

fn count_tools(conn: &Connection, id: &str) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM thread_items WHERE thread_id = ?1 AND item_type NOT IN \
         ('userMessage', 'agentMessage', 'reasoning', 'contextCompaction', 'subAgentActivity', 'sleep')",
        params![id],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionOrigin;
    use crate::time::parse_ts;
    use std::path::PathBuf;

    const MAIN: &str = "0199bbbb-0000-7000-8000-000000000001";

    /// Temporary `codex_home` built from `tests/fixtures/codex/thread_history.sql`: each section
    /// `-- @database <file>` runs on the matching SQLite file (path with a non-ASCII directory).
    struct Home {
        _dir: tempfile::TempDir,
        path: PathBuf,
    }

    impl Home {
        fn new() -> Home {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("código");
            std::fs::create_dir_all(&path).unwrap();
            let sql = include_str!("../../tests/fixtures/codex/thread_history.sql");
            let mut sections = sql.split("-- @database ").skip(1);
            for section in &mut sections {
                let (file, body) = section.split_once('\n').unwrap();
                Connection::open(path.join(file.trim()))
                    .unwrap()
                    .execute_batch(body)
                    .unwrap();
            }
            Home { _dir: dir, path }
        }

        fn db(&self, file: &str) -> Connection {
            Connection::open(self.path.join(file)).unwrap()
        }

        fn read(&self, id: &str) -> Result<Option<TranscriptExtract>> {
            read(&self.path, id)
        }

        fn extract(&self, id: &str) -> TranscriptExtract {
            self.read(id).unwrap().unwrap()
        }
    }

    fn t(v: &str) -> i64 {
        parse_ts(v).unwrap()
    }

    fn turn(prompt: &str, response: &str) -> Turn {
        Turn {
            prompt: prompt.into(),
            response: Some(response.into()),
        }
    }

    /// Codex paginado: fileChange extrai changes[].path e userMessage extrai o texto
    #[test]
    fn paginated_extracts_file_change_paths_and_user_message_text() {
        let x = Home::new().extract(MAIN);

        assert_eq!(
            x.turns,
            vec![
                turn("Crie a tela de lembretes.", "Tela de lembretes criada."),
                turn(
                    "Ajuste o horário padrão.",
                    "Horário padrão ajustado para 08:00."
                ),
            ]
        );
        assert_eq!(
            x.files_touched,
            vec![
                "/home/fulano/Code/Agenda Escolar/lembretes.py",
                "/home/fulano/Code/Agenda Escolar/app.py"
            ]
        );
        assert_eq!(x.tool_use_count, 4);
    }

    /// Codex paginado: lê os metadados da tabela threads
    #[test]
    fn paginated_reads_metadata_from_threads_table() {
        let x = Home::new().extract(MAIN);

        assert_eq!(x.cwd.as_deref(), Some("/home/fulano/Code/Agenda Escolar"));
        assert_eq!(x.branch.as_deref(), Some("main"));
        assert_eq!(x.model.as_deref(), Some("gpt-modelo-c"));
        assert_eq!(x.title.as_deref(), Some("Agenda com lembretes"));
        assert_eq!(x.first_at, Some(t("2026-10-20T13:00:00Z")));
        assert_eq!(x.last_at, Some(t("2026-10-20T13:05:00Z")));
        assert_eq!(x.origin, SessionOrigin::Interactive);
        assert_eq!(x.parent_session_id, None);
    }

    /// Codex: source exec é automation e o JSON de subagent preenche parentSessionId
    #[test]
    fn source_exec_is_automation_and_subagent_json_fills_parent() {
        let home = Home::new();
        let spawned = home.extract("0199bbbb-0000-7000-8000-000000000002");
        let guardian = home.extract("0199bbbb-0000-7000-8000-000000000003");
        let exec = home.extract("0199bbbb-0000-7000-8000-000000000004");

        assert_eq!(spawned.origin, SessionOrigin::Subagent);
        assert_eq!(spawned.parent_session_id.as_deref(), Some(MAIN));
        assert_eq!(spawned.title, None);
        assert_eq!(
            spawned.turns.last().unwrap().response.as_deref(),
            Some("Revisão concluída.")
        );
        assert_eq!(spawned.turns.len(), 1);
        assert_eq!(guardian.origin, SessionOrigin::Subagent);
        assert_eq!(guardian.parent_session_id, None);
        assert_eq!(exec.origin, SessionOrigin::Automation);
    }

    /// Codex paginado: abre as bases somente leitura
    #[test]
    fn paginated_opens_databases_read_only() {
        let home = Home::new();
        let files = [CODEX_STATE_DB, CODEX_HISTORY_DB];
        let before: Vec<Vec<u8>> = files
            .iter()
            .map(|f| std::fs::read(home.path.join(f)).unwrap())
            .collect();

        home.extract(MAIN);

        let after: Vec<Vec<u8>> = files
            .iter()
            .map(|f| std::fs::read(home.path.join(f)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert!(!home.path.join("state_5.sqlite-wal").exists());
    }

    /// Codex paginado: a conexão é mode=ro (escrever pela mesma URI lança)
    #[test]
    fn connection_is_mode_ro_and_writing_fails() {
        let home = Home::new();

        let state = connect(&home.path.join(CODEX_STATE_DB)).unwrap();
        let err = state
            .execute("UPDATE threads SET title = 'x'", [])
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("readonly"), "{err}");

        let history = connect(&home.path.join(CODEX_HISTORY_DB)).unwrap();
        let err = history
            .execute("CREATE TABLE intrusa (id INTEGER)", [])
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("readonly"), "{err}");
    }

    /// Codex paginado: devolve null para thread desconhecida ou base ausente
    #[test]
    fn unknown_thread_or_absent_database_gives_none() {
        let home = Home::new();
        assert_eq!(home.read("nao-existe").unwrap(), None);

        let empty = home.path.join("vazio");
        assert_eq!(read(&empty, MAIN).unwrap(), None);
        assert!(!empty.join(CODEX_STATE_DB).exists());
    }

    /// Codex paginado: resposta de subagente sem prompt é mantida, sem inventar prompt
    #[test]
    fn subagent_answer_without_prompt_is_kept_without_inventing_a_prompt() {
        let x = Home::new().extract("0199bbbb-0000-7000-8000-000000000003");

        assert_eq!(
            x.turns,
            vec![Turn {
                prompt: String::new(),
                response: Some("Nada suspeito no diff.".into())
            }]
        );
        let text = x.to_prompt_text(4000, None);
        assert!(!text.contains("[Usuário]"));
        assert!(text.contains("[Assistente] Nada suspeito no diff."));
    }

    /// Codex paginado: conversa por voz (thread_realtime_items) entra em ordem, sem duplicar a delegação
    #[test]
    fn voice_conversation_is_interleaved_without_duplicating_delegation() {
        let home = Home::new();
        let db = home.db(CODEX_HISTORY_DB);
        db.execute("DELETE FROM thread_items WHERE thread_id = ?1", [MAIN])
            .unwrap();

        let item = |ord: i64, json: &str, kind: &str| {
            db.execute(
                "INSERT INTO thread_items VALUES (?1, 't1', ?2, ?3, 0, ?4, ?5)",
                params![MAIN, format!("i{ord}"), ord, json, kind],
            )
            .unwrap();
        };
        let realtime = |ord: i64, kind: &str, json: &str| {
            db.execute(
                "INSERT INTO thread_realtime_items VALUES (?1, ?2, ?3, 0, ?4, ?5)",
                params![MAIN, format!("r{ord}"), ord, kind, json],
            )
            .unwrap();
        };
        let segment = |ord: i64, role: &str, text: &str| {
            let json = serde_json::json!({"id": format!("r{ord}"), "type": "transcript_segment", "role": role, "text": text});
            realtime(ord, "transcript_segment", &json.to_string());
        };

        realtime(
            1,
            "realtime_session_started",
            r#"{"type":"realtime_session_started"}"#,
        );
        segment(2, "user", " Você entende português?");
        segment(3, "assistant", " Sim, pode mandar.");
        item(
            12,
            &serde_json::json!({"type": "userMessage", "content": [{"type": "text", "text": "<realtime_delegation>\n  <input>Como ativo o modelo novo</input>\n  <transcript_delta>user:  Você entende português?</transcript_delta>\n</realtime_delegation>"}]}).to_string(),
            "userMessage",
        );
        segment(13, "user", " Como ativo o modelo novo");
        segment(14, "assistant", " Deixa eu ver.");
        item(
            15,
            &serde_json::json!({"type": "agentMessage", "text": "Abra o seletor de modelos."})
                .to_string(),
            "agentMessage",
        );
        realtime(
            16,
            "bem_item_promoted",
            r#"{"type":"bem_item_promoted","item_id":"i15"}"#,
        );
        segment(17, "assistant", "Abra o seletor de modelos.");
        realtime(
            18,
            "realtime_session_closed",
            r#"{"type":"realtime_session_closed"}"#,
        );
        drop(db);

        let x = home.extract(MAIN);
        assert_eq!(
            x.turns,
            vec![
                turn("Você entende português?", "Sim, pode mandar."),
                turn("Como ativo o modelo novo", "Abra o seletor de modelos."),
            ]
        );
    }

    /// Codex paginado: base sem thread_realtime_items (Codex antigo) continua funcionando
    #[test]
    fn database_without_realtime_table_still_works() {
        let home = Home::new();
        home.db(CODEX_HISTORY_DB)
            .execute_batch("DROP TABLE thread_realtime_items")
            .unwrap();

        let x = home.extract(MAIN);
        let prompts: Vec<&str> = x.turns.iter().map(|t| t.prompt.as_str()).collect();
        assert_eq!(
            prompts,
            ["Crie a tela de lembretes.", "Ajuste o horário padrão."]
        );
    }

    /// Codex paginado: a primeira atividade depois de um instante vem do created_at_ms dos itens
    #[test]
    fn first_activity_after_instant_comes_from_item_created_at() {
        let x = Home::new().extract(MAIN);
        assert_eq!(
            x.first_at_after(t("2026-10-20T13:00:07Z")),
            Some(t("2026-10-20T13:03:20Z"))
        );
    }

    /// Codex paginado: base ilegível lança TranscriptUnreadableException; thread inexistente devolve null
    #[test]
    fn unreadable_database_is_an_error_but_unknown_thread_is_none() {
        let home = Home::new();
        assert_eq!(home.read("thread-que-nao-existe").unwrap(), None);

        std::fs::write(home.path.join(CODEX_HISTORY_DB), "lixo".repeat(1024)).unwrap();

        assert!(matches!(
            home.read(MAIN),
            Err(Error::TranscriptUnreadable(_))
        ));
    }

    /// Codex paginado: guarda o created_at_ms de cada fileChange
    #[test]
    fn keeps_created_at_of_each_file_change() {
        let x = Home::new().extract(MAIN);
        let lembretes = "/home/fulano/Code/Agenda Escolar/lembretes.py";
        let app = "/home/fulano/Code/Agenda Escolar/app.py";

        assert_eq!(
            x.files_touched_between(Some(t("2026-10-20T13:03:00Z")), None),
            vec![app]
        );
        assert_eq!(
            x.files_touched_between(None, Some(t("2026-10-20T13:01:00Z"))),
            vec![lembretes, app]
        );
    }

    /// Esquema inesperado (threads sem updated_at_ms, tabela ausente) é erro, não "sem transcrição".
    #[test]
    fn unexpected_schema_is_an_error() {
        let home = Home::new();
        home.db(CODEX_STATE_DB)
            .execute_batch("ALTER TABLE threads DROP COLUMN updated_at_ms")
            .unwrap();
        assert!(matches!(
            home.read(MAIN),
            Err(Error::TranscriptUnreadable(_))
        ));

        let home = Home::new();
        home.db(CODEX_HISTORY_DB)
            .execute_batch("DROP TABLE thread_items")
            .unwrap();
        assert!(matches!(
            home.read(MAIN),
            Err(Error::TranscriptUnreadable(_))
        ));
    }

    /// Fingerprint v1 `{last_s}:{turns}:{turns}`: respostas vazias não mudam a contagem.
    #[test]
    fn fingerprint_matches_v1_shape() {
        let x = Home::new().extract(MAIN);
        assert_eq!(x.fingerprint(), "1792501500:2:2");
    }
}
