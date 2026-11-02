<script lang="ts">
  import { onMount } from 'svelte';
  import Pager from '../components/Pager.svelte';
  import RunCardView from '../components/RunCard.svelte';
  import { describeError, onChanged, projects, sessions } from '../lib/api';
  import { t } from '../lib/i18n';
  import { router, setQuery } from '../lib/router.svelte';
  import type { Page, ProjectCard, RunCard, SessionFilter } from '../lib/types';

  let list = $state<Page<RunCard> | null>(null);
  let projectList = $state<ProjectCard[]>([]);
  let error = $state<string | null>(null);
  let now = $state(Date.now());

  const tool = $derived((['claude', 'codex'] as const).find((x) => x === router.query.get('tool')) ?? null);
  const kind = $derived((['changed', 'talk'] as const).find((x) => x === router.query.get('kind')) ?? null);
  const projectId = $derived(Number(router.query.get('project')) || null);
  const page = $derived(Math.max(1, Number(router.query.get('page')) || 1));

  async function load() {
    const filter: SessionFilter = { tool, project_id: projectId, kind };
    try {
      list = await sessions(filter, page);
      error = null;
    } catch (e) {
      error = describeError(e);
    } finally {
      now = Date.now();
    }
  }

  // Reloads whenever a filter or the page (all in the hash) change.
  $effect(() => {
    void tool;
    void kind;
    void projectId;
    void page;
    void load();
  });

  onMount(() => {
    projects().then((p) => (projectList = p), () => {});
    return onChanged(() => void load());
  });

  // A filter change goes back to the first page.
  const filterChange = (key: 'tool' | 'project' | 'kind') => (e: Event) =>
    setQuery({ [key]: (e.currentTarget as HTMLSelectElement).value, page: null });

  const select =
    'rounded border border-stone-300 bg-white px-1.5 py-0.5 text-xs text-stone-700 dark:border-stone-700 dark:bg-stone-900 dark:text-stone-300';
</script>

<h1 class="mb-3 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">{t('sessions.title')}</h1>

<div class="mb-4 flex flex-wrap gap-2">
  <select class={select} aria-label={t('sessions.tool')} value={tool ?? ''} onchange={filterChange('tool')}>
    <option value="">{t('sessions.all_tools')}</option>
    <option value="claude">Claude</option>
    <option value="codex">Codex</option>
  </select>
  <select class={select} aria-label={t('sessions.project')} value={String(projectId ?? '')} onchange={filterChange('project')}>
    <option value="">{t('sessions.all_projects')}</option>
    {#each projectList as p (p.id)}
      <option value={String(p.id)}>{p.name}</option>
    {/each}
  </select>
  <select class={select} aria-label={t('sessions.kind')} value={kind ?? ''} onchange={filterChange('kind')}>
    <option value="">{t('sessions.all_kinds')}</option>
    <option value="changed">{t('sessions.changed')}</option>
    <option value="talk">{t('sessions.talk')}</option>
  </select>
</div>

{#if error}
  <p class="text-red-600 dark:text-red-400">{error}</p>
{:else if list && list.items.length === 0}
  <p class="text-stone-400">{t('sessions.empty')}</p>
{:else if list}
  <div class="space-y-2">
    {#each list.items as r (r.run_id)}
      <RunCardView run={r} {now} showProject />
    {/each}
  </div>
  <Pager page={list.page} pages={list.pages} onpage={(p) => setQuery({ page: p > 1 ? p : null })} />
{/if}
