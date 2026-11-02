<script lang="ts">
  import { onMount } from 'svelte';
  import { openProjectWith } from '../lib/actions';
  import { appsState, loadApps } from '../lib/apps.svelte';
  import { t } from '../lib/i18n';

  let { projectId, compact = false }: { projectId: number; compact?: boolean } = $props();

  const view = $derived(appsState.view);
  const defaultEditor = $derived(view?.editors.find((e) => e.id === view.editor) ?? null);
  const otherEditors = $derived(view?.editors.filter((e) => e.id !== view.editor) ?? []);

  onMount(() => {
    if (appsState.view === null) void loadApps();
  });

  async function pick(e: Event & { currentTarget: HTMLSelectElement }) {
    const select = e.currentTarget;
    const id = select.value;
    // Back to the placeholder right away: the select is a menu, not a state.
    select.value = '';
    if (id) await openProjectWith(projectId, id);
  }

  const size = $derived(compact ? 'px-1 text-[11px]' : 'px-2 py-px text-[11px]');
  const control =
    'rounded border border-stone-300 text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:bg-transparent dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100';
</script>

<span class="relative z-10 inline-flex flex-none items-center gap-1">
  <button
    type="button"
    class="{control} {size} disabled:opacity-50"
    disabled={defaultEditor === null}
    onclick={() => openProjectWith(projectId, null)}
  >{defaultEditor ? t('open_with.default', { editor: defaultEditor.label }) : t('open_with.open')}</button>
  {#if view && (otherEditors.length > 0 || view.terminals.length > 0)}
    <select
      class="{control} {size}"
      aria-label={t('open_with.more')}
      onchange={pick}
    >
      <option value="">{t('open_with.more')}</option>
      {#if otherEditors.length > 0}
        <optgroup label={t('open_with.editors')}>
          {#each otherEditors as a (a.id)}<option value={a.id}>{a.label}</option>{/each}
        </optgroup>
      {/if}
      {#if view.terminals.length > 0}
        <optgroup label={t('open_with.terminals')}>
          {#each view.terminals as a (a.id)}<option value={a.id}>{a.label}</option>{/each}
        </optgroup>
      {/if}
    </select>
  {/if}
</span>
