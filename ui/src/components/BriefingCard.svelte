<script lang="ts">
  import { getLang, t } from '../lib/i18n';
  import { briefingTitle } from '../lib/format';
  import type { Briefing } from '../lib/types';

  let {
    briefing,
    generating,
    today,
    onregenerate,
  }: {
    briefing: Briefing | null;
    /** True while a generation runs (backend flag or a click not yet reloaded). */
    generating: boolean;
    /** Local `YYYY-MM-DD`, for the title when there is no briefing row yet. */
    today: string;
    onregenerate: () => void;
  } = $props();

  const showGenerating = $derived(generating || !briefing || briefing.status === 'generating');
</script>

<section class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
  <div class="flex items-baseline justify-between gap-3">
    <h1 class="font-semibold">{briefingTitle(briefing?.date ?? today, getLang())}</h1>
    {#if !showGenerating}
      <button
        type="button"
        onclick={onregenerate}
        class="text-[11px] text-stone-400 transition-colors hover:text-stone-700 dark:hover:text-stone-200"
      >{t('briefing.regenerate')}</button>
    {/if}
  </div>

  {#if showGenerating}
    <p class="mt-1 text-stone-400 italic">{t('briefing.generating')}</p>
  {:else if briefing?.status === 'failed'}
    <p class="mt-1 text-stone-500 dark:text-stone-400">{t('briefing.failed')}</p>
  {:else}
    <p class="mt-1 whitespace-pre-line text-stone-600 dark:text-stone-300">{briefing?.body}</p>
  {/if}
</section>
