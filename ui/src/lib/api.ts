// Typed access to the backend: the Tauri commands inside the app; the in-memory mock only in
// `npm run dev:mock` and in tests (loaded with a dynamic import, so it is not in the production
// bundle). Function and argument names match the Rust commands.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t } from './i18n';
import type { MockBackend } from './mock';
import type {
  Choices,
  HookWrite,
  ImportDone,
  ImportProgress,
  OnboardingView,
  Page,
  ProjectCard,
  ProjectDetail,
  RunCard,
  SearchHit,
  SessionFilter,
  Tool,
  TodayView,
  WorkerStatus,
} from './types';

/** Error codes the commands return as plain strings. */
export const ERR_BRIEFING_RUNNING = 'already_running';
export const ERR_IMPORT_RUNNING = 'import_running';
export const ERR_INVALID_FILE = 'invalid file';
export const ERR_CLI_MISSING = 'CLI missing';
export const ERR_TOOL_CLI_MISSING = 'tool CLI missing';
export const ERR_ONBOARDING_PENDING = 'onboarding pending';

/** Event the worker (and the background commands) emit after writing something. */
export const CHANGED_EVENT = 'changed';
/** Event the tray emits to show a screen; the payload is the hash. */
export const NAVIGATE_EVENT = 'navigate';
/** Events of `import_history`: progress while it runs, and one `import-done` when it ends. */
export const IMPORT_PROGRESS_EVENT = 'import-progress';
export const IMPORT_DONE_EVENT = 'import-done';

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export const NO_BACKEND =
  'AIAnnotations backend unavailable: not running inside the Tauri app (use `npm run dev:mock` for the mock backend)';

let mockModule: Promise<MockBackend> | null = null;

/** The mock in mock/test mode; otherwise an error. `MODE` is a build-time constant, so the
 * production build drops the `import()` and the fixtures with it. */
function mockBackend(): Promise<MockBackend> {
  if (import.meta.env.MODE === 'mock' || import.meta.env.MODE === 'test') {
    mockModule ??= import('./mock').then((m) => m.mock);
    return mockModule;
  }
  return Promise.reject(new Error(NO_BACKEND));
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return invoke<T>(cmd, args);
  return (await mockBackend()).call<T>(cmd, args ?? {});
}

export const today = () => call<TodayView>('today');
export const projects = () => call<ProjectCard[]>('projects');
export const project_detail = (id: number) => call<ProjectDetail | null>('project_detail', { id });
export const sessions = (filter: SessionFilter, page: number) =>
  call<Page<RunCard>>('sessions', { filter, page });
export const project_runs = (id: number, include_hidden: boolean, page: number) =>
  call<Page<RunCard>>('project_runs', { id, include_hidden, page });
export const search = (text: string) => call<SearchHit[]>('search', { text });
export const dismiss = (key: string, state_hash: string) => call<void>('dismiss', { key, state_hash });
/** "Já vi tudo": the project's items in one section of the inbox (`older` or recent). */
export const dismiss_group = (project_id: number, older: boolean) =>
  call<number>('dismiss_group', { project_id, older });
/** Starts in the background; rejects with `already_running` while a generation holds the lock. */
export const regenerate_briefing = () => call<void>('regenerate_briefing');
/** Starts the backfill in the background; rejects with `import_running` if one is running. */
export const import_history = () => call<void>('import_history');
/** "▶ retomar": the backend builds the run's resume command, writes it to the clipboard and
 * returns it (a webview clipboard write after an `await` can be refused by WebKit). */
export const copy_resume_command = (run_id: number) => call<string>('copy_resume_command', { run_id });
/** Writes `text` to the system clipboard from the backend. */
export const copy_text = (text: string) => call<void>('copy_text', { text });
export const open_run_folder = (run_id: number) => call<void>('open_run_folder', { run_id });
export const open_project_folder = (project_id: number) =>
  call<void>('open_project_folder', { project_id });
/** The wizard's state: what each AI CLI shows, the current choices and the history counts. */
export const onboarding_state = () => call<OnboardingView>('onboarding_state');
/** Writes our hooks to the tool's config (after the user saw the diff). */
export const write_hooks = (tool: Tool) => call<HookWrite>('write_hooks', { tool });
/** Saves the choices and ends the wizard; resolves to an autostart warning, if any. */
export const finish_onboarding = (choices: Choices) => call<string | null>('finish_onboarding', { choices });
export const worker_status = () => call<WorkerStatus>('worker_status');

/** Calls `cb` whenever the backend wrote something. Returns the unsubscribe function. */
export function onChanged(cb: () => void): () => void {
  return subscribe(
    () => listen(CHANGED_EVENT, () => cb()),
    (m) => m.onChanged(cb),
  );
}

/** Calls `cb` with the hash the tray asks for ("Hoje" sends `#/`). Returns the unsubscribe function. */
export function onNavigate(cb: (hash: string) => void): () => void {
  return subscribe(
    () => listen<string>(NAVIGATE_EVENT, (e) => cb(e.payload)),
    (m) => m.onNavigate(cb),
  );
}

/** Calls `cb` while `import_history` advances. Returns the unsubscribe function. */
export function onImportProgress(cb: (p: ImportProgress) => void): () => void {
  return subscribe(
    () => listen<ImportProgress>(IMPORT_PROGRESS_EVENT, (e) => cb(e.payload)),
    (m) => m.onEvent(IMPORT_PROGRESS_EVENT, (p) => cb(p as ImportProgress)),
  );
}

/** Calls `cb` when `import_history` ends, with the counts or the error. */
export function onImportDone(cb: (d: ImportDone) => void): () => void {
  return subscribe(
    () => listen<ImportDone>(IMPORT_DONE_EVENT, (e) => cb(e.payload)),
    (m) => m.onEvent(IMPORT_DONE_EVENT, (d) => cb(d as ImportDone)),
  );
}

function subscribe(
  tauri: () => Promise<() => void>,
  mocked: (m: MockBackend) => () => void,
): () => void {
  let stop: (() => void) | null = null;
  let stopped = false;
  const subscribed: Promise<() => void> = isTauri() ? tauri() : mockBackend().then(mocked);
  subscribed.then(
    (unlisten) => {
      if (stopped) unlisten();
      else stop = unlisten;
    },
    () => {}, // No backend: nothing will ever come.
  );
  return () => {
    stopped = true;
    stop?.();
  };
}

/** Text for a toast from a rejected command (known codes are translated). */
export function describeError(e: unknown): string {
  const message = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
  if (message === ERR_BRIEFING_RUNNING) return t('toast.briefing_running');
  if (message === ERR_IMPORT_RUNNING) return t('toast.import_running');
  if (message === ERR_INVALID_FILE) return t('toast.invalid_file');
  if (message === ERR_CLI_MISSING) return t('toast.cli_missing');
  if (message === ERR_TOOL_CLI_MISSING) return t('toast.tool_cli_missing');
  if (message === ERR_ONBOARDING_PENDING) return t('toast.onboarding_pending');
  return t('error.generic', { message });
}
