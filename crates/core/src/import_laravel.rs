//! Import of the Laravel v1 database (spec §10.6): projects, sessions, runs, inbox dismissals
//! and briefings, column by column (same names, Inv §2.1), in one `IMMEDIATE` transaction.
//!
//! - The Laravel database is opened read-only through a `file:…?mode=ro` URI, so a live app's
//!   `-wal` is seen and nothing is ever written to it. Every table is read inside one read
//!   transaction (a consistent snapshot while the Laravel app keeps writing).
//! - Laravel datetimes are naive `YYYY-MM-DD HH:MM:SS` text in the app's zone (Inv §2.3) and
//!   become ms UTC; `transcript_mtime` is already ms and `briefings.date` stays a date.
//! - Ids are new: `previous_session_id`, `project_id`, `ai_session_id`, the dismissal keys and the
//!   briefing next steps' `project_id` are remapped.
//! - A row whose enum column (`tool`, `origin`, `end_reason`, `summary_status`) holds a value the
//!   model does not know is left out with the reason, and so are the rows that depend on it.
//! - Refused when the target already has sessions, unless `merge` (records collected before the
//!   import would otherwise be matched to Laravel's only by exact keys).
//! - Idempotent (a re-run needs `merge`): a record already present (project by `key`, session by `(tool, session_id)`, run
//!   by `(ai_session_id, started_at)`, dismissal by `item_key`, briefing by `date`) is kept as it
//!   is and not counted. `users`, `cache`, `jobs`, `sessions` and the rest are ignored.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use chrono::{NaiveDateTime, Offset, TimeDelta, TimeZone};
use chrono_tz::Tz;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};

use crate::model::{EndReason, SessionOrigin, SummaryStatus, Tool};
use crate::transcripts::codex_history;
use crate::{Error, Result, db};

/// The Laravel app's fixed zone (`config/app.php`, Inv §2.3).
pub const LARAVEL_TZ: Tz = chrono_tz::America::Sao_Paulo;

/// How to import: the zone of the Laravel datetimes and whether a target that already holds
/// sessions may be imported into.
#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    pub tz: Tz,
    /// Import into a target that already has `ai_sessions` (a collect or hook ran first). Records
    /// already present are kept; runs are matched only by exact `started_at`.
    pub merge: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            tz: LARAVEL_TZ,
            merge: false,
        }
    }
}

/// Records created by the import (records already present are not counted) and what was left
/// out, one line per record with the reason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub projects: usize,
    pub sessions: usize,
    pub runs: usize,
    pub dismissals: usize,
    pub briefings: usize,
    pub skipped: Vec<String>,
}

struct Table {
    name: &'static str,
    columns: &'static [&'static str],
    datetimes: &'static [&'static str],
    /// Columns holding a model enum (Inv §2.2): a row with any other value is left out.
    enums: &'static [EnumColumn],
}

struct EnumColumn {
    name: &'static str,
    nullable: bool,
    valid: fn(&str) -> bool,
}

fn is<T: FromStr>(s: &str) -> bool {
    s.parse::<T>().is_ok()
}

const PROJECTS: Table = Table {
    name: "projects",
    columns: &[
        "id",
        "key",
        "name",
        "path",
        "remote_url",
        "last_activity_at",
        "created_at",
        "updated_at",
    ],
    datetimes: &["last_activity_at", "created_at", "updated_at"],
    enums: &[],
};

const SESSIONS: Table = Table {
    name: "ai_sessions",
    columns: &[
        "id",
        "project_id",
        "tool",
        "session_id",
        "cwd",
        "worktree_path",
        "transcript_path",
        "transcript_mtime",
        "model",
        "title",
        "origin",
        "previous_session_id",
        "start_source",
        "parent_session_id",
        "is_empty",
        "last_activity_at",
        "created_at",
        "updated_at",
    ],
    datetimes: &["last_activity_at", "created_at", "updated_at"],
    enums: &[
        EnumColumn {
            name: "tool",
            nullable: false,
            valid: is::<Tool>,
        },
        EnumColumn {
            name: "origin",
            nullable: false,
            valid: is::<SessionOrigin>,
        },
    ],
};

const RUNS: Table = Table {
    name: "session_runs",
    columns: &[
        "id",
        "ai_session_id",
        "started_at",
        "ended_at",
        "end_reason",
        "ended_cleanly",
        "branch",
        "head_start",
        "head_end",
        "status_hash_start",
        "status_hash_end",
        "changed_files",
        "files_touched",
        "summary_status",
        "summary",
        "next_step",
        "unfinished",
        "kind",
        "summary_attempts",
        "summarized_fingerprint",
        "summary_available_at",
        "is_backfill",
        "discovered",
        "created_at",
        "updated_at",
    ],
    datetimes: &[
        "started_at",
        "ended_at",
        "summary_available_at",
        "created_at",
        "updated_at",
    ],
    enums: &[
        EnumColumn {
            name: "end_reason",
            nullable: true,
            valid: is::<EndReason>,
        },
        EnumColumn {
            name: "summary_status",
            nullable: false,
            valid: is::<SummaryStatus>,
        },
    ],
};

const DISMISSALS: Table = Table {
    name: "inbox_dismissals",
    columns: &["id", "item_key", "state_hash", "created_at", "updated_at"],
    datetimes: &["created_at", "updated_at"],
    enums: &[],
};

const BRIEFINGS: Table = Table {
    name: "briefings",
    columns: &[
        "id",
        "date",
        "status",
        "body",
        "next_steps",
        "covers_from",
        "covers_until",
        "generated_at",
        "created_at",
        "updated_at",
    ],
    datetimes: &[
        "covers_from",
        "covers_until",
        "generated_at",
        "created_at",
        "updated_at",
    ],
    enums: &[],
};

/// Inbox kinds whose `item_key` is `<kind>:<project_id>[:rest]` (`app/Inbox/Rules/*.php`).
const DISMISSAL_KINDS: [&str; 6] = [
    "interrupted",
    "dirty",
    "stale-branch",
    "ongoing",
    "reminder",
    "worktree",
];

/// One Laravel row, datetimes already converted.
struct Row {
    table: &'static Table,
    values: Vec<Value>,
}

impl Row {
    fn get(&self, column: &str) -> &Value {
        let i = self
            .table
            .columns
            .iter()
            .position(|c| *c == column)
            .expect("column listed in its table");
        &self.values[i]
    }

    fn int(&self, column: &str) -> Option<i64> {
        match self.get(column) {
            Value::Integer(i) => Some(*i),
            Value::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }

    fn id(&self) -> i64 {
        self.int("id").unwrap_or_default()
    }

    fn text(&self, column: &str) -> String {
        match self.get(column) {
            Value::Text(t) => t.clone(),
            Value::Integer(i) => i.to_string(),
            _ => String::new(),
        }
    }
}

struct Source {
    projects: Vec<Row>,
    sessions: Vec<Row>,
    runs: Vec<Row>,
    dismissals: Vec<Row>,
    briefings: Vec<Row>,
    /// Rows left out while reading (unknown enum value), with the reason.
    skipped: Vec<String>,
}

/// Imports the Laravel database at `laravel_db` into `conn`.
pub fn run(
    laravel_db: &Path,
    conn: &mut Connection,
    options: &ImportOptions,
) -> Result<ImportReport> {
    if !laravel_db.is_file() {
        return Err(Error::Import(format!(
            "Laravel database not found: {}",
            laravel_db.display()
        )));
    }
    let source = read_source(laravel_db, options.tz)?;

    let mut report = ImportReport {
        skipped: source.skipped,
        ..ImportReport::default()
    };
    let tx = db::write_tx(conn)?;
    if !options.merge {
        let sessions: i64 = tx.query_row("SELECT COUNT(*) FROM ai_sessions", [], |r| r.get(0))?;
        if sessions > 0 {
            return Err(Error::Import(format!(
                "the target database already has {sessions} sessions; import before the first \
                 collect/hook run, or pass --merge"
            )));
        }
    }
    let projects = import_projects(&tx, &source.projects, &mut report)?;
    let sessions = import_sessions(&tx, &source.sessions, &projects, &mut report)?;
    let runs = import_runs(&tx, &source.runs, &sessions, &mut report)?;
    import_dismissals(&tx, &source.dismissals, &projects, &runs, &mut report)?;
    import_briefings(&tx, &source.briefings, &projects, &mut report)?;
    tx.commit()?;
    Ok(report)
}

fn read_source(path: &Path, tz: Tz) -> Result<Source> {
    let mut src = codex_history::connect(path)?;
    let snapshot = src.transaction()?;
    let mut skipped = Vec::new();
    let source = Source {
        projects: fetch(&snapshot, &PROJECTS, tz, &mut skipped)?,
        sessions: fetch(&snapshot, &SESSIONS, tz, &mut skipped)?,
        runs: fetch(&snapshot, &RUNS, tz, &mut skipped)?,
        dismissals: fetch(&snapshot, &DISMISSALS, tz, &mut skipped)?,
        briefings: fetch(&snapshot, &BRIEFINGS, tz, &mut skipped)?,
        skipped,
    };
    snapshot.commit()?;
    Ok(source)
}

/// Rows of `table` with datetimes converted; a row with an unknown enum value goes to `skipped`.
fn fetch(
    src: &Connection,
    table: &'static Table,
    tz: Tz,
    skipped: &mut Vec<String>,
) -> Result<Vec<Row>> {
    let sql = format!(
        "SELECT {} FROM {} ORDER BY id",
        table.columns.join(", "),
        table.name
    );
    let unreadable = |e: rusqlite::Error| Error::Import(format!("{}: {e}", table.name));
    let mut stmt = src.prepare(&sql).map_err(unreadable)?;
    let raw: Vec<Vec<Value>> = stmt
        .query_map([], |r| {
            (0..table.columns.len())
                .map(|i| r.get::<_, Value>(i))
                .collect()
        })
        .map_err(unreadable)?
        .collect::<rusqlite::Result<_>>()
        .map_err(unreadable)?;

    let mut rows = Vec::with_capacity(raw.len());
    for mut values in raw {
        if let Some(reason) = invalid_enum(table, &values) {
            skipped.push(format!("{} #{}: {reason}", table.name, show(&values[0])));
            continue;
        }
        for (i, column) in table.columns.iter().enumerate() {
            if table.datetimes.contains(column) {
                values[i] = convert_datetime(&values[i], tz).ok_or_else(|| {
                    Error::Import(format!(
                        "{} #{}: {column} '{}' is not a Laravel datetime",
                        table.name,
                        show(&values[0]),
                        show(&values[i]),
                    ))
                })?;
            }
        }
        rows.push(Row { table, values });
    }
    Ok(rows)
}

/// Why the row's enum columns cannot be imported, if they cannot.
fn invalid_enum(table: &Table, values: &[Value]) -> Option<String> {
    table.enums.iter().find_map(|e| {
        let i = table.columns.iter().position(|c| *c == e.name)?;
        let ok = match &values[i] {
            Value::Null => e.nullable,
            Value::Text(t) => (e.valid)(t),
            _ => false,
        };
        (!ok).then(|| format!("{} '{}' is not a known value", e.name, show(&values[i])))
    })
}

fn show(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(t) => t.clone(),
        Value::Blob(b) => format!("<{} bytes>", b.len()),
    }
}

/// Naive Laravel datetime in `tz` → ms UTC (`NULL` stays `NULL`). On a DST fall-back the
/// earliest instant wins; inside a spring-forward gap the offset in force before the gap is used
/// (what PHP does).
fn convert_datetime(v: &Value, tz: Tz) -> Option<Value> {
    match v {
        Value::Null => Some(Value::Null),
        Value::Text(s) => local_to_ms(s, tz).map(Value::Integer),
        _ => None,
    }
}

fn local_to_ms(s: &str, tz: Tz) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S%.f").ok()?;
    if let Some(t) = tz.from_local_datetime(&naive).earliest() {
        return Some(t.timestamp_millis());
    }
    let before = tz
        .offset_from_utc_datetime(&(naive - TimeDelta::days(1)))
        .fix();
    let utc = naive - TimeDelta::seconds(i64::from(before.local_minus_utc()));
    Some(utc.and_utc().timestamp_millis())
}

/// Inserts `row` into its table with a new id, `overrides` replacing some columns; returns the
/// new id.
fn insert(tx: &Transaction, row: &Row, overrides: &[(&str, Value)]) -> Result<i64> {
    let columns: Vec<&str> = row.table.columns[1..].to_vec();
    let values = columns.iter().map(|c| {
        overrides
            .iter()
            .find(|(name, _)| name == c)
            .map_or_else(|| row.get(c).clone(), |(_, v)| v.clone())
    });
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
    tx.execute(
        &format!(
            "INSERT INTO {} ({}) VALUES ({})",
            row.table.name,
            columns.join(", "),
            placeholders.join(", ")
        ),
        params_from_iter(values),
    )?;
    Ok(tx.last_insert_rowid())
}

type IdMap = HashMap<i64, i64>;

fn import_projects(tx: &Transaction, rows: &[Row], report: &mut ImportReport) -> Result<IdMap> {
    let mut map = IdMap::new();
    for row in rows {
        let existing = tx
            .query_row(
                "SELECT id FROM projects WHERE key = ?1",
                [row.text("key")],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                report.projects += 1;
                insert(tx, row, &[])?
            }
        };
        map.insert(row.id(), id);
    }
    Ok(map)
}

fn import_sessions(
    tx: &Transaction,
    rows: &[Row],
    projects: &IdMap,
    report: &mut ImportReport,
) -> Result<IdMap> {
    let mut map = IdMap::new();
    // (new id, Laravel id, Laravel previous_session_id) of the inserted sessions: the chain is
    // linked once every session has its new id (a previous session may come later).
    let mut links = Vec::new();
    for row in rows {
        let (tool, sid) = (row.text("tool"), row.text("session_id"));
        let Some(&project) = row.int("project_id").and_then(|p| projects.get(&p)) else {
            report.skipped.push(format!(
                "ai_sessions #{} ({tool} {sid}): project #{} not imported",
                row.id(),
                show(row.get("project_id"))
            ));
            continue;
        };
        let existing = tx
            .query_row(
                "SELECT id FROM ai_sessions WHERE tool = ?1 AND session_id = ?2",
                params![tool, sid],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id = insert(
                    tx,
                    row,
                    &[
                        ("project_id", Value::Integer(project)),
                        ("previous_session_id", Value::Null),
                    ],
                )?;
                if let Some(previous) = row.int("previous_session_id") {
                    links.push((id, row.id(), previous));
                }
                report.sessions += 1;
                id
            }
        };
        map.insert(row.id(), id);
    }
    for (id, old, previous) in links {
        match map.get(&previous) {
            Some(&p) => {
                tx.execute(
                    "UPDATE ai_sessions SET previous_session_id = ?1 WHERE id = ?2",
                    params![p, id],
                )?;
            }
            None => report.skipped.push(format!(
                "ai_sessions #{old}: previous session #{previous} not imported; link dropped"
            )),
        }
    }
    Ok(map)
}

fn import_runs(
    tx: &Transaction,
    rows: &[Row],
    sessions: &IdMap,
    report: &mut ImportReport,
) -> Result<IdMap> {
    let mut map = IdMap::new();
    for row in rows {
        let Some(&session) = row.int("ai_session_id").and_then(|s| sessions.get(&s)) else {
            report.skipped.push(format!(
                "session_runs #{}: session #{} not imported",
                row.id(),
                show(row.get("ai_session_id"))
            ));
            continue;
        };
        let existing = tx
            .query_row(
                "SELECT id FROM session_runs WHERE ai_session_id = ?1 AND started_at = ?2",
                params![session, row.get("started_at")],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id = insert(tx, row, &[("ai_session_id", Value::Integer(session))])?;
                report.runs += 1;
                id
            }
        };
        map.insert(row.id(), id);
    }
    Ok(map)
}

fn import_dismissals(
    tx: &Transaction,
    rows: &[Row],
    projects: &IdMap,
    runs: &IdMap,
    report: &mut ImportReport,
) -> Result<()> {
    for row in rows {
        let key = row.text("item_key");
        let (new_key, new_hash) =
            match remap_dismissal(&key, &row.text("state_hash"), projects, runs) {
                Ok(remapped) => remapped,
                Err(reason) => {
                    report
                        .skipped
                        .push(format!("inbox_dismissals #{} '{key}': {reason}", row.id()));
                    continue;
                }
            };
        let exists = tx
            .query_row(
                "SELECT 1 FROM inbox_dismissals WHERE item_key = ?1",
                [&new_key],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            insert(
                tx,
                row,
                &[
                    ("item_key", Value::Text(new_key)),
                    ("state_hash", Value::Text(new_hash)),
                ],
            )?;
            report.dismissals += 1;
        }
    }
    Ok(())
}

/// New `(item_key, state_hash)` of a dismissal, or why it cannot be carried over.
fn remap_dismissal(
    key: &str,
    hash: &str,
    projects: &IdMap,
    runs: &IdMap,
) -> std::result::Result<(String, String), String> {
    let mut parts = key.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    if !DISMISSAL_KINDS.contains(&kind) {
        return Err(format!("unknown kind '{kind}'"));
    }
    let project = parts
        .next()
        .and_then(|p| p.parse::<i64>().ok())
        .ok_or("no project id in the key")?;
    let new_project = projects
        .get(&project)
        .ok_or(format!("project #{project} not imported"))?;
    let new_key = match parts.next() {
        Some(rest) => format!("{kind}:{new_project}:{rest}"),
        None => format!("{kind}:{new_project}"),
    };
    if kind != "interrupted" {
        return Ok((new_key, hash.to_string()));
    }
    // `<run_id>:<ended_at seconds>` (InterruptedSessionsRule): the instant is UTC, only the id
    // changes.
    let (run, ended) = hash
        .split_once(':')
        .ok_or("state hash is not <run_id>:<ended_at>")?;
    let run = run
        .parse::<i64>()
        .map_err(|_| "state hash is not <run_id>:<ended_at>")?;
    let new_run = runs.get(&run).ok_or(format!("run #{run} not imported"))?;
    Ok((new_key, format!("{new_run}:{ended}")))
}

fn import_briefings(
    tx: &Transaction,
    rows: &[Row],
    projects: &IdMap,
    report: &mut ImportReport,
) -> Result<()> {
    for row in rows {
        let date = row.text("date");
        let exists = tx
            .query_row("SELECT 1 FROM briefings WHERE date = ?1", [&date], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if exists {
            continue;
        }
        let next_steps = remap_next_steps(row.get("next_steps"), projects, &date, report)?;
        insert(tx, row, &[("next_steps", next_steps)])?;
        report.briefings += 1;
    }
    Ok(())
}

/// `next_steps` is JSON `[{project_id, projeto, passo}]` (Inv §2.1): remaps `project_id`, clearing
/// it when the project was not imported. Anything else is copied as is.
fn remap_next_steps(
    v: &Value,
    projects: &IdMap,
    date: &str,
    report: &mut ImportReport,
) -> Result<Value> {
    let Value::Text(text) = v else {
        return Ok(v.clone());
    };
    let Ok(serde_json::Value::Array(mut steps)) = serde_json::from_str(text) else {
        return Ok(v.clone());
    };
    let mut changed = false;
    for step in &mut steps {
        let Some(obj) = step.as_object_mut() else {
            continue;
        };
        let Some(old) = obj.get("project_id").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        match projects.get(&old) {
            Some(&new) if new == old => {}
            Some(&new) => {
                obj.insert("project_id".into(), new.into());
                changed = true;
            }
            None => {
                obj.insert("project_id".into(), serde_json::Value::Null);
                changed = true;
                report.skipped.push(format!(
                    "briefings {date}: next step project #{old} not imported; project_id cleared"
                ));
            }
        }
    }
    Ok(if changed {
        Value::Text(serde_json::to_string(&steps)?)
    } else {
        v.clone()
    })
}
