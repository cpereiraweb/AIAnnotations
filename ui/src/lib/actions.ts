// Actions shared by the screens: they talk to the backend and answer with the global toast.
import {
  describeErrorBare,
  describeError,
  open_project_with,
  open_run_folder,
  resume_ongoing,
  resume_run,
} from './api';
import { t } from './i18n';
import { toast } from './toast.svelte';
import type { ResumeOutcome, Tool } from './types';

export const TOOL_LABEL: Record<Tool, string> = { claude: 'Claude', codex: 'Codex' };

/** Toast for what the backend did with a resume: the command is always on the clipboard. */
function resumeToast(outcome: ResumeOutcome): string {
  switch (outcome.opened) {
    case 'ran':
      return t('toast.resume_ran');
    case 'shell_only':
      return t('toast.resume_shell_only');
    case 'none':
      return t('toast.resume_none');
    case 'failed':
      return t('toast.resume_failed', { error: describeErrorBare(outcome.error ?? '') });
  }
}

async function resumed(resume: Promise<ResumeOutcome>): Promise<void> {
  try {
    toast(resumeToast(await resume));
  } catch {
    toast(t('toast.copy_failed'));
  }
}

/** "▶ retomar": copies the resume command and opens it in the default terminal. */
export async function resumeRun(runId: number): Promise<void> {
  await resumed(resume_run(runId));
}

/** "▶ retomar" of an ongoing: same, with the ongoing's restart prompt. */
export async function resumeOngoing(projectId: number, file: string): Promise<void> {
  await resumed(resume_ongoing(projectId, file));
}

/** Opens the project in the app (`null`: the default editor). Errors go to the toast. */
export async function openProjectWith(projectId: number, appId: string | null = null): Promise<void> {
  try {
    await open_project_with(projectId, appId);
  } catch (e) {
    toast(describeError(e));
  }
}

export async function openRunFolder(runId: number): Promise<void> {
  try {
    await open_run_folder(runId);
  } catch (e) {
    toast(describeError(e));
  }
}
