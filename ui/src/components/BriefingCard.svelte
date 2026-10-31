<script lang="ts">
  import { getLang, t } from '../lib/i18n';
  import { briefingTitle } from '../lib/format';
  import type { Briefing } from '../lib/types';

  let {
    briefing,
    generating,
    paused,
    startFailed,
    today,
    onregenerate,
  }: {
    briefing: Briefing | null;
    /** True while a generation runs (backend flag or a click not yet reloaded). */
    generating: boolean;
    /** "Pausar resumos" is on: the screen does not start a `generating` briefing. */
    paused: boolean;
    /** The app's automatic start failed and left the record `generating`. */
    startFailed: boolean;
    /** Local `YYYY-MM-DD`, for the title when there is no briefing row yet. */
    today: string;
    onregenerate: () => void;
  } = $props();

  // A `generating` row nobody works on: say why instead of "gerando…" forever.
  const stuck = $derived(!generating && briefing?.status === 'generating' && (startFailed || paused));
  const showGenerating = $derived(
    !stuck && (generating || !briefing || briefing.status === 'generating'),
  );
  // A `generating` row nothing is working on can still be regenerated.
  const canRegenerate = $derived(!generating && briefing !== null);
</script>

<section class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
  <div class="flex items-baseline justify-between gap-3">
    <h1 class="font-semibold">{briefingTitle(briefing?.date ?? today, getLang())}</h1>
    {#if canRegenerate}
      <button
        type="button"
        onclick={onregenerate}
        class="text-[11px] text-stone-400 transition-colors hover:text-stone-700 dark:hover:text-stone-200"
      >{t('briefing.regenerate')}</button>
    {/if}
  </div>

  {#if showGenerating}
    <p class="mt-1 text-stone-400 italic">{t('briefing.generating')}</p>
  {:else if stuck && !startFailed}
    <p class="mt-1 text-stone-500 dark:text-stone-400">{t('briefing.paused')}</p>
  {:else if briefing?.status === 'failed' || stuck}
    <p class="mt-1 text-stone-500 dark:text-stone-400">{t('briefing.failed')}</p>
  {:else}
    <p class="mt-1 whitespace-pre-line text-stone-600 dark:text-stone-300">{briefing?.body}</p>
  {/if}
</section>
