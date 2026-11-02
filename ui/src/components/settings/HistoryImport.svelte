<script lang="ts">
  import { onMount } from 'svelte';
  import { ensureImportListeners, importState, startImport } from '../../lib/importState.svelte';
  import { TOOL_NAMES } from '../../lib/tools';
  import { box, button } from '../../lib/ui-classes';
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


  onMount(ensureImportListeners);

  const counts = $derived(importState.counts ?? (view.history.sessions > 0 ? view.history : null));

  async function importAll() {
    const failure = await startImport();
    if (failure) toast(failure);
  }

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
