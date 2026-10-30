//! SQLite connection, pragmas and schema migrations.

pub mod app_state;

use std::path::Path;

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::Result;

/// Ordered migrations; index + 1 is the `PRAGMA user_version` after applying it.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_search_triggers.sql"),
];

/// Opens (creating the parent directory if needed) the database, applies the
/// connection pragmas and pending migrations.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(path)?;
    // busy_timeout first so the pragmas/migrations below wait on a concurrent writer.
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Starts a write transaction with `BEGIN IMMEDIATE` (every write goes through this).
pub fn write_tx(conn: &mut Connection) -> Result<Transaction<'_>> {
    Ok(conn.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

fn user_version(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v as usize)
}

fn migrate(conn: &mut Connection) -> Result<()> {
    if user_version(conn)? >= MIGRATIONS.len() {
        return Ok(());
    }
    let tx = write_tx(conn)?;
    // Re-read inside the lock: another process may have migrated meanwhile.
    let current = user_version(&tx)?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    tx.commit()?;
    Ok(())
}
