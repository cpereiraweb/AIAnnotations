// Actions shared by the screens: they talk to the backend and answer with the global toast.
import { describeError, open_run_folder, resume_command } from './api';
import { t } from './i18n';
import { toast } from './toast.svelte';
import type { Tool } from './types';

export const TOOL_LABEL: Record<Tool, string> = { claude: 'Claude', codex: 'Codex' };

/** Copies `text` and toasts the outcome. Resolves to whether it was copied. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    toast(t('toast.copied'));
    return true;
  } catch {
    toast(t('toast.copy_failed'));
    return false;
  }
}

/** "▶ retomar": the resume command of the run goes to the clipboard (the MVP does not open a terminal). */
export async function resumeRun(runId: number): Promise<void> {
  let command: string;
  try {
    command = await resume_command(runId);
  } catch (e) {
    toast(describeError(e));
    return;
  }
  await copyText(command);
}

export async function openRunFolder(runId: number): Promise<void> {
  try {
    await open_run_folder(runId);
  } catch (e) {
    toast(describeError(e));
  }
}
