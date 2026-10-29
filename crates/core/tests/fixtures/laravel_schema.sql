-- Laravel v1.0.0 schema (sqlite3 .schema of the real database, schema only, no data).
-- Tables the import reads plus some it must ignore (users, cache, jobs, sessions).
CREATE TABLE IF NOT EXISTS "users" ("id" integer primary key autoincrement not null, "name" varchar not null, "email" varchar not null, "email_verified_at" datetime, "password" varchar not null, "remember_token" varchar, "created_at" datetime, "updated_at" datetime);
CREATE TABLE IF NOT EXISTS "sessions" ("id" varchar not null, "user_id" integer, "ip_address" varchar, "user_agent" text, "payload" text not null, "last_activity" integer not null, primary key ("id"));
CREATE TABLE IF NOT EXISTS "cache" ("key" varchar not null, "value" text not null, "expiration" integer not null, primary key ("key"));
CREATE TABLE IF NOT EXISTS "jobs" ("id" integer primary key autoincrement not null, "queue" varchar not null, "payload" text not null, "attempts" integer not null, "reserved_at" integer, "available_at" integer not null, "created_at" integer not null);
CREATE TABLE IF NOT EXISTS "projects" ("id" integer primary key autoincrement not null, "key" varchar not null, "name" varchar not null, "path" varchar not null, "remote_url" varchar, "last_activity_at" datetime, "created_at" datetime, "updated_at" datetime);
CREATE UNIQUE INDEX "projects_key_unique" on "projects" ("key");
CREATE TABLE IF NOT EXISTS "ai_sessions" ("id" integer primary key autoincrement not null, "project_id" integer not null, "tool" varchar not null, "session_id" varchar not null, "cwd" varchar not null, "worktree_path" varchar, "transcript_path" varchar, "model" varchar, "title" varchar, "origin" varchar not null default 'interactive', "previous_session_id" integer, "parent_session_id" varchar, "is_empty" tinyint(1) not null default '0', "last_activity_at" datetime not null, "created_at" datetime, "updated_at" datetime, "start_source" varchar, "transcript_mtime" integer, foreign key("project_id") references "projects"("id") on delete cascade, foreign key("previous_session_id") references "ai_sessions"("id") on delete set null);
CREATE UNIQUE INDEX "ai_sessions_tool_session_id_unique" on "ai_sessions" ("tool", "session_id");
CREATE TABLE IF NOT EXISTS "session_runs" ("id" integer primary key autoincrement not null, "ai_session_id" integer not null, "started_at" datetime not null, "ended_at" datetime, "end_reason" varchar, "ended_cleanly" tinyint(1), "branch" varchar, "head_start" varchar, "head_end" varchar, "status_hash_start" varchar, "status_hash_end" varchar, "changed_files" tinyint(1), "files_touched" text, "summary_status" varchar not null default 'pending', "summary" text, "next_step" text, "unfinished" tinyint(1), "kind" varchar, "summary_attempts" integer not null default '0', "summary_available_at" datetime, "is_backfill" tinyint(1) not null default '0', "created_at" datetime, "updated_at" datetime, "summarized_fingerprint" varchar, "discovered" tinyint(1) not null default '0', foreign key("ai_session_id") references "ai_sessions"("id") on delete cascade);
CREATE TABLE IF NOT EXISTS "inbox_dismissals" ("id" integer primary key autoincrement not null, "item_key" varchar not null, "state_hash" varchar not null, "created_at" datetime, "updated_at" datetime);
CREATE UNIQUE INDEX "inbox_dismissals_item_key_unique" on "inbox_dismissals" ("item_key");
CREATE TABLE IF NOT EXISTS "briefings" ("id" integer primary key autoincrement not null, "date" date not null, "status" varchar not null, "body" text, "next_steps" text, "covers_from" datetime not null, "covers_until" datetime not null, "generated_at" datetime, "created_at" datetime, "updated_at" datetime);
CREATE UNIQUE INDEX "briefings_date_unique" on "briefings" ("date");
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
