//! Inbox rules, dismissal, grouping and the git cache (Task 7).

mod support;

use std::fs;
use std::path::Path;

use aiannotations_core::Settings;
use aiannotations_core::inbox::{self, Badge, CountKey, GitCache, InboxItem, ItemKind};
use aiannotations_core::model::Tool;
use rusqlite::{Connection, params};
use serde_json::json;
use support::*;

const DAY: i64 = 86_400_000;

fn build(env: &Env) -> Vec<InboxItem> {
    inbox::build(
        &env.conn,
        &env.git,
        &GitCache::new(),
        &env.settings,
        now_ms(),
    )
    .unwrap()
}

fn of_kind(items: &[InboxItem], kind: ItemKind) -> Vec<&InboxItem> {
    items.iter().filter(|i| i.kind == kind).collect()
}

/// Project whose folder exists (plain dir) with the given last activity.
fn project(env: &Env, name: &str, activity_ms: i64) -> (i64, std::path::PathBuf) {
    let dir = env.base.join(name);
    fs::create_dir_all(&dir).unwrap();
    project_at(env, &dir, activity_ms)
}

fn project_at(env: &Env, dir: &Path, activity_ms: i64) -> (i64, std::path::PathBuf) {
    let id = insert_project(&env.conn, &dir.to_string_lossy());
    env.conn
        .execute(
            "UPDATE projects SET last_activity_at = ?1 WHERE id = ?2",
            params![activity_ms, id],
        )
        .unwrap();
    (id, dir.to_path_buf())
}

fn session_in(
    conn: &Connection,
    project_id: i64,
    sid: &str,
    tool: Tool,
    cwd: &str,
    at: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO ai_sessions (project_id, tool, session_id, cwd, last_activity_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, tool.as_str(), sid, cwd, at],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[derive(Default)]
struct R {
    branch: Option<&'static str>,
    started: i64,
    ended: Option<i64>,
    cleanly: Option<bool>,
    unfinished: Option<bool>,
    backfill: bool,
    reason: Option<&'static str>,
}

fn run(conn: &Connection, session: i64, r: R) -> i64 {
    let id = insert_run(
        conn,
        session,
        RunSeed {
            started_at: r.started,
            ended_at: r.ended,
            end_reason: r.reason,
            ended_cleanly: r.cleanly,
            is_backfill: r.backfill,
            ..Default::default()
        },
    );
    conn.execute(
        "UPDATE session_runs SET branch = ?1, unfinished = ?2 WHERE id = ?3",
        params![r.branch, r.unfinished, id],
    )
    .unwrap();
    id
}

fn closed(branch: Option<&'static str>, cleanly: bool, unfinished: Option<bool>) -> R {
    R {
        branch,
        started: now_ms() - 2 * DAY,
        ended: Some(now_ms() - DAY),
        cleanly: Some(cleanly),
        unfinished,
        ..Default::default()
    }
}

#[test]
fn interrupted_front_disappears_when_next_run_ends_clean() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let s = session_in(
        &env.conn,
        pid,
        "a",
        Tool::Claude,
        &dir.to_string_lossy(),
        now_ms(),
    );
    run(&env.conn, s, closed(Some("main"), false, None));
    let items = build(&env);
    assert_eq!(items.len(), 1);
    let it = &items[0];
    assert_eq!(it.key, format!("interrupted:{pid}:main"));
    assert_eq!(it.badge, Badge::Interrupted);
    assert_eq!(it.title, "proj · main");
    assert_eq!(it.session_id, Some(s));
    assert_eq!(it.ended_at_ms, Some(now_ms() - DAY));
    assert_eq!(it.reference_ms, it.ended_at_ms);

    // A newer run that ended cleanly hides the front, even though an older one was interrupted.
    run(
        &env.conn,
        s,
        R {
            branch: Some("main"),
            started: now_ms() - DAY / 2,
            ended: Some(now_ms() - DAY / 4),
            cleanly: Some(true),
            ..Default::default()
        },
    );
    assert!(of_kind(&build(&env), ItemKind::Interrupted).is_empty());
}

#[test]
fn different_branches_are_independent_fronts() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let s = session_in(
        &env.conn,
        pid,
        "a",
        Tool::Claude,
        &dir.to_string_lossy(),
        now_ms(),
    );
    run(&env.conn, s, closed(Some("a"), false, None));
    run(
        &env.conn,
        s,
        R {
            branch: Some("b"),
            started: now_ms() - DAY / 2,
            ended: Some(now_ms() - DAY / 4),
            cleanly: Some(true),
            ..Default::default()
        },
    );
    let items = build(&env);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, format!("interrupted:{pid}:a"));
    // No branch: the key ends in an empty branch and the title is just the project.
    run(&env.conn, s, closed(None, false, None));
    let items = build(&env);
    let none = items
        .iter()
        .find(|i| i.key == format!("interrupted:{pid}:"))
        .unwrap();
    assert_eq!(none.title, "proj");
}

#[test]
fn backfill_and_codex_idle_need_unfinished() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let cwd = dir.to_string_lossy().into_owned();
    let s1 = session_in(&env.conn, pid, "bf", Tool::Claude, &cwd, now_ms());
    let s2 = session_in(&env.conn, pid, "cx", Tool::Codex, &cwd, now_ms());
    let backfill = |unfinished| R {
        backfill: true,
        ..closed(Some("bf"), false, unfinished)
    };
    let idle = |unfinished| R {
        reason: Some("idle"),
        ..closed(Some("cx"), false, unfinished)
    };
    run(&env.conn, s1, backfill(Some(false)));
    run(&env.conn, s2, idle(None));
    assert!(build(&env).is_empty());

    run(
        &env.conn,
        s1,
        R {
            started: now_ms() - DAY,
            ..backfill(Some(true))
        },
    );
    run(
        &env.conn,
        s2,
        R {
            started: now_ms() - DAY,
            ..idle(Some(true))
        },
    );
    let items = build(&env);
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.badge == Badge::HalfDone));
}

#[test]
fn badge_interrupted_vs_half_done() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let cwd = dir.to_string_lossy().into_owned();
    let s = session_in(&env.conn, pid, "a", Tool::Claude, &cwd, now_ms());
    run(&env.conn, s, closed(Some("bad"), false, None));
    run(&env.conn, s, closed(Some("half"), true, Some(true)));
    run(&env.conn, s, closed(Some("fine"), true, Some(false)));
    run(&env.conn, s, closed(Some("open"), false, None));
    env.conn
        .execute(
            "UPDATE session_runs SET ended_at = NULL WHERE branch = 'open'",
            [],
        )
        .unwrap();
    let items = build(&env);
    let badge = |b: &str| {
        items
            .iter()
            .find(|i| i.key == format!("interrupted:{pid}:{b}"))
            .map(|i| i.badge)
    };
    assert_eq!(badge("bad"), Some(Badge::Interrupted));
    assert_eq!(badge("half"), Some(Badge::HalfDone));
    assert_eq!(badge("fine"), None);
    assert_eq!(badge("open"), None);
    let bad = items.iter().find(|i| i.key.ends_with(":bad")).unwrap();
    assert_eq!(
        bad.state_hash.split_once(':').unwrap().1,
        ((now_ms() - DAY) / 1000).to_string()
    );
}

#[test]
fn interrupted_ignores_hidden_sessions_old_runs_and_inactive_projects() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let cwd = dir.to_string_lossy().into_owned();
    let hidden = session_in(&env.conn, pid, "h", Tool::Claude, &cwd, now_ms());
    env.conn
        .execute(
            "UPDATE ai_sessions SET is_empty = 1 WHERE id = ?1",
            [hidden],
        )
        .unwrap();
    run(&env.conn, hidden, closed(Some("h"), false, None));
    let ok = session_in(&env.conn, pid, "o", Tool::Claude, &cwd, now_ms());
    run(
        &env.conn,
        ok,
        R {
            started: now_ms() - 20 * DAY,
            ended: Some(now_ms() - 19 * DAY),
            cleanly: Some(false),
            ..Default::default()
        },
    );
    let (pid2, dir2) = project(&env, "old", now_ms() - 61 * DAY);
    let s2 = session_in(
        &env.conn,
        pid2,
        "x",
        Tool::Claude,
        &dir2.to_string_lossy(),
        now_ms(),
    );
    run(&env.conn, s2, closed(None, false, None));
    assert!(build(&env).is_empty());
}

#[test]
fn stale_branch_item() {
    let env = Env::new();
    let repo = git_repo(&env.base.join("repo"));
    let (pid, _) = project_at(&env, &repo, now_ms());
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    let when = format!("{} +0000", (now_ms() - 5 * DAY) / 1000);
    let dates = [
        ("GIT_COMMITTER_DATE", when.as_str()),
        ("GIT_AUTHOR_DATE", when.as_str()),
    ];
    git_env(
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "work"],
        &dates,
    );
    git(&repo, &["checkout", "-q", "main"]);

    let items = build(&env);
    let stale = of_kind(&items, ItemKind::StaleBranch);
    assert_eq!(stale.len(), 1);
    let it = stale[0];
    assert_eq!(it.key, format!("stale-branch:{pid}:feature"));
    assert_eq!(it.title, "repo · feature");
    assert_eq!(it.ahead, Some(1));
    assert_eq!(it.age_days, Some(5));
    assert_eq!(it.state_hash, ((now_ms() - 5 * DAY) / 1000).to_string());
    assert_eq!(it.reference_ms, Some((now_ms() - 5 * DAY) / 1000 * 1000));
}

#[test]
fn dirty_only_for_idle_projects() {
    let env = Env::new();
    let idle = git_repo(&env.base.join("idle"));
    let busy = git_repo(&env.base.join("busy"));
    let clean = git_repo(&env.base.join("clean"));
    for r in [&idle, &busy] {
        fs::write(r.join("new.txt"), "x").unwrap();
    }
    let (idle_id, _) = project_at(&env, &idle, now_ms() - 10 * DAY);
    project_at(&env, &busy, now_ms() - DAY);
    project_at(&env, &clean, now_ms() - 10 * DAY);
    let items = build(&env);
    let dirty = of_kind(&items, ItemKind::Dirty);
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].key, format!("dirty:{idle_id}"));
    assert_eq!(dirty[0].title, "idle");
    assert_eq!(dirty[0].reference_ms, Some(now_ms() - 10 * DAY));
    assert_ne!(
        dirty[0].state_hash,
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    assert_eq!(dirty[0].badge, Badge::UncommittedChanges);
}

#[test]
fn worktree_with_underscore_does_not_match_sibling_path() {
    let env = Env::new();
    let repo = git_repo(&env.base.join("main_proj"));
    let (pid, _) = project_at(&env, &repo, now_ms());
    let wt_a = env.base.join("x/proj_a");
    let wt_b = env.base.join("x/proj_b");
    fs::create_dir_all(env.base.join("x")).unwrap();
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "wa", wt_a.to_str().unwrap()],
    );
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "wb", wt_b.to_str().unwrap()],
    );
    // A session in the sibling `proj_ab` must not count for `proj_a` (`_` is a LIKE wildcard).
    session_in(
        &env.conn,
        pid,
        "sib",
        Tool::Claude,
        &format!("{}/x/proj_ab/sub", env.base.display()),
        now_ms(),
    );
    // `projXa` would match `proj_a/%` if `_` were a LIKE wildcard.
    session_in(
        &env.conn,
        pid,
        "wild",
        Tool::Claude,
        &format!("{}/x/projXa/sub", env.base.display()),
        now_ms(),
    );
    // A session inside `proj_b` counts.
    session_in(
        &env.conn,
        pid,
        "in",
        Tool::Claude,
        &format!("{}/sub", wt_b.display()),
        now_ms(),
    );

    let items = build(&env);
    let wts = of_kind(&items, ItemKind::Worktree);
    assert_eq!(wts.len(), 1, "{wts:?}");
    let w = wts[0];
    assert_eq!(w.key, format!("worktree:{pid}:{}", wt_a.display()));
    assert_eq!(w.title, "main_proj · wa");
    assert_eq!(w.detail.as_deref(), Some(wt_a.to_str().unwrap()));
    assert_eq!(w.state_hash, git(&wt_a, &["rev-parse", "HEAD"]));
    assert!(w.reference_ms.is_some());
}

#[test]
fn reminder_never_has_reference_and_invalid_json_is_ignored() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    let (_, bad) = project(&env, "bad", now_ms());
    fs::create_dir_all(dir.join(".claude")).unwrap();
    fs::create_dir_all(bad.join(".claude")).unwrap();
    fs::write(bad.join(".claude/pending-reminder.json"), "{not json").unwrap();
    let first = json!({"ref": "12", "type": "issue", "title": "Fix it"});
    fs::write(
        dir.join(".claude/pending-reminder.json"),
        json!({"items": [
            first,
            {"ref": "", "text": "skipped"},
            {"title": "no ref"},
            {"ref": 5, "text": "five"},
            {"ref": "only-ref"},
            {"ref": "7", "type": "issue"},
        ]})
        .to_string(),
    )
    .unwrap();
    let items = build(&env);
    let r = of_kind(&items, ItemKind::Reminder);
    let titles: Vec<_> = r.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["#12 Fix it", "five", "only-ref", "#7"]);
    assert!(
        r.iter()
            .all(|i| i.reference_ms.is_none() && i.badge == Badge::Reminder)
    );
    assert_eq!(r[0].key, format!("reminder:{pid}:12"));
    assert_eq!(r[0].detail.as_deref(), Some("proj"));
    assert!(r[0].path.is_none());
    assert_eq!(r[0].state_hash, "cccf90fbffc9d8c413867843ba227c94520ae0f1");
}

/// Hashes are those of v1: `sha1(json_encode($item))`, computed with
/// `php -r 'echo sha1(json_encode([...]));'` on the same items (PHP escapes `/` and non-ASCII).
#[test]
fn reminder_state_hash_matches_php_json_encode() {
    let env = Env::new();
    let (_, dir) = project(&env, "proj", now_ms());
    fs::create_dir_all(dir.join(".claude")).unwrap();
    fs::write(
        dir.join(".claude/pending-reminder.json"),
        r#"{"items":[{"ref":"docs/ongoing-sessions/x.md","type":"ongoing","title":"Sessão"}]}"#,
    )
    .unwrap();
    let items = build(&env);
    let r = of_kind(&items, ItemKind::Reminder);
    assert_eq!(r[0].state_hash, "ed7571b4d73891322782da7dc0226bb749807cc6");
}

#[test]
fn reminder_ongoing_dedupes_ongoing_doc() {
    let env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    fs::create_dir_all(dir.join("docs/ongoing-sessions")).unwrap();
    fs::create_dir_all(dir.join(".claude")).unwrap();
    fs::write(dir.join("docs/ongoing-sessions/a.md"), "# a").unwrap();
    fs::write(dir.join("docs/ongoing-sessions/b.md"), "# b").unwrap();
    fs::write(dir.join("docs/ongoing-sessions/c.txt"), "no").unwrap();
    fs::write(
        dir.join(".claude/pending-reminder.json"),
        json!({"items": [{"ref": "docs/ongoing-sessions/a.md", "type": "ongoing", "title": "Resume a"}]}).to_string(),
    )
    .unwrap();
    let items = build(&env);
    let ongoing = of_kind(&items, ItemKind::Ongoing);
    assert_eq!(ongoing.len(), 1);
    assert_eq!(
        ongoing[0].key,
        format!("ongoing:{pid}:docs/ongoing-sessions/b.md")
    );
    assert_eq!(ongoing[0].title, "b");
    assert!(ongoing[0].reference_ms.is_some());
    let rem = of_kind(&items, ItemKind::Reminder);
    assert_eq!(rem.len(), 1);
    assert_eq!(
        rem[0].path.as_deref(),
        Some(
            fs::canonicalize(dir.join("docs/ongoing-sessions/a.md"))
                .unwrap()
                .to_str()
                .unwrap()
        )
    );
}

#[test]
fn dismissed_item_returns_when_state_changes() {
    let mut env = Env::new();
    let (pid, dir) = project(&env, "proj", now_ms());
    fs::create_dir_all(dir.join(".claude")).unwrap();
    let file = dir.join(".claude/pending-reminder.json");
    fs::write(
        &file,
        json!({"items": [{"ref": "1", "text": "a"}]}).to_string(),
    )
    .unwrap();
    let items = build(&env);
    assert_eq!(items.len(), 1);
    inbox::dismiss(&mut env.conn, &items[0].key, &items[0].state_hash, now_ms()).unwrap();
    inbox::dismiss(
        &mut env.conn,
        &items[0].key,
        &items[0].state_hash,
        now_ms() + 1,
    )
    .unwrap();
    assert_eq!(count(&env.conn, "inbox_dismissals"), 1);
    assert!(build(&env).is_empty());

    fs::write(
        &file,
        json!({"items": [{"ref": "1", "text": "changed"}]}).to_string(),
    )
    .unwrap();
    let items = build(&env);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, format!("reminder:{pid}:1"));

    // dismiss_all: every item, one row each.
    fs::write(
        &file,
        json!({"items": [{"ref": "1"}, {"ref": "2"}]}).to_string(),
    )
    .unwrap();
    let items = build(&env);
    inbox::dismiss_all(&mut env.conn, &items, now_ms()).unwrap();
    assert!(build(&env).is_empty());
    assert_eq!(count(&env.conn, "inbox_dismissals"), 2);
}

#[test]
fn non_git_and_missing_paths_do_not_break_inbox() {
    let env = Env::new();
    let (plain_id, plain) = project(&env, "plain", now_ms());
    fs::create_dir_all(plain.join(".claude")).unwrap();
    fs::write(
        plain.join(".claude/pending-reminder.json"),
        json!({"items": [{"ref": "r", "text": "x"}]}).to_string(),
    )
    .unwrap();
    let gone = env.base.join("gone");
    fs::create_dir_all(&gone).unwrap();
    project_at(&env, &gone, now_ms());
    fs::remove_dir(&gone).unwrap();
    let repo = git_repo(&env.base.join("repo"));
    project_at(&env, &repo, now_ms() - 10 * DAY);
    fs::write(repo.join("n.txt"), "x").unwrap();
    let items = build(&env);
    assert_eq!(of_kind(&items, ItemKind::Reminder)[0].project_id, plain_id);
    assert_eq!(of_kind(&items, ItemKind::Dirty).len(), 1);
    assert_eq!(items.len(), 2);
}

#[cfg(unix)]
#[test]
fn git_cache_reuses_until_ttl() {
    use aiannotations_core::git::Git;
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    project(&env, "proj", now_ms() - 10 * DAY);
    let counter = env.base.join("calls");
    let bin = env.base.join("fakegit");
    fs::write(
        &bin,
        format!("#!/bin/sh\necho x >> '{}'\nexit 0\n", counter.display()),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let git = Git::with_bin(bin);
    let cache = GitCache::new();
    let calls = || {
        fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    };
    let go = |at: i64| inbox::build(&env.conn, &git, &cache, &env.settings, at).unwrap();

    go(now_ms());
    let first = calls();
    assert!(first > 0);
    go(now_ms() + 60_000);
    assert_eq!(
        calls(),
        first,
        "second build inside the TTL reuses the cache"
    );
    go(now_ms() + 301_000);
    assert!(calls() > first, "after the TTL git is called again");
}

#[test]
fn grouped_orders_and_splits_older() {
    let env = Env::new();
    let (a, adir) = project(&env, "alpha", now_ms());
    let (b, bdir) = project(&env, "beta", now_ms());
    let (c, cdir) = project(&env, "gamma", now_ms());
    let s = session_in(
        &env.conn,
        a,
        "s",
        Tool::Claude,
        &adir.to_string_lossy(),
        now_ms(),
    );
    run(&env.conn, s, closed(Some("x"), false, None));
    run(&env.conn, s, closed(Some("y"), true, Some(true)));
    fs::create_dir_all(adir.join("docs/ongoing-sessions")).unwrap();
    fs::write(adir.join("docs/ongoing-sessions/doc.md"), "#").unwrap();
    fs::create_dir_all(adir.join(".claude")).unwrap();
    fs::write(
        adir.join(".claude/pending-reminder.json"),
        json!({"items": [{"ref": "1"}]}).to_string(),
    )
    .unwrap();
    // beta: a 31-day-old doc (older) plus an undated reminder (recent).
    fs::create_dir_all(bdir.join("docs/ongoing-sessions")).unwrap();
    let old = bdir.join("docs/ongoing-sessions/old.md");
    fs::write(&old, "#").unwrap();
    let mtime = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(((now_ms() - 31 * DAY) / 1000) as u64);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    fs::create_dir_all(bdir.join(".claude")).unwrap();
    fs::write(
        bdir.join(".claude/pending-reminder.json"),
        json!({"items": [{"ref": "9"}]}).to_string(),
    )
    .unwrap();
    // gamma: a recent doc only.
    fs::create_dir_all(cdir.join("docs/ongoing-sessions")).unwrap();
    fs::write(cdir.join("docs/ongoing-sessions/g.md"), "#").unwrap();
    let doc_mtime = |p: &Path| {
        let m = fs::metadata(p).unwrap().modified().unwrap();
        m.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64 / 1000 * 1000
    };
    let adoc = doc_mtime(&adir.join("docs/ongoing-sessions/doc.md"));

    let mut items = build(&env);
    items.reverse(); // grouping must reorder, not rely on build order
    let g = inbox::grouped(&env.conn, items, &env.settings, now_ms()).unwrap();

    let recent: Vec<_> = g.recent.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(recent, ["alpha", "gamma", "beta"]);
    let alpha = &g.recent[0];
    assert!(alpha.expanded);
    assert_eq!(alpha.items[0].kind, ItemKind::Interrupted);
    assert_eq!(
        alpha.counts,
        [
            (CountKey::Interrupted, 1),
            (CountKey::HalfDone, 1),
            (CountKey::Ongoing, 1),
            (CountKey::Reminder, 1)
        ]
    );
    assert_eq!(alpha.latest_ms, (now_ms() - DAY).max(adoc));
    assert!(!g.recent[1].expanded);
    assert_eq!(g.recent[2].items.len(), 1);
    assert_eq!(g.recent[2].items[0].kind, ItemKind::Reminder);
    assert_eq!(g.recent[2].latest_ms, 0);
    assert_eq!(g.older.len(), 1);
    assert_eq!(g.older[0].project_id, b);
    assert_eq!(g.older[0].items[0].title, "old");

    // A project that vanished from the table falls back to "Projeto <id>".
    env.conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    env.conn
        .execute("DELETE FROM projects WHERE id = ?1", [c])
        .unwrap();
    let items = [of_kind(&build(&env), ItemKind::Reminder)[0].clone()];
    let mut it = items[0].clone();
    it.project_id = c;
    let g = inbox::grouped(&env.conn, vec![it], &Settings::default(), now_ms()).unwrap();
    assert_eq!(g.recent[0].name, format!("Projeto {c}"));
}
