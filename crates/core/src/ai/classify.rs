//! Error classification for AI CLI output (port of the Laravel patterns, Inv §4.1).
//! No regex crate: lowercase substring searches plus word-boundary checks.

use std::path::Path;

use super::AiError;

/// Longest error detail kept in messages.
const DETAIL_CHARS: usize = 500;

/// Regex `\w`: letters, digits, underscore.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Regex `\bword\b` on already-lowercased text.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

/// Subscription or API usage limits. "429" only counts next to "too many requests"
/// or "rate"; "limit reached" only next to usage/5-hour/weekly/hour(s)/reset.
pub fn is_rate_limited(detail: &str) -> bool {
    let d = detail.to_lowercase();
    if has_any(
        &d,
        &[
            "ratelimit",
            "rate limit",
            "rate_limit",
            "rate-limit",
            "usage limit",
            "hit your limit",
            "hit your usage limit",
        ],
    ) {
        return true;
    }
    if has_word(&d, "429") && (d.contains("too many requests") || has_word(&d, "rate")) {
        return true;
    }
    d.contains("limit reached")
        && (has_any(&d, &["usage", "5-hour", "weekly", "reset"])
            || has_word(&d, "hour")
            || has_word(&d, "hours"))
}

/// The bare command (`claude`, `codex`) as an isolated word (not `~/.claude/…` nor
/// `claude-code/…`): preceded by start/whitespace/`/`/`:`/quote and followed by
/// whitespace/`:`/quote/end.
fn cites_command(detail: &str, name: &str) -> bool {
    detail.match_indices(name).any(|(i, _)| {
        let before = detail[..i].chars().next_back();
        let after = detail[i + name.len()..].chars().next();
        before.is_none_or(|c| c.is_whitespace() || matches!(c, '/' | ':' | '\'' | '"'))
            && after.is_none_or(|c| c.is_whitespace() || matches!(c, ':' | '\'' | '"'))
    })
}

/// Lowercase markers of a missing or invalid login, per CLI.
const CLAUDE_LOGIN: &[&str] = &[
    "run /login",
    "not logged in",
    "authentication failed",
    "authentication_error",
    "failed to authenticate",
    "invalid api key",
    "oauth token",
];
const CODEX_LOGIN: &[&str] = &[
    "not logged in",
    "codex login",
    "401 unauthorized",
    "unauthorized",
    "please log in",
    "authentication",
];
/// Shell messages for a binary that is not installed.
const MISSING_BINARY: &[&str] = &["command not found", ": not found"];

/// The CLI is not logged in (or its credential is invalid), as opposed to missing.
pub fn is_login_error(detail: &str) -> bool {
    let d = detail.to_lowercase();
    has_any(&d, CLAUDE_LOGIN) || has_any(&d, CODEX_LOGIN)
}

/// Shared shape of the "unavailable" checks: exit 127, any of `needles` (lowercase), or
/// "no such file or directory" citing the binary (its absolute path or the bare `command`).
fn unavailable(detail: &str, bin: &Path, exit: Option<i32>, login: &[&str], command: &str) -> bool {
    if exit == Some(127) {
        return true;
    }
    let d = detail.to_lowercase();
    if has_any(&d, login) || has_any(&d, MISSING_BINARY) {
        return true;
    }
    if !d.contains("no such file or directory") {
        return false;
    }
    let bin_text = bin.to_string_lossy();
    let cites_bin = bin.is_absolute() && detail.contains(bin_text.as_ref());
    cites_bin || cites_command(detail, command)
}

/// No login/invalid credential, or the binary is missing (exit 127, "command not found",
/// "no such file" citing the binary).
pub fn claude_unavailable(detail: &str, bin: &Path, exit: Option<i32>) -> bool {
    unavailable(detail, bin, exit, CLAUDE_LOGIN, "claude")
}

/// No login/invalid credential, or the `codex` binary is missing (exit 127,
/// "command not found", "no such file" citing the binary).
pub fn codex_unavailable(detail: &str, bin: &Path, exit: Option<i32>) -> bool {
    unavailable(detail, bin, exit, CODEX_LOGIN, "codex")
}

/// `Failed` for a reply with no JSON object, quoting its end: the model's prose (a refusal,
/// a login notice) is the only clue to why it did not answer in JSON.
pub(super) fn no_json(what: &str, reply: &str) -> AiError {
    AiError::Failed(format!(
        "{what} result has no JSON object: {}",
        tail(reply.trim())
    ))
}

/// The last `DETAIL_CHARS` characters of `s` (the error line comes last in CLI output).
pub fn tail(s: &str) -> &str {
    let skip = s.chars().count().saturating_sub(DETAIL_CHARS);
    s.char_indices().nth(skip).map_or("", |(i, _)| &s[i..])
}

/// Classifies a failed CLI call. Every copy of `prompt` is removed from `detail` first
/// (`codex exec` echoes the prompt into stderr, and transcripts often mention rate limits
/// or authentication). Messages keep only the end of the detail. `what` names the
/// command in the `Failed` message.
pub(super) fn failure(
    detail: &str,
    prompt: &str,
    exit: Option<i32>,
    is_unavailable: impl Fn(&str, Option<i32>) -> bool,
    what: &str,
) -> AiError {
    let mut detail = detail.to_string();
    for p in [prompt, prompt.trim()] {
        if !p.is_empty() {
            detail = detail.replace(p, "");
        }
    }
    let detail = detail.trim();
    let code = exit.map_or_else(|| "?".to_string(), |c| c.to_string());
    if is_rate_limited(detail) {
        AiError::RateLimited(tail(detail).to_string())
    } else if is_unavailable(detail, exit) {
        AiError::Unavailable(format!("exit {code}: {}", tail(detail)))
    } else {
        AiError::Failed(format!("{what} failed (exit {code}): {}", tail(detail)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/usr/local/bin/claude";

    #[test]
    fn login_errors_are_recognised() {
        assert!(is_login_error(
            "Failed to authenticate: OAuth session expired"
        ));
        assert!(is_login_error("Not logged in · Please run /login"));
        assert!(!is_login_error("codex: command not found"));
        assert!(!is_login_error("rate limit"));
    }

    fn unavailable(detail: &str) -> bool {
        claude_unavailable(detail, Path::new(BIN), Some(1))
    }

    /// Seen on Windows 11 with a stale login: it must pause the queue, not burn attempts.
    #[test]
    fn expired_oauth_session_is_unavailable() {
        assert!(unavailable(
            "Failed to authenticate: OAuth session expired and could not be refreshed"
        ));
    }

    #[test]
    fn usage_limit_message_is_rate_limited() {
        assert!(is_rate_limited("Claude AI usage limit reached|1730000000"));
    }

    #[test]
    fn five_hour_limit_is_rate_limited() {
        assert!(is_rate_limited("5-hour limit reached · resets 3pm"));
    }

    #[test]
    fn http_429_with_context_is_rate_limited() {
        assert!(is_rate_limited("HTTP 429 Too Many Requests"));
        assert!(is_rate_limited("error 429: rate exceeded"));
    }

    #[test]
    fn bare_429_is_not_rate_limited() {
        assert!(!is_rate_limited("Processed 429 files"));
        assert!(!is_rate_limited("id 14290 and rate of 429x"));
    }

    #[test]
    fn limit_reached_without_context_is_not_rate_limited() {
        assert!(!is_rate_limited("limit reached for this file"));
    }

    #[test]
    fn rate_limit_spellings() {
        for s in [
            "rate limit",
            "Rate_Limit exceeded",
            "rate-limit",
            "ratelimit",
            "You hit your limit",
            "you hit your usage limit",
        ] {
            assert!(is_rate_limited(s), "{s}");
        }
        assert!(!is_rate_limited("generic failure"));
    }

    #[test]
    fn limit_reached_with_hours_word() {
        assert!(is_rate_limited("limit reached, try in 2 hours"));
        assert!(!is_rate_limited("limit reached at shoursx"));
    }

    #[test]
    fn missing_binary_citing_it_is_unavailable() {
        assert!(unavailable(
            "/usr/local/bin/claude: No such file or directory"
        ));
    }

    #[test]
    fn missing_file_under_dot_claude_is_not_unavailable() {
        assert!(!unavailable(
            "ENOENT: no such file or directory, open '/home/u/.claude/projects/x.jsonl'"
        ));
        assert!(!unavailable(
            "no such file or directory: /opt/claude-code/lib.js"
        ));
    }

    #[test]
    fn bare_claude_command_missing_is_unavailable() {
        assert!(unavailable("sh: claude: No such file or directory"));
        assert!(unavailable("exec: 'claude': no such file or directory"));
    }

    #[test]
    fn login_problems_are_unavailable() {
        assert!(unavailable("Invalid API key · Please run /login"));
        assert!(unavailable("Not logged in"));
        assert!(unavailable("authentication_error: expired OAuth token"));
        assert!(!unavailable("some other failure"));
    }

    #[test]
    fn command_not_found_is_unavailable() {
        assert!(unavailable("sh: 1: claude: not found"));
        assert!(unavailable("bash: claude: command not found"));
    }

    #[test]
    fn exit_127_is_unavailable() {
        assert!(claude_unavailable("", Path::new(BIN), Some(127)));
    }

    fn codex_unavail(detail: &str) -> bool {
        codex_unavailable(detail, Path::new("/usr/local/bin/codex"), Some(1))
    }

    #[test]
    fn codex_login_problems_are_unavailable() {
        for s in [
            "Not logged in. Run codex login",
            "401 Unauthorized",
            "Please log in again",
            "Authentication required",
        ] {
            assert!(codex_unavail(s), "{s}");
        }
        assert!(!codex_unavail("some other failure"));
    }

    #[test]
    fn codex_missing_binary_is_unavailable() {
        assert!(codex_unavailable("", Path::new("/x/codex"), Some(127)));
        assert!(codex_unavail("bash: codex: command not found"));
        assert!(codex_unavail("sh: 1: codex: not found"));
        assert!(codex_unavail(
            "/usr/local/bin/codex: No such file or directory"
        ));
        assert!(codex_unavail("sh: codex: No such file or directory"));
    }

    #[test]
    fn tail_keeps_the_last_chars_on_a_char_boundary() {
        let s = format!("{}{}", "é".repeat(600), "fim");
        let t = tail(&s);
        assert_eq!(t.chars().count(), DETAIL_CHARS);
        assert!(t.ends_with("éfim"));
        assert_eq!(tail("curto"), "curto");
        assert_eq!(tail(""), "");
    }

    #[test]
    fn failure_ignores_the_prompt_in_the_detail() {
        let prompt = "dev: rate limit exceeded; authentication failed";
        let err = failure(
            &format!("user\n{prompt}\nERROR: boom"),
            prompt,
            Some(1),
            |d, e| codex_unavailable(d, Path::new("/x/codex"), e),
            "codex exec",
        );
        assert_eq!(
            err,
            AiError::Failed("codex exec failed (exit 1): user\n\nERROR: boom".into())
        );
    }

    #[test]
    fn codex_unrelated_missing_file_is_not_unavailable() {
        assert!(!codex_unavail(
            "no such file or directory, open '/home/u/.codex/sessions/x.jsonl'"
        ));
        assert!(!codex_unavail(
            "no such file or directory: /opt/codex-cli/x"
        ));
    }
}
