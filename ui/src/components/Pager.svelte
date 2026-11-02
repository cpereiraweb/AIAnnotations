<script lang="ts">
  import { t } from '../lib/i18n';

  let { page, pages, onpage }: { page: number; pages: number; onpage: (page: number) => void } = $props();

  const button =
    'rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100';
</script>

{#if pages > 1}
  <nav class="mt-4 flex items-center justify-between text-xs text-stone-500 dark:text-stone-400" aria-label={t('pager.label')}>
    <span class="w-32">
      {#if page > 1}
        <button type="button" class={button} onclick={() => onpage(page - 1)}>{t('pager.newer')}</button>
      {/if}
    </span>
    <span>{t('pager.page', { page, pages })}</span>
    <span class="w-32 text-right">
      {#if page < pages}
        <button type="button" class={button} onclick={() => onpage(page + 1)}>{t('pager.older')}</button>
      {/if}
    </span>
  </nav>
{/if}
