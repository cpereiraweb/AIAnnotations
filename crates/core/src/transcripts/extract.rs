use crate::model::SessionOrigin;

pub const MESSAGE_LIMIT: usize = 2000;
/// Files listed in the prompt; the rest becomes a "+N arquivos" line.
pub const FILES_LIMIT: usize = 50;

/// One turn: the prompt is empty when the response had no prompt before it
/// (Codex subagent); the response is `None` when the turn had no text answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub prompt: String,
    pub response: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTouch {
    pub path: String,
    pub at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptExtract {
    pub cwd: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
    pub origin: SessionOrigin,
    pub parent_session_id: Option<String>,
    pub turns: Vec<Turn>,
    pub files_touched: Vec<String>,
    pub tool_use_count: u32,
    /// Time (ms) of each transcript event, in order.
    pub activity_times: Vec<i64>,
    pub file_touches: Vec<FileTouch>,
    pub unknown_origin: Option<String>,
}

impl Default for TranscriptExtract {
    fn default() -> Self {
        Self {
            cwd: None,
            branch: None,
            model: None,
            title: None,
            first_at: None,
            last_at: None,
            origin: SessionOrigin::Interactive,
            parent_session_id: None,
            turns: Vec::new(),
            files_touched: Vec::new(),
            tool_use_count: 0,
            activity_times: Vec::new(),
            file_touches: Vec::new(),
            unknown_origin: None,
        }
    }
}

fn clip(text: &str, limit: usize) -> String {
    // PHP trim() set only.
    let text = text.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0B']);
    if text.chars().count() > limit {
        let mut s: String = text.chars().take(limit.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl TranscriptExtract {
    /// Identifies the transcript content (Inv §3.4): last event in whole seconds + turn counts.
    pub fn fingerprint(&self) -> String {
        let last = self
            .last_at
            .map_or("-".to_string(), |ms| ms.div_euclid(1000).to_string());
        // v1 counts prompts and answers lists, kept aligned per turn: both equal the turn count.
        let n = self.turns.len();
        format!("{last}:{n}:{n}")
    }

    pub fn has_user_prompt(&self) -> bool {
        self.turns.iter().any(|t| !t.prompt.is_empty())
    }

    /// First event strictly later than `ref_ms` compared in whole seconds (ended_at has no ms).
    pub fn first_at_after(&self, ref_ms: i64) -> Option<i64> {
        let limit = ref_ms.div_euclid(1000);
        self.activity_times
            .iter()
            .copied()
            .filter(|ms| ms.div_euclid(1000) > limit)
            .min()
    }

    /// Files touched between `from_ms` and `to_ms` (inclusive, whole seconds; `None` = unbounded),
    /// in order of first touch. An extract without `file_touches` returns `files_touched` whole.
    pub fn files_touched_between(&self, from_ms: Option<i64>, to_ms: Option<i64>) -> Vec<String> {
        if self.file_touches.is_empty() {
            return self.files_touched.clone();
        }
        let from = from_ms.map(|f| f.div_euclid(1000));
        let to = to_ms.map(|t| t.div_euclid(1000));
        let mut files: Vec<String> = Vec::new();
        for touch in &self.file_touches {
            // A touch without time always counts.
            if let Some(ms) = touch.at_ms {
                let second = ms.div_euclid(1000);
                if from.is_some_and(|f| second < f) || to.is_some_and(|t| second > t) {
                    continue;
                }
            }
            if !files.contains(&touch.path) {
                files.push(touch.path.clone());
            }
        }
        files
    }

    /// Text for the AI summary (Inv §4.2): the first 2 prompts plus as many final pairs as fit in
    /// `max_chars`, then the touched files (max 50). The last pair always enters, clipped if needed.
    /// Fixed texts stay in Portuguese as in v1 until Plan 2.
    pub fn to_prompt_text(&self, max_chars: usize, files: Option<&[String]>) -> String {
        let files = render_files(files.unwrap_or(&self.files_touched));
        let budget = max_chars.saturating_sub(char_len(&files));
        let count = self.turns.len();

        let mut selected: Vec<usize> = Vec::new();
        for i in (0..count).rev() {
            let mut candidate = selected.clone();
            candidate.push(i);
            if char_len(&self.render_turns(&candidate, MESSAGE_LIMIT)) > budget {
                break;
            }
            selected = candidate;
        }

        let text = if selected.is_empty() && count > 0 {
            // The last pair alone does not fit: it enters clipped into the remaining space.
            let spare = budget as i64 - char_len(&self.render_turns(&[], MESSAGE_LIMIT)) as i64;
            let limit = (spare.div_euclid(2) - 20).max(50) as usize;
            self.render_turns(&[count - 1], limit.min(MESSAGE_LIMIT))
        } else {
            self.render_turns(&selected, MESSAGE_LIMIT)
        };

        take_chars(&format!("{}{files}", take_chars(&text, budget)), max_chars)
    }

    /// `pairs` are the turns shown with prompt and response (in descending order of selection).
    fn render_turns(&self, pairs: &[usize], last_pair_limit: usize) -> String {
        let count = self.turns.len();
        let mut indexes: Vec<usize> = (0..count.min(2)).collect();
        for &p in pairs {
            if !indexes.contains(&p) {
                indexes.push(p);
            }
        }
        indexes.sort_unstable();

        let last_pair = pairs.iter().max().copied();
        let mut lines = vec!["## Conversa".to_string()];
        let mut previous: i64 = -1;

        for &i in &indexes {
            let gap = i as i64 - previous - 1;
            if gap > 0 {
                lines.push(format!("[… {gap} troca(s) omitida(s) …]"));
            }
            let limit = if Some(i) == last_pair {
                last_pair_limit
            } else {
                MESSAGE_LIMIT
            };
            let turn = &self.turns[i];
            if !turn.prompt.is_empty() {
                lines.push(format!("[Usuário] {}", clip(&turn.prompt, limit)));
            }
            if pairs.contains(&i)
                && let Some(answer) = turn.response.as_deref().filter(|a| !a.is_empty())
            {
                lines.push(format!("[Assistente] {}", clip(answer, limit)));
            }
            previous = i as i64;
        }

        if previous >= 0 && count as i64 - 1 > previous {
            lines.push(format!(
                "[… {} troca(s) omitida(s) …]",
                count as i64 - 1 - previous
            ));
        }
        format!("{}\n", lines.join("\n"))
    }
}

fn render_files(files: &[String]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = files
        .iter()
        .take(FILES_LIMIT)
        .map(|f| format!("- {f}"))
        .collect();
    if files.len() > FILES_LIMIT {
        let extra = files.len() - FILES_LIMIT;
        lines.push(if extra == 1 {
            "- +1 arquivo".to_string()
        } else {
            format!("- +{extra} arquivos")
        });
    }
    format!("\n## Arquivos tocados\n{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::parse_ts;

    fn make(prompts: &[String], answers: &[String], files: &[&str]) -> TranscriptExtract {
        TranscriptExtract {
            cwd: Some("/tmp/x".into()),
            branch: Some("main".into()),
            model: Some("m".into()),
            title: Some("t".into()),
            turns: prompts
                .iter()
                .enumerate()
                .map(|(i, p)| Turn {
                    prompt: p.clone(),
                    response: answers.get(i).filter(|a| !a.is_empty()).cloned(),
                })
                .collect(),
            files_touched: files.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn pos(text: &str, needle: &str) -> usize {
        text.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    }

    /// toPromptText respeita maxChars e mantém o último par
    #[test]
    fn prompt_text_respects_max_chars_and_keeps_last_pair() {
        let prompts: Vec<String> = (1..=30)
            .map(|i| format!("PEDIDO-{i} {}", "p".repeat(300)))
            .collect();
        let answers: Vec<String> = (1..=30)
            .map(|i| format!("RESPOSTA-{i} {}", "r".repeat(300)))
            .collect();
        let text =
            make(&prompts, &answers, &["/tmp/x/a.php", "/tmp/x/b.php"]).to_prompt_text(4000, None);

        assert!(text.chars().count() <= 4000);
        for needle in [
            "PEDIDO-1 ",
            "PEDIDO-2 ",
            "PEDIDO-30 ",
            "RESPOSTA-30 ",
            "PEDIDO-29 ",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
        assert!(!text.contains("PEDIDO-15 "));
        assert!(!text.contains("RESPOSTA-1 "));
        assert!(text.contains("/tmp/x/a.php") && text.contains("/tmp/x/b.php"));
        assert!(pos(&text, "PEDIDO-2 ") < pos(&text, "PEDIDO-29 "));
        assert!(pos(&text, "PEDIDO-30 ") < pos(&text, "RESPOSTA-30 "));
    }

    /// toPromptText limita cada mensagem a 2000 caracteres
    #[test]
    fn prompt_text_limits_each_message_to_2000_chars() {
        let text = make(
            &[format!("PEDIDO {}", "a".repeat(5000))],
            &[format!("RESPOSTA {}", "b".repeat(5000))],
            &[],
        )
        .to_prompt_text(100_000, None);

        assert!(text.matches('a').count() <= 2000);
        assert!(text.matches('b').count() <= 2000);
        assert!(text.contains("PEDIDO") && text.contains("RESPOSTA"));
    }

    /// toPromptText mantém o último par mesmo quando ele sozinho estoura o limite
    #[test]
    fn prompt_text_keeps_last_pair_even_when_it_alone_exceeds_limit() {
        let text = make(
            &[
                s("primeiro"),
                s("segundo"),
                format!("ÚLTIMO-PEDIDO {}", "x".repeat(1900)),
            ],
            &[
                s("r1"),
                s("r2"),
                format!("ÚLTIMA-RESPOSTA {}", "y".repeat(1900)),
            ],
            &[],
        )
        .to_prompt_text(1500, None);

        assert!(text.chars().count() <= 1500);
        assert!(text.contains("ÚLTIMO-PEDIDO"));
    }

    /// toPromptText não repete pedidos quando a sessão é curta
    #[test]
    fn prompt_text_does_not_repeat_prompts_in_short_session() {
        let text = make(
            &[s("PEDIDO-A"), s("PEDIDO-B")],
            &[s("RESPOSTA-A"), s("RESPOSTA-B")],
            &[],
        )
        .to_prompt_text(10_000, None);

        assert_eq!(text.matches("PEDIDO-A").count(), 1);
        assert_eq!(text.matches("PEDIDO-B").count(), 1);
        assert!(text.contains("RESPOSTA-A") && text.contains("RESPOSTA-B"));
    }

    /// toPromptText lista no máximo 50 arquivos e resume o resto
    #[test]
    fn prompt_text_lists_at_most_50_files_and_summarizes_rest() {
        let files: Vec<String> = (1..=60)
            .map(|i| format!("/tmp/x/arquivo-{i}.php"))
            .collect();
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let text =
            make(&[s("PEDIDO-FINAL")], &[s("RESPOSTA-FINAL")], &refs).to_prompt_text(100_000, None);

        assert!(text.contains("/tmp/x/arquivo-50.php"));
        assert!(!text.contains("/tmp/x/arquivo-51.php"));
        assert!(text.contains("+10 arquivos"));
        assert!(text.contains("PEDIDO-FINAL") && text.contains("RESPOSTA-FINAL"));
    }

    /// toPromptText com até 50 arquivos não mostra a linha de excedente
    #[test]
    fn prompt_text_with_up_to_50_files_has_no_overflow_line() {
        let files: Vec<String> = (1..=50)
            .map(|i| format!("/tmp/x/arquivo-{i}.php"))
            .collect();
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let text = make(&[s("P")], &[s("R")], &refs).to_prompt_text(100_000, None);

        assert!(text.contains("/tmp/x/arquivo-50.php"));
        assert!(!text.contains(" arquivos"));
    }

    /// firstAtAfter devolve o primeiro evento em segundo inteiro posterior à referência
    #[test]
    fn first_at_after_returns_first_event_after_reference_in_whole_seconds() {
        let x = TranscriptExtract {
            activity_times: vec![
                parse_ts("2026-10-20T10:00:00.000Z").unwrap(),
                parse_ts("2026-10-20T10:00:00.900Z").unwrap(),
                parse_ts("2026-10-20T10:07:00.250Z").unwrap(),
            ],
            ..Default::default()
        };
        let t = |v: &str| parse_ts(v).unwrap();

        assert_eq!(
            x.first_at_after(t("2026-10-20T10:00:00Z")),
            Some(t("2026-10-20T10:07:00.250Z"))
        );
        assert_eq!(
            x.first_at_after(t("2026-10-20T09:59:59Z")),
            Some(t("2026-10-20T10:00:00.000Z"))
        );
        assert_eq!(x.first_at_after(t("2026-10-20T10:07:00Z")), None);
        assert_eq!(
            TranscriptExtract::default().first_at_after(t("2026-10-20T10:00:00Z")),
            None
        );
    }

    /// filesTouchedBetween filtra os arquivos pelo intervalo em segundos inteiros
    #[test]
    fn files_touched_between_filters_by_whole_second_range() {
        let t = |v: &str| parse_ts(v).unwrap();
        let touch = |p: &str, at: Option<&str>| FileTouch {
            path: s(p),
            at_ms: at.map(t),
        };
        let x = TranscriptExtract {
            files_touched: vec![s("/a"), s("/b"), s("/c")],
            tool_use_count: 3,
            file_touches: vec![
                touch("/a", Some("2026-10-20T10:00:00.700Z")),
                touch("/b", Some("2026-10-20T11:00:00Z")),
                touch("/a", Some("2026-10-20T11:30:00Z")),
                touch("/c", None),
            ],
            ..Default::default()
        };
        let at = |v: &str| Some(t(v));

        assert_eq!(
            x.files_touched_between(at("2026-10-20T10:00:00Z"), at("2026-10-20T10:30:00Z")),
            vec!["/a", "/c"]
        );
        assert_eq!(
            x.files_touched_between(at("2026-10-20T10:59:00Z"), None),
            vec!["/b", "/a", "/c"]
        );
        assert_eq!(
            x.files_touched_between(at("2026-10-20T12:00:00Z"), None),
            vec!["/c"]
        );
        assert_eq!(x.files_touched_between(None, None), vec!["/a", "/b", "/c"]);
        // Extract without touches: the whole list applies.
        assert_eq!(
            make(&[], &[], &["/x", "/y"]).files_touched_between(at("2026-10-20T12:00:00Z"), None),
            vec!["/x", "/y"]
        );
    }

    /// Fingerprint conta turnos nas duas posições (v1 mantém prompts e respostas alinhados).
    #[test]
    fn fingerprint_counts_turns_even_without_response() {
        let x = make(&[s("a"), s("b")], &[s("r"), s("")], &[]);
        assert_eq!(x.turns[1].response, None);
        assert_eq!(x.fingerprint(), "-:2:2");
    }

    #[test]
    fn fingerprint_and_has_user_prompt() {
        let mut x = make(&[s("a"), s("b")], &[s("r")], &[]);
        assert_eq!(x.fingerprint(), "-:2:2");
        x.last_at = parse_ts("2026-10-20T10:00:00.900Z");
        assert!(x.fingerprint().ends_with(":2:2") && !x.fingerprint().starts_with('-'));
        assert!(x.has_user_prompt());
        assert!(!make(&[s("")], &[s("r")], &[]).has_user_prompt());
        assert_eq!(
            TranscriptExtract::default().origin,
            SessionOrigin::Interactive
        );
    }
}
