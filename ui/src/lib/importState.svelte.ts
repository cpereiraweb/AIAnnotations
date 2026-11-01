import { describeError, describeErrorBare, import_history, onImportDone, onImportProgress } from './api';
import type { HistoryCounts } from './types';

/**
 * The history import outlives any screen: the wizard's step and the Settings screen both read it,
 * so leaving and coming back (or starting in one and viewing in the other) keeps it consistent.
 */
export const importState = $state<{
  importing: boolean;
  progress: { done: number; total: number } | null;
  counts: HistoryCounts | null;
  error: string | null;
  /** Bumped on every `import-done`, so screens can refresh what depends on the import. */
  doneCount: number;
}>({ importing: false, progress: null, counts: null, error: null, doneCount: 0 });

let stopListening: (() => void) | null = null;

/** Subscribes once, for the app's lifetime, to the import events. */
export function ensureImportListeners(): void {
  if (stopListening) return;
  const stopProgress = onImportProgress((p) => {
    importState.progress = p;
  });
  const stopDone = onImportDone((d) => {
    importState.importing = false;
    importState.progress = null;
    importState.counts = d.counts;
    importState.error = d.error ? describeErrorBare(d.error) : null;
    importState.doneCount += 1;
  });
  stopListening = () => {
    stopProgress();
    stopDone();
  };
}

/** Starts the import; resolves with a message when it could not start, `null` otherwise. */
export async function startImport(): Promise<string | null> {
  ensureImportListeners();
  importState.importing = true;
  importState.error = null;
  importState.counts = null;
  importState.progress = { done: 0, total: 0 };
  try {
    await import_history();
    return null;
  } catch (e) {
    importState.importing = false;
    importState.progress = null;
    return describeError(e);
  }
}

/** Tests: forgets any import in flight or finished. Drops the listeners too; the next import screen subscribes again. */
export function resetImportState(): void {
  stopListening?.();
  stopListening = null;
  Object.assign(importState, { importing: false, progress: null, counts: null, error: null, doneCount: 0 });
}
