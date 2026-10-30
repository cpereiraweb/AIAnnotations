// Actions shared by the screens: they talk to the backend and answer with the global toast.
import { copy_resume_command, copy_text, describeError, open_run_folder } from './api';
import { t } from './i18n';
import { toast } from './toast.svelte';
import type { Tool } from './types';

export const TOOL_LABEL: Record<Tool, string> = { claude: 'Claude', codex: 'Codex' };

/** Copies `text` (through the backend) and toasts the outcome. Resolves to whether it was copied. */
export async function copyText(text: string): Promise<boolean> {
  return copied(copy_text(text));
}

/** "▶ retomar": the resume command of the run goes to the clipboard (the MVP does not open a terminal). */
export async function resumeRun(runId: number): Promise<void> {
  await copied(copy_resume_command(runId));
}

async function copied(copy: Promise<unknown>): Promise<boolean> {
  try {
    await copy;
    toast(t('toast.copied'));
    return true;
  } catch {
    toast(t('toast.copy_failed'));
    return false;
  }
}

export async function openRunFolder(runId: number): Promise<void> {
  try {
    await open_run_folder(runId);
  } catch (e) {
    toast(describeError(e));
  }
}
