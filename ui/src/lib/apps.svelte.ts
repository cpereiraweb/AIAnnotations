import { apps, describeError, onChanged } from './api';
import { toast } from './toast.svelte';
import type { AppsView } from './types';

/**
 * Detected editors/terminals/shells and the user's choices. Shared by the Settings screen and the
 * "open with" menus; reloads whenever the backend reports a change.
 */
export const appsState = $state<{ view: AppsView | null }>({ view: null });

let stopListening: (() => void) | null = null;
let loadCounter = 0;

/** Reads the view; `refresh` probes the machine again. A failure toasts and keeps what is on screen. */
export async function loadApps(refresh = false): Promise<void> {
  stopListening ??= onChanged(() => void loadApps());
  const mine = ++loadCounter;
  try {
    const view = await apps(refresh);
    if (mine === loadCounter) appsState.view = view;
  } catch (e) {
    toast(describeError(e));
  }
}

/** Tests: forgets the view and the subscription. */
export function resetAppsState(): void {
  stopListening?.();
  stopListening = null;
  appsState.view = null;
}
