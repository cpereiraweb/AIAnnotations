<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { describeError, search } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { getLang, t } from '../lib/i18n';
  import type { SearchHit } from '../lib/types';
  import ToolChip from './ToolChip.svelte';

  const DEBOUNCE_MS = 250;

  let open = $state(false);
  let text = $state('');
  let hits = $state<SearchHit[]>([]);
  let searched = $state(false);
  let error = $state<string | null>(null);
  let active = $state(0);
  let now = $state(Date.now());
  let input = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Answers of older searches are dropped.
  let seq = 0;

  async function show() {
    open = true;
    now = Date.now();
    await tick();
    input?.focus();
  }

  function close() {
    open = false;
    clearTimeout(timer);
    seq++;
    text = '';
    hits = [];
    searched = false;
    error = null;
    active = 0;
  }

  async function run(query: string) {
    const mine = ++seq;
    try {
      const found = await search(query);
      if (mine !== seq) return;
      hits = found;
      error = null;
    } catch (e) {
      if (mine !== seq) return;
      hits = [];
      error = describeError(e);
    }
    searched = true;
    active = 0;
    now = Date.now();
  }

  function oninput() {
    clearTimeout(timer);
    const query = text.trim();
    if (!query) {
      seq++;
      hits = [];
      searched = false;
      return;
    }
    timer = setTimeout(() => void run(query), DEBOUNCE_MS);
  }

  function go(hit: SearchHit) {
    location.hash = `#/projects/${hit.project_id}`;
    close();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' && hits.length) {
      e.preventDefault();
      active = (active + 1) % hits.length;
    } else if (e.key === 'ArrowUp' && hits.length) {
      e.preventDefault();
      active = (active - 1 + hits.length) % hits.length;
    } else if (e.key === 'Enter' && hits[active]) {
      e.preventDefault();
      go(hits[active]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  onMount(() => {
    const onGlobalKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (open) close();
        else void show();
      }
    };
    const onOpen = () => void show();
    window.addEventListener('keydown', onGlobalKey);
    window.addEventListener('open-palette', onOpen);
    return () => {
      window.removeEventListener('keydown', onGlobalKey);
      window.removeEventListener('open-palette', onOpen);
      clearTimeout(timer);
    };
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-stone-900/30 px-4 pt-[15vh]" onclick={close} data-palette-backdrop>
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('search.label')}
      tabindex="-1"
      class="w-full max-w-xl overflow-hidden rounded-lg border border-stone-200 bg-white shadow-xl dark:border-stone-700 dark:bg-stone-900"
      onclick={(e) => e.stopPropagation()}
      {onkeydown}
    >
      <input
        bind:this={input}
        bind:value={text}
        {oninput}
        type="text"
        role="combobox"
        aria-expanded={hits.length > 0}
        aria-controls="palette-results"
        aria-label={t('search.label')}
        placeholder={t('search.placeholder')}
        class="w-full border-b border-stone-200 bg-transparent px-3 py-2.5 text-sm outline-none placeholder:text-stone-400 dark:border-stone-700"
      />
      <ul id="palette-results" role="listbox" class="max-h-80 overflow-y-auto">
        {#each hits as hit, i (hit.run_id)}
          <li role="option" aria-selected={i === active}>
            <button
              type="button"
              class="block w-full px-3 py-2 text-left {i === active ? 'bg-stone-100 dark:bg-stone-800' : ''}"
              onclick={() => go(hit)}
              onmousemove={() => (active = i)}
            >
              <span class="flex flex-wrap items-center gap-x-1.5">
                <ToolChip tool={hit.tool} />
                <span class="font-semibold">{hit.project_name}</span>
                {#if hit.branch}
                  <span class="text-stone-400">·</span>
                  <span class="font-mono text-[11.5px] text-stone-500 dark:text-stone-400">{hit.branch}</span>
                {/if}
                <span class="text-stone-400">·</span>
                <span class="text-xs text-stone-400">{relativeTime(hit.started_at_ms, now, getLang())}</span>
              </span>
              {#if hit.summary}
                <span class="mt-0.5 line-clamp-2 block text-stone-600 dark:text-stone-300">{hit.summary}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
      {#if error}
        <p class="px-3 py-2 text-red-600 dark:text-red-400">{error}</p>
      {:else if searched && hits.length === 0}
        <p class="px-3 py-2 text-stone-400">{t('search.empty')}</p>
      {/if}
    </div>
  </div>
{/if}
