//! Data actions behind the Today screen buttons. None of them calls the AI.

use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, InboxItem};
use aiannotations_core::sessions::collector::{self, CollectMode, CollectReport};
use aiannotations_core::{Paths, Result, Settings};
use rusqlite::Connection;

/// Dismisses one inbox item at its current state; it returns when the state changes.
pub fn dismiss(conn: &mut Connection, key: &str, state_hash: &str, now_ms: i64) -> Result<()> {
    inbox::dismiss(conn, key, state_hash, now_ms)
}

/// Dismisses every item of a group in one transaction.
pub fn dismiss_group(conn: &mut Connection, items: &[InboxItem], now_ms: i64) -> Result<()> {
    inbox::dismiss_all(conn, items, now_ms)
}

/// "Import history": the whole on-disk history, runs marked `is_backfill`. Summaries are left
/// pending for the worker. `progress(done, total)` is passed to the collector.
pub fn import_history(
    conn: &mut Connection,
    paths: &Paths,
    settings: &Settings,
    git: &Git,
    now_ms: i64,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<CollectReport> {
    collector::run(
        conn,
        paths,
        settings,
        git,
        CollectMode::Backfill,
        now_ms,
        progress,
    )
}
