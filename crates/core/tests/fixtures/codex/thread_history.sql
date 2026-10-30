-- Fixture sintética das bases do Codex no modo paginado.
-- O teste separa as seções pelos marcadores de seção (comentário, arroba, "database", nome do arquivo) e
-- executa cada uma no arquivo SQLite correspondente dentro de um codex_home temporário.

-- @database state_5.sqlite
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    rollout_path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    source TEXT NOT NULL,
    model_provider TEXT NOT NULL,
    cwd TEXT NOT NULL,
    title TEXT NOT NULL,
    git_branch TEXT,
    model TEXT,
    created_at_ms INTEGER,
    updated_at_ms INTEGER,
    history_mode TEXT NOT NULL DEFAULT 'legacy'
);
INSERT INTO threads VALUES ('0199bbbb-0000-7000-8000-000000000001', '/nao/existe/rollout-1.jsonl', 1792501200, 1792501500, 'vscode', 'openai', '/home/fulano/Code/Agenda Escolar', 'Agenda com lembretes', 'main', 'gpt-modelo-c', 1792501200000, 1792501500000, 'paginated');
INSERT INTO threads VALUES ('0199bbbb-0000-7000-8000-000000000002', '/nao/existe/rollout-2.jsonl', 1792501300, 1792501400, '{"subagent":{"thread_spawn":{"parent_thread_id":"0199bbbb-0000-7000-8000-000000000001","depth":1,"agent_path":"/root/revisor","agent_nickname":"Revisor","agent_role":null}}}', 'openai', '/home/fulano/Code/Agenda Escolar', '', NULL, 'gpt-modelo-c', 1792501300000, 1792501400000, 'paginated');
INSERT INTO threads VALUES ('0199bbbb-0000-7000-8000-000000000003', '/nao/existe/rollout-3.jsonl', 1792501300, 1792501400, '{"subagent":{"other":"guardian"}}', 'openai', '/home/fulano/Code/Agenda Escolar', '', NULL, 'gpt-modelo-c', 1792501300000, 1792501400000, 'paginated');
INSERT INTO threads VALUES ('0199bbbb-0000-7000-8000-000000000004', '/nao/existe/rollout-4.jsonl', 1792501300, 1792501400, 'exec', 'openai', '/home/fulano/Code/Agenda Escolar', 'Rodada automática', NULL, 'gpt-modelo-c', 1792501300000, 1792501400000, 'paginated');

-- @database thread_history_1.sqlite
CREATE TABLE thread_items (
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    rollout_ordinal INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    item_json TEXT NOT NULL,
    item_type TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (thread_id, turn_id, item_id)
);
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i1', 1, 1792501201000, '{"type":"userMessage","id":"i1","clientId":null,"content":[{"type":"text","text":"Crie a tela de lembretes."},{"type":"localImage","path":"/tmp/print.png"}]}', 'userMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i2', 2, 1792501202000, '{"type":"reasoning","id":"i2","summary":[]}', 'reasoning');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i3', 3, 1792501203000, '{"type":"agentMessage","id":"i3","text":"Vou criar a tela.","phase":"commentary"}', 'agentMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i4', 4, 1792501204000, '{"type":"commandExecution","id":"i4","command":"ls","status":"completed"}', 'commandExecution');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i5', 5, 1792501205000, '{"type":"fileChange","id":"i5","status":"completed","changes":[{"path":"/home/fulano/Code/Agenda Escolar/lembretes.py","kind":{"type":"add"},"diff":"+def lembrar(): pass"},{"path":"/home/fulano/Code/Agenda Escolar/app.py","kind":{"type":"update","move_path":null},"diff":"-a\n+b"}]}', 'fileChange');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i6', 6, 1792501206000, '{"type":"mcpToolCall","id":"i6","server":"exemplo","tool":"buscar","status":"completed"}', 'mcpToolCall');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't1', 'i7', 7, 1792501207000, '{"type":"agentMessage","id":"i7","text":"Tela de lembretes criada.","phase":"final_answer"}', 'agentMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't2', 'i8', 8, 1792501400000, '{"type":"userMessage","id":"i8","clientId":null,"content":[{"type":"text","text":"Ajuste o horário padrão."}]}', 'userMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't2', 'i9', 9, 1792501401000, '{"type":"fileChange","id":"i9","status":"completed","changes":[{"path":"/home/fulano/Code/Agenda Escolar/app.py","kind":{"type":"update","move_path":null},"diff":"-b\n+c"}]}', 'fileChange');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000001', 't2', 'i10', 10, 1792501402000, '{"type":"agentMessage","id":"i10","text":"Horário padrão ajustado para 08:00.","phase":"final_answer"}', 'agentMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000002', 't1', 'j1', 1, 1792501301000, '{"type":"userMessage","id":"j1","clientId":null,"content":[{"type":"text","text":"Revise a tela de lembretes."}]}', 'userMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000002', 't1', 'j2', 2, 1792501302000, '{"type":"agentMessage","id":"j2","text":"Revisão concluída.","phase":"final_answer"}', 'agentMessage');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000002', 't1', 'j3', 3, 1792501302000, '{"type":"agentMessage","id":"j3","text":"Esta linha tem JSON inválido"', 'agentMessage');
-- Subagente guardian: só resposta, sem prompt do usuário.
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000003', 't1', 'k1', 1, 1792501301000, '{"type":"commandExecution","id":"k1","command":"git diff","status":"completed"}', 'commandExecution');
INSERT INTO thread_items VALUES ('0199bbbb-0000-7000-8000-000000000003', 't1', 'k2', 2, 1792501302000, '{"type":"agentMessage","id":"k2","text":"Nada suspeito no diff.","phase":"final_answer"}', 'agentMessage');
CREATE TABLE thread_realtime_items (
    thread_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    rollout_ordinal INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    item_type TEXT NOT NULL,
    item_json TEXT NOT NULL,
    PRIMARY KEY (thread_id, item_id)
);
