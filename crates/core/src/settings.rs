use serde::{Deserialize, Serialize};

use crate::model::Tool;

/// Language of AI output (summaries, briefing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
}

impl Lang {
    /// `"pt"`, `"pt-BR"`, `"pt_BR.UTF-8"` give `PtBr`; anything else `En`.
    pub fn from_tag(tag: &str) -> Lang {
        if tag.trim().to_ascii_lowercase().starts_with("pt") {
            Lang::PtBr
        } else {
            Lang::En
        }
    }

    /// `LANG` when set (Unix, and an explicit override elsewhere), else the OS locale: Windows
    /// and macOS GUI apps get no `LANG`, so the tray and the AI texts followed `en` there.
    pub fn detect(lang_env: Option<&str>, os_locale: Option<&str>) -> Lang {
        lang_env
            .filter(|t| !t.trim().is_empty())
            .or(os_locale)
            .map_or(Lang::En, Lang::from_tag)
    }
}

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
    pub lang: Lang,
    pub ai_provider: Tool,
    pub claude_model: String,
    pub claude_briefing_model: String,
    /// `None` = the Codex default model.
    pub codex_model: Option<String>,
    pub codex_briefing_model: Option<String>,
    pub ai_timeout_seconds: u32,
    pub summary_max_attempts: u32,
    pub backfill_interval_seconds: u32,
    pub extract_max_chars: u32,
    pub ai_pause_minutes: u32,
    pub interrupted_window_days: u32,
    pub active_projects_days: u32,
    pub git_cache_seconds: u32,
    pub briefing_runs_per_project: u32,
    pub briefing_lock_seconds: u32,
    /// `false` = runs imported from history (backfill) are never summarized.
    pub summarize_history: bool,
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
            lang: Lang::En,
            ai_provider: Tool::Claude,
            claude_model: "haiku".into(),
            claude_briefing_model: "sonnet".into(),
            codex_model: None,
            codex_briefing_model: None,
            ai_timeout_seconds: 180,
            summary_max_attempts: 5,
            backfill_interval_seconds: 120,
            extract_max_chars: 24000,
            ai_pause_minutes: 30,
            interrupted_window_days: 14,
            active_projects_days: 60,
            git_cache_seconds: 300,
            briefing_runs_per_project: 5,
            briefing_lock_seconds: 600,
            summarize_history: true,
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
        assert_eq!(s.lang, Lang::En);
        assert_eq!(s.ai_provider, Tool::Claude);
        assert_eq!(s.claude_model, "haiku");
        assert_eq!(s.claude_briefing_model, "sonnet");
        assert_eq!(s.codex_model, None);
        assert_eq!(s.codex_briefing_model, None);
        assert_eq!(s.ai_timeout_seconds, 180);
        assert_eq!(s.summary_max_attempts, 5);
        assert_eq!(s.backfill_interval_seconds, 120);
        assert_eq!(s.extract_max_chars, 24000);
        assert_eq!(s.ai_pause_minutes, 30);
        assert_eq!(s.interrupted_window_days, 14);
        assert_eq!(s.active_projects_days, 60);
        assert_eq!(s.git_cache_seconds, 300);
        assert_eq!(s.briefing_runs_per_project, 5);
        assert_eq!(s.briefing_lock_seconds, 600);
        assert!(s.summarize_history);
    }

    #[test]
    fn lang_from_tag() {
        assert_eq!(Lang::from_tag("pt"), Lang::PtBr);
        assert_eq!(Lang::from_tag("pt-BR"), Lang::PtBr);
        assert_eq!(Lang::from_tag("pt_BR.UTF-8"), Lang::PtBr);
        assert_eq!(Lang::from_tag("en"), Lang::En);
        assert_eq!(Lang::from_tag("en_US.UTF-8"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }

    #[test]
    fn detect_prefers_lang_then_the_os_locale() {
        assert_eq!(Lang::detect(None, Some("pt-BR")), Lang::PtBr);
        assert_eq!(Lang::detect(Some(""), Some("pt-BR")), Lang::PtBr);
        assert_eq!(Lang::detect(Some("en_US.UTF-8"), Some("pt-BR")), Lang::En);
        assert_eq!(Lang::detect(Some("pt_BR.UTF-8"), None), Lang::PtBr);
        assert_eq!(Lang::detect(None, None), Lang::En);
    }
}
