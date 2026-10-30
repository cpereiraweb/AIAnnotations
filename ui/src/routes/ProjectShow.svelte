<script lang="ts">
  import { onMount } from 'svelte';
  import Pager from '../components/Pager.svelte';
  import RunCardView from '../components/RunCard.svelte';
  import { copyText } from '../lib/actions';
  import {
    describeError,
    onChanged,
    open_project_folder,
    project_detail,
    project_runs,
  } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { getLang, t } from '../lib/i18n';
  import { router, setQuery } from '../lib/router.svelte';
  import { ongoingResumeCommand } from '../lib/shell';
  import { toast } from '../lib/toast.svelte';
  import type { Page, ProjectDetail, RunCard } from '../lib/types';

  let { id }: { id: number } = $props();

  let detail = $state<ProjectDetail | null>(null);
  let missing = $state(false);
  let runs = $state<Page<RunCard> | null>(null);
  let error = $state<string | null>(null);
  let now = $state(Date.now());

  const showHidden = $derived(router.query.get('hidden') === '1');
  const page = $derived(Math.max(1, Number(router.query.get('page')) || 1));

  async function load() {
    try {
      const [d, r] = await Promise.all([project_detail(id), project_runs(id, showHidden, page)]);
      detail = d;
      missing = d === null;
      runs = r;
      error = null;
    } catch (e) {
      error = describeError(e);
    } finally {
      now = Date.now();
    }
  }

  // Reloads when the project, the checkbox or the page change.
  $effect(() => {
    void id;
    void showHidden;
    void page;
    void load();
  });

  onMount(() => onChanged(() => void load()));

  /** Consecutive runs of the same /clear chain (same root) form a group, in page order. */
  interface Group {
    root: number;
    runs: RunCard[];
  }
  const groups = $derived.by(() => {
    const out: Group[] = [];
    const byRoot = new Map<number, Group>();
    for (const r of runs?.items ?? []) {
      let g = byRoot.get(r.root_session_id);
      if (!g) {
        g = { root: r.root_session_id, runs: [] };
        byRoot.set(r.root_session_id, g);
        out.push(g);
      }
      g.runs.push(r);
    }
    return out;
  });

  const isChain = (g: Group) => new Set(g.runs.map((r) => r.session_id)).size > 1;
  function chainHeader(g: Group): string {
    const title = g.runs.find((r) => r.session_id === g.root)?.title;
    return title ? `${t('project.clear_chain')} · ${title}` : t('project.clear_chain');
  }

  async function openFolder() {
    try {
      await open_project_folder(id);
    } catch (e) {
      toast(describeError(e));
    }
  }
</script>

{#if error}
  <p class="text-red-600 dark:text-red-400">{error}</p>
{:else if missing}
  <p class="text-stone-400">{t('project.not_found')}</p>
{:else if detail}
  {@const card = detail.card}
  <header class="mb-5">
    <div class="mb-1 text-xs text-stone-400">
      <a href="#/projects" class="hover:text-stone-600 dark:hover:text-stone-300">{t('projects.title')}</a> ›
    </div>
    <h1 class="text-base font-semibold">{card.name}</h1>
    <p class="font-mono text-[11.5px] text-stone-500 dark:text-stone-400">
      {[card.path, card.remote_url].filter(Boolean).join(' · ')}
    </p>
    {#if card.path}
      <button
        type="button"
        class="mt-1.5 rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
        onclick={openFolder}
      >{t('action.open_folder')}</button>
    {/if}
  </header>

  {#if detail.ongoings.length > 0}
    <section class="mb-6">
      <h2 class="mb-2 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">{t('project.ongoing', { count: detail.ongoings.length })}</h2>
      <ul class="space-y-1.5 rounded-lg border border-stone-200 bg-white px-3 py-2.5 dark:border-stone-800 dark:bg-stone-900">
        {#each detail.ongoings as o (o.file)}
          <li class="flex items-center gap-2">
            <span class="min-w-0 flex-1 truncate text-stone-600 dark:text-stone-300" title={o.file}>{o.title}</span>
            <span class="flex-none text-[11px] text-stone-400">{relativeTime(o.modified_ms, now, getLang())}</span>
            <button
              type="button"
              class="flex-none rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
              title={ongoingResumeCommand(o.project_path, o.name)}
              onclick={() => copyText(ongoingResumeCommand(o.project_path, o.name))}
            >{t('action.resume')}</button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <div class="mb-2 flex items-center justify-between">
    <h2 class="text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">{t('project.sessions')}</h2>
    <label class="flex cursor-pointer items-center gap-1.5 text-xs text-stone-500 dark:text-stone-400">
      <input
        type="checkbox"
        class="accent-stone-600"
        checked={showHidden}
        onchange={(e) => setQuery({ hidden: e.currentTarget.checked ? 1 : null, page: null })}
      />
      {t('project.show_hidden')}
    </label>
  </div>

  {#if runs && runs.items.length === 0}
    <p class="text-stone-400">{t('project.no_sessions')}</p>
  {:else if runs}
    <div class="space-y-2">
      {#each groups as g (g.root)}
        {#if isChain(g)}
          <div class="space-y-2 border-l-2 border-stone-200 pl-3 dark:border-stone-800" data-chain={g.root}>
            <p class="text-[11px] text-stone-400">
              {chainHeader(g)}
            </p>
            {#each g.runs as r (r.run_id)}
              <RunCardView run={r} {now} nested />
            {/each}
          </div>
        {:else}
          {#each g.runs as r (r.run_id)}
            <RunCardView run={r} {now} />
          {/each}
        {/if}
      {/each}
    </div>
    <Pager page={runs.page} pages={runs.pages} onpage={(p) => setQuery({ page: p > 1 ? p : null })} />
  {/if}
{/if}
