//! `projects` rows (Inv §3.4 "Projeto").

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::Result;
use crate::git::GitSnapshot;

/// Project key: the main repository root, or the cwd without its trailing slash outside git.
pub fn key(snap: &GitSnapshot, cwd: &Path) -> String {
    let key = match &snap.repo_root {
        Some(root) => root.to_string_lossy().into_owned(),
        None => cwd.to_string_lossy().trim_end_matches('/').to_string(),
    };
    if key.is_empty() { "/".to_string() } else { key }
}

/// (id, name, path, remote_url, last_activity_at).
type ProjectRow = (i64, String, String, Option<String>, Option<i64>);

/// PHP `basename($key) ?: $key`.
fn name(key: &str) -> String {
    // Windows cwds use `\` (git reports `/`); on Unix a backslash is an ordinary name character.
    match key
        .rsplit(|c| c == '/' || (cfg!(windows) && c == '\\'))
        .next()
    {
        Some(base) if !base.is_empty() => base.to_string(),
        _ => key.to_string(),
    }
}

/// Creates or updates the project of `snap`/`cwd` and returns its id. The name is the key's
/// basename; `remote_url` is only overwritten by a non-null value. `last_activity_at` moves
/// forward to `activity_ms` (never back). `updated_at` (= `now_ms`) changes only when a field did.
pub fn upsert(
    tx: &Transaction,
    snap: &GitSnapshot,
    cwd: &Path,
    activity_ms: i64,
    now_ms: i64,
) -> Result<i64> {
    let key = key(snap, cwd);
    let name = name(&key);
    let now = now_ms;
    let existing: Option<ProjectRow> = tx
        .query_row(
            "SELECT id, name, path, remote_url, last_activity_at FROM projects WHERE key = ?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;

    let Some((id, old_name, old_path, old_remote, old_activity)) = existing else {
        tx.execute(
            "INSERT INTO projects (key, name, path, remote_url, last_activity_at, created_at, \
             updated_at) VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?5)",
            params![key, name, snap.remote_url, activity_ms, now],
        )?;
        return Ok(tx.last_insert_rowid());
    };

    let remote = snap.remote_url.clone().or(old_remote.clone());
    let activity = Some(old_activity.map_or(activity_ms, |a| a.max(activity_ms)));
    if (&old_name, &old_path, &old_remote, &old_activity) != (&name, &key, &remote, &activity) {
        tx.execute(
            "UPDATE projects SET name = ?1, path = ?2, remote_url = ?3, last_activity_at = ?4, \
             updated_at = ?5 WHERE id = ?6",
            params![name, key, remote, activity, now, id],
        )?;
    }
    Ok(id)
}

/// `bumpProject`: the project's `last_activity_at` follows the session's when it is later.
pub(crate) fn bump_activity(
    conn: &Connection,
    project_id: i64,
    at: i64,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE projects SET last_activity_at = ?1, updated_at = ?2 \
         WHERE id = ?3 AND (last_activity_at IS NULL OR last_activity_at < ?1)",
        params![at, now_ms, project_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn key_and_name_follow_v1() {
        let none = GitSnapshot::default();
        assert_eq!(key(&none, Path::new("/a/b/")), "/a/b");
        assert_eq!(key(&none, Path::new("/")), "/");
        let git = GitSnapshot {
            repo_root: Some(PathBuf::from("/r/main-repo")),
            ..Default::default()
        };
        assert_eq!(key(&git, Path::new("/r/wt")), "/r/main-repo");
        assert_eq!(name("/r/main-repo"), "main-repo");
        assert_eq!(name("/"), "/");
    }
}
