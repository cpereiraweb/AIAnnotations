use chrono::DateTime;

/// Parses an RFC 3339 timestamp (any offset) into milliseconds since the Unix epoch (UTC).
pub fn parse_ts(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review Focus 4: mesmo instante em qualquer offset.
    #[test]
    fn parse_ts_same_instant_any_offset() {
        let a = parse_ts("2026-10-29T10:00:00-03:00").unwrap();
        let b = parse_ts("2026-10-29T13:00:00Z").unwrap();
        assert_eq!(a, b);
        assert_eq!(parse_ts("2026-10-29T13:00:00.123Z").unwrap(), b + 123);
        assert_eq!(parse_ts("lixo"), None);
        assert_eq!(parse_ts(""), None);
    }
}
