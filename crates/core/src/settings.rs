/// Defaults from the v1 inventory (Inv §13.1/§13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub idle_minutes: u32,
    pub chain_window_seconds: u32,
    pub chunk_size: u32,
    pub stale_days: u32,
    pub inbox_recent_days: u32,
    pub spool_done_days: u32,
    pub spool_failed_days: u32,
    pub busy_max_age_seconds: u32,
    pub duplicate_end_seconds: u32,
    pub resume_tolerance_seconds: u32,
    pub collect_window_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_minutes: 30,
            chain_window_seconds: 120,
            chunk_size: 1000,
            stale_days: 3,
            inbox_recent_days: 30,
            spool_done_days: 7,
            spool_failed_days: 30,
            busy_max_age_seconds: 3600,
            duplicate_end_seconds: 60,
            resume_tolerance_seconds: 60,
            collect_window_days: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_v1() {
        let s = Settings::default();
        assert_eq!(s.idle_minutes, 30);
        assert_eq!(s.chain_window_seconds, 120);
        assert_eq!(s.chunk_size, 1000);
        assert_eq!(s.stale_days, 3);
        assert_eq!(s.inbox_recent_days, 30);
        assert_eq!(s.spool_done_days, 7);
        assert_eq!(s.spool_failed_days, 30);
        assert_eq!(s.busy_max_age_seconds, 3600);
        assert_eq!(s.duplicate_end_seconds, 60);
        assert_eq!(s.resume_tolerance_seconds, 60);
        assert_eq!(s.collect_window_days, 2);
    }
}
