-- Keeps `search_index` in step with the tables (the SELECT is `search::ROWS`).
-- Renaming a project or retitling a session also reindexes its runs, which the v1 did not.

CREATE TRIGGER search_index_run_insert AFTER INSERT ON session_runs
BEGIN
    DELETE FROM search_index WHERE run_id IN (NEW.id);
    INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact)
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id
    WHERE r.id = NEW.id;
END;

CREATE TRIGGER search_index_run_update
AFTER UPDATE OF branch, summary, next_step, ai_session_id ON session_runs
BEGIN
    DELETE FROM search_index WHERE run_id IN (NEW.id);
    INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact)
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id
    WHERE r.id = NEW.id;
END;

-- Also covers the cascading deletes of projects and sessions.
CREATE TRIGGER search_index_run_delete AFTER DELETE ON session_runs
BEGIN
    DELETE FROM search_index WHERE run_id = OLD.id;
END;

CREATE TRIGGER search_index_session_title
AFTER UPDATE OF title ON ai_sessions WHEN OLD.title IS NOT NEW.title
BEGIN
    DELETE FROM search_index WHERE run_id IN (SELECT id FROM session_runs WHERE ai_session_id = NEW.id);
    INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact)
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id
    WHERE r.ai_session_id = NEW.id;
END;

CREATE TRIGGER search_index_project_name
AFTER UPDATE OF name ON projects WHEN OLD.name IS NOT NEW.name
BEGIN
    DELETE FROM search_index WHERE run_id IN (SELECT r.id FROM session_runs r JOIN ai_sessions s ON s.id = r.ai_session_id WHERE s.project_id = NEW.id);
    INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact)
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id
    WHERE s.project_id = NEW.id;
END;

-- Databases that already had runs get their index built now.
DELETE FROM search_index;
INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact)
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id;
