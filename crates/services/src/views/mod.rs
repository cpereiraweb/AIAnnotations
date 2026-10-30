//! Queries behind each screen. Each takes a `Connection` and returns plain serializable data;
//! instants are milliseconds UTC and formatting is the frontend's job.

pub mod projects;
pub mod runs;
pub mod today;

use aiannotations_core::Result;
use aiannotations_core::search::{self, SearchHit};
use rusqlite::Connection;

/// Results shown by the search box (Inv §1.6).
pub const SEARCH_LIMIT: usize = 12;

/// Free-text search over summaries, titles and projects; at most 12 hits.
pub fn search(conn: &Connection, text: &str) -> Result<Vec<SearchHit>> {
    search::query(conn, text, SEARCH_LIMIT)
}
