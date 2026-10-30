//! AI integration foundation: error classification, the client trait and helpers.

pub mod binary;
pub mod classify;
pub mod claude;
pub mod json;
pub mod process;

use serde_json::{Map, Value};

/// Failure of an AI call. `RateLimited` and `Unavailable` pause the whole batch;
/// `Failed` counts as an attempt for the item (Inv §4.1–4.2).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    #[error("AI rate limited: {0}")]
    RateLimited(String),
    #[error("AI unavailable: {0}")]
    Unavailable(String),
    #[error("AI call failed: {0}")]
    Failed(String),
}

/// Which configured model a call uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Summary,
    Briefing,
}

/// A CLI-backed AI that answers a prompt with one JSON object.
pub trait AiClient: Send + Sync {
    fn complete_json(&self, prompt: &str, role: ModelRole) -> Result<Map<String, Value>, AiError>;
}
