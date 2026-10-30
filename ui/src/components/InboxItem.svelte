<script lang="ts">
  import { openRunFolder, resumeRun, TOOL_LABEL } from '../lib/actions';
  import { t } from '../lib/i18n';
  import type { InboxItem, RunCard } from '../lib/types';
  import RunSummary from './RunSummary.svelte';

  let {
    item,
    run,
    ondismiss,
  }: { item: InboxItem; run: RunCard | null; ondismiss: (item: InboxItem) => void } = $props();

  const warn = $derived(['interrupted', 'stale_branch', 'dirty', 'worktree'].includes(item.kind));
  const badge = $derived(t(`badge.${item.badge}`, { count: item.age_days ?? 0 }));
  // `detail` is data (worktree path, project name); a branch without merge says how far ahead it is.
  const detail = $derived(
    item.kind === 'stale_branch' && item.ahead !== null ? t('inbox.ahead', { count: item.ahead }) : item.detail,
  );
</script>

<article class="rounded-lg border border-stone-200 bg-white px-3 py-2.5 dark:border-stone-800 dark:bg-stone-900">
  <div class="flex flex-wrap items-center gap-x-1.5 gap-y-1">
    <span
      class="inline-flex items-center rounded-full border px-1.5 text-[11px] leading-[18px] whitespace-nowrap {warn
        ? 'border-amber-300/80 bg-amber-50 text-amber-800 dark:border-amber-400/30 dark:bg-amber-400/10 dark:text-amber-200'
        : 'border-stone-300/80 text-stone-600 dark:border-stone-700 dark:text-stone-400'}">{badge}</span>
    <a href="#/projects/{item.project_id}" class="font-semibold hover:underline">{item.title}</a>
    {#if detail}
      <span class="text-stone-400">·</span>
      <span class="text-stone-500 dark:text-stone-400">{detail}</span>
    {/if}
  </div>

  {#if run}
    <RunSummary {run} />
  {/if}

  <div class="mt-2 flex flex-wrap items-center gap-1.5">
    {#if run}
      <button
        type="button"
        class="rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
        onclick={() => resumeRun(run.run_id)}
      >{t('action.resume_tool', { tool: TOOL_LABEL[run.tool] })}</button>
      <button
        type="button"
        class="rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
        onclick={() => openRunFolder(run.run_id)}
      >{t('action.open_folder')}</button>
    {/if}
    <button
      type="button"
      class="rounded px-2 py-px text-[11px] text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-700 dark:hover:bg-stone-800 dark:hover:text-stone-200"
      onclick={() => ondismiss(item)}
    >{t('inbox.dismiss')}</button>
  </div>
</article>
