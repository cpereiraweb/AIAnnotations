//! Run cards and the paginated run lists (Sessions screen, Inv §1.4; project detail, Inv §1.3).

use std::collections::{HashMap, HashSet};

use aiannotations_core::Result;
use aiannotations_core::model::{EndReason, SummaryStatus, Tool, VISIBLE_SESSION_SQL};
use rusqlite::{Connection, ToSql, params_from_iter};
use serde::Serialize;

pub const SESSIONS_PER_PAGE: u32 = 30;
pub const PROJECT_RUNS_PER_PAGE: u32 = 20;

/// Why a session is not shown by default (Inv §1.3); `Empty` wins over the origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenLabel {
    Empty,
    Automation,
    Subagent,
    Internal,
}

/// Filter of the Sessions screen on what the run did to the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// `changed_files = true`.
    Changed,
    /// `changed_files` false or unknown.
    Talk,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionFilter {
    pub tool: Option<Tool>,
    pub project_id: Option<i64>,
    pub kind: Option<RunKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub pages: u32,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunCard {
    pub run_id: i64,
    pub session_id: i64,
    pub tool: Tool,
    pub tool_session_id: String,
    pub project_id: i64,
    pub project_name: String,
    pub cwd: Option<String>,
    pub branch: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub next_step: Option<String>,
    pub summary_status: SummaryStatus,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub end_reason: Option<EndReason>,
    pub ended_cleanly: bool,
    pub changed_files: Option<bool>,
    pub files_count: u32,
    pub hidden_label: Option<HiddenLabel>,
    /// First session of the `/clear` chain (`previous_session_id`); the session itself when it
    /// has no predecessor. Visual grouping is the frontend's job.
    pub root_session_id: i64,
}

/// Columns of [`RunCard`], in the order [`read_card`] expects.
pub(crate) const CARD_SELECT: &str = "\
    SELECT r.id, s.id, s.tool, s.session_id, s.project_id, p.name, s.cwd, r.branch, s.title, \
           r.summary, r.next_step, r.summary_status, r.started_at, r.ended_at, r.end_reason, \
           r.ended_cleanly, r.changed_files, r.files_touched, s.origin, s.is_empty, \
           s.previous_session_id \
    FROM session_runs r \
    JOIN ai_sessions s ON s.id = r.ai_session_id \
    JOIN projects p ON p.id = s.project_id";

/// A card plus the id of the session before it in its chain.
pub(crate) struct RawCard {
    card: RunCard,
    previous: Option<i64>,
}

pub(crate) fn read_card(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawCard> {
    let tool: String = r.get(2)?;
    let summary_status: String = r.get(11)?;
    let end_reason: Option<String> = r.get(14)?;
    let files: Option<String> = r.get(17)?;
    let origin: String = r.get(18)?;
    let is_empty: bool = r.get(19)?;
    let cwd: String = r.get(6)?;
    let bad = |e: aiannotations_core::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    };
    let hidden_label = if is_empty {
        Some(HiddenLabel::Empty)
    } else {
        match origin.as_str() {
            "automation" => Some(HiddenLabel::Automation),
            "subagent" => Some(HiddenLabel::Subagent),
            "internal" => Some(HiddenLabel::Internal),
            _ => None,
        }
    };
    Ok(RawCard {
        card: RunCard {
            run_id: r.get(0)?,
            session_id: r.get(1)?,
            tool: tool.parse().map_err(bad)?,
            tool_session_id: r.get(3)?,
            project_id: r.get(4)?,
            project_name: r.get(5)?,
            cwd: (!cwd.is_empty()).then_some(cwd),
            branch: r.get(7)?,
            title: r.get(8)?,
            summary: r.get(9)?,
            next_step: r.get(10)?,
            summary_status: summary_status.parse().map_err(bad)?,
            started_at_ms: r.get(12)?,
            ended_at_ms: r.get(13)?,
            end_reason: end_reason.and_then(|s| s.parse().ok()),
            ended_cleanly: r.get::<_, Option<bool>>(15)?.unwrap_or(false),
            changed_files: r.get(16)?,
            files_count: files
                .and_then(|f| serde_json::from_str::<Vec<serde_json::Value>>(&f).ok())
                .map_or(0, |v| v.len() as u32),
            hidden_label,
            root_session_id: r.get(1)?,
        },
        previous: r.get(20)?,
    })
}

/// Runs `sql` and fills `root_session_id` of every card with one extra query for the whole
/// result (the `previous_session_id` edges of the projects involved).
pub(crate) fn query_cards(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<RunCard>> {
    let mut stmt = conn.prepare(sql)?;
    let raw = stmt
        .query_map(params_from_iter(params.iter().copied()), read_card)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if raw.iter().all(|c| c.previous.is_none()) {
        return Ok(raw.into_iter().map(|c| c.card).collect());
    }
    let projects: HashSet<i64> = raw.iter().map(|c| c.card.project_id).collect();
    let previous = previous_edges(conn, &projects)?;
    Ok(raw
        .into_iter()
        .map(|mut c| {
            c.card.root_session_id = root_of(&previous, c.card.session_id);
            c.card
        })
        .collect())
}

/// `id -> previous_session_id` for the sessions of the given projects.
fn previous_edges(conn: &Connection, projects: &HashSet<i64>) -> Result<HashMap<i64, i64>> {
    let marks = vec!["?"; projects.len()].join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT id, previous_session_id FROM ai_sessions \
         WHERE previous_session_id IS NOT NULL AND project_id IN ({marks})"
    ))?;
    let edges = stmt
        .query_map(params_from_iter(projects.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(edges)
}

/// Follows the chain back to its first session; a cycle stops where it closes.
fn root_of(previous: &HashMap<i64, i64>, mut id: i64) -> i64 {
    let mut seen = HashSet::from([id]);
    while let Some(&prev) = previous.get(&id) {
        if !seen.insert(prev) {
            break;
        }
        id = prev;
    }
    id
}

fn paginate(
    conn: &Connection,
    where_sql: &str,
    params: &[&dyn ToSql],
    per_page: u32,
    page: u32,
) -> Result<Page<RunCard>> {
    let page = page.max(1);
    let total: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM session_runs r \
             JOIN ai_sessions s ON s.id = r.ai_session_id WHERE {where_sql}"
        ),
        params_from_iter(params.iter().copied()),
        |r| r.get(0),
    )?;
    let total = total as u64;
    let sql = format!(
        "{CARD_SELECT} WHERE {where_sql} ORDER BY r.started_at DESC, r.id DESC LIMIT {per_page} OFFSET {}",
        u64::from(page - 1) * u64::from(per_page)
    );
    Ok(Page {
        items: query_cards(conn, &sql, params)?,
        page,
        pages: total.div_ceil(u64::from(per_page)).max(1) as u32,
        total,
    })
}

/// Visible runs, newest first, 30 per page (Inv §1.4).
pub fn sessions(conn: &Connection, filter: &SessionFilter, page: u32) -> Result<Page<RunCard>> {
    let mut clauses = vec![VISIBLE_SESSION_SQL.to_string()];
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(tool) = filter.tool {
        clauses.push("s.tool = ?".into());
        params.push(Box::new(tool.as_str()));
    }
    if let Some(project_id) = filter.project_id {
        clauses.push("s.project_id = ?".into());
        params.push(Box::new(project_id));
    }
    match filter.kind {
        Some(RunKind::Changed) => clauses.push("r.changed_files = 1".into()),
        Some(RunKind::Talk) => clauses.push("COALESCE(r.changed_files, 0) = 0".into()),
        None => {}
    }
    let refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
    paginate(conn, &clauses.join(" AND "), &refs, SESSIONS_PER_PAGE, page)
}

/// Runs of one project, newest first, 20 per page (Inv §1.3). `include_hidden` adds empty,
/// automation, subagent and internal sessions, each with its [`HiddenLabel`].
pub fn project_runs(
    conn: &Connection,
    project_id: i64,
    include_hidden: bool,
    page: u32,
) -> Result<Page<RunCard>> {
    let mut where_sql = "s.project_id = ?".to_string();
    if !include_hidden {
        where_sql.push_str(&format!(" AND {VISIBLE_SESSION_SQL}"));
    }
    paginate(
        conn,
        &where_sql,
        &[&project_id],
        PROJECT_RUNS_PER_PAGE,
        page,
    )
}
