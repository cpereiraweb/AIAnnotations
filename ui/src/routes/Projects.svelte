<script lang="ts">
  import { onMount } from 'svelte';
  import OpenWith from '../components/OpenWith.svelte';
  import ToolChip from '../components/ToolChip.svelte';
  import { describeError, onChanged, projects } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { getLang, t } from '../lib/i18n';
  import type { ProjectCard } from '../lib/types';

  let cards = $state<ProjectCard[] | null>(null);
  let error = $state<string | null>(null);
  let now = $state(Date.now());

  async function load() {
    try {
      cards = await projects();
      error = null;
    } catch (e) {
      error = describeError(e);
    } finally {
      now = Date.now();
    }
  }

  onMount(() => {
    void load();
    return onChanged(() => void load());
  });
</script>

<h1 class="mb-4 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">{t('projects.title')}</h1>

{#if error}
  <p class="text-red-600 dark:text-red-400">{error}</p>
{:else if cards === null}
  <!-- loading -->
{:else if cards.length === 0}
  <p class="text-stone-400">{t('projects.empty')}</p>
{:else}
  <div class="grid gap-2 sm:grid-cols-2">
    {#each cards as card (card.id)}
      <div
        data-project={card.id}
        class="relative block rounded-lg border border-stone-200 bg-white px-3 py-2.5 transition-colors hover:border-stone-300 dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-700"
      >
        <div class="flex items-baseline justify-between gap-2">
          <span class="flex min-w-0 items-baseline gap-1.5">
            <a href="#/projects/{card.id}" class="truncate font-semibold after:absolute after:inset-0">{card.name}</a>
            {#if card.ongoing_count > 0}
              <span
                class="flex-none text-[11px] text-amber-700 dark:text-amber-300"
                title={t('projects.ongoing_count', { count: card.ongoing_count })}
              >🔄 {card.ongoing_count}</span>
            {/if}
          </span>
          {#if card.last_activity_ms !== null}
            <span class="flex-none text-xs text-stone-400">{relativeTime(card.last_activity_ms, now, getLang())}</span>
          {/if}
        </div>
        <div class="flex items-center justify-between gap-2">
          <span class="truncate font-mono text-[11.5px] text-stone-500 dark:text-stone-400">{card.path ?? ''}</span>
          {#if card.path}
            <OpenWith projectId={card.id} compact />
          {/if}
        </div>
        {#if card.remote_url}
          <div class="truncate font-mono text-[11.5px] text-stone-400 dark:text-stone-500">{card.remote_url}</div>
        {/if}

        {#if card.tools_14d.length > 0 || card.last_run?.branch}
          <div class="mt-1.5 flex flex-wrap items-center gap-1">
            {#each card.tools_14d as usage (usage.tool)}
              <ToolChip tool={usage.tool} changed={usage.changed_files} />
            {/each}
            {#if card.last_run?.branch}
              <span class="font-mono text-[11.5px]">{card.last_run.branch}</span>
            {/if}
          </div>
        {/if}

        {#if card.last_run?.summary}
          <p class="mt-1.5 line-clamp-2 text-stone-600 dark:text-stone-300">{card.last_run.summary}</p>
        {/if}
      </div>
    {/each}
  </div>
{/if}
