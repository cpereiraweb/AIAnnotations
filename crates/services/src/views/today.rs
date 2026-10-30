//! The Today screen (Inv §1.1): briefing, ongoings, inbox and next steps. Reading only: it
//! creates today's briefing record but never generates it and never calls the AI.

use std::collections::HashMap;

use aiannotations_core::Result;
use aiannotations_core::Settings;
use aiannotations_core::briefing::{self, Briefing, NextStep};
use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, GitCache, GroupedInbox};
use aiannotations_core::model::BriefingStatus;
use aiannotations_core::ongoings::{self, Ongoing};
use aiannotations_core::summaries;
use chrono_tz::Tz;
use rusqlite::{Connection, ToSql};
use serde::Serialize;

use super::runs::{CARD_SELECT, RunCard, query_cards};

/// Ongoings untouched for longer than this go to the collapsed "stale" area (Inv §1.1).
pub const ONGOING_STALE_DAYS: i64 = 30;

const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Serialize)]
pub struct OngoingGroup {
    pub project_id: i64,
    pub project_name: String,
    pub items: Vec<Ongoing>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodayView {
    pub briefing: Option<Briefing>,
    pub briefing_running: bool,
    pub ongoings_recent: Vec<OngoingGroup>,
    pub ongoings_stale: Vec<OngoingGroup>,
    pub inbox: GroupedInbox,
    /// Last run of every session cited by the inbox, by `session_id`.
    pub inbox_last_runs: HashMap<i64, RunCard>,
    /// Only when the briefing is `done`.
    pub next_steps: Vec<NextStep>,
    /// `(done, total)` while a backfill still has runs to summarize.
    pub backfill: Option<(u64, u64)>,
    /// No session recorded yet: the screen offers "Importar histórico".
    pub is_empty_db: bool,
}

/// Loads the screen. Creates today's briefing record (as `generating`) if missing.
pub fn load(
    conn: &mut Connection,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    tz: Tz,
    now_ms: i64,
) -> Result<TodayView> {
    let briefing = briefing::ensure_for_today(conn, tz, now_ms)?;
    let conn: &Connection = conn;

    let items = inbox::build(conn, git, cache, settings, now_ms)?;
    let session_ids: Vec<i64> = items.iter().filter_map(|i| i.session_id).collect();
    let inbox = inbox::grouped(conn, items, settings, now_ms)?;

    let cutoff = now_ms - ONGOING_STALE_DAYS * DAY_MS;
    let (stale, recent): (Vec<_>, Vec<_>) = ongoings::for_projects(conn)?
        .into_iter()
        .partition(|o| o.modified_ms < cutoff);

    let next_steps = if briefing.status == BriefingStatus::Done {
        briefing.next_steps.clone()
    } else {
        Vec::new()
    };
    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM ai_sessions", [], |r| r.get(0))?;
    Ok(TodayView {
        briefing_running: briefing::is_running(conn, now_ms)?,
        briefing: Some(briefing),
        ongoings_recent: group(recent),
        ongoings_stale: group(stale),
        inbox,
        inbox_last_runs: last_runs(conn, &session_ids)?,
        next_steps,
        backfill: summaries::backfill_progress(conn)?,
        is_empty_db: sessions == 0,
    })
}

/// Groups by project keeping the input order (newest first) for groups and items.
fn group(list: Vec<Ongoing>) -> Vec<OngoingGroup> {
    let mut groups: Vec<OngoingGroup> = Vec::new();
    for o in list {
        match groups.iter_mut().find(|g| g.project_id == o.project_id) {
            Some(g) => g.items.push(o),
            None => groups.push(OngoingGroup {
                project_id: o.project_id,
                project_name: o.project_name.clone(),
                items: vec![o],
            }),
        }
    }
    groups
}

/// The latest run of each session, as a card, by session id.
fn last_runs(conn: &Connection, session_ids: &[i64]) -> Result<HashMap<i64, RunCard>> {
    if session_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let marks = vec!["?"; session_ids.len()].join(",");
    let sql = format!(
        "{CARD_SELECT} WHERE s.id IN ({marks}) AND r.id = (\
            SELECT r2.id FROM session_runs r2 WHERE r2.ai_session_id = s.id \
            ORDER BY r2.started_at DESC, r2.id DESC LIMIT 1)"
    );
    let params: Vec<&dyn ToSql> = session_ids.iter().map(|i| i as &dyn ToSql).collect();
    Ok(query_cards(conn, &sql, &params)?
        .into_iter()
        .map(|c| (c.session_id, c))
        .collect())
}
