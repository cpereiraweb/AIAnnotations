<script lang="ts">
  import { t } from '../lib/i18n';
  import type { RunCard } from '../lib/types';
  import ToolChip from './ToolChip.svelte';

  let { run }: { run: RunCard } = $props();
</script>

{#if run.summary}
  <p class="mt-1.5"><ToolChip tool={run.tool} changed={run.changed_files} /> {run.summary}</p>
{:else if run.summary_status === 'pending'}
  <p class="mt-1.5 text-stone-400 italic">{t('run.summary_pending')}</p>
{:else if run.summary_status === 'failed'}
  <p class="mt-1.5 text-stone-400 italic">{t('run.summary_failed')}</p>
{/if}
