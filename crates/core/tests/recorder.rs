//! Port of Laravel `tests/Feature/Sessions/IngestTest.php` (cases 1–10, 12–24, 27, 28, 33–38),
//! calling `Recorder::record` directly with `HookEvent`s.

mod support;

use aiannotations_core::model::Tool;
use aiannotations_core::transcripts::factory;
use serde_json::json;
use support::*;

const C: Tool = Tool::Claude;
const X: Tool = Tool::Codex;

/// start + end criam uma sessão com uma execução fechada e ended_cleanly true para prompt_input_exit
#[test]
fn start_and_end_create_session_with_closed_clean_run() {
    let mut env = Env::new();
    let cwd = env.repo("proj");
    let path = env.transcript("s1", &[]);

    env.record(&start(
        C,
        "s1",
        &cwd,
        Some(&path),
        "startup",
        "2026-10-20T12:00:00Z",
    ));
    env.record(&end(
        C,
        "s1",
        &cwd,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T12:30:00Z",
    ));

    let s = sole_session(&env.conn);
    let run = sole_run(&env.conn);
    let (key, name, _, project_activity) = project(&env.conn, s.project_id);

    assert_eq!(s.tool, "claude");
    assert_eq!(s.cwd, cwd.to_string_lossy());
    assert_eq!(s.transcript_path.as_deref(), Some(&*path.to_string_lossy()));
    assert_eq!(s.model.as_deref(), Some("claude-modelo-novo"));
    assert_eq!(s.origin, "interactive");
    assert!(!s.is_empty);
    assert_eq!(key, cwd.to_string_lossy());
    assert_eq!(name, "proj");
    assert_eq!(run.started_at, ts("2026-10-20T12:00:00Z"));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:30:00Z")));
    assert_eq!(run.end_reason.as_deref(), Some("prompt_input_exit"));
    assert_eq!(run.ended_cleanly, Some(true));
    assert_eq!(run.branch.as_deref(), Some("main"));
    assert!(run.head_start.is_some());
    assert_eq!(run.head_end, run.head_start);
    assert_eq!(run.status_hash_end, run.status_hash_start);
    assert_eq!(run.changed_files, Some(true));
    assert!(run.files_touched.unwrap().contains(&CONTATO.to_string()));
    assert_eq!(run.summary_status, "pending");
    assert_eq!(s.last_activity_at, ts("2026-10-20T12:30:00Z"));
    assert_eq!(project_activity, Some(s.last_activity_at));
    // Timestamps are kept (ms) for the optimistic updates of later plans.
    assert!(run.created_at.is_some() && run.updated_at >= run.created_at);
}

/// end Claude com reason other fica ended_cleanly false
#[test]
fn claude_end_with_reason_other_is_not_clean() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("s2", &[]);
    env.record(&start(
        C,
        "s2",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T12:00:00Z",
    ));
    env.record(&end(
        C,
        "s2",
        &base,
        Some(&path),
        "other",
        "2026-10-20T12:10:00Z",
    ));

    let run = sole_run(&env.conn);
    assert_eq!(run.end_reason.as_deref(), Some("other"));
    assert_eq!(run.ended_cleanly, Some(false));
}

/// motivo desconhecido vira Other e o Codex fica sempre ended_cleanly true
#[test]
fn unknown_reason_becomes_other_and_codex_is_always_clean() {
    let mut env = Env::new();
    let base = env.base.clone();
    env.record(&end(
        C,
        "c1",
        &base,
        None,
        "bypass_permissions_disabled",
        "2026-10-20T12:10:00Z",
    ));
    let mut codex_start = start(X, "x1", &base, None, "startup", "2026-10-20T12:00:00Z");
    codex_start.model = Some("gpt-5.5".into());
    env.record(&codex_start);
    env.record(&end(X, "x1", &base, None, "other", "2026-10-20T12:20:00Z"));

    let claude = session_by_sid(&env.conn, "c1");
    let claude_run = runs(&env.conn, &format!("ai_session_id = {}", claude.id));
    let codex = session_by_sid(&env.conn, "x1");
    let codex_run = runs(&env.conn, &format!("ai_session_id = {}", codex.id));

    assert_eq!(claude_run.len(), 1);
    assert_eq!(claude_run[0].end_reason.as_deref(), Some("other"));
    assert_eq!(claude_run[0].ended_cleanly, Some(false));
    assert_eq!(codex.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(codex_run.len(), 1);
    assert_eq!(codex_run[0].ended_cleanly, Some(true));
    assert_eq!(codex_run[0].end_reason.as_deref(), Some("other"));
}

/// resume gera segunda execução na mesma sessão
#[test]
fn resume_creates_second_run_in_same_session() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("s3", &[]);
    let p = Some(path.as_path());
    env.record(&start(C, "s3", &base, p, "startup", "2026-10-19T10:00:00Z"));
    env.record(&end(
        C,
        "s3",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-19T11:00:00Z",
    ));
    env.record(&start(C, "s3", &base, p, "resume", "2026-10-20T09:00:00Z"));
    env.record(&end(
        C,
        "s3",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T09:30:00Z",
    ));

    let s = sole_session(&env.conn);
    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .all(|r| r.ended_at.is_some() && r.ended_cleanly == Some(true))
    );
    assert_eq!(all[1].summary_status, "pending");
    let latest = aiannotations_core::model::latest_run_id(&env.conn, s.id).unwrap();
    assert_eq!(latest, Some(all[1].id));
}

/// sessão retomada sem edição: a 2ª execução fica sem arquivos tocados
#[test]
fn resumed_session_without_edits_has_second_run_without_files() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("s-retomada", &[]);
    let p = Some(path.as_path());
    env.record(&start(C, "sr", &base, p, "startup", "2026-10-20T12:00:00Z"));
    env.record(&end(
        C,
        "sr",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T12:30:00Z",
    ));

    // Resumed with conversation only, no Edit/Write.
    append_line(
        &path,
        &claude_user(
            "sr",
            "Explique o que foi feito.",
            "2026-10-20T13:01:00.000Z",
        ),
    );
    env.record(&start(C, "sr", &base, p, "resume", "2026-10-20T13:00:00Z"));
    env.record(&end(
        C,
        "sr",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T13:10:00Z",
    ));

    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].changed_files, Some(true));
    assert!(
        all[0]
            .files_touched
            .as_ref()
            .unwrap()
            .contains(&CONTATO.to_string())
    );
    assert_eq!(all[1].changed_files, Some(false));
    assert_eq!(all[1].files_touched, Some(vec![]));
}

/// compactação não cria execução nova
#[test]
fn compaction_does_not_create_new_run() {
    let mut env = Env::new();
    let base = env.base.clone();
    env.record(&start(
        C,
        "cp",
        &base,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&start(
        C,
        "cp",
        &base,
        None,
        "compact",
        "2026-10-20T10:40:00Z",
    ));

    let s = sole_session(&env.conn);
    let run = sole_run(&env.conn);
    assert_eq!(run.ended_at, None);
    assert_eq!(s.last_activity_at, ts("2026-10-20T10:40:00Z"));
}

/// clear encadeia previous_session_id dentro de 120 s e não encadeia aos 300 s
#[test]
fn clear_chains_within_120s_and_not_at_300s() {
    let mut env = Env::new();
    let cwd = env.base.clone();
    env.record(&start(
        C,
        "a",
        &cwd,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(C, "a", &cwd, None, "clear", "2026-10-20T10:30:00Z"));
    env.record(&start(C, "b", &cwd, None, "clear", "2026-10-20T10:31:00Z"));
    env.record(&end(C, "b", &cwd, None, "clear", "2026-10-20T11:00:00Z"));
    env.record(&start(C, "c", &cwd, None, "clear", "2026-10-20T11:05:00Z"));
    // Another tool in the same cwd does not join the chain.
    env.record(&start(X, "d", &cwd, None, "clear", "2026-10-20T10:30:30Z"));

    let a = session_by_sid(&env.conn, "a");
    let b = session_by_sid(&env.conn, "b");
    assert_eq!(b.previous_session_id, Some(a.id));
    assert_eq!(session_by_sid(&env.conn, "c").previous_session_id, None);
    assert_eq!(session_by_sid(&env.conn, "d").previous_session_id, None);
    assert_eq!(a.previous_session_id, None);
}

/// clear encadeia mesmo quando o start da nova sessão chega antes do end da anterior
#[test]
fn clear_chains_even_when_new_start_arrives_before_previous_end() {
    let mut env = Env::new();
    let cwd = env.base.clone();
    env.record(&start(
        C,
        "a",
        &cwd,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    // Inverted order: b's SessionStart(clear) queued before a's SessionEnd(clear).
    env.record(&start(C, "b", &cwd, None, "clear", "2026-10-20T10:30:00Z"));
    env.record(&end(C, "a", &cwd, None, "clear", "2026-10-20T10:30:01Z"));
    // A session started without /clear and one that started outside the window do not join.
    env.record(&start(
        C,
        "x",
        &cwd,
        None,
        "startup",
        "2026-10-20T11:00:00Z",
    ));
    env.record(&start(C, "c", &cwd, None, "clear", "2026-10-20T11:00:00Z"));
    env.record(&end(C, "x", &cwd, None, "clear", "2026-10-20T11:05:00Z"));

    let a = session_by_sid(&env.conn, "a");
    assert_eq!(
        session_by_sid(&env.conn, "b").previous_session_id,
        Some(a.id)
    );
    assert_eq!(a.previous_session_id, None);
    assert_eq!(session_by_sid(&env.conn, "c").previous_session_id, None);
    assert_eq!(session_by_sid(&env.conn, "x").previous_session_id, None);
}

/// end clear não reencadeia sessão que já tem anterior
#[test]
fn clear_end_does_not_rechain_session_that_has_previous() {
    let mut env = Env::new();
    let cwd = env.base.clone();
    env.record(&end(C, "a", &cwd, None, "clear", "2026-10-20T10:00:00Z"));
    env.record(&start(C, "b", &cwd, None, "clear", "2026-10-20T10:00:01Z"));
    // Another session in the same cwd closes with /clear right after: b stays linked to a.
    env.record(&end(C, "z", &cwd, None, "clear", "2026-10-20T10:00:30Z"));

    assert_eq!(
        session_by_sid(&env.conn, "b").previous_session_id,
        Some(session_by_sid(&env.conn, "a").id)
    );
}

/// worktree agrupa no mesmo projeto
#[test]
fn worktree_groups_into_same_project() {
    let mut env = Env::new();
    let repo = env.repo("main-repo");
    git(
        &repo,
        &["worktree", "add", "-q", "../wt-feat", "-b", "feat/x"],
    );
    let wt = aiannotations_core::paths::real_path(&env.base.join("wt-feat")).unwrap();

    env.record(&start(
        C,
        "m",
        &repo,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&start(X, "w", &wt, None, "startup", "2026-10-20T10:01:00Z"));

    let w = session_by_sid(&env.conn, "w");
    let (key, name, _, _) = project(&env.conn, w.project_id);
    assert_eq!(count(&env.conn, "projects"), 1);
    assert_eq!(key, repo.to_string_lossy());
    assert_eq!(name, "main-repo");
    assert_eq!(w.worktree_path.as_deref(), Some(&*wt.to_string_lossy()));
    let w_runs = runs(&env.conn, &format!("ai_session_id = {}", w.id));
    assert_eq!(w_runs.len(), 1);
    assert_eq!(w_runs[0].branch.as_deref(), Some("feat/x"));
    let m = session_by_sid(&env.conn, "m");
    assert_eq!(m.project_id, w.project_id);
    assert_eq!(m.worktree_path, None);
}

/// cwd inexistente registra sem git
#[test]
fn nonexistent_cwd_records_without_git() {
    let mut env = Env::new();
    let cwd = env.base.join("apagado/projeto-velho");
    env.record(&start(
        C,
        "g",
        &cwd,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(
        C,
        "g",
        &cwd,
        None,
        "prompt_input_exit",
        "2026-10-20T10:20:00Z",
    ));

    let s = sole_session(&env.conn);
    let run = sole_run(&env.conn);
    let (key, name, remote, _) = project(&env.conn, s.project_id);
    assert_eq!(key, cwd.to_string_lossy());
    assert_eq!(name, "projeto-velho");
    assert_eq!(remote, None);
    assert_eq!(run.branch, None);
    assert_eq!(run.head_start, None);
    assert_eq!(run.head_end, None);
    assert!(run.ended_at.is_some());
    assert_eq!(run.changed_files, None);
}

/// cwd com espaço e acento
#[test]
fn cwd_with_space_and_accent() {
    let mut env = Env::new();
    let cwd = env.repo("Grupo Sentia/Condomínios/Bayside");
    let path = env.transcript("acento", &[]);
    env.record(&start(
        C,
        "ac",
        &cwd,
        Some(&path),
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(
        C,
        "ac",
        &cwd,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T10:20:00Z",
    ));

    let s = sole_session(&env.conn);
    let run = sole_run(&env.conn);
    let (key, name, _, _) = project(&env.conn, s.project_id);
    assert_eq!(s.cwd, cwd.to_string_lossy());
    assert_eq!(key, cwd.to_string_lossy());
    assert_eq!(name, "Bayside");
    assert_eq!(run.branch.as_deref(), Some("main"));
    assert!(run.head_end.is_some());
}

/// sessão sdk-cli fica summary_status Skipped
#[test]
fn sdk_cli_session_is_skipped() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript(
        "sdk",
        &[("\"entrypoint\":\"cli\"", "\"entrypoint\":\"sdk-cli\"")],
    );
    env.record(&start(
        C,
        "sdk",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(
        C,
        "sdk",
        &base,
        Some(&path),
        "other",
        "2026-10-20T10:20:00Z",
    ));

    assert_eq!(sole_session(&env.conn).origin, "automation");
    assert_eq!(sole_run(&env.conn).summary_status, "skipped");
    let visible = count(
        &env.conn,
        &format!(
            "ai_sessions WHERE {}",
            aiannotations_core::model::VISIBLE_SESSION_SQL
        ),
    );
    assert_eq!(visible, 0);
}

/// sessão sem prompts do usuário fica vazia e Skipped
#[test]
fn session_without_user_prompts_is_empty_and_skipped() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = base.join("transcripts/vazia.jsonl");
    write_jsonl(
        &path,
        &[json!({"type": "permission-mode", "sessionId": "v"})],
    );
    env.record(&end(
        C,
        "v",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T10:20:00Z",
    ));

    assert!(sole_session(&env.conn).is_empty);
    assert_eq!(sole_run(&env.conn).summary_status, "skipped");
}

/// sessão curta, com um prompt e sem ferramentas, não é vazia e vai para o resumo
#[test]
fn short_session_with_one_prompt_is_not_empty_and_goes_to_summary() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = base.join("transcripts/curta.jsonl");
    write_jsonl(
        &path,
        &[
            claude_user(
                "c",
                "Qual a versão do PHP aqui?",
                "2026-10-20T10:00:00.000Z",
            ),
            claude_assistant("c", "m", "PHP 8.4.", "2026-10-20T10:00:17.000Z"),
        ],
    );
    env.record(&start(
        C,
        "c",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(
        C,
        "c",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T10:00:17Z",
    ));

    assert!(!sole_session(&env.conn).is_empty);
    assert_eq!(sole_run(&env.conn).summary_status, "pending");
}

/// end sem start cria a execução
#[test]
fn end_without_start_creates_the_run() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("sem-start", &[]);
    env.record(&end(
        C,
        "ss",
        &base,
        Some(&path),
        "logout",
        "2026-10-20T12:45:00Z",
    ));

    let run = sole_run(&env.conn);
    assert_eq!(run.started_at, ts("2026-10-20T12:00:00Z"));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:45:00Z")));
    assert_eq!(run.end_reason.as_deref(), Some("logout"));
    assert_eq!(run.ended_cleanly, Some(true));
    assert_eq!(run.head_start, None);
}

/// end sem start depois de execução fechada começa na primeira atividade posterior a ela
#[test]
fn end_without_start_after_closed_run_starts_at_first_later_activity() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("sem-start-2", &[]);
    let p = Some(path.as_path());
    env.record(&start(
        C,
        "ss2",
        &base,
        p,
        "startup",
        "2026-10-20T12:00:00Z",
    ));
    env.record(&end(
        C,
        "ss2",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T12:30:00Z",
    ));

    // Resume whose SessionStart was lost: only the end arrives.
    append_line(
        &path,
        &claude_user("ss2", "Mais uma coisa.", "2026-10-20T13:00:00.000Z"),
    );
    env.record(&end(
        C,
        "ss2",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T13:10:00Z",
    ));

    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].ended_at, Some(ts("2026-10-20T12:30:00Z")));
    assert_eq!(all[1].started_at, ts("2026-10-20T13:00:00Z"));
    assert_eq!(all[1].ended_at, Some(ts("2026-10-20T13:10:00Z")));
}

/// SessionEnd duplicado em até 60 s é ignorado; outro motivo ou mais tarde sintetiza
#[test]
fn duplicate_end_within_60s_is_ignored_other_reason_or_later_synthesizes() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("dup", &[]);
    let p = Some(path.as_path());
    env.record(&start(
        C,
        "dup",
        &base,
        p,
        "startup",
        "2026-10-20T12:00:00Z",
    ));
    env.record(&end(
        C,
        "dup",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T12:30:00Z",
    ));
    env.record(&end(
        C,
        "dup",
        &base,
        p,
        "prompt_input_exit",
        "2026-10-20T12:30:40Z",
    ));

    assert_eq!(
        sole_run(&env.conn).ended_at,
        Some(ts("2026-10-20T12:30:00Z"))
    );

    // A different reason right after is not a duplicate.
    env.record(&end(C, "dup", &base, p, "logout", "2026-10-20T12:30:50Z"));
    assert_eq!(count(&env.conn, "session_runs"), 2);

    // Same reason, but 2 min later: not a duplicate either.
    env.record(&end(C, "dup", &base, p, "logout", "2026-10-20T12:33:00Z"));
    assert_eq!(count(&env.conn, "session_runs"), 3);
}

/// título longo é cortado em 200 caracteres
#[test]
fn long_title_is_cut_at_200_chars() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("titulo", &[]);
    append_line(
        &path,
        &json!({"type": "ai-title", "aiTitle": "ção ".repeat(400)}),
    );
    env.record(&end(
        C,
        "t",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T12:45:00Z",
    ));

    assert_eq!(sole_session(&env.conn).title.unwrap().chars().count(), 200);
}

/// end real substitui a marca idle do coletor
#[test]
fn real_end_replaces_collector_idle_mark() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("idle", &[]);
    let s = insert_session(&env.conn, C, "idl", &base, None);
    let run = insert_run(
        &env.conn,
        s,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:10:00Z")),
            end_reason: Some("idle"),
            ended_cleanly: Some(false),
            ..Default::default()
        },
    );

    env.record(&end(
        C,
        "idl",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T13:00:00Z",
    ));

    let r = sole_run(&env.conn);
    assert_eq!(r.id, run);
    assert_eq!(r.end_reason.as_deref(), Some("prompt_input_exit"));
    assert_eq!(r.ended_cleanly, Some(true));
    assert_eq!(r.ended_at, Some(ts("2026-10-20T13:00:00Z")));
}

/// start reabre a execução idle quando a transcrição mostra atividade depois dela
#[test]
fn start_reopens_idle_run_when_transcript_shows_later_activity() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("reabre", &[]);
    let s = insert_session(&env.conn, C, "rb", &base, Some(&path));
    let idle = insert_run(
        &env.conn,
        s,
        RunSeed {
            started_at: ts("2026-10-20T11:00:00Z"),
            // The fixture has activity until 12:05:30, after this mark.
            ended_at: Some(ts("2026-10-20T11:30:00Z")),
            end_reason: Some("idle"),
            ended_cleanly: Some(false),
            ..Default::default()
        },
    );

    env.record(&start(
        C,
        "rb",
        &base,
        Some(&path),
        "resume",
        "2026-10-20T13:00:00Z",
    ));

    let r = sole_run(&env.conn);
    assert_eq!(r.id, idle);
    assert_eq!(r.ended_at, None);
    assert_eq!(r.end_reason, None);
    assert_eq!(r.ended_cleanly, None);
}

/// start cria execução nova quando a idle não teve atividade depois
#[test]
fn start_creates_new_run_when_idle_had_no_later_activity() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("nova", &[]);
    let s = insert_session(&env.conn, C, "nv", &base, Some(&path));
    insert_run(
        &env.conn,
        s,
        RunSeed {
            started_at: ts("2026-10-20T11:00:00Z"),
            ended_at: Some(ts("2026-10-20T14:00:00Z")),
            end_reason: Some("idle"),
            ended_cleanly: Some(false),
            ..Default::default()
        },
    );

    env.record(&start(
        C,
        "nv",
        &base,
        Some(&path),
        "resume",
        "2026-10-20T15:00:00Z",
    ));

    assert_eq!(count(&env.conn, "session_runs"), 2);
    let latest = aiannotations_core::model::latest_run_id(&env.conn, s)
        .unwrap()
        .unwrap();
    assert_eq!(runs(&env.conn, &format!("id = {latest}"))[0].ended_at, None);
}

/// alteração só no git conta quando não há sessão paralela no projeto
#[test]
fn git_only_change_counts_without_parallel_session_in_project() {
    let mut env = Env::new();
    let cwd = env.repo("paralelo");
    let path = env.transcript(
        "sem-arquivos",
        &[
            ("\"name\":\"Write\"", "\"name\":\"Read\""),
            ("\"name\":\"Edit\"", "\"name\":\"Grep\""),
        ],
    );
    let p = Some(path.as_path());

    // Session alone: the status changes between start and end.
    env.record(&start(
        C,
        "solo",
        &cwd,
        p,
        "startup",
        "2026-10-20T08:00:00Z",
    ));
    std::fs::write(cwd.join("novo.txt"), "x").unwrap();
    env.record(&end(
        C,
        "solo",
        &cwd,
        p,
        "prompt_input_exit",
        "2026-10-20T08:30:00Z",
    ));

    // Two overlapping sessions: the status changes, but it cannot be attributed.
    env.record(&start(C, "p1", &cwd, p, "startup", "2026-10-20T10:00:00Z"));
    env.record(&start(
        X,
        "p2",
        &cwd,
        None,
        "startup",
        "2026-10-20T10:05:00Z",
    ));
    std::fs::write(cwd.join("outro.txt"), "y").unwrap();
    env.record(&end(
        C,
        "p1",
        &cwd,
        p,
        "prompt_input_exit",
        "2026-10-20T10:30:00Z",
    ));

    let solo = session_by_sid(&env.conn, "solo");
    let p1 = session_by_sid(&env.conn, "p1");
    let solo_run = &runs(&env.conn, &format!("ai_session_id = {}", solo.id))[0];
    let p1_run = &runs(&env.conn, &format!("ai_session_id = {}", p1.id))[0];
    assert_eq!(solo_run.changed_files, Some(true));
    assert_eq!(p1_run.changed_files, Some(false));
    assert_eq!(p1_run.files_touched, Some(vec![]));
}

/// voltar a Pending zera tentativas e espera do resumo
#[test]
fn back_to_pending_resets_attempts_and_wait() {
    for from in ["failed", "done"] {
        let mut env = Env::new();
        let base = env.base.clone();
        let path = env.transcript("retoma", &[]);
        let s = insert_session(&env.conn, C, "rt", &base, Some(&path));
        let run = insert_run(
            &env.conn,
            s,
            RunSeed {
                started_at: ts("2026-10-20T12:00:00Z"),
                ended_at: Some(ts("2026-10-20T12:30:00Z")),
                summary_status: Some(from),
                summary_attempts: 5,
                summary_available_at: Some(now_ms() + 3_600_000),
                ..Default::default()
            },
        );

        env.recorder()
            .refresh_from_transcript(s, None, now_ms())
            .unwrap();

        let r = &runs(&env.conn, &format!("id = {run}"))[0];
        assert_eq!(r.summary_status, "pending", "from {from}");
        assert_eq!(r.summary_attempts, 0, "from {from}");
        assert_eq!(r.summary_available_at, None, "from {from}");
    }
}

/// execução que já estava Pending mantém o backoff do resumo
#[test]
fn run_already_pending_keeps_summary_backoff() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("pendente", &[]);
    let s = insert_session(&env.conn, C, "pd", &base, Some(&path));
    let available_at = (now_ms() + 8 * 60_000).div_euclid(1000) * 1000;
    let run = insert_run(
        &env.conn,
        s,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:30:00Z")),
            summary_status: Some("pending"),
            summary_attempts: 3,
            summary_available_at: Some(available_at),
            ..Default::default()
        },
    );

    env.recorder()
        .refresh_from_transcript(s, None, now_ms())
        .unwrap();

    let r = &runs(&env.conn, &format!("id = {run}"))[0];
    assert_eq!(r.summary_status, "pending");
    assert_eq!(r.summary_attempts, 3);
    assert_eq!(r.summary_available_at, Some(available_at));
}

/// start_source é gravado também quando a descoberta registrou a sessão antes do start (clear invertido)
#[test]
fn start_source_recorded_when_discovery_registered_session_first() {
    let mut env = Env::new();
    let cwd = env.base.clone();
    env.record(&start(
        C,
        "a",
        &cwd,
        None,
        "startup",
        "2026-10-20T11:00:00Z",
    ));

    // The collector finds b (transcript from 12:00:00Z to 12:05:30Z) before its start is ingested.
    let path = env.transcript("b", &[]);
    insert_discovered(
        &env.conn,
        "b",
        &cwd,
        &path,
        ts("2026-10-20T12:00:00Z"),
        false,
    );
    assert_eq!(session_by_sid(&env.conn, "b").start_source, None);

    // Inverted /clear order: b's start(clear) before a's end(clear).
    env.record(&start(
        C,
        "b",
        &cwd,
        Some(&path),
        "clear",
        "2026-10-20T11:59:59Z",
    ));
    env.record(&end(C, "a", &cwd, None, "clear", "2026-10-20T12:00:00Z"));

    let b = session_by_sid(&env.conn, "b");
    assert_eq!(b.start_source.as_deref(), Some("clear"));
    assert_eq!(
        b.previous_session_id,
        Some(session_by_sid(&env.conn, "a").id)
    );
}

/// start_source não é sobrescrito por um start posterior
#[test]
fn start_source_is_not_overwritten_by_later_start() {
    let mut env = Env::new();
    let cwd = env.base.clone();
    env.record(&start(
        C,
        "s",
        &cwd,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&end(
        C,
        "s",
        &cwd,
        None,
        "prompt_input_exit",
        "2026-10-20T10:30:00Z",
    ));
    env.record(&start(C, "s", &cwd, None, "resume", "2026-10-20T11:00:00Z"));

    assert_eq!(
        sole_session(&env.conn).start_source.as_deref(),
        Some("startup")
    );
}

/// start reaproveita a execução aberta que a descoberta acabou de criar, em vez de abrir outra
#[test]
fn start_adopts_open_run_just_created_by_discovery() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("desc", &[]);
    let discovered = insert_discovered(
        &env.conn,
        "desc",
        &base,
        &path,
        ts("2026-10-20T12:00:00Z"),
        false,
    );

    env.record(&start(
        C,
        "desc",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T11:59:58Z",
    ));

    let run = sole_run(&env.conn);
    assert_eq!(run.id, discovered);
    assert_eq!(run.ended_at, None);
    // The hook's start is the real start, and the run now belongs to the hook.
    assert_eq!(run.started_at, ts("2026-10-20T11:59:58Z"));
    assert!(!run.discovered);
    assert_eq!(
        sole_session(&env.conn).start_source.as_deref(),
        Some("startup")
    );

    // A second start (another real run) creates a new run again.
    env.record(&start(
        C,
        "desc",
        &base,
        Some(&path),
        "resume",
        "2026-10-20T12:05:50Z",
    ));
    assert_eq!(count(&env.conn, "session_runs"), 2);
}

/// start não reaproveita execução da descoberta que começou há mais de idle_minutes
#[test]
fn start_does_not_adopt_discovered_run_older_than_idle_minutes() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("velha", &[]);
    insert_discovered(
        &env.conn,
        "velha",
        &base,
        &path,
        ts("2026-10-20T12:00:00Z"),
        false,
    );

    env.record(&start(
        C,
        "velha",
        &base,
        Some(&path),
        "resume",
        "2026-10-20T12:40:00Z",
    ));

    assert_eq!(count(&env.conn, "session_runs"), 2);
}

/// start não reaproveita a execução aberta criada por outro start (hook)
#[test]
fn start_does_not_adopt_open_run_created_by_another_start() {
    let mut env = Env::new();
    let base = env.base.clone();
    env.record(&start(
        C,
        "h",
        &base,
        None,
        "startup",
        "2026-10-20T10:00:00Z",
    ));
    env.record(&start(
        C,
        "h",
        &base,
        None,
        "resume",
        "2026-10-20T10:05:00Z",
    ));

    assert_eq!(count(&env.conn, "session_runs"), 2);
    assert_eq!(count(&env.conn, "session_runs WHERE discovered = 1"), 0);
}

/// a execução da descoberta adotada pelo start deixa de ser backfill
#[test]
fn discovered_run_adopted_by_start_is_no_longer_backfill() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("bf", &[]);
    insert_discovered(
        &env.conn,
        "bf",
        &base,
        &path,
        ts("2026-10-20T12:00:00Z"),
        true,
    );
    assert!(sole_run(&env.conn).is_backfill);

    env.record(&start(
        C,
        "bf",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T11:59:58Z",
    ));

    assert!(!sole_run(&env.conn).is_backfill);
}

/// Saving an unchanged row does not bump `updated_at` (Eloquent saves only dirty rows); a change does.
#[test]
fn updated_at_moves_only_when_a_row_changes() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("u", &[]);
    env.record(&start(
        C,
        "u",
        &base,
        Some(&path),
        "startup",
        "2026-10-20T12:00:00Z",
    ));
    env.record(&end(
        C,
        "u",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T12:30:00Z",
    ));
    let before = sole_run(&env.conn);
    let s = sole_session(&env.conn).id;

    assert_eq!(before.updated_at, Some(now_ms()));

    let later = now_ms() + 60_000;
    env.recorder()
        .refresh_from_transcript(s, None, later)
        .unwrap();
    assert_eq!(sole_run(&env.conn).updated_at, before.updated_at);

    env.conn
        .execute("UPDATE session_runs SET summary_status = 'failed'", [])
        .unwrap();
    env.recorder()
        .refresh_from_transcript(s, None, later)
        .unwrap();
    let after = sole_run(&env.conn);
    assert_eq!(after.summary_status, "pending");
    assert_eq!(after.updated_at, Some(later));
    assert_eq!(after.created_at, Some(now_ms()));
}

/// A Codex end with the rollout builder of `support` is read back by the factory.
#[test]
fn support_codex_rollout_is_readable() {
    let env = Env::new();
    let path = env.base.join("rollout.jsonl");
    codex_rollout(
        &path,
        "x9",
        "/tmp/x",
        "cli",
        "2026-10-20T15:00:00Z",
        &[("Oi", "Olá")],
    );
    let x = factory::read(Tool::Codex, "x9", Some(&path), &env.paths)
        .unwrap()
        .unwrap();
    assert_eq!(x.turns.len(), 1);
    assert_eq!(x.turns[0].prompt, "Oi");
    assert_eq!(x.first_at, Some(ts("2026-10-20T15:00:00Z")));
}

/// Done is kept while the transcript fingerprint is unchanged; once it changes, the run goes back
/// to Pending with a fresh attempts budget (Inv §3.4 `refreshFromTranscript`).
#[test]
fn done_is_kept_with_same_fingerprint_and_reset_when_it_changes() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("fp", &[]);
    let fingerprint = factory::read(C, "fp", Some(&path), &env.paths)
        .unwrap()
        .unwrap()
        .fingerprint();
    let s = insert_session(&env.conn, C, "fp", &base, Some(&path));
    let available_at = now_ms() + 3_600_000;
    let run = insert_run(
        &env.conn,
        s,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:30:00Z")),
            summary_status: Some("done"),
            summary_attempts: 2,
            summary_available_at: Some(available_at),
            summarized_fingerprint: Some(fingerprint),
            ..Default::default()
        },
    );
    let view = |env: &Env| runs(&env.conn, &format!("id = {run}")).remove(0);

    env.recorder()
        .refresh_from_transcript(s, None, now_ms())
        .unwrap();
    let kept = view(&env);
    assert_eq!(kept.summary_status, "done");
    assert_eq!(kept.summary_attempts, 2);
    assert_eq!(kept.summary_available_at, Some(available_at));

    // The session grew: new content, new summary.
    append_line(
        &path,
        &claude_user("fp", "Mais uma.", "2026-10-20T12:20:00.000Z"),
    );
    env.recorder()
        .refresh_from_transcript(s, None, now_ms())
        .unwrap();
    let reset = view(&env);
    assert_eq!(reset.summary_status, "pending");
    assert_eq!(reset.summary_attempts, 0);
    assert_eq!(reset.summary_available_at, None);
}

/// The session's (and project's) last_activity_at follows the transcript's lastAt when it is
/// later than the event.
#[test]
fn last_activity_advances_to_transcript_last_at_when_later() {
    let mut env = Env::new();
    let base = env.base.clone();
    let path = env.transcript("la", &[]);
    // The fixture's last event is 12:05:30, after this end.
    env.record(&end(
        C,
        "la",
        &base,
        Some(&path),
        "prompt_input_exit",
        "2026-10-20T12:01:00Z",
    ));

    let s = sole_session(&env.conn);
    assert_eq!(s.last_activity_at, ts("2026-10-20T12:05:30Z"));
    assert_eq!(project(&env.conn, s.project_id).3, Some(s.last_activity_at));
}

/// Adoption fills head_start/status_hash_start/branch from the snapshot only where empty.
#[test]
fn adoption_fills_empty_start_snapshot_fields_only() {
    let mut env = Env::new();
    let cwd = env.repo("adota");
    let path = env.transcript("ad", &[]);
    let run = insert_discovered(
        &env.conn,
        "ad",
        &cwd,
        &path,
        ts("2026-10-20T12:00:00Z"),
        false,
    );
    env.conn
        .execute(
            "UPDATE session_runs SET status_hash_start = 's0' WHERE id = ?1",
            [run],
        )
        .unwrap();

    env.record(&start(
        C,
        "ad",
        &cwd,
        Some(&path),
        "startup",
        "2026-10-20T11:59:58Z",
    ));

    let r = sole_run(&env.conn);
    assert_eq!(r.id, run);
    assert_eq!(
        r.head_start.as_deref(),
        Some(&*git(&cwd, &["rev-parse", "HEAD"]))
    );
    assert_eq!(r.status_hash_start.as_deref(), Some("s0"));
    assert_eq!(r.branch.as_deref(), Some("main"));
}

/// Inverted /clear chaining only links a session whose *first* run started within
/// ±chain_window_seconds of the end, even if a later run is inside the window.
#[test]
fn inverted_clear_chaining_requires_first_run_inside_window() {
    for (first, chained) in [
        ("2026-10-20T10:00:00Z", false),
        ("2026-10-20T10:29:30Z", true),
    ] {
        let mut env = Env::new();
        let cwd = env.base.clone();
        env.record(&start(
            C,
            "a",
            &cwd,
            None,
            "startup",
            "2026-10-20T09:00:00Z",
        ));
        let b = insert_session(&env.conn, C, "b", &cwd, None);
        env.conn
            .execute(
                "UPDATE ai_sessions SET start_source = 'clear' WHERE id = ?1",
                [b],
            )
            .unwrap();
        for at in [first, "2026-10-20T10:29:40Z"] {
            insert_run(
                &env.conn,
                b,
                RunSeed {
                    started_at: ts(at),
                    ..Default::default()
                },
            );
        }

        env.record(&end(C, "a", &cwd, None, "clear", "2026-10-20T10:30:00Z"));

        let expected = chained.then(|| session_by_sid(&env.conn, "a").id);
        assert_eq!(
            session_by_sid(&env.conn, "b").previous_session_id,
            expected,
            "first run at {first}"
        );
    }
}

/// An unreadable transcript (Codex database with an unexpected schema) does not stop the event:
/// the run is recorded and exactly one warning is returned.
#[test]
fn unreadable_transcript_still_records_event_with_one_warning() {
    let mut env = Env::new();
    let base = env.base.clone();
    rusqlite::Connection::open(env.paths.codex_home.join("state_5.sqlite"))
        .unwrap()
        .execute_batch("CREATE TABLE threads (id TEXT);")
        .unwrap();
    rusqlite::Connection::open(env.paths.codex_home.join("thread_history_1.sqlite"))
        .unwrap()
        .execute_batch("CREATE TABLE thread_items (id TEXT);")
        .unwrap();

    let warnings = env.record_warnings(&end(X, "xu", &base, None, "other", "2026-10-20T12:20:00Z"));

    let run = sole_run(&env.conn);
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:20:00Z")));
    assert_eq!(run.changed_files, None);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("codex") && warnings[0].contains("xu"),
        "{}",
        warnings[0]
    );
    assert!(env.recorder().take_warnings().is_empty());
}
