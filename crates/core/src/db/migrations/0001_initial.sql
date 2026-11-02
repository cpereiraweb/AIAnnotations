-- Same table/column names as the Laravel v1 (Inv §2.1). Instants are INTEGER ms UTC,
-- JSON is TEXT, booleans are INTEGER. created_at/updated_at kept for 1:1 import.
CREATE TABLE projects (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    key              TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL,
    path             TEXT NOT NULL,
    remote_url       TEXT,
    last_activity_at INTEGER,
    created_at       INTEGER,
    updated_at       INTEGER
);

CREATE TABLE ai_sessions (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id          INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    tool                TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    cwd                 TEXT NOT NULL,
    worktree_path       TEXT,
    transcript_path     TEXT,
    transcript_mtime    INTEGER,
    model               TEXT,
    title               TEXT,
    origin              TEXT NOT NULL DEFAULT 'interactive',
    previous_session_id INTEGER REFERENCES ai_sessions (id) ON DELETE SET NULL,
    start_source        TEXT,
    parent_session_id   TEXT,
    is_empty            INTEGER NOT NULL DEFAULT 0,
    last_activity_at    INTEGER NOT NULL,
    created_at          INTEGER,
    updated_at          INTEGER,
    UNIQUE (tool, session_id)
);
CREATE INDEX ai_sessions_project_id_idx ON ai_sessions (project_id);
CREATE INDEX ai_sessions_previous_session_id_idx ON ai_sessions (previous_session_id);

CREATE TABLE session_runs (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    ai_session_id          INTEGER NOT NULL REFERENCES ai_sessions (id) ON DELETE CASCADE,
    started_at             INTEGER NOT NULL,
    ended_at               INTEGER,
    end_reason             TEXT,
    ended_cleanly          INTEGER,
    branch                 TEXT,
    head_start             TEXT,
    head_end               TEXT,
    status_hash_start      TEXT,
    status_hash_end        TEXT,
    changed_files          INTEGER,
    files_touched          TEXT,
    summary_status         TEXT NOT NULL DEFAULT 'pending',
    summary                TEXT,
    next_step              TEXT,
    unfinished             INTEGER,
    kind                   TEXT,
    summary_attempts       INTEGER NOT NULL DEFAULT 0,
    summarized_fingerprint TEXT,
    summary_available_at   INTEGER,
    is_backfill            INTEGER NOT NULL DEFAULT 0,
    discovered             INTEGER NOT NULL DEFAULT 0,
    created_at             INTEGER,
    updated_at             INTEGER
);
CREATE INDEX session_runs_ai_session_id_idx ON session_runs (ai_session_id);

CREATE TABLE inbox_dismissals (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    item_key   TEXT NOT NULL UNIQUE,
    state_hash TEXT NOT NULL,
    created_at INTEGER,
    updated_at INTEGER
);

CREATE TABLE briefings (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    date         TEXT NOT NULL UNIQUE, -- YYYY-MM-DD
    status       TEXT NOT NULL,
    body         TEXT,
    next_steps   TEXT,
    covers_from  INTEGER NOT NULL,
    covers_until INTEGER NOT NULL,
    generated_at INTEGER,
    created_at   INTEGER,
    updated_at   INTEGER
);

CREATE VIRTUAL TABLE search_index USING fts5(
    run_id UNINDEXED,
    project,
    branch,
    title,
    summary,
    next_step,
    compact,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Replaces Laravel's `cache` table (locks, pause flag).
CREATE TABLE app_state (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    expires_at INTEGER
);
