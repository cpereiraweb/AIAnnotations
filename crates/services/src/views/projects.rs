//! Projects screen and project detail (Inv §1.2, §1.3): a constant number of queries per screen,
//! except the ongoing count, which reads each project's folder (as the v1 does).

use std::collections::HashMap;

use aiannotations_core::Result;
use aiannotations_core::model::{Tool, VISIBLE_SESSION_SQL};
use aiannotations_core::ongoings::{self, Ongoing};
use rusqlite::Connection;
use serde::Serialize;

use super::runs::{CARD_SELECT, RunCard, query_cards};

/// Window of the tools shown as "recent" on each card.
const RECENT_DAYS: i64 = 14;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolUsage {
    pub tool: Tool,
    /// Some run of the tool in the window changed files.
    pub changed_files: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectCard {
    pub id: i64,
    pub name: String,
    pub path: Option<String>,
    pub remote_url: Option<String>,
    pub last_activity_ms: Option<i64>,
    pub ongoing_count: usize,
    pub tools_14d: Vec<ToolUsage>,
    /// Latest visible run.
    pub last_run: Option<RunCard>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectDetail {
    pub card: ProjectCard,
    pub ongoings: Vec<Ongoing>,
}

struct ProjectRow {
    id: i64,
    name: String,
    path: String,
    remote_url: Option<String>,
    last_activity_ms: Option<i64>,
}

fn project_rows(conn: &Connection, only: Option<i64>) -> Result<Vec<ProjectRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, remote_url, last_activity_at FROM projects \
         WHERE ?1 IS NULL OR id = ?1 \
         ORDER BY last_activity_at IS NULL, last_activity_at DESC, id",
    )?;
    let rows = stmt
        .query_map([only], |r| {
            Ok(ProjectRow {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                remote_url: r.get(3)?,
                last_activity_ms: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Tools used since `since_ms` per project, with whether any run changed files.
fn recent_tools(
    conn: &Connection,
    since_ms: i64,
    only: Option<i64>,
) -> Result<HashMap<i64, Vec<ToolUsage>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.project_id, s.tool, COALESCE(MAX(r.changed_files), 0) \
         FROM session_runs r JOIN ai_sessions s ON s.id = r.ai_session_id \
         WHERE {VISIBLE_SESSION_SQL} AND r.started_at >= ?1 AND (?2 IS NULL OR s.project_id = ?2) \
         GROUP BY s.project_id, s.tool ORDER BY s.project_id, s.tool"
    ))?;
    let rows = stmt
        .query_map(rusqlite::params![since_ms, only], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut map: HashMap<i64, Vec<ToolUsage>> = HashMap::new();
    for (project, tool, changed_files) in rows {
        map.entry(project).or_default().push(ToolUsage {
            tool: tool.parse()?,
            changed_files,
        });
    }
    Ok(map)
}

/// Latest visible run of each project (`row_number` over the project), by project id.
fn last_runs(conn: &Connection, only: Option<i64>) -> Result<HashMap<i64, RunCard>> {
    let sql = format!(
        "{CARD_SELECT} WHERE r.id IN (\
            SELECT run_id FROM (\
                SELECT r2.id AS run_id, row_number() OVER (\
                    PARTITION BY s2.project_id ORDER BY r2.started_at DESC, r2.id DESC) AS rn \
                FROM session_runs r2 JOIN ai_sessions s2 ON s2.id = r2.ai_session_id \
                WHERE s2.origin = 'interactive' AND s2.is_empty = 0 \
                  AND (?1 IS NULL OR s2.project_id = ?1)) \
            WHERE rn = 1)"
    );
    Ok(query_cards(conn, &sql, &[&only])?
        .into_iter()
        .map(|c| (c.project_id, c))
        .collect())
}

fn build(conn: &Connection, only: Option<i64>, now_ms: i64) -> Result<Vec<ProjectCard>> {
    let rows = project_rows(conn, only)?;
    let mut tools = recent_tools(conn, now_ms - RECENT_DAYS * DAY_MS, only)?;
    let mut last = last_runs(conn, only)?;
    Ok(rows
        .into_iter()
        .map(|p| ProjectCard {
            ongoing_count: ongoings::for_project(p.id, &p.name, &p.path).len(),
            tools_14d: tools.remove(&p.id).unwrap_or_default(),
            last_run: last.remove(&p.id),
            id: p.id,
            name: p.name,
            path: (!p.path.is_empty()).then_some(p.path),
            remote_url: p.remote_url,
            last_activity_ms: p.last_activity_ms,
        })
        .collect())
}

/// Every project, most recently active first, projects without activity last (Inv §1.2).
pub fn list(conn: &Connection, now_ms: i64) -> Result<Vec<ProjectCard>> {
    build(conn, None, now_ms)
}

/// One project with its ongoing documents, or `None` when it does not exist.
pub fn detail(conn: &Connection, id: i64, now_ms: i64) -> Result<Option<ProjectDetail>> {
    let Some(card) = build(conn, Some(id), now_ms)?.pop() else {
        return Ok(None);
    };
    let ongoings = match &card.path {
        Some(path) => ongoings::for_project(card.id, &card.name, path),
        None => Vec::new(),
    };
    Ok(Some(ProjectDetail { card, ongoings }))
}
