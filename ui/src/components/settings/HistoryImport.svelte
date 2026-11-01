<script lang="ts">
  import { onMount } from 'svelte';
  import { ensureImportListeners, importState, startImport } from '../../lib/importState.svelte';
  import { t } from '../../lib/i18n';
  import { toast } from '../../lib/toast.svelte';
  import type { OnboardingView, Tool } from '../../lib/types';

  let {
    view,
    summarizeHistory,
    onsummarize,
    provider,
  }: {
    view: OnboardingView;
    summarizeHistory: boolean;
    onsummarize: (v: boolean) => void;
    /** The CLI named in the estimate; defaults to the saved provider (the wizard passes its local choice). */
    provider?: Tool;
  } = $props();

  const TOOL_NAMES: Record<Tool, string> = { claude: 'Claude Code', codex: 'Codex' };

  onMount(ensureImportListeners);

  const counts = $derived(importState.counts ?? (view.history.sessions > 0 ? view.history : null));

  async function importAll() {
    const failure = await startImport();
    if (failure) toast(failure);
  }

  const button =
    'rounded border border-stone-300 px-3 py-1 text-xs text-stone-700 transition-colors hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800';
  const box = 'rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900';
</script>

<button type="button" class={button} disabled={importState.importing} onclick={importAll}>
  {t('onboarding.history.import')}
</button>
{#if importState.importing && importState.progress}
  <div class="space-y-1">
    <progress
      class="w-full"
      value={importState.progress.done}
      max={Math.max(importState.progress.total, 1)}
      aria-label={t('onboarding.history.progress')}
    ></progress>
    <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.history.progress')}</p>
  </div>
{/if}
{#if importState.error}
  <p class="text-red-600 dark:text-red-400">{t('onboarding.history.error', { message: importState.error })}</p>
{:else if counts && !importState.importing}
  <p>{t('onboarding.history.counts', { ...counts })}</p>
{/if}
<div class="{box} space-y-1">
  <label class="flex items-center gap-2">
    <input
      type="checkbox"
      checked={summarizeHistory}
      onchange={(e) => onsummarize(e.currentTarget.checked)}
    />
    {t('onboarding.history.summarize')}
  </label>
  <p class="text-xs text-stone-500 dark:text-stone-400">
    {t('onboarding.history.estimate', { cli: TOOL_NAMES[provider ?? view.provider], count: view.pending_backfill })}
  </p>
</div>
