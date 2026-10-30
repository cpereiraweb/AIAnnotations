//! The inbox: what deserves the developer's attention (Inv §5, port of `app/Inbox`).
//!
//! The core returns data (kinds, badges, numbers, instants); texts are the UI's. Nothing here
//! reads the clock: every function takes `now_ms`.

mod git_cache;
mod rules;

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;

pub use git_cache::{CachedGit, GitCache};

use crate::git::Git;
use crate::{Result, Settings};

pub(crate) const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Interrupted,
    StaleBranch,
    Dirty,
    Worktree,
    Reminder,
    Ongoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Interrupted,
    HalfDone,
    StaleBranch,
    UncommittedChanges,
    Worktree,
    Reminder,
    Ongoing,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxItem {
    pub key: String,
    pub kind: ItemKind,
    pub badge: Badge,
    pub project_id: i64,
    pub title: String,
    /// Data only (worktree path, project name); the wording is the UI's.
    pub detail: Option<String>,
    pub state_hash: String,
    pub session_id: Option<i64>,
    pub reference_ms: Option<i64>,
    pub path: Option<String>,
    pub ended_at_ms: Option<i64>,
    pub ahead: Option<u32>,
    pub age_days: Option<u32>,
}

/// Runs the six rules and drops duplicated-by-path, dismissed and repeated-key items.
pub fn build(
    conn: &Connection,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    now_ms: i64,
) -> Result<Vec<InboxItem>> {
    let projects = rules::active_projects(conn, settings, now_ms)?;
    let ctx = rules::Ctx {
        conn,
        git,
        cache,
        settings,
        now_ms,
        projects: &projects,
    };
    let mut items = Vec::new();
    rules::interrupted(&ctx, &mut items)?;
    rules::stale_branches(&ctx, &mut items);
    rules::dirty(&ctx, &mut items);
    rules::worktrees(&ctx, &mut items);
    rules::reminders(&ctx, &mut items);
    rules::ongoing_docs(&ctx, &mut items);

    // One file can come from two rules (an ongoing reminder and the doc): keep the reminder.
    let reminder_paths: HashSet<String> = items
        .iter()
        .filter(|i| i.kind == ItemKind::Reminder)
        .filter_map(|i| i.path.clone())
        .collect();
    items.retain(|i| {
        i.kind == ItemKind::Reminder || i.path.as_ref().is_none_or(|p| !reminder_paths.contains(p))
    });

    let mut stmt = conn.prepare("SELECT item_key, state_hash FROM inbox_dismissals")?;
    let dismissed: HashMap<String, String> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    items.retain(|i| dismissed.get(&i.key) != Some(&i.state_hash));

    let mut seen = HashSet::new();
    items.retain(|i| seen.insert(i.key.clone()));
    Ok(items)
}

/// Counter keys, in display order (interrupted sessions split by badge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountKey {
    Interrupted,
    HalfDone,
    Ongoing,
    Reminder,
    StaleBranch,
    Worktree,
    Dirty,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxGroup {
    pub project_id: i64,
    pub name: String,
    pub items: Vec<InboxItem>,
    pub expanded: bool,
    pub latest_ms: i64,
    /// Only the present keys, in `CountKey` order.
    pub counts: Vec<(CountKey, usize)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupedInbox {
    pub recent: Vec<InboxGroup>,
    pub older: Vec<InboxGroup>,
}

fn count_key(item: &InboxItem) -> CountKey {
    match item.kind {
        ItemKind::Interrupted if item.badge == Badge::HalfDone => CountKey::HalfDone,
        ItemKind::Interrupted => CountKey::Interrupted,
        ItemKind::Ongoing => CountKey::Ongoing,
        ItemKind::Reminder => CountKey::Reminder,
        ItemKind::StaleBranch => CountKey::StaleBranch,
        ItemKind::Worktree => CountKey::Worktree,
        ItemKind::Dirty => CountKey::Dirty,
    }
}

/// Splits into `recent`/`older` (reference older than `inbox_recent_days`; no reference is never
/// older) and groups by project: groups with an interrupted session first, then the newest.
pub fn grouped(
    conn: &Connection,
    items: Vec<InboxItem>,
    settings: &Settings,
    now_ms: i64,
) -> Result<GroupedInbox> {
    let cutoff = now_ms - i64::from(settings.inbox_recent_days) * DAY_MS;
    let mut stmt = conn.prepare("SELECT id, name FROM projects")?;
    let names: HashMap<i64, String> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;

    let (older, recent): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|i| i.reference_ms.is_some_and(|r| r < cutoff));
    Ok(GroupedInbox {
        recent: groups(recent, &names),
        older: groups(older, &names),
    })
}

fn groups(items: Vec<InboxItem>, names: &HashMap<i64, String>) -> Vec<InboxGroup> {
    let mut by_project: Vec<(i64, Vec<InboxItem>)> = Vec::new();
    for item in items {
        match by_project.iter_mut().find(|(id, _)| *id == item.project_id) {
            Some((_, list)) => list.push(item),
            None => by_project.push((item.project_id, vec![item])),
        }
    }
    let mut list: Vec<InboxGroup> = by_project
        .into_iter()
        .map(|(project_id, mut items)| {
            items.sort_by_key(|i| i.kind != ItemKind::Interrupted); // stable
            let mut counts: Vec<(CountKey, usize)> = Vec::new();
            for item in &items {
                let key = count_key(item);
                match counts.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((key, 1)),
                }
            }
            counts.sort_by_key(|(k, _)| *k);
            InboxGroup {
                project_id,
                name: names
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_else(|| format!("Projeto {project_id}")),
                expanded: items[0].kind == ItemKind::Interrupted,
                latest_ms: items
                    .iter()
                    .filter_map(|i| i.reference_ms)
                    .max()
                    .unwrap_or(0),
                items,
                counts,
            }
        })
        .collect();
    list.sort_by(|a, b| {
        b.expanded
            .cmp(&a.expanded)
            .then(b.latest_ms.cmp(&a.latest_ms))
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

/// Dismisses `item_key` at its current `state_hash` (it returns when the state changes).
pub fn dismiss(conn: &mut Connection, item_key: &str, state_hash: &str, now_ms: i64) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    upsert(&tx, item_key, state_hash, now_ms)?;
    tx.commit()?;
    Ok(())
}

/// Dismisses every item in one transaction.
pub fn dismiss_all(conn: &mut Connection, items: &[InboxItem], now_ms: i64) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for item in items {
        upsert(&tx, &item.key, &item.state_hash, now_ms)?;
    }
    tx.commit()?;
    Ok(())
}

fn upsert(conn: &Connection, key: &str, hash: &str, now_ms: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO inbox_dismissals (item_key, state_hash, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?3) \
         ON CONFLICT (item_key) DO UPDATE SET state_hash = excluded.state_hash, \
         updated_at = excluded.updated_at",
        params![key, hash, now_ms],
    )?;
    Ok(())
}
