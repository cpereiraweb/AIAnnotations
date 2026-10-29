//! Once-per-day warning gate for unknown session origins (Inv §3.7). The reader only reports the
//! value in `TranscriptExtract::unknown_origin`; callers log when this returns `true`.

use chrono::{Local, TimeZone};
use rusqlite::Transaction;

use crate::Result;
use crate::db::app_state;
use crate::model::Tool;

const TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// `true` the first time `(tool, value)` is seen on the local day of `now_ms`.
pub fn warn_unknown_once(tx: &Transaction, tool: Tool, value: &str, now_ms: i64) -> Result<bool> {
    let day = Local.timestamp_millis_opt(now_ms).single().map_or_else(
        || "unknown".to_string(),
        |d| d.format("%Y-%m-%d").to_string(),
    );
    let key = format!("unknown-origin:{}:{value}:{day}", tool.as_str());
    app_state::try_lock(tx, &key, TTL_MS, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// Fixed local noon, so `t` and `t + 1s` share a local day in any time zone.
    fn local_noon() -> i64 {
        Local
            .with_ymd_and_hms(2026, 10, 29, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    /// Avisa só uma vez por valor no dia; volta a avisar no dia seguinte.
    #[test]
    fn warns_once_per_value_per_local_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let tx = db::write_tx(&mut conn).unwrap();
        let t = local_noon();
        assert!(warn_unknown_once(&tx, Tool::Claude, "novo-app", t).unwrap());
        assert!(!warn_unknown_once(&tx, Tool::Claude, "novo-app", t + 1000).unwrap());
        assert!(warn_unknown_once(&tx, Tool::Claude, "outro", t).unwrap());
        assert!(warn_unknown_once(&tx, Tool::Codex, "novo-app", t).unwrap());
        assert!(warn_unknown_once(&tx, Tool::Claude, "novo-app", t + 36 * 3_600_000).unwrap());
    }
}
