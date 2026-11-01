// In-memory backend for a plain browser (`npm run dev:mock`) and for component tests. It answers
// the same commands as crates/app with realistic fixtures; mutating calls change the fixtures
// and fire the `onChanged` callbacks, like the worker's "changed" event.
import {
  COUNT_KEYS,
  type Briefing,
  type Choices,
  type CountKey,
  type HistoryCounts,
  type ImportDone,
  type ImportProgress,
  type InboxGroup,
  type InboxItem,
  type Lang,
  type OnboardingView,
  type Ongoing,
  type OngoingGroup,
  type Page,
  type ProjectCard,
  type ProjectDetail,
  type RunCard,
  type SearchHit,
  type SessionFilter,
  type SettingsPatch,
  type TodayView,
  type Tool,
  type ToolUsage,
  type WorkerStatus,
} from './types';
import { getLang } from './i18n';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const ONGOING_STALE_DAYS = 30;
const INBOX_RECENT_DAYS = 30;
const SESSIONS_PER_PAGE = 30;
const PROJECT_RUNS_PER_PAGE = 20;
const SEARCH_LIMIT = 12;

interface Project {
  id: number;
  name: string;
  path: string;
  remote_url: string | null;
}

interface State {
  now: number;
  projects: Project[];
  runs: RunCard[];
  items: InboxItem[];
  ongoings: Ongoing[];
  briefing: Briefing;
  briefingRunning: boolean;
  importing: boolean;
  /** Last text the backend wrote to the clipboard. */
  clipboard: string | null;
  backfill: [number, number] | null;
  worker: WorkerStatus;
  onboarding: OnboardingView;
  /** The system language the backend detected (`lang` follows it when there is no choice). */
  detectedLang: Lang;
  /** Summaries paused (the first-run pause until the wizard is finished). */
  paused: boolean;
  /** What `finish_onboarding` / `save_settings` last wrote (tests). */
  choices: Choices | null;
}

export interface MockOptions {
  /** Fresh install: no sessions, projects or inbox ("Importar histórico"). */
  empty?: boolean;
  /** Clock of the fixtures; defaults to `Date.now()`. */
  now?: number;
  /** Delay of the background jobs (regenerate, import), in ms. */
  delayMs?: number;
  /** The first-run wizard was finished; `false` starts the app on it. Defaults to `true`. */
  onboardingDone?: boolean;
  /** The Codex CLI is installed (the Claude one always is). */
  codexFound?: boolean;
  /** The backend's effective language; defaults to the current UI language. */
  lang?: Lang;
}

const IMPORT_STEPS = 5;
const IMPORT_TOTAL = 50;

const HOOK_DIFF = `--- ~/.claude/settings.json
+++ ~/.claude/settings.json
@@ -1,3 +1,17 @@
 {
+  "hooks": {
+    "SessionStart": [
+      { "hooks": [{ "type": "command", "command": "'/opt/aiannotations/aiannotations' hook start", "timeout": 5 }] }
+    ],
+    "SessionEnd": [
+      { "hooks": [{ "type": "command", "command": "'/opt/aiannotations/aiannotations' hook end", "timeout": 5 }] }
+    ]
+  },
   "theme": "dark"
 }
`;

function fixtureOnboarding(done: boolean, codexFound: boolean, lang: Lang): OnboardingView {
  return {
    done,
    lang,
    lang_choice: null,
    cli_found: true,
    cards: [
      {
        tool: 'claude',
        bin: '/usr/bin/claude',
        config_path: '/home/dev/.claude/settings.json',
        hook_status: 'not_installed',
        invalid_json: false,
        legacy: false,
        diff: HOOK_DIFF,
        diff_replace: null,
        can_write: true,
        diff_remove: null,
      },
      {
        tool: 'codex',
        bin: codexFound ? '/usr/bin/codex' : null,
        config_path: '/home/dev/.codex/hooks.json',
        hook_status: 'not_installed',
        invalid_json: false,
        legacy: false,
        diff: codexFound ? HOOK_DIFF.replaceAll('.claude/settings.json', '.codex/hooks.json') : null,
        diff_replace: null,
        can_write: codexFound,
        diff_remove: null,
      },
    ],
    provider: 'claude',
    claude_model: 'haiku',
    claude_briefing_model: 'sonnet',
    codex_model: null,
    codex_briefing_model: null,
    claude_models: ['haiku', 'sonnet', 'opus', 'fable'].map((m) => ({ value: m, label: m })),
    codex_models: [
      { value: 'gpt-mock', label: 'GPT Mock' },
      { value: 'gpt-mock-mini', label: 'GPT Mock Mini' },
    ],
    summarize_history: false,
    autostart: true,
    history: { sessions: 0, projects: 0, claude: 0, codex: 0 },
    pending_backfill: 0,
  };
}

const PROJECTS: Project[] = [
  {
    id: 1,
    name: 'aiannotations',
    path: '/home/dev/Code/aiannotations',
    remote_url: 'git@github.com:exemplo/aiannotations.git',
  },
  {
    id: 2,
    name: 'loja-virtual',
    path: '/home/dev/Code/loja virtual',
    remote_url: 'https://github.com/exemplo/loja-virtual.git',
  },
  { id: 3, name: 'infra-scripts', path: '/home/dev/Code/infra-scripts', remote_url: null },
];

function run(now: number, p: Project, over: Partial<RunCard> & { run_id: number; session_id: number }): RunCard {
  return {
    tool: 'claude',
    tool_session_id: `0199${String(over.session_id).padStart(4, '0')}-aaaa-7000-8000-00000000000${over.session_id % 10}`,
    project_id: p.id,
    project_name: p.name,
    cwd: p.path,
    branch: 'main',
    title: null,
    summary: null,
    next_step: null,
    summary_status: 'done',
    started_at_ms: now - DAY,
    ended_at_ms: now - DAY + 40 * MIN,
    end_reason: 'prompt_input_exit',
    ended_cleanly: true,
    changed_files: true,
    files_count: 3,
    hidden_label: null,
    root_session_id: over.session_id,
    ...over,
  };
}

function fixtureRuns(now: number): RunCard[] {
  const [a, b, c] = PROJECTS;
  const runs: RunCard[] = [
    // aiannotations: a /clear chain 10 → 11 → 12, the last one interrupted.
    run(now, a, {
      run_id: 1,
      session_id: 10,
      title: 'Worker em segundo plano',
      branch: 'feat/worker',
      summary: 'Implementou o worker com trava em app_state e os testes de concorrência.',
      next_step: 'Ligar o worker à thread do app.',
      started_at_ms: now - 2 * HOUR - 50 * MIN,
      ended_at_ms: now - 2 * HOUR - 5 * MIN,
      end_reason: 'clear',
      files_count: 6,
    }),
    run(now, a, {
      run_id: 2,
      session_id: 11,
      title: 'Worker: ajustes da revisão',
      branch: 'feat/worker',
      summary: 'Corrigiu o TTL da trava e o fingerprint do collect.',
      started_at_ms: now - 2 * HOUR,
      ended_at_ms: now - 80 * MIN,
      end_reason: 'clear',
      root_session_id: 10,
      files_count: 2,
    }),
    run(now, a, {
      run_id: 3,
      session_id: 12,
      title: 'Worker: casco Tauri',
      branch: 'feat/worker',
      summary: 'Começou o casco Tauri com os comandos invoke.',
      next_step: 'Terminar a bandeja e o single-instance.',
      started_at_ms: now - 75 * MIN,
      ended_at_ms: now - 30 * MIN,
      end_reason: 'other',
      ended_cleanly: false,
      root_session_id: 10,
      files_count: 1,
    }),
    // aiannotations: Codex, still open.
    run(now, a, {
      run_id: 4,
      session_id: 13,
      tool: 'codex',
      title: 'Revisão do i18n',
      branch: 'feat/plano-3-app',
      summary: null,
      summary_status: 'pending',
      started_at_ms: now - 12 * MIN,
      ended_at_ms: null,
      end_reason: null,
      ended_cleanly: false,
      changed_files: null,
      files_count: 0,
    }),
    // loja-virtual: half done (Codex idle), talk only, yesterday.
    run(now, b, {
      run_id: 5,
      session_id: 20,
      tool: 'codex',
      title: 'Checkout com Pix',
      branch: 'feat/pix',
      summary: 'Integrou o Pix no checkout; faltam os webhooks de confirmação.',
      next_step: 'Implementar o webhook de pagamento confirmado.',
      started_at_ms: now - DAY - 3 * HOUR,
      ended_at_ms: now - DAY - 2 * HOUR,
      end_reason: 'idle',
      ended_cleanly: false,
      files_count: 9,
    }),
    run(now, b, {
      run_id: 6,
      session_id: 21,
      title: 'Dúvida sobre o frete',
      branch: 'main',
      summary: 'Conversa sobre a regra de frete grátis acima de R$ 200.',
      started_at_ms: now - 3 * DAY,
      ended_at_ms: now - 3 * DAY + 15 * MIN,
      changed_files: false,
      files_count: 0,
    }),
    run(now, b, {
      run_id: 7,
      session_id: 22,
      title: 'Migração do banco',
      branch: 'chore/migrations',
      summary: null,
      summary_status: 'failed',
      started_at_ms: now - 9 * DAY,
      ended_at_ms: now - 9 * DAY + 2 * HOUR,
      end_reason: 'logout',
      files_count: 4,
    }),
    // infra-scripts: older work.
    run(now, c, {
      run_id: 8,
      session_id: 30,
      title: 'Backup do servidor',
      branch: 'main',
      summary: 'Ajustou o script de backup para rodar com rclone e rotação de 7 dias.',
      started_at_ms: now - 20 * DAY,
      ended_at_ms: now - 20 * DAY + HOUR,
      files_count: 1,
    }),
    // Hidden runs (only with "mostrar sessões ocultas").
    run(now, a, {
      run_id: 9,
      session_id: 14,
      title: null,
      summary: null,
      summary_status: 'skipped',
      started_at_ms: now - 5 * HOUR,
      ended_at_ms: now - 5 * HOUR + MIN,
      changed_files: false,
      files_count: 0,
      hidden_label: 'empty',
    }),
    run(now, a, {
      run_id: 10,
      session_id: 15,
      title: 'claude -p resumo',
      summary: null,
      summary_status: 'skipped',
      started_at_ms: now - 6 * HOUR,
      ended_at_ms: now - 6 * HOUR + MIN,
      changed_files: false,
      files_count: 0,
      hidden_label: 'automation',
    }),
    run(now, a, {
      run_id: 11,
      session_id: 16,
      title: 'Explorar o inbox',
      summary: 'Subagente leu as regras do inbox.',
      started_at_ms: now - 7 * HOUR,
      ended_at_ms: now - 7 * HOUR + 4 * MIN,
      changed_files: false,
      files_count: 0,
      hidden_label: 'subagent',
    }),
    run(now, b, {
      run_id: 12,
      session_id: 23,
      tool: 'codex',
      title: null,
      summary: null,
      summary_status: 'skipped',
      started_at_ms: now - 2 * DAY,
      ended_at_ms: now - 2 * DAY + MIN,
      changed_files: false,
      files_count: 0,
      hidden_label: 'internal',
    }),
  ];
  return runs;
}

function item(over: Partial<InboxItem> & Pick<InboxItem, 'key' | 'kind' | 'badge' | 'project_id' | 'title'>): InboxItem {
  return {
    detail: null,
    state_hash: `${over.key}:1`,
    session_id: null,
    reference_ms: null,
    path: null,
    ended_at_ms: null,
    ahead: null,
    age_days: null,
    ...over,
  };
}

function fixtureItems(now: number): InboxItem[] {
  const [a, b, c] = PROJECTS;
  return [
    item({
      key: `interrupted:${a.id}:feat/worker`,
      kind: 'interrupted',
      badge: 'interrupted',
      project_id: a.id,
      title: `${a.name} · feat/worker`,
      session_id: 12,
      reference_ms: now - 30 * MIN,
      ended_at_ms: now - 30 * MIN,
    }),
    item({
      key: `worktree:${a.id}:/home/dev/Code/aiannotations-wt/spike`,
      kind: 'worktree',
      badge: 'worktree',
      project_id: a.id,
      title: `${a.name} · spike/tray`,
      detail: '/home/dev/Code/aiannotations-wt/spike',
      reference_ms: now - 6 * DAY,
    }),
    item({
      key: `reminder:${a.id}:199`,
      kind: 'reminder',
      badge: 'reminder',
      project_id: a.id,
      title: '#199 Ícone da bandeja no macOS',
      detail: a.name,
    }),
    item({
      key: `interrupted:${b.id}:feat/pix`,
      kind: 'interrupted',
      badge: 'half_done',
      project_id: b.id,
      title: `${b.name} · feat/pix`,
      session_id: 20,
      reference_ms: now - DAY - 2 * HOUR,
      ended_at_ms: now - DAY - 2 * HOUR,
    }),
    item({
      key: `stale-branch:${b.id}:chore/migrations`,
      kind: 'stale_branch',
      badge: 'stale_branch',
      project_id: b.id,
      title: `${b.name} · chore/migrations`,
      reference_ms: now - 5 * DAY,
      ahead: 3,
      age_days: 5,
    }),
    item({
      key: `ongoing:${b.id}:2026-10-28-checkout-pix.md`,
      kind: 'ongoing',
      badge: 'ongoing',
      project_id: b.id,
      title: '2026-10-28-checkout-pix',
      detail: b.name,
      path: `${b.path}/docs/ongoing-sessions/2026-10-28-checkout-pix.md`,
      reference_ms: now - 2 * DAY,
    }),
    item({
      key: `dirty:${c.id}`,
      kind: 'dirty',
      badge: 'uncommitted_changes',
      project_id: c.id,
      title: c.name,
      reference_ms: now - 12 * DAY,
    }),
    // Older than 30 days: the collapsed "Parado há mais tempo".
    item({
      key: `stale-branch:${c.id}:feat/monitoring`,
      kind: 'stale_branch',
      badge: 'stale_branch',
      project_id: c.id,
      title: `${c.name} · feat/monitoring`,
      reference_ms: now - 45 * DAY,
      ahead: 1,
      age_days: 45,
    }),
  ];
}

function ongoing(now: number, p: Project, name: string, title: string, ageMs: number): Ongoing {
  return {
    project_id: p.id,
    project_name: p.name,
    project_path: p.path,
    file: `${name}.md`,
    name,
    title,
    modified_ms: now - ageMs,
  };
}

function fixtureOngoings(now: number): Ongoing[] {
  const [a, b, c] = PROJECTS;
  return [
    ongoing(now, a, '2026-10-30-casco-tauri', 'Casco Tauri e frontend base', 40 * MIN),
    ongoing(now, a, '2026-10-27-worker', 'Worker em segundo plano', 3 * DAY),
    ongoing(now, b, '2026-10-28-checkout-pix', 'Checkout com Pix', 2 * DAY),
    ongoing(now, c, '2026-08-10-monitoramento', 'Monitoramento com Uptime Kuma', 81 * DAY),
  ];
}

function localDate(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function fixtureBriefing(now: number, empty: boolean): Briefing {
  return {
    id: 1,
    date: localDate(now),
    status: empty ? 'generating' : 'done',
    body: empty
      ? null
      : 'Ontem o foco foi o worker do aiannotations: a trava e o fingerprint do collect foram corrigidos, ' +
        'e a última sessão parou no meio do casco Tauri.\n' +
        'Na loja-virtual, o checkout com Pix ficou pela metade, faltando o webhook de confirmação.',
    next_steps: empty
      ? []
      : [
          { project_id: 1, projeto: 'aiannotations', passo: 'Terminar a bandeja e o single-instance do casco Tauri.' },
          { project_id: 2, projeto: 'loja-virtual', passo: 'Implementar o webhook de pagamento confirmado do Pix.' },
        ],
    covers_from_ms: now - DAY,
    covers_until_ms: now - 2 * HOUR,
    generated_at_ms: empty ? null : now - 2 * HOUR,
  };
}

function fresh(options: MockOptions): State {
  const now = options.now ?? Date.now();
  const empty = options.empty ?? false;
  return {
    now,
    projects: empty ? [] : PROJECTS.map((p) => ({ ...p })),
    runs: empty ? [] : fixtureRuns(now),
    items: empty ? [] : fixtureItems(now),
    ongoings: empty ? [] : fixtureOngoings(now),
    briefing: fixtureBriefing(now, empty),
    briefingRunning: false,
    importing: false,
    clipboard: null,
    backfill: empty ? null : [42, 120],
    onboarding: fixtureOnboarding(options.onboardingDone ?? true, options.codexFound ?? false, options.lang ?? getLang()),
    detectedLang: options.lang ?? getLang(),
    paused: !(options.onboardingDone ?? true),
    choices: null,
    worker: {
      last_tick_ms: now - 10_000,
      paused_until_ms: null,
      pause_reason: null,
      summarizing_paused_by_user: false,
      last_errors: [],
    },
  };
}

function groups(items: InboxItem[]): InboxGroup[] {
  const byProject = new Map<number, InboxItem[]>();
  for (const i of items) byProject.set(i.project_id, [...(byProject.get(i.project_id) ?? []), i]);
  const out: InboxGroup[] = [];
  for (const [project_id, list] of byProject) {
    const sorted = [...list].sort((x, y) => Number(y.kind === 'interrupted') - Number(x.kind === 'interrupted'));
    const counts = new Map<CountKey, number>();
    for (const i of list) {
      const key: CountKey = i.kind === 'interrupted' ? (i.badge === 'half_done' ? 'half_done' : 'interrupted') : i.kind;
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    out.push({
      project_id,
      name: PROJECTS.find((p) => p.id === project_id)?.name ?? '',
      items: sorted,
      expanded: list.some((i) => i.kind === 'interrupted'),
      latest_ms: Math.max(0, ...list.map((i) => i.reference_ms ?? 0)),
      counts: COUNT_KEYS.filter((k) => counts.has(k)).map((k) => [k, counts.get(k)!] as [CountKey, number]),
    });
  }
  return out.sort(
    (x, y) => Number(y.expanded) - Number(x.expanded) || y.latest_ms - x.latest_ms || x.name.localeCompare(y.name),
  );
}

function groupOngoings(list: Ongoing[]): OngoingGroup[] {
  const out: OngoingGroup[] = [];
  for (const o of [...list].sort((x, y) => y.modified_ms - x.modified_ms)) {
    const g = out.find((g) => g.project_id === o.project_id);
    if (g) g.items.push(o);
    else out.push({ project_id: o.project_id, project_name: o.project_name, items: [o] });
  }
  return out;
}

function paginate(list: RunCard[], perPage: number, page: number): Page<RunCard> {
  const p = Math.max(1, page);
  const sorted = [...list].sort((x, y) => y.started_at_ms - x.started_at_ms || y.run_id - x.run_id);
  return {
    items: sorted.slice((p - 1) * perPage, p * perPage),
    page: p,
    pages: Math.max(1, Math.ceil(sorted.length / perPage)),
    total: sorted.length,
  };
}

function posixQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

export class MockBackend {
  private state: State;
  private listeners = new Set<() => void>();
  private navigateListeners = new Set<(hash: string) => void>();
  private events = new Map<string, Set<(payload: unknown) => void>>();
  private delayMs: number;

  constructor(options: MockOptions = {}) {
    this.state = fresh(options);
    this.delayMs = options.delayMs ?? 1500;
  }

  /** Back to the fixtures (tests); listeners are kept. */
  reset(options: MockOptions = {}): void {
    this.state = fresh(options);
    this.delayMs = options.delayMs ?? this.delayMs;
  }

  /** What `copy_resume_command` / `copy_text` last wrote (tests). */
  get clipboard(): string | null {
    return this.state.clipboard;
  }

  onChanged(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  onNavigate(cb: (hash: string) => void): () => void {
    this.navigateListeners.add(cb);
    return () => this.navigateListeners.delete(cb);
  }

  /** Subscribes to a backend event (`import-progress`, `import-done`). */
  onEvent(name: string, cb: (payload: unknown) => void): () => void {
    const set = this.events.get(name) ?? new Set();
    set.add(cb);
    this.events.set(name, set);
    return () => set.delete(cb);
  }

  private emit(name: string, payload: unknown): void {
    for (const cb of [...(this.events.get(name) ?? [])]) cb(payload);
  }

  /** What `finish_onboarding` last received (tests). */
  get choices(): Choices | null {
    return this.state.choices;
  }

  /** What the tray's "Hoje" does: emits "navigate" with the hash. */
  navigate(hash: string): void {
    for (const cb of [...this.navigateListeners]) cb(hash);
  }

  private changed(): void {
    for (const cb of [...this.listeners]) cb();
  }

  private historyCounts(): HistoryCounts {
    const sessions = new Set(this.state.runs.map((r) => r.session_id));
    const ofTool = (tool: Tool) => new Set(this.state.runs.filter((r) => r.tool === tool).map((r) => r.session_id)).size;
    return {
      sessions: sessions.size,
      projects: new Set(this.state.runs.map((r) => r.project_id)).size,
      claude: ofTool('claude'),
      codex: ofTool('codex'),
    };
  }

  private later(fn: () => void): void {
    setTimeout(() => {
      fn();
      this.changed();
    }, this.delayMs);
  }

  /** Dispatches a command by name, like `invoke`. Results are copies. */
  async call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
    const result = this.handle(cmd, args);
    return structuredClone(result) as T;
  }

  private visibleRuns(): RunCard[] {
    return this.state.runs.filter((r) => r.hidden_label === null);
  }

  private card(p: Project): ProjectCard {
    const s = this.state;
    const visible = this.visibleRuns().filter((r) => r.project_id === p.id);
    const tools = new Map<Tool, boolean>();
    for (const r of visible.filter((r) => r.started_at_ms >= s.now - 14 * DAY)) {
      tools.set(r.tool, (tools.get(r.tool) ?? false) || r.changed_files === true);
    }
    const last = paginate(visible, 1, 1).items[0] ?? null;
    return {
      id: p.id,
      name: p.name,
      path: p.path,
      remote_url: p.remote_url,
      last_activity_ms: last ? (last.ended_at_ms ?? last.started_at_ms) : null,
      ongoing_count: s.ongoings.filter((o) => o.project_id === p.id).length,
      tools_14d: [...tools]
        .sort(([x], [y]) => x.localeCompare(y))
        .map(([tool, changed_files]): ToolUsage => ({ tool, changed_files })),
      last_run: last,
    };
  }

  private run(runId: unknown): RunCard {
    const r = this.state.runs.find((r) => r.run_id === Number(runId));
    if (!r) throw `run not found: ${String(runId)}`;
    return r;
  }

  /** The background generation: `generating` now, `done` after the delay. */
  private startBriefing(): void {
    const s = this.state;
    s.briefingRunning = true;
    s.briefing = { ...s.briefing, status: 'generating' };
    this.later(() => {
      s.briefingRunning = false;
      const generated = fixtureBriefing(s.now, false);
      s.briefing =
        s.runs.length === 0
          ? { ...generated, body: 'Nenhuma sessão desde o último briefing.', next_steps: [], generated_at_ms: Date.now() }
          : { ...generated, generated_at_ms: Date.now() };
    });
  }

  private handle(cmd: string, args: Record<string, unknown>): unknown {
    const s = this.state;
    switch (cmd) {
      case 'today': {
        // Like the app: opening Today starts a `generating` briefing nothing is working on.
        if (s.briefing.status === 'generating' && !s.briefingRunning) this.startBriefing();
        const cutoff = s.now - INBOX_RECENT_DAYS * DAY;
        const isOlder = (i: InboxItem) => i.reference_ms !== null && i.reference_ms < cutoff;
        const lastRuns: Record<string, RunCard> = {};
        for (const i of s.items) {
          if (i.session_id === null) continue;
          const last = s.runs
            .filter((r) => r.session_id === i.session_id)
            .sort((x, y) => y.started_at_ms - x.started_at_ms)[0];
          if (last) lastRuns[String(i.session_id)] = last;
        }
        const stale = s.now - ONGOING_STALE_DAYS * DAY;
        const view: TodayView = {
          briefing: s.briefing,
          briefing_running: s.briefingRunning,
          ongoings_recent: groupOngoings(s.ongoings.filter((o) => o.modified_ms >= stale)),
          ongoings_stale: groupOngoings(s.ongoings.filter((o) => o.modified_ms < stale)),
          inbox: { recent: groups(s.items.filter((i) => !isOlder(i))), older: groups(s.items.filter(isOlder)) },
          inbox_last_runs: lastRuns,
          next_steps: s.briefing.status === 'done' ? s.briefing.next_steps : [],
          backfill: s.backfill,
          is_empty_db: s.runs.length === 0,
          summaries_paused: s.paused,
          briefing_start_failed: false,
          hooks_need_update: false,
        };
        return view;
      }
      case 'projects':
        return s.projects.map((p) => this.card(p)).sort((x, y) => (y.last_activity_ms ?? -1) - (x.last_activity_ms ?? -1));
      case 'project_detail': {
        const p = s.projects.find((p) => p.id === Number(args.id));
        if (!p) return null;
        const detail: ProjectDetail = {
          card: this.card(p),
          ongoings: s.ongoings.filter((o) => o.project_id === p.id).sort((x, y) => y.modified_ms - x.modified_ms),
        };
        return detail;
      }
      case 'sessions': {
        const f = (args.filter ?? {}) as SessionFilter;
        const list = this.visibleRuns().filter(
          (r) =>
            (!f.tool || r.tool === f.tool) &&
            (f.project_id == null || r.project_id === f.project_id) &&
            (!f.kind || (f.kind === 'changed' ? r.changed_files === true : r.changed_files !== true)),
        );
        return paginate(list, SESSIONS_PER_PAGE, Number(args.page ?? 1));
      }
      case 'project_runs': {
        const list = (args.include_hidden ? s.runs : this.visibleRuns()).filter((r) => r.project_id === Number(args.id));
        return paginate(list, PROJECT_RUNS_PER_PAGE, Number(args.page ?? 1));
      }
      case 'search': {
        const words = String(args.text ?? '')
          .toLowerCase()
          .split(/\s+/)
          .filter(Boolean);
        if (words.length === 0) return [];
        return paginate(this.visibleRuns(), s.runs.length || 1, 1)
          .items.filter((r) => {
            const hay = [r.project_name, r.branch, r.title, r.summary, r.next_step].join(' ').toLowerCase();
            return words.every((w) => hay.includes(w));
          })
          .slice(0, SEARCH_LIMIT)
          .map(
            (r): SearchHit => ({
              run_id: r.run_id,
              ai_session_id: r.session_id,
              project_id: r.project_id,
              project_name: r.project_name,
              tool: r.tool,
              branch: r.branch,
              title: r.title,
              summary: r.summary,
              next_step: r.next_step,
              started_at_ms: r.started_at_ms,
            }),
          );
      }
      case 'dismiss': {
        // Like the real command: no "changed" event; the screen reloads after the call.
        s.items = s.items.filter((i) => !(i.key === args.key && i.state_hash === args.state_hash));
        return null;
      }
      case 'dismiss_group': {
        // Only the section the button sits in (recent or older), as the v1.
        const cutoff = s.now - INBOX_RECENT_DAYS * DAY;
        const older = Boolean(args.older);
        const inSection = (i: InboxItem) => (i.reference_ms !== null && i.reference_ms < cutoff) === older;
        const before = s.items.length;
        s.items = s.items.filter((i) => !(i.project_id === Number(args.project_id) && inSection(i)));
        return before - s.items.length;
      }
      case 'regenerate_briefing': {
        if (s.briefingRunning) throw 'already_running';
        this.startBriefing();
        this.changed();
        return null;
      }
      case 'import_history': {
        if (s.importing) throw 'import_running';
        s.importing = true;
        // Five progress events, then the final one with the counts, like the app.
        for (let i = 1; i <= IMPORT_STEPS; i++) {
          const p: ImportProgress = { done: (IMPORT_TOTAL / IMPORT_STEPS) * i, total: IMPORT_TOTAL };
          setTimeout(() => this.emit('import-progress', p), (this.delayMs / IMPORT_STEPS) * i);
        }
        this.later(() => {
          s.importing = false;
          if (s.runs.length === 0) {
            const full = fresh({ now: s.now });
            Object.assign(s, { ...full, briefing: s.briefing, onboarding: s.onboarding, backfill: [0, full.runs.length] });
          }
          const done: ImportDone = { counts: this.historyCounts(), error: null };
          this.emit('import-done', done);
        });
        return null;
      }
      case 'onboarding_state': {
        const backfill = s.backfill ? s.backfill[1] - s.backfill[0] : 0;
        return { ...s.onboarding, history: this.historyCounts(), pending_backfill: backfill };
      }
      case 'write_hooks': {
        const card = s.onboarding.cards.find((c) => c.tool === args.tool);
        if (!card?.bin) throw 'tool CLI missing';
        if (!s.onboarding.cli_found) throw 'CLI missing';
        if (card.invalid_json) throw 'invalid file';
        const replace = Boolean(args.replace_legacy) && card.legacy;
        const changed = card.hook_status !== 'current' || replace;
        card.hook_status = 'current';
        card.diff = null;
        if (replace) card.legacy = false;
        // Ours is now current; only the v1 hooks (when kept) are left to replace.
        if (!card.legacy) card.diff_replace = null;
        card.can_write = card.legacy;
        card.diff_remove = HOOK_DIFF.replaceAll('+', '-');
        return { changed, backup: null };
      }
      case 'remove_hooks': {
        const card = s.onboarding.cards.find((c) => c.tool === args.tool);
        if (card?.invalid_json) throw 'invalid file';
        if (!card?.diff_remove) return { changed: false, backup: null };
        card.hook_status = 'not_installed';
        card.diff_remove = null;
        card.diff = HOOK_DIFF;
        card.can_write = card.bin !== null && s.onboarding.cli_found;
        return { changed: true, backup: null };
      }
      case 'finish_onboarding': {
        s.choices = args.choices as Choices;
        s.onboarding.done = true;
        s.paused = false;
        return null;
      }
      case 'save_settings': {
        if (!s.onboarding.done) throw 'onboarding pending';
        const patch = args.patch as Partial<SettingsPatch>;
        const o = s.onboarding;
        if (patch.provider !== undefined) o.provider = patch.provider;
        if (patch.claude_model !== undefined) o.claude_model = patch.claude_model;
        if (patch.claude_briefing_model !== undefined) o.claude_briefing_model = patch.claude_briefing_model;
        if (patch.codex_model !== undefined) o.codex_model = patch.codex_model || null;
        if (patch.codex_briefing_model !== undefined) o.codex_briefing_model = patch.codex_briefing_model || null;
        if (patch.summarize_history !== undefined) o.summarize_history = patch.summarize_history;
        if (patch.autostart !== undefined) o.autostart = patch.autostart;
        if (patch.lang !== undefined) {
          o.lang_choice = patch.lang;
          o.lang = patch.lang ?? s.detectedLang;
        }
        s.choices = {
          provider: o.provider,
          claude_model: o.claude_model,
          claude_briefing_model: o.claude_briefing_model,
          codex_model: o.codex_model ?? '',
          codex_briefing_model: o.codex_briefing_model ?? '',
          summarize_history: o.summarize_history,
          autostart: o.autostart,
        };
        this.changed();
        return null;
      }
      case 'set_summaries_paused': {
        // Resuming is ignored until the wizard is finished (no AI call before).
        s.paused = Boolean(args.paused) || !s.onboarding.done;
        s.worker.summarizing_paused_by_user = s.paused;
        this.changed();
        return s.paused;
      }
      case 'copy_resume_command': {
        const r = this.run(args.run_id);
        if (!r.cwd) throw 'session has no folder';
        const bare = r.tool === 'claude' ? `claude --resume ${r.tool_session_id}` : `codex resume ${r.tool_session_id}`;
        s.clipboard = `cd ${posixQuote(r.cwd)} && ${bare}`;
        return s.clipboard;
      }
      case 'copy_text': {
        s.clipboard = String(args.text);
        return null;
      }
      case 'open_run_folder': {
        if (!this.run(args.run_id).cwd) throw 'session has no folder';
        return null;
      }
      case 'open_project_folder': {
        if (!s.projects.some((p) => p.id === Number(args.project_id))) {
          throw `project not found: ${String(args.project_id)}`;
        }
        return null;
      }
      case 'worker_status':
        return s.worker;
      default:
        throw `unknown command: ${cmd}`;
    }
  }
}

function startEmpty(): boolean {
  return typeof location !== 'undefined' && new URLSearchParams(location.search).has('empty');
}

/** The shared instance used by `api.ts` outside Tauri. `?empty` in the URL starts a fresh install. */
export const mock = new MockBackend({
  empty: startEmpty(),
  // `npm run dev:mock` starts on the wizard; the tests start past it.
  onboardingDone: import.meta.env.MODE === 'test',
});
