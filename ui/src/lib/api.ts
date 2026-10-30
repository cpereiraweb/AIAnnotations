// Typed access to the backend: the Tauri commands inside the app, the in-memory mock in a plain
// browser (`npm run dev:mock`) and in tests. Function and argument names match the Rust commands.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t } from './i18n';
import { mock } from './mock';
import type {
  Page,
  ProjectCard,
  ProjectDetail,
  RunCard,
  SearchHit,
  SessionFilter,
  TodayView,
  WorkerStatus,
} from './types';

/** Error codes the commands return as plain strings. */
export const ERR_BRIEFING_RUNNING = 'already_running';
export const ERR_IMPORT_RUNNING = 'import_running';

/** Event the worker (and the background commands) emit after writing something. */
export const CHANGED_EVENT = 'changed';

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return isTauri() ? invoke<T>(cmd, args) : mock.call<T>(cmd, args ?? {});
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
export const dismiss_group = (project_id: number) => call<number>('dismiss_group', { project_id });
/** Starts in the background; rejects with `already_running` while a generation holds the lock. */
export const regenerate_briefing = () => call<void>('regenerate_briefing');
/** Starts the backfill in the background; rejects with `import_running` if one is running. */
export const import_history = () => call<void>('import_history');
export const resume_command = (run_id: number) => call<string>('resume_command', { run_id });
export const open_run_folder = (run_id: number) => call<void>('open_run_folder', { run_id });
export const open_project_folder = (project_id: number) =>
  call<void>('open_project_folder', { project_id });
export const worker_status = () => call<WorkerStatus>('worker_status');

/** Calls `cb` whenever the backend wrote something. Returns the unsubscribe function. */
export function onChanged(cb: () => void): () => void {
  if (!isTauri()) return mock.onChanged(cb);
  let stop: (() => void) | null = null;
  let stopped = false;
  void listen(CHANGED_EVENT, () => cb()).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
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
  return t('error.generic', { message });
}
