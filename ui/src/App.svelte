<script lang="ts">
  // Shell only: the screens and the hash router come with Tasks 7–8. It already exercises the
  // API (mock or Tauri) and reloads on "changed".
  import { onMount } from 'svelte';
  import { describeError, onChanged, today } from './lib/api';
  import { briefingTitle, countsLabel } from './lib/format';
  import { getLang, t } from './lib/i18n';
  import type { TodayView } from './lib/types';

  let view = $state<TodayView | null>(null);
  let error = $state<string | null>(null);
  const lang = getLang();

  async function load() {
    try {
      view = await today();
      error = null;
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    void load();
    return onChanged(() => void load());
  });

  const inboxCount = $derived(view ? view.inbox.recent.reduce((n, g) => n + g.items.length, 0) : 0);
</script>

<main class="mx-auto max-w-5xl px-6 py-8 text-[13px] text-stone-800 dark:text-stone-200">
  {#if error}
    <p class="text-red-600">{error}</p>
  {:else if view}
    {#if view.briefing}
      <h1 class="font-semibold">{briefingTitle(view.briefing.date, lang)}</h1>
    {/if}
    <h2 class="mt-4 text-xs font-semibold tracking-wide text-stone-500 uppercase">
      {t('inbox.title', { count: inboxCount })}
    </h2>
    <ul>
      {#each view.inbox.recent as group (group.project_id)}
        <li><strong>{group.name}</strong> <span class="text-stone-500">{countsLabel(group.counts, lang)}</span></li>
      {/each}
    </ul>
  {/if}
</main>
