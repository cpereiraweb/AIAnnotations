//! Daily AI briefing (Inv §6). Port of `BriefingGenerator.php` and `BriefingCommand.php`.
//!
//! One row per local day. The AI call is slow, so no transaction is open across it: the day's
//! row, the lock and the `generating` write (with the window) each take a short `write_tx`, the
//! prompt is built from plain reads, and the result is one more short write. The lock is
//! released in its own transaction on every path. Any AI failure marks the briefing `failed`
//! (text cleared) and never pauses the summary queue; a database error does too (best effort)
//! and is then returned. The core never reads the clock: `clock` is called once for the
//! window's end and again after the AI call to stamp the result.

pub mod prompt;

use std::collections::HashMap;
use std::str::FromStr;

use chrono::TimeZone;
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ai::{AiClient, ModelRole};
use crate::db::{self, app_state};
use crate::model::{BriefingStatus, Tool, VISIBLE_SESSION_SQL};
use crate::settings::Lang;
use crate::{Error, Result, Settings};
use prompt::strip_data_tags;

/// `app_state` lock held while a briefing is being generated.
pub const LOCK_KEY: &str = "briefing:running";

const TEXT_LIMIT: usize = 2000;
const DAY_MS: i64 = 86_400_000;

/// One next step, in the v1 JSON shape (`project_id`, `projeto`, `passo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextStep {
    pub project_id: i64,
    #[serde(rename = "projeto")]
    pub project: String,
    #[serde(rename = "passo")]
    pub step: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Briefing {
    pub id: i64,
    pub date: String,
    pub status: BriefingStatus,
    pub body: Option<String>,
    pub next_steps: Vec<NextStep>,
    pub covers_from_ms: i64,
    pub covers_until_ms: i64,
    pub generated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingOutcome {
    AlreadyDone,
    AlreadyRunning,
    Generated,
    Failed,
}

/// Body of a briefing with no sessions in its window.
pub fn empty_body(lang: Lang) -> &'static str {
    match lang {
        Lang::PtBr => "Nenhuma sessão desde o último briefing.",
        Lang::En => "No sessions since the last briefing.",
    }
}

const COLUMNS: &str = "id, date, status, body, next_steps, covers_from, covers_until, generated_at";

fn from_row(r: &Row) -> rusqlite::Result<Briefing> {
    let status: String = r.get(2)?;
    let steps: Option<String> = r.get(4)?;
    Ok(Briefing {
        id: r.get(0)?,
        date: r.get(1)?,
        // An unknown status can only come from a foreign writer; show it as failed.
        status: BriefingStatus::from_str(&status).unwrap_or(BriefingStatus::Failed),
        body: r.get(3)?,
        next_steps: steps.as_deref().map(parse_steps).unwrap_or_default(),
        covers_from_ms: r.get(5)?,
        covers_until_ms: r.get(6)?,
        generated_at_ms: r.get(7)?,
    })
}

/// Steps that do not fit the shape (e.g. an imported `project_id` cleared to null) are skipped.
fn parse_steps(json: &str) -> Vec<NextStep> {
    match serde_json::from_str::<Vec<Value>>(json) {
        Ok(items) => items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn local_date(tz: Tz, now_ms: i64) -> Result<String> {
    tz.timestamp_millis_opt(now_ms)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .ok_or_else(|| Error::InvalidEvent(format!("instant out of range: {now_ms}")))
}

fn by_date(conn: &Connection, date: &str) -> Result<Option<Briefing>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM briefings WHERE date = ?1"),
            [date],
            from_row,
        )
        .optional()?)
}

/// Start of the window: `covers_until` of the latest `done` briefing other than `current`,
/// else 24 h before `now_ms`.
fn window_from(conn: &Connection, current: Option<i64>, now_ms: i64) -> Result<i64> {
    let previous: Option<i64> = conn
        .query_row(
            "SELECT covers_until FROM briefings WHERE status = 'done' AND id IS NOT ?1 \
             ORDER BY covers_until DESC LIMIT 1",
            [current],
            |r| r.get(0),
        )
        .optional()?;
    Ok(previous.unwrap_or(now_ms - DAY_MS))
}

/// Creates today's briefing as `generating` if there is none; generates nothing.
pub fn ensure_for_today(conn: &mut Connection, tz: Tz, now_ms: i64) -> Result<Briefing> {
    let date = local_date(tz, now_ms)?;
    let tx = db::write_tx(conn)?;
    if by_date(&tx, &date)?.is_none() {
        let from = window_from(&tx, None, now_ms)?;
        tx.execute(
            "INSERT INTO briefings (date, status, covers_from, covers_until, created_at, updated_at) \
             VALUES (?1, 'generating', ?2, ?3, ?4, ?4)",
            params![date, from, now_ms, now_ms],
        )?;
    }
    let briefing = by_date(&tx, &date)?.expect("row just ensured");
    tx.commit()?;
    Ok(briefing)
}

/// Today's briefing, if it exists.
pub fn today(conn: &Connection, tz: Tz, now_ms: i64) -> Result<Option<Briefing>> {
    by_date(conn, &local_date(tz, now_ms)?)
}

/// Whether a generation is holding the lock.
pub fn is_running(conn: &Connection, now_ms: i64) -> Result<bool> {
    Ok(app_state::get(conn, LOCK_KEY, now_ms)?.is_some())
}

/// Generates today's briefing (`BriefingCommand`). `inbox_titles` are the inbox items' titles,
/// built by the caller.
pub fn run(
    conn: &mut Connection,
    ai: &dyn AiClient,
    inbox_titles: &[String],
    settings: &Settings,
    tz: Tz,
    force: bool,
    clock: &dyn Fn() -> i64,
) -> Result<BriefingOutcome> {
    let start = clock();
    let briefing = ensure_for_today(conn, tz, start)?;
    if briefing.status == BriefingStatus::Done && !force {
        return Ok(BriefingOutcome::AlreadyDone);
    }
    let ttl = i64::from(settings.briefing_lock_seconds) * 1000;
    let locked = {
        let tx = db::write_tx(conn)?;
        let locked = app_state::try_lock(&tx, LOCK_KEY, ttl, start)?;
        tx.commit()?;
        locked
    };
    if !locked {
        return Ok(BriefingOutcome::AlreadyRunning);
    }
    let result = generate(
        conn,
        ai,
        inbox_titles,
        settings,
        tz,
        &briefing,
        start,
        clock,
    );
    if result.is_err() {
        // Like the v1 on any Throwable: never leave the row `generating`. Best effort, the
        // original error is the one returned.
        let _ = finish(conn, briefing.id, None, clock(), BriefingOutcome::Failed);
    }
    let released = db::write_tx(conn).and_then(|tx| {
        app_state::unlock(&tx, LOCK_KEY)?;
        Ok(tx.commit()?)
    });
    let outcome = result?;
    released?;
    Ok(outcome)
}

struct RunRow {
    started_at: i64,
    ai_session_id: i64,
    tool: String,
    title: Option<String>,
    project: String,
    summary: Option<String>,
    next_step: Option<String>,
    unfinished: bool,
}

#[allow(clippy::too_many_arguments)]
fn generate(
    conn: &mut Connection,
    ai: &dyn AiClient,
    inbox_titles: &[String],
    settings: &Settings,
    tz: Tz,
    briefing: &Briefing,
    until: i64,
    clock: &dyn Fn() -> i64,
) -> Result<BriefingOutcome> {
    let from = {
        let tx = db::write_tx(conn)?;
        let from = window_from(&tx, Some(briefing.id), until)?;
        tx.execute(
            "UPDATE briefings SET status = 'generating', covers_from = ?1, covers_until = ?2, \
             updated_at = ?3 WHERE id = ?4",
            params![from, until, until, briefing.id],
        )?;
        tx.commit()?;
        from
    };

    let runs = load_runs(conn, from, until)?;
    if runs.is_empty() {
        let now = clock();
        return finish(
            conn,
            briefing.id,
            Some((empty_body(settings.lang), "[]")),
            now,
            BriefingOutcome::Generated,
        );
    }

    let text = prompt_data(&runs, settings, tz, from, until, inbox_titles);
    // No transaction is open here.
    let reply = ai.complete_json(&text, ModelRole::Briefing);
    let now = clock();
    let composed = reply.ok().and_then(|data| validate(&data));
    match composed {
        Some((body, steps)) => {
            let steps = map_steps(&steps, &project_ids(conn)?);
            let json = serde_json::to_string(&steps)?;
            finish(
                conn,
                briefing.id,
                Some((&body, &json)),
                now,
                BriefingOutcome::Generated,
            )
        }
        None => finish(conn, briefing.id, None, now, BriefingOutcome::Failed),
    }
}

/// Writes the result: `Some((body, next_steps))` is `done`, `None` is `failed` with both cleared.
fn finish(
    conn: &mut Connection,
    id: i64,
    content: Option<(&str, &str)>,
    now: i64,
    outcome: BriefingOutcome,
) -> Result<BriefingOutcome> {
    let tx = db::write_tx(conn)?;
    let (status, body, steps) = match content {
        Some((b, s)) => ("done", Some(b), Some(s)),
        None => ("failed", None, None),
    };
    tx.execute(
        "UPDATE briefings SET status = ?1, body = ?2, next_steps = ?3, generated_at = ?4, \
         updated_at = ?4 WHERE id = ?5",
        params![status, body, steps, now, id],
    )?;
    tx.commit()?;
    Ok(outcome)
}

fn load_runs(conn: &Connection, from: i64, until: i64) -> Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT r.started_at, r.ai_session_id, s.tool, s.title, p.name, r.summary, r.next_step, \
         COALESCE(r.unfinished, 0) \
         FROM session_runs r \
         JOIN ai_sessions s ON s.id = r.ai_session_id \
         JOIN projects p ON p.id = s.project_id \
         WHERE r.ai_session_id IN (SELECT id FROM ai_sessions WHERE {VISIBLE_SESSION_SQL}) \
         AND r.started_at > ?1 AND r.started_at <= ?2 \
         ORDER BY r.started_at, r.id"
    ))?;
    let rows = stmt
        .query_map(params![from, until], |r| {
            Ok(RunRow {
                started_at: r.get(0)?,
                ai_session_id: r.get(1)?,
                tool: r.get(2)?,
                title: r.get(3)?,
                project: r.get(4)?,
                summary: r.get(5)?,
                next_step: r.get(6)?,
                unfinished: r.get::<_, i64>(7)? != 0,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Distinct-session counts per key, in order of first appearance.
fn count_sessions<'a>(runs: impl Iterator<Item = (String, &'a RunRow)>) -> Vec<(String, usize)> {
    let mut order: Vec<(String, std::collections::HashSet<i64>)> = Vec::new();
    for (key, run) in runs {
        match order.iter_mut().find(|(k, _)| *k == key) {
            Some((_, set)) => {
                set.insert(run.ai_session_id);
            }
            None => order.push((key, [run.ai_session_id].into())),
        }
    }
    order.into_iter().map(|(k, s)| (k, s.len())).collect()
}

fn format_window(tz: Tz, lang: Lang, from: i64, until: i64) -> String {
    let (fmt, sep) = match lang {
        Lang::PtBr => ("%d/%m/%Y %H:%M", " a "),
        Lang::En => ("%Y-%m-%d %H:%M", " to "),
    };
    let show = |ms: i64| {
        tz.timestamp_millis_opt(ms)
            .single()
            .map(|d| d.format(fmt).to_string())
            .unwrap_or_default()
    };
    format!("{}{sep}{}", show(from), show(until))
}

fn prompt_data(
    runs: &[RunRow],
    settings: &Settings,
    tz: Tz,
    from: i64,
    until: i64,
    inbox_titles: &[String],
) -> String {
    let lang = settings.lang;
    let (no_project, no_title, no_summary, next_label) = match lang {
        Lang::PtBr => (
            "(sem projeto)",
            "(sem título)",
            "(sem resumo)",
            "próximo passo",
        ),
        Lang::En => ("(no project)", "(no title)", "(no summary)", "next step"),
    };
    let project_of = |r: &RunRow| {
        let name = strip_data_tags(&r.project);
        if name.trim().is_empty() {
            no_project.to_string()
        } else {
            name
        }
    };
    let by_tool = count_sessions(runs.iter().map(|r| {
        let label = r
            .tool
            .parse::<Tool>()
            .map_or_else(|_| r.tool.clone(), |t| t.label().to_string());
        (label, r)
    }));
    let by_project = count_sessions(runs.iter().map(|r| (project_of(r), r)));
    let join = |items: &[(String, usize)]| {
        items
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let per_project = by_project
        .iter()
        .map(|(name, _)| {
            let mut group: Vec<&RunRow> = runs.iter().filter(|r| project_of(r) == *name).collect();
            // Most recent first (the query order breaks ties by id, so the later one wins).
            group.reverse();
            group.sort_by_key(|r| std::cmp::Reverse(r.started_at));
            let lines: Vec<String> = group
                .into_iter()
                .take(settings.briefing_runs_per_project as usize)
                .map(|r| {
                    let title = strip_data_tags(r.title.as_deref().unwrap_or(no_title));
                    let summary = strip_data_tags(r.summary.as_deref().unwrap_or(no_summary));
                    let mut line = format!("- {title}: {summary}");
                    if let Some(step) = r.next_step.as_deref().filter(|s| !s.is_empty()) {
                        line.push_str(&format!(" | {next_label}: {}", strip_data_tags(step)));
                    }
                    line
                })
                .collect();
            format!("{name}\n{}", lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let inbox = inbox_titles
        .iter()
        .map(|t| format!("- {}", strip_data_tags(t)))
        .collect::<Vec<_>>()
        .join("\n");
    let unfinished = runs.iter().filter(|r| r.unfinished).count();
    prompt::build(
        lang,
        &prompt::Data {
            window: &format_window(tz, lang, from, until),
            by_tool: &join(&by_tool),
            by_project: &join(&by_project),
            unfinished,
            per_project: &per_project,
            inbox: &inbox,
        },
    )
}

/// `paragraph` (non-empty string) and `next_steps` (array, absent = empty) of the reply.
fn validate(data: &Map<String, Value>) -> Option<(String, Vec<Value>)> {
    let body = data.get("paragraph")?.as_str()?.trim();
    if body.is_empty() {
        return None;
    }
    let steps = match data.get("next_steps") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return None,
    };
    Some((truncate(body), steps))
}

fn truncate(text: &str) -> String {
    text.chars().take(TEXT_LIMIT).collect()
}

/// (id, name) by lowercase trimmed name; the project active most recently wins (nulls last,
/// then the greater id).
fn project_ids(conn: &Connection) -> Result<HashMap<String, (i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, name FROM projects \
         ORDER BY last_activity_at IS NULL, last_activity_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, name) = row?;
        map.entry(name.trim().to_lowercase()).or_insert((id, name));
    }
    Ok(map)
}

fn map_steps(items: &[Value], projects: &HashMap<String, (i64, String)>) -> Vec<NextStep> {
    let text = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    items
        .iter()
        .filter_map(|item| {
            let (id, name) = projects.get(&text(item, "project").to_lowercase())?;
            let step = text(item, "step");
            (!step.is_empty()).then(|| NextStep {
                project_id: *id,
                project: name.clone(),
                step: truncate(&step),
            })
        })
        .collect()
}
