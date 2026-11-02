<script lang="ts">
  import { TOOL_LABEL } from '../lib/actions';
  import type { Tool } from '../lib/types';

  /** `changed`: adds ✎ (changed files) or 💬 (talk only); `null`/`undefined` adds nothing. */
  let { tool, changed = null }: { tool: Tool; changed?: boolean | null } = $props();

  const label = $derived(TOOL_LABEL[tool] + (changed == null ? '' : changed ? ' ✎' : ' 💬'));
</script>

<span
  class="inline-flex items-center rounded-full border px-1.5 text-[11px] leading-[18px] whitespace-nowrap {tool === 'claude'
    ? 'border-orange-300/70 bg-orange-50 text-orange-800 dark:border-orange-400/30 dark:bg-orange-400/10 dark:text-orange-200'
    : 'border-sky-300/70 bg-sky-50 text-sky-800 dark:border-sky-400/30 dark:bg-sky-400/10 dark:text-sky-200'}"
>{label}</span>
