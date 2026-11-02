//! Key/value store with optional expiry; replaces Laravel's `cache` table
//! (locks and the pause flag). Expiry compares against `expires_at` in ms UTC.
//! Writes take a `Transaction` so they always run inside `db::write_tx` (IMMEDIATE).

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::Result;

/// Value of `key`, or `None` if absent or expired at `now_ms`.
pub fn get(conn: &Connection, key: &str, now_ms: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM app_state WHERE key = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
            params![key, now_ms],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set(tx: &Transaction, key: &str, value: &str, expires_at_ms: Option<i64>) -> Result<()> {
    tx.execute(
        "INSERT INTO app_state (key, value, expires_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at",
        params![key, value, expires_at_ms],
    )?;
    Ok(())
}

pub fn delete(tx: &Transaction, key: &str) -> Result<()> {
    tx.execute("DELETE FROM app_state WHERE key = ?1", [key])?;
    Ok(())
}

/// Atomically takes the lock `key` for `ttl_ms`; `false` if a live holder exists.
/// A lock whose `expires_at <= now_ms` is considered free.
pub fn try_lock(tx: &Transaction, key: &str, ttl_ms: i64, now_ms: i64) -> Result<bool> {
    let changed = tx.execute(
        "INSERT INTO app_state (key, value, expires_at) VALUES (?1, '1', ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at \
         WHERE app_state.expires_at IS NOT NULL AND app_state.expires_at <= ?3",
        params![key, now_ms.saturating_add(ttl_ms), now_ms],
    )?;
    Ok(changed == 1)
}

pub fn unlock(tx: &Transaction, key: &str) -> Result<()> {
    delete(tx, key)
}
