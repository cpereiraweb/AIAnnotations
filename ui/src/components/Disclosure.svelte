<script lang="ts">
  import type { Snippet } from 'svelte';

  // Collapsed-by-default section ("Parados há mais de 30 dias", "Parado há mais tempo").
  let { title, children }: { title: string; children: Snippet } = $props();
  let open = $state(false);
</script>

<div class="mt-3">
  <button
    type="button"
    onclick={() => (open = !open)}
    aria-expanded={open}
    class="flex items-baseline gap-2 text-xs font-semibold tracking-wide text-stone-400 uppercase transition-colors hover:text-stone-600 dark:hover:text-stone-300"
  >
    <span class="w-3 text-[10px] transition-transform {open ? 'rotate-90' : ''}" aria-hidden="true">▶&#xFE0E;</span>
    {title}
  </button>
  {#if open}
    <div class="mt-2">{@render children()}</div>
  {/if}
</div>
