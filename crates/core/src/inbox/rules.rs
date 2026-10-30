//! The six v1 inbox rules (Inv §5, port of `app/Inbox/Rules/*`). Each pushes its items into
//! `out`; a project whose git or files fail is skipped, never the whole rule.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, params};
use serde_json::Value;

use super::git_cache::{CachedGit, GitCache};
use super::{Badge, DAY_MS, InboxItem, ItemKind};
use crate::git::Git;
use crate::model::{Tool, VISIBLE_SESSION_SQL};
use crate::ongoings::ONGOING_DIR;
use crate::{Result, Settings};

/// sha1 of the empty string: the `status_hash` of a clean tree.
const CLEAN_STATUS_HASH: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

fn sha1_hex(s: &str) -> String {
    sha1_smol::Sha1::from(s).digest().to_string()
}

/// `json_encode` with PHP's default flags, which v1 hashes: `/` becomes `\/` and every non-ASCII
/// char `\uXXXX` (lowercase hex, UTF-16 surrogate pairs). Compact, keys in original order.
fn php_json_encode(value: &Value) -> String {
    use serde_json::ser::{Formatter, Serializer};
    use std::io;

    struct Php;
    impl Formatter for Php {
        fn write_string_fragment<W: ?Sized + io::Write>(
            &mut self,
            w: &mut W,
            fragment: &str,
        ) -> io::Result<()> {
            for c in fragment.chars() {
                match c {
                    '/' => w.write_all(b"\\/")?,
                    c if c.is_ascii() => w.write_all(&[c as u8])?,
                    c => {
                        let mut units = [0u16; 2];
                        for u in c.encode_utf16(&mut units) {
                            write!(w, "\\u{u:04x}")?;
                        }
                    }
                }
            }
            Ok(())
        }
    }

    let mut buf = Vec::new();
    if serde::Serialize::serialize(value, &mut Serializer::with_formatter(&mut buf, Php)).is_err() {
        return String::new();
    }
    String::from_utf8(buf).unwrap_or_default()
}

#[derive(Debug, Clone)]
pub(super) struct ActiveProject {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub last_activity_at: i64,
}

pub(super) struct Ctx<'a> {
    pub conn: &'a Connection,
    pub git: &'a Git,
    pub cache: &'a GitCache,
    pub settings: &'a Settings,
    pub now_ms: i64,
    pub projects: &'a [ActiveProject],
}

impl Ctx<'_> {
    fn ttl_ms(&self) -> i64 {
        i64::from(self.settings.git_cache_seconds) * 1000
    }

    fn stale_cutoff(&self) -> i64 {
        self.now_ms - i64::from(self.settings.stale_days) * DAY_MS
    }
}

/// Projects with activity in the last `active_projects_days` whose folder exists.
pub(super) fn active_projects(
    conn: &Connection,
    settings: &Settings,
    now_ms: i64,
) -> Result<Vec<ActiveProject>> {
    let cutoff = now_ms - i64::from(settings.active_projects_days) * DAY_MS;
    let mut stmt = conn.prepare(
        "SELECT id, name, path, last_activity_at FROM projects \
         WHERE last_activity_at >= ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([cutoff], |r| {
        Ok(ActiveProject {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            last_activity_at: r.get(3)?,
        })
    })?;
    let mut list = Vec::new();
    for row in rows {
        let p = row?;
        if !p.path.is_empty() && Path::new(&p.path).is_dir() {
            list.push(p);
        }
    }
    Ok(list)
}

fn item(key: String, kind: ItemKind, badge: Badge, project_id: i64, title: String) -> InboxItem {
    InboxItem {
        key,
        kind,
        badge,
        project_id,
        title,
        detail: None,
        state_hash: String::new(),
        session_id: None,
        reference_ms: None,
        path: None,
        ended_at_ms: None,
        ahead: None,
        age_days: None,
    }
}

fn project_title(name: &str, branch: Option<&str>) -> String {
    match branch {
        Some(b) if !b.is_empty() => format!("{name} · {b}"),
        _ => name.to_string(),
    }
}

/// Last closed run of each front (`project:branch`), when it ended badly or left work undone.
pub(super) fn interrupted(ctx: &Ctx, out: &mut Vec<InboxItem>) -> Result<()> {
    let cutoff = ctx.now_ms - i64::from(ctx.settings.interrupted_window_days) * DAY_MS;
    let mut stmt = ctx.conn.prepare(&format!(
        "SELECT r.id, r.ai_session_id, s.project_id, r.branch, r.ended_at, r.ended_cleanly, \
                r.unfinished, r.is_backfill, r.end_reason, s.tool \
         FROM session_runs r JOIN ai_sessions s ON s.id = r.ai_session_id \
         WHERE s.{VISIBLE_SESSION_SQL} AND r.started_at >= ?1 \
         ORDER BY r.started_at DESC, r.id DESC"
    ))?;
    let rows = stmt.query_map([cutoff], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut fronts = HashSet::new();
    for row in rows {
        let (run, session, pid, branch, ended_at, cleanly, unfinished, backfill, reason, tool) =
            row?;
        let Some(project) = ctx.projects.iter().find(|p| p.id == pid) else {
            continue;
        };
        let branch = branch.unwrap_or_default();
        if !fronts.insert((pid, branch.clone())) {
            continue;
        }
        let Some(ended_at) = ended_at else { continue };
        let unfinished = unfinished == Some(1);
        let codex_idle = tool == Tool::Codex.as_str() && reason.as_deref() == Some("idle");
        let backfill = backfill != 0;
        let enters = if backfill || codex_idle {
            unfinished
        } else {
            cleanly == Some(0) || unfinished
        };
        if !enters {
            continue;
        }
        let badge = if cleanly == Some(0) && !backfill && !codex_idle {
            Badge::Interrupted
        } else {
            Badge::HalfDone
        };
        let mut it = item(
            format!("interrupted:{pid}:{branch}"),
            ItemKind::Interrupted,
            badge,
            pid,
            project_title(&project.name, Some(&branch)),
        );
        it.state_hash = format!("{run}:{}", ended_at / 1000);
        it.session_id = Some(session);
        it.reference_ms = Some(ended_at);
        it.ended_at_ms = Some(ended_at);
        out.push(it);
    }
    Ok(())
}

pub(super) fn stale_branches(ctx: &Ctx, out: &mut Vec<InboxItem>) {
    for p in ctx.projects {
        let CachedGit::Stale(list) =
            ctx.cache
                .get_or(&format!("stale:{}", p.id), ctx.now_ms, ctx.ttl_ms(), || {
                    CachedGit::Stale(ctx.git.stale_branches(
                        Path::new(&p.path),
                        ctx.settings.stale_days,
                        ctx.now_ms,
                    ))
                })
        else {
            continue;
        };
        for b in list {
            let mut it = item(
                format!("stale-branch:{}:{}", p.id, b.branch),
                ItemKind::StaleBranch,
                Badge::StaleBranch,
                p.id,
                project_title(&p.name, Some(&b.branch)),
            );
            it.state_hash = (b.last_commit_ms / 1000).to_string();
            it.ahead = Some(b.ahead);
            it.age_days = Some(((ctx.now_ms - b.last_commit_ms).max(0) / DAY_MS) as u32);
            it.reference_ms = Some(b.last_commit_ms);
            out.push(it);
        }
    }
}

pub(super) fn dirty(ctx: &Ctx, out: &mut Vec<InboxItem>) {
    for p in ctx.projects {
        if p.last_activity_at >= ctx.stale_cutoff() {
            continue;
        }
        let CachedGit::Status(hash) = ctx.cache.get_or(
            &format!("status:{}", p.id),
            ctx.now_ms,
            ctx.ttl_ms(),
            || {
                CachedGit::Status(
                    ctx.git
                        .status_porcelain(Path::new(&p.path))
                        .map(|s| sha1_hex(&s)),
                )
            },
        ) else {
            continue;
        };
        let Some(hash) = hash.filter(|h| !h.is_empty() && h != CLEAN_STATUS_HASH) else {
            continue;
        };
        let mut it = item(
            format!("dirty:{}", p.id),
            ItemKind::Dirty,
            Badge::UncommittedChanges,
            p.id,
            p.name.clone(),
        );
        it.state_hash = hash;
        it.reference_ms = Some(p.last_activity_at);
        out.push(it);
    }
}

pub(super) fn worktrees(ctx: &Ctx, out: &mut Vec<InboxItem>) {
    for p in ctx.projects {
        let CachedGit::Worktrees(list) = ctx.cache.get_or(
            &format!("worktrees:{}", p.id),
            ctx.now_ms,
            ctx.ttl_ms(),
            || CachedGit::Worktrees(ctx.git.worktrees(Path::new(&p.path))),
        ) else {
            continue;
        };
        for wt in list.into_iter().skip(1) {
            let wt_path = wt.path.to_string_lossy().into_owned();
            // Exact prefix (substr), never LIKE: `_` and `%` are wildcards there.
            let prefix = format!(
                "{}{}",
                wt_path.trim_end_matches(['/', std::path::MAIN_SEPARATOR]),
                std::path::MAIN_SEPARATOR
            );
            let last: Option<i64> = ctx
                .conn
                .query_row(
                    "SELECT MAX(last_activity_at) FROM ai_sessions WHERE project_id = ?1 \
                     AND (cwd = ?2 OR substr(cwd, 1, ?3) = ?4)",
                    params![p.id, wt_path, prefix.chars().count() as i64, prefix],
                    |r| r.get(0),
                )
                .unwrap_or(None);
            if last.is_some_and(|l| l >= ctx.stale_cutoff()) {
                continue;
            }
            let label = wt.branch.clone().unwrap_or_else(|| {
                wt.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let mut it = item(
                format!("worktree:{}:{wt_path}", p.id),
                ItemKind::Worktree,
                Badge::Worktree,
                p.id,
                project_title(&p.name, Some(&label)),
            );
            it.state_hash = wt.head.clone().unwrap_or_default();
            it.detail = Some(wt_path);
            let reference = wt.last_commit_ms.unwrap_or(0).max(last.unwrap_or(0));
            it.reference_ms = (reference > 0).then_some(reference);
            out.push(it);
        }
    }
}

fn scalar_text(v: Option<&Value>) -> Option<String> {
    let text = match v? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// `<project>/.claude/pending-reminder.json`; an unreadable or invalid file is skipped.
pub(super) fn reminders(ctx: &Ctx, out: &mut Vec<InboxItem>) {
    for p in ctx.projects {
        let file = Path::new(&p.path).join(".claude/pending-reminder.json");
        let Some(data) = fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let Some(items) = data.get("items").and_then(Value::as_array) else {
            continue;
        };
        for reminder in items {
            let Some(reference) = scalar_text(reminder.get("ref")) else {
                continue;
            };
            let kind = reminder
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("note");
            let text =
                scalar_text(reminder.get("title")).or_else(|| scalar_text(reminder.get("text")));
            let title = match (kind, &text) {
                ("issue", Some(t)) => format!("#{reference} {t}"),
                ("issue", None) => format!("#{reference}"),
                (_, Some(t)) => t.clone(),
                (_, None) => reference.clone(),
            };
            let mut it = item(
                format!("reminder:{}:{reference}", p.id),
                ItemKind::Reminder,
                Badge::Reminder,
                p.id,
                title,
            );
            it.state_hash = sha1_hex(&php_json_encode(reminder));
            it.detail = Some(p.name.clone());
            if kind == "ongoing" {
                it.path = Some(absolute_path(&p.path, &reference));
            }
            out.push(it);
        }
    }
}

/// The file a reminder cites (relative to the project), normalized like the ongoing rule's.
fn absolute_path(project_path: &str, reference: &str) -> String {
    let file = if reference.starts_with('/') {
        PathBuf::from(reference)
    } else {
        Path::new(project_path.trim_end_matches('/')).join(reference)
    };
    fs::canonicalize(&file)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// Every `*.md` in `docs/ongoing-sessions/` (the v1 glob, not the Task 6 name rule).
pub(super) fn ongoing_docs(ctx: &Ctx, out: &mut Vec<InboxItem>) {
    for p in ctx.projects {
        let dir = Path::new(&p.path).join(ONGOING_DIR);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                (name.ends_with(".md") && e.path().is_file()).then(|| (name, e.path()))
            })
            .collect();
        files.sort();
        for (name, path) in files {
            let Some(mtime) = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
            else {
                continue;
            };
            let mut it = item(
                format!("ongoing:{}:{ONGOING_DIR}/{name}", p.id),
                ItemKind::Ongoing,
                Badge::Ongoing,
                p.id,
                name.trim_end_matches(".md").to_string(),
            );
            it.state_hash = mtime.to_string();
            it.reference_ms = Some(mtime * 1000);
            it.path = Some(
                fs::canonicalize(&path)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
            );
            it.detail = Some(p.name.clone());
            out.push(it);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_encoding_escapes_slash_and_non_ascii() {
        let v = serde_json::json!({"a": "x/y", "b": "Sessão", "c": "😀", "d": "q\"\n"});
        assert_eq!(
            php_json_encode(&v),
            r#"{"a":"x\/y","b":"Sess\u00e3o","c":"\ud83d\ude00","d":"q\"\n"}"#
        );
    }
}
