<script lang="ts">
  import { openRunFolder, resumeRun, TOOL_LABEL } from '../lib/actions';
  import { endLabel, intervalLabel } from '../lib/format';
  import { getLang, t } from '../lib/i18n';
  import type { RunCard } from '../lib/types';
  import ToolChip from './ToolChip.svelte';

  // `nested`: inside a /clear chain, whose header already carries the title.
  // `showProject`: the Sessions list links each card to its project.
  let {
    run,
    now,
    nested = false,
    showProject = false,
  }: { run: RunCard; now: number; nested?: boolean; showProject?: boolean } = $props();

  const tag =
    'inline-flex items-center rounded-full border border-stone-300/80 px-1.5 text-[11px] leading-[18px] whitespace-nowrap text-stone-600 dark:border-stone-700 dark:text-stone-400';
  const action =
    'rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100';

  const changes = $derived(
    run.changed_files
      ? run.files_count > 0
        ? t('run.changed_files', { count: run.files_count })
        : t('run.changed_files_unknown')
      : t('run.talk'),
  );
</script>

<article
  data-run={run.run_id}
  class="rounded-lg border border-stone-200 bg-white px-3 py-2.5 dark:border-stone-800 dark:bg-stone-900 {run.hidden_label ? 'opacity-70' : ''}"
>
  <div class="flex flex-wrap items-center gap-x-1.5 gap-y-1">
    <ToolChip tool={run.tool} />
    <span class={tag}>{changes}</span>
    {#if run.hidden_label}
      <span class={tag}>{t(`hidden.${run.hidden_label}`)}</span>
    {/if}
    {#if showProject}
      <a href="#/projects/{run.project_id}" class="font-semibold hover:underline">{run.project_name}</a>
      <span class="text-stone-400">·</span>
    {/if}
    <span class="font-semibold">{intervalLabel(run.started_at_ms, run.ended_at_ms, now, getLang())}</span>
    {#if run.branch}
      <span class="text-stone-400">·</span>
      <span class="font-mono text-[11.5px] text-stone-500 dark:text-stone-400">{run.branch}</span>
    {/if}
    <span class="text-stone-400">·</span>
    <span class="text-stone-500 dark:text-stone-400">{endLabel(run)}</span>
  </div>

  {#if run.title && !nested}
    <p class="mt-1 text-stone-500 dark:text-stone-400">{run.title}</p>
  {/if}

  <div class="mt-1.5">
    {#if run.summary}
      <p>{run.summary}</p>
      {#if run.next_step}
        <p class="mt-1 text-stone-500 dark:text-stone-400">{t('run.next_step', { step: run.next_step })}</p>
      {/if}
    {:else if run.summary_status === 'pending'}
      <p class="text-xs text-stone-400 italic">{t('run.summary_pending')}</p>
    {:else if run.summary_status === 'failed'}
      <p class="text-xs text-stone-400 italic">{t('run.summary_failed')}</p>
    {/if}
  </div>

  <div class="mt-2 flex flex-wrap items-center gap-1.5">
    <button type="button" class={action} onclick={() => resumeRun(run.run_id)}>{t('action.resume_tool', { tool: TOOL_LABEL[run.tool] })}</button>
    <button type="button" class={action} onclick={() => openRunFolder(run.run_id)}>{t('action.open_folder')}</button>
  </div>
</article>
