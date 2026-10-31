//! Enums (string values identical to the Laravel v1, Inv §2.2) and model rules (Inv §2.3).

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};

use crate::{Error, Result};

/// "Visible" session (`AiSession::scopeVisible`): interactive and not empty.
pub const VISIBLE_SESSION_SQL: &str = "origin = 'interactive' AND is_empty = 0";

macro_rules! string_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        impl FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> {
                match s {
                    $($s => Ok(Self::$variant),)+
                    other => Err(Error::InvalidEvent(format!(
                        "unknown {} value: {other:?}", stringify!($name)
                    ))),
                }
            }
        }
    };
}

string_enum!(Tool { Claude => "claude", Codex => "codex" });

impl Tool {
    /// Display name of the CLI.
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
        }
    }
}
string_enum!(SessionOrigin {
    Interactive => "interactive",
    Automation => "automation",
    Subagent => "subagent",
    Internal => "internal",
});
string_enum!(SummaryStatus {
    Pending => "pending",
    Done => "done",
    Failed => "failed",
    Skipped => "skipped",
});
string_enum!(BriefingStatus {
    Generating => "generating",
    Done => "done",
    Failed => "failed",
});
string_enum!(EndReason {
    PromptInputExit => "prompt_input_exit",
    Clear => "clear",
    Logout => "logout",
    Other => "other",
    Idle => "idle",
});

/// Id of the run with the greatest `started_at` (ties broken by the greater id).
pub fn latest_run_id(conn: &Connection, ai_session_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM session_runs WHERE ai_session_id = ?1 \
             ORDER BY started_at DESC, id DESC LIMIT 1",
            [ai_session_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_label() {
        assert_eq!(Tool::Claude.label(), "Claude Code");
        assert_eq!(Tool::Codex.label(), "Codex");
    }
}
