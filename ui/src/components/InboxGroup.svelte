<script lang="ts">
  import { getLang, t } from '../lib/i18n';
  import { countsLabel } from '../lib/format';
  import type { InboxGroup, InboxItem, RunCard } from '../lib/types';
  import InboxItemCard from './InboxItem.svelte';

  let {
    group,
    runs,
    ondismiss,
    ondismissall,
  }: {
    group: InboxGroup;
    /** Last run of every session, by `session_id`. */
    runs: Record<string, RunCard>;
    ondismiss: (item: InboxItem) => void;
    ondismissall: (group: InboxGroup) => void;
  } = $props();

  // The backend decides which group opens first; later it is the user's toggle.
  let open = $state<boolean | null>(null);
  const isOpen = $derived(open ?? group.expanded);
</script>

<div
  data-inbox-group={group.expanded ? 'expanded' : 'collapsed'}
  class="rounded-lg border border-stone-200 bg-stone-100/60 dark:border-stone-800 dark:bg-stone-900/40"
>
  <div class="flex items-center gap-2 px-3 py-2">
    <button
      type="button"
      onclick={() => (open = !isOpen)}
      aria-expanded={isOpen}
      class="flex min-w-0 flex-1 items-baseline gap-2 text-left"
    >
      <span class="w-3 flex-none text-[10px] text-stone-400 transition-transform {isOpen ? 'rotate-90' : ''}" aria-hidden="true">▶&#xFE0E;</span>
      <span class="truncate font-semibold">{group.name}</span>
      <span class="truncate text-[12px] text-stone-500 dark:text-stone-400">{countsLabel(group.counts, getLang())}</span>
    </button>
    <button
      type="button"
      onclick={() => ondismissall(group)}
      class="flex-none rounded px-2 py-px text-[11px] text-stone-400 transition-colors hover:bg-stone-200 hover:text-stone-700 dark:hover:bg-stone-800 dark:hover:text-stone-200"
    >{t('inbox.dismiss_all')}</button>
  </div>

  {#if isOpen}
    <div class="space-y-2 px-3 pb-3">
      {#each group.items as item (item.key)}
        <InboxItemCard {item} run={item.session_id === null ? null : (runs[String(item.session_id)] ?? null)} {ondismiss} />
      {/each}
    </div>
  {/if}
</div>
