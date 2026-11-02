//! Picks the transcript reader of a tool. Port of `TranscriptReaderFactory.php`.

use std::path::Path;

use super::extract::TranscriptExtract;
use super::{claude, codex_history, codex_rollout};
use crate::Result;
use crate::model::Tool;
use crate::paths::Paths;

/// Claude: the JSONL (located under `claude_dir` when no path is known). Codex: the rollout when
/// the file exists and is not paginated; otherwise the paginated history databases.
pub fn read(
    tool: Tool,
    session_id: &str,
    transcript_path: Option<&Path>,
    paths: &Paths,
) -> Result<Option<TranscriptExtract>> {
    match tool {
        Tool::Claude => {
            let path = match transcript_path {
                Some(p) => Some(p.to_path_buf()),
                None => claude::locate(&paths.claude_dir, session_id),
            };
            match path {
                Some(p) => claude::read(&p),
                None => Ok(None),
            }
        }
        Tool::Codex => match transcript_path {
            Some(p) if p.is_file() && !codex_rollout::is_paginated(p) => codex_rollout::read(p),
            _ => codex_history::read(&paths.codex_home, session_id),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const MAIN: &str = "0199bbbb-0000-7000-8000-000000000001";

    struct Env {
        _dir: tempfile::TempDir,
        paths: Paths,
    }

    /// Paths over a temp dir whose `codex_home` holds the paginated fixture databases.
    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let paths = Paths::from_env(
            |k| (k == "CODEX_HOME").then(|| root.join("codex").into()),
            &root,
            &root,
        );
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        let sql = include_str!("../../tests/fixtures/codex/thread_history.sql");
        for section in sql.split("-- @database ").skip(1) {
            let (file, body) = section.split_once('\n').unwrap();
            rusqlite::Connection::open(paths.codex_home.join(file.trim()))
                .unwrap()
                .execute_batch(body)
                .unwrap();
        }
        Env { _dir: dir, paths }
    }

    fn rollout_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/rollout.jsonl")
    }

    /// usa o leitor do Claude para o Claude
    #[test]
    fn claude_uses_the_claude_reader() {
        let env = env();
        let project = env.paths.claude_dir.join("projects/-tmp-x");
        std::fs::create_dir_all(&project).unwrap();
        let file = project.join("sess-1.jsonl");
        std::fs::write(
            &file,
            "{\"type\":\"user\",\"timestamp\":\"2026-10-20T10:00:00Z\",\"cwd\":\"/tmp/x\",\"message\":{\"role\":\"user\",\"content\":\"Oi Claude\"}}\n",
        )
        .unwrap();

        // Located under claude_dir when no path is given, and read directly when it is.
        for given in [None, Some(file.as_path())] {
            let x = read(Tool::Claude, "sess-1", given, &env.paths)
                .unwrap()
                .unwrap();
            assert_eq!(x.turns[0].prompt, "Oi Claude");
        }
        assert_eq!(
            read(Tool::Claude, "ausente", None, &env.paths).unwrap(),
            None
        );
        // A Codex thread id is not a Claude transcript.
        assert_eq!(read(Tool::Claude, MAIN, None, &env.paths).unwrap(), None);
    }

    /// usa o rollout do Codex quando o JSONL existe e não é paginado
    #[test]
    fn codex_uses_the_rollout_when_jsonl_exists_and_is_not_paginated() {
        let env = env();
        let path = rollout_fixture();
        let expected = codex_rollout::read(&path).unwrap().unwrap();

        // Same session id as a thread in the databases: the rollout still wins.
        let x = read(Tool::Codex, MAIN, Some(&path), &env.paths)
            .unwrap()
            .unwrap();
        assert_eq!(x, expected);
    }

    /// usa o leitor paginado quando o JSONL não existe
    #[test]
    fn codex_uses_paginated_reader_when_jsonl_is_missing() {
        let env = env();
        let missing = env.paths.data_dir.join("nada.jsonl");

        for given in [None, Some(missing.as_path())] {
            let x = read(Tool::Codex, MAIN, given, &env.paths).unwrap().unwrap();
            assert_eq!(x.title.as_deref(), Some("Agenda com lembretes"));
        }
    }

    /// usa o leitor paginado quando o history_mode é paginated
    #[test]
    fn codex_uses_paginated_reader_when_history_mode_is_paginated() {
        let env = env();
        let path = env.paths.codex_home.join("rollout.jsonl");
        std::fs::write(
            &path,
            "{\"timestamp\":\"2026-10-20T15:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"/tmp/x\",\"source\":\"vscode\",\"history_mode\":\"paginated\"}}\n",
        )
        .unwrap();

        let x = read(Tool::Codex, MAIN, Some(&path), &env.paths)
            .unwrap()
            .unwrap();
        assert_eq!(x.title.as_deref(), Some("Agenda com lembretes"));
    }
}
