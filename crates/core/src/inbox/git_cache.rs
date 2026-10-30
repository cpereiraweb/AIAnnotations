//! TTL cache of the git queries the inbox makes (port of the v1 `Cache::remember` calls).
//! Driven by the `now_ms` the caller passes; it never reads the clock.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::git::{StaleBranch, Worktree};

#[derive(Debug, Clone)]
pub enum CachedGit {
    Stale(Vec<StaleBranch>),
    /// sha1 of `git status --porcelain`; `None` when git failed.
    Status(Option<String>),
    Worktrees(Vec<Worktree>),
}

/// One value per `"<kind>:<project_id>"`, reused while younger than the TTL.
#[derive(Default)]
pub struct GitCache {
    entries: Mutex<HashMap<String, (i64, CachedGit)>>,
}

impl GitCache {
    pub fn new() -> GitCache {
        GitCache::default()
    }

    /// The cached value of `key`, or the result of `compute` (stored at `now_ms`). The lock is
    /// not held while git runs.
    pub(crate) fn get_or(
        &self,
        key: &str,
        now_ms: i64,
        ttl_ms: i64,
        compute: impl FnOnce() -> CachedGit,
    ) -> CachedGit {
        if let Ok(map) = self.entries.lock()
            && let Some((at, value)) = map.get(key)
            && now_ms - at < ttl_ms
        {
            return value.clone();
        }
        let value = compute();
        if let Ok(mut map) = self.entries.lock() {
            map.insert(key.to_string(), (now_ms, value.clone()));
        }
        value
    }
}
